use anyhow::Result;
use serde_json::Value;
use std::time::Duration;

const DUCKDUCKGO_INSTANT_ANSWER_URL: &str = "https://api.duckduckgo.com/";
const MAX_RESULTS: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchItem {
    title: Option<String>,
    text: String,
    url: Option<String>,
}

pub async fn search(query: &str, limit: usize) -> Result<String> {
    let query = query.trim();
    if query.is_empty() {
        return Ok("Please specify what to search for.".to_string());
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent("GeniePod/1.0 local web search")
        .build()?;

    let body = client
        .get(DUCKDUCKGO_INSTANT_ANSWER_URL)
        .query(&[
            ("q", query),
            ("format", "json"),
            ("no_html", "1"),
            ("no_redirect", "1"),
            ("skip_disambig", "1"),
        ])
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;

    format_results(query, &body, limit)
}

pub(crate) fn format_results(query: &str, body: &str, limit: usize) -> Result<String> {
    let value: Value = serde_json::from_str(body)?;
    let mut items = Vec::new();

    collect_answer(&value, &mut items);
    collect_abstract(&value, &mut items);
    collect_result_array(value.get("Results"), &mut items);
    collect_related_topics(value.get("RelatedTopics"), &mut items);

    let mut deduped = Vec::new();
    for item in items {
        if item.text.trim().is_empty() {
            continue;
        }
        let duplicate = deduped.iter().any(|existing: &SearchItem| {
            existing.text.eq_ignore_ascii_case(&item.text) || existing.url == item.url
        });
        if !duplicate {
            deduped.push(item);
        }
    }

    if deduped.is_empty() {
        return Ok(format!(
            "No web search results found for \"{}\".",
            query.trim()
        ));
    }

    let limit = limit.clamp(1, MAX_RESULTS);
    let mut lines = vec![format!("Web search results for \"{}\":", query.trim())];
    for item in deduped.into_iter().take(limit) {
        let text = truncate(&clean_text(&item.text), 260);
        let line = match (item.title.as_deref(), item.url.as_deref()) {
            (Some(title), Some(url)) if !title.eq_ignore_ascii_case(&text) => {
                format!("- {}: {} ({})", clean_text(title), text, url)
            }
            (_, Some(url)) => format!("- {} ({})", text, url),
            (Some(title), None) if !title.eq_ignore_ascii_case(&text) => {
                format!("- {}: {}", clean_text(title), text)
            }
            _ => format!("- {}", text),
        };
        lines.push(line);
    }

    Ok(lines.join("\n"))
}

fn collect_answer(value: &Value, items: &mut Vec<SearchItem>) {
    let Some(answer) = get_str(value, "Answer") else {
        return;
    };
    if answer.is_empty() {
        return;
    }

    items.push(SearchItem {
        title: get_str(value, "AnswerType")
            .filter(|title| !title.is_empty())
            .map(str::to_string),
        text: answer.to_string(),
        url: None,
    });
}

fn collect_abstract(value: &Value, items: &mut Vec<SearchItem>) {
    let Some(text) = get_str(value, "AbstractText").or_else(|| get_str(value, "Abstract")) else {
        return;
    };
    if text.is_empty() {
        return;
    }

    items.push(SearchItem {
        title: get_str(value, "Heading")
            .filter(|heading| !heading.is_empty())
            .map(str::to_string),
        text: text.to_string(),
        url: get_str(value, "AbstractURL")
            .filter(|url| !url.is_empty())
            .map(str::to_string),
    });
}

fn collect_result_array(value: Option<&Value>, items: &mut Vec<SearchItem>) {
    let Some(results) = value.and_then(Value::as_array) else {
        return;
    };

    for result in results {
        collect_result_item(result, items);
    }
}

fn collect_related_topics(value: Option<&Value>, items: &mut Vec<SearchItem>) {
    let Some(topics) = value.and_then(Value::as_array) else {
        return;
    };

    for topic in topics {
        if let Some(children) = topic.get("Topics") {
            collect_related_topics(Some(children), items);
        } else {
            collect_result_item(topic, items);
        }
    }
}

fn collect_result_item(value: &Value, items: &mut Vec<SearchItem>) {
    let Some(text) = get_str(value, "Text") else {
        return;
    };
    if text.is_empty() {
        return;
    }

    items.push(SearchItem {
        title: title_from_text(text),
        text: text.to_string(),
        url: get_str(value, "FirstURL")
            .filter(|url| !url.is_empty())
            .map(str::to_string),
    });
}

fn title_from_text(text: &str) -> Option<String> {
    text.split_once(" - ")
        .map(|(title, _)| clean_text(title))
        .filter(|title| !title.is_empty())
}

fn get_str<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str).map(str::trim)
}

fn clean_text(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn truncate(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{}...", truncated.trim_end())
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_abstract_result() {
        let body = r#"{
            "Heading": "Home Assistant",
            "AbstractText": "Home Assistant is free and open-source software for home automation.",
            "AbstractURL": "https://www.home-assistant.io/",
            "RelatedTopics": []
        }"#;

        let output = format_results("home assistant", body, 3).unwrap();
        assert!(output.contains("Web search results"));
        assert!(output.contains("Home Assistant"));
        assert!(output.contains("https://www.home-assistant.io/"));
    }

    #[test]
    fn formats_nested_related_topics() {
        let body = r#"{
            "RelatedTopics": [
                {
                    "Name": "Group",
                    "Topics": [
                        {
                            "Text": "Matter - Matter is an open smart home connectivity standard.",
                            "FirstURL": "https://example.test/matter"
                        }
                    ]
                }
            ]
        }"#;

        let output = format_results("matter", body, 3).unwrap();
        assert!(output.contains("Matter"));
        assert!(output.contains("https://example.test/matter"));
    }

    #[test]
    fn handles_empty_results() {
        let output = format_results("nope", r#"{"RelatedTopics":[]}"#, 3).unwrap();
        assert_eq!(output, "No web search results found for \"nope\".");
    }

    #[test]
    fn clamps_result_count() {
        let body = r#"{
            "RelatedTopics": [
                {"Text": "One - first", "FirstURL": "https://example.test/1"},
                {"Text": "Two - second", "FirstURL": "https://example.test/2"}
            ]
        }"#;

        let output = format_results("numbers", body, 1).unwrap();
        assert!(output.contains("One"));
        assert!(!output.contains("Two"));
    }
}
