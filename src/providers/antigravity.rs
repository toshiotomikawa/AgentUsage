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

use super::{Fetched, Source, parse_time};
use crate::model::{Group, Provider, Window};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

/// Minimum seconds between `agy -p /usage` runs (each launch spawns a ~165 MB CLI process).
const MIN_INTERVAL_SECS: u64 = 60;

pub fn source() -> Source {
    Source {
        key: "antigravity".into(),
        provider: Provider::Antigravity,
        fetch: Arc::new(fetch),
        min_interval: Some(MIN_INTERVAL_SECS),
        optional: false,
        // agy takes a few seconds to answer: show its last reading at once on launch.
        remember: true,
        account: None,
    }
}

fn fetch() -> Result<Fetched, String> {
    let out = run_usage()?;
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
    Ok(Fetched {
        groups,
        account: None,
        title: None,
    })
}

fn run_usage() -> Result<String, String> {
    // Prefer the installed binary over a `.cmd` shim, which would add a `cmd.exe` in between.
    let exe = dirs::data_local_dir()
        .map(|d| d.join("agy").join("bin").join("agy.exe"))
        .filter(|p| p.is_file())
        .map_or_else(|| "agy".into(), |p| p.into_os_string());
    // agy otherwise writes a new log file under ~/.gemini/antigravity-cli/log on every call,
    // thousands a day at this refresh rate. One file of our own, overwritten each time, instead.
    let log = dirs::data_local_dir()
        .map(|d| d.join("agent-usage").join("cache"))
        .filter(|d| std::fs::create_dir_all(d).is_ok())
        .map(|d| d.join("agy.log").to_string_lossy().into_owned());
    let mut args = vec!["-p", "/usage", "--output-format", "json"];
    if let Some(log) = &log {
        args.extend(["--log-file", log]);
    }
    crate::hidden::run(&exe, &args, &std::env::temp_dir(), Duration::from_secs(45)).map_err(|e| {
        match e.as_str() {
            "not found" => "agy não encontrado".to_string(),
            "timeout" => "agy não respondeu".to_string(),
            _ => "agy falhou".to_string(),
        }
    })
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

    /// Calls the real `agy` (no model turn). Run with `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn live_usage() {
        let f = fetch().unwrap();
        assert!(!f.groups.is_empty());
    }

    #[test]
    fn polls_at_most_once_a_minute() {
        assert_eq!(source().min_interval, Some(60));
    }

    #[test]
    fn parses_usage_command() {
        let v: Value = serde_json::from_str(
            r#"{"status":"SUCCESS","command":{"name":"usage","data":{"groups":[
              {"name":"Gemini Models","buckets":[
                {"window":"weekly","remaining_fraction":0.5,"reset_time":"2026-11-14T21:25:05Z"},
                {"window":"5h","remaining_fraction":0.99,"reset_time":"2026-11-08T02:25:05Z"}]},
              {"name":"Claude and GPT models","buckets":[{"window":"5h","remaining_fraction":1,"reset_time":"1970-01-01T00:00:00Z"}]},
              {"name":"Gemini 4 Argon","buckets":[{"window":"weekly","remaining_fraction":0.98}]}]}}}"#,
        )
        .unwrap();
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
}
