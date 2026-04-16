use crate::llm::{LlmClient, Message};

/// Context window manager.
///
/// LLMs have limited context windows (2K-16K tokens on Jetson).
/// This module manages the conversation context to maximize
/// useful information within the window:
///
/// 1. System prompt (always included, ~300-500 tokens)
/// 2. Summary of older conversation (compressed by LLM)
/// 3. Recent messages (verbatim, most recent N turns)
///
/// When the conversation exceeds `max_messages`, older messages
/// are summarized into a single paragraph and injected as a
/// system message at the start of the context.
pub struct ContextManager {
    max_messages: usize,
    summary_threshold: usize,
    current_summary: Option<String>,
}

impl ContextManager {
    pub fn new(max_messages: usize) -> Self {
        Self {
            max_messages,
            summary_threshold: max_messages / 2,
            current_summary: None,
        }
    }

    /// Build the message list for the LLM, managing context window.
    ///
    /// Returns: [system_prompt, (optional summary), recent_messages...]
    pub fn build_context(&self, system_prompt: &str, all_messages: &[Message]) -> Vec<Message> {
        let mut context = Vec::new();

        // 1. System prompt (always first).
        context.push(Message {
            role: "system".into(),
            content: system_prompt.to_string(),
        });

        // 2. Inject summary of older conversation if available.
        if let Some(summary) = &self.current_summary {
            context.push(Message {
                role: "system".into(),
                content: format!("Summary of earlier conversation: {}", summary),
            });
        }

        // 3. Recent messages (last N).
        let start = all_messages.len().saturating_sub(self.max_messages);
        context.extend_from_slice(&all_messages[start..]);

        context
    }

    /// Check if conversation needs summarization and do it.
    ///
    /// Called after each message. If the conversation exceeds the threshold,
    /// summarizes the older messages via the LLM and stores the summary.
    pub async fn maybe_summarize(&mut self, all_messages: &[Message], llm: &LlmClient) {
        if all_messages.len() < self.max_messages + self.summary_threshold {
            return;
        }

        // Take the messages that will be dropped from context.
        let end = all_messages.len().saturating_sub(self.max_messages);
        if end == 0 {
            return;
        }

        let old_messages = &all_messages[..end];
        let summary = summarize_messages(old_messages, llm).await;

        if let Some(s) = summary {
            tracing::info!(
                old_messages = old_messages.len(),
                summary_len = s.len(),
                "conversation summarized"
            );
            self.current_summary = Some(s);
        }
    }

    /// Get the current summary (for persistence).
    pub fn summary(&self) -> Option<&str> {
        self.current_summary.as_deref()
    }

    /// Restore a previously saved summary.
    pub fn set_summary(&mut self, summary: String) {
        self.current_summary = Some(summary);
    }

    /// Estimate token count (rough: 1 token ≈ 4 chars for English).
    pub fn estimate_tokens(messages: &[Message]) -> usize {
        messages.iter().map(|m| m.content.len() / 4 + 5).sum()
    }
}

/// Ask the LLM to summarize a batch of messages.
async fn summarize_messages(messages: &[Message], llm: &LlmClient) -> Option<String> {
    if messages.is_empty() {
        return None;
    }

    // Build a transcript of the messages.
    let transcript: String = messages
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n");

    let summary_request = vec![
        Message {
            role: "system".into(),
            content: "Summarize this conversation in 2-3 sentences. \
                     Focus on: key facts learned about the user, \
                     decisions made, and any ongoing context. \
                     Be concise — this summary replaces the original messages."
                .into(),
        },
        Message {
            role: "user".into(),
            content: transcript,
        },
    ];

    match llm.chat(&summary_request, Some(200)).await {
        Ok(summary) => Some(summary.trim().to_string()),
        Err(e) => {
            tracing::warn!(error = %e, "failed to summarize conversation");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_messages(n: usize) -> Vec<Message> {
        (0..n)
            .map(|i| Message {
                role: if i % 2 == 0 { "user" } else { "assistant" }.into(),
                content: format!("Message {}", i),
            })
            .collect()
    }

    #[test]
    fn build_context_short_conversation() {
        let ctx = ContextManager::new(20);
        let messages = make_messages(5);
        let context = ctx.build_context("system prompt", &messages);

        // System prompt + 5 messages.
        assert_eq!(context.len(), 6);
        assert_eq!(context[0].role, "system");
        assert_eq!(context[0].content, "system prompt");
        assert_eq!(context[1].content, "Message 0");
    }

    #[test]
    fn build_context_long_conversation_truncates() {
        let ctx = ContextManager::new(10);
        let messages = make_messages(30);
        let context = ctx.build_context("system prompt", &messages);

        // System prompt + last 10 messages.
        assert_eq!(context.len(), 11);
        assert_eq!(context[1].content, "Message 20"); // First kept message.
        assert_eq!(context[10].content, "Message 29"); // Last message.
    }

    #[test]
    fn build_context_with_summary() {
        let mut ctx = ContextManager::new(10);
        ctx.set_summary("User is building GeniePod, prefers dark mode.".into());

        let messages = make_messages(15);
        let context = ctx.build_context("system prompt", &messages);

        // System prompt + summary + last 10 messages = 12.
        assert_eq!(context.len(), 12);
        assert_eq!(context[0].role, "system");
        assert!(context[1].content.contains("Summary of earlier"));
        assert!(context[1].content.contains("GeniePod"));
    }

    #[test]
    fn estimate_tokens_rough() {
        let messages = vec![
            Message {
                role: "user".into(),
                content: "Hello, how are you?".into(), // 19 chars ≈ 4 tokens + 5 overhead
            },
            Message {
                role: "assistant".into(),
                content: "I'm doing great, thanks for asking!".into(), // 35 chars ≈ 8 tokens + 5
            },
        ];

        let tokens = ContextManager::estimate_tokens(&messages);
        assert!(tokens > 10);
        assert!(tokens < 30);
    }

    #[test]
    fn summary_persistence() {
        let mut ctx = ContextManager::new(20);
        assert!(ctx.summary().is_none());

        ctx.set_summary("test summary".into());
        assert_eq!(ctx.summary(), Some("test summary"));
    }
}
