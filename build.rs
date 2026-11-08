//! Prepares the app icon from `assets/icon.png` (a dark logo):
//! - `OUT_DIR/icon-mask.png`: the logo alone, white with alpha, tightly framed and optically
//!   centered. The app tints it (black on light, white on dark).
//! - `OUT_DIR/icon-round.png` and an embedded `.ico`: the logo on a white disc for the taskbar,
//!   Explorer and shortcuts, where the background color is unknown.

use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::imageops::FilterType;
use image::{GrayImage, Luma, Rgba, RgbaImage};
use std::path::PathBuf;

const SIZE: u32 = 512;
/// Logo height relative to the disc diameter.
const ON_DISC: f32 = 0.70;
/// Logo height relative to the mask canvas.
const ON_MASK: f32 = 0.96;

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

/// Crop to the logo and place it in a square canvas, optically centered: vertically on its
/// bounding box, horizontally halfway between the bounding box and the ink's center of mass.
fn framed(cov: &GrayImage, fill: f32) -> GrayImage {
    let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
    let (mut mass, mut mx) = (0.0f64, 0.0f64);
    for (x, y, p) in cov.enumerate_pixels() {
        if p[0] > 24 {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        mass += p[0] as f64;
        mx += p[0] as f64 * x as f64;
    }
    let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
    let crop = image::imageops::crop_imm(cov, x0, y0, bw, bh).to_image();
    let scale = SIZE as f32 * fill / bw.max(bh) as f32;
    let (w, h) = (
        (bw as f32 * scale).round() as u32,
        (bh as f32 * scale).round() as u32,
    );
    let logo = image::imageops::resize(&crop, w, h, FilterType::Lanczos3);
    let centroid = (mx / mass) as f32 - x0 as f32;
    let optical_x = (bw as f32 / 2.0 + centroid) / 2.0 * scale;
    let ox = (SIZE as f32 / 2.0 - optical_x).round() as i64;
    let oy = ((SIZE - h) / 2) as i64;
    let mut out = GrayImage::new(SIZE, SIZE);
    image::imageops::overlay(&mut out, &logo, ox, oy);
    out
}

fn mask(cov: &GrayImage) -> RgbaImage {
    let m = framed(cov, ON_MASK);
    RgbaImage::from_fn(SIZE, SIZE, |x, y| {
        Rgba([255, 255, 255, m.get_pixel(x, y)[0]])
    })
}

fn round(cov: &GrayImage) -> RgbaImage {
    let m = framed(cov, ON_DISC);
    let r = SIZE as f32 / 2.0;
    RgbaImage::from_fn(SIZE, SIZE, |x, y| {
        let dx = x as f32 + 0.5 - r;
        let dy = y as f32 + 0.5 - r;
        let edge = (r - 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
        let ink = 255 - m.get_pixel(x, y)[0];
        Rgba([ink, ink, ink, (edge * 255.0).round() as u8])
    })
}

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.png");
    println!("cargo:rerun-if-changed=build.rs");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let cov = coverage();
    mask(&cov).save(out_dir.join("icon-mask.png")).unwrap();
    let disc = round(&cov);
    disc.save(out_dir.join("icon-round.png")).unwrap();

    let ico = out_dir.join("icon.ico");
    let frames: Vec<IcoFrame> = [256u32, 64, 48, 40, 32, 24, 20, 16]
        .iter()
        .map(|&s| {
            let img = image::imageops::resize(&disc, s, s, FilterType::Lanczos3);
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
