//! WGPU renderer for Siglus stage composition.
//!
//! This renderer consumes a painter-ordered list of sprites and draws them
//! in order. It supports fixed sprite effects, dual-source wipes, and a
//! depth-backed path for 3D-transformed quads.

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use std::collections::{HashMap, HashSet};
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::assets::load_image_any;
use crate::image_manager::{ImageHandle, ImageKey, ImageManager};
use crate::layer::{
    ClipRect, RenderFrame, RenderSprite, SpriteBlend, SpriteFit, SpriteSizeMode, WipeRenderPlan,
};
use crate::mesh3d::{MeshAsset, load_mesh_asset};
use crate::render_math::sprite_quad_geometry_rect;
use crate::runtime::FrameCaptureBackend;

mod emote;
mod mipmap;

use crate::render_plan::*;

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

struct VertexSprite2d;

impl VertexSprite2d {
    const ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x2,
        2 => Float32x2,
        3 => Float32,
        4 => Float32x4,
        5 => Float32x4
    ];

    fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<VertexSprite2dData>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

#[derive(Debug)]
pub struct Renderer {
    pub surface: wgpu::Surface<'static>,
    /// Kept so that a replacement surface is created from the same instance (and
    /// therefore the same backend) as `device`. Android re-creates the
    /// ANativeWindow on every activity stop, and a surface built from a fresh
    /// instance may land on a different backend (GLES instead of Vulkan), which
    /// makes `Surface::configure` fail validation against the existing device.
    pub instance: wgpu::Instance,
    /// The adapter `device` came from, kept so a replacement surface can be
    /// checked against the configuration before it is applied.
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    /// Original Siglus `wait_display_vsync_total` state. The concrete wgpu
    /// present mode is selected from the current surface capabilities.
    wait_display_vsync: bool,
    logical_width: f32,
    logical_height: f32,
    scale_factor: f32,
    surface_viewport: SurfaceViewport,

    pipelines: HashMap<RenderPipelineKey, wgpu::RenderPipeline>,
    bind_group_layout: wgpu::BindGroupLayout,
    shader: wgpu::ShaderModule,
    pipeline_layout: wgpu::PipelineLayout,
    wipe_bind_group_layout: wgpu::BindGroupLayout,
    wipe_pipeline: wgpu::RenderPipeline,
    page_wipe_bind_group_layout: wgpu::BindGroupLayout,
    page_wipe_pipeline: wgpu::RenderPipeline,

    vertex_buf: wgpu::Buffer,
    vertex_capacity: usize,
    vertex_sprite2d_buf: wgpu::Buffer,

    // D3D9 keeps shader constants as device state instead of allocating a
    // constant buffer per sprite.  Mirror that model with one dynamic-uniform
    // arena per frame; each draw selects its aligned slice with a dynamic offset.
    vs_uniform_buf: wgpu::Buffer,
    vs_uniform_capacity: usize,
    vs_uniform_stride: usize,
    vs_uniform_staging: Vec<u8>,
    zero_bone_uniform_buf: wgpu::Buffer,

    // Numeric keys must not own the runtime images they identify.
    textures: HashMap<ImageKey, GpuTexture>,
    mipmap_generator: mipmap::MipmapGenerator,
    external_textures: HashMap<PathBuf, GpuTexture>,
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
    /// The scene/wipe targets and `depth` have the logical size (they are
    /// 1x1 until a frame first needs them).
    offscreen_ready: bool,
    /// The shadow map has its size (1x1 until a shadow is first cast).
    shadow_ready: bool,

    sprite2d_verts: Vec<VertexSprite2dData>,
    /// The frame's draw commands (see `crate::render_plan`).
    plan: FramePlanner,
    draw_gpu_slots: Vec<DrawGpuSlot>,
    shared_draw_bind_groups: HashMap<DrawBindKey, Arc<wgpu::BindGroup>>,
    draw_bind_epoch: u64,
    debug_frame_serial: u64,
    emote_compositor: emote::EmoteCompositor,
}

fn align_up_usize(value: usize, alignment: usize) -> usize {
    debug_assert!(alignment > 0);
    let rem = value % alignment;
    if rem == 0 {
        value
    } else {
        value.saturating_add(alignment - rem)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RendererMemoryStats {
    pub image_texture_bytes: u64,
    pub external_texture_bytes: u64,
    pub internal_color_target_bytes: u64,
    pub internal_depth_target_bytes: u64,
    pub renderer_gpu_buffer_bytes: u64,
    pub frame_arena_capacity_bytes: usize,
    pub image_texture_count: usize,
    pub external_texture_count: usize,
    pub cached_render_pipeline_count: usize,
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
    Image(ImageKey),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DrawBindKey {
    image_id: Option<ImageKey>,
    emote_render_id: Option<u64>,
    mesh_texture_path: Option<PathBuf>,
    mesh_normal_texture_path: Option<PathBuf>,
    mesh_toon_texture_path: Option<PathBuf>,
    mask_image_id: Option<ImageKey>,
    tonecurve_image_id: Option<ImageKey>,
    fog_image_id: Option<ImageKey>,
    wipe_src_image_id: Option<ImageKey>,
    overlay_backdrop: Option<BackdropTarget>,
    mesh_base_sampler: bool,
    use_bone_uniform: bool,
}

impl DrawBindKey {
    fn from_command(cmd: &DrawCommand, overlay_backdrop: Option<BackdropTarget>) -> Self {
        Self {
            image_id: cmd.image_id.as_ref().map(|id| id.key()),
            emote_render_id: cmd.emote_render_id,
            mesh_texture_path: cmd.mesh_texture_path.clone(),
            mesh_normal_texture_path: cmd.mesh_normal_texture_path.clone(),
            mesh_toon_texture_path: cmd.mesh_toon_texture_path.clone(),
            mask_image_id: cmd.mask_image_id.as_ref().map(|id| id.key()),
            tonecurve_image_id: cmd.tonecurve_image_id.as_ref().map(|id| id.key()),
            fog_image_id: cmd.fog_image_id.as_ref().map(|id| id.key()),
            wipe_src_image_id: cmd.wipe_src_image_id.as_ref().map(|id| id.key()),
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
            use_bone_uniform: draw_uses_bone_uniform(cmd),
        }
    }
}

#[derive(Debug)]
struct DrawGpuSlot {
    bone_uniform_buf: Option<wgpu::Buffer>,
    bind_group: Option<Arc<wgpu::BindGroup>>,
    bind_key: Option<DrawBindKey>,
    bind_epoch: u64,
}

fn draw_uses_bone_uniform(cmd: &DrawCommand) -> bool {
    cmd.bone_uniform_index.is_some()
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

fn d3d_front_face() -> wgpu::FrontFace {
    // Tona3 enables D3DCULL_CCW, which removes counter-clockwise triangles;
    // Direct3D's surviving/front triangles are therefore clockwise.
    wgpu::FrontFace::Cw
}

type DebugTextureReadback = (u32, u32, u64, Vec<u8>);

fn present_mode_for_wait_display_vsync(
    wait_display_vsync: bool,
    supported: &[wgpu::PresentMode],
) -> wgpu::PresentMode {
    if wait_display_vsync {
        // D3DPRESENT_INTERVAL_ONE. Fifo is guaranteed by wgpu on every
        // presentable surface and is the direct VSync-on equivalent.
        return wgpu::PresentMode::Fifo;
    }

    // D3DPRESENT_INTERVAL_IMMEDIATE. Prefer the exact no-VSync mode. When a
    // backend cannot expose it (notably some Wayland paths), AutoNoVsync's
    // documented fallback order is effectively Immediate -> Mailbox -> Fifo;
    // select the concrete mode ourselves so surface replacement can validate it.
    if supported.contains(&wgpu::PresentMode::Immediate) {
        wgpu::PresentMode::Immediate
    } else if supported.contains(&wgpu::PresentMode::Mailbox) {
        wgpu::PresentMode::Mailbox
    } else {
        wgpu::PresentMode::Fifo
    }
}

impl Renderer {
    /// Human-readable presentation adapter used by the VM's compatibility
    /// variable. Platform renderers expose the same method without exposing
    /// their native graphics object to the host.
    pub fn adapter_name(&self) -> String {
        self.adapter.get_info().name
    }

    pub async fn new<W>(window: W) -> Result<Self>
    where
        W: std::ops::Deref<Target = dyn Window>
            + Into<wgpu::SurfaceTarget<'static>>
            + raw_window_handle::HasDisplayHandle
            + std::fmt::Debug
            + Clone
            + Send
            + Sync
            + 'static,
    {
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        let backends = wgpu::Backends::GL;
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        let backends = wgpu::Backends::all();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends,
            ..wgpu::InstanceDescriptor::new_with_display_handle(Box::new(window.clone()))
        });

        let size = window.surface_size();
        let scale_factor = window.scale_factor() as f32;
        let surface = instance.create_surface(window).context("create_surface")?;
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
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                raw_display_handle: Some(raw_display_handle),
                raw_window_handle,
            })
            .context("create_surface_unsafe")?;
        Self::new_from_instance_surface(instance, surface, width, height, scale_factor).await
    }

    /// Re-attach a new platform surface to the existing device/queue.
    ///
    /// Android destroys the `ANativeWindow` whenever the activity stops, so a
    /// background/foreground round trip hands us a new window while the engine
    /// state (VM, decoded resources) has to survive. The surface format is
    /// fixed by the platform, so the current configuration is reused and only
    /// the size changes; callers re-apply their logical viewport afterwards,
    /// exactly as they do after `resize`.
    ///
    /// # Safety
    ///
    /// Both handles must be valid and identify the same display/window pair.
    /// Their underlying native objects must remain valid until the replacement
    /// surface is dropped. Call on a thread permitted by the windowing platform.
    pub unsafe fn replace_surface_from_raw_handles(
        &mut self,
        raw_display_handle: raw_window_handle::RawDisplayHandle,
        raw_window_handle: raw_window_handle::RawWindowHandle,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let surface = unsafe {
            self.instance
                .create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle {
                    raw_display_handle: Some(raw_display_handle),
                    raw_window_handle,
                })
        }
        .context("create_surface_unsafe (replace)")?;
        // `Surface::configure` aborts the process on a validation error, so check
        // the new surface really is usable with this device first. A surface built
        // from a different instance (different VkInstance) is not.
        let caps = surface.get_capabilities(&self.adapter);
        if !caps.formats.contains(&self.config.format) {
            anyhow::bail!(
                "replacement surface does not support format {:?} (adapter formats: {:?})",
                self.config.format,
                caps.formats
            );
        }
        let replacement_present_mode =
            present_mode_for_wait_display_vsync(self.wait_display_vsync, &caps.present_modes);
        if !caps.alpha_modes.contains(&self.config.alpha_mode) {
            anyhow::bail!(
                "replacement surface does not support alpha mode {:?} (adapter modes: {:?})",
                self.config.alpha_mode,
                caps.alpha_modes
            );
        }
        if !caps.usages.contains(self.config.usage) {
            anyhow::bail!(
                "replacement surface does not support usage {:?} (adapter usages: {:?})",
                self.config.usage,
                caps.usages
            );
        }
        self.config.present_mode = replacement_present_mode;
        self.surface = surface;
        let scale_factor = self.scale_factor;
        self.resize_with_scale(width.max(1), height.max(1), scale_factor);
        Ok(())
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
                apply_limit_buckets: false,
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("request_adapter")?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("siglus-bg-device"),
                required_features: wgpu::Features::empty(),
                #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
                required_limits: wgpu::Limits::downlevel_webgl2_defaults(),
                #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
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
        let wait_display_vsync = true;
        let present_mode =
            present_mode_for_wait_display_vsync(wait_display_vsync, &surface_caps.present_modes);
        let config = wgpu::SurfaceConfiguration {
            color_space: wgpu::SurfaceColorSpace::Auto,
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
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(std::mem::size_of::<VsUniform>() as u64),
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
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let (wipe_bind_group_layout, wipe_pipeline) = create_wipe_pipeline(&device, config.format);
        let (page_wipe_bind_group_layout, page_wipe_pipeline) =
            create_page_wipe_pipeline(&device, config.format);

        // The original engine starts each shared 2D vertex buffer at 32 vertices.
        // Rust emits two triangles (6 vertices) instead of four indexed vertices, so
        // 48 vertices is the equivalent eight-quad starting capacity.
        let vertex_capacity = 48;
        let vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("siglus-sprite-vertex-buf"),
            size: (vertex_capacity * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let vertex_sprite2d_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("siglus-sprite2d-vertex-buf"),
            size: (vertex_capacity * std::mem::size_of::<VertexSprite2dData>())
                as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let uniform_alignment = device.limits().min_uniform_buffer_offset_alignment.max(1) as usize;
        let vs_uniform_stride = align_up_usize(std::mem::size_of::<VsUniform>(), uniform_alignment);
        let vs_uniform_capacity = 64usize;
        let vs_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("siglus-vs-uniform-arena"),
            size: (vs_uniform_stride * vs_uniform_capacity) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let zero_bone_uniform_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("siglus-zero-bone-uniform"),
            contents: bytemuck::bytes_of(&BoneUniform::zero()),
            usage: wgpu::BufferUsages::UNIFORM,
        });

        let mipmap_generator = mipmap::MipmapGenerator::new(&device);
        let default_aux =
            create_solid_texture(&device, &queue, &mipmap_generator, [255, 255, 255, 255])?;
        let fog_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("siglus-cfx-fog-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 0.0,
            ..Default::default()
        });
        let mesh_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("siglus-tona3-mesh-clamp-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });
        let normal_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("siglus-tona3-normal-clamp-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
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
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
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
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            lod_min_clamp: 0.0,
            lod_max_clamp: 0.0,
            ..Default::default()
        });
        // The offscreen targets and the shadow map start as 1x1 stand-ins
        // and get their size when first used (`ensure_offscreen_targets`,
        // `ensure_shadow_targets`): most frames draw straight to the surface,
        // and most games never cast a 3D shadow (at 1080p they are 66 MiB).
        let internal_width = 1;
        let internal_height = 1;
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
            create_render_target_texture(&device, 1, 1, config.format, "siglus-shadow-map");
        let shadow_depth = create_depth_texture_with_format(
            &device,
            1,
            1,
            wgpu::TextureFormat::Depth16Unorm,
            "siglus-shadow-depth-d16",
        );

        let surface_viewport = SurfaceViewport::full(config.width, config.height);
        let emote_compositor = emote::EmoteCompositor::new(&device);
        Ok(Self {
            instance,
            adapter,
            surface,
            device,
            queue,
            config,
            wait_display_vsync,
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
            vertex_sprite2d_buf,
            vs_uniform_buf,
            vs_uniform_capacity,
            vs_uniform_stride,
            vs_uniform_staging: Vec::new(),
            zero_bone_uniform_buf,
            textures: HashMap::new(),
            external_textures: HashMap::new(),
            default_aux,
            mipmap_generator,
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
            offscreen_ready: false,
            shadow_ready: false,
            sprite2d_verts: Vec::new(),
            plan: FramePlanner::default(),
            draw_gpu_slots: Vec::new(),
            shared_draw_bind_groups: HashMap::new(),
            draw_bind_epoch: 1,
            debug_frame_serial: 0,
            emote_compositor,
        })
    }

    /// Gives the offscreen targets their size before a frame renders
    /// through them; they are kept from then on.
    fn ensure_offscreen_targets(&mut self) {
        if !self.offscreen_ready {
            self.offscreen_ready = true;
            self.recreate_logical_render_targets();
        }
    }

    /// Gives the shadow map its size before the first shadow pass.
    fn ensure_shadow_targets(&mut self) {
        if self.shadow_ready {
            return;
        }
        self.shadow_ready = true;
        self.shadow_map = create_render_target_texture(
            &self.device,
            2048,
            2048,
            self.config.format,
            "siglus-shadow-map",
        );
        self.shadow_depth = create_depth_texture_with_format(
            &self.device,
            2048,
            2048,
            wgpu::TextureFormat::Depth16Unorm,
            "siglus-shadow-depth-d16",
        );
        // Draw bind groups hold the shadow map's view.
        self.clear_draw_bindings();
    }

    fn recreate_logical_render_targets(&mut self) {
        let (width, height) = if self.offscreen_ready {
            (
                self.logical_width.max(1.0).round() as u32,
                self.logical_height.max(1.0).round() as u32,
            )
        } else {
            (1, 1)
        };
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
        self.clear_draw_bindings();
    }

    /// Match `tnm_set_wait_display_vsync()` from the original engine.
    ///
    /// The script-side `SET_VSYNC_WAIT_OFF_FLAG` only changes local engine
    /// state; `eng_frame.cpp` applies the resulting total state once per frame.
    /// Reconfigure the surface only when that desired state changes, matching
    /// the original change-on-difference behavior.
    pub fn set_wait_display_vsync(&mut self, wait_display_vsync: bool) {
        if self.wait_display_vsync == wait_display_vsync {
            return;
        }
        self.wait_display_vsync = wait_display_vsync;

        let caps = self.surface.get_capabilities(&self.adapter);
        let present_mode =
            present_mode_for_wait_display_vsync(wait_display_vsync, &caps.present_modes);
        if !wait_display_vsync && present_mode != wgpu::PresentMode::Immediate {
            if present_mode == wgpu::PresentMode::Fifo {
                log::warn!(
                    "VSync-off requested by Siglus script, but this surface exposes neither Immediate nor Mailbox; falling back to Fifo"
                );
            } else {
                log::warn!(
                    "VSync-off requested by Siglus script, but Immediate is unavailable; using {:?}",
                    present_mode
                );
            }
        }

        if self.config.present_mode == present_mode {
            return;
        }
        self.config.present_mode = present_mode;
        self.surface.configure(&self.device, &self.config);
    }

    pub fn scale_factor(&self) -> f32 {
        self.scale_factor
    }

    pub fn surface_viewport(&self) -> (u32, u32, u32, u32) {
        (
            self.surface_viewport.x,
            self.surface_viewport.y,
            self.surface_viewport.w,
            self.surface_viewport.h,
        )
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        self.resize_with_scale(width, height, self.scale_factor);
    }

    pub fn resize_with_scale(&mut self, width: u32, height: u32, scale_factor: f32) {
        let sf = Self::valid_scale_factor(scale_factor);
        self.resize_targets(width, height, sf, width as f32 / sf, height as f32 / sf);
        self.surface_viewport = SurfaceViewport::full(self.config.width, self.config.height);
    }

    fn valid_scale_factor(scale_factor: f32) -> f32 {
        if scale_factor.is_finite() && scale_factor > 0.0 {
            scale_factor
        } else {
            1.0
        }
    }

    fn resize_targets(
        &mut self,
        width: u32,
        height: u32,
        scale_factor: f32,
        logical_width: f32,
        logical_height: f32,
    ) {
        if width == 0 || height == 0 {
            return;
        }
        let surface_changed = self.config.width != width || self.config.height != height;
        let previous_logical_size = self.logical_size();
        self.scale_factor = Self::valid_scale_factor(scale_factor);
        self.logical_width = logical_width.max(1.0);
        self.logical_height = logical_height.max(1.0);
        self.config.width = width;
        self.config.height = height;
        // Keep explicit reconfiguration for surface-loss recovery at the same size.
        self.surface.configure(&self.device, &self.config);
        if surface_changed {
            self.surface_depth = create_depth_texture(&self.device, width, height);
        }
        if previous_logical_size != self.logical_size() {
            self.recreate_logical_render_targets();
        }
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
        self.resize_targets(
            surface_width,
            surface_height,
            scale_factor,
            logical_width.max(1) as f32,
            logical_height.max(1) as f32,
        );
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
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                // Android hands the app a new ANativeWindow whenever the activity
                // stops, and a reconfigured surface can report Lost/Outdated for a
                // frame. Recover in place: failing here would surface as an error
                // from `SiglusHost::step`, which the Android frame loop treats as
                // "exit" and would freeze the picture.
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("surface validation failed");
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
        };
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
            self.submit(encoder);
        } else {
            self.render_ordinary_frame_to_surface(images, &frame_plan.sprites, &view)?;
        }

        self.queue.present(frame);
        Ok(())
    }

    fn render_ordinary_frame_to_surface(
        &mut self,
        images: &ImageManager,
        sprites: &[RenderSprite],
        view: &wgpu::TextureView,
    ) -> Result<()> {
        let _ = self.prepare_draws(images, sprites, true)?;
        let draw_count = self.plan.draws.len();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("siglus-direct-present-encoder"),
            });

        let shadow_indices: Vec<usize> = self
            .plan
            .draws
            .iter()
            .enumerate()
            .filter_map(|(idx, cmd)| cmd.shadow_cast.then_some(idx))
            .collect();
        if !shadow_indices.is_empty() {
            self.ensure_shadow_targets();
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
            0..draw_count,
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            true,
            None,
            None,
        )?;
        self.submit(encoder);
        Ok(())
    }

    fn prepare_draws(
        &mut self,
        images: &ImageManager,
        sprites: &[RenderSprite],
        external_target: bool,
    ) -> Result<std::ops::Range<u32>> {
        self.plan.verts.clear();
        self.plan.draws.clear();
        self.plan.draw_bone_uniforms.clear();

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
            if let Some(packet) = s.sprite.emote_render.as_deref() {
                self.emote_compositor
                    .prepare(&self.device, &self.queue, packet)?;
            }
        }
        self.plan.plan(
            images,
            sprites,
            &PlanTarget {
                win_w,
                win_h,
                surface_w,
                surface_h,
                surface_viewport,
            },
        )?;

        let blit_range = append_fullscreen_blit_vertices(&mut self.plan.verts);

        self.upload_prepared_vertices()?;
        self.upload_draw_uniforms();

        let mut live_image_ids = HashSet::new();
        for cmd in &self.plan.draws {
            if let Some(ref id) = cmd.image_id {
                live_image_ids.insert(id.key());
            }
            if let Some(ref id) = cmd.mask_image_id {
                live_image_ids.insert(id.key());
            }
            if let Some(ref id) = cmd.tonecurve_image_id {
                live_image_ids.insert(id.key());
            }
            if let Some(ref id) = cmd.fog_image_id {
                live_image_ids.insert(id.key());
            }
            if let Some(ref id) = cmd.wipe_src_image_id {
                live_image_ids.insert(id.key());
            }
        }
        for id in live_image_ids {
            self.ensure_texture_uploaded(images, id)?;
        }
        self.organize_textures(images);

        self.ensure_draw_pipelines();
        self.ensure_pipeline(sprite2d_copy_render_pipeline_key(), "siglus-sprite2d-copy");

        Ok(blit_range)
    }

    fn prepare_blit_vertices(&mut self) -> Result<std::ops::Range<u32>> {
        self.plan.verts.clear();
        self.plan.draws.clear();
        let range = append_fullscreen_blit_vertices(&mut self.plan.verts);
        self.upload_prepared_vertices()?;
        self.ensure_pipeline(sprite2d_copy_render_pipeline_key(), "siglus-sprite2d-copy");
        Ok(range)
    }

    fn render_frame_to_internal(
        &mut self,
        images: &ImageManager,
        frame: &RenderFrame,
    ) -> Result<BackdropTarget> {
        self.ensure_offscreen_targets();
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
        let draw_count = self.plan.draws.len();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("siglus-offscreen-scene-encoder"),
            });

        let shadow_indices: Vec<usize> = self
            .plan
            .draws
            .iter()
            .enumerate()
            .filter_map(|(idx, cmd)| cmd.shadow_cast.then_some(idx))
            .collect();
        if !shadow_indices.is_empty() {
            self.ensure_shadow_targets();
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
        while index < draw_count {
            let is_overlay = matches!(
                self.plan.draws[index].pipeline_key.technique.special,
                TechniqueSpecial::Overlay
            );
            let start = index;
            while index < draw_count
                && matches!(
                    self.plan.draws[index].pipeline_key.technique.special,
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
        self.submit(encoder);
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
            wgpu::TexelCopyTextureInfo {
                texture: &src_tex._tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
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
        self.submit(encoder);
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
        let Some(image) = generated_wipe_mask(wipe, width, height) else {
            self.wipe_mask_cache = None;
            return Ok(());
        };
        let texture = create_gpu_texture(
            &self.device,
            &self.queue,
            &self.mipmap_generator,
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

        if let Some(ref id) = wipe.mask_image_id {
            self.ensure_texture_uploaded(images, id.key())?;
        } else {
            self.ensure_generated_wipe_mask(wipe)?;
        }

        let uniform = wipe_uniform(
            wipe,
            self.logical_width.max(1.0),
            self.logical_height.max(1.0),
        );
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
        let external_mask = wipe
            .mask_image_id
            .as_ref()
            .and_then(|id| self.textures.get(&id.key()));
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
                multiview_mask: None,
                label: Some("siglus-wipe-composite-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    depth_slice: None,
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
        self.submit(encoder);
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
                multiview_mask: None,
                label: Some("siglus-page-wipe-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    depth_slice: None,
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
        self.submit(encoder);
        Ok(())
    }

    fn debug_collect_render_chain_textures(
        &self,
    ) -> HashMap<RendererDebugTextureKey, PendingRendererDebugTexture> {
        let mut pending: HashMap<RendererDebugTextureKey, PendingRendererDebugTexture> =
            HashMap::new();

        for (draw_idx, cmd) in self.plan.draws.iter().enumerate() {
            let role_prefix = format!("draw[{draw_idx}]");
            self.debug_add_base_texture_usage(&mut pending, cmd, &format!("{role_prefix}.base"));
            self.debug_add_image_texture_usage(
                &mut pending,
                cmd.mask_image_id.as_ref(),
                "image",
                &format!("{role_prefix}.mask"),
            );
            self.debug_add_image_texture_usage(
                &mut pending,
                cmd.tonecurve_image_id.as_ref(),
                "image",
                &format!("{role_prefix}.tonecurve"),
            );
            self.debug_add_image_texture_usage(
                &mut pending,
                cmd.fog_image_id.as_ref(),
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

        if self.plan.draws.iter().any(|cmd| {
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
        if self.plan.draws.is_empty() {
            self.debug_add_default_aux_usage(&mut pending, "empty-frame.default_aux");
        }

        pending
    }

    /// Metadata-only view of the current render-chain textures.  This is safe to
    /// call every HUD frame: it never submits GPU work, maps a buffer or waits on
    /// the device.  rgba is intentionally empty; F3 requests an explicit snapshot.
    pub fn debug_render_chain_texture_metadata(&self) -> Vec<RendererDebugTexture> {
        let pending = self.debug_collect_render_chain_textures();
        let mut items = Vec::with_capacity(pending.len());
        for (key, meta) in pending.into_iter() {
            items.push((
                meta.order,
                RendererDebugTexture {
                    key: Self::debug_texture_key_string(&key),
                    kind: meta.kind,
                    label: meta.label,
                    usage: meta.usage.join("; "),
                    usage_count: meta.usage.len(),
                    width: meta.width,
                    height: meta.height,
                    version: meta.version,
                    rgba: Vec::new(),
                },
            ));
        }
        items.sort_by_key(|(order, _)| *order);
        items.into_iter().map(|(_, item)| item).collect()
    }

    /// Explicit debug snapshot used only on user request.  Unlike the metadata
    /// path above, this performs GPU->CPU copies and waits for completion.
    pub fn debug_read_render_chain_textures(&self) -> Result<Vec<RendererDebugTexture>> {
        let pending = self.debug_collect_render_chain_textures();
        let mut items = Vec::with_capacity(pending.len());
        for (key, meta) in pending.into_iter() {
            let Some((width, height, version, rgba)) = self.debug_read_texture_by_key(&key)? else {
                continue;
            };
            items.push((
                meta.order,
                RendererDebugTexture {
                    key: Self::debug_texture_key_string(&key),
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
        image_id: Option<&ImageHandle>,
        kind: &str,
        usage: &str,
    ) {
        if let Some(id) = image_id
            && let Some(tex) = self.textures.get(&id.key())
        {
            self.debug_add_pending_texture_usage(
                pending,
                RendererDebugTextureKey::Image(id.key()),
                kind,
                format!("ImageHandle({})", id.index()),
                tex.width,
                tex.height,
                tex.version,
                usage,
            );
            return;
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
        if let Some(path) = path
            && let Some(tex) = self.external_textures.get(path)
        {
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
        if let Some(path) = cmd.mesh_texture_path.as_deref()
            && let Some(tex) = self.external_textures.get(path)
        {
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
        self.debug_add_image_texture_usage(pending, cmd.image_id.as_ref(), "image", usage);
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
        self.debug_add_image_texture_usage(pending, cmd.wipe_src_image_id.as_ref(), "image", usage);
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
            RendererDebugTextureKey::Image(id) => format!("image:{id}"),
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
    ) -> Result<Option<DebugTextureReadback>> {
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
        let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
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
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &output_buffer,
                layout: wgpu::TexelCopyBufferLayout {
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
        self.submit(encoder);

        let buffer_slice = output_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        buffer_slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .context("GPU readback completion")?;
        rx.recv()
            .context("wait for debug texture readback")?
            .context("map debug texture readback")?;
        let data = buffer_slice.get_mapped_range()?;
        let mut rgba = vec![0u8; (width as usize) * (height as usize) * 4];
        for y in 0..height as usize {
            let src_offset = y * padded_bytes_per_row as usize;
            let dst_offset = y * unpadded_bytes_per_row as usize;
            let src = &data[src_offset..src_offset + unpadded_bytes_per_row as usize];
            let dst = &mut rgba[dst_offset..dst_offset + unpadded_bytes_per_row as usize];
            match format {
                wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb => {
                    for (src_px, dst_px) in src
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .zip(dst.as_chunks_mut::<4>().0.iter_mut())
                    {
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

    fn ensure_external_texture(&mut self, path: &Path) -> Option<()> {
        if self.external_textures.contains_key(path) {
            return Some(());
        }
        let img = load_image_any(path, 0).ok()?;
        let tex = create_gpu_texture(
            &self.device,
            &self.queue,
            &self.mipmap_generator,
            &format!("siglus-external-texture-{}", self.external_textures.len()),
            &img,
            0,
        )
        .ok()?;
        self.external_textures.insert(path.to_path_buf(), tex);
        Some(())
    }

    fn ensure_draw_pipelines(&mut self) {
        for draw_idx in 0..self.plan.draws.len() {
            let (pipeline_key, pipeline_label, shadow) = {
                let cmd = &self.plan.draws[draw_idx];
                let pipeline_key = cmd.pipeline_key.render_pipeline_key();
                let pipeline_label = (!self.pipelines.contains_key(&pipeline_key))
                    .then(|| format!("siglus-{}", technique_name_for_pipeline(&cmd.pipeline_key)));
                let shadow = cmd.shadow_cast.then(|| {
                    let key = cmd.pipeline_key.shadow_render_pipeline_key();
                    let label = (!self.pipelines.contains_key(&key)).then(|| {
                        cmd.shadow_pipeline_name.as_deref().map_or_else(
                            || format!("siglus-{}", key.program.short_name()),
                            |name| format!("siglus-{name}#{}", key.program.short_name()),
                        )
                    });
                    (key, label)
                });
                (pipeline_key, pipeline_label, shadow)
            };

            if let Some(label) = pipeline_label {
                self.ensure_pipeline(pipeline_key, &label);
            }
            if let Some((shadow_key, Some(label))) = shadow {
                self.ensure_pipeline(shadow_key, &label);
            }
        }
    }

    fn ensure_pipeline(&mut self, key: RenderPipelineKey, label: &str) {
        if self.pipelines.contains_key(&key) {
            return;
        }
        let blend_state = if let Some(blend) = key.blend {
            Some(match blend {
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
        } else {
            None
        };

        let pipeline = self
            .device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                cache: None,
                label: Some(label),
                layout: Some(&self.pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &self.shader,
                    entry_point: Some(key.program.vertex_entry()),
                    buffers: &[Some(if key.program.uses_sprite2d_layout() {
                        VertexSprite2d::layout()
                    } else {
                        Vertex::layout()
                    })],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &self.shader,
                    entry_point: Some(key.program.fragment_entry()),
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
                    front_face: d3d_front_face(),
                    cull_mode: if key.cull_back {
                        Some(wgpu::Face::Back)
                    } else {
                        None
                    },
                    polygon_mode: wgpu::PolygonMode::Fill,
                    unclipped_depth: false,
                    conservative: false,
                },
                depth_stencil: key.depth.map(|depth| wgpu::DepthStencilState {
                    format: if matches!(
                        key.program,
                        EffectProgram::ShadowStatic | EffectProgram::ShadowSkinned
                    ) {
                        wgpu::TextureFormat::Depth16Unorm
                    } else {
                        wgpu::TextureFormat::Depth32Float
                    },
                    depth_write_enabled: Some(depth.depth_write),
                    depth_compare: Some(if depth.use_depth {
                        wgpu::CompareFunction::LessEqual
                    } else {
                        wgpu::CompareFunction::Always
                    }),
                    stencil: wgpu::StencilState::default(),
                    bias: wgpu::DepthBiasState::default(),
                }),
                multisample: wgpu::MultisampleState::default(),
                multiview_mask: None,
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
                let cmd = &self.plan.draws[idx];
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
            multiview_mask: None,
            label: Some("siglus-sprite-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                depth_slice: None,
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
            let cmd = &self.plan.draws[draw_idx];
            let effective_key = if force_special.is_some() {
                cmd.pipeline_key.shadow_render_pipeline_key()
            } else {
                cmd.pipeline_key.render_pipeline_key()
            };
            if let Some(pipeline) = self.pipelines.get(&effective_key) {
                rp.set_pipeline(pipeline);
            }
            if effective_key.program.uses_sprite2d_layout() {
                rp.set_vertex_buffer(0, self.vertex_sprite2d_buf.slice(..));
            } else {
                rp.set_vertex_buffer(0, self.vertex_buf.slice(..));
            }
            let bind_group = self.draw_gpu_slots[draw_idx]
                .bind_group
                .as_ref()
                .expect("draw gpu slot prepared before render pass");
            let dynamic_offset = draw_idx
                .checked_mul(self.vs_uniform_stride)
                .and_then(|offset| u32::try_from(offset).ok())
                .expect("draw uniform dynamic offset exceeds wgpu u32 range");
            rp.set_bind_group(0, bind_group.as_ref(), &[dynamic_offset]);
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
                .or_else(|| {
                    cmd.image_id
                        .as_ref()
                        .and_then(|id| self.textures.get(&id.key()))
                })
                .unwrap_or(&self.default_aux)
        } else {
            cmd.image_id
                .as_ref()
                .and_then(|id| self.textures.get(&id.key()))
                .unwrap_or(&self.default_aux)
        };
        let mask = cmd
            .mask_image_id
            .as_ref()
            .and_then(|id| self.textures.get(&id.key()))
            .unwrap_or(&self.default_aux);
        let tone = cmd
            .tonecurve_image_id
            .as_ref()
            .and_then(|id| self.textures.get(&id.key()))
            .unwrap_or(&self.default_aux);
        let fog = cmd
            .fog_image_id
            .as_ref()
            .and_then(|id| self.textures.get(&id.key()))
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
        } else if let Some(ref id) = cmd.wipe_src_image_id {
            if let Some(tex) = self.textures.get(&id.key()) {
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
        let key = sprite2d_copy_render_pipeline_key();
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
        self.queue
            .write_buffer(&self.vs_uniform_buf, 0, bytemuck::bytes_of(&vs_uniform));
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
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.vs_uniform_buf,
                        offset: 0,
                        size: NonZeroU64::new(std::mem::size_of::<VsUniform>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 13,
                    resource: self.zero_bone_uniform_buf.as_entire_binding(),
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
            multiview_mask: None,
            label: Some("siglus-copy-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                depth_slice: None,
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
        rp.set_vertex_buffer(0, self.vertex_sprite2d_buf.slice(..));
        rp.set_bind_group(0, &bind_group, &[0]);
        rp.set_scissor_rect(viewport.x, viewport.y, viewport.w, viewport.h);
        rp.draw(blit_range, 0..1);
        Ok(())
    }

    fn ensure_vertex_capacity(&mut self, needed: usize) -> Result<()> {
        if needed <= self.vertex_capacity {
            return Ok(());
        }

        // wgpu/Metal buffer creation is substantially heavier than extending the
        // old D3D9 SYSTEMMEM dynamic buffer. Keep semantic contents identical but
        // grow the high-water mark geometrically so particle systems do not
        // recreate a GPU buffer every time one more quad becomes visible.
        let mut new_cap = self.vertex_capacity.max(48);
        while new_cap < needed {
            new_cap = new_cap.saturating_mul(2);
        }
        new_cap = align_up_usize(new_cap, 6);
        self.vertex_capacity = new_cap;

        self.vertex_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("siglus-sprite-vertex-buf"),
            size: (new_cap * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.vertex_sprite2d_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("siglus-sprite2d-vertex-buf"),
            size: (new_cap * std::mem::size_of::<VertexSprite2dData>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Ok(())
    }

    fn upload_prepared_vertices(&mut self) -> Result<()> {
        self.ensure_vertex_capacity(self.plan.verts.len())?;

        // Tona3 keeps separate FVF-specific 2D/3D buffers. Do the same here:
        // ordinary Siglus 2D sprites upload only pos/uv/mask-uv/alpha instead of
        // the 384-byte all-purpose mesh vertex used by the 3D path.
        self.sprite2d_verts.clear();
        self.sprite2d_verts.extend(
            self.plan
                .verts
                .iter()
                .copied()
                .map(VertexSprite2dData::from),
        );
        if !self.sprite2d_verts.is_empty() {
            self.queue.write_buffer(
                &self.vertex_sprite2d_buf,
                0,
                bytemuck::cast_slice(&self.sprite2d_verts),
            );
        }

        let needs_mesh_vertices = self
            .plan
            .draws
            .iter()
            .any(|cmd| !cmd.pipeline_key.program.uses_sprite2d_layout() || cmd.shadow_cast);
        if needs_mesh_vertices && !self.plan.verts.is_empty() {
            self.queue
                .write_buffer(&self.vertex_buf, 0, bytemuck::cast_slice(&self.plan.verts));
        }
        Ok(())
    }

    fn ensure_vs_uniform_capacity(&mut self, needed: usize) {
        if needed <= self.vs_uniform_capacity {
            return;
        }
        let mut new_cap = self.vs_uniform_capacity.max(64);
        while new_cap < needed {
            new_cap = new_cap.saturating_mul(2);
        }
        self.vs_uniform_capacity = new_cap;
        self.vs_uniform_buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("siglus-vs-uniform-arena"),
            size: (self.vs_uniform_stride * new_cap) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // Bind groups reference the old arena object. They must be rebuilt after
        // a high-water resize, but not on ordinary frames.
        self.clear_draw_bindings();
    }

    fn upload_draw_uniforms(&mut self) {
        let draw_count = self.plan.draws.len();
        if draw_count == 0 {
            return;
        }
        self.ensure_vs_uniform_capacity(draw_count);
        let byte_len = self.vs_uniform_stride.saturating_mul(draw_count);
        self.vs_uniform_staging.clear();
        self.vs_uniform_staging.resize(byte_len, 0);
        for (idx, cmd) in self.plan.draws.iter().enumerate() {
            let offset = idx * self.vs_uniform_stride;
            let bytes = bytemuck::bytes_of(&cmd.vs_uniform);
            self.vs_uniform_staging[offset..offset + bytes.len()].copy_from_slice(bytes);
        }
        self.queue
            .write_buffer(&self.vs_uniform_buf, 0, &self.vs_uniform_staging);
    }

    fn ensure_draw_gpu_slots(&mut self, needed: usize) {
        if self.draw_gpu_slots.len() >= needed {
            return;
        }
        // Slots now own GPU state only for the uncommon skinned-mesh bone palette.
        // Growing the CPU-side slot vector is cheap and does not allocate GPU
        // buffers for ordinary 2D sprites such as the 256 benchmark papers.
        self.draw_gpu_slots.resize_with(needed, || DrawGpuSlot {
            bone_uniform_buf: None,
            bind_group: None,
            bind_key: None,
            bind_epoch: 0,
        });
    }

    fn prepare_draw_gpu_slot(
        &mut self,
        draw_idx: usize,
        overlay_backdrop: Option<BackdropTarget>,
    ) -> Result<()> {
        self.ensure_draw_gpu_slots(draw_idx + 1);

        let use_bone_uniform = draw_uses_bone_uniform(&self.plan.draws[draw_idx]);
        if use_bone_uniform {
            if self.draw_gpu_slots[draw_idx].bone_uniform_buf.is_none() {
                self.draw_gpu_slots[draw_idx].bone_uniform_buf =
                    Some(self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("siglus-bone-uniform-slot"),
                        size: std::mem::size_of::<BoneUniform>() as wgpu::BufferAddress,
                        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    }));
            }
            let bone_buf = self.draw_gpu_slots[draw_idx]
                .bone_uniform_buf
                .as_ref()
                .expect("skinned draw allocated a bone uniform buffer");
            let bone_uniform_index = self.plan.draws[draw_idx]
                .bone_uniform_index
                .expect("skinned draw missing bone palette index");
            let bone_uniform = self
                .plan
                .draw_bone_uniforms
                .get(bone_uniform_index as usize)
                .expect("skinned draw bone palette index out of range");
            self.queue
                .write_buffer(bone_buf, 0, bytemuck::bytes_of(bone_uniform));
        }

        let cmd = &self.plan.draws[draw_idx];
        let bind_key = DrawBindKey::from_command(cmd, overlay_backdrop);
        // Emote's offscreen target may be recreated in-place when the requested
        // render size changes while keeping the same render id. Its texture view
        // therefore cannot be safely retained in a cached bind group across
        // frames. Ordinary Siglus images and mesh textures use stable cache keys.
        let cacheable = cmd.emote_render_id.is_none();
        let slot = &self.draw_gpu_slots[draw_idx];
        let needs_bind_group = !cacheable
            || slot.bind_group.is_none()
            || slot.bind_epoch != self.draw_bind_epoch
            || slot.bind_key.as_ref() != Some(&bind_key);
        if !needs_bind_group {
            return Ok(());
        }

        // With the shared dynamic VsUniform arena and shared zero bone palette,
        // ordinary 2D draws no longer have any slot-specific buffer binding. This
        // lets identical resource sets reuse one bind group, matching tona3's
        // state batching instead of allocating one bind group per paper sprite.
        if cacheable
            && !use_bone_uniform
            && let Some(bind_group) = self.shared_draw_bind_groups.get(&bind_key).cloned()
        {
            let slot = &mut self.draw_gpu_slots[draw_idx];
            slot.bind_group = Some(bind_group);
            slot.bind_key = Some(bind_key);
            slot.bind_epoch = self.draw_bind_epoch;
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
        let bone_uniform_buf = if use_bone_uniform {
            self.draw_gpu_slots[draw_idx]
                .bone_uniform_buf
                .as_ref()
                .expect("skinned draw bone buffer missing")
        } else {
            &self.zero_bone_uniform_buf
        };
        let bind_group = Arc::new(self.device.create_bind_group(&wgpu::BindGroupDescriptor {
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
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &self.vs_uniform_buf,
                        offset: 0,
                        size: NonZeroU64::new(std::mem::size_of::<VsUniform>() as u64),
                    }),
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
        }));

        if cacheable && !use_bone_uniform {
            self.shared_draw_bind_groups
                .insert(bind_key.clone(), Arc::clone(&bind_group));
        }
        let slot = &mut self.draw_gpu_slots[draw_idx];
        slot.bind_group = Some(bind_group);
        slot.bind_key = cacheable.then_some(bind_key);
        slot.bind_epoch = self.draw_bind_epoch;
        Ok(())
    }

    /// Drop GPU textures that are keyed by runtime ImageKey.
    ///
    /// Scene restart reinitializes ImageManager and reuses ImageKey indices from 0.
    /// Keeping the old GPU cache would make a newly decoded image with the same
    /// ImageKey/version sample the previous scene's texture. External path based
    /// textures are intentionally kept because their keys are stable resource paths.
    pub fn clear_runtime_image_textures(&mut self) {
        self.plan.draws.clear();
        self.plan.draw_bone_uniforms.clear();
        self.plan.draw_bone_uniforms.shrink_to_fit();
        self.textures.clear();
        self.clear_draw_bindings();
    }

    fn clear_draw_bindings(&mut self) {
        for slot in &mut self.draw_gpu_slots {
            slot.bind_group = None;
            slot.bind_key = None;
        }
        self.shared_draw_bind_groups.clear();
        self.draw_bind_epoch = self.draw_bind_epoch.wrapping_add(1).max(1);
    }

    pub fn texture_cache_bytes(&self) -> u64 {
        self.textures
            .values()
            .map(|tex| u64::from(tex.width) * u64::from(tex.height) * 4 * 4 / 3)
            .sum()
    }

    /// Low-overhead renderer allocation accounting for the HUD.  This only
    /// inspects sizes already stored by the renderer; it never maps or copies a
    /// GPU resource.  Texture sizes include the same 4/3 mip estimate used by
    /// texture_cache_bytes().
    pub fn debug_memory_stats(&self) -> RendererMemoryStats {
        let image_texture_bytes = self.texture_cache_bytes();
        let external_texture_bytes = self
            .external_textures
            .values()
            .map(|tex| u64::from(tex.width) * u64::from(tex.height) * 4 * 4 / 3)
            .sum();
        let color_bytes = |rt: &RenderTargetTexture| u64::from(rt.width) * u64::from(rt.height) * 4;
        let internal_color_target_bytes = color_bytes(&self.scene_a)
            + color_bytes(&self.scene_b)
            + color_bytes(&self.wipe_a)
            + color_bytes(&self.wipe_b)
            + color_bytes(&self.shadow_map);
        // No HUD bookkeeping is stored in DepthTexture. Infer the three depth
        // allocations from dimensions the renderer already owns.
        let internal_depth_target_bytes =
            u64::from(self.scene_a.width) * u64::from(self.scene_a.height) * 4
                + u64::from(self.config.width) * u64::from(self.config.height) * 4
                + u64::from(self.shadow_map.width) * u64::from(self.shadow_map.height) * 2;
        let renderer_gpu_buffer_bytes = (self.vertex_capacity * std::mem::size_of::<Vertex>())
            as u64
            + (self.vertex_capacity * std::mem::size_of::<VertexSprite2dData>()) as u64
            + (self.vs_uniform_stride * self.vs_uniform_capacity) as u64
            + std::mem::size_of::<BoneUniform>() as u64
            + self
                .draw_gpu_slots
                .iter()
                .filter(|slot| slot.bone_uniform_buf.is_some())
                .count() as u64
                * std::mem::size_of::<BoneUniform>() as u64;
        let frame_arena_capacity_bytes = self.plan.verts.capacity() * std::mem::size_of::<Vertex>()
            + self.sprite2d_verts.capacity() * std::mem::size_of::<VertexSprite2dData>()
            + self.plan.draws.capacity() * std::mem::size_of::<DrawCommand>()
            + self.plan.draw_bone_uniforms.capacity() * std::mem::size_of::<BoneUniform>()
            + self.vs_uniform_staging.capacity();
        RendererMemoryStats {
            image_texture_bytes,
            external_texture_bytes,
            internal_color_target_bytes,
            internal_depth_target_bytes,
            renderer_gpu_buffer_bytes,
            frame_arena_capacity_bytes,
            image_texture_count: self.textures.len(),
            external_texture_count: self.external_textures.len(),
            cached_render_pipeline_count: self.pipelines.len(),
        }
    }

    fn organize_textures(&mut self, images: &ImageManager) {
        let before = self.textures.len();
        self.textures.retain(|id, _| images.contains(*id));
        if self.textures.len() != before {
            // Bind groups also own texture views. Drop those references, not
            // just their cache keys, when the runtime releases a resource.
            self.clear_draw_bindings();
        }
    }

    fn ensure_texture_uploaded(&mut self, images: &ImageManager, key: ImageKey) -> Result<()> {
        let Some(id) = images.image_handle(key) else {
            return Ok(());
        };
        let Some((img, version)) = images.get_entry(&id) else {
            return Ok(());
        };
        if let Some(mut tex) = self.textures.remove(&id.key()) {
            if tex.version != version {
                if tex.width == img.width && tex.height == img.height {
                    self.update_texture(&tex, &img)?;
                    tex.version = version;
                } else {
                    tex = create_gpu_texture(
                        &self.device,
                        &self.queue,
                        &self.mipmap_generator,
                        &format!("siglus-texture-{}", id.index()),
                        &img,
                        version,
                    )?;
                    self.clear_draw_bindings();
                }
            }
            self.textures.insert(id.key(), tex);
        } else {
            let tex = create_gpu_texture(
                &self.device,
                &self.queue,
                &self.mipmap_generator,
                &format!("siglus-texture-{}", id.index()),
                &img,
                version,
            )?;
            self.textures.insert(id.key(), tex);
        }
        Ok(())
    }

    fn update_texture(&self, tex: &GpuTexture, img: &crate::assets::RgbaImage) -> Result<()> {
        if tex.width != img.width || tex.height != img.height {
            return Ok(());
        }
        upload_texture_pixels(&self.queue, &tex._tex, img);
        if let Some(mipmaps) = self.mipmap_generator.generate(&self.device, &tex._tex) {
            // D3D9 AUTOGENMIPMAP makes regenerated levels available after a
            // level-0 update. Submit this texture's chain immediately so the
            // Metal backend cannot accumulate native command buffers for every
            // texture prepared in the frame before any work reaches the queue.
            self.queue.submit(Some(mipmaps));
        }
        Ok(())
    }

    fn submit(&self, encoder: wgpu::CommandEncoder) {
        self.queue.submit(Some(encoder.finish()));
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
    mipmap_generator: &mipmap::MipmapGenerator,
    rgba: [u8; 4],
) -> Result<GpuTexture> {
    let img = crate::assets::RgbaImage {
        width: 1,
        height: 1,
        center_x: 0,
        center_y: 0,
        rgba: rgba.to_vec(),
    };
    create_gpu_texture(
        device,
        queue,
        mipmap_generator,
        "siglus-default-aux",
        &img,
        0,
    )
}

#[cfg(test)]
#[derive(Debug)]
struct Rgba8MipLevel {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}

/// Build the same kind of full mip chain requested by the original
/// D3DUSAGE_AUTOGENMIPMAP textures.  Values are averaged in the stored 8-bit
/// color space rather than converted through sRGB, matching the D3D9 setup.
#[cfg(test)]
fn build_rgba8_mip_chain(width: u32, height: u32, rgba: &[u8]) -> Vec<Rgba8MipLevel> {
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
        let mut next = vec![0u8; next_width as usize * next_height as usize * 4];

        for y in 0..next_height {
            for x in 0..next_width {
                let src_x0 = x.saturating_mul(2);
                let src_y0 = y.saturating_mul(2);
                let src_x1 = (src_x0 + 1).min(prev.width - 1);
                let src_y1 = (src_y0 + 1).min(prev.height - 1);
                let coords = [
                    (src_x0, src_y0),
                    (src_x1, src_y0),
                    (src_x0, src_y1),
                    (src_x1, src_y1),
                ];
                let dst = ((y * next_width + x) * 4) as usize;
                for channel in 0..4usize {
                    let sum = coords.iter().fold(0u32, |acc, (sx, sy)| {
                        let src = ((*sy * prev.width + *sx) * 4) as usize + channel;
                        acc + prev.rgba[src] as u32
                    });
                    next[dst + channel] = ((sum + 2) / 4).min(255) as u8;
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

fn create_gpu_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    mipmap_generator: &mipmap::MipmapGenerator,
    label: &str,
    img: &crate::assets::RgbaImage,
    version: u64,
) -> Result<GpuTexture> {
    anyhow::ensure!(img.width > 0 && img.height > 0, "empty texture dimensions");
    let pixel_bytes = (img.width as usize)
        .checked_mul(img.height as usize)
        .and_then(|n| n.checked_mul(4))
        .context("texture size overflow")?;
    anyhow::ensure!(img.rgba.len() >= pixel_bytes, "truncated texture pixels");
    let mip_level_count = u32::BITS - img.width.max(img.height).leading_zeros();
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
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });

    upload_texture_pixels(queue, &tex, img);
    if let Some(mipmaps) = mipmap_generator.generate(device, &tex) {
        queue.submit(Some(mipmaps));
    }

    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("siglus-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
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

fn upload_texture_pixels(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    img: &crate::assets::RgbaImage,
) {
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &img.rgba,
        wgpu::TexelCopyBufferLayout {
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
        mipmap_filter: wgpu::MipmapFilterMode::Nearest,
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

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn wasm_shader_source() -> String {
    SHADER.to_string()
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
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        cache: None,
        label: Some("siglus-wipe-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
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
        multiview_mask: None,
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
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        cache: None,
        label: Some("siglus-page-wipe-pipeline"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: wgpu::PipelineCompilationOptions::default(),
            buffers: &[Some(wgpu::VertexBufferLayout {
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
            })],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
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
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::LessEqual),
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview_mask: None,
    });
    (layout, pipeline)
}

const PAGE_WIPE_SHADER: &str = include_str!("shaders/page_wipe.wgsl");

#[cfg(test)]
mod present_mode_tests {
    use super::present_mode_for_wait_display_vsync;

    #[test]
    fn vsync_wait_uses_fifo() {
        let modes = [wgpu::PresentMode::Fifo, wgpu::PresentMode::Immediate];
        assert_eq!(
            present_mode_for_wait_display_vsync(true, &modes),
            wgpu::PresentMode::Fifo
        );
    }

    #[test]
    fn vsync_off_prefers_immediate_then_mailbox_then_fifo() {
        let all = [
            wgpu::PresentMode::Fifo,
            wgpu::PresentMode::Mailbox,
            wgpu::PresentMode::Immediate,
        ];
        assert_eq!(
            present_mode_for_wait_display_vsync(false, &all),
            wgpu::PresentMode::Immediate
        );
        assert_eq!(
            present_mode_for_wait_display_vsync(
                false,
                &[wgpu::PresentMode::Fifo, wgpu::PresentMode::Mailbox],
            ),
            wgpu::PresentMode::Mailbox
        );
        assert_eq!(
            present_mode_for_wait_display_vsync(false, &[wgpu::PresentMode::Fifo]),
            wgpu::PresentMode::Fifo
        );
    }
}

#[cfg(test)]
mod depth_state_tests {
    use super::{d3d_front_face, depth_write_enabled};

    #[test]
    fn translucent_3d_sprites_test_depth_without_writing_it() {
        assert!(depth_write_enabled(true, false));
        assert!(!depth_write_enabled(true, true));
        assert!(!depth_write_enabled(false, false));
    }

    #[test]
    fn tona3_ccw_cull_uses_clockwise_front_faces() {
        assert!(matches!(d3d_front_face(), wgpu::FrontFace::Cw));
    }
}

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

const WIPE_SHADER: &str = include_str!("shaders/wipe.wgsl");

const SHADER: &str = include_str!("shaders/sprite.wgsl");
