//! Public Emote-facing control surface that must stay stable while the
//! renderer/parser internals are replaced.
//!
//! This module intentionally mirrors the observed SDK usage shape instead of
//! exposing PSB internals. The implementation layer can later bind these calls
//! to parsed PSB data, motion timelines, and the WebGPU renderer.

/// One original E-mote time unit is one 1/60-second frame tick.
pub const EMOTE_TICKS_PER_SECOND: f32 = 60.0;

/// Official JS player clamps one requestAnimationFrame delta to at most 100 ms
/// before converting it to Emote frame ticks.
pub const EMOTE_UPDATE_MS_CAP: f32 = 100.0;

/// Converts milliseconds to the original driver frame-count unit.
pub fn milliseconds_to_emote_ticks(ms: f32) -> f32 {
    ms * EMOTE_TICKS_PER_SECOND / 1000.0
}

/// Converts the original driver frame-count unit to milliseconds.
pub fn emote_ticks_to_milliseconds(ticks: f32) -> f32 {
    ticks * 1000.0 / EMOTE_TICKS_PER_SECOND
}

/// Official device mask mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EmoteMaskMode {
    Stencil,
    Alpha,
}

impl EmoteMaskMode {
    pub const DEFAULT: Self = Self::Alpha;
}

/// Default render-device behavior confirmed from the official JS player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmoteDeviceRenderOptions {
    pub mask_mode: EmoteMaskMode,
    pub protect_translucent_texture_color: bool,
    pub mask_region_clipping: bool,
}

impl Default for EmoteDeviceRenderOptions {
    fn default() -> Self {
        Self {
            mask_mode: EmoteMaskMode::DEFAULT,
            protect_translucent_texture_color: true,
            mask_region_clipping: true,
        }
    }
}

/// Transform-order mask values from official `IEmotePlayer::transform_order_mask_t`.
pub mod transform_order_mask {
    pub const POSITION_TRANSLATE_TO_SCALE: u32 = 1 << 0;
    pub const POSITION_SCALE_TO_TRANSLATE: u32 = 1 << 1;
    pub const PHYSICS_TRANSLATE_TO_SCALE: u32 = 1 << 8;
    pub const PHYSICS_SCALE_TO_TRANSLATE: u32 = 1 << 9;

    pub const TRANSLATE_TO_SCALE: u32 = POSITION_TRANSLATE_TO_SCALE | PHYSICS_TRANSLATE_TO_SCALE;
    pub const SCALE_TO_TRANSLATE: u32 = POSITION_SCALE_TO_TRANSLATE | PHYSICS_SCALE_TO_TRANSLATE;
    pub const DEFAULT: u32 = POSITION_TRANSLATE_TO_SCALE | PHYSICS_SCALE_TO_TRANSLATE;

    /// Tyrano official wrapper value for orthogonal mode.
    pub const TYRANO_ORTHOGONAL: u32 = 0x101;
    /// Tyrano official wrapper value for perspective mode.
    pub const TYRANO_PERSPECTIVE: u32 = 0x202;
    /// Tyrano official wrapper default/mix value.
    pub const TYRANO_MIX: u32 = 0x201;
}

/// Timeline playback request.
///
/// The official SDK uses bit flags for `PlayTimeline`: `PARALLEL` and
/// `DIFFERENCE`. Older eluna code also used this type as a local loop/once
/// marker, so the structure keeps both pieces explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimelinePlayMode {
    pub flags: u32,
    pub looping: bool,
}

impl TimelinePlayMode {
    pub const ONCE: Self = Self {
        flags: 0,
        looping: false,
    };
    pub const LOOP: Self = Self {
        flags: 0,
        looping: true,
    };

    /// Backward-compatible aliases used by older code.
    pub const Once: Self = Self::ONCE;
    pub const Loop: Self = Self::LOOP;

    pub const PARALLEL: Self = Self {
        flags: 1 << 0,
        looping: false,
    };
    pub const DIFFERENCE: Self = Self {
        flags: 1 << 1,
        looping: false,
    };
    pub const PARALLEL_DIFFERENCE: Self = Self {
        flags: (1 << 0) | (1 << 1),
        looping: false,
    };

    pub fn with_looping(mut self, looping: bool) -> Self {
        self.looping = looping;
        self
    }

    pub fn is_looping(self) -> bool {
        self.looping
    }

    pub fn is_difference(self) -> bool {
        (self.flags & (1 << 1)) != 0
    }

    pub const fn from_flags(flags: u32) -> Self {
        Self {
            flags,
            looping: false,
        }
    }
}

/// A single variable/parameter write request.
///
/// Public samples call this through `SetVariable(name, value)` and
/// `SetVariable(name, value, time, easing)`. The `time_ticks` field uses the
/// original 1/60-second frame-count unit, not milliseconds.
#[derive(Debug, Clone, PartialEq)]
pub struct VariableWrite {
    pub name: String,
    pub value: f32,
    pub time_ticks: f32,
    pub easing: f32,
}

impl VariableWrite {
    pub fn immediate(name: impl Into<String>, value: f32) -> Self {
        Self {
            name: name.into(),
            value,
            time_ticks: 0.0,
            easing: 0.0,
        }
    }

    pub fn timed(name: impl Into<String>, value: f32, time_ticks: f32, easing: f32) -> Self {
        Self {
            name: name.into(),
            value,
            time_ticks,
            easing,
        }
    }
}

/// Stable high-level player API to preserve while replacing the original DLL.
pub trait EmotePlayerControl {
    fn show(&mut self);
    fn hide(&mut self);
    fn progress_ticks(&mut self, delta_ticks: f32);
    fn render(&mut self);

    fn coord(&self) -> [f32; 2];
    fn set_coord(&mut self, x: f32, y: f32);

    fn scale(&self) -> f32;
    fn set_scale(&mut self, scale: f32);

    fn rot(&self) -> f32;
    fn set_rot(&mut self, rot: f32);

    fn set_variable(&mut self, name: &str, value: f32) {
        self.set_variable_timed(name, value, 0.0, 0.0);
    }

    fn set_variable_timed(&mut self, name: &str, value: f32, time_ticks: f32, easing: f32);
    fn set_variable_diff(
        &mut self,
        module: &str,
        name: &str,
        value: f32,
        time_ticks: f32,
        easing: f32,
    );

    fn play_timeline(&mut self, name: &str, mode: TimelinePlayMode);
    fn stop_timeline(&mut self, name: &str);
    fn set_timeline_blend_ratio(
        &mut self,
        name: &str,
        value: f32,
        time_ticks: f32,
        easing: f32,
        stop_when_done: bool,
    );
    fn fade_in_timeline(&mut self, name: &str, time_ticks: f32, easing: f32);
    fn fade_out_timeline(&mut self, name: &str, time_ticks: f32, easing: f32);

    fn set_outer_force(&mut self, label: &str, x: f32, y: f32, time_ticks: f32, easing: f32);
    fn outer_force(&self, label: &str) -> [f32; 2];
    fn set_outer_rot(&mut self, rot: f32, time_ticks: f32, easing: f32);
    fn outer_rot(&self) -> f32;
    fn start_wind(&mut self, start: f32, goal: f32, speed: f32, pow_min: f32, pow_max: f32);
    fn stop_wind(&mut self);

    fn set_transform_order_mask(&mut self, mask: u32);
    fn transform_order_mask(&self) -> u32;
    fn set_hair_scale(&mut self, scale: f32);
    fn hair_scale(&self) -> f32;
    fn set_parts_scale(&mut self, scale: f32);
    fn parts_scale(&self) -> f32;
    fn set_bust_scale(&mut self, scale: f32);
    fn bust_scale(&self) -> f32;

    fn skip(&mut self);
}
