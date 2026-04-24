use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const TELEGRAM_MAX_MESSAGE_LEN: usize = 4096;

#[derive(Debug, Clone)]
pub struct TelegramRuntimeConfig {
    pub api_base: String,
    pub bot_token: String,
    pub core_base_url: String,
    pub poll_timeout_secs: u64,
    pub allowed_chat_ids: Vec<i64>,
    pub allow_all_chats: bool,
}

pub async fn run(config: TelegramRuntimeConfig) -> Result<()> {
    let client = Client::builder()
        .user_agent("GenieClaw/1.0")
        .timeout(Duration::from_secs(
            config.poll_timeout_secs.saturating_add(15),
        ))
        .build()
        .context("failed to build Telegram HTTP client")?;

    let api = TelegramApi::new(client, config);
    let mut offset = match api.bootstrap_offset().await {
        Ok(offset) => offset,
        Err(e) => {
            tracing::warn!(error = %e, "telegram bootstrap failed; starting from offset 0");
            0
        }
    };

    loop {
        match api.get_updates(offset).await {
            Ok(updates) => {
                for update in updates {
                    offset = offset.max(update.update_id.saturating_add(1));
                    if let Err(e) = api.handle_update(update).await {
                        tracing::warn!(error = %e, "telegram update handling failed");
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "telegram polling failed");
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }
}

struct TelegramApi {
    client: Client,
    config: TelegramRuntimeConfig,
}

impl TelegramApi {
    fn new(client: Client, config: TelegramRuntimeConfig) -> Self {
        Self { client, config }
    }

    async fn bootstrap_offset(&self) -> Result<i64> {
        let updates = self.get_updates_raw(None, 0).await?;
        let next = updates
            .iter()
            .map(|u| u.update_id)
            .max()
            .map(|id| id.saturating_add(1))
            .unwrap_or(0);
        if next > 0 {
            tracing::info!(
                dropped_updates = updates.len(),
                next_offset = next,
                "telegram bootstrap skipped pending updates"
            );
        }
        Ok(next)
    }

    async fn get_updates(&self, offset: i64) -> Result<Vec<TelegramUpdate>> {
        self.get_updates_raw(Some(offset), self.config.poll_timeout_secs)
            .await
    }

    async fn get_updates_raw(
        &self,
        offset: Option<i64>,
        timeout_secs: u64,
    ) -> Result<Vec<TelegramUpdate>> {
        let payload = match offset {
            Some(offset) => serde_json::json!({
                "timeout": timeout_secs,
                "offset": offset,
                "allowed_updates": ["message"]
            }),
            None => serde_json::json!({
                "timeout": timeout_secs,
                "allowed_updates": ["message"]
            }),
        };

        let req = self
            .client
            .post(self.method_url("getUpdates"))
            .json(&payload);

        let resp: TelegramEnvelope<Vec<TelegramUpdate>> = req
            .send()
            .await
            .context("Telegram getUpdates request failed")?
            .error_for_status()
            .context("Telegram getUpdates HTTP error")?
            .json()
            .await
            .context("Telegram getUpdates JSON decode failed")?;

        if !resp.ok {
            anyhow::bail!(
                "Telegram getUpdates API error {}",
                resp.description.unwrap_or_else(|| "unknown error".into())
            );
        }

        Ok(resp.result.unwrap_or_default())
    }

    async fn handle_update(&self, update: TelegramUpdate) -> Result<()> {
        let Some(message) = update.message else {
            return Ok(());
        };

        if message
            .from
            .as_ref()
            .and_then(|u| u.is_bot)
            .unwrap_or(false)
        {
            return Ok(());
        }

        let chat_id = message.chat.id;
        if !self.chat_allowed(chat_id) {
            let _ = self
                .send_text(chat_id, "This chat is not authorized for GenieClaw.")
                .await;
            return Ok(());
        }

        let Some(text) = message
            .text
            .as_deref()
            .map(str::trim)
            .filter(|t| !t.is_empty())
        else {
            let _ = self
                .send_text(chat_id, "Telegram v1 supports text messages only.")
                .await;
            return Ok(());
        };

        let normalized = strip_bot_mention(text);
        let normalized = normalized.trim();
        if normalized.is_empty() {
            return Ok(());
        }

        let core_response = self.chat_core(chat_id, normalized).await?;
        self.send_text(chat_id, &core_response).await?;
        Ok(())
    }

    async fn chat_core(&self, chat_id: i64, text: &str) -> Result<String> {
        let request = CoreChatRequest {
            message: text.to_string(),
            conversation_id: Some(format!("telegram-{chat_id}")),
        };

        let response: CoreChatResponse = self
            .client
            .post(format!("{}/api/chat", self.config.core_base_url))
            .header("X-Genie-Origin", "telegram")
            .json(&request)
            .send()
            .await
            .context("local GenieClaw /api/chat request failed")?
            .error_for_status()
            .context("local GenieClaw /api/chat HTTP error")?
            .json()
            .await
            .context("failed to decode GenieClaw /api/chat response")?;

        Ok(response.response)
    }

    async fn send_text(&self, chat_id: i64, text: &str) -> Result<()> {
        for chunk in split_message(text) {
            let payload = serde_json::json!({
                "chat_id": chat_id,
                "text": chunk,
            });

            let resp: TelegramEnvelope<serde_json::Value> = self
                .client
                .post(self.method_url("sendMessage"))
                .json(&payload)
                .send()
                .await
                .context("Telegram sendMessage request failed")?
                .error_for_status()
                .context("Telegram sendMessage HTTP error")?
                .json()
                .await
                .context("Telegram sendMessage JSON decode failed")?;

            if !resp.ok {
                anyhow::bail!(
                    "Telegram sendMessage API error {}",
                    resp.description.unwrap_or_else(|| "unknown error".into())
                );
            }
        }

        Ok(())
    }

    fn chat_allowed(&self, chat_id: i64) -> bool {
        self.config.allow_all_chats || self.config.allowed_chat_ids.iter().any(|id| *id == chat_id)
    }

    fn method_url(&self, method: &str) -> String {
        format!(
            "{}/bot{}/{}",
            self.config.api_base.trim_end_matches('/'),
            self.config.bot_token,
            method
        )
    }
}

fn strip_bot_mention(text: &str) -> String {
    text.split_whitespace()
        .filter(|part| !part.starts_with('@'))
        .collect::<Vec<_>>()
        .join(" ")
}

fn split_message(message: &str) -> Vec<String> {
    if message.chars().count() <= TELEGRAM_MAX_MESSAGE_LEN {
        return vec![message.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = message;

    while !remaining.is_empty() {
        let split_idx = remaining
            .char_indices()
            .nth(TELEGRAM_MAX_MESSAGE_LEN)
            .map(|(idx, _)| idx)
            .unwrap_or(remaining.len());

        if split_idx == remaining.len() {
            chunks.push(remaining.to_string());
            break;
        }

        let search_area = &remaining[..split_idx];
        let chunk_end = search_area
            .rfind('\n')
            .or_else(|| search_area.rfind(' '))
            .unwrap_or(split_idx);

        let end = if chunk_end == 0 { split_idx } else { chunk_end };
        chunks.push(remaining[..end].trim().to_string());
        remaining = remaining[end..].trim_start();
    }

    chunks
}

#[derive(Debug, Deserialize)]
struct TelegramEnvelope<T> {
    ok: bool,
    #[serde(default)]
    result: Option<T>,
    #[serde(default)]
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramUpdate {
    update_id: i64,
    #[serde(default)]
    message: Option<TelegramMessage>,
}

#[derive(Debug, Deserialize)]
struct TelegramMessage {
    chat: TelegramChat,
    #[serde(default)]
    from: Option<TelegramUser>,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TelegramChat {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct TelegramUser {
    #[serde(default)]
    is_bot: Option<bool>,
}

#[derive(Debug, Serialize)]
struct CoreChatRequest {
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    conversation_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CoreChatResponse {
    response: String,
}

#[cfg(test)]
mod tests {
    use super::{TELEGRAM_MAX_MESSAGE_LEN, split_message, strip_bot_mention};

    #[test]
    fn telegram_split_keeps_short_message() {
        let chunks = split_message("hello");
        assert_eq!(chunks, vec!["hello"]);
    }

    #[test]
    fn telegram_split_breaks_long_message() {
        let long = "x".repeat(TELEGRAM_MAX_MESSAGE_LEN + 10);
        let chunks = split_message(&long);
        assert_eq!(chunks.len(), 2);
        assert!(
            chunks
                .iter()
                .all(|c| c.chars().count() <= TELEGRAM_MAX_MESSAGE_LEN)
        );
    }

    #[test]
    fn telegram_strip_bot_mentions() {
        assert_eq!(strip_bot_mention("@geniebot hello there"), "hello there");
    }
}
