use anyhow::{bail, Context, Result};
use bytemuck::{Pod, Zeroable};
use clap::Parser;
use shion_render::{render_asset_from_bytes, mat4_mul, RenderAsset, RenderMesh, RenderNode, RenderVertex};
use std::borrow::Cow;
use std::fs;
use std::path::PathBuf;
use std::time::Instant;
use wgpu::util::DeviceExt;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::window::{Window, WindowId};

#[derive(Parser, Debug)]
struct Args {
    /// Input DirectX .x file.
    x_file: PathBuf,

    /// Initial window width.
    #[arg(long, default_value_t = 1280)]
    width: u32,

    /// Initial window height.
    #[arg(long, default_value_t = 720)]
    height: u32,

    /// Disable automatic orbit camera.
    #[arg(long)]
    no_orbit: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct ViewerVertex {
    position: [f32; 3],
    normal: [f32; 3],
    color: [f32; 4],
}

impl ViewerVertex {
    const ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x3,
        2 => Float32x4
    ];

    fn layout<'a>() -> wgpu::VertexBufferLayout<'a> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<ViewerVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct Uniforms {
    view_proj: [[f32; 4]; 4],
    light_dir: [f32; 4],
}

#[derive(Debug, Clone, Copy)]
struct Bounds {
    min: [f32; 3],
    max: [f32; 3],
}

impl Bounds {
    fn empty() -> Self {
        Self {
            min: [f32::INFINITY; 3],
            max: [f32::NEG_INFINITY; 3],
        }
    }

    fn include(&mut self, p: [f32; 3]) {
        for i in 0..3 {
            self.min[i] = self.min[i].min(p[i]);
            self.max[i] = self.max[i].max(p[i]);
        }
    }

    fn valid(&self) -> bool {
        self.min.iter().all(|v| v.is_finite()) && self.max.iter().all(|v| v.is_finite())
    }

    fn center(&self) -> [f32; 3] {
        [
            (self.min[0] + self.max[0]) * 0.5,
            (self.min[1] + self.max[1]) * 0.5,
            (self.min[2] + self.max[2]) * 0.5,
        ]
    }

    fn radius(&self) -> f32 {
        let dx = self.max[0] - self.min[0];
        let dy = self.max[1] - self.min[1];
        let dz = self.max[2] - self.min[2];
        (dx * dx + dy * dy + dz * dz).sqrt().max(1.0) * 0.5
    }
}

#[derive(Debug)]
struct CpuMesh {
    vertices: Vec<ViewerVertex>,
    indices: Vec<u32>,
    bounds: Bounds,
}

impl CpuMesh {
    fn from_render_asset(asset: &RenderAsset) -> Result<Self> {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let mut bounds = Bounds::empty();

        for mesh in &asset.meshes {
            append_mesh(asset, mesh, &mut vertices, &mut indices, &mut bounds)?;
        }

        if vertices.is_empty() || indices.is_empty() || !bounds.valid() {
            bail!("the .x scene did not produce drawable triangles");
        }

        Ok(Self {
            vertices,
            indices,
            bounds,
        })
    }
}

fn append_mesh(
    asset: &RenderAsset,
    mesh: &RenderMesh,
    out_vertices: &mut Vec<ViewerVertex>,
    out_indices: &mut Vec<u32>,
    out_bounds: &mut Bounds,
) -> Result<()> {
    let node = mesh
        .node_index
        .and_then(|idx| asset.nodes.get(idx));

    if mesh.batches.is_empty() {
        append_index_range(asset, mesh, node, None, 0, mesh.indices.len(), out_vertices, out_indices, out_bounds)?;
        return Ok(());
    }

    for batch in &mesh.batches {
        let start = batch.index_start as usize;
        let count = batch.index_count as usize;
        let material_index = batch.material_index;
        append_index_range(
            asset,
            mesh,
            node,
            material_index,
            start,
            count,
            out_vertices,
            out_indices,
            out_bounds,
        )?;
    }

    Ok(())
}

fn append_index_range(
    asset: &RenderAsset,
    mesh: &RenderMesh,
    node: Option<&RenderNode>,
    material_index: Option<usize>,
    start: usize,
    count: usize,
    out_vertices: &mut Vec<ViewerVertex>,
    out_indices: &mut Vec<u32>,
    out_bounds: &mut Bounds,
) -> Result<()> {
    let color = material_index
        .and_then(|idx| asset.materials.get(idx))
        .map(|m| m.diffuse_rgba)
        .unwrap_or([0.85, 0.85, 0.85, 1.0]);

    let transform = node.map(|n| n.world_transform);
    let end = start.saturating_add(count).min(mesh.indices.len());
    for src_index in &mesh.indices[start..end] {
        let source_vertex = mesh
            .vertices
            .get(*src_index as usize)
            .with_context(|| format!("mesh index {} is out of range", src_index))?;

        let skinned = skin_vertex_bind_pose(asset, source_vertex)?;
        let (position, normal) = if let Some(skinned) = skinned {
            skinned
        } else {
            let position = transform
                .map(|m| transform_point_xfile_order(m, source_vertex.position))
                .unwrap_or(source_vertex.position);
            let normal = transform
                .map(|m| normalize3(transform_vector_xfile_order(m, source_vertex.normal)))
                .unwrap_or_else(|| normalize3(source_vertex.normal));
            (position, normal)
        };

        let dst_index = out_vertices.len() as u32;
        out_vertices.push(ViewerVertex {
            position,
            normal,
            color,
        });
        out_indices.push(dst_index);
        out_bounds.include(position);
    }

    Ok(())
}

fn skin_vertex_bind_pose(asset: &RenderAsset, vertex: &RenderVertex) -> Result<Option<([f32; 3], [f32; 3])>> {
    let mut position = [0.0f32; 3];
    let mut normal = [0.0f32; 3];
    let mut used_weight = 0.0f32;

    for slot in 0..4 {
        let weight = vertex.bone_weights[slot];
        if weight.abs() <= f32::EPSILON {
            continue;
        }
        let bone_index = vertex.bone_indices[slot] as usize;
        let bone = asset
            .bones
            .get(bone_index)
            .with_context(|| format!("vertex references missing bone index {bone_index}"))?;
        let node_index = bone
            .node_index
            .with_context(|| format!("bone {} does not resolve to a frame node", bone.name))?;
        let node = asset
            .nodes
            .get(node_index)
            .with_context(|| format!("bone {} references missing node index {node_index}", bone.name))?;

        // DirectX skinning convention for row-vector .x matrices:
        // final = source_vertex * bone_offset_matrix * current_bone_world_matrix.
        // The viewer has no animation sampling yet, so current_bone_world_matrix is the bind-pose frame world matrix.
        let skin_matrix = mat4_mul(bone.offset_matrix, node.world_transform);
        let p = transform_point_xfile_order(skin_matrix, vertex.position);
        let n = transform_vector_xfile_order(skin_matrix, vertex.normal);
        for axis in 0..3 {
            position[axis] += p[axis] * weight;
            normal[axis] += n[axis] * weight;
        }
        used_weight += weight;
    }

    if used_weight.abs() <= f32::EPSILON {
        Ok(None)
    } else {
        Ok(Some((position, normalize3(normal))))
    }
}

fn transform_point_xfile_order(m: [f32; 16], p: [f32; 3]) -> [f32; 3] {
    [
        p[0] * m[0] + p[1] * m[4] + p[2] * m[8] + m[12],
        p[0] * m[1] + p[1] * m[5] + p[2] * m[9] + m[13],
        p[0] * m[2] + p[1] * m[6] + p[2] * m[10] + m[14],
    ]
}

fn transform_vector_xfile_order(m: [f32; 16], p: [f32; 3]) -> [f32; 3] {
    [
        p[0] * m[0] + p[1] * m[4] + p[2] * m[8],
        p[0] * m[1] + p[1] * m[5] + p[2] * m[9],
        p[0] * m[2] + p[1] * m[6] + p[2] * m[10],
    ]
}

struct DepthTexture {
    view: wgpu::TextureView,
}

impl DepthTexture {
    fn create(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("siglus-x-viewer-depth"),
            size: wgpu::Extent3d {
                width: config.width.max(1),
                height: config.height.max(1),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        Self {
            view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
        }
    }
}

struct ViewerState {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    depth: DepthTexture,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    index_count: u32,
    bounds: Bounds,
    start: Instant,
    orbit: bool,
}

impl ViewerState {
    async fn new(window: &'static Window, path: PathBuf, orbit: bool) -> Result<Self> {
        let bytes = fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        let render_asset = render_asset_from_bytes(&bytes).context("parse .x into render asset")?;
        let cpu_mesh = CpuMesh::from_render_asset(&render_asset).context("build viewer mesh")?;

        eprintln!(
            "loaded {}: meshes={} materials={} vertices={} indices={}",
            path.display(),
            render_asset.meshes.len(),
            render_asset.materials.len(),
            cpu_mesh.vertices.len(),
            cpu_mesh.indices.len()
        );

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let surface = instance.create_surface(window).context("create surface")?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("request adapter")?;
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("siglus-x-viewer-device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .context("request device")?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let present_mode = caps
            .present_modes
            .iter()
            .copied()
            .find(|m| *m == wgpu::PresentMode::Fifo)
            .unwrap_or(caps.present_modes[0]);
        let alpha_mode = caps
            .alpha_modes
            .iter()
            .copied()
            .find(|m| *m == wgpu::CompositeAlphaMode::Opaque)
            .unwrap_or(caps.alpha_modes[0]);
        let size = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode,
            alpha_mode,
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let depth = DepthTexture::create(&device, &config);

        let uniforms = Uniforms {
            view_proj: identity_mat4(),
            light_dir: normalize4([0.4, -0.8, 0.45, 0.0]),
        };
        let uniform_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("siglus-x-viewer-uniforms"),
            contents: bytemuck::bytes_of(&uniforms),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("siglus-x-viewer-bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("siglus-x-viewer-bg"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("siglus-x-viewer-shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_WGSL)),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("siglus-x-viewer-pipeline-layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("siglus-x-viewer-pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                buffers: &[ViewerVertex::layout()],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
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

        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("siglus-x-viewer-vertices"),
            contents: bytemuck::cast_slice(&cpu_mesh.vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("siglus-x-viewer-indices"),
            contents: bytemuck::cast_slice(&cpu_mesh.indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        Ok(Self {
            surface,
            device,
            queue,
            config,
            depth,
            pipeline,
            bind_group,
            uniform_buffer,
            vertex_buffer,
            index_buffer,
            index_count: cpu_mesh.indices.len() as u32,
            bounds: cpu_mesh.bounds,
            start: Instant::now(),
            orbit,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        self.depth = DepthTexture::create(&self.device, &self.config);
    }

    fn render(&mut self) -> Result<()> {
        let aspect = self.config.width as f32 / self.config.height.max(1) as f32;
        let elapsed = self.start.elapsed().as_secs_f32();
        let angle = if self.orbit { elapsed * 0.45 } else { 0.7 };
        let center = self.bounds.center();
        let radius = self.bounds.radius();
        let distance = (radius * 3.0).max(3.0);
        let eye = [
            center[0] + distance * angle.sin(),
            center[1] + radius * 0.55,
            center[2] - distance * angle.cos(),
        ];
        let view = look_at_lh(eye, center, [0.0, 1.0, 0.0]);
        let proj = perspective_lh(45.0_f32.to_radians(), aspect, 0.01, (distance + radius * 8.0).max(100.0));
        let uniforms = Uniforms {
            view_proj: mul_mat4(proj, view),
            light_dir: normalize4([0.4, -0.8, 0.45, 0.0]),
        };
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::bytes_of(&uniforms));

        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            Err(wgpu::SurfaceError::Timeout) => return Ok(()),
            Err(wgpu::SurfaceError::OutOfMemory) => bail!("surface out of memory"),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("siglus-x-viewer-encoder"),
            });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("siglus-x-viewer-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.03,
                            g: 0.035,
                            b: 0.045,
                            a: 1.0,
                        }),
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
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
            pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..self.index_count, 0, 0..1);
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
}

struct App {
    args: Args,
    window: Option<&'static Window>,
    state: Option<ViewerState>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let title = format!("Siglus .x viewer - {}", self.args.x_file.display());
        let attrs = Window::default_attributes()
            .with_title(title)
            .with_inner_size(LogicalSize::new(self.args.width as f64, self.args.height as f64));
        let window = match event_loop.create_window(attrs) {
            Ok(window) => window,
            Err(err) => {
                eprintln!("failed to create window: {err}");
                event_loop.exit();
                return;
            }
        };
        let window: &'static Window = Box::leak(Box::new(window));
        match pollster::block_on(ViewerState::new(
            window,
            self.args.x_file.clone(),
            !self.args.no_orbit,
        )) {
            Ok(state) => {
                self.window = Some(window);
                self.state = Some(state);
                window.request_redraw();
            }
            Err(err) => {
                eprintln!("failed to initialize viewer: {err:?}");
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, window_id: WindowId, event: WindowEvent) {
        let Some(window) = self.window else {
            return;
        };
        if window.id() != window_id {
            return;
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(state) = self.state.as_mut() {
                    state.resize(size.width, size.height);
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(state) = self.state.as_mut() {
                    let size = window.inner_size();
                    state.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(state) = self.state.as_mut() {
                    if let Err(err) = state.render() {
                        eprintln!("render failed: {err:?}");
                        event_loop.exit();
                    }
                }
                window.request_redraw();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _event_loop: &ActiveEventLoop) {
        if let Some(window) = self.window {
            window.request_redraw();
        }
    }
}

fn main() -> Result<()> {
    env_logger::init();
    let args = Args::parse();
    let event_loop = EventLoop::new().context("create event loop")?;
    let mut app = App {
        args,
        window: None,
        state: None,
    };
    event_loop.run_app(&mut app).context("run viewer")
}

fn identity_mat4() -> [[f32; 4]; 4] {
    [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]
}

fn perspective_lh(fovy: f32, aspect: f32, z_near: f32, z_far: f32) -> [[f32; 4]; 4] {
    let y_scale = 1.0 / (fovy * 0.5).tan();
    let x_scale = y_scale / aspect.max(0.001);
    let z_range = z_far - z_near;
    [
        [x_scale, 0.0, 0.0, 0.0],
        [0.0, y_scale, 0.0, 0.0],
        [0.0, 0.0, z_far / z_range, 1.0],
        [0.0, 0.0, -z_near * z_far / z_range, 0.0],
    ]
}

fn look_at_lh(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [[f32; 4]; 4] {
    let z = normalize3(sub3(target, eye));
    let x = normalize3(cross3(up, z));
    let y = cross3(z, x);
    [
        [x[0], y[0], z[0], 0.0],
        [x[1], y[1], z[1], 0.0],
        [x[2], y[2], z[2], 0.0],
        [-dot3(x, eye), -dot3(y, eye), -dot3(z, eye), 1.0],
    ]
}

fn mul_mat4(a: [[f32; 4]; 4], b: [[f32; 4]; 4]) -> [[f32; 4]; 4] {
    let mut out = [[0.0; 4]; 4];
    for c in 0..4 {
        for r in 0..4 {
            out[c][r] = a[0][r] * b[c][0]
                + a[1][r] * b[c][1]
                + a[2][r] * b[c][2]
                + a[3][r] * b[c][3];
        }
    }
    out
}

fn sub3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = dot3(v, v).sqrt();
    if len <= 0.000001 || !len.is_finite() {
        [0.0, 1.0, 0.0]
    } else {
        [v[0] / len, v[1] / len, v[2] / len]
    }
}

fn normalize4(v: [f32; 4]) -> [f32; 4] {
    let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if len <= 0.000001 || !len.is_finite() {
        [0.0, -1.0, 0.0, v[3]]
    } else {
        [v[0] / len, v[1] / len, v[2] / len, v[3]]
    }
}

const SHADER_WGSL: &str = r#"
struct Uniforms {
    view_proj: mat4x4<f32>,
    light_dir: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> u: Uniforms;

struct VsIn {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) color: vec4<f32>,
};

struct VsOut {
    @builtin(position) position: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(input: VsIn) -> VsOut {
    var out: VsOut;
    out.position = u.view_proj * vec4<f32>(input.position, 1.0);
    out.normal = normalize(input.normal);
    out.color = input.color;
    return out;
}

@fragment
fn fs_main(input: VsOut) -> @location(0) vec4<f32> {
    let n = normalize(input.normal);
    let l = normalize(-u.light_dir.xyz);
    let ndotl = max(dot(n, l), 0.0);
    let lighting = 0.28 + 0.72 * ndotl;
    return vec4<f32>(input.color.rgb * lighting, input.color.a);
}
"#;
