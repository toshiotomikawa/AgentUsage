//! Provider-neutral quota model. Every provider is normalized to "fraction remaining".

use chrono::{DateTime, Utc};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Provider {
    Antigravity,
    Claude,
    Codex,
    Cursor,
    OpenCode,
}

impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Provider::Antigravity => "Antigravity",
            Provider::Claude => "Claude",
            Provider::Codex => "Codex",
            Provider::Cursor => "Cursor",
            Provider::OpenCode => "OpenCode Go",
        }
    }
}

/// One quota window (5-hour, weekly, monthly, billing cycle...).
#[derive(Clone, Debug)]
pub struct Window {
    /// Short human label, e.g. "5 horas", "Semanal".
    pub label: String,
    /// 0.0 ..= 1.0, fraction of the quota still available.
    pub remaining: f32,
    pub resets_at: Option<DateTime<Utc>>,
    /// Full length of the window; enables the pace marker.
    pub length: Option<Duration>,
}

impl Window {
    pub fn new(label: impl Into<String>, remaining: f32) -> Self {
        Self {
            label: label.into(),
            remaining: if remaining.is_finite() {
                remaining.clamp(0.0, 1.0)
            } else {
                0.0
            },
            resets_at: None,
            length: None,
        }
    }

    pub fn resets(mut self, at: Option<DateTime<Utc>>) -> Self {
        self.resets_at = at;
        self
    }

    pub fn length_mins(mut self, mins: u64) -> Self {
        self.length = Some(Duration::from_secs(mins.saturating_mul(60)));
        self
    }

    /// Fraction of the window still to elapse (1.0 = just reset, 0.0 = about to reset).
    /// At a perfectly even pace, `remaining` would equal this value.
    pub fn time_left_fraction(&self, now: DateTime<Utc>) -> Option<f32> {
        let at = self.resets_at?;
        let len = self.length?.as_secs_f32();
        if len <= 0.0 {
            return None;
        }
        let left = (at - now).num_milliseconds() as f32 / 1000.0;
        Some((left / len).clamp(0.0, 1.0))
    }

    pub fn status(&self, now: DateTime<Utc>) -> Status {
        // Below 0.5% the bar reads "0%", so it is shown as depleted.
        if self.remaining < 0.005 {
            return Status::Empty;
        }
        if self.remaining < 0.10 {
            return Status::Critical;
        }
        let behind_pace = self
            .time_left_fraction(now)
            .is_some_and(|t| t - self.remaining > 0.15);
        if self.remaining <= 0.25 || behind_pace {
            Status::Warning
        } else {
            Status::Ok
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Ok,
    Warning,
    Critical,
    Empty,
}

/// A named set of windows inside an account (Antigravity has several model groups).
#[derive(Clone, Debug)]
pub struct Group {
    /// Empty for providers with a single group.
    pub name: String,
    pub windows: Vec<Window>,
}

#[derive(Clone, Debug)]
pub enum Reading {
    Loading,
    Ok(Vec<Group>),
    Unavailable(String),
}

#[derive(Clone, Debug)]
pub struct Account {
    /// Stable key for animation ids.
    pub key: String,
    pub provider: Provider,
    /// Card title override (e.g. "Cursor & Grok").
    pub title: Option<String>,
    /// Email local part; shown only when the provider has more than one account.
    pub account: Option<String>,
    pub reading: Reading,
    pub fetched_at: Option<DateTime<Utc>>,
}

impl Account {
    pub fn title(&self) -> &str {
        self.title.as_deref().unwrap_or(self.provider.name())
    }

    pub fn windows(&self) -> impl Iterator<Item = &Window> {
        let groups: &[Group] = match &self.reading {
            Reading::Ok(g) => g,
            _ => &[],
        };
        groups.iter().flat_map(|g| g.windows.iter())
    }
}

/// How long a window lasts, for the concentric rings: outer, middle and inner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Period {
    Hours,
    Week,
    Month,
}

impl Window {
    pub fn period(&self) -> Period {
        match self.length.map(|d| d.as_secs()) {
            Some(s) if s <= 24 * 3600 => Period::Hours,
            Some(s) if s <= 8 * 24 * 3600 => Period::Week,
            _ => Period::Month,
        }
    }
}

impl Account {
    /// The tightest window of each period (hours, week, month), if the account has one.
    pub fn by_period(&self) -> [Option<&Window>; 3] {
        let mut out: [Option<&Window>; 3] = [None; 3];
        for w in self.windows() {
            let i = w.period() as usize;
            if out[i].is_none_or(|b| w.remaining < b.remaining) {
                out[i] = Some(w);
            }
        }
        out
    }
}

/// All Claude accounts as one card, one group per account (alphabetical). Folders signed in to
/// the same account show once, with the most recent reading.
pub fn merge_claude(accounts: &[Account]) -> Vec<Account> {
    let (claude, mut out): (Vec<&Account>, Vec<&Account>) = accounts
        .iter()
        .partition(|a| a.provider == Provider::Claude);
    let mut out: Vec<Account> = out.drain(..).cloned().collect();
    if claude.is_empty() {
        return out;
    }
    let mut ok: Vec<(String, &Account, &Vec<Group>)> = Vec::new();
    for a in &claude {
        if let Reading::Ok(groups) = &a.reading {
            let name = a.account.clone().unwrap_or_default();
            match ok.iter_mut().find(|(n, _, _)| *n == name) {
                Some(slot) if slot.1.fetched_at < a.fetched_at => *slot = (name, a, groups),
                Some(_) => {}
                None => ok.push((name, a, groups)),
            }
        }
    }
    ok.sort_by_key(|x| x.0.to_lowercase());
    let reading = if !ok.is_empty() {
        let single = ok.len() == 1;
        Reading::Ok(
            ok.iter()
                .map(|(name, _, groups)| Group {
                    name: if single { String::new() } else { name.clone() },
                    windows: groups.iter().flat_map(|g| g.windows.clone()).collect(),
                })
                .collect(),
        )
    } else if claude.iter().any(|a| matches!(a.reading, Reading::Loading)) {
        Reading::Loading
    } else {
        claude
            .iter()
            .find_map(|a| match &a.reading {
                Reading::Unavailable(r) => Some(Reading::Unavailable(r.clone())),
                _ => None,
            })
            .unwrap_or(Reading::Loading)
    };
    out.push(Account {
        key: "claude".into(),
        provider: Provider::Claude,
        title: None,
        account: None,
        reading,
        fetched_at: ok.iter().filter_map(|(_, a, _)| a.fetched_at).max(),
    });
    out
}

pub fn email_local_part(email: &str) -> String {
    email.split('@').next().unwrap_or(email).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as D;

    #[test]
    fn pace_and_status() {
        let now = Utc::now();
        // Half the window left but only 20% quota left: behind pace.
        let w = Window::new("Semanal", 0.20)
            .resets(Some(now + D::minutes(5040)))
            .length_mins(10_080);
        assert!((w.time_left_fraction(now).unwrap() - 0.5).abs() < 0.01);
        assert_eq!(w.status(now), Status::Warning);
        assert_eq!(Window::new("x", 0.0).status(now), Status::Empty);
        assert_eq!(Window::new("x", 0.08).status(now), Status::Critical);
        assert_eq!(Window::new("x", 0.9).status(now), Status::Ok);
    }

    #[test]
    fn local_part() {
        assert_eq!(email_local_part("someone@example.com"), "someone");
    }
}

#[cfg(test)]
mod limit_tests {
    use super::*;

    fn claude(key: &str, name: &str, rem: f32, at: i64) -> Account {
        Account {
            key: key.into(),
            provider: Provider::Claude,
            title: None,
            account: Some(name.into()),
            reading: Reading::Ok(vec![Group {
                name: String::new(),
                windows: vec![Window::new("5 horas", rem).length_mins(300)],
            }]),
            fetched_at: DateTime::from_timestamp(at, 0),
        }
    }

    #[test]
    fn claude_accounts_merge_into_one_card() {
        let accs = vec![
            claude("c0", "b", 0.5, 10),
            claude("c1", "a", 0.9, 10),
            claude("c2", "b", 0.4, 20),
        ];
        let out = merge_claude(&accs);
        assert_eq!(out.len(), 1);
        let Reading::Ok(groups) = &out[0].reading else {
            panic!()
        };
        let names: Vec<_> = groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        // The duplicate keeps its most recent reading.
        assert!((groups[1].windows[0].remaining - 0.4).abs() < 1e-4);
        let rings = out[0].by_period();
        assert!((rings[0].unwrap().remaining - 0.4).abs() < 1e-4);
        assert!(rings[1].is_none() && rings[2].is_none());
    }

    #[test]
    fn state_limits() {
        let now = Utc::now();
        assert_eq!(Window::new("x", f32::NAN).remaining, 0.0);
        assert_eq!(Window::new("x", 0.10).status(now), Status::Warning);
        assert_eq!(Window::new("x", 0.099).status(now), Status::Critical);
        assert_eq!(Window::new("x", 0.004).status(now), Status::Empty);
    }
}
