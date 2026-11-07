//! Puts `assets/icon.png` on a white disc (so it reads on light and dark backgrounds), writes it to
//! `OUT_DIR/icon-round.png` for the window icon, and embeds a multi-size `.ico` in the Windows
//! executable so Explorer, the taskbar and shortcuts show it too.

use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::imageops::FilterType;
use image::{Rgba, RgbaImage};
use std::path::PathBuf;

const SIZE: u32 = 512;
/// Logo size relative to the disc.
const LOGO: f32 = 0.66;

fn round_icon() -> RgbaImage {
    let src = image::open("assets/icon.png")
        .expect("assets/icon.png")
        .to_rgba8();
    let side = (SIZE as f32 * LOGO).round() as u32;
    let logo = image::imageops::resize(&src, side, side, FilterType::Lanczos3);
    let off = (SIZE - side) / 2;
    let r = SIZE as f32 / 2.0;
    let mut out = RgbaImage::new(SIZE, SIZE);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let dx = x as f32 + 0.5 - r;
            let dy = y as f32 + 0.5 - r;
            // Anti-aliased disc edge.
            let cover = (r - 1.0 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            if cover <= 0.0 {
                continue;
            }
            let mut c = [255.0f32; 3];
            if (off..off + side).contains(&x) && (off..off + side).contains(&y) {
                let p = logo.get_pixel(x - off, y - off);
                let a = p[3] as f32 / 255.0;
                for i in 0..3 {
                    c[i] = c[i] * (1.0 - a) + p[i] as f32 * a;
                }
            }
            out.put_pixel(
                x,
                y,
                Rgba([c[0] as u8, c[1] as u8, c[2] as u8, (cover * 255.0) as u8]),
            );
        }
    }
    out
}

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.png");
    println!("cargo:rerun-if-changed=build.rs");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let icon = round_icon();
    icon.save(out_dir.join("icon-round.png")).unwrap();

    let ico = out_dir.join("icon.ico");
    let frames: Vec<IcoFrame> = [256u32, 64, 48, 32, 24, 16]
        .iter()
        .map(|&s| {
            let img = image::imageops::resize(&icon, s, s, FilterType::Lanczos3);
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
        res.set("ProductName", "Agent Usage");
        res.set("FileDescription", "Agent Usage");
        res.compile().expect("embed icon");
    }
}
