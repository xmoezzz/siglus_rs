#![cfg(feature = "wgpu-winit")]

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Parser;
use image::ColorType;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::Fullscreen;
use winit::window::{Window, WindowAttributes};

use siglus_assets::gameexe::{decode_gameexe_dat_bytes, GameexeConfig, GameexeDecodeOptions};
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

    /// Optional scene name override.
    #[arg(long)]
    scene_name: Option<String>,

    /// Optional scene index override.
    #[arg(long)]
    scene_id: Option<usize>,

    /// Window width override. Defaults to `#SCREEN_SIZE` from Gameexe.dat.
    #[arg(long)]
    width: Option<u32>,

    /// Window height override. Defaults to `#SCREEN_SIZE` from Gameexe.dat.
    #[arg(long)]
    height: Option<u32>,

    /// Save one rendered frame to a PNG file.
    #[arg(long)]
    capture_png: Option<PathBuf>,

    /// Capture after this many redraws.
    #[arg(long, default_value_t = 60)]
    capture_after_frames: u32,

    /// Exit after saving the capture.
    #[arg(long, default_value_t = false)]
    exit_after_capture: bool,

    /// Maximum VM steps per frame.
    #[arg(long, default_value_t = 2000)]
    steps_per_frame: u32,

    /// Pause at startup.
    #[arg(long, default_value_t = false)]
    paused: bool,
}

#[derive(Debug, Clone)]
struct BootConfig {
    start_scene: String,
    start_z: i32,
    menu_scene: Option<String>,
    menu_z: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcType {
    Script,
    StartWarning,
    ReturnToMenu,
    GameTimerStart,
    TimeWait,
}

#[derive(Debug, Clone)]
struct ProcFrame {
    ty: ProcType,
    option: i32,
    deadline_frame: Option<u32>,
}

#[derive(Debug, Default)]
struct ProcFlow {
    stack: Vec<ProcFrame>,
    booted_menu: bool,
}

impl ProcFlow {
    fn push(&mut self, ty: ProcType, option: i32) {
        self.stack.push(ProcFrame {
            ty,
            option,
            deadline_frame: None,
        });
    }

    fn pop(&mut self) {
        let _ = self.stack.pop();
    }

    fn top_mut(&mut self) -> Option<&mut ProcFrame> {
        self.stack.last_mut()
    }

    fn top(&self) -> Option<&ProcFrame> {
        self.stack.last()
    }
}

struct App {
    args: Args,
    initial_size: (u32, u32),
    boot: BootConfig,
    flow: ProcFlow,
    window: Option<&'static Window>,
    renderer: Option<Renderer>,
    vm: Option<SceneVm<'static>>,

    paused: bool,
    step_once: bool,
    steps_per_frame: u32,

    last_window_mode: Option<i64>,
    last_window_size: Option<i64>,
    last_cursor_hide_on: Option<i64>,
    last_cursor_hide_time: Option<i64>,
    cursor_hidden: bool,
    last_mouse_move: Instant,
    redraw_count: u32,
    captured: bool,
    pending_exit: bool,
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
        let initial_size = Self::resolve_initial_size(&args);
        let boot = Self::resolve_boot_config(&args);
        let mut flow = ProcFlow::default();
        flow.push(ProcType::Script, 0);
        flow.push(ProcType::StartWarning, 0);
        Self {
            paused: args.paused,
            step_once: false,
            steps_per_frame: args.steps_per_frame,
            initial_size,
            boot,
            flow,
            args,
            window: None,
            renderer: None,
            vm: None,
            last_window_mode: None,
            last_window_size: None,
            last_cursor_hide_on: None,
            last_cursor_hide_time: None,
            cursor_hidden: false,
            last_mouse_move: Instant::now(),
            redraw_count: 0,
            captured: false,
            pending_exit: false,
        }
    }

    fn resolve_project_dir(args: &Args) -> Option<PathBuf> {
        args.project_dir
            .clone()
            .or_else(|| siglus_scene_vm::app_path::resolve_app_base_path().ok())
    }

    fn gameexe_screen_size(cfg: &GameexeConfig) -> Option<(u32, u32)> {
        let entry = cfg.get_entry("SCREEN_SIZE")?;
        let w = entry.item_unquoted(0)?.trim().parse::<u32>().ok()?;
        let h = entry.item_unquoted(1)?.trim().parse::<u32>().ok()?;
        if w == 0 || h == 0 {
            return None;
        }
        Some((w, h))
    }

    fn gameexe_scene_entry(cfg: &GameexeConfig, key: &str) -> Option<(String, i32)> {
        let entry = cfg.get_entry(key)?;
        let scene = entry.item_unquoted(0)?.trim().trim_matches('"').to_string();
        if scene.is_empty() {
            return None;
        }
        let z = entry
            .item_unquoted(1)
            .and_then(|s| s.trim().parse::<i32>().ok())
            .unwrap_or(0);
        Some((scene, z))
    }

    fn resolve_boot_config(args: &Args) -> BootConfig {
        let cfg = Self::resolve_project_dir(args)
            .as_deref()
            .and_then(Self::try_load_gameexe);
        let (default_start, default_start_z) = cfg
            .as_ref()
            .and_then(|cfg| Self::gameexe_scene_entry(cfg, "START_SCENE"))
            .unwrap_or_else(|| ("_start".to_string(), 0));
        let (menu_scene, menu_z) = cfg
            .as_ref()
            .and_then(|cfg| Self::gameexe_scene_entry(cfg, "MENU_SCENE"))
            .map(|(s, z)| (Some(s), z))
            .unwrap_or((None, 0));
        let start_scene = if let Some(name) = args.scene_name.clone() {
            name
        } else {
            default_start
        };
        BootConfig {
            start_scene,
            start_z: default_start_z,
            menu_scene,
            menu_z,
        }
    }

    fn resolve_initial_size(args: &Args) -> (u32, u32) {
        let cfg_size = Self::resolve_project_dir(args)
            .as_deref()
            .and_then(Self::try_load_gameexe)
            .as_ref()
            .and_then(Self::gameexe_screen_size)
            .unwrap_or((1280, 720));
        (
            args.width.unwrap_or(cfg_size.0),
            args.height.unwrap_or(cfg_size.1),
        )
    }

    fn write_rgba_png(path: &Path, rgba: &[u8], width: u32, height: u32) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create capture dir: {}", parent.display()))?;
        }
        image::save_buffer(path, rgba, width, height, ColorType::Rgba8)
            .with_context(|| format!("write capture png: {}", path.display()))
    }
    fn find_gameexe_path(project_dir: &Path) -> Option<PathBuf> {
        let candidates = [
            "Gameexe.dat",
            "GameexeEN.dat",
            "GameexeZH.dat",
            "GameexeZHTW.dat",
            "GameexeDE.dat",
            "GameexeES.dat",
            "GameexeFR.dat",
            "GameexeID.dat",
        ];
        for name in candidates {
            let p = project_dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
        None
    }

    fn try_load_gameexe(project_dir: &Path) -> Option<GameexeConfig> {
        let path = Self::find_gameexe_path(project_dir)?;
        let raw = std::fs::read(&path).ok()?;
        let opt = GameexeDecodeOptions::from_project_dir(project_dir).ok()?;
        let (text, _report) = decode_gameexe_dat_bytes(&raw, &opt).ok()?;
        Some(GameexeConfig::from_text(&text))
    }

    fn init_vm(&self) -> Result<SceneVm<'static>> {
        let project_dir = self
            .args
            .project_dir
            .clone()
            .unwrap_or(siglus_scene_vm::app_path::resolve_app_base_path()?);
        let scene_pck_path = find_scene_pck_in_project(&project_dir)?;
        let opt = ScenePckDecodeOptions::from_project_dir(&project_dir)?;
        let pck = ScenePck::load_and_rebuild(&scene_pck_path, &opt)
            .with_context(|| format!("open scene.pck: {}", scene_pck_path.display()))?;

        let scene_no = if let Some(id) = self.args.scene_id {
            id
        } else if let Some(name) = self.args.scene_name.as_ref() {
            pck.find_scene_no(name).unwrap_or(0)
        } else {
            pck.find_scene_no(&self.boot.start_scene).unwrap_or(0)
        };

        let chunk = pck
            .scn_data_slice(scene_no)
            .with_context(|| format!("scene_id out of range: {}", scene_no))?;

        // The VM borrows the chunk data. We keep it alive by leaking it.
        let chunk_leaked: &'static [u8] = Box::leak(chunk.to_vec().into_boxed_slice());
        let mut stream = SceneStream::new(chunk_leaked)?;
        let start_z = if self.args.scene_id.is_some() || self.args.scene_name.is_some() {
            0
        } else {
            self.boot.start_z
        };
        stream.jump_to_z_label(start_z.max(0) as usize)?;
        let mut ctx = CommandContext::new(project_dir);
        ctx.screen_w = self.initial_size.0;
        ctx.screen_h = self.initial_size.1;
        let mut vm = SceneVm::with_config(VmConfig::from_env(), stream, ctx);
        if self.args.scene_id.is_none() {
            let scene_name = if let Some(name) = self.args.scene_name.as_ref() {
                name.clone()
            } else {
                self.boot.start_scene.clone()
            };
            vm.restart_scene_name(&scene_name, start_z)?;
        }
        Ok(vm)
    }

    fn pump_vm(&mut self) -> Result<()> {
        let Some(vm) = self.vm.as_mut() else {
            return Ok(());
        };

        if self.paused && !self.step_once {
            return Ok(());
        }

        for _ in 0..self.steps_per_frame {
            let Some(proc) = self.flow.top().cloned() else {
                self.paused = true;
                break;
            };

            match proc.ty {
                ProcType::Script => {
                    let running = vm.step()?;
                    if !running || vm.is_halted() {
                        self.flow.pop();
                        let cur_scene = vm
                            .current_scene_name()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| self.boot.start_scene.clone());
                        if !self.flow.booted_menu
                            && cur_scene == self.boot.start_scene
                            && self.boot.menu_scene.is_some()
                        {
                            self.flow.push(ProcType::ReturnToMenu, 0);
                        }
                        continue;
                    }
                    if vm.is_blocked() {
                        break;
                    }
                }
                ProcType::StartWarning => {
                    // Mirror the original startup proc conservatively:
                    // if the warning assets are absent, this proc ends immediately.
                    let warning_exists = vm
                        .ctx
                        .images
                        .project_dir()
                        .join("g00")
                        .join("___SYSEVE_WARNING.g00")
                        .exists()
                        || vm
                            .ctx
                            .images
                            .project_dir()
                            .join("g00")
                            .join("___SYSEVE_WARNING.g01")
                            .exists();
                    if !warning_exists {
                        self.flow.pop();
                        continue;
                    }
                    let cur = self.redraw_count;
                    let top = self.flow.top_mut().expect("proc top");
                    match top.option {
                        0 => {
                            top.option = 1;
                            self.flow.push(ProcType::TimeWait, 0);
                            if let Some(wait) = self.flow.top_mut() {
                                wait.deadline_frame = Some(cur.saturating_add(60));
                            }
                        }
                        _ => {
                            self.flow.pop();
                        }
                    }
                    break;
                }
                ProcType::ReturnToMenu => {
                    if let Some(menu_scene) = self.boot.menu_scene.as_deref() {
                        vm.restart_scene_name(menu_scene, self.boot.menu_z)?;
                        self.flow.pop();
                        self.flow.booted_menu = true;
                        self.flow.push(ProcType::GameTimerStart, 0);
                        self.flow.push(ProcType::Script, 0);
                        continue;
                    }
                    self.flow.pop();
                }
                ProcType::GameTimerStart => {
                    self.flow.pop();
                    continue;
                }
                ProcType::TimeWait => {
                    let deadline = proc.deadline_frame.unwrap_or(self.redraw_count);
                    if self.redraw_count >= deadline {
                        self.flow.pop();
                        continue;
                    }
                    break;
                }
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
        self.redraw_count = self.redraw_count.saturating_add(1);
        if !self.captured {
            if let Some(path) = self.args.capture_png.as_ref() {
                if self.redraw_count >= self.args.capture_after_frames {
                    let img = vm.ctx.capture_frame_rgba();
                    let nonzero_alpha = img.rgba.chunks_exact(4).filter(|px| px[3] != 0).count();
                    eprintln!(
                        "[INFO] capture stats: redraws={} sprites={} unknown_forms={} unknown_elements={} nonzero_alpha={}",
                        self.redraw_count,
                        list.len(),
                        vm.unknown_forms.len(),
                        vm.ctx.unknown.element_chains.len(),
                        nonzero_alpha,
                    );
                    Self::write_rgba_png(path, &img.rgba, img.width, img.height)?;
                    if self.args.exit_after_capture {
                        let report_path = PathBuf::from("siglus_unknown_report.txt");
                        let _ = vm.ctx.unknown.write_report(&report_path);
                    }
                    eprintln!("[INFO] capture written to {}", path.display());
                    self.captured = true;
                    if self.args.exit_after_capture {
                        self.pending_exit = true;
                    }
                }
            }
        }
        Ok(())
    }

    fn syscom_int(ctx: &CommandContext, key: i32, default: i64) -> i64 {
        ctx.globals
            .syscom
            .config_int
            .get(&key)
            .copied()
            .unwrap_or(default)
    }

    fn apply_syscom_window_config(&mut self) {
        const GET_WINDOW_MODE: i32 = 172;
        const GET_WINDOW_MODE_SIZE: i32 = 175;
        const GET_MOUSE_CURSOR_HIDE_ONOFF: i32 = 260;
        const GET_MOUSE_CURSOR_HIDE_TIME: i32 = 263;

        let (Some(w), Some(vm)) = (self.window.as_ref(), self.vm.as_ref()) else {
            return;
        };

        let mode = Self::syscom_int(&vm.ctx, GET_WINDOW_MODE, 0);
        if self.last_window_mode != Some(mode) {
            if mode == 0 {
                w.set_fullscreen(None);
            } else {
                w.set_fullscreen(Some(Fullscreen::Borderless(None)));
            }
            self.last_window_mode = Some(mode);
        }

        let size_mode = Self::syscom_int(&vm.ctx, GET_WINDOW_MODE_SIZE, 0);
        if self.last_window_size != Some(size_mode) && mode == 0 {
            let (w0, h0) = self.initial_size;
            let (nw, nh) = match size_mode {
                0 => (w0, h0),
                1 => (640, 480),
                2 => (800, 600),
                3 => (1024, 768),
                4 => (1280, 720),
                5 => (1366, 768),
                6 => (1600, 900),
                7 => (1920, 1080),
                _ => (w0, h0),
            };
            let _ = w.request_inner_size(winit::dpi::PhysicalSize::new(nw, nh));
            self.last_window_size = Some(size_mode);
        }

        let hide_on = Self::syscom_int(&vm.ctx, GET_MOUSE_CURSOR_HIDE_ONOFF, 0);
        let hide_time = Self::syscom_int(&vm.ctx, GET_MOUSE_CURSOR_HIDE_TIME, 0);
        if self.last_cursor_hide_on != Some(hide_on)
            || self.last_cursor_hide_time != Some(hide_time)
        {
            if hide_on == 0 {
                w.set_cursor_visible(true);
                self.cursor_hidden = false;
            }
            self.last_cursor_hide_on = Some(hide_on);
            self.last_cursor_hide_time = Some(hide_time);
        }

        if hide_on != 0 && hide_time > 0 {
            let elapsed_ms = self.last_mouse_move.elapsed().as_millis() as i64;
            if elapsed_ms >= hide_time && !self.cursor_hidden {
                w.set_cursor_visible(false);
                self.cursor_hidden = true;
            }
        }
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, elwt: &ActiveEventLoop) {
        let size = LogicalSize::new(self.initial_size.0 as f64, self.initial_size.1 as f64);
        let window = elwt
            .create_window(
                WindowAttributes::default()
                    .with_inner_size(size)
                    .with_title("Siglus Engine (Rust)"),
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
                if let Some(vm) = self.vm.as_mut() {
                    vm.ctx.screen_w = size.width.max(1);
                    vm.ctx.screen_h = size.height.max(1);
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
                self.last_mouse_move = Instant::now();
                if self.cursor_hidden {
                    if let Some(w) = self.window.as_ref() {
                        w.set_cursor_visible(true);
                    }
                    self.cursor_hidden = false;
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

    fn about_to_wait(&mut self, elwt: &ActiveEventLoop) {
        if self.pending_exit {
            elwt.exit();
            return;
        }
        if let Err(e) = self.pump_vm() {
            eprintln!("vm error: {e:?}");
        }
        self.apply_syscom_window_config();
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }
}

fn main() -> Result<()> {
    let _ = env_logger::try_init();
    let args = Args::parse();
    let el = EventLoop::new()?;
    let mut app = App::new(args);
    el.run_app(&mut app)?;
    Ok(())
}
