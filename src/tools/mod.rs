pub mod command;
pub mod file_ops;
pub mod project;
pub mod registry;

// Re-export Registry so external code continues to work with the same import path.
pub use registry::ToolRegistry;

use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Public types shared across all tool modules
// ---------------------------------------------------------------------------

pub type ToolEventSink = Arc<dyn Fn(ToolEvent) + Send + Sync>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub tool: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
}

impl ToolEvent {
    fn command_started(tool: &str, command: &str) -> Self {
        Self {
            event_type: "toolStart".to_string(),
            tool: tool.to_string(),
            command: Some(command.to_string()),
            stream: None,
            output: None,
            exit_code: None,
            success: None,
        }
    }

    fn output(tool: &str, stream: &str, output: String) -> Self {
        Self {
            event_type: "toolOutput".to_string(),
            tool: tool.to_string(),
            command: None,
            stream: Some(stream.to_string()),
            output: Some(output),
            exit_code: None,
            success: None,
        }
    }

    fn completed(tool: &str, exit_code: i32, success: bool, output: Option<String>) -> Self {
        Self {
            event_type: "toolComplete".to_string(),
            tool: tool.to_string(),
            command: None,
            stream: None,
            output,
            exit_code: Some(exit_code),
            success: Some(success),
        }
    }
}

/// Represents a tool that the AI agent can use to interact with code
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// Result of executing a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub success: bool,
    pub content: String,
    pub error: Option<String>,
}

// ---------------------------------------------------------------------------
// ToolName enum — compile-time safe tool routing
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolName {
    ReadFile,
    WriteFile,
    ReplaceInFile,
    SearchFiles,
    ListFiles,
    ExecuteCommand,
    CreateProject,
    Compile,
    RunTests,
}

impl ToolName {
    /// Return the canonical string name used in the tool registry / API.
    pub fn as_str(&self) -> &'static str {
        match self {
            ToolName::ReadFile => "read_file",
            ToolName::WriteFile => "write_file",
            ToolName::ReplaceInFile => "replace_in_file",
            ToolName::SearchFiles => "search_files",
            ToolName::ListFiles => "list_files",
            ToolName::ExecuteCommand => "execute_command",
            ToolName::CreateProject => "create_project",
            ToolName::Compile => "compile",
            ToolName::RunTests => "run_tests",
        }
    }
}

impl FromStr for ToolName {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "read_file" => Ok(ToolName::ReadFile),
            "write_file" => Ok(ToolName::WriteFile),
            "replace_in_file" => Ok(ToolName::ReplaceInFile),
            "search_files" => Ok(ToolName::SearchFiles),
            "list_files" => Ok(ToolName::ListFiles),
            "execute_command" => Ok(ToolName::ExecuteCommand),
            "create_project" => Ok(ToolName::CreateProject),
            "compile" => Ok(ToolName::Compile),
            "run_tests" => Ok(ToolName::RunTests),
            other => Err(format!("Unknown tool: {}", other)),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolRegistry;
    use std::sync::Arc;

    #[test]
    fn path_traversal_blocked() {
        let result =
            ToolRegistry::resolve_workspace_path("/tmp/workspace", "../etc/passwd", "test");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("試圖存取工作區以外的檔案"), "got: {}", err);
    }

    #[test]
    fn path_traversal_in_subdir_blocked() {
        let result = ToolRegistry::resolve_workspace_path(
            "/tmp/workspace",
            "subdir/../../../etc/passwd",
            "test",
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("試圖存取工作區以外的檔案"), "got: {}", err);
    }

    #[test]
    fn allowed_path_inside_workspace() {
        let result = ToolRegistry::resolve_workspace_path(".", "src/main.rs", "test");
        assert!(result.is_ok());
    }

    #[test]
    fn command_execution_streams_tool_events() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let events_clone = Arc::clone(&events);
        let sink: ToolEventSink = Arc::new(move |event| {
            events_clone.lock().unwrap().push(event);
        });

        let runtime = tokio::runtime::Runtime::new().unwrap();
        let _guard = runtime.enter();

        let registry = ToolRegistry::with_event_sink(sink);

        runtime.block_on(async {
            let result = registry
                .execute(
                    ToolName::ExecuteCommand,
                    serde_json::json!({"command": if cfg!(windows) { "echo hello" } else { "echo hello" }}),
                    ".",
                )
                .await;

            assert!(result.success, "command should succeed: {:?}", result.error);
            assert!(
                result.content.contains("hello"),
                "output should contain 'hello'"
            );

            let captured = events.lock().unwrap().clone();
            assert!(!captured.is_empty(), "should have emitted events");

            let start_event = captured.iter().find(|e| e.event_type == "toolStart");
            assert!(start_event.is_some(), "should emit toolStart event");

            let complete_event = captured.iter().find(|e| e.event_type == "toolComplete");
            assert!(complete_event.is_some(), "should emit toolComplete event");
            assert_eq!(complete_event.unwrap().success, Some(true));

            let output_event = captured.iter().find(|e| e.event_type == "toolOutput");
            if let Some(ev) = output_event {
                assert!(
                    ev.output.as_ref().unwrap().contains("hello"),
                    "toolOutput should contain 'hello'"
                );
            }
        });
    }
}