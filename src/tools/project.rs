use crate::tools::{ToolRegistry, ToolResult};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Project scaffolding tool
// ---------------------------------------------------------------------------

pub async fn tool_create_project(args: Value, workspace_root: &str) -> ToolResult {
    let project_type = args["project_type"]
        .as_str()
        .unwrap_or("rust")
        .to_lowercase();
    let project_name = args["name"].as_str().unwrap_or("new-project");
    let project_path = args["path"].as_str().unwrap_or(".");

    let full_path = match ToolRegistry::resolve_workspace_path(
        workspace_root,
        project_path,
        "create_project",
    ) {
        Ok(p) => p,
        Err(e) => {
            return ToolResult {
                success: false,
                content: String::new(),
                error: Some(e),
            }
        }
    };

    let target_dir = full_path.join(project_name);

    if target_dir.exists() {
        return ToolResult {
            success: false,
            content: String::new(),
            error: Some(format!(
                "Directory '{}' already exists. Please choose a different name.",
                target_dir.display()
            )),
        };
    }

    if let Err(e) = std::fs::create_dir_all(&target_dir) {
        return ToolResult {
            success: false,
            content: String::new(),
            error: Some(format!("Failed to create project directory: {}", e)),
        };
    }

    let (files, commands) = match project_type.as_str() {
        "rust" => (
            vec![
                ("Cargo.toml", CARGO_TOML_TEMPLATE),
                ("src/main.rs", RUST_MAIN_TEMPLATE),
            ],
            vec!["cargo build"],
        ),
        "typescript" | "ts" => (
            vec![
                ("package.json", TYPESCRIPT_PACKAGE_JSON),
                ("tsconfig.json", TSCONFIG_TEMPLATE),
                ("src/index.ts", TYPESCRIPT_INDEX_TEMPLATE),
            ],
            vec!["npm install", "npm run build"],
        ),
        "python" => (
            vec![
                ("requirements.txt", ""),
                ("main.py", PYTHON_MAIN_TEMPLATE),
            ],
            vec!["python main.py --help"],
        ),
        "web" => (
            vec![
                ("index.html", HTML_TEMPLATE),
                ("style.css", CSS_TEMPLATE),
                ("script.js", JS_TEMPLATE),
            ],
            vec![],
        ),
        _ => {
            return ToolResult {
                success: false,
                content: String::new(),
                error: Some(format!(
                    "Unsupported project type: '{}'. Supported: rust, typescript, python, web",
                    project_type
                )),
            };
        }
    };

    let mut created: Vec<String> = Vec::new();
    for (filename, content) in &files {
        let file_path = target_dir.join(filename);
        if let Some(parent) = file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match std::fs::write(&file_path, content) {
            Ok(_) => created.push(filename.to_string()),
            Err(e) => {
                return ToolResult {
                    success: false,
                    content: String::new(),
                    error: Some(format!("Failed to write '{}': {}", filename, e)),
                };
            }
        }
    }

    let mut result = format!(
        "✅ Created '{}' project '{}' at {}\n\nFiles:\n",
        project_type,
        project_name,
        target_dir.display()
    );
    for f in &created {
        result.push_str(&format!("  - {}\n", f));
    }

    if !commands.is_empty() {
        result.push_str("\nSuggested commands:\n");
        for cmd in &commands {
            result.push_str(&format!("  $ {}\n", cmd));
        }
    }

    ToolResult {
        success: true,
        content: result,
        error: None,
    }
}

// ---------------------------------------------------------------------------
// Project templates
// ---------------------------------------------------------------------------

const CARGO_TOML_TEMPLATE: &str = r#"[package]
name = "new-project"
version = "0.1.0"
edition = "2021"

[dependencies]
"#;

const RUST_MAIN_TEMPLATE: &str = r#"fn main() {
    println!("Hello, world!");
}
"#;

const TYPESCRIPT_PACKAGE_JSON: &str = r#"{
  "name": "new-project",
  "version": "1.0.0",
  "main": "dist/index.js",
  "scripts": {
    "build": "tsc",
    "start": "node dist/index.js"
  },
  "devDependencies": {
    "typescript": "^5.0.0"
  }
}
"#;

const TSCONFIG_TEMPLATE: &str = r#"{
  "compilerOptions": {
    "target": "ES2020",
    "module": "commonjs",
    "outDir": "./dist",
    "strict": true,
    "esModuleInterop": true,
    "skipLibCheck": true
  },
  "include": ["src"]
}
"#;

const TYPESCRIPT_INDEX_TEMPLATE: &str = r#"console.log("Hello, TypeScript!");
"#;

const PYTHON_MAIN_TEMPLATE: &str = r#"#!/usr/bin/env python3
"""Main entry point."""

def main():
    print("Hello, Python!")

if __name__ == "__main__":
    main()
"#;

const HTML_TEMPLATE: &str = r#"<!DOCTYPE html>
<html lang="zh-TW">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>New Project</title>
    <link rel="stylesheet" href="style.css">
</head>
<body>
    <h1>Hello, World!</h1>
    <script src="script.js"></script>
</body>
</html>
"#;

const CSS_TEMPLATE: &str = r#"* {
    margin: 0;
    padding: 0;
    box-sizing: border-box;
}

body {
    font-family: system-ui, sans-serif;
    max-width: 800px;
    margin: 2rem auto;
    padding: 1rem;
}

h1 {
    color: #333;
}
"#;

const JS_TEMPLATE: &str = r#"document.addEventListener('DOMContentLoaded', () => {
    console.log('App initialized');
});
"#;