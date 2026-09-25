//! Emote-specific PSB schema extraction and static draw-list generation.
//!
//! This module owns Emote layer/frame traversal and the recovered pieces of the
//! StepFrameMeshChain draw-list path. Semantics that are not confirmed from the
//! original driver are carried as explicit draw/runtime metadata instead of
//! being silently guessed.

use crate::{PsbFile, PsbValue};
use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt;
use std::sync::Arc;
#[cfg(debug_assertions)]
use std::sync::{Mutex, OnceLock};

#[cfg(debug_assertions)]
static MISSING_PARAMETER_VARIABLES: OnceLock<Mutex<BTreeSet<String>>> = OnceLock::new();

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteModelSchema {
    pub base_object: String,
    pub spec: Option<String>,
    pub textures: BTreeMap<String, EmoteTextureSource>,
    /// Parsed `metadata.stereovisionControl`. The native Init path resolves
    /// metadata first, then loads this after timelineControl and before
    /// charaProfile.
    pub stereovision: Option<EmoteStereovisionControl>,
    /// Root archive `stereovisionProfile`, matching
    /// MMotionManager::ExtractStereovisionProfileFromArchive.  Extraction is
    /// separate from MEmotePlayer::Init in the DLL, so this is exposed to the
    /// host and is not implicitly applied to player state.
    pub stereovision_profile: Option<EmoteStereovisionProfile>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmoteStereovisionProfile {
    pub fov: f32,
    pub f_level: f32,
    pub len_disp: f32,
    pub dist_e2d: f32,
    pub dist_eye: f32,
    pub eye_angle_ltd: f32,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EmoteStereovisionControl {
    /// Raw authored variable matching expressions, in serialized order.
    pub variable_match_list: Vec<String>,
}


#[derive(Debug, Clone, PartialEq)]
pub struct EmoteMotionInfo {
    pub name: String,
    pub duration_ticks: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteTextureSource {
    pub name: String,
    pub resource_index: u32,
    pub width: u32,
    pub height: u32,
    pub format: Option<String>,
    pub compress: Option<String>,
    pub bit_count: Option<u32>,
    pub icons: BTreeMap<String, EmoteTextureIcon>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteTextureIcon {
    pub texture_name: String,
    pub name: String,
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    pub origin_x: f32,
    pub origin_y: f32,
    pub resolution: f32,
    pub attr: Option<u32>,
}

impl EmoteTextureIcon {
    pub fn resolved_width(&self) -> f32 {
        self.width * self.resolution
    }

    pub fn resolved_height(&self) -> f32 {
        self.height * self.resolution
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmoteMeshPatch {
    pub division_x: u32,
    pub division_y: u32,
    pub domain: Option<[f32; 4]>,
    /// Sixteen cubic Bezier patch control points, row-major, each point in
    /// normalized local sprite space. The identity patch is
    /// `(col / 3, row / 3)`.
    pub control_points: [[f32; 2]; 16],
}

/// A mesh deformation and the coordinate frame in which its domain is authored.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct MeshChainEntry {
    patch: EmoteMeshPatch,
    transform: [f32; 6],
}

impl MeshChainEntry {
    fn warp_world_point(&self, point: [f32; 2]) -> Option<[f32; 2]> {
        let m = self.transform;
        let inv = affine_inverse_linear(m)?;
        let local = apply_linear4(inv, [point[0] - m[4], point[1] - m[5]]);
        let warped = mesh_sync_warp_point(MeshSyncChildState {
            patch: self.patch, mask: 1, coordinate: Some(0),
        }, local)?;
        let world = apply_linear4([m[0], m[1], m[2], m[3]], warped);
        Some([world[0] + m[4], world[1] + m[5]])
    }
}

impl EmoteMeshPatch {
    pub fn identity(division_x: u32, division_y: u32) -> Self {
        let mut control_points = [[0.0; 2]; 16];
        for row in 0..4 {
            for col in 0..4 {
                let index = row * 4 + col;
                control_points[index] = [col as f32 / 3.0, row as f32 / 3.0];
            }
        }
        Self {
            division_x: division_x.max(1),
            division_y: division_y.max(1),
            domain: None,
            control_points,
        }
    }

    pub fn sample(&self, u: f32, v: f32) -> [f32; 2] {
        // Native patch evaluation (sub_10387C50) uses u/v directly.  Do not
        // clamp here: meshSyncChild samples +/-0.0001 around an edge and
        // combined patches may legitimately evaluate just outside [0,1].
        let bu = cubic_basis(u);
        let bv = cubic_basis(v);
        let mut out = [0.0f32; 2];
        for row in 0..4 {
            for col in 0..4 {
                let w = bv[row] * bu[col];
                let p = self.control_points[row * 4 + col];
                out[0] += p[0] * w;
                out[1] += p[1] * w;
            }
        }
        out
    }

    pub fn combined_with(&self, next: &EmoteMeshPatch) -> EmoteMeshPatch {
        // Native combineMesh (sub_1038B340) does not compose Bezier
        // mappings.  It adds the source displacement from the canonical
        // 4x4 identity mesh to the destination point-by-point:
        //
        //     dst[i] += src[i] - identity[i]
        //
        // Both patches in the Rust representation are absolute control-point
        // meshes, so combining them is the same operation around identity.
        let division_x = self.division_x.max(next.division_x);
        let division_y = self.division_y.max(next.division_y);
        let identity = EmoteMeshPatch::identity(division_x, division_y);
        let mut out = next.clone();
        out.division_x = division_x;
        out.division_y = division_y;
        for i in 0..16 {
            out.control_points[i][0] +=
                self.control_points[i][0] - identity.control_points[i][0];
            out.control_points[i][1] +=
                self.control_points[i][1] - identity.control_points[i][1];
        }
        out.domain = self.domain.or(next.domain);
        out
    }

    pub fn interpolate(a: &EmoteMeshPatch, b: &EmoteMeshPatch, t: f32) -> EmoteMeshPatch {
        let t = t.clamp(0.0, 1.0);
        let mut out = EmoteMeshPatch::identity(
            a.division_x.max(b.division_x),
            a.division_y.max(b.division_y),
        );
        for i in 0..16 {
            out.control_points[i][0] =
                a.control_points[i][0] + (b.control_points[i][0] - a.control_points[i][0]) * t;
            out.control_points[i][1] =
                a.control_points[i][1] + (b.control_points[i][1] - a.control_points[i][1]) * t;
        }
        out.domain = match (a.domain, b.domain) {
            (Some(a), Some(b)) => Some([
                lerp(a[0], b[0], t),
                lerp(a[1], b[1], t),
                lerp(a[2], b[2], t),
                lerp(a[3], b[3], t),
            ]),
            (Some(domain), None) | (None, Some(domain)) => Some(domain),
            (None, None) => None,
        };
        out
    }

    pub fn control_bounds(&self) -> (f32, f32, f32, f32) {
        let mut min_x = self.control_points[0][0];
        let mut min_y = self.control_points[0][1];
        let mut max_x = self.control_points[0][0];
        let mut max_y = self.control_points[0][1];
        for p in &self.control_points[1..] {
            min_x = min_x.min(p[0]);
            min_y = min_y.min(p[1]);
            max_x = max_x.max(p[0]);
            max_y = max_y.max(p[1]);
        }
        (min_x, min_y, max_x, max_y)
    }
}

fn cubic_basis(t: f32) -> [f32; 4] {
    let s = 1.0 - t;
    [s * s * s, 3.0 * s * s * t, 3.0 * s * t * t, t * t * t]
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteStaticScene {
    pub base_object: String,
    pub sprites: Vec<EmoteStaticSprite>,
    pub bounds: Option<EmoteSceneBounds>,
    pub draw_frame_info: Vec<EmoteDrawFrameInfo>,
    pub layer_states: Vec<EmoteStepFrameLayerState>,
    // Native MMotionPlayer keeps each layer's decoded frame state alive across
    // StepFrame calls. A serialized type-0 frame returns before updating that
    // state, so the next scene rebuild must be able to reuse the previous local
    // frame values instead of reconstructing a neutral frame. Keep this
    // implementation detail private; callers observe it through normal scene
    // progression only.
    frame_runtime_states: BTreeMap<String, DynamicFrameState>,
    /// Composite-mask owners encountered during traversal, keyed by the owner
    /// layer's full path.  Value is the list of resolved source-layer paths
    /// taken from `stencilCompositeMaskLayerList` on that owner.
    ///
    /// Source of truth: `sub_103390C0` lines 407-528 (the second pass).  The
    /// owner is a layer with `stencilType & 4` and the source list is read
    /// from the OWNER, not inherited into descendants.  The renderer keys
    /// alpha-mask references by owner path; descendants whose
    /// `parent_mask_path` equals an owner path sample that owner's mask.
    pub composite_mask_owners: BTreeMap<String, Vec<String>>,
    /// Exact structural counterpart of `composite_mask_owners`, keyed by the
    /// owner's native recursive DrawFrameInfo key and containing source DFI
    /// keys. The GPU renderer uses this map so duplicate/empty authored labels
    /// cannot alias stencil sources.
    pub composite_mask_sources_by_key: BTreeMap<Vec<u64>, Vec<Vec<u64>>>,
    /// Persistent native type-4 emitter state. This remains private so callers
    /// cannot mutate MMotionPlayer's StepFrameParticle lifecycle out of band.
    particle_emitters: BTreeMap<String, ParticleEmitterRuntime>,
    /// Raw caller time (not loop-wrapped effective motion time), used to make
    /// particle progression deterministic across the SDK's two scene rebuilds
    /// within one host tick.
    scene_time_ticks: f32,
    /// Root MMotionPlayer type-5 Camera state. This is kept for compatibility
    /// with callers that drive only the root player. Native StepFrameCamera
    /// stores the value on the owning MMotionPlayer; it is not consumed by the
    /// standard 2-D DrawFrameInfo renderer.
    pub camera_runtime: Option<EmoteCameraRuntimeState>,
    /// Camera state for every flattened MMotionPlayer scope, keyed by that
    /// scope's root layer path. Nested type-3 and particle child players own
    /// independent Camera fields in the DLL, so flattening must not discard
    /// those player-local states.
    pub camera_runtimes: BTreeMap<String, EmoteCameraRuntimeState>,
}

impl EmoteStaticScene {
    /// Keeps only sprites that belong to one motion name and recomputes bounds.
    ///
    /// This is a preview helper. The original runtime motion selection still
    /// needs the `FindMotion` / timeline code path from the DLL.
    pub fn filter_motion(mut self, motion_name: &str) -> Self {
        self.sprites
            .retain(|sprite| sprite.motion_name == motion_name);
        self.bounds = compute_bounds(&self.sprites);
        self
    }


    #[cfg(test)]
    pub(crate) fn empty_for_tests() -> Self {
        Self {
            base_object: String::new(),
            sprites: Vec::new(),
            bounds: None,
            draw_frame_info: Vec::new(),
            layer_states: Vec::new(),
            frame_runtime_states: BTreeMap::new(),
            composite_mask_owners: BTreeMap::new(),
            composite_mask_sources_by_key: BTreeMap::new(),
            particle_emitters: BTreeMap::new(),
            scene_time_ticks: 0.0,
            camera_runtime: None,
            camera_runtimes: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EmoteSceneBounds {
    pub min_x: f32,
    pub min_y: f32,
    pub max_x: f32,
    pub max_y: f32,
}

impl EmoteSceneBounds {
    pub fn width(self) -> f32 {
        self.max_x - self.min_x
    }

    pub fn height(self) -> f32 {
        self.max_y - self.min_y
    }

    pub fn center(self) -> [f32; 2] {
        [
            (self.min_x + self.max_x) * 0.5,
            (self.min_y + self.max_y) * 0.5,
        ]
    }

    fn include_rect(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        self.min_x = self.min_x.min(left);
        self.min_y = self.min_y.min(top);
        self.max_x = self.max_x.max(right);
        self.max_y = self.max_y.max(bottom);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteStaticSprite {
    pub label: Option<String>,
    pub motion_name: String,
    pub texture_name: String,
    pub texture_resource_index: u32,
    pub texture_width: u32,
    pub texture_height: u32,
    pub texture_format: Option<String>,
    pub icon_name: String,
    /// True for native type-10 Feedback drawables. Their texture comes from
    /// the previous framebuffer rather than a PSB texture resource.
    pub feedback_history: bool,
    pub z: f32,
    pub opacity: f32,
    /// Native decoded-frame blend mode (`bm`). The low nibble selects the
    /// D3D blend equation; high nibble 0x10 selects MODULATE2X texture color.
    pub blend_mode: u32,
    /// Native decoded-frame blend parameter (`bp`). Kept even for modes whose
    /// fixed-function path does not consume it so specialized passes can use it.
    pub blend_parameter: f32,
    /// Native four-corner colors in serialized 0xRRGGBBAA byte order.
    pub corner_colors: [u32; 4],
    pub visible: bool,
    pub center_x: f32,
    pub center_y: f32,
    pub width: f32,
    pub height: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    pub rotation_degrees: f32,
    pub world_transform: [f32; 6],
    pub uv_left: f32,
    pub uv_top: f32,
    pub uv_right: f32,
    pub uv_bottom: f32,
    pub mesh: Option<EmoteMeshPatch>,
    pub draw_frame_info: EmoteDrawFrameInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteDrawFrameInfo {
    pub layer_label: Option<String>,
    /// Exact native DrawFrameInfo emission key. Each MMotionPlayer contributes
    /// one priority rank; nested players append another component because
    /// sub_103390C0 recursively emits the child player at the parent layer's
    /// priority slot. Particle instances append their creation serial before
    /// the child-player rank. This records the native per-player emission
    /// order; MMotionManager subsequently sorts the emitted FrameInfo pointers
    /// by resolved Z (DFI+36) and player-group value (DFI+8).
    pub native_draw_key: Vec<u64>,
    pub draw_index: usize,
    pub path: String,
    pub layer_type: i64,
    pub ready_to_draw: bool,
    pub submitted_to_draw_frame: bool,
    pub mesh_transform: i64,
    pub mesh_combine: bool,
    pub mesh_sync_child_mask: i64,
    pub mesh_sync_child_coord: bool,
    pub mesh_sync_child_angle: bool,
    pub mesh_sync_child_zoom: bool,
    pub mesh_sync_child_shape: bool,
    pub join_target: bool,
    pub inherit_mask: Option<i64>,
    pub inherit_parent: bool,
    pub inherit_opacity: bool,
    pub inherit_shape: bool,
    pub inherit_angle: bool,
    pub transform_order: Vec<i64>,
    pub coordinate: Option<i64>,
    pub ground_correction: bool,
    /// Native type-0 `objTriPriority`, copied to DrawFrameInfo +84 by
    /// sub_103390C0. The standard 2-D renderer does not consume it.
    pub obj_tri_priority: i64,
    /// Native type-7 propagated clip rectangle (sub_10352360), in model
    /// coordinates before public player transform.
    pub clip_rect: Option<[f32; 4]>,
    pub stencil_type: i64,
    pub stencil_phase: i64,
    pub stencil_composite_item: bool,
    /// Type-12 wipe runtime derived by sub_1032FB00.
    pub stencil_wipe_enabled: bool,
    pub stencil_wipe_reverse: bool,
    pub stencil_wipe_scale: f32,
    pub stencil_wipe_bias: f32,
    pub stencil_composite_mask_layer_list: Vec<String>,
    pub stencil_composite_target_paths: Vec<String>,
    pub parent_mask_path: Option<String>,
    /// Native DrawFrameInfo +120 owner resolved by StepFrameReadyToDraw. This
    /// is the nearest active stencil-ready ancestor and is distinct from the
    /// composite-mask owner list used by stencilType bit 2.
    pub stencil_parent_path: Option<String>,
    /// Exact DrawFrameInfo+120 identity. Paths are retained for diagnostics,
    /// but renderer-side stencil chaining uses this structural key because
    /// authored labels are not guaranteed to be unique.
    pub stencil_parent_native_key: Option<Vec<u64>>,
    pub control_parameter: Option<String>,
    pub control_value: Option<f32>,
    pub local_time_ticks: Option<f32>,
    pub frame_index: Option<usize>,
    pub next_frame_index: Option<usize>,
    pub interpolation_t: f32,
    pub pass: EmoteDrawPass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmoteDrawPass {
    Normal,
    MaskGeneration,
    StencilCompositeMask,
    Filtered,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteStepFrameInput {
    pub motion_name: String,
    pub time_ticks: f32,
    pub variables: BTreeMap<String, f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteStepFrameLayerState {
    pub path: String,
    /// Structural sibling-index path local to the owning MMotionPlayer. This
    /// mirrors the preorder LayerInfo tree and remains unique even when authored
    /// labels are empty or duplicated. Native parent/stencil pointer recovery
    /// must use this identity rather than the human-readable `path`.
    scope_index_path: String,
    /// Internal flattened-player scope root. Every nested MMotionPlayer gets
    /// its own root path so specialized passes can recover per-player state.
    motion_scope_root_path: String,
    /// Native StepFrame raw/composite XYZ (`layerInfo + 612..620`). Anchor and
    /// model direction type 4 operate on this coordinate before MeshChain.
    pub raw_position: [f32; 3],
    /// Native post-MeshChain XYZ (`layerInfo + 120..128`). Shape, Camera,
    /// nested/specialized consumers and Emote soft-body baseLayer lookup use
    /// this coordinate. It is rebuilt from `raw_position` after Anchor.
    pub position: [f32; 3],
    pub(crate) mesh_chain: Vec<MeshChainEntry>,
    pub(crate) frame_offset: [f32; 2],
    /// Current decoded local frame, retained only for the native specialized
    /// pass sequence (Camera/Model/Particle/Feedback). It is intentionally not
    /// part of the public SDK surface.
    specialized_frame: Option<DynamicFrameState>,
    pub transform: [f32; 6],
    pub opacity: f32,
    pub visible: bool,
    /// Type-6 model pass output from sub_103552F0. The Rust renderer does not
    /// assume a particular 3-D backend, but the exact time/direction state is
    /// available to any model backend instead of being silently discarded.
    pub model_runtime: Option<EmoteModelRuntimeState>,
    /// Native type-10 Feedback runtime. A matching synthetic sprite samples
    /// the renderer's previous framebuffer when `active` is true.
    pub feedback_runtime: Option<EmoteFeedbackRuntimeState>,
    /// Native type-1 Shape runtime built by MMotionPlayer::StepFrameShape
    /// (sub_10359D20) after type-7 bounds and before nested motions.
    pub shape_runtime: Option<EmoteShapeRuntimeState>,
    /// Composite linear channels at layer+604 used by native Shape cases 1/2.
    linear_state: FrameLinearState,
    /// Authored type-1 `shape` discriminator allocated at layerInfo+740.
    shape_kind: i32,
    /// Native type-4 static emitter configuration parsed from layerInfo+740.
    /// Kept private because it is an implementation detail of StepFrameParticle.
    particle_static: Option<ParticleStaticConfig>,
    /// Portable reconstruction of native layer+36 dirty/trigger. Native sets
    /// this when the active serialized frame buffer changes and clears it at
    /// the end of StepFrame; interpolation within one key must not retrigger.
    particle_triggered: bool,
    /// Native type-10 Feedback `screenBounds` rectangle (sub_1033ED90).
    /// It also provides the authored screen region used by feedback/screen tests.
    screen_bounds: Option<[f32; 4]>,
    pub draw_frame_info: EmoteDrawFrameInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteModelRuntimeState {
    pub local_time_ticks: f32,
    pub looped: bool,
    pub direction_type: i32,
    pub direction: Option<[f32; 3]>,
    pub direction_target: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteCameraRuntimeState {
    pub layer_path: String,
    pub fov: f32,
    pub eye: [f32; 3],
    pub target: [f32; 3],
    /// MMotionPlayer camera translation stored at player+468. Native builds
    /// this from the selected target and player layer 0 raw +612/+616/+620,
    /// rounding each component before storing the Vec2.
    pub screen_offset: [f32; 2],
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteFeedbackRuntimeState {
    pub timespan: f32,
    /// Native sub_10352800 decay exponent for this StepFrame.
    pub decay_factor: f32,
    pub screen_bounds: [f32; 4],
    /// False on the first frame because the host has no prior framebuffer yet.
    /// The GPU renderer additionally tracks whether its history texture is valid.
    pub active: bool,
}

/// Host-side equivalent of the device virtual invoked by groundCorrection.
/// The original driver delegates the actual collision/ground geometry query to
/// its host, so the portable runtime exposes the same decision boundary.
#[derive(Debug, Clone, PartialEq)]
pub struct EmoteGroundCorrectionRequest {
    pub parent_path: Option<String>,
    pub layer_path: String,
    pub parent_raw_position: [f32; 3],
    pub raw_position: [f32; 3],
    pub previous_delta: [f32; 3],
    pub coordinate: Option<i64>,
}

pub type EmoteGroundCorrectionHook = fn(&EmoteGroundCorrectionRequest) -> Option<[f32; 3]>;

/// Runtime payload returned by native `MMotionPlayer::GetShapeParam`.
///
/// `shape` is the authored discriminator read in sub_1033ED90. The exact
/// geometry is filled by sub_10359D20 from post-MeshChain position (+120),
/// composite scale/matrix (+604/+92), and the active frame offset.
#[derive(Debug, Clone, PartialEq)]
pub enum EmoteShapeRuntimeState {
    Point { center: [f32; 2] },
    Circle { center: [f32; 2], radius: f32 },
    Rect { left: f32, top: f32, right: f32, bottom: f32 },
    Quad { points: [[f32; 2]; 4] },
}

#[derive(Debug, Clone, PartialEq)]
struct ParticleMotionRef {
    object_name: String,
    motion_name: String,
}

#[derive(Debug, Clone, PartialEq)]
struct ParticleStaticConfig {
    particle: i32,
    max_num: usize,
    accel_ratio: f32,
    inherit_angle: bool,
    inherit_opacity: i32,
    inherit_velocity: i32,
    fly_direction: i32,
    apply_zoom_to_velocity: i32,
    delete_outside_screen: bool,
    motion_list: Vec<ParticleMotionRef>,
    tri_volume: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct ParticleInstanceRuntime {
    serial: u64,
    object_name: String,
    motion_name: String,
    age_ticks: f32,
    position: [f32; 3],
    velocity: [f32; 3],
    angle_degrees: f32,
    zoom: f32,
    opacity: f32,
    accel_ratio: f32,
    has_entered_screen: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct ParticleEmitterRuntime {
    initialized: bool,
    countdown_ticks: f32,
    rng_state: u64,
    next_serial: u64,
    last_raw_position: [f32; 3],
    last_transform: [f32; 6],
    last_angle_degrees: f32,
    particles: Vec<ParticleInstanceRuntime>,
}

impl ParticleEmitterRuntime {
    fn new(path: &str, raw_position: [f32; 3], transform: [f32; 6], angle_degrees: f32) -> Self {
        // The original engine uses its manager RNG. Persist a deterministic
        // per-emitter stream so two scene rebuilds at the same StepFrame time
        // are idempotent while preserving native uniform-random semantics.
        let mut seed = 0xcbf2_9ce4_8422_2325u64;
        for byte in path.as_bytes() {
            seed ^= u64::from(*byte);
            seed = seed.wrapping_mul(0x1000_0000_01b3);
        }
        Self {
            initialized: false,
            countdown_ticks: 0.0,
            rng_state: seed.max(1),
            next_serial: 0,
            last_raw_position: raw_position,
            last_transform: transform,
            last_angle_degrees: angle_degrees,
            particles: Vec::new(),
        }
    }
}

/// Native joinTarget state-transfer pool (sub_1039A780/sub_103A20E0).
///
/// Joinable layers are matched by native layer type, in traversal order. The
/// original engine stores these records while replacing an MMotionPlayer and
/// seeds compatible layers in the new player before StepFrame. Keeping the
/// pool private prevents this transition-only state from leaking into the SDK.
#[derive(Debug, Clone, Default)]
struct JoinReusePool {
    frames: BTreeMap<i64, Vec<DynamicFrameState>>,
    frame_cursor: BTreeMap<i64, usize>,
    emitters: BTreeMap<i64, Vec<ParticleEmitterRuntime>>,
    emitter_cursor: BTreeMap<i64, usize>,
    bound_emitters: BTreeMap<String, ParticleEmitterRuntime>,
}

fn native_join_target_type(layer_type: i64) -> bool {
    matches!(layer_type, 0 | 2 | 3 | 4 | 7 | 8 | 11 | 12)
}

impl JoinReusePool {
    fn from_previous(scene: Option<&EmoteStaticScene>) -> Self {
        let mut pool = Self::default();
        let Some(scene) = scene else {
            return pool;
        };
        for layer in &scene.layer_states {
            let info = &layer.draw_frame_info;
            if !info.join_target || !native_join_target_type(info.layer_type) {
                continue;
            }
            if let Some(frame) = layer.specialized_frame.clone() {
                pool.frames.entry(info.layer_type).or_default().push(frame);
            }
            if info.layer_type == 4 {
                if let Some(emitter) = scene.particle_emitters.get(&layer.path).cloned() {
                    pool.emitters.entry(4).or_default().push(emitter);
                }
            }
        }
        pool
    }

    fn take_frame(&mut self, layer_type: i64) -> Option<DynamicFrameState> {
        if !native_join_target_type(layer_type) {
            return None;
        }
        let cursor = self.frame_cursor.entry(layer_type).or_default();
        let result = self.frames.get(&layer_type)?.get(*cursor).cloned();
        if result.is_some() {
            *cursor += 1;
        }
        result
    }

    fn bind_particle_emitter(&mut self, path: &str) {
        let cursor = self.emitter_cursor.entry(4).or_default();
        let emitter = self.emitters.get(&4).and_then(|items| items.get(*cursor)).cloned();
        if let Some(emitter) = emitter {
            *cursor += 1;
            self.bound_emitters.insert(path.to_owned(), emitter);
        }
    }
}


#[derive(Debug, Clone, PartialEq)]
pub struct EmoteStepFrameMeshState {
    pub path: String,
    pub texture_resource_index: u32,
    pub texture_name: String,
    pub uv_rect: [f32; 4],
    pub mesh: Option<EmoteMeshPatch>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct EmoteMeshChainNode {
    pub layer: EmoteStepFrameLayerState,
    pub mesh: Option<EmoteStepFrameMeshState>,
    pub children: Vec<EmoteMeshChainNode>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct EmoteStepFrameOutput {
    pub layers: Vec<EmoteStepFrameLayerState>,
    pub meshes: Vec<EmoteStepFrameMeshState>,
    pub draw_frame_info: Vec<EmoteDrawFrameInfo>,
}

impl EmoteStaticSprite {
    pub fn left(&self) -> f32 {
        self.center_x - self.width * 0.5
    }

    pub fn top(&self) -> f32 {
        self.center_y - self.height * 0.5
    }

    pub fn right(&self) -> f32 {
        self.center_x + self.width * 0.5
    }

    pub fn bottom(&self) -> f32 {
        self.center_y + self.height * 0.5
    }

    pub fn bounds_rect(&self) -> (f32, f32, f32, f32) {
        let (left, top, right, bottom) = if let Some(mesh) = &self.mesh {
            let (min_u, min_v, max_u, max_v) = mesh.control_bounds();
            let left = self.left();
            let top = self.top();
            (
                left + min_u * self.width,
                top + min_v * self.height,
                left + max_u * self.width,
                top + max_v * self.height,
            )
        } else {
            (self.left(), self.top(), self.right(), self.bottom())
        };
        bounds_after_sprite_transform(self, left, top, right, bottom)
    }
}

fn bounds_after_sprite_transform(
    sprite: &EmoteStaticSprite,
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
) -> (f32, f32, f32, f32) {
    let points = [
        transform_emote_sprite_point(sprite, [left, top]),
        transform_emote_sprite_point(sprite, [left, bottom]),
        transform_emote_sprite_point(sprite, [right, top]),
        transform_emote_sprite_point(sprite, [right, bottom]),
    ];
    let mut min_x = points[0][0];
    let mut min_y = points[0][1];
    let mut max_x = points[0][0];
    let mut max_y = points[0][1];
    for p in &points[1..] {
        min_x = min_x.min(p[0]);
        min_y = min_y.min(p[1]);
        max_x = max_x.max(p[0]);
        max_y = max_y.max(p[1]);
    }
    (min_x, min_y, max_x, max_y)
}

fn transform_emote_sprite_point(sprite: &EmoteStaticSprite, point: [f32; 2]) -> [f32; 2] {
    let sx = finite_or(sprite.scale_x, 1.0);
    let sy = finite_or(sprite.scale_y, 1.0);
    let angle = finite_or(sprite.rotation_degrees, 0.0).to_radians();
    let cos = angle.cos();
    let sin = angle.sin();
    let dx = (point[0] - sprite.center_x) * sx;
    let dy = (point[1] - sprite.center_y) * sy;
    let local = [
        sprite.center_x + dx * cos - dy * sin,
        sprite.center_y + dx * sin + dy * cos,
    ];
    transform_from_array(sprite.world_transform).apply(local)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmoteSchemaError {
    RootIsNotObject,
    MissingObjectTable,
    MissingSourceTable,
    MissingBaseObject,
    InvalidSourceTexture { source: String },
    InvalidTextureResource { source: String },
    InvalidIcon { source: String, icon: String },
}

impl fmt::Display for EmoteSchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EmoteSchemaError::RootIsNotObject => write!(f, "PSB root is not an object"),
            EmoteSchemaError::MissingObjectTable => write!(f, "Emote PSB has no object table"),
            EmoteSchemaError::MissingSourceTable => write!(f, "Emote PSB has no source table"),
            EmoteSchemaError::MissingBaseObject => write!(f, "cannot determine base Emote object"),
            EmoteSchemaError::InvalidSourceTexture { source } => {
                write!(f, "invalid source texture entry for {source}")
            }
            EmoteSchemaError::InvalidTextureResource { source } => {
                write!(f, "invalid or missing texture resource for {source}")
            }
            EmoteSchemaError::InvalidIcon { source, icon } => {
                write!(f, "invalid icon {source}/{icon}")
            }
        }
    }
}

impl Error for EmoteSchemaError {}

impl EmoteModelSchema {
    pub fn from_psb(psb: &PsbFile) -> Result<Self, EmoteSchemaError> {
        let root_value = &psb.root;
        root_value
            .as_object()
            .ok_or(EmoteSchemaError::RootIsNotObject)?;
        let object = root_value
            .field("object")
            .ok_or(EmoteSchemaError::MissingObjectTable)?;
        let source = root_value
            .field("source")
            .ok_or(EmoteSchemaError::MissingSourceTable)?;

        let base_object = find_base_object(&root_value, object)?;
        let spec = root_value.field_str("spec").map(str::to_owned);
        let textures = collect_textures(source)?;
        // MEmotePlayer::Init resolves `metadata` first and sub_1026EC30 then
        // reads stereovisionControl from that object.  It is not a root-level
        // field in the native schema.
        let stereovision = root_value
            .field("metadata")
            .and_then(parse_stereovision_control);
        let stereovision_profile = parse_stereovision_profile(&root_value);

        Ok(Self {
            base_object,
            spec,
            textures,
            stereovision,
            stereovision_profile,
        })
    }

    pub fn motion_infos(&self, psb: &PsbFile) -> Result<Vec<EmoteMotionInfo>, EmoteSchemaError> {
        let root_value = &psb.root;
        root_value
            .as_object()
            .ok_or(EmoteSchemaError::RootIsNotObject)?;
        let object_table = root_value
            .field("object")
            .ok_or(EmoteSchemaError::MissingObjectTable)?;
        let base = object_table
            .field(&self.base_object)
            .ok_or(EmoteSchemaError::MissingBaseObject)?;
        let Some(motions) = base.field("motion").and_then(PsbValue::as_object) else {
            return Ok(Vec::new());
        };

        let mut out = Vec::with_capacity(motions.len());
        for (name, motion) in motions {
            out.push(EmoteMotionInfo {
                name: name.clone(),
                duration_ticks: motion_duration_ticks(motion).unwrap_or(0.0),
            });
        }
        Ok(out)
    }

    pub fn default_motion_name(&self, psb: &PsbFile) -> Result<Option<String>, EmoteSchemaError> {
        Ok(self
            .motion_infos(psb)?
            .into_iter()
            .next()
            .map(|info| info.name))
    }

    pub fn build_motion_scene_at(
        &self,
        psb: &PsbFile,
        motion_name: &str,
        time_ticks: f32,
    ) -> Result<EmoteStaticScene, EmoteSchemaError> {
        self.build_motion_scene_internal(psb, None, motion_name, time_ticks, &BTreeMap::new(), None, None, None, None)
    }

    pub fn build_motion_scene_at_with_variables(
        &self,
        psb: &PsbFile,
        motion_name: &str,
        time_ticks: f32,
        variables: &BTreeMap<String, f32>,
    ) -> Result<EmoteStaticScene, EmoteSchemaError> {
        self.build_motion_scene_internal(psb, None, motion_name, time_ticks, variables, None, None, None, None)
    }

    pub fn build_motion_scene_at_with_resources_and_variables(
        &self,
        psb: &PsbFile,
        psb_data: &[u8],
        motion_name: &str,
        time_ticks: f32,
        variables: &BTreeMap<String, f32>,
    ) -> Result<EmoteStaticScene, EmoteSchemaError> {
        self.build_motion_scene_internal(psb, Some(psb_data), motion_name, time_ticks, variables, None, None, None, None)
    }

    /// Builds one motion frame while supplying the previous native StepFrame
    /// positions.  MMotionPlayer copies last frame's finalized XYZ into
    /// layer+108 before evaluating the current frame, then turns it into the
    /// current-minus-previous displacement used by nested-motion direction
    /// mode 2 (sub_10331060 -> sub_10355BF0).
    pub fn build_motion_scene_at_with_resources_variables_and_previous_positions(
        &self,
        psb: &PsbFile,
        psb_data: &[u8],
        motion_name: &str,
        time_ticks: f32,
        variables: &BTreeMap<String, f32>,
        previous_positions: &BTreeMap<String, [f32; 3]>,
    ) -> Result<EmoteStaticScene, EmoteSchemaError> {
        self.build_motion_scene_internal(
            psb,
            Some(psb_data),
            motion_name,
            time_ticks,
            variables,
            Some(previous_positions),
            None,
            None,
            None,
        )
    }

    /// Builds one native StepFrame using the full previous scene snapshot.
    /// Besides prior XYZ (nested-motion direction mode 2), this carries the
    /// persistent per-layer decoded frame state required by serialized type-0
    /// HOLD frames (sub_1032FB00).
    pub fn build_motion_scene_at_with_resources_variables_and_previous_scene(
        &self,
        psb: &PsbFile,
        psb_data: &[u8],
        motion_name: &str,
        time_ticks: f32,
        variables: &BTreeMap<String, f32>,
        previous_scene: &EmoteStaticScene,
    ) -> Result<EmoteStaticScene, EmoteSchemaError> {
        let previous_positions: BTreeMap<String, [f32; 3]> = previous_scene
            .layer_states
            .iter()
            .map(|layer| (layer.path.clone(), layer.raw_position))
            .collect();
        self.build_motion_scene_internal(
            psb,
            Some(psb_data),
            motion_name,
            time_ticks,
            variables,
            Some(&previous_positions),
            Some(&previous_scene.frame_runtime_states),
            Some(previous_scene),
            None,
        )
    }

    pub fn build_motion_scene_at_with_resources_variables_previous_scene_and_ground_hook(
        &self,
        psb: &PsbFile,
        psb_data: &[u8],
        motion_name: &str,
        time_ticks: f32,
        variables: &BTreeMap<String, f32>,
        previous_scene: &EmoteStaticScene,
        ground_correction_hook: Option<EmoteGroundCorrectionHook>,
    ) -> Result<EmoteStaticScene, EmoteSchemaError> {
        let previous_positions: BTreeMap<String, [f32; 3]> = previous_scene
            .layer_states
            .iter()
            .map(|layer| (layer.path.clone(), layer.raw_position))
            .collect();
        self.build_motion_scene_internal(
            psb,
            Some(psb_data),
            motion_name,
            time_ticks,
            variables,
            Some(&previous_positions),
            Some(&previous_scene.frame_runtime_states),
            Some(previous_scene),
            ground_correction_hook,
        )
    }

    fn build_motion_scene_internal(
        &self,
        psb: &PsbFile,
        psb_data: Option<&[u8]>,
        motion_name: &str,
        time_ticks: f32,
        variables: &BTreeMap<String, f32>,
        previous_positions: Option<&BTreeMap<String, [f32; 3]>>,
        previous_frame_states: Option<&BTreeMap<String, DynamicFrameState>>,
        previous_scene: Option<&EmoteStaticScene>,
        ground_correction_hook: Option<EmoteGroundCorrectionHook>,
    ) -> Result<EmoteStaticScene, EmoteSchemaError> {
        let root_value = &psb.root;
        root_value
            .as_object()
            .ok_or(EmoteSchemaError::RootIsNotObject)?;
        let object_table = root_value
            .field("object")
            .ok_or(EmoteSchemaError::MissingObjectTable)?;
        let root_parameter_table = root_value.field("parameter").and_then(PsbValue::as_list);
        let base = object_table
            .field(&self.base_object)
            .ok_or(EmoteSchemaError::MissingBaseObject)?;
        let motion = base
            .field("motion")
            .and_then(|motions| motions.field(motion_name))
            .ok_or(EmoteSchemaError::MissingBaseObject)?;
        let layers = motion
            .field("layer")
            .and_then(PsbValue::as_list)
            .ok_or(EmoteSchemaError::MissingBaseObject)?;

        let effective_time = effective_motion_time(motion, time_ticks);
        let priority_ranks = Arc::new(motion_priority_ranks(motion, effective_time));
        let mut sprites = Vec::new();
        let mut layer_states = Vec::new();
        let mut frame_runtime_states = BTreeMap::<String, DynamicFrameState>::new();
        let mut mask_owners = BTreeMap::<String, Vec<String>>::new();
        let mut pending_nested = Vec::new();
        let mut pending_anchors = Vec::new();
        let mut join_reuse = JoinReusePool::from_previous(previous_scene);
        let mut particle_emitters = if previous_scene
            .map_or(false, |scene| time_ticks + f32::EPSILON >= scene.scene_time_ticks)
        {
            previous_scene
                .map(|scene| scene.particle_emitters.clone())
                .unwrap_or_default()
        } else {
            BTreeMap::new()
        };
        let particle_delta_ticks = previous_scene
            .map(|scene| (time_ticks - scene.scene_time_ticks).max(0.0))
            .unwrap_or(0.0);
        for (index, layer) in layers.iter().enumerate() {
            travel_layer_at(
                layer,
                index,
                object_table,
                motion
                    .field("parameter")
                    .and_then(PsbValue::as_list)
                    .or(root_parameter_table),
                psb,
                psb_data,
                variables,
                &self.textures,
                motion_name,
                effective_time,
                TravelContext {
                    draw_index: index,
                    priority_ranks: priority_ranks.clone(),
                    ..TravelContext::default()
                },
                previous_positions,
                previous_frame_states,
                &mut frame_runtime_states,
                &mut join_reuse,
                &mut pending_nested,
                &mut pending_anchors,
                &mut sprites,
                &mut layer_states,
                &mut mask_owners,
            )?;
        }
        apply_ground_correction_specialized_pass(
            ground_correction_hook,
            previous_positions,
            0,
            layer_states.len(),
            0,
            &mut sprites,
            &mut layer_states,
            &mut pending_nested,
        );
        apply_anchor_specialized_pass(
            &pending_anchors,
            0,
            0,
            &mut sprites,
            &mut layer_states,
            &mut pending_nested,
        );
        // sub_10331060 native specialized order: Anchor -> MeshChain.
        let scope_end = layer_states.len();
        apply_mesh_chain_specialized_pass(&mut layer_states, 0, scope_end);
        apply_ready_to_draw_specialized_pass(&mut layer_states, 0, scope_end, &mut sprites, 0);
        // StepFrameCamera is still executed at the native specialized-pass
        // position. Its output is player-local state, not a mutation of the
        // standard 2-D draw list; collect all root/nested player states after
        // nested/particle child players have been stepped below.
        let _ = apply_camera_specialized_pass(&layer_states, 0, scope_end);
        apply_type7_bounds_specialized_pass(&mut layer_states, 0, scope_end, &mut sprites, 0);
        apply_shape_specialized_pass(&mut layer_states, 0, scope_end);
        let scope_lookup = scope_layer_positions(&layer_states);
        resolve_pending_nested_motions(
            pending_nested,
            &scope_lookup,
            object_table,
            motion
                .field("parameter")
                .and_then(PsbValue::as_list)
                .or(root_parameter_table),
            psb,
            psb_data,
            variables,
            &self.textures,
            previous_positions,
            previous_frame_states,
            ground_correction_hook,
            particle_delta_ticks,
            &mut frame_runtime_states,
            &mut join_reuse,
            &mut sprites,
            &mut layer_states,
            &mut mask_owners,
        )?;
        // Native order places Model after nested MMotionPlayer resolution.
        apply_model_specialized_pass(
            &mut layer_states,
            0,
            scope_end,
            effective_time,
            previous_positions,
        );
        // sub_103A20E0 transfers type-4 runtime ownership across a joinTarget
        // replacement. Preserve an existing same-path emitter first; otherwise
        // bind the compatible previous emitter to the new layer path.
        for (path, emitter) in std::mem::take(&mut join_reuse.bound_emitters) {
            particle_emitters.entry(path).or_insert(emitter);
        }
        // Native specialized order continues with StepFrameParticle after
        // Model. Emitter state is copied from the previous scene snapshot so
        // repeated rebuilds at one host time are idempotent.
        apply_particle_specialized_pass(
            object_table,
            motion
                .field("parameter")
                .and_then(PsbValue::as_list)
                .or(root_parameter_table),
            psb,
            psb_data,
            variables,
            &self.textures,
            particle_delta_ticks,
            previous_positions,
            previous_frame_states,
            ground_correction_hook,
            &mut frame_runtime_states,
            &mut particle_emitters,
            &mut sprites,
            &mut layer_states,
            &mut mask_owners,
        )?;
        // Native specialized order ends with Feedback after Particle.
        let feedback_scope_end = layer_states.len();
        apply_feedback_specialized_pass(
            &mut layer_states,
            &mut sprites,
            0,
            feedback_scope_end,
            particle_delta_ticks,
        );
        // Every nested motion is a distinct MMotionPlayer in the DLL
        // (sub_10355BF0). StepFrameCamera therefore writes independent
        // +0x1d2..+0x1f4 camera state on each child player. Recover those
        // states by exact flattened-player scope before finalize_scene sorts
        // layer_states by path.
        let root_camera_scope = layer_states
            .first()
            .map(|state| state.motion_scope_root_path.clone());
        let camera_runtimes = collect_camera_runtimes(&layer_states);
        let camera_runtime = root_camera_scope
            .as_ref()
            .and_then(|scope| camera_runtimes.get(scope))
            .cloned();
        let mut scene = finalize_scene(
            self.base_object.clone(),
            sprites,
            layer_states,
            frame_runtime_states,
            mask_owners,
        );
        scene.particle_emitters = particle_emitters;
        scene.scene_time_ticks = time_ticks;
        scene.camera_runtime = camera_runtime;
        scene.camera_runtimes = camera_runtimes;
        Ok(scene)
    }

    pub fn build_static_scene(&self, psb: &PsbFile) -> Result<EmoteStaticScene, EmoteSchemaError> {
        let root_value = &psb.root;
        root_value
            .as_object()
            .ok_or(EmoteSchemaError::RootIsNotObject)?;
        let object_table = root_value
            .field("object")
            .ok_or(EmoteSchemaError::MissingObjectTable)?;
        let base = object_table
            .field(&self.base_object)
            .ok_or(EmoteSchemaError::MissingBaseObject)?;
        let motion_table = base
            .field("motion")
            .ok_or(EmoteSchemaError::MissingBaseObject)?;

        let mut sprites = Vec::new();
        let mut layer_states = Vec::new();
        let frame_runtime_states = BTreeMap::<String, DynamicFrameState>::new();
        let mut mask_owners = BTreeMap::<String, Vec<String>>::new();
        if let Some(motions) = motion_table.as_object() {
            for (motion_name, motion) in motions {
                if let Some(layers) = motion.field("layer").and_then(PsbValue::as_list) {
                    let ctx = TravelContext {
                        priority_ranks: Arc::new(motion_priority_ranks(motion, 0.0)),
                        ..TravelContext::default()
                    };
                    for (index, layer) in layers.iter().enumerate() {
                        travel_layer(
                            layer,
                            index,
                            object_table,
                            &self.textures,
                            motion_name,
                            TravelContext {
                                draw_index: index,
                                ..ctx.clone()
                            },
                            &mut sprites,
                            &mut layer_states,
                            &mut mask_owners,
                        )?;
                    }
                }
            }
        }

        Ok(finalize_scene(
            self.base_object.clone(),
            sprites,
            layer_states,
            frame_runtime_states,
            mask_owners,
        ))
    }
}


fn native_manager_frame_order(
    a: &EmoteStaticSprite,
    b: &EmoteStaticSprite,
) -> std::cmp::Ordering {
    // sub_10344710 is the comparator passed by sub_10338DA0 to MSVC
    // std::sort(vector<MMotionManager::FrameInfo *>). For a single player
    // tree DFI+8 is identical on every record, so DFI+36 (resolved Z) is the
    // only semantic ordering key represented by EmoteStaticScene. C++ float
    // comparisons make NaN comparator-equivalent; `partial_cmp(None) => Equal`
    // mirrors that behavior. The recursive emission key is deterministic only
    // after the native comparator has declared the records equivalent.
    a.z.partial_cmp(&b.z)
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| {
            a.draw_frame_info
                .native_draw_key
                .cmp(&b.draw_frame_info.native_draw_key)
        })
        .then_with(|| a.draw_frame_info.path.cmp(&b.draw_frame_info.path))
}

fn finalize_scene(
    base_object: String,
    mut sprites: Vec<EmoteStaticSprite>,
    mut layer_states: Vec<EmoteStepFrameLayerState>,
    frame_runtime_states: BTreeMap<String, DynamicFrameState>,
    mask_owners_raw: BTreeMap<String, Vec<String>>,
) -> EmoteStaticScene {
    // Native manager order is two-stage. sub_103390C0 first emits each
    // MMotionPlayer's DrawFrameInfo pointers in priorityFrameList order
    // (including recursive child-player emission), but sub_10338DA0 then runs
    // std::sort over the manager's `vector<MMotionManager::FrameInfo *>`. Its
    // comparator sub_10344710 compares DrawFrameInfo +36 first and +8 second,
    // both as exact floats, ascending. +36 is the layer's resolved Z
    // (`LayerInfo +620`). +8 is a per-MMotionPlayer value which is propagated
    // unchanged to nested players (sub_1034F2E0 / sub_1033ED90 /
    // sub_10357200); a single Eluna scene has one such player group, so it is
    // equal for every sprite here.
    //
    // Keep `native_draw_key` only as a deterministic tie-break for comparator-
    // equivalent entries. It must never outrank Z: Stage 11-13 did that and
    // consequently inverted broad visual planes such as rear-hair/face and
    // leg/skirt. Unlike the old Stage-10 compatibility sort, do not round Z.
    sprites.sort_by(native_manager_frame_order);
    for (index, sprite) in sprites.iter_mut().enumerate() {
        sprite.draw_frame_info.draw_index = index;
    }
    let (
        composite_mask_owners,
        composite_mask_sources_by_key,
        composite_referenced_type3,
    ) = resolve_composite_mask_owners(&sprites, &layer_states, &mask_owners_raw);
    mark_native_draw_frame_submissions(
        &mut layer_states,
        &mut sprites,
        &composite_referenced_type3,
    );
    // Keep the public/debug target list aligned with the exact native source
    // expansion used by the renderer.  The old fuzzy label/suffix resolver is
    // deliberately not used: sub_103415A0 performs an exact player-local
    // layerIndexMap lookup.
    for sprite in &mut sprites {
        sprite.draw_frame_info.stencil_composite_target_paths = composite_mask_owners
            .get(&sprite.draw_frame_info.path)
            .cloned()
            .unwrap_or_default();
    }
    let bounds = compute_bounds(&sprites);
    let draw_frame_info = sprites
        .iter()
        .map(|sprite| sprite.draw_frame_info.clone())
        .collect();
    layer_states.sort_by(|a, b| a.path.cmp(&b.path));
    EmoteStaticScene {
        base_object,
        sprites,
        bounds,
        draw_frame_info,
        layer_states,
        frame_runtime_states,
        composite_mask_owners,
        composite_mask_sources_by_key,
        particle_emitters: BTreeMap::new(),
        scene_time_ticks: 0.0,
        camera_runtime: None,
        camera_runtimes: BTreeMap::new(),
    }
}

/// Native DrawFrameInfo allocation/type gate recovered from sub_10332D00.
/// Composite-mask list parsing itself belongs only to type 12 in sub_1033ED90;
/// type 3 is a proxy DFI used when a referenced nested MMotionPlayer is expanded
/// into a type-12 owner's DFI+124 source vector.
fn native_layer_has_draw_frame_info(layer_type: i64) -> bool {
    // sub_10332D00 allocates LayerInfo+732 only for these four types.  A
    // stencil-ready helper layer of any other type can still become +724, but
    // sub_103390C0 then writes ancestor->+732 (NULL) to DrawFrameInfo+120.
    matches!(layer_type, 0 | 3 | 10 | 12)
}

fn native_player_prefix(key: &[u64]) -> &[u64] {
    key.split_last().map(|(_, prefix)| prefix).unwrap_or(&[])
}

fn same_native_player(a: &EmoteDrawFrameInfo, b: &EmoteDrawFrameInfo) -> bool {
    a.native_draw_key.len() == b.native_draw_key.len()
        && native_player_prefix(&a.native_draw_key) == native_player_prefix(&b.native_draw_key)
}

fn resolve_native_composite_layer_reference<'a>(
    reference: &str,
    owner: &EmoteStepFrameLayerState,
    states: &'a [EmoteStepFrameLayerState],
) -> Option<&'a EmoteStepFrameLayerState> {
    // sub_10337560 -> sub_103415A0: lookup is by the exact authored string in
    // this MMotionPlayer's layerIndexMap.  It is not a global suffix search and
    // is not resolved relative to the owner's visual path.  `layer_states` is
    // still in native preorder here, so the first exact entry is the closest
    // portable equivalent if malformed/exporter data contains duplicate map
    // keys (well-formed E-mote archives use a unique player-local key).
    let reference = reference.trim();
    if reference.is_empty() {
        return None;
    }
    states.iter().find(|candidate| {
        same_native_player(&owner.draw_frame_info, &candidate.draw_frame_info)
            && candidate.draw_frame_info.layer_label.as_deref() == Some(reference)
    })
}

/// Rebuilds type-12 `stencilCompositeMaskLayerList` with the same source
/// semantics as sub_10337560 + the second pass of sub_103390C0.
///
/// The runtime resolver accepts only type-0 and type-3 LayerInfos. A type-0
/// source contributes its DrawFrameInfo directly. A type-3 source contributes
/// the recursively emitted DrawFrameInfos stored in its proxy DFI+124; the
/// proxy itself is not ordinary color geometry.
fn resolve_composite_mask_owners(
    sprites: &[EmoteStaticSprite],
    layer_states: &[EmoteStepFrameLayerState],
    raw: &BTreeMap<String, Vec<String>>,
) -> (
    BTreeMap<String, Vec<String>>,
    BTreeMap<Vec<u64>, Vec<Vec<u64>>>,
    BTreeSet<Vec<u64>>,
) {
    let sprite_by_key: BTreeMap<Vec<u64>, &EmoteStaticSprite> = sprites
        .iter()
        .map(|sprite| (sprite.draw_frame_info.native_draw_key.clone(), sprite))
        .collect();
    let state_by_path: BTreeMap<&str, &EmoteStepFrameLayerState> = layer_states
        .iter()
        .map(|state| (state.path.as_str(), state))
        .collect();

    let mut out = BTreeMap::<String, Vec<String>>::new();
    let mut out_by_key = BTreeMap::<Vec<u64>, Vec<Vec<u64>>>::new();
    let mut referenced_type3 = BTreeSet::<Vec<u64>>::new();

    for (owner_path, references) in raw {
        let Some(owner) = state_by_path.get(owner_path.as_str()).copied() else {
            continue;
        };
        // sub_1033ED90 parses stencilCompositeMaskLayerList only in case 0xC;
        // sub_103390C0 second pass additionally requires stencilType bit 2.
        if owner.draw_frame_info.layer_type != 12
            || (owner.draw_frame_info.stencil_type & 4) == 0
        {
            continue;
        }

        let mut resolved = Vec::<String>::new();
        let mut resolved_keys = Vec::<Vec<u64>>::new();
        for reference in references {
            let Some(source) =
                resolve_native_composite_layer_reference(reference, owner, layer_states)
            else {
                continue;
            };
            match source.draw_frame_info.layer_type {
                0 => {
                    // Native second pass tests source LayerInfo+728. For a
                    // type-0 layer this is set only when the ordinary drawable
                    // was active and had geometry (LayerInfo+132). A visible
                    // emitted sprite is the portable equivalent.
                    if sprite_by_key
                        .get(&source.draw_frame_info.native_draw_key)
                        .is_some_and(|sprite| sprite.visible && sprite.opacity > 0.0)
                        && !resolved.contains(&source.path)
                    {
                        resolved.push(source.path.clone());
                        if !resolved_keys.contains(&source.draw_frame_info.native_draw_key) {
                            resolved_keys.push(source.draw_frame_info.native_draw_key.clone());
                        }
                    }
                }
                3 => {
                    // sub_10337560 sets source+716 for referenced type-3
                    // layers. sub_103390C0 therefore allocates/submits its
                    // proxy DFI and recursively fills proxy+124 even when the
                    // type-3 layer is not independently stencil-ready.
                    referenced_type3.insert(source.draw_frame_info.native_draw_key.clone());
                    if !source.visible {
                        continue;
                    }
                    let prefix = &source.draw_frame_info.native_draw_key;
                    // `sprites` has already been sorted by native_draw_key in
                    // finalize_scene. Prefix selection therefore expands the
                    // nested player's DFI+124 in native recursive emission
                    // order, including deeper nested players/particles.
                    for sprite in sprites {
                        let key = &sprite.draw_frame_info.native_draw_key;
                        if key.len() > prefix.len()
                            && key.starts_with(prefix)
                            && sprite.visible
                            && sprite.opacity > 0.0
                            && !resolved.contains(&sprite.draw_frame_info.path)
                        {
                            resolved.push(sprite.draw_frame_info.path.clone());
                            if !resolved_keys.contains(&sprite.draw_frame_info.native_draw_key) {
                                resolved_keys.push(sprite.draw_frame_info.native_draw_key.clone());
                            }
                        }
                    }
                }
                _ => {
                    // sub_10337560 explicitly rejects every other layer type.
                }
            }
        }
        out_by_key.insert(owner.draw_frame_info.native_draw_key.clone(), resolved_keys);
        out.insert(owner_path.clone(), resolved);
    }

    (out, out_by_key, referenced_type3)
}

fn mark_native_draw_frame_submissions(
    layer_states: &mut [EmoteStepFrameLayerState],
    sprites: &mut [EmoteStaticSprite],
    composite_referenced_type3: &BTreeSet<Vec<u64>>,
) {
    let emitted_sprite_keys: BTreeSet<Vec<u64>> = sprites
        .iter()
        .filter(|sprite| sprite.visible && sprite.opacity > 0.0)
        .map(|sprite| sprite.draw_frame_info.native_draw_key.clone())
        .collect();

    let mut submitted_by_key = BTreeMap::<Vec<u64>, bool>::new();
    for state in layer_states.iter_mut() {
        let submitted = match state.draw_frame_info.layer_type {
            0 | 10 | 12 => {
                state.visible && emitted_sprite_keys.contains(&state.draw_frame_info.native_draw_key)
            },
            3 => {
                state.visible
                    && (state.draw_frame_info.ready_to_draw
                        || composite_referenced_type3
                            .contains(&state.draw_frame_info.native_draw_key))
            }
            _ => false,
        };
        state.draw_frame_info.submitted_to_draw_frame = submitted;
        submitted_by_key.insert(state.draw_frame_info.native_draw_key.clone(), submitted);
    }
    for sprite in sprites {
        sprite.draw_frame_info.submitted_to_draw_frame = submitted_by_key
            .get(&sprite.draw_frame_info.native_draw_key)
            .copied()
            .unwrap_or(false);
    }
}

pub fn load_emote_static_scene(
    psb: &PsbFile,
) -> Result<(EmoteModelSchema, EmoteStaticScene), EmoteSchemaError> {
    let schema = EmoteModelSchema::from_psb(psb)?;
    let scene = schema.build_static_scene(psb)?;
    Ok((schema, scene))
}

fn find_base_object(root: &PsbValue, object_table: &PsbValue) -> Result<String, EmoteSchemaError> {
    if let Some(chara) = root
        .field("metadata")
        .and_then(|metadata| metadata.field("base"))
        .and_then(|base| base.field_str("chara"))
        .filter(|s| !s.is_empty())
    {
        return Ok(chara.to_owned());
    }

    if object_table.field("all_parts").is_some() {
        return Ok("all_parts".to_owned());
    }

    let first = object_table
        .as_object()
        .and_then(|fields| fields.first())
        .map(|(key, _)| key.clone())
        .ok_or(EmoteSchemaError::MissingBaseObject)?;
    Ok(first)
}

fn collect_textures(
    source: &PsbValue,
) -> Result<BTreeMap<String, EmoteTextureSource>, EmoteSchemaError> {
    let mut textures = BTreeMap::new();
    let Some(entries) = source.as_object() else {
        return Ok(textures);
    };

    for (name, value) in entries {
        let Some(texture) = value.field("texture") else {
            continue;
        };
        let resource_index = texture
            .field_u32("pixel")
            .or_else(|| texture.field_u32("data"))
            .or_else(|| texture.field_u32("resource"))
            .ok_or_else(|| EmoteSchemaError::InvalidTextureResource {
                source: name.clone(),
            })?;
        let width = number_to_positive_u32(texture.field("width")).ok_or_else(|| {
            EmoteSchemaError::InvalidSourceTexture {
                source: name.clone(),
            }
        })?;
        let height = number_to_positive_u32(texture.field("height")).ok_or_else(|| {
            EmoteSchemaError::InvalidSourceTexture {
                source: name.clone(),
            }
        })?;
        let format = texture.field_str("type").map(str::to_owned);
        let compress = texture.field_str("compress").map(str::to_owned);
        let bit_count = texture.field_u32("bitCount");

        let mut icons = BTreeMap::new();
        if let Some(icon_entries) = value.field("icon").and_then(PsbValue::as_object) {
            for (icon_name, icon_value) in icon_entries {
                let left =
                    icon_value
                        .field_f32("left")
                        .ok_or_else(|| EmoteSchemaError::InvalidIcon {
                            source: name.clone(),
                            icon: icon_name.clone(),
                        })?;
                let top =
                    icon_value
                        .field_f32("top")
                        .ok_or_else(|| EmoteSchemaError::InvalidIcon {
                            source: name.clone(),
                            icon: icon_name.clone(),
                        })?;
                let width =
                    icon_value
                        .field_f32("width")
                        .ok_or_else(|| EmoteSchemaError::InvalidIcon {
                            source: name.clone(),
                            icon: icon_name.clone(),
                        })?;
                let height = icon_value.field_f32("height").ok_or_else(|| {
                    EmoteSchemaError::InvalidIcon {
                        source: name.clone(),
                        icon: icon_name.clone(),
                    }
                })?;
                let resolution = icon_value.field_f32("resolution").unwrap_or(1.0);
                icons.insert(
                    icon_name.clone(),
                    EmoteTextureIcon {
                        texture_name: name.clone(),
                        name: icon_name.clone(),
                        left,
                        top,
                        width,
                        height,
                        origin_x: icon_value.field_f32("originX").unwrap_or(0.0),
                        origin_y: icon_value.field_f32("originY").unwrap_or(0.0),
                        resolution,
                        attr: icon_value.field_u32("attr"),
                    },
                );
            }
        }

        textures.insert(
            name.clone(),
            EmoteTextureSource {
                name: name.clone(),
                resource_index,
                width,
                height,
                format,
                compress,
                bit_count,
                icons,
            },
        );
    }

    Ok(textures)
}

fn number_to_positive_u32(value: Option<&PsbValue>) -> Option<u32> {
    let n = value?.as_i64()?;
    (n > 0 && n <= u32::MAX as i64).then_some(n as u32)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct EmoteTransform2D {
    m11: f32,
    m12: f32,
    m21: f32,
    m22: f32,
    tx: f32,
    ty: f32,
}

impl EmoteTransform2D {
    fn identity() -> Self {
        Self {
            m11: 1.0,
            m12: 0.0,
            m21: 0.0,
            m22: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    fn translation(x: f32, y: f32) -> Self {
        Self {
            tx: x,
            ty: y,
            ..Self::identity()
        }
    }

    fn flip(flip_x: bool, flip_y: bool) -> Self {
        Self {
            m11: if flip_x { -1.0 } else { 1.0 },
            m12: 0.0,
            m21: 0.0,
            m22: if flip_y { -1.0 } else { 1.0 },
            tx: 0.0,
            ty: 0.0,
        }
    }

    fn rotation(rotation_degrees: f32) -> Self {
        let angle = finite_or(rotation_degrees, 0.0).to_radians();
        let cos = angle.cos();
        let sin = angle.sin();
        Self {
            m11: cos,
            m12: -sin,
            m21: sin,
            m22: cos,
            tx: 0.0,
            ty: 0.0,
        }
    }

    fn scale(scale_x: f32, scale_y: f32) -> Self {
        Self {
            m11: finite_or(scale_x, 1.0),
            m12: 0.0,
            m21: 0.0,
            m22: finite_or(scale_y, 1.0),
            tx: 0.0,
            ty: 0.0,
        }
    }

    fn shear(shear_x: f32, shear_y: f32) -> Self {
        Self {
            m11: 1.0,
            m12: finite_or(shear_x, 0.0),
            m21: finite_or(shear_y, 0.0),
            m22: 1.0,
            tx: 0.0,
            ty: 0.0,
        }
    }

    fn then(self, rhs: Self) -> Self {
        Self {
            m11: self.m11 * rhs.m11 + self.m12 * rhs.m21,
            m12: self.m11 * rhs.m12 + self.m12 * rhs.m22,
            m21: self.m21 * rhs.m11 + self.m22 * rhs.m21,
            m22: self.m21 * rhs.m12 + self.m22 * rhs.m22,
            tx: self.m11 * rhs.tx + self.m12 * rhs.ty + self.tx,
            ty: self.m21 * rhs.tx + self.m22 * rhs.ty + self.ty,
        }
    }

    fn apply(self, point: [f32; 2]) -> [f32; 2] {
        [
            self.m11 * point[0] + self.m12 * point[1] + self.tx,
            self.m21 * point[0] + self.m22 * point[1] + self.ty,
        ]
    }

    fn as_array(self) -> [f32; 6] {
        [self.m11, self.m12, self.m21, self.m22, self.tx, self.ty]
    }
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

fn transform_from_array(values: [f32; 6]) -> EmoteTransform2D {
    EmoteTransform2D {
        m11: values[0],
        m12: values[1],
        m21: values[2],
        m22: values[3],
        tx: values[4],
        ty: values[5],
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct FrameLinearState {
    flip_x: bool,
    flip_y: bool,
    rotation_degrees: f32,
    scale_x: f32,
    scale_y: f32,
    shear_x: f32,
    shear_y: f32,
}

impl Default for FrameLinearState {
    fn default() -> Self {
        Self {
            flip_x: false,
            flip_y: false,
            rotation_degrees: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
            shear_x: 0.0,
            shear_y: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct MeshSyncChildState {
    patch: EmoteMeshPatch,
    mask: i64,
    coordinate: Option<i64>,
}

/// Native inheritance source selected by MMotionPlayer::StepFrame.
///
/// A direct parent whose inheritMask contains 0x400000 is transparent for
/// transform inheritance: descendants keep using the first ancestor above the
/// transparent chain.  The DLL stores linear matrix and XYZ origin separately,
/// and the parent's `coordinate` chooses whether the matrix acts on XY or XZ.
#[derive(Debug, Clone, Copy)]
struct InheritSourceState {
    linear: EmoteTransform2D,
    linear_state: FrameLinearState,
    location: [f32; 3],
    coordinate: Option<i64>,
    /// Composite opacity of the ancestor selected by inheritParent.
    /// StepFrame only consumes this when the current layer's inheritMask has
    /// bit 0x400 set.
    opacity: f32,
    mesh_sync: Option<MeshSyncChildState>,
}

impl Default for InheritSourceState {
    fn default() -> Self {
        Self {
            linear: EmoteTransform2D::identity(),
            linear_state: FrameLinearState::default(),
            location: [0.0; 3],
            coordinate: None,
            opacity: 1.0,
            mesh_sync: None,
        }
    }
}

#[derive(Debug, Clone)]
struct TravelContext {
    base_location: Option<[f32; 3]>,
    base_visible: bool,
    opacity_multiplier: f32,
    transform: EmoteTransform2D,
    /// Accumulated native frame channels for the current layer.
    linear_state: FrameLinearState,
    /// Ancestor selected by the native inheritParent (0x400000) walk.
    /// This is intentionally separate from `transform`: enter_layer_context
    /// overwrites current-layer metadata but must not destroy the ancestor that
    /// StepFrame uses to evaluate this layer.
    inherit_source: InheritSourceState,
    /// Composite state of layer 0 for the current MMotionPlayer.
    ///
    /// sub_10331060 treats this separately from the selected inheritance
    /// ancestor.  For partial inherit masks the native player temporarily
    /// removes these channels, builds the local matrix, then multiplies this
    /// root matrix back in unless motionIndependentLayerInherit is enabled.
    motion_root: InheritSourceState,
    motion_independent_layer_inherit: bool,
    path: String,
    motion_scope_root_path: String,
    /// Label/path local to the current MMotionPlayer, retained for authored
    /// name lookup and diagnostics. It is NOT used as native priority identity.
    scope_local_path: String,
    /// Structural sibling-index path within the current MMotionPlayer
    /// (for example `0/2/1`). Native priority.content indexes flattened
    /// LayerInfo structurally, so this must remain independent of labels.
    scope_index_path: String,
    /// Prefix contributed by parent MMotionPlayers. The current layer's
    /// priority emission rank is appended to form `native_draw_key`.
    native_draw_prefix: Vec<u64>,
    native_draw_key: Vec<u64>,
    draw_index: usize,
    // All layers in one motion read the same priority table. Context clones
    // must not copy the entire table for every node in the layer tree.
    priority_ranks: Arc<BTreeMap<String, usize>>,
    layer_type: i64,
    mesh_transform: i64,
    mesh_combine: bool,
    mesh_sync_child: i64,
    join_target: bool,
    inherit_mask: Option<i64>,
    transform_order: Vec<i64>,
    coordinate: Option<i64>,
    ground_correction: bool,
    obj_tri_priority: i64,
    stencil_type: i64,
    stencil_wipe_enabled: bool,
    stencil_wipe_reverse: bool,
    stencil_wipe_scale: f32,
    stencil_wipe_bias: f32,
    ready_to_draw: bool,
    stencil_composite_mask_layer_list: Vec<String>,
    parent_mask_path: Option<String>,
    control_parameter: Option<String>,
    control_value: Option<f32>,
    local_time_ticks: Option<f32>,
    frame_index: Option<usize>,
    next_frame_index: Option<usize>,
    frame_offset: [f32; 2],
    interpolation_t: f32,
    mesh_division_x: u32,
    mesh_division_y: u32,
    mesh_patch: Option<EmoteMeshPatch>,
    /// Active ancestor mesh-transform patches used by StepFrameMeshChain. The
    /// current layer's own mesh transform is appended only for descendants.
    mesh_chain: Arc<Vec<MeshChainEntry>>,
    /// Start of the native meshCombine-collapse suffix in `mesh_chain` for the
    /// next child. StepFrameMeshChain walks the real parent chain upward and,
    /// when the child is an active `meshCombine` node, folds active meshes
    /// through the first ancestor whose meshCombine flag is false (inclusive
    /// when that ancestor itself is active). Keeping the suffix boundary lets
    /// the flattened traversal reproduce layerInfo+706/+708 without retaining
    /// raw native pointers.
    mesh_combine_candidate_start: usize,
    mesh_parameters: Arc<BTreeSet<String>>,
}

#[derive(Debug, Clone)]
struct PendingNestedMotion {
    layer: PsbValue,
    object_name: String,
    motion_name: String,
    parent_local_time: f32,
    state: DynamicFrameState,
    ctx: TravelContext,
}

#[derive(Debug, Clone)]
struct PendingAnchor {
    path: String,
    mode: i32,
    target: String,
    flip_x: bool,
    flip_y: bool,
}

#[derive(Debug, Clone)]
struct ScopeLayerPosition {
    label: Option<String>,
    path: String,
    position: [f32; 3],
}

impl Default for TravelContext {
    fn default() -> Self {
        Self {
            base_location: None,
            base_visible: true,
            opacity_multiplier: 1.0,
            transform: EmoteTransform2D::identity(),
            linear_state: FrameLinearState::default(),
            inherit_source: InheritSourceState::default(),
            motion_root: InheritSourceState::default(),
            motion_independent_layer_inherit: false,
            path: String::new(),
            motion_scope_root_path: String::new(),
            scope_local_path: String::new(),
            scope_index_path: String::new(),
            native_draw_prefix: Vec::new(),
            native_draw_key: Vec::new(),
            draw_index: 0,
            priority_ranks: Arc::default(),
            layer_type: 0,
            mesh_transform: 0,
            mesh_combine: false,
            mesh_sync_child: 0,
            join_target: false,
            inherit_mask: None,
            transform_order: Vec::new(),
            coordinate: None,
            ground_correction: false,
            obj_tri_priority: 0,
            stencil_type: 0,
            stencil_wipe_enabled: false,
            stencil_wipe_reverse: false,
            stencil_wipe_scale: 0.0,
            stencil_wipe_bias: 0.0,
            ready_to_draw: true,
            stencil_composite_mask_layer_list: Vec::new(),
            parent_mask_path: None,
            control_parameter: None,
            control_value: None,
            local_time_ticks: None,
            frame_index: None,
            next_frame_index: None,
            frame_offset: [0.0, 0.0],
            interpolation_t: 0.0,
            mesh_division_x: 1,
            mesh_division_y: 1,
            mesh_patch: None,
            mesh_chain: Arc::default(),
            mesh_combine_candidate_start: 0,
            mesh_parameters: Arc::default(),
        }
    }
}

fn enter_layer_context(
    mut ctx: TravelContext,
    layer: &PsbValue,
    sibling_index: usize,
) -> TravelContext {
    let label = layer
        .field_str("label")
        .map(str::to_owned)
        .unwrap_or_else(|| sibling_index.to_string());
    ctx.path = if ctx.path.is_empty() {
        label.clone()
    } else {
        format!("{}/{}", ctx.path, label)
    };
    ctx.scope_local_path = if ctx.scope_local_path.is_empty() {
        label.clone()
    } else {
        format!("{}/{}", ctx.scope_local_path, label)
    };
    // sub_10333180 assigns LayerInfo indices from structural preorder. Labels
    // are metadata only and may repeat, so priority identity must use sibling
    // indices rather than the authored label path.
    ctx.scope_index_path = if ctx.scope_index_path.is_empty() {
        sibling_index.to_string()
    } else {
        format!("{}/{}", ctx.scope_index_path, sibling_index)
    };
    if ctx.motion_scope_root_path.is_empty() {
        ctx.motion_scope_root_path = ctx.path.clone();
    }
    // Native sub_103390C0 does not rank layers by label. It indexes the
    // current priorityFrameList value directly into the preorder-flattened
    // LayerInfo array (`layer = LayerInfo[priority[i] + 1]`) and walks that
    // priority list backwards. `priority_ranks` is therefore keyed by the
    // player-local structural sibling-index path corresponding to that slot;
    // labels are deliberately excluded because they are not unique identities.
    let order_component = ctx
        .priority_ranks
        .get(&ctx.scope_index_path)
        .copied()
        .unwrap_or(sibling_index);
    ctx.draw_index = order_component;
    ctx.native_draw_key = ctx.native_draw_prefix.clone();
    ctx.native_draw_key.push(order_component as u64);
    // Layer-local fields: these are intrinsic to the layer currently entered
    // and must NOT inherit from the parent's traversal context.  The original
    // engine reads them from `layerInfo + offset` directly per layer (see
    // sub_1033ED90 field offsets and sub_103390C0 / sub_10353CF0 consumers).
    // Resetting here is structurally important for stencilType and
    // stencilCompositeMaskLayerList in particular: sub_103390C0 second pass
    // (lines 407-528) reads `*(v91 + 720) & 4` and `*(v91 + 740) + 8` on the
    // OWNER, never from an ancestor context.  Inheriting these fields makes
    // a descendant accidentally take on its ancestor's mask-owner role.
    ctx.layer_type = layer.field_i64("type").unwrap_or(0);
    ctx.mesh_transform = layer.field_i64("meshTransform").unwrap_or(0);
    ctx.mesh_combine = layer.field_i64("meshCombine").unwrap_or(0) != 0;
    ctx.mesh_sync_child = layer.field_i64("meshSyncChildMask").unwrap_or(0);
    ctx.join_target = layer.field_i64("joinTarget").unwrap_or(0) != 0;
    ctx.inherit_mask = layer.field_i64("inheritMask");
    // sub_10331060: opacity inheritance is independent from the linear
    // channels.  A layer multiplies the selected inheritance source only when
    // bit 0x400 is set; otherwise its local opacity starts from the player/root
    // opacity (1.0 in this scene-local representation).
    ctx.opacity_multiplier = if (ctx.inherit_mask.unwrap_or(0) & 0x400) != 0 {
        ctx.inherit_source.opacity
    } else if !ctx.motion_independent_layer_inherit {
        ctx.motion_root.opacity
    } else {
        1.0
    };
    ctx.coordinate = layer.field_i64("coordinate");
    ctx.ground_correction = layer.field_i64("groundCorrection").unwrap_or(0) != 0;
    // sub_1033ED90 allocates this field only for type-0 drawable layers.
    // sub_103390C0 copies it into DrawFrameInfo +84. The standard 2-D draw
    // loop does not consume it, but preserve the native payload for model/
    // alternate backends instead of silently dropping it.
    ctx.obj_tri_priority = if ctx.layer_type == 0 {
        layer.field_i64("objTriPriority").unwrap_or(0)
    } else {
        0
    };
    ctx.stencil_type = layer.field_i64("stencilType").unwrap_or(0);
    ctx.transform_order = layer
        .field("transformOrder")
        .and_then(PsbValue::as_list)
        .map(|values| values.iter().filter_map(PsbValue::as_i64).collect())
        .unwrap_or_default();
    ctx.stencil_composite_mask_layer_list = Vec::new();
    // sub_1033ED90 parses stencilCompositeMaskLayerList only in the type-12
    // case and stores the resolved/runtime vector in that layer's +740 extra.
    if ctx.layer_type == 12 {
        if let Some(values) = layer
            .field("stencilCompositeMaskLayerList")
            .and_then(PsbValue::as_list)
        {
            ctx.stencil_composite_mask_layer_list = values
                .iter()
                .filter_map(PsbValue::as_str)
                .map(str::to_owned)
                .collect();
        }
    }
    if let Some((dx, dy)) = parse_mesh_division(layer.field("meshDivision")) {
        ctx.mesh_division_x = dx;
        ctx.mesh_division_y = dy;
    }
    ctx
}

fn parse_particle_motion_list(layer: &PsbValue) -> Vec<ParticleMotionRef> {
    let Some(items) = layer.field("particleMotionList").and_then(PsbValue::as_list) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            if let Some(pair) = item.as_list() {
                let object_name = pair.first().and_then(PsbValue::as_str)?.to_owned();
                let motion_name = pair.get(1).and_then(PsbValue::as_str)?.to_owned();
                if object_name.is_empty() || motion_name.is_empty() {
                    return None;
                }
                return Some(ParticleMotionRef {
                    object_name,
                    motion_name,
                });
            }
            let object_name = item
                .field_str("object")
                .or_else(|| item.field_str("src"))
                .or_else(|| item.field_str("0"))?;
            let motion_name = item
                .field_str("motion")
                .or_else(|| item.field_str("name"))
                .or_else(|| item.field_str("1"))?;
            (!object_name.is_empty() && !motion_name.is_empty()).then(|| ParticleMotionRef {
                object_name: object_name.to_owned(),
                motion_name: motion_name.to_owned(),
            })
        })
        .collect()
}

fn parse_stereovision_profile(root: &PsbValue) -> Option<EmoteStereovisionProfile> {
    // MMotionManager::ExtractStereovisionProfileFromArchive at 0x1033cd70
    // resolves root["stereovisionProfile"] and copies six consecutive f32
    // fields into the host output structure in this exact order.
    let profile = root.field("stereovisionProfile")?;
    Some(EmoteStereovisionProfile {
        fov: profile.field_f32("fov")?,
        f_level: profile.field_f32("f_level")?,
        len_disp: profile.field_f32("len_disp")?,
        dist_e2d: profile.field_f32("dist_e2d")?,
        dist_eye: profile.field_f32("dist_eye")?,
        eye_angle_ltd: profile.field_f32("eye_angle_ltd")?,
    })
}

fn parse_stereovision_control(root: &PsbValue) -> Option<EmoteStereovisionControl> {
    let control = root.field("stereovisionControl")?;
    let variable_match_list = control
        .field("variableMatchList")
        .and_then(PsbValue::as_list)
        .map(|items| {
            items
                .iter()
                .filter_map(PsbValue::as_str)
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Some(EmoteStereovisionControl { variable_match_list })
}

fn parse_particle_static_config(layer: &PsbValue) -> Option<ParticleStaticConfig> {
    (layer.field_i64("type").unwrap_or(0) == 4).then(|| ParticleStaticConfig {
        particle: layer.field_i64("particle").unwrap_or(0) as i32,
        max_num: layer.field_i64("particleMaxNum").unwrap_or(0).max(0) as usize,
        accel_ratio: finite_or(layer.field_f32("particleAccelRatio").unwrap_or(1.0), 1.0),
        inherit_angle: layer.field_i64("particleInheritAngle").unwrap_or(0) != 0,
        inherit_opacity: layer.field_i64("particleInheritOpacity").unwrap_or(1) as i32,
        inherit_velocity: layer.field_i64("particleInheritVelocity").unwrap_or(0) as i32,
        fly_direction: layer.field_i64("particleFlyDirection").unwrap_or(0) as i32,
        apply_zoom_to_velocity: layer.field_i64("particleApplyZoomToVelocity").unwrap_or(0) as i32,
        delete_outside_screen: layer.field_i64("particleDeleteOutsideScreen").unwrap_or(0) != 0,
        motion_list: parse_particle_motion_list(layer),
        tri_volume: layer.field_i64("particleTriVolume").unwrap_or(0) != 0,
    })
}

fn parse_screen_bounds(layer: &PsbValue) -> Option<[f32; 4]> {
    // sub_1033ED90 case 0xA (Feedback) owns the authored screenBounds
    // rectangle. There is no type-11 parser branch in this driver.
    if layer.field_i64("type").unwrap_or(0) != 10 {
        return None;
    }
    let bounds = layer.field("screenBounds")?;
    Some([
        bounds.field_f32("left").unwrap_or(0.0),
        bounds.field_f32("top").unwrap_or(0.0),
        bounds.field_f32("right").unwrap_or(0.0),
        bounds.field_f32("bottom").unwrap_or(0.0),
    ])
}

fn draw_frame_info(label: Option<String>, ctx: &TravelContext) -> EmoteDrawFrameInfo {
    let mesh_sync_child_mask = ctx.mesh_sync_child;
    let inherit_mask = ctx.inherit_mask.unwrap_or(0);
    let stencil_phase = ctx.stencil_type & 0x3;
    let stencil_composite_item =
        (ctx.stencil_type & 0x4) != 0 || !ctx.stencil_composite_mask_layer_list.is_empty();
    let mask_layer = is_drawframe_mask_context(&ctx);
    let pass = if mask_layer && stencil_composite_item {
        EmoteDrawPass::StencilCompositeMask
    } else if mask_layer {
        EmoteDrawPass::MaskGeneration
    } else if ctx.parent_mask_path.is_some() {
        EmoteDrawPass::Filtered
    } else {
        EmoteDrawPass::Normal
    };
    EmoteDrawFrameInfo {
        layer_label: label,
        native_draw_key: ctx.native_draw_key.clone(),
        draw_index: ctx.draw_index,
        path: ctx.path.clone(),
        layer_type: ctx.layer_type,
        ready_to_draw: ctx.ready_to_draw,
        submitted_to_draw_frame: ctx.ready_to_draw,
        mesh_transform: ctx.mesh_transform,
        mesh_combine: ctx.mesh_combine,
        mesh_sync_child_mask,
        mesh_sync_child_coord: (mesh_sync_child_mask & 1) != 0,
        mesh_sync_child_angle: (mesh_sync_child_mask & 2) != 0,
        mesh_sync_child_zoom: (mesh_sync_child_mask & 4) != 0,
        mesh_sync_child_shape: (mesh_sync_child_mask & 8) != 0,
        join_target: ctx.join_target,
        inherit_mask: ctx.inherit_mask,
        inherit_parent: (inherit_mask & (1 << 22)) != 0,
        inherit_opacity: (inherit_mask & (1 << 10)) != 0,
        inherit_shape: (inherit_mask & (1 << 25)) != 0,
        inherit_angle: (inherit_mask & (1 << 4)) != 0,
        transform_order: ctx.transform_order.clone(),
        coordinate: ctx.coordinate,
        ground_correction: ctx.ground_correction,
        obj_tri_priority: ctx.obj_tri_priority,
        clip_rect: None,
        stencil_type: ctx.stencil_type,
        stencil_phase,
        stencil_composite_item,
        stencil_wipe_enabled: ctx.stencil_wipe_enabled,
        stencil_wipe_reverse: ctx.stencil_wipe_reverse,
        stencil_wipe_scale: ctx.stencil_wipe_scale,
        stencil_wipe_bias: ctx.stencil_wipe_bias,
        stencil_composite_mask_layer_list: ctx.stencil_composite_mask_layer_list.clone(),
        stencil_composite_target_paths: Vec::new(),
        parent_mask_path: ctx.parent_mask_path.clone(),
        stencil_parent_path: None,
        stencil_parent_native_key: None,
        control_parameter: ctx.control_parameter.clone(),
        control_value: ctx.control_value,
        local_time_ticks: ctx.local_time_ticks,
        frame_index: ctx.frame_index,
        next_frame_index: ctx.next_frame_index,
        interpolation_t: ctx.interpolation_t,
        pass,
    }
}

fn layer_state_from_ctx(label: Option<String>, ctx: &TravelContext) -> EmoteStepFrameLayerState {
    let info = draw_frame_info(label, ctx);
    let raw_position = ctx.base_location.unwrap_or([0.0; 3]);
    EmoteStepFrameLayerState {
        path: ctx.path.clone(),
        scope_index_path: ctx.scope_index_path.clone(),
        motion_scope_root_path: ctx.motion_scope_root_path.clone(),
        raw_position,
        position: raw_position,
        mesh_chain: ctx.mesh_chain.as_ref().clone(),
        frame_offset: ctx.frame_offset,
        specialized_frame: None,
        transform: ctx.transform.as_array(),
        opacity: ctx.opacity_multiplier,
        visible: ctx.base_visible && ctx.opacity_multiplier > 0.0,
        model_runtime: None,
        feedback_runtime: None,
        shape_runtime: None,
        linear_state: ctx.linear_state,
        shape_kind: 0,
        particle_static: None,
        particle_triggered: false,
        screen_bounds: None,
        draw_frame_info: info,
    }
}

fn travel_layer(
    value: &PsbValue,
    sibling_index: usize,
    object_table: &PsbValue,
    textures: &BTreeMap<String, EmoteTextureSource>,
    motion_name: &str,
    mut ctx: TravelContext,
    out: &mut Vec<EmoteStaticSprite>,
    layer_states: &mut Vec<EmoteStepFrameLayerState>,
    mask_owners: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), EmoteSchemaError> {
    let Some(_) = value.as_object() else {
        return Ok(());
    };
    let layer = value;

    ctx = enter_layer_context(ctx, &layer, sibling_index);
    let label = layer.field_str("label").map(str::to_owned);
    let current_stencil_phase = ctx.stencil_type & 0x3;
    let current_composite_item =
        (ctx.stencil_type & 0x4) != 0 || !ctx.stencil_composite_mask_layer_list.is_empty();
    // sub_103390C0 second pass treats a layer as a composite-mask owner when
    // (layerInfo+720) & 4 is set.  Record this layer's path and source list so
    // descendants whose `parent_mask_path` matches can resolve the
    // corresponding mask reference at draw-stream construction time.
    if ctx.layer_type == 12
        && (ctx.stencil_type & 0x4) != 0
        && !ctx.stencil_composite_mask_layer_list.is_empty()
    {
        mask_owners
            .entry(ctx.path.clone())
            .or_insert_with(|| ctx.stencil_composite_mask_layer_list.clone());
    }

    if let Some(frame_list) = layer.field("frameList").and_then(PsbValue::as_list) {
        let initial_state = evaluate_frame_list(frame_list, 0.0, None, ctx.coordinate.unwrap_or(0), None);
        let local_ox = initial_state.ox;
        let local_oy = initial_state.oy;
        ctx = apply_layer_transform(
            ctx,
            initial_state.coord,
            initial_state.flip_x,
            initial_state.flip_y,
            initial_state.scale_x,
            initial_state.scale_y,
            initial_state.rotation_degrees,
            initial_state.shear_x,
            initial_state.shear_y,
        );
        prepare_child_inherit_source(&mut ctx, None);

        for frame in frame_list {
            let Some(_) = frame.as_object() else {
                continue;
            };
            let frame_value = frame;
            let Some(content) = frame_value.field("content").filter(|v| v.as_object().is_some())
            else {
                continue;
            };
            let Some(src) = content.field_str("src").filter(|s| !s.is_empty()) else {
                continue;
            };

            let opa_raw = content.field_f32("opa").unwrap_or(255.0);
            let time = frame_value.field_i64("time").unwrap_or(0);
            let visible = ctx.base_visible && time <= 0 && opa_raw > 0.0;
            let suggest_visible = ctx.base_visible && time <= 0 && opa_raw > 0.0;

            if native_nested_motion_layer_type(ctx.layer_type) {
                if let Some(rest) = src.strip_prefix("motion/") {
                    if ctx.base_location.is_some() {
                        let mut parts = rest.split('/').filter(|s| !s.is_empty());
                        if let (Some(object_name), Some(child_motion_name)) =
                            (parts.next(), parts.next())
                        {
                            recurse_motion(
                                object_table,
                                object_name,
                                child_motion_name,
                                textures,
                                ctx_with_visible(
                                    mask_child_context(
                                        ctx.clone(),
                                        current_stencil_phase,
                                        current_composite_item,
                                    ),
                                    suggest_visible,
                                ),
                                out,
                                layer_states,
                                mask_owners,
                            )?;
                        }
                    }
                    continue;
                }
            }

            let icon_name = content.field_str("icon");
            if let Some(icon_name) = icon_name {
                if native_color_drawable_layer_type(ctx.layer_type) && textures.contains_key(src) {
                    if let Some(base) = ctx.base_location {
                        if let Some(sprite) = build_sprite(
                            textures,
                            src,
                            icon_name,
                            label.clone(),
                            motion_name,
                            base,
                            local_ox,
                            local_oy,
                            1.0,
                            1.0,
                            0.0,
                            visible,
                            opa_raw,
                            content.field_u32("bm").unwrap_or(0x10),
                            content.field_f32("bp").unwrap_or(0.0),
                            frame_content_colors(&content),
                            ctx.clone(),
                        ) {
                            out.push(sprite);
                        }
                    }
                } else if native_nested_motion_layer_type(ctx.layer_type)
                    && object_table.field(src).is_some()
                {
                    recurse_motion(
                        object_table,
                        src,
                        icon_name,
                        textures,
                        ctx_with_visible(
                            mask_child_context(
                                ctx.clone(),
                                current_stencil_phase,
                                current_composite_item,
                            ),
                            suggest_visible,
                        ),
                        out,
                        layer_states,
                        mask_owners,
                    )?;
                }
            }
        }
    }

    if layer.field("frameList").and_then(PsbValue::as_list).is_none() {
        prepare_child_inherit_source(&mut ctx, None);
    }
    layer_states.push(layer_state_from_ctx(label, &ctx));

    if let Some(children) = layer.field("children").and_then(PsbValue::as_list) {
        for (index, child) in children.iter().enumerate() {
            let mut next_ctx =
                mask_child_context(ctx.clone(), current_stencil_phase, current_composite_item);
            next_ctx.draw_index = out.len() + index;
            travel_layer(
                child,
                index,
                object_table,
                textures,
                motion_name,
                next_ctx,
                out,
                layer_states,
                mask_owners,
            )?;
        }
    }
    if let Some(children) = layer.field("layer").and_then(PsbValue::as_list) {
        for (index, child) in children.iter().enumerate() {
            let mut next_ctx =
                mask_child_context(ctx.clone(), current_stencil_phase, current_composite_item);
            next_ctx.draw_index = out.len() + index;
            travel_layer(
                child,
                index,
                object_table,
                textures,
                motion_name,
                next_ctx,
                out,
                layer_states,
                mask_owners,
            )?;
        }
    }

    Ok(())
}

fn first_frame_content_with(frame_list: &[PsbValue], key: &str) -> Option<PsbValue> {
    for frame in frame_list {
        let Some(content) = frame.field("content") else {
            continue;
        };
        if content.field(key).is_some() {
            return Some(content.clone());
        }
    }
    None
}

fn recurse_motion(
    object_table: &PsbValue,
    object_name: &str,
    motion_name: &str,
    textures: &BTreeMap<String, EmoteTextureSource>,
    mut ctx: TravelContext,
    out: &mut Vec<EmoteStaticSprite>,
    layer_states: &mut Vec<EmoteStepFrameLayerState>,
    mask_owners: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), EmoteSchemaError> {
    let Some(layers) = object_table
        .field(object_name)
        .and_then(|object| object.field("motion"))
        .and_then(|motion| motion.field(motion_name))
        .and_then(|motion| motion.field("layer"))
        .and_then(PsbValue::as_list)
    else {
        return Ok(());
    };
    let motion = object_table
        .field(object_name)
        .and_then(|object| object.field("motion"))
        .and_then(|motion| motion.field(motion_name));
    let priority_ranks = motion
        .map(|m| Arc::new(motion_priority_ranks(m, 0.0)))
        .unwrap_or_default();
    // sub_103390C0 recursively emits a nested MMotionPlayer exactly at the
    // parent type-3/type-4 priority slot. Preserve that slot as a lexicographic
    // prefix; the child player's own reversed priority rank becomes the next
    // component.
    ctx.native_draw_prefix = ctx.native_draw_key.clone();
    ctx.scope_local_path.clear();
    ctx.scope_index_path.clear();
    ctx.motion_scope_root_path.clear();

    for (index, layer) in layers.iter().enumerate() {
        let mut next_ctx = ctx.clone();
        next_ctx.priority_ranks = priority_ranks.clone();
        travel_layer(
            layer,
            index,
            object_table,
            textures,
            motion_name,
            next_ctx,
            out,
            layer_states,
            mask_owners,
        )?;
    }
    Ok(())
}

fn ctx_with_visible(mut ctx: TravelContext, visible: bool) -> TravelContext {
    ctx.base_visible = visible;
    ctx
}

fn mask_child_context(
    mut ctx: TravelContext,
    _stencil_phase: i64,
    _composite_item: bool,
) -> TravelContext {
    // Descendants of a composite-mask owner (stencilType & 4 layer with a
    // non-empty stencilCompositeMaskLayerList) carry that owner's path so the
    // renderer can resolve a per-reference alpha mask texture for them.  The
    // condition mirrors `sub_103390C0` second-pass owner check at
    // `(layerInfo+720) & 4`.  Stencil-phase masks (bits 0..1 of +720) are a
    // separate flow that we do not implement with alpha textures, so they do
    // not influence parent_mask_path here.
    let is_composite_mask_owner =
        (ctx.stencil_type & 0x4) != 0 && !ctx.stencil_composite_mask_layer_list.is_empty();
    if is_composite_mask_owner {
        ctx.parent_mask_path = Some(ctx.path.clone());
    }
    ctx
}

fn native_color_drawable_layer_type(layer_type: i64) -> bool {
    // sub_10347C10: ordinary DrawFrameInfo materialization is gated by
    // ((1 << layerInfo.type) & 0x1401) != 0, i.e. types 0, 10 and 12.
    // Type 3 is the nested-MMotionPlayer recursion path and type 4 is the
    // particle-player path; specialized/helper layers are never submitted as
    // ordinary textured color geometry merely because their frame content
    // happens to carry src/icon fields.
    matches!(layer_type, 0 | 10 | 12)
}

fn native_nested_motion_layer_type(layer_type: i64) -> bool {
    // sub_103390C0 has a dedicated recursion branch only for type 3.
    layer_type == 3
}

fn is_drawframe_mask_context(_ctx: &TravelContext) -> bool {
    // There is no native "mask-only layer type" in sub_103390C0. Types 0/10/12
    // materialize ordinary DrawFrameInfo records; sub_101D0DD0 later re-renders
    // selected existing DrawFrameInfo geometry while building a stencil mask.
    // Type 3 is a nested-MMotionPlayer recursion node, not mask geometry. Keep
    // mask generation as a renderer-side reuse of source DrawFrameInfos rather
    // than suppressing an authored color item here.
    false
}

fn ctx_with_opacity(mut ctx: TravelContext, opa_raw: f32) -> TravelContext {
    ctx.opacity_multiplier = (ctx.opacity_multiplier * (opa_raw / 255.0)).clamp(0.0, 1.0);
    ctx
}

fn normalized_transform_order(order: &[i64]) -> [i64; 4] {
    if order.len() == 4 {
        let mut seen = [false; 4];
        let mut out = [0i64; 4];
        let mut valid = true;
        for (index, value) in order.iter().copied().enumerate() {
            if !(0..=3).contains(&value) || seen[value as usize] {
                valid = false;
                break;
            }
            seen[value as usize] = true;
            out[index] = value;
        }
        if valid {
            return out;
        }
    }
    // sub_10334D40 expects transformOrder to be a permutation of 0..3.
    // Malformed/missing data uses the native stage order instead of folding
    // unrelated stages together as the previous approximation did.
    [0, 3, 2, 1]
}

fn build_frame_linear_transform(
    transform_order: &[i64],
    flip_x: bool,
    flip_y: bool,
    rotation_degrees: f32,
    scale_x: f32,
    scale_y: f32,
    shear_x: f32,
    shear_y: f32,
) -> EmoteTransform2D {
    // Recovered from emotedriver_v.dll sub_10334D40.  Each transformOrder
    // stage left-multiplies the accumulated 2x2 matrix:
    //   0 = flip, 1 = rotation, 2 = zx/zy scale, 3 = sx/sy shear.
    let mut linear = EmoteTransform2D::identity();
    for stage in normalized_transform_order(transform_order) {
        let op = match stage {
            0 => EmoteTransform2D::flip(flip_x, flip_y),
            1 => EmoteTransform2D::rotation(rotation_degrees),
            2 => EmoteTransform2D::scale(scale_x, scale_y),
            3 => EmoteTransform2D::shear(shear_x, shear_y),
            _ => unreachable!(),
        };
        linear = op.then(linear);
    }
    linear
}

fn inherit_frame_linear_state(
    own: FrameLinearState,
    parent: FrameLinearState,
    inherit_mask: i64,
) -> FrameLinearState {
    // Recovered from sub_10331060.  The frame channels are combined
    // independently; flip uses XOR, angle/shear add, zoom multiplies.
    FrameLinearState {
        flip_x: own.flip_x ^ ((inherit_mask & 0x4) != 0 && parent.flip_x),
        flip_y: own.flip_y ^ ((inherit_mask & 0x8) != 0 && parent.flip_y),
        rotation_degrees: own.rotation_degrees
            + if (inherit_mask & 0x10) != 0 {
                parent.rotation_degrees
            } else {
                0.0
            },
        scale_x: own.scale_x
            * if (inherit_mask & 0x20) != 0 {
                parent.scale_x
            } else {
                1.0
            },
        scale_y: own.scale_y
            * if (inherit_mask & 0x40) != 0 {
                parent.scale_y
            } else {
                1.0
            },
        shear_x: own.shear_x
            + if (inherit_mask & 0x80) != 0 {
                parent.shear_x
            } else {
                0.0
            },
        shear_y: own.shear_y
            + if (inherit_mask & 0x100) != 0 {
                parent.shear_y
            } else {
                0.0
            },
    }
}

fn transform_without_translation(transform: EmoteTransform2D) -> EmoteTransform2D {
    EmoteTransform2D {
        tx: 0.0,
        ty: 0.0,
        ..transform
    }
}

fn current_context_as_inherit_source(ctx: &TravelContext) -> InheritSourceState {
    InheritSourceState {
        linear: transform_without_translation(ctx.transform),
        linear_state: ctx.linear_state,
        location: ctx.base_location.unwrap_or([0.0; 3]),
        coordinate: ctx.coordinate,
        opacity: ctx.opacity_multiplier,
        mesh_sync: None,
    }
}

fn remove_motion_root_linear_state(
    mut state: FrameLinearState,
    root: FrameLinearState,
    inherit_mask: i64,
) -> FrameLinearState {
    // sub_10331060 removes motion-root channels only when that same channel is
    // selected by inheritMask.  Removing every root channel here is subtly but
    // materially wrong: a child that inherits (for example) only angle must
    // retain its authored zoom/shear while the root matrix is reapplied.
    if (inherit_mask & 0x4) != 0 {
        state.flip_x ^= root.flip_x;
    }
    if (inherit_mask & 0x8) != 0 {
        state.flip_y ^= root.flip_y;
    }
    if (inherit_mask & 0x10) != 0 {
        state.rotation_degrees -= root.rotation_degrees;
    }
    if (inherit_mask & 0x20) != 0 {
        state.scale_x /= root.scale_x;
    }
    if (inherit_mask & 0x40) != 0 {
        state.scale_y /= root.scale_y;
    }
    if (inherit_mask & 0x80) != 0 {
        state.shear_x -= root.shear_x;
    }
    if (inherit_mask & 0x100) != 0 {
        state.shear_y -= root.shear_y;
    }
    state
}

fn map_child_coordinate_through_source(
    source: InheritSourceState,
    delta: [f32; 3],
) -> [f32; 3] {
    // sub_10331060 lines 135-152: coordinate==0 transforms XY and preserves Z;
    // non-zero coordinate transforms XZ and preserves Y.  The 2x2 matrix does
    // not contain translation; the ancestor's XYZ state is added afterwards.
    if source.coordinate.unwrap_or(0) != 0 {
        let mapped = source.linear.apply([delta[0], delta[2]]);
        [
            source.location[0] + mapped[0],
            source.location[1] + delta[1],
            source.location[2] + mapped[1],
        ]
    } else {
        let mapped = source.linear.apply([delta[0], delta[1]]);
        [
            source.location[0] + mapped[0],
            source.location[1] + mapped[1],
            source.location[2] + delta[2],
        ]
    }
}

fn apply_layer_transform(
    mut ctx: TravelContext,
    coord: Option<[f32; 3]>,
    flip_x: bool,
    flip_y: bool,
    scale_x: f32,
    scale_y: f32,
    rotation_degrees: f32,
    shear_x: f32,
    shear_y: f32,
) -> TravelContext {
    let delta = coord.unwrap_or([0.0, 0.0, 0.0]);
    let source = ctx.inherit_source;
    let parent_state = source.linear_state;
    let inherit_mask = ctx.inherit_mask.unwrap_or(0);
    let own_state = FrameLinearState {
        flip_x,
        flip_y,
        rotation_degrees,
        scale_x: finite_or(scale_x, 1.0),
        scale_y: finite_or(scale_y, 1.0),
        shear_x: finite_or(shear_x, 0.0),
        shear_y: finite_or(shear_y, 0.0),
    };
    let inherited_state = inherit_frame_linear_state(own_state, parent_state, inherit_mask);

    let own_linear = build_frame_linear_transform(
        &ctx.transform_order,
        own_state.flip_x,
        own_state.flip_y,
        own_state.rotation_degrees,
        own_state.scale_x,
        own_state.scale_y,
        own_state.shear_x,
        own_state.shear_y,
    );
    let linear = if (inherit_mask & 0x1fc) == 0x1fc {
        // sub_10331060 fast path: all linear channels inherited => multiply
        // ancestor matrix by the current local matrix.
        source.linear.then(own_linear)
    } else if !ctx.motion_independent_layer_inherit {
        // Native partial-inherit root compensation.  The authored composite
        // channels already contain whichever ancestor channels inheritMask
        // selected, including the motion root.  MMotionPlayer removes the
        // root channels only for local matrix construction and then applies
        // root.matrix as an outer transform.  Omitting this loses the outer
        // transform on partially-inheriting layers in nested motions.
        let relative_state = remove_motion_root_linear_state(
            inherited_state,
            ctx.motion_root.linear_state,
            inherit_mask,
        );
        let relative_linear = build_frame_linear_transform(
            &ctx.transform_order,
            relative_state.flip_x,
            relative_state.flip_y,
            relative_state.rotation_degrees,
            relative_state.scale_x,
            relative_state.scale_y,
            relative_state.shear_x,
            relative_state.shear_y,
        );
        ctx.motion_root.linear.then(relative_linear)
    } else {
        // Partial inheritance is channel-wise, not parent_matrix*child_matrix.
        build_frame_linear_transform(
            &ctx.transform_order,
            inherited_state.flip_x,
            inherited_state.flip_y,
            inherited_state.rotation_degrees,
            inherited_state.scale_x,
            inherited_state.scale_y,
            inherited_state.shear_x,
            inherited_state.shear_y,
        )
    };

    let world_origin = map_child_coordinate_through_source(source, delta);
    ctx.base_location = Some(world_origin);
    // Renderer storage is still a 2D affine matrix.  Keep the exact native
    // linear block and the projected XY origin here; native inheritance itself
    // uses the separate XYZ/source state above, so XZ coordinate layers no
    // longer corrupt descendant placement.
    ctx.transform = EmoteTransform2D {
        m11: linear.m11,
        m12: linear.m12,
        m21: linear.m21,
        m22: linear.m22,
        tx: world_origin[0],
        ty: world_origin[1],
    };
    ctx.linear_state = inherited_state;
    ctx
}

fn prepare_child_inherit_source(
    ctx: &mut TravelContext,
    current_mesh_sync: Option<MeshSyncChildState>,
) {
    // StepFrame walks upward while the *parent* has inheritMask & 0x400000.
    // Therefore a layer carrying this bit is transparent as an inheritance
    // source for its descendants: keep the source that this layer itself used.
    if (ctx.inherit_mask.unwrap_or(0) & 0x400000) != 0 {
        return;
    }
    ctx.inherit_source = InheritSourceState {
        linear: transform_without_translation(ctx.transform),
        linear_state: ctx.linear_state,
        location: ctx.base_location.unwrap_or([0.0; 3]),
        coordinate: ctx.coordinate,
        opacity: ctx.opacity_multiplier,
        mesh_sync: current_mesh_sync,
    };
}

fn frame_content_colors(content: &PsbValue) -> [u32; 4] {
    let blend_mode = content.field_u32("bm").unwrap_or(0x10);
    let Some(color) = content.field("color") else {
        return if (blend_mode & 0xF0) == 0 {
            [0xFFFF_FFFF; 4]
        } else {
            [0x8080_80FF; 4]
        };
    };
    if let Some(values) = color.as_list() {
        if values.len() >= 4 {
            let mut out = [0x8080_80FF; 4];
            for (dst, value) in out.iter_mut().zip(values.iter().take(4)) {
                if let Some(value) = value.as_i64() {
                    *dst = value as u32;
                }
            }
            out
        } else if let Some(value) = values.first().and_then(PsbValue::as_i64) {
            [value as u32; 4]
        } else {
            [0x8080_80FF; 4]
        }
    } else if let Some(value) = color.as_i64() {
        [value as u32; 4]
    } else {
        [0x8080_80FF; 4]
    }
}

fn build_sprite(
    textures: &BTreeMap<String, EmoteTextureSource>,
    texture_name: &str,
    icon_name: &str,
    label: Option<String>,
    motion_name: &str,
    base: [f32; 3],
    ox: f32,
    oy: f32,
    scale_x: f32,
    scale_y: f32,
    rotation_degrees: f32,
    visible: bool,
    opa_raw: f32,
    blend_mode: u32,
    blend_parameter: f32,
    corner_colors: [u32; 4],
    ctx: TravelContext,
) -> Option<EmoteStaticSprite> {
    let texture = textures.get(texture_name)?;
    let icon = texture.icons.get(icon_name)?;
    let width = icon.resolved_width();
    let height = icon.resolved_height();

    // FreeMote's win-path static painter applies this subtraction only for
    // MeshTransform.None. Keep that static behavior here. Dynamic traversal
    // evaluates recovered mesh deformation and child-mesh-sync semantics; this
    // static preview path does not re-evaluate parameterized mesh state.
    let subtract_icon_origin = ctx.mesh_transform == 0;
    let center_x = if subtract_icon_origin {
        ox - icon.origin_x
    } else {
        ox
    };
    let center_y = if subtract_icon_origin {
        oy - icon.origin_y
    } else {
        oy
    };

    Some(EmoteStaticSprite {
        label: label.clone(),
        motion_name: motion_name.to_owned(),
        texture_name: texture_name.to_owned(),
        texture_resource_index: texture.resource_index,
        texture_width: texture.width,
        texture_height: texture.height,
        texture_format: texture.format.clone(),
        icon_name: icon_name.to_owned(),
        feedback_history: false,
        z: base[2],
        opacity: (ctx.opacity_multiplier * (opa_raw / 255.0)).clamp(0.0, 1.0),
        blend_mode,
        blend_parameter,
        corner_colors,
        visible,
        center_x,
        center_y,
        width,
        height,
        scale_x,
        scale_y,
        rotation_degrees,
        world_transform: ctx.transform.as_array(),
        uv_left: icon.left / texture.width as f32,
        uv_top: icon.top / texture.height as f32,
        uv_right: (icon.left + width) / texture.width as f32,
        uv_bottom: (icon.top + height) / texture.height as f32,
        mesh: ctx.mesh_patch,
        draw_frame_info: draw_frame_info(label, &ctx),
    })
}

fn build_feedback_history_sprite(
    state: &EmoteStepFrameLayerState,
    motion_name: &str,
) -> Option<EmoteStaticSprite> {
    if state.draw_frame_info.layer_type != 10 {
        return None;
    }
    let frame = state.specialized_frame.as_ref()?;
    let timespan = frame.feedback_timespan?;
    let [left, top, right, bottom] = state.screen_bounds?;
    let width = right - left;
    let height = bottom - top;
    if !width.is_finite() || !height.is_finite() || width.abs() <= f32::EPSILON || height.abs() <= f32::EPSILON {
        return None;
    }
    Some(EmoteStaticSprite {
        label: state.draw_frame_info.layer_label.clone(),
        motion_name: motion_name.to_owned(),
        texture_name: "__eluna_feedback_history".to_owned(),
        texture_resource_index: u32::MAX,
        texture_width: 1,
        texture_height: 1,
        texture_format: None,
        icon_name: "__framebuffer".to_owned(),
        feedback_history: true,
        z: state.raw_position[2],
        opacity: state.opacity,
        blend_mode: frame.blend_mode,
        blend_parameter: frame.blend_parameter,
        corner_colors: frame.colors,
        visible: state.visible && timespan.abs() > f32::EPSILON,
        center_x: (left + right) * 0.5,
        center_y: (top + bottom) * 0.5,
        width: width.abs(),
        height: height.abs(),
        scale_x: if width < 0.0 { -1.0 } else { 1.0 },
        scale_y: if height < 0.0 { -1.0 } else { 1.0 },
        rotation_degrees: 0.0,
        world_transform: state.transform,
        uv_left: if width < 0.0 { 1.0 } else { 0.0 },
        uv_top: if height < 0.0 { 1.0 } else { 0.0 },
        uv_right: if width < 0.0 { 0.0 } else { 1.0 },
        uv_bottom: if height < 0.0 { 0.0 } else { 1.0 },
        mesh: None,
        draw_frame_info: state.draw_frame_info.clone(),
    })
}

fn feedback_pow_channel(value: u32, neutral: f32, decay: f32) -> u32 {
    let base = (value.max(1) as f32 / neutral).max(f32::MIN_POSITIVE);
    (base.powf(decay) * neutral).clamp(0.0, 255.0).round() as u32
}

fn apply_feedback_specialized_pass(
    layer_states: &mut [EmoteStepFrameLayerState],
    sprites: &mut [EmoteStaticSprite],
    scope_start: usize,
    scope_end: usize,
    delta_ticks: f32,
) {
    // MMotionPlayer::StepFrameFeedback (sub_10352800) is the final specialized
    // pass. The host supplies the prior framebuffer; this function reproduces
    // the layer-state decay that is independent of the graphics backend.
    let end = scope_end.min(layer_states.len());
    let begin = scope_start.min(end);
    if begin >= end {
        return;
    }
    let root_positions: BTreeMap<String, [f32; 3]> = layer_states[begin..end]
        .iter()
        .filter(|state| state.path == state.motion_scope_root_path)
        .map(|state| (state.path.clone(), state.raw_position))
        .collect();
    let mut updated = BTreeMap::<String, (EmoteFeedbackRuntimeState, [f32; 6], f32, [u32; 4])>::new();

    for state in layer_states.iter_mut().take(end).skip(begin) {
        if state.draw_frame_info.layer_type != 10 {
            continue;
        }
        let Some(frame) = state.specialized_frame.as_mut() else {
            state.feedback_runtime = None;
            continue;
        };
        let Some(timespan) = frame.feedback_timespan else {
            state.feedback_runtime = None;
            continue;
        };
        let Some(screen_bounds) = state.screen_bounds else {
            state.feedback_runtime = None;
            continue;
        };
        let root_raw = root_positions
            .get(&state.motion_scope_root_path)
            .copied()
            .unwrap_or(state.raw_position);
        let active = state.visible
            && delta_ticks.abs() > f32::EPSILON
            && timespan.abs() > f32::EPSILON;
        let decay = if active {
            (delta_ticks / 60.0) / timespan
        } else {
            0.0
        };
        let runtime = EmoteFeedbackRuntimeState {
            timespan,
            decay_factor: decay,
            screen_bounds,
            active,
        };
        state.feedback_runtime = Some(runtime.clone());
        if !active {
            updated.insert(state.path.clone(), (runtime, state.transform, state.opacity, frame.colors));
            continue;
        }

        let linear = &mut state.linear_state;
        linear.rotation_degrees = if linear.rotation_degrees >= 180.0 {
            360.0 - (360.0 - linear.rotation_degrees) * decay
        } else {
            linear.rotation_degrees * decay
        };
        let width = (screen_bounds[2] - screen_bounds[0]).abs();
        let height = (screen_bounds[3] - screen_bounds[1]).abs();
        if width > f32::EPSILON {
            let base = linear.scale_x * 32.0 / width;
            linear.scale_x = if base >= 0.0 { base.powf(decay) } else { -(-base).powf(decay) };
        }
        if height > f32::EPSILON {
            let base = linear.scale_y * 32.0 / height;
            linear.scale_y = if base >= 0.0 { base.powf(decay) } else { -(-base).powf(decay) };
        }
        linear.shear_x *= decay;
        linear.shear_y *= decay;

        for i in 0..3 {
            state.raw_position[i] = root_raw[i] + (state.raw_position[i] - root_raw[i]) * decay;
            state.position[i] = root_raw[i] + (state.position[i] - root_raw[i]) * decay;
        }
        let rebuilt = build_frame_linear_transform(
            &state.draw_frame_info.transform_order,
            linear.flip_x,
            linear.flip_y,
            linear.rotation_degrees,
            linear.scale_x,
            linear.scale_y,
            linear.shear_x,
            linear.shear_y,
        );
        state.transform = [
            rebuilt.m11,
            rebuilt.m12,
            rebuilt.m21,
            rebuilt.m22,
            state.raw_position[0],
            state.raw_position[1],
        ];

        state.opacity = state
            .opacity
            .max(1.0 / 255.0)
            .powf(decay)
            .clamp(0.0, 1.0);
        let neutral = if (frame.blend_mode & 0xF0) == 0x10 { 128.0 } else { 255.0 };
        for packed in &mut frame.colors {
            let r = feedback_pow_channel((*packed >> 24) & 0xff, neutral, decay);
            let g = feedback_pow_channel((*packed >> 16) & 0xff, neutral, decay);
            let b = feedback_pow_channel((*packed >> 8) & 0xff, neutral, decay);
            let a = feedback_pow_channel(*packed & 0xff, 255.0, decay);
            *packed = (r << 24) | (g << 16) | (b << 8) | a;
        }
        updated.insert(state.path.clone(), (runtime, state.transform, state.opacity, frame.colors));
    }

    for sprite in sprites.iter_mut().filter(|sprite| sprite.feedback_history) {
        if let Some((runtime, transform, opacity, colors)) = updated.get(&sprite.draw_frame_info.path) {
            sprite.world_transform = *transform;
            sprite.opacity = *opacity;
            sprite.corner_colors = *colors;
            sprite.visible &= runtime.active;
        }
    }
}

fn compute_bounds(sprites: &[EmoteStaticSprite]) -> Option<EmoteSceneBounds> {
    let mut iter = sprites
        .iter()
        .filter(|sprite| sprite.visible && sprite.opacity > 0.0);
    let first = iter.next()?;
    let (left, top, right, bottom) = first.bounds_rect();
    let mut bounds = EmoteSceneBounds {
        min_x: left,
        min_y: top,
        max_x: right,
        max_y: bottom,
    };
    for sprite in iter {
        let (left, top, right, bottom) = sprite.bounds_rect();
        bounds.include_rect(left, top, right, bottom);
    }
    Some(bounds)
}

fn mesh_sync_warp_point(sync: MeshSyncChildState, point: [f32; 2]) -> Option<[f32; 2]> {
    let [left, top, width, height] = sync.patch.domain?;
    if !width.is_finite()
        || !height.is_finite()
        || width.abs() <= f32::EPSILON
        || height.abs() <= f32::EPSILON
    {
        return None;
    }
    let u = (point[0] - left) / width;
    let v = (point[1] - top) / height;
    let mapped = sync.patch.sample(u, v);
    Some([left + mapped[0] * width, top + mapped[1] * height])
}

fn apply_mesh_sync_child_state(
    state: &mut DynamicFrameState,
    sync: MeshSyncChildState,
    inherit_mask: i64,
) {
    let Some(mut coord) = state.coord else {
        return;
    };

    // sub_10335500 selects XY for coordinate==0 and XZ otherwise.
    let use_xz = sync.coordinate.unwrap_or(0) != 0;
    let point = if use_xz {
        [coord[0], coord[2]]
    } else {
        [coord[0], coord[1]]
    };
    let Some(mapped) = mesh_sync_warp_point(sync, point) else {
        return;
    };

    // The native code samples a diamond around the original child coordinate
    // at +/-0.0001 to recover the local mesh Jacobian.  Preserve that finite
    // difference instead of differentiating the cubic analytically so edge
    // behaviour matches the DLL path.
    const EPS: f32 = 0.0001;
    let xm = mesh_sync_warp_point(sync, [point[0] - EPS, point[1]]);
    let xp = mesh_sync_warp_point(sync, [point[0] + EPS, point[1]]);
    let ym = mesh_sync_warp_point(sync, [point[0], point[1] - EPS]);
    let yp = mesh_sync_warp_point(sync, [point[0], point[1] + EPS]);

    if (sync.mask & 0x2) != 0 && (inherit_mask & 0x10) != 0 {
        if let (Some(xm), Some(xp), Some(ym), Some(yp)) = (xm, xp, ym, yp) {
            let dx = [xp[0] - xm[0], xp[1] - xm[1]];
            let dy = [yp[0] - ym[0], yp[1] - ym[1]];
            if dx[0].is_finite() && dx[1].is_finite() && dy[0].is_finite() && dy[1].is_finite() {
                // sub_10335500 averages the orientation of the deformed X axis
                // and the deformed Y axis (atan2(dx.y, dx.x) and
                // atan2(-dy.x, dy.y)).
                let ax = dx[1].atan2(dx[0]);
                let ay = (-dy[0]).atan2(dy[1]);
                state.rotation_degrees += ((ax + ay) * 0.5).to_degrees();
            }
        }
    }

    if (sync.mask & 0x4) != 0 && (inherit_mask & 0x60) != 0 {
        if let (Some(xm), Some(xp), Some(ym), Some(yp)) = (xm, xp, ym, yp) {
            // sub_10335500 / sub_103A4C50 do not collapse the four samples
            // into a Jacobian determinant.  They measure the actual warped
            // diamond as two triangle areas, then use
            // sqrt(2 * area) / 0.0002.  This differs from the determinant
            // shortcut on a non-linear Bezier patch and is observable close
            // to strongly deformed mesh edges.
            let tri_area = |a: [f32; 2], b: [f32; 2], c: [f32; 2]| {
                (((b[0] - a[0]) * (c[1] - a[1])
                    - (b[1] - a[1]) * (c[0] - a[0]))
                    .abs())
                    * 0.5
            };
            let area = tri_area(xm, xp, ym) + tri_area(xm, xp, yp);
            let scale = (2.0 * area).sqrt() / (2.0 * EPS);
            if scale.is_finite() {
                if (inherit_mask & 0x20) != 0 {
                    state.scale_x *= scale;
                }
                if (inherit_mask & 0x40) != 0 {
                    state.scale_y *= scale;
                }
            }
        }
    }

    if (sync.mask & 0x1) != 0 {
        coord[0] = mapped[0];
        if use_xz {
            coord[2] = mapped[1];
        } else {
            coord[1] = mapped[1];
        }
        state.coord = Some(coord);
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ParticleFrameState {
    trigger: i32,
    fmin: f32,
    fmax: f32,
    vmin: f32,
    vmax: f32,
    amin: f32,
    amax: f32,
    zmin: f32,
    zmax: f32,
    range: f32,
}

impl Default for ParticleFrameState {
    fn default() -> Self {
        // sub_1033D0E0 native defaults.
        Self {
            trigger: 0,
            fmin: 10.0,
            fmax: 10.0,
            vmin: 0.0,
            vmax: 0.0,
            amin: 0.0,
            amax: 0.0,
            zmin: 1.0,
            zmax: 1.0,
            range: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct CameraFrameState {
    fov: f32,
    target: String,
}

#[derive(Debug, Clone, PartialEq)]
struct ModelFrameState {
    looped: bool,
    direction_type: i32,
    direction_target: String,
    time_offset_ticks: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct StencilWipeFrameState {
    enabled: bool,
    reverse: bool,
    threshold: f32,
    softness: f32,
    scale: f32,
    bias: f32,
}

impl StencilWipeFrameState {
    fn from_native(enabled: bool, reverse: bool, threshold: f32, softness: f32) -> Self {
        if !enabled {
            return Self { enabled, reverse, threshold, softness, scale: 0.0, bias: 0.0 };
        }
        let scale = 1024.0 / (1023.0 * softness + 1.0);
        let bias = 1.0 - ((scale + 1.0) * threshold);
        Self { enabled, reverse, threshold, softness, scale, bias }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct DynamicFrameState {
    coord: Option<[f32; 3]>,
    ox: f32,
    oy: f32,
    /// Native frame +80/+81 (`fx`/`fy`).  These are step values, not lerped.
    flip_x: bool,
    flip_y: bool,
    /// Native frame +88/+92 (`zx`/`zy`).
    scale_x: f32,
    scale_y: f32,
    /// Native frame +84 (`angle`).
    rotation_degrees: f32,
    /// Native frame +96/+100 (`sx`/`sy`) shear/skew terms.
    shear_x: f32,
    shear_y: f32,
    src: Option<String>,
    icon: Option<String>,
    opa: f32,
    /// Native decoded frame +32/+36. Default bm=0x10 is MODULATE2X with
    /// neutral 0x808080FF corner color.
    blend_mode: u32,
    blend_parameter: f32,
    /// Native decoded frame +48..+60, packed as 0xRRGGBBAA.
    colors: [u32; 4],
    /// frame+18: true when a scalar `color` was broadcast to all corners.
    single_color: bool,
    /// frame+19: true when no authored color was present.
    default_color: bool,
    /// Native motion-frame payload at frameInfo+208 (sub_1033D0E0).
    motion_flags: u32,
    motion_direction_type: i32,
    motion_direction_offset_degrees: f32,
    motion_direction_offset_complete: bool,
    motion_direction_target: Option<String>,
    /// Local path tangent recovered by nested-motion direction mode 3.
    /// Modes 2 and 4 require the native post-StepFrame specialized pass and
    /// therefore remain unresolved here until the traversal is two-phase.
    motion_path_tangent_degrees: Option<f32>,
    motion_path_tangent_vector: Option<[f32; 3]>,
    time_offset_ticks: f32,
    /// Native type-4 particle frame payload (decoded frame +208).
    particle: Option<ParticleFrameState>,
    /// Native type-5 camera frame payload. StepLayer interpolates only FOV;
    /// target is a current-frame step value consumed by the camera pass.
    camera: Option<CameraFrameState>,
    /// Native type-6 model frame payload. The specialized model pass consumes
    /// this directly from the current decoded frame rather than StepLayer.
    model: Option<ModelFrameState>,
    /// Native type-10 feedback payload. StepLayer interpolates `timespan`.
    feedback_timespan: Option<f32>,
    /// Native type-12 stencil/wipe payload and its derived runtime scale/bias.
    stencil_wipe: Option<StencilWipeFrameState>,
    /// Native type-9 frame payload (`anchor.target`). `Some("")` is
    /// meaningful: sub_10351120 falls back to layer 0 when target lookup
    /// fails, so keep an authored empty target distinct from no anchor payload.
    anchor_target: Option<String>,
    /// Start time of the currently selected key frame. Nested MMotionPlayer
    /// time is relative to this value, not to the parent motion origin.
    frame_start_ticks: f32,
    /// Serialized frame type selected by the frame cursor. Type 0 is the
    /// native invalid/HOLD marker (`frame+16 != 0`) even though decoded local
    /// channels are retained from the previous runtime frame.
    serialized_frame_type: i64,
    mesh_patch: Option<EmoteMeshPatch>,
    frame_index: Option<usize>,
    next_frame_index: Option<usize>,
    interpolation_t: f32,
}

impl Default for DynamicFrameState {
    fn default() -> Self {
        Self {
            coord: None,
            ox: 0.0,
            oy: 0.0,
            flip_x: false,
            flip_y: false,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation_degrees: 0.0,
            shear_x: 0.0,
            shear_y: 0.0,
            src: None,
            icon: None,
            opa: 255.0,
            blend_mode: 0x10,
            blend_parameter: 0.0,
            colors: [0x8080_80FF; 4],
            single_color: true,
            default_color: true,
            motion_flags: 0,
            motion_direction_type: 1,
            motion_direction_offset_degrees: 0.0,
            motion_direction_offset_complete: false,
            motion_direction_target: None,
            motion_path_tangent_degrees: None,
            motion_path_tangent_vector: None,
            time_offset_ticks: 0.0,
            particle: None,
            camera: None,
            model: None,
            feedback_timespan: None,
            stencil_wipe: None,
            anchor_target: None,
            frame_start_ticks: 0.0,
            serialized_frame_type: 3,
            mesh_patch: None,
            frame_index: None,
            next_frame_index: None,
            interpolation_t: 0.0,
        }
    }
}

fn effective_motion_time(motion: &PsbValue, time_ticks: f32) -> f32 {
    // MMotionPlayer::Progress (sub_10350870) keeps accumulated time (+280)
    // separate from effective motion time (+284).  Forward playback wraps at
    // lastTime (+384) back to loopTime (+380) when loopTime is non-negative;
    // a negative loopTime means one-shot playback and clamps at lastTime.
    if !time_ticks.is_finite() || time_ticks <= 0.0 {
        return 0.0;
    }
    let last_time = motion
        .field_f32("lastTime")
        .filter(|value| value.is_finite() && *value > 0.0)
        .unwrap_or_else(|| {
            motion
                .field("layer")
                .and_then(PsbValue::as_list)
                .map(|layers| layers.iter().map(max_layer_frame_time).fold(0.0, f32::max))
                .unwrap_or(0.0)
        });
    if last_time <= 0.0 {
        return time_ticks;
    }
    if time_ticks < last_time {
        return time_ticks;
    }
    let loop_time = motion.field_f32("loopTime").unwrap_or(-1.0);
    if loop_time.is_finite() && loop_time >= 0.0 && loop_time < last_time {
        let span = last_time - loop_time;
        loop_time + (time_ticks - loop_time).rem_euclid(span)
    } else {
        last_time
    }
}

fn motion_duration_ticks(motion: &PsbValue) -> Option<f32> {
    let mut max_time = motion
        .field_f32("lastTime")
        .or_else(|| motion.field_f32("loopTime"))
        .unwrap_or(0.0)
        .max(0.0);
    if let Some(layers) = motion.field("layer").and_then(PsbValue::as_list) {
        for layer in layers {
            max_time = max_time.max(max_layer_frame_time(layer));
        }
    }
    (max_time > 0.0).then_some(max_time)
}

fn motion_priority_ranks(motion: &PsbValue, time_ticks: f32) -> BTreeMap<String, usize> {
    let Some(layers) = motion.field("layer").and_then(PsbValue::as_list) else {
        return BTreeMap::new();
    };
    let Some(priority_content) = evaluate_priority_content(
        motion.field("priority").and_then(PsbValue::as_list),
        time_ticks,
    ) else {
        return BTreeMap::new();
    };

    // sub_10333180 recursively builds one flat LayerInfo vector in preorder.
    // LayerInfo[0] is the synthetic player root, so authored layers occupy
    // native indices 1..N. priority.content stores zero-based authored-layer
    // indices. sub_103390C0 then does, literally:
    //
    //   layer = LayerInfo[priorityFrameList[N - i - 1] + 1]
    //
    // for i=0..N-1. layerIndexMap is *not* consulted by this draw path; it is
    // retained by the native player for name/index lookups elsewhere. The old
    // Rust implementation incorrectly converted priority values through
    // layerIndexMap and keyed the result by a bare label, which breaks nested
    // children and duplicate labels and is why simply reversing priority made
    // the picture even worse.
    let mut flat_structural_paths = Vec::<String>::new();
    flatten_native_layer_index_paths(layers, "", &mut flat_structural_paths);

    let mut ranks = BTreeMap::<String, usize>::new();
    for (emit_rank, value) in priority_content.iter().rev().enumerate() {
        let Some(flat_index) = value.as_i64().and_then(|v| usize::try_from(v).ok()) else {
            continue;
        };
        let Some(index_path) = flat_structural_paths.get(flat_index) else {
            continue;
        };
        // Native priority lists are permutations, but keep the first emitted
        // occurrence if malformed data repeats an index.
        ranks.entry(index_path.clone()).or_insert(emit_rank);
    }

    // A valid E-mote archive contains every authored layer exactly once in the
    // current priority frame. Compatibility fallback remains structural too,
    // so duplicate/empty labels can never alias another LayerInfo slot.
    let mut fallback_rank = priority_content.len();
    for index_path in flat_structural_paths {
        if !ranks.contains_key(&index_path) {
            ranks.insert(index_path, fallback_rank);
            fallback_rank = fallback_rank.saturating_add(1);
        }
    }
    ranks
}

fn flatten_native_layer_index_paths(layers: &[PsbValue], prefix: &str, out: &mut Vec<String>) {
    for (sibling_index, layer) in layers.iter().enumerate() {
        let index_path = if prefix.is_empty() {
            sibling_index.to_string()
        } else {
            format!("{prefix}/{sibling_index}")
        };
        out.push(index_path.clone());

        // sub_10333180 recursively parses the authored `children` array in
        // preorder. Keep the historical nested `layer` compatibility fallback,
        // but never let labels participate in structural priority identity.
        if let Some(children) = layer.field("children").and_then(PsbValue::as_list) {
            flatten_native_layer_index_paths(children, &index_path, out);
        } else if let Some(children) = layer.field("layer").and_then(PsbValue::as_list) {
            flatten_native_layer_index_paths(children, &index_path, out);
        }
    }
}

fn evaluate_priority_content(
    priority: Option<&[PsbValue]>,
    time_ticks: f32,
) -> Option<&[PsbValue]> {
    let priority = priority?;
    let mut current = priority.first();
    for frame in priority {
        let frame_time = frame.field_f32("time").unwrap_or(0.0);
        if frame_time > time_ticks {
            break;
        }
        current = Some(frame);
    }
    // sub_10340BA0 reads `content` from the currently selected priority frame
    // unconditionally. It does not filter the frame by `type` here.
    current?.field("content").and_then(PsbValue::as_list)
}

fn max_layer_frame_time(layer: &PsbValue) -> f32 {
    let mut max_time: f32 = 0.0;
    if let Some(frame_list) = layer.field("frameList").and_then(PsbValue::as_list) {
        for frame in frame_list {
            max_time = max_time.max(frame.field_f32("time").unwrap_or(0.0));
        }
    }
    if let Some(children) = layer.field("children").and_then(PsbValue::as_list) {
        for child in children {
            max_time = max_time.max(max_layer_frame_time(child));
        }
    }
    if let Some(children) = layer.field("layer").and_then(PsbValue::as_list) {
        for child in children {
            max_time = max_time.max(max_layer_frame_time(child));
        }
    }
    max_time
}

fn evaluate_frame_list(
    frame_list: &[PsbValue],
    time_ticks: f32,
    easing_table: Option<&[PsbValue]>,
    coordinate_plane: i64,
    previous_state: Option<&DynamicFrameState>,
) -> DynamicFrameState {
    // MMotionPlayer keeps two fully parsed 212-byte frame buffers.  Each
    // serialized frame is decoded from native defaults; frame contents are not
    // accumulated across history.  Only a current type-3 frame is allowed to
    // interpolate to its immediate successor (sub_1033EAB0/sub_1032FB00).
    let current_index = frame_list
        .iter()
        .enumerate()
        .take_while(|(_, frame)| frame.field_f32("time").unwrap_or(0.0) <= time_ticks)
        .map(|(index, _)| index)
        .last();
    let Some(current_index) = current_index else {
        return DynamicFrameState::default();
    };

    let current_frame = &frame_list[current_index];
    let current_time = current_frame.field_f32("time").unwrap_or(0.0);
    let current_type = current_frame.field_i64("type").unwrap_or(3);
    let mut state = DynamicFrameState::default();
    state.frame_index = Some(current_index);
    state.frame_start_ticks = current_time;
    state.serialized_frame_type = current_type;

    // Parser sub_1033EAB0 marks serialized type 0 as frame+16 invalid.
    // StepLayer sub_1032FB00 immediately returns when that flag is set, leaving
    // the layer's already-decoded local runtime state untouched. The active
    // frame cursor still points at this HOLD key, which StepFrameReadyToDraw
    // checks through frame+16; retain the prior channels but publish the new
    // cursor/type metadata.
    if current_type == 0 {
        let mut held = previous_state.cloned().unwrap_or(state);
        held.frame_index = Some(current_index);
        held.next_frame_index = frame_list.get(current_index + 1).map(|_| current_index + 1);
        held.frame_start_ticks = current_time;
        held.serialized_frame_type = 0;
        held.interpolation_t = 0.0;
        return held;
    }

    let Some(current_content) = current_frame.field("content") else {
        return state;
    };
    merge_frame_content(&mut state, current_content);

    let next_index = current_index + 1;
    let Some(next_frame) = frame_list.get(next_index) else {
        return state;
    };
    state.next_frame_index = Some(next_index);

    // frame+17 is set only for serialized type 3.  A type-0 next frame has
    // frame+16 invalid and therefore forces the native copy/hold path.
    if current_type != 3 || next_frame.field_i64("type").unwrap_or(3) == 0 {
        return state;
    }
    let Some(next_content) = next_frame.field("content") else {
        return state;
    };

    let next_time = next_frame.field_f32("time").unwrap_or(current_time);
    let span = next_time - current_time;
    if !span.is_finite() || span <= f32::EPSILON {
        return state;
    }

    let mut elapsed = (time_ticks - current_time).max(0.0);
    // frameInfo+8 (`ti`) is an integer time-quantization interval.  Native
    // truncates elapsed/ti toward zero then multiplies back before deriving t.
    let ti = current_content
        .field_i64("ti")
        .or_else(|| current_frame.field_i64("ti"))
        .unwrap_or(0);
    if ti > 0 {
        let ti = ti as f32;
        elapsed = ti * (elapsed / ti).trunc();
    }
    let t = elapsed / span;
    state.interpolation_t = t;

    let mut next_state = DynamicFrameState::default();
    merge_frame_content(&mut next_state, next_content);
    interpolate_frame_content(
        &mut state,
        &next_state,
        current_content,
        next_content,
        t,
        easing_table,
        coordinate_plane,
    );
    state
}

fn frame_easing(
    t: f32,
    easing_ref: Option<&PsbValue>,
    easing_table: Option<&[PsbValue]>,
) -> f32 {
    // MMotionPlayer property interpolation does not use the public controller
    // easing exponent. ccc/occ/acc/zcc/scc references resolve through the
    // player easing table, and sub_1039E480/sub_1034A4B0 evaluate the authored
    // cubic spline. A null/unresolved easing object is native identity.
    let t = finite_or(t, 0.0).clamp(0.0, 1.0);
    let Some(curve) = resolve_frame_easing_curve(easing_ref, easing_table) else {
        return t;
    };
    evaluate_native_easing_curve(curve, t).unwrap_or(t)
}

fn resolve_frame_easing_curve<'a>(
    easing_ref: Option<&'a PsbValue>,
    easing_table: Option<&'a [PsbValue]>,
) -> Option<&'a PsbValue> {
    let value = easing_ref?;
    // Normal E-mote PSB stores an index into the top-level `easing` table.
    // Accept an inline list/object as well because normalized/debug PSBs may
    // already have the reference expanded.
    if matches!(value, PsbValue::List(_) | PsbValue::Object(_)) {
        return Some(value);
    }
    let index = value.as_i64()?;
    if index < 0 {
        return None;
    }
    easing_table?.get(index as usize)
}

fn easing_piece_values(piece: &PsbValue) -> Option<(Vec<f32>, Vec<f32>, Vec<f32>)> {
    let xs = piece
        .field("x")?
        .as_list()?
        .iter()
        .map(PsbValue::as_f32)
        .collect::<Option<Vec<_>>>()?;
    let ys = piece
        .field("y")?
        .as_list()?
        .iter()
        .map(PsbValue::as_f32)
        .collect::<Option<Vec<_>>>()?;
    let ps = piece
        .field("p")?
        .as_list()?
        .iter()
        .map(PsbValue::as_f32)
        .collect::<Option<Vec<_>>>()?;
    if xs.len() < 2 || xs.len() != ys.len() || xs.len() != ps.len() {
        return None;
    }
    Some((xs, ys, ps))
}

fn evaluate_native_easing_piece(piece: &PsbValue, x: f32) -> Option<f32> {
    let (xs, ys, ps) = easing_piece_values(piece)?;
    let mut interval = 0usize;
    while interval + 1 < xs.len() - 1 && x > xs[interval + 1] {
        interval += 1;
    }
    while interval > 0 && x < xs[interval] {
        interval -= 1;
    }
    let x0 = xs[interval];
    let x1 = xs[interval + 1];
    let h = x1 - x0;
    if !h.is_finite() || h.abs() <= f32::EPSILON {
        return Some(ys[interval]);
    }
    let u = (x - x0) / h;
    let one_minus_u = 1.0 - u;
    // sub_10397990 is f(q)=q^3-q. sub_1034A4B0 combines that
    // with the stored `p` values exactly as a cubic-spline second-derivative
    // representation.
    let cubic_u = u * u * u - u;
    let cubic_v = one_minus_u * one_minus_u * one_minus_u - one_minus_u;
    let linear = one_minus_u * ys[interval] + u * ys[interval + 1];
    Some(
        linear
            + h * h
                * (cubic_u * ps[interval + 1] + cubic_v * ps[interval])
                / 6.0,
    )
}

fn evaluate_native_easing_curve(curve: &PsbValue, x: f32) -> Option<f32> {
    if curve.field("x").is_some() {
        return evaluate_native_easing_piece(curve, x);
    }
    let pieces = curve.as_list()?;
    if pieces.is_empty() {
        return None;
    }
    // sub_1034A4B0 moves the cached piece index until x lies between the
    // selected piece's first/last authored x value. Stateless lookup gives the
    // same result for a single evaluation.
    for piece in pieces {
        let Some((xs, _, _)) = easing_piece_values(piece) else {
            continue;
        };
        if x >= xs[0] && x <= *xs.last().unwrap_or(&xs[0]) {
            return evaluate_native_easing_piece(piece, x);
        }
    }
    // Native input is normally [0,1] and authored curves cover that domain.
    // If a malformed normalized file has a gap, use the closest endpoint
    // rather than inventing a non-native smoothstep fallback.
    let mut closest: Option<(&PsbValue, f32)> = None;
    for piece in pieces {
        let Some((xs, _, _)) = easing_piece_values(piece) else {
            continue;
        };
        let d = (x - xs[0])
            .abs()
            .min((x - *xs.last().unwrap_or(&xs[0])).abs());
        if closest.map_or(true, |(_, best)| d < best) {
            closest = Some((piece, d));
        }
    }
    closest.and_then(|(piece, _)| evaluate_native_easing_piece(piece, x))
}

fn evaluate_native_spline_piece_clamped(piece: &PsbValue, x: f32) -> Option<f32> {
    let (xs, ys, _) = easing_piece_values(piece)?;
    let first_x = *xs.first()?;
    let last_x = *xs.last()?;
    if x <= first_x {
        return ys.first().copied();
    }
    if x >= last_x {
        return ys.last().copied();
    }
    evaluate_native_easing_piece(piece, x)
}

/// Evaluate an inline MBeziersPathEntity recovered from sub_1030CEA0 and
/// sub_10349D50.
///
/// Native serialized layout:
///   x/y: cubic-Bezier control-point arrays (3*N + 1 values),
///   t:   authored segment boundaries,
///   s:   one scalar cubic-spline parameter remap per path segment. Each
///        spline object has x/y/p arrays using the same representation as the
///        ordinary easing entity.
fn evaluate_native_beziers_path(path: &PsbValue, t: f32) -> Option<[f32; 2]> {
    // MMotionPlayer+848 is a lazy MBeziersPathEntity cache object created by
    // sub_10337360. sub_1033D0E0 passes the frame's `cp` PSB value itself to
    // that cache, and a miss constructs sub_1030CEA0 directly from the value.
    // Therefore the ordinary authored cp payload is this object; no separate
    // root path table needs to be guessed.
    let path_x = path
        .field("x")?
        .as_list()?
        .iter()
        .map(PsbValue::as_f32)
        .collect::<Option<Vec<_>>>()?;
    let path_y = path
        .field("y")?
        .as_list()?
        .iter()
        .map(PsbValue::as_f32)
        .collect::<Option<Vec<_>>>()?;
    let times = path
        .field("t")?
        .as_list()?
        .iter()
        .map(PsbValue::as_f32)
        .collect::<Option<Vec<_>>>()?;
    let splines = path.field("s")?.as_list()?;
    if times.len() < 2 || splines.is_empty() {
        return None;
    }

    let segment_count = times.len() - 1;
    if splines.len() < segment_count {
        return None;
    }
    let required_points = segment_count.checked_mul(3)?.checked_add(1)?;
    if path_x.len() < required_points || path_y.len() < required_points {
        return None;
    }

    // sub_10349D50 maintains a cached segment index and walks it forward or
    // backward until t lies in [T[i], T[i+1]]. Stateless selection is
    // equivalent for one evaluation.
    let mut segment = 0usize;
    while segment + 1 < segment_count && t > times[segment + 1] {
        segment += 1;
    }
    while segment > 0 && times[segment] > t {
        segment -= 1;
    }

    let t0 = times[segment];
    let t1 = times[segment + 1];
    let span = t1 - t0;
    let local = if span.is_finite() && span.abs() > f32::EPSILON {
        (t - t0) / span
    } else {
        0.0
    };
    let u = evaluate_native_spline_piece_clamped(&splines[segment], local)?;
    let v = 1.0 - u;
    let b0 = v * v * v;
    let b1 = 3.0 * v * v * u;
    let b2 = 3.0 * v * u * u;
    let b3 = u * u * u;
    let base = segment * 3;
    Some([
        b0 * path_x[base]
            + b1 * path_x[base + 1]
            + b2 * path_x[base + 2]
            + b3 * path_x[base + 3],
        b0 * path_y[base]
            + b1 * path_y[base + 1]
            + b2 * path_y[base + 2]
            + b3 * path_y[base + 3],
    ])
}

fn interpolate_native_coordinate(
    start: [f32; 3],
    end: [f32; 3],
    t: f32,
    coordinate_plane: i64,
    cp_ref: Option<&PsbValue>,
) -> [f32; 3] {
    // sub_103A5190 has an exact-equality fast path before ccc/cp processing.
    if start == end {
        return start;
    }

    let Some([u, v]) = cp_ref.and_then(|path| evaluate_native_beziers_path(path, t)) else {
        return [
            lerp(start[0], end[0], t),
            lerp(start[1], end[1], t),
            lerp(start[2], end[2], t),
        ];
    };

    if coordinate_plane == 0 {
        let dx = end[0] - start[0];
        let dy = end[1] - start[1];
        [
            start[0] + dx * u - dy * v,
            start[1] + dy * u + dx * v,
            lerp(start[2], end[2], t),
        ]
    } else {
        let dx = end[0] - start[0];
        let dz = end[2] - start[2];
        [
            start[0] + dx * u - dz * v,
            lerp(start[1], end[1], t),
            start[2] + dz * u + dx * v,
        ]
    }
}

fn interpolate_frame_content(
    state: &mut DynamicFrameState,
    next_state: &DynamicFrameState,
    current_content: &PsbValue,
    next_content: &PsbValue,
    t: f32,
    easing_table: Option<&[PsbValue]>,
    coordinate_plane: i64,
) {
    // Native per-property curves live on the CURRENT type-3 frame:
    //   ccc = coordinate, acc = angle, zcc = zoom, scc = shear,
    //   occ = color. Opacity uses raw t.  They all resolve through the
    // top-level easing table at MMotionPlayer+844.
    let coord_t = frame_easing(t, current_content.field("ccc"), easing_table);
    let angle_t = frame_easing(t, current_content.field("acc"), easing_table);
    let zoom_t = frame_easing(t, current_content.field("zcc"), easing_table);
    let shear_t = frame_easing(t, current_content.field("scc"), easing_table);

    // sub_103A5190 first applies ccc, then optionally evaluates the authored
    // MBeziersPathEntity (`cp`). MMotionPlayer+848 is only a lazy cache for
    // this PSB object (sub_10337360/sub_1034CB40), not a separate path table.
    let a = state.coord.unwrap_or([0.0; 3]);
    let b = next_state.coord.unwrap_or([0.0; 3]);
    state.coord = Some(interpolate_native_coordinate(
        a,
        b,
        coord_t,
        coordinate_plane,
        current_content.field("cp"),
    ));

    // ox/oy are not part of the recovered StepFrame interpolation block; keep
    // the current frame's values rather than inventing a tween.
    // sub_1032FB00 interpolates the byte opacity as float, then feeds it
    // through sub_103A6180.  Opacity is non-negative, so the native branch is
    // floor(value + 0.5): round to the nearest byte before publishing the
    // runtime frame state rather than retaining a fractional alpha value.
    state.opa = native_round_nonnegative(lerp(state.opa, next_state.opa, t));
    state.scale_x = lerp(state.scale_x, next_state.scale_x, zoom_t);
    state.scale_y = lerp(state.scale_y, next_state.scale_y, zoom_t);
    state.rotation_degrees =
        lerp_angle_degrees(state.rotation_degrees, next_state.rotation_degrees, angle_t);
    state.shear_x = lerp(state.shear_x, next_state.shear_x, shear_t);
    state.shear_y = lerp(state.shear_y, next_state.shear_y, shear_t);
    // fx/fy are copied from the current frame and therefore deliberately not
    // interpolated.

    // sub_10355BF0 direction mode 3 samples the exact same authored
    // coordinate path twice at t and t+0.0001. Near the end it shifts the
    // pair back to [1-0.0001, 1] instead of sampling beyond the keyframe.
    if state.motion_direction_type == 3 {
        let mut tangent_t0 = t;
        let mut tangent_t1 = t + 0.0001;
        if tangent_t1 >= 1.0 {
            tangent_t1 = 1.0;
            tangent_t0 = 1.0 - 0.0001;
        }
        let tangent_coord_t0 =
            frame_easing(tangent_t0, current_content.field("ccc"), easing_table);
        let tangent_coord_t1 =
            frame_easing(tangent_t1, current_content.field("ccc"), easing_table);
        let p0 = interpolate_native_coordinate(
            a,
            b,
            tangent_coord_t0,
            coordinate_plane,
            current_content.field("cp"),
        );
        let p1 = interpolate_native_coordinate(
            a,
            b,
            tangent_coord_t1,
            coordinate_plane,
            current_content.field("cp"),
        );
        let tangent = [p1[0] - p0[0], p1[1] - p0[1], p1[2] - p0[2]];
        state.motion_path_tangent_vector = Some(tangent);
        state.motion_path_tangent_degrees = match coordinate_plane {
            0 => Some(tangent[1].atan2(tangent[0]).to_degrees().rem_euclid(360.0)),
            1 => Some(tangent[2].atan2(tangent[0]).to_degrees().rem_euclid(360.0)),
            _ => None,
        };
    }

    // motion.docmpl asks the type-3 nested-motion pass to complete its
    // direction offset toward the next frame. sub_10355BF0 uses the current
    // frame's ACC curve and shortest-angle branch (v112+136).
    if state.motion_direction_offset_complete && next_state.motion_direction_type != 0 {
        state.motion_direction_offset_degrees = lerp_angle_degrees(
            state.motion_direction_offset_degrees,
            next_state.motion_direction_offset_degrees,
            angle_t,
        );
    }

    // sub_1032FB00 uses OCC only for packed corner colors.  It performs the
    // interpolation in integer byte lanes with weight=floor(t*256), not with
    // floating RGBA. Preserve that exact quantization here.
    let color_t = frame_easing(t, current_content.field("occ"), easing_table);
    if state.single_color && next_state.single_color {
        let c = interpolate_native_packed_color(state.colors[0], next_state.colors[0], color_t);
        state.colors = [c; 4];
    } else {
        for i in 0..4 {
            state.colors[i] = interpolate_native_packed_color(
                state.colors[i],
                next_state.colors[i],
                color_t,
            );
        }
    }
    state.single_color = state.single_color && next_state.single_color;
    state.default_color = state.default_color && next_state.default_color;
    // `bm` and `bp` are frame-step values in the emitted DrawFrameInfo.
    // sub_103390C0 reads them from the current decoded frame (+32/+36), so do
    // not tween them here.

    // sub_1032FB00 specialized StepLayer payloads. Type-4 leaves `trigger`
    // as the current-frame step value and linearly interpolates the remaining
    // nine floats with the raw frame interpolation factor.
    if let (Some(mut a), Some(b)) = (state.particle, next_state.particle) {
        a.fmin = lerp(a.fmin, b.fmin, t);
        a.fmax = lerp(a.fmax, b.fmax, t);
        a.vmin = lerp(a.vmin, b.vmin, t);
        a.vmax = lerp(a.vmax, b.vmax, t);
        a.amin = lerp(a.amin, b.amin, t);
        a.amax = lerp(a.amax, b.amax, t);
        a.zmin = lerp(a.zmin, b.zmin, t);
        a.zmax = lerp(a.zmax, b.zmax, t);
        a.range = lerp(a.range, b.range, t);
        state.particle = Some(a);
    }
    // Type-5 StepLayer interpolates only the first scalar (camera FOV).
    if let (Some(mut a), Some(b)) = (state.camera.clone(), next_state.camera.as_ref()) {
        a.fov = lerp(a.fov, b.fov, t);
        state.camera = Some(a);
    }
    // Type-10 StepLayer interpolates feedback.timespan.
    if let (Some(a), Some(b)) = (state.feedback_timespan, next_state.feedback_timespan) {
        state.feedback_timespan = Some(lerp(a, b, t));
    }
    // Type-12 uses WCC for wrt/wsf and recomputes the native scale/bias.
    if let (Some(a), Some(b)) = (state.stencil_wipe, next_state.stencil_wipe) {
        if a.enabled {
            let wipe_t = frame_easing(t, current_content.field("wcc"), easing_table);
            state.stencil_wipe = Some(StencilWipeFrameState::from_native(
                true,
                a.reverse,
                lerp(a.threshold, b.threshold, wipe_t),
                lerp(a.softness, b.softness, wipe_t),
            ));
        }
    }

    if let (Some(mut a), Some(mut b)) = (
        parse_content_mesh_patch(current_content, 1, 1),
        parse_content_mesh_patch(next_content, 1, 1),
    ) {
        if a.domain.is_none() {
            a.domain = current_content
                .field_str("icon")
                .and_then(parse_mesh_domain_icon);
        }
        if b.domain.is_none() {
            b.domain = next_content
                .field_str("icon")
                .and_then(parse_mesh_domain_icon);
        }
        state.mesh_patch = Some(EmoteMeshPatch::interpolate(&a, &b, t));
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn interpolate_native_packed_color(a: u32, b: u32, t: f32) -> u32 {
    // Exact lane arithmetic from sub_103A4E90. Colors are serialized as
    // 0xRRGGBBAA, so the low byte is alpha.
    if a == b {
        return a;
    }
    let weight = (finite_or(t, 0.0).clamp(0.0, 1.0) * 256.0) as u32;
    let inv = 256u32.saturating_sub(weight);
    let rb_a = a & 0x00FF_00FF;
    let rb_b = b & 0x00FF_00FF;
    let ga_a = (a >> 8) & 0x00FF_00FF;
    let ga_b = (b >> 8) & 0x00FF_00FF;
    let rb = ((weight * rb_b + inv * rb_a) >> 8) & 0x00FF_00FF;
    let ga = (((weight * ga_b + inv * ga_a) >> 8) & 0x00FF_00FF) << 8;
    rb | ga
}

fn native_round_nonnegative(value: f32) -> f32 {
    // sub_103A6180 adds 0.5 on the non-negative branch before the native
    // floor helper.  Frame opacity is uint8-derived, so this is the only
    // branch reached by sub_1032FB00's opacity interpolation.
    (value + 0.5).floor()
}

fn lerp_angle_degrees(a: f32, b: f32, t: f32) -> f32 {
    // Recovered from sub_103A4D10: choose the shortest branch by moving the
    // target by +/-360 when the raw delta exceeds 180 degrees, interpolate,
    // then fold the result back into one revolution.
    let mut target = b;
    let delta = target - a;
    if delta > 180.0 {
        target -= 360.0;
    } else if delta < -180.0 {
        target += 360.0;
    }
    let value = lerp(a, target, t);
    if value.is_finite() {
        value.rem_euclid(360.0)
    } else {
        0.0
    }
}

fn content_coord(content: &PsbValue) -> Option<[f32; 3]> {
    let coord = content.field("coord")?.as_list()?;
    if coord.len() < 3 {
        return None;
    }
    Some([
        coord[0].as_f32().unwrap_or(0.0),
        coord[1].as_f32().unwrap_or(0.0),
        coord[2].as_f32().unwrap_or(0.0),
    ])
}

fn content_scale_x(content: &PsbValue) -> Option<f32> {
    content
        .field_f32("zx")
        .or_else(|| content.field_f32("scale_x"))
        .or_else(|| content.field_f32("scaleX"))
        .or_else(|| content.field_f32("scale"))
        .or_else(|| content.field_f32("zoom"))
}

fn content_scale_y(content: &PsbValue) -> Option<f32> {
    content
        .field_f32("zy")
        .or_else(|| content.field_f32("scale_y"))
        .or_else(|| content.field_f32("scaleY"))
        .or_else(|| content.field_f32("scale"))
        .or_else(|| content.field_f32("zoom"))
}

fn content_shear_x(content: &PsbValue) -> Option<f32> {
    content.field_f32("sx")
}

fn content_shear_y(content: &PsbValue) -> Option<f32> {
    content.field_f32("sy")
}

fn content_bool_like(content: &PsbValue, name: &str) -> Option<bool> {
    match content.field(name)? {
        PsbValue::Bool(value) => Some(*value),
        value => value.as_i64().map(|value| value != 0),
    }
}

fn content_rotation(content: &PsbValue) -> Option<f32> {
    content
        .field_f32("angle")
        .or_else(|| content.field_f32("rot"))
        .or_else(|| content.field_f32("rotation"))
}

fn merge_frame_content(state: &mut DynamicFrameState, content: &PsbValue) {
    if let Some(coord) = content.field("coord").and_then(PsbValue::as_list) {
        if coord.len() >= 3 {
            state.coord = Some([
                coord[0].as_f32().unwrap_or(0.0),
                coord[1].as_f32().unwrap_or(0.0),
                coord[2].as_f32().unwrap_or(0.0),
            ]);
        }
    }
    if let Some(ox) = content.field_f32("ox") {
        state.ox = ox;
    }
    if let Some(oy) = content.field_f32("oy") {
        state.oy = oy;
    }
    if let Some(flip_x) = content_bool_like(content, "fx") {
        state.flip_x = flip_x;
    }
    if let Some(flip_y) = content_bool_like(content, "fy") {
        state.flip_y = flip_y;
    }
    if let Some(scale_x) = content_scale_x(content) {
        state.scale_x = scale_x;
    }
    if let Some(scale_y) = content_scale_y(content) {
        state.scale_y = scale_y;
    }
    if let Some(rotation) = content_rotation(content) {
        state.rotation_degrees = rotation;
    }
    if let Some(shear_x) = content_shear_x(content) {
        state.shear_x = shear_x;
    }
    if let Some(shear_y) = content_shear_y(content) {
        state.shear_y = shear_y;
    }
    if let Some(src) = content.field_str("src") {
        state.src = Some(src.to_owned());
    }
    if let Some(icon) = content.field_str("icon") {
        state.icon = Some(icon.to_owned());
    }
    if let Some(opa) = content.field_f32("opa") {
        state.opa = opa;
    }
    if let Some(bm) = content.field_u32("bm") {
        state.blend_mode = bm;
    }
    if let Some(bp) = content.field_f32("bp") {
        state.blend_parameter = bp;
    }
    if let Some(color) = content.field("color") {
        state.default_color = false;
        if let Some(values) = color.as_list() {
            if values.len() >= 4 {
                state.single_color = false;
                for (dst, value) in state.colors.iter_mut().zip(values.iter().take(4)) {
                    if let Some(value) = value.as_i64() {
                        *dst = value as u32;
                    }
                }
            } else if let Some(value) = values.first().and_then(PsbValue::as_i64) {
                state.single_color = true;
                state.colors = [value as u32; 4];
            }
        } else if let Some(value) = color.as_i64() {
            state.single_color = true;
            state.colors = [value as u32; 4];
        }
    } else if (state.blend_mode & 0xF0) == 0 {
        // sub_1033D0E0: legacy non-MODULATE2X frames without an explicit
        // color use white instead of the normal neutral gray.
        state.colors = [0xFFFF_FFFF; 4];
    }
    if let Some(motion) = content.field("motion") {
        // sub_1033D0E0 initializes every motion payload to
        // flags=0, dt=1, dofst=0, docmpl=false, dtgt="", then reads the
        // fields selected by the serialized mask. Preserve those exact
        // defaults when normalized PSB omits an optional member.
        state.motion_flags = motion.field_u32("flags").unwrap_or(0);
        state.motion_direction_type = motion.field_i64("dt").unwrap_or(1) as i32;
        state.motion_direction_offset_degrees = motion.field_f32("dofst").unwrap_or(0.0);
        state.motion_direction_offset_complete = content_bool_like(motion, "docmpl").unwrap_or(false);
        state.motion_direction_target = motion
            .field_str("dtgt")
            .filter(|value| !value.is_empty())
            .map(str::to_owned);
        state.time_offset_ticks = motion.field_f32("timeOffset").unwrap_or(0.0);
    } else if let Some(time_offset) = content.field_f32("timeOffset") {
        state.time_offset_ticks = time_offset;
    }
    if let Some(prt) = content.field("prt") {
        let mut value = ParticleFrameState::default();
        value.trigger = prt.field_i64("trigger").unwrap_or(0) as i32;
        value.fmin = prt.field_f32("fmin").unwrap_or(10.0);
        value.fmax = prt.field_f32("fmax").unwrap_or(10.0);
        value.vmin = prt.field_f32("vmin").unwrap_or(0.0);
        value.vmax = prt.field_f32("vmax").unwrap_or(0.0);
        value.amin = prt.field_f32("amin").unwrap_or(0.0);
        value.amax = prt.field_f32("amax").unwrap_or(0.0);
        value.zmin = prt.field_f32("zmin").unwrap_or(1.0);
        value.zmax = prt.field_f32("zmax").unwrap_or(1.0);
        value.range = prt.field_f32("range").unwrap_or(0.0);
        state.particle = Some(value);
    }
    if let Some(camera) = content.field("camera") {
        state.camera = Some(CameraFrameState {
            fov: camera.field_f32("fov").unwrap_or(0.0),
            target: camera.field_str("target").unwrap_or("").to_owned(),
        });
    }
    if let Some(model) = content.field("model") {
        state.model = Some(ModelFrameState {
            looped: content_bool_like(model, "loop").unwrap_or(false),
            direction_type: model.field_i64("dt").unwrap_or(0) as i32,
            direction_target: model.field_str("dtgt").unwrap_or("").to_owned(),
            time_offset_ticks: model.field_f32("timeOffset").unwrap_or(0.0),
        });
    }
    if let Some(feedback) = content.field("feedback") {
        state.feedback_timespan = Some(feedback.field_f32("timespan").unwrap_or(0.0));
    }
    if content.field("stc").is_some()
        || content.field("wrv").is_some()
        || content.field("wrt").is_some()
        || content.field("wsf").is_some()
    {
        let enabled = content_bool_like(content, "stc").unwrap_or(true);
        let reverse = content_bool_like(content, "wrv").unwrap_or(false);
        let threshold = content.field_f32("wrt").unwrap_or(0.0);
        let softness = content.field_f32("wsf").unwrap_or(0.0);
        state.stencil_wipe = Some(StencilWipeFrameState::from_native(
            enabled, reverse, threshold, softness,
        ));
    }
    if let Some(anchor) = content.field("anchor") {
        state.anchor_target = Some(anchor.field_str("target").unwrap_or("").to_owned());
    }
    if let Some(mut mesh) = parse_content_mesh_patch(content, 1, 1) {
        if mesh.domain.is_none() {
            mesh.domain = content.field_str("icon").and_then(parse_mesh_domain_icon);
        }
        state.mesh_patch = Some(mesh);
    }
}

fn parse_mesh_domain_icon(icon: &str) -> Option<[f32; 4]> {
    let mut parts = icon.split(':');
    let width = parts.next()?.parse::<f32>().ok()?;
    let height = parts.next()?.parse::<f32>().ok()?;
    let origin_x = parts.next()?.parse::<f32>().ok()?;
    let origin_y = parts.next()?.parse::<f32>().ok()?;
    if parts.next().is_some()
        || ![width, height, origin_x, origin_y].iter().all(|v| v.is_finite())
        || width <= 0.0 || height <= 0.0
    {
        return None;
    }
    // Mesh icons encode width:height:origin_x:origin_y, like ordinary
    // sprite dimensions and origins. The origin need not be at the center.
    Some([-origin_x, -origin_y, width, height])
}

#[derive(Debug, Clone)]
struct LayerParameterEval {
    id: Option<String>,
    value: Option<f32>,
    local_time_ticks: f32,
}

fn layer_parameter_eval(
    layer: &PsbValue,
    parameter_table: Option<&[PsbValue]>,
    variables: &BTreeMap<String, f32>,
    _frame_list: &[PsbValue],
    fallback_time_ticks: f32,
) -> Option<LayerParameterEval> {
    if let Some(parameterize) = layer.field("parameterize") {
        let Some(parameter) = resolve_parameterize(parameterize, parameter_table) else {
            return Some(LayerParameterEval {
                id: None,
                value: None,
                local_time_ticks: 0.0,
            });
        };
        let Some(id) = parameter
            .field_str("id")
            .or_else(|| parameter.field_str("key"))
            .or_else(|| parameter.field_str("name"))
            .filter(|s| !s.is_empty())
        else {
            return Some(LayerParameterEval {
                id: None,
                value: None,
                local_time_ticks: 0.0,
            });
        };

        let Some(value) = variables.get(id).copied() else {
            #[cfg(debug_assertions)]
            {
                let seen = MISSING_PARAMETER_VARIABLES.get_or_init(|| Mutex::new(BTreeSet::new()));
                if let Ok(mut seen) = seen.lock() {
                    if seen.insert(id.to_owned()) {
                        eprintln!("parameterized layer references missing variable '{id}'");
                    }
                }
            }
            return Some(LayerParameterEval {
                id: Some(id.to_owned()),
                value: None,
                local_time_ticks: 0.0,
            });
        };

        let begin = parameter
            .field_f32("rangeBegin")
            .or_else(|| parameter.field_f32("min"))
            .unwrap_or(0.0);
        let end = parameter
            .field_f32("rangeEnd")
            .or_else(|| parameter.field_f32("max"))
            .unwrap_or(1.0);
        let division = parameter.field_f32("division").unwrap_or(1.0);
        if !division.is_finite()
            || division <= 0.0
            || !begin.is_finite()
            || !end.is_finite()
            || (end - begin).abs() <= f32::EPSILON
        {
            return Some(LayerParameterEval {
                id: Some(id.to_owned()),
                value: Some(value),
                local_time_ticks: 0.0,
            });
        }
        // EPParameter::SetValue (sub_10350230): parameter+44 is the value
        // consumed directly as layer sample time by sub_1032FB00. It first
        // truncates the value toward zero when `discretization` (+0x1C) is
        // set, then clamps it with std::min/std::max into
        // [min(begin, end), max(begin, end)]. Authored `division`, not
        // frameList max time, defines the parameter-time domain. The 2017
        // runtime inlines the same steps (0x100327cc).
        let mut sample = value;
        if content_bool_like(parameter, "discretization").unwrap_or(false) {
            sample = sample.trunc();
        }
        let sample = sample.min(begin.max(end)).max(begin.min(end));
        let local_time_ticks = (sample - begin) * division / (end - begin);
        return Some(LayerParameterEval {
            id: Some(id.to_owned()),
            value: Some(value),
            local_time_ticks,
        });
    }

    // sub_1032FB00 reads LayerInfo+12 (parameterize).  Parameterized layers
    // sample parameter.current_value (+44); ordinary layers sample the owning
    // MMotionPlayer effective motion time (+284).
    Some(LayerParameterEval {
        id: None,
        value: None,
        local_time_ticks: fallback_time_ticks,
    })
}

fn resolve_parameterize<'a>(
    parameterize: &'a PsbValue,
    parameter_table: Option<&'a [PsbValue]>,
) -> Option<&'a PsbValue> {
    match parameterize {
        PsbValue::Object(_) => Some(parameterize),
        PsbValue::Int(index) if *index >= 0 => parameter_table?.get(*index as usize),
        _ => None,
    }
}

fn layer_parameter_id(layer: &PsbValue, parameter_table: Option<&[PsbValue]>) -> Option<String> {
    let parameterize = layer.field("parameterize")?;
    let parameter = resolve_parameterize(parameterize, parameter_table)?;
    parameter
        .field_str("id")
        .or_else(|| parameter.field_str("key"))
        .or_else(|| parameter.field_str("name"))
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}


fn combine_patch(base: Option<EmoteMeshPatch>, patch: EmoteMeshPatch) -> Option<EmoteMeshPatch> {
    Some(match base {
        Some(prev) => prev.combined_with(&patch),
        None => patch,
    })
}

#[derive(Debug, Clone)]
struct MeshCombinatorEval {
    self_patch: Option<EmoteMeshPatch>,
    child_patch: Option<EmoteMeshPatch>,
}

fn evaluate_mesh_combinator_split(
    layer: &PsbValue,
    parameter_table: Option<&[PsbValue]>,
    psb: &PsbFile,
    psb_data: Option<&[u8]>,
    variables: &BTreeMap<String, f32>,
    division_x: u32,
    division_y: u32,
) -> Option<MeshCombinatorEval> {
    let data = psb_data?;
    let combinators = layer
        .field("meshCombinator")?
        .field("combinatorList")?
        .as_list()?;
    if combinators.is_empty() {
        return None;
    }
    let layer_domain = layer_mesh_domain(layer);

    let first_key = combinators
        .first()
        .and_then(|c| c.field("variable"))
        .and_then(|v| v.field_str("key"));
    let parent_key = layer
        .field("parameterize")
        .and_then(|p| resolve_parameterize(p, parameter_table))
        .and_then(|p| {
            p.field_str("id")
                .or_else(|| p.field_str("key"))
                .or_else(|| p.field_str("name"))
        });
    let restore_first_to_parent = match (parent_key, first_key) {
        (None, _) => true,
        (Some("param"), _) => true,
        (Some(parent), Some(first)) => parent == first,
        (Some(_), None) => false,
    };

    let mut self_patch = None;
    let mut child_patch = None;

    let mut start_index = 0usize;
    if restore_first_to_parent {
        if let Some(first) = combinators.first() {
            if let Some(patch) =
                evaluate_one_combinator(first, psb, data, variables, division_x, division_y, false)
            {
                self_patch = Some(patch_with_domain(patch, layer_domain));
                start_index = 1;
            }
        }
    }

    for combinator in combinators.iter().skip(start_index) {
        if let Some(patch) = evaluate_one_combinator(
            combinator, psb, data, variables, division_x, division_y, true,
        ) {
            child_patch = combine_patch(child_patch, patch_with_domain(patch, layer_domain));
        }
    }

    if self_patch.is_none() && child_patch.is_none() {
        None
    } else {
        Some(MeshCombinatorEval {
            self_patch,
            child_patch,
        })
    }
}

fn layer_mesh_domain(layer: &PsbValue) -> Option<[f32; 4]> {
    layer
        .field("frameList")
        .and_then(PsbValue::as_list)?
        .iter()
        .filter_map(|frame| frame.field("content"))
        .filter_map(|content| content.field_str("icon"))
        .find_map(parse_mesh_domain_icon)
}

fn patch_with_domain(mut patch: EmoteMeshPatch, domain: Option<[f32; 4]>) -> EmoteMeshPatch {
    if patch.domain.is_none() {
        patch.domain = domain;
    }
    patch
}

fn frame_runtime_state_key(motion_name: &str, path: &str) -> String {
    // Paths can be identical across top-level motions. Native switching to a
    // different MMotionPlayer does not inherit the prior motion's type-0 HOLD
    // state, so scope the persistent snapshot by motion identity as well.
    format!("{motion_name}\0{path}")
}

fn travel_layer_at(
    value: &PsbValue,
    sibling_index: usize,
    object_table: &PsbValue,
    parameter_table: Option<&[PsbValue]>,
    psb: &PsbFile,
    psb_data: Option<&[u8]>,
    variables: &BTreeMap<String, f32>,
    textures: &BTreeMap<String, EmoteTextureSource>,
    motion_name: &str,
    time_ticks: f32,
    mut ctx: TravelContext,
    previous_positions: Option<&BTreeMap<String, [f32; 3]>>,
    previous_frame_states: Option<&BTreeMap<String, DynamicFrameState>>,
    frame_runtime_states: &mut BTreeMap<String, DynamicFrameState>,
    join_reuse: &mut JoinReusePool,
    pending_nested: &mut Vec<PendingNestedMotion>,
    pending_anchors: &mut Vec<PendingAnchor>,
    out: &mut Vec<EmoteStaticSprite>,
    layer_states: &mut Vec<EmoteStepFrameLayerState>,
    mask_owners: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), EmoteSchemaError> {
    let Some(_) = value.as_object() else {
        return Ok(());
    };
    let layer = value;

    ctx = enter_layer_context(ctx, &layer, sibling_index);
    // sub_1034D900/sub_103A20E0: every joinTarget layer consumes the next
    // compatible prior-layer record by type. Same-path StepFrame history wins
    // during ordinary progression, but consuming the record here preserves the
    // native traversal pairing when only some paths survive a motion switch.
    let join_seed = if ctx.join_target && native_join_target_type(ctx.layer_type) {
        let seed = join_reuse.take_frame(ctx.layer_type);
        if ctx.layer_type == 4 {
            join_reuse.bind_particle_emitter(&ctx.path);
        }
        seed
    } else {
        None
    };
    // sub_103390C0 second pass: a layer owns a composite-mask reference when
    // (layerInfo+720) & 4 is set AND the layer carries a non-empty source
    // list at (layerInfo+740)+8.  Record the owner path so descendants whose
    // `parent_mask_path` points here can resolve the mask reference.
    if ctx.layer_type == 12
        && (ctx.stencil_type & 0x4) != 0
        && !ctx.stencil_composite_mask_layer_list.is_empty()
    {
        mask_owners
            .entry(ctx.path.clone())
            .or_insert_with(|| ctx.stencil_composite_mask_layer_list.clone());
    }
    let mesh_combinator = evaluate_mesh_combinator_split(
        &layer,
        parameter_table,
        psb,
        psb_data,
        variables,
        ctx.mesh_division_x,
        ctx.mesh_division_y,
    );
    let mut local_mesh_patch = None;
    let mut draw_ctx = ctx.clone();
    let mut child_ctx = ctx.clone();
    let sync_child_shape = (ctx.mesh_sync_child & 0x8) != 0;
    let layer_mesh_parameter = if sync_child_shape {
        layer_parameter_id(&layer, parameter_table)
    } else {
        None
    };
    // Descendants point back at a composite-mask owner (stencilType & 4 layer
    // with a non-empty stencilCompositeMaskLayerList).  Renderer side keys
    // alpha-mask textures by this same owner path; the linkage matches
    // sub_103390C0 second pass owner check at `(layerInfo+720) & 4`.
    let is_composite_mask_owner =
        (ctx.stencil_type & 0x4) != 0 && !ctx.stencil_composite_mask_layer_list.is_empty();
    if is_composite_mask_owner {
        child_ctx.parent_mask_path = Some(ctx.path.clone());
    }
    if let Some(mesh_combinator) = mesh_combinator {
        if let Some(patch) = mesh_combinator.self_patch {
            local_mesh_patch = combine_patch(local_mesh_patch, patch);
            draw_ctx.mesh_patch = combine_patch(draw_ctx.mesh_patch.take(), patch);
            if sync_child_shape {
                child_ctx.mesh_patch = combine_patch(child_ctx.mesh_patch.take(), patch);
                if let Some(id) = &layer_mesh_parameter {
                    Arc::make_mut(&mut child_ctx.mesh_parameters).insert(id.clone());
                }
            }
        }
        if sync_child_shape {
            if let Some(patch) = mesh_combinator.child_patch {
                child_ctx.mesh_patch = combine_patch(child_ctx.mesh_patch.take(), patch);
                if let Some(id) = &layer_mesh_parameter {
                    Arc::make_mut(&mut child_ctx.mesh_parameters).insert(id.clone());
                }
            }
        }
    }

    let label = layer.field_str("label").map(str::to_owned);
    let mut active_external_motion = false;
    let mut specialized_frame = None;
    let mut particle_triggered = false;
    if let Some(frame_list) = layer.field("frameList").and_then(PsbValue::as_list) {
        let param_eval =
            layer_parameter_eval(&layer, parameter_table, variables, frame_list, time_ticks)
                .unwrap_or(LayerParameterEval {
                    id: None,
                    value: None,
                    local_time_ticks: 0.0,
                });
        draw_ctx.control_parameter = param_eval.id.clone();
        draw_ctx.control_value = param_eval.value;
        draw_ctx.local_time_ticks = Some(param_eval.local_time_ticks);
        child_ctx.control_parameter = param_eval.id.clone();
        child_ctx.control_value = param_eval.value;
        child_ctx.local_time_ticks = Some(param_eval.local_time_ticks);
        let local_time = param_eval.local_time_ticks;
        let easing_table = psb.root.field("easing").and_then(PsbValue::as_list);
        let frame_runtime_key = frame_runtime_state_key(motion_name, &draw_ctx.path);
        let previous_local_state = previous_frame_states
            .and_then(|states| states.get(&frame_runtime_key))
            .or(join_seed.as_ref());
        let mut state = evaluate_frame_list(
            frame_list,
            local_time,
            easing_table,
            draw_ctx.coordinate.unwrap_or(0),
            previous_local_state,
        );
        // layerInfo+36 is the native StepFrame dirty/trigger byte consumed by
        // trigger-mode particles. sub_1032E470/sub_10343E30/sub_10353A10 set
        // it when the active serialized frame buffer advances or seeks, while
        // sub_10331060 clears it after StepFrame. Do NOT compare the complete
        // decoded state here: interpolation changes that state every tick and
        // would incorrectly retrigger a trigger-mode emitter every frame.
        particle_triggered = serialized_frame_transition_dirty(previous_local_state, &state);
        // Persist the decoded LOCAL frame state before parent meshSync and
        // inheritance mutate the current composite StepFrame. This is the
        // state sub_1032FB00 leaves untouched when the next serialized frame
        // has native type 0.
        frame_runtime_states.insert(frame_runtime_key, state.clone());
        if let Some(sync) = draw_ctx.inherit_source.mesh_sync {
            apply_mesh_sync_child_state(&mut state, sync, draw_ctx.inherit_mask.unwrap_or(0));
        }
        // Specialized passes consume the same composite decoded frame that
        // StepFrame leaves on the layer. Keep it attached until those passes
        // have run; this is not part of the persistent type-0 HOLD snapshot.
        specialized_frame = Some(state.clone());
        draw_ctx.frame_index = state.frame_index;
        draw_ctx.next_frame_index = state.next_frame_index;
        draw_ctx.frame_offset = [state.ox, state.oy];
        draw_ctx.interpolation_t = state.interpolation_t;
        child_ctx.frame_index = state.frame_index;
        child_ctx.next_frame_index = state.next_frame_index;
        child_ctx.frame_offset = [state.ox, state.oy];
        child_ctx.interpolation_t = state.interpolation_t;
        if let Some(mesh) = state.mesh_patch.take() {
            let mesh = EmoteMeshPatch {
                division_x: ctx.mesh_division_x.max(mesh.division_x),
                division_y: ctx.mesh_division_y.max(mesh.division_y),
                domain: mesh.domain,
                control_points: mesh.control_points,
            };
            local_mesh_patch = combine_patch(local_mesh_patch, mesh);
            draw_ctx.mesh_patch = combine_patch(draw_ctx.mesh_patch.take(), mesh);
            if sync_child_shape {
                child_ctx.mesh_patch = combine_patch(child_ctx.mesh_patch.take(), mesh);
                if let Some(id) = &layer_mesh_parameter {
                    Arc::make_mut(&mut child_ctx.mesh_parameters).insert(id.clone());
                }
            }
        }
        draw_ctx = apply_layer_transform(
            draw_ctx,
            state.coord,
            state.flip_x,
            state.flip_y,
            state.scale_x,
            state.scale_y,
            state.rotation_degrees,
            state.shear_x,
            state.shear_y,
        );
        child_ctx = apply_layer_transform(
            child_ctx,
            state.coord,
            state.flip_x,
            state.flip_y,
            state.scale_x,
            state.scale_y,
            state.rotation_degrees,
            state.shear_x,
            state.shear_y,
        );
        draw_ctx = ctx_with_opacity(draw_ctx, state.opa);
        child_ctx = ctx_with_opacity(child_ctx, state.opa);

        if draw_ctx.layer_type == 12 {
            if let Some(wipe) = state.stencil_wipe {
                draw_ctx.stencil_wipe_enabled = wipe.enabled;
                draw_ctx.stencil_wipe_reverse = wipe.reverse;
                draw_ctx.stencil_wipe_scale = wipe.scale;
                draw_ctx.stencil_wipe_bias = wipe.bias;
                child_ctx.stencil_wipe_enabled = wipe.enabled;
                child_ctx.stencil_wipe_reverse = wipe.reverse;
                child_ctx.stencil_wipe_scale = wipe.scale;
                child_ctx.stencil_wipe_bias = wipe.bias;
            }
        }

        // Native type-9 Anchor is a post-StepFrame specialized pass
        // (sub_10351120). Collect the authored constraint now, but do not
        // apply it until every base layer in this MMotionPlayer has finalized
        // its +612 XYZ position.
        if draw_ctx.layer_type == 9 {
            if let Some(target) = state.anchor_target.clone() {
                pending_anchors.push(PendingAnchor {
                    path: draw_ctx.path.clone(),
                    mode: layer.field_i64("anchor").unwrap_or(0) as i32,
                    target,
                    flip_x: draw_ctx.linear_state.flip_x,
                    flip_y: draw_ctx.linear_state.flip_y,
                });
            }
        }

        // sub_10335500 is evaluated from the same ancestor chosen by the
        // inheritParent walk.  Store this layer's mesh sync on the inheritance
        // source; transparent (0x400000) layers deliberately keep the upstream
        // source and therefore do not replace it.
        let current_mesh_sync = if ctx.mesh_transform == 1 && (ctx.mesh_sync_child & 0x7) != 0 {
            draw_ctx.mesh_patch.map(|patch| MeshSyncChildState {
                patch,
                mask: ctx.mesh_sync_child & 0x7,
                coordinate: ctx.coordinate,
            })
        } else {
            None
        };
        prepare_child_inherit_source(&mut child_ctx, current_mesh_sync);


        if let Some(src) = state.src.as_deref().filter(|src| !src.is_empty()) {
            let visible = draw_ctx.ready_to_draw
                && draw_ctx.base_visible
                && draw_ctx.opacity_multiplier > 0.0;
            let visible_child_ctx = ctx_with_visible(child_ctx.clone(), visible);
            if native_nested_motion_layer_type(draw_ctx.layer_type) {
                if let Some(rest) = src.strip_prefix("motion/") {
                    if visible_child_ctx.base_location.is_some() {
                        let mut parts = rest.split('/').filter(|s| !s.is_empty());
                        if let (Some(object_name), Some(child_motion_name)) =
                            (parts.next(), parts.next())
                        {
                            pending_nested.push(PendingNestedMotion {
                                layer: layer.clone(),
                                object_name: object_name.to_owned(),
                                motion_name: child_motion_name.to_owned(),
                                parent_local_time: local_time,
                                state: state.clone(),
                                ctx: visible_child_ctx.clone(),
                            });
                            active_external_motion = true;
                        }
                    }
                } else if let Some(icon_name) = state.icon.as_deref() {
                    if object_table.field(src).is_some() {
                        pending_nested.push(PendingNestedMotion {
                            layer: layer.clone(),
                            object_name: src.to_owned(),
                            motion_name: icon_name.to_owned(),
                            parent_local_time: local_time,
                            state: state.clone(),
                            ctx: visible_child_ctx.clone(),
                        });
                        active_external_motion = true;
                    }
                }
            } else if native_color_drawable_layer_type(draw_ctx.layer_type) {
                if let Some(icon_name) = state.icon.as_deref() {
                    if textures.contains_key(src) {
                        if let Some(base) = draw_ctx.base_location {
                            if let Some(sprite) = build_sprite(
                                textures,
                                src,
                                icon_name,
                                label.clone(),
                                motion_name,
                                base,
                                state.ox,
                                state.oy,
                                1.0,
                                1.0,
                                0.0,
                                visible,
                                255.0,
                                state.blend_mode,
                                state.blend_parameter,
                                state.colors,
                                draw_ctx.clone(),
                            ) {
                                out.push(sprite);
                            }
                        }
                    }
                }
            }
        }
    }

    // Layers without frameList still participate in the native parent walk.
    // Their current transform/location are inherited from the incoming context,
    // but their own coordinate flag becomes relevant once they are selected as
    // the source for descendants.
    if layer.field("frameList").and_then(PsbValue::as_list).is_none() {
        prepare_child_inherit_source(&mut child_ctx, None);
    }

    let mut layer_state = layer_state_from_ctx(label, &child_ctx);
    layer_state.specialized_frame = specialized_frame;
    if child_ctx.layer_type == 1 {
        layer_state.shape_kind = layer.field_i64("shape").unwrap_or(0) as i32;
    }
    layer_state.particle_static = parse_particle_static_config(&layer);
    layer_state.particle_triggered = particle_triggered;
    layer_state.screen_bounds = parse_screen_bounds(&layer);
    if let Some(feedback_sprite) = build_feedback_history_sprite(&layer_state, motion_name) {
        out.push(feedback_sprite);
    }
    layer_states.push(layer_state);

    // sub_10353CF0 layerInfo+704 is the native "active mesh-chain node"
    // bit.  Its hard structural requirements include:
    //   meshTransform != 0, a live type-1 mesh object, and
    //   (meshSyncChildMask & 8) != 0.
    // The type-1 mesh object at (layerInfo+700)+8 is only allocated for
    // meshTransform == 1 (sub_1033ED90), so in the Rust representation the
    // structural equivalent is exactly meshTransform==1 + shape-sync bit 8 +
    // a decodable patch/domain. Previously every meshTransform==1 layer was
    // appended, which made non-shape-sync ancestors warp descendants that the
    // native layerInfo+704 chain would skip.
    let native_mesh_chain_active =
        ctx.mesh_transform == 1 && (ctx.mesh_sync_child & 0x8) != 0;
    let native_mesh_chain_patch = if native_mesh_chain_active {
        // The chain already contains ancestor patches. Adding the cumulative
        // drawable patch here would apply those deformations a second time.
        local_mesh_patch.filter(|patch| patch.domain.is_some()).map(|patch| MeshChainEntry {
            patch,
            transform: draw_ctx.transform.as_array(),
        })
    } else {
        None
    };
    advance_native_mesh_combine_chain(
        &mut child_ctx.mesh_chain,
        &mut child_ctx.mesh_combine_candidate_start,
        ctx.mesh_combine_candidate_start,
        native_mesh_chain_patch,
        native_mesh_chain_active,
        ctx.mesh_combine,
    );

    if active_external_motion {
        return Ok(());
    }

    if let Some(children) = layer.field("children").and_then(PsbValue::as_list) {
        for (index, child) in children.iter().enumerate() {
            let mut next_ctx = child_ctx.clone();
            next_ctx.draw_index = out.len() + index;
            travel_layer_at(
                child,
                index,
                object_table,
                parameter_table,
                psb,
                psb_data,
                variables,
                textures,
                motion_name,
                time_ticks,
                next_ctx,
                previous_positions,
                previous_frame_states,
                frame_runtime_states,
                join_reuse,
                pending_nested,
                pending_anchors,
                out,
                layer_states,
                mask_owners,
            )?;
        }
    }
    if let Some(children) = layer.field("layer").and_then(PsbValue::as_list) {
        for (index, child) in children.iter().enumerate() {
            let mut next_ctx = child_ctx.clone();
            next_ctx.draw_index = out.len() + index;
            travel_layer_at(
                child,
                index,
                object_table,
                parameter_table,
                psb,
                psb_data,
                variables,
                textures,
                motion_name,
                time_ticks,
                next_ctx,
                previous_positions,
                previous_frame_states,
                frame_runtime_states,
                join_reuse,
                pending_nested,
                pending_anchors,
                out,
                layer_states,
                mask_owners,
            )?;
        }
    }

    Ok(())
}

fn nested_motion_direction_degrees(
    state: &DynamicFrameState,
    composite_angle_degrees: f32,
    coordinate_plane: Option<i64>,
    displacement: Option<[f32; 3]>,
    current_position: Option<[f32; 3]>,
    target_position: Option<[f32; 3]>,
) -> Option<f32> {
    match state.motion_direction_type {
        // sub_10355BF0 case 1: ordinary composite angle + authored offset.
        1 => Some(
            (composite_angle_degrees + state.motion_direction_offset_degrees).rem_euclid(360.0),
        ),
        // case 2 reads layerInfo+108 after the base StepFrame pass.  At the
        // start of sub_10331060 this slot receives the previous finalized XYZ;
        // after all layers are evaluated it is replaced by current - previous.
        // The native switch supports the same two coordinate planes as the
        // parent transform: XY (0) and XZ (1).
        2 => {
            let d = displacement.unwrap_or([0.0; 3]);
            let direction = match coordinate_plane.unwrap_or(0) {
                0 => d[1].atan2(d[0]).to_degrees(),
                1 => d[2].atan2(d[0]).to_degrees(),
                _ => return None,
            };
            Some((direction + state.motion_direction_offset_degrees).rem_euclid(360.0))
        }
        // case 3: local authored coordinate-path tangent + offset.
        3 => state.motion_path_tangent_degrees.map(|angle| {
            (angle + state.motion_direction_offset_degrees).rem_euclid(360.0)
        }),
        // case 4 looks up dtgt in the same MMotionPlayer after all base
        // StepFrame positions have finalized, then points from this layer's
        // finalized position to the target on the active coordinate plane.
        4 => {
            let current = current_position?;
            let target = target_position?;
            let direction = match coordinate_plane.unwrap_or(0) {
                0 => (target[1] - current[1])
                    .atan2(target[0] - current[0])
                    .to_degrees(),
                1 => (target[2] - current[2])
                    .atan2(target[0] - current[0])
                    .to_degrees(),
                _ => return None,
            };
            Some((direction + state.motion_direction_offset_degrees).rem_euclid(360.0))
        }
        _ => None,
    }
}

fn apply_nested_motion_direction_resolved(
    ctx: &mut TravelContext,
    state: &DynamicFrameState,
    previous_positions: Option<&BTreeMap<String, [f32; 3]>>,
    target_position: Option<[f32; 3]>,
) {
    let base_angle = ctx.linear_state.rotation_degrees;
    let displacement = previous_positions.map(|positions| {
        let previous = positions.get(&ctx.path).copied().unwrap_or_else(|| {
            // MMotionPlayer reset (`player+273`) zeroes layer+108. A missing
            // previous entry is the Rust scene-builder equivalent of that
            // first/reset frame.
            ctx.base_location.unwrap_or([0.0; 3])
        });
        let current = ctx.base_location.unwrap_or([0.0; 3]);
        [
            current[0] - previous[0],
            current[1] - previous[1],
            current[2] - previous[2],
        ]
    });
    let Some(target_angle) = nested_motion_direction_degrees(
        state,
        base_angle,
        ctx.coordinate,
        displacement,
        ctx.base_location,
        target_position,
    ) else {
        return;
    };
    if target_angle == base_angle {
        return;
    }

    // sub_10355BF0 does NOT rebuild the enclosing layer transform with the new
    // angle. It post-multiplies the already-composed layer matrix by the angle
    // difference, and reverses that delta when exactly one flip is active.
    // This distinction matters when transformOrder interleaves scale/shear and
    // rotation: changing the layer angle earlier is not algebraically equal.
    let mut delta = target_angle - base_angle;
    if ctx.linear_state.flip_x != ctx.linear_state.flip_y {
        delta = -delta;
    }
    ctx.transform = ctx.transform.then(EmoteTransform2D::rotation(delta));
    ctx.linear_state.rotation_degrees = target_angle;
}

fn apply_nested_motion_direction(ctx: &mut TravelContext, state: &DynamicFrameState) {
    apply_nested_motion_direction_resolved(ctx, state, None, None);
}

fn nested_motion_local_time(parent_local_time: f32, state: &DynamicFrameState) -> f32 {
    // sub_10355BF0: child_time = parent_time - currentFrame.time +
    // motion.timeOffset. recurse_motion_at performs the child player's own
    // loop wrapping afterwards. This is intentionally not global_time+offset.
    finite_or(parent_local_time - state.frame_start_ticks + state.time_offset_ticks, 0.0)
}

fn apply_motion_layer_inherit(
    layer: &PsbValue,
    object_table: &PsbValue,
    object_name: &str,
    motion_name: &str,
    ctx: &mut TravelContext,
) {
    // Nested type-3 layers own a full MMotionPlayer.  sub_1033ED90 reads
    // motionIndependentLayerInherit with a zero default, stores it at +756,
    // and copies the enclosing layer's coordinate/transformOrder into layer 0.
    // In the flattened Rust traversal, the enclosing layer's current
    // composite state is therefore the nested player's synthetic root.
    ctx.motion_independent_layer_inherit = layer
        .field_i64("motionIndependentLayerInherit")
        .unwrap_or(0)
        != 0;
    let root = current_context_as_inherit_source(ctx);
    ctx.motion_root = root;
    ctx.inherit_source = root;

    if ctx.motion_independent_layer_inherit {
        return;
    }
    let Some(target_parameters) = target_motion_parameter_ids(object_table, object_name, motion_name)
    else {
        ctx.mesh_patch = None;
        ctx.mesh_parameters = Arc::default();
        return;
    };
    let shared_parameter_count = target_parameters
        .iter()
        .filter(|parameter| ctx.mesh_parameters.contains(*parameter))
        .count();
    if shared_parameter_count < 4 {
        ctx.mesh_patch = None;
        ctx.mesh_parameters = Arc::default();
    }
}

fn native_anchor_mode_after_flip(mut mode: i32, flip_x: bool, flip_y: bool) -> i32 {
    // sub_10351120 +0x270..+0x2CE. Preserve the DLL literally, including the
    // seemingly asymmetric X-flip mapping 2 -> 3; do not "fix" it to a
    // geometric 0 <-> 2 swap without evidence from a different SDK build.
    if flip_x {
        if mode == 0 {
            mode = 2;
        } else if mode == 2 {
            mode = 3;
        }
    }
    if flip_y {
        if mode == 3 {
            mode = 5;
        } else if mode == 5 {
            mode = 3;
        }
    }
    mode
}

fn find_scope_layer_position_with_root_fallback(
    states: &[EmoteStepFrameLayerState],
    scope_start: usize,
    target: &str,
) -> Option<[f32; 3]> {
    let scope = states.get(scope_start..)?;
    scope
        .iter()
        .find(|state| {
            state.draw_frame_info.layer_label.as_deref() == Some(target) || state.path == target
        })
        .or_else(|| scope.first())
        .map(|state| state.raw_position)
}

fn anchor_axis_shift(
    low_active: bool,
    low_shift: f32,
    exact_active: bool,
    exact_shift: f32,
    high_active: bool,
    high_shift: f32,
) -> f32 {
    // sub_10351120 chooses exact alignment first, then the positive-side
    // constraint (modes 2/5/8), then the negative-side constraint (0/3/6).
    if exact_active {
        exact_shift
    } else if high_active {
        high_shift
    } else if low_active {
        low_shift
    } else {
        0.0
    }
}

fn apply_ground_correction_specialized_pass(
    hook: Option<EmoteGroundCorrectionHook>,
    previous_positions: Option<&BTreeMap<String, [f32; 3]>>,
    scope_start: usize,
    scope_end: usize,
    sprite_scope_start: usize,
    sprites: &mut [EmoteStaticSprite],
    layer_states: &mut [EmoteStepFrameLayerState],
    pending_nested: &mut [PendingNestedMotion],
) {
    let Some(hook) = hook else { return; };
    let end = scope_end.min(layer_states.len());
    let begin = scope_start.min(end);
    for index in begin..end {
        if !layer_states[index].draw_frame_info.ground_correction {
            continue;
        }
        let path = layer_states[index].path.clone();
        let parent_path = path.rsplit_once('/').map(|(parent, _)| parent.to_owned());
        let parent_raw = parent_path
            .as_deref()
            .and_then(|parent| layer_states[begin..end].iter().find(|state| state.path == parent))
            .map(|state| state.raw_position)
            .unwrap_or([0.0; 3]);
        let raw = layer_states[index].raw_position;
        let previous = previous_positions
            .and_then(|positions| positions.get(&path).copied())
            .unwrap_or(raw);
        let request = EmoteGroundCorrectionRequest {
            parent_path,
            layer_path: path.clone(),
            parent_raw_position: parent_raw,
            raw_position: raw,
            previous_delta: [raw[0] - previous[0], raw[1] - previous[1], raw[2] - previous[2]],
            coordinate: layer_states[index].draw_frame_info.coordinate,
        };
        let Some(corrected) = hook(&request) else { continue; };
        if corrected.iter().any(|value| !value.is_finite()) {
            continue;
        }
        let delta = [corrected[0] - raw[0], corrected[1] - raw[1], corrected[2] - raw[2]];
        if delta == [0.0; 3] {
            continue;
        }
        let descendant_prefix = format!("{path}/");
        for state in layer_states[index..end].iter_mut() {
            if state.path == path || state.path.starts_with(&descendant_prefix) {
                for axis in 0..3 {
                    state.raw_position[axis] += delta[axis];
                    state.position[axis] += delta[axis];
                }
                state.transform[4] += delta[0];
                state.transform[5] += delta[1];
            }
        }
        for sprite in sprites.iter_mut().skip(sprite_scope_start) {
            let sprite_path = sprite.draw_frame_info.path.as_str();
            if sprite_path == path || sprite_path.starts_with(&descendant_prefix) {
                sprite.world_transform[4] += delta[0];
                sprite.world_transform[5] += delta[1];
                sprite.z += delta[2];
            }
        }
        for nested in pending_nested.iter_mut() {
            if nested.ctx.path == path || nested.ctx.path.starts_with(&descendant_prefix) {
                if let Some(base) = nested.ctx.base_location.as_mut() {
                    for axis in 0..3 { base[axis] += delta[axis]; }
                }
                nested.ctx.transform.tx += delta[0];
                nested.ctx.transform.ty += delta[1];
            }
        }
    }
}

fn apply_anchor_specialized_pass(
    anchors: &[PendingAnchor],
    scope_start: usize,
    sprite_scope_start: usize,
    sprites: &mut [EmoteStaticSprite],
    layer_states: &mut [EmoteStepFrameLayerState],
    pending_nested: &mut [PendingNestedMotion],
) -> [f32; 3] {
    // Native ordering: sub_10331060 finalizes every base StepFrame, then
    // sub_10351120 evaluates all type-9 anchors and finally shifts every
    // non-root layer in this MMotionPlayer by one aggregate XYZ offset.
    if anchors.is_empty() || scope_start >= layer_states.len() {
        return [0.0; 3];
    }

    let mut low_active = [false; 3];
    let mut exact_active = [false; 3];
    let mut high_active = [false; 3];
    let mut low_shift = [f32::INFINITY; 3];
    let mut exact_shift = [0.0; 3];
    let mut high_shift = [f32::NEG_INFINITY; 3];

    for anchor in anchors {
        let Some(current) = layer_states[scope_start..]
            .iter()
            .find(|state| state.path == anchor.path)
            .map(|state| state.raw_position)
        else {
            continue;
        };
        let Some(target) = find_scope_layer_position_with_root_fallback(
            layer_states,
            scope_start,
            &anchor.target,
        ) else {
            continue;
        };
        let mode = native_anchor_mode_after_flip(anchor.mode, anchor.flip_x, anchor.flip_y);
        if !(0..=8).contains(&mode) {
            continue;
        }
        let axis = (mode / 3) as usize;
        let kind = mode % 3;
        match kind {
            0 => {
                if current[axis] > target[axis] {
                    low_active[axis] = true;
                    // v44/v41/v38 start at +FLT_MAX and are updated through
                    // sub_101AA02D -> std::min. Candidate is target-current.
                    low_shift[axis] = low_shift[axis].min(target[axis] - current[axis]);
                }
            }
            1 => {
                exact_active[axis] = true;
                // The native loop overwrites this for every exact anchor, so
                // the last exact constraint wins rather than min/max blending.
                exact_shift[axis] = target[axis] - current[axis];
            }
            2 => {
                if target[axis] > current[axis] {
                    high_active[axis] = true;
                    // v42/v39/v36 start at -FLT_MAX and are updated through
                    // sub_101A8D7C -> std::max. Candidate is target-current.
                    high_shift[axis] = high_shift[axis].max(target[axis] - current[axis]);
                }
            }
            _ => unreachable!(),
        }
    }

    let shift = [
        anchor_axis_shift(
            low_active[0], low_shift[0], exact_active[0], exact_shift[0], high_active[0], high_shift[0],
        ),
        anchor_axis_shift(
            low_active[1], low_shift[1], exact_active[1], exact_shift[1], high_active[1], high_shift[1],
        ),
        anchor_axis_shift(
            low_active[2], low_shift[2], exact_active[2], exact_shift[2], high_active[2], high_shift[2],
        ),
    ];
    if shift == [0.0; 3] {
        return shift;
    }

    // sub_10351120 starts at layer index 1: layer 0 is the MMotionPlayer root
    // and deliberately remains fixed. Our scope is traversal-ordered, matching
    // the flattened native layer order used elsewhere in this port.
    for state in layer_states.iter_mut().skip(scope_start.saturating_add(1)) {
        state.raw_position[0] += shift[0];
        state.raw_position[1] += shift[1];
        state.raw_position[2] += shift[2];
        // MeshChain runs immediately after Anchor; keep the pre-pass value in
        // sync until `apply_mesh_chain_specialized_pass` overwrites it.
        state.position = state.raw_position;
        state.transform[4] += shift[0];
        state.transform[5] += shift[1];
    }

    let root_path = layer_states
        .get(scope_start)
        .map(|state| state.path.clone());
    for sprite in sprites.iter_mut().skip(sprite_scope_start) {
        if root_path.as_deref() == Some(sprite.draw_frame_info.path.as_str()) {
            continue;
        }
        sprite.world_transform[4] += shift[0];
        sprite.world_transform[5] += shift[1];
        sprite.z += shift[2];
    }

    // Nested-motion layers are launched after Anchor in the native specialized
    // pass order. Their synthetic child root must therefore see the shifted
    // parent origin/matrix.
    for pending in pending_nested {
        if let Some(base) = pending.ctx.base_location.as_mut() {
            base[0] += shift[0];
            base[1] += shift[1];
            base[2] += shift[2];
        }
        pending.ctx.transform.tx += shift[0];
        pending.ctx.transform.ty += shift[1];
    }

    shift
}


fn advance_native_mesh_combine_chain(
    chain: &mut Arc<Vec<MeshChainEntry>>,
    child_candidate_start: &mut usize,
    current_candidate_start: usize,
    current_patch: Option<MeshChainEntry>,
    current_active: bool,
    current_mesh_combine: bool,
) {
    // MMotionPlayer::StepFrameMeshChain (sub_10353CF0), layerInfo+706/+708:
    //
    // * +708 initially points at the nearest parent that is an active mesh
    //   node (+704) or carries an inherited mesh chain (+705), otherwise at
    //   that parent's +708.
    // * for an active current node with meshCombine(+706), native walks the
    //   *real parent chain*, combines every active mesh encountered, and stops
    //   immediately after the first parent whose meshCombine flag is false;
    //   each combined active ancestor is simultaneously removed from +708.
    //
    // `current_candidate_start` is the flattened equivalent of that parent
    // walk: the suffix of `chain` that can be folded into this node. Because
    // combineMesh is pointwise displacement addition, replacing that suffix by
    // one combined patch is algebraically identical to native +708 collapse
    // and prevents descendants from applying the same ancestors twice.
    let start = current_candidate_start.min(chain.len());
    let mut appended_current = false;

    if current_active {
        if let Some(mut patch) = current_patch {
            if current_mesh_combine {
                // Displacement addition is valid only in the same coordinate
                // frame and domain. Keep other ancestors as separate mappings.
                let combine_start = chain.iter().enumerate().skip(start)
                    .rfind(|(_, ancestor)| ancestor.transform != patch.transform
                        || ancestor.patch.domain != patch.patch.domain)
                    .map_or(start, |(index, _)| index + 1);
                for ancestor in chain.iter().skip(combine_start) {
                    patch.patch = patch.patch.combined_with(&ancestor.patch);
                }
                Arc::make_mut(chain).truncate(combine_start);
            }
            Arc::make_mut(chain).push(patch);
            appended_current = true;
        }
    }

    *child_candidate_start = if current_mesh_combine {
        if appended_current {
            // The current effective patch now represents the complete
            // collapsible parent segment. A child that also combines only has
            // to fold this one node; older entries lie beyond the first native
            // meshCombine=false boundary.
            chain.len().saturating_sub(1)
        } else {
            // Inactive meshCombine nodes are transparent to the native parent
            // walk, so preserve the inherited suffix boundary.
            start
        }
    } else if appended_current {
        // A non-combining active parent is itself the inclusive stop node.
        chain.len().saturating_sub(1)
    } else {
        // A non-combining inactive parent stops the walk before any older
        // active mesh can be reached.
        chain.len()
    };
}

fn native_mesh_chain_position_consumer(layer_type: i64) -> bool {
    // sub_10347C90, called by MMotionPlayer::StepFrameMeshChain
    // (sub_10353CF0), is exactly:
    //
    //     ((1 << layerInfo.type) & 0x22) != 0
    //
    // so only type 1 (Shape) and type 5 (Camera) receive the native
    // post-MeshChain +120/+124 coordinate rebuild. Other layer types keep
    // their ordinary StepFrame coordinates and consume mesh data through
    // their own specialized paths.
    matches!(layer_type, 1 | 5)
}

fn apply_mesh_chain_specialized_pass(
    layer_states: &mut [EmoteStepFrameLayerState],
    scope_start: usize,
    scope_end: usize,
) {
    // MMotionPlayer::StepFrameMeshChain (sub_10353CF0) rebuilds +120/+124
    // from the post-Anchor raw +612/+616 by walking layerInfo+708.  The walk
    // starts at the nearest active mesh-transform ancestor and proceeds toward
    // the root, hence the reverse traversal of our root->leaf chain. Z is not
    // mesh-warped and is copied from +620 verbatim.
    //
    // Crucially, the native call is guarded by sub_10347C90: only Shape (1)
    // and Camera (5) are position consumers here. Applying this pass to every
    // flattened layer incorrectly moves Particle/Feedback/nested helper state.
    let end = scope_end.min(layer_states.len());
    for state in layer_states.iter_mut().take(end).skip(scope_start) {
        if !native_mesh_chain_position_consumer(state.draw_frame_info.layer_type) {
            state.position = state.raw_position;
            continue;
        }
        let mut xy = [state.raw_position[0], state.raw_position[1]];
        for entry in state.mesh_chain.iter().rev() {
            if let Some(mapped) = entry.warp_world_point(xy) {
                xy = mapped;
            }
        }
        state.position = [xy[0], xy[1], state.raw_position[2]];
    }
}

fn intersect_clip_rect(a: [f32; 4], b: [f32; 4]) -> Option<[f32; 4]> {
    let r = [a[0].max(b[0]), a[1].max(b[1]), a[2].min(b[2]), a[3].min(b[3])];
    (r[0] <= r[2] && r[1] <= r[3]).then_some(r)
}

fn native_type7_local_bounds(state: &EmoteStepFrameLayerState) -> [f32; 4] {
    // sub_10352360, confirmed against the x86 call chain at 0x10352444..52641:
    //   offset = M * active_frame(ox, oy)
    //   lo = raw(+612,+616) + M*(-16,-16) - offset
    //   hi = raw(+612,+616) + M*(+16,+16) - offset
    // and then Rect(lo.x, min(lo.y,hi.y), hi.x, max(...)) is normalized by
    // the native rectangle helpers. Importantly this consumes RAW +612/+616,
    // not post-MeshChain +120/+124, and the authored offset is transformed and
    // subtracted rather than added in world space.
    let m = state.transform;
    let transform = |x: f32, y: f32| -> [f32; 2] {
        [m[0] * x + m[1] * y, m[2] * x + m[3] * y]
    };
    let offset = transform(state.frame_offset[0], state.frame_offset[1]);
    let lo = transform(-16.0, -16.0);
    let hi = transform(16.0, 16.0);
    let p0 = [
        state.raw_position[0] + lo[0] - offset[0],
        state.raw_position[1] + lo[1] - offset[1],
    ];
    let p1 = [
        state.raw_position[0] + hi[0] - offset[0],
        state.raw_position[1] + hi[1] - offset[1],
    ];
    [p0[0].min(p1[0]), p0[1].min(p1[1]), p0[0].max(p1[0]), p0[1].max(p1[1])]
}

fn apply_ready_to_draw_specialized_pass(
    layer_states: &mut [EmoteStepFrameLayerState],
    scope_start: usize,
    scope_end: usize,
    sprites: &mut [EmoteStaticSprite],
    sprite_scope_start: usize,
) {
    // MMotionPlayer::StepFrameReadyToDraw (sub_1035A660) runs immediately
    // after MeshChain. +724 points to the nearest stencil-ready ancestor:
    // parent itself when parent+717 is set, otherwise parent's +724.
    // +717 is true only for a valid (non type-0/HOLD) active frame carrying a
    // nonzero stencilType. Types 0/11/12 additionally require drawable state;
    // type 12 applies the native wipe scale/bias range gate.
    let end = scope_end.min(layer_states.len());
    let begin = scope_start.min(end);
    // For each structural index, retain its nearest ready ancestor (including
    // itself). Borrow the stable paths/keys rather than copying five maps of
    // layer metadata on each of the three scene evaluations per host frame.
    let mut ready_ancestors = BTreeMap::<&str, Option<(&str, i64, &[u64])>>::new();
    for state in layer_states[begin..end].iter_mut() {
        let parent_owner = state.scope_index_path.rsplit_once('/')
            .and_then(|(parent, _)| ready_ancestors.get(parent).copied().flatten());

        // sub_103390C0 writes DrawFrameInfo+120 as
        //     layer+724 ? (layer+724)->+732 : NULL
        // and sub_10332D00 allocates +732 only for types 0/3/10/12.  This is
        // NOT "nearest drawable ancestor": if the nearest ready LayerInfo is a
        // Shape/Camera/helper without +732, the native pointer is NULL and the
        // stencil chain terminates there.  The previous Rust code incorrectly
        // exposed the helper path and let the renderer continue the chain.
        state.draw_frame_info.stencil_parent_path = parent_owner
            .as_ref()
            .filter(|(_, layer_type, _)| native_layer_has_draw_frame_info(*layer_type))
            .map(|(path, _, _)| (*path).to_owned());
        state.draw_frame_info.stencil_parent_native_key = parent_owner
            .as_ref()
            .filter(|(_, layer_type, _)| native_layer_has_draw_frame_info(*layer_type))
            .map(|(_, _, draw_key)| draw_key.to_vec());

        // Native StepFrameReadyToDraw begins at LayerInfo index 1 because
        // LayerInfo[0] is the synthetic MMotionPlayer root. `layer_states` does
        // NOT contain that synthetic root, so relative_index 0 here is already
        // native LayerInfo[1] and must be processed normally.  Older Rust code
        // skipped it, leaving the first authored layer with stale/default
        // ready/stencil state.
        let frame_valid = state
            .specialized_frame
            .as_ref()
            .map(|frame| frame.serialized_frame_type != 0)
            .unwrap_or(true);
        let special_drawable_type = matches!(state.draw_frame_info.layer_type, 0 | 11 | 12);
        let mut drawable_gate = true;
        if special_drawable_type {
            drawable_gate = state.visible;
            if state.draw_frame_info.layer_type == 12
                && state.draw_frame_info.stencil_wipe_enabled
            {
                drawable_gate &= state.draw_frame_info.stencil_wipe_scale - 1.0 <= -f32::EPSILON
                    && state.draw_frame_info.stencil_wipe_bias
                        + state.draw_frame_info.stencil_wipe_scale
                        >= f32::EPSILON;
            }
        }
        let ready = frame_valid
            && state.draw_frame_info.stencil_type != 0
            && state.visible
            && (!special_drawable_type || drawable_gate);
        state.draw_frame_info.ready_to_draw = ready;
        state.draw_frame_info.submitted_to_draw_frame = false;

        // sub_103390C0 reverses only the low two stencil phase bits for a
        // reversed type-12 wipe; bit 2 (composite owner) remains untouched.
        let mut phase = state.draw_frame_info.stencil_type & 0x3;
        if state.draw_frame_info.layer_type == 12
            && state.draw_frame_info.stencil_wipe_enabled
            && state.draw_frame_info.stencil_wipe_reverse
        {
            phase = match phase {
                1 => 2,
                2 => 1,
                other => other,
            };
        }
        state.draw_frame_info.stencil_phase = phase;
        ready_ancestors.insert(
            &state.scope_index_path,
            if ready {
                Some((&state.path, state.draw_frame_info.layer_type,
                      &state.draw_frame_info.native_draw_key))
            } else {
                parent_owner
            },
        );
    }

    // Synchronize emitted ordinary sprites by native structural draw key, not
    // by the human-readable label path. Duplicate/empty authored labels can
    // produce identical display paths but never identical LayerInfo priority
    // identities inside one recursive draw stream.
    let infos: BTreeMap<&[u64], &EmoteDrawFrameInfo> = layer_states[begin..end]
        .iter()
        .map(|state| (state.draw_frame_info.native_draw_key.as_slice(), &state.draw_frame_info))
        .collect();
    for sprite in sprites.iter_mut().skip(sprite_scope_start) {
        if let Some(info) = infos.get(sprite.draw_frame_info.native_draw_key.as_slice()) {
            sprite.draw_frame_info.ready_to_draw = info.ready_to_draw;
            sprite.draw_frame_info.submitted_to_draw_frame = info.submitted_to_draw_frame;
            sprite.draw_frame_info.stencil_parent_path = info.stencil_parent_path.clone();
            sprite.draw_frame_info.stencil_parent_native_key =
                info.stencil_parent_native_key.clone();
            sprite.draw_frame_info.stencil_phase = info.stencil_phase;
        }
    }
}

fn camera_runtime_from_scope_states(
    snapshot: &[&EmoteStepFrameLayerState],
) -> Option<EmoteCameraRuntimeState> {
    // MMotionPlayer::StepFrameCamera (sub_10351EF0). Camera layers consume
    // post-MeshChain +120..128 coordinates. `target == ""` falls back to the
    // camera layer itself; a named target is resolved inside the SAME player.
    // The resulting active flag / Vec2 offset / eye / target / FOV are stored
    // on that MMotionPlayer (+0x1d2..+0x1f4); sub_103390C0 does not consume
    // them while constructing the standard 2-D DrawFrameInfo list.
    for state in snapshot {
        if state.draw_frame_info.layer_type != 5 || !state.visible {
            continue;
        }
        let frame = state.specialized_frame.as_ref()?;
        let camera = frame.camera.as_ref()?;
        let target = if camera.target.is_empty() {
            state.position
        } else {
            snapshot
                .iter()
                .find(|candidate| {
                    candidate.draw_frame_info.layer_label.as_deref()
                        == Some(camera.target.as_str())
                        || candidate.path == camera.target
                })
                .map(|candidate| candidate.position)
                .unwrap_or(state.position)
        };
        let root_raw = snapshot
            .first()
            .map(|root| root.raw_position)
            .unwrap_or([0.0; 3]);
        let screen_offset = [
            (-(target[0] - root_raw[0])).round(),
            (-((target[1] + target[2]) - (root_raw[1] + root_raw[2]))).round(),
        ];
        return Some(EmoteCameraRuntimeState {
            layer_path: state.path.clone(),
            fov: camera.fov,
            eye: state.position,
            target,
            screen_offset,
        });
    }
    None
}

fn apply_camera_specialized_pass(
    layer_states: &[EmoteStepFrameLayerState],
    scope_start: usize,
    scope_end: usize,
) -> Option<EmoteCameraRuntimeState> {
    let end = scope_end.min(layer_states.len());
    let snapshot: Vec<&EmoteStepFrameLayerState> = layer_states
        [scope_start.min(end)..end]
        .iter()
        .collect();
    camera_runtime_from_scope_states(&snapshot)
}

fn collect_camera_runtimes(
    layer_states: &[EmoteStepFrameLayerState],
) -> BTreeMap<String, EmoteCameraRuntimeState> {
    let mut scopes = BTreeMap::<String, Vec<&EmoteStepFrameLayerState>>::new();
    for state in layer_states {
        if !state.motion_scope_root_path.is_empty() {
            scopes
                .entry(state.motion_scope_root_path.clone())
                .or_default()
                .push(state);
        }
    }
    scopes
        .into_iter()
        .filter_map(|(scope, states)| {
            camera_runtime_from_scope_states(&states).map(|camera| (scope, camera))
        })
        .collect()
}

fn apply_type7_bounds_specialized_pass(
    layer_states: &mut [EmoteStepFrameLayerState],
    scope_start: usize,
    scope_end: usize,
    sprites: &mut [EmoteStaticSprite],
    sprite_scope_start: usize,
) {
    // sub_10352360 propagates layerInfo+692 from parent to child. Active type-7
    // layers replace it with their own transformed +/-16 box intersected with
    // the inherited rectangle; inactive type-7 layers simply inherit.
    let end = scope_end.min(layer_states.len());
    let mut rects = BTreeMap::<String, Option<[f32; 4]>>::new();
    for state in layer_states.iter_mut().take(end).skip(scope_start) {
        let parent = state.path.rsplit_once('/').map(|(p, _)| p);
        let inherited = parent.and_then(|p| rects.get(p).copied().flatten());
        let rect = if state.draw_frame_info.layer_type == 7 && state.visible {
            let own = native_type7_local_bounds(state);
            inherited.and_then(|p| intersect_clip_rect(own, p)).or(Some(own).filter(|_| inherited.is_none()))
        } else {
            inherited
        };
        state.draw_frame_info.clip_rect = rect;
        rects.insert(state.path.clone(), rect);
    }
    for sprite in sprites.iter_mut().skip(sprite_scope_start) {
        if let Some(rect) = rects.get(&sprite.draw_frame_info.path).copied().flatten() {
            sprite.draw_frame_info.clip_rect = Some(rect);
        }
    }
}

fn find_layer_state_position(
    states: &[EmoteStepFrameLayerState],
    scope_start: usize,
    scope_end: usize,
    target: &str,
    raw: bool,
) -> Option<[f32; 3]> {
    states
        .iter()
        .take(scope_end.min(states.len()))
        .skip(scope_start)
        .find(|state| {
            state.draw_frame_info.layer_label.as_deref() == Some(target) || state.path == target
        })
        .map(|state| if raw { state.raw_position } else { state.position })
}

fn apply_model_specialized_pass(
    layer_states: &mut [EmoteStepFrameLayerState],
    scope_start: usize,
    scope_end: usize,
    motion_time_ticks: f32,
    previous_positions: Option<&BTreeMap<String, [f32; 3]>>,
) {
    // MMotionPlayer::StepFrameModel (sub_103552F0). The model backend itself
    // is host-specific, but the model clock and all three native direction
    // modes are pure MMotionPlayer state and must be resolved here.
    let end = scope_end.min(layer_states.len());
    let scope = &layer_states[scope_start.min(end)..end];
    // Only target-directed 3-D models read other layers. Ordinary 2-D
    // character scopes must not clone every layer for an unused snapshot.
    let snapshot = if scope.iter().any(|state| {
        state.draw_frame_info.layer_type == 6
            && state.visible
            && state.specialized_frame.as_ref()
                .and_then(|frame| frame.model.as_ref())
                .is_some_and(|model| model.direction_type == 4)
    }) {
        scope.to_vec()
    } else {
        Vec::new()
    };
    for state in layer_states.iter_mut().take(end).skip(scope_start) {
        if state.draw_frame_info.layer_type != 6 || !state.visible {
            state.model_runtime = None;
            continue;
        }
        let Some(frame) = state.specialized_frame.as_ref() else {
            state.model_runtime = None;
            continue;
        };
        let Some(model) = frame.model.as_ref() else {
            state.model_runtime = None;
            continue;
        };

        // On a newly selected model frame sub_103552F0 initializes the model
        // clock to currentMotionTime - frameStart + timeOffset. On subsequent
        // ticks it adds player delta. For an ordinary monotonic StepFrame these
        // are algebraically identical; using the local expression also makes
        // the SDK's pre/post-physics double rebuild idempotent.
        let local_time_ticks = finite_or(
            motion_time_ticks - frame.frame_start_ticks + model.time_offset_ticks,
            0.0,
        );
        let direction = match model.direction_type {
            // dt=2: exact layer+108 raw current-minus-previous displacement.
            2 if local_time_ticks != 0.0 => previous_positions.and_then(|previous| {
                previous.get(&state.path).copied().map(|p| [
                    state.raw_position[0] - p[0],
                    state.raw_position[1] - p[1],
                    state.raw_position[2] - p[2],
                ])
            }),
            // dt=3: the two 0.0001 path samples produced by StepLayer. Native
            // stores the vector itself; do not normalize it to an angle.
            3 => frame.motion_path_tangent_vector,
            // dt=4: target +612..620 minus this layer's raw composite XYZ.
            4 => find_layer_state_position(
                &snapshot,
                0,
                snapshot.len(),
                &model.direction_target,
                true,
            )
            .map(|target| [
                target[0] - state.raw_position[0],
                target[1] - state.raw_position[1],
                target[2] - state.raw_position[2],
            ]),
            _ => None,
        };
        state.model_runtime = Some(EmoteModelRuntimeState {
            local_time_ticks,
            looped: model.looped,
            direction_type: model.direction_type,
            direction,
            direction_target: (!model.direction_target.is_empty())
                .then(|| model.direction_target.clone()),
        });
    }
}

fn particle_rng_next(state: &mut u64) -> f32 {
    // Deterministic xorshift64* stream. Random *distribution* and call order
    // follow the DLL; exact manager RNG seeding is host-global and not stored
    // in the PSB, so it cannot be reconstructed from a scene snapshot alone.
    let mut x = (*state).max(1);
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    let bits = x.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40;
    (bits as f32) / ((1u32 << 24) as f32)
}

fn particle_random_range(state: &mut u64, a: f32, b: f32) -> f32 {
    let lo = finite_or(a, 0.0);
    let hi = finite_or(b, lo);
    lo + (hi - lo) * particle_rng_next(state)
}

fn particle_random_index(state: &mut u64, count: usize) -> Option<usize> {
    if count == 0 {
        None
    } else {
        Some(((particle_rng_next(state) * count as f32).floor() as usize).min(count - 1))
    }
}

fn particle_transform_vector(transform: [f32; 6], v: [f32; 2]) -> [f32; 2] {
    [
        transform[0] * v[0] + transform[1] * v[1],
        transform[2] * v[0] + transform[3] * v[1],
    ]
}

fn particle_transform_scale(transform: [f32; 6]) -> f32 {
    // Exact native sub_10357200 path for tri-volume Z: sub_1038CD60 returns
    // det(m00,m01;m10,m11), sub_1020F350 takes fabs, then the caller takes
    // sqrt. This is therefore sqrt(abs(det(M))), including non-uniform scale,
    // shear and reflection; it is not an affine approximation.
    (transform[0] * transform[3] - transform[1] * transform[2])
        .abs()
        .sqrt()
}

fn particle_emitter_angle_degrees(transform: [f32; 6]) -> f32 {
    transform[2].atan2(transform[0]).to_degrees()
}

fn particle_motion_timing(
    object_table: &PsbValue,
    object_name: &str,
    motion_name: &str,
) -> Option<(f32, bool)> {
    let motion = object_table
        .field(object_name)?
        .field("motion")?
        .field(motion_name)?;
    let last = motion_duration_ticks(motion).unwrap_or(0.0);
    let loop_time = motion.field_f32("loopTime").unwrap_or(-1.0);
    Some((last, loop_time.is_finite() && loop_time >= 0.0 && loop_time < last))
}

fn particle_instance_finished(
    object_table: &PsbValue,
    particle: &ParticleInstanceRuntime,
) -> bool {
    let Some((last, looped)) = particle_motion_timing(
        object_table,
        &particle.object_name,
        &particle.motion_name,
    ) else {
        return true;
    };
    !looped && last > 0.0 && particle.age_ticks >= last
}

fn affine_inverse_linear(transform: [f32; 6]) -> Option<[f32; 4]> {
    let det = transform[0] * transform[3] - transform[1] * transform[2];
    if !det.is_finite() || det.abs() <= f32::EPSILON {
        return None;
    }
    let inv = 1.0 / det;
    Some([
        transform[3] * inv,
        -transform[1] * inv,
        -transform[2] * inv,
        transform[0] * inv,
    ])
}

fn apply_linear4(m: [f32; 4], p: [f32; 2]) -> [f32; 2] {
    [m[0] * p[0] + m[1] * p[1], m[2] * p[0] + m[3] * p[1]]
}

fn remap_particle_with_emitter_transform(
    particle: &mut ParticleInstanceRuntime,
    previous_origin: [f32; 3],
    current_origin: [f32; 3],
    previous_transform: [f32; 6],
    current_transform: [f32; 6],
    coordinate: i64,
) {
    let Some(inv) = affine_inverse_linear(previous_transform) else {
        return;
    };
    let current_linear = [
        current_transform[0],
        current_transform[1],
        current_transform[2],
        current_transform[3],
    ];
    if coordinate == 1 {
        let rel = [
            particle.position[0] - previous_origin[0],
            particle.position[2] - previous_origin[2],
        ];
        let local = apply_linear4(inv, rel);
        let mapped = apply_linear4(current_linear, local);
        particle.position[0] = current_origin[0] + mapped[0];
        particle.position[2] = current_origin[2] + mapped[1];
        let local_v = apply_linear4(inv, [particle.velocity[0], particle.velocity[2]]);
        let mapped_v = apply_linear4(current_linear, local_v);
        particle.velocity[0] = mapped_v[0];
        particle.velocity[2] = mapped_v[1];
    } else {
        let rel = [
            particle.position[0] - previous_origin[0],
            particle.position[1] - previous_origin[1],
        ];
        let local = apply_linear4(inv, rel);
        let mapped = apply_linear4(current_linear, local);
        particle.position[0] = current_origin[0] + mapped[0];
        particle.position[1] = current_origin[1] + mapped[1];
        let local_v = apply_linear4(inv, [particle.velocity[0], particle.velocity[1]]);
        let mapped_v = apply_linear4(current_linear, local_v);
        particle.velocity[0] = mapped_v[0];
        particle.velocity[1] = mapped_v[1];
    }
}

fn spawn_particle_instance(
    emitter: &EmoteStepFrameLayerState,
    frame: ParticleFrameState,
    config: &ParticleStaticConfig,
    runtime: &mut ParticleEmitterRuntime,
    object_table: &PsbValue,
    delta_ticks: f32,
) -> Option<ParticleInstanceRuntime> {
    let motion_index = particle_random_index(&mut runtime.rng_state, config.motion_list.len())?;
    let motion_ref = config.motion_list.get(motion_index)?.clone();
    let mut local = [0.0f32; 3];
    match config.particle {
        1 => {
            let azimuth = particle_random_range(
                &mut runtime.rng_state,
                0.0,
                std::f32::consts::TAU,
            );
            if config.tri_volume {
                let polar = particle_random_range(
                    &mut runtime.rng_state,
                    0.0,
                    std::f32::consts::TAU,
                );
                let radius = particle_rng_next(&mut runtime.rng_state).cbrt() * 16.0;
                local[0] = azimuth.cos() * polar.cos() * radius;
                local[1] = azimuth.sin() * polar.cos() * radius;
                local[2] = polar.sin() * radius;
            } else {
                let radius = particle_rng_next(&mut runtime.rng_state).sqrt() * 16.0;
                local[0] = azimuth.cos() * radius;
                local[1] = azimuth.sin() * radius;
            }
        }
        2 => {
            local[0] = particle_random_range(&mut runtime.rng_state, -16.0, 16.0);
            local[1] = particle_random_range(&mut runtime.rng_state, -16.0, 16.0);
            if config.tri_volume {
                local[2] = particle_random_range(&mut runtime.rng_state, -16.0, 16.0);
            }
        }
        _ => {}
    }

    let transformed = particle_transform_vector(emitter.transform, [local[0], local[1]]);
    let z_offset = local[2] * particle_transform_scale(emitter.transform);
    let coordinate = emitter.draw_frame_info.coordinate.unwrap_or(0);
    let mut position = emitter.raw_position;
    if coordinate == 1 {
        position[0] += transformed[0];
        position[2] += transformed[1];
        position[1] += z_offset;
    } else {
        position[0] += transformed[0];
        position[1] += transformed[1];
        position[2] += z_offset;
    }

    let mut speed = particle_random_range(&mut runtime.rng_state, frame.vmin, frame.vmax);
    let mut direction_deg = match config.fly_direction {
        1 => transformed[1].atan2(transformed[0]).to_degrees(),
        2 => transformed[1].atan2(transformed[0]).to_degrees() + 180.0,
        _ => particle_emitter_angle_degrees(emitter.transform),
    };
    let spread = particle_random_range(&mut runtime.rng_state, -frame.range, frame.range);
    direction_deg += spread;
    let direction = direction_deg.to_radians();

    let mut horizontal_factor = 1.0f32;
    let mut z_factor = 0.0f32;
    if matches!(config.fly_direction, 1 | 2) && z_offset != 0.0 {
        let horizontal = (transformed[0] * transformed[0] + transformed[1] * transformed[1]).sqrt();
        let distance = (horizontal * horizontal + z_offset * z_offset).sqrt();
        if distance > f32::EPSILON {
            horizontal_factor = horizontal / distance;
            z_factor = z_offset / distance;
            if config.fly_direction == 2 {
                z_factor = -z_factor;
            }
        }
    }

    if config.fly_direction == 2 {
        if let Some((last_ticks, _)) = particle_motion_timing(
            object_table,
            &motion_ref.object_name,
            &motion_ref.motion_name,
        ) {
            let duration_seconds = last_ticks / 60.0;
            let distance = (transformed[0] * transformed[0]
                + transformed[1] * transformed[1]
                + z_offset * z_offset)
                .sqrt();
            if duration_seconds > f32::EPSILON {
                if (config.accel_ratio - 1.0).abs() <= f32::EPSILON {
                    speed = distance / duration_seconds / 60.0;
                } else if config.accel_ratio > 0.0 {
                    let growth = config.accel_ratio.powf(duration_seconds);
                    let denom = growth - 1.0;
                    if denom.abs() > f32::EPSILON {
                        speed = config.accel_ratio.ln() * distance / denom / 60.0;
                    }
                }
            }
        }
    }

    let mut velocity = [0.0f32; 3];
    if coordinate == 1 {
        velocity[0] = direction.cos() * speed * horizontal_factor;
        velocity[2] = direction.sin() * speed * horizontal_factor;
        velocity[1] = speed * z_factor;
    } else {
        velocity[0] = direction.cos() * speed * horizontal_factor;
        velocity[1] = direction.sin() * speed * horizontal_factor;
        velocity[2] = speed * z_factor;
    }

    let mut angle_random = particle_random_range(&mut runtime.rng_state, frame.amin, frame.amax);
    let flip_x = emitter.transform[0] < 0.0;
    let flip_y = emitter.transform[3] < 0.0;
    if flip_x != flip_y {
        angle_random = -angle_random;
    }
    let angle_degrees = if config.inherit_angle {
        let inherited = if flip_x { direction_deg + 180.0 } else { direction_deg };
        (inherited + angle_random).rem_euclid(360.0)
    } else {
        angle_random.rem_euclid(360.0)
    };
    let zoom = particle_random_range(&mut runtime.rng_state, frame.zmin, frame.zmax);
    if config.fly_direction != 2 {
        match config.apply_zoom_to_velocity {
            1 => {
                for component in &mut velocity {
                    *component *= zoom;
                }
            }
            2 if zoom.abs() > f32::EPSILON => {
                for component in &mut velocity {
                    *component /= zoom;
                }
            }
            _ => {}
        }
    }
    if config.inherit_velocity == 1 && delta_ticks > f32::EPSILON {
        for i in 0..3 {
            velocity[i] += (emitter.raw_position[i] - runtime.last_raw_position[i]) / delta_ticks;
        }
    }

    let serial = runtime.next_serial;
    runtime.next_serial = runtime.next_serial.wrapping_add(1);
    Some(ParticleInstanceRuntime {
        serial,
        object_name: motion_ref.object_name,
        motion_name: motion_ref.motion_name,
        age_ticks: 0.0,
        position,
        velocity,
        angle_degrees,
        zoom,
        opacity: if config.inherit_opacity != 0 { emitter.opacity } else { 1.0 },
        accel_ratio: config.accel_ratio,
        has_entered_screen: false,
    })
}

fn serialized_frame_transition_dirty(
    previous: Option<&DynamicFrameState>,
    current: &DynamicFrameState,
) -> bool {
    if current.serialized_frame_type == 0 {
        return false;
    }
    previous.map_or(true, |previous| previous.frame_index != current.frame_index)
}

fn particle_spawn_count(
    emitter: &EmoteStepFrameLayerState,
    frame: ParticleFrameState,
    runtime: &mut ParticleEmitterRuntime,
    delta_ticks: f32,
) -> usize {
    if !emitter.visible {
        runtime.initialized = false;
        return 0;
    }
    if frame.trigger == 1 {
        runtime.initialized = true;
        return if emitter.particle_triggered {
            particle_random_range(&mut runtime.rng_state, frame.fmin, frame.fmax)
                .floor()
                .max(0.0) as usize
        } else {
            0
        };
    }
    if frame.fmin <= 0.0 || frame.fmax <= 0.0 {
        runtime.initialized = true;
        return 0;
    }
    if !runtime.initialized {
        runtime.initialized = true;
        let lo = 60.0 / frame.fmin.max(f32::EPSILON);
        let hi = 60.0 / frame.fmax.max(f32::EPSILON);
        runtime.countdown_ticks += particle_random_range(&mut runtime.rng_state, lo, hi);
    }
    runtime.countdown_ticks -= delta_ticks.max(0.0);
    let mut count = 0usize;
    let mut guard = 0usize;
    while runtime.countdown_ticks <= 0.0 && guard < 4096 {
        count += 1;
        guard += 1;
        let lo = 60.0 / frame.fmin.max(f32::EPSILON);
        let hi = 60.0 / frame.fmax.max(f32::EPSILON);
        let interval = particle_random_range(&mut runtime.rng_state, lo, hi).abs().max(f32::EPSILON);
        runtime.countdown_ticks += interval;
    }
    count
}

fn particle_screen_rect(layer_states: &[EmoteStepFrameLayerState]) -> Option<[f32; 4]> {
    // The native deleteOutsideScreen path receives the host/player screen
    // rectangle; it does NOT infer a viewport from arbitrary visible layer
    // positions. In PSB-authored motion data the only recovered local source
    // is type-10 Feedback::screenBounds, so use that when present and leave the
    // test disabled otherwise rather than inventing geometry.
    layer_states.iter().find_map(|state| state.screen_bounds)
}

fn rects_overlap(a: [f32; 4], b: [f32; 4]) -> bool {
    // sub_103478B0: strict rectangle overlap. Touching an edge is outside.
    a[0] < b[2] && b[0] < a[2] && a[1] < b[3] && b[1] < a[3]
}

fn particle_child_bounds(sprites: &[EmoteStaticSprite]) -> Option<[f32; 4]> {
    let bounds = compute_bounds(sprites)?;
    Some([bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y])
}

fn particle_child_context(
    emitter: &EmoteStepFrameLayerState,
    particle: &ParticleInstanceRuntime,
) -> TravelContext {
    let mut ctx = TravelContext::default();
    let linear = EmoteTransform2D::rotation(particle.angle_degrees)
        .then(EmoteTransform2D::scale(particle.zoom, particle.zoom));
    ctx.base_location = Some(particle.position);
    ctx.opacity_multiplier = particle.opacity;
    ctx.path = format!("{}/@particle/{}", emitter.path, particle.serial);
    ctx.draw_index = emitter.draw_frame_info.draw_index;
    // Native type-4 handling in sub_103390C0 walks active particle child
    // players in manager order and recursively appends each child's draw list
    // at the emitter's priority slot. Serial is monotonic creation order in the
    // Rust runtime, so it is the stable equivalent of that per-emitter list
    // position even after old particles are drained.
    ctx.native_draw_key = emitter.draw_frame_info.native_draw_key.clone();
    ctx.native_draw_key.push(particle.serial);
    ctx.coordinate = emitter.draw_frame_info.coordinate;
    ctx.transform = EmoteTransform2D {
        tx: particle.position[0],
        ty: if emitter.draw_frame_info.coordinate == Some(1) {
            particle.position[2]
        } else {
            particle.position[1]
        },
        ..linear
    };
    ctx.linear_state = FrameLinearState {
        rotation_degrees: particle.angle_degrees,
        scale_x: particle.zoom,
        scale_y: particle.zoom,
        ..FrameLinearState::default()
    };
    let root = InheritSourceState {
        linear,
        linear_state: ctx.linear_state,
        location: particle.position,
        coordinate: ctx.coordinate,
        opacity: particle.opacity,
        mesh_sync: None,
    };
    ctx.inherit_source = root;
    ctx.motion_root = root;
    ctx.mesh_chain = Arc::new(emitter.mesh_chain.clone());
    ctx.parent_mask_path = emitter.draw_frame_info.parent_mask_path.clone();
    ctx
}

fn apply_parent_particle_clip(
    emitter: &EmoteStepFrameLayerState,
    layer_states: &mut [EmoteStepFrameLayerState],
    sprites: &mut [EmoteStaticSprite],
) {
    let Some(parent_rect) = emitter.draw_frame_info.clip_rect else {
        return;
    };
    for state in layer_states {
        state.draw_frame_info.clip_rect = state
            .draw_frame_info
            .clip_rect
            .and_then(|own| intersect_clip_rect(parent_rect, own))
            .or(Some(parent_rect));
    }
    for sprite in sprites {
        sprite.draw_frame_info.clip_rect = sprite
            .draw_frame_info
            .clip_rect
            .and_then(|own| intersect_clip_rect(parent_rect, own))
            .or(Some(parent_rect));
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_particle_specialized_pass(
    object_table: &PsbValue,
    parameter_table: Option<&[PsbValue]>,
    psb: &PsbFile,
    psb_data: Option<&[u8]>,
    variables: &BTreeMap<String, f32>,
    textures: &BTreeMap<String, EmoteTextureSource>,
    delta_ticks: f32,
    previous_positions: Option<&BTreeMap<String, [f32; 3]>>,
    previous_frame_states: Option<&BTreeMap<String, DynamicFrameState>>,
    ground_correction_hook: Option<EmoteGroundCorrectionHook>,
    frame_runtime_states: &mut BTreeMap<String, DynamicFrameState>,
    emitters: &mut BTreeMap<String, ParticleEmitterRuntime>,
    sprites: &mut Vec<EmoteStaticSprite>,
    layer_states: &mut Vec<EmoteStepFrameLayerState>,
    mask_owners: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), EmoteSchemaError> {
    // StepFrameParticle runs after Model. Process in flattened traversal order;
    // newly-created child MMotionPlayers may themselves contain emitters, so
    // continue scanning until every newly appended type-4 layer has run once.
    let screen_rect = particle_screen_rect(layer_states);
    let mut cursor = 0usize;
    let mut processed = BTreeSet::<String>::new();
    while cursor < layer_states.len() {
        let emitter = &layer_states[cursor];
        cursor += 1;
        let Some(config) = emitter.particle_static.clone() else {
            continue;
        };
        if !processed.insert(emitter.path.clone()) {
            continue;
        }
        let Some(frame) = emitter.specialized_frame.as_ref().and_then(|state| state.particle) else {
            continue;
        };
        // Recurse below may append layers; own only actual emitter state.
        let emitter = emitter.clone();
        let angle = particle_emitter_angle_degrees(emitter.transform);
        let runtime = emitters
            .entry(emitter.path.clone())
            .or_insert_with(|| ParticleEmitterRuntime::new(
                &emitter.path,
                emitter.raw_position,
                emitter.transform,
                angle,
            ));

        if config.inherit_velocity == 2 && runtime.initialized {
            for particle in &mut runtime.particles {
                remap_particle_with_emitter_transform(
                    particle,
                    runtime.last_raw_position,
                    emitter.raw_position,
                    runtime.last_transform,
                    emitter.transform,
                    emitter.draw_frame_info.coordinate.unwrap_or(0),
                );
                let delta_angle = angle - runtime.last_angle_degrees;
                particle.angle_degrees = (particle.angle_degrees + delta_angle).rem_euclid(360.0);
            }
        }
        if config.inherit_opacity == 2 {
            for particle in &mut runtime.particles {
                particle.opacity = emitter.opacity;
            }
        }

        if delta_ticks > 0.0 {
            for particle in &mut runtime.particles {
                for axis in 0..3 {
                    particle.position[axis] += particle.velocity[axis] * delta_ticks;
                }
                let accel = if particle.accel_ratio > 0.0 {
                    particle.accel_ratio.powf(delta_ticks / 60.0)
                } else {
                    0.0
                };
                for component in &mut particle.velocity {
                    *component *= accel;
                }
                particle.age_ticks += delta_ticks;
            }
        }
        runtime
            .particles
            .retain(|particle| !particle_instance_finished(object_table, particle));

        let spawn_count = particle_spawn_count(&emitter, frame, runtime, delta_ticks);
        for _ in 0..spawn_count {
            if let Some(particle) = spawn_particle_instance(
                &emitter,
                frame,
                &config,
                runtime,
                object_table,
                delta_ticks,
            ) {
                runtime.particles.push(particle);
            }
        }
        if config.max_num == 0 {
            runtime.particles.clear();
        } else if runtime.particles.len() > config.max_num {
            let remove = runtime.particles.len() - config.max_num;
            runtime.particles.drain(0..remove);
        }

        runtime.last_raw_position = emitter.raw_position;
        runtime.last_transform = emitter.transform;
        runtime.last_angle_degrees = angle;

        let particle_snapshot = runtime.particles.clone();
        let mut outside_after_entry = BTreeSet::<u64>::new();
        for particle in particle_snapshot {
            let start_layer = layer_states.len();
            let start_sprite = sprites.len();
            let ctx = particle_child_context(&emitter, &particle);
            // Particles are newly-created child players rather than replacement
            // MMotionPlayers, so they do not consume the parent's joinTarget pool.
            let mut particle_join_reuse = JoinReusePool::default();
            recurse_motion_at(
                object_table,
                &particle.object_name,
                &particle.motion_name,
                parameter_table,
                psb,
                psb_data,
                variables,
                textures,
                particle.age_ticks,
                ctx,
                previous_positions,
                previous_frame_states,
                ground_correction_hook,
                delta_ticks,
                frame_runtime_states,
                &mut particle_join_reuse,
                sprites,
                layer_states,
                mask_owners,
            )?;
            apply_parent_particle_clip(
                &emitter,
                &mut layer_states[start_layer..],
                &mut sprites[start_sprite..],
            );

            // sub_10357200 tests the spawned child MMotionPlayer's actual
            // rectangle and maintains a one-byte "has entered screen" latch.
            // A particle that starts outside is retained; only a particle that
            // has intersected the screen and subsequently leaves is deleted.
            if config.delete_outside_screen {
                if let (Some(screen), Some(child)) =
                    (screen_rect, particle_child_bounds(&sprites[start_sprite..]))
                {
                    let inside = rects_overlap(child, screen);
                    if let Some(native_particle) = runtime
                        .particles
                        .iter_mut()
                        .find(|candidate| candidate.serial == particle.serial)
                    {
                        if inside {
                            native_particle.has_entered_screen = true;
                        } else if native_particle.has_entered_screen {
                            outside_after_entry.insert(native_particle.serial);
                            // Native sub_10357200 performs the outside-screen
                            // removal before the surviving child players enter
                            // their final StepFrame loop.  recurse_motion_at has
                            // already materialized this child's draw output, so
                            // roll it back immediately to preserve the same
                            // current-frame visibility semantics.
                            sprites.truncate(start_sprite);
                            layer_states.truncate(start_layer);
                        }
                    }
                }
            }
        }
        if !outside_after_entry.is_empty() {
            runtime
                .particles
                .retain(|particle| !outside_after_entry.contains(&particle.serial));
        }
    }
    Ok(())
}

fn shape_linear_transform(transform: [f32; 6], point: [f32; 2]) -> [f32; 2] {
    [
        transform[0] * point[0] + transform[1] * point[1],
        transform[2] * point[0] + transform[3] * point[1],
    ]
}

/// MMotionPlayer::StepFrameShape (sub_10359D20).
///
/// The native pass runs after MeshChain/type-7 bounds and consumes +120/+124
/// rather than raw +612/+616. Cases 0..3 fill the 36-byte GetShapeParam
/// payload as point, circle, axis-aligned rect, or transformed 16x16 quad.
fn apply_shape_specialized_pass(
    layer_states: &mut [EmoteStepFrameLayerState],
    scope_start: usize,
    scope_end: usize,
) {
    let end = scope_end.min(layer_states.len());
    for state in &mut layer_states[scope_start.min(end)..end] {
        if state.draw_frame_info.layer_type != 1 || !state.visible {
            state.shape_runtime = None;
            continue;
        }
        let center = [state.position[0], state.position[1]];
        state.shape_runtime = match state.shape_kind {
            0 => Some(EmoteShapeRuntimeState::Point { center }),
            1 => Some(EmoteShapeRuntimeState::Circle {
                center,
                // sub_10359D20 case 1: (16 * composite zx) / 2.
                radius: 8.0 * state.linear_state.scale_x,
            }),
            2 => {
                // case 2 builds `center +/- (16*zx,16*zy)/2` without
                // normalizing the rectangle, so retain authored sign.
                let half_x = 8.0 * state.linear_state.scale_x;
                let half_y = 8.0 * state.linear_state.scale_y;
                Some(EmoteShapeRuntimeState::Rect {
                    left: center[0] - half_x,
                    top: center[1] - half_y,
                    right: center[0] + half_x,
                    bottom: center[1] + half_y,
                })
            }
            3 => {
                // Native case 3 transforms the four +/-8 local corners with
                // layer+92's 2x2 matrix, subtracts the transformed active
                // frame offset, then adds post-MeshChain +120/+124.
                let origin = shape_linear_transform(state.transform, state.frame_offset);
                let corners = [[-8.0, -8.0], [8.0, -8.0], [-8.0, 8.0], [8.0, 8.0]];
                let mut points = [[0.0; 2]; 4];
                for (dst, corner) in points.iter_mut().zip(corners) {
                    let p = shape_linear_transform(state.transform, corner);
                    *dst = [center[0] + p[0] - origin[0], center[1] + p[1] - origin[1]];
                }
                Some(EmoteShapeRuntimeState::Quad { points })
            }
            _ => None,
        };
    }
}

fn scope_layer_positions(states: &[EmoteStepFrameLayerState]) -> Vec<ScopeLayerPosition> {
    states
        .iter()
        .map(|state| ScopeLayerPosition {
            label: state.draw_frame_info.layer_label.clone(),
            path: state.path.clone(),
            position: state.position,
        })
        .collect()
}

fn find_scope_layer_position(
    states: &[ScopeLayerPosition],
    target: &str,
) -> Option<[f32; 3]> {
    // sub_10355BF0 case 4 calls MMotionPlayer's layer-name lookup
    // (sub_1019B9E2) with dtgt. Layer labels are the authored names registered
    // in that player; do not use fuzzy substring matching across nested players.
    states
        .iter()
        .find(|state| state.label.as_deref() == Some(target) || state.path == target)
        .map(|state| state.position)
}

#[allow(clippy::too_many_arguments)]
fn resolve_pending_nested_motions(
    pending: Vec<PendingNestedMotion>,
    scope_positions: &[ScopeLayerPosition],
    object_table: &PsbValue,
    parameter_table: Option<&[PsbValue]>,
    psb: &PsbFile,
    psb_data: Option<&[u8]>,
    variables: &BTreeMap<String, f32>,
    textures: &BTreeMap<String, EmoteTextureSource>,
    previous_positions: Option<&BTreeMap<String, [f32; 3]>>,
    previous_frame_states: Option<&BTreeMap<String, DynamicFrameState>>,
    ground_correction_hook: Option<EmoteGroundCorrectionHook>,
    delta_ticks: f32,
    frame_runtime_states: &mut BTreeMap<String, DynamicFrameState>,
    join_reuse: &mut JoinReusePool,
    out: &mut Vec<EmoteStaticSprite>,
    layer_states: &mut Vec<EmoteStepFrameLayerState>,
    mask_owners: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), EmoteSchemaError> {
    // Native sub_10331060 first finalizes every layer's base StepFrame, then
    // runs sub_10355BF0 for nested-motion layers. Deferring recursion here is
    // required for direction mode 4 because dtgt may name a later sibling.
    for pending in pending {
        let target_position = if pending.state.motion_direction_type == 4 {
            pending
                .state
                .motion_direction_target
                .as_deref()
                .and_then(|target| find_scope_layer_position(scope_positions, target))
        } else {
            None
        };
        let mut motion_ctx = pending.ctx;
        apply_nested_motion_direction_resolved(
            &mut motion_ctx,
            &pending.state,
            previous_positions,
            target_position,
        );
        apply_motion_layer_inherit(
            &pending.layer,
            object_table,
            &pending.object_name,
            &pending.motion_name,
            &mut motion_ctx,
        );
        recurse_motion_at(
            object_table,
            &pending.object_name,
            &pending.motion_name,
            parameter_table,
            psb,
            psb_data,
            variables,
            textures,
            nested_motion_local_time(pending.parent_local_time, &pending.state),
            motion_ctx,
            previous_positions,
            previous_frame_states,
            ground_correction_hook,
            delta_ticks,
            frame_runtime_states,
            join_reuse,
            out,
            layer_states,
            mask_owners,
        )?;
    }
    Ok(())
}

fn target_motion_parameter_ids(
    object_table: &PsbValue,
    object_name: &str,
    motion_name: &str,
) -> Option<BTreeSet<String>> {
    let parameters = object_table
        .field(object_name)?
        .field("motion")?
        .field(motion_name)?
        .field("parameter")?
        .as_list()?;
    let ids = parameters
        .iter()
        .filter_map(|parameter| {
            parameter
                .field_str("id")
                .or_else(|| parameter.field_str("key"))
                .or_else(|| parameter.field_str("name"))
        })
        .filter(|id| !id.is_empty())
        .map(str::to_owned)
        .collect();
    Some(ids)
}

fn recurse_motion_at(
    object_table: &PsbValue,
    object_name: &str,
    motion_name: &str,
    parameter_table: Option<&[PsbValue]>,
    psb: &PsbFile,
    psb_data: Option<&[u8]>,
    variables: &BTreeMap<String, f32>,
    textures: &BTreeMap<String, EmoteTextureSource>,
    time_ticks: f32,
    mut ctx: TravelContext,
    previous_positions: Option<&BTreeMap<String, [f32; 3]>>,
    previous_frame_states: Option<&BTreeMap<String, DynamicFrameState>>,
    ground_correction_hook: Option<EmoteGroundCorrectionHook>,
    delta_ticks: f32,
    frame_runtime_states: &mut BTreeMap<String, DynamicFrameState>,
    join_reuse: &mut JoinReusePool,
    out: &mut Vec<EmoteStaticSprite>,
    layer_states: &mut Vec<EmoteStepFrameLayerState>,
    mask_owners: &mut BTreeMap<String, Vec<String>>,
) -> Result<(), EmoteSchemaError> {
    // A nested type-3/particle child owns a distinct MMotionPlayer scope even
    // though its flattened path continues under the parent layer. Native
    // sub_103390C0 recursively pushes that child's DrawFrameInfo entries at
    // the current parent/particle slot, so convert the incoming key into this
    // player's prefix before assigning child-layer priority ranks.
    ctx.native_draw_prefix = ctx.native_draw_key.clone();
    ctx.scope_local_path.clear();
    ctx.scope_index_path.clear();
    ctx.motion_scope_root_path.clear();
    let Some(motion) = object_table
        .field(object_name)
        .and_then(|object| object.field("motion"))
        .and_then(|motion| motion.field(motion_name))
    else {
        return Ok(());
    };
    let Some(layers) = motion.field("layer").and_then(PsbValue::as_list) else {
        return Ok(());
    };
    let motion_parameter_table = motion
        .field("parameter")
        .and_then(PsbValue::as_list)
        .or(parameter_table);
    let effective_time = effective_motion_time(motion, time_ticks);
    let priority_ranks = Arc::new(motion_priority_ranks(motion, effective_time));
    let scope_start = layer_states.len();
    let sprite_scope_start = out.len();
    let mut pending_nested = Vec::new();
    let mut pending_anchors = Vec::new();
    for (index, layer) in layers.iter().enumerate() {
        let mut next_ctx = ctx.clone();
        next_ctx.priority_ranks = priority_ranks.clone();
        travel_layer_at(
            layer,
            index,
            object_table,
            motion_parameter_table,
            psb,
            psb_data,
            variables,
            textures,
            motion_name,
            effective_time,
            next_ctx,
            previous_positions,
            previous_frame_states,
            frame_runtime_states,
            join_reuse,
            &mut pending_nested,
            &mut pending_anchors,
            out,
            layer_states,
            mask_owners,
        )?;
    }
    let scope_end = layer_states.len();
    apply_ground_correction_specialized_pass(
        ground_correction_hook,
        previous_positions,
        scope_start,
        scope_end,
        sprite_scope_start,
        out,
        layer_states,
        &mut pending_nested,
    );
    apply_anchor_specialized_pass(
        &pending_anchors,
        scope_start,
        sprite_scope_start,
        out,
        layer_states,
        &mut pending_nested,
    );
    apply_mesh_chain_specialized_pass(layer_states, scope_start, scope_end);
    apply_ready_to_draw_specialized_pass(
        layer_states,
        scope_start,
        scope_end,
        out,
        sprite_scope_start,
    );
    let _ = apply_camera_specialized_pass(layer_states, scope_start, scope_end);
    apply_type7_bounds_specialized_pass(layer_states, scope_start, scope_end, out, sprite_scope_start);
    apply_shape_specialized_pass(layer_states, scope_start, scope_end);
    let scope_lookup = scope_layer_positions(&layer_states[scope_start..scope_end]);
    resolve_pending_nested_motions(
        pending_nested,
        &scope_lookup,
        object_table,
        motion_parameter_table,
        psb,
        psb_data,
        variables,
        textures,
        previous_positions,
        previous_frame_states,
        ground_correction_hook,
        delta_ticks,
        frame_runtime_states,
        join_reuse,
        out,
        layer_states,
        mask_owners,
    )?;
    apply_model_specialized_pass(
        layer_states,
        scope_start,
        scope_end,
        effective_time,
        previous_positions,
    );
    Ok(())
}

fn parse_mesh_division(value: Option<&PsbValue>) -> Option<(u32, u32)> {
    let value = value?;
    if let Some(n) = value.as_i64() {
        let n = n.clamp(1, 256) as u32;
        return Some((n, n));
    }
    if let Some(list) = value.as_list() {
        if list.len() >= 2 {
            let x = list[0].as_i64().unwrap_or(1).clamp(1, 256) as u32;
            let y = list[1].as_i64().unwrap_or(x as i64).clamp(1, 256) as u32;
            return Some((x, y));
        }
    }
    if let Some(x) = value
        .field_i64("x")
        .or_else(|| value.field_i64("width"))
        .or_else(|| value.field_i64("divisionX"))
    {
        let y = value
            .field_i64("y")
            .or_else(|| value.field_i64("height"))
            .or_else(|| value.field_i64("divisionY"))
            .unwrap_or(x);
        return Some((x.clamp(1, 256) as u32, y.clamp(1, 256) as u32));
    }
    None
}

fn parse_content_mesh_patch(
    content: &PsbValue,
    division_x: u32,
    division_y: u32,
) -> Option<EmoteMeshPatch> {
    if let Some(mesh) = content.field("mesh") {
        if let Some(patch) = parse_mesh_dict_patch(mesh, division_x, division_y) {
            return Some(patch);
        }
    }
    if let Some(bp) = content.field("mbp") {
        return parse_bezier_patch(bp, division_x, division_y);
    }
    None
}

fn parse_mesh_dict_patch(
    mesh: &PsbValue,
    division_x: u32,
    division_y: u32,
) -> Option<EmoteMeshPatch> {
    parse_bezier_patch(mesh.field("bp")?, division_x, division_y)
}

fn parse_bezier_patch(bp: &PsbValue, division_x: u32, division_y: u32) -> Option<EmoteMeshPatch> {
    match bp {
        PsbValue::Null => Some(EmoteMeshPatch::identity(division_x, division_y)),
        PsbValue::List(values) if values.len() >= 32 => {
            let mut patch = EmoteMeshPatch::identity(division_x, division_y);
            for i in 0..16 {
                patch.control_points[i] = [
                    values[i * 2].as_f32().unwrap_or(patch.control_points[i][0]),
                    values[i * 2 + 1]
                        .as_f32()
                        .unwrap_or(patch.control_points[i][1]),
                ];
            }
            Some(patch)
        }
        _ => None,
    }
}

fn evaluate_one_combinator(
    combinator: &PsbValue,
    psb: &PsbFile,
    psb_data: &[u8],
    variables: &BTreeMap<String, f32>,
    division_x: u32,
    division_y: u32,
    is_delta: bool,
) -> Option<EmoteMeshPatch> {
    let variable = combinator.field("variable")?;
    let key = variable.field_str("key")?;
    let mesh_count = variable.field_i64("meshCount")?.max(1) as usize;
    let begin = variable.field_f32("rangeBegin").unwrap_or(0.0);
    let end = variable.field_f32("rangeEnd").unwrap_or(1.0);
    let value = variables.get(key).copied().unwrap_or_else(|| {
        if begin <= 0.0 && end >= 0.0 {
            0.0
        } else {
            (begin + end) * 0.5
        }
    });
    let neutral_index = combinator.field_i64("neutralIndex").unwrap_or(-1);
    let resource_index = combinator.field("rawMeshList")?.as_u32()? as usize;
    let raw = psb.resource_bytes(psb_data, resource_index)?;
    let meshes = decode_raw_mesh_list(raw, mesh_count, is_delta)?;
    if meshes.is_empty() {
        return None;
    }

    let pos = if mesh_count <= 1
        || !begin.is_finite()
        || !end.is_finite()
        || (end - begin).abs() <= f32::EPSILON
    {
        0.0
    } else {
        ((value - begin) / (end - begin)).clamp(0.0, 1.0) * (mesh_count as f32 - 1.0)
    };
    let i0 = pos.floor() as usize;
    let i1 = pos.ceil() as usize;
    let t = pos - i0 as f32;
    let p0 = mesh_patch_from_values(
        meshes.get(i0)?,
        neutral_index == i0 as i64,
        division_x,
        division_y,
    );
    let p1 = mesh_patch_from_values(
        meshes.get(i1).unwrap_or(&meshes[i0]),
        neutral_index == i1 as i64,
        division_x,
        division_y,
    );
    Some(EmoteMeshPatch::interpolate(&p0, &p1, t))
}

fn mesh_patch_from_values(
    values: &[f32; 32],
    neutral: bool,
    division_x: u32,
    division_y: u32,
) -> EmoteMeshPatch {
    if neutral {
        return EmoteMeshPatch::identity(division_x, division_y);
    }
    let mut patch = EmoteMeshPatch::identity(division_x, division_y);
    for i in 0..16 {
        patch.control_points[i] = [values[i * 2], values[i * 2 + 1]];
    }
    patch
}

fn decode_raw_mesh_list(raw: &[u8], mesh_count: usize, is_delta: bool) -> Option<Vec<[f32; 32]>> {
    let total = mesh_count.checked_mul(32)?;
    let mut values = vec![0.0f32; total];
    if raw.len() >= total * 8 {
        for (i, chunk) in raw.chunks_exact(8).take(total).enumerate() {
            values[i] = f64::from_le_bytes(chunk.try_into().ok()?) as f32;
        }
    } else if raw.len() >= total * 4 {
        for (i, chunk) in raw.chunks_exact(4).take(total).enumerate() {
            values[i] = f32::from_le_bytes(chunk.try_into().ok()?);
        }
    } else {
        return None;
    }

    if is_delta {
        for mesh_index in 0..mesh_count {
            for row in 0..4 {
                for col in 0..4 {
                    let base = mesh_index * 32 + (row * 4 + col) * 2;
                    values[base] += col as f32 / 3.0;
                    values[base + 1] += row as f32 / 3.0;
                }
            }
        }
    }

    let mut out = Vec::with_capacity(mesh_count);
    for mesh_index in 0..mesh_count {
        let mut mesh = [0.0f32; 32];
        mesh.copy_from_slice(&values[mesh_index * 32..mesh_index * 32 + 32]);
        out.push(mesh);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesh_icon_dimensions_and_off_center_origin_define_local_domain() {
        assert_eq!(parse_mesh_domain_icon("701:1316:350:658"),
            Some([-350.0, -658.0, 701.0, 1316.0]));
        assert_eq!(parse_mesh_domain_icon("100:80:0:20"),
            Some([0.0, -20.0, 100.0, 80.0]));
        assert_eq!(parse_mesh_domain_icon("0:80:0:20"), None);
        assert_eq!(parse_mesh_domain_icon("NaN:80:0:20"), None);
    }

    #[test]
    fn mesh_chain_warps_in_owner_space_under_rotation_scale_and_translation() {
        let mut patch = EmoteMeshPatch::identity(1, 1);
        patch.domain = Some([-50.0, -40.0, 100.0, 80.0]);
        // A uniform local displacement of (10, 0).
        for point in &mut patch.control_points { point[0] += 0.1; }
        let entry = MeshChainEntry {
            patch,
            transform: [0.0, -2.0, 2.0, 0.0, 300.0, -700.0],
        };
        let result = entry.warp_world_point([300.0, -700.0]).unwrap();
        assert!((result[0] - 300.0).abs() < 0.001);
        assert!((result[1] + 680.0).abs() < 0.001);
    }

    fn test_layer(label: &str, children: Vec<PsbValue>) -> PsbValue {
        let mut fields = vec![("label".to_owned(), PsbValue::String(label.to_owned()))];
        if !children.is_empty() {
            fields.push(("children".to_owned(), PsbValue::List(children)));
        }
        PsbValue::Object(fields)
    }

    fn test_priority_frame(time: f32, content: &[i64]) -> PsbValue {
        PsbValue::Object(vec![
            ("time".to_owned(), PsbValue::Float(time)),
            (
                "content".to_owned(),
                PsbValue::List(content.iter().copied().map(PsbValue::Int).collect()),
            ),
        ])
    }

    fn test_runtime_layer_state(
        path: &str,
        index_path: &str,
        native_key: &[u64],
        label: &str,
        layer_type: i64,
        stencil_type: i64,
    ) -> EmoteStepFrameLayerState {
        let ctx = TravelContext {
            path: path.to_owned(),
            scope_index_path: index_path.to_owned(),
            native_draw_key: native_key.to_vec(),
            layer_type,
            stencil_type,
            ready_to_draw: false,
            ..TravelContext::default()
        };
        layer_state_from_ctx(Some(label.to_owned()), &ctx)
    }

    fn test_runtime_sprite(
        path: &str,
        native_key: &[u64],
        label: &str,
    ) -> EmoteStaticSprite {
        let ctx = TravelContext {
            path: path.to_owned(),
            native_draw_key: native_key.to_vec(),
            layer_type: 0,
            ..TravelContext::default()
        };
        EmoteStaticSprite {
            label: Some(label.to_owned()),
            motion_name: "test".to_owned(),
            texture_name: "tex".to_owned(),
            texture_resource_index: 0,
            texture_width: 1,
            texture_height: 1,
            texture_format: None,
            icon_name: "icon".to_owned(),
            feedback_history: false,
            z: 0.0,
            opacity: 1.0,
            blend_mode: 0,
            blend_parameter: 0.0,
            corner_colors: [0xFFFF_FFFF; 4],
            visible: true,
            center_x: 0.0,
            center_y: 0.0,
            width: 1.0,
            height: 1.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation_degrees: 0.0,
            world_transform: EmoteTransform2D::identity().as_array(),
            uv_left: 0.0,
            uv_top: 0.0,
            uv_right: 1.0,
            uv_bottom: 1.0,
            mesh: None,
            draw_frame_info: draw_frame_info(Some(label.to_owned()), &ctx),
        }
    }

    #[test]
    fn native_manager_sort_uses_exact_z_before_priority_emission_key() {
        let mut back = test_runtime_sprite("back", &[99], "back");
        back.z = -0.10;
        let mut front = test_runtime_sprite("front", &[0], "front");
        front.z = 0.10;
        // Stage 11-13 incorrectly let native_draw_key dominate; native
        // sub_10344710 compares DFI+36 first, so the lower-Z item is submitted
        // first regardless of per-player priority emission rank.
        assert_eq!(native_manager_frame_order(&back, &front), std::cmp::Ordering::Less);
        assert_eq!(native_manager_frame_order(&front, &back), std::cmp::Ordering::Greater);
    }

    #[test]
    fn native_manager_sort_does_not_round_z_planes() {
        let mut a = test_runtime_sprite("a", &[10], "a");
        let mut b = test_runtime_sprite("b", &[0], "b");
        a.z = 0.20;
        b.z = 0.21;
        // Both values rounded to the same integer in Stage 10. Native
        // sub_10344710 compares the stored float exactly.
        assert_eq!(native_manager_frame_order(&a, &b), std::cmp::Ordering::Less);
    }

    #[test]
    fn ready_to_draw_processes_first_authored_layer() {
        let mut states = vec![test_runtime_layer_state("mask", "0", &[0], "mask", 0, 1)];
        let mut sprites = Vec::new();
        apply_ready_to_draw_specialized_pass(&mut states, 0, 1, &mut sprites, 0);
        assert!(states[0].draw_frame_info.ready_to_draw);
    }

    #[test]
    fn ready_helper_without_dfi_cuts_native_stencil_parent_pointer() {
        let mut states = vec![
            test_runtime_layer_state("helper", "0", &[0], "helper", 1, 1),
            test_runtime_layer_state("helper/color", "0/0", &[1], "color", 0, 0),
        ];
        let mut sprites = Vec::new();
        apply_ready_to_draw_specialized_pass(&mut states, 0, 2, &mut sprites, 0);
        assert!(states[0].draw_frame_info.ready_to_draw);
        assert_eq!(states[1].draw_frame_info.stencil_parent_path, None);
        assert_eq!(states[1].draw_frame_info.stencil_parent_native_key, None);
    }

    #[test]
    fn ready_drawable_parent_exposes_native_dfi_parent_key() {
        let mut states = vec![
            test_runtime_layer_state("mask", "0", &[7], "mask", 0, 1),
            test_runtime_layer_state("mask/color", "0/0", &[9], "color", 0, 0),
        ];
        let mut sprites = Vec::new();
        apply_ready_to_draw_specialized_pass(&mut states, 0, 2, &mut sprites, 0);
        assert_eq!(states[1].draw_frame_info.stencil_parent_path.as_deref(), Some("mask"));
        assert_eq!(states[1].draw_frame_info.stencil_parent_native_key, Some(vec![7]));
    }

    #[test]
    fn type3_composite_source_expands_nested_drawframe_list() {
        let mut owner = test_runtime_layer_state("owner", "0", &[0], "owner", 12, 5);
        owner.draw_frame_info.stencil_composite_mask_layer_list = vec!["nested".to_owned()];
        let source = test_runtime_layer_state("nested", "1", &[1], "nested", 3, 0);
        let nested_sprite = test_runtime_sprite("nested/face", &[1, 0], "face");
        let unrelated_sprite = test_runtime_sprite("other", &[2], "other");
        let states = vec![owner, source];
        let sprites = vec![nested_sprite, unrelated_sprite];
        let raw = BTreeMap::from([("owner".to_owned(), vec!["nested".to_owned()])]);

        let (_by_path, by_key, referenced_type3) =
            resolve_composite_mask_owners(&sprites, &states, &raw);
        assert_eq!(by_key.get(&vec![0]).cloned(), Some(vec![vec![1, 0]]));
        assert!(referenced_type3.contains(&vec![1]));
    }

    #[test]
    fn native_priority_uses_flat_preorder_indices_and_reverse_emission() {
        // sub_10333180 flattening: A, A/B, A/C, D. The deliberately bogus
        // layerIndexMap must not influence draw order because sub_103390C0
        // indexes LayerInfo[priority + 1] directly.
        let motion = PsbValue::Object(vec![
            (
                "layer".to_owned(),
                PsbValue::List(vec![
                    test_layer("A", vec![test_layer("B", vec![]), test_layer("C", vec![])]),
                    test_layer("D", vec![]),
                ]),
            ),
            (
                "layerIndexMap".to_owned(),
                PsbValue::Object(vec![
                    ("A".to_owned(), PsbValue::Int(99)),
                    ("B".to_owned(), PsbValue::Int(98)),
                    ("C".to_owned(), PsbValue::Int(97)),
                    ("D".to_owned(), PsbValue::Int(96)),
                ]),
            ),
            (
                "priority".to_owned(),
                PsbValue::List(vec![test_priority_frame(0.0, &[2, 0, 3, 1])]),
            ),
        ]);
        let ranks = motion_priority_ranks(&motion, 0.0);
        // Native traversal reverses [2,0,3,1] -> [1,3,0,2].
        // Structural preorder paths are: 0 (A), 0/0 (A/B), 0/1 (A/C), 1 (D).
        assert_eq!(ranks.get("0/0"), Some(&0));
        assert_eq!(ranks.get("1"), Some(&1));
        assert_eq!(ranks.get("0"), Some(&2));
        assert_eq!(ranks.get("0/1"), Some(&3));
    }

    #[test]
    fn duplicate_labels_do_not_alias_native_priority_slots() {
        // Labels are not identities in native LayerInfo. Two siblings may have
        // the same (or empty) label and still occupy different priority slots.
        let motion = PsbValue::Object(vec![
            (
                "layer".to_owned(),
                PsbValue::List(vec![
                    test_layer("dup", vec![]),
                    test_layer("dup", vec![]),
                    test_layer("", vec![]),
                ]),
            ),
            (
                "priority".to_owned(),
                PsbValue::List(vec![test_priority_frame(0.0, &[0, 2, 1])]),
            ),
        ]);
        let ranks = motion_priority_ranks(&motion, 0.0);
        // Native reverse emission: [1, 2, 0]. Every structural slot remains
        // distinct even though labels collide.
        assert_eq!(ranks.get("1"), Some(&0));
        assert_eq!(ranks.get("2"), Some(&1));
        assert_eq!(ranks.get("0"), Some(&2));
        assert_eq!(ranks.len(), 3);
    }

    #[test]
    fn same_player_child_uses_global_priority_slot_not_parent_prefix() {
        let mut ranks = BTreeMap::new();
        ranks.insert("0".to_owned(), 7usize);
        ranks.insert("0/0".to_owned(), 1usize);
        let ranks = Arc::new(ranks);
        let parent = enter_layer_context(
            TravelContext {
                priority_ranks: ranks.clone(),
                ..TravelContext::default()
            },
            &test_layer("A", vec![]),
            0,
        );
        assert_eq!(parent.native_draw_key, vec![7]);
        let child = enter_layer_context(
            TravelContext {
                priority_ranks: ranks,
                ..parent
            },
            &test_layer("B", vec![]),
            0,
        );
        // A/B is another entry in the same flat LayerInfo array, therefore it
        // occupies rank 1 directly; it is not recursively nested under A.
        assert_eq!(child.native_draw_key, vec![1]);
    }

    #[test]
    fn nested_player_appends_its_priority_rank_to_parent_slot() {
        let mut child_ranks = BTreeMap::new();
        child_ranks.insert("0".to_owned(), 3usize);
        let child = enter_layer_context(
            TravelContext {
                native_draw_prefix: vec![5],
                priority_ranks: Arc::new(child_ranks),
                ..TravelContext::default()
            },
            &test_layer("Child", vec![]),
            0,
        );
        assert_eq!(child.native_draw_key, vec![5, 3]);
    }

    #[test]
    fn native_color_drawable_gate_matches_sub_10347c10() {
        for layer_type in [0, 10, 12] {
            assert!(native_color_drawable_layer_type(layer_type));
        }
        for layer_type in [1, 2, 3, 4, 5, 6, 7, 8, 9, 11] {
            assert!(!native_color_drawable_layer_type(layer_type));
        }
        assert!(native_nested_motion_layer_type(3));
        for layer_type in [0, 1, 2, 4, 5, 6, 7, 8, 9, 10, 11, 12] {
            assert!(!native_nested_motion_layer_type(layer_type));
        }
    }

    #[test]
    fn native_mesh_chain_position_consumer_is_shape_or_camera_only() {
        assert!(native_mesh_chain_position_consumer(1));
        assert!(native_mesh_chain_position_consumer(5));
        for layer_type in [0, 2, 3, 4, 6, 7, 8, 9, 10, 11, 12] {
            assert!(!native_mesh_chain_position_consumer(layer_type));
        }
    }

    fn test_mesh_patch(dx: f32, dy: f32) -> MeshChainEntry {
        let mut patch = EmoteMeshPatch::identity(1, 1);
        patch.domain = Some([-1.0, -1.0, 1.0, 1.0]);
        patch.control_points[5][0] += dx;
        patch.control_points[5][1] += dy;
        MeshChainEntry { patch, transform: EmoteTransform2D::identity().as_array() }
    }

    #[test]
    fn native_mesh_combine_collapses_inclusive_noncombining_active_parent() {
        let parent = test_mesh_patch(0.10, 0.20);
        let child = test_mesh_patch(0.30, -0.10);
        let mut chain = Arc::new(Vec::new());
        let mut candidate = 0usize;

        // Active parent with meshCombine=false: it is the inclusive stop node
        // for a combining child.
        advance_native_mesh_combine_chain(
            &mut chain,
            &mut candidate,
            0,
            Some(parent),
            true,
            false,
        );
        assert_eq!(chain.len(), 1);
        assert_eq!(candidate, 0);

        let inherited_candidate = candidate;
        advance_native_mesh_combine_chain(
            &mut chain,
            &mut candidate,
            inherited_candidate,
            Some(child),
            true,
            true,
        );
        assert_eq!(chain.len(), 1);
        let identity = EmoteMeshPatch::identity(1, 1);
        assert!((chain[0].patch.control_points[5][0] - (identity.control_points[5][0] + 0.40)).abs() < 1.0e-6);
        assert!((chain[0].patch.control_points[5][1] - (identity.control_points[5][1] + 0.10)).abs() < 1.0e-6);
    }

    #[test]
    fn native_mesh_combine_inactive_false_parent_is_a_hard_barrier() {
        let older = test_mesh_patch(0.10, 0.0);
        let current = test_mesh_patch(0.30, 0.0);
        let mut chain = Arc::new(vec![older]);
        let mut candidate = 0usize;

        // An inactive meshTransform with meshCombine=false causes the native
        // parent walk to stop before reaching the older active mesh.
        advance_native_mesh_combine_chain(
            &mut chain,
            &mut candidate,
            0,
            None,
            false,
            false,
        );
        assert_eq!(candidate, 1);

        let inherited_candidate = candidate;
        advance_native_mesh_combine_chain(
            &mut chain,
            &mut candidate,
            inherited_candidate,
            Some(current),
            true,
            true,
        );
        assert_eq!(chain.len(), 2);
    }

    #[test]
    fn native_mesh_combine_inactive_true_parent_preserves_parent_walk() {
        let older = test_mesh_patch(0.10, 0.0);
        let current = test_mesh_patch(0.30, 0.0);
        let mut chain = Arc::new(vec![older]);
        let mut candidate = 0usize;

        // meshCombine=true with no active mesh is transparent: native keeps
        // walking upward and the next active combining child reaches `older`.
        advance_native_mesh_combine_chain(
            &mut chain,
            &mut candidate,
            0,
            None,
            false,
            true,
        );
        assert_eq!(candidate, 0);

        let inherited_candidate = candidate;
        advance_native_mesh_combine_chain(
            &mut chain,
            &mut candidate,
            inherited_candidate,
            Some(current),
            true,
            true,
        );
        assert_eq!(chain.len(), 1);
        let identity = EmoteMeshPatch::identity(1, 1);
        assert!((chain[0].patch.control_points[5][0] - (identity.control_points[5][0] + 0.40)).abs() < 1.0e-6);
    }

    #[test]
    fn native_combine_mesh_adds_identity_relative_displacements() {
        let mut a = EmoteMeshPatch::identity(2, 3);
        let mut b = EmoteMeshPatch::identity(4, 1);
        a.control_points[5][0] += 0.10;
        a.control_points[5][1] -= 0.20;
        b.control_points[5][0] -= 0.30;
        b.control_points[5][1] += 0.40;

        let combined = a.combined_with(&b);
        let identity = EmoteMeshPatch::identity(4, 3);
        assert_eq!(combined.division_x, 4);
        assert_eq!(combined.division_y, 3);
        assert!((combined.control_points[5][0]
            - (identity.control_points[5][0] - 0.20))
            .abs()
            < 1.0e-6);
        assert!((combined.control_points[5][1]
            - (identity.control_points[5][1] + 0.20))
            .abs()
            < 1.0e-6);
    }

    #[test]
    fn native_combine_mesh_identity_is_neutral() {
        let identity = EmoteMeshPatch::identity(2, 2);
        let mut patch = EmoteMeshPatch::identity(2, 2);
        patch.control_points[10] = [0.82, 0.57];
        patch.domain = Some([-1.0, -2.0, 3.0, 4.0]);

        let combined = identity.combined_with(&patch);
        assert_eq!(combined.control_points, patch.control_points);
        assert_eq!(combined.domain, patch.domain);
    }

    #[test]
    fn native_combine_mesh_is_not_bezier_function_composition() {
        let mut first = EmoteMeshPatch::identity(1, 1);
        let mut second = EmoteMeshPatch::identity(1, 1);
        // Moving one interior control point makes function composition sample
        // a different location, while native combineMesh simply adds the two
        // identity-relative control-point offsets.
        first.control_points[5][0] += 0.25;
        second.control_points[6][1] -= 0.30;

        let native = first.combined_with(&second);
        let p = first.control_points[5];
        let composed = second.sample(p[0], p[1]);
        assert!((native.control_points[5][0] - composed[0]).abs() > 1.0e-4
            || (native.control_points[5][1] - composed[1]).abs() > 1.0e-4);
    }

    #[test]
    fn transform_order_defaults_to_native_order_and_accepts_all_permutations() {
        assert_eq!(normalized_transform_order(&[]), [0, 3, 2, 1]);
        assert_eq!(normalized_transform_order(&[3, 2, 1, 0]), [3, 2, 1, 0]);
        assert_eq!(normalized_transform_order(&[0, 0, 2, 3]), [0, 3, 2, 1]);
    }

    #[test]
    fn partial_inherit_combines_native_channels_independently() {
        let parent = FrameLinearState {
            flip_x: true,
            flip_y: true,
            rotation_degrees: 30.0,
            scale_x: 2.0,
            scale_y: 3.0,
            shear_x: 0.25,
            shear_y: -0.5,
        };
        let own = FrameLinearState {
            flip_x: false,
            flip_y: true,
            rotation_degrees: 5.0,
            scale_x: 4.0,
            scale_y: 5.0,
            shear_x: 0.5,
            shear_y: 0.75,
        };
        let inherited = inherit_frame_linear_state(own, parent, 0x4 | 0x10 | 0x40 | 0x80);
        assert!(inherited.flip_x);
        assert!(inherited.flip_y);
        assert!((inherited.rotation_degrees - 35.0).abs() < 1.0e-6);
        assert!((inherited.scale_x - 4.0).abs() < 1.0e-6);
        assert!((inherited.scale_y - 15.0).abs() < 1.0e-6);
        assert!((inherited.shear_x - 0.75).abs() < 1.0e-6);
        assert!((inherited.shear_y - 0.75).abs() < 1.0e-6);
    }

    #[test]
    fn child_coordinates_follow_native_xy_or_xz_plane() {
        let base = InheritSourceState {
            linear: EmoteTransform2D::rotation(90.0),
            linear_state: FrameLinearState::default(),
            location: [10.0, 20.0, 30.0],
            coordinate: Some(0),
            opacity: 1.0,
            mesh_sync: None,
        };
        let xy = map_child_coordinate_through_source(base, [2.0, 3.0, 4.0]);
        assert!((xy[0] - 7.0).abs() < 1.0e-5);
        assert!((xy[1] - 22.0).abs() < 1.0e-5);
        assert!((xy[2] - 34.0).abs() < 1.0e-5);

        let xz_source = InheritSourceState {
            coordinate: Some(1),
            ..base
        };
        let xz = map_child_coordinate_through_source(xz_source, [2.0, 3.0, 4.0]);
        assert!((xz[0] - 6.0).abs() < 1.0e-5);
        assert!((xz[1] - 23.0).abs() < 1.0e-5);
        assert!((xz[2] - 32.0).abs() < 1.0e-5);
    }

    #[test]
    fn transparent_inherit_parent_keeps_upstream_source() {
        let upstream = InheritSourceState {
            location: [11.0, 12.0, 13.0],
            opacity: 0.5,
            ..InheritSourceState::default()
        };
        let mut ctx = TravelContext {
            inherit_mask: Some(0x400000),
            inherit_source: upstream,
            base_location: Some([99.0, 98.0, 97.0]),
            opacity_multiplier: 0.25,
            ..TravelContext::default()
        };
        prepare_child_inherit_source(&mut ctx, None);
        assert_eq!(ctx.inherit_source.location, upstream.location);
        assert_eq!(ctx.inherit_source.opacity, upstream.opacity);
    }

    #[test]
    fn motion_root_compensation_removes_only_inherited_channels() {
        let root = FrameLinearState {
            flip_x: true,
            flip_y: true,
            rotation_degrees: 30.0,
            scale_x: 2.0,
            scale_y: 4.0,
            shear_x: 0.25,
            shear_y: 0.5,
        };
        let composite = FrameLinearState {
            flip_x: true,
            flip_y: false,
            rotation_degrees: 40.0,
            scale_x: 6.0,
            scale_y: 8.0,
            shear_x: 0.75,
            shear_y: 1.0,
        };
        let relative = remove_motion_root_linear_state(composite, root, 0x10);
        assert_eq!(relative.flip_x, composite.flip_x);
        assert_eq!(relative.flip_y, composite.flip_y);
        assert!((relative.rotation_degrees - 10.0).abs() < 1.0e-6);
        assert!((relative.scale_x - composite.scale_x).abs() < 1.0e-6);
        assert!((relative.scale_y - composite.scale_y).abs() < 1.0e-6);
        assert!((relative.shear_x - composite.shear_x).abs() < 1.0e-6);
        assert!((relative.shear_y - composite.shear_y).abs() < 1.0e-6);
    }

    #[test]
    fn native_frame_easing_piece_with_zero_second_derivative_is_linear() {
        let piece = PsbValue::Object(vec![
            (
                "x".to_owned(),
                PsbValue::List(vec![PsbValue::Float(0.0), PsbValue::Float(1.0)]),
            ),
            (
                "y".to_owned(),
                PsbValue::List(vec![PsbValue::Float(0.0), PsbValue::Float(1.0)]),
            ),
            (
                "p".to_owned(),
                PsbValue::List(vec![PsbValue::Float(0.0), PsbValue::Float(0.0)]),
            ),
        ]);
        let y = evaluate_native_easing_piece(&piece, 0.25).unwrap();
        assert!((y - 0.25).abs() < 1.0e-6);
    }

    #[test]
    fn native_frame_easing_matches_recovered_cubic_spline_formula() {
        let piece = PsbValue::Object(vec![
            (
                "x".to_owned(),
                PsbValue::List(vec![PsbValue::Float(0.0), PsbValue::Float(1.0)]),
            ),
            (
                "y".to_owned(),
                PsbValue::List(vec![PsbValue::Float(0.0), PsbValue::Float(1.0)]),
            ),
            (
                "p".to_owned(),
                PsbValue::List(vec![PsbValue::Float(2.0), PsbValue::Float(-1.0)]),
            ),
        ]);
        let x = 0.25f32;
        let u = x;
        let v = 1.0 - u;
        let expected = v * 0.0
            + u * 1.0
            + ((u * u * u - u) * -1.0 + (v * v * v - v) * 2.0) / 6.0;
        let y = evaluate_native_easing_piece(&piece, x).unwrap();
        assert!((y - expected).abs() < 1.0e-6);
    }

    #[test]
    fn frame_easing_resolves_native_top_level_easing_index() {
        let curve = PsbValue::Object(vec![
            (
                "x".to_owned(),
                PsbValue::List(vec![PsbValue::Float(0.0), PsbValue::Float(1.0)]),
            ),
            (
                "y".to_owned(),
                PsbValue::List(vec![PsbValue::Float(0.0), PsbValue::Float(1.0)]),
            ),
            (
                "p".to_owned(),
                PsbValue::List(vec![PsbValue::Float(0.0), PsbValue::Float(0.0)]),
            ),
        ]);
        let table = vec![curve];
        let easing_ref = PsbValue::Int(0);
        let y = frame_easing(0.75, Some(&easing_ref), Some(&table));
        assert!((y - 0.75).abs() < 1.0e-6);
    }

    fn test_frame(time: f32, frame_type: i64, content: PsbValue) -> PsbValue {
        PsbValue::Object(vec![
            ("time".to_owned(), PsbValue::Float(time)),
            ("type".to_owned(), PsbValue::Int(frame_type)),
            ("content".to_owned(), content),
        ])
    }

    fn test_content(fields: Vec<(&str, PsbValue)>) -> PsbValue {
        PsbValue::Object(
            fields
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
        )
    }

    fn test_coord(x: f32, y: f32, z: f32) -> PsbValue {
        PsbValue::List(vec![
            PsbValue::Float(x),
            PsbValue::Float(y),
            PsbValue::Float(z),
        ])
    }

    #[test]
    fn native_type_zero_frame_preserves_previous_local_state() {
        let frames = vec![
            test_frame(
                0.0,
                1,
                test_content(vec![
                    ("coord", test_coord(2.0, 3.0, 4.0)),
                    ("angle", PsbValue::Float(15.0)),
                    ("opa", PsbValue::Int(200)),
                ]),
            ),
            test_frame(10.0, 0, test_content(vec![])),
        ];
        let previous = evaluate_frame_list(&frames, 5.0, None, 0, None);
        let held = evaluate_frame_list(&frames, 12.0, None, 0, Some(&previous));
        assert_eq!(held.coord, Some([2.0, 3.0, 4.0]));
        assert!((held.rotation_degrees - 15.0).abs() < 1.0e-6);
        assert_eq!(held.opa, 200.0);
    }

    #[test]
    fn unparameterized_layer_uses_motion_player_time() {
        let layer = PsbValue::Object(vec![]);
        let frame_list = vec![test_frame(0.0, 1, test_content(vec![]))];
        let eval = layer_parameter_eval(
            &layer,
            None,
            &BTreeMap::new(),
            &frame_list,
            37.5,
        )
        .unwrap();
        assert_eq!(eval.id, None);
        assert_eq!(eval.value, None);
        assert!((eval.local_time_ticks - 37.5).abs() < 1.0e-6);
    }

    #[test]
    fn parameterized_layer_uses_native_range_and_division_mapping() {
        let layer = PsbValue::Object(vec![("parameterize".to_owned(), PsbValue::Int(0))]);
        let parameter = PsbValue::Object(vec![
            ("id".to_owned(), PsbValue::String("face_lr".to_owned())),
            ("rangeBegin".to_owned(), PsbValue::Float(-10.0)),
            ("rangeEnd".to_owned(), PsbValue::Float(10.0)),
            ("division".to_owned(), PsbValue::Float(100.0)),
        ]);
        let parameters = vec![parameter];
        let mut variables = BTreeMap::new();
        variables.insert("face_lr".to_owned(), 5.0);
        let frame_list = vec![test_frame(0.0, 1, test_content(vec![]))];
        let eval = layer_parameter_eval(&layer, Some(&parameters), &variables, &frame_list, 0.0)
            .unwrap();
        // sub_10350230: (5 - -10) * 100 / (10 - -10) = 75.
        assert!((eval.local_time_ticks - 75.0).abs() < 1.0e-6);

        // sub_10350230 clamps the value into [rangeBegin, rangeEnd] first.
        variables.insert("face_lr".to_owned(), 20.0);
        let eval = layer_parameter_eval(&layer, Some(&parameters), &variables, &frame_list, 0.0)
            .unwrap();
        assert!((eval.local_time_ticks - 100.0).abs() < 1.0e-6);
        variables.insert("face_lr".to_owned(), -20.0);
        let eval = layer_parameter_eval(&layer, Some(&parameters), &variables, &frame_list, 0.0)
            .unwrap();
        assert!(eval.local_time_ticks.abs() < 1.0e-6);
    }

    #[test]
    fn discretized_parameter_truncates_toward_zero() {
        let layer = PsbValue::Object(vec![("parameterize".to_owned(), PsbValue::Int(0))]);
        let parameters = vec![PsbValue::Object(vec![
            ("id".to_owned(), PsbValue::String("face_lr".to_owned())),
            ("discretization".to_owned(), PsbValue::Int(1)),
            ("rangeBegin".to_owned(), PsbValue::Float(-10.0)),
            ("rangeEnd".to_owned(), PsbValue::Float(10.0)),
            ("division".to_owned(), PsbValue::Float(100.0)),
        ])];
        let frame_list = vec![test_frame(0.0, 1, test_content(vec![]))];
        let eval_at = |value: f32| {
            let variables = BTreeMap::from([("face_lr".to_owned(), value)]);
            layer_parameter_eval(&layer, Some(&parameters), &variables, &frame_list, 0.0)
                .unwrap()
                .local_time_ticks
        };
        // cvttss2si: 5.7 -> 5, -3.7 -> -3.
        assert!((eval_at(5.7) - 75.0).abs() < 1.0e-6);
        assert!((eval_at(-3.7) - 35.0).abs() < 1.0e-6);
    }

    #[test]
    fn motion_time_wraps_from_last_time_to_loop_time() {
        let motion = PsbValue::Object(vec![
            ("loopTime".to_owned(), PsbValue::Float(20.0)),
            ("lastTime".to_owned(), PsbValue::Float(100.0)),
        ]);
        assert!((effective_motion_time(&motion, 99.0) - 99.0).abs() < 1.0e-6);
        assert!((effective_motion_time(&motion, 100.0) - 20.0).abs() < 1.0e-6);
        assert!((effective_motion_time(&motion, 105.0) - 25.0).abs() < 1.0e-6);
        assert!((effective_motion_time(&motion, 185.0) - 25.0).abs() < 1.0e-6);
    }

    #[test]
    fn non_looping_motion_clamps_at_last_time() {
        let motion = PsbValue::Object(vec![
            ("loopTime".to_owned(), PsbValue::Float(-1.0)),
            ("lastTime".to_owned(), PsbValue::Float(100.0)),
        ]);
        assert!((effective_motion_time(&motion, 120.0) - 100.0).abs() < 1.0e-6);
    }

    #[test]
    fn native_type_two_frame_holds_instead_of_interpolating() {
        let frames = vec![
            test_frame(
                0.0,
                2,
                test_content(vec![("coord", test_coord(2.0, 3.0, 4.0))]),
            ),
            test_frame(
                10.0,
                3,
                test_content(vec![("coord", test_coord(12.0, 13.0, 14.0))]),
            ),
        ];
        let state = evaluate_frame_list(&frames, 5.0, None, 0, None);
        assert_eq!(state.coord, Some([2.0, 3.0, 4.0]));
        assert_eq!(state.interpolation_t, 0.0);
    }

    #[test]
    fn native_type_three_frame_interpolates_to_immediate_next() {
        let frames = vec![
            test_frame(
                0.0,
                3,
                test_content(vec![
                    ("coord", test_coord(0.0, 0.0, 0.0)),
                    ("angle", PsbValue::Float(350.0)),
                ]),
            ),
            test_frame(
                10.0,
                1,
                test_content(vec![
                    ("coord", test_coord(10.0, 20.0, 30.0)),
                    ("angle", PsbValue::Float(10.0)),
                ]),
            ),
        ];
        let state = evaluate_frame_list(&frames, 5.0, None, 0, None);
        assert_eq!(state.coord, Some([5.0, 10.0, 15.0]));
        assert!(state.rotation_degrees.abs() < 1.0e-6);
        assert!((state.interpolation_t - 0.5).abs() < 1.0e-6);
    }

    #[test]
    fn native_frame_ti_quantizes_elapsed_time_before_interpolation() {
        let frames = vec![
            test_frame(
                0.0,
                3,
                test_content(vec![
                    ("coord", test_coord(0.0, 0.0, 0.0)),
                    ("ti", PsbValue::Int(4)),
                ]),
            ),
            test_frame(
                10.0,
                1,
                test_content(vec![("coord", test_coord(10.0, 0.0, 0.0))]),
            ),
        ];
        let state = evaluate_frame_list(&frames, 7.0, None, 0, None);
        assert_eq!(state.coord, Some([4.0, 0.0, 0.0]));
        assert!((state.interpolation_t - 0.4).abs() < 1.0e-6);
    }

    #[test]
    fn native_opacity_interpolation_rounds_to_nearest_byte() {
        let frames = vec![
            test_frame(
                0.0,
                3,
                test_content(vec![("opa", PsbValue::Int(0))]),
            ),
            test_frame(
                10.0,
                1,
                test_content(vec![("opa", PsbValue::Int(255))]),
            ),
        ];
        let state = evaluate_frame_list(&frames, 5.0, None, 0, None);
        // sub_1032FB00 -> sub_103A5960 -> sub_103A6180: 127.5 is rounded
        // through floor(value + 0.5), producing byte value 128.
        assert_eq!(state.opa, 128.0);
    }

    #[test]
    fn frame_contents_do_not_accumulate_across_keyframes() {
        let frames = vec![
            test_frame(
                0.0,
                1,
                test_content(vec![
                    ("src", PsbValue::String("texture_a".to_owned())),
                    ("angle", PsbValue::Float(45.0)),
                ]),
            ),
            test_frame(
                10.0,
                1,
                test_content(vec![("coord", test_coord(3.0, 4.0, 5.0))]),
            ),
        ];
        let state = evaluate_frame_list(&frames, 10.0, None, 0, None);
        assert_eq!(state.src, None);
        assert!((state.rotation_degrees - 0.0).abs() < 1.0e-6);
        assert_eq!(state.coord, Some([3.0, 4.0, 5.0]));
    }

    fn identity_path_spline() -> PsbValue {
        PsbValue::Object(vec![
            (
                "x".to_owned(),
                PsbValue::List(vec![PsbValue::Float(0.0), PsbValue::Float(1.0)]),
            ),
            (
                "y".to_owned(),
                PsbValue::List(vec![PsbValue::Float(0.0), PsbValue::Float(1.0)]),
            ),
            (
                "p".to_owned(),
                PsbValue::List(vec![PsbValue::Float(0.0), PsbValue::Float(0.0)]),
            ),
        ])
    }

    fn test_inline_cp_path() -> PsbValue {
        PsbValue::Object(vec![
            (
                "x".to_owned(),
                PsbValue::List(vec![
                    PsbValue::Float(0.0),
                    PsbValue::Float(1.0 / 3.0),
                    PsbValue::Float(2.0 / 3.0),
                    PsbValue::Float(1.0),
                ]),
            ),
            (
                "y".to_owned(),
                PsbValue::List(vec![
                    PsbValue::Float(0.0),
                    PsbValue::Float(1.0),
                    PsbValue::Float(1.0),
                    PsbValue::Float(0.0),
                ]),
            ),
            (
                "t".to_owned(),
                PsbValue::List(vec![PsbValue::Float(0.0), PsbValue::Float(1.0)]),
            ),
            ("s".to_owned(), PsbValue::List(vec![identity_path_spline()])),
        ])
    }

    #[test]
    fn nested_direction_mode_three_uses_native_cp_path_tangent() {
        let motion = PsbValue::Object(vec![
            ("dt".to_owned(), PsbValue::Int(3)),
            ("dofst".to_owned(), PsbValue::Float(0.0)),
        ]);
        let frames = vec![
            test_frame(
                0.0,
                3,
                test_content(vec![
                    ("coord", test_coord(0.0, 0.0, 0.0)),
                    ("cp", test_inline_cp_path()),
                    ("motion", motion),
                ]),
            ),
            test_frame(
                10.0,
                1,
                test_content(vec![("coord", test_coord(10.0, 0.0, 0.0))]),
            ),
        ];
        let state = evaluate_frame_list(&frames, 2.5, None, 0, None);
        let angle = state.motion_path_tangent_degrees.unwrap();
        let expected = 1.5f32.atan2(1.0).to_degrees();
        assert!((angle - expected).abs() < 0.05, "{angle} vs {expected}");
    }

    #[test]
    fn nested_direction_mode_two_uses_previous_stepframe_displacement() {
        let mut state = DynamicFrameState::default();
        state.motion_direction_type = 2;
        state.motion_direction_offset_degrees = 10.0;
        let angle = nested_motion_direction_degrees(
            &state,
            0.0,
            Some(0),
            Some([2.0, 1.0, 0.0]),
            Some([5.0, 5.0, 0.0]),
            None,
        )
        .unwrap();
        let expected = 1.0f32.atan2(2.0).to_degrees() + 10.0;
        assert!((angle - expected).abs() < 1.0e-5);
    }

    #[test]
    fn nested_direction_mode_four_faces_finalized_target_on_xz_plane() {
        let mut state = DynamicFrameState::default();
        state.motion_direction_type = 4;
        state.motion_direction_offset_degrees = -5.0;
        let angle = nested_motion_direction_degrees(
            &state,
            0.0,
            Some(1),
            None,
            Some([2.0, 7.0, 3.0]),
            Some([6.0, -100.0, 7.0]),
        )
        .unwrap();
        let expected = (4.0f32.atan2(4.0).to_degrees() - 5.0).rem_euclid(360.0);
        assert!((angle - expected).abs() < 1.0e-5);
    }

    #[test]
    fn nested_fixed_direction_post_multiplies_composed_layer_matrix() {
        let mut ctx = TravelContext::default();
        ctx.transform = EmoteTransform2D::scale(2.0, 1.0);
        ctx.linear_state = FrameLinearState {
            rotation_degrees: 0.0,
            scale_x: 2.0,
            scale_y: 1.0,
            ..FrameLinearState::default()
        };
        let mut state = DynamicFrameState::default();
        state.motion_direction_type = 1;
        state.motion_direction_offset_degrees = 90.0;
        apply_nested_motion_direction(&mut ctx, &state);
        // Native sub_10355BF0 uses composed_layer_matrix * R(delta), not a
        // rebuild with angle=90 inside transformOrder. For S(2,1)*R(90):
        // [ 0 -2 ; 1 0 ].
        assert!(ctx.transform.m11.abs() < 1.0e-6);
        assert!((ctx.transform.m12 + 2.0).abs() < 1.0e-6);
        assert!((ctx.transform.m21 - 1.0).abs() < 1.0e-6);
        assert!(ctx.transform.m22.abs() < 1.0e-6);
        assert!((ctx.linear_state.rotation_degrees - 90.0).abs() < 1.0e-6);
    }

    #[test]
    fn nested_direction_reverses_rotation_delta_under_single_axis_flip() {
        let mut ctx = TravelContext::default();
        ctx.transform = EmoteTransform2D::scale(2.0, 1.0);
        ctx.linear_state.flip_x = true;
        let mut state = DynamicFrameState::default();
        state.motion_direction_type = 1;
        state.motion_direction_offset_degrees = 90.0;
        apply_nested_motion_direction(&mut ctx, &state);
        assert!(ctx.transform.m11.abs() < 1.0e-6);
        assert!((ctx.transform.m12 - 2.0).abs() < 1.0e-6);
        assert!((ctx.transform.m21 + 1.0).abs() < 1.0e-6);
        assert!(ctx.transform.m22.abs() < 1.0e-6);
    }

    #[test]
    fn native_inline_cp_evaluates_cubic_bezier_after_parameter_spline() {
        let [u, v] = evaluate_native_beziers_path(&test_inline_cp_path(), 0.5).unwrap();
        assert!((u - 0.5).abs() < 1.0e-6);
        assert!((v - 0.75).abs() < 1.0e-6);
    }

    #[test]
    fn native_cp_maps_xy_plane_and_linearly_interpolates_z() {
        let path = test_inline_cp_path();
        let result = interpolate_native_coordinate(
            [0.0, 0.0, 0.0],
            [10.0, 0.0, 4.0],
            0.5,
            0,
            Some(&path),
        );
        assert!((result[0] - 5.0).abs() < 1.0e-6);
        assert!((result[1] - 7.5).abs() < 1.0e-6);
        assert!((result[2] - 2.0).abs() < 1.0e-6);
    }

    #[test]
    fn native_cp_maps_xz_plane_and_linearly_interpolates_y() {
        let path = test_inline_cp_path();
        let result = interpolate_native_coordinate(
            [0.0, 0.0, 0.0],
            [10.0, 3.0, 0.0],
            0.5,
            1,
            Some(&path),
        );
        assert!((result[0] - 5.0).abs() < 1.0e-6);
        assert!((result[1] - 1.5).abs() < 1.0e-6);
        assert!((result[2] - 7.5).abs() < 1.0e-6);
    }

    #[test]
    fn native_packed_color_uses_integer_lane_interpolation() {
        assert_eq!(interpolate_native_packed_color(0x0000_0000, 0xFFFF_FFFF, 0.0), 0x0000_0000);
        assert_eq!(interpolate_native_packed_color(0x0000_0000, 0xFFFF_FFFF, 0.5), 0x7F7F_7F7F);
        assert_eq!(interpolate_native_packed_color(0x0000_0000, 0xFFFF_FFFF, 1.0), 0xFFFF_FFFF);
        assert_eq!(interpolate_native_packed_color(0x1234_5678, 0x1234_5678, 0.25), 0x1234_5678);
    }

    #[test]
    fn native_stencil_wipe_derives_scale_and_bias() {
        let wipe = StencilWipeFrameState::from_native(true, false, 0.25, 0.0);
        assert!((wipe.scale - 1024.0).abs() < 1.0e-6);
        assert!((wipe.bias - (1.0 - 1025.0 * 0.25)).abs() < 1.0e-5);

        let soft = StencilWipeFrameState::from_native(true, true, 0.5, 1.0);
        assert!((soft.scale - 1.0).abs() < 1.0e-6);
        assert!(soft.bias.abs() < 1.0e-6);

        let disabled = StencilWipeFrameState::from_native(false, true, 0.5, 1.0);
        assert_eq!(disabled.scale, 0.0);
        assert_eq!(disabled.bias, 0.0);
    }

    #[test]
    fn particle_trigger_dirty_tracks_serialized_frame_switch_not_interpolation() {
        let mut previous = DynamicFrameState::default();
        previous.frame_index = Some(3);
        previous.serialized_frame_type = 3;
        previous.interpolation_t = 0.1;

        let mut same_key = previous.clone();
        same_key.interpolation_t = 0.9;
        same_key.opa = 42.0;
        assert!(!serialized_frame_transition_dirty(Some(&previous), &same_key));

        let mut next_key = same_key.clone();
        next_key.frame_index = Some(4);
        assert!(serialized_frame_transition_dirty(Some(&previous), &next_key));

        let mut hold = next_key;
        hold.serialized_frame_type = 0;
        assert!(!serialized_frame_transition_dirty(Some(&previous), &hold));
        assert!(serialized_frame_transition_dirty(None, &previous));
    }

    #[test]
    fn particle_tri_volume_scale_matches_native_sqrt_abs_determinant() {
        // sub_10357200 -> sub_1038CD60(det) -> fabs -> sqrt.
        assert!((particle_transform_scale([2.0, 0.0, 0.0, 8.0, 0.0, 0.0]) - 4.0).abs() < 1.0e-6);
        // Unit-determinant shear must not change the tri-volume Z scale.
        assert!((particle_transform_scale([1.0, 3.0, 0.0, 1.0, 0.0, 0.0]) - 1.0).abs() < 1.0e-6);
        // Native fabs makes reflections use the magnitude of the determinant.
        assert!((particle_transform_scale([-2.0, 0.0, 0.0, 3.0, 0.0, 0.0]) - 6.0f32.sqrt()).abs() < 1.0e-6);
    }

    #[test]
    fn particle_frame_defaults_match_native_decoder() {
        assert_eq!(
            ParticleFrameState::default(),
            ParticleFrameState {
                trigger: 0,
                fmin: 10.0,
                fmax: 10.0,
                vmin: 0.0,
                vmax: 0.0,
                amin: 0.0,
                amax: 0.0,
                zmin: 1.0,
                zmax: 1.0,
                range: 0.0,
            }
        );
    }

    #[test]
    fn native_opacity_uses_byte_scale() {
        let ctx = TravelContext::default();
        assert!((ctx_with_opacity(ctx.clone(), 255.0).opacity_multiplier - 1.0).abs() < 1.0e-6);
        assert!((ctx_with_opacity(ctx, 128.0).opacity_multiplier - (128.0 / 255.0)).abs() < 1.0e-6);
    }

    #[test]
    fn computes_bounds() {
        let sprite = EmoteStaticSprite {
            label: None,
            motion_name: "main".to_owned(),
            texture_name: "tex".to_owned(),
            texture_resource_index: 0,
            texture_width: 100,
            texture_height: 100,
            texture_format: Some("RGBA8".to_owned()),
            icon_name: "icon".to_owned(),
            feedback_history: false,
            z: 0.0,
            opacity: 1.0,
            blend_mode: 0x10,
            blend_parameter: 0.0,
            corner_colors: [0x8080_80FF; 4],
            visible: true,
            center_x: 10.0,
            center_y: 20.0,
            width: 40.0,
            height: 60.0,
            scale_x: 1.0,
            scale_y: 1.0,
            rotation_degrees: 0.0,
            world_transform: EmoteTransform2D::identity().as_array(),
            uv_left: 0.0,
            uv_top: 0.0,
            uv_right: 1.0,
            uv_bottom: 1.0,
            mesh: None,
            draw_frame_info: draw_frame_info(None, &TravelContext::default()),
        };
        let b = compute_bounds(&[sprite]).unwrap();
        assert_eq!(b.min_x, -10.0);
        assert_eq!(b.max_y, 50.0);
    }
}
