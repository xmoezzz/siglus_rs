use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};

use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::tween::Tween;

use crate::audio::bgm::{
    decode_bgm_to_wav_bytes, decode_ovk_entry_by_no_to_wav_bytes, resolve_koe_source, KoeSource,
};
use crate::audio::{AudioHub, TrackKind};

/// Best-effort WAV duration parsing for bring-up.
///
/// We use this only to implement WAIT-style commands without relying on
/// backend handle state APIs (which differ across Kira versions).
pub(crate) fn wav_duration_ms(wav: &[u8]) -> Option<u64> {
    // Minimal RIFF/WAVE parser.
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
        let sz =
            u32::from_le_bytes([wav[pos + 4], wav[pos + 5], wav[pos + 6], wav[pos + 7]]) as usize;
        pos += 8;
        if pos + sz > wav.len() {
            break;
        }
        if id == b"fmt " {
            if sz >= 16 {
                // byte_rate is at offset 8 within fmt chunk.
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

        // Chunks are word-aligned.
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

#[derive(Debug, Default)]
struct Slot {
    handle: Option<StaticSoundHandle>,
    until: Option<Instant>,
    last_name: Option<String>,
}

impl Slot {
    fn is_playing(&mut self) -> bool {
        if let Some(t) = self.until {
            if Instant::now() >= t {
                // Drop the handle reference; the backend should have ended.
                self.until = None;
                self.handle = None;
                self.last_name = None;
            }
        }
        self.until.is_some() && self.handle.is_some()
    }
}

pub struct SfxEngine {
    project_dir: PathBuf,
    sub_dir: String,
    volume_raw: u8,
    track_kind: TrackKind,
    slots: Vec<Slot>,
}

impl SfxEngine {
    pub fn new(
        project_dir: PathBuf,
        sub_dir: impl Into<String>,
        track_kind: TrackKind,
        slot_cnt: usize,
    ) -> Self {
        Self {
            project_dir,
            sub_dir: sub_dir.into(),
            volume_raw: 255,
            track_kind,
            slots: (0..slot_cnt).map(|_| Slot::default()).collect(),
        }
    }

    pub fn slot_cnt(&self) -> usize {
        self.slots.len()
    }

    pub fn volume_raw(&self) -> u8 {
        self.volume_raw
    }

    pub fn set_volume_raw(&mut self, audio: &mut AudioHub, volume_raw: u8) -> Result<()> {
        self.volume_raw = volume_raw;
        audio.set_track_volume_raw(self.track_kind, volume_raw);
        Ok(())
    }

    pub fn set_volume_raw_fade(
        &mut self,
        audio: &mut AudioHub,
        volume_raw: u8,
        fade_ms: i64,
    ) -> Result<()> {
        self.volume_raw = volume_raw;
        audio.set_track_volume_raw_fade(self.track_kind, volume_raw, fade_ms);
        Ok(())
    }

    pub fn is_playing_any(&mut self) -> bool {
        self.slots.iter_mut().any(|s| s.is_playing())
    }

    pub fn is_playing_slot(&mut self, slot: usize) -> bool {
        self.slots
            .get_mut(slot)
            .map(|s| s.is_playing())
            .unwrap_or(false)
    }

    pub fn last_name_slot(&self, slot: usize) -> Option<&str> {
        self.slots.get(slot).and_then(|s| s.last_name.as_deref())
    }

    pub fn stop_all(&mut self, fade_time_ms: Option<i64>) -> Result<()> {
        for s in &mut self.slots {
            if let Some(mut h) = s.handle.take() {
                let tween = fade_time_ms
                    .and_then(|v| {
                        if v > 0 {
                            Some(Duration::from_millis(v as u64))
                        } else {
                            None
                        }
                    })
                    .map(|duration| Tween {
                        duration,
                        ..Tween::default()
                    })
                    .unwrap_or_default();
                let _ = h.stop(tween);
            }
            s.until = None;
            s.last_name = None;
        }
        Ok(())
    }

    pub fn stop_slot(&mut self, slot: usize, fade_time_ms: Option<i64>) -> Result<()> {
        let Some(s) = self.slots.get_mut(slot) else {
            return Ok(());
        };
        if let Some(mut h) = s.handle.take() {
            let tween = fade_time_ms
                .and_then(|v| {
                    if v > 0 {
                        Some(Duration::from_millis(v as u64))
                    } else {
                        None
                    }
                })
                .map(|duration| Tween {
                    duration,
                    ..Tween::default()
                })
                .unwrap_or_default();
            let _ = h.stop(tween);
        }
        s.until = None;
        s.last_name = None;
        Ok(())
    }

    pub fn play_file_name_in_slot(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        file_name: &str,
        loop_flag: bool,
    ) -> Result<PathBuf> {
        if slot >= self.slots.len() {
            bail!("slot out of range: {slot}");
        }
        let path = self.resolve_path(file_name)?;
        let wav = self.decode_to_wav(&path)?;
        self.play_decoded_wav_in_slot(audio, slot, file_name, wav, loop_flag)?;
        Ok(path)
    }

    pub fn play_koe_no_in_slot(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        koe_no: i64,
        loop_flag: bool,
    ) -> Result<()> {
        if slot >= self.slots.len() {
            bail!("slot out of range: {slot}");
        }

        let resolved = resolve_koe_source(&self.project_dir, koe_no)?;
        let wav = match &resolved {
            KoeSource::File(path) => {
                decode_bgm_to_wav_bytes(path, None)
                    .with_context(|| format!("decode KOE file: {}", path.display()))?
                    .wav_bytes
            }
            KoeSource::OvkEntryByNo { path, entry_no } => {
                decode_ovk_entry_by_no_to_wav_bytes(path, *entry_no)
                    .with_context(|| {
                        format!("decode KOE OVK entry: {}#{entry_no}", path.display())
                    })?
                    .wav_bytes
            }
        };

        self.play_decoded_wav_in_slot(audio, slot, &format!("koe:{koe_no}"), wav, loop_flag)
    }

    fn play_decoded_wav_in_slot(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        display_name: &str,
        wav: Vec<u8>,
        loop_flag: bool,
    ) -> Result<()> {
        let dur_ms = wav_duration_ms(&wav);

        // Stop previous sound on this slot.
        let _ = self.stop_slot(slot, None);

        let s = &mut self.slots[slot];
        s.last_name = Some(display_name.to_string());
        if audio.is_enabled() {
            let data =
                StaticSoundData::from_cursor(Cursor::new(wav)).context("kira: decode WAV bytes")?;
            let handle = audio.play_static(self.track_kind, data)?;
            s.handle = Some(handle);
        } else {
            s.handle = None;
        }

        if loop_flag {
            s.until = None;
        } else if let Some(ms) = dur_ms {
            s.until = Some(Instant::now() + Duration::from_millis(ms));
        } else {
            // Unknown duration: keep a conservative 2s window to avoid indefinite waits.
            s.until = Some(Instant::now() + Duration::from_millis(2000));
        }

        self.set_volume_raw(audio, self.volume_raw)?;
        Ok(())
    }

    fn resolve_path(&self, file_name: &str) -> Result<PathBuf> {
        let direct = Path::new(file_name);
        if direct.exists() {
            return Ok(direct.to_path_buf());
        }

        let dir = self.project_dir.join(&self.sub_dir);
        let base = dir.join(file_name);

        if base.extension().is_some() && base.exists() {
            return Ok(base);
        }

        let candidates = ["wav", "nwa", "ogg", "owp", "ovk"];
        for ext in candidates {
            let p = base.with_extension(ext);
            if p.exists() {
                return Ok(p);
            }
        }

        bail!(
            "sound file not found: name={:?} (project_dir={:?}, sub_dir={:?})",
            file_name,
            self.project_dir,
            self.sub_dir
        );
    }

    fn decode_to_wav(&self, path: &Path) -> Result<Vec<u8>> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();

        match ext.as_str() {
            "wav" => fs::read(path).with_context(|| format!("read wav: {}", path.display())),
            "nwa" | "ogg" | "owp" | "ovk" => {
                let decoded = decode_bgm_to_wav_bytes(path, None)
                    .with_context(|| format!("decode audio: {}", path.display()))?;
                Ok(decoded.wav_bytes)
            }
            _ => Err(anyhow!("unsupported sound extension: {}", path.display())),
        }
    }
}

pub struct PcmEngine {
    inner: SfxEngine,
}

impl PcmEngine {
    pub fn new(project_dir: PathBuf) -> Self {
        // Original engine: TNM_PCM_PLAYER_CNT = 16.
        Self {
            inner: SfxEngine::new(project_dir, "pcm", TrackKind::Pcm, 16),
        }
    }

    pub fn play_file_name(&mut self, audio: &mut AudioHub, file_name: &str) -> Result<PathBuf> {
        self.inner
            .play_file_name_in_slot(audio, 0, file_name, false)
    }

    pub fn play_koe_no(&mut self, audio: &mut AudioHub, koe_no: i64) -> Result<()> {
        self.inner.play_koe_no_in_slot(audio, 0, koe_no, false)
    }

    pub fn play_in_slot(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        file_name: &str,
        loop_flag: bool,
    ) -> Result<PathBuf> {
        self.inner
            .play_file_name_in_slot(audio, slot, file_name, loop_flag)
    }

    pub fn play_koe_no_in_slot(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        koe_no: i64,
        loop_flag: bool,
    ) -> Result<()> {
        self.inner
            .play_koe_no_in_slot(audio, slot, koe_no, loop_flag)
    }

    pub fn play_decoded_wav_in_slot(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        display_name: &str,
        wav: Vec<u8>,
        loop_flag: bool,
    ) -> Result<()> {
        self.inner
            .play_decoded_wav_in_slot(audio, slot, display_name, wav, loop_flag)
    }

    pub fn stop(&mut self, fade_time_ms: Option<i64>) -> Result<()> {
        self.inner.stop_slot(0, fade_time_ms)
    }

    pub fn stop_slot(&mut self, slot: usize, fade_time_ms: Option<i64>) -> Result<()> {
        self.inner.stop_slot(slot, fade_time_ms)
    }

    pub fn stop_all(&mut self, fade_time_ms: Option<i64>) -> Result<()> {
        self.inner.stop_all(fade_time_ms)
    }

    pub fn is_playing_any(&mut self) -> bool {
        self.inner.is_playing_any()
    }

    pub fn is_playing_slot(&mut self, slot: usize) -> bool {
        self.inner.is_playing_slot(slot)
    }

    pub fn volume_raw(&self) -> u8 {
        self.inner.volume_raw()
    }

    pub fn set_volume_raw(&mut self, audio: &mut AudioHub, volume_raw: u8) -> Result<()> {
        self.inner.set_volume_raw(audio, volume_raw)
    }

    pub fn set_volume_raw_fade(
        &mut self,
        audio: &mut AudioHub,
        volume_raw: u8,
        fade_ms: i64,
    ) -> Result<()> {
        self.inner.set_volume_raw_fade(audio, volume_raw, fade_ms)
    }
}

pub struct SeEngine {
    inner: SfxEngine,
}

impl SeEngine {
    pub fn new(project_dir: PathBuf) -> Self {
        // Original engine: TNM_SE_PLAYER_CNT = 16.
        Self {
            inner: SfxEngine::new(project_dir, "se", TrackKind::Se, 16),
        }
    }

    pub fn play_file_name(&mut self, audio: &mut AudioHub, file_name: &str) -> Result<PathBuf> {
        self.inner
            .play_file_name_in_slot(audio, 0, file_name, false)
    }

    pub fn play_koe_no(&mut self, audio: &mut AudioHub, koe_no: i64) -> Result<()> {
        self.inner.play_koe_no_in_slot(audio, 0, koe_no, false)
    }

    pub fn play_in_slot(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        file_name: &str,
        loop_flag: bool,
    ) -> Result<PathBuf> {
        self.inner
            .play_file_name_in_slot(audio, slot, file_name, loop_flag)
    }

    pub fn play_koe_no_in_slot(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        koe_no: i64,
        loop_flag: bool,
    ) -> Result<()> {
        self.inner
            .play_koe_no_in_slot(audio, slot, koe_no, loop_flag)
    }

    pub fn play_decoded_wav_in_slot(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        display_name: &str,
        wav: Vec<u8>,
        loop_flag: bool,
    ) -> Result<()> {
        self.inner
            .play_decoded_wav_in_slot(audio, slot, display_name, wav, loop_flag)
    }

    pub fn stop(&mut self, fade_time_ms: Option<i64>) -> Result<()> {
        self.inner.stop_all(fade_time_ms)
    }

    pub fn stop_slot(&mut self, slot: usize, fade_time_ms: Option<i64>) -> Result<()> {
        self.inner.stop_slot(slot, fade_time_ms)
    }

    pub fn is_playing_any(&mut self) -> bool {
        self.inner.is_playing_any()
    }

    pub fn is_playing_slot(&mut self, slot: usize) -> bool {
        self.inner.is_playing_slot(slot)
    }

    pub fn volume_raw(&self) -> u8 {
        self.inner.volume_raw()
    }

    pub fn set_volume_raw(&mut self, audio: &mut AudioHub, volume_raw: u8) -> Result<()> {
        self.inner.set_volume_raw(audio, volume_raw)
    }

    pub fn set_volume_raw_fade(
        &mut self,
        audio: &mut AudioHub,
        volume_raw: u8,
        fade_ms: i64,
    ) -> Result<()> {
        self.inner.set_volume_raw_fade(audio, volume_raw, fade_ms)
    }

    pub fn last_name(&self) -> Option<&str> {
        self.inner.last_name_slot(0)
    }
}
