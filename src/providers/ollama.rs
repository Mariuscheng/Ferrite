use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::{header::HeaderMap, Client};
use serde_json::Value;

use super::{chat_stream_fallback, AiProvider, ChatMessage, ChatRequest, ChatResponse, ChatChoice, ChatUsage, Role, StreamCallback};
use crate::config::Config;

pub struct OllamaProvider {
    client: Client,
    endpoint: String,
    model: String,
}

impl OllamaProvider {
    pub fn new(config: &Config) -> Result<Self> {
        let client = Client::builder()
            .timeout(config.timeout())
            .build()
            .context("Failed to build HTTP client")?;

        let endpoint = if config.endpoint.ends_with('/') {
            config.endpoint.clone()
        } else {
            format!("{}/", config.endpoint)
        };

        Ok(Self {
            client,
            endpoint,
            model: config.model.clone(),
        })
    }
}

#[async_trait]
impl AiProvider for OllamaProvider {
    fn name(&self) -> &str {
        "ollama"
    }

    fn client(&self) -> &Client {
        &self.client
    }

    fn chat_url(&self) -> String {
        format!("{}api/chat", self.endpoint)
    }

    fn build_headers(&self) -> HeaderMap {
        use reqwest::header::{HeaderValue, CONTENT_TYPE};

        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        headers
    }

    fn build_body(&self, request: &ChatRequest) -> Value {
        let messages: Vec<Value> = request
            .messages
            .iter()
            .map(|m| {
                let mut obj = serde_json::json!({
                    "role": m.role,
                    "content": m.content,
                });
                if let Some(ref tci) = m.tool_call_id {
                    obj["tool_call_id"] = serde_json::json!(tci);
                }
                if let Some(ref n) = m.name {
                    obj["name"] = serde_json::json!(n);
                }
                obj
            })
            .collect();

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": request.stream,
        });

        if let Some(options) = request.max_tokens {
            body["options"] = serde_json::json!({
                "num_predict": options,
                "temperature": request.temperature,
            });
        }

        body
    }

    fn parse_response(&self, body: &str) -> Result<ChatResponse> {
        let v: Value = serde_json::from_str(body)
            .with_context(|| format!("Failed to parse Ollama response: {}", body))?;

        let content = v["message"]["content"].as_str().unwrap_or("");

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
            finish_reason: Some("stop".to_string()),
        };

        let usage = Some(ChatUsage {
            prompt_tokens: v
                .get("prompt_eval_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize,
            completion_tokens: v
                .get("eval_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize,
            total_tokens: 0,
        });

        Ok(ChatResponse {
            id: uuid::Uuid::new_v4().to_string(),
            object: "chat.completion".to_string(),
            created: 0,
            model: v["model"].as_str().unwrap_or(&self.model).to_string(),
            choices: vec![choice],
            usage,
        })
    }

    async fn chat_stream(
        &self,
        request: ChatRequest,
        on_chunk: StreamCallback,
    ) -> Result<()> {
        chat_stream_fallback(self, request, on_chunk).await
    }
}