use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::{header::HeaderMap, Client};
use serde_json::Value;

use super::{chat_stream_fallback, AiProvider, ChatMessage, ChatRequest, ChatResponse, ChatChoice, ChatUsage, Role, StreamCallback};
use crate::config::Config;

pub struct AnthropicProvider {
    client: Client,
    api_key: String,
    model: String,
    anthropic_version: String,
}

impl AnthropicProvider {
    pub fn new(config: &Config) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout())
            .build()
            .context("Failed to build HTTP client")?;

        Ok(Self {
            client,
            api_key: config.api_key.clone(),
            model: config.model.clone(),
            anthropic_version: "2023-06-01".to_string(),
        })
    }

    fn build_anthropic_messages(&self, request: &ChatRequest) -> (Option<String>, Vec<Value>) {
        let mut system = None;
        let mut messages = Vec::new();

        for msg in &request.messages {
            match msg.role {
                Role::System => {
                    system = Some(msg.content.clone());
                }
                Role::User | Role::Assistant => {
                    messages.push(serde_json::json!({
                        "role": msg.role,
                        "content": msg.content,
                    }));
                }
                _ => {
                    messages.push(serde_json::json!({
                        "role": "user",
                        "content": msg.content,
                    }));
                }
            }
        }

        (system, messages)
    }
}

#[async_trait]
impl AiProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    fn client(&self) -> &Client {
        &self.client
    }

    fn chat_url(&self) -> String {
        "https://api.anthropic.com/v1/messages".to_string()
    }

    fn build_headers(&self) -> HeaderMap {
        use reqwest::header::{HeaderValue, CONTENT_TYPE};

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&self.api_key).unwrap_or(HeaderValue::from_static("")),
        );
        headers.insert(
            "anthropic-version",
            HeaderValue::from_str(&self.anthropic_version)
                .unwrap_or(HeaderValue::from_static("")),
        );
        headers
    }

    fn build_body(&self, request: &ChatRequest) -> Value {
        let (system, messages) = self.build_anthropic_messages(request);
        let max_tokens = request.max_tokens.unwrap_or(4096);

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": max_tokens,
            "messages": messages,
        });

        if let Some(s) = system {
            body["system"] = serde_json::json!(s);
        }

        if request.stream {
            body["stream"] = serde_json::json!(true);
        }

        body
    }

    fn parse_response(&self, body: &str) -> Result<ChatResponse> {
        let v: Value = serde_json::from_str(body)
            .with_context(|| format!("Failed to parse Anthropic response: {}", body))?;

        let content = v["content"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|c| c["text"].as_str())
            .unwrap_or("");

        let choice = ChatChoice {
            index: 0,
            message: Some(ChatMessage {
                role: Role::Assistant,
                content: content.to_string(),
                tool_call_id: None,
                name: None,
                tool_calls: None,
            }),
            delta: None,
            finish_reason: v["stop_reason"].as_str().map(|s| s.to_string()),
        };

        let usage = v["usage"].as_object().map(|u| ChatUsage {
            prompt_tokens: u
                .get("input_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize,
            completion_tokens: u
                .get("output_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize,
            total_tokens: 0,
        });

        Ok(ChatResponse {
            id: v["id"].as_str().unwrap_or("").to_string(),
            object: "chat.completion".to_string(),
            created: 0,
            model: v["model"].as_str().unwrap_or("").to_string(),
            choices: vec![choice],
            usage,
        })
    }

    fn supports_native_tool_calls(&self) -> bool {
        true
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
        on_chunk: StreamCallback,
    ) -> Result<()> {
        chat_stream_fallback(self, request, on_chunk).await
    }
}