use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::{
    mpsc::{self, Receiver, TryRecvError},
    Arc,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};

use crate::assets::RgbaImage;
use crate::audio::{AudioHub, TrackKind};

const MPEG2_HEADER_PROBE_BYTES: usize = 256 * 1024;
const MPEG2_STREAM_CHUNK_BYTES: usize = 256 * 1024;
const MPEG2_STREAM_CHANNEL_CAPACITY: usize = 4;
const MPEG2_STREAM_MAX_DRAIN_EVENTS: usize = 8;
const MPEG2_STREAM_FRAME_KEEP: usize = 6;
const MPEG2_STREAM_DECODE_LEAD_FRAMES: usize = 3;
const MOVIE_AUDIO_SEGMENT_MS: u64 = 4000;
const MOVIE_AUDIO_DECODE_LEAD_MS: usize = 8000;
const MOVIE_AUDIO_MAX_DRAIN_EVENTS: usize = 4;
const OMV_STREAM_CHANNEL_CAPACITY: usize = 12;
const OMV_STREAM_MAX_DRAIN_EVENTS: usize = 16;
const OMV_STREAM_FRAME_KEEP: usize = 16;
const OMV_STREAM_DECODE_LEAD_FRAMES: usize = 4;

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
        if fps <= 0.0 || frames == 0 {
            return None;
        }
        Some(((frames as f64) * 1000.0 / (fps as f64)).round() as u64)
    }
}

#[derive(Debug, Clone)]
pub struct MovieStreamFrame {
    pub frame: Arc<RgbaImage>,
    pub frame_idx: usize,
    pub fps: Option<f32>,
    pub total_ms: Option<u64>,
    pub audio: Option<MovieAudio>,
    pub audio_ready: bool,
    pub decoded_now: bool,
    pub clamped_timer_ms: Option<u64>,
}

enum Mpeg2StreamEvent {
    Info {
        width: Option<u32>,
        height: Option<u32>,
        fps: Option<f32>,
    },
    Video {
        frame_idx: usize,
        frame: Arc<RgbaImage>,
    },
    Done,
}

enum MovieAudioStreamEvent {
    Segment(MovieAudio),
    Done,
}

struct Mpeg2StreamState {
    rx: Receiver<Result<Mpeg2StreamEvent, String>>,
    audio_rx: Receiver<Result<MovieAudioStreamEvent, String>>,
    frames: VecDeque<(usize, Arc<RgbaImage>)>,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f32>,
    decoded_frames: usize,
    done: bool,
    audio_segments: VecDeque<MovieAudio>,
    audio_done: bool,
    decoded_any_this_poll: bool,
    request_frames: Arc<AtomicUsize>,
    request_audio_until_ms: Arc<AtomicUsize>,
}

impl Drop for Mpeg2StreamState {
    fn drop(&mut self) {
        self.request_frames.store(usize::MAX, Ordering::Release);
        self.request_audio_until_ms.store(usize::MAX, Ordering::Release);
    }
}

enum OmvStreamEvent {
    Info {
        width: u32,
        height: u32,
        fps: Option<f32>,
        frame_time_ms: Option<f64>,
        total_frames_hint: Option<usize>,
    },
    Video {
        frame_idx: usize,
        frame: Arc<RgbaImage>,
    },
    Done,
}

struct OmvStreamState {
    rx: Receiver<Result<OmvStreamEvent, String>>,
    frames: VecDeque<(usize, Arc<RgbaImage>)>,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f32>,
    frame_time_ms: Option<f64>,
    total_frames_hint: Option<usize>,
    decoded_frames: usize,
    done: bool,
    request_frames: Arc<AtomicUsize>,
}

impl Drop for OmvStreamState {
    fn drop(&mut self) {
        self.request_frames.store(usize::MAX, Ordering::Release);
    }
}

/// Minimal movie state holder.
///
/// The original Siglus engine plays MOV via a native playback pipeline.
/// Here we provide a deterministic, cross-platform metadata path:
/// - MPEG2 (`.mpg` / `.mpeg`) via `siglus_assets::mpeg2`
/// - OMV (`.omv`) via `siglus_assets::omv`
pub struct MovieManager {
    project_dir: PathBuf,
    current_append_dir: String,
    current: Option<MovieInfo>,
    cache: HashMap<PathBuf, MovieAsset>,
    preview_cache: HashMap<PathBuf, Arc<RgbaImage>>,
    decode_tasks: HashMap<PathBuf, Receiver<Result<MovieAsset, String>>>,
    mpeg2_streams: HashMap<PathBuf, Mpeg2StreamState>,
    omv_streams: HashMap<PathBuf, OmvStreamState>,
    playbacks: HashMap<u64, MoviePlayback>,
    next_playback_id: u64,
}

impl std::fmt::Debug for MovieManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MovieManager")
            .field("project_dir", &self.project_dir)
            .field("current_append_dir", &self.current_append_dir)
            .field("current", &self.current)
            .field("cache_len", &self.cache.len())
            .field("preview_cache_len", &self.preview_cache.len())
            .field("decode_tasks_len", &self.decode_tasks.len())
            .field("mpeg2_streams_len", &self.mpeg2_streams.len())
            .field("omv_streams_len", &self.omv_streams.len())
            .field("playbacks_len", &self.playbacks.len())
            .finish()
    }
}

impl MovieManager {
    pub fn new(project_dir: PathBuf) -> Self {
        Self {
            project_dir,
            current_append_dir: String::new(),
            current: None,
            cache: HashMap::new(),
            preview_cache: HashMap::new(),
            decode_tasks: HashMap::new(),
            mpeg2_streams: HashMap::new(),
            omv_streams: HashMap::new(),
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
        self.mpeg2_streams.clear();
        self.omv_streams.clear();
    }

    pub fn prepare(&mut self, file_name: &str) -> Result<MovieInfo> {
        self.play(file_name, false, false)
    }

    pub fn prepare_omv(&mut self, file_name: &str) -> Result<MovieInfo> {
        let path = crate::resource::find_omv_path_with_append_dir(
            &self.project_dir,
            &self.current_append_dir,
            file_name,
        )?;
        let omv = siglus_assets::omv::OmvFile::open(&path)
            .with_context(|| format!("open OMV: {}", path.display()))?;
        let w = omv.header.display_width;
        let h = omv.header.display_height;
        let fps = if omv.header.frame_time_us != 0 {
            Some(1_000_000.0 / (omv.header.frame_time_us as f32))
        } else {
            None
        };
        let info = MovieInfo {
            path,
            width: (w > 0).then_some(w),
            height: (h > 0).then_some(h),
            fps,
            decoded_frames: (omv.header.packet_count_hint > 0)
                .then_some(omv.header.packet_count_hint as usize),
            audio_duration_ms: None,
        };
        self.current = Some(info.clone());
        Ok(info)
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
            let prefix = read_file_prefix(&path, MPEG2_HEADER_PROBE_BYTES)
                .with_context(|| format!("read movie header: {}", path.display()))?;

            let mut width = None;
            let mut height = None;
            let mut fps = None;

            if let Some(h) = siglus_assets::mpeg2::find_sequence_header(&prefix) {
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
        self.ensure_asset_for_path(path)
    }

    pub fn ensure_omv_asset(&mut self, file_name: &str) -> Result<(&MovieAsset, bool)> {
        let path = crate::resource::find_omv_path_with_append_dir(
            &self.project_dir,
            &self.current_append_dir,
            file_name,
        )?;
        self.ensure_asset_for_path(path)
    }

    fn ensure_asset_for_path(&mut self, path: PathBuf) -> Result<(&MovieAsset, bool)> {
        let existed = self.cache.contains_key(&path);
        if !existed {
            let asset = decode_asset_for_path(&path)?;
            self.cache.insert(path.clone(), asset);
        }
        let asset = self.cache.get(&path).expect("asset cached");
        Ok((asset, !existed))
    }

    pub fn poll_asset(&mut self, file_name: &str) -> Result<Option<(&MovieAsset, bool)>> {
        let path = resolve_mov_path(&self.project_dir, &self.current_append_dir, file_name)?;
        self.poll_asset_for_path(path)
    }

    pub fn poll_omv_asset(&mut self, file_name: &str) -> Result<Option<(&MovieAsset, bool)>> {
        let path = crate::resource::find_omv_path_with_append_dir(
            &self.project_dir,
            &self.current_append_dir,
            file_name,
        )?;
        self.poll_asset_for_path(path)
    }

    fn poll_asset_for_path(&mut self, path: PathBuf) -> Result<Option<(&MovieAsset, bool)>> {
        if self.cache.contains_key(&path) {
            let asset = self.cache.get(&path).expect("asset cached");
            return Ok(Some((asset, false)));
        }

        let mut completed = None;
        let mut failed = None;
        if let Some(rx) = self.decode_tasks.get(&path) {
            match rx.try_recv() {
                Ok(Ok(asset)) => completed = Some(asset),
                Ok(Err(err)) => failed = Some(err),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    failed = Some(format!(
                        "movie decode worker disconnected: {}",
                        path.display()
                    ));
                }
            }
        } else {
            let (tx, rx) = mpsc::channel();
            let worker_path = path.clone();
            thread::spawn(move || {
                let result = decode_asset_for_path(&worker_path).map_err(|e| format!("{:#}", e));
                let _ = tx.send(result);
            });
            self.decode_tasks.insert(path.clone(), rx);
        }

        if let Some(err) = failed {
            self.decode_tasks.remove(&path);
            return Err(anyhow!(err));
        }
        if let Some(asset) = completed {
            self.decode_tasks.remove(&path);
            self.cache.insert(path.clone(), asset);
            let asset = self.cache.get(&path).expect("asset cached");
            return Ok(Some((asset, true)));
        }

        Ok(None)
    }

    pub fn poll_global_movie_frame(
        &mut self,
        file_name: &str,
        timer_ms: u64,
    ) -> Result<Option<MovieStreamFrame>> {
        let path = resolve_mov_path(&self.project_dir, &self.current_append_dir, file_name)?;
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext == "omv" {
            return self.poll_omv_stream_frame_for_path(path, timer_ms);
        }
        self.poll_mpeg2_stream_frame_for_path(path, timer_ms)
    }

    fn poll_cached_movie_frame_for_path(
        &mut self,
        path: PathBuf,
        timer_ms: u64,
    ) -> Result<Option<MovieStreamFrame>> {
        let (asset, decoded_now) = match self.poll_asset_for_path(path)? {
            Some(v) => v,
            None => return Ok(None),
        };
        if asset.frames.is_empty() {
            return Ok(None);
        }

        let fps = asset.info.fps.unwrap_or_else(|| {
            asset
                .info
                .duration_ms()
                .filter(|ms| *ms > 0)
                .map(|ms| (asset.frames.len() as f32) * 1000.0 / (ms as f32))
                .unwrap_or(0.0)
        });
        let mut idx = frame_index_for_timer(timer_ms, fps, asset.frames.len());
        if idx >= asset.frames.len() {
            idx = asset.frames.len() - 1;
        }

        Ok(Some(MovieStreamFrame {
            frame: asset.frames[idx].clone(),
            frame_idx: idx,
            fps: (fps > 0.0).then_some(fps),
            total_ms: asset.info.duration_ms(),
            audio: asset.audio.clone(),
            audio_ready: true,
            decoded_now,
            clamped_timer_ms: None,
        }))
    }

    fn poll_mpeg2_stream_frame_for_path(
        &mut self,
        path: PathBuf,
        timer_ms: u64,
    ) -> Result<Option<MovieStreamFrame>> {
        if !self.mpeg2_streams.contains_key(&path) {
            let state = spawn_mpeg2_stream_state(path.clone())?;
            self.mpeg2_streams.insert(path.clone(), state);
        }

        let desired_before_drain = self.mpeg2_streams.get(&path).and_then(|state| {
            state
                .fps
                .filter(|f| *f > 0.0)
                .map(|fps| ((timer_ms as f64) * (fps as f64) / 1000.0).floor() as usize)
        });
        let restart_stream = desired_before_drain
            .and_then(|desired| {
                self.mpeg2_streams.get(&path).map(|state| {
                    let front_after_target = state
                        .frames
                        .front()
                        .map(|(idx, _)| *idx > desired)
                        .unwrap_or(false);
                    let decoder_already_past_target = state.frames.is_empty()
                        && state.decoded_frames > desired.saturating_add(MPEG2_STREAM_DECODE_LEAD_FRAMES)
                        && !state.done;
                    front_after_target || decoder_already_past_target
                })
            })
            .unwrap_or(false);
        if restart_stream {
            self.mpeg2_streams.remove(&path);
            let state = spawn_mpeg2_stream_state(path.clone())?;
            self.mpeg2_streams.insert(path.clone(), state);
        }

        let state = self
            .mpeg2_streams
            .get_mut(&path)
            .expect("mpeg2 stream state exists");
        let request_until = desired_before_drain
            .unwrap_or(0)
            .saturating_add(MPEG2_STREAM_DECODE_LEAD_FRAMES);
        state.request_frames.store(request_until, Ordering::Release);
        drain_mpeg2_stream_state(path.as_path(), state, desired_before_drain, timer_ms)?;

        if state.frames.is_empty() {
            return Ok(None);
        }

        let latest_idx = state.decoded_frames.saturating_sub(1);
        let desired_idx = desired_before_drain.unwrap_or(latest_idx);
        let chosen_idx = desired_idx.min(latest_idx);

        let selected = state
            .frames
            .iter()
            .rev()
            .find(|(idx, _)| *idx <= chosen_idx)
            .or_else(|| state.frames.back())
            .map(|(idx, frame)| (*idx, frame.clone()));

        let Some((actual_frame_idx, frame)) = selected else {
            return Ok(None);
        };

        let video_total_ms = if state.done && state.decoded_frames > 0 {
            state
                .fps
                .filter(|f| *f > 0.0)
                .map(|fps| ((state.decoded_frames as f64) * 1000.0 / (fps as f64)).round() as u64)
        } else {
            None
        };
        let audio_total_ms = state
            .audio_segments
            .back()
            .map(|a| a.end_ms());
        let total_ms = match (audio_total_ms, video_total_ms) {
            (Some(a), Some(v)) => Some(a.max(v)),
            (Some(a), None) => Some(a),
            (None, Some(v)) => Some(v),
            (None, None) => None,
        };

        let audio = select_audio_segment(&state.audio_segments, timer_ms);
        let audio_ready = state.audio_done && audio.is_none();
        state.decoded_any_this_poll = false;

        Ok(Some(MovieStreamFrame {
            frame,
            frame_idx: actual_frame_idx,
            fps: state.fps,
            total_ms,
            audio,
            audio_ready,
            decoded_now: false,
            clamped_timer_ms: None,
        }))
    }

    fn poll_omv_stream_frame_for_path(
        &mut self,
        path: PathBuf,
        timer_ms: u64,
    ) -> Result<Option<MovieStreamFrame>> {
        if !self.omv_streams.contains_key(&path) {
            let state = spawn_omv_stream_state(path.clone())?;
            self.omv_streams.insert(path.clone(), state);
        }

        let desired_before_drain = self.omv_streams.get(&path).and_then(|state| {
            if let Some(ms) = state.frame_time_ms.filter(|v| *v > 0.0) {
                Some(((timer_ms as f64) / ms).floor() as usize)
            } else {
                state
                    .fps
                    .filter(|f| *f > 0.0)
                    .map(|fps| ((timer_ms as f64) * (fps as f64) / 1000.0).floor() as usize)
            }
        });
        let restart_stream = desired_before_drain
            .and_then(|desired| {
                self.omv_streams.get(&path).map(|state| {
                    let front_after_target = state
                        .frames
                        .front()
                        .map(|(idx, _)| *idx > desired)
                        .unwrap_or(false);
                    let decoder_already_past_target = state.frames.is_empty()
                        && state.decoded_frames > desired.saturating_add(OMV_STREAM_DECODE_LEAD_FRAMES)
                        && !state.done;
                    front_after_target || decoder_already_past_target
                })
            })
            .unwrap_or(false);
        if restart_stream {
            self.omv_streams.remove(&path);
            let state = spawn_omv_stream_state(path.clone())?;
            self.omv_streams.insert(path.clone(), state);
        }

        let state = self
            .omv_streams
            .get_mut(&path)
            .expect("omv stream state exists");
        let request_until = desired_before_drain
            .unwrap_or(0)
            .saturating_add(OMV_STREAM_DECODE_LEAD_FRAMES);
        state.request_frames.store(request_until, Ordering::Release);
        drain_omv_stream_state(path.as_path(), state, desired_before_drain)?;

        if state.frames.is_empty() {
            return Ok(None);
        }

        let latest_idx = state.decoded_frames.saturating_sub(1);
        let desired_idx = desired_before_drain.unwrap_or(latest_idx);
        let chosen_idx = desired_idx.min(latest_idx);
        let selected = state
            .frames
            .iter()
            .rev()
            .find(|(idx, _)| *idx <= chosen_idx)
            .or_else(|| state.frames.back())
            .map(|(idx, frame)| (*idx, frame.clone()));

        let Some((actual_frame_idx, frame)) = selected else {
            return Ok(None);
        };

        let video_total_ms = if state.done && state.decoded_frames > 0 {
            state
                .frame_time_ms
                .map(|ms| ((state.decoded_frames as f64) * ms).round() as u64)
                .or_else(|| {
                    state
                        .fps
                        .filter(|f| *f > 0.0)
                        .map(|fps| ((state.decoded_frames as f64) * 1000.0 / (fps as f64)).round() as u64)
                })
        } else {
            state.total_frames_hint.and_then(|frames| {
                state
                    .frame_time_ms
                    .map(|ms| ((frames as f64) * ms).round() as u64)
                    .or_else(|| {
                        state
                            .fps
                            .filter(|f| *f > 0.0)
                            .map(|fps| ((frames as f64) * 1000.0 / (fps as f64)).round() as u64)
                    })
            })
        };

        Ok(Some(MovieStreamFrame {
            frame,
            frame_idx: actual_frame_idx,
            fps: state.fps,
            total_ms: video_total_ms,
            audio: None,
            audio_ready: true,
            decoded_now: false,
            clamped_timer_ms: None,
        }))
    }

    pub fn ensure_preview_frame(&mut self, file_name: &str) -> Result<Arc<RgbaImage>> {
        let path = resolve_mov_path(&self.project_dir, &self.current_append_dir, file_name)?;
        self.ensure_preview_frame_for_path(path)
    }

    pub fn ensure_omv_preview_frame(&mut self, file_name: &str) -> Result<Arc<RgbaImage>> {
        let path = crate::resource::find_omv_path_with_append_dir(
            &self.project_dir,
            &self.current_append_dir,
            file_name,
        )?;
        self.ensure_preview_frame_for_path(path)
    }

    fn ensure_preview_frame_for_path(&mut self, path: PathBuf) -> Result<Arc<RgbaImage>> {
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
        let local_offset_ms = offset_ms.saturating_sub(track.start_ms);
        let remaining_ms = track
            .duration_ms
            .map(|d| d.saturating_sub(local_offset_ms.min(d)));
        let wav = encode_wav_i16_interleaved_offset(track, local_offset_ms);
        let data = StaticSoundData::from_cursor(Cursor::new(wav))
            .context("kira: decode movie WAV bytes")?;
        let handle = audio.play_static(TrackKind::Mov, data)?;
        let id = self.next_playback_id;
        self.next_playback_id = self.next_playback_id.saturating_add(1).max(1);
        self.playbacks.insert(
            id,
            MoviePlayback {
                handle,
                started_at: Instant::now(),
                duration_ms: remaining_ms,
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

    pub fn audio_playback_finished(&mut self, id: u64) -> bool {
        let Some(p) = self.playbacks.get(&id) else {
            return true;
        };
        let Some(duration_ms) = p.duration_ms else {
            return false;
        };
        if p.started_at.elapsed() >= Duration::from_millis(duration_ms) {
            if let Some(mut p) = self.playbacks.remove(&id) {
                let _ = p.handle.stop(kira::tween::Tween::default());
            }
            true
        } else {
            false
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

fn read_file_prefix(path: &Path, max_len: usize) -> Result<Vec<u8>> {
    let mut file = fs::File::open(path).with_context(|| format!("open file: {}", path.display()))?;
    let mut out = vec![0u8; max_len.max(1)];
    let n = file
        .read(&mut out)
        .with_context(|| format!("read file prefix: {}", path.display()))?;
    out.truncate(n);
    Ok(out)
}

fn frame_index_for_timer(timer_ms: u64, fps: f32, frame_count: usize) -> usize {
    if frame_count == 0 {
        return 0;
    }
    if fps <= 0.0 {
        return 0;
    }
    ((timer_ms as f64) * (fps as f64) / 1000.0).floor() as usize
}

fn spawn_mpeg2_stream_state(path: PathBuf) -> Result<Mpeg2StreamState> {
    let prefix = read_file_prefix(&path, MPEG2_HEADER_PROBE_BYTES)?;
    let mut width = None;
    let mut height = None;
    let mut fps = None;
    if let Some(h) = siglus_assets::mpeg2::find_sequence_header(&prefix) {
        width = Some(h.width as u32);
        height = Some(h.height as u32);
        fps = siglus_assets::mpeg2::fps_from_frame_rate_code(h.frame_rate_code);
    }

    let (tx, rx) = mpsc::sync_channel(MPEG2_STREAM_CHANNEL_CAPACITY);
    let request_frames = Arc::new(AtomicUsize::new(MPEG2_STREAM_DECODE_LEAD_FRAMES));
    let worker_request_frames = request_frames.clone();
    let video_path = path.clone();
    thread::spawn(move || {
        let result = stream_mpeg2_video_worker(video_path.as_path(), tx.clone(), worker_request_frames);
        if let Err(err) = result {
            let _ = tx.send(Err(format!("{:#}", err)));
        }
    });

    let (audio_tx, audio_rx) = mpsc::sync_channel(MPEG2_STREAM_CHANNEL_CAPACITY);
    let request_audio_until_ms = Arc::new(AtomicUsize::new(MOVIE_AUDIO_DECODE_LEAD_MS));
    let audio_request = request_audio_until_ms.clone();
    let audio_path = path;
    thread::spawn(move || {
        let result = stream_mpeg2_audio_worker(audio_path.as_path(), audio_tx.clone(), audio_request);
        if let Err(err) = result {
            let _ = audio_tx.send(Err(format!("{:#}", err)));
        }
    });

    Ok(Mpeg2StreamState {
        rx,
        audio_rx,
        frames: VecDeque::new(),
        width,
        height,
        fps,
        decoded_frames: 0,
        done: false,
        audio_segments: VecDeque::new(),
        audio_done: false,
        decoded_any_this_poll: false,
        request_frames,
        request_audio_until_ms,
    })
}

fn wait_for_mpeg2_frame_request(request_frames: &Arc<AtomicUsize>, frame_idx: usize) {
    while frame_idx > request_frames.load(Ordering::Acquire) {
        if request_frames.load(Ordering::Acquire) == usize::MAX {
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn wait_for_audio_request(request_audio_until_ms: &Arc<AtomicUsize>, segment_start_ms: u64) {
    loop {
        let limit = request_audio_until_ms.load(Ordering::Acquire);
        if limit == usize::MAX || (segment_start_ms as usize) <= limit {
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn stream_mpeg2_video_worker(
    path: &Path,
    tx: mpsc::SyncSender<Result<Mpeg2StreamEvent, String>>,
    request_frames: Arc<AtomicUsize>,
) -> Result<()> {
    let prefix = read_file_prefix(path, MPEG2_HEADER_PROBE_BYTES)?;
    let mut width = None;
    let mut height = None;
    let mut fps = None;
    if let Some(h) = siglus_assets::mpeg2::find_sequence_header(&prefix) {
        width = Some(h.width as u32);
        height = Some(h.height as u32);
        fps = siglus_assets::mpeg2::fps_from_frame_rate_code(h.frame_rate_code);
    }
    if tx
        .send(Ok(Mpeg2StreamEvent::Info { width, height, fps }))
        .is_err()
    {
        return Ok(());
    }

    let mut file = fs::File::open(path).with_context(|| format!("open movie file: {}", path.display()))?;
    let mut pipeline = na_mpeg2_decoder::MpegVideoPipeline::new();
    let mut buf = vec![0u8; MPEG2_STREAM_CHUNK_BYTES];
    let mut frame_idx = 0usize;
    let mut send_failed = false;

    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("read movie stream: {}", path.display()))?;
        if n == 0 {
            break;
        }
        pipeline
            .push_with(&buf[..n], None, |f| {
                if send_failed {
                    return;
                }
                wait_for_mpeg2_frame_request(&request_frames, frame_idx);
                if request_frames.load(Ordering::Acquire) == usize::MAX {
                    send_failed = true;
                    return;
                }
                let w = f.width as u32;
                let h = f.height as u32;
                let mut rgba = vec![0u8; (w as usize).saturating_mul(h as usize).saturating_mul(4)];
                na_mpeg2_decoder::frame_to_rgba_bt601_limited(&f, &mut rgba);
                let frame = Arc::new(RgbaImage {
                    width: w,
                    height: h,
                    rgba,
                });
                let ev = Mpeg2StreamEvent::Video { frame_idx, frame };
                frame_idx = frame_idx.saturating_add(1);
                if tx.send(Ok(ev)).is_err() {
                    send_failed = true;
                }
            })
            .context("mpeg2 stream video decode")?;
        if send_failed {
            return Ok(());
        }
    }

    pipeline.flush_with(|f| {
        if send_failed {
            return;
        }
        wait_for_mpeg2_frame_request(&request_frames, frame_idx);
        if request_frames.load(Ordering::Acquire) == usize::MAX {
            send_failed = true;
            return;
        }
        let w = f.width as u32;
        let h = f.height as u32;
        let mut rgba = vec![0u8; (w as usize).saturating_mul(h as usize).saturating_mul(4)];
        na_mpeg2_decoder::frame_to_rgba_bt601_limited(&f, &mut rgba);
        let frame = Arc::new(RgbaImage {
            width: w,
            height: h,
            rgba,
        });
        let ev = Mpeg2StreamEvent::Video { frame_idx, frame };
        frame_idx = frame_idx.saturating_add(1);
        if tx.send(Ok(ev)).is_err() {
            send_failed = true;
        }
    })?;

    if !send_failed {
        let _ = tx.send(Ok(Mpeg2StreamEvent::Done));
    }
    Ok(())
}

fn stream_mpeg2_audio_worker(
    path: &Path,
    tx: mpsc::SyncSender<Result<MovieAudioStreamEvent, String>>,
    request_audio_until_ms: Arc<AtomicUsize>,
) -> Result<()> {
    let mut audio_channels: Option<u16> = None;
    let mut audio_sample_rate: Option<u32> = None;
    let mut pending_samples: Vec<i16> = Vec::new();
    let mut segment_start_ms = 0u64;
    let mut dropped_audio_format_changes = 0u32;

    fn segment_sample_len(channels: u16, sample_rate: u32) -> usize {
        ((sample_rate as u64)
            .saturating_mul(channels as u64)
            .saturating_mul(MOVIE_AUDIO_SEGMENT_MS)
            / 1000) as usize
    }

    fn emit_ready_segments(
        tx: &mpsc::SyncSender<Result<MovieAudioStreamEvent, String>>,
        request_audio_until_ms: &Arc<AtomicUsize>,
        channels: u16,
        sample_rate: u32,
        pending_samples: &mut Vec<i16>,
        segment_start_ms: &mut u64,
    ) -> bool {
        let seg_len = segment_sample_len(channels, sample_rate).max(channels as usize);
        while pending_samples.len() >= seg_len {
            wait_for_audio_request(request_audio_until_ms, *segment_start_ms);
            if request_audio_until_ms.load(Ordering::Acquire) == usize::MAX {
                return false;
            }
            let tail = pending_samples.split_off(seg_len);
            let segment_samples = std::mem::replace(pending_samples, tail);
            let frames_len = (segment_samples.len() as u64) / (channels as u64);
            let duration_ms = Some(((frames_len as f64) * 1000.0 / sample_rate as f64).round() as u64);
            let audio = MovieAudio {
                samples: Arc::new(segment_samples),
                channels,
                sample_rate,
                start_ms: *segment_start_ms,
                duration_ms,
            };
            *segment_start_ms = (*segment_start_ms).saturating_add(duration_ms.unwrap_or(MOVIE_AUDIO_SEGMENT_MS));
            if tx.send(Ok(MovieAudioStreamEvent::Segment(audio))).is_err() {
                return false;
            }
        }
        true
    }

    fn append_chunk(
        path: &Path,
        phase: &str,
        audio_channels: &mut Option<u16>,
        audio_sample_rate: &mut Option<u32>,
        pending_samples: &mut Vec<i16>,
        dropped_audio_format_changes: &mut u32,
        a: na_mpeg2_decoder::MpegAudioF32,
    ) {
        match (*audio_channels, *audio_sample_rate) {
            (None, None) => {
                *audio_channels = Some(a.channels);
                *audio_sample_rate = Some(a.sample_rate);
            }
            (Some(ch), Some(sr)) if ch == a.channels && sr == a.sample_rate => {}
            (Some(ch), Some(sr)) => {
                *dropped_audio_format_changes = (*dropped_audio_format_changes).saturating_add(1);
                if std::env::var_os("SG_MOVIE_TRACE").is_some()
                    || std::env::var_os("SG_DEBUG").is_some()
                {
                    eprintln!(
                        "[SG_DEBUG][MOV] mpeg2_audio_format_change.drop phase={} path={} base={}ch/{}Hz got={}ch/{}Hz samples={}",
                        phase,
                        path.display(),
                        ch,
                        sr,
                        a.channels,
                        a.sample_rate,
                        a.samples.len()
                    );
                }
                return;
            }
            _ => return,
        }
        pending_samples.extend(a.samples.into_iter().map(f32_to_i16_sample));
    }

    let mut file = fs::File::open(path).with_context(|| format!("open movie file: {}", path.display()))?;
    let mut pipeline = na_mpeg2_decoder::MpegAvPipeline::new();
    let mut buf = vec![0u8; MPEG2_STREAM_CHUNK_BYTES];
    let mut keep_running = true;

    while keep_running {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("read movie audio stream: {}", path.display()))?;
        if n == 0 {
            break;
        }
        pipeline
            .push_with(&buf[..n], None, |ev| {
                if let na_mpeg2_decoder::MpegAvEvent::Audio(a) = ev {
                    append_chunk(
                        path,
                        "decode",
                        &mut audio_channels,
                        &mut audio_sample_rate,
                        &mut pending_samples,
                        &mut dropped_audio_format_changes,
                        a,
                    );
                    if let (Some(ch), Some(sr)) = (audio_channels, audio_sample_rate) {
                        keep_running = emit_ready_segments(
                            &tx,
                            &request_audio_until_ms,
                            ch,
                            sr,
                            &mut pending_samples,
                            &mut segment_start_ms,
                        );
                    }
                }
            })
            .context("mpeg2 audio decode")?;
    }

    pipeline.flush_with(|ev| {
        if let na_mpeg2_decoder::MpegAvEvent::Audio(a) = ev {
            append_chunk(
                path,
                "flush",
                &mut audio_channels,
                &mut audio_sample_rate,
                &mut pending_samples,
                &mut dropped_audio_format_changes,
                a,
            );
        }
    })?;

    if let (Some(channels), Some(sample_rate)) = (audio_channels, audio_sample_rate) {
        let _ = emit_ready_segments(
            &tx,
            &request_audio_until_ms,
            channels,
            sample_rate,
            &mut pending_samples,
            &mut segment_start_ms,
        );
        if !pending_samples.is_empty() {
            wait_for_audio_request(&request_audio_until_ms, segment_start_ms);
            if request_audio_until_ms.load(Ordering::Acquire) != usize::MAX {
                let frames_len = (pending_samples.len() as u64) / (channels as u64);
                let duration_ms = Some(((frames_len as f64) * 1000.0 / sample_rate as f64).round() as u64);
                let audio = MovieAudio {
                    samples: Arc::new(pending_samples),
                    channels,
                    sample_rate,
                    start_ms: segment_start_ms,
                    duration_ms,
                };
                let _ = tx.send(Ok(MovieAudioStreamEvent::Segment(audio)));
            }
        }
    }
    let _ = tx.send(Ok(MovieAudioStreamEvent::Done));
    Ok(())
}

fn drain_mpeg2_stream_state(
    path: &Path,
    state: &mut Mpeg2StreamState,
    target_frame_idx: Option<usize>,
    target_timer_ms: u64,
) -> Result<()> {
    state.decoded_any_this_poll = false;

    let decode_until = target_frame_idx
        .map(|idx| idx.saturating_add(MPEG2_STREAM_DECODE_LEAD_FRAMES));

    for _ in 0..MPEG2_STREAM_MAX_DRAIN_EVENTS {
        if let Some(limit) = decode_until {
            if state.decoded_frames > limit && !state.frames.is_empty() {
                break;
            }
        }
        match state.rx.try_recv() {
            Ok(Ok(Mpeg2StreamEvent::Info { width, height, fps })) => {
                state.width = width.or(state.width);
                state.height = height.or(state.height);
                state.fps = fps.or(state.fps);
            }
            Ok(Ok(Mpeg2StreamEvent::Video { frame_idx, frame })) => {
                state.decoded_frames = state.decoded_frames.max(frame_idx.saturating_add(1));
                state.frames.push_back((frame_idx, frame));
                state.decoded_any_this_poll = true;
            }
            Ok(Ok(Mpeg2StreamEvent::Done)) => {
                state.done = true;
                break;
            }
            Ok(Err(err)) => {
                return Err(anyhow!("mpeg2 stream decode failed for {}: {}", path.display(), err));
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                state.done = true;
                break;
            }
        }
    }

    if let Some(target) = target_frame_idx {
        let keep_from = target.saturating_sub(2);
        while state
            .frames
            .front()
            .map(|(idx, _)| *idx < keep_from)
            .unwrap_or(false)
        {
            state.frames.pop_front();
        }
    }
    while state.frames.len() > MPEG2_STREAM_FRAME_KEEP {
        state.frames.pop_front();
    }

    state.request_audio_until_ms.store(
        (target_timer_ms as usize).saturating_add(MOVIE_AUDIO_DECODE_LEAD_MS),
        Ordering::Release,
    );

    for _ in 0..MOVIE_AUDIO_MAX_DRAIN_EVENTS {
        match state.audio_rx.try_recv() {
            Ok(Ok(MovieAudioStreamEvent::Segment(audio))) => {
                state.audio_segments.push_back(audio);
            }
            Ok(Ok(MovieAudioStreamEvent::Done)) => {
                state.audio_done = true;
                break;
            }
            Ok(Err(err)) => {
                eprintln!("[SG_MOV] mpeg2 audio decode failed path={} err={}", path.display(), err);
                state.audio_done = true;
                break;
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                state.audio_done = true;
                break;
            }
        }
    }


    let keep_audio_from = target_timer_ms.saturating_sub(MOVIE_AUDIO_SEGMENT_MS.saturating_mul(2));
    while state
        .audio_segments
        .front()
        .map(|a| a.end_ms() < keep_audio_from)
        .unwrap_or(false)
    {
        state.audio_segments.pop_front();
    }

    Ok(())
}

fn select_audio_segment(segments: &VecDeque<MovieAudio>, timer_ms: u64) -> Option<MovieAudio> {
    segments
        .iter()
        .find(|a| timer_ms >= a.start_ms && timer_ms < a.end_ms().saturating_add(50))
        .cloned()
        .or_else(|| segments.front().filter(|a| timer_ms < a.end_ms()).cloned())
}

fn spawn_omv_stream_state(path: PathBuf) -> Result<OmvStreamState> {
    let (tx, rx) = mpsc::sync_channel(OMV_STREAM_CHANNEL_CAPACITY);
    let request_frames = Arc::new(AtomicUsize::new(OMV_STREAM_DECODE_LEAD_FRAMES));
    let worker_request_frames = request_frames.clone();
    thread::spawn(move || {
        let result = stream_omv_video_worker(path.as_path(), tx.clone(), worker_request_frames);
        if let Err(err) = result {
            let _ = tx.send(Err(format!("{:#}", err)));
        }
    });
    Ok(OmvStreamState {
        rx,
        frames: VecDeque::new(),
        width: None,
        height: None,
        fps: None,
        frame_time_ms: None,
        total_frames_hint: None,
        decoded_frames: 0,
        done: false,
        request_frames,
    })
}

fn wait_for_omv_frame_request(request_frames: &Arc<AtomicUsize>, frame_idx: usize) {
    while frame_idx > request_frames.load(Ordering::Acquire) {
        if request_frames.load(Ordering::Acquire) == usize::MAX {
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn stream_omv_video_worker(
    path: &Path,
    tx: mpsc::SyncSender<Result<OmvStreamEvent, String>>,
    request_frames: Arc<AtomicUsize>,
) -> Result<()> {
    let omv = siglus_assets::omv::OmvFile::open(path).ok();
    let ogg_data = siglus_assets::omv::OmvFile::read_embedded_ogg(path)
        .or_else(|_| extract_ogg_by_scan(path))
        .with_context(|| format!("read embedded ogg: {}", path.display()))?;
    let mut video_tf = siglus_omv_decoder::TheoraFile::open_from_memory(ogg_data)
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
    let frame_time_ms = omv_frame_duration_ms(omv.as_ref().map(|m| &m.header), fps);
    let total_frames_hint = omv
        .as_ref()
        .and_then(|m| (m.header.packet_count_hint > 0).then_some(m.header.packet_count_hint as usize));
    let theora_type = omv
        .as_ref()
        .map(|m| m.header.theora_type)
        .unwrap_or(siglus_assets::omv::OMV_THEORA_TYPE_YUV);

    if tx
        .send(Ok(OmvStreamEvent::Info {
            width,
            height,
            fps,
            frame_time_ms,
            total_frames_hint,
        }))
        .is_err()
    {
        return Ok(());
    }

    let (_uv_w, _uv_h, y_len, u_len, v_len) =
        omv_plane_layout(vinfo.width, vinfo.height, theora_type, vinfo.fmt);
    let mut buf = vec![0u8; y_len.saturating_add(u_len).saturating_add(v_len)];
    let mut frame_idx = 0usize;
    while video_tf.read_video_frame(&mut buf)? {
        wait_for_omv_frame_request(&request_frames, frame_idx);
        if request_frames.load(Ordering::Acquire) == usize::MAX {
            return Ok(());
        }
        let rgba = convert_omv_frame(
            &buf,
            vinfo.width,
            vinfo.height,
            vinfo.fmt,
            display_h,
            theora_type,
        );
        let frame = Arc::new(RgbaImage { width, height, rgba });
        if tx.send(Ok(OmvStreamEvent::Video { frame_idx, frame })).is_err() {
            return Ok(());
        }
        frame_idx = frame_idx.saturating_add(1);
    }

    let _ = tx.send(Ok(OmvStreamEvent::Done));
    Ok(())
}

fn drain_omv_stream_state(
    path: &Path,
    state: &mut OmvStreamState,
    target_frame_idx: Option<usize>,
) -> Result<()> {
    let decode_until = target_frame_idx
        .map(|idx| idx.saturating_add(OMV_STREAM_DECODE_LEAD_FRAMES));

    for _ in 0..OMV_STREAM_MAX_DRAIN_EVENTS {
        if let Some(limit) = decode_until {
            if state.decoded_frames > limit && !state.frames.is_empty() {
                break;
            }
        }
        match state.rx.try_recv() {
            Ok(Ok(OmvStreamEvent::Info { width, height, fps, frame_time_ms, total_frames_hint })) => {
                state.width = Some(width);
                state.height = Some(height);
                state.fps = fps.or(state.fps);
                state.frame_time_ms = frame_time_ms.or(state.frame_time_ms);
                state.total_frames_hint = total_frames_hint.or(state.total_frames_hint);
            }
            Ok(Ok(OmvStreamEvent::Video { frame_idx, frame })) => {
                state.decoded_frames = state.decoded_frames.max(frame_idx.saturating_add(1));
                state.frames.push_back((frame_idx, frame));
            }
            Ok(Ok(OmvStreamEvent::Done)) => {
                state.done = true;
                break;
            }
            Ok(Err(err)) => {
                return Err(anyhow!("omv stream decode failed for {}: {}", path.display(), err));
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                state.done = true;
                break;
            }
        }
    }

    if let Some(target) = target_frame_idx {
        let keep_from = target.saturating_sub(2);
        while state
            .frames
            .front()
            .map(|(idx, _)| *idx < keep_from)
            .unwrap_or(false)
        {
            state.frames.pop_front();
        }
    }
    while state.frames.len() > OMV_STREAM_FRAME_KEEP {
        state.frames.pop_front();
    }
    Ok(())
}

fn omv_frame_duration_ms(
    header: Option<&siglus_assets::omv::OmvHeader>,
    fps: Option<f32>,
) -> Option<f64> {
    if let Some(h) = header {
        if h.frame_time_us != 0 {
            return Some((h.frame_time_us as f64) / 1000.0);
        }
    }
    let f = fps?;
    if f > 0.0 {
        Some(1000.0 / (f as f64))
    } else {
        None
    }
}

fn omv_plane_layout(
    width: i32,
    video_height: i32,
    theora_type: u32,
    fmt: i32,
) -> (usize, usize, usize, usize, usize) {
    let w = width.max(1) as usize;
    let vh = video_height.max(1) as usize;
    match theora_type {
        siglus_assets::omv::OMV_THEORA_TYPE_RGB | siglus_assets::omv::OMV_THEORA_TYPE_RGBA => {
            // OMV RGB/RGBA is not YCbCr even though it is carried by a Theora 4:4:4 stream.
            // Original tona3 copies three full-size planes as B, G, R.  RGBA stores alpha
            // in hidden rows below the visible picture area, split across those same planes.
            let plane_len = w.saturating_mul(vh);
            (w, vh, plane_len, plane_len, plane_len)
        }
        _ => {
            let y_len = w.saturating_mul(vh);
            let (uv_w, uv_h) = yuv_plane_size(width, video_height, fmt);
            let uv_len = uv_w.saturating_mul(uv_h);
            (uv_w, uv_h, y_len, uv_len, uv_len)
        }
    }
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
    pub start_ms: u64,
    pub duration_ms: Option<u64>,
}

impl MovieAudio {
    fn end_ms(&self) -> u64 {
        self.start_ms.saturating_add(self.duration_ms.unwrap_or(0))
    }
}

#[derive(Debug)]
struct MoviePlayback {
    handle: StaticSoundHandle,
    started_at: Instant,
    duration_ms: Option<u64>,
}

fn decode_asset_for_path(path: &Path) -> Result<MovieAsset> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext == "omv" {
        decode_omv_asset(path)
    } else {
        decode_mpeg2_asset(path)
    }
}

fn decode_mpeg2_preview_frame(path: &Path) -> Result<Arc<RgbaImage>> {
    let mut file = fs::File::open(path).with_context(|| format!("open movie file: {}", path.display()))?;
    let mut pipeline = na_mpeg2_decoder::MpegVideoPipeline::new();
    let mut first = None;
    let mut buf = vec![0u8; MPEG2_STREAM_CHUNK_BYTES];
    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("read movie preview stream: {}", path.display()))?;
        if n == 0 {
            break;
        }
        pipeline
            .push_with(&buf[..n], None, |f| {
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
        if first.is_some() {
            break;
        }
    }
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
    let rgba = convert_omv_frame(
        &packed,
        vinfo.width,
        vinfo.height,
        vinfo.fmt,
        display_h,
        omv.as_ref()
            .map(|m| m.header.theora_type)
            .unwrap_or(siglus_assets::omv::OMV_THEORA_TYPE_YUV),
    );
    Ok(Arc::new(RgbaImage {
        width,
        height,
        rgba,
    }))
}

fn decode_mpeg2_audio_for_path(path: &Path) -> Result<Option<MovieAudio>> {
    let mut audio_samples: Vec<i16> = Vec::new();
    let mut audio_channels: Option<u16> = None;
    let mut audio_sample_rate: Option<u32> = None;
    let mut dropped_audio_format_changes: u32 = 0;

    fn append_chunk(
        path: &Path,
        phase: &str,
        audio_channels: &mut Option<u16>,
        audio_sample_rate: &mut Option<u32>,
        audio_samples: &mut Vec<i16>,
        dropped_audio_format_changes: &mut u32,
        a: na_mpeg2_decoder::MpegAudioF32,
    ) {
        match (*audio_channels, *audio_sample_rate) {
            (None, None) => {
                *audio_channels = Some(a.channels);
                *audio_sample_rate = Some(a.sample_rate);
            }
            (Some(ch), Some(sr)) if ch == a.channels && sr == a.sample_rate => {}
            (Some(ch), Some(sr)) => {
                *dropped_audio_format_changes = (*dropped_audio_format_changes).saturating_add(1);
                if std::env::var_os("SG_MOVIE_TRACE").is_some()
                    || std::env::var_os("SG_DEBUG").is_some()
                {
                    eprintln!(
                        "[SG_DEBUG][MOV] mpeg2_audio_format_change.drop phase={} path={} base={}ch/{}Hz got={}ch/{}Hz samples={}",
                        phase,
                        path.display(),
                        ch,
                        sr,
                        a.channels,
                        a.sample_rate,
                        a.samples.len()
                    );
                }
                return;
            }
            _ => return,
        }
        audio_samples.extend(a.samples.into_iter().map(f32_to_i16_sample));
    }

    let mut file = fs::File::open(path).with_context(|| format!("open movie file: {}", path.display()))?;
    let mut pipeline = na_mpeg2_decoder::MpegAvPipeline::new();
    let mut buf = vec![0u8; MPEG2_STREAM_CHUNK_BYTES];

    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("read movie audio stream: {}", path.display()))?;
        if n == 0 {
            break;
        }
        pipeline
            .push_with(&buf[..n], None, |ev| {
                if let na_mpeg2_decoder::MpegAvEvent::Audio(a) = ev {
                    append_chunk(
                        path,
                        "decode",
                        &mut audio_channels,
                        &mut audio_sample_rate,
                        &mut audio_samples,
                        &mut dropped_audio_format_changes,
                        a,
                    );
                }
            })
            .context("mpeg2 audio decode")?;
    }

    pipeline.flush_with(|ev| {
        if let na_mpeg2_decoder::MpegAvEvent::Audio(a) = ev {
            append_chunk(
                path,
                "flush",
                &mut audio_channels,
                &mut audio_sample_rate,
                &mut audio_samples,
                &mut dropped_audio_format_changes,
                a,
            );
        }
    })?;

    match (audio_channels, audio_sample_rate, audio_samples.is_empty()) {
        (Some(channels), Some(sample_rate), false) => {
            if channels == 0 || sample_rate == 0 {
                bail!(
                    "mpeg2 audio stream has invalid format in {}: channels={} sample_rate={}",
                    path.display(), channels, sample_rate
                );
            }
            let frames_len = (audio_samples.len() as u64) / (channels as u64);
            let duration_ms = Some(
                ((frames_len as f64) * 1000.0 / sample_rate as f64).round() as u64,
            );
            Ok(Some(MovieAudio {
                samples: Arc::new(audio_samples),
                channels,
                sample_rate,
                start_ms: 0,
                duration_ms,
            }))
        }
        (None, None, true) => Ok(None),
        (Some(_), Some(_), true) => Ok(None),
        _ => bail!(
            "mpeg2 audio decoder produced incomplete format metadata for {}",
            path.display()
        ),
    }
}

fn decode_mpeg2_asset(path: &Path) -> Result<MovieAsset> {
    let prefix = read_file_prefix(path, MPEG2_HEADER_PROBE_BYTES)
        .with_context(|| format!("read movie header: {}", path.display()))?;
    let mut width = None;
    let mut height = None;
    let mut fps = None;
    if let Some(h) = siglus_assets::mpeg2::find_sequence_header(&prefix) {
        width = Some(h.width as u32);
        height = Some(h.height as u32);
        fps = siglus_assets::mpeg2::fps_from_frame_rate_code(h.frame_rate_code);
    }
    let frame = decode_mpeg2_preview_frame(path)?;
    let info = MovieInfo {
        path: path.to_path_buf(),
        width: width.or(Some(frame.width)),
        height: height.or(Some(frame.height)),
        fps,
        decoded_frames: Some(1),
        audio_duration_ms: None,
    };
    Ok(MovieAsset {
        info,
        frames: vec![frame],
        audio: None,
    })
}

fn f32_to_i16_sample(s: f32) -> i16 {
    let clamped = s.max(-1.0).min(1.0);
    (clamped * 32767.0).round() as i16
}

fn decode_omv_asset(path: &Path) -> Result<MovieAsset> {
    let omv = siglus_assets::omv::OmvFile::open(path).ok();
    let frame = decode_omv_preview_frame(path)?;
    let fps = omv.as_ref().and_then(|m| {
        if m.header.frame_time_us != 0 {
            Some(1_000_000.0 / (m.header.frame_time_us as f32))
        } else {
            None
        }
    });
    let decoded_frames = omv
        .as_ref()
        .and_then(|m| (m.header.packet_count_hint > 0).then_some(m.header.packet_count_hint as usize))
        .or(Some(1));
    let audio_duration_ms = decoded_frames.and_then(|frames| {
        omv_frame_duration_ms(omv.as_ref().map(|m| &m.header), fps)
            .map(|ms| ((frames as f64) * ms).round().max(1.0) as u64)
    });
    let info = MovieInfo {
        path: path.to_path_buf(),
        width: Some(frame.width),
        height: Some(frame.height),
        fps,
        decoded_frames,
        audio_duration_ms,
    };
    Ok(MovieAsset {
        info,
        frames: vec![frame],
        audio: None,
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
        start_ms: 0,
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
    theora_type: u32,
) -> Vec<u8> {
    let w = width.max(1) as usize;
    let vh = video_height.max(1) as usize;
    let dh = display_height.max(1) as usize;

    let (uv_w, _uv_h, y_plane_len, u_plane_len, _v_plane_len) =
        omv_plane_layout(width, video_height, theora_type, fmt);
    let y_off = 0usize;
    let u_off = y_off.saturating_add(y_plane_len);
    let v_off = u_off.saturating_add(u_plane_len);

    let mut rgba = vec![0u8; w.saturating_mul(dh).saturating_mul(4)];

    match theora_type {
        siglus_assets::omv::OMV_THEORA_TYPE_RGB => {
            for y in 0..dh {
                for x in 0..w {
                    let b = get_plane_sample(data, y_off, w, x, y, 0);
                    let g = get_plane_sample(data, u_off, uv_w, x, y, 0);
                    let r = get_plane_sample(data, v_off, uv_w, x, y, 0);
                    let out = (y * w + x) * 4;
                    rgba[out] = r;
                    rgba[out + 1] = g;
                    rgba[out + 2] = b;
                    rgba[out + 3] = 0xff;
                }
            }
        }
        siglus_assets::omv::OMV_THEORA_TYPE_RGBA => {
            let alpha_h = (dh + 2) / 3;
            let alpha_h_2 = alpha_h * 2;
            for y in 0..dh {
                let (a_off, local_y, a_width) = if y < alpha_h {
                    (y_off, y, w)
                } else if y < alpha_h_2 {
                    (u_off, y - alpha_h, uv_w)
                } else {
                    (v_off, y - alpha_h_2, uv_w)
                };
                let alpha_y = dh.saturating_add(local_y);
                for x in 0..w {
                    let b = get_plane_sample(data, y_off, w, x, y, 0);
                    let g = get_plane_sample(data, u_off, uv_w, x, y, 0);
                    let r = get_plane_sample(data, v_off, uv_w, x, y, 0);
                    let a = get_plane_sample(data, a_off, a_width, x, alpha_y, 0xff);
                    let out = (y * w + x) * 4;
                    rgba[out] = r;
                    rgba[out + 1] = g;
                    rgba[out + 2] = b;
                    rgba[out + 3] = a;
                }
            }
        }
        _ => {
            for y in 0..dh {
                let y_row = y * w;
                let uv_y = match fmt {
                    siglus_omv_decoder::TH_PF_420 => y / 2,
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

                    let r = clamp_f(yv + 1.40200 * v);
                    let g = clamp_f(yv - 0.34414 * u - 0.71414 * v);
                    let b = clamp_f(yv + 1.77200 * u);

                    let out = (y * w + x) * 4;
                    rgba[out] = r;
                    rgba[out + 1] = g;
                    rgba[out + 2] = b;
                    rgba[out + 3] = 0xff;
                }
            }
        }
    }

    rgba
}

fn get_plane_sample(
    data: &[u8],
    plane_off: usize,
    plane_width: usize,
    x: usize,
    y: usize,
    default: u8,
) -> u8 {
    if plane_width == 0 {
        return default;
    }
    data.get(
        plane_off
            .saturating_add(y.saturating_mul(plane_width))
            .saturating_add(x),
    )
    .copied()
    .unwrap_or(default)
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
