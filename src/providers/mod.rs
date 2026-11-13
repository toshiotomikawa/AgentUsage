//! Quota readers. Each one reads the local sign-in of an agent CLI and asks that vendor's own
//! endpoint for the remaining quota. Tokens never leave this process except to their own vendor.

mod antigravity;
mod claude;
mod claude_login;
mod codex;
mod cursor;
mod opencode;

use crate::config::Config;
use crate::model::{Group, Provider};
use chrono::{DateTime, TimeZone, Utc};
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

pub struct Fetched {
    pub groups: Vec<Group>,
    /// Account identifier (email local part), when known.
    pub account: Option<String>,
    pub title: Option<String>,
}

pub type FetchFn = Arc<dyn Fn() -> Result<Fetched, String> + Send + Sync>;

/// One account that can be polled.
#[derive(Clone)]
pub struct Source {
    pub key: String,
    pub provider: Provider,
    pub fetch: FetchFn,
    /// Minimum seconds between reads (slow or expensive sources).
    pub min_interval: Option<u64>,
    /// Auto-discovered extra account: hidden instead of shown as "no data" when it cannot be read.
    pub optional: bool,
    /// Slow source: its last good reading is kept on disk and shown at once on the next launch.
    pub remember: bool,
    /// Account label known before any fetch (shown even when the fetch fails).
    pub account: Option<String>,
}

pub fn discover(cfg: &Config) -> Vec<Source> {
    let mut out = Vec::new();
    out.extend(claude::sources(cfg));
    out.push(codex::source());
    out.push(cursor::source());
    out.push(opencode::source());
    if cfg.antigravity {
        out.push(antigravity::source());
    }
    out
}

// ---------- shared helpers ----------

pub(crate) fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .http_status_as_error(false)
        // Tokens go only to the vendor's own HTTPS host: no plain HTTP, no redirects.
        .https_only(true)
        .max_redirects(0)
        .user_agent(concat!("agent-usage/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// Login and settings files are small; anything larger is not one.
pub(crate) const MAX_FILE: u64 = 4 * 1024 * 1024;

pub(crate) fn read_json(path: &Path) -> Result<Value, String> {
    let text = read_small(path).ok_or_else(|| "não conectado".to_string())?;
    serde_json::from_str(&text).map_err(|_| "arquivo de login ilegível".to_string())
}

/// Read a regular file of at most `MAX_FILE` bytes (checked on the open handle).
pub(crate) fn read_small(path: &Path) -> Option<String> {
    use std::io::Read;
    let f = std::fs::File::open(path).ok()?;
    let meta = f.metadata().ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    let mut s = String::new();
    f.take(MAX_FILE + 1).read_to_string(&mut s).ok()?;
    (s.len() as u64 <= MAX_FILE).then_some(s)
}

/// The account has no active subscription: its card is hidden, not shown as "no data".
pub(crate) const INACTIVE: &str = "assinatura inativa";

/// A failure that says nothing about whether the account exists (rate limit, network, server).
pub(crate) fn is_transient(err: &str) -> bool {
    err == status_error(429)
        || err == "sem conexão"
        || err == "resposta inesperada"
        || err == claude_login::BUSY
        || err.starts_with("serviço respondeu 5")
}

pub(crate) fn status_error(code: u16) -> String {
    match code {
        401 | 403 => "login expirado — abra o agente para renovar".into(),
        429 => "limite de consultas — tentando de novo em breve".into(),
        c => format!("serviço respondeu {c}"),
    }
}

pub(crate) fn parse_time(v: &Value) -> Option<DateTime<Utc>> {
    match v {
        Value::String(s) => DateTime::parse_from_rfc3339(s)
            .ok()
            .map(|d| d.with_timezone(&Utc))
            .or_else(|| s.parse::<i64>().ok().and_then(from_epoch)),
        Value::Number(n) => n.as_i64().and_then(from_epoch),
        _ => None,
    }
}

/// Accepts seconds or milliseconds since the epoch.
pub(crate) fn from_epoch(n: i64) -> Option<DateTime<Utc>> {
    if n > 100_000_000_000 {
        Utc.timestamp_millis_opt(n).single()
    } else {
        Utc.timestamp_opt(n, 0).single()
    }
}

pub(crate) fn num(v: &Value) -> Option<f64> {
    let n = match v {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }?;
    n.is_finite().then_some(n)
}

/// Label for a window from its length in minutes.
pub(crate) fn label_for_minutes(mins: u64) -> &'static str {
    match mins {
        0..=59 => "Curta",
        60..=719 => "5 horas",
        720..=2879 => "Diária",
        2880..=14399 => "Semanal",
        _ => "Mensal",
    }
}
