use std::collections::HashMap;
use std::fs;
use std::io::{Cursor as IoCursor, Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MeshLightingType {
    #[default]
    None = 0,
    Lambert = 1,
    BlinnPhong = 2,
    PerPixelBlinnPhong = 3,
    PerPixelHalfLambert = 4,
    Toon = 5,
    FixedFunction = 6,
    PerPixelFixedFunction = 7,
    Bump = 8,
    Parallax = 9,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MeshShadingType {
    #[default]
    None = 0,
    DepthBuffer = 1,
}

pub const MESH_SHADER_OPTION_NONE: u32 = 0;
pub const MESH_SHADER_OPTION_RIM_LIGHT: u32 = 1 << 0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MeshEffectProfile {
    #[default]
    None = 0,
    Mesh = 1,
    SkinnedMesh = 2,
    ShadowMap = 3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MeshRuntimeMaterialKey {
    pub use_mesh_tex: bool,
    pub use_shadow_tex: bool,
    pub use_toon_tex: bool,
    pub use_normal_tex: bool,
    pub use_mul_vertex_color: bool,
    pub use_mrbd: bool,
    pub use_rgb: bool,
    pub lighting_type: MeshLightingType,
    pub shading_type: MeshShadingType,
    pub shader_option: u32,
    pub skinned: bool,
    pub alpha_test_enable: bool,
    pub cull_disable: bool,
    pub shadow_map_enable: bool,
}

#[derive(Debug, Clone, Default)]
pub struct MeshPrimitiveRuntimeDesc {
    pub effect_profile: MeshEffectProfile,
    pub effect_key: String,
    pub technique_name: String,
    pub shadow_effect_key: String,
    pub shadow_technique_name: String,
    pub use_mesh_texture_slot: bool,
    pub use_normal_texture_slot: bool,
    pub use_toon_texture_slot: bool,
    pub use_shadow_texture_slot: bool,
    pub material_key: MeshRuntimeMaterialKey,
    pub vertex_stride_bytes: u32,
    pub vertex_count: u32,
    pub bone_palette_len: u32,
}

#[derive(Debug, Clone, Default)]
pub struct MeshTriVertex {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub normal: [f32; 3],
    pub tangent: [f32; 3],
    pub binormal: [f32; 3],
    pub color: [f32; 4],
    pub bone_indices: [u16; 4],
    pub bone_weights: [f32; 4],
}

#[derive(Debug, Clone, Default)]
pub struct MeshMaterial {
    pub diffuse: [f32; 4],
    pub ambient: [f32; 4],
    pub specular: [f32; 4],
    pub emissive: [f32; 4],
    pub power: f32,
    pub lighting_type: MeshLightingType,
    pub shading_type: MeshShadingType,
    pub shader_option: u32,
    pub rim_light_color: [f32; 4],
    pub rim_light_power: f32,
    pub parallax_max_height: f32,
    pub alpha_test_enable: bool,
    pub alpha_ref: f32,
    pub cull_disable: bool,
    pub shadow_map_enable: bool,
    pub use_mesh_tex: bool,
    pub use_mrbd: bool,
    pub mrbd: [f32; 4],
    pub use_rgb: bool,
    pub rgb_rate: [f32; 4],
    pub add_rgb: [f32; 4],
    pub use_mul_vertex_color: bool,
    pub mul_vertex_color_rate: f32,
    pub depth_buffer_shadow_bias: f32,
    pub directional_light_ids: Vec<i32>,
    pub point_light_ids: Vec<i32>,
    pub spot_light_ids: Vec<i32>,
    pub normal_texture_path: Option<PathBuf>,
    pub toon_texture_path: Option<PathBuf>,
    pub effect_filename: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct MeshGpuPrimitiveBatch {
    pub vertices: Vec<MeshTriVertex>,
    pub frame_cols: [[f32; 4]; 4],
    pub bone_cols: Vec<[[f32; 4]; 4]>,
    pub skinned: bool,
    pub texture_path: Option<PathBuf>,
    pub material: MeshMaterial,
    pub runtime_desc: MeshPrimitiveRuntimeDesc,
}

#[derive(Debug, Clone)]
pub struct MeshAnimationState {
    pub clip_name: Option<String>,
    pub clip_index: Option<usize>,
    pub blend_clip_name: Option<String>,
    pub blend_clip_index: Option<usize>,
    pub blend_weight: f32,
    /// Object-local animation controller time, advanced by runtime tick.
    pub time_sec: f32,
    pub rate: f32,
    pub time_offset_sec: f32,
    /// Frozen controller sample time used while paused.
    pub hold_time_sec: f32,
    pub paused: bool,
    pub looped: bool,
    /// tona3-like controller state.
    pub anim_track_no: u32,
    pub anim_shift_time_sec: f32,
    pub is_anim: bool,
    pub prev_clip_name: Option<String>,
    pub prev_clip_index: Option<usize>,
    pub prev_time_sec: f32,
    pub prev_time_offset_sec: f32,
    pub prev_rate: f32,
    pub transition_elapsed_sec: f32,
}

pub const MESH_ANIM_CONTROLLER_STEP_SEC: f32 = 60.0 / 4800.0;

impl Default for MeshAnimationState {
    fn default() -> Self {
        Self {
            clip_name: None,
            clip_index: None,
            blend_clip_name: None,
            blend_clip_index: None,
            blend_weight: 0.0,
            time_sec: 0.0,
            rate: 1.0,
            time_offset_sec: 0.0,
            hold_time_sec: 0.0,
            paused: false,
            looped: true,
            anim_track_no: 0,
            anim_shift_time_sec: 0.0,
            is_anim: true,
            prev_clip_name: None,
            prev_clip_index: None,
            prev_time_sec: 0.0,
            prev_time_offset_sec: 0.0,
            prev_rate: 1.0,
            transition_elapsed_sec: 0.0,
        }
    }
}

impl MeshAnimationState {
    pub fn sanitized(&self) -> Self {
        let mut out = self.clone();
        out.blend_weight = out.blend_weight.clamp(0.0, 1.0);
        out.time_sec = out.time_sec.max(0.0);
        out.rate = out.rate.max(0.0);
        out.time_offset_sec = out.time_offset_sec.max(0.0);
        out.hold_time_sec = out.hold_time_sec.max(0.0);
        out.anim_shift_time_sec = out.anim_shift_time_sec.max(0.0);
        out.prev_time_sec = out.prev_time_sec.max(0.0);
        out.prev_time_offset_sec = out.prev_time_offset_sec.max(0.0);
        out.prev_rate = out.prev_rate.max(0.0);
        out.transition_elapsed_sec = out.transition_elapsed_sec.max(0.0);
        if out.anim_shift_time_sec <= 0.0 {
            out.prev_clip_name = None;
            out.prev_clip_index = None;
            out.prev_time_sec = 0.0;
            out.prev_time_offset_sec = 0.0;
            out.prev_rate = 1.0;
            out.transition_elapsed_sec = 0.0;
        }
        out
    }

    pub fn current_sample_base_sec(&self) -> f32 {
        if self.paused || !self.is_anim {
            self.hold_time_sec.max(0.0)
        } else {
            self.time_sec.max(0.0) * self.rate.max(0.0)
        }
    }

    pub fn current_sample_time_sec(&self) -> f32 {
        self.current_sample_base_sec() + self.time_offset_sec.max(0.0)
    }

    pub fn previous_track_sample_time_sec(&self) -> f32 {
        (self.prev_time_sec.max(0.0) * self.prev_rate.max(0.0)) + self.prev_time_offset_sec.max(0.0)
    }

    pub fn previous_track_weight(&self) -> f32 {
        if (self.prev_clip_name.is_none() && self.prev_clip_index.is_none())
            || self.anim_shift_time_sec <= 0.0
        {
            0.0
        } else {
            (1.0 - (self.transition_elapsed_sec / self.anim_shift_time_sec)).clamp(0.0, 1.0)
        }
    }

    pub fn current_track_weight(&self) -> f32 {
        (1.0 - self.previous_track_weight()).clamp(0.0, 1.0)
    }

    pub fn previous_track_speed(&self) -> f32 {
        if (self.prev_clip_name.is_none() && self.prev_clip_index.is_none())
            || self.anim_shift_time_sec <= 0.0
        {
            0.0
        } else {
            (1.0 - (self.transition_elapsed_sec / self.anim_shift_time_sec)).clamp(0.0, 1.0)
        }
    }

    pub fn current_track_speed(&self) -> f32 {
        if (self.prev_clip_name.is_none() && self.prev_clip_index.is_none())
            || self.anim_shift_time_sec <= 0.0
        {
            1.0
        } else {
            (self.transition_elapsed_sec / self.anim_shift_time_sec).clamp(0.0, 1.0)
        }
    }

    pub fn set_anim_shift_time_sec(&mut self, shift_sec: f32) {
        self.anim_shift_time_sec = shift_sec.max(0.0);
        *self = self.sanitized();
    }

    pub fn change_animation_clip(
        &mut self,
        next_clip_name: Option<String>,
        next_clip_index: Option<usize>,
    ) {
        if self.clip_name == next_clip_name && self.clip_index == next_clip_index {
            return;
        }
        self.prev_clip_name = self.clip_name.clone();
        self.prev_clip_index = self.clip_index;
        self.prev_time_sec = self.time_sec.max(0.0);
        self.prev_time_offset_sec = self.time_offset_sec.max(0.0);
        self.prev_rate = self.rate.max(0.0);
        self.clip_name = next_clip_name;
        self.clip_index = next_clip_index;
        self.time_sec = 0.0;
        self.hold_time_sec = 0.0;
        self.transition_elapsed_sec = 0.0;
        self.anim_track_no = if self.anim_track_no == 0 { 1 } else { 0 };
        if self.anim_shift_time_sec <= 0.0 {
            self.prev_clip_name = None;
            self.prev_clip_index = None;
            self.prev_time_sec = 0.0;
            self.prev_time_offset_sec = 0.0;
            self.prev_rate = 1.0;
        }
        *self = self.sanitized();
    }

    pub fn advance_controller_frames(&mut self, delta_frames: i32) {
        let delta_sec = (delta_frames.max(0) as f32) * MESH_ANIM_CONTROLLER_STEP_SEC;
        if !self.paused && self.is_anim {
            let has_prev = self.prev_clip_name.is_some() || self.prev_clip_index.is_some();
            if has_prev && self.anim_shift_time_sec > 0.0 {
                let start = self.transition_elapsed_sec.max(0.0);
                let end = (start + delta_sec).min(self.anim_shift_time_sec);
                let active_delta = (end - start).max(0.0);
                if active_delta > 0.0 {
                    let shift = self.anim_shift_time_sec.max(0.000_001);
                    let cur_advance = ((end * end) - (start * start)) / (2.0 * shift);
                    let prev_advance = active_delta - cur_advance;
                    self.time_sec = (self.time_sec.max(0.0) + cur_advance.max(0.0)).max(0.0);
                    self.prev_time_sec =
                        (self.prev_time_sec.max(0.0) + prev_advance.max(0.0)).max(0.0);
                    self.transition_elapsed_sec = end.max(0.0);
                }
                if delta_sec > active_delta {
                    self.time_sec = (self.time_sec.max(0.0) + (delta_sec - active_delta)).max(0.0);
                }
                if self.transition_elapsed_sec >= self.anim_shift_time_sec {
                    self.prev_clip_name = None;
                    self.prev_clip_index = None;
                    self.prev_time_sec = 0.0;
                    self.prev_time_offset_sec = 0.0;
                    self.prev_rate = 1.0;
                    self.transition_elapsed_sec = 0.0;
                }
            } else {
                self.time_sec = (self.time_sec.max(0.0) + delta_sec).max(0.0);
                if has_prev {
                    self.prev_clip_name = None;
                    self.prev_clip_index = None;
                    self.prev_time_sec = 0.0;
                    self.prev_time_offset_sec = 0.0;
                    self.prev_rate = 1.0;
                    self.transition_elapsed_sec = 0.0;
                }
            }
        }
        *self = self.sanitized();
    }
}

#[derive(Debug, Clone, Default)]
pub struct MeshAsset {
    pub source_path: PathBuf,
    pub texture_path: Option<PathBuf>,
    pub vertices: Vec<MeshTriVertex>,
    pub bone_count: usize,
    pub ticks_per_second: f32,
    primitives: Vec<MeshPrimitive>,
    frames: Vec<FrameNode>,
    animations: Vec<AnimationClip>,
}

impl MeshAsset {
    pub fn bounds_size(&self) -> [f32; 3] {
        let vertices = if self.vertices.is_empty() {
            self.sample_vertices(0.0)
        } else {
            self.vertices.clone()
        };
        if vertices.is_empty() {
            return [0.0, 0.0, 0.0];
        }

        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        for v in vertices {
            for axis in 0..3 {
                min[axis] = min[axis].min(v.pos[axis]);
                max[axis] = max[axis].max(v.pos[axis]);
            }
        }
        [
            (max[0] - min[0]).max(0.0),
            (max[1] - min[1]).max(0.0),
            (max[2] - min[2]).max(0.0),
        ]
    }

    pub fn is_skinned(&self) -> bool {
        self.bone_count != 0
            && self.primitives.iter().any(|p| {
                !p.bones.is_empty()
                    && p.vertices
                        .iter()
                        .any(|v| v.bone_weights.iter().copied().sum::<f32>() > 0.0)
            })
    }

    pub fn duration_seconds(&self) -> f32 {
        self.animations
            .iter()
            .map(|a| self.seconds_from_ticks(a.max_time))
            .fold(0.0, f32::max)
    }

    fn seconds_from_ticks(&self, ticks: i64) -> f32 {
        ticks as f32 / self.ticks_per_second.max(1.0)
    }

    fn ticks_from_seconds(&self, sec: f32) -> i64 {
        (sec.max(0.0) * self.ticks_per_second.max(1.0)) as i64
    }

    pub fn animation_clip_names(&self) -> Vec<String> {
        self.animations.iter().map(|a| a.name.clone()).collect()
    }

    fn resolve_clip_index(
        &self,
        clip_name: Option<&str>,
        clip_index: Option<usize>,
    ) -> Option<usize> {
        if let Some(name) = clip_name {
            if let Some((idx, _)) = self
                .animations
                .iter()
                .enumerate()
                .find(|(_, c)| c.name.eq_ignore_ascii_case(name))
            {
                return Some(idx);
            }
        }
        if let Some(idx) = clip_index {
            if idx < self.animations.len() {
                return Some(idx);
            }
        }
        if self.animations.is_empty() {
            None
        } else {
            Some(0)
        }
    }

    pub fn sample_vertices_with_clip(
        &self,
        time_sec: f32,
        clip_name: Option<&str>,
        clip_index: Option<usize>,
    ) -> Vec<MeshTriVertex> {
        if self.primitives.is_empty() {
            return self.vertices.clone();
        }
        let pose = PoseState::new_with_clip(
            self,
            time_sec,
            self.resolve_clip_index(clip_name, clip_index),
        );
        let mut out = Vec::new();
        for prim in &self.primitives {
            let frame_world = pose
                .combined
                .get(prim.frame_index)
                .copied()
                .unwrap_or(Mat4::identity());
            for src in &prim.vertices {
                let (pos, normal) = if prim.bones.is_empty() {
                    (
                        frame_world.transform_point(src.pos),
                        frame_world.transform_vector(src.normal),
                    )
                } else {
                    skin_vertex(src, prim, &pose)
                };
                out.push(MeshTriVertex {
                    pos,
                    uv: src.uv,
                    normal: normalize3(normal),
                    tangent: src.tangent,
                    binormal: src.binormal,
                    color: src.color,
                    bone_indices: src.bone_indices,
                    bone_weights: src.bone_weights,
                });
            }
        }
        out
    }

    pub fn sample_vertices(&self, time_sec: f32) -> Vec<MeshTriVertex> {
        self.sample_vertices_with_clip(time_sec, None, None)
    }

    pub fn sample_gpu_primitives_with_clip(
        &self,
        time_sec: f32,
        clip_name: Option<&str>,
        clip_index: Option<usize>,
    ) -> Vec<MeshGpuPrimitiveBatch> {
        if self.primitives.is_empty() {
            return vec![MeshGpuPrimitiveBatch {
                vertices: self.vertices.clone(),
                frame_cols: Mat4::identity().m,
                bone_cols: Vec::new(),
                skinned: false,
                texture_path: self.texture_path.clone(),
                material: default_mesh_material(),
                runtime_desc: build_mesh_primitive_runtime_desc_from_material(
                    &default_mesh_material(),
                    self.texture_path.is_some(),
                    false,
                    self.vertices.len() as u32,
                    0,
                ),
            }];
        }
        let pose = PoseState::new_with_clip(
            self,
            time_sec,
            self.resolve_clip_index(clip_name, clip_index),
        );
        let mut out = Vec::with_capacity(self.primitives.len());
        for prim in &self.primitives {
            let frame_world = pose
                .combined
                .get(prim.frame_index)
                .copied()
                .unwrap_or(Mat4::identity());
            let mut bone_cols = Vec::with_capacity(prim.bones.len());
            for bone in &prim.bones {
                bone_cols.push(skin_matrix_for_bone(bone, &pose).m);
            }
            out.push(MeshGpuPrimitiveBatch {
                vertices: prim.vertices.clone(),
                frame_cols: frame_world.m,
                bone_cols,
                skinned: !prim.bones.is_empty(),
                texture_path: prim.texture_path.clone(),
                material: prim.material.clone(),
                runtime_desc: prim.runtime_desc.clone(),
            });
        }
        out
    }

    pub fn sample_gpu_primitives(&self, time_sec: f32) -> Vec<MeshGpuPrimitiveBatch> {
        self.sample_gpu_primitives_with_clip(time_sec, None, None)
    }

    pub fn sample_gpu_primitives_with_state(
        &self,
        state: &MeshAnimationState,
    ) -> Vec<MeshGpuPrimitiveBatch> {
        if self.primitives.is_empty() {
            return vec![MeshGpuPrimitiveBatch {
                vertices: self.vertices.clone(),
                frame_cols: Mat4::identity().m,
                bone_cols: Vec::new(),
                skinned: false,
                texture_path: self.texture_path.clone(),
                material: default_mesh_material(),
                runtime_desc: build_mesh_primitive_runtime_desc_from_material(
                    &default_mesh_material(),
                    self.texture_path.is_some(),
                    false,
                    self.vertices.len() as u32,
                    0,
                ),
            }];
        }
        let pose = PoseState::new_with_state(self, state);
        let mut out = Vec::with_capacity(self.primitives.len());
        for prim in &self.primitives {
            let frame_world = pose
                .combined
                .get(prim.frame_index)
                .copied()
                .unwrap_or(Mat4::identity());
            let mut bone_cols = Vec::with_capacity(prim.bones.len());
            for bone in &prim.bones {
                bone_cols.push(skin_matrix_for_bone(bone, &pose).m);
            }
            out.push(MeshGpuPrimitiveBatch {
                vertices: prim.vertices.clone(),
                frame_cols: frame_world.m,
                bone_cols,
                skinned: !prim.bones.is_empty(),
                texture_path: prim.texture_path.clone(),
                material: prim.material.clone(),
                runtime_desc: prim.runtime_desc.clone(),
            });
        }
        out
    }
}

#[derive(Debug, Clone)]
struct MeshPrimitive {
    frame_index: usize,
    texture_path: Option<PathBuf>,
    material: MeshMaterial,
    runtime_desc: MeshPrimitiveRuntimeDesc,
    vertices: Vec<MeshTriVertex>,
    bones: Vec<BoneBinding>,
}

#[derive(Debug, Clone)]
struct BoneBinding {
    frame_name: String,
    frame_index: usize,
    offset_matrix: Mat4,
}

#[derive(Debug, Clone)]
struct FrameNode {
    name: String,
    base_local: Mat4,
    children: Vec<usize>,
}

#[derive(Debug, Clone, Default)]
struct AnimationClip {
    name: String,
    tracks: HashMap<String, AnimationTrack>,
    max_time: i64,
    open_closed: bool,
}

#[derive(Debug, Clone, Default)]
struct AnimationTrack {
    rotation_keys: Vec<QuatKey>,
    scale_keys: Vec<Vec3Key>,
    position_keys: Vec<Vec3Key>,
    matrix_keys: Vec<MatKey>,
}

#[derive(Debug, Clone, Copy)]
struct Vec3Key {
    time: i64,
    value: [f32; 3],
}

#[derive(Debug, Clone, Copy)]
struct QuatKey {
    time: i64,
    value: Quat,
}

#[derive(Debug, Clone, Copy)]
struct MatKey {
    time: i64,
    value: Mat4,
}

#[derive(Debug, Clone)]
struct PoseState {
    combined: Vec<Mat4>,
    frame_lookup: HashMap<String, usize>,
}

impl PoseState {
    fn new(asset: &MeshAsset, time_sec: f32) -> Self {
        Self::new_with_clip(
            asset,
            time_sec,
            if asset.animations.is_empty() {
                None
            } else {
                Some(0)
            },
        )
    }

    fn new_with_clip(asset: &MeshAsset, time_sec: f32, clip_index: Option<usize>) -> Self {
        let mut state = MeshAnimationState::default();
        state.time_sec = time_sec;
        state.clip_index = clip_index;
        Self::new_with_state(asset, &state)
    }

    fn new_with_state(asset: &MeshAsset, state: &MeshAnimationState) -> Self {
        let frame_lookup: HashMap<String, usize> = asset
            .frames
            .iter()
            .enumerate()
            .map(|(i, f)| (f.name.clone(), i))
            .collect();
        let clip = asset
            .resolve_clip_index(state.clip_name.as_deref(), state.clip_index)
            .and_then(|idx| asset.animations.get(idx));
        let blend_clip = asset
            .resolve_clip_index(state.blend_clip_name.as_deref(), state.blend_clip_index)
            .and_then(|idx| asset.animations.get(idx));
        let prev_clip = asset
            .resolve_clip_index(state.prev_clip_name.as_deref(), state.prev_clip_index)
            .and_then(|idx| asset.animations.get(idx));
        let primary_time = animation_time_for_state(state);
        let prev_time = state.previous_track_sample_time_sec();
        let prev_weight = state.previous_track_weight();
        let cur_weight = state.current_track_weight();
        let blend_weight = state.blend_weight.clamp(0.0, 1.0);
        let mut local = Vec::with_capacity(asset.frames.len());
        for frame in &asset.frames {
            let current = if let Some(clip) = clip {
                if let Some(track) = clip.tracks.get(&frame.name) {
                    sample_track(
                        track,
                        frame.base_local,
                        primary_time,
                        clip.max_time,
                        state.looped && clip.open_closed,
                        asset.ticks_per_second,
                    )
                } else {
                    frame.base_local
                }
            } else {
                frame.base_local
            };
            let mut primary = current;
            if prev_weight > 0.0 {
                if let Some(prev_clip) = prev_clip {
                    let previous = if let Some(prev_track) = prev_clip.tracks.get(&frame.name) {
                        sample_track(
                            prev_track,
                            frame.base_local,
                            prev_time,
                            prev_clip.max_time,
                            state.looped && prev_clip.open_closed,
                            asset.ticks_per_second,
                        )
                    } else {
                        frame.base_local
                    };
                    let normalized_prev = if (cur_weight + prev_weight) > 0.0 {
                        prev_weight / (cur_weight + prev_weight)
                    } else {
                        0.0
                    };
                    primary = blend_mats(current, previous, normalized_prev);
                }
            }
            let mat = if blend_weight > 0.0 {
                if let Some(clip_b) = blend_clip {
                    let secondary = if let Some(track_b) = clip_b.tracks.get(&frame.name) {
                        sample_track(
                            track_b,
                            frame.base_local,
                            primary_time,
                            clip_b.max_time,
                            state.looped && clip_b.open_closed,
                            asset.ticks_per_second,
                        )
                    } else {
                        frame.base_local
                    };
                    blend_mats(primary, secondary, blend_weight)
                } else {
                    primary
                }
            } else {
                primary
            };
            local.push(mat);
        }
        let mut combined = vec![Mat4::identity(); asset.frames.len()];
        if !asset.frames.is_empty() {
            let mut has_parent = vec![false; asset.frames.len()];
            for frame in &asset.frames {
                for &child in &frame.children {
                    if child < has_parent.len() {
                        has_parent[child] = true;
                    }
                }
            }
            for frame_idx in 0..asset.frames.len() {
                if !has_parent[frame_idx] {
                    update_combined_recursive(
                        &asset.frames,
                        &local,
                        &mut combined,
                        frame_idx,
                        None,
                    );
                }
            }
        }
        Self {
            combined,
            frame_lookup,
        }
    }
}

fn update_combined_recursive(
    frames: &[FrameNode],
    local: &[Mat4],
    combined: &mut [Mat4],
    frame_idx: usize,
    parent: Option<Mat4>,
) {
    let l = local.get(frame_idx).copied().unwrap_or(Mat4::identity());
    let c = if let Some(p) = parent { l.mul(&p) } else { l };
    combined[frame_idx] = c;
    if let Some(frame) = frames.get(frame_idx) {
        for &child in &frame.children {
            update_combined_recursive(frames, local, combined, child, Some(c));
        }
    }
}

fn animation_time_for_state(state: &MeshAnimationState) -> f32 {
    state.current_sample_time_sec()
}

fn blend_mats(a: Mat4, b: Mat4, w: f32) -> Mat4 {
    let w = w.clamp(0.0, 1.0);
    if w <= 0.0 {
        return a;
    }
    if w >= 1.0 {
        return b;
    }
    let (sa, ra, pa) = a.decompose_srt();
    let (sb, rb, pb) = b.decompose_srt();
    Mat4::from_scale_rotation_translation(
        lerp3(sa, sb, w),
        Quat::nlerp(ra, rb, w).normalize(),
        lerp3(pa, pb, w),
    )
}

fn sample_track(
    track: &AnimationTrack,
    base_local: Mat4,
    time_sec: f32,
    clip_max_time: i64,
    looped: bool,
    ticks_per_second: f32,
) -> Mat4 {
    let mut t = (time_sec.max(0.0) * ticks_per_second.max(1.0)) as i64;
    if clip_max_time > 0 {
        let max_time = clip_max_time.max(1);
        if looped {
            t %= max_time;
        } else {
            t = t.clamp(0, max_time);
        }
    }
    if !track.matrix_keys.is_empty() {
        return sample_mat_keys(&track.matrix_keys, t);
    }
    let (base_scale, base_rot, base_pos) = base_local.decompose_srt();
    let scale = sample_vec3_keys(&track.scale_keys, t).unwrap_or(base_scale);
    let rot = sample_quat_keys(&track.rotation_keys, t).unwrap_or(base_rot);
    let pos = sample_vec3_keys(&track.position_keys, t).unwrap_or(base_pos);
    Mat4::from_scale_rotation_translation(scale, rot, pos)
}

fn sample_vec3_keys(keys: &[Vec3Key], t: i64) -> Option<[f32; 3]> {
    if keys.is_empty() {
        return None;
    }
    if keys.len() == 1 {
        return Some(keys[0].value);
    }
    let (a, b, k) = find_key_pair_vec3(keys, t);
    Some(lerp3(a.value, b.value, k))
}

fn sample_quat_keys(keys: &[QuatKey], t: i64) -> Option<Quat> {
    if keys.is_empty() {
        return None;
    }
    if keys.len() == 1 {
        return Some(keys[0].value.normalize());
    }
    let (a, b, k) = find_key_pair_quat(keys, t);
    Some(Quat::nlerp(a.value, b.value, k).normalize())
}

fn sample_mat_keys(keys: &[MatKey], t: i64) -> Mat4 {
    if keys.is_empty() {
        return Mat4::identity();
    }
    if keys.len() == 1 {
        return keys[0].value;
    }
    let (a, b, k) = find_key_pair_mat(keys, t);
    Mat4::lerp(a.value, b.value, k)
}

fn find_key_pair_vec3(keys: &[Vec3Key], t: i64) -> (Vec3Key, Vec3Key, f32) {
    for w in keys.windows(2) {
        let a = w[0];
        let b = w[1];
        if t >= a.time && t <= b.time {
            let span = (b.time - a.time).max(1) as f32;
            return (a, b, (t - a.time) as f32 / span);
        }
    }
    let first = keys[0];
    let last = keys[keys.len() - 1];
    if t < first.time {
        (first, first, 0.0)
    } else {
        (last, last, 0.0)
    }
}

fn find_key_pair_quat(keys: &[QuatKey], t: i64) -> (QuatKey, QuatKey, f32) {
    for w in keys.windows(2) {
        let a = w[0];
        let b = w[1];
        if t >= a.time && t <= b.time {
            let span = (b.time - a.time).max(1) as f32;
            return (a, b, (t - a.time) as f32 / span);
        }
    }
    let first = keys[0];
    let last = keys[keys.len() - 1];
    if t < first.time {
        (first, first, 0.0)
    } else {
        (last, last, 0.0)
    }
}

fn find_key_pair_mat(keys: &[MatKey], t: i64) -> (MatKey, MatKey, f32) {
    for w in keys.windows(2) {
        let a = w[0];
        let b = w[1];
        if t >= a.time && t <= b.time {
            let span = (b.time - a.time).max(1) as f32;
            return (a, b, (t - a.time) as f32 / span);
        }
    }
    let first = keys[0];
    let last = keys[keys.len() - 1];
    if t < first.time {
        (first, first, 0.0)
    } else {
        (last, last, 0.0)
    }
}

fn skin_matrix_for_bone(bone: &BoneBinding, pose: &PoseState) -> Mat4 {
    let combined = pose
        .combined
        .get(bone.frame_index)
        .copied()
        .unwrap_or(Mat4::identity());
    bone.offset_matrix.mul(&combined)
}

fn skin_vertex(
    src: &MeshTriVertex,
    prim: &MeshPrimitive,
    pose: &PoseState,
) -> ([f32; 3], [f32; 3]) {
    let mut pos = [0.0f32; 3];
    let mut normal = [0.0f32; 3];
    let mut accum = 0.0f32;
    for lane in 0..4 {
        let weight = src.bone_weights[lane];
        if weight <= 0.0 {
            continue;
        }
        let bone_idx = src.bone_indices[lane] as usize;
        let Some(bone) = prim.bones.get(bone_idx) else {
            continue;
        };
        let skin_mat = skin_matrix_for_bone(bone, pose);
        let p = skin_mat.transform_point(src.pos);
        let n = skin_mat.transform_vector(src.normal);
        for i in 0..3 {
            pos[i] += p[i] * weight;
            normal[i] += n[i] * weight;
        }
        accum += weight;
    }
    if accum <= 1e-6 {
        let frame_world = pose
            .combined
            .get(prim.frame_index)
            .copied()
            .unwrap_or(Mat4::identity());
        (
            frame_world.transform_point(src.pos),
            frame_world.transform_vector(src.normal),
        )
    } else {
        (pos, normal)
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Quat {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}

impl Quat {
    fn normalize(self) -> Self {
        let len = (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt();
        if len <= 1e-8 {
            return Self {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            };
        }
        let inv = 1.0 / len;
        Self {
            x: self.x * inv,
            y: self.y * inv,
            z: self.z * inv,
            w: self.w * inv,
        }
    }

    fn nlerp(a: Self, mut b: Self, t: f32) -> Self {
        let mut dot = a.x * b.x + a.y * b.y + a.z * b.z + a.w * b.w;
        if dot < 0.0 {
            dot = -dot;
            b = Self {
                x: -b.x,
                y: -b.y,
                z: -b.z,
                w: -b.w,
            };
        }
        let k = t.clamp(0.0, 1.0);
        Self {
            x: a.x + (b.x - a.x) * k,
            y: a.y + (b.y - a.y) * k,
            z: a.z + (b.z - a.z) * k,
            w: a.w + (b.w - a.w) * k,
        }
        .normalize()
    }
}

#[derive(Debug, Clone, Copy)]
struct Mat4 {
    m: [[f32; 4]; 4],
}

impl Mat4 {
    fn identity() -> Self {
        Self {
            m: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
                [0.0, 0.0, 0.0, 1.0],
            ],
        }
    }

    fn from_x_values(vals: [f32; 16]) -> Self {
        Self {
            m: [
                [vals[0], vals[1], vals[2], vals[3]],
                [vals[4], vals[5], vals[6], vals[7]],
                [vals[8], vals[9], vals[10], vals[11]],
                [vals[12], vals[13], vals[14], vals[15]],
            ],
        }
    }

    fn mul(&self, rhs: &Self) -> Self {
        let mut out = [[0.0f32; 4]; 4];
        for r in 0..4 {
            for c in 0..4 {
                out[r][c] = self.m[r][0] * rhs.m[0][c]
                    + self.m[r][1] * rhs.m[1][c]
                    + self.m[r][2] * rhs.m[2][c]
                    + self.m[r][3] * rhs.m[3][c];
            }
        }
        Self { m: out }
    }

    fn transform_point(&self, p: [f32; 3]) -> [f32; 3] {
        [
            p[0] * self.m[0][0] + p[1] * self.m[1][0] + p[2] * self.m[2][0] + self.m[3][0],
            p[0] * self.m[0][1] + p[1] * self.m[1][1] + p[2] * self.m[2][1] + self.m[3][1],
            p[0] * self.m[0][2] + p[1] * self.m[1][2] + p[2] * self.m[2][2] + self.m[3][2],
        ]
    }

    fn transform_vector(&self, p: [f32; 3]) -> [f32; 3] {
        [
            p[0] * self.m[0][0] + p[1] * self.m[1][0] + p[2] * self.m[2][0],
            p[0] * self.m[0][1] + p[1] * self.m[1][1] + p[2] * self.m[2][1],
            p[0] * self.m[0][2] + p[1] * self.m[1][2] + p[2] * self.m[2][2],
        ]
    }

    fn lerp(a: Self, b: Self, t: f32) -> Self {
        let k = t.clamp(0.0, 1.0);
        let mut out = [[0.0f32; 4]; 4];
        for r in 0..4 {
            for c in 0..4 {
                out[r][c] = a.m[r][c] + (b.m[r][c] - a.m[r][c]) * k;
            }
        }
        Self { m: out }
    }

    fn from_scale_rotation_translation(scale: [f32; 3], rot: Quat, pos: [f32; 3]) -> Self {
        let q = rot.normalize();
        let xx = q.x * q.x;
        let yy = q.y * q.y;
        let zz = q.z * q.z;
        let xy = q.x * q.y;
        let xz = q.x * q.z;
        let yz = q.y * q.z;
        let wx = q.w * q.x;
        let wy = q.w * q.y;
        let wz = q.w * q.z;
        let mut m = Self::identity();
        m.m[0][0] = (1.0 - 2.0 * (yy + zz)) * scale[0];
        m.m[0][1] = (2.0 * (xy + wz)) * scale[0];
        m.m[0][2] = (2.0 * (xz - wy)) * scale[0];
        m.m[1][0] = (2.0 * (xy - wz)) * scale[1];
        m.m[1][1] = (1.0 - 2.0 * (xx + zz)) * scale[1];
        m.m[1][2] = (2.0 * (yz + wx)) * scale[1];
        m.m[2][0] = (2.0 * (xz + wy)) * scale[2];
        m.m[2][1] = (2.0 * (yz - wx)) * scale[2];
        m.m[2][2] = (1.0 - 2.0 * (xx + yy)) * scale[2];
        m.m[3][0] = pos[0];
        m.m[3][1] = pos[1];
        m.m[3][2] = pos[2];
        m
    }

    fn decompose_srt(&self) -> ([f32; 3], Quat, [f32; 3]) {
        let pos = [self.m[3][0], self.m[3][1], self.m[3][2]];
        let sx = (self.m[0][0] * self.m[0][0]
            + self.m[0][1] * self.m[0][1]
            + self.m[0][2] * self.m[0][2])
            .sqrt()
            .max(1e-8);
        let sy = (self.m[1][0] * self.m[1][0]
            + self.m[1][1] * self.m[1][1]
            + self.m[1][2] * self.m[1][2])
            .sqrt()
            .max(1e-8);
        let sz = (self.m[2][0] * self.m[2][0]
            + self.m[2][1] * self.m[2][1]
            + self.m[2][2] * self.m[2][2])
            .sqrt()
            .max(1e-8);
        let scale = [sx, sy, sz];
        let r00 = self.m[0][0] / sx;
        let r01 = self.m[0][1] / sx;
        let r02 = self.m[0][2] / sx;
        let r10 = self.m[1][0] / sy;
        let r11 = self.m[1][1] / sy;
        let r12 = self.m[1][2] / sy;
        let r20 = self.m[2][0] / sz;
        let r21 = self.m[2][1] / sz;
        let r22 = self.m[2][2] / sz;
        let trace = r00 + r11 + r22;
        let rot = if trace > 0.0 {
            let s = (trace + 1.0).sqrt() * 2.0;
            Quat {
                w: 0.25 * s,
                x: (r21 - r12) / s,
                y: (r02 - r20) / s,
                z: (r10 - r01) / s,
            }
        } else if r00 > r11 && r00 > r22 {
            let s = (1.0 + r00 - r11 - r22).sqrt() * 2.0;
            Quat {
                w: (r21 - r12) / s,
                x: 0.25 * s,
                y: (r01 + r10) / s,
                z: (r02 + r20) / s,
            }
        } else if r11 > r22 {
            let s = (1.0 + r11 - r00 - r22).sqrt() * 2.0;
            Quat {
                w: (r02 - r20) / s,
                x: (r01 + r10) / s,
                y: 0.25 * s,
                z: (r12 + r21) / s,
            }
        } else {
            let s = (1.0 + r22 - r00 - r11).sqrt() * 2.0;
            Quat {
                w: (r10 - r01) / s,
                x: (r02 + r20) / s,
                y: (r12 + r21) / s,
                z: 0.25 * s,
            }
        };
        (scale, rot.normalize(), pos)
    }
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len <= 1e-8 {
        [0.0, 0.0, 1.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    let k = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * k,
        a[1] + (b[1] - a[1]) * k,
        a[2] + (b[2] - a[2]) * k,
    ]
}

#[derive(Debug, Clone)]
enum Tok {
    Ident(String),
    Number(f32),
    Str(String),
    Sym(char),
}

struct Cursor {
    toks: Vec<Tok>,
    i: usize,
}

impl Cursor {
    fn new(toks: Vec<Tok>) -> Self {
        Self { toks, i: 0 }
    }

    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.i)
    }

    fn next(&mut self) -> Option<Tok> {
        let out = self.toks.get(self.i).cloned();
        if out.is_some() {
            self.i += 1;
        }
        out
    }

    fn consume_ident(&mut self, name: &str) -> bool {
        match self.peek() {
            Some(Tok::Ident(s)) if s == name => {
                self.i += 1;
                true
            }
            _ => false,
        }
    }

    fn consume_sym(&mut self, ch: char) -> bool {
        match self.peek() {
            Some(Tok::Sym(c)) if *c == ch => {
                self.i += 1;
                true
            }
            _ => false,
        }
    }

    fn next_number(&mut self) -> Result<f32> {
        match self.next() {
            Some(Tok::Number(v)) => Ok(v),
            other => bail!("expected number, got {:?}", other),
        }
    }

    fn next_i64(&mut self) -> Result<i64> {
        Ok(self.next_number()? as i64)
    }

    fn next_usize(&mut self) -> Result<usize> {
        Ok(self.next_number()? as usize)
    }

    fn next_string(&mut self) -> Result<String> {
        match self.next() {
            Some(Tok::Str(s)) => Ok(s),
            Some(Tok::Ident(s)) => Ok(s),
            other => bail!("expected string, got {:?}", other),
        }
    }

    fn optional_name_before_block(&mut self) -> Option<String> {
        match self.peek() {
            Some(Tok::Ident(s)) if matches!(self.toks.get(self.i + 1), Some(Tok::Sym('{'))) => {
                let out = s.clone();
                self.i += 1;
                Some(out)
            }
            Some(Tok::Str(s)) if matches!(self.toks.get(self.i + 1), Some(Tok::Sym('{'))) => {
                let out = s.clone();
                self.i += 1;
                Some(out)
            }
            _ => None,
        }
    }

    fn expect_block_start(&mut self) -> Result<()> {
        let _ = self.optional_name_before_block();
        if self.consume_sym('{') {
            Ok(())
        } else {
            bail!("expected '{{' at token {}", self.i)
        }
    }

    fn skip_block(&mut self) {
        let _ = self.optional_name_before_block();
        if !self.consume_sym('{') {
            let _ = self.next();
            return;
        }
        let mut depth = 1usize;
        while let Some(tok) = self.next() {
            match tok {
                Tok::Sym('{') => depth += 1,
                Tok::Sym('}') => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                _ => {}
            }
        }
    }
}

fn find_existing_casefold(path: &Path) -> Option<PathBuf> {
    if path.exists() {
        return Some(path.to_path_buf());
    }
    let parent = path.parent()?;
    let name = path.file_name()?.to_string_lossy().to_string();
    let norm = name.replace('\\', "/");
    let entries = fs::read_dir(parent).ok()?;
    for ent in entries.flatten() {
        let fname = ent.file_name().to_string_lossy().to_string();
        if fname.eq_ignore_ascii_case(&norm) {
            return Some(ent.path());
        }
    }
    None
}

fn resolve_relative_casefold(base: &Path, rel: &str) -> Option<PathBuf> {
    let mut cur = base.to_path_buf();
    for part in rel.replace('\\', "/").split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            cur = cur.parent()?.to_path_buf();
            continue;
        }
        let next = cur.join(part);
        cur = find_existing_casefold(&next).unwrap_or(next);
    }
    if cur.exists() {
        Some(cur)
    } else {
        find_existing_casefold(&cur)
    }
}

pub fn resolve_mesh_path(project_dir: &Path, append_dir: &str, file_name: &str) -> Result<PathBuf> {
    if file_name.trim().is_empty() {
        bail!("empty mesh file name")
    }
    let raw = Path::new(file_name);
    if raw.is_absolute() {
        let x_raw = if raw.extension().is_some() {
            raw.to_path_buf()
        } else {
            raw.with_extension("x")
        };
        if let Some(found) = find_existing_casefold(&x_raw) {
            return Ok(found);
        }
        let sgmesh_raw = raw.with_extension("sgmesh");
        if let Some(found) = find_existing_casefold(&sgmesh_raw) {
            return Ok(found);
        }
    }
    let norm = file_name.replace('\\', "/");
    let p = Path::new(&norm);
    let x_name = if p.extension().is_some() {
        norm.clone()
    } else {
        format!("{norm}.x")
    };
    let sgmesh_name = if p
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.eq_ignore_ascii_case("sgmesh"))
        .unwrap_or(false)
    {
        norm.clone()
    } else {
        p.with_extension("sgmesh")
            .to_string_lossy()
            .replace('\\', "/")
    };

    for append in crate::resource::ordered_append_dirs(project_dir, append_dir) {
        let base = if append.is_empty() {
            project_dir.join("x")
        } else {
            project_dir.join(&append).join("x")
        };
        if let Some(found) = resolve_relative_casefold(&base, &x_name) {
            return Ok(found);
        }
    }

    for append in crate::resource::ordered_append_dirs(project_dir, append_dir) {
        let base = if append.is_empty() {
            project_dir.join("x")
        } else {
            project_dir.join(&append).join("x")
        };
        if let Some(found) = resolve_relative_casefold(&base, &sgmesh_name) {
            return Ok(found);
        }
    }

    bail!("mesh asset not found through x resource path: {file_name}")
}

pub fn load_mesh_asset(project_dir: &Path, append_dir: &str, file_name: &str) -> Result<MeshAsset> {
    let path = resolve_mesh_path(project_dir, append_dir, file_name)?;
    load_mesh_asset_from_path(&path)
}

fn load_mesh_asset_from_path(path: &Path) -> Result<MeshAsset> {
    let bytes = fs::read(path).with_context(|| format!("read mesh {:?}", path))?;
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut asset = if ext == "sgmesh" {
        read_internal_mesh_asset(path)?
    } else if ext == "obj" {
        parse_obj(&decode_text(&bytes), path)?
    } else if ext == "x" {
        import_x_scene_bytes(&bytes, path)?.into_mesh_asset(path)?
    } else {
        let text = decode_text(&bytes);
        if text.contains("Mesh") && text.contains("xof") {
            import_x_scene_bytes(&bytes, path)?.into_mesh_asset(path)?
        } else {
            parse_obj(&text, path)?
        }
    };
    finalize_mesh_asset(&mut asset);
    Ok(asset)
}

fn finalize_mesh_asset(asset: &mut MeshAsset) {
    if asset.texture_path.is_none() {
        asset.texture_path = asset.primitives.iter().find_map(|p| p.texture_path.clone());
    }
    for prim in &mut asset.primitives {
        if prim.runtime_desc.effect_key.is_empty() || prim.runtime_desc.technique_name.is_empty() {
            prim.runtime_desc = build_mesh_primitive_runtime_desc(prim);
        } else {
            prim.runtime_desc.vertex_stride_bytes = std::mem::size_of::<MeshTriVertex>() as u32;
            prim.runtime_desc.vertex_count = prim.vertices.len() as u32;
            prim.runtime_desc.bone_palette_len = prim.bones.len() as u32;
        }
    }
    if asset.vertices.is_empty() {
        asset.vertices = asset.sample_vertices(0.0);
    }
    asset.bone_count = asset
        .primitives
        .iter()
        .map(|p| p.bones.len())
        .max()
        .unwrap_or(0);
    if asset.ticks_per_second <= 0.0 {
        asset.ticks_per_second = 1.0;
    }
}

pub fn internal_mesh_asset_path_for_source(path: &Path) -> PathBuf {
    path.with_extension("sgmesh")
}

pub fn compile_mesh_asset_file(input: &Path, output: &Path) -> Result<()> {
    let mut asset = load_mesh_asset_from_path(input)?;
    asset.source_path = input.to_path_buf();
    write_internal_mesh_asset(output, &asset)
}

fn decode_text(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[1] == 0 {
        let mut u16s = Vec::with_capacity(bytes.len() / 2);
        let mut i = 0usize;
        while i + 1 < bytes.len() {
            u16s.push(u16::from_le_bytes([bytes[i], bytes[i + 1]]));
            i += 2;
        }
        String::from_utf16_lossy(&u16s)
    } else {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn parse_obj(text: &str, path: &Path) -> Result<MeshAsset> {
    #[derive(Debug, Clone)]
    struct ObjGroup {
        material_name: Option<String>,
        vertices: Vec<MeshTriVertex>,
    }

    let mut positions = Vec::<[f32; 3]>::new();
    let mut texcoords = Vec::<[f32; 2]>::new();
    let mut normals = Vec::<[f32; 3]>::new();
    let mut mtllibs = Vec::<PathBuf>::new();
    let mut groups = vec![ObjGroup {
        material_name: None,
        vertices: Vec::new(),
    }];
    let mut current_group = 0usize;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(tag) = parts.next() else {
            continue;
        };
        match tag {
            "v" => {
                let vals: Vec<f32> = parts.filter_map(|s| s.parse::<f32>().ok()).collect();
                if vals.len() >= 3 {
                    positions.push([vals[0], vals[1], vals[2]]);
                }
            }
            "vt" => {
                let vals: Vec<f32> = parts.filter_map(|s| s.parse::<f32>().ok()).collect();
                if vals.len() >= 2 {
                    texcoords.push([vals[0], 1.0 - vals[1]]);
                }
            }
            "vn" => {
                let vals: Vec<f32> = parts.filter_map(|s| s.parse::<f32>().ok()).collect();
                if vals.len() >= 3 {
                    normals.push(normalize3([vals[0], vals[1], vals[2]]));
                }
            }
            "mtllib" => {
                for name in parts {
                    let cand = if let Some(parent) = path.parent() {
                        parent.join(name)
                    } else {
                        PathBuf::from(name)
                    };
                    mtllibs.push(cand);
                }
            }
            "usemtl" => {
                let mat = parts.next().map(|s| s.to_string());
                if let Some(idx) = groups.iter().position(|g| g.material_name == mat) {
                    current_group = idx;
                } else {
                    current_group = groups.len();
                    groups.push(ObjGroup {
                        material_name: mat,
                        vertices: Vec::new(),
                    });
                }
            }
            "f" => {
                let items: Vec<String> = parts.map(|s| s.to_string()).collect();
                if items.len() < 3 {
                    continue;
                }
                let mut face_verts = Vec::<MeshTriVertex>::new();
                for item in items {
                    let comps: Vec<&str> = item.split('/').collect();
                    let vi = comps
                        .get(0)
                        .and_then(|s| s.parse::<isize>().ok())
                        .unwrap_or(0);
                    let ti = comps
                        .get(1)
                        .and_then(|s| {
                            if s.is_empty() {
                                None
                            } else {
                                s.parse::<isize>().ok()
                            }
                        })
                        .unwrap_or(0);
                    let ni = comps
                        .get(2)
                        .and_then(|s| {
                            if s.is_empty() {
                                None
                            } else {
                                s.parse::<isize>().ok()
                            }
                        })
                        .unwrap_or(0);
                    let pos = obj_index(&positions, vi)
                        .copied()
                        .unwrap_or([0.0, 0.0, 0.0]);
                    let uv = obj_index(&texcoords, ti).copied().unwrap_or([0.0, 0.0]);
                    let normal = obj_index(&normals, ni).copied().unwrap_or([0.0, 0.0, 0.0]);
                    face_verts.push(MeshTriVertex {
                        pos,
                        uv,
                        normal,
                        tangent: [0.0, 0.0, 0.0],
                        binormal: [0.0, 0.0, 0.0],
                        color: [1.0, 1.0, 1.0, 1.0],
                        bone_indices: [0; 4],
                        bone_weights: [0.0; 4],
                    });
                }
                for i in 1..face_verts.len() - 1 {
                    let a = face_verts[0].clone();
                    let b = face_verts[i].clone();
                    let c = face_verts[i + 1].clone();
                    let n = triangle_normal(a.pos, b.pos, c.pos);
                    groups[current_group].vertices.push(MeshTriVertex {
                        normal: if has_normal(a.normal) { a.normal } else { n },
                        ..a
                    });
                    groups[current_group].vertices.push(MeshTriVertex {
                        normal: if has_normal(b.normal) { b.normal } else { n },
                        ..b
                    });
                    groups[current_group].vertices.push(MeshTriVertex {
                        normal: if has_normal(c.normal) { c.normal } else { n },
                        ..c
                    });
                }
            }
            _ => {}
        }
    }

    let material_map = parse_obj_material_libs(path, &mtllibs);
    let mut primitives = Vec::new();
    let mut combined = Vec::new();
    let mut fallback_tex = None;
    for group in groups {
        if group.vertices.is_empty() {
            continue;
        }
        let (material, texture_path) = if let Some(name) = group.material_name.as_deref() {
            material_map
                .get(name)
                .cloned()
                .unwrap_or((default_mesh_material(), None))
        } else {
            (default_mesh_material(), None)
        };
        if fallback_tex.is_none() {
            fallback_tex = texture_path.clone();
        }
        let mut vertices = group.vertices;
        apply_tangent_space(&mut vertices);
        combined.extend(vertices.iter().cloned());
        primitives.push(MeshPrimitive {
            frame_index: 0,
            texture_path,
            material,
            runtime_desc: MeshPrimitiveRuntimeDesc::default(),
            vertices,
            bones: Vec::new(),
        });
    }
    if primitives.is_empty() {
        bail!("no faces in {:?}", path);
    }
    Ok(MeshAsset {
        source_path: path.to_path_buf(),
        texture_path: fallback_tex,
        vertices: combined,
        bone_count: 0,
        ticks_per_second: 4800.0,
        primitives,
        frames: vec![FrameNode {
            name: "__root__".to_string(),
            base_local: Mat4::identity(),
            children: Vec::new(),
        }],
        animations: Vec::new(),
    })
}

fn parse_obj_material_libs(
    mesh_path: &Path,
    libs: &[PathBuf],
) -> HashMap<String, (MeshMaterial, Option<PathBuf>)> {
    let mut out = HashMap::new();
    for lib in libs {
        if let Ok(text) = fs::read_to_string(lib) {
            let base = lib
                .parent()
                .unwrap_or_else(|| mesh_path.parent().unwrap_or(Path::new(".")));
            let mut current_name: Option<String> = None;
            let mut current_material = default_mesh_material();
            let mut current_texture: Option<PathBuf> = None;
            for line in text.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut parts = line.split_whitespace();
                let Some(tag) = parts.next() else {
                    continue;
                };
                match tag {
                    "newmtl" => {
                        if let Some(name) = current_name.take() {
                            out.insert(name, (current_material.clone(), current_texture.clone()));
                        }
                        current_name = parts.next().map(|s| s.to_string());
                        current_material = default_mesh_material();
                        current_texture = None;
                    }
                    "Ka" => {
                        let vals: Vec<f32> = parts.filter_map(|s| s.parse::<f32>().ok()).collect();
                        if vals.len() >= 3 {
                            current_material.ambient = [vals[0], vals[1], vals[2], 1.0];
                        }
                    }
                    "Kd" => {
                        let vals: Vec<f32> = parts.filter_map(|s| s.parse::<f32>().ok()).collect();
                        if vals.len() >= 3 {
                            current_material.diffuse = [vals[0], vals[1], vals[2], 1.0];
                        }
                    }
                    "Ks" => {
                        let vals: Vec<f32> = parts.filter_map(|s| s.parse::<f32>().ok()).collect();
                        if vals.len() >= 3 {
                            current_material.specular = [vals[0], vals[1], vals[2], 1.0];
                        }
                    }
                    "Ke" => {
                        let vals: Vec<f32> = parts.filter_map(|s| s.parse::<f32>().ok()).collect();
                        if vals.len() >= 3 {
                            current_material.emissive = [vals[0], vals[1], vals[2], 1.0];
                        }
                    }
                    "Ns" => {
                        if let Some(v) = parts.next().and_then(|s| s.parse::<f32>().ok()) {
                            current_material.power = v.max(1.0);
                        }
                    }
                    "illum" => {
                        if let Some(v) = parts.next().and_then(|s| s.parse::<i32>().ok()) {
                            current_material.lighting_type = match v {
                                0 => MeshLightingType::None,
                                1 => MeshLightingType::Lambert,
                                2..=10 => MeshLightingType::BlinnPhong,
                                _ => current_material.lighting_type,
                            };
                        }
                    }
                    "d" | "Tr" => {
                        if let Some(v) = parts.next().and_then(|s| s.parse::<f32>().ok()) {
                            current_material.diffuse[3] = if tag == "Tr" { 1.0 - v } else { v };
                        }
                    }
                    "map_Kd" => {
                        if let Some(name) = parts.next() {
                            let tex = base.join(name);
                            current_texture = if tex.exists() {
                                Some(tex)
                            } else {
                                resolve_texture_path(mesh_path, name)
                            };
                        }
                    }
                    "map_Bump" | "bump" | "norm" => {
                        if let Some(name) = parts.next() {
                            let tex = base.join(name);
                            current_material.normal_texture_path = if tex.exists() {
                                Some(tex)
                            } else {
                                resolve_texture_path(mesh_path, name)
                            };
                            if current_material.normal_texture_path.is_some() {
                                current_material.lighting_type = MeshLightingType::Bump;
                            }
                        }
                    }
                    _ => {}
                }
            }
            if let Some(name) = current_name.take() {
                out.insert(name, (current_material, current_texture));
            }
        }
    }
    out
}

fn default_mesh_material() -> MeshMaterial {
    MeshMaterial {
        diffuse: [1.0, 1.0, 1.0, 1.0],
        ambient: [1.0, 1.0, 1.0, 1.0],
        specular: [0.0, 0.0, 0.0, 1.0],
        emissive: [0.0, 0.0, 0.0, 1.0],
        power: 16.0,
        lighting_type: MeshLightingType::None,
        shading_type: MeshShadingType::None,
        shader_option: MESH_SHADER_OPTION_NONE,
        rim_light_color: [1.0, 1.0, 1.0, 1.0],
        rim_light_power: 1.0,
        parallax_max_height: 0.016,
        alpha_test_enable: false,
        alpha_ref: 0.001,
        cull_disable: false,
        shadow_map_enable: true,
        use_mesh_tex: true,
        use_mrbd: false,
        mrbd: [0.0, 0.0, 0.0, 0.0],
        use_rgb: false,
        rgb_rate: [0.0, 0.0, 0.0, 0.0],
        add_rgb: [0.0, 0.0, 0.0, 0.0],
        use_mul_vertex_color: false,
        mul_vertex_color_rate: 1.0,
        depth_buffer_shadow_bias: 0.03,
        directional_light_ids: vec![10],
        point_light_ids: vec![20],
        spot_light_ids: vec![30],
        normal_texture_path: None,
        toon_texture_path: None,
        effect_filename: None,
    }
}

pub fn mesh_runtime_material_key(
    material: &MeshMaterial,
    has_texture: bool,
    skinned: bool,
) -> MeshRuntimeMaterialKey {
    let use_shadow_tex = material.shading_type == MeshShadingType::DepthBuffer;
    let use_toon_tex =
        material.lighting_type == MeshLightingType::Toon && material.toon_texture_path.is_some();
    let use_normal_tex = matches!(
        material.lighting_type,
        MeshLightingType::Bump | MeshLightingType::Parallax
    ) && material.normal_texture_path.is_some();
    MeshRuntimeMaterialKey {
        use_mesh_tex: has_texture && material.use_mesh_tex,
        use_shadow_tex,
        use_toon_tex,
        use_normal_tex,
        use_mul_vertex_color: material.use_mul_vertex_color,
        use_mrbd: material.use_mrbd,
        use_rgb: material.use_rgb,
        lighting_type: material.lighting_type,
        shading_type: material.shading_type,
        shader_option: material.shader_option,
        skinned,
        alpha_test_enable: material.alpha_test_enable,
        cull_disable: material.cull_disable,
        shadow_map_enable: material.shadow_map_enable,
    }
}

pub fn mesh_effect_variant_bits(material: &MeshMaterial, has_texture: bool, skinned: bool) -> u64 {
    mesh_effect_variant_bits_from_runtime_key(mesh_runtime_material_key(
        material,
        has_texture,
        skinned,
    ))
}

pub fn mesh_effect_variant_bits_from_runtime_desc(desc: &MeshPrimitiveRuntimeDesc) -> u64 {
    mesh_effect_variant_bits_from_runtime_key(desc.material_key)
}

pub fn mesh_effect_variant_bits_from_runtime_key(key: MeshRuntimeMaterialKey) -> u64 {
    let mut bits = 0u64;
    if key.use_mesh_tex {
        bits |= 1 << 0;
    }
    if key.use_shadow_tex {
        bits |= 1 << 1;
    }
    if key.use_toon_tex {
        bits |= 1 << 2;
    }
    if key.use_normal_tex {
        bits |= 1 << 3;
    }
    if key.use_mul_vertex_color {
        bits |= 1 << 4;
    }
    if key.use_mrbd {
        bits |= 1 << 5;
    }
    if key.use_rgb {
        bits |= 1 << 6;
    }
    if key.shader_option & MESH_SHADER_OPTION_RIM_LIGHT != 0 {
        bits |= 1 << 7;
    }
    bits |= ((key.lighting_type as u64) & 0xF) << 8;
    bits |= ((key.shading_type as u64) & 0xF) << 12;
    if key.skinned {
        bits |= 1 << 16;
    }
    if key.alpha_test_enable {
        bits |= 1 << 17;
    }
    if key.cull_disable {
        bits |= 1 << 18;
    }
    if key.shadow_map_enable {
        bits |= 1 << 19;
    }
    bits
}

pub fn mesh_effect_key_from_variant(bits: u64) -> String {
    let key = MeshRuntimeMaterialKey {
        use_mesh_tex: (bits & (1 << 0)) != 0,
        use_shadow_tex: (bits & (1 << 1)) != 0,
        use_toon_tex: (bits & (1 << 2)) != 0,
        use_normal_tex: (bits & (1 << 3)) != 0,
        use_mul_vertex_color: (bits & (1 << 4)) != 0,
        use_mrbd: (bits & (1 << 5)) != 0,
        use_rgb: (bits & (1 << 6)) != 0,
        lighting_type: match ((bits >> 8) & 0xF) as u32 {
            1 => MeshLightingType::Lambert,
            2 => MeshLightingType::BlinnPhong,
            3 => MeshLightingType::PerPixelBlinnPhong,
            4 => MeshLightingType::PerPixelHalfLambert,
            5 => MeshLightingType::Toon,
            6 => MeshLightingType::FixedFunction,
            7 => MeshLightingType::PerPixelFixedFunction,
            8 => MeshLightingType::Bump,
            9 => MeshLightingType::Parallax,
            _ => MeshLightingType::None,
        },
        shading_type: if ((bits >> 12) & 0xF) as u32 == 1 {
            MeshShadingType::DepthBuffer
        } else {
            MeshShadingType::None
        },
        shader_option: if (bits & (1 << 7)) != 0 {
            MESH_SHADER_OPTION_RIM_LIGHT
        } else {
            MESH_SHADER_OPTION_NONE
        },
        skinned: (bits & (1 << 16)) != 0,
        alpha_test_enable: (bits & (1 << 17)) != 0,
        cull_disable: (bits & (1 << 18)) != 0,
        shadow_map_enable: (bits & (1 << 19)) != 0,
    };
    mesh_effect_filename_from_runtime_key(
        if key.skinned {
            MeshEffectProfile::SkinnedMesh
        } else {
            MeshEffectProfile::Mesh
        },
        key,
    )
}

pub fn mesh_effect_filename_from_runtime_key(
    effect_profile: MeshEffectProfile,
    key: MeshRuntimeMaterialKey,
) -> String {
    match effect_profile {
        MeshEffectProfile::ShadowMap => {
            let mut name = String::from("shadow_map");
            if key.skinned {
                name.push_str("_skn_p_bw_bi_n.fx");
            } else {
                name.push_str("_frm_p_n.fx");
            }
            name
        }
        MeshEffectProfile::Mesh | MeshEffectProfile::SkinnedMesh | MeshEffectProfile::None => {
            let mut name =
                if key.skinned || matches!(effect_profile, MeshEffectProfile::SkinnedMesh) {
                    String::from("skinned")
                } else {
                    String::from("mesh")
                };
            if key.use_mesh_tex {
                name.push_str("_mt");
            }
            if key.use_shadow_tex {
                name.push_str("_st");
            }
            if key.use_toon_tex {
                name.push_str("_tt");
            }
            if key.use_normal_tex {
                name.push_str("_nt");
            }
            if key.use_mul_vertex_color {
                name.push_str("_vc");
            }
            if key.use_mrbd {
                name.push_str("_mrbd");
            }
            if key.use_rgb {
                name.push_str("_rgb");
            }
            name.push_str(match key.lighting_type {
                MeshLightingType::Lambert => "_lmbt",
                MeshLightingType::BlinnPhong => "_blph",
                MeshLightingType::PerPixelBlinnPhong => "_ppbp",
                MeshLightingType::PerPixelHalfLambert => "_pphl",
                MeshLightingType::Toon => "_toon",
                MeshLightingType::FixedFunction => "_ffp",
                MeshLightingType::PerPixelFixedFunction => "_ppfp",
                MeshLightingType::Bump => "_bump",
                MeshLightingType::Parallax => "_para",
                MeshLightingType::None => "_nolt",
            });
            name.push_str(match key.shading_type {
                MeshShadingType::DepthBuffer => "_dpbs",
                MeshShadingType::None => "_nost",
            });
            if key.shader_option & MESH_SHADER_OPTION_RIM_LIGHT != 0 {
                name.push_str("_rmlt");
            }
            name.push_str(".fx");
            name
        }
    }
}

fn build_mesh_primitive_runtime_desc_from_material(
    material: &MeshMaterial,
    has_texture: bool,
    skinned: bool,
    vertex_count: u32,
    bone_palette_len: u32,
) -> MeshPrimitiveRuntimeDesc {
    let material_key = mesh_runtime_material_key(material, has_texture, skinned);
    let effect_profile = if skinned {
        MeshEffectProfile::SkinnedMesh
    } else {
        MeshEffectProfile::Mesh
    };
    MeshPrimitiveRuntimeDesc {
        effect_profile,
        effect_key: mesh_effect_filename_from_runtime_key(effect_profile, material_key),
        technique_name: String::from("tech"),
        shadow_effect_key: mesh_effect_filename_from_runtime_key(
            MeshEffectProfile::ShadowMap,
            MeshRuntimeMaterialKey {
                skinned,
                shadow_map_enable: true,
                ..MeshRuntimeMaterialKey::default()
            },
        ),
        shadow_technique_name: String::from("tech"),
        use_mesh_texture_slot: material_key.use_mesh_tex,
        use_normal_texture_slot: material_key.use_normal_tex,
        use_toon_texture_slot: material_key.use_toon_tex,
        use_shadow_texture_slot: material_key.use_shadow_tex,
        material_key,
        vertex_stride_bytes: std::mem::size_of::<MeshTriVertex>() as u32,
        vertex_count,
        bone_palette_len,
    }
}

fn build_mesh_primitive_runtime_desc(prim: &MeshPrimitive) -> MeshPrimitiveRuntimeDesc {
    build_mesh_primitive_runtime_desc_from_material(
        &prim.material,
        prim.texture_path.is_some(),
        !prim.bones.is_empty(),
        prim.vertices.len() as u32,
        prim.bones.len() as u32,
    )
}

fn apply_effect_filename(material: &mut MeshMaterial, effect_name: &str) {
    let lower = effect_name.to_ascii_lowercase();
    material.effect_filename = Some(effect_name.to_string());
    if lower.contains("_lmbt") {
        material.lighting_type = MeshLightingType::Lambert;
    } else if lower.contains("_blph") {
        material.lighting_type = MeshLightingType::BlinnPhong;
    } else if lower.contains("_ppbp") {
        material.lighting_type = MeshLightingType::PerPixelBlinnPhong;
    } else if lower.contains("_pphl") {
        material.lighting_type = MeshLightingType::PerPixelHalfLambert;
    } else if lower.contains("_toon") {
        material.lighting_type = MeshLightingType::Toon;
    } else if lower.contains("_ffp") {
        material.lighting_type = MeshLightingType::FixedFunction;
    } else if lower.contains("_ppfp") {
        material.lighting_type = MeshLightingType::PerPixelFixedFunction;
    } else if lower.contains("_bump") {
        material.lighting_type = MeshLightingType::Bump;
    } else if lower.contains("_para") {
        material.lighting_type = MeshLightingType::Parallax;
    } else if lower.contains("_nolt") {
        material.lighting_type = MeshLightingType::None;
    }
    if lower.contains("_dpbs") {
        material.shading_type = MeshShadingType::DepthBuffer;
    } else if lower.contains("_nost") {
        material.shading_type = MeshShadingType::None;
    }
    if lower.contains("_rmlt") {
        material.shader_option |= MESH_SHADER_OPTION_RIM_LIGHT;
    }
}

fn parse_effect_bool(value_str: Option<&str>, values: &[f32]) -> Option<bool> {
    if let Some(v) = values.first() {
        return Some(*v != 0.0);
    }
    value_str.map(|s| {
        !matches!(
            s.to_ascii_lowercase().as_str(),
            "0" | "false" | "off" | "none"
        )
    })
}

fn parse_effect_int_list(values: &[f32]) -> Vec<i32> {
    values.iter().map(|v| *v as i32).collect()
}

fn assign_effect_param(
    material: &mut MeshMaterial,
    mesh_path: &Path,
    key: &str,
    value_str: Option<&str>,
    values: &[f32],
) {
    let key = key.to_ascii_lowercase();
    match key.as_str() {
        "lighting_type" | "mp__lighting_type" => {
            if let Some(v) = values.first() {
                material.lighting_type = match *v as i32 {
                    1 => MeshLightingType::Lambert,
                    2 => MeshLightingType::BlinnPhong,
                    3 => MeshLightingType::PerPixelBlinnPhong,
                    4 => MeshLightingType::PerPixelHalfLambert,
                    5 => MeshLightingType::Toon,
                    6 => MeshLightingType::FixedFunction,
                    7 => MeshLightingType::PerPixelFixedFunction,
                    8 => MeshLightingType::Bump,
                    9 => MeshLightingType::Parallax,
                    _ => MeshLightingType::None,
                };
            }
        }
        "shading_type" | "mp__shading_type" => {
            if let Some(v) = values.first() {
                material.shading_type = if (*v as i32) == 1 {
                    MeshShadingType::DepthBuffer
                } else {
                    MeshShadingType::None
                };
            }
        }
        "shader_option" => {
            if let Some(v) = values.first() {
                material.shader_option = *v as u32;
            }
        }
        "use_mesh_tex" | "g_usemeshtex" => {
            if let Some(v) = parse_effect_bool(value_str, values) {
                material.use_mesh_tex = v;
            }
        }
        "use_mrbd" | "g_usemrbd" => {
            if let Some(v) = parse_effect_bool(value_str, values) {
                material.use_mrbd = v;
            }
        }
        "mrbd" | "g_mrbd" => {
            if !values.is_empty() {
                material.mrbd = [
                    values.first().copied().unwrap_or(0.0),
                    values.get(1).copied().unwrap_or(0.0),
                    values.get(2).copied().unwrap_or(0.0),
                    values.get(3).copied().unwrap_or(0.0),
                ];
                material.use_mrbd = values.iter().any(|v| v.abs() > 1e-6);
            }
        }
        "use_rgb" | "g_usergb" => {
            if let Some(v) = parse_effect_bool(value_str, values) {
                material.use_rgb = v;
            }
        }
        "rgb_rate" | "g_rgbrate" => {
            if !values.is_empty() {
                material.rgb_rate = [
                    values.first().copied().unwrap_or(0.0),
                    values.get(1).copied().unwrap_or(0.0),
                    values.get(2).copied().unwrap_or(0.0),
                    values.get(3).copied().unwrap_or(0.0),
                ];
                material.use_rgb = true;
            }
        }
        "add_rgb" | "g_addrgb" => {
            if !values.is_empty() {
                material.add_rgb = [
                    values.first().copied().unwrap_or(0.0),
                    values.get(1).copied().unwrap_or(0.0),
                    values.get(2).copied().unwrap_or(0.0),
                    values.get(3).copied().unwrap_or(0.0),
                ];
                material.use_rgb = true;
            }
        }
        "use_mul_vertex_color" | "g_usemulvertexcolor" => {
            if let Some(v) = parse_effect_bool(value_str, values) {
                material.use_mul_vertex_color = v;
            }
        }
        "mul_vertex_color_rate" | "g_mulvertexcolorrate" | "g_vertexcolorrate" => {
            if let Some(v) = values.first() {
                material.mul_vertex_color_rate = (*v).clamp(0.0, 8.0);
                material.use_mul_vertex_color =
                    material.use_mul_vertex_color || (*v - 1.0).abs() > 1e-6;
            }
        }
        "depth_buffer_shadow_bias" | "g_depthbuffershadowbias" | "g_dbsbias" => {
            if let Some(v) = values.first() {
                material.depth_buffer_shadow_bias = *v;
            }
        }
        "directional_light_id_list" | "directionallightidlist" | "g_directionallightidlist" => {
            let ids = parse_effect_int_list(values);
            if !ids.is_empty() {
                material.directional_light_ids = ids;
            }
        }
        "point_light_id_list" | "pointlightidlist" | "g_pointlightidlist" => {
            let ids = parse_effect_int_list(values);
            if !ids.is_empty() {
                material.point_light_ids = ids;
            }
        }
        "spot_light_id_list" | "spotlightidlist" | "g_spotlightidlist" => {
            let ids = parse_effect_int_list(values);
            if !ids.is_empty() {
                material.spot_light_ids = ids;
            }
        }
        "g_rimlightcolor" | "rim_light_color" => {
            if values.len() >= 3 {
                material.rim_light_color = [
                    values[0],
                    values[1],
                    values[2],
                    values.get(3).copied().unwrap_or(1.0),
                ];
            }
        }
        "g_rimlightpower" | "rim_light_power" => {
            if let Some(v) = values.first() {
                material.rim_light_power = (*v).max(0.0);
                if material.rim_light_power > 0.0 {
                    material.shader_option |= MESH_SHADER_OPTION_RIM_LIGHT;
                }
            }
        }
        "g_parallaxmaxheight" | "parallax_max_height" => {
            if let Some(v) = values.first() {
                material.parallax_max_height = (*v).max(0.0);
                if material.parallax_max_height > 0.0
                    && material.lighting_type == MeshLightingType::None
                {
                    material.lighting_type = MeshLightingType::Parallax;
                }
            }
        }
        "g_materialdiffuse" | "material_diffuse" => {
            if values.len() >= 3 {
                material.diffuse = [
                    values[0],
                    values[1],
                    values[2],
                    values.get(3).copied().unwrap_or(material.diffuse[3]),
                ];
            }
        }
        "g_materialambient" | "material_ambient" => {
            if values.len() >= 3 {
                material.ambient = [
                    values[0],
                    values[1],
                    values[2],
                    values.get(3).copied().unwrap_or(material.ambient[3]),
                ];
            }
        }
        "g_materialspecular" | "material_specular" => {
            if values.len() >= 3 {
                material.specular = [
                    values[0],
                    values[1],
                    values[2],
                    values.get(3).copied().unwrap_or(material.specular[3]),
                ];
            }
        }
        "g_materialemissive" | "material_emissive" => {
            if values.len() >= 3 {
                material.emissive = [
                    values[0],
                    values[1],
                    values[2],
                    values.get(3).copied().unwrap_or(material.emissive[3]),
                ];
            }
        }
        "g_materialspecularpower" | "material_specular_power" | "material_power" => {
            if let Some(v) = values.first() {
                material.power = (*v).max(1.0);
            }
        }
        "prerendertype" | "pre_render_type" | "g_prerendertype" | "shadow_map_enable"
        | "shadowmapenable" => {
            if let Some(v) = values.first() {
                material.shadow_map_enable = ((*v as i32) & 1) != 0;
            }
        }
        "alphatestenable" | "alpha_test_enable" | "g_alphatestenable" => {
            if let Some(v) = parse_effect_bool(value_str, values) {
                material.alpha_test_enable = v;
            }
        }
        "alpharef" | "alpha_ref" | "g_alpharef" => {
            if let Some(v) = values.first() {
                material.alpha_ref = (*v).clamp(0.0, 1.0);
                if material.alpha_ref > 0.0 {
                    material.alpha_test_enable = true;
                }
            }
        }
        "cullmode" | "cull_mode" | "twosided" | "double_sided" => {
            if let Some(v) = values.first() {
                let vi = *v as i32;
                material.cull_disable = vi == 0 || vi == 1;
            } else if let Some(s) = value_str {
                let s = s.to_ascii_lowercase();
                material.cull_disable = matches!(
                    s.as_str(),
                    "none" | "off" | "0" | "1" | "two" | "twosided" | "double" | "double_sided"
                );
            }
        }
        "normaltexture" | "normal_tex" | "g_normaltexture" | "g_normaltex" | "normalmap"
        | "normal_map" => {
            if let Some(s) = value_str {
                material.normal_texture_path = resolve_texture_path(mesh_path, s);
                if material.normal_texture_path.is_some()
                    && material.lighting_type == MeshLightingType::None
                {
                    material.lighting_type = MeshLightingType::Bump;
                }
            }
        }
        "toontexture" | "toon_tex" | "g_toontexture" | "g_toontex" | "toonmap" | "toon_map" => {
            if let Some(s) = value_str {
                material.toon_texture_path = resolve_texture_path(mesh_path, s);
                if material.toon_texture_path.is_some()
                    && material.lighting_type == MeshLightingType::None
                {
                    material.lighting_type = MeshLightingType::Toon;
                }
            }
        }
        _ => {}
    }
}

fn parse_x_effect_instance(
    cur: &mut Cursor,
    mesh_path: &Path,
    material: &mut MeshMaterial,
) -> Result<()> {
    cur.expect_block_start()?;
    if let Ok(effect_name) = cur.next_string() {
        apply_effect_filename(material, &effect_name);
    }
    while let Some(tok) = cur.peek().cloned() {
        match tok {
            Tok::Sym('}') => {
                cur.next();
                break;
            }
            Tok::Ident(_) => {
                let block_name = cur.next_string()?;
                if !cur.consume_sym('{') {
                    continue;
                }
                let mut strings = Vec::<String>::new();
                let mut values = Vec::<f32>::new();
                let mut depth = 1usize;
                while let Some(tok2) = cur.next() {
                    match tok2 {
                        Tok::Sym('{') => depth += 1,
                        Tok::Sym('}') => {
                            depth -= 1;
                            if depth == 0 {
                                break;
                            }
                        }
                        Tok::Str(s) | Tok::Ident(s) => strings.push(s),
                        Tok::Number(v) => values.push(v),
                        Tok::Sym(_) => {}
                    }
                }
                let value_str = strings
                    .iter()
                    .find(|s| s.contains('.') || s.contains('/') || s.contains('\\'))
                    .map(|s| s.as_str())
                    .or_else(|| strings.get(1).map(|s| s.as_str()));
                assign_effect_param(material, mesh_path, &block_name, value_str, &values);
                if let Some(first) = strings.first() {
                    assign_effect_param(
                        material,
                        mesh_path,
                        first,
                        strings.get(1).map(|s| s.as_str()),
                        &values,
                    );
                }
            }
            _ => {
                cur.next();
            }
        }
    }
    Ok(())
}

fn triangle_tangent_binormal(
    a: &MeshTriVertex,
    b: &MeshTriVertex,
    c: &MeshTriVertex,
) -> ([f32; 3], [f32; 3]) {
    let e1 = [
        b.pos[0] - a.pos[0],
        b.pos[1] - a.pos[1],
        b.pos[2] - a.pos[2],
    ];
    let e2 = [
        c.pos[0] - a.pos[0],
        c.pos[1] - a.pos[1],
        c.pos[2] - a.pos[2],
    ];
    let duv1 = [b.uv[0] - a.uv[0], b.uv[1] - a.uv[1]];
    let duv2 = [c.uv[0] - a.uv[0], c.uv[1] - a.uv[1]];
    let denom = duv1[0] * duv2[1] - duv1[1] * duv2[0];
    if denom.abs() <= 1e-8 {
        let n = if has_normal(a.normal) {
            a.normal
        } else {
            triangle_normal(a.pos, b.pos, c.pos)
        };
        let up = if n[1].abs() < 0.999 {
            [0.0, 1.0, 0.0]
        } else {
            [1.0, 0.0, 0.0]
        };
        let t = normalize3(cross3(up, n));
        let b = normalize3(cross3(n, t));
        return (t, b);
    }
    let inv = 1.0 / denom;
    let tangent = normalize3([
        inv * (duv2[1] * e1[0] - duv1[1] * e2[0]),
        inv * (duv2[1] * e1[1] - duv1[1] * e2[1]),
        inv * (duv2[1] * e1[2] - duv1[1] * e2[2]),
    ]);
    let binormal = normalize3([
        inv * (-duv2[0] * e1[0] + duv1[0] * e2[0]),
        inv * (-duv2[0] * e1[1] + duv1[0] * e2[1]),
        inv * (-duv2[0] * e1[2] + duv1[0] * e2[2]),
    ]);
    (tangent, binormal)
}

fn apply_tangent_space(vertices: &mut [MeshTriVertex]) {
    for tri in vertices.chunks_mut(3) {
        if tri.len() != 3 {
            continue;
        }
        let (t, b) = triangle_tangent_binormal(&tri[0], &tri[1], &tri[2]);
        for v in tri {
            v.tangent = t;
            v.binormal = b;
        }
    }
}

fn has_normal(n: [f32; 3]) -> bool {
    n[0].abs() > 1e-6 || n[1].abs() > 1e-6 || n[2].abs() > 1e-6
}

fn triangle_normal(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> [f32; 3] {
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    normalize3([
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ])
}

fn obj_index<T>(items: &[T], raw: isize) -> Option<&T> {
    if raw == 0 {
        return None;
    }
    if raw > 0 {
        items.get((raw - 1) as usize)
    } else {
        let idx = items.len() as isize + raw;
        if idx < 0 {
            None
        } else {
            items.get(idx as usize)
        }
    }
}

#[derive(Debug, Clone)]
struct ParsedXMaterial {
    name: Option<String>,
    material: MeshMaterial,
    texture_path: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct XFace {
    indices: Vec<usize>,
    material_index: usize,
}

#[derive(Debug, Clone)]
struct ImportedXBoneBinding {
    frame_name: String,
    offset_matrix: Mat4,
}

#[derive(Debug, Clone)]
struct ImportedXPrimitive {
    frame_node: usize,
    texture_path: Option<PathBuf>,
    material: MeshMaterial,
    vertices: Vec<MeshTriVertex>,
    bones: Vec<ImportedXBoneBinding>,
}

#[derive(Debug, Clone)]
struct ImportedXFrame {
    name: String,
    parent: Option<usize>,
    base_local: Mat4,
}

#[derive(Debug, Clone, Default)]
struct ImportedXScene {
    frames: Vec<ImportedXFrame>,
    primitives: Vec<ImportedXPrimitive>,
    animations: Vec<AnimationClip>,
    ticks_per_second: f32,
}


fn import_x_scene_bytes_shion(bytes: &[u8], path: &Path) -> Result<ImportedXScene> {
    let file = shion_xfile::parse_x(bytes)
        .with_context(|| format!("parse DirectX .x file with Shion parser: {:?}", path))?;
    let scene = shion_xfile::Scene::from_xfile(&file)
        .with_context(|| format!("lift DirectX .x semantic scene: {:?}", path))?;
    import_x_scene_from_shion_scene(&scene, path)
}

fn import_x_scene_from_shion_scene(
    scene: &shion_xfile::Scene,
    path: &Path,
) -> Result<ImportedXScene> {
    let loose_materials = shion_loose_material_lookup(scene);
    let mut out = ImportedXScene {
        frames: vec![ImportedXFrame {
            name: "__root__".to_string(),
            parent: None,
            base_local: Mat4::identity(),
        }],
        primitives: Vec::new(),
        animations: shion_animation_clips(scene),
        ticks_per_second: scene.anim_ticks_per_second.unwrap_or(4800).max(1) as f32,
    };

    for frame in &scene.frames {
        import_shion_frame(frame, path, &loose_materials, &mut out, 0)?;
    }
    for mesh in &scene.loose_meshes {
        out.primitives.extend(shion_mesh_primitives(
            mesh,
            path,
            0,
            &loose_materials,
        )?);
    }
    Ok(out)
}

fn shion_loose_material_lookup(
    scene: &shion_xfile::Scene,
) -> HashMap<String, shion_xfile::Material> {
    let mut out = HashMap::new();
    for material in &scene.loose_materials {
        if let Some(name) = &material.name {
            out.insert(name.clone(), material.clone());
        }
    }
    out
}

fn import_shion_frame(
    frame: &shion_xfile::Frame,
    path: &Path,
    loose_materials: &HashMap<String, shion_xfile::Material>,
    scene: &mut ImportedXScene,
    parent_frame: usize,
) -> Result<usize> {
    let frame_idx = scene.frames.len();
    scene.frames.push(ImportedXFrame {
        name: frame
            .name
            .clone()
            .unwrap_or_else(|| format!("frame_{}", frame_idx)),
        parent: Some(parent_frame),
        base_local: frame
            .transform
            .map(Mat4::from_x_values)
            .unwrap_or_else(Mat4::identity),
    });

    for mesh in &frame.meshes {
        scene.primitives.extend(shion_mesh_primitives(
            mesh,
            path,
            frame_idx,
            loose_materials,
        )?);
    }
    for child in &frame.child_frames {
        import_shion_frame(child, path, loose_materials, scene, frame_idx)?;
    }
    Ok(frame_idx)
}

fn shion_mesh_primitives(
    mesh: &shion_xfile::Mesh,
    path: &Path,
    frame_index: usize,
    loose_materials: &HashMap<String, shion_xfile::Material>,
) -> Result<Vec<ImportedXPrimitive>> {
    let material_slots = shion_material_slots(mesh, path, loose_materials);
    let face_material_indices = mesh
        .material_list
        .as_ref()
        .map(|list| list.face_indexes.as_slice())
        .unwrap_or(&[]);
    let texcoords = mesh.texcoords.as_deref().unwrap_or(&[]);
    let vertex_colors = shion_vertex_color_lookup(mesh);
    let (influences, bones) = shion_skin_bindings(mesh);
    let mut groups: HashMap<usize, Vec<MeshTriVertex>> = HashMap::new();

    for (face_idx, face) in mesh.faces.iter().enumerate() {
        if face.len() < 3 {
            continue;
        }
        let material_index = face_material_indices
            .get(face_idx)
            .copied()
            .unwrap_or(0) as usize;
        for tri in 1..face.len() - 1 {
            let face_corners = [0usize, tri, tri + 1];
            let vertex_indices = [
                face[face_corners[0]] as usize,
                face[face_corners[1]] as usize,
                face[face_corners[2]] as usize,
            ];
            let tri_n = triangle_normal(
                mesh.vertices
                    .get(vertex_indices[0])
                    .copied()
                    .unwrap_or([0.0, 0.0, 0.0]),
                mesh.vertices
                    .get(vertex_indices[1])
                    .copied()
                    .unwrap_or([0.0, 0.0, 0.0]),
                mesh.vertices
                    .get(vertex_indices[2])
                    .copied()
                    .unwrap_or([0.0, 0.0, 0.0]),
            );
            let group = groups.entry(material_index).or_default();
            for (&vertex_index, &corner_index) in vertex_indices.iter().zip(face_corners.iter()) {
                let pos = mesh
                    .vertices
                    .get(vertex_index)
                    .copied()
                    .unwrap_or([0.0, 0.0, 0.0]);
                let uv = texcoords.get(vertex_index).copied().unwrap_or([0.0, 0.0]);
                let normal = shion_face_corner_normal(mesh, face_idx, corner_index)
                    .or_else(|| shion_vertex_normal(mesh, vertex_index))
                    .unwrap_or(tri_n);
                let color = vertex_colors
                    .get(&vertex_index)
                    .copied()
                    .unwrap_or([1.0, 1.0, 1.0, 1.0]);
                let (bone_indices, bone_weights) = packed_influences(influences.get(&vertex_index));
                group.push(MeshTriVertex {
                    pos,
                    uv,
                    normal,
                    tangent: [0.0, 0.0, 0.0],
                    binormal: [0.0, 0.0, 0.0],
                    color,
                    bone_indices,
                    bone_weights,
                });
            }
        }
    }

    let mut out = Vec::new();
    for (material_index, vertices) in groups {
        if vertices.is_empty() {
            continue;
        }
        let slot = material_slots
            .get(material_index)
            .cloned()
            .unwrap_or_else(|| ParsedXMaterial {
                name: None,
                material: default_mesh_material(),
                texture_path: None,
            });
        let mut vertices = vertices;
        apply_tangent_space(&mut vertices);
        out.push(ImportedXPrimitive {
            frame_node: frame_index,
            texture_path: slot.texture_path.clone(),
            material: slot.material,
            vertices,
            bones: bones.clone(),
        });
    }
    Ok(out)
}

fn shion_material_slots(
    mesh: &shion_xfile::Mesh,
    path: &Path,
    loose_materials: &HashMap<String, shion_xfile::Material>,
) -> Vec<ParsedXMaterial> {
    let mut slots = Vec::new();
    if let Some(list) = &mesh.material_list {
        for material in &list.materials {
            slots.push(shion_material_to_parsed(material, path));
        }
        for reference in &list.material_references {
            if let Some(name) = &reference.name {
                if let Some(material) = loose_materials.get(name) {
                    slots.push(shion_material_to_parsed(material, path));
                    continue;
                }
            }
            slots.push(ParsedXMaterial {
                name: reference.name.clone(),
                material: default_mesh_material(),
                texture_path: None,
            });
        }
        while slots.len() < list.material_count as usize {
            slots.push(ParsedXMaterial {
                name: None,
                material: default_mesh_material(),
                texture_path: None,
            });
        }
    }
    if slots.is_empty() {
        slots.push(ParsedXMaterial {
            name: None,
            material: default_mesh_material(),
            texture_path: None,
        });
    }

    for effect in &mesh.effect_instances {
        for slot in &mut slots {
            shion_apply_effect_instance(&mut slot.material, path, effect);
        }
    }
    slots
}

fn shion_material_to_parsed(
    material: &shion_xfile::Material,
    path: &Path,
) -> ParsedXMaterial {
    let mut out = default_mesh_material();
    out.diffuse = material.face_color;
    out.ambient = material.face_color;
    out.specular = [
        material.specular_color[0],
        material.specular_color[1],
        material.specular_color[2],
        1.0,
    ];
    out.emissive = [
        material.emissive_color[0],
        material.emissive_color[1],
        material.emissive_color[2],
        1.0,
    ];
    out.power = material.power.max(1.0);

    let mut texture_path = material
        .texture_filenames
        .iter()
        .find_map(|tex| resolve_texture_path(path, tex));
    for effect in &material.effect_instances {
        shion_apply_effect_instance(&mut out, path, effect);
    }
    if texture_path.is_none() {
        texture_path = material.effect_instances.iter().find_map(|effect| {
            effect
                .strings
                .iter()
                .find_map(|param| resolve_texture_path(path, &param.value))
                .or_else(|| {
                    effect
                        .legacy_strings
                        .iter()
                        .find_map(|param| resolve_texture_path(path, &param.value))
                })
        });
    }

    ParsedXMaterial {
        name: material.name.clone(),
        material: out,
        texture_path,
    }
}

fn shion_apply_effect_instance(
    material: &mut MeshMaterial,
    path: &Path,
    effect: &shion_xfile::EffectInstance,
) {
    if !effect.effect_filename.trim().is_empty() {
        apply_effect_filename(material, &effect.effect_filename);
    }
    for param in &effect.strings {
        assign_effect_param(material, path, &param.param_name, Some(&param.value), &[]);
    }
    for param in &effect.dwords {
        assign_effect_param(
            material,
            path,
            &param.param_name,
            None,
            &[param.value as f32],
        );
    }
    for param in &effect.floats {
        assign_effect_param(material, path, &param.param_name, None, &param.values);
    }
    for param in &effect.legacy_strings {
        assign_effect_param(material, path, "effect_string", Some(&param.value), &[]);
    }
    for param in &effect.legacy_dwords {
        assign_effect_param(material, path, "effect_dword", None, &[param.value as f32]);
    }
    for param in &effect.legacy_floats {
        assign_effect_param(material, path, "effect_floats", None, &param.values);
    }
}

fn shion_vertex_color_lookup(mesh: &shion_xfile::Mesh) -> HashMap<usize, [f32; 4]> {
    let mut out = HashMap::new();
    if let Some(colors) = &mesh.vertex_colors {
        for color in colors {
            out.insert(color.index as usize, color.rgba);
        }
    }
    out
}

fn shion_face_corner_normal(
    mesh: &shion_xfile::Mesh,
    face_index: usize,
    corner_index: usize,
) -> Option<[f32; 3]> {
    let normals = mesh.normals.as_ref()?;
    let normal_index = (*normals.face_normals.get(face_index)?.get(corner_index)?) as usize;
    normals.normals.get(normal_index).copied()
}

fn shion_vertex_normal(mesh: &shion_xfile::Mesh, vertex_index: usize) -> Option<[f32; 3]> {
    let normals = mesh.normals.as_ref()?;
    normals.normals.get(vertex_index).copied()
}

fn shion_skin_bindings(
    mesh: &shion_xfile::Mesh,
) -> (
    HashMap<usize, Vec<(usize, f32)>>,
    Vec<ImportedXBoneBinding>,
) {
    let mut influences: HashMap<usize, Vec<(usize, f32)>> = HashMap::new();
    let mut bones = Vec::new();
    for skin in &mesh.skin_weights {
        let bone_index = bones.len();
        bones.push(ImportedXBoneBinding {
            frame_name: skin.transform_node_name.clone(),
            offset_matrix: Mat4::from_x_values(skin.matrix_offset),
        });
        for (&vertex_index, &weight) in skin.vertex_indices.iter().zip(skin.weights.iter()) {
            influences
                .entry(vertex_index as usize)
                .or_default()
                .push((bone_index, weight));
        }
    }
    (influences, bones)
}

fn shion_animation_clips(scene: &shion_xfile::Scene) -> Vec<AnimationClip> {
    let mut clips = Vec::new();
    for (set_index, set) in scene.animation_sets.iter().enumerate() {
        let mut clip = AnimationClip {
            name: set
                .name
                .clone()
                .unwrap_or_else(|| format!("anim_{}", set_index)),
            open_closed: true,
            ..Default::default()
        };
        for animation in &set.animations {
            if let Some(options) = &animation.options {
                clip.open_closed = options.open_closed != 0;
            }
            let Some(target_name) = animation.target_name.as_deref() else {
                continue;
            };
            let track = clip.tracks.entry(target_name.to_string()).or_default();
            for block in &animation.keys {
                for key in &block.keys {
                    let time = key.time as i64;
                    clip.max_time = clip.max_time.max(time);
                    match block.key_type {
                        0 => {
                            if key.values.len() >= 4 {
                                track.rotation_keys.push(QuatKey {
                                    time,
                                    value: Quat {
                                        x: key.values[1],
                                        y: key.values[2],
                                        z: key.values[3],
                                        w: key.values[0],
                                    }
                                    .normalize(),
                                });
                            }
                        }
                        1 => {
                            track.scale_keys.push(Vec3Key {
                                time,
                                value: [
                                    key.values.get(0).copied().unwrap_or(1.0),
                                    key.values.get(1).copied().unwrap_or(1.0),
                                    key.values.get(2).copied().unwrap_or(1.0),
                                ],
                            });
                        }
                        2 => {
                            track.position_keys.push(Vec3Key {
                                time,
                                value: [
                                    key.values.get(0).copied().unwrap_or(0.0),
                                    key.values.get(1).copied().unwrap_or(0.0),
                                    key.values.get(2).copied().unwrap_or(0.0),
                                ],
                            });
                        }
                        4 => {
                            let mut m = [0.0f32; 16];
                            for (dst, src) in m.iter_mut().zip(key.values.iter().copied()) {
                                *dst = src;
                            }
                            track.matrix_keys.push(MatKey {
                                time,
                                value: Mat4::from_x_values(m),
                            });
                        }
                        _ => {}
                    }
                }
            }
        }
        if !clip.tracks.is_empty() {
            clips.push(clip);
        }
    }
    clips
}

impl ImportedXScene {
    fn into_mesh_asset(self, path: &Path) -> Result<MeshAsset> {
        let mut children_map: Vec<Vec<usize>> = vec![Vec::new(); self.frames.len().max(1)];
        for (idx, frame) in self.frames.iter().enumerate() {
            if let Some(parent) = frame.parent {
                if parent < children_map.len() {
                    children_map[parent].push(idx);
                }
            }
        }
        let frames = self
            .frames
            .iter()
            .enumerate()
            .map(|(idx, frame)| FrameNode {
                name: frame.name.clone(),
                base_local: frame.base_local,
                children: children_map.get(idx).cloned().unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        let frame_lookup: HashMap<String, usize> = frames
            .iter()
            .enumerate()
            .map(|(i, f)| (f.name.clone(), i))
            .collect();
        let primitives = self
            .primitives
            .into_iter()
            .map(|prim| MeshPrimitive {
                frame_index: prim.frame_node,
                texture_path: prim.texture_path,
                material: prim.material,
                runtime_desc: MeshPrimitiveRuntimeDesc::default(),
                vertices: prim.vertices,
                bones: prim
                    .bones
                    .into_iter()
                    .map(|bone| BoneBinding {
                        frame_index: frame_lookup.get(&bone.frame_name).copied().unwrap_or(0),
                        frame_name: bone.frame_name,
                        offset_matrix: bone.offset_matrix,
                    })
                    .collect(),
            })
            .collect::<Vec<_>>();
        let texture_path = primitives.iter().find_map(|p| p.texture_path.clone());
        let mut asset = MeshAsset {
            source_path: path.to_path_buf(),
            texture_path,
            vertices: Vec::new(),
            bone_count: primitives.iter().map(|p| p.bones.len()).max().unwrap_or(0),
            ticks_per_second: self.ticks_per_second.max(1.0),
            primitives,
            frames,
            animations: self.animations,
        };
        asset.vertices = asset.sample_vertices(0.0);
        if asset.vertices.is_empty() {
            bail!("no usable Mesh block in {:?}", path)
        }
        Ok(asset)
    }
}

fn import_x_scene_tokens(toks: Vec<Tok>, path: &Path) -> Result<ImportedXScene> {
    let mut cur = Cursor::new(toks);
    let mut scene = ImportedXScene {
        frames: vec![ImportedXFrame {
            name: "__root__".to_string(),
            parent: None,
            base_local: Mat4::identity(),
        }],
        primitives: Vec::new(),
        animations: Vec::new(),
        ticks_per_second: 4800.0,
    };
    while let Some(tok) = cur.peek().cloned() {
        match tok {
            Tok::Ident(name) if name == "Frame" => {
                cur.next();
                import_x_frame_block(&mut cur, path, &mut scene, 0)?;
            }
            Tok::Ident(name) if name == "Mesh" => {
                cur.next();
                let mesh_prims = parse_x_mesh_block(&mut cur, path, 0)?;
                scene
                    .primitives
                    .extend(mesh_prims.into_iter().map(imported_primitive_from_mesh));
            }
            Tok::Ident(name) if name == "AnimationSet" => {
                cur.next();
                parse_x_animation_set(&mut cur, &mut scene.animations)?;
            }
            Tok::Ident(name) if name == "AnimTicksPerSecond" => {
                cur.next();
                scene.ticks_per_second = parse_x_anim_ticks_per_second(&mut cur)
                    .unwrap_or(scene.ticks_per_second)
                    .max(1.0);
            }
            Tok::Ident(_) => {
                cur.next();
                cur.skip_block();
            }
            _ => {
                cur.next();
            }
        }
    }
    Ok(scene)
}

fn parse_x_tokens(toks: Vec<Tok>, path: &Path) -> Result<MeshAsset> {
    import_x_scene_tokens(toks, path)?.into_mesh_asset(path)
}

fn import_x_scene_text(text: &str, path: &Path) -> Result<ImportedXScene> {
    import_x_scene_tokens(tokenize_x(text), path)
}

fn import_x_scene_binary(bytes: &[u8], path: &Path, float_bits: u32) -> Result<ImportedXScene> {
    import_x_scene_tokens(tokenize_x_binary(bytes, float_bits)?, path)
}

fn import_x_scene_bytes(bytes: &[u8], path: &Path) -> Result<ImportedXScene> {
    import_x_scene_bytes_shion(bytes, path)
}

fn parse_x_text(text: &str, path: &Path) -> Result<MeshAsset> {
    import_x_scene_text(text, path)?.into_mesh_asset(path)
}

fn parse_x_binary(bytes: &[u8], path: &Path, float_bits: u32) -> Result<MeshAsset> {
    import_x_scene_binary(bytes, path, float_bits)?.into_mesh_asset(path)
}

fn parse_x_bytes(bytes: &[u8], path: &Path) -> Result<MeshAsset> {
    import_x_scene_bytes(bytes, path)?.into_mesh_asset(path)
}

fn imported_primitive_from_mesh(prim: MeshPrimitive) -> ImportedXPrimitive {
    ImportedXPrimitive {
        frame_node: prim.frame_index,
        texture_path: prim.texture_path,
        material: prim.material,
        vertices: prim.vertices,
        bones: prim
            .bones
            .into_iter()
            .map(|bone| ImportedXBoneBinding {
                frame_name: bone.frame_name,
                offset_matrix: bone.offset_matrix,
            })
            .collect(),
    }
}

fn import_x_frame_block(
    cur: &mut Cursor,
    path: &Path,
    scene: &mut ImportedXScene,
    parent_frame: usize,
) -> Result<usize> {
    let name = cur
        .optional_name_before_block()
        .unwrap_or_else(|| format!("frame_{}", scene.frames.len()));
    cur.expect_block_start()?;
    let frame_idx = scene.frames.len();
    scene.frames.push(ImportedXFrame {
        name,
        parent: Some(parent_frame),
        base_local: Mat4::identity(),
    });
    while let Some(tok) = cur.peek().cloned() {
        match tok {
            Tok::Sym('}') => {
                cur.next();
                break;
            }
            Tok::Ident(name) if name == "FrameTransformMatrix" => {
                cur.next();
                scene.frames[frame_idx].base_local = parse_x_matrix_block(cur)?;
            }
            Tok::Ident(name) if name == "Frame" => {
                cur.next();
                let _ = import_x_frame_block(cur, path, scene, frame_idx)?;
            }
            Tok::Ident(name) if name == "Mesh" => {
                cur.next();
                let mesh_prims = parse_x_mesh_block(cur, path, frame_idx)?;
                scene
                    .primitives
                    .extend(mesh_prims.into_iter().map(imported_primitive_from_mesh));
            }
            Tok::Ident(name) if name == "AnimationSet" => {
                cur.next();
                cur.skip_block();
            }
            Tok::Ident(_) => {
                cur.next();
                cur.skip_block();
            }
            _ => {
                cur.next();
            }
        }
    }
    Ok(frame_idx)
}

fn parse_x_mesh_block(
    cur: &mut Cursor,
    path: &Path,
    frame_index: usize,
) -> Result<Vec<MeshPrimitive>> {
    cur.expect_block_start()?;
    let vertex_count = cur.next_usize()?;
    let mut positions = Vec::with_capacity(vertex_count);
    for _ in 0..vertex_count {
        positions.push([cur.next_number()?, cur.next_number()?, cur.next_number()?]);
    }
    let face_count = cur.next_usize()?;
    let mut faces = Vec::<XFace>::with_capacity(face_count);
    for _ in 0..face_count {
        let n = cur.next_usize()?;
        let mut face = Vec::with_capacity(n);
        for _ in 0..n {
            face.push(cur.next_usize()?);
        }
        faces.push(XFace {
            indices: face,
            material_index: 0,
        });
    }

    let mut texcoords = Vec::<[f32; 2]>::new();
    let mut normals = Vec::<[f32; 3]>::new();
    let mut vertex_colors = HashMap::<usize, [f32; 4]>::new();
    let mut influences: HashMap<usize, Vec<(usize, f32)>> = HashMap::new();
    let mut bones = Vec::<BoneBinding>::new();
    let mut bone_map: HashMap<String, usize> = HashMap::new();
    let mut texture_path: Option<PathBuf> = None;
    let mut materials = vec![ParsedXMaterial {
        name: None,
        material: default_mesh_material(),
        texture_path: None,
    }];

    while let Some(tok) = cur.peek().cloned() {
        match tok {
            Tok::Sym('}') => {
                cur.next();
                break;
            }
            Tok::Ident(name) if name == "MeshTextureCoords" => {
                cur.next();
                texcoords = parse_x_texcoords(cur)?;
            }
            Tok::Ident(name) if name == "MeshNormals" => {
                cur.next();
                normals = parse_x_normals(cur)?;
            }
            Tok::Ident(name) if name == "MeshVertexColors" => {
                cur.next();
                vertex_colors = parse_x_vertex_colors(cur)?;
            }
            Tok::Ident(name) if name == "SkinWeights" => {
                cur.next();
                parse_x_skin_weights(cur, &mut influences, &mut bones, &mut bone_map)?;
            }
            Tok::Ident(name) if name == "TextureFilename" => {
                cur.next();
                texture_path = parse_x_texture_filename(cur, path);
            }
            Tok::Ident(name) if name == "MeshMaterialList" => {
                cur.next();
                let (face_mtls, parsed_mtls) = parse_x_material_list(cur, path)?;
                for (face, midx) in faces.iter_mut().zip(face_mtls.into_iter()) {
                    face.material_index = midx;
                }
                if !parsed_mtls.is_empty() {
                    materials = parsed_mtls;
                }
            }
            Tok::Ident(_) => {
                cur.next();
                cur.skip_block();
            }
            _ => {
                cur.next();
            }
        }
    }

    let mut groups: HashMap<usize, Vec<MeshTriVertex>> = HashMap::new();
    for face in faces {
        if face.indices.len() < 3 {
            continue;
        }
        for tri in 1..face.indices.len() - 1 {
            let idxs = [face.indices[0], face.indices[tri], face.indices[tri + 1]];
            let tri_n = triangle_normal(
                positions.get(idxs[0]).copied().unwrap_or([0.0, 0.0, 0.0]),
                positions.get(idxs[1]).copied().unwrap_or([0.0, 0.0, 0.0]),
                positions.get(idxs[2]).copied().unwrap_or([0.0, 0.0, 0.0]),
            );
            let group = groups.entry(face.material_index).or_default();
            for &idx in &idxs {
                let pos = positions.get(idx).copied().unwrap_or([0.0, 0.0, 0.0]);
                let uv = texcoords.get(idx).copied().unwrap_or([0.0, 0.0]);
                let normal = normals.get(idx).copied().unwrap_or(tri_n);
                let (bone_indices, bone_weights) = packed_influences(influences.get(&idx));
                let color = vertex_colors
                    .get(&idx)
                    .copied()
                    .unwrap_or([1.0, 1.0, 1.0, 1.0]);
                group.push(MeshTriVertex {
                    pos,
                    uv,
                    normal,
                    tangent: [0.0, 0.0, 0.0],
                    binormal: [0.0, 0.0, 0.0],
                    color,
                    bone_indices,
                    bone_weights,
                });
            }
        }
    }

    let mut out = Vec::new();
    for (midx, vertices) in groups {
        if vertices.is_empty() {
            continue;
        }
        let parsed = materials.get(midx).cloned().unwrap_or(ParsedXMaterial {
            name: None,
            material: default_mesh_material(),
            texture_path: None,
        });
        let mut vertices = vertices;
        apply_tangent_space(&mut vertices);
        out.push(MeshPrimitive {
            frame_index,
            texture_path: parsed.texture_path.clone().or_else(|| texture_path.clone()),
            material: parsed.material,
            runtime_desc: MeshPrimitiveRuntimeDesc::default(),
            vertices,
            bones: bones.clone(),
        });
    }
    Ok(out)
}

fn parse_x_material_list(
    cur: &mut Cursor,
    mesh_path: &Path,
) -> Result<(Vec<usize>, Vec<ParsedXMaterial>)> {
    cur.expect_block_start()?;
    let material_count = cur.next_usize()?;
    let face_count = cur.next_usize()?;
    let mut face_mtls = Vec::with_capacity(face_count);
    for _ in 0..face_count {
        face_mtls.push(cur.next_usize()?);
    }
    let mut materials = Vec::new();
    let mut named_materials = HashMap::<String, ParsedXMaterial>::new();
    while let Some(tok) = cur.peek().cloned() {
        match tok {
            Tok::Sym('}') => {
                cur.next();
                break;
            }
            Tok::Ident(name) if name == "Material" => {
                cur.next();
                let parsed = parse_x_material(cur, mesh_path)?;
                if let Some(name) = parsed.name.clone() {
                    named_materials.insert(name, parsed.clone());
                }
                materials.push(parsed);
            }
            Tok::Sym('{') => {
                cur.next();
                let ref_name = match cur.peek() {
                    Some(Tok::Ident(s)) => Some(s.clone()),
                    Some(Tok::Str(s)) => Some(s.clone()),
                    _ => None,
                };
                if ref_name.is_some() {
                    let _ = cur.next();
                }
                while !cur.consume_sym('}') {
                    if cur.next().is_none() {
                        break;
                    }
                }
                let parsed = ref_name
                    .and_then(|name| named_materials.get(&name).cloned())
                    .unwrap_or(ParsedXMaterial {
                        name: None,
                        material: default_mesh_material(),
                        texture_path: None,
                    });
                materials.push(parsed);
            }
            _ => {
                cur.next();
            }
        }
    }
    while materials.len() < material_count {
        materials.push(ParsedXMaterial {
            name: None,
            material: default_mesh_material(),
            texture_path: None,
        });
    }
    Ok((face_mtls, materials))
}

fn parse_x_material(cur: &mut Cursor, mesh_path: &Path) -> Result<ParsedXMaterial> {
    let name = cur.optional_name_before_block();
    cur.expect_block_start()?;
    let diffuse = [
        cur.next_number()?,
        cur.next_number()?,
        cur.next_number()?,
        cur.next_number()?,
    ];
    let power = cur.next_number()?.max(1.0);
    let specular = [
        cur.next_number()?,
        cur.next_number()?,
        cur.next_number()?,
        1.0,
    ];
    let emissive = [
        cur.next_number()?,
        cur.next_number()?,
        cur.next_number()?,
        1.0,
    ];
    let mut material = default_mesh_material();
    material.diffuse = diffuse;
    material.ambient = [diffuse[0], diffuse[1], diffuse[2], diffuse[3]];
    material.specular = specular;
    material.emissive = emissive;
    material.power = power;
    let mut texture_path = None;
    while let Some(tok) = cur.peek().cloned() {
        match tok {
            Tok::Sym('}') => {
                cur.next();
                break;
            }
            Tok::Ident(name) if name == "TextureFilename" => {
                cur.next();
                texture_path = parse_x_texture_filename(cur, mesh_path);
            }
            Tok::Ident(name) if name == "EffectInstance" => {
                cur.next();
                parse_x_effect_instance(cur, mesh_path, &mut material)?;
            }
            Tok::Ident(_) => {
                cur.next();
                cur.skip_block();
            }
            _ => {
                cur.next();
            }
        }
    }
    Ok(ParsedXMaterial {
        name,
        material,
        texture_path,
    })
}

fn parse_x_matrix_block(cur: &mut Cursor) -> Result<Mat4> {
    cur.expect_block_start()?;
    let mut vals = [0.0f32; 16];
    for v in &mut vals {
        *v = cur.next_number()?;
    }
    let _ = cur.consume_sym('}');
    Ok(Mat4::from_x_values(vals))
}

fn parse_x_texcoords(cur: &mut Cursor) -> Result<Vec<[f32; 2]>> {
    cur.expect_block_start()?;
    let count = cur.next_usize()?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        out.push([cur.next_number()?, cur.next_number()?]);
    }
    let _ = cur.consume_sym('}');
    Ok(out)
}

fn parse_x_normals(cur: &mut Cursor) -> Result<Vec<[f32; 3]>> {
    cur.expect_block_start()?;
    let count = cur.next_usize()?;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        out.push(normalize3([
            cur.next_number()?,
            cur.next_number()?,
            cur.next_number()?,
        ]));
    }
    let face_count = cur.next_usize()?;
    for _ in 0..face_count {
        let n = cur.next_usize()?;
        for _ in 0..n {
            let _ = cur.next_usize()?;
        }
    }
    let _ = cur.consume_sym('}');
    Ok(out)
}

fn parse_x_vertex_colors(cur: &mut Cursor) -> Result<HashMap<usize, [f32; 4]>> {
    cur.expect_block_start()?;
    let count = cur.next_usize()?;
    let mut out = HashMap::with_capacity(count);
    for _ in 0..count {
        let vidx = cur.next_usize()?;
        let rgba = [
            cur.next_number()?,
            cur.next_number()?,
            cur.next_number()?,
            cur.next_number()?,
        ];
        out.insert(vidx, rgba);
    }
    let _ = cur.consume_sym('}');
    Ok(out)
}

fn parse_x_skin_weights(
    cur: &mut Cursor,
    influences: &mut HashMap<usize, Vec<(usize, f32)>>,
    bones: &mut Vec<BoneBinding>,
    bone_map: &mut HashMap<String, usize>,
) -> Result<()> {
    cur.expect_block_start()?;
    let bone_name = cur.next_string()?;
    let bone_idx = if let Some(idx) = bone_map.get(&bone_name).copied() {
        idx
    } else {
        let idx = bones.len();
        bones.push(BoneBinding {
            frame_name: bone_name.clone(),
            frame_index: 0,
            offset_matrix: Mat4::identity(),
        });
        bone_map.insert(bone_name.clone(), idx);
        idx
    };
    let count = cur.next_usize()?;
    let mut verts = Vec::with_capacity(count);
    for _ in 0..count {
        verts.push(cur.next_usize()?);
    }
    let mut weights = Vec::with_capacity(count);
    for _ in 0..count {
        weights.push(cur.next_number()?);
    }
    let mut vals = [0.0f32; 16];
    for v in &mut vals {
        *v = cur.next_number()?;
    }
    bones[bone_idx].offset_matrix = Mat4::from_x_values(vals);
    let _ = cur.consume_sym('}');
    for (vidx, weight) in verts.into_iter().zip(weights.into_iter()) {
        influences.entry(vidx).or_default().push((bone_idx, weight));
    }
    Ok(())
}

fn parse_x_texture_filename(cur: &mut Cursor, mesh_path: &Path) -> Option<PathBuf> {
    if cur.expect_block_start().is_err() {
        return None;
    }
    let name = cur.next_string().ok()?;
    let _ = cur.consume_sym('}');
    resolve_texture_path(mesh_path, &name)
}

fn parse_x_anim_ticks_per_second(cur: &mut Cursor) -> Result<f32> {
    cur.expect_block_start()?;
    let ticks = cur.next_number()?.max(1.0);
    let _ = cur.consume_sym('}');
    Ok(ticks)
}

fn parse_x_animation_options(cur: &mut Cursor, clip: &mut AnimationClip) -> Result<()> {
    cur.expect_block_start()?;
    clip.open_closed = cur.next_i64().unwrap_or(1) != 0;
    let _ = cur.next_i64();
    let _ = cur.consume_sym('}');
    Ok(())
}

fn parse_x_animation_set(cur: &mut Cursor, out: &mut Vec<AnimationClip>) -> Result<()> {
    let name = cur
        .optional_name_before_block()
        .unwrap_or_else(|| format!("anim_{}", out.len()));
    cur.expect_block_start()?;
    let mut clip = AnimationClip {
        name,
        open_closed: true,
        ..Default::default()
    };
    while let Some(tok) = cur.peek().cloned() {
        match tok {
            Tok::Sym('}') => {
                cur.next();
                break;
            }
            Tok::Ident(id) if id == "Animation" => {
                cur.next();
                parse_x_animation(cur, &mut clip)?;
            }
            Tok::Ident(id) if id == "AnimationOptions" => {
                cur.next();
                parse_x_animation_options(cur, &mut clip)?;
            }
            Tok::Ident(_) => {
                cur.next();
                cur.skip_block();
            }
            _ => {
                cur.next();
            }
        }
    }
    if !clip.tracks.is_empty() {
        out.push(clip);
    }
    Ok(())
}

fn parse_x_animation(cur: &mut Cursor, clip: &mut AnimationClip) -> Result<()> {
    let _anim_name = cur.optional_name_before_block();
    cur.expect_block_start()?;
    let mut target_name: Option<String> = None;
    while let Some(tok) = cur.peek().cloned() {
        match tok {
            Tok::Sym('}') => {
                cur.next();
                break;
            }
            Tok::Sym('{') => {
                cur.next();
                if target_name.is_none() {
                    target_name = Some(cur.next_string()?);
                }
                while !cur.consume_sym('}') {
                    let _ = cur.next();
                }
            }
            Tok::Ident(id) if id == "AnimationKey" => {
                cur.next();
                if let Some(target) = target_name.clone() {
                    parse_x_animation_key(cur, clip, &target)?;
                } else {
                    cur.skip_block();
                }
            }
            Tok::Ident(_) => {
                cur.next();
                cur.skip_block();
            }
            _ => {
                cur.next();
            }
        }
    }
    Ok(())
}

fn parse_x_animation_key(
    cur: &mut Cursor,
    clip: &mut AnimationClip,
    target_name: &str,
) -> Result<()> {
    cur.expect_block_start()?;
    let key_type = cur.next_i64()?;
    let count = cur.next_usize()?;
    let track = clip.tracks.entry(target_name.to_string()).or_default();
    for _ in 0..count {
        let time = cur.next_i64()?;
        let nvals = cur.next_usize()?;
        clip.max_time = clip.max_time.max(time);
        match key_type {
            0 => {
                let vals = read_n_numbers(cur, nvals)?;
                let q = if vals.len() >= 4 {
                    // .x rotation keys are written as w, x, y, z.
                    Quat {
                        x: vals[1],
                        y: vals[2],
                        z: vals[3],
                        w: vals[0],
                    }
                    .normalize()
                } else {
                    Quat {
                        x: 0.0,
                        y: 0.0,
                        z: 0.0,
                        w: 1.0,
                    }
                };
                track.rotation_keys.push(QuatKey { time, value: q });
            }
            1 => {
                let vals = read_n_numbers(cur, nvals)?;
                let v = [
                    vals.get(0).copied().unwrap_or(1.0),
                    vals.get(1).copied().unwrap_or(1.0),
                    vals.get(2).copied().unwrap_or(1.0),
                ];
                track.scale_keys.push(Vec3Key { time, value: v });
            }
            2 => {
                let vals = read_n_numbers(cur, nvals)?;
                let v = [
                    vals.get(0).copied().unwrap_or(0.0),
                    vals.get(1).copied().unwrap_or(0.0),
                    vals.get(2).copied().unwrap_or(0.0),
                ];
                track.position_keys.push(Vec3Key { time, value: v });
            }
            4 => {
                let vals = read_n_numbers(cur, nvals)?;
                let mut m = [0.0f32; 16];
                for (dst, src) in m.iter_mut().zip(vals.into_iter()) {
                    *dst = src;
                }
                track.matrix_keys.push(MatKey {
                    time,
                    value: Mat4::from_x_values(m),
                });
            }
            _ => {
                let _ = read_n_numbers(cur, nvals)?;
            }
        }
    }
    let _ = cur.consume_sym('}');
    Ok(())
}

fn read_n_numbers(cur: &mut Cursor, n: usize) -> Result<Vec<f32>> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(cur.next_number()?);
    }
    Ok(out)
}

fn packed_influences(src: Option<&Vec<(usize, f32)>>) -> ([u16; 4], [f32; 4]) {
    let mut bone_indices = [0u16; 4];
    let mut bone_weights = [0.0f32; 4];
    let Some(src) = src else {
        return (bone_indices, bone_weights);
    };
    let mut vals = src.clone();
    vals.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut sum = 0.0f32;
    for (dst_idx, (bone, weight)) in vals.into_iter().take(4).enumerate() {
        bone_indices[dst_idx] = bone as u16;
        bone_weights[dst_idx] = weight.max(0.0);
        sum += bone_weights[dst_idx];
    }
    if sum > 0.0 {
        for w in &mut bone_weights {
            *w /= sum;
        }
    }
    (bone_indices, bone_weights)
}

fn resolve_texture_path(mesh_path: &Path, tex_name: &str) -> Option<PathBuf> {
    let p = Path::new(tex_name);
    if p.is_absolute() {
        if let Some(found) = find_existing_casefold(p) {
            return Some(found);
        }
    }
    let mut candidates = Vec::new();
    if let Some(parent) = mesh_path.parent() {
        if let Some(found) = resolve_relative_casefold(parent, tex_name) {
            return Some(found);
        }
        candidates.push(parent.join(tex_name));
    }
    candidates.push(PathBuf::from(tex_name));
    if p.extension().is_none() {
        for ext in ["png", "bmp", "jpg", "jpeg", "g00"] {
            if let Some(parent) = mesh_path.parent() {
                candidates.push(parent.join(format!("{}.{}", tex_name, ext)));
            }
        }
    }
    candidates
        .into_iter()
        .find_map(|p| find_existing_casefold(&p))
}

fn tokenize_x(text: &str) -> Vec<Tok> {
    let mut out = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let ch = chars[i];
        if ch == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if ch.is_whitespace() || ch == ';' || ch == ',' {
            i += 1;
            continue;
        }
        if ch == '{' || ch == '}' {
            out.push(Tok::Sym(ch));
            i += 1;
            continue;
        }
        if ch == '"' {
            i += 1;
            let start = i;
            while i < chars.len() && chars[i] != '"' {
                i += 1;
            }
            out.push(Tok::Str(chars[start..i].iter().collect()));
            if i < chars.len() {
                i += 1;
            }
            continue;
        }
        if ch.is_ascii_digit() || ch == '-' || ch == '+' || ch == '.' {
            let start = i;
            i += 1;
            while i < chars.len() {
                let c = chars[i];
                if c.is_ascii_digit() || matches!(c, '.' | '-' | '+' | 'e' | 'E') {
                    i += 1;
                } else {
                    break;
                }
            }
            let s: String = chars[start..i].iter().collect();
            if let Ok(v) = s.parse::<f32>() {
                out.push(Tok::Number(v));
            }
            continue;
        }
        if ch.is_alphanumeric() || ch == '_' || ch == '.' {
            let start = i;
            i += 1;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                i += 1;
            }
            out.push(Tok::Ident(chars[start..i].iter().collect()));
            continue;
        }
        i += 1;
    }
    out
}

fn tokenize_x_binary(bytes: &[u8], float_bits: u32) -> Result<Vec<Tok>> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 2 <= bytes.len() {
        let token = u16::from_le_bytes([bytes[i], bytes[i + 1]]);
        i += 2;
        match token {
            1 => {
                let count = read_u32_le(bytes, &mut i)? as usize;
                let raw = read_bytes(bytes, &mut i, count)?;
                out.push(Tok::Ident(trim_x_string(raw)));
            }
            2 => {
                let count = read_u32_le(bytes, &mut i)? as usize;
                let raw = read_bytes(bytes, &mut i, count)?;
                out.push(Tok::Str(trim_x_string(raw)));
            }
            3 => {
                let v = read_u32_le(bytes, &mut i)? as i32;
                out.push(Tok::Number(v as f32));
            }
            5 => {
                let _ = read_bytes(bytes, &mut i, 16)?;
            }
            6 => {
                let count = read_u32_le(bytes, &mut i)? as usize;
                for _ in 0..count {
                    let v = read_u32_le(bytes, &mut i)? as i32;
                    out.push(Tok::Number(v as f32));
                }
            }
            7 => {
                let count = read_u32_le(bytes, &mut i)? as usize;
                if float_bits >= 64 {
                    for _ in 0..count {
                        let v = read_f64_le(bytes, &mut i)?;
                        out.push(Tok::Number(v as f32));
                    }
                } else {
                    for _ in 0..count {
                        let v = read_f32_le(bytes, &mut i)?;
                        out.push(Tok::Number(v));
                    }
                }
            }
            10 => out.push(Tok::Sym('{')),
            11 => out.push(Tok::Sym('}')),
            12 => out.push(Tok::Sym('(')),
            13 => out.push(Tok::Sym(')')),
            14 => out.push(Tok::Sym('[')),
            15 => out.push(Tok::Sym(']')),
            16 => out.push(Tok::Sym('<')),
            17 => out.push(Tok::Sym('>')),
            18 => out.push(Tok::Sym('.')),
            19 => out.push(Tok::Sym(',')),
            20 => out.push(Tok::Sym(';')),
            31 => out.push(Tok::Ident("template".to_string())),
            40 => out.push(Tok::Ident("WORD".to_string())),
            41 => out.push(Tok::Ident("DWORD".to_string())),
            42 => out.push(Tok::Ident("FLOAT".to_string())),
            43 => out.push(Tok::Ident("DOUBLE".to_string())),
            44 => out.push(Tok::Ident("CHAR".to_string())),
            45 => out.push(Tok::Ident("UCHAR".to_string())),
            46 => out.push(Tok::Ident("SWORD".to_string())),
            47 => out.push(Tok::Ident("SDWORD".to_string())),
            48 => out.push(Tok::Ident("VOID".to_string())),
            49 => out.push(Tok::Ident("LPSTR".to_string())),
            50 => out.push(Tok::Ident("UNICODE".to_string())),
            51 => out.push(Tok::Ident("CSTRING".to_string())),
            52 => out.push(Tok::Ident("ARRAY".to_string())),
            _ => {
                bail!(
                    "unsupported .x binary token {} at offset {}",
                    token,
                    i.saturating_sub(2)
                );
            }
        }
    }
    Ok(out)
}

fn read_bytes<'a>(bytes: &'a [u8], i: &mut usize, n: usize) -> Result<&'a [u8]> {
    if bytes.len().saturating_sub(*i) < n {
        bail!("unexpected eof in .x binary stream")
    }
    let out = &bytes[*i..*i + n];
    *i += n;
    Ok(out)
}

fn read_u32_le(bytes: &[u8], i: &mut usize) -> Result<u32> {
    let raw = read_bytes(bytes, i, 4)?;
    Ok(u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn read_f32_le(bytes: &[u8], i: &mut usize) -> Result<f32> {
    let raw = read_bytes(bytes, i, 4)?;
    Ok(f32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]))
}

fn read_f64_le(bytes: &[u8], i: &mut usize) -> Result<f64> {
    let raw = read_bytes(bytes, i, 8)?;
    Ok(f64::from_le_bytes([
        raw[0], raw[1], raw[2], raw[3], raw[4], raw[5], raw[6], raw[7],
    ]))
}

fn trim_x_string(raw: &[u8]) -> String {
    let s = String::from_utf8_lossy(raw).to_string();
    s.trim_end_matches('\0').to_string()
}

pub const SIGLUS_INTERNAL_MESH_MAGIC: &[u8; 8] = b"SGMESH10";
pub const SIGLUS_INTERNAL_MESH_VERSION: u32 = 3;

pub fn write_internal_mesh_asset(path: &Path, asset: &MeshAsset) -> Result<()> {
    let mut writer = AssetWriter::default();
    writer.write_bytes(SIGLUS_INTERNAL_MESH_MAGIC)?;
    writer.write_u32(SIGLUS_INTERNAL_MESH_VERSION)?;
    let base_dir = asset.source_path.parent().unwrap_or_else(|| Path::new(""));
    writer.write_string(&normalize_runtime_path_string(&asset.source_path))?;
    writer.write_opt_path(asset.texture_path.as_deref(), base_dir)?;
    writer.write_u32(asset.bone_count as u32)?;
    writer.write_f32(asset.ticks_per_second)?;

    writer.write_u32(asset.frames.len() as u32)?;
    for frame in &asset.frames {
        writer.write_string(&frame.name)?;
        writer.write_mat4(frame.base_local)?;
        writer.write_u32(frame.children.len() as u32)?;
        for &child in &frame.children {
            writer.write_u32(child as u32)?;
        }
    }

    writer.write_u32(asset.primitives.len() as u32)?;
    for prim in &asset.primitives {
        writer.write_u32(prim.frame_index as u32)?;
        writer.write_opt_path(prim.texture_path.as_deref(), base_dir)?;
        writer.write_mesh_material(&prim.material, base_dir)?;
        writer.write_u32(prim.vertices.len() as u32)?;
        for vertex in &prim.vertices {
            writer.write_vertex(vertex)?;
        }
        writer.write_u32(prim.bones.len() as u32)?;
        for bone in &prim.bones {
            writer.write_string(&bone.frame_name)?;
            writer.write_u32(bone.frame_index as u32)?;
            writer.write_mat4(bone.offset_matrix)?;
        }
        writer.write_mesh_runtime_desc(&prim.runtime_desc)?;
    }

    writer.write_u32(asset.animations.len() as u32)?;
    for clip in &asset.animations {
        writer.write_string(&clip.name)?;
        writer.write_i64(clip.max_time)?;
        writer.write_bool(clip.open_closed)?;
        writer.write_u32(clip.tracks.len() as u32)?;
        for (track_name, track) in &clip.tracks {
            writer.write_string(track_name)?;
            writer.write_u32(track.rotation_keys.len() as u32)?;
            for key in &track.rotation_keys {
                writer.write_i64(key.time)?;
                writer.write_quat(key.value)?;
            }
            writer.write_u32(track.scale_keys.len() as u32)?;
            for key in &track.scale_keys {
                writer.write_i64(key.time)?;
                writer.write_vec3(key.value)?;
            }
            writer.write_u32(track.position_keys.len() as u32)?;
            for key in &track.position_keys {
                writer.write_i64(key.time)?;
                writer.write_vec3(key.value)?;
            }
            writer.write_u32(track.matrix_keys.len() as u32)?;
            for key in &track.matrix_keys {
                writer.write_i64(key.time)?;
                writer.write_mat4(key.value)?;
            }
        }
    }

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("create mesh asset dir {:?}", parent))?;
    }
    fs::write(path, writer.finish())
        .with_context(|| format!("write internal mesh asset {:?}", path))?;
    Ok(())
}

pub fn read_internal_mesh_asset(path: &Path) -> Result<MeshAsset> {
    let bytes = fs::read(path).with_context(|| format!("read internal mesh asset {:?}", path))?;
    let mut reader = AssetReader::new(&bytes);
    let magic = reader.read_fixed::<8>()?;
    if &magic != SIGLUS_INTERNAL_MESH_MAGIC {
        bail!("invalid internal mesh asset magic in {:?}", path);
    }
    let version = reader.read_u32()?;
    if version == 0 || version > SIGLUS_INTERNAL_MESH_VERSION {
        bail!(
            "unsupported internal mesh asset version {} in {:?}",
            version,
            path
        );
    }
    let base_dir = path.parent().unwrap_or_else(|| Path::new(""));
    let source_path = resolve_runtime_asset_path(base_dir, &reader.read_string()?);
    let texture_path = reader.read_opt_path(base_dir)?;
    let bone_count = reader.read_u32()? as usize;
    let ticks_per_second = reader.read_f32()?;

    let frame_count = reader.read_u32()? as usize;
    let mut frames = Vec::with_capacity(frame_count);
    for _ in 0..frame_count {
        let name = reader.read_string()?;
        let base_local = reader.read_mat4()?;
        let child_count = reader.read_u32()? as usize;
        let mut children = Vec::with_capacity(child_count);
        for _ in 0..child_count {
            children.push(reader.read_u32()? as usize);
        }
        frames.push(FrameNode {
            name,
            base_local,
            children,
        });
    }

    let prim_count = reader.read_u32()? as usize;
    let mut primitives = Vec::with_capacity(prim_count);
    for _ in 0..prim_count {
        let frame_index = reader.read_u32()? as usize;
        let texture_path = reader.read_opt_path(base_dir)?;
        let material = reader.read_mesh_material(base_dir)?;
        let vertex_count = reader.read_u32()? as usize;
        let mut vertices = Vec::with_capacity(vertex_count);
        for _ in 0..vertex_count {
            vertices.push(reader.read_vertex()?);
        }
        let bone_binding_count = reader.read_u32()? as usize;
        let mut bones = Vec::with_capacity(bone_binding_count);
        for _ in 0..bone_binding_count {
            bones.push(BoneBinding {
                frame_name: reader.read_string()?,
                frame_index: reader.read_u32()? as usize,
                offset_matrix: reader.read_mat4()?,
            });
        }
        let runtime_desc = if version >= 2 {
            reader.read_mesh_runtime_desc(version)?
        } else {
            build_mesh_primitive_runtime_desc_from_material(
                &material,
                texture_path.is_some(),
                !bones.is_empty(),
                vertex_count as u32,
                bone_binding_count as u32,
            )
        };
        primitives.push(MeshPrimitive {
            frame_index,
            texture_path,
            material,
            runtime_desc,
            vertices,
            bones,
        });
    }

    let anim_count = reader.read_u32()? as usize;
    let mut animations = Vec::with_capacity(anim_count);
    for _ in 0..anim_count {
        let name = reader.read_string()?;
        let max_time = reader.read_i64()?;
        let open_closed = reader.read_bool()?;
        let track_count = reader.read_u32()? as usize;
        let mut tracks = HashMap::new();
        for _ in 0..track_count {
            let track_name = reader.read_string()?;
            let rotation_count = reader.read_u32()? as usize;
            let mut rotation_keys = Vec::with_capacity(rotation_count);
            for _ in 0..rotation_count {
                rotation_keys.push(QuatKey {
                    time: reader.read_i64()?,
                    value: reader.read_quat()?,
                });
            }
            let scale_count = reader.read_u32()? as usize;
            let mut scale_keys = Vec::with_capacity(scale_count);
            for _ in 0..scale_count {
                scale_keys.push(Vec3Key {
                    time: reader.read_i64()?,
                    value: reader.read_vec3()?,
                });
            }
            let position_count = reader.read_u32()? as usize;
            let mut position_keys = Vec::with_capacity(position_count);
            for _ in 0..position_count {
                position_keys.push(Vec3Key {
                    time: reader.read_i64()?,
                    value: reader.read_vec3()?,
                });
            }
            let matrix_count = reader.read_u32()? as usize;
            let mut matrix_keys = Vec::with_capacity(matrix_count);
            for _ in 0..matrix_count {
                matrix_keys.push(MatKey {
                    time: reader.read_i64()?,
                    value: reader.read_mat4()?,
                });
            }
            tracks.insert(
                track_name,
                AnimationTrack {
                    rotation_keys,
                    scale_keys,
                    position_keys,
                    matrix_keys,
                },
            );
        }
        animations.push(AnimationClip {
            name,
            tracks,
            max_time,
            open_closed,
        });
    }

    let mut asset = MeshAsset {
        source_path,
        texture_path,
        vertices: Vec::new(),
        bone_count,
        ticks_per_second,
        primitives,
        frames,
        animations,
    };
    finalize_mesh_asset(&mut asset);
    Ok(asset)
}

fn normalize_runtime_path_string(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn normalize_runtime_rel_path(path: &Path, base_dir: &Path) -> String {
    if let Ok(rel) = path.strip_prefix(base_dir) {
        return normalize_runtime_path_string(rel);
    }
    normalize_runtime_path_string(path)
}

fn resolve_runtime_asset_path(base_dir: &Path, raw: &str) -> PathBuf {
    let path = Path::new(raw);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    base_dir.join(path)
}

#[derive(Default)]
struct AssetWriter {
    bytes: Vec<u8>,
}

impl AssetWriter {
    fn finish(self) -> Vec<u8> {
        self.bytes
    }

    fn write_bytes(&mut self, raw: &[u8]) -> Result<()> {
        self.bytes.write_all(raw)?;
        Ok(())
    }

    fn write_u8(&mut self, value: u8) -> Result<()> {
        self.bytes.write_all(&[value])?;
        Ok(())
    }

    fn write_bool(&mut self, value: bool) -> Result<()> {
        self.write_u8(if value { 1 } else { 0 })
    }

    fn write_u16(&mut self, value: u16) -> Result<()> {
        self.bytes.write_all(&value.to_le_bytes())?;
        Ok(())
    }

    fn write_u32(&mut self, value: u32) -> Result<()> {
        self.bytes.write_all(&value.to_le_bytes())?;
        Ok(())
    }

    fn write_i32(&mut self, value: i32) -> Result<()> {
        self.bytes.write_all(&value.to_le_bytes())?;
        Ok(())
    }

    fn write_i64(&mut self, value: i64) -> Result<()> {
        self.bytes.write_all(&value.to_le_bytes())?;
        Ok(())
    }

    fn write_f32(&mut self, value: f32) -> Result<()> {
        self.bytes.write_all(&value.to_le_bytes())?;
        Ok(())
    }

    fn write_string(&mut self, value: &str) -> Result<()> {
        let raw = value.as_bytes();
        self.write_u32(raw.len() as u32)?;
        self.bytes.write_all(raw)?;
        Ok(())
    }

    fn write_opt_string(&mut self, value: Option<&str>) -> Result<()> {
        match value {
            Some(v) => {
                self.write_bool(true)?;
                self.write_string(v)?;
                Ok(())
            }
            None => self.write_bool(false),
        }
    }

    fn write_opt_path(&mut self, value: Option<&Path>, base_dir: &Path) -> Result<()> {
        self.write_opt_string(
            value
                .map(|p| normalize_runtime_rel_path(p, base_dir))
                .as_deref(),
        )
    }

    fn write_vec3(&mut self, value: [f32; 3]) -> Result<()> {
        for v in value {
            self.write_f32(v)?;
        }
        Ok(())
    }

    fn write_vec4(&mut self, value: [f32; 4]) -> Result<()> {
        for v in value {
            self.write_f32(v)?;
        }
        Ok(())
    }

    fn write_mat4(&mut self, value: Mat4) -> Result<()> {
        for row in value.m {
            self.write_vec4(row)?;
        }
        Ok(())
    }

    fn write_quat(&mut self, value: Quat) -> Result<()> {
        self.write_f32(value.x)?;
        self.write_f32(value.y)?;
        self.write_f32(value.z)?;
        self.write_f32(value.w)?;
        Ok(())
    }

    fn write_vertex(&mut self, value: &MeshTriVertex) -> Result<()> {
        self.write_vec3(value.pos)?;
        self.write_f32(value.uv[0])?;
        self.write_f32(value.uv[1])?;
        self.write_vec3(value.normal)?;
        self.write_vec3(value.tangent)?;
        self.write_vec3(value.binormal)?;
        self.write_vec4(value.color)?;
        for index in value.bone_indices {
            self.write_u16(index)?;
        }
        for weight in value.bone_weights {
            self.write_f32(weight)?;
        }
        Ok(())
    }

    fn write_mesh_material(&mut self, value: &MeshMaterial, base_dir: &Path) -> Result<()> {
        self.write_vec4(value.diffuse)?;
        self.write_vec4(value.ambient)?;
        self.write_vec4(value.specular)?;
        self.write_vec4(value.emissive)?;
        self.write_f32(value.power)?;
        self.write_u32(value.lighting_type as u32)?;
        self.write_u32(value.shading_type as u32)?;
        self.write_u32(value.shader_option)?;
        self.write_vec4(value.rim_light_color)?;
        self.write_f32(value.rim_light_power)?;
        self.write_f32(value.parallax_max_height)?;
        self.write_bool(value.alpha_test_enable)?;
        self.write_f32(value.alpha_ref)?;
        self.write_bool(value.cull_disable)?;
        self.write_bool(value.shadow_map_enable)?;
        self.write_bool(value.use_mesh_tex)?;
        self.write_bool(value.use_mrbd)?;
        self.write_vec4(value.mrbd)?;
        self.write_bool(value.use_rgb)?;
        self.write_vec4(value.rgb_rate)?;
        self.write_vec4(value.add_rgb)?;
        self.write_bool(value.use_mul_vertex_color)?;
        self.write_f32(value.mul_vertex_color_rate)?;
        self.write_f32(value.depth_buffer_shadow_bias)?;
        self.write_u32(value.directional_light_ids.len() as u32)?;
        for id in &value.directional_light_ids {
            self.write_i32(*id)?;
        }
        self.write_u32(value.point_light_ids.len() as u32)?;
        for id in &value.point_light_ids {
            self.write_i32(*id)?;
        }
        self.write_u32(value.spot_light_ids.len() as u32)?;
        for id in &value.spot_light_ids {
            self.write_i32(*id)?;
        }
        self.write_opt_path(value.normal_texture_path.as_deref(), base_dir)?;
        self.write_opt_path(value.toon_texture_path.as_deref(), base_dir)?;
        self.write_opt_string(value.effect_filename.as_deref())?;
        Ok(())
    }

    fn write_mesh_runtime_material_key(&mut self, value: MeshRuntimeMaterialKey) -> Result<()> {
        self.write_bool(value.use_mesh_tex)?;
        self.write_bool(value.use_shadow_tex)?;
        self.write_bool(value.use_toon_tex)?;
        self.write_bool(value.use_normal_tex)?;
        self.write_bool(value.use_mul_vertex_color)?;
        self.write_bool(value.use_mrbd)?;
        self.write_bool(value.use_rgb)?;
        self.write_u32(value.lighting_type as u32)?;
        self.write_u32(value.shading_type as u32)?;
        self.write_u32(value.shader_option)?;
        self.write_bool(value.skinned)?;
        self.write_bool(value.alpha_test_enable)?;
        self.write_bool(value.cull_disable)?;
        self.write_bool(value.shadow_map_enable)?;
        Ok(())
    }

    fn write_mesh_runtime_desc(&mut self, value: &MeshPrimitiveRuntimeDesc) -> Result<()> {
        self.write_u32(value.effect_profile as u32)?;
        self.write_string(&value.effect_key)?;
        self.write_string(&value.technique_name)?;
        self.write_string(&value.shadow_effect_key)?;
        self.write_string(&value.shadow_technique_name)?;
        self.write_bool(value.use_mesh_texture_slot)?;
        self.write_bool(value.use_normal_texture_slot)?;
        self.write_bool(value.use_toon_texture_slot)?;
        self.write_bool(value.use_shadow_texture_slot)?;
        self.write_mesh_runtime_material_key(value.material_key)?;
        self.write_u32(value.vertex_stride_bytes)?;
        self.write_u32(value.vertex_count)?;
        self.write_u32(value.bone_palette_len)?;
        Ok(())
    }
}

struct AssetReader<'a> {
    cursor: IoCursor<&'a [u8]>,
}

impl<'a> AssetReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self {
            cursor: IoCursor::new(bytes),
        }
    }

    fn read_exact_into<const N: usize>(&mut self) -> Result<[u8; N]> {
        let mut raw = [0u8; N];
        self.cursor.read_exact(&mut raw)?;
        Ok(raw)
    }

    fn read_fixed<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.read_exact_into::<N>()
    }

    fn read_u8(&mut self) -> Result<u8> {
        Ok(self.read_exact_into::<1>()?[0])
    }

    fn read_bool(&mut self) -> Result<bool> {
        Ok(self.read_u8()? != 0)
    }

    fn read_u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.read_exact_into::<2>()?))
    }

    fn read_u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.read_exact_into::<4>()?))
    }

    fn read_i32(&mut self) -> Result<i32> {
        Ok(i32::from_le_bytes(self.read_exact_into::<4>()?))
    }

    fn read_i64(&mut self) -> Result<i64> {
        Ok(i64::from_le_bytes(self.read_exact_into::<8>()?))
    }

    fn read_f32(&mut self) -> Result<f32> {
        Ok(f32::from_le_bytes(self.read_exact_into::<4>()?))
    }

    fn read_string(&mut self) -> Result<String> {
        let len = self.read_u32()? as usize;
        let mut raw = vec![0u8; len];
        self.cursor.read_exact(&mut raw)?;
        Ok(String::from_utf8_lossy(&raw).to_string())
    }

    fn read_opt_string(&mut self) -> Result<Option<String>> {
        if self.read_bool()? {
            Ok(Some(self.read_string()?))
        } else {
            Ok(None)
        }
    }

    fn read_opt_path(&mut self, base_dir: &Path) -> Result<Option<PathBuf>> {
        Ok(self
            .read_opt_string()?
            .map(|raw| resolve_runtime_asset_path(base_dir, &raw)))
    }

    fn read_vec3(&mut self) -> Result<[f32; 3]> {
        Ok([self.read_f32()?, self.read_f32()?, self.read_f32()?])
    }

    fn read_vec4(&mut self) -> Result<[f32; 4]> {
        Ok([
            self.read_f32()?,
            self.read_f32()?,
            self.read_f32()?,
            self.read_f32()?,
        ])
    }

    fn read_mat4(&mut self) -> Result<Mat4> {
        Ok(Mat4 {
            m: [
                self.read_vec4()?,
                self.read_vec4()?,
                self.read_vec4()?,
                self.read_vec4()?,
            ],
        })
    }

    fn read_quat(&mut self) -> Result<Quat> {
        Ok(Quat {
            x: self.read_f32()?,
            y: self.read_f32()?,
            z: self.read_f32()?,
            w: self.read_f32()?,
        })
    }

    fn read_vertex(&mut self) -> Result<MeshTriVertex> {
        Ok(MeshTriVertex {
            pos: self.read_vec3()?,
            uv: [self.read_f32()?, self.read_f32()?],
            normal: self.read_vec3()?,
            tangent: self.read_vec3()?,
            binormal: self.read_vec3()?,
            color: self.read_vec4()?,
            bone_indices: [
                self.read_u16()?,
                self.read_u16()?,
                self.read_u16()?,
                self.read_u16()?,
            ],
            bone_weights: [
                self.read_f32()?,
                self.read_f32()?,
                self.read_f32()?,
                self.read_f32()?,
            ],
        })
    }

    fn read_mesh_runtime_material_key(&mut self) -> Result<MeshRuntimeMaterialKey> {
        Ok(MeshRuntimeMaterialKey {
            use_mesh_tex: self.read_bool()?,
            use_shadow_tex: self.read_bool()?,
            use_toon_tex: self.read_bool()?,
            use_normal_tex: self.read_bool()?,
            use_mul_vertex_color: self.read_bool()?,
            use_mrbd: self.read_bool()?,
            use_rgb: self.read_bool()?,
            lighting_type: match self.read_u32()? {
                1 => MeshLightingType::Lambert,
                2 => MeshLightingType::BlinnPhong,
                3 => MeshLightingType::PerPixelBlinnPhong,
                4 => MeshLightingType::PerPixelHalfLambert,
                5 => MeshLightingType::Toon,
                6 => MeshLightingType::FixedFunction,
                7 => MeshLightingType::PerPixelFixedFunction,
                8 => MeshLightingType::Bump,
                9 => MeshLightingType::Parallax,
                _ => MeshLightingType::None,
            },
            shading_type: if self.read_u32()? == 1 {
                MeshShadingType::DepthBuffer
            } else {
                MeshShadingType::None
            },
            shader_option: self.read_u32()?,
            skinned: self.read_bool()?,
            alpha_test_enable: self.read_bool()?,
            cull_disable: self.read_bool()?,
            shadow_map_enable: self.read_bool()?,
        })
    }

    fn read_mesh_runtime_desc(&mut self, version: u32) -> Result<MeshPrimitiveRuntimeDesc> {
        let effect_profile = match self.read_u32()? {
            1 => MeshEffectProfile::Mesh,
            2 => MeshEffectProfile::SkinnedMesh,
            3 => MeshEffectProfile::ShadowMap,
            _ => MeshEffectProfile::None,
        };
        let effect_key = self.read_string()?;
        let technique_name = self.read_string()?;
        let (shadow_effect_key, shadow_technique_name) = if version >= 3 {
            (self.read_string()?, self.read_string()?)
        } else {
            let skinned = effect_profile == MeshEffectProfile::SkinnedMesh
                || effect_key.starts_with("skinned");
            (
                mesh_effect_filename_from_runtime_key(
                    MeshEffectProfile::ShadowMap,
                    MeshRuntimeMaterialKey {
                        skinned,
                        shadow_map_enable: true,
                        ..MeshRuntimeMaterialKey::default()
                    },
                ),
                String::from("tech"),
            )
        };
        Ok(MeshPrimitiveRuntimeDesc {
            effect_profile,
            effect_key,
            technique_name,
            shadow_effect_key,
            shadow_technique_name,
            use_mesh_texture_slot: self.read_bool()?,
            use_normal_texture_slot: self.read_bool()?,
            use_toon_texture_slot: self.read_bool()?,
            use_shadow_texture_slot: self.read_bool()?,
            material_key: self.read_mesh_runtime_material_key()?,
            vertex_stride_bytes: self.read_u32()?,
            vertex_count: self.read_u32()?,
            bone_palette_len: self.read_u32()?,
        })
    }

    fn read_mesh_material(&mut self, base_dir: &Path) -> Result<MeshMaterial> {
        let diffuse = self.read_vec4()?;
        let ambient = self.read_vec4()?;
        let specular = self.read_vec4()?;
        let emissive = self.read_vec4()?;
        let power = self.read_f32()?;
        let lighting_type = match self.read_u32()? {
            1 => MeshLightingType::Lambert,
            2 => MeshLightingType::BlinnPhong,
            3 => MeshLightingType::PerPixelBlinnPhong,
            4 => MeshLightingType::PerPixelHalfLambert,
            5 => MeshLightingType::Toon,
            6 => MeshLightingType::FixedFunction,
            7 => MeshLightingType::PerPixelFixedFunction,
            8 => MeshLightingType::Bump,
            9 => MeshLightingType::Parallax,
            _ => MeshLightingType::None,
        };
        let shading_type = match self.read_u32()? {
            1 => MeshShadingType::DepthBuffer,
            _ => MeshShadingType::None,
        };
        let shader_option = self.read_u32()?;
        let rim_light_color = self.read_vec4()?;
        let rim_light_power = self.read_f32()?;
        let parallax_max_height = self.read_f32()?;
        let alpha_test_enable = self.read_bool()?;
        let alpha_ref = self.read_f32()?;
        let cull_disable = self.read_bool()?;
        let shadow_map_enable = self.read_bool()?;
        let use_mesh_tex = self.read_bool()?;
        let use_mrbd = self.read_bool()?;
        let mrbd = self.read_vec4()?;
        let use_rgb = self.read_bool()?;
        let rgb_rate = self.read_vec4()?;
        let add_rgb = self.read_vec4()?;
        let use_mul_vertex_color = self.read_bool()?;
        let mul_vertex_color_rate = self.read_f32()?;
        let depth_buffer_shadow_bias = self.read_f32()?;
        let directional_count = self.read_u32()? as usize;
        let mut directional_light_ids = Vec::with_capacity(directional_count);
        for _ in 0..directional_count {
            directional_light_ids.push(self.read_i32()?);
        }
        let point_count = self.read_u32()? as usize;
        let mut point_light_ids = Vec::with_capacity(point_count);
        for _ in 0..point_count {
            point_light_ids.push(self.read_i32()?);
        }
        let spot_count = self.read_u32()? as usize;
        let mut spot_light_ids = Vec::with_capacity(spot_count);
        for _ in 0..spot_count {
            spot_light_ids.push(self.read_i32()?);
        }
        let normal_texture_path = self.read_opt_path(base_dir)?;
        let toon_texture_path = self.read_opt_path(base_dir)?;
        let effect_filename = self.read_opt_string()?;
        Ok(MeshMaterial {
            diffuse,
            ambient,
            specular,
            emissive,
            power,
            lighting_type,
            shading_type,
            shader_option,
            rim_light_color,
            rim_light_power,
            parallax_max_height,
            alpha_test_enable,
            alpha_ref,
            cull_disable,
            shadow_map_enable,
            use_mesh_tex,
            use_mrbd,
            mrbd,
            use_rgb,
            rgb_rate,
            add_rgb,
            use_mul_vertex_color,
            mul_vertex_color_rate,
            depth_buffer_shadow_bias,
            directional_light_ids,
            point_light_ids,
            spot_light_ids,
            normal_texture_path,
            toon_texture_path,
            effect_filename,
        })
    }
}
