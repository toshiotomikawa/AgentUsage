//! Last good reading of slow sources (`Source::remember`), kept in
//! `<local data dir>/agent-usage/cache/<key>.json` so their card shows at once on launch while
//! the fresh read runs. Only percentages, reset times and labels are stored.

use crate::model::{Group, Window};
use crate::providers::{parse_time, read_small};
use chrono::{DateTime, Duration as D, Utc};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;

/// Older readings are not worth showing, even for a few seconds.
const MAX_AGE_HOURS: i64 = 12;

fn path(key: &str) -> Option<PathBuf> {
    let safe: String = key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .collect();
    dirs::data_local_dir().map(|d| {
        d.join("agent-usage")
            .join("cache")
            .join(format!("{safe}.json"))
    })
}

/// Saved reading of `key`, unless it is too old or a window has reset since (stale numbers).
pub fn load(key: &str) -> Option<(Vec<Group>, DateTime<Utc>)> {
    let text = read_small(&path(key)?)?;
    decode(&serde_json::from_str(&text).ok()?, Utc::now())
}

/// Save `groups` for `key`, skipping the write when nothing changed since the last one.
pub fn save(key: &str, groups: &[Group]) {
    static LAST: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
    let body = encode(groups, Utc::now());
    let groups_only = body["groups"].to_string();
    {
        let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
        let last = last.get_or_insert_with(HashMap::new);
        if last.get(key) == Some(&groups_only) {
            return;
        }
        last.insert(key.to_string(), groups_only);
    }
    let Some(path) = path(key) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Write then rename, so a reader (the window or the background copy) never sees half a file.
    let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
    if std::fs::write(&tmp, body.to_string()).is_ok() && std::fs::rename(&tmp, &path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

fn encode(groups: &[Group], now: DateTime<Utc>) -> Value {
    json!({
        "saved_at": now.to_rfc3339(),
        "groups": groups.iter().map(|g| json!({
            "name": g.name,
            "windows": g.windows.iter().map(|w| json!({
                "label": w.label,
                "remaining": w.remaining,
                "resets_at": w.resets_at.map(|t| t.to_rfc3339()),
                "length_secs": w.length.map(|d| d.as_secs()),
            })).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

fn decode(v: &Value, now: DateTime<Utc>) -> Option<(Vec<Group>, DateTime<Utc>)> {
    let saved = parse_time(v.get("saved_at")?)?;
    if now - saved > D::hours(MAX_AGE_HOURS) || saved > now + D::minutes(5) {
        return None;
    }
    let mut groups = Vec::new();
    for g in v.get("groups")?.as_array()? {
        let mut windows = Vec::new();
        for w in g.get("windows")?.as_array()? {
            let mut win = Window::new(
                w.get("label")?.as_str()?,
                w.get("remaining")?.as_f64()? as f32,
            )
            .resets(w.get("resets_at").and_then(parse_time));
            if win.resets_at.is_some_and(|t| t <= now) {
                return None;
            }
            win.length = w
                .get("length_secs")
                .and_then(Value::as_u64)
                .map(Duration::from_secs);
            windows.push(win);
        }
        groups.push(Group {
            name: g.get("name")?.as_str()?.to_string(),
            windows,
        });
    }
    (!groups.is_empty()).then_some((groups, saved))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groups(now: DateTime<Utc>) -> Vec<Group> {
        vec![Group {
            name: "Gemini".into(),
            windows: vec![
                Window::new("5 horas", 0.67)
                    .resets(Some(now + D::hours(2)))
                    .length_mins(300),
            ],
        }]
    }

    #[test]
    fn round_trip() {
        let now = Utc::now();
        let (g, saved) = decode(&encode(&groups(now), now), now).unwrap();
        assert_eq!(g[0].name, "Gemini");
        assert!((g[0].windows[0].remaining - 0.67).abs() < 1e-4);
        assert_eq!(g[0].windows[0].length, Some(Duration::from_secs(300 * 60)));
        assert_eq!(saved.timestamp(), now.timestamp());
    }

    #[test]
    fn stale_readings_are_dropped() {
        let now = Utc::now();
        let v = encode(&groups(now), now);
        // Too old.
        assert!(decode(&v, now + D::hours(13)).is_none());
        // A window reset since it was saved.
        assert!(decode(&v, now + D::hours(3)).is_none());
        assert!(decode(&v, now + D::hours(1)).is_some());
    }
}
