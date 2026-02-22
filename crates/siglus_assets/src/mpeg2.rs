//! Minimal MPEG-1/2 video probing.
//!
//! Siglus titles sometimes ship MPEG-PS/MPEG-2 video alongside OMV.
//! For now we only implement lightweight parsing of the sequence header
//! (0x000001B3) to retrieve dimensions and frame-rate code.

use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone, Copy)]
pub struct MpegSeqHeader {
    pub width: u16,
    pub height: u16,
    pub aspect_ratio_code: u8,
    pub frame_rate_code: u8,
    pub bit_rate: u32,
    pub vbv_buffer_size: u16,
    pub constrained_parameters_flag: bool,
}

/// Convert MPEG frame_rate_code to nominal FPS.
///
/// ISO/IEC 11172-2 / 13818-2 table 6-4.
pub fn fps_from_frame_rate_code(code: u8) -> Option<f32> {
    match code {
        1 => Some(24000.0 / 1001.0),
        2 => Some(24.0),
        3 => Some(25.0),
        4 => Some(30000.0 / 1001.0),
        5 => Some(30.0),
        6 => Some(50.0),
        7 => Some(60000.0 / 1001.0),
        8 => Some(60.0),
        _ => None,
    }
}

/// Scan the first `max_scan_bytes` bytes of a file for a MPEG sequence header.
pub fn probe_sequence_header(path: impl AsRef<Path>, max_scan_bytes: usize) -> Result<Option<MpegSeqHeader>> {
    let mut f = File::open(&path)
        .with_context(|| format!("open MPEG: {}", path.as_ref().display()))?;
    let file_len = f.seek(SeekFrom::End(0))?;
    f.seek(SeekFrom::Start(0))?;
    let to_read = std::cmp::min(max_scan_bytes as u64, file_len) as usize;
    let mut buf = vec![0u8; to_read];
    f.read_exact(&mut buf)?;
    Ok(find_sequence_header(&buf))
}

pub fn find_sequence_header(data: &[u8]) -> Option<MpegSeqHeader> {
    // Sequence header start code: 00 00 01 B3
    let mut i = 0usize;
    while i + 4 < data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 && data[i + 3] == 0xB3 {
            // Need at least 8 bytes after the start code.
            if i + 12 <= data.len() {
                return parse_sequence_header(&data[i + 4..]);
            }
            return None;
        }
        i += 1;
    }
    None
}

fn parse_sequence_header(p: &[u8]) -> Option<MpegSeqHeader> {
    // ISO/IEC 11172-2 / 13818-2 sequence_header()
    // width: 12 bits, height: 12 bits, aspect_ratio:4, frame_rate:4,
    // bit_rate: 18, marker:1, vbv_buffer_size:10, constrained:1
    if p.len() < 8 {
        return None;
    }
    let width = ((p[0] as u16) << 4) | ((p[1] as u16) >> 4);
    let height = (((p[1] as u16) & 0x0F) << 8) | (p[2] as u16);
    let aspect_ratio_code = (p[3] >> 4) & 0x0F;
    let frame_rate_code = p[3] & 0x0F;

    let bit_rate = ((p[4] as u32) << 10) | ((p[5] as u32) << 2) | ((p[6] as u32) >> 6);
    let marker_bit = (p[6] >> 5) & 0x01;
    if marker_bit != 1 {
        // Not a hard failure, but usually indicates false-positive.
        return None;
    }
    let vbv_buffer_size = (((p[6] as u16) & 0x1F) << 5) | ((p[7] as u16) >> 3);
    let constrained_parameters_flag = ((p[7] >> 2) & 0x01) != 0;

    Some(MpegSeqHeader {
        width,
        height,
        aspect_ratio_code,
        frame_rate_code,
        bit_rate,
        vbv_buffer_size,
        constrained_parameters_flag,
    })
}

/// Convenience: validate that a file looks like MPEG by finding a sequence header.
pub fn ensure_mpeg_like(path: impl AsRef<Path>) -> Result<MpegSeqHeader> {
    match probe_sequence_header(path, 1 << 20)? {
        Some(h) => Ok(h),
        None => bail!("no MPEG sequence header found"),
    }
}

// -----------------------------------------------------------------------------------------------
// Decode (FFmpeg)
// -----------------------------------------------------------------------------------------------

/// A decoded video frame in interleaved RGBA8.
#[derive(Debug, Clone)]
pub struct VideoFrameRgba {
    pub width: u32,
    pub height: u32,
    /// Raw PTS from the decoder (time base depends on container/stream).
    pub pts: Option<i64>,
    /// Interleaved RGBA, row-major, tightly packed (width * height * 4 bytes).
    pub rgba: Vec<u8>,
}

/// Decode MPEG-1/2 video frames using FFmpeg and convert them to RGBA.
///
/// This is intended for Siglus shipped MPEG-PS/MPEG-2 assets. The original engine
/// relies on platform decoders; here we route through FFmpeg for a consistent
/// decoder on desktop.
///
/// Notes:
/// - Requires Cargo feature `mpeg2_ffmpeg` (enabled by default in this workspace).
/// - By default this links against system FFmpeg libraries. To build FFmpeg from
///   source (static), enable feature `ffmpeg_build`.
#[cfg(feature = "mpeg2_ffmpeg")]
pub fn decode_mpeg2_to_rgba_frames(
    path: impl AsRef<Path>,
    max_frames: Option<usize>,
) -> Result<Vec<VideoFrameRgba>> {
    use anyhow::anyhow;
    use ffmpeg_next as ffmpeg;

    ffmpeg::init().map_err(|e| anyhow!("ffmpeg init failed: {e}"))?;

    let path = path.as_ref();
    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow!("non-UTF8 path: {}", path.display()))?
        .to_string();

    let mut ictx = ffmpeg::format::input(&path_str)
        .map_err(|e| anyhow!("ffmpeg open failed: {}: {e}", path.display()))?;

    let input = ictx
        .streams()
        .best(ffmpeg::media::Type::Video)
        .ok_or_else(|| anyhow!("no video stream: {}", path.display()))?;

    let video_stream_index = input.index();

    // Enforce MPEG-1/2 video to match the caller intent. (If Siglus packs other
    // codecs in the future, callers should add a separate route.)
    let codec_id = input.parameters().id();
    if codec_id != ffmpeg::codec::Id::MPEG2VIDEO && codec_id != ffmpeg::codec::Id::MPEG1VIDEO {
        return Err(anyhow!(
            "not MPEG-1/2 video: codec_id={codec_id:?} ({})",
            path.display()
        ));
    }

    let context_decoder = ffmpeg::codec::context::Context::from_parameters(input.parameters())
        .map_err(|e| anyhow!("ffmpeg context from parameters failed: {e}"))?;
    let mut decoder = context_decoder
        .decoder()
        .video()
        .map_err(|e| anyhow!("ffmpeg video decoder init failed: {e}"))?;

    let mut scaler = ffmpeg::software::scaling::context::Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        ffmpeg::format::Pixel::RGBA,
        decoder.width(),
        decoder.height(),
        ffmpeg::software::scaling::flag::Flags::BILINEAR,
    )
    .map_err(|e| anyhow!("ffmpeg scaler init failed: {e}"))?;

    let mut out = Vec::<VideoFrameRgba>::new();

    let mut receive_and_convert = |decoder: &mut ffmpeg::decoder::Video,
                                   scaler: &mut ffmpeg::software::scaling::context::Context,
                                   out: &mut Vec<VideoFrameRgba>| -> Result<()> {
        let mut decoded = ffmpeg::util::frame::video::Video::empty();
        while decoder.receive_frame(&mut decoded).is_ok() {
            let pts = decoded.pts();

            let mut rgba_frame = ffmpeg::util::frame::video::Video::empty();
            scaler
                .run(&decoded, &mut rgba_frame)
                .map_err(|e| anyhow!("ffmpeg scale failed: {e}"))?;

            let w = rgba_frame.width();
            let h = rgba_frame.height();
            let stride = rgba_frame.stride(0);
            let src = rgba_frame.data(0);
            let row_bytes = (w as usize).saturating_mul(4);

            if stride <= 0 {
                return Err(anyhow!("invalid RGBA stride: {stride}"));
            }
            let stride_u = stride as usize;

            let mut rgba = vec![0u8; row_bytes.saturating_mul(h as usize)];
            for y in 0..(h as usize) {
                let src_off = y.saturating_mul(stride_u);
                let dst_off = y.saturating_mul(row_bytes);
                let src_end = src_off.saturating_add(row_bytes);
                let dst_end = dst_off.saturating_add(row_bytes);
                if src_end > src.len() || dst_end > rgba.len() {
                    return Err(anyhow!(
                        "frame copy out of range (src_end={src_end}, src_len={}, dst_end={dst_end}, dst_len={})",
                        src.len(),
                        rgba.len()
                    ));
                }
                rgba[dst_off..dst_end].copy_from_slice(&src[src_off..src_end]);
            }

            out.push(VideoFrameRgba {
                width: w,
                height: h,
                pts,
                rgba,
            });

            if let Some(limit) = max_frames {
                if out.len() >= limit {
                    break;
                }
            }
        }
        Ok(())
    };

    for (stream, packet) in ictx.packets() {
        if stream.index() != video_stream_index {
            continue;
        }
        decoder
            .send_packet(&packet)
            .map_err(|e| anyhow!("ffmpeg send_packet failed: {e}"))?;
        receive_and_convert(&mut decoder, &mut scaler, &mut out)?;
        if let Some(limit) = max_frames {
            if out.len() >= limit {
                break;
            }
        }
    }

    // Flush.
    decoder
        .send_eof()
        .map_err(|e| anyhow!("ffmpeg send_eof failed: {e}"))?;
    receive_and_convert(&mut decoder, &mut scaler, &mut out)?;

    Ok(out)
}

#[cfg(not(feature = "mpeg2_ffmpeg"))]
pub fn decode_mpeg2_to_rgba_frames(
    _path: impl AsRef<Path>,
    _max_frames: Option<usize>,
) -> Result<Vec<VideoFrameRgba>> {
    bail!("mpeg2 decode disabled (enable Cargo feature: mpeg2_ffmpeg)")
}
