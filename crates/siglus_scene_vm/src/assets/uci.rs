//! UCI still-image payloads embedded in some G00 files.
//!
//! UCI stores independent Annex B H.264 streams for color and (optionally)
//! alpha. Assets may use the High 10 profile, so a baseline-only decoder is
//! insufficient.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

fn take_stream<'a>(bytes: &'a [u8], offset: &mut usize) -> Result<&'a [u8]> {
    let length_bytes = bytes
        .get(*offset..*offset + 4)
        .context("UCI stream length missing")?;
    let length = u32::from_le_bytes(length_bytes.try_into()?) as usize;
    *offset += 4;
    let stream = bytes
        .get(*offset..*offset + length)
        .context("UCI stream truncated")?;
    *offset += length;
    Ok(stream)
}

fn decode_h264(stream: &[u8], pixel_format: &str, expected_len: usize) -> Result<Vec<u8>> {
    let mut child = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "h264",
            "-i",
            "pipe:0",
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            pixel_format,
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("start ffmpeg for UCI H.264 decode (FFmpeg must be installed)")?;
    let mut stdin = child.stdin.take().context("FFmpeg stdin unavailable")?;
    let input = stream.to_vec();
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let output = child
        .wait_with_output()
        .context("wait for UCI FFmpeg decode")?;
    writer.join().expect("UCI FFmpeg writer thread panicked")?;
    if !output.status.success() {
        bail!(
            "UCI FFmpeg decode failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    if output.stdout.len() != expected_len {
        bail!(
            "UCI FFmpeg output size mismatch: got {}, expected {expected_len}",
            output.stdout.len()
        );
    }
    Ok(output.stdout)
}

fn crop_even_dimensions(pixels: &[u8], width: usize, height: usize, channels: usize) -> Vec<u8> {
    let padded_width = (width + 1) & !1;
    let mut cropped = vec![0; width * height * channels];
    for row in 0..height {
        let source = row * padded_width * channels;
        let destination = row * width * channels;
        cropped[destination..destination + width * channels]
            .copy_from_slice(&pixels[source..source + width * channels]);
    }
    cropped
}

/// Decode an H.264 UCI image to RGBA, returning its stored dimensions.
pub fn decode_uci(bytes: &[u8]) -> Result<(u32, u32, Vec<u8>)> {
    let kind = *bytes.get(3).context("UCI header truncated")?;
    if !bytes.starts_with(b"UCI") || !matches!(kind, 0x20 | 0x21) {
        bail!("unsupported UCI variant {kind:#04x}");
    }
    let width = u32::from_le_bytes(bytes.get(4..8).context("UCI width missing")?.try_into()?);
    let height = u32::from_le_bytes(bytes.get(8..12).context("UCI height missing")?.try_into()?);
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .context("UCI dimensions overflow")?;
    if pixel_count == 0 || pixel_count > 64 * 1024 * 1024 {
        bail!("invalid UCI dimensions {width}x{height}");
    }
    let mut offset = 12;
    let color_stream = take_stream(bytes, &mut offset)?;
    let alpha_stream = if kind == 0x21 {
        Some(take_stream(bytes, &mut offset)?)
    } else {
        None
    };
    // UCI stores the original dimensions; its H.264 4:2:0 picture is padded
    // to even dimensions when either axis is odd.
    let padded_count = (((width as usize) + 1) & !1)
        .checked_mul(((height as usize) + 1) & !1)
        .context("UCI padded dimensions overflow")?;
    let color = decode_h264(color_stream, "rgba", padded_count * 4)?;
    let mut rgba = crop_even_dimensions(&color, width as usize, height as usize, 4);
    if let Some(alpha_stream) = alpha_stream {
        let padded_alpha = decode_h264(alpha_stream, "gray", padded_count)?;
        let alpha = crop_even_dimensions(&padded_alpha, width as usize, height as usize, 1);
        for (pixel, alpha) in rgba.chunks_exact_mut(4).zip(alpha) {
            pixel[3] = alpha;
        }
    }
    Ok((width, height, rgba))
}
