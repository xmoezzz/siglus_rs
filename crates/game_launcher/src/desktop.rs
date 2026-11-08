//! Desktop window runner for every engine.
//!
//! SiglusEngine games go to the Siglus desktop host; RealLive, AVG32 and UK2
//! games are presented from their RGBA frames, letterboxed and scaled with
//! nearest-neighbour filtering.

use std::borrow::Cow;
use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use engine_detect::EngineKind;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::run_on_demand::EventLoopExtRunOnDemand;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

use crate::nls::Nls;
use crate::runtime::{self, FramebufferGame, GameKey, PointerButton};

/// Runs the game at `root` until it ends. Returns a process exit code.
pub fn run(root: &Path, nls: Option<Nls>) -> i32 {
    let engine = match engine_detect::detect(root) {
        Ok(engine) => engine,
        Err(error) => {
            log::error!("{}: {error}", root.display());
            return 1;
        }
    };
    if engine == EngineKind::Siglus {
        // Like the framebuffer engines below, the window is sized in
        // physical pixels: no HiDPI scaling for any engine in the app.
        return siglus_scene_vm::pump_host::run_game(&root.to_string_lossy(), true);
    }
    match run_framebuffer(root, engine, nls) {
        Ok(()) => 0,
        Err(error) => {
            log::error!("{error:#}");
            eprintln!("{error:#}");
            1
        }
    }
}

fn run_framebuffer(root: &Path, engine: EngineKind, nls: Option<Nls>) -> Result<()> {
    let game = runtime::open(root, engine, nls)?;
    let mut event_loop = EventLoop::new().context("create event loop")?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        game,
        window: None,
        gpu: None,
        cursor: (0.0, 0.0),
        last: Instant::now(),
        title: String::new(),
        error: None,
        done: false,
    };
    event_loop
        .run_app_on_demand(&mut app)
        .context("run event loop")?;
    app.game.shutdown();
    if let Some(error) = app.error {
        bail!(error);
    }
    Ok(())
}

struct App {
    game: Box<dyn FramebufferGame>,
    window: Option<&'static dyn Window>,
    gpu: Option<Gpu>,
    cursor: (f64, f64),
    last: Instant,
    title: String,
    error: Option<String>,
    done: bool,
}

impl App {
    /// Letterboxed viewport (x, y, w, h) of the frame in the surface.
    fn viewport(&self) -> (f32, f32, f32, f32) {
        let Some(window) = &self.window else {
            return (0.0, 0.0, 1.0, 1.0);
        };
        let size = window.surface_size();
        let (fw, fh) = self.game.size();
        let (sw, sh) = (size.width.max(1) as f32, size.height.max(1) as f32);
        let scale = (sw / fw as f32).min(sh / fh as f32);
        let (w, h) = (fw as f32 * scale, fh as f32 * scale);
        ((sw - w) / 2.0, (sh - h) / 2.0, w, h)
    }

    fn game_position(&self) -> (i32, i32) {
        let (vx, vy, vw, vh) = self.viewport();
        let (fw, fh) = self.game.size();
        let x = ((self.cursor.0 as f32 - vx) * fw as f32 / vw.max(1.0)).floor() as i32;
        let y = ((self.cursor.1 as f32 - vy) * fh as f32 / vh.max(1.0)).floor() as i32;
        (x.clamp(0, fw as i32 - 1), y.clamp(0, fh as i32 - 1))
    }

    fn frame(&mut self, event_loop: &dyn ActiveEventLoop) {
        let now = Instant::now();
        let dt = now.duration_since(self.last).as_millis().min(200) as u32;
        self.last = now;
        if !self.game.step(dt) {
            self.done = true;
            event_loop.exit();
            return;
        }
        let title = self.game.title();
        let Some(window) = &self.window else {
            return;
        };
        if title != self.title && !title.is_empty() {
            window.set_title(&title);
            self.title = title;
        }
        window.set_cursor_visible(self.game.cursor_visible());
        let viewport = self.viewport();
        let size = self.game.size();
        if let Some(gpu) = &mut self.gpu
            && let Err(error) = gpu.draw(self.game.frame(), size, viewport)
        {
            log::warn!("render: {error:#}");
        }
    }
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let (w, h) = self.game.size();
        let scale = if w <= 640 { 2 } else { 1 };
        let attributes = WindowAttributes::default()
            .with_title(self.game.title())
            .with_resizable(true)
            .with_surface_size(PhysicalSize::new(w * scale, h * scale))
            .with_min_surface_size(PhysicalSize::new(w / 2, h / 2));
        let window = match event_loop.create_window(attributes) {
            Ok(window) => window,
            Err(error) => {
                self.error = Some(format!("create window: {error}"));
                event_loop.exit();
                return;
            }
        };
        // Kept for the rest of the process, like the surface that borrows it.
        let window: &'static dyn Window = Box::leak(window);
        match pollster::block_on(Gpu::new(window)) {
            Ok(gpu) => self.gpu = Some(gpu),
            Err(error) => {
                self.error = Some(format!("{error:#}"));
                event_loop.exit();
                return;
            }
        }
        self.window = Some(window);
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                self.done = true;
                event_loop.exit();
            }
            WindowEvent::SurfaceResized(size) => {
                if let Some(gpu) = &mut self.gpu {
                    gpu.resize(size.width, size.height);
                }
            }
            WindowEvent::RedrawRequested => self.frame(event_loop),
            WindowEvent::PointerMoved { position, .. } => {
                self.cursor = (position.x, position.y);
                let (x, y) = self.game_position();
                self.game.pointer_move(x, y);
            }
            WindowEvent::PointerButton {
                state,
                button,
                position,
                ..
            } => {
                self.cursor = (position.x, position.y);
                let (x, y) = self.game_position();
                self.game.pointer_move(x, y);
                let button = match button.mouse_button() {
                    Some(MouseButton::Left) => PointerButton::Left,
                    Some(MouseButton::Right) => PointerButton::Right,
                    _ => return,
                };
                self.game
                    .pointer_button(button, state == ElementState::Pressed);
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    MouseScrollDelta::PixelDelta(position) => position.y,
                    _ => 0.0,
                };
                if dy != 0.0 {
                    self.game.wheel(dy > 0.0);
                }
            }
            WindowEvent::Ime(Ime::Commit(text)) => self.game.text(&text),
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if let PhysicalKey::Code(code) = event.physical_key {
                    if let Some(key) = map_key(code) {
                        self.game.key(key, pressed);
                    } else if let Some(c) = event
                        .text
                        .as_ref()
                        .and_then(|text| text.chars().next())
                        .filter(|c| !c.is_control())
                    {
                        self.game.key(GameKey::Char(c), pressed);
                    }
                }
                if pressed
                    && let Some(text) = event
                        .text
                        .as_ref()
                        .filter(|text| text.chars().all(|c| !c.is_control()))
                {
                    self.game.text(text);
                }
            }
            WindowEvent::Focused(false) => {
                self.game.key(GameKey::Ctrl, false);
                self.game.key(GameKey::Shift, false);
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.done {
            event_loop.exit();
            return;
        }
        if let Some(window) = &self.window {
            window.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(16),
        ));
    }
}

pub fn map_key(code: KeyCode) -> Option<GameKey> {
    use KeyCode::*;
    Some(match code {
        Enter | NumpadEnter => GameKey::Enter,
        Escape => GameKey::Escape,
        Space => GameKey::Space,
        ArrowUp => GameKey::Up,
        ArrowDown => GameKey::Down,
        ArrowLeft => GameKey::Left,
        ArrowRight => GameKey::Right,
        PageUp => GameKey::PageUp,
        PageDown => GameKey::PageDown,
        Home => GameKey::Home,
        End => GameKey::End,
        Backspace => GameKey::Backspace,
        Tab => GameKey::Tab,
        ControlLeft | ControlRight => GameKey::Ctrl,
        ShiftLeft | ShiftRight => GameKey::Shift,
        F1 => GameKey::F(1),
        F2 => GameKey::F(2),
        F3 => GameKey::F(3),
        F4 => GameKey::F(4),
        F5 => GameKey::F(5),
        F6 => GameKey::F(6),
        F7 => GameKey::F(7),
        F8 => GameKey::F(8),
        F9 => GameKey::F(9),
        F10 => GameKey::F(10),
        F11 => GameKey::F(11),
        F12 => GameKey::F(12),
        _ => return None,
    })
}

const SHADER: &str = r#"
@group(0) @binding(0) var frame_texture: texture_2d<f32>;
@group(0) @binding(1) var frame_sampler: sampler;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 6>(
        vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(-1.0, 1.0),
        vec2(-1.0, 1.0), vec2(1.0, -1.0), vec2(1.0, 1.0),
    );
    var uvs = array<vec2<f32>, 6>(
        vec2(0.0, 1.0), vec2(1.0, 1.0), vec2(0.0, 0.0),
        vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(1.0, 0.0),
    );
    var output: VertexOutput;
    output.position = vec4(positions[index], 0.0, 1.0);
    output.uv = uvs[index];
    return output;
}

@fragment fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(frame_texture, frame_sampler, input.uv);
}
"#;

struct Gpu {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    texture: Option<(wgpu::Texture, wgpu::BindGroup, (u32, u32))>,
}

impl Gpu {
    async fn new(window: &'static dyn Window) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance.create_surface(window).context("create surface")?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                apply_limit_buckets: false,
                power_preference: wgpu::PowerPreference::LowPower,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("no graphics adapter")?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("game-launcher"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::downlevel_webgl2_defaults()
                    .using_resolution(adapter.limits()),
                ..Default::default()
            })
            .await
            .context("request device")?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(caps.formats[0]);
        let size = window.surface_size();
        let config = wgpu::SurfaceConfiguration {
            color_space: wgpu::SurfaceColorSpace::Auto,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
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
            label: None,
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            cache: None,
            label: None,
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::REPLACE),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
        });
        Ok(Self {
            surface,
            device,
            queue,
            config,
            layout,
            sampler,
            pipeline,
            texture: None,
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    fn draw(
        &mut self,
        rgba: &[u8],
        size: (u32, u32),
        viewport: (f32, f32, f32, f32),
    ) -> Result<()> {
        if rgba.len() < (size.0 * size.1 * 4) as usize {
            return Ok(());
        }
        if self
            .texture
            .as_ref()
            .is_none_or(|(_, _, current)| *current != size)
        {
            let texture = self.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: size.0,
                    height: size.1,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &self.layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&self.sampler),
                    },
                ],
            });
            self.texture = Some((texture, bind_group, size));
        }
        let (texture, bind_group, _) = self.texture.as_ref().expect("texture");
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba[..(size.0 * size.1 * 4) as usize],
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.0 * 4),
                rows_per_image: Some(size.1),
            },
            wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
        );
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(());
            }
            wgpu::CurrentSurfaceTexture::Validation => {
                anyhow::bail!("GPU surface validation failed")
            }
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                multiview_mask: None,
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    depth_slice: None,
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
            let (x, y, w, h) = viewport;
            pass.set_viewport(x, y, w.max(1.0), h.max(1.0), 0.0, 1.0);
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        Ok(())
    }
}
