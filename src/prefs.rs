//! Choices made in the app (theme, language), saved to `<config dir>/agent-usage/prefs.json`.

use crate::i18n::Lang;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemeMode {
    pub fn next(self) -> Self {
        match self {
            ThemeMode::System => ThemeMode::Light,
            ThemeMode::Light => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::System,
        }
    }

    pub fn name(self, lang: Lang) -> &'static str {
        match self {
            ThemeMode::System => lang.t("Sistema", "System"),
            ThemeMode::Light => lang.t("Claro", "Light"),
            ThemeMode::Dark => lang.t("Escuro", "Dark"),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Prefs {
    pub theme: ThemeMode,
    pub lang: Lang,
    /// Cards the user hid from the dashboard (card keys).
    pub hidden: Vec<String>,
}

impl Prefs {
    pub fn toggle_card(&mut self, key: &str) {
        if let Some(i) = self.hidden.iter().position(|k| k == key) {
            self.hidden.remove(i);
        } else {
            self.hidden.push(key.to_string());
        }
    }
}

impl Prefs {
    fn path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("agent-usage").join("prefs.json"))
    }

    pub fn load() -> Self {
        Self::path()
            .and_then(|p| crate::providers::read_small(&p))
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }
}
