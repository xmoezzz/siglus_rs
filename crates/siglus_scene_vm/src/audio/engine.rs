use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context, Result};
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::sound::{EndPosition, Region};
use kira::tween::Tween;
use kira::Volume;
use siglus_assets::gameexe::{decode_gameexe_dat_bytes, GameexeConfig, GameexeDecodeOptions};

use super::bgm::decode_bgm_to_wav_bytes;
use super::{AudioHub, TrackKind};

const TNM_BGM_START_POS_INI: i64 = -1;
const TNM_BGM_PLAYER_CNT: usize = 2;

#[derive(Debug, Clone)]
struct WavPcmRegion {
    data_offset: usize,
    data_len: usize,
    data_len_field_offset: usize,
    riff_len_field_offset: usize,
    byte_rate: u32,
    block_align: usize,
    sample_rate: u32,
}

impl WavPcmRegion {
    fn sample_count(&self) -> u64 {
        (self.data_len / self.block_align.max(1)) as u64
    }
}

fn wav_pcm_region(wav: &[u8]) -> Option<WavPcmRegion> {
    if wav.len() < 44 {
        return None;
    }
    if &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        return None;
    }

    let mut pos = 12usize;
    let mut byte_rate: Option<u32> = None;
    let mut block_align: Option<usize> = None;
    let mut data_offset: Option<usize> = None;
    let mut data_len: Option<usize> = None;
    let mut data_len_field_offset: Option<usize> = None;

    while pos + 8 <= wav.len() {
        let id = &wav[pos..pos + 4];
        let sz =
            u32::from_le_bytes([wav[pos + 4], wav[pos + 5], wav[pos + 6], wav[pos + 7]]) as usize;
        let sz_field_off = pos + 4;
        pos += 8;
        if pos + sz > wav.len() {
            break;
        }
        if id == b"fmt " {
            if sz >= 16 {
                let off = pos + 8;
                if off + 6 <= wav.len() {
                    byte_rate = Some(u32::from_le_bytes([
                        wav[off],
                        wav[off + 1],
                        wav[off + 2],
                        wav[off + 3],
                    ]));
                    block_align = Some(u16::from_le_bytes([wav[pos + 12], wav[pos + 13]]) as usize);
                }
            }
        } else if id == b"data" {
            data_offset = Some(pos);
            data_len = Some(sz);
            data_len_field_offset = Some(sz_field_off);
        }
        pos += sz;
        if (sz & 1) != 0 {
            pos += 1;
        }
    }

    let byte_rate = byte_rate?;
    let block_align = block_align?.max(1);
    let sample_rate = (byte_rate / (block_align as u32)).max(1);

    Some(WavPcmRegion {
        data_offset: data_offset?,
        data_len: data_len?,
        data_len_field_offset: data_len_field_offset?,
        riff_len_field_offset: 4,
        byte_rate,
        block_align,
        sample_rate,
    })
}

fn wav_slice_samples(wav: &[u8], start_sample: u64, end_sample: Option<u64>) -> Option<Vec<u8>> {
    let region = wav_pcm_region(wav)?;
    let total_samples = region.sample_count();
    let start_sample = start_sample.min(total_samples);
    let end_sample = end_sample
        .unwrap_or(total_samples)
        .min(total_samples)
        .max(start_sample);

    let start_byte = (start_sample as usize) * region.block_align;
    let end_byte = (end_sample as usize) * region.block_align;
    let src_begin = region.data_offset + start_byte;
    let src_end = region.data_offset + end_byte;
    if src_begin > wav.len() || src_end > wav.len() || src_begin > src_end {
        return None;
    }

    let mut out = wav.to_vec();
    out.splice(
        region.data_offset..region.data_offset + region.data_len,
        wav[src_begin..src_end].iter().copied(),
    );
    let new_data_len = (src_end - src_begin) as u32;
    out[region.data_len_field_offset..region.data_len_field_offset + 4]
        .copy_from_slice(&new_data_len.to_le_bytes());
    let riff_len = (out.len().saturating_sub(8)) as u32;
    out[region.riff_len_field_offset..region.riff_len_field_offset + 4]
        .copy_from_slice(&riff_len.to_le_bytes());
    Some(out)
}

fn parse_i64_like(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Ok(v) = s.parse::<i64>() {
        return Some(v);
    }
    if let Some(rest) = s.strip_prefix("0x") {
        return i64::from_str_radix(rest, 16).ok();
    }
    if let Some(rest) = s.strip_prefix("-0x") {
        return i64::from_str_radix(rest, 16).ok().map(|v| -v);
    }
    None
}

fn normalize_regist_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn clamp_sample_range(
    total_samples: u64,
    start_sample: i64,
    end_sample: i64,
    restart_sample: i64,
) -> (u64, u64, u64) {
    let total_samples_i64 = total_samples as i64;
    let end_sample = if end_sample < 0 {
        total_samples_i64
    } else {
        end_sample.clamp(0, total_samples_i64)
    };
    let start_sample = start_sample.clamp(0, end_sample);
    let restart_sample = restart_sample.clamp(0, end_sample);
    (
        start_sample as u64,
        end_sample as u64,
        restart_sample as u64,
    )
}

fn tween_ms(ms: i64) -> Tween {
    if ms > 0 {
        Tween {
            duration: Duration::from_millis(ms as u64),
            ..Tween::default()
        }
    } else {
        Tween::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingBgmActionKind {
    Stop,
    Pause,
}

#[derive(Debug, Clone, Copy)]
struct PendingBgmAction {
    kind: PendingBgmActionKind,
    at: Instant,
}

pub const TNM_PLAYER_STATE_FREE: i32 = 0;
pub const TNM_PLAYER_STATE_PLAY: i32 = 1;
pub const TNM_PLAYER_STATE_FADE_OUT: i32 = 2;
pub const TNM_PLAYER_STATE_PAUSE: i32 = 3;

#[derive(Debug, Clone)]
struct BgmScriptEntry {
    file_name: String,
    start_sample: i64,
    end_sample: i64,
    repeat_sample: i64,
}

#[derive(Debug, Default)]
struct BgmPlayerSlot {
    handle: Option<StaticSoundHandle>,
    source_wav: Option<Vec<u8>>,
    sample_rate_hz: u32,
    total_samples: u64,
    start_sample: u64,
    end_sample: u64,
    restart_sample: u64,
    current_segment_start_sample: u64,
    current_segment_samples: u64,
    start_time: Option<Instant>,
    paused_at: Option<Instant>,
    paused_total: Duration,
    pending: Option<PendingBgmAction>,
    fade_outing: bool,
    loop_flag: bool,
    name: Option<String>,
    file_name: Option<String>,
    ready_only: bool,
}

impl BgmPlayerSlot {
    fn reset_all(&mut self) {
        if let Some(mut h) = self.handle.take() {
            let _ = h.stop(Tween::default());
        }
        self.source_wav = None;
        self.sample_rate_hz = 0;
        self.total_samples = 0;
        self.start_sample = 0;
        self.end_sample = 0;
        self.restart_sample = 0;
        self.current_segment_start_sample = 0;
        self.current_segment_samples = 0;
        self.start_time = None;
        self.paused_at = None;
        self.paused_total = Duration::from_millis(0);
        self.pending = None;
        self.fade_outing = false;
        self.loop_flag = false;
        self.name = None;
        self.file_name = None;
        self.ready_only = false;
    }

    fn clear_runtime_only(&mut self) {
        self.handle = None;
        self.current_segment_start_sample = self.start_sample;
        self.current_segment_samples = self.end_sample.saturating_sub(self.start_sample);
        self.start_time = None;
        self.paused_at = None;
        self.paused_total = Duration::from_millis(0);
        self.pending = None;
        self.fade_outing = false;
        self.ready_only = false;
    }

    fn elapsed_ms(&self) -> u64 {
        let Some(start) = self.start_time else {
            return 0;
        };
        let now = self.paused_at.unwrap_or_else(Instant::now);
        now.saturating_duration_since(start)
            .saturating_sub(self.paused_total)
            .as_millis() as u64
    }

    fn elapsed_samples(&self) -> u64 {
        if self.sample_rate_hz == 0 {
            return 0;
        }
        self.elapsed_ms().saturating_mul(self.sample_rate_hz as u64) / 1000
    }

    fn playback_window_samples(&self) -> u64 {
        self.end_sample.saturating_sub(self.start_sample)
    }

    fn loop_span_samples(&self) -> u64 {
        self.end_sample.saturating_sub(self.restart_sample)
    }

    fn has_loop_region(&self) -> bool {
        self.loop_flag && self.restart_sample < self.end_sample
    }

    fn play_pos_samples(&self) -> u64 {
        let elapsed = self.elapsed_samples();
        if self.has_loop_region() {
            let intro_samples = self.restart_sample.saturating_sub(self.start_sample);
            let loop_span = self.loop_span_samples();
            if loop_span == 0 {
                return self.end_sample;
            }
            if self.start_sample < self.restart_sample && elapsed < intro_samples {
                return self.start_sample + elapsed.min(intro_samples);
            }
            let loop_elapsed = if self.start_sample < self.restart_sample {
                elapsed.saturating_sub(intro_samples)
            } else {
                elapsed
            };
            return self.restart_sample + (loop_elapsed % loop_span);
        }
        self.start_sample + elapsed.min(self.playback_window_samples())
    }

    fn check_state(&self) -> i32 {
        if self.handle.is_none() {
            return TNM_PLAYER_STATE_FREE;
        }
        if self.paused_at.is_some() {
            return TNM_PLAYER_STATE_PAUSE;
        }
        if self.fade_outing {
            return TNM_PLAYER_STATE_FADE_OUT;
        }
        TNM_PLAYER_STATE_PLAY
    }

    fn is_playing(&self) -> bool {
        self.handle.is_some() && !self.fade_outing && self.paused_at.is_none()
    }
}

pub struct BgmEngine {
    project_dir: PathBuf,
    current_append_dir: String,
    game_volume_raw: u8,
    system_volume_raw: u8,
    current_name: Option<String>,
    current_player_id: Option<usize>,
    players: Vec<BgmPlayerSlot>,
    retired: Vec<(StaticSoundHandle, Instant)>,
    delay_deadline: Option<Instant>,
    delayed_fade_in_ms: i64,
    loop_flag: bool,
    pause_flag: bool,
}

impl std::fmt::Debug for BgmEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BgmEngine")
            .field("game_volume_raw", &self.game_volume_raw)
            .field("system_volume_raw", &self.system_volume_raw)
            .field("current_name", &self.current_name)
            .field("current_player_id", &self.current_player_id)
            .field("loop_flag", &self.loop_flag)
            .field("pause_flag", &self.pause_flag)
            .finish()
    }
}

impl BgmEngine {
    pub fn new(project_dir: PathBuf) -> Self {
        Self {
            project_dir,
            current_append_dir: String::new(),
            game_volume_raw: 255,
            system_volume_raw: 255,
            current_name: None,
            current_player_id: None,
            players: (0..TNM_BGM_PLAYER_CNT)
                .map(|_| BgmPlayerSlot::default())
                .collect(),
            retired: Vec::new(),
            delay_deadline: None,
            delayed_fade_in_ms: 0,
            loop_flag: false,
            pause_flag: false,
        }
    }

    pub fn current_name(&self) -> Option<&str> {
        self.current_name.as_deref()
    }

    pub fn set_current_append_dir(&mut self, append_dir: impl Into<String>) {
        self.current_append_dir = append_dir.into();
    }

    pub fn volume_raw(&self) -> u8 {
        self.game_volume_raw
    }

    fn total_gain_amplitude(&self) -> f64 {
        (self.game_volume_raw as f64 / 255.0) * (self.system_volume_raw as f64 / 255.0)
    }

    fn apply_all_active_volumes(&mut self, fade_ms: i64) {
        let amp = self.total_gain_amplitude();
        let tween = tween_ms(fade_ms);
        for slot in &mut self.players {
            if let Some(h) = &mut slot.handle {
                let _ = h.set_volume(Volume::Amplitude(amp), tween);
            }
        }
    }

    pub fn set_volume_raw(&mut self, _audio: &mut AudioHub, volume_raw: u8) -> Result<()> {
        self.game_volume_raw = volume_raw;
        self.apply_all_active_volumes(0);
        Ok(())
    }

    pub fn set_volume_raw_fade(
        &mut self,
        _audio: &mut AudioHub,
        volume_raw: u8,
        fade_ms: i64,
    ) -> Result<()> {
        self.game_volume_raw = volume_raw;
        self.apply_all_active_volumes(fade_ms);
        Ok(())
    }

    pub fn set_system_volume_raw(&mut self, volume_raw: u8) {
        self.system_volume_raw = volume_raw;
        self.apply_all_active_volumes(0);
    }

    pub fn set_looping(&mut self, looping: bool) -> Result<()> {
        self.loop_flag = looping;
        Ok(())
    }

    pub fn check_state(&self) -> i32 {
        self.current_slot()
            .map(|slot| slot.check_state())
            .unwrap_or(TNM_PLAYER_STATE_FREE)
    }

    pub fn is_playing(&self) -> bool {
        self.current_slot()
            .map(|slot| slot.is_playing())
            .unwrap_or(false)
    }

    pub fn is_fade_out_doing(&self) -> bool {
        self.check_state() == TNM_PLAYER_STATE_FADE_OUT
    }

    pub fn can_wait(&self) -> bool {
        self.is_playing() && !self.loop_flag
    }

    fn current_slot(&self) -> Option<&BgmPlayerSlot> {
        self.current_player_id.and_then(|id| self.players.get(id))
    }

    fn current_slot_mut(&mut self) -> Option<&mut BgmPlayerSlot> {
        self.current_player_id
            .and_then(|id| self.players.get_mut(id))
    }

    fn lookup_gameexe_bgm_entry(&self, regist_name: &str) -> Option<BgmScriptEntry> {
        let cfg = load_gameexe_config(&self.project_dir)?;
        let target = normalize_regist_name(regist_name);
        let cnt = cfg.indexed_count("BGM");
        for i in 0..cnt {
            let Some(key_name) = cfg.get_indexed_item_unquoted("BGM", i, 0) else {
                continue;
            };
            if normalize_regist_name(key_name) != target {
                continue;
            }
            let file_name = cfg
                .get_indexed_item_unquoted("BGM", i, 1)?
                .trim()
                .to_string();
            if file_name.is_empty() {
                continue;
            }
            let start_sample = cfg
                .get_indexed_item_unquoted("BGM", i, 2)
                .and_then(parse_i64_like)
                .unwrap_or(0);
            let end_sample = cfg
                .get_indexed_item_unquoted("BGM", i, 3)
                .and_then(parse_i64_like)
                .unwrap_or(-1);
            let repeat_sample = cfg
                .get_indexed_item_unquoted("BGM", i, 4)
                .and_then(parse_i64_like)
                .unwrap_or(0);
            return Some(BgmScriptEntry {
                file_name,
                start_sample,
                end_sample,
                repeat_sample,
            });
        }
        None
    }

    fn resolve_bgm_script(&self, regist_name: &str) -> Result<(BgmScriptEntry, PathBuf)> {
        if let Some(entry) = self.lookup_gameexe_bgm_entry(regist_name) {
            let (path, _ty) = crate::resource::find_audio_path_with_append_dir(
                &self.project_dir,
                &self.current_append_dir,
                "bgm",
                &entry.file_name,
            )
            .map_err(|_| {
                anyhow!(
                    "BGM file not found for regist name {}: {}",
                    regist_name,
                    entry.file_name
                )
            })?;
            return Ok((entry, path));
        }

        let direct_name = regist_name.trim();
        let (path, _ty) = crate::resource::find_audio_path_with_append_dir(
            &self.project_dir,
            &self.current_append_dir,
            "bgm",
            direct_name,
        )
        .map_err(|_| anyhow!("BGM regist name not found in script table: {regist_name}"))?;
        Ok((
            BgmScriptEntry {
                file_name: direct_name.to_string(),
                start_sample: 0,
                end_sample: -1,
                repeat_sample: 0,
            },
            path,
        ))
    }

    fn prepare_slot(
        &mut self,
        slot_id: usize,
        regist_name: &str,
        loop_flag: bool,
        start_pos_sample: i64,
        ready_only: bool,
    ) -> Result<()> {
        let (script_entry, path) = self.resolve_bgm_script(regist_name)?;
        let decoded = decode_bgm_to_wav_bytes(&path, None)
            .with_context(|| format!("decode BGM: {}", path.display()))?;
        let region = wav_pcm_region(&decoded.wav_bytes)
            .ok_or_else(|| anyhow!("decoded BGM is not PCM WAV: {}", path.display()))?;
        let total_samples = region.sample_count();

        let script_start = script_entry.start_sample;
        let script_end = script_entry.end_sample;
        let script_repeat = script_entry.repeat_sample;
        let effective_start = if start_pos_sample == TNM_BGM_START_POS_INI {
            script_start
        } else {
            start_pos_sample
        };
        let (start_sample, end_sample, restart_sample) =
            clamp_sample_range(total_samples, effective_start, script_end, script_repeat);

        let slot = &mut self.players[slot_id];
        slot.reset_all();
        slot.source_wav = Some(decoded.wav_bytes);
        slot.sample_rate_hz = region.sample_rate;
        slot.total_samples = total_samples;
        slot.start_sample = start_sample;
        slot.end_sample = end_sample;
        slot.restart_sample = restart_sample;
        slot.current_segment_start_sample = start_sample;
        slot.current_segment_samples = end_sample.saturating_sub(start_sample);
        slot.loop_flag = loop_flag;
        slot.name = Some(regist_name.to_string());
        slot.file_name = Some(path.to_string_lossy().to_string());
        slot.ready_only = ready_only;
        Ok(())
    }

    fn start_slot_internal(
        &mut self,
        audio: &mut AudioHub,
        slot_id: usize,
        fade_in_ms: i64,
        start_paused: bool,
    ) -> Result<()> {
        let amp = self.total_gain_amplitude();
        let slot = &mut self.players[slot_id];
        let Some(source) = slot.source_wav.as_ref() else {
            return Err(anyhow!("BGM slot not prepared"));
        };

        let mut effective_start = slot.start_sample;
        if effective_start >= slot.end_sample {
            if slot.has_loop_region() {
                effective_start = slot.restart_sample;
            } else {
                return Err(anyhow!("invalid BGM range: start >= end"));
            }
        }
        if effective_start >= slot.end_sample {
            return Err(anyhow!("invalid BGM range after restart clamp"));
        }

        if !audio.is_enabled() {
            slot.handle = None;
            slot.current_segment_start_sample = effective_start;
            slot.current_segment_samples = slot.end_sample.saturating_sub(effective_start);
            slot.start_time = Some(Instant::now());
            slot.paused_at = if start_paused { slot.start_time } else { None };
            slot.paused_total = Duration::from_millis(0);
            slot.pending = None;
            slot.fade_outing = false;
            slot.start_sample = effective_start;
            slot.ready_only = start_paused;
            return Ok(());
        }

        let Some(playback_wav) = wav_slice_samples(source, 0, Some(slot.end_sample)) else {
            return Err(anyhow!("failed to build BGM playback WAV"));
        };
        let start_sec = if slot.sample_rate_hz == 0 {
            0.0
        } else {
            effective_start as f64 / slot.sample_rate_hz as f64
        };
        let mut data = StaticSoundData::from_cursor(Cursor::new(playback_wav))
            .context("kira: decode WAV bytes")?;
        data = data.start_position(start_sec);
        if slot.has_loop_region() {
            let loop_region = Region {
                start: (slot.restart_sample as f64 / slot.sample_rate_hz.max(1) as f64).into(),
                end: EndPosition::Custom(
                    (slot.end_sample as f64 / slot.sample_rate_hz.max(1) as f64).into(),
                ),
            };
            data = data.loop_region(loop_region);
        }
        let mut handle = audio.play_static(TrackKind::Bgm, data)?;

        if start_paused {
            let _ = handle.set_volume(Volume::Amplitude(amp), Tween::default());
            let _ = handle.pause(Tween::default());
        } else if fade_in_ms > 0 {
            let _ = handle.set_volume(Volume::Amplitude(0.0), Tween::default());
            let _ = handle.set_volume(Volume::Amplitude(amp), tween_ms(fade_in_ms));
        } else {
            let _ = handle.set_volume(Volume::Amplitude(amp), Tween::default());
        }

        slot.handle = Some(handle);
        slot.current_segment_start_sample = effective_start;
        slot.current_segment_samples = slot.end_sample.saturating_sub(effective_start);
        slot.start_time = Some(Instant::now());
        slot.paused_at = if start_paused { slot.start_time } else { None };
        slot.paused_total = Duration::from_millis(0);
        slot.pending = None;
        slot.fade_outing = false;
        slot.start_sample = effective_start;
        slot.ready_only = start_paused;
        Ok(())
    }

    fn start_slot(&mut self, audio: &mut AudioHub, slot_id: usize, fade_in_ms: i64) -> Result<()> {
        self.start_slot_internal(audio, slot_id, fade_in_ms, false)
    }

    fn ready_slot(&mut self, audio: &mut AudioHub, slot_id: usize) -> Result<()> {
        self.start_slot_internal(audio, slot_id, 0, true)
    }

    fn handoff_current_to_retired(&mut self, fade_out_ms: i64) {
        let Some(cur_id) = self.current_player_id else {
            return;
        };
        let slot = &mut self.players[cur_id];
        if let Some(mut h) = slot.handle.take() {
            if fade_out_ms > 0 {
                let _ = h.stop(tween_ms(fade_out_ms));
                self.retired.push((
                    h,
                    Instant::now() + Duration::from_millis(fade_out_ms as u64),
                ));
            } else {
                let _ = h.stop(Tween::default());
            }
        }
        slot.clear_runtime_only();
    }

    pub fn play_name_script(
        &mut self,
        audio: &mut AudioHub,
        name: &str,
        loop_flag: bool,
        fade_in_ms: i64,
        fade_out_ms: i64,
        start_pos_sample: i64,
        ready_only: bool,
        delay_time_ms: i64,
    ) -> Result<()> {
        let regist_name = normalize_regist_name(name);
        if self.current_name.as_deref() == Some(regist_name.as_str()) && self.loop_flag && loop_flag
        {
            return Ok(());
        }

        self.handoff_current_to_retired(fade_out_ms);
        let next_id = match self.current_player_id {
            Some(id) => (id + 1) % TNM_BGM_PLAYER_CNT,
            None => 0,
        };
        let total_ready_only = ready_only || delay_time_ms > 0;
        self.prepare_slot(
            next_id,
            &regist_name,
            loop_flag,
            start_pos_sample,
            total_ready_only,
        )?;
        self.current_player_id = Some(next_id);
        self.current_name = Some(regist_name);
        self.loop_flag = loop_flag;
        self.pause_flag = ready_only;
        self.delayed_fade_in_ms = fade_in_ms;
        self.delay_deadline = if delay_time_ms > 0 {
            Some(Instant::now() + Duration::from_millis(delay_time_ms.max(0) as u64))
        } else {
            None
        };

        if total_ready_only {
            self.ready_slot(audio, next_id)?;
        } else {
            self.start_slot(audio, next_id, fade_in_ms)?;
        }
        Ok(())
    }

    pub fn ready_name(
        &mut self,
        audio: &mut AudioHub,
        name: &str,
        start_pos_sample: i64,
    ) -> Result<()> {
        self.play_name_script(audio, name, self.loop_flag, 0, 0, start_pos_sample, true, 0)
    }

    pub fn play_name_with_options(
        &mut self,
        audio: &mut AudioHub,
        name: &str,
        start_pos_sample: i64,
        fade_in_ms: i64,
    ) -> Result<()> {
        self.play_name_script(
            audio,
            name,
            self.loop_flag,
            fade_in_ms,
            0,
            start_pos_sample,
            false,
            0,
        )
    }

    pub fn play_name(&mut self, audio: &mut AudioHub, name: &str) -> Result<()> {
        self.play_name_script(audio, name, true, 0, 0, TNM_BGM_START_POS_INI, false, 0)
    }

    pub fn play_pos_samples(&self) -> u64 {
        self.current_slot()
            .map(|s| s.play_pos_samples())
            .unwrap_or(0)
    }

    pub fn pause_fade(&mut self, _audio: &mut AudioHub, fade_ms: i64) -> Result<()> {
        self.delay_deadline = None;
        self.pause_flag = true;
        let amp = self.total_gain_amplitude();
        let Some(slot) = self.current_slot_mut() else {
            return Ok(());
        };
        if slot.handle.is_none() || slot.paused_at.is_some() {
            return Ok(());
        }
        if fade_ms > 0 {
            if let Some(h) = &mut slot.handle {
                let _ = h.set_volume(Volume::Amplitude(amp), Tween::default());
                let _ = h.set_volume(Volume::Amplitude(0.0), tween_ms(fade_ms));
            }
            slot.fade_outing = true;
            slot.pending = Some(PendingBgmAction {
                kind: PendingBgmActionKind::Pause,
                at: Instant::now() + Duration::from_millis(fade_ms as u64),
            });
        } else {
            if let Some(h) = &mut slot.handle {
                let _ = h.pause(Tween::default());
            }
            slot.paused_at = Some(Instant::now());
        }
        Ok(())
    }

    pub fn pause(&mut self) -> Result<()> {
        self.delay_deadline = None;
        self.pause_flag = true;
        let Some(slot) = self.current_slot_mut() else {
            return Ok(());
        };
        if slot.handle.is_none() || slot.paused_at.is_some() {
            return Ok(());
        }
        if let Some(h) = &mut slot.handle {
            let _ = h.pause(Tween::default());
        }
        slot.paused_at = Some(Instant::now());
        Ok(())
    }

    pub fn resume_script(
        &mut self,
        audio: &mut AudioHub,
        fade_in_ms: i64,
        delay_time_ms: i64,
    ) -> Result<()> {
        if delay_time_ms > 0 {
            self.delay_deadline =
                Some(Instant::now() + Duration::from_millis(delay_time_ms as u64));
            self.delayed_fade_in_ms = fade_in_ms;
            self.pause_flag = false;
            return Ok(());
        }

        let amp = self.total_gain_amplitude();
        let Some(cur_id) = self.current_player_id else {
            return Ok(());
        };
        if self.players[cur_id].handle.is_none() {
            self.start_slot(audio, cur_id, fade_in_ms)?;
            self.pause_flag = false;
            self.delay_deadline = None;
            return Ok(());
        }

        let slot = &mut self.players[cur_id];
        if let Some(p) = slot.paused_at.take() {
            slot.paused_total += Instant::now().saturating_duration_since(p);
        }
        if let Some(h) = &mut slot.handle {
            let _ = h.resume(Tween::default());
            if fade_in_ms > 0 {
                let _ = h.set_volume(Volume::Amplitude(0.0), Tween::default());
                let _ = h.set_volume(Volume::Amplitude(amp), tween_ms(fade_in_ms));
            } else {
                let _ = h.set_volume(Volume::Amplitude(amp), Tween::default());
            }
        }
        slot.fade_outing = false;
        slot.pending = None;
        slot.ready_only = false;
        self.pause_flag = false;
        self.delay_deadline = None;
        Ok(())
    }

    pub fn resume_fade(&mut self, audio: &mut AudioHub, fade_ms: i64) -> Result<()> {
        self.resume_script(audio, fade_ms, 0)
    }

    pub fn resume(&mut self) -> Result<()> {
        let amp = self.total_gain_amplitude();
        let Some(slot) = self.current_slot_mut() else {
            return Ok(());
        };
        if let Some(p) = slot.paused_at.take() {
            slot.paused_total += Instant::now().saturating_duration_since(p);
            if let Some(h) = &mut slot.handle {
                let _ = h.resume(Tween::default());
                let _ = h.set_volume(Volume::Amplitude(amp), Tween::default());
            }
        }
        slot.ready_only = false;
        self.pause_flag = false;
        Ok(())
    }

    pub fn stop(&mut self) -> Result<()> {
        self.stop_current_internal(0)
    }

    fn stop_current_internal(&mut self, fade_out_ms: i64) -> Result<()> {
        self.delay_deadline = None;
        if let Some(slot) = self.current_slot_mut() {
            if slot.handle.is_none() {
                slot.clear_runtime_only();
            } else if fade_out_ms > 0 {
                if let Some(h) = &mut slot.handle {
                    let _ = h.stop(tween_ms(fade_out_ms));
                }
                slot.fade_outing = true;
                slot.pending = Some(PendingBgmAction {
                    kind: PendingBgmActionKind::Stop,
                    at: Instant::now() + Duration::from_millis(fade_out_ms as u64),
                });
            } else {
                if let Some(mut h) = slot.handle.take() {
                    let _ = h.stop(Tween::default());
                }
                slot.clear_runtime_only();
            }
        }
        self.current_name = None;
        self.loop_flag = false;
        self.pause_flag = false;
        Ok(())
    }

    pub fn stop_fade(&mut self, fade_out_ms: i64) -> Result<()> {
        self.stop_current_internal(fade_out_ms)
    }

    pub fn tick(&mut self, audio: &mut AudioHub) -> Result<()> {
        let now = Instant::now();
        self.retired.retain_mut(|(h, deadline)| {
            if now >= *deadline {
                let _ = h.stop(Tween::default());
                false
            } else {
                true
            }
        });

        if let Some(deadline) = self.delay_deadline {
            if now >= deadline {
                self.delay_deadline = None;
                self.resume_script(audio, self.delayed_fade_in_ms, 0)?;
            }
        }

        let Some(cur_id) = self.current_player_id else {
            return Ok(());
        };

        let pending = self.players[cur_id].pending;
        if let Some(pending) = pending {
            if now >= pending.at {
                let slot = &mut self.players[cur_id];
                slot.pending = None;
                match pending.kind {
                    PendingBgmActionKind::Stop => {
                        if let Some(mut h) = slot.handle.take() {
                            let _ = h.stop(Tween::default());
                        }
                        slot.clear_runtime_only();
                    }
                    PendingBgmActionKind::Pause => {
                        if let Some(h) = &mut slot.handle {
                            let _ = h.pause(Tween::default());
                        }
                        slot.paused_at = Some(now);
                        slot.fade_outing = false;
                    }
                }
            }
        }

        let (paused, has_handle, segment_samples, elapsed_samples, has_loop_region) = {
            let slot = &self.players[cur_id];
            (
                slot.paused_at.is_some(),
                slot.handle.is_some(),
                slot.playback_window_samples(),
                slot.elapsed_samples(),
                slot.has_loop_region(),
            )
        };
        if paused || !has_handle {
            return Ok(());
        }
        if segment_samples == 0 {
            return Ok(());
        }
        if elapsed_samples < segment_samples {
            return Ok(());
        }
        if has_loop_region {
            return Ok(());
        } else {
            let slot = &mut self.players[cur_id];
            if let Some(mut h) = slot.handle.take() {
                let _ = h.stop(Tween::default());
            }
            slot.clear_runtime_only();
        }
        Ok(())
    }
}

fn find_gameexe_path(project_dir: &Path) -> Option<PathBuf> {
    const CANDIDATES: &[&str] = &[
        "Gameexe.dat",
        "Gameexe.ini",
        "gameexe.dat",
        "gameexe.ini",
        "GameexeEN.dat",
        "GameexeEN.ini",
        "GameexeZH.dat",
        "GameexeZH.ini",
        "GameexeZHTW.dat",
        "GameexeZHTW.ini",
        "GameexeDE.dat",
        "GameexeDE.ini",
        "GameexeES.dat",
        "GameexeES.ini",
        "GameexeFR.dat",
        "GameexeFR.ini",
        "GameexeID.dat",
        "GameexeID.ini",
    ];
    for name in CANDIDATES {
        let p = project_dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn load_gameexe_config(project_dir: &Path) -> Option<GameexeConfig> {
    let path = find_gameexe_path(project_dir)?;
    let raw = std::fs::read(&path).ok()?;
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ini"))
    {
        let text = String::from_utf8(raw).ok()?;
        return Some(GameexeConfig::from_text(&text));
    }
    let opt = GameexeDecodeOptions::from_project_dir(project_dir).ok()?;
    let (text, _report) = decode_gameexe_dat_bytes(&raw, &opt).ok()?;
    Some(GameexeConfig::from_text(&text))
}
