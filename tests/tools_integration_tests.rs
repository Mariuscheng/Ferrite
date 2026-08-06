use ferrite::tools::{ToolName, ToolRegistry, ToolResult};
use serde_json::json;
use std::path::PathBuf;

/// Create a unique temporary workspace directory.
fn temp_workspace(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "ferrite-tools-test-{}-{}",
        std::process::id(),
        tag
    ));
    if dir.exists() {
        let _ = std::fs::remove_dir_all(&dir);
    }
    std::fs::create_dir_all(&dir).expect("create temp workspace");
    dir
}

fn workspace_str(workspace: &PathBuf) -> &str {
    workspace.to_str().expect("valid utf8 path")
}

#[tokio::test]
async fn read_file_tool_reads_content_with_line_numbers() {
    let ws = temp_workspace("read");
    std::fs::write(ws.join("hello.txt"), "line one\nline two\n").expect("write file");

    let registry = ToolRegistry::new();
    let result = registry
        .execute(
            ToolName::ReadFile,
            json!({"path": "hello.txt"}),
            workspace_str(&ws),
        )
        .await;

    assert!(result.success, "read should succeed: {:?}", result.error);
    assert!(result.content.contains("1 | line one"));
    assert!(result.content.contains("2 | line two"));
}

#[tokio::test]
async fn read_file_tool_rejects_path_traversal() {
    let ws = temp_workspace("read-traversal");

    let registry = ToolRegistry::new();
    let result = registry
        .execute(
            ToolName::ReadFile,
            json!({"path": "../../etc/passwd"}),
            workspace_str(&ws),
        )
        .await;

    assert!(!result.success);
    assert!(
        result.error.as_deref().unwrap_or("").contains("試圖存取工作區以外"),
        "got: {:?}",
        result.error
    );
}

#[tokio::test]
async fn read_file_tool_rejects_absolute_path() {
    let ws = temp_workspace("read-absolute");

    let registry = ToolRegistry::new();
    let result = registry
        .execute(
            ToolName::ReadFile,
            json!({"path": "/etc/passwd"}),
            workspace_str(&ws),
        )
        .await;

    assert!(!result.success);
    assert!(
        result.error.as_deref().unwrap_or("").contains("不允許使用絕對路徑"),
        "got: {:?}",
        result.error
    );
}

#[tokio::test]
async fn read_file_tool_missing_file_returns_error() {
    let ws = temp_workspace("read-missing");

    let registry = ToolRegistry::new();
    let result = registry
        .execute(
            ToolName::ReadFile,
            json!({"path": "does-not-exist.txt"}),
            workspace_str(&ws),
        )
        .await;

    assert!(!result.success);
    assert!(result.error.is_some());
}

#[tokio::test]
async fn write_file_tool_creates_file_and_directory() {
    let ws = temp_workspace("write");

    let registry = ToolRegistry::new();
    let result = registry
        .execute(
            ToolName::WriteFile,
            json!({"path": "src/main.rs", "content": "fn main() {}\n"}),
            workspace_str(&ws),
        )
        .await;

    assert!(result.success, "write should succeed: {:?}", result.error);
    let content = std::fs::read_to_string(ws.join("src/main.rs")).expect("file exists");
    assert_eq!(content, "fn main() {}\n");
}

#[tokio::test]
async fn replace_in_file_single_block_edit() {
    let ws = temp_workspace("replace-single");
    std::fs::write(ws.join("data.txt"), "foo bar baz").expect("write file");

    let registry = ToolRegistry::new();
    let result = registry
        .execute(
            ToolName::ReplaceInFile,
            json!({
                "path": "data.txt",
                "search": "bar",
                "replace": "qux"
            }),
            workspace_str(&ws),
        )
        .await;

    assert!(result.success, "replace should succeed: {:?}", result.error);
    let content = std::fs::read_to_string(ws.join("data.txt")).expect("file exists");
    assert_eq!(content, "foo qux baz");
}

#[tokio::test]
async fn replace_in_file_multi_block_edit_applies_sequentially() {
    let ws = temp_workspace("replace-multi");
    std::fs::write(ws.join("data.txt"), "alpha\nbeta\ngamma\n").expect("write file");

    let registry = ToolRegistry::new();
    let result = registry
        .execute(
            ToolName::ReplaceInFile,
            json!({
                "path": "data.txt",
                "diff": [
                    {"search": "alpha", "replace": "ALPHA"},
                    {"search": "gamma", "replace": "GAMMA"}
                ]
            }),
            workspace_str(&ws),
        )
        .await;

    assert!(result.success, "replace should succeed: {:?}", result.error);
    let content = std::fs::read_to_string(ws.join("data.txt")).expect("file exists");
    assert_eq!(content, "ALPHA\nbeta\nGAMMA\n");
}

#[tokio::test]
async fn replace_in_file_failed_search_returns_error() {
    let ws = temp_workspace("replace-fail");
    std::fs::write(ws.join("data.txt"), "existing content").expect("write file");

    let registry = ToolRegistry::new();
    let result = registry
        .execute(
            ToolName::ReplaceInFile,
            json!({
                "path": "data.txt",
                "search": "not present",
                "replace": "replacement"
            }),
            workspace_str(&ws),
        )
        .await;

    assert!(!result.success);
    assert!(
        result.error.as_deref().unwrap_or("").contains("search text not found"),
        "got: {:?}",
        result.error
    );
}

#[tokio::test]
async fn list_files_tool_lists_directory_contents() {
    let ws = temp_workspace("list");
    std::fs::create_dir_all(ws.join("src")).expect("create dir");
    std::fs::write(ws.join("Cargo.toml"), "[package]\n").expect("write file");
    std::fs::write(ws.join("src/main.rs"), "fn main() {}\n").expect("write file");

    let registry = ToolRegistry::new();
    let result = registry
        .execute(
            ToolName::ListFiles,
            json!({"path": ".", "recursive": true}),
            workspace_str(&ws),
        )
        .await;

    assert!(result.success, "list should succeed: {:?}", result.error);
    assert!(result.content.contains("Cargo.toml"));
    assert!(result.content.contains("src/"));
    assert!(result.content.contains("main.rs"));
}

#[tokio::test]
async fn search_files_tool_finds_matching_lines() {
    let ws = temp_workspace("search");
    std::fs::write(ws.join("a.rs"), "fn main() {\n    println!(\"hello\");\n}\n").expect("write");
    std::fs::write(ws.join("b.txt"), "no match here\n").expect("write");

    let registry = ToolRegistry::new();
    let result = registry
        .execute(
            ToolName::SearchFiles,
            json!({"pattern": "println", "path": "."}),
            workspace_str(&ws),
        )
        .await;

    assert!(result.success, "search should succeed: {:?}", result.error);
    assert!(result.content.contains("println"));
    assert!(result.content.contains("a.rs"));
    assert!(!result.content.contains("no match here"));
}

#[tokio::test]
async fn search_files_tool_invalid_regex_returns_error() {
    let ws = temp_workspace("search-invalid");

    let registry = ToolRegistry::new();
    let result = registry
        .execute(
            ToolName::SearchFiles,
            json!({"pattern": "(", "path": "."}),
            workspace_str(&ws),
        )
        .await;

    assert!(!result.success);
    assert!(
        result.error.as_deref().unwrap_or("").contains("Invalid regex"),
        "got: {:?}",
        result.error
    );
}

#[tokio::test]
async fn execute_command_tool_runs_safe_command() {
    let ws = temp_workspace("cmd");

    let registry = ToolRegistry::new();
    let command = if cfg!(target_os = "windows") {
        "echo hello-test-123"
    } else {
        "echo hello-test-123"
    };
    let result = registry
        .execute(
            ToolName::ExecuteCommand,
            json!({"command": command}),
            workspace_str(&ws),
        )
        .await;

    assert!(result.success, "command should succeed: {:?}", result.error);
    assert!(
        result.content.contains("hello-test-123"),
        "output should contain marker: {:?}",
        result.content
    );
}

#[tokio::test]
async fn execute_command_tool_rejects_dangerous_command() {
    let ws = temp_workspace("cmd-dangerous");

    let registry = ToolRegistry::new();
    let result = registry
        .execute(
            ToolName::ExecuteCommand,
            json!({"command": "rm -rf /"}),
            workspace_str(&ws),
        )
        .await;

    assert!(!result.success);
    assert!(
        result.error.as_deref().unwrap_or("").contains("安全性攔截"),
        "got: {:?}",
        result.error
    );
}

#[tokio::test]
async fn apply_diff_tool_previews_and_applies_patch() {
    let ws = temp_workspace("diff");
    std::fs::write(ws.join("example.txt"), "one\ntwo\nthree\n").expect("write file");

    let patch = "--- a/example.txt\n+++ b/example.txt\n@@ -1,3 +1,3 @@\n one\n-two\n+2\n three\n";

    let registry = ToolRegistry::new();

    // Dry-run first: must not modify the file.
    let preview = registry
        .execute(
            ToolName::ApplyDiff,
            json!({"patch": patch, "dry_run": true}),
            workspace_str(&ws),
        )
        .await;
    assert!(preview.success, "preview should succeed: {:?}", preview.error);
    let original_after_preview = std::fs::read_to_string(ws.join("example.txt")).expect("file");
    assert_eq!(original_after_preview, "one\ntwo\nthree\n");

    // Apply for real.
    let applied = registry
        .execute(
            ToolName::ApplyDiff,
            json!({"patch": patch, "dry_run": false}),
            workspace_str(&ws),
        )
        .await;
    assert!(applied.success, "apply should succeed: {:?}", applied.error);
    let modified = std::fs::read_to_string(ws.join("example.txt")).expect("file");
    // The diff engine joins lines without preserving a trailing newline.
    assert_eq!(modified, "one\n2\nthree");
}

#[tokio::test]
async fn apply_diff_tool_empty_patch_returns_error() {
    let ws = temp_workspace("diff-empty");

    let registry = ToolRegistry::new();
    let result = registry
        .execute(
            ToolName::ApplyDiff,
            json!({"patch": "", "dry_run": true}),
            workspace_str(&ws),
        )
        .await;

    assert!(!result.success);
    assert!(
        result.error.as_deref().unwrap_or("").contains("patch 參數為空"),
        "got: {:?}",
        result.error
    );
}

#[tokio::test]
async fn create_project_tool_scaffolds_rust_project() {
    let ws = temp_workspace("project");

    let registry = ToolRegistry::new();
    let result = registry
        .execute(
            ToolName::CreateProject,
            json!({
                "project_type": "rust",
                "name": "my-demo-app",
                "path": "."
            }),
            workspace_str(&ws),
        )
        .await;

    assert!(result.success, "create should succeed: {:?}", result.error);
    assert!(ws.join("my-demo-app/Cargo.toml").exists());
    assert!(ws.join("my-demo-app/src/main.rs").exists());
}

#[tokio::test]
async fn create_project_tool_rejects_existing_directory() {
    let ws = temp_workspace("project-existing");
    std::fs::create_dir_all(ws.join("dup")).expect("create dir");

    let registry = ToolRegistry::new();
    let result = registry
        .execute(
            ToolName::CreateProject,
            json!({
                "project_type": "rust",
                "name": "dup",
                "path": "."
            }),
            workspace_str(&ws),
        )
        .await;

    assert!(!result.success);
    assert!(
        result.error.as_deref().unwrap_or("").contains("already exists"),
        "got: {:?}",
        result.error
    );
}

#[tokio::test]
async fn create_project_tool_unsupported_type_returns_error() {
    let ws = temp_workspace("project-unsupported");

    let registry = ToolRegistry::new();
    let result = registry
        .execute(
            ToolName::CreateProject,
            json!({
                "project_type": "cobol",
                "name": "legacy",
                "path": "."
            }),
            workspace_str(&ws),
        )
        .await;

    assert!(!result.success);
    assert!(
        result.error.as_deref().unwrap_or("").contains("Unsupported project type"),
        "got: {:?}",
        result.error
    );
}

#[test]
fn tool_name_parsing_roundtrip() {
    use std::str::FromStr;

    for name in [
        "read_file",
        "write_file",
        "replace_in_file",
        "search_files",
        "list_files",
        "execute_command",
        "create_project",
        "compile",
        "run_tests",
        "apply_diff",
    ] {
        let parsed = ToolName::from_str(name).expect("parse tool name");
        assert_eq!(parsed.as_str(), name);
    }

    assert!(ToolName::from_str("unknown_tool").is_err());
}

#[test]
fn tool_result_serializes_for_frontend() {
    let result = ToolResult {
        success: true,
        content: "output".to_string(),
        error: None,
    };
    let json = serde_json::to_value(&result).expect("serialize");
    assert_eq!(json["success"], true);
    assert_eq!(json["content"], "output");
}