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
