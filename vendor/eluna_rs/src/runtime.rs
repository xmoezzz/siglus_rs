//! Runtime-facing Emote player state.
//!
//! This module keeps the public player/control surface alive while the original
//! mesh deformation path is still being recovered from `StepFrameMeshChain`.
//! Variable writes are accepted, queued, progressed, and queryable. Timeline
//! variable tracks from `metadata/timelineControl` are evaluated every frame and
//! feed the same variable map used by the renderer-side mesh deformation path.

use crate::api::{transform_order_mask, EmotePlayerControl, TimelinePlayMode, VariableWrite};
use crate::{
    load_emote_static_scene, EmoteSceneBounds, EmoteSchemaError, EmoteStaticScene,
    EmoteStereovisionControl, PsbFile, PsbValue,
};
use std::collections::{BTreeMap, VecDeque};

const PHYSICS_MAX_SUBSTEP_TICKS: f32 = 1.0;
const PHYSICS_EPSILON_TICKS: f32 = 0.00000011920929;
// sub_10268A30 clamps each controller-loop iteration with std::min(remaining, 1.1).
const CONTROL_STEP_CAP_TICKS: f32 = 1.1;
const PEND_BEND_POWER_STEP: f32 = 0.03125;
const PEND_BEND_TRIGGER_VALUE: f32 = 28.0;
const TAU: f32 = std::f32::consts::PI * 2.0;

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteVariableFrameInfo {
    pub label: String,
    pub value: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteVariableInfo {
    pub name: String,
    pub default_value: f32,
    pub min_value: Option<f32>,
    pub max_value: Option<f32>,
    pub frames: Vec<EmoteVariableFrameInfo>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteVariableState {
    pub info: EmoteVariableInfo,
    pub value: f32,
    pub target: Option<EmoteVariableTarget>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteVariableTarget {
    pub start_value: f32,
    pub target_value: f32,
    pub elapsed_ticks: f32,
    pub duration_ticks: f32,
    pub easing: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteTimelineFrame {
    pub time_ticks: f32,
    /// Native TimelineVariableFrame::type == 0.  Such a frame is a no-write
    /// marker: it advances the timeline cursor without issuing a variable
    /// command.  Keeping this bit is essential because timeline tracks are
    /// command streams, not stateless interpolation curves.
    pub hold: bool,
    pub value: f32,
    pub easing: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteTimelineVariable {
    pub name: String,
    pub frames: Vec<EmoteTimelineFrame>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteTimeline {
    pub name: String,
    pub path: Option<String>,
    /// Authored loop start.  Native Timeline::loopBegin < 0 means one-shot.
    pub loop_begin_ticks: f32,
    /// Authored loop end.  When looping, the native player first advances to
    /// this boundary, seeks to loop_begin, then consumes the residual delta.
    pub loop_end_ticks: f32,
    /// Native Timeline::lastTime.
    pub last_time_ticks: f32,
    /// Public compatibility alias for last_time_ticks.
    pub duration_ticks: f32,
    pub variables: Vec<EmoteTimelineVariable>,
    pub is_difference: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct ActiveTimelineState {
    mode: TimelinePlayMode,
    elapsed_ticks: f32,
    /// Native TimelineVariable keeps one current frame cursor per variable.
    frame_indices: Vec<usize>,
    /// Timeline runtime +36. sub_10270340 initializes this to 1.0.
    blend_ratio: f32,
    blend_target: Option<TimelineBlendTarget>,
    /// EPTransitionControl queues SetTimelineBlendRatio commands when the
    /// player's global queuing flag is enabled.
    blend_queue: VecDeque<TimelineBlendCommand>,
    /// Timeline runtime +40. This is independent from the transition target:
    /// sub_10275350 removes the runtime once the blend transition becomes idle.
    stop_when_blend_idle: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct TimelineBlendCommand {
    target_value: f32,
    duration_ticks: f32,
    easing: f32,
}

#[derive(Debug, Clone, PartialEq)]
struct TimelineBlendTarget {
    start_value: f32,
    target_value: f32,
    elapsed_ticks: f32,
    duration_ticks: f32,
    easing: f32,
}

#[derive(Debug, Clone, PartialEq)]
struct Vector2TransitionCommand {
    target: [f32; 2],
    duration_ticks: f32,
    easing_exponent: f32,
}

/// Two-channel native transition object recovered from sub_10218860 /
/// sub_102164E0.  MEmotePlayer owns one for each outer-force label
/// (bust/hair/parts).
#[derive(Debug, Clone, PartialEq)]
struct Vector2TransitionState {
    current: [f32; 2],
    start: [f32; 2],
    target: [f32; 2],
    active: bool,
    inv_duration: f32,
    easing_exponent: f32,
    progress: f32,
    queue: VecDeque<Vector2TransitionCommand>,
}

impl Default for Vector2TransitionState {
    fn default() -> Self {
        Self {
            current: [0.0, 0.0],
            start: [0.0, 0.0],
            target: [0.0, 0.0],
            active: false,
            inv_duration: 0.0,
            easing_exponent: 1.0,
            progress: 0.0,
            queue: VecDeque::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteApiLogEntry {
    pub command: String,
    pub args: Vec<String>,
}

impl EmoteApiLogEntry {
    fn new(command: impl Into<String>, args: Vec<String>) -> Self {
        Self {
            command: command.into(),
            args,
        }
    }

    fn encode(&self) -> String {
        if self.args.is_empty() {
            self.command.clone()
        } else {
            format!("{}\t{}", self.command, self.args.join("\t"))
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteCharaProfileInfo {
    pub label: String,
    pub value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WindPulse {
    pub active: bool,
    pub position: f32,
    pub power: f32,
}

impl Default for WindPulse {
    fn default() -> Self {
        Self {
            active: false,
            position: 0.0,
            power: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindState {
    pub start: f32,
    pub goal: f32,
    /// Positive API speed.  The native EPWindControl stores direction
    /// separately by applying sign(goal-start) to this value.
    pub speed: f32,
    pub pow_min: f32,
    pub pow_max: f32,
    pub elapsed_ticks: f32,
    /// Native +28 accumulator.  EPWindControl attempts one pulse spawn each
    /// time this crosses zero, then subtracts exactly one tick.
    pub spawn_accumulator: f32,
    pub signed_speed: f32,
    /// Recovered EPWindControl storage: 128 entries of {active,pos,power}.
    pub pulses: [WindPulse; 128],
}

/// Runtime state for one EPBustControl spring (one entry per bustControl item).
///
/// The original control keeps a root point, a bob point, a velocity vector,
/// and a root-to-target offset.  The group updater interpolates the target
/// baseLayer position over fixed substeps before calling EPBustControl::step.
#[derive(Debug, Clone, PartialEq)]
pub struct BustPhysicsState {
    /// EPBustControl root point, original object +28/+32/+36. `param.op`
    /// initializes this absolute physics-space point; it is not a baseLayer
    /// offset. After the first controller step only X/Y follow the anchor.
    pub root: [f32; 3],
    /// Current bob position, corresponding to the original object at +52.
    pub bob: [f32; 3],
    /// Current bob velocity, corresponding to the original object at +64.
    pub vel: [f32; 3],
    /// Y rest/bias term used by the var_ud output.
    pub ofs: f32,
    /// MEmotePlayer bust-entry first-frame flag (entry +4 in sub_10273DA0).
    /// This is separate from EPBustControl's own reset flag.
    pub group_first_tick: bool,
    /// EPBustControl first-step flag (object +24 in sub_101D4300).
    pub controller_first_tick: bool,
    /// Native +40/+44: root-to-input-anchor delta captured on the first
    /// controller step and preserved while the baseLayer anchor moves.
    pub root_delta: [f32; 2],
    /// Previous baseLayer anchor *without* OuterForce. The native group
    /// updater interpolates from this point to current_anchor + OuterForce.
    pub last_anchor: Option<[f32; 3]>,
}

/// Runtime state for one EPPendControl two-segment pendulum.
///
/// The original control owns a root, two rest points, two current bob points,
/// two velocities, and a bend oscillator.  Hair and parts controls both use
/// this state type.
#[derive(Debug, Clone, PartialEq)]
pub struct HairPhysicsState {
    /// Current bob position for the two segments, original +112 and +124.
    pub bob: [[f32; 3]; 2],
    /// Current velocity for the two segments, original +136 and +148.
    pub vel: [[f32; 3]; 2],
    /// Y rest/bias term used by var_ud.
    pub ofs: f32,
    /// Set on the first group update.
    pub first_tick: bool,
    /// Original root offset: root = target_baseLayer + root_offset.
    pub root_offset: [f32; 3],
    /// Previous baseLayer target, used for the original interpolated substep loop.
    pub last_anchor: Option<[f32; 3]>,
    /// Bend oscillator phase, original field +164.
    pub bend_phase: f32,
    /// Bend oscillator power, original field +168.
    pub bend_power: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ElunaPlayer {
    shown: bool,
    smoothing: bool,
    mesh_division_ratio: f32,
    queuing: bool,
    color_rgba: u32,
    grayscale: f32,
    as_original_scale: bool,
    coord: [f32; 2],
    scale: f32,
    rot: f32,
    elapsed_ticks: f32,
    pub paused: bool,
    pub physics_enabled: bool,
    scene: EmoteStaticScene,
    variables: BTreeMap<String, EmoteVariableState>,
    timelines: BTreeMap<String, EmoteTimeline>,
    pending_writes: Vec<VariableWrite>,
    active_timelines: BTreeMap<String, TimelinePlayMode>,
    active_timeline_states: BTreeMap<String, ActiveTimelineState>,
    timeline_diff_variables: BTreeMap<String, BTreeMap<String, EmoteVariableState>>,
    timeline_blend_ratios: BTreeMap<String, f32>,
    /// Values of physics-output variables immediately before the native
    /// post-control physics stage ran on the most recent Progress/Pass.  The
    /// DLL applies Difference -> controller chain -> Mirror/Clamp -> physics;
    /// keeping this small snapshot lets evaluated_variable_states reconstruct
    /// that order even though solver outputs remain queryable in `variables`.
    pre_physics_output_values: BTreeMap<String, f32>,
    runtime_pipeline: EmoteRuntimePipeline,
    eye_states: Vec<EyeControlState>,
    eyebrow_states: Vec<EyebrowControlState>,
    mouth_states: Vec<MouthControlState>,
    selector_states: Vec<SelectorControlState>,
    transition_states: Vec<ScalarTransitionState>,
    loop_states: Vec<LoopControlState>,
    bust_states: Vec<BustPhysicsState>,
    hair_states: Vec<HairPhysicsState>,
    outer_force_states: BTreeMap<String, Vector2TransitionState>,
    /// Current EPRotateControl value in degrees.
    outer_rot: f32,
    outer_rot_target: Option<EmoteVariableTarget>,
    transform_order_mask: u32,
    hair_scale: f32,
    parts_scale: f32,
    bust_scale: f32,
    wind: Option<WindState>,
    /// Native +389. Runtime/user mirror toggle. The active mirror byte (+388)
    /// is this value XOR metadata.mirror (+390).
    runtime_mirror_enabled: bool,
    /// Native +492. Disabled by default and explicitly enabled by the host.
    stereovision_enabled: bool,
    /// Native +496, default 1.0. Multiplied by camera stereovision depth.
    stereovision_level: f32,
    /// Native +500 (`fov`), default 0.2; Camera StepFrame overwrites this value.
    stereovision_fov: f32,
    /// Native +504. Kept as the host-visible current screen selector.
    stereovision_screen_index: usize,
    /// Native +508, initialized to two screens in this driver build.
    stereovision_screen_count: usize,
    modified: bool,
    recording_api_log: bool,
    replaying_api_log: bool,
    api_log: Vec<EmoteApiLogEntry>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EmoteRuntimePipeline {
    pub instant_variables: Vec<String>,
    /// metadata.mirror (+390). The player combines this with its runtime/user
    /// mirror toggle (+389) to recover the active mirror byte (+388).
    pub mirror_enabled: bool,
    pub selector_controls: Vec<SelectorControl>,
    pub clamp_controls: Vec<ClampControl>,
    pub loop_controls: Vec<LoopControl>,
    pub mirror_control: Option<MirrorControl>,
    /// metadata.stereovisionControl, parsed in the same native Init stage as
    /// timelineControl/mirrorControl.
    pub stereovision_control: Option<EmoteStereovisionControl>,
    pub transition_controls: Vec<TransitionControl>,
    pub physics_controls: Vec<PhysicsControl>,
    pub parts_controls: Vec<OpaqueControl>,
    pub eye_controls: Vec<EyeControl>,
    pub eyebrow_controls: Vec<EyebrowControl>,
    pub mouth_controls: Vec<MouthControl>,
    pub unsupported_fields: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectorControl {
    pub label: String,
    pub enabled: bool,
    pub option_list: Vec<SelectorOption>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SelectorOption {
    pub label: String,
    pub off_value: f32,
    pub on_value: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClampControl {
    pub label: String,
    pub enabled: bool,
    pub kind: i64,
    pub var_lr: String,
    pub var_ud: String,
    pub min: f32,
    pub max: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoopControl {
    pub label: Option<String>,
    pub enabled: bool,
    pub var_loop: Option<String>,
    pub transition_list: Vec<LoopTransition>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LoopTransition {
    pub start: f32,
    pub end: f32,
    pub duration_ticks: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MirrorControl {
    pub variable_match_list: Vec<String>,
}

/// Native MEmotePlayer::StereovisionScreen.  sub_1027A4D0 rebuilds one
/// `(slope, intercept)` pair per physical stereoscopic screen and
/// sub_10277100 applies `slope * value + intercept` on variable writes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmoteStereovisionScreen {
    pub slope: f32,
    pub intercept: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransitionControl {
    pub label: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EyeControl {
    pub label: String,
    pub enabled: bool,
    pub begin_frame: i32,
    pub end_frame: i32,
    pub blink_interval_min: f32,
    pub blink_interval_max: f32,
    pub blink_frame_count: f32,
    pub blink_enabled: bool,
    /// Native EP-eye path graph.  The graph itself is retained even though the
    /// timed shortest-path traversal is restored separately from auto blink.
    pub edge: Vec<[f32; 2]>,
    pub node: Vec<Vec<f32>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EyebrowControl {
    pub label: String,
    pub enabled: bool,
    pub begin_frame: i32,
    pub edge: Vec<[f32; 2]>,
    pub node: Vec<Vec<f32>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct MouthControl {
    pub label: String,
    pub talk_label: String,
    pub enabled: bool,
    pub begin_frame: i32,
}

#[derive(Debug, Clone, PartialEq)]
struct TimedControlCommand {
    target: f32,
    duration_ticks: f32,
    easing: f32,
}

#[derive(Debug, Clone, PartialEq)]
struct SelectorControlState {
    current: i32,
    active: Option<ActiveSelectorControl>,
    queue: VecDeque<TimedControlCommand>,
}

#[derive(Debug, Clone, PartialEq)]
struct ActiveSelectorControl {
    inv_duration: f32,
    progress: f32,
}

#[derive(Debug, Clone, PartialEq)]
struct MouthControlState {
    begin_frame: i32,
    current: f32,
    active: Option<ActiveScalarControl>,
    queue: VecDeque<TimedControlCommand>,
}

#[derive(Debug, Clone, PartialEq)]
struct ActiveScalarControl {
    start: f32,
    target: f32,
    inv_duration: f32,
    easing: f32,
    progress: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EyeBlinkState {
    Idle,
    Closing,
    ClosedHold,
    Opening,
}

#[derive(Debug, Clone, PartialEq)]
struct GraphControlCommand {
    target: f32,
    duration_ticks: f32,
    easing_exponent: f32,
}

#[derive(Debug, Clone, PartialEq)]
struct GraphControlState {
    stage: u8,
    current: f32,
    inv_duration: f32,
    direction: f32,
    segment_target: f32,
    total_distance: f32,
    traversed_distance: f32,
    easing_exponent: f32,
    route: VecDeque<[f32; 2]>,
    queue: VecDeque<GraphControlCommand>,
}

impl GraphControlState {
    fn new(begin_frame: f32) -> Self {
        Self {
            stage: 0,
            current: begin_frame,
            inv_duration: 0.0,
            direction: 1.0,
            segment_target: begin_frame,
            total_distance: 0.0,
            traversed_distance: 0.0,
            easing_exponent: 1.0,
            route: VecDeque::new(),
            queue: VecDeque::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct EyeControlState {
    /// EPEyeControl's graph-controlled eye pose (+0x6c). Auto blink overlays
    /// this value rather than replacing it.
    graph: GraphControlState,
    blink_state: EyeBlinkState,
    blink_frame: f32,
    blink_timer: f32,
}

#[derive(Debug, Clone, PartialEq)]
struct EyebrowControlState {
    graph: GraphControlState,
}

#[derive(Debug, Clone, PartialEq)]
struct ScalarTransitionState {
    current: f32,
    active: Option<ActiveScalarControl>,
    queue: VecDeque<TimedControlCommand>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LoopControlState {
    index: usize,
    elapsed_ticks: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PhysicsControl {
    Bust(PhysicsControlDefinition),
    Hair(PhysicsControlDefinition),
    Parts(PhysicsControlDefinition),
}

#[derive(Debug, Clone, PartialEq)]
pub struct PhysicsControlDefinition {
    pub label: String,
    pub enabled: bool,
    pub base_layer: Option<String>,
    pub parameter: Option<String>,
    pub var_lr: Option<String>,
    pub var_ud: Option<String>,
    pub var_lrm: Option<String>,
    pub fields: BTreeMap<String, PsbValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OpaqueControl {
    pub label: Option<String>,
    pub enabled: bool,
    pub fields: BTreeMap<String, PsbValue>,
}

impl ElunaPlayer {
    pub fn from_psb(psb: &PsbFile) -> Result<Self, EmoteSchemaError> {
        let (_schema, scene) = load_emote_static_scene(psb)?;
        Ok(Self::from_scene_variables_timelines_runtime(
            scene,
            collect_emote_variables(psb),
            collect_emote_timelines(psb),
            collect_emote_runtime_pipeline(psb),
        ))
    }

    pub fn from_scene(scene: EmoteStaticScene) -> Self {
        Self::from_scene_and_variables(scene, Vec::new())
    }

    pub fn scene(&self) -> &EmoteStaticScene {
        &self.scene
    }

    pub fn replace_scene(&mut self, scene: EmoteStaticScene) {
        self.swap_scene(scene);
    }

    /// Install a scene and retain the preceding frame without copying it.
    pub fn swap_scene(&mut self, scene: EmoteStaticScene) -> EmoteStaticScene {
        // Camera StepFrame writes the active camera fov into the player's
        // stereovision coefficient input (+500 in this driver build).
        if let Some(camera) = scene.camera_runtime.as_ref() {
            self.stereovision_fov = camera.fov;
        }
        std::mem::replace(&mut self.scene, scene)
    }

    pub fn bounds(&self) -> Option<EmoteSceneBounds> {
        self.scene.bounds
    }

    pub fn is_shown(&self) -> bool {
        self.shown
    }

    pub fn smoothing(&self) -> bool {
        self.smoothing
    }
    pub fn set_smoothing(&mut self, state: bool) {
        self.smoothing = state;
        self.modified = true;
    }

    pub fn mesh_division_ratio(&self) -> f32 {
        self.mesh_division_ratio
    }
    pub fn set_mesh_division_ratio(&mut self, ratio: f32) {
        if ratio.is_finite() && ratio > 0.0 {
            self.mesh_division_ratio = ratio;
            self.modified = true;
        }
    }

    pub fn queuing(&self) -> bool {
        self.queuing
    }
    pub fn set_queuing(&mut self, state: bool) {
        self.queuing = state;
        self.modified = true;
    }

    pub fn color_rgba(&self) -> u32 {
        self.color_rgba
    }
    pub fn set_color_rgba(&mut self, rgba: u32, _frame_count: f32, _easing: f32) {
        self.color_rgba = rgba;
        self.modified = true;
        self.record_api_call(
            "SetColor",
            vec![
                rgba.to_string(),
                _frame_count.max(0.0).to_string(),
                _easing.to_string(),
            ],
        );
    }

    pub fn grayscale(&self) -> f32 {
        self.grayscale
    }
    pub fn set_grayscale(&mut self, rate: f32, _frame_count: f32, _easing: f32) {
        if rate.is_finite() {
            self.grayscale = rate.clamp(0.0, 1.0);
            self.modified = true;
            self.record_api_call(
                "SetGrayscale",
                vec![
                    self.grayscale.to_string(),
                    _frame_count.max(0.0).to_string(),
                    _easing.to_string(),
                ],
            );
        }
    }

    pub fn as_original_scale(&self) -> bool {
        self.as_original_scale
    }
    pub fn set_as_original_scale(&mut self, state: bool) {
        self.as_original_scale = state;
        self.modified = true;
    }

    pub fn state_value(&self, label: &str) -> f32 {
        match label {
            "scale" => self.scale,
            "rot" => self.rot,
            "x" => self.coord[0],
            "y" => self.coord[1],
            _ => self.variable_value(label).unwrap_or(0.0),
        }
    }

    pub fn elapsed_ticks(&self) -> f32 {
        self.elapsed_ticks
    }

    pub fn variables(&self) -> &BTreeMap<String, EmoteVariableState> {
        &self.variables
    }

    /// Values written to MMotionPlayer variable references for scene
    /// evaluation.  The native player keeps these separate from the logical
    /// variable map returned by GetVariable: sub_10276500 starts from the
    /// logical map, applies mirror to resolved references, then immediately
    /// runs ClampControl via sub_10275CC0.  When stereovision is enabled this
    /// returns the currently selected physical screen's projected values.
    pub fn evaluated_variable_values(&self) -> BTreeMap<String, f32> {
        self.evaluated_variable_values_for_screen(self.stereovision_screen_index)
    }

    /// Evaluate the variable reference image for one physical stereovision
    /// screen.  Native sub_10277100 stores one resolved reference image per
    /// screen; exposing that image explicitly lets a host render the screens
    /// separately without inventing a proprietary multiview backend.
    pub fn evaluated_variable_values_for_screen(
        &self,
        screen_index: usize,
    ) -> BTreeMap<String, f32> {
        let mut values = self.evaluated_variable_states();
        if self.stereovision_enabled {
            apply_stereovision_screen_projection(
                &self.runtime_pipeline,
                &mut values,
                screen_index,
                self.stereovision_screen_count,
                self.stereovision_level,
                self.stereovision_fov,
                self.active_mirror_enabled(),
            );
        }
        values
            .into_iter()
            .map(|(name, state)| (name, state.value))
            .collect()
    }

    fn evaluated_variable_states(&self) -> BTreeMap<String, EmoteVariableState> {
        // Native sub_10268A30 order is:
        //   Timeline -> Difference -> Eye/Eyebrow/Mouth/Selector/Transition/
        //   Loop/Wind -> Mirror/Clamp -> Bust/Hair/Parts physics.
        // `self.variables` intentionally keeps the latest controller/physics
        // values queryable, so reconstruct the pre-physics image first and
        // then replay only the ordering-sensitive overlays below.
        let final_variables = &self.variables;
        let mut values = final_variables.clone();

        // A positive-tick physics pass runs *after* Mirror/Clamp. Restore the
        // values those output references held immediately before physics so
        // they are not mirrored/clamped a second time during scene evaluation.
        for (name, pre_physics_value) in &self.pre_physics_output_values {
            if let Some(state) = values.get_mut(name) {
                state.value = *pre_physics_value;
            }
        }

        // sub_10275A30 applies every active difference transition before the
        // fixed-step controllers.  Most ordinary variables therefore receive
        // the additive overlay here. LoopControl and a physics controller can
        // later overwrite their output reference, so an earlier difference
        // contribution to those labels must not be re-added after the fact.
        for (timeline_name, diff_values) in &self.timeline_diff_variables {
            let blend = self
                .active_timeline_states
                .get(timeline_name)
                .map(|state| state.blend_ratio)
                .or_else(|| self.timeline_blend_ratios.get(timeline_name).copied())
                .unwrap_or(0.0);
            if blend.abs() <= f32::EPSILON {
                continue;
            }
            for (name, diff_state) in diff_values {
                if loop_control_overwrites_variable(&self.runtime_pipeline, name)
                    || self.pre_physics_output_values.contains_key(name)
                {
                    continue;
                }
                let base = values.entry(name.clone()).or_insert_with(|| EmoteVariableState {
                    info: diff_state.info.clone(),
                    value: diff_state.info.default_value,
                    target: None,
                });
                base.value += diff_state.value * blend;
            }
        }

        // Native exits the controller loop through sub_10276500, whose tail
        // calls ClampControl. These passes happen before the physics groups.
        let pipeline = self.pipeline_with_active_mirror();
        apply_post_control_variable_passes(&pipeline, &mut values);

        // Finally publish the solver outputs produced after Mirror/Clamp. This
        // is especially important for mirrored characters: the native bust /
        // pendulum response is already computed in the player's mirrored
        // transform space and must not be sign-flipped a second time here.
        for name in self.pre_physics_output_values.keys() {
            if let Some(final_state) = final_variables.get(name) {
                values.insert(name.clone(), final_state.clone());
            }
        }
        values
    }

    /// Runtime/user mirror byte (+389). Native sub_1026E290 combines this
    /// with metadata.mirror (+390) using XOR to produce active mirror (+388).
    pub fn runtime_mirror_enabled(&self) -> bool {
        self.runtime_mirror_enabled
    }

    pub fn set_runtime_mirror_enabled(&mut self, enabled: bool) {
        self.runtime_mirror_enabled = enabled;
        self.modified = true;
        self.evaluate_runtime_pipeline(0.0);
    }

    pub fn active_mirror_enabled(&self) -> bool {
        active_mirror_state(
            self.runtime_mirror_enabled,
            self.runtime_pipeline.mirror_enabled,
        )
    }

    fn pipeline_with_active_mirror(&self) -> EmoteRuntimePipeline {
        let mut pipeline = self.runtime_pipeline.clone();
        pipeline.mirror_enabled = self.active_mirror_enabled();
        pipeline
    }

    pub fn stereovision_enabled(&self) -> bool {
        self.stereovision_enabled
    }

    pub fn set_stereovision_enabled(&mut self, enabled: bool) {
        self.stereovision_enabled = enabled;
        self.modified = true;
    }

    /// Native +496, initialized to 1.0 and multiplied by the camera fov when
    /// rebuilding StereovisionScreen coefficients.
    pub fn stereovision_level(&self) -> f32 {
        self.stereovision_level
    }

    pub fn set_stereovision_level(&mut self, level: f32) {
        if level.is_finite() {
            self.stereovision_level = level;
            self.modified = true;
        }
    }

    /// Current camera fov used by native sub_1027A4D0 (+500).
    pub fn stereovision_fov(&self) -> f32 {
        self.stereovision_fov
    }

    pub fn set_stereovision_fov(&mut self, fov: f32) {
        if fov.is_finite() {
            self.stereovision_fov = fov;
            self.modified = true;
        }
    }

    pub fn stereovision_screen_index(&self) -> usize {
        self.stereovision_screen_index
    }

    pub fn set_stereovision_screen_index(&mut self, screen_index: usize) {
        self.stereovision_screen_index = screen_index
            .min(self.stereovision_screen_count.saturating_sub(1));
        self.modified = true;
    }

    pub fn stereovision_screen_count(&self) -> usize {
        self.stereovision_screen_count
    }

    /// Portable host boundary for the native screen count field (+508).  This
    /// driver initializes it to 2; callers that own a multiview output surface
    /// can provide a larger count and evaluate each screen independently.
    pub fn set_stereovision_screen_count(&mut self, screen_count: usize) {
        self.stereovision_screen_count = screen_count.max(2);
        self.stereovision_screen_index = self
            .stereovision_screen_index
            .min(self.stereovision_screen_count - 1);
        self.modified = true;
    }

    /// Recovered coefficients generated by native sub_1027A4D0 for a named
    /// stereovision variable.  Returns physical-screen order, including the
    /// native active-mirror screen reversal.
    pub fn stereovision_screens_for_variable(
        &self,
        variable_name: &str,
    ) -> Option<Vec<EmoteStereovisionScreen>> {
        let state = self.variables.get(variable_name)?;
        if !stereovision_variable_is_targeted(&self.runtime_pipeline, variable_name) {
            return None;
        }
        let min = state.info.min_value?;
        let max = state.info.max_value?;
        let mut screens = Vec::with_capacity(self.stereovision_screen_count);
        for physical_index in 0..self.stereovision_screen_count {
            let vector_index = stereovision_vector_index(
                physical_index,
                self.stereovision_screen_count,
                self.active_mirror_enabled(),
            )?;
            screens.push(stereovision_screen_for_range(
                min,
                max,
                vector_index,
                self.stereovision_screen_count,
                self.stereovision_level,
                self.stereovision_fov,
            )?);
        }
        Some(screens)
    }

    pub fn timelines(&self) -> &BTreeMap<String, EmoteTimeline> {
        &self.timelines
    }

    pub fn default_timeline_name(&self) -> Option<&str> {
        self.timelines.keys().next().map(String::as_str)
    }

    pub fn main_timeline_labels(&self) -> Vec<&str> {
        self.timelines
            .values()
            .filter(|timeline| !timeline.is_difference)
            .map(|timeline| timeline.name.as_str())
            .collect()
    }

    pub fn diff_timeline_labels(&self) -> Vec<&str> {
        self.timelines
            .values()
            .filter(|timeline| timeline.is_difference)
            .map(|timeline| timeline.name.as_str())
            .collect()
    }

    pub fn playing_timeline_info(&self) -> Vec<(String, u32)> {
        self.active_timelines
            .iter()
            .map(|(name, mode)| (name.clone(), mode.flags))
            .collect()
    }

    pub fn timeline_total_frame_count(&self, name: &str) -> Option<f32> {
        self.timelines
            .get(name)
            .map(|timeline| timeline.duration_ticks)
    }

    pub fn timeline_blend_ratio(&self, name: &str) -> f32 {
        self.active_timeline_states
            .get(name)
            .map(|state| state.blend_ratio)
            .or_else(|| self.timeline_blend_ratios.get(name).copied())
            .unwrap_or(0.0)
    }

    pub fn is_timeline_playing(&self, name: &str) -> bool {
        if name.is_empty() {
            return !self.active_timelines.is_empty();
        }
        self.active_timelines.contains_key(name)
    }

    pub fn is_loop_timeline(&self, name: &str) -> bool {
        if let Some(timeline) = self.timelines.get(name) {
            if timeline.loop_begin_ticks >= 0.0
                && timeline.loop_end_ticks > timeline.loop_begin_ticks
            {
                return true;
            }
        }
        self.active_timeline_states
            .get(name)
            .map(|state| state.mode.is_looping())
            .or_else(|| {
                self.active_timelines
                    .get(name)
                    .map(|mode| mode.is_looping())
            })
            .unwrap_or(false)
    }

    pub fn variable_value(&self, name: &str) -> Option<f32> {
        self.variables.get(name).map(|state| state.value)
    }

    pub fn variable_frame_count(&self, name: &str) -> usize {
        self.variables
            .get(name)
            .map(|state| state.info.frames.len())
            .unwrap_or(0)
    }

    pub fn variable_frame_label_at(&self, name: &str, index: usize) -> Option<&str> {
        self.variables
            .get(name)?
            .info
            .frames
            .get(index)
            .map(|frame| frame.label.as_str())
    }

    pub fn variable_frame_value_at(&self, name: &str, index: usize) -> Option<f32> {
        self.variables
            .get(name)?
            .info
            .frames
            .get(index)
            .map(|frame| frame.value)
    }

    pub fn pending_writes(&self) -> &[VariableWrite] {
        &self.pending_writes
    }

    pub fn active_timelines(&self) -> &BTreeMap<String, TimelinePlayMode> {
        &self.active_timelines
    }

    pub fn apply_write(&mut self, write: VariableWrite) {
        self.set_variable_timed(&write.name, write.value, write.time_ticks, write.easing);
    }

    pub fn from_scene_and_variables(
        scene: EmoteStaticScene,
        infos: Vec<EmoteVariableInfo>,
    ) -> Self {
        Self::from_scene_variables_timelines(scene, infos, Vec::new())
    }

    pub fn from_scene_variables_timelines(
        scene: EmoteStaticScene,
        infos: Vec<EmoteVariableInfo>,
        timelines: Vec<EmoteTimeline>,
    ) -> Self {
        Self::from_scene_variables_timelines_runtime(
            scene,
            infos,
            timelines,
            EmoteRuntimePipeline::default(),
        )
    }

    pub fn from_scene_variables_timelines_runtime(
        scene: EmoteStaticScene,
        infos: Vec<EmoteVariableInfo>,
        timelines: Vec<EmoteTimeline>,
        runtime_pipeline: EmoteRuntimePipeline,
    ) -> Self {
        let mut variables = BTreeMap::new();
        for info in infos {
            let name = info.name.clone();
            variables.entry(name).or_insert_with(|| EmoteVariableState {
                value: info.default_value,
                info,
                target: None,
            });
        }

        let mut timeline_map = BTreeMap::new();
        for timeline in timelines {
            for variable in &timeline.variables {
                variables
                    .entry(variable.name.clone())
                    .or_insert_with(|| EmoteVariableState {
                        info: EmoteVariableInfo {
                            name: variable.name.clone(),
                            // Timeline parsing never seeds the target variable
                            // from its first key. If a referenced variable was
                            // not present in the authored variable/control/mesh
                            // tables, keep Rust's synthetic fallback at the
                            // native zero-initialized scalar value.
                            default_value: 0.0,
                            min_value: None,
                            max_value: None,
                            frames: Vec::new(),
                        },
                        value: 0.0,
                        target: None,
                    });
                if let Some(state) = variables.get_mut(&variable.name) {
                    // Native timeline parsing registers commands/cursors but does not
                    // rewrite an already-created variable's initial value.  The old
                    // Rust path let every timeline's first key overwrite the global
                    // pose during construction, making startup depend on timeline
                    // enumeration order.
                    merge_timeline_variable_range(&mut state.info, variable);
                }
            }
            timeline_map.insert(timeline.name.clone(), timeline);
        }

        // Native controls are independent controller objects.  They are not
        // synthesized as hidden looping timelines and therefore no timeline is
        // active until PlayTimeline is called.
        let active_timelines = BTreeMap::new();
        let active_timeline_states = BTreeMap::new();

        augment_variable_ranges_from_controls(&mut variables, &runtime_pipeline);

        let eye_states = init_eye_states(&runtime_pipeline);
        let eyebrow_states = init_eyebrow_states(&runtime_pipeline);
        let mouth_states = init_mouth_states(&runtime_pipeline);
        let mut transition_states = init_transition_states(&runtime_pipeline);
        let selector_states = init_selector_states(&runtime_pipeline, &mut transition_states);
        let loop_states = init_loop_states(&runtime_pipeline);
        let bust_states = init_bust_states(&runtime_pipeline);
        let hair_states = init_hair_states(&runtime_pipeline);
        // MEmotePlayer::Init seeds +500 to 0.2.  The camera specialized
        // pass overwrites the same field with the active camera fov, so a
        // scene that already carries recovered camera runtime state starts
        // from that value.  Keep this local before `scene` is moved below.
        let stereovision_fov = scene
            .camera_runtime
            .as_ref()
            .map(|camera| camera.fov)
            .unwrap_or(0.2);

        let mut player = Self {
            shown: true,
            smoothing: true,
            mesh_division_ratio: 1.0,
            queuing: false,
            color_rgba: 0xffff_ffff,
            grayscale: 0.0,
            as_original_scale: false,
            coord: [0.0, 0.0],
            scale: 1.0,
            rot: 0.0,
            elapsed_ticks: 0.0,
            paused: false,
            physics_enabled: true,
            scene,
            variables,
            timelines: timeline_map,
            pending_writes: Vec::new(),
            active_timelines,
            active_timeline_states,
            timeline_diff_variables: BTreeMap::new(),
            timeline_blend_ratios: BTreeMap::new(),
            pre_physics_output_values: BTreeMap::new(),
            runtime_pipeline,
            eye_states,
            eyebrow_states,
            mouth_states,
            selector_states,
            transition_states,
            loop_states,
            bust_states,
            hair_states,
            outer_force_states: ["bust", "hair", "parts"]
                .into_iter()
                .map(|label| (label.to_owned(), Vector2TransitionState::default()))
                .collect(),
            outer_rot: 0.0,
            outer_rot_target: None,
            transform_order_mask: transform_order_mask::DEFAULT,
            hair_scale: 1.0,
            parts_scale: 1.0,
            bust_scale: 1.0,
            wind: None,
            runtime_mirror_enabled: false,
            stereovision_enabled: false,
            stereovision_level: 1.0,
            stereovision_fov,
            stereovision_screen_index: 0,
            stereovision_screen_count: 2,
            modified: false,
            recording_api_log: false,
            replaying_api_log: false,
            api_log: Vec::new(),
        };
        player.evaluate_runtime_pipeline(0.0);
        player
    }

    pub fn runtime_pipeline(&self) -> &EmoteRuntimePipeline {
        &self.runtime_pipeline
    }

    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn set_physics_enabled(&mut self, enabled: bool) {
        self.physics_enabled = enabled;
    }

    pub fn is_physics_enabled(&self) -> bool {
        self.physics_enabled
    }

    pub fn evaluate_physics_for_current_scene(&mut self, delta_ticks: f32) {
        self.pre_physics_output_values.clear();
        if !self.physics_enabled || !delta_ticks.is_finite() || delta_ticks < 0.0 {
            return;
        }
        if delta_ticks > PHYSICS_EPSILON_TICKS {
            self.capture_pre_physics_output_values();
            self.evaluate_physics_controls(delta_ticks);
        }
        self.modified = true;
    }

    pub fn progress_ticks_without_physics(&mut self, delta_ticks: f32) {
        self.progress_ticks_internal(delta_ticks, false);
    }

    pub fn set_variable_immediate(&mut self, name: &str, value: f32) {
        self.pre_physics_output_values.remove(name);
        if self
            .set_control_variable(name, value, 0.0, 0.0)
            .is_some()
        {
            self.pending_writes
                .push(VariableWrite::timed(name, value, 0.0, 0.0));
            self.modified = true;
            self.evaluate_runtime_pipeline(0.0);
        } else if let Some(state) = self.variables.get_mut(name) {
            // Authored min/max are descriptive parameter/UI ranges. Native
            // SetVariable does not clamp ordinary variables here; ClampControl
            // is the explicit post-control constraint pass.
            state.value = value;
            state.target = None;
            self.modified = true;
            self.evaluate_runtime_pipeline(0.0);
        }
    }

    pub fn reset_variable_to_default(&mut self, name: &str) {
        self.pre_physics_output_values.remove(name);
        if let Some(state) = self.variables.get_mut(name) {
            let default = state.info.default_value;
            state.value = default;
            state.target = None;
            self.modified = true;
        }
    }

    pub fn reset_physics(&mut self) {
        self.bust_states = init_bust_states(&self.runtime_pipeline);
        self.hair_states = init_hair_states(&self.runtime_pipeline);
    }

    pub fn timeline_elapsed_ticks(&self, name: &str) -> f32 {
        self.active_timeline_states
            .get(name)
            .map(|s| s.elapsed_ticks)
            .unwrap_or(0.0)
    }

    pub fn set_timeline_time(&mut self, name: &str, ticks: f32) -> Result<(), String> {
        let Some(timeline) = self.timelines.get(name).cloned() else {
            return Err(format!("timeline not found: {name}"));
        };
        let duration = timeline.duration_ticks.max(0.0);
        let local_time = if duration > 0.0 {
            ticks.clamp(0.0, duration)
        } else {
            ticks.max(0.0)
        };
        let mode = self
            .active_timeline_states
            .get(name)
            .map(|state| state.mode)
            .or_else(|| self.active_timelines.get(name).copied())
            .unwrap_or(TimelinePlayMode::Once);
        self.active_timelines.insert(name.to_owned(), mode);

        // Seeking an already-active native timeline does not reconstruct its
        // EPTransitionControl. In particular, a running FadeIn/FadeOut keeps
        // its blend transition and runtime +40 stop flag across the seek.
        if let Some(state) = self.active_timeline_states.get_mut(name) {
            state.mode = mode;
            state.elapsed_ticks = local_time;
            state.frame_indices = vec![usize::MAX; timeline.variables.len()];
        } else {
            self.timeline_blend_ratios.insert(name.to_owned(), 1.0);
            self.active_timeline_states.insert(
                name.to_owned(),
                ActiveTimelineState {
                    mode,
                    elapsed_ticks: local_time,
                    frame_indices: vec![usize::MAX; timeline.variables.len()],
                    blend_ratio: 1.0,
                    blend_target: None,
                    blend_queue: VecDeque::new(),
                    stop_when_blend_idle: false,
                },
            );
        }
        self.seek_active_timeline(name, local_time);
        self.evaluate_runtime_pipeline(0.0);
        Ok(())
    }

    pub fn set_selector_option(&mut self, label: &str, option_index: usize) -> Result<(), String> {
        let Some(control) = self
            .runtime_pipeline
            .selector_controls
            .iter()
            .find(|control| control.label == label)
        else {
            return Err(format!("selectorControl not found: {label}"));
        };
        if option_index >= control.option_list.len() {
            return Err(format!(
                "selectorControl {label} option index {option_index} out of range {}",
                control.option_list.len()
            ));
        }
        self.set_variable_immediate(label, option_index as f32);
        self.evaluate_runtime_pipeline(0.0);
        Ok(())
    }

    pub fn reset_all_variables_to_default(&mut self) {
        for state in self.variables.values_mut() {
            state.value = state.info.default_value;
            state.target = None;
        }
        self.modified = true;
        self.evaluate_runtime_pipeline(0.0);
    }

    fn set_variable_timed_internal(
        &mut self,
        name: &str,
        value: f32,
        time_ticks: f32,
        easing: f32,
    ) -> Option<f32> {
        if name.is_empty() || !value.is_finite() {
            return None;
        }

        if let Some(target_value) = self.set_control_variable(name, value, time_ticks, easing) {
            self.pending_writes.push(VariableWrite::timed(
                name,
                target_value,
                time_ticks.max(0.0),
                easing,
            ));
            self.modified = true;
            return Some(target_value);
        }

        let state = self.ensure_variable(name);
        // Do not clamp to EmoteVariableInfo min/max.  Those values describe
        // the authored parameter/control domain; native ordinary variable
        // writes remain unconstrained until the dedicated ClampControl pass.
        let target_value = value;

        if time_ticks <= 0.0 || !time_ticks.is_finite() {
            state.value = target_value;
            state.target = None;
        } else {
            state.target = Some(EmoteVariableTarget {
                start_value: state.value,
                target_value,
                elapsed_ticks: 0.0,
                duration_ticks: time_ticks,
                easing,
            });
        }

        self.pending_writes.push(VariableWrite::timed(
            name,
            target_value,
            time_ticks.max(0.0),
            easing,
        ));
        self.modified = true;
        Some(target_value)
    }

    fn set_control_variable(
        &mut self,
        name: &str,
        value: f32,
        time_ticks: f32,
        easing: f32,
    ) -> Option<f32> {
        // sub_102780B0 routes variable references by controller type before
        // falling back to an ordinary variable.  Mouth has two references:
        // `label` changes the integer mouth frame immediately, while
        // `talkLabel` enters EPMouthControl's timed queue.
        for (index, control) in self.runtime_pipeline.mouth_controls.iter().enumerate() {
            if control.label == name {
                if let Some(state) = self.mouth_states.get_mut(index) {
                    state.begin_frame = value as i32;
                    return Some(value);
                }
            }
            if control.talk_label == name {
                if let Some(state) = self.mouth_states.get_mut(index) {
                    set_mouth_talk_target(state, value, time_ticks, easing, self.queuing);
                    return Some(value);
                }
            }
        }

        // Variable-reference type 8 routes selector labels to
        // EPSelectorControl::Set (sub_1020EE10).  Selector commands have their
        // own queue/timer and retarget linked transition controls when they
        // begin; they are not ordinary interpolated variables.
        if let Some(index) = self
            .runtime_pipeline
            .selector_controls
            .iter()
            .position(|control| control.label == name)
        {
            let control = self.runtime_pipeline.selector_controls[index].clone();
            if index < self.selector_states.len() {
                let pipeline = self.runtime_pipeline.clone();
                let queuing = self.queuing;
                set_selector_target(
                    &control,
                    &mut self.selector_states[index],
                    value,
                    time_ticks,
                    easing,
                    queuing,
                    &pipeline,
                    &mut self.transition_states,
                );
                return Some(value);
            }
        }

        for (index, control) in self.runtime_pipeline.transition_controls.iter().enumerate() {
            if control.label == name {
                if let Some(state) = self.transition_states.get_mut(index) {
                    set_scalar_transition_target(
                        state,
                        value,
                        time_ticks,
                        easing,
                        self.queuing,
                    );
                    return Some(value);
                }
            }
        }

        // Variable-reference types 4/5 route to EPEyeControl/EPEyebrowControl.
        // sub_101E7040/sub_101EA310 keep their own timed command queues and
        // traverse EPGraph routes rather than tweening frame numbers directly.
        for (index, control) in self.runtime_pipeline.eye_controls.iter().enumerate() {
            if control.label == name {
                if let Some(state) = self.eye_states.get_mut(index) {
                    set_eye_graph_target(state, value, time_ticks, easing, self.queuing);
                    return Some(value);
                }
            }
        }
        for (index, control) in self.runtime_pipeline.eyebrow_controls.iter().enumerate() {
            if control.label == name {
                if let Some(state) = self.eyebrow_states.get_mut(index) {
                    set_eyebrow_graph_target(state, value, time_ticks, easing, self.queuing);
                    return Some(value);
                }
            }
        }

        None
    }

    pub fn variable_diff_value(&self, module: &str, name: &str) -> Option<f32> {
        self.variable_value(&format!("{module}/{name}"))
    }

    pub fn is_animating(&self) -> bool {
        if self
            .active_timeline_states
            .values()
            .any(|state| state.blend_ratio > 0.0)
        {
            return true;
        }
        self.variables.values().any(|state| state.target.is_some())
            || self.outer_rot_target.is_some()
            || self
                .outer_force_states
                .values()
                .any(|state| state.active || !state.queue.is_empty())
            || self.wind.is_some()
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn clear_modified(&mut self) {
        self.modified = false;
    }

    pub fn pass(&mut self) {
        self.reapply_active_timelines_at_current_time();
        self.evaluate_runtime_pipeline(0.0);
        self.modified = true;
        self.record_api_call("Pass", Vec::new());
    }

    pub fn step(&mut self) {
        <Self as EmotePlayerControl>::progress_ticks(self, 1.0);
        self.record_api_call("Step", Vec::new());
    }

    pub fn start_record_api_log(&mut self) {
        self.api_log.clear();
        self.recording_api_log = true;
    }

    pub fn stop_record_api_log(&mut self) {
        self.recording_api_log = false;
    }

    pub fn is_recording_api_log(&self) -> bool {
        self.recording_api_log
    }

    pub fn start_replay_api_log(&mut self) {
        self.replaying_api_log = true;
    }

    pub fn stop_replay_api_log(&mut self) {
        self.replaying_api_log = false;
    }

    pub fn is_replaying_api_log(&self) -> bool {
        self.replaying_api_log
    }

    pub fn clear_api_log(&mut self) {
        self.api_log.clear();
    }

    pub fn api_log(&self) -> String {
        self.api_log
            .iter()
            .map(EmoteApiLogEntry::encode)
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn set_api_log(&mut self, log: &str) {
        self.api_log = log.lines().filter_map(parse_api_log_entry).collect();
    }

    pub fn replay_api_log_once(&mut self) {
        let entries = self.api_log.clone();
        for entry in entries {
            self.apply_api_log_entry(&entry);
        }
    }

    fn record_api_call(&mut self, command: &str, args: Vec<String>) {
        if self.recording_api_log && !self.replaying_api_log {
            self.api_log.push(EmoteApiLogEntry::new(command, args));
        }
    }

    fn apply_api_log_entry(&mut self, entry: &EmoteApiLogEntry) {
        self.replaying_api_log = true;
        match entry.command.as_str() {
            "SetVariable" if entry.args.len() >= 4 => {
                if let (Ok(value), Ok(frame_count), Ok(easing)) = (
                    entry.args[1].parse::<f32>(),
                    entry.args[2].parse::<f32>(),
                    entry.args[3].parse::<f32>(),
                ) {
                    self.set_variable_timed(&entry.args[0], value, frame_count, easing);
                }
            }
            "SetVariableDiff" if entry.args.len() >= 5 => {
                if let (Ok(value), Ok(frame_count), Ok(easing)) = (
                    entry.args[2].parse::<f32>(),
                    entry.args[3].parse::<f32>(),
                    entry.args[4].parse::<f32>(),
                ) {
                    self.set_variable_diff(
                        &entry.args[0],
                        &entry.args[1],
                        value,
                        frame_count,
                        easing,
                    );
                }
            }
            "PlayTimeline" if entry.args.len() >= 2 => {
                if let Ok(flags) = entry.args[1].parse::<u32>() {
                    self.play_timeline(
                        &entry.args[0],
                        TimelinePlayMode {
                            flags,
                            looping: false,
                        },
                    );
                }
            }
            "StopTimeline" if !entry.args.is_empty() => self.stop_timeline(&entry.args[0]),
            "SetTimelineBlendRatio" if entry.args.len() >= 5 => {
                if let (Ok(value), Ok(frame_count), Ok(easing), Ok(stop_when_done)) = (
                    entry.args[1].parse::<f32>(),
                    entry.args[2].parse::<f32>(),
                    entry.args[3].parse::<f32>(),
                    entry.args[4].parse::<bool>(),
                ) {
                    self.set_timeline_blend_ratio(
                        &entry.args[0],
                        value,
                        frame_count,
                        easing,
                        stop_when_done,
                    );
                }
            }
            "SetOuterForce" if entry.args.len() >= 5 => {
                if let (Ok(x), Ok(y), Ok(frame_count), Ok(easing)) = (
                    entry.args[1].parse::<f32>(),
                    entry.args[2].parse::<f32>(),
                    entry.args[3].parse::<f32>(),
                    entry.args[4].parse::<f32>(),
                ) {
                    self.set_outer_force(&entry.args[0], x, y, frame_count, easing);
                }
            }
            "SetOuterRot" if entry.args.len() >= 3 => {
                if let (Ok(rot), Ok(frame_count), Ok(easing)) = (
                    entry.args[0].parse::<f32>(),
                    entry.args[1].parse::<f32>(),
                    entry.args[2].parse::<f32>(),
                ) {
                    self.set_outer_rot(rot, frame_count, easing);
                }
            }
            "SetCoord" if entry.args.len() >= 2 => {
                if let (Ok(x), Ok(y)) = (entry.args[0].parse::<f32>(), entry.args[1].parse::<f32>())
                {
                    self.set_coord(x, y);
                }
            }
            "SetScale" if !entry.args.is_empty() => {
                if let Ok(scale) = entry.args[0].parse::<f32>() {
                    self.set_scale(scale);
                }
            }
            "SetRot" if !entry.args.is_empty() => {
                if let Ok(rot) = entry.args[0].parse::<f32>() {
                    self.set_rot(rot);
                }
            }
            "Progress" if !entry.args.is_empty() => {
                if let Ok(frame_count) = entry.args[0].parse::<f32>() {
                    <Self as EmotePlayerControl>::progress_ticks(self, frame_count);
                }
            }
            _ => {}
        }
        self.replaying_api_log = false;
    }

    fn ensure_variable(&mut self, name: &str) -> &mut EmoteVariableState {
        self.variables
            .entry(name.to_owned())
            .or_insert_with(|| EmoteVariableState {
                info: EmoteVariableInfo {
                    name: name.to_owned(),
                    default_value: 0.0,
                    min_value: None,
                    max_value: None,
                    frames: Vec::new(),
                },
                value: 0.0,
                target: None,
            })
    }

    fn is_timeline_control_variable(&self, name: &str) -> bool {
        // TimelineVariable::special (+28), set by sub_1026FA30 from
        // MEmotePlayer::timelinePlayingVariableLabelSet.  These labels resolve
        // to controller-backed variable references (types 4..8 in
        // sub_102780B0), not plain float variables.
        self.runtime_pipeline
            .mouth_controls
            .iter()
            .any(|control| control.label == name || control.talk_label == name)
            || self
                .runtime_pipeline
                .selector_controls
                .iter()
                .any(|control| control.label == name)
            || self
                .runtime_pipeline
                .transition_controls
                .iter()
                .any(|control| control.label == name)
            || self
                .runtime_pipeline
                .eye_controls
                .iter()
                .any(|control| control.label == name)
            || self
                .runtime_pipeline
                .eyebrow_controls
                .iter()
                .any(|control| control.label == name)
    }

    fn timeline_command_target(
        &self,
        _variable: &EmoteTimelineVariable,
        frame: &EmoteTimelineFrame,
        difference_path: bool,
    ) -> f32 {
        // sub_1026A660/sub_1026B1F0 pass TimelineVariableFrame::value (+8)
        // directly to both setter paths.  Ordinary timelines are NOT blended
        // around the metadata default; Timeline runtime +36 is the DIFFERENCE
        // transition weight progressed by sub_10275810.  Stage 6 already
        // established that difference frames are authored deltas, so both
        // branches consume the same raw frame value here.
        let _ = difference_path;
        frame.value
    }

    fn issue_timeline_frame_command(
        &mut self,
        timeline_name: &str,
        variable: &EmoteTimelineVariable,
        frame_index: usize,
        command_time: f32,
        mode: TimelinePlayMode,
    ) {
        let Some(frame) = variable.frames.get(frame_index) else {
            return;
        };
        // TimelineVariableFrame::type == 0 is a cursor-only marker. Native
        // sub_1026A660/sub_1026B1F0 deliberately skip the variable setter.
        if frame.hold {
            return;
        }
        let duration = variable
            .frames
            .get(frame_index + 1)
            .map(|next| (next.time_ticks - command_time - 1.0).max(0.0))
            .unwrap_or(0.0);
        let special = self.is_timeline_control_variable(&variable.name);
        // sub_1026A660/sub_1026B1F0: flag bit 2 skips special/controller-backed
        // timeline variables.  The public name of this legacy flag is not
        // present in the stripped DLL, so keep the native bit test local.
        if (mode.flags & (1 << 2)) != 0 && special {
            return;
        }
        let difference_path = mode.is_difference() && !special;
        let target = self.timeline_command_target(variable, frame, difference_path);
        if difference_path {
            self.set_difference_timeline_target(
                timeline_name,
                &variable.name,
                target,
                duration,
                frame.easing,
            );
        } else {
            let _ = self.set_variable_timed_internal(
                &variable.name,
                target,
                duration,
                frame.easing,
            );
        }
    }

    fn set_difference_timeline_target(
        &mut self,
        timeline_name: &str,
        variable_name: &str,
        target_value: f32,
        time_ticks: f32,
        easing: f32,
    ) {
        let state = self
            .timeline_diff_variables
            .entry(timeline_name.to_owned())
            .or_default()
            .entry(variable_name.to_owned())
            .or_insert_with(|| EmoteVariableState {
                info: EmoteVariableInfo {
                    name: variable_name.to_owned(),
                    default_value: 0.0,
                    min_value: None,
                    max_value: None,
                    frames: Vec::new(),
                },
                value: 0.0,
                target: None,
            });

        if time_ticks <= 0.0 || !time_ticks.is_finite() {
            state.value = target_value;
            state.target = None;
        } else {
            state.target = Some(EmoteVariableTarget {
                start_value: state.value,
                target_value,
                elapsed_ticks: 0.0,
                duration_ticks: time_ticks,
                easing,
            });
        }
    }

    /// Native Timeline seek (sub_1026A660): rebuild each variable cursor at
    /// `time_ticks`, then issue the latest non-hold command with the remaining
    /// authored duration. This is intentionally separate from forward progress.
    fn seek_active_timeline(&mut self, name: &str, time_ticks: f32) {
        let Some(timeline) = self.timelines.get(name).cloned() else {
            return;
        };
        let Some(mut state) = self.active_timeline_states.remove(name) else {
            return;
        };
        if state.frame_indices.len() != timeline.variables.len() {
            state.frame_indices = vec![usize::MAX; timeline.variables.len()];
        }
        let target_time = time_ticks.max(0.0);
        state.elapsed_ticks = target_time;
        let mode = state.mode;
        for (var_index, variable) in timeline.variables.iter().enumerate() {
            if variable.frames.is_empty() {
                continue;
            }
            let mut cursor = usize::MAX;
            let mut latest_command = None;
            for (index, frame) in variable.frames.iter().enumerate() {
                if frame.time_ticks > target_time {
                    break;
                }
                cursor = index;
                if !frame.hold {
                    latest_command = Some(index);
                }
            }
            state.frame_indices[var_index] = cursor;
            if let Some(index) = latest_command {
                self.issue_timeline_frame_command(
                    name,
                    variable,
                    index,
                    target_time,
                    mode,
                );
            }
        }
        self.active_timeline_states.insert(name.to_owned(), state);
    }

    /// Native forward timeline progress (sub_1026B1F0). Only crossing a frame
    /// boundary can issue a new variable command; the timeline is not a
    /// stateless interpolation curve.
    fn advance_active_timeline_to(&mut self, name: &str, target_time: f32, inclusive: bool) {
        let Some(timeline) = self.timelines.get(name).cloned() else {
            return;
        };
        let Some(mut state) = self.active_timeline_states.remove(name) else {
            return;
        };
        if state.frame_indices.len() != timeline.variables.len() {
            state.frame_indices = vec![usize::MAX; timeline.variables.len()];
        }
        let mode = state.mode;
        for (var_index, variable) in timeline.variables.iter().enumerate() {
            if variable.frames.is_empty() {
                continue;
            }
            let mut cursor = state.frame_indices[var_index];
            loop {
                let next_index = if cursor == usize::MAX { 0 } else { cursor + 1 };
                if next_index >= variable.frames.len() {
                    break;
                }
                let next_time = variable.frames[next_index].time_ticks;
                let crossed = if inclusive {
                    target_time >= next_time
                } else {
                    target_time > next_time
                };
                if !crossed {
                    break;
                }
                cursor = next_index;
                self.issue_timeline_frame_command(
                    name,
                    variable,
                    cursor,
                    target_time,
                    mode,
                );
            }
            state.frame_indices[var_index] = cursor;
        }
        state.elapsed_ticks = target_time;
        self.active_timeline_states.insert(name.to_owned(), state);
    }

    fn reapply_active_timelines_at_current_time(&mut self) {
        let names: Vec<(String, f32)> = self
            .active_timeline_states
            .iter()
            .map(|(name, state)| (name.clone(), state.elapsed_ticks))
            .collect();
        for (name, time) in names {
            self.seek_active_timeline(&name, time);
        }
    }

    fn progress_active_timelines(&mut self, delta_ticks: f32) {
        if !delta_ticks.is_finite() || delta_ticks <= 0.0 {
            return;
        }
        let names: Vec<String> = self.active_timeline_states.keys().cloned().collect();
        let mut finished = Vec::new();

        for name in names {
            let Some(timeline) = self.timelines.get(&name).cloned() else {
                finished.push(name);
                continue;
            };

            let mut remaining = delta_ticks;
            let mut current = self
                .active_timeline_states
                .get(&name)
                .map(|state| state.elapsed_ticks)
                .unwrap_or(0.0);
            let mut natural_finished = false;

            // Native authored looping is controlled by loopBegin/loopEnd. The
            // legacy Rust `mode.looping` is retained only as a compatibility
            // fallback for old synthetic timelines lacking those fields.
            let authored_loop = timeline.loop_begin_ticks >= 0.0
                && timeline.loop_end_ticks > timeline.loop_begin_ticks;
            let compat_loop = !authored_loop
                && self
                    .active_timeline_states
                    .get(&name)
                    .map(|state| state.mode.is_looping())
                    .unwrap_or(false)
                && timeline.last_time_ticks > 0.0;
            let loop_begin = if authored_loop {
                timeline.loop_begin_ticks
            } else {
                0.0
            };
            let loop_end = if authored_loop {
                timeline.loop_end_ticks
            } else {
                timeline.last_time_ticks
            };

            if authored_loop || compat_loop {
                while remaining > 0.0 && loop_end > loop_begin {
                    let to_end = (loop_end - current).max(0.0);
                    if remaining < to_end {
                        current += remaining;
                        self.advance_active_timeline_to(&name, current, true);
                        remaining = 0.0;
                    } else {
                        // sub_10275350: advance exactly to loopEnd with the
                        // non-inclusive boundary mode, seek loopBegin, then
                        // consume the residual delta.
                        self.advance_active_timeline_to(&name, loop_end, false);
                        remaining -= to_end;
                        current = loop_begin;
                        self.seek_active_timeline(&name, loop_begin);
                        if to_end <= f32::EPSILON && remaining > 0.0 {
                            // Degenerate authored loop guard.
                            remaining = 0.0;
                        }
                    }
                }
            } else {
                let end = timeline.last_time_ticks.max(0.0);
                let target = (current + remaining).min(end);
                self.advance_active_timeline_to(&name, target, true);
                natural_finished = target >= end - f32::EPSILON;
            }

            // sub_10275350 advances/seekes timeline frame cursors first and
            // only then calls sub_10275810.  That helper advances Timeline
            // runtime +36 and per-variable transitions exclusively when the
            // DIFFERENCE bit is set.
            let difference_mode = self
                .active_timeline_states
                .get(&name)
                .map(|state| state.mode.is_difference())
                .unwrap_or(false);
            if difference_mode {
                if let Some(state) = self.active_timeline_states.get_mut(&name) {
                    let ratio = advance_timeline_blend(state, delta_ticks);
                    self.timeline_blend_ratios.insert(name.clone(), ratio);
                }
                self.progress_difference_timeline_targets_for(&name, delta_ticks);
            }

            // Runtime +40 is checked after sub_10275810. Native erases the
            // timeline whenever that flag is nonzero and EPTransitionControl
            // is idle; the numeric target need not be zero.
            let stop_on_idle = self
                .active_timeline_states
                .get(&name)
                .map(|state| {
                    state.stop_when_blend_idle && timeline_blend_transition_idle(state)
                })
                .unwrap_or(false);
            if natural_finished || stop_on_idle {
                finished.push(name);
            }
        }

        for name in finished {
            self.active_timelines.remove(&name);
            self.active_timeline_states.remove(&name);
            self.timeline_blend_ratios.remove(&name);
            self.timeline_diff_variables.remove(&name);
        }
    }

    fn evaluate_runtime_control_step(
        &mut self,
        pipeline: &EmoteRuntimePipeline,
        step_ticks: f32,
    ) {
        // sub_10268A30 fixed-step order.  Mirror/Clamp are intentionally not
        // here: native executes them once after this whole substep loop.
        for (index, control) in pipeline.eye_controls.iter().enumerate() {
            if let Some(state) = self.eye_states.get_mut(index) {
                evaluate_eye_control(control, state, step_ticks, &mut self.variables);
            }
        }
        for (index, control) in pipeline.eyebrow_controls.iter().enumerate() {
            if let Some(state) = self.eyebrow_states.get_mut(index) {
                evaluate_eyebrow_control(control, state, step_ticks, &mut self.variables);
            }
        }
        for (index, control) in pipeline.mouth_controls.iter().enumerate() {
            if let Some(state) = self.mouth_states.get_mut(index) {
                evaluate_mouth_control(control, state, step_ticks, &mut self.variables);
            }
        }
        for (index, control) in pipeline.selector_controls.iter().enumerate() {
            if let Some(state) = self.selector_states.get_mut(index) {
                evaluate_selector_control(
                    control,
                    state,
                    step_ticks,
                    pipeline,
                    &mut self.transition_states,
                    &mut self.variables,
                );
            }
        }
        for (index, control) in pipeline.transition_controls.iter().enumerate() {
            if let Some(state) = self.transition_states.get_mut(index) {
                evaluate_transition_control(control, state, step_ticks, &mut self.variables);
            }
        }
        for (index, control) in pipeline.loop_controls.iter().enumerate() {
            if let Some(state) = self.loop_states.get_mut(index) {
                evaluate_loop_control(control, state, step_ticks, &mut self.variables);
            }
        }

        // sub_10275C40 is the optional EPWindControl dispatch at the end of
        // each controller substep.  Pend/bust physics runs later, after motion
        // evaluation, and samples the already-advanced wind field.
        if !self.paused {
            self.progress_wind(step_ticks);
        }
    }

    fn evaluate_runtime_pipeline(&mut self, delta_ticks: f32) {
        let pipeline = self.pipeline_with_active_mirror();

        // Constructor sub_1026B900 initializes the frame divider to {1,0,0}
        // so normal players execute this block every Progress call.  Inside
        // sub_10268A30 each iteration receives min(remaining, 1.1), where the
        // 1.1 constant is at rdata 0x1050EA30.  This matters for queued control
        // transitions and wind spawning when the host supplies a long frame.
        if delta_ticks.is_finite() && delta_ticks > 0.0 {
            let mut remaining = delta_ticks;
            while remaining > 0.0 {
                let step_ticks = remaining.min(CONTROL_STEP_CAP_TICKS);
                self.evaluate_runtime_control_step(&pipeline, step_ticks);
                remaining -= step_ticks;
                if remaining <= f32::EPSILON {
                    break;
                }
            }
        } else {
            // The native force-update byte can make sub_10268A30 run one zero
            // tick after setters.  Rust's Pass/initialization paths call this
            // method with zero for the same purpose.
            self.evaluate_runtime_control_step(&pipeline, 0.0);
        }

        // Native sub_10268A30 executes Mirror then Clamp only after all control
        // substeps. Rust materializes those non-destructively in
        // evaluated_variable_states().  A positive-tick physics pass comes
        // *after* them, so remember the affected references before the solver
        // overwrites them and re-overlay the solver result at evaluation time.
        // A zero-time setter (e.g. face_talk) must preserve the already
        // published physics result. Otherwise rebuilding the same frame clamps
        // or mirrors that result again, causing visible discontinuities.
        if delta_ticks > PHYSICS_EPSILON_TICKS {
            self.pre_physics_output_values.clear();
        }
        if self.physics_enabled && delta_ticks > PHYSICS_EPSILON_TICKS {
            self.capture_pre_physics_output_values();
            self.evaluate_physics_controls(delta_ticks);
        }
    }

    fn physics_anchor_with_player_transform(&self, mut anchor: [f32; 3]) -> [f32; 3] {
        // IEmotePlayer::transform_order_mask_t uses an independent high-byte
        // ordering for the coordinates fed into Bust/Hair/Parts. Keep the
        // same two affine orderings as the public position half:
        //   0x100: translate first, then scale => (p + coord) * scale
        //   0x200: scale first, then translate => p * scale + coord
        // The player rotation is intentionally NOT folded into the point here;
        // native physics receives effective rotation separately as an angle.
        let scale = self.scale;
        if (self.transform_order_mask
            & crate::api::transform_order_mask::PHYSICS_TRANSLATE_TO_SCALE)
            != 0
        {
            anchor[0] = (anchor[0] + self.coord[0]) * scale;
            anchor[1] = (anchor[1] + self.coord[1]) * scale;
        } else {
            // SDK default and 0x200 both select SCALE_TO_TRANSLATE. If a caller
            // supplies neither bit, preserve this native/default branch rather
            // than silently dropping scale from soft-body anchors.
            anchor[0] = anchor[0] * scale + self.coord[0];
            anchor[1] = anchor[1] * scale + self.coord[1];
        }
        anchor[2] *= scale;
        anchor
    }

    fn capture_pre_physics_output_values(&mut self) {
        self.pre_physics_output_values.clear();
        for control in &self.runtime_pipeline.physics_controls {
            let def = match control {
                PhysicsControl::Bust(def) | PhysicsControl::Hair(def) | PhysicsControl::Parts(def) => def,
            };
            if !def.enabled {
                continue;
            }
            for name in [def.var_lr.as_deref(), def.var_ud.as_deref(), def.var_lrm.as_deref()]
                .into_iter()
                .flatten()
                .filter(|name| !name.is_empty())
            {
                let value = self
                    .variables
                    .get(name)
                    .map(|state| state.value)
                    .unwrap_or(0.0);
                self.pre_physics_output_values
                    .entry(name.to_owned())
                    .or_insert(value);
            }
        }
    }

    fn evaluate_physics_controls(&mut self, delta_ticks: f32) {
        if delta_ticks <= PHYSICS_EPSILON_TICKS {
            return;
        }
        let pipeline = self.runtime_pipeline.clone();
        let wind = self.wind.clone();
        let mut bust_idx = 0;
        let mut pend_idx = 0;
        for control in &pipeline.physics_controls {
            match control {
                PhysicsControl::Bust(def) => {
                    let (anchor, _) = self.layer_world_pose(def.base_layer.as_deref());
                    let anchor = self.physics_anchor_with_player_transform(anchor);
                    let outer_force = self.outer_force("bust");
                    let scale = self.bust_scale;
                    // sub_10273DA0 passes MMotionPlayer's effective player
                    // rotation (sub_10276980, converted to radians) plus the
                    // EPRotateControl/OuterRot value (sub_102726C0). The
                    // baseLayer's own local affine angle is not used here.
                    let physics_angle = self.rot + self.outer_rot.to_radians();
                    if let Some(state) = self.bust_states.get_mut(bust_idx) {
                        step_bust_physics(
                            state,
                            def,
                            delta_ticks,
                            anchor,
                            physics_angle,
                            outer_force,
                            scale,
                            &mut self.variables,
                        );
                    }
                    bust_idx += 1;
                }
                PhysicsControl::Hair(def) => {
                    let (anchor, _) = self.layer_world_pose(def.base_layer.as_deref());
                    let anchor = self.physics_anchor_with_player_transform(anchor);
                    let outer_force = self.outer_force("hair");
                    let scale = self.hair_scale;
                    let physics_angle = self.rot + self.outer_rot.to_radians();
                    if let Some(state) = self.hair_states.get_mut(pend_idx) {
                        step_hair_physics(
                            state,
                            def,
                            delta_ticks,
                            anchor,
                            physics_angle,
                            outer_force,
                            wind.as_ref(),
                            scale,
                            &mut self.variables,
                        );
                    }
                    pend_idx += 1;
                }
                PhysicsControl::Parts(def) => {
                    let (anchor, _) = self.layer_world_pose(def.base_layer.as_deref());
                    let anchor = self.physics_anchor_with_player_transform(anchor);
                    let outer_force = self.outer_force("parts");
                    let scale = self.parts_scale;
                    let physics_angle = self.rot + self.outer_rot.to_radians();
                    if let Some(state) = self.hair_states.get_mut(pend_idx) {
                        step_hair_physics(
                            state,
                            def,
                            delta_ticks,
                            anchor,
                            physics_angle,
                            outer_force,
                            wind.as_ref(),
                            scale,
                            &mut self.variables,
                        );
                    }
                    pend_idx += 1;
                }
            }
        }
    }

    fn progress_difference_timeline_targets_for(&mut self, timeline_name: &str, delta_ticks: f32) {
        if !delta_ticks.is_finite() || delta_ticks <= 0.0 {
            return;
        }
        let Some(variables) = self.timeline_diff_variables.get_mut(timeline_name) else {
            return;
        };
        for state in variables.values_mut() {
            let Some(mut target) = state.target.take() else {
                continue;
            };
            target.elapsed_ticks =
                (target.elapsed_ticks + delta_ticks).min(target.duration_ticks.max(0.0));
            let t = if target.duration_ticks <= 0.0 {
                1.0
            } else {
                (target.elapsed_ticks / target.duration_ticks).clamp(0.0, 1.0)
            };
            let eased = native_control_easing(t, target.easing);
            state.value = target.start_value
                + (target.target_value - target.start_value) * eased;
            if t < 1.0 {
                state.target = Some(target);
            }
        }
    }

    fn progress_ticks_internal(&mut self, delta_ticks: f32, include_physics: bool) {
        if !delta_ticks.is_finite() || delta_ticks <= 0.0 {
            return;
        }
        // Native pause freezes MMotionPlayer effective time and all timed
        // control transitions.  The old Rust path advanced elapsed_ticks before
        // checking pause, so ordinary motion continued while the UI said Pause.
        if self.paused {
            return;
        }
        self.elapsed_ticks += delta_ticks;
        self.progress_active_timelines(delta_ticks);
        self.progress_outer_rot(delta_ticks);
        self.progress_outer_forces(delta_ticks);

        if include_physics {
            self.evaluate_runtime_pipeline(delta_ticks);
        } else {
            // The player still advances Eye/Eyebrow/Mouth/Selector/Transition/
            // Loop/Wind with the real host delta when physics is evaluated in a
            // separate post-scene pass. Passing 0 here froze every recovered
            // controller in normal eluna_player/SDK playback.
            let was_enabled = self.physics_enabled;
            self.physics_enabled = false;
            self.evaluate_runtime_pipeline(delta_ticks);
            self.physics_enabled = was_enabled;
        }
        self.modified = true;

        for state in self.variables.values_mut() {
            let Some(mut target) = state.target.take() else {
                continue;
            };

            target.elapsed_ticks =
                (target.elapsed_ticks + delta_ticks).min(target.duration_ticks.max(0.0));
            let t = if target.duration_ticks <= 0.0 {
                1.0
            } else {
                (target.elapsed_ticks / target.duration_ticks).clamp(0.0, 1.0)
            };
            let eased = preview_easing(t, target.easing);
            state.value = target.start_value
                + (target.target_value - target.start_value) * eased;

            if t < 1.0 {
                state.target = Some(target);
            }
        }
    }

    fn progress_wind(&mut self, delta_ticks: f32) {
        let Some(wind) = self.wind.as_mut() else {
            return;
        };
        if !delta_ticks.is_finite() || delta_ticks <= 0.0 {
            return;
        }

        // EPWindControl::step, recovered from sub_1021AAE0.
        wind.elapsed_ticks += delta_ticks;
        wind.spawn_accumulator += delta_ticks;
        while wind.spawn_accumulator >= 0.0 {
            wind.spawn_accumulator -= 1.0;
            if rand::random::<f32>() < 0.0625 {
                if let Some(pulse) = wind.pulses.iter_mut().find(|pulse| !pulse.active) {
                    pulse.active = true;
                    pulse.position = wind.start;
                    pulse.power = wind.pow_min
                        + (wind.pow_max - wind.pow_min) * rand::random::<f32>();
                }
            }
        }

        for pulse in &mut wind.pulses {
            if !pulse.active {
                continue;
            }
            pulse.position += wind.signed_speed * delta_ticks;
            if (wind.signed_speed > 0.0 && pulse.position > wind.goal)
                || (wind.signed_speed < 0.0 && wind.goal > pulse.position)
            {
                pulse.active = false;
            }
        }
    }

    fn progress_outer_forces(&mut self, delta_ticks: f32) {
        for state in self.outer_force_states.values_mut() {
            step_vector2_transition(state, delta_ticks);
        }
    }

    fn progress_outer_rot(&mut self, delta_ticks: f32) {
        let Some(mut target) = self.outer_rot_target.take() else {
            return;
        };
        target.elapsed_ticks =
            (target.elapsed_ticks + delta_ticks.max(0.0)).min(target.duration_ticks.max(0.0));
        let t = if target.duration_ticks <= 0.0 {
            1.0
        } else {
            (target.elapsed_ticks / target.duration_ticks).clamp(0.0, 1.0)
        };
        // EPRotateControl::step (sub_1020B590) receives the exponent produced
        // by the same sub_1026AD10 API conversion as other timed controls.
        let eased = native_control_easing(t, target.easing);
        self.outer_rot = normalize_degrees(
            target.start_value + (target.target_value - target.start_value) * eased,
        );
        // Native completion tolerance is 1e-4.
        if t < 0.9999 {
            self.outer_rot_target = Some(target);
        } else {
            self.outer_rot = normalize_degrees(target.target_value);
        }
    }

    fn layer_world_pose(&self, base_layer: Option<&str>) -> ([f32; 3], f32) {
        let Some(base_layer) = base_layer.filter(|s| !s.is_empty()) else {
            return ([0.0, 0.0, 0.0], 0.0);
        };
        if let Some(layer) = self.scene.layer_states.iter().find(|layer| {
            layer.path == base_layer
                || layer.draw_frame_info.layer_label.as_deref() == Some(base_layer)
                || layer.path.ends_with(base_layer)
        }) {
            let m = layer.transform;
            let angle = m[2].atan2(m[0]);
            // Native physics controls anchor to the layer's finalized StepFrame
            // XYZ, not the projected affine translation.  This distinction is
            // essential for coordinate==1 (XZ) layers, where Y is preserved
            // while the 2x2 matrix acts on X/Z.
            return (layer.position, angle);
        }
        let sprite = self.scene.sprites.iter().find(|sprite| {
            sprite.draw_frame_info.path == base_layer
                || sprite.label.as_deref() == Some(base_layer)
                || sprite.draw_frame_info.layer_label.as_deref() == Some(base_layer)
                || sprite.draw_frame_info.path.ends_with(base_layer)
        });
        let Some(sprite) = sprite else {
            return ([0.0, 0.0, 0.0], 0.0);
        };
        let m = sprite.world_transform;
        let x = m[0] * sprite.center_x + m[1] * sprite.center_y + m[4];
        let y = m[2] * sprite.center_x + m[3] * sprite.center_y + m[5];
        let angle = m[2].atan2(m[0]);
        ([x, y, 0.0], angle)
    }
}

impl EmotePlayerControl for ElunaPlayer {
    fn show(&mut self) {
        self.shown = true;
    }

    fn hide(&mut self) {
        self.shown = false;
    }

    fn progress_ticks(&mut self, delta_ticks: f32) {
        self.progress_ticks_internal(delta_ticks, true);
    }

    fn render(&mut self) {}

    fn coord(&self) -> [f32; 2] {
        self.coord
    }

    fn set_coord(&mut self, x: f32, y: f32) {
        if x.is_finite() && y.is_finite() {
            self.coord = [x, y];
            self.modified = true;
            self.record_api_call(
                "SetCoord",
                vec![x.to_string(), y.to_string(), "0".to_owned(), "0".to_owned()],
            );
        }
    }

    fn scale(&self) -> f32 {
        self.scale
    }

    fn set_scale(&mut self, scale: f32) {
        if scale.is_finite() && scale > 0.0 {
            self.scale = scale;
            self.modified = true;
            self.record_api_call(
                "SetScale",
                vec![scale.to_string(), "0".to_owned(), "0".to_owned()],
            );
        }
    }

    fn rot(&self) -> f32 {
        self.rot
    }

    fn set_rot(&mut self, rot: f32) {
        if rot.is_finite() {
            self.rot = rot;
            self.modified = true;
            self.record_api_call(
                "SetRot",
                vec![rot.to_string(), "0".to_owned(), "0".to_owned()],
            );
        }
    }

    fn set_variable_timed(&mut self, name: &str, value: f32, time_ticks: f32, easing: f32) {
        if let Some(target_value) =
            self.set_variable_timed_internal(name, value, time_ticks, easing)
        {
            self.record_api_call(
                "SetVariable",
                vec![
                    name.to_owned(),
                    target_value.to_string(),
                    time_ticks.max(0.0).to_string(),
                    easing.to_string(),
                ],
            );
        }
    }

    fn set_variable_diff(
        &mut self,
        module: &str,
        name: &str,
        value: f32,
        time_ticks: f32,
        easing: f32,
    ) {
        if module.is_empty() || name.is_empty() {
            return;
        }
        let full_name = format!("{module}/{name}");
        if self
            .set_variable_timed_internal(&full_name, value, time_ticks, easing)
            .is_some()
        {
            self.record_api_call(
                "SetVariableDiff",
                vec![
                    module.to_owned(),
                    name.to_owned(),
                    value.to_string(),
                    time_ticks.max(0.0).to_string(),
                    easing.to_string(),
                ],
            );
        }
    }

    fn play_timeline(&mut self, name: &str, mode: TimelinePlayMode) {
        if name.is_empty() || !self.timelines.contains_key(name) {
            return;
        }

        // MEmotePlayer::PlayTimeline sub_10273220: flag bit 0 is PARALLEL.
        // Without it the previous main timeline is stopped before starting the
        // requested one. DIFFERENCE (bit 1) is orthogonal to this decision.
        if (mode.flags & 1) == 0 {
            let old_main: Vec<String> = self
                .active_timelines
                .keys()
                .filter(|active_name| active_name.as_str() != name)
                .cloned()
                .collect();
            for old in old_main {
                self.active_timelines.remove(&old);
                self.active_timeline_states.remove(&old);
                self.timeline_blend_ratios.remove(&old);
                self.timeline_diff_variables.remove(&old);
            }
        }

        self.active_timelines.insert(name.to_owned(), mode);
        let variable_count = self
            .timelines
            .get(name)
            .map_or(0, |timeline| timeline.variables.len());

        // sub_10270340 reinitializes an existing Timeline runtime as well as a
        // newly allocated one: blend output +36 = 1.0, stop flag +40 = 0, and
        // every ordinary DIFFERENCE variable transition is reset to zero.
        self.timeline_blend_ratios.insert(name.to_owned(), 1.0);
        self.timeline_diff_variables.remove(name);
        self.active_timeline_states.insert(
            name.to_owned(),
            ActiveTimelineState {
                mode,
                elapsed_ticks: 0.0,
                frame_indices: vec![usize::MAX; variable_count],
                blend_ratio: 1.0,
                blend_target: None,
                blend_queue: VecDeque::new(),
                stop_when_blend_idle: false,
            },
        );
        // Native PlayTimeline initializes the runtime and seeks it to zero.
        self.seek_active_timeline(name, 0.0);
        self.evaluate_runtime_pipeline(0.0);
        self.modified = true;
        self.record_api_call(
            "PlayTimeline",
            vec![name.to_owned(), mode.flags.to_string()],
        );
    }

    fn stop_timeline(&mut self, name: &str) {
        if name.is_empty() {
            self.active_timelines.clear();
            self.active_timeline_states.clear();
            self.timeline_blend_ratios.clear();
            self.timeline_diff_variables.clear();
        } else {
            self.active_timelines.remove(name);
            self.active_timeline_states.remove(name);
            self.timeline_blend_ratios.remove(name);
            self.timeline_diff_variables.remove(name);
        }
        // Native StopTimeline removes runtime state; it does not reset every
        // variable touched by any timeline back to metadata defaults.
        self.evaluate_runtime_pipeline(0.0);
        self.modified = true;
        self.record_api_call("StopTimeline", vec![name.to_owned()]);
    }

    fn set_timeline_blend_ratio(
        &mut self,
        name: &str,
        value: f32,
        time_ticks: f32,
        easing: f32,
        stop_when_done: bool,
    ) {
        if name.is_empty() || !value.is_finite() {
            return;
        }

        // sub_10277E30 looks up the active Timeline runtime and immediately
        // returns when runtime+4 (its EPTransitionControl) is null.  It does
        // not implicitly PlayTimeline or manufacture an inactive runtime.
        let queuing = self.queuing;
        let Some(entry) = self.active_timeline_states.get_mut(name) else {
            return;
        };

        set_timeline_blend_transition(entry, value, time_ticks, easing, queuing);
        // Runtime +40 is stored independently after EPTransitionControl::Set.
        // It means "erase this timeline when the blend transition is idle",
        // not "stop only if the target ratio eventually reaches zero".
        entry.stop_when_blend_idle = stop_when_done;
        let next_ratio = entry.blend_ratio;
        self.timeline_blend_ratios
            .insert(name.to_owned(), next_ratio);
        self.modified = true;
        self.record_api_call(
            "SetTimelineBlendRatio",
            vec![
                name.to_owned(),
                value.to_string(),
                time_ticks.max(0.0).to_string(),
                easing.to_string(),
                stop_when_done.to_string(),
            ],
        );
    }

    fn fade_in_timeline(&mut self, name: &str, time_ticks: f32, easing: f32) {
        // Native sub_1026ADD0: if the label is not already playing, start it
        // with flags=3 (PARALLEL|DIFFERENCE), force the blend output to 0
        // immediately, then schedule the requested 0 -> 1 fade.
        if !self.active_timelines.contains_key(name) {
            self.play_timeline(name, TimelinePlayMode::PARALLEL_DIFFERENCE);
            self.set_timeline_blend_ratio(name, 0.0, 0.0, 0.0, false);
        }
        self.set_timeline_blend_ratio(name, 1.0, time_ticks, easing, false);
    }

    fn fade_out_timeline(&mut self, name: &str, time_ticks: f32, easing: f32) {
        self.set_timeline_blend_ratio(name, 0.0, time_ticks, easing, true);
    }

    fn set_outer_force(&mut self, label: &str, x: f32, y: f32, time_ticks: f32, easing: f32) {
        if !matches!(label, "bust" | "hair" | "parts")
            || !x.is_finite()
            || !y.is_finite()
        {
            return;
        }
        let duration = if time_ticks.is_finite() {
            time_ticks.max(0.0)
        } else {
            0.0
        };
        if let Some(state) = self.outer_force_states.get_mut(label) {
            set_vector2_transition_target(
                state,
                [x, y],
                duration,
                easing,
                self.queuing,
            );
        }
        self.modified = true;
        self.record_api_call(
            "SetOuterForce",
            vec![
                label.to_owned(),
                x.to_string(),
                y.to_string(),
                duration.to_string(),
                easing.to_string(),
            ],
        );
    }

    fn outer_force(&self, label: &str) -> [f32; 2] {
        self.outer_force_states
            .get(label)
            .map(|state| state.current)
            .unwrap_or([0.0, 0.0])
    }

    fn set_outer_rot(&mut self, rot: f32, time_ticks: f32, easing: f32) {
        if !rot.is_finite() {
            return;
        }
        let duration = if time_ticks.is_finite() {
            time_ticks.max(0.0)
        } else {
            0.0
        };
        if duration <= f32::EPSILON {
            self.outer_rot = normalize_degrees(rot);
            self.outer_rot_target = None;
        } else {
            self.outer_rot_target = Some(EmoteVariableTarget {
                start_value: self.outer_rot,
                target_value: shortest_angle_target(self.outer_rot, rot),
                elapsed_ticks: 0.0,
                duration_ticks: duration,
                easing,
            });
        }
        self.modified = true;
        self.record_api_call(
            "SetOuterRot",
            vec![
                rot.to_string(),
                duration.to_string(),
                easing.to_string(),
            ],
        );
    }

    fn outer_rot(&self) -> f32 {
        // Keep the internal interpolation domain normalized while preserving
        // the signed degree convention exposed by the SDK/debug UI.
        if self.outer_rot > 180.0 {
            self.outer_rot - 360.0
        } else {
            self.outer_rot
        }
    }

    fn start_wind(
        &mut self,
        mut start: f32,
        mut goal: f32,
        mut speed: f32,
        pow_min: f32,
        pow_max: f32,
    ) {
        let args = vec![
            start.to_string(),
            goal.to_string(),
            speed.to_string(),
            pow_min.to_string(),
            pow_max.to_string(),
        ];
        if !start.is_finite()
            || !goal.is_finite()
            || !speed.is_finite()
            || !pow_min.is_finite()
            || !pow_max.is_finite()
        {
            return;
        }

        // Recovered MEmotePlayer wind setup (sub_10279810): negative speed
        // reverses the endpoints and becomes positive before EPWindControl is
        // created. Degenerate/no-power requests destroy the active control.
        if speed < 0.0 {
            std::mem::swap(&mut start, &mut goal);
            speed = -speed;
        }
        if start == goal || speed == 0.0 || (pow_min == 0.0 && pow_max == 0.0) {
            self.wind = None;
        } else {
            // sub_10279810 converts the public/player-space start, goal and
            // speed into EPWindControl's internal model coordinates by the
            // player scale at creation time. Existing pulses are not later
            // rescaled when SetScale changes.
            let player_scale = if self.scale.is_finite() && self.scale.abs() > f32::EPSILON {
                self.scale.abs()
            } else {
                1.0
            };
            start /= player_scale;
            goal /= player_scale;
            speed /= player_scale;
            let signed_speed = if goal < start { -speed } else { speed };
            self.wind = Some(WindState {
                start,
                goal,
                speed,
                pow_min,
                pow_max,
                elapsed_ticks: 0.0,
                spawn_accumulator: 0.0,
                signed_speed,
                pulses: [WindPulse::default(); 128],
            });
        }
        self.modified = true;
        self.record_api_call("StartWind", args);
    }

    fn stop_wind(&mut self) {
        self.wind = None;
        self.modified = true;
        self.record_api_call("StopWind", Vec::new());
    }

    fn set_transform_order_mask(&mut self, mask: u32) {
        self.transform_order_mask = mask;
    }

    fn transform_order_mask(&self) -> u32 {
        self.transform_order_mask
    }

    fn set_hair_scale(&mut self, scale: f32) {
        if scale.is_finite() && scale >= 0.0 {
            self.hair_scale = scale;
        }
    }

    fn hair_scale(&self) -> f32 {
        self.hair_scale
    }

    fn set_parts_scale(&mut self, scale: f32) {
        if scale.is_finite() && scale >= 0.0 {
            self.parts_scale = scale;
        }
    }

    fn parts_scale(&self) -> f32 {
        self.parts_scale
    }

    fn set_bust_scale(&mut self, scale: f32) {
        if scale.is_finite() && scale >= 0.0 {
            self.bust_scale = scale;
        }
    }

    fn bust_scale(&self) -> f32 {
        self.bust_scale
    }

    fn skip(&mut self) {
        for state in self.variables.values_mut() {
            if let Some(target) = state.target.take() {
                state.value = target.target_value;
            }
        }
        if let Some(target) = self.outer_rot_target.take() {
            self.outer_rot = normalize_degrees(target.target_value);
        }
        for state in self.outer_force_states.values_mut() {
            if state.active {
                state.current = state.target;
            }
            while let Some(command) = state.queue.pop_back() {
                state.current = command.target;
                state.target = command.target;
            }
            state.start = state.current;
            state.active = false;
            state.progress = 1.0;
            state.inv_duration = 0.0;
        }
    }
}

fn normalize_degrees(value: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    // Native angle normalization (sub_1020C030) repeatedly folds by one
    // revolution. rem_euclid is equivalent and also handles API values more
    // than one turn outside the canonical range.
    value.rem_euclid(360.0)
}

fn shortest_angle_target(start: f32, target: f32) -> f32 {
    let mut target = target;
    if target <= start {
        if start - target > 180.0 {
            target += 360.0;
        }
    } else if target - start > 180.0 {
        target -= 360.0;
    }
    target
}

fn set_timeline_blend_transition(
    state: &mut ActiveTimelineState,
    target_value: f32,
    duration_ticks: f32,
    easing: f32,
    queuing: bool,
) {
    // EPTransitionControl::Set (sub_10218860) treats a non-positive duration as
    // an immediate assignment: queued/active commands are cleared first.
    if duration_ticks <= 0.0 || !duration_ticks.is_finite() {
        state.blend_queue.clear();
        state.blend_target = None;
        state.blend_ratio = target_value;
        return;
    }

    let command = TimelineBlendCommand {
        target_value,
        duration_ticks,
        easing,
    };
    if queuing {
        state.blend_queue.push_back(command);
        return;
    }

    // sub_10218860 has one non-replacement optimization: when the active
    // transition already has the same target, a *slower* request than the
    // remaining active duration is ignored. Equal/faster requests restart it.
    if let Some(active) = state.blend_target.as_ref() {
        if active.target_value == target_value {
            let remaining = (active.duration_ticks - active.elapsed_ticks).max(0.0);
            if duration_ticks > remaining {
                return;
            }
        }
    }

    state.blend_queue.clear();
    state.blend_target = None;
    state.blend_queue.push_back(command);
    start_next_timeline_blend(state);
}

fn start_next_timeline_blend(state: &mut ActiveTimelineState) -> bool {
    let Some(command) = state.blend_queue.pop_front() else {
        state.blend_target = None;
        return false;
    };
    state.blend_target = Some(TimelineBlendTarget {
        start_value: state.blend_ratio,
        target_value: command.target_value,
        elapsed_ticks: 0.0,
        duration_ticks: command.duration_ticks,
        easing: command.easing,
    });
    true
}

fn advance_timeline_blend(state: &mut ActiveTimelineState, delta_ticks: f32) -> f32 {
    if !delta_ticks.is_finite() || delta_ticks < 0.0 {
        return state.blend_ratio;
    }

    // Generic EPTransitionControl::Step (sub_102164E0) loops after completing
    // a command: if another queued command exists, it starts that command and
    // applies the same host delta to it in the same Step call.
    loop {
        if state.blend_target.is_none() && !start_next_timeline_blend(state) {
            return state.blend_ratio;
        }

        let Some(mut target) = state.blend_target.take() else {
            return state.blend_ratio;
        };
        target.elapsed_ticks =
            (target.elapsed_ticks + delta_ticks).min(target.duration_ticks.max(0.0));
        let t = if target.duration_ticks <= 0.0 {
            1.0
        } else {
            (target.elapsed_ticks / target.duration_ticks).clamp(0.0, 1.0)
        };
        let eased = preview_easing(t, target.easing);
        // Native SetTimelineBlendRatio passes the caller's float directly to
        // the scalar transition; neither sub_10277E30 nor sub_10275810 clamps
        // the output to [0,1].
        state.blend_ratio =
            target.start_value + (target.target_value - target.start_value) * eased;
        if t < 1.0 - 0.0001 {
            state.blend_target = Some(target);
            return state.blend_ratio;
        }

        state.blend_ratio = target.target_value;
        state.blend_target = None;
        if state.blend_queue.is_empty() {
            return state.blend_ratio;
        }
    }
}

fn timeline_blend_transition_idle(state: &ActiveTimelineState) -> bool {
    state.blend_target.is_none() && state.blend_queue.is_empty()
}

fn set_vector2_transition_target(
    state: &mut Vector2TransitionState,
    target: [f32; 2],
    duration_ticks: f32,
    easing: f32,
    queuing: bool,
) {
    let duration = duration_ticks.max(0.0);
    let easing_exponent = native_easing_exponent(easing);
    if duration <= f32::EPSILON {
        state.queue.clear();
        state.current = target;
        state.start = target;
        state.target = target;
        state.active = false;
        state.inv_duration = 0.0;
        state.easing_exponent = easing_exponent;
        state.progress = 1.0;
        return;
    }

    let command = Vector2TransitionCommand {
        target,
        duration_ticks: duration,
        easing_exponent,
    };
    if queuing {
        state.queue.push_back(command);
        return;
    }

    // sub_10218860 ignores a same-target request only when it is slower than
    // the active command's remaining duration. Any different target or equal/
    // faster request replaces the active transition and queued commands.
    if state.active && state.target == target && state.inv_duration > 0.0 {
        let remaining = (1.0 - state.progress).max(0.0) / state.inv_duration;
        if duration > remaining {
            return;
        }
    }
    state.queue.clear();
    state.active = false;
    state.queue.push_back(command);
    start_next_vector2_transition(state);
}

fn start_next_vector2_transition(state: &mut Vector2TransitionState) -> bool {
    let Some(command) = state.queue.pop_front() else {
        state.active = false;
        return false;
    };
    state.start = state.current;
    state.target = command.target;
    state.inv_duration = if command.duration_ticks > f32::EPSILON {
        1.0 / command.duration_ticks
    } else {
        0.0
    };
    state.easing_exponent = command.easing_exponent;
    state.progress = 0.0;
    state.active = true;
    true
}

fn step_vector2_transition(state: &mut Vector2TransitionState, delta_ticks: f32) {
    if !delta_ticks.is_finite() || delta_ticks < 0.0 {
        return;
    }
    loop {
        if !state.active && !start_next_vector2_transition(state) {
            return;
        }
        state.progress += delta_ticks * state.inv_duration;
        if state.progress >= 0.9999 {
            state.progress = 1.0;
            state.current = state.target;
            state.active = false;
            // sub_102164E0 immediately consumes the next queued command in the
            // same Step call; the host delta is intentionally reused.
            if state.queue.is_empty() {
                return;
            }
            continue;
        }
        let eased = state
            .progress
            .clamp(0.0, 1.0)
            .powf(state.easing_exponent);
        state.current = [
            state.start[0] + (state.target[0] - state.start[0]) * eased,
            state.start[1] + (state.target[1] - state.start[1]) * eased,
        ];
        return;
    }
}

fn native_easing_exponent(easing: f32) -> f32 {
    // sub_1026AD10 is the shared public-API easing conversion used before
    // EPTransitionControl/EPRotateControl receive a timed command.
    if !easing.is_finite() || easing == 0.0 {
        1.0
    } else if easing < 0.0 {
        1.0 / (1.0 - easing)
    } else {
        easing + 1.0
    }
}

fn preview_easing(t: f32, easing: f32) -> f32 {
    // The old implementation used a smoothstep preview approximation.  The
    // DLL converts the authored/API easing with sub_1026AD10 and all of these
    // scalar transition paths then evaluate pow(progress, exponent).
    t.clamp(0.0, 1.0).powf(native_easing_exponent(easing))
}

fn merge_timeline_variable_range(info: &mut EmoteVariableInfo, variable: &EmoteTimelineVariable) {
    for frame in &variable.frames {
        if !frame.hold {
            merge_range_value(&mut info.min_value, &mut info.max_value, frame.value);
        }
    }
}

fn merge_range_value(min_value: &mut Option<f32>, max_value: &mut Option<f32>, value: f32) {
    if !value.is_finite() {
        return;
    }
    *min_value = Some(min_value.map_or(value, |min| min.min(value)));
    *max_value = Some(max_value.map_or(value, |max| max.max(value)));
}

fn ensure_variable_range_entry<'a>(
    variables: &'a mut BTreeMap<String, EmoteVariableState>,
    name: &str,
    default_value: f32,
) -> &'a mut EmoteVariableState {
    variables.entry(name.to_owned()).or_insert_with(|| EmoteVariableState {
        info: EmoteVariableInfo {
            name: name.to_owned(),
            default_value,
            min_value: None,
            max_value: None,
            frames: Vec::new(),
        },
        value: default_value,
        target: None,
    })
}

fn augment_variable_ranges_from_controls(
    variables: &mut BTreeMap<String, EmoteVariableState>,
    pipeline: &EmoteRuntimePipeline,
) {
    for control in &pipeline.eye_controls {
        let state = ensure_variable_range_entry(variables, &control.label, control.begin_frame as f32);
        let mut values = vec![control.begin_frame as f32, control.end_frame as f32];
        for edge in &control.edge {
            values.extend_from_slice(edge);
        }
        for node in &control.node {
            values.extend(node.iter().copied());
        }
        for value in values {
            merge_range_value(&mut state.info.min_value, &mut state.info.max_value, value);
        }
    }
    for control in &pipeline.eyebrow_controls {
        let state = ensure_variable_range_entry(variables, &control.label, control.begin_frame as f32);
        let mut values = vec![control.begin_frame as f32];
        for edge in &control.edge {
            values.extend_from_slice(edge);
        }
        for node in &control.node {
            values.extend(node.iter().copied());
        }
        for value in values {
            merge_range_value(&mut state.info.min_value, &mut state.info.max_value, value);
        }
    }
    for control in &pipeline.selector_controls {
        let max_index = control.option_list.len().saturating_sub(1) as f32;
        let state = ensure_variable_range_entry(variables, &control.label, 0.0);
        merge_range_value(&mut state.info.min_value, &mut state.info.max_value, 0.0);
        merge_range_value(&mut state.info.min_value, &mut state.info.max_value, max_index);
        for option in &control.option_list {
            if option.label.is_empty() {
                continue;
            }
            let transition = ensure_variable_range_entry(variables, &option.label, option.off_value);
            merge_range_value(
                &mut transition.info.min_value,
                &mut transition.info.max_value,
                option.off_value,
            );
            merge_range_value(
                &mut transition.info.min_value,
                &mut transition.info.max_value,
                option.on_value,
            );
        }
    }
    for control in &pipeline.loop_controls {
        let Some(name) = control.var_loop.as_deref().filter(|name| !name.is_empty()) else {
            continue;
        };
        let default = control.transition_list.first().map(|item| item.start).unwrap_or(0.0);
        let state = ensure_variable_range_entry(variables, name, default);
        for item in &control.transition_list {
            merge_range_value(&mut state.info.min_value, &mut state.info.max_value, item.start);
            merge_range_value(&mut state.info.min_value, &mut state.info.max_value, item.end);
        }
    }
    // ClampControl authors one common [min,max] domain for the LR/UD pair.
    // These are genuine authored ranges, unlike Bust/Pend solver outputs.
    for control in &pipeline.clamp_controls {
        if !control.enabled {
            continue;
        }
        for name in [&control.var_lr, &control.var_ud] {
            if name.is_empty() {
                continue;
            }
            let state = ensure_variable_range_entry(variables, name, 0.0);
            merge_range_value(&mut state.info.min_value, &mut state.info.max_value, control.min);
            merge_range_value(&mut state.info.min_value, &mut state.info.max_value, control.max);
        }
    }
}

pub fn collect_emote_runtime_pipeline(psb: &PsbFile) -> EmoteRuntimePipeline {
    let mut pipeline = EmoteRuntimePipeline::default();
    let Some(metadata) = psb.root.field("metadata") else {
        return pipeline;
    };

    // sub_1026EC30 stores metadata["mirror"] in the player before parsing
    // mirrorControl.  sub_10271C20 returns false immediately when this flag is
    // clear, so the pattern list alone never enables mirroring.
    pipeline.mirror_enabled = metadata.field_i64("mirror").unwrap_or(0) != 0;
    pipeline.instant_variables = metadata
        .field("instantVariableList")
        .and_then(PsbValue::as_list)
        .map(|items| {
            items
                .iter()
                .filter_map(PsbValue::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    pipeline.selector_controls = parse_selector_controls(metadata.field("selectorControl"));
    pipeline.clamp_controls = parse_clamp_controls(metadata.field("clampControl"));
    pipeline.loop_controls = parse_loop_controls(metadata.field("loopControl"));
    pipeline.mirror_control = parse_mirror_control(metadata.field("mirrorControl"));
    pipeline.stereovision_control = parse_stereovision_control(metadata.field("stereovisionControl"));
    pipeline.transition_controls = parse_transition_controls(metadata.field("transitionControl"));
    pipeline.physics_controls.extend(parse_physics_controls(
        metadata.field("bustControl"),
        PhysicsControlKind::Bust,
    ));
    pipeline.physics_controls.extend(parse_physics_controls(
        metadata.field("hairControl"),
        PhysicsControlKind::Hair,
    ));
    pipeline.physics_controls.extend(parse_physics_controls(
        metadata.field("partsControl"),
        PhysicsControlKind::Parts,
    ));
    pipeline.parts_controls = parse_opaque_controls(metadata.field("partsControl"));
    pipeline.eye_controls = parse_eye_controls(metadata.field("eyeControl"));
    pipeline.eyebrow_controls = parse_eyebrow_controls(metadata.field("eyebrowControl"));
    pipeline.mouth_controls = parse_mouth_controls(metadata.field("mouthControl"));

    // This DLL revision does not parse a top-level `physicsVariableList` in
    // MEmotePlayer::Init (sub_1026EC30). Bust/Hair/Parts output variables are
    // runtime controller references, so absence of that non-native key is not
    // a parity failure.
    pipeline
}

fn parse_selector_controls(value: Option<&PsbValue>) -> Vec<SelectorControl> {
    value
        .and_then(PsbValue::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|control| {
            let enabled = control.field_i64("enabled").unwrap_or(1) != 0;
            if !enabled {
                return None;
            }
            let label = control.field_str("label")?.to_owned();
            let option_list = control
                .field("optionList")
                .and_then(PsbValue::as_list)
                .unwrap_or(&[])
                .iter()
                .filter_map(|option| {
                    Some(SelectorOption {
                        label: option.field_str("label")?.to_owned(),
                        off_value: option.field_f32("offValue")?,
                        on_value: option.field_f32("onValue")?,
                    })
                })
                .collect();
            Some(SelectorControl {
                label,
                enabled,
                option_list,
            })
        })
        .collect()
}

fn parse_clamp_controls(value: Option<&PsbValue>) -> Vec<ClampControl> {
    value
        .and_then(PsbValue::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|control| {
            Some(ClampControl {
                label: control.field_str("label").unwrap_or("").to_owned(),
                enabled: control.field_i64("enabled").unwrap_or(1) != 0,
                kind: control.field_i64("type")?,
                var_lr: control.field_str("var_lr")?.to_owned(),
                var_ud: control.field_str("var_ud")?.to_owned(),
                min: control.field_f32("min")?,
                max: control.field_f32("max")?,
            })
        })
        .collect()
}

fn parse_loop_controls(value: Option<&PsbValue>) -> Vec<LoopControl> {
    value
        .and_then(PsbValue::as_list)
        .unwrap_or(&[])
        .iter()
        .map(|control| {
            let transition_list = control
                .field("transitionList")
                .and_then(PsbValue::as_list)
                .unwrap_or(&[])
                .iter()
                .filter_map(parse_loop_transition)
                .collect();
            LoopControl {
                label: control.field_str("label").map(str::to_owned),
                enabled: control.field_i64("enabled").unwrap_or(1) != 0,
                var_loop: control.field_str("var_loop").map(str::to_owned),
                transition_list,
            }
        })
        .collect()
}

fn parse_loop_transition(value: &PsbValue) -> Option<LoopTransition> {
    if let Some(items) = value.as_list() {
        return Some(LoopTransition {
            start: items.first().and_then(PsbValue::as_f32)?,
            end: items.get(1).and_then(PsbValue::as_f32)?,
            duration_ticks: items.get(2).and_then(PsbValue::as_f32)?.max(0.0),
        });
    }
    Some(LoopTransition {
        start: value.field_f32("start")?,
        end: value.field_f32("end")?,
        duration_ticks: value
            .field_f32("duration")
            .or_else(|| value.field_f32("duration_ticks"))?
            .max(0.0),
    })
}

fn parse_mirror_control(value: Option<&PsbValue>) -> Option<MirrorControl> {
    let value = value?;
    let variable_match_list = value
        .field("variableMatchList")
        .and_then(PsbValue::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(PsbValue::as_str)
        .map(str::to_owned)
        .collect();
    Some(MirrorControl {
        variable_match_list,
    })
}

fn parse_stereovision_control(value: Option<&PsbValue>) -> Option<EmoteStereovisionControl> {
    let value = value?;
    let variable_match_list = value
        .field("variableMatchList")
        .and_then(PsbValue::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(PsbValue::as_str)
        .map(str::to_owned)
        .collect();
    Some(EmoteStereovisionControl { variable_match_list })
}

fn parse_transition_controls(value: Option<&PsbValue>) -> Vec<TransitionControl> {
    value
        .and_then(PsbValue::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|control| {
            let enabled = control.field_i64("enabled").unwrap_or(1) != 0;
            if !enabled {
                return None;
            }
            Some(TransitionControl {
                label: control.field_str("label")?.to_owned(),
                enabled,
            })
        })
        .collect()
}

fn parse_graph_edges(value: Option<&PsbValue>) -> Vec<[f32; 2]> {
    value
        .and_then(PsbValue::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|edge| {
            let items = edge.as_list()?;
            Some([
                items.first()?.as_f32()?,
                items.get(1)?.as_f32()?,
            ])
        })
        .collect()
}

fn parse_graph_nodes(value: Option<&PsbValue>) -> Vec<Vec<f32>> {
    value
        .and_then(PsbValue::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|node| {
            Some(
                node.as_list()?
                    .iter()
                    .filter_map(PsbValue::as_f32)
                    .collect::<Vec<_>>(),
            )
        })
        .collect()
}

fn parse_eye_controls(value: Option<&PsbValue>) -> Vec<EyeControl> {
    value
        .and_then(PsbValue::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|control| {
            let enabled = control.field_i64("enabled").unwrap_or(1) != 0;
            if !enabled {
                return None;
            }
            Some(EyeControl {
                label: control.field_str("label")?.to_owned(),
                enabled,
                begin_frame: control.field_i64("beginFrame")? as i32,
                end_frame: control.field_i64("endFrame")? as i32,
                blink_interval_min: control.field_f32("blinkIntervalMin")?,
                blink_interval_max: control.field_f32("blinkIntervalMax")?,
                blink_frame_count: control.field_f32("blinkFrameCount")?,
                blink_enabled: control.field_i64("blinkEnabled").unwrap_or(1) != 0,
                edge: parse_graph_edges(control.field("edge")),
                node: parse_graph_nodes(control.field("node")),
            })
        })
        .collect()
}

fn parse_eyebrow_controls(value: Option<&PsbValue>) -> Vec<EyebrowControl> {
    value
        .and_then(PsbValue::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|control| {
            let enabled = control.field_i64("enabled").unwrap_or(1) != 0;
            if !enabled {
                return None;
            }
            Some(EyebrowControl {
                label: control.field_str("label")?.to_owned(),
                enabled,
                begin_frame: control.field_i64("beginFrame")? as i32,
                edge: parse_graph_edges(control.field("edge")),
                node: parse_graph_nodes(control.field("node")),
            })
        })
        .collect()
}

fn parse_mouth_controls(value: Option<&PsbValue>) -> Vec<MouthControl> {
    value
        .and_then(PsbValue::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|control| {
            let enabled = control.field_i64("enabled").unwrap_or(1) != 0;
            if !enabled {
                return None;
            }
            Some(MouthControl {
                label: control.field_str("label")?.to_owned(),
                talk_label: control.field_str("talkLabel")?.to_owned(),
                enabled,
                begin_frame: control.field_i64("beginFrame")? as i32,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PhysicsControlKind {
    Bust,
    Hair,
    Parts,
}

fn parse_physics_controls(
    value: Option<&PsbValue>,
    kind: PhysicsControlKind,
) -> Vec<PhysicsControl> {
    value
        .and_then(PsbValue::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|control| {
            let definition = PhysicsControlDefinition {
                label: control.field_str("label").unwrap_or("").to_owned(),
                enabled: control.field_i64("enabled").unwrap_or(1) != 0,
                base_layer: control.field_str("baseLayer").map(str::to_owned),
                parameter: control.field_str("parameter").map(str::to_owned),
                var_lr: control.field_str("var_lr").map(str::to_owned),
                var_ud: control.field_str("var_ud").map(str::to_owned),
                var_lrm: control.field_str("var_lrm").map(str::to_owned),
                fields: object_to_map(control)?,
            };
            Some(match kind {
                PhysicsControlKind::Bust => PhysicsControl::Bust(definition),
                PhysicsControlKind::Hair => PhysicsControl::Hair(definition),
                PhysicsControlKind::Parts => PhysicsControl::Parts(definition),
            })
        })
        .collect()
}

fn parse_opaque_controls(value: Option<&PsbValue>) -> Vec<OpaqueControl> {
    value
        .and_then(PsbValue::as_list)
        .unwrap_or(&[])
        .iter()
        .filter_map(|control| {
            Some(OpaqueControl {
                label: control.field_str("label").map(str::to_owned),
                enabled: control.field_i64("enabled").unwrap_or(1) != 0,
                fields: object_to_map(control)?,
            })
        })
        .collect()
}

fn object_to_map(value: &PsbValue) -> Option<BTreeMap<String, PsbValue>> {
    Some(value.as_object()?.iter().cloned().collect())
}

fn native_random_range(min: f32, max: f32) -> f32 {
    min + (max - min) * rand::random::<f32>()
}

fn init_eye_states(pipeline: &EmoteRuntimePipeline) -> Vec<EyeControlState> {
    pipeline
        .eye_controls
        .iter()
        .map(|control| {
            let begin = control.begin_frame as f32;
            EyeControlState {
                graph: GraphControlState::new(begin),
                blink_state: EyeBlinkState::Idle,
                blink_frame: begin,
                // EPEyeControl ctor calls the same random interval helper used
                // after the closed-hold state.  Do not start all characters at
                // the minimum interval; the native player de-synchronizes them.
                blink_timer: native_random_range(
                    control.blink_interval_min,
                    control.blink_interval_max,
                ),
            }
        })
        .collect()
}

fn init_eyebrow_states(pipeline: &EmoteRuntimePipeline) -> Vec<EyebrowControlState> {
    pipeline
        .eyebrow_controls
        .iter()
        .map(|control| EyebrowControlState {
            graph: GraphControlState::new(control.begin_frame as f32),
        })
        .collect()
}

fn init_mouth_states(pipeline: &EmoteRuntimePipeline) -> Vec<MouthControlState> {
    pipeline
        .mouth_controls
        .iter()
        .map(|control| MouthControlState {
            begin_frame: control.begin_frame,
            // EPMouthControl ctor stores the float at 0x10504c00 into current;
            // the DLL image contains 0.0f at that address.
            current: 0.0,
            active: None,
            queue: VecDeque::new(),
        })
        .collect()
}

fn init_transition_states(pipeline: &EmoteRuntimePipeline) -> Vec<ScalarTransitionState> {
    pipeline
        .transition_controls
        .iter()
        .map(|_| ScalarTransitionState {
            // EPTransitionControl ctor zero-fills current/start/target buffers.
            current: 0.0,
            active: None,
            queue: VecDeque::new(),
        })
        .collect()
}

fn transition_index_by_label(pipeline: &EmoteRuntimePipeline, label: &str) -> Option<usize> {
    pipeline
        .transition_controls
        .iter()
        .position(|control| control.enabled && control.label == label)
}

fn init_selector_states(
    pipeline: &EmoteRuntimePipeline,
    transition_states: &mut [ScalarTransitionState],
) -> Vec<SelectorControlState> {
    pipeline
        .selector_controls
        .iter()
        .map(|control| {
            // EPSelectorControl ctor (sub_1020D0C0) initializes the queue and
            // immediately calls Select(0, 0, 0).  This also initializes every
            // linked option transition to its authored on/off value.
            let mut state = SelectorControlState {
                current: 0,
                active: None,
                queue: VecDeque::new(),
            };
            apply_selector_target(
                control,
                &mut state,
                0.0,
                0.0,
                0.0,
                transition_states,
                pipeline,
            );
            state
        })
        .collect()
}

fn init_loop_states(pipeline: &EmoteRuntimePipeline) -> Vec<LoopControlState> {
    pipeline
        .loop_controls
        .iter()
        .map(|_| LoopControlState {
            index: 0,
            elapsed_ticks: 0.0,
        })
        .collect()
}

/// Initialize EPBustControl state exactly from its serialized `param` block.
///
/// sub_102687F0 / sub_101D50C0 map `op` -> root (+28), `p` -> bob (+52),
/// `pv` -> velocity (+64), and `ofs` -> +76. In particular, `op` is an
/// absolute physics-space root at reset time; the controller does *not* add it
/// to the baseLayer anchor. sub_101D4300 captures root-anchor on its first
/// step and follows later anchor movement with that saved delta.
fn init_bust_states(pipeline: &EmoteRuntimePipeline) -> Vec<BustPhysicsState> {
    let mut out = Vec::new();
    for control in &pipeline.physics_controls {
        let PhysicsControl::Bust(def) = control else {
            continue;
        };
        let param = def.fields.get("param");
        let root = parse_vec3_field(param, "op");
        let bob = parse_vec3_field(param, "p");
        let vel = parse_vec3_field(param, "pv");
        let param_ofs = param.and_then(|v| v.field_f32("ofs")).unwrap_or(0.0);
        out.push(BustPhysicsState {
            root,
            bob,
            vel,
            ofs: param_ofs,
            group_first_tick: true,
            controller_first_tick: true,
            root_delta: [0.0, 0.0],
            last_anchor: None,
        });
    }
    out
}

/// Restore the exported pendulum state. Without a serialized state, use the
/// constructor's straight, stationary chain. Mixing a serialized equilibrium
/// bias with constructor bob positions produces a large startup impulse.
fn init_hair_states(pipeline: &EmoteRuntimePipeline) -> Vec<HairPhysicsState> {
    let mut out = Vec::new();
    for control in &pipeline.physics_controls {
        let def = match control {
            PhysicsControl::Hair(def) | PhysicsControl::Parts(def) => def,
            _ => continue,
        };
        let lengths = physics_field_f32_list_2(def, "length");
        let param = def.fields.get("param");
        let root = parse_vec3_field(param, "op");
        let rest0 = [root[0], root[1] + lengths[0], root[2]];
        let rest1 = [rest0[0], rest0[1] + lengths[1], rest0[2]];
        let param_ofs = param.and_then(|v| v.field_f32("ofs")).unwrap_or(0.0);
        let vectors = |key: &str, fallback: [[f32; 3]; 2]| {
            let Some(items) = param.and_then(|p| p.field(key)).and_then(PsbValue::as_list)
                .filter(|items| items.len() == 2) else { return fallback; };
            std::array::from_fn(|i| [
                items[i].field_f32("x").unwrap_or(fallback[i][0]),
                items[i].field_f32("y").unwrap_or(fallback[i][1]),
                items[i].field_f32("z").unwrap_or(fallback[i][2]),
            ])
        };
        out.push(HairPhysicsState {
            bob: vectors("p", [rest0, rest1]),
            vel: vectors("pv", [[0.0; 3]; 2]),
            ofs: param_ofs,
            first_tick: true,
            root_offset: root,
            last_anchor: None,
            bend_phase: param.and_then(|p| p.field_f32("bendR")).unwrap_or(0.0),
            bend_power: param.and_then(|p| p.field_f32("bendS")).unwrap_or(0.0),
        });
    }
    out
}

fn parse_vec3_field(parent: Option<&PsbValue>, key: &str) -> [f32; 3] {
    let Some(v) = parent.and_then(|p| p.field(key)) else {
        return [0.0; 3];
    };
    [
        v.field_f32("x").unwrap_or(0.0),
        v.field_f32("y").unwrap_or(0.0),
        v.field_f32("z").unwrap_or(0.0),
    ]
}

fn evaluate_selector_control(
    control: &SelectorControl,
    state: &mut SelectorControlState,
    delta_ticks: f32,
    pipeline: &EmoteRuntimePipeline,
    transition_states: &mut [ScalarTransitionState],
    variables: &mut BTreeMap<String, EmoteVariableState>,
) {
    if !control.enabled {
        return;
    }

    // EPSelectorControl::Step (sub_1020DB50) does not interpolate the selector
    // index.  A queued command changes the selection immediately when it
    // starts, then the control remains busy for the authored duration while
    // the linked EPTransitionControls perform their own cross-fades.
    let guard = state.queue.len().saturating_add(2);
    for _ in 0..guard {
        if state.active.is_none() {
            let Some(command) = state.queue.pop_front() else {
                break;
            };
            apply_selector_target(
                control,
                state,
                command.target,
                command.duration_ticks,
                command.easing,
                transition_states,
                pipeline,
            );
            state.active = Some(ActiveSelectorControl {
                inv_duration: 1.0 / command.duration_ticks,
                progress: 0.0,
            });
        }
        let Some(active) = state.active.as_mut() else {
            break;
        };
        active.progress += delta_ticks * active.inv_duration;
        if active.progress - 1.0 >= -0.000099999997 {
            active.progress = 1.0;
            state.active = None;
            continue;
        }
        break;
    }
    set_evaluated_variable(variables, &control.label, state.current as f32);
}

fn apply_selector_target(
    control: &SelectorControl,
    state: &mut SelectorControlState,
    target: f32,
    duration_ticks: f32,
    easing: f32,
    transition_states: &mut [ScalarTransitionState],
    pipeline: &EmoteRuntimePipeline,
) {
    // sub_1020D910 receives the selector target as int after the queued float
    // is converted with C's truncating float-to-int conversion.
    state.current = target as i32;

    for (option_index, option) in control.option_list.iter().enumerate() {
        let Some(transition_index) = transition_index_by_label(pipeline, &option.label) else {
            // Native parser stores a null pointer for an option whose label did
            // not resolve to an EPTransitionControl and simply skips it here.
            continue;
        };
        let Some(transition) = transition_states.get_mut(transition_index) else {
            continue;
        };

        // sub_1020D910 calls EPTransitionControl::Step(0) before inspecting its
        // current value.  A previously queued command may therefore become the
        // active command even though no time advances.
        advance_scalar_transition_state(transition, 0.0);

        let target_value = if state.current == option_index as i32 {
            option.on_value
        } else {
            option.off_value
        };
        let normalized_distance =
            ((transition.current - target_value) / (option.on_value - option.off_value)).abs();
        let is_animating = transition.active.is_some() || !transition.queue.is_empty();

        // The DLL only retargets a settled option when its current value is not
        // already the desired value.  When retargeting, duration is multiplied
        // by the normalized remaining on/off distance so reversal mid-fade
        // keeps the same transition speed.
        if is_animating || (transition.current - target_value).abs() >= f32::EPSILON {
            set_scalar_transition_target(
                transition,
                target_value,
                duration_ticks * normalized_distance,
                easing,
                false,
            );
        }
    }
}

fn evaluate_clamp_control(
    control: &ClampControl,
    pipeline: &EmoteRuntimePipeline,
    variables: &mut BTreeMap<String, EmoteVariableState>,
) {
    if !control.enabled {
        return;
    }
    let lr = variables
        .get(&control.var_lr)
        .map(|state| state.value)
        .unwrap_or(0.0);
    let ud = variables
        .get(&control.var_ud)
        .map(|state| state.value)
        .unwrap_or(0.0);
    let span = control.max - control.min;
    if !span.is_finite() || span.abs() <= f32::EPSILON {
        return;
    }

    // Recovered from sub_10275CC0. ClampControl operates in a normalized
    // [-1,1]^2 domain, not directly in the authored variable units.
    let mut x = ((lr - control.min) / span) * 2.0 - 1.0;
    let mut y = ((ud - control.min) / span) * 2.0 - 1.0;

    if x != 0.0 && y != 0.0 {
        match control.kind {
            1 => {
                // Native type 1: radial unit-circle clipping.  The DLL uses
                // atan2/cos/sin; dividing by radius is algebraically identical.
                let radius = x.hypot(y);
                if radius > 1.0 {
                    x /= radius;
                    y /= radius;
                }
            }
            0 => {
                // Native type 0 square-to-disc remap.  Keep the operation
                // order from sub_10275CC0 to preserve its edge behaviour.
                let mut q = (x / y).abs();
                if q > 1.0 {
                    q = 1.0 / q;
                }
                let inv = 1.0 / (q * q + 1.0).sqrt();
                x *= inv;
                y *= inv;
                let radius = x.hypot(y);
                if radius > f32::EPSILON {
                    let radial = (radius * std::f32::consts::FRAC_PI_2).sin() / radius;
                    let axis_mix = 1.0 - (q * std::f32::consts::FRAC_PI_2).cos();
                    let scale = (radial - 1.0) * axis_mix + 1.0;
                    x *= scale;
                    y *= scale;
                }
            }
            _ => {}
        }
    }

    let mut next_lr = ((x + 1.0) * 0.5) * span + control.min;
    let next_ud = ((y + 1.0) * 0.5) * span + control.min;
    // sub_10275CC0 asks the same mirror predicate for var_lr immediately
    // before writing it.  var_ud is never sign-flipped here.
    if variable_is_mirrored(pipeline, &control.var_lr) {
        next_lr = -next_lr;
    }
    set_evaluated_variable(variables, &control.var_lr, next_lr);
    set_evaluated_variable(variables, &control.var_ud, next_ud);
}

fn native_control_easing(progress: f32, easing: f32) -> f32 {
    // Public setters first pass easing through sub_1026AD10, then the control
    // step uses the resulting exponent in pow(progress, exponent).  Keep raw
    // API easing in Rust state and apply the conversion here.
    progress
        .clamp(0.0, 1.0)
        .powf(native_easing_exponent(easing))
}


#[derive(Debug, Clone, Copy, PartialEq)]
struct GraphVertex {
    line: usize,
    value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct GraphArc {
    to: usize,
    weight: f32,
    segment: Option<[f32; 2]>,
}

#[derive(Debug, Clone, PartialEq)]
struct GraphRoute {
    segments: Vec<[f32; 2]>,
    total_distance: f32,
}

fn graph_line_contains(edge: &[f32; 2], value: f32) -> bool {
    // EPGraph::find_line (sub_101F4F70) uses inclusive authored bounds.
    edge[0] <= value && value <= edge[1]
}

fn graph_find_line(edges: &[[f32; 2]], value: f32) -> Option<usize> {
    edges.iter().position(|edge| graph_line_contains(edge, value))
}

fn graph_vertex_index(vertices: &[GraphVertex], line: usize, value: f32) -> Option<usize> {
    vertices
        .iter()
        .position(|vertex| vertex.line == line && vertex.value == value)
}

fn build_ep_graph_route(
    edges: &[[f32; 2]],
    nodes: &[Vec<f32>],
    start: f32,
    goal: f32,
) -> GraphRoute {
    // sub_101FBF00/sub_101F5170 special-case two values on the same authored
    // line before exploring junctions.  Preserve that choice even if a graph
    // with unusual zero-cost junctions could offer another numerical route.
    let Some(start_line) = graph_find_line(edges, start) else {
        return GraphRoute {
            segments: vec![[goal, goal]],
            total_distance: 0.0,
        };
    };
    let Some(goal_line) = graph_find_line(edges, goal) else {
        return GraphRoute {
            segments: vec![[goal, goal]],
            total_distance: 0.0,
        };
    };
    if start_line == goal_line {
        return GraphRoute {
            segments: vec![[start, goal]],
            total_distance: (goal - start).abs(),
        };
    }

    // Each authored line is a continuous one-dimensional interval. Cross/node
    // entries join distinct frame values at zero distance. Represent a graph
    // vertex as (line,value), otherwise equal numeric values on unrelated lines
    // would be incorrectly merged.
    let mut line_points = vec![Vec::<f32>::new(); edges.len()];
    line_points[start_line].push(start);
    line_points[goal_line].push(goal);
    for node in nodes {
        for &point in node {
            if let Some(line) = graph_find_line(edges, point) {
                line_points[line].push(point);
            }
        }
    }
    for points in &mut line_points {
        points.sort_by(|a, b| a.total_cmp(b));
        points.dedup_by(|a, b| *a == *b);
    }

    let mut vertices = Vec::<GraphVertex>::new();
    for (line, points) in line_points.iter().enumerate() {
        for &value in points {
            vertices.push(GraphVertex { line, value });
        }
    }
    let Some(start_vertex) = graph_vertex_index(&vertices, start_line, start) else {
        return GraphRoute {
            segments: vec![[goal, goal]],
            total_distance: 0.0,
        };
    };
    let Some(goal_vertex) = graph_vertex_index(&vertices, goal_line, goal) else {
        return GraphRoute {
            segments: vec![[goal, goal]],
            total_distance: 0.0,
        };
    };

    let mut arcs = vec![Vec::<GraphArc>::new(); vertices.len()];
    for (line, points) in line_points.iter().enumerate() {
        for pair in points.windows(2) {
            let a = pair[0];
            let b = pair[1];
            let Some(ai) = graph_vertex_index(&vertices, line, a) else {
                continue;
            };
            let Some(bi) = graph_vertex_index(&vertices, line, b) else {
                continue;
            };
            let distance = (b - a).abs();
            arcs[ai].push(GraphArc {
                to: bi,
                weight: distance,
                segment: Some([a, b]),
            });
            arcs[bi].push(GraphArc {
                to: ai,
                weight: distance,
                segment: Some([b, a]),
            });
        }
    }

    // EPGraph::Cross is the parsed `node` list.  Moving among values in one
    // cross changes line identity but adds no route length.
    for node in nodes {
        let mut junction_vertices = Vec::<usize>::new();
        for &point in node {
            let Some(line) = graph_find_line(edges, point) else {
                continue;
            };
            let Some(index) = graph_vertex_index(&vertices, line, point) else {
                continue;
            };
            if !junction_vertices.contains(&index) {
                junction_vertices.push(index);
            }
        }
        for &from in &junction_vertices {
            for &to in &junction_vertices {
                if from != to {
                    arcs[from].push(GraphArc {
                        to,
                        weight: 0.0,
                        segment: None,
                    });
                }
            }
        }
    }

    // Native enumerates candidate routes and selects the first strictly
    // shortest total (sub_101FBF00).  A stable O(V^2) Dijkstra has the same
    // metric while preserving insertion order on equal distances.
    let mut distance = vec![f32::INFINITY; vertices.len()];
    let mut visited = vec![false; vertices.len()];
    let mut previous = vec![None::<(usize, Option<[f32; 2]>)>; vertices.len()];
    distance[start_vertex] = 0.0;
    for _ in 0..vertices.len() {
        let mut best = None::<usize>;
        let mut best_distance = f32::INFINITY;
        for (index, &candidate) in distance.iter().enumerate() {
            if !visited[index] && candidate < best_distance {
                best = Some(index);
                best_distance = candidate;
            }
        }
        let Some(from) = best else {
            break;
        };
        if from == goal_vertex {
            break;
        }
        visited[from] = true;
        for arc in &arcs[from] {
            let next_distance = distance[from] + arc.weight;
            if next_distance < distance[arc.to] {
                distance[arc.to] = next_distance;
                previous[arc.to] = Some((from, arc.segment));
            }
        }
    }

    if !distance[goal_vertex].is_finite() {
        // sub_101FBF00 fallback: push [goal,goal] and set total route length 0.
        return GraphRoute {
            segments: vec![[goal, goal]],
            total_distance: 0.0,
        };
    }

    let mut reverse_segments = Vec::<[f32; 2]>::new();
    let mut cursor = goal_vertex;
    while cursor != start_vertex {
        let Some((from, segment)) = previous[cursor] else {
            return GraphRoute {
                segments: vec![[goal, goal]],
                total_distance: 0.0,
            };
        };
        if let Some(segment) = segment {
            reverse_segments.push(segment);
        }
        cursor = from;
    }
    reverse_segments.reverse();
    GraphRoute {
        segments: reverse_segments,
        total_distance: distance[goal_vertex],
    }
}

fn graph_active_destination(state: &GraphControlState) -> f32 {
    state
        .route
        .back()
        .map(|segment| segment[1])
        .unwrap_or(state.segment_target)
}

fn set_graph_control_target(
    state: &mut GraphControlState,
    target: f32,
    duration_ticks: f32,
    easing: f32,
    queuing: bool,
) {
    // sub_101E7040/sub_101EA310 receive the already converted power exponent
    // from the public variable setter. Store that native payload, rather than
    // applying the public easing conversion repeatedly while stepping.
    if duration_ticks <= 0.0 || !duration_ticks.is_finite() {
        state.queue.clear();
        state.route.clear();
        state.stage = 0;
        state.current = target;
        state.segment_target = target;
        state.total_distance = 0.0;
        state.traversed_distance = 0.0;
        return;
    }

    let command = GraphControlCommand {
        target,
        duration_ticks,
        easing_exponent: native_easing_exponent(easing),
    };
    if queuing {
        state.queue.push_back(command);
        return;
    }

    let should_replace = if state.stage == 0 {
        true
    } else {
        let active_target = graph_active_destination(state);
        if active_target != target {
            true
        } else {
            // sub_101E7040 computes the current time fraction by applying the
            // inverse power to traversed/total, then compares the requested
            // duration against the remaining native route time.
            let remaining = if state.total_distance > 0.0
                && state.easing_exponent > 0.0
                && state.inv_duration > 0.0
            {
                let distance_fraction = state.traversed_distance / state.total_distance;
                let time_fraction = distance_fraction.powf(1.0 / state.easing_exponent);
                (1.0 - time_fraction) / state.inv_duration
            } else {
                0.0
            };
            duration_ticks <= remaining
        }
    };

    if should_replace {
        state.queue.clear();
        state.route.clear();
        state.stage = 0;
        state.queue.push_back(command);
    }
}

fn set_eye_graph_target(
    state: &mut EyeControlState,
    target: f32,
    duration_ticks: f32,
    easing: f32,
    queuing: bool,
) {
    set_graph_control_target(&mut state.graph, target, duration_ticks, easing, queuing);
}

fn set_eyebrow_graph_target(
    state: &mut EyebrowControlState,
    target: f32,
    duration_ticks: f32,
    easing: f32,
    queuing: bool,
) {
    set_graph_control_target(&mut state.graph, target, duration_ticks, easing, queuing);
}

fn native_graph_direction(delta: f32) -> f32 {
    // sub_101E6980: negative -> -1, zero/positive -> +1.
    if delta < 0.0 { -1.0 } else { 1.0 }
}

fn advance_graph_control(
    edges: &[[f32; 2]],
    nodes: &[Vec<f32>],
    state: &mut GraphControlState,
    delta_ticks: f32,
) {
    // Direct transcription of the stage 0/1/2 loop shared by
    // EPEyeControl::Step (sub_101E2970) and EPEyebrowControl::Step
    // (sub_101E9B00). In particular, crossing a route segment returns to
    // stage 1 and may consume another segment in the *same* Step call using
    // the same delta. Do not collapse this into a whole-route interpolation.
    let guard = state
        .queue
        .len()
        .saturating_add(state.route.len())
        .saturating_mul(3)
        .saturating_add(16);
    for _ in 0..guard {
        match state.stage {
            0 => {
                let Some(command) = state.queue.pop_front() else {
                    break;
                };
                let route = build_ep_graph_route(edges, nodes, state.current, command.target);
                state.route = route.segments.into_iter().collect();
                state.total_distance = route.total_distance;
                state.traversed_distance = 0.0;
                state.inv_duration = 1.0 / command.duration_ticks;
                state.easing_exponent = command.easing_exponent;
                state.stage = 1;
            }
            1 => {
                let Some(segment) = state.route.pop_front() else {
                    state.stage = 0;
                    continue;
                };
                let start = segment[0];
                let end = segment[1];
                if start == end {
                    state.current = end;
                    continue;
                }
                state.current = start;
                state.segment_target = end;
                state.direction = native_graph_direction(end - start);
                state.stage = 2;
            }
            2 => {
                // Native stores cumulative route distance rather than elapsed
                // time. Recover the current time fraction through the inverse
                // power, advance by delta/duration, then re-apply the power.
                let old_time_fraction = (state.traversed_distance / state.total_distance)
                    .powf(1.0 / state.easing_exponent);
                let eased_distance_fraction =
                    (delta_ticks * state.inv_duration + old_time_fraction)
                        .powf(state.easing_exponent);
                let advance_distance =
                    eased_distance_fraction * state.total_distance - state.traversed_distance;
                state.current += state.direction * advance_distance;

                let reached = if state.direction <= 0.0 {
                    state.current - state.segment_target <= 1.0e-4
                } else {
                    state.current - state.segment_target >= -1.0e-4
                };
                if reached {
                    let correction =
                        (state.segment_target - state.current) * state.direction;
                    state.current = state.segment_target;
                    state.stage = 1;
                    state.traversed_distance += correction;
                    continue;
                }

                state.traversed_distance += advance_distance;
                break;
            }
            _ => {
                state.stage = 0;
            }
        }
    }
}

fn evaluate_eye_control(
    control: &EyeControl,
    state: &mut EyeControlState,
    delta_ticks: f32,
    variables: &mut BTreeMap<String, EmoteVariableState>,
) {
    if !control.enabled {
        return;
    }

    advance_graph_control(
        &control.edge,
        &control.node,
        &mut state.graph,
        delta_ticks,
    );

    let begin = control.begin_frame as f32;
    let end = control.end_frame as f32;
    // sub_101E2970 deliberately chains state transitions in one call via
    // `continue`, so a long tick can enter closing/hold/opening immediately.
    for _ in 0..8 {
        match state.blink_state {
            EyeBlinkState::Idle => {
                if control.blink_enabled && state.blink_frame as i32 == control.begin_frame {
                    state.blink_timer -= delta_ticks;
                    if state.blink_timer <= 0.0 {
                        state.blink_state = EyeBlinkState::Closing;
                        continue;
                    }
                }
            }
            EyeBlinkState::Closing => {
                if control.blink_frame_count.abs() > f32::EPSILON {
                    state.blink_frame +=
                        (end - begin) * (2.5 * delta_ticks / control.blink_frame_count);
                } else {
                    state.blink_frame = end;
                }
                if state.blink_frame >= end {
                    state.blink_frame = end;
                    state.blink_timer = control.blink_frame_count / 5.0;
                    state.blink_state = EyeBlinkState::ClosedHold;
                    continue;
                }
            }
            EyeBlinkState::ClosedHold => {
                state.blink_timer -= delta_ticks;
                if state.blink_timer <= 0.0 {
                    state.blink_state = EyeBlinkState::Opening;
                    state.blink_timer = native_random_range(
                        control.blink_interval_min,
                        control.blink_interval_max,
                    );
                    continue;
                }
            }
            EyeBlinkState::Opening => {
                if control.blink_frame_count.abs() > f32::EPSILON {
                    state.blink_frame -=
                        (end - begin) * (2.5 * delta_ticks / control.blink_frame_count);
                } else {
                    state.blink_frame = begin;
                }
                if state.blink_frame <= begin {
                    state.blink_frame = begin;
                    state.blink_state = EyeBlinkState::Idle;
                    continue;
                }
            }
        }
        break;
    }

    let graph_value = state.graph.current;
    let output = if graph_value < begin || end < graph_value || (end - begin).abs() <= f32::EPSILON {
        graph_value
    } else {
        graph_value + (state.blink_frame - begin) * (end - graph_value) / (end - begin)
    };
    set_evaluated_variable(variables, &control.label, output);
}

fn evaluate_eyebrow_control(
    control: &EyebrowControl,
    state: &mut EyebrowControlState,
    delta_ticks: f32,
    variables: &mut BTreeMap<String, EmoteVariableState>,
) {
    if !control.enabled {
        return;
    }
    advance_graph_control(&control.edge, &control.node, &mut state.graph, delta_ticks);
    set_evaluated_variable(variables, &control.label, state.graph.current);
}

fn evaluate_mouth_control(
    control: &MouthControl,
    state: &mut MouthControlState,
    delta_ticks: f32,
    variables: &mut BTreeMap<String, EmoteVariableState>,
) {
    if !control.enabled {
        return;
    }

    // sub_10200560: when a command completes, the state machine loops and may
    // start the next queued command in the same Step call.  It uses the same
    // tick delta for that next command (there is no residual-time calculation).
    let guard = state.queue.len().saturating_add(2);
    for _ in 0..guard {
        if state.active.is_none() {
            let Some(command) = state.queue.pop_front() else {
                break;
            };
            state.active = Some(ActiveScalarControl {
                start: state.current,
                target: command.target,
                inv_duration: 1.0 / command.duration_ticks,
                easing: command.easing,
                progress: 0.0,
            });
        }

        let Some(active) = state.active.as_mut() else {
            break;
        };
        active.progress += delta_ticks * active.inv_duration;
        if active.progress - 1.0 >= -0.000099999997 {
            active.progress = 1.0;
            state.current = active.target;
            state.active = None;
            continue;
        }
        let eased = native_control_easing(active.progress, active.easing);
        state.current = active.start + (active.target - active.start) * eased;
        break;
    }

    set_evaluated_variable(variables, &control.label, state.begin_frame as f32);
    set_evaluated_variable(variables, &control.talk_label, state.current);
}

fn set_mouth_talk_target(
    state: &mut MouthControlState,
    target: f32,
    duration_ticks: f32,
    easing: f32,
    queuing: bool,
) {
    // sub_10200C30.  The command payload is {target, duration, easing}.
    if duration_ticks > 0.0 {
        let command = TimedControlCommand {
            target,
            duration_ticks,
            easing,
        };
        if queuing {
            state.queue.push_back(command);
            return;
        }

        let should_replace = match state.active.as_ref() {
            None => true,
            Some(active) => {
                let remaining = if active.inv_duration.abs() <= f32::EPSILON {
                    f32::INFINITY
                } else {
                    (1.0 - active.progress) / active.inv_duration
                };
                active.target != target || duration_ticks <= remaining
            }
        };
        if should_replace {
            state.queue.clear();
            state.active = None;
            state.queue.push_back(command);
        }
    } else {
        state.queue.clear();
        state.active = None;
        state.current = target;
    }
}

fn set_selector_target(
    control: &SelectorControl,
    state: &mut SelectorControlState,
    target: f32,
    duration_ticks: f32,
    easing: f32,
    queuing: bool,
    pipeline: &EmoteRuntimePipeline,
    transition_states: &mut [ScalarTransitionState],
) {
    // EPSelectorControl::Set (sub_1020EE10): unlike Mouth/Transition, a
    // non-queuing timed selector command unconditionally cancels the current
    // selector timer and pending queue before appending the new command.
    if duration_ticks > 0.0 {
        if !queuing {
            state.queue.clear();
            state.active = None;
        }
        state.queue.push_back(TimedControlCommand {
            target,
            duration_ticks,
            easing,
        });
        return;
    }

    state.queue.clear();
    state.active = None;
    apply_selector_target(
        control,
        state,
        target,
        0.0,
        easing,
        transition_states,
        pipeline,
    );
}

fn set_scalar_transition_target(
    state: &mut ScalarTransitionState,
    target: f32,
    duration_ticks: f32,
    easing: f32,
    queuing: bool,
) {
    // EPTransitionControl::Set (sub_10218860), specialized to the one-float
    // instance created by the metadata parser (sub_102705B0 passes count=1).
    if duration_ticks > 0.0 {
        let command = TimedControlCommand {
            target,
            duration_ticks,
            easing,
        };
        if queuing {
            state.queue.push_back(command);
            return;
        }
        let should_replace = match state.active.as_ref() {
            None => true,
            Some(active) => {
                let remaining = if active.inv_duration.abs() <= f32::EPSILON {
                    f32::INFINITY
                } else {
                    (1.0 - active.progress) / active.inv_duration
                };
                active.target != target || duration_ticks <= remaining
            }
        };
        if should_replace {
            state.queue.clear();
            state.active = None;
            state.queue.push_back(command);
        }
    } else {
        state.queue.clear();
        state.active = None;
        state.current = target;
    }
}

fn loop_control_overwrites_variable(pipeline: &EmoteRuntimePipeline, name: &str) -> bool {
    pipeline.loop_controls.iter().any(|control| {
        control.enabled
            && control
                .var_loop
                .as_deref()
                .is_some_and(|var_loop| !var_loop.is_empty() && var_loop == name)
    })
}

fn evaluate_loop_control(
    control: &LoopControl,
    state: &mut LoopControlState,
    delta_ticks: f32,
    variables: &mut BTreeMap<String, EmoteVariableState>,
) {
    if !control.enabled {
        return;
    }
    let Some(var_loop) = control.var_loop.as_deref() else {
        return;
    };
    if control.transition_list.is_empty() {
        return;
    }

    state.index %= control.transition_list.len();
    state.elapsed_ticks += delta_ticks;
    // sub_101FF170 advances one transition at a time and preserves the
    // residual elapsed value; it is not derived from global player time.
    for _ in 0..control.transition_list.len().saturating_add(1) {
        let duration = control.transition_list[state.index].duration_ticks;
        if duration > f32::EPSILON && state.elapsed_ticks < duration {
            break;
        }
        if duration > f32::EPSILON {
            state.elapsed_ticks -= duration;
        } else {
            state.elapsed_ticks = 0.0;
        }
        state.index = (state.index + 1) % control.transition_list.len();
    }

    let item = &control.transition_list[state.index];
    let ratio = if item.duration_ticks.abs() <= f32::EPSILON {
        1.0
    } else {
        state.elapsed_ticks / item.duration_ticks
    };
    set_evaluated_variable(
        variables,
        var_loop,
        (1.0 - ratio) * item.start + item.end * ratio,
    );
}

fn variable_is_mirrored(pipeline: &EmoteRuntimePipeline, variable_name: &str) -> bool {
    if !pipeline.mirror_enabled {
        return false;
    }
    let Some(control) = pipeline.mirror_control.as_ref() else {
        return false;
    };
    // sub_10271C20 calls std::string::find(pattern, 0) and caches positive /
    // negative references.  Caching is not observable, so substring matching
    // is the exact semantic requirement.
    control
        .variable_match_list
        .iter()
        .any(|pattern| variable_name.contains(pattern))
}

/// Native sub_1026E290: active mirror (+388) is the XOR of the
/// runtime/user byte (+389) and metadata mirror byte (+390).
fn active_mirror_state(runtime_mirror: bool, metadata_mirror: bool) -> bool {
    runtime_mirror ^ metadata_mirror
}

fn stereovision_variable_is_targeted(
    pipeline: &EmoteRuntimePipeline,
    variable_name: &str,
) -> bool {
    let Some(control) = pipeline.stereovision_control.as_ref() else {
        return false;
    };
    // sub_1026F6E0 resolves each authored variableMatchList entry to an
    // MMotion variable reference during Init.  Unlike MirrorControl it does
    // not run std::string::find against every variable at write time, so the
    // portable representation intentionally uses exact authored names here.
    control
        .variable_match_list
        .iter()
        .any(|entry| entry == variable_name)
}

fn stereovision_vector_index(
    physical_index: usize,
    screen_count: usize,
    reverse_screens: bool,
) -> Option<usize> {
    if screen_count < 2 || physical_index >= screen_count {
        return None;
    }
    Some(if reverse_screens {
        screen_count - physical_index - 1
    } else {
        physical_index
    })
}

fn stereovision_screen_for_range(
    min: f32,
    max: f32,
    vector_index: usize,
    screen_count: usize,
    level: f32,
    fov: f32,
) -> Option<EmoteStereovisionScreen> {
    if screen_count < 2
        || vector_index >= screen_count
        || !min.is_finite()
        || !max.is_finite()
        || !level.is_finite()
        || !fov.is_finite()
    {
        return None;
    }
    let span = max - min;
    if !span.is_finite() || span.abs() <= f32::EPSILON {
        return None;
    }

    // Exact simplification of sub_1027A4D0.  Native code computes
    //   p = player[125] * player[124]          (+500 * +496)
    //   anchor = min + (N-i-1)/(N-1) * span
    // then derives a linear pair whose algebra reduces to:
    //   slope = 1-p; intercept = p*anchor.
    // `p` is deliberately not clamped: the DLL does not clamp it here.
    let p = fov * level;
    let anchor_ratio = (screen_count - vector_index - 1) as f32
        / (screen_count - 1) as f32;
    let anchor = min + span * anchor_ratio;
    Some(EmoteStereovisionScreen {
        slope: 1.0 - p,
        intercept: p * anchor,
    })
}

fn apply_stereovision_screen_projection(
    pipeline: &EmoteRuntimePipeline,
    variables: &mut BTreeMap<String, EmoteVariableState>,
    physical_screen_index: usize,
    screen_count: usize,
    level: f32,
    fov: f32,
    reverse_screens: bool,
) {
    let Some(vector_index) = stereovision_vector_index(
        physical_screen_index,
        screen_count,
        reverse_screens,
    ) else {
        return;
    };

    for (name, state) in variables.iter_mut() {
        if !stereovision_variable_is_targeted(pipeline, name) {
            // sub_10277100 writes the unmodified value to every physical
            // screen when the variable has no Stereovision map entry.
            continue;
        }
        let (Some(min), Some(max)) = (state.info.min_value, state.info.max_value) else {
            continue;
        };
        let Some(screen) = stereovision_screen_for_range(
            min,
            max,
            vector_index,
            screen_count,
            level,
            fov,
        ) else {
            continue;
        };
        state.value = screen.slope * state.value + screen.intercept;
    }
}

fn evaluate_mirror_control(
    pipeline: &EmoteRuntimePipeline,
    variables: &mut BTreeMap<String, EmoteVariableState>,
) {
    if !pipeline.mirror_enabled {
        return;
    }
    // sub_10276500 iterates the player's +0x60 map<string,float>, resolves each
    // variable reference, and negates only the value written to that resolved
    // reference.  This function therefore operates on a cloned/evaluated map,
    // never on ElunaPlayer::variables itself.
    for (name, state) in variables.iter_mut() {
        if variable_is_mirrored(pipeline, name) {
            state.value = -state.value;
        }
    }
}

fn apply_post_control_variable_passes(
    pipeline: &EmoteRuntimePipeline,
    variables: &mut BTreeMap<String, EmoteVariableState>,
) {
    // Exact native order: sub_10268A30 exits its fixed controller loop and
    // calls sub_10276500.  sub_10276500 performs Mirror and its tail-call is
    // sub_10275CC0 (ClampControl).
    evaluate_mirror_control(pipeline, variables);
    for control in &pipeline.clamp_controls {
        evaluate_clamp_control(control, pipeline, variables);
    }
}

fn evaluate_transition_control(
    control: &TransitionControl,
    state: &mut ScalarTransitionState,
    delta_ticks: f32,
    variables: &mut BTreeMap<String, EmoteVariableState>,
) {
    if !control.enabled {
        return;
    }
    advance_scalar_transition_state(state, delta_ticks);
    set_evaluated_variable(variables, &control.label, state.current);
}

fn advance_scalar_transition_state(state: &mut ScalarTransitionState, delta_ticks: f32) {
    let guard = state.queue.len().saturating_add(2);
    for _ in 0..guard {
        if state.active.is_none() {
            let Some(command) = state.queue.pop_front() else {
                break;
            };
            state.active = Some(ActiveScalarControl {
                start: state.current,
                target: command.target,
                inv_duration: 1.0 / command.duration_ticks,
                easing: command.easing,
                progress: 0.0,
            });
        }
        let Some(active) = state.active.as_mut() else {
            break;
        };
        active.progress += delta_ticks * active.inv_duration;
        if active.progress - 1.0 >= -0.000099999997 {
            active.progress = 1.0;
            state.current = active.target;
            state.active = None;
            continue;
        }
        let eased = native_control_easing(active.progress, active.easing);
        state.current = active.start + (active.target - active.start) * eased;
        break;
    }
}

/// EPBustControl group update.
///
/// This mirrors MEmotePlayer::Bust::step (sub_10273DA0) and the nested
/// EPBustControl::step (sub_101D4300). There are deliberately two independent
/// first-step flags: the group invokes the controller with dt=0 on its first
/// frame, while the controller itself captures root-anchor into +40/+44.
fn step_bust_physics(
    state: &mut BustPhysicsState,
    def: &PhysicsControlDefinition,
    delta_ticks: f32,
    target_anchor: [f32; 3],
    angle_radians: f32,
    outer_force: [f32; 2],
    output_scale: f32,
    variables: &mut BTreeMap<String, EmoteVariableState>,
) {
    if !def.enabled {
        return;
    }

    // sub_10273DA0 resolves baseLayer first, then adds the bust OuterForce to
    // the *current* endpoint. At the end of every group update it subtracts
    // that force again before saving entry+132/+136. Consequently the next
    // frame interpolates from previous_base (no force) to
    // current_base + current_force. Do not add a full force after lerp.
    let forced_target = [
        target_anchor[0] + outer_force[0],
        target_anchor[1] + outer_force[1],
        target_anchor[2],
    ];

    if state.group_first_tick {
        state.group_first_tick = false;
        step_bust_physics_once(
            state,
            def,
            0.0,
            forced_target,
            angle_radians,
            output_scale,
            variables,
        );
        // Native entry+132/+136 stores the baseLayer point after subtracting
        // OuterForce, even on the first group update.
        state.last_anchor = Some(target_anchor);
        return;
    }

    let previous_anchor = state.last_anchor.unwrap_or(target_anchor);
    if delta_ticks > PHYSICS_EPSILON_TICKS {
        let mut elapsed = 0.0;
        while (delta_ticks - PHYSICS_EPSILON_TICKS) > elapsed {
            let step = (delta_ticks - elapsed).min(PHYSICS_MAX_SUBSTEP_TICKS);
            elapsed += step;
            let ratio = (elapsed / delta_ticks).clamp(0.0, 1.0);
            let anchor = lerp_vec3(previous_anchor, forced_target, ratio);
            step_bust_physics_once(
                state,
                def,
                step,
                anchor,
                angle_radians,
                output_scale,
                variables,
            );
        }
    }

    // Same bookkeeping as the native post-loop force subtraction.
    state.last_anchor = Some(target_anchor);
}

fn step_bust_physics_once(
    state: &mut BustPhysicsState,
    def: &PhysicsControlDefinition,
    dt: f32,
    input_anchor: [f32; 3],
    angle_radians: f32,
    output_scale: f32,
    variables: &mut BTreeMap<String, EmoteVariableState>,
) {
    let gravity = physics_field_f32(def, "gravity", 0.0);
    let spring = physics_field_f32(def, "spring", 0.0);
    let friction = physics_field_f32(def, "friction", 0.0);
    let scale_x = physics_field_f32(def, "scale_x", 1.0);
    let scale_y = physics_field_f32(def, "scale_y", 1.0);

    // EPBustControl::step sub_101D4300 does not interpret param.op as an
    // ordinary local offset. On the first call it leaves root untouched and
    // captures root.xy - input.xy into +40/+44. Later calls rebuild root.xy
    // from the moving input anchor plus that saved delta. root.z is never
    // replaced by the baseLayer Z here.
    if state.controller_first_tick {
        state.root_delta = [
            state.root[0] - input_anchor[0],
            state.root[1] - input_anchor[1],
        ];
        state.controller_first_tick = false;
    } else {
        state.root[0] = input_anchor[0] + state.root_delta[0];
        state.root[1] = input_anchor[1] + state.root_delta[1];
    }

    // DLL global force basis at 0x10582920 is (0, 1, 0); sub_101D4300 rotates
    // it by -angle before applying gravity.
    let down = rotated_down(angle_radians);
    let disp = sub_vec3(state.root, state.bob);

    // Exact native integration order:
    //   v += spring * dt * (root-bob)
    //   v += gravity * dt * rotated_down
    //   v -= friction * dt * v
    //   bob += dt * v
    state.vel[0] += spring * dt * disp[0];
    state.vel[1] += spring * dt * disp[1];
    state.vel[2] += spring * dt * disp[2];
    state.vel[0] += gravity * dt * down[0];
    state.vel[1] += gravity * dt * down[1];

    let damp = 1.0 - friction * dt;
    state.vel[0] *= damp;
    state.vel[1] *= damp;
    state.vel[2] *= damp;

    state.bob[0] += state.vel[0] * dt;
    state.bob[1] += state.vel[1] * dt;
    state.bob[2] += state.vel[2] * dt;

    // sub_101D4300 scales the displacement vector by the player bust-scale
    // first, then applies authored X/Y output scales and the atan response.
    let mut out_disp = sub_vec3(state.root, state.bob);
    out_disp[0] *= output_scale;
    out_disp[1] *= output_scale;
    out_disp[2] *= output_scale;
    let var_lr = physics_output_response(-out_disp[0] * scale_x);
    let var_ud = physics_output_response((-out_disp[1] - state.ofs) * scale_y);

    if let Some(name) = &def.var_lr {
        set_evaluated_variable(variables, name, var_lr);
    }
    if let Some(name) = &def.var_ud {
        set_evaluated_variable(variables, name, var_ud);
    }
}

fn step_hair_physics(
    state: &mut HairPhysicsState,
    def: &PhysicsControlDefinition,
    delta_ticks: f32,
    target_anchor: [f32; 3],
    angle_radians: f32,
    outer_force: [f32; 2],
    wind: Option<&WindState>,
    output_scale: f32,
    variables: &mut BTreeMap<String, EmoteVariableState>,
) {
    if !def.enabled {
        return;
    }
    if state.first_tick {
        // Attach the restored simulation to this model's anchor without
        // discarding its equilibrium displacement or velocity.
        for bob in &mut state.bob {
            *bob = add_vec3(*bob, target_anchor);
        }
        state.last_anchor = Some(target_anchor);
        state.first_tick = false;
        step_hair_physics_once(
            state,
            def,
            0.0,
            target_anchor,
            angle_radians,
            outer_force,
            wind,
            output_scale,
            variables,
        );
        return;
    }
    if delta_ticks <= PHYSICS_EPSILON_TICKS {
        return;
    }
    let previous_anchor = state.last_anchor.unwrap_or(target_anchor);
    let mut elapsed = 0.0;
    while (delta_ticks - PHYSICS_EPSILON_TICKS) > elapsed {
        let step = (delta_ticks - elapsed).min(PHYSICS_MAX_SUBSTEP_TICKS);
        elapsed += step;
        let ratio = (elapsed / delta_ticks).clamp(0.0, 1.0);
        let anchor = lerp_vec3(previous_anchor, target_anchor, ratio);
        step_hair_physics_once(
            state,
            def,
            step,
            anchor,
            angle_radians,
            outer_force,
            wind,
            output_scale,
            variables,
        );
    }
    state.last_anchor = Some(target_anchor);
}

/// Sample the native EPWindControl field at a pendulum X coordinate.
///
/// Recovered from sub_1021B1B0: pulses are checked in storage order, each
/// pulse has a strict support interval `(position - 2*power, position +
/// 2*power)`, and the first matching pulse contributes `sign(speed)*power`.
fn sample_wind(wind: &WindState, x: f32) -> f32 {
    for pulse in &wind.pulses {
        if !pulse.active {
            continue;
        }
        let radius = pulse.power * 2.0;
        if x > pulse.position - radius && pulse.position + radius > x {
            let direction = if wind.signed_speed < 0.0 { -1.0 } else { 1.0 };
            return direction * pulse.power;
        }
    }
    0.0
}

fn step_hair_physics_once(
    state: &mut HairPhysicsState,
    def: &PhysicsControlDefinition,
    dt: f32,
    target_anchor: [f32; 3],
    angle_radians: f32,
    outer_force: [f32; 2],
    wind: Option<&WindState>,
    output_scale: f32,
    variables: &mut BTreeMap<String, EmoteVariableState>,
) {
    let gravity = physics_field_f32(def, "gravity", 0.0);
    let friction_x = physics_field_f32(def, "friction_x", 0.0);
    let friction_y = physics_field_f32(def, "friction_y", 0.0);
    let b_rate = physics_field_f32(def, "b_rate", 0.0);
    let v_bound = physics_field_f32(def, "v_bound", 0.0);
    let bend_spd = physics_field_f32(def, "bend_spd", 0.0);
    let bend_vol = physics_field_f32(def, "bend_vol", 0.0);
    let lengths = physics_field_f32_list_2(def, "length");
    let scale_x = physics_field_f32_list_2(def, "scale_x");
    let scale_y = physics_field_f32_list_2(def, "scale_y");
    let ud_eft = physics_field_i64(def, "ud_eft", 0).clamp(0, 1) as usize;

    // Hair/parts group wrappers sub_10274480/sub_102749E0 forward their
    // independent OuterForce X/Y fields to sub_10274A80, which adds them to
    // the baseLayer root before EPPendControl::step.  Preserve that exact
    // moving-anchor semantics instead of treating the values as acceleration.
    let root = [
        target_anchor[0] + outer_force[0] + state.root_offset[0],
        target_anchor[1] + outer_force[1] + state.root_offset[1],
        target_anchor[2] + state.root_offset[2],
    ];
    let rest0 = [root[0], root[1] + lengths[0], root[2]];
    let rest1 = [rest0[0], rest0[1] + lengths[1], rest0[2]];
    let rest = [rest0, rest1];
    let down = rotated_down(angle_radians);
    let gravity_vec = [down[0] * gravity, down[1] * gravity, 0.0];

    for j in 0..2 {
        let hinge = if j == 0 { root } else { state.bob[0] };
        let hinge_to_bob = sub_vec3(state.bob[j], hinge);
        let dist = vec3_len(hinge_to_bob);
        let seg_len = lengths[j].max(f32::EPSILON);
        if dist > seg_len && dist > f32::EPSILON {
            let outward = scale_vec3(hinge_to_bob, 1.0 / dist);
            let inward = scale_vec3(outward, -1.0);
            let excess = dist - seg_len;
            if excess > 0.015625 {
                if j == 1 {
                    state.bob[j] = add_vec3(state.bob[j], scale_vec3(inward, excess));
                    let radial_velocity = dot_vec3(state.vel[j], inward);
                    state.vel[j] = add_vec3(
                        state.vel[j],
                        scale_vec3(inward, -radial_velocity * v_bound * dt),
                    );
                } else {
                    state.vel[j] = add_vec3(state.vel[j], scale_vec3(inward, excess * b_rate * dt));
                }
            }
        }

        state.vel[j][0] += gravity_vec[0] * dt;
        state.vel[j][1] += gravity_vec[1] * dt;
        state.vel[j][2] += gravity_vec[2] * dt;

        // sub_10201AB0 samples EPWindControl at the current pendulum bob X
        // after gravity but before friction, and applies the result as an X
        // velocity impulse (not multiplied by dt).
        if let Some(wind) = wind {
            state.vel[j][0] += sample_wind(wind, state.bob[j][0]);
        }

        // sub_10201AB0 damps only the X/Y velocity components at +136/+140.
        // Z (+144) is intentionally left untouched here; reusing friction_y
        // for Z made out-of-plane constraint motion decay unlike the DLL.
        state.vel[j][0] -= state.vel[j][0] * friction_x * dt;
        state.vel[j][1] -= state.vel[j][1] * friction_y * dt;

        state.bob[j] = add_vec3(state.bob[j], scale_vec3(state.vel[j], dt));
    }

    let d0 = sub_vec3(rest[0], state.bob[0]);
    let d1 = sub_vec3(rest[1], state.bob[1]);
    // EPPendControl::step applies sub_1021A4F0 to each output first; the bend
    // post-process (sub_10202260) then offsets the already-shaped LR/LRM pair.
    let mut var_lr = physics_output_response(-d0[0] * scale_x[0] * output_scale);
    let mut var_lrm = physics_output_response(-d1[0] * scale_x[1] * output_scale);
    let ud_disp = if ud_eft == 0 { d0[1] } else { d1[1] };
    let var_ud = physics_output_response(
        (state.ofs - ud_disp) * scale_y[ud_eft] * output_scale,
    );

    apply_pend_bend(state, bend_spd, bend_vol, dt, &mut var_lr, &mut var_lrm);

    if let Some(name) = &def.var_lr {
        set_evaluated_variable(variables, name, var_lr);
    }
    if let Some(name) = &def.var_lrm {
        set_evaluated_variable(variables, name, var_lrm);
    }
    if let Some(name) = &def.var_ud {
        set_evaluated_variable(variables, name, var_ud);
    }
}

fn apply_pend_bend(
    state: &mut HairPhysicsState,
    bend_spd: f32,
    bend_vol: f32,
    dt: f32,
    var_lr: &mut f32,
    var_lrm: &mut f32,
) {
    if bend_spd == 0.0 || bend_vol == 0.0 {
        return;
    }
    let trigger = var_lr.abs();
    if trigger <= PEND_BEND_TRIGGER_VALUE {
        state.bend_power = (state.bend_power - PEND_BEND_POWER_STEP * dt).max(0.0);
    } else {
        state.bend_power = (state.bend_power + PEND_BEND_POWER_STEP * dt).min(1.0);
    }
    state.bend_phase = (state.bend_phase + bend_spd * state.bend_power * dt).rem_euclid(TAU);
    let bend = state.bend_phase.sin() * state.bend_power * bend_vol;
    *var_lrm += bend;
    *var_lr -= bend;
}

fn rotated_down(angle_radians: f32) -> [f32; 3] {
    let c = (-angle_radians).cos();
    let s = (-angle_radians).sin();
    [-s, c, 0.0]
}

fn lerp_vec3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn add_vec3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub_vec3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale_vec3(a: [f32; 3], s: f32) -> [f32; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot_vec3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn vec3_len(a: [f32; 3]) -> f32 {
    dot_vec3(a, a).sqrt()
}

fn physics_output_response(value: f32) -> f32 {
    // sub_1021A4F0:
    //   atan((1.15 * value) * (PI / 80)) / (PI / 80)
    // This is not a deadzone. It is a smooth saturating response shared by
    // EPBustControl and EPPendControl before their variables are written.
    const K: f32 = std::f32::consts::PI / 80.0;
    ((1.15 * value) * K).atan() / K
}

fn physics_field_f32(def: &PhysicsControlDefinition, key: &str, default: f32) -> f32 {
    def.fields
        .get("param")
        .and_then(|param| param.field_f32(key))
        .or_else(|| def.fields.get(key).and_then(PsbValue::as_f32))
        .unwrap_or(default)
}

fn physics_field_i64(def: &PhysicsControlDefinition, key: &str, default: i64) -> i64 {
    def.fields
        .get("param")
        .and_then(|param| param.field_i64(key))
        .or_else(|| def.fields.get(key).and_then(PsbValue::as_i64))
        .unwrap_or(default)
}

fn physics_field_f32_list_2(def: &PhysicsControlDefinition, key: &str) -> [f32; 2] {
    if let Some(param) = def.fields.get("param") {
        let parsed = parse_f32_value_list_2(param.field(key));
        if parsed != [0.0, 0.0] {
            return parsed;
        }
    }
    parse_f32_value_list_2(def.fields.get(key))
}

fn parse_f32_value_list_2(value: Option<&PsbValue>) -> [f32; 2] {
    let Some(value) = value else {
        return [0.0, 0.0];
    };
    match value {
        PsbValue::List(items) => {
            let a = items.first().and_then(PsbValue::as_f32).unwrap_or(0.0);
            let b = items.get(1).and_then(PsbValue::as_f32).unwrap_or(a);
            [a, b]
        }
        _ => {
            let v = value.as_f32().unwrap_or(0.0);
            [v, v]
        }
    }
}

fn set_evaluated_variable(
    variables: &mut BTreeMap<String, EmoteVariableState>,
    name: &str,
    value: f32,
) {
    if name.is_empty() || !value.is_finite() {
        return;
    }
    let state = variables
        .entry(name.to_owned())
        .or_insert_with(|| EmoteVariableState {
            info: EmoteVariableInfo {
                name: name.to_owned(),
                default_value: value,
                min_value: None,
                max_value: None,
                frames: Vec::new(),
            },
            value,
            target: None,
        });
    state.value = value;
    state.target = None;
}

pub fn collect_emote_timelines(psb: &PsbFile) -> Vec<EmoteTimeline> {
    let mut out = Vec::new();
    if let Some(timeline_root) = psb
        .root
        .field("metadata")
        .and_then(|metadata| metadata.field("timelineControl"))
    {
        // MEmotePlayer timelineControl parser (sub_10270070) iterates the
        // top-level array directly.  Each entry is keyed by its authored
        // `label`; array indices are never part of the public timeline name.
        collect_timeline_nodes(timeline_root, &mut out);
    }

    out.sort_by(|a, b| a.name.cmp(&b.name));
    out.dedup_by(|a, b| a.name.as_str() == b.name.as_str());
    out
}

fn collect_timeline_nodes(value: &PsbValue, out: &mut Vec<EmoteTimeline>) {
    match value {
        PsbValue::List(items) => {
            // sub_10270070 indexes timelineControl directly.  The array index
            // has no semantic meaning and is never concatenated into a label.
            for item in items {
                collect_timeline_nodes(item, out);
            }
        }
        PsbValue::Object(_) => {
            let Some(variable_list) = value.field("variableList").and_then(PsbValue::as_list) else {
                // Native timelineControl entries are Timeline objects.  Do not
                // recursively reinterpret arbitrary nested metadata as extra
                // timelines: that was the source of the old @control and
                // numeric-path pseudo timelines.
                return;
            };
            let label = value.field_str("label").unwrap_or("").to_owned();

            let mut variables = Vec::new();
            // Native Timeline parser sub_1026FA30 stores three independent
            // boundaries. loopBegin < 0 means one-shot; lastTime is the
            // terminal authored time (or the last parsed frame when absent).
            let loop_begin_ticks = value.field_f32("loopBegin").unwrap_or(-1.0);
            let loop_end_ticks = value.field_f32("loopEnd").unwrap_or(-1.0);
            let authored_last_time = value.field_f32("lastTime").unwrap_or(-1.0);
            let mut parsed_last_time = 0.0f32;

            for variable in variable_list {
                let Some(var_name) = variable.field_str("label").filter(|name| !name.is_empty()) else {
                    continue;
                };
                let Some(frame_list) = variable.field("frameList").and_then(PsbValue::as_list)
                else {
                    continue;
                };
                let mut frames = Vec::new();
                for frame in frame_list {
                    let time = frame.field_f32("time").unwrap_or(0.0);
                    parsed_last_time = time;
                    let hold = frame.field_i64("type").unwrap_or(0) == 0;
                    // TimelineVariableFrame::type == 0 has no content in the
                    // native parser. Preserve the cursor marker but do not
                    // manufacture a variable write for it.
                    let (frame_value, easing) = if hold {
                        (0.0, 0.0)
                    } else if let Some(content) = frame.field("content") {
                        (
                            content.field_f32("value").unwrap_or(0.0),
                            content.field_f32("easing").unwrap_or(0.0),
                        )
                    } else {
                        (0.0, 0.0)
                    };
                    if hold || frame_value.is_finite() {
                        frames.push(EmoteTimelineFrame {
                            time_ticks: time,
                            hold,
                            value: frame_value,
                            easing,
                        });
                    }
                }
                // sub_1026FA30 preserves frameList order.  Cursor walking and
                // the lastTime<0 fallback both use that authored order.
                variables.push(EmoteTimelineVariable {
                    name: var_name.to_owned(),
                    frames,
                });
            }

            let last_time_ticks = if authored_last_time >= 0.0 {
                // sub_1026FA30 only falls back to the last parsed frame when
                // metadata.lastTime itself is negative.
                authored_last_time
            } else {
                parsed_last_time
            };
            // sub_10270070 inserts every timelineControl entry into the map,
            // including a valid timeline that happens to contain zero tracks.
            out.push(EmoteTimeline {
                name: label,
                path: None,
                loop_begin_ticks,
                loop_end_ticks,
                last_time_ticks,
                duration_ticks: last_time_ticks,
                variables,
                is_difference: psb_field_bool_like(value, "diff").unwrap_or(false),
            });
        }
        _ => {}
    }
}

pub fn collect_emote_variables(psb: &PsbFile) -> Vec<EmoteVariableInfo> {
    let mut out = BTreeMap::<String, EmoteVariableInfo>::new();

    if let Some(metadata) = psb.root.field("metadata") {
        // sub_1026EC30 only constructs ordinary variable records from the
        // authored variableList.  Control objects are parsed separately and
        // their `label` fields are controller identities/references, not a
        // recursively discoverable variable tree.
        collect_variable_list(metadata.field("variableList"), &mut out);
    }

    collect_variable_list(psb.root.field("parameter"), &mut out);
    collect_variable_list(psb.root.field("parameters"), &mut out);
    collect_parameter_variables_recursive(&psb.root, &mut out);
    collect_mesh_combinator_variables(&psb.root, &mut out);
    for timeline in collect_emote_timelines(psb) {
        for variable in &timeline.variables {
            merge_timeline_variable_info(variable, &mut out);
        }
    }

    // Controller variable references are registered explicitly by their
    // native control type.  This avoids the previous recursive label scrape,
    // which exposed bust/hair/parts control names as bogus variables while
    // missing their actual var_lr/var_ud outputs.
    let pipeline = collect_emote_runtime_pipeline(psb);
    collect_runtime_control_variable_infos(&pipeline, &mut out);

    out.into_values().collect()
}

fn collect_parameter_variables_recursive(
    value: &PsbValue,
    out: &mut BTreeMap<String, EmoteVariableInfo>,
) {
    if let Some(parameter_list) = value.field("parameter") {
        collect_variable_list(Some(parameter_list), out);
    }
    match value {
        PsbValue::List(values) => {
            for child in values {
                collect_parameter_variables_recursive(child, out);
            }
        }
        PsbValue::Object(fields) => {
            for (key, child) in fields {
                if key == "parameter" {
                    continue;
                }
                collect_parameter_variables_recursive(child, out);
            }
        }
        _ => {}
    }
}

fn merge_runtime_variable_ref(
    out: &mut BTreeMap<String, EmoteVariableInfo>,
    name: &str,
    default_value: f32,
    min_value: Option<f32>,
    max_value: Option<f32>,
) {
    if name.is_empty() {
        return;
    }
    merge_variable_info(
        name.to_owned(),
        EmoteVariableInfo {
            name: name.to_owned(),
            default_value,
            min_value,
            max_value,
            frames: Vec::new(),
        },
        out,
    );
}

fn graph_control_range(
    begin_frame: f32,
    end_frame: Option<f32>,
    edges: &[[f32; 2]],
    nodes: &[Vec<f32>],
) -> (Option<f32>, Option<f32>) {
    let mut min_value = Some(begin_frame);
    let mut max_value = Some(begin_frame);
    if let Some(value) = end_frame {
        merge_range_value(&mut min_value, &mut max_value, value);
    }
    for edge in edges {
        for value in edge {
            merge_range_value(&mut min_value, &mut max_value, *value);
        }
    }
    for node in nodes {
        for value in node {
            merge_range_value(&mut min_value, &mut max_value, *value);
        }
    }
    (min_value, max_value)
}

fn collect_runtime_control_variable_infos(
    pipeline: &EmoteRuntimePipeline,
    out: &mut BTreeMap<String, EmoteVariableInfo>,
) {
    // sub_10270D80 builds timelinePlayingVariableLabelSet from the control
    // references.  Register exactly those references instead of recursively
    // interpreting arbitrary control-object labels as ordinary variables.
    for control in &pipeline.eye_controls {
        let (min, max) = graph_control_range(
            control.begin_frame as f32,
            Some(control.end_frame as f32),
            &control.edge,
            &control.node,
        );
        merge_runtime_variable_ref(
            out,
            &control.label,
            control.begin_frame as f32,
            min,
            max,
        );
    }
    for control in &pipeline.eyebrow_controls {
        let (min, max) = graph_control_range(
            control.begin_frame as f32,
            None,
            &control.edge,
            &control.node,
        );
        merge_runtime_variable_ref(
            out,
            &control.label,
            control.begin_frame as f32,
            min,
            max,
        );
    }
    for control in &pipeline.mouth_controls {
        merge_runtime_variable_ref(
            out,
            &control.label,
            control.begin_frame as f32,
            Some(control.begin_frame as f32),
            None,
        );
        // talkLabel is a timed scalar controller reference. Its authored range
        // normally comes from parameter/timeline/mesh metadata, so do not
        // invent +/-1 when no such range exists.
        merge_runtime_variable_ref(out, &control.talk_label, 0.0, None, None);
    }
    for control in &pipeline.transition_controls {
        if control.enabled {
            merge_runtime_variable_ref(out, &control.label, 0.0, None, None);
        }
    }
    for control in &pipeline.selector_controls {
        if !control.enabled {
            continue;
        }
        let max = control.option_list.len().saturating_sub(1) as f32;
        merge_runtime_variable_ref(out, &control.label, 0.0, Some(0.0), Some(max));
        for option in &control.option_list {
            merge_runtime_variable_ref(
                out,
                &option.label,
                option.off_value,
                Some(option.off_value.min(option.on_value)),
                Some(option.off_value.max(option.on_value)),
            );
        }
    }
    for control in &pipeline.loop_controls {
        if !control.enabled {
            continue;
        }
        let Some(name) = control.var_loop.as_deref().filter(|name| !name.is_empty()) else {
            continue;
        };
        let default = control
            .transition_list
            .first()
            .map(|item| item.start)
            .unwrap_or(0.0);
        let mut min = None;
        let mut max = None;
        for item in &control.transition_list {
            merge_range_value(&mut min, &mut max, item.start);
            merge_range_value(&mut min, &mut max, item.end);
        }
        merge_runtime_variable_ref(out, name, default, min, max);
    }
    for control in &pipeline.clamp_controls {
        if !control.enabled {
            continue;
        }
        for name in [&control.var_lr, &control.var_ud] {
            merge_runtime_variable_ref(
                out,
                name,
                0.0,
                Some(control.min),
                Some(control.max),
            );
        }
    }
    for control in &pipeline.physics_controls {
        let def = match control {
            PhysicsControl::Bust(def) | PhysicsControl::Hair(def) | PhysicsControl::Parts(def) => def,
        };
        if !def.enabled {
            continue;
        }
        for name in [def.var_lr.as_deref(), def.var_ud.as_deref(), def.var_lrm.as_deref()]
            .into_iter()
            .flatten()
        {
            // Solver outputs have no fixed authored range in bust/pend control
            // metadata. Mesh/parameter metadata, when present, is merged above.
            merge_runtime_variable_ref(out, name, 0.0, None, None);
        }
    }
}

fn collect_mesh_combinator_variables(
    value: &PsbValue,
    out: &mut BTreeMap<String, EmoteVariableInfo>,
) {
    if let Some(combinators) = value
        .field("meshCombinator")
        .and_then(|mesh_combinator| mesh_combinator.field("combinatorList"))
        .and_then(PsbValue::as_list)
    {
        for combinator in combinators {
            if let Some(variable) = combinator.field("variable") {
                collect_one_variable(variable, out);
            }
        }
    }

    match value {
        PsbValue::List(values) => {
            for child in values {
                collect_mesh_combinator_variables(child, out);
            }
        }
        PsbValue::Object(fields) => {
            for (_key, child) in fields {
                collect_mesh_combinator_variables(child, out);
            }
        }
        _ => {}
    }
}

fn collect_variable_list(value: Option<&PsbValue>, out: &mut BTreeMap<String, EmoteVariableInfo>) {
    let Some(value) = value else {
        return;
    };

    match value {
        PsbValue::List(values) => {
            for child in values {
                collect_one_variable(child, out);
            }
        }
        PsbValue::Object(fields) => {
            for (_key, child) in fields {
                collect_one_variable(child, out);
            }
        }
        _ => {}
    }
}

fn collect_one_variable(value: &PsbValue, out: &mut BTreeMap<String, EmoteVariableInfo>) {
    let Some(name) = variable_object_name(value) else {
        return;
    };

    let frames = variable_frame_infos(value);
    let frame_min = frames.iter().map(|frame| frame.value).reduce(f32::min);
    let frame_max = frames.iter().map(|frame| frame.value).reduce(f32::max);

    let next = EmoteVariableInfo {
        name: name.clone(),
        default_value: value
            .field_f32("default")
            .or_else(|| value.field_f32("defaultValue"))
            .or_else(|| value.field_f32("initial"))
            .or_else(|| value.field_f32("curValue"))
            .or_else(|| value.field_f32("value"))
            .or_else(|| frames.first().map(|frame| frame.value))
            .unwrap_or(0.0),
        min_value: value
            .field_f32("min")
            .or_else(|| value.field_f32("minValue"))
            .or_else(|| value.field_f32("rangeBegin"))
            .or(frame_min),
        max_value: value
            .field_f32("max")
            .or_else(|| value.field_f32("maxValue"))
            .or_else(|| value.field_f32("rangeEnd"))
            .or(frame_max),
        frames,
    };

    merge_variable_info(name, next, out);
}

fn merge_timeline_variable_info(
    variable: &EmoteTimelineVariable,
    out: &mut BTreeMap<String, EmoteVariableInfo>,
) {
    if variable.name.is_empty() {
        return;
    }
    // TimelineVariableFrame::type == 0 is a native no-write marker.  Its
    // synthetic Rust value field must never leak into authored UI ranges or
    // defaults.  More importantly, timeline keys do not replace an existing
    // variable's initial value in MEmotePlayer.
    let mut frames = Vec::new();
    for frame in &variable.frames {
        if frame.hold {
            continue;
        }
        frames.push(EmoteVariableFrameInfo {
            label: format!("{:.3}", frame.time_ticks),
            value: frame.value,
        });
    }
    let frame_min = frames.iter().map(|frame| frame.value).reduce(f32::min);
    let frame_max = frames.iter().map(|frame| frame.value).reduce(f32::max);
    // sub_1026FA30 only parses timeline commands. It does not use the first
    // command as the variable's initial value. A timeline-only synthetic
    // variable therefore starts at the scalar zero default.
    let timeline_default = 0.0;

    if let Some(entry) = out.get_mut(&variable.name) {
        if let Some(min) = frame_min {
            merge_range_value(&mut entry.min_value, &mut entry.max_value, min);
        }
        if let Some(max) = frame_max {
            merge_range_value(&mut entry.min_value, &mut entry.max_value, max);
        }
        for frame in frames {
            if !entry.frames.iter().any(|existing| {
                existing.label == frame.label
                    && (existing.value - frame.value).abs() <= f32::EPSILON
            }) {
                merge_range_value(&mut entry.min_value, &mut entry.max_value, frame.value);
                entry.frames.push(frame);
            }
        }
        return;
    }

    out.insert(
        variable.name.clone(),
        EmoteVariableInfo {
            name: variable.name.clone(),
            default_value: timeline_default,
            min_value: frame_min,
            max_value: frame_max,
            frames,
        },
    );
}

fn merge_variable_info(
    name: String,
    next: EmoteVariableInfo,
    out: &mut BTreeMap<String, EmoteVariableInfo>,
) {
    let entry = out.entry(name).or_insert_with(|| next.clone());
    if entry.frames.is_empty() && !next.frames.is_empty() {
        entry.default_value = next.default_value;
    }
    if let Some(min) = next.min_value {
        merge_range_value(&mut entry.min_value, &mut entry.max_value, min);
    }
    if let Some(max) = next.max_value {
        merge_range_value(&mut entry.min_value, &mut entry.max_value, max);
    }
    for frame in next.frames {
        if !entry.frames.iter().any(|existing| {
            existing.label == frame.label && (existing.value - frame.value).abs() <= f32::EPSILON
        }) {
            merge_range_value(&mut entry.min_value, &mut entry.max_value, frame.value);
            entry.frames.push(frame);
        }
    }
}

fn variable_frame_infos(value: &PsbValue) -> Vec<EmoteVariableFrameInfo> {
    let Some(frame_list) = value.field("frameList").and_then(PsbValue::as_list) else {
        return Vec::new();
    };
    let mut frames = Vec::new();
    for frame in frame_list {
        let label = frame
            .field_str("label")
            .or_else(|| {
                frame
                    .field("content")
                    .and_then(|content| content.field_str("label"))
            })
            .unwrap_or("");
        let value = frame
            .field_f32("frame")
            .or_else(|| {
                frame
                    .field("content")
                    .and_then(|content| content.field_f32("frame"))
            })
            .or_else(|| frame.field_f32("value"))
            .or_else(|| {
                frame
                    .field("content")
                    .and_then(|content| content.field_f32("value"))
            });
        if let Some(value) = value.filter(|v| v.is_finite()) {
            frames.push(EmoteVariableFrameInfo {
                label: label.to_owned(),
                value,
            });
        }
    }
    frames.sort_by(|a, b| {
        a.value
            .total_cmp(&b.value)
            .then_with(|| a.label.cmp(&b.label))
    });
    frames.dedup_by(|a, b| a.label == b.label && (a.value - b.value).abs() <= f32::EPSILON);
    frames
}

fn psb_field_bool_like(value: &PsbValue, name: &str) -> Option<bool> {
    match value.field(name)? {
        PsbValue::Bool(v) => Some(*v),
        PsbValue::Int(v) => Some(*v != 0),
        PsbValue::Float(v) => Some(*v != 0.0),
        PsbValue::Double(v) => Some(*v != 0.0),
        PsbValue::String(v) => match v.as_str() {
            "true" | "TRUE" | "True" | "1" => Some(true),
            "false" | "FALSE" | "False" | "0" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn parse_api_log_entry(line: &str) -> Option<EmoteApiLogEntry> {
    let mut parts = line.split('\t');
    let command = parts.next()?.trim();
    if command.is_empty() {
        return None;
    }
    Some(EmoteApiLogEntry::new(
        command.to_owned(),
        parts.map(str::to_owned).collect(),
    ))
}

fn variable_object_name(obj: &PsbValue) -> Option<String> {
    for key in ["id", "key", "name", "label"] {
        if let Some(value) = obj.field_str(key).filter(|s| !s.is_empty()) {
            return Some(value.to_owned());
        }
    }
    None
}


#[cfg(test)]
mod reverse_parity_tests {
    use super::*;

    #[test]
    fn outer_rotation_uses_shortest_branch() {
        let target = shortest_angle_target(350.0, 10.0);
        assert!((target - 370.0).abs() < 1.0e-6);
        let target = shortest_angle_target(10.0, 350.0);
        assert!((target + 10.0).abs() < 1.0e-6);
    }

    #[test]
    fn public_easing_maps_to_native_power_exponent() {
        assert!((native_easing_exponent(0.0) - 1.0).abs() < 1.0e-6);
        assert!((native_easing_exponent(1.0) - 2.0).abs() < 1.0e-6);
        assert!((native_easing_exponent(-1.0) - 0.5).abs() < 1.0e-6);
        assert!((native_control_easing(0.5, 0.0) - 0.5).abs() < 1.0e-6);
        assert!((native_control_easing(0.5, 1.0) - 0.25).abs() < 1.0e-6);
        assert!((native_control_easing(0.25, -1.0) - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn active_mirror_matches_native_user_xor_metadata_truth_table() {
        assert!(!active_mirror_state(false, false));
        assert!(active_mirror_state(true, false));
        assert!(active_mirror_state(false, true));
        assert!(!active_mirror_state(true, true));
    }

    #[test]
    fn stereovision_two_screen_coefficients_match_native_builder() {
        // sub_1027A4D0 with min=-10, max=10, fov=.2 and level=1.0.
        // Screen-vector 0 anchors at max and vector 1 anchors at min.
        let left = stereovision_screen_for_range(-10.0, 10.0, 0, 2, 1.0, 0.2)
            .expect("screen 0");
        let right = stereovision_screen_for_range(-10.0, 10.0, 1, 2, 1.0, 0.2)
            .expect("screen 1");
        assert!((left.slope - 0.8).abs() < 1.0e-6);
        assert!((left.intercept - 2.0).abs() < 1.0e-6);
        assert!((right.slope - 0.8).abs() < 1.0e-6);
        assert!((right.intercept + 2.0).abs() < 1.0e-6);
        assert!(((left.slope * 0.0 + left.intercept) - 2.0).abs() < 1.0e-6);
        assert!(((right.slope * 0.0 + right.intercept) + 2.0).abs() < 1.0e-6);
    }

    #[test]
    fn stereovision_screen_order_reverses_with_active_mirror() {
        assert_eq!(stereovision_vector_index(0, 2, false), Some(0));
        assert_eq!(stereovision_vector_index(1, 2, false), Some(1));
        assert_eq!(stereovision_vector_index(0, 2, true), Some(1));
        assert_eq!(stereovision_vector_index(1, 2, true), Some(0));
    }

    #[test]
    fn stereovision_projection_only_changes_resolved_targets() {
        let mut pipeline = EmoteRuntimePipeline::default();
        pipeline.stereovision_control = Some(EmoteStereovisionControl {
            variable_match_list: vec!["face_lr".to_owned()],
        });
        let info = |name: &str| EmoteVariableInfo {
            name: name.to_owned(),
            default_value: 0.0,
            min_value: Some(-10.0),
            max_value: Some(10.0),
            frames: Vec::new(),
        };
        let mut values = BTreeMap::new();
        values.insert(
            "face_lr".to_owned(),
            EmoteVariableState {
                info: info("face_lr"),
                value: 5.0,
                target: None,
            },
        );
        values.insert(
            "face_lr_extra".to_owned(),
            EmoteVariableState {
                info: info("face_lr_extra"),
                value: 5.0,
                target: None,
            },
        );
        apply_stereovision_screen_projection(
            &pipeline,
            &mut values,
            0,
            2,
            1.0,
            0.2,
            false,
        );
        assert!((values["face_lr"].value - 6.0).abs() < 1.0e-6);
        assert!((values["face_lr_extra"].value - 5.0).abs() < 1.0e-6);
    }

    #[test]
    fn mirror_pass_uses_substring_matching_without_mutating_source_copy() {
        let mut pipeline = EmoteRuntimePipeline::default();
        pipeline.mirror_enabled = true;
        pipeline.mirror_control = Some(MirrorControl {
            variable_match_list: vec!["face_lr".to_owned()],
        });

        let info = |name: &str| EmoteVariableInfo {
            name: name.to_owned(),
            default_value: 0.0,
            min_value: None,
            max_value: None,
            frames: Vec::new(),
        };
        let mut logical = BTreeMap::new();
        logical.insert(
            "char/face_lr_main".to_owned(),
            EmoteVariableState {
                info: info("char/face_lr_main"),
                value: 3.0,
                target: None,
            },
        );
        logical.insert(
            "char/face_ud_main".to_owned(),
            EmoteVariableState {
                info: info("char/face_ud_main"),
                value: 4.0,
                target: None,
            },
        );
        let mut evaluated = logical.clone();
        apply_post_control_variable_passes(&pipeline, &mut evaluated);
        assert_eq!(logical["char/face_lr_main"].value, 3.0);
        assert_eq!(evaluated["char/face_lr_main"].value, -3.0);
        assert_eq!(evaluated["char/face_ud_main"].value, 4.0);
    }

    #[test]
    fn bust_controller_preserves_param_op_as_root_on_first_step() {
        let def = PhysicsControlDefinition {
            label: "bust".to_owned(),
            enabled: true,
            base_layer: None,
            parameter: None,
            var_lr: None,
            var_ud: None,
            var_lrm: None,
            fields: BTreeMap::new(),
        };
        let mut state = BustPhysicsState {
            root: [10.0, 20.0, 7.0],
            bob: [1.0, 2.0, 3.0],
            vel: [0.0; 3],
            ofs: 0.0,
            group_first_tick: true,
            controller_first_tick: true,
            root_delta: [0.0; 2],
            last_anchor: None,
        };
        let mut variables = BTreeMap::new();

        step_bust_physics(
            &mut state,
            &def,
            1.0,
            [100.0, 200.0, 9.0],
            0.0,
            [5.0, 6.0],
            1.0,
            &mut variables,
        );

        // Native first group step calls EPBustControl with dt=0. param.op is
        // still the root; only root-input is captured into +40/+44.
        assert_eq!(state.root, [10.0, 20.0, 7.0]);
        assert_eq!(state.bob, [1.0, 2.0, 3.0]);
        assert_eq!(state.root_delta, [-95.0, -186.0]);
        assert_eq!(state.last_anchor, Some([100.0, 200.0, 9.0]));
    }

    #[test]
    fn bust_group_interpolates_to_forced_endpoint_and_keeps_root_z() {
        let def = PhysicsControlDefinition {
            label: "bust".to_owned(),
            enabled: true,
            base_layer: None,
            parameter: None,
            var_lr: None,
            var_ud: None,
            var_lrm: None,
            fields: BTreeMap::new(),
        };
        let mut state = BustPhysicsState {
            root: [10.0, 20.0, 7.0],
            bob: [1.0, 2.0, 3.0],
            vel: [0.0; 3],
            ofs: 0.0,
            group_first_tick: true,
            controller_first_tick: true,
            root_delta: [0.0; 2],
            last_anchor: None,
        };
        let mut variables = BTreeMap::new();
        step_bust_physics(
            &mut state,
            &def,
            1.0,
            [100.0, 200.0, 9.0],
            0.0,
            [5.0, 6.0],
            1.0,
            &mut variables,
        );
        step_bust_physics(
            &mut state,
            &def,
            1.0,
            [110.0, 210.0, 99.0],
            0.0,
            [2.0, 4.0],
            1.0,
            &mut variables,
        );

        // previous stored endpoint is [100,200] without force; after one
        // native-sized step ratio==1, input is [112,214]. Saved delta from
        // the first call was [-95,-186], therefore root.xy=[17,28].
        assert_eq!(state.root, [17.0, 28.0, 7.0]);
        assert_eq!(state.last_anchor, Some([110.0, 210.0, 99.0]));
    }

    #[test]
    fn physics_output_response_is_odd_and_not_a_deadzone() {
        let small = physics_output_response(0.005);
        assert!(small > 0.0);
        assert!((physics_output_response(-3.0) + physics_output_response(3.0)).abs() < 1.0e-5);
    }

    #[test]
    fn ep_graph_same_line_uses_direct_native_segment() {
        let route = build_ep_graph_route(&[[0.0, 10.0]], &[], 2.0, 8.0);
        assert_eq!(route.segments, vec![[2.0, 8.0]]);
        assert!((route.total_distance - 6.0).abs() < 1.0e-6);
    }

    #[test]
    fn ep_graph_cross_node_is_zero_distance_junction() {
        let edges = [[0.0, 10.0], [20.0, 30.0]];
        let nodes = vec![vec![10.0, 20.0]];
        let route = build_ep_graph_route(&edges, &nodes, 2.0, 28.0);
        assert_eq!(route.segments, vec![[2.0, 10.0], [20.0, 28.0]]);
        assert!((route.total_distance - 16.0).abs() < 1.0e-6);
    }

    #[test]
    fn ep_graph_unreachable_target_uses_native_zero_length_fallback() {
        let route = build_ep_graph_route(&[[0.0, 10.0]], &[], 2.0, 20.0);
        assert_eq!(route.segments, vec![[20.0, 20.0]]);
        assert_eq!(route.total_distance, 0.0);
    }

    #[test]
    fn graph_setter_ignores_slower_duplicate_target() {
        let mut state = GraphControlState::new(0.0);
        state.stage = 2;
        state.segment_target = 10.0;
        state.total_distance = 10.0;
        state.traversed_distance = 5.0;
        state.inv_duration = 0.1; // 10 ticks total
        state.easing_exponent = 1.0;
        set_graph_control_target(&mut state, 10.0, 6.0, 0.0, false);
        assert!(state.queue.is_empty());
        assert_eq!(state.stage, 2); // 5 ticks remain, so a 6-tick duplicate is ignored.

        set_graph_control_target(&mut state, 10.0, 4.0, 0.0, false);
        assert_eq!(state.stage, 0);
        assert_eq!(state.queue.len(), 1);
        assert_eq!(state.queue[0].target, 10.0);
    }

    #[test]
    fn vector2_transition_matches_native_immediate_and_midpoint() {
        let mut state = Vector2TransitionState::default();
        set_vector2_transition_target(&mut state, [4.0, -2.0], 0.0, 0.0, false);
        assert_eq!(state.current, [4.0, -2.0]);
        assert!(!state.active);

        set_vector2_transition_target(&mut state, [8.0, 2.0], 10.0, 0.0, false);
        step_vector2_transition(&mut state, 5.0);
        assert!((state.current[0] - 6.0).abs() < 1.0e-6);
        assert!((state.current[1] - 0.0).abs() < 1.0e-6);
    }

    #[test]
    fn vector2_transition_ignores_only_slower_duplicate_target() {
        let mut state = Vector2TransitionState::default();
        set_vector2_transition_target(&mut state, [10.0, 20.0], 10.0, 0.0, false);
        step_vector2_transition(&mut state, 5.0);
        let progress = state.progress;
        set_vector2_transition_target(&mut state, [10.0, 20.0], 6.0, 0.0, false);
        assert_eq!(state.progress, progress);
        assert!(state.active);

        set_vector2_transition_target(&mut state, [10.0, 20.0], 4.0, 0.0, false);
        assert_eq!(state.progress, 0.0);
        assert!(state.active);
    }

    #[test]
    fn vector2_transition_queue_starts_next_command_in_same_step() {
        let mut state = Vector2TransitionState::default();
        set_vector2_transition_target(&mut state, [1.0, 0.0], 1.0, 0.0, false);
        set_vector2_transition_target(&mut state, [2.0, 0.0], 2.0, 0.0, true);
        step_vector2_transition(&mut state, 1.0);
        assert_eq!(state.current, [1.5, 0.0]);
        assert_eq!(state.target, [2.0, 0.0]);
        assert!(state.active);
    }

    fn empty_test_scene() -> EmoteStaticScene {
        EmoteStaticScene::empty_for_tests()
    }

    #[test]
    fn authored_ranges_do_not_clamp_ordinary_variable_writes() {
        let info = EmoteVariableInfo {
            name: "free".to_owned(),
            default_value: 0.5,
            min_value: Some(0.0),
            max_value: Some(1.0),
            frames: Vec::new(),
        };
        let mut player = ElunaPlayer::from_scene_and_variables(empty_test_scene(), vec![info]);
        player.set_variable_immediate("free", 2.5);
        assert_eq!(player.variable_value("free"), Some(2.5));
        player.set_variable_timed("free", -2.0, 10.0, 0.0);
        player.progress_ticks_without_physics(10.0);
        assert_eq!(player.variable_value("free"), Some(-2.0));
    }

    #[test]
    fn timeline_hold_markers_do_not_pollute_authored_ranges() {
        let variable = EmoteTimelineVariable {
            name: "pose".to_owned(),
            frames: vec![
                EmoteTimelineFrame { time_ticks: 0.0, hold: true, value: 0.0, easing: 0.0 },
                EmoteTimelineFrame { time_ticks: 10.0, hold: false, value: 3.0, easing: 0.0 },
                EmoteTimelineFrame { time_ticks: 20.0, hold: false, value: 5.0, easing: 0.0 },
            ],
        };
        let mut out = BTreeMap::new();
        merge_timeline_variable_info(&variable, &mut out);
        let info = &out["pose"];
        assert_eq!(info.default_value, 3.0);
        assert_eq!(info.min_value, Some(3.0));
        assert_eq!(info.max_value, Some(5.0));
        assert_eq!(info.frames.len(), 2);
    }

    #[test]
    fn timeline_only_variable_does_not_take_first_key_as_initial_value() {
        let mut out = BTreeMap::new();
        let variable = EmoteTimelineVariable {
            name: "timeline_only".to_owned(),
            frames: vec![EmoteTimelineFrame {
                time_ticks: 0.0,
                hold: false,
                value: 7.0,
                easing: 0.0,
            }],
        };
        merge_timeline_variable_info(&variable, &mut out);
        assert_eq!(out["timeline_only"].default_value, 0.0);
    }

    #[test]
    fn timeline_merge_preserves_existing_variable_default() {
        let mut out = BTreeMap::new();
        out.insert(
            "pose".to_owned(),
            EmoteVariableInfo {
                name: "pose".to_owned(),
                default_value: 1.25,
                min_value: Some(-1.0),
                max_value: Some(2.0),
                frames: Vec::new(),
            },
        );
        let variable = EmoteTimelineVariable {
            name: "pose".to_owned(),
            frames: vec![EmoteTimelineFrame {
                time_ticks: 0.0,
                hold: false,
                value: 9.0,
                easing: 0.0,
            }],
        };
        merge_timeline_variable_info(&variable, &mut out);
        assert_eq!(out["pose"].default_value, 1.25);
        assert_eq!(out["pose"].max_value, Some(9.0));
    }

    #[test]
    fn ordinary_timeline_frames_are_not_scaled_by_difference_blend_ratio() {
        let info = EmoteVariableInfo {
            name: "pose".to_owned(),
            default_value: 10.0,
            min_value: None,
            max_value: None,
            frames: Vec::new(),
        };
        let timeline = EmoteTimeline {
            name: "main".to_owned(),
            path: None,
            loop_begin_ticks: -1.0,
            loop_end_ticks: -1.0,
            last_time_ticks: 100.0,
            duration_ticks: 100.0,
            variables: vec![EmoteTimelineVariable {
                name: "pose".to_owned(),
                frames: vec![EmoteTimelineFrame {
                    time_ticks: 0.0,
                    hold: false,
                    value: 20.0,
                    easing: 0.0,
                }],
            }],
            is_difference: false,
        };
        let mut player = ElunaPlayer::from_scene_variables_timelines_runtime(
            empty_test_scene(),
            vec![info],
            vec![timeline],
            EmoteRuntimePipeline::default(),
        );
        player.play_timeline("main", TimelinePlayMode::PARALLEL);
        player.set_timeline_blend_ratio("main", 0.0, 0.0, 0.0, false);
        player.set_timeline_time("main", 0.0).unwrap();

        // sub_1026A660/sub_1026B1F0 pass the authored frame value directly on
        // the ordinary setter path. Runtime +36 is a DIFFERENCE weight only.
        assert_eq!(player.variable_value("pose"), Some(20.0));
    }

    #[test]
    fn timeline_blend_setter_does_not_activate_an_inactive_runtime() {
        let timeline = EmoteTimeline {
            name: "diff".to_owned(),
            path: None,
            loop_begin_ticks: -1.0,
            loop_end_ticks: -1.0,
            last_time_ticks: 100.0,
            duration_ticks: 100.0,
            variables: Vec::new(),
            is_difference: true,
        };
        let mut player = ElunaPlayer::from_scene_variables_timelines_runtime(
            empty_test_scene(),
            Vec::new(),
            vec![timeline],
            EmoteRuntimePipeline::default(),
        );
        player.set_timeline_blend_ratio("diff", 0.25, 10.0, 0.0, false);
        assert!(!player.is_timeline_playing("diff"));
        assert!(!player.active_timeline_states.contains_key("diff"));
    }

    #[test]
    fn timeline_stop_flag_removes_when_blend_becomes_idle_even_above_zero() {
        let timeline = EmoteTimeline {
            name: "diff".to_owned(),
            path: None,
            loop_begin_ticks: -1.0,
            loop_end_ticks: -1.0,
            last_time_ticks: 100.0,
            duration_ticks: 100.0,
            variables: Vec::new(),
            is_difference: true,
        };
        let mut player = ElunaPlayer::from_scene_variables_timelines_runtime(
            empty_test_scene(),
            Vec::new(),
            vec![timeline],
            EmoteRuntimePipeline::default(),
        );
        player.play_timeline("diff", TimelinePlayMode::PARALLEL_DIFFERENCE);
        player.set_timeline_blend_ratio("diff", 0.5, 10.0, 0.0, true);
        player.progress_ticks_without_physics(10.0);
        assert!(!player.is_timeline_playing("diff"));
    }

    #[test]
    fn immediate_timeline_stop_waits_for_the_next_progress_step() {
        let timeline = EmoteTimeline {
            name: "diff".to_owned(),
            path: None,
            loop_begin_ticks: -1.0,
            loop_end_ticks: -1.0,
            last_time_ticks: 100.0,
            duration_ticks: 100.0,
            variables: Vec::new(),
            is_difference: true,
        };
        let mut player = ElunaPlayer::from_scene_variables_timelines_runtime(
            empty_test_scene(),
            Vec::new(),
            vec![timeline],
            EmoteRuntimePipeline::default(),
        );
        player.play_timeline("diff", TimelinePlayMode::PARALLEL_DIFFERENCE);
        player.set_timeline_blend_ratio("diff", 0.5, 0.0, 0.0, true);
        assert!(player.is_timeline_playing("diff"));
        player.progress_ticks_without_physics(1.0);
        assert!(!player.is_timeline_playing("diff"));
    }

    #[test]
    fn fade_in_starts_in_parallel_difference_mode_from_zero_blend() {
        let timeline = EmoteTimeline {
            name: "fade".to_owned(),
            path: None,
            loop_begin_ticks: -1.0,
            loop_end_ticks: -1.0,
            last_time_ticks: 100.0,
            duration_ticks: 100.0,
            variables: Vec::new(),
            is_difference: true,
        };
        let mut player = ElunaPlayer::from_scene_variables_timelines_runtime(
            empty_test_scene(),
            Vec::new(),
            vec![timeline],
            EmoteRuntimePipeline::default(),
        );
        player.fade_in_timeline("fade", 10.0, 0.0);
        let state = player.active_timeline_states.get("fade").unwrap();
        assert_eq!(state.mode.flags & 3, 3);
        assert_eq!(state.blend_ratio, 0.0);
        assert_eq!(state.blend_target.as_ref().map(|target| target.target_value), Some(1.0));
    }

    #[test]
    fn queued_timeline_blend_commands_share_the_native_step_delta() {
        let timeline = EmoteTimeline {
            name: "diff".to_owned(),
            path: None,
            loop_begin_ticks: -1.0,
            loop_end_ticks: -1.0,
            last_time_ticks: 100.0,
            duration_ticks: 100.0,
            variables: Vec::new(),
            is_difference: true,
        };
        let mut player = ElunaPlayer::from_scene_variables_timelines_runtime(
            empty_test_scene(),
            Vec::new(),
            vec![timeline],
            EmoteRuntimePipeline::default(),
        );
        player.play_timeline("diff", TimelinePlayMode::PARALLEL_DIFFERENCE);
        player.set_queuing(true);
        player.set_timeline_blend_ratio("diff", 0.0, 10.0, 0.0, false);
        player.set_timeline_blend_ratio("diff", 0.5, 10.0, 0.0, false);
        player.progress_ticks_without_physics(10.0);

        // sub_102164E0 completes the first queued command, starts the next one,
        // and applies the same host delta to that command in the same Step.
        assert!((player.timeline_blend_ratio("diff") - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn difference_timeline_routes_controller_labels_through_native_setter() {
        let info = EmoteVariableInfo {
            name: "fade".to_owned(),
            default_value: 0.0,
            min_value: None,
            max_value: None,
            frames: Vec::new(),
        };
        let timeline = EmoteTimeline {
            name: "diff".to_owned(),
            path: None,
            loop_begin_ticks: -1.0,
            loop_end_ticks: -1.0,
            last_time_ticks: 10.0,
            duration_ticks: 10.0,
            variables: vec![EmoteTimelineVariable {
                name: "fade".to_owned(),
                frames: vec![EmoteTimelineFrame {
                    time_ticks: 0.0,
                    hold: false,
                    value: 1.0,
                    easing: 0.0,
                }],
            }],
            is_difference: true,
        };
        let mut pipeline = EmoteRuntimePipeline::default();
        pipeline.transition_controls.push(TransitionControl {
            label: "fade".to_owned(),
            enabled: true,
        });
        let mut player = ElunaPlayer::from_scene_variables_timelines_runtime(
            empty_test_scene(),
            vec![info],
            vec![timeline],
            pipeline,
        );
        player.play_timeline("diff", TimelinePlayMode::PARALLEL_DIFFERENCE);
        assert_eq!(player.variable_value("fade"), Some(1.0));
        assert!(player.timeline_diff_variables.get("diff").map_or(true, BTreeMap::is_empty));
    }

    #[test]
    fn difference_timeline_frame_value_is_native_delta_not_default_relative() {
        let info = EmoteVariableInfo {
            name: "pose".to_owned(),
            default_value: 10.0,
            min_value: None,
            max_value: None,
            frames: Vec::new(),
        };
        let timeline = EmoteTimeline {
            name: "diff".to_owned(),
            path: None,
            loop_begin_ticks: -1.0,
            loop_end_ticks: -1.0,
            last_time_ticks: 10.0,
            duration_ticks: 10.0,
            variables: vec![EmoteTimelineVariable {
                name: "pose".to_owned(),
                frames: vec![EmoteTimelineFrame {
                    time_ticks: 0.0,
                    hold: false,
                    value: 2.5,
                    easing: 0.0,
                }],
            }],
            is_difference: true,
        };
        let mut player = ElunaPlayer::from_scene_variables_timelines_runtime(
            empty_test_scene(),
            vec![info],
            vec![timeline],
            EmoteRuntimePipeline::default(),
        );

        player.play_timeline("diff", TimelinePlayMode::PARALLEL_DIFFERENCE);
        let values = player.evaluated_variable_values();
        assert_eq!(values.get("pose").copied(), Some(12.5));
    }

    #[test]
    fn difference_overlay_runs_before_loop_control_overwrite() {
        let info = EmoteVariableInfo {
            name: "loop_pose".to_owned(),
            default_value: 0.0,
            min_value: None,
            max_value: None,
            frames: Vec::new(),
        };
        let timeline = EmoteTimeline {
            name: "diff".to_owned(),
            path: None,
            loop_begin_ticks: -1.0,
            loop_end_ticks: -1.0,
            last_time_ticks: 10.0,
            duration_ticks: 10.0,
            variables: vec![EmoteTimelineVariable {
                name: "loop_pose".to_owned(),
                frames: vec![EmoteTimelineFrame {
                    time_ticks: 0.0,
                    hold: false,
                    value: 3.0,
                    easing: 0.0,
                }],
            }],
            is_difference: true,
        };
        let mut pipeline = EmoteRuntimePipeline::default();
        pipeline.loop_controls.push(LoopControl {
            label: Some("loop".to_owned()),
            enabled: true,
            var_loop: Some("loop_pose".to_owned()),
            transition_list: vec![LoopTransition {
                start: 7.0,
                end: 7.0,
                duration_ticks: 1.0,
            }],
        });
        let mut player = ElunaPlayer::from_scene_variables_timelines_runtime(
            empty_test_scene(),
            vec![info],
            vec![timeline],
            pipeline,
        );
        player.play_timeline("diff", TimelinePlayMode::PARALLEL_DIFFERENCE);
        player.pass();

        // sub_10275A30 first makes the reference 0+3, then LoopControl writes
        // 7. The old final-overlay implementation incorrectly returned 10.
        assert_eq!(player.evaluated_variable_values().get("loop_pose").copied(), Some(7.0));
    }

    #[test]
    fn physics_output_is_published_after_mirror_and_clamp_stage() {
        let info = EmoteVariableInfo {
            name: "hair_lr".to_owned(),
            default_value: 0.0,
            min_value: None,
            max_value: None,
            frames: Vec::new(),
        };
        let mut pipeline = EmoteRuntimePipeline::default();
        pipeline.mirror_enabled = true;
        pipeline.mirror_control = Some(MirrorControl {
            variable_match_list: vec!["hair_lr".to_owned()],
        });
        let mut player = ElunaPlayer::from_scene_variables_timelines_runtime(
            empty_test_scene(),
            vec![info],
            Vec::new(),
            pipeline,
        );
        player.variables.get_mut("hair_lr").unwrap().value = 2.0;
        player
            .pre_physics_output_values
            .insert("hair_lr".to_owned(), 0.5);

        // Native mirrors the pre-physics 0.5, then the solver overwrites the
        // reference with +2.0. The old Rust final pass mirrored +2.0 to -2.0.
        assert_eq!(player.evaluated_variable_values().get("hair_lr").copied(), Some(2.0));

        let mut mouth = player.variables["hair_lr"].clone();
        mouth.info.name = "face_talk".to_owned();
        player.variables.insert("face_talk".to_owned(), mouth);
        player.set_variable_immediate("face_talk", 0.75);
        assert_eq!(player.evaluated_variable_values()["hair_lr"], 2.0);
        // An explicit write to the physics output itself still goes through
        // the ordinary variable mirror/clamp stage.
        player.set_variable_immediate("hair_lr", 3.0);
        assert_eq!(player.evaluated_variable_values()["hair_lr"], -3.0);
    }

    #[test]
    fn native_timeline_flag_bit_two_skips_controller_backed_tracks() {
        let info = EmoteVariableInfo {
            name: "fade".to_owned(),
            default_value: 0.0,
            min_value: None,
            max_value: None,
            frames: Vec::new(),
        };
        let timeline = EmoteTimeline {
            name: "skip".to_owned(),
            path: None,
            loop_begin_ticks: -1.0,
            loop_end_ticks: -1.0,
            last_time_ticks: 10.0,
            duration_ticks: 10.0,
            variables: vec![EmoteTimelineVariable {
                name: "fade".to_owned(),
                frames: vec![EmoteTimelineFrame {
                    time_ticks: 0.0,
                    hold: false,
                    value: 1.0,
                    easing: 0.0,
                }],
            }],
            is_difference: false,
        };
        let mut pipeline = EmoteRuntimePipeline::default();
        pipeline.transition_controls.push(TransitionControl {
            label: "fade".to_owned(),
            enabled: true,
        });
        let mut player = ElunaPlayer::from_scene_variables_timelines_runtime(
            empty_test_scene(),
            vec![info],
            vec![timeline],
            pipeline,
        );
        player.play_timeline(
            "skip",
            TimelinePlayMode {
                flags: 1 << 2,
                looping: false,
            },
        );
        assert_eq!(player.variable_value("fade"), Some(0.0));
    }

    #[test]
    fn timeline_list_indices_are_not_part_of_native_labels() {
        let timeline = |label: &str| {
            PsbValue::Object(vec![
                ("label".to_owned(), PsbValue::String(label.to_owned())),
                ("variableList".to_owned(), PsbValue::List(Vec::new())),
                ("lastTime".to_owned(), PsbValue::Float(10.0)),
            ])
        };
        let value = PsbValue::List(vec![timeline("idle"), timeline("sample")]);
        let mut out = Vec::new();
        collect_timeline_nodes(&value, &mut out);
        assert_eq!(out.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(), vec!["idle", "sample"]);
    }

    #[test]
    fn player_constructor_does_not_apply_timeline_first_key_before_play() {
        let info = EmoteVariableInfo {
            name: "pose".to_owned(),
            default_value: 2.0,
            min_value: Some(-10.0),
            max_value: Some(10.0),
            frames: Vec::new(),
        };
        let timeline = EmoteTimeline {
            name: "main".to_owned(),
            path: None,
            loop_begin_ticks: -1.0,
            loop_end_ticks: -1.0,
            last_time_ticks: 10.0,
            duration_ticks: 10.0,
            variables: vec![EmoteTimelineVariable {
                name: "pose".to_owned(),
                frames: vec![EmoteTimelineFrame {
                    time_ticks: 0.0,
                    hold: false,
                    value: 8.0,
                    easing: 0.0,
                }],
            }],
            is_difference: false,
        };
        let player = ElunaPlayer::from_scene_variables_timelines(
            empty_test_scene(),
            vec![info],
            vec![timeline],
        );
        assert_eq!(player.variable_value("pose"), Some(2.0));
        assert!(player.active_timelines().is_empty());
    }

    #[test]
    fn runtime_variable_collection_uses_physics_outputs_not_control_identity() {
        let mut out = BTreeMap::new();
        let pipeline = EmoteRuntimePipeline {
            physics_controls: vec![PhysicsControl::Bust(PhysicsControlDefinition {
                label: "bust-controller".to_owned(),
                enabled: true,
                base_layer: Some("胸".to_owned()),
                parameter: None,
                var_lr: Some("bust_LR".to_owned()),
                var_ud: Some("bust_UD".to_owned()),
                var_lrm: None,
                fields: BTreeMap::new(),
            })],
            ..EmoteRuntimePipeline::default()
        };
        collect_runtime_control_variable_infos(&pipeline, &mut out);
        assert!(!out.contains_key("bust-controller"));
        assert!(out.contains_key("bust_LR"));
        assert!(out.contains_key("bust_UD"));
    }

    #[test]
    fn native_timeline_parser_preserves_frame_list_order() {
        let frame = |time: f32, value: f32| {
            PsbValue::Object(vec![
                ("time".to_owned(), PsbValue::Float(time)),
                ("type".to_owned(), PsbValue::Int(1)),
                (
                    "content".to_owned(),
                    PsbValue::Object(vec![("value".to_owned(), PsbValue::Float(value))]),
                ),
            ])
        };
        let value = PsbValue::List(vec![PsbValue::Object(vec![
            ("label".to_owned(), PsbValue::String("ordered".to_owned())),
            (
                "variableList".to_owned(),
                PsbValue::List(vec![PsbValue::Object(vec![
                    ("label".to_owned(), PsbValue::String("pose".to_owned())),
                    (
                        "frameList".to_owned(),
                        PsbValue::List(vec![frame(5.0, 1.0), frame(2.0, 2.0)]),
                    ),
                ])]),
            ),
            ("lastTime".to_owned(), PsbValue::Float(-1.0)),
        ])]);
        let mut out = Vec::new();
        collect_timeline_nodes(&value, &mut out);
        assert_eq!(out[0].variables[0].frames[0].time_ticks, 5.0);
        assert_eq!(out[0].variables[0].frames[1].time_ticks, 2.0);
        assert_eq!(out[0].last_time_ticks, 2.0);
    }

    #[test]
    fn wind_sample_uses_first_matching_pulse_and_direction() {
        let mut wind = WindState {
            start: 0.0,
            goal: 100.0,
            speed: 1.0,
            pow_min: 2.0,
            pow_max: 2.0,
            elapsed_ticks: 0.0,
            spawn_accumulator: 0.0,
            signed_speed: 1.0,
            pulses: [WindPulse::default(); 128],
        };
        wind.pulses[0] = WindPulse {
            active: true,
            position: 10.0,
            power: 2.0,
        };
        assert_eq!(sample_wind(&wind, 10.0), 2.0);
        assert_eq!(sample_wind(&wind, 14.0), 0.0); // native support test is strict
        wind.signed_speed = -1.0;
        assert_eq!(sample_wind(&wind, 10.0), -2.0);
    }
}
