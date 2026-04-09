//! Minimal wgpu renderer for the BG stage.
//!
//! This renderer consumes a painter-ordered list of sprites and draws them
//! in order. It is intentionally simple (one draw call per sprite).

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use winit::window::Window;

use crate::image_manager::{ImageId, ImageManager};
use crate::layer::{ClipRect, RenderSprite, SpriteBlend, SpriteFit, SpriteSizeMode};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    alpha: f32,
    effects1: [f32; 4],
    effects2: [f32; 4],
    effects3: [f32; 4],
    effects4: [f32; 4],
}

impl Vertex {
    const ATTRS: [wgpu::VertexAttribute; 7] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32,
        3 => Float32x4,
        4 => Float32x4,
        5 => Float32x4,
        6 => Float32x4
    ];

    fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

pub struct Renderer {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,

    pipelines: HashMap<SpriteBlend, wgpu::RenderPipeline>,
    bind_group_layout: wgpu::BindGroupLayout,

    vertex_buf: wgpu::Buffer,
    vertex_capacity: usize,

    textures: HashMap<ImageId, GpuTexture>,

    verts: Vec<Vertex>,
    draws: Vec<DrawCommand>,
}

struct GpuTexture {
    _tex: wgpu::Texture,
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
    width: u32,
    height: u32,
    version: u64,
}

#[derive(Debug, Clone)]
struct DrawCommand {
    image_id: ImageId,
    range: std::ops::Range<u32>,
    scissor: Option<ScissorRect>,
    blend: SpriteBlend,
}

#[derive(Debug, Copy, Clone)]
struct ScissorRect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
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

        let mut pipelines = HashMap::new();
        for blend in [
            SpriteBlend::Normal,
            SpriteBlend::Add,
            SpriteBlend::Sub,
            SpriteBlend::Mul,
            SpriteBlend::Screen,
        ] {
            let blend_state = match blend {
                SpriteBlend::Normal => wgpu::BlendState::ALPHA_BLENDING,
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
            };

            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("siglus-sprite-pipeline"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: "vs_main",
                    buffers: &[Vertex::layout()],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: "fs_main",
                    targets: &[Some(wgpu::ColorTargetState {
                        format: config.format,
                        blend: Some(blend_state),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
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
            pipelines.insert(blend, pipeline);
        }

        let vertex_capacity = 6;
        let vertex_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("siglus-sprite-vertex-buf"),
            size: (vertex_capacity * std::mem::size_of::<Vertex>()) as wgpu::BufferAddress,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipelines,
            bind_group_layout,
            vertex_buf,
            vertex_capacity,
            textures: HashMap::new(),
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

        // Build a packed vertex stream + per-sprite vertex offsets.
        self.verts.clear();
        self.draws.clear();

        let win_w = self.config.width as f32;
        let win_h = self.config.height as f32;

        for s in sprites {
            let sprite = &s.sprite;
            let Some(img_id) = sprite.image_id else {
                continue;
            };
            let Some(img) = images.get(img_id) else {
                continue;
            };

            let (src_left, src_top, src_right, src_bottom) =
                src_clip_rect(sprite.src_clip, img.width, img.height)?;
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
            let effects4 = [sprite.mask_mode as f32, 0.0, 0.0, 0.0];
            let (u0, v0, u1, v1) = (
                (src_left / img.width as f32).clamp(0.0, 1.0),
                (src_top / img.height as f32).clamp(0.0, 1.0),
                (src_right / img.width as f32).clamp(0.0, 1.0),
                (src_bottom / img.height as f32).clamp(0.0, 1.0),
            );

            let p0 = transform_point(0.0, 0.0, dst_x, dst_y, dst_w, dst_h, sprite);
            let p1 = transform_point(dst_w, 0.0, dst_x, dst_y, dst_w, dst_h, sprite);
            let p2 = transform_point(dst_w, dst_h, dst_x, dst_y, dst_w, dst_h, sprite);
            let p3 = transform_point(0.0, dst_h, dst_x, dst_y, dst_w, dst_h, sprite);

            let base = self.verts.len() as u32;
            let (x0, y0) = pixel_to_ndc(p0.0, p0.1, win_w, win_h);
            let (x1, y1) = pixel_to_ndc(p1.0, p1.1, win_w, win_h);
            let (x2, y2) = pixel_to_ndc(p2.0, p2.1, win_w, win_h);
            let (x3, y3) = pixel_to_ndc(p3.0, p3.1, win_w, win_h);

            self.verts.extend_from_slice(&[
                Vertex {
                    pos: [x0, y0],
                    uv: [u0, v0],
                    alpha,
                    effects1,
                    effects2,
                    effects3,
                    effects4,
                },
                Vertex {
                    pos: [x1, y1],
                    uv: [u1, v0],
                    alpha,
                    effects1,
                    effects2,
                    effects3,
                    effects4,
                },
                Vertex {
                    pos: [x2, y2],
                    uv: [u1, v1],
                    alpha,
                    effects1,
                    effects2,
                    effects3,
                    effects4,
                },
                Vertex {
                    pos: [x0, y0],
                    uv: [u0, v0],
                    alpha,
                    effects1,
                    effects2,
                    effects3,
                    effects4,
                },
                Vertex {
                    pos: [x2, y2],
                    uv: [u1, v1],
                    alpha,
                    effects1,
                    effects2,
                    effects3,
                    effects4,
                },
                Vertex {
                    pos: [x3, y3],
                    uv: [u0, v1],
                    alpha,
                    effects1,
                    effects2,
                    effects3,
                    effects4,
                },
            ]);
            self.draws.push(DrawCommand {
                image_id: img_id,
                range: base..base + 6,
                scissor,
                blend: sprite.blend,
            });
        }

        if self.verts.is_empty() {
            // Clear to black.
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("siglus-clear-encoder"),
                });
            {
                let _rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("siglus-clear-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
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
            }
            self.queue.submit(Some(encoder.finish()));
            frame.present();
            return Ok(());
        }

        self.ensure_vertex_capacity(self.verts.len())?;
        self.queue
            .write_buffer(&self.vertex_buf, 0, bytemuck::cast_slice(&self.verts));

        // Upload/update textures needed this frame.
        for cmd in &self.draws {
            let img_id = cmd.image_id;
            let Some((img, version)) = images.get_entry(img_id) else {
                continue;
            };
            if let Some(mut tex) = self.textures.remove(&img_id) {
                if tex.version != version {
                    if tex.width == img.width && tex.height == img.height {
                        self.update_texture(&mut tex, img)?;
                        tex.version = version;
                    } else {
                        tex = self.upload_texture(&img_id, img, version)?;
                    }
                }
                self.textures.insert(img_id, tex);
            } else {
                let tex = self.upload_texture(&img_id, img, version)?;
                self.textures.insert(img_id, tex);
            }
        }

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("siglus-sprite-encoder"),
            });

        {
            let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("siglus-sprite-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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

            let Some(pipeline) = self.pipelines.get(&SpriteBlend::Normal) else {
                return Ok(());
            };
            rp.set_pipeline(pipeline);
            rp.set_vertex_buffer(0, self.vertex_buf.slice(..));

            for cmd in &self.draws {
                let Some(tex) = self.textures.get(&cmd.image_id) else {
                    continue;
                };
                if let Some(pipeline) = self.pipelines.get(&cmd.blend) {
                    rp.set_pipeline(pipeline);
                }
                rp.set_bind_group(0, &tex.bind_group, &[]);
                if let Some(sci) = cmd.scissor {
                    rp.set_scissor_rect(sci.x, sci.y, sci.w, sci.h);
                } else {
                    rp.set_scissor_rect(0, 0, self.config.width, self.config.height);
                }
                rp.draw(cmd.range.clone(), 0..1);
            }
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }

    fn ensure_vertex_capacity(&mut self, needed: usize) -> Result<()> {
        if needed <= self.vertex_capacity {
            return Ok(());
        }
        // Grow to next multiple of 6.
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

    fn upload_texture(
        &self,
        id: &ImageId,
        img: &crate::assets::RgbaImage,
        version: u64,
    ) -> Result<GpuTexture> {
        let tex = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("siglus-texture-{}", id.index())),
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

        self.queue.write_texture(
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
        let sampler = self.device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("siglus-sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("siglus-sprite-bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Ok(GpuTexture {
            _tex: tex,
            view,
            sampler,
            bind_group,
            width: img.width,
            height: img.height,
            version,
        })
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

fn pixel_to_ndc(x: f32, y: f32, win_w: f32, win_h: f32) -> (f32, f32) {
    // Pixel coords: origin top-left. NDC: origin center, y up.
    let nx = (x / win_w) * 2.0 - 1.0;
    let ny = 1.0 - (y / win_h) * 2.0;
    (nx, ny)
}

fn transform_point(
    px: f32,
    py: f32,
    dst_x: f32,
    dst_y: f32,
    _dst_w: f32,
    _dst_h: f32,
    sprite: &crate::layer::Sprite,
) -> (f32, f32) {
    let pivot_x = sprite.pivot_x;
    let pivot_y = sprite.pivot_y;
    let lx = px - pivot_x;
    let ly = py - pivot_y;
    let sx = lx * sprite.scale_x;
    let sy = ly * sprite.scale_y;
    let (sin_r, cos_r) = sprite.rotate.sin_cos();
    let rx = sx * cos_r - sy * sin_r;
    let ry = sx * sin_r + sy * cos_r;
    (dst_x + pivot_x + rx, dst_y + pivot_y + ry)
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
  @location(0) pos: vec2<f32>,
  @location(1) uv: vec2<f32>,
  @location(2) alpha: f32,
  @location(3) effects1: vec4<f32>,
  @location(4) effects2: vec4<f32>,
  @location(5) effects3: vec4<f32>,
  @location(6) effects4: vec4<f32>,
};

struct VsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) alpha: f32,
  @location(2) effects1: vec4<f32>,
  @location(3) effects2: vec4<f32>,
  @location(4) effects3: vec4<f32>,
  @location(5) effects4: vec4<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
  var o: VsOut;
  o.pos = vec4<f32>(v.pos, 0.0, 1.0);
  o.uv = v.uv;
  o.alpha = v.alpha;
  o.effects1 = v.effects1;
  o.effects2 = v.effects2;
  o.effects3 = v.effects3;
  o.effects4 = v.effects4;
  return o;
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
  var c = textureSample(tex, smp, i.uv);
  let tr = i.effects1.x;
  let mono = i.effects1.y;
  let rev = i.effects1.z;
  let bright = i.effects1.w;
  let dark = i.effects2.x;
  let color_rate = i.effects2.y;
  let color_add = vec3<f32>(i.effects2.z, i.effects2.w, i.effects3.x);
  let color_tgt = vec3<f32>(i.effects3.y, i.effects3.z, i.effects3.w);
  let mask_mode = i.effects4.x;

  var rgb = c.rgb;
  rgb = mix(rgb, vec3<f32>(1.0) - rgb, rev);
  let gray = dot(rgb, vec3<f32>(0.299, 0.587, 0.114));
  rgb = mix(rgb, vec3<f32>(gray), mono);
  rgb = rgb + vec3<f32>(bright);
  rgb = rgb * (1.0 - dark);
  rgb = mix(rgb, color_tgt, color_rate);
  rgb = clamp(rgb + color_add, vec3<f32>(0.0), vec3<f32>(1.0));

  var alpha = c.a;
  if (mask_mode > 0.5) {
    if (mask_mode < 1.5) {
      alpha = gray;
    }
  }

  let a = alpha * i.alpha * tr;
  return vec4<f32>(rgb * a, a);
}
"#;
