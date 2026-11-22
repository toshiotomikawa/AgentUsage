//! "Is there a newer AgentUsage?": one background read of the repo's latest GitHub release and,
//! on Windows, a one-click install: download the release exe, check its SHA-256 against the
//! digest GitHub publishes, swap it in place of the running exe and restart.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const LATEST: &str = "https://api.github.com/repos/toshiotomikawa/AgentUsage/releases/latest";
const RELEASES: &str = "https://github.com/toshiotomikawa/AgentUsage/releases";
const DOWNLOADS: &str = "https://github.com/toshiotomikawa/AgentUsage/releases/download/";
/// Release builds are about 20 MB; anything far larger is not one.
const MAX_EXE: u64 = 64 * 1024 * 1024;
/// Passed to the new exe after an update, so it starts instead of focusing the old window.
pub const UPDATED_ARG: &str = "--updated";

/// The Windows exe of a release, as the release JSON describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    url: String,
    sha256: String,
    size: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Idle,
    Checking,
    UpToDate,
    /// A newer release: its version, its page and, where it can be installed here, its exe.
    Available(String, String, Option<Asset>),
    Installing,
    /// The new exe is in place: restart into it.
    Ready,
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
            if matches!(*s, State::Checking | State::Installing | State::Ready) {
                return;
            }
            *s = State::Checking;
        }
        self.run(latest, done);
    }

    /// Install the available release. Without an installable exe, or if anything fails,
    /// `fallback` gets the release page to open instead.
    pub fn install(
        &self,
        fallback: impl FnOnce(&str) + Send + 'static,
        done: impl FnOnce(&State) + Send + 'static,
    ) {
        let (ver, page, asset) = {
            let Ok(mut s) = self.state.lock() else { return };
            let State::Available(v, p, a) = s.clone() else {
                return;
            };
            if a.is_some() {
                *s = State::Installing;
            }
            (v, p, a)
        };
        let Some(asset) = asset else {
            fallback(&page);
            return;
        };
        self.run(
            move || match install(&asset) {
                Ok(()) => State::Ready,
                Err(_) => {
                    fallback(&page);
                    // Leave the version showing: a second click opens the page again.
                    State::Available(ver, page, None)
                }
            },
            done,
        );
    }

    fn run(
        &self,
        work: impl FnOnce() -> State + Send + 'static,
        done: impl FnOnce(&State) + Send + 'static,
    ) {
        let state = self.state.clone();
        std::thread::spawn(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(work))
                .unwrap_or(State::Failed);
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
    release(&v, env!("CARGO_PKG_VERSION"))
}

/// What the release JSON `v` means for a running `current` version.
fn release(v: &serde_json::Value, current: &str) -> State {
    let Some(tag) = v["tag_name"].as_str() else {
        return State::Failed;
    };
    if !newer(tag, current) {
        return State::UpToDate;
    }
    // Only ever open this repository's own release pages.
    let page = v["html_url"]
        .as_str()
        .filter(|u| u.starts_with(RELEASES))
        .unwrap_or(RELEASES);
    let asset = if cfg!(windows) {
        windows_asset(v)
    } else {
        None
    };
    State::Available(
        tag.trim_start_matches(['v', 'V']).to_string(),
        page.into(),
        asset,
    )
}

/// The release's `*-windows-x64.exe`, if it comes from this repository with a SHA-256 digest.
fn windows_asset(v: &serde_json::Value) -> Option<Asset> {
    v["assets"].as_array()?.iter().find_map(|a| {
        let name = a["name"].as_str()?;
        let url = a["browser_download_url"].as_str()?;
        let sha256 = a["digest"].as_str()?.strip_prefix("sha256:")?;
        let size = a["size"].as_u64()?;
        let hex = sha256.len() == 64 && sha256.bytes().all(|b| b.is_ascii_hexdigit());
        (name.ends_with("-windows-x64.exe")
            && url.starts_with(DOWNLOADS)
            && hex
            && size > 0
            && size <= MAX_EXE)
            .then(|| Asset {
                url: url.into(),
                sha256: sha256.to_ascii_lowercase(),
                size,
            })
    })
}

fn install(asset: &Asset) -> std::io::Result<()> {
    let bytes = download(asset)?;
    swap(&std::env::current_exe()?, &bytes)
}

/// Download the exe and check its size and SHA-256 before anything touches the disk.
fn download(asset: &Asset) -> std::io::Result<Vec<u8>> {
    let err = |m: &str| std::io::Error::other(m.to_string());
    // GitHub serves release files through a redirect to its storage host, so this agent follows
    // a few HTTPS redirects; it carries no token, and the digest below is the trust anchor.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(300)))
        .http_status_as_error(false)
        .https_only(true)
        .max_redirects(5)
        .user_agent(concat!("agent-usage/", env!("CARGO_PKG_VERSION")))
        .build()
        .into();
    let mut resp = agent
        .get(&asset.url)
        .call()
        .map_err(|_| err("download failed"))?;
    if resp.status().as_u16() != 200 {
        return Err(err("download failed"));
    }
    let mut bytes = Vec::with_capacity(asset.size as usize);
    resp.body_mut()
        .with_config()
        .limit(MAX_EXE)
        .reader()
        .take(MAX_EXE + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 != asset.size || hex(&Sha256::digest(&bytes)) != asset.sha256 {
        return Err(err("checksum mismatch"));
    }
    Ok(bytes)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.file_name().unwrap_or_default().to_owned();
    name.push(suffix);
    exe.with_file_name(name)
}

/// Retry a file operation a few times: antivirus scans briefly lock a freshly written exe.
fn retry(mut op: impl FnMut() -> std::io::Result<()>) -> std::io::Result<()> {
    let mut last = Ok(());
    for i in 0..5 {
        if i > 0 {
            std::thread::sleep(Duration::from_millis(200));
        }
        last = op();
        if last.is_ok() {
            break;
        }
    }
    last
}

/// Put `bytes` where `exe` is. Windows lets a running exe be renamed but not overwritten, so the
/// old one moves aside to `<exe>.<pid>.old` (removed on a later start; the pid keeps a copy still
/// running from it, such as `--background`, from blocking the next update) and the new one takes
/// its name, which keeps shortcuts and the single-window check working.
fn swap(exe: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let new = sibling(exe, ".new");
    let old = sibling(exe, &format!(".{}.old", std::process::id()));
    {
        let mut f = std::fs::File::create(&new)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if let Err(e) = retry(|| std::fs::rename(exe, &old)) {
        let _ = std::fs::remove_file(&new);
        return Err(e);
    }
    if let Err(e) = retry(|| std::fs::rename(&new, exe)) {
        let _ = std::fs::rename(&old, exe);
        let _ = std::fs::remove_file(&new);
        return Err(e);
    }
    Ok(())
}

/// Start the new exe with this run's arguments, minus `drop` plus `add`, and report whether it
/// started.
fn spawn_new(drop: &[&str], add: &[&str]) -> bool {
    let Ok(exe) = std::env::current_exe() else {
        return false;
    };
    let args: Vec<String> = std::env::args()
        .skip(1)
        .filter(|a| a != UPDATED_ARG && !drop.contains(&a.as_str()))
        .chain(add.iter().map(|a| a.to_string()))
        .collect();
    let mut started = false;
    let _ = retry(|| {
        std::process::Command::new(&exe)
            .args(&args)
            .arg(UPDATED_ARG)
            .spawn()
            .map(|_| started = true)
    });
    started
}

impl Checker {
    /// After `Ready`: start the new exe and return true, so the caller quits. If it cannot be
    /// started, keep this window; the new version is already in place for the next start.
    /// `drop` and `add` adjust the arguments, so the new run opens the way this one is now.
    pub fn relaunch(&self, drop: &[&str], add: &[&str]) -> bool {
        if spawn_new(drop, add) {
            return true;
        }
        if let Ok(mut s) = self.state.lock() {
            *s = State::Idle;
        }
        false
    }
}

/// Remove the exes earlier updates left aside; each stays locked until its process has exited.
pub fn clean_up() {
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let (Some(dir), Some(name)) = (exe.parent(), exe.file_name()) else {
        return;
    };
    let prefix = format!("{}.", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let olds: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .is_some_and(|n| n.starts_with(&prefix) && n.ends_with(".old"))
        })
        .collect();
    if olds.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        for _ in 0..40 {
            if olds
                .iter()
                .all(|p| std::fs::remove_file(p).is_ok() || !p.exists())
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    });
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
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(newer("v0.2.0", "0.1.0"));
        assert!(newer("0.1.1", "0.1"));
        assert!(newer("v1.0.0", "0.9.9"));
        assert!(!newer("v0.1.0", "0.1.0"));
        assert!(!newer("v0.1.0-beta", "0.1.0"));
        assert!(!newer("0.0.9", "0.1.0"));
    }

    fn json(url: &str, digest: &str) -> serde_json::Value {
        serde_json::json!({
            "tag_name": "v0.1.1",
            "html_url": "https://github.com/toshiotomikawa/AgentUsage/releases/tag/v0.1.1",
            "assets": [
                {"name": "SHA256SUMS.txt", "size": 100, "digest": format!("sha256:{}", "a".repeat(64)),
                 "browser_download_url": format!("{DOWNLOADS}v0.1.1/SHA256SUMS.txt")},
                {"name": "AgentUsage-v0.1.1-windows-x64.exe", "size": 20, "digest": digest,
                 "browser_download_url": url}
            ]
        })
    }

    #[test]
    fn picks_only_this_repos_exe_with_a_digest() {
        let ok = format!("{DOWNLOADS}v0.1.1/AgentUsage-v0.1.1-windows-x64.exe");
        let d = format!("sha256:{}", "AB".repeat(32));
        let a = windows_asset(&json(&ok, &d)).unwrap();
        assert_eq!(a.url, ok);
        assert_eq!(a.sha256, "ab".repeat(32));
        assert!(windows_asset(&json("https://evil.example/x-windows-x64.exe", &d)).is_none());
        assert!(windows_asset(&json(&ok, "sha1:abc")).is_none());
        assert!(windows_asset(&json(&ok, "sha256:xyz")).is_none());
    }

    #[test]
    fn release_reports_up_to_date_or_available() {
        let v = json(&format!("{DOWNLOADS}v0.1.1/a-windows-x64.exe"), "sha256:00");
        assert_eq!(release(&v, "0.1.1"), State::UpToDate);
        let State::Available(ver, page, _) = release(&v, "0.1.0") else {
            panic!("expected Available");
        };
        assert_eq!(ver, "0.1.1");
        assert!(page.starts_with(RELEASES));
    }

    #[test]
    fn swap_replaces_and_keeps_the_old_aside() {
        let dir = std::env::temp_dir().join(format!("au-swap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("agent-usage.exe");
        std::fs::write(&exe, b"old").unwrap();
        swap(&exe, b"new").unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        let old = sibling(&exe, &format!(".{}.old", std::process::id()));
        assert_eq!(std::fs::read(old).unwrap(), b"old");
        assert!(!sibling(&exe, ".new").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Live: reads the real latest release as if this were 0.0.0, downloads and verifies its exe,
    /// and swaps it onto a stand-in file. Run with `cargo test -- --ignored live_update`.
    #[test]
    #[ignore]
    fn live_update_from_github() {
        let text = crate::providers::agent()
            .get(LATEST)
            .header("Accept", "application/vnd.github+json")
            .call()
            .unwrap()
            .body_mut()
            .read_to_string()
            .unwrap();
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        let State::Available(_, _, Some(asset)) = release(&v, "0.0.0") else {
            panic!("no installable asset");
        };
        let bytes = download(&asset).unwrap();
        let dir = std::env::temp_dir().join(format!("au-live-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("agent-usage.exe");
        std::fs::write(&exe, b"old").unwrap();
        swap(&exe, &bytes).unwrap();
        assert_eq!(
            hex(&Sha256::digest(std::fs::read(&exe).unwrap())),
            asset.sha256
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hex_is_lowercase() {
        assert_eq!(hex(&[0x0a, 0xff]), "0aff");
    }
}
