use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{Arc, Mutex as StdMutex};

use crate::config::Config;
use crate::context;
use crate::edit_plan;
use crate::providers::{self, AiProvider, ChatMessage, ChatRequest, ChatStreamChunk, Role};
use crate::rpc::StreamChunkSink;
use crate::session::{AgentSession, SessionInfo, SessionSnapshot};
use crate::tool_parser;
use crate::tools::{ToolEventSink, ToolName, ToolRegistry};

// Re-export for backward compatibility
pub use crate::context::{get_reasoning_prompt, get_system_prompt};
pub use crate::edit_plan::ValidationRunResult;

pub struct CodingAgent {
    provider: Box<dyn AiProvider>,
    config: Config,
    tool_registry: ToolRegistry,
    sessions: HashMap<String, AgentSession>,
    /// Cached tool definitions as JSON values for native tool-call APIs.
    /// Invalidated when the provider changes (e.g. reconfigure).
    cached_native_tool_defs: Option<Arc<Vec<Value>>>,
    /// Cached tools prompt string: (use_native_tool_calls, prompt_text).
    /// Invalidated on reconfigure.
    cached_tools_prompt: Option<(bool, String)>,
}

impl CodingAgent {
    pub fn new_with_event_sink(
        config: Config,
        tool_event_sink: Option<ToolEventSink>,
    ) -> Result<Self> {
        let provider = providers::create_provider(&config)?;
        let provider_name = provider.name().to_string();
        tracing::debug!("Initialized provider: {}", provider_name);
        let mut tool_registry = match tool_event_sink {
            Some(event_sink) => ToolRegistry::with_event_sink(event_sink),
            None => ToolRegistry::new(),
        };
        tool_registry.set_shell_template(config.effective_shell().to_string());

        let mut agent = Self {
            provider,
            config,
            tool_registry,
            sessions: HashMap::new(),
            cached_native_tool_defs: None,
            cached_tools_prompt: None,
        };
        agent.load_sessions();
        Ok(agent)
    }

    // ── Session Persistence ────────────────────────────────────────────────

    fn sessions_dir() -> std::path::PathBuf {
        Config::config_dir().join("sessions")
    }

    fn session_file_path(session_id: &str) -> std::path::PathBuf {
        Self::sessions_dir().join(format!("{}.json", session_id))
    }

    fn save_sessions(&self) {
        let dir = Self::sessions_dir();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            tracing::warn!("Failed to create sessions dir {:?}: {}", dir, e);
            return;
        }
        for session in self.sessions.values() {
            let snapshot = SessionSnapshot::from_session(session);
            match serde_json::to_string_pretty(&snapshot) {
                Ok(json) => {
                    let path = Self::session_file_path(&session.id);
                    if let Err(e) = std::fs::write(&path, &json) {
                        tracing::warn!("Failed to save session {}: {}", session.id, e);
                    }
                }
                Err(e) => tracing::warn!("Failed to serialize session {}: {}", session.id, e),
            }
        }
    }

    fn load_sessions(&mut self) {
        let dir = Self::sessions_dir();
        if !dir.exists() {
            return;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Failed to read sessions dir {:?}: {}", dir, e);
                return;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e != "json").unwrap_or(true) {
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<SessionSnapshot>(&content) {
                    Ok(snapshot) => {
                        self.sessions.insert(snapshot.id.clone(), snapshot.into_session());
                    }
                    Err(e) => tracing::warn!("Failed to parse session file {:?}: {}", path, e),
                },
                Err(e) => tracing::warn!("Failed to read session file {:?}: {}", path, e),
            }
        }
        if !self.sessions.is_empty() {
            tracing::info!("Loaded {} sessions from disk", self.sessions.len());
        }
    }

    // ── Session Management ──────────────────────────────────────────────────

    pub fn create_session(&mut self, workspace_root: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        let base_prompt = if self.config.reasoning {
            get_reasoning_prompt()
        } else {
            get_system_prompt()
        };
        let tools_section = self.get_tools_prompt();
        let project_context = context::build_project_context(workspace_root);
        let system_prompt = format!(
            "{}\n\n{}\n\n{}",
            base_prompt, project_context, tools_section
        );
        let session = AgentSession::new(id.clone(), workspace_root.to_string(), system_prompt);
        self.sessions.insert(id.clone(), session);
        self.save_sessions();
        id
    }

    pub fn get_session(&self, id: &str) -> Option<&AgentSession> {
        self.sessions.get(id)
    }

    pub fn get_session_mut(&mut self, id: &str) -> Option<&mut AgentSession> {
        self.sessions.get_mut(id)
    }

    pub fn remove_session(&mut self, id: &str) -> bool {
        let removed = self.sessions.remove(id).is_some();
        if removed {
            let path = Self::session_file_path(id);
            let _ = std::fs::remove_file(&path);
            self.save_sessions();
        }
        removed
    }

    pub fn list_sessions(&self) -> Vec<SessionInfo> {
        self.sessions
            .iter()
            .map(|(id, s)| SessionInfo {
                id: id.clone(),
                title: s.title.clone(),
            })
            .collect()
    }

    pub fn save_terminal_state(&mut self, id: &str, state: &str) -> bool {
        if let Some(session) = self.sessions.get_mut(id) {
            session.metadata.insert("terminalState".into(), state.to_string());
            self.save_sessions();
            true
        } else {
            false
        }
    }

    pub fn rename_session(&mut self, id: &str, title: &str) -> bool {
        if let Some(session) = self.sessions.get_mut(id) {
            session.title = title.to_string();
            self.save_sessions();
            true
        } else {
            false
        }
    }

    // ── Tool Execution Helpers ──────────────────────────────────────────────

    /// Execute native tool calls from an assistant message.
    async fn execute_native_tool_calls(
        &mut self,
        session_id: &str,
        msg: &crate::providers::ChatMessage,
        workspace_root: &str,
    ) -> bool {
        let tool_calls = match msg.tool_calls.as_ref().filter(|tc| !tc.is_empty()) {
            Some(tc) => tc,
            None => return false,
        };

        {
            let Some(session) = self.sessions.get_mut(session_id) else {
                tracing::warn!("Session {} not found during native tool execution", session_id);
                return false;
            };
            session.add_native_assistant_message(msg.clone());
        }
        for tool_call in tool_calls {
            let tool_name = &tool_call.function.name;
            let tool_name_enum = match ToolName::from_str(tool_name) {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("Unknown native tool '{}': {}", tool_name, e);
                    continue;
                }
            };
            let args = serde_json::from_str(&tool_call.function.arguments).unwrap_or_else(|error| {
                serde_json::json!({
                    "_tool_argument_error": format!("Invalid JSON arguments: {}", error)
                })
            });
            let result = self
                .tool_registry
                .execute(tool_name_enum, args, workspace_root)
                .await;
            let tool_output = if result.success {
                result.content
            } else {
                format!(
                    "Tool '{}' failed: {}",
                    tool_name,
                    result.error.unwrap_or_default()
                )
            };
            let Some(session) = self.sessions.get_mut(session_id) else {
                tracing::warn!("Session {} lost during native tool execution", session_id);
                return false;
            };
            session.add_native_tool_result(&tool_call.id, &tool_output);
        }
        true
    }

    /// Extract and execute XML-based tool calls from a text response.
    async fn execute_xml_tool_calls_if_any(
        &mut self,
        session_id: &str,
        response_text: &str,
        workspace_root: &str,
    ) -> bool {
        let tool_definitions = self.tool_registry.get_definitions();
        let tool_calls = tool_parser::extract_tool_calls_from(response_text, tool_definitions);

        let Some(tool_calls) = tool_calls else {
            return false;
        };

        {
            let Some(session) = self.sessions.get_mut(session_id) else {
                tracing::warn!("Session {} not found during XML tool execution", session_id);
                return false;
            };
            session.add_assistant_message(response_text);
        }
        for tc in &tool_calls {
            let tool_name_enum = match ToolName::from_str(&tc.name) {
                Ok(t) => t,
                Err(e) => {
                    tracing::warn!("Unknown XML tool '{}': {}", tc.name, e);
                    continue;
                }
            };
            let args: serde_json::Value =
                serde_json::from_str(&tc.arguments).unwrap_or(serde_json::Value::Null);
            let result = self
                .tool_registry
                .execute(tool_name_enum, args, workspace_root)
                .await;
            let Some(session) = self.sessions.get_mut(session_id) else {
                tracing::warn!("Session {} lost during XML tool execution", session_id);
                return false;
            };
            session.add_assistant_message(&format!(
                "[Tool Call: {} with args: {}]",
                tc.name, tc.arguments
            ));
            if result.success {
                session.add_tool_result(&tc.name, &result);
            } else {
                let err_msg = format!(
                    "Tool '{}' failed: {}",
                    tc.name,
                    result.error.unwrap_or_default()
                );
                session.add_tool_msg(&tc.name, &err_msg);
            }
        }
        true
    }

    // ── Chat API ────────────────────────────────────────────────────────────

    pub async fn chat_stream(
        &mut self,
        session_id: &str,
        user_message: &str,
        sink: StreamChunkSink,
    ) -> Result<AgentResponse> {
        self.chat_impl(session_id, user_message, Some(sink), true)
            .await
    }

    pub async fn chat(
        &mut self,
        session_id: &str,
        user_message: &str,
    ) -> Result<AgentResponse> {
        self.chat_impl(session_id, user_message, None, false).await
    }

    async fn chat_impl(
        &mut self,
        session_id: &str,
        user_message: &str,
        sink: Option<StreamChunkSink>,
        stream_mode: bool,
    ) -> Result<AgentResponse> {
        if !self.sessions.contains_key(session_id) {
            anyhow::bail!("Session {} not found", session_id);
        }

        {
            let session = self.sessions.get_mut(session_id).unwrap();
            session.add_user_message(user_message);
        }

        let model = self.config.model.clone();
        let temperature = self.config.temperature;
        let reasoning = self.config.reasoning;
        let workspace_root = {
            let session = self.sessions.get(session_id).unwrap();
            session.workspace_root.clone()
        };
        let native_tools = self.native_tool_definitions();
        let use_native_tools = self.supports_native_tool_calls();

        let mut iteration = 0;
        let max_iterations = self.config.max_tool_iterations;

        let mut full_content;
        loop {
            if iteration >= max_iterations {
                let msg = "[系統] 已達到最大工具呼叫次數限制，請確認結果。".to_string();
                if let Some(ref sink) = sink {
                    sink(serde_json::json!({ "content": msg.clone(), "done": true }));
                }
                full_content = msg;
                break;
            }
            iteration += 1;

            let messages = {
                let session = self
                    .sessions
                    .get(session_id)
                    .ok_or_else(|| anyhow::anyhow!("Session {} not found", session_id))?;
                session.get_history()
            };

            let request = ChatRequest {
                model: model.clone(),
                messages,
                temperature,
                max_tokens: Some(16384),
                stream: stream_mode && !use_native_tools,
                reasoning,
                tools: use_native_tools.then(|| native_tools.as_ref().clone()),
            };

            if use_native_tools {
                let response = self.provider.chat(request).await?;
                let choice = response.choices.first().ok_or_else(|| {
                    anyhow::anyhow!("AI returned no choices in the response")
                })?;
                let msg = choice.message.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("AI response has no message content")
                })?;

                let text_before_tools = msg.content.clone();
                if !text_before_tools.trim().is_empty() {
                    if let Some(ref sink) = sink {
                        sink(serde_json::json!({ "content": text_before_tools, "done": false }));
                    }
                }

                if self
                    .execute_native_tool_calls(session_id, msg, &workspace_root)
                    .await
                {
                    continue;
                }

                let content = msg.content.clone();
                if let Some(ref sink) = sink {
                    if text_before_tools.trim().is_empty() {
                        sink(serde_json::json!({ "content": content, "done": true }));
                    } else {
                        sink(serde_json::json!({ "content": null, "done": true }));
                    }
                }
                {
                    let session = self
                        .sessions
                        .get_mut(session_id)
                        .ok_or_else(|| anyhow::anyhow!("Session {} not found", session_id))?;
                    session.add_assistant_message(&msg.content);
                }
                full_content = content;
                break;
            } else if stream_mode {
                let sink = sink.as_ref().unwrap().clone();
                let collected = Arc::new(StdMutex::new(String::new()));
                let collected_clone = Arc::clone(&collected);
                let sink_for_callback = sink.clone();
                self.provider
                    .chat_stream(
                        request,
                        Box::new(move |chunk: ChatStreamChunk| {
                            if let Some(content) = &chunk.content {
                                if let Ok(mut s) = collected_clone.lock() {
                                    s.push_str(content);
                                }
                                sink_for_callback(serde_json::json!({
                                    "content": content.clone(),
                                    "done": false
                                }));
                            }
                            if chunk.finish_reason.is_some() {
                                sink_for_callback(serde_json::json!({
                                    "content": null,
                                    "done": true
                                }));
                            }
                        }),
                    )
                    .await?;

                let collected_str = collected.lock().map(|s| s.clone()).unwrap_or_default();
                full_content = collected_str.clone();

                if self
                    .execute_xml_tool_calls_if_any(session_id, &full_content, &workspace_root)
                    .await
                {
                    sink(serde_json::json!({ "content": null, "done": true }));
                    continue;
                }

                {
                    let session = self
                        .sessions
                        .get_mut(session_id)
                        .ok_or_else(|| anyhow::anyhow!("Session {} not found", session_id))?;
                    session.add_assistant_message(&full_content);
                }
                break;
            } else {
                let response = self.provider.chat(request).await?;
                let choice = response.choices.first().ok_or_else(|| {
                    anyhow::anyhow!("AI returned no choices in the response")
                })?;
                let msg = choice.message.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("AI response has no message content")
                })?;

                if self
                    .execute_native_tool_calls(session_id, msg, &workspace_root)
                    .await
                {
                    continue;
                }

                if self
                    .execute_xml_tool_calls_if_any(session_id, &msg.content, &workspace_root)
                    .await
                {
                    continue;
                }

                {
                    let session = self
                        .sessions
                        .get_mut(session_id)
                        .ok_or_else(|| anyhow::anyhow!("Session {} not found", session_id))?;
                    session.add_assistant_message(&msg.content);
                }
                full_content = msg.content.clone();
                break;
            }
        }

        self.save_sessions();
        Ok(AgentResponse {
            content: if full_content.is_empty() {
                "[系統] 未取得回應".to_string()
            } else {
                full_content
            },
            session_id: session_id.to_string(),
            model,
        })
    }

    // ── Tools / Prompt Caching ──────────────────────────────────────────────

    fn supports_native_tool_calls(&self) -> bool {
        self.provider.supports_native_tool_calls()
    }

    fn get_tools_prompt(&mut self) -> String {
        let use_native = self.supports_native_tool_calls();
        if let Some((ref cached_flag, ref cached)) = self.cached_tools_prompt {
            if *cached_flag == use_native {
                return cached.clone();
            }
        }
        let tools = self.tool_registry.get_definitions();
        let prompt = context::build_tools_prompt(tools, use_native);
        self.cached_tools_prompt = Some((use_native, prompt.clone()));
        prompt
    }

    fn native_tool_definitions(&mut self) -> Arc<Vec<Value>> {
        if let Some(ref cached) = self.cached_native_tool_defs {
            return Arc::clone(cached);
        }

        let tools = self.tool_registry.get_definitions();
        let defs = context::build_native_tool_defs(tools);

        let arc = Arc::new(defs);
        self.cached_native_tool_defs = Some(Arc::clone(&arc));
        arc
    }

    // ── Message History ─────────────────────────────────────────────────────

    pub fn get_session_messages(&self, id: &str) -> Vec<ChatMessage> {
        let messages: Vec<ChatMessage> = self
            .get_session(id)
            .map(|s| {
                s.messages
                    .iter()
                    .skip(1)
                    .filter(|msg| {
                        if msg.role == Role::Tool {
                            return false;
                        }
                        if msg.role == Role::Assistant {
                            let content = msg.content.trim();
                            if content.is_empty() {
                                return false;
                            }
                            if content.starts_with("[Tool Call:") {
                                return false;
                            }
                        }
                        if msg.role == Role::User
                            && msg.content.trim().starts_with("[Tool result:")
                        {
                            return false;
                        }
                        true
                    })
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();

        // Merge consecutive messages of the same role so that a single
        // assistant turn (which may span multiple internal messages due to
        // tool-call iterations) appears as one coherent response bubble.
        let mut merged: Vec<ChatMessage> = Vec::new();
        for msg in messages {
            if let Some(last) = merged.last_mut() {
                if last.role == msg.role {
                    last.content.push('\n');
                    last.content.push_str(&msg.content);
                    continue;
                }
            }
            merged.push(msg);
        }
        merged
    }

    // ── IDE Context ─────────────────────────────────────────────────────────

    pub fn set_session_ide_context(&mut self, session_id: &str, context: &Value) -> Result<()> {
        let session = self
            .get_session_mut(session_id)
            .ok_or_else(|| anyhow::anyhow!("Session {} not found", session_id))?;
        session
            .metadata
            .insert("ide_context".into(), serde_json::to_string(context)?);
        Ok(())
    }

    fn get_session_ide_context_text(&self, session_id: &str) -> Option<String> {
        let session = self.get_session(session_id)?;
        session.metadata.get("ide_context").cloned()
    }

    // ── Edit Plan (delegated to edit_plan module) ───────────────────────────

    pub async fn generate_edit_plan(
        &mut self,
        session_id: &str,
        goal: &str,
        ide_context: Option<Value>,
    ) -> Result<Value> {
        if self.get_session(session_id).is_none() {
            anyhow::bail!("Session {} not found", session_id);
        }

        if let Some(ctx) = ide_context.as_ref() {
            self.set_session_ide_context(session_id, ctx)?;
        }

        let ide_context_text = self
            .get_session_ide_context_text(session_id)
            .unwrap_or_else(|| "{}".to_string());

        edit_plan::generate_edit_plan(
            self.provider.as_ref(),
            &self.config.model,
            self.config.reasoning,
            goal,
            &ide_context_text,
        )
        .await
    }

    pub async fn run_validation_commands(
        &self,
        workspace_root: &str,
        commands: &[String],
    ) -> Vec<ValidationRunResult> {
        edit_plan::run_validation_commands(&self.tool_registry, workspace_root, commands).await
    }

    // ── Reconfigure ─────────────────────────────────────────────────────────

    pub fn reconfigure(&mut self, config: Config) -> Result<()> {
        self.tool_registry
            .set_shell_template(config.effective_shell().to_string());
        self.provider = providers::create_provider(&config)?;
        self.config = config;
        self.cached_native_tool_defs = None;
        self.cached_tools_prompt = None;
        Ok(())
    }

    pub fn get_config(&self) -> &Config {
        &self.config
    }
}

// ── Response Types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResponse {
    pub content: String,
    pub session_id: String,
    pub model: String,
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{NativeFunctionCall, NativeToolCall};
    use crate::tools::ToolResult;

    #[test]
    fn xml_tool_results_are_sent_as_user_messages() {
        let mut session = AgentSession::new(
            "session".to_string(),
            ".".to_string(),
            "system prompt".to_string(),
        );
        session.add_tool_result(
            "list_files",
            &ToolResult {
                success: true,
                content: "src/\nCargo.toml".to_string(),
                error: None,
            },
        );

        let result = session.get_history().pop().expect("tool result message");
        assert_eq!(result.role, Role::User);
        assert_eq!(result.tool_call_id, None);
        assert_eq!(result.name, None);
        assert!(result.content.contains("[Tool result: list_files (success)]"));
        assert!(result.content.contains("src/"));
    }

    #[test]
    fn legacy_tool_messages_are_normalized_before_api_requests() {
        let mut session = AgentSession::new(
            "session".to_string(),
            ".".to_string(),
            "system prompt".to_string(),
        );
        session.messages.push_back(ChatMessage {
            role: Role::Tool,
            content: "legacy result".to_string(),
            tool_call_id: Some("legacy-id".to_string()),
            name: Some("list_files".to_string()),
            tool_calls: None,
        });

        let result = session.get_history().pop().expect("normalized message");
        assert_eq!(result.role, Role::User);
        assert_eq!(result.tool_call_id, None);
        assert_eq!(result.name, None);
        assert!(result.content.contains("legacy result"));
    }

    #[test]
    fn matching_native_tool_results_are_kept_for_api_requests() {
        let mut session = AgentSession::new(
            "session".to_string(),
            ".".to_string(),
            "system prompt".to_string(),
        );
        session.add_native_assistant_message(ChatMessage {
            role: Role::Assistant,
            content: String::new(),
            tool_call_id: None,
            name: None,
            tool_calls: Some(vec![NativeToolCall {
                id: "call_1".to_string(),
                kind: "function".to_string(),
                function: NativeFunctionCall {
                    name: "list_files".to_string(),
                    arguments: r#"{"path":"."}"#.to_string(),
                },
            }]),
        });
        session.add_native_tool_result("call_1", "Cargo.toml\nsrc/");

        let history = session.get_history();
        let assistant = &history[1];
        let tool_result = &history[2];

        assert_eq!(assistant.role, Role::Assistant);
        assert_eq!(assistant.tool_calls.as_ref().map(Vec::len), Some(1));
        assert_eq!(tool_result.role, Role::Tool);
        assert_eq!(tool_result.tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(tool_result.content, "Cargo.toml\nsrc/");
    }
}