//! Claude Code subscription limits: `GET api.anthropic.com/api/oauth/usage` with the OAuth token
//! Claude Code keeps in `<config dir>/.credentials.json`. One source per config folder (account).

use super::{Fetched, Source, agent, parse_time, read_json, status_error};
use crate::config::Config;
use crate::model::{Group, Provider, Window, email_local_part};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const URL: &str = "https://api.anthropic.com/api/oauth/usage";
const EXPIRED: &str = "login expirado — abra o Claude Code nesta conta";
const MIN_INTERVAL_SECS: u64 = 300;
/// Longest wait after repeated rate-limit answers.
const MAX_BACKOFF: Duration = Duration::from_secs(30 * 60);

/// Pause after a rate-limit answer: no request goes out before `until`, and each further 429
/// doubles the pause, so the limit gets the quiet time it needs to reset.
#[derive(Default)]
struct Backoff {
    until: Option<Instant>,
    wait: Duration,
}

pub fn sources(cfg: &Config) -> Vec<Source> {
    // (folder, optional): folders found by name only are optional and hide when unreadable.
    let mut dirs: Vec<(PathBuf, bool)> = Vec::new();
    let mut add = |p: PathBuf, optional: bool| {
        let canon = p.canonicalize().unwrap_or(p);
        if canon.join(".credentials.json").is_file() && !dirs.iter().any(|(d, _)| d == &canon) {
            dirs.push((canon, optional));
        }
    };
    if let Some(env) = std::env::var_os("CLAUDE_CONFIG_DIR") {
        add(PathBuf::from(env), false);
    }
    for d in &cfg.claude_dirs {
        add(d.clone(), false);
    }
    if let Some(home) = dirs::home_dir() {
        add(home.join(".claude"), false);
        if let Ok(entries) = std::fs::read_dir(&home) {
            let mut extra: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.is_dir()
                        && p.file_name()
                            .and_then(|n| n.to_str())
                            .is_some_and(|n| n.starts_with(".claude-") || n.starts_with(".claude_"))
                })
                .collect();
            extra.sort();
            for p in extra {
                add(p, true);
            }
        }
    }
    if dirs.is_empty() {
        // Still show the card, explaining it is not connected.
        dirs.push((dirs::home_dir().unwrap_or_default().join(".claude"), false));
    }

    dirs.into_iter()
        .enumerate()
        .map(|(i, (dir, optional))| {
            // Until a read succeeds the account behind the token is unknown (the email files
            // follow whichever login ran last), so name the folder instead.
            let account = dir
                .file_name()
                .map(|n| format!("~/{}", n.to_string_lossy()));
            let backoff = Arc::new(Mutex::new(Backoff::default()));
            Source {
                key: format!("claude-{i}"),
                provider: Provider::Claude,
                fetch: Arc::new(move || fetch(&dir, &backoff)),
                // The usage endpoint rate-limits frequent callers; Claude Code itself reads it
                // rarely. Its numbers move slowly, so a few minutes between reads loses nothing.
                min_interval: Some(MIN_INTERVAL_SECS),
                optional,
                account,
            }
        })
        .collect()
}

fn account_email(dir: &Path) -> Option<String> {
    // Claude Code keeps the account in `.claude.json`: inside the config folder when
    // CLAUDE_CONFIG_DIR is set, next to it (in the home folder) for the default `~/.claude`.
    let candidates = [dir.join(".claude.json"), dir.parent()?.join(".claude.json")];
    candidates.iter().find_map(|p| {
        let v = read_json(p).ok()?;
        v.pointer("/oauthAccount/emailAddress")?
            .as_str()
            .map(email_local_part)
    })
}

fn fetch(dir: &Path, backoff: &Mutex<Backoff>) -> Result<Fetched, String> {
    let creds = read_json(&dir.join(".credentials.json"))?;
    let oauth = creds
        .get("claudeAiOauth")
        .ok_or("sem login por assinatura")?;
    let token = oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .ok_or("sem login por assinatura")?;
    // Claude Code renews the token only while it runs with this folder; an expired one is
    // refused anyway, so say so instead of asking.
    let expires_ms = oauth.get("expiresAt").and_then(Value::as_i64);
    if expires_ms.is_some_and(|ms| ms <= chrono::Utc::now().timestamp_millis()) {
        return Err(EXPIRED.into());
    }
    if backoff
        .lock()
        .unwrap()
        .until
        .is_some_and(|t| Instant::now() < t)
    {
        return Err(status_error(429));
    }
    let mut resp = agent()
        .get(URL)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .call()
        .map_err(|_| "sem conexão".to_string())?;
    let code = resp.status().as_u16();
    if code == 429 {
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(Duration::from_secs);
        let mut b = backoff.lock().unwrap();
        b.wait = (b.wait * 2)
            .max(Duration::from_secs(MIN_INTERVAL_SECS))
            .max(retry_after.unwrap_or_default())
            .min(MAX_BACKOFF);
        b.until = Some(Instant::now() + b.wait);
        return Err(status_error(429));
    }
    *backoff.lock().unwrap() = Backoff::default();
    if code == 401 {
        return Err(EXPIRED.into());
    }
    if !resp.status().is_success() {
        return Err(status_error(code));
    }
    let body: Value = resp
        .body_mut()
        .read_json()
        .map_err(|_| "resposta inesperada".to_string())?;

    let mut windows = Vec::new();
    let mut add = |key: &str, label: &str, mins: u64| {
        let Some(w) = body.get(key).filter(|w| w.is_object()) else {
            return;
        };
        let Some(used) = w.get("utilization").and_then(Value::as_f64) else {
            return;
        };
        let reset = w.get("resets_at").and_then(parse_time);
        windows.push(
            Window::new(label, 1.0 - used as f32 / 100.0)
                .resets(reset)
                .length_mins(mins),
        );
    };
    add("five_hour", "5 horas", 300);
    add("seven_day", "Semanal", 10_080);

    if windows.is_empty() {
        return Err("sem limites informados".into());
    }
    Ok(Fetched {
        groups: vec![Group {
            name: String::new(),
            windows,
        }],
        account: account_email(dir),
        title: None,
    })
}
