//! Provider marks (SVG, rasterized by egui at the exact on-screen size so they stay sharp) and
//! the app icon. Single-color marks are stored white and tinted with the text color.

use crate::model::Provider;
use eframe::egui::{
    self, Color32, ColorImage, ImageSource, Rect, TextureHandle, TextureOptions, Ui, load::Bytes,
};

pub const APP_ICON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon-round.png"));

pub struct Icons {
    pub app: TextureHandle,
}

impl Icons {
    pub fn load(ctx: &egui::Context) -> Self {
        egui_extras::install_image_loaders(ctx);
        let rgba = image::load_from_memory(APP_ICON)
            .map(|i| {
                i.resize_exact(64, 64, image::imageops::FilterType::Lanczos3)
                    .to_rgba8()
            })
            .unwrap_or_else(|_| image::RgbaImage::new(64, 64));
        let img = ColorImage::from_rgba_unmultiplied(
            [rgba.width() as usize, rgba.height() as usize],
            rgba.as_raw(),
        );
        Self {
            app: ctx.load_texture("app", img, TextureOptions::LINEAR),
        }
    }
}

macro_rules! svg {
    ($name:literal) => {
        ImageSource::Bytes {
            uri: concat!("bytes://", $name, ".svg").into(),
            bytes: Bytes::Static(include_bytes!(concat!(
                "../assets/providers/",
                $name,
                ".svg"
            ))),
        }
    };
}

/// The mark and whether it is single-color (to be tinted).
fn source(p: Provider) -> (ImageSource<'static>, bool) {
    match p {
        Provider::Antigravity => (svg!("antigravity"), false),
        Provider::Claude => (svg!("claude"), false),
        Provider::Codex => (svg!("openai"), true),
        // Cursor and Grok share one card; Grok's mark stands for the pair.
        Provider::Cursor => (svg!("grok"), true),
        Provider::OpenCode => (svg!("opencode"), true),
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

/// Paint a texture with rounded corners, faded by `alpha`.
pub fn paint(ui: &Ui, tex: &TextureHandle, rect: Rect, radius: f32, alpha: f32) {
    egui::Image::new((tex.id(), rect.size()))
        .corner_radius(radius)
        .tint(Color32::WHITE.gamma_multiply(alpha))
        .paint_at(ui, rect);
}
