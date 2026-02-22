use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

#[derive(Debug, Clone)]
pub struct MovieInfo {
    pub path: PathBuf,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub fps: Option<f32>,
    pub decoded_frames: Option<usize>,
}

/// Minimal movie state holder.
///
/// The original Siglus engine plays MOV via a native playback pipeline.
/// Here we provide a deterministic, offline path for MPEG2 (`.mpg`) by
/// leveraging `siglus_assets` probe/decode helpers when the ffmpeg feature is enabled.
#[derive(Debug)]
pub struct MovieManager {
    project_dir: PathBuf,
    current: Option<MovieInfo>,
}

impl MovieManager {
    pub fn new(project_dir: PathBuf) -> Self {
        Self {
            project_dir,
            current: None,
        }
    }

    pub fn current(&self) -> Option<&MovieInfo> {
        self.current.as_ref()
    }

    pub fn stop(&mut self) {
        self.current = None;
    }

    pub fn play(&mut self, file_name: &str, _wait: bool, _key_skip: bool) -> Result<MovieInfo> {
        let path = resolve_mov_path(&self.project_dir, file_name)?;

        let bytes = fs::read(&path).with_context(|| format!("read movie file: {}", path.display()))?;

        let mut width = None;
        let mut height = None;
        let mut fps = None;

        if let Some(h) = siglus_assets::mpeg2::find_sequence_header(&bytes) {
            width = Some(h.width as u32);
            height = Some(h.height as u32);
            fps = siglus_assets::mpeg2::fps_from_frame_rate_code(h.frame_rate_code);
        }

        let decoded_frames = decode_frames_if_enabled(&path)?;

        let info = MovieInfo {
            path,
            width,
            height,
            fps,
            decoded_frames,
        };

        self.current = Some(info.clone());
        Ok(info)
    }
}

fn resolve_mov_path(project_dir: &Path, file_name: &str) -> Result<PathBuf> {
    let p = Path::new(file_name);
    if p.is_absolute() || p.exists() {
        return Ok(p.to_path_buf());
    }

    // Matches `tnm_find_mov(base_dir, "mov", ...)`.
    let mov_dir = project_dir.join("mov");
    let direct = mov_dir.join(file_name);
    if direct.exists() {
        return Ok(direct);
    }

    // Try known extensions (Siglus supports wmv/mpg/avi; we only decode mpg currently).
    for ext in ["mpg", "mpeg", "wmv", "avi"] {
        let cand = mov_dir.join(format!("{}.{}", file_name, ext));
        if cand.exists() {
            return Ok(cand);
        }
    }

    Err(anyhow!("movie not found: {file_name}"))
}

fn decode_frames_if_enabled(path: &Path) -> Result<Option<usize>> {
    #[cfg(feature = "assets-mpeg2-ffmpeg")]
    {
        // Decode is expensive; we keep only metadata (count).
        let frames = siglus_assets::mpeg2::decode_mpeg2_to_rgba_frames(path, None)
            .context("mpeg2 decode")?;
        return Ok(Some(frames.len()));
    }

    #[cfg(not(feature = "assets-mpeg2-ffmpeg"))]
    {
        let _ = path;
        Ok(None)
    }
}
