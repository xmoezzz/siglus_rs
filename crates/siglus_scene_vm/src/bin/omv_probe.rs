use std::fs;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};

fn main() -> Result<()> {
    env_logger::init();
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("usage: omv_probe <path/to/file.omv>"))?;

    let ogg_data = siglus_assets::omv::OmvFile::read_embedded_ogg(&path)
        .or_else(|_| extract_ogg_by_scan(&path))
        .with_context(|| format!("read embedded ogg: {}", path.display()))?;

    let mut tf = siglus_omv_decoder::TheoraFile::open_from_memory(ogg_data)
        .with_context(|| format!("open theora: {}", path.display()))?;
    let info = tf.info();
    println!(
        "theora info: {}x{} fps={} fmt={}",
        info.width, info.height, info.fps, info.fmt
    );

    let (uv_w, uv_h) = yuv_plane_size(info.width, info.height, info.fmt);
    let y_len = (info.width as usize).saturating_mul(info.height as usize);
    let uv_len = uv_w.saturating_mul(uv_h);
    let buf_size = y_len.saturating_add(uv_len).saturating_add(uv_len);
    let mut buf = vec![0u8; buf_size];

    for i in 0..3 {
        let ok = tf.read_video_frame(&mut buf)?;
        if !ok {
            println!("eos at frame {}", i);
            break;
        }
        let sample_y = buf.get(0).copied().unwrap_or(0);
        let sample_u = buf.get(y_len).copied().unwrap_or(0);
        let sample_v = buf.get(y_len + uv_len).copied().unwrap_or(0);
        println!(
            "frame {} ok (y={}, u={}, v={})",
            i, sample_y, sample_u, sample_v
        );
    }

    Ok(())
}

fn extract_ogg_by_scan(path: &std::path::Path) -> Result<Vec<u8>> {
    let bytes = fs::read(path).with_context(|| format!("read file: {}", path.display()))?;
    let needle = b"OggS";
    let pos = bytes
        .windows(needle.len())
        .position(|w| w == needle)
        .ok_or_else(|| anyhow!("OggS not found in OMV: {}", path.display()))?;
    Ok(bytes[pos..].to_vec())
}

fn yuv_plane_size(width: i32, height: i32, fmt: i32) -> (usize, usize) {
    let w = width.max(1) as usize;
    let h = height.max(1) as usize;
    match fmt {
        siglus_omv_decoder::TH_PF_420 => (w / 2, h / 2),
        siglus_omv_decoder::TH_PF_422 => (w / 2, h),
        siglus_omv_decoder::TH_PF_444 => (w, h),
        _ => (w / 2, h / 2),
    }
}
