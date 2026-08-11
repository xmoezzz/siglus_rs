//! Runtime scaffolding for command execution.
//!
//! Scene bytecode is dispatched by numeric form/owner codes. Siglus user
//! commands are script procedures entered by `SceneVm` through Scene.pck.

pub mod constants;
pub mod forms;
pub mod graphics;
pub mod input;
pub mod opcode;

pub use opcode::OpCode;
pub mod gan;
pub mod game_display_info;
pub mod game_title;
pub mod globals;
pub mod int_event;
pub mod string_semantics;
pub mod net;
pub mod native_ui;
pub mod tables;
pub mod tonecurve;
pub mod ui;
pub mod unknown;
pub mod wait;
mod wipe;
pub(crate) mod wipe_mask;
use crate::runtime::forms::codes::syscom_op;
use crate::runtime::forms::pcmevent as pcmevent_form;
use crate::runtime::forms::syscom as syscom_form;

use anyhow::{anyhow, Result};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use crate::assets::RgbaImage;
use crate::audio::{AudioHub, BgmEngine, KoeEngine, PcmEngine, SeEngine};
use crate::image_manager::{ImageId, ImageManager};
use crate::layer::{
    ClipRect, LayerId, LayerManager, RenderFrame, RenderSprite, Sprite, SpriteFit, SpriteId,
    SpriteRuntimeLight, SpriteSizeMode, WipeRenderPlan,
};
use crate::movie::MovieManager;
use crate::text_render::{embedded_default_font_names, FontCache, TextStyle};
use siglus_assets::scene_pck::{ScenePck, ScenePckDecodeOptions};
use std::fs;
use std::path::{Path, PathBuf};

use crate::env::trace_env;

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Str(String),
    /// An element chain as raw i32 codes (as stored on the VM int stack).
    Element(Vec<i32>),
    /// A nested list value (FM_LIST).
    List(Vec<Value>),
    /// A named argument (id -> value), used by some engine commands.
    NamedArg {
        id: i32,
        value: Box<Value>,
    },
}

impl Value {
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(v) => Some(*v),
            Value::NamedArg { value, .. } => value.as_i64(),
            _ => None,
        }
    }

    pub fn named_id(&self) -> Option<i32> {
        match self {
            Value::NamedArg { id, .. } => Some(*id),
            _ => None,
        }
    }

    pub fn unwrap_named(&self) -> &Value {
        match self {
            Value::NamedArg { value, .. } => value.as_ref(),
            _ => self,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s.as_str()),
            Value::NamedArg { value, .. } => value.as_str(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Command {
    pub name: String,
    /// Optional numeric code for VM forms.
    pub code: Option<opcode::OpCode>,
    pub args: Vec<Value>,
}


#[derive(Debug, Clone)]
struct MsgBackLayoutEntry {
    history_index: usize,
    text: String,
    total_pos: i32,
    height: i32,
}

#[derive(Debug, Clone)]
struct MsgBackSeparatorLayout {
    file: Option<String>,
    total_pos: i32,
    height: i32,
}

#[derive(Debug, Clone, Default)]
struct MsgBackLayout {
    entries: Vec<MsgBackLayoutEntry>,
    separators: Vec<MsgBackSeparatorLayout>,
    total_height: i32,
}

/// State used by EXCALL runtime helpers.
///
/// We intentionally keep these names offset-based instead of guessing their meaning.
#[derive(Debug, Clone)]
pub struct ExcallCompatState {
    pub ready: bool,
    /// C_elm_excall::free() runs frame-action end callbacks before clearing
    /// its child lists/stages. Rust drains those callbacks immediately after
    /// command dispatch, so resource release is deferred until that drain.
    pub pending_free: bool,
    pub ex_call_flag: bool,
    pub flag_204: bool,
    pub flag_2148: bool,
    pub script_proc_requested: bool,
    pub script_proc_pop_requested: bool,

    // C_elm_excall owns an independent set of SCRIPT font overrides.
    // They are not aliases of Gp_local: cmd_script.cpp routes
    // EXCALL.SCRIPT.{87..95} to Gp_excall->m_font_name / m_pod.
    pub font_name: String,
    pub font_bold: i64,
    pub font_shadow: i64,
    // Newer C_elm_excall POD field corresponding to SCRIPT.98/99/100.
    pub joypad_mode_override: i64,
}

impl Default for ExcallCompatState {
    fn default() -> Self {
        Self {
            ready: false,
            pending_free: false,
            ex_call_flag: false,
            flag_204: false,
            flag_2148: false,
            script_proc_requested: false,
            script_proc_pop_requested: false,
            font_name: String::new(),
            font_bold: -1,
            font_shadow: -1,
            joypad_mode_override: -1,
        }
    }
}

/// Optional external handler for numeric forms.
///
/// The project can keep game-specific implementations (e.g. SCREEN/MSGBK)
/// outside this crate, while still letting the VM dispatch through here.
pub trait ExternalFormHandler: Send + Sync {
    /// Return true if the form ID was handled.
    fn dispatch_form(
        &self,
        ctx: &mut CommandContext,
        form_id: u32,
        args: &[Value],
    ) -> anyhow::Result<bool>;
}
/// Cooperative script-process boundary, mirroring Siglus' `TNM_PROC_TYPE_*`
/// model at the VM/runtime boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcKind {
    Script,
    Disp,
    Frame,
    Command,
    MessageBlock,
    MessageWait,
    KeyWait,
    TimeWait,
    MovieWait,
    WipeWait,
    AudioWait,
    EventWait,
    Selection,
    SystemModal,
}

#[derive(Debug, Clone, Default)]
pub struct VmCallMeta {
    pub element: Vec<i32>,
    pub al_id: i64,
    pub ret_form: i64,
}

#[derive(Debug, Clone)]
pub struct DebugActiveTextureEntry {
    pub image_id: ImageId,
    pub width: u32,
    pub height: u32,
    pub source_label: String,
    pub submitted_this_frame: bool,
    pub visible_refs: usize,
    pub total_refs: usize,
    pub ref_summary: String,
}

#[derive(Debug, Default, Clone)]
struct DebugActiveTextureAccum {
    width: u32,
    height: u32,
    source_label: String,
    submitted_this_frame: bool,
    visible_refs: usize,
    total_refs: usize,
    ref_labels: Vec<String>,
}

fn sg_mwnd_state_trace_runtime(
    scene: &str,
    scene_no: &str,
    line: i64,
    reason: &str,
    stage_idx: i64,
    mwnd_idx: usize,
    old_open: bool,
    new_open: bool,
    m: &globals::MwndState,
) {
    if !trace_env().sg_debug {
        return;
    }
    eprintln!(
        "[SG_DEBUG][MWND_STATE_TRACE] scene={} scene_no={} line={} reason={} stage={} mwnd={} old_open={} new_open={} buttons={} faces={} objects={} waku={} filter={} pos={:?} size={:?} open_anim=({}, {}) close_anim=({}, {}) selection={} msg_len={} name_len={}",
        scene,
        scene_no,
        line,
        reason,
        stage_idx,
        mwnd_idx,
        old_open,
        new_open,
        m.button_list.len(),
        m.face_list.len(),
        m.object_list.len(),
        if m.waku_file.is_empty() { "-" } else { m.waku_file.as_str() },
        if m.filter_file.is_empty() { "-" } else { m.filter_file.as_str() },
        m.window_pos,
        m.window_size,
        m.open_anime_type,
        m.open_anime_time,
        m.close_anime_type,
        m.close_anime_time,
        m.selection.is_some(),
        m.msg_text.len(),
        m.name_text.len(),
    );
}


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeSaveKind {
    Normal,
    Quick,
    End,
    Inner,
}

/// GPU service for original-engine capture commands. Captures must pass through
/// the same render graph as presentation so meshes, depth, fog and shader state
/// are preserved.
pub trait FrameCaptureBackend {
    fn capture_render_frame(
        &mut self,
        images: &ImageManager,
        frame: &RenderFrame,
        logical_width: u32,
        logical_height: u32,
    ) -> Result<RgbaImage>;
}

pub type FrameCaptureBackendRef = Rc<RefCell<dyn FrameCaptureBackend>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeSaveRequest {
    pub kind: RuntimeSaveKind,
    pub index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeLoadRequest {
    pub kind: RuntimeSaveKind,
    pub index: usize,
}

/// Engine-side cache mirroring the C++ `S_tnm_local_save` (a.k.a. `Gp_eng->m_local_save`).
/// Populated by GLOBAL_SAVEPOINT (`tnm_save_local`) and after a successful load
/// (`tnm_load_local_on_file`). All save-to-file paths (normal / quick / end) must
/// pull from this cache instead of re-serializing the live runtime.
#[derive(Debug, Clone, Default)]
pub struct LocalSaveSnapshot {
    pub save_id: [u16; 7],
    pub append_dir: String,
    pub append_name: String,
    pub save_scene_title: String,
    pub save_msg: String,
    pub save_full_msg: String,
    pub local_stream: Vec<u8>,
    pub local_ex_stream: Vec<u8>,
    pub sel_saves: Vec<crate::original_save::OriginalLocalSaveEnvelope>,
}

#[derive(Debug, Clone, Copy)]
struct MouseCursorFrameRuntime {
    image_id: ImageId,
    hot_x: i32,
    hot_y: i32,
}

#[derive(Debug, Clone)]
struct MouseCursorRuntime {
    frames: Vec<MouseCursorFrameRuntime>,
    anime_speed_ms: i64,
}

pub struct CommandContext {
    pub project_dir: PathBuf,

    /// Project-wide variable DWORD used by encrypted Emote PSB files. Native
    /// hosts preload/recover this before scene execution; direct VM users still
    /// pick up an explicitly configured `key.toml` value here.
    pub emote_key: Option<u32>,
    pub images: ImageManager,
    pub layers: LayerManager,
    /// 1x1 white sprite used for screen-space overlays (filters, etc.).
    pub solid_white: ImageId,

    pub audio: AudioHub,

    pub bgm: BgmEngine,
    pub koe: KoeEngine,
    pub pcm: PcmEngine,
    pub se: SeEngine,

    pub movie: MovieManager,

    /// Runtime numeric constants (form/element/op codes).
    pub ids: constants::RuntimeConstants,

    /// Graphics runtime state for stage/object sprite binding.
    pub gfx: graphics::GfxRuntime,

    /// UI runtime (text window, message waits, etc.).
    pub ui: ui::UiRuntime,
    /// Shared font cache for stage/object text rendering.
    pub font_cache: FontCache,

    /// Runtime-visible input state (button manager, waits, runtime systems).
    pub input: input::InputState,
    /// Script-visible input state (`Gp_script_input` in the original engine).
    pub script_input: input::InputState,

    /// Current render target size (used for UI layout).
    pub screen_w: u32,
    pub screen_h: u32,

    /// Shared wgpu renderer used by synchronous capture commands.
    frame_capture_backend: Option<FrameCaptureBackendRef>,

    /// VM blocking state (WAIT / WAIT_KEY).
    pub wait: wait::VmWait,

    /// Cooperative proc boundary generation. Form handlers bump this when they
    /// perform an original-engine proc switch/push.
    proc_generation: u64,
    last_proc_kind: ProcKind,

    /// Lightweight network/browser helper mirroring the engine's `tnm_net` slot.
    pub net: net::TnmNet,

    /// Gameexe-driven asset tables (CGTABLE / DATABASE / THUMBTABLE).
    pub tables: tables::AssetTables,

    /// Value stack used by form handlers to return results.
    pub stack: Vec<Value>,

    pub unknown: unknown::UnknownOpRecorder,

    pub globals: globals::GlobalState,
    pub tonecurve: tonecurve::ToneCurveRuntime,

    pub excall_state: ExcallCompatState,

    /// Last fully presented scene list before wipe composition.
    pub last_presented_render_list: Vec<RenderSprite>,
    mouse_cursor_cache: HashMap<(i64, String), MouseCursorRuntime>,
    failed_gfx_image_repairs: HashSet<(String, i64, usize, String, u32)>,

    /// Optional project-provided form handler (game-specific).
    pub external_forms: Option<Arc<dyn ExternalFormHandler>>,

    /// Optional platform-native UI backend used by mobile ports.
    pub native_ui_backend: Option<Arc<dyn native_ui::NativeUiBackend>>,
    pub native_ui: native_ui::NativeUiRuntime,

    /// Current scene number tracked by the VM.
    pub current_scene_no: Option<i64>,
    /// Current scene name tracked by the VM.
    pub current_scene_name: Option<String>,
    /// Current source line tracked by the VM (`CD_NL`).
    pub current_line_no: i64,

    /// Current VM-originated form call metadata. Form handlers read this instead of
    /// relying on trailing wrapper arguments.
    pub vm_call: Option<VmCallMeta>,

    /// Set by concrete message/voice command handlers when original C++ consumes
    /// the following read-flag integer through Gp_lexer->pop_ret<int>().
    pending_read_flag_no: bool,
    pending_selbtn_read_flag_no: bool,
    /// Exact MWND that owns the deferred lexer read-flag operand. The scene
    /// number is captured when the command is dispatched because the VM pops
    /// the flag only after the form handler returns.
    pending_mwnd_read_flag_target: Option<(u32, i64, usize, i64)>,

    /// Deferred VM-owned save request. The form handler can only see CommandContext;
    /// the VM consumes this after the command returns so the saved stream includes
    /// the current lexer pc and stacks.
    pending_runtime_save: Option<RuntimeSaveRequest>,
    /// Deferred VM-owned load request, consumed by SceneVm after the command returns.
    pending_runtime_load: Option<RuntimeLoadRequest>,
    runtime_load_completed: bool,

    /// Engine-equivalent of `Gp_eng->m_local_save`. Built at GLOBAL_SAVEPOINT and
    /// refreshed by `tnm_load_local_on_file`-equivalent load. Save-to-file dispatch
    /// reads from here so the saved stream reflects the savepoint, not the live
    /// menu/UI state when the user picks a slot.
    pub local_save_snapshot: Option<LocalSaveSnapshot>,

    /// Set when the engine wants the next safe boundary (post-command, pre-next-step)
    /// to (re)build `local_save_snapshot`. Mirrors C++ `tnm_msg_proc_start_msg_block`
    /// calling `tnm_init_local_save` + `tnm_set_save_point` at message-block start.
    /// Form handlers can't build the snapshot themselves (they don't see SceneVm),
    /// so they request via this flag and the VM drains it at a safe point.
    pub pending_auto_savepoint: bool,

    /// `C_elm_btn_select::decide` pushes the result and immediately calls
    /// `tnm_set_sel_point()`.  CommandContext cannot snapshot VM stacks, so it
    /// carries the result to SceneVm's frame boundary where the exact resume
    /// point is recorded with that value temporarily present on the int stack.
    pending_sel_point_result: Option<i32>,

    frame_clock_last: Option<crate::platform_time::Instant>,
    last_button_hover_sound_pos: Option<(i32, i32)>,
    suppress_next_right_syscom_open: bool,
}

impl CommandContext {
    pub fn sync_script_input_from_runtime(&mut self) {
        self.script_input = self.input.clone();
    }

    pub fn proc_generation(&self) -> u64 {
        self.proc_generation
    }

    pub fn request_read_flag_no(&mut self) {
        self.pending_read_flag_no = true;
        // Global PRINT/KOE aliases normally route through the current MWND,
        // but retain an exact fallback target for compact/testcase aliases.
        if self.pending_mwnd_read_flag_target.is_none() {
            let form_id = if self.ids.form_global_stage != 0 {
                self.ids.form_global_stage
            } else {
                crate::runtime::constants::global_form::STAGE_ALT
            };
            self.pending_mwnd_read_flag_target = Some((
                form_id,
                self.globals.current_mwnd_stage_idx,
                self.globals.current_mwnd_no.unwrap_or(0),
                self.current_scene_no.unwrap_or(-1),
            ));
        }
    }

    pub fn request_read_flag_no_for_mwnd(
        &mut self,
        form_id: u32,
        stage_idx: i64,
        mwnd_idx: usize,
    ) {
        self.pending_read_flag_no = true;
        self.pending_selbtn_read_flag_no = false;
        self.pending_mwnd_read_flag_target = Some((
            form_id,
            stage_idx,
            mwnd_idx,
            self.current_scene_no.unwrap_or(-1),
        ));
    }

    pub fn request_read_flag_no_for_selbtn(&mut self) {
        self.pending_read_flag_no = true;
        self.pending_selbtn_read_flag_no = true;
        self.pending_mwnd_read_flag_target = None;
    }

    pub fn take_read_flag_no_request(&mut self) -> bool {
        let requested = std::mem::take(&mut self.pending_read_flag_no);
        if !requested {
            self.pending_selbtn_read_flag_no = false;
            self.pending_mwnd_read_flag_target = None;
        }
        requested
    }

    pub fn submit_read_flag_no(&mut self, value: i32) {
        if std::mem::take(&mut self.pending_selbtn_read_flag_no) {
            self.pending_mwnd_read_flag_target = None;
            self.globals.selbtn.read_flag_flag_no = value as i64;
            return;
        }

        let Some((form_id, stage_idx, mwnd_idx, scene_no)) =
            self.pending_mwnd_read_flag_target.take()
        else {
            return;
        };
        self.globals.script.cur_read_flag_scn_no = scene_no;
        self.globals.script.cur_read_flag_flag_no = value as i64;
        let mut commit_now = false;
        if let Some(mwnd) = self
            .globals
            .stage_forms
            .get_mut(&form_id)
            .and_then(|st| st.mwnd_lists.get_mut(&stage_idx))
            .and_then(|list| list.get_mut(mwnd_idx))
        {
            mwnd.read_flag_stock.push((scene_no, value as i64));
            // A synchronous PRINT reaches clear-ready before the VM can pop
            // this trailing operand. Commit immediately in that case so the
            // deferred ABI preserves C++ tnm_msg_proc_clear_ready ordering.
            commit_now = mwnd.clear_ready;
        }
        if commit_now {
            let stock = self
                .globals
                .stage_forms
                .get_mut(&form_id)
                .and_then(|st| st.mwnd_lists.get_mut(&stage_idx))
                .and_then(|list| list.get_mut(mwnd_idx))
                .map(|mwnd| std::mem::take(&mut mwnd.read_flag_stock))
                .unwrap_or_default();
            for (scene_no, flag_no) in stock {
                self.globals.set_read_flag(scene_no, flag_no);
            }
        }
    }

    pub fn request_runtime_save(&mut self, kind: RuntimeSaveKind, index: usize) {
        self.pending_runtime_save = Some(RuntimeSaveRequest { kind, index });
    }

    /// Form-handler entrypoint matching C++ `tnm_msg_proc_start_msg_block`'s
    /// `tnm_init_local_save` + (gated) `tnm_set_save_point` calls. Honors the
    /// `dont_set_save_point` script flag (suspends auto SAVEPOINT for special
    /// blocks). The VM drains the request at the next safe boundary and runs
    /// `build_local_save_snapshot`.
    pub fn request_auto_savepoint(&mut self) {
        if self.globals.script.dont_set_save_point {
            return;
        }
        self.pending_auto_savepoint = true;
    }

    pub fn take_pending_auto_savepoint(&mut self) -> bool {
        std::mem::take(&mut self.pending_auto_savepoint)
    }

    pub fn request_sel_point_with_result(&mut self, result: i64) {
        self.pending_sel_point_result = Some(result.clamp(i32::MIN as i64, i32::MAX as i64) as i32);
    }

    pub fn take_pending_sel_point_result(&mut self) -> Option<i32> {
        self.pending_sel_point_result.take()
    }

    pub fn request_runtime_load(&mut self, kind: RuntimeSaveKind, index: usize) {
        self.pending_runtime_load = Some(RuntimeLoadRequest { kind, index });
    }

    pub fn take_runtime_save_request(&mut self) -> Option<RuntimeSaveRequest> {
        self.pending_runtime_save.take()
    }

    pub fn take_runtime_load_request(&mut self) -> Option<RuntimeLoadRequest> {
        self.pending_runtime_load.take()
    }

    pub fn begin_runtime_load_apply(&mut self) {
        // Mirror C++ `tnm_finish_local()` + `tnm_reinit_local(false)`. The loaded
        // local stream re-populates per-scene state via `parse_original_local_stream`,
        // but anything that isn't *always* present in the stream (or that lives outside
        // it entirely - layers, UI, focus, menu, pending button actions, etc.) must be
        // torn down here so the save/load menu we're loading away from can't leak
        // through to the restored scene. Without this, a stage that the live save menu
        // populated but the snapshot omits would survive across the load.
        self.layers.clear_all();
        self.gfx = graphics::GfxRuntime::default();
        self.ui = ui::UiRuntime::default();
        self.wait = wait::VmWait::default();
        self.stack.clear();
        self.last_presented_render_list.clear();
        self.vm_call = None;
        self.pending_read_flag_no = false;
        self.pending_selbtn_read_flag_no = false;
        self.pending_mwnd_read_flag_target = None;
        self.pending_sel_point_result = None;
        self.frame_clock_last = None;
        self.last_button_hover_sound_pos = None;

        self.globals.focused_editbox = None;
        self.globals.focused_stage_group = None;
        self.globals.focused_stage_mwnd = None;
        self.globals.current_stage_object = None;
        self.globals.current_object_chain = None;
        self.globals.pending_button_actions.clear();
        self.globals.pending_frame_action_finishes.clear();
        self.globals.capture_for_object_image = None;
        self.globals.save_thumb_capture_image = None;
        self.globals.save_thumb_capture_prior =
            crate::runtime::forms::syscom::CAPTURE_PRIOR_NONE;
        self.globals.selbtn = globals::BtnSelectRuntimeState::default();
        self.globals.syscom.pending_proc = None;
        self.globals.syscom.menu_open = false;
        self.globals.syscom.menu_kind = None;
        self.globals.syscom.menu_result = None;
        self.globals.syscom.fallback_dialog = None;
        self.globals.syscom.fallback_origin = None;
        self.globals.syscom.msg_back_open = false;
        self.globals.syscom.msg_back_proc_initialized = false;
        self.globals.system.messagebox_modal = None;
        self.globals.system.messagebox_modal_result = None;
        self.globals.finish_wipe();

        // Per-scene state that lives in `globals` and is reconstructed from the
        // loaded local stream. Wipe before parsing so that snapshot entries that
        // are simply *absent* (the snapshot has no mask list, no editbox, etc.)
        // truly become absent post-load instead of inheriting from the menu.
        self.globals.stage_forms.clear();
        self.globals.screen_forms.clear();
        self.globals.counter_lists.clear();
        self.globals.frame_actions.clear();
        self.globals.frame_action_lists.clear();
        self.globals.mask_lists.clear();
        self.globals.pcm_event_lists.clear();
        self.globals.editbox_lists.clear();
        self.globals.msgbk_forms.clear();
    }

    pub fn mark_runtime_load_completed(&mut self) {
        self.runtime_load_completed = true;
    }

    pub fn take_runtime_load_completed(&mut self) -> bool {
        std::mem::take(&mut self.runtime_load_completed)
    }

    pub fn needs_continuous_frame(&self) -> bool {
        fn frame_action_needs_tick(fa: &globals::ObjectFrameActionState) -> bool {
            fa.counter.is_running() || (!fa.cmd_name.is_empty() && !fa.end_flag)
        }

        fn screen_effect_needs_tick(e: &globals::ScreenEffectState) -> bool {
            e.x.check_event()
                || e.y.check_event()
                || e.z.check_event()
                || e.mono.check_event()
                || e.reverse.check_event()
                || e.bright.check_event()
                || e.dark.check_event()
                || e.color_r.check_event()
                || e.color_g.check_event()
                || e.color_b.check_event()
                || e.color_rate.check_event()
                || e.color_add_r.check_event()
                || e.color_add_g.check_event()
                || e.color_add_b.check_event()
        }

        fn object_needs_tick(obj: &globals::ObjectState) -> bool {
            obj.any_event_active()
                || frame_action_needs_tick(&obj.frame_action)
                || obj.frame_action_ch.iter().any(frame_action_needs_tick)
                || obj.movie.playing
                || obj.gan.is_active()
                || obj.runtime.child_objects.iter().any(object_needs_tick)
        }

        if self.wait.needs_runtime_poll() {
            return true;
        }
        if self.pcm.needs_tick() {
            return true;
        }
        if self
            .ui
            .needs_continuous_frame(&self.globals.script, &self.globals.syscom)
        {
            return true;
        }
        if self.globals.mov.playing || self.globals.wipe.is_some() {
            return true;
        }
        if self.custom_mouse_cursor_needs_tick() {
            return true;
        }
        if self.globals.pending_frame_action_finishes.is_empty() == false
            || self.globals.pending_button_actions.is_empty() == false
        {
            return true;
        }
        if self
            .globals
            .counter_lists
            .values()
            .any(|v| v.iter().any(|c| c.is_running()))
        {
            return true;
        }
        if self
            .globals
            .int_event_roots
            .values()
            .any(|e| e.check_event())
            || self
                .globals
                .int_event_lists
                .values()
                .any(|v| v.iter().any(|e| e.check_event()))
        {
            return true;
        }
        if self
            .globals
            .frame_actions
            .values()
            .any(frame_action_needs_tick)
            || self
                .globals
                .frame_action_lists
                .values()
                .any(|v| v.iter().any(frame_action_needs_tick))
        {
            return true;
        }
        if self.globals.screen_forms.values().any(|screen| {
            screen.effect_list.iter().any(screen_effect_needs_tick)
                || screen.quake_list.iter().any(|q| q.is_active())
                || screen.shake.is_active()
        }) {
            return true;
        }
        let wipe_active = self.globals.wipe.is_some();
        let mwnd_ui_state = self
            .ui
            .current_mwnd_window_render_state(self.screen_w, self.screen_h);
        self.globals.stage_forms.values().any(|stage| {
            stage.object_lists.iter().any(|(&stage_idx, list)| {
                if stage_idx == TNM_STAGE_NEXT_I64 && !wipe_active {
                    return false;
                }
                list.iter().enumerate().any(|(obj_idx, obj)| {
                    !stage.is_embedded_object_slot(stage_idx, obj_idx) && object_needs_tick(obj)
                })
            }) || stage.mwnd_lists.iter().any(|(&stage_idx, list)| {
                if stage_idx == TNM_STAGE_NEXT_I64 && !wipe_active {
                    return false;
                }
                list.iter().any(|m| {
                    let Some((window_x, window_y)) = m.window_pos else {
                        return false;
                    };
                    let Some((window_w, window_h)) = m.window_size else {
                        return false;
                    };
                    if window_w <= 0 || window_h <= 0 {
                        return false;
                    }
                    let visible_or_animating = m.open
                        || mwnd_ui_state.map_or(false, |ui| {
                            ui.x as i64 == window_x
                                && ui.y as i64 == window_y
                                && ui.w as i64 == window_w
                                && ui.h as i64 == window_h
                        });
                    visible_or_animating
                        && (m.object_list.iter().any(object_needs_tick)
                            || m.button_list.iter().any(object_needs_tick)
                            || m.face_list.iter().any(object_needs_tick))
                })
            })
        })
    }

    pub fn last_proc_kind(&self) -> ProcKind {
        self.last_proc_kind
    }

    pub fn request_proc_boundary(&mut self, kind: ProcKind) {
        self.last_proc_kind = kind;
        self.proc_generation = self.proc_generation.wrapping_add(1);
    }

    pub fn request_disp_proc_boundary(&mut self) {
        self.request_proc_boundary(ProcKind::Disp);
    }

    pub fn request_message_block_proc_boundary(&mut self) {
        self.request_proc_boundary(ProcKind::MessageBlock);
    }

    pub fn request_message_wait_proc_boundary(&mut self) {
        self.request_proc_boundary(ProcKind::MessageWait);
    }

    pub fn request_wait_proc_boundary(&mut self, kind: ProcKind) {
        self.request_proc_boundary(kind);
    }

    /// End the active wipe with the same teardown performed by
    /// `C_tnm_wipe::end()` in the original engine.
    ///
    /// `GlobalState::finish_wipe()` only clears the timing marker and is kept
    /// for whole-runtime resets.  Live wipe completion must use this method so
    /// the copied NEXT stage and all of its backend sprites are reinitialized.
    pub fn finish_wipe_runtime(&mut self) {
        let Some(stage_form_id) = self.globals.wipe.as_ref().map(|w| w.stage_form_id) else {
            return;
        };
        self.globals.finish_wipe();
        crate::runtime::forms::stage::reinit_wipe_next_stage(self, stage_form_id);
    }

    pub fn notify_wait_key(&mut self) -> bool {
        let wipe_skipped = {
            let wait = &mut self.wait;
            let globals = &mut self.globals;
            wait.notify_key(globals, &self.ids)
        };
        self.finish_skipped_movie_waits();
        if wipe_skipped {
            self.finish_wipe_runtime();
        }
        wipe_skipped
    }

    pub fn notify_movie_wait_down_up(&mut self, result: i64) -> bool {
        let skipped = {
            let wait = &mut self.wait;
            let globals = &mut self.globals;
            wait.notify_movie_down_up(globals, &self.ids, result)
        };
        if skipped {
            if sg_debug_enabled() {
                eprintln!("[SG_DEBUG][WAIT_KEY] down_up result={}", result);
            }
            self.finish_skipped_movie_waits();
            if !self.globals.mov.playing && self.globals.mov.file_name.is_some() {
                self.close_global_movie_runtime();
            }
        }
        skipped
    }

    fn runtime_is_skipping(&self) -> bool {
        // eng_frame.cpp suppresses skip acceleration while message-back is
        // open, even if Ctrl/read-skip/script-trigger remain logically set.
        if self.globals.syscom.msg_back_open {
            return false;
        }
        let script = &self.globals.script;
        if script.ctrl_disable {
            return false;
        }
        if self.input.vk_is_down(0x11) {
            return true;
        }
        if script.skip_disable {
            return false;
        }
        script.skip_trigger || self.globals.syscom.read_skip.onoff
    }

    fn should_wheel_advance_message(&self) -> bool {
        const GET_WHEEL_NEXT_MESSAGE_ONOFF: i32 = 305;
        self.globals
            .syscom
            .config_int
            .get(&GET_WHEEL_NEXT_MESSAGE_ONOFF)
            .copied()
            .unwrap_or(1)
            != 0
    }

    fn should_stop_koe_on_advance(&self) -> bool {
        const GET_KOE_DONT_STOP_ONOFF: i32 = 308;
        let syscom_dont_stop = self
            .globals
            .syscom
            .config_int
            .get(&GET_KOE_DONT_STOP_ONOFF)
            .copied()
            .unwrap_or(0)
            != 0;
        let script = &self.globals.script;
        let mut dont_stop = syscom_dont_stop || script.koe_dont_stop_on_flag;
        if script.koe_dont_stop_off_flag {
            dont_stop = false;
        }
        !dont_stop
    }

    fn is_modifier_key(k: input::VmKey) -> bool {
        matches!(
            k,
            input::VmKey::Shift
                | input::VmKey::Control
                | input::VmKey::Meta
                | input::VmKey::Alt
        )
    }

    fn cancel_pending_editbox_composition(&mut self) {
        let Some((form_id, idx)) = self.globals.focused_editbox else {
            return;
        };
        if let Some(eb) = self
            .globals
            .editbox_lists
            .get_mut(&form_id)
            .and_then(|list| list.boxes.get_mut(idx))
        {
            eb.cancel_pending_composition_clear();
        }
    }

    pub(crate) fn set_focused_editbox(&mut self, target: Option<(u32, usize)>) {
        let old = self.globals.focused_editbox;
        if old == target {
            return;
        }
        if let Some((form_id, idx)) = old {
            if let Some(eb) = self
                .globals
                .editbox_lists
                .get_mut(&form_id)
                .and_then(|list| list.boxes.get_mut(idx))
            {
                eb.cancel_composition();
                eb.mouse_selecting = false;
            }
        }
        self.globals.focused_editbox = target;
        if let Some((form_id, idx)) = target {
            if let Some(eb) = self
                .globals
                .editbox_lists
                .get_mut(&form_id)
                .and_then(|list| list.boxes.get_mut(idx))
            {
                eb.ensure_caret_visible_for_focus();
            }
        }
    }

    fn sync_editbox_runtime(&mut self) {
        let sw = self.screen_w as i32;
        let sh = self.screen_h as i32;
        let display_cnt = self.globals.change_display_mode_proc_cnt;
        for list in self.globals.editbox_lists.values_mut() {
            for eb in &mut list.boxes {
                eb.update_rect(sw, sh);
                eb.frame(display_cnt);
            }
        }
        if let Some((form_id, idx)) = self.globals.focused_editbox {
            let keep = self
                .globals
                .editbox_lists
                .get(&form_id)
                .and_then(|list| list.boxes.get(idx))
                .map(|eb| eb.created)
                .unwrap_or(false);
            if !keep {
                self.set_focused_editbox(None);
            }
        }
    }

    fn toggle_screen_size_mode_for_editbox(&mut self) {
        const GET_WINDOW_MODE: i32 = syscom_op::GET_WINDOW_MODE;
        let current = self
            .globals
            .syscom
            .config_int
            .get(&GET_WINDOW_MODE)
            .copied()
            .unwrap_or(0);
        let next = if current == 0 { 1 } else { 0 };
        self.globals.syscom.config_int.insert(GET_WINDOW_MODE, next);
        self.globals.change_display_mode_proc_cnt =
            self.globals.change_display_mode_proc_cnt.max(2);
    }

    fn move_editbox_focus(&mut self, forward: bool) {
        let Some((form_id, idx)) = self.globals.focused_editbox else {
            return;
        };
        let target = {
            let Some(list) = self.globals.editbox_lists.get(&form_id) else {
                return;
            };
            let len = list.boxes.len();
            if len == 0 {
                return;
            }
            let mut cur = idx;
            let mut target = None;
            for _ in 0..len {
                cur = if forward {
                    (cur + 1) % len
                } else {
                    (cur + len - 1) % len
                };
                if list.boxes.get(cur).is_some_and(|eb| eb.created) {
                    target = Some((form_id, cur));
                    break;
                }
            }
            target
        };
        if target.is_some() {
            self.set_focused_editbox(target);
        }
    }

    /// Advance the current message wait.
    ///
    /// Returns true when the input was consumed only to reveal the rest of the
    /// typewriter text. In that case the VM key wait must stay blocked.
    fn advance_message_wait(&mut self, allow: bool) -> bool {
        if !allow || !self.ui.mwnd.msg.waiting {
            return false;
        }
        if !self.ui.message_wait_text_fully_revealed() {
            self.ui.reveal_message_now();
            return true;
        }
        match self.ui.end_wait_message() {
            ui::MessageWaitClearAction::None => {}
            ui::MessageWaitClearAction::Clear => self.clear_current_mwnd_after_wait(),
            ui::MessageWaitClearAction::NovelClear => {
                forms::stage::novel_clear_current_mwnd_after_wait(self);
            }
        }
        if self.should_stop_koe_on_advance() {
            // eng_message.cpp stops the active C_elm_koe voice here.  The old
            // port accidentally stopped SE and every PCM/PCMCH channel instead.
            let _ = self.koe.stop(None);
            self.globals.sound_routing.bgmfade2_need_flag = false;
        }
        false
    }

    fn clear_current_mwnd_after_wait(&mut self) {
        let default_form_id = if self.ids.form_global_stage != 0 {
            self.ids.form_global_stage
        } else {
            constants::global_form::STAGE_ALT
        };
        let target = self.globals.focused_stage_mwnd.unwrap_or((
            default_form_id,
            self.globals.current_mwnd_stage_idx,
            self.globals.current_mwnd_no.unwrap_or(0),
        ));
        let (form_id, stage_idx, mwnd_idx) = target;
        let mut read_flags = Vec::new();
        if let Some(m) = self
            .globals
            .stage_forms
            .get_mut(&form_id)
            .and_then(|st| st.mwnd_lists.get_mut(&stage_idx))
            .and_then(|list| list.get_mut(mwnd_idx))
        {
            // tnm_msg_proc_clear_ready(): mark for deferred clear, commit the
            // block's read flags, and end block-local message modes.  Do not
            // erase glyphs/cursor/name here; C_elm_mwnd::clear() runs only
            // when the next message block starts.
            m.clear_ready = true;
            m.msg_block_started = false;
            m.multi_msg = false;
            m.key_icon_appear = false;
            m.key_icon_pos = None;
            m.text_dirty = false;
            read_flags = std::mem::take(&mut m.read_flag_stock);
        }
        for (scene_no, flag_no) in read_flags {
            self.globals.set_read_flag(scene_no, flag_no);
        }
        self.globals.script.multi_msg_mode = false;
        self.globals.script.cur_koe_no = -1;
        self.globals.script.cur_chr_no = -1;
        self.globals.script.auto_mode_moji_cnt = 0;
        self.globals.syscom.replay_koe = None;
        if self.globals.script.async_msg_mode_once {
            self.globals.script.async_msg_mode = false;
            self.globals.script.async_msg_mode_once = false;
        }
    }
    pub fn new(project_dir: PathBuf) -> Self {
        crate::resource::preload_project_file_index(&project_dir);

        let mut unknown = unknown::UnknownOpRecorder::default();
        let tables = tables::AssetTables::load(&project_dir, &mut unknown);
        let emote_key = crate::resource::load_project_emote_key(&project_dir)
            .ok()
            .flatten();

        let ids = constants::RuntimeConstants::default();

        let audio = AudioHub::new();
        let mut images = ImageManager::new(project_dir.clone());
        let solid_white = images.solid_rgba((255, 255, 255, 255));
        let tonecurve = tonecurve::ToneCurveRuntime::new(&project_dir);

        let mut ctx = Self {
            images,
            layers: LayerManager::default(),
            audio,
            bgm: BgmEngine::new(project_dir.clone()),
            koe: KoeEngine::new(project_dir.clone()),
            pcm: PcmEngine::new(project_dir.clone()),
            se: SeEngine::new(project_dir.clone()),
            movie: MovieManager::new(project_dir.clone()),
            project_dir,
            emote_key,
            solid_white,
            tables,
            stack: Vec::new(),
            unknown,
            ids,
            gfx: graphics::GfxRuntime::default(),
            ui: ui::UiRuntime::default(),
            font_cache: FontCache::new(),
            input: input::InputState::default(),
            script_input: input::InputState::default(),
            wait: wait::VmWait::default(),
            proc_generation: 0,
            last_proc_kind: ProcKind::Script,
            net: net::TnmNet::default(),

            screen_w: 1280,
            screen_h: 720,
            frame_capture_backend: None,
            globals: globals::GlobalState::default(),
            tonecurve,
            excall_state: ExcallCompatState::default(),
            last_presented_render_list: Vec::new(),
            mouse_cursor_cache: HashMap::new(),
            failed_gfx_image_repairs: HashSet::new(),
            external_forms: None,
            native_ui_backend: None,
            native_ui: native_ui::NativeUiRuntime::default(),
            current_scene_no: None,
            current_scene_name: None,
            current_line_no: -1,
            vm_call: None,
            pending_read_flag_no: false,
            pending_selbtn_read_flag_no: false,
            pending_mwnd_read_flag_target: None,
            pending_runtime_save: None,
            pending_runtime_load: None,
            runtime_load_completed: false,
            local_save_snapshot: None,
            pending_auto_savepoint: false,
            pending_sel_point_result: None,
            frame_clock_last: None,
            last_button_hover_sound_pos: None,
            suppress_next_right_syscom_open: false,
        };
        ctx.apply_gameexe_runtime_defaults();
        ctx
    }

    pub(crate) fn effective_font_name(&self) -> &str {
        let config = &self.globals.syscom.original_config.font_name;
        if self.globals.syscom.msg_back_open {
            return config;
        }
        if self.excall_state.ex_call_flag {
            if self.excall_state.font_name.is_empty() {
                config
            } else {
                &self.excall_state.font_name
            }
        } else if self.globals.script.font_name.is_empty() {
            config
        } else {
            &self.globals.script.font_name
        }
    }

    pub(crate) fn effective_font_shadow_mode(&self) -> i64 {
        let config = self.globals.syscom.original_config.font_shadow;
        let value = if self.globals.syscom.msg_back_open {
            config
        } else if self.excall_state.ex_call_flag {
            if self.excall_state.font_shadow >= 0 {
                self.excall_state.font_shadow
            } else {
                config
            }
        } else if self.globals.script.font_shadow >= 0 {
            self.globals.script.font_shadow
        } else {
            config
        };
        crate::text_render::normalize_font_shadow_mode(value)
    }

    pub(crate) fn effective_font_bold(&self) -> bool {
        let config = self.globals.syscom.original_config.font_futoku;
        if self.globals.syscom.msg_back_open {
            config
        } else if self.excall_state.ex_call_flag {
            if self.excall_state.font_bold >= 0 {
                self.excall_state.font_bold != 0
            } else {
                config
            }
        } else if self.globals.script.font_bold >= 0 {
            self.globals.script.font_bold != 0
        } else {
            config
        }
    }

    fn apply_gameexe_runtime_defaults(&mut self) {
        self.initialize_flag_lists();
        self.globals.script.cursor_no = self.mouse_cursor_default_no();
        // C++ keeps the local/excall overrides at -1 so they continue to follow
        // the current system configuration until SCRIPT.SET_FONT_* is used.
        self.globals.script.font_bold = -1;
        self.globals.script.font_shadow = -1;
        let text = self.gameexe_color(self.tables.mwnd_render.moji_color);
        let shadow = self.gameexe_color(self.tables.mwnd_render.shadow_color);
        let fuchi = (self.tables.mwnd_render.fuchi_color >= 0)
            .then_some(self.gameexe_color(self.tables.mwnd_render.fuchi_color));
        self.ui.set_text_colors_full(text, shadow, fuchi);
    }

    fn configured_flag_count(&self, global: bool) -> usize {
        let keys = if global {
            ["#GLOBAL_FLAG.CNT", "GLOBAL_FLAG.CNT"]
        } else {
            ["#FLAG.CNT", "FLAG.CNT"]
        };
        self.tables
            .gameexe
            .as_ref()
            .and_then(|cfg| keys.into_iter().find_map(|key| cfg.get_usize(key)))
            .unwrap_or(1000)
            .min(10000)
    }

    fn initialize_flag_lists(&mut self) {
        use crate::runtime::forms::codes;

        let local_count = self.configured_flag_count(false);
        let global_count = self.configured_flag_count(true);
        for form in [
            codes::ELM_GLOBAL_A,
            codes::ELM_GLOBAL_B,
            codes::ELM_GLOBAL_C,
            codes::ELM_GLOBAL_D,
            codes::ELM_GLOBAL_E,
            codes::ELM_GLOBAL_F,
            codes::ELM_GLOBAL_X,
        ] {
            let list = self
                .globals
                .int_lists
                .entry(form as u32)
                .or_insert_with(Vec::new);
            if list.len() < local_count {
                list.resize(local_count, 0);
            }
        }
        for form in [codes::ELM_GLOBAL_G, codes::ELM_GLOBAL_Z] {
            let list = self
                .globals
                .int_lists
                .entry(form as u32)
                .or_insert_with(Vec::new);
            if list.len() < global_count {
                list.resize(global_count, 0);
            }
        }
        let local_strings = self
            .globals
            .str_lists
            .entry(codes::ELM_GLOBAL_S as u32)
            .or_insert_with(Vec::new);
        if local_strings.len() < local_count {
            local_strings.resize_with(local_count, String::new);
        }
        let global_strings = self
            .globals
            .str_lists
            .entry(codes::ELM_GLOBAL_M as u32)
            .or_insert_with(Vec::new);
        if global_strings.len() < global_count {
            global_strings.resize_with(global_count, String::new);
        }
        for form in [
            codes::ELM_GLOBAL_NAMAE_LOCAL,
            codes::ELM_GLOBAL_NAMAE_GLOBAL,
        ] {
            let list = self
                .globals
                .str_lists
                .entry(form as u32)
                .or_insert_with(Vec::new);
            let name_count = 26 + 26 * 26;
            if list.len() < name_count {
                list.resize_with(name_count, String::new);
            }
        }
    }

    fn gameexe_color(&self, color_no: i64) -> (u8, u8, u8) {
        if color_no >= 0 {
            if let Some(&c) = self.tables.color_table.get(color_no as usize) {
                return c;
            }
        }
        (255, 255, 255)
    }

    fn gameexe_value(&self, key: &str) -> Option<&str> {
        self.tables.gameexe.as_ref()?.get_value(key)
    }

    fn gameexe_raw(&self, key: &str) -> Option<&str> {
        self.tables.gameexe.as_ref()?.get_unquoted(key)
    }

    fn gameexe_string(&self, key: &str) -> Option<String> {
        self.gameexe_raw(key)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn gameexe_rgba_default(&self, key: &str, default: (u8, u8, u8, u8)) -> (u8, u8, u8, u8) {
        let vals = Self::parse_i64_list(self.gameexe_value(key));
        if vals.len() >= 4 {
            (
                vals[0].clamp(0, 255) as u8,
                vals[1].clamp(0, 255) as u8,
                vals[2].clamp(0, 255) as u8,
                vals[3].clamp(0, 255) as u8,
            )
        } else {
            default
        }
    }

    fn syscom_filter_config_rgba(&self) -> (u8, u8, u8, u8) {
        let default = self.gameexe_rgba_default("CONFIG.FILTER_COLOR", (0, 0, 0, 128));
        let cfg = &self.globals.syscom.config_int;
        let pick = |key: i32, fallback: u8| -> u8 {
            cfg.get(&key)
                .copied()
                .unwrap_or(fallback as i64)
                .clamp(0, 255) as u8
        };
        (
            pick(syscom_op::GET_FILTER_COLOR_R, default.0),
            pick(syscom_op::GET_FILTER_COLOR_G, default.1),
            pick(syscom_op::GET_FILTER_COLOR_B, default.2),
            pick(syscom_op::GET_FILTER_COLOR_A, default.3),
        )
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

    fn parse_i64_list(raw: Option<&str>) -> Vec<i64> {
        let Some(raw) = raw else {
            return Vec::new();
        };
        raw.split(|c: char| c == ',' || c.is_whitespace())
            .filter_map(|part| {
                let t = part.trim();
                if t.is_empty() {
                    None
                } else {
                    t.parse::<i64>().ok()
                }
            })
            .collect()
    }

    fn gameexe_i64_default(&self, key: &str, default: i64) -> i64 {
        self.gameexe_value(key)
            .and_then(Self::parse_first_i64)
            .unwrap_or(default)
    }

    fn mouse_cursor_count(&self) -> usize {
        self.gameexe_value("#MOUSE_CURSOR.CNT")
            .or_else(|| self.gameexe_value("MOUSE_CURSOR.CNT"))
            .and_then(Self::parse_first_i64)
            .filter(|v| *v >= 0)
            .map(|v| v as usize)
            .unwrap_or(16)
            .min(256)
    }

    fn mouse_cursor_default_no(&self) -> i64 {
        let cnt = self.mouse_cursor_count() as i64;
        let no = self
            .gameexe_value("#MOUSE_CURSOR.DEFAULT")
            .or_else(|| self.gameexe_value("MOUSE_CURSOR.DEFAULT"))
            .and_then(Self::parse_first_i64)
            .unwrap_or(-1);
        if no >= 0 && no < cnt { no } else { -1 }
    }

    fn mouse_cursor_file_name(&self, cursor_no: i64) -> Option<String> {
        if cursor_no < 0 || cursor_no as usize >= self.mouse_cursor_count() {
            return None;
        }
        self.tables
            .gameexe
            .as_ref()
            .and_then(|cfg| cfg.get_indexed_field_unquoted("MOUSE_CURSOR", cursor_no as usize, "FILE"))
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    fn mouse_cursor_anime_speed(&self, cursor_no: i64) -> i64 {
        if cursor_no < 0 || cursor_no as usize >= self.mouse_cursor_count() {
            return 100;
        }
        self.tables
            .gameexe
            .as_ref()
            .and_then(|cfg| cfg.get_indexed_field("MOUSE_CURSOR", cursor_no as usize, "SPEED"))
            .and_then(Self::parse_first_i64)
            .unwrap_or(100)
    }

    fn load_mouse_cursor_runtime(&mut self, cursor_no: i64) -> Option<&MouseCursorRuntime> {
        let append_dir = self.images.current_append_dir().to_string();
        let key = (cursor_no, append_dir);
        if !self.mouse_cursor_cache.contains_key(&key) {
            if let Some(loaded) = self.load_mouse_cursor_runtime_uncached(cursor_no) {
                self.mouse_cursor_cache.insert(key.clone(), loaded);
            }
        }
        self.mouse_cursor_cache.get(&key)
    }

    fn load_mouse_cursor_runtime_uncached(&mut self, cursor_no: i64) -> Option<MouseCursorRuntime> {
        let file_name = self.mouse_cursor_file_name(cursor_no)?;
        let (path, pct) = match crate::resource::find_g00_image_with_append_dir(
            &self.project_dir,
            self.images.current_append_dir(),
            &file_name,
        ) {
            Ok(v) => v,
            Err(err) => {
                self.unknown.record_note(&format!(
                    "mouse_cursor.not_found:no={cursor_no}:file={file_name}:{err}"
                ));
                return None;
            }
        };
        if pct != crate::resource::PctType::G00 {
            self.unknown.record_note(&format!(
                "mouse_cursor.unsupported_type:no={cursor_no}:file={file_name}:path={}",
                path.display()
            ));
            return None;
        }
        let bytes = match crate::resource::read_file_bytes(&path) {
            Ok(v) => v,
            Err(err) => {
                self.unknown.record_note(&format!(
                    "mouse_cursor.read_failed:no={cursor_no}:path={}:{}",
                    path.display(),
                    err
                ));
                return None;
            }
        };
        let decoded = match crate::assets::g00::decode_g00(&bytes) {
            Ok(v) => v,
            Err(err) => {
                self.unknown.record_note(&format!(
                    "mouse_cursor.decode_failed:no={cursor_no}:path={}:{}",
                    path.display(),
                    err
                ));
                return None;
            }
        };
        if decoded.frames.is_empty() {
            return None;
        }
        let mut frames = Vec::with_capacity(decoded.frames.len());
        for (idx, img) in decoded.frames.iter().enumerate() {
            if img.width != 32 || img.height != 32 {
                self.unknown.record_note(&format!(
                    "mouse_cursor.invalid_size:no={cursor_no}:file={file_name}:patno={idx}:{}x{}",
                    img.width, img.height
                ));
                return None;
            }
            let image_id = match self.images.load_file(&path, idx) {
                Ok(id) => id,
                Err(err) => {
                    self.unknown.record_note(&format!(
                        "mouse_cursor.frame_load_failed:no={cursor_no}:file={file_name}:patno={idx}:{err}"
                    ));
                    return None;
                }
            };
            frames.push(MouseCursorFrameRuntime {
                image_id,
                hot_x: img.center_x,
                hot_y: img.center_y,
            });
        }
        Some(MouseCursorRuntime {
            frames,
            anime_speed_ms: self.mouse_cursor_anime_speed(cursor_no),
        })
    }

    pub fn has_active_custom_mouse_cursor(&mut self) -> bool {
        let cursor_no = self.globals.script.cursor_no;
        self.load_mouse_cursor_runtime(cursor_no).is_some()
    }

    fn custom_mouse_cursor_needs_tick(&self) -> bool {
        let cursor_no = self.globals.script.cursor_no;
        if cursor_no < 0 || cursor_no as usize >= self.mouse_cursor_count() {
            return false;
        }
        self.mouse_cursor_anime_speed(cursor_no) > 0 && self.mouse_cursor_file_name(cursor_no).is_some()
    }

    fn append_mouse_cursor_sprite(&mut self, list: &mut Vec<RenderSprite>) {
        if !self.globals.script.cursor_runtime_visible || self.globals.script.cursor_disp_off {
            return;
        }
        if !self.input.has_mouse_position() {
            return;
        }
        let cursor_no = self.globals.script.cursor_no;
        let cur_time = self.globals.local_real_time.max(0) as u64;
        let frame = {
            let Some(cursor) = self.load_mouse_cursor_runtime(cursor_no) else {
                return;
            };
            if cursor.frames.is_empty() {
                return;
            }
            let pat_no = if cursor.anime_speed_ms <= 0 {
                0usize
            } else {
                ((cur_time / cursor.anime_speed_ms as u64) as usize) % cursor.frames.len()
            };
            cursor.frames[pat_no]
        };

        let mut sprite = Sprite::default();
        sprite.image_id = Some(frame.image_id);
        sprite.visible = true;
        sprite.fit = SpriteFit::PixelRect;
        sprite.size_mode = SpriteSizeMode::Intrinsic;
        sprite.x = self.input.mouse_x.saturating_sub(frame.hot_x);
        sprite.y = self.input.mouse_y.saturating_sub(frame.hot_y);
        sprite.alpha = 255;
        sprite.tr = 255;
        sprite.alpha_blend = true;
        sprite.alpha_test = false;
        sprite.object_anchor = false;
        sprite.order = i32::MAX;
        list.push(RenderSprite::with_sorter(None, None, i32::MAX, i32::MAX, sprite));
    }

    fn gameexe_pair_default(&self, key: &str, default: (i64, i64)) -> (i64, i64) {
        let vals = Self::parse_i64_list(self.gameexe_value(key));
        if vals.len() >= 2 {
            (vals[0], vals[1])
        } else {
            default
        }
    }

    fn gameexe_rect_default(
        &self,
        key: &str,
        default: (i64, i64, i64, i64),
    ) -> (i64, i64, i64, i64) {
        let vals = Self::parse_i64_list(self.gameexe_value(key));
        if vals.len() >= 4 {
            (vals[0], vals[1], vals[2], vals[3])
        } else {
            default
        }
    }

    fn msg_back_button_pos(&self, key: &str, default: (i32, i32)) -> (i32, i32) {
        let vals = Self::parse_i64_list(self.gameexe_value(key));
        if vals.len() >= 2 {
            (vals[0] as i32, vals[1] as i32)
        } else {
            default
        }
    }

    pub fn lookup_scene_no(&self, scene_name: &str) -> Result<i64> {
        if scene_name.is_empty() {
            anyhow::bail!("empty scene name")
        }
        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        let pck = {
            let scene_pck_path = self.project_dir.join("Scene.pck");
            let bytes = crate::resource::read_file_bytes(&scene_pck_path)?;
            let exe = ["key.toml", "Key.toml"]
                .iter()
                .find_map(|name| {
                    let p = self.project_dir.join(name);
                    if !crate::resource::wasm_path_is_file(&p) {
                        return None;
                    }
                    let text = crate::resource::read_file_to_string(&p).ok()?;
                    siglus_assets::key_toml::parse_key_toml(&text)
                        .ok()
                        .and_then(|cfg| cfg.exe_key16)
                        .map(|v| v.to_vec())
                });
            let opt = ScenePckDecodeOptions {
                exe_angou_element: exe,
                easy_angou_code: Some(siglus_assets::keys::SCENE_KEY.to_vec()),
            };
            ScenePck::load_and_rebuild_from_bytes(bytes, &opt)?
        };
        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        let pck = {
            let scene_pck_path = crate::resource::find_scene_pck_path(&self.project_dir)?;
            let opt = ScenePckDecodeOptions::from_project_dir(&self.project_dir)?;
            ScenePck::load_and_rebuild(&scene_pck_path, &opt)?
        };
        let scene_no = pck
            .find_scene_no(scene_name)
            .ok_or_else(|| anyhow::anyhow!("scene not found: {}", scene_name))?;
        Ok(scene_no as i64)
    }

    pub fn reset_for_scene_restart(&mut self) {
        self.audio = AudioHub::new();
        self.bgm = BgmEngine::new(self.project_dir.clone());
        self.koe = KoeEngine::new(self.project_dir.clone());
        self.pcm = PcmEngine::new(self.project_dir.clone());
        self.se = SeEngine::new(self.project_dir.clone());
        self.movie = MovieManager::new(self.project_dir.clone());
        self.images = ImageManager::new(self.project_dir.clone());
        self.mouse_cursor_cache.clear();
        self.solid_white = self.images.solid_rgba((255, 255, 255, 255));
        self.layers.clear_all();
        self.gfx = graphics::GfxRuntime::default();
        self.ui = ui::UiRuntime::default();
        self.font_cache = FontCache::new();
        self.wait = wait::VmWait::default();
        self.stack.clear();
        self.globals = globals::GlobalState::default();
        self.tonecurve = tonecurve::ToneCurveRuntime::new(&self.project_dir);
        self.excall_state = ExcallCompatState::default();
        self.last_presented_render_list.clear();
        self.input.clear_all();
        self.vm_call = None;
        self.pending_read_flag_no = false;
        self.pending_selbtn_read_flag_no = false;
        self.pending_mwnd_read_flag_target = None;
        self.pending_sel_point_result = None;
        self.runtime_load_completed = false;
        self.frame_clock_last = None;
        self.last_button_hover_sound_pos = None;
        self.apply_gameexe_runtime_defaults();
    }

    /// Install or clear an external form handler.
    pub fn set_external_form_handler(&mut self, h: Option<Arc<dyn ExternalFormHandler>>) {
        self.external_forms = h;
    }

    // ------------------------------------------------------------------
    // Object button runtime
    // ------------------------------------------------------------------

    fn active_button_stage_form_id(&self) -> Option<u32> {
        const EXCALL_LOCAL_NS_XOR: u32 = 0x4000;
        let normal_stage_form = if self.ids.form_global_stage != 0 {
            self.ids.form_global_stage
        } else {
            crate::runtime::forms::codes::FORM_GLOBAL_STAGE
        };
        if self.excall_state.ex_call_flag {
            if self.excall_state.ready {
                Some(normal_stage_form ^ EXCALL_LOCAL_NS_XOR)
            } else {
                None
            }
        } else {
            Some(normal_stage_form)
        }
    }

    fn load_any_image_for_hit(
        images: &mut ImageManager,
        file: &str,
        patno: i64,
    ) -> Option<crate::image_manager::ImageId> {
        let pat_u32 = if patno < 0 { 0 } else { patno as u32 };
        if let Ok(id) = images.load_g00(file, pat_u32) {
            return Some(id);
        }
        if let Ok(id) = images.load_bg(file) {
            return Some(id);
        }
        None
    }

    fn hit_test_sprite_rect(x: i32, y: i32, w: u32, h: u32, mx: i32, my: i32) -> bool {
        let x2 = x.saturating_add(w as i32);
        let y2 = y.saturating_add(h as i32);
        mx >= x && mx < x2 && my >= y && my < y2
    }

    fn alpha_test_image(img: &crate::assets::RgbaImage, local_x: i32, local_y: i32) -> bool {
        if local_x < 0 || local_y < 0 {
            return false;
        }
        let lx = local_x as u32;
        let ly = local_y as u32;
        if lx >= img.width || ly >= img.height {
            return false;
        }
        let idx = ((ly * img.width + lx) * 4 + 3) as usize;
        img.rgba.get(idx).copied().unwrap_or(0) != 0
    }

    fn play_button_se_no(&mut self, se_no: i64) {
        if se_no < 0 {
            return;
        }
        let Some(file_name) = self
            .tables
            .se_file_names
            .get(se_no as usize)
            .and_then(|v| v.as_deref())
            .filter(|s| !s.is_empty())
        else {
            self.unknown
                .record_note(&format!("se.table.missing:{se_no}"));
            return;
        };
        if self.se.play_file_name(&mut self.audio, file_name).is_err() {
            self.unknown
                .record_note(&format!("se.play.failed:{se_no}:{file_name}"));
        }
    }

    fn button_template_se_no(&self, template_no: i64, event: ButtonSeEvent) -> Option<i64> {
        if template_no < 0 {
            return None;
        }
        let template = self.tables.button_se_templates.get(template_no as usize)?;
        let se_no = match event {
            ButtonSeEvent::Hit => template.hit_no,
            ButtonSeEvent::Push => template.push_no,
            ButtonSeEvent::Decide => template.decide_no,
        };
        (se_no >= 0).then_some(se_no)
    }

    fn play_button_template_se(&mut self, template_no: i64, event: ButtonSeEvent) {
        if let Some(se_no) = self.button_template_se_no(template_no, event) {
            self.play_button_se_no(se_no);
        }
    }

    fn update_object_button_hover(&mut self) {
        if !self.input.has_mouse_position() {
            return;
        }
        let mx = self.input.mouse_x;
        let my = self.input.mouse_y;
        let play_hover_sound = match self.last_button_hover_sound_pos {
            Some((last_x, last_y)) if last_x == mx && last_y == my => false,
            Some(_) => true,
            None => false,
        };
        self.last_button_hover_sound_pos = Some((mx, my));
        let Some(form_id) = self.active_button_stage_form_id() else {
            return;
        };
        let mut hit_sounds = Vec::new();
        if sg_input_trace_enabled() {
            eprintln!("[SG_DEBUG][INPUT] hover mouse=({}, {})", mx, my);
        }

        {
            let Some(st) = self.globals.stage_forms.get_mut(&form_id) else {
                return;
            };

            let embedded_by_stage: HashMap<i64, HashSet<usize>> = st
                .embedded_object_slots
                .iter()
                .fold(HashMap::new(), |mut acc, (key, &slot)| {
                    if let Some((stage, _)) = key.split_once(':') {
                        if let Ok(stage_idx) = stage.parse::<i64>() {
                            acc.entry(stage_idx)
                                .or_insert_with(HashSet::new)
                                .insert(slot);
                        }
                    }
                    acc
                });
            let images = &mut self.images;
            let layers = &self.layers;
            let gfx = &self.gfx;
            let ids = &self.ids;
            let (object_lists, group_lists) = (&mut st.object_lists, &mut st.group_lists);

            let mut stage_ids: Vec<i64> = object_lists.keys().copied().collect();
            stage_ids.sort_unstable();
            for stage_idx in &stage_ids {
                let Some(objs) = object_lists.get_mut(stage_idx) else {
                    continue;
                };
                for (obj_idx, obj) in objs.iter_mut().enumerate() {
                    if embedded_by_stage
                        .get(stage_idx)
                        .map_or(false, |slots| slots.contains(&obj_idx))
                    {
                        continue;
                    }
                    clear_button_hit_recursive(obj);
                }
            }

            let mut group_stage_ids: Vec<i64> = group_lists.keys().copied().collect();
            group_stage_ids.sort_unstable();
            for stage_idx in group_stage_ids {
                let Some(groups) = group_lists.get_mut(&stage_idx) else {
                    continue;
                };
                for (group_idx, g) in groups.iter_mut().enumerate() {
                    if !g.is_doing() {
                        g.hit_button_no = -1;
                        g.hit_runtime_slot = None;
                        continue;
                    }
                    let Some(objs) = object_lists.get_mut(&stage_idx) else {
                        g.hit_button_no = -1;
                        g.hit_runtime_slot = None;
                        continue;
                    };

                    let mut best: Option<ButtonHitCandidate> = None;
                    let mut tied = false;
                    for (obj_idx, obj) in objs.iter_mut().enumerate() {
                        if embedded_by_stage
                            .get(&stage_idx)
                            .map_or(false, |slots| slots.contains(&obj_idx))
                        {
                            continue;
                        }
                        if let Some(hit) = hit_test_object_button_recursive(
                            images,
                            layers,
                            gfx,
                            ids,
                            &self.globals.syscom,
                            stage_idx,
                            group_idx,
                            mx,
                            my,
                            obj_idx,
                            obj,
                            None,
                        ) {
                            merge_button_hit(&mut best, &mut tied, hit);
                        }
                    }

                    if !tied {
                        if let Some(hit) = best {
                            g.hit_button_no = hit.button_no;
                            g.hit_runtime_slot = Some(hit.runtime_slot);
                            if sg_debug_enabled() {
                                eprintln!(
                                    "[SG_DEBUG][INPUT] group stage={} group={} hit_button={} slot={} order={} started={} pushed={} decided={}",
                                    stage_idx, group_idx, hit.button_no, hit.runtime_slot, hit.sort_key.display_tuple(), g.started, g.pushed_button_no, g.decided_button_no
                                );
                            }
                            if play_hover_sound && !hit.was_hit {
                                hit_sounds.push(hit.se_no);
                            }
                            for (obj_idx, obj) in objs.iter_mut().enumerate() {
                                if embedded_by_stage
                                    .get(&stage_idx)
                                    .map_or(false, |slots| slots.contains(&obj_idx))
                                {
                                    continue;
                                }
                                set_button_hit_by_runtime_slot_recursive(
                                    obj_idx,
                                    obj,
                                    hit.runtime_slot,
                                );
                            }
                        } else {
                            g.hit_button_no = -1;
                            g.hit_runtime_slot = None;
                            if sg_debug_enabled() {
                                eprintln!(
                                    "[SG_DEBUG][INPUT] group stage={} group={} no_hit started={}",
                                    stage_idx, group_idx, g.started
                                );
                            }
                        }
                    } else {
                        g.hit_button_no = -1;
                        g.hit_runtime_slot = None;
                        if sg_debug_enabled() {
                            eprintln!(
                                "[SG_DEBUG][INPUT] group stage={} group={} hit_tie",
                                stage_idx, group_idx
                            );
                        }
                    }
                }
            }

            let mut standalone_best: Option<ButtonHitCandidate> = None;
            let mut standalone_tied = false;
            for stage_idx in &stage_ids {
                let Some(objs) = object_lists.get_mut(stage_idx) else {
                    continue;
                };
                for (obj_idx, obj) in objs.iter_mut().enumerate() {
                    if embedded_by_stage
                        .get(stage_idx)
                        .map_or(false, |slots| slots.contains(&obj_idx))
                    {
                        continue;
                    }
                    if let Some(hit) = hit_test_standalone_action_button_recursive(
                        images,
                        layers,
                        gfx,
                        ids,
                        &self.globals.syscom,
                        *stage_idx,
                        mx,
                        my,
                        obj_idx,
                        obj,
                        None,
                    ) {
                        merge_button_hit(&mut standalone_best, &mut standalone_tied, hit);
                    }
                }
            }
            if !standalone_tied {
                if let Some(hit) = standalone_best {
                    if play_hover_sound && !hit.was_hit {
                        hit_sounds.push(hit.se_no);
                    }
                    for stage_idx in &stage_ids {
                        let Some(objs) = object_lists.get_mut(stage_idx) else {
                            continue;
                        };
                        for (obj_idx, obj) in objs.iter_mut().enumerate() {
                            if embedded_by_stage
                                .get(stage_idx)
                                .map_or(false, |slots| slots.contains(&obj_idx))
                            {
                                continue;
                            }
                            set_button_hit_by_runtime_slot_recursive(
                                obj_idx,
                                obj,
                                hit.runtime_slot,
                            );
                        }
                    }
                }
            }
        }

        {
            let mwnd_ui_state = self
                .ui
                .current_mwnd_window_render_state(self.screen_w, self.screen_h);
            let mwnd_hidden =
                self.globals.script.mwnd_disp_off_flag
                    || self.globals.syscom.hide_mwnd.onoff
                    || self.globals.syscom.msg_back_open;
            if let Some(st) = self.globals.stage_forms.get_mut(&form_id) {
                let images = &mut self.images;
                let layers = &self.layers;
                let gfx = &self.gfx;
                let ids = &self.ids;
                let mut standalone_best: Option<ButtonHitCandidate> = None;
                let mut standalone_tied = false;
                let mut stage_ids: Vec<i64> = st.mwnd_lists.keys().copied().collect();
                stage_ids.sort_unstable();
                for stage_idx in &stage_ids {
                    let Some(mwnds) = st.mwnd_lists.get_mut(stage_idx) else {
                        continue;
                    };
                    for mwnd in mwnds {
                        for obj in &mut mwnd.button_list {
                            clear_button_hit_recursive(obj);
                        }
                        for obj in &mut mwnd.face_list {
                            clear_button_hit_recursive(obj);
                        }
                        for obj in &mut mwnd.object_list {
                            clear_button_hit_recursive(obj);
                        }
                        if mwnd_hidden || !mwnd.open {
                            continue;
                        }
                        let Some((window_x, window_y)) = mwnd.window_pos else {
                            continue;
                        };
                        let Some((window_w, window_h)) = mwnd.window_size else {
                            continue;
                        };
                        if window_w <= 0 || window_h <= 0 {
                            continue;
                        }
                        let ui_state = mwnd_ui_state.filter(|ui| {
                            ui.x as i64 == window_x
                                && ui.y as i64 == window_y
                                && ui.w as i64 == window_w
                                && ui.h as i64 == window_h
                        });
                        let anim_parent =
                            ui_state.map(|ui| mwnd_anim_parent_from_ui_state(mwnd, ui));
                        let button_len = mwnd.button_list.len();
                        for button_idx in 0..button_len {
                            let skip = {
                                let obj = &mwnd.button_list[button_idx];
                                !object_button_renderable_by_syscom(&self.globals.syscom, obj)
                                    || button_effective_disabled(
                                        &self.globals.syscom,
                                        obj,
                                        Some(button_idx),
                                    )
                                    || self.globals.syscom.mwnd_btn_touch_disable
                            };
                            if skip {
                                continue;
                            }
                            let parent = apply_mwnd_window_anim_parent(
                                mwnd_button_parent_render_state(
                                    mwnd, button_idx, window_x, window_y, window_w, window_h,
                                ),
                                anim_parent,
                            );
                            let obj = &mut mwnd.button_list[button_idx];
                            if let Some(hit) = hit_test_standalone_action_button_recursive(
                                images,
                                layers,
                                gfx,
                                ids,
                                &self.globals.syscom,
                                *stage_idx,
                                mx,
                                my,
                                button_idx,
                                obj,
                                Some(parent),
                            ) {
                                merge_button_hit(&mut standalone_best, &mut standalone_tied, hit);
                            }
                        }
                        let face_len = mwnd.face_list.len();
                        for face_idx in 0..face_len {
                            let parent = apply_mwnd_window_anim_parent(
                                mwnd_face_parent_render_state(mwnd, face_idx, window_x, window_y),
                                anim_parent,
                            );
                            let obj = &mut mwnd.face_list[face_idx];
                            if let Some(hit) = hit_test_standalone_action_button_recursive(
                                images,
                                layers,
                                gfx,
                                ids,
                                &self.globals.syscom,
                                *stage_idx,
                                mx,
                                my,
                                face_idx,
                                obj,
                                Some(parent),
                            ) {
                                merge_button_hit(&mut standalone_best, &mut standalone_tied, hit);
                            }
                        }
                        let object_parent = apply_mwnd_window_anim_parent(
                            mwnd_parent_render_state_at(mwnd, window_x, window_y),
                            anim_parent,
                        );
                        let object_len = mwnd.object_list.len();
                        for object_idx in 0..object_len {
                            let obj = &mut mwnd.object_list[object_idx];
                            if let Some(hit) = hit_test_standalone_action_button_recursive(
                                images,
                                layers,
                                gfx,
                                ids,
                                &self.globals.syscom,
                                *stage_idx,
                                mx,
                                my,
                                object_idx,
                                obj,
                                Some(object_parent),
                            ) {
                                merge_button_hit(&mut standalone_best, &mut standalone_tied, hit);
                            }
                        }
                    }
                }
                if !standalone_tied {
                    if let Some(hit) = standalone_best {
                        if play_hover_sound && !hit.was_hit {
                            hit_sounds.push(hit.se_no);
                        }
                        for stage_idx in &stage_ids {
                            let Some(mwnds) = st.mwnd_lists.get_mut(stage_idx) else {
                                continue;
                            };
                            for mwnd in mwnds {
                                for (button_idx, obj) in mwnd.button_list.iter_mut().enumerate() {
                                    set_button_hit_by_runtime_slot_recursive(
                                        button_idx,
                                        obj,
                                        hit.runtime_slot,
                                    );
                                }
                                for (face_idx, obj) in mwnd.face_list.iter_mut().enumerate() {
                                    set_button_hit_by_runtime_slot_recursive(
                                        face_idx,
                                        obj,
                                        hit.runtime_slot,
                                    );
                                }
                                for (object_idx, obj) in mwnd.object_list.iter_mut().enumerate() {
                                    set_button_hit_by_runtime_slot_recursive(
                                        object_idx,
                                        obj,
                                        hit.runtime_slot,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        for se_no in hit_sounds {
            self.play_button_template_se(se_no, ButtonSeEvent::Hit);
        }
        self.update_mwnd_message_button_hover(play_hover_sound);
    }

    fn update_mwnd_message_button_hover(&mut self, play_hover_sound: bool) -> bool {
        let hit = self.ui.mwnd_message_button_hit(
            self.input.mouse_x,
            self.input.mouse_y,
            self.screen_w,
            self.screen_h,
        );
        let mut play_se = None;
        let mut active = false;
        for st in self.globals.stage_forms.values_mut() {
            for groups in st.group_lists.values_mut() {
                for group in groups {
                    group.hit_message_button = false;
                }
            }
        }
        if let Some(hit) = hit {
            if let Some(group) = self
                .globals
                .stage_forms
                .get_mut(&hit.form_id)
                .and_then(|st| st.group_lists.get_mut(&hit.stage_idx))
                .and_then(|groups| groups.get_mut(hit.group_no.max(0) as usize))
            {
                if group.is_doing() {
                    let changed = group.hit_button_no != hit.btn_no || !group.hit_message_button;
                    group.hit_button_no = hit.btn_no;
                    group.hit_runtime_slot = None;
                    group.hit_message_button = true;
                    group.message_button_se_no = hit.se_no;
                    active = true;
                    if changed && play_hover_sound {
                        play_se = Some(hit.se_no);
                    }
                }
            }
        }
        if let Some(se) = play_se {
            self.play_button_template_se(se, ButtonSeEvent::Hit);
        }
        active
    }

    fn handle_mwnd_message_button_mouse_down(&mut self) -> bool {
        let mut play_se = None;
        let mut consumed = false;
        for st in self.globals.stage_forms.values_mut() {
            for groups in st.group_lists.values_mut() {
                for group in groups {
                    if group.is_doing() && group.hit_message_button && group.hit_button_no >= 0 {
                        group.pushed_button_no = group.hit_button_no;
                        group.pushed_runtime_slot = None;
                        group.pushed_message_button = true;
                        play_se = Some(group.message_button_se_no);
                        consumed = true;
                    }
                }
            }
        }
        if let Some(se) = play_se {
            self.play_button_template_se(se, ButtonSeEvent::Push);
        }
        consumed
    }

    fn handle_mwnd_message_button_mouse_up(&mut self) -> bool {
        let mut play_se = None;
        let mut result_to_push = None;
        let mut consumed = false;
        let mut clear_focus = None;
        for (form_id, st) in self.globals.stage_forms.iter_mut() {
            for (stage_idx, groups) in st.group_lists.iter_mut() {
                for (group_idx, group) in groups.iter_mut().enumerate() {
                    if !group.pushed_message_button {
                        continue;
                    }
                    consumed = true;
                    let same = group.hit_message_button
                        && group.hit_button_no == group.pushed_button_no
                        && group.pushed_button_no >= 0;
                    let pushed = group.pushed_button_no;
                    let se_no = group.message_button_se_no;
                    let was_waiting = group.wait_flag;
                    if same && group.decide(pushed) {
                        play_se = Some(se_no);
                        if was_waiting {
                            group.wait_flag = false;
                            result_to_push = Some(pushed);
                            clear_focus = Some((*form_id, *stage_idx, group_idx));
                        }
                    } else {
                        group.pushed_button_no = -1;
                        group.pushed_message_button = false;
                    }
                }
            }
        }
        if let Some(value) = result_to_push {
            self.stack.push(Value::Int(value));
        }
        if clear_focus.is_some() && self.globals.focused_stage_group == clear_focus {
            self.globals.focused_stage_group = None;
        }
        if let Some(se) = play_se {
            self.play_button_template_se(se, ButtonSeEvent::Decide);
        }
        consumed
    }

    fn handle_object_button_mouse_down(&mut self, b: input::VmMouseButton) -> bool {
        // The original button manager separates pushed_this_frame from decided_this_frame.
        // Press starts the push state; release inside the same button decides it.
        self.update_object_button_hover();
        if matches!(b, input::VmMouseButton::Left)
            && self.handle_mwnd_message_button_mouse_down()
        {
            return true;
        }

        let Some(form_id) = self.active_button_stage_form_id() else {
            return false;
        };
        let mut template_sounds = Vec::new();
        let mut direct_sounds = Vec::new();
        let mut consumed_button = false;

        {
            let Some(st) = self.globals.stage_forms.get_mut(&form_id) else {
                return false;
            };

            let embedded_by_stage: HashMap<i64, HashSet<usize>> = st
                .embedded_object_slots
                .iter()
                .fold(HashMap::new(), |mut acc, (key, &slot)| {
                    if let Some((stage, _)) = key.split_once(':') {
                        if let Ok(stage_idx) = stage.parse::<i64>() {
                            acc.entry(stage_idx)
                                .or_insert_with(HashSet::new)
                                .insert(slot);
                        }
                    }
                    acc
                });
            let (object_lists, group_lists) = (&mut st.object_lists, &mut st.group_lists);

            match b {
                input::VmMouseButton::Left => {
                    let mut group_stage_ids: Vec<i64> = group_lists.keys().copied().collect();
                    group_stage_ids.sort_unstable();
                    for stage_idx in group_stage_ids {
                        let Some(groups) = group_lists.get_mut(&stage_idx) else {
                            continue;
                        };
                        for (group_idx, g) in groups.iter_mut().enumerate() {
                            if !g.is_doing() {
                                continue;
                            }
                            let hit = g.hit_button_no;
                            let Some(hit_slot) = g.hit_runtime_slot else {
                                continue;
                            };
                            if hit < 0 {
                                continue;
                            }
                            if g.pushed_runtime_slot != Some(hit_slot) {
                                if let Some(objs) = object_lists.get(&stage_idx) {
                                    if let Some(se_no) =
                                        find_button_se_no_in_list_by_runtime_slot(objs, hit_slot)
                                    {
                                        template_sounds.push(se_no);
                                    }
                                }
                            }
                            g.pushed_button_no = hit;
                            g.pushed_runtime_slot = Some(hit_slot);
                            if let Some(objs) = object_lists.get_mut(&stage_idx) {
                                for (obj_idx, obj) in objs.iter_mut().enumerate() {
                                    set_button_pushed_by_runtime_slot_recursive(
                                        obj_idx, obj, hit_slot,
                                    );
                                }
                            }
                        }
                    }

                    let mut stage_ids: Vec<i64> = object_lists.keys().copied().collect();
                    stage_ids.sort_unstable();
                    for stage_idx in stage_ids {
                        let Some(objs) = object_lists.get_mut(&stage_idx) else {
                            continue;
                        };
                        for (obj_idx, obj) in objs.iter_mut().enumerate() {
                            if embedded_by_stage
                                .get(&stage_idx)
                                .map_or(false, |slots| slots.contains(&obj_idx))
                            {
                                continue;
                            }
                            if standalone_button_hit_recursive(obj) {
                                consumed_button = true;
                            }
                            if let Some(se_no) =
                                mark_standalone_button_pushed_from_hit_recursive(obj_idx, obj)
                            {
                                template_sounds.push(se_no);
                            }
                        }
                    }
                }
                input::VmMouseButton::Right => {
                    let mut candidates: Vec<(i64, usize, i64)> = Vec::new();
                    let mut group_stage_ids: Vec<i64> = group_lists.keys().copied().collect();
                    group_stage_ids.sort_unstable();
                    for stage_idx in group_stage_ids {
                        let Some(groups) = group_lists.get(&stage_idx) else {
                            continue;
                        };
                        for (group_idx, g) in groups.iter().enumerate() {
                            if g.is_doing() && g.cancel_flag {
                                candidates.push((g.cancel_priority, group_idx, stage_idx));
                            }
                        }
                    }
                    candidates.sort_by(|a, b| b.0.cmp(&a.0));
                    if let Some((_priority, group_idx, stage_idx)) = candidates.first().copied() {
                        if let Some(groups) = group_lists.get_mut(&stage_idx) {
                            if let Some(g) = groups.get_mut(group_idx) {
                                let was_waiting = g.wait_flag;
                                let cancel_se_no = g.cancel_se_no;
                                if g.cancel().is_some() {
                                    if sg_debug_enabled() {
                                        eprintln!(
                                            "[SG_DEBUG][GROUP] cancel form={} stage={} group={} wait={} result_button={} se={}",
                                            form_id, stage_idx, group_idx, was_waiting, g.result_button_no, cancel_se_no
                                        );
                                    }
                                    if was_waiting {
                                        self.stack.push(Value::Int(globals::TNM_GROUP_CANCELED));
                                    }
                                    g.wait_flag = false;
                                    direct_sounds.push(cancel_se_no);
                                    if self.globals.focused_stage_group
                                        == Some((form_id, stage_idx, group_idx))
                                    {
                                        self.globals.focused_stage_group = None;
                                    }
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        {
            let mwnd_hidden =
                self.globals.script.mwnd_disp_off_flag
                    || self.globals.syscom.hide_mwnd.onoff
                    || self.globals.syscom.msg_back_open;
            let syscom = self.globals.syscom.clone();
            if let Some(st) = self.globals.stage_forms.get_mut(&form_id) {
                let mut stage_ids: Vec<i64> = st.mwnd_lists.keys().copied().collect();
                stage_ids.sort_unstable();
                for stage_idx in stage_ids {
                    let Some(mwnds) = st.mwnd_lists.get_mut(&stage_idx) else {
                        continue;
                    };
                    for mwnd in mwnds {
                        if mwnd_hidden || !mwnd.open {
                            continue;
                        }
                        let Some((_, _)) = mwnd.window_pos else {
                            continue;
                        };
                        let Some((window_w, window_h)) = mwnd.window_size else {
                            continue;
                        };
                        if window_w <= 0 || window_h <= 0 {
                            continue;
                        }
                        for (button_idx, obj) in mwnd.button_list.iter_mut().enumerate() {
                            if !object_button_renderable_by_syscom(&syscom, obj)
                                || button_effective_disabled(&syscom, obj, Some(button_idx))
                                || syscom.mwnd_btn_touch_disable
                            {
                                continue;
                            }
                            if standalone_button_hit_recursive(obj) {
                                consumed_button = true;
                            }
                            if let Some(se_no) =
                                mark_standalone_button_pushed_from_hit_recursive(button_idx, obj)
                            {
                                template_sounds.push(se_no);
                            }
                        }
                        for (face_idx, obj) in mwnd.face_list.iter_mut().enumerate() {
                            if !object_button_renderable_by_syscom(&syscom, obj)
                                || button_effective_disabled(&syscom, obj, None)
                                || syscom.mwnd_btn_touch_disable
                            {
                                continue;
                            }
                            if standalone_button_hit_recursive(obj) {
                                consumed_button = true;
                            }
                            if let Some(se_no) =
                                mark_standalone_button_pushed_from_hit_recursive(face_idx, obj)
                            {
                                template_sounds.push(se_no);
                            }
                        }
                        for (object_idx, obj) in mwnd.object_list.iter_mut().enumerate() {
                            if !object_button_renderable_by_syscom(&syscom, obj)
                                || button_effective_disabled(&syscom, obj, None)
                                || syscom.mwnd_btn_touch_disable
                            {
                                continue;
                            }
                            if standalone_button_hit_recursive(obj) {
                                consumed_button = true;
                            }
                            if let Some(se_no) =
                                mark_standalone_button_pushed_from_hit_recursive(object_idx, obj)
                            {
                                template_sounds.push(se_no);
                            }
                        }
                    }
                }
            }
        }

        let consumed = consumed_button || !template_sounds.is_empty() || !direct_sounds.is_empty();
        for se_no in template_sounds {
            self.play_button_template_se(se_no, ButtonSeEvent::Push);
        }
        for se_no in direct_sounds {
            self.play_button_se_no(se_no);
        }
        consumed
    }

    fn handle_object_button_mouse_up(&mut self, b: input::VmMouseButton) -> bool {
        if !matches!(b, input::VmMouseButton::Left) {
            return false;
        }

        self.update_object_button_hover();
        if self.handle_mwnd_message_button_mouse_up() {
            return true;
        }

        let Some(form_id) = self.active_button_stage_form_id() else {
            return false;
        };
        let mut pending_button_actions = Vec::new();
        let mut sounds = Vec::new();
        let mut consumed_button = false;

        {
            let Some(st) = self.globals.stage_forms.get_mut(&form_id) else {
                return false;
            };

            let embedded_by_stage: HashMap<i64, HashSet<usize>> = st
                .embedded_object_slots
                .iter()
                .fold(HashMap::new(), |mut acc, (key, &slot)| {
                    if let Some((stage, _)) = key.split_once(':') {
                        if let Ok(stage_idx) = stage.parse::<i64>() {
                            acc.entry(stage_idx)
                                .or_insert_with(HashSet::new)
                                .insert(slot);
                        }
                    }
                    acc
                });
            let (object_lists, group_lists) = (&mut st.object_lists, &mut st.group_lists);

            let mut group_stage_ids: Vec<i64> = group_lists.keys().copied().collect();
            group_stage_ids.sort_unstable();
            for stage_idx in group_stage_ids {
                let Some(groups) = group_lists.get_mut(&stage_idx) else {
                    continue;
                };
                for (group_idx, g) in groups.iter_mut().enumerate() {
                    if !g.is_doing() {
                        continue;
                    }
                    let pushed = g.pushed_button_no;
                    let pushed_slot = g.pushed_runtime_slot;
                    let release_keeps_push = pushed_slot
                        .and_then(|slot| {
                            object_lists.get(&stage_idx).map(|objs| {
                                object_button_push_keep_in_list_by_runtime_slot(objs, slot)
                            })
                        })
                        .unwrap_or(false);
                    let released_on_same_button = pushed >= 0
                        && pushed_slot.is_some()
                        && (g.hit_runtime_slot == pushed_slot || release_keeps_push);
                    if released_on_same_button {
                        let was_waiting = g.wait_flag;
                        let action_slot = pushed_slot.unwrap();
                        if g.decide(pushed) {
                            if sg_debug_enabled() {
                                eprintln!(
                                    "[SG_DEBUG][GROUP] decide form={} stage={} group={} button={} slot={} wait={}",
                                    form_id, stage_idx, group_idx, pushed, action_slot, was_waiting
                                );
                            }
                            if let Some(objs) = object_lists.get(&stage_idx) {
                                if let Some(se_no) =
                                    find_button_se_no_in_list_by_runtime_slot(objs, action_slot)
                                {
                                    sounds.push(se_no);
                                }
                                for (obj_idx, obj) in objs.iter().enumerate() {
                                    if embedded_by_stage
                                        .get(&stage_idx)
                                        .map_or(false, |slots| slots.contains(&obj_idx))
                                    {
                                        continue;
                                    }
                                    collect_button_decided_action_by_runtime_slot_recursive(
                                        obj_idx,
                                        obj,
                                        action_slot,
                                        &mut pending_button_actions,
                                    );
                                }
                            }
                            if was_waiting {
                                self.stack.push(Value::Int(pushed));
                                g.wait_flag = false;
                                if self.globals.focused_stage_group
                                    == Some((form_id, stage_idx, group_idx))
                                {
                                    self.globals.focused_stage_group = None;
                                }
                            }
                        }
                    } else {
                        g.pushed_button_no = -1;
                        g.pushed_runtime_slot = None;
                    }
                }
            }

            let mut stage_ids: Vec<i64> = object_lists.keys().copied().collect();
            stage_ids.sort_unstable();
            for stage_idx in &stage_ids {
                let Some(objs) = object_lists.get(stage_idx) else {
                    continue;
                };
                for (obj_idx, obj) in objs.iter().enumerate() {
                    if embedded_by_stage
                        .get(stage_idx)
                        .map_or(false, |slots| slots.contains(&obj_idx))
                    {
                        continue;
                    }
                    if standalone_button_pushed_recursive(obj) {
                        consumed_button = true;
                    }
                    collect_standalone_button_decided_actions_recursive(
                        obj,
                        &mut pending_button_actions,
                        &mut sounds,
                    );
                }
            }

            for stage_idx in &stage_ids {
                let Some(objs) = object_lists.get_mut(stage_idx) else {
                    continue;
                };
                for (obj_idx, obj) in objs.iter_mut().enumerate() {
                    if embedded_by_stage
                        .get(stage_idx)
                        .map_or(false, |slots| slots.contains(&obj_idx))
                    {
                        continue;
                    }
                    clear_button_pushed_recursive(obj);
                }
            }
        }

        {
            let mwnd_hidden =
                self.globals.script.mwnd_disp_off_flag
                    || self.globals.syscom.hide_mwnd.onoff
                    || self.globals.syscom.msg_back_open;
            let syscom = self.globals.syscom.clone();
            if let Some(st) = self.globals.stage_forms.get_mut(&form_id) {
                let mut stage_ids: Vec<i64> = st.mwnd_lists.keys().copied().collect();
                stage_ids.sort_unstable();
                for stage_idx in &stage_ids {
                    let Some(mwnds) = st.mwnd_lists.get(stage_idx) else {
                        continue;
                    };
                    for mwnd in mwnds {
                        if mwnd_hidden || !mwnd.open {
                            continue;
                        }
                        let Some((_, _)) = mwnd.window_pos else {
                            continue;
                        };
                        let Some((window_w, window_h)) = mwnd.window_size else {
                            continue;
                        };
                        if window_w <= 0 || window_h <= 0 {
                            continue;
                        }
                        for (button_idx, obj) in mwnd.button_list.iter().enumerate() {
                            if !object_button_renderable_by_syscom(&syscom, obj)
                                || button_effective_disabled(&syscom, obj, Some(button_idx))
                                || syscom.mwnd_btn_touch_disable
                            {
                                continue;
                            }
                            collect_standalone_button_decided_actions_recursive(
                                obj,
                                &mut pending_button_actions,
                                &mut sounds,
                            );
                        }
                        for obj in &mwnd.face_list {
                            if !object_button_renderable_by_syscom(&syscom, obj)
                                || button_effective_disabled(&syscom, obj, None)
                                || syscom.mwnd_btn_touch_disable
                            {
                                continue;
                            }
                            collect_standalone_button_decided_actions_recursive(
                                obj,
                                &mut pending_button_actions,
                                &mut sounds,
                            );
                        }
                        for obj in &mwnd.object_list {
                            if !object_button_renderable_by_syscom(&syscom, obj)
                                || button_effective_disabled(&syscom, obj, None)
                                || syscom.mwnd_btn_touch_disable
                            {
                                continue;
                            }
                            collect_standalone_button_decided_actions_recursive(
                                obj,
                                &mut pending_button_actions,
                                &mut sounds,
                            );
                        }
                    }
                }
                for stage_idx in &stage_ids {
                    let Some(mwnds) = st.mwnd_lists.get_mut(stage_idx) else {
                        continue;
                    };
                    for mwnd in mwnds {
                        for obj in &mut mwnd.button_list {
                            clear_button_pushed_recursive(obj);
                        }
                        for obj in &mut mwnd.face_list {
                            clear_button_pushed_recursive(obj);
                        }
                        for obj in &mut mwnd.object_list {
                            clear_button_pushed_recursive(obj);
                        }
                    }
                }
            }
        }

        let consumed = consumed_button || !pending_button_actions.is_empty() || !sounds.is_empty();
        self.globals
            .pending_button_actions
            .extend(pending_button_actions);
        for se_no in sounds {
            self.play_button_template_se(se_no, ButtonSeEvent::Decide);
        }
        consumed
    }
    // ------------------------------------------------------------------
    // Input bridge (platform event -> VM state)
    // ------------------------------------------------------------------

    pub fn platform_shortcuts_blocked(&self) -> bool {
        self.globals.system.messagebox_modal.is_some()
            || self.globals.syscom.menu_open
            || self.globals.syscom.msg_back_open
            || self.globals.selbtn.started
            || self.globals.selbtn.processing_flag_0
            || self.globals.focused_editbox.is_some()
    }

    fn is_vm_key_disabled(&self, k: input::VmKey) -> bool {
        input::vmkey_to_vk_code(k)
            .map(|vk| self.globals.script.key_disable.contains(&vk))
            .unwrap_or(false)
    }

    pub fn on_key_down(&mut self, k: input::VmKey) {
        if self.handle_system_messagebox_key(k) {
            return;
        }
        if self.handle_msg_back_key(k) {
            return;
        }
        self.cancel_pending_editbox_composition();

        // A native Win32 EDIT owns keyboard focus before the engine's selection
        // handlers. Preserve that ordering for the cross-platform editor.
        let editbox_active = self.editbox_accepts_keyboard_input();
        let mut input_recorded = false;
        if editbox_active {
            if self.is_vm_key_disabled(k) {
                return;
            }
            self.input.on_key_down(k);
            input_recorded = true;
            if Self::is_modifier_key(k) {
                return;
            }
            if self.handle_editbox_key(k) {
                return;
            }
        }

        if self.globals.syscom.hide_mwnd.onoff
            && matches!(k, input::VmKey::Enter | input::VmKey::Escape | input::VmKey::Space)
        {
            if !input_recorded {
                self.input.on_key_down(k);
            }
            return;
        }
        if self.handle_selbtn_key(k) {
            return;
        }
        if self.is_vm_key_disabled(k) {
            return;
        }
        if !input_recorded {
            self.input.on_key_down(k);
        }
        if Self::is_modifier_key(k) {
            return;
        }

        let handled_mwnd_selection = self.handle_mwnd_selection_key(k);

        // Stage group selection runtime: map Enter/Escape to a decision.
        if !handled_mwnd_selection {
            if let Some((form_id, stage_idx, group_idx)) = self.globals.focused_stage_group {
                if let Some(st) = self.globals.stage_forms.get_mut(&form_id) {
                    if let Some(list) = st.group_lists.get_mut(&stage_idx) {
                        if let Some(g) = list.get_mut(group_idx) {
                            match k {
                                input::VmKey::Enter => {
                                    let button_no = if g.hit_button_no >= 0 {
                                        g.hit_button_no
                                    } else {
                                        0
                                    };
                                    let was_waiting = g.wait_flag;
                                    if g.decide(button_no) {
                                        if sg_debug_enabled() {
                                            eprintln!(
                                                "[SG_DEBUG][GROUP] key_decide form={} stage={} group={} button={} wait={}",
                                                form_id, stage_idx, group_idx, button_no, was_waiting
                                            );
                                        }
                                        if was_waiting {
                                            self.stack.push(Value::Int(button_no));
                                        }
                                        g.wait_flag = false;
                                        self.globals.focused_stage_group = None;
                                    }
                                }
                                input::VmKey::Escape => {
                                    let was_waiting = g.wait_flag;
                                    if g.cancel().is_some() {
                                        if sg_debug_enabled() {
                                            eprintln!(
                                                "[SG_DEBUG][GROUP] key_cancel form={} stage={} group={} wait={} result_button={}",
                                                form_id, stage_idx, group_idx, was_waiting, g.result_button_no
                                            );
                                        }
                                        if was_waiting {
                                            self.stack
                                                .push(Value::Int(globals::TNM_GROUP_CANCELED));
                                        }
                                        g.wait_flag = false;
                                        self.globals.focused_stage_group = None;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        if !self.advance_message_wait(true) {
            self.notify_wait_key();
        }
    }

    pub fn on_key_up(&mut self, k: input::VmKey) {
        self.cancel_pending_editbox_composition();
        if self.is_vm_key_disabled(k) {
            return;
        }
        self.input.on_key_up(k);
        if self.editbox_accepts_keyboard_input()
            && !matches!(k, input::VmKey::F(_) | input::VmKey::Other(_))
        {
            return;
        }
        if self.globals.syscom.hide_mwnd.onoff
            && matches!(k, input::VmKey::Enter | input::VmKey::Escape | input::VmKey::Space)
        {
            self.globals.syscom.hide_mwnd.onoff = false;
            return;
        }
        if let Some(vk) = input::vmkey_to_vk_code(k) {
            if self.input.vk_down_up_stock(vk) {
                match k {
                    input::VmKey::Enter | input::VmKey::Space => {
                        self.notify_movie_wait_down_up(1);
                    }
                    input::VmKey::Escape => {
                        self.notify_movie_wait_down_up(-1);
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn on_text_input(&mut self, text: &str) {
        if self.globals.system.messagebox_modal.is_some() {
            return;
        }
        let Some((form_id, idx)) = self.globals.focused_editbox else {
            return;
        };
        let Some(list) = self.globals.editbox_lists.get_mut(&form_id) else {
            return;
        };
        let Some(eb) = list.boxes.get_mut(idx) else {
            return;
        };
        if !eb.created || !eb.visible {
            return;
        }
        eb.commit_text(text);
    }

    pub fn on_ime_preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) {
        if self.globals.system.messagebox_modal.is_some() {
            return;
        }
        let Some((form_id, idx)) = self.globals.focused_editbox else {
            return;
        };
        let Some(eb) = self
            .globals
            .editbox_lists
            .get_mut(&form_id)
            .and_then(|list| list.boxes.get_mut(idx))
        else {
            return;
        };
        if !eb.created || !eb.visible {
            return;
        }
        eb.set_ime_preedit(text, cursor);
    }

    pub fn on_ime_disabled(&mut self) {
        let Some((form_id, idx)) = self.globals.focused_editbox else {
            return;
        };
        if let Some(eb) = self
            .globals
            .editbox_lists
            .get_mut(&form_id)
            .and_then(|list| list.boxes.get_mut(idx))
        {
            eb.cancel_composition();
        }
    }

    pub fn editbox_accepts_keyboard_input(&self) -> bool {
        let Some((form_id, idx)) = self.globals.focused_editbox else {
            return false;
        };
        self.globals
            .editbox_lists
            .get(&form_id)
            .and_then(|list| list.boxes.get(idx))
            .map(|eb| eb.created && eb.visible)
            .unwrap_or(false)
    }

    pub fn editbox_accepts_direct_text(&self) -> bool {
        let Some((form_id, idx)) = self.globals.focused_editbox else {
            return false;
        };
        let Some(editbox) = self
            .globals
            .editbox_lists
            .get(&form_id)
            .and_then(|list| list.boxes.get(idx))
        else {
            return false;
        };
        editbox.created && editbox.visible && !editbox.is_composing()
    }

    pub fn focused_editbox_ime_area(&self) -> Option<(i32, i32, i32, i32)> {
        let (form_id, idx) = self.globals.focused_editbox?;
        let eb = self.globals.editbox_lists.get(&form_id)?.boxes.get(idx)?;
        if !eb.created || !eb.visible {
            return None;
        }
        Some((
            eb.caret_window_x().max(eb.window_x),
            eb.window_y.saturating_add(1),
            1,
            eb.window_moji_size.max(1).min(eb.window_h.max(1)),
        ))
    }

    fn open_syscom_menu_from_cancel_key(&mut self) -> bool {
        // Original C++ cancel_call_proc(): right-click/Escape/Z is VK_EX_CANCEL.
        // When the local syscom menu is enabled, it clears read-skip and calls
        // tnm_syscom_open(); tnm_syscom_open() enters CANCEL_SCENE when configured.
        if self.globals.syscom.syscom_menu_disable {
            return false;
        }
        if self.globals.syscom.msg_back_open || self.globals.syscom.hide_mwnd.onoff {
            return false;
        }
        // Original C++ cancel_call_proc() does not open another cancel/syscom scene
        // while an EXCALL scene is active.  This is required for syscom scenes such
        // as CANCEL_SCENE/sys10_qm00: the right-click stock must remain available to
        // that scene's own script instead of recursively opening CANCEL_SCENE again.
        if self.excall_state.ex_call_flag {
            return false;
        }
        if self.movie.current().is_some() {
            return false;
        }
        self.globals.syscom.read_skip.onoff = false;
        self.globals.syscom.pending_proc = Some(globals::SyscomPendingProc {
            kind: globals::SyscomPendingProcKind::OpenSyscomMenu,
            warning: false,
            se_play: false,
            fade_out: false,
            leave_msgbk: false,
            save_id: 0,
        });
        if trace_env().proc_flow_trace {
            eprintln!(
                "[SG_PROC_FLOW] open_syscom_menu_from_cancel_key scene={:?} line={} pending_proc={:?}",
                self.current_scene_name,
                self.current_line_no,
                self.globals.syscom.pending_proc
            );
        }
        true
    }

    pub fn on_mouse_move(&mut self, x: i32, y: i32) {
        self.input.on_mouse_move(x, y);
        if self.update_editbox_drag_selection() {
            return;
        }
        if let Some(idx) = self.selbtn_hit_index(x, y) {
            if let Some(pressed) = self.globals.selbtn.pressed_index {
                let inside = pressed == idx;
                if self.globals.selbtn.pressed_inside != inside {
                    self.globals.selbtn.pressed_inside = inside;
                    self.sync_selbtn_item_selection();
                }
            }
            if self.globals.selbtn.cursor != idx {
                self.globals.selbtn.cursor = idx;
                self.sync_selbtn_item_selection();
            }
            return;
        }
        if self.globals.selbtn.pressed_index.is_some() && self.globals.selbtn.pressed_inside {
            self.globals.selbtn.pressed_inside = false;
            self.sync_selbtn_item_selection();
        }
        if self.handle_msg_back_mouse_move() {
            return;
        }
        self.update_object_button_hover();
    }

    pub fn on_mouse_down(&mut self, b: input::VmMouseButton) {
        if sg_input_trace_enabled() {
            eprintln!(
                "[SG_DEBUG][INPUT] mouse_down {:?} at=({}, {})",
                b, self.input.mouse_x, self.input.mouse_y
            );
        }
        if self.handle_system_messagebox_click(b) {
            return;
        }
        if self.globals.syscom.msg_back_open {
            self.input.on_mouse_down(b);
            self.handle_msg_back_mouse_down(b);
            return;
        }
        self.cancel_pending_editbox_composition();
        if self.begin_editbox_mouse_down(b) {
            self.input.on_mouse_down(b);
            return;
        }
        if self.handle_selbtn_mouse_down(b) {
            return;
        }
        let handled_mwnd_selection = self.handle_mwnd_selection_click(b);
        self.input.on_mouse_down(b);
        let handled_button = if !handled_mwnd_selection {
            self.handle_object_button_mouse_down(b)
        } else {
            false
        };
        if matches!(b, input::VmMouseButton::Right) && handled_button {
            self.suppress_next_right_syscom_open = true;
        }
        if !handled_button {
            if !self.advance_message_wait(true) {
                self.notify_wait_key();
            }
        }
    }

    fn begin_editbox_mouse_down(&mut self, b: input::VmMouseButton) -> bool {
        if !matches!(b, input::VmMouseButton::Left) {
            return false;
        }
        let x = self.input.mouse_x;
        let y = self.input.mouse_y;
        let mut hits = Vec::new();
        for (form_id, list) in &self.globals.editbox_lists {
            for (idx, eb) in list.boxes.iter().enumerate() {
                if eb.contains_point(x, y) {
                    hits.push((*form_id, idx));
                }
            }
        }
        hits.sort_unstable();
        let Some(target) = hits.into_iter().next_back() else {
            self.set_focused_editbox(None);
            return false;
        };
        self.set_focused_editbox(Some(target));
        let extend = self.input.vk_is_down(0x10);
        if let Some(eb) = self
            .globals
            .editbox_lists
            .get_mut(&target.0)
            .and_then(|list| list.boxes.get_mut(target.1))
        {
            eb.set_cursor_from_window_x(x, extend);
            eb.mouse_selecting = true;
        }
        true
    }

    fn update_editbox_drag_selection(&mut self) -> bool {
        let Some((form_id, idx)) = self.globals.focused_editbox else {
            return false;
        };
        if !self.input.vk_is_down(0x01) {
            return false;
        }
        let x = self.input.mouse_x;
        let Some(eb) = self
            .globals
            .editbox_lists
            .get_mut(&form_id)
            .and_then(|list| list.boxes.get_mut(idx))
        else {
            return false;
        };
        if !eb.mouse_selecting {
            return false;
        }
        eb.set_cursor_from_window_x(x, true);
        true
    }

    fn finish_editbox_mouse_up(&mut self, b: input::VmMouseButton) -> bool {
        if !matches!(b, input::VmMouseButton::Left) {
            return false;
        }
        let Some((form_id, idx)) = self.globals.focused_editbox else {
            return false;
        };
        let Some(eb) = self
            .globals
            .editbox_lists
            .get_mut(&form_id)
            .and_then(|list| list.boxes.get_mut(idx))
        else {
            return false;
        };
        let was_selecting = eb.mouse_selecting;
        eb.mouse_selecting = false;
        was_selecting
    }

    pub fn on_mouse_up(&mut self, b: input::VmMouseButton) {
        if sg_input_trace_enabled() {
            eprintln!(
                "[SG_DEBUG][INPUT] mouse_up {:?} at=({}, {})",
                b, self.input.mouse_x, self.input.mouse_y
            );
        }
        self.input.on_mouse_up(b);
        if self.finish_editbox_mouse_up(b) {
            return;
        }
        if self.handle_msg_back_mouse_up(b) {
            return;
        }
        if self.handle_selbtn_mouse_up(b) {
            return;
        }
        let movie_skipped = match b {
            input::VmMouseButton::Left if self.input.vk_down_up_stock(0x01) => {
                self.notify_movie_wait_down_up(1)
            }
            input::VmMouseButton::Right if self.input.vk_down_up_stock(0x02) => {
                self.notify_movie_wait_down_up(-1)
            }
            _ => false,
        };
        if movie_skipped {
            return;
        }
        if matches!(b, input::VmMouseButton::Right) && self.input.vk_down_up_stock(0x02) {
            if std::mem::take(&mut self.suppress_next_right_syscom_open) {
                return;
            }
            if self.open_syscom_menu_from_cancel_key() {
                return;
            }
        }
        let _ = self.handle_object_button_mouse_up(b);
        // Generic/message key waits are already advanced from mouse-down.
        // Do not consume the same physical click again on mouse-up: the
        // original engine uses consumable DOWN_UP stock
        // (tnm_input_use_key_down_up), so one click cannot both reveal the
        // current message and dismiss the following MESSAGE_KEY_WAIT.
        // Down-up-specific waits (TIMEWAIT_KEY/MOV/OBJECT movie etc.) were
        // handled above by notify_movie_wait_down_up().
    }

    pub fn on_mouse_wheel(&mut self, delta_y: i32) {
        self.input.on_mouse_wheel(delta_y);
        if self.globals.syscom.msg_back_open {
            if delta_y > 0 {
                self.msg_back_target_up();
            } else if delta_y < 0 {
                self.msg_back_target_down();
            }
            return;
        }
        if delta_y > 0 && self.msg_back_is_enable() {
            self.open_msg_back_proc();
            return;
        }
        if !self.advance_message_wait(self.should_wheel_advance_message()) {
            self.notify_wait_key();
        }
    }

    fn finish_skipped_movie_waits(&mut self) {
        while let Some(info) = self.wait.take_movie_skip() {
            let Some(st) = self.globals.stage_forms.get_mut(&info.stage_form_id) else {
                continue;
            };
            let Some(list) = st.object_lists.get_mut(&info.stage_idx) else {
                continue;
            };
            let Some(obj) = find_object_by_runtime_slot_mut(list, info.runtime_slot) else {
                continue;
            };

            // Key skip triggers C_elm_object::init_type(true) on the actual object that owns
            // the movie, including nested CHILD objects addressed by runtime slot.
            let audio_id = obj.movie.audio_id.take();
            let backend = obj.backend.clone();
            obj.init_type_like();

            if let Some(id) = audio_id {
                self.movie.stop_audio(id);
            }
            if let globals::ObjectBackend::Movie {
                layer_id,
                sprite_id,
                ..
            } = backend
            {
                if let Some(layer) = self.layers.layer_mut(layer_id) {
                    if let Some(sprite) = layer.sprite_mut(sprite_id) {
                        sprite.visible = false;
                        sprite.image_id = None;
                    }
                }
            }
        }
    }

    fn handle_editbox_key(&mut self, k: input::VmKey) -> bool {
        let Some((form_id, idx)) = self.globals.focused_editbox else {
            return false;
        };
        let alt_down = self.input.vk_is_down(0x12);
        let shift_down = self.input.vk_is_down(0x10);
        let shortcut_down = self.input.vk_is_down(0x11) || self.input.vk_is_down(0x5B);
        let clipboard = self.globals.editbox_clipboard.clone();
        let mut move_focus: Option<bool> = None;
        let mut toggle_screen = false;
        let mut copied_text: Option<String> = None;
        let consumed = {
            let Some(eb) = self
                .globals
                .editbox_lists
                .get_mut(&form_id)
                .and_then(|list| list.boxes.get_mut(idx))
            else {
                return false;
            };
            if !eb.created || !eb.visible {
                return false;
            }

            // While an IME owns composition, editing keys must stay with the IME.
            // winit will subsequently deliver the updated Preedit/Commit event.
            if eb.is_composing() {
                !matches!(k, input::VmKey::F(_) | input::VmKey::Other(_))
            } else {
                match k {
                    input::VmKey::Enter => {
                        if alt_down {
                            toggle_screen = true;
                        } else {
                            eb.action_flag =
                                crate::runtime::globals::EDITBOX_ACTION_DECIDED;
                        }
                        true
                    }
                    input::VmKey::Escape => {
                        eb.action_flag = crate::runtime::globals::EDITBOX_ACTION_CANCELED;
                        true
                    }
                    input::VmKey::Backspace => {
                        eb.delete_backward(shortcut_down);
                        true
                    }
                    input::VmKey::Delete => {
                        eb.delete_forward(shortcut_down);
                        true
                    }
                    input::VmKey::ArrowLeft => {
                        eb.move_cursor_left(shift_down, shortcut_down);
                        true
                    }
                    input::VmKey::ArrowRight => {
                        eb.move_cursor_right(shift_down, shortcut_down);
                        true
                    }
                    input::VmKey::ArrowUp | input::VmKey::ArrowDown => true,
                    input::VmKey::Home => {
                        eb.move_cursor_home(shift_down);
                        true
                    }
                    input::VmKey::End => {
                        eb.move_cursor_end(shift_down);
                        true
                    }
                    input::VmKey::Tab => {
                        move_focus = Some(!shift_down);
                        true
                    }
                    input::VmKey::Letter(letter) if shortcut_down => {
                        match letter.to_ascii_uppercase() {
                            'A' => eb.select_all(),
                            'C' => copied_text = eb.selected_text(),
                            'X' => copied_text = eb.cut_selection(),
                            'V' if !clipboard.is_empty() => eb.commit_text(&clipboard),
                            'V' => {},
                            'Z' if shift_down => eb.redo(),
                            'Z' => eb.undo(),
                            'Y' => eb.redo(),
                            _ => {}
                        }
                        true
                    }
                    input::VmKey::Space
                    | input::VmKey::Letter(_)
                    | input::VmKey::Digit(_) => true,
                    input::VmKey::Shift
                    | input::VmKey::Control
                    | input::VmKey::Meta
                    | input::VmKey::Alt => false,
                    input::VmKey::F(_) | input::VmKey::Other(_) => false,
                }
            }
        };

        if let Some(text) = copied_text {
            self.globals.editbox_clipboard = text;
        }
        if toggle_screen {
            self.toggle_screen_size_mode_for_editbox();
        }
        if let Some(forward) = move_focus {
            self.move_editbox_focus(forward);
        }
        consumed
    }

    pub fn wait_poll(&mut self) -> bool {
        self.poll_native_messagebox_result();
        // TNM_PROC_TYPE_MESSAGE_WAIT is not a key wait: it completes as soon
        // as C_elm_mwnd has revealed the full typewriter message.
        if self.wait.message_reveal_waiting() && self.ui.message_wait_text_fully_revealed() {
            self.wait.finish_message_reveal();
            // PP/R/PAGE may still have MESSAGE_KEY_WAIT active; in that case
            // keep UiRuntime::waiting alive so input/auto-mode can advance it.
            if !self.wait.waiting_for_key() {
                self.ui.finish_message_reveal_wait();
            }
        }
        let (wait, stack, bgm, koe, se, pcm, globals) = (
            &mut self.wait,
            &mut self.stack,
            &mut self.bgm,
            &mut self.koe,
            &mut self.se,
            &mut self.pcm,
            &mut self.globals,
        );
        wait.poll(stack, bgm, koe, se, pcm, globals, &self.ids)
    }

    pub fn push(&mut self, v: Value) {
        self.stack.push(v);
    }

    pub fn pop(&mut self) -> Option<Value> {
        self.stack.pop()
    }

    pub fn set_native_ui_backend(
        &mut self,
        backend: Option<Arc<dyn native_ui::NativeUiBackend>>,
    ) {
        self.native_ui_backend = backend;
    }

    /// Return the game title for platform UI and runtime dialogs.
    ///
    /// The value is read from Gameexe `GAMENAME` when available. If Gameexe is
    /// missing, undecodable, or the field is empty, this returns the project
    /// directory name, then `Siglus` as the final fallback.
    pub fn game_title(&self) -> String {
        game_title::resolve_game_title(self.tables.gameexe.as_ref(), &self.project_dir)
    }

    /// Return the game display name for bundle/mobile UI.
    pub fn game_name(&self) -> String {
        self.game_title()
    }

    /// Return display metadata for platform UI.
    ///
    /// If the game directory contains `cover.png`, `cover.jpg`, `cover.jpeg`,
    /// `thumbnail.png`, or `icon.png`, `cover` is populated. Otherwise callers
    /// should display the game name.
    pub fn game_display_info(&self) -> game_display_info::GameDisplayInfo {
        let cover = game_display_info::resolve_game_cover_from_project_dir(&self.project_dir);
        let name = self.game_name();
        game_display_info::GameDisplayInfo {
            title: name.clone(),
            name,
            cover,
        }
    }

    /// Return the optional cover for bundle/mobile UI.
    pub fn game_cover(&self) -> Option<game_display_info::GameCover> {
        game_display_info::resolve_game_cover_from_project_dir(&self.project_dir)
    }

    pub fn submit_native_messagebox_result(&mut self, request_id: u64, value: i64) {
        self.native_ui
            .enqueue_messagebox_result(request_id, value);
        self.poll_native_messagebox_result();
    }

    pub fn request_system_messagebox(
        &mut self,
        kind: i32,
        debug_only: bool,
        text: String,
        buttons: Vec<globals::SystemMessageBoxButton>,
    ) {
        self.request_system_messagebox_internal(kind, debug_only, text, buttons, true);
    }

    pub fn request_system_messagebox_no_return(
        &mut self,
        kind: i32,
        debug_only: bool,
        text: String,
        buttons: Vec<globals::SystemMessageBoxButton>,
    ) {
        self.request_system_messagebox_internal(kind, debug_only, text, buttons, false);
    }

    /// Show an engine-rendered modal without delegating to a platform-native
    /// message box.  Syscom's built-in fallback menus use this path because
    /// they are multi-step in-game UI and must behave identically on every
    /// winit target.
    pub fn request_internal_system_messagebox_no_return(
        &mut self,
        kind: i32,
        debug_only: bool,
        text: String,
        buttons: Vec<globals::SystemMessageBoxButton>,
    ) {
        let request_id = self.native_ui.next_messagebox_request_id();
        self.globals.system.messagebox_modal_result = None;
        self.globals.system.messagebox_modal = Some(globals::SystemMessageBoxModalState {
            request_id,
            kind,
            text,
            debug_only,
            buttons,
            cursor: 0,
            native_pending: false,
            complete_wait_with_value: false,
        });
        self.wait.wait_system_modal();
    }

    fn request_system_messagebox_internal(
        &mut self,
        kind: i32,
        debug_only: bool,
        text: String,
        buttons: Vec<globals::SystemMessageBoxButton>,
        complete_wait_with_value: bool,
    ) {
        let request_id = self.native_ui.next_messagebox_request_id();
        let native_pending = self.native_ui_backend.is_some();
        self.globals.system.messagebox_modal_result = None;
        self.globals.system.messagebox_modal = Some(globals::SystemMessageBoxModalState {
            request_id,
            kind,
            text: text.clone(),
            debug_only,
            buttons,
            cursor: 0,
            native_pending,
            complete_wait_with_value,
        });
        self.wait.wait_system_modal();

        if let Some(backend) = self.native_ui_backend.as_ref() {
            backend.show_system_messagebox(native_ui::NativeMessageBoxRequest {
                request_id,
                kind: native_ui::NativeMessageBoxKind::from_system_op(kind),
                title: self.game_title(),
                message: text,
                buttons: self.globals.system.messagebox_modal
                    .as_ref()
                    .map(|modal| {
                        modal
                            .buttons
                            .iter()
                            .map(|button| native_ui::NativeMessageBoxButton {
                                label: button.label.clone(),
                                value: button.value,
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                debug_only,
            });
        }
    }

    fn poll_native_messagebox_result(&mut self) {
        while let Some(result) = self.native_ui.pop_messagebox_result() {
            let Some(modal) = self.globals.system.messagebox_modal.as_ref() else {
                continue;
            };
            if modal.request_id != result.request_id {
                continue;
            }
            let max_value = modal.buttons.iter().map(|b| b.value).max().unwrap_or(0);
            let value = result.value.clamp(0, max_value);
            self.finish_system_messagebox(value);
            break;
        }
    }

    pub fn set_screen_size(&mut self, w: u32, h: u32) {
        self.screen_w = w;
        self.screen_h = h;
        self.ui.sync_layout(&mut self.layers, w, h);
        self.sync_editbox_runtime();
    }

    pub fn tick_frame(&mut self) {
        let now = crate::platform_time::Instant::now();
        let last = self.frame_clock_last.replace(now);
        let elapsed_ms = last
            .map(|t| now.saturating_duration_since(t).as_millis() as i32)
            .unwrap_or(16);
        let real_delta_ms = elapsed_ms.max(0);
        // eng_frame.cpp advances game-time and wipe-time at 32x while the
        // engine is in Ctrl/read/script-trigger skip. Real-time audio/fades do
        // not accelerate.
        let skipping = self.runtime_is_skipping();
        let game_delta_ms = if skipping {
            real_delta_ms.saturating_mul(32)
        } else {
            real_delta_ms
        };
        self.update_selbtn_animation(game_delta_ms as i64);
        let trace = trace_env().sg_ctx_tick_trace;
        if trace {
            eprintln!(
                "[SG_CTX_TICK] start game_delta_ms={} real_delta_ms={}",
                game_delta_ms, real_delta_ms
            );
        }
        self.sync_editbox_runtime();
        self.poll_native_messagebox_result();
        syscom_form::poll_fallback_dialog(self);
        if trace {
            eprintln!("[SG_CTX_TICK] after sync_editbox_runtime");
        }
        self.sync_mwnd_window_ui();
        if trace {
            eprintln!("[SG_CTX_TICK] after sync_mwnd_window_ui");
        }
        self.ui.tick(
            &mut self.layers,
            &mut self.images,
            &self.project_dir,
            self.screen_w,
            self.screen_h,
            &self.globals.script,
            &self.globals.syscom,
            &self.globals.editbox_lists,
            self.globals.focused_editbox,
        );
        for ((form_id, stage_idx, mwnd_idx), visible) in self.ui.mwnd_reveal_counts() {
            if let Some(mwnd) = self
                .globals
                .stage_forms
                .get_mut(&form_id)
                .and_then(|st| st.mwnd_lists.get_mut(&stage_idx))
                .and_then(|list| list.get_mut(mwnd_idx))
            {
                for page in &mut mwnd.message_pages {
                    for glyph in &mut page.glyphs {
                        glyph.appeared = glyph.ruby || glyph.reveal_index <= visible;
                    }
                    let page_body = page.glyphs.iter().filter(|glyph| !glyph.ruby).count();
                    let page_first = page
                        .glyphs
                        .iter()
                        .filter(|glyph| !glyph.ruby)
                        .map(|glyph| glyph.reveal_index)
                        .min()
                        .unwrap_or(1);
                    page.disp_moji_cnt = visible
                        .saturating_sub(page_first.saturating_sub(1))
                        .min(page_body)
                        .min(i32::MAX as usize) as i64;
                }
                for glyph in &mut mwnd.glyphs {
                    glyph.appeared = glyph.ruby || glyph.reveal_index <= visible;
                }
                let active_first = mwnd
                    .glyphs
                    .iter()
                    .filter(|glyph| !glyph.ruby)
                    .map(|glyph| glyph.reveal_index)
                    .min()
                    .unwrap_or(visible.saturating_add(1));
                let active_body = mwnd.glyphs.iter().filter(|glyph| !glyph.ruby).count();
                mwnd.disp_moji_cnt = visible
                    .saturating_sub(active_first.saturating_sub(1))
                    .min(active_body)
                    .min(i32::MAX as usize) as i64;
            }
        }
        // Apply syscom flags that should skip visual transitions immediately.
        self.apply_syscom_skip_flags();
        if trace {
            eprintln!("[SG_CTX_TICK] after apply_syscom_skip_flags");
        }
        // Sync message length for auto-mode timing.
        self.globals.script.auto_mode_moji_cnt =
            self.ui.message_text().unwrap_or("").chars().count() as i64;
        if self
            .ui
            .auto_advance_due(&self.globals.script, &self.globals.syscom)
        {
            if !self.advance_message_wait(true) {
                self.notify_wait_key();
            }
        }
        // Message-window hide is visibility-only here.  UI.tick applies script hide,
        // SYSCOM hide, and message-back proc state without clearing message contents.
        self.sync_syscom_menu_ui();
        if trace {
            eprintln!("[SG_CTX_TICK] after sync_syscom_menu_ui");
        }
        self.sync_mwnd_selection_ui();
        if trace {
            eprintln!("[SG_CTX_TICK] after sync_mwnd_selection_ui");
        }
        // C++ eng_frame.cpp updates the allocated EXCALL runtime even while
        // local SCRIPT.time_stop_flag freezes the gameplay scene.  Resolve the
        // synthetic EXCALL storage ids before mutably borrowing globals.
        let excall_tick = self
            .excall_state
            .ready
            .then(|| crate::runtime::forms::excall::tick_targets(self));
        self.globals.tick_frame(
            game_delta_ms,
            real_delta_ms,
            &self.tables.shake_templates,
            excall_tick,
        );
        if trace {
            eprintln!("[SG_CTX_TICK] after globals.tick_frame");
        }
        if self
            .globals
            .wipe
            .as_ref()
            .is_some_and(globals::WipeState::is_done)
        {
            self.finish_wipe_runtime();
            if trace {
                eprintln!("[SG_CTX_TICK] after finish_wipe_runtime");
            }
        }
        self.apply_object_event_animations();
        if trace {
            eprintln!("[SG_CTX_TICK] after apply_object_event_animations");
        }
        self.sync_weather_objects(game_delta_ms, real_delta_ms);
        if trace {
            eprintln!("[SG_CTX_TICK] after sync_weather_objects");
        }
        pcmevent_form::tick_all(self, game_delta_ms, real_delta_ms);
        let _ = self.bgm.tick(&mut self.audio);
        self.pcm.tick();
        syscom_form::update_audio_routing(self, real_delta_ms, false);
        if trace {
            eprintln!("[SG_CTX_TICK] after pcmevent/bgm/pcm/audio routing");
        }
        self.sync_emote_objects();
        self.sync_movie_objects();
        if trace {
            eprintln!("[SG_CTX_TICK] after sync_movie_objects");
        }
        self.sync_global_movie();
        if trace {
            eprintln!("[SG_CTX_TICK] after sync_global_movie");
        }
        self.update_object_button_hover();
        if trace {
            eprintln!("[SG_CTX_TICK] after update_object_button_hover");
        }
        self.apply_object_disp_override();
        if trace {
            eprintln!("[SG_CTX_TICK] after apply_object_disp_override");
        }
    }

    fn apply_syscom_skip_flags(&mut self) {
        let no_wipe = self
            .globals
            .syscom
            .original_config
            .no_wipe_anime_flag;
        // CONFIG.SKIP_WIPE_ANIME controls whether WIPE_WAIT accepts input;
        // only CONFIG.NO_WIPE_ANIME suppresses the transition itself.
        if no_wipe && self.globals.wipe.is_some() {
            self.finish_wipe_runtime();
        }
    }

    fn apply_object_event_animations(&mut self) {
        let wipe_active = self.globals.wipe.is_some();
        let ids = self.ids.clone();
        let gfx = &mut self.gfx;
        let images = &mut self.images;
        let layers = &mut self.layers;
        let mwnd_ui_state = self
            .ui
            .current_mwnd_window_render_state(self.screen_w, self.screen_h);
        let mut form_ids: Vec<u32> = self.globals.stage_forms.keys().copied().collect();
        form_ids.sort_unstable();
        for form_id in form_ids {
            let Some(st) = self.globals.stage_forms.get_mut(&form_id) else {
                continue;
            };
            let mut stage_ids: Vec<i64> = st
                .object_lists
                .keys()
                .chain(st.mwnd_lists.keys())
                .copied()
                .collect();
            stage_ids.sort_unstable();
            stage_ids.dedup();
            for stage_idx in stage_ids {
                if stage_idx == TNM_STAGE_NEXT_I64 && !wipe_active {
                    continue;
                }
                let embedded_prefix = format!("{stage_idx}:");
                let embedded_slots: HashSet<usize> = st
                    .embedded_object_slots
                    .iter()
                    .filter_map(|(key, &slot)| key.starts_with(&embedded_prefix).then_some(slot))
                    .collect();
                let Some(objs) = st.object_lists.get_mut(&stage_idx) else {
                    continue;
                };
                for (obj_idx, obj) in objs.iter_mut().enumerate() {
                    if embedded_slots.contains(&obj_idx) {
                        continue;
                    }
                    apply_object_event_animations_recursive(
                        &ids,
                        gfx,
                        images,
                        layers,
                        stage_idx,
                        object_runtime_slot(obj_idx, obj) as i64,
                        obj,
                    );
                }
            }

            let mut mwnd_stage_ids: Vec<i64> = st.mwnd_lists.keys().copied().collect();
            mwnd_stage_ids.sort_unstable();
            for stage_idx in mwnd_stage_ids {
                if stage_idx == TNM_STAGE_NEXT_I64 && !wipe_active {
                    continue;
                }
                let Some(mwnds) = st.mwnd_lists.get_mut(&stage_idx) else {
                    continue;
                };
                for mwnd in mwnds {
                    let Some((window_x, window_y)) = mwnd.window_pos else {
                        continue;
                    };
                    let Some((window_w, window_h)) = mwnd.window_size else {
                        continue;
                    };
                    if window_w <= 0 || window_h <= 0 {
                        continue;
                    }
                    let visible_or_animating = mwnd.open
                        || mwnd_ui_state.map_or(false, |ui| {
                            ui.x as i64 == window_x
                                && ui.y as i64 == window_y
                                && ui.w as i64 == window_w
                                && ui.h as i64 == window_h
                        });
                    if !visible_or_animating {
                        continue;
                    }
                    for (obj_idx, obj) in mwnd.button_list.iter_mut().enumerate() {
                        apply_object_event_animations_recursive(
                            &ids,
                            gfx,
                            images,
                            layers,
                            stage_idx,
                            object_runtime_slot(obj_idx, obj) as i64,
                            obj,
                        );
                    }
                    for (obj_idx, obj) in mwnd.face_list.iter_mut().enumerate() {
                        apply_object_event_animations_recursive(
                            &ids,
                            gfx,
                            images,
                            layers,
                            stage_idx,
                            object_runtime_slot(obj_idx, obj) as i64,
                            obj,
                        );
                    }
                    for (obj_idx, obj) in mwnd.object_list.iter_mut().enumerate() {
                        apply_object_event_animations_recursive(
                            &ids,
                            gfx,
                            images,
                            layers,
                            stage_idx,
                            object_runtime_slot(obj_idx, obj) as i64,
                            obj,
                        );
                    }
                }
            }
        }
    }

    fn apply_object_masks(&mut self) {
        let Some(mask_info) = self.build_mask_info() else {
            return;
        };
        if mask_info.is_empty() {
            return;
        }

        let mut resolved_masks = HashMap::new();
        for (mask_name, _, _) in mask_info.iter().flatten() {
            if resolved_masks.contains_key(mask_name) {
                continue;
            }
            if let Some(id) = self.resolve_mask_image(mask_name) {
                resolved_masks.insert(mask_name.clone(), id);
            }
        }

        let ids = self.ids.clone();
        let gfx = &mut self.gfx;
        let layers = &mut self.layers;
        let mut form_ids: Vec<u32> = self.globals.stage_forms.keys().copied().collect();
        form_ids.sort_unstable();
        for form_id in form_ids {
            let Some(st) = self.globals.stage_forms.get_mut(&form_id) else {
                continue;
            };
            let mut stage_ids: Vec<i64> = st.object_lists.keys().copied().collect();
            stage_ids.sort_unstable();
            for stage_idx in stage_ids {
                let Some(objs) = st.object_lists.get_mut(&stage_idx) else {
                    continue;
                };
                for (obj_idx, obj) in objs.iter_mut().enumerate() {
                    apply_object_masks_recursive(
                        &ids,
                        gfx,
                        layers,
                        stage_idx,
                        object_runtime_slot(obj_idx, obj) as i64,
                        obj,
                        &mask_info,
                        &resolved_masks,
                    );
                }
            }
        }
    }

    fn active_mask_list(&self) -> Option<&globals::MaskListState> {
        if self.ids.form_global_mask != 0 {
            return self.globals.mask_lists.get(&self.ids.form_global_mask);
        }
        None
    }

    fn build_mask_info(&self) -> Option<Vec<Option<(String, i32, i32)>>> {
        let ml = self.active_mask_list()?;
        let mut out = Vec::with_capacity(ml.masks.len());
        for m in &ml.masks {
            let Some(name) = m.name.as_ref() else {
                out.push(None);
                continue;
            };
            if name.is_empty() {
                out.push(None);
                continue;
            }
            let x = m.x_event.get_total_value();
            let y = m.y_event.get_total_value();
            out.push(Some((name.clone(), x, y)));
        }
        Some(out)
    }

    fn resolve_mask_image(&mut self, name: &str) -> Option<ImageId> {
        if name.is_empty() {
            return None;
        }
        if let Some(path) = resolve_mask_path(&self.project_dir, name) {
            if let Ok(id) = self.images.load_file(&path, 0) {
                return Some(id);
            }
        }
        if let Ok(id) = self.images.load_g00(name, 0) {
            return Some(id);
        }
        if let Ok(id) = self.images.load_bg(name) {
            return Some(id);
        }
        None
    }

    fn apply_object_tonecurves(&mut self) {
        let ids = self.ids.clone();
        let gfx = &mut self.gfx;
        let images = &mut self.images;
        let layers = &mut self.layers;
        let tonecurve = &mut self.tonecurve;
        let mut form_ids: Vec<u32> = self.globals.stage_forms.keys().copied().collect();
        form_ids.sort_unstable();
        for form_id in form_ids {
            let Some(st) = self.globals.stage_forms.get_mut(&form_id) else {
                continue;
            };
            let mut stage_ids: Vec<i64> = st.object_lists.keys().copied().collect();
            stage_ids.sort_unstable();
            for stage_idx in stage_ids {
                let Some(objs) = st.object_lists.get_mut(&stage_idx) else {
                    continue;
                };
                for (obj_idx, obj) in objs.iter_mut().enumerate() {
                    apply_object_tonecurves_recursive(
                        &ids,
                        gfx,
                        images,
                        layers,
                        tonecurve,
                        stage_idx,
                        object_runtime_slot(obj_idx, obj) as i64,
                        obj,
                    );
                }
            }
        }
    }

    fn apply_gan_effects(&mut self, sprites: &mut Vec<RenderSprite>) {
        let mut index: HashMap<(Option<LayerId>, Option<SpriteId>), usize> = HashMap::new();
        for (i, s) in sprites.iter().enumerate() {
            index.insert((s.layer_id, s.sprite_id), i);
        }

        let gfx = &mut self.gfx;
        let images = &mut self.images;
        let mut form_ids: Vec<u32> = self.globals.stage_forms.keys().copied().collect();
        form_ids.sort_unstable();
        for form_id in form_ids {
            let Some(st) = self.globals.stage_forms.get_mut(&form_id) else {
                continue;
            };

            let mut object_stage_ids: Vec<i64> = st.object_lists.keys().copied().collect();
            object_stage_ids.sort_unstable();
            for stage_idx in object_stage_ids {
                let Some(objs) = st.object_lists.get_mut(&stage_idx) else {
                    continue;
                };
                for (obj_idx, obj) in objs.iter_mut().enumerate() {
                    apply_gan_effects_recursive(
                        gfx,
                        images,
                        sprites,
                        &index,
                        stage_idx,
                        object_runtime_slot(obj_idx, obj) as i64,
                        obj,
                    );
                }
            }

            let mut mwnd_stage_ids: Vec<i64> = st.mwnd_lists.keys().copied().collect();
            mwnd_stage_ids.sort_unstable();
            for stage_idx in mwnd_stage_ids {
                let Some(mwnds) = st.mwnd_lists.get_mut(&stage_idx) else {
                    continue;
                };
                for mwnd in mwnds {
                    for (obj_idx, obj) in mwnd.button_list.iter_mut().enumerate() {
                        apply_gan_effects_recursive(
                            gfx,
                            images,
                            sprites,
                            &index,
                            stage_idx,
                            object_runtime_slot(obj_idx, obj) as i64,
                            obj,
                        );
                    }
                    for (obj_idx, obj) in mwnd.face_list.iter_mut().enumerate() {
                        apply_gan_effects_recursive(
                            gfx,
                            images,
                            sprites,
                            &index,
                            stage_idx,
                            object_runtime_slot(obj_idx, obj) as i64,
                            obj,
                        );
                    }
                    for (obj_idx, obj) in mwnd.object_list.iter_mut().enumerate() {
                        apply_gan_effects_recursive(
                            gfx,
                            images,
                            sprites,
                            &index,
                            stage_idx,
                            object_runtime_slot(obj_idx, obj) as i64,
                            obj,
                        );
                    }
                }
            }

            let mut btnsel_stage_ids: Vec<i64> = st.btnselitem_lists.keys().copied().collect();
            btnsel_stage_ids.sort_unstable();
            for stage_idx in btnsel_stage_ids {
                let Some(items) = st.btnselitem_lists.get_mut(&stage_idx) else {
                    continue;
                };
                for item in items {
                    for (obj_idx, obj) in item.object_list.iter_mut().enumerate() {
                        apply_gan_effects_recursive(
                            gfx,
                            images,
                            sprites,
                            &index,
                            stage_idx,
                            object_runtime_slot(obj_idx, obj) as i64,
                            obj,
                        );
                    }
                }
            }
        }
    }

    fn apply_object_disp_override(&mut self) {
        const GET_OBJECT_DISP_ONOFF: i32 = 278;
        let disp_on = self
            .globals
            .syscom
            .config_int
            .get(&GET_OBJECT_DISP_ONOFF)
            .copied()
            .unwrap_or(1)
            != 0;
        if disp_on {
            return;
        }

        let ui_layer = self.ui.mwnd.layer;
        for (stage_idx, list) in self
            .globals
            .stage_forms
            .values()
            .flat_map(|st| st.object_lists.iter())
        {
            for (obj_idx, obj) in list.iter().enumerate() {
                match &obj.backend {
                    globals::ObjectBackend::String { layer_id, .. } => {
                        if Some(*layer_id) == ui_layer {
                            continue;
                        }
                        for (_, sprite_id) in layer_backed_object_sprite_bindings(&obj.backend) {
                            if let Some(spr) = self
                                .layers
                                .layer_mut(*layer_id)
                                .and_then(|layer| layer.sprite_mut(sprite_id))
                            {
                                spr.visible = false;
                            }
                        }
                    }
                    globals::ObjectBackend::Rect {
                        layer_id,
                        sprite_id,
                        ..
                    }
                    | globals::ObjectBackend::Movie {
                        layer_id,
                        sprite_id,
                        ..
                    } => {
                        if Some(*layer_id) == ui_layer {
                            continue;
                        }
                        if let Some(layer) = self.layers.layer_mut(*layer_id) {
                            if let Some(spr) = layer.sprite_mut(*sprite_id) {
                                spr.visible = false;
                            }
                        }
                    }
                    globals::ObjectBackend::Number {
                        layer_id,
                        sprite_ids,
                    }
                    | globals::ObjectBackend::Weather {
                        layer_id,
                        sprite_ids,
                    } => {
                        if Some(*layer_id) == ui_layer {
                            continue;
                        }
                        if let Some(layer) = self.layers.layer_mut(*layer_id) {
                            for sid in sprite_ids {
                                if let Some(spr) = layer.sprite_mut(*sid) {
                                    spr.visible = false;
                                }
                            }
                        }
                    }
                    globals::ObjectBackend::Gfx => {
                        if let Some((lid, sid)) =
                            self.gfx.object_sprite_binding(*stage_idx, obj_idx as i64)
                        {
                            if Some(lid) == ui_layer {
                                continue;
                            }
                            if let Some(layer) = self.layers.layer_mut(lid) {
                                if let Some(spr) = layer.sprite_mut(sid) {
                                    spr.visible = false;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn handle_system_messagebox_key(&mut self, k: input::VmKey) -> bool {
        let Some(modal) = self.globals.system.messagebox_modal.as_mut() else {
            return false;
        };
        if modal.native_pending {
            return true;
        }
        let mut finish_value: Option<i64> = None;
        match k {
            input::VmKey::ArrowLeft | input::VmKey::ArrowUp => {
                let len = modal.buttons.len();
                if len > 0 {
                    modal.cursor = if modal.cursor == 0 {
                        len - 1
                    } else {
                        modal.cursor - 1
                    };
                }
            }
            input::VmKey::ArrowRight | input::VmKey::ArrowDown | input::VmKey::Tab => {
                let len = modal.buttons.len();
                if len > 0 {
                    modal.cursor = (modal.cursor + 1) % len;
                }
            }
            input::VmKey::Enter | input::VmKey::Space => {
                finish_value = Some(modal.selected_value());
            }
            input::VmKey::Escape => {
                finish_value = Some(modal.cancel_value());
            }
            input::VmKey::Digit(d) => {
                let idx = d.saturating_sub(1) as usize;
                if idx < modal.buttons.len() {
                    modal.cursor = idx;
                    finish_value = Some(modal.selected_value());
                }
            }
            _ => {}
        }
        if let Some(value) = finish_value {
            self.finish_system_messagebox(value);
        }
        true
    }

    fn handle_system_messagebox_click(&mut self, b: input::VmMouseButton) -> bool {
        let Some(modal) = self.globals.system.messagebox_modal.as_mut() else {
            return false;
        };
        if modal.native_pending {
            return true;
        }
        match b {
            input::VmMouseButton::Left => {
                let len = modal.buttons.len().max(1);
                // The engine-rendered modal lays buttons out one per line.
                // Match the hit test to that visual layout instead of the old
                // horizontal native-message-box approximation.
                let body_lines = modal.text.lines().count() as i32;
                let first_button_y = 40 + (body_lines + 2) * 30;
                let mut idx = ((self.input.mouse_y - first_button_y).max(0) / 30) as usize;
                if idx >= len {
                    idx = len - 1;
                }
                modal.cursor = idx;
                let value = modal.selected_value();
                self.finish_system_messagebox(value);
            }
            input::VmMouseButton::Right => {
                let value = modal.cancel_value();
                self.finish_system_messagebox(value);
            }
            _ => {}
        }
        true
    }

    fn finish_system_messagebox(&mut self, value: i64) {
        let complete_wait_with_value = self
            .globals
            .system
            .messagebox_modal
            .as_ref()
            .map(|modal| modal.complete_wait_with_value)
            .unwrap_or(false);
        self.globals.system.messagebox_modal = None;
        self.globals.system.messagebox_modal_result = Some(value);
        if complete_wait_with_value {
            self.wait.finish_system_modal(Value::Int(value));
        } else {
            self.wait.finish_system_modal_void();
        }
        self.ui.set_sys_overlay(false, String::new());
    }

    fn sync_system_messagebox_ui(&mut self) -> bool {
        let Some(modal) = self.globals.system.messagebox_modal.as_ref() else {
            return false;
        };
        if modal.native_pending {
            // A host-provided native dialog owns presentation and input.
            self.ui.set_sys_overlay(false, String::new());
            return true;
        }

        let mut text = modal.text.clone();
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push('\n');
        for (index, button) in modal.buttons.iter().enumerate() {
            if index == modal.cursor {
                text.push_str("▶ ");
            } else {
                text.push_str("  ");
            }
            text.push_str(&button.label);
            if index == modal.cursor {
                text.push_str(" ◀");
            }
            text.push('\n');
        }
        text.push_str("\n↑↓/←→: select   Enter: decide   Esc: back");
        self.ui.set_sys_overlay(true, text);
        true
    }

    fn msg_back_state(&self) -> Option<&globals::MsgBackState> {
        let form_id = self.ids.form_global_msgbk;
        if form_id == 0 {
            return None;
        }
        self.globals.msgbk_forms.get(&form_id)
    }

    fn sync_msg_back_history_capacity(&mut self) {
        let max_count = self
            .gameexe_i64_default("MSGBK.HISTORY_CNT", 256)
            .clamp(1, 4096) as usize;
        let form_id = self.ids.form_global_msgbk;
        if form_id == 0 {
            return;
        }
        if let Some(st) = self.globals.msgbk_forms.get_mut(&form_id) {
            st.set_history_cnt_max(max_count);
        }
    }

    fn msg_back_entry_has_content(entry: &globals::MsgBackEntry) -> bool {
        entry.pct_flag
            || !entry.msg_str.is_empty()
            || !entry.disp_name.is_empty()
            || !entry.original_name.is_empty()
            || !entry.koe_no_list.is_empty()
    }

    fn msg_back_visible_entry_indices(&self) -> Vec<usize> {
        self.msg_back_state()
            .map(|st| {
                st.ordered_history_indices()
                    .into_iter()
                    .filter(|&i| st.history.get(i).map_or(false, Self::msg_back_entry_has_content))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn msg_back_is_enable(&self) -> bool {
        self.globals.syscom.msg_back.check_enabled() != 0 && !self.globals.script.msg_back_disable
    }

    fn msg_back_line_step(&self) -> i32 {
        let moji_size = self.gameexe_i64_default("MSGBK.MOJI_SIZE", 24).max(1) as i32;
        let moji_space = self.gameexe_pair_default("MSGBK.MOJI_SPACE", (-1, 10));
        (moji_size + moji_space.1 as i32).max(1)
    }

    fn msg_back_text_area_width(moji_cnt: (i64, i64), moji_size: i32, moji_space: (i64, i64)) -> i32 {
        let cols = moji_cnt.0.max(1) as i32;
        moji_size
            .saturating_mul(cols)
            .saturating_add((moji_space.0 as i32).saturating_mul((cols - 1).max(0)))
            .max(1)
    }

    fn msg_back_text_area_height(moji_cnt: (i64, i64), moji_size: i32, moji_space: (i64, i64)) -> i32 {
        let rows = moji_cnt.1.max(1) as i32;
        moji_size
            .saturating_mul(rows)
            .saturating_add((moji_space.1 as i32).saturating_mul((rows - 1).max(0)))
            .max(1)
    }

    fn msg_back_is_hankaku(ch: char) -> bool {
        ch.is_ascii() || matches!(ch as u32, 0xFF61..=0xFF9F)
    }

    fn msg_back_is_kinsoku_moji(ch: char) -> bool {
        matches!(
            ch,
            'ぁ' | 'ぃ' | 'ぅ' | 'ぇ' | 'ぉ' | 'っ' | 'ゃ' | 'ゅ' | 'ょ' | 'ゎ'
                | 'ァ' | 'ィ' | 'ゥ' | 'ェ' | 'ォ' | 'ッ' | 'ャ' | 'ュ' | 'ョ' | 'ヮ'
                | 'ヵ' | 'ヶ' | 'ﾞ' | 'ﾟ' | '｡' | '､' | '!' | '?' | ':' | ';' | '｣'
                | ')' | ']' | '>' | '}' | '\'' | '"' | 'ｰ' | '･' | '.' | ','
                | 'ｧ' | 'ｨ' | 'ｩ' | 'ｪ' | 'ｫ' | 'ｯ' | 'ｬ' | 'ｭ' | 'ｮ'
        )
    }

    fn msg_back_entry_text(entry: &globals::MsgBackEntry) -> String {
        if entry.pct_flag {
            return String::new();
        }
        let mut out = String::new();
        if !entry.disp_name.is_empty() {
            out.push_str(&entry.disp_name);
            out.push('\u{0007}');
        }
        if !entry.msg_str.is_empty() {
            out.push_str(&entry.msg_str);
            out.push('\u{0007}');
        }
        out
    }

    fn msg_back_measure_entry_text(
        entry: &globals::MsgBackEntry,
        moji_cnt: (i64, i64),
        moji_size: i32,
        moji_space: (i64, i64),
    ) -> (String, i32) {
        let text = Self::msg_back_entry_text(entry);
        if text.is_empty() {
            return (text, moji_size.max(1));
        }

        let msg_w = Self::msg_back_text_area_width(moji_cnt, moji_size, moji_space);
        let msg_h = Self::msg_back_text_area_height(moji_cnt, moji_size, moji_space);
        let space_x = moji_space.0 as i32;
        let space_y = moji_space.1 as i32;
        let line_step = (moji_size + space_y).max(1);
        let mut x = 0i32;
        let mut y = 0i32;
        let mut indent_pos = 0i32;
        let mut indent_moji = '\0';
        let mut indent_cnt = 0i32;
        let mut line_head = true;

        let clear_indent = |indent_pos: &mut i32, indent_moji: &mut char, indent_cnt: &mut i32| {
            *indent_pos = 0;
            *indent_moji = '\0';
            *indent_cnt = 0;
        };
        let new_line_indent = |x: &mut i32, y: &mut i32, indent_pos: i32| {
            *x = indent_pos;
            *y = (*y).saturating_add(line_step);
        };

        for ch in text.chars() {
            if ch == '\r' {
                continue;
            }
            if ch == '\n' {
                new_line_indent(&mut x, &mut y, indent_pos);
                line_head = true;
                continue;
            }
            if ch == '\u{0007}' {
                clear_indent(&mut indent_pos, &mut indent_moji, &mut indent_cnt);
                new_line_indent(&mut x, &mut y, indent_pos);
                line_head = true;
                continue;
            }

            let this_moji_size = if Self::msg_back_is_hankaku(ch) {
                (moji_size / 2).max(1)
            } else {
                moji_size.max(1)
            };
            let this_check_size = this_moji_size.saturating_add(space_x);
            let mut auto_indent = false;
            if x.saturating_add(this_check_size) > msg_w.saturating_add(moji_size) {
                new_line_indent(&mut x, &mut y, indent_pos);
                auto_indent = true;
            } else if x.saturating_add(this_check_size) > msg_w && !Self::msg_back_is_kinsoku_moji(ch) {
                new_line_indent(&mut x, &mut y, indent_pos);
                auto_indent = true;
            }
            if auto_indent && (ch == ' ' || ch == '　') {
                continue;
            }
            if y >= msg_h {
                break;
            }

            x = x.saturating_add(this_moji_size).saturating_add(space_x);

            if ch == '「' || ch == '『' || ch == '（' {
                if line_head {
                    indent_pos = x;
                    indent_moji = ch;
                    indent_cnt = 1;
                } else if ch == indent_moji {
                    indent_cnt += 1;
                }
            }
            if indent_cnt > 0 {
                if (indent_moji == '「' && ch == '」')
                    || (indent_moji == '『' && ch == '』')
                    || (indent_moji == '（' && ch == '）')
                {
                    indent_cnt -= 1;
                    if indent_cnt == 0 {
                        clear_indent(&mut indent_pos, &mut indent_moji, &mut indent_cnt);
                    }
                }
            }
            line_head = false;
        }

        let height = y.saturating_sub(space_y).max(moji_size.max(1));
        (text, height)
    }

    fn msg_back_image_size_by_name(&mut self, file: Option<&str>) -> Option<(i32, i32)> {
        let raw = file.map(str::trim).filter(|s| !s.is_empty())?;
        let id = self
            .images
            .load_g00(raw, 0)
            .or_else(|_| self.images.load_bg_frame(raw, 0))
            .or_else(|_| {
                let path = self.project_dir.join(raw);
                self.images.load_file(&path, 0)
            })
            .ok()?;
        self.images
            .get(id)
            .map(|img| (img.width as i32, img.height as i32))
    }

    fn msg_back_image_size_from_gameexe(&mut self, key: &str) -> Option<(i32, i32)> {
        let file = self.gameexe_string(key);
        self.msg_back_image_size_by_name(file.as_deref())
    }

    fn build_msg_back_layout(&mut self) -> MsgBackLayout {
        self.sync_msg_back_history_capacity();
        let mut out = MsgBackLayout::default();
        let indices = self.msg_back_visible_entry_indices();
        let entries: Vec<(usize, globals::MsgBackEntry)> = {
            let Some(st) = self.msg_back_state() else {
                return out;
            };
            indices
                .into_iter()
                .filter_map(|history_index| {
                    st.history
                        .get(history_index)
                        .cloned()
                        .map(|entry| (history_index, entry))
                })
                .collect()
        };
        if entries.is_empty() {
            return out;
        }

        let moji_cnt = self.gameexe_pair_default("MSGBK.MOJI_CNT", (20, 15));
        let moji_size = self.gameexe_i64_default("MSGBK.MOJI_SIZE", 24).max(1) as i32;
        let moji_space = self.gameexe_pair_default("MSGBK.MOJI_SPACE", (-1, 10));
        let separator_file = self.gameexe_string("MSGBK.SEPARATOR_FILE");
        let separator_top_file = self.gameexe_string("MSGBK.SEPARATOR_TOP_FILE");
        let separator_bottom_file = self.gameexe_string("MSGBK.SEPARATOR_BOTTOM_FILE");
        let separator_height = self
            .msg_back_image_size_by_name(separator_file.as_deref())
            .map(|(_, h)| h.max(0))
            .unwrap_or(0);
        let separator_top_height = self
            .msg_back_image_size_by_name(separator_top_file.as_deref())
            .map(|(_, h)| h.max(0))
            .unwrap_or(0);
        let separator_bottom_height = self
            .msg_back_image_size_by_name(separator_bottom_file.as_deref())
            .map(|(_, h)| h.max(0))
            .unwrap_or(0);

        if separator_top_file.is_some() && separator_top_height > 0 {
            out.separators.push(MsgBackSeparatorLayout {
                file: separator_top_file.clone(),
                total_pos: -separator_top_height,
                height: separator_top_height,
            });
        }

        let mut total_height = 0i32;
        let mut last_margin = 0i32;
        for (visible_pos, (history_index, entry)) in entries.iter().enumerate() {
            if entry.pct_flag {
                let total_pos = total_height;
                let height = self
                    .msg_back_image_size_by_name(Some(entry.msg_str.as_str()))
                    .map(|(_, h)| h.max(1))
                    .unwrap_or_else(|| moji_size.max(1));
                out.entries.push(MsgBackLayoutEntry {
                    history_index: *history_index,
                    text: String::new(),
                    total_pos,
                    height,
                });
                total_height = total_height.saturating_add(height);
                last_margin = 0;
            } else {
                let (text, height) = Self::msg_back_measure_entry_text(entry, moji_cnt, moji_size, moji_space);
                let total_pos = total_height.saturating_add(last_margin);
                out.entries.push(MsgBackLayoutEntry {
                    history_index: *history_index,
                    text,
                    total_pos,
                    height,
                });
                total_height = total_height
                    .saturating_add(last_margin)
                    .saturating_add(height);
                last_margin = moji_size;
            }

            if visible_pos + 1 < entries.len() {
                if separator_file.is_some() && separator_height > 0 {
                    out.separators.push(MsgBackSeparatorLayout {
                        file: separator_file.clone(),
                        total_pos: total_height,
                        height: separator_height,
                    });
                    total_height = total_height.saturating_add(separator_height);
                    last_margin = 0;
                }
            } else if separator_bottom_file.is_some() && separator_bottom_height > 0 {
                out.separators.push(MsgBackSeparatorLayout {
                    file: separator_bottom_file.clone(),
                    total_pos: total_height,
                    height: separator_bottom_height,
                });
                total_height = total_height.saturating_add(separator_bottom_height);
                last_margin = 0;
            }
        }
        out.total_height = total_height.max(0);
        out
    }

    fn msg_back_slider_track(&self) -> (i32, i32, i32) {
        let vals = Self::parse_i64_list(self.gameexe_value("MSGBK_ITEM.SLIDER.POS"));
        if vals.len() >= 3 {
            (vals[0] as i32, vals[1] as i32, vals[2] as i32)
        } else {
            (0, 0, 0)
        }
    }

    fn msg_back_slider_size_i32(&mut self) -> (i32, i32) {
        if let Some((w, h)) = self.msg_back_image_size_from_gameexe("MSGBK_ITEM.SLIDER.FILE") {
            return (w.max(0), h.max(0));
        }
        self.ui
            .msg_back_slider_size()
            .map(|(w, h)| (w as i32, h as i32))
            .unwrap_or((0, 0))
    }

    fn limit_i32(a: i32, v: i32, b: i32) -> i32 {
        let lo = a.min(b);
        let hi = a.max(b);
        v.clamp(lo, hi)
    }

    fn linear_i32(x: i32, x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
        if x1 == x2 {
            return y1;
        }
        let num = (x as i64 - x1 as i64) * (y2 as i64 - y1 as i64);
        (y1 as i64 + num / (x2 as i64 - x1 as i64)) as i32
    }

    fn msg_back_scroll_limits(&self, layout: &MsgBackLayout) -> Option<(i32, i32)> {
        let first = layout.entries.first()?;
        let last = layout.entries.last()?;
        let window_size = self.gameexe_pair_default("MSGBK.WINDOW_SIZE", (780, 580));
        let wind_height = window_size.1.max(1) as i32;
        let msgsp = wind_height / 2 - first.height / 2;
        let mut msgep = wind_height / 2 + last.height / 2 - layout.total_height;
        if layout.entries.len() == 1 {
            msgep = msgsp;
        }
        Some((msgep, msgsp))
    }

    fn msg_back_calc_target_no_from_scroll(&mut self, layout: &MsgBackLayout) {
        if layout.entries.is_empty() {
            self.globals.syscom.msg_back_target_no = -1;
            return;
        }
        let window_size = self.gameexe_pair_default("MSGBK.WINDOW_SIZE", (780, 580));
        let center = (window_size.1.max(1) as i32) / 2;
        let mut target = layout.entries.last().map(|e| e.history_index as isize).unwrap_or(-1);
        for entry in layout.entries.iter().rev() {
            if self.globals.syscom.msg_back_scroll_pos
                .saturating_add(entry.total_pos)
                .saturating_add(entry.height)
                >= center
            {
                target = entry.history_index as isize;
            }
        }
        self.globals.syscom.msg_back_target_no = target;
    }

    fn msg_back_calc_slider_pos_from_scroll(&mut self, layout: &MsgBackLayout) {
        let Some((msgep, msgsp)) = self.msg_back_scroll_limits(layout) else {
            let (_x, top, _bottom) = self.msg_back_slider_track();
            self.globals.syscom.msg_back_scroll_pos = 0;
            self.globals.syscom.msg_back_slider_pos = top;
            return;
        };
        let (_x, top, bottom) = self.msg_back_slider_track();
        let slider_h = self.msg_back_slider_size_i32().1.max(0);
        let slider_end = bottom.saturating_sub(slider_h);
        self.globals.syscom.msg_back_scroll_pos =
            Self::limit_i32(msgep, self.globals.syscom.msg_back_scroll_pos, msgsp);
        self.globals.syscom.msg_back_slider_pos = Self::linear_i32(
            self.globals.syscom.msg_back_scroll_pos,
            msgep,
            slider_end,
            msgsp,
            top,
        );
        self.globals.syscom.msg_back_slider_pos =
            Self::limit_i32(top, self.globals.syscom.msg_back_slider_pos, slider_end);
    }

    fn msg_back_calc_scroll_pos_from_slider(&mut self, layout: &MsgBackLayout) {
        let Some((msgep, msgsp)) = self.msg_back_scroll_limits(layout) else {
            self.globals.syscom.msg_back_scroll_pos = 0;
            return;
        };
        let (_x, top, bottom) = self.msg_back_slider_track();
        let slider_h = self.msg_back_slider_size_i32().1.max(0);
        let slider_end = bottom.saturating_sub(slider_h);
        self.globals.syscom.msg_back_slider_pos =
            Self::limit_i32(top, self.globals.syscom.msg_back_slider_pos, slider_end);
        self.globals.syscom.msg_back_scroll_pos = Self::linear_i32(
            self.globals.syscom.msg_back_slider_pos,
            top,
            msgsp,
            slider_end,
            msgep,
        );
        self.globals.syscom.msg_back_scroll_pos =
            Self::limit_i32(msgep, self.globals.syscom.msg_back_scroll_pos, msgsp);
    }

    fn msg_back_calc_scroll_pos_from_target(&mut self, layout: &MsgBackLayout) {
        if layout.entries.is_empty() {
            self.globals.syscom.msg_back_target_no = -1;
            self.globals.syscom.msg_back_scroll_pos = 0;
            return;
        }
        let target_no = self.globals.syscom.msg_back_target_no;
        let entry = layout
            .entries
            .iter()
            .find(|entry| entry.history_index as isize == target_no)
            .unwrap_or_else(|| layout.entries.last().expect("layout is not empty"));
        self.globals.syscom.msg_back_target_no = entry.history_index as isize;
        let window_size = self.gameexe_pair_default("MSGBK.WINDOW_SIZE", (780, 580));
        let wind_height = window_size.1.max(1) as i32;
        self.globals.syscom.msg_back_scroll_pos =
            wind_height / 2 - (entry.total_pos + entry.height / 2);
    }

    fn msg_back_update_pos_from_scroll(&mut self, layout: &MsgBackLayout) {
        self.msg_back_calc_target_no_from_scroll(layout);
        self.msg_back_calc_slider_pos_from_scroll(layout);
    }

    fn msg_back_update_pos_from_slider(&mut self, layout: &MsgBackLayout) {
        self.msg_back_calc_scroll_pos_from_slider(layout);
        self.msg_back_calc_target_no_from_scroll(layout);
    }

    fn msg_back_update_pos_from_target(&mut self, layout: &MsgBackLayout) {
        self.msg_back_calc_scroll_pos_from_target(layout);
        self.msg_back_calc_slider_pos_from_scroll(layout);
    }

    fn msg_back_target_up(&mut self) {
        let layout = self.build_msg_back_layout();
        if layout.entries.is_empty() {
            return;
        }
        let current = self.globals.syscom.msg_back_target_no;
        let pos = layout
            .entries
            .iter()
            .position(|entry| entry.history_index as isize == current)
            .unwrap_or_else(|| layout.entries.len().saturating_sub(1));
        let next_pos = pos.saturating_sub(1);
        self.globals.syscom.msg_back_target_no = layout.entries[next_pos].history_index as isize;
        self.msg_back_update_pos_from_target(&layout);
    }

    fn msg_back_target_down(&mut self) {
        let layout = self.build_msg_back_layout();
        if layout.entries.is_empty() {
            return;
        }
        let current = self.globals.syscom.msg_back_target_no;
        let pos = layout
            .entries
            .iter()
            .position(|entry| entry.history_index as isize == current)
            .unwrap_or_else(|| layout.entries.len().saturating_sub(1));
        let next_pos = (pos + 1).min(layout.entries.len() - 1);
        self.globals.syscom.msg_back_target_no = layout.entries[next_pos].history_index as isize;
        self.msg_back_update_pos_from_target(&layout);
    }

    fn msg_back_window_contains(&self, x: i32, y: i32) -> bool {
        let window_pos = self.gameexe_pair_default("MSGBK.WINDOW_POS", (10, 10));
        let window_size = self.gameexe_pair_default("MSGBK.WINDOW_SIZE", (780, 580));
        let left = window_pos.0 as i32;
        let top = window_pos.1 as i32;
        let right = left.saturating_add(window_size.0.max(1) as i32);
        let bottom = top.saturating_add(window_size.1.max(1) as i32);
        left <= x && x < right && top <= y && y < bottom
    }

    fn msg_back_initialize_open_state(&mut self, layout: &MsgBackLayout) {
        let (_x, _top, bottom) = self.msg_back_slider_track();
        let slider_h = self.msg_back_slider_size_i32().1.max(0);
        self.globals.syscom.msg_back_msg_total_height = layout.total_height;

        // C_elm_msg_back::open() first places the slider at the bottom, derives
        // scroll/slider from that position, and only then assigns m_target_no to
        // m_history_last_pos. Do not use the last message target to drive the
        // initial scroll position here.
        self.globals.syscom.msg_back_slider_pos = bottom.saturating_sub(slider_h);
        self.msg_back_update_pos_from_slider(layout);
        self.msg_back_update_pos_from_scroll(layout);
        self.globals.syscom.msg_back_target_no = if layout.entries.is_empty() {
            -1
        } else if let Some(st) = self.msg_back_state() {
            if layout
                .entries
                .iter()
                .any(|entry| entry.history_index == st.history_last_pos)
            {
                st.history_last_pos as isize
            } else {
                layout.entries.last().map(|entry| entry.history_index as isize).unwrap_or(-1)
            }
        } else {
            layout.entries.last().map(|entry| entry.history_index as isize).unwrap_or(-1)
        };
        self.globals.syscom.msg_back_slider_dragging = false;
        self.globals.syscom.msg_back_content_dragging = false;
        self.globals.syscom.msg_back_proc_initialized = true;
    }

    fn open_msg_back_proc(&mut self) {
        if !self.msg_back_is_enable() {
            return;
        }
        self.globals.syscom.read_skip.onoff = false;
        self.globals.syscom.msg_back_open = true;
        self.globals.syscom.pending_proc = Some(globals::SyscomPendingProc {
            kind: globals::SyscomPendingProcKind::MsgBack,
            warning: false,
            se_play: false,
            fade_out: false,
            leave_msgbk: false,
            save_id: 0,
        });
        let layout = self.build_msg_back_layout();
        self.msg_back_initialize_open_state(&layout);
    }

    fn close_msg_back_proc(&mut self) {
        self.globals.syscom.msg_back_open = false;
        self.globals.syscom.msg_back_slider_dragging = false;
        self.globals.syscom.msg_back_content_dragging = false;
        self.globals.syscom.msg_back_proc_initialized = false;
        self.ui.set_msg_back_projection(None);
        self.ui.set_sys_overlay(false, String::new());
    }

    fn handle_msg_back_key(&mut self, k: input::VmKey) -> bool {
        if !self.globals.syscom.msg_back_open {
            return false;
        }
        match k {
            input::VmKey::Escape | input::VmKey::Enter | input::VmKey::Space => {
                self.close_msg_back_proc();
            }
            input::VmKey::ArrowUp | input::VmKey::ArrowLeft => self.msg_back_target_up(),
            input::VmKey::ArrowDown | input::VmKey::ArrowRight => self.msg_back_target_down(),
            input::VmKey::F(5) => {
                let layout = self.build_msg_back_layout();
                if let Some(entry) = layout.entries.first() {
                    self.globals.syscom.msg_back_target_no = entry.history_index as isize;
                    self.msg_back_update_pos_from_target(&layout);
                }
            }
            input::VmKey::F(6) => {
                let layout = self.build_msg_back_layout();
                if let Some(entry) = layout.entries.last() {
                    self.globals.syscom.msg_back_target_no = entry.history_index as isize;
                    self.msg_back_update_pos_from_target(&layout);
                }
            }
            _ => {}
        }
        true
    }

    fn handle_msg_back_mouse_down(&mut self, b: input::VmMouseButton) -> bool {
        if !self.globals.syscom.msg_back_open {
            return false;
        }
        match b {
            input::VmMouseButton::Right => {
                self.close_msg_back_proc();
            }
            input::VmMouseButton::Left => {
                match self.ui.msg_back_hit_action(self.input.mouse_x, self.input.mouse_y) {
                    Some(ui::MsgBackHitAction::Close) => self.close_msg_back_proc(),
                    Some(ui::MsgBackHitAction::Up) => self.msg_back_target_up(),
                    Some(ui::MsgBackHitAction::Down) => self.msg_back_target_down(),
                    Some(ui::MsgBackHitAction::Slider) => {
                        self.globals.syscom.msg_back_slider_dragging = true;
                        self.globals.syscom.msg_back_slider_drag_start_mouse = self.input.mouse_y;
                        self.globals.syscom.msg_back_slider_drag_start_pos =
                            self.globals.syscom.msg_back_slider_pos;
                    }
                    None => {
                        if self.msg_back_window_contains(self.input.mouse_x, self.input.mouse_y) {
                            self.globals.syscom.msg_back_content_dragging = true;
                            self.globals.syscom.msg_back_content_drag_start_mouse = self.input.mouse_y;
                            self.globals.syscom.msg_back_content_drag_start_scroll_pos =
                                self.globals.syscom.msg_back_scroll_pos;
                        }
                    }
                }
            }
            _ => {}
        }
        true
    }

    fn handle_msg_back_mouse_up(&mut self, b: input::VmMouseButton) -> bool {
        if !self.globals.syscom.msg_back_open {
            return false;
        }
        if matches!(b, input::VmMouseButton::Left) {
            self.globals.syscom.msg_back_slider_dragging = false;
            self.globals.syscom.msg_back_content_dragging = false;
        }
        true
    }

    fn handle_msg_back_mouse_move(&mut self) -> bool {
        if !self.globals.syscom.msg_back_open {
            return false;
        }
        if self.globals.syscom.msg_back_slider_dragging {
            let layout = self.build_msg_back_layout();
            self.globals.syscom.msg_back_slider_pos = self
                .globals
                .syscom
                .msg_back_slider_drag_start_pos
                .saturating_add(self.input.mouse_y - self.globals.syscom.msg_back_slider_drag_start_mouse);
            self.msg_back_update_pos_from_slider(&layout);
            return true;
        }
        if self.globals.syscom.msg_back_content_dragging {
            let layout = self.build_msg_back_layout();
            self.globals.syscom.msg_back_scroll_pos = self
                .globals
                .syscom
                .msg_back_content_drag_start_scroll_pos
                .saturating_sub(self.globals.syscom.msg_back_content_drag_start_mouse - self.input.mouse_y);
            self.msg_back_update_pos_from_scroll(&layout);
            return true;
        }

        let layout = self.build_msg_back_layout();
        self.globals.syscom.msg_back_mouse_target_no = -1;
        let window_pos = self.gameexe_pair_default("MSGBK.WINDOW_POS", (10, 10));
        let window_size = self.gameexe_pair_default("MSGBK.WINDOW_SIZE", (780, 580));
        let disp_margin = self.gameexe_rect_default("MSGBK.DISP_MARGIN", (20, 20, 20, 20));
        let local_x = self.input.mouse_x.saturating_sub(window_pos.0 as i32);
        let local_y = self.input.mouse_y.saturating_sub(window_pos.1 as i32);
        let in_display_rect = local_x >= disp_margin.0 as i32
            && local_x < window_size.0.max(1) as i32 - disp_margin.2 as i32
            && local_y >= disp_margin.1 as i32
            && local_y < window_size.1.max(1) as i32 - disp_margin.3 as i32;
        if in_display_rect {
            for entry in layout.entries.iter() {
                let top = entry.total_pos.saturating_add(self.globals.syscom.msg_back_scroll_pos);
                let bottom = top.saturating_add(entry.height);
                if top <= local_y && local_y < bottom {
                    self.globals.syscom.msg_back_mouse_target_no = entry.history_index as isize;
                    break;
                }
            }
        }
        false
    }

    fn msg_back_build_visible_text(&self, layout: &MsgBackLayout) -> (String, i32) {
        if layout.entries.is_empty() {
            return (String::new(), self.gameexe_rect_default("MSGBK.DISP_MARGIN", (20, 20, 20, 20)).1 as i32);
        }
        let window_size = self.gameexe_pair_default("MSGBK.WINDOW_SIZE", (780, 580));
        let disp_margin = self.gameexe_rect_default("MSGBK.DISP_MARGIN", (20, 20, 20, 20));
        let clip_top = disp_margin.1 as i32;
        let clip_bottom = window_size.1.max(1) as i32 - disp_margin.3 as i32;
        let scroll = self.globals.syscom.msg_back_scroll_pos;
        let line_step = self.msg_back_line_step();
        let mut first_idx = None;
        let mut last_idx = None;
        for (i, entry) in layout.entries.iter().enumerate() {
            let top = entry.total_pos.saturating_add(scroll);
            let bottom = top.saturating_add(entry.height);
            if bottom > clip_top && top < clip_bottom {
                if first_idx.is_none() {
                    first_idx = Some(i);
                }
                last_idx = Some(i);
            }
        }
        let Some(first) = first_idx else {
            let target_no = self.globals.syscom.msg_back_target_no;
            let entry = layout
                .entries
                .iter()
                .find(|entry| entry.history_index as isize == target_no)
                .unwrap_or_else(|| layout.entries.last().expect("layout is not empty"));
            return (entry.text.clone(), entry.total_pos.saturating_add(scroll));
        };
        let last = last_idx.unwrap_or(first);
        let mut text = String::new();
        for i in first..=last {
            let entry = &layout.entries[i];
            if entry.text.is_empty() {
                continue;
            }
            if !text.is_empty() {
                let prev = &layout.entries[i - 1];
                let gap = entry.total_pos - (prev.total_pos + prev.height);
                let blank_lines = (gap / line_step).max(0) as usize;
                for _ in 0..blank_lines {
                    text.push('\n');
                }
            }
            text.push_str(&entry.text);
            if !text.ends_with('\n') {
                text.push('\n');
            }
        }
        (text, layout.entries[first].total_pos.saturating_add(scroll))
    }

    fn build_msg_back_projection(&mut self) -> Option<ui::MsgBackUiProjection> {
        if !self.globals.syscom.msg_back_open {
            return None;
        }
        let layout = self.build_msg_back_layout();
        self.globals.syscom.msg_back_msg_total_height = layout.total_height;
        if !self.globals.syscom.msg_back_proc_initialized {
            self.msg_back_initialize_open_state(&layout);
        } else {
            self.msg_back_update_pos_from_scroll(&layout);
        }

        let window_pos = self.gameexe_pair_default("MSGBK.WINDOW_POS", (10, 10));
        let window_size = self.gameexe_pair_default("MSGBK.WINDOW_SIZE", (780, 580));
        let disp_margin = self.gameexe_rect_default("MSGBK.DISP_MARGIN", (20, 20, 20, 20));
        let filter_margin = self.gameexe_rect_default("MSGBK.FILTER_MARGIN", (0, 0, 0, 0));
        let filter_rgba = self.gameexe_rgba_default("MSGBK.FILTER_COLOR", (0, 0, 0, 0));
        let filter_config_rgba = self.syscom_filter_config_rgba();
        let moji_space = self.gameexe_pair_default("MSGBK.MOJI_SPACE", (-1, 10));
        let moji_size = self.gameexe_i64_default("MSGBK.MOJI_SIZE", 24).max(1);
        let msg_pos = self.gameexe_i64_default("MSGBK.MESSAGE_POS", 30) as i32;
        let order = self.gameexe_i64_default("MSGBK.ORDER", 10000) as i32;
        let scroll = self.globals.syscom.msg_back_scroll_pos;
        let (dl, dt, dr, db) = disp_margin;
        let clip_top = dt as i32;
        let clip_bottom = window_size.1.max(1) as i32 - db as i32;
        let moji_cnt = self.gameexe_pair_default("MSGBK.MOJI_CNT", (20, 15));
        let text_width = Self::msg_back_text_area_width(moji_cnt, moji_size as i32, moji_space) as u32;
        let font_shadow_mode = self.effective_font_shadow_mode();
        let (font_shadow, font_fuchi) =
            crate::text_render::font_shadow_mode_flags(font_shadow_mode);
        let font_bold = self.effective_font_bold();
        let base_style = TextStyle {
            color: self.gameexe_color(self.tables.mwnd_render.moji_color),
            shadow_color: self.gameexe_color(self.tables.mwnd_render.shadow_color),
            fuchi_color: self.gameexe_color(self.tables.mwnd_render.fuchi_color),
            shadow_mode: font_shadow_mode,
            shadow: font_shadow,
            fuchi: font_fuchi,
            bold: font_bold,
        };
        let active_style = TextStyle {
            color: self.gameexe_color(self.gameexe_i64_default("MSGBK.ACTIVE_MOJI_COLOR", 7)),
            shadow_color: self.gameexe_color(self.gameexe_i64_default("MSGBK.ACTIVE_MOJI_SHADOW_COLOR", 0)),
            fuchi_color: self.gameexe_color(self.gameexe_i64_default("MSGBK.ACTIVE_MOJI_FUCHI_COLOR", 0)),
            shadow_mode: font_shadow_mode,
            shadow: font_shadow,
            fuchi: font_fuchi,
            bold: font_bold,
        };
        let debug_style = TextStyle {
            color: self.gameexe_color(self.gameexe_i64_default("MSGBK.DEBUG_MOJI_COLOR", 5)),
            shadow_color: self.gameexe_color(self.gameexe_i64_default("MSGBK.DEBUG_MOJI_SHADOW_COLOR", 0)),
            fuchi_color: self.gameexe_color(self.gameexe_i64_default("MSGBK.DEBUG_MOJI_FUCHI_COLOR", 0)),
            shadow_mode: font_shadow_mode,
            shadow: font_shadow,
            fuchi: font_fuchi,
            bold: font_bold,
        };

        let koe_btn_file = self.gameexe_string("MSGBK_ITEM.KOE_BTN.FILE");
        let koe_btn_pos = self.msg_back_button_pos("MSGBK_ITEM.KOE_BTN.POS", (-20, -10));
        let load_btn_file = self.gameexe_string("MSGBK_ITEM.LOAD_BTN.FILE");
        let load_btn_pos = self.msg_back_button_pos("MSGBK_ITEM.LOAD_BTN.POS", (-20, 0));

        let mut text_entries = Vec::new();
        let mut koe_buttons = Vec::new();
        let mut load_buttons = Vec::new();
        let separators = layout
            .separators
            .iter()
            .filter_map(|sep| {
                if sep.file.is_none() || sep.height <= 0 {
                    return None;
                }
                let local_y = sep.total_pos.saturating_add(scroll);
                let bottom = local_y.saturating_add(sep.height);
                if bottom > clip_top && local_y < clip_bottom {
                    Some(ui::MsgBackImageProjection {
                        file: sep.file.clone(),
                        x: 0,
                        y: local_y,
                    })
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();

        if let Some(st) = self.msg_back_state() {
            for layout_entry in layout.entries.iter() {
                let Some(entry) = st.history.get(layout_entry.history_index) else {
                    continue;
                };
                let local_y = layout_entry.total_pos.saturating_add(scroll);
                let bottom = local_y.saturating_add(layout_entry.height);
                let is_in_rect = bottom > clip_top && local_y < clip_bottom;
                if is_in_rect && !layout_entry.text.is_empty() {
                    let mut style = base_style;
                    if self.globals.system.debug_flag
                        && self.globals.syscom.msg_back_target_no == layout_entry.history_index as isize
                    {
                        style = debug_style;
                    }
                    if self.globals.syscom.msg_back_mouse_target_no == layout_entry.history_index as isize {
                        style = active_style;
                    }
                    text_entries.push(ui::MsgBackTextProjection {
                        history_index: layout_entry.history_index,
                        text: layout_entry.text.clone(),
                        x: msg_pos,
                        y: local_y,
                        width: text_width,
                        height: layout_entry.height.max(1) as u32,
                        style,
                    });
                }
                if is_in_rect && entry.pct_flag {
                    koe_buttons.push(ui::MsgBackEntryButtonProjection {
                        history_index: layout_entry.history_index,
                        file: Some(entry.msg_str.clone()),
                        x: msg_pos.saturating_add(entry.pct_pos_x),
                        y: local_y.saturating_add(entry.pct_pos_y),
                    });
                } else if is_in_rect && !entry.koe_no_list.is_empty() {
                    koe_buttons.push(ui::MsgBackEntryButtonProjection {
                        history_index: layout_entry.history_index,
                        file: koe_btn_file.clone(),
                        x: msg_pos.saturating_add(koe_btn_pos.0),
                        y: local_y.saturating_add(koe_btn_pos.1),
                    });
                }
                if is_in_rect && entry.save_id_check_flag {
                    load_buttons.push(ui::MsgBackEntryButtonProjection {
                        history_index: layout_entry.history_index,
                        file: load_btn_file.clone(),
                        x: msg_pos.saturating_add(load_btn_pos.0),
                        y: local_y.saturating_add(load_btn_pos.1),
                    });
                }
            }
        }

        let (slider_x, _slider_top, _slider_bottom) = self.msg_back_slider_track();
        if trace_env().sg_msgbk_trace {
            eprintln!(
                "[SG_MSGBK_TRACE][PROJECTION] entries={} separators={} text={} koe={} load={} total_height={} scroll={} slider={} target={} mouse_target={}",
                layout.entries.len(),
                layout.separators.len(),
                text_entries.len(),
                koe_buttons.len(),
                load_buttons.len(),
                layout.total_height,
                self.globals.syscom.msg_back_scroll_pos,
                self.globals.syscom.msg_back_slider_pos,
                self.globals.syscom.msg_back_target_no,
                self.globals.syscom.msg_back_mouse_target_no
            );
            for entry in &layout.entries {
                eprintln!(
                    "[SG_MSGBK_TRACE][LAYOUT] history_index={} total_pos={} height={} has_text={}",
                    entry.history_index,
                    entry.total_pos,
                    entry.height,
                    !entry.text.is_empty()
                );
            }
            for sep in &layout.separators {
                eprintln!(
                    "[SG_MSGBK_TRACE][SEPARATOR] file={:?} total_pos={} height={}",
                    sep.file,
                    sep.total_pos,
                    sep.height
                );
            }
        }
        Some(ui::MsgBackUiProjection {
            window_x: window_pos.0 as i32,
            window_y: window_pos.1 as i32,
            window_w: window_size.0.max(1) as u32,
            window_h: window_size.1.max(1) as u32,
            disp_margin,
            msg_pos,
            moji_size,
            moji_space: Some(moji_space),
            order,
            filter_layer_rep: self.tables.mwnd_render.filter_layer_rep as i32,
            waku_layer_rep: self.tables.mwnd_render.waku_layer_rep as i32,
            moji_layer_rep: self.tables.mwnd_render.moji_layer_rep as i32,
            waku_file: self.gameexe_string("MSGBK.BACK_FILE"),
            filter_file: self.gameexe_string("MSGBK.FILTER_FILE"),
            filter_margin,
            filter_rgba,
            filter_config_rgba,
            text_entries,
            separators,
            koe_buttons,
            load_buttons,
            close_btn_file: self.gameexe_string("MSGBK_ITEM.CLOSE_BTN.FILE"),
            close_btn_pos: self.msg_back_button_pos("MSGBK_ITEM.CLOSE_BTN.POS", (0, 0)),
            msg_up_btn_file: self.gameexe_string("MSGBK_ITEM.MSG_UP_BTN.FILE"),
            msg_up_btn_pos: self.msg_back_button_pos("MSGBK_ITEM.MSG_UP_BTN.POS", (0, 0)),
            msg_down_btn_file: self.gameexe_string("MSGBK_ITEM.MSG_DOWN_BTN.FILE"),
            msg_down_btn_pos: self.msg_back_button_pos("MSGBK_ITEM.MSG_DOWN_BTN.POS", (0, 0)),
            slider_file: self.gameexe_string("MSGBK_ITEM.SLIDER.FILE"),
            slider_rect: (slider_x, self.msg_back_slider_track().1, slider_x, self.msg_back_slider_track().2),
            slider_pos: (slider_x, self.globals.syscom.msg_back_slider_pos),
            ex_btn_files: [
                self.gameexe_string("MSGBK_ITEM.EX_BTN_1.FILE"),
                self.gameexe_string("MSGBK_ITEM.EX_BTN_2.FILE"),
                self.gameexe_string("MSGBK_ITEM.EX_BTN_3.FILE"),
                self.gameexe_string("MSGBK_ITEM.EX_BTN_4.FILE"),
            ],
            ex_btn_pos: [
                self.msg_back_button_pos("MSGBK_ITEM.EX_BTN_1.POS", (0, 0)),
                self.msg_back_button_pos("MSGBK_ITEM.EX_BTN_2.POS", (0, 0)),
                self.msg_back_button_pos("MSGBK_ITEM.EX_BTN_3.POS", (0, 0)),
                self.msg_back_button_pos("MSGBK_ITEM.EX_BTN_4.POS", (0, 0)),
            ],
        })
    }

    fn sync_syscom_menu_ui(&mut self) {
        self.ui.set_msg_back_projection(None);
        self.ui.set_sys_overlay(false, String::new());
        if self.sync_system_messagebox_ui() {
            return;
        }
        if self.globals.syscom.msg_back_open {
            let projection = self.build_msg_back_projection();
            self.ui.set_msg_back_projection(projection);
            return;
        }
        if self.globals.syscom.menu_open {
            log::error!("SYSCOM menu proc is not implemented; fake Rust text menu is disabled");
            self.globals.syscom.menu_open = false;
            self.globals.syscom.menu_kind = None;
            self.globals.syscom.menu_result = None;
        }
    }

    fn selbtn_choice_selectable(choice: &globals::BtnSelectChoiceState) -> bool {
        choice.item_type == TNM_SEL_ITEM_TYPE_ON_I64
    }

    fn next_selbtn_cursor(&self, dir: i32) -> usize {
        let choices = &self.globals.selbtn.choices;
        if choices.is_empty() {
            return 0;
        }
        let len = choices.len() as i32;
        let mut idx = self.globals.selbtn.cursor.min(choices.len() - 1) as i32;
        for _ in 0..choices.len() {
            idx = (idx + dir).rem_euclid(len);
            if Self::selbtn_choice_selectable(&choices[idx as usize]) {
                return idx as usize;
            }
        }
        self.globals.selbtn.cursor.min(choices.len() - 1)
    }

    fn selbtn_linear_limit(now: i64, start: i64, start_value: i64, end: i64, end_value: i64) -> i64 {
        if now >= end {
            return end_value;
        }
        if now <= start || start == end {
            return start_value;
        }
        (end_value - start_value)
            .saturating_mul(now - start)
            / (end - start)
            + start_value
    }

    fn selbtn_speed_up_limit(now: i64, start: i64, start_value: i64, end: i64, end_value: i64) -> i64 {
        if start == end {
            return end_value;
        }
        let current = if start < end {
            now.clamp(start, end)
        } else {
            now.clamp(end, start)
        };
        let t = (current - start) as f64;
        let d = (end - start) as f64;
        (t * t * (end_value - start_value) as f64 / (d * d) + start_value as f64) as i64
    }

    fn selbtn_speed_down_limit(now: i64, start: i64, start_value: i64, end: i64, end_value: i64) -> i64 {
        if start == end {
            return end_value;
        }
        let current = if start < end {
            now.clamp(start, end)
        } else {
            now.clamp(end, start)
        };
        let t = (current - end) as f64;
        let d = (end - start) as f64;
        (-t * t * (end_value - start_value) as f64 / (d * d) + end_value as f64) as i64
    }

    fn selbtn_accepts_input(&self) -> bool {
        let sel = &self.globals.selbtn;
        sel.started
            && sel.appear_flag
            && sel.open_anime_type == 0
            && sel.decide_anime_type == 0
            && sel.close_anime_type == 0
            && !sel.capture_now_flag
    }

    fn deliver_selbtn_result(&mut self) {
        if self.globals.selbtn.result_delivered {
            return;
        }
        let result = self.globals.selbtn.result;
        self.globals.selbtn.result_delivered = true;
        self.stack.push(Value::Int(result));
        self.notify_wait_key();
    }

    fn end_selbtn_close_animation(&mut self) {
        self.globals.selbtn.close_anime_type = 0;
        self.globals.selbtn.started = false;
        self.globals.selbtn.appear_flag = false;
        self.globals.selbtn.processing_flag_0 = false;
        self.globals.selbtn.processing_flag_1 = false;
        self.globals.selbtn.processing_flag_2 = false;
        self.globals.selbtn.pressed_index = None;
        self.globals.selbtn.pressed_inside = false;
        if self.globals.selbtn.capture_flag {
            crate::runtime::forms::syscom::free_runtime_save_thumb_capture(
                self,
                crate::runtime::forms::syscom::CAPTURE_PRIOR_SAVE,
            );
        }
        self.clear_selbtn_items_from_front_stage();
        if self.globals.selbtn.sync_type == 0 {
            self.deliver_selbtn_result();
        }
    }

    fn begin_selbtn_close_animation(&mut self) {
        let template_no = self.globals.selbtn.template_no.max(0) as usize;
        let tmpl = self
            .tables
            .sel_btn_templates
            .get(template_no)
            .cloned()
            .unwrap_or_default();
        let sel = &mut self.globals.selbtn;
        sel.appear_flag = false;
        sel.started = false;
        sel.close_anime_type = tmpl.close_anime_type.max(0);
        sel.close_anime_time = tmpl.close_anime_time.max(0);
        sel.close_anime_cur_time = 0;
        if sel.decide_sel_no >= 0
            && tmpl.decide_anime_type >= 1
            && sel.close_anime_type >= 1
        {
            // C_elm_btn_select::close forces a plain fade after a decide
            // animation so only the selected item remains visible.
            sel.close_anime_type = 1;
        }
        sel.processing_flag_1 = false;
        let end_immediately = sel.close_anime_type == 0;
        let release_now = sel.sync_type == 1;
        if release_now {
            self.deliver_selbtn_result();
        }
        if end_immediately {
            self.end_selbtn_close_animation();
        }
    }

    fn finish_selbtn(&mut self, result: i64) {
        if !self.selbtn_accepts_input() {
            return;
        }
        self.globals.selbtn.result = result;
        self.globals.selbtn.started = false;
        self.globals.selbtn.pressed_index = None;
        self.globals.selbtn.pressed_inside = false;
        self.globals.selbtn.decide_sel_no = result;
        self.request_sel_point_with_result(result);
        self.globals.selbtn.processing_flag_2 = false;
        if result >= 0 {
            if let Some(choice) = self.globals.selbtn.choices.get(result as usize) {
                self.globals.syscom.system_extra_str_value = choice.text.clone();
            }
        } else {
            self.globals.syscom.system_extra_str_value = "（キャンセル）".to_string();
        }

        // C_elm_btn_select::button_event commits its registered read flag
        // immediately after decide/cancel, before any sync point releases the
        // script.  The registration is one-shot and is cleared either way.
        let read_scene_no = self.globals.selbtn.read_flag_scene_no;
        let read_flag_no = self.globals.selbtn.read_flag_flag_no;
        let _ = self.globals.set_read_flag(read_scene_no, read_flag_no);
        self.globals.selbtn.read_flag_scene_no = -1;
        self.globals.selbtn.read_flag_flag_no = -1;

        if self.globals.selbtn.sync_type == 2 {
            self.deliver_selbtn_result();
        }

        let template_no = self.globals.selbtn.template_no.max(0) as usize;
        let tmpl = self
            .tables
            .sel_btn_templates
            .get(template_no)
            .cloned()
            .unwrap_or_default();
        if result >= 0 && tmpl.decide_anime_type >= 1 {
            self.globals.selbtn.decide_anime_type = tmpl.decide_anime_type;
            self.globals.selbtn.decide_anime_time = tmpl.decide_anime_time.max(0);
            self.globals.selbtn.decide_anime_cur_time = 0;
            if self.globals.selbtn.decide_anime_time == 0 {
                self.globals.selbtn.decide_anime_type = 0;
                self.begin_selbtn_close_animation();
            }
        } else {
            self.begin_selbtn_close_animation();
        }
    }

    fn update_selbtn_animation(&mut self, elapsed_ms: i64) {
        let elapsed_ms = elapsed_ms.max(0);
        let active = {
            let sel = &self.globals.selbtn;
            sel.appear_flag || sel.close_anime_type > 0 || sel.decide_anime_type > 0
        };
        if !active {
            return;
        }

        {
            let sel = &mut self.globals.selbtn;
            sel.open_anime_cur_time = sel.open_anime_cur_time.saturating_add(elapsed_ms);
            sel.close_anime_cur_time = sel.close_anime_cur_time.saturating_add(elapsed_ms);
            sel.decide_anime_cur_time = sel.decide_anime_cur_time.saturating_add(elapsed_ms);
        }

        let item_count = self
            .globals
            .stage_forms
            .get(&self.ids.form_global_stage)
            .and_then(|st| st.btnselitem_lists.get(&TNM_STAGE_FRONT_I64))
            .map(Vec::len)
            .unwrap_or(0);
        let template_no = self.globals.selbtn.template_no.max(0) as usize;
        let tmpl = self
            .tables
            .sel_btn_templates
            .get(template_no)
            .cloned()
            .unwrap_or_default();
        let stagger_y_count = if tmpl.max_y_cnt > 0 {
            (tmpl.max_y_cnt as usize).min(item_count)
        } else {
            item_count
        };
        // The C++ end-time calculation uses the actual vertical item count.
        // Keep zero for an empty selection, but use one only as a modulo guard
        // inside the item loop (which is empty in that case).
        let y_count = stagger_y_count.max(1);

        // Capture is a one-frame state in C_elm_btn_select.  Once the capture
        // completes, the optional selection-start farcall is queued through
        // the same VM-owned action path used by object buttons.
        if self.globals.selbtn.capture_now_flag {
            self.globals.selbtn.capture_now_flag = false;
            let scene = self.globals.selbtn.sel_start_call_scn.clone();
            let z_no = self.globals.selbtn.sel_start_call_z_no;
            if !scene.is_empty() && z_no >= 0 {
                self.globals.pending_button_actions.push(globals::PendingButtonAction {
                    kind: globals::PendingButtonActionKind::UserCall {
                        scn_name: scene,
                        cmd_name: String::new(),
                        z_no,
                    },
                });
            }
        }

        let stagger = (stagger_y_count as i64).saturating_mul(50);
        let open_stagger = if matches!(self.globals.selbtn.open_anime_type, 2..=6) {
            stagger
        } else {
            0
        };
        let open_end = self
            .globals
            .selbtn
            .open_anime_time
            .saturating_add(open_stagger);
        let decide_end = self.globals.selbtn.decide_anime_time;

        if self.globals.selbtn.open_anime_type > 0
            && self.globals.selbtn.open_anime_cur_time >= open_end
        {
            self.globals.selbtn.open_anime_type = 0;
            if self.globals.selbtn.capture_flag {
                self.globals.selbtn.capture_now_flag = true;
                crate::runtime::forms::syscom::prepare_runtime_save_thumb_capture_with_priority(
                    self,
                    crate::runtime::forms::syscom::CAPTURE_PRIOR_SAVE,
                );
            }
        }
        let close_stagger = if matches!(self.globals.selbtn.close_anime_type, 2..=6) {
            stagger
        } else {
            0
        };
        let close_end = self
            .globals
            .selbtn
            .close_anime_time
            .saturating_add(close_stagger);
        if self.globals.selbtn.close_anime_type > 0
            && self.globals.selbtn.close_anime_cur_time >= close_end
        {
            self.end_selbtn_close_animation();
            return;
        }
        // The original update order is capture -> open -> close -> decide.
        // In particular, a close animation started by end_decide_anime() must
        // not also be advanced/completed by the close test in the same tick.
        if self.globals.selbtn.decide_anime_type > 0
            && self.globals.selbtn.decide_anime_cur_time >= decide_end
        {
            self.begin_selbtn_close_animation();
            self.globals.selbtn.decide_anime_type = 0;
        }

        let sel = self.globals.selbtn.clone();
        let screen_w = self.screen_w as i64;
        let screen_h = self.screen_h as i64;
        let decided_pos = if sel.decide_sel_no >= 0 {
            self.globals
                .selbtn
                .choices
                .get(sel.decide_sel_no as usize)
                .map(|choice| choice.pos)
        } else {
            None
        };
        if let Some(items) = self
            .globals
            .stage_forms
            .get_mut(&self.ids.form_global_stage)
            .and_then(|st| st.btnselitem_lists.get_mut(&TNM_STAGE_FRONT_I64))
        {
            for (index, item) in items.iter_mut().enumerate() {
                let row = index % y_count;
                let mut tr = 255i64;
                let mut x_rep = 0i64;
                let mut y_rep = 0i64;

                match sel.open_anime_type {
                    1 => {
                        tr = Self::selbtn_linear_limit(
                            sel.open_anime_cur_time,
                            0,
                            0,
                            sel.open_anime_time,
                            255,
                        );
                    }
                    2 => {
                        let time = sel
                            .open_anime_cur_time
                            .saturating_sub(50 * (y_count.saturating_sub(row)) as i64);
                        tr = Self::selbtn_linear_limit(time, 0, 0, sel.open_anime_time, 255);
                        y_rep = Self::selbtn_speed_down_limit(
                            time,
                            0,
                            -screen_h,
                            sel.open_anime_time,
                            0,
                        );
                    }
                    3 => {
                        let time = sel.open_anime_cur_time.saturating_sub(50 * row as i64);
                        tr = Self::selbtn_linear_limit(time, 0, 0, sel.open_anime_time, 255);
                        y_rep = Self::selbtn_speed_down_limit(
                            time,
                            0,
                            screen_h,
                            sel.open_anime_time,
                            0,
                        );
                    }
                    4 => {
                        let time = sel.open_anime_cur_time.saturating_sub(50 * row as i64);
                        tr = Self::selbtn_linear_limit(time, 0, 0, sel.open_anime_time, 255);
                        x_rep = Self::selbtn_speed_down_limit(
                            time,
                            0,
                            -screen_w,
                            sel.open_anime_time,
                            0,
                        );
                    }
                    5 => {
                        let time = sel.open_anime_cur_time.saturating_sub(50 * row as i64);
                        tr = Self::selbtn_linear_limit(time, 0, 0, sel.open_anime_time, 255);
                        x_rep = Self::selbtn_speed_down_limit(
                            time,
                            0,
                            screen_w,
                            sel.open_anime_time,
                            0,
                        );
                    }
                    6 => {
                        let time = sel.open_anime_cur_time.saturating_sub(50 * row as i64);
                        tr = Self::selbtn_linear_limit(time, 0, 0, sel.open_anime_time, 255);
                        let start = if row % 2 == 0 { screen_w } else { -screen_w };
                        x_rep = Self::selbtn_speed_down_limit(
                            time,
                            0,
                            start,
                            sel.open_anime_time,
                            0,
                        );
                    }
                    _ => {}
                }

                if sel.decide_sel_no >= 0 && index as i64 != sel.decide_sel_no {
                    match sel.decide_anime_type {
                        1 => {
                            tr = Self::selbtn_linear_limit(
                                sel.decide_anime_cur_time,
                                0,
                                255,
                                sel.decide_anime_time,
                                0,
                            );
                        }
                        2 => {
                            if let Some((decided_x, decided_y)) = decided_pos {
                                let (my_x, my_y) = item.pos;
                                tr = Self::selbtn_linear_limit(
                                    sel.decide_anime_cur_time,
                                    0,
                                    255,
                                    sel.decide_anime_time,
                                    0,
                                );
                                x_rep = Self::selbtn_speed_down_limit(
                                    sel.decide_anime_cur_time,
                                    0,
                                    0,
                                    sel.decide_anime_time,
                                    decided_x - my_x,
                                );
                                y_rep = Self::selbtn_speed_down_limit(
                                    sel.decide_anime_cur_time,
                                    0,
                                    0,
                                    sel.decide_anime_time,
                                    decided_y - my_y,
                                );
                            }
                        }
                        _ => {}
                    }
                }

                if sel.close_anime_type >= 1
                    && tmpl.decide_anime_type >= 1
                    && sel.decide_sel_no >= 0
                    && index as i64 != sel.decide_sel_no
                {
                    tr = 0;
                } else {
                    match sel.close_anime_type {
                        1 => {
                            tr = Self::selbtn_linear_limit(
                                sel.close_anime_cur_time,
                                0,
                                255,
                                sel.close_anime_time,
                                0,
                            );
                        }
                        2 => {
                            let time = sel.close_anime_cur_time.saturating_sub(50 * row as i64);
                            tr = Self::selbtn_linear_limit(time, 0, 255, sel.close_anime_time, 0);
                            y_rep = Self::selbtn_speed_up_limit(
                                time,
                                0,
                                0,
                                sel.close_anime_time,
                                -screen_h,
                            );
                        }
                        3 => {
                            let time = sel
                                .close_anime_cur_time
                                .saturating_sub(50 * (y_count.saturating_sub(row)) as i64);
                            tr = Self::selbtn_linear_limit(time, 0, 255, sel.close_anime_time, 0);
                            y_rep = Self::selbtn_speed_up_limit(
                                time,
                                0,
                                0,
                                sel.close_anime_time,
                                screen_h,
                            );
                        }
                        4 => {
                            let time = sel.close_anime_cur_time.saturating_sub(50 * row as i64);
                            tr = Self::selbtn_linear_limit(time, 0, 255, sel.close_anime_time, 0);
                            x_rep = Self::selbtn_speed_up_limit(
                                time,
                                0,
                                0,
                                sel.close_anime_time,
                                -screen_w,
                            );
                        }
                        5 => {
                            let time = sel.close_anime_cur_time.saturating_sub(50 * row as i64);
                            tr = Self::selbtn_linear_limit(time, 0, 255, sel.close_anime_time, 0);
                            x_rep = Self::selbtn_speed_up_limit(
                                time,
                                0,
                                0,
                                sel.close_anime_time,
                                screen_w,
                            );
                        }
                        6 => {
                            let time = sel.close_anime_cur_time.saturating_sub(50 * row as i64);
                            tr = Self::selbtn_linear_limit(time, 0, 255, sel.close_anime_time, 0);
                            let end = if row % 2 == 0 { screen_w } else { -screen_w };
                            x_rep = Self::selbtn_speed_up_limit(
                                time,
                                0,
                                0,
                                sel.close_anime_time,
                                end,
                            );
                        }
                        _ => {}
                    }
                }

                item.animation_offset = (x_rep, y_rep);
                item.animation_tr = Some(tr.clamp(0, 255));
            }
        }
    }

    fn sync_selbtn_item_selection(&mut self) {
        if let Some(st) = self.globals.stage_forms.get_mut(&self.ids.form_global_stage) {
            if let Some(items) = st.btnselitem_lists.get_mut(&TNM_STAGE_FRONT_I64) {
                for (idx, item) in items.iter_mut().enumerate() {
                    item.selected = idx == self.globals.selbtn.cursor;
                    let selectable = item.item_type == TNM_SEL_ITEM_TYPE_ON_I64;
                    item.button_state = if item.item_type == TNM_SEL_ITEM_TYPE_READ_I64 {
                        TNM_BTN_STATE_DISABLE
                    } else if self.globals.selbtn.pressed_index == Some(idx)
                        && self.globals.selbtn.pressed_inside
                        && selectable
                    {
                        TNM_BTN_STATE_PUSH
                    } else if item.selected && selectable {
                        TNM_BTN_STATE_HIT
                    } else {
                        TNM_BTN_STATE_NORMAL
                    };
                }
            }
        }
    }

    fn hide_selbtn_object_backing(&mut self, obj: &globals::ObjectState) {
        match &obj.backend {
            globals::ObjectBackend::Gfx => {
                if let Some(slot) = obj.nested_runtime_slot {
                    let _ = self.gfx.object_clear(
                        &mut self.images,
                        &mut self.layers,
                        TNM_STAGE_FRONT_I64,
                        slot as i64,
                    );
                }
            }
            globals::ObjectBackend::None => {}
            backend => {
                for (layer_id, sprite_id) in layer_backed_object_sprite_bindings(backend) {
                    if let Some(sprite) = self
                        .layers
                        .layer_mut(layer_id)
                        .and_then(|layer| layer.sprite_mut(sprite_id))
                    {
                        sprite.visible = false;
                        sprite.image_id = None;
                    }
                }
            }
        }
        for child in &obj.runtime.child_objects {
            self.hide_selbtn_object_backing(child);
        }
    }

    fn clear_selbtn_items_from_front_stage(&mut self) {
        let old_items = self
            .globals
            .stage_forms
            .get(&self.ids.form_global_stage)
            .and_then(|st| st.btnselitem_lists.get(&TNM_STAGE_FRONT_I64))
            .cloned()
            .unwrap_or_default();
        for item in &old_items {
            for obj in item.generated_objects.iter().chain(item.object_list.iter()) {
                self.hide_selbtn_object_backing(obj);
            }
        }
        if let Some(st) = self.globals.stage_forms.get_mut(&self.ids.form_global_stage) {
            st.btnselitem_lists.remove(&TNM_STAGE_FRONT_I64);
        }
    }

    fn handle_selbtn_key(&mut self, k: input::VmKey) -> bool {
        if !self.globals.selbtn.started {
            return false;
        }
        if !self.selbtn_accepts_input() {
            return true;
        }
        match k {
            input::VmKey::ArrowUp => {
                self.globals.selbtn.cursor = self.next_selbtn_cursor(-1);
                self.sync_selbtn_item_selection();
                true
            }
            input::VmKey::ArrowDown => {
                self.globals.selbtn.cursor = self.next_selbtn_cursor(1);
                self.sync_selbtn_item_selection();
                true
            }
            input::VmKey::Enter => {
                let idx = self.globals.selbtn.cursor;
                if self
                    .globals
                    .selbtn
                    .choices
                    .get(idx)
                    .is_some_and(Self::selbtn_choice_selectable)
                {
                    self.finish_selbtn(idx as i64);
                }
                true
            }
            input::VmKey::Escape if self.globals.selbtn.cancel_enable => {
                self.finish_selbtn(-1);
                true
            }
            _ => true,
        }
    }

    fn handle_selbtn_mouse_down(&mut self, b: input::VmMouseButton) -> bool {
        if !self.globals.selbtn.started {
            return false;
        }
        if !self.selbtn_accepts_input() {
            self.input.on_mouse_down(b);
            return true;
        }
        self.input.on_mouse_down(b);
        match b {
            input::VmMouseButton::Left => {
                if let Some(idx) = self.selbtn_hit_index(self.input.mouse_x, self.input.mouse_y) {
                    self.globals.selbtn.cursor = idx;
                    self.globals.selbtn.pressed_index = Some(idx);
                    self.globals.selbtn.pressed_inside = true;
                    self.sync_selbtn_item_selection();
                }
                true
            }
            input::VmMouseButton::Right => true,
            _ => true,
        }
    }

    fn handle_selbtn_mouse_up(&mut self, b: input::VmMouseButton) -> bool {
        let selection_active = self.globals.selbtn.started
            || self.globals.selbtn.pressed_index.is_some()
            || self.globals.selbtn.decide_anime_type > 0
            || self.globals.selbtn.close_anime_type > 0;
        if !selection_active {
            return false;
        }
        match b {
            input::VmMouseButton::Left => {
                let pressed = self.globals.selbtn.pressed_index.take();
                let inside = std::mem::take(&mut self.globals.selbtn.pressed_inside);
                let released = self.selbtn_hit_index(self.input.mouse_x, self.input.mouse_y);
                self.sync_selbtn_item_selection();
                if self.selbtn_accepts_input() && inside && pressed.is_some() && pressed == released {
                    self.finish_selbtn(pressed.unwrap_or(0) as i64);
                }
                true
            }
            input::VmMouseButton::Right => {
                if self.selbtn_accepts_input()
                    && self.globals.selbtn.cancel_enable
                    && self.input.vk_down_up_stock(0x02)
                {
                    self.finish_selbtn(-1);
                }
                true
            }
            _ => true,
        }
    }

    fn selbtn_hit_index(&self, mx: i32, my: i32) -> Option<usize> {
        if !self.selbtn_accepts_input() || self.globals.selbtn.choices.is_empty() {
            return None;
        }
        for (idx, choice) in self.globals.selbtn.choices.iter().enumerate().rev() {
            if !Self::selbtn_choice_selectable(choice) {
                continue;
            }
            let (x, y) = choice.pos;
            let (w, h) = choice.size;
            let x0 = x as i32;
            let y0 = y as i32;
            let x1 = x.saturating_add(w.max(1)) as i32;
            let y1 = y.saturating_add(h.max(1)) as i32;
            if mx >= x0 && mx < x1 && my >= y0 && my < y1 {
                return Some(idx);
            }
        }
        None
    }

    fn handle_mwnd_selection_key(&mut self, k: input::VmKey) -> bool {
        let Some((form_id, stage_idx, mwnd_idx)) = self.globals.focused_stage_mwnd else {
            return false;
        };
        let trace_scene = self.current_scene_name.as_deref().unwrap_or("<none>").to_string();
        let trace_scene_no = self.current_scene_no.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string());
        let trace_line = self.current_line_no;
        let mut clear_focus = false;
        let mut handled = false;
        let mut close_anim: Option<(i64, i64)> = None;
        let mut result_to_push: Option<i64> = None;
        if let Some(st) = self.globals.stage_forms.get_mut(&form_id) {
            if let Some(list) = st.mwnd_lists.get_mut(&stage_idx) {
                if let Some(m) = list.get_mut(mwnd_idx) {
                    let close_time = m.close_anime_time;
                    let close_type = m.close_anime_type;
                    let mut close_after = false;
                    let mut clear_selection = false;
                    if let Some(sel) = m.selection.as_mut() {
                        handled = match k {
                            input::VmKey::ArrowUp => {
                                if !sel.choices.is_empty() {
                                    sel.cursor = if sel.cursor == 0 {
                                        sel.choices.len() - 1
                                    } else {
                                        sel.cursor - 1
                                    };
                                }
                                true
                            }
                            input::VmKey::ArrowDown => {
                                if !sel.choices.is_empty() {
                                    sel.cursor = (sel.cursor + 1) % sel.choices.len();
                                }
                                true
                            }
                            input::VmKey::Enter => {
                                sel.result = (sel.cursor as i64) + 1;
                                result_to_push = Some(sel.result);
                                close_after = sel.close_mwnd;
                                clear_selection = true;
                                clear_focus = true;
                                true
                            }
                            input::VmKey::Escape if sel.cancel_enable => {
                                sel.result = -1;
                                result_to_push = Some(sel.result);
                                close_after = sel.close_mwnd;
                                clear_selection = true;
                                clear_focus = true;
                                true
                            }
                            _ => false,
                        };
                    } else {
                        clear_focus = true;
                    }
                    if clear_selection {
                        m.selection = None;
                    }
                    if close_after {
                        let old_open = m.open;
                        m.open = false;
                        sg_mwnd_state_trace_runtime(&trace_scene, &trace_scene_no, trace_line, "MWND_SELECTION_KEY_CLOSE", stage_idx, mwnd_idx, old_open, m.open, m);
                        close_anim = Some((close_type, close_time));
                    }
                } else {
                    clear_focus = true;
                }
            } else {
                clear_focus = true;
            }
        } else {
            clear_focus = true;
        }
        if clear_focus {
            self.globals.focused_stage_mwnd = None;
        }
        if let Some(v) = result_to_push {
            self.stack.push(Value::Int(v));
        }
        if let Some((ty, ms)) = close_anim {
            self.ui.begin_mwnd_close(ty, ms);
        }
        handled
    }

    fn handle_mwnd_selection_click(&mut self, b: input::VmMouseButton) -> bool {
        let Some((form_id, stage_idx, mwnd_idx)) = self.globals.focused_stage_mwnd else {
            return false;
        };
        let trace_scene = self.current_scene_name.as_deref().unwrap_or("<none>").to_string();
        let trace_scene_no = self.current_scene_no.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string());
        let trace_line = self.current_line_no;
        let mut clear_focus = false;
        let mut handled = false;
        let mut close_anim: Option<(i64, i64)> = None;
        let mut result_to_push: Option<i64> = None;
        if let Some(st) = self.globals.stage_forms.get_mut(&form_id) {
            if let Some(list) = st.mwnd_lists.get_mut(&stage_idx) {
                if let Some(m) = list.get_mut(mwnd_idx) {
                    let close_time = m.close_anime_time;
                    let close_type = m.close_anime_type;
                    let mut close_after = false;
                    let mut clear_selection = false;
                    if let Some(sel) = m.selection.as_mut() {
                        handled = match b {
                            input::VmMouseButton::Left => {
                                sel.result = (sel.cursor as i64) + 1;
                                result_to_push = Some(sel.result);
                                close_after = sel.close_mwnd;
                                clear_selection = true;
                                clear_focus = true;
                                true
                            }
                            input::VmMouseButton::Right if sel.cancel_enable => {
                                sel.result = -1;
                                result_to_push = Some(sel.result);
                                close_after = sel.close_mwnd;
                                clear_selection = true;
                                clear_focus = true;
                                true
                            }
                            _ => false,
                        };
                    } else {
                        clear_focus = true;
                    }
                    if clear_selection {
                        m.selection = None;
                    }
                    if close_after {
                        let old_open = m.open;
                        m.open = false;
                        sg_mwnd_state_trace_runtime(&trace_scene, &trace_scene_no, trace_line, "MWND_SELECTION_MOUSE_CLOSE", stage_idx, mwnd_idx, old_open, m.open, m);
                        close_anim = Some((close_type, close_time));
                    }
                } else {
                    clear_focus = true;
                }
            } else {
                clear_focus = true;
            }
        } else {
            clear_focus = true;
        }
        if clear_focus {
            self.globals.focused_stage_mwnd = None;
        }
        if let Some(v) = result_to_push {
            self.stack.push(Value::Int(v));
        }
        if let Some((ty, ms)) = close_anim {
            self.ui.begin_mwnd_close(ty, ms);
        }
        handled
    }

    fn sync_mwnd_window_ui(&mut self) {
        let focused = self.globals.focused_stage_mwnd;
        let wipe_active = self.globals.wipe.is_some();
        let color_table = &self.tables.color_table;
        let resolve_color = |color_no: i64| -> (u8, u8, u8) {
            if color_no >= 0 {
                color_table
                    .get(color_no as usize)
                    .copied()
                    .unwrap_or((255, 255, 255))
            } else {
                (255, 255, 255)
            }
        };
        let mut projections = Vec::new();
        let use_chara_color = self.globals.syscom.original_config.message_chrcolor_flag;
        let font_shadow_mode = self.effective_font_shadow_mode();
        let font_bold = self.effective_font_bold();
        let (draw_shadow, draw_fuchi) =
            crate::text_render::font_shadow_mode_flags(font_shadow_mode);

        for (form_id, st) in &self.globals.stage_forms {
            for (stage_idx, list) in &st.mwnd_lists {
                if *stage_idx == TNM_STAGE_NEXT_I64 && !wipe_active {
                    continue;
                }
                for (mwnd_idx, m) in list.iter().enumerate() {
                    if !m.initialized_from_gameexe {
                        continue;
                    }
                    let key_icon_template = if m.icon_no >= 0 {
                        self.tables.icon_templates.get(m.icon_no as usize)
                    } else {
                        None
                    };
                    let page_icon_template = if m.page_icon_no >= 0 {
                        self.tables.icon_templates.get(m.page_icon_no as usize)
                    } else {
                        None
                    };
                    let msg_moji_no = m
                        .moji_color
                        .or(if use_chara_color { m.chara_moji_color } else { None })
                        .unwrap_or(m.default_moji_color);
                    let msg_shadow_no = m
                        .shadow_color
                        .or(if use_chara_color { m.chara_shadow_color } else { None })
                        .unwrap_or(m.default_shadow_color);
                    let msg_fuchi_no = m
                        .fuchi_color
                        .or(if use_chara_color { m.chara_fuchi_color } else { None })
                        .unwrap_or(m.default_fuchi_color);
                    let name_moji_no = m
                        .name_moji_color
                        .or(if use_chara_color { m.chara_moji_color } else { None })
                        .unwrap_or(m.default_name_moji_color);
                    let name_shadow_no = m
                        .name_shadow_color
                        .or(if use_chara_color { m.chara_shadow_color } else { None })
                        .unwrap_or(m.default_name_shadow_color);
                    let name_fuchi_no = m
                        .name_fuchi_color
                        .or(if use_chara_color { m.chara_fuchi_color } else { None })
                        .unwrap_or(m.default_name_fuchi_color);

                    let select_emoji = |requested_size: i64| -> Option<(&str, i64)> {
                        let mut best: Option<&crate::runtime::tables::EmojiTemplate> = None;
                        for item in &self.tables.emoji_templates {
                            if item.file_name.is_empty() || item.font_size <= 0 {
                                continue;
                            }
                            best = match best {
                                None => Some(item),
                                Some(cur) if cur.font_size > requested_size => {
                                    if item.font_size < cur.font_size { Some(item) } else { Some(cur) }
                                }
                                Some(cur) if item.font_size <= requested_size && item.font_size > cur.font_size => Some(item),
                                Some(cur) => Some(cur),
                            };
                        }
                        best.map(|item| (item.file_name.as_str(), item.font_size))
                    };

                    let projection = crate::runtime::ui::MwndProjectionState {
                        bg_file: (!m.waku_file.is_empty()).then(|| m.waku_file.clone()),
                        filter_file: (!m.filter_file.is_empty()).then(|| m.filter_file.clone()),
                        filter_margin: m.filter_margin,
                        filter_color: m.filter_color,
                        filter_config_color: m.filter_config_color,
                        filter_config_tr: m.filter_config_tr,
                        face_file: (!m.face_file.is_empty()).then(|| m.face_file.clone()),
                        face_no: m.face_no,
                        rep_pos: m.rep_pos,
                        window_pos: m.window_pos,
                        window_size: m.window_size,
                        message_pos: m.message_pos,
                        message_margin: m.message_margin,
                        window_moji_cnt: m.window_moji_cnt,
                        moji_size: m.moji_size.or(Some(m.default_moji_size.max(1))),
                        moji_space: m.moji_space,
                        mwnd_extend_type: m.mwnd_extend_type,
                        moji_color: m.moji_color,
                        shadow_color: m.shadow_color,
                        fuchi_color: m.fuchi_color,
                        chara_moji_color: m.chara_moji_color,
                        chara_shadow_color: m.chara_shadow_color,
                        chara_fuchi_color: m.chara_fuchi_color,
                        name_moji_color: m.name_moji_color,
                        name_shadow_color: m.name_shadow_color,
                        name_fuchi_color: m.name_fuchi_color,
                        resolved_msg_color: resolve_color(msg_moji_no),
                        resolved_msg_shadow_color: resolve_color(msg_shadow_no),
                        resolved_msg_fuchi_color: (msg_fuchi_no >= 0)
                            .then(|| resolve_color(msg_fuchi_no)),
                        resolved_name_color: resolve_color(name_moji_no),
                        resolved_name_shadow_color: resolve_color(name_shadow_no),
                        resolved_name_fuchi_color: (name_fuchi_no >= 0)
                            .then(|| resolve_color(name_fuchi_no)),
                        font_shadow_mode,
                        font_bold,
                        key_icon_file: key_icon_template.and_then(|t| {
                            (!t.file_name.is_empty()).then(|| t.file_name.clone())
                        }),
                        key_icon_pat_cnt: key_icon_template
                            .map(|t| t.anime_pat_cnt)
                            .unwrap_or(1),
                        key_icon_speed: key_icon_template.map(|t| t.anime_speed).unwrap_or(100),
                        page_icon_file: page_icon_template.and_then(|t| {
                            (!t.file_name.is_empty()).then(|| t.file_name.clone())
                        }),
                        page_icon_pat_cnt: page_icon_template
                            .map(|t| t.anime_pat_cnt)
                            .unwrap_or(1),
                        page_icon_speed: page_icon_template
                            .map(|t| t.anime_speed)
                            .unwrap_or(100),
                        key_icon_appear: m.key_icon_appear,
                        key_icon_mode: m.key_icon_mode,
                        key_icon_pos: m.key_icon_pos,
                        icon_pos_type: m.icon_pos_type,
                        icon_pos_base: m.icon_pos_base,
                        icon_pos: m.icon_pos,
                        slide_enabled: m.slide_msg,
                        slide_time: m.slide_time,
                        vertical_writing: m.vertical_writing,
                        name_text: m.name_text.clone(),
                        name_glyphs: m
                            .name_glyphs
                            .iter()
                            .map(|g| {
                                let emoji = if g.moji_type == 0 {
                                    None
                                } else {
                                    select_emoji(g.size.max(1))
                                };
                                crate::runtime::ui::MwndGlyphProjection {
                                    moji_type: g.moji_type,
                                    code: g.code,
                                    ch: g.ch,
                                    x: g.x.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                                    y: g.y.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                                    size: g.size.max(1).min(i32::MAX as i64) as i32,
                                    color: resolve_color(g.moji_color_no),
                                    shadow_color: resolve_color(g.shadow_color_no),
                                    fuchi_color: resolve_color(g.fuchi_color_no),
                                    shadow_mode: font_shadow_mode,
                                    shadow: draw_shadow,
                                    fuchi: draw_fuchi,
                                    bold: font_bold && g.moji_type == 0,
                                    reveal_index: 0,
                                    ruby: false,
                                    appeared: true,
                                    emoji_file: emoji.map(|(file, _)| file.to_string()),
                                    emoji_font_size: emoji
                                        .map(|(_, size)| size as i32)
                                        .unwrap_or(0),
                                    message_button: None,
                                }
                            })
                            .collect(),
                        msg_text: m
                            .message_pages
                            .iter()
                            .map(|page| page.msg_text.as_str())
                            .chain(std::iter::once(m.msg_text.as_str()))
                            .collect::<String>(),
                        glyphs: m
                            .message_pages
                            .iter()
                            .flat_map(|page| page.glyphs.iter())
                            .chain(m.glyphs.iter())
                            .map(|g| {
                                let mut color_no = g.moji_color_no;
                                let mut shadow_color_no = g.shadow_color_no;
                                let mut fuchi_color_no = g.fuchi_color_no;
                                if let Some(button) = &g.message_button {
                                    let button_state = st
                                        .group_lists
                                        .get(stage_idx)
                                        .and_then(|groups| groups.get(button.group_no.max(0) as usize))
                                        .map(|group| {
                                            if group.pushed_button_no == button.btn_no { 2 }
                                            else if group.hit_button_no == button.btn_no { 1 }
                                            else { 0 }
                                        })
                                        .unwrap_or(0);
                                    if let Some(action) = self
                                        .tables
                                        .message_button_templates
                                        .get(button.action_no.max(0) as usize)
                                    {
                                        let button_color = action.color_no[button_state];
                                        if button_color >= 0 {
                                            color_no = button_color;
                                            // C_elm_mwnd_msg::frame resets both edge colors to
                                            // the global MWND colors whenever a message-button
                                            // state color is applied.
                                            shadow_color_no = self.tables.mwnd_render.shadow_color;
                                            fuchi_color_no = self.tables.mwnd_render.fuchi_color;
                                        }
                                    }
                                }
                                let emoji = if g.moji_type == 0 {
                                    None
                                } else {
                                    select_emoji(g.size.max(1))
                                };
                                crate::runtime::ui::MwndGlyphProjection {
                                    moji_type: g.moji_type,
                                    code: g.code,
                                    ch: g.ch,
                                    x: g.x.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                                    y: g.y.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                                    size: g.size.max(1).min(i32::MAX as i64) as i32,
                                    color: resolve_color(color_no),
                                    shadow_color: resolve_color(shadow_color_no),
                                    fuchi_color: resolve_color(fuchi_color_no),
                                    shadow_mode: font_shadow_mode,
                                    shadow: draw_shadow,
                                    fuchi: draw_fuchi,
                                    bold: font_bold && g.moji_type == 0,
                                    reveal_index: g.reveal_index,
                                    ruby: g.ruby,
                                    appeared: g.appeared,
                                    emoji_file: emoji.map(|(file, _)| file.to_string()),
                                    emoji_font_size: emoji.map(|(_, size)| size as i32).unwrap_or(0),
                                    message_button: g.message_button.as_ref().map(|button| {
                                        crate::runtime::ui::MwndMessageButtonProjection {
                                            btn_no: button.btn_no,
                                            group_no: button.group_no,
                                            action_no: button.action_no,
                                            se_no: button.se_no,
                                        }
                                    }),
                                }
                            })
                            .collect(),
                        open: m.open,
                        open_anime_type: m.open_anime_type,
                        open_anime_time: m.open_anime_time,
                        close_anime_type: m.close_anime_type,
                        close_anime_time: m.close_anime_time,
                        order: m.order,
                        layer: m.layer,
                    };
                    projections.push(((*form_id, *stage_idx, mwnd_idx), projection));
                }
            }
        }

        self.ui.apply_mwnd_projection_set(projections, focused);
    }

    fn sync_mwnd_selection_ui(&mut self) {
        if self.globals.system.messagebox_modal.is_some() {
            return;
        }
        self.ui.set_sys_overlay(false, String::new());
    }

    fn sync_emote_objects(&mut self) {
        let koe_playing = self.koe.is_playing_any();
        let live_mouth = if koe_playing {
            self.koe.current_mouth_volume()
        } else {
            0.0
        };
        let koe_chara_no = self.globals.sound_routing.koe_chara_no;
        let koe_ex = self.globals.sound_routing.koe_ex_flag;
        let mouth_stop = self.globals.script.emote_mouth_stop_flag;

        let layers = &mut self.layers;
        for stage in self.globals.stage_forms.values_mut() {
            for list in stage.object_lists.values_mut() {
                for obj in list {
                    sync_emote_object_recursive(
                        layers, obj, mouth_stop, koe_playing, koe_ex, koe_chara_no, live_mouth,
                    );
                }
            }
            for items in stage.btnselitem_lists.values_mut() {
                for item in items {
                    for obj in &mut item.object_list {
                        sync_emote_object_recursive(
                            layers, obj, mouth_stop, koe_playing, koe_ex, koe_chara_no, live_mouth,
                        );
                    }
                }
            }
            for mwnds in stage.mwnd_lists.values_mut() {
                for mwnd in mwnds {
                    for list in [&mut mwnd.object_list, &mut mwnd.button_list, &mut mwnd.face_list] {
                        for obj in list {
                            sync_emote_object_recursive(
                                layers, obj, mouth_stop, koe_playing, koe_ex, koe_chara_no, live_mouth,
                            );
                        }
                    }
                }
            }
        }
    }

    fn sync_movie_objects(&mut self) {
        let wipe_active = self.globals.wipe.is_some();
        let (globals, layers, movie_mgr, audio, gfx, images, ids) = (
            &mut self.globals,
            &mut self.layers,
            &mut self.movie,
            &mut self.audio,
            &mut self.gfx,
            &mut self.images,
            &self.ids,
        );
        let mut decoded_any = false;
        let mut form_ids: Vec<u32> = globals.stage_forms.keys().copied().collect();
        form_ids.sort_unstable();
        for form_id in form_ids {
            let Some(st) = globals.stage_forms.get_mut(&form_id) else {
                continue;
            };
            let mut stage_ids: Vec<i64> = st.object_lists.keys().copied().collect();
            stage_ids.sort_unstable();
            for stage_idx in stage_ids {
                if stage_idx == TNM_STAGE_NEXT_I64 && !wipe_active {
                    continue;
                }
                let Some(objs) = st.object_lists.get_mut(&stage_idx) else {
                    continue;
                };
                for (obj_idx, obj) in objs.iter_mut().enumerate() {
                    sync_movie_object_recursive(
                        ids,
                        layers,
                        movie_mgr,
                        audio,
                        gfx,
                        images,
                        stage_idx,
                        object_runtime_slot(obj_idx, obj) as i64,
                        obj,
                        &mut decoded_any,
                    );
                }
            }
        }
        let _ = decoded_any;
    }

    fn close_global_movie_runtime(&mut self) {
        let was_active = self.globals.mov.playing
            || self.globals.mov.file_name.is_some()
            || self.globals.mov.audio_id.is_some()
            || self.globals.mov.image_id.is_some();

        if let Some(id) = self.globals.mov.audio_id.take() {
            self.movie.stop_audio(id);
        }
        if was_active {
            self.movie.stop();
        }
        if let (Some(layer_id), Some(sprite_id)) =
            (self.globals.mov.layer_id, self.globals.mov.sprite_id)
        {
            if let Some(sprite) = self
                .layers
                .layer_mut(layer_id)
                .and_then(|l| l.sprite_mut(sprite_id))
            {
                sprite.visible = false;
                sprite.image_id = None;
            }
        }
        self.globals.mov.image_id = None;
        self.globals.mov.last_frame_idx = None;

        if was_active {
            self.globals.mov.stop();
        }
    }

    fn sync_global_movie(&mut self) {
        let trace = trace_env().sg_movie_trace;
        let file_name = self.globals.mov.file_name.clone();

        if !self.globals.mov.playing || file_name.as_deref().unwrap_or("").is_empty() {
            // Native Siglus closes C_elm_mov when a MOV wait naturally finishes or is skipped.
            // Keep that lifecycle here so the movie window, image, and movie audio track do not
            // survive past the wait procedure.
            self.close_global_movie_runtime();
            return;
        }
        let file_name = file_name.expect("checked global movie file name");

        if let Some(id) = self.globals.mov.audio_id {
            if let Some(position_ms) = self.movie.audio_playback_position_ms(id) {
                self.globals.mov.timer_ms = position_ms;
            }
            if self.movie.audio_playback_finished(id) {
                self.globals.mov.audio_id = None;
                self.globals.mov.audio_start_attempted = false;
                if let Some(total_ms) = self.globals.mov.total_ms {
                    self.globals.mov.timer_ms = total_ms;
                }
                self.globals.mov.playing = false;
                return;
            }
        }

        let (x, y, width, height, timer_ms, last_frame_idx, image_id, need_audio) = {
            let m = &self.globals.mov;
            (
                m.x,
                m.y,
                m.width.max(1),
                m.height.max(1),
                m.timer_ms,
                m.last_frame_idx,
                m.image_id,
                m.audio_id.is_none() && !m.audio_start_attempted,
            )
        };

        let (layer_id, sprite_id) = match (self.globals.mov.layer_id, self.globals.mov.sprite_id) {
            (Some(layer_id), Some(sprite_id))
                if self
                    .layers
                    .layer(layer_id)
                    .and_then(|l| l.sprite(sprite_id))
                    .is_some() =>
            {
                (layer_id, sprite_id)
            }
            _ => {
                let layer_id = self.layers.create_layer();
                let sprite_id = self
                    .layers
                    .layer_mut(layer_id)
                    .expect("newly created global movie layer")
                    .create_sprite();
                self.globals.mov.layer_id = Some(layer_id);
                self.globals.mov.sprite_id = Some(sprite_id);
                (layer_id, sprite_id)
            }
        };

        let polled = match self.movie.poll_global_movie_frame(&file_name, timer_ms) {
            Ok(Some(frame)) => frame,
            Ok(None) => {
                // Native Siglus starts MOV playback without blocking the UI thread.
                // Keep only the movie timer at the start until the first frame exists.
                // Do not reset the global frame clock here, because that throttles all
                // counters, frame actions, and object events while the decoder warms up.
                if last_frame_idx.is_none() {
                    self.globals.mov.timer_ms = 0;
                }
                return;
            }
            Err(err) => {
                eprintln!("[SG_MOV] error file={} err={:#}", file_name, err);
                self.globals.mov.playing = false;
                return;
            }
        };

        if let Some(ms) = polled.clamped_timer_ms {
            self.globals.mov.timer_ms = ms;
        }
        if self.globals.mov.total_ms.is_none() || polled.total_ms.is_some() {
            self.globals.mov.total_ms = polled.total_ms.or(self.globals.mov.total_ms);
        }
        if let Some(total) = self.globals.mov.total_ms {
            if total > 0 && self.globals.mov.timer_ms >= total {
                self.globals.mov.timer_ms = total;
                self.globals.mov.playing = false;
            }
        }
        let waiting_for_movie_audio_start =
            need_audio && polled.audio.is_none() && !polled.audio_ready;
        let _ = polled.decoded_now;

        let frame = polled.frame.clone();
        let frame_idx = polled.frame_idx;

        if need_audio {
            if let Some(track) = polled.audio.as_ref() {
                match self
                    .movie
                    .start_audio(&mut self.audio, track, self.globals.mov.timer_ms, false)
                {
                    Ok(id) => {
                        self.globals.mov.audio_id = Some(id);
                        self.globals.mov.audio_start_attempted = false;
                        if trace || sg_debug_enabled() {
                            eprintln!(
                                "[SG_DEBUG][MOV] audio_start file={} samples={} channels={} rate={} offset_ms={}",
                                file_name,
                                track.samples.len(),
                                track.channels,
                                track.sample_rate,
                                self.globals.mov.timer_ms
                            );
                        }
                    }
                    Err(err) => {
                        eprintln!(
                            "[SG_MOV] audio_start.failed file={} channels={} rate={} samples={} err={:#}",
                            file_name,
                            track.channels,
                            track.sample_rate,
                            track.samples.len(),
                            err
                        );
                    }
                }
            } else if polled.audio_ready {
                self.globals.mov.audio_start_attempted = true;
                if trace || sg_debug_enabled() {
                    eprintln!("[SG_DEBUG][MOV] audio_track.missing file={}", file_name);
                }
            }
        }

        let img_id = if image_id.is_some() && last_frame_idx != Some(frame_idx) {
            let id = image_id.unwrap();
            let _ = self.images.replace_image_arc(id, frame.clone());
            id
        } else if let Some(id) = image_id {
            id
        } else {
            self.images.insert_image_arc(frame.clone())
        };
        self.globals.mov.image_id = Some(img_id);
        self.globals.mov.last_frame_idx = Some(frame_idx);

        if let Some(sprite) = self
            .layers
            .layer_mut(layer_id)
            .and_then(|l| l.sprite_mut(sprite_id))
        {
            sprite.visible = true;
            sprite.image_id = Some(img_id);
            sprite.fit = SpriteFit::PixelRect;
            sprite.size_mode = SpriteSizeMode::Explicit { width, height };
            sprite.x = x;
            sprite.y = y;
            sprite.alpha = 255;
            sprite.tr = 255;
            sprite.alpha_blend = true;
            sprite.order = i32::MAX - 16;
        }

        if waiting_for_movie_audio_start && self.globals.mov.audio_id.is_none() {
            self.globals.mov.timer_ms = 0;
        }

        if trace {
            eprintln!(
                "[SG_MOVIE_TRACE] global MOV frame file={} idx={} timer={} pos=({}, {}) size={}x{} layer={} sprite={}",
                file_name, frame_idx, self.globals.mov.timer_ms, x, y, width, height, layer_id, sprite_id
            );
        }
    }

    fn sync_weather_objects(&mut self, game_delta_ms: i32, real_delta_ms: i32) {
        let wipe_active = self.globals.wipe.is_some();
        let screen_w = self.screen_w.max(1) as i64;
        let screen_h = self.screen_h.max(1) as i64;
        let (globals, layers, images, ids) = (
            &mut self.globals,
            &mut self.layers,
            &mut self.images,
            &self.ids,
        );
        let mut form_ids: Vec<u32> = globals.stage_forms.keys().copied().collect();
        form_ids.sort_unstable();
        for form_id in form_ids {
            let Some(st) = globals.stage_forms.get_mut(&form_id) else {
                continue;
            };
            let mut stage_ids: Vec<i64> = st.object_lists.keys().copied().collect();
            stage_ids.sort_unstable();
            for stage_idx in stage_ids {
                if stage_idx == TNM_STAGE_NEXT_I64 && !wipe_active {
                    continue;
                }
                let Some(objs) = st.object_lists.get_mut(&stage_idx) else {
                    continue;
                };
                for obj in objs.iter_mut() {
                    sync_weather_object_recursive(
                        ids,
                        layers,
                        images,
                        screen_w,
                        screen_h,
                        game_delta_ms,
                        real_delta_ms,
                        obj,
                    );
                }
            }
        }
    }

    fn repair_missing_gfx_leaf_images(&mut self) {
        fn collect(
            ids: &crate::runtime::constants::RuntimeConstants,
            stage_idx: i64,
            objs: &[globals::ObjectState],
            out: &mut Vec<(i64, usize, String, i64)>,
        ) {
            for (idx, obj) in objs.iter().enumerate() {
                if obj.used && matches!(obj.backend, globals::ObjectBackend::Gfx) {
                    let slot = object_runtime_slot(idx, obj);
                    let file = obj.file_name.clone();
                    if let Some(file) = file {
                        if !file.is_empty() {
                            let patno = obj.lookup_int_prop(ids, ids.obj_patno).unwrap_or(0);
                            out.push((stage_idx, slot, file, patno));
                        }
                    }
                }
                if !obj.runtime.child_objects.is_empty() {
                    collect(ids, stage_idx, &obj.runtime.child_objects, out);
                }
            }
        }

        let mut tasks: Vec<(i64, usize, String, i64)> = Vec::new();
        let mut form_ids: Vec<u32> = self.globals.stage_forms.keys().copied().collect();
        form_ids.sort_unstable();
        for form_id in form_ids {
            let Some(st) = self.globals.stage_forms.get(&form_id) else {
                continue;
            };
            let mut stage_ids: Vec<i64> = st
                .object_lists
                .keys()
                .chain(st.mwnd_lists.keys())
                .copied()
                .collect();
            stage_ids.sort_unstable();
            stage_ids.dedup();
            for stage_idx in stage_ids {
                if let Some(objs) = st.object_lists.get(&stage_idx) {
                    collect(&self.ids, stage_idx, objs, &mut tasks);
                }
                if let Some(mwnds) = st.mwnd_lists.get(&stage_idx) {
                    for mwnd in mwnds {
                        collect(&self.ids, stage_idx, &mwnd.button_list, &mut tasks);
                        collect(&self.ids, stage_idx, &mwnd.face_list, &mut tasks);
                        collect(&self.ids, stage_idx, &mwnd.object_list, &mut tasks);
                    }
                }
            }
        }

        for (stage_idx, runtime_slot, state_file, state_patno) in tasks {
            let Some((layer_id, sprite_id)) = self
                .gfx
                .object_sprite_binding(stage_idx, runtime_slot as i64)
            else {
                continue;
            };
            let needs_image = self
                .layers
                .layer(layer_id)
                .and_then(|layer| layer.sprite(sprite_id))
                .map(|sprite| sprite.image_id.is_none())
                .unwrap_or(false);
            if !needs_image {
                continue;
            }

            let file = self
                .gfx
                .object_peek_file(stage_idx, runtime_slot as i64)
                .unwrap_or_else(|| state_file.clone());
            if file.is_empty() {
                continue;
            }
            let patno = self
                .gfx
                .object_peek_patno(stage_idx, runtime_slot as i64)
                .unwrap_or(state_patno)
                .max(0) as u32;

            let repair_key = (
                self.globals.append_dir.clone(),
                stage_idx,
                runtime_slot,
                file.clone(),
                patno,
            );
            if self.failed_gfx_image_repairs.contains(&repair_key) {
                continue;
            }

            let img_id = match self.images.load_g00(&file, patno) {
                Ok(id) => Ok(id),
                Err(_) => self.images.load_bg_frame(&file, patno as usize),
            };
            match img_id {
                Ok(img_id) => {
                    if let Some(layer) = self.layers.layer_mut(layer_id) {
                        if let Some(sprite) = layer.sprite_mut(sprite_id) {
                            sprite.image_id = Some(img_id);
                            if let Some(img) = self.images.get(img_id) {
                                sprite.object_anchor = true;
                                sprite.texture_center_x = img.center_x as f32;
                                sprite.texture_center_y = img.center_y as f32;
                            } else {
                                sprite.object_anchor = false;
                                sprite.texture_center_x = 0.0;
                                sprite.texture_center_y = 0.0;
                            }
                        }
                    }
                }
                Err(err) => {
                    self.failed_gfx_image_repairs.insert(repair_key);
                    self.unknown.record_note(&format!(
                        "gfx.image.repair.failed:stage={stage_idx}:slot={runtime_slot}:file={file}:patno={patno}:{err}"
                    ));
                }
            }
        }
    }

    /// Build a render list and apply screen/wipe effects.
    ///
    /// Original Siglus does not render from a flat layer list. It first builds a
    /// stage/object sprite tree and then flattens that tree. We mirror that shape here:
    /// use the existing layer-backed sprites only as leaf payloads, but rebuild the final
    /// submission order from stage -> top-level object -> child objects.
    fn build_render_list_pre_wipe(&mut self) -> (Vec<RenderSprite>, Vec<String>) {
        self.layers.reset_runtime_effects();
        self.repair_missing_gfx_leaf_images();
        self.apply_object_masks();
        self.apply_object_tonecurves();
        let base = self.layers.render_list();
        let (mut list, debug_lines) =
            build_siglus_object_render_list(self, &base, TNM_STAGE_FRONT_I64);
        apply_button_visuals(self, &mut list);
        apply_selbtn_item_visuals(self, &mut list);
        self.apply_gan_effects(&mut list);
        apply_stage_render_effects(
            &self.globals,
            &self.ids,
            TNM_STAGE_FRONT_I64,
            &mut list,
        );
        (list, debug_lines)
    }

    pub fn set_frame_capture_backend(&mut self, backend: Option<FrameCaptureBackendRef>) {
        self.frame_capture_backend = backend;
    }

    pub fn render_frame_with_effects(&mut self) -> RenderFrame {
        self.render_frame_with_effects_inner(true)
    }

    /// Compatibility/debug flattening. Actual presentation uses `RenderFrame`
    /// and never rasterizes a stage on the CPU.
    pub fn render_list_with_effects(&mut self) -> Vec<RenderSprite> {
        self.render_frame_with_effects().debug_flatten()
    }

    fn render_frame_with_effects_inner(&mut self, include_mouse_cursor: bool) -> RenderFrame {
        let (pre_wipe_list, debug_lines) = self.build_render_list_pre_wipe();
        let frame = if let Some(wipe_state) = self.globals.wipe.as_ref().cloned() {
            let (wipe_begin_order, wipe_end_order) =
                effective_wipe_render_order_bounds(self, &wipe_state);
            let base = self.layers.render_list();
            let (mut next_list, next_debug_lines) =
                build_siglus_object_render_list(self, &base, TNM_STAGE_NEXT_I64);
            apply_button_visuals(self, &mut next_list);
            apply_selbtn_item_visuals(self, &mut next_list);
            self.apply_gan_effects(&mut next_list);
            apply_stage_render_effects(
                &self.globals,
                &self.ids,
                TNM_STAGE_NEXT_I64,
                &mut next_list,
            );
            if config_button_trace_enabled() {
                eprintln!(
                    "[SG_DEBUG][CONFIG_BUTTON_TRACE][RENDER_PHASE] wipe_active=true pre_wipe_len={} next_len={} next_debug_lines={} wipe_type={}",
                    pre_wipe_list.len(),
                    next_list.len(),
                    next_debug_lines.len(),
                    wipe_state.wipe_type,
                );
                for line in next_debug_lines
                    .iter()
                    .filter(|line| line.contains("CONFIG_BUTTON_TRACE"))
                {
                    eprintln!("{}", line);
                }
            }

            let with_low = wipe_state.with_low_order != 0;
            let mut under = Vec::new();
            let mut current = Vec::new();
            let mut over = Vec::new();
            for rs in pre_wipe_list.iter().cloned() {
                match classify_wipe_partition(
                    &rs,
                    wipe_state.begin_layer,
                    wipe_state.end_layer,
                    wipe_begin_order,
                    wipe_end_order,
                    with_low,
                ) {
                    WipePartition::Under => under.push(rs),
                    WipePartition::Target => current.push(rs),
                    WipePartition::Over => over.push(rs),
                }
            }
            let mut next = next_list
                .into_iter()
                .filter(|rs| {
                    classify_wipe_partition(
                        rs,
                        wipe_state.begin_layer,
                        wipe_state.end_layer,
                        wipe_begin_order,
                        wipe_end_order,
                        with_low,
                    ) == WipePartition::Target
                })
                .collect::<Vec<_>>();

            under.retain(render_sprite_visible_for_submit);
            current.retain(render_sprite_visible_for_submit);
            next.retain(render_sprite_visible_for_submit);
            over.retain(render_sprite_visible_for_submit);

            // Siglus types 0/1/2 do not process isolated target sprites over a
            // separately rendered `under` texture. The original engine draws
            // complete scenes for these three basic modes:
            //
            //   type 0: base   = under + NEXT, wipe buffer = under + FRONT
            //   type 1: result = under + FRONT
            //   type 2: result = under + NEXT
            //
            // Keeping `under` separate here would render FRONT/NEXT target
            // sprites against an opaque black offscreen clear and then place
            // that full-screen black texture over the actual lower orders.
            // Promote both target lists to complete scene inputs instead.
            compose_basic_wipe_scene_inputs(
                wipe_state.wipe_type,
                &mut under,
                &mut current,
                &mut next,
            );
            if include_mouse_cursor {
                self.append_mouse_cursor_sprite(&mut over);
            }

            let progress =
                wipe::eased_progress(wipe_state.progress(), wipe_state.speed_mode);
            RenderFrame {
                sprites: Vec::new(),
                wipe: Some(WipeRenderPlan {
                    under,
                    current,
                    next,
                    over,
                    wipe_type: wipe_state.wipe_type,
                    option: wipe_state.option,
                    progress,
                    mask_image_id: wipe_state.mask_image_id,
                    random_seed: wipe_state.random_seed,
                }),
            }
        } else {
            let mut list = pre_wipe_list.clone();
            list.retain(render_sprite_visible_for_submit);
            if include_mouse_cursor {
                self.append_mouse_cursor_sprite(&mut list);
            }
            self.last_presented_render_list = pre_wipe_list.clone();
            RenderFrame::ordinary(list)
        };

        let debug_list = frame.debug_flatten();
        if config_button_trace_enabled() {
            trace_final_render_order(self, &debug_list);
        }
        if save_load_render_trace_enabled() {
            trace_save_load_render_sprites(self, &debug_list);
        }
        if sg_render_tree_debug_enabled() {
            use std::sync::atomic::{AtomicU64, Ordering};
            static FRAME_NO: AtomicU64 = AtomicU64::new(0);
            let frame_no = FRAME_NO.fetch_add(1, Ordering::Relaxed) + 1;
            eprintln!("[SG_DEBUG] ===== frame {} =====", frame_no);
            for line in debug_lines {
                eprintln!("{}", line);
            }
            if let Some(wipe) = self.globals.wipe.as_ref() {
                eprintln!(
                    "[SG_DEBUG] wipe active type={} progress={:.3} range=({},{})->({},{}) with_low={} wait={}",
                    wipe.wipe_type,
                    wipe.progress(),
                    wipe.begin_order,
                    wipe.begin_layer,
                    wipe.end_order,
                    wipe.end_layer,
                    wipe.with_low_order,
                    wipe.wait_flag,
                );
            }
            eprintln!(
                "[SG_DEBUG] submitted_render_list len={}",
                frame.submitted_sprite_count()
            );
        }
        frame
    }

    pub fn debug_active_texture_entries(
        &self,
        submitted: &[RenderSprite],
    ) -> Vec<DebugActiveTextureEntry> {
        let mut submitted_keys: HashSet<(LayerId, SpriteId)> = HashSet::new();
        let mut submitted_images: HashSet<ImageId> = HashSet::new();
        for rs in submitted {
            if let Some(id) = rs.sprite.image_id {
                submitted_images.insert(id);
            }
            if let (Some(layer_id), Some(sprite_id)) = (rs.layer_id, rs.sprite_id) {
                submitted_keys.insert((layer_id, sprite_id));
            }
        }

        let mut acc: HashMap<ImageId, DebugActiveTextureAccum> = HashMap::new();
        let mut form_ids: Vec<u32> = self.globals.stage_forms.keys().copied().collect();
        form_ids.sort_unstable();
        for form_id in form_ids {
            let Some(st) = self.globals.stage_forms.get(&form_id) else {
                continue;
            };
            let mut stage_ids: Vec<i64> = st.object_lists.keys().copied().collect();
            stage_ids.sort_unstable();
            for stage_idx in stage_ids {
                let Some(list) = st.object_lists.get(&stage_idx) else {
                    continue;
                };
                for (obj_idx, obj) in list.iter().enumerate() {
                    collect_debug_active_textures_from_object(
                        self,
                        form_id,
                        stage_idx,
                        obj_idx,
                        obj,
                        &submitted_keys,
                        &submitted_images,
                        &mut acc,
                    );
                }
            }
        }

        let mut out: Vec<DebugActiveTextureEntry> = acc
            .into_iter()
            .map(|(image_id, entry)| DebugActiveTextureEntry {
                image_id,
                width: entry.width,
                height: entry.height,
                source_label: entry.source_label,
                submitted_this_frame: entry.submitted_this_frame,
                visible_refs: entry.visible_refs,
                total_refs: entry.total_refs,
                ref_summary: if entry.ref_labels.is_empty() {
                    String::new()
                } else {
                    entry.ref_labels.join(" | ")
                },
            })
            .collect();
        out.sort_by(|a, b| {
            b.submitted_this_frame
                .cmp(&a.submitted_this_frame)
                .then_with(|| b.visible_refs.cmp(&a.visible_refs))
                .then_with(|| b.total_refs.cmp(&a.total_refs))
                .then_with(|| a.image_id.0.cmp(&b.image_id.0))
        });
        out
    }

    /// Capture the current frame through the same wgpu render graph as presentation.
    pub fn capture_frame_rgba(&mut self) -> Result<RgbaImage> {
        let backend = self
            .frame_capture_backend
            .clone()
            .ok_or_else(|| anyhow!("GPU frame capture requested without an attached renderer"))?;
        let frame = self.render_frame_with_effects_inner(false);
        let result = {
            let mut backend = backend.borrow_mut();
            backend.capture_render_frame(
                &self.images,
                &frame,
                self.screen_w,
                self.screen_h,
            )
        };
        result
    }

    /// Capture only sprites up to the original engine order/layer cut line.
    pub fn capture_frame_rgba_until(
        &mut self,
        end_order: i64,
        end_layer: i64,
    ) -> Result<RgbaImage> {
        let backend = self
            .frame_capture_backend
            .clone()
            .ok_or_else(|| anyhow!("GPU frame capture requested without an attached renderer"))?;
        let mut frame = self.render_frame_with_effects_inner(false);
        let within = |rs: &RenderSprite| {
            i64::from(rs.sorter_order) < end_order
                || (i64::from(rs.sorter_order) == end_order
                    && i64::from(rs.sorter_layer) <= end_layer)
        };
        frame.sprites.retain(within);
        if let Some(wipe) = frame.wipe.as_mut() {
            wipe.under.retain(within);
            wipe.current.retain(within);
            wipe.next.retain(within);
            wipe.over.retain(within);
        }
        let result = {
            let mut backend = backend.borrow_mut();
            backend.capture_render_frame(
                &self.images,
                &frame,
                self.screen_w,
                self.screen_h,
            )
        };
        result
    }
}

fn collect_debug_active_textures_from_object(
    ctx: &CommandContext,
    stage_form_id: u32,
    stage_idx: i64,
    obj_idx: usize,
    obj: &globals::ObjectState,
    submitted_keys: &HashSet<(LayerId, SpriteId)>,
    submitted_images: &HashSet<ImageId>,
    out: &mut HashMap<ImageId, DebugActiveTextureAccum>,
) {
    if !object_participates_in_tree(obj) {
        return;
    }

    let info = effective_object_info(ctx, stage_idx, obj_idx, obj);
    let bound = fetch_bound_render_sprites_any(ctx, stage_idx, info.runtime_slot, obj);
    for rs in bound {
        let Some(image_id) = rs.sprite.image_id else {
            continue;
        };
        let submitted = submitted_images.contains(&image_id)
            || rs
                .layer_id
                .zip(rs.sprite_id)
                .map(|key| submitted_keys.contains(&key))
                .unwrap_or(false);
        let debug_img = ctx.images.debug_image_info(image_id);
        let entry = out
            .entry(image_id)
            .or_insert_with(|| DebugActiveTextureAccum {
                width: debug_img.as_ref().map(|d| d.width).unwrap_or(0),
                height: debug_img.as_ref().map(|d| d.height).unwrap_or(0),
                source_label: debug_img
                    .as_ref()
                    .and_then(|d| {
                        d.source_path.as_ref().map(|p| {
                            if let Some(frame_index) = d.frame_index {
                                format!("{}#{}", p.display(), frame_index)
                            } else {
                                p.display().to_string()
                            }
                        })
                    })
                    .unwrap_or_else(|| {
                        obj.file_name
                            .clone()
                            .unwrap_or_else(|| "<dynamic>".to_string())
                    }),
                submitted_this_frame: false,
                visible_refs: 0,
                total_refs: 0,
                ref_labels: Vec::new(),
            });
        entry.submitted_this_frame |= submitted;
        entry.total_refs += 1;
        if info.disp {
            entry.visible_refs += 1;
        }
        let file = obj.file_name.as_deref().unwrap_or("-");
        let ref_label = format!(
            "sf{} st{} slot{} {} disp={} backend={}",
            stage_form_id,
            stage_idx,
            info.runtime_slot,
            file,
            if info.disp { 1 } else { 0 },
            debug_object_backend_name(obj)
        );
        if !entry.ref_labels.iter().any(|s| s == &ref_label) {
            if entry.ref_labels.len() < 3 {
                entry.ref_labels.push(ref_label);
            } else if entry.ref_labels.len() == 3 {
                entry.ref_labels.push("...".to_string());
            }
        }
    }

    for (child_idx, child) in obj.runtime.child_objects.iter().enumerate() {
        collect_debug_active_textures_from_object(
            ctx,
            stage_form_id,
            stage_idx,
            child_idx,
            child,
            submitted_keys,
            submitted_images,
            out,
        );
    }
}

fn debug_object_backend_name(obj: &globals::ObjectState) -> &'static str {
    match &obj.backend {
        globals::ObjectBackend::None => "None",
        globals::ObjectBackend::Gfx => "Gfx",
        globals::ObjectBackend::Rect { .. } => "Rect",
        globals::ObjectBackend::String { .. } => "String",
        globals::ObjectBackend::Number { .. } => "Number",
        globals::ObjectBackend::Weather { .. } => "Weather",
        globals::ObjectBackend::Movie { .. } => "Movie",
    }
}

fn sg_debug_enabled() -> bool {
    trace_env().sg_debug_value
}

fn sg_input_trace_enabled() -> bool {
    trace_env().sg_input_trace
}

fn sg_mwnd_object_trace_enabled() -> bool {
    trace_env().sg_mwnd_object_trace
}

fn sg_render_tree_debug_enabled() -> bool {
    trace_env().sg_render_tree_debug
}

fn config_button_trace_enabled() -> bool {
    trace_env().sg_config_button_trace
}

fn config_button_trace_object(obj: &globals::ObjectState) -> bool {
    if obj.button.enabled || obj.button.state == TNM_BTN_STATE_DISABLE {
        return true;
    }
    let Some(file) = obj.file_name.as_deref() else {
        return false;
    };
    let f = file.to_ascii_lowercase();
    f.starts_with("mn_")
        || f.contains("config")
        || f.contains("conf")
        || f.contains("sys")
        || f.contains("mw")
}

fn config_tr_write_trace_file(file: Option<&str>) -> bool {
    let Some(name) = file else {
        return false;
    };
    name.starts_with("mn_sm_menu_cbox")
        || name.starts_with("mn_cfa_tab_pbtn")
        || name.starts_with("mn_cfb_")
        || name.starts_with("mn_cfe_")
        || name.starts_with("mn_tt_menu")
        || name.starts_with("mn_tt_copy")
}

fn config_tr_write_trace_object(obj_i64: i64, obj: &globals::ObjectState) -> bool {
    (100057..=100067).contains(&obj_i64) || config_tr_write_trace_file(obj.file_name.as_deref())
}

fn trace_config_event_frame_prop_write(
    ids: &constants::RuntimeConstants,
    stage_i64: i64,
    obj_i64: i64,
    obj: &globals::ObjectState,
    prop_id: i32,
    old_value: i64,
    new_value: i64,
) {
    if !sg_debug_enabled() || !config_tr_write_trace_object(obj_i64, obj) {
        return;
    }
    let prop = if ids.obj_tr != 0 && prop_id == ids.obj_tr {
        "TR"
    } else if ids.obj_alpha != 0 && prop_id == ids.obj_alpha {
        "ALPHA"
    } else {
        return;
    };
    eprintln!(
        "[SG_DEBUG][CONFIG_TR_WRITE_TRACE][EVENT_FRAME] stage={} runtime_slot={} file={} prop={} old={} new={} disp={} tr={} alpha={} backend={:?} used={} children={}",
        stage_i64,
        obj_i64,
        obj.file_name.as_deref().unwrap_or("-"),
        prop,
        old_value,
        new_value,
        obj.get_int_prop(ids, ids.obj_disp),
        obj.get_int_prop(ids, ids.obj_tr),
        obj.get_int_prop(ids, ids.obj_alpha),
        obj.backend,
        obj.used,
        obj.runtime.child_objects.len(),
    );
}

fn save_load_render_trace_enabled() -> bool {
    trace_env().saveload_trace
}

fn trace_save_load_render_sprites(ctx: &CommandContext, list: &[RenderSprite]) {
    let scene = ctx.current_scene_name.as_deref().unwrap_or("<none>");
    let scene_match = scene.contains("sys10_sv") || scene.contains("save") || scene.contains("load");
    let mut emitted = 0usize;
    for (idx, rs) in list.iter().enumerate() {
        let Some(image_id) = rs.sprite.image_id else {
            continue;
        };
        let info = ctx.images.debug_image_info(image_id);
        let width = info.as_ref().map(|d| d.width).unwrap_or(0);
        let height = info.as_ref().map(|d| d.height).unwrap_or(0);
        let source_path = info
            .as_ref()
            .and_then(|d| d.source_path.as_ref())
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "-".to_string());
        let source_path_lc = source_path.to_ascii_lowercase();
        let source = render_sprite_source_name(ctx, rs);
        let source_lc = source.to_ascii_lowercase();
        let near_origin = rs.sprite.x.abs() <= 4 && rs.sprite.y.abs() <= 4 && width >= 16 && height >= 16;
        let unowned = source.starts_with("unowned:");
        let path_match = source_path_lc.contains("savedata")
            || source_path_lc.contains("thumb")
            || source_path_lc.contains("capture")
            || source_path_lc.contains("mn_sv")
            || source_lc.contains("mn_sv")
            || source_lc.contains("save")
            || source_lc.contains("thumb")
            || source_lc.contains("capture");
        if !(scene_match || near_origin || unowned || path_match) {
            continue;
        }
        if !(near_origin || unowned || path_match) {
            continue;
        }
        eprintln!(
            "[SG_SAVELOAD_TRACE][RENDER] idx={} scene={} source={} layer_id={:?} sprite_id={:?} image={:?} image_size={}x{} image_version={} image_source={} frame={:?} pos=({}, {}) visible={} alpha={} tr={} order=({}, {}) packed_order={} fit={:?} size_mode={:?} clip={:?}",
            idx,
            scene,
            source,
            rs.layer_id,
            rs.sprite_id,
            rs.sprite.image_id,
            width,
            height,
            info.as_ref().map(|d| d.version).unwrap_or(0),
            source_path,
            info.as_ref().and_then(|d| d.frame_index),
            rs.sprite.x,
            rs.sprite.y,
            rs.sprite.visible,
            rs.sprite.alpha,
            rs.sprite.tr,
            rs.sorter_order,
            rs.sorter_layer,
            rs.sprite.order,
            rs.sprite.fit,
            rs.sprite.size_mode,
            rs.sprite.dst_clip
        );
        emitted += 1;
        if emitted >= 120 {
            eprintln!("[SG_SAVELOAD_TRACE][RENDER] truncated after {} entries", emitted);
            break;
        }
    }
}

fn trace_final_render_order(ctx: &CommandContext, list: &[RenderSprite]) {
    eprintln!(
        "[SG_DEBUG][CONFIG_BUTTON_TRACE][FINAL_ORDER] len={} wipe_active={} selected_stage=front",
        list.len(),
        ctx.globals.wipe.is_some()
    );
    for (idx, rs) in list.iter().enumerate() {
        let source = render_sprite_source_name(ctx, rs);
        eprintln!(
            "[SG_DEBUG][CONFIG_BUTTON_TRACE][FINAL_ORDER] idx={} source={} layer_id={:?} sprite_id={:?} sorter=({}, {}) packed_order={} visible={} alpha={} tr={} pos=({}, {}) z={} fit={:?} size={:?} image={:?} blend={:?} clip={:?}",
            idx,
            source,
            rs.layer_id,
            rs.sprite_id,
            rs.sorter_order,
            rs.sorter_layer,
            rs.sprite.order,
            rs.sprite.visible,
            rs.sprite.alpha,
            rs.sprite.tr,
            rs.sprite.x,
            rs.sprite.y,
            rs.sprite.z,
            rs.sprite.fit,
            rs.sprite.size_mode,
            rs.sprite.image_id,
            rs.sprite.blend,
            rs.sprite.dst_clip
        );
    }
}

fn render_sprite_source_name(ctx: &CommandContext, rs: &RenderSprite) -> String {
    let Some(layer_id) = rs.layer_id else {
        return "background".to_string();
    };
    let Some(sprite_id) = rs.sprite_id else {
        return "background".to_string();
    };
    let mut found: Vec<String> = Vec::new();
    let mut form_ids: Vec<u32> = ctx.globals.stage_forms.keys().copied().collect();
    form_ids.sort_unstable();
    for form_id in form_ids {
        let Some(st) = ctx.globals.stage_forms.get(&form_id) else {
            continue;
        };
        let mut stage_ids: Vec<i64> = st
            .object_lists
            .keys()
            .chain(st.mwnd_lists.keys())
            .chain(st.btnselitem_lists.keys())
            .copied()
            .collect();
        stage_ids.sort_unstable();
        stage_ids.dedup();
        for stage_idx in stage_ids {
            if let Some(list) = st.object_lists.get(&stage_idx) {
                for (obj_idx, obj) in list.iter().enumerate() {
                    collect_render_sprite_source_for_object(
                        ctx,
                        form_id,
                        stage_idx,
                        obj_idx,
                        obj,
                        layer_id,
                        sprite_id,
                        "object",
                        &mut found,
                    );
                }
            }
            if let Some(mwnds) = st.mwnd_lists.get(&stage_idx) {
                for (mwnd_idx, m) in mwnds.iter().enumerate() {
                    for (obj_idx, obj) in m.button_list.iter().enumerate() {
                        collect_render_sprite_source_for_object(
                            ctx,
                            form_id,
                            stage_idx,
                            obj_idx,
                            obj,
                            layer_id,
                            sprite_id,
                            &format!("mwnd{mwnd_idx}.button"),
                            &mut found,
                        );
                    }
                    for (obj_idx, obj) in m.face_list.iter().enumerate() {
                        collect_render_sprite_source_for_object(
                            ctx,
                            form_id,
                            stage_idx,
                            obj_idx,
                            obj,
                            layer_id,
                            sprite_id,
                            &format!("mwnd{mwnd_idx}.face"),
                            &mut found,
                        );
                    }
                    for (obj_idx, obj) in m.object_list.iter().enumerate() {
                        collect_render_sprite_source_for_object(
                            ctx,
                            form_id,
                            stage_idx,
                            obj_idx,
                            obj,
                            layer_id,
                            sprite_id,
                            &format!("mwnd{mwnd_idx}.object"),
                            &mut found,
                        );
                    }
                }
            }
        }
    }
    if found.is_empty() {
        format!("unowned:{layer_id}/{sprite_id}")
    } else {
        found.join("|")
    }
}

fn collect_render_sprite_source_for_object(
    ctx: &CommandContext,
    form_id: u32,
    stage_idx: i64,
    obj_idx: usize,
    obj: &globals::ObjectState,
    layer_id: LayerId,
    sprite_id: SpriteId,
    source_kind: &str,
    found: &mut Vec<String>,
) {
    let file = obj.file_name.as_deref().unwrap_or("-");
    if object_backend_owns_sprite(ctx, stage_idx, obj_idx, obj, layer_id, sprite_id) {
        found.push(format!(
            "form{form_id}:stage{stage_idx}:{source_kind}[{obj_idx}]:slot{}:file{}",
            effective_object_slot_for_trace(obj_idx, obj),
            file
        ));
    }
    for (child_idx, child) in obj.runtime.child_objects.iter().enumerate() {
        collect_render_sprite_source_for_object(
            ctx,
            form_id,
            stage_idx,
            child_idx,
            child,
            layer_id,
            sprite_id,
            &format!("{source_kind}[{obj_idx}].child"),
            found,
        );
    }
}

fn effective_object_slot_for_trace(obj_idx: usize, obj: &globals::ObjectState) -> i64 {
    obj.runtime_slot_or(obj_idx) as i64
}

fn layer_backed_object_sprite_bindings(
    backend: &globals::ObjectBackend,
) -> Vec<(LayerId, SpriteId)> {
    match backend {
        globals::ObjectBackend::Rect {
            layer_id,
            sprite_id,
            ..
        }
        | globals::ObjectBackend::Movie {
            layer_id,
            sprite_id,
            ..
        } => vec![(*layer_id, *sprite_id)],
        globals::ObjectBackend::String {
            layer_id,
            shadow_sprite_id,
            fuchi_sprite_id,
            sprite_id,
            glyphs,
            ..
        } => {
            if glyphs.is_empty() {
                vec![
                    (*layer_id, *shadow_sprite_id),
                    (*layer_id, *fuchi_sprite_id),
                    (*layer_id, *sprite_id),
                ]
            } else {
                let mut bindings = Vec::with_capacity(glyphs.len() * 3);
                bindings.extend(
                    glyphs
                        .iter()
                        .map(|glyph| (*layer_id, glyph.shadow_sprite_id)),
                );
                bindings.extend(
                    glyphs
                        .iter()
                        .map(|glyph| (*layer_id, glyph.fuchi_sprite_id)),
                );
                bindings.extend(
                    glyphs
                        .iter()
                        .map(|glyph| (*layer_id, glyph.body_sprite_id)),
                );
                bindings
            }
        }
        globals::ObjectBackend::Number {
            layer_id,
            sprite_ids,
        }
        | globals::ObjectBackend::Weather {
            layer_id,
            sprite_ids,
        } => sprite_ids.iter().map(|sid| (*layer_id, *sid)).collect(),
        globals::ObjectBackend::Gfx | globals::ObjectBackend::None => Vec::new(),
    }
}

fn object_backend_sprite_layer_offset(
    ctx: &CommandContext,
    backend: &globals::ObjectBackend,
    sprite_id: Option<SpriteId>,
) -> i64 {
    let Some(sprite_id) = sprite_id else {
        return 0;
    };
    let globals::ObjectBackend::String {
        shadow_sprite_id,
        fuchi_sprite_id,
        sprite_id: body_sprite_id,
        glyphs,
        mwnd_layer_reps: true,
        ..
    } = backend
    else {
        return 0;
    };
    if glyphs.iter().any(|glyph| glyph.shadow_sprite_id == sprite_id)
        || (glyphs.is_empty() && sprite_id == *shadow_sprite_id)
    {
        ctx.tables.mwnd_render.shadow_layer_rep
    } else if glyphs.iter().any(|glyph| glyph.fuchi_sprite_id == sprite_id)
        || (glyphs.is_empty() && sprite_id == *fuchi_sprite_id)
    {
        ctx.tables.mwnd_render.fuchi_layer_rep
    } else if glyphs.iter().any(|glyph| glyph.body_sprite_id == sprite_id)
        || (glyphs.is_empty() && sprite_id == *body_sprite_id)
    {
        ctx.tables.mwnd_render.moji_layer_rep
    } else {
        0
    }
}

fn object_backend_sprite_local_offset(
    backend: &globals::ObjectBackend,
    sprite_id: Option<SpriteId>,
) -> (i64, i64) {
    let Some(sprite_id) = sprite_id else {
        return (0, 0);
    };
    let globals::ObjectBackend::String { glyphs, .. } = backend else {
        return (0, 0);
    };
    for glyph in glyphs {
        if glyph.shadow_sprite_id == sprite_id {
            return (glyph.shadow_local_x as i64, glyph.shadow_local_y as i64);
        }
        if glyph.fuchi_sprite_id == sprite_id {
            return (glyph.fuchi_local_x as i64, glyph.fuchi_local_y as i64);
        }
        if glyph.body_sprite_id == sprite_id {
            return (glyph.body_local_x as i64, glyph.body_local_y as i64);
        }
    }
    (0, 0)
}

fn object_backend_owns_sprite(
    ctx: &CommandContext,
    stage_idx: i64,
    obj_idx: usize,
    obj: &globals::ObjectState,
    layer_id: LayerId,
    sprite_id: SpriteId,
) -> bool {
    match &obj.backend {
        globals::ObjectBackend::Gfx => ctx
            .gfx
            .object_sprite_binding(stage_idx, effective_object_slot_for_trace(obj_idx, obj))
            == Some((layer_id, sprite_id)),
        globals::ObjectBackend::None => false,
        backend => layer_backed_object_sprite_bindings(backend)
            .into_iter()
            .any(|binding| binding == (layer_id, sprite_id)),
    }
}

#[derive(Debug, Clone, Default)]
struct ObjectRenderInfo {
    runtime_slot: usize,
    used: bool,
    object_type: i64,
    disp: bool,
    x: i64,
    y: i64,
    x_rep: i64,
    y_rep: i64,
    z_rep: i64,
    order: i64,
    layer: i64,
    alpha: i64,
    tr: i64,
    tr_rep: i64,
    mono: i64,
    reverse: i64,
    bright: i64,
    dark: i64,
    color_rate: i64,
    color_add_r: i64,
    color_add_g: i64,
    color_add_b: i64,
    color_r: i64,
    color_g: i64,
    color_b: i64,
    z: i64,
    world_no: i64,
    center_x: i64,
    center_y: i64,
    center_z: i64,
    center_rep_x: i64,
    center_rep_y: i64,
    center_rep_z: i64,
    scale_x: i64,
    scale_y: i64,
    scale_z: i64,
    rotate_x: i64,
    rotate_y: i64,
    rotate_z: i64,
    culling: bool,
    alpha_test: bool,
    alpha_blend: bool,
    fog_use: bool,
    light_no: i64,
    blend: crate::layer::SpriteBlend,
    child_sort_type: i64,
    dst_clip: Option<ClipRect>,
    billboard: bool,
    file_name: Option<String>,
    mesh_animation: crate::mesh3d::MeshAnimationState,
}

#[derive(Debug, Clone, Copy)]
struct ParentRenderState {
    world_no: i64,
    pos_x: f32,
    pos_y: f32,
    pos_z: f32,
    center_rep_x: f32,
    center_rep_y: f32,
    center_rep_z: f32,
    scale_x: f32,
    scale_y: f32,
    scale_z: f32,
    rotate_x: f32,
    rotate_y: f32,
    rotate_z: f32,
    tr: i32,
    mono: i32,
    reverse: i32,
    bright: i32,
    dark: i32,
    color_rate: i32,
    color_r: i32,
    color_g: i32,
    color_b: i32,
    color_add_r: i32,
    color_add_g: i32,
    color_add_b: i32,
    blend: crate::layer::SpriteBlend,
    dst_clip: Option<ClipRect>,
    mask_image_id: Option<ImageId>,
    mask_offset_x: i32,
    mask_offset_y: i32,
    tonecurve_image_id: Option<ImageId>,
    tonecurve_row: f32,
    tonecurve_sat: f32,
}

fn object_runtime_slot(obj_idx: usize, obj: &globals::ObjectState) -> usize {
    obj.runtime_slot_or(obj_idx)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ButtonSeEvent {
    Hit,
    Push,
    Decide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ButtonSortKey {
    order: i64,
    layer: i64,
}

impl ButtonSortKey {
    fn display_tuple(self) -> String {
        format!("({}, {})", self.order, self.layer)
    }
}

#[derive(Debug, Clone)]
struct ButtonVisualState {
    state: i64,
    action_no: i64,
    file_name: Option<String>,
    base_patno: i64,
    cut_no: i64,
}

const TNM_BTN_STATE_NORMAL: i64 = 0;
const TNM_BTN_STATE_HIT: i64 = 1;
const TNM_BTN_STATE_PUSH: i64 = 2;
const TNM_BTN_STATE_SELECT: i64 = 3;
const TNM_BTN_STATE_DISABLE: i64 = 4;

const TNM_SYSCOM_TYPE_NONE: i64 = 0;
const TNM_SYSCOM_TYPE_SAVE: i64 = 1;
const TNM_SYSCOM_TYPE_LOAD: i64 = 2;
const TNM_SYSCOM_TYPE_READ_SKIP: i64 = 3;
const TNM_SYSCOM_TYPE_AUTO_MODE: i64 = 4;
const TNM_SYSCOM_TYPE_RETURN_SEL: i64 = 5;
const TNM_SYSCOM_TYPE_HIDE_MWND: i64 = 6;
const TNM_SYSCOM_TYPE_MSG_BACK: i64 = 7;
const TNM_SYSCOM_TYPE_KOE_PLAY: i64 = 8;
const TNM_SYSCOM_TYPE_QUICK_SAVE: i64 = 9;
const TNM_SYSCOM_TYPE_QUICK_LOAD: i64 = 10;
const TNM_SYSCOM_TYPE_CONFIG: i64 = 11;
const TNM_SYSCOM_TYPE_LOCAL_EXTRA_SWITCH: i64 = 12;
const TNM_SYSCOM_TYPE_LOCAL_EXTRA_MODE: i64 = 13;
const TNM_SYSCOM_TYPE_GLOBAL_EXTRA_SWITCH: i64 = 14;
const TNM_SYSCOM_TYPE_GLOBAL_EXTRA_MODE: i64 = 15;

#[derive(Debug, Clone, Copy)]
struct ButtonHitCandidate {
    button_no: i64,
    sort_key: ButtonSortKey,
    runtime_slot: usize,
    se_no: i64,
    was_hit: bool,
}

#[derive(Debug, Clone, Copy)]
struct ButtonOwnerInfo {
    button_no: i64,
    runtime_slot: usize,
    se_no: i64,
    was_hit: bool,
}

fn push_object_button_decided_action(
    obj: &globals::ObjectState,
    out: &mut Vec<globals::PendingButtonAction>,
) {
    if !obj.button.decided_action_scn_name.is_empty() {
        if sg_debug_enabled() {
            eprintln!(
                "[SG_DEBUG][BUTTON_TRACE][CALLBACK] enqueue user_call file={:?} button_no={} group_no={} action_no={} state={} hit={} pushed={} call={}::{}/{}",
                obj.file_name,
                obj.button.button_no,
                obj.button.group_no,
                obj.button.action_no,
                obj.button.state,
                obj.button.hit,
                obj.button.pushed,
                obj.button.decided_action_scn_name,
                obj.button.decided_action_cmd_name,
                obj.button.decided_action_z_no
            );
        }
        out.push(globals::PendingButtonAction {
            kind: globals::PendingButtonActionKind::UserCall {
                scn_name: obj.button.decided_action_scn_name.clone(),
                cmd_name: obj.button.decided_action_cmd_name.clone(),
                z_no: obj.button.decided_action_z_no,
            },
        });
    } else if obj.button.sys_type != 0 {
        if sg_debug_enabled() {
            eprintln!(
                "[SG_DEBUG][BUTTON_TRACE][CALLBACK] enqueue syscom file={:?} button_no={} group_no={} action_no={} state={} hit={} pushed={} sys_type={} sys_opt={} mode={}",
                obj.file_name,
                obj.button.button_no,
                obj.button.group_no,
                obj.button.action_no,
                obj.button.state,
                obj.button.hit,
                obj.button.pushed,
                obj.button.sys_type,
                obj.button.sys_type_opt,
                obj.button.mode
            );
        }
        out.push(globals::PendingButtonAction {
            kind: globals::PendingButtonActionKind::Syscom {
                sys_type: obj.button.sys_type,
                sys_type_opt: obj.button.sys_type_opt,
                mode: obj.button.mode,
            },
        });
    } else if sg_debug_enabled() {
        eprintln!(
            "[SG_DEBUG][BUTTON_TRACE][CALLBACK] no_callback file={:?} button_no={} group_no={} action_no={} state={} hit={} pushed={}",
            obj.file_name,
            obj.button.button_no,
            obj.button.group_no,
            obj.button.action_no,
            obj.button.state,
            obj.button.hit,
            obj.button.pushed
        );
    }
}

fn syscom_feature_enabled_for_button(
    syscom: &globals::SyscomRuntimeState,
    button: &globals::ObjectButtonState,
) -> bool {
    match button.sys_type {
        TNM_SYSCOM_TYPE_NONE => true,
        TNM_SYSCOM_TYPE_SAVE => syscom.save_feature.check_enabled() != 0,
        TNM_SYSCOM_TYPE_LOAD => syscom.load_feature.check_enabled() != 0,
        TNM_SYSCOM_TYPE_READ_SKIP => syscom.read_skip.check_enabled() != 0,
        TNM_SYSCOM_TYPE_AUTO_MODE => syscom.auto_mode.check_enabled() != 0,
        TNM_SYSCOM_TYPE_RETURN_SEL => syscom.return_to_sel.check_enabled() != 0,
        TNM_SYSCOM_TYPE_HIDE_MWND => syscom.hide_mwnd.check_enabled() != 0,
        TNM_SYSCOM_TYPE_MSG_BACK => syscom.msg_back.check_enabled() != 0,
        TNM_SYSCOM_TYPE_KOE_PLAY => true,
        TNM_SYSCOM_TYPE_QUICK_SAVE => syscom.save_feature.check_enabled() != 0,
        TNM_SYSCOM_TYPE_QUICK_LOAD => syscom.load_feature.check_enabled() != 0,
        TNM_SYSCOM_TYPE_CONFIG => true,
        TNM_SYSCOM_TYPE_LOCAL_EXTRA_SWITCH => syscom.local_extra_switch.check_enabled() != 0,
        TNM_SYSCOM_TYPE_LOCAL_EXTRA_MODE => syscom.local_extra_mode.check_enabled() != 0,
        TNM_SYSCOM_TYPE_GLOBAL_EXTRA_SWITCH | TNM_SYSCOM_TYPE_GLOBAL_EXTRA_MODE => true,
        _ => true,
    }
}

fn syscom_mode_for_button(
    syscom: &globals::SyscomRuntimeState,
    button: &globals::ObjectButtonState,
) -> i64 {
    match button.sys_type {
        TNM_SYSCOM_TYPE_READ_SKIP => i64::from(syscom.read_skip.onoff),
        TNM_SYSCOM_TYPE_AUTO_MODE => i64::from(syscom.auto_mode.onoff),
        TNM_SYSCOM_TYPE_LOCAL_EXTRA_SWITCH => i64::from(syscom.local_extra_switch.onoff),
        TNM_SYSCOM_TYPE_LOCAL_EXTRA_MODE => syscom.local_extra_mode.value,
        _ => 0,
    }
}

fn button_syscom_mode_visible(
    syscom: &globals::SyscomRuntimeState,
    button: &globals::ObjectButtonState,
) -> bool {
    button.sys_type == TNM_SYSCOM_TYPE_NONE || syscom_mode_for_button(syscom, button) == button.mode
}

fn mwnd_button_forced_disabled(
    syscom: &globals::SyscomRuntimeState,
    mwnd_button_idx: Option<usize>,
) -> bool {
    if syscom.mwnd_btn_disable_all {
        return true;
    }
    mwnd_button_idx
        .and_then(|idx| syscom.mwnd_btn_disable.get(&(idx as i64)))
        .copied()
        .unwrap_or(false)
}

fn button_effective_disabled(
    syscom: &globals::SyscomRuntimeState,
    obj: &globals::ObjectState,
    mwnd_button_idx: Option<usize>,
) -> bool {
    button_disabled_reason(syscom, obj, mwnd_button_idx).is_some()
}

fn button_disabled_reason(
    syscom: &globals::SyscomRuntimeState,
    obj: &globals::ObjectState,
    mwnd_button_idx: Option<usize>,
) -> Option<&'static str> {
    if obj.button.is_disabled() {
        return Some("object_state_disable");
    }
    if mwnd_button_forced_disabled(syscom, mwnd_button_idx) {
        return Some("syscom_mwnd_button_disable");
    }
    if !syscom_feature_enabled_for_button(syscom, &obj.button) {
        return Some("syscom_feature_disable");
    }
    None
}

fn button_state_name(state: i64) -> &'static str {
    match state {
        TNM_BTN_STATE_NORMAL => "normal",
        TNM_BTN_STATE_HIT => "hit",
        TNM_BTN_STATE_PUSH => "push",
        TNM_BTN_STATE_SELECT => "select",
        TNM_BTN_STATE_DISABLE => "disable",
        _ => "unknown",
    }
}

fn object_button_renderable_by_syscom(
    syscom: &globals::SyscomRuntimeState,
    obj: &globals::ObjectState,
) -> bool {
    !obj.button.enabled || button_syscom_mode_visible(syscom, &obj.button)
}

fn button_real_state_for_visual(
    syscom: &globals::SyscomRuntimeState,
    st: &globals::StageFormState,
    stage_idx: i64,
    obj: &globals::ObjectState,
    mwnd_button_idx: Option<usize>,
) -> i64 {
    if let Some(reason) = button_disabled_reason(syscom, obj, mwnd_button_idx) {
        if sg_debug_enabled() {
            eprintln!(
                "[SG_DEBUG][BUTTON_TRACE][VISUAL] real_state=disable reason={} stage={} file={:?} mwnd_button_idx={:?} button_no={} group_no={} group_idx={:?} action_no={} raw_state={} enabled={} hit={} pushed={} sys_type={} sys_opt={} mode={} touch_disable={}",
                reason,
                stage_idx,
                obj.file_name,
                mwnd_button_idx,
                obj.button.button_no,
                obj.button.group_no,
                obj.button.group_idx(),
                obj.button.action_no,
                obj.button.state,
                obj.button.enabled,
                obj.button.hit,
                obj.button.pushed,
                obj.button.sys_type,
                obj.button.sys_type_opt,
                obj.button.mode,
                syscom.mwnd_btn_touch_disable
            );
        }
        return TNM_BTN_STATE_DISABLE;
    }
    if obj.button.state == TNM_BTN_STATE_SELECT || obj.button.state == TNM_BTN_STATE_DISABLE {
        return obj.button.state;
    }
    if syscom.mwnd_btn_touch_disable {
        if sg_debug_enabled() && obj.button.enabled {
            eprintln!(
                "[SG_DEBUG][BUTTON_TRACE][VISUAL] real_state=normal reason=touch_disable stage={} file={:?} mwnd_button_idx={:?} button_no={} group_no={} action_no={}",
                stage_idx, obj.file_name, mwnd_button_idx, obj.button.button_no, obj.button.group_no, obj.button.action_no
            );
        }
        return TNM_BTN_STATE_NORMAL;
    }
    if let Some(gidx) = obj.button.group_idx() {
        if let Some(gl) = st
            .group_lists
            .get(&stage_idx)
            .and_then(|groups| groups.get(gidx))
        {
            if gl.decided_button_no == obj.button.button_no {
                return TNM_BTN_STATE_PUSH;
            }
            if gl.hit_button_no == obj.button.button_no {
                return TNM_BTN_STATE_HIT;
            }
            if gl.pushed_button_no == obj.button.button_no {
                return TNM_BTN_STATE_PUSH;
            }
        }
    } else if obj.button.pushed {
        return TNM_BTN_STATE_PUSH;
    } else if obj.button.hit {
        return TNM_BTN_STATE_HIT;
    }
    TNM_BTN_STATE_NORMAL
}

fn collect_button_decided_action_by_runtime_slot_recursive(
    obj_idx: usize,
    obj: &globals::ObjectState,
    runtime_slot: usize,
    out: &mut Vec<globals::PendingButtonAction>,
) -> bool {
    if object_runtime_slot(obj_idx, obj) == runtime_slot {
        if obj.used && obj.button.enabled && obj.button.action_no >= 0 {
            push_object_button_decided_action(obj, out);
        }
        return true;
    }
    for (child_idx, child) in obj.runtime.child_objects.iter().enumerate() {
        if collect_button_decided_action_by_runtime_slot_recursive(
            child_idx,
            child,
            runtime_slot,
            out,
        ) {
            return true;
        }
    }
    false
}

fn find_button_se_no_by_runtime_slot_recursive(
    obj_idx: usize,
    obj: &globals::ObjectState,
    runtime_slot: usize,
) -> Option<i64> {
    if object_runtime_slot(obj_idx, obj) == runtime_slot {
        return (obj.used && obj.button.enabled && obj.button.action_no >= 0)
            .then_some(obj.button.se_no);
    }
    for (child_idx, child) in obj.runtime.child_objects.iter().enumerate() {
        if let Some(se_no) =
            find_button_se_no_by_runtime_slot_recursive(child_idx, child, runtime_slot)
        {
            return Some(se_no);
        }
    }
    None
}

fn find_button_se_no_in_list_by_runtime_slot(
    objs: &[globals::ObjectState],
    runtime_slot: usize,
) -> Option<i64> {
    for (obj_idx, obj) in objs.iter().enumerate() {
        if let Some(se_no) = find_button_se_no_by_runtime_slot_recursive(obj_idx, obj, runtime_slot)
        {
            return Some(se_no);
        }
    }
    None
}

fn set_button_pushed_by_runtime_slot_recursive(
    obj_idx: usize,
    obj: &mut globals::ObjectState,
    runtime_slot: usize,
) -> bool {
    if object_runtime_slot(obj_idx, obj) == runtime_slot {
        if obj.button.enabled {
            obj.button.last_pushed = obj.button.pushed;
            obj.button.pushed = true;
        }
        return true;
    }
    for (child_idx, child) in obj.runtime.child_objects.iter_mut().enumerate() {
        if set_button_pushed_by_runtime_slot_recursive(child_idx, child, runtime_slot) {
            return true;
        }
    }
    false
}

fn object_button_push_keep_by_runtime_slot_recursive(
    obj_idx: usize,
    obj: &globals::ObjectState,
    runtime_slot: usize,
) -> bool {
    if object_runtime_slot(obj_idx, obj) == runtime_slot {
        return obj.button.enabled && obj.button.push_keep;
    }
    obj.runtime
        .child_objects
        .iter()
        .enumerate()
        .any(|(child_idx, child)| {
            object_button_push_keep_by_runtime_slot_recursive(child_idx, child, runtime_slot)
        })
}

fn object_button_push_keep_in_list_by_runtime_slot(
    objs: &[globals::ObjectState],
    runtime_slot: usize,
) -> bool {
    objs.iter().enumerate().any(|(obj_idx, obj)| {
        object_button_push_keep_by_runtime_slot_recursive(obj_idx, obj, runtime_slot)
    })
}

fn clear_button_hit_recursive(obj: &mut globals::ObjectState) {
    if obj.button.enabled {
        obj.button.last_hit = obj.button.hit;
        obj.button.hit = false;
    }
    for child in &mut obj.runtime.child_objects {
        clear_button_hit_recursive(child);
    }
}

fn set_button_hit_by_runtime_slot_recursive(
    obj_idx: usize,
    obj: &mut globals::ObjectState,
    runtime_slot: usize,
) -> bool {
    if object_runtime_slot(obj_idx, obj) == runtime_slot {
        obj.button.hit = true;
        return true;
    }
    for (child_idx, child) in obj.runtime.child_objects.iter_mut().enumerate() {
        if set_button_hit_by_runtime_slot_recursive(child_idx, child, runtime_slot) {
            return true;
        }
    }
    false
}

fn set_button_pushed_recursive(obj: &mut globals::ObjectState, group_idx: usize, button_no: i64) {
    if obj.button.enabled
        && obj.button.group_idx() == Some(group_idx)
        && obj.button.button_no == button_no
    {
        obj.button.last_pushed = obj.button.pushed;
        obj.button.pushed = true;
    }
    for child in &mut obj.runtime.child_objects {
        set_button_pushed_recursive(child, group_idx, button_no);
    }
}

fn mark_standalone_button_pushed_from_hit_recursive(
    _obj_idx: usize,
    obj: &mut globals::ObjectState,
) -> Option<i64> {
    if has_standalone_button_action(obj) && obj.button.hit {
        let was_pushed = obj.button.pushed;
        obj.button.last_pushed = obj.button.pushed;
        obj.button.pushed = true;
        if !was_pushed {
            return Some(obj.button.se_no);
        }
    }
    for (child_idx, child) in obj.runtime.child_objects.iter_mut().enumerate() {
        if let Some(se_no) = mark_standalone_button_pushed_from_hit_recursive(child_idx, child) {
            return Some(se_no);
        }
    }
    None
}
fn standalone_button_hit_recursive(obj: &globals::ObjectState) -> bool {
    if has_standalone_button_action(obj) && obj.button.hit {
        return true;
    }
    obj.runtime
        .child_objects
        .iter()
        .any(standalone_button_hit_recursive)
}

fn standalone_button_pushed_recursive(obj: &globals::ObjectState) -> bool {
    if has_standalone_button_action(obj) && obj.button.pushed {
        return true;
    }
    obj.runtime
        .child_objects
        .iter()
        .any(standalone_button_pushed_recursive)
}

fn clear_button_pushed_recursive(obj: &mut globals::ObjectState) {
    if obj.button.enabled {
        obj.button.last_pushed = obj.button.pushed;
        obj.button.pushed = false;
    }
    for child in &mut obj.runtime.child_objects {
        clear_button_pushed_recursive(child);
    }
}

fn object_button_push_keep_recursive(
    obj: &globals::ObjectState,
    group_idx: usize,
    button_no: i64,
) -> bool {
    if obj.button.enabled
        && obj.button.group_idx() == Some(group_idx)
        && obj.button.button_no == button_no
        && obj.button.push_keep
    {
        return true;
    }
    obj.runtime
        .child_objects
        .iter()
        .any(|child| object_button_push_keep_recursive(child, group_idx, button_no))
}

fn hit_test_render_sprite(
    images: &mut ImageManager,
    sprite: &Sprite,
    mx: i32,
    my: i32,
    alpha_test: bool,
) -> bool {
    if !sprite.visible || sprite.tr == 0 {
        return false;
    }
    if let Some(clip) = sprite.dst_clip {
        if mx < clip.left || my < clip.top || mx >= clip.right || my >= clip.bottom {
            return false;
        }
    }
    let Some(img_id) = sprite.image_id else {
        return false;
    };
    let Some(img) = images.get(img_id).map(|a| a.as_ref()) else {
        return false;
    };
    let (w, h) = match sprite.size_mode {
        SpriteSizeMode::Intrinsic => (img.width as f32, img.height as f32),
        SpriteSizeMode::Explicit { width, height } => (width as f32, height as f32),
    };
    let (anchor_x, anchor_y) = match sprite.fit {
        SpriteFit::PixelRect => (sprite.x as f32, sprite.y as f32),
        SpriteFit::FullScreen => (0.0, 0.0),
    };
    if sprite.scale_x == 0.0 || sprite.scale_y == 0.0 {
        return false;
    }
    let (origin_x, origin_y) = if sprite.object_anchor {
        (anchor_x, anchor_y)
    } else {
        (anchor_x + sprite.pivot_x, anchor_y + sprite.pivot_y)
    };
    let mut px = mx as f32 - origin_x;
    let mut py = my as f32 - origin_y;
    if sprite.rotate != 0.0 {
        let (s, c) = (-sprite.rotate).sin_cos();
        let rx = px * c - py * s;
        let ry = px * s + py * c;
        px = rx;
        py = ry;
    }
    let (tex_center_x, tex_center_y) = if sprite.object_anchor {
        (sprite.texture_center_x, sprite.texture_center_y)
    } else {
        (0.0, 0.0)
    };
    let local_x = px / sprite.scale_x + sprite.pivot_x + tex_center_x;
    let local_y = py / sprite.scale_y + sprite.pivot_y + tex_center_y;
    if !(0.0 <= local_x && local_x < w && 0.0 <= local_y && local_y < h) {
        return false;
    }
    if alpha_test {
        let (sx, sy) = match sprite.src_clip {
            Some(src) => (
                src.left.saturating_add(local_x.floor() as i32),
                src.top.saturating_add(local_y.floor() as i32),
            ),
            None => (local_x.floor() as i32, local_y.floor() as i32),
        };
        if !CommandContext::alpha_test_image(img, sx, sy) {
            return false;
        }
    }
    true
}

fn hit_test_layer_sprite(
    images: &mut ImageManager,
    layers: &LayerManager,
    layer_id: LayerId,
    sprite_id: SpriteId,
    mx: i32,
    my: i32,
    alpha_test: bool,
) -> bool {
    let Some(spr) = layers.layer(layer_id).and_then(|l| l.sprite(sprite_id)) else {
        return false;
    };
    hit_test_render_sprite(images, spr, mx, my, alpha_test)
}

fn object_button_sort_key(
    ids: &constants::RuntimeConstants,
    gfx: &graphics::GfxRuntime,
    stage_idx: i64,
    runtime_slot: usize,
    obj: &globals::ObjectState,
) -> ButtonSortKey {
    let embedded_tree_object = obj.nested_runtime_slot.is_some();
    let layer = obj
        .lookup_int_prop(ids, ids.obj_layer)
        .or_else(|| {
            if embedded_tree_object {
                None
            } else {
                gfx.object_peek_layer(stage_idx, runtime_slot as i64)
            }
        })
        .unwrap_or(obj.base.layer);
    let order = obj
        .lookup_int_prop(ids, ids.obj_order)
        .or_else(|| {
            if embedded_tree_object {
                None
            } else {
                gfx.object_peek_order(stage_idx, runtime_slot as i64)
            }
        })
        .unwrap_or(obj.base.order);
    ButtonSortKey { order, layer }
}

fn button_sort_ge(lhs: ButtonSortKey, rhs: ButtonSortKey) -> bool {
    lhs.order > rhs.order || (lhs.order == rhs.order && lhs.layer >= rhs.layer)
}

fn has_standalone_button_action(obj: &globals::ObjectState) -> bool {
    obj.used
        && obj.button.enabled
        && !obj.button.is_disabled()
        && obj.button.group_idx().is_none()
        && obj.button.action_no >= 0
}

fn merge_button_hit(
    best: &mut Option<ButtonHitCandidate>,
    tied: &mut bool,
    hit: ButtonHitCandidate,
) {
    match *best {
        None => {
            *best = Some(hit);
            *tied = false;
        }
        Some(prev) if button_sort_ge(hit.sort_key, prev.sort_key) => {
            // C_tnm_btn_mng::hit_test_proc uses >=, so equal order/layer means
            // the later registered button wins rather than producing a tie.
            *best = Some(hit);
            *tied = false;
        }
        _ => {}
    }
}

fn object_event_value(
    ids: &constants::RuntimeConstants,
    obj: &globals::ObjectState,
    event_op: i32,
    current: i64,
) -> i64 {
    if event_op != 0 {
        obj.int_event_by_op(ids, event_op)
            .map(|ev| ev.get_total_value() as i64)
            .unwrap_or(current)
    } else {
        current
    }
}

fn object_button_effective_gfx_hit(
    images: &mut ImageManager,
    layers: &LayerManager,
    gfx: &graphics::GfxRuntime,
    ids: &constants::RuntimeConstants,
    stage_idx: i64,
    runtime_slot: usize,
    obj: &globals::ObjectState,
    mx: i32,
    my: i32,
    parent_state: Option<ParentRenderState>,
) -> Option<ButtonSortKey> {
    let embedded_tree_object = obj.nested_runtime_slot.is_some();
    let disp = obj
        .lookup_int_prop(ids, ids.obj_disp)
        .or_else(|| {
            if embedded_tree_object {
                None
            } else {
                gfx.object_peek_disp(stage_idx, runtime_slot as i64)
            }
        })
        .unwrap_or(obj.base.disp);
    if disp == 0 {
        return None;
    }

    let mut tr = obj.lookup_int_prop(ids, ids.obj_tr).unwrap_or(obj.base.tr);
    tr = object_event_value(ids, obj, ids.obj_tr_eve, tr);
    tr = obj
        .runtime
        .prop_event_lists
        .tr_rep
        .iter()
        .fold(tr, |acc, ev| {
            acc.saturating_mul(ev.get_total_value() as i64)
                .div_euclid(255)
        });
    if tr <= 0 {
        return None;
    }

    if parent_state.is_none() {
        if let Some((layer_id, sprite_id)) =
            gfx.object_sprite_binding(stage_idx, runtime_slot as i64)
        {
            if hit_test_layer_sprite(
                images,
                layers,
                layer_id,
                sprite_id,
                mx,
                my,
                obj.button.alpha_test,
            ) {
                return Some(object_button_sort_key(
                    ids,
                    gfx,
                    stage_idx,
                    runtime_slot,
                    obj,
                ));
            }
            return None;
        }
    }

    let (base_x, base_y) = if embedded_tree_object {
        (obj.base.x, obj.base.y)
    } else {
        gfx.object_peek_pos(stage_idx, runtime_slot as i64)
            .unwrap_or((obj.base.x, obj.base.y))
    };
    let mut x = obj.lookup_int_prop(ids, ids.obj_x).unwrap_or(base_x);
    let mut y = obj.lookup_int_prop(ids, ids.obj_y).unwrap_or(base_y);
    x = object_event_value(ids, obj, ids.obj_x_eve, x);
    y = object_event_value(ids, obj, ids.obj_y_eve, y);
    x += obj
        .runtime
        .prop_event_lists
        .x_rep
        .iter()
        .map(|ev| ev.get_total_value() as i64)
        .sum::<i64>();
    y += obj
        .runtime
        .prop_event_lists
        .y_rep
        .iter()
        .map(|ev| ev.get_total_value() as i64)
        .sum::<i64>();

    let mut scale_x = obj
        .lookup_int_prop(ids, ids.obj_scale_x)
        .unwrap_or(obj.base.scale_x);
    let mut scale_y = obj
        .lookup_int_prop(ids, ids.obj_scale_y)
        .unwrap_or(obj.base.scale_y);
    scale_x = object_event_value(ids, obj, ids.obj_scale_x_eve, scale_x);
    scale_y = object_event_value(ids, obj, ids.obj_scale_y_eve, scale_y);
    if scale_x == 0 || scale_y == 0 {
        return None;
    }

    let mut patno = obj
        .lookup_int_prop(ids, ids.obj_patno)
        .or_else(|| gfx.object_peek_patno(stage_idx, runtime_slot as i64))
        .unwrap_or(obj.base.patno);
    patno = object_event_value(ids, obj, ids.obj_patno_eve, patno);
    patno = patno.saturating_add(obj.button.cut_no);

    let file_name = obj.file_name.as_ref()?;
    let img_id = CommandContext::load_any_image_for_hit(images, file_name.as_str(), patno)?;

    let mut sprite = Sprite::default();
    sprite.image_id = Some(img_id);
    if let Some(img) = images.get(img_id) {
        sprite.object_anchor = true;
        sprite.texture_center_x = img.center_x as f32;
        sprite.texture_center_y = img.center_y as f32;
    } else {
        sprite.object_anchor = false;
        sprite.texture_center_x = 0.0;
        sprite.texture_center_y = 0.0;
    }
    sprite.visible = true;
    let center_x = obj.lookup_int_prop(ids, ids.obj_center_x).unwrap_or(obj.base.center_x);
    let center_y = obj.lookup_int_prop(ids, ids.obj_center_y).unwrap_or(obj.base.center_y);
    let center_z = obj.lookup_int_prop(ids, ids.obj_center_z).unwrap_or(obj.base.center_z);
    let center_rep_x = obj.lookup_int_prop(ids, ids.obj_center_rep_x).unwrap_or(obj.base.center_rep_x);
    let center_rep_y = obj.lookup_int_prop(ids, ids.obj_center_rep_y).unwrap_or(obj.base.center_rep_y);
    let center_rep_z = obj.lookup_int_prop(ids, ids.obj_center_rep_z).unwrap_or(obj.base.center_rep_z);
    sprite.x = x as i32;
    sprite.y = y as i32;
    sprite.pivot_x = (center_x + center_rep_x) as f32;
    sprite.pivot_y = (center_y + center_rep_y) as f32;
    sprite.pivot_z = (center_z + center_rep_z) as f32;
    sprite.scale_x = scale_x as f32 / 1000.0;
    sprite.scale_y = scale_y as f32 / 1000.0;
    sprite.tr = tr.clamp(0, 255) as u8;
    if let Some(parent) = parent_state {
        let dummy = ObjectRenderInfo::default();
        apply_parent_render_state_to_sprite(&mut sprite, &dummy, &parent);
    }
    sprite.x = (sprite.x as i64 + center_rep_x).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    sprite.y = (sprite.y as i64 + center_rep_y).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    sprite.z += center_rep_z as f32;

    if !hit_test_render_sprite(images, &sprite, mx, my, obj.button.alpha_test) {
        return None;
    }

    Some(object_button_sort_key(
        ids,
        gfx,
        stage_idx,
        runtime_slot,
        obj,
    ))
}

fn collect_standalone_button_decided_actions_recursive(
    obj: &globals::ObjectState,
    out: &mut Vec<globals::PendingButtonAction>,
    sounds: &mut Vec<i64>,
) {
    if has_standalone_button_action(obj)
        && obj.button.pushed
        && (obj.button.hit || obj.button.push_keep)
    {
        push_object_button_decided_action(obj, out);
        sounds.push(obj.button.se_no);
    }
    for child in &obj.runtime.child_objects {
        collect_standalone_button_decided_actions_recursive(child, out, sounds);
    }
}

#[derive(Debug, Clone, Copy)]
struct ButtonObjectRenderInfo {
    disp: bool,
    x: i64,
    y: i64,
    z: i64,
    x_rep: i64,
    y_rep: i64,
    z_rep: i64,
    center_x: i64,
    center_y: i64,
    center_z: i64,
    center_rep_x: i64,
    center_rep_y: i64,
    center_rep_z: i64,
    scale_x: i64,
    scale_y: i64,
    scale_z: i64,
    rotate_x: i64,
    rotate_y: i64,
    rotate_z: i64,
    tr: i64,
    tr_rep: i64,
    world_no: i64,
    dst_clip: Option<ClipRect>,
}

fn fetch_bound_render_sprites_for_hit(
    layers: &LayerManager,
    gfx: &graphics::GfxRuntime,
    stage_idx: i64,
    runtime_slot: usize,
    obj: &globals::ObjectState,
) -> Vec<RenderSprite> {
    fn push_one(layers: &LayerManager, lid: LayerId, sid: SpriteId, out: &mut Vec<RenderSprite>) {
        let Some(layer) = layers.layer(lid) else {
            return;
        };
        let Some(sprite) = layer.sprite(sid) else {
            return;
        };
        if sprite.image_id.is_none() {
            return;
        }
        out.push(RenderSprite::new(Some(lid), Some(sid), sprite.clone()));
    }
    let mut out = Vec::new();
    match &obj.backend {
        globals::ObjectBackend::Gfx => {
            if let Some((lid, sid)) = gfx.object_sprite_binding(stage_idx, runtime_slot as i64) {
                push_one(layers, lid, sid, &mut out);
            }
        }
        globals::ObjectBackend::None => {}
        backend => {
            for (layer_id, sprite_id) in layer_backed_object_sprite_bindings(backend) {
                push_one(layers, layer_id, sprite_id, &mut out);
            }
        }
    }
    out
}

fn button_object_render_info(
    ids: &constants::RuntimeConstants,
    gfx: &graphics::GfxRuntime,
    stage_idx: i64,
    obj_idx: usize,
    obj: &globals::ObjectState,
) -> ButtonObjectRenderInfo {
    let runtime_slot = object_runtime_slot(obj_idx, obj);
    let embedded_tree_object = obj.nested_runtime_slot.is_some();
    let use_gfx_object_state =
        matches!(obj.backend, globals::ObjectBackend::Gfx) && !embedded_tree_object;
    let extra = |id: i32, default: i64| -> i64 {
        if id != 0 {
            obj.lookup_int_prop(ids, id).unwrap_or(default)
        } else {
            default
        }
    };
    let gfx_disp = || {
        if use_gfx_object_state {
            gfx.object_peek_disp(stage_idx, runtime_slot as i64)
        } else {
            None
        }
    };
    let gfx_pos = || {
        if use_gfx_object_state {
            gfx.object_peek_pos(stage_idx, runtime_slot as i64)
        } else {
            None
        }
    };
    let x_rep = obj
        .runtime
        .prop_event_lists
        .x_rep
        .iter()
        .map(|ev| ev.get_total_value() as i64)
        .sum::<i64>();
    let y_rep = obj
        .runtime
        .prop_event_lists
        .y_rep
        .iter()
        .map(|ev| ev.get_total_value() as i64)
        .sum::<i64>();
    let z_rep = obj
        .runtime
        .prop_event_lists
        .z_rep
        .iter()
        .map(|ev| ev.get_total_value() as i64)
        .sum::<i64>();
    let tr_rep = obj
        .runtime
        .prop_event_lists
        .tr_rep
        .iter()
        .fold(255i64, |acc, ev| {
            acc.saturating_mul(ev.get_total_value() as i64)
                .div_euclid(255)
        });
    let dst_clip = if extra(ids.obj_clip_use, obj.base.clip_use) != 0 {
        Some(ClipRect {
            left: extra(ids.obj_clip_left, obj.base.clip_left) as i32,
            top: extra(ids.obj_clip_top, obj.base.clip_top) as i32,
            right: extra(ids.obj_clip_right, obj.base.clip_right) as i32,
            bottom: extra(ids.obj_clip_bottom, obj.base.clip_bottom) as i32,
        })
    } else {
        None
    };
    ButtonObjectRenderInfo {
        disp: extra(ids.obj_disp, gfx_disp().unwrap_or(obj.base.disp)) != 0,
        x: object_event_value(
            ids,
            obj,
            ids.obj_x_eve,
            extra(ids.obj_x, gfx_pos().map(|v| v.0).unwrap_or(obj.base.x)),
        ),
        y: object_event_value(
            ids,
            obj,
            ids.obj_y_eve,
            extra(ids.obj_y, gfx_pos().map(|v| v.1).unwrap_or(obj.base.y)),
        ),
        z: object_event_value(ids, obj, ids.obj_z_eve, extra(ids.obj_z, obj.base.z)),
        x_rep,
        y_rep,
        z_rep,
        center_x: object_event_value(
            ids,
            obj,
            ids.obj_center_x_eve,
            extra(ids.obj_center_x, obj.base.center_x),
        ),
        center_y: object_event_value(
            ids,
            obj,
            ids.obj_center_y_eve,
            extra(ids.obj_center_y, obj.base.center_y),
        ),
        center_z: object_event_value(
            ids,
            obj,
            ids.obj_center_z_eve,
            extra(ids.obj_center_z, obj.base.center_z),
        ),
        center_rep_x: extra(ids.obj_center_rep_x, obj.base.center_rep_x),
        center_rep_y: extra(ids.obj_center_rep_y, obj.base.center_rep_y),
        center_rep_z: extra(ids.obj_center_rep_z, obj.base.center_rep_z),
        scale_x: object_event_value(
            ids,
            obj,
            ids.obj_scale_x_eve,
            extra(ids.obj_scale_x, obj.base.scale_x),
        ),
        scale_y: object_event_value(
            ids,
            obj,
            ids.obj_scale_y_eve,
            extra(ids.obj_scale_y, obj.base.scale_y),
        ),
        scale_z: object_event_value(
            ids,
            obj,
            ids.obj_scale_z_eve,
            extra(ids.obj_scale_z, obj.base.scale_z),
        ),
        rotate_x: object_event_value(
            ids,
            obj,
            ids.obj_rotate_x_eve,
            extra(ids.obj_rotate_x, obj.base.rotate_x),
        ),
        rotate_y: object_event_value(
            ids,
            obj,
            ids.obj_rotate_y_eve,
            extra(ids.obj_rotate_y, obj.base.rotate_y),
        ),
        rotate_z: object_event_value(
            ids,
            obj,
            ids.obj_rotate_z_eve,
            extra(ids.obj_rotate_z, obj.base.rotate_z),
        ),
        tr: object_event_value(ids, obj, ids.obj_tr_eve, extra(ids.obj_tr, obj.base.tr)),
        tr_rep,
        world_no: extra(ids.obj_world, obj.base.world),
        dst_clip,
    }
}

fn apply_button_object_render_info_to_sprite(sprite: &mut Sprite, info: &ButtonObjectRenderInfo) {
    sprite.visible = info.disp;
    sprite.x = (info.x + info.x_rep).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    sprite.y = (info.y + info.y_rep).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    sprite.z = (info.z + info.z_rep) as f32;
    sprite.pivot_x = (info.center_x + info.center_rep_x) as f32;
    sprite.pivot_y = (info.center_y + info.center_rep_y) as f32;
    sprite.pivot_z = (info.center_z + info.center_rep_z) as f32;
    sprite.scale_x = info.scale_x as f32 / 1000.0;
    sprite.scale_y = info.scale_y as f32 / 1000.0;
    sprite.scale_z = info.scale_z as f32 / 1000.0;
    sprite.rotate = info.rotate_z as f32 * std::f32::consts::PI / 1800.0;
    sprite.rotate_x = info.rotate_x as f32 * std::f32::consts::PI / 1800.0;
    sprite.rotate_y = info.rotate_y as f32 * std::f32::consts::PI / 1800.0;
    sprite.tr = ((info.tr.clamp(0, 255) * info.tr_rep.clamp(0, 255)) / 255).clamp(0, 255) as u8;
    sprite.dst_clip = info.dst_clip;
}

fn finalize_button_object_center_rep_to_sprite(sprite: &mut Sprite, info: &ButtonObjectRenderInfo) {
    let x = (sprite.x as i64 + info.center_rep_x).clamp(i32::MIN as i64, i32::MAX as i64);
    let y = (sprite.y as i64 + info.center_rep_y).clamp(i32::MIN as i64, i32::MAX as i64);
    sprite.x = x as i32;
    sprite.y = y as i32;
    sprite.z += info.center_rep_z as f32;
}

fn object_button_hit_sort_key_from_render(
    images: &mut ImageManager,
    layers: &LayerManager,
    gfx: &graphics::GfxRuntime,
    ids: &constants::RuntimeConstants,
    syscom: &globals::SyscomRuntimeState,
    stage_idx: i64,
    obj_idx: usize,
    obj: &globals::ObjectState,
    mx: i32,
    my: i32,
    parent_state: Option<ParentRenderState>,
) -> Option<ButtonSortKey> {
    if !object_button_renderable_by_syscom(syscom, obj)
        || button_effective_disabled(syscom, obj, None)
        || syscom.mwnd_btn_touch_disable
    {
        if sg_debug_enabled() && obj.button.enabled {
            eprintln!(
                "[SG_DEBUG][BUTTON_TRACE][HIT] reject stage={} obj_idx={} runtime_slot={} file={:?} mx={} my={} visible={} disabled_reason={:?} touch_disable={} button_no={} group_no={} group_idx={:?} action_no={} state={} hit={} pushed={} alpha_test={} sys_type={} sys_opt={} mode={}",
                stage_idx,
                obj_idx,
                object_runtime_slot(obj_idx, obj),
                obj.file_name,
                mx,
                my,
                object_button_renderable_by_syscom(syscom, obj),
                button_disabled_reason(syscom, obj, None),
                syscom.mwnd_btn_touch_disable,
                obj.button.button_no,
                obj.button.group_no,
                obj.button.group_idx(),
                obj.button.action_no,
                obj.button.state,
                obj.button.hit,
                obj.button.pushed,
                obj.button.alpha_test,
                obj.button.sys_type,
                obj.button.sys_type_opt,
                obj.button.mode
            );
        }
        return None;
    }
    let runtime_slot = object_runtime_slot(obj_idx, obj);
    let info = button_object_render_info(ids, gfx, stage_idx, obj_idx, obj);
    let mut bound = fetch_bound_render_sprites_for_hit(layers, gfx, stage_idx, runtime_slot, obj);
    for rs in &mut bound {
        apply_button_object_render_info_to_sprite(&mut rs.sprite, &info);
        if let Some(parent) = parent_state {
            let dummy = ObjectRenderInfo::default();
            apply_parent_render_state_to_sprite(&mut rs.sprite, &dummy, &parent);
        }
        finalize_button_object_center_rep_to_sprite(&mut rs.sprite, &info);
        if hit_test_render_sprite(images, &rs.sprite, mx, my, obj.button.alpha_test) {
            let sort_key = object_button_sort_key(ids, gfx, stage_idx, runtime_slot, obj);
            if sg_debug_enabled() {
                eprintln!(
                    "[SG_DEBUG][BUTTON_TRACE][HIT] success stage={} obj_idx={} runtime_slot={} file={:?} mx={} my={} button_no={} group_no={} group_idx={:?} action_no={} state={} hit={} pushed={} alpha_test={} sprite=({:?},{:?}) pos=({}, {}) size_mode={:?} sort={}",
                    stage_idx,
                    obj_idx,
                    runtime_slot,
                    obj.file_name,
                    mx,
                    my,
                    obj.button.button_no,
                    obj.button.group_no,
                    obj.button.group_idx(),
                    obj.button.action_no,
                    obj.button.state,
                    obj.button.hit,
                    obj.button.pushed,
                    obj.button.alpha_test,
                    rs.layer_id,
                    rs.sprite_id,
                    rs.sprite.x,
                    rs.sprite.y,
                    rs.sprite.size_mode,
                    sort_key.display_tuple()
                );
            }
            return Some(sort_key);
        }
    }

    if bound.is_empty() {
        return object_button_effective_gfx_hit(
            images,
            layers,
            gfx,
            ids,
            stage_idx,
            runtime_slot,
            obj,
            mx,
            my,
            parent_state,
        );
    }
    None
}

fn button_parent_render_state(
    layers: &LayerManager,
    gfx: &graphics::GfxRuntime,
    ids: &constants::RuntimeConstants,
    stage_idx: i64,
    obj_idx: usize,
    obj: &globals::ObjectState,
    parent_state: Option<ParentRenderState>,
) -> ParentRenderState {
    let runtime_slot = object_runtime_slot(obj_idx, obj);
    let info = button_object_render_info(ids, gfx, stage_idx, obj_idx, obj);
    let bound = fetch_bound_render_sprites_for_hit(layers, gfx, stage_idx, runtime_slot, obj);
    let mut cur = ParentRenderState {
        world_no: info.world_no,
        pos_x: (info.x + info.x_rep) as f32,
        pos_y: (info.y + info.y_rep) as f32,
        pos_z: (info.z + info.z_rep) as f32,
        center_rep_x: info.center_rep_x as f32,
        center_rep_y: info.center_rep_y as f32,
        center_rep_z: info.center_rep_z as f32,
        scale_x: info.scale_x as f32 / 1000.0,
        scale_y: info.scale_y as f32 / 1000.0,
        scale_z: info.scale_z as f32 / 1000.0,
        rotate_x: info.rotate_x as f32 * std::f32::consts::PI / 1800.0,
        rotate_y: info.rotate_y as f32 * std::f32::consts::PI / 1800.0,
        rotate_z: info.rotate_z as f32 * std::f32::consts::PI / 1800.0,
        tr: ((info.tr.clamp(0, 255) * info.tr_rep.clamp(0, 255)) / 255) as i32,
        mono: 0,
        reverse: 0,
        bright: 0,
        dark: 0,
        color_rate: 0,
        color_r: 255,
        color_g: 255,
        color_b: 255,
        color_add_r: 0,
        color_add_g: 0,
        color_add_b: 0,
        blend: crate::layer::SpriteBlend::Normal,
        dst_clip: info.dst_clip,
        mask_image_id: bound.first().and_then(|s| s.sprite.mask_image_id),
        mask_offset_x: bound.first().map(|s| s.sprite.mask_offset_x).unwrap_or(0),
        mask_offset_y: bound.first().map(|s| s.sprite.mask_offset_y).unwrap_or(0),
        tonecurve_image_id: bound.first().and_then(|s| s.sprite.tonecurve_image_id),
        tonecurve_row: bound.first().map(|s| s.sprite.tonecurve_row).unwrap_or(0.0),
        tonecurve_sat: bound.first().map(|s| s.sprite.tonecurve_sat).unwrap_or(0.0),
    };
    if let Some(parent) = parent_state {
        cur = compose_parent_render_state(parent, cur);
    }
    cur
}

fn hit_test_standalone_action_button_recursive(
    images: &mut ImageManager,
    layers: &LayerManager,
    gfx: &graphics::GfxRuntime,
    ids: &constants::RuntimeConstants,
    syscom: &globals::SyscomRuntimeState,
    stage_idx: i64,
    mx: i32,
    my: i32,
    obj_idx: usize,
    obj: &mut globals::ObjectState,
    parent_state: Option<ParentRenderState>,
) -> Option<ButtonHitCandidate> {
    fn recurse(
        images: &mut ImageManager,
        layers: &LayerManager,
        gfx: &graphics::GfxRuntime,
        ids: &constants::RuntimeConstants,
        syscom: &globals::SyscomRuntimeState,
        stage_idx: i64,
        mx: i32,
        my: i32,
        obj_idx: usize,
        obj: &mut globals::ObjectState,
        parent_state: Option<ParentRenderState>,
        inherited_owner: Option<ButtonOwnerInfo>,
    ) -> Option<ButtonHitCandidate> {
        let runtime_slot = object_runtime_slot(obj_idx, obj);
        let current_owner = if has_standalone_button_action(obj) && !obj.base.no_event_hint {
            Some(ButtonOwnerInfo {
                button_no: obj.button.button_no,
                runtime_slot,
                se_no: obj.button.se_no,
                was_hit: obj.button.last_hit,
            })
        } else {
            None
        };
        let effective_owner = current_owner.or(inherited_owner);

        let mut best = None;
        let mut tied = false;
        if let Some(owner) = effective_owner {
            if !obj.base.no_event_hint {
                if let Some(sort_key) = object_button_hit_sort_key_from_render(
                    images,
                    layers,
                    gfx,
                    ids,
                    syscom,
                    stage_idx,
                    obj_idx,
                    obj,
                    mx,
                    my,
                    parent_state,
                ) {
                    best = Some(ButtonHitCandidate {
                        button_no: owner.button_no,
                        sort_key,
                        runtime_slot: owner.runtime_slot,
                        se_no: owner.se_no,
                        was_hit: owner.was_hit,
                    });
                }
            }
        }
        let cur_parent_state =
            button_parent_render_state(layers, gfx, ids, stage_idx, obj_idx, obj, parent_state);
        for (child_idx, child) in obj.runtime.child_objects.iter_mut().enumerate() {
            if let Some(hit) = recurse(
                images,
                layers,
                gfx,
                ids,
                syscom,
                stage_idx,
                mx,
                my,
                child_idx,
                child,
                Some(cur_parent_state),
                effective_owner,
            ) {
                merge_button_hit(&mut best, &mut tied, hit);
            }
        }
        if tied {
            None
        } else {
            best
        }
    }

    recurse(
        images,
        layers,
        gfx,
        ids,
        syscom,
        stage_idx,
        mx,
        my,
        obj_idx,
        obj,
        parent_state,
        None,
    )
}

fn hit_test_object_button_recursive(
    images: &mut ImageManager,
    layers: &LayerManager,
    gfx: &graphics::GfxRuntime,
    ids: &constants::RuntimeConstants,
    syscom: &globals::SyscomRuntimeState,
    stage_idx: i64,
    group_idx: usize,
    mx: i32,
    my: i32,
    obj_idx: usize,
    obj: &mut globals::ObjectState,
    parent_state: Option<ParentRenderState>,
) -> Option<ButtonHitCandidate> {
    fn recurse(
        images: &mut ImageManager,
        layers: &LayerManager,
        gfx: &graphics::GfxRuntime,
        ids: &constants::RuntimeConstants,
        syscom: &globals::SyscomRuntimeState,
        stage_idx: i64,
        group_idx: usize,
        mx: i32,
        my: i32,
        obj_idx: usize,
        obj: &mut globals::ObjectState,
        parent_state: Option<ParentRenderState>,
        inherited_owner: Option<ButtonOwnerInfo>,
    ) -> Option<ButtonHitCandidate> {
        let runtime_slot = object_runtime_slot(obj_idx, obj);
        let current_owner = if obj.used
            && obj.button.enabled
            && !obj.button.is_disabled()
            && !obj.base.no_event_hint
            && obj.button.action_no >= 0
            && obj.button.group_idx() == Some(group_idx)
        {
            Some(ButtonOwnerInfo {
                button_no: obj.button.button_no,
                runtime_slot,
                se_no: obj.button.se_no,
                was_hit: obj.button.last_hit,
            })
        } else {
            None
        };
        let effective_owner = current_owner.or(inherited_owner);

        let mut best = None;
        let mut tied = false;
        if let Some(owner) = effective_owner {
            if !obj.base.no_event_hint {
                if let Some(sort_key) = object_button_hit_sort_key_from_render(
                    images,
                    layers,
                    gfx,
                    ids,
                    syscom,
                    stage_idx,
                    obj_idx,
                    obj,
                    mx,
                    my,
                    parent_state,
                ) {
                    best = Some(ButtonHitCandidate {
                        button_no: owner.button_no,
                        sort_key,
                        runtime_slot: owner.runtime_slot,
                        se_no: owner.se_no,
                        was_hit: owner.was_hit,
                    });
                }
            }
        }
        let cur_parent_state =
            button_parent_render_state(layers, gfx, ids, stage_idx, obj_idx, obj, parent_state);
        for (child_idx, child) in obj.runtime.child_objects.iter_mut().enumerate() {
            if let Some(hit) = recurse(
                images,
                layers,
                gfx,
                ids,
                syscom,
                stage_idx,
                group_idx,
                mx,
                my,
                child_idx,
                child,
                Some(cur_parent_state),
                effective_owner,
            ) {
                merge_button_hit(&mut best, &mut tied, hit);
            }
        }
        if tied {
            None
        } else {
            best
        }
    }

    recurse(
        images,
        layers,
        gfx,
        ids,
        syscom,
        stage_idx,
        group_idx,
        mx,
        my,
        obj_idx,
        obj,
        parent_state,
        None,
    )
}

fn find_object_by_runtime_slot_mut(
    mut objects: &mut [globals::ObjectState],
    runtime_slot: usize,
) -> Option<&mut globals::ObjectState> {
    let mut idx = 0usize;
    while let Some((obj, tail)) = objects.split_first_mut() {
        if obj.runtime_slot_or(idx) == runtime_slot {
            return Some(obj);
        }
        if let Some(found) =
            find_object_by_runtime_slot_mut(&mut obj.runtime.child_objects, runtime_slot)
        {
            return Some(found);
        }
        objects = tail;
        idx += 1;
    }
    None
}

fn compose_clip_rect(
    parent_clip: Option<ClipRect>,
    child_clip: Option<ClipRect>,
) -> Option<ClipRect> {
    match (parent_clip, child_clip) {
        (Some(parent), Some(child)) => Some(ClipRect {
            // This intentionally mirrors tnm_add_parent_trp(). The original
            // combines TRP clip bounds with min(left/top) and max(right/bottom)
            // before converting to the final render parameter. It does not
            // transform the child rectangle by the parent matrix here.
            left: child.left.min(parent.left),
            top: child.top.min(parent.top),
            right: child.right.max(parent.right),
            bottom: child.bottom.max(parent.bottom),
        }),
        (Some(parent), None) => Some(parent),
        (None, Some(child)) => Some(child),
        (None, None) => None,
    }
}

fn compose_parent_render_state(
    parent: ParentRenderState,
    mut cur: ParentRenderState,
) -> ParentRenderState {
    if cur.world_no < 0 {
        cur.world_no = parent.world_no;
    }

    let child_clip = cur.dst_clip;

    cur.pos_x = (cur.pos_x - parent.center_rep_x) * parent.scale_x + parent.center_rep_x;
    cur.pos_y = (cur.pos_y - parent.center_rep_y) * parent.scale_y + parent.center_rep_y;
    cur.pos_z = (cur.pos_z - parent.center_rep_z) * parent.scale_z + parent.center_rep_z;
    {
        let tmp_x = cur.pos_x;
        let tmp_y = cur.pos_y;
        let (s, c) = parent.rotate_z.sin_cos();
        cur.pos_x = (tmp_x - parent.center_rep_x) * c - (tmp_y - parent.center_rep_y) * s
            + parent.center_rep_x;
        cur.pos_y = (tmp_x - parent.center_rep_x) * s
            + (tmp_y - parent.center_rep_y) * c
            + parent.center_rep_y;
    }
    cur.pos_x += parent.pos_x;
    cur.pos_y += parent.pos_y;
    cur.pos_z += parent.pos_z;
    cur.scale_x *= parent.scale_x;
    cur.scale_y *= parent.scale_y;
    cur.scale_z *= parent.scale_z;
    cur.rotate_x += parent.rotate_x;
    cur.rotate_y += parent.rotate_y;
    cur.rotate_z += parent.rotate_z;

    cur.dst_clip = compose_clip_rect(parent.dst_clip, child_clip);

    cur.tr = (cur.tr * parent.tr / 255).clamp(0, 255);
    cur.mono = combine_lerp(cur.mono as u8, parent.mono) as i32;
    cur.reverse = combine_lerp(cur.reverse as u8, parent.reverse) as i32;
    cur.bright = combine_lerp(cur.bright as u8, parent.bright) as i32;
    cur.dark = combine_lerp(cur.dark as u8, parent.dark) as i32;
    if cur.color_rate + parent.color_rate > 0 {
        let parent_rate = (parent.color_rate * 255 * 255)
            / (255 * 255 - (255 - cur.color_rate) * (255 - parent.color_rate)).max(1);
        cur.color_r = blend_color(cur.color_r as u8, parent.color_r, parent_rate) as i32;
        cur.color_g = blend_color(cur.color_g as u8, parent.color_g, parent_rate) as i32;
        cur.color_b = blend_color(cur.color_b as u8, parent.color_b, parent_rate) as i32;
    }
    cur.color_rate = combine_lerp(cur.color_rate as u8, parent.color_rate) as i32;
    cur.color_add_r = clamp_add(cur.color_add_r as u8, parent.color_add_r) as i32;
    cur.color_add_g = clamp_add(cur.color_add_g as u8, parent.color_add_g) as i32;
    cur.color_add_b = clamp_add(cur.color_add_b as u8, parent.color_add_b) as i32;
    if matches!(cur.blend, crate::layer::SpriteBlend::Normal) {
        cur.blend = parent.blend;
    }
    if cur.mask_image_id.is_none() {
        cur.mask_image_id = parent.mask_image_id;
        cur.mask_offset_x = parent.mask_offset_x;
        cur.mask_offset_y = parent.mask_offset_y;
    }
    if cur.tonecurve_image_id.is_none() {
        cur.tonecurve_image_id = parent.tonecurve_image_id;
        cur.tonecurve_row = parent.tonecurve_row;
        cur.tonecurve_sat = parent.tonecurve_sat;
    }
    cur
}

fn apply_object_event_animations_recursive(
    ids: &constants::RuntimeConstants,
    gfx: &mut graphics::GfxRuntime,
    images: &mut ImageManager,
    layers: &mut LayerManager,
    stage_i64: i64,
    obj_i64: i64,
    obj: &mut globals::ObjectState,
) {
    if obj.any_event_active() {
        let read_ev = |op_id: i32, obj: &globals::ObjectState| -> Option<i64> {
            if op_id == 0 {
                None
            } else {
                obj.int_event_by_op(ids, op_id)
                    .filter(|ev| ev.check_event())
                    .map(|ev| ev.get_total_value() as i64)
            }
        };
        let read_list0 = |op_id: i32, obj: &globals::ObjectState| -> Option<i64> {
            if op_id == 0 {
                None
            } else {
                obj.int_event_list_by_op(ids, op_id)
                    .and_then(|list| list.get(0))
                    .filter(|ev| ev.check_event())
                    .map(|ev| ev.get_total_value() as i64)
            }
        };

        let x: Option<i64> = read_ev(ids.obj_x_eve, obj);
        let y: Option<i64> = read_ev(ids.obj_y_eve, obj);
        let x_rep: Option<i64> = read_list0(ids.obj_x_rep_eve, obj);
        let y_rep: Option<i64> = read_list0(ids.obj_y_rep_eve, obj);
        let z_rep: Option<i64> = read_list0(ids.obj_z_rep_eve, obj);
        let alpha: Option<i64> = None;
        let patno: Option<i64> = read_ev(ids.obj_patno_eve, obj);
        let order: Option<i64> = None;
        let layer_no: Option<i64> = None;
        let z: Option<i64> = read_ev(ids.obj_z_eve, obj);
        let center_x: Option<i64> = read_ev(ids.obj_center_x_eve, obj);
        let center_y: Option<i64> = read_ev(ids.obj_center_y_eve, obj);
        let center_z: Option<i64> = read_ev(ids.obj_center_z_eve, obj);
        let center_rep_x: Option<i64> = read_ev(ids.obj_center_rep_x_eve, obj);
        let center_rep_y: Option<i64> = read_ev(ids.obj_center_rep_y_eve, obj);
        let center_rep_z: Option<i64> = read_ev(ids.obj_center_rep_z_eve, obj);
        let scale_x: Option<i64> = read_ev(ids.obj_scale_x_eve, obj);
        let scale_y: Option<i64> = read_ev(ids.obj_scale_y_eve, obj);
        let scale_z: Option<i64> = read_ev(ids.obj_scale_z_eve, obj);
        let rotate_x: Option<i64> = read_ev(ids.obj_rotate_x_eve, obj);
        let rotate_y: Option<i64> = read_ev(ids.obj_rotate_y_eve, obj);
        let rotate_z: Option<i64> = read_ev(ids.obj_rotate_z_eve, obj);
        let clip_left: Option<i64> = read_ev(ids.obj_clip_left_eve, obj);
        let clip_top: Option<i64> = read_ev(ids.obj_clip_top_eve, obj);
        let clip_right: Option<i64> = read_ev(ids.obj_clip_right_eve, obj);
        let clip_bottom: Option<i64> = read_ev(ids.obj_clip_bottom_eve, obj);
        let src_clip_left: Option<i64> = read_ev(ids.obj_src_clip_left_eve, obj);
        let src_clip_top: Option<i64> = read_ev(ids.obj_src_clip_top_eve, obj);
        let src_clip_right: Option<i64> = read_ev(ids.obj_src_clip_right_eve, obj);
        let src_clip_bottom: Option<i64> = read_ev(ids.obj_src_clip_bottom_eve, obj);
        let tr: Option<i64> = read_ev(ids.obj_tr_eve, obj);
        let tr_rep: Option<i64> = read_list0(ids.obj_tr_rep_eve, obj);
        let mono: Option<i64> = read_ev(ids.obj_mono_eve, obj);
        let reverse: Option<i64> = read_ev(ids.obj_reverse_eve, obj);
        let bright: Option<i64> = read_ev(ids.obj_bright_eve, obj);
        let dark: Option<i64> = read_ev(ids.obj_dark_eve, obj);
        let color_rate: Option<i64> = read_ev(ids.obj_color_rate_eve, obj);
        let color_add_r: Option<i64> = read_ev(ids.obj_color_add_r_eve, obj);
        let color_add_g: Option<i64> = read_ev(ids.obj_color_add_g_eve, obj);
        let color_add_b: Option<i64> = read_ev(ids.obj_color_add_b_eve, obj);
        let color_r: Option<i64> = read_ev(ids.obj_color_r_eve, obj);
        let color_g: Option<i64> = read_ev(ids.obj_color_g_eve, obj);
        let color_b: Option<i64> = read_ev(ids.obj_color_b_eve, obj);

        let mut set_extra_prop = |prop_id: i32, val: Option<i64>| {
            if prop_id != 0 {
                if let Some(v) = val {
                    let old_value = obj.get_int_prop(ids, prop_id);
                    trace_config_event_frame_prop_write(
                        ids,
                        stage_i64,
                        obj_i64,
                        obj,
                        prop_id,
                        old_value,
                        v,
                    );
                    obj.set_int_prop_from_event_frame(ids, prop_id, v);
                }
            }
        };
        set_extra_prop(ids.obj_x, x);
        set_extra_prop(ids.obj_y, y);
        // REP event lists are consumed directly by ObjectRenderInfo and hit testing.
        // Do not write animated totals back through obj_x_rep/obj_y_rep/obj_z_rep,
        // because those properties alias the same event-list storage.
        set_extra_prop(ids.obj_alpha, alpha);
        set_extra_prop(ids.obj_patno, patno);
        set_extra_prop(ids.obj_order, order);
        set_extra_prop(ids.obj_layer, layer_no);
        set_extra_prop(ids.obj_z, z);
        set_extra_prop(ids.obj_center_x, center_x);
        set_extra_prop(ids.obj_center_y, center_y);
        set_extra_prop(ids.obj_center_z, center_z);
        set_extra_prop(ids.obj_center_rep_x, center_rep_x);
        set_extra_prop(ids.obj_center_rep_y, center_rep_y);
        set_extra_prop(ids.obj_center_rep_z, center_rep_z);
        set_extra_prop(ids.obj_scale_x, scale_x);
        set_extra_prop(ids.obj_scale_y, scale_y);
        set_extra_prop(ids.obj_scale_z, scale_z);
        set_extra_prop(ids.obj_rotate_x, rotate_x);
        set_extra_prop(ids.obj_rotate_y, rotate_y);
        set_extra_prop(ids.obj_rotate_z, rotate_z);
        set_extra_prop(ids.obj_clip_left, clip_left);
        set_extra_prop(ids.obj_clip_top, clip_top);
        set_extra_prop(ids.obj_clip_right, clip_right);
        set_extra_prop(ids.obj_clip_bottom, clip_bottom);
        set_extra_prop(ids.obj_src_clip_left, src_clip_left);
        set_extra_prop(ids.obj_src_clip_top, src_clip_top);
        set_extra_prop(ids.obj_src_clip_right, src_clip_right);
        set_extra_prop(ids.obj_src_clip_bottom, src_clip_bottom);
        set_extra_prop(ids.obj_tr, tr);
        // obj_tr_rep also aliases prop_event_lists.tr_rep and must not be overwritten here.
        set_extra_prop(ids.obj_mono, mono);
        set_extra_prop(ids.obj_reverse, reverse);
        set_extra_prop(ids.obj_bright, bright);
        set_extra_prop(ids.obj_dark, dark);
        set_extra_prop(ids.obj_color_rate, color_rate);
        set_extra_prop(ids.obj_color_add_r, color_add_r);
        set_extra_prop(ids.obj_color_add_g, color_add_g);
        set_extra_prop(ids.obj_color_add_b, color_add_b);
        set_extra_prop(ids.obj_color_r, color_r);
        set_extra_prop(ids.obj_color_g, color_g);
        set_extra_prop(ids.obj_color_b, color_b);

        if !(x.is_none()
            && y.is_none()
            && x_rep.is_none()
            && y_rep.is_none()
            && z_rep.is_none()
            && alpha.is_none()
            && patno.is_none()
            && order.is_none()
            && layer_no.is_none()
            && z.is_none()
            && center_x.is_none()
            && center_y.is_none()
            && center_z.is_none()
            && center_rep_x.is_none()
            && center_rep_y.is_none()
            && center_rep_z.is_none()
            && scale_x.is_none()
            && scale_y.is_none()
            && scale_z.is_none()
            && rotate_x.is_none()
            && rotate_y.is_none()
            && rotate_z.is_none()
            && clip_left.is_none()
            && clip_top.is_none()
            && clip_right.is_none()
            && clip_bottom.is_none()
            && src_clip_left.is_none()
            && src_clip_top.is_none()
            && src_clip_right.is_none()
            && src_clip_bottom.is_none()
            && tr.is_none()
            && tr_rep.is_none()
            && mono.is_none()
            && reverse.is_none()
            && bright.is_none()
            && dark.is_none()
            && color_rate.is_none()
            && color_add_r.is_none()
            && color_add_g.is_none()
            && color_add_b.is_none()
            && color_r.is_none()
            && color_g.is_none()
            && color_b.is_none())
        {
            match &obj.backend {
                globals::ObjectBackend::Gfx => {
                    if let Some(ax) = x {
                        let _ = gfx.object_set_x(images, layers, stage_i64, obj_i64, ax);
                    }
                    if let Some(ay) = y {
                        let _ = gfx.object_set_y(images, layers, stage_i64, obj_i64, ay);
                    }
                    if let Some(a) = alpha {
                        let _ = gfx.object_set_alpha(images, layers, stage_i64, obj_i64, a);
                    }
                    if let Some(p) = patno {
                        let _ = gfx.object_set_pat_no(images, layers, stage_i64, obj_i64, p);
                    }
                    if let Some(o) = order {
                        let _ = gfx.object_set_order(images, layers, stage_i64, obj_i64, o);
                    }
                    if let Some(l) = layer_no {
                        let _ = gfx.object_set_layer(images, layers, stage_i64, obj_i64, l);
                    }
                    if let Some(zv) = z {
                        let _ = gfx.object_set_z(stage_i64, obj_i64, zv);
                    }
                    if center_x.is_some() || center_y.is_some() {
                        let cx = center_x
                            .or_else(|| {
                                (ids.obj_center_x != 0)
                                    .then_some(obj.get_int_prop(ids, ids.obj_center_x))
                            })
                            .unwrap_or(0);
                        let cy = center_y
                            .or_else(|| {
                                (ids.obj_center_y != 0)
                                    .then_some(obj.get_int_prop(ids, ids.obj_center_y))
                            })
                            .unwrap_or(0);
                        let _ = gfx.object_set_center(images, layers, stage_i64, obj_i64, cx, cy);
                    }
                    if scale_x.is_some() || scale_y.is_some() {
                        let sx = scale_x
                            .or_else(|| {
                                (ids.obj_scale_x != 0)
                                    .then_some(obj.get_int_prop(ids, ids.obj_scale_x))
                            })
                            .unwrap_or(1000);
                        let sy = scale_y
                            .or_else(|| {
                                (ids.obj_scale_y != 0)
                                    .then_some(obj.get_int_prop(ids, ids.obj_scale_y))
                            })
                            .unwrap_or(1000);
                        let _ = gfx.object_set_scale(images, layers, stage_i64, obj_i64, sx, sy);
                    }
                    if let Some(rz) = rotate_z {
                        let _ = gfx.object_set_rotate(images, layers, stage_i64, obj_i64, rz);
                    }
                    if clip_left.is_some()
                        || clip_top.is_some()
                        || clip_right.is_some()
                        || clip_bottom.is_some()
                    {
                        let use_flag = if ids.obj_clip_use != 0 {
                            obj.get_int_prop(ids, ids.obj_clip_use)
                        } else {
                            0
                        };
                        let left = clip_left
                            .or_else(|| {
                                (ids.obj_clip_left != 0)
                                    .then_some(obj.get_int_prop(ids, ids.obj_clip_left))
                            })
                            .unwrap_or(0);
                        let top = clip_top
                            .or_else(|| {
                                (ids.obj_clip_top != 0)
                                    .then_some(obj.get_int_prop(ids, ids.obj_clip_top))
                            })
                            .unwrap_or(0);
                        let right = clip_right
                            .or_else(|| {
                                (ids.obj_clip_right != 0)
                                    .then_some(obj.get_int_prop(ids, ids.obj_clip_right))
                            })
                            .unwrap_or(0);
                        let bottom = clip_bottom
                            .or_else(|| {
                                (ids.obj_clip_bottom != 0)
                                    .then_some(obj.get_int_prop(ids, ids.obj_clip_bottom))
                            })
                            .unwrap_or(0);
                        let _ = gfx.object_set_clip(
                            images, layers, stage_i64, obj_i64, use_flag, left, top, right, bottom,
                        );
                    }
                    if src_clip_left.is_some()
                        || src_clip_top.is_some()
                        || src_clip_right.is_some()
                        || src_clip_bottom.is_some()
                    {
                        let use_flag = if ids.obj_src_clip_use != 0 {
                            obj.lookup_int_prop(ids, ids.obj_src_clip_use).unwrap_or(0)
                        } else {
                            0
                        };
                        let left = src_clip_left
                            .or_else(|| {
                                if ids.obj_src_clip_left != 0 {
                                    obj.lookup_int_prop(ids, ids.obj_src_clip_left)
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0);
                        let top = src_clip_top
                            .or_else(|| {
                                if ids.obj_src_clip_top != 0 {
                                    obj.lookup_int_prop(ids, ids.obj_src_clip_top)
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0);
                        let right = src_clip_right
                            .or_else(|| {
                                if ids.obj_src_clip_right != 0 {
                                    obj.lookup_int_prop(ids, ids.obj_src_clip_right)
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0);
                        let bottom = src_clip_bottom
                            .or_else(|| {
                                if ids.obj_src_clip_bottom != 0 {
                                    obj.lookup_int_prop(ids, ids.obj_src_clip_bottom)
                                } else {
                                    None
                                }
                            })
                            .unwrap_or(0);
                        let _ = gfx.object_set_src_clip(
                            images, layers, stage_i64, obj_i64, use_flag, left, top, right, bottom,
                        );
                    }
                    if let Some(v) = tr {
                        let _ = gfx.object_set_tr(images, layers, stage_i64, obj_i64, v);
                    }
                    if let Some(v) = mono {
                        let _ = gfx.object_set_mono(images, layers, stage_i64, obj_i64, v);
                    }
                    if let Some(v) = reverse {
                        let _ = gfx.object_set_reverse(images, layers, stage_i64, obj_i64, v);
                    }
                    if let Some(v) = bright {
                        let _ = gfx.object_set_bright(images, layers, stage_i64, obj_i64, v);
                    }
                    if let Some(v) = dark {
                        let _ = gfx.object_set_dark(images, layers, stage_i64, obj_i64, v);
                    }
                    if let Some(v) = color_rate {
                        let _ = gfx.object_set_color_rate(images, layers, stage_i64, obj_i64, v);
                    }
                    if color_add_r.is_some() || color_add_g.is_some() || color_add_b.is_some() {
                        let r = color_add_r.unwrap_or_else(|| {
                            if ids.obj_color_add_r != 0 {
                                obj.get_int_prop(ids, ids.obj_color_add_r)
                            } else {
                                0
                            }
                        });
                        let g = color_add_g.unwrap_or_else(|| {
                            if ids.obj_color_add_g != 0 {
                                obj.get_int_prop(ids, ids.obj_color_add_g)
                            } else {
                                0
                            }
                        });
                        let b = color_add_b.unwrap_or_else(|| {
                            if ids.obj_color_add_b != 0 {
                                obj.get_int_prop(ids, ids.obj_color_add_b)
                            } else {
                                0
                            }
                        });
                        let _ =
                            gfx.object_set_color_add(images, layers, stage_i64, obj_i64, r, g, b);
                    }
                    if color_r.is_some() || color_g.is_some() || color_b.is_some() {
                        let r = color_r.unwrap_or_else(|| {
                            if ids.obj_color_r != 0 {
                                obj.get_int_prop(ids, ids.obj_color_r)
                            } else {
                                0
                            }
                        });
                        let g = color_g.unwrap_or_else(|| {
                            if ids.obj_color_g != 0 {
                                obj.get_int_prop(ids, ids.obj_color_g)
                            } else {
                                0
                            }
                        });
                        let b = color_b.unwrap_or_else(|| {
                            if ids.obj_color_b != 0 {
                                obj.get_int_prop(ids, ids.obj_color_b)
                            } else {
                                0
                            }
                        });
                        let _ = gfx.object_set_color(images, layers, stage_i64, obj_i64, r, g, b);
                    }
                }
                backend => {
                    for (layer_id, sprite_id) in layer_backed_object_sprite_bindings(backend) {
                        let Some(sprite) = layers
                            .layer_mut(layer_id)
                            .and_then(|layer| layer.sprite_mut(sprite_id))
                        else {
                            continue;
                        };
                        if let Some(ax) = x {
                            sprite.x = ax as i32;
                        }
                        if let Some(ay) = y {
                            sprite.y = ay as i32;
                        }
                        if let Some(v) = alpha {
                            sprite.alpha = v.clamp(0, 255) as u8;
                        }
                        if let Some(v) = order {
                            sprite.order = v as i32;
                        }
                        if let Some(v) = tr {
                            sprite.tr = v.clamp(0, 255) as u8;
                        }
                    }
                }
            }
        }
    }

    for (child_idx, child) in obj.runtime.child_objects.iter_mut().enumerate() {
        apply_object_event_animations_recursive(
            ids,
            gfx,
            images,
            layers,
            stage_i64,
            object_runtime_slot(child_idx, child) as i64,
            child,
        );
    }
}

const WEATHER_APPEAR_MS: i64 = 1000;
const WEATHER_DISAPPEAR_MS: i64 = 1000;
const WEATHER_ANGLE_FULL: f64 = 3600.0;

fn weather_alpha_for_state(state: i64, cur: i64, len: i64) -> u8 {
    match state {
        1 => ((cur.clamp(0, WEATHER_APPEAR_MS) * 255) / WEATHER_APPEAR_MS).clamp(0, 255) as u8,
        2 => 255,
        3 => {
            let len = if len <= 0 { WEATHER_DISAPPEAR_MS } else { len };
            ((len.saturating_sub(cur).clamp(0, len) * 255) / len).clamp(0, 255) as u8
        }
        _ => 0,
    }
}

fn weather_wave(time: i64, period: i64, power: i64) -> i64 {
    if period == 0 || power == 0 {
        return 0;
    }
    let rad = (time as f64 / period.abs() as f64) * std::f64::consts::TAU;
    (rad.sin() * power as f64).round() as i64
}

fn weather_pattern(obj: &mut globals::ObjectState, idx: usize) -> i64 {
    let p = obj.weather_param.clone();
    let first = p.pat_no_00.min(p.pat_no_01);
    let last = p.pat_no_00.max(p.pat_no_01);
    let span = (last - first + 1).max(1);
    match p.pat_mode {
        1 if p.pat_time > 0 => {
            let t = obj
                .weather_work
                .sub
                .get(idx)
                .map(|s| s.move_cur_time.max(0) % p.pat_time)
                .unwrap_or(0);
            first + t.saturating_mul(span) / p.pat_time
        }
        2 => first + obj.weather_work.rand_mod(span),
        _ => p.pat_no_00,
    }
}

fn ensure_weather_sprites(
    layers: &mut LayerManager,
    obj: &mut globals::ObjectState,
) -> Option<(LayerId, Vec<SpriteId>)> {
    let required = obj.weather_sprite_count();
    let (layer_id, sprite_ids) = match &mut obj.backend {
        globals::ObjectBackend::Weather {
            layer_id,
            sprite_ids,
        } => (*layer_id, sprite_ids),
        _ => return None,
    };
    if let Some(layer) = layers.layer_mut(layer_id) {
        while sprite_ids.len() < required {
            let sid = layer.create_sprite();
            if let Some(sprite) = layer.sprite_mut(sid) {
                sprite.fit = SpriteFit::PixelRect;
                sprite.size_mode = SpriteSizeMode::Intrinsic;
                sprite.visible = false;
                sprite.image_id = None;
            }
            sprite_ids.push(sid);
        }
    }
    Some((layer_id, sprite_ids.clone()))
}

fn set_weather_sprite(
    ids: &constants::RuntimeConstants,
    layers: &mut LayerManager,
    _images: &mut ImageManager,
    obj: &globals::ObjectState,
    layer_id: LayerId,
    sprite_id: SpriteId,
    image_id: Option<ImageId>,
    x: i64,
    y: i64,
    alpha: u8,
    scale_x: i64,
    scale_y: i64,
) {
    let Some(layer) = layers.layer_mut(layer_id) else {
        return;
    };
    let Some(sprite) = layer.sprite_mut(sprite_id) else {
        return;
    };
    sprite.image_id = image_id;
    sprite.visible = image_id.is_some() && obj.get_int_prop(ids, ids.obj_disp) != 0 && alpha > 0;
    sprite.fit = SpriteFit::PixelRect;
    sprite.size_mode = SpriteSizeMode::Intrinsic;
    // Weather backing sprites store particle-local state.  The object-tree
    // collector applies OBJECT.X/Y/scale/TR and parent transforms exactly once.
    sprite.x = x.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    sprite.y = y.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    sprite.alpha = 255;
    sprite.tr = alpha;
    sprite.order = 0;
    sprite.scale_x = scale_x as f32 / 1000.0;
    sprite.scale_y = scale_y as f32 / 1000.0;
}

fn weather_wrap_position(value: i64, extent: i64) -> i64 {
    if extent <= 0 {
        return value;
    }
    // Match the source's `x > 0 ? x % extent : extent - ((-x) % extent)`.
    if value > 0 {
        value % extent
    } else {
        extent - ((-value) % extent)
    }
}

fn sync_weather_object_recursive(
    ids: &constants::RuntimeConstants,
    layers: &mut LayerManager,
    images: &mut ImageManager,
    screen_w: i64,
    screen_h: i64,
    game_delta_ms: i32,
    real_delta_ms: i32,
    obj: &mut globals::ObjectState,
) {
    if obj.used && obj.object_type == 4 && matches!(obj.weather_param.weather_type, 1 | 2) {
        obj.update_weather_time(game_delta_ms, real_delta_ms, screen_w, screen_h);
        let Some((layer_id, sprite_ids)) = ensure_weather_sprites(layers, obj) else {
            return;
        };

        let file_name = obj.file_name.clone().unwrap_or_default();
        let cnt_max = obj.weather_work.cnt_max.min(obj.weather_work.sub.len());
        let mut used = 0usize;
        for idx in 0..cnt_max {
            let sub = obj.weather_work.sub[idx].clone();
            if sub.state == 0 {
                continue;
            }
            let pat_no = weather_pattern(obj, idx).max(0) as u32;
            let image_id = if file_name.is_empty() {
                None
            } else {
                images.load_g00(&file_name, pat_no).ok()
            };
            let alpha = weather_alpha_for_state(sub.state, sub.state_cur_time, sub.state_time_len);

            if obj.weather_param.weather_type == 1 {
                let move_x = if sub.move_time_x == 0 {
                    0
                } else {
                    (1000.0 / sub.move_time_x as f64 * sub.move_cur_time.max(0) as f64) as i64
                };
                let move_y = if sub.move_time_y == 0 {
                    0
                } else {
                    (1000.0 / sub.move_time_y as f64 * sub.move_cur_time.max(0) as f64) as i64
                };
                let mut x = sub.move_start_pos_x
                    + move_x
                    + weather_wave(sub.sin_cur_time, sub.sin_time_x, sub.sin_power_x);
                let mut y = sub.move_start_pos_y
                    + move_y
                    + weather_wave(sub.sin_cur_time, sub.sin_time_y, sub.sin_power_y);
                x = weather_wrap_position(x, screen_w);
                y = weather_wrap_position(y, screen_h);

                let (over_l, over_r, over_u, over_d) = image_id
                    .and_then(|id| images.get(id))
                    .map(|img| {
                        let sx = sub.scale_x as f64 / 1000.0;
                        let sy = sub.scale_y as f64 / 1000.0;
                        let left = x as f64 - img.center_x as f64 * sx;
                        let right = x as f64 + (img.width as i64 - img.center_x as i64) as f64 * sx;
                        let top = y as f64 - img.center_y as f64 * sy;
                        let bottom = y as f64 + (img.height as i64 - img.center_y as i64) as f64 * sy;
                        (left < 0.0, right >= screen_w as f64, top < 0.0, bottom >= screen_h as f64)
                    })
                    .unwrap_or((false, false, false, false));
                let wrap_x = if over_l {
                    screen_w
                } else if over_r {
                    -screen_w
                } else {
                    0
                };
                let wrap_y = if over_u {
                    screen_h
                } else if over_d {
                    -screen_h
                } else {
                    0
                };
                let offsets = [
                    Some((0, 0)),
                    (over_l || over_r).then_some((wrap_x, 0)),
                    (over_u || over_d).then_some((0, wrap_y)),
                    ((over_l || over_r) && (over_u || over_d)).then_some((wrap_x, wrap_y)),
                ];
                for offset in offsets.into_iter().flatten() {
                    if let Some(&sid) = sprite_ids.get(used) {
                        set_weather_sprite(
                            ids,
                            layers,
                            images,
                            obj,
                            layer_id,
                            sid,
                            image_id,
                            x + offset.0,
                            y + offset.1,
                            alpha,
                            sub.scale_x,
                            sub.scale_y,
                        );
                    }
                    used += 1;
                }
            } else {
                let total_time = (WEATHER_APPEAR_MS
                    + sub.active_time_len
                    + WEATHER_DISAPPEAR_MS)
                    .max(1);
                let move_cur_time = if sub.move_time_x > 0 {
                    sub.move_cur_time.max(0)
                } else {
                    total_time.saturating_sub(sub.move_cur_time.max(0))
                };
                let mut rep_x = sub.move_start_distance as f64;
                let mut rep_y = 0.0f64;
                if sub.move_time_x != 0 {
                    let mt = sub.move_time_x as f64;
                    let t = move_cur_time as f64;
                    rep_x += 1000.0 / mt / mt * t * t;
                }
                // The source intentionally applies sin_time_x to the orthogonal
                // axis and sin_time_y to the radial axis.
                rep_y += weather_wave(sub.sin_cur_time, sub.sin_time_x, sub.sin_power_x) as f64;
                rep_x += weather_wave(sub.sin_cur_time, sub.sin_time_y, sub.sin_power_y) as f64;

                let mut rad = (sub.move_start_degree as f64 / 10.0).to_radians();
                let theta_deg = rep_x * (sub.center_rotate as f64 / 10.0) / 1000.0;
                rad += theta_deg.to_radians();
                let x = obj.weather_param.center_x
                    + (rep_x * rad.cos() - rep_y * rad.sin()) as i64;
                let y = obj.weather_param.center_y
                    + (rep_x * rad.sin() + rep_y * rad.cos()) as i64;
                let process = (move_cur_time.saturating_mul(1000) / total_time).clamp(0, 1000);
                let zoom = sub.zoom_min
                    + (sub.zoom_max - sub.zoom_min).saturating_mul(process) / 1000;
                if let Some(&sid) = sprite_ids.get(used) {
                    set_weather_sprite(
                        ids,
                        layers,
                        images,
                        obj,
                        layer_id,
                        sid,
                        image_id,
                        x,
                        y,
                        alpha,
                        zoom,
                        zoom,
                    );
                }
                used += 1;
            }
        }

        if let Some(layer) = layers.layer_mut(layer_id) {
            for sid in sprite_ids.into_iter().skip(used) {
                if let Some(sprite) = layer.sprite_mut(sid) {
                    sprite.visible = false;
                    sprite.image_id = None;
                }
            }
        }
    }

    for child in &mut obj.runtime.child_objects {
        sync_weather_object_recursive(
            ids,
            layers,
            images,
            screen_w,
            screen_h,
            game_delta_ms,
            real_delta_ms,
            child,
        );
    }
}

fn install_object_movie_preview_if_missing(
    layers: &mut LayerManager,
    movie_mgr: &mut MovieManager,
    images: &mut ImageManager,
    obj: &mut globals::ObjectState,
    stage_idx: i64,
    obj_idx: i64,
    file: &str,
    trace: bool,
) {
    let globals::ObjectBackend::Movie {
        layer_id,
        sprite_id,
        image_id,
        width,
        height,
    } = &mut obj.backend
    else {
        return;
    };

    if image_id.is_some() {
        return;
    }

    match movie_mgr.ensure_omv_preview_frame(file) {
        Ok(frame) => {
            let img_id = images.insert_image_arc(frame.clone());
            *image_id = Some(img_id);
            *width = frame.width;
            *height = frame.height;
            obj.movie.frame_image_ids[0] = Some(img_id);
            obj.movie.frame_image_cursor = 0;
            if let Some(layer) = layers.layer_mut(*layer_id) {
                if let Some(sprite) = layer.sprite_mut(*sprite_id) {
                    sprite.image_id = Some(img_id);
                    sprite.object_anchor = true;
                    sprite.texture_center_x = 0.0;
                    sprite.texture_center_y = 0.0;
                }
            }
            if trace || sg_debug_enabled() {
                eprintln!(
                    "[SG_DEBUG][MOV] object_movie.preview_installed stage={} obj={} file={} image={:?} size={}x{}",
                    stage_idx, obj_idx, file, img_id, frame.width, frame.height
                );
            }
        }
        Err(err) => {
            if trace || sg_debug_enabled() {
                eprintln!(
                    "[SG_DEBUG][MOV] object_movie.preview_failed stage={} obj={} file={} err={:#}",
                    stage_idx, obj_idx, file, err
                );
            }
        }
    }
}

fn install_object_movie_stream_frame(
    layers: &mut LayerManager,
    images: &mut ImageManager,
    obj: &mut globals::ObjectState,
    stage_idx: i64,
    obj_idx: i64,
    file: &str,
    frame_idx: usize,
    frame: std::sync::Arc<crate::assets::RgbaImage>,
    trace: bool,
) {
    let globals::ObjectBackend::Movie {
        layer_id,
        sprite_id,
        image_id,
        width,
        height,
    } = &mut obj.backend
    else {
        return;
    };

    // C_elm_object::restruct_movie() creates one D3DUSAGE_DYNAMIC texture and
    // C_elm_object::movie_frame() updates that same texture with
    // D3DLOCK_DISCARD. Keep the ImageId stable for the entire OBJECT movie
    // lifetime so the renderer can update one GPU texture in place without
    // invalidating/rebuilding the sprite bind group on every decoded frame.
    let img_id = if let Some(id) = obj.movie.frame_image_ids[0] {
        let _ = images.replace_image_arc(id, frame.clone());
        id
    } else {
        let id = images.insert_image_arc(frame.clone());
        obj.movie.frame_image_ids[0] = Some(id);
        id
    };
    obj.movie.frame_image_cursor = 0;

    *image_id = Some(img_id);
    *width = frame.width;
    *height = frame.height;
    if let Some(layer) = layers.layer_mut(*layer_id) {
        if let Some(sprite) = layer.sprite_mut(*sprite_id) {
            sprite.image_id = Some(img_id);
            sprite.object_anchor = true;
            sprite.texture_center_x = 0.0;
            sprite.texture_center_y = 0.0;
        }
    }
    if trace || sg_debug_enabled() {
        eprintln!(
            "[SG_DEBUG][MOV] object_movie.frame stage={} obj={} file={} frame={} image={:?} size={}x{} timer_ms={}",
            stage_idx, obj_idx, file, frame_idx, img_id, frame.width, frame.height, obj.movie.timer_ms
        );
    }
}

fn sync_emote_object_recursive(
    layers: &mut LayerManager,
    obj: &mut globals::ObjectState,
    mouth_stop: bool,
    koe_playing: bool,
    koe_ex: bool,
    koe_chara_no: i64,
    live_mouth: f32,
) {
    if obj.used && obj.object_type == 12 {
        let fallback = obj.emote.koe_mouth_volume as f32 / 1000.0;
        let mouth = if !mouth_stop
            && obj.emote.koe_chara_no >= 0
            && obj.emote.koe_chara_no == koe_chara_no
            && !koe_ex
            && koe_playing
        {
            live_mouth
        } else {
            fallback
        };
        if let Some(runtime) = obj.emote.runtime.as_mut() {
            if let Err(err) = runtime.set_face_talk(mouth) {
                log::error!("Emote face_talk update failed: {err:#}");
            }
        }
        if let globals::ObjectBackend::Rect { layer_id, sprite_id, width, height } = obj.backend {
            if let Some(sprite) = layers.layer_mut(layer_id).and_then(|layer| layer.sprite_mut(sprite_id)) {
                sprite.emote_render = obj.emote.runtime.as_ref().map(|runtime| {
                    runtime.packet(obj.emote.width, obj.emote.height, obj.emote.rep_x, obj.emote.rep_y)
                });
                sprite.size_mode = SpriteSizeMode::Explicit { width, height };
                sprite.alpha_test = true;
                sprite.alpha_blend = true;
            }
        }
    }
    for child in &mut obj.runtime.child_objects {
        sync_emote_object_recursive(
            layers, child, mouth_stop, koe_playing, koe_ex, koe_chara_no, live_mouth,
        );
    }
}

fn sync_movie_object_recursive(
    ids: &constants::RuntimeConstants,
    layers: &mut LayerManager,
    movie_mgr: &mut MovieManager,
    audio: &mut AudioHub,
    gfx: &mut graphics::GfxRuntime,
    images: &mut ImageManager,
    stage_idx: i64,
    obj_idx: i64,
    obj: &mut globals::ObjectState,
    decoded_any: &mut bool,
) {
    let trace = trace_env().sg_movie_trace;
    if obj.used && obj.object_type == 9 {
        if let Some(file_name) = obj.file_name.clone() {
            if trace {
                eprintln!("[SG_MOVIE_TRACE] enter stage={} obj={} file={} playing={} pause={} backend={:?} children={}", stage_idx, obj_idx, file_name, obj.movie.playing, obj.movie.pause_flag, obj.backend, obj.runtime.child_objects.len());
            }
            let file = file_name.as_str();
            if obj.movie.just_finished {
                if let Some(id) = obj.movie.audio_id.take() {
                    movie_mgr.stop_audio(id);
                }
                obj.movie.just_finished = false;
                if obj.movie.auto_free_flag {
                    // C_elm_object::update_movie calls init_type(true) at EOS when
                    // auto-free is enabled, regardless of whether a visible frame was
                    // installed. Decoder failure must not leave a dead movie object.
                    if let globals::ObjectBackend::Movie {
                        layer_id,
                        sprite_id,
                        ..
                    } = obj.backend
                    {
                        if let Some(layer) = layers.layer_mut(layer_id) {
                            if let Some(sprite) = layer.sprite_mut(sprite_id) {
                                sprite.visible = false;
                                sprite.image_id = None;
                            }
                        }
                    }
                    obj.init_type_like();
                }
            } else if !obj.movie.playing {
                if let Some(id) = obj.movie.audio_id.take() {
                    movie_mgr.stop_audio(id);
                }
            }

            if obj.object_type == 9 {
                let (layer_id, sprite_id) = if let globals::ObjectBackend::Movie {
                    layer_id,
                    sprite_id,
                    ..
                } = &obj.backend
                {
                    (*layer_id, *sprite_id)
                } else {
                    let Some(layer_id) = gfx.ensure_stage_layer_id(layers, stage_idx) else {
                        return;
                    };
                    let Some(layer) = layers.layer_mut(layer_id) else {
                        return;
                    };
                    let sid = layer.create_sprite();
                    if let Some(sprite) = layer.sprite_mut(sid) {
                        sprite.visible = true;
                        sprite.alpha = 255;
                        sprite.fit = SpriteFit::PixelRect;
                        sprite.size_mode = SpriteSizeMode::Intrinsic;
                        sprite.object_anchor = true;
                        sprite.texture_center_x = 0.0;
                        sprite.texture_center_y = 0.0;
                        sprite.x = 0;
                        sprite.y = 0;
                        sprite.order = 0;
                    }
                    obj.backend = globals::ObjectBackend::Movie {
                        layer_id,
                        sprite_id: sid,
                        image_id: None,
                        width: 0,
                        height: 0,
                    };
                    (layer_id, sid)
                };

                if let Some(layer) = layers.layer_mut(layer_id) {
                    if let Some(sprite) = layer.sprite_mut(sprite_id) {
                        let render_info =
                            button_object_render_info(ids, gfx, stage_idx, obj_idx as usize, obj);
                        apply_button_object_render_info_to_sprite(sprite, &render_info);
                        finalize_button_object_center_rep_to_sprite(sprite, &render_info);
                        if ids.obj_alpha != 0 {
                            sprite.alpha = obj
                                .lookup_int_prop(ids, ids.obj_alpha)
                                .unwrap_or(255)
                                .clamp(0, 255) as u8;
                        }
                        if ids.obj_order != 0 {
                            sprite.order =
                                obj.lookup_int_prop(ids, ids.obj_order).unwrap_or(0) as i32;
                        }
                        sprite.blend = crate::layer::SpriteBlend::from_i64(
                            obj.lookup_int_prop(ids, ids.obj_blend).unwrap_or(0),
                        );
                        // C++ OBJECT movie uses movie_frame() and trp_to_rp(), so OBJECT.CENTER
                        // must shift the OMV quad just like a texture object. The dynamic OMV
                        // texture itself keeps the C_d3d_texture default center of (0, 0).
                        sprite.object_anchor = true;
                        sprite.texture_center_x = 0.0;
                        sprite.texture_center_y = 0.0;
                    }
                }

                // Object movie sprites need a texture immediately after CREATE_MOVIE.
                // The streaming decoder can return None while its worker is warming up;
                // without this preview surface the object stays as image_id=None/0x0 and
                // is filtered out by render submission. The stream path below replaces it.
                install_object_movie_preview_if_missing(
                    layers, movie_mgr, images, obj, stage_idx, obj_idx, file, trace,
                );

                if obj.movie.seeked {
                    if let Some(id) = obj.movie.audio_id.take() {
                        movie_mgr.stop_audio(id);
                    }
                }
                obj.movie.seeked = false;
                obj.movie.just_looped = false;

                if let Some(id) = obj.movie.audio_id {
                    if let Some(position_ms) = movie_mgr.audio_playback_position_ms(id) {
                        obj.movie.timer_ms = position_ms;
                    }
                    if movie_mgr.audio_playback_finished(id) {
                        obj.movie.audio_id = None;
                        if !obj.movie.loop_flag {
                            if let Some(total_ms) = obj.movie.total_ms {
                                obj.movie.timer_ms = total_ms;
                            }
                            obj.movie.playing = false;
                            obj.movie.just_finished = true;
                        }
                    }
                }

                // Pause/resume is edge-triggered by the OBJECT commands in
                // forms/stage.rs. Reissuing Kira resume every native tick can
                // continuously restart its transition state and produce choppy
                // movie audio even while video rendering remains smooth.

                if obj.movie.pause_flag {
                    if let globals::ObjectBackend::Movie {
                        layer_id,
                        sprite_id,
                        image_id,
                        width,
                        height,
                    } = &mut obj.backend
                    {
                        if image_id.is_none() {
                            match movie_mgr.ensure_preview_frame(file) {
                                Ok(frame) => {
                                    let img_id = images.insert_image_arc(frame.clone());
                                    *image_id = Some(img_id);
                                    *width = frame.width;
                                    *height = frame.height;
                                    if let Some(layer) = layers.layer_mut(*layer_id) {
                                        if let Some(sprite) = layer.sprite_mut(*sprite_id) {
                                            sprite.image_id = Some(img_id);
                                            sprite.object_anchor = true;
                                            sprite.texture_center_x = 0.0;
                                            sprite.texture_center_y = 0.0;
                                        }
                                    }
                                    if trace {
                                        eprintln!(
                                            "[SG_MOVIE_TRACE] installed paused preview stage={} obj={} file={} size={}x{}",
                                            stage_idx,
                                            obj_idx,
                                            file,
                                            frame.width,
                                            frame.height,
                                        );
                                    }
                                }
                                Err(err) => {
                                    if trace {
                                        eprintln!(
                                            "[SG_MOVIE_TRACE] paused preview decode failed stage={} obj={} file={} err={:#}",
                                            stage_idx,
                                            obj_idx,
                                            file,
                                            err,
                                        );
                                    }
                                }
                            }
                        }
                    }
                    for (child_idx, child) in obj.runtime.child_objects.iter_mut().enumerate() {
                        sync_movie_object_recursive(
                            ids,
                            layers,
                            movie_mgr,
                            audio,
                            gfx,
                            images,
                            stage_idx,
                            object_runtime_slot(child_idx, child) as i64,
                            child,
                            decoded_any,
                        );
                    }
                    return;
                }

                if trace {
                    eprintln!(
                        "[SG_MOVIE_TRACE] poll_stream stage={} obj={} file={}",
                        stage_idx, obj_idx, file
                    );
                }
                let polled = match movie_mgr.poll_global_movie_frame_with_loop(
                    file,
                    obj.movie.timer_ms,
                    obj.movie.loop_flag,
                ) {
                    Ok(Some(frame)) => frame,
                    Ok(None) => {
                        if obj.movie.last_frame_idx.is_none() {
                            obj.movie.timer_ms = 0;
                            obj.movie.last_tick = Some(crate::platform_time::Instant::now());
                        }
                        for (child_idx, child) in obj.runtime.child_objects.iter_mut().enumerate() {
                            sync_movie_object_recursive(
                                ids,
                                layers,
                                movie_mgr,
                                audio,
                                gfx,
                                images,
                                stage_idx,
                                object_runtime_slot(child_idx, child) as i64,
                                child,
                                decoded_any,
                            );
                        }
                        return;
                    }
                    Err(err) => {
                        eprintln!(
                            "[SG_MOVIE] object movie error stage={} obj={} file={}: {:#}",
                            stage_idx, obj_idx, file, err
                        );
                        obj.movie.playing = false;
                        for (child_idx, child) in obj.runtime.child_objects.iter_mut().enumerate() {
                            sync_movie_object_recursive(
                                ids,
                                layers,
                                movie_mgr,
                                audio,
                                gfx,
                                images,
                                stage_idx,
                                object_runtime_slot(child_idx, child) as i64,
                                child,
                                decoded_any,
                            );
                        }
                        return;
                    }
                };
                if obj.movie.total_ms.is_none() || polled.total_ms.is_some() {
                    obj.movie.total_ms = polled.total_ms.or(obj.movie.total_ms);
                }
                let frame_idx = polled.frame_idx;
                if obj.movie.last_frame_idx != Some(frame_idx) {
                    obj.movie.last_frame_idx = Some(frame_idx);
                    let frame = polled.frame.clone();
                    install_object_movie_stream_frame(
                        layers, images, obj, stage_idx, obj_idx, file, frame_idx, frame, trace,
                    );
                }
                let waiting_for_movie_audio_start =
                    obj.movie.audio_id.is_none() && polled.audio.is_none() && !polled.audio_ready;
                if obj.movie.playing && obj.movie.audio_id.is_none() {
                    if let Some(track) = polled.audio.as_ref() {
                        if let Ok(id) = movie_mgr.start_audio(audio, track, obj.movie.timer_ms, obj.movie.loop_flag) {
                            obj.movie.audio_id = Some(id);
                            obj.movie.audio_started_once = true;
                        }
                    }
                }
                if waiting_for_movie_audio_start
                    && obj.movie.audio_id.is_none()
                    && !obj.movie.audio_started_once
                {
                    obj.movie.timer_ms = 0;
                    obj.movie.last_tick = Some(crate::platform_time::Instant::now());
                }
            }
        }
    }

    for (child_idx, child) in obj.runtime.child_objects.iter_mut().enumerate() {
        sync_movie_object_recursive(
            ids,
            layers,
            movie_mgr,
            audio,
            gfx,
            images,
            stage_idx,
            object_runtime_slot(child_idx, child) as i64,
            child,
            decoded_any,
        );
    }
}

fn apply_object_masks_recursive(
    ids: &constants::RuntimeConstants,
    gfx: &mut graphics::GfxRuntime,
    layers: &mut LayerManager,
    stage_i64: i64,
    obj_i64: i64,
    obj: &mut globals::ObjectState,
    mask_info: &[Option<(String, i32, i32)>],
    resolved_masks: &HashMap<String, ImageId>,
) {
    let mask_no = if ids.obj_mask_no != 0 {
        obj.lookup_int_prop(ids, ids.obj_mask_no).unwrap_or(-1)
    } else {
        -1
    };
    let mask_binding = usize::try_from(mask_no)
        .ok()
        .and_then(|mask_idx| mask_info.get(mask_idx))
        .and_then(|entry| entry.as_ref())
        .and_then(|(mask_name, mask_x, mask_y)| {
            resolved_masks
                .get(mask_name)
                .copied()
                .map(|mask_image_id| (mask_image_id, *mask_x, *mask_y))
        });

    let targets: Vec<(LayerId, SpriteId)> = match &obj.backend {
        globals::ObjectBackend::Gfx => gfx
            .object_sprite_binding(stage_i64, obj_i64)
            .into_iter()
            .collect(),
        backend => layer_backed_object_sprite_bindings(backend),
    };

    for (layer_id, sprite_id) in targets {
        let Some(sprite) = layers
            .layer_mut(layer_id)
            .and_then(|layer| layer.sprite_mut(sprite_id))
        else {
            continue;
        };
        if let Some((mask_image_id, mask_x, mask_y)) = mask_binding {
            sprite.mask_image_id = Some(mask_image_id);
            sprite.mask_offset_x = mask_x;
            sprite.mask_offset_y = mask_y;
        } else {
            sprite.mask_image_id = None;
            sprite.mask_offset_x = 0;
            sprite.mask_offset_y = 0;
        }
    }

    for (child_idx, child) in obj.runtime.child_objects.iter_mut().enumerate() {
        apply_object_masks_recursive(
            ids,
            gfx,
            layers,
            stage_i64,
            object_runtime_slot(child_idx, child) as i64,
            child,
            mask_info,
            resolved_masks,
        );
    }
}

fn apply_object_tonecurves_recursive(
    ids: &constants::RuntimeConstants,
    gfx: &mut graphics::GfxRuntime,
    images: &mut ImageManager,
    layers: &mut LayerManager,
    tonecurve: &mut tonecurve::ToneCurveRuntime,
    stage_i64: i64,
    obj_i64: i64,
    obj: &mut globals::ObjectState,
) {
    let tonecurve_no = if ids.obj_tonecurve_no != 0 {
        obj.lookup_int_prop(ids, ids.obj_tonecurve_no).unwrap_or(-1)
    } else {
        -1
    };
    if tonecurve_no >= 0 {
        if let Some((tonecurve_image_id, tonecurve_row, tonecurve_sat)) =
            tonecurve.shader_binding(images, tonecurve_no as i32)
        {
            let targets: Vec<(LayerId, SpriteId)> = match &obj.backend {
                globals::ObjectBackend::Gfx => gfx
                    .object_sprite_binding(stage_i64, obj_i64)
                    .into_iter()
                    .collect(),
                backend => layer_backed_object_sprite_bindings(backend),
            };
            for (layer_id, sprite_id) in targets {
                if let Some(sprite) = layers
                    .layer_mut(layer_id)
                    .and_then(|l| l.sprite_mut(sprite_id))
                {
                    sprite.tonecurve_image_id = Some(tonecurve_image_id);
                    sprite.tonecurve_row = tonecurve_row;
                    sprite.tonecurve_sat = tonecurve_sat;
                }
            }
        }
    }

    for (child_idx, child) in obj.runtime.child_objects.iter_mut().enumerate() {
        apply_object_tonecurves_recursive(
            ids,
            gfx,
            images,
            layers,
            tonecurve,
            stage_i64,
            object_runtime_slot(child_idx, child) as i64,
            child,
        );
    }
}

fn apply_gan_effects_recursive(
    gfx: &mut graphics::GfxRuntime,
    images: &mut ImageManager,
    sprites: &mut Vec<RenderSprite>,
    index: &HashMap<(Option<LayerId>, Option<SpriteId>), usize>,
    stage_i64: i64,
    obj_i64: i64,
    obj: &mut globals::ObjectState,
) {
    if let Some(pat) = obj.gan.current_pat() {
        if !(pat.pat_no == 0 && pat.x == 0 && pat.y == 0 && pat.tr == 255) {
            let keys: Vec<(LayerId, SpriteId)> = match &obj.backend {
                globals::ObjectBackend::Gfx => gfx
                    .object_sprite_binding(stage_i64, obj_i64)
                    .into_iter()
                    .collect(),
                backend => layer_backed_object_sprite_bindings(backend),
            };
            let replacement_image = if pat.pat_no != 0 {
                gfx.object_peek_file(stage_i64, obj_i64).and_then(|file| {
                    let base_pat = gfx.object_peek_patno(stage_i64, obj_i64).unwrap_or(0);
                    let pat_no = (base_pat + pat.pat_no as i64).max(0) as u32;
                    images.load_g00(&file, pat_no).ok()
                })
            } else {
                None
            };
            for (layer_id, sprite_id) in keys {
                let Some(&idx) = index.get(&(Some(layer_id), Some(sprite_id))) else {
                    continue;
                };
                let sprite = &mut sprites[idx].sprite;
                if pat.x != 0 {
                    sprite.x = sprite.x.saturating_add(pat.x);
                }
                if pat.y != 0 {
                    sprite.y = sprite.y.saturating_add(pat.y);
                }
                if pat.tr != 255 {
                    let tr = (sprite.tr as i64 * pat.tr as i64 / 255).clamp(0, 255) as u8;
                    sprite.tr = tr;
                }
                if let Some(id) = replacement_image {
                    sprite.image_id = Some(id);
                }
            }
        }
    }

    for (child_idx, child) in obj.runtime.child_objects.iter_mut().enumerate() {
        apply_gan_effects_recursive(
            gfx,
            images,
            sprites,
            index,
            stage_i64,
            object_runtime_slot(child_idx, child) as i64,
            child,
        );
    }
}

fn build_parent_render_state(
    info: &ObjectRenderInfo,
    first_sprite: Option<&Sprite>,
) -> ParentRenderState {
    ParentRenderState {
        world_no: info.world_no,
        pos_x: (info.x + info.x_rep) as f32,
        pos_y: (info.y + info.y_rep) as f32,
        pos_z: (info.z + info.z_rep) as f32,
        center_rep_x: info.center_rep_x as f32,
        center_rep_y: info.center_rep_y as f32,
        center_rep_z: info.center_rep_z as f32,
        scale_x: info.scale_x as f32 / 1000.0,
        scale_y: info.scale_y as f32 / 1000.0,
        scale_z: info.scale_z as f32 / 1000.0,
        rotate_x: info.rotate_x as f32 * std::f32::consts::PI / 1800.0,
        rotate_y: info.rotate_y as f32 * std::f32::consts::PI / 1800.0,
        rotate_z: info.rotate_z as f32 * std::f32::consts::PI / 1800.0,
        tr: ((info.tr.clamp(0, 255) * info.tr_rep.clamp(0, 255)) / 255) as i32,
        mono: info.mono.clamp(0, 255) as i32,
        reverse: info.reverse.clamp(0, 255) as i32,
        bright: info.bright.clamp(0, 255) as i32,
        dark: info.dark.clamp(0, 255) as i32,
        color_rate: info.color_rate.clamp(0, 255) as i32,
        color_r: info.color_r.clamp(0, 255) as i32,
        color_g: info.color_g.clamp(0, 255) as i32,
        color_b: info.color_b.clamp(0, 255) as i32,
        color_add_r: info.color_add_r.clamp(0, 255) as i32,
        color_add_g: info.color_add_g.clamp(0, 255) as i32,
        color_add_b: info.color_add_b.clamp(0, 255) as i32,
        blend: info.blend,
        dst_clip: info.dst_clip,
        mask_image_id: first_sprite.and_then(|s| s.mask_image_id),
        mask_offset_x: first_sprite.map(|s| s.mask_offset_x).unwrap_or(0),
        mask_offset_y: first_sprite.map(|s| s.mask_offset_y).unwrap_or(0),
        tonecurve_image_id: first_sprite.and_then(|s| s.tonecurve_image_id),
        tonecurve_row: first_sprite.map(|s| s.tonecurve_row).unwrap_or(0.0),
        tonecurve_sat: first_sprite.map(|s| s.tonecurve_sat).unwrap_or(0.0),
    }
}

fn apply_parent_render_state_to_sprite(
    sprite: &mut Sprite,
    _info: &ObjectRenderInfo,
    state: &ParentRenderState,
) {
    let local_x = sprite.x as f32;
    let local_y = sprite.y as f32;
    let local_z = sprite.z;

    let mut rel_x = local_x - state.center_rep_x;
    let mut rel_y = local_y - state.center_rep_y;
    rel_x *= state.scale_x;
    rel_y *= state.scale_y;
    let (sin_z, cos_z) = state.rotate_z.sin_cos();
    let rot_x = rel_x * cos_z - rel_y * sin_z;
    let rot_y = rel_x * sin_z + rel_y * cos_z;

    sprite.x = (state.pos_x + state.center_rep_x + rot_x).round() as i32;
    sprite.y = (state.pos_y + state.center_rep_y + rot_y).round() as i32;
    sprite.z = state.pos_z + state.center_rep_z + local_z * state.scale_z;
    sprite.scale_x *= state.scale_x;
    sprite.scale_y *= state.scale_y;
    sprite.scale_z *= state.scale_z;
    sprite.rotate_x += state.rotate_x;
    sprite.rotate_y += state.rotate_y;
    sprite.rotate += state.rotate_z;

    sprite.tr = ((sprite.tr as i32 * state.tr.clamp(0, 255)) / 255).clamp(0, 255) as u8;
    sprite.mono = combine_lerp(sprite.mono, state.mono);
    sprite.reverse = combine_lerp(sprite.reverse, state.reverse);
    sprite.bright = combine_lerp(sprite.bright, state.bright);
    sprite.dark = combine_lerp(sprite.dark, state.dark);
    if (sprite.color_rate as i32) + state.color_rate > 0 {
        let parent_rate = (state.color_rate * 255 * 255)
            / (255 * 255 - (255 - sprite.color_rate as i32) * (255 - state.color_rate)).max(1);
        sprite.color_r = blend_color(sprite.color_r, state.color_r, parent_rate);
        sprite.color_g = blend_color(sprite.color_g, state.color_g, parent_rate);
        sprite.color_b = blend_color(sprite.color_b, state.color_b, parent_rate);
        sprite.color_rate = combine_lerp(sprite.color_rate, state.color_rate);
    }
    sprite.color_add_r = sprite
        .color_add_r
        .saturating_add(state.color_add_r.clamp(0, 255) as u8);
    sprite.color_add_g = sprite
        .color_add_g
        .saturating_add(state.color_add_g.clamp(0, 255) as u8);
    sprite.color_add_b = sprite
        .color_add_b
        .saturating_add(state.color_add_b.clamp(0, 255) as u8);
    if matches!(sprite.blend, crate::layer::SpriteBlend::Normal) {
        sprite.blend = state.blend;
    }
    let child_clip = sprite.dst_clip;
    sprite.dst_clip = compose_clip_rect(state.dst_clip, child_clip);
    if state.dst_clip.is_some() && child_clip.is_some() && sprite.dst_clip.is_none() {
        sprite.tr = 0;
    }
    if sprite.mask_image_id.is_none() {
        sprite.mask_image_id = state.mask_image_id;
        sprite.mask_offset_x = state.mask_offset_x;
        sprite.mask_offset_y = state.mask_offset_y;
    }
    if sprite.tonecurve_image_id.is_none() {
        sprite.tonecurve_image_id = state.tonecurve_image_id;
        sprite.tonecurve_row = state.tonecurve_row;
        sprite.tonecurve_sat = state.tonecurve_sat;
    }

    if state.world_no >= 0 {
        sprite.world_no = state.world_no as i32;
    }
}

fn apply_world_camera_mode(
    sprite: &mut Sprite,
    worlds: Option<&Vec<globals::WorldState>>,
    screen_w: u32,
    screen_h: u32,
) {
    if sprite.world_no < 0 {
        return;
    }
    let Some(worlds) = worlds else {
        return;
    };
    let Some(world) = worlds.get(sprite.world_no as usize) else {
        return;
    };

    let cam_eye = [
        world.camera_eye_x.get_total_value() as f32,
        world.camera_eye_y.get_total_value() as f32,
        world.camera_eye_z.get_total_value() as f32,
    ];
    let cam_target = [
        world.camera_pint_x.get_total_value() as f32,
        world.camera_pint_y.get_total_value() as f32,
        world.camera_pint_z.get_total_value() as f32,
    ];
    let cam_up = [
        world.camera_up_x.get_total_value() as f32,
        world.camera_up_y.get_total_value() as f32,
        world.camera_up_z.get_total_value() as f32,
    ];
    sprite.camera_view_angle_deg = (world.camera_view_angle as f32) / 10.0;
    if world.mono != 0 {
        let base = sprite.mono as i32;
        let parent = world.mono.clamp(0, 255);
        sprite.mono = (255 - (255 - base) * (255 - parent) / 255) as u8;
    }

    if world.mode == 0 {
        let dz = sprite.z - cam_eye[2];
        if dz <= 0.0 {
            sprite.visible = false;
            return;
        }
        let camera_scale = 1000.0 / dz;
        let sw = screen_w as f32;
        let sh = screen_h as f32;
        sprite.x = (((sprite.x as f32) - cam_eye[0]) * camera_scale + sw * 0.5)
            .round()
            .clamp(i32::MIN as f32, i32::MAX as f32) as i32;
        sprite.y = (((sprite.y as f32) - cam_eye[1]) * camera_scale + sh * 0.5)
            .round()
            .clamp(i32::MIN as f32, i32::MAX as f32) as i32;
        sprite.scale_x *= camera_scale;
        sprite.scale_y *= camera_scale;
        sprite.z = 0.0;
        sprite.pivot_z = 0.0;
        sprite.scale_z = 1.0;
        sprite.rotate_x = 0.0;
        sprite.rotate_y = 0.0;
        sprite.billboard = false;
        sprite.culling = false;
        sprite.fog_use = false;
        sprite.light_no = -1;
        sprite.light_enabled = false;
        sprite.light_diffuse = [1.0, 1.0, 1.0, 1.0];
        sprite.light_ambient = [0.0, 0.0, 0.0, 1.0];
        sprite.light_specular = [0.0, 0.0, 0.0, 1.0];
        sprite.light_factor = 0.0;
        sprite.light_kind = -1;
        sprite.light_pos = [0.0, 0.0, 0.0, 0.0];
        sprite.light_dir = [0.0, 0.0, -1.0, 0.0];
        sprite.light_atten = [1.0, 0.0, 0.0, 5000.0];
        sprite.light_cone = [0.0, 0.0, 1.0, 0.0];
        sprite.fog_enabled = false;
        sprite.fog_color = [0.0, 0.0, 0.0, 1.0];
        sprite.fog_near = 0.0;
        sprite.fog_far = 0.0;
        sprite.fog_scroll_x = 0.0;
        sprite.fog_texture_image_id = None;
        sprite.camera_enabled = false;
        sprite.camera_eye = [0.0, 0.0, -1000.0];
        sprite.camera_target = [0.0, 0.0, 0.0];
        sprite.camera_up = [0.0, 1.0, 0.0];
        return;
    }

    sprite.camera_enabled = true;
    sprite.camera_eye = cam_eye;
    sprite.camera_target = cam_target;
    sprite.camera_up = cam_up;
}

fn fetch_bound_render_sprites(
    ctx: &CommandContext,
    stage_idx: i64,
    runtime_slot: usize,
    obj: &globals::ObjectState,
) -> Vec<RenderSprite> {
    // Object tree visibility is driven by C_elm_object::disp and parent visibility.
    // The backing layer sprite visible bit is only a cached render backend state and
    // can be stale for object-owned sprites. Fetch the sprite payload unconditionally;
    // append_object_tree_nodes() applies the original object visibility gate.
    fetch_bound_render_sprites_impl(ctx, stage_idx, runtime_slot, obj, false)
}

fn fetch_bound_render_sprites_any(
    ctx: &CommandContext,
    stage_idx: i64,
    runtime_slot: usize,
    obj: &globals::ObjectState,
) -> Vec<RenderSprite> {
    fetch_bound_render_sprites_impl(ctx, stage_idx, runtime_slot, obj, false)
}

fn fetch_bound_render_sprites_impl(
    ctx: &CommandContext,
    stage_idx: i64,
    runtime_slot: usize,
    obj: &globals::ObjectState,
    visible_only: bool,
) -> Vec<RenderSprite> {
    fn push_one(
        ctx: &CommandContext,
        lid: LayerId,
        sid: SpriteId,
        visible_only: bool,
        out: &mut Vec<RenderSprite>,
    ) {
        let Some(layer) = ctx.layers.layer(lid) else {
            return;
        };
        let Some(sprite) = layer.sprite(sid) else {
            return;
        };
        if visible_only && !sprite.visible {
            return;
        }
        let has_emote = sprite.emote_render.is_some();
        if sprite.image_id.is_none() && !has_emote {
            return;
        }
        out.push(RenderSprite::new(Some(lid), Some(sid), sprite.clone()));
    }

    let mut out = Vec::new();
    match &obj.backend {
        globals::ObjectBackend::Gfx => {
            if let Some((lid, sid)) = ctx
                .gfx
                .object_sprite_binding(stage_idx, runtime_slot as i64)
            {
                push_one(ctx, lid, sid, visible_only, &mut out);
            }
        }
        globals::ObjectBackend::None => {}
        backend => {
            for (layer_id, sprite_id) in layer_backed_object_sprite_bindings(backend) {
                push_one(ctx, layer_id, sprite_id, visible_only, &mut out);
            }
        }
    }
    out
}

fn effective_object_info(
    ctx: &CommandContext,
    stage_idx: i64,
    obj_idx: usize,
    obj: &globals::ObjectState,
) -> ObjectRenderInfo {
    let runtime_slot = object_runtime_slot(obj_idx, obj);
    let ids = &ctx.ids;
    let extra = |id: i32, default: i64| -> i64 {
        if id != 0 {
            obj.lookup_int_prop(ids, id).unwrap_or(default)
        } else {
            default
        }
    };
    let extra_str = |id: i32| -> Option<String> {
        if id != 0 {
            obj.lookup_str_prop(ids, id)
        } else {
            None
        }
    };

    let dst_clip = if extra(ids.obj_clip_use, obj.base.clip_use) != 0 {
        Some(ClipRect {
            left: extra(ids.obj_clip_left, obj.base.clip_left) as i32,
            top: extra(ids.obj_clip_top, obj.base.clip_top) as i32,
            right: extra(ids.obj_clip_right, obj.base.clip_right) as i32,
            bottom: extra(ids.obj_clip_bottom, obj.base.clip_bottom) as i32,
        })
    } else {
        None
    };

    let x_rep_total = obj
        .runtime
        .prop_event_lists
        .x_rep
        .iter()
        .map(|ev| ev.get_total_value() as i64)
        .sum::<i64>();
    let y_rep_total = obj
        .runtime
        .prop_event_lists
        .y_rep
        .iter()
        .map(|ev| ev.get_total_value() as i64)
        .sum::<i64>();
    let z_rep_total = obj
        .runtime
        .prop_event_lists
        .z_rep
        .iter()
        .map(|ev| ev.get_total_value() as i64)
        .sum::<i64>();
    let tr_rep_total = obj
        .runtime
        .prop_event_lists
        .tr_rep
        .iter()
        .fold(255i64, |acc, ev| {
            acc.saturating_mul(ev.get_total_value() as i64)
                .div_euclid(255)
        });

    let mut info = ObjectRenderInfo {
        runtime_slot,
        used: obj.used,
        object_type: obj.object_type,
        disp: extra(ids.obj_disp, obj.base.disp) != 0,
        x: extra(ids.obj_x, obj.base.x),
        y: extra(ids.obj_y, obj.base.y),
        x_rep: x_rep_total,
        y_rep: y_rep_total,
        z_rep: z_rep_total,
        order: extra(ids.obj_order, obj.base.order),
        layer: extra(ids.obj_layer, obj.base.layer),
        alpha: extra(ids.obj_alpha, obj.base.alpha),
        tr: extra(ids.obj_tr, obj.base.tr),
        tr_rep: tr_rep_total,
        mono: extra(ids.obj_mono, obj.base.mono),
        reverse: extra(ids.obj_reverse, obj.base.reverse),
        bright: extra(ids.obj_bright, obj.base.bright),
        dark: extra(ids.obj_dark, obj.base.dark),
        color_rate: extra(ids.obj_color_rate, obj.base.color_rate),
        color_add_r: extra(ids.obj_color_add_r, obj.base.color_add_r),
        color_add_g: extra(ids.obj_color_add_g, obj.base.color_add_g),
        color_add_b: extra(ids.obj_color_add_b, obj.base.color_add_b),
        color_r: extra(ids.obj_color_r, obj.base.color_r),
        color_g: extra(ids.obj_color_g, obj.base.color_g),
        color_b: extra(ids.obj_color_b, obj.base.color_b),
        z: extra(ids.obj_z, obj.base.z),
        world_no: extra(ids.obj_world, obj.base.world),
        center_x: extra(ids.obj_center_x, obj.base.center_x),
        center_y: extra(ids.obj_center_y, obj.base.center_y),
        center_z: extra(ids.obj_center_z, obj.base.center_z),
        center_rep_x: extra(ids.obj_center_rep_x, obj.base.center_rep_x),
        center_rep_y: extra(ids.obj_center_rep_y, obj.base.center_rep_y),
        center_rep_z: extra(ids.obj_center_rep_z, obj.base.center_rep_z),
        scale_x: extra(ids.obj_scale_x, obj.base.scale_x),
        scale_y: extra(ids.obj_scale_y, obj.base.scale_y),
        scale_z: extra(ids.obj_scale_z, obj.base.scale_z),
        rotate_x: extra(ids.obj_rotate_x, obj.base.rotate_x),
        rotate_y: extra(ids.obj_rotate_y, obj.base.rotate_y),
        rotate_z: extra(ids.obj_rotate_z, obj.base.rotate_z),
        culling: extra(ids.obj_culling, obj.base.culling) != 0,
        alpha_test: extra(ids.obj_alpha_test, obj.base.alpha_test) != 0,
        alpha_blend: extra(ids.obj_alpha_blend, obj.base.alpha_blend) != 0,
        fog_use: extra(ids.obj_fog_use, obj.base.fog_use) != 0,
        light_no: extra(ids.obj_light_no, obj.base.light_no),
        blend: crate::layer::SpriteBlend::from_i64(extra(ids.obj_blend, obj.base.blend)),
        child_sort_type: obj.base.child_sort_type,
        dst_clip,
        billboard: obj.object_type == 7,
        file_name: obj.file_name.clone(),
        mesh_animation: obj.mesh_animation_state.clone(),
    };

    match &obj.backend {
        globals::ObjectBackend::Gfx => {
            // C_elm_mwnd_waku::m_btn_list and OBJECT.CHILD entries are internal
            // object trees, not top-level C_elm_stage::m_obj_list entries. Their
            // Gfx layer sprite is only backing storage. Do not read the backing
            // sprite's cached visible/pos/order/layer state here, because it can be
            // hidden to prevent raw LayerManager leakage and because the authoritative
            // state for tree rendering is the C_elm_object property block.
            let embedded_tree_object = obj.nested_runtime_slot.is_some();
            if !embedded_tree_object {
                if let Some(v) = ctx.gfx.object_peek_disp(stage_idx, runtime_slot as i64) {
                    info.disp = v != 0;
                }
                if let Some((x, y)) = ctx.gfx.object_peek_pos(stage_idx, runtime_slot as i64) {
                    info.x = x;
                    info.y = y;
                }
                if let Some(v) = ctx.gfx.object_peek_order(stage_idx, runtime_slot as i64) {
                    info.order = v;
                }
                if let Some(v) = ctx.gfx.object_peek_layer(stage_idx, runtime_slot as i64) {
                    info.layer = v;
                }
                if let Some(v) = ctx.gfx.object_peek_alpha(stage_idx, runtime_slot as i64) {
                    info.alpha = v;
                }
            }
            if !embedded_tree_object {
                if let Some((lid, sid)) = ctx
                    .gfx
                    .object_sprite_binding(stage_idx, runtime_slot as i64)
                {
                    if let Some(layer) = ctx.layers.layer(lid) {
                        if let Some(sprite) = layer.sprite(sid) {
                            info.tr = sprite.tr as i64;
                        }
                    }
                }
            }
        }
        globals::ObjectBackend::Rect { .. }
        | globals::ObjectBackend::String { .. }
        | globals::ObjectBackend::Movie { .. }
        | globals::ObjectBackend::Number { .. }
        | globals::ObjectBackend::Weather { .. } => {
            // The backend sprite only stores image handles and backend-only data.
            // C++ C_elm_object::frame uses the object parameter block for DISP,
            // X/Y, sorter, alpha and TR.  Reading those fields back from the
            // storage sprite makes objects created at local (0,0), such as save
            // thumbnails, ignore later SET_POS or parent object transforms.
        }
        globals::ObjectBackend::None => {
            if let Some(v) = obj.lookup_int_prop(ids, ids.obj_disp) {
                info.disp = v != 0;
            } else if obj.object_type == 0 && !obj.runtime.child_objects.is_empty() {
                info.disp = true;
            }
        }
    }

    let event_total = |event_op: i32, current: i64| -> i64 {
        if event_op != 0 {
            obj.int_event_by_op(ids, event_op)
                .map(|ev| ev.get_total_value() as i64)
                .unwrap_or(current)
        } else {
            current
        }
    };

    info.x = event_total(ids.obj_x_eve, info.x);
    info.y = event_total(ids.obj_y_eve, info.y);
    info.z = event_total(ids.obj_z_eve, info.z);
    info.tr = event_total(ids.obj_tr_eve, info.tr);
    info.mono = event_total(ids.obj_mono_eve, info.mono);
    info.reverse = event_total(ids.obj_reverse_eve, info.reverse);
    info.bright = event_total(ids.obj_bright_eve, info.bright);
    info.dark = event_total(ids.obj_dark_eve, info.dark);
    info.color_rate = event_total(ids.obj_color_rate_eve, info.color_rate);
    info.color_add_r = event_total(ids.obj_color_add_r_eve, info.color_add_r);
    info.color_add_g = event_total(ids.obj_color_add_g_eve, info.color_add_g);
    info.color_add_b = event_total(ids.obj_color_add_b_eve, info.color_add_b);
    info.color_r = event_total(ids.obj_color_r_eve, info.color_r);
    info.color_g = event_total(ids.obj_color_g_eve, info.color_g);
    info.color_b = event_total(ids.obj_color_b_eve, info.color_b);
    info.center_x = event_total(ids.obj_center_x_eve, info.center_x);
    info.center_y = event_total(ids.obj_center_y_eve, info.center_y);
    info.center_z = event_total(ids.obj_center_z_eve, info.center_z);
    info.center_rep_x = event_total(ids.obj_center_rep_x_eve, info.center_rep_x);
    info.center_rep_y = event_total(ids.obj_center_rep_y_eve, info.center_rep_y);
    info.center_rep_z = event_total(ids.obj_center_rep_z_eve, info.center_rep_z);
    info.scale_x = event_total(ids.obj_scale_x_eve, info.scale_x);
    info.scale_y = event_total(ids.obj_scale_y_eve, info.scale_y);
    info.scale_z = event_total(ids.obj_scale_z_eve, info.scale_z);
    info.rotate_x = event_total(ids.obj_rotate_x_eve, info.rotate_x);
    info.rotate_y = event_total(ids.obj_rotate_y_eve, info.rotate_y);
    info.rotate_z = event_total(ids.obj_rotate_z_eve, info.rotate_z);

    if extra(ids.obj_clip_use, 0) != 0 {
        info.dst_clip = Some(ClipRect {
            left: event_total(ids.obj_clip_left_eve, extra(ids.obj_clip_left, 0)) as i32,
            top: event_total(ids.obj_clip_top_eve, extra(ids.obj_clip_top, 0)) as i32,
            right: event_total(ids.obj_clip_right_eve, extra(ids.obj_clip_right, 0)) as i32,
            bottom: event_total(ids.obj_clip_bottom_eve, extra(ids.obj_clip_bottom, 0)) as i32,
        });
    }

    info
}

fn configure_sprite_3d(
    sprite: &mut crate::layer::Sprite,
    info: &ObjectRenderInfo,
    _worlds: Option<&Vec<globals::WorldState>>,
    _screen_w: u32,
    _screen_h: u32,
) {
    sprite.z = info.z as f32;
    sprite.pivot_z = info.center_z as f32;
    sprite.scale_z = info.scale_z as f32 / 1000.0;
    sprite.rotate_x = info.rotate_x as f32 * std::f32::consts::PI / 1800.0;
    sprite.rotate_y = info.rotate_y as f32 * std::f32::consts::PI / 1800.0;
    sprite.culling = info.culling;
    sprite.alpha_test = info.alpha_test;
    sprite.alpha_blend = info.alpha_blend;
    sprite.fog_use = info.fog_use;
    sprite.light_no = info.light_no as i32;
    sprite.world_no = info.world_no as i32;
    sprite.billboard = info.billboard;
    sprite.mesh_file_name = if info.object_type == 6 {
        info.file_name.clone()
    } else {
        None
    };
    sprite.mesh_kind = if info.object_type == 6 { 1 } else { 0 };
    sprite.shadow_cast = sprite.mesh_kind != 0;
    sprite.shadow_receive = sprite.mesh_kind != 0;
    sprite.mesh_animation = info.mesh_animation.clone();

    let uses_3d = matches!(info.object_type, 6 | 7)
        || info.billboard
        || info.z != 0
        || info.center_z != 0
        || info.scale_z != 1000
        || info.rotate_x != 0
        || info.rotate_y != 0;

    sprite.camera_enabled = uses_3d;
    sprite.camera_eye = [0.0, 0.0, -1000.0];
    sprite.camera_target = [0.0, 0.0, 0.0];
    sprite.camera_up = [0.0, 1.0, 0.0];
    sprite.camera_view_angle_deg = 45.0;
}


fn object_motion_trace_enabled() -> bool {
    trace_env().sg_object_motion_trace
}

fn object_motion_trace_object(obj: &globals::ObjectState) -> bool {
    let Some(file_name) = obj.file_name.as_deref() else {
        return false;
    };
    file_name
        .rsplit(|c| c == '/' || c == '\\')
        .next()
        .map(|base| base.to_ascii_lowercase().starts_with("mp_"))
        .unwrap_or(false)
}

fn object_motion_trace_bind(
    ctx: &CommandContext,
    stage_idx: i64,
    info: &ObjectRenderInfo,
    obj: &globals::ObjectState,
) -> Option<(LayerId, SpriteId)> {
    match &obj.backend {
        globals::ObjectBackend::Gfx => ctx
            .gfx
            .object_sprite_binding(stage_idx, info.runtime_slot as i64),
        globals::ObjectBackend::Rect {
            layer_id,
            sprite_id,
            ..
        }
        | globals::ObjectBackend::String {
            layer_id,
            sprite_id,
            ..
        }
        | globals::ObjectBackend::Movie {
            layer_id,
            sprite_id,
            ..
        } => Some((*layer_id, *sprite_id)),
        globals::ObjectBackend::Number {
            layer_id,
            sprite_ids,
        }
        | globals::ObjectBackend::Weather {
            layer_id,
            sprite_ids,
        } => sprite_ids.first().copied().map(|sid| (*layer_id, sid)),
        globals::ObjectBackend::None => None,
    }
}

fn object_motion_trace_emit(
    ctx: &CommandContext,
    stage_idx: i64,
    obj_idx: usize,
    obj: &globals::ObjectState,
    info: &ObjectRenderInfo,
    parent_visible: bool,
    visible: bool,
    local_tr: i64,
    total_order: i64,
    total_layer: i64,
    bound_len: usize,
    bind_dbg: Option<(LayerId, SpriteId)>,
    emitted: bool,
    sprite: Option<&Sprite>,
) {
    let ev = &obj.runtime.prop_events;
    let sprite_desc = sprite
        .map(|s| {
            format!(
                "sprite_pos=({}, {}, {:.3}) sprite_order={} sprite_sorter=({}, {}) sprite_alpha={} sprite_tr={} image={:?}",
                s.x,
                s.y,
                s.z,
                s.order,
                unpack_legacy_sorter_key(s.order).0,
                unpack_legacy_sorter_key(s.order).1,
                s.alpha,
                s.tr,
                s.image_id
            )
        })
        .unwrap_or_else(|| "sprite_pos=(none) sprite_order=(none) sprite_sorter=(none) sprite_alpha=(none) sprite_tr=(none) image=None".to_string());
    eprintln!(
        "[SG_OBJECT_MOTION_TRACE][RENDER] frame={} stage={} obj_idx={} slot={} file={} used={} backend={:?} parent_visible={} visible={} emitted={} disp={} local=({}, {}, {}) rep=({}, {}, {}) center=({}, {}, {}) center_rep=({}, {}, {}) prop_final=({}, {}, {}) order={} layer={} total_order={} total_layer={} alpha={} tr={} tr_rep={} local_tr={} bound_len={} bind={:?} x_eve=active:{} total:{} value:{} time:{}/{} y_eve=active:{} total:{} value:{} time:{}/{} tr_eve=active:{} total:{} value:{} time:{}/{} {}",
        ctx.globals.render_frame,
        stage_idx,
        obj_idx,
        info.runtime_slot,
        obj.file_name.as_deref().unwrap_or("-"),
        obj.used,
        obj.backend,
        parent_visible,
        visible,
        emitted,
        info.disp,
        info.x,
        info.y,
        info.z,
        info.x_rep,
        info.y_rep,
        info.z_rep,
        info.center_x,
        info.center_y,
        info.center_z,
        info.center_rep_x,
        info.center_rep_y,
        info.center_rep_z,
        info.x + info.x_rep + info.center_rep_x,
        info.y + info.y_rep + info.center_rep_y,
        info.z + info.z_rep + info.center_rep_z,
        info.order,
        info.layer,
        total_order,
        total_layer,
        info.alpha,
        info.tr,
        info.tr_rep,
        local_tr,
        bound_len,
        bind_dbg,
        ev.x.check_event(),
        ev.x.get_total_value(),
        ev.x.get_value(),
        ev.x.cur_time,
        ev.x.end_time,
        ev.y.check_event(),
        ev.y.get_total_value(),
        ev.y.get_value(),
        ev.y.cur_time,
        ev.y.end_time,
        ev.tr.check_event(),
        ev.tr.get_total_value(),
        ev.tr.get_value(),
        ev.tr.cur_time,
        ev.tr.end_time,
        sprite_desc,
    );
}

fn object_tree_texture_key(
    ctx: &CommandContext,
    stage_idx: i64,
    obj_idx: usize,
    obj: &globals::ObjectState,
) -> (bool, u32) {
    let slot = object_runtime_slot(obj_idx, obj);
    let image = fetch_bound_render_sprites_any(ctx, stage_idx, slot, obj)
        .first()
        .and_then(|rs| rs.sprite.image_id);
    (image.is_some(), image.map(|id| id.0).unwrap_or(0))
}

fn object_tree_stored_axis(obj: &globals::ObjectState, axis: u8) -> i32 {
    // C_elm_object::get_pos_x/y/z() uses IntEvent::get_value(), not the
    // animated get_total_value(). Custom child sorting therefore follows the
    // stored destination value and does not reshuffle while an event is moving.
    match axis {
        0 => obj.runtime.prop_events.x.get_value(),
        1 => obj.runtime.prop_events.y.get_value(),
        _ => obj.runtime.prop_events.z.get_value(),
    }
}

fn append_object_tree_nodes(
    ctx: &CommandContext,
    worlds: Option<&Vec<globals::WorldState>>,
    stage_idx: i64,
    obj_idx: usize,
    obj: &globals::ObjectState,
    parent_visible: bool,
    parent_order: i64,
    parent_layer: i64,
    parent_state: Option<ParentRenderState>,
    out: &mut Vec<SiglusRenderNode>,
    object_keys: &mut HashSet<(LayerId, SpriteId)>,
    debug_lines: &mut Vec<String>,
) {
    if !object_participates_in_tree(obj) {
        return;
    }

    let debug_enabled = sg_render_tree_debug_enabled();
    let info = effective_object_info(ctx, stage_idx, obj_idx, obj);
    let local_tr = ((info.tr.clamp(0, 255) * info.tr_rep.clamp(0, 255)) / 255).clamp(0, 255);
    let visible = parent_visible
        && info.disp
        && local_tr > 0
        && object_button_renderable_by_syscom(&ctx.globals.syscom, obj);
    let total_order = parent_order.saturating_add(info.order);
    let total_layer = parent_layer.saturating_add(info.layer);
    let mut own_sprites: Vec<RenderSprite> = Vec::new();

    if debug_enabled {
        let bind_dbg = match &obj.backend {
            globals::ObjectBackend::Gfx => ctx
                .gfx
                .object_sprite_binding(stage_idx, info.runtime_slot as i64),
            globals::ObjectBackend::Rect {
                layer_id,
                sprite_id,
                ..
            }
            | globals::ObjectBackend::String {
                layer_id,
                sprite_id,
                ..
            }
            | globals::ObjectBackend::Movie {
                layer_id,
                sprite_id,
                ..
            } => Some((*layer_id, *sprite_id)),
            globals::ObjectBackend::Number {
                layer_id,
                sprite_ids,
            }
            | globals::ObjectBackend::Weather {
                layer_id,
                sprite_ids,
            } => sprite_ids.first().copied().map(|sid| (*layer_id, sid)),
            globals::ObjectBackend::None => None,
        };
        debug_lines.push(format!(
            "[SG_DEBUG]     obj[{obj_idx}] slot={} used={} type={} backend={:?} file={} disp={} pos=({}, {}) center=({}, {}, {}) center_rep=({}, {}, {}) final_pos=({}, {}, {}) order={} layer={} alpha={} tr={} z={} child_sort={} wipe_copy={} wipe_erase={} bind={:?}",
            info.runtime_slot,
            obj.used,
            obj.object_type,
            obj.backend,
            obj.file_name.as_deref().unwrap_or("-"),
            info.disp,
            info.x,
            info.y,
            info.center_x,
            info.center_y,
            info.center_z,
            info.center_rep_x,
            info.center_rep_y,
            info.center_rep_z,
            info.x + info.x_rep + info.center_rep_x,
            info.y + info.y_rep + info.center_rep_y,
            info.z + info.z_rep + info.center_rep_z,
            info.order,
            info.layer,
            info.alpha,
            info.tr,
            info.z,
            info.child_sort_type,
            obj.get_int_prop(&ctx.ids, ctx.ids.obj_wipe_copy),
            obj.get_int_prop(&ctx.ids, ctx.ids.obj_wipe_erase),
            bind_dbg,
        ));
    }

    if debug_enabled && obj.button.enabled {
        debug_lines.push(format!(
            "[SG_DEBUG]       button enabled=true no={} group_no={} group_idx={:?} cut={} action={} se={} state={} hit={} pushed={} alpha_test={} call={}::{}/{}",
            obj.button.button_no,
            obj.button.group_no,
            obj.button.group_idx(),
            obj.button.cut_no,
            obj.button.action_no,
            obj.button.se_no,
            obj.button.state,
            obj.button.hit,
            obj.button.pushed,
            obj.button.alpha_test,
            obj.button.decided_action_scn_name,
            obj.button.decided_action_cmd_name,
            obj.button.decided_action_z_no,
        ));
    }
    if debug_enabled && (!obj.frame_action.cmd_name.is_empty() || obj.frame_action.end_flag) {
        debug_lines.push(format!(
            "[SG_DEBUG]       frame_action cmd={}::{} count={} end_time={} real={} end_flag={} args={:?}",
            obj.frame_action.scn_name,
            obj.frame_action.cmd_name,
            obj.frame_action.counter.get_count(),
            obj.frame_action.end_time,
            obj.frame_action.real_time_flag,
            obj.frame_action.end_flag,
            obj.frame_action.args,
        ));
    }
    for (fa_idx, fa) in obj.frame_action_ch.iter().enumerate() {
        if debug_enabled && (!fa.cmd_name.is_empty() || fa.end_flag) {
            debug_lines.push(format!(
                "[SG_DEBUG]       frame_action_ch[{}] cmd={}::{} count={} end_time={} real={} end_flag={} args={:?}",
                fa_idx,
                fa.scn_name,
                fa.cmd_name,
                fa.counter.get_count(),
                fa.end_time,
                fa.real_time_flag,
                fa.end_flag,
                fa.args,
            ));
        }
    }
    let ev = &obj.runtime.prop_events;
    if debug_enabled
        && (ev.color_rate.check_event()
            || ev.tr.check_event()
            || ev.x.check_event()
            || ev.y.check_event())
    {
        debug_lines.push(format!(
            "[SG_DEBUG]       active_events x={}/{} t={}/{} y={}/{} t={}/{} tr={}/{} t={}/{} color_rate={}/{} t={}/{}",
            ev.x.get_total_value(), ev.x.get_value(), ev.x.cur_time, ev.x.end_time,
            ev.y.get_total_value(), ev.y.get_value(), ev.y.cur_time, ev.y.end_time,
            ev.tr.get_total_value(), ev.tr.get_value(), ev.tr.cur_time, ev.tr.end_time,
            ev.color_rate.get_total_value(), ev.color_rate.get_value(), ev.color_rate.cur_time, ev.color_rate.end_time,
        ));
    }
    if debug_enabled
        && (!obj.runtime.prop_event_lists.x_rep.is_empty()
            || !obj.runtime.prop_event_lists.y_rep.is_empty()
            || !obj.runtime.prop_event_lists.tr_rep.is_empty())
    {
        let fmt_list = |list: &Vec<crate::runtime::int_event::IntEvent>| -> Vec<String> {
            list.iter()
                .enumerate()
                .filter(|(_, ev)| {
                    ev.check_event()
                        || ev.get_total_value() != ev.def_value
                        || ev.get_value() != ev.def_value
                })
                .map(|(idx, ev)| {
                    format!(
                        "{}:{}/{} t={}/{} active={}",
                        idx,
                        ev.get_total_value(),
                        ev.get_value(),
                        ev.cur_time,
                        ev.end_time,
                        ev.check_event()
                    )
                })
                .collect()
        };
        let x_rep = fmt_list(&obj.runtime.prop_event_lists.x_rep);
        let y_rep = fmt_list(&obj.runtime.prop_event_lists.y_rep);
        let tr_rep = fmt_list(&obj.runtime.prop_event_lists.tr_rep);
        if !x_rep.is_empty() || !y_rep.is_empty() || !tr_rep.is_empty() {
            debug_lines.push(format!(
                "[SG_DEBUG]       rep_events x={:?} y={:?} tr={:?}",
                x_rep, y_rep, tr_rep,
            ));
        }
    }

    let mut bound = fetch_bound_render_sprites(ctx, stage_idx, info.runtime_slot, obj);
    let motion_trace = object_motion_trace_enabled() && object_motion_trace_object(obj);
    let motion_bind_dbg = if motion_trace {
        object_motion_trace_bind(ctx, stage_idx, &info, obj)
    } else {
        None
    };
    if motion_trace && (!visible || bound.is_empty()) {
        object_motion_trace_emit(
            ctx,
            stage_idx,
            obj_idx,
            obj,
            &info,
            parent_visible,
            visible,
            local_tr,
            total_order,
            total_layer,
            bound.len(),
            motion_bind_dbg,
            false,
            None,
        );
    }
    if config_button_trace_enabled() && config_button_trace_object(obj) {
        let bind_dbg = match &obj.backend {
            globals::ObjectBackend::Gfx => ctx
                .gfx
                .object_sprite_binding(stage_idx, info.runtime_slot as i64),
            globals::ObjectBackend::Rect {
                layer_id,
                sprite_id,
                ..
            }
            | globals::ObjectBackend::String {
                layer_id,
                sprite_id,
                ..
            }
            | globals::ObjectBackend::Movie {
                layer_id,
                sprite_id,
                ..
            } => Some((*layer_id, *sprite_id)),
            globals::ObjectBackend::Number {
                layer_id,
                sprite_ids,
            }
            | globals::ObjectBackend::Weather {
                layer_id,
                sprite_ids,
            } => sprite_ids.first().copied().map(|sid| (*layer_id, sid)),
            globals::ObjectBackend::None => None,
        };
        let syscom_renderable = object_button_renderable_by_syscom(&ctx.globals.syscom, obj);
        debug_lines.push(format!(
            "[SG_DEBUG][CONFIG_BUTTON_TRACE][COLLECT] stage={} obj_idx={} runtime_slot={} file={} backend={:?} participates={} parent_visible={} disp={} local_tr={} tr={} tr_rep={} syscom_renderable={} visible={} bound_len={} bind={:?} order={} layer={} total_order={} total_layer={} button_enabled={} button_state={} button_no={} group_no={} action_no={} hit={} pushed={} disabled_reason={:?} parent_state={}",
            stage_idx,
            obj_idx,
            info.runtime_slot,
            obj.file_name.as_deref().unwrap_or("-"),
            obj.backend,
            object_participates_in_tree(obj),
            parent_visible,
            info.disp,
            local_tr,
            info.tr,
            info.tr_rep,
            syscom_renderable,
            visible,
            bound.len(),
            bind_dbg,
            info.order,
            info.layer,
            total_order,
            total_layer,
            obj.button.enabled,
            obj.button.state,
            obj.button.button_no,
            obj.button.group_no,
            obj.button.action_no,
            obj.button.hit,
            obj.button.pushed,
            button_disabled_reason(&ctx.globals.syscom, obj, None),
            parent_state.is_some()
        ));
    }
    for rs in &bound {
        if let (Some(lid), Some(sid)) = (rs.layer_id, rs.sprite_id) {
            object_keys.insert((lid, sid));
        }
    }
    let mut cur_parent_state = build_parent_render_state(&info, bound.first().map(|rs| &rs.sprite));
    if let Some(parent) = parent_state {
        cur_parent_state = compose_parent_render_state(parent, cur_parent_state);
    }

    if visible {
        if obj.object_type == 4 {
            let out_len_before = own_sprites.len();
            append_weather_sprites(
                ctx,
                worlds,
                obj,
                &info,
                total_order,
                total_layer,
                &bound,
                &mut own_sprites,
            );
            for rs in own_sprites[out_len_before..].iter_mut() {
                if let Some(parent) = parent_state {
                    apply_parent_render_state_to_sprite(&mut rs.sprite, &info, &parent);
                }
                finalize_object_center_rep_to_sprite(&mut rs.sprite, &info);
                apply_world_camera_mode(&mut rs.sprite, worlds, ctx.screen_w, ctx.screen_h);
                apply_runtime_light_and_fog(ctx, &mut rs.sprite);
                if motion_trace {
                    object_motion_trace_emit(
                        ctx,
                        stage_idx,
                        obj_idx,
                        obj,
                        &info,
                        parent_visible,
                        visible,
                        local_tr,
                        total_order,
                        total_layer,
                        bound.len(),
                        motion_bind_dbg,
                        rs.sprite.tr > 0,
                        Some(&rs.sprite),
                    );
                }
            }
        } else {
            let bound_len_for_trace = bound.len();
            for mut rs in bound.drain(..) {
                apply_object_render_info_to_sprite(&mut rs.sprite, &info);
                let (local_x, local_y) =
                    object_backend_sprite_local_offset(&obj.backend, rs.sprite_id);
                if local_x != 0 || local_y != 0 {
                    rs.sprite.x = (rs.sprite.x as i64 + local_x)
                        .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                    rs.sprite.y = (rs.sprite.y as i64 + local_y)
                        .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                    // All glyph quads share the object's transform origin.
                    // Moving the individual quad requires the inverse pivot
                    // adjustment, matching C++ rp.center -= glyph.pos.
                    rs.sprite.pivot_x -= local_x as f32;
                    rs.sprite.pivot_y -= local_y as f32;
                }
                let sprite_layer = total_layer.saturating_add(
                    object_backend_sprite_layer_offset(ctx, &obj.backend, rs.sprite_id),
                );
                rs.set_sorter(total_order, sprite_layer);
                rs.sprite.order = legacy_packed_sorter_key(total_order, sprite_layer);
                configure_sprite_3d(&mut rs.sprite, &info, worlds, ctx.screen_w, ctx.screen_h);
                if let Some(parent) = parent_state {
                    apply_parent_render_state_to_sprite(&mut rs.sprite, &info, &parent);
                }
                finalize_object_center_rep_to_sprite(&mut rs.sprite, &info);
                apply_world_camera_mode(&mut rs.sprite, worlds, ctx.screen_w, ctx.screen_h);
                apply_runtime_light_and_fog(ctx, &mut rs.sprite);
                if motion_trace {
                    object_motion_trace_emit(
                        ctx,
                        stage_idx,
                        obj_idx,
                        obj,
                        &info,
                        parent_visible,
                        visible,
                        local_tr,
                        total_order,
                        total_layer,
                        bound_len_for_trace,
                        motion_bind_dbg,
                        rs.sprite.tr > 0,
                        Some(&rs.sprite),
                    );
                }
                if rs.sprite.tr > 0 {
                    own_sprites.push(rs);
                }
            }
        }
    }

    if config_button_trace_enabled() && config_button_trace_object(obj) {
        debug_lines.push(format!(
            "[SG_DEBUG][CONFIG_BUTTON_TRACE][EMIT_DONE] stage={} obj_idx={} runtime_slot={} file={} out_len_now={} visible={} child_count={}",
            stage_idx,
            obj_idx,
            info.runtime_slot,
            obj.file_name.as_deref().unwrap_or("-"),
            own_sprites.len(),
            visible,
            obj.runtime.child_objects.len()
        ));
    }

    if debug_enabled && !obj.runtime.child_objects.is_empty() {
        debug_lines.push(format!(
            "[SG_DEBUG]       child_list op=93 len={}",
            obj.runtime.child_objects.len()
        ));
    }
    if sg_mwnd_object_trace_enabled() && !obj.runtime.child_objects.is_empty() {
        for (child_idx, child) in obj.runtime.child_objects.iter().enumerate() {
            let child_info = effective_object_info(ctx, stage_idx, child_idx, child);
            let child_bind = match &child.backend {
                globals::ObjectBackend::Gfx => ctx
                    .gfx
                    .object_sprite_binding(stage_idx, child_info.runtime_slot as i64),
                globals::ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                }
                | globals::ObjectBackend::String {
                    layer_id,
                    sprite_id,
                    ..
                }
                | globals::ObjectBackend::Movie {
                    layer_id,
                    sprite_id,
                    ..
                } => Some((*layer_id, *sprite_id)),
                globals::ObjectBackend::Number {
                    layer_id,
                    sprite_ids,
                }
                | globals::ObjectBackend::Weather {
                    layer_id,
                    sprite_ids,
                } => sprite_ids.first().copied().map(|sid| (*layer_id, sid)),
                globals::ObjectBackend::None => None,
            };
            debug_lines.push(format!(
                "[SG_DEBUG][MWND_OBJECT_TRACE]       child parent_slot={} parent_obj_idx={} child[{}] slot={} participates={} used={} type={} backend={:?} file={} disp={} pos=({}, {}) center=({}, {}, {}) center_rep=({}, {}, {}) final_pos=({}, {}, {}) order={} layer={} alpha={} tr={} nested_slot={:?} bind={:?} grandchildren={}",
                info.runtime_slot,
                obj_idx,
                child_idx,
                child_info.runtime_slot,
                object_participates_in_tree(child),
                child.used,
                child.object_type,
                child.backend,
                child.file_name.as_deref().unwrap_or("-"),
                child_info.disp,
                child_info.x,
                child_info.y,
                child_info.center_x,
                child_info.center_y,
                child_info.center_z,
                child_info.center_rep_x,
                child_info.center_rep_y,
                child_info.center_rep_z,
                child_info.x + child_info.x_rep + child_info.center_rep_x,
                child_info.y + child_info.y_rep + child_info.center_rep_y,
                child_info.z + child_info.z_rep + child_info.center_rep_z,
                child_info.order,
                child_info.layer,
                child_info.alpha,
                child_info.tr,
                child.nested_runtime_slot,
                child_bind,
                child.runtime.child_objects.len()
            ));
        }
    }

    let mut children: Vec<(usize, &globals::ObjectState)> = obj
        .runtime
        .child_objects
        .iter()
        .enumerate()
        .filter(|(_, child)| object_participates_in_tree(child))
        .collect();

    let tree_sort_type = if matches!(obj.object_type, 3 | 4 | 5) {
        // STRING/NUMBER/WEATHER create a fresh dummy sprite for CHILD, whose
        // child-sort mode remains DEFAULT regardless of the owning object.
        0
    } else {
        info.child_sort_type
    };
    match tree_sort_type {
        0 | 1 => {
            // DEFAULT is sorted at tree-flatten time because one child object
            // may expand to several sibling nodes (STRING/NUMBER/WEATHER).
            // NONE preserves insertion order.
        }
        2 => {
            children.sort_by(|(lhs_idx, lhs), (rhs_idx, rhs)| {
                object_tree_texture_key(ctx, stage_idx, *lhs_idx, lhs)
                    .cmp(&object_tree_texture_key(ctx, stage_idx, *rhs_idx, rhs))
            });
        }
        3 if obj.object_type == 0 => {
            children.sort_by_key(|(_, child)| object_tree_stored_axis(child, 0));
        }
        4 if obj.object_type == 0 => {
            children.sort_by_key(|(_, child)| {
                std::cmp::Reverse(object_tree_stored_axis(child, 0))
            });
        }
        5 if obj.object_type == 0 => {
            children.sort_by_key(|(_, child)| object_tree_stored_axis(child, 1));
        }
        6 if obj.object_type == 0 => {
            children.sort_by_key(|(_, child)| {
                std::cmp::Reverse(object_tree_stored_axis(child, 1))
            });
        }
        7 if obj.object_type == 0 => {
            children.sort_by_key(|(_, child)| object_tree_stored_axis(child, 2));
        }
        8 if obj.object_type == 0 => {
            children.sort_by_key(|(_, child)| {
                std::cmp::Reverse(object_tree_stored_axis(child, 2))
            });
        }
        3..=8 => {
            // In the original source, X/Y/Z child sorting is implemented only
            // for TYPE_NONE. Other object types append no children for these
            // modes.
            children.clear();
        }
        _ => {}
    }

    // C_elm_object::frame passes the composed TRP to every child. A hidden or
    // fully transparent parent therefore suppresses the complete descendant
    // tree, even when the parent node itself is only a dummy/container.
    let recurse_children = visible;
    let mut child_nodes = Vec::new();
    for (child_idx, child) in children {
        append_object_tree_nodes(
            ctx,
            worlds,
            stage_idx,
            child_idx,
            child,
            recurse_children,
            total_order,
            total_layer,
            Some(cur_parent_state),
            &mut child_nodes,
            object_keys,
            debug_lines,
        );
    }

    if !visible {
        return;
    }

    match obj.object_type {
        3 | 4 | 5 => {
            // STRING/NUMBER/WEATHER add every generated sprite directly to the
            // current parent, then add a separate dummy node for CHILD. This is
            // why they must not be collapsed into one contiguous object group.
            for rs in own_sprites {
                out.push(SiglusRenderNode::from_single_sprite(rs));
            }
            if !child_nodes.is_empty() {
                out.push(SiglusRenderNode::dummy(
                    total_order,
                    total_layer,
                    true,
                    child_nodes,
                ));
            }
        }
        0 => {
            // TYPE_NONE contributes only a dummy tree node and only when it has
            // children.
            if !child_nodes.is_empty() {
                out.push(SiglusRenderNode::dummy(
                    total_order,
                    total_layer,
                    tree_sort_type == 0,
                    child_nodes,
                ));
            }
        }
        _ => {
            if own_sprites.is_empty() && child_nodes.is_empty() {
                return;
            }
            let (sorter_order, sorter_layer) = own_sprites
                .first()
                .map(|rs| (rs.sorter_order, rs.sorter_layer))
                .unwrap_or((
                    total_order.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                    total_layer.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
                ));
            out.push(SiglusRenderNode {
                sorter_order,
                sorter_layer,
                sprites: own_sprites,
                sort_children_default: tree_sort_type == 0,
                children: child_nodes,
            });
        }
    }
}

fn append_weather_sprites(
    ctx: &CommandContext,
    worlds: Option<&Vec<globals::WorldState>>,
    _obj: &globals::ObjectState,
    info: &ObjectRenderInfo,
    total_order: i64,
    total_layer: i64,
    bound: &[RenderSprite],
    out: &mut Vec<RenderSprite>,
) {
    // `sync_weather_objects()` has already evaluated C_elm_object::weather_frame
    // into layer-backed particle sprites.  Preserve that local particle state
    // and apply the owning object's transform exactly once.
    for template in bound {
        if !template.sprite.visible || template.sprite.image_id.is_none() {
            continue;
        }
        let mut rs = template.clone();
        let local_x = rs.sprite.x;
        let local_y = rs.sprite.y;
        let local_scale_x = rs.sprite.scale_x;
        let local_scale_y = rs.sprite.scale_y;
        let weather_tr = rs.sprite.tr;

        apply_object_render_info_to_sprite(&mut rs.sprite, info);
        rs.sprite.x = (rs.sprite.x as i64 + local_x as i64)
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        rs.sprite.y = (rs.sprite.y as i64 + local_y as i64)
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        rs.sprite.scale_x *= local_scale_x;
        rs.sprite.scale_y *= local_scale_y;
        rs.sprite.tr = ((info.tr.clamp(0, 255) * weather_tr as i64) / 255)
            .clamp(0, 255) as u8;
        rs.set_sorter(total_order, total_layer);
        rs.sprite.order = legacy_packed_sorter_key(total_order, total_layer);
        configure_sprite_3d(&mut rs.sprite, info, worlds, ctx.screen_w, ctx.screen_h);
        if rs.sprite.tr > 0 {
            out.push(rs);
        }
    }
}

fn apply_object_render_info_to_sprite(sprite: &mut Sprite, info: &ObjectRenderInfo) {
    sprite.visible = info.disp;
    sprite.x = (info.x + info.x_rep).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    sprite.y = (info.y + info.y_rep).clamp(i32::MIN as i64, i32::MAX as i64) as i32;
    sprite.z = (info.z + info.z_rep) as f32;
    sprite.pivot_x = (info.center_x + info.center_rep_x) as f32;
    sprite.pivot_y = (info.center_y + info.center_rep_y) as f32;
    sprite.pivot_z = (info.center_z + info.center_rep_z) as f32;
    sprite.scale_x = info.scale_x as f32 / 1000.0;
    sprite.scale_y = info.scale_y as f32 / 1000.0;
    sprite.scale_z = info.scale_z as f32 / 1000.0;
    sprite.rotate = info.rotate_z as f32 * std::f32::consts::PI / 1800.0;
    sprite.rotate_x = info.rotate_x as f32 * std::f32::consts::PI / 1800.0;
    sprite.rotate_y = info.rotate_y as f32 * std::f32::consts::PI / 1800.0;
    sprite.alpha = info.alpha.clamp(0, 255) as u8;
    sprite.tr = ((info.tr.clamp(0, 255) * info.tr_rep.clamp(0, 255)) / 255).clamp(0, 255) as u8;
    sprite.mono = info.mono.clamp(0, 255) as u8;
    sprite.reverse = info.reverse.clamp(0, 255) as u8;
    sprite.bright = info.bright.clamp(0, 255) as u8;
    sprite.dark = info.dark.clamp(0, 255) as u8;
    sprite.color_rate = info.color_rate.clamp(0, 255) as u8;
    sprite.color_add_r = info.color_add_r.clamp(0, 255) as u8;
    sprite.color_add_g = info.color_add_g.clamp(0, 255) as u8;
    sprite.color_add_b = info.color_add_b.clamp(0, 255) as u8;
    sprite.color_r = info.color_r.clamp(0, 255) as u8;
    sprite.color_g = info.color_g.clamp(0, 255) as u8;
    sprite.color_b = info.color_b.clamp(0, 255) as u8;
    sprite.blend = info.blend;
    sprite.dst_clip = info.dst_clip;
}

fn finalize_object_center_rep_to_sprite(sprite: &mut Sprite, info: &ObjectRenderInfo) {
    let x = (sprite.x as i64 + info.center_rep_x).clamp(i32::MIN as i64, i32::MAX as i64);
    let y = (sprite.y as i64 + info.center_rep_y).clamp(i32::MIN as i64, i32::MAX as i64);
    sprite.x = x as i32;
    sprite.y = y as i32;
    sprite.z += info.center_rep_z as f32;
}

fn object_participates_in_tree(obj: &globals::ObjectState) -> bool {
    if obj.used {
        return true;
    }
    if !obj.runtime.child_objects.is_empty() {
        return true;
    }
    !matches!(obj.backend, globals::ObjectBackend::None)
}

fn mark_object_tree_sprite_keys(
    ctx: &CommandContext,
    stage_idx: i64,
    obj_idx: usize,
    obj: &globals::ObjectState,
    object_keys: &mut HashSet<(LayerId, SpriteId)>,
) {
    let runtime_slot = object_runtime_slot(obj_idx, obj);
    for rs in fetch_bound_render_sprites_any(ctx, stage_idx, runtime_slot, obj) {
        if let (Some(lid), Some(sid)) = (rs.layer_id, rs.sprite_id) {
            object_keys.insert((lid, sid));
        }
    }
    for (child_idx, child) in obj.runtime.child_objects.iter().enumerate() {
        mark_object_tree_sprite_keys(ctx, stage_idx, child_idx, child, object_keys);
    }
}

fn mark_mwnd_owned_sprite_keys(
    ctx: &CommandContext,
    stage_idx: i64,
    m: &globals::MwndState,
    object_keys: &mut HashSet<(LayerId, SpriteId)>,
) {
    for (idx, obj) in m.button_list.iter().enumerate() {
        mark_object_tree_sprite_keys(ctx, stage_idx, idx, obj, object_keys);
    }
    for (idx, obj) in m.face_list.iter().enumerate() {
        mark_object_tree_sprite_keys(ctx, stage_idx, idx, obj, object_keys);
    }
    for (idx, obj) in m.object_list.iter().enumerate() {
        mark_object_tree_sprite_keys(ctx, stage_idx, idx, obj, object_keys);
    }
}

#[derive(Debug)]
struct SiglusRenderNode {
    sorter_order: i32,
    sorter_layer: i32,
    sprites: Vec<RenderSprite>,
    sort_children_default: bool,
    children: Vec<SiglusRenderNode>,
}

impl SiglusRenderNode {
    fn from_single_sprite(rs: RenderSprite) -> Self {
        Self {
            sorter_order: rs.sorter_order,
            sorter_layer: rs.sorter_layer,
            sprites: vec![rs],
            sort_children_default: true,
            children: Vec::new(),
        }
    }

    fn dummy(
        sorter_order: i64,
        sorter_layer: i64,
        sort_children_default: bool,
        children: Vec<SiglusRenderNode>,
    ) -> Self {
        Self {
            sorter_order: sorter_order.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            sorter_layer: sorter_layer.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            sprites: Vec::new(),
            sort_children_default,
            children,
        }
    }

    fn set_stage_metadata(&mut self, stage_form_id: u32, wipe_order: i64, wipe_layer: i64) {
        for rs in &mut self.sprites {
            rs.set_wipe_sorter(wipe_order, wipe_layer);
            rs.set_stage_form_owner(stage_form_id);
        }
        for child in &mut self.children {
            child.set_stage_metadata(stage_form_id, wipe_order, wipe_layer);
        }
    }

    fn sprite_count(&self) -> usize {
        self.sprites.len()
            + self
                .children
                .iter()
                .map(SiglusRenderNode::sprite_count)
                .sum::<usize>()
    }

    fn flatten(mut self, out: &mut Vec<RenderSprite>) {
        out.extend(self.sprites);
        if self.sort_children_default {
            self.children.sort_by(siglus_render_node_cmp);
        }
        for child in self.children {
            child.flatten(out);
        }
    }
}

fn siglus_render_node_cmp(
    lhs: &SiglusRenderNode,
    rhs: &SiglusRenderNode,
) -> std::cmp::Ordering {
    (lhs.sorter_order, lhs.sorter_layer).cmp(&(rhs.sorter_order, rhs.sorter_layer))
}

fn collect_object_render_nodes(
    ctx: &CommandContext,
    stage_form_id: u32,
    worlds: Option<&Vec<globals::WorldState>>,
    stage_idx: i64,
    obj_idx: usize,
    obj: &globals::ObjectState,
    parent_visible: bool,
    parent_order: i64,
    parent_layer: i64,
    parent_state: Option<ParentRenderState>,
    wipe_order: i64,
    wipe_layer: i64,
    object_keys: &mut HashSet<(LayerId, SpriteId)>,
    debug: &mut Vec<String>,
) -> Vec<SiglusRenderNode> {
    if !object_participates_in_tree(obj) {
        return Vec::new();
    }

    let mut nodes = Vec::new();
    append_object_tree_nodes(
        ctx,
        worlds,
        stage_idx,
        obj_idx,
        obj,
        parent_visible,
        parent_order,
        parent_layer,
        parent_state,
        &mut nodes,
        object_keys,
        debug,
    );
    for node in &mut nodes {
        node.set_stage_metadata(stage_form_id, wipe_order, wipe_layer);
    }
    nodes
}

fn mwnd_parent_render_state_at(
    m: &globals::MwndState,
    window_x: i64,
    window_y: i64,
) -> ParentRenderState {
    ParentRenderState {
        // C++ C_elm_mwnd::frame builds the MWND render parent from a fresh
        // S_tnm_render_param and never assigns p_world before passing it to
        // C_elm_mwnd_waku::frame.  Waku buttons therefore remain 2D UI sprites
        // even if the MWND form has a WORLD value.  Do not inherit m.world here:
        // doing so routes message-window buttons through the 3D/depth path, which
        // makes their textures appear in the renderer chain while the final frame
        // can depth-test them away behind earlier quads.
        world_no: -1,
        pos_x: window_x as f32,
        pos_y: window_y as f32,
        pos_z: 0.0,
        center_rep_x: 0.0,
        center_rep_y: 0.0,
        center_rep_z: 0.0,
        scale_x: 1.0,
        scale_y: 1.0,
        scale_z: 1.0,
        rotate_x: 0.0,
        rotate_y: 0.0,
        rotate_z: 0.0,
        tr: 255,
        mono: 0,
        reverse: 0,
        bright: 0,
        dark: 0,
        color_rate: 0,
        color_r: 0,
        color_g: 0,
        color_b: 0,
        color_add_r: 0,
        color_add_g: 0,
        color_add_b: 0,
        blend: crate::layer::SpriteBlend::Normal,
        dst_clip: None,
        mask_image_id: None,
        mask_offset_x: 0,
        mask_offset_y: 0,
        tonecurve_image_id: None,
        tonecurve_row: 0.0,
        tonecurve_sat: 0.0,
    }
}

fn mwnd_parent_render_state(m: &globals::MwndState) -> ParentRenderState {
    let (x, y) = m.window_pos.unwrap_or((0, 0));
    mwnd_parent_render_state_at(m, x, y)
}

fn mwnd_window_rect_for_embedded(
    ctx: &CommandContext,
    m: &globals::MwndState,
) -> Option<(
    i64,
    i64,
    i64,
    i64,
    Option<crate::runtime::ui::MwndWindowRenderState>,
)> {
    let (x, y) = m.window_pos?;
    let (w, h) = m.window_size?;
    if w <= 0 || h <= 0 {
        return None;
    }
    let ui_state = ctx
        .ui
        .current_mwnd_window_render_state(ctx.screen_w, ctx.screen_h)
        .filter(|ui| ui.x as i64 == x && ui.y as i64 == y && ui.w as i64 == w && ui.h as i64 == h);
    Some((x, y, w, h, ui_state))
}

fn mwnd_anim_parent_from_ui_state(
    m: &globals::MwndState,
    ui: crate::runtime::ui::MwndWindowRenderState,
) -> ParentRenderState {
    let mut parent = mwnd_parent_render_state_at(m, 0, 0);
    parent.pos_x = ui.dx as f32;
    parent.pos_y = ui.dy as f32;
    parent.center_rep_x = ui.pivot_abs_x - ui.dx as f32;
    parent.center_rep_y = ui.pivot_abs_y - ui.dy as f32;
    parent.scale_x = ui.scale_x;
    parent.scale_y = ui.scale_y;
    parent.rotate_z = ui.rotate;
    parent.tr = ui.alpha as i32;
    parent
}

fn apply_mwnd_window_anim_parent(
    parent: ParentRenderState,
    anim_parent: Option<ParentRenderState>,
) -> ParentRenderState {
    match anim_parent {
        Some(anim) => compose_parent_render_state(anim, parent),
        None => parent,
    }
}

fn append_mwnd_embedded_object_list_groups(
    ctx: &CommandContext,
    stage_form_id: u32,
    worlds: Option<&Vec<globals::WorldState>>,
    stage_idx: i64,
    list: &[globals::ObjectState],
    parent: ParentRenderState,
    parent_order: i64,
    parent_layer: i64,
    wipe_order: i64,
    wipe_layer: i64,
    groups: &mut Vec<SiglusRenderNode>,
    object_keys: &mut HashSet<(LayerId, SpriteId)>,
    debug: &mut Vec<String>,
) {
    for (obj_idx, obj) in list.iter().enumerate() {
        groups.extend(collect_object_render_nodes(
            ctx,
            stage_form_id,
            worlds,
            stage_idx,
            obj_idx,
            obj,
            true,
            parent_order,
            parent_layer,
            Some(parent),
            wipe_order,
            wipe_layer,
            object_keys,
            debug,
        ));
    }
}

fn mwnd_button_parent_render_state(
    m: &globals::MwndState,
    button_idx: usize,
    window_x: i64,
    window_y: i64,
    window_w: i64,
    window_h: i64,
) -> ParentRenderState {
    let mut parent = mwnd_parent_render_state_at(m, window_x, window_y);
    let Some(&(pos_base, x, y)) = m.waku_button_layout.get(button_idx) else {
        return parent;
    };
    match pos_base {
        1 => {
            parent.pos_x += (window_w - x) as f32;
            parent.pos_y += y as f32;
        }
        2 => {
            parent.pos_x += x as f32;
            parent.pos_y += (window_h - y) as f32;
        }
        3 => {
            parent.pos_x += (window_w - x) as f32;
            parent.pos_y += (window_h - y) as f32;
        }
        _ => {
            parent.pos_x += x as f32;
            parent.pos_y += y as f32;
        }
    }
    parent
}

fn mwnd_face_parent_render_state(
    m: &globals::MwndState,
    face_idx: usize,
    window_x: i64,
    window_y: i64,
) -> ParentRenderState {
    let mut parent = mwnd_parent_render_state_at(m, window_x, window_y);
    if let Some(&(x, y)) = m.waku_face_pos.get(face_idx) {
        parent.pos_x += x as f32;
        parent.pos_y += y as f32;
    }
    parent
}

fn btnselitem_parent_render_state(item: &globals::BtnSelItemState) -> ParentRenderState {
    ParentRenderState {
        world_no: -1,
        pos_x: item.pos.0.saturating_add(item.animation_offset.0) as f32,
        pos_y: item.pos.1.saturating_add(item.animation_offset.1) as f32,
        pos_z: 0.0,
        center_rep_x: 0.0,
        center_rep_y: 0.0,
        center_rep_z: 0.0,
        scale_x: 1.0,
        scale_y: 1.0,
        scale_z: 1.0,
        rotate_x: 0.0,
        rotate_y: 0.0,
        rotate_z: 0.0,
        tr: if item.visible {
            item.animation_tr.unwrap_or(255).clamp(0, 255) as i32
        } else {
            0
        },
        mono: 0,
        reverse: 0,
        bright: 0,
        dark: 0,
        color_rate: 0,
        color_r: 0,
        color_g: 0,
        color_b: 0,
        color_add_r: 0,
        color_add_g: 0,
        color_add_b: 0,
        blend: crate::layer::SpriteBlend::Normal,
        dst_clip: None,
        mask_image_id: None,
        mask_offset_x: 0,
        mask_offset_y: 0,
        tonecurve_image_id: None,
        tonecurve_row: 0.0,
        tonecurve_sat: 0.0,
    }
}

fn append_btnselitem_groups(
    ctx: &CommandContext,
    stage_form_id: u32,
    worlds: Option<&Vec<globals::WorldState>>,
    stage_idx: i64,
    items: &[globals::BtnSelItemState],
    groups: &mut Vec<SiglusRenderNode>,
    object_keys: &mut HashSet<(LayerId, SpriteId)>,
    debug: &mut Vec<String>,
) {
    let wipe_order = ctx.tables.mwnd_render.order;
    let wipe_layer = 0;
    for (item_idx, item) in items.iter().enumerate() {
        if !item.visible {
            continue;
        }
        let parent = btnselitem_parent_render_state(item);
        for (obj_idx, obj) in item.generated_objects.iter().enumerate() {
            groups.extend(collect_object_render_nodes(
                ctx,
                stage_form_id,
                worlds,
                stage_idx,
                obj_idx,
                obj,
                true,
                wipe_order,
                0,
                Some(parent),
                wipe_order,
                wipe_layer,
                object_keys,
                debug,
            ));
        }
        for (obj_idx, obj) in item.object_list.iter().enumerate() {
            groups.extend(collect_object_render_nodes(
                ctx,
                stage_form_id,
                worlds,
                stage_idx,
                obj_idx,
                obj,
                true,
                wipe_order,
                0,
                Some(parent),
                wipe_order,
                wipe_layer,
                object_keys,
                debug,
            ));
        }
        if sg_render_tree_debug_enabled()
            && (item.generated_objects.len() + item.object_list.len()) == 0
        {
            debug.push(format!(
                "[SG_DEBUG]     btnselitem[{item_idx}] text_len={} visible={} pos=({}, {}) size=({}, {}) no_objects",
                item.text.chars().count(),
                item.visible,
                item.pos.0,
                item.pos.1,
                item.size.0,
                item.size.1,
            ));
        }
    }
}

#[derive(Clone)]
struct SelBtnSpriteVisual {
    component: u8,
    action_no: i64,
    state: i64,
    base_file: Option<String>,
    base_patno: i64,
    text_color: Option<(u8, u8, u8)>,
}

fn collect_selbtn_sprite_visuals_recursive(
    obj: &globals::ObjectState,
    component: u8,
    action_no: i64,
    state: i64,
    text_color: Option<(u8, u8, u8)>,
    map: &mut HashMap<(LayerId, SpriteId), SelBtnSpriteVisual>,
) {
    match &obj.backend {
        globals::ObjectBackend::Gfx | globals::ObjectBackend::None => {}
        globals::ObjectBackend::String {
            layer_id,
            shadow_sprite_id,
            fuchi_sprite_id,
            sprite_id,
            glyphs,
            ..
        } if component == 2 => {
            // The original text item owns three sprites per glyph.  Only body
            // sprites switch between normal/hit colours; shadow and fuchi
            // retain their configured colours.
            if glyphs.is_empty() {
                for (sid, text_component, color) in [
                    (*shadow_sprite_id, 4u8, None),
                    (*fuchi_sprite_id, 5u8, None),
                    (*sprite_id, 2u8, text_color),
                ] {
                    map.insert(
                        (*layer_id, sid),
                        SelBtnSpriteVisual {
                            component: text_component,
                            action_no,
                            state,
                            base_file: obj.file_name.clone(),
                            base_patno: obj.base.patno,
                            text_color: color,
                        },
                    );
                }
            } else {
                for glyph in glyphs {
                    for (sid, text_component, color) in [
                        (glyph.shadow_sprite_id, 4u8, None),
                        (glyph.fuchi_sprite_id, 5u8, None),
                        (glyph.body_sprite_id, 2u8, text_color),
                    ] {
                        map.insert(
                            (*layer_id, sid),
                            SelBtnSpriteVisual {
                                component: text_component,
                                action_no,
                                state,
                                base_file: obj.file_name.clone(),
                                base_patno: obj.base.patno,
                                text_color: color,
                            },
                        );
                    }
                }
            }
        }
        _ => {
            for (layer_id, sprite_id) in layer_backed_object_sprite_bindings(&obj.backend) {
                map.insert(
                    (layer_id, sprite_id),
                    SelBtnSpriteVisual {
                        component,
                        action_no,
                        state,
                        base_file: obj.file_name.clone(),
                        base_patno: obj.base.patno,
                        text_color,
                    },
                );
            }
        }
    }

    for child in &obj.runtime.child_objects {
        collect_selbtn_sprite_visuals_recursive(
            child,
            component,
            action_no,
            state,
            text_color,
            map,
        );
    }
}

fn apply_selbtn_item_visuals(ctx: &mut CommandContext, sprites: &mut [RenderSprite]) {
    let mut map: HashMap<(LayerId, SpriteId), SelBtnSpriteVisual> = HashMap::new();
    let template_no = ctx.globals.selbtn.template_no.max(0) as usize;
    let hit_color_no = ctx
        .globals
        .selbtn
        .saved_cur_param
        .map(|param| param[17])
        .or_else(|| {
            ctx.tables
                .sel_btn_templates
                .get(template_no)
                .map(|tmpl| tmpl.moji_hit_color)
        })
        .unwrap_or(-1);
    for st in ctx.globals.stage_forms.values() {
        for items in st.btnselitem_lists.values() {
            for item in items {
                if !item.visible {
                    continue;
                }
                for (obj_idx, obj) in item.generated_objects.iter().enumerate() {
                    let component = match obj_idx {
                        0 => 0,
                        1 => 1,
                        _ => 2,
                    };
                    let text_color_no = if component == 2
                        && item.button_state != TNM_BTN_STATE_NORMAL
                        && item.button_state != TNM_BTN_STATE_DISABLE
                        && hit_color_no >= 0
                    {
                        hit_color_no
                    } else {
                        item.color
                    };
                    let text_color = if component == 2 {
                        Some(ctx.gameexe_color(text_color_no))
                    } else {
                        None
                    };
                    collect_selbtn_sprite_visuals_recursive(
                        obj,
                        component,
                        item.button_action_no,
                        item.button_state,
                        text_color,
                        &mut map,
                    );
                }
                for obj in &item.object_list {
                    collect_selbtn_sprite_visuals_recursive(
                        obj,
                        3,
                        item.button_action_no,
                        item.button_state,
                        None,
                        &mut map,
                    );
                }
            }
        }
    }

    if map.is_empty() {
        return;
    }

    for rs in sprites.iter_mut() {
        let (Some(lid), Some(sid)) = (rs.layer_id, rs.sprite_id) else {
            continue;
        };
        let Some(visual) = map.get(&(lid, sid)).cloned() else {
            continue;
        };
        let pat = button_action_pattern(&ctx.tables, visual.action_no, visual.state);

        rs.sprite.x = rs.sprite.x.saturating_add(pat.rep_pos_x as i32);
        rs.sprite.y = rs.sprite.y.saturating_add(pat.rep_pos_y as i32);

        match visual.component {
            0 => {
                if let Some(file_name) = visual.base_file.as_deref().filter(|s| !s.is_empty()) {
                    let patno = visual
                        .base_patno
                        .saturating_add(pat.rep_pat_no)
                        .max(0) as u32;
                    let image_id = match ctx.images.load_g00(file_name, patno) {
                        Ok(id) => Some(id),
                        Err(_) => ctx.images.load_bg_frame(file_name, patno as usize).ok(),
                    };
                    if let Some(image_id) = image_id {
                        rs.sprite.image_id = Some(image_id);
                        if let Some(img) = ctx.images.get(image_id) {
                            rs.sprite.object_anchor = true;
                            rs.sprite.texture_center_x = img.center_x as f32;
                            rs.sprite.texture_center_y = img.center_y as f32;
                        }
                    }
                }
                rs.sprite.tr = ((rs.sprite.tr as i64 * pat.rep_tr.clamp(0, 255)) / 255)
                    .clamp(0, 255) as u8;
                rs.sprite.bright = (rs.sprite.bright as i64 + pat.rep_bright).clamp(0, 255) as u8;
                rs.sprite.dark = (rs.sprite.dark as i64 + pat.rep_dark).clamp(0, 255) as u8;
            }
            1 => {
                let (cfg_r, cfg_g, cfg_b, cfg_a) = ctx.syscom_filter_config_rgba();
                rs.sprite.alpha = 255;
                rs.sprite.tr = ((cfg_a as i64 * pat.rep_tr.clamp(0, 255)) / 255)
                    .clamp(0, 255) as u8;
                rs.sprite.alpha_test = true;
                rs.sprite.alpha_blend = true;
                rs.sprite.color_rate = 0;
                rs.sprite.color_add_r = cfg_r;
                rs.sprite.color_add_g = cfg_g;
                rs.sprite.color_add_b = cfg_b;
                rs.sprite.color_r = 0;
                rs.sprite.color_g = 0;
                rs.sprite.color_b = 0;
                rs.sprite.bright = 0;
                rs.sprite.dark = 0;
                rs.sprite.mask_mode = 0;
            }
            2 => {
                if let Some((r, g, b)) = visual.text_color {
                    // C_elm_btn_select_item::frame() supplies the selected
                    // colour to the body glyphs every frame.  Replace the
                    // cached normal RGB while preserving its coverage alpha.
                    rs.sprite.color_rate = 255;
                    rs.sprite.color_r = r;
                    rs.sprite.color_g = g;
                    rs.sprite.color_b = b;
                    rs.sprite.color_add_r = 0;
                    rs.sprite.color_add_g = 0;
                    rs.sprite.color_add_b = 0;
                    rs.sprite.bright = 0;
                    rs.sprite.dark = 0;
                }
            }
            _ => {}
        }
    }
}

fn append_mwnd_embedded_groups(
    ctx: &CommandContext,
    stage_form_id: u32,
    worlds: Option<&Vec<globals::WorldState>>,
    stage_idx: i64,
    m: &globals::MwndState,
    groups: &mut Vec<SiglusRenderNode>,
    object_keys: &mut HashSet<(LayerId, SpriteId)>,
    debug: &mut Vec<String>,
) {
    if ctx.globals.script.mwnd_disp_off_flag
        || ctx.globals.syscom.hide_mwnd.onoff
        || ctx.globals.syscom.msg_back_open {
        if config_button_trace_enabled() {
            debug.push(format!(
                "[SG_DEBUG][CONFIG_BUTTON_TRACE][MWND_SKIP] stage={} reason=hidden script_off={} sys_hide={} open={} buttons={} objects={} waku={} filter={} pos={:?} size={:?}",
                stage_idx,
                ctx.globals.script.mwnd_disp_off_flag,
                ctx.globals.syscom.hide_mwnd.onoff,
                m.open,
                m.button_list.len(),
                m.object_list.len(),
                if m.waku_file.is_empty() { "-" } else { m.waku_file.as_str() },
                if m.filter_file.is_empty() { "-" } else { m.filter_file.as_str() },
                m.window_pos,
                m.window_size
            ));
        }
        return;
    }
    let Some((window_x, window_y, window_w, window_h, ui_state)) =
        mwnd_window_rect_for_embedded(ctx, m)
    else {
        if config_button_trace_enabled() {
            debug.push(format!(
                "[SG_DEBUG][CONFIG_BUTTON_TRACE][MWND_SKIP] stage={} reason=no_window_rect open={} buttons={} objects={} waku={} filter={} pos={:?} size={:?}",
                stage_idx,
                m.open,
                m.button_list.len(),
                m.object_list.len(),
                if m.waku_file.is_empty() { "-" } else { m.waku_file.as_str() },
                if m.filter_file.is_empty() { "-" } else { m.filter_file.as_str() },
                m.window_pos,
                m.window_size
            ));
        }
        return;
    };
    if !m.open && ui_state.is_none() {
        if config_button_trace_enabled() {
            debug.push(format!(
                "[SG_DEBUG][CONFIG_BUTTON_TRACE][MWND_SKIP] stage={} reason=closed_no_anim open={} buttons={} objects={} waku={} filter={} rect=({}, {}, {}, {})",
                stage_idx,
                m.open,
                m.button_list.len(),
                m.object_list.len(),
                if m.waku_file.is_empty() { "-" } else { m.waku_file.as_str() },
                if m.filter_file.is_empty() { "-" } else { m.filter_file.as_str() },
                window_x, window_y, window_w, window_h
            ));
        }
        return;
    }
    let mwnd_order_source = if m.order <= 0 {
        ctx.tables.mwnd_render.order.max(1)
    } else {
        m.order
    };
    let mwnd_order = mwnd_order_source;
    let mwnd_layer = m.layer;
    let anim_parent = ui_state.map(|ui| mwnd_anim_parent_from_ui_state(m, ui));
    if config_button_trace_enabled() {
        debug.push(format!(
            "[SG_DEBUG][CONFIG_BUTTON_TRACE][MWND_COLLECT] stage={} open={} buttons={} faces={} objects={} waku={} filter={} rect=({}, {}, {}, {}) ui_anim={} order={} layer={} hide_flags=(script:{},sys:{})",
            stage_idx,
            m.open,
            m.button_list.len(),
            m.face_list.len(),
            m.object_list.len(),
            if m.waku_file.is_empty() { "-" } else { m.waku_file.as_str() },
            if m.filter_file.is_empty() { "-" } else { m.filter_file.as_str() },
            window_x, window_y, window_w, window_h,
            anim_parent.is_some(),
            mwnd_order,
            mwnd_layer,
            ctx.globals.script.mwnd_disp_off_flag,
            ctx.globals.syscom.hide_mwnd.onoff
        ));
    }
    for (button_idx, obj) in m.button_list.iter().enumerate() {
        if !object_participates_in_tree(obj) {
            if config_button_trace_enabled() {
                debug.push(format!(
                    "[SG_DEBUG][CONFIG_BUTTON_TRACE][MWND_BUTTON_SKIP] stage={} button_idx={} reason=not_participating file={} used={} type={} disp={} backend={:?}",
                    stage_idx,
                    button_idx,
                    obj.file_name.as_deref().unwrap_or("-"),
                    obj.used,
                    obj.object_type,
                    obj.base.disp,
                    obj.backend
                ));
            }
            continue;
        }
        let local_parent =
            mwnd_button_parent_render_state(m, button_idx, window_x, window_y, window_w, window_h);
        let parent = apply_mwnd_window_anim_parent(local_parent, anim_parent);
        if sg_render_tree_debug_enabled() {
            debug.push(format!(
                "[SG_DEBUG]       mwnd_button_parent[{}] file={} pos=({}, {}) local_base={:?} order={} layer={}",
                button_idx,
                obj.file_name.as_deref().unwrap_or("-"),
                parent.pos_x,
                parent.pos_y,
                m.waku_button_layout.get(button_idx),
                mwnd_order,
                mwnd_layer.saturating_add(ctx.tables.mwnd_render.waku_layer_rep),
            ));
        }
        groups.extend(collect_object_render_nodes(
            ctx,
            stage_form_id,
            worlds,
            stage_idx,
            button_idx,
            obj,
            true,
            mwnd_order,
            mwnd_layer.saturating_add(ctx.tables.mwnd_render.waku_layer_rep),
            Some(parent),
            mwnd_order,
            mwnd_layer,
            object_keys,
            debug,
        ));
    }
    for (face_idx, obj) in m.face_list.iter().enumerate() {
        if !object_participates_in_tree(obj) {
            continue;
        }
        let parent = apply_mwnd_window_anim_parent(
            mwnd_face_parent_render_state(m, face_idx, window_x, window_y),
            anim_parent,
        );
        groups.extend(collect_object_render_nodes(
            ctx,
            stage_form_id,
            worlds,
            stage_idx,
            face_idx,
            obj,
            true,
            mwnd_order,
            mwnd_layer.saturating_add(ctx.tables.mwnd_render.face_layer_rep),
            Some(parent),
            mwnd_order,
            mwnd_layer,
            object_keys,
            debug,
        ));
    }
    let parent = apply_mwnd_window_anim_parent(
        mwnd_parent_render_state_at(m, window_x, window_y),
        anim_parent,
    );
    append_mwnd_embedded_object_list_groups(
        ctx,
        stage_form_id,
        worlds,
        stage_idx,
        &m.object_list,
        parent,
        mwnd_order,
        mwnd_layer,
        mwnd_order,
        mwnd_layer,
        groups,
        object_keys,
        debug,
    );
}

fn mwnd_sort_base(
    ctx: &CommandContext,
    m: &globals::MwndState,
) -> (i64, i64) {
    let order = if m.order <= 0 {
        ctx.tables.mwnd_render.order.max(1)
    } else {
        m.order
    };
    (order, m.layer)
}

fn selected_mwnd_sort_base(ctx: &CommandContext) -> Option<(i64, i64)> {
    if let Some((focused_form, focused_stage, focused_idx)) = ctx.globals.focused_stage_mwnd {
        if let Some(m) = ctx
            .globals
            .stage_forms
            .get(&focused_form)
            .and_then(|st| st.mwnd_lists.get(&focused_stage))
            .and_then(|list| list.get(focused_idx))
            .filter(|m| m.open)
        {
            return Some(mwnd_sort_base(ctx, m));
        }
    }

    let mut form_ids: Vec<u32> = ctx.globals.stage_forms.keys().copied().collect();
    form_ids.sort_unstable();
    for form_id in form_ids {
        let Some(st) = ctx.globals.stage_forms.get(&form_id) else {
            continue;
        };
        let mut stage_ids: Vec<i64> = st.mwnd_lists.keys().copied().collect();
        stage_ids.sort_unstable();
        for stage_idx in stage_ids {
            let Some(list) = st.mwnd_lists.get(&stage_idx) else {
                continue;
            };
            for m in list {
                if m.open {
                    return Some(mwnd_sort_base(ctx, m));
                }
            }
        }
    }
    None
}

fn normalize_mwnd_ui_sprite_sorter(
    ctx: &CommandContext,
    layer_id: Option<LayerId>,
    order: i32,
) -> (i32, i32) {
    if let Some(sorter) = ctx.ui.mwnd_sorter_for_ui_layer(
        layer_id,
        order,
        ctx.tables.mwnd_render.waku_layer_rep,
        ctx.tables.mwnd_render.filter_layer_rep,
        ctx.tables.mwnd_render.face_layer_rep,
        ctx.tables.mwnd_render.shadow_layer_rep,
        ctx.tables.mwnd_render.fuchi_layer_rep,
        ctx.tables.mwnd_render.moji_layer_rep,
    ) {
        return sorter;
    }
    let Some((mwnd_order, mwnd_layer)) = selected_mwnd_sort_base(ctx) else {
        return unpack_legacy_sorter_key(order);
    };
    let layer = match order {
        1_000_000 | 1_000_030 => {
            mwnd_layer.saturating_add(ctx.tables.mwnd_render.waku_layer_rep)
        }
        1_000_005 => mwnd_layer.saturating_add(ctx.tables.mwnd_render.filter_layer_rep),
        1_000_008 => mwnd_layer.saturating_add(ctx.tables.mwnd_render.face_layer_rep),
        1_000_010 | 1_000_020 => {
            mwnd_layer.saturating_add(ctx.tables.mwnd_render.shadow_layer_rep)
        }
        1_000_011 | 1_000_021 => {
            mwnd_layer.saturating_add(ctx.tables.mwnd_render.fuchi_layer_rep)
        }
        1_000_012 | 1_000_013 | 1_000_022 => {
            mwnd_layer.saturating_add(ctx.tables.mwnd_render.moji_layer_rep)
        }
        _ => return unpack_legacy_sorter_key(order),
    };
    (
        mwnd_order.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        layer.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
    )
}

const TNM_STAGE_FRONT_I64: i64 = 1;
const TNM_SEL_ITEM_TYPE_ON_I64: i64 = 1;
const TNM_SEL_ITEM_TYPE_READ_I64: i64 = 2;
const TNM_STAGE_NEXT_I64: i64 = 2;
const EXCALL_LOCAL_NS_XOR: u32 = 0x4000;
const INIDEF_EXCALL_ORDER: i64 = 20_000;

fn excall_stage_form_id(ctx: &CommandContext) -> u32 {
    let normal = if ctx.ids.form_global_stage != 0 {
        ctx.ids.form_global_stage
    } else {
        crate::runtime::forms::codes::FORM_GLOBAL_STAGE
    };
    normal ^ EXCALL_LOCAL_NS_XOR
}

fn is_excall_stage_form(ctx: &CommandContext, form_id: u32) -> bool {
    ctx.excall_state.ready && form_id == excall_stage_form_id(ctx)
}

fn configured_excall_order(ctx: &CommandContext) -> i64 {
    ctx.tables
        .gameexe
        .as_ref()
        .and_then(|cfg| {
            cfg.get_i64("#EXCALL.ORDER")
                .or_else(|| cfg.get_i64("EXCALL.ORDER"))
        })
        .unwrap_or(INIDEF_EXCALL_ORDER)
}

fn mark_all_stage_owned_sprite_keys(
    ctx: &CommandContext,
    object_keys: &mut HashSet<(LayerId, SpriteId)>,
) {
    let mut form_ids: Vec<u32> = ctx.globals.stage_forms.keys().copied().collect();
    form_ids.sort_unstable();
    for form_id in form_ids {
        let Some(st) = ctx.globals.stage_forms.get(&form_id) else {
            continue;
        };

        let mut stage_ids: Vec<i64> = st
            .object_lists
            .keys()
            .chain(st.mwnd_lists.keys())
            .chain(st.btnselitem_lists.keys())
            .copied()
            .collect();
        stage_ids.sort_unstable();
        stage_ids.dedup();

        for stage_idx in stage_ids {
            if let Some(list) = st.object_lists.get(&stage_idx) {
                for (obj_idx, obj) in list.iter().enumerate() {
                    mark_object_tree_sprite_keys(ctx, stage_idx, obj_idx, obj, object_keys);
                }
            }
            if let Some(mwnds) = st.mwnd_lists.get(&stage_idx) {
                for m in mwnds {
                    mark_mwnd_owned_sprite_keys(ctx, stage_idx, m, object_keys);
                }
            }
            if let Some(items) = st.btnselitem_lists.get(&stage_idx) {
                for item in items {
                    for (obj_idx, obj) in item.generated_objects.iter().enumerate() {
                        mark_object_tree_sprite_keys(ctx, stage_idx, obj_idx, obj, object_keys);
                    }
                    for (obj_idx, obj) in item.object_list.iter().enumerate() {
                        mark_object_tree_sprite_keys(ctx, stage_idx, obj_idx, obj, object_keys);
                    }
                }
            }
        }
    }
}

fn build_siglus_object_render_list(
    ctx: &CommandContext,
    base: &[RenderSprite],
    selected_stage: i64,
) -> (Vec<RenderSprite>, Vec<String>) {
    let debug_enabled = sg_render_tree_debug_enabled();
    let mut object_keys: HashSet<(LayerId, SpriteId)> = HashSet::new();
    // Original Siglus builds the draw list from C_elm_stage::get_sprite_tree()
    // for the selected stage. LayerManager is only a backend storage cache here;
    // object-owned backing sprites from BACK/NEXT or hidden objects must not leak
    // through the generic layer render list.
    mark_all_stage_owned_sprite_keys(ctx, &mut object_keys);
    let focused_mwnd = ctx.globals.focused_stage_mwnd;
    let mut render_nodes: Vec<SiglusRenderNode> = Vec::new();
    let mut debug = Vec::new();
    if config_button_trace_enabled() {
        debug.push(format!(
            "[SG_DEBUG][CONFIG_BUTTON_TRACE][BUILD] selected_stage={} focused_mwnd={:?} base_len={} wipe_active={}",
            selected_stage,
            ctx.globals.focused_stage_mwnd,
            base.len(),
            ctx.globals.wipe.is_some()
        ));
    }

    let mut form_ids: Vec<u32> = ctx.globals.stage_forms.keys().copied().collect();
    form_ids.sort_unstable();
    for form_id in form_ids {
        let Some(st) = ctx.globals.stage_forms.get(&form_id) else {
            continue;
        };
        if debug_enabled {
            debug.push(format!("[SG_DEBUG] stage_form {}", form_id));
        }
        let mut stage_ids: Vec<i64> = st
            .object_lists
            .keys()
            .chain(st.mwnd_lists.keys())
            .chain(st.group_lists.keys())
            .chain(st.btnselitem_lists.keys())
            .chain(st.world_lists.keys())
            .chain(st.effect_lists.keys())
            .chain(st.quake_lists.keys())
            .copied()
            .collect();
        stage_ids.sort_unstable();
        stage_ids.dedup();
        for stage_idx in stage_ids {
            let worlds = st.world_lists.get(&stage_idx);
            if let Some(mwnds) = st.mwnd_lists.get(&stage_idx) {
                for m in mwnds {
                    mark_mwnd_owned_sprite_keys(ctx, stage_idx, m, &mut object_keys);
                }
            }

            let active_cnt = st
                .object_lists
                .get(&stage_idx)
                .map(|list| {
                    list.iter()
                        .enumerate()
                        .filter(|(obj_idx, o)| {
                            !st.is_embedded_object_slot(stage_idx, *obj_idx)
                                && object_participates_in_tree(o)
                        })
                        .count()
                })
                .unwrap_or(0);
            let mwnd_embedded_cnt = st
                .mwnd_lists
                .get(&stage_idx)
                .map(|mwnds| {
                    mwnds
                        .iter()
                        .map(|m| m.button_list.len() + m.face_list.len() + m.object_list.len())
                        .sum::<usize>()
                })
                .unwrap_or(0);
            let group_cnt = st.group_lists.get(&stage_idx).map(|v| v.len()).unwrap_or(0);
            let btnselitem_cnt = st
                .btnselitem_lists
                .get(&stage_idx)
                .map(|v| v.len())
                .unwrap_or(0);
            let world_cnt = st.world_lists.get(&stage_idx).map(|v| v.len()).unwrap_or(0);
            let effect_cnt = st
                .effect_lists
                .get(&stage_idx)
                .map(|v| v.len())
                .unwrap_or(0);
            let quake_cnt = st.quake_lists.get(&stage_idx).map(|v| v.len()).unwrap_or(0);
            if active_cnt == 0
                && mwnd_embedded_cnt == 0
                && group_cnt == 0
                && btnselitem_cnt == 0
                && world_cnt == 0
                && effect_cnt == 0
                && quake_cnt == 0
            {
                continue;
            }
            if debug_enabled {
                debug.push(format!(
                    "[SG_DEBUG]   stage {} active_objects={} mwnd_embedded={} groups={} btnselitems={} worlds={} effects={} quakes={}",
                    stage_idx, active_cnt, mwnd_embedded_cnt, group_cnt, btnselitem_cnt, world_cnt, effect_cnt, quake_cnt
                ));
                if let Some(effects) = st.effect_lists.get(&stage_idx) {
                    for (effect_idx, effect) in effects.iter().enumerate() {
                        debug.push(format!(
                            "[SG_DEBUG]     effect[{}] range=({},{})->({},{}) wipe_copy={} wipe_erase={} xy=({}, {}) color_rate={} bright={} dark={} tr-like-mono={}",
                            effect_idx,
                            effect.begin_order,
                            effect.begin_layer,
                            effect.end_order,
                            effect.end_layer,
                            effect.wipe_copy,
                            effect.wipe_erase,
                            effect.x.get_total_value(),
                            effect.y.get_total_value(),
                            effect.color_rate.get_total_value(),
                            effect.bright.get_total_value(),
                            effect.dark.get_total_value(),
                            effect.mono.get_total_value(),
                        ));
                    }
                }
                if let Some(quakes) = st.quake_lists.get(&stage_idx) {
                    for (quake_idx, quake) in quakes.iter().enumerate() {
                        debug.push(format!(
                            "[SG_DEBUG]     quake[{}] type={} power={} vec={} center=({}, {}) order_range={}..{} active={}",
                            quake_idx,
                            quake.quake_type,
                            quake.power,
                            quake.vec,
                            quake.center_x,
                            quake.center_y,
                            quake.begin_order,
                            quake.end_order,
                            quake.is_active(),
                        ));
                    }
                }
            }
            if stage_idx != selected_stage {
                if config_button_trace_enabled() {
                    let mwnd_summary = st.mwnd_lists.get(&stage_idx).map(|mwnds| {
                        mwnds.iter().enumerate().map(|(idx, m)| {
                            format!(
                                "{}:open={} buttons={} objects={} waku={} filter={} pos={:?} size={:?}",
                                idx,
                                m.open,
                                m.button_list.len(),
                                m.object_list.len(),
                                if m.waku_file.is_empty() { "-" } else { m.waku_file.as_str() },
                                if m.filter_file.is_empty() { "-" } else { m.filter_file.as_str() },
                                m.window_pos,
                                m.window_size
                            )
                        }).collect::<Vec<_>>()
                    }).unwrap_or_default();
                    debug.push(format!(
                        "[SG_DEBUG][CONFIG_BUTTON_TRACE][STAGE_SKIP] form={} stage={} selected_stage={} active_objects={} mwnd_embedded={} mwnds={:?} focused_mwnd={:?}",
                        form_id,
                        stage_idx,
                        selected_stage,
                        active_cnt,
                        mwnd_embedded_cnt,
                        mwnd_summary,
                        focused_mwnd
                    ));
                }
                // C_elm_stage::get_sprite_tree() only returns objects owned by
                // that concrete stage.  Focus controls input/message routing;
                // it must never inject FRONT MWND children into a NEXT render
                // tree (or vice versa), otherwise the wipe textures contain
                // two copies of the same window/face/button sprites.
                continue;
            }
            if let Some(list) = st.object_lists.get(&stage_idx) {
                // C_elm_stage::frame(..., order) passes Gp_ini->excall_order
                // only to top-level OBJECTs of the EXCALL stage. Children inherit
                // that parent sorter through C_elm_object::frame(). MWND/BTNSEL
                // are intentionally not offset here because the original stage
                // frame calls their frame methods without the EXCALL order value.
                let stage_parent_order = if is_excall_stage_form(ctx, form_id) {
                    configured_excall_order(ctx)
                } else {
                    0
                };
                for (obj_idx, obj) in list.iter().enumerate() {
                    if st.is_embedded_object_slot(stage_idx, obj_idx)
                        || !object_participates_in_tree(obj)
                    {
                        continue;
                    }
                    let info = effective_object_info(ctx, stage_idx, obj_idx, obj);
                    let top_order = stage_parent_order.saturating_add(info.order);
                    render_nodes.extend(collect_object_render_nodes(
                        ctx,
                        form_id,
                        worlds,
                        stage_idx,
                        obj_idx,
                        obj,
                        true,
                        stage_parent_order,
                        0,
                        None,
                        top_order,
                        info.layer,
                        &mut object_keys,
                        &mut debug,
                    ));
                }
            }
            if let Some(mwnds) = st.mwnd_lists.get(&stage_idx) {
                for (mwnd_idx, m) in mwnds.iter().enumerate() {
                    if debug_enabled {
                        let embedded_cnt =
                            m.button_list.len() + m.face_list.len() + m.object_list.len();
                        if m.open
                            || embedded_cnt != 0
                            || !m.msg_text.is_empty()
                            || !m.name_text.is_empty()
                            || m.selection.is_some()
                        {
                            debug.push(format!(
                                "[SG_DEBUG]     mwnd[{mwnd_idx}] open={} order={} layer={} world={} msg_len={} name_len={} embedded={} button={} face={} object={} waku={} filter={} face_file={} open_anim=({}, {}) close_anim=({}, {}) selection={} hide_flags=(script:{},sys:{})",
                                m.open,
                                m.order,
                                m.layer,
                                m.world,
                                m.msg_text.chars().count(),
                                m.name_text.chars().count(),
                                embedded_cnt,
                                m.button_list.len(),
                                m.face_list.len(),
                                m.object_list.len(),
                                if m.waku_file.is_empty() { "-" } else { m.waku_file.as_str() },
                                if m.filter_file.is_empty() { "-" } else { m.filter_file.as_str() },
                                if m.face_file.is_empty() { "-" } else { m.face_file.as_str() },
                                m.open_anime_type,
                                m.open_anime_time,
                                m.close_anime_type,
                                m.close_anime_time,
                                m.selection.is_some(),
                                ctx.globals.script.mwnd_disp_off_flag,
                                ctx.globals.syscom.hide_mwnd.onoff,
                            ));
                        }
                    }
                    append_mwnd_embedded_groups(
                        ctx,
                        form_id,
                        worlds,
                        stage_idx,
                        m,
                        &mut render_nodes,
                        &mut object_keys,
                        &mut debug,
                    );
                }
            }
            if let Some(items) = st.btnselitem_lists.get(&stage_idx) {
                append_btnselitem_groups(
                    ctx,
                    form_id,
                    worlds,
                    stage_idx,
                    items,
                    &mut render_nodes,
                    &mut object_keys,
                    &mut debug,
                );
            }
        }
    }

    let mut bg = Vec::new();
    let mut rest = Vec::new();
    for rs in base {
        if let Some(mwnd_stage) = ctx.ui.mwnd_stage_for_ui_layer(rs.layer_id) {
            if mwnd_stage != selected_stage {
                continue;
            }
        }
        match (rs.layer_id, rs.sprite_id) {
            (Some(lid), Some(sid)) if object_keys.contains(&(lid, sid)) => {}
            (None, None) if render_sprite_visible_for_submit(rs) => bg.push(rs.clone()),
            (None, None) => {}
            _ if render_sprite_visible_for_submit(rs) => rest.push(rs.clone()),
            _ => {}
        }
    }

    for mut rs in rest {
        // LayerManager ids are storage handles. They are not Siglus script-layer
        // values. For MWND UI-runtime sprites, translate the sentinel order into
        // the same C++ S_tnm_sorter(order, layer) pair that C_elm_mwnd_waku uses.
        let (order, layer) =
            normalize_mwnd_ui_sprite_sorter(ctx, rs.layer_id, rs.sprite.order);
        rs.set_sorter(order as i64, layer as i64);
        rs.sprite.order = legacy_packed_sorter_key(order as i64, layer as i64);
        if let Some((wipe_order, wipe_layer)) =
            ctx.ui.mwnd_base_sorter_for_ui_layer(rs.layer_id)
        {
            rs.set_wipe_sorter(wipe_order as i64, wipe_layer as i64);
        }
        if let Some((form_id, _)) = ctx.ui.mwnd_owner_for_ui_layer(rs.layer_id) {
            rs.set_stage_form_owner(form_id);
        }
        render_nodes.push(SiglusRenderNode::from_single_sprite(rs));
    }

    // C_tnm_wnd::disp_proc_sprite_tree_to_sprite_list() stable-sorts only the
    // root node's immediate children, then recursively flattens each subtree.
    // Keep every object subtree contiguous; globally sorting all descendant
    // sprites changes CHILD_SORT_TYPE_NONE/custom ordering and lets descendants
    // cross unrelated top-level objects.
    render_nodes.sort_by(siglus_render_node_cmp);
    let sprite_count = render_nodes
        .iter()
        .map(SiglusRenderNode::sprite_count)
        .sum::<usize>();
    let mut final_list = Vec::with_capacity(bg.len() + sprite_count);
    final_list.extend(bg);
    for node in render_nodes {
        node.flatten(&mut final_list);
    }
    (final_list, debug)
}

fn trace_codes_enabled() -> bool {
    trace_env().siglus_trace_codes
}

pub fn dispatch_form_code(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    ctx.images
        .set_current_append_dir(ctx.globals.append_dir.clone());
    ctx.movie
        .set_current_append_dir(ctx.globals.append_dir.clone());
    ctx.bgm
        .set_current_append_dir(ctx.globals.append_dir.clone());

    let code = opcode::OpCode::form(form_id);
    if trace_codes_enabled() {
        let chain = ctx
            .vm_call
            .as_ref()
            .map(|call| call.element.clone())
            .unwrap_or_default();
        eprintln!(
            "[TRACE code] form={} chain={chain:?} argc={} args={args:?}",
            form_id,
            args.len()
        );
    }

    opcode::dispatch_code(ctx, code, args)
}

/// Dispatch a decoded numeric form command.
pub fn dispatch(ctx: &mut CommandContext, cmd: &Command) -> Result<()> {
    let Some(code) = cmd.code else {
        anyhow::bail!(
            "name-only command dispatch is invalid: {}; Siglus user commands must run through SceneVm",
            cmd.name
        );
    };

    if !dispatch_form_code(ctx, code.id, &cmd.args)? {
        anyhow::bail!("unhandled form code {}", code.id);
    }
    Ok(())
}

fn apply_button_visuals(ctx: &mut CommandContext, sprites: &mut [RenderSprite]) {
    let mut map: HashMap<(LayerId, SpriteId), ButtonVisualState> = HashMap::new();

    let mut form_ids: Vec<u32> = ctx.globals.stage_forms.keys().copied().collect();
    form_ids.sort_unstable();
    for form_id in form_ids {
        let Some(st) = ctx.globals.stage_forms.get(&form_id) else {
            continue;
        };
        let mut stage_ids: Vec<i64> = st
            .object_lists
            .keys()
            .chain(st.mwnd_lists.keys())
            .chain(st.btnselitem_lists.keys())
            .copied()
            .collect();
        stage_ids.sort_unstable();
        stage_ids.dedup();
        for stage_idx in stage_ids {
            if let Some(objs) = st.object_lists.get(&stage_idx) {
                for (obj_idx, obj) in objs.iter().enumerate() {
                    collect_button_visuals_recursive(
                        ctx, st, stage_idx, obj_idx, obj, &mut map, None, None,
                    );
                }
            }
            if let Some(mwnds) = st.mwnd_lists.get(&stage_idx) {
                for m in mwnds {
                    for (obj_idx, obj) in m.button_list.iter().enumerate() {
                        collect_button_visuals_recursive(
                            ctx,
                            st,
                            stage_idx,
                            obj_idx,
                            obj,
                            &mut map,
                            None,
                            Some(obj_idx),
                        );
                    }
                    for (obj_idx, obj) in m.face_list.iter().enumerate() {
                        collect_button_visuals_recursive(
                            ctx, st, stage_idx, obj_idx, obj, &mut map, None, None,
                        );
                    }
                    for (obj_idx, obj) in m.object_list.iter().enumerate() {
                        collect_button_visuals_recursive(
                            ctx, st, stage_idx, obj_idx, obj, &mut map, None, None,
                        );
                    }
                }
            }
            if let Some(items) = st.btnselitem_lists.get(&stage_idx) {
                for item in items {
                    for (obj_idx, obj) in item.generated_objects.iter().enumerate() {
                        collect_button_visuals_recursive(
                            ctx, st, stage_idx, obj_idx, obj, &mut map, None, None,
                        );
                    }
                    for (obj_idx, obj) in item.object_list.iter().enumerate() {
                        collect_button_visuals_recursive(
                            ctx, st, stage_idx, obj_idx, obj, &mut map, None, None,
                        );
                    }
                }
            }
        }
    }

    if map.is_empty() {
        return;
    }

    for rs in sprites.iter_mut() {
        let (Some(lid), Some(sid)) = (rs.layer_id, rs.sprite_id) else {
            continue;
        };
        let Some(visual) = map.get(&(lid, sid)).cloned() else {
            continue;
        };
        apply_button_state_visual(&ctx.tables, &mut ctx.images, &mut rs.sprite, visual);
    }
}

fn collect_button_visuals_recursive(
    ctx: &CommandContext,
    st: &globals::StageFormState,
    stage_idx: i64,
    obj_idx: usize,
    obj: &globals::ObjectState,
    map: &mut HashMap<(LayerId, SpriteId), ButtonVisualState>,
    inherited_visual: Option<ButtonVisualState>,
    mwnd_button_idx: Option<usize>,
) {
    use globals::ObjectBackend;

    let mut effective_visual = inherited_visual;
    if obj.button.enabled || obj.button.state == TNM_BTN_STATE_DISABLE {
        if !button_syscom_mode_visible(&ctx.globals.syscom, &obj.button) {
            effective_visual = None;
        } else {
            let state = button_real_state_for_visual(
                &ctx.globals.syscom,
                st,
                stage_idx,
                obj,
                mwnd_button_idx,
            );
            if sg_debug_enabled() {
                let runtime_slot = object_runtime_slot(obj_idx, obj);
                eprintln!(
                    "[SG_DEBUG][BUTTON_TRACE][VISUAL] collect stage={} obj_idx={} runtime_slot={} file={:?} mwnd_button_idx={:?} state={}({}) raw_state={} enabled={} visible={} disabled_reason={:?} button_no={} group_no={} group_idx={:?} action_no={} cut_no={} hit={} pushed={} sys_type={} sys_opt={} mode={} call={}::{}/{}",
                    stage_idx,
                    obj_idx,
                    runtime_slot,
                    obj.file_name,
                    mwnd_button_idx,
                    state,
                    button_state_name(state),
                    obj.button.state,
                    obj.button.enabled,
                    button_syscom_mode_visible(&ctx.globals.syscom, &obj.button),
                    button_disabled_reason(&ctx.globals.syscom, obj, mwnd_button_idx),
                    obj.button.button_no,
                    obj.button.group_no,
                    obj.button.group_idx(),
                    obj.button.action_no,
                    obj.button.cut_no,
                    obj.button.hit,
                    obj.button.pushed,
                    obj.button.sys_type,
                    obj.button.sys_type_opt,
                    obj.button.mode,
                    obj.button.decided_action_scn_name,
                    obj.button.decided_action_cmd_name,
                    obj.button.decided_action_z_no
                );
            }
            let base_patno = obj
                .lookup_int_prop(&ctx.ids, ctx.ids.obj_patno)
                .unwrap_or(obj.base.patno);
            effective_visual = Some(ButtonVisualState {
                state,
                action_no: obj.button.action_no,
                file_name: obj.file_name.clone(),
                base_patno,
                cut_no: obj.button.cut_no,
            });
        }
    }

    if let Some(visual) = effective_visual.clone() {
        let runtime_slot = object_runtime_slot(obj_idx, obj);
        match &obj.backend {
            ObjectBackend::Gfx => {
                if let Some((lid, sid)) = ctx
                    .gfx
                    .object_sprite_binding(stage_idx, runtime_slot as i64)
                {
                    map.insert((lid, sid), visual.clone());
                }
            }
            ObjectBackend::Rect {
                layer_id,
                sprite_id,
                ..
            } => {
                map.insert((*layer_id, *sprite_id), visual.clone());
            }
            ObjectBackend::String { .. } => {
                for binding in layer_backed_object_sprite_bindings(&obj.backend) {
                    map.insert(binding, visual.clone());
                }
            }
            ObjectBackend::Movie {
                layer_id,
                sprite_id,
                ..
            } => {
                map.insert((*layer_id, *sprite_id), visual.clone());
            }
            ObjectBackend::Number {
                layer_id,
                sprite_ids,
            }
            | ObjectBackend::Weather {
                layer_id,
                sprite_ids,
            } => {
                for sid in sprite_ids {
                    map.insert((*layer_id, *sid), visual.clone());
                }
            }
            ObjectBackend::None => {}
        }
    }

    for (child_idx, child) in obj.runtime.child_objects.iter().enumerate() {
        collect_button_visuals_recursive(
            ctx,
            st,
            stage_idx,
            child_idx,
            child,
            map,
            effective_visual.clone(),
            None,
        );
    }
}

fn button_action_pattern(
    tables: &tables::AssetTables,
    action_no: i64,
    state: i64,
) -> tables::ButtonActionPattern {
    let state_idx = state.clamp(0, 4) as usize;
    if action_no >= 0 {
        if let Some(tpl) = tables.button_action_templates.get(action_no as usize) {
            return tpl.state[state_idx];
        }
    }
    tables::ButtonActionTemplate::default().state[state_idx]
}

fn apply_button_state_visual(
    tables: &tables::AssetTables,
    images: &mut ImageManager,
    sprite: &mut Sprite,
    visual: ButtonVisualState,
) {
    let pat = button_action_pattern(tables, visual.action_no, visual.state);

    if let Some(file_name) = visual.file_name.as_deref().filter(|s| !s.is_empty()) {
        let patno = visual
            .base_patno
            .saturating_add(visual.cut_no)
            .saturating_add(pat.rep_pat_no)
            .max(0) as u32;
        let image_id = match images.load_g00(file_name, patno) {
            Ok(id) => Some(id),
            Err(_) => images.load_bg_frame(file_name, patno as usize).ok(),
        };
        if let Some(image_id) = image_id {
            sprite.image_id = Some(image_id);
            if let Some(img) = images.get(image_id) {
                sprite.object_anchor = true;
                sprite.texture_center_x = img.center_x as f32;
                sprite.texture_center_y = img.center_y as f32;
            } else {
                sprite.object_anchor = false;
                sprite.texture_center_x = 0.0;
                sprite.texture_center_y = 0.0;
            }
        }
    }
    sprite.x = sprite.x.saturating_add(pat.rep_pos_x as i32);
    sprite.y = sprite.y.saturating_add(pat.rep_pos_y as i32);
    sprite.tr = ((sprite.tr as i64 * pat.rep_tr.clamp(0, 255)) / 255).clamp(0, 255) as u8;
    sprite.bright = (sprite.bright as i64 + pat.rep_bright).clamp(0, 255) as u8;
    sprite.dark = (sprite.dark as i64 + pat.rep_dark).clamp(0, 255) as u8;
}

fn unpack_legacy_sorter_key(order: i32) -> (i32, i32) {
    if order.abs() >= 1024 {
        (order.div_euclid(1024), order.rem_euclid(1024))
    } else {
        (0, order)
    }
}

fn legacy_packed_sorter_key(order: i64, layer: i64) -> i32 {
    order
        .clamp(i32::MIN as i64 / 1024, i32::MAX as i64 / 1024)
        .saturating_mul(1024)
        .saturating_add(layer.clamp(-1023, 1023)) as i32
}

fn sorter_key(order: i32, layer: i32) -> (i32, i32) {
    (order, layer)
}

fn sprite_sorter_key(rs: &RenderSprite) -> (i32, i32) {
    (rs.sorter_order, rs.sorter_layer)
}

fn quake_order_affects_sprite(quake: &globals::ScreenQuakeState, rs: &RenderSprite) -> bool {
    let order = rs.sorter_order;
    let (lo, hi) = if quake.begin_order <= quake.end_order {
        (quake.begin_order, quake.end_order)
    } else {
        (quake.end_order, quake.begin_order)
    };
    lo <= order && order <= hi
}

fn apply_quake_transform(sprite: &mut Sprite, tr: globals::ScreenQuakeTransform) {
    sprite.x = sprite.x.saturating_add(tr.x);
    sprite.y = sprite.y.saturating_add(tr.y);
    sprite.pivot_x += tr.center_x as f32;
    sprite.pivot_y += tr.center_y as f32;
    sprite.scale_x *= tr.scale;
    sprite.scale_y *= tr.scale;
    sprite.rotate += tr.rotate_degrees * std::f32::consts::PI / 180.0;
}

fn render_sprite_owned_by(rs: &RenderSprite, stage_form_id: u32) -> bool {
    rs.stage_form_id == Some(stage_form_id)
}

fn collect_screen_shake(globals: &globals::GlobalState) -> (i32, i32) {
    let mut shake_x = 0i32;
    let mut shake_y = 0i32;
    let mut screen_form_ids: Vec<u32> = globals.screen_forms.keys().copied().collect();
    screen_form_ids.sort_unstable();
    for form_id in screen_form_ids {
        let Some(st) = globals.screen_forms.get(&form_id) else {
            continue;
        };
        if st.shake.is_active() {
            shake_x = shake_x.saturating_add(st.shake.cur_x);
            shake_y = shake_y.saturating_add(st.shake.cur_y);
        }
    }
    (shake_x, shake_y)
}

fn collect_screen_quakes(globals: &globals::GlobalState) -> Vec<&globals::ScreenQuakeState> {
    if globals.script.quake_stop_flag {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut screen_form_ids: Vec<u32> = globals.screen_forms.keys().copied().collect();
    screen_form_ids.sort_unstable();
    for form_id in screen_form_ids {
        let Some(st) = globals.screen_forms.get(&form_id) else {
            continue;
        };
        out.extend(st.quake_list.iter().filter(|q| q.is_active()));
    }
    out
}

fn collect_stage_quakes(
    globals: &globals::GlobalState,
    stage_form_id: u32,
    stage_idx: i64,
) -> Vec<&globals::ScreenQuakeState> {
    if globals.script.quake_stop_flag {
        return Vec::new();
    }
    globals
        .stage_forms
        .get(&stage_form_id)
        .and_then(|st| st.quake_lists.get(&stage_idx))
        .map(|quakes| quakes.iter().filter(|q| q.is_active()).collect())
        .unwrap_or_default()
}

fn apply_quakes_to_owner(
    sprites: &mut [RenderSprite],
    owner_form_id: u32,
    quakes: &[&globals::ScreenQuakeState],
) {
    if quakes.is_empty() {
        return;
    }
    for rs in sprites {
        if !render_sprite_owned_by(rs, owner_form_id) {
            continue;
        }
        for quake in quakes {
            if quake_order_affects_sprite(quake, rs) {
                apply_quake_transform(&mut rs.sprite, quake.transform());
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct EffectParam {
    x: i32,
    y: i32,
    z: i32,
    mono: i32,
    reverse: i32,
    bright: i32,
    dark: i32,
    color_r: i32,
    color_g: i32,
    color_b: i32,
    color_rate: i32,
    color_add_r: i32,
    color_add_g: i32,
    color_add_b: i32,
    begin_order: i32,
    begin_layer: i32,
    end_order: i32,
    end_layer: i32,
}

fn apply_effects_to_owner(
    sprites: &mut [RenderSprite],
    owner_form_id: u32,
    effects: &[EffectParam],
) {
    if effects.is_empty() {
        return;
    }
    for effect in effects {
        for rs in sprites.iter_mut() {
            if !render_sprite_owned_by(rs, owner_form_id) || !in_sorter_range(rs, effect) {
                continue;
            }
            apply_effect_to_sprite(&mut rs.sprite, effect);
        }
    }
}

fn read_effect_event(ev: &crate::runtime::int_event::IntEvent) -> i32 {
    ev.get_total_value() as i32
}

fn effect_param_from_state(effect: &globals::ScreenEffectState) -> EffectParam {
    EffectParam {
        x: read_effect_event(&effect.x),
        y: read_effect_event(&effect.y),
        z: read_effect_event(&effect.z),
        mono: read_effect_event(&effect.mono),
        reverse: read_effect_event(&effect.reverse),
        bright: read_effect_event(&effect.bright),
        dark: read_effect_event(&effect.dark),
        color_r: read_effect_event(&effect.color_r),
        color_g: read_effect_event(&effect.color_g),
        color_b: read_effect_event(&effect.color_b),
        color_rate: read_effect_event(&effect.color_rate),
        color_add_r: read_effect_event(&effect.color_add_r),
        color_add_g: read_effect_event(&effect.color_add_g),
        color_add_b: read_effect_event(&effect.color_add_b),
        begin_order: effect.begin_order,
        begin_layer: effect.begin_layer,
        end_order: effect.end_order,
        end_layer: effect.end_layer,
    }
}

fn collect_global_screen_effects(globals: &globals::GlobalState) -> Vec<EffectParam> {
    let mut out = Vec::new();
    let mut screen_form_ids: Vec<u32> = globals.screen_forms.keys().copied().collect();
    screen_form_ids.sort_unstable();
    for form_id in screen_form_ids {
        let Some(st) = globals.screen_forms.get(&form_id) else {
            continue;
        };
        for effect in &st.effect_list {
            let rp = effect_param_from_state(effect);
            if effect_is_use(&rp) {
                out.push(rp);
            }
        }
    }
    out
}

fn collect_stage_effects(
    globals: &globals::GlobalState,
    stage_form_id: u32,
    stage_idx: i64,
) -> Vec<EffectParam> {
    globals
        .stage_forms
        .get(&stage_form_id)
        .and_then(|st| st.effect_lists.get(&stage_idx))
        .map(|effects| {
            effects
                .iter()
                .map(effect_param_from_state)
                .filter(effect_is_use)
                .collect()
        })
        .unwrap_or_default()
}

fn apply_stage_render_effects(
    globals: &globals::GlobalState,
    ids: &constants::RuntimeConstants,
    stage_idx: i64,
    sprites: &mut [RenderSprite],
) {
    // C_tnm_wnd::disp_proc_stage_ready() applies render parameters in this
    // exact sequence. SCREEN effects/quakes belong only to the normal global
    // stage; each EXCALL stage receives only its own stage effects/quakes.
    let normal_stage_form = ids.form_global_stage;
    if normal_stage_form != 0 {
        let screen_effects = collect_global_screen_effects(globals);
        apply_effects_to_owner(sprites, normal_stage_form, &screen_effects);

        let screen_quakes = collect_screen_quakes(globals);
        apply_quakes_to_owner(sprites, normal_stage_form, &screen_quakes);
    }

    let mut stage_form_ids: Vec<u32> = globals.stage_forms.keys().copied().collect();
    stage_form_ids.sort_unstable();
    for stage_form_id in stage_form_ids {
        let effects = collect_stage_effects(globals, stage_form_id, stage_idx);
        apply_effects_to_owner(sprites, stage_form_id, &effects);

        let quakes = collect_stage_quakes(globals, stage_form_id, stage_idx);
        apply_quakes_to_owner(sprites, stage_form_id, &quakes);
    }

    // SCREEN.SHAKE is implemented by the original as a final game-buffer
    // offset, after stage effects and quakes. Until the backend carries a
    // separate presentation transform, applying the same final translation to
    // every game sprite is the closest equivalent and, importantly, does not
    // let the shake translation participate in quake scaling/rotation.
    let (shake_x, shake_y) = collect_screen_shake(globals);
    if shake_x != 0 || shake_y != 0 {
        for rs in sprites {
            rs.sprite.x = rs.sprite.x.saturating_add(shake_x);
            rs.sprite.y = rs.sprite.y.saturating_add(shake_y);
        }
    }
}

fn effect_is_use(effect: &EffectParam) -> bool {
    effect.x != 0
        || effect.y != 0
        || effect.z != 0
        || effect.mono != 0
        || effect.reverse != 0
        || effect.bright != 0
        || effect.dark != 0
        || effect.color_r != 0
        || effect.color_g != 0
        || effect.color_b != 0
        || effect.color_rate != 0
        || effect.color_add_r != 0
        || effect.color_add_g != 0
        || effect.color_add_b != 0
}

fn in_sorter_range(rs: &RenderSprite, effect: &EffectParam) -> bool {
    let key = sprite_sorter_key(rs);
    let begin = sorter_key(effect.begin_order, effect.begin_layer);
    let end = sorter_key(effect.end_order, effect.end_layer);
    if begin <= end {
        begin <= key && key <= end
    } else {
        end <= key && key <= begin
    }
}

fn apply_effect_to_sprite(sprite: &mut Sprite, effect: &EffectParam) {
    sprite.x = sprite.x.saturating_add(effect.x);
    sprite.y = sprite.y.saturating_add(effect.y);
    sprite.z += effect.z as f32;

    sprite.mono = combine_lerp(sprite.mono, effect.mono);
    sprite.reverse = combine_lerp(sprite.reverse, effect.reverse);
    sprite.bright = combine_lerp(sprite.bright, effect.bright);
    sprite.dark = combine_lerp(sprite.dark, effect.dark);

    // Color rate uses the original blend formula.
    let sr = sprite.color_rate as i32;
    let pr = clamp_u8(effect.color_rate);
    if sr + pr > 0 {
        let parent_rate = (pr * 255 * 255) / (255 * 255 - (255 - sr) * (255 - pr));
        sprite.color_r = blend_color(sprite.color_r, effect.color_r, parent_rate);
        sprite.color_g = blend_color(sprite.color_g, effect.color_g, parent_rate);
        sprite.color_b = blend_color(sprite.color_b, effect.color_b, parent_rate);
    }
    sprite.color_rate = (255 - (255 - sr) * (255 - pr) / 255) as u8;

    sprite.color_add_r = clamp_add(sprite.color_add_r, effect.color_add_r);
    sprite.color_add_g = clamp_add(sprite.color_add_g, effect.color_add_g);
    sprite.color_add_b = clamp_add(sprite.color_add_b, effect.color_add_b);
}

fn combine_lerp(base: u8, parent: i32) -> u8 {
    let parent = clamp_u8(parent);
    (255 - (255 - base as i32) * (255 - parent) / 255) as u8
}

fn blend_color(base: u8, parent: i32, rate: i32) -> u8 {
    let parent = clamp_u8(parent);
    let base = base as i32;
    ((base * (255 - rate) + parent * rate) / 255) as u8
}

fn clamp_u8(v: i32) -> i32 {
    v.clamp(0, 255)
}

fn clamp_add(base: u8, add: i32) -> u8 {
    let v = base as i32 + add;
    v.clamp(0, 255) as u8
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WipePartition {
    Under,
    Target,
    Over,
}

fn compose_basic_wipe_scene_inputs<T: Clone>(
    wipe_type: i32,
    under: &mut Vec<T>,
    front: &mut Vec<T>,
    next: &mut Vec<T>,
) {
    if !matches!(wipe_type, 0 | 1 | 2) {
        return;
    }

    let mut front_scene = Vec::with_capacity(under.len() + front.len());
    front_scene.extend(under.iter().cloned());
    front_scene.append(front);

    let mut next_scene = Vec::with_capacity(under.len() + next.len());
    next_scene.extend(under.iter().cloned());
    next_scene.append(next);

    under.clear();
    *front = front_scene;
    *next = next_scene;
}

fn effective_wipe_render_order_bounds(
    ctx: &CommandContext,
    wipe: &globals::WipeState,
) -> (i32, i32) {
    if wipe.stage_form_id != excall_stage_form_id(ctx) {
        return (wipe.begin_order, wipe.end_order);
    }

    // eng_disp.cpp offsets EXCALL wipe sorters by Gp_ini->excall_order before
    // slicing the combined normal+EXCALL sprite tree. Preserve the unbounded
    // sentinels; finite script orders live in the EXCALL order band.
    let offset = configured_excall_order(ctx);
    let shift = |value: i32| -> i32 {
        if value == i32::MIN || value == i32::MAX {
            value
        } else {
            (value as i64)
                .saturating_add(offset)
                .clamp(i32::MIN as i64, i32::MAX as i64) as i32
        }
    };
    (shift(wipe.begin_order), shift(wipe.end_order))
}

fn render_sprite_wipe_sorter(rs: &RenderSprite) -> (i32, i32) {
    // C_elm_stage::get_sprite_tree(begin, end) tests only the top-level
    // OBJECT/MWND/BTNSEL sorter and includes the complete selected subtree.
    // Effects and quakes still use each sprite's own sorter elsewhere.
    (rs.wipe_sorter_order, rs.wipe_sorter_layer)
}

fn classify_wipe_partition(
    rs: &RenderSprite,
    begin_layer: i32,
    end_layer: i32,
    begin_order: i32,
    end_order: i32,
    with_low: bool,
) -> WipePartition {
    let (order, layer) = render_sprite_wipe_sorter(rs);
    if layer < begin_layer {
        return WipePartition::Under;
    }
    if layer > end_layer {
        return WipePartition::Over;
    }
    let affected = if with_low {
        order <= end_order
    } else {
        order >= begin_order && order <= end_order
    };
    if affected {
        WipePartition::Target
    } else if order < begin_order {
        WipePartition::Under
    } else {
        WipePartition::Over
    }
}

fn sprite_forward_dir(sprite: &Sprite) -> [f32; 3] {
    let (sx, cx) = sprite.rotate_x.sin_cos();
    let (sy, cy) = sprite.rotate_y.sin_cos();
    let x = -sy * cx;
    let y = sx;
    let z = -cy * cx;
    let len = (x * x + y * y + z * z).sqrt().max(1e-6);
    [x / len, y / len, z / len]
}

fn apply_runtime_light_and_fog(ctx: &CommandContext, sprite: &mut Sprite) {
    sprite.light_enabled = false;
    sprite.mesh_runtime_lights.clear();
    sprite.light_diffuse = [1.0, 1.0, 1.0, 1.0];
    sprite.light_ambient = [0.0, 0.0, 0.0, 1.0];
    sprite.light_specular = [0.0, 0.0, 0.0, 1.0];
    sprite.light_factor = 0.0;
    sprite.light_kind = -1;
    sprite.light_pos = [0.0, 0.0, 0.0, 0.0];
    sprite.light_dir = [0.0, 0.0, -1.0, 0.0];
    sprite.light_atten = [1.0, 0.0, 0.0, 5000.0];
    sprite.light_cone = [0.0, 0.0, 1.0, 0.0];
    sprite.shadow_cast = sprite.mesh_kind != 0;
    sprite.shadow_receive = sprite.mesh_kind != 0;
    sprite.fog_enabled = false;
    sprite.fog_color = [0.0, 0.0, 0.0, 1.0];
    sprite.fog_near = 0.0;
    sprite.fog_far = 0.0;
    sprite.fog_scroll_x = 0.0;
    sprite.fog_texture_image_id = None;

    if sprite.light_no >= 0 {
        let camera_default_light;
        let light = if let Some(light) = ctx.globals.lights.get(&sprite.light_no) {
            Some(light)
        } else if sprite.light_no == 0 {
            camera_default_light = siglus_default_camera_light(sprite);
            Some(&camera_default_light)
        } else {
            None
        };
        if let Some(light) = light {
            let n = if sprite.billboard {
                [0.0, 0.0, -1.0]
            } else {
                sprite_forward_dir(sprite)
            };
            let pos = [sprite.x as f32, sprite.y as f32, sprite.z];
            let mut ndotl = 0.0f32;
            let mut attenuation = 1.0f32;
            match light.kind {
                globals::LightType::Directional => {
                    let l = [-light.dir[0], -light.dir[1], -light.dir[2]];
                    let ll = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt().max(1e-6);
                    ndotl = (n[0] * (l[0] / ll) + n[1] * (l[1] / ll) + n[2] * (l[2] / ll)).max(0.0);
                }
                globals::LightType::Point
                | globals::LightType::Spot
                | globals::LightType::ShadowMapSpot => {
                    let mut l = [
                        light.pos[0] - pos[0],
                        light.pos[1] - pos[1],
                        light.pos[2] - pos[2],
                    ];
                    let dist = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt().max(1e-6);
                    l = [l[0] / dist, l[1] / dist, l[2] / dist];
                    ndotl = (n[0] * l[0] + n[1] * l[1] + n[2] * l[2]).max(0.0);
                    attenuation = 1.0
                        / (light.attenuation0
                            + light.attenuation1 * dist
                            + light.attenuation2 * dist * dist)
                            .max(1.0);
                    if light.range > 0.0 {
                        attenuation *= (1.0 - dist / light.range).clamp(0.0, 1.0);
                    }
                    if matches!(
                        light.kind,
                        globals::LightType::Spot | globals::LightType::ShadowMapSpot
                    ) {
                        let spot_dir = [light.dir[0], light.dir[1], light.dir[2]];
                        let sll = (spot_dir[0] * spot_dir[0]
                            + spot_dir[1] * spot_dir[1]
                            + spot_dir[2] * spot_dir[2])
                            .sqrt()
                            .max(1e-6);
                        let cosang = (l[0] * (-spot_dir[0] / sll)
                            + l[1] * (-spot_dir[1] / sll)
                            + l[2] * (-spot_dir[2] / sll))
                            .clamp(-1.0, 1.0);
                        let cos_theta = (light.theta_deg.to_radians() * 0.5).cos();
                        let cos_phi = (light.phi_deg.to_radians() * 0.5).cos();
                        let spot = if cosang >= cos_theta {
                            1.0
                        } else if cosang <= cos_phi {
                            0.0
                        } else {
                            ((cosang - cos_phi) / (cos_theta - cos_phi).max(1e-6))
                                .powf(light.falloff.max(0.01))
                        };
                        attenuation *= spot;
                    }
                }
                globals::LightType::None => {}
            }
            sprite.light_enabled = !matches!(light.kind, globals::LightType::None);
            sprite.light_diffuse = light.diffuse;
            sprite.light_ambient = light.ambient;
            sprite.light_specular = light.specular;
            sprite.light_factor = (ndotl * attenuation).clamp(0.0, 1.0);
            sprite.light_kind = light.kind as i32;
            sprite.light_pos = [light.pos[0], light.pos[1], light.pos[2], 1.0];
            let dir_len = (light.dir[0] * light.dir[0]
                + light.dir[1] * light.dir[1]
                + light.dir[2] * light.dir[2])
                .sqrt()
                .max(1e-6);
            sprite.light_dir = [
                light.dir[0] / dir_len,
                light.dir[1] / dir_len,
                light.dir[2] / dir_len,
                0.0,
            ];
            sprite.light_atten = [
                light.attenuation0,
                light.attenuation1,
                light.attenuation2,
                light.range,
            ];
            sprite.light_cone = [
                (light.theta_deg.to_radians() * 0.5).cos(),
                (light.phi_deg.to_radians() * 0.5).cos(),
                light.falloff,
                if matches!(light.kind, globals::LightType::ShadowMapSpot) {
                    1.0
                } else {
                    0.0
                },
            ];
            if matches!(light.kind, globals::LightType::ShadowMapSpot) {
                sprite.shadow_cast = sprite.mesh_kind != 0;
                sprite.shadow_receive = sprite.camera_enabled;
            }
        }
    }

    if sprite.mesh_kind != 0 || sprite.camera_enabled {
        let mut ids: Vec<i32> = ctx.globals.lights.keys().copied().collect();
        if !ctx.globals.lights.contains_key(&0) {
            ids.push(0);
        }
        ids.sort_unstable();
        ids.dedup();
        for light_id in ids {
            let camera_default_light;
            let light = if let Some(light) = ctx.globals.lights.get(&light_id) {
                light
            } else if light_id == 0 {
                camera_default_light = siglus_default_camera_light(sprite);
                &camera_default_light
            } else {
                continue;
            };
            if matches!(light.kind, globals::LightType::None) {
                continue;
            }
            let dir_len = (light.dir[0] * light.dir[0]
                + light.dir[1] * light.dir[1]
                + light.dir[2] * light.dir[2])
                .sqrt()
                .max(1e-6);
            sprite.mesh_runtime_lights.push(SpriteRuntimeLight {
                id: light_id,
                kind: light.kind as i32,
                diffuse: light.diffuse,
                ambient: light.ambient,
                specular: light.specular,
                pos: [light.pos[0], light.pos[1], light.pos[2], 1.0],
                dir: [
                    light.dir[0] / dir_len,
                    light.dir[1] / dir_len,
                    light.dir[2] / dir_len,
                    0.0,
                ],
                atten: [
                    light.attenuation0,
                    light.attenuation1,
                    light.attenuation2,
                    light.range,
                ],
                cone: [
                    (light.theta_deg.to_radians() * 0.5).cos(),
                    (light.phi_deg.to_radians() * 0.5).cos(),
                    light.falloff,
                    if matches!(light.kind, globals::LightType::ShadowMapSpot) {
                        1.0
                    } else {
                        0.0
                    },
                ],
            });
        }
    }

    if sprite.fog_use && sprite.camera_enabled && ctx.globals.fog_global.enabled {
        let fog = &ctx.globals.fog_global;
        sprite.fog_enabled = true;
        sprite.fog_color = fog.color;
        sprite.fog_near = fog.near;
        sprite.fog_far = fog.far;
        sprite.fog_scroll_x = fog.scroll_x;
        sprite.fog_texture_image_id = fog.texture_image_id;
    }
}

fn siglus_default_camera_light(sprite: &Sprite) -> globals::LightState {
    let mut light = globals::LightState::directional(0, [0.0, 1.0, 0.0]);
    light.pos = sprite.camera_eye;
    light.diffuse = [1.0, 1.0, 1.0, 1.0];
    light.ambient = [3.0, 3.0, 3.0, 1.0];
    light.specular = [0.0, 0.0, 0.0, 1.0];
    light
}

fn render_sprite_visible_for_submit(rs: &RenderSprite) -> bool {
    let has_payload = rs.sprite.image_id.is_some()
        || (rs.sprite.mesh_kind != 0 && rs.sprite.mesh_file_name.is_some());
    rs.sprite.visible && has_payload && rs.sprite.alpha > 0 && rs.sprite.tr > 0
}

fn resolve_mask_path(project_dir: &Path, raw: &str) -> Option<PathBuf> {
    let norm = raw.replace('\\', "/");
    let mut candidates = Vec::new();

    if !norm.contains('.') {
        for ext in ["png", "bmp", "jpg", "jpeg", "g00"] {
            candidates.push(project_dir.join(format!("{}.{}", norm, ext)));
            candidates.push(project_dir.join("dat").join(format!("{}.{}", norm, ext)));
            candidates.push(project_dir.join("mask").join(format!("{}.{}", norm, ext)));
        }
    }

    candidates.push(project_dir.join(&norm));
    candidates.push(project_dir.join("dat").join(&norm));
    candidates.push(project_dir.join("mask").join(&norm));

    for c in candidates {
        if let Some(path) = crate::resource::resolve_game_file(&c).ok().flatten() {
            return Some(path);
        }
    }
    None
}

fn ensure_font_list(syscom: &mut globals::SyscomRuntimeState, project_dir: &Path) {
    if !syscom.font_list.is_empty() {
        return;
    }

    let mut seen = HashSet::new();
    for dir in [project_dir.join("font"), project_dir.join("fonts")] {
        let Some(dir) = crate::resource::resolve_game_path(&dir).ok().flatten() else {
            continue;
        };
        let Ok(entries) = fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if ext == "ttf" || ext == "otf" || ext == "ttc" {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    if seen.insert(name.to_string()) {
                        syscom.font_list.push(name.to_string());
                    }
                }
            }
        }
    }

    for name in embedded_default_font_names() {
        if seen.insert((*name).to_string()) {
            syscom.font_list.push((*name).to_string());
        }
    }
    syscom.font_list.sort();
}

#[cfg(test)]
mod basic_wipe_scene_input_tests {
    use super::compose_basic_wipe_scene_inputs;

    #[test]
    fn cross_fade_inputs_are_complete_front_and_next_scenes() {
        let mut under = vec![1, 2];
        let mut front = vec![3];
        let mut next = vec![4];

        compose_basic_wipe_scene_inputs(0, &mut under, &mut front, &mut next);

        assert!(under.is_empty());
        assert_eq!(front, vec![1, 2, 3]);
        assert_eq!(next, vec![1, 2, 4]);
    }

    #[test]
    fn processed_wipes_keep_their_separate_under_input() {
        let mut under = vec![1, 2];
        let mut front = vec![3];
        let mut next = vec![4];

        compose_basic_wipe_scene_inputs(50, &mut under, &mut front, &mut next);

        assert_eq!(under, vec![1, 2]);
        assert_eq!(front, vec![3]);
        assert_eq!(next, vec![4]);
    }
}

#[cfg(test)]
mod render_tree_fidelity_tests {
    use super::{
        apply_effects_to_owner, classify_wipe_partition, compose_clip_rect, EffectParam,
        SiglusRenderNode, WipePartition,
    };
    use crate::layer::{ClipRect, RenderSprite, Sprite};

    fn sprite(order: i32, layer: i32, marker: i32) -> RenderSprite {
        let mut sprite = Sprite::default();
        sprite.x = marker;
        RenderSprite::with_sorter(None, None, order, layer, sprite)
    }

    #[test]
    fn root_sort_keeps_each_subtree_contiguous() {
        let child = SiglusRenderNode::from_single_sprite(sprite(100, 0, 11));
        let first = SiglusRenderNode {
            sorter_order: 10,
            sorter_layer: 0,
            sprites: vec![sprite(10, 0, 10)],
            sort_children_default: true,
            children: vec![child],
        };
        let second = SiglusRenderNode::from_single_sprite(sprite(20, 0, 20));
        let mut roots = vec![second, first];
        roots.sort_by(super::siglus_render_node_cmp);

        let mut flattened = Vec::new();
        for root in roots {
            root.flatten(&mut flattened);
        }
        assert_eq!(
            flattened.iter().map(|rs| rs.sprite.x).collect::<Vec<_>>(),
            vec![10, 11, 20]
        );
    }

    #[test]
    fn default_child_sort_sorts_expanded_root_nodes() {
        let parent = SiglusRenderNode {
            sorter_order: 0,
            sorter_layer: 0,
            sprites: vec![sprite(0, 0, 0)],
            sort_children_default: true,
            children: vec![
                SiglusRenderNode::from_single_sprite(sprite(30, 0, 30)),
                SiglusRenderNode::from_single_sprite(sprite(10, 0, 10)),
            ],
        };
        let mut flattened = Vec::new();
        parent.flatten(&mut flattened);
        assert_eq!(
            flattened.iter().map(|rs| rs.sprite.x).collect::<Vec<_>>(),
            vec![0, 10, 30]
        );
    }

    #[test]
    fn none_child_sort_preserves_insertion_order() {
        let parent = SiglusRenderNode {
            sorter_order: 0,
            sorter_layer: 0,
            sprites: vec![sprite(0, 0, 0)],
            sort_children_default: false,
            children: vec![
                SiglusRenderNode::from_single_sprite(sprite(30, 0, 30)),
                SiglusRenderNode::from_single_sprite(sprite(10, 0, 10)),
            ],
        };
        let mut flattened = Vec::new();
        parent.flatten(&mut flattened);
        assert_eq!(
            flattened.iter().map(|rs| rs.sprite.x).collect::<Vec<_>>(),
            vec![0, 30, 10]
        );
    }

    #[test]
    fn parent_trp_clip_composition_uses_original_union_rule() {
        let parent = ClipRect {
            left: 20,
            top: 30,
            right: 80,
            bottom: 90,
        };
        let child = ClipRect {
            left: 10,
            top: 40,
            right: 70,
            bottom: 100,
        };
        assert_eq!(
            compose_clip_rect(Some(parent), Some(child)),
            Some(ClipRect {
                left: 10,
                top: 30,
                right: 80,
                bottom: 100,
            })
        );
    }

    #[test]
    fn stage_effects_do_not_cross_stage_form_owners() {
        let mut normal = sprite(0, 0, 0);
        normal.set_stage_form_owner(49);
        let mut excall = sprite(0, 0, 0);
        excall.set_stage_form_owner(65);
        let mut sprites = vec![normal, excall];
        let effect = EffectParam {
            x: 12,
            y: 0,
            z: 0,
            mono: 0,
            reverse: 0,
            bright: 0,
            dark: 0,
            color_r: 0,
            color_g: 0,
            color_b: 0,
            color_rate: 0,
            color_add_r: 0,
            color_add_g: 0,
            color_add_b: 0,
            begin_order: i32::MIN,
            begin_layer: i32::MIN,
            end_order: i32::MAX,
            end_layer: i32::MAX,
        };
        apply_effects_to_owner(&mut sprites, 49, &[effect]);
        assert_eq!(sprites[0].sprite.x, 12);
        assert_eq!(sprites[1].sprite.x, 0);
    }

    #[test]
    fn wipe_range_uses_top_level_entity_sorter() {
        let mut child = sprite(900, 900, 0);
        child.set_wipe_sorter(10, 5);
        assert_eq!(
            classify_wipe_partition(&child, 0, 10, 0, 20, false),
            WipePartition::Target
        );
    }
}
