//! Claude Code subscription limits: `GET api.anthropic.com/api/oauth/usage` with the OAuth token
//! Claude Code keeps in `<config dir>/.credentials.json`. One source per config folder (account).

use super::claude_login::{EXPIRED, Renewal, near_expiry, read_login, renew};
use super::{Fetched, Source, agent, parse_time, status_error};
use crate::config::Config;
use crate::model::{Group, Provider, Window, email_local_part};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const URL: &str = "https://api.anthropic.com/api/oauth/usage";
const MIN_INTERVAL_SECS: u64 = 300;
/// Longest wait after repeated rate-limit answers.
const MAX_BACKOFF: Duration = Duration::from_secs(30 * 60);

/// Pause after a rate-limit answer: no request goes out before `until`, and each further 429
/// doubles the pause, so the limit gets the quiet time it needs to reset.
#[derive(Default)]
struct Backoff {
    until: Option<Instant>,
    wait: Duration,
    /// Account behind the current token (hash of the token, email local part), asked once.
    who: Option<(u64, Option<String>)>,
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
            let renewal = Arc::new(Mutex::new(Renewal::default()));
            Source {
                key: format!("claude-{i}"),
                provider: Provider::Claude,
                fetch: Arc::new(move || fetch(&dir, &backoff, &renewal)),
                // The usage endpoint rate-limits frequent callers; Claude Code itself reads it
                // rarely. Its numbers move slowly, so a few minutes between reads loses nothing.
                min_interval: Some(MIN_INTERVAL_SECS),
                optional,
                account,
            }
        })
        .collect()
}

const PROFILE_URL: &str = "https://api.anthropic.com/api/oauth/profile";

/// The account a token belongs to, asked from the service once per token. The `.claude.json`
/// files cannot be trusted for this: the home one follows whichever login ran last.
fn token_account(token: &str, backoff: &Mutex<Backoff>) -> Option<String> {
    use std::hash::{DefaultHasher, Hash, Hasher};
    let mut h = DefaultHasher::new();
    token.hash(&mut h);
    let key = h.finish();
    if let Some((k, who)) = &backoff.lock().unwrap().who
        && *k == key
    {
        return who.clone();
    }
    let who = agent()
        .get(PROFILE_URL)
        .header("Authorization", &format!("Bearer {token}"))
        .header("anthropic-beta", "oauth-2025-04-20")
        .call()
        .ok()
        .filter(|r| r.status().is_success())
        .and_then(|mut r| r.body_mut().read_json::<Value>().ok())
        .and_then(|v| {
            v.pointer("/account/email_address")
                .or_else(|| v.pointer("/account/email"))
                .and_then(Value::as_str)
                .map(email_local_part)
        });
    match who {
        Some(w) => {
            backoff.lock().unwrap().who = Some((key, Some(w.clone())));
            Some(w)
        }
        // Not cached, so the next read asks again; meanwhile the card shows the folder.
        None => None,
    }
}

/// `Retry-After` as seconds or as an HTTP date.
pub(super) fn parse_retry_after(v: &str) -> Option<Duration> {
    let v = v.trim();
    if let Ok(secs) = v.parse::<u64>() {
        return Some(Duration::from_secs(secs));
    }
    let at = chrono::DateTime::parse_from_rfc2822(v).ok()?;
    (at.with_timezone(&chrono::Utc) - chrono::Utc::now())
        .to_std()
        .ok()
}

fn fetch(
    dir: &Path,
    backoff: &Mutex<Backoff>,
    renewal: &Mutex<Renewal>,
) -> Result<Fetched, String> {
    let (_, mut login) = read_login(&dir.join(".credentials.json"))?;
    if backoff
        .lock()
        .unwrap()
        .until
        .is_some_and(|t| Instant::now() < t)
    {
        return Err(status_error(429));
    }
    // Claude Code renews the token only while it runs; renew it here when it has run out (or
    // save a renewal still waiting to be written).
    let mut renewed = false;
    if near_expiry(&login) || renewal.lock().unwrap().has_pending() {
        login = renew(dir, None, renewal)?;
        renewed = true;
    }
    let usage = |token: &str| {
        agent()
            .get(URL)
            .header("Authorization", &format!("Bearer {token}"))
            .header("anthropic-beta", "oauth-2025-04-20")
            .call()
            .map_err(|_| "sem conexão".to_string())
    };
    let mut resp = usage(&login.token)?;
    if resp.status().as_u16() == 401 && !renewed && login.refresh.is_some() {
        // Revoked before its expiry (a sign-in elsewhere, say): renew once and ask again.
        login = renew(dir, Some(&login.token), renewal)?;
        resp = usage(&login.token)?;
    }
    let code = resp.status().as_u16();
    if code == 429 {
        let retry_after = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(parse_retry_after);
        let mut b = backoff.lock().unwrap();
        b.wait = (b.wait * 2)
            .max(Duration::from_secs(MIN_INTERVAL_SECS))
            .min(MAX_BACKOFF);
        // Our own doubling is capped; a longer wait asked by the service is not shortened.
        let wait = b.wait.max(retry_after.unwrap_or_default());
        b.until = Some(
            Instant::now()
                .checked_add(wait)
                .unwrap_or_else(|| Instant::now() + MAX_BACKOFF * 48),
        );
        return Err(status_error(429));
    }
    {
        let mut b = backoff.lock().unwrap();
        b.until = None;
        b.wait = Duration::ZERO;
    }
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
        account: token_account(&login.token, backoff),
        title: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads every real Claude folder, renewing expired logins; prints only the outcome.
    /// Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn live_usage() {
        let mut failed = 0;
        for (i, src) in sources(&Config::default()).iter().enumerate() {
            let r = (src.fetch)().map(|f| f.groups.len());
            // Numbered, not named: folder names can identify a person.
            println!(
                "conta {}: {}",
                i + 1,
                r.as_ref().map_or_else(|e| e.as_str(), |_| "ok")
            );
            failed += r.is_err() as usize;
        }
        assert_eq!(failed, 0, "{failed} conta(s) sem dados");
    }

    #[test]
    fn retry_after_forms() {
        assert_eq!(parse_retry_after(" 120 "), Some(Duration::from_secs(120)));
        let later = (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc2822();
        let d = parse_retry_after(&later).unwrap();
        assert!(d > Duration::from_secs(7000));
        assert_eq!(parse_retry_after("soon"), None);
    }
}
