use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use egui::{ColorImage, TextureHandle, TextureOptions};
use egui_wgpu::{Renderer as EguiRenderer, ScreenDescriptor};

use anyhow::{bail, Context, Result};
use clap::Parser;
use image::ColorType;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, Ime, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{KeyCode, PhysicalKey};
use winit::window::Fullscreen;
use winit::window::{Window, WindowAttributes, WindowId};

use siglus_assets::gameexe::{decode_gameexe_dat_bytes, GameexeConfig, GameexeDecodeOptions};
use siglus_assets::scene_pck::{find_scene_pck_in_project, ScenePck, ScenePckDecodeOptions};

use siglus_scene_vm::image_manager::ImageId;
use siglus_scene_vm::render::{Renderer, RendererDebugTexture};
use siglus_scene_vm::runtime::globals::{
    SyscomPendingProc, SyscomPendingProcKind, SystemMessageBoxButton, SystemMessageBoxModalState,
    WipeState,
};
use siglus_scene_vm::runtime::input::{VmKey, VmMouseButton};
use siglus_scene_vm::runtime::{native_ui, CommandContext, ProcKind};
use siglus_scene_vm::scene_stream::SceneStream;
use siglus_scene_vm::vm::{SceneVm, VmConfig};

const FRAME_INTERVAL: Duration = Duration::from_millis(16);

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
    menu_scene: Option<String>,
    menu_z: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcType {
    Script,
    StartWarning,
    SyscomWarning,
    ReturnToMenu,
    GameEndWipe,
    Disp,
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
    texture_cache: HashMap<ImageId, HudTextureCacheEntry>,
    gpu_texture_cache: HashMap<String, HudTextureCacheEntry>,
}

struct HudTextureCacheEntry {
    version: u64,
    handle: TextureHandle,
    width: u32,
    height: u32,
    debug_hash: u64,
}

#[derive(Debug, Clone)]
struct HudGalleryTile {
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
    runtime_image_id: Option<ImageId>,
    image_id: Option<ImageId>,
    width: u32,
    height: u32,
    source_label: String,
    source_kind: String,
}

struct App {
    args: Args,
    initial_size: (u32, u32),
    boot: BootConfig,
    flow: ProcFlow,
    window: Option<&'static Window>,
    window_id: Option<WindowId>,
    renderer: Option<Renderer>,
    hud_window: Option<&'static Window>,
    hud_window_id: Option<WindowId>,
    hud_renderer: Option<Renderer>,
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
    next_frame_at: Instant,
    frame_dirty: bool,
    script_needs_pump: bool,
    script_resume_after_redraw: bool,
    captured: bool,
    pending_exit: bool,

    hud_show_active_textures: bool,
    hud_scroll: usize,
    hud_total_lines: usize,
    hud_gui: Option<HudGui>,
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
        ShiftLeft | ShiftRight => Some(VmKey::Shift),
        AltLeft | AltRight => Some(VmKey::Alt),

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
            initial_size,
            boot,
            flow,
            args,
            window: None,
            window_id: None,
            renderer: None,
            hud_window: None,
            hud_window_id: None,
            hud_renderer: None,
            vm: None,
            last_window_mode: None,
            last_window_size: None,
            last_cursor_hide_on: None,
            last_cursor_hide_time: None,
            cursor_hidden: false,
            last_mouse_move: Instant::now(),
            redraw_count: 0,
            next_frame_at: Instant::now(),
            frame_dirty: true,
            script_needs_pump: true,
            script_resume_after_redraw: false,
            captured: false,
            pending_exit: false,
            hud_show_active_textures: false,
            hud_scroll: 0,
            hud_total_lines: 0,
            hud_gui: None,
        }
    }

    fn clamp_hud_scroll(&mut self, visible_rows: usize) {
        let max_scroll = self.hud_total_lines.saturating_sub(visible_rows);
        if self.hud_scroll > max_scroll {
            self.hud_scroll = max_scroll;
        }
    }

    fn adjust_hud_scroll(&mut self, delta: isize, visible_rows: usize) {
        let max_scroll = self.hud_total_lines.saturating_sub(visible_rows) as isize;
        let next = (self.hud_scroll as isize + delta).clamp(0, max_scroll.max(0));
        self.hud_scroll = next as usize;
    }

    const HUD_STAGE_COUNT: i64 = 3;
    const HUD_OBJECT_COUNT: usize = 1024;
    const HUD_CARD_W_PX: u32 = 280;
    const HUD_CARD_H_PX: u32 = 300;
    const HUD_CARD_HEADER_PX: u32 = 96;

    fn hud_visible_rows(screen_h: u32) -> usize {
        ((screen_h.saturating_sub(112)) / Self::HUD_CARD_H_PX).max(1) as usize
    }

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

    fn hud_file_name_from_source_path(path: &Path) -> String {
        path.file_stem()
            .or_else(|| path.file_name())
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string())
    }

    fn hud_populate_image_info(
        vm: &SceneVm<'static>,
        image_id: ImageId,
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
            }
        }
    }

    fn collect_hud_tiles(vm: &mut SceneVm<'static>) -> Vec<HudGalleryTile> {
        let mut rows = Vec::new();
        let mut seen = HashSet::new();
        Self::collect_hud_tile_metadata_from_stage_forms(&*vm, &mut rows, &mut seen);
        Self::collect_hud_tile_metadata_from_runtime_probe(&*vm, &mut rows, &mut seen);
        Self::resolve_hud_tile_images(vm, &mut rows);
        rows.sort_by_key(|tile| (tile.stage_idx, tile.obj_idx));
        rows
    }

    fn hud_object_participates_in_tree(
        obj: &siglus_scene_vm::runtime::globals::ObjectState,
    ) -> bool {
        if obj.used {
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
        seen: &mut HashSet<(i64, usize)>,
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
        seen: &mut HashSet<(i64, usize)>,
        stage_form_id: u32,
        stage_idx: i64,
        obj_idx: usize,
        obj: &siglus_scene_vm::runtime::globals::ObjectState,
    ) {
        if !Self::hud_object_participates_in_tree(obj) {
            return;
        }

        let runtime_slot = obj.runtime_slot_or(obj_idx);
        let key = (stage_idx, runtime_slot);
        if seen.insert(key) {
            let mut disp = obj.base.disp != 0;
            let mut tr = obj.base.tr;
            let mut alpha = obj.base.alpha;
            let mut runtime_image_id = None;
            let mut patno = obj.base.patno;
            let mut width = 0u32;
            let mut height = 0u32;

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
                            if let Some(layer) = vm.ctx.layers.layer(lid) {
                                if let Some(sprite) = layer.sprite(sid) {
                                    // HUD must show the actual bound image. Visibility belongs to the object tree,
                                    // not to a stale layer flag, so keep `disp` from object/gfx state here.
                                    tr = sprite.tr as i64;
                                    alpha = sprite.alpha as i64;
                                    runtime_image_id = sprite.image_id;
                                }
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
                    if let Some(layer) = vm.ctx.layers.layer(*layer_id) {
                        if let Some(sprite) = layer.sprite(*sprite_id) {
                            tr = sprite.tr as i64;
                            alpha = sprite.alpha as i64;
                            runtime_image_id = sprite.image_id;
                        }
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
                        if let Some(layer) = vm.ctx.layers.layer(*layer_id) {
                            if let Some(sprite) = layer.sprite(sid) {
                                tr = sprite.tr as i64;
                                alpha = sprite.alpha as i64;
                                runtime_image_id = sprite.image_id;
                            }
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
            let mut tile = HudGalleryTile {
                stage_idx,
                stage_label: Self::hud_stage_name(stage_idx).to_string(),
                obj_idx: runtime_slot,
                file: file.clone(),
                backend,
                disp,
                tr,
                alpha,
                bind,
                patno,
                runtime_image_id,
                image_id: runtime_image_id,
                width,
                height,
                source_label: file,
                source_kind: if runtime_image_id.is_some() {
                    "runtime-bind".to_string()
                } else {
                    format!("stage-form-{}", stage_form_id)
                },
            };
            if let Some(image_id) = tile.runtime_image_id {
                Self::hud_populate_image_info(vm, image_id, &mut tile);
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
        seen: &mut HashSet<(i64, usize)>,
    ) {
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

                let key = (stage_idx, obj_idx);
                let runtime_image_id = sprite.image_id;
                let mut file = format!("<obj {}>", obj_idx);
                let mut source_label = format!("runtime L{}:S{}", layer_id, sprite_id);
                let mut width = 0u32;
                let mut height = 0u32;
                if let Some(image_id) = runtime_image_id {
                    if let Some(info) = vm.ctx.images.debug_image_info(image_id) {
                        width = info.width;
                        height = info.height;
                        if let Some(path) = info.source_path {
                            file = Self::hud_file_name_from_source_path(&path);
                            source_label = path.display().to_string();
                        }
                    }
                }

                if !seen.insert(key) {
                    if let Some(tile) = rows
                        .iter_mut()
                        .find(|tile| tile.stage_idx == stage_idx && tile.obj_idx == obj_idx)
                    {
                        tile.bind = format!("L{}:S{}", layer_id, sprite_id);
                        tile.disp = sprite.visible;
                        tile.tr = sprite.tr as i64;
                        tile.alpha = sprite.alpha as i64;
                        tile.patno = vm
                            .ctx
                            .gfx
                            .object_peek_patno(stage_idx, obj_idx as i64)
                            .unwrap_or(tile.patno);
                        tile.runtime_image_id = runtime_image_id.or(tile.runtime_image_id);
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
                            tile.image_id = tile.runtime_image_id;
                            tile.source_kind = "runtime-bind".to_string();
                        }
                    }
                    continue;
                }

                rows.push(HudGalleryTile {
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
                    runtime_image_id,
                    image_id: runtime_image_id,
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

    fn resolve_hud_tile_images(vm: &mut SceneVm<'static>, rows: &mut [HudGalleryTile]) {
        for tile in rows.iter_mut() {
            Self::resolve_hud_tile_image(vm, tile);
        }
    }

    fn resolve_hud_tile_image(vm: &mut SceneVm<'static>, tile: &mut HudGalleryTile) {
        tile.image_id = tile.runtime_image_id;
        if let Some(image_id) = tile.runtime_image_id {
            // For runtime-bound objects, the HUD must show the exact image submitted by the engine.
            // Do not replace it with file/patno 0 preview data.
            Self::hud_populate_image_info(vm, image_id, tile);
            tile.source_kind = "runtime-bind".to_string();
            return;
        }

        if tile.file.is_empty() || tile.file == "-" || tile.file.starts_with('<') {
            return;
        }

        match vm.ctx.images.load_g00(&tile.file, 0) {
            Ok(image_id) => {
                tile.image_id = Some(image_id);
                tile.source_kind = "preview-g00-0".to_string();
                Self::hud_populate_image_info(vm, image_id, tile);
            }
            Err(g00_err) => match vm.ctx.images.load_bg_frame(&tile.file, 0) {
                Ok(image_id) => {
                    tile.image_id = Some(image_id);
                    tile.source_kind = "preview-bg-0".to_string();
                    Self::hud_populate_image_info(vm, image_id, tile);
                }
                Err(bg_err) => {
                    if tile.image_id.is_none() {
                        tile.source_kind = "missing".to_string();
                        log::error!(
                            "HUD preview load failed: stage={} obj={} backend={} file={} bind={} runtime_pat={} g00_err={:#} bg_err={:#}",
                            tile.stage_idx,
                            tile.obj_idx,
                            tile.backend,
                            tile.file,
                            tile.bind,
                            tile.patno,
                            g00_err,
                            bg_err,
                        );
                    }
                }
            },
        }
    }

    fn hud_debug_rgb_preview(rgba: &[u8], width: u32, height: u32) -> (ColorImage, u64) {
        let pixel_count = width as usize * height as usize;
        let mut out = Vec::with_capacity(pixel_count.saturating_mul(4));
        let mut hash = 0xcbf29ce484222325u64;
        for (i, px) in rgba.chunks_exact(4).take(pixel_count).enumerate() {
            // HUD preview must expose decoded image contents, not normal game alpha semantics.
            // Force raw RGB opaque so fully transparent or incorrectly-alpha-decoded images are still visible.
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

    fn hud_alpha_summary(vm: &SceneVm<'static>, image_id: ImageId) -> Option<(u8, u8, usize)> {
        let (img, _) = vm.ctx.images.get_entry(image_id)?;
        let mut min_a = u8::MAX;
        let mut max_a = 0u8;
        let mut nonzero = 0usize;
        for px in img.rgba.chunks_exact(4) {
            let a = px[3];
            min_a = min_a.min(a);
            max_a = max_a.max(a);
            if a != 0 {
                nonzero += 1;
            }
        }
        Some((min_a, max_a, nonzero))
    }

    fn sync_hud_texture(
        gui: &mut HudGui,
        vm: &SceneVm<'static>,
        tile: &HudGalleryTile,
    ) -> Option<egui::TextureId> {
        let image_id = tile.image_id?;
        let (img, version) = vm.ctx.images.get_entry(image_id)?;
        let (color, debug_hash) =
            Self::hud_debug_rgb_preview(img.rgba.as_slice(), img.width, img.height);
        if let Some(entry) = gui.texture_cache.get_mut(&image_id) {
            if entry.version != version
                || entry.width != img.width
                || entry.height != img.height
                || entry.debug_hash != debug_hash
            {
                entry.handle.set(color, TextureOptions::LINEAR);
                entry.version = version;
                entry.width = img.width;
                entry.height = img.height;
                entry.debug_hash = debug_hash;
            }
            return Some(entry.handle.id());
        }
        let handle = gui.ctx.load_texture(
            format!("siglus-hud-debug-rgb-image-{}", image_id.0),
            color,
            TextureOptions::LINEAR,
        );
        let id = handle.id();
        gui.texture_cache.insert(
            image_id,
            HudTextureCacheEntry {
                version,
                handle,
                width: img.width,
                height: img.height,
                debug_hash,
            },
        );
        Some(id)
    }

    fn hud_debug_rgba_preview(rgba: &[u8], width: u32, height: u32) -> (ColorImage, u64) {
        let pixel_count = width as usize * height as usize;
        let mut out = Vec::with_capacity(pixel_count.saturating_mul(4));
        let mut hash = 0xcbf29ce484222325u64;
        for (i, px) in rgba.chunks_exact(4).take(pixel_count).enumerate() {
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
        for px in rgba.chunks_exact(4) {
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
        if !self.hud_show_active_textures {
            return Ok(());
        }
        let (size, scale) = {
            let Some(window) = self.hud_window.as_ref() else {
                return Ok(());
            };
            (window.inner_size(), window.scale_factor() as f32)
        };

        let textures = {
            let Some(renderer) = self.renderer.as_ref() else {
                return Ok(());
            };
            renderer.debug_read_render_chain_textures()?
        };

        let card_w_px = 340u32;
        let card_h_px = 360u32;
        let columns = ((size.width.saturating_sub(24)) / card_w_px).max(1) as usize;
        let visible_rows = ((size.height.saturating_sub(84)) / card_h_px).max(1) as usize;
        self.hud_total_lines = (textures.len() + columns.saturating_sub(1)) / columns.max(1);
        self.clamp_hud_scroll(visible_rows);

        let scroll = self.hud_scroll;
        let total_rows = self.hud_total_lines;
        let start = scroll.saturating_mul(columns);
        let end = (start + visible_rows.saturating_mul(columns)).min(textures.len());
        let card_w = card_w_px as f32 / scale;
        let card_h = card_h_px as f32 / scale;
        let thumb_w = card_w - 20.0;
        let thumb_h = 210.0;

        let image_count = textures.iter().filter(|t| t.kind == "image").count();
        let external_count = textures.iter().filter(|t| t.kind == "external").count();
        let target_count = textures
            .iter()
            .filter(|t| t.kind == "render-target")
            .count();
        let default_count = textures.iter().filter(|t| t.kind == "default").count();
        let usage_total: usize = textures.iter().map(|t| t.usage_count).sum();

        let mut visible_texture_ids = vec![None; end.saturating_sub(start)];
        let (ctx, raw_input) = {
            let Some(gui) = self.hud_gui.as_mut() else {
                return Ok(());
            };
            for (idx, texture) in textures[start..end].iter().enumerate() {
                visible_texture_ids[idx] = Self::sync_hud_gpu_texture(gui, texture);
            }
            gui.ctx.set_pixels_per_point(scale);
            let ctx = gui.ctx.clone();
            let raw_input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(size.width as f32 / scale, size.height as f32 / scale),
                )),
                time: Some(gui.start_time.elapsed().as_secs_f64()),
                ..Default::default()
            };
            (ctx, raw_input)
        };

        let output = ctx.run(raw_input, |ctx| {
            egui::TopBottomPanel::top("hud_top").show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.heading("Siglus texture HUD");
                    ui.separator();
                    ui.label(format!(
                        "textures={} usages={} image={} external={} target={} default={} rows={}/{} cols={} F2 hide, Wheel/PgUp/PgDn/Home/End scroll",
                        textures.len(),
                        usage_total,
                        image_count,
                        external_count,
                        target_count,
                        default_count,
                        scroll,
                        total_rows,
                        columns,
                    ));
                });
            });
            egui::CentralPanel::default().show(ctx, |ui| {
                if textures.is_empty() {
                    ui.label("no renderer GPU textures recorded for the current render chain");
                    return;
                }
                for (row_idx, row_textures) in textures[start..end].chunks(columns).enumerate() {
                    ui.horizontal_top(|ui| {
                        for (col_idx, texture) in row_textures.iter().enumerate() {
                            let tex_id = visible_texture_ids
                                .get(row_idx * columns + col_idx)
                                .copied()
                                .flatten();
                            ui.allocate_ui_with_layout(
                                egui::vec2(card_w, card_h),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    egui::Frame::group(ui.style()).show(ui, |ui| {
                                        ui.set_min_size(egui::vec2(card_w - 8.0, card_h - 8.0));
                                        ui.set_max_width(card_w - 8.0);
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "{}  {}",
                                                texture.kind,
                                                Self::shorten_for_hud(&texture.label, 34),
                                            ))
                                            .strong()
                                            .monospace(),
                                        );
                                        let (min_a, max_a, nonzero_a) =
                                            Self::hud_alpha_summary_rgba(texture.rgba.as_slice());
                                        ui.small(format!(
                                            "key={} size={}x{} ver={} alpha={}..{} nz={} usages={}",
                                            Self::shorten_for_hud(&texture.key, 44),
                                            texture.width,
                                            texture.height,
                                            texture.version,
                                            min_a,
                                            max_a,
                                            nonzero_a,
                                            texture.usage_count,
                                        ));
                                        ui.small("source=renderer GPU texture readback, preview=raw RGB forced opaque");

                                        let (rect, _) = ui.allocate_exact_size(
                                            egui::vec2(thumb_w, thumb_h),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().rect_filled(rect, 4.0, egui::Color32::from_gray(24));
                                        if let Some(tex_id) = tex_id {
                                            let mut draw_w = thumb_w;
                                            let mut draw_h = thumb_h;
                                            if texture.width > 0 && texture.height > 0 {
                                                let sx = thumb_w / texture.width as f32;
                                                let sy = thumb_h / texture.height as f32;
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
                                                egui::Image::new((tex_id, egui::vec2(draw_w, draw_h))),
                                            );
                                        } else {
                                            ui.painter().text(
                                                rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                "no texture",
                                                egui::FontId::proportional(16.0),
                                                egui::Color32::LIGHT_GRAY,
                                            );
                                        }

                                        ui.small(Self::shorten_for_hud(&texture.usage, 140));
                                    });
                                },
                            );
                        }
                    });
                }
            });
        });

        let screen_desc = ScreenDescriptor {
            size_in_pixels: [size.width, size.height],
            pixels_per_point: scale,
        };
        let paint_jobs = ctx.tessellate(output.shapes, scale);
        let (Some(renderer), Some(gui)) = (self.hud_renderer.as_mut(), self.hud_gui.as_mut())
        else {
            return Ok(());
        };
        for (id, delta) in &output.textures_delta.set {
            gui.renderer
                .update_texture(&renderer.device, &renderer.queue, *id, delta);
        }

        let frame = match renderer.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                renderer.resize(renderer.config.width, renderer.config.height);
                return Ok(());
            }
            Err(wgpu::SurfaceError::OutOfMemory) => anyhow::bail!("hud surface out of memory"),
            Err(wgpu::SurfaceError::Timeout) => return Ok(()),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = renderer
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("hud_egui_encoder"),
            });
        gui.renderer.update_buffers(
            &renderer.device,
            &renderer.queue,
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
                            r: 0.08,
                            g: 0.08,
                            b: 0.10,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            gui.renderer.render(&mut pass, &paint_jobs, &screen_desc);
        }
        renderer.queue.submit(Some(encoder.finish()));
        frame.present();
        for id in output.textures_delta.free {
            gui.renderer.free_texture(&id);
        }
        Ok(())
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
            "Gameexe.ini",
            "gameexe.dat",
            "gameexe.ini",
            "GameexeEN.dat",
            "GameexeEN.ini",
            "GameexeZH.dat",
            "GameexeZH.ini",
            "GameexeZHTW.dat",
            "GameexeZHTW.ini",
            "GameexeDE.dat",
            "GameexeDE.ini",
            "GameexeES.dat",
            "GameexeES.ini",
            "GameexeFR.dat",
            "GameexeFR.ini",
            "GameexeID.dat",
            "GameexeID.ini",
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
        if path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("ini"))
        {
            let text = String::from_utf8(raw).ok()?;
            return Some(GameexeConfig::from_text(&text));
        }
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

    fn consume_syscom_pending_proc(&mut self) -> Result<bool> {
        let Some(proc) = ({
            let Some(vm) = self.vm.as_mut() else {
                return Ok(false);
            };
            let proc = vm.ctx.globals.syscom.pending_proc.take();
            if proc.is_some() {
                vm.ctx.globals.syscom.menu_open = false;
                vm.ctx.globals.syscom.menu_kind = None;
                vm.ctx.globals.syscom.msg_back_open = false;
            }
            proc
        }) else {
            return Ok(false);
        };

        match proc.kind {
            SyscomPendingProcKind::ReturnToMenu => {
                if proc.warning {
                    self.begin_syscom_warning(proc);
                } else {
                    self.queue_return_to_menu_proc(proc);
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
            self.flow.push(ProcType::Script, 0);
        }
    }

    fn begin_syscom_warning(&mut self, mut proc: SyscomPendingProc) {
        let Some(vm) = self.vm.as_mut() else {
            return;
        };
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
        let text = Self::return_to_menu_warning_text(vm);
        let native_pending = vm.ctx.native_ui_backend.is_some();
        vm.ctx.globals.system.messagebox_modal = Some(SystemMessageBoxModalState {
            request_id,
            kind: 19,
            text: text.clone(),
            debug_only: false,
            buttons: buttons.clone(),
            cursor: 1,
            native_pending,
        });
        if let Some(backend) = vm.ctx.native_ui_backend.as_ref() {
            backend.show_system_messagebox(native_ui::NativeMessageBoxRequest {
                request_id,
                kind: native_ui::NativeMessageBoxKind::YesNo,
                title: vm.ctx.game_title(),
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
        }
        self.flow.push(ProcType::SyscomWarning, 0);
    }

    fn return_to_menu_warning_text(vm: &SceneVm<'static>) -> String {
        vm.ctx
            .tables
            .gameexe
            .as_ref()
            .and_then(|cfg| cfg.get_unquoted("WARNINGINFO.RETURNMENU_WARNING_STR"))
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "Return to title menu?".to_string())
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
            if let Some(pair) = cfg.and_then(|c| c.get_unquoted(key)).and_then(parse_pair) {
                return pair;
            }
        }
        (0, 1000)
    }

    fn queue_return_to_menu_proc(&mut self, proc: SyscomPendingProc) {
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
        vm.ctx.globals.start_wipe(WipeState::new(
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

    fn perform_return_to_menu(&mut self, leave_msgbk: bool) -> Result<()> {
        let target_scene = self
            .boot
            .menu_scene
            .as_deref()
            .unwrap_or(self.boot.start_scene.as_str())
            .to_string();
        let target_z = if self.boot.menu_scene.is_some() {
            self.boot.menu_z
        } else {
            self.boot.start_z
        };
        let Some(vm) = self.vm.as_mut() else {
            return Ok(());
        };
        let saved_msgbk = if leave_msgbk {
            Some(vm.ctx.globals.msgbk_forms.clone())
        } else {
            None
        };
        vm.restart_scene_name(&target_scene, target_z)?;
        if let Some(msgbk) = saved_msgbk {
            vm.ctx.globals.msgbk_forms = msgbk;
        }
        vm.ctx.globals.finish_wipe();
        self.flow.stack.clear();
        self.flow.pending_syscom_proc = None;
        self.flow.booted_menu = true;
        self.flow.push(ProcType::GameTimerStart, 0);
        self.flow.push(ProcType::Script, 0);
        Ok(())
    }

    fn pump_vm(&mut self) -> Result<()> {
        self.script_needs_pump = false;
        self.ensure_requested_script_proc();
        if self.vm.is_none() {
            return Ok(());
        }

        if self.paused && !self.step_once {
            return Ok(());
        }

        if let Some(vm) = self.vm.as_mut() {
            vm.begin_script_proc_pump();
        }

        // Match the original C++ frame_main_proc(): keep advancing the proc
        // stack until the active proc asks to break for this frame. Script
        // execution itself is boundary-driven; there is no instruction quota.
        loop {
            let Some(proc) = self.flow.top().cloned() else {
                self.paused = true;
                break;
            };

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
                    ) = {
                        let vm = self.vm.as_mut().expect("vm checked");
                        let proc_gen_before = vm.proc_generation();
                        let running = vm.run_script_proc_continue()?;
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
                        )
                    };
                    self.ensure_requested_script_proc();
                    if pop_script_proc {
                        if std::env::var_os("SG_DEBUG").is_some() {
                            eprintln!(
                                "[SG_DEBUG][EXCALL] pop SCRIPT proc requested by ex-call return"
                            );
                        }
                        self.flow.pop();
                        continue;
                    }
                    if !running || halted {
                        self.flow.pop();
                        if !self.flow.booted_menu
                            && cur_scene == self.boot.start_scene
                            && self.boot.menu_scene.is_some()
                        {
                            self.flow.push(ProcType::ReturnToMenu, 0);
                        }
                        continue;
                    }
                    if pending {
                        if self.consume_syscom_pending_proc()? {
                            continue;
                        }
                        let blocked = self.vm.as_mut().map(|vm| vm.is_blocked()).unwrap_or(false);
                        if blocked {
                            break;
                        }
                    } else if blocked {
                        break;
                    } else if proc_boundary {
                        match boundary_kind {
                            // C++ frame_main_proc consumes DISP immediately and then breaks
                            // out to the renderer. The SCRIPT proc remains underneath and is
                            // resumed on the next frame.
                            ProcKind::Disp => {
                                self.script_resume_after_redraw = true;
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
                                continue;
                            }
                        }
                    }
                }
                ProcType::StartWarning => {
                    let warning_exists = {
                        let vm = self.vm.as_mut().expect("vm checked");
                        vm.ctx
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
                                .exists()
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
                                SyscomPendingProcKind::ReturnToMenu => {
                                    self.queue_return_to_menu_proc(proc);
                                }
                                _ => {}
                            }
                        }
                    }
                    continue;
                }
                ProcType::Disp => {
                    self.flow.pop();
                    self.script_resume_after_redraw = true;
                    break;
                }
                ProcType::GameEndWipe => {
                    let mut start = false;
                    if let Some(top) = self.flow.top_mut() {
                        if top.option == 0 {
                            top.option = 1;
                            start = true;
                        }
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
        if wait_poll_needed {
            if let Some(vm) = self.vm.as_mut() {
                if !vm.is_blocked() {
                    self.script_needs_pump = true;
                    self.next_frame_at = Instant::now();
                }
            }
        }
        self.ensure_requested_script_proc();
        let Some(vm) = self.vm.as_mut() else {
            return Ok(());
        };
        let list = vm.ctx.render_list_with_effects();

        {
            let Some(renderer) = self.renderer.as_mut() else {
                return Ok(());
            };
            renderer.render_sprites(&vm.ctx.images, &list)?;
        }

        if self.script_resume_after_redraw {
            self.script_resume_after_redraw = false;
            self.script_needs_pump = true;
        }

        self.redraw_count = self.redraw_count.saturating_add(1);
        self.next_frame_at = Instant::now() + FRAME_INTERVAL;
        self.maybe_capture_current_frame()?;
        if self.hud_show_active_textures {
            if let Some(w) = self.hud_window.as_ref() {
                w.request_redraw();
            }
        }

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
            let img = vm.ctx.capture_frame_rgba();
            let render_list = vm.ctx.render_list_with_effects();
            let nonzero_alpha = img.rgba.chunks_exact(4).filter(|px| px[3] != 0).count();
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
            let _ = (nw, nh);
            self.last_window_size = Some(size_mode);
        }

        let hide_on = Self::syscom_int(&vm.ctx, GET_MOUSE_CURSOR_HIDE_ONOFF, 0);
        let hide_time = Self::syscom_int(&vm.ctx, GET_MOUSE_CURSOR_HIDE_TIME, 0);
        if vm.ctx.globals.script.cursor_disp_off {
            w.set_cursor_visible(false);
            self.cursor_hidden = true;
            self.last_cursor_hide_on = Some(hide_on);
            self.last_cursor_hide_time = Some(hide_time);
            return;
        }
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
        if hide_on == 0 && self.cursor_hidden {
            w.set_cursor_visible(true);
            self.cursor_hidden = false;
        }

        if hide_on != 0 && hide_time > 0 {
            let elapsed_ms = self.last_mouse_move.elapsed().as_millis() as i64;
            if elapsed_ms >= hide_time && !self.cursor_hidden {
                w.set_cursor_visible(false);
                self.cursor_hidden = true;
            }
        }
    }
    fn wake_for_input(&mut self) {
        self.frame_dirty = true;
        self.script_needs_pump = true;
        self.next_frame_at = Instant::now();
        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
    }

    fn needs_continuous_frame(&self) -> bool {
        if self.pending_exit {
            return false;
        }
        if self.flow.top().map(|p| p.ty == ProcType::TimeWait).unwrap_or(false) {
            return true;
        }
        self.vm
            .as_ref()
            .map(|vm| vm.ctx.needs_continuous_frame())
            .unwrap_or(false)
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, elwt: &ActiveEventLoop) {
        let size = LogicalSize::new(self.initial_size.0 as f64, self.initial_size.1 as f64);
        let title = Self::resolve_project_dir(&self.args)
            .as_deref()
            .map(siglus_scene_vm::runtime::game_display_info::resolve_game_name_from_project_dir)
            .unwrap_or_else(|| "Siglus Engine".to_string());
        let window = elwt
            .create_window(
                WindowAttributes::default()
                    .with_inner_size(size)
                    .with_title(title),
            )
            .expect("create window");
        let window: &'static Window = Box::leak(Box::new(window));
        let hud_window = elwt
            .create_window(
                WindowAttributes::default()
                    .with_inner_size(LogicalSize::new(1280.0, 900.0))
                    .with_title("Siglus HUD")
                    .with_visible(false),
            )
            .expect("create hud window");
        let hud_window: &'static Window = Box::leak(Box::new(hud_window));

        let renderer = pollster::block_on(Renderer::new(window)).expect("renderer init");
        let hud_renderer =
            pollster::block_on(Renderer::new(hud_window)).expect("hud renderer init");
        let hud_gui = HudGui {
            ctx: egui::Context::default(),
            renderer: EguiRenderer::new(&hud_renderer.device, hud_renderer.config.format, None, 1),
            start_time: Instant::now(),
            texture_cache: HashMap::new(),
            gpu_texture_cache: HashMap::new(),
        };
        let vm = self.init_vm().expect("vm init");

        self.window_id = Some(window.id());
        self.window = Some(window);
        self.renderer = Some(renderer);
        self.hud_window_id = Some(hud_window.id());
        self.hud_window = Some(hud_window);
        self.hud_renderer = Some(hud_renderer);
        self.hud_gui = Some(hud_gui);
        self.vm = Some(vm);

        if let Some(w) = self.window.as_ref() {
            w.request_redraw();
        }
        if self.hud_show_active_textures {
            if let Some(w) = self.hud_window.as_ref() {
                w.request_redraw();
            }
        }
    }

    fn window_event(
        &mut self,
        elwt: &ActiveEventLoop,
        id: winit::window::WindowId,
        event: WindowEvent,
    ) {
        let is_main = self.window_id == Some(id);
        let is_hud = self.hud_window_id == Some(id);
        if !is_main && !is_hud {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                if is_hud {
                    self.hud_show_active_textures = false;
                    self.hud_scroll = 0;
                    if let Some(gui) = self.hud_gui.as_mut() {
                        gui.gpu_texture_cache.clear();
                    }
                    if let Some(w) = self.hud_window.as_ref() {
                        w.set_visible(false);
                    }
                    return;
                }
                if let Some(vm) = self.vm.as_ref() {
                    eprintln!("[SG_UNKNOWN]\n{}", vm.ctx.unknown.summary_string(2048));
                }
                elwt.exit();
            }
            WindowEvent::Resized(size) => {
                if is_hud {
                    if let Some(renderer) = self.hud_renderer.as_mut() {
                        renderer.resize_with_scale(
                            size.width,
                            size.height,
                            self.hud_window
                                .as_ref()
                                .map(|w| w.scale_factor() as f32)
                                .unwrap_or(1.0),
                        );
                    }
                    if let Some(w) = self.hud_window.as_ref() {
                        w.request_redraw();
                    }
                } else {
                    if let Some(renderer) = self.renderer.as_mut() {
                        renderer.resize_with_scale(
                            size.width,
                            size.height,
                            self.window
                                .as_ref()
                                .map(|w| w.scale_factor() as f32)
                                .unwrap_or(1.0),
                        );
                    }
                    if let Some(w) = self.window.as_ref() {
                        w.request_redraw();
                    }
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
                let hud_rows = self
                    .hud_window
                    .as_ref()
                    .map(|w| Self::hud_visible_rows(w.inner_size().height))
                    .unwrap_or(24);
                let hud_handled = match code {
                    KeyCode::F2 => {
                        self.hud_show_active_textures = !self.hud_show_active_textures;
                        if !self.hud_show_active_textures {
                            self.hud_scroll = 0;
                            if let Some(gui) = self.hud_gui.as_mut() {
                                gui.gpu_texture_cache.clear();
                            }
                        }
                        if let Some(w) = self.hud_window.as_ref() {
                            w.set_visible(self.hud_show_active_textures);
                            if self.hud_show_active_textures {
                                if let Some(main_window) = self.window.as_ref() {
                                    main_window.request_redraw();
                                }
                                w.request_redraw();
                            }
                        }
                        true
                    }
                    KeyCode::PageDown if self.hud_show_active_textures => {
                        self.adjust_hud_scroll(10, hud_rows);
                        true
                    }
                    KeyCode::PageUp if self.hud_show_active_textures => {
                        self.adjust_hud_scroll(-10, hud_rows);
                        true
                    }
                    KeyCode::Home if self.hud_show_active_textures => {
                        self.hud_scroll = 0;
                        true
                    }
                    KeyCode::End if self.hud_show_active_textures => {
                        self.hud_scroll = self.hud_total_lines;
                        self.clamp_hud_scroll(hud_rows);
                        true
                    }
                    KeyCode::ArrowDown if self.hud_show_active_textures => {
                        self.adjust_hud_scroll(1, hud_rows);
                        true
                    }
                    KeyCode::ArrowUp if self.hud_show_active_textures => {
                        self.adjust_hud_scroll(-1, hud_rows);
                        true
                    }
                    _ => false,
                };
                if hud_handled {
                    if self.hud_show_active_textures {
                        if let Some(w) = self.hud_window.as_ref() {
                            w.request_redraw();
                        }
                    }
                    return;
                }

                if !is_main {
                    return;
                }

                if let Some(vm) = self.vm.as_mut() {
                    if let Some(k) = map_keycode(code) {
                        vm.ctx.on_key_down(k);
                    } else {
                        vm.ctx.notify_wait_key();
                    }
                    if let Some(t) = text.as_ref() {
                        if t.chars().any(|c| !c.is_control()) {
                            vm.ctx.on_text_input(t);
                        }
                    }
                }

                self.wake_for_input();
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
                if code == KeyCode::F2 {
                    return;
                }
                if self.hud_show_active_textures
                    && matches!(
                        code,
                        KeyCode::PageDown
                            | KeyCode::PageUp
                            | KeyCode::Home
                            | KeyCode::End
                            | KeyCode::ArrowDown
                            | KeyCode::ArrowUp
                    )
                {
                    return;
                }
                if !is_main {
                    return;
                }
                if let Some(vm) = self.vm.as_mut() {
                    if let Some(k) = map_keycode(code) {
                        vm.ctx.on_key_up(k);
                    }
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
            WindowEvent::RedrawRequested => {
                let res = if is_hud {
                    self.redraw_hud_window()
                } else {
                    self.redraw()
                };
                if let Err(e) = res {
                    eprintln!("render error: {e:?}");
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                if !is_main {
                    return;
                }
                if let Some(vm) = self.vm.as_mut() {
                    let (x, y) = if let Some(w) = self.window.as_ref() {
                        let p = position.to_logical::<f64>(w.scale_factor());
                        (p.x.round() as i32, p.y.round() as i32)
                    } else {
                        (position.x.round() as i32, position.y.round() as i32)
                    };
                    vm.ctx.on_mouse_move(x, y);
                }
                self.last_mouse_move = Instant::now();
                self.wake_for_input();
                let force_cursor_hidden = self
                    .vm
                    .as_ref()
                    .map(|vm| vm.ctx.globals.script.cursor_disp_off)
                    .unwrap_or(false);
                if self.cursor_hidden && !force_cursor_hidden {
                    if let Some(w) = self.window.as_ref() {
                        w.set_cursor_visible(true);
                    }
                    self.cursor_hidden = false;
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_lx, ly) => (ly * 120.0) as i32,
                    MouseScrollDelta::PixelDelta(p) => p.y.round() as i32,
                };
                if is_hud && self.hud_show_active_textures {
                    let hud_rows = self
                        .hud_renderer
                        .as_ref()
                        .map(|r| Self::hud_visible_rows(r.config.height))
                        .unwrap_or(24);
                    if dy < 0 {
                        self.adjust_hud_scroll(3, hud_rows);
                    } else if dy > 0 {
                        self.adjust_hud_scroll(-3, hud_rows);
                    }
                    if let Some(w) = self.hud_window.as_ref() {
                        w.request_redraw();
                    }
                    return;
                }
                if !is_main {
                    return;
                }
                if let Some(vm) = self.vm.as_mut() {
                    vm.ctx.on_mouse_wheel(dy);
                }
                self.wake_for_input();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if !is_main {
                    return;
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

    fn about_to_wait(&mut self, elwt: &ActiveEventLoop) {
        if self.pending_exit {
            elwt.exit();
            return;
        }

        let capture_pending = self.args.capture_png.is_some() && !self.captured;
        let continuous_before = self.needs_continuous_frame();
        let now = Instant::now();
        let wants_frame_or_script =
            self.frame_dirty
                || self.script_needs_pump
                || self.script_resume_after_redraw
                || continuous_before
                || capture_pending;
        if wants_frame_or_script && !capture_pending && now < self.next_frame_at {
            elwt.set_control_flow(ControlFlow::WaitUntil(self.next_frame_at));
            return;
        }

        let should_pump_script = self.script_needs_pump || capture_pending;
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
            self.frame_dirty = true;
        }

        let continuous_after = self.needs_continuous_frame();
        let now_after = Instant::now();
        if !self.frame_dirty
            && !self.script_needs_pump
            && !self.script_resume_after_redraw
            && !capture_pending
            && continuous_after
            && now_after < self.next_frame_at
        {
            elwt.set_control_flow(ControlFlow::WaitUntil(self.next_frame_at));
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
            if self.hud_show_active_textures {
                if let Some(w) = self.hud_window.as_ref() {
                    w.request_redraw();
                }
            }
            self.frame_dirty = false;
            if capture_pending {
                elwt.set_control_flow(ControlFlow::Wait);
            } else {
                let wake_at = self.next_frame_at.max(Instant::now());
                elwt.set_control_flow(ControlFlow::WaitUntil(wake_at));
            }
        } else {
            elwt.set_control_flow(ControlFlow::Wait);
        }
    }
}

fn main() -> Result<()> {
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("error"))
        .try_init();
    let args = Args::parse();
    if args.capture_png.is_some() && args.exit_after_capture {
        return run_headless_capture(args);
    }
    let el = EventLoop::new()?;
    let mut app = App::new(args);
    el.run_app(&mut app)?;
    Ok(())
}

fn run_headless_capture(args: Args) -> Result<()> {
    let mut app = App::new(args);
    let vm = app.init_vm()?;
    app.vm = Some(vm);

    let capture_target = app.args.capture_after_frames.max(1);
    let max_frames = capture_target.saturating_add(600);
    for _ in 0..max_frames {
        app.pump_vm()?;
        let mut injected_wait_click = false;
        if let Some(vm) = app.vm.as_mut() {
            if vm.is_blocked() {
                let x = ((vm.ctx.screen_w / 2).min(i32::MAX as u32)) as i32;
                let y = ((vm.ctx.screen_h / 2).min(i32::MAX as u32)) as i32;
                vm.ctx.on_mouse_move(x, y);
                vm.ctx.on_mouse_down(VmMouseButton::Left);
                vm.ctx.on_mouse_up(VmMouseButton::Left);
                injected_wait_click = true;
            }
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
