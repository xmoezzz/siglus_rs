#![cfg(feature = "wgpu-winit")]

use anyhow::{Context, Result};
use clap::Parser;
use std::path::PathBuf;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::{Window, WindowAttributes};

use siglus_assets::scene_pck::{find_scene_pck_in_project, ScenePck, ScenePckDecodeOptions};

use siglus_scene_vm::render::Renderer;
use siglus_scene_vm::runtime::input::{VmKey, VmMouseButton};
use siglus_scene_vm::runtime::CommandContext;
use siglus_scene_vm::scene_stream::SceneStream;
use siglus_scene_vm::vm::{SceneVm, VmConfig};

#[derive(Debug, Parser)]
struct Args {
    /// The game's extracted root directory (contains g00/bg/etc).
    #[arg(long)]
    project_dir: Option<PathBuf>,

    /// Path to scene.pck.
    #[arg(long)]
    scene_pck: Option<PathBuf>,

    /// Scene chunk index.
    #[arg(long, default_value_t = 0)]
    scene_id: usize,

    /// Window width.
    #[arg(long, default_value_t = 1280)]
    width: u32,

    /// Window height.
    #[arg(long, default_value_t = 720)]
    height: u32,

    /// Maximum VM steps per frame.
    #[arg(long, default_value_t = 2000)]
    steps_per_frame: u32,

    /// Pause at startup.
    #[arg(long, default_value_t = false)]
    paused: bool,
}

struct App {
    args: Args,
    window: Option<&'static Window>,
    renderer: Option<Renderer>,
    vm: Option<SceneVm<'static>>,

    paused: bool,
    step_once: bool,
    steps_per_frame: u32,
}

fn map_mouse_button(b: MouseButton) -> Option<VmMouseButton> {
    match b {
        MouseButton::Left => Some(VmMouseButton::Left),
        MouseButton::Right => Some(VmMouseButton::Right),
        MouseButton::Middle => Some(VmMouseButton::Middle),
        _ => None,
    }
}

fn map_keycode(k: KeyCode) -> Option<VmKey> {
    use KeyCode::*;
    match k {
        Escape => Some(VmKey::Escape),
        Enter => Some(VmKey::Enter),
        Space => Some(VmKey::Space),
        Backspace => Some(VmKey::Backspace),
        Tab => Some(VmKey::Tab),

        ArrowLeft => Some(VmKey::ArrowLeft),
        ArrowUp => Some(VmKey::ArrowUp),
        ArrowRight => Some(VmKey::ArrowRight),
        ArrowDown => Some(VmKey::ArrowDown),

        KeyA => Some(VmKey::Letter('A')),
        KeyB => Some(VmKey::Letter('B')),
        KeyC => Some(VmKey::Letter('C')),
        KeyD => Some(VmKey::Letter('D')),
        KeyE => Some(VmKey::Letter('E')),
        KeyF => Some(VmKey::Letter('F')),
        KeyG => Some(VmKey::Letter('G')),
        KeyH => Some(VmKey::Letter('H')),
        KeyI => Some(VmKey::Letter('I')),
        KeyJ => Some(VmKey::Letter('J')),
        KeyK => Some(VmKey::Letter('K')),
        KeyL => Some(VmKey::Letter('L')),
        KeyM => Some(VmKey::Letter('M')),
        KeyN => Some(VmKey::Letter('N')),
        KeyO => Some(VmKey::Letter('O')),
        KeyP => Some(VmKey::Letter('P')),
        KeyQ => Some(VmKey::Letter('Q')),
        KeyR => Some(VmKey::Letter('R')),
        KeyS => Some(VmKey::Letter('S')),
        KeyT => Some(VmKey::Letter('T')),
        KeyU => Some(VmKey::Letter('U')),
        KeyV => Some(VmKey::Letter('V')),
        KeyW => Some(VmKey::Letter('W')),
        KeyX => Some(VmKey::Letter('X')),
        KeyY => Some(VmKey::Letter('Y')),
        KeyZ => Some(VmKey::Letter('Z')),

        Digit0 => Some(VmKey::Digit(0)),
        Digit1 => Some(VmKey::Digit(1)),
        Digit2 => Some(VmKey::Digit(2)),
        Digit3 => Some(VmKey::Digit(3)),
        Digit4 => Some(VmKey::Digit(4)),
        Digit5 => Some(VmKey::Digit(5)),
        Digit6 => Some(VmKey::Digit(6)),
        Digit7 => Some(VmKey::Digit(7)),
        Digit8 => Some(VmKey::Digit(8)),
        Digit9 => Some(VmKey::Digit(9)),

        F1 => Some(VmKey::F(1)),
        F2 => Some(VmKey::F(2)),
        F3 => Some(VmKey::F(3)),
        F4 => Some(VmKey::F(4)),
        F5 => Some(VmKey::F(5)),
        F6 => Some(VmKey::F(6)),
        F7 => Some(VmKey::F(7)),
        F8 => Some(VmKey::F(8)),
        F9 => Some(VmKey::F(9)),
        F10 => Some(VmKey::F(10)),
        F11 => Some(VmKey::F(11)),
        F12 => Some(VmKey::F(12)),

        _ => None,
    }
}

impl App {
    fn new(args: Args) -> Self {
        Self {
            paused: args.paused,
            step_once: false,
            steps_per_frame: args.steps_per_frame,
            args,
            window: None,
            renderer: None,
            vm: None,
        }
    }

    fn init_vm(&self) -> Result<SceneVm<'static>> {
        let project_dir = self
            .args
            .project_dir
            .clone()
            .unwrap_or(siglus_scene_vm::app_path::resolve_app_base_path()?);
        let scene_pck = match self.args.scene_pck.clone() {
            Some(p) => p,
            None => find_scene_pck_in_project(&project_dir)?,
        };

        let opt = ScenePckDecodeOptions::from_project_dir(&project_dir)?;
        let pck = ScenePck::load_and_rebuild(&scene_pck, &opt)
            .with_context(|| format!("open scene.pck: {}", scene_pck.display()))?;
        let chunk = pck
            .scn_data_slice(self.args.scene_id)
            .with_context(|| format!("scene_id out of range: {}", self.args.scene_id))?;

        // The VM borrows the chunk data. We keep it alive by leaking it.
        let chunk_leaked: &'static [u8] = Box::leak(chunk.to_vec().into_boxed_slice());
        let stream = SceneStream::new(chunk_leaked)?;
        let ctx = CommandContext::new(project_dir);

        Ok(SceneVm::with_config(VmConfig::from_env(), stream, ctx))
    }

    fn pump_vm(&mut self) -> Result<()> {
        let Some(vm) = self.vm.as_mut() else {
            return Ok(());
        };

        if self.paused && !self.step_once {
            return Ok(());
        }

        for _ in 0..self.steps_per_frame {
            let running = vm.step()?;
            if !running {
                self.paused = true;
                break;
            }

            // Avoid spinning when the script requested a wait.
            if vm.is_blocked() {
                break;
            }
        }

        self.step_once = false;
        Ok(())
    }

    fn redraw(&mut self) -> Result<()> {
        let Some(renderer) = self.renderer.as_mut() else {
            return Ok(());
        };
        let Some(vm) = self.vm.as_mut() else {
            return Ok(());
        };

        vm.ctx.tick_frame();
        let list = vm.ctx.render_list_with_effects();
        renderer.render_sprites(&vm.ctx.images, &list)?;
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, elwt: &ActiveEventLoop) {
        let size = LogicalSize::new(self.args.width as f64, self.args.height as f64);
        let window = elwt
            .create_window(
                WindowAttributes::default()
                    .with_inner_size(size)
                    .with_title("Siglus Scene Player"),
            )
            .expect("create window");
        let window: &'static Window = Box::leak(Box::new(window));

        let renderer = pollster::block_on(Renderer::new(window)).expect("renderer init");
        let vm = self.init_vm().expect("vm init");

        self.window = Some(window);
        self.renderer = Some(renderer);
        self.vm = Some(vm);

        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        elwt: &ActiveEventLoop,
        _id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => {
                if let Some(vm) = self.vm.as_ref() {
                    let report_path = PathBuf::from("siglus_unknown_report.txt");
                    if let Err(e) = vm.ctx.unknown.write_report(&report_path) {
                        eprintln!(
                            "[WARN] failed to write unknown report to {}: {e}",
                            report_path.display()
                        );
                    } else {
                        eprintln!("[INFO] unknown report written to {}", report_path.display());
                    }
                }
                elwt.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.resize(size.width, size.height);
                }
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key: PhysicalKey::Code(code),
                        text,
                        ..
                    },
                ..
            } => {
                if let Some(vm) = self.vm.as_mut() {
                    if let Some(k) = map_keycode(code) {
                        vm.ctx.on_key_down(k);
                    } else {
                        // Still treat as generic input for waits.
                        if vm.ctx.wait.notify_key() {
                            vm.ctx.globals.finish_wipe();
                        }
                    }
                    if let Some(t) = text.as_ref() {
                        if t.chars().any(|c| !c.is_control()) {
                            vm.ctx.on_text_input(t);
                        }
                    }
                }

                match code {
                    KeyCode::Escape => elwt.exit(),
                    KeyCode::Space => {
                        self.paused = !self.paused;
                    }
                    KeyCode::KeyN => {
                        self.step_once = true;
                    }
                    KeyCode::Equal | KeyCode::NumpadAdd => {
                        self.steps_per_frame = (self.steps_per_frame + 500).min(200_000);
                        eprintln!("steps_per_frame={}", self.steps_per_frame);
                    }
                    KeyCode::Minus | KeyCode::NumpadSubtract => {
                        self.steps_per_frame = self.steps_per_frame.saturating_sub(500).max(1);
                        eprintln!("steps_per_frame={}", self.steps_per_frame);
                    }
                    _ => {}
                }

                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Released,
                        physical_key: PhysicalKey::Code(code),
                        ..
                    },
                ..
            } => {
                if let Some(vm) = self.vm.as_mut() {
                    if let Some(k) = map_keycode(code) {
                        vm.ctx.on_key_up(k);
                    }
                }
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                if let Some(vm) = self.vm.as_mut() {
                    vm.ctx.on_text_input(&text);
                }
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = self.redraw() {
                    eprintln!("render error: {e:?}");
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(vm) = self.vm.as_mut() {
                    let x = position.x.round() as i32;
                    let y = position.y.round() as i32;
                    vm.ctx.on_mouse_move(x, y);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                if let Some(vm) = self.vm.as_mut() {
                    let dy = match delta {
                        MouseScrollDelta::LineDelta(_lx, ly) => (ly * 120.0) as i32,
                        MouseScrollDelta::PixelDelta(p) => p.y.round() as i32,
                    };
                    vm.ctx.on_mouse_wheel(dy);
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if let Some(vm) = self.vm.as_mut() {
                    if let Some(b) = map_mouse_button(button) {
                        match state {
                            ElementState::Pressed => vm.ctx.on_mouse_down(b),
                            ElementState::Released => vm.ctx.on_mouse_up(b),
                        }
                    } else if state == ElementState::Pressed {
                        if vm.ctx.wait.notify_key() {
                            vm.ctx.globals.finish_wipe();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, _elwt: &ActiveEventLoop) {
        if let Err(e) = self.pump_vm() {
            eprintln!("vm error: {e:?}");
        }
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    let el = EventLoop::new()?;
    let mut app = App::new(args);
    el.run_app(&mut app)?;
    Ok(())
}
