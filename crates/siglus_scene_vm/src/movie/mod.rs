use std::collections::{HashMap, VecDeque};
use std::fs;
use std::io::{BufReader, Cursor, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{
    mpsc::{self, Receiver, TryRecvError},
    Arc,
};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use crate::platform_time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
#[cfg(not(target_arch = "wasm32"))]
use kira::sound::streaming::{Decoder as KiraStreamingDecoder, StreamingSoundData, StreamingSoundHandle};
use kira::sound::PlaybackState;
#[cfg(not(target_arch = "wasm32"))]
use kira::Frame;

use crate::assets::RgbaImage;
use crate::audio::{AudioHub, TrackKind};

const MPEG2_HEADER_PROBE_BYTES: usize = 256 * 1024;
const MPEG2_STREAM_CHUNK_BYTES: usize = 256 * 1024;
const MPEG_AUDIO_HEAD_PROBE_MAX_BYTES: u64 = 16 * 1024 * 1024;
const MPEG_AUDIO_TAIL_PROBE_INITIAL_BYTES: u64 = 4 * 1024 * 1024;
const MPEG_AUDIO_TAIL_PROBE_MAX_BYTES: u64 = 32 * 1024 * 1024;
const MPEG_AUDIO_SEEK_INITIAL_BACKTRACK_BYTES: u64 = 512 * 1024;
const MPEG_AUDIO_SEEK_MAX_PRIME_BYTES: u64 = 16 * 1024 * 1024;
const MPEG_VIDEO_SEEK_BACKTRACK_BYTES: u64 = 8 * 1024 * 1024;
const MPEG_VIDEO_SEEK_FORWARD_PROBE_BYTES: u64 = 1024 * 1024;
const MPEG2_STREAM_CHANNEL_CAPACITY: usize = 4;
const MPEG2_STREAM_MAX_DRAIN_EVENTS: usize = 8;
const MPEG2_STREAM_FRAME_KEEP: usize = 6;
const MPEG2_STREAM_DECODE_LEAD_FRAMES: usize = 3;
const OMV_STREAM_CHANNEL_CAPACITY: usize = 12;
const OMV_STREAM_MAX_DRAIN_EVENTS: usize = 16;
const OMV_STREAM_FRAME_KEEP: usize = 16;
const OMV_STREAM_DECODE_LEAD_FRAMES: usize = 4;
const OMV_LOOP_HEAD_CACHE_MAX_FRAMES: usize = 60;
const OMV_LOOP_HEAD_CACHE_MAX_BYTES: usize = 64 * 1024 * 1024;
const WMV_STREAM_CHANNEL_CAPACITY: usize = 8;
const WMV_STREAM_MAX_DRAIN_EVENTS: usize = 16;
const WMV_STREAM_FRAME_KEEP: usize = 12;
const WMV_STREAM_DECODE_LEAD_MS: usize = 750;

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
    /// True when `frame_idx` is the coded/decode-order index rather than a
    /// monotonically increasing presentation-order index. WMV3 with B pictures
    /// has this property even though frame selection itself is PTS ordered.
    pub frame_idx_is_decode_order: bool,
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
        pts_90k: Option<i64>,
        frame: Arc<RgbaImage>,
    },
    Done,
}

#[derive(Clone)]
struct Mpeg2DecodedFrame {
    frame_idx: usize,
    pts_90k: Option<i64>,
    frame: Arc<RgbaImage>,
}

struct Mpeg2StreamState {
    rx: Receiver<Result<Mpeg2StreamEvent, String>>,
    frames: VecDeque<Mpeg2DecodedFrame>,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f32>,
    decoded_frames: usize,
    first_video_pts_90k: Option<i64>,
    last_video_timeline_ms: Option<u64>,
    last_requested_timer_ms: u64,
    done: bool,
    audio: Option<MovieAudio>,
    decoded_any_this_poll: bool,
    request_frames: Arc<AtomicUsize>,
}

impl Drop for Mpeg2StreamState {
    fn drop(&mut self) {
        self.request_frames.store(usize::MAX, Ordering::Release);
    }
}

struct Mpeg2AudioTask {
    rx: Receiver<Result<Option<MovieAudio>, String>>,
    cancel: Arc<AtomicBool>,
}

impl Drop for Mpeg2AudioTask {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

enum WmvStreamEvent {
    Info {
        width: u32,
        height: u32,
    },
    Video {
        frame_idx: usize,
        source_pts_ms: u64,
        frame: Arc<RgbaImage>,
    },
    Done,
}

#[derive(Clone)]
struct WmvDecodedFrame {
    frame_idx: usize,
    /// ASF presentation timestamp, after ASF preroll handling but before
    /// rebasing to the movie-local clock. WMV3/VC-1 decode order is not PTS
    /// order when B pictures are present, so keep this raw until the consumer
    /// has sorted the presentation queue.
    source_pts_ms: u64,
    frame: Arc<RgbaImage>,
}

struct WmvStreamState {
    rx: Receiver<Result<WmvStreamEvent, String>>,
    frames: VecDeque<WmvDecodedFrame>,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f32>,
    decoded_frames: usize,
    timeline_origin_ms: Option<u64>,
    last_video_source_pts_ms: Option<u64>,
    last_frame_delta_ms: Option<u64>,
    total_ms_hint: Option<u64>,
    last_effective_timer_ms: u64,
    done: bool,
    audio: Option<MovieAudio>,
    decoded_any_this_poll: bool,
    request_ms: Arc<AtomicUsize>,
}

impl Drop for WmvStreamState {
    fn drop(&mut self) {
        self.request_ms.store(usize::MAX, Ordering::Release);
    }
}

struct WmvAudioTask {
    rx: Receiver<Result<Option<MovieAudio>, String>>,
    cancel: Arc<AtomicBool>,
}

impl Drop for WmvAudioTask {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}

enum OmvStreamEvent {
    Info {
        width: u32,
        height: u32,
        fps: Option<f32>,
        frame_time_ms: Option<f64>,
        total_frames_hint: Option<usize>,
        total_ms_hint: Option<u64>,
        frame_times: Arc<Vec<(u64, u64)>>,
    },
    Reset {
        frame_idx: usize,
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
    loop_head_frames: VecDeque<(usize, Arc<RgbaImage>)>,
    loop_head_bytes: usize,
    width: Option<u32>,
    height: Option<u32>,
    fps: Option<f32>,
    frame_time_ms: Option<f64>,
    total_frames_hint: Option<usize>,
    total_ms_hint: Option<u64>,
    frame_times: Option<Arc<Vec<(u64, u64)>>>,
    decoded_frames: usize,
    done: bool,
    request_frame: Arc<AtomicUsize>,
    /// Highest frame index this poll loop has served to the consumer. Requests
    /// are clamped to never go backward from here so timer jitter cannot force
    /// an indexed seek (which rewinds the stream and visibly flashes).
    last_served_frame_idx: usize,
    /// Effective timer value of the previous poll, used to detect genuine
    /// loop wraps that legitimately rewind the stream.
    last_effective_timer_ms: u64,
    /// The frame served last. When a loop wrap forces the worker to rewind
    /// and re-decode from a key frame, the live queue lags behind the timer
    /// again; serving its fast-catch-up tail would flash stale dark head
    /// frames. Hold this frame until the stream catches back up.
    held_frame: Option<(usize, Arc<RgbaImage>)>,
}

impl Drop for OmvStreamState {
    fn drop(&mut self) {
        self.request_frame.store(usize::MAX, Ordering::Release);
    }
}

/// Minimal movie state holder.
///
/// The original Siglus engine plays MOV via a native playback pipeline.
/// Here we provide a deterministic, cross-platform metadata path:
/// - MPEG2 (`.mpg` / `.mpeg`) via `siglus_assets::mpeg2`
/// - ASF/WMV (`.wmv`) via `wmv_decoder`
/// - OMV (`.omv`) via `siglus_assets::omv`
pub struct MovieManager {
    project_dir: PathBuf,
    current_append_dir: String,
    current: Option<MovieInfo>,
    cache: HashMap<PathBuf, MovieAsset>,
    preview_cache: HashMap<PathBuf, Arc<RgbaImage>>,
    decode_tasks: HashMap<PathBuf, Receiver<Result<MovieAsset, String>>>,
    mpeg2_audio_cache: HashMap<PathBuf, Option<MovieAudio>>,
    mpeg2_audio_tasks: HashMap<PathBuf, Mpeg2AudioTask>,
    mpeg2_streams: HashMap<PathBuf, Mpeg2StreamState>,
    wmv_audio_cache: HashMap<PathBuf, Option<MovieAudio>>,
    wmv_audio_tasks: HashMap<PathBuf, WmvAudioTask>,
    wmv_streams: HashMap<PathBuf, WmvStreamState>,
    omv_streams: HashMap<PathBuf, OmvStreamState>,
    playbacks: HashMap<u64, MoviePlayback>,
    next_playback_id: u64,
    /// Resolved movie paths keyed by (append_dir, file_name). The per-frame
    /// movie poll re-resolves the path on every frame, and on Android this
    /// translates into slow FUSE directory scans.
    path_cache: HashMap<(String, String), PathBuf>,
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
            .field("mpeg2_audio_cache_len", &self.mpeg2_audio_cache.len())
            .field("mpeg2_audio_tasks_len", &self.mpeg2_audio_tasks.len())
            .field("mpeg2_streams_len", &self.mpeg2_streams.len())
            .field("wmv_audio_cache_len", &self.wmv_audio_cache.len())
            .field("wmv_audio_tasks_len", &self.wmv_audio_tasks.len())
            .field("wmv_streams_len", &self.wmv_streams.len())
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
            mpeg2_audio_cache: HashMap::new(),
            mpeg2_audio_tasks: HashMap::new(),
            mpeg2_streams: HashMap::new(),
            wmv_audio_cache: HashMap::new(),
            wmv_audio_tasks: HashMap::new(),
            wmv_streams: HashMap::new(),
            omv_streams: HashMap::new(),
            playbacks: HashMap::new(),
            next_playback_id: 1,
            path_cache: HashMap::new(),
        }
    }

    pub fn current(&self) -> Option<&MovieInfo> {
        self.current.as_ref()
    }

    pub fn set_current_append_dir(&mut self, append_dir: impl Into<String>) {
        let append_dir = append_dir.into();
        if self.current_append_dir != append_dir {
            self.current_append_dir = append_dir;
        }
    }

    pub fn set_current_append_dir_ref(&mut self, append_dir: &str) {
        if self.current_append_dir != append_dir {
            self.current_append_dir.clear();
            self.current_append_dir.push_str(append_dir);
        }
    }

    pub fn stop(&mut self) {
        self.current = None;
        self.mpeg2_streams.clear();
        self.mpeg2_audio_tasks.clear();
        self.wmv_streams.clear();
        self.wmv_audio_tasks.clear();
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
        let header = read_omv_header_for_path(&path)
            .with_context(|| format!("open OMV: {}", path.display()))?;
        let w = header.display_width;
        let h = header.display_height;
        let fps = if header.frame_time_us != 0 {
            Some(1_000_000.0 / (header.frame_time_us as f32))
        } else {
            None
        };
        let info = MovieInfo {
            path,
            width: (w > 0).then_some(w),
            height: (h > 0).then_some(h),
            fps,
            decoded_frames: (header.packet_count_hint > 0)
                .then_some(header.packet_count_hint as usize),
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
            let header = read_omv_header_for_path(&path)
                .with_context(|| format!("open OMV: {}", path.display()))?;
            let w = header.display_width;
            let h = header.display_height;
            let fps = if header.frame_time_us != 0 {
                Some(1_000_000.0 / (header.frame_time_us as f32))
            } else {
                None
            };
            MovieInfo {
                path,
                width: (w > 0).then_some(w),
                height: (h > 0).then_some(h),
                fps,
                decoded_frames: (header.packet_count_hint > 0)
                    .then_some(header.packet_count_hint as usize),
                audio_duration_ms: None,
            }
        } else if is_wmv_extension(&ext) {
            probe_wmv_movie_info(&path)?
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
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            {
                let asset = decode_asset_for_path(&path)?;
                self.cache.insert(path.clone(), asset);
                let asset = self.cache.get(&path).expect("asset cached");
                return Ok(Some((asset, true)));
            }
            #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
            {
                let (tx, rx) = mpsc::channel();
                let worker_path = path.clone();
                thread::spawn(move || {
                    let result = decode_asset_for_path(&worker_path).map_err(|e| format!("{:#}", e));
                    let _ = tx.send(result);
                });
                self.decode_tasks.insert(path.clone(), rx);
            }
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
        self.poll_global_movie_frame_with_loop(file_name, timer_ms, false)
    }

    pub fn poll_global_movie_frame_with_loop(
        &mut self,
        file_name: &str,
        timer_ms: u64,
        loop_flag: bool,
    ) -> Result<Option<MovieStreamFrame>> {
        let path = self.resolve_mov_path_cached(file_name)?;
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            return self.poll_cached_movie_frame_for_path_with_loop(path, timer_ms, loop_flag);
        }
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext == "omv" {
            return self.poll_omv_stream_frame_for_path(path, timer_ms, loop_flag);
        }
        if is_wmv_extension(&ext) {
            return self.poll_wmv_stream_frame_for_path(path, timer_ms, loop_flag);
        }
        self.poll_mpeg2_stream_frame_for_path(path, timer_ms)
    }

    fn resolve_mov_path_cached(&mut self, file_name: &str) -> Result<PathBuf> {
        let key = (self.current_append_dir.clone(), file_name.to_string());
        if let Some(path) = self.path_cache.get(&key) {
            return Ok(path.clone());
        }
        let path = resolve_mov_path(&self.project_dir, &self.current_append_dir, file_name)?;
        self.path_cache.insert(key, path.clone());
        Ok(path)
    }

    fn poll_cached_movie_frame_for_path(
        &mut self,
        path: PathBuf,
        timer_ms: u64,
    ) -> Result<Option<MovieStreamFrame>> {
        self.poll_cached_movie_frame_for_path_with_loop(path, timer_ms, false)
    }

    fn poll_cached_movie_frame_for_path_with_loop(
        &mut self,
        path: PathBuf,
        timer_ms: u64,
        loop_flag: bool,
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
        let effective_timer_ms = if loop_flag {
            asset
                .info
                .duration_ms()
                .filter(|ms| *ms > 0)
                .map(|ms| timer_ms % ms)
                .unwrap_or(timer_ms)
        } else {
            timer_ms
        };
        let mut idx = frame_index_for_timer(effective_timer_ms, fps, asset.frames.len());
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
            frame_idx_is_decode_order: false,
            clamped_timer_ms: None,
        }))
    }

    fn poll_mpeg2_stream_frame_for_path(
        &mut self,
        path: PathBuf,
        timer_ms: u64,
    ) -> Result<Option<MovieStreamFrame>> {
        let (audio, audio_ready) = self.poll_mpeg2_audio_for_path(path.as_path());
        if !self.mpeg2_streams.contains_key(&path) {
            let state = spawn_mpeg2_stream_state(path.clone(), audio.clone(), timer_ms)?;
            self.mpeg2_streams.insert(path.clone(), state);
        } else if audio_ready {
            if let Some(state) = self.mpeg2_streams.get_mut(&path) {
                state.audio = audio.clone();
                reindex_mpeg_stream_frames(state);
            }
        }

        let desired_frame_idx = self.mpeg2_streams.get(&path).and_then(|state| {
            state
                .fps
                .filter(|f| *f > 0.0)
                .map(|fps| ((timer_ms as f64) * (fps as f64) / 1000.0).floor() as usize)
        });
        let restart_stream = self
            .mpeg2_streams
            .get(&path)
            .is_some_and(|state| mpeg_stream_needs_restart(state, timer_ms));
        if restart_stream {
            self.mpeg2_streams.remove(&path);
            let state = spawn_mpeg2_stream_state(path.clone(), audio.clone(), timer_ms)?;
            self.mpeg2_streams.insert(path.clone(), state);
        }

        let state = self
            .mpeg2_streams
            .get_mut(&path)
            .expect("mpeg2 stream state exists");
        state.last_requested_timer_ms = timer_ms;
        let request_until = desired_frame_idx
            .unwrap_or(0)
            .saturating_add(MPEG2_STREAM_DECODE_LEAD_FRAMES);
        state.request_frames.store(request_until, Ordering::Release);
        drain_mpeg2_stream_state(path.as_path(), state, desired_frame_idx, timer_ms)?;

        if state.frames.is_empty() {
            return Ok(None);
        }

        let Some(chosen) = select_mpeg_stream_frame(
            &state.frames,
            state,
            timer_ms,
            desired_frame_idx,
        )
        .cloned()
        else {
            return Ok(None);
        };

        let video_total_ms = if state.done {
            state.last_video_timeline_ms.map(|last| {
                let frame_ms = state
                    .fps
                    .filter(|fps| *fps > 0.0)
                    .map(|fps| (1000.0 / fps as f64).round() as u64)
                    .unwrap_or(0);
                last.saturating_add(frame_ms)
            })
        } else {
            None
        };
        let audio_total_ms = state.audio.as_ref().map(|a| a.end_ms());
        let total_ms = match (audio_total_ms, video_total_ms) {
            (Some(a), Some(v)) => Some(a.max(v)),
            (Some(a), None) => Some(a),
            (None, Some(v)) => Some(v),
            (None, None) => None,
        };

        let audio = state
            .audio
            .as_ref()
            .filter(|track| timer_ms < track.end_ms())
            .cloned();
        let decoded_now = state.decoded_any_this_poll;
        state.decoded_any_this_poll = false;

        Ok(Some(MovieStreamFrame {
            frame: chosen.frame,
            frame_idx: chosen.frame_idx,
            fps: state.fps,
            total_ms,
            audio,
            audio_ready,
            decoded_now,
            frame_idx_is_decode_order: false,
            clamped_timer_ms: None,
        }))
    }

    fn poll_mpeg2_audio_for_path(&mut self, path: &Path) -> (Option<MovieAudio>, bool) {
        if let Some(audio) = self.mpeg2_audio_cache.get(path) {
            return (audio.clone(), true);
        }

        let mut completed = None;
        let mut failed = None;
        if let Some(task) = self.mpeg2_audio_tasks.get(path) {
            match task.rx.try_recv() {
                Ok(Ok(audio)) => completed = Some(audio),
                Ok(Err(err)) => failed = Some(err),
                Err(TryRecvError::Empty) => return (None, false),
                Err(TryRecvError::Disconnected) => {
                    failed = Some(format!(
                        "mpeg2 audio worker disconnected: {}",
                        path.display()
                    ));
                }
            }
        } else {
            let (tx, rx) = mpsc::channel();
            let cancel = Arc::new(AtomicBool::new(false));
            let worker_cancel = cancel.clone();
            let worker_path = path.to_path_buf();
            thread::spawn(move || {
                let result = decode_mpeg2_audio_for_path(&worker_path, worker_cancel.as_ref())
                    .map_err(|err| format!("{:#}", err));
                let _ = tx.send(result);
            });
            self.mpeg2_audio_tasks.insert(
                path.to_path_buf(),
                Mpeg2AudioTask { rx, cancel },
            );
            return (None, false);
        }

        self.mpeg2_audio_tasks.remove(path);
        if let Some(err) = failed {
            eprintln!(
                "[SG_MOV] mpeg2 audio decode failed path={} err={}",
                path.display(),
                err
            );
            self.mpeg2_audio_cache.insert(path.to_path_buf(), None);
            return (None, true);
        }
        let audio = completed.unwrap_or(None);
        self.mpeg2_audio_cache
            .insert(path.to_path_buf(), audio.clone());
        (audio, true)
    }

    fn poll_wmv_stream_frame_for_path(
        &mut self,
        path: PathBuf,
        timer_ms: u64,
        loop_flag: bool,
    ) -> Result<Option<MovieStreamFrame>> {
        let (audio, audio_ready) = self.poll_wmv_audio_for_path(path.as_path());
        if !self.wmv_streams.contains_key(&path) {
            let state = spawn_wmv_stream_state(path.clone(), audio.clone(), timer_ms)?;
            self.wmv_streams.insert(path.clone(), state);
        } else if audio_ready {
            if let Some(state) = self.wmv_streams.get_mut(&path) {
                state.audio = audio.clone();
            }
        }

        let duration_hint = self.wmv_streams.get(&path).and_then(|state| {
            let video_total = state.total_ms_hint.or_else(|| {
                if state.done {
                    match (state.timeline_origin_ms, state.last_video_source_pts_ms) {
                        (Some(origin), Some(last)) => Some(
                            last.saturating_sub(origin)
                                .saturating_add(state.last_frame_delta_ms.unwrap_or(0)),
                        ),
                        _ => None,
                    }
                } else {
                    None
                }
            });
            let audio_total = state.audio.as_ref().map(MovieAudio::end_ms);
            match (video_total, audio_total) {
                (Some(v), Some(a)) => Some(v.max(a)),
                (Some(v), None) => Some(v),
                (None, Some(a)) => Some(a),
                (None, None) => None,
            }
        });

        // Video presentation is always driven by the movie clock. Audio probing/decoding
        // is asynchronous and must never clamp the WMV timeline to zero: doing so freezes
        // the picture on the first frame whenever the soundtrack is not ready yet. Once
        // decoded PCM becomes available, audio starts at the current movie position and
        // may then become the master clock through the normal playback-handle path.
        let effective_timer_ms = if loop_flag {
            duration_hint
                .filter(|duration| *duration > 0)
                .map(|duration| timer_ms % duration)
                .unwrap_or(timer_ms)
        } else {
            timer_ms
        };

        let restart_stream = self
            .wmv_streams
            .get(&path)
            .map(|state| effective_timer_ms.saturating_add(100) < state.last_effective_timer_ms)
            .unwrap_or(false);
        if restart_stream {
            self.wmv_streams.remove(&path);
            let state = spawn_wmv_stream_state(path.clone(), audio.clone(), effective_timer_ms)?;
            self.wmv_streams.insert(path.clone(), state);
        }

        let state = self
            .wmv_streams
            .get_mut(&path)
            .expect("wmv stream state exists");
        state.last_effective_timer_ms = effective_timer_ms;
        let request_ms = effective_timer_ms
            .saturating_add(WMV_STREAM_DECODE_LEAD_MS as u64)
            .min(usize::MAX as u64) as usize;
        state.request_ms.store(request_ms, Ordering::Release);
        drain_wmv_stream_state(path.as_path(), state, effective_timer_ms)?;

        // Match wmv-player-wgpu: the decode thread must never choose the movie
        // clock origin. WMV3 B pictures are emitted in coded order, so first
        // sort by ASF PTS and only then establish the presentation origin from
        // the earliest queued presentation frame.
        if state.timeline_origin_ms.is_none() {
            if let Some(first) = state.frames.front() {
                let origin = first.source_pts_ms;
                state.timeline_origin_ms = Some(origin);
                state.total_ms_hint = state
                    .total_ms_hint
                    .and_then(|total| wmv_rebase_duration_ms(total, origin));
            }
        }
        let Some(origin_ms) = state.timeline_origin_ms else {
            return Ok(None);
        };
        let presentation_pts_ms = origin_ms.saturating_add(effective_timer_ms);

        // Now that the presentation origin is known, stale-frame trimming can
        // use the same absolute PTS domain as selection.
        discard_wmv_stream_frames(state, presentation_pts_ms);

        let Some(chosen) = select_wmv_stream_frame(&state.frames, presentation_pts_ms).cloned() else {
            return Ok(None);
        };

        let video_total_ms = state.total_ms_hint.or_else(|| {
            if state.done {
                state.last_video_source_pts_ms.map(|last| {
                    last.saturating_sub(origin_ms)
                        .saturating_add(state.last_frame_delta_ms.unwrap_or(0))
                })
            } else {
                None
            }
        });
        let audio_total_ms = state.audio.as_ref().map(MovieAudio::end_ms);
        let total_ms = match (audio_total_ms, video_total_ms) {
            (Some(a), Some(v)) => Some(a.max(v)),
            (Some(a), None) => Some(a),
            (None, Some(v)) => Some(v),
            (None, None) => None,
        };
        let audio = state
            .audio
            .as_ref()
            .filter(|track| loop_flag || effective_timer_ms < track.end_ms())
            .cloned();
        let decoded_now = state.decoded_any_this_poll;
        state.decoded_any_this_poll = false;

        Ok(Some(MovieStreamFrame {
            frame: chosen.frame,
            frame_idx: chosen.frame_idx,
            fps: state.fps,
            total_ms,
            audio,
            audio_ready,
            decoded_now,
            frame_idx_is_decode_order: true,
            clamped_timer_ms: loop_flag.then_some(effective_timer_ms),
        }))
    }

    fn poll_wmv_audio_for_path(&mut self, path: &Path) -> (Option<MovieAudio>, bool) {
        if let Some(audio) = self.wmv_audio_cache.get(path) {
            return (audio.clone(), true);
        }

        let mut completed = None;
        let mut failed = None;
        if let Some(task) = self.wmv_audio_tasks.get(path) {
            match task.rx.try_recv() {
                Ok(Ok(audio)) => completed = Some(audio),
                Ok(Err(err)) => failed = Some(err),
                Err(TryRecvError::Empty) => return (None, false),
                Err(TryRecvError::Disconnected) => {
                    failed = Some(format!("wmv audio worker disconnected: {}", path.display()));
                }
            }
        } else {
            let (tx, rx) = mpsc::channel();
            let cancel = Arc::new(AtomicBool::new(false));
            let worker_cancel = cancel.clone();
            let worker_path = path.to_path_buf();
            thread::spawn(move || {
                let result = decode_wmv_audio_for_path(&worker_path, worker_cancel.as_ref())
                    .map_err(|err| format!("{:#}", err));
                let _ = tx.send(result);
            });
            self.wmv_audio_tasks
                .insert(path.to_path_buf(), WmvAudioTask { rx, cancel });
            return (None, false);
        }

        self.wmv_audio_tasks.remove(path);
        if let Some(err) = failed {
            eprintln!(
                "[SG_MOV] wmv audio decode failed path={} err={}",
                path.display(),
                err
            );
            self.wmv_audio_cache.insert(path.to_path_buf(), None);
            return (None, true);
        }
        let audio = completed.unwrap_or(None);
        self.wmv_audio_cache
            .insert(path.to_path_buf(), audio.clone());
        (audio, true)
    }

    fn poll_omv_stream_frame_for_path(
        &mut self,
        path: PathBuf,
        timer_ms: u64,
        loop_flag: bool,
    ) -> Result<Option<MovieStreamFrame>> {
        if !self.omv_streams.contains_key(&path) {
            let state = spawn_omv_stream_state(path.clone())?;
            self.omv_streams.insert(path.clone(), state);
        }

        let effective_timer_ms = self
            .omv_streams
            .get(&path)
            .and_then(|state| {
                if !loop_flag {
                    return None;
                }
                state
                    .total_ms_hint
                    .or_else(|| {
                        state.total_frames_hint.and_then(|frames| {
                            state
                                .frame_time_ms
                                .filter(|ms| *ms > 0.0)
                                .map(|ms| ((frames as f64) * ms).round() as u64)
                        })
                    })
                    .filter(|duration| *duration > 0)
            })
            .map(|duration| timer_ms % duration)
            .unwrap_or(timer_ms);
        let desired_before_drain = self.omv_streams.get(&path).and_then(|state| {
            if let Some(frame_times) = state.frame_times.as_deref() {
                return omv_frame_for_time(frame_times, effective_timer_ms);
            }
            if let Some(ms) = state.frame_time_ms.filter(|v| *v > 0.0) {
                Some(((effective_timer_ms as f64) / ms).floor() as usize)
            } else {
                state.fps.filter(|f| *f > 0.0).map(|fps| {
                    ((effective_timer_ms as f64) * (fps as f64) / 1000.0).floor() as usize
                })
            }
        });

        let state = self
            .omv_streams
            .get_mut(&path)
            .expect("omv stream state exists");
        let timer_rewound = effective_timer_ms < state.last_effective_timer_ms.saturating_sub(200);
        state.last_effective_timer_ms = effective_timer_ms;
        let desired_before_drain = if timer_rewound {
            // Genuine loop wrap / explicit rewind: let the stream seek back and
            // restart the replay from the head.
            state.last_served_frame_idx = 0;
            desired_before_drain
        } else {
            // Timer jitter must never make the requested frame go backward:
            // an indexed seek would rewind the displayed picture (flash).
            desired_before_drain.map(|idx| idx.max(state.last_served_frame_idx))
        };
        let requested_frame = desired_before_drain.unwrap_or(0);
        state
            .request_frame
            .store(requested_frame, Ordering::Release);
        drain_omv_stream_state(
            path.as_path(),
            state,
            desired_before_drain,
            false,
            true,
        )?;

        let has_loop_head = loop_flag && !state.loop_head_frames.is_empty();
        if state.frames.is_empty() && !has_loop_head {
            return Ok(None);
        }

        let latest_idx = state.decoded_frames.saturating_sub(1);
        let desired_idx = desired_before_drain.unwrap_or(latest_idx);
        let selected = if loop_flag || timer_rewound {
            // Loop wraps (and object-driven restarts) rewind the worker to a
            // key frame. Serve the cached loop head during that window so the
            // picture restarts seamlessly instead of freezing or fast-forwarding.
            select_omv_loop_frame(state, desired_idx)
        } else {
            select_stream_frame(&state.frames, desired_idx.min(latest_idx))
        };
        let Some((mut actual_frame_idx, mut frame)) = selected else {
            return Ok(None);
        };
        // A loop wrap rewinds the worker to a key frame; while it re-decodes
        // from the head, the live queue tail is far behind the timer. Serving
        // that tail would fast-forward stale head frames (visible flash).
        if !timer_rewound && actual_frame_idx < state.last_served_frame_idx {
            if let Some((held_idx, held_frame)) = state.held_frame.clone() {
                actual_frame_idx = held_idx;
                frame = held_frame;
            }
        }
        state.last_served_frame_idx = if timer_rewound {
            actual_frame_idx
        } else {
            state.last_served_frame_idx.max(actual_frame_idx)
        };
        state.held_frame = Some((actual_frame_idx, frame.clone()));

        let video_total_ms = state.total_ms_hint.or_else(|| {
            if state.done && state.decoded_frames > 0 {
                state
                    .frame_time_ms
                    .map(|ms| ((state.decoded_frames as f64) * ms).round() as u64)
                    .or_else(|| {
                        state.fps.filter(|f| *f > 0.0).map(|fps| {
                            ((state.decoded_frames as f64) * 1000.0 / (fps as f64)).round()
                                as u64
                        })
                    })
            } else {
                state.total_frames_hint.and_then(|frames| {
                    state
                        .frame_time_ms
                        .map(|ms| ((frames as f64) * ms).round() as u64)
                        .or_else(|| {
                            state.fps.filter(|f| *f > 0.0).map(|fps| {
                                ((frames as f64) * 1000.0 / (fps as f64)).round() as u64
                            })
                        })
                })
            }
        });

        Ok(Some(MovieStreamFrame {
            frame,
            frame_idx: actual_frame_idx,
            fps: state.fps,
            total_ms: video_total_ms,
            audio: None,
            audio_ready: true,
            decoded_now: false,
            frame_idx_is_decode_order: false,
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
        } else if is_wmv_extension(&ext) {
            decode_wmv_preview_frame(&path)?
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
        loop_flag: bool,
    ) -> Result<u64> {
        let local_offset_ms = offset_ms.saturating_sub(track.start_ms);

        // Kira 0.9 intentionally does not expose `sound::streaming` on wasm32.
        // The wasm movie path already decodes MPEG/WMV/OMV audio into
        // `track.samples` from the browser VFS. Native keeps compressed MPEG
        // and ASF/WMA audio bounded and incremental.
        #[cfg(not(target_arch = "wasm32"))]
        let (handle, timeline_base_ms) = if let Some(stream) = track.mpeg_stream.as_ref() {
            let decoder = MpegMovieAudioDecoder::new(stream.clone(), track.sample_rate)?;
            let mut data = StreamingSoundData::from_decoder(decoder)
                .start_position(local_offset_ms as f64 / 1000.0);
            if loop_flag {
                data = data.loop_region(..);
            }
            (
                MoviePlaybackHandle::Streaming(audio.play_streaming(TrackKind::Mov, data)?),
                0,
            )
        } else if let Some(stream) = track.wmv_stream.as_ref() {
            let mut decoder = WmvMovieAudioDecoder::new(stream.clone(), track.sample_rate)?;

            // The standalone WMV player feeds AsfWmaDecoder strictly forward and does
            // not ask Kira to seek the stateful WMA bit reservoir before playback.
            // Keep the engine path identical for normal (non-looping) MOV playback:
            // advance to the requested movie offset synchronously, then hand the
            // already-positioned decoder to Kira at source position zero.  Calling
            // StreamingSoundData::start_position() here makes Kira drive Decoder::seek
            // on its decode scheduler; that path was the only material difference from
            // the standalone player and could leave WMA1/2 superframe state misaligned
            // ("frame_len overflow") while the same file decoded correctly standalone.
            let timeline_base_ms = if !loop_flag && local_offset_ms > 0 {
                let requested_frames = (((local_offset_ms as u128) * track.sample_rate as u128)
                    / 1000)
                    .min(stream.num_frames.saturating_sub(1) as u128)
                    as usize;
                let actual_frames = decoder.prime_to_timeline_frame(requested_frames)?;
                track.start_ms.saturating_add(
                    ((actual_frames as u128) * 1000 / track.sample_rate as u128)
                        .min(u64::MAX as u128) as u64,
                )
            } else {
                track.start_ms
            };

            let mut data = StreamingSoundData::from_decoder(decoder);
            if loop_flag {
                // Looping object movies still need Kira's source-relative loop/seek
                // semantics.  Their normal restart path begins from zero.
                data = data
                    .start_position(local_offset_ms as f64 / 1000.0)
                    .loop_region(..);
            }
            (
                MoviePlaybackHandle::Streaming(audio.play_streaming(TrackKind::Mov, data)?),
                timeline_base_ms,
            )
        } else {
            make_static_movie_playback(audio, track, local_offset_ms, loop_flag)?
        };

        #[cfg(target_arch = "wasm32")]
        let (handle, timeline_base_ms) =
            make_static_movie_playback(audio, track, local_offset_ms, loop_flag)?;

        let id = self.next_playback_id;
        self.next_playback_id = self.next_playback_id.saturating_add(1).max(1);
        self.playbacks.insert(
            id,
            MoviePlayback {
                handle,
                timeline_base_ms,
            },
        );
        Ok(id)
    }

    pub fn pause_audio(&mut self, id: u64) {
        if let Some(p) = self.playbacks.get_mut(&id) {
            p.pause();
        }
    }

    pub fn resume_audio(&mut self, id: u64) {
        if let Some(p) = self.playbacks.get_mut(&id) {
            p.resume();
        }
    }

    pub fn stop_audio(&mut self, id: u64) {
        if let Some(mut p) = self.playbacks.remove(&id) {
            p.stop();
        }
    }

    pub fn audio_playback_position_ms(&mut self, id: u64) -> Option<u64> {
        let (position_ms, stream_error) = {
            let p = self.playbacks.get_mut(&id)?;
            (p.movie_position_ms(), p.take_stream_error())
        };
        if let Some(err) = stream_error {
            eprintln!("[SG_MOV] streaming audio decoder error: {err:#}");
            if let Some(mut p) = self.playbacks.remove(&id) {
                p.stop();
            }
            return None;
        }
        Some(position_ms)
    }

    pub fn audio_playback_finished(&mut self, id: u64) -> bool {
        let (finished, stream_error) = {
            let Some(p) = self.playbacks.get_mut(&id) else {
                return false;
            };
            (p.state() == PlaybackState::Stopped, p.take_stream_error())
        };
        if let Some(err) = stream_error {
            // A decoder failure is not the end of the movie.  Detach the failed
            // audio clock and let video continue on its own timer.  In particular,
            // never turn a WMA decode error into MOV.PLAY/PLAY_WAIT completion.
            eprintln!("[SG_MOV] streaming audio decoder error: {err:#}");
            if let Some(mut p) = self.playbacks.remove(&id) {
                p.stop();
            }
            return false;
        }
        if finished {
            self.playbacks.remove(&id);
        }
        finished
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
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        let mut out = read_movie_bytes(path)?;
        out.truncate(max_len.min(out.len()));
        return Ok(out);
    }
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        let mut file = fs::File::open(path).with_context(|| format!("open file: {}", path.display()))?;
        let mut out = vec![0u8; max_len.max(1)];
        let n = file
            .read(&mut out)
            .with_context(|| format!("read file prefix: {}", path.display()))?;
        out.truncate(n);
        Ok(out)
    }
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

// Cache position is not evidence of a seek: decoding can lag behind the
// audio clock, and sparse PTS can put the next cached frame ahead of it.
// Restart only for a request discontinuity outside the cached interval.
// In particular, MPEG files with only an initial sequence header must decode
// from the beginning after a seek. Repeatedly restarting a lagging worker
// prevents it from ever catching up.
fn mpeg_stream_needs_restart(state: &Mpeg2StreamState, timer_ms: u64) -> bool {
    let rewound = timer_ms.saturating_add(40) < state.last_requested_timer_ms;
    let jumped_forward = timer_ms > state.last_requested_timer_ms.saturating_add(5_000);
    let before_cache = state
        .frames
        .front()
        .and_then(|frame| mpeg_frame_timeline_ms(frame, state))
        .is_some_and(|front| timer_ms.saturating_add(40) < front);
    let after_cache = state
        .frames
        .back()
        .and_then(|frame| mpeg_frame_timeline_ms(frame, state))
        .is_some_and(|back| timer_ms > back.saturating_add(5_000));
    (rewound && (before_cache || state.frames.is_empty())) || (jumped_forward && after_cache)
}

fn spawn_mpeg2_stream_state(
    path: PathBuf,
    audio: Option<MovieAudio>,
    target_ms: u64,
) -> Result<Mpeg2StreamState> {
    let prefix = read_file_prefix(&path, MPEG2_HEADER_PROBE_BYTES)?;
    let mut width = None;
    let mut height = None;
    let mut fps = None;
    if let Some(h) = siglus_assets::mpeg2::find_sequence_header(&prefix) {
        width = Some(h.width as u32);
        height = Some(h.height as u32);
        fps = siglus_assets::mpeg2::fps_from_frame_rate_code(h.frame_rate_code);
    }

    let first_video_pts_90k = audio
        .as_ref()
        .and_then(|track| track.mpeg_stream.as_ref())
        .and_then(|stream| stream.first_video_pts_90k)
        .or_else(|| probe_first_video_pts_90k(&prefix));
    let start_offset = estimate_mpeg_video_seek_offset(&path, target_ms, audio.as_ref())?;
    let requested_frame_idx = fps
        .filter(|value| *value > 0.0)
        .map(|value| ((target_ms as f64) * value as f64 / 1000.0).floor() as usize)
        .unwrap_or(0);
    // The worker's decode counter is local to the chosen random-access start.
    // The receiver converts PTS back to a global movie frame index.
    let initial_frame_idx = 0usize;
    let (tx, rx) = mpsc::sync_channel(MPEG2_STREAM_CHANNEL_CAPACITY);
    let request_frames = Arc::new(AtomicUsize::new(
        requested_frame_idx.saturating_add(MPEG2_STREAM_DECODE_LEAD_FRAMES),
    ));
    let worker_request_frames = request_frames.clone();
    let video_path = path.clone();
    thread::spawn(move || {
        let result = stream_mpeg2_video_worker(
            video_path.as_path(),
            tx.clone(),
            worker_request_frames,
            start_offset,
            initial_frame_idx,
        );
        if let Err(err) = result {
            let _ = tx.send(Err(format!("{:#}", err)));
        }
    });

    Ok(Mpeg2StreamState {
        rx,
        frames: VecDeque::new(),
        width,
        height,
        fps,
        decoded_frames: 0,
        first_video_pts_90k,
        last_video_timeline_ms: None,
        last_requested_timer_ms: target_ms,
        done: false,
        audio,
        decoded_any_this_poll: false,
        request_frames,
    })
}

fn probe_first_video_pts_90k(data: &[u8]) -> Option<i64> {
    let mut demux = na_mpeg2_decoder::Demuxer::new_auto();
    demux
        .push(data, None)
        .into_iter()
        .find_map(|packet| {
            (packet.stream_type == na_mpeg2_decoder::StreamType::MpegVideo)
                .then_some(packet.pts_90k)
                .flatten()
        })
}

fn estimate_mpeg_video_seek_offset(
    path: &Path,
    target_ms: u64,
    audio: Option<&MovieAudio>,
) -> Result<u64> {
    if target_ms < 2_000 {
        return Ok(0);
    }
    let file_len = fs::metadata(path)
        .with_context(|| format!("stat movie file: {}", path.display()))?
        .len();
    let duration_ms = audio.and_then(|track| track.duration_ms).unwrap_or(0);
    if file_len <= MPEG2_STREAM_CHUNK_BYTES as u64 || duration_ms == 0 {
        return Ok(0);
    }
    let proportional = ((file_len as u128)
        .saturating_mul(target_ms.min(duration_ms) as u128)
        / duration_ms as u128)
        .min(file_len as u128) as u64;
    let transport = audio
        .and_then(|track| track.mpeg_stream.as_ref())
        .map(|stream| stream.transport_stream)
        .unwrap_or_else(|| {
            read_file_prefix(path, 188 * 3)
                .map(|prefix| is_transport_stream_prefix(&prefix))
                .unwrap_or(false)
        });

    let mut region_start = proportional.saturating_sub(
        MPEG_VIDEO_SEEK_BACKTRACK_BYTES.min(proportional),
    );
    if transport {
        region_start -= region_start % 188;
    }
    let region_end = proportional
        .saturating_add(MPEG_VIDEO_SEEK_FORWARD_PROBE_BYTES)
        .min(file_len);
    let mut file = fs::File::open(path)
        .with_context(|| format!("open movie seek probe: {}", path.display()))?;
    file.seek(SeekFrom::Start(region_start))
        .with_context(|| format!("seek movie seek probe: {}", path.display()))?;
    let mut probe = vec![0u8; region_end.saturating_sub(region_start) as usize];
    let n = file
        .read(&mut probe)
        .with_context(|| format!("read movie seek probe: {}", path.display()))?;
    probe.truncate(n);
    if probe.is_empty() {
        return Ok(region_start);
    }

    let target_in_probe = proportional.saturating_sub(region_start) as usize;
    let sequence_pos = rfind_byte_pattern_before(
        &probe,
        b"\0\0\x01\xb3",
        target_in_probe.min(probe.len()),
    );
    let Some(sequence_pos) = sequence_pos else {
        // Without a sequence header before the target, a mid-stream decoder
        // cannot reconstruct reference pictures safely. Fall back to the
        // beginning instead of displaying a future GOP.
        return Ok(0);
    };

    if transport {
        let packet = sequence_pos / 188;
        for packet_index in (0..=packet).rev() {
            let pos = packet_index * 188;
            if pos + 188 > probe.len() || probe[pos] != 0x47 || (probe[pos + 1] & 0x40) == 0 {
                continue;
            }
            let afc = (probe[pos + 3] >> 4) & 0x3;
            let mut payload = pos + 4;
            if afc == 2 || afc == 3 {
                if payload >= pos + 188 {
                    continue;
                }
                payload = payload.saturating_add(1 + probe[payload] as usize);
            }
            if payload + 4 <= pos + 188
                && probe[payload] == 0
                && probe[payload + 1] == 0
                && probe[payload + 2] == 1
                && (0xE0..=0xEF).contains(&probe[payload + 3])
            {
                return Ok(region_start.saturating_add(pos as u64));
            }
        }
        return Ok(region_start.saturating_add((packet * 188) as u64));
    }

    let pack_pos = rfind_byte_pattern_before(&probe, b"\0\0\x01\xba", sequence_pos)
        .unwrap_or(sequence_pos);
    Ok(region_start.saturating_add(pack_pos as u64))
}

fn rfind_byte_pattern_before(haystack: &[u8], needle: &[u8], end: usize) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    let end = end.min(haystack.len());
    if end < needle.len() {
        return None;
    }
    haystack[..end]
        .windows(needle.len())
        .rposition(|window| window == needle)
}

fn wait_for_mpeg2_frame_request(request_frames: &Arc<AtomicUsize>, frame_idx: usize) {
    while frame_idx > request_frames.load(Ordering::Acquire) {
        if request_frames.load(Ordering::Acquire) == usize::MAX {
            return;
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn stream_mpeg2_video_worker(
    path: &Path,
    tx: mpsc::SyncSender<Result<Mpeg2StreamEvent, String>>,
    request_frames: Arc<AtomicUsize>,
    start_offset: u64,
    initial_frame_idx: usize,
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
    if start_offset > 0 {
        file.seek(SeekFrom::Start(start_offset))
            .with_context(|| format!("seek movie video: {}", path.display()))?;
    }
    let mut pipeline = na_mpeg2_decoder::MpegVideoPipeline::new();
    let mut buf = vec![0u8; MPEG2_STREAM_CHUNK_BYTES];
    let mut frame_idx = initial_frame_idx;
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
                    center_x: 0,
                    center_y: 0,
                    rgba,
                });
                let ev = Mpeg2StreamEvent::Video {
                    frame_idx,
                    pts_90k: f.pts_90k,
                    frame,
                };
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
            center_x: 0,
            center_y: 0,
            rgba,
        });
        let ev = Mpeg2StreamEvent::Video {
            frame_idx,
            pts_90k: f.pts_90k,
            frame,
        };
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
            Ok(Ok(Mpeg2StreamEvent::Video {
                frame_idx,
                pts_90k,
                frame,
            })) => {
                if state.first_video_pts_90k.is_none() {
                    state.first_video_pts_90k = pts_90k;
                }
                let mut decoded = Mpeg2DecodedFrame {
                    frame_idx,
                    pts_90k,
                    frame,
                };
                if let Some(timeline_ms) = mpeg_frame_timeline_ms(&decoded, state) {
                    if let Some(fps) = state.fps.filter(|value| *value > 0.0) {
                        decoded.frame_idx = ((timeline_ms as f64) * fps as f64 / 1000.0)
                            .round()
                            .max(0.0) as usize;
                    }
                    state.last_video_timeline_ms = Some(
                        state
                            .last_video_timeline_ms
                            .map(|last| last.max(timeline_ms))
                            .unwrap_or(timeline_ms),
                    );
                }
                state.decoded_frames = state
                    .decoded_frames
                    .max(decoded.frame_idx.saturating_add(1));
                state.frames.push_back(decoded);
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

    while state.frames.len() > 2 {
        let discard = state
            .frames
            .get(1)
            .and_then(|frame| mpeg_frame_timeline_ms(frame, state))
            .map(|next_ms| next_ms.saturating_add(100) < target_timer_ms)
            .unwrap_or_else(|| {
                target_frame_idx
                    .zip(state.frames.get(1).map(|frame| frame.frame_idx))
                    .map(|(target, next)| next.saturating_add(2) < target)
                    .unwrap_or(false)
            });
        if !discard {
            break;
        }
        state.frames.pop_front();
    }
    while state.frames.len() > MPEG2_STREAM_FRAME_KEEP {
        state.frames.pop_front();
    }

    Ok(())
}

fn mpeg_timeline_origin_90k(state: &Mpeg2StreamState) -> Option<i64> {
    let video = state.first_video_pts_90k.or_else(|| {
        state
            .audio
            .as_ref()
            .and_then(|track| track.mpeg_stream.as_ref())
            .and_then(|stream| stream.first_video_pts_90k)
    });
    let audio = state
        .audio
        .as_ref()
        .and_then(|track| track.mpeg_stream.as_ref())
        .map(|stream| stream.first_audio_pts_90k);
    match (video, audio) {
        (Some(video), Some(audio)) => Some(earlier_mpeg_pts_90k(video, audio)),
        (Some(video), None) => Some(video),
        (None, Some(audio)) => Some(audio),
        (None, None) => None,
    }
}

fn mpeg_frame_timeline_ms(
    frame: &Mpeg2DecodedFrame,
    state: &Mpeg2StreamState,
) -> Option<u64> {
    if let (Some(pts), Some(origin)) = (frame.pts_90k, mpeg_timeline_origin_90k(state)) {
        return mpeg_pts_delta_90k(pts, origin)
            .map(|ticks| ((ticks as i128) * 1000 / 90_000).max(0) as u64);
    }
    state
        .fps
        .filter(|fps| *fps > 0.0)
        .map(|fps| ((frame.frame_idx as f64) * 1000.0 / fps as f64).round() as u64)
}

fn reindex_mpeg_stream_frames(state: &mut Mpeg2StreamState) {
    let origin = mpeg_timeline_origin_90k(state);
    let fps = state.fps.filter(|value| *value > 0.0);
    let mut last_timeline = None;
    for frame in &mut state.frames {
        let timeline_ms = match (frame.pts_90k, origin) {
            (Some(pts), Some(origin)) => mpeg_pts_delta_90k(pts, origin)
                .map(|ticks| ((ticks as i128) * 1000 / 90_000).max(0) as u64),
            _ => fps.map(|fps| {
                ((frame.frame_idx as f64) * 1000.0 / fps as f64).round() as u64
            }),
        };
        if let Some(timeline_ms) = timeline_ms {
            if let Some(fps) = fps {
                frame.frame_idx = ((timeline_ms as f64) * fps as f64 / 1000.0)
                    .round()
                    .max(0.0) as usize;
            }
            last_timeline = Some(
                last_timeline
                    .map(|last: u64| last.max(timeline_ms))
                    .unwrap_or(timeline_ms),
            );
        }
    }
    if let Some(last) = last_timeline {
        state.last_video_timeline_ms = Some(
            state
                .last_video_timeline_ms
                .map(|current| current.max(last))
                .unwrap_or(last),
        );
    }
    state.decoded_frames = state
        .frames
        .iter()
        .map(|frame| frame.frame_idx.saturating_add(1))
        .max()
        .unwrap_or(state.decoded_frames);
}

fn select_mpeg_stream_frame<'a>(
    frames: &'a VecDeque<Mpeg2DecodedFrame>,
    state: &Mpeg2StreamState,
    target_ms: u64,
    fallback_idx: Option<usize>,
) -> Option<&'a Mpeg2DecodedFrame> {
    let mut before = None;
    let mut after = None;
    for frame in frames {
        if let Some(frame_ms) = mpeg_frame_timeline_ms(frame, state) {
            if frame_ms <= target_ms {
                before = Some(frame);
            } else {
                after = Some(frame);
                break;
            }
        }
    }
    if let Some(frame) = before.or(after) {
        return Some(frame);
    }

    let chosen_idx = fallback_idx.unwrap_or_else(|| frames.back().map(|f| f.frame_idx).unwrap_or(0));
    frames
        .iter()
        .rev()
        .find(|frame| frame.frame_idx <= chosen_idx)
        .or_else(|| frames.front())
}

fn is_wmv_extension(ext: &str) -> bool {
    ext == "wmv"
}

fn probe_wmv_movie_info(path: &Path) -> Result<MovieInfo> {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    let decoder = {
        let bytes = read_movie_bytes(path)?;
        wmv_decoder::AsfWmv2Decoder::open(Cursor::new(bytes))
            .with_context(|| format!("open ASF/WMV: {}", path.display()))?
    };
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    let decoder = {
        let file = fs::File::open(path)
            .with_context(|| format!("open ASF/WMV: {}", path.display()))?;
        wmv_decoder::AsfWmv2Decoder::open(BufReader::new(file))
            .with_context(|| format!("parse ASF/WMV: {}", path.display()))?
    };
    let duration_ms = decoder.duration_ms();
    let video = decoder.video_stream_info();
    Ok(MovieInfo {
        path: path.to_path_buf(),
        width: (video.width > 0).then_some(video.width),
        height: (video.height > 0).then_some(video.height),
        fps: None,
        decoded_frames: None,
        // MovieInfo::duration_ms treats this field as the authoritative media
        // duration. ASF exposes a container-level play duration, not merely audio.
        audio_duration_ms: duration_ms,
    })
}

fn spawn_wmv_stream_state(
    path: PathBuf,
    audio: Option<MovieAudio>,
    target_ms: u64,
) -> Result<WmvStreamState> {
    let info = probe_wmv_movie_info(&path)?;
    let (tx, rx) = mpsc::sync_channel(WMV_STREAM_CHANNEL_CAPACITY);
    let request_ms = Arc::new(AtomicUsize::new(
        target_ms
            .saturating_add(WMV_STREAM_DECODE_LEAD_MS as u64)
            .min(usize::MAX as u64) as usize,
    ));
    let worker_request_ms = request_ms.clone();
    let worker_path = path.clone();
    thread::spawn(move || {
        let result = stream_wmv_video_worker(
            worker_path.as_path(),
            tx.clone(),
            worker_request_ms,
        );
        if let Err(err) = result {
            let _ = tx.send(Err(format!("{:#}", err)));
        }
    });

    Ok(WmvStreamState {
        rx,
        frames: VecDeque::new(),
        width: info.width,
        height: info.height,
        fps: None,
        decoded_frames: 0,
        timeline_origin_ms: None,
        last_video_source_pts_ms: None,
        last_frame_delta_ms: None,
        total_ms_hint: info.duration_ms(),
        last_effective_timer_ms: target_ms,
        done: false,
        audio,
        decoded_any_this_poll: false,
        request_ms,
    })
}

fn stream_wmv_video_worker(
    path: &Path,
    tx: mpsc::SyncSender<Result<WmvStreamEvent, String>>,
    request_ms: Arc<AtomicUsize>,
) -> Result<()> {
    let file = fs::File::open(path)
        .with_context(|| format!("open WMV movie file: {}", path.display()))?;
    let mut decoder = wmv_decoder::AsfWmv2Decoder::open(BufReader::new(file))
        .with_context(|| format!("open WMV video decoder: {}", path.display()))?;
    let info = decoder.video_stream_info().clone();
    if tx
        .send(Ok(WmvStreamEvent::Info {
            width: info.width,
            height: info.height,
        }))
        .is_err()
    {
        return Ok(());
    }

    let trace = std::env::var_os("SG_MOVIE_TRACE").is_some();
    if trace {
        eprintln!(
            "[SG_MOVIE_TRACE][WMV] decoder.open path={} size={}x{}",
            path.display(),
            info.width,
            info.height
        );
    }

    let mut frame_idx = 0usize;
    loop {
        // The bounded channel is the decode-ahead/backpressure mechanism.  Do
        // not gate decoding on the current presentation clock here: WMV3/VC-1
        // B pictures are decoded in coded order, so a future P anchor can have
        // a later PTS than B pictures which have not been decoded yet.  Waiting
        // on that anchor's PTS before decoding the following B pictures can
        // stall presentation.  This matches the standalone WMV player: decode
        // continuously into a small bounded queue, then order/present by PTS on
        // the consumer side.  request_ms is retained only as the cancellation
        // sentinel used by WmvStreamState::drop().
        if request_ms.load(Ordering::Acquire) == usize::MAX {
            return Ok(());
        }
        let decoded = decoder
            .next_frame()
            .with_context(|| format!("decode WMV video: {}", path.display()))?;
        let Some(decoded) = decoded else {
            break;
        };
        let source_pts_ms = decoded.pts_ms as u64;

        if trace && (frame_idx < 5 || frame_idx % 60 == 0) {
            eprintln!(
                "[SG_MOVIE_TRACE][WMV] decoded idx={} pts={}",
                frame_idx, source_pts_ms
            );
        }

        let frame = Arc::new(wmv_yuv_frame_to_rgba(&decoded.frame));
        if tx
            .send(Ok(WmvStreamEvent::Video {
                frame_idx,
                source_pts_ms,
                frame,
            }))
            .is_err()
        {
            return Ok(());
        }
        frame_idx = frame_idx.saturating_add(1);
    }

    let _ = tx.send(Ok(WmvStreamEvent::Done));
    Ok(())
}

fn drain_wmv_stream_state(
    path: &Path,
    state: &mut WmvStreamState,
    target_timer_ms: u64,
) -> Result<()> {
    state.decoded_any_this_poll = false;

    // Keep the game path bounded in exactly the same sense as
    // `wmv-player-wgpu`: the receiver-side presentation queue is part of the
    // decode-ahead bound.  Draining a bounded channel into an effectively
    // unbounded local queue defeats backpressure and lets the decoder race
    // arbitrarily far ahead of the movie/audio clock.  The previous game path
    // then "fixed" that growth by dropping the oldest presentation frames,
    // which can discard the very PTS range the renderer currently needs.
    //
    // First retire frames that are genuinely stale for the current clock. This
    // makes room for new decode output without ever deleting future frames just
    // because the decoder is faster than presentation.
    if let Some(origin_ms) = state.timeline_origin_ms {
        discard_wmv_stream_frames(state, origin_ms.saturating_add(target_timer_ms));
    }

    let mut video_budget = WMV_STREAM_FRAME_KEEP.saturating_sub(state.frames.len());
    for _ in 0..WMV_STREAM_MAX_DRAIN_EVENTS {
        // Once the presentation queue is full, stop receiving Video events and
        // let the bounded mpsc channel block the decoder thread.  This mirrors
        // the standalone player's `MAX_PENDING_VIDEO` policy.  Info is always
        // sent before Video, and Done cannot be queued ahead of undrained Video,
        // so breaking here cannot hide control events that are actionable now.
        if video_budget == 0 {
            break;
        }
        match state.rx.try_recv() {
            Ok(Ok(WmvStreamEvent::Info { width, height })) => {
                state.width = (width > 0).then_some(width).or(state.width);
                state.height = (height > 0).then_some(height).or(state.height);
            }
            Ok(Ok(WmvStreamEvent::Video {
                frame_idx,
                source_pts_ms,
                frame,
            })) => {
                state.last_video_source_pts_ms = Some(
                    state
                        .last_video_source_pts_ms
                        .map(|last| last.max(source_pts_ms))
                        .unwrap_or(source_pts_ms),
                );
                state.decoded_frames = state.decoded_frames.max(frame_idx.saturating_add(1));
                let decoded = WmvDecodedFrame {
                    frame_idx,
                    source_pts_ms,
                    frame,
                };
                // WMV3/VC-1 with B pictures is decoded in coded order, while ASF PTS
                // is presentation order. Keep raw PTS here and sort before selecting
                // the presentation origin, exactly like wmv-player-wgpu.
                let insert_at = state
                    .frames
                    .iter()
                    .position(|queued| queued.source_pts_ms > decoded.source_pts_ms)
                    .unwrap_or(state.frames.len());
                state.frames.insert(insert_at, decoded);
                video_budget = video_budget.saturating_sub(1);

                // Derive cadence only from adjacent presentation-order PTS. Decode-order
                // deltas are invalid whenever B pictures reorder around a future anchor.
                if insert_at > 0 {
                    let prev = state.frames[insert_at - 1].source_pts_ms;
                    let cur = state.frames[insert_at].source_pts_ms;
                    let delta = cur.saturating_sub(prev);
                    if (2..=1_000).contains(&delta) {
                        state.last_frame_delta_ms = Some(delta);
                        state.fps = Some(1000.0 / delta as f32);
                    }
                }
                if insert_at + 1 < state.frames.len() {
                    let cur = state.frames[insert_at].source_pts_ms;
                    let next = state.frames[insert_at + 1].source_pts_ms;
                    let delta = next.saturating_sub(cur);
                    if (2..=1_000).contains(&delta) {
                        state.last_frame_delta_ms = Some(delta);
                        state.fps = Some(1000.0 / delta as f32);
                    }
                }
                state.decoded_any_this_poll = true;
            }
            Ok(Ok(WmvStreamEvent::Done)) => {
                state.done = true;
                break;
            }
            Ok(Err(err)) => {
                return Err(anyhow!(
                    "wmv stream decode failed for {}: {}",
                    path.display(),
                    err
                ));
            }
            Err(TryRecvError::Empty) => break,
            Err(TryRecvError::Disconnected) => {
                state.done = true;
                break;
            }
        }
    }

    // Do not discard against a movie-local timer until the consumer has chosen
    // a presentation origin from the PTS-sorted queue. This second pass handles
    // frames inserted during this drain that are already stale for a clock which
    // advanced while decoding.
    if let Some(origin_ms) = state.timeline_origin_ms {
        discard_wmv_stream_frames(state, origin_ms.saturating_add(target_timer_ms));
    }
    Ok(())
}

fn discard_wmv_stream_frames(state: &mut WmvStreamState, target_source_pts_ms: u64) {
    while state.frames.len() > 2 {
        let discard = state
            .frames
            .get(1)
            .map(|frame| frame.source_pts_ms.saturating_add(100) < target_source_pts_ms)
            .unwrap_or(false);
        if !discard {
            break;
        }
        state.frames.pop_front();
    }
    // Do not impose the queue bound by dropping the front here.  Future frames
    // are valid presentation data; the bound is enforced by receiver
    // backpressure in `drain_wmv_stream_state`.
}

fn select_wmv_stream_frame<'a>(
    frames: &'a VecDeque<WmvDecodedFrame>,
    target_source_pts_ms: u64,
) -> Option<&'a WmvDecodedFrame> {
    // Presentation is PTS-driven. Never expose a frame whose PTS is still in
    // the future merely because decoding has run ahead. The target is kept in
    // the same absolute ASF PTS domain as queued frames; rebasing happens only
    // at the presentation clock boundary, not in the decoder thread.
    let mut before = None;
    for frame in frames {
        if frame.source_pts_ms <= target_source_pts_ms {
            before = Some(frame);
        } else {
            break;
        }
    }
    before
}

fn wmv_yuv_frame_to_rgba(frame: &wmv_decoder::YuvFrame) -> RgbaImage {
    // The original desktop engine delegates WMV presentation to the Windows
    // media stack. For WMV streams without explicit matrix metadata, match the
    // Windows/DXVA fallback: BT.601 for <=576-line SD, BT.709 for HD.
    RgbaImage {
        width: frame.width,
        height: frame.height,
        center_x: 0,
        center_y: 0,
        rgba: wmv_decoder::yuv420p_to_rgba(frame),
    }
}

fn spawn_omv_stream_state(path: PathBuf) -> Result<OmvStreamState> {
    let (tx, rx) = mpsc::sync_channel(OMV_STREAM_CHANNEL_CAPACITY);
    let request_frame = Arc::new(AtomicUsize::new(0));
    let worker_request_frame = request_frame.clone();
    thread::spawn(move || {
        let result = stream_omv_video_worker(path.as_path(), tx.clone(), worker_request_frame);
        if let Err(err) = result {
            let _ = tx.send(Err(format!("{:#}", err)));
        }
    });
    Ok(OmvStreamState {
        rx,
        frames: VecDeque::new(),
        loop_head_frames: VecDeque::new(),
        loop_head_bytes: 0,
        width: None,
        height: None,
        fps: None,
        frame_time_ms: None,
        total_frames_hint: None,
        total_ms_hint: None,
        frame_times: None,
        decoded_frames: 0,
        done: false,
        request_frame,
        last_served_frame_idx: 0,
        last_effective_timer_ms: 0,
        held_frame: None,
    })
}

fn stream_omv_video_worker(
    path: &Path,
    tx: mpsc::SyncSender<Result<OmvStreamEvent, String>>,
    request_frame: Arc<AtomicUsize>,
) -> Result<()> {
    let omv = siglus_assets::omv::OmvFile::open(path)
        .with_context(|| format!("open OMV index: {}", path.display()))?;
    let reader = omv
        .open_embedded_ogg_reader(path)
        .with_context(|| format!("open embedded Ogg stream: {}", path.display()))?;
    let mut video_tf = siglus_omv_decoder::TheoraVideoStream::open(reader)
        .with_context(|| format!("open streaming Theora decoder: {}", path.display()))?;
    let vinfo = video_tf.info();

    let display_w = omv.header.display_width as i32;
    let display_h = omv.header.display_height as i32;
    let width = display_w.max(1) as u32;
    let height = display_h.max(1) as u32;
    let fps = if omv.header.frame_time_us != 0 {
        Some(1_000_000.0 / (omv.header.frame_time_us as f32))
    } else if vinfo.fps > 0.0 {
        Some(vinfo.fps as f32)
    } else {
        None
    };
    let frame_time_ms = omv_frame_duration_ms(Some(&omv.header), fps);
    let total_frames_hint = (!omv.packets.is_empty())
        .then_some(omv.packets.len())
        .or_else(|| {
            (omv.header.packet_count_hint > 0).then_some(omv.header.packet_count_hint as usize)
        });
    let frame_times = Arc::new(
        omv.packets
            .iter()
            .map(|packet| {
                (
                    u64::try_from(packet.frame_time_start.max(0)).unwrap_or(0),
                    u64::try_from(packet.frame_time_end.max(0)).unwrap_or(0),
                )
            })
            .collect::<Vec<_>>(),
    );
    let total_ms_hint = frame_times
        .last()
        .map(|(_, end)| *end)
        .filter(|end| *end > 0);
    let theora_type = omv.header.theora_type;

    if tx
        .send(Ok(OmvStreamEvent::Info {
            width,
            height,
            fps,
            frame_time_ms,
            total_frames_hint,
            total_ms_hint,
            frame_times,
        }))
        .is_err()
    {
        return Ok(());
    }

    let total_frames = total_frames_hint.unwrap_or(0);
    let mut next_frame_idx = 0usize;
    let mut last_requested_frame = 0usize;
    let mut eof_reported = false;

    loop {
        let requested = request_frame.load(Ordering::Acquire);
        if requested == usize::MAX {
            return Ok(());
        }
        let target_frame = if total_frames > 0 {
            requested.min(total_frames.saturating_sub(1))
        } else {
            requested
        };
        let indexed_seek = omv_should_index_seek(
            &omv,
            last_requested_frame,
            target_frame,
        );

        if indexed_seek {
            let seek_result = omv
                .seek_point_for_frame(target_frame)
                .and_then(|point| {
                    video_tf.seek_to_indexed_frame(
                        point.file_offset,
                        point.key_page_file_offset,
                        point.first_packet_no,
                        point.key_frame_packet_no,
                        point.target_packet_no,
                    )
                });
            let packed = match seek_result {
                Ok(frame) => frame,
                Err(index_err) => {
                    eprintln!(
                        "[SG_MOV] OMV indexed seek fallback path={} frame={} err={:#}",
                        path.display(),
                        target_frame,
                        index_err
                    );
                    let reader = omv
                        .open_embedded_ogg_reader(path)
                        .with_context(|| format!("reopen embedded Ogg stream: {}", path.display()))?;
                    let mut restarted = siglus_omv_decoder::TheoraVideoStream::open(reader)
                        .with_context(|| {
                            format!("restart streaming Theora decoder: {}", path.display())
                        })?;
                    let mut frame = None;
                    for _ in 0..=target_frame {
                        frame = restarted.read_video_frame()?;
                        if frame.is_none() {
                            break;
                        }
                    }
                    video_tf = restarted;
                    frame
                }
            };

            if tx
                .send(Ok(OmvStreamEvent::Reset {
                    frame_idx: target_frame,
                }))
                .is_err()
            {
                return Ok(());
            }
            if let Some(buf) = packed {
                if !send_omv_video_frame(
                    &tx,
                    target_frame,
                    &buf,
                    vinfo,
                    display_h,
                    theora_type,
                    width,
                    height,
                )? {
                    return Ok(());
                }
            }
            // The indexed seek consumed the target packet even when Theora
            // reports a duplicate frame and produces no new pixels. Continue
            // with the following packet instead of relabelling it as target.
            next_frame_idx = target_frame.saturating_add(1);
            eof_reported = false;
        }

        last_requested_frame = target_frame;
        let decode_until = if total_frames > 0 {
            target_frame
                .saturating_add(OMV_STREAM_DECODE_LEAD_FRAMES)
                .min(total_frames.saturating_sub(1))
        } else {
            target_frame.saturating_add(OMV_STREAM_DECODE_LEAD_FRAMES)
        };
        let mut did_work = false;
        while next_frame_idx <= decode_until {
            if request_frame.load(Ordering::Acquire) == usize::MAX {
                return Ok(());
            }
            let Some(buf) = video_tf.read_video_frame()? else {
                if !eof_reported {
                    if tx.send(Ok(OmvStreamEvent::Done)).is_err() {
                        return Ok(());
                    }
                    eof_reported = true;
                }
                break;
            };
            if !send_omv_video_frame(
                &tx,
                next_frame_idx,
                &buf,
                vinfo,
                display_h,
                theora_type,
                width,
                height,
            )? {
                return Ok(());
            }
            next_frame_idx = next_frame_idx.saturating_add(1);
            did_work = true;
        }

        if !did_work {
            thread::sleep(Duration::from_millis(1));
        }
    }
}

fn omv_should_index_seek(
    omv: &siglus_assets::omv::OmvFile,
    current_frame: usize,
    target_frame: usize,
) -> bool {
    if target_frame < current_frame {
        return true;
    }
    let Some(current_packet) = omv.packets.get(current_frame) else {
        return false;
    };
    let Some(target_packet) = omv.packets.get(target_frame) else {
        return false;
    };
    if current_packet.key_frame_packet_no == target_packet.key_frame_packet_no {
        return false;
    }
    let Ok(key_page_no) = usize::try_from(target_packet.key_frame_page_no) else {
        return false;
    };
    let Some(key_page) = omv.pages.get(key_page_no) else {
        return false;
    };
    current_packet.own_page_no < key_page.seek_page_no
}

fn send_omv_video_frame(
    tx: &mpsc::SyncSender<Result<OmvStreamEvent, String>>,
    frame_idx: usize,
    buf: &[u8],
    vinfo: siglus_omv_decoder::VideoInfo,
    display_h: i32,
    theora_type: u32,
    width: u32,
    height: u32,
) -> Result<bool> {
    let rgba = convert_omv_frame(
        buf,
        vinfo.width,
        vinfo.height,
        vinfo.fmt,
        display_h,
        theora_type,
    );
    let frame = Arc::new(RgbaImage {
        width,
        height,
        center_x: 0,
        center_y: 0,
        rgba,
    });
    Ok(tx
        .send(Ok(OmvStreamEvent::Video { frame_idx, frame }))
        .is_ok())
}

fn select_stream_frame(
    frames: &VecDeque<(usize, Arc<RgbaImage>)>,
    chosen_idx: usize,
) -> Option<(usize, Arc<RgbaImage>)> {
    let (front_idx, _) = frames.front()?;
    let (back_idx, back_frame) = frames.back()?;
    if chosen_idx < *front_idx {
        let (_, front_frame) = frames.front()?;
        return Some((*front_idx, front_frame.clone()));
    }
    if chosen_idx >= *back_idx {
        return Some((*back_idx, back_frame.clone()));
    }

    let direct = chosen_idx.saturating_sub(*front_idx);
    if let Some((idx, frame)) = frames.get(direct) {
        if *idx == chosen_idx {
            return Some((*idx, frame.clone()));
        }
    }

    let mut i = direct.min(frames.len().saturating_sub(1));
    loop {
        if let Some((idx, frame)) = frames.get(i) {
            if *idx <= chosen_idx {
                return Some((*idx, frame.clone()));
            }
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }

    Some((*back_idx, back_frame.clone()))
}

fn select_omv_loop_frame(
    state: &OmvStreamState,
    chosen_idx: usize,
) -> Option<(usize, Arc<RgbaImage>)> {
    let live_queue_is_after_target = state
        .frames
        .front()
        .map(|(idx, _)| *idx > chosen_idx)
        .unwrap_or(true);
    let target_is_in_cached_head = state
        .loop_head_frames
        .back()
        .map(|(idx, _)| chosen_idx <= *idx)
        .unwrap_or(false);
    if live_queue_is_after_target || target_is_in_cached_head {
        // During the short indexed rewind window, keep displaying the cached
        // beginning of the loop. If the requested frame advances past the
        // cache, hold its final frame instead of flashing the old loop tail.
        if let Some(frame) = select_stream_frame(&state.loop_head_frames, chosen_idx) {
            return Some(frame);
        }
    }
    select_stream_frame(&state.frames, chosen_idx)
        .or_else(|| select_stream_frame(&state.loop_head_frames, chosen_idx))
}

fn cache_omv_loop_head_frame(
    state: &mut OmvStreamState,
    frame_idx: usize,
    frame: &Arc<RgbaImage>,
) {
    if frame_idx >= OMV_LOOP_HEAD_CACHE_MAX_FRAMES
        || state
            .loop_head_frames
            .iter()
            .any(|(cached_idx, _)| *cached_idx == frame_idx)
    {
        return;
    }
    let frame_bytes = frame.rgba.len();
    if !state.loop_head_frames.is_empty()
        && state.loop_head_bytes.saturating_add(frame_bytes) > OMV_LOOP_HEAD_CACHE_MAX_BYTES
    {
        return;
    }
    state.loop_head_bytes = state.loop_head_bytes.saturating_add(frame_bytes);
    state.loop_head_frames.push_back((frame_idx, frame.clone()));
}

fn drain_omv_stream_state(
    path: &Path,
    state: &mut OmvStreamState,
    target_frame_idx: Option<usize>,
    retain_from_start: bool,
    cache_loop_head: bool,
) -> Result<()> {
    for _ in 0..OMV_STREAM_MAX_DRAIN_EVENTS {
        match state.rx.try_recv() {
            Ok(Ok(OmvStreamEvent::Info {
                width,
                height,
                fps,
                frame_time_ms,
                total_frames_hint,
                total_ms_hint,
                frame_times,
            })) => {
                state.width = Some(width);
                state.height = Some(height);
                state.fps = fps.or(state.fps);
                state.frame_time_ms = frame_time_ms.or(state.frame_time_ms);
                state.total_frames_hint = total_frames_hint.or(state.total_frames_hint);
                state.total_ms_hint = total_ms_hint.or(state.total_ms_hint);
                state.frame_times = Some(frame_times);
            }
            Ok(Ok(OmvStreamEvent::Reset { frame_idx })) => {
                state.frames.clear();
                state.decoded_frames = frame_idx;
                state.done = false;
            }
            Ok(Ok(OmvStreamEvent::Video { frame_idx, frame })) => {
                state.decoded_frames = state.decoded_frames.max(frame_idx.saturating_add(1));
                if cache_loop_head {
                    cache_omv_loop_head_frame(state, frame_idx, &frame);
                }
                state.frames.push_back((frame_idx, frame));
            }
            Ok(Ok(OmvStreamEvent::Done)) => {
                // The worker remains alive after EOF so a loop rewind can seek
                // through the OMV index. Continue draining: Reset and the first
                // post-rewind frame may already be queued behind Done.
                state.done = true;
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

    if !retain_from_start {
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
    }
    Ok(())
}

fn omv_frame_for_time(frame_times: &[(u64, u64)], now_ms: u64) -> Option<usize> {
    if frame_times.is_empty() {
        return None;
    }
    let mut low = 0usize;
    let mut high = frame_times.len();
    while low < high {
        let mid = low + (high - low) / 2;
        if now_ms <= frame_times[mid].1 {
            high = mid;
        } else {
            low = mid.saturating_add(1);
        }
    }
    Some(low.min(frame_times.len().saturating_sub(1)))
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
pub struct MpegStreamAudio {
    pub path: Arc<PathBuf>,
    pub num_frames: usize,
    pub first_audio_pts_ms: i64,
    pub first_video_pts_ms: Option<i64>,
    pub first_audio_pts_90k: i64,
    pub first_video_pts_90k: Option<i64>,
    pub audio_end_pts_90k: i64,
    pub file_len: u64,
    pub transport_stream: bool,
}

#[derive(Debug, Clone)]
pub struct WmvStreamAudio {
    pub path: Arc<PathBuf>,
    pub num_frames: usize,
    pub initial_silence_frames: usize,
    pub source_skip_frames: usize,
}

#[derive(Debug, Clone)]
pub struct MovieAudio {
    /// Present for OMV and for the retained DVD-private static fallback.
    pub samples: Arc<Vec<i16>>,
    /// Present for MPEG Layer I/II/III program-stream audio.  The audio is
    /// decoded by Kira's bounded streaming scheduler instead of materializing
    /// a complete WAV before playback begins.
    pub mpeg_stream: Option<MpegStreamAudio>,
    /// Present for native ASF/WMA playback. The compressed stream is decoded
    /// incrementally by Kira rather than materializing the entire soundtrack.
    pub wmv_stream: Option<WmvStreamAudio>,
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

fn make_static_movie_playback(
    audio: &mut AudioHub,
    track: &MovieAudio,
    local_offset_ms: u64,
    loop_flag: bool,
) -> Result<(MoviePlaybackHandle, u64)> {
    let wav = encode_wav_i16_interleaved(
        track.samples.as_ref(),
        track.channels,
        track.sample_rate,
    );
    let mut data = StaticSoundData::from_cursor(Cursor::new(wav))
        .context("kira: decode movie WAV bytes")?
        .start_position(local_offset_ms as f64 / 1000.0);
    if loop_flag {
        data = data.loop_region(..);
    }
    Ok((
        MoviePlaybackHandle::Static(audio.play_static(TrackKind::Mov, data)?),
        track.start_ms,
    ))
}

#[derive(Debug)]
enum MoviePlaybackHandle {
    Static(StaticSoundHandle),
    #[cfg(not(target_arch = "wasm32"))]
    Streaming(StreamingSoundHandle<anyhow::Error>),
}

#[derive(Debug)]
struct MoviePlayback {
    handle: MoviePlaybackHandle,
    /// Kira reports a position relative to the audio source. This offset maps
    /// that value back to the Siglus movie timeline.
    timeline_base_ms: u64,
}

impl MoviePlayback {
    fn state(&self) -> PlaybackState {
        match &self.handle {
            MoviePlaybackHandle::Static(handle) => handle.state(),
            #[cfg(not(target_arch = "wasm32"))]
            MoviePlaybackHandle::Streaming(handle) => handle.state(),
        }
    }

    fn movie_position_ms(&self) -> u64 {
        let seconds = match &self.handle {
            MoviePlaybackHandle::Static(handle) => handle.position(),
            #[cfg(not(target_arch = "wasm32"))]
            MoviePlaybackHandle::Streaming(handle) => handle.position(),
        };
        self.timeline_base_ms
            .saturating_add((seconds.max(0.0) * 1000.0).round() as u64)
    }

    fn pause(&mut self) {
        match &mut self.handle {
            MoviePlaybackHandle::Static(handle) => {
                handle.pause(kira::tween::Tween::default());
            }
            #[cfg(not(target_arch = "wasm32"))]
            MoviePlaybackHandle::Streaming(handle) => {
                handle.pause(kira::tween::Tween::default());
            }
        }
    }

    fn resume(&mut self) {
        match &mut self.handle {
            MoviePlaybackHandle::Static(handle) => {
                handle.resume(kira::tween::Tween::default());
            }
            #[cfg(not(target_arch = "wasm32"))]
            MoviePlaybackHandle::Streaming(handle) => {
                handle.resume(kira::tween::Tween::default());
            }
        }
    }

    fn stop(&mut self) {
        match &mut self.handle {
            MoviePlaybackHandle::Static(handle) => {
                handle.stop(kira::tween::Tween::default());
            }
            #[cfg(not(target_arch = "wasm32"))]
            MoviePlaybackHandle::Streaming(handle) => {
                handle.stop(kira::tween::Tween::default());
            }
        }
    }

    fn take_stream_error(&mut self) -> Option<anyhow::Error> {
        match &mut self.handle {
            MoviePlaybackHandle::Static(_) => None,
            #[cfg(not(target_arch = "wasm32"))]
            MoviePlaybackHandle::Streaming(handle) => handle.pop_error(),
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
const WMV_AUDIO_DECODE_FRAMES: usize = 4096;

#[cfg(not(target_arch = "wasm32"))]
struct WmvMovieAudioDecoder {
    info: WmvStreamAudio,
    sample_rate: u32,
    decoder: wmv_decoder::AsfWmaDecoder<BufReader<fs::File>>,
    pending: VecDeque<Frame>,
    initial_silence_remaining: usize,
    source_skip_remaining: usize,
    produced_frames: usize,
    eof: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl WmvMovieAudioDecoder {
    fn new(info: WmvStreamAudio, sample_rate: u32) -> Result<Self> {
        if sample_rate == 0 || info.num_frames == 0 {
            bail!("invalid WMV movie audio stream description");
        }
        let decoder = Self::open_decoder(info.path.as_ref())?;
        let initial_silence_remaining = info.initial_silence_frames;
        let source_skip_remaining = info.source_skip_frames;
        Ok(Self {
            info,
            sample_rate,
            decoder,
            pending: VecDeque::new(),
            initial_silence_remaining,
            source_skip_remaining,
            produced_frames: 0,
            eof: false,
        })
    }

    fn open_decoder(path: &Path) -> Result<wmv_decoder::AsfWmaDecoder<BufReader<fs::File>>> {
        let file = fs::File::open(path)
            .with_context(|| format!("open WMV movie audio: {}", path.display()))?;
        wmv_decoder::AsfWmaDecoder::open(BufReader::new(file))
            .with_context(|| format!("open WMA decoder: {}", path.display()))
    }

    fn reset(&mut self) -> Result<()> {
        self.decoder = Self::open_decoder(self.info.path.as_ref())?;
        self.pending.clear();
        self.initial_silence_remaining = self.info.initial_silence_frames;
        self.source_skip_remaining = self.info.source_skip_frames;
        self.produced_frames = 0;
        self.eof = false;
        Ok(())
    }

    fn decode_more(&mut self) -> Result<()> {
        if self.eof {
            return Ok(());
        }
        match self.decoder.next_frame()? {
            Some(decoded) => {
                let frames = convert_movie_audio_chunk_to_frames(
                    &decoded.frame.samples,
                    decoded.frame.channels,
                    decoded.frame.sample_rate,
                    self.sample_rate,
                );
                let skip = self.source_skip_remaining.min(frames.len());
                self.source_skip_remaining -= skip;
                self.pending.extend(frames.into_iter().skip(skip));
            }
            None => self.eof = true,
        }
        Ok(())
    }

    /// Advance from the beginning of the movie timeline without invoking Kira's
    /// streaming seek machinery.  Every compressed WMA packet is still decoded in
    /// forward order, preserving the superframe/bit-reservoir state exactly like the
    /// standalone WMV player.
    fn prime_to_timeline_frame(&mut self, target: usize) -> Result<usize> {
        let target = target.min(self.info.num_frames.saturating_sub(1));
        let mut advanced = 0usize;

        let silence = target.min(self.initial_silence_remaining);
        self.initial_silence_remaining -= silence;
        advanced = advanced.saturating_add(silence);

        while advanced < target && !self.eof {
            if self.pending.is_empty() {
                self.decode_more()?;
            }
            while advanced < target {
                let Some(_frame) = self.pending.pop_front() else {
                    break;
                };
                advanced = advanced.saturating_add(1);
            }
        }
        self.produced_frames = advanced;
        Ok(advanced)
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl KiraStreamingDecoder for WmvMovieAudioDecoder {
    type Error = anyhow::Error;

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn num_frames(&self) -> usize {
        self.info.num_frames
    }

    fn decode(&mut self) -> Result<Vec<Frame>, Self::Error> {
        let remaining = self.info.num_frames.saturating_sub(self.produced_frames);
        if remaining == 0 {
            return Ok(Vec::new());
        }
        let target = remaining.min(WMV_AUDIO_DECODE_FRAMES);
        let mut out = Vec::with_capacity(target);
        while out.len() < target {
            if self.initial_silence_remaining > 0 {
                let count = self
                    .initial_silence_remaining
                    .min(target.saturating_sub(out.len()));
                out.resize(out.len().saturating_add(count), Frame::ZERO);
                self.initial_silence_remaining -= count;
                continue;
            }
            while out.len() < target {
                let Some(frame) = self.pending.pop_front() else {
                    break;
                };
                out.push(frame);
            }
            if out.len() >= target {
                break;
            }
            if self.eof {
                // ASF play duration is the scheduler's finite length. If the
                // compressed audio ends slightly earlier, preserve the movie
                // timeline with silence instead of spinning at EOF.
                out.resize(target, Frame::ZERO);
                break;
            }
            self.decode_more()?;
        }
        self.produced_frames = self.produced_frames.saturating_add(out.len());
        Ok(out)
    }

    fn seek(&mut self, index: usize) -> Result<usize, Self::Error> {
        self.reset()?;
        self.prime_to_timeline_frame(index)
    }
}

#[cfg(not(target_arch = "wasm32"))]
const MPEG_AUDIO_DECODE_FRAMES: usize = 4096;

#[cfg(not(target_arch = "wasm32"))]
struct MpegMovieAudioDecoder {
    info: MpegStreamAudio,
    sample_rate: u32,
    file: fs::File,
    pipeline: na_mpeg2_decoder::MpegAudioPipeline,
    read_buf: Vec<u8>,
    pending: VecDeque<Frame>,
    initial_silence_frames: usize,
    produced_frames: usize,
    eof: bool,
}

#[cfg(not(target_arch = "wasm32"))]
impl MpegMovieAudioDecoder {
    fn new(info: MpegStreamAudio, sample_rate: u32) -> Result<Self> {
        if sample_rate == 0 || info.num_frames == 0 {
            bail!("invalid MPEG movie audio stream description");
        }
        let file = fs::File::open(info.path.as_ref())
            .with_context(|| format!("open MPEG movie audio: {}", info.path.display()))?;
        let initial_silence_frames = mpeg_initial_silence_frames(&info, sample_rate);
        Ok(Self {
            info,
            sample_rate,
            file,
            pipeline: na_mpeg2_decoder::MpegAudioPipeline::new(),
            read_buf: vec![0u8; MPEG2_STREAM_CHUNK_BYTES],
            pending: VecDeque::new(),
            initial_silence_frames,
            produced_frames: 0,
            eof: false,
        })
    }

    fn reset(&mut self) -> Result<()> {
        self.reset_at_offset(0, 0)
    }

    fn reset_at_offset(&mut self, offset: u64, produced_frames: usize) -> Result<()> {
        self.file = fs::File::open(self.info.path.as_ref())
            .with_context(|| format!("reopen MPEG movie audio: {}", self.info.path.display()))?;
        if offset > 0 {
            self.file
                .seek(SeekFrom::Start(offset))
                .with_context(|| format!("seek MPEG movie audio: {}", self.info.path.display()))?;
        }
        self.pipeline = na_mpeg2_decoder::MpegAudioPipeline::new();
        self.pending.clear();
        self.initial_silence_frames = if offset == 0 {
            mpeg_initial_silence_frames(&self.info, self.sample_rate)
        } else {
            0
        };
        self.produced_frames = produced_frames;
        self.eof = false;
        Ok(())
    }

    fn seek_start_offset(&self, target_audio_frames: usize, backtrack: u64) -> Result<u64> {
        let initial_silence = mpeg_initial_silence_frames(&self.info, self.sample_rate);
        let audio_frames = self.info.num_frames.saturating_sub(initial_silence).max(1);
        let proportional = ((self.info.file_len as u128)
            .saturating_mul(target_audio_frames.min(audio_frames) as u128)
            / audio_frames as u128)
            .min(self.info.file_len as u128) as u64;
        let start = proportional.saturating_sub(backtrack.min(proportional));
        align_mpeg_container_seek(
            self.info.path.as_ref(),
            start,
            self.info.transport_stream,
        )
    }

    fn prime_seek_from_offset(
        &mut self,
        offset: u64,
        target_index: usize,
    ) -> Result<Option<usize>> {
        self.reset_at_offset(offset, 0)?;
        let mut bytes_read = 0u64;
        while bytes_read < MPEG_AUDIO_SEEK_MAX_PRIME_BYTES {
            let n = self
                .file
                .read(&mut self.read_buf)
                .with_context(|| format!("prime MPEG movie audio seek: {}", self.info.path.display()))?;
            if n == 0 {
                break;
            }
            bytes_read = bytes_read.saturating_add(n as u64);
            let input = self.read_buf[..n].to_vec();
            let mut chunks = Vec::new();
            self.pipeline
                .push_with(&input, None, |chunk| chunks.push(chunk))
                .context("prime MPEG movie audio decoder")?;
            let mut actual_index = None;
            for chunk in chunks {
                if actual_index.is_none() {
                    let Some(chunk_index) = mpeg_audio_chunk_timeline_frame(
                        &self.info,
                        chunk.pts_ms,
                        self.sample_rate,
                    ) else {
                        continue;
                    };
                    if chunk_index > target_index {
                        return Ok(None);
                    }
                    actual_index = Some(chunk_index);
                    self.initial_silence_frames = 0;
                    self.produced_frames = chunk_index;
                }
                let converted = convert_movie_audio_chunk_to_frames(
                    &chunk.samples,
                    chunk.channels,
                    chunk.sample_rate,
                    self.sample_rate,
                );
                self.pending.extend(converted);
            }
            if let Some(actual) = actual_index.filter(|_| !self.pending.is_empty()) {
                return Ok(Some(actual));
            }
        }
        Ok(None)
    }

    fn append_chunk(&mut self, chunk: na_mpeg2_decoder::MpegAudioF32) {
        let converted = convert_movie_audio_chunk_to_frames(
            &chunk.samples,
            chunk.channels,
            chunk.sample_rate,
            self.sample_rate,
        );
        self.pending.extend(converted);
    }

    fn decode_more(&mut self) -> Result<()> {
        if self.eof {
            return Ok(());
        }
        let n = self
            .file
            .read(&mut self.read_buf)
            .with_context(|| format!("read MPEG movie audio: {}", self.info.path.display()))?;
        if n == 0 {
            let mut chunks = Vec::new();
            self.pipeline
                .flush_with(|chunk| chunks.push(chunk))
                .context("flush MPEG movie audio")?;
            for chunk in chunks {
                self.append_chunk(chunk);
            }
            self.eof = true;
            return Ok(());
        }

        // Keep the source bytes independent from `self` while mutably driving
        // the pipeline; this avoids a whole-struct borrow conflict.
        let input = self.read_buf[..n].to_vec();
        let mut chunks = Vec::new();
        self.pipeline
            .push_with(&input, None, |chunk| chunks.push(chunk))
            .context("decode MPEG movie audio chunk")?;
        for chunk in chunks {
            self.append_chunk(chunk);
        }
        Ok(())
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl KiraStreamingDecoder for MpegMovieAudioDecoder {
    type Error = anyhow::Error;

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn num_frames(&self) -> usize {
        self.info.num_frames
    }

    fn decode(&mut self) -> Result<Vec<Frame>, Self::Error> {
        let remaining = self.info.num_frames.saturating_sub(self.produced_frames);
        if remaining == 0 {
            return Ok(Vec::new());
        }
        let target = remaining.min(MPEG_AUDIO_DECODE_FRAMES);
        let mut out = Vec::with_capacity(target);

        while out.len() < target {
            if self.initial_silence_frames > 0 {
                let count = self
                    .initial_silence_frames
                    .min(target.saturating_sub(out.len()));
                out.resize(out.len().saturating_add(count), Frame::ZERO);
                self.initial_silence_frames -= count;
                continue;
            }

            while out.len() < target {
                let Some(frame) = self.pending.pop_front() else {
                    break;
                };
                out.push(frame);
            }
            if out.len() >= target {
                break;
            }

            if self.eof {
                // The compressed-frame probe supplies the finite length Kira
                // requires. A damaged frame may be counted but rejected by the
                // sample decoder; preserve the timeline with silence instead of
                // making the scheduler spin forever waiting for missing frames.
                out.resize(target, Frame::ZERO);
                break;
            }
            self.decode_more()?;
        }

        self.produced_frames = self.produced_frames.saturating_add(out.len());
        Ok(out)
    }

    fn seek(&mut self, index: usize) -> Result<usize, Self::Error> {
        let target = index.min(self.info.num_frames.saturating_sub(1));
        let initial_silence = mpeg_initial_silence_frames(&self.info, self.sample_rate);
        if target <= initial_silence || self.info.file_len == 0 {
            self.reset()?;
            return Ok(0);
        }

        let target_audio_frames = target.saturating_sub(initial_silence);
        let mut backtrack = MPEG_AUDIO_SEEK_INITIAL_BACKTRACK_BYTES;
        loop {
            let offset = self.seek_start_offset(target_audio_frames, backtrack)?;
            if let Some(actual) = self.prime_seek_from_offset(offset, target)? {
                return Ok(actual);
            }
            if offset == 0 || backtrack >= MPEG_AUDIO_SEEK_MAX_PRIME_BYTES {
                break;
            }
            backtrack = backtrack
                .saturating_mul(4)
                .min(MPEG_AUDIO_SEEK_MAX_PRIME_BYTES);
        }

        self.reset()?;
        Ok(0)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn align_mpeg_container_seek(
    path: &Path,
    offset: u64,
    transport_stream: bool,
) -> Result<u64> {
    if offset == 0 {
        return Ok(0);
    }
    if transport_stream {
        return Ok(offset - offset % 188);
    }

    let file_len = fs::metadata(path)
        .with_context(|| format!("stat MPEG seek source: {}", path.display()))?
        .len();
    let mut file = fs::File::open(path)
        .with_context(|| format!("open MPEG seek source: {}", path.display()))?;
    file.seek(SeekFrom::Start(offset))
        .with_context(|| format!("seek MPEG source: {}", path.display()))?;
    let probe_len = (1024 * 1024u64).min(file_len.saturating_sub(offset)) as usize;
    let mut probe = vec![0u8; probe_len];
    let n = file
        .read(&mut probe)
        .with_context(|| format!("read MPEG seek source: {}", path.display()))?;
    probe.truncate(n);
    if let Some(pos) = find_byte_pattern(&probe, b"\0\0\x01\xba")
        .or_else(|| find_byte_pattern(&probe, b"\0\0\x01\xb3"))
    {
        Ok(offset.saturating_add(pos as u64))
    } else {
        Ok(offset)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn mpeg_audio_chunk_timeline_frame(
    info: &MpegStreamAudio,
    chunk_pts_ms: i64,
    sample_rate: u32,
) -> Option<usize> {
    if sample_rate == 0 || (chunk_pts_ms == 0 && info.first_audio_pts_ms != 0) {
        return None;
    }
    let origin = match info.first_video_pts_90k {
        Some(video) if mpeg_pts_delta_90k(video, info.first_audio_pts_90k).is_none() => video,
        Some(_) | None => info.first_audio_pts_90k,
    };
    let chunk_pts_90k = chunk_pts_ms.saturating_mul(90);
    let ticks = mpeg_pts_delta_90k(chunk_pts_90k, origin)?;
    Some(pts90k_ticks_to_audio_frames(ticks, sample_rate))
}

#[cfg(not(target_arch = "wasm32"))]
fn mpeg_initial_silence_frames(info: &MpegStreamAudio, sample_rate: u32) -> usize {
    let origin = info
        .first_video_pts_90k
        .map(|video| earlier_mpeg_pts_90k(video, info.first_audio_pts_90k))
        .unwrap_or(info.first_audio_pts_90k);
    pts90k_ticks_to_audio_frames(
        mpeg_pts_delta_90k(info.first_audio_pts_90k, origin).unwrap_or(0),
        sample_rate,
    )
}

#[cfg(not(target_arch = "wasm32"))]
fn convert_movie_audio_chunk_to_frames(
    samples: &[f32],
    src_channels: u16,
    src_sample_rate: u32,
    dst_sample_rate: u32,
) -> Vec<Frame> {
    let src_channels = src_channels as usize;
    if src_channels == 0 || src_sample_rate == 0 || dst_sample_rate == 0 {
        return Vec::new();
    }
    let src_frames = samples.len() / src_channels;
    if src_frames == 0 {
        return Vec::new();
    }
    let dst_frames = ((src_frames as u128) * (dst_sample_rate as u128)
        / (src_sample_rate as u128))
        .max(1) as usize;
    let mut out = Vec::with_capacity(dst_frames);

    for dst_index in 0..dst_frames {
        let src_num = (dst_index as u128) * (src_sample_rate as u128);
        let src_index = (src_num / dst_sample_rate as u128) as usize;
        let src_index = src_index.min(src_frames - 1);
        let next_index = src_index.saturating_add(1).min(src_frames - 1);
        let fraction = (src_num % dst_sample_rate as u128) as f32 / dst_sample_rate as f32;
        let sample = |frame: usize, channel: usize| -> f32 {
            samples
                .get(frame.saturating_mul(src_channels).saturating_add(channel.min(src_channels - 1)))
                .copied()
                .unwrap_or(0.0)
        };
        let interpolate = |channel: usize| {
            let a = sample(src_index, channel);
            let b = sample(next_index, channel);
            a + (b - a) * fraction
        };
        let left = interpolate(0);
        let right = if src_channels == 1 { left } else { interpolate(1) };
        out.push(Frame::new(left, right));
    }
    out
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn read_movie_bytes(path: &Path) -> Result<Vec<u8>> {
    crate::resource::read_file_bytes(path)
        .with_context(|| format!("read movie file: {}", path.display()))
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn read_movie_bytes(path: &Path) -> Result<Vec<u8>> {
    crate::resource::read_file_bytes(path)
        .with_context(|| format!("read movie file: {}", path.display()))
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn read_omv_header_for_path(path: &Path) -> Result<siglus_assets::omv::OmvHeader> {
    let bytes = read_movie_bytes(path)?;
    read_omv_header_from_bytes(&bytes)
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn read_omv_header_for_path(path: &Path) -> Result<siglus_assets::omv::OmvHeader> {
    siglus_assets::omv::OmvFile::read_header(path)
}

fn read_omv_header_from_bytes(buf: &[u8]) -> Result<siglus_assets::omv::OmvHeader> {
    if buf.len() < 0x58 {
        bail!("OMV header too small");
    }
    let header_size = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let version = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let theora_type = u32::from_le_bytes([buf[0x28], buf[0x29], buf[0x2a], buf[0x2b]]);
    let display_width = u32::from_le_bytes([buf[0x2c], buf[0x2d], buf[0x2e], buf[0x2f]]);
    let display_height = u32::from_le_bytes([buf[0x30], buf[0x31], buf[0x32], buf[0x33]]);
    let frame_time_us = u32::from_le_bytes([buf[0x3c], buf[0x3d], buf[0x3e], buf[0x3f]]);
    let max_data_size = u32::from_le_bytes([buf[0x40], buf[0x41], buf[0x42], buf[0x43]]);
    let page_count_hint = u32::from_le_bytes([buf[0x4c], buf[0x4d], buf[0x4e], buf[0x4f]]);
    let packet_count_hint = u32::from_le_bytes([buf[0x50], buf[0x51], buf[0x52], buf[0x53]]);
    if header_size < 0x58 {
        bail!("invalid OMV header size: {header_size:#x}");
    }
    if theora_type > siglus_assets::omv::OMV_THEORA_TYPE_YUV {
        bail!("invalid OMV theora type: {theora_type}");
    }
    if display_width == 0 || display_height == 0 {
        bail!("invalid OMV display size: {}x{}", display_width, display_height);
    }
    Ok(siglus_assets::omv::OmvHeader {
        header_size,
        version,
        theora_type,
        display_width,
        display_height,
        frame_time_us,
        max_data_size,
        page_count_hint,
        packet_count_hint,
    })
}

fn extract_ogg_from_bytes(bytes: &[u8]) -> Result<Vec<u8>> {
    let needle = b"OggS";
    let pos = bytes
        .windows(needle.len())
        .position(|w| w == needle)
        .ok_or_else(|| anyhow!("OggS not found in OMV payload"))?;
    Ok(bytes[pos..].to_vec())
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn decode_mpeg2_asset_from_bytes(path: &Path, bytes: Vec<u8>) -> Result<MovieAsset> {
    let mut width = None;
    let mut height = None;
    let mut fps = None;
    if let Some(h) = siglus_assets::mpeg2::find_sequence_header(&bytes[..bytes.len().min(MPEG2_HEADER_PROBE_BYTES)]) {
        width = Some(h.width as u32);
        height = Some(h.height as u32);
        fps = siglus_assets::mpeg2::fps_from_frame_rate_code(h.frame_rate_code);
    }

    let mut frames: Vec<Arc<RgbaImage>> = Vec::new();
    let mut audio_samples: Vec<i16> = Vec::new();
    let mut audio_channels: Option<u16> = None;
    let mut audio_sample_rate: Option<u32> = None;
    let mut dropped_audio_format_changes = 0u32;
    let mut pipeline = na_mpeg2_decoder::MpegAvPipeline::new();
    pipeline
        .push_with(&bytes, None, |ev| match ev {
            na_mpeg2_decoder::MpegAvEvent::Video(f) => {
                let w = f.width;
                let h = f.height;
                frames.push(Arc::new(RgbaImage { width: w, height: h, center_x: 0, center_y: 0, rgba: f.rgba }));
            }
            na_mpeg2_decoder::MpegAvEvent::Audio(a) => {
                append_mpeg2_audio_chunk_for_asset(
                    path,
                    &mut audio_channels,
                    &mut audio_sample_rate,
                    &mut audio_samples,
                    &mut dropped_audio_format_changes,
                    a,
                );
            }
        })
        .context("mpeg2 wasm full decode")?;
    pipeline.flush_with(|ev| match ev {
        na_mpeg2_decoder::MpegAvEvent::Video(f) => {
            let w = f.width;
            let h = f.height;
            frames.push(Arc::new(RgbaImage { width: w, height: h, center_x: 0, center_y: 0, rgba: f.rgba }));
        }
        na_mpeg2_decoder::MpegAvEvent::Audio(a) => {
            append_mpeg2_audio_chunk_for_asset(
                path,
                &mut audio_channels,
                &mut audio_sample_rate,
                &mut audio_samples,
                &mut dropped_audio_format_changes,
                a,
            );
        }
    })?;

    if frames.is_empty() {
        bail!("mpeg2 decoder produced no frames: {}", path.display());
    }
    let audio = build_movie_audio_from_parts(path, audio_samples, audio_channels, audio_sample_rate)?;
    let audio_duration_ms = audio.as_ref().and_then(|a| a.duration_ms);
    let first = frames.first().expect("frames not empty");
    let info = MovieInfo {
        path: path.to_path_buf(),
        width: width.or(Some(first.width)),
        height: height.or(Some(first.height)),
        fps,
        decoded_frames: Some(frames.len()),
        audio_duration_ms,
    };
    Ok(MovieAsset { info, frames, audio })
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn decode_wmv_asset_from_bytes(path: &Path, bytes: Vec<u8>) -> Result<MovieAsset> {
    let mut decoder = wmv_decoder::AsfWmv2Decoder::open(Cursor::new(bytes.clone()))
        .with_context(|| format!("open ASF/WMV decoder: {}", path.display()))?;
    let stream_info = decoder.video_stream_info().clone();
    let mut frames = Vec::<Arc<RgbaImage>>::new();
    let mut pts = Vec::<u64>::new();
    while let Some(decoded) = decoder
        .next_frame()
        .with_context(|| format!("decode ASF/WMV frame: {}", path.display()))?
    {
        pts.push(decoded.pts_ms as u64);
        frames.push(Arc::new(wmv_yuv_frame_to_rgba(&decoded.frame)));
    }
    if frames.is_empty() {
        bail!("wmv decoder produced no frames: {}", path.display());
    }

    let timeline_origin_ms = pts.first().copied().unwrap_or(0);
    for pts_ms in &mut pts {
        *pts_ms = pts_ms.saturating_sub(timeline_origin_ms);
    }
    let cancel = AtomicBool::new(false);
    let audio = decode_wmv_audio_full_from_reader(
        Cursor::new(bytes),
        &cancel,
        timeline_origin_ms,
    )?;
    let fps = wmv_fps_from_pts(&pts);
    let video_duration_ms = pts.last().copied().map(|last| {
        last.saturating_add(wmv_frame_duration_ms(&pts).unwrap_or(0))
    });
    let audio_duration_ms = audio.as_ref().map(MovieAudio::end_ms);
    let total_ms = match (audio_duration_ms, video_duration_ms) {
        (Some(a), Some(v)) => Some(a.max(v)),
        (Some(a), None) => Some(a),
        (None, Some(v)) => Some(v),
        (None, None) => None,
    };
    let info = MovieInfo {
        path: path.to_path_buf(),
        width: (stream_info.width > 0).then_some(stream_info.width),
        height: (stream_info.height > 0).then_some(stream_info.height),
        fps,
        decoded_frames: Some(frames.len()),
        audio_duration_ms: total_ms,
    };
    Ok(MovieAsset { info, frames, audio })
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn decode_omv_asset_from_bytes(path: &Path, bytes: Vec<u8>) -> Result<MovieAsset> {
    let header = read_omv_header_from_bytes(&bytes).ok();
    let ogg_data = extract_ogg_from_bytes(&bytes)
        .with_context(|| format!("read embedded ogg: {}", path.display()))?;
    let mut tf = siglus_omv_decoder::TheoraFile::open_from_memory(ogg_data)
        .with_context(|| format!("open theora: {}", path.display()))?;
    let vinfo = tf.info();
    let display_w = header.as_ref().map(|h| h.display_width as i32).unwrap_or(vinfo.width);
    let display_h = header.as_ref().map(|h| h.display_height as i32).unwrap_or(vinfo.height);
    let width = display_w.max(1) as u32;
    let height = display_h.max(1) as u32;
    let fps = header.as_ref().and_then(|h| {
        if h.frame_time_us != 0 {
            Some(1_000_000.0 / (h.frame_time_us as f32))
        } else if vinfo.fps > 0.0 {
            Some(vinfo.fps as f32)
        } else {
            None
        }
    }).or_else(|| (vinfo.fps > 0.0).then_some(vinfo.fps as f32));
    let theora_type = header
        .as_ref()
        .map(|h| h.theora_type)
        .unwrap_or(siglus_assets::omv::OMV_THEORA_TYPE_YUV);
    let (_uv_w, _uv_h, y_len, u_len, v_len) =
        omv_plane_layout(vinfo.width, vinfo.height, theora_type, vinfo.fmt);
    let mut packed = vec![0u8; y_len.saturating_add(u_len).saturating_add(v_len)];
    let mut frames = Vec::<Arc<RgbaImage>>::new();
    while tf.read_video_frame(&mut packed)? {
        let rgba = convert_omv_frame(
            &packed,
            vinfo.width,
            vinfo.height,
            vinfo.fmt,
            display_h,
            theora_type,
        );
        frames.push(Arc::new(RgbaImage { width, height, center_x: 0, center_y: 0, rgba }));
    }
    if frames.is_empty() {
        bail!("omv decoder produced no frames: {}", path.display());
    }
    tf.reset();
    let audio = decode_omv_audio(&mut tf)?;
    let frame_time_ms = omv_frame_duration_ms(header.as_ref(), fps);
    let video_duration_ms = frame_time_ms.map(|ms| ((frames.len() as f64) * ms).round().max(1.0) as u64);
    let audio_duration_ms = audio.as_ref().and_then(|a| a.duration_ms).or(video_duration_ms);
    let info = MovieInfo {
        path: path.to_path_buf(),
        width: Some(width),
        height: Some(height),
        fps,
        decoded_frames: Some(frames.len()),
        audio_duration_ms,
    };
    Ok(MovieAsset { info, frames, audio })
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn append_mpeg2_audio_chunk_for_asset(
    _path: &Path,
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
        (Some(_), Some(_)) => {
            *dropped_audio_format_changes = (*dropped_audio_format_changes).saturating_add(1);
            return;
        }
        _ => return,
    }
    audio_samples.extend(a.samples.into_iter().map(f32_to_i16_sample));
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn build_movie_audio_from_parts(
    path: &Path,
    audio_samples: Vec<i16>,
    audio_channels: Option<u16>,
    audio_sample_rate: Option<u32>,
) -> Result<Option<MovieAudio>> {
    match (audio_channels, audio_sample_rate, audio_samples.is_empty()) {
        (Some(channels), Some(sample_rate), false) => {
            if channels == 0 || sample_rate == 0 {
                bail!(
                    "movie audio stream has invalid format in {}: channels={} sample_rate={}",
                    path.display(), channels, sample_rate
                );
            }
            let frames_len = (audio_samples.len() as u64) / (channels as u64);
            let duration_ms = Some(((frames_len as f64) * 1000.0 / sample_rate as f64).round() as u64);
            Ok(Some(MovieAudio {
                samples: Arc::new(audio_samples),
                mpeg_stream: None,
                wmv_stream: None,
                channels,
                sample_rate,
                start_ms: 0,
                duration_ms,
            }))
        }
        (None, None, true) => Ok(None),
        (Some(_), Some(_), true) => Ok(None),
        _ => bail!("movie audio decoder produced incomplete format metadata for {}", path.display()),
    }
}

fn decode_asset_for_path(path: &Path) -> Result<MovieAsset> {
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        let bytes = read_movie_bytes(path)?;
        if ext == "omv" {
            return decode_omv_asset_from_bytes(path, bytes);
        }
        if is_wmv_extension(&ext) {
            return decode_wmv_asset_from_bytes(path, bytes);
        }
        return decode_mpeg2_asset_from_bytes(path, bytes);
    }
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        if ext == "omv" {
            decode_omv_asset(path)
        } else if is_wmv_extension(&ext) {
            decode_wmv_asset(path)
        } else {
            decode_mpeg2_asset(path)
        }
    }
}

fn wmv_frame_duration_ms(pts: &[u64]) -> Option<u64> {
    let mut deltas = pts
        .windows(2)
        .filter_map(|pair| {
            let delta = pair[1].saturating_sub(pair[0]);
            (delta > 0 && delta <= 1_000).then_some(delta)
        })
        .collect::<Vec<_>>();
    if deltas.is_empty() {
        return None;
    }
    deltas.sort_unstable();
    Some(deltas[deltas.len() / 2])
}

fn wmv_fps_from_pts(pts: &[u64]) -> Option<f32> {
    wmv_frame_duration_ms(pts)
        .filter(|delta| *delta > 0)
        .map(|delta| 1000.0 / delta as f32)
}

fn wmv_rebase_duration_ms(source_duration_ms: u64, timeline_origin_ms: u64) -> Option<u64> {
    let rebased = source_duration_ms.saturating_sub(timeline_origin_ms);
    (rebased > 0).then_some(rebased)
}

fn wmv_audio_alignment_frames(
    first_audio_pts_ms: u64,
    timeline_origin_ms: u64,
    sample_rate: u32,
) -> (usize, usize) {
    if sample_rate == 0 {
        return (0, 0);
    }
    let silence_ms = first_audio_pts_ms.saturating_sub(timeline_origin_ms);
    let skip_ms = timeline_origin_ms.saturating_sub(first_audio_pts_ms);
    let initial_silence_frames =
        (((silence_ms as u128) * sample_rate as u128 + 999) / 1000)
            .min(usize::MAX as u128) as usize;
    let source_skip_frames = ((skip_ms as u128) * sample_rate as u128 / 1000)
        .min(usize::MAX as u128) as usize;
    (initial_silence_frames, source_skip_frames)
}

fn probe_wmv_first_video_pts_ms(path: &Path) -> Result<Option<u64>> {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    let mut decoder = {
        let bytes = read_movie_bytes(path)?;
        wmv_decoder::AsfWmv2Decoder::open(Cursor::new(bytes))
            .with_context(|| format!("open ASF/WMV timeline probe: {}", path.display()))?
    };
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    let mut decoder = {
        let file = fs::File::open(path)
            .with_context(|| format!("open WMV timeline probe: {}", path.display()))?;
        wmv_decoder::AsfWmv2Decoder::open(BufReader::new(file))
            .with_context(|| format!("open WMV timeline probe decoder: {}", path.display()))?
    };

    Ok(decoder
        .next_frame()
        .with_context(|| format!("decode WMV timeline probe: {}", path.display()))?
        .map(|frame| frame.pts_ms as u64))
}

fn decode_wmv_preview_frame(path: &Path) -> Result<Arc<RgbaImage>> {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    let mut decoder = {
        let bytes = read_movie_bytes(path)?;
        wmv_decoder::AsfWmv2Decoder::open(Cursor::new(bytes))
            .with_context(|| format!("open WMV preview decoder: {}", path.display()))?
    };
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    let mut decoder = {
        let file = fs::File::open(path)
            .with_context(|| format!("open WMV preview: {}", path.display()))?;
        wmv_decoder::AsfWmv2Decoder::open(BufReader::new(file))
            .with_context(|| format!("open WMV preview decoder: {}", path.display()))?
    };

    let decoded = decoder
        .next_frame()
        .with_context(|| format!("decode WMV preview: {}", path.display()))?
        .ok_or_else(|| anyhow!("wmv preview frame missing: {}", path.display()))?;
    Ok(Arc::new(wmv_yuv_frame_to_rgba(&decoded.frame)))
}

fn decode_mpeg2_preview_frame(path: &Path) -> Result<Arc<RgbaImage>> {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        let bytes = read_movie_bytes(path)?;
        let asset = decode_mpeg2_asset_from_bytes(path, bytes)?;
        return asset
            .frames
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("mpeg2 preview frame missing: {}", path.display()));
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
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
                            center_x: 0,
                            center_y: 0,
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
                        center_x: 0,
                        center_y: 0,
                        rgba,
                    }));
                }
            })?;
        }
        first.ok_or_else(|| anyhow!("mpeg2 preview frame missing: {}", path.display()))
    }
}

fn decode_omv_preview_frame(path: &Path) -> Result<Arc<RgbaImage>> {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        let bytes = read_movie_bytes(path)?;
        let asset = decode_omv_asset_from_bytes(path, bytes)?;
        return asset
            .frames
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("omv preview frame missing: {}", path.display()));
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        let omv = siglus_assets::omv::OmvFile::open(path)
            .with_context(|| format!("open OMV index: {}", path.display()))?;
        let reader = omv
            .open_embedded_ogg_reader(path)
            .with_context(|| format!("open embedded Ogg stream: {}", path.display()))?;
        let mut stream = siglus_omv_decoder::TheoraVideoStream::open(reader)
            .with_context(|| format!("open streaming Theora decoder: {}", path.display()))?;
        let vinfo = stream.info();
        let packed = stream
            .read_video_frame()?
            .ok_or_else(|| anyhow!("omv preview frame missing: {}", path.display()))?;
        let display_h = omv.header.display_height as i32;
        let width = omv.header.display_width.max(1);
        let height = display_h.max(1) as u32;
        let rgba = convert_omv_frame(
            &packed,
            vinfo.width,
            vinfo.height,
            vinfo.fmt,
            display_h,
            omv.header.theora_type,
        );
        Ok(Arc::new(RgbaImage {
            width,
            height,
            center_x: 0,
            center_y: 0,
            rgba,
        }))
    }
}


fn open_wmv_wma_decoder<R: Read + Seek>(
    mut reader: R,
) -> Result<Option<wmv_decoder::AsfWmaDecoder<R>>> {
    let asf = wmv_decoder::asf::AsfFile::open(&mut reader)
        .context("probe ASF audio streams")?;
    if asf.audio_streams.is_empty() {
        return Ok(None);
    }
    if !asf
        .audio_streams
        .iter()
        .any(|stream| matches!(stream.format_tag, 0x0160 | 0x0161 | 0x0162))
    {
        let tags = asf
            .audio_streams
            .iter()
            .map(|stream| format!("{:#06x}", stream.format_tag))
            .collect::<Vec<_>>()
            .join(", ");
        bail!("unsupported ASF/WMA audio format tag(s): {tags}");
    }
    reader.seek(SeekFrom::Start(0))?;
    let decoder = wmv_decoder::AsfWmaDecoder::open(reader)
        .context("open ASF/WMA decoder")?;
    Ok(Some(decoder))
}

fn decode_wmv_audio_full_from_reader<R: Read + Seek>(
    reader: R,
    cancel: &AtomicBool,
    timeline_origin_ms: u64,
) -> Result<Option<MovieAudio>> {
    let Some(mut decoder) = open_wmv_wma_decoder(reader)? else {
        return Ok(None);
    };
    let channels = decoder.channels();
    let sample_rate = decoder.sample_rate();
    if channels == 0 || sample_rate == 0 {
        return Ok(None);
    }

    let mut samples = Vec::<i16>::new();
    let mut first_pts_ms = None::<u64>;
    while !cancel.load(Ordering::Acquire) {
        let decoded = decoder.next_frame()?;
        let Some(decoded) = decoded else {
            break;
        };
        first_pts_ms.get_or_insert(decoded.pts_ms as u64);
        samples.extend(decoded.frame.samples.iter().copied().map(f32_to_i16_sample));
    }
    if cancel.load(Ordering::Acquire) || samples.is_empty() {
        return Ok(None);
    }
    let first_audio_pts_ms = first_pts_ms.unwrap_or(timeline_origin_ms);
    let (initial_silence_frames, source_skip_frames) = wmv_audio_alignment_frames(
        first_audio_pts_ms,
        timeline_origin_ms,
        sample_rate,
    );
    let source_skip_samples = source_skip_frames.saturating_mul(channels as usize);
    if source_skip_samples >= samples.len() {
        return Ok(None);
    }
    if source_skip_samples > 0 {
        samples.drain(..source_skip_samples);
    }
    if initial_silence_frames > 0 {
        let mut timeline_samples = Vec::with_capacity(
            initial_silence_frames
                .saturating_mul(channels as usize)
                .saturating_add(samples.len()),
        );
        timeline_samples.resize(
            initial_silence_frames.saturating_mul(channels as usize),
            0,
        );
        timeline_samples.extend_from_slice(&samples);
        samples = timeline_samples;
    }
    let frames = samples.len() as u64 / channels as u64;
    let duration_ms = Some(
        ((frames as f64) * 1000.0 / sample_rate as f64)
            .round()
            .max(1.0) as u64,
    );
    Ok(Some(MovieAudio {
        samples: Arc::new(samples),
        mpeg_stream: None,
        wmv_stream: None,
        channels,
        sample_rate,
        start_ms: 0,
        duration_ms,
    }))
}

fn decode_wmv_audio_for_path(
    path: &Path,
    cancel: &AtomicBool,
) -> Result<Option<MovieAudio>> {
    // Native WMV playback must become ready after a bounded head probe, not after
    // decoding the whole WMA soundtrack. `WmvMovieAudioDecoder` already provides
    // the forward-only Kira streaming path; this probe supplies only the metadata
    // and A/V timeline alignment it needs. MPEG uses its own independent probe and
    // decoder path and is intentionally untouched here.
    let timeline_origin_ms = probe_wmv_first_video_pts_ms(path)?.unwrap_or(0);
    if cancel.load(Ordering::Acquire) {
        return Ok(None);
    }

    let file = fs::File::open(path)
        .with_context(|| format!("open WMV audio source: {}", path.display()))?;
    let Some(mut decoder) = open_wmv_wma_decoder(BufReader::new(file))? else {
        return Ok(None);
    };
    let channels = decoder.channels();
    let sample_rate = decoder.sample_rate();
    if channels == 0 || sample_rate == 0 {
        return Ok(None);
    }

    // Decode only until the first PCM frame. Besides validating that the selected
    // WMA stream can start, its ASF PTS determines whether the movie timeline needs
    // leading silence or source-frame skipping. The streaming decoder later reopens
    // the file from byte zero and consumes every compressed media object in order.
    let first = decoder
        .next_frame()
        .with_context(|| format!("probe first WMA frame: {}", path.display()))?;
    if cancel.load(Ordering::Acquire) {
        return Ok(None);
    }
    let Some(first) = first else {
        return Ok(None);
    };
    let first_audio_pts_ms = first.pts_ms as u64;
    let (initial_silence_frames, source_skip_frames) = wmv_audio_alignment_frames(
        first_audio_pts_ms,
        timeline_origin_ms,
        sample_rate,
    );

    // ASF File Properties carries the finite movie duration. Rebase it to the same
    // local timeline used by `poll_wmv_stream_frame_for_path` (whose zero is the first
    // presented video PTS), then let Kira request decoded WMA frames incrementally.
    let duration_ms = decoder
        .duration_ms()
        .and_then(|duration| wmv_rebase_duration_ms(duration, timeline_origin_ms));

    let Some(duration_ms) = duration_ms.filter(|duration| *duration > 0) else {
        // Rare malformed/streaming ASF files may omit a usable play duration. Keep
        // the old static fallback for that case because Kira requires a finite
        // `num_frames`; normal WMV files never take this full-decode path.
        let file = fs::File::open(path)
            .with_context(|| format!("reopen WMV audio fallback: {}", path.display()))?;
        return decode_wmv_audio_full_from_reader(
            BufReader::new(file),
            cancel,
            timeline_origin_ms,
        )
        .with_context(|| format!("decode WMA audio fallback: {}", path.display()));
    };

    let num_frames = (((duration_ms as u128) * sample_rate as u128 + 999) / 1000)
        .max(1)
        .min(usize::MAX as u128) as usize;

    Ok(Some(MovieAudio {
        samples: Arc::new(Vec::new()),
        mpeg_stream: None,
        wmv_stream: Some(WmvStreamAudio {
            path: Arc::new(path.to_path_buf()),
            num_frames,
            initial_silence_frames,
            source_skip_frames,
        }),
        channels,
        sample_rate,
        start_ms: 0,
        duration_ms: Some(duration_ms),
    }))
}

fn decode_mpeg2_audio_for_path(
    path: &Path,
    cancel: &AtomicBool,
) -> Result<Option<MovieAudio>> {
    if let Some(audio) = quick_probe_mpeg2_stream_audio(path, cancel)? {
        return Ok(Some(audio));
    }

    // Files without usable MPEG-audio PTS (and DVD private audio) retain the
    // compatibility fallback.  Normal MP1/2/3 program streams never take this
    // full-scan path, so playback can start after bounded head/tail reads.
    decode_mpeg2_audio_full_probe_or_static(path, cancel)
}

fn quick_probe_mpeg2_stream_audio(
    path: &Path,
    cancel: &AtomicBool,
) -> Result<Option<MovieAudio>> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("open movie file: {}", path.display()))?;
    let file_len = file
        .metadata()
        .with_context(|| format!("stat movie file: {}", path.display()))?
        .len();
    if file_len == 0 {
        return Ok(None);
    }

    let mut head_probe = na_mpeg2_decoder::MpegAudioProbePipeline::new();
    let mut buf = vec![0u8; MPEG2_STREAM_CHUNK_BYTES];
    let mut head_read = 0u64;
    let head_limit = file_len.min(MPEG_AUDIO_HEAD_PROBE_MAX_BYTES);
    let mut prefix = Vec::new();
    while head_read < head_limit {
        if cancel.load(Ordering::Acquire) {
            return Ok(None);
        }
        let want = (head_limit - head_read).min(buf.len() as u64) as usize;
        let n = file
            .read(&mut buf[..want])
            .with_context(|| format!("probe movie audio head: {}", path.display()))?;
        if n == 0 {
            break;
        }
        if prefix.len() < 188 * 3 {
            let take = (188 * 3 - prefix.len()).min(n);
            prefix.extend_from_slice(&buf[..take]);
        }
        head_probe.push(&buf[..n], None);
        head_read = head_read.saturating_add(n as u64);
        if head_probe
            .stream_info()
            .is_some_and(|info| info.first_video_pts_90k.is_some())
        {
            break;
        }
    }
    let Some(head) = head_probe.stream_info() else {
        return Ok(None);
    };
    if head.sample_rate == 0 || head.channels == 0 {
        return Ok(None);
    }
    let transport_stream = is_transport_stream_prefix(&prefix);

    let mut tail_window = MPEG_AUDIO_TAIL_PROBE_INITIAL_BYTES.min(file_len);
    let tail = loop {
        if cancel.load(Ordering::Acquire) {
            return Ok(None);
        }
        let mut start = file_len.saturating_sub(tail_window);
        if transport_stream {
            start -= start % 188;
        }
        file.seek(SeekFrom::Start(start))
            .with_context(|| format!("seek movie audio tail: {}", path.display()))?;
        let mut tail_bytes = Vec::with_capacity((file_len - start).min(usize::MAX as u64) as usize);
        file.read_to_end(&mut tail_bytes)
            .with_context(|| format!("read movie audio tail: {}", path.display()))?;
        let probe_bytes = if transport_stream {
            tail_bytes.as_slice()
        } else if let Some(pack) = find_byte_pattern(&tail_bytes, b"\0\0\x01\xba") {
            &tail_bytes[pack..]
        } else {
            tail_bytes.as_slice()
        };
        let mut tail_probe = na_mpeg2_decoder::MpegAudioTailProbePipeline::new();
        tail_probe.push(probe_bytes);
        if let Some(info) = tail_probe.finish().filter(|info| {
            info.sample_rate == head.sample_rate && info.channels == head.channels
        }) {
            break Some(info);
        }
        if tail_window >= file_len || tail_window >= MPEG_AUDIO_TAIL_PROBE_MAX_BYTES {
            break None;
        }
        tail_window = tail_window
            .saturating_mul(2)
            .min(file_len)
            .min(MPEG_AUDIO_TAIL_PROBE_MAX_BYTES);
    };
    let Some(tail) = tail else {
        return Ok(None);
    };

    let origin_pts_90k = head
        .first_video_pts_90k
        .map(|video| earlier_mpeg_pts_90k(video, head.first_audio_pts_90k))
        .unwrap_or(head.first_audio_pts_90k);
    let delay_ticks = mpeg_pts_delta_90k(head.first_audio_pts_90k, origin_pts_90k)
        .unwrap_or(0);
    let before_tail_ticks = match mpeg_pts_delta_90k(
        tail.anchor_pts_90k,
        head.first_audio_pts_90k,
    ) {
        Some(value) => value,
        None => return Ok(None),
    };
    let delay_frames = pts90k_ticks_to_audio_frames(delay_ticks, head.sample_rate);
    let before_tail_frames =
        pts90k_ticks_to_audio_frames(before_tail_ticks, head.sample_rate);
    let num_frames = delay_frames
        .saturating_add(before_tail_frames)
        .saturating_add(tail.output_frames);
    if num_frames == 0 {
        return Ok(None);
    }
    let tail_ticks = audio_frames_to_pts90k_ticks(tail.output_frames, head.sample_rate);
    let audio_end_pts_90k = tail
        .anchor_pts_90k
        .saturating_add(tail_ticks)
        & ((1i64 << 33) - 1);
    let duration_ms = Some(
        ((num_frames as u128).saturating_mul(1000) / head.sample_rate as u128)
            .min(u64::MAX as u128) as u64,
    );

    Ok(Some(MovieAudio {
        samples: Arc::new(Vec::new()),
        mpeg_stream: Some(MpegStreamAudio {
            path: Arc::new(path.to_path_buf()),
            num_frames,
            first_audio_pts_ms: pts90k_to_ms(head.first_audio_pts_90k),
            first_video_pts_ms: head.first_video_pts_90k.map(pts90k_to_ms),
            first_audio_pts_90k: head.first_audio_pts_90k,
            first_video_pts_90k: head.first_video_pts_90k,
            audio_end_pts_90k,
            file_len,
            transport_stream,
        }),
        wmv_stream: None,
        channels: head.channels,
        sample_rate: head.sample_rate,
        start_ms: 0,
        duration_ms,
    }))
}

fn decode_mpeg2_audio_full_probe_or_static(
    path: &Path,
    cancel: &AtomicBool,
) -> Result<Option<MovieAudio>> {
    let mut file = fs::File::open(path)
        .with_context(|| format!("open movie file: {}", path.display()))?;
    let file_len = file.metadata().map(|m| m.len()).unwrap_or(0);
    let mut probe = na_mpeg2_decoder::MpegAudioProbePipeline::new();
    let mut buf = vec![0u8; MPEG2_STREAM_CHUNK_BYTES];
    let mut prefix = Vec::new();

    loop {
        if cancel.load(Ordering::Acquire) {
            return Ok(None);
        }
        let n = file
            .read(&mut buf)
            .with_context(|| format!("probe movie audio stream: {}", path.display()))?;
        if n == 0 {
            break;
        }
        if prefix.len() < 188 * 3 {
            let take = (188 * 3 - prefix.len()).min(n);
            prefix.extend_from_slice(&buf[..take]);
        }
        probe.push(&buf[..n], None);
    }

    if let Some(info) = probe.finish() {
        let duration_ms = Some(
            ((info.num_frames as u128).saturating_mul(1000) / info.sample_rate as u128)
                .min(u64::MAX as u128) as u64,
        );
        let audio_duration_ticks = audio_frames_to_pts90k_ticks(
            info.num_frames.saturating_sub(pts90k_ticks_to_audio_frames(
                mpeg_pts_delta_90k(
                    info.first_audio_pts_90k,
                    info.first_video_pts_90k
                        .map(|video| earlier_mpeg_pts_90k(video, info.first_audio_pts_90k))
                        .unwrap_or(info.first_audio_pts_90k),
                )
                .unwrap_or(0),
                info.sample_rate,
            )),
            info.sample_rate,
        );
        return Ok(Some(MovieAudio {
            samples: Arc::new(Vec::new()),
            mpeg_stream: Some(MpegStreamAudio {
                path: Arc::new(path.to_path_buf()),
                num_frames: info.num_frames,
                first_audio_pts_ms: info.first_audio_pts_ms,
                first_video_pts_ms: info.first_video_pts_ms,
                first_audio_pts_90k: info.first_audio_pts_90k,
                first_video_pts_90k: info.first_video_pts_90k,
                audio_end_pts_90k: info
                    .first_audio_pts_90k
                    .saturating_add(audio_duration_ticks),
                file_len,
                transport_stream: is_transport_stream_prefix(&prefix),
            }),
            wmv_stream: None,
            channels: info.channels,
            sample_rate: info.sample_rate,
            start_ms: 0,
            duration_ms,
        }));
    }

    decode_mpeg2_audio_static_fallback(path, cancel)
}

fn is_transport_stream_prefix(prefix: &[u8]) -> bool {
    prefix.len() >= 188 * 3
        && prefix[0] == 0x47
        && prefix[188] == 0x47
        && prefix[376] == 0x47
}

fn find_byte_pattern(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|window| window == needle)
}

fn earlier_mpeg_pts_90k(lhs: i64, rhs: i64) -> i64 {
    if mpeg_pts_delta_90k(lhs, rhs).is_some() {
        rhs
    } else {
        lhs
    }
}

fn mpeg_pts_delta_90k(later: i64, earlier: i64) -> Option<i64> {
    const PTS_WRAP: i64 = 1i64 << 33;
    let mut delta = later.saturating_sub(earlier);
    if delta < 0 {
        delta = delta.saturating_add(PTS_WRAP);
    }
    // A movie longer than half the 33-bit PTS range is outside the supported
    // Siglus media use case and more likely indicates unrelated/missing PTS.
    (delta <= PTS_WRAP / 2).then_some(delta)
}

fn pts90k_ticks_to_audio_frames(ticks: i64, sample_rate: u32) -> usize {
    if ticks <= 0 || sample_rate == 0 {
        return 0;
    }
    (((ticks as i128) * (sample_rate as i128) + 45_000) / 90_000)
        .clamp(0, usize::MAX as i128) as usize
}

fn audio_frames_to_pts90k_ticks(frames: usize, sample_rate: u32) -> i64 {
    if frames == 0 || sample_rate == 0 {
        return 0;
    }
    (((frames as i128) * 90_000 + sample_rate as i128 / 2) / sample_rate as i128)
        .clamp(0, i64::MAX as i128) as i64
}

fn pts90k_to_ms(pts: i64) -> i64 {
    ((pts as i128) * 1000 / 90_000)
        .clamp(i64::MIN as i128, i64::MAX as i128) as i64
}

fn decode_mpeg2_audio_static_fallback(
    path: &Path,
    cancel: &AtomicBool,
) -> Result<Option<MovieAudio>> {
    let mut chunks = Vec::<na_mpeg2_decoder::MpegAudioF32>::new();
    let mut file = fs::File::open(path)
        .with_context(|| format!("open movie file: {}", path.display()))?;
    let mut pipeline = na_mpeg2_decoder::MpegAudioPipeline::new();
    let mut buf = vec![0u8; MPEG2_STREAM_CHUNK_BYTES];

    loop {
        if cancel.load(Ordering::Acquire) {
            return Ok(None);
        }
        let n = file
            .read(&mut buf)
            .with_context(|| format!("read movie audio stream: {}", path.display()))?;
        if n == 0 {
            break;
        }
        pipeline
            .push_with(&buf[..n], None, |audio| chunks.push(audio))
            .context("mpeg2 audio-only decode")?;
    }
    if cancel.load(Ordering::Acquire) {
        return Ok(None);
    }
    pipeline
        .flush_with(|audio| chunks.push(audio))
        .context("mpeg2 audio-only flush")?;

    build_mpeg2_audio_timeline(path, chunks, pipeline.first_video_pts_ms(), cancel)
}

fn build_mpeg2_audio_timeline(
    path: &Path,
    chunks: Vec<na_mpeg2_decoder::MpegAudioF32>,
    first_video_pts_ms: Option<i64>,
    cancel: &AtomicBool,
) -> Result<Option<MovieAudio>> {
    let Some(first) = chunks
        .iter()
        .find(|chunk| chunk.channels > 0 && chunk.sample_rate > 0 && !chunk.samples.is_empty())
    else {
        return Ok(None);
    };

    let channels = first.channels;
    let sample_rate = first.sample_rate;
    let first_audio_pts_ms = chunks
        .iter()
        .find(|chunk| chunk.channels > 0 && chunk.sample_rate > 0 && !chunk.samples.is_empty())
        .map(|chunk| chunk.pts_ms)
        .unwrap_or(0);
    let timeline_origin_ms = match first_video_pts_ms {
        Some(video_pts) => video_pts.min(first_audio_pts_ms),
        None => first_audio_pts_ms,
    };

    // MPEG audio is a continuous elementary stream. PES PTS values are sparse
    // anchors, not edit instructions for every decoded frame. The previous
    // implementation rounded every anchor to integer milliseconds and then
    // inserted silence or discarded PCM whenever the rounded target differed
    // from the accumulated sample count. On 44.1 kHz MP2 this periodically
    // manufactured audible gaps/overlaps (the reported "disc skipping" sound).
    //
    // Preserve only the initial A/V offset here and append decoded audio frames
    // contiguously. The decoder already emits frames in elementary-stream order.
    let initial_delay_ms = first_audio_pts_ms.saturating_sub(timeline_origin_ms).max(0);
    let initial_delay_frames = ((initial_delay_ms as i128) * (sample_rate as i128) / 1000)
        .clamp(0, usize::MAX as i128) as usize;
    let mut samples = vec![
        0i16;
        initial_delay_frames.saturating_mul(channels as usize)
    ];
    let mut decoded_frames = 0usize;
    let mut discontinuity_count = 0usize;
    let mut max_abs_drift_frames = 0usize;
    let tolerance_frames = ((sample_rate as u64) * 5 / 1000).max(1) as usize;

    for chunk in chunks {
        if cancel.load(Ordering::Acquire) {
            return Ok(None);
        }
        if chunk.channels == 0 || chunk.sample_rate == 0 || chunk.samples.is_empty() {
            continue;
        }
        let converted = convert_movie_audio_chunk(
            &chunk.samples,
            chunk.channels,
            chunk.sample_rate,
            channels,
            sample_rate,
        );
        if converted.is_empty() {
            continue;
        }

        // Keep PTS drift as diagnostics only. Do not splice the PCM stream at
        // each PES boundary: those boundaries may split an MPEG audio frame and
        // integer-millisecond timestamps cannot represent 1152 / 44100 seconds.
        let relative_pts_ms = chunk.pts_ms.saturating_sub(first_audio_pts_ms);
        if relative_pts_ms >= 0 {
            let pts_frames = ((relative_pts_ms as i128) * (sample_rate as i128) / 1000)
                .clamp(0, usize::MAX as i128) as usize;
            let abs_drift = pts_frames.abs_diff(decoded_frames);
            max_abs_drift_frames = max_abs_drift_frames.max(abs_drift);
            if abs_drift > tolerance_frames {
                discontinuity_count = discontinuity_count.saturating_add(1);
            }
        }

        decoded_frames = decoded_frames
            .saturating_add(converted.len() / channels as usize);
        samples.extend_from_slice(&converted);
    }

    if (std::env::var_os("SG_MOVIE_TRACE").is_some()
        || std::env::var_os("SG_DEBUG").is_some())
        && discontinuity_count > 0
    {
        eprintln!(
            "[SG_DEBUG][MOV] audio_pts.contiguous path={} anchors_over_tolerance={} max_drift_frames={} max_drift_ms={}",
            path.display(),
            discontinuity_count,
            max_abs_drift_frames,
            (max_abs_drift_frames as u64).saturating_mul(1000) / sample_rate as u64
        );
    }

    if samples.is_empty() {
        return Ok(None);
    }
    let frames_len = samples.len() as u64 / channels as u64;
    let duration_ms = Some(
        ((frames_len as f64) * 1000.0 / sample_rate as f64).round() as u64,
    );
    Ok(Some(MovieAudio {
        samples: Arc::new(samples),
        mpeg_stream: None,
        wmv_stream: None,
        channels,
        sample_rate,
        start_ms: 0,
        duration_ms,
    }))
}

fn convert_movie_audio_chunk(
    samples: &[f32],
    src_channels: u16,
    src_sample_rate: u32,
    dst_channels: u16,
    dst_sample_rate: u32,
) -> Vec<i16> {
    let src_channels_usize = src_channels as usize;
    let dst_channels_usize = dst_channels as usize;
    if src_channels_usize == 0
        || dst_channels_usize == 0
        || src_sample_rate == 0
        || dst_sample_rate == 0
    {
        return Vec::new();
    }
    let src_frames = samples.len() / src_channels_usize;
    if src_frames == 0 {
        return Vec::new();
    }

    if src_channels == dst_channels && src_sample_rate == dst_sample_rate {
        return samples.iter().copied().map(f32_to_i16_sample).collect();
    }

    let dst_frames = ((src_frames as u128) * (dst_sample_rate as u128)
        / (src_sample_rate as u128))
        .max(1) as usize;
    let mut out = Vec::with_capacity(dst_frames.saturating_mul(dst_channels_usize));

    for dst_frame in 0..dst_frames {
        let src_pos_num = (dst_frame as u128) * (src_sample_rate as u128);
        let src_index = (src_pos_num / dst_sample_rate as u128) as usize;
        let src_index = src_index.min(src_frames - 1);
        let next_index = src_index.saturating_add(1).min(src_frames - 1);
        let frac_num = (src_pos_num % dst_sample_rate as u128) as f32;
        let frac = frac_num / dst_sample_rate as f32;

        for dst_channel in 0..dst_channels_usize {
            let sample_at = |frame: usize| -> f32 {
                if dst_channels_usize == 1 && src_channels_usize > 1 {
                    let base = frame * src_channels_usize;
                    let sum: f32 = samples[base..base + src_channels_usize]
                        .iter()
                        .copied()
                        .sum();
                    sum / src_channels_usize as f32
                } else if src_channels_usize == 1 {
                    samples[frame]
                } else {
                    let src_channel = dst_channel.min(src_channels_usize - 1);
                    samples[frame * src_channels_usize + src_channel]
                }
            };
            let a = sample_at(src_index);
            let b = sample_at(next_index);
            out.push(f32_to_i16_sample(a + (b - a) * frac));
        }
    }
    out
}

fn decode_wmv_asset(path: &Path) -> Result<MovieAsset> {
    let info = probe_wmv_movie_info(path)?;
    let frame = decode_wmv_preview_frame(path)?;
    Ok(MovieAsset {
        info: MovieInfo {
            width: info.width.or(Some(frame.width)),
            height: info.height.or(Some(frame.height)),
            decoded_frames: Some(1),
            ..info
        },
        frames: vec![frame],
        audio: None,
    })
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
        mpeg_stream: None,
        wmv_stream: None,
        channels: channels_u16,
        sample_rate: sample_rate_u32,
        start_ms: 0,
        duration_ms,
    }))
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

    let (uv_w, uv_h, y_plane_len, u_plane_len, _v_plane_len) =
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
            // The decoded chroma planes are subsampled for 4:2:0 and
            // 4:2:2. Nearest-neighbour duplication makes each chroma
            // sample visible as a 2x2 or 2x1 square. Precompute the
            // centre-aligned resampling coordinates once per frame, then
            // bilinearly reconstruct Cb and Cr at each luma pixel centre.
            let chroma_x: Vec<_> = (0..w)
                .map(|x| centred_resample_coordinate(x, uv_w, w))
                .collect();
            let chroma_y: Vec<_> = (0..dh)
                .map(|y| centred_resample_coordinate(y, uv_h, vh))
                .collect();

            for y in 0..dh {
                let y_row = y * w;
                let y_coord = chroma_y[y];
                for x in 0..w {
                    let y_idx = y_row + x;
                    let yv = data.get(y_idx).copied().unwrap_or(0) as f32;
                    let x_coord = chroma_x[x];
                    let u = get_bilinear_plane_sample(
                        data, u_off, uv_w, x_coord, y_coord, 128,
                    ) as f32
                        - 128.0;
                    let v = get_bilinear_plane_sample(
                        data, v_off, uv_w, x_coord, y_coord, 128,
                    ) as f32
                        - 128.0;

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

fn get_bilinear_plane_sample(
    data: &[u8],
    plane_off: usize,
    plane_width: usize,
    x: (usize, usize, u32),
    y: (usize, usize, u32),
    default: u8,
) -> u8 {
    let (x0, x1, fx) = x;
    let (y0, y1, fy) = y;
    let p00 = u64::from(get_plane_sample(
        data, plane_off, plane_width, x0, y0, default,
    ));
    let p10 = u64::from(get_plane_sample(
        data, plane_off, plane_width, x1, y0, default,
    ));
    let p01 = u64::from(get_plane_sample(
        data, plane_off, plane_width, x0, y1, default,
    ));
    let p11 = u64::from(get_plane_sample(
        data, plane_off, plane_width, x1, y1, default,
    ));

    const ONE: u64 = 1 << 16;
    let fx = u64::from(fx);
    let fy = u64::from(fy);
    let top = p00 * (ONE - fx) + p10 * fx;
    let bottom = p01 * (ONE - fx) + p11 * fx;
    ((top * (ONE - fy) + bottom * fy + (1 << 31)) >> 32) as u8
}

fn centred_resample_coordinate(
    output_index: usize,
    input_len: usize,
    output_len: usize,
) -> (usize, usize, u32) {
    if input_len <= 1 || output_len <= 1 {
        return (0, 0, 0);
    }

    // Map pixel centres instead of aligning sample corners. For a 2:1
    // subsampled plane this maps the first luma pixel to -0.25 chroma pixels
    // (clamped at the edge), and then advances by 0.5 per output pixel.
    let numerator = (2_i128 * output_index as i128 + 1)
        * input_len as i128
        * (1_i128 << 15);
    let mut position = numerator / output_len as i128 - (1_i128 << 15);
    let max_position = ((input_len - 1) as i128) << 16;
    position = position.clamp(0, max_position);

    let i0 = (position >> 16) as usize;
    let i1 = (i0 + 1).min(input_len - 1);
    let fraction = (position & 0xFFFF) as u32;
    (i0, i1, fraction)
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


#[cfg(test)]
mod mpeg_video_pts_tests {
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicUsize;
    use std::sync::{mpsc, Arc};

    use super::{
        mpeg_stream_needs_restart, select_mpeg_stream_frame, Mpeg2DecodedFrame, Mpeg2StreamEvent,
        Mpeg2StreamState, RgbaImage,
    };

    fn frame(index: usize, pts_90k: i64) -> Mpeg2DecodedFrame {
        Mpeg2DecodedFrame {
            frame_idx: index,
            pts_90k: Some(pts_90k),
            frame: Arc::new(RgbaImage {
                width: 1,
                height: 1,
                center_x: 0,
                center_y: 0,
                rgba: vec![0, 0, 0, 255],
            }),
        }
    }

    fn state(frames: VecDeque<Mpeg2DecodedFrame>, timer_ms: u64) -> Mpeg2StreamState {
        let (_tx, rx) = mpsc::channel::<Result<Mpeg2StreamEvent, String>>();
        Mpeg2StreamState {
            rx,
            frames,
            width: Some(1),
            height: Some(1),
            fps: Some(30.0),
            decoded_frames: 3,
            first_video_pts_90k: Some(0),
            last_video_timeline_ms: Some(300),
            last_requested_timer_ms: timer_ms,
            done: false,
            audio: None,
            decoded_any_this_poll: false,
            request_frames: Arc::new(AtomicUsize::new(0)),
        }
    }

    #[test]
    fn lagging_decoder_is_not_restarted_during_continuous_playback() {
        // A seek into a file with no later sequence header starts decoding at
        // zero. Let it catch up, even if it is over five seconds behind audio.
        let mut state = state(VecDeque::from([frame(30, 90_000)]), 60_000);
        for timer in (60_016..80_000).step_by(16) {
            assert!(!mpeg_stream_needs_restart(&state, timer));
            state.last_requested_timer_ms = timer;
        }
    }

    #[test]
    fn future_pts_does_not_turn_continuous_playback_into_a_rewind() {
        let state = state(VecDeque::from([frame(1806, 5_418_000)]), 60_000);
        assert!(!mpeg_stream_needs_restart(&state, 60_016));
    }

    #[test]
    fn explicit_seek_outside_cache_restarts_once() {
        let mut state = state(VecDeque::from([frame(1800, 5_400_000)]), 60_000);
        assert!(mpeg_stream_needs_restart(&state, 90_000));
        state.last_requested_timer_ms = 90_000;
        assert!(!mpeg_stream_needs_restart(&state, 90_016));
        assert!(mpeg_stream_needs_restart(&state, 30_000));
    }

    #[test]
    fn rewind_while_seek_is_still_loading_restarts() {
        let state = state(VecDeque::new(), 60_000);
        assert!(mpeg_stream_needs_restart(&state, 0));
    }

    #[test]
    fn rewind_inside_cache_does_not_restart_decoder() {
        let state = state(VecDeque::from([frame(0, 0), frame(9, 27_000)]), 200);
        assert!(!mpeg_stream_needs_restart(&state, 100));
    }

    #[test]
    fn frame_selection_uses_pts_instead_of_fixed_fps_index() {
        let state = state(
            VecDeque::from([frame(0, 0), frame(1, 9_000), frame(2, 27_000)]),
            0,
        );

        // Fixed 30 fps arithmetic would request index 6 at 200 ms.  PTS says
        // frame 1 is still the last frame whose presentation time has passed.
        let selected = select_mpeg_stream_frame(&state.frames, &state, 200, Some(6))
            .expect("selected frame");
        assert_eq!(selected.frame_idx, 1);
    }
}

#[cfg(test)]
mod mpeg_audio_timeline_tests {
    use std::path::Path;
    use std::sync::atomic::AtomicBool;

    use super::build_mpeg2_audio_timeline;

    #[test]
    fn pes_pts_discontinuities_do_not_splice_continuous_pcm() {
        let frame_samples = 1152usize * 2;
        let chunks = vec![
            na_mpeg2_decoder::MpegAudioF32 {
                pts_ms: 0,
                sample_rate: 44_100,
                channels: 2,
                samples: vec![0.25; frame_samples],
            },
            na_mpeg2_decoder::MpegAudioF32 {
                pts_ms: 26,
                sample_rate: 44_100,
                channels: 2,
                samples: vec![0.5; frame_samples],
            },
            // A bad/sparse PES anchor must not manufacture silence or drop PCM.
            na_mpeg2_decoder::MpegAudioF32 {
                pts_ms: 1_000,
                sample_rate: 44_100,
                channels: 2,
                samples: vec![0.75; frame_samples],
            },
        ];
        let cancel = AtomicBool::new(false);
        let audio = build_mpeg2_audio_timeline(Path::new("synthetic.mpg"), chunks, Some(0), &cancel)
            .expect("timeline build")
            .expect("audio track");

        assert_eq!(audio.channels, 2);
        assert_eq!(audio.sample_rate, 44_100);
        assert_eq!(audio.samples.len(), frame_samples * 3);
    }
}

#[cfg(test)]
mod chroma_resampling_tests {
    use super::{centred_resample_coordinate, get_bilinear_plane_sample};

    #[test]
    fn half_rate_coordinates_are_centre_aligned() {
        assert_eq!(centred_resample_coordinate(0, 2, 4), (0, 1, 0));
        assert_eq!(centred_resample_coordinate(1, 2, 4), (0, 1, 16_384));
        assert_eq!(centred_resample_coordinate(2, 2, 4), (0, 1, 49_152));
        assert_eq!(centred_resample_coordinate(3, 2, 4), (1, 1, 0));
    }

    #[test]
    fn bilinear_sample_blends_both_axes() {
        let plane = [0_u8, 100, 200, 255];
        let half = (0, 1, 32_768);
        assert_eq!(
            get_bilinear_plane_sample(&plane, 0, 2, half, half, 128),
            139
        );
    }
}

#[cfg(test)]
mod omv_loop_rewind_tests {
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicUsize;
    use std::sync::{mpsc, Arc};

    use super::{
        drain_omv_stream_state, omv_frame_for_time, select_omv_loop_frame,
        select_stream_frame, OmvStreamEvent, OmvStreamState, RgbaImage,
    };

    fn image(value: u8) -> Arc<RgbaImage> {
        Arc::new(RgbaImage {
            width: 1,
            height: 1,
            center_x: 0,
            center_y: 0,
            rgba: vec![value, value, value, 255],
        })
    }

    fn state() -> OmvStreamState {
        let (_tx, rx) = mpsc::channel::<Result<OmvStreamEvent, String>>();
        OmvStreamState {
            rx,
            frames: VecDeque::from([(100, image(100)), (101, image(101))]),
            loop_head_frames: VecDeque::from([(0, image(0)), (1, image(1)), (2, image(2))]),
            loop_head_bytes: 12,
            width: Some(1),
            height: Some(1),
            fps: Some(30.0),
            frame_time_ms: Some(1000.0 / 30.0),
            total_frames_hint: Some(102),
            total_ms_hint: Some(3_400),
            frame_times: None,
            decoded_frames: 102,
            done: true,
            request_frame: Arc::new(AtomicUsize::new(0)),
            last_served_frame_idx: 100,
            last_effective_timer_ms: 0,
            held_frame: None,
        }
    }

    #[test]
    fn loop_wrap_selects_cached_head_instead_of_stale_tail() {
        let state = state();
        let (idx, frame) = select_omv_loop_frame(&state, 0).expect("loop frame");
        assert_eq!(idx, 0);
        assert_eq!(frame.rgba[0], 0);
    }

    #[test]
    fn request_before_live_queue_returns_its_front_not_its_tail() {
        let frames = VecDeque::from([(10, image(10)), (11, image(11))]);
        let (idx, _) = select_stream_frame(&frames, 0).expect("front frame");
        assert_eq!(idx, 10);
    }

    #[test]
    fn done_does_not_hide_the_queued_rewind_reset() {
        let (tx, rx) = mpsc::channel::<Result<OmvStreamEvent, String>>();
        let mut state = state();
        state.rx = rx;
        tx.send(Ok(OmvStreamEvent::Done)).expect("queue done");
        tx.send(Ok(OmvStreamEvent::Reset { frame_idx: 0 }))
            .expect("queue reset");
        tx.send(Ok(OmvStreamEvent::Video {
            frame_idx: 0,
            frame: image(0),
        }))
        .expect("queue video");

        drain_omv_stream_state(
            std::path::Path::new("synthetic.omv"),
            &mut state,
            Some(0),
            false,
            true,
        )
        .expect("drain rewind events");

        assert!(!state.done);
        assert_eq!(state.frames.front().map(|(idx, _)| *idx), Some(0));
    }

    #[test]
    fn packet_time_boundaries_match_original_lookup() {
        let times = [(0, 32), (33, 65), (66, 99)];
        assert_eq!(omv_frame_for_time(&times, 0), Some(0));
        assert_eq!(omv_frame_for_time(&times, 32), Some(0));
        assert_eq!(omv_frame_for_time(&times, 33), Some(1));
        assert_eq!(omv_frame_for_time(&times, 99), Some(2));
    }
}

#[cfg(test)]
mod wmv_movie_adapter_tests {
    use std::collections::VecDeque;
    use std::sync::Arc;

    use super::{
        is_wmv_extension, select_wmv_stream_frame, wmv_audio_alignment_frames,
        wmv_frame_duration_ms, wmv_rebase_duration_ms, wmv_yuv_frame_to_rgba,
        RgbaImage, WmvDecodedFrame,
    };

    fn frame(index: usize, pts_ms: u64) -> WmvDecodedFrame {
        WmvDecodedFrame {
            frame_idx: index,
            source_pts_ms: pts_ms,
            frame: Arc::new(RgbaImage {
                width: 1,
                height: 1,
                center_x: 0,
                center_y: 0,
                rgba: vec![0, 0, 0, 255],
            }),
        }
    }

    #[test]
    fn source_pts_are_rebased_to_movie_time_zero() {
        let origin = 3_000u64;
        assert_eq!(3_000u64.saturating_sub(origin), 0);
        assert_eq!(3_040u64.saturating_sub(origin), 40);
        assert_eq!(wmv_rebase_duration_ms(63_000, origin), Some(60_000));
    }

    #[test]
    fn audio_alignment_uses_video_origin() {
        assert_eq!(wmv_audio_alignment_frames(3_000, 3_000, 48_000), (0, 0));
        assert_eq!(wmv_audio_alignment_frames(3_100, 3_000, 48_000), (4_800, 0));
        assert_eq!(wmv_audio_alignment_frames(2_900, 3_000, 48_000), (0, 4_800));
    }

    #[test]
    fn recognizes_siglus_wmv_extension() {
        assert!(is_wmv_extension("wmv"));
        assert!(!is_wmv_extension("asf"));
        assert!(!is_wmv_extension("mpg"));
    }

    #[test]
    fn frame_selection_follows_asf_pts() {
        let frames = VecDeque::from([frame(0, 3_000), frame(1, 3_040), frame(2, 3_120)]);
        let selected = select_wmv_stream_frame(&frames, 3_080).expect("selected frame");
        assert_eq!(selected.frame_idx, 1);
    }

    #[test]
    fn frame_selection_never_presents_future_pts() {
        let frames = VecDeque::from([frame(7, 3_040), frame(8, 3_080)]);
        assert!(select_wmv_stream_frame(&frames, 3_020).is_none());
        assert_eq!(
            select_wmv_stream_frame(&frames, 3_040).map(|frame| frame.frame_idx),
            Some(7)
        );
    }

    #[test]
    fn b_picture_decode_order_does_not_choose_clock_origin() {
        let mut frames = VecDeque::new();
        for decoded in [frame(0, 3_000), frame(1, 3_120), frame(2, 3_040), frame(3, 3_080)] {
            let pos = frames
                .iter()
                .position(|queued: &WmvDecodedFrame| queued.source_pts_ms > decoded.source_pts_ms)
                .unwrap_or(frames.len());
            frames.insert(pos, decoded);
        }
        assert_eq!(
            frames.iter().map(|f| f.source_pts_ms).collect::<Vec<_>>(),
            vec![3_000, 3_040, 3_080, 3_120]
        );
        let origin = frames.front().expect("first presentation frame").source_pts_ms;
        assert_eq!(origin, 3_000);
        assert_eq!(
            select_wmv_stream_frame(&frames, origin + 80).map(|frame| frame.frame_idx),
            Some(3)
        );
    }

    #[test]
    fn frame_duration_uses_median_positive_pts_delta() {
        assert_eq!(wmv_frame_duration_ms(&[0, 40, 80, 200]), Some(40));
    }

    #[test]
    fn yuv_black_converts_to_opaque_black() {
        let yuv = wmv_decoder::YuvFrame {
            width: 2,
            height: 2,
            y: vec![16; 4],
            cb: vec![128],
            cr: vec![128],
        };
        let rgba = wmv_yuv_frame_to_rgba(&yuv);
        assert_eq!(rgba.rgba, [0, 0, 0, 255].repeat(4));
    }
}
