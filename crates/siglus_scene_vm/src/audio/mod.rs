//! Audio subsystem.

pub mod bgm;
pub mod engine;
pub mod kira_hub;
pub mod sfx_engine;

pub use engine::{
    BgmEngine, TNM_PLAYER_STATE_FADE_OUT, TNM_PLAYER_STATE_FREE, TNM_PLAYER_STATE_PAUSE,
    TNM_PLAYER_STATE_PLAY,
};
pub use kira_hub::{AudioHub, TrackKind};
pub use sfx_engine::{PcmEngine, SeEngine};
