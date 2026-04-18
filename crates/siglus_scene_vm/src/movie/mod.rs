use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};

use crate::assets::RgbaImage;
use crate::audio::{AudioHub, TrackKind};

#[derive(Debug, Clone)]
pub struct MovieInfo {
    pub path: PathBuf,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f32>,
    pub decoded_frames: Option<usize>,
    pub audio_duration_ms: Option<u64>,
}

impl MovieInfo {
    pub fn duration_ms(&self) -> Option<u64> {
        if let Some(ms) = self.audio_duration_ms {
            return Some(ms);
        }
        let fps = self.fps?;
        let frames = self.decoded_frames?;
        if fps <= 0.0 {
            return None;
        }
        Some(((frames as f64) * 1000.0 / (fps as f64)).round() as u64)
    }
}

/// Minimal movie state holder.
///
/// The original Siglus engine plays MOV via a native playback pipeline.
/// Here we provide a deterministic, cross-platform metadata path:
/// - MPEG2 (`.mpg` / `.mpeg`) via `siglus_assets::mpeg2`
/// - OMV (`.omv`) via `siglus_assets::omv`
#[derive(Debug)]
pub struct MovieManager {
    project_dir: PathBuf,
    current_append_dir: String,
    current: Option<MovieInfo>,
    cache: HashMap<PathBuf, MovieAsset>,
    preview_cache: HashMap<PathBuf, Arc<RgbaImage>>,
    playbacks: HashMap<u64, MoviePlayback>,
    next_playback_id: u64,
}

impl MovieManager {
    pub fn new(project_dir: PathBuf) -> Self {
        Self {
            project_dir,
            current_append_dir: String::new(),
            current: None,
            cache: HashMap::new(),
            preview_cache: HashMap::new(),
            playbacks: HashMap::new(),
            next_playback_id: 1,
        }
    }

    pub fn current(&self) -> Option<&MovieInfo> {
        self.current.as_ref()
    }

    pub fn set_current_append_dir(&mut self, append_dir: impl Into<String>) {
        self.current_append_dir = append_dir.into();
    }

    pub fn stop(&mut self) {
        self.current = None;
    }

    pub fn prepare(&mut self, file_name: &str) -> Result<MovieInfo> {
        self.play(file_name, false, false)
    }

    pub fn play(&mut self, file_name: &str, _wait: bool, _key_skip: bool) -> Result<MovieInfo> {
        let path = resolve_mov_path(&self.project_dir, &self.current_append_dir, file_name)?;
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        let info = if ext == "omv" {
            let omv = siglus_assets::omv::OmvFile::open(&path)
                .with_context(|| format!("open OMV: {}", path.display()))?;
            let w = omv.header.display_width;
            let h = omv.header.display_height;
            let fps = if omv.header.frame_time_us != 0 {
                Some(1_000_000.0 / (omv.header.frame_time_us as f32))
            } else {
                None
            };
            MovieInfo {
                path,
                width: (w > 0).then_some(w),
                height: (h > 0).then_some(h),
                fps,
                decoded_frames: (omv.header.packet_count_hint > 0)
                    .then_some(omv.header.packet_count_hint as usize),
                audio_duration_ms: None,
            }
        } else {
            let bytes =
                fs::read(&path).with_context(|| format!("read movie file: {}", path.display()))?;

            let mut width = None;
            let mut height = None;
            let mut fps = None;

            if let Some(h) = siglus_assets::mpeg2::find_sequence_header(&bytes) {
                width = Some(h.width as u32);
                height = Some(h.height as u32);
                fps = siglus_assets::mpeg2::fps_from_frame_rate_code(h.frame_rate_code);
            }

            let decoded_frames = decode_frames_if_enabled(&path)?;

            MovieInfo {
                path,
                width,
                height,
                fps,
                decoded_frames,
                audio_duration_ms: None,
            }
        };

        self.current = Some(info.clone());
        Ok(info)
    }

    /// Resolve and decode a movie asset into RGBA frames (cached).
    pub fn ensure_asset(&mut self, file_name: &str) -> Result<(&MovieAsset, bool)> {
        let path = resolve_mov_path(&self.project_dir, &self.current_append_dir, file_name)?;
        let existed = self.cache.contains_key(&path);
        if !existed {
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();

            let asset = if ext == "omv" {
                decode_omv_asset(&path)?
            } else {
                decode_mpeg2_asset(&path)?
            };
            self.cache.insert(path.clone(), asset);
        }
        let asset = self.cache.get(&path).expect("asset cached");
        Ok((asset, !existed))
    }

    pub fn ensure_preview_frame(&mut self, file_name: &str) -> Result<Arc<RgbaImage>> {
        let path = resolve_mov_path(&self.project_dir, &self.current_append_dir, file_name)?;
        if let Some(frame) = self.preview_cache.get(&path) {
            return Ok(frame.clone());
        }
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let frame = if ext == "omv" {
            decode_omv_preview_frame(&path)?
        } else {
            decode_mpeg2_preview_frame(&path)?
        };
        self.preview_cache.insert(path, frame.clone());
        Ok(frame)
    }
    pub fn start_audio(
        &mut self,
        audio: &mut AudioHub,
        track: &MovieAudio,
        offset_ms: u64,
    ) -> Result<u64> {
        let wav = encode_wav_i16_interleaved_offset(track, offset_ms);
        let data = StaticSoundData::from_cursor(Cursor::new(wav))
            .context("kira: decode movie WAV bytes")?;
        let handle = audio.play_static(TrackKind::Mov, data)?;
        let id = self.next_playback_id;
        self.next_playback_id = self.next_playback_id.saturating_add(1).max(1);
        self.playbacks.insert(
            id,
            MoviePlayback {
                handle,
                duration_ms: track.duration_ms,
            },
        );
        Ok(id)
    }

    pub fn pause_audio(&mut self, id: u64) {
        let Some(p) = self.playbacks.get_mut(&id) else {
            return;
        };
        let _ = p.handle.pause(kira::tween::Tween::default());
    }

    pub fn resume_audio(&mut self, id: u64) {
        let Some(p) = self.playbacks.get_mut(&id) else {
            return;
        };
        let _ = p.handle.resume(kira::tween::Tween::default());
    }

    pub fn stop_audio(&mut self, id: u64) {
        if let Some(mut p) = self.playbacks.remove(&id) {
            let _ = p.handle.stop(kira::tween::Tween::default());
        }
    }
}

fn resolve_mov_path(
    project_dir: &Path,
    current_append_dir: &str,
    file_name: &str,
) -> Result<PathBuf> {
    let (path, _ty) =
        crate::resource::find_mov_path_with_append_dir(project_dir, current_append_dir, file_name)?;
    Ok(path)
}

fn decode_frames_if_enabled(_path: &Path) -> Result<Option<usize>> {
    Ok(None)
}

#[derive(Debug, Clone)]
pub struct MovieAsset {
    pub info: MovieInfo,
    pub frames: Vec<Arc<RgbaImage>>,
    pub audio: Option<MovieAudio>,
}

#[derive(Debug, Clone)]
pub struct MovieAudio {
    pub samples: Arc<Vec<i16>>,
    pub channels: u16,
    pub sample_rate: u32,
    pub duration_ms: Option<u64>,
}

#[derive(Debug)]
struct MoviePlayback {
    handle: StaticSoundHandle,
    duration_ms: Option<u64>,
}

fn decode_mpeg2_preview_frame(path: &Path) -> Result<Arc<RgbaImage>> {
    let bytes = fs::read(path).with_context(|| format!("read movie file: {}", path.display()))?;
    let mut pipeline = na_mpeg2_decoder::MpegVideoPipeline::new();
    let mut first = None;
    pipeline
        .push_with(&bytes, None, |f| {
            if first.is_none() {
                let w = f.width as u32;
                let h = f.height as u32;
                let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
                na_mpeg2_decoder::frame_to_rgba_bt601_limited(&f, &mut rgba);
                first = Some(Arc::new(RgbaImage {
                    width: w,
                    height: h,
                    rgba,
                }));
            }
        })
        .context("mpeg2 preview decode")?;
    if first.is_none() {
        pipeline.flush_with(|f| {
            if first.is_none() {
                let w = f.width as u32;
                let h = f.height as u32;
                let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
                na_mpeg2_decoder::frame_to_rgba_bt601_limited(&f, &mut rgba);
                first = Some(Arc::new(RgbaImage {
                    width: w,
                    height: h,
                    rgba,
                }));
            }
        })?;
    }
    first.ok_or_else(|| anyhow!("mpeg2 preview frame missing: {}", path.display()))
}

fn decode_omv_preview_frame(path: &Path) -> Result<Arc<RgbaImage>> {
    let omv = siglus_assets::omv::OmvFile::open(path).ok();
    let ogg_data = siglus_assets::omv::OmvFile::read_embedded_ogg(path)
        .or_else(|_| extract_ogg_by_scan(path))
        .with_context(|| format!("read embedded ogg: {}", path.display()))?;
    let (vinfo, packed) = siglus_omv_decoder::decode_first_video_frame_from_memory(ogg_data)
        .with_context(|| format!("decode first omv frame: {}", path.display()))?;
    let display_h = omv
        .as_ref()
        .map(|m| m.header.display_height as i32)
        .unwrap_or(vinfo.height);
    let width = omv
        .as_ref()
        .map(|m| m.header.display_width.max(1))
        .unwrap_or(vinfo.width.max(1) as u32);
    let height = display_h.max(1) as u32;
    let rgba = convert_omv_frame(&packed, vinfo.width, vinfo.height, vinfo.fmt, display_h);
    Ok(Arc::new(RgbaImage {
        width,
        height,
        rgba,
    }))
}

fn decode_mpeg2_asset(path: &Path) -> Result<MovieAsset> {
    let bytes = fs::read(path).with_context(|| format!("read movie file: {}", path.display()))?;
    let mut width = None;
    let mut height = None;
    let mut fps = None;
    if let Some(h) = siglus_assets::mpeg2::find_sequence_header(&bytes) {
        width = Some(h.width as u32);
        height = Some(h.height as u32);
        fps = siglus_assets::mpeg2::fps_from_frame_rate_code(h.frame_rate_code);
    }

    let mut frames: Vec<Arc<RgbaImage>> = Vec::new();
    let mut pipeline = na_mpeg2_decoder::MpegVideoPipeline::new();
    pipeline
        .push_with(&bytes, None, |f| {
            let w = f.width as u32;
            let h = f.height as u32;
            let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
            na_mpeg2_decoder::frame_to_rgba_bt601_limited(&f, &mut rgba);
            frames.push(Arc::new(RgbaImage {
                width: w,
                height: h,
                rgba,
            }));
        })
        .context("mpeg2 decode")?;
    pipeline.flush_with(|f| {
        let w = f.width as u32;
        let h = f.height as u32;
        let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
        na_mpeg2_decoder::frame_to_rgba_bt601_limited(&f, &mut rgba);
        frames.push(Arc::new(RgbaImage {
            width: w,
            height: h,
            rgba,
        }));
    })?;

    let info = MovieInfo {
        path: path.to_path_buf(),
        width: width.or_else(|| frames.first().map(|f| f.width)),
        height: height.or_else(|| frames.first().map(|f| f.height)),
        fps,
        decoded_frames: Some(frames.len()),
        audio_duration_ms: None,
    };
    Ok(MovieAsset {
        info,
        frames,
        audio: None,
    })
}

fn decode_omv_asset(path: &Path) -> Result<MovieAsset> {
    let omv = siglus_assets::omv::OmvFile::open(path).ok();

    let ogg_data = siglus_assets::omv::OmvFile::read_embedded_ogg(path)
        .or_else(|_| extract_ogg_by_scan(path))
        .with_context(|| format!("read embedded ogg: {}", path.display()))?;

    let mut video_tf = siglus_omv_decoder::TheoraFile::open_from_memory(ogg_data.clone())
        .with_context(|| format!("open theora: {}", path.display()))?;
    let vinfo = video_tf.info();

    let display_w = omv
        .as_ref()
        .map(|m| m.header.display_width as i32)
        .unwrap_or(vinfo.width);
    let display_h = omv
        .as_ref()
        .map(|m| m.header.display_height as i32)
        .unwrap_or(vinfo.height);

    let width = display_w.max(1) as u32;
    let height = display_h.max(1) as u32;

    let fps = if let Some(m) = omv.as_ref() {
        if m.header.frame_time_us != 0 {
            Some(1_000_000.0 / (m.header.frame_time_us as f32))
        } else if vinfo.fps > 0.0 {
            Some(vinfo.fps as f32)
        } else {
            None
        }
    } else if vinfo.fps > 0.0 {
        Some(vinfo.fps as f32)
    } else {
        None
    };

    let audio = {
        let mut audio_tf = siglus_omv_decoder::TheoraFile::open_from_memory(ogg_data)
            .with_context(|| format!("open theora audio: {}", path.display()))?;
        decode_omv_audio(&mut audio_tf)?
    };

    let (uv_w, uv_h) = yuv_plane_size(vinfo.width, vinfo.height, vinfo.fmt);
    let buf_size = (vinfo.width as usize)
        .saturating_mul(vinfo.height as usize)
        .saturating_add(uv_w.saturating_mul(uv_h))
        .saturating_add(uv_w.saturating_mul(uv_h));
    let mut buf = vec![0u8; buf_size];

    let mut frames: Vec<Arc<RgbaImage>> = Vec::new();
    while video_tf.read_video_frame(&mut buf)? {
        let rgba = convert_omv_frame(&buf, vinfo.width, vinfo.height, vinfo.fmt, display_h);
        frames.push(Arc::new(RgbaImage {
            width,
            height,
            rgba,
        }));
    }

    let info = MovieInfo {
        path: path.to_path_buf(),
        width: Some(width),
        height: Some(height),
        fps,
        decoded_frames: Some(frames.len()),
        audio_duration_ms: audio.as_ref().and_then(|a| a.duration_ms),
    };

    Ok(MovieAsset {
        info,
        frames,
        audio,
    })
}

fn extract_ogg_by_scan(path: &Path) -> Result<Vec<u8>> {
    let bytes = fs::read(path).with_context(|| format!("read file: {}", path.display()))?;
    let needle = b"OggS";
    let pos = bytes
        .windows(needle.len())
        .position(|w| w == needle)
        .ok_or_else(|| anyhow!("OggS not found in OMV: {}", path.display()))?;
    Ok(bytes[pos..].to_vec())
}

fn decode_omv_audio(tf: &mut siglus_omv_decoder::TheoraFile) -> Result<Option<MovieAudio>> {
    if !tf.has_audio() {
        return Ok(None);
    }
    let Some((channels, sample_rate)) = tf.audio_info() else {
        return Ok(None);
    };
    if channels <= 0 || sample_rate <= 0 {
        return Ok(None);
    }
    let channels_u16 = channels as u16;
    let sample_rate_u32 = sample_rate as u32;

    let mut samples: Vec<f32> = Vec::new();
    let mut buf = vec![0.0f32; (4096usize).saturating_mul(channels as usize)];
    loop {
        let read = tf.read_audio_samples(&mut buf)?;
        if read == 0 {
            break;
        }
        samples.extend_from_slice(&buf[..read]);
    }

    if samples.is_empty() {
        return Ok(None);
    }
    let mut samples_i16: Vec<i16> = Vec::with_capacity(samples.len());
    for &s in &samples {
        let clamped = s.max(-1.0).min(1.0);
        let v = (clamped * 32767.0).round() as i16;
        samples_i16.push(v);
    }
    let frames = (samples_i16.len() as u64) / (channels_u16 as u64);
    let duration_ms = if sample_rate_u32 > 0 {
        Some(((frames as f64) * 1000.0 / (sample_rate_u32 as f64)).round() as u64)
    } else {
        None
    };

    Ok(Some(MovieAudio {
        samples: Arc::new(samples_i16),
        channels: channels_u16,
        sample_rate: sample_rate_u32,
        duration_ms,
    }))
}

fn encode_wav_i16_interleaved_offset(track: &MovieAudio, offset_ms: u64) -> Vec<u8> {
    let channels = track.channels;
    let sample_rate = track.sample_rate;
    let samples = track.samples.as_ref();
    let frames_offset = ((offset_ms as u64) * (sample_rate as u64) / 1000) as usize;
    let start = frames_offset.saturating_mul(channels as usize);
    let slice = if start < samples.len() {
        &samples[start..]
    } else {
        &samples[samples.len()..]
    };
    encode_wav_i16_interleaved(slice, channels, sample_rate)
}

fn encode_wav_i16_interleaved(samples: &[i16], channels: u16, sample_rate: u32) -> Vec<u8> {
    let bytes_per_sample = 2u16;
    let block_align = channels.saturating_mul(bytes_per_sample);
    let byte_rate = (sample_rate as u64).saturating_mul(block_align as u64) as u32;
    let data_bytes = samples.len().saturating_mul(bytes_per_sample as usize) as u32;
    let riff_size = 36u32.saturating_add(data_bytes);

    let mut out = Vec::with_capacity((data_bytes as usize) + 44);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes.to_le_bytes());

    for &s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

fn convert_omv_frame(
    data: &[u8],
    width: i32,
    video_height: i32,
    fmt: i32,
    display_height: i32,
) -> Vec<u8> {
    let w = width.max(1) as usize;
    let vh = video_height.max(1) as usize;
    let dh = display_height.max(1) as usize;

    let y_plane_len = w.saturating_mul(vh);
    // OMV alpha movies store the alpha channel in the lower half of the luma
    // plane while keeping chroma sized for the visible display height.
    let chroma_height = if vh > dh {
        display_height
    } else {
        video_height
    };
    let (uv_w, uv_h) = yuv_plane_size(width, chroma_height, fmt);
    let uv_len = uv_w.saturating_mul(uv_h);
    let u_off = y_plane_len;
    let v_off = y_plane_len.saturating_add(uv_len);

    let mut rgba = vec![0u8; w.saturating_mul(dh).saturating_mul(4)];
    let alpha_offset = if vh > dh { Some(dh) } else { None };

    for y in 0..dh {
        let y_row = y * w;
        let uv_y = match fmt {
            siglus_omv_decoder::TH_PF_420 => (y / 2),
            _ => y,
        };
        for x in 0..w {
            let y_idx = y_row + x;
            let yv = data.get(y_idx).copied().unwrap_or(0) as f32;

            let uv_x = match fmt {
                siglus_omv_decoder::TH_PF_420 | siglus_omv_decoder::TH_PF_422 => x / 2,
                _ => x,
            };
            let u_idx = u_off
                .saturating_add(uv_y.saturating_mul(uv_w))
                .saturating_add(uv_x);
            let v_idx = v_off
                .saturating_add(uv_y.saturating_mul(uv_w))
                .saturating_add(uv_x);
            let u = data.get(u_idx).copied().unwrap_or(128) as f32 - 128.0;
            let v = data.get(v_idx).copied().unwrap_or(128) as f32 - 128.0;

            let r = clamp_f(yv + 1.402 * v);
            let g = clamp_f(yv - 0.344136 * u - 0.714136 * v);
            let b = clamp_f(yv + 1.772 * u);

            let a = if let Some(off) = alpha_offset {
                let ay = off.saturating_add(y);
                if ay < vh {
                    data.get(ay * w + x).copied().unwrap_or(0xFF)
                } else {
                    0xFF
                }
            } else {
                0xFF
            };

            let out = (y * w + x) * 4;
            rgba[out] = r;
            rgba[out + 1] = g;
            rgba[out + 2] = b;
            rgba[out + 3] = a;
        }
    }

    rgba
}

fn clamp_f(v: f32) -> u8 {
    if v <= 0.0 {
        0
    } else if v >= 255.0 {
        255
    } else {
        v.round() as u8
    }
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
