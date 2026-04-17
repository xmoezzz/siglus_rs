//! WGPU renderer for Siglus stage composition.
//!
//! This renderer consumes a painter-ordered list of sprites and draws them
//! in order. It supports fixed sprite effects, dual-source wipes, and a
//! depth-backed path for 3D-transformed quads.

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::assets::load_image_any;
use crate::image_manager::{ImageId, ImageManager};
use crate::layer::{
    ClipRect, RenderSprite, SpriteBlend, SpriteFit, SpriteRuntimeLight, SpriteSizeMode,
};
use crate::mesh3d::{load_mesh_asset, MeshAsset};
use crate::render_math::sprite_quad_points;

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

const MAX_BONES: usize = 64;
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
    const ATTRS: [wgpu::VertexAttribute; 26] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x2,
        2 => Float32x2,
        3 => Float32,
        4 => Float32x4,
        5 => Float32x4,
        6 => Float32x4,
        7 => Float32x4,
        8 => Float32x4,
        9 => Float32x4,
        10 => Float32x4,
        11 => Float32x4,
        12 => Float32x4,
        13 => Float32x4,
        14 => Float32x4,
        15 => Float32x4,
        16 => Float32x4,
        17 => Float32x4,
        18 => Float32x4,
        19 => Float32x4,
        20 => Float32x4,
        21 => Float32x4,
        22 => Float32x4,
        23 => Float32x4,
        24 => Float32x4,
        25 => Float32x4
    ];

    fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

struct VertexSprite2d;

impl VertexSprite2d {
    const ATTRS: [wgpu::VertexAttribute; 15] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x2,
        2 => Float32x2,
        3 => Float32,
        4 => Float32x4,
        5 => Float32x4,
        6 => Float32x4,
        7 => Float32x4,
        8 => Float32x4,
        9 => Float32x4,
        10 => Float32x4,
        11 => Float32x4,
        12 => Float32x4,
        13 => Float32x4,
        14 => Float32x4
    ];

    fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
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

    pipelines: HashMap<PipelineKey, wgpu::RenderPipeline>,
    bind_group_layout: wgpu::BindGroupLayout,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,

    vertex_buf: wgpu::Buffer,
    vertex_capacity: usize,

    textures: HashMap<ImageId, GpuTexture>,
    external_textures: HashMap<PathBuf, GpuTexture>,
    mesh_assets: HashMap<String, MeshAsset>,
    default_aux: GpuTexture,
    depth: DepthTexture,
    scene_a: RenderTargetTexture,
    scene_b: RenderTargetTexture,
    shadow_map: RenderTargetTexture,
    shadow_depth: DepthTexture,

    verts: Vec<Vertex>,
    draws: Vec<DrawCommand>,
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
}

#[derive(Debug, Clone, Copy)]
enum InternalColorTarget {
    SceneA,
    SceneB,
    ShadowMap,
}

#[derive(Debug, Clone, Copy)]
enum DepthTarget {
    None,
    Main,
    Shadow,
}

#[derive(Debug, Clone, Copy)]
enum BackdropTarget {
    SceneA,
    SceneB,
}

#[derive(Debug, Clone, Copy)]
enum ColorTarget<'a> {
    External(&'a wgpu::TextureView),
    Internal(InternalColorTarget),
}

#[derive(Debug, Clone)]
struct DrawCommand {
    image_id: Option<ImageId>,
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
        tex: u8::from(sprite.image_id.is_some()),
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
    Some([x_ndc, y_ndc, depth * 2.0 - 1.0, 1.0])
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
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window).context("create_surface")?;

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
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .context("request_device")?;

        let surface_caps = surface.get_capabilities(&adapter);
        let format = surface_caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(surface_caps.formats[0]);

        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: surface_caps.present_modes[0],
            alpha_mode: surface_caps.alpha_modes[0],
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
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
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

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("siglus-sprite-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("siglus-sprite-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let vertex_capacity = 6;
        let vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("siglus-sprite-vertex-buf"),
            size: (vertex_capacity * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let default_aux = create_solid_texture(&device, &queue, [255, 255, 255, 255])?;
        let depth = create_depth_texture(&device, config.width, config.height);
        let scene_a = create_render_target_texture(
            &device,
            config.width,
            config.height,
            config.format,
            "siglus-scene-a",
        );
        let scene_b = create_render_target_texture(
            &device,
            config.width,
            config.height,
            config.format,
            "siglus-scene-b",
        );
        let shadow_map =
            create_render_target_texture(&device, 2048, 2048, config.format, "siglus-shadow-map");
        let shadow_depth = create_depth_texture(&device, 2048, 2048);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipelines: HashMap::new(),
            bind_group_layout,
            shader,
            pipeline_layout,
            vertex_buf,
            vertex_capacity,
            textures: HashMap::new(),
            external_textures: HashMap::new(),
            mesh_assets: HashMap::new(),
            default_aux,
            depth,
            scene_a,
            scene_b,
            shadow_map,
            shadow_depth,
            verts: Vec::new(),
            draws: Vec::new(),
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth = create_depth_texture(&self.device, self.config.width, self.config.height);
        self.scene_a = create_render_target_texture(
            &self.device,
            self.config.width,
            self.config.height,
            self.config.format,
            "siglus-scene-a",
        );
        self.scene_b = create_render_target_texture(
            &self.device,
            self.config.width,
            self.config.height,
            self.config.format,
            "siglus-scene-b",
        );
    }

    pub fn render_sprites(
        &mut self,
        images: &ImageManager,
        sprites: &[RenderSprite],
    ) -> Result<()> {
        let frame = self
            .surface
            .get_current_texture()
            .context("get_current_texture")?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        self.verts.clear();
        self.draws.clear();

        let win_w = self.config.width as f32;
        let win_h = self.config.height as f32;

        for s in sprites {
            let sprite = &s.sprite;
            let img_id = sprite.image_id;
            let img = img_id.and_then(|id| images.get(id));

            let (src_left, src_top, src_right, src_bottom) = if let Some(img) = img {
                src_clip_rect(sprite.src_clip, img.width, img.height)?
            } else {
                (0.0, 0.0, 1.0, 1.0)
            };
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

            let scissor = dst_scissor_rect(sprite.dst_clip, self.config.width, self.config.height);
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
            let pipeline_key = PipelineKey {
                technique,
                blend: sprite.blend,
                alpha_blend: if matches!(technique.special, TechniqueSpecial::Overlay) {
                    false
                } else {
                    sprite.alpha_blend
                },
                use_depth,
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
                            sprite.alpha_blend
                        },
                        use_depth,
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
                    let vs_uniform = if batch.skinned {
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
            let Some(img) = img else {
                continue;
            };
            let (u0, v0, u1, v1) = (
                (src_left / img.width as f32).clamp(0.0, 1.0),
                (src_top / img.height as f32).clamp(0.0, 1.0),
                (src_right / img.width as f32).clamp(0.0, 1.0),
                (src_bottom / img.height as f32).clamp(0.0, 1.0),
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
            self.draws.push(DrawCommand {
                image_id: img_id,
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
                vs_uniform: VsUniform::for_2d(win_w, win_h),
                bone_uniform: BoneUniform::zero(),
            });
        }

        let blit_range = append_fullscreen_blit_vertices(&mut self.verts);

        if self.draws.is_empty() {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("siglus-clear-encoder"),
                });
            self.render_command_slice(
                &mut encoder,
                ColorTarget::External(&view),
                DepthTarget::Main,
                0..0,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                true,
                None,
                None,
            )?;
            self.queue.submit(Some(encoder.finish()));
            frame.present();
            return Ok(());
        }

        self.ensure_vertex_capacity(self.verts.len())?;
        self.queue
            .write_buffer(&self.vertex_buf, 0, bytemuck::cast_slice(&self.verts));

        let draws_snapshot = self.draws.clone();
        for cmd in &draws_snapshot {
            if let Some(id) = cmd.image_id {
                self.ensure_texture_uploaded(images, id)?;
            }
            if let Some(id) = cmd.mask_image_id {
                self.ensure_texture_uploaded(images, id)?;
            }
            if let Some(id) = cmd.tonecurve_image_id {
                self.ensure_texture_uploaded(images, id)?;
            }
            if let Some(id) = cmd.fog_image_id {
                self.ensure_texture_uploaded(images, id)?;
            }
            if let Some(id) = cmd.wipe_src_image_id {
                self.ensure_texture_uploaded(images, id)?;
            }
        }

        let has_overlay = draws_snapshot.iter().any(|cmd| {
            matches!(
                cmd.pipeline_key.technique.special,
                TechniqueSpecial::Overlay
            )
        });
        for cmd in draws_snapshot.clone() {
            self.ensure_pipeline(cmd.pipeline_key.clone());
            if cmd.shadow_cast {
                self.ensure_pipeline(shadow_pipeline_key(
                    cmd.pipeline_key,
                    cmd.shadow_pipeline_name.as_deref(),
                ));
            }
        }
        if has_overlay {
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
                cull_back: false,
                mesh_fx_variant: 0,
                pipeline_name: String::new(),
                program: EffectProgram::Sprite2D,
            });
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("siglus-sprite-encoder"),
            });
        let draws_for_pass = self.draws.clone();

        let shadow_indices: Vec<usize> = draws_for_pass
            .iter()
            .enumerate()
            .filter_map(|(idx, cmd)| if cmd.shadow_cast { Some(idx) } else { None })
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

        if !has_overlay {
            self.render_command_slice(
                &mut encoder,
                ColorTarget::External(&view),
                DepthTarget::Main,
                0..draws_for_pass.len(),
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                true,
                None,
                None,
            )?;
        } else {
            self.render_command_slice(
                &mut encoder,
                ColorTarget::Internal(InternalColorTarget::SceneA),
                DepthTarget::Main,
                0..0,
                wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                true,
                None,
                None,
            )?;
            let mut current_is_a = true;
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
                    let (src, dst) = if current_is_a {
                        (BackdropTarget::SceneA, InternalColorTarget::SceneB)
                    } else {
                        (BackdropTarget::SceneB, InternalColorTarget::SceneA)
                    };
                    self.render_copy_pass(
                        &mut encoder,
                        ColorTarget::Internal(dst),
                        src,
                        blit_range.clone(),
                    )?;
                    self.render_command_slice(
                        &mut encoder,
                        ColorTarget::Internal(dst),
                        DepthTarget::Main,
                        start..index,
                        wgpu::LoadOp::Load,
                        false,
                        Some(src),
                        None,
                    )?;
                    current_is_a = !current_is_a;
                } else {
                    let color_target = if current_is_a {
                        ColorTarget::Internal(InternalColorTarget::SceneA)
                    } else {
                        ColorTarget::Internal(InternalColorTarget::SceneB)
                    };
                    self.render_command_slice(
                        &mut encoder,
                        color_target,
                        DepthTarget::Main,
                        start..index,
                        wgpu::LoadOp::Load,
                        false,
                        None,
                        None,
                    )?;
                }
            }

            let final_src = if current_is_a {
                BackdropTarget::SceneA
            } else {
                BackdropTarget::SceneB
            };
            self.render_copy_pass(
                &mut encoder,
                ColorTarget::External(&view),
                final_src,
                blit_range,
            )?;
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
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
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::One,
                        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
                        operation: wgpu::BlendOperation::Add,
                    },
                },
                SpriteBlend::Add => wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent::OVER,
                },
                SpriteBlend::Sub => wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::SrcAlpha,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::ReverseSubtract,
                    },
                    alpha: wgpu::BlendComponent::OVER,
                },
                SpriteBlend::Mul => wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::Dst,
                        dst_factor: wgpu::BlendFactor::Zero,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent::OVER,
                },
                SpriteBlend::Screen => wgpu::BlendState {
                    color: wgpu::BlendComponent {
                        src_factor: wgpu::BlendFactor::OneMinusDst,
                        dst_factor: wgpu::BlendFactor::One,
                        operation: wgpu::BlendOperation::Add,
                    },
                    alpha: wgpu::BlendComponent::OVER,
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
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth32Float,
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

    fn color_target_view<'a>(&'a self, target: ColorTarget<'a>) -> &'a wgpu::TextureView {
        match target {
            ColorTarget::External(view) => view,
            ColorTarget::Internal(InternalColorTarget::SceneA) => &self.scene_a.view,
            ColorTarget::Internal(InternalColorTarget::SceneB) => &self.scene_b.view,
            ColorTarget::Internal(InternalColorTarget::ShadowMap) => &self.shadow_map.view,
        }
    }

    fn depth_target_view(&self, target: DepthTarget) -> Option<&wgpu::TextureView> {
        match target {
            DepthTarget::None => None,
            DepthTarget::Main => Some(&self.depth.view),
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
        let commands: Vec<DrawCommand> = range.clone().map(|idx| self.draws[idx].clone()).collect();
        for cmd in &commands {
            if let Some(path) = cmd.mesh_texture_path.as_deref() {
                let _ = self.ensure_external_texture(path);
            }
            if let Some(path) = cmd.mesh_normal_texture_path.as_deref() {
                let _ = self.ensure_external_texture(path);
            }
            if let Some(path) = cmd.mesh_toon_texture_path.as_deref() {
                let _ = self.ensure_external_texture(path);
            }
        }

        let mut keep_vs_uniform_bufs: Vec<wgpu::Buffer> = Vec::new();
        let mut keep_bone_uniform_bufs: Vec<wgpu::Buffer> = Vec::new();
        let mut keep_bind_groups: Vec<wgpu::BindGroup> = Vec::new();
        let config_width = self.config.width;
        let config_height = self.config.height;
        let overlay_backdrop = overlay_backdrop.map(|target| self.backdrop_target_ref(target));
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

        for cmd in commands {
            let semantics = self.resolve_effect_resources_for_draw(&cmd, overlay_backdrop);
            let vs_uniform_buf =
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("siglus-vs-uniform"),
                        contents: bytemuck::bytes_of(&cmd.vs_uniform),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
            let bone_uniform_buf =
                self.device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("siglus-bone-uniform"),
                        contents: bytemuck::bytes_of(&cmd.bone_uniform),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });

            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("siglus-sprite-bg"),
                layout: &self.bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&semantics.base.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&semantics.base.sampler),
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
                        resource: wgpu::BindingResource::Sampler(&semantics.fog.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 10,
                        resource: wgpu::BindingResource::TextureView(semantics.shadow_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 11,
                        resource: wgpu::BindingResource::Sampler(semantics.shadow_sampler),
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
                        resource: wgpu::BindingResource::TextureView(&semantics.normal.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 15,
                        resource: wgpu::BindingResource::Sampler(&semantics.normal.sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 16,
                        resource: wgpu::BindingResource::TextureView(&semantics.toon.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 17,
                        resource: wgpu::BindingResource::Sampler(&semantics.toon.sampler),
                    },
                ],
            });

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
            keep_vs_uniform_bufs.push(vs_uniform_buf);
            keep_bone_uniform_bufs.push(bone_uniform_buf);
            keep_bind_groups.push(bind_group);
            let bind_group_ptr = keep_bind_groups.last().unwrap() as *const wgpu::BindGroup;
            unsafe {
                rp.set_bind_group(0, &*bind_group_ptr, &[]);
            }
            if let Some(sci) = cmd.scissor {
                rp.set_scissor_rect(sci.x, sci.y, sci.w, sci.h);
            } else {
                rp.set_scissor_rect(0, 0, config_width, config_height);
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
        let base = if let Some(path) = cmd.mesh_texture_path.as_deref() {
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
            shadow_sampler: &self.shadow_map.sampler,
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
            cull_back: false,
            mesh_fx_variant: 0,
            pipeline_name: String::new(),
            program: EffectProgram::Sprite2D,
        };
        let vs_uniform = VsUniform::for_2d(self.config.width as f32, self.config.height as f32);
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
                    resource: wgpu::BindingResource::Sampler(&self.shadow_map.sampler),
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
                    resource: wgpu::BindingResource::Sampler(&self.default_aux.sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 16,
                    resource: wgpu::BindingResource::TextureView(&self.default_aux.view),
                },
                wgpu::BindGroupEntry {
                    binding: 17,
                    resource: wgpu::BindingResource::Sampler(&self.default_aux.sampler),
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
        rp.set_vertex_buffer(0, self.vertex_buf.slice(..));
        rp.set_bind_group(0, &bind_group, &[]);
        rp.set_scissor_rect(0, 0, self.config.width, self.config.height);
        rp.draw(blit_range, 0..1);
        Ok(())
    }

    fn ensure_vertex_capacity(&mut self, needed: usize) -> Result<()> {
        if needed <= self.vertex_capacity {
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
        Ok(())
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
fn create_solid_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    rgba: [u8; 4],
) -> Result<GpuTexture> {
    let img = crate::assets::RgbaImage {
        width: 1,
        height: 1,
        rgba: rgba.to_vec(),
    };
    create_gpu_texture(device, queue, "siglus-default-aux", &img, 0)
}

fn create_gpu_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    img: &crate::assets::RgbaImage,
    version: u64,
) -> Result<GpuTexture> {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: img.width,
            height: img.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    queue.write_texture(
        wgpu::ImageCopyTexture {
            texture: &tex,
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

    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("siglus-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
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
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::RENDER_ATTACHMENT,
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
    let nz = depth * 2.0 - 1.0;
    (nx, ny, nz)
}

fn create_depth_texture(device: &wgpu::Device, width: u32, height: u32) -> DepthTexture {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("siglus-depth"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
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

fn dst_scissor_rect(clip: Option<ClipRect>, win_w: u32, win_h: u32) -> Option<ScissorRect> {
    let c = clip?;
    let mut left = c.left.max(0) as i64;
    let mut top = c.top.max(0) as i64;
    let mut right = c.right.max(0) as i64;
    let mut bottom = c.bottom.max(0) as i64;
    let max_w = win_w as i64;
    let max_h = win_h as i64;
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

const SHADER: &str = r#"
struct VsIn {
  @location(0) pos: vec3<f32>,
  @location(1) uv: vec2<f32>,
  @location(2) uv_aux: vec2<f32>,
  @location(3) alpha: f32,
  @location(4) effects1: vec4<f32>,
  @location(5) effects2: vec4<f32>,
  @location(6) effects3: vec4<f32>,
  @location(7) effects4: vec4<f32>,
  @location(8) effects5: vec4<f32>,
  @location(9) effects6: vec4<f32>,
  @location(10) effects7: vec4<f32>,
  @location(11) effects8: vec4<f32>,
  @location(12) effects9: vec4<f32>,
  @location(13) effects10: vec4<f32>,
  @location(14) effects11: vec4<f32>,
  @location(15) world_pos: vec4<f32>,
  @location(16) world_normal: vec4<f32>,
  @location(17) world_tangent: vec4<f32>,
  @location(18) world_binormal: vec4<f32>,
  @location(19) shadow_pos: vec4<f32>,
  @location(20) bone_indices: vec4<f32>,
  @location(21) bone_weights: vec4<f32>,
  @location(22) light_pos_kind: vec4<f32>,
  @location(23) light_dir_shadow: vec4<f32>,
  @location(24) light_atten: vec4<f32>,
  @location(25) light_cone: vec4<f32>,
};

struct VsIn2d {
  @location(0) pos: vec3<f32>,
  @location(1) uv: vec2<f32>,
  @location(2) uv_aux: vec2<f32>,
  @location(3) alpha: f32,
  @location(4) effects1: vec4<f32>,
  @location(5) effects2: vec4<f32>,
  @location(6) effects3: vec4<f32>,
  @location(7) effects4: vec4<f32>,
  @location(8) effects5: vec4<f32>,
  @location(9) effects6: vec4<f32>,
  @location(10) effects7: vec4<f32>,
  @location(11) effects8: vec4<f32>,
  @location(12) effects9: vec4<f32>,
  @location(13) effects10: vec4<f32>,
  @location(14) effects11: vec4<f32>,
};

struct VsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) uv_aux: vec2<f32>,
  @location(2) alpha: f32,
  @location(3) effects1: vec4<f32>,
  @location(4) effects2: vec4<f32>,
  @location(5) effects3: vec4<f32>,
  @location(6) effects4: vec4<f32>,
  @location(7) effects5: vec4<f32>,
  @location(8) effects6: vec4<f32>,
  @location(9) effects7: vec4<f32>,
  @location(10) effects8: vec4<f32>,
  @location(11) effects9: vec4<f32>,
  @location(12) effects10: vec4<f32>,
  @location(13) effects11: vec4<f32>,
  @location(14) world_pos: vec4<f32>,
  @location(15) world_normal: vec4<f32>,
  @location(16) world_tangent: vec4<f32>,
  @location(17) world_binormal: vec4<f32>,
  @location(18) shadow_pos: vec4<f32>,
  @location(19) light_pos_kind: vec4<f32>,
  @location(20) light_dir_shadow: vec4<f32>,
  @location(21) light_atten: vec4<f32>,
  @location(22) light_cone: vec4<f32>,
};

struct VsOut2d {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) uv_aux: vec2<f32>,
  @location(2) alpha: f32,
  @location(3) effects1: vec4<f32>,
  @location(4) effects2: vec4<f32>,
  @location(5) effects3: vec4<f32>,
  @location(6) effects4: vec4<f32>,
  @location(7) effects5: vec4<f32>,
  @location(8) effects6: vec4<f32>,
  @location(9) effects7: vec4<f32>,
  @location(10) effects8: vec4<f32>,
  @location(11) effects9: vec4<f32>,
  @location(12) effects10: vec4<f32>,
  @location(13) effects11: vec4<f32>,
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

fn skin_local(local: vec3<f32>, bone_indices: vec4<f32>, bone_weights: vec4<f32>) -> vec3<f32> {
  let sum_w = bone_weights.x + bone_weights.y + bone_weights.z + bone_weights.w;
  if (vs_u.flags.w <= 0.5 || sum_w <= 1e-6) {
    return apply_frame(local);
  }
  var out = vec4<f32>(0.0, 0.0, 0.0, 0.0);
  if (bone_weights.x > 0.0) {
    out = out + (bone_u.matrices[min(u32(max(bone_indices.x, 0.0)), 63u)] * vec4<f32>(local, 1.0)) * bone_weights.x;
  }
  if (bone_weights.y > 0.0) {
    out = out + (bone_u.matrices[min(u32(max(bone_indices.y, 0.0)), 63u)] * vec4<f32>(local, 1.0)) * bone_weights.y;
  }
  if (bone_weights.z > 0.0) {
    out = out + (bone_u.matrices[min(u32(max(bone_indices.z, 0.0)), 63u)] * vec4<f32>(local, 1.0)) * bone_weights.z;
  }
  if (bone_weights.w > 0.0) {
    out = out + (bone_u.matrices[min(u32(max(bone_indices.w, 0.0)), 63u)] * vec4<f32>(local, 1.0)) * bone_weights.w;
  }
  return out.xyz;
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
    let z_ndc = clamp((cz - 1.0) / 10000.0 - 1.0, -1.0, 1.0);
    return vec4<f32>(x_ndc, y_ndc, z_ndc, 1.0);
  }
  let x_ndc = (world.x / max(vs_u.camera_params.z, 1.0)) * 2.0 - 1.0;
  let y_ndc = 1.0 - (world.y / max(vs_u.camera_params.w, 1.0)) * 2.0;
  let z_ndc = clamp(-world.z / 50000.0, -1.0, 1.0);
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
  return vec4<f32>(x_ndc, y_ndc, depth * 2.0 - 1.0, 1.0);
}

fn vs_common(v: VsIn) -> VsOut {
  var o: VsOut;
  if (vs_u.flags.x > 0.5) {
    let local_world = skin_local(v.pos, v.bone_indices, v.bone_weights);
    let local_normal = skin_normal(v.world_normal.xyz, v.bone_indices, v.bone_weights);
    let local_tangent = skin_normal(v.world_tangent.xyz, v.bone_indices, v.bone_weights);
    let local_binormal = skin_normal(v.world_binormal.xyz, v.bone_indices, v.bone_weights);
    let world = apply_model(local_world);
    let normal = apply_normal(local_normal);
    let tangent = apply_normal(local_tangent);
    let binormal = apply_normal(local_binormal);
    let shadow = project_shadow(world);
    o.pos = project_main(world);
    o.world_pos = vec4<f32>(world, 1.0);
    o.world_normal = vec4<f32>(normal, 0.0);
    o.world_tangent = vec4<f32>(tangent, 0.0);
    o.world_binormal = vec4<f32>(binormal, 0.0);
    o.shadow_pos = shadow;
  } else {
    o.pos = vec4<f32>(v.pos, 1.0);
    o.world_pos = v.world_pos;
    o.world_normal = v.world_normal;
    o.world_tangent = v.world_tangent;
    o.world_binormal = v.world_binormal;
    o.shadow_pos = v.shadow_pos;
  }
  o.uv = v.uv;
  o.uv_aux = v.uv_aux;
  o.alpha = v.alpha;
  o.effects1 = v.effects1;
  o.effects2 = v.effects2;
  o.effects3 = v.effects3;
  o.effects4 = v.effects4;
  o.effects5 = v.effects5;
  o.effects6 = v.effects6;
  o.effects7 = v.effects7;
  o.effects8 = v.effects8;
  o.effects9 = v.effects9;
  o.effects10 = v.effects10;
  o.effects11 = v.effects11;
  o.light_pos_kind = v.light_pos_kind;
  o.light_dir_shadow = v.light_dir_shadow;
  o.light_atten = v.light_atten;
  o.light_cone = v.light_cone;
  return o;
}

fn vs_shadow_common(v: VsIn) -> ShadowVsOut {
  var o: ShadowVsOut;
  if (vs_u.flags.x > 0.5) {
    let local_world = skin_local(v.pos, v.bone_indices, v.bone_weights);
    let world = apply_model(local_world);
    let shadow = project_shadow(world);
    o.pos = vec4<f32>(shadow.xyz, 1.0);
    o.depth = clamp(shadow.z * 0.5 + 0.5, 0.0, 1.0);
  } else {
    o.pos = vec4<f32>(v.shadow_pos.xyz, 1.0);
    o.depth = clamp(v.shadow_pos.z * 0.5 + 0.5, 0.0, 1.0);
  }
  o.uv = v.uv;
  o.alpha_test = v.effects4.y;
  return o;
}

fn vs_common_2d(v: VsIn2d) -> VsOut2d {
  var o: VsOut2d;
  o.pos = vec4<f32>(v.pos, 1.0);
  o.uv = v.uv;
  o.uv_aux = v.uv_aux;
  o.alpha = v.alpha;
  o.effects1 = v.effects1;
  o.effects2 = v.effects2;
  o.effects3 = v.effects3;
  o.effects4 = v.effects4;
  o.effects5 = v.effects5;
  o.effects6 = v.effects6;
  o.effects7 = v.effects7;
  o.effects8 = v.effects8;
  o.effects9 = v.effects9;
  o.effects10 = v.effects10;
  o.effects11 = v.effects11;
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
  return textureSample(tex1, smp1, uv);
}

fn apply_tonecurve(color_in: vec3<f32>, row: f32, sat: f32) -> vec3<f32> {
  var color = color_in;
  let gray = dot(color, vec3<f32>(0.2989, 0.5886, 0.1145));
  color = mix(color, vec3<f32>(gray, gray, gray), clamp(sat, 0.0, 1.0));
  let y = clamp(row, 0.0, 1.0);
  let r = textureSample(tex2, smp2, vec2<f32>(clamp(color.r, 0.0, 1.0), y)).r;
  let g = textureSample(tex2, smp2, vec2<f32>(clamp(color.g, 0.0, 1.0), y)).g;
  let b = textureSample(tex2, smp2, vec2<f32>(clamp(color.b, 0.0, 1.0), y)).b;
  return vec3<f32>(r, g, b);
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
  return dot(vec3<f32>(0.299, 0.587, 0.114), color.rgb);
}

fn sample_shimi(uv: vec2<f32>, fade: f32, progress: f32) -> vec4<f32> {
  var color = sample_tex0_safe(uv);
  if (rgb_brightness(color) > progress) {
    color.a = color.a * max(fade - mix(fade, 0.0, progress), 0.0);
  }
  return color;
}

fn sample_shimi_inv(uv: vec2<f32>, fade: f32, progress: f32) -> vec4<f32> {
  var color = sample_tex0_safe(uv);
  if (rgb_brightness(color) < 1.0 - progress) {
    color.a = color.a * max(fade - mix(fade, 0.0, progress), 0.0);
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
  if (uv.x < 0.0 || uv.y < 0.0 || uv.x > 1.0 || uv.y > 1.0) {
    return 1.0;
  }
  let dims_u = textureDimensions(shadow_tex, 0);
  let texel = vec2<f32>(1.0 / max(f32(dims_u.x), 1.0), 1.0 / max(f32(dims_u.y), 1.0));
  let current = clamp(ndc.z * 0.5 + 0.5, 0.0, 1.0);
  let bias = max(vs_u.mesh_misc.y, 0.0005);
  var vis = 0.0;
  for (var oy: i32 = -1; oy <= 1; oy = oy + 1) {
    for (var ox: i32 = -1; ox <= 1; ox = ox + 1) {
      let sample_uv = uv + vec2<f32>(f32(ox), f32(oy)) * texel;
      let stored = textureSample(shadow_tex, shadow_smp, sample_uv).r;
      vis = vis + select(0.35, 1.0, current <= stored + bias);
    }
  }
  return vis / 9.0;
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
  var L = vec3<f32>(0.0, 0.0, 1.0);
  var attenuation = 1.0;
  if (kind == 0) {
    L = normalize(-light_dir);
  } else {
    let dir_point = light_pos - world_pos;
    let distance_point = max(length(dir_point), 1e-5);
    L = dir_point / distance_point;
    attenuation = 1.0 / max(light_atten.x + light_atten.y * distance_point + light_atten.z * distance_point * distance_point, 1e-5);
    attenuation = attenuation * clamp(1.0 - distance_point / max(light_atten.w, 1.0), 0.0, 1.0);
    if (kind >= 2) {
      let rho = dot(normalize(dir_point), normalize(-light_dir));
      if (rho <= light_cone.y) {
        attenuation = 0.0;
      } else if (rho < light_cone.x) {
        attenuation = attenuation * pow((rho - light_cone.y) / max(light_cone.x - light_cone.y, 1e-5), max(light_cone.z, 0.01));
      }
    }
  }
  let V = normalize(vs_u.camera_eye.xyz - world_pos);
  let H = normalize(L + V);
  let ndotl_raw = dot(N, L);
  let ndotl = max(ndotl_raw, 0.0);
  let half_lambert = clamp(ndotl_raw * 0.5 + 0.5, 0.0, 1.0);
  let ndoth = max(dot(N, H), 0.0);
  let rdotv = max(dot(reflect(-L, N), V), 0.0);
  var visibility = 1.0;
  if (shadow_enabled && (shading_type == 1 || kind == 3)) {
    visibility = sample_shadow_visibility(shadow_pos);
  }
  let ambient_term = base_rgb * mtrl_ambient * light_ambient;
  var diffuse_strength = ndotl;
  if (lighting_type == 4) { diffuse_strength = half_lambert; }
  var diffuse_color = light_diffuse;
  if (lighting_type == 5) { diffuse_color = diffuse_color * sample_toon_tex(diffuse_strength); }
  var specular_strength = pow(ndoth, mtrl_power);
  if (lighting_type == 6 || lighting_type == 7) { specular_strength = pow(rdotv, mtrl_power); }
  if (lighting_type == 1 || lighting_type == 4 || lighting_type == 5 || lighting_type == 0) { specular_strength = 0.0; }
  let diffuse_term = base_rgb * diffuse_color * diffuse_strength * attenuation;
  let specular_term = mtrl_specular * light_specular * specular_strength * attenuation;
  return ambient_term + (diffuse_term + specular_term) * visibility;
}

fn mesh_lighting(
  base_rgb: vec3<f32>,
  world_pos: vec3<f32>,
  world_normal: vec3<f32>,
  world_tangent: vec3<f32>,
  world_binormal: vec3<f32>,
  shaded_uv: vec2<f32>,
  light_pos_kind: vec4<f32>,
  light_dir_shadow: vec4<f32>,
  light_atten: vec4<f32>,
  light_cone: vec4<f32>,
  shadow_pos: vec4<f32>
) -> vec3<f32> {
  let lighting_type = i32(round(vs_u.mtrl_params.y));
  let rim_power = max(vs_u.mtrl_params.w, 0.0);
  let mtrl_emissive = vs_u.mtrl_emissive.rgb;
  var N = normalize(world_normal);
  if (lighting_type == 8 || lighting_type == 9) {
    N = apply_normal_map(N, world_tangent, world_binormal, shaded_uv);
  }
  var accum = mtrl_emissive;
  let dir_count = i32(round(vs_u.mesh_light_counts.x));
  let point_count = i32(round(vs_u.mesh_light_counts.y));
  let spot_count = i32(round(vs_u.mesh_light_counts.z));
  if (dir_count + point_count + spot_count > 0) {
    for (var i: i32 = 0; i < 4; i = i + 1) {
      if (i < dir_count) {
        accum = accum + mesh_light_contrib(base_rgb, world_pos, N, shaded_uv, vs_u.dir_light_diffuse[i].rgb, vs_u.dir_light_ambient[i].rgb, vs_u.dir_light_specular[i].rgb, 0, vec3<f32>(0.0), vs_u.dir_light_dir[i].xyz, vec4<f32>(1.0, 0.0, 0.0, 0.0), vec4<f32>(0.0), shadow_pos, false);
      }
      if (i < point_count) {
        accum = accum + mesh_light_contrib(base_rgb, world_pos, N, shaded_uv, vs_u.point_light_diffuse[i].rgb, vs_u.point_light_ambient[i].rgb, vs_u.point_light_specular[i].rgb, 1, vs_u.point_light_pos[i].xyz, vec3<f32>(0.0, 0.0, -1.0), vs_u.point_light_atten[i], vec4<f32>(0.0), shadow_pos, false);
      }
      if (i < spot_count) {
        accum = accum + mesh_light_contrib(base_rgb, world_pos, N, shaded_uv, vs_u.spot_light_diffuse[i].rgb, vs_u.spot_light_ambient[i].rgb, vs_u.spot_light_specular[i].rgb, 2 + i32(vs_u.spot_light_cone[i].w > 0.5), vs_u.spot_light_pos[i].xyz, vs_u.spot_light_dir[i].xyz, vs_u.spot_light_atten[i], vs_u.spot_light_cone[i], shadow_pos, vs_u.spot_light_cone[i].w > 0.5);
      }
    }
  } else {
    let kind = i32(round(light_pos_kind.w));
    accum = accum + mesh_light_contrib(base_rgb, world_pos, N, shaded_uv, vs_u.light_diffuse_u.rgb, vs_u.light_ambient_u.rgb, vs_u.light_specular_u.rgb, kind, light_pos_kind.xyz, light_dir_shadow.xyz, light_atten, light_cone, shadow_pos, kind == 3 && light_dir_shadow.w > 0.5);
  }
  var rim_term = vec3<f32>(0.0, 0.0, 0.0);
  let shader_option_bits = i32(round(vs_u.mtrl_extra.z));
  if (rim_power > 0.0 && (shader_option_bits & 1) != 0) {
    let V = normalize(vs_u.camera_eye.xyz - world_pos);
    let rim = pow(clamp(1.0 - max(dot(N, V), 0.0), 0.0, 1.0), max(rim_power, 1e-3));
    rim_term = vs_u.mtrl_rim.rgb * rim;
  }
  return clamp(accum + rim_term, vec3<f32>(0.0), vec3<f32>(8.0));
}

fn fs_common_2d(i: VsOut2d) -> vec4<f32> {
  let has_mask = i.effects5.x;
  let has_tonecurve = i.effects5.y;
  let tonecurve_row = i.effects5.z;
  let tonecurve_sat = i.effects5.w;
  let tr = i.effects1.x;
  let mono = i.effects1.y;
  let rev = i.effects1.z;
  let bright = i.effects1.w;
  let dark = i.effects2.x;
  let color_rate = i.effects2.y;
  let color_add = vec3<f32>(i.effects2.z, i.effects2.w, i.effects3.x);
  let color_tgt = vec3<f32>(i.effects3.y, i.effects3.z, i.effects3.w);
  let mask_mode = i.effects4.x;
  let alpha_test = i.effects4.y;
  let light_on = i.effects4.z;
  let fog_on = i.effects4.w;
  let wipe_mode = i.effects6.x;
  let wipe_p0 = i.effects6.y;
  let wipe_p1 = i.effects6.z;
  let wipe_p2 = i.effects6.w;
  let wipe_p3 = i.effects7.x;
  let has_wipe_src = i.effects7.y;
  let blend_code = i.effects7.z;
  let wipe_aux1 = i.effects7.w;
  let light_factor = i.effects8.w;
  let alpha_ref = max(vs_u.mtrl_extra.y, 0.001);
  let fog_scroll_x = i.effects9.w;
  let fog_color = i.effects10.xyz;
  let sprite_z = i.effects10.w;
  let fog_near = i.effects11.x;
  let fog_far = i.effects11.y;
  let has_fog_tex = i.effects11.z;
  let camera_z = i.effects11.w;

  var c = textureSample(tex0, smp0, i.uv);
  if (wipe_mode > 0.5 && wipe_mode < 1.5) {
    c = sample_mosaic(i.uv, wipe_p0, wipe_p1);
  } else if (wipe_mode > 1.5 && wipe_mode < 2.5) {
    c = sample_raster_h(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3);
  } else if (wipe_mode > 2.5 && wipe_mode < 3.5) {
    c = sample_raster_v(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3);
  } else if (wipe_mode > 3.5 && wipe_mode < 4.5) {
    c = sample_explosion_blur(i.uv, vec2<f32>(wipe_p0, wipe_p1), wipe_p2, wipe_p3);
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
    let oldc = sample_raster_h_tex3(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3);
    let newc = sample_raster_h(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3);
    c = mix(oldc, newc, clamp(wipe_p3, 0.0, 1.0));
  } else if (wipe_mode > 11.5 && wipe_mode < 12.5 && has_wipe_src > 0.5) {
    let oldc = sample_raster_v_tex3(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3);
    let newc = sample_raster_v(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3);
    c = mix(oldc, newc, clamp(wipe_p3, 0.0, 1.0));
  } else if (wipe_mode > 12.5 && wipe_mode < 13.5 && has_wipe_src > 0.5) {
    let oldc = sample_explosion_blur_tex3(i.uv, vec2<f32>(wipe_p0, wipe_p1), wipe_p2, wipe_p3);
    let newc = sample_explosion_blur(i.uv, vec2<f32>(wipe_p0, wipe_p1), wipe_p2, wipe_p3);
    c = mix(oldc, newc, clamp(tonecurve_row, 0.0, 1.0));
  }

  var rgb = c.rgb;
  var alpha = c.a;

  if (has_tonecurve > 0.5) {
    rgb = apply_tonecurve(rgb, tonecurve_row, tonecurve_sat);
  }

  if (light_on > 0.5) {
    let lit = clamp(vs_u.light_ambient_u.rgb + vs_u.light_diffuse_u.rgb * light_factor, vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(2.0, 2.0, 2.0));
    rgb = clamp(rgb * lit + vs_u.mtrl_emissive.rgb, vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(8.0, 8.0, 8.0));
  }

  rgb = mix(rgb, vec3<f32>(1.0, 1.0, 1.0) - rgb, rev);
  let gray = dot(rgb, vec3<f32>(0.299, 0.587, 0.114));
  rgb = mix(rgb, vec3<f32>(gray, gray, gray), mono);
  rgb = rgb + vec3<f32>(bright, bright, bright);
  rgb = clamp(rgb - vec3<f32>(dark, dark, dark), vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(1.0, 1.0, 1.0));
  rgb = mix(rgb, color_tgt, color_rate);
  rgb = clamp(rgb + color_add, vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(1.0, 1.0, 1.0));

  if (has_mask > 0.5) {
    let m = sample_mask(i.uv_aux);
    let mask_luma = dot(m.rgb, vec3<f32>(0.299, 0.587, 0.114));
    alpha = alpha * mask_luma * m.a;
  }

  if (fog_on > 0.5) {
    let depth = abs(sprite_z - camera_z);
    let fog_t = clamp((depth - fog_near) / max(fog_far - fog_near, 1.0), 0.0, 1.0);
    if (fog_t > 0.0) {
      var fog_rgb = fog_color;
      if (has_fog_tex > 0.5) {
        let dims_u = textureDimensions(tex4, 0);
        let fw = max(f32(dims_u.x), 1.0);
        let fh = max(f32(dims_u.y), 1.0);
        let fog_uv = vec2<f32>(fract((i.pos.x + fog_scroll_x) / fw), fract(i.pos.y / fh));
        let fog_sample = sample_tex4_safe(fog_uv);
        fog_rgb = mix(fog_rgb, fog_sample.rgb, fog_sample.a);
      }
      rgb = mix(rgb, fog_rgb, fog_t);
      alpha = alpha * (1.0 - 0.25 * fog_t);
    }
  }

  if (mask_mode > 0.5 && mask_mode < 1.5) {
    alpha = gray;
  }

  if (alpha_test > 0.5 && alpha <= alpha_ref) {
    discard;
  }

  let a = alpha * i.alpha * tr;
  if (blend_code > 2.5 && blend_code < 3.5) {
    let mul_rgb = mix(vec3<f32>(1.0, 1.0, 1.0), rgb, a);
    return vec4<f32>(mul_rgb, a);
  }
  if (blend_code > 4.5 && blend_code < 5.5) {
    let dims_u = textureDimensions(tex3, 0);
    let screen_uv = vec2<f32>(
      clamp(i.pos.x / max(f32(dims_u.x), 1.0), 0.0, 1.0),
      clamp(i.pos.y / max(f32(dims_u.y), 1.0), 0.0, 1.0)
    );
    let dst = sample_tex3_safe(screen_uv);
    let ov = overlay_rgb(dst.rgb, rgb);
    let out_rgb = mix(dst.rgb, ov, a);
    return vec4<f32>(out_rgb, 1.0);
  }
  return vec4<f32>(rgb * a, a);
}

fn fs_common(i: VsOut) -> vec4<f32> {
  let has_mask = i.effects5.x;
  let has_tonecurve = i.effects5.y;
  let tonecurve_row = i.effects5.z;
  let tonecurve_sat = i.effects5.w;
  let tr = i.effects1.x;
  let mono = i.effects1.y;
  let rev = i.effects1.z;
  let bright = i.effects1.w;
  let dark = i.effects2.x;
  let color_rate = i.effects2.y;
  let color_add = vec3<f32>(i.effects2.z, i.effects2.w, i.effects3.x);
  let color_tgt = vec3<f32>(i.effects3.y, i.effects3.z, i.effects3.w);
  let mask_mode = i.effects4.x;
  let alpha_test = i.effects4.y;
  let light_on = i.effects4.z;
  let fog_on = i.effects4.w;
  let wipe_mode = i.effects6.x;
  let wipe_p0 = i.effects6.y;
  let wipe_p1 = i.effects6.z;
  let wipe_p2 = i.effects6.w;
  let wipe_p3 = i.effects7.x;
  let has_wipe_src = i.effects7.y;
  let blend_code = i.effects7.z;
  let wipe_aux1 = i.effects7.w;
  let light_factor = i.effects8.w;
  let world_pos = i.world_pos.xyz;
  let alpha_ref = max(vs_u.mtrl_extra.y, 0.001);
  let world_has_pos = i.world_pos.w > 0.5;
  let world_normal = i.world_normal.xyz;
  let world_tangent = i.world_tangent.xyz;
  let world_binormal = i.world_binormal.xyz;
  let light_pos_kind = i.light_pos_kind;
  let light_dir_shadow = i.light_dir_shadow;
  let light_atten = i.light_atten;
  let light_cone = i.light_cone;
  let fog_scroll_x = i.effects9.w;
  let fog_color = i.effects10.xyz;
  let sprite_z = i.effects10.w;
  let fog_near = i.effects11.x;
  let fog_far = i.effects11.y;
  let has_fog_tex = i.effects11.z;
  let camera_z = i.effects11.w;

  var shaded_uv = i.uv;
  if (vs_u.flags.x > 0.5 && i.world_pos.w > 0.5 && i.world_normal.w > 0.5) {
    let lighting_type = i32(round(vs_u.mtrl_params.y));
    if (lighting_type == 9) {
      let view_dir_world = normalize(vs_u.camera_eye.xyz - world_pos);
      shaded_uv = apply_parallax_uv(world_normal, world_tangent, world_binormal, i.uv, view_dir_world, max(vs_u.mtrl_extra.x, 0.0));
    }
  }
  let mesh_use_tex = vs_u.mesh_flags.x > 0.5;
  let mesh_use_mrbd = vs_u.mesh_flags.y > 0.5;
  let mesh_use_rgb = vs_u.mesh_flags.z > 0.5;
  let mesh_use_mul_vertex_color = vs_u.mesh_flags.w > 0.5;
  var c = select(vec4<f32>(1.0, 1.0, 1.0, 1.0), textureSample(tex0, smp0, shaded_uv), mesh_use_tex);
  if (wipe_mode > 0.5 && wipe_mode < 1.5) {
    c = sample_mosaic(i.uv, wipe_p0, wipe_p1);
  } else if (wipe_mode > 1.5 && wipe_mode < 2.5) {
    c = sample_raster_h(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3);
  } else if (wipe_mode > 2.5 && wipe_mode < 3.5) {
    c = sample_raster_v(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3);
  } else if (wipe_mode > 3.5 && wipe_mode < 4.5) {
    c = sample_explosion_blur(i.uv, vec2<f32>(wipe_p0, wipe_p1), wipe_p2, wipe_p3);
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
    let oldc = sample_raster_h_tex3(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3);
    let newc = sample_raster_h(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3);
    c = mix(oldc, newc, clamp(wipe_p3, 0.0, 1.0));
  } else if (wipe_mode > 11.5 && wipe_mode < 12.5 && has_wipe_src > 0.5) {
    let oldc = sample_raster_v_tex3(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3);
    let newc = sample_raster_v(i.uv, wipe_p0, wipe_p1, wipe_p2, wipe_p3);
    c = mix(oldc, newc, clamp(wipe_p3, 0.0, 1.0));
  } else if (wipe_mode > 12.5 && wipe_mode < 13.5 && has_wipe_src > 0.5) {
    let oldc = sample_explosion_blur_tex3(i.uv, vec2<f32>(wipe_p0, wipe_p1), wipe_p2, wipe_p3);
    let newc = sample_explosion_blur(i.uv, vec2<f32>(wipe_p0, wipe_p1), wipe_p2, wipe_p3);
    c = mix(oldc, newc, clamp(tonecurve_row, 0.0, 1.0));
  }

  var rgb = c.rgb;
  var alpha = c.a;
  if (mesh_use_mul_vertex_color) {
    let vc_rate = clamp(vs_u.mesh_misc.x, 0.0, 1.0);
    let vertex_color = vec4<f32>(i.effects8.x, i.effects8.y, i.effects8.z, i.effects9.x);
    rgb = mix(rgb, rgb * vertex_color.rgb, vc_rate);
    alpha = alpha * mix(1.0, vertex_color.a, vc_rate);
  }
  if (vs_u.flags.x > 0.5) {
    rgb = rgb * vs_u.mtrl_diffuse.rgb;
    alpha = alpha * vs_u.mtrl_diffuse.a;
  }

  if (light_on > 0.5) {
    if (world_has_pos && length(world_normal) > 0.25) {
      rgb = mesh_lighting(rgb, world_pos, world_normal, world_tangent, world_binormal, shaded_uv, light_pos_kind, light_dir_shadow, light_atten, light_cone, i.shadow_pos);
    } else {
      let lit = clamp(vs_u.light_ambient_u.rgb + vs_u.light_diffuse_u.rgb * light_factor, vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(2.0, 2.0, 2.0));
      rgb = clamp(rgb * lit + vs_u.mtrl_emissive.rgb, vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(8.0, 8.0, 8.0));
    }
  } else if (vs_u.flags.x > 0.5) {
    rgb = clamp(rgb + vs_u.mtrl_emissive.rgb, vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(8.0, 8.0, 8.0));
  }

  if (mesh_use_mrbd) {
    let mesh_mono = clamp(vs_u.mesh_mrbd.x, 0.0, 1.0);
    let mesh_rev = clamp(vs_u.mesh_mrbd.y, 0.0, 1.0);
    let mesh_bright = max(vs_u.mesh_mrbd.z, 0.0);
    let mesh_dark = max(vs_u.mesh_mrbd.w, 0.0);
    rgb = mix(rgb, vec3<f32>(1.0, 1.0, 1.0) - rgb, mesh_rev);
    let mesh_gray = dot(rgb, vec3<f32>(0.299, 0.587, 0.114));
    rgb = mix(rgb, vec3<f32>(mesh_gray, mesh_gray, mesh_gray), mesh_mono);
    rgb = clamp(rgb + vec3<f32>(mesh_bright, mesh_bright, mesh_bright), vec3<f32>(0.0), vec3<f32>(1.0));
    rgb = clamp(rgb - vec3<f32>(mesh_dark, mesh_dark, mesh_dark), vec3<f32>(0.0), vec3<f32>(1.0));
  }

  if (mesh_use_rgb) {
    let mesh_rgb_tgt = clamp(vs_u.mesh_rgb_rate.xyz, vec3<f32>(0.0), vec3<f32>(1.0));
    let mesh_rgb_rate = clamp(vs_u.mesh_rgb_rate.w, 0.0, 1.0);
    rgb = mix(rgb, mesh_rgb_tgt, mesh_rgb_rate);
    rgb = clamp(rgb + vs_u.mesh_add_rgb.xyz, vec3<f32>(0.0), vec3<f32>(1.0));
  }

  if (has_tonecurve > 0.5) {
    rgb = apply_tonecurve(rgb, tonecurve_row, tonecurve_sat);
  }

  rgb = mix(rgb, vec3<f32>(1.0, 1.0, 1.0) - rgb, rev);
  let gray = dot(rgb, vec3<f32>(0.299, 0.587, 0.114));
  rgb = mix(rgb, vec3<f32>(gray, gray, gray), mono);
  rgb = rgb + vec3<f32>(bright, bright, bright);
  rgb = clamp(rgb - vec3<f32>(dark, dark, dark), vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(1.0, 1.0, 1.0));
  rgb = mix(rgb, color_tgt, color_rate);
  rgb = clamp(rgb + color_add, vec3<f32>(0.0, 0.0, 0.0), vec3<f32>(1.0, 1.0, 1.0));

  if (has_mask > 0.5) {
    let m = sample_mask(i.uv_aux);
    let mask_luma = dot(m.rgb, vec3<f32>(0.299, 0.587, 0.114));
    alpha = alpha * mask_luma * m.a;
  }

  if (fog_on > 0.5) {
    var depth = abs(sprite_z - camera_z);
    if (world_has_pos) {
      depth = length(world_pos - vec3<f32>(0.0, 0.0, camera_z));
    }
    let fog_t = clamp((depth - fog_near) / max(fog_far - fog_near, 1.0), 0.0, 1.0);
    if (fog_t > 0.0) {
      var fog_rgb = fog_color;
      if (has_fog_tex > 0.5) {
        let dims_u = textureDimensions(tex4, 0);
        let fw = max(f32(dims_u.x), 1.0);
        let fh = max(f32(dims_u.y), 1.0);
        let fog_uv = vec2<f32>(fract((i.pos.x + fog_scroll_x) / fw), fract(i.pos.y / fh));
        let fog_sample = sample_tex4_safe(fog_uv);
        fog_rgb = mix(fog_rgb, fog_sample.rgb, fog_sample.a);
      }
      rgb = mix(rgb, fog_rgb, fog_t);
      alpha = alpha * (1.0 - 0.25 * fog_t);
    }
  }

  if (mask_mode > 0.5 && mask_mode < 1.5) {
    alpha = gray;
  }

  if (alpha_test > 0.5 && alpha <= alpha_ref) {
    discard;
  }

  let a = alpha * i.alpha * tr;
  if (blend_code > 2.5 && blend_code < 3.5) {
    let mul_rgb = mix(vec3<f32>(1.0, 1.0, 1.0), rgb, a);
    return vec4<f32>(mul_rgb, a);
  }
  if (blend_code > 4.5 && blend_code < 5.5) {
    let dims_u = textureDimensions(tex3, 0);
    let screen_uv = vec2<f32>(
      clamp(i.pos.x / max(f32(dims_u.x), 1.0), 0.0, 1.0),
      clamp(i.pos.y / max(f32(dims_u.y), 1.0), 0.0, 1.0)
    );
    let dst = sample_tex3_safe(screen_uv);
    let ov = overlay_rgb(dst.rgb, rgb);
    let out_rgb = mix(dst.rgb, ov, a);
    return vec4<f32>(out_rgb, 1.0);
  }
  return vec4<f32>(rgb * a, a);
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
