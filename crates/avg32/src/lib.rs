//! VisualArt's AVG32 engine (the pre-RealLive system behind Kanon, AIR and
//! many other titles).
//!
//! * [`scenario`] interprets `TPC32` bytecode from `SEEN.TXT`;
//! * [`system`], [`meswin`] and [`menu`] provide the message window,
//!   choices, menus, save/load, animation and input;
//! * [`pdtmgr`] and [`effect`] implement the 32 PDT buffers and every
//!   screen transition;
//! * [`sound`] plays CD/DirectSound BGM, WAV effects and KOE voices.

#[cfg(feature = "native-engine")]
pub mod animation;
#[cfg(feature = "native-engine")]
pub mod archive;
#[cfg(feature = "native-engine")]
pub mod ard;
#[cfg(feature = "native-engine")]
pub mod buffer;
#[cfg(feature = "native-engine")]
pub mod cgmode;
#[cfg(feature = "native-engine")]
pub mod cursor;
#[cfg(feature = "native-engine")]
pub mod effect;
#[cfg(feature = "native-engine")]
pub mod engine;
pub mod flags;
#[cfg(feature = "native-engine")]
pub mod font;
#[cfg(feature = "native-engine")]
pub mod game;
#[cfg(feature = "native-engine")]
pub mod ini;
#[cfg(feature = "native-engine")]
pub mod menu;
#[cfg(feature = "native-engine")]
pub mod meswin;
#[cfg(feature = "native-engine")]
pub mod movie;
pub mod nls;
#[cfg(feature = "native-engine")]
pub mod pdt;
#[cfg(feature = "native-engine")]
pub mod pdtmgr;
#[cfg(feature = "portable-vm")]
pub mod portable;
#[cfg(feature = "native-engine")]
pub mod resource;
#[cfg(feature = "native-engine")]
pub mod savedata;
pub mod scenario;
pub mod scene;
#[cfg(feature = "native-engine")]
pub mod sound;
#[cfg(feature = "native-engine")]
pub mod system;
#[cfg(feature = "native-engine")]
pub mod voicepatch;

#[cfg(feature = "native-engine")]
pub use archive::{PaclArchive, PaclEntry};
#[cfg(feature = "native-engine")]
pub use buffer::{AVG32_HEIGHT, AVG32_WIDTH};
#[cfg(feature = "native-engine")]
pub use engine::{Avg32Engine, EngineOptions};
#[cfg(feature = "native-engine")]
pub use game::{Avg32Game, EngineKind, GameLayout, detect_game_root};
pub use nls::Nls;
#[cfg(feature = "native-engine")]
pub use pdt::{PdtImage, decode_pdt};
pub use scene::{Avg32SceneHeader, SceneMenu, SceneSubmenu, SceneValue, ValueKind};
#[cfg(feature = "native-engine")]
pub use system::Key;

#[cfg(all(feature = "portable-vm", feature = "native-engine"))]
compile_error!(
    "portable-vm and native-engine are exclusive; disable default features for portable-vm"
);

pub mod input;
mod rng;
