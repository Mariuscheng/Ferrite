use ferrite::context::{build_native_tool_defs, build_tools_prompt};
use ferrite::tools::ToolDefinition;
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
    let ctx = ferrite::context::build_project_context(".");
    assert!(ctx.contains("Rust"));
}
