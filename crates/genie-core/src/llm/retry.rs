use std::time::Duration;

use anyhow::Result;

use super::client::{LlmClient, Message};

/// LLM client with retry logic and graceful degradation.
///
/// Wraps LlmClient to handle:
/// - Connection failures (llama.cpp restarting during governor mode switch)
/// - Timeout on slow responses (large context, cold model)
/// - Graceful fallback messages when LLM is completely unavailable
pub struct RetryLlmClient {
    inner: LlmClient,
    max_retries: u32,
    retry_delay: Duration,
    timeout: Duration,
}

impl RetryLlmClient {
    pub fn new(client: LlmClient) -> Self {
        Self {
            inner: client,
            max_retries: 2,
            retry_delay: Duration::from_secs(2),
            timeout: Duration::from_secs(60),
        }
    }

    pub fn with_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Chat with retry. Returns a graceful error message instead of propagating errors.
    pub async fn chat_or_fallback(&self, messages: &[Message], max_tokens: Option<u32>) -> String {
        match self.chat_with_retry(messages, max_tokens).await {
            Ok(response) => response,
            Err(e) => {
                tracing::warn!(error = %e, "LLM unavailable, using fallback");
                fallback_response(messages)
            }
        }
    }

    /// Chat with retry logic.
    pub async fn chat_with_retry(
        &self,
        messages: &[Message],
        max_tokens: Option<u32>,
    ) -> Result<String> {
        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            if attempt > 0 {
                tracing::info!(attempt, "retrying LLM request");
                tokio::time::sleep(self.retry_delay).await;
            }

            match tokio::time::timeout(self.timeout, self.inner.chat(messages, max_tokens)).await {
                Ok(Ok(response)) => return Ok(response),
                Ok(Err(e)) => {
                    tracing::warn!(attempt, error = %e, "LLM request failed");
                    last_error = Some(e);
                }
                Err(_) => {
                    tracing::warn!(
                        attempt,
                        timeout_secs = self.timeout.as_secs(),
                        "LLM request timed out"
                    );
                    last_error = Some(anyhow::anyhow!("timeout after {}s", self.timeout.as_secs()));
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow::anyhow!("LLM unavailable")))
    }

    /// Stream with retry (first attempt only — streaming can't easily retry mid-stream).
    pub async fn stream_or_fallback<F>(
        &self,
        messages: &[Message],
        max_tokens: Option<u32>,
        on_token: F,
    ) -> String
    where
        F: FnMut(&str),
    {
        match tokio::time::timeout(
            self.timeout,
            self.inner.chat_stream(messages, max_tokens, on_token),
        )
        .await
        {
            Ok(Ok(response)) => response,
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "LLM stream failed");
                fallback_response(messages)
            }
            Err(_) => {
                tracing::warn!("LLM stream timed out");
                fallback_response(messages)
            }
        }
    }

    /// Health check with short timeout.
    pub async fn is_available(&self) -> bool {
        tokio::time::timeout(Duration::from_secs(3), self.inner.health())
            .await
            .unwrap_or(false)
    }

    /// Get a reference to the inner client.
    pub fn inner(&self) -> &LlmClient {
        &self.inner
    }
}

/// Generate a useful fallback response when the LLM is offline.
///
/// Instead of "error: connection refused", give the user something actionable.
fn fallback_response(messages: &[Message]) -> String {
    // Check if the last user message looks like a tool request.
    let last_user = messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.to_lowercase());

    if let Some(text) = &last_user {
        if text.contains("time") {
            // We can answer this without the LLM.
            let now = chrono_free_time();
            return format!(
                "The LLM is currently restarting. But I can tell you the time: {}",
                now
            );
        }
        if text.contains("weather") {
            return "The LLM is currently restarting. I'll check the weather once it's back — try again in a few seconds.".into();
        }
    }

    "I'm momentarily unavailable — the AI model is restarting. Please try again in a few seconds."
        .into()
}

fn chrono_free_time() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    #[cfg(unix)]
    {
        let time_t = secs as libc::time_t;
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::localtime_r(&time_t, &mut tm) };
        if !result.is_null() {
            return format!("{:02}:{:02}:{:02}", tm.tm_hour, tm.tm_min, tm.tm_sec);
        }
    }

    format!("Unix: {}", secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_for_time_request() {
        let messages = vec![Message {
            role: "user".into(),
            content: "what time is it".into(),
        }];
        let response = fallback_response(&messages);
        assert!(response.contains("time"));
        assert!(!response.contains("error"));
    }

    #[test]
    fn fallback_for_generic_request() {
        let messages = vec![Message {
            role: "user".into(),
            content: "tell me a joke".into(),
        }];
        let response = fallback_response(&messages);
        assert!(response.contains("unavailable") || response.contains("restarting"));
    }

    #[test]
    fn fallback_for_weather_request() {
        let messages = vec![Message {
            role: "user".into(),
            content: "weather in Denver".into(),
        }];
        let response = fallback_response(&messages);
        assert!(response.contains("weather"));
    }

    #[test]
    fn retry_client_defaults() {
        let client = LlmClient::new("127.0.0.1", 8080);
        let retry = RetryLlmClient::new(client);
        assert_eq!(retry.max_retries, 2);
        assert_eq!(retry.timeout, Duration::from_secs(60));
    }

    #[test]
    fn retry_client_builder() {
        let client = LlmClient::new("127.0.0.1", 8080);
        let retry = RetryLlmClient::new(client)
            .with_retries(5)
            .with_timeout(Duration::from_secs(120));
        assert_eq!(retry.max_retries, 5);
        assert_eq!(retry.timeout, Duration::from_secs(120));
    }
}
