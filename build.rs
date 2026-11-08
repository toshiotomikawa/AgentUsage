//! Prepares the app icon from `assets/icon.png` (a dark logo):
//! - `OUT_DIR/icon-mask.png`: the logo alone, white with alpha, tightly framed and centered.
//!   The app tints it (black on light, white on dark).
//! - `OUT_DIR/icon-tile.png` and an embedded `.ico`: the logo on a white squircle for the
//!   taskbar, Start menu, Explorer and shortcuts, where the background color is unknown. Every
//!   `.ico` size is drawn at its own size with supersampling, not shrunk from a big one, so edges
//!   stay smooth and no dark fringe appears around the tile.

use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::imageops::FilterType;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::path::PathBuf;

/// Side of the framed logo maps (high, so the small sizes sample a sharp source).
const SIZE: u32 = 1024;
/// Logo's longer side relative to the tile.
const ON_TILE: f32 = 0.70;
/// Logo's longer side relative to the mask canvas.
const ON_MASK: f32 = 0.96;
/// Squircle exponent: 2 is a circle, higher gets closer to a square.
const SQUIRCLE: f32 = 5.0;
/// Subsamples per pixel side when drawing a tile.
const SUPER: u32 = 8;

/// Coverage of the dark logo, 0..=255, whatever the source background (white or transparent).
fn coverage() -> GrayImage {
    let src = image::open("assets/icon.png")
        .expect("assets/icon.png")
        .to_rgba8();
    GrayImage::from_fn(src.width(), src.height(), |x, y| {
        let p = src.get_pixel(x, y);
        let lum = (0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32) / 255.0;
        Luma([((1.0 - lum) * p[3] as f32).round() as u8])
    })
}

/// Crop to the logo and center its bounding box in a square canvas.
fn framed(cov: &GrayImage, fill: f32) -> GrayImage {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    for (x, y, p) in cov.enumerate_pixels() {
        if p[0] > 127 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
    let crop = image::imageops::crop_imm(cov, x0, y0, bw, bh).to_image();
    let scale = SIZE as f32 * fill / bw.max(bh) as f32;
    let (w, h) = (
        (bw as f32 * scale).round() as u32,
        (bh as f32 * scale).round() as u32,
    );
    let logo = image::imageops::resize(&crop, w, h, FilterType::CatmullRom);
    let mut out = GrayImage::new(SIZE, SIZE);
    image::imageops::overlay(
        &mut out,
        &logo,
        ((SIZE - w) / 2) as i64,
        ((SIZE - h) / 2) as i64,
    );
    out
}

fn mask(cov: &GrayImage) -> RgbaImage {
    let m = framed(cov, ON_MASK);
    let m = image::imageops::resize(&m, 512, 512, FilterType::Triangle);
    RgbaImage::from_fn(512, 512, |x, y| Rgba([255, 255, 255, m.get_pixel(x, y)[0]]))
}

/// Logo coverage (0..=1) at a point of the unit square, bilinear.
fn sample(m: &GrayImage, u: f32, v: f32) -> f32 {
    let x = (u * SIZE as f32 - 0.5).clamp(0.0, (SIZE - 1) as f32);
    let y = (v * SIZE as f32 - 0.5).clamp(0.0, (SIZE - 1) as f32);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(SIZE - 1), (y0 + 1).min(SIZE - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let p = |x, y| m.get_pixel(x, y)[0] as f32 / 255.0;
    let top = p(x0, y0) * (1.0 - fx) + p(x1, y0) * fx;
    let bottom = p(x0, y1) * (1.0 - fx) + p(x1, y1) * fx;
    top * (1.0 - fy) + bottom * fy
}

/// The black logo on a white squircle filling the whole `side`×`side` image.
fn tile(m: &GrayImage, side: u32) -> RgbaImage {
    let n = (side * SUPER) as f32;
    RgbaImage::from_fn(side, side, |x, y| {
        let (mut inside, mut ink) = (0.0f32, 0.0f32);
        for sy in 0..SUPER {
            for sx in 0..SUPER {
                let u = ((x * SUPER + sx) as f32 + 0.5) / n;
                let v = ((y * SUPER + sy) as f32 + 0.5) / n;
                let (dx, dy) = ((2.0 * u - 1.0).abs(), (2.0 * v - 1.0).abs());
                if dx.powf(SQUIRCLE) + dy.powf(SQUIRCLE) <= 1.0 {
                    inside += 1.0;
                    ink += sample(m, u, v);
                }
            }
        }
        if inside == 0.0 {
            return Rgba([255, 255, 255, 0]);
        }
        let shade = (255.0 * (1.0 - ink / inside)).round() as u8;
        let alpha = (255.0 * inside / (SUPER * SUPER) as f32).round() as u8;
        Rgba([shade, shade, shade, alpha])
    })
}

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.png");
    println!("cargo:rerun-if-changed=build.rs");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let cov = coverage();
    mask(&cov).save(out_dir.join("icon-mask.png")).unwrap();
    let logo = framed(&cov, ON_TILE);
    tile(&logo, 256)
        .save(out_dir.join("icon-tile.png"))
        .unwrap();

    let ico = out_dir.join("icon.ico");
    let frames: Vec<IcoFrame> = [256u32, 64, 48, 40, 32, 24, 20, 16]
        .iter()
        .map(|&s| {
            let img = tile(&logo, s);
            IcoFrame::as_png(img.as_raw(), s, s, image::ExtendedColorType::Rgba8).unwrap()
        })
        .collect();
    IcoEncoder::new(std::fs::File::create(&ico).unwrap())
        .encode_images(&frames)
        .unwrap();

    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon(ico.to_str().unwrap());
        res.set("ProductName", "AgentUsage");
        res.set("FileDescription", "AgentUsage");
        res.compile().expect("embed icon");
    }
}
