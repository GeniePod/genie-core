/// Voice output formatter.
///
/// LLMs output markdown, bullet points, long paragraphs, and special characters
/// that sound terrible when spoken by TTS. This module cleans up LLM output
/// for natural-sounding voice delivery.

/// Clean LLM text for TTS output.
pub fn for_voice(text: &str) -> String {
    let mut result = text.to_string();

    // Strip markdown formatting.
    result = strip_markdown(&result);

    // Normalize whitespace.
    result = normalize_whitespace(&result);

    // Shorten if too long for voice (>3 sentences).
    result = truncate_for_voice(&result, 3);

    // Clean up special characters that TTS handles badly.
    result = clean_for_tts(&result);

    result.trim().to_string()
}

/// Strip markdown formatting (bold, italic, headers, links, code blocks).
fn strip_markdown(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut in_code_block = false;

    for line in text.lines() {
        let trimmed = line.trim();

        // Skip code block markers.
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        // Skip lines inside code blocks.
        if in_code_block {
            continue;
        }

        // Strip header markers.
        let line = if trimmed.starts_with('#') {
            trimmed.trim_start_matches('#').trim()
        } else {
            trimmed
        };

        // Strip bullet points.
        let line = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .or_else(|| line.strip_prefix("• "))
            .unwrap_or(line);

        // Strip numbered lists.
        let line = strip_numbered_prefix(line);

        if !line.is_empty() {
            if !result.is_empty() {
                result.push(' ');
            }
            result.push_str(line);
        }
    }

    // Strip inline formatting: **bold**, *italic*, `code`, [links](url).
    #[allow(clippy::collapsible_str_replace)]
    let result = result
        .replace("**", "")
        .replace("__", "")
        .replace('*', "")
        .replace('`', "");

    // Strip markdown links: [text](url) → text
    strip_links(&result)
}

fn strip_numbered_prefix(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut i = 0;

    // Skip digits.
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }

    // Check for ". " after digits.
    if i > 0 && i < bytes.len() - 1 && bytes[i] == b'.' && bytes[i + 1] == b' ' {
        &line[i + 2..]
    } else {
        line
    }
}

fn strip_links(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '[' {
            // Collect link text.
            let mut link_text = String::new();
            for c in chars.by_ref() {
                if c == ']' {
                    break;
                }
                link_text.push(c);
            }
            // Skip (url) part.
            if chars.peek() == Some(&'(') {
                chars.next(); // skip '('
                for c in chars.by_ref() {
                    if c == ')' {
                        break;
                    }
                }
            }
            result.push_str(&link_text);
        } else {
            result.push(ch);
        }
    }

    result
}

/// Normalize whitespace: collapse multiple spaces, trim.
fn normalize_whitespace(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut last_was_space = false;

    for ch in text.chars() {
        if ch.is_whitespace() {
            if !last_was_space {
                result.push(' ');
                last_was_space = true;
            }
        } else {
            result.push(ch);
            last_was_space = false;
        }
    }

    result
}

/// Truncate to N sentences for voice output.
fn truncate_for_voice(text: &str, max_sentences: usize) -> String {
    let mut sentences = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        current.push(ch);
        if is_sentence_boundary(ch) && current.len() > 5 {
            sentences.push(current.trim().to_string());
            current = String::new();
            if sentences.len() >= max_sentences {
                break;
            }
        }
    }

    // Include trailing fragment if we have room.
    let trailing = current.trim().to_string();
    if !trailing.is_empty() && sentences.len() < max_sentences {
        sentences.push(trailing);
    }

    sentences.join(" ")
}

fn is_sentence_boundary(ch: char) -> bool {
    matches!(ch, '.' | '!' | '?' | '。' | '！' | '？')
}

/// Clean special characters that TTS engines handle poorly.
fn clean_for_tts(text: &str) -> String {
    text.replace("...", ", ")
        .replace(" - ", ", ")
        .replace(" — ", ", ")
        .replace(" – ", ", ")
        .replace("(", ", ")
        .replace(")", ", ")
        .replace("[", "")
        .replace("]", "")
        .replace("{", "")
        .replace("}", "")
        .replace("\"", "")
        .replace("'s", "s") // possessive sounds weird with some TTS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_bold_and_italic() {
        assert_eq!(
            for_voice("This is **bold** and *italic*"),
            "This is bold and italic"
        );
    }

    #[test]
    fn strip_markdown_headers() {
        assert_eq!(for_voice("## Weather\nIt's sunny."), "Weather Its sunny.");
    }

    #[test]
    fn strip_bullet_points() {
        let input = "Here's what I found:\n- Item one\n- Item two\n- Item three";
        let output = for_voice(input);
        assert!(output.contains("Item one"));
        assert!(!output.contains("- "));
    }

    #[test]
    fn strip_code_blocks() {
        let input = "Here's the code:\n```\nlet x = 5;\n```\nThat's it.";
        let output = for_voice(input);
        assert!(!output.contains("let x"));
        assert!(output.contains("Thats it"));
    }

    #[test]
    fn strip_links() {
        let input = "Check [this guide](https://example.com) for details.";
        let output = for_voice(input);
        assert!(output.contains("this guide"));
        assert!(!output.contains("https://"));
    }

    #[test]
    fn truncate_long_response() {
        let input =
            "First sentence. Second sentence. Third sentence. Fourth sentence. Fifth sentence.";
        let output = for_voice(input);
        assert!(output.contains("First"));
        assert!(output.contains("Third"));
        assert!(!output.contains("Fourth"));
    }

    #[test]
    fn truncate_handles_chinese_punctuation() {
        let input = "第一句。第二句！第三句？第四句。";
        let output = for_voice(input);
        assert!(output.contains("第一句"));
        assert!(output.contains("第三句"));
        assert!(!output.contains("第四句"));
    }

    #[test]
    fn clean_special_chars() {
        let input = "The temperature is 72°F (about 22°C)...nice!";
        let output = for_voice(input);
        assert!(!output.contains("("));
        assert!(!output.contains("..."));
    }

    #[test]
    fn empty_input() {
        assert_eq!(for_voice(""), "");
    }

    #[test]
    fn already_clean() {
        assert_eq!(for_voice("The lights are on."), "The lights are on.");
    }
}
