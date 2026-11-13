//! Background polling. Each source is fetched on its own thread so a slow vendor never delays
//! the others; results land in shared state and wake the UI.

use crate::model::{Account, Reading};
use crate::providers::Source;
use chrono::Utc;
use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Default)]
pub struct Shared {
    pub accounts: Vec<Account>,
    pub in_flight: usize,
    /// Sources with a read in progress; a source never runs twice at once.
    pub fetching: std::collections::HashSet<String>,
    pub last_refresh: Option<chrono::DateTime<Utc>>,
}

pub struct Poller {
    pub shared: Arc<Mutex<Shared>>,
    trigger: Sender<()>,
}

impl Poller {
    pub fn start(sources: Vec<Source>, every: Duration, ctx: eframe::egui::Context) -> Self {
        let accounts = sources
            .iter()
            .map(|s| {
                // A slow source starts from its last saved reading until the fresh one lands.
                let saved = s.remember.then(|| crate::cache::load(&s.key)).flatten();
                let (reading, fetched_at, account) = match saved {
                    Some((groups, at, who)) => (Reading::Ok(groups), Some(at), who),
                    None => (Reading::Loading, None, None),
                };
                Account {
                    key: s.key.clone(),
                    provider: s.provider,
                    title: None,
                    account: account.or_else(|| s.account.clone()),
                    reading,
                    fetched_at,
                }
            })
            .collect();
        let shared = Arc::new(Mutex::new(Shared {
            accounts,
            ..Default::default()
        }));
        let (trigger, rx) = mpsc::channel();
        let s2 = shared.clone();
        std::thread::Builder::new()
            .name("poller".into())
            .spawn(move || run(sources, every, rx, s2, ctx))
            .expect("spawn poller");
        Self { shared, trigger }
    }

    pub fn refresh(&self) {
        let _ = self.trigger.send(());
    }
}

fn run(
    sources: Vec<Source>,
    every: Duration,
    rx: Receiver<()>,
    shared: Arc<Mutex<Shared>>,
    ctx: eframe::egui::Context,
) {
    let mut last_run: HashMap<String, Instant> = HashMap::new();
    let mut manual = false;
    loop {
        {
            let mut s = shared.lock().unwrap();
            s.last_refresh = Some(Utc::now());
        }
        for src in &sources {
            // A manual refresh skips the minimum interval for accounts still without data
            // (e.g. right after signing in again); the providers' own backoff still applies.
            let has_data = || {
                shared
                    .lock()
                    .unwrap()
                    .accounts
                    .iter()
                    .any(|a| a.key == src.key && matches!(a.reading, Reading::Ok(_)))
            };
            // A window whose reset time has just passed is read again at once, instead of
            // showing the old numbers until the minimum interval runs out.
            let reset_passed = || {
                let now = Utc::now();
                shared
                    .lock()
                    .unwrap()
                    .accounts
                    .iter()
                    .any(|a| a.key == src.key && just_reset(a, now))
            };
            if let (Some(min), Some(at)) = (src.min_interval, last_run.get(&src.key))
                && at.elapsed() < Duration::from_secs(min)
                && (!manual || has_data())
                && !reset_passed()
            {
                continue;
            }
            if spawn_fetch(src.clone(), shared.clone(), ctx.clone()) {
                last_run.insert(src.key.clone(), Instant::now());
            }
        }
        ctx.request_repaint();
        // Wait for the interval or a manual refresh.
        match rx.recv_timeout(every) {
            Ok(()) => manual = true,
            Err(mpsc::RecvTimeoutError::Timeout) => manual = false,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        // Collapse a burst of clicks into one refresh.
        while rx.try_recv().is_ok() {}
    }
}

/// How long after a reset the old reading still counts as stale: the vendor may take a few
/// seconds to report the new window, so a read that lands too early gets more tries.
const RESET_GRACE_SECS: i64 = 30;

/// Whether one of the account's windows reset after its last reading, within the grace period.
fn just_reset(a: &Account, now: chrono::DateTime<Utc>) -> bool {
    let (Reading::Ok(groups), Some(fetched)) = (&a.reading, a.fetched_at) else {
        return false;
    };
    groups
        .iter()
        .flat_map(|g| &g.windows)
        .filter_map(|w| w.resets_at)
        .any(|at| at <= now && fetched < at + chrono::Duration::seconds(RESET_GRACE_SECS))
}

/// Start one read of `src` unless one is already running. Returns whether it started.
fn spawn_fetch(src: Source, shared: Arc<Mutex<Shared>>, ctx: eframe::egui::Context) -> bool {
    {
        let mut s = shared.lock().unwrap();
        if !s.fetching.insert(src.key.clone()) {
            return false;
        }
        s.in_flight += 1;
    }
    let started = std::thread::Builder::new()
        .name(format!("fetch-{}", src.key))
        .spawn({
            let shared = shared.clone();
            let src = src.clone();
            move || {
                let result =
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (src.fetch)()))
                        .unwrap_or_else(|_| Err("resposta inesperada".into()));
                if src.remember
                    && let Ok(f) = &result
                {
                    crate::cache::save(&src.key, &f.groups, f.account.as_deref());
                }
                let mut s = shared.lock().unwrap();
                s.fetching.remove(&src.key);
                s.in_flight = s.in_flight.saturating_sub(1);
                apply(&mut s, &src, result);
                drop(s);
                ctx.request_repaint();
            }
        });
    if started.is_err() {
        let mut s = shared.lock().unwrap();
        s.fetching.remove(&src.key);
        s.in_flight = s.in_flight.saturating_sub(1);
        return false;
    }
    true
}

fn apply(s: &mut Shared, src: &Source, result: Result<crate::providers::Fetched, String>) {
    let pos = s.accounts.iter().position(|a| a.key == src.key);
    match result {
        Ok(f) => {
            let acc = match pos {
                Some(i) => &mut s.accounts[i],
                // An optional account hidden earlier comes back once it reads again.
                None => {
                    s.accounts.push(Account {
                        key: src.key.clone(),
                        provider: src.provider,
                        title: None,
                        account: None,
                        reading: Reading::Loading,
                        fetched_at: None,
                    });
                    s.accounts.last_mut().unwrap()
                }
            };
            acc.reading = Reading::Ok(f.groups);
            acc.account = f.account.or_else(|| src.account.clone());
            acc.title = f.title;
            acc.fetched_at = Some(Utc::now());
        }
        Err(e) => {
            let Some(i) = pos else { return };
            let transient = crate::providers::is_transient(&e);
            let acc = &mut s.accounts[i];
            if transient && matches!(acc.reading, Reading::Ok(_)) {
                // Keep the last good reading through a rate limit or a network blip.
            } else if e == crate::providers::INACTIVE || (src.optional && !transient) {
                s.accounts.remove(i);
            } else {
                // A lasting failure (expired login, signed out) replaces old numbers, which
                // would otherwise pass for current ones.
                acc.reading = Reading::Unavailable(e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Group, Provider, Window};
    use crate::providers::Fetched;

    fn source(optional: bool) -> Source {
        Source {
            key: "k".into(),
            provider: Provider::Claude,
            fetch: Arc::new(|| Err("x".into())),
            min_interval: None,
            optional,
            remember: false,
            account: Some("~/.claude".into()),
        }
    }

    fn ok() -> Result<Fetched, String> {
        Ok(Fetched {
            groups: vec![Group {
                name: String::new(),
                windows: vec![Window::new("5 horas", 0.5)],
            }],
            account: None,
            title: None,
        })
    }

    fn shared_with(src: &Source) -> Shared {
        let mut s = Shared::default();
        s.accounts.push(Account {
            key: src.key.clone(),
            provider: src.provider,
            title: None,
            account: None,
            reading: Reading::Loading,
            fetched_at: None,
        });
        s
    }

    #[test]
    fn lasting_failure_replaces_old_numbers() {
        let src = source(false);
        let mut s = shared_with(&src);
        apply(&mut s, &src, ok());
        apply(&mut s, &src, Err(crate::providers::status_error(429)));
        assert!(matches!(s.accounts[0].reading, Reading::Ok(_)));
        apply(
            &mut s,
            &src,
            Err("login expirado — abra o agente para renovar".into()),
        );
        assert!(matches!(s.accounts[0].reading, Reading::Unavailable(_)));
        assert_eq!(s.accounts[0].account.as_deref(), Some("~/.claude"));
    }

    #[test]
    fn passed_reset_counts_as_stale_for_a_while() {
        let now = Utc::now();
        let d = chrono::Duration::seconds;
        let acc = |fetched, resets| Account {
            key: "k".into(),
            provider: Provider::Claude,
            title: None,
            account: None,
            reading: Reading::Ok(vec![Group {
                name: String::new(),
                windows: vec![Window::new("5 horas", 0.2).resets(Some(resets))],
            }]),
            fetched_at: Some(fetched),
        };
        // Read before the reset, reset now behind us: stale.
        assert!(just_reset(&acc(now - d(300), now - d(10)), now));
        // Reset still ahead: fresh.
        assert!(!just_reset(&acc(now - d(300), now + d(10)), now));
        // Read well after the reset (the vendor kept the old window): stop retrying.
        assert!(!just_reset(&acc(now - d(5), now - d(200)), now));
    }

    #[test]
    fn hidden_optional_account_comes_back() {
        let src = source(true);
        let mut s = shared_with(&src);
        apply(&mut s, &src, Err("não conectado".into()));
        assert!(s.accounts.is_empty());
        apply(&mut s, &src, ok());
        assert_eq!(s.accounts.len(), 1);
    }

    #[test]
    fn inactive_subscription_hides_the_card() {
        let src = source(false);
        let mut s = shared_with(&src);
        apply(&mut s, &src, ok());
        apply(&mut s, &src, Err(crate::providers::INACTIVE.into()));
        assert!(s.accounts.is_empty());
        apply(&mut s, &src, ok());
        assert_eq!(s.accounts.len(), 1);
    }
}
