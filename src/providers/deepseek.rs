use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use reqwest::{header::HeaderMap, Client};
use serde_json::Value;

use super::openai_compat;
use super::{AiProvider, ChatRequest, ChatResponse, ChatStreamChunk, StreamCallback};
use crate::config::Config;

pub struct DeepSeekProvider {
    client: Client,
    api_key: String,
    endpoint: String,
    model: String,
}

impl DeepSeekProvider {
    pub fn new(config: &Config) -> Result<Self> {
        let client = openai_compat::build_http_client(config.timeout())?;
        // DeepSeek uses /v1/chat/completions path
        let api_url = openai_compat::normalize_chat_url(&config.endpoint, true);

        Ok(Self {
            client,
            api_key: config.api_key.clone(),
            endpoint: api_url,
            model: config.model.clone(),
        })
    }
}

#[async_trait]
impl AiProvider for DeepSeekProvider {
    fn name(&self) -> &str {
        "deepseek"
    }

    fn client(&self) -> &Client {
        &self.client
    }

    fn chat_url(&self) -> String {
        self.endpoint.clone()
    }

    fn build_headers(&self) -> HeaderMap {
        openai_compat::build_openai_headers(&self.api_key)
    }

    fn build_body(&self, request: &ChatRequest) -> Value {
        // DeepSeek requires null content for assistant messages with tool_calls
        openai_compat::build_openai_body(&self.model, request, true)
    }

    fn parse_response(&self, body: &str) -> Result<ChatResponse> {
        openai_compat::parse_openai_response(body, self.name())
    }

    fn supports_native_tool_calls(&self) -> bool {
        true
    }

    /// DeepSeek uses real HTTP SSE streaming.
    async fn chat_stream(
        &self,
        mut request: ChatRequest,
        on_chunk: StreamCallback,
    ) -> Result<()> {
        request.stream = true;
        let body = self.build_body(&request);
        let headers = self.build_headers();

        let resp = self
            .client()
            .post(&self.endpoint)
            .headers(headers)
            .json(&body)
            .send()
            .await
            .with_context(|| "Failed to send streaming request to DeepSeek")?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("DeepSeek API error ({}): {}", status.as_u16(), text);
        }

        let mut stream = resp.bytes_stream();
        let mut buffer = String::new();

        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.with_context(|| "Failed to read stream chunk")?;
            buffer.push_str(&String::from_utf8_lossy(&chunk));

            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.is_empty() || line.starts_with(':') {
                    continue;
                }

                if line == "[DONE]" {
                    on_chunk(ChatStreamChunk {
                        content: None,
                        finish_reason: Some("stop".to_string()),
                    });
                    return Ok(());
                }

                if let Some(data) = line.strip_prefix("data: ") {
                    if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                        if let Some(choices) = parsed.get("choices").and_then(|c| c.as_array()) {
                            if let Some(first) = choices.first() {
                                let content = first
                                    .get("delta")
                                    .and_then(|d| d.get("content"))
                                    .and_then(|c| c.as_str())
                                    .map(|s| s.to_string());
                                let finish_reason = first
                                    .get("finish_reason")
                                    .and_then(|r| r.as_str())
                                    .map(|s| s.to_string());

                                on_chunk(ChatStreamChunk {
                                    content,
                                    finish_reason,
                                });
                            }
                        }
                    }
                }
            }
        }

        // Stream ended without [DONE] marker
        on_chunk(ChatStreamChunk {
            content: None,
            finish_reason: Some("stop".to_string()),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{ChatMessage, NativeFunctionCall, NativeToolCall, Role};

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
}