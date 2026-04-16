use genie_common::config::Config;
use genie_common::tegrastats;

use crate::http::Response;

/// GET /api/status — current mode, memory, uptime.
pub async fn get_status(_config: &Config) -> Response {
    // Read governor status via its Unix socket.
    let governor_status = query_governor(r#"{"cmd":"status"}"#).await;

    // Augment with live memory reading.
    let mem_avail = tegrastats::mem_available_mb().unwrap_or(0);

    let body = if let Some(mut status) = governor_status {
        // Merge live mem_available into the governor's response.
        if let Some(obj) = status.as_object_mut() {
            obj.insert(
                "mem_available_mb_live".into(),
                serde_json::Value::from(mem_avail),
            );
        }
        serde_json::to_string(&status).unwrap_or_default()
    } else {
        // Governor not running — return basic info.
        serde_json::json!({
            "mode": "unknown",
            "mem_available_mb": mem_avail,
            "governor": "offline"
        })
        .to_string()
    };

    Response {
        status: 200,
        content_type: "application/json",
        body,
    }
}

/// GET /api/tegrastats — recent history from governor's SQLite.
pub async fn get_tegrastats(config: &Config) -> Response {
    let db_path = config.data_dir.join("governor.db");

    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let conn =
            rusqlite::Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|e| e.to_string())?;

        let mut stmt = conn
            .prepare(
                "SELECT ts_ms, ram_used_mb, ram_total_mb, gpu_freq_pct, gpu_temp_c, cpu_temp_c, power_mw
                 FROM tegrastats
                 ORDER BY ts_ms DESC
                 LIMIT 720",
            )
            .map_err(|e| e.to_string())?;

        let rows: Vec<serde_json::Value> = stmt
            .query_map([], |row| {
                Ok(serde_json::json!({
                    "ts": row.get::<_, i64>(0)?,
                    "ram_used": row.get::<_, i64>(1)?,
                    "ram_total": row.get::<_, i64>(2)?,
                    "gpu_pct": row.get::<_, i64>(3)?,
                    "gpu_c": row.get::<_, Option<f64>>(4)?,
                    "cpu_c": row.get::<_, Option<f64>>(5)?,
                    "power_mw": row.get::<_, Option<i64>>(6)?,
                }))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        serde_json::to_string(&rows).map_err(|e| e.to_string())
    })
    .await;

    match result {
        Ok(Ok(json)) => Response {
            status: 200,
            content_type: "application/json",
            body: json,
        },
        _ => Response {
            status: 200,
            content_type: "application/json",
            body: "[]".into(),
        },
    }
}

/// GET /api/services — health check status from health monitor's SQLite.
pub async fn get_services(config: &Config) -> Response {
    let db_path = config.data_dir.join("health.db");

    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let conn = rusqlite::Connection::open_with_flags(
            &db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .map_err(|e| e.to_string())?;

        // Get the latest health check for each service.
        let mut stmt = conn
            .prepare(
                "SELECT service, healthy, response_ms, error, MAX(ts_ms) as last_check
                 FROM health_log
                 GROUP BY service
                 ORDER BY service",
            )
            .map_err(|e| e.to_string())?;

        let rows: Vec<serde_json::Value> = stmt
            .query_map([], |row| {
                Ok(serde_json::json!({
                    "service": row.get::<_, String>(0)?,
                    "healthy": row.get::<_, i32>(1)? == 1,
                    "response_ms": row.get::<_, i64>(2)?,
                    "error": row.get::<_, Option<String>>(3)?,
                    "last_check": row.get::<_, i64>(4)?,
                }))
            })
            .map_err(|e| e.to_string())?
            .filter_map(|r| r.ok())
            .collect();

        serde_json::to_string(&rows).map_err(|e| e.to_string())
    })
    .await;

    match result {
        Ok(Ok(json)) => Response {
            status: 200,
            content_type: "application/json",
            body: json,
        },
        _ => Response {
            status: 200,
            content_type: "application/json",
            body: "[]".into(),
        },
    }
}

/// POST /api/mode — send mode change command to governor.
pub async fn post_mode(body: Option<&str>) -> Response {
    let Some(body) = body else {
        return Response {
            status: 400,
            content_type: "application/json",
            body: r#"{"error":"missing body"}"#.into(),
        };
    };

    // Forward the command to the governor via its control socket.
    let result = query_governor(body).await;

    match result {
        Some(val) => Response {
            status: 200,
            content_type: "application/json",
            body: val.to_string(),
        },
        None => Response {
            status: 500,
            content_type: "application/json",
            body: r#"{"error":"governor unreachable"}"#.into(),
        },
    }
}

/// Query the governor via its Unix control socket.
async fn query_governor(json_cmd: &str) -> Option<serde_json::Value> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::UnixStream;

    let stream = UnixStream::connect("/run/geniepod/governor.sock")
        .await
        .ok()?;
    let (reader, mut writer) = stream.into_split();

    writer.write_all(json_cmd.as_bytes()).await.ok()?;
    writer.write_all(b"\n").await.ok()?;

    let mut lines = BufReader::new(reader).lines();
    let line = tokio::time::timeout(std::time::Duration::from_secs(2), lines.next_line())
        .await
        .ok()?
        .ok()?;

    line.and_then(|l| serde_json::from_str(&l).ok())
}

/// GET / — serve the dashboard HTML.
pub fn serve_dashboard() -> Response {
    Response {
        status: 200,
        content_type: "text/html; charset=utf-8",
        body: include_str!("../../dashboard/index.html").into(),
    }
}

/// GET /dashboard.js — serve the dashboard JavaScript.
pub fn serve_dashboard_js() -> Response {
    Response {
        status: 200,
        content_type: "application/javascript; charset=utf-8",
        body: include_str!("../../dashboard/dashboard.js").into(),
    }
}
