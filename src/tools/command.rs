use crate::tools::{ToolEvent, ToolRegistry, ToolResult};
use serde_json::Value;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;

// ---------------------------------------------------------------------------
// Command execution tool
// ---------------------------------------------------------------------------

/// Check whether a shell command contains dangerous patterns that could
/// cause irreversible damage.  Returns an error message when the command
/// is blocked; `None` when the command is allowed.
fn check_dangerous_command(command: &str) -> Option<String> {
    let lower = command.to_lowercase();

    // Destructive filesystem operations (recursive / force)
    let dangerous_patterns: &[(&str, &str)] = &[
        ("rm -rf", "遞迴強制刪除 (rm -rf)"),
        ("rm -r ", "遞迴刪除 (rm -r)"),
        ("del /f", "強制刪除 (del /f)"),
        ("del /s", "遞迴刪除 (del /s)"),
        ("rd /s", "遞迴刪除目錄 (rd /s)"),
        ("rmdir /s", "遞迴刪除目錄 (rmdir /s)"),
        ("deltree", "刪除目錄樹 (deltree)"),
        ("format ", "磁碟格式化 (format)"),
        ("mkfs.", "檔案系統格式化 (mkfs)"),
        ("dd if=", "磁碟直接寫入 (dd)"),
        ("> /dev/sd", "寫入磁碟裝置"),
    ];

    for (pattern, description) in dangerous_patterns {
        if lower.contains(pattern) {
            return Some(format!(
                "🚨 安全性攔截: 指令包含危險操作 '{}' ({})。\
                 此操作可能造成不可逆的資料損失，已被拒絕執行。\n\
                 若確需執行此操作，請在終端機中手動執行。",
                pattern, description
            ));
        }
    }

    None
}

pub async fn tool_execute_command(
    registry: &ToolRegistry,
    tool_name: &str,
    args: Value,
    workspace_root: &str,
) -> ToolResult {
    let command = args["command"].as_str().unwrap_or("echo 'No command'");

    if let Some(block_msg) = check_dangerous_command(command) {
        return ToolResult {
            success: false,
            content: String::new(),
            error: Some(block_msg),
        };
    }
    let working_dir = args["working_dir"].as_str().unwrap_or(workspace_root);

    registry.emit_event(ToolEvent::command_started(tool_name, command));

    // Build process from the configurable shell template.
    // Supports two forms:
    //   1) "<program> <arg> {cmd} <arg...>"  → arg list with {cmd} replaced
    //   2) plain "cmd /C"                   → legacy: args as written + command
    let template = registry.shell_template().to_string();
    let resolved = if template.contains("{cmd}") {
        template.replace("{cmd}", command)
    } else {
        // Backward-compat: treat everything as args + command
        format!("{} {}", template, command)
    };

    let mut parts = shlex::split(&resolved).unwrap_or_else(|| {
        // Fallback: split by whitespace (handles cases like missing quotes)
        resolved.split_whitespace().map(|s| s.to_string()).collect()
    });

    let mut process = if parts.is_empty() {
        // Should never happen, but fall back to cmd on Windows
        let mut p = if cfg!(target_os = "windows") {
            tokio::process::Command::new("cmd")
        } else {
            tokio::process::Command::new("sh")
        };
        p.arg(command);
        p
    } else {
        let program = parts.remove(0);
        let mut p = tokio::process::Command::new(program);
        p.args(&parts);
        p
    };
    process
        .current_dir(working_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    match process.spawn() {
        Ok(mut child) => {
            let (line_tx, mut line_rx) = mpsc::unbounded_channel::<(String, String)>();

            if let Some(stdout) = child.stdout.take() {
                let line_tx = line_tx.clone();
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stdout).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        if line_tx.send(("stdout".to_string(), line)).is_err() {
                            break;
                        }
                    }
                });
            }

            if let Some(stderr) = child.stderr.take() {
                let line_tx = line_tx.clone();
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stderr).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        if line_tx.send(("stderr".to_string(), line)).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(line_tx);

            let mut stdout = String::new();
            let mut stderr = String::new();
            while let Some((stream, line)) = line_rx.recv().await {
                let output_line = format!("{}\n", line);
                if stream == "stderr" {
                    stderr.push_str(&output_line);
                } else {
                    stdout.push_str(&output_line);
                }
                registry.emit_event(ToolEvent::output(
                    tool_name,
                    &stream,
                    output_line,
                ));
            }

            match child.wait().await {
                Ok(status) => {
                    let exit_code = status.code().unwrap_or(-1);
                    let success = status.success();
                    let mut result = String::new();
                    if !stdout.is_empty() {
                        result.push_str(&format!("STDOUT:\n{}\n", stdout));
                    }
                    if !stderr.is_empty() {
                        result.push_str(&format!("STDERR:\n{}\n", stderr));
                    }
                    if !success {
                        result.push_str(&format!("Exit code: {}\n", exit_code));
                    }

                    registry.emit_event(ToolEvent::completed(
                        tool_name,
                        exit_code,
                        success,
                        None,
                    ));
                    ToolResult {
                        success,
                        content: result,
                        error: None,
                    }
                }
                Err(error) => {
                    let output = format!("{}{}", stdout, stderr);
                    registry.emit_event(ToolEvent::completed(
                        tool_name,
                        -1,
                        false,
                        Some(output.clone()),
                    ));
                    ToolResult {
                        success: false,
                        content: output,
                        error: Some(format!("Process error: {}", error)),
                    }
                }
            }
        }
        Err(error) => {
            registry.emit_event(ToolEvent::completed(tool_name, -1, false, None));
            ToolResult {
                success: false,
                content: String::new(),
                error: Some(format!("Failed to execute command: {}", error)),
            }
        }
    }
}