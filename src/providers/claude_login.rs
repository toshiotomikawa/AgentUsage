//! Renewal of an expired Claude Code login, done the way Claude Code itself does it, so the app
//! never needs a Claude Code window open just to read the usage.
//!
//! Claude Code keeps its login in `<config dir>/.credentials.json` and renews it while it runs.
//! The refresh token is single use (each renewal returns a new one), so two programs renewing at
//! once could leave one of them holding a dead token. To avoid that, the app takes the same two
//! lock directories Claude Code takes before renewing (`<dir>/.oauth_refresh.lock` and the legacy
//! `<dir>.lock`, created with `mkdir`, stale after 60 s), reads the file again under the locks, and
//! renews only if nobody else did. The new login is written back in one step over the same file,
//! keeping its permissions, and kept in memory until it is saved.

use super::{agent, read_json, status_error};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

pub(super) const EXPIRED: &str = "login expirado — entre de novo no Claude Code";
pub(crate) const BUSY: &str = "login em renovação — tentando de novo em breve";
/// Claude Code's own sign-in service and app id.
const TOKEN_URL: &str = "https://platform.claude.com/v1/oauth/token";
const CLIENT_ID: &str = "9d1c250a-e61b-44d9-88ed-5944d1962f5e";
/// Renew a little before the token runs out, not on the edge.
const RENEW_MARGIN_MS: i64 = 60_000;
/// Claude Code treats a lock directory older than this as left behind by a dead process.
const LOCK_STALE: Duration = Duration::from_secs(60);
const TEMP_PREFIX: &str = ".credentials.json.agent-usage-";
/// A token longer than this, or a lifetime outside 1 s..=1 year, is not a real answer.
const MAX_TOKEN_LEN: usize = 16 * 1024;
const MAX_LIFETIME_SECS: i64 = 366 * 24 * 3600;
const MIN_BACKOFF: Duration = Duration::from_secs(300);
const MAX_BACKOFF: Duration = Duration::from_secs(30 * 60);

/// The stored login.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Login {
    pub token: String,
    pub refresh: Option<String>,
    pub expires_ms: Option<i64>,
    pub scopes: Vec<String>,
}

/// Renewal state of one account.
#[derive(Default)]
pub(super) struct Renewal {
    /// No renewal request before this (the sign-in service asked to wait).
    until: Option<Instant>,
    wait: Duration,
    /// A renewal the service already granted but that could not be saved yet. The old refresh
    /// token it replaced tells whether the file still needs it.
    pending: Option<(String, Login)>,
}

impl Renewal {
    pub fn has_pending(&self) -> bool {
        self.pending.is_some()
    }
}

pub(super) fn read_login(path: &Path) -> Result<(Value, Login), String> {
    let creds = read_json(path)?;
    let oauth = creds
        .get("claudeAiOauth")
        .ok_or("sem login por assinatura")?;
    let token = oauth
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|t| valid_token(t))
        .ok_or("sem login por assinatura")?
        .to_string();
    let login = Login {
        token,
        refresh: oauth
            .get("refreshToken")
            .and_then(Value::as_str)
            .filter(|t| valid_token(t))
            .map(String::from),
        expires_ms: oauth.get("expiresAt").and_then(Value::as_i64),
        scopes: oauth
            .get("scopes")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default(),
    };
    Ok((creds, login))
}

pub(super) fn near_expiry(login: &Login) -> bool {
    login.expires_ms.is_some_and(|ms| {
        ms.saturating_sub(RENEW_MARGIN_MS) <= chrono::Utc::now().timestamp_millis()
    })
}

fn valid_token(t: &str) -> bool {
    !t.is_empty() && t.len() <= MAX_TOKEN_LEN && t.bytes().all(|b| b.is_ascii_graphic())
}

/// Renew the login in `dir` if it has expired, or if the service just refused `refused`.
/// Returns the login to use, which may be one another program renewed meanwhile.
pub(super) fn renew(
    dir: &Path,
    refused: Option<&str>,
    state: &Mutex<Renewal>,
) -> Result<Login, String> {
    if state
        .lock()
        .unwrap()
        .until
        .is_some_and(|t| Instant::now() < t)
    {
        return Err(status_error(429));
    }
    let path = dir.join(".credentials.json");
    let _lock = DirLock::acquire(dir)?;
    remove_own_temps(dir);
    let (_, login) = read_login(&path)?;

    // A renewal granted earlier but not saved: save it now, unless the file moved on.
    let pending = state.lock().unwrap().pending.take();
    if let Some((old_refresh, new)) = pending
        && login.refresh.as_deref() == Some(old_refresh.as_str())
    {
        // The refresh token the file holds now: the new one once saved, else still the old one.
        let in_file = if store(&path, &new).is_ok() {
            new.refresh.clone().unwrap_or_default()
        } else {
            state.lock().unwrap().pending = Some((old_refresh.clone(), new.clone()));
            old_refresh
        };
        if !near_expiry(&new) && refused != Some(new.token.as_str()) {
            return Ok(new);
        }
        return renew_now(&path, new, &in_file, state);
    }

    if !near_expiry(&login) && refused != Some(login.token.as_str()) {
        // Someone else renewed it already.
        return Ok(login);
    }
    let in_file = login.refresh.clone().unwrap_or_default();
    renew_now(&path, login, &in_file, state)
}

/// Ask the sign-in service for a new login and save it. Runs under the locks. `in_file` is the
/// refresh token the file holds now, which a save still pending would replace.
fn renew_now(
    path: &Path,
    login: Login,
    in_file: &str,
    state: &Mutex<Renewal>,
) -> Result<Login, String> {
    let refresh = login.refresh.clone().ok_or(EXPIRED)?;
    let mut body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh,
        "client_id": CLIENT_ID,
    });
    if !login.scopes.is_empty() {
        body["scope"] = Value::from(login.scopes.join(" "));
    }
    let mut resp = agent()
        .post(TOKEN_URL)
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .map_err(|_| "sem conexão".to_string())?;
    let code = resp.status().as_u16();
    if code == 429 {
        let asked = resp
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .and_then(super::claude::parse_retry_after);
        let mut s = state.lock().unwrap();
        s.wait = (s.wait * 2).max(MIN_BACKOFF).min(MAX_BACKOFF);
        let wait = s.wait.max(asked.unwrap_or_default());
        s.until = Instant::now().checked_add(wait);
        return Err(status_error(429));
    }
    let v: Option<Value> = resp.body_mut().read_json().ok();
    if code != 200 {
        let invalid_grant = v
            .as_ref()
            .and_then(|v| v.get("error"))
            .and_then(Value::as_str)
            == Some("invalid_grant");
        if code == 401 || code == 403 || (code == 400 && invalid_grant) {
            // Maybe another program renewed it meanwhile and its token is the good one now.
            if let Ok((_, now)) = read_login(path)
                && now.refresh.as_deref() != Some(refresh.as_str())
                && !near_expiry(&now)
            {
                return Ok(now);
            }
            return Err(EXPIRED.into());
        }
        return Err(status_error(code));
    }
    let new = parse_grant(
        v.as_ref().ok_or("resposta inesperada")?,
        &login,
        chrono::Utc::now().timestamp_millis(),
    )?;
    {
        let mut s = state.lock().unwrap();
        s.until = None;
        s.wait = Duration::ZERO;
    }
    // The old refresh token is spent now: if the file cannot take the new one yet, keep it in
    // memory and save it on the next read instead of asking for another.
    let mut s = state.lock().unwrap();
    s.pending = None;
    drop(s);
    if store(path, &new).is_err() {
        state.lock().unwrap().pending = Some((in_file.to_string(), new.clone()));
    }
    Ok(new)
}

/// Check a renewal answer and build the new login from it, without touching anything.
fn parse_grant(v: &Value, old: &Login, now_ms: i64) -> Result<Login, String> {
    let bad = || "resposta inesperada".to_string();
    let token = v
        .get("access_token")
        .and_then(Value::as_str)
        .filter(|t| valid_token(t))
        .ok_or_else(bad)?;
    let refresh = match v.get("refresh_token") {
        None | Some(Value::Null) => old.refresh.clone().ok_or_else(bad)?,
        Some(r) => r
            .as_str()
            .filter(|t| valid_token(t))
            .ok_or_else(bad)?
            .to_string(),
    };
    let expires_ms = v
        .get("expires_in")
        .and_then(Value::as_i64)
        .filter(|s| (1..=MAX_LIFETIME_SECS).contains(s))
        .and_then(|s| s.checked_mul(1000))
        .and_then(|d| now_ms.checked_add(d))
        .ok_or_else(bad)?;
    let scopes = match v.get("scope").and_then(Value::as_str) {
        Some(s) if !s.trim().is_empty() => s.split_whitespace().map(String::from).collect(),
        _ => old.scopes.clone(),
    };
    Ok(Login {
        token: token.to_string(),
        refresh: Some(refresh),
        expires_ms: Some(expires_ms),
        scopes,
    })
}

/// Write `login` into the file, read again right before so other fields stay as they are now.
/// Retries briefly: a reader (backup, antivirus) may hold the file for a moment.
fn store(path: &Path, login: &Login) -> Result<(), ()> {
    for attempt in 0..10 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(200));
        }
        let Ok(mut creds) = read_json(path) else {
            continue;
        };
        let Some(oauth) = creds
            .get_mut("claudeAiOauth")
            .and_then(Value::as_object_mut)
        else {
            return Err(());
        };
        oauth.insert("accessToken".into(), login.token.clone().into());
        if let Some(r) = &login.refresh {
            oauth.insert("refreshToken".into(), r.clone().into());
        }
        if let Some(ms) = login.expires_ms {
            oauth.insert("expiresAt".into(), ms.into());
        }
        oauth.insert("scopes".into(), login.scopes.clone().into());
        if save(path, &creds).is_ok() {
            return Ok(());
        }
    }
    Err(())
}

/// Replace the file in one step, so nobody ever reads half of it, keeping its permissions.
pub(super) fn save(path: &Path, creds: &Value) -> std::io::Result<()> {
    use std::io::Write;
    let dir = path.parent().ok_or(std::io::ErrorKind::NotFound)?;
    let text = serde_json::to_vec(creds)?;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    let tmp = Temp(dir.join(format!("{TEMP_PREFIX}{}-{nanos}.tmp", std::process::id())));
    let mut opts = std::fs::OpenOptions::new();
    // A new file only: never write through a name someone prepared (a link, say).
    opts.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut opts, 0o600);
    let mut f = opts.open(&tmp.0)?;
    f.write_all(&text)?;
    f.sync_all()?;
    drop(f);
    replace(&tmp.0, path)?;
    std::mem::forget(tmp);
    Ok(())
}

/// Removes the temporary file unless it was moved into place.
struct Temp(PathBuf);
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(windows)]
fn replace(tmp: &Path, path: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW};
    let wide = |p: &Path| -> Vec<u16> { p.as_os_str().encode_wide().chain([0]).collect() };
    let (to, from) = (wide(path), wide(tmp));
    // Unlike a rename, this keeps the original file's permissions and attributes.
    let ok = unsafe {
        ReplaceFileW(
            to.as_ptr(),
            from.as_ptr(),
            std::ptr::null(),
            REPLACEFILE_IGNORE_MERGE_ERRORS,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if ok != 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(windows))]
fn replace(tmp: &Path, path: &Path) -> std::io::Result<()> {
    if let Ok(meta) = std::fs::metadata(path) {
        let _ = std::fs::set_permissions(tmp, meta.permissions());
    }
    std::fs::rename(tmp, path)
}

/// Temporary files this app left behind after a crash. Runs under the locks, so none is in use.
fn remove_own_temps(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let name = e.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(TEMP_PREFIX)
            && name.ends_with(".tmp")
            && e.path().symlink_metadata().is_ok_and(|m| m.is_file())
        {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Claude Code's renewal locks, held together; released on drop.
struct DirLock(Vec<PathBuf>);

impl DirLock {
    fn acquire(dir: &Path) -> Result<Self, String> {
        let main = dir.join(".oauth_refresh.lock");
        let mut legacy = dir.as_os_str().to_owned();
        legacy.push(".lock");
        let legacy = PathBuf::from(legacy);
        for attempt in 0..6 {
            if attempt > 0 {
                // Claude Code waits 1–2 s between tries; so do we.
                let jitter = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.subsec_millis() as u64)
                    .unwrap_or(500);
                std::thread::sleep(Duration::from_millis(1000 + jitter));
            }
            if take(&main) {
                if take(&legacy) {
                    return Ok(DirLock(vec![legacy, main]));
                }
                let _ = std::fs::remove_dir(&main);
            }
        }
        Err(BUSY.into())
    }
}

impl Drop for DirLock {
    fn drop(&mut self) {
        for p in &self.0 {
            let _ = std::fs::remove_dir(p);
        }
    }
}

/// Create a lock directory; one left by a process that died (older than `LOCK_STALE`) is taken over.
fn take(p: &Path) -> bool {
    for _ in 0..2 {
        match std::fs::create_dir(p) {
            Ok(()) => return true,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = p.symlink_metadata().is_ok_and(|m| {
                    m.is_dir()
                        && m.modified()
                            .ok()
                            .and_then(|t| t.elapsed().ok())
                            .is_some_and(|age| age > LOCK_STALE)
                });
                if !stale || std::fs::remove_dir(p).is_err() {
                    return false;
                }
            }
            Err(_) => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("agent-usage-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn old() -> Login {
        Login {
            token: "a".into(),
            refresh: Some("r".into()),
            expires_ms: Some(1),
            scopes: vec!["x".into()],
        }
    }

    #[test]
    fn grant_is_checked_before_use() {
        let now = 1_000_000;
        let ok = parse_grant(
            &serde_json::json!({"access_token":"b","refresh_token":"s","expires_in":3600}),
            &old(),
            now,
        )
        .unwrap();
        assert_eq!(ok.expires_ms, Some(now + 3_600_000));
        assert_eq!(ok.refresh.as_deref(), Some("s"));
        assert_eq!(ok.scopes, ["x"]);
        for bad in [
            serde_json::json!({"access_token":"","expires_in":3600}),
            serde_json::json!({"access_token":"b","refresh_token":"","expires_in":3600}),
            serde_json::json!({"access_token":"b","expires_in":-1}),
            serde_json::json!({"access_token":"b","expires_in":i64::MAX}),
            serde_json::json!({"access_token":"b c","expires_in":3600}),
            serde_json::json!({"expires_in":3600}),
        ] {
            assert!(parse_grant(&bad, &old(), now).is_err(), "{bad}");
        }
        // No new refresh token: the old one stays.
        let kept = parse_grant(
            &serde_json::json!({"access_token":"b","expires_in":60}),
            &old(),
            now,
        )
        .unwrap();
        assert_eq!(kept.refresh.as_deref(), Some("r"));
    }

    #[test]
    fn extreme_expiry_does_not_overflow() {
        let mut l = old();
        l.expires_ms = Some(i64::MIN);
        assert!(near_expiry(&l));
        l.expires_ms = Some(i64::MAX);
        assert!(!near_expiry(&l));
    }

    #[test]
    fn store_keeps_other_fields_and_leaves_no_temp() {
        let dir = temp_dir("store");
        let path = dir.join(".credentials.json");
        std::fs::write(
            &path,
            r#"{"other":1,"claudeAiOauth":{"accessToken":"a","refreshToken":"r","expiresAt":1,"scopes":["x"],"subscriptionType":"pro"}}"#,
        )
        .unwrap();
        let mut new = old();
        new.token = "b".into();
        new.expires_ms = Some(5);
        store(&path, &new).unwrap();
        let (again, login) = read_login(&path).unwrap();
        assert_eq!(login, new);
        assert_eq!(again["other"], 1);
        assert_eq!(again["claudeAiOauth"]["subscriptionType"], "pro");
        let left: Vec<_> = std::fs::read_dir(&dir).unwrap().collect();
        assert_eq!(left.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn locks_are_shared_and_stale_ones_taken_over() {
        let dir = temp_dir("lock");
        let held = DirLock::acquire(&dir).unwrap();
        assert!(dir.join(".oauth_refresh.lock").is_dir());
        assert!(!take(&dir.join(".oauth_refresh.lock")));
        drop(held);
        assert!(!dir.join(".oauth_refresh.lock").exists());
        // A fresh lock someone else holds is respected; an old one is taken over.
        let other = dir.join("held.lock");
        std::fs::create_dir(&other).unwrap();
        assert!(!take(&other));
        let past = SystemTime::now() - Duration::from_secs(120);
        std::fs::File::open(&other)
            .and_then(|f| f.set_modified(past))
            .ok();
        if other.metadata().unwrap().modified().unwrap() <= past {
            assert!(take(&other));
        }
        let _ = std::fs::remove_dir_all(&dir);
        let mut legacy = dir.into_os_string();
        legacy.push(".lock");
        let _ = std::fs::remove_dir(legacy);
    }

    #[test]
    fn renewal_backoff_blocks_requests() {
        let state = Mutex::new(Renewal {
            until: Some(Instant::now() + Duration::from_secs(60)),
            ..Default::default()
        });
        let dir = temp_dir("backoff");
        assert_eq!(renew(&dir, None, &state), Err(status_error(429)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn valid_login_needs_no_request() {
        let dir = temp_dir("fresh");
        let far = chrono::Utc::now().timestamp_millis() + 3_600_000;
        std::fs::write(
            dir.join(".credentials.json"),
            format!(
                r#"{{"claudeAiOauth":{{"accessToken":"a","refreshToken":"r","expiresAt":{far}}}}}"#
            ),
        )
        .unwrap();
        let state = Mutex::new(Renewal::default());
        let l = renew(&dir, None, &state).unwrap();
        assert_eq!(l.token, "a");
        // Locks released.
        assert!(!dir.join(".oauth_refresh.lock").exists());
        let _ = std::fs::remove_dir_all(&dir);
        let mut legacy = dir.into_os_string();
        legacy.push(".lock");
        let _ = std::fs::remove_dir(legacy);
    }
}
