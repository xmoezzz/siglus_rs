use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use crate::env::trace_env;
use crate::platform_time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};

use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::tween::Tween;
use kira::Volume;

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

#[derive(Debug)]
struct Slot {
    handle: Option<StaticSoundHandle>,
    /// Logical end time for non-looping playback. This remains available when
    /// audio output is disabled, so WAIT/CHECK keep script-time semantics.
    until: Option<Instant>,
    duration_ms: Option<u64>,
    looping: bool,
    last_name: Option<String>,
    /// Time at which a pause transition becomes fully effective. Presence of
    /// this field is enough for CHECK to report false, matching is_pausing().
    paused_at: Option<Instant>,
    /// READY prepares a source in the paused state and starts it on RESUME.
    ready_only: bool,
    /// A STOP fade remains observable by WAIT_FADE until this deadline.
    /// CHECK/WAIT already treat the slot as stopped while the fade runs.
    fade_until: Option<Instant>,
    /// Delayed PCMCH.RESUME target time.
    resume_at: Option<Instant>,
    resume_fade_ms: i64,
    /// Script-visible channel volume (`PCMCH.SET_VOLUME`).
    volume_raw: u8,
    /// Original `m_system_volume`: category/chara/BGM-fade routing, updated
    /// independently from the script-visible channel volume.
    system_volume_raw: u8,
}

impl Default for Slot {
    fn default() -> Self {
        Self {
            handle: None,
            until: None,
            duration_ms: None,
            looping: false,
            last_name: None,
            paused_at: None,
            ready_only: false,
            fade_until: None,
            resume_at: None,
            resume_fade_ms: 0,
            volume_raw: 255,
            system_volume_raw: 255,
        }
    }
}

impl Slot {
    fn amplitude(&self) -> f64 {
        let combined = u16::from(self.volume_raw) * u16::from(self.system_volume_raw) / 255;
        f64::from(combined) / 255.0
    }

    fn tween_for_ms(ms: i64) -> Tween {
        if ms > 0 {
            Tween {
                duration: Duration::from_millis(ms as u64),
                ..Tween::default()
            }
        } else {
            Tween::default()
        }
    }

    fn clear(&mut self) {
        self.handle = None;
        self.until = None;
        self.duration_ms = None;
        self.looping = false;
        self.last_name = None;
        self.paused_at = None;
        self.ready_only = false;
        self.fade_until = None;
        self.resume_at = None;
        self.resume_fade_ms = 0;
    }

    fn has_source(&self) -> bool {
        self.last_name.is_some()
    }

    fn resume_now(&mut self, fade_ms: i64) {
        if !self.has_source() || self.fade_until.is_some() {
            self.resume_at = None;
            self.resume_fade_ms = 0;
            return;
        }

        let now = Instant::now();
        let was_ready = self.ready_only;
        if let Some(mut handle) = self.handle.take() {
            if fade_ms > 0 {
                let amplitude = self.amplitude();
                let _ = handle.set_volume(Volume::Amplitude(0.0), Tween::default());
                let _ = handle.resume(Tween::default());
                let _ = handle.set_volume(
                    Volume::Amplitude(amplitude),
                    Self::tween_for_ms(fade_ms),
                );
            } else {
                let amplitude = self.amplitude();
                let _ = handle.resume(Tween::default());
                let _ = handle.set_volume(Volume::Amplitude(amplitude), Tween::default());
            }
            self.handle = Some(handle);
        }

        if was_ready {
            if self.looping {
                self.until = None;
            } else {
                let duration_ms = self.duration_ms.unwrap_or(2000);
                self.until = Some(now + Duration::from_millis(duration_ms));
            }
        } else if let Some(paused_at) = self.paused_at {
            // A fade-pause continues until paused_at. Resuming before then must
            // not add time that the sound actually kept playing.
            let effective = if paused_at > now { now } else { paused_at };
            let paused_for = now.saturating_duration_since(effective);
            if let Some(until) = self.until {
                self.until = Some(until + paused_for);
            }
        }

        self.paused_at = None;
        self.ready_only = false;
        self.fade_until = None;
        self.resume_at = None;
        self.resume_fade_ms = 0;
    }

    fn tick(&mut self) {
        let now = Instant::now();
        if self.fade_until.is_some_and(|at| now >= at) {
            self.clear();
            return;
        }
        if self.resume_at.is_some_and(|at| now >= at) {
            self.resume_now(self.resume_fade_ms);
        }

        let fully_paused = self.paused_at.is_some_and(|at| now >= at);
        if !self.looping && !self.ready_only && !fully_paused {
            if self.until.is_some_and(|at| now >= at) {
                self.clear();
            }
        }
    }

    fn is_playing(&mut self) -> bool {
        self.tick();
        if !self.has_source()
            || self.fade_until.is_some()
            || self.ready_only
            || self.paused_at.is_some()
            || self.resume_at.is_some()
        {
            return false;
        }
        self.looping || self.until.is_some()
    }

    fn is_fading_out(&mut self) -> bool {
        self.tick();
        self.fade_until.is_some()
    }

    fn play_pos_ms(&mut self) -> u64 {
        self.tick();
        let Some(duration) = self.duration_ms else { return 0; };
        if self.ready_only || !self.has_source() {
            return 0;
        }
        let now = self.paused_at.unwrap_or_else(Instant::now);
        if let Some(until) = self.until {
            let remaining = until.saturating_duration_since(now).as_millis() as u64;
            return duration.saturating_sub(remaining.min(duration));
        }
        0
    }

    fn needs_tick(&self) -> bool {
        self.resume_at.is_some() || self.fade_until.is_some()
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

    pub fn slot_play_pos_ms(&mut self, slot: usize) -> u64 {
        self.slots.get_mut(slot).map(|s| s.play_pos_ms()).unwrap_or(0)
    }

    pub fn is_fading_slot(&mut self, slot: usize) -> bool {
        self.slots
            .get_mut(slot)
            .map(|s| s.is_fading_out())
            .unwrap_or(false)
    }

    pub fn last_name_slot(&self, slot: usize) -> Option<&str> {
        self.slots.get(slot).and_then(|s| s.last_name.as_deref())
    }

    pub fn stop_all(&mut self, fade_time_ms: Option<i64>) -> Result<()> {
        for slot in 0..self.slots.len() {
            self.stop_slot(slot, fade_time_ms)?;
        }
        Ok(())
    }

    pub fn stop_slot(&mut self, slot: usize, fade_time_ms: Option<i64>) -> Result<()> {
        let Some(s) = self.slots.get_mut(slot) else {
            return Ok(());
        };
        s.tick();
        let fade_ms = fade_time_ms.unwrap_or(0).max(0);
        if fade_ms == 0 || !s.has_source() {
            if let Some(mut handle) = s.handle.take() {
                let _ = handle.stop(Tween::default());
            }
            s.clear();
            return Ok(());
        }

        if let Some(handle) = &mut s.handle {
            let _ = handle.stop(Slot::tween_for_ms(fade_ms));
        }
        s.until = None;
        s.paused_at = None;
        s.ready_only = false;
        s.resume_at = None;
        s.resume_fade_ms = 0;
        s.fade_until = Some(Instant::now() + Duration::from_millis(fade_ms as u64));
        Ok(())
    }

    pub fn play_file_name_in_slot(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        file_name: &str,
        loop_flag: bool,
    ) -> Result<PathBuf> {
        self.play_file_name_in_slot_with_options(audio, slot, file_name, loop_flag, 0, false)
    }

    pub fn play_file_name_in_slot_with_options(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        file_name: &str,
        loop_flag: bool,
        fade_in_ms: i64,
        ready_only: bool,
    ) -> Result<PathBuf> {
        if slot >= self.slots.len() {
            bail!("slot out of range: {slot}");
        }
        let path = self.resolve_path(file_name)?;
        let wav = self.decode_to_wav(&path)?;
        self.play_decoded_wav_in_slot_with_options(
            audio,
            slot,
            file_name,
            wav,
            loop_flag,
            fade_in_ms,
            ready_only,
        )?;
        Ok(path)
    }

    pub fn play_koe_no_in_slot(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        koe_no: i64,
        loop_flag: bool,
    ) -> Result<()> {
        self.play_koe_no_in_slot_with_options(audio, slot, koe_no, loop_flag, 0, false)
    }

    pub fn play_koe_no_in_slot_with_options(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        koe_no: i64,
        loop_flag: bool,
        fade_in_ms: i64,
        ready_only: bool,
    ) -> Result<()> {
        if slot >= self.slots.len() {
            bail!("slot out of range: {slot}");
        }

        let wav = self.decode_koe_no(koe_no)?;
        self.play_decoded_wav_in_slot_with_options(
            audio,
            slot,
            &format!("koe:{koe_no}"),
            wav,
            loop_flag,
            fade_in_ms,
            ready_only,
        )
    }

    fn decode_koe_no(&self, koe_no: i64) -> Result<Vec<u8>> {
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
        if trace_env().sg_audio_trace {
            eprintln!(
                "[SG_AUDIO_TRACE] koe resolved koe_no={} source={:?} wav_ms={:?}",
                koe_no,
                resolved,
                wav_duration_ms(&wav)
            );
        }
        Ok(wav)
    }

    fn play_decoded_wav_in_slot(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        display_name: &str,
        wav: Vec<u8>,
        loop_flag: bool,
    ) -> Result<()> {
        self.play_decoded_wav_in_slot_with_options(
            audio,
            slot,
            display_name,
            wav,
            loop_flag,
            0,
            false,
        )
    }

    fn play_decoded_wav_in_slot_with_options(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        display_name: &str,
        wav: Vec<u8>,
        loop_flag: bool,
        fade_in_ms: i64,
        ready_only: bool,
    ) -> Result<()> {
        if slot >= self.slots.len() {
            bail!("slot out of range: {slot}");
        }
        let duration_ms = wav_duration_ms(&wav).or(Some(2000));

        // C_tnm_player::reinit/release_sound discards the previous slot source.
        self.stop_slot(slot, None)?;

        let mut handle = None;
        if audio.is_enabled() {
            let mut data =
                StaticSoundData::from_cursor(Cursor::new(wav)).context("kira: decode WAV bytes")?;
            if loop_flag {
                data = data.loop_region(0.0..);
            }
            let mut new_handle = audio.play_static(self.track_kind, data)?;
            let amplitude = self.slots[slot].amplitude();
            if ready_only {
                let _ = new_handle.set_volume(Volume::Amplitude(amplitude), Tween::default());
                let _ = new_handle.pause(Tween::default());
            } else if fade_in_ms > 0 {
                let _ = new_handle.set_volume(Volume::Amplitude(0.0), Tween::default());
                let _ = new_handle.set_volume(
                    Volume::Amplitude(amplitude),
                    Slot::tween_for_ms(fade_in_ms),
                );
            } else {
                let _ = new_handle.set_volume(Volume::Amplitude(amplitude), Tween::default());
            }
            handle = Some(new_handle);
        }

        let now = Instant::now();
        let s = &mut self.slots[slot];
        s.handle = handle;
        s.duration_ms = duration_ms;
        s.looping = loop_flag;
        s.last_name = Some(display_name.to_string());
        s.ready_only = ready_only;
        s.fade_until = None;
        s.paused_at = ready_only.then_some(now);
        s.resume_at = None;
        s.resume_fade_ms = 0;
        s.until = if ready_only || loop_flag {
            None
        } else {
            duration_ms.map(|ms| now + Duration::from_millis(ms))
        };

        Ok(())
    }

    pub fn slot_volume_raw(&self, slot: usize) -> u8 {
        self.slots.get(slot).map(|s| s.volume_raw).unwrap_or(255)
    }

    pub fn slot_system_volume_raw(&self, slot: usize) -> u8 {
        self.slots
            .get(slot)
            .map(|s| s.system_volume_raw)
            .unwrap_or(255)
    }

    pub fn slot_duration_ms(&self, slot: usize) -> u64 {
        self.slots
            .get(slot)
            .and_then(|s| s.duration_ms)
            .unwrap_or(0)
    }

    pub fn slot_loop_flag(&self, slot: usize) -> bool {
        self.slots.get(slot).map(|s| s.looping).unwrap_or(false)
    }

    pub fn slot_ready_flag(&self, slot: usize) -> bool {
        self.slots.get(slot).map(|s| s.ready_only).unwrap_or(false)
    }

    pub fn slot_resume_delay_ms(&self, slot: usize) -> i64 {
        self.slots
            .get(slot)
            .and_then(|s| s.resume_at)
            .map(|deadline| {
                deadline
                    .saturating_duration_since(Instant::now())
                    .as_millis()
                    .min(i64::MAX as u128) as i64
            })
            .unwrap_or(0)
    }

    pub fn set_slot_volume_raw_fade(
        &mut self,
        slot: usize,
        volume_raw: u8,
        fade_ms: i64,
    ) -> Result<()> {
        let Some(s) = self.slots.get_mut(slot) else {
            return Ok(());
        };
        s.volume_raw = volume_raw;
        let amplitude = s.amplitude();
        if let Some(handle) = &mut s.handle {
            let _ = handle.set_volume(
                Volume::Amplitude(amplitude),
                Slot::tween_for_ms(fade_ms.max(0)),
            );
        }
        Ok(())
    }

    pub fn set_slot_system_volume_raw(&mut self, slot: usize, volume_raw: u8) -> Result<()> {
        let Some(s) = self.slots.get_mut(slot) else {
            return Ok(());
        };
        if s.system_volume_raw == volume_raw {
            return Ok(());
        }
        s.system_volume_raw = volume_raw;
        let amplitude = s.amplitude();
        if let Some(handle) = &mut s.handle {
            let _ = handle.set_volume(Volume::Amplitude(amplitude), Tween::default());
        }
        Ok(())
    }

    pub fn pause_slot(&mut self, slot: usize, fade_time_ms: Option<i64>) -> Result<()> {
        let Some(s) = self.slots.get_mut(slot) else {
            return Ok(());
        };
        s.tick();
        if !s.has_source() || s.ready_only || s.paused_at.is_some() || s.resume_at.is_some() {
            return Ok(());
        }
        let fade_ms = fade_time_ms.unwrap_or(0).max(0);
        if let Some(handle) = &mut s.handle {
            let _ = handle.pause(Slot::tween_for_ms(fade_ms));
        }
        let now = Instant::now();
        s.paused_at = Some(now + Duration::from_millis(fade_ms as u64));
        s.resume_at = None;
        s.resume_fade_ms = 0;
        Ok(())
    }

    pub fn resume_slot(&mut self, slot: usize, fade_ms: i64, delay_ms: i64) -> Result<()> {
        let Some(s) = self.slots.get_mut(slot) else {
            return Ok(());
        };
        s.tick();
        if !s.has_source()
            || s.fade_until.is_some()
            || (!s.ready_only && s.paused_at.is_none())
        {
            return Ok(());
        }
        let delay_ms = delay_ms.max(0);
        if delay_ms == 0 {
            s.resume_now(fade_ms.max(0));
        } else {
            s.resume_at = Some(Instant::now() + Duration::from_millis(delay_ms as u64));
            s.resume_fade_ms = fade_ms.max(0);
        }
        Ok(())
    }

    pub fn tick(&mut self) {
        for slot in &mut self.slots {
            slot.tick();
        }
    }

    pub fn needs_tick(&self) -> bool {
        self.slots.iter().any(Slot::needs_tick)
    }

    fn resolve_path(&self, file_name: &str) -> Result<PathBuf> {
        let direct = Path::new(file_name);
        // Relative names are project-rooted (SE registered by script name);
        // probing them against the process CWD is always a miss and costs a
        // syscall on every play, so only absolute paths take this shortcut.
        if direct.is_absolute() && path_exists(direct) {
            return Ok(direct.to_path_buf());
        }

        if let Ok((path, _ty)) = crate::resource::find_audio_path_with_append_dir(
            &self.project_dir,
            "",
            &self.sub_dir,
            file_name,
        ) {
            return Ok(path);
        }

        let dir = self.project_dir.join(&self.sub_dir);
        let base = dir.join(file_name);

        if base.extension().is_some() && path_exists(&base) {
            return Ok(base);
        }

        let candidates = ["wav", "nwa", "ogg", "owp", "ovk"];
        for ext in candidates {
            let p = base.with_extension(ext);
            if path_exists(&p) {
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
            "wav" => crate::resource::read_file_bytes(path).with_context(|| format!("read wav: {}", path.display())),
            "nwa" | "ogg" | "owp" | "ovk" => {
                let decoded = decode_bgm_to_wav_bytes(path, None)
                    .with_context(|| format!("decode audio: {}", path.display()))?;
                Ok(decoded.wav_bytes)
            }
            _ => Err(anyhow!("unsupported sound extension: {}", path.display())),
        }
    }
}


fn path_exists(path: &Path) -> bool {
    crate::resource::game_file_exists(path)
}

pub struct PcmEngine {
    inner: SfxEngine,
}

impl PcmEngine {
    pub fn new(project_dir: PathBuf) -> Self {
        // The original has one independent global PCM player plus a 16-entry
        // PCMCH list. Keep them separate internally; channel N maps to N + 1.
        Self {
            inner: SfxEngine::new(project_dir, "wav", TrackKind::Pcm, 17),
        }
    }

    fn channel_slot(channel: usize) -> usize {
        channel.saturating_add(1)
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
            .play_file_name_in_slot(audio, Self::channel_slot(slot), file_name, loop_flag)
    }

    pub fn play_in_slot_with_options(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        file_name: &str,
        loop_flag: bool,
        fade_in_ms: i64,
        ready_only: bool,
    ) -> Result<PathBuf> {
        self.inner.play_file_name_in_slot_with_options(
            audio,
            Self::channel_slot(slot),
            file_name,
            loop_flag,
            fade_in_ms,
            ready_only,
        )
    }

    pub fn play_koe_no_in_slot(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        koe_no: i64,
        loop_flag: bool,
    ) -> Result<()> {
        self.inner
            .play_koe_no_in_slot(audio, Self::channel_slot(slot), koe_no, loop_flag)
    }

    pub fn play_koe_no_in_slot_with_options(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        koe_no: i64,
        loop_flag: bool,
        fade_in_ms: i64,
        ready_only: bool,
    ) -> Result<()> {
        self.inner.play_koe_no_in_slot_with_options(
            audio,
            Self::channel_slot(slot),
            koe_no,
            loop_flag,
            fade_in_ms,
            ready_only,
        )
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
            .play_decoded_wav_in_slot(audio, Self::channel_slot(slot), display_name, wav, loop_flag)
    }

    pub fn play_decoded_wav_in_slot_with_options(
        &mut self,
        audio: &mut AudioHub,
        slot: usize,
        display_name: &str,
        wav: Vec<u8>,
        loop_flag: bool,
        fade_in_ms: i64,
        ready_only: bool,
    ) -> Result<()> {
        self.inner.play_decoded_wav_in_slot_with_options(
            audio,
            Self::channel_slot(slot),
            display_name,
            wav,
            loop_flag,
            fade_in_ms,
            ready_only,
        )
    }

    pub fn stop(&mut self, fade_time_ms: Option<i64>) -> Result<()> {
        self.inner.stop_slot(0, fade_time_ms)
    }

    pub fn stop_slot(&mut self, slot: usize, fade_time_ms: Option<i64>) -> Result<()> {
        self.inner.stop_slot(Self::channel_slot(slot), fade_time_ms)
    }

    pub fn stop_all(&mut self, fade_time_ms: Option<i64>) -> Result<()> {
        self.inner.stop_all(fade_time_ms)
    }

    pub fn pause_slot(&mut self, slot: usize, fade_time_ms: Option<i64>) -> Result<()> {
        self.inner.pause_slot(Self::channel_slot(slot), fade_time_ms)
    }

    pub fn resume_slot(&mut self, slot: usize, fade_ms: i64, delay_ms: i64) -> Result<()> {
        self.inner.resume_slot(Self::channel_slot(slot), fade_ms, delay_ms)
    }

    pub fn tick(&mut self) {
        self.inner.tick();
    }

    pub fn needs_tick(&self) -> bool {
        self.inner.needs_tick()
    }

    pub fn is_playing_any(&mut self) -> bool {
        self.inner.is_playing_any()
    }

    pub fn is_playing_slot(&mut self, slot: usize) -> bool {
        self.inner.is_playing_slot(Self::channel_slot(slot))
    }

    pub fn is_fading_slot(&mut self, slot: usize) -> bool {
        self.inner.is_fading_slot(Self::channel_slot(slot))
    }

    pub fn slot_volume_raw(&self, slot: usize) -> u8 {
        self.inner.slot_volume_raw(Self::channel_slot(slot))
    }

    pub fn slot_loop_flag(&self, slot: usize) -> bool {
        self.inner.slot_loop_flag(Self::channel_slot(slot))
    }

    pub fn slot_ready_flag(&self, slot: usize) -> bool {
        self.inner.slot_ready_flag(Self::channel_slot(slot))
    }

    pub fn slot_resume_delay_ms(&self, slot: usize) -> i64 {
        self.inner.slot_resume_delay_ms(Self::channel_slot(slot))
    }

    pub fn set_slot_volume_raw_fade(
        &mut self,
        slot: usize,
        volume_raw: u8,
        fade_ms: i64,
    ) -> Result<()> {
        self.inner
            .set_slot_volume_raw_fade(Self::channel_slot(slot), volume_raw, fade_ms)
    }

    pub fn global_duration_ms(&self) -> u64 {
        self.inner.slot_duration_ms(0)
    }

    pub fn slot_duration_ms(&self, slot: usize) -> u64 {
        self.inner.slot_duration_ms(Self::channel_slot(slot))
    }

    pub fn set_global_system_volume_raw(&mut self, volume_raw: u8) -> Result<()> {
        self.inner.set_slot_system_volume_raw(0, volume_raw)
    }

    pub fn set_slot_system_volume_raw(&mut self, slot: usize, volume_raw: u8) -> Result<()> {
        self.inner
            .set_slot_system_volume_raw(Self::channel_slot(slot), volume_raw)
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

fn load_koe_mouth_volume_table(
    project_dir: &Path,
    current_append_dir: &str,
    koe_no: i64,
) -> Result<Vec<f32>> {
    let name = format!("z{koe_no:09}.vol.csv");
    let Some(path) = crate::resource::resolve_dat_file_path(
        project_dir,
        current_append_dir,
        &name,
    )? else {
        // Original play_koe accepts a missing .vol.csv and simply leaves the
        // mouth-volume table empty.
        return Ok(Vec::new());
    };

    let bytes = crate::resource::read_file_bytes(&path)
        .with_context(|| format!("read KOE mouth-volume table: {}", path.display()))?;
    if bytes.len() % 2 != 0 {
        bail!("KOE mouth-volume table has odd UTF-16 byte length: {}", path.display());
    }

    let (payload, big_endian) = if bytes.starts_with(&[0xff, 0xfe]) {
        (&bytes[2..], false)
    } else if bytes.starts_with(&[0xfe, 0xff]) {
        (&bytes[2..], true)
    } else {
        (&bytes[..], false)
    };
    let units: Vec<u16> = payload
        .chunks_exact(2)
        .map(|pair| {
            if big_endian {
                u16::from_be_bytes([pair[0], pair[1]])
            } else {
                u16::from_le_bytes([pair[0], pair[1]])
            }
        })
        .collect();
    let text = String::from_utf16(&units)
        .with_context(|| format!("decode KOE mouth-volume table as UTF-16: {}", path.display()))?;
    let first_line = text.lines().next().unwrap_or("");
    if first_line.is_empty() {
        bail!("KOE mouth-volume table is empty: {}", path.display());
    }

    first_line
        .split(',')
        .map(|part| {
            part.trim().parse::<f32>().with_context(|| {
                format!("parse KOE mouth-volume value {part:?}: {}", path.display())
            })
        })
        .collect()
}

pub struct KoeEngine {
    inner: SfxEngine,
    current_koe_no: i64,
    /// Original C_tnm_player owns the mouth table together with the currently
    /// loaded KOE. It is rebuilt on every play_koe() reinit, not cached by
    /// voice number across append-directory changes.
    mouth_volume_table: Vec<f32>,
}

impl KoeEngine {
    pub fn new(project_dir: PathBuf) -> Self {
        // Original engine: C_elm_koe owns one active voice player and stops it
        // before starting the next KOE.
        Self {
            inner: SfxEngine::new(project_dir, "wav", TrackKind::Koe, 1),
            current_koe_no: -1,
            mouth_volume_table: Vec::new(),
        }
    }

    pub fn play_koe_no(
        &mut self,
        audio: &mut AudioHub,
        koe_no: i64,
        current_append_dir: &str,
    ) -> Result<()> {
        // C_tnm_player::play_koe starts with reinit(): clear the old player
        // metadata and mouth table before resolving/loading the new voice.
        let _ = self.stop(None);
        self.current_koe_no = -1;
        self.mouth_volume_table.clear();

        if koe_no < 0 {
            return Ok(());
        }

        // Decode/prepare the KOE first. The original player loads the mouth
        // CSV only after the voice stream has been prepared, but before play().
        let wav = self.inner.decode_koe_no(koe_no)?;
        self.mouth_volume_table = load_koe_mouth_volume_table(
            &self.inner.project_dir,
            current_append_dir,
            koe_no,
        )?;
        self.inner.play_decoded_wav_in_slot_with_options(
            audio,
            0,
            &format!("koe:{koe_no}"),
            wav,
            false,
            0,
            false,
        )?;
        self.current_koe_no = koe_no;
        Ok(())
    }

    pub fn stop(&mut self, fade_time_ms: Option<i64>) -> Result<()> {
        self.inner.stop_slot(0, fade_time_ms)
    }

    pub fn is_playing_any(&mut self) -> bool {
        self.inner.is_playing_any()
    }

    pub fn current_koe_no(&self) -> i64 {
        self.current_koe_no
    }

    pub fn current_play_pos_ms(&mut self) -> u64 {
        self.inner.slot_play_pos_ms(0)
    }

    pub fn current_mouth_volume(&mut self) -> f32 {
        if !self.inner.is_playing_slot(0) {
            return 0.0;
        }
        let frame = (self.inner.slot_play_pos_ms(0).saturating_mul(60) / 1000) as usize;
        self.mouth_volume_table.get(frame).copied().unwrap_or(0.0)
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
            inner: SfxEngine::new(project_dir, "wav", TrackKind::Se, 16),
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
