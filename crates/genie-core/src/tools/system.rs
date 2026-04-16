use anyhow::Result;
use genie_common::tegrastats;

/// Get system status: memory, uptime, governor mode.
pub async fn system_info() -> Result<String> {
    let mut info = Vec::new();

    // Memory.
    if let Ok(avail) = tegrastats::mem_available_mb() {
        info.push(format!("Memory available: {} MB", avail));
    }

    // Uptime.
    if let Ok(contents) = tokio::fs::read_to_string("/proc/uptime").await
        && let Some(secs_str) = contents.split_whitespace().next()
        && let Ok(secs) = secs_str.parse::<f64>()
    {
        let hours = (secs / 3600.0) as u64;
        let mins = ((secs % 3600.0) / 60.0) as u64;
        info.push(format!("Uptime: {}h {}m", hours, mins));
    }

    // Governor mode (try control socket).
    if let Some(status) = query_governor_status().await {
        if let Some(mode) = status.get("mode").and_then(|v| v.as_str()) {
            info.push(format!("Governor mode: {}", mode));
        }
        if let Some(mem) = status.get("mem_available_mb").and_then(|v| v.as_u64()) {
            info.push(format!("Governor reports: {} MB available", mem));
        }
    } else {
        info.push("Governor: not running".to_string());
    }

    // Load average.
    if let Ok(contents) = tokio::fs::read_to_string("/proc/loadavg").await {
        let parts: Vec<&str> = contents.split_whitespace().collect();
        if parts.len() >= 3 {
            info.push(format!("Load: {} {} {}", parts[0], parts[1], parts[2]));
        }
    }

    if info.is_empty() {
        Ok("System info unavailable.".into())
    } else {
        Ok(info.join(". ") + ".")
    }
}

async fn query_governor_status() -> Option<serde_json::Value> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let stream = UnixStream::connect("/run/geniepod/governor.sock")
        .await
        .ok()?;
    let (reader, mut writer) = stream.into_split();

    writer.write_all(b"{\"cmd\":\"status\"}\n").await.ok()?;

    let mut lines = BufReader::new(reader).lines();
    let line = tokio::time::timeout(std::time::Duration::from_secs(2), lines.next_line())
        .await
        .ok()?
        .ok()?;

    line.and_then(|l| serde_json::from_str(&l).ok())
}
