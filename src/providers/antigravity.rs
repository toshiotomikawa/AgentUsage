//! Antigravity (Google) plan quota from `agy -p /usage --output-format json`.
//!
//! The CLI answers this command itself, without a model turn (0 tokens), and returns structured
//! data under `command.data.groups[].buckets[]`:
//!
//! ```json
//! {"name": "Gemini Models", "buckets": [
//!   {"window": "5h", "remaining_fraction": 0.99, "reset_time": "2026-11-08T02:25:05Z"},
//!   {"window": "weekly", "remaining_fraction": 1.0, "reset_time": "2026-11-14T21:25:05Z"}]}
//! ```
//!
//! When silent auth fails, `agy -p` falls back to interactive OAuth and launches the browser via
//! `rundll32` on Windows (`open` / `xdg-open` on Unix). Setting `SSH_CONNECTION` suppresses the
//! browser, but also makes `agy` skip the OS keyring and break signed-in reads. Instead, we
//! prepend a no-op browser shim directory to the child's `PATH` and abort the process as soon as
//! `stderr` prints `"Authentication required"`, then back off until a reading succeeds.

use super::{Fetched, Source, parse_time};
use crate::hidden::RunError;
use crate::model::{Group, Provider, Window};
use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Minimum seconds between `agy -p /usage` runs (each launch spawns a ~165 MB CLI process).
const MIN_INTERVAL_SECS: u64 = 60;

/// Initial and maximum pause after `agy` reports it is not signed in.
const INITIAL_AUTH_BACKOFF: Duration = Duration::from_secs(5 * 60);
const MAX_AUTH_BACKOFF: Duration = Duration::from_secs(30 * 60);

/// Error shown on the card when `agy` has no valid sign-in.
pub(crate) const NOT_SIGNED_IN: &str = "sem login — entre no agy";

/// Stderr marker printed by `agy -p` as soon as silent auth fails and interactive OAuth starts.
const AUTH_STDERR_MARKER: &str = "Authentication required";

/// Pause between signed-out polls so we do not spawn `agy` every minute while signed out.
/// Doubles from 5 min up to 30 min, and resets immediately once a reading succeeds.
#[derive(Default)]
struct AuthBackoff {
    until: Option<Instant>,
    wait: Duration,
}

impl AuthBackoff {
    fn is_active(&self, now: Instant) -> bool {
        self.until.is_some_and(|t| now < t)
    }

    fn record_signed_out(&mut self, now: Instant) {
        self.wait = (self.wait * 2)
            .max(INITIAL_AUTH_BACKOFF)
            .min(MAX_AUTH_BACKOFF);
        self.until = now.checked_add(self.wait);
    }

    fn reset(&mut self) {
        self.until = None;
        self.wait = Duration::ZERO;
    }
}

#[derive(Debug, PartialEq, Eq)]
enum UsageError {
    AuthRequired,
    NotFound,
    Timeout,
    Failed,
}

pub fn source() -> Source {
    let backoff = Arc::new(Mutex::new(AuthBackoff::default()));
    Source {
        key: "antigravity".into(),
        provider: Provider::Antigravity,
        fetch: Arc::new(move || {
            fetch_with(&backoff, Instant::now, run_usage, || {
                crate::cache::clear("antigravity")
            })
        }),
        min_interval: Some(MIN_INTERVAL_SECS),
        optional: false,
        // agy takes a few seconds to answer: show its last reading at once on launch.
        remember: true,
        account: None,
    }
}

fn fetch_with(
    backoff: &Mutex<AuthBackoff>,
    now: impl Fn() -> Instant,
    runner: impl FnOnce() -> Result<String, UsageError>,
    on_signed_out: impl FnOnce(),
) -> Result<Fetched, String> {
    if backoff.lock().unwrap().is_active(now()) {
        return Err(NOT_SIGNED_IN.into());
    }
    let out = match runner() {
        Ok(out) => out,
        Err(UsageError::AuthRequired) => {
            backoff.lock().unwrap().record_signed_out(now());
            on_signed_out();
            return Err(NOT_SIGNED_IN.into());
        }
        Err(UsageError::NotFound) => return Err("agy não encontrado".into()),
        Err(UsageError::Timeout) => return Err("agy não respondeu".into()),
        Err(UsageError::Failed) => return Err("agy falhou".into()),
    };
    let v: Value =
        serde_json::from_str(out.trim()).map_err(|_| "resposta inesperada do agy".to_string())?;
    if v.get("status")
        .and_then(Value::as_str)
        .is_some_and(|s| s != "SUCCESS")
    {
        return Err("agy não conseguiu ler a cota".into());
    }
    let groups = parse(&v);
    if groups.is_empty() {
        return Err("sem cota de plano (login por chave de API?)".into());
    }
    backoff.lock().unwrap().reset();
    Ok(Fetched {
        groups,
        account: None,
        title: None,
    })
}

/// Directory containing no-op browser opener shims, prepended to the child's `PATH` so `agy`
/// can never launch a browser tab even if silent auth fails.
fn no_browser_dir() -> Option<PathBuf> {
    let dir = dirs::data_local_dir()
        .map(|d| d.join("agent-usage").join("cache").join("no-browser"))
        .unwrap_or_else(|| std::env::temp_dir().join("agent-usage-no-browser"));
    std::fs::create_dir_all(&dir).ok()?;
    #[cfg(windows)]
    {
        write_if_changed(&dir.join("rundll32.cmd"), b"@exit /b 1\r\n")?;
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        for name in ["xdg-open", "open"] {
            let p = dir.join(name);
            write_if_changed(&p, b"#!/bin/sh\nexit 1\n")?;
            let _ = std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755));
        }
    }
    Some(dir)
}

fn write_if_changed(path: &Path, content: &[u8]) -> Option<()> {
    if std::fs::read(path).ok().as_deref() != Some(content) {
        std::fs::write(path, content).ok()?;
    }
    Some(())
}

fn child_env(shim_dir: Option<&Path>) -> Vec<(&'static str, OsString)> {
    let mut env = Vec::new();
    if let Some(dir) = shim_dir {
        let mut paths = vec![dir.to_path_buf()];
        if let Some(existing) = std::env::var_os("PATH") {
            paths.extend(std::env::split_paths(&existing));
        }
        if let Ok(joined) = std::env::join_paths(paths) {
            env.push(("PATH", joined));
        }
    }
    #[cfg(not(windows))]
    {
        env.push(("BROWSER", OsString::from("false")));
    }
    env
}

fn is_signed_out_text(text: &str) -> bool {
    text.contains(AUTH_STDERR_MARKER)
        || text.contains("Print mode: silent auth failed")
        || text.contains("Print mode: triggering interactive OAuth")
        || text.contains("not logged in and no controlling terminal")
}

fn read_log_tail(path: &Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(16 * 1024);
    f.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    f.take(16 * 1024).read_to_end(&mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn run_usage() -> Result<String, UsageError> {
    // Prefer the installed binary over a `.cmd` shim, which would add a `cmd.exe` in between.
    let exe = dirs::data_local_dir()
        .map(|d| d.join("agy").join("bin").join("agy.exe"))
        .filter(|p| p.is_file())
        .map_or_else(|| "agy".into(), |p| p.into_os_string());
    // agy otherwise writes a new log file under ~/.gemini/antigravity-cli/log on every call,
    // thousands a day at this refresh rate. One file of our own, overwritten each time, instead.
    let log_path = dirs::data_local_dir()
        .map(|d| d.join("agent-usage").join("cache"))
        .filter(|d| std::fs::create_dir_all(d).is_ok())
        .map(|d| d.join("agy.log"));
    let log_str = log_path.as_ref().map(|p| p.to_string_lossy().into_owned());
    let mut args = vec!["-p", "/usage", "--output-format", "json"];
    if let Some(log) = &log_str {
        args.extend(["--log-file", log]);
    }
    let shim_dir = no_browser_dir();
    let env_owned = child_env(shim_dir.as_deref());
    let env_refs: Vec<(&str, &std::ffi::OsStr)> =
        env_owned.iter().map(|(k, v)| (*k, v.as_os_str())).collect();
    match crate::hidden::run(
        &exe,
        &args,
        &std::env::temp_dir(),
        Duration::from_secs(45),
        &env_refs,
        Some(AUTH_STDERR_MARKER),
    ) {
        Ok(out) => {
            if is_signed_out_text(&out.stderr) {
                return Err(UsageError::AuthRequired);
            }
            Ok(out.stdout)
        }
        Err(RunError::Aborted) => Err(UsageError::AuthRequired),
        Err(RunError::NotFound) => Err(UsageError::NotFound),
        Err(err) => {
            if log_path
                .as_deref()
                .and_then(read_log_tail)
                .is_some_and(|tail| is_signed_out_text(&tail))
            {
                return Err(UsageError::AuthRequired);
            }
            match err {
                RunError::Timeout => Err(UsageError::Timeout),
                _ => Err(UsageError::Failed),
            }
        }
    }
}

pub(crate) fn parse(v: &Value) -> Vec<Group> {
    let Some(groups) = v.pointer("/command/data/groups").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for g in groups {
        let name = g.get("name").and_then(Value::as_str).unwrap_or("");
        let mut windows = Vec::new();
        for b in g
            .get("buckets")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let Some(rem) = b.get("remaining_fraction").and_then(Value::as_f64) else {
                continue;
            };
            let (label, mins) = match b.get("window").and_then(Value::as_str) {
                Some("5h") => ("5 horas", 300),
                Some("weekly") => ("Semanal", 10_080),
                Some("daily") => ("Diária", 1_440),
                Some("monthly") => ("Mensal", 43_200),
                _ => ("Limite", 0),
            };
            // An untouched window's timer has not started: agy reports the Unix epoch. Kept, it
            // read as a reset already past, and the saved reading was refused for being stale.
            let reset = b
                .get("reset_time")
                .and_then(parse_time)
                .filter(|t| t.timestamp() > 0);
            let mut w = Window::new(label, rem as f32).resets(reset);
            if mins > 0 {
                w = w.length_mins(mins);
            }
            windows.push(w);
        }
        if windows.is_empty() {
            continue;
        }
        // Shortest window first: 5 hours, then weekly.
        windows.sort_by_key(|w| w.length.map(|d| d.as_secs()).unwrap_or(u64::MAX));
        out.push(Group {
            note: None,
            name: short_group(name),
            windows,
        });
    }
    out
}

fn short_group(name: &str) -> String {
    let lower = name.to_lowercase();
    if lower == "gemini models" {
        "Gemini".into()
    } else if lower.starts_with("claude and gpt") {
        "Claude & GPT".into()
    } else {
        name.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const SAMPLE_JSON: &str = r#"{"status":"SUCCESS","command":{"name":"usage","data":{"groups":[
      {"name":"Gemini Models","buckets":[
        {"window":"weekly","remaining_fraction":0.5,"reset_time":"2026-11-14T21:25:05Z"},
        {"window":"5h","remaining_fraction":0.99,"reset_time":"2026-11-08T02:25:05Z"}]},
      {"name":"Claude and GPT models","buckets":[{"window":"5h","remaining_fraction":1,"reset_time":"1970-01-01T00:00:00Z"}]},
      {"name":"Gemini 4 Argon","buckets":[{"window":"weekly","remaining_fraction":0.98}]}]}}}"#;

    /// Calls the real `agy` (no model turn). Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn live_usage() {
        // As in main: without it, this run could start agy's own updater.
        // SAFETY: an ignored test, run on its own.
        unsafe { std::env::set_var("AGY_CLI_DISABLE_AUTO_UPDATE", "true") };
        let backoff = Mutex::new(AuthBackoff::default());
        let f = fetch_with(&backoff, Instant::now, run_usage, || {}).unwrap();
        assert!(!f.groups.is_empty());
    }

    /// Calls the real `agy` in an isolated signed-out environment (never touching the real
    /// keyring) with a temporary marker-writing browser shim, proving the shim was invoked
    /// instead of a real browser and the process aborted on the auth prompt.
    #[test]
    #[ignore]
    fn live_signed_out_opens_no_browser() {
        let exe = dirs::data_local_dir()
            .map(|d| d.join("agy").join("bin").join("agy.exe"))
            .filter(|p| p.is_file())
            .map_or_else(|| OsString::from("agy"), |p| p.into_os_string());
        let tmp =
            std::env::temp_dir().join(format!("agent-usage-signed-out-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        struct Clean(PathBuf);
        impl Drop for Clean {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _clean = Clean(tmp.clone());
        let shim = tmp.join("shim");
        std::fs::create_dir_all(&shim).unwrap();
        let marker = tmp.join("browser-called.txt");
        #[cfg(windows)]
        {
            let script = format!("@echo %* > \"{}\"\r\n@exit /b 1\r\n", marker.display());
            std::fs::write(shim.join("rundll32.cmd"), script).unwrap();
        }
        #[cfg(not(windows))]
        {
            use std::os::unix::fs::PermissionsExt;
            let script = format!(
                "#!/bin/sh\necho \"$@\" > \"{}\"\nexit 1\n",
                marker.display()
            );
            for name in ["xdg-open", "open"] {
                let p = shim.join(name);
                std::fs::write(&p, &script).unwrap();
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        let log = tmp.join("agy.log").to_string_lossy().into_owned();
        let mut env_owned = child_env(Some(&shim));
        env_owned.extend([
            ("AGY_CLI_DISABLE_AUTO_UPDATE", OsString::from("true")),
            ("AGY_CLI_FORCE_FILE_TOKEN_STORAGE", OsString::from("true")),
            ("USERPROFILE", tmp.as_os_str().to_os_string()),
            ("HOME", tmp.as_os_str().to_os_string()),
            ("APPDATA", tmp.as_os_str().to_os_string()),
            ("LOCALAPPDATA", tmp.as_os_str().to_os_string()),
        ]);
        let env_refs: Vec<(&str, &std::ffi::OsStr)> =
            env_owned.iter().map(|(k, v)| (*k, v.as_os_str())).collect();
        let err = crate::hidden::run(
            &exe,
            &[
                "-p",
                "/usage",
                "--output-format",
                "json",
                "--log-file",
                &log,
            ],
            &tmp,
            Duration::from_secs(15),
            &env_refs,
            Some(AUTH_STDERR_MARKER),
        )
        .unwrap_err();
        assert_eq!(err, RunError::Aborted);
        assert!(
            marker.is_file(),
            "expected browser shim marker file to be written by signed-out agy"
        );
        let recorded = std::fs::read_to_string(&marker).unwrap_or_default();
        assert!(
            recorded.contains("accounts.google.com") || recorded.contains("oauth"),
            "expected OAuth URL in shim marker, got: {recorded}"
        );
    }

    #[test]
    fn polls_at_most_once_a_minute() {
        assert_eq!(source().min_interval, Some(60));
    }

    #[test]
    fn parses_usage_command() {
        let v: Value = serde_json::from_str(SAMPLE_JSON).unwrap();
        let g = parse(&v);
        assert_eq!(g.len(), 3);
        assert_eq!(g[0].name, "Gemini");
        assert_eq!(g[0].windows[0].label, "5 horas");
        assert!((g[0].windows[1].remaining - 0.5).abs() < 1e-4);
        assert!(g[0].windows[1].resets_at.is_some());
        assert_eq!(g[1].name, "Claude & GPT");
        assert!(g[1].windows[0].resets_at.is_none());
        assert_eq!(g[2].name, "Gemini 4 Argon");
    }

    #[test]
    fn auth_backoff_grows_caps_and_resets() {
        let t0 = Instant::now();
        let mut b = AuthBackoff::default();
        assert!(!b.is_active(t0));

        b.record_signed_out(t0);
        assert_eq!(b.wait, Duration::from_secs(5 * 60));
        assert!(b.is_active(t0 + Duration::from_secs(4 * 60)));
        assert!(!b.is_active(t0 + Duration::from_secs(5 * 60)));

        let t1 = t0 + Duration::from_secs(5 * 60);
        b.record_signed_out(t1);
        assert_eq!(b.wait, Duration::from_secs(10 * 60));

        let t2 = t1 + Duration::from_secs(10 * 60);
        b.record_signed_out(t2);
        assert_eq!(b.wait, Duration::from_secs(20 * 60));

        let t3 = t2 + Duration::from_secs(20 * 60);
        b.record_signed_out(t3);
        assert_eq!(b.wait, MAX_AUTH_BACKOFF);

        b.reset();
        assert!(!b.is_active(t3));
        assert_eq!(b.wait, Duration::ZERO);
    }

    #[test]
    fn signed_out_backs_off_and_resumes_after_success() {
        let backoff = Mutex::new(AuthBackoff::default());
        let clock = Cell::new(Instant::now());
        let calls = Cell::new(0usize);
        let cleared = Cell::new(0usize);

        // First poll: signed out -> sets backoff, clears stale cache, returns sign-in message.
        let err = fetch_with(
            &backoff,
            || clock.get(),
            || {
                calls.set(calls.get() + 1);
                Err(UsageError::AuthRequired)
            },
            || cleared.set(cleared.get() + 1),
        )
        .err()
        .unwrap();
        assert_eq!(err, NOT_SIGNED_IN);
        assert!(!crate::providers::is_transient(&err));
        assert_eq!(calls.get(), 1);
        assert_eq!(cleared.get(), 1);

        // One minute later: still in backoff -> returns sign-in message without invoking agy.
        clock.set(clock.get() + Duration::from_secs(60));
        let err = fetch_with(
            &backoff,
            || clock.get(),
            || {
                calls.set(calls.get() + 1);
                Ok(SAMPLE_JSON.into())
            },
            || cleared.set(cleared.get() + 1),
        )
        .err()
        .unwrap();
        assert_eq!(err, NOT_SIGNED_IN);
        assert!(!crate::providers::is_transient(&err));
        assert_eq!(calls.get(), 1);

        // After backoff expires: agy runs again and succeeds -> backoff clears.
        clock.set(clock.get() + INITIAL_AUTH_BACKOFF);
        let ok = fetch_with(
            &backoff,
            || clock.get(),
            || {
                calls.set(calls.get() + 1);
                Ok(SAMPLE_JSON.into())
            },
            || cleared.set(cleared.get() + 1),
        )
        .unwrap();
        assert_eq!(ok.groups.len(), 3);
        assert_eq!(calls.get(), 2);
        assert!(!backoff.lock().unwrap().is_active(clock.get()));
    }

    #[test]
    fn classifies_signed_out_from_stderr_and_log() {
        assert!(is_signed_out_text(
            "Authentication required. Please visit the URL to log in:\nhttps://example.com"
        ));
        assert!(is_signed_out_text(
            "I1127 12:49:37.202645 1 printmode.go:416] Print mode: silent auth failed"
        ));
        assert!(is_signed_out_text(
            "I1127 12:49:37.202645 1 printmode.go:433] Print mode: triggering interactive OAuth"
        ));
        assert!(!is_signed_out_text(
            "I1127 12:50:39.740369 1 printmode.go:412] Print mode: silent auth succeeded"
        ));
    }

    #[cfg(windows)]
    #[test]
    fn no_browser_shim_intercepts_rundll32_and_aborts_on_auth_prompt() {
        let shim = no_browser_dir().unwrap();
        let env_owned = child_env(Some(&shim));
        let env_refs: Vec<(&str, &std::ffi::OsStr)> =
            env_owned.iter().map(|(k, v)| (*k, v.as_os_str())).collect();
        let cmd = std::env::var_os("ComSpec").unwrap_or_else(|| "cmd.exe".into());
        // Simulate signed-out agy: resolve rundll32 via PATH, print the auth prompt to stderr,
        // and block waiting for a callback.
        let out = crate::hidden::run(
            &cmd,
            &["/c", "where", "rundll32"],
            &std::env::temp_dir(),
            Duration::from_secs(10),
            &env_refs,
            None,
        )
        .unwrap();
        let first = out.stdout.lines().next().unwrap_or("");
        assert!(
            first.eq_ignore_ascii_case(&shim.join("rundll32.cmd").to_string_lossy()),
            "expected shim first in PATH, got: {first}"
        );

        let started = Instant::now();
        let err = crate::hidden::run(
            &cmd,
            &[
                "/c",
                "rundll32 url.dll,FileProtocolHandler https://example.com & echo Authentication required. Please visit the URL to log in: 1>&2 & ping -n 20 127.0.0.1 >nul",
            ],
            &std::env::temp_dir(),
            Duration::from_secs(15),
            &env_refs,
            Some(AUTH_STDERR_MARKER),
        )
        .unwrap_err();
        assert_eq!(err, RunError::Aborted);
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
