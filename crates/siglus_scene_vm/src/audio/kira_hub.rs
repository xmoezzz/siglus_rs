use std::fmt;

use anyhow::{anyhow, Context, Result};
use kira::manager::{
	backend::DefaultBackend,
	AudioManager,
	AudioManagerSettings,
};
use kira::sound::static_sound::{StaticSoundData, StaticSoundHandle};
use kira::track::{TrackBuilder, TrackHandle};
use kira::tween::Tween;
use kira::Volume;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrackKind {
	Bgm,
	Se,
	Pcm,
	Mov,
}

/// Shared Kira backend.
///
/// Kira 0.9 uses `AudioManager::play` plus `StaticSoundData::output_destination(&track)`.
/// We keep one sub-track per Siglus category (BGM/SE/PCM).
pub struct AudioHub {
	manager: Option<AudioManager<DefaultBackend>>,
	bgm: Option<TrackHandle>,
	se: Option<TrackHandle>,
	pcm: Option<TrackHandle>,
	mov: Option<TrackHandle>,
	bgm_base: u8,
	se_base: u8,
	pcm_base: u8,
	mov_base: u8,
	bgm_master: u8,
	se_master: u8,
	pcm_master: u8,
	mov_master: u8,
}

impl fmt::Debug for AudioHub {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.debug_struct("AudioHub")
			.field("enabled", &self.manager.is_some())
			.finish()
	}
}

impl AudioHub {
	pub fn new() -> Self {
		match AudioManager::<DefaultBackend>::new(AudioManagerSettings::default()) {
			Ok(mut manager) => {
				let bgm = manager.add_sub_track(TrackBuilder::default()).ok();
				let se = manager.add_sub_track(TrackBuilder::default()).ok();
				let pcm = manager.add_sub_track(TrackBuilder::default()).ok();
				let mov = manager.add_sub_track(TrackBuilder::default()).ok();
				Self {
					manager: Some(manager),
					bgm,
					se,
					pcm,
					mov,
					bgm_base: 255,
					se_base: 255,
					pcm_base: 255,
					mov_base: 255,
					bgm_master: 255,
					se_master: 255,
					pcm_master: 255,
					mov_master: 255,
				}
			}
			Err(e) => {
				eprintln!("kira init failed, audio disabled: {:#}", e);
				Self {
					manager: None,
					bgm: None,
					se: None,
					pcm: None,
					mov: None,
					bgm_base: 255,
					se_base: 255,
					pcm_base: 255,
					mov_base: 255,
					bgm_master: 255,
					se_master: 255,
					pcm_master: 255,
					mov_master: 255,
				}
			}
		}
	}

	pub fn is_enabled(&self) -> bool {
		self.manager.is_some()
	}

	fn track_ref(&self, kind: TrackKind) -> Option<&TrackHandle> {
		match kind {
			TrackKind::Bgm => self.bgm.as_ref(),
			TrackKind::Se => self.se.as_ref(),
			TrackKind::Pcm => self.pcm.as_ref(),
			TrackKind::Mov => self.mov.as_ref(),
		}
	}

	fn track_mut(&mut self, kind: TrackKind) -> Option<&mut TrackHandle> {
		match kind {
			TrackKind::Bgm => self.bgm.as_mut(),
			TrackKind::Se => self.se.as_mut(),
			TrackKind::Pcm => self.pcm.as_mut(),
			TrackKind::Mov => self.mov.as_mut(),
		}
	}

	fn track_base_mut(&mut self, kind: TrackKind) -> &mut u8 {
		match kind {
			TrackKind::Bgm => &mut self.bgm_base,
			TrackKind::Se => &mut self.se_base,
			TrackKind::Pcm => &mut self.pcm_base,
			TrackKind::Mov => &mut self.mov_base,
		}
	}

	fn track_master_mut(&mut self, kind: TrackKind) -> &mut u8 {
		match kind {
			TrackKind::Bgm => &mut self.bgm_master,
			TrackKind::Se => &mut self.se_master,
			TrackKind::Pcm => &mut self.pcm_master,
			TrackKind::Mov => &mut self.mov_master,
		}
	}

	fn track_base(&self, kind: TrackKind) -> u8 {
		match kind {
			TrackKind::Bgm => self.bgm_base,
			TrackKind::Se => self.se_base,
			TrackKind::Pcm => self.pcm_base,
			TrackKind::Mov => self.mov_base,
		}
	}

	fn track_master(&self, kind: TrackKind) -> u8 {
		match kind {
			TrackKind::Bgm => self.bgm_master,
			TrackKind::Se => self.se_master,
			TrackKind::Pcm => self.pcm_master,
			TrackKind::Mov => self.mov_master,
		}
	}

	fn apply_track_volume(&mut self, kind: TrackKind) {
		let base = self.track_base(kind) as f64 / 255.0;
		let master = self.track_master(kind) as f64 / 255.0;
		let amp = base * master;
		let Some(track) = self.track_mut(kind) else {
			return;
		};
		track.set_volume(Volume::Amplitude(amp), Tween::default());
	}

	fn apply_track_volume_fade(&mut self, kind: TrackKind, fade_ms: i64) {
		let base = self.track_base(kind) as f64 / 255.0;
		let master = self.track_master(kind) as f64 / 255.0;
		let amp = base * master;
		let Some(track) = self.track_mut(kind) else {
			return;
		};
		let tween = if fade_ms > 0 {
			Tween::new(std::time::Duration::from_millis(fade_ms as u64))
		} else {
			Tween::default()
		};
		track.set_volume(Volume::Amplitude(amp), tween);
	}

	pub fn play_static(&mut self, kind: TrackKind, data: StaticSoundData) -> Result<StaticSoundHandle> {
			// Decide output destination before taking a mutable borrow of the manager.
			let data = if let Some(track) = self.track_ref(kind) {
				data.output_destination(track)
			} else {
				data
			};

			let manager = self
				.manager
				.as_mut()
				.ok_or_else(|| anyhow!("audio disabled"))?;

			manager.play(data).context("kira: play static sound")
	}

	pub fn set_track_volume_raw(&mut self, kind: TrackKind, volume_raw: u8) {
		*self.track_base_mut(kind) = volume_raw;
		self.apply_track_volume(kind);
	}

	pub fn set_track_master_volume_raw(&mut self, kind: TrackKind, volume_raw: u8) {
		*self.track_master_mut(kind) = volume_raw;
		self.apply_track_volume(kind);
	}

	pub fn set_track_volume_raw_fade(&mut self, kind: TrackKind, volume_raw: u8, fade_ms: i64) {
		*self.track_base_mut(kind) = volume_raw;
		self.apply_track_volume_fade(kind, fade_ms);
	}
}
