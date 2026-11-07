//! OpenCode Go plan: `GET opencode.ai/zen/go/v1/usage` with the key the OpenCode CLI uses
//! (Console key in `opencode.db`, `opencode-go` key in `auth.json`, or `OPENCODE_API_KEY`).

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

fn console_key(dir: &std::path::Path) -> Option<String> {
    let db = dir.join("opencode.db");
    if !db.is_file() {
        return None;
    }
    let conn = rusqlite::Connection::open_with_flags(
        db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .ok()?;
    conn.busy_timeout(std::time::Duration::from_millis(150))
        .ok()?;
    let raw: String = conn
        .query_row(
            "SELECT value FROM credential WHERE integration_id = 'opencode' AND active IS NOT 0 \
             ORDER BY active DESC, time_created DESC, id DESC LIMIT 1",
            [],
            |r| r.get(0),
        )
        .ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    if v.get("type").and_then(Value::as_str) != Some("key") {
        return None;
    }
    if let Some(server) = v.pointer("/metadata/server").and_then(Value::as_str)
        && !server
            .trim_end_matches('/')
            .ends_with("opencode.ai/console")
    {
        return None;
    }
    v.get("key")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
}

fn keys() -> Vec<String> {
    let dir = data_dir();
    let mut keys = Vec::new();
    if let Some(k) = console_key(&dir) {
        keys.push(k);
    }
    let go = read_json(&dir.join("auth.json")).ok().and_then(|a| {
        a.pointer("/opencode-go/key")
            .and_then(Value::as_str)
            .map(str::to_string)
    });
    if let Some(k) = go.or_else(|| std::env::var("OPENCODE_API_KEY").ok()) {
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
