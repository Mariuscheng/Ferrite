use serde::{Deserialize, Serialize};

use crate::tools::ToolDefinition;

/// A parsed tool-call extracted from an AI text response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRequest {
    pub name: String,
    pub arguments: String,
}

/// Extract tool calls from a text response using multiple strategies:
/// 1. JSON code blocks with tool/arguments
/// 2. Standalone JSON objects
/// 3. Pure JSON object
/// 4. Manual XML tag parsing
pub fn extract_tool_calls_from(
    content: &str,
    tool_definitions: &[ToolDefinition],
) -> Option<Vec<ToolCallRequest>> {
    let mut tools = Vec::new();

    // Strategy 0: Strip <tool_call> or <tool_calls> wrappers (AI may wrap tools)
    let clean = strip_tool_call_wrappers(content);
    let haystack = clean.as_deref().unwrap_or(content);

    // Strategy 1: JSON code blocks
    if let Ok(re) = regex_lite::Regex::new(r"```json\s*(\{[\s\S]*?\})\s*```") {
        for cap in re.captures_iter(haystack) {
            if let Some(m) = cap.get(1) {
                if let Some(tc) = parse_json_tool_call(m.as_str(), tool_definitions) {
                    tools.push(tc);
                }
            }
        }
    }

    // Strategy 2: Standalone JSON objects
    if tools.is_empty() {
        if let Ok(re) = regex_lite::Regex::new(
            r###"\{[\s\S]*?\x22tool\x22[\s\S]*?\x22arguments\x22[\s\S]*?\}"###,
        ) {
            for cap in re.captures_iter(haystack) {
                let m = cap.get(0).unwrap().as_str();
                if let Some(tc) = parse_json_tool_call(m, tool_definitions) {
                    tools.push(tc);
                }
            }
        }
    }

    // Strategy 3: Pure JSON
    if tools.is_empty() && haystack.trim().starts_with('{') {
        if let Some(tc) = parse_json_tool_call(haystack.trim(), tool_definitions) {
            tools.push(tc);
        }
    }

    // Strategy 4: Manual XML tool call parser
    if tools.is_empty() {
        let xml_tools = parse_xml_tool_calls_manual(haystack, tool_definitions);
        tools.extend(xml_tools);
    }

    if tools.is_empty() {
        None
    } else {
        Some(tools)
    }
}

/// Parse direct tool tags and the common `<tool_call name="...">` wrapper format.
pub fn parse_xml_tool_calls_manual(
    content: &str,
    tool_definitions: &[ToolDefinition],
) -> Vec<ToolCallRequest> {
    let mut tools = Vec::new();

    let mut remaining = content;
    while let Some(start) = remaining.find("<tool_call") {
        let after_tag = &remaining[start + "<tool_call".len()..];
        if after_tag.starts_with('s') {
            remaining = after_tag;
            continue;
        }
        if !after_tag.starts_with('>') && !after_tag.starts_with(char::is_whitespace) {
            remaining = after_tag;
            continue;
        }

        let Some(open_end) = after_tag.find('>') else {
            break;
        };
        let opening_tag = &after_tag[..=open_end];
        let body = &after_tag[open_end + 1..];
        let Some(close_index) = body.find("</tool_call>") else {
            break;
        };
        let inner = &body[..close_index];

        let named_tool = xml_attribute(opening_tag, "name")
            .or_else(|| xml_attribute(opening_tag, "tool"));
        if let Some(raw_name) = named_tool {
            if let Some(name) = fuzzy_tool_name(&raw_name, tool_definitions) {
                let args = extract_xml_params_manual(inner);
                tools.push(ToolCallRequest {
                    name,
                    arguments: serde_json::to_string(&args)
                        .unwrap_or_else(|_| inner.trim().to_string()),
                });
            }
        }

        remaining = &body[close_index + "</tool_call>".len()..];
    }

    for definition in tool_definitions {
        let open_tag = format!("<{}", definition.name);
        let close_tag = format!("</{}>", definition.name);
        let mut remaining = content;

        while let Some(start) = remaining.find(&open_tag) {
            let after_name = &remaining[start + open_tag.len()..];
            if !after_name.starts_with('>') && !after_name.starts_with(char::is_whitespace) {
                remaining = after_name;
                continue;
            }
            let Some(open_end) = after_name.find('>') else {
                break;
            };
            let body = &after_name[open_end + 1..];
            let Some(close_index) = body.find(&close_tag) else {
                break;
            };
            let inner = &body[..close_index];
            let args = extract_xml_params_manual(inner);
            tools.push(ToolCallRequest {
                name: definition.name.clone(),
                arguments: serde_json::to_string(&args)
                    .unwrap_or_else(|_| inner.trim().to_string()),
            });
            remaining = &body[close_index + close_tag.len()..];
        }
    }

    tools
}

/// Extract an XML attribute value from an opening tag (e.g., `name="foo"`).
fn xml_attribute(opening_tag: &str, attribute: &str) -> Option<String> {
    for quote in ['"', '\''] {
        let needle = format!("{}={}", attribute, quote);
        if let Some(attribute_start) = opening_tag.find(&needle) {
            let start = attribute_start + needle.len();
            let value = &opening_tag[start..];
            if let Some(end) = value.find(quote) {
                return Some(value[..end].to_string());
            }
        }
    }
    None
}

/// Manual nested param parser: extracts `<key>value</key>` pairs from inner XML.
pub fn extract_xml_params_manual(inner: &str) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    let mut remaining = inner;

    while let Some(start) = remaining.find('<') {
        let after_open = &remaining[start + 1..];
        if after_open.starts_with('/') {
            remaining = &after_open[1..];
            continue;
        }
        let Some(open_end) = after_open.find('>') else {
            break;
        };
        let opening_tag = &after_open[..open_end];
        let key = opening_tag
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim_end_matches('/');
        if key.is_empty() {
            remaining = &after_open[open_end + 1..];
            continue;
        }

        let body = &after_open[open_end + 1..];
        let close_tag = format!("</{}>", key);
        let Some(close_index) = body.find(&close_tag) else {
            remaining = body;
            continue;
        };
        let value = body[..close_index].trim().to_string();
        let value = if value == "true" {
            serde_json::Value::Bool(true)
        } else if value == "false" {
            serde_json::Value::Bool(false)
        } else if let Ok(number) = value.parse::<i64>() {
            serde_json::json!(number)
        } else if let Ok(number) = value.parse::<f64>() {
            serde_json::json!(number)
        } else {
            serde_json::Value::String(value)
        };
        map.insert(key.to_string(), value);
        remaining = &body[close_index + close_tag.len()..];
    }

    serde_json::Value::Object(map)
}

/// Strip outer `<tool_call>` or `<tool_calls>` wrapper tags to expose inner tool tags.
pub fn strip_tool_call_wrappers(content: &str) -> Option<String> {
    if let Ok(re) =
        regex_lite::Regex::new(r"(?s)^\s*<tool_calls?>\s*([\s\S]*?)\s*</tool_calls?>\s*$")
    {
        if let Some(cap) = re.captures(content) {
            if let Some(m) = cap.get(1) {
                let inner = m.as_str().to_string();
                if !inner.is_empty() {
                    return Some(inner);
                }
            }
        }
    }
    None
}

/// Fuzzy-match a raw tool name to a known tool definition, including common aliases.
pub fn fuzzy_tool_name(raw: &str, tool_definitions: &[ToolDefinition]) -> Option<String> {
    let lower = raw.to_lowercase();
    if tool_definitions.iter().any(|t| t.name == lower) {
        return Some(lower);
    }
    let alias_map: &[(&str, &str)] = &[
        ("read", "read_file"),
        ("write", "write_file"),
        ("replace", "replace_in_file"),
        ("edit", "replace_in_file"),
        ("search", "search_files"),
        ("list", "list_files"),
        ("list_files", "list_files"),
        ("execute", "execute_command"),
        ("run", "execute_command"),
        ("shell", "execute_command"),
        ("create_project", "create_project"),
        ("compile", "compile"),
        ("build", "compile"),
        ("run_tests", "run_tests"),
        ("test", "run_tests"),
    ];
    for (alias, target) in alias_map {
        if lower == *alias && tool_definitions.iter().any(|t| t.name == *target) {
            return Some(target.to_string());
        }
    }
    None
}

/// Try to parse a JSON string as a tool call object with `tool`/`name` + `arguments` fields.
pub fn parse_json_tool_call(
    json_str: &str,
    tool_definitions: &[ToolDefinition],
) -> Option<ToolCallRequest> {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
        if let Some(tool_name) = v.get("tool").and_then(|v| v.as_str()) {
            if tool_definitions.iter().any(|t| t.name == tool_name) {
                let args = v
                    .get("arguments")
                    .map(|a| a.to_string())
                    .unwrap_or_default();
                return Some(ToolCallRequest {
                    name: tool_name.to_string(),
                    arguments: args,
                });
            }
        }
        if let Some(tool_name) = v.get("name").and_then(|v| v.as_str()) {
            if tool_definitions.iter().any(|t| t.name == tool_name) {
                let args = v
                    .get("arguments")
                    .map(|a| a.to_string())
                    .unwrap_or_default();
                return Some(ToolCallRequest {
                    name: tool_name.to_string(),
                    arguments: args,
                });
            }
        }
    }
    None
}

/// Extract the first complete JSON object from a string (balanced braces).
pub fn extract_first_json_object(text: &str) -> Option<String> {
    let start = text.find('{')?;
    let mut depth = 0_i32;
    let mut end_idx = None;

    for (i, ch) in text[start..].char_indices() {
        if ch == '{' {
            depth += 1;
        } else if ch == '}' {
            depth -= 1;
            if depth == 0 {
                end_idx = Some(start + i + 1);
                break;
            }
        }
    }

    end_idx.map(|end| text[start..end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::ToolRegistry;

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
}