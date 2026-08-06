use ferrite::config::Config;
use ferrite::providers::{ChatMessage, Role};
use ferrite::session::{AgentSession, SessionInfo, SessionSnapshot};
use serde_json::json;

/// Build a session with realistic content for persistence tests.
fn build_sample_session() -> AgentSession {
    let mut session = AgentSession::new(
        "test-session-id".to_string(),
        "/tmp/workspace".to_string(),
        "system prompt".to_string(),
    );
    session.add_user_message("請幫我 refactor 這個專案");
    session.add_assistant_message("好的，我來分析一下。");
    session.metadata.insert("terminalState".into(), "ready".into());
    session.metadata.insert("customKey".into(), "customValue".into());
    session
}

#[test]
fn session_snapshot_serializes_and_restores_complete_state() {
    let session = build_sample_session();
    let snapshot = SessionSnapshot::from_session(&session);

    // Serialize to JSON and back (simulates disk persistence).
    let json = serde_json::to_string_pretty(&snapshot).expect("serialize snapshot");
    let restored: SessionSnapshot = serde_json::from_str(&json).expect("deserialize snapshot");

    assert_eq!(restored.id, "test-session-id");
    assert_eq!(restored.title, session.title);
    assert_eq!(restored.workspace_root, "/tmp/workspace");
    assert_eq!(restored.messages.len(), 3);
    assert_eq!(restored.messages[0].role, Role::System);
    assert_eq!(restored.messages[1].role, Role::User);
    assert_eq!(
        restored.metadata.get("terminalState").map(String::as_str),
        Some("ready")
    );
    assert_eq!(
        restored.metadata.get("customKey").map(String::as_str),
        Some("customValue")
    );
}

#[test]
fn snapshot_into_session_rebuilds_history_with_normalization() {
    let session = build_sample_session();
    let snapshot = SessionSnapshot::from_session(&session);
    let rebuilt = snapshot.into_session();

    let history = rebuilt.get_history();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0].role, Role::System);
    assert_eq!(history[0].content, "system prompt");
    assert_eq!(history[1].role, Role::User);
    assert_eq!(history[1].content, "請幫我 refactor 這個專案");
    assert_eq!(history[2].role, Role::Assistant);
    assert_eq!(history[2].content, "好的，我來分析一下。");
}

#[test]
fn session_info_serializes_for_frontend() {
    let info = SessionInfo {
        id: "abc-123".to_string(),
        title: "my session".to_string(),
    };
    let json = serde_json::to_value(&info).expect("serialize");
    assert_eq!(json["id"], "abc-123");
    assert_eq!(json["title"], "my session");
}

#[test]
fn config_default_works_for_session_creation() {
    // CodingAgent creation requires a valid provider; ollama needs no API key.
    let mut config = Config::default();
    config.provider = "ollama".to_string();
    config.endpoint = "http://localhost:11434".to_string();
    let _ = config.validate().expect("ollama config is valid");
}

#[test]
fn chat_message_roundtrip_preserves_native_tool_calls() {
    let message = ChatMessage {
        role: Role::Assistant,
        content: String::new(),
        tool_call_id: None,
        name: None,
        tool_calls: Some(vec![ferrite::providers::NativeToolCall {
            id: "call_42".to_string(),
            kind: "function".to_string(),
            function: ferrite::providers::NativeFunctionCall {
                name: "list_files".to_string(),
                arguments: r#"{"path":"."}"#.to_string(),
            },
        }]),
    };

    let json = serde_json::to_value(&message).expect("serialize");
    let restored: ChatMessage = serde_json::from_value(json).expect("deserialize");
    let tool_calls = restored.tool_calls.expect("tool calls present");
    assert_eq!(tool_calls[0].id, "call_42");
    assert_eq!(tool_calls[0].function.name, "list_files");
}

#[test]
fn agent_response_serializes_for_rpc_output() {
    let response = ferrite::agent::AgentResponse {
        content: "generated text".to_string(),
        session_id: "sess-1".to_string(),
        model: "deepseek-chat".to_string(),
    };
    let value = serde_json::to_value(&response).expect("serialize");
    assert_eq!(value["content"], "generated text");
    assert_eq!(value["session_id"], "sess-1");
    assert_eq!(value["model"], "deepseek-chat");
}

/// Guard: project context builder works with a real workspace root.
#[test]
fn project_context_builds_for_existing_directory() {
    let ctx = ferrite::context::build_project_context(".");
    assert!(ctx.contains("工作區根目錄"));
    assert!(ctx.contains("Rust"));
}

#[test]
fn tools_prompt_exposes_all_registry_tools() {
    let registry = ferrite::tools::ToolRegistry::new();
    let definitions = registry.get_definitions();
    let prompt = ferrite::context::build_tools_prompt(definitions, false);

    for def in definitions {
        assert!(
            prompt.contains(&format!("### {}", def.name)),
            "prompt should document tool {}",
            def.name
        );
    }
}

#[test]
fn json_rpc_contract_matches_rpc_module() {
    // Verify the JSON-RPC response structure used by the sidecar.
    let ok = ferrite::rpc::JsonRpcResponse::success(None, json!({"status": "ready"}));
    let ok_json = serde_json::to_value(&ok).expect("serialize");
    assert_eq!(ok_json["jsonrpc"], "2.0");
    assert!(ok_json.get("result").is_some());

    let err = ferrite::rpc::JsonRpcResponse::error(
        Some(json!(7)),
        -32601,
        "Method not found: foo".to_string(),
    );
    let err_json = serde_json::to_value(&err).expect("serialize");
    assert_eq!(err_json["error"]["code"], -32601);
    assert_eq!(err_json["error"]["message"], "Method not found: foo");
}