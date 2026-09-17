use crate::tools::{ToolRegistry, ToolResult};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Diff-based editing — apply unified diffs with fuzz matching, dry-run preview,
// and automatic backup for undo support.
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffHunk {
    pub old_start: usize,
    pub old_count: usize,
    pub new_start: usize,
    pub new_count: usize,
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffLineKind {
    Context,
    Added,
    Removed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedDiff {
    pub original_path: String,
    pub new_path: String,
    pub hunks: Vec<DiffHunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffPreview {
    pub file: String,
    pub hunks_preview: Vec<HunkPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HunkPreview {
    pub header: String,
    pub applied: bool,
    pub old_text: String,
    pub new_text: String,
    /// The full hunk lines (with Context/Added/Removed markers),
    /// used by the frontend to faithfully rebuild the patch.
    pub lines: Vec<DiffLine>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffApplyResult {
    pub file: String,
    pub applied: bool,
    pub hunks_applied: usize,
    pub hunks_failed: usize,
    pub backup_path: Option<String>,
    pub preview: Option<DiffPreview>,
}

// ---------------------------------------------------------------------------
// Main tool entry point
// ---------------------------------------------------------------------------

pub async fn tool_apply_diff(args: Value, workspace_root: &str) -> ToolResult {
    let patch = args["patch"].as_str().unwrap_or("");
    let dry_run = args["dry_run"].as_bool().unwrap_or(false);
    let fuzz = args["fuzz"].as_u64().unwrap_or(3) as usize;

    if patch.is_empty() {
        return ToolResult {
            success: false,
            content: String::new(),
            error: Some("patch 參數為空".to_string()),
        };
    }

    let diffs = match parse_unified_diff(patch) {
        Ok(d) => d,
        Err(e) => {
            return ToolResult {
                success: false,
                content: String::new(),
                error: Some(format!("無法解析 diff: {}", e)),
            }
        }
    };

    if diffs.is_empty() {
        return ToolResult {
            success: false,
            content: String::new(),
            error: Some("diff 中沒有找到任何檔案變更".to_string()),
        };
    }

    let mut results: Vec<DiffApplyResult> = Vec::new();

    for diff in &diffs {
        let full_path = match ToolRegistry::resolve_workspace_path(
            workspace_root,
            &diff.new_path,
            "apply_diff",
        ) {
            Ok(p) => p,
            Err(e) => {
                results.push(DiffApplyResult {
                    file: diff.new_path.clone(),
                    applied: false,
                    hunks_applied: 0,
                    hunks_failed: diff.hunks.len(),
                    backup_path: None,
                    preview: None,
                });
                if !dry_run {
                    return ToolResult {
                        success: false,
                        content: serde_json::to_string_pretty(&results).unwrap_or_default(),
                        error: Some(e),
                    };
                }
                continue;
            }
        };

        let original_content = match std::fs::read_to_string(&full_path) {
            Ok(c) => c,
            Err(e) => {
                results.push(DiffApplyResult {
                    file: diff.new_path.clone(),
                    applied: false,
                    hunks_applied: 0,
                    hunks_failed: diff.hunks.len(),
                    backup_path: None,
                    preview: None,
                });
                if !dry_run {
                    return ToolResult {
                        success: false,
                        content: serde_json::to_string_pretty(&results).unwrap_or_default(),
                        error: Some(format!("無法讀取 '{}': {}", diff.new_path, e)),
                    };
                }
                continue;
            }
        };

        let apply_result = apply_diff_hunks(
            &original_content,
            diff,
            fuzz,
            dry_run,
            &full_path,
        );

        results.push(apply_result);
    }

    if dry_run {
        // Build preview JSON
        let previews: Vec<DiffPreview> = results
            .iter()
            .filter_map(|r| r.preview.clone())
            .collect();
        let all_hunks: usize = diffs.iter().map(|d| d.hunks.len()).sum();
        let applicable: usize = results
            .iter()
            .filter_map(|r| r.preview.as_ref())
            .map(|p| p.hunks_preview.iter().filter(|h| h.applied).count())
            .sum();

        ToolResult {
            success: true,
            content: serde_json::to_string_pretty(&serde_json::json!({
                "mode": "dry_run",
                "files": diffs.len(),
                "totalHunks": all_hunks,
                "applicableHunks": applicable,
                "previews": previews,
            }))
            .unwrap_or_default(),
            error: None,
        }
    } else {
        // Write files
        let mut applied_files = 0_usize;
        let mut failed_files = 0_usize;

        for result in &results {
            if result.applied {
                applied_files += 1;
            } else {
                failed_files += 1;
            }
        }

        let all_applied = failed_files == 0;

        ToolResult {
            success: all_applied,
            content: serde_json::to_string_pretty(&serde_json::json!({
                "mode": "apply",
                "filesApplied": applied_files,
                "filesFailed": failed_files,
                "results": results,
            }))
            .unwrap_or_default(),
            error: if all_applied {
                None
            } else {
                Some(format!("{} 個檔案套用失敗", failed_files))
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Unified diff parser
// ---------------------------------------------------------------------------

pub fn parse_unified_diff(patch: &str) -> Result<Vec<ParsedDiff>, String> {
    let mut diffs: Vec<ParsedDiff> = Vec::new();
    let mut current_diff: Option<ParsedDiff> = None;
    let mut current_hunk: Option<DiffHunk> = None;

    for raw_line in patch.lines() {
        let line = raw_line;
        // Allow trailing \r (Windows line endings mixed in)
        let line = line.trim_end_matches('\r');

        if let Some(rest) = line.strip_prefix("--- ") {
            // Start of a new file diff — finalise the previous file
            if let Some(mut diff) = current_diff.take() {
                if let Some(hunk) = current_hunk.take() {
                    diff.hunks.push(hunk);
                }
                diffs.push(diff);
            }
            let original = rest.trim().to_string();
            current_diff = Some(ParsedDiff {
                original_path: original,
                new_path: String::new(),
                hunks: Vec::new(),
            });
            continue;
        }

        if let Some(rest) = line.strip_prefix("+++ ") {
            let new_path = rest.trim().to_string();
            if let Some(ref mut diff) = current_diff {
                diff.new_path = new_path;
            } else {
                // Standalone +++ without preceding ---
                current_diff = Some(ParsedDiff {
                    original_path: String::new(),
                    new_path,
                    hunks: Vec::new(),
                });
            }
            continue;
        }

        if line.starts_with("@@") {
            // Start of a new hunk
            if let Some(ref mut diff) = current_diff {
                if let Some(hunk) = current_hunk.take() {
                    diff.hunks.push(hunk);
                }
            }

            let header = line.to_string();
            let (old_start, old_count, new_start, new_count) =
                parse_hunk_header(&header)?;

            current_hunk = Some(DiffHunk {
                old_start,
                old_count,
                new_start,
                new_count,
                header,
                lines: Vec::new(),
            });
            continue;
        }

        // Accumulate lines into the current hunk
        if let Some(ref mut hunk) = current_hunk {
            if let Some(rest) = line.strip_prefix(' ') {
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Context,
                    text: rest.to_string(),
                });
            } else if let Some(rest) = line.strip_prefix('-') {
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Removed,
                    text: rest.to_string(),
                });
            } else if let Some(rest) = line.strip_prefix('+') {
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Added,
                    text: rest.to_string(),
                });
            } else if line.is_empty() {
                // Empty line in context — treat as context
                hunk.lines.push(DiffLine {
                    kind: DiffLineKind::Context,
                    text: String::new(),
                });
            }
            // Lines not starting with ' ', '-', '+' are ignored (e.g. \ No newline at end of file)
        }
    }

    // Push the last hunk and diff
    if let Some(ref mut diff) = current_diff {
        if let Some(hunk) = current_hunk.take() {
            diff.hunks.push(hunk);
        }
    }
    if let Some(diff) = current_diff {
        diffs.push(diff);
    }

    if diffs.is_empty() {
        return Err("找不到有效的 unified diff 內容".to_string());
    }

    // Normalize paths: strip the a/ and b/ prefixes
    for diff in &mut diffs {
        diff.new_path = strip_git_prefix(&diff.new_path);
        diff.original_path = strip_git_prefix(&diff.original_path);
    }

    Ok(diffs)
}

pub fn strip_git_prefix(path: &str) -> String {
    if path.starts_with("a/") || path.starts_with("b/") {
        path[2..].to_string()
    } else {
        path.to_string()
    }
}

pub fn parse_hunk_header(header: &str) -> Result<(usize, usize, usize, usize), String> {
    // @@ -start,count +start,count @@ context
    let inner = header
        .trim_start_matches("@@")
        .trim()
        .split("@@")
        .next()
        .unwrap_or("")
        .trim();

    let parts: Vec<&str> = inner.split_whitespace().collect();
    if parts.len() < 2 {
        return Err(format!("無法解析 hunk header: {}", header));
    }

    let old = parts[0].trim_start_matches('-');
    let new = parts[1].trim_start_matches('+');

    let (old_start, old_count) = parse_hunk_range(old)?;
    let (new_start, new_count) = parse_hunk_range(new)?;

    Ok((old_start, old_count, new_start, new_count))
}

fn parse_hunk_range(s: &str) -> Result<(usize, usize), String> {
    if let Some(comma) = s.find(',') {
        let start: usize = s[..comma]
            .parse()
            .map_err(|_| format!("無效的 hunk 範圍: {}", s))?;
        let count: usize = s[comma + 1..]
            .parse()
            .map_err(|_| format!("無效的 hunk 範圍: {}", s))?;
        Ok((start, count))
    } else {
        let start: usize = s
            .parse()
            .map_err(|_| format!("無效的 hunk 範圍: {}", s))?;
        Ok((start, 1))
    }
}

// ---------------------------------------------------------------------------
// Hunk application with fuzz matching
// ---------------------------------------------------------------------------

pub fn apply_diff_hunks(
    original: &str,
    diff: &ParsedDiff,
    fuzz: usize,
    dry_run: bool,
    file_path: &PathBuf,
) -> DiffApplyResult {
    let original_lines: Vec<&str> = original.lines().collect();
    let mut hunks_preview: Vec<HunkPreview> = Vec::new();
    let mut applied_count = 0_usize;
    let mut failed_count = 0_usize;

    // Build the new content by applying hunks sequentially
    // Track offset as we modify the line numbers
    let mut line_offset: isize = 0;
    let mut current_lines: Vec<String> = original_lines.iter().map(|s| s.to_string()).collect();

    for hunk in &diff.hunks {
        let target_line = (hunk.old_start as isize + line_offset - 1).max(0) as usize;

        let (matched, match_line) = find_hunk_match(
            &current_lines,
            hunk,
            target_line,
            fuzz,
        );

        if matched {
            // Build preview
            let old_text = extract_context(
                &current_lines,
                match_line,
                hunk.old_count.min(current_lines.len().saturating_sub(match_line)),
            );

            // Apply the hunk
            let new_lines = build_new_lines(hunk);
            let end = (match_line + hunk.old_count).min(current_lines.len());

            let new_text = new_lines.join("\n");

            // Replace in current_lines
            current_lines.splice(match_line..end, new_lines.clone());

            let delta = new_lines.len() as isize - (end - match_line) as isize;
            line_offset += delta;

            hunks_preview.push(HunkPreview {
                header: hunk.header.clone(),
                applied: true,
                old_text,
                new_text,
                lines: hunk.lines.clone(),
                error: None,
            });
            applied_count += 1;
        } else {
            hunks_preview.push(HunkPreview {
                header: hunk.header.clone(),
                applied: false,
                old_text: String::new(),
                new_text: String::new(),
                lines: hunk.lines.clone(),
                error: Some(format!(
                    "找不到符合的上下文（搜尋範圍內無匹配，fuzz={}）",
                    fuzz
                )),
            });
            failed_count += 1;

            if !dry_run {
                // Don't proceed with remaining hunks if this one fails in real mode
                break;
            }
        }
    }

    let preview = DiffPreview {
        file: diff.new_path.clone(),
        hunks_preview,
    };

    if !dry_run && failed_count == 0 {
        // Write the modified content to file
        let backup_path = create_backup(file_path);
        let new_content = current_lines.join("\n");

        match std::fs::write(file_path, &new_content) {
            Ok(_) => DiffApplyResult {
                file: diff.new_path.clone(),
                applied: true,
                hunks_applied: applied_count,
                hunks_failed: failed_count,
                backup_path,
                preview: None,
            },
            Err(_) => DiffApplyResult {
                file: diff.new_path.clone(),
                applied: false,
                hunks_applied: 0,
                hunks_failed: diff.hunks.len(),
                backup_path: None,
                preview: Some(preview),
            },
        }
    } else {
        DiffApplyResult {
            file: diff.new_path.clone(),
            applied: false, // not actually applied in dry_run or when hunks fail
            hunks_applied: applied_count,
            hunks_failed: failed_count,
            backup_path: None,
            preview: Some(preview),
        }
    }
}

/// Try to match a hunk against the current file content within `fuzz` lines
/// of the expected position.
fn find_hunk_match(
    lines: &[String],
    hunk: &DiffHunk,
    target_line: usize,
    fuzz: usize,
) -> (bool, usize) {
    let search_start = target_line.saturating_sub(fuzz);
    let search_end = (target_line + fuzz).min(lines.len());

    for candidate_line in search_start..=search_end {
                if hunk_matches_at(lines, hunk, candidate_line) {
            return (true, candidate_line);
        }
    }

    (false, target_line)
}

/// Check whether a hunk matches at a specific line in the file.
fn hunk_matches_at(lines: &[String], hunk: &DiffHunk, start: usize) -> bool {
    if start + hunk.old_count > lines.len() {
        return false;
    }

    let mut file_idx = start;
    for diff_line in &hunk.lines {
        match diff_line.kind {
            DiffLineKind::Context => {
                if file_idx >= lines.len() {
                    return false;
                }
                if lines[file_idx] != diff_line.text {
                    return false;
                }
                file_idx += 1;
            }
            DiffLineKind::Removed => {
                if file_idx >= lines.len() {
                    return false;
                }
                if lines[file_idx] != diff_line.text {
                    return false;
                }
                file_idx += 1;
            }
            DiffLineKind::Added => {
                // Added lines don't need to match against the original file
            }
        }
    }

    true
}

/// Build the replacement lines for a hunk (context + added lines, no removed lines).
fn build_new_lines(hunk: &DiffHunk) -> Vec<String> {
    hunk.lines
        .iter()
        .filter(|l| l.kind != DiffLineKind::Removed)
        .map(|l| l.text.clone())
        .collect()
}

/// Extract context text around a hunk match for preview display.
fn extract_context(lines: &[String], start: usize, count: usize) -> String {
    let end = (start + count).min(lines.len());
    let slice = &lines[start..end];
    slice.join("\n")
}

/// Create a backup of the original file before modification.
fn create_backup(file_path: &PathBuf) -> Option<String> {
    let backup_path = file_path.with_extension(
        format!(
            "{}.ferrite-bak",
            file_path
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| format!(".{}", e))
                .unwrap_or_default()
        ),
    );

    match std::fs::copy(file_path, &backup_path) {
        Ok(_) => Some(backup_path.to_string_lossy().to_string()),
        Err(e) => {
            tracing::warn!("無法建立備份 '{}': {}", file_path.display(), e);
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Diff listing tools — allow the AI to review what it has changed so far
// (backed by .ferrite-bak snapshots created on each apply_diff).
// ---------------------------------------------------------------------------

/// List files that have a `.ferrite-bak` snapshot (i.e. files modified by the
/// agent via apply_diff). Returns relative paths within the workspace.
pub fn list_diff_files(workspace_root: &str) -> Vec<String> {
    let root = std::path::Path::new(workspace_root);
    let mut files: Vec<String> = Vec::new();

    let mut stack: Vec<std::path::PathBuf> = vec![root.to_path_buf()];
    // Depth-first search to avoid recursion limits on huge workspaces.
    while let Some(dir) = stack.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Skip hidden directories (e.g. .git, node_modules handled below)
                let name = entry.file_name();
                let name_str = name.to_string_lossy();
                if name_str == ".git" {
                    continue;
                }
                stack.push(path);
            } else {
                // Look for .ferrite-bak files: "foo.rs.ferrite-bak" matches "foo.rs"
                let fname = entry.file_name().to_string_lossy().to_string();
                if let Some(orig) = fname.strip_suffix(".ferrite-bak") {
                    let orig_path = path.with_file_name(orig);
                    if orig_path.exists() {
                        if let Ok(rel) = orig_path.strip_prefix(root) {
                            files.push(rel.to_string_lossy().replace('\\', "/"));
                        }
                    }
                }
            }
        }
    }

    files.sort();
    files
}

/// Generate a unified diff for a single file by comparing its `.ferrite-bak`
/// snapshot (pre-edit state) with the current on-disk content.
/// Returns `None` when the file has no backup or can't be read.
pub fn get_file_diff(
    workspace_root: &str,
    rel_path: &str,
) -> Result<String, String> {
    let full_path =
        ToolRegistry::resolve_workspace_path(workspace_root, rel_path, "get_file_diff")?;

    // Locate the .ferrite-bak snapshot next to the file.
    let fname = full_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "無效的檔案名稱".to_string())?;
    let backup_path = full_path.with_file_name(format!("{}.ferrite-bak", fname));
    if !backup_path.exists() {
        return Err(format!(
            "'{}' 沒有 .ferrite-bak 備份（尚未透過 apply_diff 修改過）",
            rel_path
        ));
    }

    let old_text = std::fs::read_to_string(&backup_path)
        .map_err(|e| format!("無法讀取備份 '{}': {}", backup_path.display(), e))?;
    let new_text = std::fs::read_to_string(&full_path)
        .map_err(|e| format!("無法讀取 '{}': {}", rel_path, e))?;

    // Generate unified diff using the `similar` crate's built-in
    // unified diff formatter (with 3 lines of context).
    let diff = similar::TextDiff::from_lines(&old_text, &new_text);
    let out = diff
        .unified_diff()
        .header(&format!("a/{}", rel_path), &format!("b/{}", rel_path))
        .context_radius(3)
        .to_string();

    Ok(out)
}

/// Tool entry point for `list_diff`.
pub async fn tool_list_diff(args: Value, workspace_root: &str) -> ToolResult {
    let _ = args; // list_diff takes no arguments, but keep the unified tool signature
    let files = list_diff_files(workspace_root);
    ToolResult {
        success: true,
        content: serde_json::to_string_pretty(&serde_json::json!({
            "files": files,
            "count": files.len(),
        }))
        .unwrap_or_else(|_| "[]".to_string()),
        error: None,
    }
}

/// Tool entry point for `get_file_diff`.
pub async fn tool_get_file_diff(args: Value, workspace_root: &str) -> ToolResult {
    let path = args["path"].as_str().unwrap_or("");
    if path.is_empty() {
        return ToolResult {
            success: false,
            content: String::new(),
            error: Some("path 參數為空".to_string()),
        };
    }

    match get_file_diff(workspace_root, path) {
        Ok(patch) => ToolResult {
            success: true,
            content: serde_json::json!({
                "path": path,
                "patch": patch,
            })
            .to_string(),
            error: None,
        },
        Err(e) => ToolResult {
            success: false,
            content: String::new(),
            error: Some(e),
        },
    }
}

