//! Last good reading of slow sources (`Source::remember`), kept in
//! `<local data dir>/agent-usage/cache/<key>.json`. Every copy of the app (the window and the
//! background one) reads it: the card shows at once on launch, and a reading one copy made is
//! reused by the others instead of asking the vendor again. A lease file lets only one copy
//! read a source at a time. Only percentages, reset times, labels and the account name are stored.

use crate::model::{Group, Window};
use crate::providers::{parse_time, read_small};
use chrono::{DateTime, Duration as D, Utc};
use serde_json::{Value, json};
use std::path::PathBuf;
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
pub fn load(key: &str) -> Option<Saved> {
    let text = read_small(&path(key)?)?;
    decode(&serde_json::from_str(&text).ok()?, Utc::now())
}

/// A saved reading: groups, when it was read, and the account it belongs to.
pub type Saved = (Vec<Group>, DateTime<Utc>, Option<String>);

/// Save `groups` for `key`. Written even when unchanged: the time it was read is what tells the
/// other copies the reading is fresh.
pub fn save(key: &str, groups: &[Group], account: Option<&str>) {
    let body = encode(groups, account, Utc::now());
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

/// The right to read a source; the lease file goes away when this is dropped.
pub struct Lease(Option<PathBuf>);

impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(p) = &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Take the lease on `key`, unless another copy holds it. A lease older than `ttl` belongs to a
/// copy that died mid-read and is taken over. Without a data folder there is nothing to share.
pub fn lease(key: &str, ttl: Duration) -> Option<Lease> {
    let Some(p) = path(key).map(|p| p.with_extension("lease")) else {
        return Some(Lease(None));
    };
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    for _ in 0..2 {
        let opened = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&p);
        if opened.is_ok() {
            return Some(Lease(Some(p)));
        }
        let age = std::fs::metadata(&p)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok());
        if age.is_some_and(|a| a < ttl) {
            return None;
        }
        let _ = std::fs::remove_file(&p);
    }
    None
}

fn encode(groups: &[Group], account: Option<&str>, now: DateTime<Utc>) -> Value {
    json!({
        "saved_at": now.to_rfc3339(),
        "account": account,
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

fn decode(v: &Value, now: DateTime<Utc>) -> Option<Saved> {
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
    let account = v.get("account").and_then(Value::as_str).map(str::to_string);
    (!groups.is_empty()).then_some((groups, saved, account))
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
        let (g, saved, account) = decode(&encode(&groups(now), Some("ana"), now), now).unwrap();
        assert_eq!(account.as_deref(), Some("ana"));
        assert_eq!(g[0].name, "Gemini");
        assert!((g[0].windows[0].remaining - 0.67).abs() < 1e-4);
        assert_eq!(g[0].windows[0].length, Some(Duration::from_secs(300 * 60)));
        assert_eq!(saved.timestamp(), now.timestamp());
    }

    #[test]
    fn one_lease_at_a_time() {
        let key = format!("test-lease-{}", std::process::id());
        let first = lease(&key, Duration::from_secs(60)).unwrap();
        assert!(lease(&key, Duration::from_secs(60)).is_none());
        // A lease older than its lifetime is taken over.
        let second = lease(&key, Duration::ZERO).unwrap();
        std::mem::forget(first);
        drop(second);
        assert!(lease(&key, Duration::from_secs(60)).is_some());
    }

    #[test]
    fn stale_readings_are_dropped() {
        let now = Utc::now();
        let v = encode(&groups(now), None, now);
        // Too old.
        assert!(decode(&v, now + D::hours(13)).is_none());
        // A window reset since it was saved.
        assert!(decode(&v, now + D::hours(3)).is_none());
        assert!(decode(&v, now + D::hours(1)).is_some());
    }
}
