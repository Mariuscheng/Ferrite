use ferrite::config::Config;
use ferrite::providers::deepseek::DeepSeekProvider;
use ferrite::providers::{AiProvider, ChatMessage, ChatRequest, NativeFunctionCall, NativeToolCall, Role};

#[test]
fn serializes_official_native_tool_payload() {
    let provider = DeepSeekProvider::new(&Config::default()).expect("provider build");
    let request = ChatRequest {
        model: "deepseek-chat".to_string(),
        messages: vec![
            ChatMessage {
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
            },
            ChatMessage {
                role: Role::Tool,
                content: "workspace data".to_string(),
                tool_call_id: Some("call_1".to_string()),
                name: None,
                tool_calls: None,
            },
        ],
        temperature: 0.2,
        max_tokens: Some(128),
        stream: false,
        reasoning: false,
        tools: Some(vec![serde_json::json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "parameters": { "type": "object" }
            }
        })]),
    };

    let body = provider.build_body(&request);

    assert!(body["messages"][0]["content"].is_null());
    assert_eq!(body["messages"][0]["tool_calls"][0]["id"], "call_1");
    assert_eq!(
        body["messages"][0]["tool_calls"][0]["function"]["name"],
        "read_file"
    );
    assert_eq!(body["messages"][1]["tool_call_id"], "call_1");
    assert_eq!(body["tools"][0]["function"]["name"], "read_file");
}

#[test]
fn parses_null_content_native_tool_call_response() {
    let provider = DeepSeekProvider::new(&Config::default()).expect("provider build");
    let body = r#"{
        "id":"completion_1",
        "object":"chat.completion",
        "created":1,
        "model":"deepseek-chat",
        "choices":[{
            "index":0,
            "message":{
                "role":"assistant",
                "content":null,
                "tool_calls":[{
                    "id":"call_1",
                    "type":"function",
                    "function":{
                        "name":"read_file",
                        "arguments":"{\"path\":\"Cargo.toml\"}"
                    }
                }]
            },
            "finish_reason":"tool_calls"
        }]
    }"#;

    let response = provider.parse_response(body).expect("response should parse");
    let message = response.choices[0]
        .message
        .as_ref()
        .expect("assistant message");
    let tool_call = message
        .tool_calls
        .as_ref()
        .and_then(|tool_calls| tool_calls.first())
        .expect("native tool call");

    assert_eq!(message.content, "");
    assert_eq!(tool_call.id, "call_1");
    assert_eq!(tool_call.function.name, "read_file");
    assert_eq!(tool_call.function.arguments, r#"{"path":"Cargo.toml"}"#);
}
