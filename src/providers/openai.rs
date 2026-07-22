use anyhow::Result;
use async_trait::async_trait;
use reqwest::{header::HeaderMap, Client};
use serde_json::Value;

use super::openai_compat;
use super::{chat_stream_fallback, AiProvider, ChatRequest, ChatResponse, StreamCallback};
use crate::config::Config;

pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    endpoint: String,
    model: String,
}

impl OpenAiProvider {
    pub fn new(config: &Config) -> Result<Self> {
        let client = openai_compat::build_http_client(config.timeout())?;
        let endpoint = openai_compat::normalize_chat_url(&config.endpoint, false);

        Ok(Self {
            client,
            api_key: config.api_key.clone(),
            endpoint,
            model: config.model.clone(),
        })
    }
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    fn name(&self) -> &str {
        "openai"
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
        openai_compat::build_openai_body(&self.model, request, false)
    }

    fn parse_response(&self, body: &str) -> Result<ChatResponse> {
        openai_compat::parse_openai_response(body, self.name())
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