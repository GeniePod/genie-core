use genie_common::config::Config;
use genie_common::tegrastats;
use rusqlite::params;
use serde::{Deserialize, Serialize};

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

#[derive(Debug, Serialize)]
struct DashboardMemoryEntry {
    id: i64,
    kind: String,
    content: String,
    created_ms: i64,
    accessed_ms: i64,
    recall_count: i64,
    promoted: bool,
    scope: String,
    sensitivity: String,
    spoken_policy: String,
    display_order: i64,
}

#[derive(Debug, Deserialize)]
struct MemoryUpdateRequest {
    id: i64,
    content: String,
    kind: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MemoryDeleteRequest {
    id: i64,
}

#[derive(Debug, Deserialize)]
struct MemoryReorderRequest {
    ids: Vec<i64>,
}

pub async fn get_actuation_pending(_config: &Config) -> Response {
    match proxy_core_json("GET", "/api/actuation/pending", None).await {
        Ok(body) => Response {
            status: 200,
            content_type: "application/json",
            body,
        },
        Err(e) => Response {
            status: 502,
            content_type: "application/json",
            body: serde_json::json!({ "error": e }).to_string(),
        },
    }
}

pub async fn get_actuation_audit(config: &Config) -> Response {
    let path = config.data_dir.join("safety/actuation-audit.jsonl");
    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        if !path.exists() {
            return Ok("[]".into());
        }
        let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let items = text
            .lines()
            .rev()
            .take(50)
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .collect::<Vec<_>>();
        serde_json::to_string(&items).map_err(|e| e.to_string())
    })
    .await;

    match result {
        Ok(Ok(body)) => Response {
            status: 200,
            content_type: "application/json",
            body,
        },
        Ok(Err(e)) => Response {
            status: 500,
            content_type: "application/json",
            body: serde_json::json!({ "error": e }).to_string(),
        },
        Err(e) => Response {
            status: 500,
            content_type: "application/json",
            body: serde_json::json!({ "error": e.to_string() }).to_string(),
        },
    }
}

pub async fn post_actuation_confirm(_config: &Config, body: Option<&str>) -> Response {
    let Some(body) = body else {
        return Response {
            status: 400,
            content_type: "application/json",
            body: r#"{"error":"missing body"}"#.into(),
        };
    };

    match proxy_core_json("POST", "/api/actuation/confirm", Some(body)).await {
        Ok(payload) => Response {
            status: 200,
            content_type: "application/json",
            body: payload,
        },
        Err(e) => Response {
            status: 502,
            content_type: "application/json",
            body: serde_json::json!({ "error": e }).to_string(),
        },
    }
}

pub async fn get_memories(config: &Config) -> Response {
    let db_path = config.data_dir.join("memory.db");
    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
        ensure_memory_dashboard_schema(&conn).map_err(|e| e.to_string())?;
        let mut stmt = conn
            .prepare(
                "SELECT id, kind, content, created_ms, accessed_ms, recall_count, promoted,
                        scope, sensitivity, spoken_policy, display_order
                 FROM memories
                 ORDER BY display_order ASC, accessed_ms DESC, id DESC
                 LIMIT 500",
            )
            .map_err(|e| e.to_string())?;
        let entries = stmt
            .query_map([], |row| {
                Ok(DashboardMemoryEntry {
                    id: row.get(0)?,
                    kind: row.get(1)?,
                    content: row.get(2)?,
                    created_ms: row.get(3)?,
                    accessed_ms: row.get(4)?,
                    recall_count: row.get(5)?,
                    promoted: row.get::<_, i64>(6)? != 0,
                    scope: row.get(7)?,
                    sensitivity: row.get(8)?,
                    spoken_policy: row.get(9)?,
                    display_order: row.get(10)?,
                })
            })
            .map_err(|e| e.to_string())?
            .filter_map(|row| row.ok())
            .collect::<Vec<_>>();
        serde_json::to_string(&entries).map_err(|e| e.to_string())
    })
    .await;

    match result {
        Ok(Ok(body)) => Response {
            status: 200,
            content_type: "application/json",
            body,
        },
        Ok(Err(e)) => Response {
            status: 500,
            content_type: "application/json",
            body: serde_json::json!({ "error": e }).to_string(),
        },
        Err(e) => Response {
            status: 500,
            content_type: "application/json",
            body: serde_json::json!({ "error": e.to_string() }).to_string(),
        },
    }
}

pub async fn post_memory_update(config: &Config, body: Option<&str>) -> Response {
    let Some(body) = body else {
        return Response {
            status: 400,
            content_type: "application/json",
            body: r#"{"error":"missing body"}"#.into(),
        };
    };
    let req: MemoryUpdateRequest = match serde_json::from_str(body) {
        Ok(req) => req,
        Err(e) => {
            return Response {
                status: 400,
                content_type: "application/json",
                body: serde_json::json!({ "error": e.to_string() }).to_string(),
            };
        }
    };

    let db_path = config.data_dir.join("memory.db");
    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
        ensure_memory_dashboard_schema(&conn).map_err(|e| e.to_string())?;
        let updated = if let Some(kind) = req.kind {
            conn.execute(
                "UPDATE memories SET content = ?1, kind = ?2 WHERE id = ?3",
                params![req.content.trim(), kind.trim(), req.id],
            )
            .map_err(|e| e.to_string())?
        } else {
            conn.execute(
                "UPDATE memories SET content = ?1 WHERE id = ?2",
                params![req.content.trim(), req.id],
            )
            .map_err(|e| e.to_string())?
        };
        serde_json::json!({ "ok": updated > 0 })
            .to_string()
            .pipe(Ok)
    })
    .await;

    match result {
        Ok(Ok(body)) => Response {
            status: 200,
            content_type: "application/json",
            body,
        },
        Ok(Err(e)) => Response {
            status: 500,
            content_type: "application/json",
            body: serde_json::json!({ "error": e }).to_string(),
        },
        Err(e) => Response {
            status: 500,
            content_type: "application/json",
            body: serde_json::json!({ "error": e.to_string() }).to_string(),
        },
    }
}

pub async fn post_memory_delete(config: &Config, body: Option<&str>) -> Response {
    let Some(body) = body else {
        return Response {
            status: 400,
            content_type: "application/json",
            body: r#"{"error":"missing body"}"#.into(),
        };
    };
    let req: MemoryDeleteRequest = match serde_json::from_str(body) {
        Ok(req) => req,
        Err(e) => {
            return Response {
                status: 400,
                content_type: "application/json",
                body: serde_json::json!({ "error": e.to_string() }).to_string(),
            };
        }
    };

    let db_path = config.data_dir.join("memory.db");
    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
        let deleted = conn
            .execute("DELETE FROM memories WHERE id = ?1", params![req.id])
            .map_err(|e| e.to_string())?;
        serde_json::json!({ "ok": deleted > 0 })
            .to_string()
            .pipe(Ok)
    })
    .await;

    match result {
        Ok(Ok(body)) => Response {
            status: 200,
            content_type: "application/json",
            body,
        },
        Ok(Err(e)) => Response {
            status: 500,
            content_type: "application/json",
            body: serde_json::json!({ "error": e }).to_string(),
        },
        Err(e) => Response {
            status: 500,
            content_type: "application/json",
            body: serde_json::json!({ "error": e.to_string() }).to_string(),
        },
    }
}

pub async fn post_memory_reorder(config: &Config, body: Option<&str>) -> Response {
    let Some(body) = body else {
        return Response {
            status: 400,
            content_type: "application/json",
            body: r#"{"error":"missing body"}"#.into(),
        };
    };
    let req: MemoryReorderRequest = match serde_json::from_str(body) {
        Ok(req) => req,
        Err(e) => {
            return Response {
                status: 400,
                content_type: "application/json",
                body: serde_json::json!({ "error": e.to_string() }).to_string(),
            };
        }
    };

    let db_path = config.data_dir.join("memory.db");
    let result = tokio::task::spawn_blocking(move || -> Result<String, String> {
        let mut conn = rusqlite::Connection::open(&db_path).map_err(|e| e.to_string())?;
        ensure_memory_dashboard_schema(&conn).map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        for (idx, id) in req.ids.iter().enumerate() {
            tx.execute(
                "UPDATE memories SET display_order = ?1 WHERE id = ?2",
                params![idx as i64, id],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
        serde_json::json!({ "ok": true }).to_string().pipe(Ok)
    })
    .await;

    match result {
        Ok(Ok(body)) => Response {
            status: 200,
            content_type: "application/json",
            body,
        },
        Ok(Err(e)) => Response {
            status: 500,
            content_type: "application/json",
            body: serde_json::json!({ "error": e }).to_string(),
        },
        Err(e) => Response {
            status: 500,
            content_type: "application/json",
            body: serde_json::json!({ "error": e.to_string() }).to_string(),
        },
    }
}

fn ensure_memory_dashboard_schema(conn: &rusqlite::Connection) -> rusqlite::Result<()> {
    let _ = conn.execute(
        "ALTER TABLE memories ADD COLUMN display_order INTEGER NOT NULL DEFAULT 2147483647",
        [],
    );
    let _ = conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_memories_display_order ON memories(display_order, accessed_ms DESC)",
        [],
    );
    Ok(())
}

async fn proxy_core_json(method: &str, path: &str, body: Option<&str>) -> Result<String, String> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;

    let mut stream = TcpStream::connect("127.0.0.1:3000")
        .await
        .map_err(|e| e.to_string())?;
    let body_str = body.unwrap_or("");
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body_str.len(),
        body_str
    );
    stream
        .write_all(request.as_bytes())
        .await
        .map_err(|e| e.to_string())?;
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .await
        .map_err(|e| e.to_string())?;
    let raw = String::from_utf8_lossy(&raw);
    let (_, body) = raw
        .split_once("\r\n\r\n")
        .ok_or_else(|| "invalid core response".to_string())?;
    Ok(body.to_string())
}

trait Pipe: Sized {
    fn pipe<T>(self, f: impl FnOnce(Self) -> T) -> T {
        f(self)
    }
}

impl<T> Pipe for T {}
