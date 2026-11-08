//! Colors and fonts. Color encodes state only; providers are told apart by their icons.

use crate::model::Status;
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

/// On pace.
const OK: Color32 = Color32::from_rgb(0x00, 0x90, 0x51);
/// Needs attention (low, or used faster than the window elapses).
const WARN: Color32 = Color32::from_rgb(0xC6, 0x90, 0x35);
/// Below 10% left.
const CRIT: Color32 = Color32::from_rgb(0x77, 0x00, 0x01);
/// Depleted until the window resets.
const EMPTY: Color32 = Color32::from_rgb(0x8F, 0x93, 0x94);
/// Window close button on hover.
const CLOSE: Color32 = Color32::from_rgb(0xC4, 0x2B, 0x1C);

#[derive(Clone, Copy)]
pub struct Palette {
    pub bg: Color32,
    pub card: Color32,
    pub card_hover: Color32,
    pub border: Color32,
    pub border_hover: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub faint: Color32,
    pub track: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub crit: Color32,
    pub empty: Color32,
    pub close_hover: Color32,
    pub pace: Color32,
    pub dark: bool,
}

impl Palette {
    pub fn light() -> Self {
        Self {
            bg: Color32::from_rgb(0xF5, 0xF5, 0xF3),
            card: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            card_hover: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            border: Color32::from_rgb(0xE7, 0xE7, 0xE3),
            border_hover: Color32::from_rgb(0xD2, 0xD2, 0xCC),
            text: Color32::from_rgb(0x17, 0x17, 0x1A),
            muted: Color32::from_rgb(0x82, 0x82, 0x7E),
            faint: Color32::from_rgb(0xB4, 0xB4, 0xAF),
            track: Color32::from_rgb(0xEE, 0xEE, 0xEA),
            ok: OK,
            warn: WARN,
            crit: CRIT,
            empty: EMPTY,
            close_hover: CLOSE,
            pace: Color32::from_rgb(0x17, 0x17, 0x1A),
            dark: false,
        }
    }

    pub fn dark() -> Self {
        Self {
            bg: Color32::from_rgb(0x0F, 0x0F, 0x11),
            card: Color32::from_rgb(0x17, 0x17, 0x1A),
            card_hover: Color32::from_rgb(0x1B, 0x1B, 0x1F),
            border: Color32::from_rgb(0x24, 0x24, 0x29),
            border_hover: Color32::from_rgb(0x34, 0x34, 0x3A),
            text: Color32::from_rgb(0xEC, 0xEC, 0xEE),
            muted: Color32::from_rgb(0x8C, 0x8C, 0x93),
            faint: Color32::from_rgb(0x55, 0x55, 0x5C),
            track: Color32::from_rgb(0x26, 0x26, 0x2B),
            ok: OK,
            warn: WARN,
            crit: CRIT,
            empty: EMPTY,
            close_hover: CLOSE,
            pace: Color32::from_rgb(0xEC, 0xEC, 0xEE),
            dark: true,
        }
    }

    pub fn status(&self, s: Status) -> Color32 {
        match s {
            Status::Ok => self.ok,
            Status::Warning => self.warn,
            Status::Critical => self.crit,
            Status::Empty => self.empty,
        }
    }
}

pub const SEMIBOLD: &str = "semibold";
/// Fixed-width digits for numbers that animate (percentages, countdowns).
pub const MONO: &str = "mono";
pub const MONO_SEMIBOLD: &str = "mono-semibold";

/// Google Sans for text and Google Sans Mono for numbers, from the system or the user's font
/// folder; Segoe UI / Consolas (or egui's built-ins) when they are not installed.
pub fn install_fonts(ctx: &egui::Context) {
    let mut dirs = vec![std::path::PathBuf::from("C:/Windows/Fonts")];
    if let Some(local) = dirs::data_local_dir() {
        dirs.push(local.join("Microsoft/Windows/Fonts"));
    }
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/share/fonts"));
        dirs.push(home.join("Library/Fonts"));
    }
    let read = |names: &[&str]| {
        names
            .iter()
            .flat_map(|n| dirs.iter().map(move |d| d.join(n)))
            .find_map(|p| std::fs::read(p).ok())
    };
    let wanted: [(&str, &[&str]); 4] = [
        ("ui", &["GoogleSans-Regular.ttf", "segoeui.ttf"]),
        (SEMIBOLD, &["GoogleSans-SemiBold.ttf", "seguisb.ttf"]),
        (MONO, &["Google-Sans-Mono-Regular.ttf", "consola.ttf"]),
        (
            MONO_SEMIBOLD,
            &["Google-Sans-Mono-SemiBold.ttf", "consolab.ttf"],
        ),
    ];
    let mut fonts = FontDefinitions::default();
    let base = fonts
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    for (name, files) in wanted {
        let mut family = Vec::new();
        if let Some(bytes) = read(files) {
            fonts
                .font_data
                .insert(name.to_string(), Arc::new(FontData::from_owned(bytes)));
            family.push(name.to_string());
        }
        family.extend(base.iter().cloned());
        if name == "ui" {
            fonts.families.insert(FontFamily::Proportional, family);
        } else {
            fonts.families.insert(FontFamily::Name(name.into()), family);
        }
    }
    ctx.set_fonts(fonts);
}
