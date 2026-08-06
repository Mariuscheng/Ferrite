use serde::{Deserialize, Serialize};
use serde_json::Value;

use std::sync::Arc;

use crate::agent::CodingAgent;
use crate::config::Config;
use crate::tools::ToolEventSink;

/// Sink for streaming chat chunks — called from agent during streaming
pub type StreamChunkSink = Arc<dyn Fn(serde_json::Value) + Send + Sync>;

/// Map reasoning effort levels for compatibility:
/// low/medium → high, xhigh → max
pub fn map_reasoning_effort(effort: &str) -> String {
    match effort.to_lowercase().as_str() {
        "low" | "medium" => "high".into(),
        "xhigh" => "max".into(),
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

/// JSON-RPC 2.0 request
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// JSON-RPC 2.0 response
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jsonrpc: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcResponse {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: Some("2.0".into()),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<Value>, code: i64, message: String) -> Self {
        Self {
            jsonrpc: Some("2.0".into()),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data: None,
            }),
        }
    }
}

// ---------------------------------------------------------------------------
// Parameter extraction helpers — eliminate repetitive `match` chains
// ---------------------------------------------------------------------------

/// Extract a required string parameter, returning an error response if missing.
fn require_string_param(
    id: &Option<Value>,
    params: &Value,
    name: &str,
) -> Result<String, Box<JsonRpcResponse>> {
    params[name]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| {
            Box::new(JsonRpcResponse::error(
                id.clone(),
                -32602,
                format!("Missing required param: {}", name),
            ))
        })
}

/// Extract the `sessionId` parameter (required by many RPC methods).
fn require_session_id(id: &Option<Value>, params: &Value) -> Result<String, Box<JsonRpcResponse>> {
    require_string_param(id, params, "sessionId")
}

/// Extract `workspaceRoot`, defaulting to `"."`.
fn workspace_root(params: &Value) -> String {
    params["workspaceRoot"].as_str().unwrap_or(".").to_string()
}

/// Resolve a session (create if missing), then optionally apply `ideContext`.
fn resolve_session(
    agent: &mut CodingAgent,
    session_id: &str,
    workspace_root: &str,
    params: &Value,
) -> String {
    let effective_id = if agent.get_session(session_id).is_some() {
        session_id.to_string()
    } else {
        tracing::warn!(
            "Session {} not found; creating a new session for workspace {}",
            session_id,
            workspace_root
        );
        agent.create_session(workspace_root)
    };

    if let Some(ide_context) = params.get("ideContext") {
        let _ = agent.set_session_ide_context(&effective_id, ide_context);
    }

    effective_id
}

/// Build the JSON for chat-style responses.
fn chat_response_json(
    session_id: &str,
    model: &str,
    content: &str,
    recovered: bool,
) -> Value {
    serde_json::json!({
        "sessionId": session_id,
        "model": model,
        "content": content,
        "recovered": recovered,
    })
}

// ---------------------------------------------------------------------------
// RpcHandler
// ---------------------------------------------------------------------------

pub struct RpcHandler {
    agent: CodingAgent,
    stream_sink: Option<StreamChunkSink>,
    cancel_tx: tokio::sync::watch::Sender<bool>,
    cancel_rx: Option<tokio::sync::watch::Receiver<bool>>,
}

impl RpcHandler {
    pub fn with_stream_sink(
        config: Config,
        tool_event_sink: ToolEventSink,
        stream_sink: StreamChunkSink,
    ) -> anyhow::Result<Self> {
        let (cancel_tx, cancel_rx) = tokio::sync::watch::channel(false);
        Ok(Self {
            agent: CodingAgent::new_with_event_sink(config, Some(tool_event_sink))?,
            stream_sink: Some(stream_sink),
            cancel_tx,
            cancel_rx: Some(cancel_rx),
        })
    }

    pub async fn handle_request(&mut self, req: JsonRpcRequest) -> JsonRpcResponse {
        tracing::debug!("Handling RPC method: {}", req.method);

        match req.method.as_str() {
            "initialize" => {
                let workspace_root = workspace_root(&req.params);
                let session_id = self.agent.create_session(&workspace_root);
                JsonRpcResponse::success(
                    req.id,
                    serde_json::json!({
                        "sessionId": session_id,
                        "workspaceRoot": workspace_root,
                        "status": "ready"
                    }),
                )
            }

            "chatStream" => {
                let session_id = match require_session_id(&req.id, &req.params) {
                    Ok(id) => id,
                    Err(e) => return *e,
                };
                let user_message = match require_string_param(&req.id, &req.params, "message") {
                    Ok(msg) => msg,
                    Err(e) => return *e,
                };
                let ws = workspace_root(&req.params);
                let effective = resolve_session(&mut self.agent, &session_id, &ws, &req.params);

                let sink = match self.stream_sink.clone() {
                    Some(s) => s,
                    None => {
                        return JsonRpcResponse::error(
                            req.id,
                            -32000,
                            "stream sink not configured".into(),
                        );
                    }
                };

                let cancel_rx = self.cancel_rx.take().unwrap_or_else(|| {
                    let (_, rx) = tokio::sync::watch::channel(false);
                    rx
                });

                let result = self.agent.chat_stream_with_cancel(
                    &effective, &user_message, sink, cancel_rx,
                ).await;

                // Reset cancel channel for next request
                let (new_tx, new_rx) = tokio::sync::watch::channel(false);
                self.cancel_tx = new_tx;
                self.cancel_rx = Some(new_rx);

                match result {
                    Ok(resp) => JsonRpcResponse::success(
                        req.id,
                        chat_response_json(
                            &resp.session_id,
                            &resp.model,
                            &resp.content,
                            effective != session_id,
                        ),
                    ),
                    Err(e) => {
                        JsonRpcResponse::error(req.id, -32000, format!("Agent error: {}", e))
                    }
                }
            }

            "chat" => {
                let session_id = match require_session_id(&req.id, &req.params) {
                    Ok(id) => id,
                    Err(e) => return *e,
                };
                let user_message = match require_string_param(&req.id, &req.params, "message") {
                    Ok(msg) => msg,
                    Err(e) => return *e,
                };
                let ws = workspace_root(&req.params);
                let effective = resolve_session(&mut self.agent, &session_id, &ws, &req.params);

                match self.agent.chat(&effective, &user_message).await {
                    Ok(resp) => JsonRpcResponse::success(
                        req.id,
                        chat_response_json(
                            &resp.session_id,
                            &resp.model,
                            &resp.content,
                            effective != session_id,
                        ),
                    ),
                    Err(e) => JsonRpcResponse::error(req.id, -32000, format!("Agent error: {}", e)),
                }
            }

            "stopGeneration" => {
                // Signal the cancel channel to abort the in-flight chatStream loop.
                let _ = self.cancel_tx.send(true);
                tracing::info!("Cancellation signal sent to chat loop");
                JsonRpcResponse::success(
                    req.id,
                    serde_json::json!({"status": "stopRequested"}),
                )
            }

            "updateIdeContext" => {
                let session_id = match require_session_id(&req.id, &req.params) {
                    Ok(id) => id,
                    Err(e) => return *e,
                };
                let context = match req.params.get("context") {
                    Some(v) => v,
                    None => {
                        return JsonRpcResponse::error(
                            req.id,
                            -32602,
                            "Missing required param: context".into(),
                        );
                    }
                };

                match self.agent.set_session_ide_context(&session_id, context) {
                    Ok(_) => {
                        JsonRpcResponse::success(req.id, serde_json::json!({"status": "ok"}))
                    }
                    Err(e) => JsonRpcResponse::error(
                        req.id,
                        -32000,
                        format!("Failed to update IDE context: {}", e),
                    ),
                }
            }

            "generateEditPlan" => {
                let session_id = match require_session_id(&req.id, &req.params) {
                    Ok(id) => id,
                    Err(e) => return *e,
                };
                let goal = match require_string_param(&req.id, &req.params, "goal") {
                    Ok(g) => g,
                    Err(e) => return *e,
                };
                let ide_context = req.params.get("ideContext").cloned();

                match self
                    .agent
                    .generate_edit_plan(&session_id, &goal, ide_context)
                    .await
                {
                    Ok(plan) => {
                        JsonRpcResponse::success(req.id, serde_json::json!({ "plan": plan }))
                    }
                    Err(e) => JsonRpcResponse::error(
                        req.id,
                        -32000,
                        format!("Failed to generate edit plan: {}", e),
                    ),
                }
            }

            "runValidation" => {
                let workspace_root = workspace_root(&req.params);
                let commands: Vec<String> = req
                    .params
                    .get("commands")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|v| v.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();

                let results = self
                    .agent
                    .run_validation_commands(&workspace_root, &commands)
                    .await;
                let all_passed = results.iter().all(|r| r.success);

                JsonRpcResponse::success(
                    req.id,
                    serde_json::json!({
                        "allPassed": all_passed,
                        "results": results
                    }),
                )
            }

            "getConfig" => {
                let config = self.agent.get_config();
                JsonRpcResponse::success(
                    req.id,
                    serde_json::json!({
                        "provider": config.provider,
                        "apiKeyConfigured": !config.api_key.is_empty(),
                        "model": config.model,
                        "endpoint": config.endpoint,
                        "temperature": config.temperature,
                        "timeoutSeconds": config.timeout_seconds,
                        "agentName": config.agent_name,
                        "reasoning": config.reasoning,
                        "reasoningEffort": config.reasoning_effort,
                        "shell": config.shell,
                        "maxToolIterations": config.max_tool_iterations,
                    }),
                )
            }

            "updateConfig" => {
                match self.apply_config_update(&req.id, &req.params) {
                    Ok(resp) => resp,
                    Err(resp) => *resp,
                }
            }

            "listSessions" => {
                let sessions = self.agent.list_sessions();
                JsonRpcResponse::success(req.id, serde_json::json!({ "sessions": sessions }))
            }

            "removeSession" => {
                let session_id = match require_session_id(&req.id, &req.params) {
                    Ok(id) => id,
                    Err(e) => return *e,
                };
                let removed = self.agent.remove_session(&session_id);
                JsonRpcResponse::success(
                    req.id,
                    serde_json::json!({ "removed": removed, "sessionId": session_id }),
                )
            }

            "renameSession" => {
                let session_id = match require_session_id(&req.id, &req.params) {
                    Ok(id) => id,
                    Err(e) => return *e,
                };
                let title = match require_string_param(&req.id, &req.params, "title") {
                    Ok(t) => t,
                    Err(e) => return *e,
                };
                let updated = self.agent.rename_session(&session_id, &title);
                JsonRpcResponse::success(
                    req.id,
                    serde_json::json!({ "updated": updated, "sessionId": session_id, "title": title }),
                )
            }

            "getStatus" => {
                let config = self.agent.get_config();
                let sessions = self.agent.list_sessions();
                JsonRpcResponse::success(
                    req.id,
                    serde_json::json!({
                        "provider": config.provider,
                        "model": config.model,
                        "apiKeyConfigured": !config.api_key.is_empty(),
                        "activeSessions": sessions.len(),
                    }),
                )
            }

            "getSessionMessages" => {
                let session_id = match require_session_id(&req.id, &req.params) {
                    Ok(id) => id,
                    Err(e) => return *e,
                };
                let messages = self.agent.get_session_messages(&session_id);
                let terminal_state = self
                    .agent
                    .get_session(&session_id)
                    .and_then(|s| s.metadata.get("terminalState").cloned())
                    .unwrap_or_default();
                JsonRpcResponse::success(
                    req.id,
                    serde_json::json!({
                        "sessionId": session_id,
                        "messages": messages,
                        "terminalState": terminal_state,
                    }),
                )
            }

            "saveTerminalState" => {
                let session_id = match require_session_id(&req.id, &req.params) {
                    Ok(id) => id,
                    Err(e) => return *e,
                };
                let state = match require_string_param(&req.id, &req.params, "state") {
                    Ok(s) => s,
                    Err(e) => return *e,
                };
                let saved = self.agent.save_terminal_state(&session_id, &state);
                JsonRpcResponse::success(
                    req.id,
                    serde_json::json!({ "sessionId": session_id, "saved": saved }),
                )
            }

            "applyDiff" => {
                let workspace_root = workspace_root(&req.params);
                let patch = match require_string_param(&req.id, &req.params, "patch") {
                    Ok(p) => p,
                    Err(e) => return *e,
                };
                let dry_run = req.params["dryRun"].as_bool().unwrap_or(false);
                let fuzz = req.params["fuzz"].as_u64().unwrap_or(3);

                let result = crate::tools::diff_ops::tool_apply_diff(
                    serde_json::json!({
                        "patch": patch,
                        "dry_run": dry_run,
                        "fuzz": fuzz,
                    }),
                    &workspace_root,
                )
                .await;

                if result.success {
                    // Try to parse the content as JSON for nice structured output
                    if let Ok(v) = serde_json::from_str::<Value>(&result.content) {
                        JsonRpcResponse::success(req.id, v)
                    } else {
                        JsonRpcResponse::success(req.id, serde_json::json!({ "result": result.content }))
                    }
                } else {
                    JsonRpcResponse::error(
                        req.id,
                        -32000,
                        result.error.unwrap_or_else(|| "applyDiff failed".into()),
                    )
                }
            }

            "listDiff" => {
                let workspace_root = workspace_root(&req.params);
                let result = crate::tools::diff_ops::tool_list_diff(
                    Value::Null,
                    &workspace_root,
                )
                .await;

                if result.success {
                    if let Ok(v) = serde_json::from_str::<Value>(&result.content) {
                        JsonRpcResponse::success(req.id, v)
                    } else {
                        JsonRpcResponse::success(req.id, serde_json::json!({ "files": [] }))
                    }
                } else {
                    JsonRpcResponse::error(
                        req.id,
                        -32000,
                        result.error.unwrap_or_else(|| "listDiff failed".into()),
                    )
                }
            }

            "getFileDiff" => {
                let workspace_root = workspace_root(&req.params);
                let path = match require_string_param(&req.id, &req.params, "path") {
                    Ok(p) => p,
                    Err(e) => return *e,
                };
                let result = crate::tools::diff_ops::tool_get_file_diff(
                    serde_json::json!({ "path": path }),
                    &workspace_root,
                )
                .await;

                if result.success {
                    if let Ok(v) = serde_json::from_str::<Value>(&result.content) {
                        JsonRpcResponse::success(req.id, v)
                    } else {
                        JsonRpcResponse::success(req.id, serde_json::json!({ "path": path, "patch": "" }))
                    }
                } else {
                    JsonRpcResponse::error(
                        req.id,
                        -32000,
                        result.error.unwrap_or_else(|| "getFileDiff failed".into()),
                    )
                }
            }

            "shutdown" => {
                tracing::info!("Shutdown requested via RPC");
                JsonRpcResponse::success(req.id, serde_json::json!({"status": "shutting_down"}))
            }

            _ => JsonRpcResponse::error(
                req.id,
                -32601,
                format!("Method not found: {}", req.method),
            ),
        }
    }

    /// Apply a config update from the params. Extracted to keep handle_request lean.
    fn apply_config_update(
        &mut self,
        id: &Option<Value>,
        params: &Value,
    ) -> Result<JsonRpcResponse, Box<JsonRpcResponse>> {
        let mut config = self.agent.get_config().clone();

        if let Some(v) = params.get("provider").and_then(|v| v.as_str()) {
            config.provider = v.to_string();
        }
        if let Some(v) = params.get("apiKey").and_then(|v| v.as_str()) {
            let trimmed = v.trim();
            let is_placeholder =
                trimmed.is_empty() || trimmed == "••••••••" || trimmed.contains('•') || trimmed.contains('●');
            if !is_placeholder {
                config.api_key = trimmed.to_string();
            }
        }
        if let Some(v) = params.get("model").and_then(|v| v.as_str()) {
            config.model = v.to_string();
        }
        if let Some(v) = params.get("endpoint").and_then(|v| v.as_str()) {
            config.endpoint = v.to_string();
        }
        if let Some(v) = params.get("temperature").and_then(|v| v.as_f64()) {
            config.temperature = v as f32;
        }
        if let Some(v) = params.get("timeoutSeconds").and_then(|v| v.as_u64()) {
            config.timeout_seconds = v;
        }
        if let Some(v) = params.get("reasoning").and_then(|v| v.as_bool()) {
            config.reasoning = v;
        }
        if let Some(v) = params.get("reasoningEffort").and_then(|v| v.as_str()) {
            config.reasoning_effort = map_reasoning_effort(v);
        }
        if let Some(v) = params.get("shell").and_then(|v| v.as_str()) {
            config.shell = v.to_string();
        }
        if let Some(v) = params.get("maxToolIterations").and_then(|v| v.as_u64()) {
            config.max_tool_iterations = v as usize;
        }

        if let Err(e) = config.validate() {
            return Err(Box::new(JsonRpcResponse::error(
                id.clone(),
                -32602,
                format!("Config validation failed: {}", e),
            )));
        }

        config
            .save("config")
            .map_err(|e| {
                Box::new(JsonRpcResponse::error(id.clone(), -32000, format!("Failed to save config: {}", e)))
            })?;

        self.agent
            .reconfigure(config)
            .map_err(|e| {
                Box::new(JsonRpcResponse::error(
                    id.clone(),
                    -32000,
                    format!("Failed to reconfigure agent: {}", e),
                ))
            })?;

        Ok(JsonRpcResponse::success(
            id.clone(),
            serde_json::json!({"status": "ok", "message": "設定已更新"}),
        ))
    }
}
