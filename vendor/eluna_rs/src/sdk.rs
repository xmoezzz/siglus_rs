//! Stable third-party facade for loading and driving Emote models.
//!
//! This module keeps callers away from PSB traversal details. It intentionally
//! exposes the runtime result as drawFrameInfo plus texture resources, because
//! the original renderer contract is draw-list oriented.

use crate::api::{
    milliseconds_to_emote_ticks, transform_order_mask, EmotePlayerControl, TimelinePlayMode,
};
use crate::{
    collect_emote_runtime_pipeline, collect_emote_timelines, collect_emote_variables, ElunaPlayer,
    EmoteDrawFrameInfo, EmoteGroundCorrectionHook, EmoteModelSchema, EmoteMotionInfo, EmoteRuntimePipeline, EmoteSceneBounds,
    EmoteSchemaError, EmoteStaticScene, EmoteStaticSprite, EmoteTextureSource, PsbDecryptionKey,
    PsbError, PsbFile, PsbNormalizeOptions, PsbValue,
};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct EmoteLoadOptions {
    pub normalize: PsbNormalizeOptions,
    pub motion: Option<String>,
    pub timeline: Option<String>,
    pub autoplay_timeline: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmoteTransformMode {
    Orthogonal,
    Perspective,
    Mix,
}

impl EmoteTransformMode {
    pub fn transform_order_mask(self) -> u32 {
        match self {
            Self::Orthogonal => transform_order_mask::TYRANO_ORTHOGONAL,
            Self::Perspective => transform_order_mask::TYRANO_PERSPECTIVE,
            Self::Mix => transform_order_mask::TYRANO_MIX,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EmoteNewOptions {
    pub name: String,
    pub jname: Option<String>,
    pub layer: i32,
    pub page: String,
    pub scale: f32,
    pub x: f32,
    pub y: f32,
    pub grayscale: f32,
    pub color_rgb: u32,
    pub mesh_division_ratio: f32,
    pub physics: bool,
    pub zindex: i32,
    pub reset_motion_on_hide: Option<String>,
}

impl Default for EmoteNewOptions {
    fn default() -> Self {
        Self {
            name: String::new(),
            jname: None,
            layer: 0,
            page: "fore".to_owned(),
            scale: 1.0,
            x: 0.0,
            y: 0.0,
            grayscale: 0.0,
            color_rgb: 0x00ff_ffff,
            mesh_division_ratio: 1.0,
            physics: true,
            zindex: 1,
            reset_motion_on_hide: Some("初期化".to_owned()),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EmoteShowOptions {
    pub scale: Option<f32>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub grayscale: Option<f32>,
    pub color_rgb: Option<u32>,
    pub time_ms: f32,
    pub wait: bool,
    pub zindex: Option<i32>,
}

impl Default for EmoteShowOptions {
    fn default() -> Self {
        Self {
            scale: None,
            x: None,
            y: None,
            grayscale: None,
            color_rgb: None,
            time_ms: 1000.0,
            wait: true,
            zindex: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EmoteHideOptions {
    pub time_ms: f32,
    pub wait: bool,
}

impl Default for EmoteHideOptions {
    fn default() -> Self {
        Self {
            time_ms: 1000.0,
            wait: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct EmoteTransOptions {
    pub time_ms: f32,
    pub scale: Option<f32>,
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub grayscale: Option<f32>,
    pub color_rgb: Option<u32>,
    pub zindex: Option<i32>,
    pub wait: bool,
}

impl Default for EmoteTransOptions {
    fn default() -> Self {
        Self {
            time_ms: 1000.0,
            scale: None,
            x: None,
            y: None,
            grayscale: None,
            color_rgb: None,
            zindex: None,
            wait: true,
        }
    }
}

impl Default for EmoteLoadOptions {
    fn default() -> Self {
        Self {
            normalize: PsbNormalizeOptions::default(),
            motion: None,
            timeline: None,
            autoplay_timeline: true,
        }
    }
}

impl EmoteLoadOptions {
    pub fn with_emote_key(mut self, key: u32) -> Self {
        self.normalize.decrypt_key = Some(PsbDecryptionKey::emote_key(key));
        self
    }

    pub fn with_motion(mut self, motion: impl Into<String>) -> Self {
        self.motion = Some(motion.into());
        self
    }

    pub fn with_timeline(mut self, timeline: impl Into<String>) -> Self {
        self.timeline = Some(timeline.into());
        self
    }
}

#[derive(Debug)]
pub enum EmoteRuntimeError {
    Io(io::Error),
    Psb(PsbError),
    Schema(EmoteSchemaError),
    MissingActiveMotion,
    MissingTimeline(String),
    RuntimeControl(String),
    InvalidMotionSlot(usize),
}

impl fmt::Display for EmoteRuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EmoteRuntimeError::Io(err) => write!(f, "{err}"),
            EmoteRuntimeError::Psb(err) => write!(f, "{err}"),
            EmoteRuntimeError::Schema(err) => write!(f, "{err}"),
            EmoteRuntimeError::MissingActiveMotion => write!(f, "Emote model has no active motion"),
            EmoteRuntimeError::MissingTimeline(name) => {
                write!(f, "timeline '{name}' does not exist")
            }
            EmoteRuntimeError::RuntimeControl(message) => write!(f, "{message}"),
            EmoteRuntimeError::InvalidMotionSlot(slot) => {
                write!(f, "motion slot must be main or 1..6, got {slot}")
            }
        }
    }
}

impl Error for EmoteRuntimeError {}

impl From<io::Error> for EmoteRuntimeError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<PsbError> for EmoteRuntimeError {
    fn from(value: PsbError) -> Self {
        Self::Psb(value)
    }
}

impl From<EmoteSchemaError> for EmoteRuntimeError {
    fn from(value: EmoteSchemaError) -> Self {
        Self::Schema(value)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmoteRuntimeParityReport {
    pub confirmed: Vec<&'static str>,
    pub partial: Vec<&'static str>,
    pub missing: Vec<&'static str>,
}

pub fn emote_runtime_parity_report() -> EmoteRuntimeParityReport {
    EmoteRuntimeParityReport {
        confirmed: vec![
            "PSB/MDF/LZ4/key normalization and texture atlas extraction",
            "stateful timeline/control progression including type-0 HOLD and authored loops",
            "native Timeline blend lifecycle: active-only SetTimelineBlendRatio, DIFFERENCE-only stepping, queue/replace rules, unclamped scalar output, idle-stop flag, seek preservation, replay reset, and FadeIn/FadeOut wrappers",
            "native frame transformOrder and player transform-order position/physics halves",
            "raw +612/+616/+620 versus post-MeshChain +120/+124/+128 coordinate split",
            "specialized-pass ordering for Anchor/MeshChain/readyToDraw/Camera/type-7/Shape/nested/Model/Particle/Feedback",
            "type-7 raw-coordinate bounds propagation and clipping",
            "Shape point/circle/rect/quad transform semantics",
            "joinTarget decoded-frame and type-4 emitter state transfer",
            "persistent particle child-MMotionPlayer lifecycle, continuous emission, flyDirection branches, and entered-screen deletion",
            "native tri-volume particle Z scaling as sqrt(abs(det(layer_2x2))) for non-uniform scale/shear/reflection",
            "native stencil ancestry reconstruction, Inner EQUAL+INCR, Outer EQUAL+DECR, composite-mask source lists, and type-12 wipe parameters",
            "Feedback decay math and previous-framebuffer sampling",
            "Camera FOV/eye/target/rounded-offset state per root and nested MMotionPlayer scope; standard 2-D draw path proven not to consume the offset",
            "stereovisionControl metadata hierarchy, exact per-screen affine variable projection, active-mirror XOR, screen-order reversal, and six-field stereovisionProfile extraction",
            "Model type-6 local-time/direction output for direction modes 2/3/4",
            "groundCorrection host callback boundary and corrected raw-position writeback",
            "bm/bp/four-corner color/objTriPriority/screenBounds metadata retention",
            "bp/objTriPriority absence from this DLL's standard 2-D DrawFrameInfo consumer path",
        ],
        partial: vec![
            "native runtime frame-mesh path content.mesh.bp is recovered; exporter-side meshCombinator/rawMeshList compatibility schema remains corpus-dependent because this DLL does not name those fields",
            "stereovision player-side screen images are recovered; physical multiview/display composition remains a host responsibility",
            "Model player-side type-6 state is recovered; loading/rendering referenceModelFileList resources remains a host 3-D backend responsibility",
            "type-12 wipe/stencil and type-10 Feedback GPU paths are implemented but still require representative GPU runtime validation",
            "particle random range/call sites are recovered but the DLL's host-global RNG seed/state is not serialized; the portable trigger-byte reconstruction can still differ on host-forced dirty paths",
        ],
        missing: vec![
            "binary-compatible IEmotePlayer/PEmotePlayer ABI",
            "host-specific stereoscopic display compositor and 3-D model renderer",
            "host geometry behavior when no groundCorrection callback is supplied",
            "actual cargo/test/GPU validation in the current toolchain-less execution environment",
        ],
    }
}

#[derive(Debug, Clone)]
pub struct EmoteRuntime {
    normalized_data: Vec<u8>,
    psb: PsbFile,
    schema: EmoteModelSchema,
    active_motion: Option<String>,
    player: ElunaPlayer,
    name: Option<String>,
    jname: Option<String>,
    layer: i32,
    page: String,
    zindex: i32,
    reset_motion_on_hide: Option<String>,
    diff_slots: [Option<String>; 6],
    ground_correction_hook: Option<EmoteGroundCorrectionHook>,
}

impl EmoteRuntime {
    pub fn from_path(
        path: impl AsRef<Path>,
        options: EmoteLoadOptions,
    ) -> Result<Self, EmoteRuntimeError> {
        let data = fs::read(path)?;
        Self::from_bytes(&data, options)
    }

    pub fn from_bytes(data: &[u8], options: EmoteLoadOptions) -> Result<Self, EmoteRuntimeError> {
        let (normalized_data, psb) = PsbFile::parse_normalized(data, &options.normalize)?;
        let schema = EmoteModelSchema::from_psb(&psb)?;
        let active_motion = options
            .motion
            .clone()
            .or_else(|| schema.default_motion_name(&psb).ok().flatten());
        let variables = collect_emote_variables(&psb);
        let timelines = collect_emote_timelines(&psb);
        let initial_values = initial_variable_values(&variables);
        let runtime_pipeline = collect_emote_runtime_pipeline(&psb);
        let scene = match active_motion.as_deref() {
            Some(motion) => schema.build_motion_scene_at_with_resources_and_variables(
                &psb,
                &normalized_data,
                motion,
                0.0,
                &initial_values,
            )?,
            None => schema.build_static_scene(&psb)?,
        };
        let mut player = ElunaPlayer::from_scene_variables_timelines_runtime(
            scene,
            variables,
            timelines,
            runtime_pipeline,
        );

        if options.autoplay_timeline {
            if let Some(name) = options.timeline.clone().or_else(|| {
                active_motion
                    .as_deref()
                    .and_then(|motion| find_timeline_for_motion(&player, motion))
                    .or_else(|| {
                        player
                            .timelines()
                            .values()
                            .find(|timeline| !timeline.is_difference)
                            .map(|timeline| timeline.name.clone())
                    })
            }) {
                if !player.timelines().contains_key(&name) {
                    return Err(EmoteRuntimeError::MissingTimeline(name));
                }
                player.play_timeline(&name, TimelinePlayMode::ONCE);
            }
        }

        let mut runtime = Self {
            normalized_data,
            psb,
            schema,
            active_motion,
            player,
            name: None,
            jname: None,
            layer: 0,
            page: "fore".to_owned(),
            zindex: 1,
            reset_motion_on_hide: Some("初期化".to_owned()),
            diff_slots: Default::default(),
            ground_correction_hook: None,
        };
        runtime.rebuild_scene()?;
        Ok(runtime)
    }

    pub fn progress_ticks(&mut self, delta_ticks: f32) -> Result<(), EmoteRuntimeError> {
        self.player.progress_ticks_without_physics(delta_ticks);
        self.rebuild_scene_with_physics(delta_ticks)
    }

    pub fn progress_seconds(&mut self, delta_seconds: f32) -> Result<(), EmoteRuntimeError> {
        self.progress_ticks(delta_seconds * crate::EMOTE_TICKS_PER_SECOND)
    }

    pub fn progress_milliseconds_capped(&mut self, delta_ms: f32) -> Result<(), EmoteRuntimeError> {
        self.progress_ticks(crate::milliseconds_to_emote_ticks(
            delta_ms.min(crate::EMOTE_UPDATE_MS_CAP),
        ))
    }

    pub fn rebuild_scene(&mut self) -> Result<(), EmoteRuntimeError> {
        self.rebuild_scene_with_physics(0.0)
    }

    fn rebuild_scene_with_physics(
        &mut self,
        physics_delta_ticks: f32,
    ) -> Result<(), EmoteRuntimeError> {
        let Some(motion) = self.active_motion.as_deref() else {
            return Ok(());
        };
        // Preserve the complete pre-tick StepFrame snapshot across BOTH scene
        // builds below. Native needs previous finalized XYZ for nested-motion
        // dt=2 and persistent local frame values for serialized type-0 HOLD.
        // The second rebuild only reflects physics variables at the same motion
        // time and must not treat the first rebuild as a new native frame.
        let previous_scene = self.player.scene().clone();
        let variables = self.player.evaluated_variable_values();
        let scene = self
            .schema
            .build_motion_scene_at_with_resources_variables_previous_scene_and_ground_hook(
                &self.psb,
                &self.normalized_data,
                motion,
                self.player.elapsed_ticks(),
                &variables,
                &previous_scene,
                self.ground_correction_hook,
            )?;
        self.player.replace_scene(scene);
        if physics_delta_ticks > 0.0 && self.player.is_physics_enabled() {
            self.player
                .evaluate_physics_for_current_scene(physics_delta_ticks);
            let variables = self.player.evaluated_variable_values();
            let scene = self
                .schema
                .build_motion_scene_at_with_resources_variables_previous_scene_and_ground_hook(
                    &self.psb,
                    &self.normalized_data,
                    motion,
                    self.player.elapsed_ticks(),
                    &variables,
                    &previous_scene,
                    self.ground_correction_hook,
                )?;
            self.player.replace_scene(scene);
        }
        Ok(())
    }

    /// Installs the host geometry callback used by native groundCorrection.
    /// The DLL delegates this query outside MMotionPlayer as well; keeping it
    /// injectable avoids hard-coding game-specific collision geometry here.
    pub fn set_ground_correction_hook(
        &mut self,
        hook: Option<EmoteGroundCorrectionHook>,
    ) -> Result<(), EmoteRuntimeError> {
        self.ground_correction_hook = hook;
        self.rebuild_scene()
    }

    pub fn configure_transform(
        &mut self,
        mode: EmoteTransformMode,
    ) -> Result<(), EmoteRuntimeError> {
        self.set_transform_order_mask(mode.transform_order_mask())
    }

    pub fn apply_new_options(&mut self, options: EmoteNewOptions) -> Result<(), EmoteRuntimeError> {
        self.name = (!options.name.is_empty()).then_some(options.name);
        self.jname = options.jname;
        self.layer = options.layer;
        self.page = options.page;
        self.zindex = options.zindex;
        self.reset_motion_on_hide = options.reset_motion_on_hide;
        self.player.set_scale(options.scale);
        self.player.set_coord(options.x, options.y);
        self.player.set_grayscale(options.grayscale, 0.0, 0.0);
        self.player
            .set_color_rgba(0xff00_0000 | (options.color_rgb & 0x00ff_ffff), 0.0, 0.0);
        self.player
            .set_mesh_division_ratio(options.mesh_division_ratio);
        self.player.set_physics_enabled(options.physics);
        self.rebuild_scene()
    }

    pub fn emote_show(&mut self, options: EmoteShowOptions) -> Result<(), EmoteRuntimeError> {
        if let Some(scale) = options.scale {
            self.player.set_scale(scale);
        }
        if options.x.is_some() || options.y.is_some() {
            let current = self.player.coord();
            self.player.set_coord(
                options.x.unwrap_or(current[0]),
                options.y.unwrap_or(current[1]),
            );
        }
        let ticks = milliseconds_to_emote_ticks(options.time_ms.max(0.0));
        if let Some(grayscale) = options.grayscale {
            self.player.set_grayscale(grayscale, ticks, 0.0);
        }
        if let Some(color) = options.color_rgb {
            self.player
                .set_color_rgba(0xff00_0000 | (color & 0x00ff_ffff), ticks, 0.0);
        }
        if let Some(zindex) = options.zindex {
            self.zindex = zindex;
        }
        let _wait = options.wait;
        self.player.show();
        self.rebuild_scene()
    }

    pub fn emote_hide(&mut self, options: EmoteHideOptions) -> Result<(), EmoteRuntimeError> {
        let _ticks = milliseconds_to_emote_ticks(options.time_ms.max(0.0));
        let _wait = options.wait;
        self.player.hide();
        if let Some(reset_motion) = self.reset_motion_on_hide.clone() {
            if self.player.timelines().contains_key(&reset_motion) {
                self.player.stop_timeline("");
                self.player
                    .play_timeline(&reset_motion, TimelinePlayMode::ONCE);
                self.diff_slots = Default::default();
            }
        }
        self.rebuild_scene()
    }

    pub fn emote_motion(
        &mut self,
        motion: Option<&str>,
        slot: Option<usize>,
    ) -> Result<(), EmoteRuntimeError> {
        match slot {
            None | Some(0) => {
                if let Some(name) = motion {
                    self.play_timeline(name, TimelinePlayMode::ONCE)?;
                } else {
                    let active: Vec<String> = self
                        .player
                        .active_timelines()
                        .iter()
                        .filter(|(_, mode)| !mode.is_difference())
                        .map(|(name, _)| name.clone())
                        .collect();
                    for name in active {
                        self.player.stop_timeline(&name);
                    }
                    self.rebuild_scene()?;
                }
            }
            Some(slot @ 1..=6) => {
                let index = slot - 1;
                if let Some(previous) = self.diff_slots[index].take() {
                    self.player.stop_timeline(&previous);
                }
                if let Some(name) = motion {
                    if !self.player.timelines().contains_key(name) {
                        return Err(EmoteRuntimeError::MissingTimeline(name.to_owned()));
                    }
                    self.player
                        .play_timeline(name, TimelinePlayMode::PARALLEL_DIFFERENCE);
                    self.diff_slots[index] = Some(name.to_owned());
                    self.rebuild_scene()?;
                }
            }
            Some(slot) => return Err(EmoteRuntimeError::InvalidMotionSlot(slot)),
        }
        Ok(())
    }

    pub fn emote_variable(
        &mut self,
        variable: &str,
        value: f32,
        time_ms: f32,
    ) -> Result<(), EmoteRuntimeError> {
        let ticks = milliseconds_to_emote_ticks(time_ms.max(0.0));
        self.player.set_variable_timed(variable, value, ticks, 0.0);
        self.rebuild_scene()
    }

    pub fn emote_trans(&mut self, options: EmoteTransOptions) -> Result<(), EmoteRuntimeError> {
        let ticks = milliseconds_to_emote_ticks(options.time_ms.max(0.0));
        if let Some(scale) = options.scale {
            self.player.set_scale(scale);
        }
        if options.x.is_some() || options.y.is_some() {
            let current = self.player.coord();
            self.player.set_coord(
                options.x.unwrap_or(current[0]),
                options.y.unwrap_or(current[1]),
            );
        }
        if let Some(grayscale) = options.grayscale {
            self.player.set_grayscale(grayscale, ticks, 0.0);
        }
        if let Some(color) = options.color_rgb {
            self.player
                .set_color_rgba(0xff00_0000 | (color & 0x00ff_ffff), ticks, 0.0);
        }
        if let Some(zindex) = options.zindex {
            self.zindex = zindex;
        }
        let _wait = options.wait;
        self.rebuild_scene()
    }

    pub fn show(&mut self) {
        self.player.show();
    }

    pub fn hide(&mut self) {
        self.player.hide();
    }

    pub fn set_smoothing(&mut self, state: bool) {
        self.player.set_smoothing(state);
    }
    pub fn smoothing(&self) -> bool {
        self.player.smoothing()
    }

    pub fn set_mesh_division_ratio(&mut self, ratio: f32) {
        self.player.set_mesh_division_ratio(ratio);
    }
    pub fn mesh_division_ratio(&self) -> f32 {
        self.player.mesh_division_ratio()
    }

    pub fn set_queuing(&mut self, state: bool) {
        self.player.set_queuing(state);
    }
    pub fn queuing(&self) -> bool {
        self.player.queuing()
    }

    pub fn set_color_rgba(&mut self, rgba: u32, frame_count: f32, easing: f32) {
        self.player.set_color_rgba(rgba, frame_count, easing);
    }
    pub fn color_rgba(&self) -> u32 {
        self.player.color_rgba()
    }

    pub fn set_grayscale(&mut self, rate: f32, frame_count: f32, easing: f32) {
        self.player.set_grayscale(rate, frame_count, easing);
    }
    pub fn grayscale(&self) -> f32 {
        self.player.grayscale()
    }

    pub fn set_as_original_scale(&mut self, state: bool) {
        self.player.set_as_original_scale(state);
    }
    pub fn as_original_scale(&self) -> bool {
        self.player.as_original_scale()
    }

    pub fn state_value(&self, label: &str) -> f32 {
        self.player.state_value(label)
    }

    pub fn set_variable(&mut self, name: &str, value: f32) {
        self.player.set_variable(name, value);
    }

    pub fn set_variable_immediate(
        &mut self,
        name: &str,
        value: f32,
    ) -> Result<(), EmoteRuntimeError> {
        self.player.set_variable_immediate(name, value);
        self.rebuild_scene()
    }

    pub fn set_variable_timed(&mut self, name: &str, value: f32, time_ticks: f32, easing: f32) {
        self.player
            .set_variable_timed(name, value, time_ticks, easing);
    }

    pub fn set_variable_diff(
        &mut self,
        module: &str,
        name: &str,
        value: f32,
        time_ticks: f32,
        easing: f32,
    ) -> Result<(), EmoteRuntimeError> {
        self.player
            .set_variable_diff(module, name, value, time_ticks, easing);
        self.rebuild_scene()
    }

    pub fn play_timeline(
        &mut self,
        name: &str,
        mode: TimelinePlayMode,
    ) -> Result<(), EmoteRuntimeError> {
        if !self.player.timelines().contains_key(name) {
            return Err(EmoteRuntimeError::MissingTimeline(name.to_owned()));
        }
        self.player.play_timeline(name, mode);
        self.rebuild_scene()
    }

    pub fn fade_out_timeline(
        &mut self,
        name: &str,
        time_ticks: f32,
        easing: f32,
    ) -> Result<(), EmoteRuntimeError> {
        self.player.fade_out_timeline(name, time_ticks, easing);
        self.rebuild_scene()
    }

    pub fn fade_in_timeline(
        &mut self,
        name: &str,
        time_ticks: f32,
        easing: f32,
    ) -> Result<(), EmoteRuntimeError> {
        self.player.fade_in_timeline(name, time_ticks, easing);
        self.rebuild_scene()
    }

    pub fn set_timeline_blend_ratio(
        &mut self,
        name: &str,
        value: f32,
        time_ticks: f32,
        easing: f32,
        stop_when_done: bool,
    ) -> Result<(), EmoteRuntimeError> {
        self.player
            .set_timeline_blend_ratio(name, value, time_ticks, easing, stop_when_done);
        self.rebuild_scene()
    }

    pub fn set_timeline_time(&mut self, name: &str, ticks: f32) -> Result<(), EmoteRuntimeError> {
        self.player
            .set_timeline_time(name, ticks)
            .map_err(EmoteRuntimeError::RuntimeControl)?;
        self.rebuild_scene()
    }

    pub fn stop_timeline(&mut self, name: &str) -> Result<(), EmoteRuntimeError> {
        self.player.stop_timeline(name);
        self.rebuild_scene()
    }

    pub fn main_timeline_labels(&self) -> Vec<&str> {
        self.player.main_timeline_labels()
    }

    pub fn diff_timeline_labels(&self) -> Vec<&str> {
        self.player.diff_timeline_labels()
    }

    pub fn playing_timeline_info(&self) -> Vec<(String, u32)> {
        self.player.playing_timeline_info()
    }

    pub fn is_timeline_playing(&self, label: &str) -> bool {
        self.player.is_timeline_playing(label)
    }

    pub fn is_loop_timeline(&self, label: &str) -> bool {
        self.player.is_loop_timeline(label)
    }

    pub fn timeline_total_frame_count(&self, label: &str) -> Option<f32> {
        self.player.timeline_total_frame_count(label)
    }

    pub fn timeline_blend_ratio(&self, label: &str) -> f32 {
        self.player.timeline_blend_ratio(label)
    }

    pub fn pause(&mut self) {
        self.player.set_paused(true);
    }

    pub fn play(&mut self) {
        self.player.set_paused(false);
    }

    pub fn pass(&mut self) -> Result<(), EmoteRuntimeError> {
        self.player.pass();
        self.rebuild_scene()
    }

    pub fn step(&mut self) -> Result<(), EmoteRuntimeError> {
        self.player.step();
        self.rebuild_scene()
    }

    pub fn is_animating(&self) -> bool {
        self.player.is_animating()
    }

    pub fn is_modified(&self) -> bool {
        self.player.is_modified()
    }

    pub fn clear_modified(&mut self) {
        self.player.clear_modified();
    }

    pub fn set_selector_option(
        &mut self,
        label: &str,
        option_index: usize,
    ) -> Result<(), EmoteRuntimeError> {
        self.player
            .set_selector_option(label, option_index)
            .map_err(EmoteRuntimeError::RuntimeControl)?;
        self.rebuild_scene()
    }

    pub fn reset_all_variables(&mut self) -> Result<(), EmoteRuntimeError> {
        self.player.reset_all_variables_to_default();
        self.rebuild_scene()
    }

    pub fn set_physics_enabled(&mut self, enabled: bool) {
        self.player.set_physics_enabled(enabled);
    }

    pub fn reset_physics(&mut self) -> Result<(), EmoteRuntimeError> {
        self.player.reset_physics();
        self.rebuild_scene()
    }

    pub fn set_coord(&mut self, x: f32, y: f32) {
        self.player.set_coord(x, y);
    }

    pub fn set_scale(&mut self, scale: f32) {
        self.player.set_scale(scale);
    }

    pub fn set_rot(&mut self, rot: f32) {
        self.player.set_rot(rot);
    }

    pub fn set_outer_force(
        &mut self,
        label: &str,
        x: f32,
        y: f32,
        time_ticks: f32,
        easing: f32,
    ) -> Result<(), EmoteRuntimeError> {
        self.player.set_outer_force(label, x, y, time_ticks, easing);
        self.rebuild_scene()
    }

    pub fn set_outer_rot(
        &mut self,
        rot: f32,
        time_ticks: f32,
        easing: f32,
    ) -> Result<(), EmoteRuntimeError> {
        self.player.set_outer_rot(rot, time_ticks, easing);
        self.rebuild_scene()
    }

    pub fn start_wind(&mut self, start: f32, goal: f32, speed: f32, pow_min: f32, pow_max: f32) {
        self.player.start_wind(start, goal, speed, pow_min, pow_max);
    }

    pub fn stop_wind(&mut self) {
        self.player.stop_wind();
    }

    pub fn set_transform_order_mask(&mut self, mask: u32) -> Result<(), EmoteRuntimeError> {
        self.player.set_transform_order_mask(mask);
        self.rebuild_scene()
    }

    pub fn set_hair_scale(&mut self, scale: f32) -> Result<(), EmoteRuntimeError> {
        self.player.set_hair_scale(scale);
        self.rebuild_scene()
    }

    pub fn set_parts_scale(&mut self, scale: f32) -> Result<(), EmoteRuntimeError> {
        self.player.set_parts_scale(scale);
        self.rebuild_scene()
    }

    pub fn set_bust_scale(&mut self, scale: f32) -> Result<(), EmoteRuntimeError> {
        self.player.set_bust_scale(scale);
        self.rebuild_scene()
    }

    pub fn motion_names(&self) -> Result<Vec<String>, EmoteRuntimeError> {
        Ok(self
            .schema
            .motion_infos(&self.psb)?
            .into_iter()
            .map(|motion| motion.name)
            .collect())
    }

    pub fn motion_infos(&self) -> Result<Vec<EmoteMotionInfo>, EmoteRuntimeError> {
        Ok(self.schema.motion_infos(&self.psb)?)
    }

    pub fn active_motion(&self) -> Option<&str> {
        self.active_motion.as_deref()
    }

    pub fn set_motion(&mut self, motion: impl Into<String>) -> Result<(), EmoteRuntimeError> {
        self.active_motion = Some(motion.into());
        self.player.skip();
        self.rebuild_scene()
    }

    pub fn timeline_names(&self) -> Vec<&str> {
        self.player.timelines().keys().map(String::as_str).collect()
    }

    pub fn variable_names(&self) -> Vec<&str> {
        self.player.variables().keys().map(String::as_str).collect()
    }

    pub fn variable_value(&self, name: &str) -> Option<f32> {
        self.player.variable_value(name)
    }

    pub fn variable_frame_count(&self, name: &str) -> usize {
        self.player.variable_frame_count(name)
    }

    pub fn variable_frame_label_at(&self, name: &str, frame_index: usize) -> Option<&str> {
        self.player.variable_frame_label_at(name, frame_index)
    }

    pub fn variable_frame_value_at(&self, name: &str, frame_index: usize) -> Option<f32> {
        self.player.variable_frame_value_at(name, frame_index)
    }

    pub fn get_variable_diff(&self, module: &str, name: &str) -> Option<f32> {
        self.player.variable_diff_value(module, name)
    }

    pub fn variable_values(&self) -> BTreeMap<String, f32> {
        self.player
            .variables()
            .iter()
            .map(|(name, state)| (name.clone(), state.value))
            .collect()
    }

    pub fn scene(&self) -> &EmoteStaticScene {
        self.player.scene()
    }

    pub fn sprites(&self) -> &[EmoteStaticSprite] {
        &self.player.scene().sprites
    }

    pub fn draw_frame_info(&self) -> &[EmoteDrawFrameInfo] {
        &self.player.scene().draw_frame_info
    }

    pub fn bounds(&self) -> Option<EmoteSceneBounds> {
        self.player.bounds()
    }

    pub fn texture_sources(&self) -> &BTreeMap<String, EmoteTextureSource> {
        &self.schema.textures
    }

    pub fn stereovision_control(&self) -> Option<&crate::emote::EmoteStereovisionControl> {
        self.schema.stereovision.as_ref()
    }

    pub fn stereovision_profile(&self) -> Option<&crate::emote::EmoteStereovisionProfile> {
        self.schema.stereovision_profile.as_ref()
    }

    pub fn runtime_mirror_enabled(&self) -> bool {
        self.player.runtime_mirror_enabled()
    }

    pub fn active_mirror_enabled(&self) -> bool {
        self.player.active_mirror_enabled()
    }

    pub fn set_runtime_mirror_enabled(
        &mut self,
        enabled: bool,
    ) -> Result<(), EmoteRuntimeError> {
        self.player.set_runtime_mirror_enabled(enabled);
        self.rebuild_scene()
    }

    pub fn stereovision_enabled(&self) -> bool {
        self.player.stereovision_enabled()
    }

    pub fn set_stereovision_enabled(
        &mut self,
        enabled: bool,
    ) -> Result<(), EmoteRuntimeError> {
        self.player.set_stereovision_enabled(enabled);
        self.rebuild_scene()
    }

    pub fn stereovision_level(&self) -> f32 {
        self.player.stereovision_level()
    }

    pub fn set_stereovision_level(&mut self, level: f32) -> Result<(), EmoteRuntimeError> {
        self.player.set_stereovision_level(level);
        self.rebuild_scene()
    }

    pub fn stereovision_fov(&self) -> f32 {
        self.player.stereovision_fov()
    }

    pub fn set_stereovision_fov(&mut self, fov: f32) -> Result<(), EmoteRuntimeError> {
        self.player.set_stereovision_fov(fov);
        self.rebuild_scene()
    }

    pub fn stereovision_screen_index(&self) -> usize {
        self.player.stereovision_screen_index()
    }

    pub fn set_stereovision_screen_index(
        &mut self,
        screen_index: usize,
    ) -> Result<(), EmoteRuntimeError> {
        self.player.set_stereovision_screen_index(screen_index);
        self.rebuild_scene()
    }

    pub fn stereovision_screen_count(&self) -> usize {
        self.player.stereovision_screen_count()
    }

    pub fn set_stereovision_screen_count(
        &mut self,
        screen_count: usize,
    ) -> Result<(), EmoteRuntimeError> {
        self.player.set_stereovision_screen_count(screen_count);
        self.rebuild_scene()
    }

    pub fn evaluated_variable_values_for_stereovision_screen(
        &self,
        screen_index: usize,
    ) -> BTreeMap<String, f32> {
        self.player.evaluated_variable_values_for_screen(screen_index)
    }

    pub fn stereovision_screens_for_variable(
        &self,
        variable_name: &str,
    ) -> Option<Vec<crate::runtime::EmoteStereovisionScreen>> {
        self.player.stereovision_screens_for_variable(variable_name)
    }

    pub fn camera_runtime(&self) -> Option<&crate::emote::EmoteCameraRuntimeState> {
        self.player.scene().camera_runtime.as_ref()
    }

    /// Camera state for every MMotionPlayer scope, including nested type-3
    /// and particle child players. Keys are flattened scope-root layer paths.
    pub fn camera_runtimes(
        &self,
    ) -> &BTreeMap<String, crate::emote::EmoteCameraRuntimeState> {
        &self.player.scene().camera_runtimes
    }

    pub fn camera_runtime_for_scope(
        &self,
        scope_root_path: &str,
    ) -> Option<&crate::emote::EmoteCameraRuntimeState> {
        self.player.scene().camera_runtimes.get(scope_root_path)
    }

    pub fn texture_bytes(&self, resource_index: u32) -> Option<&[u8]> {
        self.psb
            .resource_bytes(&self.normalized_data, resource_index as usize)
    }

    pub fn runtime_pipeline(&self) -> &EmoteRuntimePipeline {
        self.player.runtime_pipeline()
    }

    pub fn start_record_api_log(&mut self) {
        self.player.start_record_api_log();
    }

    pub fn stop_record_api_log(&mut self) {
        self.player.stop_record_api_log();
    }

    pub fn is_recording_api_log(&self) -> bool {
        self.player.is_recording_api_log()
    }

    pub fn start_replay_api_log(&mut self) {
        self.player.start_replay_api_log();
    }

    pub fn stop_replay_api_log(&mut self) {
        self.player.stop_replay_api_log();
    }

    pub fn is_replaying_api_log(&self) -> bool {
        self.player.is_replaying_api_log()
    }

    pub fn clear_api_log(&mut self) {
        self.player.clear_api_log();
    }

    pub fn api_log(&self) -> String {
        self.player.api_log()
    }

    pub fn set_api_log(&mut self, log: &str) {
        self.player.set_api_log(log);
    }

    pub fn replay_api_log_once(&mut self) -> Result<(), EmoteRuntimeError> {
        self.player.replay_api_log_once();
        self.rebuild_scene()
    }

    pub fn is_chara_profile_available(&self) -> bool {
        !self.chara_profile_labels().is_empty()
    }

    pub fn chara_profile_labels(&self) -> Vec<String> {
        collect_chara_profiles(&self.psb)
            .into_iter()
            .map(|profile| profile.label)
            .collect()
    }

    pub fn chara_profile_value(&self, label: &str) -> Option<f32> {
        collect_chara_profiles(&self.psb)
            .into_iter()
            .find(|profile| profile.label == label)
            .map(|profile| profile.value)
    }

    pub fn chara_height(&self) -> Option<f32> {
        self.chara_profile_value("height")
            .or_else(|| self.chara_profile_value("身長"))
            .or_else(|| self.chara_profile_value("charaHeight"))
    }

    pub fn inner_player(&self) -> &ElunaPlayer {
        &self.player
    }

    pub fn inner_player_mut(&mut self) -> &mut ElunaPlayer {
        &mut self.player
    }
}

fn find_timeline_for_motion(player: &ElunaPlayer, motion: &str) -> Option<String> {
    if player.timelines().contains_key(motion) {
        return Some(motion.to_owned());
    }
    let suffix = format!("/{motion}");
    player
        .timelines()
        .values()
        .filter(|timeline| !timeline.is_difference)
        .map(|timeline| &timeline.name)
        .find(|name| name.ends_with(&suffix))
        .cloned()
}

fn initial_variable_values(
    infos: &[crate::EmoteVariableInfo],
) -> BTreeMap<String, f32> {
    infos
        .iter()
        .map(|info| (info.name.clone(), info.default_value))
        .collect()
}

fn collect_chara_profiles(psb: &PsbFile) -> Vec<crate::EmoteCharaProfileInfo> {
    let mut out = BTreeMap::<String, f32>::new();
    if let Some(metadata) = psb.root.field("metadata") {
        collect_chara_profile_nodes(metadata.field("charaProfile"), &mut out);
    }
    collect_chara_profile_nodes(psb.root.field("charaProfile"), &mut out);
    out.into_iter()
        .map(|(label, value)| crate::EmoteCharaProfileInfo { label, value })
        .collect()
}

fn collect_chara_profile_nodes(value: Option<&PsbValue>, out: &mut BTreeMap<String, f32>) {
    let Some(value) = value else {
        return;
    };
    match value {
        PsbValue::List(items) => {
            for item in items {
                collect_chara_profile_nodes(Some(item), out);
            }
        }
        PsbValue::Object(fields) => {
            if let Some(label) = value
                .field_str("label")
                .or_else(|| value.field_str("name"))
                .or_else(|| value.field_str("id"))
            {
                if let Some(v) = value
                    .field_f32("value")
                    .or_else(|| value.field_f32("height"))
                    .or_else(|| value.field_f32("val"))
                {
                    out.insert(label.to_owned(), v);
                }
            }
            for (key, child) in fields {
                if let Some(v) = child.as_f32() {
                    out.insert(key.clone(), v);
                } else {
                    collect_chara_profile_nodes(Some(child), out);
                }
            }
        }
        _ => {}
    }
}
