//! Desktop shell for the AVG32 engine: uploads the engine's 640x480 frame
//! and routes mouse, keyboard and IME text input to it.

use std::borrow::Cow;
use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::Parser;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, Ime, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, ModifiersState, PhysicalKey};
use winit::monitor::Fullscreen;
use winit::window::{Window, WindowId};

use avg32::system::HostRequest;
use avg32::{AVG32_HEIGHT, AVG32_WIDTH, Avg32Engine, EngineOptions, Key, Nls};

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

#[derive(Debug, Parser)]
struct Args {
    /// Installed AVG32 game directory containing Gameexe.ini and DAT/SEEN.TXT.
    game_root: PathBuf,

    /// Integer window scale for the original 640x480 framebuffer.
    #[arg(long, default_value_t = 2)]
    scale: u32,

    /// Text encoding of the scenario and configuration: sjis, gbk, big5 or
    /// utf8. File names that are not found fall back to Shift-JIS.
    #[arg(long, default_value_t = Nls::Sjis)]
    nls: Nls,
}

/// Fits the fixed 640x480 AVG32 framebuffer into an arbitrarily resized
/// window while preserving its aspect ratio (pillar/letterboxed), mirroring
/// `siglus_scene_vm`'s `aspect_fit_viewport` so both engines' desktop shells
/// behave the same way under free window resizing instead of stretching the
/// image to fill a non-4:3 window.
fn aspect_fit_viewport(
    surface_w: u32,
    surface_h: u32,
    game_w: u32,
    game_h: u32,
) -> (u32, u32, u32, u32) {
    let surface_w = surface_w.max(1);
    let surface_h = surface_h.max(1);
    let game_w = game_w.max(1);
    let game_h = game_h.max(1);
    let scale = (surface_w as f64 / game_w as f64).min(surface_h as f64 / game_h as f64);
    let viewport_w = ((game_w as f64 * scale).floor() as u32).clamp(1, surface_w);
    let viewport_h = ((game_h as f64 * scale).floor() as u32).clamp(1, surface_h);
    let viewport_x = surface_w.saturating_sub(viewport_w) / 2;
    let viewport_y = surface_h.saturating_sub(viewport_h) / 2;
    (viewport_x, viewport_y, viewport_w, viewport_h)
}

struct PlayerState {
    engine: Avg32Engine,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    configuration: wgpu::SurfaceConfiguration,
    frame_texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
    cursor: (f64, f64),
    modifiers: ModifiersState,
    title: String,
}

impl PlayerState {
    async fn new(window: &'static dyn Window, root: PathBuf, nls: Nls) -> Result<Self> {
        let options = EngineOptions {
            nls,
            ..EngineOptions::default()
        };
        let mut engine = Avg32Engine::open(&root, options)
            .with_context(|| format!("open AVG32 game at {}", root.display()))?;
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos() as u64)
            .unwrap_or(0);
        engine.seed_random(seed);
        #[allow(deprecated)]
        window.set_ime_allowed(true);
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance
            .create_surface(window)
            .context("create GPU surface")?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                apply_limit_buckets: false,
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .context("request GPU adapter")?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("avg32-player-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .context("request GPU device")?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|format| format.is_srgb())
            .unwrap_or(caps.formats[0]);
        let size = window.surface_size();
        let configuration = wgpu::SurfaceConfiguration {
            color_space: wgpu::SurfaceColorSpace::Auto,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: caps
                .present_modes
                .iter()
                .copied()
                .find(|mode| *mode == wgpu::PresentMode::Fifo)
                .unwrap_or(caps.present_modes[0]),
            alpha_mode: caps
                .alpha_modes
                .iter()
                .copied()
                .find(|mode| *mode == wgpu::CompositeAlphaMode::Opaque)
                .unwrap_or(caps.alpha_modes[0]),
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &configuration);

        let frame_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("avg32-framebuffer"),
            size: wgpu::Extent3d {
                width: AVG32_WIDTH,
                height: AVG32_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let texture_view = frame_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("avg32-framebuffer-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("avg32-framebuffer-layout"),
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
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("avg32-framebuffer-bind-group"),
            layout: &bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("avg32-present-shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("avg32-present-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            cache: None,
            label: Some("avg32-present-pipeline"),
            layout: Some(&layout),
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
            engine,
            surface,
            device,
            queue,
            configuration,
            frame_texture,
            bind_group,
            pipeline,
            cursor: (0.0, 0.0),
            modifiers: ModifiersState::empty(),
            title: String::new(),
        })
    }

    fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.configuration.width = width;
        self.configuration.height = height;
        self.surface.configure(&self.device, &self.configuration);
    }

    /// The letterboxed rect (within the physical surface) the 640x480
    /// framebuffer is actually drawn into at the current window size.
    fn viewport(&self) -> (u32, u32, u32, u32) {
        aspect_fit_viewport(
            self.configuration.width,
            self.configuration.height,
            AVG32_WIDTH,
            AVG32_HEIGHT,
        )
    }

    fn game_position(&self) -> (i32, i32) {
        let (vx, vy, vw, vh) = self.viewport();
        let x = ((self.cursor.0 - f64::from(vx)) * f64::from(AVG32_WIDTH) / f64::from(vw.max(1)))
            .floor() as i32;
        let y = ((self.cursor.1 - f64::from(vy)) * f64::from(AVG32_HEIGHT) / f64::from(vh.max(1)))
            .floor() as i32;
        (x, y)
    }

    fn render(&mut self, window: &dyn Window) -> Result<bool> {
        self.engine.tick();
        for warning in self.engine.take_warnings() {
            eprintln!("avg32: {warning}");
        }
        for request in self.engine.take_requests() {
            match request {
                HostRequest::ToggleFullscreen(_) => {
                    let fullscreen = window.fullscreen().is_some();
                    window.set_fullscreen((!fullscreen).then_some(Fullscreen::Borderless(None)));
                }
                HostRequest::Quit => return Ok(false),
            }
        }
        if !self.engine.running() {
            return Ok(false);
        }
        if self.engine.window_title() != self.title {
            self.title = self.engine.window_title().to_owned();
            window.set_title(&self.title);
        }
        window.set_cursor_visible(self.engine.cursor_visible());
        let pixels = self.engine.frame_rgba();
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.frame_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &pixels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * AVG32_WIDTH),
                rows_per_image: Some(AVG32_HEIGHT),
            },
            wgpu::Extent3d {
                width: AVG32_WIDTH,
                height: AVG32_HEIGHT,
                depth_or_array_layers: 1,
            },
        );
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(frame)
            | wgpu::CurrentSurfaceTexture::Suboptimal(frame) => frame,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.configuration);
                return Ok(true);
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(true);
            }
            wgpu::CurrentSurfaceTexture::Validation => bail!("GPU surface validation failed"),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("avg32-present-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                multiview_mask: None,
                label: Some("avg32-present-pass"),
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
            let (vx, vy, vw, vh) = self.viewport();
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.set_viewport(vx as f32, vy as f32, vw as f32, vh as f32, 0.0, 1.0);
            pass.set_scissor_rect(vx, vy, vw, vh);
            pass.draw(0..6, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        Ok(true)
    }
}

fn map_key(code: KeyCode) -> Option<Key> {
    Some(match code {
        KeyCode::Enter | KeyCode::NumpadEnter => Key::Enter,
        KeyCode::ArrowUp => Key::Up,
        KeyCode::ArrowDown => Key::Down,
        KeyCode::ArrowLeft => Key::Left,
        KeyCode::ArrowRight => Key::Right,
        KeyCode::Escape => Key::Escape,
        KeyCode::Space => Key::Space,
        KeyCode::PageUp => Key::PageUp,
        KeyCode::PageDown => Key::PageDown,
        KeyCode::Backspace => Key::Backspace,
        _ => return None,
    })
}

struct App {
    args: Args,
    window: Option<&'static dyn Window>,
    state: Option<PlayerState>,
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let scale = self.args.scale.max(1);
        let window = match event_loop.create_window(
            winit::window::WindowAttributes::default()
                .with_title("AVG32")
                .with_resizable(true)
                .with_surface_size(PhysicalSize::new(AVG32_WIDTH * scale, AVG32_HEIGHT * scale))
                .with_min_surface_size(PhysicalSize::new(AVG32_WIDTH / 4, AVG32_HEIGHT / 4)),
        ) {
            Ok(window) => window,
            Err(error) => {
                eprintln!("create window: {error}");
                event_loop.exit();
                return;
            }
        };
        let window: &'static dyn Window = Box::leak(window);
        match pollster::block_on(PlayerState::new(
            window,
            self.args.game_root.clone(),
            self.args.nls,
        )) {
            Ok(state) => {
                self.window = Some(window);
                self.state = Some(state);
                window.request_redraw();
            }
            Err(error) => {
                eprintln!("initialize AVG32 player: {error:#}");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &dyn ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(window) = self.window else {
            return;
        };
        if window.id() != window_id {
            return;
        }
        let Some(state) = self.state.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => {
                state.engine.shutdown();
                event_loop.exit();
            }
            WindowEvent::SurfaceResized(size) => state.resize(size.width, size.height),
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = window.surface_size();
                state.resize(size.width, size.height);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                state.modifiers = modifiers.state();
                let skip = state.modifiers.shift_key() || state.modifiers.control_key();
                state.engine.set_skip(skip);
            }
            WindowEvent::PointerMoved { position, .. } => {
                state.cursor = (position.x, position.y);
                let (x, y) = state.game_position();
                state.engine.mouse_move(x, y);
            }
            WindowEvent::PointerButton {
                state: ElementState::Released,
                button,
                position,
                ..
            } => {
                state.cursor = (position.x, position.y);
                let (x, y) = state.game_position();
                state.engine.mouse_move(x, y);
                match button.mouse_button() {
                    Some(MouseButton::Left) => state.engine.mouse_up(false),
                    Some(MouseButton::Right) => state.engine.mouse_up(true),
                    _ => {}
                }
            }
            WindowEvent::Ime(Ime::Commit(text)) => state.engine.text_input(&text),
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y as f64,
                    winit::event::MouseScrollDelta::PixelDelta(position) => position.y,
                    _ => 0.0,
                };
                if dy.abs() > 0.0 {
                    state.engine.wheel(dy > 0.0);
                }
            }
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if code == KeyCode::F11 || (code == KeyCode::Enter && state.modifiers.alt_key())
                    {
                        let fullscreen = window.fullscreen().is_some();
                        window
                            .set_fullscreen((!fullscreen).then_some(Fullscreen::Borderless(None)));
                    } else if let Some(key) = map_key(code) {
                        state.engine.key_down(key);
                    }
                }
                if let Some(text) = event.text.as_ref()
                    && text.chars().all(|c| !c.is_control())
                {
                    state.engine.text_input(text);
                }
            }
            WindowEvent::RedrawRequested => match state.render(window) {
                Ok(true) => {}
                Ok(false) => {
                    state.engine.shutdown();
                    event_loop.exit();
                }
                Err(error) => {
                    eprintln!("AVG32: {error:#}");
                    event_loop.exit();
                }
            },
            _ => {}
        }
        window.request_redraw();
    }

    fn about_to_wait(&mut self, _event_loop: &dyn ActiveEventLoop) {
        if let Some(window) = self.window {
            window.request_redraw();
        }
    }
}

fn main() -> Result<()> {
    let event_loop = EventLoop::new().context("create event loop")?;
    event_loop
        .run_app(App {
            args: Args::parse(),
            window: None,
            state: None,
        })
        .context("run AVG32 player")
}
