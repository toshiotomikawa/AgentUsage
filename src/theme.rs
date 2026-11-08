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
    /// Window background, a soft vertical gradient from `bg` to `bg_low`.
    pub bg: Color32,
    pub bg_low: Color32,
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
            // Cool pearl gray, slightly lighter at the top.
            bg: Color32::from_rgb(0xF1, 0xF3, 0xF6),
            bg_low: Color32::from_rgb(0xE6, 0xE9, 0xEE),
            card: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            card_hover: Color32::from_rgb(0xFF, 0xFF, 0xFF),
            border: Color32::from_rgb(0xE1, 0xE4, 0xE9),
            border_hover: Color32::from_rgb(0xCB, 0xD0, 0xD7),
            text: Color32::from_rgb(0x16, 0x18, 0x1C),
            muted: Color32::from_rgb(0x7D, 0x83, 0x8C),
            faint: Color32::from_rgb(0xAF, 0xB5, 0xBD),
            track: Color32::from_rgb(0xEC, 0xEE, 0xF2),
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
            bg: Color32::from_rgb(0x11, 0x12, 0x15),
            bg_low: Color32::from_rgb(0x0B, 0x0C, 0x0E),
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

/// The window background: a quiet top-to-bottom gradient.
pub fn paint_background(p: &egui::Painter, rect: egui::Rect, pal: &Palette) {
    let mut mesh = egui::Mesh::default();
    let i = mesh.vertices.len() as u32;
    mesh.colored_vertex(rect.left_top(), pal.bg);
    mesh.colored_vertex(rect.right_top(), pal.bg);
    mesh.colored_vertex(rect.right_bottom(), pal.bg_low);
    mesh.colored_vertex(rect.left_bottom(), pal.bg_low);
    mesh.add_triangle(i, i + 1, i + 2);
    mesh.add_triangle(i, i + 2, i + 3);
    p.add(mesh);
}

pub const SEMIBOLD: &str = "semibold";

/// Google Sans for everything, from the system or the user's font folder; Segoe UI (or egui's
/// built-ins) when it is not installed.
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
    let wanted: [(&str, &[&str]); 2] = [
        ("ui", &["GoogleSans-Regular.ttf", "segoeui.ttf"]),
        (SEMIBOLD, &["GoogleSans-SemiBold.ttf", "seguisb.ttf"]),
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
