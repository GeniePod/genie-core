//! Deterministic routing for high-frequency utility requests.
//!
//! These intents should not depend on the LLM selecting the right tool. The
//! scope is intentionally small: status, time, and diagnostics where arguments
//! are unambiguous and repeated daily usefulness matters.

use super::ToolCall;

pub fn route(text: &str) -> Option<ToolCall> {
    let normalized = normalize(text);
    if normalized.is_empty() {
        return None;
    }

    if asks_memory_status(&normalized) {
        return Some(tool("memory_status", serde_json::json!({})));
    }

    if asks_system_status(&normalized) || asks_home_assistant_status(&normalized) {
        return Some(tool("system_info", serde_json::json!({})));
    }

    if asks_current_time(&normalized) {
        return Some(tool("get_time", serde_json::json!({})));
    }

    None
}

fn tool(name: &str, arguments: serde_json::Value) -> ToolCall {
    ToolCall {
        name: name.to_string(),
        arguments,
    }
}

fn normalize(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .replace(|c: char| !c.is_alphanumeric() && !c.is_whitespace(), " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn asks_memory_status(text: &str) -> bool {
    contains_any(
        text,
        &[
            "memory status",
            "memory health",
            "memory database",
            "memory diagnostics",
            "memory index",
        ],
    )
}

fn asks_home_assistant_status(text: &str) -> bool {
    contains_any(
        text,
        &[
            "home assistant status",
            "home assistant connected",
            "home assistant connection",
            "is home assistant connected",
            "ha status",
            "ha connected",
        ],
    )
}

fn asks_system_status(text: &str) -> bool {
    matches!(
        text,
        "system status"
            | "geniepod status"
            | "genie status"
            | "status of geniepod"
            | "status of genie"
            | "uptime"
            | "load average"
            | "governor status"
    )
}

fn asks_current_time(text: &str) -> bool {
    matches!(
        text,
        "what time is it"
            | "what is the time"
            | "whats the time"
            | "current time"
            | "tell me the time"
            | "what date is it"
            | "what is today"
            | "what day is it"
            | "date and time"
    )
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_home_assistant_status_to_system_info() {
        let call = route("home assistant status").unwrap();
        assert_eq!(call.name, "system_info");
    }

    #[test]
    fn routes_memory_health_to_memory_status() {
        let call = route("check memory health").unwrap();
        assert_eq!(call.name, "memory_status");
    }

    #[test]
    fn routes_time_question_to_get_time() {
        let call = route("what time is it?").unwrap();
        assert_eq!(call.name, "get_time");
    }

    #[test]
    fn does_not_route_ambiguous_time_reference() {
        assert!(route("what time is my meeting").is_none());
    }
}
