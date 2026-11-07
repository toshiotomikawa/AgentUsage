//! Turns `assets/icon.png` into a multi-size `.ico` and embeds it in the Windows executable,
//! so Explorer, the taskbar and shortcuts all show the app icon.

use image::codecs::ico::{IcoEncoder, IcoFrame};
use image::imageops::FilterType;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.png");
    let out = PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("icon.ico");
    let src = image::open("assets/icon.png")
        .expect("assets/icon.png")
        .to_rgba8();
    let frames: Vec<IcoFrame> = [256u32, 64, 48, 32, 24, 16]
        .iter()
        .map(|&s| {
            let img = image::imageops::resize(&src, s, s, FilterType::Lanczos3);
            IcoFrame::as_png(img.as_raw(), s, s, image::ExtendedColorType::Rgba8).unwrap()
        })
        .collect();
    let file = std::fs::File::create(&out).unwrap();
    IcoEncoder::new(file).encode_images(&frames).unwrap();

    #[cfg(windows)]
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon(out.to_str().unwrap());
        res.set("ProductName", "Agent Usage");
        res.set("FileDescription", "Agent Usage");
        res.compile().expect("embed icon");
    }
}
