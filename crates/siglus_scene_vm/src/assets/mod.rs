pub mod g00;

use anyhow::{bail, Context, Result};
use std::path::Path;

/// A decoded RGBA image.
#[derive(Clone, Debug)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    /// length = width * height * 4
    pub rgba: Vec<u8>,
}

/// Load an image from disk.
///
/// Supported:
/// - .g00 (decoded by our g00 decoder)
/// - .png/.jpg/.bmp (decoded by `image` crate)
///
/// DDS is detected but not decoded in this stage.
pub fn load_image_any(path: &Path, g00_frame_index: usize) -> Result<RgbaImage> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    match ext.as_str() {
        "g00" => {
            let bytes = std::fs::read(path).with_context(|| format!("read {:?}", path))?;
            let decoded =
                g00::decode_g00(&bytes).with_context(|| format!("decode g00 {:?}", path))?;
            if decoded.frames.is_empty() {
                bail!("g00 has no frames: {:?}", path);
            }
            let idx = g00_frame_index.min(decoded.frames.len() - 1);
            Ok(decoded.frames[idx].clone())
        }
        "png" | "jpg" | "jpeg" | "bmp" | "dds" => {
            let img = image::open(path).with_context(|| format!("decode image {:?}", path))?;
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            Ok(RgbaImage {
                width: w,
                height: h,
                rgba: rgba.into_raw(),
            })
        }
        _ => {
            bail!("unsupported image extension: {:?}", path);
        }
    }
}
