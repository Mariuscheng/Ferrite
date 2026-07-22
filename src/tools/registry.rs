use crate::tools::{ToolDefinition, ToolEvent, ToolEventSink, ToolName, ToolResult};
use serde_json::Value;

/// Registry of available tools for the AI agent
pub struct ToolRegistry {
    tools: Vec<ToolDefinition>,
    event_sink: Option<ToolEventSink>,
    /// Shell command template for execute_command.  `{cmd}` is replaced
    /// with the actual command.  Default auto-detected from OS.
    shell_template: String,
}

impl ToolRegistry {
    pub fn new() -> Self {
        let tools = vec![
            ToolDefinition {
                name: "read_file".into(),
                description: "Read the contents of a file at the specified path.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The path to the file to read, relative to the workspace root"
                        },
                        "start_line": {
                            "type": "integer",
                            "description": "Optional start line (1-based)"
                        },
                        "end_line": {
                            "type": "integer",
                            "description": "Optional end line (inclusive)"
                        }
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "write_file".into(),
                description: "Write content to a file, creating it if it doesn't exist.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The path to write to, relative to the workspace root"
                        },
                        "content": {
                            "type": "string",
                            "description": "The content to write to the file"
                        }
                    },
                    "required": ["path", "content"]
                }),
            },
            ToolDefinition {
                name: "replace_in_file".into(),
                description: "Perform exact string replacements in an existing file. \
Supports both single-block mode (search + replace) and multi-block mode (diff array). \
When editing text, ensure you preserve the exact indentation (tabs/spaces) as it appears before. \
\n\n**Single-block:** Provide 'search' and 'replace' strings. \
\n**Multi-block:** Provide a 'diff' array of {search, replace} objects — each pair is applied \
sequentially to the same file. This is the preferred way to apply several independent edits \
without sending the entire file.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The path to the file to modify"
                        },
                        "diff": {
                            "type": "array",
                            "description": "Optional array of {search, replace} objects for multi-block editing (preferred)",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "search": { "type": "string", "description": "The exact text to find" },
                                    "replace": { "type": "string", "description": "The text to replace it with" }
                                },
                                "required": ["search", "replace"]
                            }
                        },
                        "search": {
                            "type": "string",
                            "description": "The text to replace (single-block mode)"
                        },
                        "replace": {
                            "type": "string",
                            "description": "The text to replace it with (single-block mode)"
                        }
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "search_files".into(),
                description: "Search for a pattern across files in the workspace.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "The regex pattern to search for"
                        },
                        "path": {
                            "type": "string",
                            "description": "The directory path to search in (relative to workspace root)"
                        },
                        "file_pattern": {
                            "type": "string",
                            "description": "Optional file pattern filter (e.g., '*.rs', '*.ts')"
                        }
                    },
                    "required": ["pattern"]
                }),
            },
            ToolDefinition {
                name: "list_files".into(),
                description: "List files and directories in the specified workspace path.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "path": {
                            "type": "string",
                            "description": "The directory to list, relative to workspace root"
                        },
                        "recursive": {
                            "type": "boolean",
                            "description": "Whether to list files recursively"
                        }
                    },
                    "required": ["path"]
                }),
            },
            ToolDefinition {
                name: "execute_command".into(),
                description: "Execute a shell command in the workspace and return the output."
                    .into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The command to execute"
                        },
                        "working_dir": {
                            "type": "string",
                            "description": "Optional working directory for the command"
                        }
                    },
                    "required": ["command"]
                }),
            },
            ToolDefinition {
                name: "create_project".into(),
                description: "Scaffold a new project with basic structure.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "project_type": {
                            "type": "string",
                            "description": "Type of project (e.g., 'rust', 'typescript', 'python', 'web')"
                        },
                        "name": {
                            "type": "string",
                            "description": "Name of the project"
                        },
                        "path": {
                            "type": "string",
                            "description": "Path where to create the project"
                        }
                    },
                    "required": ["project_type", "name", "path"]
                }),
            },
            ToolDefinition {
                name: "compile".into(),
                description: "Compile the project and report any errors.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The build/compile command to run (e.g., 'cargo build', 'npm run build')"
                        }
                    },
                    "required": ["command"]
                }),
            },
            ToolDefinition {
                name: "run_tests".into(),
                description: "Run the project's test suite.".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The test command to run (e.g., 'cargo test', 'npm test')"
                        }
                    },
                    "required": ["command"]
                }),
            },
        ];

        let shell_template = if cfg!(target_os = "windows") {
            "cmd /C {cmd}".to_string()
        } else {
            "sh -c {cmd}".to_string()
        };

        Self {
            tools,
            event_sink: None,
            shell_template,
        }
    }

    pub fn with_event_sink(event_sink: ToolEventSink) -> Self {
        let mut registry = Self::new();
        registry.event_sink = Some(event_sink);
        registry
    }

    /// Get all tool definitions (for sending to AI API)
    pub fn get_definitions(&self) -> &[ToolDefinition] {
        &self.tools
    }

    pub fn emit_event(&self, event: ToolEvent) {
        if let Some(event_sink) = &self.event_sink {
            event_sink(event);
        }
    }

    /// Update the shell command template for execute_command.
    /// `{cmd}` will be replaced with the actual command text.
    pub fn set_shell_template(&mut self, template: String) {
        self.shell_template = template;
    }

    /// Return the currently active shell command template.
    pub fn shell_template(&self) -> &str {
        &self.shell_template
    }

    /// Resolve a user-supplied path safely within the workspace root.
    /// Returns an error if the path attempts to escape the workspace via
    /// parent-directory traversal (`..`) or absolute-prefix tricks.
    pub fn resolve_workspace_path(
        workspace_root: &str,
        user_path: &str,
        tool_name: &str,
    ) -> Result<std::path::PathBuf, String> {
        let root = std::path::Path::new(workspace_root);

        // Reject obviously malicious absolute paths passed as "relative".
        let normalized = user_path.replace('\\', "/");
        if normalized.starts_with('/')
            || (normalized.len() > 2 && normalized.as_bytes()[1] == b':')
        {
            return Err(format!(
                "[{}] 不允許使用絕對路徑: '{}'",
                tool_name, user_path
            ));
        }

        // Manually resolve `..` segments without touching the filesystem,
        // so the check works even for non-existent paths inside the workspace.
        let mut components: Vec<&str> = Vec::new();
        for segment in normalized.split('/') {
            match segment {
                "" | "." => continue,
                ".." => {
                    if components.is_empty() {
                        return Err(format!(
                            "[{}] 路徑 '{}' 試圖存取工作區以外的檔案。這是不允許的。",
                            tool_name, user_path
                        ));
                    }
                    components.pop();
                }
                other => components.push(other),
            }
        }

        let relative: std::path::PathBuf = components.iter().collect();
        let full = root.join(&relative);

        // Canonicalize the workspace root for a clean comparison baseline.
        let canonical_root = root.canonicalize().map_err(|e| {
            format!(
                "[{}] 無法存取工作區根目錄 '{}': {}",
                tool_name, workspace_root, e
            )
        })?;

        // If the resolved path exists, canonicalize it for comparison.
        // Otherwise, manually verify it doesn't escape the root.
        if full.exists() {
            let canonical_full = full.canonicalize().map_err(|e| {
                format!("[{}] 無法解析路徑 '{}': {}", tool_name, user_path, e)
            })?;
            if !canonical_full.starts_with(&canonical_root) {
                return Err(format!(
                    "[{}] 路徑 '{}' 試圖存取工作區以外的檔案。這是不允許的。",
                    tool_name, user_path
                ));
            }
            return Ok(canonical_full);
        }

        // Non-existent path: verify it stays inside workspace by checking
        // the manually resolved components.
        let canonical_full = root.join(&relative);
        // Attempt to canonicalize the parent directory to verify boundaries.
        if let Some(parent) = canonical_full.parent() {
            if parent.exists() {
                let canonical_parent = parent.canonicalize().map_err(|e| {
                    format!(
                        "[{}] 無法解析路徑 '{}' 的父目錄: {}",
                        tool_name, user_path, e
                    )
                })?;
                if !canonical_parent.starts_with(&canonical_root) {
                    return Err(format!(
                        "[{}] 路徑 '{}' 試圖存取工作區以外的檔案。這是不允許的。",
                        tool_name, user_path
                    ));
                }
            }
        }

        Ok(canonical_full)
    }

    /// Execute a tool by its enum name.
    pub async fn execute(
        &self,
        name: ToolName,
        args: Value,
        workspace_root: &str,
    ) -> ToolResult {
        match name {
            ToolName::ReadFile => {
                crate::tools::file_ops::tool_read_file(args, workspace_root).await
            }
            ToolName::WriteFile => {
                crate::tools::file_ops::tool_write_file(args, workspace_root).await
            }
            ToolName::ReplaceInFile => {
                crate::tools::file_ops::tool_replace_in_file(args, workspace_root).await
            }
            ToolName::SearchFiles => {
                crate::tools::file_ops::tool_search_files(args, workspace_root).await
            }
            ToolName::ListFiles => {
                crate::tools::file_ops::tool_list_files(args, workspace_root).await
            }
            ToolName::ExecuteCommand => {
                crate::tools::command::tool_execute_command(
                    self,
                    name.as_str(),
                    args,
                    workspace_root,
                )
                .await
            }
            ToolName::CreateProject => {
                crate::tools::project::tool_create_project(args, workspace_root).await
            }
            ToolName::Compile => {
                crate::tools::command::tool_execute_command(
                    self,
                    name.as_str(),
                    serde_json::json!({"command": args["command"].as_str().unwrap_or("cargo build")}),
                    workspace_root,
                )
                .await
            }
            ToolName::RunTests => {
                crate::tools::command::tool_execute_command(
                    self,
                    name.as_str(),
                    serde_json::json!({"command": args["command"].as_str().unwrap_or("cargo test")}),
                    workspace_root,
                )
                .await
            }
        }
    }
}