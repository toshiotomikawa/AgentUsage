//! Colors and fonts. Color encodes state only; providers are told apart by their icons.

use crate::model::Status;
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

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
            ok: Color32::from_rgb(0x2B, 0x2B, 0x30),
            warn: Color32::from_rgb(0xE0, 0x8A, 0x1E),
            crit: Color32::from_rgb(0xE5, 0x48, 0x4D),
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
            ok: Color32::from_rgb(0xE4, 0xE4, 0xE7),
            warn: Color32::from_rgb(0xF2, 0xA1, 0x3B),
            crit: Color32::from_rgb(0xFF, 0x5F, 0x64),
            pace: Color32::from_rgb(0xEC, 0xEC, 0xEE),
            dark: true,
        }
    }

    pub fn status(&self, s: Status) -> Color32 {
        match s {
            Status::Ok => self.ok,
            Status::Warning => self.warn,
            Status::Critical | Status::Empty => self.crit,
        }
    }
}

pub const SEMIBOLD: &str = "semibold";

/// Use the system UI font when available (Segoe UI on Windows); fall back to egui's defaults.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let candidates: &[(&str, &[&str])] = &[
        (
            "ui",
            &[
                "C:/Windows/Fonts/segoeui.ttf",
                "/System/Library/Fonts/SFNS.ttf",
                "/usr/share/fonts/truetype/inter/Inter-Regular.ttf",
            ],
        ),
        (
            SEMIBOLD,
            &[
                "C:/Windows/Fonts/seguisb.ttf",
                "/System/Library/Fonts/SFNS.ttf",
                "/usr/share/fonts/truetype/inter/Inter-SemiBold.ttf",
            ],
        ),
    ];
    let mut semibold_loaded = false;
    for (name, paths) in candidates {
        if let Some(bytes) = paths.iter().find_map(|p| std::fs::read(p).ok()) {
            fonts
                .font_data
                .insert((*name).to_string(), Arc::new(FontData::from_owned(bytes)));
            if *name == "ui" {
                fonts
                    .families
                    .entry(FontFamily::Proportional)
                    .or_default()
                    .insert(0, "ui".into());
            } else {
                semibold_loaded = true;
            }
        }
    }
    let mut semibold = Vec::new();
    if semibold_loaded {
        semibold.push(SEMIBOLD.to_string());
    }
    semibold.extend(
        fonts
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    fonts
        .families
        .insert(FontFamily::Name(SEMIBOLD.into()), semibold);
    ctx.set_fonts(fonts);
}
