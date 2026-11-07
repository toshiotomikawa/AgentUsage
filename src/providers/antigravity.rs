//! Antigravity (Google) plan quota, read by asking the `agy acp` stdio server for `/usage`.
//! Newer agy versions answer that command themselves with a Markdown "Quota Limits" section:
//!
//! ```text
//! ### Quota Limits
//! - **Gemini Models**
//!   - **5-hour Limit Remaining:** 99% remaining (resets Nov 7, 2026, 23:10 UTC)
//!   - **Weekly Limit Remaining:** 0% remaining (resets Nov 8, 2026, 21:12 UTC)
//! ```
//!
//! Opt-in (`antigravity = true` in the config) and polled at most every 10 minutes.

use super::{Fetched, Source};
use crate::model::{Group, Provider, Window};
use chrono::{NaiveDateTime, TimeZone, Utc};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub fn source() -> Source {
    Source {
        key: "antigravity".into(),
        provider: Provider::Antigravity,
        fetch: Arc::new(fetch),
        min_interval: Some(600),
        optional: false,
    }
}

fn fetch() -> Result<Fetched, String> {
    let answer = ask_usage()?;
    let groups = parse(&answer);
    if groups.is_empty() {
        return Err("esta versão do agy não informa a cota".into());
    }
    Ok(Fetched {
        groups,
        account: None,
        title: None,
    })
}

fn ask_usage() -> Result<String, String> {
    let mut cmd = Command::new("agy");
    cmd.arg("acp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .current_dir(std::env::temp_dir());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let mut child = cmd.spawn().map_err(|_| "agy não encontrado".to_string())?;
    let mut stdin = child.stdin.take().ok_or("agy sem stdin")?;
    let stdout = child.stdout.take().ok_or("agy sem stdout")?;

    let (tx, rx) = mpsc::channel::<Value>();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(v) = serde_json::from_str::<Value>(&line)
                && tx.send(v).is_err()
            {
                break;
            }
        }
    });

    let deadline = Instant::now() + Duration::from_secs(60);
    let mut send = |id: u64, method: &str, params: Value| -> Result<(), String> {
        let msg = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        writeln!(stdin, "{msg}").map_err(|_| "agy fechou".to_string())?;
        stdin.flush().map_err(|_| "agy fechou".to_string())
    };
    let mut text = String::new();
    let wait = |id: u64, text: &mut String| -> Result<Value, String> {
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let msg = rx
                .recv_timeout(left)
                .map_err(|_| "agy não respondeu".to_string())?;
            if msg.get("method").and_then(Value::as_str) == Some("session/update") {
                let u = &msg["params"]["update"];
                if u["sessionUpdate"] == "agent_message_chunk"
                    && let Some(t) = u["content"]["text"].as_str()
                {
                    text.push_str(t);
                }
            } else if msg.get("id").and_then(Value::as_u64) == Some(id)
                && msg.get("method").is_none()
            {
                if let Some(e) = msg.get("error") {
                    return Err(format!("agy: {}", e["message"].as_str().unwrap_or("erro")));
                }
                return Ok(msg["result"].clone());
            }
        }
    };

    let result = (|| {
        send(
            1,
            "initialize",
            json!({"protocolVersion": 1, "clientCapabilities": {}}),
        )?;
        wait(1, &mut text)?;
        let cwd = std::env::temp_dir().to_string_lossy().to_string();
        send(2, "session/new", json!({"cwd": cwd, "mcpServers": []}))?;
        let session = wait(2, &mut text)?;
        let sid = session["sessionId"]
            .as_str()
            .ok_or("agy sem sessão")?
            .to_string();
        send(
            3,
            "session/prompt",
            json!({"sessionId": sid, "prompt": [{"type": "text", "text": "/usage"}]}),
        )?;
        wait(3, &mut text)?;
        Ok(text.clone())
    })();
    let _ = child.kill();
    let _ = child.wait();
    result
}

pub(crate) fn parse(answer: &str) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    let mut in_section = false;
    for line in answer.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            in_section = trimmed
                .trim_start_matches('#')
                .trim()
                .eq_ignore_ascii_case("Quota Limits");
            continue;
        }
        if !in_section {
            continue;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        // Group header: "- **Gemini Models**"
        if !indented && let Some(name) = bold_only(trimmed.trim_start_matches('-').trim()) {
            groups.push(Group {
                name: short_group(&name),
                windows: Vec::new(),
            });
            continue;
        }
        // Bucket: "  - **Weekly Limit Remaining:** 96% remaining (resets Oct 29, 2026, 10:12 UTC)"
        let Some(group) = groups.last_mut() else {
            continue;
        };
        let body = trimmed.trim_start_matches('-').trim();
        let Some(rest) = body.strip_prefix("**") else {
            continue;
        };
        let Some((bucket, after)) = rest.split_once(":**") else {
            continue;
        };
        let after = after.trim();
        let Some((pct, tail)) = after.split_once('%') else {
            continue;
        };
        let Ok(pct) = pct.trim().parse::<f32>() else {
            continue;
        };
        let reset = tail
            .split_once("(resets ")
            .and_then(|(_, r)| r.split_once(')'))
            .and_then(|(r, _)| parse_reset(r));
        let lower = bucket.to_lowercase();
        let (label, mins) = if lower.contains("week") {
            ("Semanal", 10_080)
        } else if lower.contains("5-hour") || lower.contains("5 hour") || lower.contains("5h") {
            ("5 horas", 300)
        } else {
            ("Limite", 0)
        };
        let mut w = Window::new(label, pct / 100.0).resets(reset);
        if mins > 0 {
            w = w.length_mins(mins);
        }
        group.windows.push(w);
    }
    groups.retain(|g| !g.windows.is_empty());
    for g in &mut groups {
        // 5-hour first, then weekly.
        g.windows
            .sort_by_key(|w| w.length.map(|d| d.as_secs()).unwrap_or(u64::MAX));
    }
    groups
}

fn bold_only(s: &str) -> Option<String> {
    let inner = s.strip_prefix("**")?.strip_suffix("**")?;
    (!inner.contains("**")).then(|| inner.trim().to_string())
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

fn parse_reset(s: &str) -> Option<chrono::DateTime<Utc>> {
    let s = s.trim().trim_end_matches("UTC").trim();
    let naive = NaiveDateTime::parse_from_str(s, "%b %d, %Y, %H:%M").ok()?;
    Utc.from_local_datetime(&naive).single()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_quota_section() {
        let text = "Hello\n### Quota Limits\n- **Gemini Models**\n  *(Models within this group: Gemini Flash)*\n  - **Weekly Limit Remaining:** 96% remaining (resets Oct 29, 2026, 10:12 UTC)\n  - **5-hour Limit Remaining:** 40% remaining (resets Oct 22, 2026, 08:00 UTC)\n- **Claude and GPT models**\n  - **Weekly Limit Remaining:** 100% remaining\n### Other\n- **x**\n";
        let g = parse(text);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].name, "Gemini");
        assert_eq!(g[0].windows[0].label, "5 horas");
        assert!((g[0].windows[1].remaining - 0.96).abs() < 1e-4);
        assert!(g[0].windows[1].resets_at.is_some());
        assert_eq!(g[1].name, "Claude & GPT");
    }
}
