//! Fictional readings for `--demo`: lets anyone try the UI without signing in to anything.

use crate::model::{Account, Group, Provider, Reading, Window};
use chrono::{Duration, Utc};

fn w(label: &str, remaining: f32, resets_in_mins: i64, len_mins: u64) -> Window {
    Window::new(label, remaining)
        .resets(Some(Utc::now() + Duration::minutes(resets_in_mins)))
        .length_mins(len_mins)
}

fn one(windows: Vec<Window>) -> Reading {
    Reading::Ok(vec![Group {
        name: String::new(),
        windows,
    }])
}

pub fn accounts() -> Vec<Account> {
    let acc = |key: &str, provider, title: Option<&str>, account: Option<&str>, reading| Account {
        key: key.into(),
        provider,
        title: title.map(str::to_string),
        account: account.map(str::to_string),
        reading,
        fetched_at: Some(Utc::now()),
    };
    vec![
        acc(
            "claude-0",
            Provider::Claude,
            None,
            Some("personal"),
            one(vec![
                w("5 horas", 0.97, 282, 300),
                w("Semanal", 0.81, 6 * 1440 + 360, 10_080),
            ]),
        ),
        acc(
            "claude-1",
            Provider::Claude,
            None,
            Some("work"),
            one(vec![
                w("5 horas", 0.42, 150, 300),
                w("Semanal", 0.18, 2 * 1440, 10_080),
            ]),
        ),
        acc(
            "codex",
            Provider::Codex,
            None,
            None,
            one(vec![
                w("5 horas", 1.0, 299, 300),
                w("Semanal", 0.37, 4 * 1440 + 18 * 60, 10_080),
            ]),
        ),
        acc(
            "cursor",
            Provider::Cursor,
            Some("Cursor & Grok"),
            None,
            one(vec![w("Semanal", 0.0, 2 * 1440 + 21 * 60, 10_080)]),
        ),
        acc(
            "opencode",
            Provider::OpenCode,
            None,
            None,
            one(vec![
                w("5 horas", 1.0, 296, 300),
                w("Semanal", 0.53, 26 * 60, 10_080),
                w("Mensal", 0.07, 7 * 1440 + 360, 43_200),
            ]),
        ),
        acc(
            "antigravity",
            Provider::Antigravity,
            None,
            None,
            Reading::Ok(vec![
                Group {
                    name: "Gemini".into(),
                    windows: vec![
                        w("5 horas", 0.99, 297, 300),
                        w("Semanal", 0.66, 23 * 60 + 53, 10_080),
                    ],
                },
                Group {
                    name: "Claude & GPT".into(),
                    windows: vec![
                        w("5 horas", 1.0, 300, 300),
                        w("Semanal", 0.50, 3 * 1440 + 22 * 60, 10_080),
                    ],
                },
                Group {
                    name: "Gemini 4 Argon".into(),
                    windows: vec![w("Semanal", 0.87, 5 * 1440 + 6 * 60, 10_080)],
                },
            ]),
        ),
    ]
}
