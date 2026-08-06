use async_trait::async_trait;
use ferrite::config::Config;
use ferrite::providers::{
    chat_stream_fallback, ChatChoice, ChatMessage, ChatRequest, ChatResponse, Role,
};
use ferrite::providers::{AiProvider, StreamCallback};
use ferrite::providers::openai_compat::{build_openai_headers, build_openai_messages, build_openai_body, normalize_chat_url, parse_openai_response};
use ferrite::providers::{NativeFunctionCall, NativeToolCall};
use ferrite::rpc::{map_reasoning_effort, JsonRpcRequest, JsonRpcResponse};
use reqwest::{header::{HeaderMap, AUTHORIZATION, CONTENT_TYPE}, Client};
use serde_json::Value;

// ── providers.rs 搬遷測試 ─────────────────────────────────────────────

#[test]
fn is_retryable_accepts_429_and_server_errors() {
    assert!(ferrite::providers::is_retryable(reqwest::StatusCode::TOO_MANY_REQUESTS));
    assert!(ferrite::providers::is_retryable(reqwest::StatusCode::INTERNAL_SERVER_ERROR));
    assert!(ferrite::providers::is_retryable(reqwest::StatusCode::BAD_GATEWAY));
    assert!(!ferrite::providers::is_retryable(reqwest::StatusCode::BAD_REQUEST));
    assert!(!ferrite::providers::is_retryable(reqwest::StatusCode::UNAUTHORIZED));
    assert!(!ferrite::providers::is_retryable(reqwest::StatusCode::OK));
}

#[tokio::test]
async fn retry_delay_allows_three_attempts_then_stops() {
    // attempt 0, 1, 2 → allowed; attempt 3 → exhausted
    assert!(ferrite::providers::retry_delay(0).await);
    assert!(ferrite::providers::retry_delay(1).await);
    assert!(ferrite::providers::retry_delay(2).await);
    assert!(!ferrite::providers::retry_delay(3).await);
}

#[test]
fn role_serializes_lowercase() {
    assert_eq!(serde_json::to_string(&Role::System).unwrap(), r#""system""#);
    assert_eq!(serde_json::to_string(&Role::User).unwrap(), r#""user""#);
    assert_eq!(serde_json::to_string(&Role::Assistant).unwrap(), r#""assistant""#);
    assert_eq!(serde_json::to_string(&Role::Tool).unwrap(), r#""tool""#);
}

#[test]
fn chat_message_deserializes_null_content_as_empty() {
    let json = r#"{"role":"assistant","content":null}"#;
    let msg: ChatMessage = serde_json::from_str(json).expect("parse");
    assert_eq!(msg.content, "");
}

#[test]
fn chat_message_serializes_empty_content_as_null() {
    let msg = ChatMessage {
        role: Role::Assistant,
        content: String::new(),
        tool_call_id: None,
        name: None,
        tool_calls: None,
    };
    let json = serde_json::to_value(&msg).unwrap();
    assert!(json["content"].is_null());
}

#[test]
fn chat_message_omits_optional_fields_when_none() {
    let msg = ChatMessage {
        role: Role::User,
        content: "hello".to_string(),
        tool_call_id: None,
        name: None,
        tool_calls: None,
    };
    let json = serde_json::to_value(&msg).unwrap();
    assert!(json.get("tool_call_id").is_none());
    assert!(json.get("name").is_none());
    assert!(json.get("tool_calls").is_none());
}

#[test]
fn create_provider_creates_known_providers() {
    let mut config = Config::default();
    config.api_key = "test-key".to_string();

    config.provider = "deepseek".to_string();
    let provider = ferrite::providers::create_provider(&config).expect("deepseek");
    assert_eq!(provider.name(), "deepseek");

    config.provider = "openai".to_string();
    let provider = ferrite::providers::create_provider(&config).expect("openai");
    assert_eq!(provider.name(), "openai");

    config.provider = "ollama".to_string();
    config.endpoint = "http://localhost:11434".to_string();
    let provider = ferrite::providers::create_provider(&config).expect("ollama");
    assert_eq!(provider.name(), "ollama");

    config.provider = "anthropic".to_string();
    let provider = ferrite::providers::create_provider(&config).expect("anthropic");
    assert_eq!(provider.name(), "anthropic");
}

#[test]
fn create_provider_rejects_unknown_provider() {
    let mut config = Config::default();
    config.provider = "unknown-vendor".to_string();
    let result = ferrite::providers::create_provider(&config);
    assert!(result.is_err());
}

// ── chat_stream_fallback（用 MockProvider 避免網路） ─────────────────

struct MockProvider {
    response: ChatResponse,
}

#[async_trait]
impl AiProvider for MockProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn client(&self) -> &Client {
        unreachable!("mock provider never performs HTTP requests")
    }

    fn chat_url(&self) -> String {
        "http://localhost/mock".to_string()
    }

    fn build_headers(&self) -> HeaderMap {
        HeaderMap::new()
    }

    fn build_body(&self, _request: &ChatRequest) -> Value {
        Value::Null
    }

    fn parse_response(&self, _body: &str) -> Result<ChatResponse, anyhow::Error> {
        Ok(self.response.clone())
    }

    async fn chat(&self, _request: ChatRequest) -> Result<ChatResponse, anyhow::Error> {
        Ok(self.response.clone())
    }

    async fn chat_stream(
        &self,
        _request: ChatRequest,
        _on_chunk: StreamCallback,
    ) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn chat_stream_fallback_emits_content_then_stop() {
    let provider = MockProvider {
        response: ChatResponse {
            id: "mock-1".to_string(),
            object: "chat.completion".to_string(),
            created: 0,
            model: "mock-model".to_string(),
            choices: vec![ChatChoice {
                index: 0,
                message: Some(ChatMessage {
                    role: Role::Assistant,
                    content: "hello from mock".to_string(),
                    tool_call_id: None,
                    name: None,
                    tool_calls: None,
                }),
                delta: None,
                finish_reason: Some("stop".to_string()),
            }],
            usage: None,
        },
    };

    let request = ChatRequest {
        model: "mock-model".to_string(),
        messages: Vec::new(),
        temperature: 0.3,
        max_tokens: None,
        stream: true,
        reasoning: false,
        tools: None,
    };

    let chunks = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let chunks_clone = std::sync::Arc::clone(&chunks);
    chat_stream_fallback(
        &provider,
        request,
        Box::new(move |chunk| {
            chunks_clone.lock().unwrap().push(chunk);
        }),
    )
    .await
    .expect("fallback should succeed");

    let captured = chunks.lock().unwrap().clone();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0].content.as_deref(), Some("hello from mock"));
    assert_eq!(captured[1].content, None);
    assert_eq!(captured[1].finish_reason.as_deref(), Some("stop"));
}

// ── openai_compat.rs 搬遷測試 ─────────────────────────────────────────

fn sample_request() -> ChatRequest {
    ChatRequest {
        model: "test-model".to_string(),
        messages: vec![ChatMessage {
            role: Role::System,
            content: "system prompt".to_string(),
            tool_call_id: None,
            name: None,
            tool_calls: None,
        }],
        temperature: 0.5,
        max_tokens: Some(256),
        stream: true,
        reasoning: false,
        tools: Some(vec![serde_json::json!({
            "type": "function",
            "function": { "name": "read_file" }
        })]),
    }
}

#[test]
fn normalize_chat_url_handles_trailing_slash() {
    assert_eq!(
        normalize_chat_url("https://api.openai.com/", false),
        "https://api.openai.com/chat/completions"
    );
}

#[test]
fn normalize_chat_url_handles_no_trailing_slash() {
    assert_eq!(
        normalize_chat_url("https://api.openai.com", false),
        "https://api.openai.com/chat/completions"
    );
}

#[test]
fn normalize_chat_url_ensure_v1_appends_v1() {
    assert_eq!(
        normalize_chat_url("https://api.deepseek.com", true),
        "https://api.deepseek.com/v1/chat/completions"
    );
}

#[test]
fn normalize_chat_url_ensure_v1_keeps_existing_v1() {
    assert_eq!(
        normalize_chat_url("https://api.deepseek.com/v1", true),
        "https://api.deepseek.com/v1/chat/completions"
    );
}

#[test]
fn openai_headers_include_bearer_auth() {
    let headers = build_openai_headers("secret-key");
    assert_eq!(
        headers.get(AUTHORIZATION).map(|v| v.to_str().unwrap()),
        Some("Bearer secret-key")
    );
    assert!(headers.contains_key(CONTENT_TYPE));
}

#[test]
fn build_openai_messages_serialize_roles_and_content() {
    let messages = vec![
        ChatMessage {
            role: Role::System,
            content: "sys".to_string(),
            tool_call_id: None,
            name: None,
            tool_calls: None,
        },
        ChatMessage {
            role: Role::User,
            content: "user text".to_string(),
            tool_call_id: None,
            name: None,
            tool_calls: None,
        },
    ];
    let json = build_openai_messages(&messages, false);
    assert_eq!(json.len(), 2);
    assert_eq!(json[0]["role"], "system");
    assert_eq!(json[0]["content"], "sys");
    assert_eq!(json[1]["role"], "user");
    assert_eq!(json[1]["content"], "user text");
}

#[test]
fn build_openai_messages_with_tool_calls_sets_null_content_when_requested() {
    let messages = vec![ChatMessage {
        role: Role::Assistant,
        content: String::new(),
        tool_call_id: None,
        name: None,
        tool_calls: Some(vec![NativeToolCall {
            id: "call_1".to_string(),
            kind: "function".to_string(),
            function: NativeFunctionCall {
                name: "read_file".to_string(),
                arguments: "{}".to_string(),
            },
        }]),
    }];
    let json = build_openai_messages(&messages, true);
    assert!(json[0]["content"].is_null());
    assert_eq!(json[0]["tool_calls"][0]["id"], "call_1");
}

#[test]
fn build_openai_body_includes_model_messages_and_stream() {
    let request = sample_request();
    let body = build_openai_body("test-model", &request, false);
    assert_eq!(body["model"], "test-model");
    assert_eq!(body["stream"], true);
    assert_eq!(body["messages"][0]["role"], "system");
    assert_eq!(body["max_tokens"], 256);
    assert_eq!(body["tools"][0]["function"]["name"], "read_file");
}

#[test]
fn parse_openai_response_parses_valid_body() {
    let body = r#"{
        "id": "resp_1",
        "object": "chat.completion",
        "created": 123,
        "model": "test-model",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": "hello" },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 5,
            "total_tokens": 15
        }
    }"#;
    let response = parse_openai_response(body, "test").expect("parse");
    assert_eq!(response.id, "resp_1");
    assert_eq!(response.choices.len(), 1);
    assert_eq!(
        response.choices[0].message.as_ref().map(|m| m.content.as_str()),
        Some("hello")
    );
    assert_eq!(response.usage.as_ref().map(|u| u.total_tokens), Some(15));
}

#[test]
fn parse_openai_response_returns_error_on_invalid_json() {
    let result = parse_openai_response("not json", "test");
    assert!(result.is_err());
}

// ── rpc.rs 搬遷測試 ────────────────────────────────────────────────────

#[test]
fn map_reasoning_effort_low_and_medium_map_to_high() {
    assert_eq!(map_reasoning_effort("low"), "high");
    assert_eq!(map_reasoning_effort("medium"), "high");
    assert_eq!(map_reasoning_effort("LOW"), "high");
}

#[test]
fn map_reasoning_effort_xhigh_maps_to_max() {
    assert_eq!(map_reasoning_effort("xhigh"), "max");
    assert_eq!(map_reasoning_effort("XHIGH"), "max");
}

#[test]
fn map_reasoning_effort_passes_through_other_values() {
    assert_eq!(map_reasoning_effort("high"), "high");
    assert_eq!(map_reasoning_effort("max"), "max");
    assert_eq!(map_reasoning_effort(""), "");
}

#[test]
fn jsonrpc_success_response_shape() {
    let resp = JsonRpcResponse::success(
        Some(serde_json::json!(1)),
        serde_json::json!({"status": "ok"}),
    );
    let value = serde_json::to_value(&resp).expect("serialize");
    assert_eq!(value["jsonrpc"], "2.0");
    assert_eq!(value["id"], 1);
    assert_eq!(value["result"]["status"], "ok");
    assert!(value.get("error").is_none());
}

#[test]
fn jsonrpc_error_response_shape() {
    let resp = JsonRpcResponse::error(
        Some(serde_json::json!(2)),
        -32602,
        "Missing required param: message".to_string(),
    );
    let value = serde_json::to_value(&resp).expect("serialize");
    assert_eq!(value["jsonrpc"], "2.0");
    assert_eq!(value["id"], 2);
    assert_eq!(value["error"]["code"], -32602);
    assert_eq!(value["error"]["message"], "Missing required param: message");
    assert!(value.get("result").is_none());
}

#[test]
fn jsonrpc_request_parses_default_params() {
    let request: JsonRpcRequest = serde_json::from_str(
        r#"{"id": 3, "method": "getStatus"}"#,
    )
    .expect("parse");
    assert_eq!(request.method, "getStatus");
    assert_eq!(request.params, Value::Null);
}