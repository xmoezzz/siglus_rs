//! Host-driven Siglus runtime entry points shared by desktop pump and mobile FFI.
//!
//! This module deliberately keeps platform event-loop code out of the VM.  A host owns
//! the native event loop or view/surface and calls into this driver for one frame at a
//! time.  The VM semantics are the same proc-stack loop used by the desktop winit
//! shell: script execution runs until an original-engine boundary asks to present a
//! frame, wait for input, or wait for runtime work.

use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use siglus_assets::gameexe::{decode_gameexe_dat_bytes, GameexeConfig, GameexeDecodeOptions};
use siglus_assets::scene_pck::{find_scene_pck_in_project, ScenePck, ScenePckDecodeOptions};

use crate::render::Renderer;
use crate::runtime::globals::{
    SyscomPendingProc, SyscomPendingProcKind, SystemMessageBoxButton, SystemMessageBoxModalState,
    WipeState,
};
use crate::runtime::input::{VmKey, VmMouseButton};
use crate::runtime::{native_ui, CommandContext, ProcKind};
use crate::scene_stream::SceneStream;
use crate::vm::{SceneVm, VmConfig};

const FRAME_INTERVAL_MS: u32 = 16;

#[derive(Debug, Clone)]
pub struct SiglusHostConfig {
    pub project_dir: PathBuf,
    pub scene_name: Option<String>,
    pub scene_id: Option<usize>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

impl SiglusHostConfig {
    pub fn new(project_dir: PathBuf) -> Self {
        Self {
            project_dir,
            scene_name: None,
            scene_id: None,
            width: None,
            height: None,
        }
    }
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

    fn top(&self) -> Option<&ProcFrame> {
        self.stack.last()
    }

    fn top_mut(&mut self) -> Option<&mut ProcFrame> {
        self.stack.last_mut()
    }
}

/// Button values follow SYSTEM.MESSAGEBOX_* VM semantics.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiglusNativeMessageBoxKind {
    Ok = 0,
    OkCancel = 1,
    YesNo = 2,
    YesNoCancel = 3,
}

impl From<native_ui::NativeMessageBoxKind> for SiglusNativeMessageBoxKind {
    fn from(value: native_ui::NativeMessageBoxKind) -> Self {
        match value {
            native_ui::NativeMessageBoxKind::Ok => Self::Ok,
            native_ui::NativeMessageBoxKind::OkCancel => Self::OkCancel,
            native_ui::NativeMessageBoxKind::YesNo => Self::YesNo,
            native_ui::NativeMessageBoxKind::YesNoCancel => Self::YesNoCancel,
        }
    }
}

/// Callback used by mobile hosts to show a native dialog on the platform UI thread.
///
/// All string pointers are valid only for the duration of the callback.  The host must
/// copy them before returning if it needs to keep them.  The selected button must be
/// delivered later with the platform-specific `siglus_*_submit_messagebox_result` ABI.
pub type SiglusNativeMessageBoxCallback = unsafe extern "C" fn(
    user_data: *mut c_void,
    request_id: u64,
    kind: i32,
    title_utf8: *const c_char,
    message_utf8: *const c_char,
);

struct CNativeUiBackend {
    callback: SiglusNativeMessageBoxCallback,
    user_data: usize,
}

unsafe impl Send for CNativeUiBackend {}
unsafe impl Sync for CNativeUiBackend {}

impl native_ui::NativeUiBackend for CNativeUiBackend {
    fn show_system_messagebox(&self, request: native_ui::NativeMessageBoxRequest) {
        let title = CString::new(request.title).unwrap_or_else(|_| CString::new("Siglus").unwrap());
        let message = CString::new(request.message).unwrap_or_else(|_| CString::new("").unwrap());
        let kind: SiglusNativeMessageBoxKind = request.kind.into();
        unsafe {
            (self.callback)(
                self.user_data as *mut c_void,
                request.request_id,
                kind as i32,
                title.as_ptr(),
                message.as_ptr(),
            );
        }
    }
}

pub struct SiglusHost {
    config: SiglusHostConfig,
    boot: BootConfig,
    flow: ProcFlow,
    renderer: Renderer,
    vm: SceneVm<'static>,
    redraw_count: u32,
    script_needs_pump: bool,
    script_resume_after_redraw: bool,
    paused: bool,
    pending_exit: bool,
    last_step: Option<Instant>,
}

impl SiglusHost {
    pub async fn new_with_renderer(config: SiglusHostConfig, renderer: Renderer) -> Result<Self> {
        let initial_size = Self::resolve_initial_size(&config);
        let boot = Self::resolve_boot_config(&config);
        let mut flow = ProcFlow::default();
        flow.push(ProcType::Script, 0);
        flow.push(ProcType::StartWarning, 0);
        let vm = Self::init_vm(&config, &boot, initial_size)?;
        Ok(Self {
            config,
            boot,
            flow,
            renderer,
            vm,
            redraw_count: 0,
            script_needs_pump: true,
            script_resume_after_redraw: false,
            paused: false,
            pending_exit: false,
            last_step: None,
        })
    }

    pub fn set_native_messagebox_callback(
        &mut self,
        callback: Option<SiglusNativeMessageBoxCallback>,
        user_data: *mut c_void,
    ) {
        let backend = callback.map(|cb| {
            Arc::new(CNativeUiBackend {
                callback: cb,
                user_data: user_data as usize,
            }) as Arc<dyn native_ui::NativeUiBackend>
        });
        self.vm.ctx.set_native_ui_backend(backend);
    }

    pub fn submit_native_messagebox_result(&mut self, request_id: u64, value: i64) {
        self.vm.ctx.submit_native_messagebox_result(request_id, value);
        self.script_needs_pump = true;
    }

    pub fn resize(&mut self, width: u32, height: u32, scale_factor: f32) {
        self.renderer.resize_with_scale(width, height, scale_factor.max(1.0));
        let logical_w = ((width as f32) / scale_factor.max(1.0)).max(1.0).round() as u32;
        let logical_h = ((height as f32) / scale_factor.max(1.0)).max(1.0).round() as u32;
        self.vm.ctx.set_screen_size(logical_w, logical_h);
        self.script_needs_pump = true;
    }

    /// Step one frame and present when needed. Returns true if the engine requested exit.
    pub fn step(&mut self, dt_ms: u32) -> Result<bool> {
        let _ = dt_ms;
        self.last_step = Some(Instant::now());
        if self.script_needs_pump || self.vm.ctx.wait.needs_runtime_poll() {
            self.pump_vm()?;
        }
        self.redraw()?;
        Ok(self.pending_exit || self.vm.is_halted())
    }

    pub fn mouse_move(&mut self, x: f64, y: f64) {
        self.vm.ctx.on_mouse_move(x.round() as i32, y.round() as i32);
        self.script_needs_pump = true;
    }

    pub fn mouse_down(&mut self, button: VmMouseButton) {
        self.vm.ctx.on_mouse_down(button);
        self.script_needs_pump = true;
    }

    pub fn mouse_up(&mut self, button: VmMouseButton) {
        self.vm.ctx.on_mouse_up(button);
        self.script_needs_pump = true;
    }

    pub fn mouse_wheel(&mut self, delta_y: i32) {
        self.vm.ctx.on_mouse_wheel(delta_y);
        self.script_needs_pump = true;
    }

    pub fn touch(&mut self, phase: i32, x: f64, y: f64) {
        self.mouse_move(x, y);
        match phase {
            0 => self.mouse_down(VmMouseButton::Left),
            1 => {}
            2 | 3 => self.mouse_up(VmMouseButton::Left),
            _ => {}
        }
    }

    pub fn key_down(&mut self, key: VmKey) {
        self.vm.ctx.on_key_down(key);
        self.script_needs_pump = true;
    }

    pub fn key_up(&mut self, key: VmKey) {
        self.vm.ctx.on_key_up(key);
        self.script_needs_pump = true;
    }

    pub fn text_input(&mut self, text: &str) {
        self.vm.ctx.on_text_input(text);
        self.script_needs_pump = true;
    }

    pub fn renderer_mut(&mut self) -> &mut Renderer {
        &mut self.renderer
    }

    pub fn vm_mut(&mut self) -> &mut SceneVm<'static> {
        &mut self.vm
    }

    fn resolve_initial_size(config: &SiglusHostConfig) -> (u32, u32) {
        let cfg_size = Self::try_load_gameexe(&config.project_dir)
            .as_ref()
            .and_then(Self::gameexe_screen_size)
            .unwrap_or((1280, 720));
        (
            config.width.unwrap_or(cfg_size.0),
            config.height.unwrap_or(cfg_size.1),
        )
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

    fn resolve_boot_config(config: &SiglusHostConfig) -> BootConfig {
        let cfg = Self::try_load_gameexe(&config.project_dir);
        let (default_start, default_start_z) = cfg
            .as_ref()
            .and_then(|cfg| Self::gameexe_scene_entry(cfg, "START_SCENE"))
            .unwrap_or_else(|| ("_start".to_string(), 0));
        let (menu_scene, menu_z) = cfg
            .as_ref()
            .and_then(|cfg| Self::gameexe_scene_entry(cfg, "MENU_SCENE"))
            .map(|(s, z)| (Some(s), z))
            .unwrap_or((None, 0));
        BootConfig {
            start_scene: config.scene_name.clone().unwrap_or(default_start),
            start_z: default_start_z,
            menu_scene,
            menu_z,
        }
    }

    fn find_gameexe_path(project_dir: &Path) -> Option<PathBuf> {
        let candidates = [
            "Gameexe.dat", "Gameexe.ini", "gameexe.dat", "gameexe.ini", "GameexeEN.dat",
            "GameexeEN.ini", "GameexeZH.dat", "GameexeZH.ini", "GameexeZHTW.dat",
            "GameexeZHTW.ini", "GameexeDE.dat", "GameexeDE.ini", "GameexeES.dat",
            "GameexeES.ini", "GameexeFR.dat", "GameexeFR.ini", "GameexeID.dat",
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

    fn init_vm(
        config: &SiglusHostConfig,
        boot: &BootConfig,
        initial_size: (u32, u32),
    ) -> Result<SceneVm<'static>> {
        let project_dir = config.project_dir.clone();
        let scene_pck_path = find_scene_pck_in_project(&project_dir)?;
        let opt = ScenePckDecodeOptions::from_project_dir(&project_dir)?;
        let pck = ScenePck::load_and_rebuild(&scene_pck_path, &opt)
            .with_context(|| format!("open scene.pck: {}", scene_pck_path.display()))?;

        let scene_no = if let Some(id) = config.scene_id {
            id
        } else if let Some(name) = config.scene_name.as_ref() {
            pck.find_scene_no(name).unwrap_or(0)
        } else {
            pck.find_scene_no(&boot.start_scene).unwrap_or(0)
        };

        let chunk = pck
            .scn_data_slice(scene_no)
            .with_context(|| format!("scene_id out of range: {}", scene_no))?;
        let chunk_leaked: &'static [u8] = Box::leak(chunk.to_vec().into_boxed_slice());
        let mut stream = SceneStream::new(chunk_leaked)?;
        let start_z = if config.scene_id.is_some() || config.scene_name.is_some() {
            0
        } else {
            boot.start_z
        };
        stream.jump_to_z_label(start_z.max(0) as usize)?;
        let mut ctx = CommandContext::new(project_dir);
        ctx.screen_w = initial_size.0;
        ctx.screen_h = initial_size.1;
        let mut vm = SceneVm::with_config(VmConfig::from_env(), stream, ctx);
        if config.scene_id.is_none() {
            let scene_name = config
                .scene_name
                .clone()
                .unwrap_or_else(|| boot.start_scene.clone());
            vm.restart_scene_name(&scene_name, start_z)?;
        }
        Ok(vm)
    }

    fn consume_syscom_pending_proc(&mut self) -> Result<bool> {
        let Some(proc) = self.vm.ctx.globals.syscom.pending_proc.take() else {
            return Ok(false);
        };

        self.vm.ctx.globals.syscom.menu_open = false;
        self.vm.ctx.globals.syscom.menu_kind = None;
        self.vm.ctx.globals.syscom.msg_back_open = false;

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
                if self.vm.restore_last_sel_point() {
                    self.flow.stack.clear();
                    self.flow.push(ProcType::GameTimerStart, 0);
                    self.flow.push(ProcType::Script, 0);
                    Ok(true)
                } else {
                    self.vm.ctx.unknown.record_note(
                        "SYSCOM.RETURN_TO_SEL requested without an in-memory SELPOINT snapshot",
                    );
                    Ok(false)
                }
            }
            SyscomPendingProcKind::BacklogLoad => {
                if self.vm.restore_last_sel_point() {
                    self.flow.stack.clear();
                    self.flow.push(ProcType::GameTimerStart, 0);
                    self.flow.push(ProcType::Script, 0);
                    Ok(true)
                } else {
                    self.vm.ctx.unknown.record_note(&format!(
                        "SYSCOM.MSG_BACK_LOAD requested but backlog save {} is not materialized without SAVE/LOAD support",
                        proc.save_id
                    ));
                    Ok(false)
                }
            }
        }
    }

    fn ensure_requested_script_proc(&mut self) {
        let requested = self.vm.take_script_proc_request();
        if requested {
            self.flow.push(ProcType::Script, 0);
        }
    }

    fn begin_syscom_warning(&mut self, mut proc: SyscomPendingProc) {
        proc.warning = false;
        self.flow.pending_syscom_proc = Some(proc);
        self.vm.ctx.globals.system.messagebox_modal_result = None;
        let request_id = self.vm.ctx.native_ui.next_messagebox_request_id();
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
        let text = self.return_to_menu_warning_text();
        let native_pending = self.vm.ctx.native_ui_backend.is_some();
        self.vm.ctx.globals.system.messagebox_modal = Some(SystemMessageBoxModalState {
            request_id,
            kind: 19,
            text: text.clone(),
            debug_only: false,
            buttons: buttons.clone(),
            cursor: 1,
            native_pending,
        });
        if let Some(backend) = self.vm.ctx.native_ui_backend.as_ref() {
            backend.show_system_messagebox(native_ui::NativeMessageBoxRequest {
                request_id,
                kind: native_ui::NativeMessageBoxKind::YesNo,
                title: self.vm.ctx.game_title(),
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

    fn return_to_menu_warning_text(&self) -> String {
        self.vm
            .ctx
            .tables
            .gameexe
            .as_ref()
            .and_then(|cfg| cfg.get_unquoted("WARNINGINFO.RETURNMENU_WARNING_STR"))
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "Return to title menu?".to_string())
    }

    fn load_wipe_params(&self) -> (i32, i32) {
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
        let cfg = self.vm.ctx.tables.gameexe.as_ref();
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
        let (wipe_type, wipe_time) = self.load_wipe_params();
        self.vm.ctx.globals.start_wipe(WipeState::new(
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
        let saved_msgbk = if leave_msgbk {
            Some(self.vm.ctx.globals.msgbk_forms.clone())
        } else {
            None
        };
        self.vm.restart_scene_name(&target_scene, target_z)?;
        self.renderer.clear_runtime_image_textures();
        if let Some(msgbk) = saved_msgbk {
            self.vm.ctx.globals.msgbk_forms = msgbk;
        }
        self.vm.ctx.globals.finish_wipe();
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
        if self.paused {
            return Ok(());
        }

        self.vm.begin_script_proc_pump();

        loop {
            let Some(proc) = self.flow.top().cloned() else {
                self.paused = true;
                break;
            };

            match proc.ty {
                ProcType::Script => {
                    let proc_gen_before = self.vm.proc_generation();
                    let running = self.vm.run_script_proc_continue()?;
                    let proc_boundary = self.vm.proc_generation() != proc_gen_before;
                    let boundary_kind = self.vm.last_proc_kind();
                    let pop_script_proc = self.vm.take_script_proc_pop_request();
                    let halted = self.vm.is_halted();
                    let cur_scene = self
                        .vm
                        .current_scene_name()
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| self.boot.start_scene.clone());
                    let pending = self.vm.ctx.globals.syscom.pending_proc.is_some();
                    let blocked = if pending { false } else { self.vm.is_blocked() };

                    self.ensure_requested_script_proc();
                    if pop_script_proc {
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
                        if self.vm.is_blocked() {
                            break;
                        }
                    } else if blocked {
                        break;
                    } else if proc_boundary {
                        match boundary_kind {
                            ProcKind::Disp => {
                                self.script_resume_after_redraw = true;
                                break;
                            }
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
                    let warning_exists = self
                        .vm
                        .ctx
                        .images
                        .project_dir()
                        .join("g00")
                        .join("___SYSEVE_WARNING.g00")
                        .exists()
                        || self
                            .vm
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
                ProcType::SyscomWarning => {
                    if self.vm.ctx.globals.system.messagebox_modal.is_some() {
                        break;
                    }
                    let result = self
                        .vm
                        .ctx
                        .globals
                        .system
                        .messagebox_modal_result
                        .take()
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
                    if self.vm.ctx.globals.wipe_done() {
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
        Ok(())
    }

    fn redraw(&mut self) -> Result<()> {
        let wait_poll_needed = self.vm.ctx.wait.needs_runtime_poll();
        self.vm.tick_frame()?;
        if wait_poll_needed && !self.vm.is_blocked() {
            self.script_needs_pump = true;
        }
        self.ensure_requested_script_proc();
        let list = self.vm.ctx.render_list_with_effects();
        self.renderer.render_sprites(&self.vm.ctx.images, &list)?;
        if self.script_resume_after_redraw {
            self.script_resume_after_redraw = false;
            self.script_needs_pump = true;
        }
        self.redraw_count = self.redraw_count.saturating_add(1);
        Ok(())
    }
}

pub unsafe fn cstr_opt(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }
    let s = CStr::from_ptr(ptr).to_string_lossy().to_string();
    if s.is_empty() { None } else { Some(s) }
}

pub unsafe fn cstr_required(ptr: *const c_char, what: &str) -> Result<String> {
    if ptr.is_null() {
        anyhow::bail!("{what} is null");
    }
    Ok(CStr::from_ptr(ptr).to_str()?.to_string())
}

pub fn parse_bool_exit(result: Result<bool>, context: &str) -> i32 {
    match result {
        Ok(true) => 1,
        Ok(false) => 0,
        Err(e) => {
            log::error!("{context}: {e:?}");
            1
        }
    }
}

pub fn default_frame_interval_ms(dt_ms: u32) -> u32 {
    if dt_ms == 0 { FRAME_INTERVAL_MS } else { dt_ms }
}
