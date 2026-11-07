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
            remaining: remaining.clamp(0.0, 1.0),
            resets_at: None,
            length: None,
        }
    }

    pub fn resets(mut self, at: Option<DateTime<Utc>>) -> Self {
        self.resets_at = at;
        self
    }

    pub fn length_mins(mut self, mins: u64) -> Self {
        self.length = Some(Duration::from_secs(mins * 60));
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
        if self.remaining <= 0.005 {
            return Status::Empty;
        }
        if self.remaining <= 0.10 {
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

    /// The window that will stop work first: lowest remaining.
    pub fn binding(&self) -> Option<&Window> {
        self.windows()
            .min_by(|a, b| a.remaining.total_cmp(&b.remaining))
    }
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
