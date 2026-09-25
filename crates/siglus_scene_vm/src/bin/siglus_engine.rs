use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use web_time::Instant;

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use std::time::Instant;

use egui::{ColorImage, TextureHandle, TextureOptions};
use egui_wgpu::{Renderer as EguiRenderer, ScreenDescriptor};

use anyhow::{Context, Result};
use clap::Parser;
use image::ColorType;
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalSize};
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::monitor::Fullscreen;
use winit::window::{Window, WindowAttributes, WindowId};

use siglus_assets::gameexe::{GameexeConfig, decode_gameexe_dat_bytes};
use siglus_assets::scene_pck::ScenePck;

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use siglus_scene_vm::desktop_chihaya_bench::DesktopChihayaBenchWindow;
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use siglus_scene_vm::desktop_config::{ConfigDialog, DesktopConfigAction, DesktopConfigWindow};
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use siglus_scene_vm::desktop_messagebox::{DesktopMessageBoxBridge, DesktopMessageBoxWindow};
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use siglus_scene_vm::desktop_twitter::{DesktopTwitterAction, DesktopTwitterWindow};
use siglus_scene_vm::image_manager::{ImageHandle, ImageKey};
use siglus_scene_vm::layer::RenderFrame;
use siglus_scene_vm::render::{Renderer, RendererDebugTexture};
use siglus_scene_vm::runtime::forms::syscom;
use siglus_scene_vm::runtime::globals::{
    SyscomPendingProc, SyscomPendingProcKind, SystemMessageBoxButton, SystemMessageBoxModalState,
    WipeState,
};
use siglus_scene_vm::runtime::input::{VmKey, VmMouseButton};
#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
use siglus_scene_vm::runtime::twitter;
use siglus_scene_vm::runtime::{CommandContext, FrameCaptureBackendRef, ProcKind, native_ui};
use siglus_scene_vm::scene_stream::SceneStream;
use siglus_scene_vm::vm::{SceneVm, VmConfig};

/// High-resolution Siglus titles should not have their requested client size
/// multiplied by the platform HiDPI backing scale.  Keep <=720p titles on the
/// existing logical-window path for compatibility, but request >720p windows
/// in physical pixels so a 1920x1080 game does not become a 3840x2160 surface
/// on a 2x Retina display.
const PIXEL_EXACT_WINDOW_HEIGHT_THRESHOLD: u32 = 720;

#[derive(Debug, Parser)]
struct Args {
    /// The game's extracted root directory (contains g00/bg/etc).
    #[arg(long)]
    project_dir: Option<PathBuf>,

    /// Optional scene name override. Also accepted as `--scene` for direct script startup.
    #[arg(long, visible_alias = "scene")]
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

    /// Pause at startup.
    #[arg(long, default_value_t = false)]
    paused: bool,
}

#[derive(Debug, Clone)]
struct BootConfig {
    start_scene: String,
    start_z: i32,
    menu_scene: String,
    menu_z: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcType {
    Script,
    StartWarning,
    SyscomWarning,
    MsgBack,
    ReturnToMenu,
    GameEndWipe,
    Disp,
    EndGame,
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
    pending_syscom_proc: Option<SyscomPendingProc>,
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

struct HudGui {
    ctx: egui::Context,
    renderer: EguiRenderer,
    start_time: Instant,
    raw_input: egui::RawInput,
    pointer_pos: Option<egui::Pos2>,
    gpu_texture_cache: HashMap<String, HudTextureCacheEntry>,
}

struct HudState {
    window: Arc<dyn Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    gui: HudGui,
    process_memory: ProcessMemorySnapshot,
    process_before_open: ProcessMemorySnapshot,
    preview_refresh_requested: bool,
    show_memory: bool,
    show_objects: bool,
    show_textures: bool,
    card_width: f32,
    preview_height: f32,
    object_list_height: f32,
}

struct HudTextureCacheEntry {
    version: u64,
    handle: TextureHandle,
    width: u32,
    height: u32,
    debug_hash: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct ProcessMemorySnapshot {
    /// macOS phys_footprint. This is the number closest to Activity Monitor's
    /// Memory column. On other platforms the primary number falls back to RSS.
    physical_footprint_bytes: Option<u64>,
    /// Current resident working set / RSS reported by the OS.
    resident_bytes: Option<u64>,
    /// Windows private commit (not the same quantity as RSS).
    private_bytes: Option<u64>,
    /// Current virtual address-space size where the platform exposes it cheaply.
    virtual_bytes: Option<u64>,
}

impl ProcessMemorySnapshot {
    fn primary(self) -> Option<(&'static str, u64)> {
        self.physical_footprint_bytes
            .map(|bytes| ("physical footprint", bytes))
            .or_else(|| self.resident_bytes.map(|bytes| ("resident", bytes)))
            .or_else(|| self.private_bytes.map(|bytes| ("private", bytes)))
    }
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Default)]
struct MacRusageInfoV2 {
    ri_uuid: [u8; 16],
    ri_user_time: u64,
    ri_system_time: u64,
    ri_pkg_idle_wkups: u64,
    ri_interrupt_wkups: u64,
    ri_pageins: u64,
    ri_wired_size: u64,
    ri_resident_size: u64,
    ri_phys_footprint: u64,
    ri_proc_start_abstime: u64,
    ri_proc_exit_abstime: u64,
    ri_child_user_time: u64,
    ri_child_system_time: u64,
    ri_child_pkg_idle_wkups: u64,
    ri_child_interrupt_wkups: u64,
    ri_child_pageins: u64,
    ri_child_elapsed_abstime: u64,
    ri_diskio_bytesread: u64,
    ri_diskio_byteswritten: u64,
}

#[cfg(target_os = "macos")]
#[link(name = "proc")]
unsafe extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut std::ffi::c_void) -> i32;
}

#[cfg(target_os = "macos")]
fn read_process_memory_snapshot() -> ProcessMemorySnapshot {
    const RUSAGE_INFO_V2: i32 = 2;
    let mut info = MacRusageInfoV2::default();
    let rc = unsafe {
        proc_pid_rusage(
            std::process::id() as i32,
            RUSAGE_INFO_V2,
            (&mut info as *mut MacRusageInfoV2).cast(),
        )
    };
    if rc != 0 {
        return ProcessMemorySnapshot::default();
    }
    ProcessMemorySnapshot {
        physical_footprint_bytes: Some(info.ri_phys_footprint),
        resident_bytes: Some(info.ri_resident_size),
        private_bytes: None,
        virtual_bytes: None,
    }
}

#[cfg(target_os = "linux")]
fn read_process_memory_snapshot() -> ProcessMemorySnapshot {
    fn status_kib(status: &str, key: &str) -> Option<u64> {
        status.lines().find_map(|line| {
            let mut fields = line.split_whitespace();
            if fields.next()? != key {
                return None;
            }
            fields.next()?.parse::<u64>().ok().map(|kib| kib * 1024)
        })
    }

    let Ok(status) = std::fs::read_to_string("/proc/self/status") else {
        return ProcessMemorySnapshot::default();
    };
    ProcessMemorySnapshot {
        physical_footprint_bytes: None,
        resident_bytes: status_kib(&status, "VmRSS:"),
        private_bytes: None,
        virtual_bytes: status_kib(&status, "VmSize:"),
    }
}

#[cfg(target_os = "windows")]
#[repr(C)]
#[allow(non_snake_case)]
struct ProcessMemoryCountersEx {
    cb: u32,
    PageFaultCount: u32,
    PeakWorkingSetSize: usize,
    WorkingSetSize: usize,
    QuotaPeakPagedPoolUsage: usize,
    QuotaPagedPoolUsage: usize,
    QuotaPeakNonPagedPoolUsage: usize,
    QuotaNonPagedPoolUsage: usize,
    PagefileUsage: usize,
    PeakPagefileUsage: usize,
    PrivateUsage: usize,
}

#[cfg(target_os = "windows")]
#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentProcess() -> *mut std::ffi::c_void;
}

#[cfg(target_os = "windows")]
#[link(name = "psapi")]
unsafe extern "system" {
    fn GetProcessMemoryInfo(
        process: *mut std::ffi::c_void,
        counters: *mut ProcessMemoryCountersEx,
        size: u32,
    ) -> i32;
}

#[cfg(target_os = "windows")]
fn read_process_memory_snapshot() -> ProcessMemorySnapshot {
    let mut counters: ProcessMemoryCountersEx = unsafe { std::mem::zeroed() };
    counters.cb = std::mem::size_of::<ProcessMemoryCountersEx>() as u32;
    let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    if ok == 0 {
        return ProcessMemorySnapshot::default();
    }
    ProcessMemorySnapshot {
        physical_footprint_bytes: None,
        resident_bytes: Some(counters.WorkingSetSize as u64),
        private_bytes: Some(counters.PrivateUsage as u64),
        virtual_bytes: None,
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn read_process_memory_snapshot() -> ProcessMemorySnapshot {
    ProcessMemorySnapshot::default()
}

#[derive(Debug, Clone, Copy, Default)]
struct HudMemorySnapshot {
    images_cpu_bytes: usize,
    renderer_image_gpu_bytes: u64,
    renderer_external_gpu_bytes: u64,
    renderer_target_gpu_bytes: u64,
    renderer_depth_gpu_bytes: u64,
    renderer_buffer_gpu_bytes: u64,
    renderer_frame_arena_bytes: usize,
    scene_pck_bytes: usize,
    scene_streams: usize,
    movie_video_bytes: usize,
    movie_audio_bytes: usize,
    movie_frames: usize,
    movie_assets: usize,
    movie_previews: usize,
    movie_streams: usize,
    koe_cache_bytes: usize,
    koe_cache_entries: usize,
    bgm_source_bytes: usize,
    bgm_source_slots: usize,
    hud_readback_bytes: usize,
    hud_preview_bytes: usize,
    renderer_image_textures: usize,
    renderer_external_textures: usize,
    renderer_cached_pipelines: usize,
}

impl HudMemorySnapshot {
    fn tracked_engine_bytes(self) -> u64 {
        self.images_cpu_bytes as u64
            + self.renderer_image_gpu_bytes
            + self.renderer_external_gpu_bytes
            + self.renderer_target_gpu_bytes
            + self.renderer_depth_gpu_bytes
            + self.renderer_buffer_gpu_bytes
            + self.renderer_frame_arena_bytes as u64
            + self.scene_pck_bytes as u64
            + self.movie_video_bytes as u64
            + self.movie_audio_bytes as u64
            + self.koe_cache_bytes as u64
            + self.bgm_source_bytes as u64
    }
}

#[derive(Debug, Clone)]
struct HudGalleryTile {
    stage_form_id: u32,
    stage_idx: i64,
    stage_label: String,
    obj_idx: usize,
    file: String,
    backend: String,
    disp: bool,
    tr: i64,
    alpha: i64,
    bind: String,
    patno: i64,
    runtime_image_id: Option<ImageHandle>,
    image_id: Option<ImageHandle>,
    width: u32,
    height: u32,
    source_label: String,
    source_kind: String,
}

struct App {
    args: Args,
    /// Fixed Siglus script/render coordinate space from Gameexe SCREEN_SIZE.
    game_size: (u32, u32),
    /// Initial native client size. CLI --width/--height only affect this.
    initial_size: (u32, u32),
    boot: BootConfig,
    flow: ProcFlow,
    window: Option<&'static dyn Window>,
    window_id: Option<WindowId>,
    renderer: Option<Rc<RefCell<Renderer>>>,
    pending_surface_size: Option<PhysicalSize<u32>>,
    last_presented_frame: Option<RenderFrame>,
    hud: Option<HudState>,
    vm: Option<SceneVm<'static>>,

    paused: bool,
    step_once: bool,

    last_window_mode: Option<i64>,
    last_window_size: Option<i64>,
    last_cursor_hide_on: Option<i64>,
    last_cursor_hide_time: Option<i64>,
    cursor_hidden: bool,
    last_mouse_move: Instant,
    redraw_count: u32,
    frame_dirty: bool,
    script_needs_pump: bool,
    script_resume_after_redraw: bool,
    suppress_render_once: bool,
    syscom_suspended_waits: Vec<(usize, siglus_scene_vm::runtime::wait::VmWait, String)>,
    captured: bool,
    pending_exit: bool,

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    desktop_messagebox_bridge: DesktopMessageBoxBridge,
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    desktop_messagebox_window: Option<DesktopMessageBoxWindow>,
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    desktop_chihaya_bench_window: Option<DesktopChihayaBenchWindow>,
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    desktop_config_window: Option<DesktopConfigWindow>,
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    desktop_config_request: Option<ConfigDialog>,
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    desktop_config_previous_dialog: Option<ConfigDialog>,
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    desktop_config_open: bool,
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    desktop_twitter_window: Option<DesktopTwitterWindow>,
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
        Enter | NumpadEnter => Some(VmKey::Enter),
        Space => Some(VmKey::Space),
        Backspace => Some(VmKey::Backspace),
        Delete => Some(VmKey::Delete),
        Tab => Some(VmKey::Tab),
        ShiftLeft | ShiftRight => Some(VmKey::Shift),
        ControlLeft | ControlRight => Some(VmKey::Control),
        MetaLeft | MetaRight => Some(VmKey::Meta),
        AltLeft | AltRight => Some(VmKey::Alt),
        Home => Some(VmKey::Home),
        End => Some(VmKey::End),

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
        let viewport_w = ((game_w as f64 * scale).ceil() as u32)
            .min(surface_w)
            .max(1);
        let viewport_h = ((game_h as f64 * scale).ceil() as u32)
            .min(surface_h)
            .max(1);
        let viewport_x = surface_w.saturating_sub(viewport_w) / 2;
        let viewport_y = surface_h.saturating_sub(viewport_h) / 2;
        (viewport_x, viewport_y, viewport_w, viewport_h)
    }

    fn surface_point_to_game(
        position_x: f64,
        position_y: f64,
        surface_w: u32,
        surface_h: u32,
        game_w: u32,
        game_h: u32,
    ) -> (i32, i32) {
        let (vx, vy, vw, vh) = Self::aspect_fit_viewport(surface_w, surface_h, game_w, game_h);
        // Winit cursor coordinates are physical pixels.  This mirrors the
        // original engine's screen_size_proc/input conversion:
        //   (client_pos - total_game_screen_pos) * game_size / total_game_size
        // Keep out-of-viewport values negative/greater-than-size instead of
        // clamping; the original uses those values for hit testing as well.
        let px = position_x.round() as i64;
        let py = position_y.round() as i64;
        let gx = (px - vx as i64) * game_w.max(1) as i64 / vw.max(1) as i64;
        let gy = (py - vy as i64) * game_h.max(1) as i64 / vh.max(1) as i64;
        (
            gx.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            gy.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        )
    }

    fn configure_main_renderer(
        renderer: &mut Renderer,
        surface_w: u32,
        surface_h: u32,
        game_w: u32,
        game_h: u32,
    ) {
        let (vx, vy, vw, vh) = Self::aspect_fit_viewport(surface_w, surface_h, game_w, game_h);
        // Game coordinates are always Gameexe SCREEN_SIZE.  The native backing
        // scale belongs only to the OS window; it must never redefine the
        // Siglus logical render size.
        renderer.resize_with_logical_viewport(
            surface_w.max(1),
            surface_h.max(1),
            1.0,
            game_w.max(1),
            game_h.max(1),
            vx,
            vy,
            vw,
            vh,
        );
    }

    fn new(args: Args) -> Self {
        let game_size = Self::resolve_game_size(&args);
        let initial_size = (
            args.width.unwrap_or(game_size.0).max(1),
            args.height.unwrap_or(game_size.1).max(1),
        );
        let boot = Self::resolve_boot_config(&args);
        let mut flow = ProcFlow::default();
        flow.push(ProcType::Script, 0);
        flow.push(ProcType::StartWarning, 0);
        Self {
            paused: args.paused,
            step_once: false,
            game_size,
            initial_size,
            boot,
            flow,
            args,
            window: None,
            window_id: None,
            renderer: None,
            pending_surface_size: None,
            last_presented_frame: None,
            hud: None,
            vm: None,
            last_window_mode: None,
            last_window_size: None,
            last_cursor_hide_on: None,
            last_cursor_hide_time: None,
            cursor_hidden: false,
            last_mouse_move: Instant::now(),
            redraw_count: 0,
            frame_dirty: true,
            script_needs_pump: true,
            script_resume_after_redraw: false,
            suppress_render_once: false,
            syscom_suspended_waits: Vec::new(),
            captured: false,
            pending_exit: false,
            #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
            desktop_messagebox_bridge: DesktopMessageBoxBridge::new(),
            #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
            desktop_messagebox_window: None,
            #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
            desktop_chihaya_bench_window: None,
            #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
            desktop_config_window: None,
            #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
            desktop_config_request: None,
            #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
            desktop_config_previous_dialog: None,
            #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
            desktop_config_open: false,
            #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
            desktop_twitter_window: None,
        }
    }

    const HUD_STAGE_COUNT: i64 = 3;
    const HUD_OBJECT_COUNT: usize = 1024;

    fn hud_stage_name(stage_idx: i64) -> &'static str {
        match stage_idx {
            0 => "back",
            1 => "front",
            2 => "next",
            _ => "stage",
        }
    }

    fn shorten_for_hud(text: &str, max_chars: usize) -> String {
        let mut out = String::new();
        let mut count = 0usize;
        for ch in text.chars() {
            if count >= max_chars {
                out.push_str("...");
                break;
            }
            out.push(ch);
            count += 1;
        }
        out
    }

    fn hud_format_bytes(bytes: u64) -> String {
        const KIB: f64 = 1024.0;
        const MIB: f64 = 1024.0 * 1024.0;
        const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
        let bytes_f = bytes as f64;
        if bytes_f >= GIB {
            format!("{:.2} GiB", bytes_f / GIB)
        } else if bytes_f >= MIB {
            format!("{:.1} MiB", bytes_f / MIB)
        } else if bytes_f >= KIB {
            format!("{:.1} KiB", bytes_f / KIB)
        } else {
            format!("{} B", bytes)
        }
    }

    fn hud_format_byte_delta(now: u64, before: u64) -> String {
        if now >= before {
            format!("+{}", Self::hud_format_bytes(now - before))
        } else {
            format!("-{}", Self::hud_format_bytes(before - now))
        }
    }

    fn hud_file_name_from_source_path(path: &Path) -> String {
        path.file_stem()
            .or_else(|| path.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    }

    fn hud_populate_image_info(
        vm: &SceneVm<'static>,
        image_id: &ImageHandle,
        tile: &mut HudGalleryTile,
    ) {
        if let Some(info) = vm.ctx.images.debug_image_info(image_id) {
            tile.width = info.width;
            tile.height = info.height;
            if let Some(path) = info.source_path {
                if tile.file.is_empty() || tile.file == "-" || tile.file.starts_with("<obj ") {
                    tile.file = Self::hud_file_name_from_source_path(&path);
                }
                tile.source_label = path.display().to_string();
            } else if let Some(descriptor) = info.composite_descriptor {
                tile.source_label = descriptor;
                tile.source_kind = "composed-g00".to_string();
            }
        }
    }

    fn hud_renderer_image_id(texture: &RendererDebugTexture) -> Option<ImageKey> {
        texture
            .key
            .strip_prefix("image:")?
            .parse::<u32>()
            .ok()
            .map(ImageKey)
    }

    fn collect_hud_runtime_image_sources(vm: &SceneVm<'static>) -> HashMap<ImageKey, Vec<String>> {
        let mut rows = Vec::new();
        let mut seen = HashSet::new();
        Self::collect_hud_tile_metadata_from_stage_forms(vm, &mut rows, &mut seen);
        Self::collect_hud_tile_metadata_from_runtime_probe(vm, &mut rows, &mut seen);

        let mut sources: HashMap<ImageKey, Vec<String>> = HashMap::new();
        for tile in rows {
            let Some(image_id) = tile.runtime_image_id else {
                continue;
            };
            let line = format!(
                "object={}[{}] backend={} file={} patno={} bind={} disp={} tr={} alpha={}",
                tile.stage_label,
                tile.obj_idx,
                tile.backend,
                tile.file,
                tile.patno,
                tile.bind,
                if tile.disp { 1 } else { 0 },
                tile.tr,
                tile.alpha,
            );
            let entry = sources.entry(image_id.key()).or_default();
            if !entry.iter().any(|existing| existing == &line) {
                entry.push(line);
            }
        }
        sources
    }

    fn collect_hud_image_origins(
        vm: &SceneVm<'static>,
        textures: &[RendererDebugTexture],
    ) -> HashMap<ImageKey, Vec<String>> {
        let mut origins = HashMap::new();
        for texture in textures {
            let Some(image_id) = Self::hud_renderer_image_id(texture) else {
                continue;
            };
            let Some(info) = vm
                .ctx
                .images
                .image_handle(image_id)
                .as_ref()
                .and_then(|id| vm.ctx.images.debug_image_info(id))
            else {
                continue;
            };

            let mut lines = Vec::new();
            if let Some(descriptor) = info.composite_descriptor {
                let append = info.composite_append_dir.unwrap_or_default();
                lines.push(format!(
                    "origin=composed-g00 append={} descriptor={}",
                    if append.is_empty() {
                        "<root>"
                    } else {
                        append.as_str()
                    },
                    descriptor,
                ));
            }
            if let Some(path) = info.source_path {
                if let Some(frame_index) = info.frame_index {
                    lines.push(format!(
                        "origin=file {} frame/cut={}",
                        path.display(),
                        frame_index,
                    ));
                } else {
                    lines.push(format!("origin=file {}", path.display()));
                }
            }
            if lines.is_empty() {
                lines.push("origin=generated/runtime image (no file/composite key)".to_string());
            }
            origins.insert(image_id, lines);
        }
        origins
    }

    fn collect_hud_object_metadata(vm: &SceneVm<'static>) -> Vec<HudGalleryTile> {
        // Passive object-tree snapshot for debugging.  Do not resolve or load
        // preview images here: the HUD must not mutate ImageManager merely by
        // being open.  This intentionally includes objects that currently have
        // no runtime ImageHandle/binding, which are exactly the cases hidden by the
        // renderer-texture view.
        let mut rows = Vec::new();
        let mut seen = HashSet::new();
        Self::collect_hud_tile_metadata_from_stage_forms(vm, &mut rows, &mut seen);
        Self::collect_hud_tile_metadata_from_runtime_probe(vm, &mut rows, &mut seen);
        rows.sort_by_key(|tile| (tile.stage_form_id, tile.stage_idx, tile.obj_idx));
        rows
    }

    fn hud_object_participates_in_tree(
        obj: &siglus_scene_vm::runtime::globals::ObjectState,
    ) -> bool {
        if obj.object_type != 0 {
            return true;
        }
        if !obj.runtime.child_objects.is_empty() {
            return true;
        }
        !matches!(
            obj.backend,
            siglus_scene_vm::runtime::globals::ObjectBackend::None
        )
    }

    fn collect_hud_tile_metadata_from_stage_forms(
        vm: &SceneVm<'static>,
        rows: &mut Vec<HudGalleryTile>,
        seen: &mut HashSet<(u32, i64, usize)>,
    ) {
        let mut stage_form_keys = vm
            .ctx
            .globals
            .stage_forms
            .keys()
            .copied()
            .collect::<Vec<_>>();
        stage_form_keys.sort_unstable();
        for stage_form_id in stage_form_keys {
            let Some(st) = vm.ctx.globals.stage_forms.get(&stage_form_id) else {
                continue;
            };
            let mut stage_keys = st.object_lists.keys().copied().collect::<Vec<_>>();
            stage_keys.sort_unstable();
            for stage_idx in stage_keys {
                let Some(objs) = st.object_lists.get(&stage_idx) else {
                    continue;
                };
                for (obj_idx, obj) in objs.iter().enumerate() {
                    // Debug HUD intentionally inspects stale/disabled payloads too.
                    // Rendering is gated by object_slot_use, but hiding those rows
                    // here would make lifecycle corruption harder to diagnose.
                    Self::collect_hud_tile_metadata_from_object_tree(
                        vm,
                        rows,
                        seen,
                        stage_form_id,
                        stage_idx,
                        obj_idx,
                        obj,
                    );
                }
            }
        }
    }

    fn collect_hud_tile_metadata_from_object_tree(
        vm: &SceneVm<'static>,
        rows: &mut Vec<HudGalleryTile>,
        seen: &mut HashSet<(u32, i64, usize)>,
        stage_form_id: u32,
        stage_idx: i64,
        obj_idx: usize,
        obj: &siglus_scene_vm::runtime::globals::ObjectState,
    ) {
        if !Self::hud_object_participates_in_tree(obj) {
            return;
        }

        let runtime_slot = obj.runtime_slot_or(obj_idx);
        let key = (stage_form_id, stage_idx, runtime_slot);
        if seen.insert(key) {
            let mut disp = obj.base.disp != 0;
            let mut tr = obj.base.tr;
            let mut alpha = obj.base.alpha;
            let mut runtime_image_id = None;
            let mut patno = obj.base.patno;
            let width = 0u32;
            let height = 0u32;

            let bind = match &obj.backend {
                siglus_scene_vm::runtime::globals::ObjectBackend::Gfx => {
                    if let Some(v) = vm.ctx.gfx.object_peek_disp(stage_idx, runtime_slot as i64) {
                        disp = v != 0;
                    }
                    if let Some(v) = vm.ctx.gfx.object_peek_alpha(stage_idx, runtime_slot as i64) {
                        alpha = v;
                    }
                    if let Some(v) = vm.ctx.gfx.object_peek_patno(stage_idx, runtime_slot as i64) {
                        patno = v;
                    }
                    match vm
                        .ctx
                        .gfx
                        .object_sprite_binding(stage_idx, runtime_slot as i64)
                    {
                        Some((lid, sid)) => {
                            if let Some(layer) = vm.ctx.layers.layer(lid)
                                && let Some(sprite) = layer.sprite(sid)
                            {
                                // HUD must show the actual bound image. Visibility belongs to the object tree,
                                // not to a stale layer flag, so keep `disp` from object/gfx state here.
                                tr = sprite.tr as i64;
                                alpha = sprite.alpha as i64;
                                runtime_image_id = sprite.image_id.clone();
                            }
                            format!("L{}:S{}", lid, sid)
                        }
                        None => "-".to_string(),
                    }
                }
                siglus_scene_vm::runtime::globals::ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                }
                | siglus_scene_vm::runtime::globals::ObjectBackend::String {
                    layer_id,
                    sprite_id,
                    ..
                }
                | siglus_scene_vm::runtime::globals::ObjectBackend::Movie {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = vm.ctx.layers.layer(*layer_id)
                        && let Some(sprite) = layer.sprite(*sprite_id)
                    {
                        tr = sprite.tr as i64;
                        alpha = sprite.alpha as i64;
                        runtime_image_id = sprite.image_id.clone();
                    }
                    format!("L{}:S{}", layer_id, sprite_id)
                }
                siglus_scene_vm::runtime::globals::ObjectBackend::Number {
                    layer_id,
                    sprite_ids,
                }
                | siglus_scene_vm::runtime::globals::ObjectBackend::Weather {
                    layer_id,
                    sprite_ids,
                } => {
                    if let Some(&sid) = sprite_ids.first() {
                        if let Some(layer) = vm.ctx.layers.layer(*layer_id)
                            && let Some(sprite) = layer.sprite(sid)
                        {
                            tr = sprite.tr as i64;
                            alpha = sprite.alpha as i64;
                            runtime_image_id = sprite.image_id.clone();
                        }
                        format!("L{}:S{}", layer_id, sid)
                    } else {
                        "-".to_string()
                    }
                }
                siglus_scene_vm::runtime::globals::ObjectBackend::None => "-".to_string(),
            };

            let backend = match &obj.backend {
                siglus_scene_vm::runtime::globals::ObjectBackend::None => "None",
                siglus_scene_vm::runtime::globals::ObjectBackend::Gfx => "Gfx",
                siglus_scene_vm::runtime::globals::ObjectBackend::Rect { .. } => "Rect",
                siglus_scene_vm::runtime::globals::ObjectBackend::String { .. } => "String",
                siglus_scene_vm::runtime::globals::ObjectBackend::Number { .. } => "Number",
                siglus_scene_vm::runtime::globals::ObjectBackend::Weather { .. } => "Weather",
                siglus_scene_vm::runtime::globals::ObjectBackend::Movie { .. } => "Movie",
            }
            .to_string();

            let file = obj.file_name.clone().unwrap_or_else(|| "-".to_string());
            let normal_stage_form_id = if vm.ctx.ids.form_global_stage != 0 {
                vm.ctx.ids.form_global_stage
            } else {
                siglus_scene_vm::runtime::forms::codes::FORM_GLOBAL_STAGE
            };
            let stage_label = if stage_form_id == normal_stage_form_id {
                Self::hud_stage_name(stage_idx).to_string()
            } else {
                format!("EXCALL.{}", Self::hud_stage_name(stage_idx))
            };
            let mut tile = HudGalleryTile {
                stage_form_id,
                stage_idx,
                stage_label,
                obj_idx: runtime_slot,
                file: file.clone(),
                backend,
                disp,
                tr,
                alpha,
                bind,
                patno,
                runtime_image_id: runtime_image_id.clone(),
                image_id: runtime_image_id.clone(),
                width,
                height,
                source_label: file,
                source_kind: if runtime_image_id.is_some() {
                    "runtime-bind".to_string()
                } else {
                    format!("stage-form-{}", stage_form_id)
                },
            };
            if let Some(image_id) = tile.runtime_image_id.clone() {
                Self::hud_populate_image_info(vm, &image_id, &mut tile);
            }
            rows.push(tile);
        }

        for (child_idx, child) in obj.runtime.child_objects.iter().enumerate() {
            Self::collect_hud_tile_metadata_from_object_tree(
                vm,
                rows,
                seen,
                stage_form_id,
                stage_idx,
                child_idx,
                child,
            );
        }
    }

    fn collect_hud_tile_metadata_from_runtime_probe(
        vm: &SceneVm<'static>,
        rows: &mut Vec<HudGalleryTile>,
        seen: &mut HashSet<(u32, i64, usize)>,
    ) {
        let normal_stage_form_id = if vm.ctx.ids.form_global_stage != 0 {
            vm.ctx.ids.form_global_stage
        } else {
            siglus_scene_vm::runtime::forms::codes::FORM_GLOBAL_STAGE
        };
        for stage_idx in 0..Self::HUD_STAGE_COUNT {
            for obj_idx in 0..Self::HUD_OBJECT_COUNT {
                let Some((layer_id, sprite_id)) =
                    vm.ctx.gfx.object_sprite_binding(stage_idx, obj_idx as i64)
                else {
                    continue;
                };
                let Some(layer) = vm.ctx.layers.layer(layer_id) else {
                    continue;
                };
                let Some(sprite) = layer.sprite(sprite_id) else {
                    continue;
                };

                let key = (normal_stage_form_id, stage_idx, obj_idx);
                let runtime_image_id = sprite.image_id.clone();
                let mut file = format!("<obj {}>", obj_idx);
                let mut source_label = format!("runtime L{}:S{}", layer_id, sprite_id);
                let mut width = 0u32;
                let mut height = 0u32;
                if let Some(image_id) = runtime_image_id.as_ref()
                    && let Some(info) = vm.ctx.images.debug_image_info(image_id)
                {
                    width = info.width;
                    height = info.height;
                    if let Some(path) = info.source_path {
                        file = Self::hud_file_name_from_source_path(&path);
                        source_label = path.display().to_string();
                    }
                }

                if !seen.insert(key) {
                    if let Some(tile) = rows.iter_mut().find(|tile| {
                        tile.stage_form_id == normal_stage_form_id
                            && tile.stage_idx == stage_idx
                            && tile.obj_idx == obj_idx
                    }) {
                        tile.bind = format!("L{}:S{}", layer_id, sprite_id);
                        tile.disp = sprite.visible;
                        tile.tr = sprite.tr as i64;
                        tile.alpha = sprite.alpha as i64;
                        tile.patno = vm
                            .ctx
                            .gfx
                            .object_peek_patno(stage_idx, obj_idx as i64)
                            .unwrap_or(tile.patno);
                        tile.runtime_image_id = runtime_image_id.or(tile.runtime_image_id.clone());
                        if (tile.file.is_empty()
                            || tile.file == "-"
                            || tile.file.starts_with("<obj "))
                            && !file.starts_with("<obj ")
                        {
                            tile.file = file.clone();
                        }
                        if tile.source_label == tile.file || tile.source_label == "-" {
                            tile.source_label = source_label.clone();
                        }
                        if tile.width == 0 {
                            tile.width = width;
                        }
                        if tile.height == 0 {
                            tile.height = height;
                        }
                        if tile.runtime_image_id.is_some() {
                            tile.image_id = tile.runtime_image_id.clone();
                            tile.source_kind = "runtime-bind".to_string();
                        }
                    }
                    continue;
                }

                rows.push(HudGalleryTile {
                    stage_form_id: normal_stage_form_id,
                    stage_idx,
                    stage_label: Self::hud_stage_name(stage_idx).to_string(),
                    obj_idx,
                    file,
                    backend: "Gfx".to_string(),
                    disp: sprite.visible,
                    tr: sprite.tr as i64,
                    alpha: sprite.alpha as i64,
                    bind: format!("L{}:S{}", layer_id, sprite_id),
                    patno: vm
                        .ctx
                        .gfx
                        .object_peek_patno(stage_idx, obj_idx as i64)
                        .unwrap_or(0),
                    runtime_image_id: runtime_image_id.clone(),
                    image_id: runtime_image_id.clone(),
                    width,
                    height,
                    source_label,
                    source_kind: if runtime_image_id.is_some() {
                        "runtime-bind".to_string()
                    } else {
                        "runtime-probe".to_string()
                    },
                });
            }
        }
    }

    fn hud_debug_rgba_preview(rgba: &[u8], width: u32, height: u32) -> (ColorImage, u64) {
        let pixel_count = width as usize * height as usize;
        let mut out = Vec::with_capacity(pixel_count.saturating_mul(4));
        let mut hash = 0xcbf29ce484222325u64;
        for (i, px) in rgba.as_chunks::<4>().0.iter().take(pixel_count).enumerate() {
            let r = px[0];
            let g = px[1];
            let b = px[2];
            let a = px[3];
            hash ^= ((r as u64) << 24)
                ^ ((g as u64) << 16)
                ^ ((b as u64) << 8)
                ^ (a as u64)
                ^ (i as u64);
            hash = hash.wrapping_mul(0x100000001b3);
            out.extend_from_slice(&[r, g, b, 255]);
        }
        (
            ColorImage::from_rgba_unmultiplied([width as usize, height as usize], out.as_slice()),
            hash,
        )
    }

    fn hud_alpha_summary_rgba(rgba: &[u8]) -> (u8, u8, usize) {
        let mut min_a = u8::MAX;
        let mut max_a = 0u8;
        let mut nonzero = 0usize;
        for px in rgba.as_chunks::<4>().0 {
            let a = px[3];
            min_a = min_a.min(a);
            max_a = max_a.max(a);
            if a != 0 {
                nonzero += 1;
            }
        }
        if rgba.is_empty() {
            min_a = 0;
        }
        (min_a, max_a, nonzero)
    }

    fn sync_hud_gpu_texture(
        gui: &mut HudGui,
        texture: &RendererDebugTexture,
    ) -> Option<egui::TextureId> {
        if texture.width == 0 || texture.height == 0 || texture.rgba.is_empty() {
            return None;
        }
        let (color, debug_hash) =
            Self::hud_debug_rgba_preview(texture.rgba.as_slice(), texture.width, texture.height);
        if let Some(entry) = gui.gpu_texture_cache.get_mut(&texture.key) {
            if entry.version != texture.version
                || entry.width != texture.width
                || entry.height != texture.height
                || entry.debug_hash != debug_hash
            {
                entry.handle.set(color, TextureOptions::LINEAR);
                entry.version = texture.version;
                entry.width = texture.width;
                entry.height = texture.height;
                entry.debug_hash = debug_hash;
            }
            return Some(entry.handle.id());
        }
        let handle = gui.ctx.load_texture(
            format!("siglus-hud-renderer-gpu-texture-{}", texture.key),
            color,
            TextureOptions::LINEAR,
        );
        let id = handle.id();
        gui.gpu_texture_cache.insert(
            texture.key.clone(),
            HudTextureCacheEntry {
                version: texture.version,
                handle,
                width: texture.width,
                height: texture.height,
                debug_hash,
            },
        );
        Some(id)
    }

    fn render_hud_egui(&mut self) -> Result<()> {
        let Some(mut hud) = self.hud.take() else {
            return Ok(());
        };

        let result = (|| -> Result<()> {
            let size = hud.window.surface_size();
            if size.width == 0 || size.height == 0 {
                return Ok(());
            }
            let scale = hud.window.scale_factor() as f32;

            // The normal HUD path is metadata-only. GPU pixels are copied back
            // only after an explicit F3/button snapshot request.
            let refresh_previews = std::mem::take(&mut hud.preview_refresh_requested);
            let (textures, renderer_memory) = {
                let Some(renderer) = self.renderer.as_ref() else {
                    return Ok(());
                };
                let renderer = renderer.borrow();
                let textures = if refresh_previews {
                    renderer.debug_read_render_chain_textures()?
                } else {
                    renderer.debug_render_chain_texture_metadata()
                };
                (textures, renderer.debug_memory_stats())
            };

            let mut memory = HudMemorySnapshot {
                renderer_image_gpu_bytes: renderer_memory.image_texture_bytes,
                renderer_external_gpu_bytes: renderer_memory.external_texture_bytes,
                renderer_target_gpu_bytes: renderer_memory.internal_color_target_bytes,
                renderer_depth_gpu_bytes: renderer_memory.internal_depth_target_bytes,
                renderer_buffer_gpu_bytes: renderer_memory.renderer_gpu_buffer_bytes,
                renderer_frame_arena_bytes: renderer_memory.frame_arena_capacity_bytes,
                renderer_image_textures: renderer_memory.image_texture_count,
                renderer_external_textures: renderer_memory.external_texture_count,
                renderer_cached_pipelines: renderer_memory.cached_render_pipeline_count,
                hud_readback_bytes: textures.iter().map(|texture| texture.rgba.len()).sum(),
                ..HudMemorySnapshot::default()
            };

            let (image_origins, runtime_image_sources, stage_objects) =
                if let Some(vm) = self.vm.as_ref() {
                    let scene_memory = vm.debug_scene_memory_stats();
                    let movie_memory = vm.ctx.movie.debug_memory_stats();
                    let (koe_cache_bytes, koe_cache_entries) = vm.ctx.koe.debug_cache_memory();
                    let (bgm_source_bytes, bgm_source_slots) = vm.ctx.bgm.debug_source_memory();
                    memory.images_cpu_bytes = vm.ctx.images.resident_bytes();
                    memory.scene_pck_bytes = scene_memory.scene_pck_bytes;
                    memory.scene_streams = scene_memory.cached_scene_streams;
                    memory.movie_video_bytes = movie_memory.video_bytes;
                    memory.movie_audio_bytes = movie_memory.audio_pcm_bytes;
                    memory.movie_frames = movie_memory.video_frames;
                    memory.movie_assets = movie_memory.asset_cache_entries;
                    memory.movie_previews = movie_memory.preview_cache_entries;
                    memory.movie_streams = movie_memory.active_streams;
                    memory.koe_cache_bytes = koe_cache_bytes;
                    memory.koe_cache_entries = koe_cache_entries;
                    memory.bgm_source_bytes = bgm_source_bytes;
                    memory.bgm_source_slots = bgm_source_slots;
                    (
                        Self::collect_hud_image_origins(vm, &textures),
                        Self::collect_hud_runtime_image_sources(vm),
                        Self::collect_hud_object_metadata(vm),
                    )
                } else {
                    (HashMap::new(), HashMap::new(), Vec::new())
                };

            // Only keep snapshots for textures that still exist. Merely opening
            // or scrolling the HUD must never duplicate all game textures.
            hud.gui
                .gpu_texture_cache
                .retain(|key, _| textures.iter().any(|texture| &texture.key == key));
            if refresh_previews {
                for texture in &textures {
                    let _ = Self::sync_hud_gpu_texture(&mut hud.gui, texture);
                }
            }
            memory.hud_preview_bytes = hud
                .gui
                .gpu_texture_cache
                .values()
                .map(|entry| entry.width as usize * entry.height as usize * 4)
                .sum();

            hud.gui.ctx.set_pixels_per_point(scale);
            hud.gui.raw_input.screen_rect = Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(size.width as f32 / scale, size.height as f32 / scale),
            ));
            hud.gui.raw_input.time = Some(hud.gui.start_time.elapsed().as_secs_f64());
            let raw_input = hud.gui.raw_input.take();
            let ctx = hud.gui.ctx.clone();

            let image_count = textures.iter().filter(|t| t.kind == "image").count();
            let external_count = textures.iter().filter(|t| t.kind == "external").count();
            let target_count = textures
                .iter()
                .filter(|t| t.kind == "render-target")
                .count();
            let usage_total: usize = textures.iter().map(|t| t.usage_count).sum();

            let output = ctx.run(raw_input, |ctx| {
                egui::TopBottomPanel::top("hud_toolbar").show(ctx, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.heading("Siglus HUD");
                        ui.separator();
                        if ui.button("Refresh stats").clicked() {
                            hud.process_memory = read_process_memory_snapshot();
                        }
                        if ui.button("Snapshot GPU textures").clicked() {
                            hud.preview_refresh_requested = true;
                            hud.window.request_redraw();
                        }
                        if ui.button("Clear snapshots").clicked() {
                            hud.gui.gpu_texture_cache.clear();
                        }
                        ui.separator();
                        ui.checkbox(&mut hud.show_memory, "Memory");
                        ui.checkbox(&mut hud.show_objects, "Objects");
                        ui.checkbox(&mut hud.show_textures, "Textures");
                        ui.separator();
                        ui.label("F2 close · F3 snapshot");
                    });
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Card width");
                        ui.add(egui::Slider::new(&mut hud.card_width, 240.0..=640.0).suffix(" pt"));
                        ui.label("Preview height");
                        ui.add(
                            egui::Slider::new(&mut hud.preview_height, 100.0..=480.0)
                                .suffix(" pt"),
                        );
                        ui.label("Object list height");
                        ui.add(
                            egui::Slider::new(&mut hud.object_list_height, 80.0..=500.0)
                                .suffix(" pt"),
                        );
                    });
                });

                if hud.show_memory {
                    egui::SidePanel::left("hud_memory")
                        .resizable(true)
                        .default_width(340.0)
                        .min_width(260.0)
                        .show(ctx, |ui| {
                            ui.heading("Memory");
                            if let Some((kind, bytes)) = hud.process_memory.primary() {
                                ui.monospace(format!(
                                    "Process {kind}: {}",
                                    Self::hud_format_bytes(bytes)
                                ));
                            } else {
                                ui.monospace("Process memory: unavailable");
                            }
                            if let Some((kind, before)) = hud.process_before_open.primary() {
                                ui.monospace(format!(
                                    "Before HUD {kind}: {}",
                                    Self::hud_format_bytes(before)
                                ));
                                if let Some((_, now)) = hud.process_memory.primary() {
                                    ui.monospace(format!(
                                        "HUD/open-time delta: {}",
                                        Self::hud_format_byte_delta(now, before)
                                    ));
                                }
                            }
                            if let Some(bytes) = hud.process_memory.resident_bytes {
                                ui.monospace(format!(
                                    "Resident / working set: {}",
                                    Self::hud_format_bytes(bytes)
                                ));
                            }
                            if let Some(bytes) = hud.process_memory.private_bytes {
                                ui.monospace(format!(
                                    "Private commit: {}",
                                    Self::hud_format_bytes(bytes)
                                ));
                            }
                            if let Some(bytes) = hud.process_memory.virtual_bytes {
                                ui.monospace(format!(
                                    "Virtual size: {}",
                                    Self::hud_format_bytes(bytes)
                                ));
                            }
                            ui.separator();
                            ui.monospace(format!(
                                "Known engine payloads: {}",
                                Self::hud_format_bytes(memory.tracked_engine_bytes())
                            ));
                            ui.small("Diagnostic payload sum only; it is not the OS process total.");
                            ui.separator();
                            egui::Grid::new("hud_memory_grid")
                                .num_columns(2)
                                .striped(true)
                                .show(ui, |ui| {
                                    let mut row = |name: &str, value: String| {
                                        ui.label(name);
                                        ui.monospace(value);
                                        ui.end_row();
                                    };
                                    row(
                                        "CPU images",
                                        Self::hud_format_bytes(memory.images_cpu_bytes as u64),
                                    );
                                    row(
                                        "GPU images",
                                        format!(
                                            "{} / {} textures",
                                            Self::hud_format_bytes(memory.renderer_image_gpu_bytes),
                                            memory.renderer_image_textures,
                                        ),
                                    );
                                    row(
                                        "GPU external",
                                        format!(
                                            "{} / {} textures",
                                            Self::hud_format_bytes(memory.renderer_external_gpu_bytes),
                                            memory.renderer_external_textures,
                                        ),
                                    );
                                    row(
                                        "GPU color targets",
                                        Self::hud_format_bytes(memory.renderer_target_gpu_bytes),
                                    );
                                    row(
                                        "GPU depth targets",
                                        Self::hud_format_bytes(memory.renderer_depth_gpu_bytes),
                                    );
                                    row(
                                        "Renderer buffers",
                                        Self::hud_format_bytes(memory.renderer_buffer_gpu_bytes),
                                    );
                                    row(
                                        "Cached pipelines",
                                        memory.renderer_cached_pipelines.to_string(),
                                    );
                                    row(
                                        "Frame arenas",
                                        Self::hud_format_bytes(memory.renderer_frame_arena_bytes as u64),
                                    );
                                    row(
                                        "Scene.pck",
                                        format!(
                                            "{} / {} streams",
                                            Self::hud_format_bytes(memory.scene_pck_bytes as u64),
                                            memory.scene_streams,
                                        ),
                                    );
                                    row(
                                        "BGM source bytes",
                                        format!(
                                            "{} / {} slots",
                                            Self::hud_format_bytes(memory.bgm_source_bytes as u64),
                                            memory.bgm_source_slots,
                                        ),
                                    );
                                    row(
                                        "Movie cache",
                                        format!(
                                            "video {} + PCM {}",
                                            Self::hud_format_bytes(memory.movie_video_bytes as u64),
                                            Self::hud_format_bytes(memory.movie_audio_bytes as u64),
                                        ),
                                    );
                                    row(
                                        "KOE cache",
                                        format!(
                                            "{} / {} entries",
                                            Self::hud_format_bytes(memory.koe_cache_bytes as u64),
                                            memory.koe_cache_entries,
                                        ),
                                    );
                                    row(
                                        "HUD snapshots",
                                        Self::hud_format_bytes(memory.hud_preview_bytes as u64),
                                    );
                                });
                        });
                }

                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if hud.show_objects {
                                egui::CollapsingHeader::new(format!(
                                    "Stage objects ({})",
                                    stage_objects.len(),
                                ))
                                .default_open(true)
                                .show(ui, |ui| {
                                    egui::ScrollArea::vertical()
                                        .id_source("hud_objects_scroll")
                                        .max_height(hud.object_list_height)
                                        .show(ui, |ui| {
                                            for tile in &stage_objects {
                                                let image = tile
                                                    .runtime_image_id
                                                    .as_ref()
                                                    .map(|id| format!("ImageHandle({})", id.index()))
                                                    .unwrap_or_else(|| "-".to_string());
                                                let line = format!(
                                                    "{}[{}] disp={} backend={} file={} patno={} bind={} image={} tr={} alpha={}",
                                                    tile.stage_label,
                                                    tile.obj_idx,
                                                    if tile.disp { 1 } else { 0 },
                                                    tile.backend,
                                                    tile.file,
                                                    tile.patno,
                                                    tile.bind,
                                                    image,
                                                    tile.tr,
                                                    tile.alpha,
                                                );
                                                ui.monospace(Self::shorten_for_hud(&line, 220))
                                                    .on_hover_text(line);
                                            }
                                        });
                                });
                                ui.separator();
                            }

                            if !hud.show_textures {
                                return;
                            }

                            ui.horizontal_wrapped(|ui| {
                                ui.heading("Renderer textures");
                                ui.separator();
                                ui.label(format!(
                                    "{} textures · {} usages · {} images · {} external · {} targets",
                                    textures.len(),
                                    usage_total,
                                    image_count,
                                    external_count,
                                    target_count,
                                ));
                                if memory.hud_readback_bytes > 0 {
                                    ui.label(format!(
                                        "snapshot {}",
                                        Self::hud_format_bytes(memory.hud_readback_bytes as u64)
                                    ));
                                }
                            });

                            if textures.is_empty() {
                                ui.label("No renderer GPU textures recorded for the current render chain.");
                                return;
                            }

                            let spacing = ui.spacing().item_spacing.x.max(4.0);
                            let available = ui.available_width().max(hud.card_width);
                            let columns = ((available + spacing) / (hud.card_width + spacing))
                                .floor()
                                .max(1.0) as usize;

                            for row_textures in textures.chunks(columns) {
                                ui.horizontal_top(|ui| {
                                    for texture in row_textures {
                                        let tex_id = hud
                                            .gui
                                            .gpu_texture_cache
                                            .get(&texture.key)
                                            .map(|entry| entry.handle.id());
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(
                                                hud.card_width,
                                                hud.preview_height + 150.0,
                                            ),
                                            egui::Layout::top_down(egui::Align::Min),
                                            |ui| {
                                                egui::Frame::group(ui.style()).show(ui, |ui| {
                                                    ui.set_min_width(hud.card_width - 8.0);
                                                    ui.set_max_width(hud.card_width - 8.0);
                                                    ui.label(
                                                        egui::RichText::new(format!(
                                                            "{}  {}",
                                                            texture.kind,
                                                            Self::shorten_for_hud(&texture.label, 42),
                                                        ))
                                                        .strong()
                                                        .monospace(),
                                                    );
                                                    ui.small(format!(
                                                        "{}x{} · ver={} · usages={}",
                                                        texture.width,
                                                        texture.height,
                                                        texture.version,
                                                        texture.usage_count,
                                                    ));
                                                    ui.small(Self::shorten_for_hud(&texture.key, 72))
                                                        .on_hover_text(texture.key.clone());

                                                    if let Some(image_id) = Self::hud_renderer_image_id(texture) {
                                                        if let Some(lines) = image_origins.get(&image_id) {
                                                            for line in lines {
                                                                ui.small(Self::shorten_for_hud(line, 100))
                                                                    .on_hover_text(line);
                                                            }
                                                        }
                                                        if let Some(lines) = runtime_image_sources.get(&image_id) {
                                                            for line in lines.iter().take(3) {
                                                                ui.small(Self::shorten_for_hud(line, 100))
                                                                    .on_hover_text(line);
                                                            }
                                                            if lines.len() > 3 {
                                                                ui.small(format!(
                                                                    "+{} more object bindings",
                                                                    lines.len() - 3
                                                                ));
                                                            }
                                                        }
                                                    }

                                                    let preview_size = egui::vec2(
                                                        (hud.card_width - 20.0).max(80.0),
                                                        hud.preview_height,
                                                    );
                                                    let (rect, _) = ui.allocate_exact_size(
                                                        preview_size,
                                                        egui::Sense::hover(),
                                                    );
                                                    ui.painter().rect_filled(
                                                        rect,
                                                        4.0,
                                                        egui::Color32::from_gray(24),
                                                    );
                                                    if let Some(tex_id) = tex_id {
                                                        let mut draw_w = preview_size.x;
                                                        let mut draw_h = preview_size.y;
                                                        if texture.width > 0 && texture.height > 0 {
                                                            let sx = preview_size.x / texture.width as f32;
                                                            let sy = preview_size.y / texture.height as f32;
                                                            let s = sx.min(sy).max(0.01);
                                                            draw_w = texture.width as f32 * s;
                                                            draw_h = texture.height as f32 * s;
                                                        }
                                                        let image_rect = egui::Rect::from_center_size(
                                                            rect.center(),
                                                            egui::vec2(draw_w, draw_h),
                                                        );
                                                        ui.put(
                                                            image_rect,
                                                            egui::Image::new((
                                                                tex_id,
                                                                egui::vec2(draw_w, draw_h),
                                                            )),
                                                        );
                                                    } else {
                                                        ui.painter().text(
                                                            rect.center(),
                                                            egui::Align2::CENTER_CENTER,
                                                            "F3 / Snapshot GPU textures",
                                                            egui::FontId::proportional(14.0),
                                                            egui::Color32::LIGHT_GRAY,
                                                        );
                                                    }
                                                    ui.small(Self::shorten_for_hud(&texture.usage, 120));
                                                });
                                            },
                                        );
                                    }
                                });
                            }
                        });
                });
            });

            let screen_desc = ScreenDescriptor {
                size_in_pixels: [size.width, size.height],
                pixels_per_point: scale,
            };
            let paint_jobs = ctx.tessellate(output.shapes, scale);

            let Some(main_renderer) = self.renderer.as_ref() else {
                return Ok(());
            };
            let main_renderer = main_renderer.borrow();
            for (id, delta) in &output.textures_delta.set {
                hud.gui.renderer.update_texture(
                    &main_renderer.device,
                    &main_renderer.queue,
                    *id,
                    delta,
                );
            }

            let frame = match hud.surface.get_current_texture() {
                Ok(frame) => frame,
                Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                    hud.surface.configure(&main_renderer.device, &hud.config);
                    return Ok(());
                }
                Err(wgpu::SurfaceError::OutOfMemory) => anyhow::bail!("hud surface out of memory"),
                Err(wgpu::SurfaceError::Timeout) => return Ok(()),
            };
            let view = frame
                .texture
                .create_view(&wgpu::TextureViewDescriptor::default());
            let mut encoder =
                main_renderer
                    .device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                        label: Some("hud_egui_encoder"),
                    });
            hud.gui.renderer.update_buffers(
                &main_renderer.device,
                &main_renderer.queue,
                &mut encoder,
                &paint_jobs,
                &screen_desc,
            );
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("hud_egui_pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.06,
                                g: 0.06,
                                b: 0.07,
                                a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                hud.gui
                    .renderer
                    .render(&mut pass, &paint_jobs, &screen_desc);
            }
            main_renderer.queue.submit(Some(encoder.finish()));
            frame.present();
            for id in output.textures_delta.free {
                hud.gui.renderer.free_texture(&id);
            }
            Ok(())
        })();

        self.hud = Some(hud);
        result
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
        // C_tnm_ini::C_tnm_ini() defaults MENU_SCENE to "_menu" and only
        // overwrites it when Gameexe provides #MENU_SCENE.
        let (menu_scene, menu_z) = cfg
            .as_ref()
            .and_then(|cfg| Self::gameexe_scene_entry(cfg, "MENU_SCENE"))
            .unwrap_or_else(|| ("_menu".to_string(), 0));
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

    fn resolve_game_size(args: &Args) -> (u32, u32) {
        Self::resolve_project_dir(args)
            .as_deref()
            .and_then(Self::try_load_gameexe)
            .as_ref()
            .and_then(Self::gameexe_screen_size)
            .unwrap_or((1280, 720))
    }

    fn write_rgba_png(path: &Path, rgba: &[u8], width: u32, height: u32) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create capture dir: {}", parent.display()))?;
        }
        image::save_buffer(path, rgba, width, height, ColorType::Rgba8)
            .with_context(|| format!("write capture png: {}", path.display()))
    }
    fn try_load_gameexe(project_dir: &Path) -> Option<GameexeConfig> {
        let path = siglus_scene_vm::resource::find_initial_gameexe_path(project_dir).ok()?;
        let raw = siglus_scene_vm::resource::read_file_bytes(&path).ok()?;
        if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("ini"))
        {
            let text = String::from_utf8(raw).ok()?;
            return Some(GameexeConfig::from_text(&text));
        }
        let opt = siglus_scene_vm::resource::load_gameexe_decode_options(project_dir).ok()?;
        let (text, _report) = decode_gameexe_dat_bytes(&raw, &opt).ok()?;
        Some(GameexeConfig::from_text(&text))
    }

    fn init_vm(&self) -> Result<SceneVm<'static>> {
        let project_dir = self
            .args
            .project_dir
            .clone()
            .unwrap_or(siglus_scene_vm::app_path::resolve_app_base_path()?);
        let scene_pck_path = siglus_scene_vm::resource::find_scene_pck_path(&project_dir)?;
        let opt = siglus_scene_vm::resource::load_scene_pck_decode_options(&project_dir)?;
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
        let owner: std::sync::Arc<[u8]> = std::sync::Arc::from(chunk.to_vec().into_boxed_slice());
        let mut stream = SceneStream::new_owned_with_string_codec(owner, pck.string_codec)?;
        let start_z = if self.args.scene_id.is_some() || self.args.scene_name.is_some() {
            0
        } else {
            self.boot.start_z
        };
        stream.jump_to_z_label(start_z.max(0) as usize)?;
        let mut ctx = CommandContext::new(project_dir);
        let active_append = ctx.globals.append_dir.clone();
        ctx.install_scene_metadata(&active_append, &pck)?;
        ctx.screen_w = self.game_size.0;
        ctx.screen_h = self.game_size.1;
        let mut vm = SceneVm::with_config(VmConfig::from_env(), stream, ctx);
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        vm.ctx
            .set_native_ui_backend(Some(self.desktop_messagebox_bridge.backend()));
        // Original init_global() loads global/read/config state before start()
        // initializes local scene state.
        if self.args.scene_id.is_none() && self.args.scene_name.is_none() {
            siglus_scene_vm::runtime::forms::syscom::load_global_save(&mut vm.ctx)
                .context("load global save during engine initialization")?;
        }
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

    fn suspend_wait_for_syscom_excall(&mut self, key: &str) {
        let Some(vm) = self.vm.as_mut() else {
            return;
        };
        let flow_depth = self.flow.stack.len();
        let saved_wait = std::mem::take(&mut vm.ctx.wait);
        vm.ctx.input.use_current();
        vm.ctx.script_input.use_current();
        self.syscom_suspended_waits
            .push((flow_depth, saved_wait, key.to_string()));
        if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
            eprintln!(
                "[SG_PROC_FLOW] suspend_wait_for_syscom_excall key={} flow_depth={} saved_count={} scene={:?} line={}",
                key,
                flow_depth,
                self.syscom_suspended_waits.len(),
                vm.current_scene_name(),
                vm.current_line_no()
            );
        }
    }

    fn restore_wait_after_syscom_excall(&mut self, popped_depth: usize) {
        let should_restore = self
            .syscom_suspended_waits
            .last()
            .map(|(depth, _, _)| *depth == popped_depth)
            .unwrap_or(false);
        if !should_restore {
            return;
        }
        let Some((_depth, saved_wait, key)) = self.syscom_suspended_waits.pop() else {
            return;
        };
        let Some(vm) = self.vm.as_mut() else {
            return;
        };
        vm.ctx.wait = saved_wait;
        vm.ctx.input.clear_all();
        vm.ctx.script_input.clear_all();
        if key == "SAVE_SCENE" {
            syscom::free_runtime_save_thumb_capture(&mut vm.ctx, syscom::CAPTURE_PRIOR_SAVE);
        }
        if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
            eprintln!(
                "[SG_PROC_FLOW] restore_wait_after_syscom_excall popped_depth={} remaining={} scene={:?} line={}",
                popped_depth,
                self.syscom_suspended_waits.len(),
                vm.current_scene_name(),
                vm.current_line_no()
            );
        }
    }

    fn consume_syscom_pending_proc(&mut self) -> Result<bool> {
        let Some(proc) = ({
            let Some(vm) = self.vm.as_mut() else {
                return Ok(false);
            };
            let proc = vm.ctx.globals.syscom.pending_proc.take();
            if let Some(p) = proc.as_ref() {
                vm.ctx.globals.syscom.menu_open = false;
                vm.ctx.globals.syscom.menu_kind = None;
                if p.kind != SyscomPendingProcKind::MsgBack {
                    vm.ctx.globals.syscom.msg_back_open = false;
                }
            }
            proc
        }) else {
            return Ok(false);
        };

        if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
            let scene = self
                .vm
                .as_ref()
                .and_then(|vm| vm.current_scene_name())
                .unwrap_or("<none>");
            let line = self
                .vm
                .as_ref()
                .map(|vm| vm.current_line_no())
                .unwrap_or(-1);
            eprintln!(
                "[SG_PROC_FLOW] consume_syscom_pending kind={:?} before scene={} line={} flow={:?}",
                proc.kind, scene, line, self.flow.stack
            );
        }

        match proc.kind {
            SyscomPendingProcKind::EndGame => {
                if proc.warning {
                    self.begin_syscom_warning(proc);
                } else {
                    self.queue_end_game_proc(proc);
                }
                Ok(true)
            }
            SyscomPendingProcKind::ReturnToMenu => {
                if proc.warning {
                    self.begin_syscom_warning(proc);
                } else {
                    self.queue_return_to_menu_proc(proc);
                }
                Ok(true)
            }
            SyscomPendingProcKind::RestartScene => {
                if proc.warning {
                    self.begin_syscom_warning(proc);
                } else {
                    self.perform_restart_from_scene()?;
                }
                Ok(true)
            }
            SyscomPendingProcKind::Save => {
                if proc.warning {
                    self.begin_syscom_warning(proc);
                } else {
                    let Some(vm) = self.vm.as_mut() else {
                        return Ok(false);
                    };
                    syscom::menu_save_slot(&mut vm.ctx, false, proc.save_id.max(0) as usize);
                    syscom::write_global_save(&vm.ctx);
                }
                Ok(true)
            }
            SyscomPendingProcKind::Load => {
                if proc.warning {
                    self.begin_syscom_warning(proc);
                } else {
                    let Some(vm) = self.vm.as_mut() else {
                        return Ok(false);
                    };
                    syscom::menu_load_slot(&mut vm.ctx, false, proc.save_id.max(0) as usize);
                }
                Ok(true)
            }
            SyscomPendingProcKind::QuickSave => {
                if proc.warning {
                    self.begin_syscom_warning(proc);
                } else {
                    let Some(vm) = self.vm.as_mut() else {
                        return Ok(false);
                    };
                    syscom::menu_save_slot(&mut vm.ctx, true, proc.save_id.max(0) as usize);
                    syscom::write_global_save(&vm.ctx);
                }
                Ok(true)
            }
            SyscomPendingProcKind::QuickLoad => {
                if proc.warning {
                    self.begin_syscom_warning(proc);
                } else {
                    let Some(vm) = self.vm.as_mut() else {
                        return Ok(false);
                    };
                    syscom::menu_load_slot(&mut vm.ctx, true, proc.save_id.max(0) as usize);
                }
                Ok(true)
            }
            SyscomPendingProcKind::ReturnToSel => {
                let Some(vm) = self.vm.as_mut() else {
                    return Ok(false);
                };
                if vm.restore_last_sel_point() {
                    self.flow.stack.clear();
                    self.flow.push(ProcType::GameTimerStart, 0);
                    self.flow.push(ProcType::Script, 0);
                    Ok(true)
                } else {
                    vm.ctx.unknown.record_note(
                        "SYSCOM.RETURN_TO_SEL requested without an in-memory SELPOINT snapshot",
                    );
                    Ok(false)
                }
            }
            SyscomPendingProcKind::BacklogLoad => {
                let Some(vm) = self.vm.as_mut() else {
                    return Ok(false);
                };
                if vm.restore_last_sel_point() {
                    self.flow.stack.clear();
                    self.flow.push(ProcType::GameTimerStart, 0);
                    self.flow.push(ProcType::Script, 0);
                    Ok(true)
                } else {
                    vm.ctx.unknown.record_note(&format!(
                        "SYSCOM.MSG_BACK_LOAD requested but backlog save {} is not materialized without SAVE/LOAD support",
                        proc.save_id
                    ));
                    Ok(false)
                }
            }
            SyscomPendingProcKind::MsgBack => {
                let open = self
                    .vm
                    .as_ref()
                    .map(|vm| vm.ctx.globals.syscom.msg_back_open)
                    .unwrap_or(false);
                if open {
                    self.flow.push(ProcType::MsgBack, 0);
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            SyscomPendingProcKind::OpenSyscomMenu => {
                let opened = {
                    let Some(vm) = self.vm.as_mut() else {
                        return Ok(false);
                    };
                    vm.call_syscom_configured_scene("CANCEL_SCENE")?
                };
                if opened {
                    self.ensure_requested_script_proc();
                    self.suspend_wait_for_syscom_excall("CANCEL_SCENE");
                    Ok(true)
                } else {
                    let Some(vm) = self.vm.as_mut() else {
                        return Ok(false);
                    };
                    syscom::open_fallback_dialog(
                        &mut vm.ctx,
                        SyscomPendingProcKind::OpenSyscomMenu,
                    );
                    Ok(true)
                }
            }
            SyscomPendingProcKind::OpenSave => {
                let opened = {
                    let Some(vm) = self.vm.as_mut() else {
                        return Ok(false);
                    };
                    vm.call_syscom_configured_scene("SAVE_SCENE")?
                };
                if opened {
                    self.ensure_requested_script_proc();
                    self.suspend_wait_for_syscom_excall("SAVE_SCENE");
                    Ok(true)
                } else {
                    let Some(vm) = self.vm.as_mut() else {
                        return Ok(false);
                    };
                    syscom::open_fallback_dialog(&mut vm.ctx, SyscomPendingProcKind::OpenSave);
                    Ok(true)
                }
            }
            SyscomPendingProcKind::OpenLoad => {
                let opened = {
                    let Some(vm) = self.vm.as_mut() else {
                        return Ok(false);
                    };
                    vm.call_syscom_configured_scene("LOAD_SCENE")?
                };
                if opened {
                    self.ensure_requested_script_proc();
                    self.suspend_wait_for_syscom_excall("LOAD_SCENE");
                    Ok(true)
                } else {
                    let Some(vm) = self.vm.as_mut() else {
                        return Ok(false);
                    };
                    syscom::open_fallback_dialog(&mut vm.ctx, SyscomPendingProcKind::OpenLoad);
                    Ok(true)
                }
            }
            SyscomPendingProcKind::OpenConfig | SyscomPendingProcKind::OpenConfigDialog => {
                let opened = {
                    let Some(vm) = self.vm.as_mut() else {
                        return Ok(false);
                    };
                    proc.kind == SyscomPendingProcKind::OpenConfig
                        && vm.call_syscom_configured_scene("CONFIG_SCENE")?
                };
                if opened {
                    self.ensure_requested_script_proc();
                    self.suspend_wait_for_syscom_excall("CONFIG_SCENE");
                    Ok(true)
                } else {
                    let Some(vm) = self.vm.as_mut() else {
                        return Ok(false);
                    };
                    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
                    {
                        self.desktop_config_request = Some(ConfigDialog::new(&vm.ctx));
                        self.desktop_config_open = true;
                    }
                    #[cfg(not(any(
                        target_os = "macos",
                        target_os = "windows",
                        target_os = "linux"
                    )))]
                    syscom::open_fallback_dialog(&mut vm.ctx, SyscomPendingProcKind::OpenConfig);
                    Ok(true)
                }
            }
        }
    }

    fn ensure_requested_script_proc(&mut self) {
        let requested = self
            .vm
            .as_mut()
            .map(|vm| vm.take_script_proc_request())
            .unwrap_or(false);
        if requested {
            if std::env::var_os("SG_DEBUG").is_some() {
                eprintln!("[SG_DEBUG][EXCALL] push SCRIPT proc requested by button/frame action");
            }
            if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
                let scene = self
                    .vm
                    .as_ref()
                    .and_then(|vm| vm.current_scene_name())
                    .unwrap_or("<none>");
                let line = self
                    .vm
                    .as_ref()
                    .map(|vm| vm.current_line_no())
                    .unwrap_or(-1);
                eprintln!(
                    "[SG_PROC_FLOW] ensure_requested_script_proc push before scene={} line={} flow={:?}",
                    scene, line, self.flow.stack
                );
            }
            self.flow.push(ProcType::Script, 0);
            if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
                eprintln!(
                    "[SG_PROC_FLOW] ensure_requested_script_proc push after flow={:?}",
                    self.flow.stack
                );
            }
        }
    }

    fn begin_syscom_warning(&mut self, mut proc: SyscomPendingProc) {
        let Some(vm) = self.vm.as_mut() else {
            return;
        };
        let kind = proc.kind;
        proc.warning = false;
        self.flow.pending_syscom_proc = Some(proc);
        vm.ctx.globals.system.messagebox_modal_result = None;
        let request_id = vm.ctx.native_ui.next_messagebox_request_id();
        let buttons = vec![
            SystemMessageBoxButton {
                label: "YES".to_string(),
                value: 0,
            },
            SystemMessageBoxButton {
                label: "NO".to_string(),
                value: 1,
            },
        ];
        let text = Self::syscom_warning_text(vm, kind);
        let title = vm.ctx.game_title();
        if let Some(backend) = vm.ctx.native_ui_backend.as_ref() {
            vm.ctx.globals.system.messagebox_modal = Some(SystemMessageBoxModalState {
                request_id,
                kind: 19,
                text: text.clone(),
                debug_only: false,
                buttons: buttons.clone(),
                cursor: 1,
                native_pending: true,
                complete_wait_with_value: false,
            });
            backend.show_system_messagebox(native_ui::NativeMessageBoxRequest {
                request_id,
                kind: native_ui::NativeMessageBoxKind::YesNo,
                title,
                message: text,
                buttons: buttons
                    .into_iter()
                    .map(|button| native_ui::NativeMessageBoxButton {
                        label: button.label,
                        value: button.value,
                    })
                    .collect(),
                debug_only: false,
            });
        } else {
            vm.ctx.globals.system.messagebox_modal = Some(SystemMessageBoxModalState {
                request_id,
                kind: 19,
                text,
                debug_only: false,
                buttons,
                cursor: 1,
                native_pending: false,
                complete_wait_with_value: false,
            });
        }
        self.flow.push(ProcType::SyscomWarning, 0);
    }

    fn syscom_warning_text(vm: &SceneVm<'static>, kind: SyscomPendingProcKind) -> String {
        let keys: &[&str] = match kind {
            SyscomPendingProcKind::EndGame => &[
                "#WARNINGINFO.GAMEEND_WARNING_STR",
                "WARNINGINFO.GAMEEND_WARNING_STR",
            ],
            SyscomPendingProcKind::ReturnToSel => &[
                "#WARNINGINFO.RETURNSEL_WARNING_STR",
                "WARNINGINFO.RETURNSEL_WARNING_STR",
                "#WARNINGINFO.RETURNMENU_WARNING_STR",
                "WARNINGINFO.RETURNMENU_WARNING_STR",
            ],
            SyscomPendingProcKind::RestartScene => &[
                "#WARNINGINFO.SCENESTART_WARNING_STR",
                "WARNINGINFO.SCENESTART_WARNING_STR",
            ],
            SyscomPendingProcKind::Save | SyscomPendingProcKind::QuickSave => &[
                "#WARNINGINFO.SAVE_WARNING_STR",
                "WARNINGINFO.SAVE_WARNING_STR",
            ],
            SyscomPendingProcKind::Load | SyscomPendingProcKind::QuickLoad => &[
                "#WARNINGINFO.LOAD_WARNING_STR",
                "WARNINGINFO.LOAD_WARNING_STR",
            ],
            _ => &[
                "#WARNINGINFO.RETURNMENU_WARNING_STR",
                "WARNINGINFO.RETURNMENU_WARNING_STR",
            ],
        };
        let default = match kind {
            SyscomPendingProcKind::EndGame => "終了してもよろしいですか？",
            SyscomPendingProcKind::ReturnToSel => "前の選択肢に戻ってもよろしいですか？",
            SyscomPendingProcKind::RestartScene => "途中から始めてもよろしいですか？",
            SyscomPendingProcKind::Save | SyscomPendingProcKind::QuickSave => {
                "セーブデータを上書きしてもよろしいですか？"
            }
            SyscomPendingProcKind::Load | SyscomPendingProcKind::QuickLoad => {
                "セーブデータをロードしてもよろしいですか？"
            }
            _ => "タイトルに戻ってもよろしいですか？",
        };
        let cfg = vm.ctx.tables.gameexe.as_ref();
        keys.iter()
            .find_map(|key| cfg.and_then(|c| c.get_unquoted(key)))
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| default.to_string())
    }

    fn return_to_menu_warning_text(vm: &SceneVm<'static>) -> String {
        let cfg = vm.ctx.tables.gameexe.as_ref();
        [
            "#WARNINGINFO.RETURNMENU_WARNING_STR",
            "WARNINGINFO.RETURNMENU_WARNING_STR",
        ]
        .iter()
        .find_map(|key| cfg.and_then(|c| c.get_unquoted(key)))
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| "タイトルに戻ってもよろしいですか？".to_string())
    }

    fn load_wipe_params(vm: &SceneVm<'static>) -> (i32, i32) {
        fn parse_pair(raw: &str) -> Option<(i32, i32)> {
            let nums: Vec<i32> = raw
                .split(|c: char| !(c == '-' || c.is_ascii_digit()))
                .filter(|s| !s.is_empty())
                .filter_map(|s| s.parse::<i32>().ok())
                .collect();
            if nums.len() >= 2 {
                Some((nums[0], nums[1]))
            } else {
                None
            }
        }
        let cfg = vm.ctx.tables.gameexe.as_ref();
        for key in ["LOAD.WIPE", "LOAD . WIPE", "#LOAD.WIPE", "#LOAD . WIPE"] {
            if let Some(pair) = cfg.and_then(|c| c.get_value(key)).and_then(parse_pair) {
                return pair;
            }
        }
        (0, 1000)
    }

    fn queue_end_game_proc(&mut self, proc: SyscomPendingProc) {
        self.flow.pending_syscom_proc = None;
        self.flow.push(ProcType::EndGame, 0);
        if proc.fade_out {
            self.flow.push(ProcType::GameEndWipe, 0);
            self.flow.push(ProcType::Disp, 0);
        } else {
            self.flow.push(ProcType::Disp, 0);
        }
    }

    fn queue_return_to_menu_proc(&mut self, proc: SyscomPendingProc) {
        // Original tnm_syscom_return_to_menu() persists global data only after
        // the warning (if any) has been accepted, and before fade/scene return.
        if let Some(vm) = self.vm.as_ref() {
            syscom::write_global_save(&vm.ctx);
        }
        let option = if proc.leave_msgbk { 1 } else { 0 };
        self.flow.pending_syscom_proc = Some(proc.clone());
        self.flow.push(ProcType::ReturnToMenu, option);
        if proc.fade_out {
            self.flow.push(ProcType::GameEndWipe, 0);
            self.flow.push(ProcType::Disp, 0);
        }
    }

    fn start_game_end_wipe(&mut self) {
        let Some(vm) = self.vm.as_mut() else {
            return;
        };
        let (wipe_type, wipe_time) = Self::load_wipe_params(vm);
        if vm.ctx.globals.wipe.is_some() {
            vm.ctx.finish_wipe_runtime();
        }
        let stage_form_id = if vm.ctx.ids.form_global_stage != 0 {
            vm.ctx.ids.form_global_stage
        } else {
            siglus_scene_vm::runtime::forms::codes::FORM_GLOBAL_STAGE
        };
        vm.ctx.globals.start_wipe(WipeState::new(
            stage_form_id,
            None,
            None,
            wipe_type,
            wipe_time,
            0,
            0,
            Vec::new(),
            i32::MIN,
            i32::MAX,
            i32::MIN,
            i32::MAX,
            false,
            0,
            0,
        ));
    }

    /// Called when `vm.take_runtime_load_completed()` returned true: the VM just
    /// replaced its scene/call/state with the loaded snapshot, so any proc-flow
    /// entries the bin had pushed for the save/load menu excall, any suspended
    /// waits, and any cached runtime image textures are now stale. Re-seed the
    /// flow with a single Script proc on top of GameTimerStart so the loaded
    /// scene runs immediately on the next pump.
    fn finish_runtime_load(&mut self) {
        if let Some(renderer) = self.renderer.as_ref() {
            renderer.borrow_mut().clear_runtime_image_textures();
        }
        self.flow.stack.clear();
        self.flow.pending_syscom_proc = None;
        self.syscom_suspended_waits.clear();
        self.paused = false;
        self.script_resume_after_redraw = false;
        if let Some(vm) = self.vm.as_mut() {
            vm.ctx.globals.syscom.pending_proc = None;
            vm.ctx.globals.syscom.menu_open = false;
            vm.ctx.globals.syscom.menu_kind = None;
            vm.ctx.globals.syscom.menu_result = None;
            vm.ctx.globals.syscom.msg_back_open = false;
            vm.ctx.globals.finish_wipe();
        }
        self.flow.push(ProcType::GameTimerStart, 0);
        self.flow.push(ProcType::Script, 0);
        self.script_needs_pump = true;
        self.frame_dirty = true;
    }

    fn perform_return_to_menu(&mut self, leave_msgbk: bool) -> Result<()> {
        let target_scene = self.boot.menu_scene.clone();
        let target_z = self.boot.menu_z;
        let Some(vm) = self.vm.as_mut() else {
            return Ok(());
        };
        let saved_msgbk = if leave_msgbk {
            Some(vm.ctx.globals.msgbk_forms.clone())
        } else {
            None
        };
        // eng_scene.cpp::tnm_scene_proc_restart_from_menu_scene() always returns
        // to the initial Select.ini append, reloads Scene.pck, then resolves the
        // configured MENU_SCENE. SceneVm reloads its package cache when append
        // changes, so resetting the append before restart preserves that ordering.
        vm.ctx.reset_active_append_to_initial();
        vm.restart_scene_name(&target_scene, target_z)?;
        if let Some(renderer) = self.renderer.as_ref() {
            renderer.borrow_mut().clear_runtime_image_textures();
        }
        if let Some(msgbk) = saved_msgbk {
            vm.ctx.globals.msgbk_forms = msgbk;
        }
        vm.ctx.globals.finish_wipe();
        self.flow.stack.clear();
        self.flow.pending_syscom_proc = None;
        self.flow.booted_menu = true;
        // C++ tnm_scene_proc_restart_func() pushes SCRIPT first, then
        // tnm_return_to_menu_proc() pushes GAME_TIMER_START on top of it.
        self.flow.push(ProcType::Script, 0);
        self.flow.push(ProcType::GameTimerStart, 0);
        Ok(())
    }

    fn perform_restart_from_scene(&mut self) -> Result<()> {
        let Some(vm) = self.vm.as_mut() else {
            return Ok(());
        };
        let (target_scene, target_z) = vm
            .ctx
            .pending_scene_restart
            .take()
            .ok_or_else(|| anyhow::anyhow!("GLOBAL.RETURNMENU scene restart missing target"))?;

        // eng_syscom.cpp::tnm_syscom_restart_from_scene() saves global state only
        // after the SCENESTART warning has been accepted, then restarts the named
        // scene without resetting the active append.
        syscom::write_global_save(&vm.ctx);
        vm.restart_scene_name(&target_scene, target_z)?;
        if let Some(renderer) = self.renderer.as_ref() {
            renderer.borrow_mut().clear_runtime_image_textures();
        }
        vm.ctx.globals.finish_wipe();
        self.flow.stack.clear();
        self.flow.pending_syscom_proc = None;
        self.flow.push(ProcType::Script, 0);
        self.script_needs_pump = true;
        self.frame_dirty = true;
        Ok(())
    }

    fn pump_vm(&mut self) -> Result<()> {
        if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
            let scene = self
                .vm
                .as_ref()
                .and_then(|vm| vm.current_scene_name())
                .unwrap_or("<none>");
            let line = self
                .vm
                .as_ref()
                .map(|vm| vm.current_line_no())
                .unwrap_or(-1);
            let pending = self
                .vm
                .as_ref()
                .and_then(|vm| vm.ctx.globals.syscom.pending_proc.as_ref())
                .map(|p| format!("{:?}", p));
            eprintln!(
                "[SG_PROC_FLOW] pump_vm start paused={} step_once={} frame_dirty={} script_needs_pump={} scene={} line={} flow={:?} pending_proc={}",
                self.paused,
                self.step_once,
                self.frame_dirty,
                self.script_needs_pump,
                scene,
                line,
                self.flow.stack,
                pending.as_deref().unwrap_or("None")
            );
        }
        self.script_needs_pump = false;
        self.ensure_requested_script_proc();
        if self.vm.is_none() {
            return Ok(());
        }

        if self.paused && !self.step_once {
            if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
                eprintln!(
                    "[SG_PROC_FLOW] pump_vm paused-return flow={:?}",
                    self.flow.stack
                );
            }
            return Ok(());
        }

        if let Some(vm) = self.vm.as_mut() {
            vm.process_pending_button_actions()?;
        }
        if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
            let scene = self
                .vm
                .as_ref()
                .and_then(|vm| vm.current_scene_name())
                .unwrap_or("<none>");
            let line = self
                .vm
                .as_ref()
                .map(|vm| vm.current_line_no())
                .unwrap_or(-1);
            let pending = self
                .vm
                .as_ref()
                .and_then(|vm| vm.ctx.globals.syscom.pending_proc.as_ref())
                .map(|p| format!("{:?}", p));
            eprintln!(
                "[SG_PROC_FLOW] pump_vm after_process_button_actions scene={} line={} flow={:?} pending_proc={}",
                scene,
                line,
                self.flow.stack,
                pending.as_deref().unwrap_or("None")
            );
        }
        let has_syscom_pending = self
            .vm
            .as_ref()
            .map(|vm| vm.ctx.globals.syscom.pending_proc.is_some())
            .unwrap_or(false);
        if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
            let scene = self
                .vm
                .as_ref()
                .and_then(|vm| vm.current_scene_name())
                .unwrap_or("<none>");
            let line = self
                .vm
                .as_ref()
                .map(|vm| vm.current_line_no())
                .unwrap_or(-1);
            let pending = self
                .vm
                .as_ref()
                .and_then(|vm| vm.ctx.globals.syscom.pending_proc.as_ref())
                .map(|p| format!("{:?}", p));
            eprintln!(
                "[SG_PROC_FLOW] pump_vm after_has_syscom_pending={} scene={} line={} flow={:?} pending_proc={}",
                has_syscom_pending,
                scene,
                line,
                self.flow.stack,
                pending.as_deref().unwrap_or("None")
            );
        }
        if has_syscom_pending {
            if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
                eprintln!(
                    "[SG_PROC_FLOW] pump_vm consume_pending_proc before flow={:?}",
                    self.flow.stack
                );
            }
            self.consume_syscom_pending_proc()?;
            self.ensure_requested_script_proc();
            if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
                let scene = self
                    .vm
                    .as_ref()
                    .and_then(|vm| vm.current_scene_name())
                    .unwrap_or("<none>");
                let line = self
                    .vm
                    .as_ref()
                    .map(|vm| vm.current_line_no())
                    .unwrap_or(-1);
                eprintln!(
                    "[SG_PROC_FLOW] pump_vm consume_pending_proc after scene={} line={} flow={:?}",
                    scene, line, self.flow.stack
                );
            }
        }

        if let Some(vm) = self.vm.as_mut() {
            vm.begin_script_proc_pump();
        }

        // Match the original C++ frame_main_proc(): keep advancing the proc
        // stack until the active proc asks to break for this frame. Script
        // execution itself is boundary-driven; there is no instruction quota.
        loop {
            if self.native_messagebox_pending() {
                break;
            }
            let Some(proc) = self.flow.top().cloned() else {
                self.paused = true;
                break;
            };
            if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
                let scene = self
                    .vm
                    .as_ref()
                    .and_then(|vm| vm.current_scene_name())
                    .unwrap_or("<none>");
                let line = self
                    .vm
                    .as_ref()
                    .map(|vm| vm.current_line_no())
                    .unwrap_or(-1);
                eprintln!(
                    "[SG_PROC_FLOW] pump_vm loop top proc={:?} scene={} line={} flow={:?}",
                    proc, scene, line, self.flow.stack
                );
            }

            match proc.ty {
                ProcType::Script => {
                    let (
                        running,
                        halted,
                        cur_scene,
                        pending,
                        blocked,
                        pop_script_proc,
                        proc_boundary,
                        boundary_kind,
                        load_completed,
                    ) = {
                        let vm = self.vm.as_mut().expect("vm checked");
                        let proc_gen_before = vm.proc_generation();
                        let running = vm.run_script_proc_continue()?;
                        let load_completed = vm.take_runtime_load_completed();
                        let proc_boundary = vm.proc_generation() != proc_gen_before;
                        let boundary_kind = vm.last_proc_kind();
                        let pop_script_proc = vm.take_script_proc_pop_request();
                        let halted = vm.is_halted();
                        let cur_scene = vm
                            .current_scene_name()
                            .map(|s| s.to_string())
                            .unwrap_or_else(|| self.boot.start_scene.clone());
                        let pending = vm.ctx.globals.syscom.pending_proc.is_some();
                        let blocked = if pending { false } else { vm.is_blocked() };
                        (
                            running,
                            halted,
                            cur_scene,
                            pending,
                            blocked,
                            pop_script_proc,
                            proc_boundary,
                            boundary_kind,
                            load_completed,
                        )
                    };
                    if load_completed {
                        // VM replaced the active scene wholesale, so the LOAD_SCENE
                        // excall and any other syscom flow entries pushed on top
                        // of the saved scene are orphaned. Drop them and resume
                        // with a clean Script proc.
                        self.finish_runtime_load();
                        continue;
                    }
                    if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
                        eprintln!(
                            "[SG_PROC_FLOW] script_continue result running={} halted={} cur_scene={} pending={} blocked={} pop_script_proc={} proc_boundary={} boundary={:?} flow={:?}",
                            running,
                            halted,
                            cur_scene,
                            pending,
                            blocked,
                            pop_script_proc,
                            proc_boundary,
                            boundary_kind,
                            self.flow.stack
                        );
                    }
                    self.ensure_requested_script_proc();
                    if pop_script_proc {
                        if std::env::var_os("SG_DEBUG").is_some() {
                            eprintln!(
                                "[SG_DEBUG][EXCALL] pop SCRIPT proc requested by ex-call return"
                            );
                        }
                        let popped_depth = self.flow.stack.len();
                        self.flow.pop();
                        self.restore_wait_after_syscom_excall(popped_depth);
                        continue;
                    }
                    if !running || halted {
                        self.flow.pop();
                        if !self.flow.booted_menu && cur_scene == self.boot.start_scene {
                            self.flow.push(ProcType::ReturnToMenu, 0);
                        }
                        continue;
                    }
                    if pending {
                        if self.consume_syscom_pending_proc()? {
                            continue;
                        }
                        let blocked = self
                            .vm
                            .as_ref()
                            .map(|vm| vm.ctx.wait.needs_runtime_poll())
                            .unwrap_or(false);
                        if blocked {
                            break;
                        }
                    } else if proc_boundary {
                        match boundary_kind {
                            // C++ frame_main_proc consumes DISP immediately and then breaks
                            // out to the renderer. The SCRIPT proc remains underneath and is
                            // resumed on the next frame.
                            ProcKind::Disp => {
                                self.script_resume_after_redraw = true;
                                break;
                            }
                            ProcKind::Frame => {
                                self.script_resume_after_redraw = true;
                                self.suppress_render_once = true;
                                break;
                            }
                            // These proc kinds are explicit C++ proc-stack boundaries, but
                            // when their runtime wait has already completed they do not consume
                            // a frame by themselves. Continue the frame_main_proc loop instead
                            // of deferring to a fixed per-frame slice.
                            ProcKind::Command
                            | ProcKind::MessageBlock
                            | ProcKind::MessageWait
                            | ProcKind::KeyWait
                            | ProcKind::TimeWait
                            | ProcKind::MovieWait
                            | ProcKind::WipeWait
                            | ProcKind::AudioWait
                            | ProcKind::EventWait
                            | ProcKind::Selection
                            | ProcKind::SystemModal
                            | ProcKind::Script => {
                                if blocked {
                                    break;
                                }
                                continue;
                            }
                        }
                    } else if blocked {
                        break;
                    }
                }
                ProcType::StartWarning => {
                    let warning_exists = {
                        let vm = self.vm.as_mut().expect("vm checked");
                        siglus_scene_vm::resource::game_file_exists(
                            &vm.ctx
                                .images
                                .project_dir()
                                .join("g00")
                                .join("___SYSEVE_WARNING.g00"),
                        ) || siglus_scene_vm::resource::game_file_exists(
                            &vm.ctx
                                .images
                                .project_dir()
                                .join("g00")
                                .join("___SYSEVE_WARNING.g01"),
                        )
                    };
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
                ProcType::SyscomWarning => {
                    let modal_active = self
                        .vm
                        .as_ref()
                        .map(|vm| vm.ctx.globals.system.messagebox_modal.is_some())
                        .unwrap_or(false);
                    if modal_active {
                        break;
                    }
                    let result = self
                        .vm
                        .as_mut()
                        .and_then(|vm| vm.ctx.globals.system.messagebox_modal_result.take())
                        .unwrap_or(1);
                    let pending = self.flow.pending_syscom_proc.take();
                    self.flow.pop();
                    if result == 0 {
                        if let Some(proc) = pending {
                            match proc.kind {
                                SyscomPendingProcKind::EndGame => {
                                    self.queue_end_game_proc(proc);
                                }
                                SyscomPendingProcKind::ReturnToMenu => {
                                    self.queue_return_to_menu_proc(proc);
                                }
                                SyscomPendingProcKind::RestartScene => {
                                    self.perform_restart_from_scene()?;
                                }
                                SyscomPendingProcKind::Save => {
                                    let Some(vm) = self.vm.as_mut() else {
                                        break;
                                    };
                                    syscom::menu_save_slot(
                                        &mut vm.ctx,
                                        false,
                                        proc.save_id.max(0) as usize,
                                    );
                                    syscom::write_global_save(&vm.ctx);
                                }
                                SyscomPendingProcKind::Load => {
                                    let Some(vm) = self.vm.as_mut() else {
                                        break;
                                    };
                                    syscom::menu_load_slot(
                                        &mut vm.ctx,
                                        false,
                                        proc.save_id.max(0) as usize,
                                    );
                                }
                                SyscomPendingProcKind::QuickSave => {
                                    let Some(vm) = self.vm.as_mut() else {
                                        break;
                                    };
                                    syscom::menu_save_slot(
                                        &mut vm.ctx,
                                        true,
                                        proc.save_id.max(0) as usize,
                                    );
                                    syscom::write_global_save(&vm.ctx);
                                }
                                SyscomPendingProcKind::QuickLoad => {
                                    let Some(vm) = self.vm.as_mut() else {
                                        break;
                                    };
                                    syscom::menu_load_slot(
                                        &mut vm.ctx,
                                        true,
                                        proc.save_id.max(0) as usize,
                                    );
                                }
                                _ => {}
                            }
                        }
                    } else if matches!(
                        pending.as_ref().map(|proc| proc.kind),
                        Some(SyscomPendingProcKind::RestartScene)
                    ) {
                        if let Some(vm) = self.vm.as_mut() {
                            vm.ctx.pending_scene_restart = None;
                        }
                    } else if matches!(
                        pending.as_ref().map(|proc| proc.kind),
                        Some(SyscomPendingProcKind::Save)
                    ) && let Some(vm) = self.vm.as_mut()
                    {
                        syscom::free_runtime_save_thumb_capture(
                            &mut vm.ctx,
                            syscom::CAPTURE_PRIOR_SAVE,
                        );
                    }
                    continue;
                }
                ProcType::MsgBack => {
                    let open = self
                        .vm
                        .as_ref()
                        .map(|vm| vm.ctx.globals.syscom.msg_back_open)
                        .unwrap_or(false);
                    if !open {
                        self.flow.pop();
                        continue;
                    }
                    break;
                }
                ProcType::Disp => {
                    self.flow.pop();
                    self.script_resume_after_redraw = true;
                    break;
                }
                ProcType::GameEndWipe => {
                    let mut start = false;
                    if let Some(top) = self.flow.top_mut()
                        && top.option == 0
                    {
                        top.option = 1;
                        start = true;
                    }
                    if start {
                        self.start_game_end_wipe();
                        break;
                    }
                    let wipe_done = self
                        .vm
                        .as_ref()
                        .map(|vm| vm.ctx.globals.wipe_done())
                        .unwrap_or(true);
                    if wipe_done {
                        self.flow.pop();
                        continue;
                    }
                    break;
                }
                ProcType::ReturnToMenu => {
                    let leave_msgbk = proc.option != 0;
                    self.perform_return_to_menu(leave_msgbk)?;
                    // Original tnm_return_to_menu_proc() returns false here:
                    // leave frame_main_proc and present once before the new
                    // GAME_TIMER_START/SCRIPT stack is resumed.
                    self.script_resume_after_redraw = true;
                    break;
                }
                ProcType::EndGame => {
                    self.flow.pop();
                    if let Some(vm) = self.vm.as_mut() {
                        syscom::write_global_save(&vm.ctx);
                        vm.ctx.globals.system.active_flag = false;
                    }
                    self.pending_exit = true;
                    continue;
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
            break;
        }
        self.step_once = false;
        Ok(())
    }

    fn redraw(&mut self) -> Result<()> {
        if let Some(size) = self.pending_surface_size.take()
            && let Some(renderer) = self.renderer.as_ref()
        {
            Self::configure_main_renderer(
                &mut renderer.borrow_mut(),
                size.width,
                size.height,
                self.game_size.0,
                self.game_size.1,
            );
        }
        // A modal dialog freezes script/frame evaluation, but the compositor
        // still needs a presented buffer to complete a resize (notably Wayland).
        if self.native_messagebox_pending() {
            if let (Some(vm), Some(renderer), Some(frame)) = (
                self.vm.as_ref(),
                self.renderer.as_ref(),
                self.last_presented_frame.as_ref(),
            ) {
                renderer.borrow_mut().render_frame(&vm.ctx.images, frame)?;
            }
            return Ok(());
        }
        if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
            let scene = self
                .vm
                .as_ref()
                .and_then(|vm| vm.current_scene_name())
                .unwrap_or("<none>");
            let line = self
                .vm
                .as_ref()
                .map(|vm| vm.current_line_no())
                .unwrap_or(-1);
            let pending = self
                .vm
                .as_ref()
                .and_then(|vm| vm.ctx.globals.syscom.pending_proc.as_ref())
                .map(|p| format!("{:?}", p));
            eprintln!(
                "[SG_PROC_FLOW] redraw start frame_dirty={} script_needs_pump={} scene={} line={} flow={:?} pending_proc={}",
                self.frame_dirty,
                self.script_needs_pump,
                scene,
                line,
                self.flow.stack,
                pending.as_deref().unwrap_or("None")
            );
        }
        // Match the original C++ frame order: script/input processing runs before
        // element frame evaluation and rendering.  If an input event woke the script,
        // pump it here before tick_frame(), otherwise the redraw for the same input
        // can show stale pre-script object/event state for one frame.
        if self.script_needs_pump {
            self.pump_vm()?;
        }
        // Original eng_frame.cpp turns wait_display_vsync_off_flag into the
        // device present interval once per frame. The opcode was already
        // implemented in the VM; apply its display-side effect here.
        if let (Some(vm), Some(renderer)) = (self.vm.as_ref(), self.renderer.as_ref()) {
            renderer
                .borrow_mut()
                .set_wait_display_vsync(!vm.ctx.globals.script.wait_display_vsync_off_flag);
        }
        let wait_poll_needed = self
            .vm
            .as_ref()
            .map(|vm| vm.ctx.wait.needs_runtime_poll())
            .unwrap_or(false);
        {
            let Some(vm) = self.vm.as_mut() else {
                return Ok(());
            };
            vm.tick_frame()?;
        }
        let has_syscom_pending = self
            .vm
            .as_ref()
            .map(|vm| vm.ctx.globals.syscom.pending_proc.is_some())
            .unwrap_or(false);
        if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
            let scene = self
                .vm
                .as_ref()
                .and_then(|vm| vm.current_scene_name())
                .unwrap_or("<none>");
            let line = self
                .vm
                .as_ref()
                .map(|vm| vm.current_line_no())
                .unwrap_or(-1);
            let pending = self
                .vm
                .as_ref()
                .and_then(|vm| vm.ctx.globals.syscom.pending_proc.as_ref())
                .map(|p| format!("{:?}", p));
            eprintln!(
                "[SG_PROC_FLOW] redraw after_tick has_syscom_pending={} scene={} line={} flow={:?} pending_proc={}",
                has_syscom_pending,
                scene,
                line,
                self.flow.stack,
                pending.as_deref().unwrap_or("None")
            );
        }
        if has_syscom_pending {
            self.consume_syscom_pending_proc()?;
            self.ensure_requested_script_proc();
            self.script_needs_pump = true;
        }
        if wait_poll_needed
            && let Some(vm) = self.vm.as_mut()
            && !vm.is_blocked()
        {
            self.script_needs_pump = true;
        }
        self.ensure_requested_script_proc();
        let render_suppressed = self.suppress_render_once;
        self.suppress_render_once = false;
        if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
            let scene = self
                .vm
                .as_ref()
                .and_then(|vm| vm.current_scene_name())
                .unwrap_or("<none>");
            let line = self
                .vm
                .as_ref()
                .map(|vm| vm.current_line_no())
                .unwrap_or(-1);
            eprintln!(
                "[SG_PROC_FLOW] redraw render_decision render_suppressed={} scene={} line={} flow={:?}",
                render_suppressed, scene, line, self.flow.stack
            );
        }
        if !render_suppressed {
            let Some(vm) = self.vm.as_mut() else {
                return Ok(());
            };
            let frame = vm.ctx.render_frame_with_effects();

            {
                let Some(renderer) = self.renderer.as_ref() else {
                    return Ok(());
                };
                renderer.borrow_mut().render_frame(&vm.ctx.images, &frame)?;
            }
            self.last_presented_frame = Some(frame);
        }

        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        if !render_suppressed {
            self.materialize_tweet_capture_after_disp()?;
        }

        if self.script_resume_after_redraw {
            self.script_resume_after_redraw = false;
            self.script_needs_pump = true;

            // C_tnm_eng::frame() invokes frame_main_proc() again on the next
            // engine frame without requiring an input/window event.  Winit's
            // desktop loop is event-driven, so a proc that deliberately broke
            // out for one presentation (DISP / FRAME / RETURN_TO_MENU) must
            // explicitly schedule that next engine frame here.  Merely setting
            // script_needs_pump can otherwise leave ControlFlow::Wait asleep
            // until the user moves/clicks the mouse.
            let vsync_wait_off = self
                .vm
                .as_ref()
                .map(|vm| vm.ctx.globals.script.wait_display_vsync_off_flag)
                .unwrap_or(false);
            if !vsync_wait_off && let Some(window) = self.window.as_ref() {
                window.request_redraw();
            }
        }

        self.redraw_count = self.redraw_count.saturating_add(1);
        // DISP/FRAME are frame_main_proc boundaries. The original loop resumes
        // SCRIPT after presentation rather than inserting a second fixed timer.
        // `script_resume_after_redraw` above preserves that boundary; the
        // renderer supplies display pacing only while VSync waiting is enabled.
        if !render_suppressed {
            self.maybe_capture_current_frame()?;
        }

        Ok(())
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn materialize_tweet_capture_after_disp(&mut self) -> Result<()> {
        let pending = self
            .vm
            .as_ref()
            .map(|vm| vm.ctx.globals.capture_for_tweet_pending)
            .unwrap_or(false);
        if !pending {
            return Ok(());
        }
        let Some(vm) = self.vm.as_mut() else {
            return Ok(());
        };
        let image = syscom::capture_for_tweet(&mut vm.ctx)?;
        vm.ctx.globals.capture_image = Some(image);
        vm.ctx.globals.capture_for_tweet_pending = false;
        Ok(())
    }

    fn maybe_capture_current_frame(&mut self) -> Result<()> {
        if self.captured {
            return Ok(());
        }
        let Some(path) = self.args.capture_png.as_ref() else {
            return Ok(());
        };
        let Some(vm) = self.vm.as_mut() else {
            return Ok(());
        };

        let render_frames = vm.ctx.globals.render_frame;
        let capture_gate = self.args.capture_after_frames as u64;
        if self.redraw_count as u64 >= capture_gate || render_frames >= capture_gate {
            let img = vm.ctx.capture_frame_rgba()?;
            let render_list = vm.ctx.render_list_with_effects();
            let nonzero_alpha = img
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|px| px[3] != 0)
                .count();
            let cur_scene = vm.current_scene_name().unwrap_or("<none>");
            eprintln!(
                "[INFO] capture stats: scene={} redraws={} render_frames={} sprites={} unknown_forms={} unknown_elements={} nonzero_alpha={}",
                cur_scene,
                self.redraw_count,
                render_frames,
                render_list.len(),
                vm.unknown_forms.len(),
                vm.ctx.unknown.element_chains.len(),
                nonzero_alpha,
            );
            Self::write_rgba_png(path, &img.rgba, img.width, img.height)?;
            if self.args.exit_after_capture {
                eprintln!("[SG_UNKNOWN]\n{}", vm.ctx.unknown.summary_string(2048));
            }
            eprintln!("[INFO] capture written to {}", path.display());
            self.captured = true;
            if self.args.exit_after_capture {
                self.pending_exit = true;
            }
        }
        Ok(())
    }

    fn redraw_hud_window(&mut self) -> Result<()> {
        self.render_hud_egui()
    }

    fn syscom_int(ctx: &CommandContext, key: i32, default: i64) -> i64 {
        ctx.globals
            .syscom
            .config_int
            .get(&key)
            .copied()
            .unwrap_or(default)
    }

    fn parse_first_i64(raw: &str) -> Option<i64> {
        raw.split(|c: char| c == ',' || c.is_whitespace())
            .find_map(|part| {
                let t = part.trim();
                if t.is_empty() {
                    None
                } else {
                    t.parse::<i64>().ok()
                }
            })
    }

    fn gameexe_i64(ctx: &CommandContext, key: &str, default: i64) -> i64 {
        ctx.tables
            .gameexe
            .as_ref()
            .and_then(|cfg| cfg.get_value(key))
            .and_then(Self::parse_first_i64)
            .unwrap_or(default)
    }

    fn syscom_window_config(ctx: &CommandContext) -> (i64, i64) {
        use siglus_scene_vm::runtime::forms::codes::syscom_op::{
            GET_WINDOW_MODE, GET_WINDOW_MODE_SIZE,
        };
        (
            Self::syscom_int(ctx, GET_WINDOW_MODE, 0),
            Self::syscom_int(ctx, GET_WINDOW_MODE_SIZE, 100),
        )
    }

    fn apply_syscom_window_config(&mut self) {
        const GET_MOUSE_CURSOR_HIDE_ONOFF: i32 =
            siglus_scene_vm::runtime::forms::codes::syscom_op::GET_MOUSE_CURSOR_HIDE_ONOFF;
        const GET_MOUSE_CURSOR_HIDE_TIME: i32 =
            siglus_scene_vm::runtime::forms::codes::syscom_op::GET_MOUSE_CURSOR_HIDE_TIME;

        let Some(w) = self.window.as_ref() else {
            return;
        };

        let (mode, size_mode) = {
            let Some(vm) = self.vm.as_ref() else {
                return;
            };
            Self::syscom_window_config(&vm.ctx)
        };

        if self.last_window_mode != Some(mode) {
            if mode == 0 {
                w.set_fullscreen(None);
            } else {
                w.set_fullscreen(Some(Fullscreen::Borderless(None)));
                // Reapply the selected client size when returning to windowed mode.
                self.last_window_size = None;
            }
            self.last_window_mode = Some(mode);
        }

        if self.last_window_size != Some(size_mode) && mode == 0 {
            let (w0, h0) = self.initial_size;
            let scale = size_mode.clamp(25, 400) as u32;
            if let Some(size) = w.request_surface_size(
                winit::dpi::PhysicalSize::new(
                    w0.saturating_mul(scale) / 100,
                    h0.saturating_mul(scale) / 100,
                )
                .into(),
            ) {
                // Wayland applies client-requested sizes synchronously and may
                // not send Resized. Present a buffer using the returned size.
                if size.width > 0 && size.height > 0 {
                    self.pending_surface_size = Some(size);
                }
            }
            w.request_redraw();
            self.last_window_size = Some(size_mode);
        }

        let Some(vm) = self.vm.as_mut() else {
            return;
        };
        let default_hide_on =
            Self::gameexe_i64(&vm.ctx, "CONFIG.MOUSE_CURSOR_HIDE_ONOFF", 0).clamp(0, 1);
        let default_hide_time =
            Self::gameexe_i64(&vm.ctx, "CONFIG.MOUSE_CURSOR_HIDE_TIME", 5000).max(0);
        let cfg_hide_on = Self::syscom_int(&vm.ctx, GET_MOUSE_CURSOR_HIDE_ONOFF, default_hide_on);
        let cfg_hide_time =
            Self::syscom_int(&vm.ctx, GET_MOUSE_CURSOR_HIDE_TIME, default_hide_time);
        let script = &vm.ctx.globals.script;
        let hide_on = match script.mouse_cursor_hide_onoff {
            0 => 0,
            1 => 1,
            _ => cfg_hide_on,
        };
        let hide_time = if script.mouse_cursor_hide_time >= 0 {
            script.mouse_cursor_hide_time
        } else {
            cfg_hide_time
        };

        let mut runtime_visible = !script.cursor_disp_off;
        if runtime_visible && hide_on != 0 && hide_time > 0 {
            let elapsed_ms = self.last_mouse_move.elapsed().as_millis() as i64;
            if elapsed_ms >= hide_time {
                runtime_visible = false;
            }
        }

        vm.ctx.globals.script.cursor_runtime_visible = runtime_visible;
        let custom_cursor = vm.ctx.has_active_custom_mouse_cursor();
        let custom_cursor_can_draw = custom_cursor && vm.ctx.input.has_mouse_position();
        let native_visible = runtime_visible && !custom_cursor_can_draw;
        w.set_cursor_visible(native_visible);
        if let Some((x, y, width, height)) = vm.ctx.focused_editbox_ime_area() {
            let surface = w.surface_size();
            let (vx, vy, vw, vh) = Self::aspect_fit_viewport(
                surface.width,
                surface.height,
                self.game_size.0,
                self.game_size.1,
            );
            let game_w = self.game_size.0.max(1) as f64;
            let game_h = self.game_size.1.max(1) as f64;
            let sx = vw as f64 / game_w;
            let sy = vh as f64 / game_h;
            let px = vx as f64 + x as f64 * sx;
            let py = vy as f64 + y as f64 * sy;
            let pw = width.max(1) as f64 * sx;
            let ph = height.max(1) as f64 * sy;
            let native_scale = w.scale_factor().max(f64::EPSILON);
            siglus_scene_vm::ime::enable_ime(
                *w,
                LogicalPosition::new(px / native_scale, py / native_scale).into(),
                LogicalSize::new(pw / native_scale, ph / native_scale).into(),
            );
        } else {
            siglus_scene_vm::ime::disable_ime(*w);
        }
        self.cursor_hidden = !native_visible;
        self.last_cursor_hide_on = Some(hide_on);
        self.last_cursor_hide_time = Some(hide_time);
    }
    fn wake_for_input(&mut self) {
        if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
            let scene = self
                .vm
                .as_ref()
                .and_then(|vm| vm.current_scene_name())
                .unwrap_or("<none>");
            let line = self
                .vm
                .as_ref()
                .map(|vm| vm.current_line_no())
                .unwrap_or(-1);
            let pending = self
                .vm
                .as_ref()
                .and_then(|vm| vm.ctx.globals.syscom.pending_proc.as_ref())
                .map(|p| format!("{:?}", p));
            eprintln!(
                "[SG_PROC_FLOW] wake_for_input before frame_dirty={} script_needs_pump={} scene={} line={} flow={:?} pending_proc={}",
                self.frame_dirty,
                self.script_needs_pump,
                scene,
                line,
                self.flow.stack,
                pending.as_deref().unwrap_or("None")
            );
        }
        self.frame_dirty = true;
        self.script_needs_pump = true;
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    fn modal_owner_event_allowed(event: &WindowEvent) -> bool {
        matches!(
            event,
            WindowEvent::SurfaceResized(_)
                | WindowEvent::ScaleFactorChanged { .. }
                | WindowEvent::RedrawRequested
        )
    }

    fn native_messagebox_pending(&self) -> bool {
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        if self.desktop_config_open {
            return true;
        }
        self.vm
            .as_ref()
            .and_then(|vm| vm.ctx.globals.system.messagebox_modal.as_ref())
            .map(|modal| modal.native_pending)
            .unwrap_or(false)
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn pump_desktop_config_request(&mut self, elwt: &dyn ActiveEventLoop) {
        let Some(mut dialog) = self.desktop_config_request.take() else {
            return;
        };
        if let Some(previous) = self.desktop_config_previous_dialog.as_ref() {
            dialog.remember_tab_from(previous);
        }
        match DesktopConfigWindow::new(elwt, dialog) {
            Ok(window) => self.desktop_config_window = Some(window),
            Err(err) => {
                log::error!("configuration window creation failed: {err:#}");
                self.desktop_config_open = false;
                if let Some(vm) = self.vm.as_mut() {
                    siglus_scene_vm::runtime::forms::syscom::open_fallback_dialog(
                        &mut vm.ctx,
                        SyscomPendingProcKind::OpenConfig,
                    );
                }
                self.wake_for_input();
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn handle_desktop_config_event(&mut self, event: WindowEvent) {
        use siglus_scene_vm::runtime::forms::syscom;
        let Some(window) = self.desktop_config_window.as_mut() else {
            return;
        };
        let Some(action) = window.handle_window_event(event) else {
            return;
        };
        if let Some(vm) = self.vm.as_mut() {
            syscom::apply_config_dialog_state(&mut vm.ctx, window.dialog.state.clone());
            if matches!(action, DesktopConfigAction::Close) {
                syscom::write_config_save(&vm.ctx);
                // The owner window did not receive releases while the dialog was active.
                vm.ctx.input = Default::default();
                vm.ctx.script_input = Default::default();
            }
        }
        if matches!(action, DesktopConfigAction::Close) {
            self.desktop_config_previous_dialog = self
                .desktop_config_window
                .take()
                .map(DesktopConfigWindow::into_dialog);
            self.desktop_config_open = false;
            if let Some(window) = self.window.as_ref() {
                window.focus_window();
            }
            self.wake_for_input();
        }
        self.apply_syscom_window_config();
    }

    fn needs_continuous_frame(&self) -> bool {
        if self.pending_exit {
            return false;
        }
        if self
            .flow
            .top()
            .map(|p| p.ty == ProcType::TimeWait)
            .unwrap_or(false)
        {
            return true;
        }
        self.vm
            .as_ref()
            .map(|vm| vm.ctx.needs_continuous_frame())
            .unwrap_or(false)
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn pump_desktop_messagebox_requests(&mut self, elwt: &dyn ActiveEventLoop) {
        if self.desktop_messagebox_window.is_some() || self.desktop_chihaya_bench_window.is_some() {
            return;
        }
        let Some(request) = self.desktop_messagebox_bridge.pop_request() else {
            return;
        };
        let request_id = request.request_id;
        let cancel_value = request
            .buttons
            .last()
            .map(|button| button.value)
            .unwrap_or(0);
        match DesktopMessageBoxWindow::new(elwt, request) {
            Ok(window) => {
                self.desktop_messagebox_window = Some(window);
            }
            Err(err) => {
                log::error!("desktop messagebox creation failed: {err:#}");
                if let Some(vm) = self.vm.as_mut() {
                    vm.ctx
                        .submit_native_messagebox_result(request_id, cancel_value);
                }
                self.wake_for_input();
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn handle_desktop_messagebox_window_event(&mut self, event: WindowEvent) {
        let Some(window) = self.desktop_messagebox_window.as_mut() else {
            return;
        };
        let request_id = window.request_id();
        let result = window.handle_window_event(event);
        if let Some(value) = result {
            if let Some(window) = self.desktop_messagebox_window.take() {
                window.hide();
            }
            if let Some(vm) = self.vm.as_mut() {
                vm.ctx.submit_native_messagebox_result(request_id, value);
            }
            self.wake_for_input();
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn pump_desktop_chihaya_bench_requests(&mut self, elwt: &dyn ActiveEventLoop) {
        if self.desktop_chihaya_bench_window.is_some() || self.desktop_messagebox_window.is_some() {
            return;
        }
        let Some(request) = self.desktop_messagebox_bridge.pop_chihaya_request() else {
            return;
        };
        let request_id = request.request_id;
        match DesktopChihayaBenchWindow::new(elwt, request) {
            Ok(window) => {
                self.desktop_chihaya_bench_window = Some(window);
            }
            Err(err) => {
                log::error!("desktop Chihaya benchmark dialog creation failed: {err:#}");
                if let Some(vm) = self.vm.as_mut() {
                    vm.ctx.submit_native_messagebox_result(request_id, 0);
                }
                self.wake_for_input();
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn handle_desktop_chihaya_bench_window_event(&mut self, event: WindowEvent) {
        let Some(window) = self.desktop_chihaya_bench_window.as_mut() else {
            return;
        };
        let request_id = window.request_id();
        let result = window.handle_window_event(event);
        if let Some(value) = result {
            if let Some(window) = self.desktop_chihaya_bench_window.take() {
                window.hide();
            }
            if let Some(vm) = self.vm.as_mut() {
                vm.ctx.submit_native_messagebox_result(request_id, value);
            }
            self.wake_for_input();
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn sync_desktop_twitter_account(&mut self) {
        let account = self.vm.as_ref().map(|vm| {
            let state = &vm.ctx.globals.twitter;
            (
                state.is_authorized(),
                state.user_name.clone(),
                state.screen_name.clone(),
            )
        });
        if let (Some(window), Some((authorized, user_name, screen_name))) =
            (self.desktop_twitter_window.as_mut(), account)
        {
            window.set_account_state(authorized, &user_name, &screen_name);
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn pump_desktop_twitter_request(&mut self, elwt: &dyn ActiveEventLoop) {
        let request = self
            .vm
            .as_mut()
            .and_then(|vm| vm.ctx.globals.twitter_dialog_request.take());
        let Some(request) = request else {
            return;
        };

        self.desktop_twitter_window = None;
        match DesktopTwitterWindow::new(elwt, request) {
            Ok(window) => {
                self.desktop_twitter_window = Some(window);
                self.sync_desktop_twitter_account();
            }
            Err(err) => {
                log::error!("desktop Twitter window creation failed: {err:#}");
            }
        }
    }

    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    fn handle_desktop_twitter_window_event(&mut self, event: WindowEvent) {
        self.sync_desktop_twitter_account();
        let action = self
            .desktop_twitter_window
            .as_mut()
            .and_then(|window| window.handle_window_event(event));
        let Some(action) = action else {
            return;
        };

        match action {
            DesktopTwitterAction::Close => {
                // Destroy the window and its surface; Wayland cannot hide windows.
                self.desktop_twitter_window = None;
            }
            DesktopTwitterAction::Authorize => {
                let result = self
                    .vm
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("VM is not available"))
                    .and_then(|vm| twitter::begin_authorize(&mut vm.ctx));
                match result {
                    Ok(_) => {
                        if let Some(window) = self.desktop_twitter_window.as_mut() {
                            window.show_authorization_entry();
                        }
                    }
                    Err(err) => {
                        if let Some(window) = self.desktop_twitter_window.as_mut() {
                            window.show_error(format!(
                                "Twitter 認証を開始できませんでした。\n\n{err:#}"
                            ));
                        }
                    }
                }
                self.sync_desktop_twitter_account();
            }
            DesktopTwitterAction::CompleteAuthorize(callback_or_verifier) => {
                let result = self
                    .vm
                    .as_mut()
                    .ok_or_else(|| anyhow::anyhow!("VM is not available"))
                    .and_then(|vm| twitter::complete_authorize(&mut vm.ctx, &callback_or_verifier));
                match result {
                    Ok(()) => {
                        self.sync_desktop_twitter_account();
                        if let Some(window) = self.desktop_twitter_window.as_mut() {
                            window.authentication_succeeded();
                        }
                    }
                    Err(err) => {
                        if let Some(window) = self.desktop_twitter_window.as_mut() {
                            window.show_error(format!("Twitter 認証に失敗しました。\n\n{err:#}"));
                        }
                    }
                }
            }
            DesktopTwitterAction::Tweet(text) => {
                let image_path = self
                    .desktop_twitter_window
                    .as_ref()
                    .map(|window| window.image_path().to_path_buf());
                let result = match (self.vm.as_mut(), image_path) {
                    (Some(vm), Some(path)) => twitter::tweet(&mut vm.ctx, &text, &path),
                    _ => Err(anyhow::anyhow!(
                        "Twitter dialog lost its VM or capture image"
                    )),
                };
                match result {
                    Ok(()) => {
                        if let Some(window) = self.desktop_twitter_window.as_mut() {
                            window.tweet_succeeded();
                        }
                    }
                    Err(err) => {
                        if let Some(window) = self.desktop_twitter_window.as_mut() {
                            window
                                .show_error(format!("Twitter への投稿に失敗しました。\n\n{err:#}"));
                        }
                    }
                }
            }
        }
    }

    fn begin_main_window_close(&mut self) {
        if self
            .syscom_suspended_waits
            .iter()
            .any(|(_, _, key)| key == "CLOSE_SCENE")
        {
            return;
        }
        let Some(vm) = self.vm.as_mut() else {
            return;
        };
        // EXCALL storage is shared by menus. Opening another menu while it
        // is owned could overwrite the active menu's objects and wait state.
        if !vm.ctx.excall_state.ex_call_flag && !vm.ctx.excall_state.ready {
            match vm.call_game_close_scene() {
                Ok(true) => {
                    self.ensure_requested_script_proc();
                    self.suspend_wait_for_syscom_excall("CLOSE_SCENE");
                    return;
                }
                Ok(false) => {}
                Err(err) => log::warn!("game close action failed: {err:#}"),
            }
        }
        self.begin_syscom_warning(SyscomPendingProc {
            kind: SyscomPendingProcKind::EndGame,
            warning: true,
            se_play: false,
            fade_out: false,
            leave_msgbk: false,
            save_id: 0,
        });
    }

    fn request_main_window_close(&mut self, elwt: &dyn ActiveEventLoop) {
        let Some(vm) = self.vm.as_ref() else {
            elwt.exit();
            return;
        };
        if vm.is_halted() {
            elwt.exit();
            return;
        }
        if self
            .flow
            .stack
            .iter()
            .any(|p| matches!(p.ty, ProcType::SyscomWarning | ProcType::EndGame))
        {
            self.wake_for_input();
            return;
        }
        if vm.ctx.globals.system.messagebox_modal.is_some() {
            self.wake_for_input();
            return;
        }
        self.begin_main_window_close();
        self.wake_for_input();
    }
}

impl App {
    fn update_pointer_position(&mut self, position: winit::dpi::PhysicalPosition<f64>) {
        if let Some(vm) = self.vm.as_mut() {
            let (x, y) = if let Some(w) = self.window.as_ref() {
                let surface = w.surface_size();
                Self::surface_point_to_game(
                    position.x,
                    position.y,
                    surface.width,
                    surface.height,
                    self.game_size.0,
                    self.game_size.1,
                )
            } else {
                (position.x.round() as i32, position.y.round() as i32)
            };
            vm.ctx.on_mouse_move(x, y);
        }
    }

    fn hud_egui_key(code: KeyCode) -> Option<egui::Key> {
        Some(match code {
            KeyCode::ArrowDown => egui::Key::ArrowDown,
            KeyCode::ArrowLeft => egui::Key::ArrowLeft,
            KeyCode::ArrowRight => egui::Key::ArrowRight,
            KeyCode::ArrowUp => egui::Key::ArrowUp,
            KeyCode::Escape => egui::Key::Escape,
            KeyCode::Tab => egui::Key::Tab,
            KeyCode::Backspace => egui::Key::Backspace,
            KeyCode::Enter | KeyCode::NumpadEnter => egui::Key::Enter,
            KeyCode::Space => egui::Key::Space,
            KeyCode::Insert => egui::Key::Insert,
            KeyCode::Delete => egui::Key::Delete,
            KeyCode::Home => egui::Key::Home,
            KeyCode::End => egui::Key::End,
            KeyCode::PageUp => egui::Key::PageUp,
            KeyCode::PageDown => egui::Key::PageDown,
            _ => return None,
        })
    }

    fn hud_pointer_button(button: MouseButton) -> Option<egui::PointerButton> {
        match button {
            MouseButton::Left => Some(egui::PointerButton::Primary),
            MouseButton::Right => Some(egui::PointerButton::Secondary),
            MouseButton::Middle => Some(egui::PointerButton::Middle),
            _ => None,
        }
    }

    /// Feed only the native HUD window into egui. The game window continues to
    /// use Siglus input semantics and never pays for egui input translation.
    fn feed_hud_egui_event(&mut self, event: &WindowEvent) -> bool {
        let Some(hud) = self.hud.as_mut() else {
            return false;
        };
        let scale = (hud.window.scale_factor() as f32).max(f32::EPSILON);
        let modifiers = hud.gui.raw_input.modifiers;

        match event {
            WindowEvent::PointerMoved {
                position,
                primary: true,
                ..
            }
            | WindowEvent::PointerEntered {
                position,
                primary: true,
                ..
            } => {
                let pos = egui::pos2(position.x as f32 / scale, position.y as f32 / scale);
                hud.gui.pointer_pos = Some(pos);
                hud.gui
                    .raw_input
                    .events
                    .push(egui::Event::PointerMoved(pos));
                true
            }
            WindowEvent::PointerLeft { primary: true, .. } => {
                hud.gui.pointer_pos = None;
                hud.gui.raw_input.events.push(egui::Event::PointerGone);
                true
            }
            WindowEvent::PointerButton {
                state,
                button,
                position,
                primary: true,
                ..
            } => {
                let Some(mouse_button) = button.clone().mouse_button() else {
                    return false;
                };
                let Some(button) = Self::hud_pointer_button(mouse_button) else {
                    return false;
                };
                let pos = egui::pos2(position.x as f32 / scale, position.y as f32 / scale);
                hud.gui.pointer_pos = Some(pos);
                hud.gui.raw_input.events.push(egui::Event::PointerButton {
                    pos,
                    button,
                    pressed: *state == ElementState::Pressed,
                    modifiers,
                });
                true
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (unit, delta) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => {
                        (egui::MouseWheelUnit::Line, egui::vec2(*x, *y))
                    }
                    MouseScrollDelta::PixelDelta(pos) => (
                        egui::MouseWheelUnit::Point,
                        egui::vec2(pos.x as f32 / scale, pos.y as f32 / scale),
                    ),
                    _ => return false,
                };
                hud.gui.raw_input.events.push(egui::Event::MouseWheel {
                    unit,
                    delta,
                    modifiers,
                });
                true
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state,
                        physical_key: PhysicalKey::Code(code),
                        repeat,
                        ..
                    },
                ..
            } => {
                let Some(key) = Self::hud_egui_key(*code) else {
                    return false;
                };
                hud.gui.raw_input.events.push(egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: *state == ElementState::Pressed,
                    repeat: *repeat,
                    modifiers,
                });
                true
            }
            WindowEvent::Focused(focused) => {
                hud.gui.raw_input.focused = *focused;
                hud.gui
                    .raw_input
                    .events
                    .push(egui::Event::WindowFocused(*focused));
                true
            }
            _ => false,
        }
    }

    fn resize_hud_surface(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            return;
        }
        let Some(renderer) = self.renderer.as_ref().cloned() else {
            return;
        };
        let Some(hud) = self.hud.as_mut() else {
            return;
        };
        hud.config.width = size.width;
        hud.config.height = size.height;
        let renderer = renderer.borrow();
        hud.surface.configure(&renderer.device, &hud.config);
    }

    /// Create the debug HUD lazily. It deliberately reuses the game's wgpu
    /// Instance/Device/Queue instead of constructing a second Siglus Renderer.
    /// With `self.hud == None` there is no HUD window, surface, egui renderer,
    /// texture cache, memory sampler, timer, or redraw work alive.
    fn open_hud(&mut self, elwt: &dyn ActiveEventLoop) -> Result<()> {
        if self.hud.is_some() {
            return Ok(());
        }
        let Some(renderer_rc) = self.renderer.as_ref().cloned() else {
            anyhow::bail!("main renderer is not initialized");
        };

        // Sample before allocating the HUD so the panel can distinguish game
        // memory from the diagnostic window itself. This syscall happens only
        // when F2 opens the HUD or when the user presses Refresh stats.
        let process_before_open = read_process_memory_snapshot();
        let window: Arc<dyn Window> = Arc::from(
            elwt.create_window(
                WindowAttributes::default()
                    .with_surface_size(LogicalSize::new(1280.0, 900.0))
                    .with_title("Siglus HUD")
                    .with_visible(true),
            )
            .context("create hud window")?,
        );

        let (surface, config, gui_renderer) = {
            let renderer = renderer_rc.borrow();
            let surface = renderer
                .instance
                .create_surface(window.clone())
                .context("create HUD surface")?;
            let caps = surface.get_capabilities(&renderer.adapter);
            let format = if caps.formats.contains(&renderer.config.format) {
                renderer.config.format
            } else {
                caps.formats
                    .iter()
                    .copied()
                    .find(|format| !format.is_srgb())
                    .unwrap_or(caps.formats[0])
            };
            let alpha_mode = caps
                .alpha_modes
                .iter()
                .copied()
                .find(|mode| *mode == wgpu::CompositeAlphaMode::Opaque)
                .unwrap_or(caps.alpha_modes[0]);
            let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Fifo) {
                wgpu::PresentMode::Fifo
            } else {
                caps.present_modes[0]
            };
            let size = window.surface_size();
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
            surface.configure(&renderer.device, &config);
            let gui_renderer = EguiRenderer::new(&renderer.device, format, None, 1);
            (surface, config, gui_renderer)
        };

        let raw_input = egui::RawInput {
            focused: true,
            ..Default::default()
        };
        let gui = HudGui {
            ctx: egui::Context::default(),
            renderer: gui_renderer,
            start_time: Instant::now(),
            raw_input,
            pointer_pos: None,
            gpu_texture_cache: HashMap::new(),
        };
        let process_memory = read_process_memory_snapshot();
        self.hud = Some(HudState {
            window: window.clone(),
            surface,
            config,
            gui,
            process_memory,
            process_before_open,
            preview_refresh_requested: false,
            show_memory: true,
            show_objects: true,
            show_textures: true,
            card_width: 360.0,
            preview_height: 220.0,
            object_list_height: 180.0,
        });
        window.request_redraw();
        Ok(())
    }

    /// Close means destroy, not hide. No device poll/wait is performed because
    /// the HUD shares the game's device; dropping the surface/egui resources is
    /// sufficient and avoids introducing a GPU synchronization stall.
    fn close_hud(&mut self) {
        if let Some(mut hud) = self.hud.take() {
            hud.gui.gpu_texture_cache.clear();
            drop(hud);
        }
    }

    fn handle_hud_window_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                self.close_hud();
            }
            WindowEvent::SurfaceResized(size) => {
                self.resize_hud_surface(size);
                if let Some(hud) = self.hud.as_ref() {
                    hud.window.request_redraw();
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(hud) = self.hud.as_ref() {
                    hud.window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key: PhysicalKey::Code(KeyCode::F2),
                        ..
                    },
                ..
            } => self.close_hud(),
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key: PhysicalKey::Code(KeyCode::F3),
                        ..
                    },
                ..
            } => {
                if let Some(hud) = self.hud.as_mut() {
                    hud.preview_refresh_requested = true;
                    hud.window.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if let Err(err) = self.redraw_hud_window() {
                    eprintln!("HUD render error: {err:?}");
                }
            }
            other => {
                if self.feed_hud_egui_event(&other)
                    && let Some(hud) = self.hud.as_ref()
                {
                    hud.window.request_redraw();
                }
            }
        }
    }
}
impl ApplicationHandler for App {
    fn can_create_surfaces(&mut self, elwt: &dyn ActiveEventLoop) {
        let title = Self::resolve_project_dir(&self.args)
            .as_deref()
            .map(siglus_scene_vm::runtime::game_display_info::resolve_game_name_from_project_dir)
            .unwrap_or_else(|| "Siglus Engine".to_string());
        let window_attrs = WindowAttributes::default().with_title(title);
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        let window_attrs = if let Some(project_dir) = Self::resolve_project_dir(&self.args) {
            siglus_scene_vm::desktop_icon::configure_window(window_attrs, &project_dir)
        } else {
            window_attrs
        };
        let window_attrs = if self.game_size.1 > PIXEL_EXACT_WINDOW_HEIGHT_THRESHOLD {
            window_attrs
                .with_surface_size(PhysicalSize::new(self.initial_size.0, self.initial_size.1))
        } else {
            window_attrs.with_surface_size(LogicalSize::new(
                self.initial_size.0 as f64,
                self.initial_size.1 as f64,
            ))
        };
        let window = elwt.create_window(window_attrs).expect("create window");
        let window: &'static dyn Window = Box::leak(window);
        let renderer = Rc::new(RefCell::new(
            pollster::block_on(Renderer::new(window)).expect("renderer init"),
        ));
        {
            let surface = window.surface_size();
            let mut renderer_ref = renderer.borrow_mut();
            Self::configure_main_renderer(
                &mut renderer_ref,
                surface.width,
                surface.height,
                self.game_size.0,
                self.game_size.1,
            );
        }
        let mut vm = self.init_vm().expect("vm init");
        vm.ctx.globals.system.chihaya_display_adapter_name =
            renderer.borrow().adapter.get_info().name;
        let capture_backend: FrameCaptureBackendRef = renderer.clone();
        vm.ctx.set_frame_capture_backend(Some(capture_backend));

        self.window_id = Some(window.id());
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.vm = Some(vm);

        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    fn window_event(
        &mut self,
        elwt: &dyn ActiveEventLoop,
        id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        if self.desktop_config_open
            && self.desktop_config_window.as_ref().map(|w| w.window_id()) == Some(id)
        {
            self.handle_desktop_config_event(event);
            return;
        }

        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        if self
            .desktop_messagebox_window
            .as_ref()
            .map(|window| window.window_id())
            == Some(id)
        {
            self.handle_desktop_messagebox_window_event(event);
            return;
        }

        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        if self
            .desktop_chihaya_bench_window
            .as_ref()
            .map(|window| window.window_id())
            == Some(id)
        {
            self.handle_desktop_chihaya_bench_window_event(event);
            return;
        }

        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        if self
            .desktop_twitter_window
            .as_ref()
            .map(|window| window.window_id())
            == Some(id)
        {
            self.handle_desktop_twitter_window_event(event);
            return;
        }

        let is_main = self.window_id == Some(id);
        if !is_main {
            // Normal game-window events do not even inspect HUD state. The only
            // steady-state HUD hook while closed is the F2 key branch below.
            if self.hud.as_ref().is_some_and(|hud| hud.window.id() == id) {
                self.handle_hud_window_event(event);
            }
            return;
        }
        if self.native_messagebox_pending() && !Self::modal_owner_event_allowed(&event) {
            // The original owner window is disabled for the duration of the
            // blocking MessageBox call; do not queue input for later VM frames.
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                self.request_main_window_close(elwt);
            }

            WindowEvent::SurfaceResized(size) => {
                if size.width > 0 && size.height > 0 {
                    // Configure once for the latest size at the next redraw.
                    self.pending_surface_size = Some(size);
                }
                if let Some(w) = self.window.as_ref() {
                    w.request_redraw();
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key: PhysicalKey::Code(KeyCode::F2),
                        ..
                    },
                ..
            } => {
                if self.hud.is_some() {
                    self.close_hud();
                } else if let Err(err) = self.open_hud(elwt) {
                    eprintln!("open HUD failed: {err:#}");
                    self.close_hud();
                }
                return;
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        physical_key: PhysicalKey::Code(code),
                        text,
                        repeat,
                        ..
                    },
                ..
            } => {
                if let (KeyCode::F3, Some(hud)) = (code, self.hud.as_mut()) {
                    hud.preview_refresh_requested = true;
                    hud.window.request_redraw();
                    return;
                }

                if !is_main {
                    return;
                }

                if let Some(vm) = self.vm.as_mut() {
                    if let Some(k) = map_keycode(code) {
                        // A held key must not become a fresh menu decision after
                        // RETURNMENU resets the VM input state. Keep text/edit
                        // repeats, but require a new press for decide/cancel.
                        if !repeat || !matches!(k, VmKey::Enter | VmKey::Space | VmKey::Escape) {
                            vm.ctx.on_key_down(k);
                        }
                    } else if !vm.ctx.editbox_accepts_keyboard_input() {
                        vm.ctx.notify_wait_key();
                    }
                    if vm.ctx.editbox_accepts_direct_text()
                        && let Some(text) = text.as_deref()
                    {
                        vm.ctx.on_text_input(text);
                    }
                }

                self.wake_for_input();
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Released,
                        physical_key: PhysicalKey::Code(KeyCode::F2),
                        ..
                    },
                ..
            } => return,
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Released,
                        physical_key: PhysicalKey::Code(code),
                        ..
                    },
                ..
            } => {
                if let Some(vm) = self.vm.as_mut()
                    && let Some(k) = map_keycode(code)
                {
                    vm.ctx.on_key_up(k);
                }
                self.wake_for_input();
            }
            WindowEvent::Ime(Ime::Preedit(text, cursor)) => {
                if !is_main {
                    return;
                }
                if let Some(vm) = self.vm.as_mut() {
                    vm.ctx.on_ime_preedit(&text, cursor);
                }
                self.wake_for_input();
            }
            WindowEvent::Ime(Ime::Commit(text)) => {
                if !is_main {
                    return;
                }
                if let Some(vm) = self.vm.as_mut() {
                    vm.ctx.on_text_input(&text);
                }
                self.wake_for_input();
            }
            WindowEvent::Ime(Ime::Disabled) => {
                if !is_main {
                    return;
                }
                if let Some(vm) = self.vm.as_mut() {
                    vm.ctx.on_ime_disabled();
                }
                self.wake_for_input();
            }
            WindowEvent::Ime(Ime::Enabled) => {}
            WindowEvent::RedrawRequested => {
                let res = if self
                    .vm
                    .as_ref()
                    .map(|vm| vm.ctx.globals.script.wait_display_vsync_off_flag)
                    .unwrap_or(false)
                {
                    // In IMMEDIATE mode the game loop is driven directly from
                    // about_to_wait(). Window-system redraw requests can be
                    // coalesced and must not create an extra VM frame here.
                    self.frame_dirty = true;
                    Ok(())
                } else {
                    self.redraw()
                };
                if let Err(e) = res {
                    eprintln!("render error: {e:?}");
                }
            }
            WindowEvent::PointerMoved {
                position,
                primary: true,
                ..
            }
            | WindowEvent::PointerEntered {
                position,
                primary: true,
                ..
            } => {
                if !is_main {
                    return;
                }
                self.update_pointer_position(position);
                self.last_mouse_move = Instant::now();
                self.wake_for_input();
                self.apply_syscom_window_config();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_lx, ly) => (ly * 120.0) as i32,
                    MouseScrollDelta::PixelDelta(p) => p.y.round() as i32,
                    _ => 0,
                };

                if let Some(vm) = self.vm.as_mut() {
                    vm.ctx.on_mouse_wheel(dy);
                }
                self.wake_for_input();
            }
            WindowEvent::PointerButton {
                state,
                button,
                position,
                primary: true,
                ..
            } => {
                let Some(button) = button.mouse_button() else {
                    return;
                };
                if !is_main {
                    return;
                }
                self.update_pointer_position(position);
                if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
                    let scene = self
                        .vm
                        .as_ref()
                        .and_then(|vm| vm.current_scene_name())
                        .unwrap_or("<none>");
                    let line = self
                        .vm
                        .as_ref()
                        .map(|vm| vm.current_line_no())
                        .unwrap_or(-1);
                    let pos = self
                        .vm
                        .as_ref()
                        .map(|vm| (vm.ctx.input.mouse_x, vm.ctx.input.mouse_y));
                    eprintln!(
                        "[SG_PROC_FLOW] window_mouse_input state={:?} button={:?} mapped={:?} pos={:?} scene={} line={} flow={:?}",
                        state,
                        button,
                        map_mouse_button(button),
                        pos,
                        scene,
                        line,
                        self.flow.stack
                    );
                }
                if let Some(vm) = self.vm.as_mut() {
                    if let Some(b) = map_mouse_button(button) {
                        match state {
                            ElementState::Pressed => vm.ctx.on_mouse_down(b),
                            ElementState::Released => vm.ctx.on_mouse_up(b),
                        }
                    } else if state == ElementState::Pressed {
                        vm.ctx.notify_wait_key();
                    }
                }
                self.wake_for_input();
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, elwt: &dyn ActiveEventLoop) {
        if self.pending_exit {
            elwt.exit();
            return;
        }

        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        {
            self.pump_desktop_config_request(elwt);
            self.pump_desktop_messagebox_requests(elwt);
            self.pump_desktop_chihaya_bench_requests(elwt);
            self.pump_desktop_twitter_request(elwt);
        }

        if self.native_messagebox_pending() {
            // `tnm_game_warning_box()` does not return until the user chooses a
            // button.  Freeze the VM exactly at that call boundary.
            elwt.set_control_flow(ControlFlow::Wait);
            return;
        }

        let capture_pending = self.args.capture_png.is_some() && !self.captured;
        let continuous_before = self.needs_continuous_frame();
        let wants_frame_or_script = self.frame_dirty
            || self.script_needs_pump
            || self.script_resume_after_redraw
            || continuous_before
            || capture_pending;

        let should_pump_script = self.script_needs_pump || capture_pending;
        if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
            let scene = self
                .vm
                .as_ref()
                .and_then(|vm| vm.current_scene_name())
                .unwrap_or("<none>");
            let line = self
                .vm
                .as_ref()
                .map(|vm| vm.current_line_no())
                .unwrap_or(-1);
            let blocked = self
                .vm
                .as_ref()
                .map(|vm| vm.ctx.wait.needs_runtime_poll())
                .unwrap_or(false);
            let pending = self
                .vm
                .as_ref()
                .and_then(|vm| vm.ctx.globals.syscom.pending_proc.as_ref())
                .map(|p| format!("{:?}", p));
            eprintln!(
                "[SG_PROC_FLOW] about_to_wait wants={} should_pump={} frame_dirty={} script_needs_pump={} resume_after_redraw={} continuous={} capture={} wait_until_future=false scene={} line={} blocked={} flow={:?} pending_proc={}",
                wants_frame_or_script,
                should_pump_script,
                self.frame_dirty,
                self.script_needs_pump,
                self.script_resume_after_redraw,
                continuous_before,
                capture_pending,
                scene,
                line,
                blocked,
                self.flow.stack,
                pending.as_deref().unwrap_or("None")
            );
        }
        if should_pump_script {
            if let Err(e) = self.pump_vm() {
                let scene_name = self
                    .vm
                    .as_ref()
                    .and_then(|vm| vm.current_scene_name())
                    .unwrap_or("<none>");
                let scene_no = self
                    .vm
                    .as_ref()
                    .and_then(|vm| vm.current_scene_no())
                    .map(|v: usize| v.to_string())
                    .unwrap_or_else(|| "?".to_string());
                let line_no = self
                    .vm
                    .as_ref()
                    .map(|vm| vm.current_line_no())
                    .unwrap_or(-1);
                eprintln!(
                    "vm error: scene={} scene_no={} line={} {e:?}",
                    scene_name, scene_no, line_no
                );
            }
            if let Err(e) = self.maybe_capture_current_frame() {
                eprintln!("capture error: {e:?}");
            }
            self.apply_syscom_window_config();
            #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
            {
                self.pump_desktop_config_request(elwt);
                self.pump_desktop_messagebox_requests(elwt);
                self.pump_desktop_chihaya_bench_requests(elwt);
                self.pump_desktop_twitter_request(elwt);
            }
            self.frame_dirty = true;
        }

        let continuous_after = self.needs_continuous_frame();
        let vsync_wait_off = self
            .vm
            .as_ref()
            .map(|vm| vm.ctx.globals.script.wait_display_vsync_off_flag)
            .unwrap_or(false);

        if vsync_wait_off {
            // D3DPRESENT_INTERVAL_IMMEDIATE in the original engine does not
            // wait for a window-system paint notification: frame_main_proc(),
            // element frame processing and Present keep running back-to-back.
            // Winit RedrawRequested is explicitly coalescible, so merely using
            // ControlFlow::Poll while still waiting for RedrawRequested leaves
            // the benchmark paced by the compositor. Drive one engine frame
            // directly per poll iteration instead.
            self.frame_dirty = false;
            if let Err(e) = self.redraw() {
                eprintln!("render error: {e:?}");
            }
            elwt.set_control_flow(ControlFlow::Poll);
            return;
        }

        if self.frame_dirty
            || self.script_needs_pump
            || self.script_resume_after_redraw
            || continuous_after
            || capture_pending
        {
            if let Some(w) = self.window.as_ref() {
                w.request_redraw();
            }
            self.frame_dirty = false;
            elwt.set_control_flow(ControlFlow::Wait);
        } else {
            elwt.set_control_flow(ControlFlow::Wait);
        }
    }
}

fn main() -> Result<()> {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("error"))
        .try_init();
    let args = Args::parse();
    // Frame capture must use the renderer attached during `can_create_surfaces`; the old
    // exit-after-capture shortcut constructed only a VM and could never own a
    // GPU FrameCaptureBackend.
    let el = EventLoop::new()?;
    let app = App::new(args);
    el.run_app(app)?;
    Ok(())
}

#[allow(dead_code)]
fn run_headless_capture(args: Args) -> Result<()> {
    let mut app = App::new(args);
    let vm = app.init_vm()?;
    app.vm = Some(vm);

    let capture_target = app.args.capture_after_frames.max(1);
    let max_frames = capture_target.saturating_add(600);
    for _ in 0..max_frames {
        app.pump_vm()?;
        let mut injected_wait_click = false;
        if let Some(vm) = app.vm.as_mut()
            && vm.is_blocked()
        {
            let x = ((vm.ctx.screen_w / 2).min(i32::MAX as u32)) as i32;
            let y = ((vm.ctx.screen_h / 2).min(i32::MAX as u32)) as i32;
            vm.ctx.on_mouse_move(x, y);
            vm.ctx.on_mouse_down(VmMouseButton::Left);
            vm.ctx.on_mouse_up(VmMouseButton::Left);
            injected_wait_click = true;
        }
        if injected_wait_click {
            app.pump_vm()?;
        }
        if let Some(vm) = app.vm.as_mut() {
            vm.tick_frame()?;
        }
        app.ensure_requested_script_proc();
        app.redraw_count = app.redraw_count.saturating_add(1);
        app.maybe_capture_current_frame()?;
        if app.pending_exit || app.captured {
            break;
        }
    }

    anyhow::ensure!(
        app.captured,
        "headless capture did not reach frame {} within {} frames",
        capture_target,
        max_frames
    );
    Ok(())
}

#[cfg(test)]
mod desktop_coordinate_tests {
    use super::App;

    #[cfg(feature = "virtual-clock")]
    fn close_test_excall_errors() -> &'static std::sync::Mutex<Vec<String>> {
        struct Logger(std::sync::Mutex<Vec<String>>);
        impl log::Log for Logger {
            fn enabled(&self, meta: &log::Metadata<'_>) -> bool {
                meta.level() == log::Level::Error
            }
            fn log(&self, record: &log::Record<'_>) {
                if self.enabled(record.metadata()) && record.target().ends_with("forms::excall") {
                    self.0.lock().unwrap().push(record.args().to_string());
                }
            }
            fn flush(&self) {}
        }
        static LOGGER: Logger = Logger(std::sync::Mutex::new(Vec::new()));
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            log::set_logger(&LOGGER).unwrap();
            log::set_max_level(log::LevelFilter::Error);
        });
        &LOGGER.0
    }

    #[cfg(feature = "virtual-clock")]
    #[test]
    #[ignore = "requires a disposable SIGLUS_CLOSE_TEST_PROJECT with Sana assets"]
    fn game_close_during_loading_uses_fallback() {
        use super::*;
        let errors = close_test_excall_errors();
        let project = std::env::var("SIGLUS_CLOSE_TEST_PROJECT").unwrap();
        // Both the boot scene and the title's initial loading state must avoid
        // borrowing a cancel-menu callback or uninitialized title buttons.
        for scene in ["__start", "_01menu"] {
            for result in [1, 0] {
                let mut app = App::new(Args::parse_from([
                    "siglus_engine",
                    "--project-dir",
                    &project,
                    "--scene-name",
                    scene,
                ]));
                app.vm = Some(app.init_vm().unwrap());
                app.flow.push(ProcType::Script, 0);
                let depth = app.flow.stack.len();
                app.begin_main_window_close();
                let vm = app.vm.as_mut().unwrap();
                assert_eq!(vm.current_scene_name(), Some(scene));
                assert!(!vm.take_script_proc_request());
                assert!(!vm.ctx.excall_state.ready);
                assert!(vm.ctx.globals.system.messagebox_modal.take().is_some());
                assert!(app.syscom_suspended_waits.is_empty());
                assert_eq!(app.flow.stack.len(), depth + 1);
                vm.ctx.globals.system.messagebox_modal_result = Some(result);
                app.pump_vm().unwrap();
                // EndGame presents one last frame before completing.
                if result == 0 {
                    for _ in 0..4 {
                        if app.pending_exit {
                            break;
                        }
                        app.redraw_count += 1;
                        app.pump_vm().unwrap();
                    }
                }
                assert_eq!(app.pending_exit, result == 0);
                assert!(
                    !app.flow
                        .stack
                        .iter()
                        .any(|proc| proc.ty == ProcType::SyscomWarning)
                );
            }
        }
        assert!(errors.lock().unwrap().is_empty(), "{errors:?}");
    }

    #[cfg(feature = "virtual-clock")]
    #[test]
    #[ignore = "requires a disposable SIGLUS_CLOSE_TEST_PROJECT; set SIGLUS_CLOSE_TEST_GAME=sprb for Summer Pockets RB"]
    fn game_close_dialog_cancel_and_confirm() {
        use super::*;
        use siglus_scene_vm::runtime::forms::codes::ELM_GLOBAL_G;
        let errors = close_test_excall_errors();

        fn frames(app: &mut App, count: usize) {
            for _ in 0..count {
                siglus_scene_vm::platform_time::advance_virtual_clock(
                    std::time::Duration::from_millis(16),
                );
                app.pump_vm().unwrap();
                app.vm.as_mut().unwrap().tick_frame().unwrap();
                app.redraw_count += 1;
                if app.pending_exit {
                    break;
                }
            }
        }

        let project = std::env::var("SIGLUS_CLOSE_TEST_PROJECT").unwrap();
        let sprb = std::env::var("SIGLUS_CLOSE_TEST_GAME").as_deref() == Ok("sprb");
        let (title, dialog, flag, yes) = if sprb {
            ("_rb_titlemenu", "__sys_system_call", 1323, (850, 540))
        } else {
            ("_01menu", "_00dialog", 18, (900, 510))
        };
        let mut app = App::new(Args::parse_from([
            "siglus_engine",
            "--project-dir",
            &project,
            "--scene-name",
            title,
        ]));
        app.vm = Some(app.init_vm().unwrap());
        app.flow.push(ProcType::Script, 0);
        frames(&mut app, 500);
        let vm = app.vm.as_mut().unwrap();
        let flags = vm
            .ctx
            .globals
            .int_lists
            .entry(ELM_GLOBAL_G as u32)
            .or_default();
        if flags.len() <= flag {
            flags.resize(flag + 1, 0);
        }
        flags[flag] = 1;
        let original_scene = vm.current_scene_name().unwrap().to_string();
        let depth = app.flow.stack.len();
        app.begin_main_window_close();
        assert_eq!(app.syscom_suspended_waits.len(), 1);
        app.begin_main_window_close();
        assert_eq!(app.syscom_suspended_waits.len(), 1);
        frames(&mut app, 120);
        assert_eq!(app.vm.as_ref().unwrap().current_scene_name(), Some(dialog));
        assert!(
            app.vm
                .as_ref()
                .unwrap()
                .ctx
                .globals
                .system
                .messagebox_modal
                .is_none()
        );
        // The game maps right-click to No.
        app.vm
            .as_mut()
            .unwrap()
            .ctx
            .on_mouse_down(VmMouseButton::Right);
        frames(&mut app, 2);
        app.vm
            .as_mut()
            .unwrap()
            .ctx
            .on_mouse_up(VmMouseButton::Right);
        frames(&mut app, 120);
        assert!(!app.pending_exit);
        assert!(app.syscom_suspended_waits.is_empty());
        assert_eq!(app.flow.stack.len(), depth);
        assert_eq!(
            app.vm.as_ref().unwrap().current_scene_name(),
            Some(original_scene.as_str())
        );

        app.begin_main_window_close();
        frames(&mut app, 120);
        app.vm.as_mut().unwrap().ctx.on_mouse_move(yes.0, yes.1);
        frames(&mut app, 2);
        app.vm
            .as_mut()
            .unwrap()
            .ctx
            .on_mouse_down(VmMouseButton::Left);
        frames(&mut app, 2);
        app.vm
            .as_mut()
            .unwrap()
            .ctx
            .on_mouse_up(VmMouseButton::Left);
        frames(&mut app, 240);
        assert!(app.pending_exit);
        assert!(errors.lock().unwrap().is_empty(), "{errors:?}");
    }

    fn temp_project_dir(tag: &str) -> std::path::PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("siglus-{tag}-{}-{nonce}", std::process::id()))
    }

    #[test]
    fn boot_config_uses_original_start_and_menu_defaults() {
        use super::*;

        let project_dir = temp_project_dir("boot-defaults");
        std::fs::create_dir_all(&project_dir).unwrap();
        let args = Args::parse_from([
            "siglus_engine",
            "--project-dir",
            project_dir.to_str().unwrap(),
        ]);
        let boot = App::resolve_boot_config(&args);
        assert_eq!(boot.start_scene, "_start");
        assert_eq!(boot.start_z, 0);
        assert_eq!(boot.menu_scene, "_menu");
        assert_eq!(boot.menu_z, 0);
        let _ = std::fs::remove_dir_all(project_dir);
    }

    #[test]
    fn boot_config_overrides_original_menu_default_from_gameexe() {
        use super::*;

        let project_dir = temp_project_dir("boot-menu-override");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::write(
            project_dir.join("Gameexe.ini"),
            "#START_SCENE = \"entry\",2\n#MENU_SCENE = \"title\",7\n",
        )
        .unwrap();
        let args = Args::parse_from([
            "siglus_engine",
            "--project-dir",
            project_dir.to_str().unwrap(),
        ]);
        let boot = App::resolve_boot_config(&args);
        assert_eq!(boot.start_scene, "entry");
        assert_eq!(boot.start_z, 2);
        assert_eq!(boot.menu_scene, "title");
        assert_eq!(boot.menu_z, 7);
        let _ = std::fs::remove_dir_all(project_dir);
    }

    #[test]
    fn config_subdialogs_preserve_active_script_and_excall() {
        use super::*;
        use siglus_scene_vm::runtime::forms::codes::syscom_op::*;
        use siglus_scene_vm::runtime::{Value, VmCallMeta};

        // No scene assets are needed: attempting to load CONFIG_SCENE must fail,
        // whereas opening a native subdialog must leave this caller intact.
        let mut header = [0i32; 33];
        for index in (1..33).step_by(2).chain(std::iter::once(0)) {
            header[index] = 33 * 4;
        }
        let bytes: Vec<u8> = header.into_iter().flat_map(i32::to_le_bytes).collect();
        let stream = SceneStream::new(Box::leak(bytes.into_boxed_slice())).unwrap();
        let mut app = App::new(Args::parse_from(["siglus_engine"]));
        let mut ctx = CommandContext::new(std::env::temp_dir().join("siglus-subdialog-test"));
        ctx.tables.gameexe = Some(GameexeConfig::from_text(
            "#CONFIG_SCENE=\"must_not_be_loaded\",0",
        ));
        ctx.excall_state.ready = true;
        ctx.excall_state.ex_call_flag = true;
        let form = ctx.ids.form_global_syscom;
        app.vm = Some(SceneVm::new(stream, ctx));
        let proc_depth = app.flow.stack.len();

        for op in [
            CALL_CONFIG_FONT_MENU,
            CALL_CONFIG_WINDOW_MODE_MENU,
            CALL_CONFIG_VOLUME_MENU,
            CALL_CONFIG_BGMFADE_MENU,
            CALL_CONFIG_KOEMODE_MENU,
            CALL_CONFIG_CHARAKOE_MENU,
            CALL_CONFIG_JITAN_MENU,
            CALL_CONFIG_MESSAGE_SPEED_MENU,
            CALL_CONFIG_FILTER_COLOR_MENU,
            CALL_CONFIG_AUTO_MODE_MENU,
            CALL_CONFIG_SYSTEM_MENU,
            CALL_CONFIG_MOVIE_MENU,
        ] {
            let vm = app.vm.as_mut().unwrap();
            vm.ctx.vm_call = Some(VmCallMeta {
                element: vec![form as i32, op],
                ..Default::default()
            });
            assert!(syscom::dispatch(&mut vm.ctx, form, &[] as &[Value]).unwrap());
            assert!(app.consume_syscom_pending_proc().unwrap());
            assert!(app.desktop_config_open);
            assert!(app.desktop_config_request.take().is_some());
            assert_eq!(app.flow.stack.len(), proc_depth);
            assert!(app.syscom_suspended_waits.is_empty());
            let vm = app.vm.as_mut().unwrap();
            assert!(vm.ctx.excall_state.ready);
            assert!(vm.ctx.excall_state.ex_call_flag);
            assert!(!vm.take_script_proc_request());
            app.desktop_config_open = false;
        }

        let vm = app.vm.as_mut().unwrap();
        vm.ctx.vm_call.as_mut().unwrap().element[1] = CALL_CONFIG_MENU;
        syscom::dispatch(&mut vm.ctx, form, &[]).unwrap();
        assert_eq!(
            vm.ctx.globals.syscom.pending_proc.as_ref().unwrap().kind,
            SyscomPendingProcKind::OpenConfig,
        );
    }

    #[test]
    fn modal_owner_allows_presentation_but_blocks_game_input() {
        use winit::event::{ElementState, MouseButton, WindowEvent};
        assert!(App::modal_owner_event_allowed(
            &WindowEvent::RedrawRequested
        ));
        assert!(App::modal_owner_event_allowed(
            &WindowEvent::SurfaceResized(winit::dpi::PhysicalSize::new(1280, 720),)
        ));
        assert!(!App::modal_owner_event_allowed(
            &WindowEvent::CloseRequested
        ));
        assert!(!App::modal_owner_event_allowed(
            &WindowEvent::PointerButton {
                device_id: None,
                primary: true,
                position: winit::dpi::PhysicalPosition::new(0.0, 0.0),
                is_macos_activation_click: false,
                state: ElementState::Pressed,
                button: MouseButton::Left.into(),
            }
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    #[ignore = "requires a desktop/GPU and SIGLUS_CONFIG_TEST_GAME pointing to a disposable game copy"]
    fn modal_window_can_shrink_and_grow_repeatedly() {
        use super::*;
        use winit::platform::wayland::EventLoopBuilderExtWayland;
        const SCALES: [u32; 6] = [50, 100, 75, 125, 50, 100];
        const TOTAL_STEPS: usize = SCALES.len() * 10;

        struct Probe {
            app: App,
            step: usize,
            deadline: Instant,
            presented: bool,
        }
        impl Probe {
            fn resize(&mut self) {
                let state = &mut self
                    .app
                    .desktop_config_window
                    .as_mut()
                    .unwrap()
                    .dialog
                    .state;
                state.screen_size_mode = 0;
                let scale = i64::from(SCALES[self.step % SCALES.len()]);
                state.screen_size_scale = (scale, scale);
                syscom::apply_config_dialog_state(
                    &mut self.app.vm.as_mut().unwrap().ctx,
                    state.clone(),
                );
                self.app.apply_syscom_window_config();
                self.presented = false;
                self.deadline = Instant::now() + std::time::Duration::from_secs(10);
            }
        }
        impl ApplicationHandler for Probe {
            fn can_create_surfaces(&mut self, elwt: &dyn ActiveEventLoop) {
                let size = self.app.initial_size;
                let window: &'static dyn Window = Box::leak(
                    elwt.create_window(
                        WindowAttributes::default()
                            .with_title("Siglus resize regression")
                            .with_surface_size(PhysicalSize::new(size.0, size.1)),
                    )
                    .unwrap(),
                );
                self.app.window = Some(window);
                self.app.window_id = Some(window.id());
                let mut renderer = pollster::block_on(Renderer::new(window)).unwrap();
                App::configure_main_renderer(&mut renderer, size.0, size.1, size.0, size.1);
                self.app.renderer = Some(Rc::new(RefCell::new(renderer)));
                self.app.vm = Some(self.app.init_vm().unwrap());
                self.app.last_presented_frame = Some(RenderFrame::ordinary(Vec::new()));
                self.app.desktop_config_open = true;
                self.app.redraw().unwrap();
                self.app.desktop_config_request =
                    Some(ConfigDialog::new(&self.app.vm.as_ref().unwrap().ctx));
                self.app.pump_desktop_config_request(elwt);
                self.resize();
            }

            fn window_event(
                &mut self,
                elwt: &dyn ActiveEventLoop,
                id: WindowId,
                event: WindowEvent,
            ) {
                if self.app.window_id == Some(id) && matches!(event, WindowEvent::RedrawRequested) {
                    self.presented = true;
                }
                self.app.window_event(elwt, id, event);
            }

            fn about_to_wait(&mut self, elwt: &dyn ActiveEventLoop) {
                let scale = SCALES[self.step % SCALES.len()];
                let size = self.app.window.unwrap().surface_size();
                let expected = PhysicalSize::new(
                    self.app.initial_size.0 * scale / 100,
                    self.app.initial_size.1 * scale / 100,
                );
                let rendered = self.app.renderer.as_ref().unwrap().borrow();
                let ready = self.presented
                    && size == expected
                    && (rendered.config.width, rendered.config.height)
                        == (expected.width, expected.height);
                drop(rendered);
                assert!(
                    Instant::now() < self.deadline,
                    "resize to {scale}% stalled at {size:?}"
                );
                if ready {
                    assert_eq!(
                        self.app.redraw_count, 0,
                        "modal redraw advanced the VM frame"
                    );
                    eprintln!(
                        "modal resize {scale}%: {}x{} presented",
                        size.width, size.height
                    );
                    self.step += 1;
                    if self.step == TOTAL_STEPS {
                        self.app
                            .handle_desktop_config_event(WindowEvent::CloseRequested);
                        assert!(self.app.desktop_config_window.is_none());
                        elwt.exit();
                        return;
                    }
                    self.resize();
                }
                elwt.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + std::time::Duration::from_millis(20),
                ));
            }
        }

        let project =
            std::env::var("SIGLUS_CONFIG_TEST_GAME").expect("set SIGLUS_CONFIG_TEST_GAME");
        let app = App::new(Args::parse_from([
            "siglus_engine",
            "--project-dir",
            &project,
            "--scene",
            "_menu",
        ]));
        let probe = Probe {
            app,
            step: 0,
            deadline: Instant::now(),
            presented: false,
        };
        EventLoop::builder()
            .with_any_thread(true)
            .build()
            .unwrap()
            .run_app(probe)
            .unwrap();
    }

    #[test]
    fn desktop_window_config_reads_live_dialog_and_script_settings() {
        use siglus_scene_vm::desktop_config::ConfigDialog;
        use siglus_scene_vm::runtime::CommandContext;
        use siglus_scene_vm::runtime::forms::{codes::syscom_op::*, syscom};

        let mut ctx = CommandContext::new(std::env::temp_dir().join("siglus-window-config-test"));
        assert_eq!(App::syscom_window_config(&ctx), (0, 100));
        let mut dialog = ConfigDialog::new(&ctx);
        dialog.state.screen_size_scale = (75, 75);
        syscom::apply_config_dialog_state(&mut ctx, dialog.state.clone());
        assert_eq!(App::syscom_window_config(&ctx), (0, 75));
        dialog.state.screen_size_mode = 1;
        syscom::apply_config_dialog_state(&mut ctx, dialog.state);
        assert_eq!(App::syscom_window_config(&ctx), (1, 75));
        ctx.globals.syscom.config_int.insert(GET_WINDOW_MODE, 0);
        ctx.globals
            .syscom
            .config_int
            .insert(GET_WINDOW_MODE_SIZE, 150);
        assert_eq!(App::syscom_window_config(&ctx), (0, 150));
    }

    #[test]
    fn aspect_fit_16_inch_retina_surface_keeps_1920x1080_ratio() {
        assert_eq!(
            App::aspect_fit_viewport(3456, 2234, 1920, 1080),
            (0, 145, 3456, 1944)
        );
    }

    #[test]
    fn retina_surface_center_maps_to_game_center() {
        assert_eq!(
            App::surface_point_to_game(1728.0, 1117.0, 3456, 2234, 1920, 1080),
            (960, 540)
        );
    }

    #[test]
    fn pixel_exact_window_maps_one_to_one() {
        assert_eq!(
            App::surface_point_to_game(1234.0, 567.0, 1920, 1080, 1920, 1080),
            (1234, 567)
        );
    }
}
