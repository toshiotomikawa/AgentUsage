//! Provider marks (SVG, rasterized by egui at the exact on-screen size so they stay sharp) and
//! the app logo. Single-color marks are stored white and tinted with the text color.

use crate::model::Provider;
use eframe::egui::{
    self, Color32, ColorImage, ImageSource, Rect, TextureHandle, TextureOptions, Ui, load::Bytes,
};
use image::RgbaImage;
use image::imageops::FilterType;

/// The logo on a white disc (window and taskbar icon).
pub const APP_ICON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon-round.png"));
/// The logo alone, white with alpha, for tinting inside the app.
const APP_MASK: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon-mask.png"));

pub const REPO_URL: &str = "https://github.com/toshiotomikawa/AgentUsage";

pub struct Icons {
    mask: RgbaImage,
    /// Logo texture rasterized for one pixel size, rebuilt when the size changes.
    logo: Option<(u32, TextureHandle)>,
}

impl Icons {
    pub fn load(ctx: &egui::Context) -> Self {
        egui_extras::install_image_loaders(ctx);
        let mask = image::load_from_memory(APP_MASK)
            .map(|i| i.to_rgba8())
            .unwrap_or_else(|_| RgbaImage::new(1, 1));
        Self { mask, logo: None }
    }

    /// Paint the app logo in `rect`, tinted `ink`, sampled for the exact physical size.
    pub fn paint_logo(&mut self, ui: &Ui, rect: Rect, ink: Color32) {
        let px = (rect.width() * ui.ctx().pixels_per_point())
            .round()
            .max(1.0) as u32;
        if self.logo.as_ref().is_none_or(|(s, _)| *s != px) {
            let img = image::imageops::resize(&self.mask, px, px, FilterType::Lanczos3);
            let color = ColorImage::from_rgba_unmultiplied(
                [img.width() as usize, img.height() as usize],
                img.as_raw(),
            );
            let tex = ui
                .ctx()
                .load_texture("app-logo", color, TextureOptions::LINEAR);
            self.logo = Some((px, tex));
        }
        if let Some((_, tex)) = &self.logo {
            egui::Image::new((tex.id(), rect.size()))
                .tint(ink)
                .paint_at(ui, rect);
        }
    }
}

macro_rules! svg {
    ($path:literal) => {
        ImageSource::Bytes {
            uri: concat!("bytes://", $path).into(),
            bytes: Bytes::Static(include_bytes!(concat!("../assets/", $path))),
        }
    };
}

/// The mark and whether it is single-color (to be tinted).
fn source(p: Provider) -> (ImageSource<'static>, bool) {
    match p {
        Provider::Antigravity => (svg!("providers/antigravity.svg"), false),
        Provider::Claude => (svg!("providers/claude.svg"), false),
        Provider::Codex => (svg!("providers/openai.svg"), true),
        // Cursor and Grok share one card; Grok's mark stands for the pair.
        Provider::Cursor => (svg!("providers/grok.svg"), true),
        Provider::OpenCode => (svg!("providers/opencode.svg"), true),
    }
}

/// Paint a provider mark in `rect`, faded by `alpha`; single-color marks take `ink`.
pub fn paint_provider(ui: &Ui, p: Provider, rect: Rect, ink: Color32, alpha: f32) {
    let (src, mono) = source(p);
    let tint = if mono { ink } else { Color32::WHITE };
    egui::Image::new(src)
        .tint(tint.gamma_multiply(alpha))
        .paint_at(ui, rect);
}

/// GitHub mark, tinted `ink`.
pub fn paint_github(ui: &Ui, rect: Rect, ink: Color32) {
    egui::Image::new(svg!("github.svg"))
        .tint(ink)
        .paint_at(ui, rect);
}
