//! Codex (ChatGPT plan) limits: `GET chatgpt.com/backend-api/wham/usage` with the ChatGPT sign-in
//! Codex CLI keeps in `~/.codex/auth.json` (or `$CODEX_HOME/auth.json`).

use super::{Fetched, Source, agent, from_epoch, label_for_minutes, num, read_json, status_error};
use crate::model::{Group, Provider, Window};
use chrono::Utc;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

const URL: &str = "https://chatgpt.com/backend-api/wham/usage";

pub fn source() -> Source {
    Source {
        key: "codex".into(),
        provider: Provider::Codex,
        fetch: Arc::new(fetch),
        min_interval: None,
        optional: false,
    }
}

fn home() -> PathBuf {
    std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".codex"))
}

fn fetch() -> Result<Fetched, String> {
    let auth = read_json(&home().join("auth.json"))?;
    let token = auth
        .pointer("/tokens/access_token")
        .and_then(Value::as_str)
        .ok_or("sem login do ChatGPT")?;
    let mut req = agent()
        .get(URL)
        .header("Authorization", &format!("Bearer {token}"))
        .header("User-Agent", "codex_cli_rs");
    if let Some(id) = auth.pointer("/tokens/account_id").and_then(Value::as_str) {
        req = req.header("ChatGPT-Account-Id", id);
    }
    let mut resp = req.call().map_err(|_| "sem conexão".to_string())?;
    if !resp.status().is_success() {
        return Err(status_error(resp.status().as_u16()));
    }
    let body: Value = resp
        .body_mut()
        .read_json()
        .map_err(|_| "resposta inesperada".to_string())?;

    let limits = body.get("rate_limit").ok_or("sem limites informados")?;
    let mut windows = Vec::new();
    for key in ["primary_window", "secondary_window"] {
        let Some(w) = limits.get(key).filter(|w| w.is_object()) else {
            continue;
        };
        let Some(used) = w.get("used_percent").and_then(num) else {
            continue;
        };
        let secs = w.get("limit_window_seconds").and_then(num).unwrap_or(0.0) as u64;
        let reset = w
            .get("reset_at")
            .and_then(num)
            .and_then(|n| from_epoch(n as i64))
            .or_else(|| {
                let after = w.get("reset_after_seconds").and_then(num)?;
                Some(Utc::now() + chrono::Duration::seconds(after as i64))
            });
        let mut win =
            Window::new(label_for_minutes(secs / 60), 1.0 - used as f32 / 100.0).resets(reset);
        if secs > 0 {
            win = win.length_mins(secs / 60);
        }
        windows.push(win);
    }
    if windows.is_empty() {
        return Err("sem limites informados".into());
    }
    Ok(Fetched {
        groups: vec![Group {
            name: String::new(),
            windows,
        }],
        account: None,
        title: None,
    })
}
