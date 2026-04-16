use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// Home Assistant REST API client.
///
/// Talks to HA Core running locally on :8123.
/// Supports entity state queries, service calls, and fuzzy entity matching.
pub struct HaClient {
    host: String,
    port: u16,
    token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub entity_id: String,
    pub state: String,
    #[serde(default)]
    pub attributes: serde_json::Value,
}

impl Entity {
    /// Friendly name from attributes, or entity_id as fallback.
    pub fn friendly_name(&self) -> &str {
        self.attributes
            .get("friendly_name")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.entity_id)
    }
}

impl HaClient {
    pub fn new(host: &str, port: u16, token: &str) -> Self {
        Self {
            host: host.to_string(),
            port,
            token: token.to_string(),
        }
    }

    /// Get all entity states.
    pub async fn get_states(&self) -> Result<Vec<Entity>> {
        let body = self.http_get("/api/states").await?;
        let entities: Vec<Entity> = serde_json::from_str(&body)?;
        Ok(entities)
    }

    /// Get a single entity state.
    pub async fn get_state(&self, entity_id: &str) -> Result<Entity> {
        let path = format!("/api/states/{}", entity_id);
        let body = self.http_get(&path).await?;
        let entity: Entity = serde_json::from_str(&body)?;
        Ok(entity)
    }

    /// Call a Home Assistant service (e.g., `light.turn_on`).
    pub async fn call_service(
        &self,
        domain: &str,
        service: &str,
        data: &serde_json::Value,
    ) -> Result<()> {
        let path = format!("/api/services/{}/{}", domain, service);
        let body = serde_json::to_string(data)?;
        self.http_post(&path, &body).await?;
        Ok(())
    }

    /// Fuzzy match an entity by name.
    ///
    /// Fuzzy match: compare user query against entity
    /// friendly names using normalized substring matching + word overlap.
    /// Returns the best match above a minimum score threshold.
    pub async fn find_entity(&self, query: &str) -> Result<Option<Entity>> {
        let entities = self.get_states().await?;
        let query_lower = query.to_lowercase();
        let query_words: Vec<&str> = query_lower.split_whitespace().collect();

        let mut best: Option<(f32, Entity)> = None;

        for entity in entities {
            let name = entity.friendly_name().to_lowercase();
            let score = fuzzy_score(&query_words, &query_lower, &name);

            if score > 0.4 && best.as_ref().is_none_or(|(s, _)| score > *s) {
                best = Some((score, entity));
            }
        }

        Ok(best.map(|(_, e)| e))
    }

    async fn http_get(&self, path: &str) -> Result<String> {
        let addr = format!("{}:{}", self.host, self.port);
        let stream =
            tokio::time::timeout(std::time::Duration::from_secs(5), TcpStream::connect(&addr))
                .await??;

        let (reader, mut writer) = stream.into_split();

        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n",
            path, addr, self.token
        );
        writer.write_all(request.as_bytes()).await?;

        read_http_body(reader).await
    }

    async fn http_post(&self, path: &str, body: &str) -> Result<String> {
        let addr = format!("{}:{}", self.host, self.port);
        let stream =
            tokio::time::timeout(std::time::Duration::from_secs(5), TcpStream::connect(&addr))
                .await??;

        let (reader, mut writer) = stream.into_split();

        let request = format!(
            "POST {} HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            path,
            addr,
            self.token,
            body.len(),
            body
        );
        writer.write_all(request.as_bytes()).await?;

        read_http_body(reader).await
    }
}

async fn read_http_body(reader: tokio::net::tcp::OwnedReadHalf) -> Result<String> {
    let mut buf_reader = BufReader::new(reader);
    let mut content_length: usize = 0;

    // Read headers.
    loop {
        let mut line = String::new();
        buf_reader.read_line(&mut line).await?;
        if line.trim().is_empty() {
            break;
        }
        if let Some(val) = line.to_lowercase().strip_prefix("content-length: ") {
            content_length = val.trim().parse().unwrap_or(0);
        }
    }

    if content_length > 0 {
        let mut buf = vec![0u8; content_length];
        tokio::io::AsyncReadExt::read_exact(&mut buf_reader, &mut buf).await?;
        Ok(String::from_utf8_lossy(&buf).to_string())
    } else {
        let mut body = String::new();
        tokio::io::AsyncReadExt::read_to_string(&mut buf_reader, &mut body).await?;
        Ok(body)
    }
}

/// Fuzzy matching score between a user query and an entity name.
///
/// Combines:
/// 1. Exact substring match (highest weight)
/// 2. Word overlap ratio
/// 3. Prefix matching bonus
fn fuzzy_score(query_words: &[&str], query_lower: &str, name_lower: &str) -> f32 {
    let mut score: f32 = 0.0;

    // Exact substring.
    if name_lower.contains(query_lower) {
        score += 0.8;
    }

    // Word overlap.
    let name_words: Vec<&str> = name_lower.split_whitespace().collect();
    if !query_words.is_empty() && !name_words.is_empty() {
        let matching = query_words
            .iter()
            .filter(|qw| {
                name_words
                    .iter()
                    .any(|nw| nw.contains(*qw) || qw.contains(nw))
            })
            .count();
        let overlap = matching as f32 / query_words.len().max(1) as f32;
        score += overlap * 0.5;
    }

    // Prefix bonus — "living room" matches "living room light".
    if name_lower.starts_with(query_lower) {
        score += 0.2;
    }

    score.min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_exact_match() {
        let query = "living room light";
        let words: Vec<&str> = query.split_whitespace().collect();
        let score = fuzzy_score(&words, query, "living room light");
        assert!(score > 0.9, "exact match should score high: {}", score);
    }

    #[test]
    fn fuzzy_partial_match() {
        let query = "living room";
        let words: Vec<&str> = query.split_whitespace().collect();
        let score = fuzzy_score(&words, query, "living room ceiling light");
        assert!(score > 0.6, "partial should score well: {}", score);
    }

    #[test]
    fn fuzzy_no_match() {
        let query = "garage door";
        let words: Vec<&str> = query.split_whitespace().collect();
        let score = fuzzy_score(&words, query, "kitchen light");
        assert!(score < 0.4, "no overlap should score low: {}", score);
    }

    #[test]
    fn fuzzy_single_word() {
        let query = "bedroom";
        let words: Vec<&str> = query.split_whitespace().collect();
        let score = fuzzy_score(&words, query, "bedroom lamp");
        assert!(score > 0.5, "single word match: {}", score);
    }
}
