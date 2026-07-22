pub mod openai;
pub mod openai_compat;
pub mod anthropic;
pub mod ollama;
pub mod deepseek;

use anyhow::{Context, Result};
use async_trait::async_trait;
use reqwest::{header::HeaderMap, Client};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::time::Duration;

use crate::config::Config;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    #[serde(
        default,
        deserialize_with = "deserialize_nullable_string",
        serialize_with = "serialize_nullable_string"
    )]
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<NativeToolCall>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeToolCall {
    pub id: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub function: NativeFunctionCall,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeFunctionCall {
    pub name: String,
    pub arguments: String,
}

fn deserialize_nullable_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

fn serialize_nullable_string<S>(value: &str, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    if value.is_empty() {
        serializer.serialize_none()
    } else {
        serializer.serialize_str(value)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
    pub max_tokens: Option<usize>,
    pub stream: bool,
    pub reasoning: bool,
    pub tools: Option<Vec<Value>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatChoice {
    pub index: usize,
    pub message: Option<ChatMessage>,
    pub delta: Option<ChatDelta>,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatDelta {
    pub role: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatUsage {
    pub prompt_tokens: usize,
    pub completion_tokens: usize,
    pub total_tokens: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatResponse {
    pub id: String,
    pub object: String,
    pub created: u64,
    pub model: String,
    pub choices: Vec<ChatChoice>,
    pub usage: Option<ChatUsage>,
}

/// A single chunk from a streaming chat completion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatStreamChunk {
    pub content: Option<String>,
    pub finish_reason: Option<String>,
}

/// Callback for streaming chunks
pub type StreamCallback = Box<dyn Fn(ChatStreamChunk) + Send + 'static>;

/// Maximum number of retry attempts for transient API errors (429, 5xx).
const MAX_RETRIES: u32 = 3;

/// Base backoff duration — doubled on each retry (1s, 2s, 4s).
const BASE_BACKOFF_MS: u64 = 1_000;

/// Check whether an HTTP status code is retryable (rate-limit or server error).
pub fn is_retryable(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
}

/// Wait for an exponential-backoff duration before the next retry attempt,
/// returning `true` when another attempt is allowed or `false` when the
/// maximum retry count has been exhausted.
pub async fn retry_delay(attempt: u32) -> bool {
    if attempt >= MAX_RETRIES {
        return false;
    }
    let ms = BASE_BACKOFF_MS * (1u64 << attempt); // 1s, 2s, 4s
    tokio::time::sleep(Duration::from_millis(ms)).await;
    true
}

/// Trait that all AI providers must implement
#[async_trait]
pub trait AiProvider: Send + Sync {
    // -----------------------------------------------------------------------
    // 抽象方法 — 每個 provider 只實作差異部分
    // -----------------------------------------------------------------------

    /// Provider identifier name.
    fn name(&self) -> &str;

    /// The HTTP client used for API requests.
    fn client(&self) -> &Client;

    /// Full chat-completions endpoint URL (including path).
    fn chat_url(&self) -> String;

    /// Build request headers (auth, content-type, version, etc.).
    fn build_headers(&self) -> HeaderMap;

    /// Build the JSON request body from a `ChatRequest`.
    fn build_body(&self, request: &ChatRequest) -> Value;

    /// Parse the raw HTTP response body into a `ChatResponse`.
    fn parse_response(&self, body: &str) -> Result<ChatResponse>;

    /// Whether this provider supports OpenAI-compatible native tool_calls.
    fn supports_native_tool_calls(&self) -> bool {
        false
    }

    // -----------------------------------------------------------------------
    // 預設實作 — 共享的 retry + POST 邏輯
    // -----------------------------------------------------------------------

    /// Send a chat completion request (non-streaming), with automatic retry.
    async fn chat(&self, mut request: ChatRequest) -> Result<ChatResponse> {
        request.stream = false;
        let body = self.build_body(&request);
        let headers = self.build_headers();
        let url = self.chat_url();

        let mut attempt = 0_u32;
        loop {
            let resp = self
                .client()
                .post(&url)
                .headers(headers.clone())
                .json(&body)
                .send()
                .await
                .with_context(|| format!("Failed to send request to {}", self.name()))?;

            let status = resp.status();
            let text = resp
                .text()
                .await
                .with_context(|| "Failed to read response body")?;

            if status.is_success() {
                return self.parse_response(&text);
            }

            if is_retryable(status) && retry_delay(attempt).await {
                attempt += 1;
                tracing::warn!(
                    "{} API retry {}/{} after status {}",
                    self.name(),
                    attempt,
                    MAX_RETRIES,
                    status.as_u16()
                );
                continue;
            }

            anyhow::bail!("{} API error ({}): {}", self.name(), status.as_u16(), text);
        }
    }

    /// Send a streaming chat completion request.
    async fn chat_stream(
        &self,
        request: ChatRequest,
        on_chunk: StreamCallback,
    ) -> Result<()>;
}

/// Fallback `chat_stream` implementation for providers that do not support
/// real HTTP streaming: calls `chat()` and emits content as a single chunk.
pub async fn chat_stream_fallback(
    provider: &dyn AiProvider,
    request: ChatRequest,
    on_chunk: StreamCallback,
) -> Result<()> {
    let response = provider.chat(request).await?;
    if let Some(choice) = response.choices.first() {
        if let Some(msg) = &choice.message {
            on_chunk(ChatStreamChunk {
                content: Some(msg.content.clone()),
                finish_reason: choice.finish_reason.clone(),
            });
        }
    }
    on_chunk(ChatStreamChunk {
        content: None,
        finish_reason: Some("stop".to_string()),
    });
    Ok(())
}

/// Factory to create the appropriate AI provider
pub fn create_provider(config: &Config) -> Result<Box<dyn AiProvider>> {
    match config.provider.to_lowercase().as_str() {
        "openai" | "azure" => {
            let provider = openai::OpenAiProvider::new(config)?;
            Ok(Box::new(provider))
        }
        "anthropic" | "claude" => {
            let provider = anthropic::AnthropicProvider::new(config)?;
            Ok(Box::new(provider))
        }
        "ollama" => {
            let provider = ollama::OllamaProvider::new(config)?;
            Ok(Box::new(provider))
        }
        "deepseek" => {
            let provider = deepseek::DeepSeekProvider::new(config)?;
            Ok(Box::new(provider))
        }
        other => anyhow::bail!(
            "Unknown AI provider: {}. Supported: openai, anthropic, ollama, deepseek",
            other
        ),
    }
}