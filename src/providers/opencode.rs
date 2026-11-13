//! OpenCode Go plan: `GET opencode.ai/zen/go/v1/usage` with the key the OpenCode CLI uses
//! (keys or Console login in `opencode.db`, keys in `auth.json`, or `OPENCODE_API_KEY`).

use super::{Fetched, Source, agent, parse_time, read_json, status_error};
use crate::model::{Group, Provider, Window};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

const URL: &str = "https://opencode.ai/zen/go/v1/usage";

pub fn source() -> Source {
    Source {
        key: "opencode".into(),
        provider: Provider::OpenCode,
        fetch: Arc::new(fetch),
        min_interval: None,
        optional: false,
        remember: false,
        account: None,
    }
}

fn data_dir() -> PathBuf {
    if let Some(x) = std::env::var_os("XDG_DATA_HOME") {
        return PathBuf::from(x).join("opencode");
    }
    dirs::home_dir()
        .unwrap_or_default()
        .join(".local")
        .join("share")
        .join("opencode")
}

/// Keys stored by the OpenCode CLI: API keys for `opencode-go` or `opencode`, and the access
/// token of a Console login (OpenCode now stores that login as OAuth). Active ones first.
fn db_keys(dir: &std::path::Path) -> Vec<String> {
    let db = dir.join("opencode.db");
    if !db.is_file() {
        return Vec::new();
    }
    let Ok(conn) = rusqlite::Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Vec::new();
    };
    let _ = conn.busy_timeout(std::time::Duration::from_millis(150));
    let Ok(mut stmt) = conn.prepare(
        "SELECT value FROM credential WHERE integration_id IN ('opencode-go', 'opencode')          ORDER BY COALESCE(active, 0) DESC, integration_id = 'opencode-go' DESC,          time_created DESC, id DESC",
    ) else {
        return Vec::new();
    };
    let now_ms = chrono::Utc::now().timestamp_millis();
    let rows = stmt.query_map([], |r| r.get::<_, String>(0));
    let Ok(rows) = rows else { return Vec::new() };
    rows.flatten()
        .filter_map(|raw| serde_json::from_str::<Value>(&raw).ok())
        .filter_map(|v| {
            if let Some(server) = v.pointer("/metadata/server").and_then(Value::as_str)
                && !server
                    .trim_end_matches('/')
                    .ends_with("opencode.ai/console")
            {
                return None;
            }
            match v.get("type").and_then(Value::as_str) {
                Some("key") => v.get("key").and_then(Value::as_str).map(str::to_string),
                Some("oauth") => {
                    let expired = v
                        .get("expires")
                        .and_then(Value::as_i64)
                        .is_some_and(|e| e <= now_ms);
                    if expired {
                        None
                    } else {
                        v.get("access").and_then(Value::as_str).map(str::to_string)
                    }
                }
                _ => None,
            }
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn keys() -> Vec<String> {
    let dir = data_dir();
    let mut keys = db_keys(&dir);
    let auth = read_json(&dir.join("auth.json")).ok();
    let from_auth = ["/opencode-go/key", "/opencode/key"]
        .into_iter()
        .filter_map(|p| {
            auth.as_ref()
                .and_then(|a| a.pointer(p))
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    for k in from_auth.chain(std::env::var("OPENCODE_API_KEY").ok()) {
        let k = k.trim().to_string();
        if !k.is_empty() && !keys.contains(&k) {
            keys.push(k);
        }
    }
    keys
}

fn fetch() -> Result<Fetched, String> {
    let keys = keys();
    if keys.is_empty() {
        return Err("não conectado".into());
    }
    let mut last = String::from("sem assinatura Go");
    for key in keys {
        let mut resp = agent()
            .get(URL)
            .header("Authorization", &format!("Bearer {key}"))
            .call()
            .map_err(|_| "sem conexão".to_string())?;
        let code = resp.status().as_u16();
        if code == 401 || code == 403 {
            last = "sem assinatura Go".into();
            continue;
        }
        if !resp.status().is_success() {
            return Err(status_error(code));
        }
        let body: Value = resp
            .body_mut()
            .read_json()
            .map_err(|_| "resposta inesperada".to_string())?;
        let usage = body.get("usage").ok_or("sem limites informados")?;
        let mut windows = Vec::new();
        for (key, label, mins) in [
            ("rolling", "5 horas", 300),
            ("weekly", "Semanal", 10_080),
            ("monthly", "Mensal", 43_200),
        ] {
            let Some(w) = usage.get(key) else { continue };
            let Some(used) = w.get("percent").and_then(Value::as_f64) else {
                continue;
            };
            windows.push(
                Window::new(label, 1.0 - used as f32 / 100.0)
                    .resets(w.get("resetsAt").and_then(parse_time))
                    .length_mins(mins),
            );
        }
        if windows.is_empty() {
            return Err("sem limites informados".into());
        }
        return Ok(Fetched {
            groups: vec![Group {
                name: String::new(),
                windows,
            }],
            account: None,
            title: None,
        });
    }
    Err(last)
}
