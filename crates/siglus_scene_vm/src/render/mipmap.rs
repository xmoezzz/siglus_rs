//! Generate image mip levels on the renderer GPU, matching the original
//! D3DUSAGE_AUTOGENMIPMAP lifetime instead of rebuilding every level on CPU.

#[derive(Debug)]
pub(super) struct MipmapGenerator {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

impl MipmapGenerator {
    pub fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("siglus-mipmap-layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("siglus-mipmap-shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mipmap.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("siglus-mipmap-pipeline-layout"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            cache: None,
            label: Some("siglus-mipmap-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
        });
        Self { layout, pipeline }
    }

    pub fn generate(
        &self,
        device: &wgpu::Device,
        texture: &wgpu::Texture,
    ) -> Option<wgpu::CommandBuffer> {
        let count = texture.mip_level_count();
        if count <= 1 {
            return None;
        }
        let views: Vec<_> = (0..count)
            .map(|level| {
                texture.create_view(&wgpu::TextureViewDescriptor {
                    usage: None,
                    label: Some("siglus-mipmap-level"),
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            })
            .collect();
        // Keep a mip chain within one command encoder, but do not retain that
        // encoder across unrelated textures. On Metal, wgpu may allocate a
        // native command buffer while ending each render pass. Holding an
        // unbounded number of such passes until the whole frame is prepared can
        // exhaust the command queue's in-flight slots before any of them have
        // been submitted, which blocks the main thread permanently.
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("siglus-mipmap-encoder"),
        });
        for level in 1..count as usize {
            let source = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("siglus-mipmap-source"),
                layout: &self.layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[level - 1]),
                }],
            });
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                multiview_mask: None,
                label: Some("siglus-mipmap-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    depth_slice: None,
                    view: &views[level],
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &source, &[]);
            pass.draw(0..3, 0..1);
        }
        Some(encoder.finish())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_level(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
        level: u32,
    ) -> Vec<u8> {
        let width = (texture.width() >> level).max(1);
        let height = (texture.height() >> level).max(1);
        let stride = (width * 4).div_ceil(256) * 256;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("mipmap-test-readback"),
            size: u64::from(stride) * u64::from(height),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
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
        let (tx, rx) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        rx.recv().unwrap().unwrap();
        let mapped = buffer.slice(..).get_mapped_range().unwrap();
        let result = mapped
            .chunks_exact(stride as usize)
            .flat_map(|row| row[..width as usize * 4].iter().copied())
            .collect();
        drop(mapped);
        buffer.unmap();
        result
    }

    #[test]
    fn gpu_mips_match_integer_reference_including_odd_sizes_alpha_and_updates() {
        let instance = wgpu::Instance::default();
        let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
            eprintln!("GPU mipmap comparison skipped: no graphics adapter");
            return;
        };
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults(),
            label: Some("mipmap-test-device"),
            ..Default::default()
        }))
        .unwrap();
        let generator = MipmapGenerator::new(&device);
        let mut textures = Vec::new();
        for (width, height) in [(1, 1), (1, 17), (19, 1), (3, 5), (16, 8), (129, 65)] {
            let img = crate::assets::RgbaImage {
                width,
                height,
                center_x: 0,
                center_y: 0,
                rgba: (0..width * height * 4)
                    .map(|i| ((i * 73 + i / 7) % 256) as u8)
                    .collect(),
            };
            let texture = super::super::create_gpu_texture(
                &device,
                &queue,
                &generator,
                "mipmap-test",
                &img,
                0,
            )
            .unwrap();
            textures.push((img, texture));
        }
        for revision in 0..2 {
            if revision == 1 {
                for (img, texture) in &mut textures {
                    for value in &mut img.rgba {
                        *value = 255 - *value;
                    }
                    super::super::upload_texture_pixels(&queue, &texture._tex, img);
                    if let Some(mipmaps) = generator.generate(&device, &texture._tex) {
                        queue.submit(Some(mipmaps));
                    }
                }
            }
            for (img, texture) in &textures {
                let (width, height) = (img.width, img.height);
                for (level, expected) in
                    super::super::build_rgba8_mip_chain(width, height, &img.rgba)
                        .iter()
                        .enumerate()
                {
                    assert_eq!(
                        read_level(&device, &queue, &texture._tex, level as u32),
                        expected.rgba,
                        "{width}x{height} mip={level} revision={revision}"
                    );
                }
            }
        }
    }
}
