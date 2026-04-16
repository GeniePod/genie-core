use super::dispatch::{ToolCall, ToolDispatcher, ToolResult};

/// Parse a tool call from LLM output and execute it.
///
/// LLMs output tool calls in various formats. This parser handles:
/// 1. Raw JSON: `{"tool": "get_time", "arguments": {}}`
/// 2. Markdown code block: ````json\n{"tool": "get_time"}\n````
/// 3. Embedded in text: `I'll check that. {"tool": "get_weather", "arguments": {"location": "Denver"}}`
/// 4. With extra fields: `{"tool": "set_timer", "arguments": {"seconds": 300}, "reasoning": "..."}`
pub async fn try_tool_call(response: &str, tools: &ToolDispatcher) -> Option<ToolResult> {
    let json_str = extract_json(response)?;
    let call: ToolCall = serde_json::from_str(&json_str).ok()?;

    if call.name.is_empty() {
        return None;
    }

    Some(tools.execute(&call).await)
}

/// Extract the first valid JSON object from LLM output.
///
/// Handles: raw JSON, markdown fenced blocks, embedded in prose.
fn extract_json(text: &str) -> Option<String> {
    let trimmed = text.trim();

    // 1. Try the whole response as JSON.
    if trimmed.starts_with('{')
        && trimmed.ends_with('}')
        && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
    {
        return Some(trimmed.to_string());
    }

    // 2. Try extracting from markdown code block: ```json ... ``` or ``` ... ```
    if let Some(json) = extract_from_code_block(trimmed) {
        return Some(json);
    }

    // 3. Try finding JSON embedded in text.
    if let Some(json) = extract_embedded_json(trimmed) {
        return Some(json);
    }

    None
}

/// Extract JSON from markdown fenced code blocks.
fn extract_from_code_block(text: &str) -> Option<String> {
    // Match ```json\n...\n``` or ```\n...\n```
    let patterns = ["```json\n", "```json\r\n", "```\n", "```\r\n"];

    for pattern in &patterns {
        if let Some(start) = text.find(pattern) {
            let content_start = start + pattern.len();
            if let Some(end) = text[content_start..].find("```") {
                let json_str = text[content_start..content_start + end].trim();
                if json_str.starts_with('{')
                    && serde_json::from_str::<serde_json::Value>(json_str).is_ok()
                {
                    return Some(json_str.to_string());
                }
            }
        }
    }

    None
}

/// Find a JSON object embedded in prose text.
fn extract_embedded_json(text: &str) -> Option<String> {
    // Find the first '{' and try to match it with a closing '}'.
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'{' {
            // Try to find the matching closing brace.
            let mut depth = 0;
            let mut in_string = false;
            let mut escape = false;

            for j in i..bytes.len() {
                if escape {
                    escape = false;
                    continue;
                }

                match bytes[j] {
                    b'\\' if in_string => escape = true,
                    b'"' => in_string = !in_string,
                    b'{' if !in_string => depth += 1,
                    b'}' if !in_string => {
                        depth -= 1;
                        if depth == 0 {
                            let candidate = &text[i..=j];
                            if serde_json::from_str::<serde_json::Value>(candidate).is_ok() {
                                return Some(candidate.to_string());
                            }
                            break;
                        }
                    }
                    _ => {}
                }
            }
        }
        i += 1;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_raw_json() {
        let input = r#"{"tool": "get_time", "arguments": {}}"#;
        let json = extract_json(input).unwrap();
        let call: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(call.name, "get_time");
    }

    #[test]
    fn parse_markdown_code_block() {
        let input = "Sure, let me check the time for you.\n\n```json\n{\"tool\": \"get_time\", \"arguments\": {}}\n```";
        let json = extract_json(input).unwrap();
        let call: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(call.name, "get_time");
    }

    #[test]
    fn parse_markdown_block_no_language() {
        let input = "```\n{\"tool\": \"set_timer\", \"arguments\": {\"seconds\": 300}}\n```";
        let json = extract_json(input).unwrap();
        let call: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(call.name, "set_timer");
    }

    #[test]
    fn parse_embedded_in_prose() {
        let input = "I'll turn on the lights for you. {\"tool\": \"home_control\", \"arguments\": {\"entity\": \"living room light\", \"action\": \"turn_on\"}} Done!";
        let json = extract_json(input).unwrap();
        let call: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(call.name, "home_control");
    }

    #[test]
    fn parse_with_extra_fields() {
        let input = r#"{"tool": "get_weather", "arguments": {"location": "Tokyo"}, "reasoning": "User asked about weather"}"#;
        let json = extract_json(input).unwrap();
        let call: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(call.name, "get_weather");
    }

    #[test]
    fn no_tool_call_in_normal_response() {
        let input = "The current time is 3:45 PM. Is there anything else I can help with?";
        assert!(extract_json(input).is_none());
    }

    #[test]
    fn nested_json_in_arguments() {
        let input = r#"{"tool": "home_control", "arguments": {"entity": "thermostat", "action": "set_temperature", "value": 72}}"#;
        let json = extract_json(input).unwrap();
        let call: ToolCall = serde_json::from_str(&json).unwrap();
        assert_eq!(call.name, "home_control");
        assert_eq!(call.arguments["value"], 72);
    }

    #[test]
    fn empty_tool_name_rejected() {
        let input = r#"{"tool": "", "arguments": {}}"#;
        let json = extract_json(input).unwrap();
        let call: ToolCall = serde_json::from_str(&json).unwrap();
        assert!(call.name.is_empty()); // Parser returns it, but try_tool_call filters it
    }
}
