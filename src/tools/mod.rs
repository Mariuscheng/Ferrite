pub mod command;
pub mod diff_ops;
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
    ApplyDiff,
    ListDiff,
    GetFileDiff,
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
            ToolName::ApplyDiff => "apply_diff",
            ToolName::ListDiff => "list_diff",
            ToolName::GetFileDiff => "get_file_diff",
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
            "apply_diff" => Ok(ToolName::ApplyDiff),
            "list_diff" => Ok(ToolName::ListDiff),
            "get_file_diff" => Ok(ToolName::GetFileDiff),
            other => Err(format!("Unknown tool: {}", other)),
        }
    }
}

