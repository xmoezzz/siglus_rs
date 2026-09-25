use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use anyhow::{Result, anyhow};
use bytemuck::{Pod, Zeroable};
use eluna::{EmoteDrawFrameInfo, EmoteDrawPass, EmoteStaticScene, EmoteStaticSprite};
use wgpu::util::DeviceExt;

use crate::emote::EmoteRenderPacket;
use crate::render_plan::emote::{EmoteTexture, EmoteVertex, native_blend_index, plan_emote_draws};

use super::GpuTexture;

const TARGET_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const STENCIL_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;

const SHADER: &str = include_str!("shaders/emote.wgsl");

impl EmoteVertex {
    const ATTRS: [wgpu::VertexAttribute; 7] = [
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 0,
            shader_location: 0,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 8,
            shader_location: 1,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x2,
            offset: 16,
            shader_location: 2,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 24,
            shader_location: 3,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32,
            offset: 40,
            shader_location: 4,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x4,
            offset: 44,
            shader_location: 5,
        },
        wgpu::VertexAttribute {
            format: wgpu::VertexFormat::Float32x3,
            offset: 60,
            shader_location: 6,
        },
    ];

    fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

#[derive(Debug)]
struct Target {
    output: GpuTexture,
    feedback: GpuTexture,
    feedback_valid: bool,
    alpha_readback_version: Option<u64>,
    stencil_texture: wgpu::Texture,
    stencil_view: wgpu::TextureView,
    textures: HashMap<u32, GpuTexture>,
    texture_bind_groups: HashMap<u32, wgpu::BindGroup>,
    feedback_bind_group: wgpu::BindGroup,
    version: u64,
}

#[derive(Debug)]
struct Draw {
    texture: EmoteTexture,
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,
    blend_index: usize,
    stencil_groups: Vec<StencilGroup>,
    stencil_initial_reference: u32,
    stencil_final_reference: u32,
}

#[derive(Debug)]
struct StencilSource {
    texture: EmoteTexture,
    vertex_buffer: wgpu::Buffer,
    vertex_count: u32,
}

#[derive(Debug)]
struct StencilGroup {
    phase: u32,
    sources: Vec<StencilSource>,
}

#[derive(Debug)]
pub(super) struct EmoteCompositor {
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline_layout: wgpu::PipelineLayout,
    shader: wgpu::ShaderModule,
    color_pipelines: Vec<wgpu::RenderPipeline>,
    stencil_color_pipelines: Vec<wgpu::RenderPipeline>,
    stencil_inner_pipeline: wgpu::RenderPipeline,
    stencil_outer_pipeline: wgpu::RenderPipeline,
    targets: HashMap<u64, Target>,
}

impl EmoteCompositor {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("siglus-emote-texture-bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
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
            label: Some("siglus-emote-shader"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("siglus-emote-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let color_pipelines = (0..6)
            .map(|mode| create_color_pipeline(device, &pipeline_layout, &shader, mode, false))
            .collect();
        let stencil_color_pipelines = (0..6)
            .map(|mode| create_color_pipeline(device, &pipeline_layout, &shader, mode, true))
            .collect();
        let stencil_inner_pipeline = create_mask_pipeline(
            device,
            &pipeline_layout,
            &shader,
            wgpu::StencilOperation::IncrementWrap,
            "siglus-emote-inner-mask",
        );
        let stencil_outer_pipeline = create_mask_pipeline(
            device,
            &pipeline_layout,
            &shader,
            wgpu::StencilOperation::DecrementWrap,
            "siglus-emote-outer-mask",
        );
        Self {
            bind_group_layout,
            pipeline_layout,
            shader,
            color_pipelines,
            stencil_color_pipelines,
            stencil_inner_pipeline,
            stencil_outer_pipeline,
            targets: HashMap::new(),
        }
    }

    pub(super) fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        packet: &EmoteRenderPacket,
    ) -> Result<()> {
        let recreate = self.targets.get(&packet.render_id).is_none_or(|target| {
            target.output.width != packet.width || target.output.height != packet.height
        });
        if recreate {
            let target = create_target(device, queue, &self.bind_group_layout, packet)?;
            self.targets.insert(packet.render_id, target);
        }
        // Drive map_async callbacks without blocking. This is also valid on
        // wasm32, where Maintain::Wait is not available as a synchronous path.
        device.poll(wgpu::Maintain::Poll);
        if self
            .targets
            .get(&packet.render_id)
            .is_some_and(|target| target.version == packet.version)
        {
            if packet.alpha_readback && !packet.has_current_hit_surface() {
                let target = self
                    .targets
                    .get_mut(&packet.render_id)
                    .ok_or_else(|| anyhow!("Emote compositor target disappeared"))?;
                if target.alpha_readback_version != Some(packet.version) {
                    schedule_alpha_readback(device, queue, target, packet);
                    target.alpha_readback_version = Some(packet.version);
                }
            }
            return Ok(());
        }

        let draws = build_draws(device, packet)?;
        let target = self
            .targets
            .get(&packet.render_id)
            .ok_or_else(|| anyhow!("Emote compositor target disappeared"))?;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("siglus-emote-object-encoder"),
        });
        {
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("siglus-emote-object-clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.output.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.0,
                            g: 0.0,
                            b: 0.0,
                            a: 0.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }

        for draw in &draws {
            let Some(bind_group) = resolve_bind_group(target, draw.texture) else {
                continue;
            };
            if draw.stencil_groups.is_empty() {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("siglus-emote-color"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &target.output.view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                pass.set_pipeline(&self.color_pipelines[draw.blend_index]);
                pass.set_bind_group(0, bind_group, &[]);
                pass.set_vertex_buffer(0, draw.vertex_buffer.slice(..));
                pass.draw(0..draw.vertex_count, 0..1);
                continue;
            }

            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("siglus-emote-stencil-color"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &target.output.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.stencil_view,
                    depth_ops: None,
                    stencil_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(draw.stencil_initial_reference),
                        store: wgpu::StoreOp::Store,
                    }),
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            let mut reference = draw.stencil_initial_reference;
            for group in draw.stencil_groups.iter().filter(|group| group.phase == 1) {
                pass.set_pipeline(&self.stencil_inner_pipeline);
                pass.set_stencil_reference(reference);
                for source in &group.sources {
                    let Some(mask_bind) = resolve_bind_group(target, source.texture) else {
                        continue;
                    };
                    pass.set_bind_group(0, mask_bind, &[]);
                    pass.set_vertex_buffer(0, source.vertex_buffer.slice(..));
                    pass.draw(0..source.vertex_count, 0..1);
                }
                reference = reference.saturating_add(1).min(255);
            }
            let final_reference = draw.stencil_final_reference;
            if draw.stencil_groups.iter().any(|group| group.phase == 2) {
                pass.set_pipeline(&self.stencil_outer_pipeline);
                pass.set_stencil_reference(final_reference);
                for group in draw.stencil_groups.iter().filter(|group| group.phase == 2) {
                    for source in &group.sources {
                        let Some(mask_bind) = resolve_bind_group(target, source.texture) else {
                            continue;
                        };
                        pass.set_bind_group(0, mask_bind, &[]);
                        pass.set_vertex_buffer(0, source.vertex_buffer.slice(..));
                        pass.draw(0..source.vertex_count, 0..1);
                    }
                }
            }
            pass.set_pipeline(&self.stencil_color_pipelines[draw.blend_index]);
            pass.set_stencil_reference(final_reference);
            pass.set_bind_group(0, bind_group, &[]);
            pass.set_vertex_buffer(0, draw.vertex_buffer.slice(..));
            pass.draw(0..draw.vertex_count, 0..1);
        }

        encoder.copy_texture_to_texture(
            wgpu::ImageCopyTexture {
                texture: &target.output._tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyTexture {
                texture: &target.feedback._tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: packet.width,
                height: packet.height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let target = self.targets.get_mut(&packet.render_id).unwrap();
        target.feedback_valid = true;
        target.version = packet.version;
        if packet.alpha_readback && target.alpha_readback_version != Some(packet.version) {
            schedule_alpha_readback(device, queue, target, packet);
            target.alpha_readback_version = Some(packet.version);
        }
        Ok(())
    }

    pub(super) fn texture(&self, render_id: u64) -> Option<&GpuTexture> {
        self.targets.get(&render_id).map(|target| &target.output)
    }

    pub(super) fn retain_render_ids(&mut self, live: &HashSet<u64>) {
        self.targets.retain(|render_id, _| live.contains(render_id));
    }
}

fn schedule_alpha_readback(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &Target,
    packet: &EmoteRenderPacket,
) {
    let width = packet.width.max(1);
    let height = packet.height.max(1);
    let unpadded_bytes_per_row = width.saturating_mul(4);
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
    let buffer = Arc::new(device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("siglus-emote-alpha-readback"),
        size: padded_bytes_per_row as u64 * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    }));
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("siglus-emote-alpha-readback-encoder"),
    });
    encoder.copy_texture_to_buffer(
        wgpu::ImageCopyTexture {
            texture: &target.output._tex,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyBuffer {
            buffer: buffer.as_ref(),
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
    queue.submit(Some(encoder.finish()));

    let callback_buffer = buffer.clone();
    let callback_packet = packet.clone();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            if let Err(err) = result {
                log::error!("Emote alpha readback failed: {err}");
                return;
            }
            let width = callback_packet.width as usize;
            let height = callback_packet.height as usize;
            let row_bytes = width.saturating_mul(4);
            let data = callback_buffer.slice(..).get_mapped_range();
            let mut alpha = vec![0u8; width.saturating_mul(height)];
            for y in 0..height {
                let src_offset = y.saturating_mul(padded_bytes_per_row as usize);
                let src_end = src_offset.saturating_add(row_bytes);
                if src_end > data.len() {
                    break;
                }
                for x in 0..width {
                    alpha[y * width + x] = data[src_offset + x * 4 + 3];
                }
            }
            drop(data);
            callback_buffer.unmap();
            callback_packet.publish_hit_alpha(alpha);
        });
}

fn create_target(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    layout: &wgpu::BindGroupLayout,
    packet: &EmoteRenderPacket,
) -> Result<Target> {
    let output = create_gpu_texture(
        device,
        "siglus-emote-object-output",
        packet.width,
        packet.height,
        TARGET_FORMAT,
        wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC,
        0,
    );
    let feedback = create_gpu_texture(
        device,
        "siglus-emote-feedback",
        packet.width,
        packet.height,
        TARGET_FORMAT,
        wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        0,
    );

    // Filter the moving mesh's RGBA texels before compositing. Point sampling
    // makes thin hair and alpha edges jump between texels as the mesh deforms;
    // filtering the completed object texture later cannot remove that shimmer.
    // This deliberately improves on eng_emote.cpp's POINT initialization.
    let internal_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("siglus-emote-internal-linear-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    let feedback_bind_group =
        create_texture_bind_group(device, layout, &feedback, &internal_sampler);

    let stencil_texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("siglus-emote-stencil"),
        size: wgpu::Extent3d {
            width: packet.width,
            height: packet.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: STENCIL_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let stencil_view = stencil_texture.create_view(&wgpu::TextureViewDescriptor::default());

    let mut textures = HashMap::new();
    let mut texture_bind_groups = HashMap::new();
    for (&resource_index, source) in packet.textures.iter() {
        let tex = create_gpu_texture(
            device,
            "siglus-emote-resource",
            source.width,
            source.height,
            TARGET_FORMAT,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            0,
        );
        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &tex._tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            source.rgba.as_slice(),
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(4 * source.width),
                rows_per_image: Some(source.height),
            },
            wgpu::Extent3d {
                width: source.width,
                height: source.height,
                depth_or_array_layers: 1,
            },
        );
        let bind_group = create_texture_bind_group(device, layout, &tex, &internal_sampler);
        textures.insert(resource_index, tex);
        texture_bind_groups.insert(resource_index, bind_group);
    }

    Ok(Target {
        output,
        feedback,
        feedback_valid: false,
        alpha_readback_version: None,
        stencil_texture,
        stencil_view,
        textures,
        texture_bind_groups,
        feedback_bind_group,
        version: 0,
    })
}

fn create_gpu_texture(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    usage: wgpu::TextureUsages,
    version: u64,
) -> GpuTexture {
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
        usage,
        view_formats: &[],
    });
    let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("siglus-emote-sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    GpuTexture {
        _tex: tex,
        view,
        sampler,
        width: width.max(1),
        height: height.max(1),
        version,
    }
}

fn create_texture_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    texture: &GpuTexture,
    sampler: &wgpu::Sampler,
) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("siglus-emote-texture-bg"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&texture.view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    })
}

fn resolve_bind_group(target: &Target, texture: EmoteTexture) -> Option<&wgpu::BindGroup> {
    match texture {
        EmoteTexture::Resource(index) => target.texture_bind_groups.get(&index),
        EmoteTexture::Feedback => target.feedback_valid.then_some(&target.feedback_bind_group),
    }
}

fn native_blend_state(mode: u32) -> wgpu::BlendState {
    let (operation, src_factor, dst_factor) = match mode {
        1 => (
            wgpu::BlendOperation::Add,
            wgpu::BlendFactor::SrcAlpha,
            wgpu::BlendFactor::One,
        ),
        2 | 5 => (
            wgpu::BlendOperation::ReverseSubtract,
            wgpu::BlendFactor::SrcAlpha,
            wgpu::BlendFactor::One,
        ),
        3 => (
            wgpu::BlendOperation::Add,
            wgpu::BlendFactor::Dst,
            wgpu::BlendFactor::OneMinusSrcAlpha,
        ),
        4 => (
            wgpu::BlendOperation::Add,
            wgpu::BlendFactor::OneMinusDst,
            wgpu::BlendFactor::One,
        ),
        _ => (
            wgpu::BlendOperation::Add,
            wgpu::BlendFactor::SrcAlpha,
            wgpu::BlendFactor::OneMinusSrcAlpha,
        ),
    };
    let color = wgpu::BlendComponent {
        src_factor,
        dst_factor,
        operation,
    };
    let alpha = if mode == 0 {
        wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
            operation: wgpu::BlendOperation::Add,
        }
    } else {
        wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::Zero,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        }
    };
    wgpu::BlendState { color, alpha }
}

fn create_color_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    mode: u32,
    stencil: bool,
) -> wgpu::RenderPipeline {
    let depth_stencil = stencil.then_some(wgpu::DepthStencilState {
        format: STENCIL_FORMAT,
        depth_write_enabled: false,
        depth_compare: wgpu::CompareFunction::Always,
        stencil: wgpu::StencilState {
            front: wgpu::StencilFaceState {
                compare: wgpu::CompareFunction::Equal,
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Keep,
                pass_op: wgpu::StencilOperation::Keep,
            },
            back: wgpu::StencilFaceState {
                compare: wgpu::CompareFunction::Equal,
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Keep,
                pass_op: wgpu::StencilOperation::Keep,
            },
            read_mask: 0xff,
            write_mask: 0xff,
        },
        bias: wgpu::DepthBiasState::default(),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("siglus-emote-color-pipeline"),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: "vs_main",
            buffers: &[EmoteVertex::layout()],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: TARGET_FORMAT,
                blend: Some(native_blend_state(mode)),
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
        depth_stencil,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    })
}

fn create_mask_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    shader: &wgpu::ShaderModule,
    pass_op: wgpu::StencilOperation,
    label: &str,
) -> wgpu::RenderPipeline {
    let face = wgpu::StencilFaceState {
        compare: wgpu::CompareFunction::Equal,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: pass_op,
        pass_op,
    };
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: shader,
            entry_point: "vs_main",
            buffers: &[EmoteVertex::layout()],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: TARGET_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::empty(),
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
        depth_stencil: Some(wgpu::DepthStencilState {
            format: STENCIL_FORMAT,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::Always,
            stencil: wgpu::StencilState {
                front: face,
                back: face,
                read_mask: 0xff,
                write_mask: 0xff,
            },
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    })
}

fn build_draws(device: &wgpu::Device, packet: &EmoteRenderPacket) -> Result<Vec<Draw>> {
    let buffer = |vertices: &[EmoteVertex], label: &str| {
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(label),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        })
    };
    Ok(plan_emote_draws(packet)
        .into_iter()
        .map(|draw| Draw {
            texture: draw.texture,
            vertex_buffer: buffer(&draw.vertices, "siglus-emote-draw-vertices"),
            vertex_count: draw.vertices.len() as u32,
            blend_index: draw.blend_index,
            stencil_groups: draw
                .stencil_groups
                .into_iter()
                .map(|group| StencilGroup {
                    phase: group.phase,
                    sources: group
                        .sources
                        .into_iter()
                        .map(|source| StencilSource {
                            texture: source.texture,
                            vertex_buffer: buffer(
                                &source.vertices,
                                "siglus-emote-stencil-vertices",
                            ),
                            vertex_count: source.vertices.len() as u32,
                        })
                        .collect(),
                })
                .collect(),
            stencil_initial_reference: draw.stencil_initial_reference,
            stencil_final_reference: draw.stencil_final_reference,
        })
        .collect())
}
