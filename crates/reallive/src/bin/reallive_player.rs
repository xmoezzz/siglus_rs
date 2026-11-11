//! Desktop shell for the RealLive engine: presents the engine's frame and
//! routes mouse, keyboard and IME text input to it.

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

use reallive::Nls;
use reallive::engine::{Engine, EngineOptions};
use reallive::input::{Button, InputEvent, Key};

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
    /// Game directory containing Gameexe.ini and SEEN.TXT.
    game_root: PathBuf,

    /// Window scale for the game's framebuffer.
    #[arg(long, default_value_t = 1)]
    scale: u32,

    /// Text encoding of the scenario and configuration (sjis, gbk, big5,
    /// utf8, ...). By default the encoding recorded by RLdev, else sjis.
    /// File names that are not found fall back to Shift-JIS.
    #[arg(long)]
    nls: Option<Nls>,

    /// Start at this scenario instead of #SEEN_START.
    #[arg(long)]
    scene: Option<i32>,

    /// Run without sound.
    #[arg(long)]
    no_audio: bool,

    /// Directory for save files (default: `savedata_rs` in the game).
    #[arg(long)]
    save_dir: Option<PathBuf>,
}

/// Fits the game's framebuffer into the window, keeping its aspect ratio.
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
    engine: Engine,
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
    width: u32,
    height: u32,
    last_tick: std::time::Instant,
}

impl PlayerState {
    async fn new(window: &'static dyn Window, engine: Engine) -> Result<Self> {
        let (width, height) = (
            engine.machine.sys.gfx.width as u32,
            engine.machine.sys.gfx.height as u32,
        );
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
                label: Some("reallive-player-device"),
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
            label: Some("reallive-framebuffer"),
            size: wgpu::Extent3d {
                width,
                height,
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
            label: Some("reallive-framebuffer-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("reallive-framebuffer-layout"),
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
            label: Some("reallive-framebuffer-bind-group"),
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
            label: Some("reallive-present-shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("reallive-present-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            cache: None,
            label: Some("reallive-present-pipeline"),
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
            width,
            height,
            last_tick: std::time::Instant::now(),
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

    fn viewport(&self) -> (u32, u32, u32, u32) {
        aspect_fit_viewport(
            self.configuration.width,
            self.configuration.height,
            self.width,
            self.height,
        )
    }

    fn game_position(&self) -> (i32, i32) {
        let (vx, vy, vw, vh) = self.viewport();
        let x = ((self.cursor.0 - f64::from(vx)) * f64::from(self.width) / f64::from(vw.max(1)))
            .floor() as i32;
        let y = ((self.cursor.1 - f64::from(vy)) * f64::from(self.height) / f64::from(vh.max(1)))
            .floor() as i32;
        (x, y)
    }

    fn render(&mut self, window: &dyn Window) -> Result<bool> {
        let elapsed = self.last_tick.elapsed().as_millis() as u64;
        self.last_tick = std::time::Instant::now();
        self.engine.tick(elapsed);
        for error in self.engine.machine.diagnostics.errors.drain(..) {
            eprintln!("reallive: {error}");
        }
        if self.engine.finished() {
            return Ok(false);
        }
        let (vx, vy, vw, vh) = self.viewport();
        let sys = &mut self.engine.machine.sys;
        if let Some((x, y)) = sys.warp_cursor.take() {
            let px = f64::from(vx) + f64::from(x) * f64::from(vw) / f64::from(self.width);
            let py = f64::from(vy) + f64::from(y) * f64::from(vh) / f64::from(self.height);
            let _ = window.set_cursor_position(winit::dpi::PhysicalPosition::new(px, py).into());
        }
        let title = if sys.title.is_empty() {
            sys.gameexe.str("CAPTION").unwrap_or("RealLive").to_owned()
        } else {
            sys.title.clone()
        };
        if title != self.title {
            self.title = title;
            window.set_title(&self.title);
        }
        window.set_cursor_visible(sys.cursor_visible);
        let frame = self.engine.render();
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.frame_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &frame.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * self.width),
                rows_per_image: Some(self.height),
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        let output = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(output)
            | wgpu::CurrentSurfaceTexture::Suboptimal(output) => output,
            wgpu::CurrentSurfaceTexture::Lost | wgpu::CurrentSurfaceTexture::Outdated => {
                self.surface.configure(&self.device, &self.configuration);
                return Ok(true);
            }
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => {
                return Ok(true);
            }
            wgpu::CurrentSurfaceTexture::Validation => bail!("GPU surface validation failed"),
        };
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("reallive-present-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                multiview_mask: None,
                label: Some("reallive-present-pass"),
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
        self.queue.present(output);
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
        KeyCode::Home => Key::Home,
        KeyCode::End => Key::End,
        KeyCode::Tab => Key::Tab,
        KeyCode::Backspace => Key::Backspace,
        KeyCode::ControlLeft | KeyCode::ControlRight => Key::Ctrl,
        KeyCode::ShiftLeft | KeyCode::ShiftRight => Key::Shift,
        KeyCode::F1 => Key::F(1),
        KeyCode::F2 => Key::F(2),
        KeyCode::F3 => Key::F(3),
        KeyCode::F4 => Key::F(4),
        KeyCode::F5 => Key::F(5),
        KeyCode::F6 => Key::F(6),
        KeyCode::F7 => Key::F(7),
        KeyCode::F8 => Key::F(8),
        KeyCode::F9 => Key::F(9),
        KeyCode::F10 => Key::F(10),
        KeyCode::F12 => Key::F(12),
        KeyCode::Digit1 => Key::Char('1'),
        KeyCode::Digit2 => Key::Char('2'),
        KeyCode::Digit3 => Key::Char('3'),
        KeyCode::Digit4 => Key::Char('4'),
        KeyCode::Digit5 => Key::Char('5'),
        KeyCode::Digit6 => Key::Char('6'),
        KeyCode::Digit7 => Key::Char('7'),
        KeyCode::Digit8 => Key::Char('8'),
        KeyCode::Digit9 => Key::Char('9'),
        _ => return None,
    })
}

struct App {
    args: Args,
    window: Option<&'static dyn Window>,
    state: Option<PlayerState>,
}

impl App {
    fn open_engine(&self) -> Result<Engine> {
        let mut options = EngineOptions::new(self.args.game_root.clone());
        options.nls = self.args.nls;
        options.audio = !self.args.no_audio;
        options.save_dir = self.args.save_dir.clone();
        options.start_scene = self.args.scene;
        Engine::open(options)
            .with_context(|| format!("open RealLive game at {}", self.args.game_root.display()))
    }
}

impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let engine = match self.open_engine() {
            Ok(engine) => engine,
            Err(error) => {
                eprintln!("{error:#}");
                event_loop.exit();
                return;
            }
        };
        let (width, height) = (
            engine.machine.sys.gfx.width as u32,
            engine.machine.sys.gfx.height as u32,
        );
        let scale = self.args.scale.max(1);
        let window = match event_loop.create_window(
            winit::window::WindowAttributes::default()
                .with_title("RealLive")
                .with_resizable(true)
                .with_surface_size(PhysicalSize::new(width * scale, height * scale))
                .with_min_surface_size(PhysicalSize::new(width / 4, height / 4)),
        ) {
            Ok(window) => window,
            Err(error) => {
                eprintln!("create window: {error}");
                event_loop.exit();
                return;
            }
        };
        let window: &'static dyn Window = Box::leak(window);
        #[allow(deprecated)]
        window.set_ime_allowed(true);
        match pollster::block_on(PlayerState::new(window, engine)) {
            Ok(state) => {
                self.window = Some(window);
                self.state = Some(state);
                window.request_redraw();
            }
            Err(error) => {
                eprintln!("initialize RealLive player: {error:#}");
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
                reallive::save::save_global(&state.engine.machine).ok();
                event_loop.exit();
            }
            WindowEvent::SurfaceResized(size) => state.resize(size.width, size.height),
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = window.surface_size();
                state.resize(size.width, size.height);
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                state.modifiers = modifiers.state();
                // A Ctrl or Shift release can go missing (the key let go
                // in another window, or during a system shortcut); the
                // modifier state is the reliable one.
                let input = &mut state.engine.machine.sys.input;
                if !state.modifiers.control_key() {
                    input.ctrl = false;
                }
                if !state.modifiers.shift_key() {
                    input.shift = false;
                }
            }
            WindowEvent::Focused(false) => {
                let input = &mut state.engine.machine.sys.input;
                input.ctrl = false;
                input.shift = false;
            }
            WindowEvent::PointerMoved { position, .. } => {
                state.cursor = (position.x, position.y);
                state.engine.machine.sys.input.mouse = state.game_position();
            }
            WindowEvent::PointerButton {
                state: button_state,
                button,
                position,
                ..
            } => {
                state.cursor = (position.x, position.y);
                state.engine.machine.sys.input.mouse = state.game_position();
                let which = match button.mouse_button() {
                    Some(MouseButton::Left) => Button::Left,
                    Some(MouseButton::Right) => Button::Right,
                    _ => return,
                };
                state.engine.input(match button_state {
                    ElementState::Pressed => InputEvent::Press(which),
                    ElementState::Released => InputEvent::Release(which),
                });
            }
            WindowEvent::Ime(Ime::Commit(text)) => state.engine.input(InputEvent::Text(text)),
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => f64::from(y),
                    winit::event::MouseScrollDelta::PixelDelta(position) => position.y,
                    _ => 0.0,
                };
                if dy > 0.0 {
                    state.engine.input(InputEvent::Press(Button::WheelUp));
                } else if dy < 0.0 {
                    state.engine.input(InputEvent::Press(Button::WheelDown));
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                let pressed = event.state == ElementState::Pressed;
                if let PhysicalKey::Code(code) = event.physical_key {
                    if pressed
                        && (code == KeyCode::F11
                            || (code == KeyCode::Enter && state.modifiers.alt_key()))
                    {
                        let fullscreen = window.fullscreen().is_some();
                        window
                            .set_fullscreen((!fullscreen).then_some(Fullscreen::Borderless(None)));
                    } else if let Some(key) = map_key(code) {
                        state.engine.input(if pressed {
                            InputEvent::KeyDown(key)
                        } else {
                            InputEvent::KeyUp(key)
                        });
                    }
                }
                if pressed
                    && let Some(text) = event
                        .text
                        .as_ref()
                        .filter(|t| t.chars().all(|c| !c.is_control()))
                {
                    state.engine.input(InputEvent::Text(text.to_string()));
                }
            }
            WindowEvent::RedrawRequested => match state.render(window) {
                Ok(true) => {}
                Ok(false) => {
                    reallive::save::save_global(&state.engine.machine).ok();
                    event_loop.exit();
                }
                Err(error) => {
                    eprintln!("RealLive: {error:#}");
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
        .context("run RealLive player")
}
