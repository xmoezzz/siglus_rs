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
				Self {
					manager: Some(manager),
					bgm,
					se,
					pcm,
				}
			}
			Err(e) => {
				eprintln!("kira init failed, audio disabled: {:#}", e);
				Self {
					manager: None,
					bgm: None,
					se: None,
					pcm: None,
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
		}
	}

	fn track_mut(&mut self, kind: TrackKind) -> Option<&mut TrackHandle> {
		match kind {
			TrackKind::Bgm => self.bgm.as_mut(),
			TrackKind::Se => self.se.as_mut(),
			TrackKind::Pcm => self.pcm.as_mut(),
		}
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
		let Some(track) = self.track_mut(kind) else {
			return;
		};
		let amp = (volume_raw as f64) / 255.0;
		track.set_volume(Volume::Amplitude(amp), Tween::default());
	}
}
