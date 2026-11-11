//! Optional user config at `<config dir>/agent-usage/config.toml`.
//!
//! ```toml
//! # Extra Claude Code config folders (one per account). ~/.claude and ~/.claude-* are found automatically.
//! claude_dirs = ["C:/Users/me/.claude-work"]
//! # Antigravity is read with `agy -p /usage` (no model turn). Set to false to skip it.
//! antigravity = false
//! # Seconds between automatic refreshes.
//! refresh_secs = 20
//! # Local port of the read-only snapshot for the phone app (127.0.0.1 only). 0 turns it off.
//! share_port = 47321
//! ```

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub claude_dirs: Vec<PathBuf>,
    pub antigravity: bool,
    pub refresh_secs: u64,
    pub share_port: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            claude_dirs: Vec::new(),
            antigravity: true,
            refresh_secs: 20,
            share_port: 47321,
        }
    }
}

impl Config {
    pub fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("agent-usage").join("config.toml"))
    }

    pub fn load() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match crate::providers::read_small(&path) {
            // The error text can quote the file, so it is not printed.
            Some(text) => toml::from_str(&text).unwrap_or_else(|_| {
                eprintln!("config.toml inválido; usando os padrões");
                Self::default()
            }),
            None => Self::default(),
        }
    }
}
