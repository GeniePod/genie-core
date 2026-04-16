use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;

/// Home Assistant REST API client.
///
/// Uses the local or LAN Home Assistant HTTP API for:
/// - state reads
/// - service calls
/// - lightweight template rendering for area discovery
#[derive(Debug, Clone)]
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

#[derive(Debug)]
struct HttpResponse {
    status_code: u16,
    body: String,
}

impl HaClient {
    pub fn new(host: &str, port: u16, token: &str) -> Self {
        Self {
            host: host.to_string(),
            port,
            token: token.to_string(),
        }
    }

    /// Build from a configured Home Assistant HTTP URL such as
    /// `http://127.0.0.1:8123/api/` or `http://homeassistant.local:8123/`.
    pub fn from_url(url: &str, token: &str) -> Result<Self> {
        let (host, port) = parse_http_url(url)?;
        Ok(Self::new(&host, port, token))
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    /// Simple connectivity check.
    pub async fn test_connection(&self) -> Result<()> {
        self.http_get("/api/").await.map(|_| ())
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
    ) -> Result<Vec<Entity>> {
        let path = format!("/api/services/{}/{}", domain, service);
        let body = serde_json::to_string(data)?;
        let body = self.http_post_json(&path, &body).await?;

        if body.trim().is_empty() {
            return Ok(Vec::new());
        }

        serde_json::from_str(&body).or_else(|_| Ok(Vec::new()))
    }

    /// Render a Home Assistant template and return its plain-text output.
    pub async fn render_template(&self, template: &str) -> Result<String> {
        let body = serde_json::json!({ "template": template }).to_string();
        self.http_post_json("/api/template", &body).await
    }

    /// Render a template expected to serialize JSON and decode it to `T`.
    pub async fn render_template_json<T: DeserializeOwned>(&self, template: &str) -> Result<T> {
        let body = self.render_template(template).await?;
        serde_json::from_str(body.trim())
            .with_context(|| format!("failed to decode Home Assistant template output: {}", body))
    }

    async fn http_get(&self, path: &str) -> Result<String> {
        let response = self.http_request("GET", path, None, None).await?;
        Ok(response.body)
    }

    async fn http_post_json(&self, path: &str, body: &str) -> Result<String> {
        let response = self
            .http_request("POST", path, Some("application/json"), Some(body))
            .await?;
        Ok(response.body)
    }

    async fn http_request(
        &self,
        method: &str,
        path: &str,
        content_type: Option<&str>,
        body: Option<&str>,
    ) -> Result<HttpResponse> {
        let addr = format!("{}:{}", self.host, self.port);
        let stream =
            tokio::time::timeout(std::time::Duration::from_secs(5), TcpStream::connect(&addr))
                .await??;

        let (reader, mut writer) = stream.into_split();

        let request = if let Some(body) = body {
            format!(
                "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Type: {content_type}\r\nContent-Length: {content_length}\r\nConnection: close\r\n\r\n{body}",
                method = method,
                path = path,
                addr = addr,
                token = self.token,
                content_type = content_type.unwrap_or("application/json"),
                content_length = body.len(),
                body = body
            )
        } else {
            format!(
                "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n",
                method = method,
                path = path,
                addr = addr,
                token = self.token
            )
        };

        writer.write_all(request.as_bytes()).await?;
        let response = read_http_response(reader).await?;

        if !(200..300).contains(&response.status_code) {
            let body = response.body.trim().replace('\n', " ");
            anyhow::bail!(
                "Home Assistant HTTP {} for {} {}{}",
                response.status_code,
                method,
                path,
                if body.is_empty() {
                    String::new()
                } else {
                    format!(": {}", body)
                }
            );
        }

        Ok(response)
    }
}

async fn read_http_response(reader: tokio::net::tcp::OwnedReadHalf) -> Result<HttpResponse> {
    let mut buf_reader = BufReader::new(reader);

    let mut status_line = String::new();
    buf_reader.read_line(&mut status_line).await?;
    let status_code = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| anyhow::anyhow!("invalid HTTP response status line: {}", status_line))?;

    let mut content_length: Option<usize> = None;
    let mut chunked = false;

    loop {
        let mut line = String::new();
        buf_reader.read_line(&mut line).await?;
        if line.trim().is_empty() {
            break;
        }

        let lower = line.to_lowercase();
        if let Some(val) = lower.strip_prefix("content-length:") {
            content_length = val.trim().parse().ok();
        }
        if let Some(val) = lower.strip_prefix("transfer-encoding:")
            && val.contains("chunked")
        {
            chunked = true;
        }
    }

    let body = if chunked {
        read_chunked_body(&mut buf_reader).await?
    } else if let Some(content_length) = content_length {
        let mut buf = vec![0u8; content_length];
        buf_reader.read_exact(&mut buf).await?;
        String::from_utf8_lossy(&buf).to_string()
    } else {
        let mut body = String::new();
        buf_reader.read_to_string(&mut body).await?;
        body
    };

    Ok(HttpResponse { status_code, body })
}

async fn read_chunked_body<R: AsyncBufReadExt + Unpin>(reader: &mut R) -> Result<String> {
    let mut body = Vec::new();

    loop {
        let mut size_line = String::new();
        reader.read_line(&mut size_line).await?;
        let size_hex = size_line.trim();
        let size = usize::from_str_radix(size_hex, 16)
            .with_context(|| format!("invalid chunk size: {}", size_hex))?;

        if size == 0 {
            let mut trailing = String::new();
            reader.read_line(&mut trailing).await?;
            break;
        }

        let mut chunk = vec![0u8; size];
        tokio::io::AsyncReadExt::read_exact(reader, &mut chunk).await?;
        body.extend_from_slice(&chunk);

        let mut crlf = [0u8; 2];
        tokio::io::AsyncReadExt::read_exact(reader, &mut crlf).await?;
    }

    Ok(String::from_utf8_lossy(&body).to_string())
}

fn parse_http_url(url: &str) -> Result<(String, u16)> {
    let rest = url.strip_prefix("http://").ok_or_else(|| {
        anyhow::anyhow!(
            "unsupported Home Assistant URL '{}': only http:// is supported",
            url
        )
    })?;

    let authority = rest
        .split('/')
        .next()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("invalid Home Assistant URL '{}'", url))?;

    if authority.starts_with('[') {
        let end = authority
            .find(']')
            .ok_or_else(|| anyhow::anyhow!("invalid IPv6 Home Assistant URL '{}'", url))?;
        let host = authority[1..end].to_string();
        let port = authority[end + 1..]
            .strip_prefix(':')
            .and_then(|p| p.parse::<u16>().ok())
            .unwrap_or(8123);
        return Ok((host, port));
    }

    if let Some((host, port)) = authority.rsplit_once(':')
        && let Ok(port) = port.parse::<u16>()
    {
        return Ok((host.to_string(), port));
    }

    Ok((authority.to_string(), 8123))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_http_url_with_api_path() {
        let (host, port) = parse_http_url("http://127.0.0.1:8123/api/").unwrap();
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 8123);
    }

    #[test]
    fn parse_http_url_defaults_port() {
        let (host, port) = parse_http_url("http://homeassistant.local/").unwrap();
        assert_eq!(host, "homeassistant.local");
        assert_eq!(port, 8123);
    }

    #[test]
    fn parse_http_url_rejects_https() {
        let err = parse_http_url("https://example.com")
            .unwrap_err()
            .to_string();
        assert!(err.contains("only http:// is supported"));
    }
}
