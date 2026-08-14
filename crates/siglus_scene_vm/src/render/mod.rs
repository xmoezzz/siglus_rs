//! WGPU renderer for Siglus stage composition.
//!
//! This renderer consumes a painter-ordered list of sprites and draws them
//! in order. It supports fixed sprite effects, dual-source wipes, and a
//! depth-backed path for 3D-transformed quads.

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::assets::load_image_any;
use crate::image_manager::{ImageId, ImageManager};
use crate::layer::{
    ClipRect, RenderFrame, RenderSprite, SpriteBlend, SpriteFit, SpriteSizeMode, WipeRenderPlan,
};
use crate::mesh3d::{load_mesh_asset, MeshAsset};
use crate::render_math::sprite_quad_points;
use crate::runtime::FrameCaptureBackend;

use std::ops::RangeFull;

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use fearless_simd::{
    dispatch, Level, Simd, SimdBase, SimdNarrow, SimdSplit,
    SimdWiden, u8x32, u16x16,
};

mod emote;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 3],
    uv: [f32; 2],
    uv_aux: [f32; 2],
    alpha: f32,
    effects1: [f32; 4],
    effects2: [f32; 4],
    effects3: [f32; 4],
    effects4: [f32; 4],
    effects5: [f32; 4],
    effects6: [f32; 4],
    effects7: [f32; 4],
    effects8: [f32; 4],
    effects9: [f32; 4],
    effects10: [f32; 4],
    effects11: [f32; 4],
    world_pos: [f32; 4],
    world_normal: [f32; 4],
    world_tangent: [f32; 4],
    world_binormal: [f32; 4],
    shadow_pos: [f32; 4],
    bone_indices: [f32; 4],
    bone_weights: [f32; 4],
    light_pos_kind: [f32; 4],
    light_dir_shadow: [f32; 4],
    light_atten: [f32; 4],
    light_cone: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct WipeUniform {
    kind_progress: [f32; 4],
    option0: [f32; 4],
    option1: [f32; 4],
    option2: [f32; 4],
    option3: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct PageWipeVertex {
    clip_position: [f32; 4],
    uv: [f32; 2],
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WipeMaskCacheKey {
    wipe_type: i32,
    option: Vec<i32>,
    width: u32,
    height: u32,
    seed: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct VsUniform {
    model_col0: [f32; 4],
    model_col1: [f32; 4],
    model_col2: [f32; 4],
    model_col3: [f32; 4],
    normal_col0: [f32; 4],
    normal_col1: [f32; 4],
    normal_col2: [f32; 4],
    frame_col0: [f32; 4],
    frame_col1: [f32; 4],
    frame_col2: [f32; 4],
    frame_col3: [f32; 4],
    frame_normal0: [f32; 4],
    frame_normal1: [f32; 4],
    frame_normal2: [f32; 4],
    camera_eye: [f32; 4],
    camera_forward: [f32; 4],
    camera_right: [f32; 4],
    camera_up: [f32; 4],
    camera_params: [f32; 4],
    shadow_eye: [f32; 4],
    shadow_forward: [f32; 4],
    shadow_right: [f32; 4],
    shadow_up: [f32; 4],
    shadow_params: [f32; 4],
    mtrl_diffuse: [f32; 4],
    mtrl_ambient: [f32; 4],
    mtrl_specular: [f32; 4],
    mtrl_emissive: [f32; 4],
    mtrl_params: [f32; 4],
    mtrl_rim: [f32; 4],
    mtrl_extra: [f32; 4],
    light_diffuse_u: [f32; 4],
    light_ambient_u: [f32; 4],
    light_specular_u: [f32; 4],
    /// Per-draw CFX values. Mesh pipelines read these from the uniform
    /// instead of exceeding WebGPU's 16 vertex-attribute limit.
    sprite_effects: [[f32; 4]; 11],
    single_light_pos_kind: [f32; 4],
    single_light_dir_shadow: [f32; 4],
    single_light_atten: [f32; 4],
    single_light_cone: [f32; 4],
    mesh_flags: [f32; 4],
    mesh_mrbd: [f32; 4],
    mesh_rgb_rate: [f32; 4],
    mesh_add_rgb: [f32; 4],
    mesh_misc: [f32; 4],
    mesh_light_counts: [f32; 4],
    dir_light_diffuse: [[f32; 4]; MAX_BATCH_LIGHTS],
    dir_light_ambient: [[f32; 4]; MAX_BATCH_LIGHTS],
    dir_light_specular: [[f32; 4]; MAX_BATCH_LIGHTS],
    dir_light_dir: [[f32; 4]; MAX_BATCH_LIGHTS],
    point_light_diffuse: [[f32; 4]; MAX_BATCH_LIGHTS],
    point_light_ambient: [[f32; 4]; MAX_BATCH_LIGHTS],
    point_light_specular: [[f32; 4]; MAX_BATCH_LIGHTS],
    point_light_pos: [[f32; 4]; MAX_BATCH_LIGHTS],
    point_light_atten: [[f32; 4]; MAX_BATCH_LIGHTS],
    spot_light_diffuse: [[f32; 4]; MAX_BATCH_LIGHTS],
    spot_light_ambient: [[f32; 4]; MAX_BATCH_LIGHTS],
    spot_light_specular: [[f32; 4]; MAX_BATCH_LIGHTS],
    spot_light_pos: [[f32; 4]; MAX_BATCH_LIGHTS],
    spot_light_dir: [[f32; 4]; MAX_BATCH_LIGHTS],
    spot_light_atten: [[f32; 4]; MAX_BATCH_LIGHTS],
    spot_light_cone: [[f32; 4]; MAX_BATCH_LIGHTS],
    flags: [f32; 4],
}

impl VsUniform {
    fn for_2d(win_w: f32, win_h: f32) -> Self {
        Self {
            model_col0: [1.0, 0.0, 0.0, 0.0],
            model_col1: [0.0, 1.0, 0.0, 0.0],
            model_col2: [0.0, 0.0, 1.0, 0.0],
            model_col3: [0.0, 0.0, 0.0, 1.0],
            normal_col0: [1.0, 0.0, 0.0, 0.0],
            normal_col1: [0.0, 1.0, 0.0, 0.0],
            normal_col2: [0.0, 0.0, 1.0, 0.0],
            frame_col0: [1.0, 0.0, 0.0, 0.0],
            frame_col1: [0.0, 1.0, 0.0, 0.0],
            frame_col2: [0.0, 0.0, 1.0, 0.0],
            frame_col3: [0.0, 0.0, 0.0, 1.0],
            frame_normal0: [1.0, 0.0, 0.0, 0.0],
            frame_normal1: [0.0, 1.0, 0.0, 0.0],
            frame_normal2: [0.0, 0.0, 1.0, 0.0],
            camera_eye: [0.0, 0.0, 0.0, 0.0],
            camera_forward: [0.0, 0.0, 1.0, 0.0],
            camera_right: [1.0, 0.0, 0.0, 0.0],
            camera_up: [0.0, 1.0, 0.0, 0.0],
            camera_params: [0.0, 0.0, win_w, win_h],
            shadow_eye: [0.0, 0.0, 0.0, 0.0],
            shadow_forward: [0.0, 0.0, 1.0, 0.0],
            shadow_right: [1.0, 0.0, 0.0, 0.0],
            shadow_up: [0.0, 1.0, 0.0, 0.0],
            shadow_params: [1.0, 1.0, 0.0, 0.0],
            mtrl_diffuse: [1.0, 1.0, 1.0, 1.0],
            mtrl_ambient: [1.0, 1.0, 1.0, 1.0],
            mtrl_specular: [0.0, 0.0, 0.0, 1.0],
            mtrl_emissive: [0.0, 0.0, 0.0, 1.0],
            mtrl_params: [16.0, 0.0, 0.0, 0.0],
            mtrl_rim: [1.0, 1.0, 1.0, 1.0],
            mtrl_extra: [0.016, 0.001, 0.0, 0.0],
            light_diffuse_u: [1.0, 1.0, 1.0, 1.0],
            light_ambient_u: [0.0, 0.0, 0.0, 1.0],
            light_specular_u: [0.0, 0.0, 0.0, 1.0],
            sprite_effects: [[0.0; 4]; 11],
            single_light_pos_kind: [0.0, 0.0, 0.0, -1.0],
            single_light_dir_shadow: [0.0, 0.0, -1.0, 0.0],
            single_light_atten: [1.0, 0.0, 0.0, 5000.0],
            single_light_cone: [0.0, 0.0, 1.0, 0.0],
            mesh_flags: [1.0, 0.0, 0.0, 0.0],
            mesh_mrbd: [0.0, 0.0, 0.0, 0.0],
            mesh_rgb_rate: [0.0, 0.0, 0.0, 0.0],
            mesh_add_rgb: [0.0, 0.0, 0.0, 0.0],
            mesh_misc: [1.0, 0.03, 0.0, 0.0],
            mesh_light_counts: [0.0, 0.0, 0.0, 0.0],
            dir_light_diffuse: [[0.0; 4]; MAX_BATCH_LIGHTS],
            dir_light_ambient: [[0.0; 4]; MAX_BATCH_LIGHTS],
            dir_light_specular: [[0.0; 4]; MAX_BATCH_LIGHTS],
            dir_light_dir: [[0.0; 4]; MAX_BATCH_LIGHTS],
            point_light_diffuse: [[0.0; 4]; MAX_BATCH_LIGHTS],
            point_light_ambient: [[0.0; 4]; MAX_BATCH_LIGHTS],
            point_light_specular: [[0.0; 4]; MAX_BATCH_LIGHTS],
            point_light_pos: [[0.0; 4]; MAX_BATCH_LIGHTS],
            point_light_atten: [[0.0; 4]; MAX_BATCH_LIGHTS],
            spot_light_diffuse: [[0.0; 4]; MAX_BATCH_LIGHTS],
            spot_light_ambient: [[0.0; 4]; MAX_BATCH_LIGHTS],
            spot_light_specular: [[0.0; 4]; MAX_BATCH_LIGHTS],
            spot_light_pos: [[0.0; 4]; MAX_BATCH_LIGHTS],
            spot_light_dir: [[0.0; 4]; MAX_BATCH_LIGHTS],
            spot_light_atten: [[0.0; 4]; MAX_BATCH_LIGHTS],
            spot_light_cone: [[0.0; 4]; MAX_BATCH_LIGHTS],
            flags: [0.0, 0.0, 0.0, 0.0],
        }
    }
}

fn set_sprite2d_effect_uniforms(
    u: &mut VsUniform,
    effects1: [f32; 4],
    effects2: [f32; 4],
    effects3: [f32; 4],
    effects4: [f32; 4],
    effects5: [f32; 4],
    effects6: [f32; 4],
    effects7: [f32; 4],
    effects8: [f32; 4],
    effects9: [f32; 4],
    effects10: [f32; 4],
    effects11: [f32; 4],
) {
    u.sprite_effects = [
        effects1, effects2, effects3, effects4, effects5, effects6, effects7, effects8, effects9,
        effects10, effects11,
    ];
}

fn sprite2d_uniform_for_effects(
    win_w: f32,
    win_h: f32,
    effects1: [f32; 4],
    effects2: [f32; 4],
    effects3: [f32; 4],
    effects4: [f32; 4],
    effects5: [f32; 4],
    effects6: [f32; 4],
    effects7: [f32; 4],
    effects8: [f32; 4],
    effects9: [f32; 4],
    effects10: [f32; 4],
    effects11: [f32; 4],
) -> VsUniform {
    let mut u = VsUniform::for_2d(win_w, win_h);
    set_sprite2d_effect_uniforms(
        &mut u, effects1, effects2, effects3, effects4, effects5, effects6, effects7, effects8,
        effects9, effects10, effects11,
    );
    u
}

fn plain_sprite2d_uniform(win_w: f32, win_h: f32) -> VsUniform {
    sprite2d_uniform_for_effects(
        win_w,
        win_h,
        [1.0, 0.0, 0.0, 0.0],
        [0.0; 4],
        [0.0; 4],
        [0.0; 4],
        [0.0; 4],
        [0.0; 4],
        [0.0; 4],
        [0.0; 4],
        [0.0; 4],
        [0.0; 4],
        [0.0; 4],
    )
}

const MAX_BONES: usize = crate::mesh3d::MAX_GPU_BONE_PALETTE;
const MAX_BATCH_LIGHTS: usize = 4;

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct BoneUniform {
    matrices: [[[f32; 4]; 4]; MAX_BONES],
}

impl BoneUniform {
    fn zero() -> Self {
        Self {
            matrices: [[[0.0; 4]; 4]; MAX_BONES],
        }
    }

    fn from_cols_list(cols: &[[[f32; 4]; 4]]) -> Self {
        let mut out = Self::zero();
        for (dst, src) in out.matrices.iter_mut().zip(cols.iter()) {
            *dst = *src;
        }
        out
    }
}

impl Vertex {
    // The backing buffer keeps the complete CPU-side Vertex structure, but
    // mesh shaders consume only ten attributes. The previous 26-attribute
    // declaration exceeded WebGPU's guaranteed MAX_VERTEX_ATTRIBUTES=16.
    const ATTRS: [wgpu::VertexAttribute; 10] = [
        wgpu::VertexAttribute {
            offset: 0,
            shader_location: 0,
            format: wgpu::VertexFormat::Float32x3,
        },
        wgpu::VertexAttribute {
            offset: 12,
            shader_location: 1,
            format: wgpu::VertexFormat::Float32x2,
        },
        wgpu::VertexAttribute {
            offset: 28,
            shader_location: 2,
            format: wgpu::VertexFormat::Float32,
        },
        wgpu::VertexAttribute {
            offset: 144,
            shader_location: 3,
            format: wgpu::VertexFormat::Float32x4,
        },
        wgpu::VertexAttribute {
            offset: 160,
            shader_location: 4,
            format: wgpu::VertexFormat::Float32x4,
        },
        wgpu::VertexAttribute {
            offset: 224,
            shader_location: 5,
            format: wgpu::VertexFormat::Float32x4,
        },
        wgpu::VertexAttribute {
            offset: 240,
            shader_location: 6,
            format: wgpu::VertexFormat::Float32x4,
        },
        wgpu::VertexAttribute {
            offset: 256,
            shader_location: 7,
            format: wgpu::VertexFormat::Float32x4,
        },
        wgpu::VertexAttribute {
            offset: 288,
            shader_location: 8,
            format: wgpu::VertexFormat::Float32x4,
        },
        wgpu::VertexAttribute {
            offset: 304,
            shader_location: 9,
            format: wgpu::VertexFormat::Float32x4,
        },
    ];

    fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct VertexSprite2dData {
    pos: [f32; 3],
    uv: [f32; 2],
    uv_aux: [f32; 2],
    alpha: f32,
    effects1: [f32; 4],
    effects2: [f32; 4],
    effects3: [f32; 4],
    effects4: [f32; 4],
    effects5: [f32; 4],
    effects6: [f32; 4],
    effects7: [f32; 4],
    effects8: [f32; 4],
    effects9: [f32; 4],
    effects10: [f32; 4],
    effects11: [f32; 4],
}

impl From<Vertex> for VertexSprite2dData {
    fn from(v: Vertex) -> Self {
        Self {
            pos: v.pos,
            uv: v.uv,
            uv_aux: v.uv_aux,
            alpha: v.alpha,
            effects1: v.effects1,
            effects2: v.effects2,
            effects3: v.effects3,
            effects4: v.effects4,
            effects5: v.effects5,
            effects6: v.effects6,
            effects7: v.effects7,
            effects8: v.effects8,
            effects9: v.effects9,
            effects10: v.effects10,
            effects11: v.effects11,
        }
    }
}

struct VertexSprite2d;

impl VertexSprite2d {
    const ATTRS: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x2,
        2 => Float32x2,
        3 => Float32
    ];

    fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        let array_stride = std::mem::size_of::<VertexSprite2dData>() as wgpu::BufferAddress;
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        let array_stride = std::mem::size_of::<Vertex>() as wgpu::BufferAddress;

        wgpu::VertexBufferLayout {
            array_stride,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TechniqueSpecial {
    None,
    Overlay,
    WipeMosaic,
    WipeRasterH,
    WipeRasterV,
    WipeExplosionBlur,
    WipeShimi,
    WipeShimiInv,
    WipeCrossMosaic,
    WipeCrossRasterH,
    WipeCrossRasterV,
    WipeCrossExplosionBlur,
    Mesh,
    SkinnedMesh,
    Shadow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum EffectProgram {
    Sprite2D,
    OverlayGpu,
    WipeMosaic,
    WipeRasterH,
    WipeRasterV,
    WipeExplosionBlur,
    WipeShimi,
    WipeShimiInv,
    WipeCrossMosaic,
    WipeCrossRasterH,
    WipeCrossRasterV,
    WipeCrossExplosionBlur,
    MeshStaticUnlit,
    MeshStaticLambert,
    MeshStaticBlinnPhong,
    MeshStaticPerPixelBlinnPhong,
    MeshStaticPerPixelHalfLambert,
    MeshStaticToon,
    MeshStaticFixedFunction,
    MeshStaticPerPixelFixedFunction,
    MeshStaticBump,
    MeshStaticParallax,
    MeshSkinnedUnlit,
    MeshSkinnedLambert,
    MeshSkinnedBlinnPhong,
    MeshSkinnedPerPixelBlinnPhong,
    MeshSkinnedPerPixelHalfLambert,
    MeshSkinnedToon,
    MeshSkinnedFixedFunction,
    MeshSkinnedPerPixelFixedFunction,
    MeshSkinnedBump,
    MeshSkinnedParallax,
    ShadowStatic,
    ShadowSkinned,
}

impl EffectProgram {
    fn uses_sprite2d_layout(self) -> bool {
        matches!(
            self,
            EffectProgram::Sprite2D
                | EffectProgram::OverlayGpu
                | EffectProgram::WipeMosaic
                | EffectProgram::WipeRasterH
                | EffectProgram::WipeRasterV
                | EffectProgram::WipeExplosionBlur
                | EffectProgram::WipeShimi
                | EffectProgram::WipeShimiInv
                | EffectProgram::WipeCrossMosaic
                | EffectProgram::WipeCrossRasterH
                | EffectProgram::WipeCrossRasterV
                | EffectProgram::WipeCrossExplosionBlur
        )
    }

    fn vertex_entry(self) -> &'static str {
        match self {
            EffectProgram::Sprite2D
            | EffectProgram::OverlayGpu
            | EffectProgram::WipeMosaic
            | EffectProgram::WipeRasterH
            | EffectProgram::WipeRasterV
            | EffectProgram::WipeExplosionBlur
            | EffectProgram::WipeShimi
            | EffectProgram::WipeShimiInv
            | EffectProgram::WipeCrossMosaic
            | EffectProgram::WipeCrossRasterH
            | EffectProgram::WipeCrossRasterV
            | EffectProgram::WipeCrossExplosionBlur => "vs_sprite_2d",
            EffectProgram::MeshStaticUnlit
            | EffectProgram::MeshStaticLambert
            | EffectProgram::MeshStaticBlinnPhong
            | EffectProgram::MeshStaticPerPixelBlinnPhong
            | EffectProgram::MeshStaticPerPixelHalfLambert
            | EffectProgram::MeshStaticToon
            | EffectProgram::MeshStaticFixedFunction
            | EffectProgram::MeshStaticPerPixelFixedFunction
            | EffectProgram::MeshStaticBump
            | EffectProgram::MeshStaticParallax => "vs_mesh_static",
            EffectProgram::MeshSkinnedUnlit
            | EffectProgram::MeshSkinnedLambert
            | EffectProgram::MeshSkinnedBlinnPhong
            | EffectProgram::MeshSkinnedPerPixelBlinnPhong
            | EffectProgram::MeshSkinnedPerPixelHalfLambert
            | EffectProgram::MeshSkinnedToon
            | EffectProgram::MeshSkinnedFixedFunction
            | EffectProgram::MeshSkinnedPerPixelFixedFunction
            | EffectProgram::MeshSkinnedBump
            | EffectProgram::MeshSkinnedParallax => "vs_mesh_skinned",
            EffectProgram::ShadowStatic => "vs_shadow_static",
            EffectProgram::ShadowSkinned => "vs_shadow_skinned",
        }
    }

    fn fragment_entry(self) -> &'static str {
        match self {
            EffectProgram::Sprite2D => "fs_sprite_2d",
            EffectProgram::OverlayGpu => "fs_overlay_gpu",
            EffectProgram::WipeMosaic => "fs_wipe_mosaic",
            EffectProgram::WipeRasterH => "fs_wipe_raster_h",
            EffectProgram::WipeRasterV => "fs_wipe_raster_v",
            EffectProgram::WipeExplosionBlur => "fs_wipe_explosion_blur",
            EffectProgram::WipeShimi => "fs_wipe_shimi",
            EffectProgram::WipeShimiInv => "fs_wipe_shimi_inv",
            EffectProgram::WipeCrossMosaic => "fs_wipe_cross_mosaic",
            EffectProgram::WipeCrossRasterH => "fs_wipe_cross_raster_h",
            EffectProgram::WipeCrossRasterV => "fs_wipe_cross_raster_v",
            EffectProgram::WipeCrossExplosionBlur => "fs_wipe_cross_explosion_blur",
            EffectProgram::MeshStaticUnlit | EffectProgram::MeshSkinnedUnlit => "fs_mesh_unlit",
            EffectProgram::MeshStaticLambert | EffectProgram::MeshSkinnedLambert => {
                "fs_mesh_lambert"
            }
            EffectProgram::MeshStaticBlinnPhong | EffectProgram::MeshSkinnedBlinnPhong => {
                "fs_mesh_blinn_phong"
            }
            EffectProgram::MeshStaticPerPixelBlinnPhong
            | EffectProgram::MeshSkinnedPerPixelBlinnPhong => "fs_mesh_pp_blinn_phong",
            EffectProgram::MeshStaticPerPixelHalfLambert
            | EffectProgram::MeshSkinnedPerPixelHalfLambert => "fs_mesh_pp_half_lambert",
            EffectProgram::MeshStaticToon | EffectProgram::MeshSkinnedToon => "fs_mesh_toon",
            EffectProgram::MeshStaticFixedFunction | EffectProgram::MeshSkinnedFixedFunction => {
                "fs_mesh_ffp"
            }
            EffectProgram::MeshStaticPerPixelFixedFunction
            | EffectProgram::MeshSkinnedPerPixelFixedFunction => "fs_mesh_pp_ffp",
            EffectProgram::MeshStaticBump | EffectProgram::MeshSkinnedBump => "fs_mesh_bump",
            EffectProgram::MeshStaticParallax | EffectProgram::MeshSkinnedParallax => {
                "fs_mesh_parallax"
            }
            EffectProgram::ShadowStatic | EffectProgram::ShadowSkinned => "fs_shadow_map",
        }
    }

    fn short_name(self) -> &'static str {
        match self {
            EffectProgram::Sprite2D => "sprite2d",
            EffectProgram::OverlayGpu => "overlay_gpu",
            EffectProgram::WipeMosaic => "wipe_mosaic",
            EffectProgram::WipeRasterH => "wipe_raster_h",
            EffectProgram::WipeRasterV => "wipe_raster_v",
            EffectProgram::WipeExplosionBlur => "wipe_explosion_blur",
            EffectProgram::WipeShimi => "wipe_shimi",
            EffectProgram::WipeShimiInv => "wipe_shimi_inv",
            EffectProgram::WipeCrossMosaic => "wipe_cross_mosaic",
            EffectProgram::WipeCrossRasterH => "wipe_cross_raster_h",
            EffectProgram::WipeCrossRasterV => "wipe_cross_raster_v",
            EffectProgram::WipeCrossExplosionBlur => "wipe_cross_explosion_blur",
            EffectProgram::MeshStaticUnlit => "mesh_static_unlit",
            EffectProgram::MeshStaticLambert => "mesh_static_lambert",
            EffectProgram::MeshStaticBlinnPhong => "mesh_static_blinn_phong",
            EffectProgram::MeshStaticPerPixelBlinnPhong => "mesh_static_pp_blinn_phong",
            EffectProgram::MeshStaticPerPixelHalfLambert => "mesh_static_pp_half_lambert",
            EffectProgram::MeshStaticToon => "mesh_static_toon",
            EffectProgram::MeshStaticFixedFunction => "mesh_static_ffp",
            EffectProgram::MeshStaticPerPixelFixedFunction => "mesh_static_pp_ffp",
            EffectProgram::MeshStaticBump => "mesh_static_bump",
            EffectProgram::MeshStaticParallax => "mesh_static_parallax",
            EffectProgram::MeshSkinnedUnlit => "mesh_skinned_unlit",
            EffectProgram::MeshSkinnedLambert => "mesh_skinned_lambert",
            EffectProgram::MeshSkinnedBlinnPhong => "mesh_skinned_blinn_phong",
            EffectProgram::MeshSkinnedPerPixelBlinnPhong => "mesh_skinned_pp_blinn_phong",
            EffectProgram::MeshSkinnedPerPixelHalfLambert => "mesh_skinned_pp_half_lambert",
            EffectProgram::MeshSkinnedToon => "mesh_skinned_toon",
            EffectProgram::MeshSkinnedFixedFunction => "mesh_skinned_ffp",
            EffectProgram::MeshSkinnedPerPixelFixedFunction => "mesh_skinned_pp_ffp",
            EffectProgram::MeshSkinnedBump => "mesh_skinned_bump",
            EffectProgram::MeshSkinnedParallax => "mesh_skinned_parallax",
            EffectProgram::ShadowStatic => "shadow_static",
            EffectProgram::ShadowSkinned => "shadow_skinned",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct TechniqueKey {
    d3: bool,
    light: bool,
    fog: bool,
    tex: u8,
    diffuse: bool,
    mrbd: bool,
    rgb: bool,
    tonecurve: bool,
    mask: bool,
    special: TechniqueSpecial,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct PipelineKey {
    technique: TechniqueKey,
    blend: SpriteBlend,
    alpha_blend: bool,
    use_depth: bool,
    depth_attachment: bool,
    cull_back: bool,
    mesh_fx_variant: u64,
    pipeline_name: String,
    program: EffectProgram,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MeshDrawKind {
    SpriteQuad,
    StaticMesh,
    SkinnedMesh,
    ShadowCaster,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MeshMaterialKey {
    pub lighting: bool,
    pub fog: bool,
    pub shadow: bool,
    pub use_mesh_tex: bool,
    pub use_mrbd: bool,
    pub use_rgb: bool,
    pub use_normal_tex: bool,
    pub use_toon_tex: bool,
    pub skinned: bool,
}

#[derive(Debug, Clone)]
pub struct SkinnedPoseState {
    pub world_matrix_count: usize,
}

#[derive(Debug)]
pub struct Renderer {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    logical_width: f32,
    logical_height: f32,
    scale_factor: f32,
    surface_viewport: SurfaceViewport,

    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    bind_group_layout: wgpu::BindGroupLayout,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    wipe_bind_group_layout: wgpu::BindGroupLayout,
    wipe_pipeline: wgpu::RenderPipeline,
    page_wipe_bind_group_layout: wgpu::BindGroupLayout,
    page_wipe_pipeline: wgpu::RenderPipeline,

    vertex_buf: wgpu::Buffer,
    vertex_capacity: usize,
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    vertex_sprite2d_buf: wgpu::Buffer,
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    vertex_sprite2d_capacity: usize,

    textures: HashMap<ImageId, GpuTexture>,
    external_textures: HashMap<PathBuf, GpuTexture>,
    mesh_assets: HashMap<String, MeshAsset>,
    default_aux: GpuTexture,
    /// shader.cfx declares the fog sampler with WRAP addressing.  Keep it
    /// separate from the image-owned CLAMP sampler used by normal sprites.
    fog_sampler: wgpu::Sampler,
    /// tona3 samplers used by dynamic mesh/shadow effects.
    mesh_sampler: wgpu::Sampler,
    normal_sampler: wgpu::Sampler,
    toon_sampler: wgpu::Sampler,
    shadow_sampler: wgpu::Sampler,
    depth: DepthTexture,
    surface_depth: DepthTexture,
    scene_a: RenderTargetTexture,
    scene_b: RenderTargetTexture,
    wipe_a: RenderTargetTexture,
    wipe_b: RenderTargetTexture,
    wipe_mask_cache: Option<(WipeMaskCacheKey, GpuTexture)>,
    shadow_map: RenderTargetTexture,
    shadow_depth: DepthTexture,

    verts: Vec<Vertex>,
    draws: Vec<DrawCommand>,
    draw_gpu_slots: Vec<DrawGpuSlot>,
    draw_bind_epoch: u64,
    debug_frame_serial: u64,
    emote_compositor: emote::EmoteCompositor,
}

#[derive(Debug, Clone, Copy)]
struct SurfaceViewport {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

impl SurfaceViewport {
    fn full(width: u32, height: u32) -> Self {
        Self {
            x: 0,
            y: 0,
            w: width.max(1),
            h: height.max(1),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RendererDebugTexture {
    pub key: String,
    pub kind: String,
    pub label: String,
    pub usage: String,
    pub usage_count: usize,
    pub width: u32,
    pub height: u32,
    pub version: u64,
    pub rgba: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RendererDebugRenderTarget {
    SceneA,
    SceneB,
    ShadowMap,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RendererDebugTextureKey {
    DefaultAux,
    Image(ImageId),
    External(PathBuf),
    RenderTarget(RendererDebugRenderTarget),
}

#[derive(Debug, Clone)]
struct PendingRendererDebugTexture {
    order: usize,
    kind: String,
    label: String,
    usage: Vec<String>,
    width: u32,
    height: u32,
    version: u64,
}

#[derive(Debug)]
struct DepthTexture {
    _tex: wgpu::Texture,
    view: wgpu::TextureView,
}

#[derive(Debug)]
struct GpuTexture {
    _tex: wgpu::Texture,
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    width: u32,
    height: u32,
    version: u64,
}

#[derive(Debug)]
struct RenderTargetTexture {
    _tex: wgpu::Texture,
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
}

#[derive(Debug, Clone, Copy)]
enum InternalColorTarget {
    SceneA,
    SceneB,
    WipeA,
    WipeB,
    ShadowMap,
}

#[derive(Debug, Clone, Copy)]
enum DepthTarget {
    None,
    Main,
    Surface,
    Shadow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BackdropTarget {
    SceneA,
    SceneB,
}

fn backdrop_to_internal(target: BackdropTarget) -> InternalColorTarget {
    match target {
        BackdropTarget::SceneA => InternalColorTarget::SceneA,
        BackdropTarget::SceneB => InternalColorTarget::SceneB,
    }
}

fn opposite_backdrop(target: BackdropTarget) -> BackdropTarget {
    match target {
        BackdropTarget::SceneA => BackdropTarget::SceneB,
        BackdropTarget::SceneB => BackdropTarget::SceneA,
    }
}

#[derive(Debug, Clone, Copy)]
enum ColorTarget<'a> {
    External(&'a wgpu::TextureView),
    Internal(InternalColorTarget),
}

#[derive(Debug, Clone)]
struct DrawCommand {
    image_id: Option<ImageId>,
    emote_render_id: Option<u64>,
    mesh_texture_path: Option<PathBuf>,
    mesh_normal_texture_path: Option<PathBuf>,
    mesh_toon_texture_path: Option<PathBuf>,
    mask_image_id: Option<ImageId>,
    tonecurve_image_id: Option<ImageId>,
    fog_image_id: Option<ImageId>,
    wipe_src_image_id: Option<ImageId>,
    range: std::ops::Range<u32>,
    scissor: Option<ScissorRect>,
    pipeline_key: PipelineKey,
    shadow_pipeline_name: Option<String>,
    draw_kind: MeshDrawKind,
    mesh_material_key: Option<MeshMaterialKey>,
    shadow_cast: bool,
    vs_uniform: VsUniform,
    bone_uniform: BoneUniform,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DrawBindKey {
    image_id: Option<ImageId>,
    emote_render_id: Option<u64>,
    mesh_texture_path: Option<PathBuf>,
    mesh_normal_texture_path: Option<PathBuf>,
    mesh_toon_texture_path: Option<PathBuf>,
    mask_image_id: Option<ImageId>,
    tonecurve_image_id: Option<ImageId>,
    fog_image_id: Option<ImageId>,
    wipe_src_image_id: Option<ImageId>,
    overlay_backdrop: Option<BackdropTarget>,
    mesh_base_sampler: bool,
}

impl DrawBindKey {
    fn from_command(cmd: &DrawCommand, overlay_backdrop: Option<BackdropTarget>) -> Self {
        Self {
            image_id: cmd.image_id,
            emote_render_id: cmd.emote_render_id,
            mesh_texture_path: cmd.mesh_texture_path.clone(),
            mesh_normal_texture_path: cmd.mesh_normal_texture_path.clone(),
            mesh_toon_texture_path: cmd.mesh_toon_texture_path.clone(),
            mask_image_id: cmd.mask_image_id,
            tonecurve_image_id: cmd.tonecurve_image_id,
            fog_image_id: cmd.fog_image_id,
            wipe_src_image_id: cmd.wipe_src_image_id,
            overlay_backdrop: if matches!(
                cmd.pipeline_key.technique.special,
                TechniqueSpecial::Overlay
            ) {
                overlay_backdrop
            } else {
                None
            },
            mesh_base_sampler: matches!(
                cmd.draw_kind,
                MeshDrawKind::StaticMesh | MeshDrawKind::SkinnedMesh | MeshDrawKind::ShadowCaster
            ),
        }
    }
}

#[derive(Debug)]
struct DrawGpuSlot {
    vs_uniform_buf: wgpu::Buffer,
    bone_uniform_buf: wgpu::Buffer,
    bind_group: Option<wgpu::BindGroup>,
    bind_key: Option<DrawBindKey>,
    bind_epoch: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct EffectGlobalValPackSemantic {
    use_bone_uniform: bool,
    use_shadow_tex: bool,
    use_normal_tex: bool,
    use_toon_tex: bool,
}

#[derive(Debug)]
struct EffectResolvedResources<'a> {
    base: &'a GpuTexture,
    mask: &'a GpuTexture,
    tone: &'a GpuTexture,
    fog: &'a GpuTexture,
    normal: &'a GpuTexture,
    toon: &'a GpuTexture,
    aux_view: &'a wgpu::TextureView,
    aux_sampler: &'a wgpu::Sampler,
    shadow_view: &'a wgpu::TextureView,
    shadow_sampler: &'a wgpu::Sampler,
    global_vals: EffectGlobalValPackSemantic,
}

#[derive(Debug, Copy, Clone)]
struct ScissorRect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

fn uses_depth_pipeline(sprite: &crate::layer::Sprite) -> bool {
    sprite.camera_enabled
        || sprite.billboard
        || sprite.z.abs() > f32::EPSILON
        || sprite.pivot_z.abs() > f32::EPSILON
        || (sprite.scale_z - 1.0).abs() > 1e-6
        || sprite.rotate_x.abs() > f32::EPSILON
        || sprite.rotate_y.abs() > f32::EPSILON
}

fn pipeline_cull_back(sprite: &crate::layer::Sprite, material_cull_disable: bool) -> bool {
    uses_depth_pipeline(sprite) && sprite.culling && !material_cull_disable
}

fn sprite_has_mrbd(sprite: &crate::layer::Sprite) -> bool {
    sprite.mono != 0 || sprite.reverse != 0 || sprite.bright != 0 || sprite.dark != 0
}

fn sprite_has_rgb(sprite: &crate::layer::Sprite) -> bool {
    sprite.color_rate != 0
        || sprite.color_add_r != 0
        || sprite.color_add_g != 0
        || sprite.color_add_b != 0
        || sprite.color_r != 0
        || sprite.color_g != 0
        || sprite.color_b != 0
}

fn sprite_has_diffuse(sprite: &crate::layer::Sprite) -> bool {
    uses_depth_pipeline(sprite) || sprite.tr != 255 || sprite.alpha != 255
}

fn is_mesh_special(special: TechniqueSpecial) -> bool {
    matches!(
        special,
        TechniqueSpecial::Mesh | TechniqueSpecial::SkinnedMesh | TechniqueSpecial::Shadow
    )
}

fn is_wipe_special(special: TechniqueSpecial) -> bool {
    matches!(
        special,
        TechniqueSpecial::WipeMosaic
            | TechniqueSpecial::WipeRasterH
            | TechniqueSpecial::WipeRasterV
            | TechniqueSpecial::WipeExplosionBlur
            | TechniqueSpecial::WipeShimi
            | TechniqueSpecial::WipeShimiInv
            | TechniqueSpecial::WipeCrossMosaic
            | TechniqueSpecial::WipeCrossRasterH
            | TechniqueSpecial::WipeCrossRasterV
            | TechniqueSpecial::WipeCrossExplosionBlur
    )
}

fn wipe_special_for_sprite(
    sprite: &crate::layer::Sprite,
    has_wipe_src: bool,
) -> Option<TechniqueSpecial> {
    match (sprite.wipe_fx_mode, has_wipe_src) {
        (1, _) => Some(TechniqueSpecial::WipeMosaic),
        (2, _) => Some(TechniqueSpecial::WipeRasterH),
        (3, _) => Some(TechniqueSpecial::WipeRasterV),
        (4, _) => Some(TechniqueSpecial::WipeExplosionBlur),
        (5, _) => Some(TechniqueSpecial::WipeShimi),
        (6, _) => Some(TechniqueSpecial::WipeShimiInv),
        (10, true) => Some(TechniqueSpecial::WipeCrossMosaic),
        (11, true) => Some(TechniqueSpecial::WipeCrossRasterH),
        (12, true) => Some(TechniqueSpecial::WipeCrossRasterV),
        (13, true) => Some(TechniqueSpecial::WipeCrossExplosionBlur),
        _ => None,
    }
}

fn sprite_has_emote_texture(sprite: &crate::layer::Sprite) -> bool {
    sprite.emote_render.is_some()
}

fn build_technique_key(
    sprite: &crate::layer::Sprite,
    has_mask: bool,
    has_tonecurve: bool,
    has_wipe_src: bool,
    special_override: Option<TechniqueSpecial>,
) -> TechniqueKey {
    let d3 = uses_depth_pipeline(sprite);
    let special = if let Some(s) = special_override {
        s
    } else if matches!(sprite.blend, SpriteBlend::Overlay) {
        TechniqueSpecial::Overlay
    } else if let Some(wipe) = wipe_special_for_sprite(sprite, has_wipe_src) {
        wipe
    } else if sprite.mesh_kind == 3 {
        TechniqueSpecial::SkinnedMesh
    } else if sprite.mesh_kind == 1 || sprite.mesh_kind == 2 {
        TechniqueSpecial::Mesh
    } else {
        TechniqueSpecial::None
    };
    let light = d3 && sprite.light_enabled && !has_mask;
    let fog = d3 && sprite.fog_enabled && !has_mask;
    TechniqueKey {
        d3,
        light,
        fog,
        tex: u8::from(sprite.image_id.is_some() || sprite_has_emote_texture(sprite)),
        diffuse: sprite_has_diffuse(sprite),
        mrbd: sprite_has_mrbd(sprite),
        rgb: sprite_has_rgb(sprite),
        tonecurve: has_tonecurve,
        mask: has_mask,
        special,
    }
}

fn mesh_material_key_for_sprite(
    sprite: &crate::layer::Sprite,
    special: TechniqueSpecial,
) -> Option<MeshMaterialKey> {
    if !is_mesh_special(special) {
        return None;
    }
    Some(MeshMaterialKey {
        lighting: sprite.light_enabled,
        fog: sprite.fog_enabled,
        shadow: sprite.shadow_receive,
        use_mesh_tex: sprite.image_id.is_some() || is_mesh_special(special),
        use_mrbd: sprite_has_mrbd(sprite),
        use_rgb: sprite_has_rgb(sprite),
        use_normal_tex: false,
        use_toon_tex: false,
        skinned: matches!(special, TechniqueSpecial::SkinnedMesh),
    })
}

fn mesh_material_key_for_batch(
    sprite: &crate::layer::Sprite,
    special: TechniqueSpecial,
    batch: &crate::mesh3d::MeshGpuPrimitiveBatch,
) -> Option<MeshMaterialKey> {
    if !is_mesh_special(special) {
        return None;
    }
    Some(MeshMaterialKey {
        lighting: sprite.light_enabled,
        fog: sprite.fog_enabled,
        shadow: sprite.shadow_receive,
        use_mesh_tex: batch.runtime_desc.material_key.use_mesh_tex,
        use_mrbd: sprite_has_mrbd(sprite) || batch.runtime_desc.material_key.use_mrbd,
        use_rgb: sprite_has_rgb(sprite) || batch.runtime_desc.material_key.use_rgb,
        use_normal_tex: batch.runtime_desc.material_key.use_normal_tex,
        use_toon_tex: batch.runtime_desc.material_key.use_toon_tex,
        skinned: batch.runtime_desc.material_key.skinned
            || matches!(special, TechniqueSpecial::SkinnedMesh),
    })
}

fn shadow_pipeline_key(src: PipelineKey, pipeline_name: Option<&str>) -> PipelineKey {
    let mut technique = src.technique;
    technique.special = TechniqueSpecial::Shadow;
    PipelineKey {
        technique,
        blend: SpriteBlend::Normal,
        alpha_blend: false,
        use_depth: true,
        depth_attachment: true,
        cull_back: src.cull_back,
        mesh_fx_variant: src.mesh_fx_variant,
        pipeline_name: pipeline_name.unwrap_or("").to_string(),
        program: shadow_effect_program_from_source(src.program),
    }
}

#[derive(Clone, Copy)]
struct RVec3 {
    x: f32,
    y: f32,
    z: f32,
}

impl RVec3 {
    fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
    }
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
    fn dot(self, rhs: Self) -> f32 {
        self.x * rhs.x + self.y * rhs.y + self.z * rhs.z
    }
    fn cross(self, rhs: Self) -> Self {
        Self::new(
            self.y * rhs.z - self.z * rhs.y,
            self.z * rhs.x - self.x * rhs.z,
            self.x * rhs.y - self.y * rhs.x,
        )
    }
    fn normalize(self) -> Self {
        let len = (self.dot(self)).sqrt();
        if len <= 1e-6 {
            Self::new(0.0, 0.0, 0.0)
        } else {
            Self::new(self.x / len, self.y / len, self.z / len)
        }
    }
}

fn rrotate_x(v: RVec3, angle: f32) -> RVec3 {
    let (s, c) = angle.sin_cos();
    RVec3::new(v.x, v.y * c - v.z * s, v.y * s + v.z * c)
}

fn rrotate_y(v: RVec3, angle: f32) -> RVec3 {
    let (s, c) = angle.sin_cos();
    RVec3::new(v.x * c + v.z * s, v.y, -v.x * s + v.z * c)
}

fn rrotate_z(v: RVec3, angle: f32) -> RVec3 {
    let (s, c) = angle.sin_cos();
    RVec3::new(v.x * c - v.y * s, v.x * s + v.y * c, v.z)
}

fn sprite_camera_basis(sprite: &crate::layer::Sprite) -> (RVec3, RVec3, RVec3, RVec3) {
    let eye = RVec3::new(
        sprite.camera_eye[0],
        sprite.camera_eye[1],
        sprite.camera_eye[2],
    );
    let target = RVec3::new(
        sprite.camera_target[0],
        sprite.camera_target[1],
        sprite.camera_target[2],
    );
    let up = RVec3::new(
        sprite.camera_up[0],
        sprite.camera_up[1],
        sprite.camera_up[2],
    );
    let forward = target.sub(eye).normalize();
    let right = up.cross(forward).normalize();
    let up2 = forward.cross(right).normalize();
    (eye, forward, right, up2)
}

fn transform_model_point_world(
    sprite: &crate::layer::Sprite,
    local: [f32; 3],
    anchor_x: f32,
    anchor_y: f32,
) -> [f32; 3] {
    let mut p = RVec3::new(
        local[0] - sprite.pivot_x,
        local[1] - sprite.pivot_y,
        local[2] - sprite.pivot_z,
    );
    p.x *= sprite.scale_x;
    p.y *= sprite.scale_y;
    p.z *= sprite.scale_z;
    if sprite.billboard {
        let (_, _, right, up) = sprite_camera_basis(sprite);
        let (s, c) = sprite.rotate.sin_cos();
        let rx = p.x * c - p.y * s;
        let ry = p.x * s + p.y * c;
        let anchor = RVec3::new(
            anchor_x + sprite.pivot_x,
            anchor_y + sprite.pivot_y,
            sprite.z + sprite.pivot_z,
        );
        let out = anchor.add(RVec3::new(
            right.x * rx + up.x * ry,
            right.y * rx + up.y * ry,
            right.z * rx + up.z * ry,
        ));
        return [out.x, out.y, out.z];
    }
    p = rrotate_x(p, sprite.rotate_x);
    p = rrotate_y(p, sprite.rotate_y);
    p = rrotate_z(p, sprite.rotate);
    p = p.add(RVec3::new(
        anchor_x + sprite.pivot_x,
        anchor_y + sprite.pivot_y,
        sprite.z + sprite.pivot_z,
    ));
    [p.x, p.y, p.z]
}

fn transform_model_normal_world(sprite: &crate::layer::Sprite, normal: [f32; 3]) -> [f32; 3] {
    let mut n = RVec3::new(normal[0], normal[1], normal[2]);
    if sprite.billboard {
        let (_, forward, right, up) = sprite_camera_basis(sprite);
        let basis_z = forward.normalize();
        let basis_x = right.normalize();
        let basis_y = up.normalize();
        let out = RVec3::new(
            basis_x.x * n.x + basis_y.x * n.y + basis_z.x * n.z,
            basis_x.y * n.x + basis_y.y * n.y + basis_z.y * n.z,
            basis_x.z * n.x + basis_y.z * n.y + basis_z.z * n.z,
        )
        .normalize();
        return [out.x, out.y, out.z];
    }
    n = rrotate_x(n, sprite.rotate_x);
    n = rrotate_y(n, sprite.rotate_y);
    n = rrotate_z(n, sprite.rotate);
    n = n.normalize();
    [n.x, n.y, n.z]
}

fn project_shadow_point(sprite: &crate::layer::Sprite, world: [f32; 3]) -> Option<[f32; 4]> {
    if sprite.light_kind < 2 {
        return None;
    }
    let eye = RVec3::new(
        sprite.light_pos[0],
        sprite.light_pos[1],
        sprite.light_pos[2],
    );
    let light_dir = RVec3::new(
        sprite.light_dir[0],
        sprite.light_dir[1],
        sprite.light_dir[2],
    )
    .normalize();
    let target = eye.add(light_dir);
    let mut up = RVec3::new(0.0, 1.0, 0.0);
    if up.cross(light_dir).dot(up.cross(light_dir)) <= 1e-6 {
        up = RVec3::new(1.0, 0.0, 0.0);
    }
    let forward = target.sub(eye).normalize();
    let right = up.cross(forward).normalize();
    let up2 = forward.cross(right).normalize();
    let rel = RVec3::new(world[0], world[1], world[2]).sub(eye);
    let cx = rel.dot(right);
    let cy = rel.dot(up2);
    let cz = rel.dot(forward);
    if cz <= 1e-3 {
        return None;
    }
    let fov_deg = if sprite.light_cone[0] > 0.0 {
        (2.0 * sprite.light_cone[0].acos()).to_degrees().max(1.0)
    } else {
        45.0
    };
    let tan_half = (fov_deg.to_radians() * 0.5).tan().max(1e-3);
    let x_ndc = cx / (cz * tan_half);
    let y_ndc = cy / (cz * tan_half);
    if x_ndc.abs() > 1.5 || y_ndc.abs() > 1.5 {
        return None;
    }
    let depth = (cz / sprite.light_atten[3].max(1.0)).clamp(0.0, 1.0);
    Some([x_ndc, y_ndc, depth, 1.0])
}

fn sprite_model_cols(
    sprite: &crate::layer::Sprite,
    anchor_x: f32,
    anchor_y: f32,
) -> ([[f32; 4]; 4], [[f32; 4]; 3]) {
    let origin = transform_model_point_world(sprite, [0.0, 0.0, 0.0], anchor_x, anchor_y);
    let px = transform_model_point_world(sprite, [1.0, 0.0, 0.0], anchor_x, anchor_y);
    let py = transform_model_point_world(sprite, [0.0, 1.0, 0.0], anchor_x, anchor_y);
    let pz = transform_model_point_world(sprite, [0.0, 0.0, 1.0], anchor_x, anchor_y);
    let nx = transform_model_normal_world(sprite, [1.0, 0.0, 0.0]);
    let ny = transform_model_normal_world(sprite, [0.0, 1.0, 0.0]);
    let nz = transform_model_normal_world(sprite, [0.0, 0.0, 1.0]);
    (
        [
            [px[0] - origin[0], px[1] - origin[1], px[2] - origin[2], 0.0],
            [py[0] - origin[0], py[1] - origin[1], py[2] - origin[2], 0.0],
            [pz[0] - origin[0], pz[1] - origin[1], pz[2] - origin[2], 0.0],
            [origin[0], origin[1], origin[2], 1.0],
        ],
        [
            [nx[0], nx[1], nx[2], 0.0],
            [ny[0], ny[1], ny[2], 0.0],
            [nz[0], nz[1], nz[2], 0.0],
        ],
    )
}

fn shadow_uniform_data(
    sprite: &crate::layer::Sprite,
) -> ([f32; 4], [f32; 4], [f32; 4], [f32; 4], [f32; 4]) {
    if sprite.light_kind < 2 {
        return (
            [0.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0, 0.0],
        );
    }
    let eye = RVec3::new(
        sprite.light_pos[0],
        sprite.light_pos[1],
        sprite.light_pos[2],
    );
    let light_dir = RVec3::new(
        sprite.light_dir[0],
        sprite.light_dir[1],
        sprite.light_dir[2],
    )
    .normalize();
    let mut up = RVec3::new(0.0, 1.0, 0.0);
    if up.cross(light_dir).dot(up.cross(light_dir)) <= 1e-6 {
        up = RVec3::new(1.0, 0.0, 0.0);
    }
    let forward = light_dir;
    let right = up.cross(forward).normalize();
    let up2 = forward.cross(right).normalize();
    let fov_deg = if sprite.light_cone[0] > 0.0 {
        (2.0 * sprite.light_cone[0].acos()).to_degrees().max(1.0)
    } else {
        45.0
    };
    let tan_half = (fov_deg.to_radians() * 0.5).tan().max(1e-3);
    (
        [eye.x, eye.y, eye.z, 0.0],
        [forward.x, forward.y, forward.z, 0.0],
        [right.x, right.y, right.z, 0.0],
        [up2.x, up2.y, up2.z, 0.0],
        [tan_half, sprite.light_atten[3].max(1.0), 1.0, 0.0],
    )
}

fn normalize_col3(v: [f32; 4]) -> [f32; 4] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len <= 1e-6 {
        [0.0, 0.0, 1.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len, 0.0]
    }
}

fn light_id_selected(ids: &[i32], light_id: i32) -> bool {
    ids.is_empty() || ids.iter().any(|&id| id == light_id)
}

fn fill_mesh_light_uniforms(
    sprite: &crate::layer::Sprite,
    material: &crate::mesh3d::MeshMaterial,
    u: &mut VsUniform,
) {
    let mut dir_count = 0usize;
    let mut point_count = 0usize;
    let mut spot_count = 0usize;
    for lt in &sprite.mesh_runtime_lights {
        match lt.kind {
            0 if dir_count < MAX_BATCH_LIGHTS
                && light_id_selected(&material.directional_light_ids, lt.id) =>
            {
                u.dir_light_diffuse[dir_count] = lt.diffuse;
                u.dir_light_ambient[dir_count] = lt.ambient;
                u.dir_light_specular[dir_count] = lt.specular;
                u.dir_light_dir[dir_count] = lt.dir;
                dir_count += 1;
            }
            1 if point_count < MAX_BATCH_LIGHTS
                && light_id_selected(&material.point_light_ids, lt.id) =>
            {
                u.point_light_diffuse[point_count] = lt.diffuse;
                u.point_light_ambient[point_count] = lt.ambient;
                u.point_light_specular[point_count] = lt.specular;
                u.point_light_pos[point_count] = lt.pos;
                u.point_light_atten[point_count] = lt.atten;
                point_count += 1;
            }
            2 | 3
                if spot_count < MAX_BATCH_LIGHTS
                    && light_id_selected(&material.spot_light_ids, lt.id) =>
            {
                u.spot_light_diffuse[spot_count] = lt.diffuse;
                u.spot_light_ambient[spot_count] = lt.ambient;
                u.spot_light_specular[spot_count] = lt.specular;
                u.spot_light_pos[spot_count] = lt.pos;
                u.spot_light_dir[spot_count] = lt.dir;
                u.spot_light_atten[spot_count] = lt.atten;
                u.spot_light_cone[spot_count] = lt.cone;
                spot_count += 1;
            }
            _ => {}
        }
    }
    u.mesh_light_counts = [dir_count as f32, point_count as f32, spot_count as f32, 0.0];
}

fn render_sprite_frame_per_mesh_set_effect_constant_common(
    sprite: &crate::layer::Sprite,
    anchor_x: f32,
    anchor_y: f32,
    win_w: f32,
    win_h: f32,
    frame_cols: [[f32; 4]; 4],
    material: &crate::mesh3d::MeshMaterial,
) -> VsUniform {
    let mut u = vertex_uniform_for_mesh(sprite, anchor_x, anchor_y, win_w, win_h);
    u.frame_col0 = frame_cols[0];
    u.frame_col1 = frame_cols[1];
    u.frame_col2 = frame_cols[2];
    u.frame_col3 = frame_cols[3];
    u.frame_normal0 = normalize_col3(frame_cols[0]);
    u.frame_normal1 = normalize_col3(frame_cols[1]);
    u.frame_normal2 = normalize_col3(frame_cols[2]);
    u.mtrl_diffuse = material.diffuse;
    u.mtrl_ambient = material.ambient;
    u.mtrl_specular = material.specular;
    u.mtrl_emissive = material.emissive;
    u.mtrl_params = [
        material.power.max(1.0),
        material.lighting_type as i32 as f32,
        material.shading_type as i32 as f32,
        material.rim_light_power.max(0.0),
    ];
    u.mtrl_rim = material.rim_light_color;
    u.mtrl_extra = [
        material.parallax_max_height.max(0.0),
        material.alpha_ref.clamp(0.0, 1.0),
        material.shader_option as f32,
        0.0,
    ];
    u.light_diffuse_u = sprite.light_diffuse;
    u.light_ambient_u = sprite.light_ambient;
    u.light_specular_u = sprite.light_specular;
    u.mesh_flags = [
        if material.use_mesh_tex { 1.0 } else { 0.0 },
        if material.use_mrbd { 1.0 } else { 0.0 },
        if material.use_rgb { 1.0 } else { 0.0 },
        if material.use_mul_vertex_color {
            1.0
        } else {
            0.0
        },
    ];
    u.mesh_mrbd = material.mrbd;
    u.mesh_rgb_rate = material.rgb_rate;
    u.mesh_add_rgb = material.add_rgb;
    u.mesh_misc = [
        material.mul_vertex_color_rate.max(0.0),
        material.depth_buffer_shadow_bias,
        0.0,
        0.0,
    ];
    fill_mesh_light_uniforms(sprite, material, &mut u);
    u
}

fn render_sprite_frame_per_mesh_set_effect_constant_mesh(
    sprite: &crate::layer::Sprite,
    anchor_x: f32,
    anchor_y: f32,
    win_w: f32,
    win_h: f32,
    frame_cols: [[f32; 4]; 4],
    material: &crate::mesh3d::MeshMaterial,
) -> VsUniform {
    let mut u = render_sprite_frame_per_mesh_set_effect_constant_common(
        sprite, anchor_x, anchor_y, win_w, win_h, frame_cols, material,
    );
    u.flags[3] = 0.0;
    u
}

fn render_sprite_frame_per_mesh_set_effect_constant_skinned_mesh(
    sprite: &crate::layer::Sprite,
    anchor_x: f32,
    anchor_y: f32,
    win_w: f32,
    win_h: f32,
    frame_cols: [[f32; 4]; 4],
    material: &crate::mesh3d::MeshMaterial,
) -> VsUniform {
    let mut u = render_sprite_frame_per_mesh_set_effect_constant_common(
        sprite, anchor_x, anchor_y, win_w, win_h, frame_cols, material,
    );
    u.flags[3] = 1.0;
    u
}

fn vertex_uniform_for_mesh(
    sprite: &crate::layer::Sprite,
    anchor_x: f32,
    anchor_y: f32,
    win_w: f32,
    win_h: f32,
) -> VsUniform {
    let (model_cols, normal_cols) = sprite_model_cols(sprite, anchor_x, anchor_y);
    let (eye, forward, right, up) = sprite_camera_basis(sprite);
    let aspect = if win_h.abs() > f32::EPSILON {
        win_w / win_h
    } else {
        1.0
    };
    let hfov = sprite
        .camera_view_angle_deg
        .to_radians()
        .clamp(1e-3, std::f32::consts::PI - 1e-3);
    let tan_half_h = (hfov * 0.5).tan().max(1e-3);
    let tan_half_v = (tan_half_h / aspect.max(1e-3)).max(1e-3);
    let (shadow_eye, shadow_forward, shadow_right, shadow_up, shadow_params) =
        shadow_uniform_data(sprite);
    VsUniform {
        model_col0: model_cols[0],
        model_col1: model_cols[1],
        model_col2: model_cols[2],
        model_col3: model_cols[3],
        normal_col0: normal_cols[0],
        normal_col1: normal_cols[1],
        normal_col2: normal_cols[2],
        frame_col0: [1.0, 0.0, 0.0, 0.0],
        frame_col1: [0.0, 1.0, 0.0, 0.0],
        frame_col2: [0.0, 0.0, 1.0, 0.0],
        frame_col3: [0.0, 0.0, 0.0, 1.0],
        frame_normal0: [1.0, 0.0, 0.0, 0.0],
        frame_normal1: [0.0, 1.0, 0.0, 0.0],
        frame_normal2: [0.0, 0.0, 1.0, 0.0],
        camera_eye: [eye.x, eye.y, eye.z, 0.0],
        camera_forward: [forward.x, forward.y, forward.z, 0.0],
        camera_right: [right.x, right.y, right.z, 0.0],
        camera_up: [up.x, up.y, up.z, 0.0],
        camera_params: [tan_half_h, tan_half_v, win_w.max(1.0), win_h.max(1.0)],
        shadow_eye,
        shadow_forward,
        shadow_right,
        shadow_up,
        shadow_params,
        mtrl_diffuse: [1.0, 1.0, 1.0, 1.0],
        mtrl_ambient: [1.0, 1.0, 1.0, 1.0],
        mtrl_specular: [0.0, 0.0, 0.0, 1.0],
        mtrl_emissive: [0.0, 0.0, 0.0, 1.0],
        mtrl_params: [16.0, 0.0, 0.0, 0.0],
        mtrl_rim: [1.0, 1.0, 1.0, 1.0],
        mtrl_extra: [0.016, 0.001, 0.0, 0.0],
        light_diffuse_u: sprite.light_diffuse,
        light_ambient_u: sprite.light_ambient,
        light_specular_u: sprite.light_specular,
        sprite_effects: [[0.0; 4]; 11],
        single_light_pos_kind: [
            sprite.light_pos[0],
            sprite.light_pos[1],
            sprite.light_pos[2],
            sprite.light_kind as f32,
        ],
        single_light_dir_shadow: [
            sprite.light_dir[0],
            sprite.light_dir[1],
            sprite.light_dir[2],
            if sprite.shadow_receive { 1.0 } else { 0.0 },
        ],
        single_light_atten: sprite.light_atten,
        single_light_cone: sprite.light_cone,
        mesh_flags: [1.0, 0.0, 0.0, 0.0],
        mesh_mrbd: [0.0, 0.0, 0.0, 0.0],
        mesh_rgb_rate: [0.0, 0.0, 0.0, 0.0],
        mesh_add_rgb: [0.0, 0.0, 0.0, 0.0],
        mesh_misc: [1.0, 0.03, 0.0, 0.0],
        mesh_light_counts: [0.0, 0.0, 0.0, 0.0],
        dir_light_diffuse: [[0.0; 4]; MAX_BATCH_LIGHTS],
        dir_light_ambient: [[0.0; 4]; MAX_BATCH_LIGHTS],
        dir_light_specular: [[0.0; 4]; MAX_BATCH_LIGHTS],
        dir_light_dir: [[0.0; 4]; MAX_BATCH_LIGHTS],
        point_light_diffuse: [[0.0; 4]; MAX_BATCH_LIGHTS],
        point_light_ambient: [[0.0; 4]; MAX_BATCH_LIGHTS],
        point_light_specular: [[0.0; 4]; MAX_BATCH_LIGHTS],
        point_light_pos: [[0.0; 4]; MAX_BATCH_LIGHTS],
        point_light_atten: [[0.0; 4]; MAX_BATCH_LIGHTS],
        spot_light_diffuse: [[0.0; 4]; MAX_BATCH_LIGHTS],
        spot_light_ambient: [[0.0; 4]; MAX_BATCH_LIGHTS],
        spot_light_specular: [[0.0; 4]; MAX_BATCH_LIGHTS],
        spot_light_pos: [[0.0; 4]; MAX_BATCH_LIGHTS],
        spot_light_dir: [[0.0; 4]; MAX_BATCH_LIGHTS],
        spot_light_atten: [[0.0; 4]; MAX_BATCH_LIGHTS],
        spot_light_cone: [[0.0; 4]; MAX_BATCH_LIGHTS],
        flags: [
            1.0,
            if sprite.camera_enabled { 1.0 } else { 0.0 },
            if sprite.light_kind >= 2 { 1.0 } else { 0.0 },
            0.0,
        ],
    }
}

fn mesh_animation_state_for_sprite(
    sprite: &crate::layer::Sprite,
) -> crate::mesh3d::MeshAnimationState {
    sprite.mesh_animation.sanitized()
}

fn resolved_mesh_pipeline_name_from_runtime_desc(
    desc: &crate::mesh3d::MeshPrimitiveRuntimeDesc,
    technique: TechniqueKey,
) -> String {
    let mut technique_name = desc.technique_name.clone();
    if technique.light {
        technique_name.push_str("_light");
    } else if technique.fog {
        technique_name.push_str("_fog");
    }
    if technique.d3 {
        technique_name.push_str("_d3");
    }
    format!("{}::{}", desc.effect_key, technique_name)
}

fn resolved_shadow_pipeline_name_from_runtime_desc(
    desc: &crate::mesh3d::MeshPrimitiveRuntimeDesc,
) -> String {
    format!("{}::{}", desc.shadow_effect_key, desc.shadow_technique_name)
}

fn pipeline_program_for_special(special: TechniqueSpecial) -> EffectProgram {
    match special {
        TechniqueSpecial::Overlay => EffectProgram::OverlayGpu,
        TechniqueSpecial::WipeMosaic => EffectProgram::WipeMosaic,
        TechniqueSpecial::WipeRasterH => EffectProgram::WipeRasterH,
        TechniqueSpecial::WipeRasterV => EffectProgram::WipeRasterV,
        TechniqueSpecial::WipeExplosionBlur => EffectProgram::WipeExplosionBlur,
        TechniqueSpecial::WipeShimi => EffectProgram::WipeShimi,
        TechniqueSpecial::WipeShimiInv => EffectProgram::WipeShimiInv,
        TechniqueSpecial::WipeCrossMosaic => EffectProgram::WipeCrossMosaic,
        TechniqueSpecial::WipeCrossRasterH => EffectProgram::WipeCrossRasterH,
        TechniqueSpecial::WipeCrossRasterV => EffectProgram::WipeCrossRasterV,
        TechniqueSpecial::WipeCrossExplosionBlur => EffectProgram::WipeCrossExplosionBlur,
        TechniqueSpecial::None => EffectProgram::Sprite2D,
        TechniqueSpecial::Mesh | TechniqueSpecial::SkinnedMesh | TechniqueSpecial::Shadow => {
            unreachable!("mesh/shadow techniques must resolve through MeshPrimitiveRuntimeDesc")
        }
    }
}

fn mesh_effect_program_from_runtime_desc(
    desc: &crate::mesh3d::MeshPrimitiveRuntimeDesc,
) -> EffectProgram {
    let skinned = matches!(
        desc.effect_profile,
        crate::mesh3d::MeshEffectProfile::SkinnedMesh
    ) || desc.material_key.skinned;
    match (skinned, desc.material_key.lighting_type) {
        (false, crate::mesh3d::MeshLightingType::None) => EffectProgram::MeshStaticUnlit,
        (false, crate::mesh3d::MeshLightingType::Lambert) => EffectProgram::MeshStaticLambert,
        (false, crate::mesh3d::MeshLightingType::BlinnPhong) => EffectProgram::MeshStaticBlinnPhong,
        (false, crate::mesh3d::MeshLightingType::PerPixelBlinnPhong) => {
            EffectProgram::MeshStaticPerPixelBlinnPhong
        }
        (false, crate::mesh3d::MeshLightingType::PerPixelHalfLambert) => {
            EffectProgram::MeshStaticPerPixelHalfLambert
        }
        (false, crate::mesh3d::MeshLightingType::Toon) => EffectProgram::MeshStaticToon,
        (false, crate::mesh3d::MeshLightingType::FixedFunction) => {
            EffectProgram::MeshStaticFixedFunction
        }
        (false, crate::mesh3d::MeshLightingType::PerPixelFixedFunction) => {
            EffectProgram::MeshStaticPerPixelFixedFunction
        }
        (false, crate::mesh3d::MeshLightingType::Bump) => EffectProgram::MeshStaticBump,
        (false, crate::mesh3d::MeshLightingType::Parallax) => EffectProgram::MeshStaticParallax,
        (true, crate::mesh3d::MeshLightingType::None) => EffectProgram::MeshSkinnedUnlit,
        (true, crate::mesh3d::MeshLightingType::Lambert) => EffectProgram::MeshSkinnedLambert,
        (true, crate::mesh3d::MeshLightingType::BlinnPhong) => EffectProgram::MeshSkinnedBlinnPhong,
        (true, crate::mesh3d::MeshLightingType::PerPixelBlinnPhong) => {
            EffectProgram::MeshSkinnedPerPixelBlinnPhong
        }
        (true, crate::mesh3d::MeshLightingType::PerPixelHalfLambert) => {
            EffectProgram::MeshSkinnedPerPixelHalfLambert
        }
        (true, crate::mesh3d::MeshLightingType::Toon) => EffectProgram::MeshSkinnedToon,
        (true, crate::mesh3d::MeshLightingType::FixedFunction) => {
            EffectProgram::MeshSkinnedFixedFunction
        }
        (true, crate::mesh3d::MeshLightingType::PerPixelFixedFunction) => {
            EffectProgram::MeshSkinnedPerPixelFixedFunction
        }
        (true, crate::mesh3d::MeshLightingType::Bump) => EffectProgram::MeshSkinnedBump,
        (true, crate::mesh3d::MeshLightingType::Parallax) => EffectProgram::MeshSkinnedParallax,
    }
}

fn shadow_effect_program_from_source(src: EffectProgram) -> EffectProgram {
    match src {
        EffectProgram::MeshSkinnedUnlit
        | EffectProgram::MeshSkinnedLambert
        | EffectProgram::MeshSkinnedBlinnPhong
        | EffectProgram::MeshSkinnedPerPixelBlinnPhong
        | EffectProgram::MeshSkinnedPerPixelHalfLambert
        | EffectProgram::MeshSkinnedToon
        | EffectProgram::MeshSkinnedFixedFunction
        | EffectProgram::MeshSkinnedPerPixelFixedFunction
        | EffectProgram::MeshSkinnedBump
        | EffectProgram::MeshSkinnedParallax
        | EffectProgram::ShadowSkinned => EffectProgram::ShadowSkinned,
        _ => EffectProgram::ShadowStatic,
    }
}

fn technique_name_for_pipeline(key: &PipelineKey) -> String {
    let base = if !key.pipeline_name.is_empty() {
        key.pipeline_name.clone()
    } else {
        match key.technique.special {
            TechniqueSpecial::Overlay => "tec_overlay_gpu".to_string(),
            TechniqueSpecial::WipeMosaic => "tec_tex1_mosaic".to_string(),
            TechniqueSpecial::WipeRasterH => "tec_tex1_raster_h".to_string(),
            TechniqueSpecial::WipeRasterV => "tec_tex1_raster_v".to_string(),
            TechniqueSpecial::WipeExplosionBlur => "tec_tex1_explosion_blur".to_string(),
            TechniqueSpecial::WipeShimi => "tec_tex1_shimi".to_string(),
            TechniqueSpecial::WipeShimiInv => "tec_tex1_shimi_inv".to_string(),
            TechniqueSpecial::WipeCrossMosaic => "tec_tex2_mosaic".to_string(),
            TechniqueSpecial::WipeCrossRasterH => "tec_tex2_raster_h".to_string(),
            TechniqueSpecial::WipeCrossRasterV => "tec_tex2_raster_v".to_string(),
            TechniqueSpecial::WipeCrossExplosionBlur => "tec_tex2_explosion_blur".to_string(),
            TechniqueSpecial::Mesh | TechniqueSpecial::SkinnedMesh => {
                let mut name = crate::mesh3d::mesh_effect_key_from_variant(key.mesh_fx_variant);
                if key.technique.light {
                    name.push_str("::tech_light");
                } else if key.technique.fog {
                    name.push_str("::tech_fog");
                } else {
                    name.push_str("::tech");
                }
                if key.technique.d3 {
                    name.push_str("_d3");
                }
                name
            }
            TechniqueSpecial::Shadow => {
                let base_key = crate::mesh3d::MeshRuntimeMaterialKey {
                    use_mesh_tex: false,
                    use_shadow_tex: false,
                    use_toon_tex: false,
                    use_normal_tex: false,
                    use_mul_vertex_color: false,
                    use_mrbd: false,
                    use_rgb: false,
                    lighting_type: crate::mesh3d::MeshLightingType::None,
                    shading_type: crate::mesh3d::MeshShadingType::None,
                    shader_option: crate::mesh3d::MESH_SHADER_OPTION_NONE,
                    skinned: matches!(key.program, EffectProgram::ShadowSkinned),
                    alpha_test_enable: false,
                    cull_disable: false,
                    shadow_map_enable: true,
                };
                format!(
                    "{}::tech",
                    crate::mesh3d::mesh_effect_filename_from_runtime_key(
                        crate::mesh3d::MeshEffectProfile::ShadowMap,
                        base_key,
                    )
                )
            }
            TechniqueSpecial::None => {
                let vertex_name = format!(
                    "{}{}",
                    if key.technique.d3 { "_d3" } else { "" },
                    if key.technique.light {
                        "_light"
                    } else if key.technique.fog {
                        "_fog"
                    } else {
                        ""
                    }
                );
                let pixel_name = format!(
                    "{}{}{}{}{}{}{}{}",
                    if key.technique.light {
                        "_v2"
                    } else if key.technique.fog {
                        "_v1"
                    } else {
                        "_v0"
                    },
                    if key.technique.tex != 0 { "_tex" } else { "" },
                    if key.technique.diffuse {
                        "_diffuse"
                    } else {
                        ""
                    },
                    if key.technique.mrbd { "_mrbd" } else { "" },
                    if key.technique.rgb { "_rgb" } else { "" },
                    if key.technique.tonecurve {
                        "_tonecurve"
                    } else {
                        ""
                    },
                    if key.technique.mask { "_mask" } else { "" },
                    match key.blend {
                        SpriteBlend::Normal => "",
                        SpriteBlend::Add => "_add",
                        SpriteBlend::Sub => "_sub",
                        SpriteBlend::Mul => "_mul",
                        SpriteBlend::Screen => "_screen",
                        SpriteBlend::Overlay => "_overlay",
                    }
                );
                format!("tec{}{}", vertex_name, pixel_name)
            }
        }
    };
    format!("{}#{}", base, key.program.short_name())
}

impl Renderer {
    pub async fn new(window: &'static Window) -> Result<Self> {
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        let backends = wgpu::Backends::GL;
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        let backends = wgpu::Backends::all();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });

        let surface = instance.create_surface(window).context("create_surface")?;
        let size = window.inner_size();
        let scale_factor = window.scale_factor() as f32;
        Self::new_from_instance_surface(instance, surface, size.width, size.height, scale_factor)
            .await
    }

    #[cfg(any(target_os = "android", target_os = "ios"))]
    pub async unsafe fn new_from_raw_handles(
        raw_display_handle: raw_window_handle::RawDisplayHandle,
        raw_window_handle: raw_window_handle::RawWindowHandle,
        width: u32,
        height: u32,
        scale_factor: f32,
    ) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let surface = instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle,
                raw_window_handle,
            })
            .context("create_surface_unsafe")?;
        Self::new_from_instance_surface(instance, surface, width, height, scale_factor).await
    }

    async fn new_from_instance_surface(
        instance: wgpu::Instance,
        surface: wgpu::Surface<'static>,
        width: u32,
        height: u32,
        scale_factor: f32,
    ) -> Result<Self> {
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("request_adapter")?;

        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("siglus-bg-device"),
                    required_features: wgpu::Features::empty(),
                    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
                    required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .context("request_device")?;

        let surface_caps = surface.get_capabilities(&adapter);
        // The original D3D9 renderer uses A8R8G8B8/X8R8G8B8 without
        // D3DSAMP_SRGBTEXTURE or D3DRS_SRGBWRITEENABLE.  Prefer the non-sRGB
        // surface view so texture sampling, blending, and render-target writes
        // operate directly on the stored 8-bit channel values.
        let format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);
        if format.is_srgb() {
            log::error!(
                "adapter exposes no non-sRGB surface format; D3D9 byte-space blending cannot be reproduced exactly (using {format:?})"
            );
        }

        let scale_factor = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        let width = width.max(1);
        let height = height.max(1);
        let logical_width = ((width as f32) / scale_factor).max(1.0);
        let logical_height = ((height as f32) / scale_factor).max(1.0);
        let alpha_mode = surface_caps
            .alpha_modes
            .iter()
            .copied()
            .find(|m| *m == wgpu::CompositeAlphaMode::Opaque)
            .unwrap_or(surface_caps.alpha_modes[0]);
        let present_mode = surface_caps
            .present_modes
            .iter()
            .copied()
            .find(|m| *m == wgpu::PresentMode::Fifo)
            .unwrap_or(surface_caps.present_modes[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("siglus-sprite-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 9,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 10,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 11,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 12,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 13,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 14,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 15,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 16,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 17,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        let shader_source = wgpu::ShaderSource::Wgsl(wasm_shader_source().into());
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        let shader_source = wgpu::ShaderSource::Wgsl(SHADER.into());
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("siglus-sprite-shader"),
            source: shader_source,
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("siglus-sprite-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let (wipe_bind_group_layout, wipe_pipeline) = create_wipe_pipeline(&device, config.format);
        let (page_wipe_bind_group_layout, page_wipe_pipeline) =
            create_page_wipe_pipeline(&device, config.format);

        let vertex_capacity = 6;
        let vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("siglus-sprite-vertex-buf"),
            size: (vertex_capacity * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        let vertex_sprite2d_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("siglus-sprite2d-vertex-buf"),
            size: (vertex_capacity * std::mem::size_of::<VertexSprite2dData>())
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        let vertex_sprite2d_capacity = vertex_capacity;

        let default_aux = create_solid_texture(&device, &queue, [255, 255, 255, 255])?;
        let fog_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("siglus-cfx-fog-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 0.0,
            ..Default::default()
        });
        let mesh_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("siglus-tona3-mesh-wrap-sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let normal_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("siglus-tona3-normal-clamp-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 0.0,
            ..Default::default()
        });
        let toon_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("siglus-tona3-toon-clamp-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 0.0,
            ..Default::default()
        });
        let shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("siglus-tona3-shadow-point-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 0.0,
            ..Default::default()
        });
        let internal_width = logical_width.max(1.0).round() as u32;
        let internal_height = logical_height.max(1.0).round() as u32;
        let depth = create_depth_texture(&device, internal_width, internal_height);
        let surface_depth = create_depth_texture(&device, config.width, config.height);
        let scene_a = create_render_target_texture(
            &device,
            internal_width,
            internal_height,
            config.format,
            "siglus-scene-a",
        );
        let scene_b = create_render_target_texture(
            &device,
            internal_width,
            internal_height,
            config.format,
            "siglus-scene-b",
        );
        let wipe_a = create_render_target_texture(
            &device,
            internal_width,
            internal_height,
            config.format,
            "siglus-wipe-a",
        );
        let wipe_b = create_render_target_texture(
            &device,
            internal_width,
            internal_height,
            config.format,
            "siglus-wipe-b",
        );
        let shadow_map =
            create_render_target_texture(&device, 2048, 2048, config.format, "siglus-shadow-map");
        let shadow_depth = create_depth_texture_with_format(
            &device,
            2048,
            2048,
            wgpu::TextureFormat::Depth16Unorm,
            "siglus-shadow-depth-d16",
        );

        let surface_viewport = SurfaceViewport::full(config.width, config.height);
        let emote_compositor = emote::EmoteCompositor::new(&device);
        Ok(Self {
            surface,
            device,
            queue,
            config,
            logical_width,
            logical_height,
            scale_factor: scale_factor.max(1.0),
            surface_viewport,
            pipelines: HashMap::new(),
            bind_group_layout,
            shader,
            pipeline_layout,
            wipe_bind_group_layout,
            wipe_pipeline,
            page_wipe_bind_group_layout,
            page_wipe_pipeline,
            vertex_buf,
            vertex_capacity,
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            vertex_sprite2d_buf,
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            vertex_sprite2d_capacity,
            textures: HashMap::new(),
            external_textures: HashMap::new(),
            mesh_assets: HashMap::new(),
            default_aux,
            fog_sampler,
            mesh_sampler,
            normal_sampler,
            toon_sampler,
            shadow_sampler,
            depth,
            surface_depth,
            scene_a,
            scene_b,
            wipe_a,
            wipe_b,
            wipe_mask_cache: None,
            shadow_map,
            shadow_depth,
            verts: Vec::new(),
            draws: Vec::new(),
            draw_gpu_slots: Vec::new(),
            draw_bind_epoch: 1,
            debug_frame_serial: 0,
            emote_compositor,
        })
    }

    fn recreate_logical_render_targets(&mut self) {
        let width = self.logical_width.max(1.0).round() as u32;
        let height = self.logical_height.max(1.0).round() as u32;
        self.depth = create_depth_texture(&self.device, width, height);
        self.scene_a = create_render_target_texture(
            &self.device,
            width,
            height,
            self.config.format,
            "siglus-scene-a",
        );
        self.scene_b = create_render_target_texture(
            &self.device,
            width,
            height,
            self.config.format,
            "siglus-scene-b",
        );
        self.wipe_a = create_render_target_texture(
            &self.device,
            width,
            height,
            self.config.format,
            "siglus-wipe-a",
        );
        self.wipe_b = create_render_target_texture(
            &self.device,
            width,
            height,
            self.config.format,
            "siglus-wipe-b",
        );
        self.wipe_mask_cache = None;
        self.draw_bind_epoch = self.draw_bind_epoch.wrapping_add(1).max(1);
    }

    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.resize_with_scale(width, height, self.scale_factor);
    }

    pub fn resize_with_scale(&mut self, width: u32, height: u32, scale_factor: f32) {
        if width == 0 || height == 0 {
            return;
        }
        let sf = if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        };
        self.scale_factor = sf;
        self.logical_width = ((width as f32) / sf).max(1.0);
        self.logical_height = ((height as f32) / sf).max(1.0);
        self.surface_viewport = SurfaceViewport::full(width, height);
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.surface_depth =
            create_depth_texture(&self.device, self.config.width, self.config.height);
        self.recreate_logical_render_targets();
    }

    pub fn resize_with_logical_viewport(
        &mut self,
        surface_width: u32,
        surface_height: u32,
        scale_factor: f32,
        logical_width: u32,
        logical_height: u32,
        viewport_x: u32,
        viewport_y: u32,
        viewport_width: u32,
        viewport_height: u32,
    ) {
        self.resize_with_scale(surface_width, surface_height, scale_factor);
        self.logical_width = logical_width.max(1) as f32;
        self.logical_height = logical_height.max(1) as f32;
        self.recreate_logical_render_targets();
        let max_w = self.config.width;
        let max_h = self.config.height;
        let x = viewport_x.min(max_w.saturating_sub(1));
        let y = viewport_y.min(max_h.saturating_sub(1));
        let w = viewport_width.max(1).min(max_w.saturating_sub(x).max(1));
        let h = viewport_height.max(1).min(max_h.saturating_sub(y).max(1));
        self.surface_viewport = SurfaceViewport { x, y, w, h };
    }

    pub fn logical_size(&self) -> (u32, u32) {
        (
            self.logical_width.max(1.0).round() as u32,
            self.logical_height.max(1.0).round() as u32,
        )
    }

    pub fn render_sprites(
        &mut self,
        images: &ImageManager,
        sprites: &[RenderSprite],
    ) -> Result<()> {
        self.render_frame(images, &RenderFrame::ordinary(sprites.to_vec()))
    }

    pub fn render_frame(&mut self, images: &ImageManager, frame_plan: &RenderFrame) -> Result<()> {
        let frame = self
            .surface
            .get_current_texture()
            .context("get_current_texture")?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        self.debug_frame_serial = self.debug_frame_serial.wrapping_add(1);
        {
            let mut live_emote_ids = HashSet::new();
            let mut collect = |sprites: &[RenderSprite]| {
                for entry in sprites {
                    if let Some(packet) = entry.sprite.emote_render.as_deref() {
                        live_emote_ids.insert(packet.render_id);
                    }
                }
            };
            if let Some(wipe) = frame_plan.wipe.as_ref() {
                collect(&wipe.under);
                collect(&wipe.current);
                collect(&wipe.next);
                collect(&wipe.over);
            } else {
                collect(&frame_plan.sprites);
            }
            self.emote_compositor.retain_render_ids(&live_emote_ids);
        }

        // The original engine draws an ordinary frame directly into the final
        // opaque back buffer.  A game/offscreen buffer is only used when a
        // feature actually needs to sample the already rendered scene (WIPE,
        // OVERLAY, capture, ...).  Routing every frame through an
        // alpha-bearing texture changes SRCALPHA/INVSRCALPHA accumulation and
        // makes translucent objects and glyphs darken against the intermediate
        // black surface before presentation.
        let needs_scene_texture = frame_plan.wipe.is_some()
            || frame_plan
                .sprites
                .iter()
                .any(|entry| matches!(entry.sprite.blend, SpriteBlend::Overlay));

        if needs_scene_texture {
            let final_target = self.render_frame_to_internal(images, frame_plan)?;
            let blit_range = self.prepare_blit_vertices()?;
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("siglus-present-encoder"),
                });
            self.render_copy_pass(
                &mut encoder,
                ColorTarget::External(&view),
                final_target,
                blit_range,
            )?;
            self.queue.submit(Some(encoder.finish()));
        } else {
            self.render_ordinary_frame_to_surface(images, &frame_plan.sprites, &view)?;
        }

        frame.present();
        Ok(())
    }

    fn render_ordinary_frame_to_surface(
        &mut self,
        images: &ImageManager,
        sprites: &[RenderSprite],
        view: &wgpu::TextureView,
    ) -> Result<()> {
        let _ = self.prepare_draws(images, sprites, true)?;
        let draws_for_pass = self.draws.clone();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("siglus-direct-present-encoder"),
            });

        let shadow_indices: Vec<usize> = draws_for_pass
            .iter()
            .enumerate()
            .filter_map(|(idx, cmd)| cmd.shadow_cast.then_some(idx))
            .collect();
        if !shadow_indices.is_empty() {
            self.render_command_slice(
                &mut encoder,
                ColorTarget::Internal(InternalColorTarget::ShadowMap),
                DepthTarget::Shadow,
                0..0,
                wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                true,
                None,
                None,
            )?;
            for idx in shadow_indices {
                self.render_command_slice(
                    &mut encoder,
                    ColorTarget::Internal(InternalColorTarget::ShadowMap),
                    DepthTarget::Shadow,
                    idx..idx + 1,
                    wgpu::LoadOp::Load,
                    false,
                    None,
                    Some(TechniqueSpecial::Shadow),
                )?;
            }
        }

        self.render_command_slice(
            &mut encoder,
            ColorTarget::External(view),
            DepthTarget::Surface,
            0..draws_for_pass.len(),
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            true,
            None,
            None,
        )?;
        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    fn prepare_draws(
        &mut self,
        images: &ImageManager,
        sprites: &[RenderSprite],
        external_target: bool,
    ) -> Result<std::ops::Range<u32>> {
        self.verts.clear();
        self.draws.clear();

        let win_w = self.logical_width.max(1.0);
        let win_h = self.logical_height.max(1.0);
        let (surface_w, surface_h, surface_viewport) = if external_target {
            (self.config.width, self.config.height, self.surface_viewport)
        } else {
            let width = self.logical_width.max(1.0).round() as u32;
            let height = self.logical_height.max(1.0).round() as u32;
            (width, height, SurfaceViewport::full(width, height))
        };

        for s in sprites {
            let sprite = &s.sprite;
            let img_id = sprite.image_id;
            let img = img_id.and_then(|id| images.get(id));
            let emote_packet = sprite.emote_render.as_deref();
            let emote_render_id = if let Some(packet) = emote_packet {
                self.emote_compositor
                    .prepare(&self.device, &self.queue, packet)?;
                Some(packet.render_id)
            } else {
                None
            };
            let (source_width, source_height) = if let Some(img) = img {
                (img.width, img.height)
            } else if let Some(packet) = emote_packet {
                (packet.width, packet.height)
            } else {
                (1, 1)
            };
            let (src_left, src_top, src_right, src_bottom) =
                src_clip_rect(sprite.src_clip, source_width, source_height)?;
            let src_w = (src_right - src_left).max(1.0);
            let src_h = (src_bottom - src_top).max(1.0);
            let (dst_x, dst_y, dst_w, dst_h) = match sprite.fit {
                SpriteFit::FullScreen => (0.0f32, 0.0f32, win_w, win_h),
                SpriteFit::PixelRect => {
                    let (w, h) = match sprite.size_mode {
                        SpriteSizeMode::Intrinsic => (src_w, src_h),
                        SpriteSizeMode::Explicit { width, height } => (width as f32, height as f32),
                    };
                    (sprite.x as f32, sprite.y as f32, w, h)
                }
            };

            let scissor = dst_scissor_rect_to_viewport(
                sprite.dst_clip,
                surface_viewport,
                win_w,
                win_h,
                surface_w,
                surface_h,
            );
            if let Some(sci) = scissor {
                if sci.w == 0 || sci.h == 0 {
                    continue;
                }
            }

            let alpha = (sprite.alpha as f32) / 255.0;
            let tr = (sprite.tr as f32) / 255.0;
            let mono = (sprite.mono as f32) / 255.0;
            let reverse = (sprite.reverse as f32) / 255.0;
            let bright = (sprite.bright as f32) / 255.0;
            let dark = (sprite.dark as f32) / 255.0;
            let color_rate = (sprite.color_rate as f32) / 255.0;
            let color_add_r = (sprite.color_add_r as f32) / 255.0;
            let color_add_g = (sprite.color_add_g as f32) / 255.0;
            let color_add_b = (sprite.color_add_b as f32) / 255.0;
            let color_r = (sprite.color_r as f32) / 255.0;
            let color_g = (sprite.color_g as f32) / 255.0;
            let color_b = (sprite.color_b as f32) / 255.0;
            let effects1 = [tr, mono, reverse, bright];
            let effects2 = [dark, color_rate, color_add_r, color_add_g];
            let effects3 = [color_add_b, color_r, color_g, color_b];

            let has_mask = sprite.mask_image_id.and_then(|id| images.get(id)).is_some();
            let has_tonecurve = sprite
                .tonecurve_image_id
                .and_then(|id| images.get(id))
                .is_some();
            let has_wipe_src = sprite
                .wipe_src_image_id
                .and_then(|id| images.get(id))
                .is_some();
            let has_fog_tex = sprite
                .fog_texture_image_id
                .and_then(|id| images.get(id))
                .is_some();

            let effects4 = [
                sprite.mask_mode as f32,
                if sprite.alpha_test { 1.0 } else { 0.0 },
                if sprite.light_enabled { 1.0 } else { 0.0 },
                if sprite.fog_enabled { 1.0 } else { 0.0 },
            ];
            let effects5 = [
                if has_mask { 1.0 } else { 0.0 },
                if has_tonecurve { 1.0 } else { 0.0 },
                sprite.tonecurve_row,
                sprite.tonecurve_sat,
            ];
            let effects6 = [
                sprite.wipe_fx_mode as f32,
                sprite.wipe_fx_params[0],
                sprite.wipe_fx_params[1],
                sprite.wipe_fx_params[2],
            ];
            let blend_code = match sprite.blend {
                SpriteBlend::Normal => 0.0,
                SpriteBlend::Add => 1.0,
                SpriteBlend::Sub => 2.0,
                SpriteBlend::Mul => 3.0,
                SpriteBlend::Screen => 4.0,
                SpriteBlend::Overlay => 5.0,
            };
            let effects7 = if sprite.wipe_fx_mode >= 10 {
                [
                    sprite.wipe_fx_params[3],
                    if has_wipe_src { 1.0 } else { 0.0 },
                    blend_code,
                    sprite.tonecurve_sat,
                ]
            } else {
                [0.0, if has_wipe_src { 1.0 } else { 0.0 }, blend_code, 0.0]
            };
            let effects8 = [
                sprite.light_diffuse[0],
                sprite.light_diffuse[1],
                sprite.light_diffuse[2],
                sprite.light_factor,
            ];
            let effects9 = [
                sprite.light_ambient[0],
                sprite.light_ambient[1],
                sprite.light_ambient[2],
                sprite.fog_scroll_x,
            ];
            let effects10 = [
                sprite.fog_color[0],
                sprite.fog_color[1],
                sprite.fog_color[2],
                sprite.z,
            ];
            let effects11 = [
                sprite.fog_near,
                sprite.fog_far,
                if has_fog_tex { 1.0 } else { 0.0 },
                sprite.camera_eye[2],
            ];
            let zero4 = [0.0f32; 4];
            let light_pos_kind_base = [
                sprite.light_pos[0],
                sprite.light_pos[1],
                sprite.light_pos[2],
                sprite.light_kind as f32,
            ];
            let light_dir_shadow_base = [
                sprite.light_dir[0],
                sprite.light_dir[1],
                sprite.light_dir[2],
                if sprite.shadow_receive && sprite.light_cone[3] > 0.5 {
                    1.0
                } else {
                    0.0
                },
            ];
            let light_atten_base = sprite.light_atten;
            let light_cone_base = sprite.light_cone;

            let mut special_override = None;
            let mut mesh_batches: Option<Vec<crate::mesh3d::MeshGpuPrimitiveBatch>> = None;
            if sprite.mesh_kind != 0 {
                if let Some(file_name) = sprite.mesh_file_name.as_deref() {
                    if let Some(asset) = self.ensure_mesh_asset(images, file_name) {
                        let anim_state = mesh_animation_state_for_sprite(sprite);
                        let sampled = asset.sample_gpu_primitives_with_state(&anim_state);
                        if !sampled.is_empty() {
                            special_override = Some(if asset.is_skinned() {
                                TechniqueSpecial::SkinnedMesh
                            } else {
                                TechniqueSpecial::Mesh
                            });
                            mesh_batches = Some(sampled);
                        }
                    }
                }
            }

            let use_depth = uses_depth_pipeline(sprite);
            let technique = build_technique_key(
                sprite,
                has_mask,
                has_tonecurve,
                has_wipe_src,
                special_override,
            );
            let draw_kind = if matches!(technique.special, TechniqueSpecial::Shadow) {
                MeshDrawKind::ShadowCaster
            } else if matches!(technique.special, TechniqueSpecial::SkinnedMesh) {
                MeshDrawKind::SkinnedMesh
            } else if matches!(technique.special, TechniqueSpecial::Mesh) {
                MeshDrawKind::StaticMesh
            } else {
                MeshDrawKind::SpriteQuad
            };
            let requires_alpha_composition = sprite.alpha < 255
                || sprite.tr < 255
                || has_mask
                || has_tonecurve
                || has_wipe_src
                || sprite.wipe_fx_mode != 0;
            let pipeline_key = PipelineKey {
                technique,
                blend: sprite.blend,
                alpha_blend: if matches!(technique.special, TechniqueSpecial::Overlay) {
                    false
                } else {
                    sprite.alpha_blend || requires_alpha_composition
                },
                use_depth,
                depth_attachment: true,
                cull_back: pipeline_cull_back(sprite, false),
                mesh_fx_variant: 0,
                pipeline_name: String::new(),
                program: pipeline_program_for_special(technique.special),
            };

            if let Some(mesh_batches) = mesh_batches {
                let technique_special = special_override.unwrap_or(TechniqueSpecial::Mesh);
                for batch in mesh_batches {
                    if batch.vertices.is_empty() {
                        continue;
                    }
                    let batch_special = if batch.skinned {
                        TechniqueSpecial::SkinnedMesh
                    } else {
                        technique_special
                    };
                    let mut batch_technique = build_technique_key(
                        sprite,
                        has_mask,
                        has_tonecurve,
                        has_wipe_src,
                        Some(batch_special),
                    );
                    batch_technique.tex = batch_technique
                        .tex
                        .max(u8::from(batch.runtime_desc.material_key.use_mesh_tex));
                    batch_technique.mrbd =
                        batch_technique.mrbd || batch.runtime_desc.material_key.use_mrbd;
                    batch_technique.rgb =
                        batch_technique.rgb || batch.runtime_desc.material_key.use_rgb;
                    let batch_draw_kind =
                        if matches!(batch_technique.special, TechniqueSpecial::Shadow) {
                            MeshDrawKind::ShadowCaster
                        } else if matches!(batch_technique.special, TechniqueSpecial::SkinnedMesh) {
                            MeshDrawKind::SkinnedMesh
                        } else if matches!(batch_technique.special, TechniqueSpecial::Mesh) {
                            MeshDrawKind::StaticMesh
                        } else {
                            MeshDrawKind::SpriteQuad
                        };
                    let batch_pipeline_key = PipelineKey {
                        technique: batch_technique,
                        blend: sprite.blend,
                        alpha_blend: if matches!(batch_technique.special, TechniqueSpecial::Overlay)
                        {
                            false
                        } else {
                            sprite.alpha_blend || requires_alpha_composition
                        },
                        use_depth,
                        depth_attachment: true,
                        cull_back: pipeline_cull_back(sprite, batch.material.cull_disable),
                        mesh_fx_variant: crate::mesh3d::mesh_effect_variant_bits_from_runtime_desc(
                            &batch.runtime_desc,
                        ),
                        pipeline_name: resolved_mesh_pipeline_name_from_runtime_desc(
                            &batch.runtime_desc,
                            batch_technique,
                        ),
                        program: mesh_effect_program_from_runtime_desc(&batch.runtime_desc),
                    };
                    let base = self.verts.len() as u32;
                    let mut added = 0u32;
                    let mut vs_uniform = if batch.skinned {
                        render_sprite_frame_per_mesh_set_effect_constant_skinned_mesh(
                            sprite,
                            sprite.x as f32,
                            sprite.y as f32,
                            win_w,
                            win_h,
                            batch.frame_cols,
                            &batch.material,
                        )
                    } else {
                        render_sprite_frame_per_mesh_set_effect_constant_mesh(
                            sprite,
                            sprite.x as f32,
                            sprite.y as f32,
                            win_w,
                            win_h,
                            batch.frame_cols,
                            &batch.material,
                        )
                    };
                    debug_assert!(batch.bone_cols.len() <= MAX_BONES);
                    let bone_uniform = BoneUniform::from_cols_list(&batch.bone_cols);
                    let effects4 = [
                        sprite.mask_mode as f32,
                        if sprite.alpha_test || batch.material.alpha_test_enable {
                            1.0
                        } else {
                            0.0
                        },
                        if sprite.light_enabled { 1.0 } else { 0.0 },
                        if sprite.fog_enabled { 1.0 } else { 0.0 },
                    ];
                    set_sprite2d_effect_uniforms(
                        &mut vs_uniform,
                        effects1,
                        effects2,
                        effects3,
                        effects4,
                        effects5,
                        effects6,
                        effects7,
                        effects8,
                        effects9,
                        effects10,
                        effects11,
                    );
                    for tri in batch.vertices.chunks(3) {
                        if tri.len() != 3 {
                            continue;
                        }
                        let v0_bones = [
                            tri[0].bone_indices[0] as f32,
                            tri[0].bone_indices[1] as f32,
                            tri[0].bone_indices[2] as f32,
                            tri[0].bone_indices[3] as f32,
                        ];
                        let v1_bones = [
                            tri[1].bone_indices[0] as f32,
                            tri[1].bone_indices[1] as f32,
                            tri[1].bone_indices[2] as f32,
                            tri[1].bone_indices[3] as f32,
                        ];
                        let v2_bones = [
                            tri[2].bone_indices[0] as f32,
                            tri[2].bone_indices[1] as f32,
                            tri[2].bone_indices[2] as f32,
                            tri[2].bone_indices[3] as f32,
                        ];
                        let mut v0_effects8 = effects8;
                        let mut v1_effects8 = effects8;
                        let mut v2_effects8 = effects8;
                        let mut v0_effects9 = effects9;
                        let mut v1_effects9 = effects9;
                        let mut v2_effects9 = effects9;
                        v0_effects8[0] = tri[0].color[0];
                        v0_effects8[1] = tri[0].color[1];
                        v0_effects8[2] = tri[0].color[2];
                        v1_effects8[0] = tri[1].color[0];
                        v1_effects8[1] = tri[1].color[1];
                        v1_effects8[2] = tri[1].color[2];
                        v2_effects8[0] = tri[2].color[0];
                        v2_effects8[1] = tri[2].color[1];
                        v2_effects8[2] = tri[2].color[2];
                        v0_effects9[0] = tri[0].color[3];
                        v1_effects9[0] = tri[1].color[3];
                        v2_effects9[0] = tri[2].color[3];
                        self.verts.extend_from_slice(&[
                            Vertex {
                                pos: tri[0].pos,
                                uv: tri[0].uv,
                                uv_aux: [0.0, 0.0],
                                alpha,
                                effects1,
                                effects2,
                                effects3,
                                effects4,
                                effects5,
                                effects6,
                                effects7,
                                effects8: v0_effects8,
                                effects9: v0_effects9,
                                effects10,
                                effects11,
                                world_pos: zero4,
                                world_normal: [
                                    tri[0].normal[0],
                                    tri[0].normal[1],
                                    tri[0].normal[2],
                                    0.0,
                                ],
                                world_tangent: [
                                    tri[0].tangent[0],
                                    tri[0].tangent[1],
                                    tri[0].tangent[2],
                                    0.0,
                                ],
                                world_binormal: [
                                    tri[0].binormal[0],
                                    tri[0].binormal[1],
                                    tri[0].binormal[2],
                                    0.0,
                                ],
                                shadow_pos: zero4,
                                bone_indices: v0_bones,
                                bone_weights: tri[0].bone_weights,
                                light_pos_kind: light_pos_kind_base,
                                light_dir_shadow: light_dir_shadow_base,
                                light_atten: light_atten_base,
                                light_cone: light_cone_base,
                            },
                            Vertex {
                                pos: tri[1].pos,
                                uv: tri[1].uv,
                                uv_aux: [0.0, 0.0],
                                alpha,
                                effects1,
                                effects2,
                                effects3,
                                effects4,
                                effects5,
                                effects6,
                                effects7,
                                effects8: v1_effects8,
                                effects9: v1_effects9,
                                effects10,
                                effects11,
                                world_pos: zero4,
                                world_normal: [
                                    tri[1].normal[0],
                                    tri[1].normal[1],
                                    tri[1].normal[2],
                                    0.0,
                                ],
                                world_tangent: [
                                    tri[1].tangent[0],
                                    tri[1].tangent[1],
                                    tri[1].tangent[2],
                                    0.0,
                                ],
                                world_binormal: [
                                    tri[1].binormal[0],
                                    tri[1].binormal[1],
                                    tri[1].binormal[2],
                                    0.0,
                                ],
                                shadow_pos: zero4,
                                bone_indices: v1_bones,
                                bone_weights: tri[1].bone_weights,
                                light_pos_kind: light_pos_kind_base,
                                light_dir_shadow: light_dir_shadow_base,
                                light_atten: light_atten_base,
                                light_cone: light_cone_base,
                            },
                            Vertex {
                                pos: tri[2].pos,
                                uv: tri[2].uv,
                                uv_aux: [0.0, 0.0],
                                alpha,
                                effects1,
                                effects2,
                                effects3,
                                effects4,
                                effects5,
                                effects6,
                                effects7,
                                effects8: v2_effects8,
                                effects9: v2_effects9,
                                effects10,
                                effects11,
                                world_pos: zero4,
                                world_normal: [
                                    tri[2].normal[0],
                                    tri[2].normal[1],
                                    tri[2].normal[2],
                                    0.0,
                                ],
                                world_tangent: [
                                    tri[2].tangent[0],
                                    tri[2].tangent[1],
                                    tri[2].tangent[2],
                                    0.0,
                                ],
                                world_binormal: [
                                    tri[2].binormal[0],
                                    tri[2].binormal[1],
                                    tri[2].binormal[2],
                                    0.0,
                                ],
                                shadow_pos: zero4,
                                bone_indices: v2_bones,
                                bone_weights: tri[2].bone_weights,
                                light_pos_kind: light_pos_kind_base,
                                light_dir_shadow: light_dir_shadow_base,
                                light_atten: light_atten_base,
                                light_cone: light_cone_base,
                            },
                        ]);
                        added += 3;
                    }
                    if added != 0 {
                        self.draws.push(DrawCommand {
                            image_id: img_id,
                            emote_render_id: None,
                            mesh_texture_path: batch.texture_path.clone(),
                            mesh_normal_texture_path: batch.material.normal_texture_path.clone(),
                            mesh_toon_texture_path: batch.material.toon_texture_path.clone(),
                            mask_image_id: None,
                            tonecurve_image_id: if has_tonecurve {
                                sprite.tonecurve_image_id
                            } else {
                                None
                            },
                            fog_image_id: if has_fog_tex {
                                sprite.fog_texture_image_id
                            } else {
                                None
                            },
                            wipe_src_image_id: if has_wipe_src {
                                sprite.wipe_src_image_id
                            } else {
                                None
                            },
                            range: base..base + added,
                            scissor,
                            pipeline_key: batch_pipeline_key,
                            shadow_pipeline_name: Some(
                                resolved_shadow_pipeline_name_from_runtime_desc(
                                    &batch.runtime_desc,
                                ),
                            ),
                            draw_kind: batch_draw_kind,
                            mesh_material_key: mesh_material_key_for_batch(
                                sprite,
                                batch_technique.special,
                                &batch,
                            ),
                            shadow_cast: sprite.shadow_cast
                                && use_depth
                                && sprite.light_cone[3] > 0.5
                                && batch.material.shadow_map_enable,
                            vs_uniform,
                            bone_uniform,
                        });
                    }
                }
                continue;
            }
            if img.is_none() && emote_render_id.is_none() {
                continue;
            }
            let source_width_f = source_width.max(1) as f32;
            let source_height_f = source_height.max(1) as f32;
            let (u0, v0, u1, v1) = (
                (src_left / source_width_f).clamp(0.0, 1.0),
                (src_top / source_height_f).clamp(0.0, 1.0),
                (src_right / source_width_f).clamp(0.0, 1.0),
                (src_bottom / source_height_f).clamp(0.0, 1.0),
            );
            let mask_uv = if let Some(mask_id) = sprite.mask_image_id {
                if let Some(mask_img) = images.get(mask_id) {
                    let mw = mask_img.width.max(1) as f32;
                    let mh = mask_img.height.max(1) as f32;
                    [
                        [
                            (src_left + sprite.mask_offset_x as f32) / mw,
                            (src_top + sprite.mask_offset_y as f32) / mh,
                        ],
                        [
                            (src_right + sprite.mask_offset_x as f32) / mw,
                            (src_top + sprite.mask_offset_y as f32) / mh,
                        ],
                        [
                            (src_right + sprite.mask_offset_x as f32) / mw,
                            (src_bottom + sprite.mask_offset_y as f32) / mh,
                        ],
                        [
                            (src_left + sprite.mask_offset_x as f32) / mw,
                            (src_bottom + sprite.mask_offset_y as f32) / mh,
                        ],
                    ]
                } else {
                    [[0.0, 0.0]; 4]
                }
            } else {
                [[0.0, 0.0]; 4]
            };

            let Some([p0, p1, p2, p3]) =
                sprite_quad_points(sprite, dst_x, dst_y, dst_w, dst_h, win_w, win_h)
            else {
                continue;
            };
            let base = self.verts.len() as u32;
            let (x0, y0, z0) = pixel_to_ndc(p0.x, p0.y, p0.depth, win_w, win_h);
            let (x1, y1, z1) = pixel_to_ndc(p1.x, p1.y, p1.depth, win_w, win_h);
            let (x2, y2, z2) = pixel_to_ndc(p2.x, p2.y, p2.depth, win_w, win_h);
            let (x3, y3, z3) = pixel_to_ndc(p3.x, p3.y, p3.depth, win_w, win_h);
            self.verts.extend_from_slice(&[
                Vertex {
                    pos: [x0, y0, z0],
                    uv: [u0, v0],
                    uv_aux: mask_uv[0],
                    alpha,
                    effects1,
                    effects2,
                    effects3,
                    effects4,
                    effects5,
                    effects6,
                    effects7,
                    effects8,
                    effects9,
                    effects10,
                    effects11,
                    world_pos: zero4,
                    world_normal: zero4,
                    world_tangent: zero4,
                    world_binormal: zero4,
                    shadow_pos: zero4,
                    bone_indices: zero4,
                    bone_weights: zero4,
                    light_pos_kind: light_pos_kind_base,
                    light_dir_shadow: light_dir_shadow_base,
                    light_atten: light_atten_base,
                    light_cone: light_cone_base,
                },
                Vertex {
                    pos: [x1, y1, z1],
                    uv: [u1, v0],
                    uv_aux: mask_uv[1],
                    alpha,
                    effects1,
                    effects2,
                    effects3,
                    effects4,
                    effects5,
                    effects6,
                    effects7,
                    effects8,
                    effects9,
                    effects10,
                    effects11,
                    world_pos: zero4,
                    world_normal: zero4,
                    world_tangent: zero4,
                    world_binormal: zero4,
                    shadow_pos: zero4,
                    bone_indices: zero4,
                    bone_weights: zero4,
                    light_pos_kind: light_pos_kind_base,
                    light_dir_shadow: light_dir_shadow_base,
                    light_atten: light_atten_base,
                    light_cone: light_cone_base,
                },
                Vertex {
                    pos: [x2, y2, z2],
                    uv: [u1, v1],
                    uv_aux: mask_uv[2],
                    alpha,
                    effects1,
                    effects2,
                    effects3,
                    effects4,
                    effects5,
                    effects6,
                    effects7,
                    effects8,
                    effects9,
                    effects10,
                    effects11,
                    world_pos: zero4,
                    world_normal: zero4,
                    world_tangent: zero4,
                    world_binormal: zero4,
                    shadow_pos: zero4,
                    bone_indices: zero4,
                    bone_weights: zero4,
                    light_pos_kind: light_pos_kind_base,
                    light_dir_shadow: light_dir_shadow_base,
                    light_atten: light_atten_base,
                    light_cone: light_cone_base,
                },
                Vertex {
                    pos: [x0, y0, z0],
                    uv: [u0, v0],
                    uv_aux: mask_uv[0],
                    alpha,
                    effects1,
                    effects2,
                    effects3,
                    effects4,
                    effects5,
                    effects6,
                    effects7,
                    effects8,
                    effects9,
                    effects10,
                    effects11,
                    world_pos: zero4,
                    world_normal: zero4,
                    world_tangent: zero4,
                    world_binormal: zero4,
                    shadow_pos: zero4,
                    bone_indices: zero4,
                    bone_weights: zero4,
                    light_pos_kind: light_pos_kind_base,
                    light_dir_shadow: light_dir_shadow_base,
                    light_atten: light_atten_base,
                    light_cone: light_cone_base,
                },
                Vertex {
                    pos: [x2, y2, z2],
                    uv: [u1, v1],
                    uv_aux: mask_uv[2],
                    alpha,
                    effects1,
                    effects2,
                    effects3,
                    effects4,
                    effects5,
                    effects6,
                    effects7,
                    effects8,
                    effects9,
                    effects10,
                    effects11,
                    world_pos: zero4,
                    world_normal: zero4,
                    world_tangent: zero4,
                    world_binormal: zero4,
                    shadow_pos: zero4,
                    bone_indices: zero4,
                    bone_weights: zero4,
                    light_pos_kind: light_pos_kind_base,
                    light_dir_shadow: light_dir_shadow_base,
                    light_atten: light_atten_base,
                    light_cone: light_cone_base,
                },
                Vertex {
                    pos: [x3, y3, z3],
                    uv: [u0, v1],
                    uv_aux: mask_uv[3],
                    alpha,
                    effects1,
                    effects2,
                    effects3,
                    effects4,
                    effects5,
                    effects6,
                    effects7,
                    effects8,
                    effects9,
                    effects10,
                    effects11,
                    world_pos: zero4,
                    world_normal: zero4,
                    world_tangent: zero4,
                    world_binormal: zero4,
                    shadow_pos: zero4,
                    bone_indices: zero4,
                    bone_weights: zero4,
                    light_pos_kind: light_pos_kind_base,
                    light_dir_shadow: light_dir_shadow_base,
                    light_atten: light_atten_base,
                    light_cone: light_cone_base,
                },
            ]);
            let sprite_vs_uniform = sprite2d_uniform_for_effects(
                win_w, win_h, effects1, effects2, effects3, effects4, effects5, effects6, effects7,
                effects8, effects9, effects10, effects11,
            );

            self.draws.push(DrawCommand {
                image_id: img_id,
                emote_render_id,
                mesh_texture_path: None,
                mesh_normal_texture_path: None,
                mesh_toon_texture_path: None,
                mask_image_id: if has_mask { sprite.mask_image_id } else { None },
                tonecurve_image_id: if has_tonecurve {
                    sprite.tonecurve_image_id
                } else {
                    None
                },
                fog_image_id: if has_fog_tex {
                    sprite.fog_texture_image_id
                } else {
                    None
                },
                wipe_src_image_id: if has_wipe_src {
                    sprite.wipe_src_image_id
                } else {
                    None
                },
                range: base..base + 6,
                scissor,
                pipeline_key,
                shadow_pipeline_name: None,
                draw_kind,
                mesh_material_key: mesh_material_key_for_sprite(sprite, technique.special),
                shadow_cast: sprite.shadow_cast && use_depth,
                vs_uniform: sprite_vs_uniform,
                bone_uniform: BoneUniform::zero(),
            });
        }

        let blit_range = append_fullscreen_blit_vertices(&mut self.verts);

        self.ensure_vertex_capacity(self.verts.len())?;
        self.queue
            .write_buffer(&self.vertex_buf, 0, bytemuck::cast_slice(&self.verts));
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            let sprite2d_verts: Vec<VertexSprite2dData> = self
                .verts
                .iter()
                .copied()
                .map(VertexSprite2dData::from)
                .collect();
            self.queue.write_buffer(
                &self.vertex_sprite2d_buf,
                0,
                bytemuck::cast_slice(&sprite2d_verts),
            );
        }

        let mut live_image_ids = HashSet::new();
        for cmd in &self.draws {
            if let Some(id) = cmd.image_id {
                live_image_ids.insert(id);
            }
            if let Some(id) = cmd.mask_image_id {
                live_image_ids.insert(id);
            }
            if let Some(id) = cmd.tonecurve_image_id {
                live_image_ids.insert(id);
            }
            if let Some(id) = cmd.fog_image_id {
                live_image_ids.insert(id);
            }
            if let Some(id) = cmd.wipe_src_image_id {
                live_image_ids.insert(id);
            }
        }
        for id in live_image_ids.iter().copied() {
            self.ensure_texture_uploaded(images, id)?;
        }
        // Runtime ImageIds remain stable until scene restart. Keep uploaded
        // textures resident for that lifetime instead of evicting everything
        // not referenced by the current frame. PATNO/animation-heavy games
        // otherwise bounce the same textures through create/upload every frame.
        // Scene restart explicitly calls clear_runtime_image_textures().

        let pipeline_requests: Vec<(PipelineKey, Option<PipelineKey>)> = self
            .draws
            .iter()
            .map(|cmd| {
                let shadow = cmd.shadow_cast.then(|| {
                    shadow_pipeline_key(
                        cmd.pipeline_key.clone(),
                        cmd.shadow_pipeline_name.as_deref(),
                    )
                });
                (cmd.pipeline_key.clone(), shadow)
            })
            .collect();
        for (pipeline_key, shadow_key) in pipeline_requests {
            self.ensure_pipeline(pipeline_key);
            if let Some(shadow_key) = shadow_key {
                self.ensure_pipeline(shadow_key);
            }
        }
        self.ensure_pipeline(PipelineKey {
            technique: TechniqueKey {
                d3: false,
                light: false,
                fog: false,
                tex: 1,
                diffuse: false,
                mrbd: false,
                rgb: false,
                tonecurve: false,
                mask: false,
                special: TechniqueSpecial::None,
            },
            blend: SpriteBlend::Normal,
            alpha_blend: false,
            use_depth: false,
            depth_attachment: false,
            cull_back: false,
            mesh_fx_variant: 0,
            pipeline_name: String::new(),
            program: EffectProgram::Sprite2D,
        });

        Ok(blit_range)
    }

    fn prepare_blit_vertices(&mut self) -> Result<std::ops::Range<u32>> {
        self.verts.clear();
        self.draws.clear();
        let range = append_fullscreen_blit_vertices(&mut self.verts);
        self.ensure_vertex_capacity(self.verts.len())?;
        self.queue
            .write_buffer(&self.vertex_buf, 0, bytemuck::cast_slice(&self.verts));
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            let sprite2d_verts: Vec<VertexSprite2dData> = self
                .verts
                .iter()
                .copied()
                .map(VertexSprite2dData::from)
                .collect();
            self.queue.write_buffer(
                &self.vertex_sprite2d_buf,
                0,
                bytemuck::cast_slice(&sprite2d_verts),
            );
        }
        self.ensure_pipeline(PipelineKey {
            technique: TechniqueKey {
                d3: false,
                light: false,
                fog: false,
                tex: 1,
                diffuse: false,
                mrbd: false,
                rgb: false,
                tonecurve: false,
                mask: false,
                special: TechniqueSpecial::None,
            },
            blend: SpriteBlend::Normal,
            alpha_blend: false,
            use_depth: false,
            depth_attachment: false,
            cull_back: false,
            mesh_fx_variant: 0,
            pipeline_name: String::new(),
            program: EffectProgram::Sprite2D,
        });
        Ok(range)
    }

    fn render_frame_to_internal(
        &mut self,
        images: &ImageManager,
        frame: &RenderFrame,
    ) -> Result<BackdropTarget> {
        if let Some(wipe) = frame.wipe.as_ref() {
            let current = self.render_sprite_list_to_scene_pair(images, &wipe.current, None)?;
            self.copy_internal_target(backdrop_to_internal(current), InternalColorTarget::WipeA);
            let next = self.render_sprite_list_to_scene_pair(images, &wipe.next, None)?;
            self.copy_internal_target(backdrop_to_internal(next), InternalColorTarget::WipeB);
            let under = self.render_sprite_list_to_scene_pair(images, &wipe.under, None)?;
            let composed = self.render_wipe_composite(images, wipe, under)?;
            self.render_sprite_list_to_scene_pair(images, &wipe.over, Some(composed))
        } else {
            self.render_sprite_list_to_scene_pair(images, &frame.sprites, None)
        }
    }

    fn render_sprite_list_to_scene_pair(
        &mut self,
        images: &ImageManager,
        sprites: &[RenderSprite],
        initial: Option<BackdropTarget>,
    ) -> Result<BackdropTarget> {
        let blit_range = self.prepare_draws(images, sprites, false)?;
        let draws_for_pass = self.draws.clone();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("siglus-offscreen-scene-encoder"),
            });

        let shadow_indices: Vec<usize> = draws_for_pass
            .iter()
            .enumerate()
            .filter_map(|(idx, cmd)| cmd.shadow_cast.then_some(idx))
            .collect();
        if !shadow_indices.is_empty() {
            self.render_command_slice(
                &mut encoder,
                ColorTarget::Internal(InternalColorTarget::ShadowMap),
                DepthTarget::Shadow,
                0..0,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                true,
                None,
                None,
            )?;
            for idx in shadow_indices {
                self.render_command_slice(
                    &mut encoder,
                    ColorTarget::Internal(InternalColorTarget::ShadowMap),
                    DepthTarget::Shadow,
                    idx..idx + 1,
                    wgpu::LoadOp::Load,
                    false,
                    None,
                    Some(TechniqueSpecial::Shadow),
                )?;
            }
        }

        let mut current = initial.unwrap_or(BackdropTarget::SceneA);
        let initial_color_load = if initial.is_some() {
            wgpu::LoadOp::Load
        } else {
            wgpu::LoadOp::Clear(wgpu::Color::BLACK)
        };
        self.render_command_slice(
            &mut encoder,
            ColorTarget::Internal(backdrop_to_internal(current)),
            DepthTarget::Main,
            0..0,
            initial_color_load,
            true,
            None,
            None,
        )?;

        let mut index = 0usize;
        while index < draws_for_pass.len() {
            let is_overlay = matches!(
                draws_for_pass[index].pipeline_key.technique.special,
                TechniqueSpecial::Overlay
            );
            let start = index;
            while index < draws_for_pass.len()
                && matches!(
                    draws_for_pass[index].pipeline_key.technique.special,
                    TechniqueSpecial::Overlay
                ) == is_overlay
            {
                index += 1;
            }
            if is_overlay {
                let dst = opposite_backdrop(current);
                self.render_copy_pass(
                    &mut encoder,
                    ColorTarget::Internal(backdrop_to_internal(dst)),
                    current,
                    blit_range.clone(),
                )?;
                self.render_command_slice(
                    &mut encoder,
                    ColorTarget::Internal(backdrop_to_internal(dst)),
                    DepthTarget::Main,
                    start..index,
                    wgpu::LoadOp::Load,
                    false,
                    Some(current),
                    None,
                )?;
                current = dst;
            } else {
                self.render_command_slice(
                    &mut encoder,
                    ColorTarget::Internal(backdrop_to_internal(current)),
                    DepthTarget::Main,
                    start..index,
                    wgpu::LoadOp::Load,
                    false,
                    None,
                    None,
                )?;
            }
        }
        self.queue.submit(Some(encoder.finish()));
        Ok(current)
    }

    fn copy_internal_target(&self, src: InternalColorTarget, dst: InternalColorTarget) {
        let src_tex = self.internal_target_ref(src);
        let dst_tex = self.internal_target_ref(dst);
        let width = src_tex.width.min(dst_tex.width);
        let height = src_tex.height.min(dst_tex.height);
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("siglus-copy-internal-target"),
            });
        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture {
                texture: &src_tex._tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyTexture {
                texture: &dst_tex._tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));
    }

    fn ensure_generated_wipe_mask(&mut self, wipe: &WipeRenderPlan) -> Result<()> {
        let width = self.logical_width.max(1.0).round() as u32;
        let height = self.logical_height.max(1.0).round() as u32;
        let key = WipeMaskCacheKey {
            wipe_type: wipe.wipe_type,
            option: wipe.option.clone(),
            width,
            height,
            seed: wipe.random_seed,
        };
        if self
            .wipe_mask_cache
            .as_ref()
            .is_some_and(|(cached, _)| cached == &key)
        {
            return Ok(());
        }
        let Some(gray) = crate::runtime::wipe_mask::generate(
            wipe.wipe_type,
            &wipe.option,
            width,
            height,
            wipe.random_seed,
        ) else {
            self.wipe_mask_cache = None;
            return Ok(());
        };
        let mut rgba = Vec::with_capacity(gray.pixels.len() * 4);
        for value in gray.pixels {
            rgba.extend_from_slice(&[value, value, value, 255]);
        }
        let image = crate::assets::RgbaImage {
            width: gray.width,
            height: gray.height,
            center_x: 0,
            center_y: 0,
            rgba,
        };
        let texture = create_gpu_texture(
            &self.device,
            &self.queue,
            "siglus-generated-wipe-mask",
            &image,
            wipe.random_seed as u64,
        )?;
        self.wipe_mask_cache = Some((key, texture));
        Ok(())
    }

    fn render_wipe_composite(
        &mut self,
        images: &ImageManager,
        wipe: &WipeRenderPlan,
        under: BackdropTarget,
    ) -> Result<BackdropTarget> {
        if matches!(wipe.wipe_type, 300 | 301) {
            return self.render_page_wipe(wipe, under);
        }

        if let Some(id) = wipe.mask_image_id {
            self.ensure_texture_uploaded(images, id)?;
        } else {
            self.ensure_generated_wipe_mask(wipe)?;
        }

        let mut values = [0.0f32; 16];
        for (dst, src) in values.iter_mut().zip(wipe.option.iter().copied()) {
            *dst = src as f32;
        }
        // Types 242/243 choose their random explosion center at wipe start.
        // The seed is stable for the wipe lifetime but changes on the next WIPE.
        values[15] = wipe.random_seed as f32;
        let uniform = WipeUniform {
            kind_progress: [
                wipe.wipe_type as f32,
                wipe.progress.clamp(0.0, 1.0),
                self.logical_width.max(1.0),
                self.logical_height.max(1.0),
            ],
            option0: values[0..4].try_into().expect("four wipe options"),
            option1: values[4..8].try_into().expect("four wipe options"),
            option2: values[8..12].try_into().expect("four wipe options"),
            option3: values[12..16].try_into().expect("four wipe options"),
        };
        let uniform_buffer = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("siglus-wipe-uniform"),
                contents: bytemuck::bytes_of(&uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });

        let under_texture = self.backdrop_target_ref(under);
        let current_texture = &self.wipe_a;
        let next_texture = &self.wipe_b;
        let external_mask = wipe.mask_image_id.and_then(|id| self.textures.get(&id));
        let generated_mask = self.wipe_mask_cache.as_ref().map(|(_, texture)| texture);
        let mask_texture = external_mask
            .or(generated_mask)
            .unwrap_or(&self.default_aux);
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("siglus-wipe-bind-group"),
            layout: &self.wipe_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&under_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&under_texture.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&current_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&current_texture.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&next_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&next_texture.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&mask_texture.view),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Sampler(&mask_texture.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: uniform_buffer.as_entire_binding(),
                },
            ],
        });

        let target = opposite_backdrop(under);
        let target_view = &self.backdrop_target_ref(target).view;
        let viewport = SurfaceViewport::full(
            self.logical_width.max(1.0).round() as u32,
            self.logical_height.max(1.0).round() as u32,
        );
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("siglus-wipe-composite-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("siglus-wipe-composite-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_viewport(
                viewport.x as f32,
                viewport.y as f32,
                viewport.w as f32,
                viewport.h as f32,
                0.0,
                1.0,
            );
            pass.set_pipeline(&self.wipe_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.draw(0..3, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        Ok(target)
    }

    fn render_page_wipe(
        &mut self,
        wipe: &WipeRenderPlan,
        under: BackdropTarget,
    ) -> Result<BackdropTarget> {
        // Page wipes use the same GPU source render targets and preserve the
        // original perspective/culling branch.  The page geometry itself is
        // emitted as regular 3D sprite quads, never rasterized on the CPU.
        let target = opposite_backdrop(under);
        self.copy_internal_target(backdrop_to_internal(under), backdrop_to_internal(target));
        self.render_page_wipe_geometry(wipe, target)?;
        Ok(target)
    }

    fn render_page_wipe_geometry(
        &mut self,
        wipe: &WipeRenderPlan,
        target: BackdropTarget,
    ) -> Result<()> {
        let draws = build_page_wipe_draws(
            wipe,
            self.logical_width.max(1.0),
            self.logical_height.max(1.0),
        );
        if draws.is_empty() {
            return Ok(());
        }

        let mut buffers = Vec::with_capacity(draws.len());
        let mut bind_groups = Vec::with_capacity(draws.len());
        for draw in &draws {
            let buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("siglus-page-wipe-vertex-buffer"),
                    contents: bytemuck::cast_slice(&draw.vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                });
            let texture = if draw.use_current {
                &self.wipe_a
            } else {
                &self.wipe_b
            };
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("siglus-page-wipe-bind-group"),
                layout: &self.page_wipe_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&texture.sampler),
                    },
                ],
            });
            buffers.push(buffer);
            bind_groups.push(bind_group);
        }

        let target_view = &self.backdrop_target_ref(target).view;
        let viewport = SurfaceViewport::full(
            self.logical_width.max(1.0).round() as u32,
            self.logical_height.max(1.0).round() as u32,
        );
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("siglus-page-wipe-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("siglus-page-wipe-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth.view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_viewport(
                viewport.x as f32,
                viewport.y as f32,
                viewport.w as f32,
                viewport.h as f32,
                0.0,
                1.0,
            );
            pass.set_pipeline(&self.page_wipe_pipeline);
            for (index, draw) in draws.iter().enumerate() {
                pass.set_bind_group(0, &bind_groups[index], &[]);
                pass.set_vertex_buffer(0, buffers[index].slice(..));
                pass.draw(0..draw.vertices.len() as u32, 0..1);
            }
        }
        self.queue.submit(Some(encoder.finish()));
        Ok(())
    }

    pub fn debug_read_render_chain_textures(&self) -> Result<Vec<RendererDebugTexture>> {
        let mut pending: HashMap<RendererDebugTextureKey, PendingRendererDebugTexture> =
            HashMap::new();

        for (draw_idx, cmd) in self.draws.iter().enumerate() {
            let role_prefix = format!("draw[{draw_idx}]");
            self.debug_add_base_texture_usage(&mut pending, cmd, &format!("{role_prefix}.base"));
            self.debug_add_image_texture_usage(
                &mut pending,
                cmd.mask_image_id,
                "image",
                &format!("{role_prefix}.mask"),
            );
            self.debug_add_image_texture_usage(
                &mut pending,
                cmd.tonecurve_image_id,
                "image",
                &format!("{role_prefix}.tonecurve"),
            );
            self.debug_add_image_texture_usage(
                &mut pending,
                cmd.fog_image_id,
                "image",
                &format!("{role_prefix}.fog"),
            );
            self.debug_add_aux_texture_usage(&mut pending, cmd, &format!("{role_prefix}.aux"));
            self.debug_add_external_texture_usage(
                &mut pending,
                cmd.mesh_normal_texture_path.as_deref(),
                "external",
                &format!("{role_prefix}.normal"),
            );
            self.debug_add_external_texture_usage(
                &mut pending,
                cmd.mesh_toon_texture_path.as_deref(),
                "external",
                &format!("{role_prefix}.toon"),
            );
            if cmd.pipeline_key.use_depth
                || cmd.shadow_cast
                || cmd.mesh_material_key.as_ref().is_some_and(|k| k.shadow)
            {
                self.debug_add_render_target_usage(
                    &mut pending,
                    RendererDebugRenderTarget::ShadowMap,
                    &format!("{role_prefix}.shadow"),
                );
            }
        }

        if self.draws.iter().any(|cmd| {
            matches!(
                cmd.pipeline_key.technique.special,
                TechniqueSpecial::Overlay
            )
        }) {
            self.debug_add_render_target_usage(
                &mut pending,
                RendererDebugRenderTarget::SceneA,
                "overlay.backdrop.scene_a",
            );
            self.debug_add_render_target_usage(
                &mut pending,
                RendererDebugRenderTarget::SceneB,
                "overlay.backdrop.scene_b",
            );
        }
        if self.draws.is_empty() {
            self.debug_add_default_aux_usage(&mut pending, "empty-frame.default_aux");
        }

        let mut items = Vec::with_capacity(pending.len());
        for (key, meta) in pending.into_iter() {
            let Some((width, height, version, rgba)) = self.debug_read_texture_by_key(&key)? else {
                continue;
            };
            let key_string = Self::debug_texture_key_string(&key);
            items.push((
                meta.order,
                RendererDebugTexture {
                    key: key_string,
                    kind: meta.kind,
                    label: meta.label,
                    usage: meta.usage.join("; "),
                    usage_count: meta.usage.len(),
                    width,
                    height,
                    version,
                    rgba,
                },
            ));
        }
        items.sort_by_key(|(order, _)| *order);
        Ok(items.into_iter().map(|(_, item)| item).collect())
    }

    fn debug_add_pending_texture_usage(
        &self,
        pending: &mut HashMap<RendererDebugTextureKey, PendingRendererDebugTexture>,
        key: RendererDebugTextureKey,
        kind: &str,
        label: String,
        width: u32,
        height: u32,
        version: u64,
        usage: &str,
    ) {
        let order = pending.len();
        let entry = pending
            .entry(key)
            .or_insert_with(|| PendingRendererDebugTexture {
                order,
                kind: kind.to_string(),
                label,
                usage: Vec::new(),
                width,
                height,
                version,
            });
        if !entry.usage.iter().any(|s| s == usage) {
            entry.usage.push(usage.to_string());
        }
    }

    fn debug_add_default_aux_usage(
        &self,
        pending: &mut HashMap<RendererDebugTextureKey, PendingRendererDebugTexture>,
        usage: &str,
    ) {
        self.debug_add_pending_texture_usage(
            pending,
            RendererDebugTextureKey::DefaultAux,
            "default",
            "default_aux".to_string(),
            self.default_aux.width,
            self.default_aux.height,
            self.default_aux.version,
            usage,
        );
    }

    fn debug_add_image_texture_usage(
        &self,
        pending: &mut HashMap<RendererDebugTextureKey, PendingRendererDebugTexture>,
        image_id: Option<ImageId>,
        kind: &str,
        usage: &str,
    ) {
        if let Some(id) = image_id {
            if let Some(tex) = self.textures.get(&id) {
                self.debug_add_pending_texture_usage(
                    pending,
                    RendererDebugTextureKey::Image(id),
                    kind,
                    format!("ImageId({})", id.index()),
                    tex.width,
                    tex.height,
                    tex.version,
                    usage,
                );
                return;
            }
        }
        self.debug_add_default_aux_usage(pending, usage);
    }

    fn debug_add_external_texture_usage(
        &self,
        pending: &mut HashMap<RendererDebugTextureKey, PendingRendererDebugTexture>,
        path: Option<&Path>,
        kind: &str,
        usage: &str,
    ) {
        if let Some(path) = path {
            if let Some(tex) = self.external_textures.get(path) {
                self.debug_add_pending_texture_usage(
                    pending,
                    RendererDebugTextureKey::External(path.to_path_buf()),
                    kind,
                    path.display().to_string(),
                    tex.width,
                    tex.height,
                    tex.version,
                    usage,
                );
                return;
            }
        }
        self.debug_add_default_aux_usage(pending, usage);
    }

    fn debug_add_render_target_usage(
        &self,
        pending: &mut HashMap<RendererDebugTextureKey, PendingRendererDebugTexture>,
        target: RendererDebugRenderTarget,
        usage: &str,
    ) {
        let rt = self.debug_render_target_ref(target);
        self.debug_add_pending_texture_usage(
            pending,
            RendererDebugTextureKey::RenderTarget(target),
            "render-target",
            match target {
                RendererDebugRenderTarget::SceneA => "scene_a".to_string(),
                RendererDebugRenderTarget::SceneB => "scene_b".to_string(),
                RendererDebugRenderTarget::ShadowMap => "shadow_map".to_string(),
            },
            rt.width,
            rt.height,
            self.debug_frame_serial,
            usage,
        );
    }

    fn debug_add_base_texture_usage(
        &self,
        pending: &mut HashMap<RendererDebugTextureKey, PendingRendererDebugTexture>,
        cmd: &DrawCommand,
        usage: &str,
    ) {
        if let Some(path) = cmd.mesh_texture_path.as_deref() {
            if let Some(tex) = self.external_textures.get(path) {
                self.debug_add_pending_texture_usage(
                    pending,
                    RendererDebugTextureKey::External(path.to_path_buf()),
                    "external",
                    path.display().to_string(),
                    tex.width,
                    tex.height,
                    tex.version,
                    usage,
                );
                return;
            }
        }
        self.debug_add_image_texture_usage(pending, cmd.image_id, "image", usage);
    }

    fn debug_add_aux_texture_usage(
        &self,
        pending: &mut HashMap<RendererDebugTextureKey, PendingRendererDebugTexture>,
        cmd: &DrawCommand,
        usage: &str,
    ) {
        if matches!(
            cmd.pipeline_key.technique.special,
            TechniqueSpecial::Overlay
        ) {
            self.debug_add_render_target_usage(pending, RendererDebugRenderTarget::SceneA, usage);
            self.debug_add_render_target_usage(pending, RendererDebugRenderTarget::SceneB, usage);
            return;
        }
        self.debug_add_image_texture_usage(pending, cmd.wipe_src_image_id, "image", usage);
    }

    fn debug_render_target_ref(&self, target: RendererDebugRenderTarget) -> &RenderTargetTexture {
        match target {
            RendererDebugRenderTarget::SceneA => &self.scene_a,
            RendererDebugRenderTarget::SceneB => &self.scene_b,
            RendererDebugRenderTarget::ShadowMap => &self.shadow_map,
        }
    }

    fn debug_texture_key_string(key: &RendererDebugTextureKey) -> String {
        match key {
            RendererDebugTextureKey::DefaultAux => "default_aux".to_string(),
            RendererDebugTextureKey::Image(id) => format!("image:{}", id.index()),
            RendererDebugTextureKey::External(path) => format!("external:{}", path.display()),
            RendererDebugTextureKey::RenderTarget(RendererDebugRenderTarget::SceneA) => {
                "render-target:scene_a".to_string()
            }
            RendererDebugTextureKey::RenderTarget(RendererDebugRenderTarget::SceneB) => {
                "render-target:scene_b".to_string()
            }
            RendererDebugTextureKey::RenderTarget(RendererDebugRenderTarget::ShadowMap) => {
                "render-target:shadow_map".to_string()
            }
        }
    }

    fn debug_read_texture_by_key(
        &self,
        key: &RendererDebugTextureKey,
    ) -> Result<Option<(u32, u32, u64, Vec<u8>)>> {
        match key {
            RendererDebugTextureKey::DefaultAux => Ok(Some((
                self.default_aux.width,
                self.default_aux.height,
                self.default_aux.version,
                self.debug_read_texture_rgba(
                    &self.default_aux._tex,
                    self.default_aux.width,
                    self.default_aux.height,
                    wgpu::TextureFormat::Rgba8Unorm,
                )?,
            ))),
            RendererDebugTextureKey::Image(id) => {
                let Some(tex) = self.textures.get(id) else {
                    return Ok(None);
                };
                Ok(Some((
                    tex.width,
                    tex.height,
                    tex.version,
                    self.debug_read_texture_rgba(
                        &tex._tex,
                        tex.width,
                        tex.height,
                        wgpu::TextureFormat::Rgba8Unorm,
                    )?,
                )))
            }
            RendererDebugTextureKey::External(path) => {
                let Some(tex) = self.external_textures.get(path) else {
                    return Ok(None);
                };
                Ok(Some((
                    tex.width,
                    tex.height,
                    tex.version,
                    self.debug_read_texture_rgba(
                        &tex._tex,
                        tex.width,
                        tex.height,
                        wgpu::TextureFormat::Rgba8Unorm,
                    )?,
                )))
            }
            RendererDebugTextureKey::RenderTarget(target) => {
                let rt = self.debug_render_target_ref(*target);
                Ok(Some((
                    rt.width,
                    rt.height,
                    self.debug_frame_serial,
                    self.debug_read_texture_rgba(&rt._tex, rt.width, rt.height, rt.format)?,
                )))
            }
        }
    }

    fn debug_read_texture_rgba(
        &self,
        texture: &wgpu::Texture,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> Result<Vec<u8>> {
        if width == 0 || height == 0 {
            return Ok(Vec::new());
        }
        let bytes_per_pixel = 4u32;
        let unpadded_bytes_per_row = width.saturating_mul(bytes_per_pixel);
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let padded_bytes_per_row = ((unpadded_bytes_per_row + align - 1) / align) * align;
        let output_buffer_size = padded_bytes_per_row as u64 * height as u64;
        let output_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("siglus-debug-texture-readback"),
            size: output_buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("siglus-debug-texture-readback-encoder"),
            });
        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &output_buffer,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit(Some(encoder.finish()));

        let buffer_slice = output_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .context("wait for debug texture readback")?
            .context("map debug texture readback")?;
        let data = buffer_slice.get_mapped_range();
        let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
        for y in 0..height as usize {
            let src_offset = y * padded_bytes_per_row as usize;
            let dst_offset = y * unpadded_bytes_per_row as usize;
            let src = &data[src_offset..src_offset + unpadded_bytes_per_row as usize];
            let dst = &mut rgba[dst_offset..dst_offset + unpadded_bytes_per_row as usize];
            match format {
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
                    for (src_px, dst_px) in src.chunks_exact(4).zip(dst.chunks_exact_mut(4)) {
                        dst_px[0] = src_px[2];
                        dst_px[1] = src_px[1];
                        dst_px[2] = src_px[0];
                        dst_px[3] = src_px[3];
                    }
                }
                wgpu::TextureFormat::Rgba8Unorm | wgpu::TextureFormat::Rgba8UnormSrgb => {
                    dst.copy_from_slice(src);
                }
                other => {
                    anyhow::bail!("unsupported debug texture readback format: {other:?}");
                }
            }
        }
        drop(data);
        output_buffer.unmap();
        Ok(rgba)
    }

    fn ensure_mesh_asset(&mut self, images: &ImageManager, file_name: &str) -> Option<MeshAsset> {
        if let Some(asset) = self.mesh_assets.get(file_name) {
            return Some(asset.clone());
        }
        let asset =
            load_mesh_asset(images.project_dir(), images.current_append_dir(), file_name).ok()?;
        self.mesh_assets
            .insert(file_name.to_string(), asset.clone());
        Some(asset)
    }

    fn ensure_external_texture(&mut self, path: &Path) -> Option<()> {
        if self.external_textures.contains_key(path) {
            return Some(());
        }
        let img = load_image_any(path, 0).ok()?;
        let tex = create_gpu_texture(
            &self.device,
            &self.queue,
            &format!("siglus-external-texture-{}", self.external_textures.len()),
            &img,
            0,
        )
        .ok()?;
        self.external_textures.insert(path.to_path_buf(), tex);
        Some(())
    }

    fn ensure_pipeline(&mut self, key: PipelineKey) {
        if self.pipelines.contains_key(&key) {
            return;
        }
        let blend_state = if !key.alpha_blend {
            None
        } else {
            Some(match key.blend {
                SpriteBlend::Normal => wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                },
                SpriteBlend::Add => wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                },
                SpriteBlend::Sub => wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::ReverseSubtract,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                },
                SpriteBlend::Mul => wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::Zero,
                        dst_factor: wgpu::BlendFactor::Src,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                },
                SpriteBlend::Screen => wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrc,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                },
                SpriteBlend::Overlay => wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent::OVER,
                },
            })
        };

        let pipeline_label = format!("siglus-{}", technique_name_for_pipeline(&key));
        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(pipeline_label.as_str()),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: key.program.vertex_entry(),
                    buffers: &[if key.program.uses_sprite2d_layout() {
                        VertexSprite2d::layout()
                    } else {
                        Vertex::layout()
                    }],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: key.program.fragment_entry(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: self.config.format,
                        blend: blend_state,
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    strip_index_format: None,
                    front_face: wgpu::FrontFace::Ccw,
                    cull_mode: if key.cull_back {
                        Some(wgpu::Face::Back)
                    } else {
                        None
                    },
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: key.depth_attachment.then_some(wgpu::DepthStencilState {
                    format: if matches!(
                        key.program,
                        EffectProgram::ShadowStatic | EffectProgram::ShadowSkinned
                    ) {
                        wgpu::TextureFormat::Depth16Unorm
                    } else {
                        wgpu::TextureFormat::Depth32Float
                    },
                    depth_write_enabled: key.use_depth,
                    depth_compare: if key.use_depth {
                        wgpu::CompareFunction::LessEqual
                    } else {
                        wgpu::CompareFunction::Always
                    },
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });
        self.pipelines.insert(key, pipeline);
    }

    fn internal_target_ref(&self, target: InternalColorTarget) -> &RenderTargetTexture {
        match target {
            InternalColorTarget::SceneA => &self.scene_a,
            InternalColorTarget::SceneB => &self.scene_b,
            InternalColorTarget::WipeA => &self.wipe_a,
            InternalColorTarget::WipeB => &self.wipe_b,
            InternalColorTarget::ShadowMap => &self.shadow_map,
        }
    }

    fn color_target_view<'a>(&'a self, target: ColorTarget<'a>) -> &'a wgpu::TextureView {
        match target {
            ColorTarget::External(view) => view,
            ColorTarget::Internal(InternalColorTarget::SceneA) => &self.scene_a.view,
            ColorTarget::Internal(InternalColorTarget::SceneB) => &self.scene_b.view,
            ColorTarget::Internal(InternalColorTarget::WipeA) => &self.wipe_a.view,
            ColorTarget::Internal(InternalColorTarget::WipeB) => &self.wipe_b.view,
            ColorTarget::Internal(InternalColorTarget::ShadowMap) => &self.shadow_map.view,
        }
    }

    fn depth_target_view(&self, target: DepthTarget) -> Option<&wgpu::TextureView> {
        match target {
            DepthTarget::None => None,
            DepthTarget::Main => Some(&self.depth.view),
            DepthTarget::Surface => Some(&self.surface_depth.view),
            DepthTarget::Shadow => Some(&self.shadow_depth.view),
        }
    }

    fn backdrop_target_ref(&self, target: BackdropTarget) -> &RenderTargetTexture {
        match target {
            BackdropTarget::SceneA => &self.scene_a,
            BackdropTarget::SceneB => &self.scene_b,
        }
    }

    fn render_command_slice(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        color_target: ColorTarget<'_>,
        depth_target: DepthTarget,
        range: std::ops::Range<usize>,
        color_load: wgpu::LoadOp<wgpu::Color>,
        clear_depth: bool,
        overlay_backdrop: Option<BackdropTarget>,
        force_special: Option<TechniqueSpecial>,
    ) -> Result<()> {
        // D3D9 keeps its constant buffers/device state alive across draw calls.
        // Creating two wgpu buffers and a bind group for every sprite on every
        // frame made CPU cost scale catastrophically with scene complexity.
        // Resolve external resources first, then update persistent per-draw slots.
        let external_paths: Vec<PathBuf> = range
            .clone()
            .flat_map(|idx| {
                let cmd = &self.draws[idx];
                [
                    cmd.mesh_texture_path.clone(),
                    cmd.mesh_normal_texture_path.clone(),
                    cmd.mesh_toon_texture_path.clone(),
                ]
                .into_iter()
                .flatten()
            })
            .collect();
        for path in external_paths {
            let _ = self.ensure_external_texture(&path);
        }
        for draw_idx in range.clone() {
            self.prepare_draw_gpu_slot(draw_idx, overlay_backdrop)?;
        }

        let viewport = match color_target {
            ColorTarget::External(_) => self.surface_viewport,
            ColorTarget::Internal(InternalColorTarget::SceneA)
            | ColorTarget::Internal(InternalColorTarget::SceneB)
            | ColorTarget::Internal(InternalColorTarget::WipeA)
            | ColorTarget::Internal(InternalColorTarget::WipeB) => SurfaceViewport::full(
                self.logical_width.max(1.0).round() as u32,
                self.logical_height.max(1.0).round() as u32,
            ),
            ColorTarget::Internal(InternalColorTarget::ShadowMap) => {
                SurfaceViewport::full(self.shadow_map.width, self.shadow_map.height)
            }
        };
        let color_view = self.color_target_view(color_target);
        let depth_view = self.depth_target_view(depth_target);
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("siglus-sprite-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: color_load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: depth_view.map(|view| {
                wgpu::RenderPassDepthStencilAttachment {
                    view,
                    depth_ops: Some(wgpu::Operations {
                        load: if clear_depth {
                            wgpu::LoadOp::Clear(1.0)
                        } else {
                            wgpu::LoadOp::Load
                        },
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        rp.set_vertex_buffer(0, self.vertex_buf.slice(..));
        rp.set_viewport(
            viewport.x as f32,
            viewport.y as f32,
            viewport.w as f32,
            viewport.h as f32,
            0.0,
            1.0,
        );

        for draw_idx in range {
            let cmd = &self.draws[draw_idx];
            let mut effective_key = cmd.pipeline_key.clone();
            if let Some(special) = force_special {
                effective_key = shadow_pipeline_key(
                    cmd.pipeline_key.clone(),
                    cmd.shadow_pipeline_name.as_deref(),
                );
                effective_key.technique.special = special;
            }
            if let Some(pipeline) = self.pipelines.get(&effective_key) {
                rp.set_pipeline(pipeline);
            }
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            {
                if effective_key.program.uses_sprite2d_layout() {
                    rp.set_vertex_buffer(0, self.vertex_sprite2d_buf.slice(..));
                } else {
                    rp.set_vertex_buffer(0, self.vertex_buf.slice(..));
                }
            }
            let bind_group = self.draw_gpu_slots[draw_idx]
                .bind_group
                .as_ref()
                .expect("draw gpu slot prepared before render pass");
            rp.set_bind_group(0, bind_group, &[]);
            if let Some(sci) = cmd.scissor {
                rp.set_scissor_rect(sci.x, sci.y, sci.w, sci.h);
            } else {
                rp.set_scissor_rect(viewport.x, viewport.y, viewport.w, viewport.h);
            }
            rp.draw(cmd.range.clone(), 0..1);
        }
        Ok(())
    }

    fn resolve_effect_resources_for_draw<'a>(
        &'a self,
        cmd: &'a DrawCommand,
        overlay_backdrop: Option<&'a RenderTargetTexture>,
    ) -> EffectResolvedResources<'a> {
        let emote_base = cmd
            .emote_render_id
            .and_then(|id| self.emote_compositor.texture(id));
        let base = if let Some(texture) = emote_base {
            texture
        } else if let Some(path) = cmd.mesh_texture_path.as_deref() {
            self.external_textures
                .get(path)
                .or_else(|| cmd.image_id.and_then(|id| self.textures.get(&id)))
                .unwrap_or(&self.default_aux)
        } else {
            cmd.image_id
                .and_then(|id| self.textures.get(&id))
                .unwrap_or(&self.default_aux)
        };
        let mask = cmd
            .mask_image_id
            .and_then(|id| self.textures.get(&id))
            .unwrap_or(&self.default_aux);
        let tone = cmd
            .tonecurve_image_id
            .and_then(|id| self.textures.get(&id))
            .unwrap_or(&self.default_aux);
        let fog = cmd
            .fog_image_id
            .and_then(|id| self.textures.get(&id))
            .unwrap_or(&self.default_aux);
        let normal = cmd
            .mesh_normal_texture_path
            .as_deref()
            .and_then(|p| self.external_textures.get(p))
            .unwrap_or(&self.default_aux);
        let toon = cmd
            .mesh_toon_texture_path
            .as_deref()
            .and_then(|p| self.external_textures.get(p))
            .unwrap_or(&self.default_aux);
        let (aux_view, aux_sampler) = if matches!(
            cmd.pipeline_key.technique.special,
            TechniqueSpecial::Overlay
        ) {
            if let Some(backdrop) = overlay_backdrop {
                (&backdrop.view, &backdrop.sampler)
            } else {
                (&self.default_aux.view, &self.default_aux.sampler)
            }
        } else if let Some(id) = cmd.wipe_src_image_id {
            if let Some(tex) = self.textures.get(&id) {
                (&tex.view, &tex.sampler)
            } else {
                (&self.default_aux.view, &self.default_aux.sampler)
            }
        } else {
            (&self.default_aux.view, &self.default_aux.sampler)
        };
        let global_vals = EffectGlobalValPackSemantic {
            use_bone_uniform: matches!(
                cmd.draw_kind,
                MeshDrawKind::SkinnedMesh | MeshDrawKind::ShadowCaster
            ) && cmd.mesh_material_key.as_ref().is_some_and(|k| k.skinned),
            use_shadow_tex: cmd.pipeline_key.use_depth
                || cmd.shadow_cast
                || cmd.mesh_material_key.as_ref().is_some_and(|k| k.shadow),
            use_normal_tex: cmd
                .mesh_material_key
                .as_ref()
                .is_some_and(|k| k.use_normal_tex),
            use_toon_tex: cmd
                .mesh_material_key
                .as_ref()
                .is_some_and(|k| k.use_toon_tex),
        };
        EffectResolvedResources {
            base,
            mask,
            tone,
            fog,
            normal,
            toon,
            aux_view,
            aux_sampler,
            shadow_view: &self.shadow_map.view,
            shadow_sampler: &self.shadow_sampler,
            global_vals,
        }
    }

    fn render_copy_pass(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        color_target: ColorTarget<'_>,
        src: BackdropTarget,
        blit_range: std::ops::Range<u32>,
    ) -> Result<()> {
        let color_view = self.color_target_view(color_target);
        let src = self.backdrop_target_ref(src);
        let key = PipelineKey {
            technique: TechniqueKey {
                d3: false,
                light: false,
                fog: false,
                tex: 1,
                diffuse: false,
                mrbd: false,
                rgb: false,
                tonecurve: false,
                mask: false,
                special: TechniqueSpecial::None,
            },
            blend: SpriteBlend::Normal,
            alpha_blend: false,
            use_depth: false,
            depth_attachment: false,
            cull_back: false,
            mesh_fx_variant: 0,
            pipeline_name: String::new(),
            program: EffectProgram::Sprite2D,
        };
        let target_is_external = matches!(color_target, ColorTarget::External(_));
        let uniform_width = if target_is_external {
            self.config.width as f32
        } else {
            self.logical_width.max(1.0)
        };
        let uniform_height = if target_is_external {
            self.config.height as f32
        } else {
            self.logical_height.max(1.0)
        };
        let vs_uniform = plain_sprite2d_uniform(uniform_width, uniform_height);
        let vs_uniform_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("siglus-copy-vs-uniform"),
                contents: bytemuck::bytes_of(&vs_uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bone_uniform = BoneUniform::zero();
        let bone_uniform_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("siglus-copy-bone-uniform"),
                contents: bytemuck::bytes_of(&bone_uniform),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("siglus-copy-bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&src.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&src.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&self.default_aux.view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&self.default_aux.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&self.default_aux.view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&self.default_aux.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(&self.default_aux.view),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Sampler(&self.default_aux.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(&self.default_aux.view),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::Sampler(&self.default_aux.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: wgpu::BindingResource::TextureView(&self.shadow_map.view),
                },
                wgpu::BindGroupEntry {
                    binding: 11,
                    resource: wgpu::BindingResource::Sampler(&self.shadow_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 12,
                    resource: vs_uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 13,
                    resource: bone_uniform_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 14,
                    resource: wgpu::BindingResource::TextureView(&self.default_aux.view),
                },
                wgpu::BindGroupEntry {
                    binding: 15,
                    resource: wgpu::BindingResource::Sampler(&self.normal_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 16,
                    resource: wgpu::BindingResource::TextureView(&self.default_aux.view),
                },
                wgpu::BindGroupEntry {
                    binding: 17,
                    resource: wgpu::BindingResource::Sampler(&self.toon_sampler),
                },
            ],
        });
        let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("siglus-copy-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: color_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        if let Some(pipeline) = self.pipelines.get(&key) {
            rp.set_pipeline(pipeline);
        }
        let viewport = if target_is_external {
            self.surface_viewport
        } else {
            SurfaceViewport::full(
                self.logical_width.max(1.0).round() as u32,
                self.logical_height.max(1.0).round() as u32,
            )
        };
        rp.set_viewport(
            viewport.x as f32,
            viewport.y as f32,
            viewport.w as f32,
            viewport.h as f32,
            0.0,
            1.0,
        );
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        rp.set_vertex_buffer(0, self.vertex_buf.slice(..));
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        rp.set_vertex_buffer(0, self.vertex_sprite2d_buf.slice(..));
        rp.set_bind_group(0, &bind_group, &[]);
        rp.set_scissor_rect(viewport.x, viewport.y, viewport.w, viewport.h);
        rp.draw(blit_range, 0..1);
        Ok(())
    }

    fn ensure_vertex_capacity(&mut self, needed: usize) -> Result<()> {
        if needed <= self.vertex_capacity {
            #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
            {
                if needed > self.vertex_sprite2d_capacity {
                    let new_cap = ((needed + 5) / 6) * 6;
                    self.vertex_sprite2d_capacity = new_cap;
                    self.vertex_sprite2d_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("siglus-sprite2d-vertex-buf"),
                        size: (new_cap * std::mem::size_of::<VertexSprite2dData>())
                            as wgpu::BufferAddress,
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                }
            }
            return Ok(());
        }
        let new_cap = ((needed + 5) / 6) * 6;
        self.vertex_capacity = new_cap;

        self.vertex_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("siglus-sprite-vertex-buf"),
            size: (new_cap * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            self.vertex_sprite2d_capacity = new_cap;
            self.vertex_sprite2d_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("siglus-sprite2d-vertex-buf"),
                size: (new_cap * std::mem::size_of::<VertexSprite2dData>()) as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
        }
        Ok(())
    }

    fn ensure_draw_gpu_slots(&mut self, needed: usize) {
        while self.draw_gpu_slots.len() < needed {
            let slot_no = self.draw_gpu_slots.len();
            let vs_uniform_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("siglus-vs-uniform-slot"),
                size: std::mem::size_of::<VsUniform>() as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let bone_uniform_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("siglus-bone-uniform-slot"),
                size: std::mem::size_of::<BoneUniform>() as wgpu::BufferAddress,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.draw_gpu_slots.push(DrawGpuSlot {
                vs_uniform_buf,
                bone_uniform_buf,
                bind_group: None,
                bind_key: None,
                bind_epoch: 0,
            });
            debug_assert_eq!(self.draw_gpu_slots.len(), slot_no + 1);
        }
    }

    fn prepare_draw_gpu_slot(
        &mut self,
        draw_idx: usize,
        overlay_backdrop: Option<BackdropTarget>,
    ) -> Result<()> {
        self.ensure_draw_gpu_slots(draw_idx + 1);

        let cmd = &self.draws[draw_idx];
        self.queue.write_buffer(
            &self.draw_gpu_slots[draw_idx].vs_uniform_buf,
            0,
            bytemuck::bytes_of(&cmd.vs_uniform),
        );
        self.queue.write_buffer(
            &self.draw_gpu_slots[draw_idx].bone_uniform_buf,
            0,
            bytemuck::bytes_of(&cmd.bone_uniform),
        );

        let bind_key = DrawBindKey::from_command(cmd, overlay_backdrop);
        // Emote's offscreen target may be recreated in-place when the requested
        // render size changes while keeping the same render id. Its texture view
        // therefore cannot be safely retained in a cached bind group across
        // frames. Ordinary Siglus images and mesh textures use stable cache keys
        // and can retain their bind groups until the resource epoch changes.
        let cacheable = cmd.emote_render_id.is_none();
        let slot = &self.draw_gpu_slots[draw_idx];
        let needs_bind_group = !cacheable
            || slot.bind_group.is_none()
            || slot.bind_epoch != self.draw_bind_epoch
            || slot.bind_key.as_ref() != Some(&bind_key);
        if !needs_bind_group {
            return Ok(());
        }

        let semantics = self.resolve_effect_resources_for_draw(
            cmd,
            overlay_backdrop.map(|target| self.backdrop_target_ref(target)),
        );
        let base_sampler = if bind_key.mesh_base_sampler {
            &self.mesh_sampler
        } else {
            &semantics.base.sampler
        };
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("siglus-sprite-bg-slot"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&semantics.base.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(base_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&semantics.mask.view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&semantics.mask.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: wgpu::BindingResource::TextureView(&semantics.tone.view),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::Sampler(&semantics.tone.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: wgpu::BindingResource::TextureView(semantics.aux_view),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: wgpu::BindingResource::Sampler(semantics.aux_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: wgpu::BindingResource::TextureView(&semantics.fog.view),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: wgpu::BindingResource::Sampler(&self.fog_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: wgpu::BindingResource::TextureView(semantics.shadow_view),
                },
                wgpu::BindGroupEntry {
                    binding: 11,
                    resource: wgpu::BindingResource::Sampler(&self.shadow_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 12,
                    resource: self.draw_gpu_slots[draw_idx]
                        .vs_uniform_buf
                        .as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 13,
                    resource: self.draw_gpu_slots[draw_idx]
                        .bone_uniform_buf
                        .as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 14,
                    resource: wgpu::BindingResource::TextureView(&semantics.normal.view),
                },
                wgpu::BindGroupEntry {
                    binding: 15,
                    resource: wgpu::BindingResource::Sampler(&self.normal_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 16,
                    resource: wgpu::BindingResource::TextureView(&semantics.toon.view),
                },
                wgpu::BindGroupEntry {
                    binding: 17,
                    resource: wgpu::BindingResource::Sampler(&self.toon_sampler),
                },
            ],
        });

        let slot = &mut self.draw_gpu_slots[draw_idx];
        slot.bind_group = Some(bind_group);
        slot.bind_key = cacheable.then_some(bind_key);
        slot.bind_epoch = self.draw_bind_epoch;
        Ok(())
    }

    /// Drop GPU textures that are keyed by runtime ImageId.
    ///
    /// Scene restart reinitializes ImageManager and reuses ImageId indices from 0.
    /// Keeping the old GPU cache would make a newly decoded image with the same
    /// ImageId/version sample the previous scene's texture. External path based
    /// textures are intentionally kept because their keys are stable resource paths.
    pub fn clear_runtime_image_textures(&mut self) {
        self.textures.clear();
        self.draw_bind_epoch = self.draw_bind_epoch.wrapping_add(1).max(1);
    }

    fn ensure_texture_uploaded(&mut self, images: &ImageManager, id: ImageId) -> Result<()> {
        let Some((img, version)) = images.get_entry(id) else {
            return Ok(());
        };
        if let Some(mut tex) = self.textures.remove(&id) {
            if tex.version != version {
                if tex.width == img.width && tex.height == img.height {
                    self.update_texture(&mut tex, img)?;
                    tex.version = version;
                } else {
                    tex = create_gpu_texture(
                        &self.device,
                        &self.queue,
                        &format!("siglus-texture-{}", id.index()),
                        img,
                        version,
                    )?;
                    self.draw_bind_epoch = self.draw_bind_epoch.wrapping_add(1).max(1);
                }
            }
            self.textures.insert(id, tex);
        } else {
            let tex = create_gpu_texture(
                &self.device,
                &self.queue,
                &format!("siglus-texture-{}", id.index()),
                img,
                version,
            )?;
            self.textures.insert(id, tex);
        }
        Ok(())
    }

    fn update_texture(&self, tex: &GpuTexture, img: &crate::assets::RgbaImage) -> Result<()> {
        if tex.width != img.width || tex.height != img.height {
            return Ok(());
        }
        self.queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &tex._tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &img.rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * img.width),
                rows_per_image: Some(img.height),
            },
            wgpu::Extent3d {
                width: img.width,
                height: img.height,
                depth_or_array_layers: 1,
            },
        );
        Ok(())
    }
}
impl FrameCaptureBackend for Renderer {
    fn capture_render_frame(
        &mut self,
        images: &ImageManager,
        frame: &RenderFrame,
        logical_width: u32,
        logical_height: u32,
    ) -> Result<crate::assets::RgbaImage> {
        let renderer_width = self.logical_width.max(1.0).round() as u32;
        let renderer_height = self.logical_height.max(1.0).round() as u32;
        if renderer_width != logical_width.max(1) || renderer_height != logical_height.max(1) {
            anyhow::bail!(
                "capture logical size mismatch: renderer={}x{}, runtime={}x{}",
                renderer_width,
                renderer_height,
                logical_width,
                logical_height,
            );
        }
        let final_target = self.render_frame_to_internal(images, frame)?;
        let target = self.backdrop_target_ref(final_target);
        let rgba =
            self.debug_read_texture_rgba(&target._tex, target.width, target.height, target.format)?;
        Ok(crate::assets::RgbaImage {
            width: target.width,
            height: target.height,
            center_x: 0,
            center_y: 0,
            rgba,
        })
    }
}

fn create_solid_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    rgba: [u8; 4],
) -> Result<GpuTexture> {
    let img = crate::assets::RgbaImage {
        width: 1,
        height: 1,
        center_x: 0,
        center_y: 0,
        rgba: rgba.to_vec(),
    };
    create_gpu_texture(device, queue, "siglus-default-aux", &img, 0)
}

#[derive(Debug)]
struct Rgba8MipLevel {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Build the same kind of full mip chain requested by the original
/// D3DUSAGE_AUTOGENMIPMAP textures.  Values are averaged in the stored 8-bit
/// color space rather than converted through sRGB, matching the D3D9 setup.
pub fn build_rgba8_mip_chain(width: u32, height: u32, rgba: &[u8]) -> Vec<Rgba8MipLevel> {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        build_rgba8_mip_chain_swar(width, height, rgba)
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        let level = Level::new();
        dispatch!(level, simd => {
            // avx512容易因为降频产生性能回退，故默认禁用
            #[cfg(any(target_arch = "x86", target_arch = "x86_64"))]
            #[cfg(not(feature = "avx512-mipmap"))]
            if let Level::Avx512(_) = simd.level() {
                let avx2 = simd.level().as_avx2().expect("AVX-512 implies AVX2");
                return build_rgba8_mip_chain_simd(avx2, width, height, rgba);
            }
            build_rgba8_mip_chain_simd(simd, width, height, rgba)
        })
    }
}

#[inline(always)]
fn avg4_swar(p0: u32, p1: u32, p2: u32, p3: u32) -> u32 {
    const MASK: u32 = 0x00FF_00FF;
    let lo = (p0 & MASK) + (p1 & MASK) + (p2 & MASK) + (p3 & MASK);
    let hi = ((p0 >> 8) & MASK) + ((p1 >> 8) & MASK) + ((p2 >> 8) & MASK) + ((p3 >> 8) & MASK);
    (((lo + 0x0002_0002) >> 2) & MASK) | ((((hi + 0x0002_0002) >> 2) & MASK) << 8)
}

pub fn build_rgba8_mip_chain_swar(width: u32, height: u32, rgba: &[u8]) -> Vec<Rgba8MipLevel> {
    if width == 0 || height == 0 || rgba.len() < width as usize * height as usize * 4 {
        return Vec::new();
    }

    let mut levels = vec![Rgba8MipLevel {
        width,
        height,
        rgba: rgba[..width as usize * height as usize * 4].to_vec(),
    }];

    while levels
        .last()
        .is_some_and(|level| level.width > 1 || level.height > 1)
    {
        let prev = levels.last().expect("mip chain contains level zero");
        let next_width = (prev.width / 2).max(1);
        let next_height = (prev.height / 2).max(1);
        let prev_w = prev.width as usize;
        let prev_h = prev.height as usize;
        let next_w = next_width as usize;
        let next_h = next_height as usize;
        let mut next = vec![0u8; next_w * next_h * 4];

        let row_bytes = prev_w * 4;

        if prev_w >= 2 && prev_h >= 2 {
            for y in 0..next_h {
                let row0 = y * 2 * row_bytes;
                let dst_row = y * next_w * 4;
                for x in 0..next_w {
                    let base = row0 + x * 8;
                    let p0 = u32::from_ne_bytes(prev.rgba[base..base + 4].try_into().unwrap());
                    let p1 = u32::from_ne_bytes(prev.rgba[base + 4..base + 8].try_into().unwrap());
                    let p2 = u32::from_ne_bytes(
                        prev.rgba[base + row_bytes..base + row_bytes + 4]
                            .try_into()
                            .unwrap(),
                    );
                    let p3 = u32::from_ne_bytes(
                        prev.rgba[base + row_bytes + 4..base + row_bytes + 8]
                            .try_into()
                            .unwrap(),
                    );
                    let dst = dst_row + x * 4;
                    next[dst..dst + 4].copy_from_slice(&avg4_swar(p0, p1, p2, p3).to_ne_bytes());
                }
            }
        } else {
            let last_src_x = (next_w * 2 - 1).min(prev_w - 1);
            let last_src_y = (next_h * 2 - 1).min(prev_h - 1);
            for y in 0..next_h {
                let sy0 = y * 2;
                let sy1 = (sy0 + 1).min(last_src_y);
                let row_off = (sy1 - sy0) * row_bytes;
                let row0 = sy0 * row_bytes;
                let dst_row = y * next_w * 4;
                for x in 0..next_w {
                    let sx0 = x * 2;
                    let sx1 = (sx0 + 1).min(last_src_x);
                    let x_off = (sx1 - sx0) * 4;
                    let base = row0 + sx0 * 4;
                    let p0 = u32::from_ne_bytes(prev.rgba[base..base + 4].try_into().unwrap());
                    let p1 = u32::from_ne_bytes(
                        prev.rgba[base + x_off..base + x_off + 4]
                            .try_into()
                            .unwrap(),
                    );
                    let p2 = u32::from_ne_bytes(
                        prev.rgba[base + row_off..base + row_off + 4]
                            .try_into()
                            .unwrap(),
                    );
                    let p3 = u32::from_ne_bytes(
                        prev.rgba[base + row_off + x_off..base + row_off + x_off + 4]
                            .try_into()
                            .unwrap(),
                    );
                    let dst = dst_row + x * 4;
                    next[dst..dst + 4].copy_from_slice(&avg4_swar(p0, p1, p2, p3).to_ne_bytes());
                }
            }
        }

        levels.push(Rgba8MipLevel {
            width: next_width,
            height: next_height,
            rgba: next,
        });
    }

    levels
}

#[inline(always)]
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn build_rgba8_mip_chain_simd<S: Simd>(simd: S, width: u32, height: u32, rgba: &[u8],) -> Vec<Rgba8MipLevel>
{
    if width == 0 || height == 0 || rgba.len() < width as usize * height as usize * 4 {
        return Vec::new();
    }

    let mut levels = vec![Rgba8MipLevel {
        width,
        height,
        rgba: rgba[..width as usize * height as usize * 4].to_vec(),
    }];

    while levels
        .last()
        .is_some_and(|level| level.width > 1 || level.height > 1)
    {
        let prev = levels.last().expect("mip chain contains level zero");
        let next_width = (prev.width / 2).max(1);
        let next_height = (prev.height / 2).max(1);
        let prev_w = prev.width as usize;
        let prev_h = prev.height as usize;
        let next_w = next_width as usize;
        let next_h = next_height as usize;
        let mut next = vec![0u8; next_w * next_h * 4];

        let row_bytes = prev_w * 4;

        if prev_w >= 2 && prev_h >= 2 {
            let full = next_w / 4;
            let two = u16x16::splat(simd, 2);
            let zero = u16x16::splat(simd, 0);
            let perm = u8x32::from_slice(
                simd,
                &[
                    0, 1, 2, 3, 8, 9, 10, 11, 16, 17, 18, 19, 24, 25, 26, 27, //
                    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, //
                ],
            );

            for y in 0..next_h {
                let row0 = y * 2 * row_bytes;
                let dst_row = y * next_w * 4;

                for g in 0..full {
                    let base = row0 + g * 32;
                    let a = u8x32::from_slice(simd, &prev.rgba[base..base + 32]);
                    let b = u8x32::from_slice(
                        simd,
                        &prev.rgba[base + row_bytes..base + row_bytes + 32],
                    );

                    let (a0, a1) = a.widen();
                    let (b0, b1) = b.widen();

                    let t0 = a0 + a0.slide::<4>(zero);
                    let t1 = a1 + a1.slide::<4>(zero);
                    let u0 = b0 + b0.slide::<4>(zero);
                    let u1 = b1 + b1.slide::<4>(zero);

                    let s0 = t0 + u0;
                    let s1 = t1 + u1;

                    let r0 = (s0 + two) >> 2;
                    let r1 = (s1 + two) >> 2;

                    let out: u8x32<S> = r0.narrow(r1);
                    let packed = out.swizzle_dyn(perm);
                    let (lo, _hi) = packed.split();
                    let dst = dst_row + g * 16;
                    lo.store_slice(&mut next[dst..dst + 16]);
                }

                for k in 0..next_w - full * 4 {
                    let x = full * 4 + k;
                    let base = row0 + x * 8;
                    let p0 = u32::from_ne_bytes(prev.rgba[base..base + 4].try_into().unwrap());
                    let p1 =
                        u32::from_ne_bytes(prev.rgba[base + 4..base + 8].try_into().unwrap());
                    let p2 = u32::from_ne_bytes(
                        prev.rgba[base + row_bytes..base + row_bytes + 4]
                            .try_into()
                            .unwrap(),
                    );
                    let p3 = u32::from_ne_bytes(
                        prev.rgba[base + row_bytes + 4..base + row_bytes + 8]
                            .try_into()
                            .unwrap(),
                    );
                    let dst = dst_row + x * 4;
                    next[dst..dst + 4].copy_from_slice(&avg4_swar(p0, p1, p2, p3).to_ne_bytes());
                }
            }
        } else {
            write_degen_level(
                prev.rgba.as_slice(),
                prev_w,
                prev_h,
                next.as_mut_ptr(),
                next_w,
                next_h,
            );
        }

        levels.push(Rgba8MipLevel {
            width: next_width,
            height: next_height,
            rgba: next,
        });
    }

    levels
}

fn write_degen_level(prev: &[u8], prev_w: usize, prev_h: usize, next_ptr: *mut u8, next_w: usize, next_h: usize,)
{
    let row_bytes = prev_w * 4;
    let last_src_x = (next_w * 2 - 1).min(prev_w - 1);
    let last_src_y = (next_h * 2 - 1).min(prev_h - 1);
    for y in 0..next_h {
        let sy0 = y * 2;
        let sy1 = (sy0 + 1).min(last_src_y);
        let row_off = (sy1 - sy0) * row_bytes;
        let row0 = sy0 * row_bytes;
        let mut dst = (y * next_w * 4) as isize;
        for x in 0..next_w {
            let sx0 = x * 2;
            let sx1 = (sx0 + 1).min(last_src_x);
            let x_off = (sx1 - sx0) * 4;
            let base = row0 + sx0 * 4;
            let p0 = u32::from_ne_bytes(prev[base..base + 4].try_into().unwrap());
            let p1 = u32::from_ne_bytes(prev[base + x_off..base + x_off + 4].try_into().unwrap());
            let p2 =
                u32::from_ne_bytes(prev[base + row_off..base + row_off + 4].try_into().unwrap());
            let p3 = u32::from_ne_bytes(
                prev[base + row_off + x_off..base + row_off + x_off + 4]
                    .try_into()
                    .unwrap(),
            );
            let out = avg4_swar(p0, p1, p2, p3);
            // SAFETY: within `next`, each byte written exactly once.
            unsafe { (next_ptr.offset(dst) as *mut u32).write_unaligned(out) };
            dst += 4;
        }
    }
}

fn create_gpu_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    img: &crate::assets::RgbaImage,
    version: u64,
) -> Result<GpuTexture> {
    let mip_chain = build_rgba8_mip_chain(img.width, img.height, &img.rgba);
    let mip_level_count = mip_chain.len().max(1) as u32;
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: img.width,
            height: img.height,
            depth_or_array_layers: 1,
        },
        mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });

    for (mip_level, mip) in mip_chain.iter().enumerate() {
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &tex,
                mip_level: mip_level as u32,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &mip.rgba,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * mip.width),
                rows_per_image: Some(mip.height),
            },
            wgpu::Extent3d {
                width: mip.width,
                height: mip.height,
                depth_or_array_layers: 1,
            },
        );
    }

    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("siglus-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });

    Ok(GpuTexture {
        _tex: tex,
        view,
        sampler,
        width: img.width,
        height: img.height,
        version,
    })
}

fn create_render_target_texture(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    label: &str,
) -> RenderTargetTexture {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("siglus-render-target-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    RenderTargetTexture {
        _tex: tex,
        view,
        sampler,
        width: width.max(1),
        height: height.max(1),
        format,
    }
}

fn append_fullscreen_blit_vertices(verts: &mut Vec<Vertex>) -> std::ops::Range<u32> {
    let base = verts.len() as u32;
    let effects1 = [1.0, 0.0, 0.0, 0.0];
    let zero = [0.0; 4];
    verts.extend_from_slice(&[
        Vertex {
            pos: [-1.0, 1.0, 0.0],
            uv: [0.0, 0.0],
            uv_aux: [0.0, 0.0],
            alpha: 1.0,
            effects1,
            effects2: zero,
            effects3: zero,
            effects4: zero,
            effects5: zero,
            effects6: zero,
            effects7: zero,
            effects8: zero,
            effects9: zero,
            effects10: zero,
            effects11: zero,
            world_pos: zero,
            world_normal: zero,
            world_tangent: zero,
            world_binormal: zero,
            shadow_pos: zero,
            bone_indices: zero,
            bone_weights: zero,
            light_pos_kind: zero,
            light_dir_shadow: zero,
            light_atten: zero,
            light_cone: zero,
        },
        Vertex {
            pos: [1.0, 1.0, 0.0],
            uv: [1.0, 0.0],
            uv_aux: [0.0, 0.0],
            alpha: 1.0,
            effects1,
            effects2: zero,
            effects3: zero,
            effects4: zero,
            effects5: zero,
            effects6: zero,
            effects7: zero,
            effects8: zero,
            effects9: zero,
            effects10: zero,
            effects11: zero,
            world_pos: zero,
            world_normal: zero,
            world_tangent: zero,
            world_binormal: zero,
            shadow_pos: zero,
            bone_indices: zero,
            bone_weights: zero,
            light_pos_kind: zero,
            light_dir_shadow: zero,
            light_atten: zero,
            light_cone: zero,
        },
        Vertex {
            pos: [1.0, -1.0, 0.0],
            uv: [1.0, 1.0],
            uv_aux: [0.0, 0.0],
            alpha: 1.0,
            effects1,
            effects2: zero,
            effects3: zero,
            effects4: zero,
            effects5: zero,
            effects6: zero,
            effects7: zero,
            effects8: zero,
            effects9: zero,
            effects10: zero,
            effects11: zero,
            world_pos: zero,
            world_normal: zero,
            world_tangent: zero,
            world_binormal: zero,
            shadow_pos: zero,
            bone_indices: zero,
            bone_weights: zero,
            light_pos_kind: zero,
            light_dir_shadow: zero,
            light_atten: zero,
            light_cone: zero,
        },
        Vertex {
            pos: [-1.0, 1.0, 0.0],
            uv: [0.0, 0.0],
            uv_aux: [0.0, 0.0],
            alpha: 1.0,
            effects1,
            effects2: zero,
            effects3: zero,
            effects4: zero,
            effects5: zero,
            effects6: zero,
            effects7: zero,
            effects8: zero,
            effects9: zero,
            effects10: zero,
            effects11: zero,
            world_pos: zero,
            world_normal: zero,
            world_tangent: zero,
            world_binormal: zero,
            shadow_pos: zero,
            bone_indices: zero,
            bone_weights: zero,
            light_pos_kind: zero,
            light_dir_shadow: zero,
            light_atten: zero,
            light_cone: zero,
        },
        Vertex {
            pos: [1.0, -1.0, 0.0],
            uv: [1.0, 1.0],
            uv_aux: [0.0, 0.0],
            alpha: 1.0,
            effects1,
            effects2: zero,
            effects3: zero,
            effects4: zero,
            effects5: zero,
            effects6: zero,
            effects7: zero,
            effects8: zero,
            effects9: zero,
            effects10: zero,
            effects11: zero,
            world_pos: zero,
            world_normal: zero,
            world_tangent: zero,
            world_binormal: zero,
            shadow_pos: zero,
            bone_indices: zero,
            bone_weights: zero,
            light_pos_kind: zero,
            light_dir_shadow: zero,
            light_atten: zero,
            light_cone: zero,
        },
        Vertex {
            pos: [-1.0, -1.0, 0.0],
            uv: [0.0, 1.0],
            uv_aux: [0.0, 0.0],
            alpha: 1.0,
            effects1,
            effects2: zero,
            effects3: zero,
            effects4: zero,
            effects5: zero,
            effects6: zero,
            effects7: zero,
            effects8: zero,
            effects9: zero,
            effects10: zero,
            effects11: zero,
            world_pos: zero,
            world_normal: zero,
            world_tangent: zero,
            world_binormal: zero,
            shadow_pos: zero,
            bone_indices: zero,
            bone_weights: zero,
            light_pos_kind: zero,
            light_dir_shadow: zero,
            light_atten: zero,
            light_cone: zero,
        },
    ]);
    base..base + 6
}

fn pixel_to_ndc(x: f32, y: f32, depth: f32, win_w: f32, win_h: f32) -> (f32, f32, f32) {
    let nx = (x / win_w) * 2.0 - 1.0;
    let ny = 1.0 - (y / win_h) * 2.0;
    // WGPU follows the Direct3D/Vulkan clip-space convention: z is 0..1.
    // The previous OpenGL-style mapping (depth * 2 - 1) put all 2D quads at z=-1,
    // which is outside WGPU clip space and makes the game window render black even
    // though the VM submits sprites correctly.
    let nz = depth.clamp(0.0, 1.0);
    (nx, ny, nz)
}

fn create_depth_texture(device: &wgpu::Device, width: u32, height: u32) -> DepthTexture {
    create_depth_texture_with_format(
        device,
        width,
        height,
        wgpu::TextureFormat::Depth32Float,
        "siglus-depth",
    )
}

fn create_depth_texture_with_format(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    label: &str,
) -> DepthTexture {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    DepthTexture { _tex: tex, view }
}

fn src_clip_rect(clip: Option<ClipRect>, img_w: u32, img_h: u32) -> Result<(f32, f32, f32, f32)> {
    if let Some(c) = clip {
        let mut left = c.left.max(0) as f32;
        let mut top = c.top.max(0) as f32;
        let mut right = c.right.max(0) as f32;
        let mut bottom = c.bottom.max(0) as f32;
        let max_w = img_w as f32;
        let max_h = img_h as f32;
        left = left.min(max_w);
        right = right.min(max_w);
        top = top.min(max_h);
        bottom = bottom.min(max_h);
        if right <= left || bottom <= top {
            return Ok((0.0, 0.0, max_w, max_h));
        }
        Ok((left, top, right, bottom))
    } else {
        Ok((0.0, 0.0, img_w as f32, img_h as f32))
    }
}

fn dst_scissor_rect_to_viewport(
    clip: Option<ClipRect>,
    viewport: SurfaceViewport,
    logical_w: f32,
    logical_h: f32,
    surface_w: u32,
    surface_h: u32,
) -> Option<ScissorRect> {
    let c = clip?;
    let sx = (viewport.w as f32) / logical_w.max(1.0);
    let sy = (viewport.h as f32) / logical_h.max(1.0);
    let mut left = viewport.x as i64 + ((c.left.max(0) as f32) * sx).floor() as i64;
    let mut top = viewport.y as i64 + ((c.top.max(0) as f32) * sy).floor() as i64;
    let mut right = viewport.x as i64 + ((c.right.max(0) as f32) * sx).ceil() as i64;
    let mut bottom = viewport.y as i64 + ((c.bottom.max(0) as f32) * sy).ceil() as i64;
    let max_w = surface_w as i64;
    let max_h = surface_h as i64;
    left = left.min(max_w);
    right = right.min(max_w);
    top = top.min(max_h);
    bottom = bottom.min(max_h);
    if right <= left || bottom <= top {
        return Some(ScissorRect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        });
    }
    Some(ScissorRect {
        x: left as u32,
        y: top as u32,
        w: (right - left) as u32,
        h: (bottom - top) as u32,
    })
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn wasm_shader_source() -> String {
    SHADER.to_string()
}

#[derive(Debug)]
struct PageWipeDraw {
    use_current: bool,
    vertices: Vec<PageWipeVertex>,
}

fn wipe_option(options: &[i32], index: usize, default: i32) -> i32 {
    options.get(index).copied().unwrap_or(default)
}

fn page_range_angle(start: f32, end: f32, range_type: i32, progress: f32) -> f32 {
    let half = (start + end) * 0.5;
    match range_type {
        1 => half + (end - half) * progress,
        2 => start + (half - start) * progress,
        _ => start + (end - start) * progress,
    }
}

fn project_page_wipe_vertex(
    position: [f32; 3],
    uv: [f32; 2],
    angle: f32,
    width: f32,
    height: f32,
    fov_radians: f32,
) -> PageWipeVertex {
    let (sin, cos) = angle.sin_cos();
    let world_x = position[0] * cos + position[2] * sin;
    let world_z = -position[0] * sin + position[2] * cos;
    let tan_half = (fov_radians * 0.5).tan().max(0.000001);
    let focal = height * 0.5 / tan_half;
    let view_z = (world_z + focal).max(0.000001);
    let y_scale = 1.0 / tan_half;
    let aspect = (width / height.max(1.0)).max(0.000001);
    let x_scale = y_scale / aspect;
    let near = 1.0;
    let far = 10000.0;
    let clip_z = far / (far - near) * view_z - near * far / (far - near);
    PageWipeVertex {
        clip_position: [world_x * x_scale, position[1] * y_scale, clip_z, view_z],
        uv,
    }
}

fn page_quad_vertices(
    positions: [[f32; 3]; 4],
    uvs: [[f32; 2]; 4],
    angle: f32,
    width: f32,
    height: f32,
    fov_radians: f32,
) -> Vec<PageWipeVertex> {
    let projected = positions
        .into_iter()
        .zip(uvs)
        .map(|(position, uv)| {
            project_page_wipe_vertex(position, uv, angle, width, height, fov_radians)
        })
        .collect::<Vec<_>>();
    [0usize, 2, 1, 2, 3, 1]
        .into_iter()
        .map(|index| projected[index])
        .collect()
}

fn build_page_300_draw(
    wipe: &WipeRenderPlan,
    width: f32,
    height: f32,
    use_current: bool,
    is_front: bool,
) -> PageWipeDraw {
    let reverse = wipe_option(&wipe.option, 0, 0) != 0;
    let (start, end) = if is_front {
        if reverse {
            (std::f32::consts::PI, 0.0)
        } else {
            (std::f32::consts::PI, std::f32::consts::TAU)
        }
    } else if reverse {
        (0.0, -std::f32::consts::PI)
    } else {
        (0.0, std::f32::consts::PI)
    };
    let angle = page_range_angle(
        start,
        end,
        wipe_option(&wipe.option, 2, 0),
        wipe.progress.clamp(0.0, 1.0),
    );
    let half_width = width * 0.5;
    let half_height = height * 0.5;
    let positions = [
        [-half_width - 0.5, half_height + 0.5, 0.0],
        [half_width - 0.5, half_height + 0.5, 0.0],
        [-half_width - 0.5, -half_height + 0.5, 0.0],
        [half_width - 0.5, -half_height + 0.5, 0.0],
    ];
    let uvs = [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0], [1.0, 1.0]];
    let fov = (wipe_option(&wipe.option, 1, 450) as f32 / 10.0)
        .clamp(1.0, 179.0)
        .to_radians();
    PageWipeDraw {
        use_current,
        vertices: page_quad_vertices(positions, uvs, angle, width, height, fov),
    }
}

fn build_page_301_draws_for_stage(
    wipe: &WipeRenderPlan,
    width: f32,
    height: f32,
    use_current: bool,
    is_front: bool,
) -> Vec<PageWipeDraw> {
    let reverse = wipe_option(&wipe.option, 0, 0) != 0;
    let fov = (wipe_option(&wipe.option, 1, 450) as f32 / 10.0)
        .clamp(1.0, 179.0)
        .to_radians();
    let half_width = width * 0.5;
    let half_height = height * 0.5;
    let mut out = Vec::with_capacity(2);
    for half in 0..2 {
        let (start, end, z) = match (half, is_front, reverse) {
            (0, true, true) => (0.0, 0.0, 0.0),
            (0, true, false) => (std::f32::consts::PI, std::f32::consts::TAU, -1.0),
            (0, false, true) => (std::f32::consts::TAU, std::f32::consts::PI, -1.0),
            (0, false, false) => (0.0, 0.0, 0.0),
            (1, true, true) => (std::f32::consts::PI, 0.0, -1.0),
            (1, true, false) => (0.0, 0.0, 0.0),
            (1, false, true) => (0.0, 0.0, 0.0),
            (1, false, false) => (0.0, std::f32::consts::PI, -1.0),
            _ => unreachable!(),
        };
        let angle = page_range_angle(
            start,
            end,
            wipe_option(&wipe.option, 2, 0),
            wipe.progress.clamp(0.0, 1.0),
        );
        let (x0, x1, u0, u1) = if half == 0 {
            (-half_width - 0.5, -0.5, 0.0, 0.5)
        } else {
            (-0.5, half_width - 0.5, 0.5, 1.0)
        };
        let positions = [
            [x0, half_height + 0.5, z],
            [x1, half_height + 0.5, z],
            [x0, -half_height + 0.5, z],
            [x1, -half_height + 0.5, z],
        ];
        let uvs = [[u0, 0.0], [u1, 0.0], [u0, 1.0], [u1, 1.0]];
        out.push(PageWipeDraw {
            use_current,
            vertices: page_quad_vertices(positions, uvs, angle, width, height, fov),
        });
    }
    out
}

fn build_page_wipe_draws(wipe: &WipeRenderPlan, width: f32, height: f32) -> Vec<PageWipeDraw> {
    match wipe.wipe_type {
        300 => vec![
            build_page_300_draw(wipe, width, height, false, true),
            build_page_300_draw(wipe, width, height, true, false),
        ],
        301 => {
            let front_stage_first = wipe.progress < 0.5;
            let first_current = front_stage_first;
            let mut out = build_page_301_draws_for_stage(wipe, width, height, first_current, true);
            out.extend(build_page_301_draws_for_stage(
                wipe,
                width,
                height,
                !first_current,
                false,
            ));
            out
        }
        _ => Vec::new(),
    }
}

fn create_wipe_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> (wgpu::BindGroupLayout, wgpu::RenderPipeline) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("siglus-wipe-bind-group-layout"),
        entries: &[
            texture_layout_entry(0),
            sampler_layout_entry(1),
            texture_layout_entry(2),
            sampler_layout_entry(3),
            texture_layout_entry(4),
            sampler_layout_entry(5),
            texture_layout_entry(6),
            sampler_layout_entry(7),
            wgpu::BindGroupLayoutEntry {
                binding: 8,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("siglus-wipe-shader"),
        source: wgpu::ShaderSource::Wgsl(WIPE_SHADER.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("siglus-wipe-pipeline-layout"),
        bind_group_layouts: &[&layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("siglus-wipe-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: None,
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    });
    (layout, pipeline)
}

fn create_page_wipe_pipeline(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
) -> (wgpu::BindGroupLayout, wgpu::RenderPipeline) {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("siglus-page-wipe-bind-group-layout"),
        entries: &[texture_layout_entry(0), sampler_layout_entry(1)],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("siglus-page-wipe-shader"),
        source: wgpu::ShaderSource::Wgsl(PAGE_WIPE_SHADER.into()),
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("siglus-page-wipe-pipeline-layout"),
        bind_group_layouts: &[&layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("siglus-page-wipe-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: std::mem::size_of::<PageWipeVertex>() as wgpu::BufferAddress,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x4,
                        offset: 0,
                        shader_location: 0,
                    },
                    wgpu::VertexAttribute {
                        format: wgpu::VertexFormat::Float32x2,
                        offset: 16,
                        shader_location: 1,
                    },
                ],
            }],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            strip_index_format: None,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            polygon_mode: wgpu::PolygonMode::Fill,
            unclipped_depth: false,
            conservative: false,
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    });
    (layout, pipeline)
}

const PAGE_WIPE_SHADER: &str = r#"
@group(0) @binding(0) var page_tex: texture_2d<f32>;
@group(0) @binding(1) var page_smp: sampler;

struct VsIn {
    @location(0) clip_position: vec4<f32>,
    @location(1) uv: vec2<f32>,
};
struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};
@vertex
fn vs_main(input: VsIn) -> VsOut {
    var output: VsOut;
    output.position = input.clip_position;
    output.uv = input.uv;
    return output;
}
@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let color = textureSample(page_tex, page_smp, input.uv);
    if (color.a <= 0.0) { discard; }
    return color;
}
"#;

fn texture_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            multisampled: false,
            view_dimension: wgpu::TextureViewDimension::D2,
            sample_type: wgpu::TextureSampleType::Float { filterable: true },
        },
        count: None,
    }
}

fn sampler_layout_entry(binding: u32) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
        count: None,
    }
}

const WIPE_SHADER: &str = r#"
struct WipeUniform {
    kind_progress: vec4<f32>,
    option0: vec4<f32>,
    option1: vec4<f32>,
    option2: vec4<f32>,
    option3: vec4<f32>,
};

@group(0) @binding(0) var under_tex: texture_2d<f32>;
@group(0) @binding(1) var under_smp: sampler;
@group(0) @binding(2) var current_tex: texture_2d<f32>;
@group(0) @binding(3) var current_smp: sampler;
@group(0) @binding(4) var next_tex: texture_2d<f32>;
@group(0) @binding(5) var next_smp: sampler;
@group(0) @binding(6) var mask_tex: texture_2d<f32>;
@group(0) @binding(7) var mask_smp: sampler;
@group(0) @binding(8) var<uniform> wipe: WipeUniform;

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) index: u32) -> VsOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0)
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0)
    );
    var out: VsOut;
    out.pos = vec4<f32>(positions[index], 0.0, 1.0);
    out.uv = uvs[index];
    return out;
}

fn option(index: i32) -> f32 {
    if (index < 4) { return wipe.option0[index]; }
    if (index < 8) { return wipe.option1[index - 4]; }
    if (index < 12) { return wipe.option2[index - 8]; }
    return wipe.option3[index - 12];
}

fn inside(uv: vec2<f32>) -> bool {
    return all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0));
}

fn sample_or_zero(tex: texture_2d<f32>, smp: sampler, uv: vec2<f32>) -> vec4<f32> {
    if (!inside(uv)) { return vec4<f32>(0.0); }
    return textureSample(tex, smp, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)));
}

fn alpha_over(dst: vec4<f32>, src: vec4<f32>) -> vec4<f32> {
    let out_a = src.a + dst.a * (1.0 - src.a);
    if (out_a <= 0.000001) { return vec4<f32>(0.0); }
    let rgb = (src.rgb * src.a + dst.rgb * dst.a * (1.0 - src.a)) / out_a;
    return vec4<f32>(rgb, out_a);
}

fn mask_fade(mode: i32) -> f32 {
    switch mode {
        case 0: { return 0.0; }
        case 1: { return 0.5; }
        case 2: { return 0.75; }
        case 3: { return 0.875; }
        case 4: { return 0.9375; }
        case 5: { return 0.96875; }
        case 6: { return 0.984375; }
        case 7: { return 0.9921875; }
        default: { return 1.0; }
    }
}

fn mask_reveal(progress: f32, threshold: f32, fade: f32) -> f32 {
    if (fade <= 0.000001) { return select(0.0, 1.0, progress >= threshold); }
    return clamp((progress - threshold * (1.0 - fade)) / fade, 0.0, 1.0);
}

fn luminance(c: vec4<f32>) -> f32 {
    return dot(c.rgb, vec3<f32>(0.299, 0.587, 0.114)) * c.a;
}

fn rect_sample(tex: texture_2d<f32>, smp: sampler, uv: vec2<f32>, rect: vec4<f32>, alpha: f32) -> vec4<f32> {
    let local = (uv - rect.xy) / max(rect.zw, vec2<f32>(0.000001));
    let c = sample_or_zero(tex, smp, local);
    return vec4<f32>(c.rgb, c.a * alpha);
}

fn move_rect(direction: i32, mode: i32, progress: f32, incoming: bool) -> vec4<f32> {
    if (mode == 0) { return vec4<f32>(0.0, 0.0, 1.0, 1.0); }
    if (mode == 1) {
        var start = vec2<f32>(0.0);
        var end = vec2<f32>(0.0);
        if (incoming) {
            if (direction == 0) { start.y = -1.0; }
            else if (direction == 1) { start.y = 1.0; }
            else if (direction == 2) { start.x = -1.0; }
            else { start.x = 1.0; }
        } else {
            if (direction == 0) { end.y = 1.0; }
            else if (direction == 1) { end.y = -1.0; }
            else if (direction == 2) { end.x = 1.0; }
            else { end.x = -1.0; }
        }
        return vec4<f32>(mix(start, end, progress), 1.0, 1.0);
    }
    let scale = max(select(1.0 - progress, progress, incoming), 0.000001);
    var rect = vec4<f32>(0.0, 0.0, 1.0, 1.0);
    if (direction <= 1) {
        rect.w = scale;
        if (direction == 1) { rect.y = 1.0 - scale; }
    } else {
        rect.z = scale;
        if (direction == 3) { rect.x = 1.0 - scale; }
    }
    return rect;
}

fn scale_rect(mode: i32, scale_value: f32) -> vec4<f32> {
    let s = max(scale_value, 0.000001);
    var anchor = vec2<f32>(0.5, 0.5);
    var scale = vec2<f32>(s, s);
    switch mode {
        case 1: { anchor = vec2<f32>(0.0, 0.0); }
        case 2: { anchor = vec2<f32>(1.0, 0.0); }
        case 3: { anchor = vec2<f32>(0.0, 1.0); }
        case 4: { anchor = vec2<f32>(1.0, 1.0); }
        case 5: { scale.x = 1.0; }
        case 6: { scale.y = 1.0; }
        case 7: { anchor = vec2<f32>(0.0, 0.0); scale.x = 1.0; }
        case 8: { anchor = vec2<f32>(0.0, 1.0); scale.x = 1.0; }
        case 9: { anchor = vec2<f32>(0.0, 0.0); scale.y = 1.0; }
        case 10: { anchor = vec2<f32>(1.0, 0.0); scale.y = 1.0; }
        case 11: {
            anchor = vec2<f32>(option(2) / max(wipe.kind_progress.z, 1.0), option(3) / max(wipe.kind_progress.w, 1.0));
        }
        default: {}
    }
    return vec4<f32>(anchor * (vec2<f32>(1.0) - scale), scale);
}

fn scale_uv(mode: i32, rate_in: f32) -> vec4<f32> {
    var rate = rate_in;
    var vrate = rate_in;
    switch mode {
        case 0: { return vec4<f32>(0.5 - 0.5 * rate, 0.5 - 0.5 * vrate, rate, vrate); }
        case 1: { return vec4<f32>(0.0, 0.0, rate, vrate); }
        case 2: { return vec4<f32>(1.0 - rate, 0.0, rate, vrate); }
        case 3: { return vec4<f32>(0.0, 1.0 - vrate, rate, vrate); }
        case 4: { return vec4<f32>(1.0 - rate, 1.0 - vrate, rate, vrate); }
        case 5: { vrate = mix(0.49, 1.0, clamp(vrate, 0.0, 1.0)); return vec4<f32>(0.0, 1.0 - vrate, 1.0, 2.0 * vrate - 1.0); }
        case 6: { rate = mix(0.49, 1.0, clamp(rate, 0.0, 1.0)); return vec4<f32>(1.0 - rate, 0.0, 2.0 * rate - 1.0, 1.0); }
        case 7: { return vec4<f32>(0.0, 0.0, 1.0, vrate); }
        case 8: { return vec4<f32>(0.0, 1.0 - vrate, 1.0, vrate); }
        case 9: { return vec4<f32>(0.0, 0.0, rate, 1.0); }
        case 10: { return vec4<f32>(1.0 - rate, 0.0, rate, 1.0); }
        case 11: {
            let x = option(2) / max(wipe.kind_progress.z, 1.0);
            let y = option(3) / max(wipe.kind_progress.w, 1.0);
            return vec4<f32>(x - x * rate, y - y * vrate, rate, vrate);
        }
        default: { return vec4<f32>(0.0, 0.0, 1.0, 1.0); }
    }
}

fn sample_uv_box(tex: texture_2d<f32>, smp: sampler, uv: vec2<f32>, box: vec4<f32>, alpha: f32) -> vec4<f32> {
    let sample_uv = box.xy + uv * box.zw;
    let c = textureSample(tex, smp, clamp(sample_uv, vec2<f32>(0.0), vec2<f32>(1.0)));
    return vec4<f32>(c.rgb, c.a * alpha);
}

fn sample_mosaic(tex: texture_2d<f32>, smp: sampler, uv: vec2<f32>, size: f32) -> vec4<f32> {
    let dims = vec2<f32>(textureDimensions(tex));
    let cell = max(vec2<f32>(1.0), vec2<f32>(size));
    let px = floor(uv * dims / cell) * cell + 0.5 * cell;
    return textureSample(tex, smp, clamp(px / dims, vec2<f32>(0.0), vec2<f32>(1.0)));
}

fn explosion(tex: texture_2d<f32>, smp: sampler, uv: vec2<f32>, center: vec2<f32>, power: f32) -> vec4<f32> {
    var sum = vec4<f32>(0.0);
    let delta = (center - uv) * power / 16.0;
    for (var i = 0; i < 16; i = i + 1) {
        sum += textureSample(tex, smp, clamp(uv + delta * f32(i), vec2<f32>(0.0), vec2<f32>(1.0)));
    }
    return sum / 16.0;
}

fn raster_offset(uv: vec2<f32>, vertical: bool, fraction: f32, wave: f32, power: f32, progress: f32) -> vec2<f32> {
    let axis = select(uv.x, uv.y, vertical);
    let phase = axis * max(fraction, 1.0) * 6.28318530718 + progress * wave * 6.28318530718;
    let amp = sin(phase) * power;
    return select(vec2<f32>(amp, 0.0), vec2<f32>(0.0, amp), vertical);
}

fn wipe_shimi_source(color_in: vec4<f32>, fade: f32, progress: f32, reverse: bool) -> vec4<f32> {
    var color = color_in;
    let brightness = dot(vec3<f32>(0.299, 0.587, 0.114), color.rgb);
    let hide = select(brightness > progress, brightness < 1.0 - progress, reverse);
    if (hide) {
        color.a = color.a * max(fade * (1.0 - progress), 0.0);
    }
    return color;
}

fn triangular_parameter(kind: i32, reverse: bool, progress: f32) -> f32 {
    var value = 0.0;
    if (kind == 0) {
        value = 1.0 - progress;
    } else if (kind == 10) {
        value = progress;
    } else {
        let threshold = clamp(f32(kind) / 10.0, 0.000001, 0.999999);
        value = select(
            (1.0 - progress) / (1.0 - threshold),
            progress / threshold,
            progress < threshold,
        );
    }
    return clamp(select(value, 1.0 - value, reverse), 0.0, 1.0);
}

fn affected_color(uv: vec2<f32>) -> vec4<f32> {
    let kind = i32(round(wipe.kind_progress.x));
    let p = clamp(wipe.kind_progress.y, 0.0, 1.0);
    let current = textureSample(current_tex, current_smp, uv);
    let next = textureSample(next_tex, next_smp, uv);

    // After WIPE starts, FRONT is the newly prepared scene and NEXT is the
    // saved old scene.  C_tnm_wnd::disp_proc_wipe_for_cross_fade draws
    // under+NEXT first, then fades the under+FRONT wipe buffer in from
    // progress 0 to 255.  Types 1 and 2 are the corresponding fixed FRONT and
    // fixed NEXT modes.  These inputs are complete scenes, not isolated
    // transparent target layers.
    if (kind == 0) {
        if (p <= 0.0) { return next; }
        if (p >= 1.0) { return current; }
        return mix(next, current, p);
    }
    if (kind == 1) { return current; }
    if (kind == 2) { return next; }

    // C++ mask and raster wipes always transition from NEXT (the saved old
    // scene) to FRONT (the prepared new scene). Keep the legacy endpoint
    // behavior for the other effect families until each of those families is
    // audited independently against eng_disp_wipe.cpp.
    let old_to_new_family =
        (kind >= 5 && kind < 200) || kind == 900 || kind == 901 || kind == 220 || kind == 221;
    if (old_to_new_family) {
        if (p <= 0.0) { return next; }
        if (p >= 1.0) { return current; }
    } else {
        if (p <= 0.0) { return current; }
        if (p >= 1.0) { return next; }
    }
    if (kind == 200) {
        let direction = i32(option(0)) % 4;
        let cmode = i32(option(1));
        let nmode = i32(option(2));
        let c = rect_sample(current_tex, current_smp, uv, move_rect(direction, cmode, p, false), 1.0);
        let n = rect_sample(next_tex, next_smp, uv, move_rect(direction, nmode, p, true), 1.0);
        return select(alpha_over(n, c), alpha_over(c, n), cmode == 0);
    }
    if (kind == 210 || kind == 211) {
        let incoming = kind == 210;
        let base = select(current, next, incoming);
        let moving_tex_next = incoming;
        let rect = scale_rect(i32(option(0)), select(1.0 - p, p, incoming));
        let alpha = select(select(1.0, 1.0 - p, i32(option(1)) == 1), select(1.0, p, i32(option(1)) == 1), incoming);
        let moving = select(rect_sample(current_tex, current_smp, uv, rect, alpha), rect_sample(next_tex, next_smp, uv, rect, alpha), moving_tex_next);
        return alpha_over(base, moving);
    }
    if (kind == 212) {
        if (p < 0.5) { return sample_uv_box(current_tex, current_smp, uv, scale_uv(i32(option(0)), mix(1.0, 0.001, p * 2.0)), 1.0); }
        return sample_uv_box(next_tex, next_smp, uv, scale_uv(i32(option(0)), mix(0.001, 1.0, (p - 0.5) * 2.0)), 1.0);
    }
    if (kind == 213) {
        let n = sample_uv_box(next_tex, next_smp, uv, scale_uv(i32(option(0)), mix(0.333, 1.0, p)), p);
        return alpha_over(current, n);
    }
    if (kind == 214) {
        let c = sample_uv_box(current_tex, current_smp, uv, scale_uv(i32(option(0)), mix(1.0, 0.333, p)), 1.0 - p);
        return alpha_over(next, c);
    }
    if (kind == 215) {
        let sx = clamp(option(2) / max(wipe.kind_progress.z, 1.0), 0.0, 1.0);
        let sy = clamp(option(3) / max(wipe.kind_progress.w, 1.0), 0.0, 1.0);
        let ex = clamp(option(4) / max(wipe.kind_progress.z, 1.0), 0.0, 1.0);
        let ey = clamp(option(5) / max(wipe.kind_progress.w, 1.0), 0.0, 1.0);
        let specified = vec4<f32>(min(sx, ex), min(sy, ey), max(abs(ex - sx), 1.0 / max(wipe.kind_progress.z, 1.0)), max(abs(ey - sy), 1.0 / max(wipe.kind_progress.w, 1.0)));
        let alpha_mode = i32(option(0));
        let alpha = select(select(1.0, 1.0 - p, alpha_mode == 2), p, alpha_mode == 1);
        if (i32(option(1)) == 0) {
            let rect = mix(specified, vec4<f32>(0.0, 0.0, 1.0, 1.0), p);
            return alpha_over(current, rect_sample(next_tex, next_smp, uv, rect, alpha));
        }
        let rect = mix(vec4<f32>(0.0, 0.0, 1.0, 1.0), specified, p);
        return alpha_over(next, rect_sample(current_tex, current_smp, uv, rect, alpha));
    }
    if (kind == 220 || kind == 221) {
        let vertical = i32(option(0)) == 0;
        let dim = select(wipe.kind_progress.z, wipe.kind_progress.w, vertical);
        let fraction = dim / max(option(1), 1.0);
        if (kind == 220) {
            // C++ cross-raster renders NEXT (the pre-wipe/old scene) into
            // wipe buffer 1 and FRONT (the prepared/new scene) into buffer 0.
            // The transition therefore has to start at NEXT and finish at
            // FRONT. Keep both endpoints undistorted, with the opposite image
            // carrying the full raster displacement away from its endpoint.
            let offset = raster_offset(uv, vertical, fraction, option(2), option(3) / max(dim, 1.0), p);
            let old_scene = sample_or_zero(next_tex, next_smp, uv - offset * p);
            let new_scene = sample_or_zero(current_tex, current_smp, uv + offset * (1.0 - p));
            return mix(old_scene, new_scene, p);
        }

        // C++ single-raster chooses which stage is rendered directly and which
        // one is processed through the wipe buffer. option[4]==0 means
        // base=NEXT(old), processed=FRONT(new), wpf=p. option[4]!=0 means
        // base=FRONT(new), processed=NEXT(old), wpf=1-p. In both cases the
        // visible result progresses old -> new.
        let reverse = i32(option(4)) != 0;
        let t = select(p, 1.0 - p, reverse);
        let offset = raster_offset(uv, vertical, fraction, option(2), option(3) / max(dim, 1.0), t);
        if (!reverse) {
            let warped_new = sample_or_zero(current_tex, current_smp, uv + offset * (1.0 - t));
            return alpha_over(next, vec4<f32>(warped_new.rgb, warped_new.a * t));
        }
        let warped_old = sample_or_zero(next_tex, next_smp, uv + offset * (1.0 - t));
        return alpha_over(current, vec4<f32>(warped_old.rgb, warped_old.a * t));
    }
    if (kind == 230 || kind == 231) {
        if (kind == 230) {
            let first = p < 0.5;
            let local = select((p - 0.5) * 2.0, p * 2.0, first);
            let size = mix(1.0, max(option(0), 1.0), select(1.0 - local, local, first));
            return select(sample_mosaic(next_tex, next_smp, uv, size), sample_mosaic(current_tex, current_smp, uv, size), first);
        }
        let use_next = i32(option(1)) != 0;
        let size = mix(max(option(0), 1.0), 1.0, p);
        let src = select(sample_mosaic(current_tex, current_smp, uv, size), sample_mosaic(next_tex, next_smp, uv, size), use_next);
        return vec4<f32>(src.rgb, src.a * select(1.0 - p, p, use_next));
    }
    if (kind >= 240 && kind <= 243) {
        // C_tnm_wnd::disp_proc_wipe_for_explosion_blur_get_stage returns the
        // stage drawn underneath. The opposite stage is the sprite processed
        // by the explosion-blur technique.
        var base_is_current = false;
        if (kind == 241) { base_is_current = i32(option(7)) == 0; }
        if (kind == 243) { base_is_current = i32(option(5)) == 0; }
        let processed_is_next = base_is_current;
        let base = select(next, current, base_is_current);
        let processed = select(current, next, processed_is_next);

        let alpha_type = i32(select(option(0), option(2), kind <= 241));
        let alpha_reverse = i32(select(option(1), option(3), kind <= 241)) != 0;
        // The C++ value is rp.tr (transparency), so convert it to visible alpha.
        let visible_alpha = 1.0 - triangular_parameter(alpha_type, alpha_reverse, p);

        let power_type = i32(select(option(2), option(4), kind <= 241));
        let power_reverse = i32(select(option(3), option(5), kind <= 241)) != 0;
        let power = triangular_parameter(power_type, power_reverse, p);
        let coefficient = max(select(option(4), option(6), kind <= 241), 0.0);

        var center = vec2<f32>(0.5);
        if (kind <= 241) {
            center = vec2<f32>(
                option(0) / max(wipe.kind_progress.z, 1.0),
                option(1) / max(wipe.kind_progress.w, 1.0),
            );
        } else {
            let seed = option(15);
            center = fract(vec2<f32>(sin(seed * 12.9898), sin(seed * 78.233)) * 43758.5453);
        }
        let blurred = select(
            explosion(current_tex, current_smp, uv, center, power * coefficient),
            explosion(next_tex, next_smp, uv, center, power * coefficient),
            processed_is_next,
        );
        let processed_color = vec4<f32>(blurred.rgb, processed.a * visible_alpha);
        return alpha_over(base, processed_color);
    }
    if (kind == 50) {
        // Original type 50 draws NEXT first and then draws FRONT through
        // tec_tex1_shimi / tec_tex1_shimi_inv. option[0] selects the fade
        // constant and option[1] selects the inverse technique.
        let processed = wipe_shimi_source(
            current,
            mask_fade(i32(option(0))),
            p,
            i32(option(1)) != 0,
        );
        return alpha_over(next, processed);
    }
    if (kind == 901) {
        let max_root = sqrt(max(option(1) / 1000.0, 0.000001));
        let root_now = select(max_root * p, max_root * (1.0 - p), i32(option(0)) != 0);
        let scale = max(root_now * root_now, 0.000001);
        let angle = radians(360.0 * option(2) * p);
        let cs = cos(angle);
        let sn = sin(angle);
        let d = uv - vec2<f32>(0.5);
        var muv = vec2<f32>(cs * d.x + sn * d.y, -sn * d.x + cs * d.y) / scale + vec2<f32>(0.5);
        let cells = clamp(option(3), 0.0, 64.0);
        var mask_value = 1.0;
        if (cells > 0.0) {
            muv = fract(muv * cells);
            if (i32(option(4)) != 0 && (muv.x < 0.03 || muv.y < 0.03 || muv.x > 0.97 || muv.y > 0.97)) { mask_value = 1.0; }
            else { mask_value = luminance(textureSample(mask_tex, mask_smp, muv)); }
        } else if (inside(muv)) {
            mask_value = luminance(textureSample(mask_tex, mask_smp, muv));
        }
        return mix(current, next, select(0.0, 1.0, mask_value >= 0.5));
    }
    if (kind == 900 || (kind >= 5 && kind < 200)) {
        let mask_value = luminance(textureSample(mask_tex, mask_smp, uv));
        let reveal = mask_reveal(p, 1.0 - mask_value, mask_fade(i32(option(0))));
        // After C_elm_stage_list::wipe(), FRONT is the prepared/new scene and
        // NEXT is the saved pre-wipe/old scene. C++ disp_proc_wipe_for_mask()
        // draws NEXT first, then draws FRONT into wipe buffer 0 and reveals
        // that buffer through the mask. Therefore reveal=0 must show NEXT and
        // reveal=1 must show FRONT. The previous order was exactly reversed,
        // which is especially obvious for blind masks (102/122/142/152).
        return mix(next, current, reveal);
    }
    return mix(current, next, p);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let uv = clamp(in.uv, vec2<f32>(0.0), vec2<f32>(1.0));
    let kind = i32(round(wipe.kind_progress.x));
    if (kind == 0 || kind == 1 || kind == 2) {
        return affected_color(uv);
    }
    let under = textureSample(under_tex, under_smp, uv);
    return alpha_over(under, affected_color(uv));
}
"#;

const SHADER: &str = r#"
struct VsIn {
  @location(0) pos: vec3<f32>,
  @location(1) uv: vec2<f32>,
  @location(2) alpha: f32,
  @location(3) vertex_color_rgb: vec4<f32>,
  @location(4) vertex_color_alpha: vec4<f32>,
  @location(5) world_normal: vec4<f32>,
  @location(6) world_tangent: vec4<f32>,
  @location(7) world_binormal: vec4<f32>,
  @location(8) bone_indices: vec4<f32>,
  @location(9) bone_weights: vec4<f32>,
};

struct VsIn2d {
  @location(0) pos: vec3<f32>,
  @location(1) uv: vec2<f32>,
  @location(2) uv_aux: vec2<f32>,
  @location(3) alpha: f32,
};

struct VsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) alpha: f32,
  @location(2) vertex_color: vec4<f32>,
  @location(3) world_pos: vec4<f32>,
  @location(4) world_normal: vec4<f32>,
  @location(5) world_tangent: vec4<f32>,
  @location(6) world_binormal: vec4<f32>,
  @location(7) shadow_pos: vec4<f32>,
  @location(8) proj_pos: vec4<f32>,
};

struct VsOut2d {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) uv_aux: vec2<f32>,
  @location(2) alpha: f32,
};

struct ShadowVsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) depth: f32,
  @location(1) uv: vec2<f32>,
  @location(2) alpha_test: f32,
};

struct VsUniform {
  model_col0: vec4<f32>,
  model_col1: vec4<f32>,
  model_col2: vec4<f32>,
  model_col3: vec4<f32>,
  normal_col0: vec4<f32>,
  normal_col1: vec4<f32>,
  normal_col2: vec4<f32>,
  frame_col0: vec4<f32>,
  frame_col1: vec4<f32>,
  frame_col2: vec4<f32>,
  frame_col3: vec4<f32>,
  frame_normal0: vec4<f32>,
  frame_normal1: vec4<f32>,
  frame_normal2: vec4<f32>,
  camera_eye: vec4<f32>,
  camera_forward: vec4<f32>,
  camera_right: vec4<f32>,
  camera_up: vec4<f32>,
  camera_params: vec4<f32>,
  shadow_eye: vec4<f32>,
  shadow_forward: vec4<f32>,
  shadow_right: vec4<f32>,
  shadow_up: vec4<f32>,
  shadow_params: vec4<f32>,
  mtrl_diffuse: vec4<f32>,
  mtrl_ambient: vec4<f32>,
  mtrl_specular: vec4<f32>,
  mtrl_emissive: vec4<f32>,
  mtrl_params: vec4<f32>,
  mtrl_rim: vec4<f32>,
  mtrl_extra: vec4<f32>,
  light_diffuse_u: vec4<f32>,
  light_ambient_u: vec4<f32>,
  light_specular_u: vec4<f32>,
  sprite_effects: array<vec4<f32>, 11>,
  single_light_pos_kind: vec4<f32>,
  single_light_dir_shadow: vec4<f32>,
  single_light_atten: vec4<f32>,
  single_light_cone: vec4<f32>,
  mesh_flags: vec4<f32>,
  mesh_mrbd: vec4<f32>,
  mesh_rgb_rate: vec4<f32>,
  mesh_add_rgb: vec4<f32>,
  mesh_misc: vec4<f32>,
  mesh_light_counts: vec4<f32>,
  dir_light_diffuse: array<vec4<f32>, 4>,
  dir_light_ambient: array<vec4<f32>, 4>,
  dir_light_specular: array<vec4<f32>, 4>,
  dir_light_dir: array<vec4<f32>, 4>,
  point_light_diffuse: array<vec4<f32>, 4>,
  point_light_ambient: array<vec4<f32>, 4>,
  point_light_specular: array<vec4<f32>, 4>,
  point_light_pos: array<vec4<f32>, 4>,
  point_light_atten: array<vec4<f32>, 4>,
  spot_light_diffuse: array<vec4<f32>, 4>,
  spot_light_ambient: array<vec4<f32>, 4>,
  spot_light_specular: array<vec4<f32>, 4>,
  spot_light_pos: array<vec4<f32>, 4>,
  spot_light_dir: array<vec4<f32>, 4>,
  spot_light_atten: array<vec4<f32>, 4>,
  spot_light_cone: array<vec4<f32>, 4>,
  flags: vec4<f32>,
};

struct BoneUniform {
  matrices: array<mat4x4<f32>, 64>,
};

@group(0) @binding(10) var shadow_tex: texture_2d<f32>;
@group(0) @binding(11) var shadow_smp: sampler;
@group(0) @binding(12) var<uniform> vs_u: VsUniform;
@group(0) @binding(13) var<uniform> bone_u: BoneUniform;

fn apply_model(local: vec3<f32>) -> vec3<f32> {
  return vs_u.model_col0.xyz * local.x + vs_u.model_col1.xyz * local.y + vs_u.model_col2.xyz * local.z + vs_u.model_col3.xyz;
}

fn apply_normal(local: vec3<f32>) -> vec3<f32> {
  let n = vs_u.normal_col0.xyz * local.x + vs_u.normal_col1.xyz * local.y + vs_u.normal_col2.xyz * local.z;
  if (length(n) <= 1e-6) {
    return vec3<f32>(0.0, 0.0, 1.0);
  }
  return normalize(n);
}

fn apply_frame(local: vec3<f32>) -> vec3<f32> {
  return vs_u.frame_col0.xyz * local.x + vs_u.frame_col1.xyz * local.y + vs_u.frame_col2.xyz * local.z + vs_u.frame_col3.xyz;
}

fn apply_frame_normal(local: vec3<f32>) -> vec3<f32> {
  let n = vs_u.frame_normal0.xyz * local.x + vs_u.frame_normal1.xyz * local.y + vs_u.frame_normal2.xyz * local.z;
  if (length(n) <= 1e-6) {
    return vec3<f32>(0.0, 0.0, 1.0);
  }
  return normalize(n);
}

fn apply_bone_point(m: mat4x4<f32>, local: vec3<f32>) -> vec3<f32> {
  return m[0].xyz * local.x + m[1].xyz * local.y + m[2].xyz * local.z + m[3].xyz;
}

fn skin_local(local: vec3<f32>, bone_indices: vec4<f32>, bone_weights: vec4<f32>) -> vec3<f32> {
  let sum_w = bone_weights.x + bone_weights.y + bone_weights.z + bone_weights.w;
  if (vs_u.flags.w <= 0.5 || sum_w <= 1e-6) {
    return apply_frame(local);
  }
  var out = vec3<f32>(0.0, 0.0, 0.0);
  if (bone_weights.x > 0.0) {
    let m = bone_u.matrices[min(u32(max(bone_indices.x, 0.0)), 63u)];
    out = out + apply_bone_point(m, local) * bone_weights.x;
  }
  if (bone_weights.y > 0.0) {
    let m = bone_u.matrices[min(u32(max(bone_indices.y, 0.0)), 63u)];
    out = out + apply_bone_point(m, local) * bone_weights.y;
  }
  if (bone_weights.z > 0.0) {
    let m = bone_u.matrices[min(u32(max(bone_indices.z, 0.0)), 63u)];
    out = out + apply_bone_point(m, local) * bone_weights.z;
  }
  if (bone_weights.w > 0.0) {
    let m = bone_u.matrices[min(u32(max(bone_indices.w, 0.0)), 63u)];
    out = out + apply_bone_point(m, local) * bone_weights.w;
  }
  return out;
}

fn skin_normal(local: vec3<f32>, bone_indices: vec4<f32>, bone_weights: vec4<f32>) -> vec3<f32> {
  let sum_w = bone_weights.x + bone_weights.y + bone_weights.z + bone_weights.w;
  if (vs_u.flags.w <= 0.5 || sum_w <= 1e-6) {
    return apply_frame_normal(local);
  }
  var out = vec3<f32>(0.0, 0.0, 0.0);
  if (bone_weights.x > 0.0) {
    let m = bone_u.matrices[min(u32(max(bone_indices.x, 0.0)), 63u)];
    out = out + (m[0].xyz * local.x + m[1].xyz * local.y + m[2].xyz * local.z) * bone_weights.x;
  }
  if (bone_weights.y > 0.0) {
    let m = bone_u.matrices[min(u32(max(bone_indices.y, 0.0)), 63u)];
    out = out + (m[0].xyz * local.x + m[1].xyz * local.y + m[2].xyz * local.z) * bone_weights.y;
  }
  if (bone_weights.z > 0.0) {
    let m = bone_u.matrices[min(u32(max(bone_indices.z, 0.0)), 63u)];
    out = out + (m[0].xyz * local.x + m[1].xyz * local.y + m[2].xyz * local.z) * bone_weights.z;
  }
  if (bone_weights.w > 0.0) {
    let m = bone_u.matrices[min(u32(max(bone_indices.w, 0.0)), 63u)];
    out = out + (m[0].xyz * local.x + m[1].xyz * local.y + m[2].xyz * local.z) * bone_weights.w;
  }
  if (length(out) <= 1e-6) {
    return vec3<f32>(0.0, 0.0, 1.0);
  }
  return normalize(out);
}

fn project_main(world: vec3<f32>) -> vec4<f32> {
  if (vs_u.flags.y > 0.5) {
    let rel = world - vs_u.camera_eye.xyz;
    let cx = dot(rel, vs_u.camera_right.xyz);
    let cy = dot(rel, vs_u.camera_up.xyz);
    let cz = dot(rel, vs_u.camera_forward.xyz);
    if (cz <= 1e-3) {
      return vec4<f32>(2.0, 2.0, 2.0, 1.0);
    }
    let x_ndc = cx / (cz * max(vs_u.camera_params.x, 1e-3));
    let y_ndc = cy / (cz * max(vs_u.camera_params.y, 1e-3));
    let z_ndc = clamp((cz - 1.0) / 10000.0, 0.0, 1.0);
    return vec4<f32>(x_ndc, y_ndc, z_ndc, 1.0);
  }
  let x_ndc = (world.x / max(vs_u.camera_params.z, 1.0)) * 2.0 - 1.0;
  let y_ndc = 1.0 - (world.y / max(vs_u.camera_params.w, 1.0)) * 2.0;
  let z_ndc = clamp(-world.z / 50000.0, 0.0, 1.0);
  return vec4<f32>(x_ndc, y_ndc, z_ndc, 1.0);
}

fn project_shadow(world: vec3<f32>) -> vec4<f32> {
  if (vs_u.shadow_params.z <= 0.5) {
    return vec4<f32>(0.0, 0.0, 1.0, 1.0);
  }
  let rel = world - vs_u.shadow_eye.xyz;
  let cx = dot(rel, vs_u.shadow_right.xyz);
  let cy = dot(rel, vs_u.shadow_up.xyz);
  let cz = dot(rel, vs_u.shadow_forward.xyz);
  if (cz <= 1e-3) {
    return vec4<f32>(0.0, 0.0, 1.0, 1.0);
  }
  let x_ndc = cx / (cz * max(vs_u.shadow_params.x, 1e-3));
  let y_ndc = cy / (cz * max(vs_u.shadow_params.x, 1e-3));
  let depth = clamp(cz / max(vs_u.shadow_params.y, 1.0), 0.0, 1.0);
  return vec4<f32>(x_ndc, y_ndc, depth, 1.0);
}

fn vs_common(v: VsIn) -> VsOut {
  var o: VsOut;
  let local_world = skin_local(v.pos, v.bone_indices, v.bone_weights);
  let local_normal = skin_normal(v.world_normal.xyz, v.bone_indices, v.bone_weights);
  let local_tangent = skin_normal(v.world_tangent.xyz, v.bone_indices, v.bone_weights);
  let local_binormal = skin_normal(v.world_binormal.xyz, v.bone_indices, v.bone_weights);
  let world = apply_model(local_world);
  let normal = apply_normal(local_normal);
  let tangent = apply_normal(local_tangent);
  let binormal = apply_normal(local_binormal);
  o.pos = project_main(world);
  o.proj_pos = o.pos;
  o.uv = v.uv;
  o.alpha = v.alpha;
  o.vertex_color = vec4<f32>(
    v.vertex_color_rgb.xyz,
    v.vertex_color_alpha.x,
  );
  o.world_pos = vec4<f32>(world, 1.0);
  o.world_normal = vec4<f32>(normal, 1.0);
  o.world_tangent = vec4<f32>(tangent, 0.0);
  o.world_binormal = vec4<f32>(binormal, 0.0);
  o.shadow_pos = project_shadow(world);
  return o;
}

fn vs_shadow_common(v: VsIn) -> ShadowVsOut {
  var o: ShadowVsOut;
  let local_world = skin_local(v.pos, v.bone_indices, v.bone_weights);
  let world = apply_model(local_world);
  let shadow = project_shadow(world);
  o.pos = vec4<f32>(shadow.xyz, 1.0);
  o.depth = clamp(shadow.z / max(abs(shadow.w), 1e-6), 0.0, 1.0);
  o.uv = v.uv;
  o.alpha_test = vs_u.sprite_effects[3].y;
  return o;
}

fn vs_common_2d(v: VsIn2d) -> VsOut2d {
  var o: VsOut2d;
  o.pos = vec4<f32>(v.pos, 1.0);
  o.uv = v.uv;
  o.uv_aux = v.uv_aux;
  o.alpha = v.alpha;
  return o;
}

@group(0) @binding(0) var tex0: texture_2d<f32>;
@group(0) @binding(1) var smp0: sampler;
@group(0) @binding(2) var tex1: texture_2d<f32>;
@group(0) @binding(3) var smp1: sampler;
@group(0) @binding(4) var tex2: texture_2d<f32>;
@group(0) @binding(5) var smp2: sampler;
@group(0) @binding(6) var tex3: texture_2d<f32>;
@group(0) @binding(7) var smp3: sampler;
@group(0) @binding(8) var tex4: texture_2d<f32>;
@group(0) @binding(9) var smp4: sampler;
@group(0) @binding(14) var tex5: texture_2d<f32>;
@group(0) @binding(15) var smp5: sampler;
@group(0) @binding(16) var tex6: texture_2d<f32>;
@group(0) @binding(17) var smp6: sampler;
fn sample_mask(uv: vec2<f32>) -> vec4<f32> {
  if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0) {
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
  }
  return textureSampleLevel(tex1, smp1, uv, 0.0);
}

fn apply_tonecurve_from_mono(color_in: vec3<f32>, mono_y: f32, row: f32, sat: f32) -> vec3<f32> {
  // shader.cfx computes mono_y before tonecurve/reverse and then uses that
  // preserved value for saturation reduction.  CLAMP is supplied by smp2.
  var color = mix(color_in, vec3<f32>(mono_y, mono_y, mono_y), sat);
  let r = textureSampleLevel(tex2, smp2, vec2<f32>(color.r, row), 0.0).r;
  let g = textureSampleLevel(tex2, smp2, vec2<f32>(color.g, row), 0.0).g;
  let b = textureSampleLevel(tex2, smp2, vec2<f32>(color.b, row), 0.0).b;
  return vec3<f32>(r, g, b);
}

fn apply_tonecurve(color_in: vec3<f32>, row: f32, sat: f32) -> vec3<f32> {
  let mono_y = dot(color_in, vec3<f32>(0.2989, 0.5886, 0.1145));
  return apply_tonecurve_from_mono(color_in, mono_y, row, sat);
}

fn sample_tex0_safe(uv: vec2<f32>) -> vec4<f32> {
  if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0) {
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
  }
  return textureSample(tex0, smp0, uv);
}

fn sample_tex3_safe(uv: vec2<f32>) -> vec4<f32> {
  if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0) {
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
  }
  return textureSample(tex3, smp3, uv);
}

fn sample_tex4_safe(uv: vec2<f32>) -> vec4<f32> {
  if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0) {
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
  }
  return textureSample(tex4, smp4, uv);
}

fn sample_mosaic_tex3(uv: vec2<f32>, cut_u: f32, tex_rate_for_square: f32) -> vec4<f32> {
  let cu = max(cut_u, 1e-5);
  let cv = max(cut_u * max(tex_rate_for_square, 1e-5), 1e-5);
  let tc = vec2<f32>(floor(uv.x / cu) * cu, floor(uv.y / cv) * cv);
  return sample_tex3_safe(tc);
}

fn raster_amp(progress: f32) -> f32 {
  let rp = clamp(1.0 - progress, 1e-4, 1.0);
  let lv = max((1.0 - rp) * 100.0, 1e-4);
  return 1.0 - ((log(lv) / log(10.0)) + 1.0) / 3.0;
}

fn sample_raster_h_tex3(uv: vec2<f32>, fraction_num: f32, wave_num: f32, power: f32, progress: f32) -> vec4<f32> {
  let fnn = max(fraction_num, 1.0);
  var tex_coord_for_sin = uv.y * fnn;
  tex_coord_for_sin = fract(tex_coord_for_sin);
  tex_coord_for_sin = (tex_coord_for_sin - fnn * 0.1) / fnn;
  let dx = sin(3.14159265 * progress * power + tex_coord_for_sin * 3.14159265 * wave_num) * raster_amp(progress);
  return sample_tex3_safe(vec2<f32>(uv.x + dx, uv.y));
}

fn sample_raster_v_tex3(uv: vec2<f32>, fraction_num: f32, wave_num: f32, power: f32, progress: f32) -> vec4<f32> {
  let fnn = max(fraction_num, 1.0);
  var tex_coord_for_sin = uv.x * fnn;
  tex_coord_for_sin = fract(tex_coord_for_sin);
  tex_coord_for_sin = (tex_coord_for_sin - fnn * 0.1) / fnn;
  let dy = sin(3.14159265 * progress * power + tex_coord_for_sin * 3.14159265 * wave_num) * raster_amp(progress);
  return sample_tex3_safe(vec2<f32>(uv.x, uv.y + dy));
}

fn sample_explosion_blur_tex3(uv: vec2<f32>, center_uv: vec2<f32>, blur_power: f32, blur_coeff: f32) -> vec4<f32> {
  let dims_u = textureDimensions(tex3, 0);
  let dims = vec2<f32>(f32(dims_u.x), f32(dims_u.y));
  let texel = 1.0 / max(max(dims.x, dims.y), 1.0);
  var dir = center_uv - uv;
  let len = length(dir);
  if (len <= 1e-5 || blur_power <= 1e-5) {
    return sample_tex3_safe(uv);
  }
  dir = normalize(dir) * texel * blur_power * len * max(blur_coeff, 0.0);
  return
      sample_tex3_safe(uv) * 0.19 +
      sample_tex3_safe(uv + dir * 1.0) * 0.17 +
      sample_tex3_safe(uv + dir * 2.0) * 0.15 +
      sample_tex3_safe(uv + dir * 3.0) * 0.13 +
      sample_tex3_safe(uv + dir * 4.0) * 0.11 +
      sample_tex3_safe(uv + dir * 5.0) * 0.09 +
      sample_tex3_safe(uv + dir * 6.0) * 0.07 +
      sample_tex3_safe(uv + dir * 7.0) * 0.05 +
      sample_tex3_safe(uv + dir * 8.0) * 0.03 +
      sample_tex3_safe(uv + dir * 9.0) * 0.01;
}

fn sample_mosaic(uv: vec2<f32>, cut_u: f32, tex_rate_for_square: f32) -> vec4<f32> {
  let cu = max(cut_u, 1e-5);
  let cv = max(cut_u * max(tex_rate_for_square, 1e-5), 1e-5);
  let tc = vec2<f32>(floor(uv.x / cu) * cu, floor(uv.y / cv) * cv);
  return sample_tex0_safe(tc);
}

fn sample_raster_h(uv: vec2<f32>, fraction_num: f32, wave_num: f32, power: f32, progress: f32) -> vec4<f32> {
  let fnn = max(fraction_num, 1.0);
  var tex_coord_for_sin = uv.y * fnn;
  tex_coord_for_sin = fract(tex_coord_for_sin);
  tex_coord_for_sin = (tex_coord_for_sin - fnn * 0.1) / fnn;
  let dx = sin(3.14159265 * progress * power + tex_coord_for_sin * 3.14159265 * wave_num) * raster_amp(progress);
  return sample_tex0_safe(vec2<f32>(uv.x + dx, uv.y));
}

fn sample_raster_v(uv: vec2<f32>, fraction_num: f32, wave_num: f32, power: f32, progress: f32) -> vec4<f32> {
  let fnn = max(fraction_num, 1.0);
  var tex_coord_for_sin = uv.x * fnn;
  tex_coord_for_sin = fract(tex_coord_for_sin);
  tex_coord_for_sin = (tex_coord_for_sin - fnn * 0.1) / fnn;
  let dy = sin(3.14159265 * progress * power + tex_coord_for_sin * 3.14159265 * wave_num) * raster_amp(progress);
  return sample_tex0_safe(vec2<f32>(uv.x, uv.y + dy));
}

fn sample_explosion_blur(uv: vec2<f32>, center_uv: vec2<f32>, blur_power: f32, blur_coeff: f32) -> vec4<f32> {
  let dims_u = textureDimensions(tex0, 0);
  let dims = vec2<f32>(f32(dims_u.x), f32(dims_u.y));
  let texel = 1.0 / max(max(dims.x, dims.y), 1.0);
  var dir = center_uv - uv;
  let len = length(dir);
  if (len <= 1e-5 || blur_power <= 1e-5) {
    return sample_tex0_safe(uv);
  }
  dir = normalize(dir) * texel * blur_power * len * max(blur_coeff, 0.0);
  return
      sample_tex0_safe(uv) * 0.19 +
      sample_tex0_safe(uv + dir * 1.0) * 0.17 +
      sample_tex0_safe(uv + dir * 2.0) * 0.15 +
      sample_tex0_safe(uv + dir * 3.0) * 0.13 +
      sample_tex0_safe(uv + dir * 4.0) * 0.11 +
      sample_tex0_safe(uv + dir * 5.0) * 0.09 +
      sample_tex0_safe(uv + dir * 6.0) * 0.07 +
      sample_tex0_safe(uv + dir * 7.0) * 0.05 +
      sample_tex0_safe(uv + dir * 8.0) * 0.03 +
      sample_tex0_safe(uv + dir * 9.0) * 0.01;
}

fn rgb_brightness(color: vec4<f32>) -> f32 {
  return dot(vec3<f32>(0.2989, 0.5886, 0.1145), color.rgb);
}

fn sample_shimi(uv: vec2<f32>, fade_multiplier: f32, threshold: f32) -> vec4<f32> {
  var color = sample_tex0_safe(uv);
  // tec_tex1_shimi: pixels whose luminance is at or below c1.w have
  // their alpha multiplied by c2.x. RGB is not modified.
  if (rgb_brightness(color) <= threshold) {
    color.a = color.a * fade_multiplier;
  }
  return color;
}

fn sample_shimi_inv(uv: vec2<f32>, fade_multiplier: f32, threshold: f32) -> vec4<f32> {
  var color = sample_tex0_safe(uv);
  // tec_tex1_shimi_inv performs the complementary comparison.
  if (rgb_brightness(color) >= threshold) {
    color.a = color.a * fade_multiplier;
  }
  return color;
}

fn overlay_channel(dst: f32, src: f32) -> f32 {
  if (dst <= 0.5) {
    return 2.0 * dst * src;
  }
  return 1.0 - 2.0 * (1.0 - dst) * (1.0 - src);
}

fn overlay_rgb(dst: vec3<f32>, src: vec3<f32>) -> vec3<f32> {
  return vec3<f32>(
    overlay_channel(dst.r, src.r),
    overlay_channel(dst.g, src.g),
    overlay_channel(dst.b, src.b)
  );
}

fn sample_normal_tex(uv: vec2<f32>) -> vec3<f32> {
  if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0) {
    return vec3<f32>(0.5, 0.5, 1.0);
  }
  let dims_u = textureDimensions(tex5, 0);
  if (dims_u.x <= 1u && dims_u.y <= 1u) {
    return vec3<f32>(0.5, 0.5, 1.0);
  }
  return textureSample(tex5, smp5, uv).xyz;
}

fn sample_toon_tex(value: f32) -> vec3<f32> {
  let u = clamp(value, 0.0, 1.0);
  let dims_u = textureDimensions(tex6, 0);
  if (dims_u.x <= 1u && dims_u.y <= 1u) {
    let q = floor(u * 4.0) / 3.0;
    return vec3<f32>(q, q, q);
  }
  return textureSample(tex6, smp6, vec2<f32>(u, 0.5)).rgb;
}

fn apply_parallax_uv(base_n: vec3<f32>, base_t: vec3<f32>, base_b: vec3<f32>, uv: vec2<f32>, view_dir_world: vec3<f32>, max_height: f32) -> vec2<f32> {
  let dims_u = textureDimensions(tex5, 0);
  if (dims_u.x <= 1u && dims_u.y <= 1u || max_height <= 1e-6) {
    return uv;
  }
  let N = normalize(base_n);
  var T = normalize(base_t);
  var B = normalize(base_b);
  if (length(T) <= 1e-5 || length(B) <= 1e-5) {
    let up = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 1.0, 0.0), abs(N.z) > 0.9);
    T = normalize(cross(up, N));
    B = normalize(cross(N, T));
  }
  let Vt = vec3<f32>(dot(view_dir_world, T), dot(view_dir_world, B), dot(view_dir_world, N));
  let height = textureSample(tex5, smp5, uv).a;
  let denom = select(-1e-4, Vt.z, abs(Vt.z) > 1e-4);
  let shift = (height - 0.5) * max_height;
  return uv + (Vt.xy / denom) * shift;
}

fn apply_normal_map(base_n: vec3<f32>, base_t: vec3<f32>, base_b: vec3<f32>, uv: vec2<f32>) -> vec3<f32> {
  let tex_n = sample_normal_tex(uv) * 2.0 - vec3<f32>(1.0, 1.0, 1.0);
  let N = normalize(base_n);
  var T = normalize(base_t);
  var B = normalize(base_b);
  if (length(T) <= 1e-5 || length(B) <= 1e-5) {
    let up = select(vec3<f32>(0.0, 0.0, 1.0), vec3<f32>(0.0, 1.0, 0.0), abs(N.z) > 0.9);
    T = normalize(cross(up, N));
    B = normalize(cross(N, T));
  }
  let mapped = normalize(T * tex_n.x + B * tex_n.y + N * tex_n.z);
  return mapped;
}

fn sample_shadow_visibility(shadow_pos: vec4<f32>) -> f32 {
  let ndc = shadow_pos.xyz / max(abs(shadow_pos.w), 1e-5);
  let uv = vec2<f32>(ndc.x * 0.5 + 0.5, 1.0 - (ndc.y * 0.5 + 0.5));
  // The tona3 shadow sampler is POINT with a white BORDER. Emulate the
  // border explicitly because portable WebGPU border samplers are limited.
  if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0) {
    return 1.0;
  }
  let current = clamp(ndc.z, 0.0, 1.0);
  let stored = textureSampleLevel(shadow_tex, shadow_smp, uv, 0.0).r;
  let bias = max(vs_u.mesh_misc.y, 0.0);
  // Original code: shadow * Depth.w < Depth.z - bias.
  return select(0.0, 1.0, current - bias <= stored);
}

fn mesh_light_contrib(
  base_rgb: vec3<f32>,
  world_pos: vec3<f32>,
  N: vec3<f32>,
  shaded_uv: vec2<f32>,
  light_diffuse: vec3<f32>,
  light_ambient: vec3<f32>,
  light_specular: vec3<f32>,
  kind: i32,
  light_pos: vec3<f32>,
  light_dir: vec3<f32>,
  light_atten: vec4<f32>,
  light_cone: vec4<f32>,
  shadow_pos: vec4<f32>,
  shadow_enabled: bool
) -> vec3<f32> {
  let lighting_type = i32(round(vs_u.mtrl_params.y));
  let shading_type = i32(round(vs_u.mtrl_params.z));
  let mtrl_ambient = vs_u.mtrl_ambient.rgb;
  let mtrl_specular = vs_u.mtrl_specular.rgb;
  let mtrl_power = max(vs_u.mtrl_params.x, 1.0);

  var L = normalize(-light_dir);
  var distance_attenuation = 1.0;
  var spot_power = 1.0;
  if (kind != 0) {
    let dir_point = light_pos - world_pos;
    let distance_point = max(length(dir_point), 1e-5);
    L = dir_point / distance_point;
    distance_attenuation = 1.0 / max(
      light_atten.x + light_atten.y * distance_point + light_atten.z * distance_point * distance_point,
      1e-5
    );
    if (light_atten.w > 0.0) {
      distance_attenuation = distance_attenuation * clamp(1.0 - distance_point / light_atten.w, 0.0, 1.0);
    }
    if (kind >= 2) {
      let rho = dot(L, normalize(-light_dir));
      if (rho >= light_cone.x) {
        spot_power = 1.0;
      } else if (rho <= light_cone.y) {
        spot_power = 0.0;
      } else {
        spot_power = pow(
          (rho - light_cone.y) / max(light_cone.x - light_cone.y, 1e-5),
          max(light_cone.z, 0.01)
        );
      }
    }
  }

  let V = normalize(vs_u.camera_eye.xyz - world_pos);
  let H = normalize(L + V);
  let ndotl_raw = dot(N, L);
  let ndotl = max(ndotl_raw, 0.0);
  // tona3 half-Lambert squares the remapped term.
  let half_lambert = pow(clamp(ndotl_raw * 0.5 + 0.5, 0.0, 1.0), 2.0);
  let ndoth = max(dot(N, H), 0.0);
  let rdotv = max(dot(reflect(-L, N), V), 0.0);

  var visibility = 1.0;
  if (shadow_enabled && (shading_type == 1 || kind == 3)) {
    visibility = sample_shadow_visibility(shadow_pos);
  }

  let ambient_term = base_rgb * mtrl_ambient * light_ambient;
  var diffuse_strength = ndotl;
  if (lighting_type == 4) {
    diffuse_strength = half_lambert;
  }

  var diffuse_term = base_rgb * light_diffuse * diffuse_strength * distance_attenuation * spot_power;
  if (lighting_type == 5) {
    // Original toon coordinate is 0.0001 + mean RGB light brightness.
    let lbrightness = light_ambient + light_diffuse * diffuse_strength * distance_attenuation * spot_power;
    let toon = 0.0001 + (lbrightness.x + lbrightness.y + lbrightness.z) * 0.333;
    diffuse_term = base_rgb * sample_toon_tex(toon);
  }

  var specular_strength = pow(ndoth, mtrl_power);
  if (lighting_type == 6 || lighting_type == 7) {
    specular_strength = pow(rdotv, mtrl_power);
  }
  if (lighting_type == 0 || lighting_type == 1 || lighting_type == 4 || lighting_type == 5) {
    specular_strength = 0.0;
  }

  // The per-pixel FFP generator deliberately omits SpotPower from the
  // specular accumulation; vertex FFP includes it.
  var specular_spot = spot_power;
  if (lighting_type == 7) {
    specular_spot = 1.0;
  }
  let specular_term = mtrl_specular * light_specular * specular_strength * distance_attenuation * specular_spot;
  return ambient_term + (diffuse_term + specular_term) * visibility;
}

fn mesh_lighting(
  base_rgb: vec3<f32>,
  world_pos: vec3<f32>,
  world_normal: vec3<f32>,
  world_tangent: vec3<f32>,
  world_binormal: vec3<f32>,
  shaded_uv: vec2<f32>,
  shadow_pos: vec4<f32>
) -> vec3<f32> {
  let lighting_type = i32(round(vs_u.mtrl_params.y));
  let rim_power = max(vs_u.mtrl_params.w, 0.0);
  var N = normalize(world_normal);
  if (lighting_type == 8 || lighting_type == 9) {
    N = apply_normal_map(N, world_tangent, world_binormal, shaded_uv);
  }

  var accum = vs_u.mtrl_emissive.rgb;
  let dir_count = i32(round(vs_u.mesh_light_counts.x));
  let point_count = i32(round(vs_u.mesh_light_counts.y));
  let spot_count = i32(round(vs_u.mesh_light_counts.z));
  if (dir_count + point_count + spot_count > 0) {
    for (var li: i32 = 0; li < 4; li = li + 1) {
      if (li < dir_count) {
        accum = accum + mesh_light_contrib(
          base_rgb, world_pos, N, shaded_uv,
          vs_u.dir_light_diffuse[li].rgb, vs_u.dir_light_ambient[li].rgb, vs_u.dir_light_specular[li].rgb,
          0, vec3<f32>(0.0), vs_u.dir_light_dir[li].xyz,
          vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0), shadow_pos, false
        );
      }
      if (li < point_count) {
        accum = accum + mesh_light_contrib(
          base_rgb, world_pos, N, shaded_uv,
          vs_u.point_light_diffuse[li].rgb, vs_u.point_light_ambient[li].rgb, vs_u.point_light_specular[li].rgb,
          1, vs_u.point_light_pos[li].xyz, vec3<f32>(0.0, 0.0, -1.0),
          vs_u.point_light_atten[li], vec4<f32>(0.0), shadow_pos, false
        );
      }
      if (li < spot_count) {
        let receives_shadow = vs_u.spot_light_cone[li].w > 0.5;
        accum = accum + mesh_light_contrib(
          base_rgb, world_pos, N, shaded_uv,
          vs_u.spot_light_diffuse[li].rgb, vs_u.spot_light_ambient[li].rgb, vs_u.spot_light_specular[li].rgb,
          select(2, 3, receives_shadow), vs_u.spot_light_pos[li].xyz, vs_u.spot_light_dir[li].xyz,
          vs_u.spot_light_atten[li], vs_u.spot_light_cone[li], shadow_pos, receives_shadow
        );
      }
    }
  } else {
    let kind = i32(round(vs_u.single_light_pos_kind.w));
    accum = accum + mesh_light_contrib(
      base_rgb, world_pos, N, shaded_uv,
      vs_u.light_diffuse_u.rgb, vs_u.light_ambient_u.rgb, vs_u.light_specular_u.rgb,
      kind, vs_u.single_light_pos_kind.xyz, vs_u.single_light_dir_shadow.xyz,
      vs_u.single_light_atten, vs_u.single_light_cone, shadow_pos,
      kind == 3 && vs_u.single_light_dir_shadow.w > 0.5
    );
  }

  let shader_option_bits = i32(round(vs_u.mtrl_extra.z));
  if (rim_power > 0.0 && (shader_option_bits & 1) != 0) {
    let V = normalize(vs_u.camera_eye.xyz - world_pos);
    let rim = pow(clamp(1.0 - max(dot(N, V), 0.0), 0.0, 1.0), max(rim_power, 1e-3));
    accum = accum + vs_u.mtrl_rim.rgb * rim;
  }
  return accum;
}

fn fs_common_2d(i: VsOut2d) -> vec4<f32> {
  let e1 = vs_u.sprite_effects[0];
  let e2 = vs_u.sprite_effects[1];
  let e3 = vs_u.sprite_effects[2];
  let e4 = vs_u.sprite_effects[3];
  let e5 = vs_u.sprite_effects[4];
  let e6 = vs_u.sprite_effects[5];
  let e7 = vs_u.sprite_effects[6];
  let e8 = vs_u.sprite_effects[7];
  let e9 = vs_u.sprite_effects[8];
  let e10 = vs_u.sprite_effects[9];
  let e11 = vs_u.sprite_effects[10];

  let tr = e1.x;
  let mono = e1.y;
  let rev = e1.z;
  let bright = e1.w;
  let dark = e2.x;
  let color_rate = e2.y;
  let color_add = vec3<f32>(e2.z, e2.w, e3.x);
  let color_tgt = e3.yzw;
  let mask_mode = e4.x;
  let alpha_test = e4.y;
  let light_on = e4.z;
  let fog_on = e4.w;
  let has_mask = e5.x;
  let has_tonecurve = e5.y;
  let tonecurve_row = e5.z;
  let tonecurve_sat = e5.w;
  let wipe_mode = e6.x;
  let wipe_p0 = e6.y;
  let wipe_p1 = e6.z;
  let wipe_p2 = e6.w;
  let wipe_p3 = e7.x;
  let has_wipe_src = e7.y;
  let blend_code = e7.z;
  let wipe_aux1 = e7.w;
  let light_factor = e8.w;
  let fog_scroll_x = e9.w;
  let fog_color_fallback = vec4<f32>(e10.xyz, 1.0);
  let sprite_z = e10.w;
  let fog_near = e11.x;
  let fog_far = e11.y;
  let has_fog_tex = e11.z;
  let camera_z = e11.w;
  let alpha_ref = 1.0 / 255.0;

  var c = textureSample(tex0, smp0, i.uv);
  if (wipe_mode > 0.5 && wipe_mode < 1.5) {
    c = sample_mosaic(i.uv, wipe_p0, wipe_p1);
    c.a = 1.0;
  } else if (wipe_mode > 1.5 && wipe_mode < 2.5) {
    c = sample_raster_h(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3);
  } else if (wipe_mode > 2.5 && wipe_mode < 3.5) {
    c = sample_raster_v(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3);
  } else if (wipe_mode > 3.5 && wipe_mode < 4.5) {
    c = sample_explosion_blur(i.uv, vec2<f32>(wipe_p0, wipe_p1), wipe_p2, wipe_p3);
    c.a = 1.0;
  } else if (wipe_mode > 4.5 && wipe_mode < 5.5) {
    c = sample_shimi(i.uv, wipe_p0, wipe_p1);
  } else if (wipe_mode > 5.5 && wipe_mode < 6.5) {
    c = sample_shimi_inv(i.uv, wipe_p0, wipe_p1);
  } else if (wipe_mode > 9.5 && wipe_mode < 10.5 && has_wipe_src > 0.5) {
    let oldc = sample_mosaic_tex3(i.uv, wipe_p0, wipe_p1);
    let newc = sample_mosaic(i.uv, wipe_p0, wipe_p1);
    if (wipe_p3 < 230.5) {
      c = select(oldc, newc, wipe_p2 >= 0.5);
    } else {
      c = mix(select(newc, oldc, wipe_aux1 < 0.5), select(oldc, newc, wipe_aux1 < 0.5), clamp(wipe_p2, 0.0, 1.0));
    }
  } else if (wipe_mode > 10.5 && wipe_mode < 11.5 && has_wipe_src > 0.5) {
    c = mix(
      sample_raster_h_tex3(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3),
      sample_raster_h(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3),
      clamp(wipe_p3, 0.0, 1.0)
    );
  } else if (wipe_mode > 11.5 && wipe_mode < 12.5 && has_wipe_src > 0.5) {
    c = mix(
      sample_raster_v_tex3(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3),
      sample_raster_v(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3),
      clamp(wipe_p3, 0.0, 1.0)
    );
  } else if (wipe_mode > 12.5 && wipe_mode < 13.5 && has_wipe_src > 0.5) {
    c = mix(
      sample_explosion_blur_tex3(i.uv, vec2<f32>(wipe_p0, wipe_p1), wipe_p2, wipe_p3),
      sample_explosion_blur(i.uv, vec2<f32>(wipe_p0, wipe_p1), wipe_p2, wipe_p3),
      clamp(tonecurve_row, 0.0, 1.0)
    );
    c.a = 1.0;
  }

  var color = c * vec4<f32>(1.0, 1.0, 1.0, i.alpha * tr);
  let color_org = color;

  // CFX v2 calc_light multiplies the complete float4 by light power and
  // ambient. light_factor is the CPU-side world-position/normal result.
  if (light_on > 0.5) {
    color = color * vec4<f32>(e9.xyz, 1.0) * light_factor;
  }

  if (fog_on > 0.5) {
    let depth = abs(sprite_z - camera_z);
    let fog_t = clamp((depth - fog_near) / max(fog_far - fog_near, 1e-5), 0.0, 1.0);
    if (fog_t > 0.0) {
      var fog_color = fog_color_fallback;
      if (has_fog_tex > 0.5) {
        let dims_u = textureDimensions(tex4, 0);
        let tw = max(f32(dims_u.x), 1.0);
        let th = max(f32(dims_u.y), 1.0);
        let vw = max(vs_u.camera_params.z, 1.0);
        let vh = max(vs_u.camera_params.w, 1.0);
        let aspect = th / vh;
        let fog_w = vw / tw * aspect;
        let fog_h = vh / th;
        let fog_x = -fog_scroll_x / tw * aspect - 0.5 / vw;
        let fog_y = 0.5 / vh;
        let proj01 = vec2<f32>(i.pos.x / vw, i.pos.y / vh);
        let fog_base = vec2<f32>(proj01.x * fog_w + fog_x, proj01.y);
        let fog_uv = fog_base * fog_h + vec2<f32>(fog_y);
        fog_color = textureSampleLevel(tex4, smp4, fog_uv, 0.0);
      }
      color = mix(color, fog_color, fog_t);
    }
  }

  let mono_y = dot(color.rgb, vec3<f32>(0.2989, 0.5886, 0.1145));
  if (has_tonecurve > 0.5) {
    color = vec4<f32>(apply_tonecurve_from_mono(color.rgb, mono_y, tonecurve_row, tonecurve_sat), color.a);
  }
  color = vec4<f32>(mix(color.rgb, vec3<f32>(1.0) - color.rgb, rev), color.a);
  color = vec4<f32>(mix(color.rgb, vec3<f32>(mono_y), mono), color.a);
  color = vec4<f32>(color.rgb + vec3<f32>(bright), color.a);
  color = vec4<f32>(color.rgb - vec3<f32>(dark), color.a);
  color = vec4<f32>(mix(color.rgb, color_tgt, color_rate), color.a);
  color = vec4<f32>(color.rgb + color_add, color.a);

  if (blend_code > 2.5 && blend_code < 3.5) {
    color = mix(vec4<f32>(1.0), color, color_org.a);
  } else if (blend_code > 3.5 && blend_code < 4.5) {
    color = mix(vec4<f32>(0.0), color, color_org.a);
  }
  color.a = color_org.a;

  let final_gray = dot(color.rgb, vec3<f32>(0.2989, 0.5886, 0.1145));
  if (has_mask > 0.5) {
    color = color * sample_mask(i.uv_aux);
  }
  if (mask_mode > 0.5 && mask_mode < 1.5) {
    color.a = final_gray;
  }
  if (alpha_test > 0.5 && color.a < alpha_ref) {
    discard;
  }

  if (blend_code > 4.5 && blend_code < 5.5) {
    let dims_u = textureDimensions(tex3, 0);
    let screen_uv = vec2<f32>(
      clamp(i.pos.x / max(f32(dims_u.x), 1.0), 0.0, 1.0),
      clamp(i.pos.y / max(f32(dims_u.y), 1.0), 0.0, 1.0)
    );
    let dst = sample_tex3_safe(screen_uv);
    let ov = overlay_rgb(dst.rgb, color.rgb);
    return vec4<f32>(mix(dst.rgb, ov, color.a), 1.0);
  }
  return color;
}

fn fs_common(i: VsOut) -> vec4<f32> {
  let e1 = vs_u.sprite_effects[0];
  let e2 = vs_u.sprite_effects[1];
  let e3 = vs_u.sprite_effects[2];
  let e4 = vs_u.sprite_effects[3];
  let e5 = vs_u.sprite_effects[4];
  let e6 = vs_u.sprite_effects[5];
  let e7 = vs_u.sprite_effects[6];
  let e8 = vs_u.sprite_effects[7];
  let e9 = vs_u.sprite_effects[8];
  let e10 = vs_u.sprite_effects[9];
  let e11 = vs_u.sprite_effects[10];

  let tr = e1.x;
  let mono = e1.y;
  let rev = e1.z;
  let bright = e1.w;
  let dark = e2.x;
  let color_rate = e2.y;
  let color_add = vec3<f32>(e2.z, e2.w, e3.x);
  let color_tgt = e3.yzw;
  let mask_mode = e4.x;
  let alpha_test = e4.y;
  let light_on = e4.z;
  let fog_on = e4.w;
  let has_mask = e5.x;
  let has_tonecurve = e5.y;
  let tonecurve_row = e5.z;
  let tonecurve_sat = e5.w;
  let wipe_mode = e6.x;
  let wipe_p0 = e6.y;
  let wipe_p1 = e6.z;
  let wipe_p2 = e6.w;
  let wipe_p3 = e7.x;
  let has_wipe_src = e7.y;
  let blend_code = e7.z;
  let wipe_aux1 = e7.w;
  let light_factor = e8.w;
  let fog_scroll_x = e9.w;
  let fog_color_fallback = vec4<f32>(e10.xyz, 1.0);
  let fog_near = e11.x;
  let fog_far = e11.y;
  let has_fog_tex = e11.z;
  let alpha_ref = max(vs_u.mtrl_extra.y, 1.0 / 255.0);

  let world_pos = i.world_pos.xyz;
  let world_has_pos = i.world_pos.w > 0.5;
  let world_normal = i.world_normal.xyz;
  let world_tangent = i.world_tangent.xyz;
  let world_binormal = i.world_binormal.xyz;
  let mesh_pipeline = vs_u.flags.x > 0.5;
  let mesh_use_tex = vs_u.mesh_flags.x > 0.5;
  let mesh_use_mrbd = vs_u.mesh_flags.y > 0.5;
  let mesh_use_rgb = vs_u.mesh_flags.z > 0.5;
  let mesh_use_vertex_color = vs_u.mesh_flags.w > 0.5;

  var shaded_uv = i.uv;
  if (mesh_pipeline && world_has_pos && length(world_normal) > 0.25 && i32(round(vs_u.mtrl_params.y)) == 9) {
    let view_dir_world = normalize(vs_u.camera_eye.xyz - world_pos);
    shaded_uv = apply_parallax_uv(
      world_normal, world_tangent, world_binormal, i.uv, view_dir_world, max(vs_u.mtrl_extra.x, 0.0)
    );
  }

  var c = select(vec4<f32>(1.0), textureSample(tex0, smp0, shaded_uv), mesh_use_tex);
  if (wipe_mode > 0.5 && wipe_mode < 1.5) {
    c = sample_mosaic(i.uv, wipe_p0, wipe_p1);
    c.a = 1.0;
  } else if (wipe_mode > 1.5 && wipe_mode < 2.5) {
    c = sample_raster_h(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3);
  } else if (wipe_mode > 2.5 && wipe_mode < 3.5) {
    c = sample_raster_v(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3);
  } else if (wipe_mode > 3.5 && wipe_mode < 4.5) {
    c = sample_explosion_blur(i.uv, vec2<f32>(wipe_p0, wipe_p1), wipe_p2, wipe_p3);
    c.a = 1.0;
  } else if (wipe_mode > 4.5 && wipe_mode < 5.5) {
    c = sample_shimi(i.uv, wipe_p0, wipe_p1);
  } else if (wipe_mode > 5.5 && wipe_mode < 6.5) {
    c = sample_shimi_inv(i.uv, wipe_p0, wipe_p1);
  } else if (wipe_mode > 9.5 && wipe_mode < 10.5 && has_wipe_src > 0.5) {
    let oldc = sample_mosaic_tex3(i.uv, wipe_p0, wipe_p1);
    let newc = sample_mosaic(i.uv, wipe_p0, wipe_p1);
    if (wipe_p3 < 230.5) {
      c = select(oldc, newc, wipe_p2 >= 0.5);
    } else {
      c = mix(select(newc, oldc, wipe_aux1 < 0.5), select(oldc, newc, wipe_aux1 < 0.5), clamp(wipe_p2, 0.0, 1.0));
    }
  } else if (wipe_mode > 10.5 && wipe_mode < 11.5 && has_wipe_src > 0.5) {
    c = mix(sample_raster_h_tex3(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3), sample_raster_h(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3), clamp(wipe_p3, 0.0, 1.0));
  } else if (wipe_mode > 11.5 && wipe_mode < 12.5 && has_wipe_src > 0.5) {
    c = mix(sample_raster_v_tex3(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3), sample_raster_v(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3), clamp(wipe_p3, 0.0, 1.0));
  } else if (wipe_mode > 12.5 && wipe_mode < 13.5 && has_wipe_src > 0.5) {
    c = mix(sample_explosion_blur_tex3(i.uv, vec2<f32>(wipe_p0, wipe_p1), wipe_p2, wipe_p3), sample_explosion_blur(i.uv, vec2<f32>(wipe_p0, wipe_p1), wipe_p2, wipe_p3), clamp(tonecurve_row, 0.0, 1.0));
    c.a = 1.0;
  }

  var color = c * vec4<f32>(1.0, 1.0, 1.0, i.alpha * tr);
  if (mesh_pipeline) {
    color = color * vs_u.mtrl_diffuse;
    if (mesh_use_vertex_color) {
      color = color * mix(vec4<f32>(1.0), i.vertex_color, clamp(vs_u.mesh_misc.x, 0.0, 1.0));
    }
  }
  let color_org = color;

  if (light_on > 0.5) {
    if (mesh_pipeline && world_has_pos && length(world_normal) > 0.25) {
      color = vec4<f32>(
        mesh_lighting(
          color.rgb, world_pos, world_normal, world_tangent, world_binormal, shaded_uv, i.shadow_pos
        ),
        color.a
      );
    } else {
      color = color * vec4<f32>(e9.xyz, 1.0) * light_factor;
    }
  } else if (mesh_pipeline) {
    color = vec4<f32>(color.rgb + vs_u.mtrl_emissive.rgb, color.a);
  }

  if (fog_on > 0.5) {
    var depth = abs(e10.w - e11.w);
    if (world_has_pos) {
      depth = length(vs_u.camera_eye.xyz - world_pos);
    }
    let fog_t = clamp((depth - fog_near) / max(fog_far - fog_near, 1e-5), 0.0, 1.0);
    if (fog_t > 0.0) {
      var fog_color = fog_color_fallback;
      if (has_fog_tex > 0.5) {
        let dims_u = textureDimensions(tex4, 0);
        let tw = max(f32(dims_u.x), 1.0);
        let th = max(f32(dims_u.y), 1.0);
        let vw = max(vs_u.camera_params.z, 1.0);
        let vh = max(vs_u.camera_params.w, 1.0);
        let aspect = th / vh;
        let fog_w = vw / tw * aspect;
        let fog_h = vh / th;
        let fog_x = -fog_scroll_x / tw * aspect - 0.5 / vw;
        let fog_y = 0.5 / vh;
        let ndc = i.proj_pos.xy / max(abs(i.proj_pos.w), 1e-5);
        let fog_base = vec2<f32>((ndc.x + 1.0) * 0.5 * fog_w + fog_x, 1.0 - (ndc.y + 1.0) * 0.5);
        let fog_uv = fog_base * fog_h + vec2<f32>(fog_y);
        fog_color = textureSampleLevel(tex4, smp4, fog_uv, 0.0);
      }
      color = mix(color, fog_color, fog_t);
    }
  }

  // Material MRBD/RGB belongs after lighting/fog and before the shared CFX
  // tonecurve/reverse/mono/bright/dark/RGB sequence.
  if (mesh_use_mrbd) {
    let mesh_mono_y = dot(color.rgb, vec3<f32>(0.2989, 0.5886, 0.1145));
    color = vec4<f32>(mix(color.rgb, vec3<f32>(1.0) - color.rgb, vs_u.mesh_mrbd.y), color.a);
    color = vec4<f32>(mix(color.rgb, vec3<f32>(mesh_mono_y), vs_u.mesh_mrbd.x), color.a);
    color = vec4<f32>(color.rgb + vec3<f32>(vs_u.mesh_mrbd.z), color.a);
    color = vec4<f32>(color.rgb - vec3<f32>(vs_u.mesh_mrbd.w), color.a);
  }
  if (mesh_use_rgb) {
    color = vec4<f32>(mix(color.rgb, vs_u.mesh_rgb_rate.xyz, vs_u.mesh_rgb_rate.w), color.a);
    color = vec4<f32>(color.rgb + vs_u.mesh_add_rgb.xyz, color.a);
  }

  let mono_y = dot(color.rgb, vec3<f32>(0.2989, 0.5886, 0.1145));
  if (has_tonecurve > 0.5) {
    color = vec4<f32>(apply_tonecurve_from_mono(color.rgb, mono_y, tonecurve_row, tonecurve_sat), color.a);
  }
  color = vec4<f32>(mix(color.rgb, vec3<f32>(1.0) - color.rgb, rev), color.a);
  color = vec4<f32>(mix(color.rgb, vec3<f32>(mono_y), mono), color.a);
  color = vec4<f32>(color.rgb + vec3<f32>(bright), color.a);
  color = vec4<f32>(color.rgb - vec3<f32>(dark), color.a);
  color = vec4<f32>(mix(color.rgb, color_tgt, color_rate), color.a);
  color = vec4<f32>(color.rgb + color_add, color.a);

  if (blend_code > 2.5 && blend_code < 3.5) {
    color = mix(vec4<f32>(1.0), color, color_org.a);
  } else if (blend_code > 3.5 && blend_code < 4.5) {
    color = mix(vec4<f32>(0.0), color, color_org.a);
  }
  color.a = color_org.a;

  let final_gray = dot(color.rgb, vec3<f32>(0.2989, 0.5886, 0.1145));
  if (has_mask > 0.5) {
    color = color * sample_mask(i.uv);
  }
  if (mask_mode > 0.5 && mask_mode < 1.5) {
    color.a = final_gray;
  }
  if (alpha_test > 0.5 && color.a < alpha_ref) {
    discard;
  }

  if (blend_code > 4.5 && blend_code < 5.5) {
    let dims_u = textureDimensions(tex3, 0);
    let screen_uv = vec2<f32>(
      clamp(i.pos.x / max(f32(dims_u.x), 1.0), 0.0, 1.0),
      clamp(i.pos.y / max(f32(dims_u.y), 1.0), 0.0, 1.0)
    );
    let dst = sample_tex3_safe(screen_uv);
    let ov = overlay_rgb(dst.rgb, color.rgb);
    return vec4<f32>(mix(dst.rgb, ov, color.a), 1.0);
  }
  return color;
}

fn fs_shadow_common(i: ShadowVsOut) -> vec4<f32> {
  let base = textureSample(tex0, smp0, i.uv);
  if ((i.alpha_test > 0.5 || base.a < 0.999) && base.a <= max(vs_u.mtrl_extra.y, 0.001)) {
    discard;
  }
  return vec4<f32>(i.depth, i.depth, i.depth, 1.0);
}

@vertex
fn vs_sprite_2d(v: VsIn2d) -> VsOut2d {
  return vs_common_2d(v);
}

@vertex
fn vs_mesh_static(v: VsIn) -> VsOut {
  return vs_common(v);
}

@vertex
fn vs_mesh_skinned(v: VsIn) -> VsOut {
  return vs_common(v);
}

@vertex
fn vs_shadow_static(v: VsIn) -> ShadowVsOut {
  return vs_shadow_common(v);
}

@vertex
fn vs_shadow_skinned(v: VsIn) -> ShadowVsOut {
  return vs_shadow_common(v);
}

@fragment
fn fs_sprite_2d(i: VsOut2d) -> @location(0) vec4<f32> {
  return fs_common_2d(i);
}

@fragment
fn fs_overlay_gpu(i: VsOut2d) -> @location(0) vec4<f32> {
  return fs_common_2d(i);
}

@fragment
fn fs_wipe_mosaic(i: VsOut2d) -> @location(0) vec4<f32> {
  return fs_common_2d(i);
}

@fragment
fn fs_wipe_raster_h(i: VsOut2d) -> @location(0) vec4<f32> {
  return fs_common_2d(i);
}

@fragment
fn fs_wipe_raster_v(i: VsOut2d) -> @location(0) vec4<f32> {
  return fs_common_2d(i);
}

@fragment
fn fs_wipe_explosion_blur(i: VsOut2d) -> @location(0) vec4<f32> {
  return fs_common_2d(i);
}

@fragment
fn fs_wipe_shimi(i: VsOut2d) -> @location(0) vec4<f32> {
  return fs_common_2d(i);
}

@fragment
fn fs_wipe_shimi_inv(i: VsOut2d) -> @location(0) vec4<f32> {
  return fs_common_2d(i);
}

@fragment
fn fs_wipe_cross_mosaic(i: VsOut2d) -> @location(0) vec4<f32> {
  return fs_common_2d(i);
}

@fragment
fn fs_wipe_cross_raster_h(i: VsOut2d) -> @location(0) vec4<f32> {
  return fs_common_2d(i);
}

@fragment
fn fs_wipe_cross_raster_v(i: VsOut2d) -> @location(0) vec4<f32> {
  return fs_common_2d(i);
}

@fragment
fn fs_wipe_cross_explosion_blur(i: VsOut2d) -> @location(0) vec4<f32> {
  return fs_common_2d(i);
}

@fragment
fn fs_mesh_unlit(i: VsOut) -> @location(0) vec4<f32> {
  return fs_common(i);
}

@fragment
fn fs_mesh_lambert(i: VsOut) -> @location(0) vec4<f32> {
  return fs_common(i);
}

@fragment
fn fs_mesh_blinn_phong(i: VsOut) -> @location(0) vec4<f32> {
  return fs_common(i);
}

@fragment
fn fs_mesh_pp_blinn_phong(i: VsOut) -> @location(0) vec4<f32> {
  return fs_common(i);
}

@fragment
fn fs_mesh_pp_half_lambert(i: VsOut) -> @location(0) vec4<f32> {
  return fs_common(i);
}

@fragment
fn fs_mesh_toon(i: VsOut) -> @location(0) vec4<f32> {
  return fs_common(i);
}

@fragment
fn fs_mesh_ffp(i: VsOut) -> @location(0) vec4<f32> {
  return fs_common(i);
}

@fragment
fn fs_mesh_pp_ffp(i: VsOut) -> @location(0) vec4<f32> {
  return fs_common(i);
}

@fragment
fn fs_mesh_bump(i: VsOut) -> @location(0) vec4<f32> {
  return fs_common(i);
}

@fragment
fn fs_mesh_parallax(i: VsOut) -> @location(0) vec4<f32> {
  return fs_common(i);
}

@fragment
fn fs_shadow_map(i: ShadowVsOut) -> @location(0) vec4<f32> {
  return fs_shadow_common(i);
}
"#;
