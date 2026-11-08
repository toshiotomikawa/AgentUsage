//! "Is there a newer AgentUsage?": one background read of the repo's latest GitHub release.

use std::sync::{Arc, Mutex};

const LATEST: &str = "https://api.github.com/repos/toshiotomikawa/AgentUsage/releases/latest";
const RELEASES: &str = "https://github.com/toshiotomikawa/AgentUsage/releases";

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Idle,
    Checking,
    UpToDate,
    /// A newer release: its version and page.
    Available(String, String),
    Failed,
}

#[derive(Clone)]
pub struct Checker {
    state: Arc<Mutex<State>>,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(State::Idle)),
        }
    }

    pub fn state(&self) -> State {
        self.state
            .lock()
            .map(|s| s.clone())
            .unwrap_or(State::Failed)
    }

    /// Start a check unless one is running; `done` runs on the result (to repaint).
    pub fn check(&self, done: impl FnOnce(&State) + Send + 'static) {
        {
            let Ok(mut s) = self.state.lock() else { return };
            if *s == State::Checking {
                return;
            }
            *s = State::Checking;
        }
        let state = self.state.clone();
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(latest).unwrap_or(State::Failed);
            if let Ok(mut s) = state.lock() {
                *s = r.clone();
            }
            done(&r);
        });
    }
}

fn latest() -> State {
    let agent = crate::providers::agent();
    let Ok(mut resp) = agent
        .get(LATEST)
        .header("Accept", "application/vnd.github+json")
        .call()
    else {
        return State::Failed;
    };
    match resp.status().as_u16() {
        // No release published yet: nothing newer to get.
        404 => return State::UpToDate,
        200 => {}
        _ => return State::Failed,
    }
    let Ok(text) = resp
        .body_mut()
        .with_config()
        .limit(1 << 20)
        .read_to_string()
    else {
        return State::Failed;
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return State::Failed;
    };
    let Some(tag) = v["tag_name"].as_str() else {
        return State::Failed;
    };
    if !newer(tag, env!("CARGO_PKG_VERSION")) {
        return State::UpToDate;
    }
    // Only ever open this repository's own release pages.
    let page = v["html_url"]
        .as_str()
        .filter(|u| u.starts_with(RELEASES))
        .unwrap_or(RELEASES);
    State::Available(tag.trim_start_matches(['v', 'V']).to_string(), page.into())
}

fn parts(v: &str) -> Vec<u64> {
    v.trim()
        .trim_start_matches(['v', 'V'])
        .split(['-', '+'])
        .next()
        .unwrap_or("")
        .split('.')
        .map(|p| p.parse().unwrap_or(0))
        .collect()
}

/// Is release `tag` newer than version `current`?
fn newer(tag: &str, current: &str) -> bool {
    let (mut a, mut b) = (parts(tag), parts(current));
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    a > b
}

#[cfg(test)]
mod tests {
    use super::newer;

    #[test]
    fn compares_versions() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("0.1.1", "0.1"));
        assert!(newer("v1.0.0", "0.9.9"));
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("v0.1.0-beta", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"));
    }
}
