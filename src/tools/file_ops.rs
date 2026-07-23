use crate::tools::{ToolRegistry, ToolResult};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Read / Write / Replace / Search / List — file-system tools
// ---------------------------------------------------------------------------

pub async fn tool_read_file(args: Value, workspace_root: &str) -> ToolResult {
    let path = args["path"].as_str().unwrap_or("");

    let full_path =
        match ToolRegistry::resolve_workspace_path(workspace_root, path, "read_file") {
            Ok(p) => p,
            Err(e) => {
                return ToolResult {
                    success: false,
                    content: String::new(),
                    error: Some(e),
                }
            }
        };

    match std::fs::read_to_string(&full_path) {
        Ok(content) => {
            let start_line = args["start_line"].as_u64().unwrap_or(1) as usize;
            let end_line = args["end_line"]
                .as_u64()
                .unwrap_or(usize::MAX as u64) as usize;

            let lines: Vec<&str> = content.lines().collect();
            let total_lines = lines.len();
            let start = start_line.min(total_lines).max(1) - 1;
            let end = end_line.min(total_lines).max(start);

            let formatted: String = lines[start..end]
                .iter()
                .enumerate()
                .map(|(i, line)| format!("{:>4} | {}\n", start + i + 1, line))
                .collect();

            ToolResult {
                success: true,
                content: formatted,
                error: None,
            }
        }
        Err(e) => ToolResult {
            success: false,
            content: String::new(),
            error: Some(format!("Failed to read file '{}': {}", path, e)),
        },
    }
}

pub async fn tool_write_file(args: Value, workspace_root: &str) -> ToolResult {
    let path = args["path"].as_str().unwrap_or("");
    let content = args["content"].as_str().unwrap_or("");

    let full_path =
        match ToolRegistry::resolve_workspace_path(workspace_root, path, "write_file") {
            Ok(p) => p,
            Err(e) => {
                return ToolResult {
                    success: false,
                    content: String::new(),
                    error: Some(e),
                }
            }
        };

    if let Some(parent) = full_path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            return ToolResult {
                success: false,
                content: String::new(),
                error: Some(format!("Failed to create directory: {}", e)),
            };
        }
    }

    match std::fs::write(&full_path, content) {
        Ok(_) => ToolResult {
            success: true,
            content: format!("Successfully wrote to '{}'", path),
            error: None,
        },
        Err(e) => ToolResult {
            success: false,
            content: String::new(),
            error: Some(format!("Failed to write to '{}': {}", path, e)),
        },
    }
}

pub async fn tool_replace_in_file(args: Value, workspace_root: &str) -> ToolResult {
    let path = args["path"].as_str().unwrap_or("");

    let full_path =
        match ToolRegistry::resolve_workspace_path(workspace_root, path, "replace_in_file") {
            Ok(p) => p,
            Err(e) => {
                return ToolResult {
                    success: false,
                    content: String::new(),
                    error: Some(e),
                }
            }
        };

    // Build the list of (search, replace) pairs
    let mut edits: Vec<(String, String)> = Vec::new();

    // Multi-block mode: diff array
    if let Some(diff_array) = args["diff"].as_array() {
        for (i, item) in diff_array.iter().enumerate() {
            let search = item["search"].as_str().unwrap_or("");
            let replace = item["replace"].as_str().unwrap_or("");
            if search.is_empty() {
                return ToolResult {
                    success: false,
                    content: String::new(),
                    error: Some(format!(
                        "diff[{}].search is empty — each edit block must have a non-empty 'search' string",
                        i
                    )),
                };
            }
            edits.push((search.to_string(), replace.to_string()));
        }
    }

    // Single-block mode (backward compatible)
    if edits.is_empty() {
        let search = args["search"].as_str().unwrap_or("");
        let replace = args["replace"].as_str().unwrap_or("");
        if search.is_empty() {
            return ToolResult {
                success: false,
                content: String::new(),
                error: Some(
                    "Either 'diff' array or non-empty 'search' string must be provided"
                        .to_string(),
                ),
            };
        }
        edits.push((search.to_string(), replace.to_string()));
    }

    match std::fs::read_to_string(&full_path) {
        Ok(mut content) => {
            let mut applied = 0_usize;
            let mut details = Vec::new();

            for (i, (search, replace)) in edits.iter().enumerate() {
                if !content.contains(search.as_str()) {
                    return ToolResult {
                        success: false,
                        content: String::new(),
                        error: Some(format!(
                            "Edit #{} failed: search text not found in '{}'. \
                             The search text must match exactly (whitespace, indentation, and line endings).\n\
                             Search text preview (first 200 chars): {}",
                            i + 1,
                            path,
                            &search.chars().take(200).collect::<String>()
                        )),
                    };
                }

                // Count lines before and after for diff-like output
                let old_line_count = search.lines().count();
                let new_line_count = replace.lines().count();
                let line_delta = new_line_count as isize - old_line_count as isize;

                content = content.replacen(search.as_str(), replace.as_str(), 1);
                applied += 1;

                let line_info = if line_delta == 0 {
                    format!("{} line(s) modified", old_line_count)
                } else if line_delta > 0 {
                    format!(
                        "{} → {} lines (+{})",
                        old_line_count, new_line_count, line_delta
                    )
                } else {
                    format!(
                        "{} → {} lines ({})",
                        old_line_count, new_line_count, line_delta
                    )
                };

                details.push(format!(
                    "  Edit #{}: {} — {}",
                    i + 1,
                    line_info,
                    describe_change(search, replace)
                ));
            }

            match std::fs::write(&full_path, &content) {
                Ok(_) => {
                    let summary = format!(
                        "✅ Replaced in '{}': {} edit(s) applied.\n{}",
                        path,
                        applied,
                        details.join("\n")
                    );
                    ToolResult {
                        success: true,
                        content: summary,
                        error: None,
                    }
                }
                Err(e) => ToolResult {
                    success: false,
                    content: String::new(),
                    error: Some(format!("Failed to write to '{}': {}", path, e)),
                },
            }
        }
        Err(e) => ToolResult {
            success: false,
            content: String::new(),
            error: Some(format!("Failed to read file '{}': {}", path, e)),
        },
    }
}

/// Produce a short human-readable summary of what changed in an edit.
fn describe_change(search: &str, replace: &str) -> String {
    if replace.is_empty() {
        return "deleted".to_string();
    }
    if search.is_empty() || search == replace {
        return "unchanged".to_string();
    }

    // Compare first line of each for a quick label
    let old_first = search.lines().next().unwrap_or("").trim();
    let new_first = replace.lines().next().unwrap_or("").trim();

    if old_first.is_empty() && !new_first.is_empty() {
        return format!("added '{}'", new_first.chars().take(30).collect::<String>());
    }
    if old_first == new_first {
        return "modified".to_string();
    }
    format!(
        "'{}' → '{}'",
        old_first.chars().take(20).collect::<String>(),
        new_first.chars().take(20).collect::<String>()
    )
}

pub async fn tool_search_files(args: Value, workspace_root: &str) -> ToolResult {
    let pattern = args["pattern"].as_str().unwrap_or("");
    let search_path = args["path"].as_str().unwrap_or(".");
    let file_pattern = args["file_pattern"].as_str();

    let full_path =
        match ToolRegistry::resolve_workspace_path(workspace_root, search_path, "search_files") {
            Ok(p) => p,
            Err(e) => {
                return ToolResult {
                    success: false,
                    content: String::new(),
                    error: Some(e),
                }
            }
        };

    let mut results = Vec::new();
    let regex = match regex_lite::Regex::new(pattern) {
        Ok(r) => r,
        Err(e) => {
            return ToolResult {
                success: false,
                content: String::new(),
                error: Some(format!("Invalid regex pattern: {}", e)),
            }
        }
    };

    if let Err(e) =
        search_recursive(&full_path, &regex, file_pattern, &mut results)
    {
        return ToolResult {
            success: false,
            content: String::new(),
            error: Some(format!("Search failed: {}", e)),
        };
    }

    ToolResult {
        success: true,
        content: results.join("\n"),
        error: None,
    }
}

fn search_recursive(
    dir: &std::path::Path,
    regex: &regex_lite::Regex,
    file_pattern: Option<&str>,
    results: &mut Vec<String>,
) -> std::io::Result<()> {
    use std::io::Read;

    if !dir.is_dir() {
        return Ok(());
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            if name == "target"
                || name == "node_modules"
                || name.starts_with('.')
            {
                continue;
            }
            search_recursive(&path, regex, file_pattern, results)?;
        } else if path.is_file() {
            let filename = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();

            if let Some(fp) = file_pattern {
                if !glob_match(fp, &filename) {
                    continue;
                }
            }

            if let Ok(mut file) = std::fs::File::open(&path) {
                let mut content = String::new();
                if file.read_to_string(&mut content).is_ok() {
                    for (line_num, line) in content.lines().enumerate() {
                        if regex.is_match(line) {
                            results.push(format!(
                                "{}:{}: {}",
                                path.display(),
                                line_num + 1,
                                line
                            ));
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

fn glob_match(pattern: &str, filename: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    if pattern.starts_with("*.") {
        return filename.ends_with(&pattern[1..]);
    }
    filename.contains(pattern.trim_matches('*'))
}

pub async fn tool_list_files(args: Value, workspace_root: &str) -> ToolResult {
    let path = args["path"].as_str().unwrap_or(".");
    let recursive = args["recursive"].as_bool().unwrap_or(false);

    let full_path =
        match ToolRegistry::resolve_workspace_path(workspace_root, path, "list_files") {
            Ok(p) => p,
            Err(e) => {
                return ToolResult {
                    success: false,
                    content: String::new(),
                    error: Some(e),
                }
            }
        };

    let mut results = Vec::new();

    if let Err(e) = list_recursive(&full_path, recursive, 0, &mut results) {
        return ToolResult {
            success: false,
            content: String::new(),
            error: Some(format!("Failed to list files: {}", e)),
        };
    }

    ToolResult {
        success: true,
        content: results.join("\n"),
        error: None,
    }
}

/// Maximum recursion depth for `list_files` to prevent stack overflow.
const MAX_LIST_DEPTH: usize = 20;

fn list_recursive(
    dir: &std::path::Path,
    recursive: bool,
    depth: usize,
    results: &mut Vec<String>,
) -> std::io::Result<()> {
    if !dir.is_dir() || depth > MAX_LIST_DEPTH {
        return Ok(());
    }

    let indent = "  ".repeat(depth);

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();

        if name.starts_with('.')
            || name == "target"
            || name == "node_modules"
        {
            continue;
        }

        if path.is_dir() {
            results.push(format!("{}{}/", indent, name));
            if recursive {
                list_recursive(&path, recursive, depth + 1, results)?;
            }
        } else {
            results.push(format!("{}{}", indent, name));
        }
    }

    Ok(())
}