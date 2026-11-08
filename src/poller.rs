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
            .map(|s| Account {
                key: s.key.clone(),
                provider: s.provider,
                title: None,
                account: s.account.clone(),
                reading: Reading::Loading,
                fetched_at: None,
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
            if let (Some(min), Some(at)) = (src.min_interval, last_run.get(&src.key))
                && at.elapsed() < Duration::from_secs(min)
                && (!manual || has_data())
            {
                continue;
            }
            last_run.insert(src.key.clone(), Instant::now());
            spawn_fetch(src.clone(), shared.clone(), ctx.clone());
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

fn spawn_fetch(src: Source, shared: Arc<Mutex<Shared>>, ctx: eframe::egui::Context) {
    shared.lock().unwrap().in_flight += 1;
    std::thread::spawn(move || {
        let result = (src.fetch)();
        let mut s = shared.lock().unwrap();
        s.in_flight = s.in_flight.saturating_sub(1);
        if let Some(acc) = s.accounts.iter_mut().find(|a| a.key == src.key) {
            match result {
                Ok(f) => {
                    acc.reading = Reading::Ok(f.groups);
                    acc.account = f.account.or_else(|| src.account.clone());
                    acc.title = f.title;
                    acc.fetched_at = Some(Utc::now());
                }
                Err(e) => {
                    // Keep the last good reading through a transient failure.
                    if !matches!(acc.reading, Reading::Ok(_)) {
                        if src.optional && !crate::providers::is_transient(&e) {
                            let key = src.key.clone();
                            s.accounts.retain(|a| a.key != key);
                        } else {
                            acc.reading = Reading::Unavailable(e);
                        }
                    }
                }
            }
        }
        drop(s);
        ctx.request_repaint();
    });
}
