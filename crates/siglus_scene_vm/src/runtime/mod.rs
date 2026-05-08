//! Runtime scaffolding for command execution.
//!
//! This layer provides shared dispatch and runtime state for VM forms and
//! named commands.

pub mod commands;
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
pub mod net;
pub mod native_ui;
pub mod tables;
pub mod tonecurve;
pub mod ui;
pub mod unknown;
pub mod wait;
use crate::runtime::forms::codes::syscom_op;
use crate::runtime::forms::syscom as syscom_form;

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use crate::assets::RgbaImage;
use crate::audio::{AudioHub, BgmEngine, KoeEngine, PcmEngine, SeEngine};
use crate::image_manager::{ImageId, ImageManager};
use crate::layer::{
    ClipRect, LayerId, LayerManager, RenderSprite, Sprite, SpriteFit, SpriteId, SpriteRuntimeLight,
    SpriteSizeMode,
};
use crate::movie::MovieManager;
use crate::soft_render;
use crate::text_render::{embedded_default_font_names, FontCache};
use siglus_assets::scene_pck::{find_scene_pck_in_project, ScenePck, ScenePckDecodeOptions};
use std::fs;
use std::path::{Path, PathBuf};

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

/// State used by EXCALL runtime helpers.
///
/// We intentionally keep these names offset-based instead of guessing their meaning.
#[derive(Debug, Default, Clone)]
pub struct ExcallCompatState {
    pub ready: bool,
    pub ex_call_flag: bool,
    pub flag_204: bool,
    pub flag_2148: bool,
    pub script_proc_requested: bool,
    pub script_proc_pop_requested: bool,
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
    if std::env::var_os("SG_DEBUG").is_none() {
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

pub struct CommandContext {
    pub project_dir: PathBuf,

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
    /// Offscreen target image for the front/old stage during dual-source wipes.
    pub wipe_front_rt_image: Option<ImageId>,
    /// Offscreen target image for the next/new stage during dual-source wipes.
    pub wipe_next_rt_image: Option<ImageId>,
    /// Legacy runtime slot for overlay intermediate images. GPU overlay composition now bypasses it.
    pub overlay_rt_image: Option<ImageId>,

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

    frame_clock_last: Option<std::time::Instant>,
    last_button_hover_sound_pos: Option<(i32, i32)>,
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
    }

    pub fn take_read_flag_no_request(&mut self) -> bool {
        std::mem::take(&mut self.pending_read_flag_no)
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
        if self
            .ui
            .needs_continuous_frame(&self.globals.script, &self.globals.syscom)
        {
            return true;
        }
        if self.globals.mov.playing || self.globals.wipe.is_some() {
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
                || screen.quake_list.iter().any(|q| q.until.is_some())
                || screen.shake.until.is_some()
        }) {
            return true;
        }
        let mwnd_ui_state = self
            .ui
            .current_mwnd_window_render_state(self.screen_w, self.screen_h);
        self.globals.stage_forms.values().any(|stage| {
            stage.object_lists.iter().any(|(&stage_idx, list)| {
                list.iter().enumerate().any(|(obj_idx, obj)| {
                    !stage.is_embedded_object_slot(stage_idx, obj_idx) && object_needs_tick(obj)
                })
            }) || stage.mwnd_lists.values().any(|list| {
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

    pub fn notify_wait_key(&mut self) -> bool {
        let wipe_skipped = {
            let wait = &mut self.wait;
            let globals = &mut self.globals;
            wait.notify_key(globals, &self.ids)
        };
        self.finish_skipped_movie_waits();
        if wipe_skipped {
            self.globals.finish_wipe();
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
        matches!(k, input::VmKey::Shift | input::VmKey::Alt)
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
                .map(|eb| eb.created && eb.visible)
                .unwrap_or(false);
            if !keep {
                self.globals.focused_editbox = None;
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
        let Some(list) = self.globals.editbox_lists.get(&form_id) else {
            return;
        };
        let len = list.boxes.len();
        if len == 0 {
            return;
        }
        let mut cur = idx;
        for _ in 0..len {
            cur = if forward {
                (cur + 1) % len
            } else {
                (cur + len - 1) % len
            };
            if let Some(eb) = list.boxes.get(cur) {
                if eb.created {
                    self.globals.focused_editbox = Some((form_id, cur));
                    return;
                }
            }
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
        let clear_message_window = self.ui.end_wait_message();
        if clear_message_window {
            self.clear_current_mwnd_after_wait();
        }
        if self.should_stop_koe_on_advance() {
            let _ = self.se.stop(None);
            let _ = self.pcm.stop_all(None);
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
        if let Some(m) = self
            .globals
            .stage_forms
            .get_mut(&form_id)
            .and_then(|st| st.mwnd_lists.get_mut(&stage_idx))
            .and_then(|list| list.get_mut(mwnd_idx))
        {
            m.msg_text.clear();
            m.name_text.clear();
            m.key_icon_appear = false;
            m.key_icon_pos = None;
            m.text_dirty = false;
        }
        self.ui.clear_name();
    }
    pub fn new(project_dir: PathBuf) -> Self {
        let mut unknown = unknown::UnknownOpRecorder::default();
        let tables = tables::AssetTables::load(&project_dir, &mut unknown);

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
            globals: globals::GlobalState::default(),
            tonecurve,
            excall_state: ExcallCompatState::default(),
            last_presented_render_list: Vec::new(),
            wipe_front_rt_image: None,
            wipe_next_rt_image: None,
            overlay_rt_image: None,
            external_forms: None,
            native_ui_backend: None,
            native_ui: native_ui::NativeUiRuntime::default(),
            current_scene_no: None,
            current_scene_name: None,
            current_line_no: -1,
            vm_call: None,
            pending_read_flag_no: false,
            frame_clock_last: None,
            last_button_hover_sound_pos: None,
        };
        ctx.apply_gameexe_runtime_defaults();
        ctx
    }

    fn apply_gameexe_runtime_defaults(&mut self) {
        self.globals.script.font_bold = self.tables.font_defaults.futoku;
        self.globals.script.font_shadow = self.tables.font_defaults.shadow;
        let text = self.gameexe_color(self.tables.mwnd_render.moji_color);
        let shadow = self.gameexe_color(self.tables.mwnd_render.shadow_color);
        let fuchi = (self.tables.mwnd_render.fuchi_color >= 0)
            .then_some(self.gameexe_color(self.tables.mwnd_render.fuchi_color));
        self.ui.set_text_colors_full(text, shadow, fuchi);
    }

    fn gameexe_color(&self, color_no: i64) -> (u8, u8, u8) {
        if color_no >= 0 {
            if let Some(&c) = self.tables.color_table.get(color_no as usize) {
                return c;
            }
        }
        (255, 255, 255)
    }

    pub fn lookup_scene_no(&self, scene_name: &str) -> Result<i64> {
        if scene_name.is_empty() {
            anyhow::bail!("empty scene name")
        }
        let scene_pck_path = find_scene_pck_in_project(&self.project_dir)?;
        let opt = ScenePckDecodeOptions::from_project_dir(&self.project_dir)?;
        let pck = ScenePck::load_and_rebuild(&scene_pck_path, &opt)?;
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
        self.wipe_front_rt_image = None;
        self.wipe_next_rt_image = None;
        self.overlay_rt_image = None;
        self.input.clear_all();
        self.vm_call = None;
        self.pending_read_flag_no = false;
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
        let normal_stage_form = self.ids.form_global_stage;
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
                    if !g.started {
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
                self.globals.script.mwnd_disp_off_flag || self.globals.syscom.hide_mwnd.onoff;
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
    }

    fn handle_object_button_mouse_down(&mut self, b: input::VmMouseButton) -> bool {
        // The original button manager separates pushed_this_frame from decided_this_frame.
        // Press starts the push state; release inside the same button decides it.
        self.update_object_button_hover();

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
                            if !g.started {
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
                            if g.started && g.cancel_flag {
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
                self.globals.script.mwnd_disp_off_flag || self.globals.syscom.hide_mwnd.onoff;
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
                    if !g.started {
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
                self.globals.script.mwnd_disp_off_flag || self.globals.syscom.hide_mwnd.onoff;
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
        if self.handle_syscom_menu_key(k) {
            return;
        }
        if self.handle_selbtn_key(k) {
            return;
        }
        if self.is_vm_key_disabled(k) {
            return;
        }
        self.input.on_key_down(k);
        if Self::is_modifier_key(k) {
            return;
        }

        // EditBox runtime: map common keys and focus changes.
        self.handle_editbox_key(k);

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
        if self.is_vm_key_disabled(k) {
            return;
        }
        self.input.on_key_up(k);
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
        if !text.is_empty() {
            eb.insert_text_at_cursor(text);
        }
    }

    pub fn on_mouse_move(&mut self, x: i32, y: i32) {
        self.input.on_mouse_move(x, y);
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
        if self.handle_msg_back_click(b) {
            return;
        }
        if self.handle_syscom_menu_click() {
            return;
        }
        if self.handle_selbtn_mouse_click(b) {
            return;
        }
        let handled_mwnd_selection = self.handle_mwnd_selection_click(b);
        self.input.on_mouse_down(b);
        self.update_editbox_focus_from_mouse_down(b);
        let handled_button = if !handled_mwnd_selection {
            self.handle_object_button_mouse_down(b)
        } else {
            false
        };
        if !handled_button {
            if !self.advance_message_wait(true) {
                self.notify_wait_key();
            }
        }
    }

    fn update_editbox_focus_from_mouse_down(&mut self, b: input::VmMouseButton) {
        if !matches!(b, input::VmMouseButton::Left) {
            return;
        }
        let x = self.input.mouse_x;
        let y = self.input.mouse_y;
        let mut new_focus = None;
        for (form_id, list) in self.globals.editbox_lists.iter() {
            for (idx, eb) in list.boxes.iter().enumerate() {
                if eb.contains_point(x, y) {
                    new_focus = Some((*form_id, idx));
                    break;
                }
            }
            if new_focus.is_some() {
                break;
            }
        }
        if new_focus.is_some() {
            self.globals.focused_editbox = new_focus;
        }
    }

    pub fn on_mouse_up(&mut self, b: input::VmMouseButton) {
        if sg_input_trace_enabled() {
            eprintln!(
                "[SG_DEBUG][INPUT] mouse_up {:?} at=({}, {})",
                b, self.input.mouse_x, self.input.mouse_y
            );
        }
        self.input.on_mouse_up(b);
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
        let handled_button = self.handle_object_button_mouse_up(b);
        if !handled_button {
            self.notify_wait_key();
        }
    }

    pub fn on_mouse_wheel(&mut self, delta_y: i32) {
        self.input.on_mouse_wheel(delta_y);
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

    fn handle_editbox_key(&mut self, k: input::VmKey) {
        let Some((form_id, idx)) = self.globals.focused_editbox else {
            return;
        };
        let alt_down = self.input.vk_is_down(0x12);
        let shift_down = self.input.vk_is_down(0x10);
        let mut move_focus: Option<bool> = None;
        let mut toggle_screen = false;
        {
            let Some(list) = self.globals.editbox_lists.get_mut(&form_id) else {
                return;
            };
            let Some(eb) = list.boxes.get_mut(idx) else {
                return;
            };
            if !eb.created || !eb.visible {
                return;
            }

            match k {
                input::VmKey::Enter => {
                    if alt_down {
                        toggle_screen = true;
                    } else {
                        eb.action_flag = crate::runtime::globals::EDITBOX_ACTION_DECIDED;
                    }
                }
                input::VmKey::Escape => {
                    eb.action_flag = crate::runtime::globals::EDITBOX_ACTION_CANCELED;
                }
                input::VmKey::Backspace => {
                    eb.backspace_like();
                }
                input::VmKey::Tab => {
                    move_focus = Some(!shift_down);
                }
                _ => {}
            }
        }
        if toggle_screen {
            self.toggle_screen_size_mode_for_editbox();
        }
        if let Some(forward) = move_focus {
            self.move_editbox_focus(forward);
        }
    }

    pub fn wait_poll(&mut self) -> bool {
        self.poll_native_messagebox_result();
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
        let now = std::time::Instant::now();
        let last = self.frame_clock_last.replace(now);
        let elapsed_ms = last
            .map(|t| now.saturating_duration_since(t).as_millis() as i32)
            .unwrap_or(16);
        let real_delta_ms = elapsed_ms.max(0);
        let game_delta_ms = real_delta_ms;
        let trace = std::env::var_os("SG_CTX_TICK_TRACE").is_some();
        if trace {
            eprintln!(
                "[SG_CTX_TICK] start game_delta_ms={} real_delta_ms={}",
                game_delta_ms, real_delta_ms
            );
        }
        self.sync_editbox_runtime();
        self.poll_native_messagebox_result();
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
        // If scripts request message-window hide, enforce it after UI tick.
        if self.globals.script.mwnd_disp_off_flag {
            self.ui.force_message_bg_visible(false);
        }
        self.sync_syscom_menu_ui();
        if trace {
            eprintln!("[SG_CTX_TICK] after sync_syscom_menu_ui");
        }
        self.sync_mwnd_selection_ui();
        if trace {
            eprintln!("[SG_CTX_TICK] after sync_mwnd_selection_ui");
        }
        self.globals.tick_frame(game_delta_ms, real_delta_ms);
        if trace {
            eprintln!("[SG_CTX_TICK] after globals.tick_frame");
        }
        self.apply_object_event_animations();
        if trace {
            eprintln!("[SG_CTX_TICK] after apply_object_event_animations");
        }
        self.sync_weather_objects(game_delta_ms, real_delta_ms);
        if trace {
            eprintln!("[SG_CTX_TICK] after sync_weather_objects");
        }
        let _ = self.bgm.tick(&mut self.audio);
        if trace {
            eprintln!("[SG_CTX_TICK] after bgm.tick");
        }
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
        const GET_NO_WIPE_ANIME_ONOFF: i32 = 286;
        const GET_SKIP_WIPE_ANIME_ONOFF: i32 = 288;
        let cfg = &self.globals.syscom.config_int;
        let no_wipe = cfg.get(&GET_NO_WIPE_ANIME_ONOFF).copied().unwrap_or(0) != 0;
        let skip_wipe = cfg.get(&GET_SKIP_WIPE_ANIME_ONOFF).copied().unwrap_or(0) != 0;
        if (no_wipe || skip_wipe) && self.globals.wipe.is_some() {
            self.globals.finish_wipe();
        }
    }

    fn apply_object_event_animations(&mut self) {
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
        let images = &mut self.images;
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
                        images,
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
                let bw = (self.screen_w as i32 / len as i32).max(1);
                let mut idx = (self.input.mouse_x.max(0) / bw) as usize;
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
        self.globals.system.messagebox_modal = None;
        self.globals.system.messagebox_modal_result = Some(value);
        self.wait.finish_system_modal(Value::Int(value));
        self.ui.set_sys_overlay(false, String::new());
    }

    fn sync_system_messagebox_ui(&mut self) -> bool {
        let Some(modal) = self.globals.system.messagebox_modal.as_ref() else {
            return false;
        };
        if modal.native_pending {
            self.ui.set_sys_overlay(false, String::new());
            return true;
        }
        let mut text = String::new();
        if modal.debug_only {
            text.push_str("[DEBUG]\n");
        }
        text.push_str(&modal.text);
        text.push_str("\n\n");
        for (i, button) in modal.buttons.iter().enumerate() {
            if i == modal.cursor {
                text.push_str("> ");
            } else {
                text.push_str("  ");
            }
            text.push_str(&(i + 1).to_string());
            text.push_str(": ");
            text.push_str(&button.label);
            text.push_str("\n");
        }
        text.push_str("\nEnter/Space: decide  Esc/Right click: cancel");
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
                st.history
                    .iter()
                    .enumerate()
                    .filter_map(|(i, entry)| Self::msg_back_entry_has_content(entry).then_some(i))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn msg_back_is_enable(&self) -> bool {
        self.globals.syscom.msg_back.check_enabled() != 0
            && !self.globals.script.msg_back_disable
            && !self.msg_back_visible_entry_indices().is_empty()
    }

    fn clamp_msg_back_view_pos(&mut self) {
        let count = self.msg_back_visible_entry_indices().len();
        if count == 0 {
            self.globals.syscom.msg_back_view_pos = 0;
        } else if self.globals.syscom.msg_back_view_pos >= count {
            self.globals.syscom.msg_back_view_pos = count - 1;
        }
    }

    fn open_msg_back_proc(&mut self) {
        if !self.msg_back_is_enable() {
            return;
        }
        // Original tnm_syscom_open_msg_back() stops read skip before entering
        // the message-back process.
        self.globals.syscom.read_skip.onoff = false;
        let count = self.msg_back_visible_entry_indices().len();
        self.globals.syscom.msg_back_view_pos = count.saturating_sub(1);
        self.globals.syscom.msg_back_open = true;
    }

    fn close_msg_back_proc(&mut self) {
        self.globals.syscom.msg_back_open = false;
        self.ui.set_sys_overlay(false, String::new());
    }

    fn handle_msg_back_key(&mut self, k: input::VmKey) -> bool {
        if !self.globals.syscom.msg_back_open {
            return false;
        }
        let count = self.msg_back_visible_entry_indices().len();
        match k {
            input::VmKey::Escape | input::VmKey::Enter | input::VmKey::Space => {
                self.close_msg_back_proc();
            }
            input::VmKey::ArrowUp | input::VmKey::ArrowLeft => {
                if self.globals.syscom.msg_back_view_pos > 0 {
                    self.globals.syscom.msg_back_view_pos -= 1;
                }
            }
            input::VmKey::ArrowDown | input::VmKey::ArrowRight => {
                if count > 0 {
                    self.globals.syscom.msg_back_view_pos =
                        (self.globals.syscom.msg_back_view_pos + 1).min(count - 1);
                }
            }
            input::VmKey::F(5) => {
                self.globals.syscom.msg_back_view_pos = 0;
            }
            input::VmKey::F(6) => {
                self.globals.syscom.msg_back_view_pos = count.saturating_sub(1);
            }
            _ => {}
        }
        true
    }

    fn handle_msg_back_click(&mut self, b: input::VmMouseButton) -> bool {
        if !self.globals.syscom.msg_back_open {
            return false;
        }
        match b {
            input::VmMouseButton::Left | input::VmMouseButton::Right => self.close_msg_back_proc(),
            _ => {}
        }
        true
    }

    fn build_msg_back_overlay_text(&mut self) -> String {
        let visible = self.msg_back_visible_entry_indices();
        let count = visible.len();
        self.clamp_msg_back_view_pos();
        let pos = self.globals.syscom.msg_back_view_pos.min(count.saturating_sub(1));
        let mut text = String::new();
        text.push_str("MESSAGE LOG\n");
        text.push_str("Esc/Enter/Click: close    Arrow keys: scroll\n\n");
        if count == 0 {
            text.push_str("No message history.\n");
            return text;
        }
        let start = pos.saturating_sub(10);
        if let Some(st) = self.msg_back_state() {
            for (display_pos, entry_idx) in visible.iter().enumerate().skip(start).take(12) {
                let entry = &st.history[*entry_idx];
                let cursor = if display_pos == pos { ">" } else { " " };
                text.push_str(cursor);
                text.push(' ');
                text.push_str(&(display_pos + 1).to_string());
                text.push('/');
                text.push_str(&count.to_string());
                text.push(' ');
                if !entry.disp_name.is_empty() {
                    text.push('[');
                    text.push_str(&entry.disp_name);
                    text.push_str("] ");
                } else if !entry.original_name.is_empty() {
                    text.push('[');
                    text.push_str(&entry.original_name);
                    text.push_str("] ");
                }
                if entry.pct_flag {
                    text.push_str("[image] ");
                }
                if !entry.msg_str.is_empty() {
                    text.push_str(&entry.msg_str.replace('\n', " "));
                }
                if !entry.koe_no_list.is_empty() {
                    text.push_str("  [voice]");
                }
                text.push('\n');
            }
        }
        text
    }

    fn handle_syscom_menu_key(&mut self, k: input::VmKey) -> bool {
        if !self.globals.syscom.menu_open {
            return false;
        }
        match k {
            input::VmKey::Escape => {
                self.close_syscom_menu();
                return true;
            }
            input::VmKey::Enter => {
                self.activate_syscom_menu_item();
                return true;
            }
            input::VmKey::ArrowUp => {
                let len = self.menu_items().len();
                if len > 0 {
                    let c = self.globals.syscom.menu_cursor;
                    self.globals.syscom.menu_cursor = if c == 0 { len - 1 } else { c - 1 };
                }
                return true;
            }
            input::VmKey::ArrowDown => {
                let len = self.menu_items().len();
                if len > 0 {
                    self.globals.syscom.menu_cursor = (self.globals.syscom.menu_cursor + 1) % len;
                }
                return true;
            }
            input::VmKey::ArrowLeft => {
                self.menu_adjust(-1);
                return true;
            }
            input::VmKey::ArrowRight => {
                self.menu_adjust(1);
                return true;
            }
            input::VmKey::Digit(d) => {
                let idx = d as usize;
                if self.handle_save_load_digit(idx) {
                    return true;
                }
            }
            _ => {}
        }
        true
    }

    fn handle_syscom_menu_click(&mut self) -> bool {
        if !self.globals.syscom.menu_open {
            return false;
        }
        self.activate_syscom_menu_item();
        true
    }

    fn close_syscom_menu(&mut self) {
        self.globals.syscom.menu_open = false;
        self.globals.syscom.menu_kind = None;
    }

    fn activate_syscom_menu_item(&mut self) {
        if !self.globals.syscom.menu_open {
            return;
        }
        let items = self.menu_items();
        if items.is_empty() {
            self.close_syscom_menu();
            return;
        }
        let idx = self.globals.syscom.menu_cursor.min(items.len() - 1);
        match &items[idx] {
            MenuItem::Action { kind, .. } => self.activate_syscom_action(*kind),
            MenuItem::Int { .. } | MenuItem::Bool { .. } | MenuItem::FontName => {
                self.menu_adjust(1)
            }
        }
    }

    fn activate_syscom_action(&mut self, kind: i32) {
        match kind {
            syscom_op::CALL_SAVE_MENU
            | syscom_op::CALL_LOAD_MENU
            | syscom_op::CALL_CONFIG_MENU
            | syscom_op::CALL_CONFIG_WINDOW_MODE_MENU
            | syscom_op::CALL_CONFIG_VOLUME_MENU
            | syscom_op::CALL_CONFIG_BGMFADE_MENU
            | syscom_op::CALL_CONFIG_KOEMODE_MENU
            | syscom_op::CALL_CONFIG_CHARAKOE_MENU
            | syscom_op::CALL_CONFIG_JITAN_MENU
            | syscom_op::CALL_CONFIG_MESSAGE_SPEED_MENU
            | syscom_op::CALL_CONFIG_AUTO_MODE_MENU
            | syscom_op::CALL_CONFIG_FONT_MENU
            | syscom_op::CALL_CONFIG_FILTER_COLOR_MENU
            | syscom_op::CALL_CONFIG_SYSTEM_MENU
            | syscom_op::CALL_CONFIG_MOVIE_MENU => {
                self.globals.syscom.menu_kind = Some(kind);
                self.globals.syscom.menu_cursor = 0;
                self.globals.syscom.last_menu_call = kind;
            }
            syscom_op::OPEN_MSG_BACK => {
                self.open_msg_back_proc();
                self.globals.syscom.last_menu_call = kind;
                self.close_syscom_menu();
            }
            syscom_op::RETURN_TO_SEL => {
                self.globals.syscom.pending_proc = Some(globals::SyscomPendingProc {
                    kind: globals::SyscomPendingProcKind::ReturnToSel,
                    warning: false,
                    se_play: false,
                    fade_out: false,
                    leave_msgbk: false,
                    save_id: 0,
                });
                self.globals.syscom.last_menu_call = kind;
                self.close_syscom_menu();
            }
            syscom_op::RETURN_TO_MENU => {
                self.globals.syscom.pending_proc = Some(globals::SyscomPendingProc {
                    kind: globals::SyscomPendingProcKind::ReturnToMenu,
                    warning: false,
                    se_play: false,
                    fade_out: false,
                    leave_msgbk: false,
                    save_id: 0,
                });
                self.globals.syscom.last_menu_call = kind;
                self.close_syscom_menu();
            }
            syscom_op::END_GAME => {
                self.globals.syscom.last_menu_call = kind;
                self.globals.system.active_flag = false;
                self.close_syscom_menu();
            }
            _ => {
                self.globals.syscom.last_menu_call = kind;
                self.close_syscom_menu();
            }
        }
    }

    fn handle_save_load_digit(&mut self, idx: usize) -> bool {
        if let Some(kind) = self.globals.syscom.menu_kind {
            if kind == syscom_op::CALL_SAVE_MENU {
                syscom_form::menu_save_slot(self, false, idx);
            } else if kind == syscom_op::CALL_LOAD_MENU {
                syscom_form::menu_load_slot(self, false, idx);
            } else if kind == syscom_op::QUICK_SAVE {
                syscom_form::menu_save_slot(self, true, idx);
            } else if kind == syscom_op::QUICK_LOAD {
                syscom_form::menu_load_slot(self, true, idx);
            } else {
                return false;
            }
            self.globals.syscom.menu_result = Some(idx as i64);
            self.globals.syscom.system_extra_int_value = idx as i64;
            self.close_syscom_menu();
            return true;
        }
        false
    }

    fn menu_items(&mut self) -> Vec<MenuItem> {
        syscom_menu_items(&mut self.globals.syscom, &self.project_dir)
    }

    fn menu_adjust(&mut self, dir: i32) {
        let mut items = self.menu_items();
        if items.is_empty() {
            return;
        }
        let idx = self.globals.syscom.menu_cursor.min(items.len() - 1);
        match items.get_mut(idx) {
            Some(MenuItem::Int {
                key,
                min,
                max,
                step,
                ..
            }) => {
                let cur = self
                    .globals
                    .syscom
                    .config_int
                    .get(key)
                    .copied()
                    .unwrap_or(*min as i64);
                let next = (cur + (*step as i64 * dir as i64)).clamp(*min as i64, *max as i64);
                self.globals.syscom.config_int.insert(*key, next);
                if *key == GET_ALL_VOLUME
                    || *key == GET_BGM_VOLUME
                    || *key == GET_KOE_VOLUME
                    || *key == GET_PCM_VOLUME
                    || *key == GET_SE_VOLUME
                    || *key == GET_MOV_VOLUME
                    || *key == GET_BGMFADE_VOLUME
                {
                    syscom_form::apply_audio_config(self);
                }
            }
            Some(MenuItem::Bool { key, .. }) => {
                let cur = self
                    .globals
                    .syscom
                    .config_int
                    .get(key)
                    .copied()
                    .unwrap_or(0);
                let next = if cur == 0 { 1 } else { 0 };
                self.globals.syscom.config_int.insert(*key, next);
                if *key == GET_ALL_ONOFF
                    || *key == GET_BGM_ONOFF
                    || *key == GET_KOE_ONOFF
                    || *key == GET_PCM_ONOFF
                    || *key == GET_SE_ONOFF
                    || *key == GET_MOV_ONOFF
                    || *key == GET_BGMFADE_ONOFF
                {
                    syscom_form::apply_audio_config(self);
                }
            }
            Some(MenuItem::Action { kind, .. }) => {
                self.activate_syscom_action(*kind);
            }
            Some(MenuItem::FontName) => {
                let list = self.globals.syscom.font_list.clone();
                if list.is_empty() {
                    return;
                }
                let cur = self
                    .globals
                    .syscom
                    .config_str
                    .get(&GET_FONT_NAME)
                    .cloned()
                    .unwrap_or_default();
                let mut pos = list.iter().position(|s| s == &cur).unwrap_or(0) as i32;
                pos += dir;
                if pos < 0 {
                    pos = list.len() as i32 - 1;
                }
                let pos = (pos as usize) % list.len();
                self.globals
                    .syscom
                    .config_str
                    .insert(GET_FONT_NAME, list[pos].clone());
            }
            None => {}
        }
    }

    fn sync_syscom_menu_ui(&mut self) {
        if self.sync_system_messagebox_ui() {
            return;
        }
        if self.globals.syscom.msg_back_open {
            let text = self.build_msg_back_overlay_text();
            self.ui.set_sys_overlay(true, text);
            return;
        }
        if !self.globals.syscom.menu_open {
            if self.globals.selbtn.started {
                let text = build_selbtn_menu_text(&self.globals.selbtn);
                self.ui.set_sys_overlay(true, text);
            } else {
                self.ui.set_sys_overlay(false, String::new());
            }
            return;
        }
        let len = self.menu_items().len();
        if len > 0 && self.globals.syscom.menu_cursor >= len {
            self.globals.syscom.menu_cursor = 0;
        }
        let text = build_syscom_menu_text(&mut self.globals.syscom, &self.project_dir);
        self.ui.set_sys_overlay(true, text);
    }

    fn handle_selbtn_key(&mut self, k: input::VmKey) -> bool {
        if !self.globals.selbtn.started {
            return false;
        }
        match k {
            input::VmKey::ArrowUp => {
                if !self.globals.selbtn.choices.is_empty() {
                    let c = self.globals.selbtn.cursor;
                    self.globals.selbtn.cursor = if c == 0 {
                        self.globals.selbtn.choices.len() - 1
                    } else {
                        c - 1
                    };
                }
                true
            }
            input::VmKey::ArrowDown => {
                if !self.globals.selbtn.choices.is_empty() {
                    self.globals.selbtn.cursor =
                        (self.globals.selbtn.cursor + 1) % self.globals.selbtn.choices.len();
                }
                true
            }
            input::VmKey::Enter => {
                let result = (self.globals.selbtn.cursor as i64) + 1;
                self.finish_selbtn(result);
                true
            }
            input::VmKey::Escape if self.globals.selbtn.cancel_enable => {
                self.finish_selbtn(-1);
                true
            }
            _ => true,
        }
    }

    fn handle_selbtn_mouse_click(&mut self, b: input::VmMouseButton) -> bool {
        if !self.globals.selbtn.started {
            return false;
        }
        match b {
            input::VmMouseButton::Left => {
                let result = (self.globals.selbtn.cursor as i64) + 1;
                self.finish_selbtn(result);
                true
            }
            input::VmMouseButton::Right if self.globals.selbtn.cancel_enable => {
                self.finish_selbtn(-1);
                true
            }
            _ => true,
        }
    }

    fn finish_selbtn(&mut self, result: i64) {
        self.globals.selbtn.result = result;
        self.globals.selbtn.started = false;
        if result > 0 {
            if let Some(choice) = self.globals.selbtn.choices.get((result - 1) as usize) {
                self.globals.syscom.system_extra_str_value = choice.text.clone();
            }
        }
        self.stack.push(Value::Int(result));
        self.notify_wait_key();
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
        let mut selected: Option<crate::runtime::ui::MwndProjectionState> = None;

        for (form_id, st) in &self.globals.stage_forms {
            for (stage_idx, list) in &st.mwnd_lists {
                for (mwnd_idx, m) in list.iter().enumerate() {
                    if !m.open {
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
                    let candidate = crate::runtime::ui::MwndProjectionState {
                        bg_file: if m.waku_file.is_empty() {
                            None
                        } else {
                            Some(m.waku_file.clone())
                        },
                        filter_file: if m.filter_file.is_empty() {
                            None
                        } else {
                            Some(m.filter_file.clone())
                        },
                        filter_margin: m.filter_margin,
                        filter_color: m.filter_color,
                        filter_config_color: m.filter_config_color,
                        filter_config_tr: m.filter_config_tr,
                        face_file: if m.face_file.is_empty() {
                            None
                        } else {
                            Some(m.face_file.clone())
                        },
                        face_no: m.face_no,
                        rep_pos: m.rep_pos,
                        window_pos: m.window_pos,
                        window_size: m.window_size,
                        message_pos: m.message_pos,
                        message_margin: m.message_margin,
                        window_moji_cnt: m.window_moji_cnt,
                        moji_size: m.moji_size,
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
                        key_icon_file: key_icon_template.and_then(|t| {
                            if t.file_name.is_empty() {
                                None
                            } else {
                                Some(t.file_name.clone())
                            }
                        }),
                        key_icon_pat_cnt: key_icon_template.map(|t| t.anime_pat_cnt).unwrap_or(1),
                        key_icon_speed: key_icon_template.map(|t| t.anime_speed).unwrap_or(100),
                        page_icon_file: page_icon_template.and_then(|t| {
                            if t.file_name.is_empty() {
                                None
                            } else {
                                Some(t.file_name.clone())
                            }
                        }),
                        page_icon_pat_cnt: page_icon_template.map(|t| t.anime_pat_cnt).unwrap_or(1),
                        page_icon_speed: page_icon_template.map(|t| t.anime_speed).unwrap_or(100),
                        key_icon_appear: m.key_icon_appear,
                        key_icon_mode: m.key_icon_mode,
                        key_icon_pos: m.key_icon_pos,
                        icon_pos_type: m.icon_pos_type,
                        icon_pos_base: m.icon_pos_base,
                        icon_pos: m.icon_pos,
                        slide_enabled: m.slide_msg,
                        slide_time: m.slide_time,
                        name_text: m.name_text.clone(),
                        msg_text: m.msg_text.clone(),
                    };
                    let is_focused = focused == Some((*form_id, *stage_idx, mwnd_idx));
                    if is_focused || selected.is_none() {
                        selected = Some(candidate);
                    }
                }
            }
        }

        if let Some(proj) = selected {
            let moji_no = proj
                .chara_moji_color
                .or(proj.name_moji_color)
                .or(proj.moji_color)
                .unwrap_or(self.tables.mwnd_render.moji_color);
            let shadow_no = proj
                .chara_shadow_color
                .or(proj.name_shadow_color)
                .or(proj.shadow_color)
                .unwrap_or(self.tables.mwnd_render.shadow_color);
            let fuchi_no = proj
                .chara_fuchi_color
                .or(proj.name_fuchi_color)
                .or(proj.fuchi_color)
                .unwrap_or(self.tables.mwnd_render.fuchi_color);
            let text_color = self.gameexe_color(moji_no);
            let shadow_color = self.gameexe_color(shadow_no);
            let fuchi_color = (fuchi_no >= 0).then_some(self.gameexe_color(fuchi_no));
            self.ui
                .set_text_colors_full(text_color, shadow_color, fuchi_color);
            self.ui.apply_mwnd_projection(&proj);
        } else if !self.ui.mwnd.anim.visible {
            self.ui.clear_mwnd_window_state();
        }
    }

    fn sync_mwnd_selection_ui(&mut self) {
        if self.globals.syscom.menu_open || self.globals.syscom.msg_back_open {
            return;
        }
        let text = if let Some((form_id, stage_idx, mwnd_idx)) = self.globals.focused_stage_mwnd {
            self.globals
                .stage_forms
                .get(&form_id)
                .and_then(|st| st.mwnd_lists.get(&stage_idx))
                .and_then(|list| list.get(mwnd_idx))
                .and_then(|m| m.selection.as_ref())
                .map(|sel| {
                    let mut lines = Vec::new();
                    lines.push("Select".to_string());
                    for (i, choice) in sel.choices.iter().enumerate() {
                        let cursor = if i == sel.cursor { ">" } else { " " };
                        lines.push(format!("{cursor} {}", choice.text));
                    }
                    if sel.cancel_enable {
                        lines.push("[Esc] Cancel".to_string());
                    }
                    lines.join("\n")
                })
        } else {
            None
        };
        if let Some(text) = text {
            self.ui.set_sys_overlay(true, text);
        } else {
            self.ui.set_sys_overlay(false, String::new());
        }
    }

    fn sync_movie_objects(&mut self) {
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
        let trace = std::env::var_os("SG_MOVIE_TRACE").is_some();
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
            if self.movie.audio_playback_finished(id) {
                self.globals.mov.audio_id = None;
                self.globals.mov.audio_start_attempted = false;
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
                    .start_audio(&mut self.audio, track, self.globals.mov.timer_ms)
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
        apply_quake(&self.globals, &mut list);
        apply_button_visuals(self, &mut list);
        self.apply_gan_effects(&mut list);
        apply_screen_effects(&self.globals, &self.ids, &mut list);
        apply_syscom_filter(self, &mut list);
        (list, debug_lines)
    }

    pub fn render_list_with_effects(&mut self) -> Vec<RenderSprite> {
        let (pre_wipe_list, debug_lines) = self.build_render_list_pre_wipe();
        let mut list = if self.globals.wipe.is_some() {
            let base = self.layers.render_list();
            let (next_list, next_debug_lines) = build_siglus_object_render_list(self, &base, TNM_STAGE_NEXT_I64);
            if config_button_trace_enabled() {
                eprintln!(
                    "[SG_DEBUG][CONFIG_BUTTON_TRACE][RENDER_PHASE] wipe_active=true pre_wipe_len={} next_len={} next_debug_lines={} wipe_type={:?}",
                    pre_wipe_list.len(),
                    next_list.len(),
                    next_debug_lines.len(),
                    self.globals.wipe.as_ref().map(|w| w.wipe_type)
                );
                for line in next_debug_lines.iter().filter(|line| line.contains("CONFIG_BUTTON_TRACE")) {
                    eprintln!("{}", line);
                }
            }
            if let Some(composed) = build_dual_source_wipe_list(self, &pre_wipe_list, &next_list) {
                if config_button_trace_enabled() {
                    eprintln!("[SG_DEBUG][CONFIG_BUTTON_TRACE][RENDER_PHASE] wipe_compose=dual_source");
                }
                composed
            } else if let Some(composed) =
                build_regular_stage_wipe_list(self, &pre_wipe_list, &next_list)
            {
                if config_button_trace_enabled() {
                    eprintln!("[SG_DEBUG][CONFIG_BUTTON_TRACE][RENDER_PHASE] wipe_compose=regular");
                }
                composed
            } else {
                if config_button_trace_enabled() {
                    eprintln!("[SG_DEBUG][CONFIG_BUTTON_TRACE][RENDER_PHASE] wipe_compose=effect_fallback");
                }
                let mut l = pre_wipe_list.clone();
                apply_wipe_effect(self, &mut l);
                l.retain(render_sprite_visible_for_submit);
                l
            }
        } else {
            if config_button_trace_enabled() {
                eprintln!(
                    "[SG_DEBUG][CONFIG_BUTTON_TRACE][RENDER_PHASE] wipe_active=false pre_wipe_len={}",
                    pre_wipe_list.len()
                );
            }
            pre_wipe_list.clone()
        };
        let before_retain_len = list.len();
        list.retain(render_sprite_visible_for_submit);
        if config_button_trace_enabled() && before_retain_len != list.len() {
            eprintln!(
                "[SG_DEBUG][CONFIG_BUTTON_TRACE][RENDER_PHASE] final_retain before={} after={}",
                before_retain_len,
                list.len()
            );
        }
        if config_button_trace_enabled() {
            trace_final_render_order(self, &list);
        }
        overlay_precompose_if_needed(self, &mut list);
        if self.globals.wipe.is_none() {
            self.last_presented_render_list = pre_wipe_list.clone();
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
            eprintln!("[SG_DEBUG] submitted_render_list len={}", list.len());
            for (i, rs) in list.iter().enumerate() {
                eprintln!(
                    "[SG_DEBUG]   render[{}] layer={:?} sprite={:?} img={:?} pos=({}, {}) sorter=({}, {}) order={} alpha={} tr={} alpha_blend={} blend={:?} fit={:?} size={:?} dst_clip={:?} src_clip={:?} scale=({:.3}, {:.3}) rot={:.3} anchor={} tex_center=({:.3},{:.3}) pivot=({:.3},{:.3},{:.3})",
                    i,
                    rs.layer_id,
                    rs.sprite_id,
                    rs.sprite.image_id,
                    rs.sprite.x,
                    rs.sprite.y,
                    rs.sorter_order,
                    rs.sorter_layer,
                    rs.sprite.order,
                    rs.sprite.alpha,
                    rs.sprite.tr,
                    rs.sprite.alpha_blend,
                    rs.sprite.blend,
                    rs.sprite.fit,
                    rs.sprite.size_mode,
                    rs.sprite.dst_clip,
                    rs.sprite.src_clip,
                    rs.sprite.scale_x,
                    rs.sprite.scale_y,
                    rs.sprite.rotate,
                    rs.sprite.object_anchor,
                    rs.sprite.texture_center_x,
                    rs.sprite.texture_center_y,
                    rs.sprite.pivot_x,
                    rs.sprite.pivot_y,
                    rs.sprite.pivot_z,
                );
            }
        }
        list
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

    /// Capture the current frame (UI + scene) into a CPU RGBA buffer.
    pub fn capture_frame_rgba(&mut self) -> RgbaImage {
        let sprites = self.render_list_with_effects();
        soft_render::render_to_image(&self.images, &sprites, self.screen_w, self.screen_h)
    }

    /// Capture only sprites up to the original engine order/layer cut line.
    pub fn capture_frame_rgba_until(&mut self, end_order: i64, end_layer: i64) -> RgbaImage {
        let order = end_order.clamp(i32::MIN as i64 / 1024, i32::MAX as i64 / 1024);
        let layer = end_layer.clamp(-1023, 1023);
        let limit = order
            .saturating_mul(1024)
            .saturating_add(layer)
            .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        let mut sprites = self.render_list_with_effects();
        sprites.retain(|rs| rs.sprite.order <= limit);
        soft_render::render_to_image(&self.images, &sprites, self.screen_w, self.screen_h)
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
    matches!(
        std::env::var("SG_DEBUG").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn sg_input_trace_enabled() -> bool {
    matches!(
        std::env::var("SG_INPUT_TRACE").ok().as_deref(),
        Some("1") | Some("true") | Some("TRUE") | Some("yes") | Some("YES")
    )
}

fn sg_mwnd_object_trace_enabled() -> bool {
    sg_debug_enabled()
}

fn sg_render_tree_debug_enabled() -> bool {
    sg_debug_enabled()
}

fn config_button_trace_enabled() -> bool {
    sg_debug_enabled()
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
        globals::ObjectBackend::Rect {
            layer_id: lid,
            sprite_id: sid,
            ..
        }
        | globals::ObjectBackend::String {
            layer_id: lid,
            sprite_id: sid,
            ..
        }
        | globals::ObjectBackend::Movie {
            layer_id: lid,
            sprite_id: sid,
            ..
        } => *lid == layer_id && *sid == sprite_id,
        globals::ObjectBackend::Number {
            layer_id: lid,
            sprite_ids,
        }
        | globals::ObjectBackend::Weather {
            layer_id: lid,
            sprite_ids,
        } => *lid == layer_id && sprite_ids.iter().any(|sid| *sid == sprite_id),
        globals::ObjectBackend::None => false,
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
        } => {
            push_one(layers, *layer_id, *sprite_id, &mut out);
        }
        globals::ObjectBackend::Number {
            layer_id,
            sprite_ids,
        }
        | globals::ObjectBackend::Weather {
            layer_id,
            sprite_ids,
        } => {
            for sid in sprite_ids {
                push_one(layers, *layer_id, *sid, &mut out);
            }
        }
        globals::ObjectBackend::None => {}
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

fn intersect_clip_rect(lhs: ClipRect, rhs: ClipRect) -> Option<ClipRect> {
    let left = lhs.left.max(rhs.left);
    let top = lhs.top.max(rhs.top);
    let right = lhs.right.min(rhs.right);
    let bottom = lhs.bottom.min(rhs.bottom);
    if left < right && top < bottom {
        Some(ClipRect {
            left,
            top,
            right,
            bottom,
        })
    } else {
        None
    }
}

fn transform_clip_rect_by_parent(clip: ClipRect, parent: &ParentRenderState) -> ClipRect {
    let (sin_z, cos_z) = parent.rotate_z.sin_cos();
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for (x, y) in [
        (clip.left as f32, clip.top as f32),
        (clip.right as f32, clip.top as f32),
        (clip.left as f32, clip.bottom as f32),
        (clip.right as f32, clip.bottom as f32),
    ] {
        let rel_x = (x - parent.center_rep_x) * parent.scale_x;
        let rel_y = (y - parent.center_rep_y) * parent.scale_y;
        let rot_x = rel_x * cos_z - rel_y * sin_z;
        let rot_y = rel_x * sin_z + rel_y * cos_z;
        let tx = parent.pos_x + parent.center_rep_x + rot_x;
        let ty = parent.pos_y + parent.center_rep_y + rot_y;
        min_x = min_x.min(tx);
        min_y = min_y.min(ty);
        max_x = max_x.max(tx);
        max_y = max_y.max(ty);
    }

    ClipRect {
        left: min_x.floor() as i32,
        top: min_y.floor() as i32,
        right: max_x.ceil() as i32,
        bottom: max_y.ceil() as i32,
    }
}

fn compose_clip_rect(
    parent_clip: Option<ClipRect>,
    child_clip: Option<ClipRect>,
    parent: &ParentRenderState,
) -> Option<ClipRect> {
    match (parent_clip, child_clip) {
        (Some(parent_clip), Some(child_clip)) => intersect_clip_rect(
            parent_clip,
            transform_clip_rect_by_parent(child_clip, parent),
        ),
        (Some(parent_clip), None) => Some(parent_clip),
        (None, Some(child_clip)) => Some(transform_clip_rect_by_parent(child_clip, parent)),
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

    cur.dst_clip = compose_clip_rect(parent.dst_clip, child_clip, &parent);

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
                globals::ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                }
                | globals::ObjectBackend::String {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = layers.layer_mut(*layer_id) {
                        if let Some(spr) = layer.sprite_mut(*sprite_id) {
                            if let Some(ax) = x {
                                spr.x = ax as i32;
                            }
                            if let Some(ay) = y {
                                spr.y = ay as i32;
                            }
                            if let Some(v) = alpha {
                                spr.alpha = v.clamp(0, 255) as u8;
                            }
                            if let Some(v) = order {
                                spr.order = v as i32;
                            }
                            if let Some(v) = tr {
                                spr.tr = v.clamp(0, 255) as u8;
                            }
                        }
                    }
                }
                globals::ObjectBackend::Number { .. }
                | globals::ObjectBackend::Weather { .. }
                | globals::ObjectBackend::Movie { .. }
                | globals::ObjectBackend::None => {}
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
        1 => {
            let pat_time = p.pat_time.max(1);
            let t = obj
                .weather_work
                .sub
                .get(idx)
                .map(|s| s.move_cur_time.max(0))
                .unwrap_or(0);
            first + ((t / pat_time) % span)
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
    images: &mut ImageManager,
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
    sprite.x = obj
        .lookup_int_prop(ids, ids.obj_x)
        .unwrap_or(0)
        .saturating_add(x) as i32;
    sprite.y = obj
        .lookup_int_prop(ids, ids.obj_y)
        .unwrap_or(0)
        .saturating_add(y) as i32;
    sprite.alpha = if ids.obj_alpha != 0 {
        obj.lookup_int_prop(ids, ids.obj_alpha)
            .unwrap_or(obj.base.alpha)
    } else {
        obj.base.alpha
    }
    .clamp(0, 255) as u8;
    sprite.tr = ((obj
        .lookup_int_prop(ids, ids.obj_tr)
        .unwrap_or(255)
        .clamp(0, 255)
        * alpha as i64)
        / 255)
        .clamp(0, 255) as u8;
    sprite.order = obj.lookup_int_prop(ids, ids.obj_order).unwrap_or(0) as i32;
    sprite.scale_x = (scale_x as f32) / 1000.0;
    sprite.scale_y = (scale_y as f32) / 1000.0;
    sprite.blend =
        crate::layer::SpriteBlend::from_i64(obj.lookup_int_prop(ids, ids.obj_blend).unwrap_or(0));
    if let Some(img) = image_id.and_then(|id| images.get(id)) {
        if matches!(sprite.size_mode, SpriteSizeMode::Intrinsic) {
            let _ = (img.width, img.height);
        }
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
                    1000i64.saturating_mul(sub.move_cur_time) / sub.move_time_x
                };
                let move_y = if sub.move_time_y == 0 {
                    0
                } else {
                    1000i64.saturating_mul(sub.move_cur_time) / sub.move_time_y
                };
                let mut x = sub.move_start_pos_x
                    + move_x
                    + weather_wave(sub.sin_cur_time, sub.sin_time_x, sub.sin_power_x);
                let mut y = sub.move_start_pos_y
                    + move_y
                    + weather_wave(sub.sin_cur_time, sub.sin_time_y, sub.sin_power_y);
                x = ((x % screen_w) + screen_w) % screen_w;
                y = ((y % screen_h) + screen_h) % screen_h;
                let offsets = [
                    (0, 0),
                    (-screen_w, 0),
                    (0, -screen_h),
                    (-screen_w, -screen_h),
                ];
                for (ox, oy) in offsets {
                    if let Some(&sid) = sprite_ids.get(used) {
                        set_weather_sprite(
                            ids,
                            layers,
                            images,
                            obj,
                            layer_id,
                            sid,
                            image_id,
                            x + ox,
                            y + oy,
                            alpha,
                            sub.scale_x,
                            sub.scale_y,
                        );
                    }
                    used += 1;
                }
            } else {
                let mt = sub.move_time_x.max(1);
                let t = sub.move_cur_time.max(0);
                let distance = sub.move_start_distance.saturating_add(
                    1000i64.saturating_mul(t).saturating_mul(t) / mt.saturating_mul(mt),
                );
                let degree = sub.move_start_degree
                    + if sub.center_rotate == 0 {
                        0
                    } else {
                        sub.center_rotate.saturating_mul(t) / 1000
                    };
                let rad = degree as f64 / WEATHER_ANGLE_FULL * std::f64::consts::TAU;
                let wave_x = weather_wave(sub.sin_cur_time, sub.sin_time_x, sub.sin_power_x);
                let wave_y = weather_wave(sub.sin_cur_time, sub.sin_time_y, sub.sin_power_y);
                let x = obj.weather_param.center_x
                    + (rad.cos() * distance as f64).round() as i64
                    + wave_x;
                let y = obj.weather_param.center_y
                    + (rad.sin() * distance as f64).round() as i64
                    + wave_y;
                let zoom_span = sub.zoom_max.saturating_sub(sub.zoom_min);
                let zoom = if sub.active_time_len <= 0 {
                    sub.zoom_min
                } else {
                    sub.zoom_min
                        + zoom_span.saturating_mul(t.min(sub.active_time_len)) / sub.active_time_len
                };
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
                        sub.scale_x.saturating_mul(zoom) / 1000,
                        sub.scale_y.saturating_mul(zoom) / 1000,
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

    let next_cursor = obj.movie.frame_image_cursor ^ 1;
    let img_id = if let Some(id) = obj.movie.frame_image_ids[next_cursor] {
        let _ = images.replace_image_arc(id, frame.clone());
        id
    } else {
        let id = images.insert_image_arc(frame.clone());
        obj.movie.frame_image_ids[next_cursor] = Some(id);
        id
    };
    obj.movie.frame_image_cursor = next_cursor;

    *image_id = Some(img_id);
    *width = frame.width;
    *height = frame.height;
    if let Some(layer) = layers.layer_mut(*layer_id) {
        if let Some(sprite) = layer.sprite_mut(*sprite_id) {
            sprite.image_id = Some(img_id);
        }
    }
    if trace || sg_debug_enabled() {
        eprintln!(
            "[SG_DEBUG][MOV] object_movie.frame stage={} obj={} file={} frame={} image={:?} size={}x{} timer_ms={}",
            stage_idx, obj_idx, file, frame_idx, img_id, frame.width, frame.height, obj.movie.timer_ms
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
    let trace = std::env::var_os("SG_MOVIE_TRACE").is_some();
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
                    // Original OMV objects are freed after the player has actually
                    // reached EOS.  Keep the object alive if no decoded frame was ever
                    // installed; otherwise metadata/timing mismatches can erase a movie
                    // object immediately after CREATE_MOVIE.
                    if obj.movie.last_frame_idx.is_some() {
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
                    } else {
                        obj.movie.playing = true;
                    }
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
                    }
                }

                // Object movie sprites need a texture immediately after CREATE_MOVIE.
                // The streaming decoder can return None while its worker is warming up;
                // without this preview surface the object stays as image_id=None/0x0 and
                // is filtered out by render submission. The stream path below replaces it.
                install_object_movie_preview_if_missing(
                    layers, movie_mgr, images, obj, stage_idx, obj_idx, file, trace,
                );

                if obj.movie.seeked || obj.movie.just_looped {
                    if let Some(id) = obj.movie.audio_id.take() {
                        movie_mgr.stop_audio(id);
                    }
                }
                obj.movie.seeked = false;
                obj.movie.just_looped = false;

                if obj.movie.pause_flag {
                    if let Some(id) = obj.movie.audio_id {
                        movie_mgr.pause_audio(id);
                    }
                } else if obj.movie.playing {
                    if let Some(id) = obj.movie.audio_id {
                        movie_mgr.resume_audio(id);
                    }
                }

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
                if let Some(id) = obj.movie.audio_id {
                    if movie_mgr.audio_playback_finished(id) {
                        obj.movie.audio_id = None;
                    }
                }
                let polled = match movie_mgr.poll_global_movie_frame(file, obj.movie.timer_ms) {
                    Ok(Some(frame)) => frame,
                    Ok(None) => {
                        if obj.movie.last_frame_idx.is_none() {
                            obj.movie.timer_ms = 0;
                            obj.movie.last_tick = Some(std::time::Instant::now());
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
                        if let Ok(id) = movie_mgr.start_audio(audio, track, obj.movie.timer_ms) {
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
                    obj.movie.last_tick = Some(std::time::Instant::now());
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
    images: &mut ImageManager,
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
    if mask_no >= 0 {
        let mask_idx = mask_no as usize;
        if let Some(Some((mask_name, mask_x, mask_y))) = mask_info.get(mask_idx) {
            if let Some(mask_image_id) = resolved_masks.get(mask_name).copied() {
                let targets: Vec<(LayerId, SpriteId)> = match &obj.backend {
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
                    } => vec![(*layer_id, *sprite_id)],
                    globals::ObjectBackend::Number {
                        layer_id,
                        sprite_ids,
                    }
                    | globals::ObjectBackend::Weather {
                        layer_id,
                        sprite_ids,
                    } => sprite_ids.iter().map(|sid| (*layer_id, *sid)).collect(),
                    globals::ObjectBackend::Gfx => gfx
                        .object_sprite_binding(stage_i64, obj_i64)
                        .into_iter()
                        .collect(),
                    _ => Vec::new(),
                };
                for (layer_id, sprite_id) in targets {
                    let Some(sprite) = layers
                        .layer_mut(layer_id)
                        .and_then(|l| l.sprite_mut(sprite_id))
                    else {
                        continue;
                    };
                    let Some(base_id) = sprite.image_id else {
                        continue;
                    };
                    let (base_img, base_ver) = match images.get_entry(base_id) {
                        Some(v) => v,
                        None => continue,
                    };
                    let (mask_img, mask_ver) = match images.get_entry(mask_image_id) {
                        Some(v) => v,
                        None => continue,
                    };
                    let key = (layer_id, sprite_id);
                    if let Some(cache) = obj.mask_cache.get(&key) {
                        if cache.base_image_id == base_id
                            && cache.base_version == base_ver
                            && cache.mask_image_id == mask_image_id
                            && cache.mask_version == mask_ver
                            && cache.mask_x == *mask_x
                            && cache.mask_y == *mask_y
                        {
                            sprite.image_id = Some(cache.masked_image_id);
                            continue;
                        }
                    }
                    let masked = apply_mask_image(base_img, mask_img, *mask_x, *mask_y);
                    let masked_id = images.insert_image(masked);
                    obj.mask_cache.insert(
                        key,
                        globals::MaskedSpriteCache {
                            base_image_id: base_id,
                            base_version: base_ver,
                            mask_image_id,
                            mask_version: mask_ver,
                            mask_x: *mask_x,
                            mask_y: *mask_y,
                            masked_image_id: masked_id,
                        },
                    );
                    sprite.image_id = Some(masked_id);
                }
            }
        }
    }

    for (child_idx, child) in obj.runtime.child_objects.iter_mut().enumerate() {
        apply_object_masks_recursive(
            ids,
            gfx,
            images,
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
                } => vec![(*layer_id, *sprite_id)],
                globals::ObjectBackend::Number {
                    layer_id,
                    sprite_ids,
                }
                | globals::ObjectBackend::Weather {
                    layer_id,
                    sprite_ids,
                } => sprite_ids.iter().map(|sid| (*layer_id, *sid)).collect(),
                globals::ObjectBackend::Gfx => gfx
                    .object_sprite_binding(stage_i64, obj_i64)
                    .into_iter()
                    .collect(),
                _ => Vec::new(),
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
            let key: Option<(LayerId, SpriteId)> = match &obj.backend {
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
                globals::ObjectBackend::Gfx => gfx.object_sprite_binding(stage_i64, obj_i64),
                _ => None,
            };
            if let Some((layer_id, sprite_id)) = key {
                if let Some(&idx) = index.get(&(Some(layer_id), Some(sprite_id))) {
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
                    if pat.pat_no != 0 {
                        if let Some(file) = gfx.object_peek_file(stage_i64, obj_i64) {
                            let base_pat = gfx.object_peek_patno(stage_i64, obj_i64).unwrap_or(0);
                            let pat_no = (base_pat + pat.pat_no as i64).max(0) as u32;
                            if let Ok(id) = images.load_g00(&file, pat_no) {
                                sprite.image_id = Some(id);
                            }
                        }
                    }
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
    sprite.pivot_x += state.center_rep_x;
    sprite.pivot_y += state.center_rep_y;
    sprite.pivot_z += state.center_rep_z;

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
    sprite.blend = state.blend;
    let child_clip = sprite.dst_clip;
    sprite.dst_clip = compose_clip_rect(state.dst_clip, child_clip, state);
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
    // append_object_tree_sprites() applies the original object visibility gate.
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
        if sprite.image_id.is_none() {
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
        } => {
            push_one(ctx, *layer_id, *sprite_id, visible_only, &mut out);
        }
        globals::ObjectBackend::Number {
            layer_id,
            sprite_ids,
        }
        | globals::ObjectBackend::Weather {
            layer_id,
            sprite_ids,
        } => {
            for sid in sprite_ids {
                push_one(ctx, *layer_id, *sid, visible_only, &mut out);
            }
        }
        globals::ObjectBackend::None => {}
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
        } => {
            if let Some(layer) = ctx.layers.layer(*layer_id) {
                if let Some(sprite) = layer.sprite(*sprite_id) {
                    info.disp = sprite.visible;
                    info.x = sprite.x as i64;
                    info.y = sprite.y as i64;
                    info.order = sprite.order as i64;
                    info.alpha = sprite.alpha as i64;
                    info.tr = sprite.tr as i64;
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
            if let Some(&sid) = sprite_ids.first() {
                if let Some(layer) = ctx.layers.layer(*layer_id) {
                    if let Some(sprite) = layer.sprite(sid) {
                        info.disp = sprite.visible;
                        info.x = sprite.x as i64;
                        info.y = sprite.y as i64;
                        info.order = sprite.order as i64;
                        info.alpha = sprite.alpha as i64;
                        info.tr = sprite.tr as i64;
                    }
                }
            }
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

fn append_object_tree_sprites(
    ctx: &CommandContext,
    worlds: Option<&Vec<globals::WorldState>>,
    stage_idx: i64,
    obj_idx: usize,
    obj: &globals::ObjectState,
    parent_visible: bool,
    parent_order: i64,
    parent_layer: i64,
    parent_state: Option<ParentRenderState>,
    out: &mut Vec<RenderSprite>,
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
            let out_len_before = out.len();
            append_weather_sprites(
                ctx,
                worlds,
                obj,
                &info,
                total_order,
                total_layer,
                &bound,
                out,
            );
            for rs in out[out_len_before..].iter_mut() {
                if let Some(parent) = parent_state {
                    apply_parent_render_state_to_sprite(&mut rs.sprite, &info, &parent);
                }
                finalize_object_center_rep_to_sprite(&mut rs.sprite, &info);
                apply_world_camera_mode(&mut rs.sprite, worlds, ctx.screen_w, ctx.screen_h);
                apply_runtime_light_and_fog(ctx, &mut rs.sprite);
            }
        } else {
            for mut rs in bound.drain(..) {
                apply_object_render_info_to_sprite(&mut rs.sprite, &info);
                rs.set_sorter(total_order, total_layer);
                rs.sprite.order = legacy_packed_sorter_key(total_order, total_layer);
                configure_sprite_3d(&mut rs.sprite, &info, worlds, ctx.screen_w, ctx.screen_h);
                if let Some(parent) = parent_state {
                    apply_parent_render_state_to_sprite(&mut rs.sprite, &info, &parent);
                }
                finalize_object_center_rep_to_sprite(&mut rs.sprite, &info);
                apply_world_camera_mode(&mut rs.sprite, worlds, ctx.screen_w, ctx.screen_h);
                apply_runtime_light_and_fog(ctx, &mut rs.sprite);
                if rs.sprite.tr > 0 {
                    out.push(rs);
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
            out.len(),
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

    let mut children: Vec<(usize, &globals::ObjectState)> = Vec::new();
    for (child_idx, child) in obj.runtime.child_objects.iter().enumerate() {
        if object_participates_in_tree(child) {
            children.push((child_idx, child));
        }
    }
    match info.child_sort_type {
        0 => {
            children.sort_by(|(lhs_idx, lhs), (rhs_idx, rhs)| {
                let l = effective_object_info(ctx, stage_idx, *lhs_idx, lhs);
                let r = effective_object_info(ctx, stage_idx, *rhs_idx, rhs);
                (l.order, l.layer).cmp(&(r.order, r.layer))
            });
        }
        2 => {
            children.sort_by(|(_, lhs), (_, rhs)| lhs.file_name.cmp(&rhs.file_name));
        }
        3 => {
            children.sort_by(|(lhs_idx, lhs), (rhs_idx, rhs)| {
                let l = effective_object_info(ctx, stage_idx, *lhs_idx, lhs);
                let r = effective_object_info(ctx, stage_idx, *rhs_idx, rhs);
                l.x.cmp(&r.x)
            });
        }
        4 => {
            children.sort_by(|(lhs_idx, lhs), (rhs_idx, rhs)| {
                let l = effective_object_info(ctx, stage_idx, *lhs_idx, lhs);
                let r = effective_object_info(ctx, stage_idx, *rhs_idx, rhs);
                r.x.cmp(&l.x)
            });
        }
        5 => {
            children.sort_by(|(lhs_idx, lhs), (rhs_idx, rhs)| {
                let l = effective_object_info(ctx, stage_idx, *lhs_idx, lhs);
                let r = effective_object_info(ctx, stage_idx, *rhs_idx, rhs);
                l.y.cmp(&r.y)
            });
        }
        6 => {
            children.sort_by(|(lhs_idx, lhs), (rhs_idx, rhs)| {
                let l = effective_object_info(ctx, stage_idx, *lhs_idx, lhs);
                let r = effective_object_info(ctx, stage_idx, *rhs_idx, rhs);
                r.y.cmp(&l.y)
            });
        }
        7 => {
            children.sort_by(|(lhs_idx, lhs), (rhs_idx, rhs)| {
                let l = effective_object_info(ctx, stage_idx, *lhs_idx, lhs);
                let r = effective_object_info(ctx, stage_idx, *rhs_idx, rhs);
                l.z.cmp(&r.z)
            });
        }
        8 => {
            children.sort_by(|(lhs_idx, lhs), (rhs_idx, rhs)| {
                let l = effective_object_info(ctx, stage_idx, *lhs_idx, lhs);
                let r = effective_object_info(ctx, stage_idx, *rhs_idx, rhs);
                r.z.cmp(&l.z)
            });
        }
        _ => {}
    }
    let child_tree_container = matches!(obj.backend, globals::ObjectBackend::None)
        && !obj.runtime.child_objects.is_empty();
    let recurse_children = if child_tree_container || matches!(obj.object_type, 3 | 4 | 5) {
        // Containers, STRING, NUMBER, and WEATHER keep traversing their child object list.
        // The parent node supplies the transform; sprite emission belongs to the descendants.
        parent_visible
    } else {
        visible
    };
    for (child_idx, child) in children {
        append_object_tree_sprites(
            ctx,
            worlds,
            stage_idx,
            child_idx,
            child,
            recurse_children,
            total_order,
            total_layer,
            Some(cur_parent_state),
            out,
            object_keys,
            debug_lines,
        );
    }
}

fn append_weather_sprites(
    ctx: &CommandContext,
    worlds: Option<&Vec<globals::WorldState>>,
    obj: &globals::ObjectState,
    info: &ObjectRenderInfo,
    total_order: i64,
    total_layer: i64,
    bound: &[RenderSprite],
    out: &mut Vec<RenderSprite>,
) {
    let Some(template) = bound.first() else {
        return;
    };
    let wp = &obj.weather_param;
    let cnt = wp.cnt.clamp(0, 256) as usize;
    if cnt == 0 {
        return;
    }
    let frame = ctx.globals.render_frame as f32;
    let sw = ctx.screen_w as f32;
    let sh = ctx.screen_h as f32;
    let base_x = info.x as f32;
    let base_y = info.y as f32;
    let scale_x = (wp.scale_x as f32 / 1000.0).max(0.01);
    let scale_y = (wp.scale_y as f32 / 1000.0).max(0.01);
    for i in 0..cnt {
        let phase = i as f32 / cnt.max(1) as f32;
        let mut rs = template.clone();
        let mut x = base_x;
        let mut y = base_y;
        let mut zoom = 1.0f32;
        match wp.weather_type {
            2 => {
                let period = (wp.move_time.abs().max(1)) as f32;
                let t = (frame / period + phase).fract();
                let angle =
                    (wp.center_rotate as f32 / 10.0).to_radians() + t * std::f32::consts::TAU;
                let radius = (wp.appear_range as f32) * (0.2 + ((phase * 17.0).sin().abs() * 0.8));
                x += wp.center_x as f32 + angle.cos() * radius;
                y += wp.center_y as f32 + angle.sin() * radius * 0.65;
                let zmin = wp.zoom_min as f32 / 1000.0;
                let zmax = wp.zoom_max as f32 / 1000.0;
                let mix = (0.5
                    + 0.5 * ((frame / period) * std::f32::consts::TAU + phase * 9.0).sin())
                .clamp(0.0, 1.0);
                zoom = zmin + (zmax - zmin) * mix;
            }
            _ => {
                let tx = (wp.move_time_x.abs().max(1)) as f32;
                let ty = (wp.move_time_y.abs().max(1)) as f32;
                let ux = (frame / tx + phase).fract();
                let uy = (frame / ty + phase * 1.37).fract();
                x += (ux - 0.5) * sw * 1.4;
                y += (uy - 0.5) * sh * 1.4;
                if wp.sin_time_x != 0 {
                    x += ((frame / wp.sin_time_x.abs().max(1) as f32) * std::f32::consts::TAU
                        + phase * 7.0)
                        .sin()
                        * wp.sin_power_x as f32;
                }
                if wp.sin_time_y != 0 {
                    y += ((frame / wp.sin_time_y.abs().max(1) as f32) * std::f32::consts::TAU
                        + phase * 11.0)
                        .sin()
                        * wp.sin_power_y as f32;
                }
            }
        }
        rs.sprite.x = x.round() as i32;
        rs.sprite.y = y.round() as i32;
        rs.sprite.scale_x *= scale_x * zoom;
        rs.sprite.scale_y *= scale_y * zoom;
        rs.set_sorter(total_order, total_layer);
        rs.sprite.order = legacy_packed_sorter_key(total_order, total_layer);
        if wp.active_time > 0 {
            let life = (frame + phase * wp.active_time as f32).rem_euclid(wp.active_time as f32)
                / wp.active_time as f32;
            let fade = if life < 0.1 {
                life / 0.1
            } else if life > 0.9 {
                (1.0 - life) / 0.1
            } else {
                1.0
            };
            rs.sprite.alpha = ((rs.sprite.alpha as f32) * fade.clamp(0.0, 1.0))
                .round()
                .clamp(0.0, 255.0) as u8;
        }
        configure_sprite_3d(&mut rs.sprite, info, worlds, ctx.screen_w, ctx.screen_h);
        apply_world_camera_mode(&mut rs.sprite, worlds, ctx.screen_w, ctx.screen_h);
        apply_runtime_light_and_fog(ctx, &mut rs.sprite);
        out.push(rs);
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

fn append_mwnd_embedded_object_list_sprites(
    ctx: &CommandContext,
    worlds: Option<&Vec<globals::WorldState>>,
    stage_idx: i64,
    list: &[globals::ObjectState],
    parent: ParentRenderState,
    parent_order: i64,
    parent_layer: i64,
    out: &mut Vec<RenderSprite>,
    object_keys: &mut HashSet<(LayerId, SpriteId)>,
    debug: &mut Vec<String>,
) {
    for (obj_idx, obj) in list.iter().enumerate() {
        if !object_participates_in_tree(obj) {
            continue;
        }
        append_object_tree_sprites(
            ctx,
            worlds,
            stage_idx,
            obj_idx,
            obj,
            true,
            parent_order,
            parent_layer,
            Some(parent),
            out,
            object_keys,
            debug,
        );
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

fn append_mwnd_embedded_sprites(
    ctx: &CommandContext,
    worlds: Option<&Vec<globals::WorldState>>,
    stage_idx: i64,
    m: &globals::MwndState,
    out: &mut Vec<RenderSprite>,
    object_keys: &mut HashSet<(LayerId, SpriteId)>,
    debug: &mut Vec<String>,
) {
    if ctx.globals.script.mwnd_disp_off_flag || ctx.globals.syscom.hide_mwnd.onoff {
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
        append_object_tree_sprites(
            ctx,
            worlds,
            stage_idx,
            button_idx,
            obj,
            true,
            mwnd_order,
            mwnd_layer.saturating_add(ctx.tables.mwnd_render.waku_layer_rep),
            Some(parent),
            out,
            object_keys,
            debug,
        );
    }
    for (face_idx, obj) in m.face_list.iter().enumerate() {
        if !object_participates_in_tree(obj) {
            continue;
        }
        let parent = apply_mwnd_window_anim_parent(
            mwnd_face_parent_render_state(m, face_idx, window_x, window_y),
            anim_parent,
        );
        append_object_tree_sprites(
            ctx,
            worlds,
            stage_idx,
            face_idx,
            obj,
            true,
            mwnd_order,
            mwnd_layer.saturating_add(ctx.tables.mwnd_render.face_layer_rep),
            Some(parent),
            out,
            object_keys,
            debug,
        );
    }
    let parent = apply_mwnd_window_anim_parent(
        mwnd_parent_render_state_at(m, window_x, window_y),
        anim_parent,
    );
    append_mwnd_embedded_object_list_sprites(
        ctx,
        worlds,
        stage_idx,
        &m.object_list,
        parent,
        mwnd_order,
        mwnd_layer,
        out,
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

fn normalize_mwnd_ui_sprite_sorter(ctx: &CommandContext, order: i32) -> (i32, i32) {
    let Some((mwnd_order, mwnd_layer)) = selected_mwnd_sort_base(ctx) else {
        return unpack_legacy_sorter_key(order);
    };
    let layer = match order {
        // UiRuntime stores C++ MWND-owned sprites with sentinel orders. Translate
        // those sentinels back to C_elm_mwnd_waku/C_elm_mwnd_moji sorter layers.
        1_000_000 => mwnd_layer.saturating_add(ctx.tables.mwnd_render.waku_layer_rep),
        1_000_005 => mwnd_layer.saturating_add(ctx.tables.mwnd_render.filter_layer_rep),
        1_000_008 => mwnd_layer.saturating_add(ctx.tables.mwnd_render.face_layer_rep),
        1_000_010 | 1_000_020 => {
            mwnd_layer.saturating_add(ctx.tables.mwnd_render.moji_layer_rep)
        }
        1_000_030 => mwnd_layer.saturating_add(ctx.tables.mwnd_render.waku_layer_rep),
        _ => return unpack_legacy_sorter_key(order),
    };
    (
        mwnd_order.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        layer.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
    )
}

const TNM_STAGE_FRONT_I64: i64 = 1;
const TNM_STAGE_NEXT_I64: i64 = 2;

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
    let mut object_list = Vec::new();
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
                            quake.until.is_some(),
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
                if let Some((focused_form, focused_stage, focused_idx)) = focused_mwnd {
                    if focused_form == form_id && focused_stage == stage_idx {
                        if let Some(mwnds) = st.mwnd_lists.get(&stage_idx) {
                            if let Some(m) = mwnds.get(focused_idx) {
                                append_mwnd_embedded_sprites(
                                    ctx,
                                    worlds,
                                    stage_idx,
                                    m,
                                    &mut object_list,
                                    &mut object_keys,
                                    &mut debug,
                                );
                            }
                        }
                    }
                }
                continue;
            }
            if let Some(list) = st.object_lists.get(&stage_idx) {
                let mut top: Vec<(usize, &globals::ObjectState)> = list
                    .iter()
                    .enumerate()
                    .filter(|(obj_idx, o)| {
                        !st.is_embedded_object_slot(stage_idx, *obj_idx)
                            && object_participates_in_tree(o)
                    })
                    .collect();
                top.sort_by(|(lhs_idx, lhs), (rhs_idx, rhs)| {
                    let l = effective_object_info(ctx, stage_idx, *lhs_idx, lhs);
                    let r = effective_object_info(ctx, stage_idx, *rhs_idx, rhs);
                    (l.order, l.layer).cmp(&(r.order, r.layer))
                });
                for (obj_idx, obj) in top {
                    append_object_tree_sprites(
                        ctx,
                        worlds,
                        stage_idx,
                        obj_idx,
                        obj,
                        true,
                        0,
                        0,
                        None,
                        &mut object_list,
                        &mut object_keys,
                        &mut debug,
                    );
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
                    append_mwnd_embedded_sprites(
                        ctx,
                        worlds,
                        stage_idx,
                        m,
                        &mut object_list,
                        &mut object_keys,
                        &mut debug,
                    );
                }
            }
        }
    }

    let mut bg = Vec::new();
    let mut rest = Vec::new();
    for rs in base {
        match (rs.layer_id, rs.sprite_id) {
            (Some(lid), Some(sid)) if object_keys.contains(&(lid, sid)) => {}
            (None, None) if render_sprite_visible_for_submit(rs) => bg.push(rs.clone()),
            (None, None) => {}
            _ if render_sprite_visible_for_submit(rs) => rest.push(rs.clone()),
            _ => {}
        }
    }

    let rest_len = rest.len();
    let mut ordered: Vec<(i32, i32, usize, RenderSprite)> =
        Vec::with_capacity(rest.len() + object_list.len());
    for (idx, mut rs) in rest.into_iter().enumerate() {
        // LayerManager ids are storage handles. They are not Siglus script-layer
        // values. For MWND UI-runtime sprites, translate the sentinel order into
        // the same C++ S_tnm_sorter(order, layer) pair that C_elm_mwnd_waku uses.
        let (order, layer) = normalize_mwnd_ui_sprite_sorter(ctx, rs.sprite.order);
        rs.set_sorter(order as i64, layer as i64);
        rs.sprite.order = legacy_packed_sorter_key(order as i64, layer as i64);
        ordered.push((order, layer, idx, rs));
    }
    for (idx, rs) in object_list.into_iter().enumerate() {
        // Object tree sprites carry the original C++ S_tnm_sorter(order, layer)
        // separately from the backend layer id. Do not derive ordering from
        // LayerManager; it is only storage.
        ordered.push((
            rs.sorter_order,
            rs.sorter_layer,
            rest_len.saturating_add(idx),
            rs,
        ));
    }
    ordered.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));

    let mut final_list = Vec::with_capacity(bg.len() + ordered.len());
    final_list.extend(bg);
    final_list.extend(ordered.into_iter().map(|(_, _, _, rs)| rs));
    (final_list, debug)
}

fn trace_codes_enabled() -> bool {
    std::env::var_os("SIGLUS_TRACE_CODES").is_some()
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

pub fn dispatch_named_command(
    ctx: &mut CommandContext,
    name: &str,
    args: &[Value],
) -> Result<bool> {
    let cmd = Command {
        name: name.to_string(),
        code: None,
        args: args.to_vec(),
    };

    if commands::misc::handle(ctx, &cmd)? {
        return Ok(true);
    }
    if commands::text::handle(ctx, &cmd)? {
        return Ok(true);
    }
    if commands::audio::handle(ctx, &cmd)? {
        return Ok(true);
    }
    if commands::bg::handle(ctx, &cmd)? {
        return Ok(true);
    }
    if commands::chr::handle(ctx, &cmd)? {
        return Ok(true);
    }
    if commands::layer::handle(ctx, &cmd)? {
        return Ok(true);
    }

    Ok(false)
}

pub fn dispatch(ctx: &mut CommandContext, cmd: &Command) -> Result<()> {
    if let Some(code) = cmd.code {
        let handled = dispatch_form_code(ctx, code.id, &cmd.args)?;
        if !handled {
            anyhow::bail!("unhandled form code {}", code.id);
        }
        return Ok(());
    }

    let handled = dispatch_named_command(ctx, &cmd.name, &cmd.args)?;
    if !handled {
        anyhow::bail!("unhandled command {}", cmd.name);
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
            ObjectBackend::String {
                layer_id,
                sprite_id,
                ..
            } => {
                map.insert((*layer_id, *sprite_id), visual.clone());
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
        if let Ok(image_id) = images.load_g00(file_name, patno) {
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

fn apply_quake(globals: &globals::GlobalState, sprites: &mut [RenderSprite]) {
    let mut dx_total: f32 = 0.0;
    let mut dy_total: f32 = 0.0;
    let mut screen_form_ids: Vec<u32> = globals.screen_forms.keys().copied().collect();
    screen_form_ids.sort_unstable();
    for form_id in screen_form_ids {
        let Some(st) = globals.screen_forms.get(&form_id) else {
            continue;
        };
        if let Some(t) = st.shake.until {
            if std::time::Instant::now() < t {
                let power = 6.0f32;
                let ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as f32;
                dx_total += (ms * 0.021).sin() * power;
                dy_total += (ms * 0.019).cos() * power;
            }
        }
        for quake in &st.quake_list {
            if quake.until.is_none() {
                continue;
            }
            let power = quake.power.min(32) as f32;
            if power <= 0.0 {
                continue;
            }
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as f32;
            let mut dx = (t * 0.02).sin() * power;
            let mut dy = (t * 0.017).cos() * power;
            if quake.vec != 0 {
                let angle = (quake.vec as f32) * std::f32::consts::PI / 180.0;
                let (s, c) = angle.sin_cos();
                let rx = dx * c - dy * s;
                let ry = dx * s + dy * c;
                dx = rx;
                dy = ry;
            }
            dx_total += dx;
            dy_total += dy;
        }
    }

    let mut stage_quakes: Vec<globals::ScreenQuakeState> = Vec::new();
    let mut stage_form_ids: Vec<u32> = globals.stage_forms.keys().copied().collect();
    stage_form_ids.sort_unstable();
    for form_id in stage_form_ids {
        let Some(st) = globals.stage_forms.get(&form_id) else {
            continue;
        };
        let mut stage_ids: Vec<i64> = st.quake_lists.keys().copied().collect();
        stage_ids.sort_unstable();
        for stage_idx in stage_ids {
            if stage_idx != TNM_STAGE_FRONT_I64 {
                continue;
            }
            if let Some(quakes) = st.quake_lists.get(&stage_idx) {
                stage_quakes.extend(quakes.iter().filter(|q| q.until.is_some()).cloned());
            }
        }
    }

    let apply_to_all = dx_total != 0.0 || dy_total != 0.0;
    if !apply_to_all && stage_quakes.is_empty() {
        return;
    }

    for rs in sprites.iter_mut() {
        let mut dx = if apply_to_all { dx_total } else { 0.0 };
        let mut dy = if apply_to_all { dy_total } else { 0.0 };
        for quake in &stage_quakes {
            if !quake_order_affects_sprite(quake, rs) {
                continue;
            }
            let power = quake.power.min(32) as f32;
            if power <= 0.0 {
                continue;
            }
            let t = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as f32;
            let mut qdx = (t * 0.02).sin() * power;
            let mut qdy = (t * 0.017).cos() * power;
            if quake.vec != 0 {
                let angle = (quake.vec as f32) * std::f32::consts::PI / 180.0;
                let (s, c) = angle.sin_cos();
                let rx = qdx * c - qdy * s;
                let ry = qdx * s + qdy * c;
                qdx = rx;
                qdy = ry;
            }
            dx += qdx;
            dy += qdy;
        }
        if dx != 0.0 || dy != 0.0 {
            rs.sprite.x = rs.sprite.x.saturating_add(dx as i32);
            rs.sprite.y = rs.sprite.y.saturating_add(dy as i32);
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct EffectParam {
    x: i32,
    y: i32,
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

fn apply_screen_effects(
    globals: &globals::GlobalState,
    ids: &constants::RuntimeConstants,
    sprites: &mut [RenderSprite],
) {
    let effects = collect_screen_effects(globals, ids);
    if effects.is_empty() {
        return;
    }
    for effect in &effects {
        for rs in sprites.iter_mut() {
            if !in_sorter_range(rs, effect) {
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

fn collect_screen_effects(
    globals: &globals::GlobalState,
    _ids: &constants::RuntimeConstants,
) -> Vec<EffectParam> {
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

    let mut stage_form_ids: Vec<u32> = globals.stage_forms.keys().copied().collect();
    stage_form_ids.sort_unstable();
    for form_id in stage_form_ids {
        let Some(st) = globals.stage_forms.get(&form_id) else {
            continue;
        };
        let mut stage_ids: Vec<i64> = st.effect_lists.keys().copied().collect();
        stage_ids.sort_unstable();
        for stage_idx in stage_ids {
            if stage_idx != TNM_STAGE_FRONT_I64 {
                continue;
            }
            let Some(effects) = st.effect_lists.get(&stage_idx) else {
                continue;
            };
            for effect in effects {
                let rp = effect_param_from_state(effect);
                if effect_is_use(&rp) {
                    out.push(rp);
                }
            }
        }
    }
    out
}

fn effect_is_use(effect: &EffectParam) -> bool {
    effect.x != 0
        || effect.y != 0
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

fn render_sprite_sorter(rs: &RenderSprite) -> (i32, i32) {
    // LayerId/SpriteId are backend storage handles and must not be used for
    // Siglus wipe/effect ranges. Use the C++ S_tnm_sorter pair carried by the
    // submitted render sprite.
    (rs.sorter_order, rs.sorter_layer)
}

fn classify_wipe_partition(
    rs: &RenderSprite,
    begin_layer: i32,
    end_layer: i32,
    begin_order: i32,
    end_order: i32,
    with_low: bool,
) -> WipePartition {
    let (order, layer) = render_sprite_sorter(rs);
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

fn upsert_runtime_image_slot(
    images: &mut ImageManager,
    slot: &mut Option<ImageId>,
    img: RgbaImage,
) -> ImageId {
    if let Some(id) = *slot {
        let _ = images.replace_image(id, img);
        id
    } else {
        let id = images.insert_image(img);
        *slot = Some(id);
        id
    }
}

fn overlay_precompose_if_needed(ctx: &mut CommandContext, _sprites: &mut Vec<RenderSprite>) {
    // Overlay composition now stays in the GPU renderer. Keep the runtime slot cleared so
    // stale CPU fallback images are not reused by other paths.
    ctx.overlay_rt_image = None;
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

fn scale_sprite_tr(sprite: &mut Sprite, rate: f32) {
    sprite.tr = ((sprite.tr as f32) * rate.clamp(0.0, 1.0))
        .round()
        .clamp(0.0, 255.0) as u8;
}

fn build_regular_stage_wipe_list(
    ctx: &mut CommandContext,
    front_stage: &[RenderSprite],
    next_stage: &[RenderSprite],
) -> Option<Vec<RenderSprite>> {
    let wipe = ctx.globals.wipe.as_ref()?;
    let wipe_type = wipe.wipe_type;
    if (220..=243).contains(&wipe_type) {
        return None;
    }

    let begin_layer = wipe.begin_layer;
    let end_layer = wipe.end_layer;
    let begin_order = wipe.begin_order;
    let end_order = wipe.end_order;
    let with_low = wipe.with_low_order != 0;
    let raw_progress = wipe.progress();
    let progress = match wipe.speed_mode {
        1 => raw_progress * raw_progress,
        2 => 1.0 - (1.0 - raw_progress) * (1.0 - raw_progress),
        3 => raw_progress * raw_progress * (3.0 - 2.0 * raw_progress),
        _ => raw_progress,
    };

    let mut under = Vec::new();
    let mut front_target = Vec::new();
    let mut over = Vec::new();
    for rs in front_stage.iter().cloned() {
        match classify_wipe_partition(
            &rs,
            begin_layer,
            end_layer,
            begin_order,
            end_order,
            with_low,
        ) {
            WipePartition::Under => under.push(rs),
            WipePartition::Target => front_target.push(rs),
            WipePartition::Over => over.push(rs),
        }
    }

    let mut next_target = Vec::new();
    for rs in next_stage.iter().cloned() {
        if matches!(
            classify_wipe_partition(
                &rs,
                begin_layer,
                end_layer,
                begin_order,
                end_order,
                with_low
            ),
            WipePartition::Target
        ) {
            next_target.push(rs);
        }
    }

    let mut out = Vec::new();
    out.extend(under);
    match wipe_type {
        1 => out.extend(front_target),
        2 => out.extend(next_target),
        _ => {
            out.extend(next_target);
            for mut rs in front_target {
                scale_sprite_tr(&mut rs.sprite, progress);
                out.push(rs);
            }
        }
    }
    out.extend(over);
    out.retain(render_sprite_visible_for_submit);
    Some(out)
}

fn build_dual_source_wipe_list(
    ctx: &mut CommandContext,
    current: &[RenderSprite],
    next_stage: &[RenderSprite],
) -> Option<Vec<RenderSprite>> {
    let wipe = ctx.globals.wipe.as_ref()?;
    let wipe_type = wipe.wipe_type;
    if !(220..=243).contains(&wipe_type) {
        return None;
    }

    let front = if next_stage.is_empty() {
        current.to_vec()
    } else {
        next_stage.to_vec()
    };

    let begin_layer = wipe.begin_layer;
    let end_layer = wipe.end_layer;
    let begin_order = wipe.begin_order;
    let end_order = wipe.end_order;
    let with_low = wipe.with_low_order != 0;
    let progress = wipe.progress();
    let option = wipe.option.clone();

    let mut under = Vec::new();
    let mut front_target = Vec::new();
    let mut over = Vec::new();
    for rs in front.into_iter() {
        match classify_wipe_partition(
            &rs,
            begin_layer,
            end_layer,
            begin_order,
            end_order,
            with_low,
        ) {
            WipePartition::Under => under.push(rs),
            WipePartition::Target => front_target.push(rs),
            WipePartition::Over => over.push(rs),
        }
    }
    let mut next_target = Vec::new();
    for rs in current.iter().cloned() {
        if matches!(
            classify_wipe_partition(
                &rs,
                begin_layer,
                end_layer,
                begin_order,
                end_order,
                with_low
            ),
            WipePartition::Target
        ) {
            next_target.push(rs);
        }
    }

    let front_img =
        soft_render::render_to_image(&ctx.images, &front_target, ctx.screen_w, ctx.screen_h);
    let next_img =
        soft_render::render_to_image(&ctx.images, &next_target, ctx.screen_w, ctx.screen_h);
    let front_id =
        upsert_runtime_image_slot(&mut ctx.images, &mut ctx.wipe_front_rt_image, front_img);
    let next_id = upsert_runtime_image_slot(&mut ctx.images, &mut ctx.wipe_next_rt_image, next_img);

    let mut comp = crate::layer::Sprite::default();
    comp.visible = true;
    comp.fit = SpriteFit::FullScreen;
    comp.image_id = Some(next_id);
    comp.wipe_src_image_id = Some(front_id);
    comp.alpha_blend = true;
    comp.alpha_test = false;
    comp.tr = 255;
    comp.alpha = 255;

    match wipe_type {
        220 => {
            let axis = option.get(0).copied().unwrap_or(0);
            let denom = option.get(1).copied().unwrap_or(1).max(1) as f32;
            let wave_num = option.get(2).copied().unwrap_or(3) as f32;
            let power = option.get(3).copied().unwrap_or(0) as f32;
            comp.wipe_fx_mode = if axis == 0 { 12 } else { 11 };
            comp.wipe_fx_params = [
                if axis == 0 {
                    ctx.screen_h as f32 / denom
                } else {
                    ctx.screen_w as f32 / denom
                },
                wave_num,
                power,
                progress,
            ];
        }
        221 => {
            let axis = option.get(0).copied().unwrap_or(0);
            let denom = option.get(1).copied().unwrap_or(1).max(1) as f32;
            let wave_num = option.get(2).copied().unwrap_or(3) as f32;
            let power = option.get(3).copied().unwrap_or(0) as f32;
            let front_bias = if option.get(4).copied().unwrap_or(0) != 0 {
                1.0
            } else {
                0.0
            };
            comp.wipe_fx_mode = if axis == 0 { 12 } else { 11 };
            comp.wipe_fx_params = [
                if axis == 0 {
                    ctx.screen_h as f32 / denom
                } else {
                    ctx.screen_w as f32 / denom
                },
                wave_num,
                power,
                progress,
            ];
            comp.tonecurve_row = 221.0;
            comp.tonecurve_sat = front_bias;
        }
        230 => {
            let (st, ed) = mosaic_size_pair(option.get(0).copied().unwrap_or(0));
            let cut = if progress < 0.5 {
                st + (ed - st) * (progress / 0.5)
            } else {
                ed + (st - ed) * ((progress - 0.5) / 0.5)
            };
            comp.wipe_fx_mode = 10;
            comp.wipe_fx_params = [
                cut.max(0.0005),
                ctx.screen_w as f32 / ctx.screen_h.max(1) as f32,
                progress,
                230.0,
            ];
        }
        231 => {
            let (mut st, mut ed) = mosaic_size_pair(option.get(0).copied().unwrap_or(0));
            let fade_mode = option.get(1).copied().unwrap_or(0);
            if fade_mode == 1 {
                std::mem::swap(&mut st, &mut ed);
            }
            let cut = (st + (ed - st) * progress).max(0.0005);
            comp.wipe_fx_mode = 10;
            comp.wipe_fx_params = [
                cut,
                ctx.screen_w as f32 / ctx.screen_h.max(1) as f32,
                progress,
                231.0,
            ];
            comp.tonecurve_sat = fade_mode as f32;
        }
        240 | 242 => {
            let (alpha_type, alpha_reverse, bp_type, bp_reverse, blur_coeff) = if wipe_type == 240 {
                (
                    option.get(2).copied().unwrap_or(0),
                    option.get(3).copied().unwrap_or(0),
                    option.get(4).copied().unwrap_or(0),
                    option.get(5).copied().unwrap_or(0),
                    option.get(6).copied().unwrap_or(1) as f32,
                )
            } else {
                (
                    option.get(0).copied().unwrap_or(0),
                    option.get(1).copied().unwrap_or(0),
                    option.get(2).copied().unwrap_or(0),
                    option.get(3).copied().unwrap_or(0),
                    option.get(4).copied().unwrap_or(1) as f32,
                )
            };
            let alpha_f = effect_curve(alpha_type, alpha_reverse != 0, progress);
            let bp = effect_curve(bp_type, bp_reverse != 0, progress);
            let (cx, cy) = if wipe_type == 242 {
                (0.5, 0.5)
            } else {
                (
                    option.get(0).copied().unwrap_or(ctx.screen_w as i32 / 2) as f32
                        / ctx.screen_w.max(1) as f32,
                    option.get(1).copied().unwrap_or(ctx.screen_h as i32 / 2) as f32
                        / ctx.screen_h.max(1) as f32,
                )
            };
            comp.wipe_fx_mode = 13;
            comp.wipe_fx_params = [cx, cy, bp, blur_coeff];
            comp.tonecurve_row = alpha_f;
            comp.tonecurve_sat = wipe_type as f32;
        }
        241 | 243 => {
            let (alpha_type, alpha_reverse, bp_type, bp_reverse, blur_coeff, front_bias) =
                if wipe_type == 241 {
                    (
                        option.get(2).copied().unwrap_or(0),
                        option.get(3).copied().unwrap_or(0),
                        option.get(4).copied().unwrap_or(0),
                        option.get(5).copied().unwrap_or(0),
                        option.get(6).copied().unwrap_or(1) as f32,
                        if option.get(7).copied().unwrap_or(0) == 0 {
                            1.0
                        } else {
                            0.0
                        },
                    )
                } else {
                    (
                        option.get(0).copied().unwrap_or(0),
                        option.get(1).copied().unwrap_or(0),
                        option.get(2).copied().unwrap_or(0),
                        option.get(3).copied().unwrap_or(0),
                        option.get(4).copied().unwrap_or(1) as f32,
                        if option.get(5).copied().unwrap_or(0) == 0 {
                            1.0
                        } else {
                            0.0
                        },
                    )
                };
            let alpha_f = effect_curve(alpha_type, alpha_reverse != 0, progress);
            let bp = effect_curve(bp_type, bp_reverse != 0, progress);
            comp.wipe_fx_mode = 13;
            comp.wipe_fx_params = [0.5, 0.5, bp, blur_coeff];
            comp.tonecurve_row = alpha_f;
            comp.tonecurve_sat = front_bias * 1000.0 + wipe_type as f32;
        }
        _ => return None,
    }

    let mut out = Vec::with_capacity(under.len() + 1 + over.len());
    out.extend(under);
    out.push(RenderSprite::new(None, None, comp));
    out.extend(over);
    Some(out)
}

fn apply_wipe_effect(ctx: &mut CommandContext, sprites: &mut [RenderSprite]) {
    let Some(wipe) = ctx.globals.wipe.as_mut() else {
        return;
    };
    let mut mask_cache = std::mem::take(&mut wipe.mask_cache);
    let mask_file = wipe.mask_file.clone();
    let mask_image_id = wipe.mask_image_id;
    let wipe_type = wipe.wipe_type;
    let speed_mode = wipe.speed_mode;
    let option = wipe.option.clone();
    let begin_layer = wipe.begin_layer;
    let end_layer = wipe.end_layer;
    let begin_order = wipe.begin_order;
    let end_order = wipe.end_order;
    let with_low = wipe.with_low_order != 0;
    let mut progress = wipe.progress();
    let _ = wipe;
    progress = match speed_mode {
        1 => progress * progress,
        2 => 1.0 - (1.0 - progress) * (1.0 - progress),
        3 => progress * progress * (3.0 - 2.0 * progress),
        _ => progress,
    };
    let fade = (progress * 255.0).clamp(0.0, 255.0) as u8;

    for rs in sprites.iter_mut() {
        let (order, layer) = render_sprite_sorter(rs);
        if layer < begin_layer || layer > end_layer {
            continue;
        }
        if !with_low && (order < begin_order || order > end_order) {
            continue;
        }
        if with_low && order < begin_order {
            // Include lower orders when requested.
        } else if order < begin_order || order > end_order {
            continue;
        }

        rs.sprite.wipe_fx_mode = 0;
        rs.sprite.wipe_fx_params = [0.0; 4];

        if mask_file.is_some() {
            let Some(mask_id) = mask_image_id else {
                continue;
            };
            let reverse = option.get(0).copied().unwrap_or(0) != 0;
            let t = if reverse { 1.0 - progress } else { progress };
            let bucket = (t * 255.0).round().clamp(0.0, 255.0) as u16;

            if let Some(base_id) = rs.sprite.image_id {
                let Some((base_img, base_ver)) = ctx.images.get_entry(base_id) else {
                    continue;
                };
                let Some((mask_img, mask_ver)) = ctx.images.get_entry(mask_id) else {
                    continue;
                };

                let key = (base_id, base_ver, mask_id, mask_ver, bucket);
                if let Some(&masked_id) = mask_cache.get(&key) {
                    rs.sprite.image_id = Some(masked_id);
                } else {
                    let masked = apply_wipe_mask_image(base_img, mask_img, t);
                    let masked_id = ctx.images.insert_image(masked);
                    mask_cache.insert(key, masked_id);
                    rs.sprite.image_id = Some(masked_id);
                }
            }

            rs.sprite.tr = ((rs.sprite.tr as f32) * (fade as f32 / 255.0)) as u8;
            continue;
        }

        match wipe_type {
            1 | 2 | 3 | 4 | 5 | 6 => {
                if let Some((left, top, right, bottom)) = sprite_bounds(&rs.sprite, ctx) {
                    let w = (right - left).max(1);
                    let h = (bottom - top).max(1);
                    let clip = match wipe_type {
                        1 => {
                            let x = left + ((w as f32) * progress) as i32;
                            ClipRect {
                                left,
                                top,
                                right: x,
                                bottom,
                            }
                        }
                        2 => {
                            let x = right - ((w as f32) * progress) as i32;
                            ClipRect {
                                left: x,
                                top,
                                right,
                                bottom,
                            }
                        }
                        3 => {
                            let y = top + ((h as f32) * progress) as i32;
                            ClipRect {
                                left,
                                top,
                                right,
                                bottom: y,
                            }
                        }
                        4 => {
                            let y = bottom - ((h as f32) * progress) as i32;
                            ClipRect {
                                left,
                                top: y,
                                right,
                                bottom,
                            }
                        }
                        5 => {
                            let cx = left + w / 2;
                            let cy = top + h / 2;
                            let hw = ((w as f32) * progress / 2.0) as i32;
                            let hh = ((h as f32) * progress / 2.0) as i32;
                            ClipRect {
                                left: cx - hw,
                                top: cy - hh,
                                right: cx + hw,
                                bottom: cy + hh,
                            }
                        }
                        6 => {
                            let cx = left + w / 2;
                            let cy = top + h / 2;
                            let hw = ((w as f32) * (1.0 - progress) / 2.0) as i32;
                            let hh = ((h as f32) * (1.0 - progress) / 2.0) as i32;
                            ClipRect {
                                left: cx - hw,
                                top: cy - hh,
                                right: cx + hw,
                                bottom: cy + hh,
                            }
                        }
                        _ => ClipRect {
                            left,
                            top,
                            right,
                            bottom,
                        },
                    };
                    rs.sprite.dst_clip = Some(clip);
                    rs.sprite.tr = ((rs.sprite.tr as f32) * (fade as f32 / 255.0)) as u8;
                } else {
                    rs.sprite.tr = ((rs.sprite.tr as f32) * (fade as f32 / 255.0)) as u8;
                }
            }
            220 | 221 => {
                if let Some((left, top, right, bottom)) = sprite_bounds(&rs.sprite, ctx) {
                    let w = (right - left).max(1) as f32;
                    let h = (bottom - top).max(1) as f32;
                    let denom = option.get(1).copied().unwrap_or(1).max(1) as f32;
                    let wave_num = option.get(2).copied().unwrap_or(3) as f32;
                    let power = option.get(3).copied().unwrap_or(0) as f32;
                    let reverse = option.get(4).copied().unwrap_or(0) != 0;
                    let progress_eff = if wipe_type == 221 && reverse {
                        1.0 - progress
                    } else {
                        progress
                    };
                    rs.sprite.wipe_fx_mode = if option.get(0).copied().unwrap_or(0) == 0 {
                        3
                    } else {
                        2
                    };
                    rs.sprite.wipe_fx_params = [
                        if option.get(0).copied().unwrap_or(0) == 0 {
                            h / denom
                        } else {
                            w / denom
                        },
                        wave_num,
                        power,
                        progress_eff,
                    ];
                    rs.sprite.tr = ((rs.sprite.tr as f32)
                        * (255.0 * progress_eff).clamp(0.0, 255.0)
                        / 255.0) as u8;
                }
            }
            230 | 231 => {
                if let Some(id) = rs.sprite.image_id {
                    if let Some((img, _)) = ctx.images.get_entry(id) {
                        let (mut st, mut ed) =
                            mosaic_size_pair(option.get(0).copied().unwrap_or(0));
                        let mut cut = if wipe_type == 230 {
                            if progress < 0.5 {
                                st + (ed - st) * (progress / 0.5)
                            } else {
                                ed + (st - ed) * ((progress - 0.5) / 0.5)
                            }
                        } else {
                            if option.get(1).copied().unwrap_or(0) == 1 {
                                std::mem::swap(&mut st, &mut ed);
                            }
                            st + (ed - st) * progress
                        };
                        cut = cut.max(0.0005);
                        rs.sprite.wipe_fx_mode = 1;
                        rs.sprite.wipe_fx_params =
                            [cut, img.width as f32 / img.height.max(1) as f32, 0.0, 0.0];
                        if wipe_type == 231 {
                            let trf = if option.get(1).copied().unwrap_or(0) == 0 {
                                1.0 - progress
                            } else {
                                progress
                            };
                            rs.sprite.tr = ((rs.sprite.tr as f32) * (255.0 * trf).clamp(0.0, 255.0)
                                / 255.0) as u8;
                        }
                    }
                }
            }
            240 | 241 | 242 | 243 => {
                if let Some(id) = rs.sprite.image_id {
                    if let Some((img, _)) = ctx.images.get_entry(id) {
                        let (alpha_type, alpha_reverse, bp_type, bp_reverse, blur_coeff) =
                            if wipe_type == 240 || wipe_type == 241 {
                                (
                                    option.get(2).copied().unwrap_or(0),
                                    option.get(3).copied().unwrap_or(0) != 0,
                                    option.get(4).copied().unwrap_or(0),
                                    option.get(5).copied().unwrap_or(0) != 0,
                                    option.get(6).copied().unwrap_or(1) as f32,
                                )
                            } else {
                                (
                                    option.get(0).copied().unwrap_or(0),
                                    option.get(1).copied().unwrap_or(0) != 0,
                                    option.get(2).copied().unwrap_or(0),
                                    option.get(3).copied().unwrap_or(0) != 0,
                                    option.get(4).copied().unwrap_or(1) as f32,
                                )
                            };
                        let alpha_f = effect_curve(alpha_type, alpha_reverse, progress);
                        let bp = effect_curve(bp_type, bp_reverse, progress);
                        let (cx, cy) = if wipe_type == 242 || wipe_type == 243 {
                            let seed = ((rs.sprite.order as i64 * 1103515245
                                + rs.sprite.x as i64 * 12345
                                + rs.sprite.y as i64 * 34567
                                + (progress * 997.0) as i64)
                                & 0x7fffffff) as u64;
                            (
                                (seed % img.width.max(1) as u64) as f32 / img.width.max(1) as f32,
                                (((seed / 97) % img.height.max(1) as u64) as f32)
                                    / img.height.max(1) as f32,
                            )
                        } else {
                            (
                                option.get(0).copied().unwrap_or(img.width as i32 / 2) as f32
                                    / img.width.max(1) as f32,
                                option.get(1).copied().unwrap_or(img.height as i32 / 2) as f32
                                    / img.height.max(1) as f32,
                            )
                        };
                        rs.sprite.wipe_fx_mode = 4;
                        rs.sprite.wipe_fx_params = [cx, cy, bp, blur_coeff];
                        rs.sprite.tr = ((rs.sprite.tr as f32) * (255.0 * alpha_f).clamp(0.0, 255.0)
                            / 255.0) as u8;
                    }
                }
            }
            _ => {
                rs.sprite.tr = ((rs.sprite.tr as f32) * (fade as f32 / 255.0)) as u8;
            }
        }
    }

    if let Some(wipe) = ctx.globals.wipe.as_mut() {
        wipe.mask_cache = mask_cache;
    }
}

fn mosaic_size_pair(kind: i32) -> (f32, f32) {
    match kind {
        0 => (0.001, 0.025),
        1 => (0.002, 0.04),
        2 => (0.003, 0.06),
        3 => (0.004, 0.08),
        4 => (0.005, 0.10),
        5 => (0.006, 0.15),
        6 => (0.007, 0.20),
        7 => (0.008, 0.30),
        8 => (0.009, 0.40),
        9 => (0.010, 0.50),
        _ => (0.005, 0.10),
    }
}

fn effect_curve(kind: i32, reverse: bool, progress: f32) -> f32 {
    let mut v = if kind == 0 {
        1.0 - progress
    } else if kind == 10 {
        progress
    } else if (1..10).contains(&kind) {
        let threshold = kind as f32 / 10.0;
        if progress < threshold {
            if threshold <= 0.0 {
                1.0
            } else {
                progress / threshold
            }
        } else {
            let span = (1.0 - threshold).max(1e-5);
            ((1.0 - progress) / span).clamp(0.0, 1.0)
        }
    } else {
        1.0
    };
    if reverse {
        v = 1.0 - v;
    }
    v.clamp(0.0, 1.0)
}

fn sprite_bounds(sprite: &Sprite, ctx: &CommandContext) -> Option<(i32, i32, i32, i32)> {
    match sprite.fit {
        crate::layer::SpriteFit::FullScreen => {
            let w = ctx.screen_w as i32;
            let h = ctx.screen_h as i32;
            Some((0, 0, w, h))
        }
        crate::layer::SpriteFit::PixelRect => {
            let (mut w, mut h) = match sprite.size_mode {
                crate::layer::SpriteSizeMode::Explicit { width, height } => {
                    (width as i32, height as i32)
                }
                crate::layer::SpriteSizeMode::Intrinsic => {
                    let Some(id) = sprite.image_id else {
                        return None;
                    };
                    let (img, _) = ctx.images.get_entry(id)?;
                    (img.width as i32, img.height as i32)
                }
            };
            w = ((w as f32) * sprite.scale_x) as i32;
            h = ((h as f32) * sprite.scale_y) as i32;
            let left = sprite.x;
            let top = sprite.y;
            Some((left, top, left + w, top + h))
        }
    }
}

fn apply_syscom_filter(ctx: &CommandContext, sprites: &mut Vec<RenderSprite>) {
    const GET_FILTER_COLOR_R: i32 = 84;
    const GET_FILTER_COLOR_G: i32 = 91;
    const GET_FILTER_COLOR_B: i32 = 92;
    const GET_FILTER_COLOR_A: i32 = 93;
    let cfg = &ctx.globals.syscom.config_int;
    let a = cfg
        .get(&GET_FILTER_COLOR_A)
        .copied()
        .unwrap_or(0)
        .clamp(0, 255) as u8;
    if a == 0 {
        return;
    }
    let r = cfg
        .get(&GET_FILTER_COLOR_R)
        .copied()
        .unwrap_or(0)
        .clamp(0, 255) as u8;
    let g = cfg
        .get(&GET_FILTER_COLOR_G)
        .copied()
        .unwrap_or(0)
        .clamp(0, 255) as u8;
    let b = cfg
        .get(&GET_FILTER_COLOR_B)
        .copied()
        .unwrap_or(0)
        .clamp(0, 255) as u8;

    let mut s = crate::layer::Sprite::default();
    s.visible = true;
    s.image_id = Some(ctx.solid_white);
    s.fit = crate::layer::SpriteFit::FullScreen;
    s.size_mode = crate::layer::SpriteSizeMode::Intrinsic;
    s.alpha = a;
    s.color_rate = 255;
    s.color_r = r;
    s.color_g = g;
    s.color_b = b;
    s.order = i32::MAX;
    sprites.push(RenderSprite::new(None, None, s));
}

fn resolve_mask_path(project_dir: &Path, raw: &str) -> Option<PathBuf> {
    let mut norm = raw.replace('\\', "/");
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
        if c.exists() {
            return Some(c);
        }
    }
    None
}

fn apply_mask_image(base: &RgbaImage, mask: &RgbaImage, mask_x: i32, mask_y: i32) -> RgbaImage {
    let mut out = base.clone();
    let bw = base.width as i32;
    let bh = base.height as i32;
    let mw = mask.width as i32;
    let mh = mask.height as i32;

    for y in 0..bh {
        for x in 0..bw {
            let mx = x + mask_x;
            let my = y + mask_y;
            let mask_alpha = if mx >= 0 && my >= 0 && mx < mw && my < mh {
                let mi = ((my as u32 * mask.width + mx as u32) * 4) as usize;
                let mr = mask.rgba[mi] as f32 / 255.0;
                let mg = mask.rgba[mi + 1] as f32 / 255.0;
                let mb = mask.rgba[mi + 2] as f32 / 255.0;
                let ma = mask.rgba[mi + 3] as f32 / 255.0;
                let l = mr * 0.299 + mg * 0.587 + mb * 0.114;
                (l * ma).clamp(0.0, 1.0)
            } else {
                0.0
            };

            let bi = ((y as u32 * base.width + x as u32) * 4) as usize;
            let ba = out.rgba[bi + 3] as f32 / 255.0;
            let na = (ba * mask_alpha).clamp(0.0, 1.0);
            out.rgba[bi + 3] = (na * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }

    out
}

fn apply_wipe_mask_image(base: &RgbaImage, mask: &RgbaImage, threshold: f32) -> RgbaImage {
    let mut out = base.clone();
    let bw = base.width as i32;
    let bh = base.height as i32;
    let mw = mask.width as i32;
    let mh = mask.height as i32;

    for y in 0..bh {
        for x in 0..bw {
            let mx = (x * mw) / bw;
            let my = (y * mh) / bh;
            let mi = ((my as u32 * mask.width + mx as u32) * 4) as usize;
            let mr = mask.rgba[mi] as f32 / 255.0;
            let mg = mask.rgba[mi + 1] as f32 / 255.0;
            let mb = mask.rgba[mi + 2] as f32 / 255.0;
            let ma = mask.rgba[mi + 3] as f32 / 255.0;
            let l = (mr * 0.299 + mg * 0.587 + mb * 0.114) * ma;

            let bi = ((y as u32 * base.width + x as u32) * 4) as usize;
            if l > threshold {
                out.rgba[bi + 3] = 0;
            }
        }
    }

    out
}

fn build_selbtn_menu_text(sel: &globals::BtnSelectRuntimeState) -> String {
    let mut s = String::from(
        "SELBTN
Esc: cancel  Enter/Click: decide
",
    );
    for (i, choice) in sel.choices.iter().enumerate() {
        let mark = if i == sel.cursor { ">" } else { " " };
        s.push_str(&format!(
            "{} {}
",
            mark, choice.text
        ));
    }
    if sel.choices.is_empty() {
        s.push_str(
            "  No choices
",
        );
    }
    s
}

fn build_syscom_menu_text(syscom: &mut globals::SyscomRuntimeState, project_dir: &Path) -> String {
    let kind = syscom.menu_kind.unwrap_or(syscom_op::CALL_SYSCOM_MENU);
    match kind {
        syscom_op::CALL_SAVE_MENU => {
            build_save_slot_menu_text("SAVE MENU", &syscom.save_slots, false)
        }
        syscom_op::CALL_LOAD_MENU => {
            build_save_slot_menu_text("LOAD MENU", &syscom.save_slots, true)
        }
        syscom_op::QUICK_SAVE => {
            build_save_slot_menu_text("QUICK SAVE MENU", &syscom.quick_save_slots, false)
        }
        syscom_op::QUICK_LOAD => {
            build_save_slot_menu_text("QUICK LOAD MENU", &syscom.quick_save_slots, true)
        }
        _ => {
            ensure_font_list(syscom, project_dir);
            let title = match kind {
                syscom_op::CALL_SYSCOM_MENU => "SYSCOM MENU",
                syscom_op::CALL_CONFIG_MENU => "CONFIG MENU",
                syscom_op::CALL_CONFIG_WINDOW_MODE_MENU => "WINDOW MODE CONFIG",
                syscom_op::CALL_CONFIG_VOLUME_MENU => "VOLUME CONFIG",
                syscom_op::CALL_CONFIG_MESSAGE_SPEED_MENU => "MESSAGE SPEED CONFIG",
                syscom_op::CALL_CONFIG_AUTO_MODE_MENU => "AUTO MODE CONFIG",
                syscom_op::CALL_CONFIG_FILTER_COLOR_MENU => "FILTER COLOR CONFIG",
                syscom_op::CALL_CONFIG_FONT_MENU => "FONT CONFIG",
                syscom_op::CALL_CONFIG_MOVIE_MENU => "MOVIE CONFIG",
                syscom_op::CALL_CONFIG_SYSTEM_MENU => "SYSTEM CONFIG",
                syscom_op::CALL_CONFIG_BGMFADE_MENU => "BGM FADE CONFIG",
                syscom_op::CALL_CONFIG_KOEMODE_MENU => "KOE MODE CONFIG",
                syscom_op::CALL_CONFIG_CHARAKOE_MENU => "CHARA KOE CONFIG",
                syscom_op::CALL_CONFIG_JITAN_MENU => "JITAN CONFIG",
                _ => "MENU",
            };
            let items = syscom_menu_items(syscom, project_dir);
            build_menu_text_from_items(title, syscom, &items)
        }
    }
}

fn build_save_slot_menu_text(title: &str, slots: &[globals::SaveSlotState], load: bool) -> String {
    let action = if load { "load" } else { "save" };
    let mut s = format!(
        "{}\nPress 0-9 to {} slot\nEsc: close  Enter/Click: activate selected item\n",
        title, action
    );
    for i in 0..10 {
        let exist = slots.get(i).map(|v| v.exist).unwrap_or(false);
        let slot_title = slots.get(i).map(|v| v.title.as_str()).unwrap_or("");
        let used = if exist { "USED" } else { "EMPTY" };
        if slot_title.is_empty() {
            s.push_str(&format!("  Slot {}: {}\n", i, used));
        } else {
            s.push_str(&format!("  Slot {}: {} {}\n", i, used, slot_title));
        }
    }
    s
}

#[derive(Clone)]
enum MenuItem {
    Action {
        label: &'static str,
        kind: i32,
    },
    Int {
        label: &'static str,
        key: i32,
        min: i32,
        max: i32,
        step: i32,
    },
    Bool {
        label: &'static str,
        key: i32,
    },
    FontName,
}

const GET_WINDOW_MODE: i32 = syscom_op::GET_WINDOW_MODE;
const GET_WINDOW_MODE_SIZE: i32 = syscom_op::GET_WINDOW_MODE_SIZE;
const GET_ALL_VOLUME: i32 = syscom_op::GET_ALL_VOLUME;
const GET_BGM_VOLUME: i32 = syscom_op::GET_BGM_VOLUME;
const GET_KOE_VOLUME: i32 = syscom_op::GET_KOE_VOLUME;
const GET_PCM_VOLUME: i32 = syscom_op::GET_PCM_VOLUME;
const GET_SE_VOLUME: i32 = syscom_op::GET_SE_VOLUME;
const GET_MOV_VOLUME: i32 = syscom_op::GET_MOV_VOLUME;
const GET_MOV_ONOFF: i32 = syscom_op::GET_MOV_ONOFF;
const GET_ALL_ONOFF: i32 = syscom_op::GET_ALL_ONOFF;
const GET_BGM_ONOFF: i32 = syscom_op::GET_BGM_ONOFF;
const GET_KOE_ONOFF: i32 = syscom_op::GET_KOE_ONOFF;
const GET_PCM_ONOFF: i32 = syscom_op::GET_PCM_ONOFF;
const GET_SE_ONOFF: i32 = syscom_op::GET_SE_ONOFF;
const GET_MESSAGE_SPEED: i32 = syscom_op::GET_MESSAGE_SPEED;
const GET_AUTO_MODE_MOJI_WAIT: i32 = syscom_op::GET_AUTO_MODE_MOJI_WAIT;
const GET_AUTO_MODE_MIN_WAIT: i32 = syscom_op::GET_AUTO_MODE_MIN_WAIT;
const GET_FILTER_COLOR_R: i32 = syscom_op::GET_FILTER_COLOR_R;
const GET_FILTER_COLOR_G: i32 = syscom_op::GET_FILTER_COLOR_G;
const GET_FILTER_COLOR_B: i32 = syscom_op::GET_FILTER_COLOR_B;
const GET_FILTER_COLOR_A: i32 = syscom_op::GET_FILTER_COLOR_A;
const GET_NO_WIPE_ANIME_ONOFF: i32 = syscom_op::GET_NO_WIPE_ANIME_ONOFF;
const GET_SKIP_WIPE_ANIME_ONOFF: i32 = syscom_op::GET_SKIP_WIPE_ANIME_ONOFF;
const GET_WHEEL_NEXT_MESSAGE_ONOFF: i32 = syscom_op::GET_WHEEL_NEXT_MESSAGE_ONOFF;
const GET_KOE_DONT_STOP_ONOFF: i32 = syscom_op::GET_KOE_DONT_STOP_ONOFF;
const GET_SKIP_UNREAD_MESSAGE_ONOFF: i32 = syscom_op::GET_SKIP_UNREAD_MESSAGE_ONOFF;
const GET_PLAY_SILENT_SOUND_ONOFF: i32 = syscom_op::GET_PLAY_SILENT_SOUND_ONOFF;
const GET_FONT_NAME: i32 = syscom_op::GET_FONT_NAME;
const GET_BGMFADE_VOLUME: i32 = syscom_op::GET_BGMFADE_VOLUME;
const GET_BGMFADE_ONOFF: i32 = syscom_op::GET_BGMFADE_ONOFF;
const GET_KOEMODE: i32 = syscom_op::GET_KOEMODE;
const GET_CHARAKOE_ONOFF: i32 = syscom_op::GET_CHARAKOE_ONOFF;
const GET_CHARAKOE_VOLUME: i32 = syscom_op::GET_CHARAKOE_VOLUME;
const GET_JITAN_NORMAL_ONOFF: i32 = syscom_op::GET_JITAN_NORMAL_ONOFF;
const GET_JITAN_AUTO_MODE_ONOFF: i32 = syscom_op::GET_JITAN_AUTO_MODE_ONOFF;
const GET_JITAN_KOE_REPLAY_ONOFF: i32 = syscom_op::GET_JITAN_KOE_REPLAY_ONOFF;
const GET_JITAN_SPEED: i32 = syscom_op::GET_JITAN_SPEED;

fn syscom_menu_items(
    syscom: &mut globals::SyscomRuntimeState,
    project_dir: &Path,
) -> Vec<MenuItem> {
    let kind = syscom.menu_kind.unwrap_or(syscom_op::CALL_SYSCOM_MENU);
    match kind {
        syscom_op::CALL_SYSCOM_MENU => vec![
            MenuItem::Action {
                label: "SAVE",
                kind: syscom_op::CALL_SAVE_MENU,
            },
            MenuItem::Action {
                label: "LOAD",
                kind: syscom_op::CALL_LOAD_MENU,
            },
            MenuItem::Action {
                label: "CONFIG",
                kind: syscom_op::CALL_CONFIG_MENU,
            },
            MenuItem::Action {
                label: "MESSAGE BACK",
                kind: syscom_op::OPEN_MSG_BACK,
            },
            MenuItem::Action {
                label: "RETURN TO SELECT",
                kind: syscom_op::RETURN_TO_SEL,
            },
            MenuItem::Action {
                label: "RETURN TO MENU",
                kind: syscom_op::RETURN_TO_MENU,
            },
            MenuItem::Action {
                label: "END GAME",
                kind: syscom_op::END_GAME,
            },
        ],
        syscom_op::CALL_CONFIG_MENU => vec![
            MenuItem::Action {
                label: "WINDOW MODE",
                kind: syscom_op::CALL_CONFIG_WINDOW_MODE_MENU,
            },
            MenuItem::Action {
                label: "VOLUME",
                kind: syscom_op::CALL_CONFIG_VOLUME_MENU,
            },
            MenuItem::Action {
                label: "BGM FADE",
                kind: syscom_op::CALL_CONFIG_BGMFADE_MENU,
            },
            MenuItem::Action {
                label: "KOE MODE",
                kind: syscom_op::CALL_CONFIG_KOEMODE_MENU,
            },
            MenuItem::Action {
                label: "CHARA KOE",
                kind: syscom_op::CALL_CONFIG_CHARAKOE_MENU,
            },
            MenuItem::Action {
                label: "JITAN",
                kind: syscom_op::CALL_CONFIG_JITAN_MENU,
            },
            MenuItem::Action {
                label: "MESSAGE SPEED",
                kind: syscom_op::CALL_CONFIG_MESSAGE_SPEED_MENU,
            },
            MenuItem::Action {
                label: "AUTO MODE",
                kind: syscom_op::CALL_CONFIG_AUTO_MODE_MENU,
            },
            MenuItem::Action {
                label: "FILTER COLOR",
                kind: syscom_op::CALL_CONFIG_FILTER_COLOR_MENU,
            },
            MenuItem::Action {
                label: "FONT",
                kind: syscom_op::CALL_CONFIG_FONT_MENU,
            },
            MenuItem::Action {
                label: "MOVIE",
                kind: syscom_op::CALL_CONFIG_MOVIE_MENU,
            },
            MenuItem::Action {
                label: "SYSTEM",
                kind: syscom_op::CALL_CONFIG_SYSTEM_MENU,
            },
        ],
        syscom_op::CALL_CONFIG_WINDOW_MODE_MENU => vec![
            MenuItem::Bool {
                label: "WINDOW_MODE",
                key: GET_WINDOW_MODE,
            },
            MenuItem::Int {
                label: "WINDOW_SIZE",
                key: GET_WINDOW_MODE_SIZE,
                min: 0,
                max: 7,
                step: 1,
            },
        ],
        syscom_op::CALL_CONFIG_VOLUME_MENU => vec![
            MenuItem::Int {
                label: "ALL_VOL",
                key: GET_ALL_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Int {
                label: "BGM_VOL",
                key: GET_BGM_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Int {
                label: "KOE_VOL",
                key: GET_KOE_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Int {
                label: "PCM_VOL",
                key: GET_PCM_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Int {
                label: "SE_VOL",
                key: GET_SE_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Bool {
                label: "ALL_ONOFF",
                key: GET_ALL_ONOFF,
            },
            MenuItem::Bool {
                label: "BGM_ONOFF",
                key: GET_BGM_ONOFF,
            },
            MenuItem::Bool {
                label: "KOE_ONOFF",
                key: GET_KOE_ONOFF,
            },
            MenuItem::Bool {
                label: "PCM_ONOFF",
                key: GET_PCM_ONOFF,
            },
            MenuItem::Bool {
                label: "SE_ONOFF",
                key: GET_SE_ONOFF,
            },
        ],
        syscom_op::CALL_CONFIG_BGMFADE_MENU => vec![
            MenuItem::Int {
                label: "BGMFADE_VOL",
                key: GET_BGMFADE_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Bool {
                label: "BGMFADE_ONOFF",
                key: GET_BGMFADE_ONOFF,
            },
        ],
        syscom_op::CALL_CONFIG_KOEMODE_MENU => vec![MenuItem::Int {
            label: "KOEMODE",
            key: GET_KOEMODE,
            min: 0,
            max: 2,
            step: 1,
        }],
        syscom_op::CALL_CONFIG_CHARAKOE_MENU => vec![
            MenuItem::Bool {
                label: "CHARAKOE_ONOFF",
                key: GET_CHARAKOE_ONOFF,
            },
            MenuItem::Int {
                label: "CHARAKOE_VOL",
                key: GET_CHARAKOE_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
        ],
        syscom_op::CALL_CONFIG_JITAN_MENU => vec![
            MenuItem::Bool {
                label: "JITAN_NORMAL",
                key: GET_JITAN_NORMAL_ONOFF,
            },
            MenuItem::Bool {
                label: "JITAN_AUTO",
                key: GET_JITAN_AUTO_MODE_ONOFF,
            },
            MenuItem::Bool {
                label: "JITAN_KOE_REPLAY",
                key: GET_JITAN_KOE_REPLAY_ONOFF,
            },
            MenuItem::Int {
                label: "JITAN_SPEED",
                key: GET_JITAN_SPEED,
                min: 0,
                max: 100,
                step: 5,
            },
        ],
        syscom_op::CALL_CONFIG_MESSAGE_SPEED_MENU => vec![MenuItem::Int {
            label: "MSG_SPEED",
            key: GET_MESSAGE_SPEED,
            min: 0,
            max: 100,
            step: 5,
        }],
        syscom_op::CALL_CONFIG_AUTO_MODE_MENU => vec![
            MenuItem::Int {
                label: "AUTO_MOJI_WAIT",
                key: GET_AUTO_MODE_MOJI_WAIT,
                min: 0,
                max: 300,
                step: 5,
            },
            MenuItem::Int {
                label: "AUTO_MIN_WAIT",
                key: GET_AUTO_MODE_MIN_WAIT,
                min: 0,
                max: 10000,
                step: 100,
            },
        ],
        syscom_op::CALL_CONFIG_FILTER_COLOR_MENU => vec![
            MenuItem::Int {
                label: "FILTER_R",
                key: GET_FILTER_COLOR_R,
                min: 0,
                max: 255,
                step: 5,
            },
            MenuItem::Int {
                label: "FILTER_G",
                key: GET_FILTER_COLOR_G,
                min: 0,
                max: 255,
                step: 5,
            },
            MenuItem::Int {
                label: "FILTER_B",
                key: GET_FILTER_COLOR_B,
                min: 0,
                max: 255,
                step: 5,
            },
            MenuItem::Int {
                label: "FILTER_A",
                key: GET_FILTER_COLOR_A,
                min: 0,
                max: 255,
                step: 5,
            },
        ],
        syscom_op::CALL_CONFIG_FONT_MENU => {
            ensure_font_list(syscom, project_dir);
            vec![MenuItem::FontName]
        }
        syscom_op::CALL_CONFIG_MOVIE_MENU => vec![
            MenuItem::Int {
                label: "MOV_VOL",
                key: GET_MOV_VOLUME,
                min: 0,
                max: 100,
                step: 5,
            },
            MenuItem::Bool {
                label: "MOV_ONOFF",
                key: GET_MOV_ONOFF,
            },
        ],
        syscom_op::CALL_CONFIG_SYSTEM_MENU => vec![
            MenuItem::Bool {
                label: "NO_WIPE",
                key: GET_NO_WIPE_ANIME_ONOFF,
            },
            MenuItem::Bool {
                label: "SKIP_WIPE",
                key: GET_SKIP_WIPE_ANIME_ONOFF,
            },
            MenuItem::Bool {
                label: "WHEEL_NEXT",
                key: GET_WHEEL_NEXT_MESSAGE_ONOFF,
            },
            MenuItem::Bool {
                label: "KOE_DONT_STOP",
                key: GET_KOE_DONT_STOP_ONOFF,
            },
            MenuItem::Bool {
                label: "SKIP_UNREAD",
                key: GET_SKIP_UNREAD_MESSAGE_ONOFF,
            },
            MenuItem::Bool {
                label: "PLAY_SILENT",
                key: GET_PLAY_SILENT_SOUND_ONOFF,
            },
        ],
        _ => Vec::new(),
    }
}

fn build_menu_text_from_items(
    title: &str,
    syscom: &globals::SyscomRuntimeState,
    items: &[MenuItem],
) -> String {
    let mut s = format!(
        "{}\nEsc: close  Enter/Click: activate  Left/Right: change\n",
        title
    );
    for (i, item) in items.iter().enumerate() {
        let mark = if i == syscom.menu_cursor { ">" } else { " " };
        match item {
            MenuItem::Action { label, .. } => {
                s.push_str(&format!("{} {}\n", mark, label));
            }
            MenuItem::Int { label, key, .. } => {
                let v = syscom.config_int.get(key).copied().unwrap_or(0);
                s.push_str(&format!("{} {} = {}\n", mark, label, v));
            }
            MenuItem::Bool { label, key } => {
                let v = syscom.config_int.get(key).copied().unwrap_or(0);
                s.push_str(&format!(
                    "{} {} = {}\n",
                    mark,
                    label,
                    if v == 0 { "OFF" } else { "ON" }
                ));
            }
            MenuItem::FontName => {
                let v = syscom
                    .config_str
                    .get(&GET_FONT_NAME)
                    .map(|s| s.as_str())
                    .unwrap_or("");
                s.push_str(&format!("{} FONT = {}\n", mark, v));
            }
        }
    }
    if items.is_empty() {
        s.push_str("  No in-engine menu items are available for this system page.\n");
    }
    s
}

fn ensure_font_list(syscom: &mut globals::SyscomRuntimeState, project_dir: &Path) {
    if !syscom.font_list.is_empty() {
        return;
    }

    let mut seen = HashSet::new();
    for dir in [project_dir.join("font"), project_dir.join("fonts")] {
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
