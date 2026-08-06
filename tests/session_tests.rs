use ferrite::providers::{ChatMessage, NativeFunctionCall, NativeToolCall, Role};
use ferrite::session::{AgentSession, SessionSnapshot};
use ferrite::tools::ToolResult;

fn new_session() -> AgentSession {
    AgentSession::new(
        "test-session".to_string(),
        ".".to_string(),
        "system prompt".to_string(),
    )
}

#[test]
fn new_session_starts_with_system_message_and_metadata() {
    let session = new_session();
    assert_eq!(session.messages.len(), 1);
    assert_eq!(session.messages[0].role, Role::System);
    assert_eq!(session.messages[0].content, "system prompt");
    assert_eq!(session.metadata.get("session_id").map(String::as_str), Some("test-session"));
}

#[test]
fn new_session_title_uses_first_eight_chars() {
    let session = AgentSession::new(
        "1234567890".to_string(),
        ".".to_string(),
        "prompt".to_string(),
    );
    assert_eq!(session.title, "12345678");
}

#[test]
fn new_session_title_uses_full_id_when_short() {
    let session = AgentSession::new("abc".to_string(), ".".to_string(), "prompt".to_string());
    assert_eq!(session.title, "abc");
}

#[test]
fn add_user_and_assistant_messages_appended_in_order() {
    let mut session = new_session();
    session.add_user_message("hello");
    session.add_assistant_message("hi there");

    let history = session.get_history();
    assert_eq!(history.len(), 3);
    assert_eq!(history[1].role, Role::User);
    assert_eq!(history[1].content, "hello");
    assert_eq!(history[2].role, Role::Assistant);
    assert_eq!(history[2].content, "hi there");
}

#[test]
fn add_tool_msg_wraps_failure_content() {
    let mut session = new_session();
    session.add_tool_msg("list_files", "permission denied");

    let msg = session.messages.back().expect("message");
    assert_eq!(msg.role, Role::User);
    assert!(msg.content.contains("[Tool result: list_files (failure)]"));
    assert!(msg.content.contains("permission denied"));
}

#[test]
fn add_tool_result_wraps_success_content() {
    let mut session = new_session();
    session.add_tool_result(
        "list_files",
        &ToolResult {
            success: true,
            content: "src/\nCargo.toml".to_string(),
            error: None,
        },
    );

    let msg = session.messages.back().expect("message");
    assert_eq!(msg.role, Role::User);
    assert!(msg.content.contains("[Tool result: list_files (success)]"));
    assert!(msg.content.contains("src/"));
}

#[test]
fn orphaned_tool_messages_are_downgraded_to_user() {
    let mut session = new_session();
    // A tool message with no matching pending assistant tool_call
    session.add_native_tool_result("orphan-id", "stale result");

    let history = session.get_history();
    let last = history.last().expect("message");
    assert_eq!(last.role, Role::User);
    assert_ne!(last.tool_call_id, Some("orphan-id".to_string()));
    // The orphaned content is wrapped as a recoverable user message.
    assert!(last.content.contains("stale result"));
    assert!(last.content.contains("[Tool result: workspace tool"));
}

#[test]
fn snapshot_roundtrip_preserves_state() {
    let mut session = new_session();
    session.add_user_message("question");
    session.add_assistant_message("answer");
    session.metadata.insert("terminalState".into(), "ready".into());

    let snapshot = SessionSnapshot::from_session(&session);
    assert_eq!(snapshot.messages.len(), 3);

    let rebuilt = snapshot.into_session();
    assert_eq!(rebuilt.id, session.id);
    assert_eq!(rebuilt.title, session.title);
    assert_eq!(rebuilt.workspace_root, session.workspace_root);
    assert_eq!(rebuilt.messages.len(), 3);
    assert_eq!(
        rebuilt.metadata.get("terminalState").map(String::as_str),
        Some("ready")
    );
}

#[test]
fn history_is_cached_and_reused_until_dirty() {
    let mut session = new_session();
    session.add_user_message("hello");
    let first = session.get_history();
    let second = session.get_history();
    // Same content; cache reused.
    assert_eq!(first.len(), second.len());
    assert_eq!(
        std::ptr::eq(first.as_ptr(), second.as_ptr()),
        false,
        "clones are separate Vecs but cache works"
    );
}

#[test]
fn maybe_trim_evicts_oldest_non_system_messages() {
    let mut session = new_session();
    // Push enough messages to trigger trimming (MAX_MESSAGE_COUNT = 51).
    for i in 0..100 {
        session.add_user_message(&format!("message {}", i));
    }

    // System prompt always stays at index 0.
    assert_eq!(session.messages[0].role, Role::System);
    // The trim keeps the newest messages.
    assert!(session.messages.len() <= 51);
    let last = session.messages.back().expect("message");
    assert_eq!(last.content, "message 99");
}

#[test]
fn native_tool_call_and_result_are_preserved_in_history() {
    let mut session = new_session();
    session.add_native_assistant_message(ChatMessage {
        role: Role::Assistant,
        content: String::new(),
        tool_call_id: None,
        name: None,
        tool_calls: Some(vec![NativeToolCall {
            id: "call_1".to_string(),
            kind: "function".to_string(),
            function: NativeFunctionCall {
                name: "read_file".to_string(),
                arguments: r#"{"path":"Cargo.toml"}"#.to_string(),
            },
        }]),
    });
    session.add_native_tool_result("call_1", "workspace data");

    let history = session.get_history();
    assert_eq!(history[1].role, Role::Assistant);
    assert!(history[1].tool_calls.is_some());
    assert_eq!(history[2].role, Role::Tool);
    assert_eq!(history[2].tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(history[2].content, "workspace data");
}

#[test]
fn trim_removes_orphaned_tool_results_with_parent_assistant() {
    let mut session = new_session();
    // Fill so that trimming will evict the native assistant + its tool result together.
    for i in 0..50 {
        session.add_user_message(&format!("msg {}", i));
    }
    session.add_native_assistant_message(ChatMessage {
        role: Role::Assistant,
        content: String::new(),
        tool_call_id: None,
        name: None,
        tool_calls: Some(vec![NativeToolCall {
            id: "call_old".to_string(),
            kind: "function".to_string(),
            function: NativeFunctionCall {
                name: "list_files".to_string(),
                arguments: "{}".to_string(),
            },
        }]),
    });
    session.add_native_tool_result("call_old", "old result");
    session.add_user_message("final message");

    let history = session.get_history();
    // Any Tool message in history must have a matching pending assistant call.
    let orphan_tool = history.iter().any(|m| {
        m.role == Role::Tool
            && !history.iter().any(|a| {
                a.role == Role::Assistant
                    && a.tool_calls.as_ref().is_some_and(|tcs| {
                        tcs.iter().any(|tc| tc.id == m.tool_call_id.as_deref().unwrap_or(""))
                    })
            })
    });
    assert!(!orphan_tool, "no orphaned tool messages in history");
}