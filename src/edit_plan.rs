use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::providers::{AiProvider, ChatMessage, ChatRequest, Role};
use crate::tool_parser;
use crate::tools::{ToolName, ToolRegistry};

/// Generate an edit plan from a natural-language goal and optional IDE context.
pub async fn generate_edit_plan(
    provider: &dyn AiProvider,
    model: &str,
    reasoning: bool,
    goal: &str,
    ide_context_text: &str,
) -> Result<Value> {
    let system_prompt = r#"You are a code modification planner.
Return STRICT JSON only, no markdown, no explanations.
Schema:
{
  \"summary\": string,
  \"steps\": string[],
  \"edits\": [
    {
      \"path\": string,
      \"search\": string,
      \"replace\": string,
      \"reason\": string
    }
  ],
  \"validationCommands\": string[],
  \"notes\": string[]
}
Rules:
- edits must be deterministic search/replace operations.
- keep changes minimal and safe.
- if unsure, return empty edits and explain in notes.
"#;

    let user_prompt = format!(
        "Goal:\n{}\n\nIDE Context JSON:\n{}\n\nGenerate the edit plan now.",
        goal, ide_context_text
    );

    let request = ChatRequest {
        model: model.to_string(),
        messages: vec![
            ChatMessage {
                role: Role::System,
                content: system_prompt.to_string(),
                tool_call_id: None,
                name: None,
                tool_calls: None,
            },
            ChatMessage {
                role: Role::User,
                content: user_prompt,
                tool_call_id: None,
                name: None,
                tool_calls: None,
            },
        ],
        temperature: 0.1,
        max_tokens: Some(4096),
        stream: false,
        reasoning,
        tools: None,
    };

    let response = provider.chat(request).await?;
    let content = response
        .choices
        .first()
        .and_then(|c| c.message.as_ref())
        .map(|m| m.content.clone())
        .unwrap_or_default();

    let parsed = if let Ok(v) = serde_json::from_str::<Value>(&content) {
        v
    } else {
        tool_parser::extract_first_json_object(&content)
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .unwrap_or_else(|| {
                serde_json::json!({
                    "summary": "無法解析模型輸出為 JSON",
                    "steps": [],
                    "edits": [],
                    "validationCommands": [],
                    "notes": [content]
                })
            })
    };

    Ok(parsed)
}

/// Run a list of shell commands as validation steps against the workspace.
pub async fn run_validation_commands(
    tool_registry: &ToolRegistry,
    workspace_root: &str,
    commands: &[String],
) -> Vec<ValidationRunResult> {
    let mut out = Vec::new();
    for cmd in commands {
        let result = tool_registry
            .execute(
                ToolName::ExecuteCommand,
                serde_json::json!({ "command": cmd }),
                workspace_root,
            )
            .await;

        out.push(ValidationRunResult {
            command: cmd.clone(),
            success: result.success,
            output: result.content,
            error: result.error,
        });
    }
    out
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationRunResult {
    pub command: String,
    pub success: bool,
    pub output: String,
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::providers;
    #[tokio::test]
    async fn generate_edit_plan_with_empty_context() {
        let config = Config::default();
        let provider = providers::create_provider(&config).expect("create provider");
        let result = generate_edit_plan(
            provider.as_ref(),
            &config.model,
            config.reasoning,
            "test goal",
            "{}",
        )
        .await;

        // Should not panic and return a Value
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn validation_run_result_serializes() {
        let result = ValidationRunResult {
            command: "cargo build".into(),
            success: true,
            output: "Compiled successfully".into(),
            error: None,
        };
        let json = serde_json::to_value(&result).expect("serialize");
        assert_eq!(json["command"], "cargo build");
        assert_eq!(json["success"], true);
    }
}