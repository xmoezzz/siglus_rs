//! Desktop player for UK2 (`UK2.EXE`) games.
//!
//! The engine port runs on its own thread exactly like the original DOS
//! program: it busy-waits on emulated VSYNC/mouse/keyboard interrupts.  The
//! window thread converts host input into PC-98 scan codes and absolute
//! mouse positions, and displays the frames the engine presents.

use std::borrow::Cow;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use pollster::block_on;
use uk2::Uk2Game;
use uk2::engine::audio::MusicPlayer;
use uk2::engine::vram::{SCREEN_H, SCREEN_W};
use uk2::engine::{Engine, HostEvent, MusicCommand, Platform, VSYNC_HZ, scan};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes, WindowId};

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

fn main() -> Result<()> {
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow::anyhow!("usage: uk2_player <game-directory>"))?;
    let event_loop = EventLoop::new()?;
    event_loop.run_app(Uk2App::new(root))?;
    Ok(())
}

/// Latest frame handed from the engine thread to the window thread.
#[derive(Default)]
struct FrameSlot {
    rgba: Vec<u8>,
    fresh: bool,
    finished: bool,
}

struct DesktopPlatform {
    start: Instant,
    events: Receiver<HostEvent>,
    frame: Arc<Mutex<FrameSlot>>,
    music: Option<MusicPlayer>,
}

impl DesktopPlatform {
    fn tick_at(&self, instant: Instant) -> u64 {
        (instant.duration_since(self.start).as_secs_f64() * VSYNC_HZ) as u64
    }
}

impl Platform for DesktopPlatform {
    fn ticks(&mut self) -> u64 {
        self.tick_at(Instant::now())
    }

    fn wait(&mut self) {
        // Sleep until the next VSYNC; the original waits on interrupts.
        let next = (self.tick_at(Instant::now()) + 1) as f64 / VSYNC_HZ;
        let target = self.start + Duration::from_secs_f64(next);
        let now = Instant::now();
        if target > now {
            std::thread::sleep((target - now).min(Duration::from_millis(20)));
        }
    }

    fn poll_events(&mut self, events: &mut Vec<HostEvent>) {
        events.extend(self.events.try_iter());
    }

    fn present(&mut self, rgba: &[u8]) {
        let mut slot = self.frame.lock().expect("frame slot poisoned");
        slot.rgba.clear();
        slot.rgba.extend_from_slice(rgba);
        slot.fresh = true;
    }

    fn music(&mut self, command: MusicCommand) {
        if let Some(music) = &mut self.music {
            music.command(command);
        }
    }
}

struct Uk2App {
    root: PathBuf,
    window: Option<&'static dyn Window>,
    graphics: Option<Graphics>,
    events: Option<Sender<HostEvent>>,
    frame: Arc<Mutex<FrameSlot>>,
    engine_thread: Option<JoinHandle<()>>,
}

impl Uk2App {
    fn new(root: PathBuf) -> Self {
        Self {
            root,
            window: None,
            graphics: None,
            events: None,
            frame: Arc::new(Mutex::new(FrameSlot::default())),
            engine_thread: None,
        }
    }

    fn send(&self, event: HostEvent) {
        if let Some(events) = &self.events {
            let _ = events.send(event);
        }
    }

    fn start_engine(&mut self) -> Result<()> {
        let game = Uk2Game::open(&self.root)?;
        let (tx, rx) = mpsc::channel();
        let frame = Arc::clone(&self.frame);
        let thread = std::thread::Builder::new()
            .name("uk2-engine".to_owned())
            .spawn(move || {
                let music = match MusicPlayer::new() {
                    Ok(music) => Some(music),
                    Err(error) => {
                        eprintln!("uk2: audio disabled: {error:#}");
                        None
                    }
                };
                let platform = DesktopPlatform {
                    start: Instant::now(),
                    events: rx,
                    frame: Arc::clone(&frame),
                    music,
                };
                let result =
                    Engine::new(game, Box::new(platform)).and_then(|mut engine| engine.run());
                if let Err(error) = result
                    && !Engine::quit_requested(&error)
                {
                    eprintln!("uk2: engine stopped: {error:#}");
                }
                frame.lock().expect("frame slot poisoned").finished = true;
            })
            .context("spawn engine thread")?;
        self.events = Some(tx);
        self.engine_thread = Some(thread);
        Ok(())
    }
}

impl ApplicationHandler for Uk2App {
    fn can_create_surfaces(&mut self, event_loop: &dyn ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let window = match event_loop.create_window(
            WindowAttributes::default()
                .with_title("uk2 player")
                .with_resizable(true)
                .with_surface_size(PhysicalSize::new(SCREEN_W as u32, SCREEN_H as u32))
                .with_min_surface_size(PhysicalSize::new(320, 200)),
        ) {
            Ok(window) => window,
            Err(error) => {
                eprintln!("create uk2 window: {error}");
                event_loop.exit();
                return;
            }
        };
        let window: &'static dyn Window = Box::leak(window);
        // The engine draws the PC-98 software cursor itself.
        window.set_cursor_visible(false);
        match block_on(Graphics::new(window)).and_then(|graphics| {
            self.graphics = Some(graphics);
            self.start_engine()
        }) {
            Ok(()) => {
                self.window = Some(window);
                event_loop.set_control_flow(ControlFlow::Poll);
            }
            Err(error) => {
                eprintln!("initialize UK2 player: {error:#}");
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
        match event {
            WindowEvent::RedrawRequested => {
                if let Some(graphics) = self.graphics.as_mut()
                    && let Err(error) = graphics.render()
                {
                    eprintln!("render error: {error:#}");
                }
            }
            WindowEvent::CloseRequested => {
                self.send(HostEvent::Quit);
                if self.events.is_none() {
                    event_loop.exit();
                }
            }
            WindowEvent::SurfaceResized(size) => {
                if let Some(graphics) = self.graphics.as_mut() {
                    graphics.resize(size.width.max(1), size.height.max(1));
                }
                window.request_redraw();
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key
                    && let Some(code) = pc98_scan_code(code)
                {
                    self.send(HostEvent::Key {
                        code,
                        pressed: event.state == ElementState::Pressed,
                    });
                }
            }
            WindowEvent::PointerButton { state, button, .. } => {
                let left = match button.mouse_button() {
                    Some(MouseButton::Left) => true,
                    Some(MouseButton::Right) => false,
                    _ => return,
                };
                self.send(HostEvent::MouseButton {
                    left,
                    pressed: state == ElementState::Pressed,
                });
            }
            WindowEvent::PointerMoved { position, .. } => {
                let size = window.surface_size();
                let x = position.x * SCREEN_W as f64 / f64::from(size.width.max(1));
                let y = position.y * SCREEN_H as f64 / f64::from(size.height.max(1));
                self.send(HostEvent::MouseMove {
                    x: (x as i32).clamp(0, SCREEN_W as i32 - 1),
                    y: (y as i32).clamp(0, SCREEN_H as i32 - 1),
                });
            }
            WindowEvent::Focused(false) => {
                // Release everything so no key sticks while unfocused.
                for code in [scan::SHIFT, scan::CTRL, scan::GRPH] {
                    self.send(HostEvent::Key {
                        code,
                        pressed: false,
                    });
                }
                for left in [true, false] {
                    self.send(HostEvent::MouseButton {
                        left,
                        pressed: false,
                    });
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &dyn ActiveEventLoop) {
        let (frame, finished) = {
            let mut slot = self.frame.lock().expect("frame slot poisoned");
            let frame = slot.fresh.then(|| slot.rgba.clone());
            slot.fresh = false;
            (frame, slot.finished)
        };
        if let (Some(frame), Some(graphics)) = (frame, self.graphics.as_mut()) {
            graphics.upload(&frame);
            if let Some(window) = self.window {
                window.request_redraw();
            }
        }
        if finished {
            if let Some(thread) = self.engine_thread.take() {
                let _ = thread.join();
            }
            self.events = None;
            event_loop.exit();
            return;
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            Instant::now() + Duration::from_millis(8),
        ));
    }
}

/// Maps a host key to the PC-98 keyboard scan code.
fn pc98_scan_code(code: KeyCode) -> Option<u8> {
    use KeyCode::*;
    Some(match code {
        Escape => scan::ESC,
        Digit1 => 0x01,
        Digit2 => 0x02,
        Digit3 => 0x03,
        Digit4 => 0x04,
        Digit5 => 0x05,
        Digit6 => 0x06,
        Digit7 => 0x07,
        Digit8 => 0x08,
        Digit9 => 0x09,
        Digit0 => 0x0A,
        Minus => 0x0B,
        Equal => 0x0C,
        IntlYen => 0x0D,
        Backspace => 0x0E,
        Tab => 0x0F,
        KeyQ => 0x10,
        KeyW => 0x11,
        KeyE => 0x12,
        KeyR => 0x13,
        KeyT => 0x14,
        KeyY => 0x15,
        KeyU => 0x16,
        KeyI => 0x17,
        KeyO => 0x18,
        KeyP => 0x19,
        BracketLeft => 0x1A,
        BracketRight => 0x1B,
        Enter => scan::RETURN,
        KeyA => 0x1D,
        KeyS => 0x1E,
        KeyD => 0x1F,
        KeyF => 0x20,
        KeyG => 0x21,
        KeyH => 0x22,
        KeyJ => 0x23,
        KeyK => 0x24,
        KeyL => 0x25,
        Semicolon => 0x26,
        Quote => 0x27,
        Backslash => 0x28,
        KeyZ => 0x29,
        KeyX => 0x2A,
        KeyC => 0x2B,
        KeyV => 0x2C,
        KeyB => 0x2D,
        KeyN => 0x2E,
        KeyM => 0x2F,
        Comma => 0x30,
        Period => 0x31,
        Slash => 0x32,
        IntlRo => 0x33,
        Space => scan::SPACE,
        Convert => 0x35,
        PageUp => 0x36,
        PageDown => 0x37,
        Insert => 0x38,
        Delete => 0x39,
        ArrowUp => scan::UP,
        ArrowLeft => scan::LEFT,
        ArrowRight => scan::RIGHT,
        ArrowDown => scan::DOWN,
        Home => 0x3E,
        End => 0x3F,
        NumpadSubtract => 0x40,
        NumpadDivide => 0x41,
        Numpad7 => 0x42,
        Numpad8 => scan::PAD8,
        Numpad9 => 0x44,
        NumpadMultiply => 0x45,
        Numpad4 => scan::PAD4,
        Numpad5 => 0x47,
        Numpad6 => scan::PAD6,
        NumpadAdd => 0x49,
        Numpad1 => 0x4A,
        Numpad2 => scan::PAD2,
        Numpad3 => 0x4C,
        NumpadEqual => 0x4D,
        Numpad0 => 0x4E,
        NumpadComma => 0x4F,
        NumpadDecimal => 0x50,
        NonConvert => 0x51,
        NumpadEnter => scan::RETURN,
        F1 => 0x62,
        F2 => 0x63,
        F3 => 0x64,
        F4 => 0x65,
        F5 => 0x66,
        F6 => 0x67,
        F7 => 0x68,
        F8 => 0x69,
        F9 => 0x6A,
        F10 => 0x6B,
        F11 => scan::VF1,
        F12 => 0x53,
        ShiftLeft | ShiftRight => scan::SHIFT,
        CapsLock => scan::CAPS,
        KanaMode => 0x72,
        AltLeft | AltRight => scan::GRPH,
        ControlLeft | ControlRight => scan::CTRL,
        _ => return None,
    })
}

struct Graphics {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    pipeline: wgpu::RenderPipeline,
}

impl Graphics {
    async fn new(window: &'static dyn Window) -> Result<Self> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance.create_surface(window)?;
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
                label: Some("uk2-player-device"),
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
        let config = wgpu::SurfaceConfiguration {
            color_space: wgpu::SurfaceColorSpace::Auto,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: caps.present_modes[0],
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("uk2-frame"),
            size: wgpu::Extent3d {
                width: SCREEN_W as u32,
                height: SCREEN_H as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("uk2-frame-sampler"),
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::MipmapFilterMode::Nearest,
            ..Default::default()
        });
        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uk2-frame-layout"),
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
            label: Some("uk2-frame-bind-group"),
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
            label: Some("uk2-present-shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER)),
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("uk2-present-layout"),
            bind_group_layouts: &[Some(&bind_group_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            cache: None,
            label: Some("uk2-present-pipeline"),
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
        let graphics = Self {
            surface,
            device,
            queue,
            config,
            texture,
            bind_group,
            pipeline,
        };
        graphics.upload(&vec![0; SCREEN_W * SCREEN_H * 4]);
        Ok(graphics)
    }

    fn upload(&self, rgba: &[u8]) {
        if rgba.len() != SCREEN_W * SCREEN_H * 4 {
            return;
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(SCREEN_W as u32 * 4),
                rows_per_image: Some(SCREEN_H as u32),
            },
            wgpu::Extent3d {
                width: SCREEN_W as u32,
                height: SCREEN_H as u32,
                depth_or_array_layers: 1,
            },
        );
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.config.width = width.max(1);
        self.config.height = height.max(1);
        self.surface.configure(&self.device, &self.config);
    }

    fn render(&mut self) -> Result<()> {
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
            wgpu::CurrentSurfaceTexture::Validation => bail!("GPU surface validation failed"),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("uk2-present-encoder"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                multiview_mask: None,
                label: Some("uk2-present-pass"),
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
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &self.bind_group, &[]);
            pass.draw(0..6, 0..1);
        }
        self.queue.submit(Some(encoder.finish()));
        self.queue.present(frame);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_confirm_cancel_and_cursor_keys() {
        assert_eq!(pc98_scan_code(KeyCode::Enter), Some(scan::RETURN));
        assert_eq!(pc98_scan_code(KeyCode::Space), Some(scan::SPACE));
        assert_eq!(pc98_scan_code(KeyCode::Escape), Some(scan::ESC));
        assert_eq!(pc98_scan_code(KeyCode::ArrowUp), Some(scan::UP));
        assert_eq!(pc98_scan_code(KeyCode::Numpad2), Some(scan::PAD2));
        assert_eq!(pc98_scan_code(KeyCode::ShiftRight), Some(scan::SHIFT));
    }
}
