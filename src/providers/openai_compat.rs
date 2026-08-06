use anyhow::{Context, Result};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use serde_json::Value;

use super::{ChatMessage, ChatRequest, ChatResponse, Role};

// ── Shared Helpers ────────────────────────────────────────────────────────

/// Build standard OpenAI-compatible `Bearer` auth headers.
pub fn build_openai_headers(api_key: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        AUTHORIZATION,
        HeaderValue::from_str(&format!("Bearer {}", api_key))
            .unwrap_or_else(|_| HeaderValue::from_static("Bearer invalid")),
    );
    headers
}

/// Serialize a list of `ChatMessage`s into the standard OpenAI-compatible JSON
/// array format. When `content_is_null_for_tool_calls` is true, assistant
/// messages with `tool_calls` will have their `content` set to `null` instead
/// of an empty string (required by DeepSeek).
pub fn build_openai_messages(
    messages: &[ChatMessage],
    content_is_null_for_tool_calls: bool,
) -> Vec<Value> {
    messages
        .iter()
        .map(|m| {
            let content = if content_is_null_for_tool_calls
                && m.role == Role::Assistant
                && m.content.is_empty()
                && m.tool_calls.is_some()
            {
                Value::Null
            } else {
                serde_json::json!(m.content)
            };
            let mut obj = serde_json::json!({
                "role": m.role,
                "content": content,
            });
            if let Some(ref tci) = m.tool_call_id {
                obj["tool_call_id"] = serde_json::json!(tci);
            }
            if let Some(ref n) = m.name {
                obj["name"] = serde_json::json!(n);
            }
            if let Some(ref tool_calls) = m.tool_calls {
                obj["tool_calls"] = serde_json::to_value(tool_calls)
                    .expect("native tool calls must be serializable");
            }
            obj
        })
        .collect()
}

/// Build the standard OpenAI-compatible JSON request body.
pub fn build_openai_body(
    model: &str,
    request: &ChatRequest,
    content_is_null_for_tool_calls: bool,
) -> Value {
    let messages = build_openai_messages(&request.messages, content_is_null_for_tool_calls);

    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "temperature": request.temperature,
        "stream": request.stream,
    });

    if let Some(max_tokens) = request.max_tokens {
        body["max_tokens"] = serde_json::json!(max_tokens);
    }
    if let Some(tools) = &request.tools {
        body["tools"] = serde_json::json!(tools);
    }

    body
}

/// Parse an OpenAI-compatible JSON response string into a `ChatResponse`.
pub fn parse_openai_response(body: &str, provider_name: &str) -> Result<ChatResponse> {
    let resp: ChatResponse = serde_json::from_str(body).with_context(|| {
        format!(
            "Failed to parse {} response: {}",
            provider_name, body
        )
    })?;
    Ok(resp)
}

/// Build an HTTP `Client` with a timeout and connection-pool limits.
pub fn build_http_client(timeout: std::time::Duration) -> Result<Client> {
    Client::builder()
        .timeout(timeout)
        .pool_max_idle_per_host(4)
        .pool_idle_timeout(std::time::Duration::from_secs(90))
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .context("Failed to build HTTP client")
}

/// Normalize the endpoint URL for the `/chat/completions` path, with
/// optional `/v1` path handling (DeepSeek compatible).
pub fn normalize_chat_url(endpoint: &str, ensure_v1: bool) -> String {
    let base = endpoint.trim_end_matches('/');
    if ensure_v1 {
        if base.ends_with("/v1") {
            format!("{}/chat/completions", base)
        } else {
            format!("{}/v1/chat/completions", base)
        }
    } else {
        if base.ends_with('/') {
            format!("{}chat/completions", base)
        } else {
            format!("{}/chat/completions", base)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{NativeFunctionCall, NativeToolCall};

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
}
