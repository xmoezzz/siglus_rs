//! Minimal wgpu renderer for the BG stage.
//!
//! This renderer consumes a painter-ordered list of sprites and draws them
//! in order. It is intentionally simple (one draw call per sprite).

use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use std::collections::HashMap;
use wgpu::util::DeviceExt;
use winit::window::Window;

use crate::image_manager::{ImageId, ImageManager};
use crate::layer::{RenderSprite, SpriteFit, SpriteSizeMode};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    alpha: f32,
}

impl Vertex {
    const ATTRS: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32];

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

    pipeline: wgpu::RenderPipeline,
    bind_group_layout: wgpu::BindGroupLayout,

    vertex_buf: wgpu::Buffer,
    vertex_capacity: usize,

    textures: HashMap<ImageId, GpuTexture>,
}

struct GpuTexture {
    _tex: wgpu::Texture,
    view: wgpu::TextureView,
    sampler: wgpu::Sampler,
    bind_group: wgpu::BindGroup,
}

impl Renderer {
    pub async fn new(window: &'static Window) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance
            .create_surface(window)
            .context("create_surface")?;

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

        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("siglus-sprite-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[Vertex::layout()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format: config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
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
            pipeline,
            bind_group_layout,
            vertex_buf,
            vertex_capacity,
            textures: HashMap::new(),
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

    pub fn render_sprites(&mut self, images: &ImageManager, sprites: &[RenderSprite]) -> Result<()> {
        let frame = self
            .surface
            .get_current_texture()
            .context("get_current_texture")?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        // Build a packed vertex stream + per-sprite vertex offsets.
        let mut verts: Vec<Vertex> = Vec::new();
        let mut draws: Vec<(ImageId, std::ops::Range<u32>)> = Vec::new();

        let win_w = self.config.width as f32;
        let win_h = self.config.height as f32;

        for s in sprites {
            let Some(img) = images.get(s.image) else {
                continue;
            };

            let (dst_x, dst_y, dst_w, dst_h) = match s.fit {
                SpriteFit::FullScreen => (0.0f32, 0.0f32, win_w, win_h),
                SpriteFit::PixelRect => {
                    let (w, h) = match s.size_mode {
                        SpriteSizeMode::Intrinsic => (img.width as f32, img.height as f32),
                        SpriteSizeMode::Explicit { width, height } => (width as f32, height as f32),
                    };
                    (s.x as f32, s.y as f32, w, h)
                }
            };

            let alpha = (s.alpha as f32) / 255.0;
            let (x0, y0, x1, y1) = pixel_rect_to_ndc(dst_x, dst_y, dst_w, dst_h, win_w, win_h);

            let base = verts.len() as u32;
            verts.extend_from_slice(&[
                Vertex { pos: [x0, y1], uv: [0.0, 1.0], alpha },
                Vertex { pos: [x1, y1], uv: [1.0, 1.0], alpha },
                Vertex { pos: [x1, y0], uv: [1.0, 0.0], alpha },
                Vertex { pos: [x0, y1], uv: [0.0, 1.0], alpha },
                Vertex { pos: [x1, y0], uv: [1.0, 0.0], alpha },
                Vertex { pos: [x0, y0], uv: [0.0, 0.0], alpha },
            ]);
            draws.push((s.image, base..base + 6));
        }

        if verts.is_empty() {
            // Clear to black.
            let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
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

        self.ensure_vertex_capacity(verts.len())?;
        self.queue
            .write_buffer(&self.vertex_buf, 0, bytemuck::cast_slice(&verts));

        // Upload textures needed this frame.
        for (img_id, _) in &draws {
            if self.textures.contains_key(img_id) {
                continue;
            }
            let Some(img) = images.get(*img_id) else {
                continue;
            };
            let tex = self.upload_texture(img_id, img)?;
            self.textures.insert(*img_id, tex);
        }

        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
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

            rp.set_pipeline(&self.pipeline);
            rp.set_vertex_buffer(0, self.vertex_buf.slice(..));

            for (img_id, vr) in &draws {
                let Some(tex) = self.textures.get(img_id) else {
                    continue;
                };
                rp.set_bind_group(0, &tex.bind_group, &[]);
                rp.draw(vr.clone(), 0..1);
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

    fn upload_texture(&self, id: &ImageId, img: &crate::assets::RgbaImage) -> Result<GpuTexture> {
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
        })
    }
}

fn pixel_rect_to_ndc(x: f32, y: f32, w: f32, h: f32, win_w: f32, win_h: f32) -> (f32, f32, f32, f32) {
    // Pixel coords: origin top-left. NDC: origin center, y up.
    let x0 = (x / win_w) * 2.0 - 1.0;
    let x1 = ((x + w) / win_w) * 2.0 - 1.0;
    let y0 = 1.0 - (y / win_h) * 2.0;
    let y1 = 1.0 - ((y + h) / win_h) * 2.0;
    (x0, y0, x1, y1)
}

const SHADER: &str = r#"
struct VsIn {
  @location(0) pos: vec2<f32>,
  @location(1) uv: vec2<f32>,
  @location(2) alpha: f32,
};

struct VsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) alpha: f32,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
  var o: VsOut;
  o.pos = vec4<f32>(v.pos, 0.0, 1.0);
  o.uv = v.uv;
  o.alpha = v.alpha;
  return o;
}

@group(0) @binding(0) var tex: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
  let c = textureSample(tex, smp, i.uv);
  return vec4<f32>(c.rgb * i.alpha, c.a * i.alpha);
}
"#;
