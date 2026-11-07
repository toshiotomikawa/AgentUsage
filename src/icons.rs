//! Provider and app icons, decoded once into GPU textures.

use crate::model::Provider;
use eframe::egui::{self, Color32, ColorImage, Rect, TextureHandle, TextureOptions, Ui};
use image::imageops::FilterType;

pub struct Icons {
    pub app: TextureHandle,
    antigravity: TextureHandle,
    claude: TextureHandle,
    codex: TextureHandle,
    cursor: TextureHandle,
    opencode: TextureHandle,
}

pub const APP_ICON: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/icon-round.png"));

fn texture(ctx: &egui::Context, name: &str, bytes: &[u8], size: u32) -> TextureHandle {
    let rgba = image::load_from_memory(bytes)
        .map(|i| i.resize_exact(size, size, FilterType::Triangle).to_rgba8())
        .unwrap_or_else(|_| image::RgbaImage::new(size, size));
    let img = ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    );
    ctx.load_texture(name, img, TextureOptions::LINEAR)
}

impl Icons {
    pub fn load(ctx: &egui::Context) -> Self {
        let t = |name: &str, bytes: &[u8]| texture(ctx, name, bytes, 96);
        Self {
            app: t("app", APP_ICON),
            antigravity: t(
                "antigravity",
                include_bytes!("../assets/providers/antigravity.webp"),
            ),
            claude: t("claude", include_bytes!("../assets/providers/claude.webp")),
            codex: t("codex", include_bytes!("../assets/providers/chatgpt.webp")),
            // Cursor and Grok share one card; Grok's mark stands for the pair.
            cursor: t("cursor", include_bytes!("../assets/providers/grok.webp")),
            opencode: t(
                "opencode",
                include_bytes!("../assets/providers/opencode.webp"),
            ),
        }
    }

    pub fn provider(&self, p: Provider) -> &TextureHandle {
        match p {
            Provider::Antigravity => &self.antigravity,
            Provider::Claude => &self.claude,
            Provider::Codex => &self.codex,
            Provider::Cursor => &self.cursor,
            Provider::OpenCode => &self.opencode,
        }
    }
}

/// Paint a square icon tile with rounded corners, faded by `alpha`.
pub fn paint(ui: &Ui, tex: &TextureHandle, rect: Rect, radius: f32, alpha: f32) {
    egui::Image::new((tex.id(), rect.size()))
        .corner_radius(radius)
        .tint(Color32::WHITE.gamma_multiply(alpha))
        .paint_at(ui, rect);
}
