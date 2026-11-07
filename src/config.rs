//! Optional user config at `<config dir>/agent-usage/config.toml`.
//!
//! ```toml
//! # Extra Claude Code config folders (one per account). ~/.claude and ~/.claude-* are found automatically.
//! claude_dirs = ["C:/Users/me/.claude-work"]
//! # Antigravity is read with `agy -p /usage` (no model turn). Set to false to skip it.
//! antigravity = false
//! # Seconds between automatic refreshes.
//! refresh_secs = 60
//! ```

use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
#[serde(default)]
pub struct Config {
    pub claude_dirs: Vec<PathBuf>,
    pub antigravity: bool,
    pub refresh_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            claude_dirs: Vec::new(),
            antigravity: true,
            refresh_secs: 60,
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
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
                eprintln!("config inválida em {}: {e}", path.display());
                Self::default()
            }),
            Err(_) => Self::default(),
        }
    }
}
