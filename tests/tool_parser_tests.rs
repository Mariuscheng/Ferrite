use ferrite::tool_parser::extract_tool_calls_from;
use ferrite::tools::ToolRegistry;

#[test]
fn parses_named_xml_tool_call_after_unicode_text() {
    let registry = ToolRegistry::new();
    let definitions = registry.get_definitions();
    let response = r#"好的，讓我先探索一下你的專案結構。

<tool_calls>
  <tool_call name="list_files">
    <recursive>true</recursive>
    <path>.</path>
  </tool_call>
</tool_calls>"#;

    let calls = extract_tool_calls_from(response, &definitions)
        .expect("the named XML tool call should be recognized");

    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].name, "list_files");

    let args: serde_json::Value =
        serde_json::from_str(&calls[0].arguments).expect("tool arguments should be JSON");
    assert_eq!(args["recursive"], true);
    assert_eq!(args["path"], ".");
}
