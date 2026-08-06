use ferrite::providers::{ChatMessage, NativeFunctionCall, NativeToolCall, Role};
use ferrite::session::AgentSession;
use ferrite::tools::ToolResult;

#[test]
fn xml_tool_results_are_sent_as_user_messages() {
    let mut session = AgentSession::new(
        "session".to_string(),
        ".".to_string(),
        "system prompt".to_string(),
    );
    session.add_tool_result(
        "list_files",
        &ToolResult {
            success: true,
            content: "src/\nCargo.toml".to_string(),
            error: None,
        },
    );

    let result = session.get_history().pop().expect("tool result message");
    assert_eq!(result.role, Role::User);
    assert_eq!(result.tool_call_id, None);
    assert_eq!(result.name, None);
    assert!(result.content.contains("[Tool result: list_files (success)]"));
    assert!(result.content.contains("src/"));
}

#[test]
fn legacy_tool_messages_are_normalized_before_api_requests() {
    let mut session = AgentSession::new(
        "session".to_string(),
        ".".to_string(),
        "system prompt".to_string(),
    );
    session.messages.push_back(ChatMessage {
        role: Role::Tool,
        content: "legacy result".to_string(),
        tool_call_id: Some("legacy-id".to_string()),
        name: Some("list_files".to_string()),
        tool_calls: None,
    });

    let result = session.get_history().pop().expect("normalized message");
    assert_eq!(result.role, Role::User);
    assert_eq!(result.tool_call_id, None);
    assert_eq!(result.name, None);
    assert!(result.content.contains("legacy result"));
}

#[test]
fn matching_native_tool_results_are_kept_for_api_requests() {
    let mut session = AgentSession::new(
        "session".to_string(),
        ".".to_string(),
        "system prompt".to_string(),
    );
    session.add_native_assistant_message(ChatMessage {
        role: Role::Assistant,
        content: String::new(),
        tool_call_id: None,
        name: None,
        tool_calls: Some(vec![NativeToolCall {
            id: "call_1".to_string(),
            kind: "function".to_string(),
            function: NativeFunctionCall {
                name: "list_files".to_string(),
                arguments: r#"{"path":"."}"#.to_string(),
            },
        }]),
    });
    session.add_native_tool_result("call_1", "Cargo.toml\nsrc/");

    let history = session.get_history();
    let assistant = &history[1];
    let tool_result = &history[2];

    assert_eq!(assistant.role, Role::Assistant);
    assert_eq!(assistant.tool_calls.as_ref().map(Vec::len), Some(1));
    assert_eq!(tool_result.role, Role::Tool);
    assert_eq!(tool_result.tool_call_id.as_deref(), Some("call_1"));
    assert_eq!(tool_result.content, "Cargo.toml\nsrc/");
}
