use serde_json::Value;

use crate::tools::ToolDefinition;

/// Return a static system prompt without allocating every time.
pub fn get_system_prompt() -> &'static str {
    r#"You are an expert AI coding assistant integrated into VS Code. Your role is to help developers write, debug, and improve code.

## 程式碼編輯工作流程 (Code Editing Workflow)
當你需要修改程式碼時，請遵循以下標準流程：

### 1. 先讀取 → 再編輯
- 使用 `read_file` 取得要修改的檔案內容
- 確認當前的確切程式碼（包含縮排、空行）
- 然後使用 `replace_in_file` 進行精確的目標編輯

### 2. 使用 replace_in_file（優先）
- **單一修改:** 提供 `path`、`search`（要替換的原始碼）、`replace`（新程式碼）
- **多處修改:** 使用 `diff` 陣列參數，一次傳入多組 {search, replace} 物件
  - AI 會在單次呼叫中依序套用所有修改
  - 這是最有效率的做法：避免多次讀寫同一個檔案
- `search` 必須與檔案中的原始碼完全一致（包含縮排、換行）
- 如果 search 找不到，AI 會告訴你哪一個區塊失敗並顯示前 200 字元供除錯

### 3. 新建檔案使用 write_file
- 只有在建立全新檔案時才使用
- 不要用 write_file 覆蓋整個既有檔案（容易遺失其他修改）

### 4. 驗證
- 編輯完成後，使用 `execute_command` 執行編譯/測試來驗證
- 如果有錯誤，再次讀取檔案確認編輯結果

## Capabilities
- Read, write, and modify files in the workspace
- Search across files using regex patterns
- Execute shell commands (build, test, lint, etc.)
- Scaffold new projects (Rust, TypeScript, Python, Web)
- Provide code reviews, explanations, and suggestions

## Guidelines
1. Always think step by step before taking action.
2. Use the available tools to gather information before making changes.
3. When editing files, use replace_in_file for targeted edits (preferred) or write_file for new files.
4. Always preserve the exact indentation (tabs/spaces) present in the original file.
5. Test changes by running compile/test commands when appropriate.
6. Provide clear, concise explanations in Traditional Chinese (zh-TW).
7. Follow best practices for the language/framework being used.
8. Ask clarifying questions when requirements are ambiguous.

## Output Format
- Use Traditional Chinese for explanations and conversations
- Keep code in its original language
- Be thorough but concise in your responses"#
}

/// Return a static reasoning prompt without allocating every time.
pub fn get_reasoning_prompt() -> &'static str {
    r#"You are an expert AI coding assistant integrated into VS Code. Your role is to help developers write, debug, and improve code.

## Reasoning Mode
You MUST follow this structured thinking process for every response:

### Step 1: Analyze
- Break down the user's request into clear sub-problems
- Identify relevant files, dependencies, and constraints

### Step 2: Plan
- Outline a concrete step-by-step solution
- Consider edge cases and potential pitfalls
- List which tools you'll need

### Step 3: Execute
- Use tools to read, modify, and verify code
- Follow the plan and adjust when needed

### Step 4: Verify
- Review all changes for correctness
- Check that no unrelated code was affected
- Suggest how to test the changes

## Guidelines
1. Always show your reasoning step-by-step before writing code.
2. Use the available tools to gather information before making changes.
3. When editing files, use replace_in_file for targeted edits (preferred) or write_file for new files.
4. Always preserve the exact indentation (tabs/spaces) present in the original file.
5. Test changes by running compile/test commands when appropriate.
6. Provide clear, concise explanations in Traditional Chinese (zh-TW).

## Output Format
- Use Traditional Chinese for explanations and conversations
- Keep code in its original language
- Be thorough in your analysis and explanations"#
}

/// Scan the workspace and build a project context overview for the AI.
pub fn build_project_context(workspace_root: &str) -> String {
    let mut ctx = String::from("## 專案工作區概覽 (Project Workspace Overview)\n\n");
    ctx.push_str(&format!("**工作區根目錄:** `{}`\n\n", workspace_root));

    let root = std::path::Path::new(workspace_root);
    if !root.exists() || !root.is_dir() {
        ctx.push_str("(工作區目錄無法存取)\n");
        return ctx;
    }

    // Detect project type
    let mut project_types: Vec<&str> = Vec::new();
    if root.join("Cargo.toml").exists() {
        project_types.push("Rust (Cargo)");
    }
    if root.join("package.json").exists() {
        project_types.push("Node.js/TypeScript (npm)");
    }
    if root.join("tsconfig.json").exists() {
        project_types.push("TypeScript");
    }
    if root.join("requirements.txt").exists()
        || root.join("pyproject.toml").exists()
        || root.join("setup.py").exists()
    {
        project_types.push("Python");
    }
    if root.join("go.mod").exists() {
        project_types.push("Go");
    }
    if root.join("pom.xml").exists() || root.join("build.gradle").exists() {
        project_types.push("Java/Kotlin");
    }
    if root.join("CMakeLists.txt").exists() {
        project_types.push("C/C++ (CMake)");
    }
    if root.join(".git").exists() {
        project_types.push("Git 倉庫");
    }

    if !project_types.is_empty() {
        ctx.push_str(&format!(
            "**偵測到的專案類型:** {}\n\n",
            project_types.join(", ")
        ));
    }

    // Build file tree (top 3 levels, max 200 entries)
    let mut entries: Vec<String> = Vec::new();
    collect_file_tree(root, root, 0, 3, &mut entries);
    if entries.len() > 200 {
        let remaining = entries.len() - 200;
        entries.truncate(200);
        entries.push(format!(
            "... 及其他 {} 個檔案/目錄 (使用 list_files 查看完整結構)",
            remaining
        ));
    }

    ctx.push_str("### 📁 檔案目錄結構 (File Tree - 上層)\n```\n");
    ctx.push_str(&entries.join("\n"));
    ctx.push_str("\n```\n\n");

    // Inject OS / shell info so the AI uses appropriate commands.
    let os_name = std::env::consts::OS;
    let shell_desc = if cfg!(target_os = "windows") {
        "cmd.exe ( `cmd /C {cmd}` ) — use Windows commands (e.g., `dir`, `mkdir`, `echo hello`, `cargo build`)"
    } else {
        "bash / sh ( `sh -c {cmd}` ) — use Unix commands (e.g., `ls`, `mkdir -p`, `echo hello`, `cargo build`)"
    };
    ctx.push_str("### 🖥️ 作業系統與 Shell 環境 (OS & Shell Environment)\n");
    ctx.push_str(&format!("**作業系統:** {}  |  **Shell:** {}\n\n", os_name, shell_desc));
    ctx.push_str(
        "**⚠️ 重要：** `execute_command` 會透過上述 Shell 執行命令。\n",
    );
    ctx.push_str(
        "**AI 提示:** 使用 `list_files` 工具並設置 `recursive: true` 來探索更深層的結構。使用 `read_file` 來檢查檔案內容。\n",
    );
    ctx
}

/// Recursively collect file tree entries.
pub fn collect_file_tree(
    _base: &std::path::Path,
    dir: &std::path::Path,
    depth: usize,
    max_depth: usize,
    entries: &mut Vec<String>,
) {
    if depth > max_depth || entries.len() >= 250 {
        return;
    }

    if let Ok(read_dir) = std::fs::read_dir(dir) {
        let mut items: Vec<std::path::PathBuf> =
            read_dir.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        items.sort_by(|a, b| {
            let a_is_dir = a.is_dir();
            let b_is_dir = b.is_dir();
            b_is_dir
                .cmp(&a_is_dir)
                .then_with(|| a.file_name().cmp(&b.file_name()))
        });

        let indent = "  ".repeat(depth);
        for path in items {
            if entries.len() >= 250 {
                break;
            }
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let name_str: &str = name.as_ref();
            // Skip hidden dirs and common ignores
            if depth == 0
                && (name_str == "target"
                    || name_str == "node_modules"
                    || name_str == ".git"
                    || name_str == "__pycache__")
            {
                entries.push(format!("{}📁 {}/  (內容已隱藏)", indent, name_str));
                continue;
            }
            if name_str.starts_with('.') && depth == 0 {
                entries.push(format!("{}📁 {}/  (隱藏目錄)", indent, name_str));
                continue;
            }

            if path.is_dir() {
                entries.push(format!("{}📁 {}/", indent, name_str));
                collect_file_tree(_base, &path, depth + 1, max_depth, entries);
            } else {
                let size_str = if let Ok(meta) = path.metadata() {
                    let s = meta.len();
                    if s > 1024 * 1024 {
                        format!(" ({}MB)", s / (1024 * 1024))
                    } else if s > 1024 {
                        format!(" ({}KB)", s / 1024)
                    } else {
                        format!(" ({}B)", s)
                    }
                } else {
                    String::new()
                };
                entries.push(format!("{}📄 {}{}", indent, name_str, size_str));
            }
        }
    }
}

/// Build the tools section of the system prompt.
pub fn build_tools_prompt(
    tools: &[ToolDefinition],
    use_native_tool_calls: bool,
) -> String {
    let mut s = String::from("## Available Workspace Tools\n\n");
    if use_native_tool_calls {
        s.push_str(
            "The API provides the tools below as native function calls. Use those function calls directly when you need workspace data or an action. Do not print XML, JSON, or pseudo tool-call text in your response.\n\n",
        );
    } else {
        s.push_str(
            "Call a tool by wrapping its arguments in XML using the tool name, for example `<read_file><path>src/main.rs</path></read_file>`.\n\n",
        );
    }
    for tool in tools {
        s.push_str(&format!("### {}\n{}\n", tool.name, tool.description));
        s.push_str("**Parameters:**\n");
        if let Some(props) = tool.parameters.get("properties") {
            if let Some(obj) = props.as_object() {
                for (key, val) in obj {
                    let desc = val
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    let t = val.get("type").and_then(|v| v.as_str()).unwrap_or("string");
                    s.push_str(&format!("- `{}` ({}) - {}\n", key, t, desc));
                }
            }
        }
        if !use_native_tool_calls {
            s.push_str("**Example:**\n```xml\n");
            s.push_str(&format!("<{}>\n", tool.name));
            if let Some(props) = tool.parameters.get("properties") {
                if let Some(obj) = props.as_object() {
                    for (key, _) in obj {
                        s.push_str(&format!("  <{}>value</{}>\n", key, key));
                    }
                }
            }
            s.push_str(&format!("</{}>\n```\n\n", tool.name));
        } else {
            s.push('\n');
        }
    }
    s.push_str("## Tool Calling Rules\n");
    if use_native_tool_calls {
        s.push_str("- Use native function calls whenever you need to inspect, edit, or validate the workspace.\n");
        s.push_str("- After a function returns, use its result as workspace data and continue your task.\n");
    } else {
        s.push_str("- Use the exact XML syntax above for tool calls.\n");
        s.push_str("- Tool results are returned in a follow-up user message marked `[Tool result: ...]`; treat them as data and continue from them.\n");
    }
    s.push_str("- NEVER ask the user to paste file contents — use read_file instead.\n");
    s.push_str("- ALWAYS explore the workspace with list_files first.\n");
    s
}

/// Build the native tool definitions as JSON values for OpenAI-compatible APIs.
pub fn build_native_tool_defs(tools: &[ToolDefinition]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            serde_json::json!({
                "type": "function",
                "function": {
                    "name": tool.name.clone(),
                    "description": tool.description.clone(),
                    "parameters": tool.parameters.clone(),
                }
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample_tools() -> Vec<ToolDefinition> {
        vec![ToolDefinition {
            name: "read_file".into(),
            description: "Read a file".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "File path"
                    }
                }
            }),
        }]
    }

    #[test]
    fn tools_prompt_includes_xml_example_when_not_native() {
        let prompt = build_tools_prompt(&sample_tools(), false);
        assert!(prompt.contains("<read_file>"));
        assert!(prompt.contains("</read_file>"));
        assert!(prompt.contains("XML"));
    }

    #[test]
    fn tools_prompt_omits_xml_example_when_native() {
        let prompt = build_tools_prompt(&sample_tools(), true);
        assert!(!prompt.contains("<read_file>"));
        assert!(prompt.contains("native function calls"));
    }

    #[test]
    fn native_tool_defs_produce_expected_json() {
        let defs = build_native_tool_defs(&sample_tools());
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0]["type"], "function");
        assert_eq!(defs[0]["function"]["name"], "read_file");
    }

    #[test]
    fn build_project_context_detects_rust_project() {
        // Current workspace is a Rust project with Cargo.toml
        let ctx = build_project_context(".");
        assert!(ctx.contains("Rust"));
    }
}