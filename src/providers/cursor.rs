//! Cursor plan usage: Connect-JSON calls to `api2.cursor.sh` DashboardService with the Cursor CLI
//! sign-in. A "merged" plan is the same included usage Grok draws from, so the card is titled
//! "Cursor & Grok" and shows a single total.

use super::{Fetched, Source, agent, label_for_minutes, num, parse_time, read_json, status_error};
use crate::model::{Group, Provider, Window};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;

const ENDPOINT: &str = "https://api2.cursor.sh";

pub fn source() -> Source {
    Source {
        key: "cursor".into(),
        provider: Provider::Cursor,
        fetch: Arc::new(fetch),
        min_interval: None,
        optional: false,
        account: None,
    }
}

fn auth_file() -> PathBuf {
    if cfg!(windows) {
        dirs::config_dir()
            .unwrap_or_default()
            .join("Cursor")
            .join("auth.json")
    } else if cfg!(target_os = "macos") {
        dirs::home_dir()
            .unwrap_or_default()
            .join(".cursor")
            .join("auth.json")
    } else {
        dirs::config_dir()
            .unwrap_or_default()
            .join("cursor")
            .join("auth.json")
    }
}

fn post(path: &str, token: &str) -> Result<Value, String> {
    let mut resp = agent()
        .post(format!("{ENDPOINT}{path}"))
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .header("Connect-Protocol-Version", "1")
        .send("{}")
        .map_err(|_| "sem conexão".to_string())?;
    if !resp.status().is_success() {
        return Err(status_error(resp.status().as_u16()));
    }
    resp.body_mut()
        .read_json()
        .map_err(|_| "resposta inesperada".to_string())
}

fn fetch() -> Result<Fetched, String> {
    let auth = read_json(&auth_file())?;
    let token = auth
        .get("accessToken")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .ok_or("sem login")?;
    let usage = post("/aiserver.v1.DashboardService/GetCurrentPeriodUsage", token)?;
    let merged = post("/aiserver.v1.DashboardService/GetPlanInfo", token)
        .ok()
        .and_then(|p| p.get("isMerged").and_then(Value::as_bool))
        .unwrap_or(false);

    let start = usage.get("billingCycleStart").and_then(parse_time);
    let end = usage.get("billingCycleEnd").and_then(parse_time);
    let mins = match (start, end) {
        (Some(s), Some(e)) if e > s => Some((e - s).num_minutes() as u64),
        _ => None,
    };
    // No plan block means no included quota to show, not an unused one.
    let plan = usage
        .get("planUsage")
        .filter(|v| v.is_object())
        .ok_or("sem limites informados")?;
    // proto3 JSON omits zeros: a missing percentage on a present plan means 0% used.
    let pct = |k: &str| plan.get(k).and_then(num).unwrap_or(0.0);
    let window = |label: &str, used: f64| {
        let mut w = Window::new(label, 1.0 - used as f32 / 100.0).resets(end);
        if let Some(m) = mins {
            w = w.length_mins(m);
        }
        w
    };
    let cycle = mins.map(label_for_minutes).unwrap_or("Ciclo");
    let has_split = plan.get("autoPercentUsed").is_some() || plan.get("apiPercentUsed").is_some();
    let windows = if merged || !has_split {
        vec![window(cycle, pct("totalPercentUsed"))]
    } else {
        vec![
            window(
                &format!("Auto · {}", cycle.to_lowercase()),
                pct("autoPercentUsed"),
            ),
            window(
                &format!("API · {}", cycle.to_lowercase()),
                pct("apiPercentUsed"),
            ),
        ]
    };
    Ok(Fetched {
        groups: vec![Group {
            name: String::new(),
            windows,
        }],
        account: None,
        // Cursor and Grok are shown together whatever the plan type.
        title: Some("Cursor & Grok".to_string()),
    })
}
