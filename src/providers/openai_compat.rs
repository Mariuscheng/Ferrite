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

/// Build an HTTP `Client` with a timeout.
pub fn build_http_client(timeout: std::time::Duration) -> Result<Client> {
    Client::builder()
        .timeout(timeout)
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