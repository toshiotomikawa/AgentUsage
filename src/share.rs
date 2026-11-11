//! Read-only snapshot of the readings for a companion phone app.
//!
//! Listens on 127.0.0.1 only (never on a network interface, so Windows asks nothing). To reach it
//! from a phone, publish the port to a private network yourself, e.g.
//! `tailscale serve --bg --http=47321 http://127.0.0.1:47321`. Only percentages, reset times and
//! labels are served; logins and tokens never are.

use crate::model::{Account, Reading};
use crate::poller::Shared;
use crate::prefs::Prefs;
use serde_json::{Value, json};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub fn start(port: u16, shared: Arc<Mutex<Shared>>) {
    if port == 0 {
        return;
    }
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(_) => {
            eprintln!("porta {port} ocupada; o app do celular fica sem dados");
            return;
        }
    };
    let _ = std::thread::Builder::new()
        .name("share".into())
        .spawn(move || {
            for stream in listener.incoming().flatten() {
                let shared = shared.clone();
                let _ = std::thread::Builder::new()
                    .name("share-conn".into())
                    .spawn(move || {
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            serve(stream, &shared)
                        }));
                    });
            }
        });
}

fn serve(mut stream: TcpStream, shared: &Mutex<Shared>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
    let mut buf = [0u8; 4096];
    let mut len = 0;
    while len < buf.len() {
        match stream.read(&mut buf[len..]) {
            Ok(0) | Err(_) => break,
            Ok(n) => len += n,
        }
        if buf[..len].windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf[..len]);
    let line = head.lines().next().unwrap_or("");
    let mut parts = line.split(' ');
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let path = path.split('?').next().unwrap_or("");
    let (status, body) = if method == "GET" && (path == "/usage" || path == "/usage.json") {
        let accounts = shared
            .lock()
            .map(|s| s.accounts.clone())
            .unwrap_or_default();
        (
            "200 OK",
            snapshot(&accounts, &Prefs::load().hidden).to_string(),
        )
    } else {
        ("404 Not Found", "{}".to_string())
    };
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=utf-8\r\nCache-Control: no-store\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(reply.as_bytes());
}

pub fn snapshot(accounts: &[Account], hidden: &[String]) -> Value {
    let en = crate::i18n::Lang::En;
    let cards: Vec<Value> = crate::model::merge_claude(accounts)
        .iter()
        .filter(|a| !hidden.contains(&a.key))
        .map(|a| {
            let (state, message, groups) = match &a.reading {
                Reading::Loading => ("loading", None, Vec::new()),
                Reading::Unavailable(r) => ("unavailable", Some(r.clone()), Vec::new()),
                Reading::Ok(g) => ("ok", None, g.clone()),
            };
            json!({
                "key": a.key,
                "provider": a.provider.name(),
                "title": a.title(),
                "account": a.account,
                "state": state,
                "message": message,
                "message_en": message.as_deref().map(|m| en.data(m)),
                "fetched_at": a.fetched_at.map(|t| t.to_rfc3339()),
                "groups": groups.iter().map(|g| json!({
                    "name": g.name,
                    "name_en": en.data(&g.name),
                    "windows": g.windows.iter().map(|w| json!({
                        "label": w.label,
                        "label_en": en.data(&w.label),
                        "remaining": w.remaining,
                        "resets_at": w.resets_at.map(|t| t.to_rfc3339()),
                        "length_secs": w.length.map(|d| d.as_secs()),
                    })).collect::<Vec<_>>(),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({ "version": 1, "now": chrono::Utc::now().to_rfc3339(), "cards": cards })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_has_no_secrets_and_respects_hidden() {
        let accs = crate::demo::accounts();
        let all = snapshot(&accs, &[]);
        let n = all["cards"].as_array().unwrap().len();
        assert!(n > 0);
        let key = all["cards"][0]["key"].as_str().unwrap().to_string();
        let fewer = snapshot(&accs, &[key]);
        assert_eq!(fewer["cards"].as_array().unwrap().len(), n - 1);
        let text = all.to_string().to_lowercase();
        assert!(!text.contains("token") && !text.contains("credential"));
    }
}
