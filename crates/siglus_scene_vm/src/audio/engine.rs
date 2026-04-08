use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};

use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::tween::Tween;

use super::bgm::decode_bgm_to_wav_bytes;
use super::{AudioHub, TrackKind};

fn wav_duration_ms(wav: &[u8]) -> Option<u64> {
    if wav.len() < 44 {
        return None;
    }
    if &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }
    let mut pos = 12usize;
    let mut byte_rate: Option<u32> = None;
    let mut data_size: Option<u32> = None;

    while pos + 8 <= wav.len() {
        let id = &wav[pos..pos + 4];
        let sz = u32::from_le_bytes([wav[pos + 4], wav[pos + 5], wav[pos + 6], wav[pos + 7]]) as usize;
        pos += 8;
        if pos + sz > wav.len() {
            break;
        }
        if id == b"fmt " {
            if sz >= 16 {
                let off = pos + 8;
                if off + 4 <= wav.len() {
                    byte_rate = Some(u32::from_le_bytes([
                        wav[off],
                        wav[off + 1],
                        wav[off + 2],
                        wav[off + 3],
                    ]));
                }
            }
        } else if id == b"data" {
            data_size = Some(sz as u32);
        }

        pos += sz;
        if (sz & 1) != 0 {
            pos += 1;
        }

        if byte_rate.is_some() && data_size.is_some() {
            break;
        }
    }

    let br = byte_rate?;
    if br == 0 {
        return None;
    }
    let ds = data_size? as u64;
    Some((ds * 1000) / (br as u64))
}

/// Minimal BGM runtime (one track).
///
/// This engine is bring-up quality: it aims to match the original command flow
/// (play/stop/pause/volume/loop flag) without recreating Siglus' full mixer.
pub struct BgmEngine {
    project_dir: PathBuf,
    volume_raw: u8,
    looping: bool,
    handle: Option<StaticSoundHandle>,

    /// Best-effort playback deadline for one-shot BGMs.
    until: Option<Instant>,

    current_name: Option<String>,

    start_time: Option<Instant>,
    paused_at: Option<Instant>,
    paused_total: Duration,
}

impl std::fmt::Debug for BgmEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BgmEngine")
            .field("volume_raw", &self.volume_raw)
            .field("looping", &self.looping)
            .field("current_name", &self.current_name)
            .finish()
    }
}

impl BgmEngine {
	pub fn new(project_dir: PathBuf) -> Self {
        Self {
            project_dir,
            volume_raw: 255,
            looping: true,
            handle: None,
            until: None,
            current_name: None,
            start_time: None,
            paused_at: None,
            paused_total: Duration::from_millis(0),
        }
    }

    pub fn current_name(&self) -> Option<&str> {
        self.current_name.as_deref()
    }

    pub fn is_playing(&self) -> bool {
        self.handle.is_some() && self.until.map(|t| Instant::now() < t).unwrap_or(true)
    }

    /// Returns true if this BGM instance has a finite playback deadline.
    ///
    /// Bring-up rule: only one-shot BGMs set `until`. Looping BGMs keep `until=None`.
    pub fn can_wait(&self) -> bool {
        self.until.is_some()
    }

    pub fn is_finished(&mut self) -> bool {
        if let Some(t) = self.until {
            if Instant::now() >= t {
                self.until = None;
                self.current_name = None;
                self.handle = None;
                return true;
            }
        }
        false
    }

    pub fn volume_raw(&self) -> u8 {
        self.volume_raw
    }

	pub fn set_volume_raw(&mut self, audio: &mut AudioHub, volume_raw: u8) -> Result<()> {
        self.volume_raw = volume_raw;
		audio.set_track_volume_raw(TrackKind::Bgm, volume_raw);
        Ok(())
    }

	pub fn set_volume_raw_fade(&mut self, audio: &mut AudioHub, volume_raw: u8, fade_ms: i64) -> Result<()> {
        self.volume_raw = volume_raw;
		audio.set_track_volume_raw_fade(TrackKind::Bgm, volume_raw, fade_ms);
        Ok(())
    }

    pub fn set_looping(&mut self, looping: bool) -> Result<()> {
        self.looping = looping;
        Ok(())
    }

    pub fn pause(&mut self) -> Result<()> {
        if let Some(h) = &mut self.handle {
            let _ = h.pause(Tween::default());
        }
        if self.start_time.is_some() && self.paused_at.is_none() {
            self.paused_at = Some(Instant::now());
        }
        Ok(())
    }

    pub fn resume(&mut self) -> Result<()> {
        if let Some(h) = &mut self.handle {
            let _ = h.resume(Tween::default());
        }
        if let Some(p) = self.paused_at.take() {
            self.paused_total += Instant::now().saturating_duration_since(p);
        }
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        self.current_name = None;
        self.until = None;
        self.start_time = None;
        self.paused_at = None;
        self.paused_total = Duration::from_millis(0);
        if let Some(mut h) = self.handle.take() {
            let _ = h.stop(Tween::default());
        }
        Ok(())
    }

    pub fn stop_fade(&mut self, fade_ms: i64) -> Result<()> {
        self.current_name = None;
        self.until = None;
        self.start_time = None;
        self.paused_at = None;
        self.paused_total = Duration::from_millis(0);
        if let Some(mut h) = self.handle.take() {
            let tween = if fade_ms > 0 {
                Tween::new(Duration::from_millis(fade_ms as u64))
            } else {
                Tween::default()
            };
            let _ = h.stop(tween);
        }
        Ok(())
    }

	pub fn play_name(&mut self, audio: &mut AudioHub, name: &str) -> Result<()> {
        let path = self.resolve_bgm_path(name)?;

        let decoded = decode_bgm_to_wav_bytes(&path, None)
            .with_context(|| format!("decode BGM: {}", path.display()))?;

        let duration_ms = crate::audio::sfx_engine::wav_duration_ms(&decoded.wav_bytes);

        // Stop previous BGM before starting a new one.
        let _ = self.stop();

        let mut data = StaticSoundData::from_cursor(Cursor::new(decoded.wav_bytes))
            .context("kira: decode WAV bytes")?;

        // Best-effort looping: if the Kira version supports loop settings, the user can
        // extend this later. We keep the flag for command compatibility.
        if self.looping {
            // Intentionally no-op on bring-up.
        }

		let handle = audio.play_static(TrackKind::Bgm, data)?;

        self.handle = Some(handle);
		self.set_volume_raw(audio, self.volume_raw)?;
        self.current_name = Some(name.to_string());
        self.start_time = Some(Instant::now());
        self.paused_at = None;
        self.paused_total = Duration::from_millis(0);

        // If looping is enabled, WAIT should not block forever in bring-up.
        // For one-shot, use best-effort duration from the decoded WAV.
        if self.looping {
            self.until = None;
        } else {
            self.until = duration_ms.map(|ms| Instant::now() + Duration::from_millis(ms));
        }
        Ok(())
    }

    pub fn play_pos_ms(&self) -> u64 {
        let Some(start) = self.start_time else {
            return 0;
        };
        let now = self.paused_at.unwrap_or_else(Instant::now);
        let elapsed = now.saturating_duration_since(start);
        let elapsed = elapsed.saturating_sub(self.paused_total);
        elapsed.as_millis() as u64
    }

    fn resolve_bgm_path(&self, name: &str) -> Result<PathBuf> {
        // Bring-up resolver: `<project>/bgm/<name>.<ext>`.
        let direct = Path::new(name);
        if direct.exists() {
            return Ok(direct.to_path_buf());
        }

        let bgm_dir = self.project_dir.join("bgm");
        let base = bgm_dir.join(name);

        if base.extension().is_some() {
            if base.exists() {
                return Ok(base);
            }
        }

        for ext in ["wav", "nwa", "ogg", "owp", "ovk"] {
            let p = base.with_extension(ext);
            if p.exists() {
                return Ok(p);
            }
        }

        Err(anyhow!("BGM not found: {name}"))
    }
}
