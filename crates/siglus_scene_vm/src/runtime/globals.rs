use std::collections::{HashMap, HashSet};

use crate::assets::RgbaImage;
use crate::runtime::gan::GanState;
use crate::runtime::int_event::IntEvent;
use std::time::{Duration, Instant};

use crate::image_manager::ImageId;
use crate::layer::{LayerId, SpriteId};

/// Screen wipe transition state.
///
/// This models the timing and script-visible behavior of the original `Gp_wipe`
/// subsystem. Rendering is handled elsewhere; here we only track parameters and
/// completion.
#[derive(Debug, Clone)]
pub struct WipeState {
    pub mask_file: Option<String>,
    pub mask_image_id: Option<ImageId>,
    pub wipe_type: i32,
    pub wipe_time_ms: i32,
    pub speed_mode: i32,
    pub start_time_ms: i32,
    pub option: Vec<i32>,

    pub begin_order: i32,
    pub end_order: i32,
    pub begin_layer: i32,
    pub end_layer: i32,

    pub wait_flag: bool,
    pub key_wait_mode: i32,
    pub with_low_order: i32,

    pub mask_cache: HashMap<(ImageId, u64, ImageId, u64, u16), ImageId>,

    started_at: Instant,
    end_at: Instant,
}

impl WipeState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mask_file: Option<String>,
        mask_image_id: Option<ImageId>,
        wipe_type: i32,
        wipe_time_ms: i32,
        start_time_ms: i32,
        speed_mode: i32,
        option: Vec<i32>,
        begin_order: i32,
        end_order: i32,
        begin_layer: i32,
        end_layer: i32,
        wait_flag: bool,
        key_wait_mode: i32,
        with_low_order: i32,
    ) -> Self {
        let now = Instant::now();
        let wipe_time_ms_u = wipe_time_ms.max(0) as u64;
        let start_ms_u = start_time_ms.max(0) as u64;
        let start_adv = start_ms_u.min(wipe_time_ms_u);

        let started_at = now
            .checked_sub(Duration::from_millis(start_adv))
            .unwrap_or(now);
        let end_at = started_at + Duration::from_millis(wipe_time_ms_u);

        Self {
            mask_file,
            mask_image_id,
            wipe_type,
            wipe_time_ms,
            speed_mode,
            start_time_ms,
            option,
            begin_order,
            end_order,
            begin_layer,
            end_layer,
            wait_flag,
            key_wait_mode,
            with_low_order,
            mask_cache: HashMap::new(),
            started_at,
            end_at,
        }
    }

    pub fn is_done(&self) -> bool {
        Instant::now() >= self.end_at
    }

    pub fn progress(&self) -> f32 {
        let total = self
            .end_at
            .saturating_duration_since(self.started_at)
            .as_secs_f32();
        if total <= 0.0 {
            return 1.0;
        }
        let elapsed = Instant::now()
            .saturating_duration_since(self.started_at)
            .as_secs_f32();
        (elapsed / total).clamp(0.0, 1.0)
    }

    #[allow(dead_code)]
    pub fn remaining_ms(&self) -> u64 {
        if self.is_done() {
            0
        } else {
            self.end_at.duration_since(Instant::now()).as_millis() as u64
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScriptRuntimeState {
    pub dont_set_save_point: bool,
    pub skip_disable: bool,
    pub ctrl_disable: bool,
    pub not_stop_skip_by_click: bool,
    pub not_skip_msg_by_click: bool,
    pub skip_unread_message: bool,

    pub auto_mode_flag: bool,
    pub auto_mode_moji_wait: i64,
    pub auto_mode_min_wait: i64,
    pub auto_mode_moji_cnt: i64,

    pub mouse_cursor_hide_onoff: i64,
    pub mouse_cursor_hide_time: i64,

    pub msg_speed: i64,
    pub msg_nowait: bool,
    pub async_msg_mode: bool,
    pub async_msg_mode_once: bool,

    pub hide_mwnd_disable: bool,
    pub msg_back_disable: bool,
    pub msg_back_off: bool,
    pub msg_back_disp_off: bool,

    pub cursor_disp_off: bool,
    pub cursor_move_by_key_disable: bool,
    pub key_disable: HashSet<u8>,

    pub mwnd_anime_off_flag: bool,
    pub mwnd_anime_on_flag: bool,
    pub mwnd_disp_off_flag: bool,

    pub koe_dont_stop_on_flag: bool,
    pub koe_dont_stop_off_flag: bool,

    pub shortcut_disable: bool,
    pub quake_stop_flag: bool,
    pub emote_mouth_stop_flag: bool,
    pub bgmfade_flag: bool,
    pub wait_display_vsync_off_flag: bool,
    pub skip_trigger: bool,
    pub ignore_r_flag: bool,
    pub cursor_no: i64,

    pub time_stop_flag: bool,
    pub counter_time_stop_flag: bool,
    pub frame_action_time_stop_flag: bool,
    pub stage_time_stop_flag: bool,

    pub font_name: String,
    pub font_bold: i64,
    pub font_shadow: i64,
}

impl Default for ScriptRuntimeState {
    fn default() -> Self {
        Self {
            dont_set_save_point: false,
            skip_disable: false,
            ctrl_disable: false,
            not_stop_skip_by_click: false,
            not_skip_msg_by_click: false,
            skip_unread_message: false,
            auto_mode_flag: false,
            auto_mode_moji_wait: -1,
            auto_mode_min_wait: -1,
            auto_mode_moji_cnt: 0,
            mouse_cursor_hide_onoff: -1,
            mouse_cursor_hide_time: -1,
            msg_speed: -1,
            msg_nowait: false,
            async_msg_mode: false,
            async_msg_mode_once: false,
            hide_mwnd_disable: false,
            msg_back_disable: false,
            msg_back_off: false,
            msg_back_disp_off: false,
            cursor_disp_off: false,
            cursor_move_by_key_disable: false,
            key_disable: HashSet::new(),
            mwnd_anime_off_flag: false,
            mwnd_anime_on_flag: false,
            mwnd_disp_off_flag: false,
            koe_dont_stop_on_flag: false,
            koe_dont_stop_off_flag: false,
            shortcut_disable: false,
            quake_stop_flag: false,
            emote_mouth_stop_flag: false,
            bgmfade_flag: false,
            wait_display_vsync_off_flag: false,
            skip_trigger: false,
            ignore_r_flag: false,
            cursor_no: 0,
            time_stop_flag: false,
            counter_time_stop_flag: false,
            frame_action_time_stop_flag: false,
            stage_time_stop_flag: false,
            font_name: String::new(),
            font_bold: -1,
            font_shadow: -1,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SystemMessageBoxRecord {
    pub kind: i32,
    pub text: String,
    pub debug_only: bool,
}

#[derive(Debug, Clone)]
pub struct SystemMessageBoxButton {
    pub label: String,
    pub value: i64,
}

#[derive(Debug, Clone)]
pub struct SystemMessageBoxModalState {
    pub kind: i32,
    pub text: String,
    pub debug_only: bool,
    pub buttons: Vec<SystemMessageBoxButton>,
    pub cursor: usize,
}

impl SystemMessageBoxModalState {
    pub fn selected_value(&self) -> i64 {
        self.buttons
            .get(self.cursor.min(self.buttons.len().saturating_sub(1)))
            .map(|b| b.value)
            .unwrap_or(0)
    }

    pub fn cancel_value(&self) -> i64 {
        self.buttons
            .last()
            .map(|b| b.value)
            .unwrap_or_else(|| self.selected_value())
    }
}

#[derive(Debug, Clone)]
pub struct SystemRuntimeState {
    pub active_flag: bool,
    pub debug_flag: bool,
    pub language_code: String,
    pub debug_logs: Vec<String>,
    pub dummy_checks: HashSet<String>,
    pub bench_dialogs: Vec<String>,
    pub messagebox_history: Vec<SystemMessageBoxRecord>,
    pub messagebox_response_queue: Vec<i64>,
    pub messagebox_modal: Option<SystemMessageBoxModalState>,
    pub spec_info: String,
}

impl Default for SystemRuntimeState {
    fn default() -> Self {
        Self {
            active_flag: true,
            debug_flag: false,
            language_code: std::env::var("SIGLUS_LANGUAGE").unwrap_or_else(|_| "JP".to_string()),
            debug_logs: Vec::new(),
            dummy_checks: HashSet::new(),
            bench_dialogs: Vec::new(),
            messagebox_history: Vec::new(),
            messagebox_response_queue: Vec::new(),
            messagebox_modal: None,
            spec_info: "siglus_scene_vm".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ToggleFeatureState {
    pub onoff: bool,
    pub enable: bool,
    pub exist: bool,
}

impl ToggleFeatureState {
    pub fn check_enabled(&self) -> i64 {
        if self.enable && self.exist {
            1
        } else {
            0
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct ValueFeatureState {
    pub value: i64,
    pub enable: bool,
    pub exist: bool,
}

impl ValueFeatureState {
    pub fn check_enabled(&self) -> i64 {
        if self.enable && self.exist {
            1
        } else {
            0
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SaveSlotState {
    pub exist: bool,
    pub year: i64,
    pub month: i64,
    pub day: i64,
    pub weekday: i64,
    pub hour: i64,
    pub minute: i64,
    pub second: i64,
    pub millisecond: i64,
    pub title: String,
    pub message: String,
    pub full_message: String,
    pub comment: String,
    pub append_dir: String,
    pub append_name: String,
    pub values: HashMap<i32, i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscomPendingProcKind {
    ReturnToSel,
    ReturnToMenu,
    BacklogLoad,
}

#[derive(Debug, Clone)]
pub struct SyscomPendingProc {
    pub kind: SyscomPendingProcKind,
    pub warning: bool,
    pub se_play: bool,
    pub fade_out: bool,
    pub leave_msgbk: bool,
    pub save_id: i64,
}

#[derive(Debug, Clone, Default)]
pub struct SyscomRuntimeState {
    pub syscom_menu_disable: bool,
    pub menu_open: bool,
    pub menu_kind: Option<i32>,
    pub menu_result: Option<i64>,
    pub menu_cursor: usize,
    pub font_list: Vec<String>,
    pub mwnd_btn_disable_all: bool,
    pub mwnd_btn_touch_disable: bool,
    pub mwnd_btn_disable: HashMap<i64, bool>,
    pub read_skip: ToggleFeatureState,
    pub auto_skip: ToggleFeatureState,
    pub auto_mode: ToggleFeatureState,
    pub hide_mwnd: ToggleFeatureState,
    pub local_extra_switch: ToggleFeatureState,
    pub local_extra_mode: ValueFeatureState,
    pub msg_back: ToggleFeatureState,
    pub msg_back_open: bool,
    pub return_to_sel: ToggleFeatureState,
    pub return_to_menu: ToggleFeatureState,
    pub end_game: ToggleFeatureState,
    pub save_feature: ToggleFeatureState,
    pub load_feature: ToggleFeatureState,
    pub replay_koe: Option<(i64, i64)>,
    pub current_save_scene_title: String,
    pub current_save_message: String,
    pub total_play_time: i64,
    pub save_slots: Vec<SaveSlotState>,
    pub quick_save_slots: Vec<SaveSlotState>,
    pub inner_save_exists: bool,
    pub end_save_exists: bool,
    pub last_menu_call: i32,
    pub system_extra_int_value: i64,
    pub system_extra_str_value: String,
    pub config_int: HashMap<i32, i64>,
    pub config_str: HashMap<i32, String>,
    pub capture_buffer: Option<RgbaImage>,
    pub capture_size: Option<(u32, u32)>,
    pub return_scene_once: Option<(String, i64)>,
    pub pending_proc: Option<SyscomPendingProc>,
    pub msg_back_load_tid: i64,
}

/// Global mutable state used by various "global element" (form) handlers.
///
/// This crate keeps these structures generic on purpose: many Siglus
/// "global elements" are simple lists, counters, etc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LightType {
    None = -1,
    Directional = 0,
    Point = 1,
    Spot = 2,
    ShadowMapSpot = 3,
}

pub const WORLD_LIGHT_MAX: usize = 128;
pub const OBJ_DIRECTIONAL_LIGHT_MAX: usize = 4;
pub const OBJ_POINT_LIGHT_MAX: usize = 4;
pub const OBJ_SPOT_LIGHT_MAX: usize = 4;

#[derive(Debug, Clone)]
pub struct LightState {
    pub id: i32,
    pub kind: LightType,
    pub diffuse: [f32; 4],
    pub ambient: [f32; 4],
    pub specular: [f32; 4],
    pub pos: [f32; 3],
    pub dir: [f32; 3],
    pub attenuation0: f32,
    pub attenuation1: f32,
    pub attenuation2: f32,
    pub range: f32,
    pub theta_deg: f32,
    pub phi_deg: f32,
    pub falloff: f32,
}

impl LightState {
    pub fn directional(id: i32, dir: [f32; 3]) -> Self {
        Self {
            id,
            kind: LightType::Directional,
            diffuse: [1.0, 1.0, 1.0, 1.0],
            ambient: [0.18, 0.18, 0.18, 1.0],
            specular: [0.0, 0.0, 0.0, 1.0],
            pos: [0.0, 0.0, 0.0],
            dir,
            attenuation0: 1.0,
            attenuation1: 0.0,
            attenuation2: 0.0,
            range: 5000.0,
            theta_deg: 20.0,
            phi_deg: 40.0,
            falloff: 1.0,
        }
    }
}

impl Default for LightState {
    fn default() -> Self {
        Self::directional(0, [0.0, 0.0, -1.0])
    }
}

#[derive(Debug, Clone)]
pub struct FogGlobalState {
    pub enabled: bool,
    pub name: String,
    pub near: f32,
    pub far: f32,
    pub color: [f32; 4],
    pub scroll_x: f32,
    pub texture_image_id: Option<ImageId>,
}

impl Default for FogGlobalState {
    fn default() -> Self {
        Self {
            enabled: false,
            name: String::new(),
            near: 400.0,
            far: 2600.0,
            color: [0.62, 0.62, 0.62, 1.0],
            scroll_x: 0.0,
            texture_image_id: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct GlobalState {
    /// Generic int-list storage keyed by the global form ID.
    pub int_lists: HashMap<u32, Vec<i64>>,
    /// Generic string-list storage keyed by the global form ID.
    pub str_lists: HashMap<u32, Vec<String>>,
    /// Counter-list storage keyed by the global form ID.
    pub counter_lists: HashMap<u32, Vec<Counter>>,
    /// PCM-event lists keyed by the global form ID.
    pub pcm_event_lists: HashMap<u32, Vec<PcmEventState>>,

    /// Generic integer-event roots keyed by the form ID.
    pub int_event_roots: HashMap<u32, IntEvent>,
    /// Generic integer-event lists keyed by the form ID.
    pub int_event_lists: HashMap<u32, Vec<IntEvent>>,

    /// Generic int properties keyed by (form_id -> op_id).
    pub int_props: HashMap<u32, HashMap<i32, i64>>,
    /// Generic string properties keyed by (form_id -> op_id).
    pub str_props: HashMap<u32, HashMap<i32, String>>,

    /// Learned bit-width selectors for int lists (form_id/op -> bit width).
    pub intlist_bit_widths: HashMap<(u32, i32), i32>,
    /// First-seen ordering of bit selectors per int list form.
    pub intlist_bit_order: HashMap<u32, Vec<i32>>,

    /// CGTABLE global disable flag.
    pub cg_table_off: bool,

    /// DATABASE global disable flag.
    pub database_off: bool,

    /// G00BUF slots. Each slot stores an ImageId loaded from the `g00/` directory.
    pub g00buf: Vec<Option<ImageId>>,

    /// RNG state for MATH.RAND (xorshift32). 0 means "uninitialized".
    pub rng_state: u32,

    /// Mask subsystem state keyed by the (guessed or mapped) form id.
    pub mask_lists: HashMap<u32, MaskListState>,
    /// EditBox subsystem state keyed by the (guessed or mapped) form id.
    pub editbox_lists: HashMap<u32, EditBoxListState>,
    /// Currently focused editbox (form_id, index).
    pub focused_editbox: Option<(u32, usize)>,
    /// Display-mode transition counter used by editbox frame visibility.
    pub change_display_mode_proc_cnt: i32,

    /// Global frame-action roots keyed by the owning form id.
    pub frame_actions: HashMap<u32, ObjectFrameActionState>,
    /// Global frame-action channel lists keyed by the owning form id.
    pub frame_action_lists: HashMap<u32, Vec<ObjectFrameActionState>>,
    /// Finish callbacks queued by FRAMEACTION.START/START_REAL/END.
    pub pending_frame_action_finishes: Vec<PendingFrameActionFinish>,
    /// Button decided actions queued from C_elm_object::button_event semantics.
    pub pending_button_actions: Vec<PendingButtonAction>,

    /// Stage UI subsystem state keyed by the stage form ID.
    pub stage_forms: HashMap<u32, StageFormState>,
    /// Currently focused stage group selection (form_id, stage_idx, group_idx).
    pub focused_stage_group: Option<(u32, i64, usize)>,
    /// Currently focused message-window selection (form_id, stage_idx, mwnd_idx).
    pub focused_stage_mwnd: Option<(u32, i64, usize)>,
    /// GLOBAL.SELBTN button-selection runtime state.
    pub selbtn: BtnSelectRuntimeState,
    /// Last object target touched by stage/object dispatch. Compact object-only chains in scene bytecode
    /// use this as the ambient current-object context when they omit the object index.
    pub current_stage_object: Option<(i64, usize)>,
    pub current_object_chain: Option<Vec<i32>>,

    /// Screen subsystem state keyed by the screen form ID.
    pub screen_forms: HashMap<u32, ScreenFormState>,

    /// Message backlog (MSGBK) subsystem state keyed by the form ID.
    pub msgbk_forms: HashMap<u32, MsgBackState>,

    /// Script/global runtime state translated from the original the original implementation command handlers.
    pub script: ScriptRuntimeState,

    /// System helper runtime state.
    pub system: SystemRuntimeState,

    /// System-command runtime state.
    pub syscom: SyscomRuntimeState,
    /// Active GLOBAL.MOV direct movie player.
    pub mov: GlobalMovieState,

    /// Last full-screen capture made by GLOBAL.CAPTURE / CAPTURE_FROM_FILE.
    pub capture_image: Option<RgbaImage>,
    /// Capture buffer used by OBJECT.CREATE_CAPTURE and thumb fallback paths.
    pub capture_for_object_image: Option<RgbaImage>,

    /// Currently selected append directory used by original file resolution helpers.
    pub append_dir: String,

    /// BGM table listened flags keyed by registered name.
    pub bgm_table_listened: HashMap<String, bool>,
    /// BGM table flags indexed by original BGM registration number.
    pub bgm_table_flags: Vec<bool>,
    /// Default flag applied to names not seen yet via BGMTABLE.SET_ALL_FLAG.
    pub bgm_table_all_flag: bool,

    /// Active wipe transition (WIPE / MASK_WIPE).
    pub wipe: Option<WipeState>,

    /// Global light manager keyed by original engine light id.
    pub lights: HashMap<i32, LightState>,
    /// Global fog state.
    pub fog_global: FogGlobalState,

    /// Monotonic frame counter used by render effects.
    pub render_frame: u64,
}

impl Default for GlobalState {
    fn default() -> Self {
        Self {
            int_lists: HashMap::new(),
            str_lists: HashMap::new(),
            counter_lists: HashMap::new(),
            pcm_event_lists: HashMap::new(),
            int_event_roots: HashMap::new(),
            int_event_lists: HashMap::new(),
            int_props: HashMap::new(),
            str_props: HashMap::new(),
            intlist_bit_widths: HashMap::new(),
            intlist_bit_order: HashMap::new(),
            cg_table_off: false,
            database_off: false,
            g00buf: Vec::new(),
            rng_state: 0,
            mask_lists: HashMap::new(),
            editbox_lists: HashMap::new(),
            focused_editbox: None,
            change_display_mode_proc_cnt: 0,

            frame_actions: HashMap::new(),
            frame_action_lists: HashMap::new(),
            pending_frame_action_finishes: Vec::new(),
            pending_button_actions: Vec::new(),
            stage_forms: HashMap::new(),
            focused_stage_group: None,
            focused_stage_mwnd: None,
            selbtn: BtnSelectRuntimeState::default(),
            current_stage_object: None,
            current_object_chain: None,

            screen_forms: HashMap::new(),
            msgbk_forms: HashMap::new(),

            script: ScriptRuntimeState::default(),
            system: SystemRuntimeState::default(),
            syscom: SyscomRuntimeState::default(),
            mov: GlobalMovieState::default(),
            capture_image: None,
            capture_for_object_image: None,
            append_dir: String::new(),

            bgm_table_listened: HashMap::new(),
            bgm_table_flags: Vec::new(),
            bgm_table_all_flag: false,

            wipe: None,
            lights: HashMap::new(),
            fog_global: FogGlobalState::default(),
            render_frame: 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct BtnSelectChoiceState {
    pub text: String,
    pub item_type: i64,
    pub color: i64,
}

#[derive(Debug, Clone, Default)]
pub struct BtnSelectRuntimeState {
    pub template_no: i64,
    pub choices: Vec<BtnSelectChoiceState>,
    pub cursor: usize,
    pub cancel_enable: bool,
    pub capture_flag: bool,
    pub started: bool,
    pub result: i64,
    pub sync_type: i64,
    pub read_flag_scene_no: i64,
    pub read_flag_flag_no: i64,
    pub sel_start_call_scn: String,
    pub sel_start_call_z_no: i64,
}

#[derive(Debug, Clone, Default)]
pub struct PendingFrameActionFinish {
    pub frame_action_chain: Vec<i32>,
    pub object_chain: Option<Vec<i32>>,
    pub scn_name: String,
    pub cmd_name: String,
    pub end_time: i64,
    pub args: Vec<crate::runtime::Value>,
}

#[derive(Debug, Clone)]
pub enum PendingButtonActionKind {
    UserCall { scn_name: String, cmd_name: String, z_no: i64 },
    Syscom { sys_type: i64, sys_type_opt: i64, mode: i64 },
}

impl Default for PendingButtonActionKind {
    fn default() -> Self {
        Self::UserCall { scn_name: String::new(), cmd_name: String::new(), z_no: -1 }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PendingButtonAction {
    pub kind: PendingButtonActionKind,
}

/// Runtime state for the GLOBAL.MOV player.
///
/// Original Siglus MOV is not a stage OBJECT; it is a full-screen/direct movie
/// player. The WGPU port renders it through a dedicated LayerManager sprite so
/// MOV.PLAY/WAIT/STOP still produce visible frames when stage objects are hidden.
#[derive(Debug, Clone)]
pub struct GlobalMovieState {
    pub file_name: Option<String>,
    pub playing: bool,
    pub key_skip_flag: bool,
    pub timer_ms: u64,
    pub total_ms: Option<u64>,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub layer_id: Option<LayerId>,
    pub sprite_id: Option<SpriteId>,
    pub image_id: Option<ImageId>,
    pub last_frame_idx: Option<usize>,
    pub audio_id: Option<u64>,
}

impl Default for GlobalMovieState {
    fn default() -> Self {
        Self {
            file_name: None,
            playing: false,
            key_skip_flag: false,
            timer_ms: 0,
            total_ms: None,
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            layer_id: None,
            sprite_id: None,
            image_id: None,
            last_frame_idx: None,
            audio_id: None,
        }
    }
}

impl GlobalMovieState {
    pub fn start(
        &mut self,
        file_name: String,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        total_ms: Option<u64>,
        key_skip_flag: bool,
    ) {
        self.file_name = Some(file_name);
        self.playing = true;
        self.key_skip_flag = key_skip_flag;
        self.timer_ms = 0;
        self.total_ms = total_ms;
        self.x = x;
        self.y = y;
        self.width = width;
        self.height = height;
        self.last_frame_idx = None;
        self.audio_id = None;
    }

    pub fn stop(&mut self) {
        self.file_name = None;
        self.playing = false;
        self.key_skip_flag = false;
        self.timer_ms = 0;
        self.total_ms = None;
        self.last_frame_idx = None;
        self.audio_id = None;
    }

    pub fn tick(&mut self, past_real_time: i32) {
        if !self.playing {
            return;
        }
        let add = past_real_time.max(0) as u64;
        if add == 0 {
            return;
        }
        self.timer_ms = self.timer_ms.saturating_add(add);
        if let Some(total) = self.total_ms {
            if total > 0 && self.timer_ms >= total {
                self.timer_ms = total;
                self.playing = false;
            }
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Counter {
    cur_time: i64,
    is_running: bool,
    real_flag: bool,
    frame_mode: bool,
    frame_loop_flag: bool,
    frame_start_value: i64,
    frame_end_value: i64,
    frame_time: i64,
}

impl Default for Counter {
    fn default() -> Self {
        Self {
            cur_time: 0,
            is_running: false,
            real_flag: false,
            frame_mode: false,
            frame_loop_flag: false,
            frame_start_value: 0,
            frame_end_value: 0,
            frame_time: 0,
        }
    }
}

impl Counter {
    fn limit(min: i64, value: i64, max: i64) -> i64 {
        if value < min {
            min
        } else if value > max {
            max
        } else {
            value
        }
    }

    pub fn reinit(&mut self) {
        *self = Self::default();
    }

    pub fn reset(&mut self) {
        self.is_running = false;
        self.real_flag = false;
        self.frame_mode = false;
        self.cur_time = 0;
    }

    pub fn set_count(&mut self, value: i64) {
        if self.frame_mode {
            if self.frame_end_value == self.frame_start_value {
                self.cur_time = 0;
                return;
            }

            let denom = self.frame_end_value - self.frame_start_value;
            let frame_time = self.frame_time;
            let mut cur_time = (value - self.frame_start_value) * frame_time / denom;
            if self.frame_loop_flag {
                cur_time = Self::limit(0, cur_time, frame_time - 1);
            } else {
                cur_time = Self::limit(0, cur_time, frame_time);
            }
            self.cur_time = cur_time;
        } else {
            self.cur_time = value;
        }
    }

    pub fn start(&mut self) {
        self.is_running = true;
        self.real_flag = false;
        self.frame_mode = false;
        self.cur_time = 0;
    }

    pub fn start_real(&mut self) {
        self.is_running = true;
        self.real_flag = true;
        self.frame_mode = false;
        self.cur_time = 0;
    }

    pub fn start_frame(&mut self, from: i64, to: i64, frame_time: i64) {
        self.is_running = true;
        self.real_flag = false;
        self.frame_mode = true;
        self.frame_loop_flag = false;
        self.frame_start_value = from;
        self.frame_end_value = to;
        self.frame_time = frame_time;
        self.cur_time = 0;
    }

    pub fn start_frame_real(&mut self, from: i64, to: i64, frame_time: i64) {
        self.is_running = true;
        self.real_flag = true;
        self.frame_mode = true;
        self.frame_loop_flag = false;
        self.frame_start_value = from;
        self.frame_end_value = to;
        self.frame_time = frame_time;
        self.cur_time = 0;
    }

    pub fn start_frame_loop(&mut self, from: i64, to: i64, frame_time: i64) {
        self.is_running = true;
        self.real_flag = false;
        self.frame_mode = true;
        self.frame_loop_flag = true;
        self.frame_start_value = from;
        self.frame_end_value = to;
        self.frame_time = frame_time;
        self.cur_time = 0;
    }

    pub fn start_frame_loop_real(&mut self, from: i64, to: i64, frame_time: i64) {
        self.is_running = true;
        self.real_flag = true;
        self.frame_mode = true;
        self.frame_loop_flag = true;
        self.frame_start_value = from;
        self.frame_end_value = to;
        self.frame_time = frame_time;
        self.cur_time = 0;
    }

    pub fn stop(&mut self) {
        self.is_running = false;
    }

    pub fn resume(&mut self) {
        self.is_running = true;
    }

    pub fn update_time(&mut self, past_game_time: i32, past_real_time: i32) {
        if self.is_running {
            let add = if self.real_flag { past_real_time } else { past_game_time };
            self.cur_time = self.cur_time.saturating_add(add as i64);
        }

        if self.frame_mode && !self.frame_loop_flag && self.cur_time >= self.frame_time {
            self.is_running = false;
        }
    }

    pub fn get_count(&self) -> i64 {
        if self.frame_mode {
            if self.frame_time <= 0 {
                return self.frame_end_value;
            }
            if self.frame_start_value == self.frame_end_value {
                return self.frame_end_value;
            }

            let span = self.frame_end_value - self.frame_start_value;
            let mut value = span * self.cur_time / self.frame_time;
            if self.frame_loop_flag {
                value %= span;
                value += self.frame_start_value;
            } else {
                value += self.frame_start_value;
                if self.frame_start_value > self.frame_end_value {
                    value = Self::limit(self.frame_end_value, value, self.frame_start_value);
                } else {
                    value = Self::limit(self.frame_start_value, value, self.frame_end_value);
                }
            }
            value
        } else {
            self.cur_time
        }
    }

    pub fn get_count_with_frame(&self, _current_frame: i64) -> i64 {
        self.get_count()
    }

    pub fn is_running(&self) -> bool {
        self.is_running
    }
}

#[derive(Debug, Clone, Default)]
pub struct PcmEventLine {
    pub file_name: String,
    pub probability: i32,
    pub min_time: i32,
    pub max_time: i32,
}

#[derive(Debug, Clone, Default)]
pub struct PcmEventState {
    pub active: bool,
    pub looped: bool,
    pub random: bool,
    pub volume_type: i32,
    pub chara_no: i32,
    pub bgm_fade_target_flag: bool,
    pub bgm_fade2_target_flag: bool,
    pub bgm_fade2_source_flag: bool,
    pub real_flag: bool,
    pub time_type: bool,
    pub lines: Vec<PcmEventLine>,
}

impl PcmEventState {
    pub fn reinit(&mut self) {
        *self = Self::default();
    }
}

/// Mask state.
#[derive(Debug, Clone)]
pub struct MaskState {
    pub name: Option<String>,
    pub x_event: IntEvent,
    pub y_event: IntEvent,
    pub extra_int: HashMap<i32, i32>,
    pub script_events: HashMap<i32, IntEvent>,
}

impl MaskState {
    pub fn new() -> Self {
        Self {
            name: None,
            x_event: IntEvent::new(0),
            y_event: IntEvent::new(0),
            extra_int: HashMap::new(),
            script_events: HashMap::new(),
        }
    }

    pub fn reinit(&mut self) {
        self.name = None;
        self.x_event.reinit();
        self.y_event.reinit();
        self.extra_int.clear();
        self.script_events.clear();
    }
}

#[derive(Debug, Clone)]
pub struct MaskListState {
    pub masks: Vec<MaskState>,
}

#[derive(Debug, Clone)]
pub struct MaskedSpriteCache {
    pub base_image_id: ImageId,
    pub base_version: u64,
    pub mask_image_id: ImageId,
    pub mask_version: u64,
    pub mask_x: i32,
    pub mask_y: i32,
    pub masked_image_id: ImageId,
}

pub const EDITBOX_ACTION_NOT_DECIDED: i32 = 0;
pub const EDITBOX_ACTION_DECIDED: i32 = 1;
pub const EDITBOX_ACTION_CANCELED: i32 = -1;

#[derive(Debug, Clone)]
pub struct EditBoxState {
    pub created: bool,
    pub visible: bool,
    pub text: String,
    pub cursor_pos: usize,
    pub action_flag: i32,
    pub moji_size: i32,
    pub rect_x: i32,
    pub rect_y: i32,
    pub rect_w: i32,
    pub rect_h: i32,
    pub design_screen_w: i32,
    pub design_screen_h: i32,
    pub window_x: i32,
    pub window_y: i32,
    pub window_w: i32,
    pub window_h: i32,
    pub window_moji_size: i32,
}

impl Default for EditBoxState {
    fn default() -> Self {
        Self {
            created: false,
            visible: false,
            text: String::new(),
            cursor_pos: 0,
            action_flag: EDITBOX_ACTION_NOT_DECIDED,
            moji_size: 0,
            rect_x: 0,
            rect_y: 0,
            rect_w: 0,
            rect_h: 0,
            design_screen_w: 0,
            design_screen_h: 0,
            window_x: 0,
            window_y: 0,
            window_w: 0,
            window_h: 0,
            window_moji_size: 0,
        }
    }
}

impl EditBoxState {
    pub fn create_like(
        &mut self,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        moji_size: i32,
        design_screen_w: i32,
        design_screen_h: i32,
    ) {
        self.created = true;
        self.visible = false;
        self.text.clear();
        self.cursor_pos = 0;
        self.action_flag = EDITBOX_ACTION_NOT_DECIDED;
        self.rect_x = x;
        self.rect_y = y;
        self.rect_w = w;
        self.rect_h = h;
        self.moji_size = moji_size;
        self.design_screen_w = design_screen_w.max(1);
        self.design_screen_h = design_screen_h.max(1);
        self.window_x = 0;
        self.window_y = 0;
        self.window_w = 0;
        self.window_h = 0;
        self.window_moji_size = 0;
    }

    pub fn destroy_like(&mut self) {
        self.created = false;
        self.visible = false;
        self.text.clear();
        self.cursor_pos = 0;
        self.action_flag = EDITBOX_ACTION_NOT_DECIDED;
        self.rect_x = 0;
        self.rect_y = 0;
        self.rect_w = 0;
        self.rect_h = 0;
        self.moji_size = 0;
        self.design_screen_w = 0;
        self.design_screen_h = 0;
        self.window_x = 0;
        self.window_y = 0;
        self.window_w = 0;
        self.window_h = 0;
        self.window_moji_size = 0;
    }

    pub fn set_text_like(&mut self, text: String) {
        self.text = text;
        self.cursor_pos = self.text.len();
    }

    pub fn insert_text_at_cursor(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        let pos = self.cursor_pos.min(self.text.len());
        self.text.insert_str(pos, text);
        self.cursor_pos = pos.saturating_add(text.len()).min(self.text.len());
    }

    pub fn backspace_like(&mut self) {
        if self.cursor_pos == 0 || self.text.is_empty() {
            return;
        }
        let mut prev = 0usize;
        for (i, _) in self.text.char_indices() {
            if i >= self.cursor_pos {
                break;
            }
            prev = i;
        }
        self.text.drain(prev..self.cursor_pos.min(self.text.len()));
        self.cursor_pos = prev;
    }

    pub fn update_rect(&mut self, screen_w: i32, screen_h: i32) {
        let base_w = self.design_screen_w.max(1);
        let base_h = self.design_screen_h.max(1);
        let sw = screen_w.max(1);
        let sh = screen_h.max(1);
        self.window_x = self.rect_x.saturating_mul(sw) / base_w;
        self.window_y = self.rect_y.saturating_mul(sh) / base_h;
        self.window_w = self.rect_w.saturating_mul(sw) / base_w;
        self.window_h = self.rect_h.saturating_mul(sh) / base_h;
        self.window_moji_size = self.moji_size.saturating_mul(sh) / base_h;
    }

    pub fn frame(&mut self, display_mode_change_proc_cnt: i32) {
        self.visible = self.created && display_mode_change_proc_cnt == 0;
    }

    pub fn clear_input(&mut self) {
        self.action_flag = EDITBOX_ACTION_NOT_DECIDED;
    }

    pub fn is_decided(&self) -> bool {
        self.action_flag == EDITBOX_ACTION_DECIDED
    }

    pub fn is_canceled(&self) -> bool {
        self.action_flag == EDITBOX_ACTION_CANCELED
    }

    pub fn contains_point(&self, x: i32, y: i32) -> bool {
        self.created
            && self.visible
            && self.window_w > 0
            && self.window_h > 0
            && x >= self.window_x
            && y >= self.window_y
            && x < self.window_x.saturating_add(self.window_w)
            && y < self.window_y.saturating_add(self.window_h)
    }
}

#[derive(Debug, Clone)]
pub struct EditBoxListState {
    pub boxes: Vec<EditBoxState>,
}

impl EditBoxListState {
    pub fn new(cnt: usize) -> Self {
        Self {
            boxes: vec![EditBoxState::default(); cnt],
        }
    }

    pub fn ensure_size(&mut self, cnt: usize) {
        if self.boxes.len() < cnt {
            self.boxes
                .extend((0..(cnt - self.boxes.len())).map(|_| EditBoxState::default()));
        } else if self.boxes.len() > cnt {
            self.boxes.truncate(cnt);
        }
    }
}

// -----------------------------------------------------------------------------
// Stage/MWND/Group state
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldListOpKind {
    GetSize,
    Create,
    Destroy,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorldOpKind {
    Init,
    GetNo,
    CameraEyeX,
    CameraEyeY,
    CameraEyeZ,
    CameraPintX,
    CameraPintY,
    CameraPintZ,
    CameraUpX,
    CameraUpY,
    CameraUpZ,
    CameraEyeXEve,
    CameraEyeYEve,
    CameraEyeZEve,
    CameraPintXEve,
    CameraPintYEve,
    CameraPintZEve,
    CameraUpXEve,
    CameraUpYEve,
    CameraUpZEve,
    CameraViewAngle,
    SetCameraEye,
    CalcCameraEye,
    SetCameraPint,
    CalcCameraPint,
    SetCameraUp,
    Mono,
    SetCameraEveXzRotate,
    Order,
    Layer,
    WipeCopy,
    WipeErase,
    Unknown,
}

#[derive(Debug, Clone, Copy)]
pub struct WorldRotateEvent {
    pub loop_type: i32,
    pub cur_time: i32,
    pub end_time: i32,
    pub delay_time: i32,
    pub speed_type: i32,
    pub start_x: i32,
    pub start_z: i32,
    pub end_x: i32,
    pub end_z: i32,
}

impl WorldRotateEvent {
    pub fn new() -> Self {
        Self {
            loop_type: -1,
            cur_time: 0,
            end_time: 0,
            delay_time: 0,
            speed_type: 0,
            start_x: 0,
            start_z: 0,
            end_x: 0,
            end_z: 0,
        }
    }

    pub fn is_active(&self) -> bool {
        self.loop_type != -1
    }
}

#[derive(Debug, Clone)]
pub struct WorldState {
    pub world_no: i32,
    pub mode: i32,
    pub camera_eye_x: IntEvent,
    pub camera_eye_y: IntEvent,
    pub camera_eye_z: IntEvent,
    pub camera_pint_x: IntEvent,
    pub camera_pint_y: IntEvent,
    pub camera_pint_z: IntEvent,
    pub camera_up_x: IntEvent,
    pub camera_up_y: IntEvent,
    pub camera_up_z: IntEvent,
    pub camera_view_angle: i32,
    pub mono: i32,
    pub order: i32,
    pub layer: i32,
    pub wipe_copy: i32,
    pub wipe_erase: i32,
    pub camera_eye_xz_eve: WorldRotateEvent,
    pub script_events: HashMap<i32, IntEvent>,
    pub extra_int: HashMap<i32, i64>,
    pub extra_str: HashMap<i32, String>,
}

impl WorldState {
    pub fn new(world_no: i32) -> Self {
        let mut out = Self {
            world_no,
            mode: 1,
            camera_eye_x: IntEvent::new(0),
            camera_eye_y: IntEvent::new(0),
            camera_eye_z: IntEvent::new(-1000),
            camera_pint_x: IntEvent::new(0),
            camera_pint_y: IntEvent::new(0),
            camera_pint_z: IntEvent::new(0),
            camera_up_x: IntEvent::new(0),
            camera_up_y: IntEvent::new(1),
            camera_up_z: IntEvent::new(0),
            camera_view_angle: 450,
            mono: 0,
            order: 0,
            layer: 0,
            wipe_copy: 0,
            wipe_erase: 0,
            camera_eye_xz_eve: WorldRotateEvent::new(),
            script_events: HashMap::new(),
            extra_int: HashMap::new(),
            extra_str: HashMap::new(),
        };
        out.reinit();
        out
    }

    pub fn reinit(&mut self) {
        self.mode = 1;
        self.camera_eye_x = IntEvent::new(0);
        self.camera_eye_y = IntEvent::new(0);
        self.camera_eye_z = IntEvent::new(-1000);
        self.camera_pint_x = IntEvent::new(0);
        self.camera_pint_y = IntEvent::new(0);
        self.camera_pint_z = IntEvent::new(0);
        self.camera_up_x = IntEvent::new(0);
        self.camera_up_y = IntEvent::new(1);
        self.camera_up_z = IntEvent::new(0);
        self.camera_view_angle = 450;
        self.mono = 0;
        self.order = 0;
        self.layer = 0;
        self.wipe_copy = 0;
        self.wipe_erase = 0;
        self.camera_eye_xz_eve = WorldRotateEvent::new();
    }

    pub fn update_time(&mut self, past_game_time: i32, past_real_time: i32) {
        self.camera_eye_x
            .update_time(past_game_time, past_real_time);
        self.camera_eye_y
            .update_time(past_game_time, past_real_time);
        self.camera_eye_z
            .update_time(past_game_time, past_real_time);
        self.camera_pint_x
            .update_time(past_game_time, past_real_time);
        self.camera_pint_y
            .update_time(past_game_time, past_real_time);
        self.camera_pint_z
            .update_time(past_game_time, past_real_time);
        self.camera_up_x.update_time(past_game_time, past_real_time);
        self.camera_up_y.update_time(past_game_time, past_real_time);
        self.camera_up_z.update_time(past_game_time, past_real_time);
        if self.camera_eye_xz_eve.is_active() {
            self.camera_eye_xz_eve.cur_time = self
                .camera_eye_xz_eve
                .cur_time
                .saturating_add(past_game_time);
        }
    }

    pub fn frame(&mut self) {
        self.camera_eye_x.frame();
        self.camera_eye_y.frame();
        self.camera_eye_z.frame();
        self.camera_pint_x.frame();
        self.camera_pint_y.frame();
        self.camera_pint_z.frame();
        self.camera_up_x.frame();
        self.camera_up_y.frame();
        self.camera_up_z.frame();

        if self.camera_eye_xz_eve.is_active() {
            self.frame_xz_rotate();
        }
    }

    fn frame_xz_rotate(&mut self) {
        let mut cur_time = self.camera_eye_xz_eve.cur_time - self.camera_eye_xz_eve.delay_time;
        let end_time = self.camera_eye_xz_eve.end_time;

        if self.camera_eye_xz_eve.loop_type == 0 && cur_time - end_time >= 0 {
            self.camera_eye_xz_eve.loop_type = -1;
            return;
        }

        if cur_time <= 0 {
            self.camera_eye_x.cur_value = self.camera_eye_x.start_value;
            self.camera_eye_z.cur_value = self.camera_eye_z.start_value;
            return;
        }

        if end_time <= 0 {
            return;
        }

        if self.camera_eye_xz_eve.loop_type == 1 {
            cur_time %= end_time;
        }
        if self.camera_eye_xz_eve.loop_type == 2 {
            cur_time %= end_time * 2;
            if cur_time - end_time > 0 {
                cur_time = end_time - (cur_time - end_time);
            }
        }

        match self.camera_eye_xz_eve.speed_type {
            1 => {
                cur_time = (cur_time as f64 * cur_time as f64 / end_time as f64) as i32;
            }
            2 => {
                let ct = (cur_time - end_time) as f64;
                let et = end_time as f64;
                cur_time = (-ct * ct / et + et) as i32;
            }
            _ => {}
        }

        let px = self.camera_pint_x.get_total_value() as f64;
        let pz = self.camera_pint_z.get_total_value() as f64;
        let sx = self.camera_eye_x.start_value as f64;
        let sz = self.camera_eye_z.start_value as f64;
        let ex = self.camera_eye_x.end_value as f64;
        let ez = self.camera_eye_z.end_value as f64;

        let sdx = sx - px;
        let sdz = sz - pz;
        let edx = ex - px;
        let edz = ez - pz;

        let s_len = (sdx * sdx + sdz * sdz).sqrt();
        let e_len = (edx * edx + edz * edz).sqrt();
        let t_len = linear(cur_time, s_len, end_time, e_len);

        let mut s_theta = sdz.atan2(sdx);
        let mut e_theta = edz.atan2(edx);
        if (s_theta - e_theta).abs() > std::f64::consts::PI {
            if e_theta < 0.0 {
                e_theta += std::f64::consts::PI * 2.0;
            } else {
                e_theta -= std::f64::consts::PI * 2.0;
            }
        }
        let t_theta = linear(cur_time, s_theta, end_time, e_theta);

        let tmp_x = t_len * t_theta.cos() + px;
        let tmp_z = t_len * t_theta.sin() + pz;

        self.camera_eye_x.cur_value = tmp_x as i32;
        self.camera_eye_z.cur_value = tmp_z as i32;
    }
}

fn linear(cur: i32, start_value: f64, end_time: i32, end_value: f64) -> f64 {
    if end_time <= 0 {
        return end_value;
    }
    let t = cur as f64 / end_time as f64;
    start_value + (end_value - start_value) * t
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectListOpKind {
    GetSize,
    Resize,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectOpKind {
    Init,
    Free,
    InitParam,
    CreatePct,
    CreateRect,
    CreateString,
    /// SET_POS (2 or 3 ints)
    SetPos,
    /// SET_CENTER (2 or 3 ints)
    SetCenter,
    /// SET_SCALE (2 or 3 ints)
    SetScale,
    /// SET_ROTATE (2 or 3 ints)
    SetRotate,
    /// SET_CLIP (4 ints)
    SetClip,
    /// SET_SRC_CLIP (4 ints)
    SetSrcClip,
    /// CLEAR_BUTTON
    ClearButton,
    /// SET_BUTTON (1..4 ints, al_id=0..2)
    SetButton,
    /// SET_BUTTON_GROUP (int or element)
    SetButtonGroup,
    /// Int-list sub-element (X_REP/Y_REP/Z_REP/TR_REP/F, etc.).
    RepIntList,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectBackend {
    None,
    /// Uses the engine's GfxRuntime object pipeline.
    Gfx,
    /// Rectangle backed by a standalone LayerManager sprite.
    Rect {
        layer_id: LayerId,
        sprite_id: SpriteId,
        width: u32,
        height: u32,
    },
    /// STRING object backend: a single sprite with rendered text.
    String {
        layer_id: LayerId,
        sprite_id: SpriteId,
        width: u32,
        height: u32,
    },
    /// NUMBER object backend: a fixed sprite list (16) with per-digit sprites.
    Number {
        layer_id: LayerId,
        sprite_ids: Vec<SpriteId>,
    },
    /// WEATHER object backend: sprite list owned by the weather object runtime.
    Weather {
        layer_id: LayerId,
        sprite_ids: Vec<SpriteId>,
    },
    /// MOVIE object backend: a single sprite updated with video frames.
    Movie {
        layer_id: LayerId,
        sprite_id: SpriteId,
        image_id: Option<ImageId>,
        width: u32,
        height: u32,
    },
}

impl Default for ObjectBackend {
    fn default() -> Self {
        Self::None
    }
}

pub const OBJECT_NESTED_SLOT_KEY: i32 = i32::MIN + 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectEventTarget {
    X,
    Y,
    XRep,
    YRep,
    ZRep,
    Alpha,
    Patno,
    Order,
    Layer,
    Z,
    CenterX,
    CenterY,
    CenterZ,
    CenterRepX,
    CenterRepY,
    CenterRepZ,
    ScaleX,
    ScaleY,
    ScaleZ,
    RotateX,
    RotateY,
    RotateZ,
    TrRep,
    ClipLeft,
    ClipTop,
    ClipRight,
    ClipBottom,
    SrcClipLeft,
    SrcClipTop,
    SrcClipRight,
    SrcClipBottom,
    Tr,
    Mono,
    Reverse,
    Bright,
    Dark,
    ColorRate,
    ColorAddR,
    ColorAddG,
    ColorAddB,
    ColorR,
    ColorG,
    ColorB,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ObjectButtonState {
    pub enabled: bool,
    pub button_no: i64,
    pub group_no: i64,
    /// Optional override derived from SET_BUTTON_GROUP(element).
    pub group_idx_override: Option<usize>,
    pub action_no: i64,
    pub se_no: i64,
    pub sys_type: i64,
    pub sys_type_opt: i64,
    pub mode: i64,
    pub push_keep: bool,
    pub alpha_test: bool,
    /// Button state constants: 0=normal, 1=hit, 2=push, 3=select, 4=disable.
    pub state: i64,
    pub hit: bool,
    pub pushed: bool,

    // Decided action (set_button_decided_action)
    pub decided_action_scn_name: String,
    pub decided_action_cmd_name: String,
    pub decided_action_z_no: i64,
    /// Previous-frame hit flag used to model *_this_frame button transitions.
    pub last_hit: bool,
    /// Previous-frame pushed flag used to model *_this_frame button transitions.
    pub last_pushed: bool,
}

impl Default for ObjectButtonState {
    fn default() -> Self {
        Self {
            enabled: false,
            button_no: 0,
            group_no: -1,
            group_idx_override: None,
            action_no: -1,
            se_no: -1,
            sys_type: 0,
            sys_type_opt: 0,
            mode: 0,
            push_keep: false,
            alpha_test: false,
            state: 0,
            hit: false,
            pushed: false,
            decided_action_scn_name: String::new(),
            decided_action_cmd_name: String::new(),
            decided_action_z_no: -1,
            last_hit: false,
            last_pushed: false,
        }
    }
}

impl ObjectButtonState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn group_idx(&self) -> Option<usize> {
        if !self.enabled {
            return None;
        }
        if let Some(i) = self.group_idx_override {
            return Some(i);
        }
        if self.group_no < 0 {
            return None;
        }
        Some(self.group_no as usize)
    }

    pub fn is_disabled(&self) -> bool {
        self.enabled && self.state == 4
    }
}

#[derive(Debug, Default, Clone)]
pub struct ObjectStringParam {
    pub moji_size: i64,
    pub moji_space_x: i64,
    pub moji_space_y: i64,
    pub moji_cnt: i64,
    pub moji_color: i64,
    pub shadow_color: i64,
    pub fuchi_color: i64,
    /// -1: auto/default in original engine
    pub shadow_mode: i64,
}

#[derive(Debug, Default, Clone)]
pub struct ObjectNumberParam {
    pub keta_max: i64,
    pub disp_zero: i64,
    pub disp_sign: i64,
    pub tumeru_sign: i64,
    pub space_mod: i64,
    pub space: i64,
}

#[derive(Debug, Default, Clone)]
pub struct ObjectWeatherParam {
    /// TNM_OBJECT_WEATHER_TYPE_* (0=none, 1=type A, 2=type B)
    pub weather_type: i64,
    pub cnt: i64,
    pub pat_mode: i64,
    pub pat_no_00: i64,
    pub pat_no_01: i64,
    pub pat_time: i64,
    pub move_time_x: i64,
    pub move_time_y: i64,
    pub sin_time_x: i64,
    pub sin_power_x: i64,
    pub sin_time_y: i64,
    pub sin_power_y: i64,
    pub center_x: i64,
    pub center_y: i64,
    pub appear_range: i64,
    pub move_time: i64,
    pub center_rotate: i64,
    pub zoom_min: i64,
    pub zoom_max: i64,
    pub scale_x: i64,
    pub scale_y: i64,
    pub active_time: i64,
    pub real_time_flag: bool,
}

#[derive(Debug, Default, Clone)]
pub struct ObjectWeatherWorkSub {
    pub state: i64,
    pub state_cur_time: i64,
    pub state_time_len: i64,
    pub move_start_pos_x: i64,
    pub move_start_pos_y: i64,
    pub move_start_distance: i64,
    pub move_start_degree: i64,
    pub move_time_x: i64,
    pub move_time_y: i64,
    pub move_cur_time: i64,
    pub sin_time_x: i64,
    pub sin_time_y: i64,
    pub sin_power_x: i64,
    pub sin_power_y: i64,
    pub sin_cur_time: i64,
    pub center_rotate: i64,
    pub zoom_min: i64,
    pub zoom_max: i64,
    pub scale_x: i64,
    pub scale_y: i64,
    pub active_time_len: i64,
    pub real_time_flag: bool,
    pub restruct_flag: bool,
}

#[derive(Debug, Clone)]
pub struct ObjectWeatherWorkState {
    pub cnt_max: usize,
    pub sub: Vec<ObjectWeatherWorkSub>,
    rand_seed: u32,
}

impl Default for ObjectWeatherWorkState {
    fn default() -> Self {
        Self {
            cnt_max: 0,
            sub: Vec::new(),
            rand_seed: 0x1234_abcd,
        }
    }
}

impl ObjectWeatherWorkState {
    fn next_rand(&mut self) -> i64 {
        self.rand_seed = self
            .rand_seed
            .wrapping_mul(1103515245)
            .wrapping_add(12345);
        ((self.rand_seed >> 16) & 0x7fff) as i64
    }

    pub fn rand_mod(&mut self, modulo: i64) -> i64 {
        if modulo <= 0 {
            0
        } else {
            self.next_rand() % modulo
        }
    }
}


#[derive(Debug, Clone)]
pub struct ObjectMovieState {
    pub loop_flag: bool,
    pub auto_free_flag: bool,
    pub real_time_flag: bool,
    pub pause_flag: bool,

    /// Current playback timer in milliseconds (the original implementation: m_omv_timer).
    pub timer_ms: u64,
    /// Total movie time in milliseconds if known.
    pub total_ms: Option<u64>,

    pub playing: bool,
    pub last_tick: Option<std::time::Instant>,
    pub last_frame_idx: Option<usize>,
    pub audio_id: Option<u64>,
    pub just_finished: bool,
    pub just_looped: bool,
    pub seeked: bool,
}

impl Default for ObjectMovieState {
    fn default() -> Self {
        Self {
            loop_flag: false,
            auto_free_flag: true,
            real_time_flag: true,
            pause_flag: false,
            timer_ms: 0,
            total_ms: None,
            playing: false,
            last_tick: None,
            last_frame_idx: None,
            audio_id: None,
            just_finished: false,
            just_looped: false,
            seeked: false,
        }
    }
}

impl ObjectMovieState {
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    pub fn start(
        &mut self,
        total_ms: Option<u64>,
        loop_flag: bool,
        auto_free_flag: bool,
        real_time_flag: bool,
        ready_only: bool,
    ) {
        self.total_ms = total_ms;
        self.loop_flag = loop_flag;
        self.auto_free_flag = auto_free_flag;
        self.real_time_flag = real_time_flag;
        self.pause_flag = ready_only;
        self.timer_ms = 0;
        self.playing = !ready_only;
        self.last_tick = Some(std::time::Instant::now());
        self.last_frame_idx = None;
        self.audio_id = None;
        self.just_finished = false;
        self.just_looped = false;
        self.seeked = false;
    }



    pub fn tick(&mut self, past_game_time: i32, past_real_time: i32) {
        self.just_finished = false;
        self.just_looped = false;
        if !self.playing || self.pause_flag {
            return;
        }
        let add = if self.real_time_flag {
            past_real_time.max(0) as u64
        } else {
            past_game_time.max(0) as u64
        };
        if add == 0 {
            return;
        }
        self.timer_ms = self.timer_ms.saturating_add(add);
        if let Some(total) = self.total_ms {
            if total > 0 && self.timer_ms >= total {
                if self.loop_flag {
                    self.timer_ms %= total;
                    self.just_looped = true;
                } else {
                    self.playing = false;
                    self.just_finished = true;
                }
            }
        }
    }

    pub fn seek(&mut self, time_ms: u64) {
        self.timer_ms = time_ms;
        if let Some(total) = self.total_ms {
            if total > 0 {
                self.timer_ms %= total;
            }
        }
        self.last_tick = Some(std::time::Instant::now());
        self.last_frame_idx = None;
        self.seeked = true;
    }

    pub fn get_seek_time(&self) -> u64 {
        if let Some(total) = self.total_ms {
            if total > 0 {
                return self.timer_ms % total;
            }
        }
        0
    }

    pub fn check_movie(&self) -> bool {
        self.playing
    }
}

#[derive(Debug, Default, Clone)]
pub struct ObjectEmoteParam {
    pub width: i64,
    pub height: i64,
    pub file_name: Option<String>,
    pub rep_x: i64,
    pub rep_y: i64,
}

#[derive(Debug, Clone, Default)]
pub struct ObjectFrameActionState {
    pub scn_name: String,
    pub cmd_name: String,
    pub counter: Counter,
    pub end_time: i64,
    pub real_time_flag: bool,
    pub end_flag: bool,
    pub args: Vec<crate::runtime::Value>,
}

#[derive(Debug, Clone)]
pub struct ObjectBaseState {
    pub wipe_copy: i64,
    pub wipe_erase: i64,
    pub click_disable: i64,
    pub disp: i64,
    pub patno: i64,
    pub world: i64,
    pub order: i64,
    pub layer: i64,
    pub x: i64,
    pub y: i64,
    pub z: i64,
    pub center_x: i64,
    pub center_y: i64,
    pub center_z: i64,
    pub center_rep_x: i64,
    pub center_rep_y: i64,
    pub center_rep_z: i64,
    pub scale_x: i64,
    pub scale_y: i64,
    pub scale_z: i64,
    pub rotate_x: i64,
    pub rotate_y: i64,
    pub rotate_z: i64,
    pub clip_use: i64,
    pub clip_left: i64,
    pub clip_top: i64,
    pub clip_right: i64,
    pub clip_bottom: i64,
    pub src_clip_use: i64,
    pub src_clip_left: i64,
    pub src_clip_top: i64,
    pub src_clip_right: i64,
    pub src_clip_bottom: i64,
    pub alpha: i64,
    pub tr: i64,
    pub mono: i64,
    pub reverse: i64,
    pub bright: i64,
    pub dark: i64,
    pub color_r: i64,
    pub color_g: i64,
    pub color_b: i64,
    pub color_rate: i64,
    pub color_add_r: i64,
    pub color_add_g: i64,
    pub color_add_b: i64,
    pub mask_no: i64,
    pub tonecurve_no: i64,
    pub light_no: i64,
    pub fog_use: i64,
    pub culling: i64,
    pub alpha_test: i64,
    pub alpha_blend: i64,
    pub blend: i64,
    pub child_sort_type: i64,
    pub no_event_hint: bool,
}

impl Default for ObjectBaseState {
    fn default() -> Self {
        Self {
            wipe_copy: 0,
            wipe_erase: 0,
            click_disable: 0,
            disp: 0,
            patno: 0,
            world: -1,
            order: 0,
            layer: 0,
            x: 0,
            y: 0,
            z: 0,
            center_x: 0,
            center_y: 0,
            center_z: 0,
            center_rep_x: 0,
            center_rep_y: 0,
            center_rep_z: 0,
            scale_x: 1000,
            scale_y: 1000,
            scale_z: 1000,
            rotate_x: 0,
            rotate_y: 0,
            rotate_z: 0,
            clip_use: 0,
            clip_left: 0,
            clip_top: 0,
            clip_right: 0,
            clip_bottom: 0,
            src_clip_use: 0,
            src_clip_left: 0,
            src_clip_top: 0,
            src_clip_right: 0,
            src_clip_bottom: 0,
            alpha: 255,
            tr: 255,
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
            mask_no: -1,
            tonecurve_no: -1,
            light_no: -1,
            fog_use: 0,
            culling: 0,
            alpha_test: 1,
            alpha_blend: 1,
            blend: 0,
            child_sort_type: 0,
            no_event_hint: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ObjectPropEvents {
    pub patno: IntEvent,
    pub x: IntEvent,
    pub y: IntEvent,
    pub z: IntEvent,
    pub center_x: IntEvent,
    pub center_y: IntEvent,
    pub center_z: IntEvent,
    pub center_rep_x: IntEvent,
    pub center_rep_y: IntEvent,
    pub center_rep_z: IntEvent,
    pub scale_x: IntEvent,
    pub scale_y: IntEvent,
    pub scale_z: IntEvent,
    pub rotate_x: IntEvent,
    pub rotate_y: IntEvent,
    pub rotate_z: IntEvent,
    pub clip_left: IntEvent,
    pub clip_top: IntEvent,
    pub clip_right: IntEvent,
    pub clip_bottom: IntEvent,
    pub src_clip_left: IntEvent,
    pub src_clip_top: IntEvent,
    pub src_clip_right: IntEvent,
    pub src_clip_bottom: IntEvent,
    pub tr: IntEvent,
    pub mono: IntEvent,
    pub reverse: IntEvent,
    pub bright: IntEvent,
    pub dark: IntEvent,
    pub color_r: IntEvent,
    pub color_g: IntEvent,
    pub color_b: IntEvent,
    pub color_rate: IntEvent,
    pub color_add_r: IntEvent,
    pub color_add_g: IntEvent,
    pub color_add_b: IntEvent,
}

impl Default for ObjectPropEvents {
    fn default() -> Self {
        Self {
            patno: IntEvent::new(0),
            x: IntEvent::new(0),
            y: IntEvent::new(0),
            z: IntEvent::new(0),
            center_x: IntEvent::new(0),
            center_y: IntEvent::new(0),
            center_z: IntEvent::new(0),
            center_rep_x: IntEvent::new(0),
            center_rep_y: IntEvent::new(0),
            center_rep_z: IntEvent::new(0),
            scale_x: IntEvent::new(1000),
            scale_y: IntEvent::new(1000),
            scale_z: IntEvent::new(1000),
            rotate_x: IntEvent::new(0),
            rotate_y: IntEvent::new(0),
            rotate_z: IntEvent::new(0),
            clip_left: IntEvent::new(0),
            clip_top: IntEvent::new(0),
            clip_right: IntEvent::new(0),
            clip_bottom: IntEvent::new(0),
            src_clip_left: IntEvent::new(0),
            src_clip_top: IntEvent::new(0),
            src_clip_right: IntEvent::new(0),
            src_clip_bottom: IntEvent::new(0),
            tr: IntEvent::new(255),
            mono: IntEvent::new(0),
            reverse: IntEvent::new(0),
            bright: IntEvent::new(0),
            dark: IntEvent::new(0),
            color_r: IntEvent::new(0),
            color_g: IntEvent::new(0),
            color_b: IntEvent::new(0),
            color_rate: IntEvent::new(0),
            color_add_r: IntEvent::new(0),
            color_add_g: IntEvent::new(0),
            color_add_b: IntEvent::new(0),
        }
    }
}

impl ObjectPropEvents {
    pub fn clear(&mut self) {
        self.patno.reinit();
        self.x.reinit();
        self.y.reinit();
        self.z.reinit();
        self.center_x.reinit();
        self.center_y.reinit();
        self.center_z.reinit();
        self.center_rep_x.reinit();
        self.center_rep_y.reinit();
        self.center_rep_z.reinit();
        self.scale_x.reinit();
        self.scale_y.reinit();
        self.scale_z.reinit();
        self.rotate_x.reinit();
        self.rotate_y.reinit();
        self.rotate_z.reinit();
        self.clip_left.reinit();
        self.clip_top.reinit();
        self.clip_right.reinit();
        self.clip_bottom.reinit();
        self.src_clip_left.reinit();
        self.src_clip_top.reinit();
        self.src_clip_right.reinit();
        self.src_clip_bottom.reinit();
        self.tr.reinit();
        self.mono.reinit();
        self.reverse.reinit();
        self.bright.reinit();
        self.dark.reinit();
        self.color_r.reinit();
        self.color_g.reinit();
        self.color_b.reinit();
        self.color_rate.reinit();
        self.color_add_r.reinit();
        self.color_add_g.reinit();
        self.color_add_b.reinit();
    }

    pub fn update_time(&mut self, past_game_time: i32, past_real_time: i32) {
        self.patno.update_time(past_game_time, past_real_time);
        self.x.update_time(past_game_time, past_real_time);
        self.y.update_time(past_game_time, past_real_time);
        self.z.update_time(past_game_time, past_real_time);
        self.center_x.update_time(past_game_time, past_real_time);
        self.center_y.update_time(past_game_time, past_real_time);
        self.center_z.update_time(past_game_time, past_real_time);
        self.center_rep_x.update_time(past_game_time, past_real_time);
        self.center_rep_y.update_time(past_game_time, past_real_time);
        self.center_rep_z.update_time(past_game_time, past_real_time);
        self.scale_x.update_time(past_game_time, past_real_time);
        self.scale_y.update_time(past_game_time, past_real_time);
        self.scale_z.update_time(past_game_time, past_real_time);
        self.rotate_x.update_time(past_game_time, past_real_time);
        self.rotate_y.update_time(past_game_time, past_real_time);
        self.rotate_z.update_time(past_game_time, past_real_time);
        self.clip_left.update_time(past_game_time, past_real_time);
        self.clip_top.update_time(past_game_time, past_real_time);
        self.clip_right.update_time(past_game_time, past_real_time);
        self.clip_bottom.update_time(past_game_time, past_real_time);
        self.src_clip_left.update_time(past_game_time, past_real_time);
        self.src_clip_top.update_time(past_game_time, past_real_time);
        self.src_clip_right.update_time(past_game_time, past_real_time);
        self.src_clip_bottom.update_time(past_game_time, past_real_time);
        self.tr.update_time(past_game_time, past_real_time);
        self.mono.update_time(past_game_time, past_real_time);
        self.reverse.update_time(past_game_time, past_real_time);
        self.bright.update_time(past_game_time, past_real_time);
        self.dark.update_time(past_game_time, past_real_time);
        self.color_r.update_time(past_game_time, past_real_time);
        self.color_g.update_time(past_game_time, past_real_time);
        self.color_b.update_time(past_game_time, past_real_time);
        self.color_rate.update_time(past_game_time, past_real_time);
        self.color_add_r.update_time(past_game_time, past_real_time);
        self.color_add_g.update_time(past_game_time, past_real_time);
        self.color_add_b.update_time(past_game_time, past_real_time);
    }

    pub fn frame(&mut self) {
        self.patno.frame();
        self.x.frame();
        self.y.frame();
        self.z.frame();
        self.center_x.frame();
        self.center_y.frame();
        self.center_z.frame();
        self.center_rep_x.frame();
        self.center_rep_y.frame();
        self.center_rep_z.frame();
        self.scale_x.frame();
        self.scale_y.frame();
        self.scale_z.frame();
        self.rotate_x.frame();
        self.rotate_y.frame();
        self.rotate_z.frame();
        self.clip_left.frame();
        self.clip_top.frame();
        self.clip_right.frame();
        self.clip_bottom.frame();
        self.src_clip_left.frame();
        self.src_clip_top.frame();
        self.src_clip_right.frame();
        self.src_clip_bottom.frame();
        self.tr.frame();
        self.mono.frame();
        self.reverse.frame();
        self.bright.frame();
        self.dark.frame();
        self.color_r.frame();
        self.color_g.frame();
        self.color_b.frame();
        self.color_rate.frame();
        self.color_add_r.frame();
        self.color_add_g.frame();
        self.color_add_b.frame();
    }

    pub fn tick(&mut self, delta: i32) {
        self.update_time(delta, delta);
        self.frame();
    }

    pub fn any_active(&self) -> bool {
        self.patno.check_event()
            || self.x.check_event()
            || self.y.check_event()
            || self.z.check_event()
            || self.center_x.check_event()
            || self.center_y.check_event()
            || self.center_z.check_event()
            || self.center_rep_x.check_event()
            || self.center_rep_y.check_event()
            || self.center_rep_z.check_event()
            || self.scale_x.check_event()
            || self.scale_y.check_event()
            || self.scale_z.check_event()
            || self.rotate_x.check_event()
            || self.rotate_y.check_event()
            || self.rotate_z.check_event()
            || self.clip_left.check_event()
            || self.clip_top.check_event()
            || self.clip_right.check_event()
            || self.clip_bottom.check_event()
            || self.src_clip_left.check_event()
            || self.src_clip_top.check_event()
            || self.src_clip_right.check_event()
            || self.src_clip_bottom.check_event()
            || self.tr.check_event()
            || self.mono.check_event()
            || self.reverse.check_event()
            || self.bright.check_event()
            || self.dark.check_event()
            || self.color_r.check_event()
            || self.color_g.check_event()
            || self.color_b.check_event()
            || self.color_rate.check_event()
            || self.color_add_r.check_event()
            || self.color_add_g.check_event()
            || self.color_add_b.check_event()
    }

    pub fn end_all(&mut self) {
        self.patno.end_event();
        self.x.end_event();
        self.y.end_event();
        self.z.end_event();
        self.center_x.end_event();
        self.center_y.end_event();
        self.center_z.end_event();
        self.center_rep_x.end_event();
        self.center_rep_y.end_event();
        self.center_rep_z.end_event();
        self.scale_x.end_event();
        self.scale_y.end_event();
        self.scale_z.end_event();
        self.rotate_x.end_event();
        self.rotate_y.end_event();
        self.rotate_z.end_event();
        self.clip_left.end_event();
        self.clip_top.end_event();
        self.clip_right.end_event();
        self.clip_bottom.end_event();
        self.src_clip_left.end_event();
        self.src_clip_top.end_event();
        self.src_clip_right.end_event();
        self.src_clip_bottom.end_event();
        self.tr.end_event();
        self.mono.end_event();
        self.reverse.end_event();
        self.bright.end_event();
        self.dark.end_event();
        self.color_r.end_event();
        self.color_g.end_event();
        self.color_b.end_event();
        self.color_rate.end_event();
        self.color_add_r.end_event();
        self.color_add_g.end_event();
        self.color_add_b.end_event();
    }

    pub fn get(&self, target: ObjectEventTarget) -> Option<&IntEvent> {
        match target {
            ObjectEventTarget::Patno => Some(&self.patno),
            ObjectEventTarget::X => Some(&self.x),
            ObjectEventTarget::Y => Some(&self.y),
            ObjectEventTarget::Z => Some(&self.z),
            ObjectEventTarget::CenterX => Some(&self.center_x),
            ObjectEventTarget::CenterY => Some(&self.center_y),
            ObjectEventTarget::CenterZ => Some(&self.center_z),
            ObjectEventTarget::CenterRepX => Some(&self.center_rep_x),
            ObjectEventTarget::CenterRepY => Some(&self.center_rep_y),
            ObjectEventTarget::CenterRepZ => Some(&self.center_rep_z),
            ObjectEventTarget::ScaleX => Some(&self.scale_x),
            ObjectEventTarget::ScaleY => Some(&self.scale_y),
            ObjectEventTarget::ScaleZ => Some(&self.scale_z),
            ObjectEventTarget::RotateX => Some(&self.rotate_x),
            ObjectEventTarget::RotateY => Some(&self.rotate_y),
            ObjectEventTarget::RotateZ => Some(&self.rotate_z),
            ObjectEventTarget::ClipLeft => Some(&self.clip_left),
            ObjectEventTarget::ClipTop => Some(&self.clip_top),
            ObjectEventTarget::ClipRight => Some(&self.clip_right),
            ObjectEventTarget::ClipBottom => Some(&self.clip_bottom),
            ObjectEventTarget::SrcClipLeft => Some(&self.src_clip_left),
            ObjectEventTarget::SrcClipTop => Some(&self.src_clip_top),
            ObjectEventTarget::SrcClipRight => Some(&self.src_clip_right),
            ObjectEventTarget::SrcClipBottom => Some(&self.src_clip_bottom),
            ObjectEventTarget::Tr => Some(&self.tr),
            ObjectEventTarget::Mono => Some(&self.mono),
            ObjectEventTarget::Reverse => Some(&self.reverse),
            ObjectEventTarget::Bright => Some(&self.bright),
            ObjectEventTarget::Dark => Some(&self.dark),
            ObjectEventTarget::ColorR => Some(&self.color_r),
            ObjectEventTarget::ColorG => Some(&self.color_g),
            ObjectEventTarget::ColorB => Some(&self.color_b),
            ObjectEventTarget::ColorRate => Some(&self.color_rate),
            ObjectEventTarget::ColorAddR => Some(&self.color_add_r),
            ObjectEventTarget::ColorAddG => Some(&self.color_add_g),
            ObjectEventTarget::ColorAddB => Some(&self.color_add_b),
            ObjectEventTarget::XRep
            | ObjectEventTarget::YRep
            | ObjectEventTarget::ZRep
            | ObjectEventTarget::TrRep
            | ObjectEventTarget::Alpha
            | ObjectEventTarget::Order
            | ObjectEventTarget::Layer
            | ObjectEventTarget::Unknown => None,
        }
    }

    pub fn get_mut(&mut self, target: ObjectEventTarget) -> Option<&mut IntEvent> {
        match target {
            ObjectEventTarget::Patno => Some(&mut self.patno),
            ObjectEventTarget::X => Some(&mut self.x),
            ObjectEventTarget::Y => Some(&mut self.y),
            ObjectEventTarget::Z => Some(&mut self.z),
            ObjectEventTarget::CenterX => Some(&mut self.center_x),
            ObjectEventTarget::CenterY => Some(&mut self.center_y),
            ObjectEventTarget::CenterZ => Some(&mut self.center_z),
            ObjectEventTarget::CenterRepX => Some(&mut self.center_rep_x),
            ObjectEventTarget::CenterRepY => Some(&mut self.center_rep_y),
            ObjectEventTarget::CenterRepZ => Some(&mut self.center_rep_z),
            ObjectEventTarget::ScaleX => Some(&mut self.scale_x),
            ObjectEventTarget::ScaleY => Some(&mut self.scale_y),
            ObjectEventTarget::ScaleZ => Some(&mut self.scale_z),
            ObjectEventTarget::RotateX => Some(&mut self.rotate_x),
            ObjectEventTarget::RotateY => Some(&mut self.rotate_y),
            ObjectEventTarget::RotateZ => Some(&mut self.rotate_z),
            ObjectEventTarget::ClipLeft => Some(&mut self.clip_left),
            ObjectEventTarget::ClipTop => Some(&mut self.clip_top),
            ObjectEventTarget::ClipRight => Some(&mut self.clip_right),
            ObjectEventTarget::ClipBottom => Some(&mut self.clip_bottom),
            ObjectEventTarget::SrcClipLeft => Some(&mut self.src_clip_left),
            ObjectEventTarget::SrcClipTop => Some(&mut self.src_clip_top),
            ObjectEventTarget::SrcClipRight => Some(&mut self.src_clip_right),
            ObjectEventTarget::SrcClipBottom => Some(&mut self.src_clip_bottom),
            ObjectEventTarget::Tr => Some(&mut self.tr),
            ObjectEventTarget::Mono => Some(&mut self.mono),
            ObjectEventTarget::Reverse => Some(&mut self.reverse),
            ObjectEventTarget::Bright => Some(&mut self.bright),
            ObjectEventTarget::Dark => Some(&mut self.dark),
            ObjectEventTarget::ColorR => Some(&mut self.color_r),
            ObjectEventTarget::ColorG => Some(&mut self.color_g),
            ObjectEventTarget::ColorB => Some(&mut self.color_b),
            ObjectEventTarget::ColorRate => Some(&mut self.color_rate),
            ObjectEventTarget::ColorAddR => Some(&mut self.color_add_r),
            ObjectEventTarget::ColorAddG => Some(&mut self.color_add_g),
            ObjectEventTarget::ColorAddB => Some(&mut self.color_add_b),
            ObjectEventTarget::XRep
            | ObjectEventTarget::YRep
            | ObjectEventTarget::ZRep
            | ObjectEventTarget::TrRep
            | ObjectEventTarget::Alpha
            | ObjectEventTarget::Order
            | ObjectEventTarget::Layer
            | ObjectEventTarget::Unknown => None,
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ObjectPropEventLists {
    pub x_rep: Vec<IntEvent>,
    pub y_rep: Vec<IntEvent>,
    pub z_rep: Vec<IntEvent>,
    pub tr_rep: Vec<IntEvent>,
}

impl ObjectPropEventLists {
    pub fn clear(&mut self) {
        self.x_rep.clear();
        self.y_rep.clear();
        self.z_rep.clear();
        self.tr_rep.clear();
    }

    pub fn update_time(&mut self, past_game_time: i32, past_real_time: i32) {
        for ev in &mut self.x_rep {
            ev.update_time(past_game_time, past_real_time);
        }
        for ev in &mut self.y_rep {
            ev.update_time(past_game_time, past_real_time);
        }
        for ev in &mut self.z_rep {
            ev.update_time(past_game_time, past_real_time);
        }
        for ev in &mut self.tr_rep {
            ev.update_time(past_game_time, past_real_time);
        }
    }

    pub fn frame(&mut self) {
        for ev in &mut self.x_rep {
            ev.frame();
        }
        for ev in &mut self.y_rep {
            ev.frame();
        }
        for ev in &mut self.z_rep {
            ev.frame();
        }
        for ev in &mut self.tr_rep {
            ev.frame();
        }
    }

    pub fn tick(&mut self, delta: i32) {
        self.update_time(delta, delta);
        self.frame();
    }

    pub fn any_active(&self) -> bool {
        self.x_rep.iter().any(|e| e.check_event())
            || self.y_rep.iter().any(|e| e.check_event())
            || self.z_rep.iter().any(|e| e.check_event())
            || self.tr_rep.iter().any(|e| e.check_event())
    }

    pub fn end_all(&mut self) {
        for ev in &mut self.x_rep {
            ev.end_event();
        }
        for ev in &mut self.y_rep {
            ev.end_event();
        }
        for ev in &mut self.z_rep {
            ev.end_event();
        }
        for ev in &mut self.tr_rep {
            ev.end_event();
        }
    }
}

#[derive(Debug, Clone)]
pub struct ObjectPropLists {
    pub x_rep: Vec<i64>,
    pub y_rep: Vec<i64>,
    pub z_rep: Vec<i64>,
    pub tr_rep: Vec<i64>,
    pub f: Vec<i64>,
}

impl Default for ObjectPropLists {
    fn default() -> Self {
        Self {
            x_rep: Vec::new(),
            y_rep: Vec::new(),
            z_rep: Vec::new(),
            tr_rep: Vec::new(),
            f: vec![0; 32],
        }
    }
}

impl ObjectPropLists {
    pub fn clear(&mut self) {
        self.x_rep.clear();
        self.y_rep.clear();
        self.z_rep.clear();
        self.tr_rep.clear();
        self.f.fill(0);
    }
}

#[derive(Debug, Default, Clone)]
pub struct ObjectRuntimeState {
    pub explicit_int_props: HashSet<i32>,
    pub explicit_str_props: HashSet<i32>,
    pub prop_events: ObjectPropEvents,
    pub prop_event_lists: ObjectPropEventLists,
    pub prop_lists: ObjectPropLists,
    pub child_objects: Vec<ObjectState>,
}

#[derive(Debug, Default, Clone)]
pub struct ObjectState {
    pub used: bool,
    pub backend: ObjectBackend,
    pub file_name: Option<String>,
    pub string_value: Option<String>,

    /// TNM_OBJECT_TYPE_* (0=none, 1=rect, 2=pct, 3=string, 4=weather, 5=number, ...).
    pub object_type: i64,

    /// For NUMBER objects, stores the current number value.
    pub number_value: i64,

    /// For STRING objects.
    pub string_param: ObjectStringParam,

    /// For NUMBER objects.
    pub number_param: ObjectNumberParam,

    /// For WEATHER objects (type A/B).
    pub weather_param: ObjectWeatherParam,
    pub weather_work: ObjectWeatherWorkState,

    /// For SAVE_THUMB / THUMB objects.
    pub thumb_save_no: i64,

    /// For MOVIE objects.
    pub movie: ObjectMovieState,

    /// For E-mote objects.
    pub emote: ObjectEmoteParam,

    /// Last loaded GAN file.
    pub gan_file: Option<String>,
    /// GAN runtime state.
    pub gan: GanState,

    /// OBJECT.FRAME_ACTION state.
    pub frame_action: ObjectFrameActionState,
    /// OBJECT.FRAME_ACTION_CH state.
    pub frame_action_ch: Vec<ObjectFrameActionState>,

    /// Cached masked sprite images keyed by (layer_id, sprite_id).
    pub mask_cache: HashMap<(LayerId, SpriteId), MaskedSpriteCache>,

    pub base: ObjectBaseState,

    pub button: ObjectButtonState,

    pub runtime: ObjectRuntimeState,

    pub mesh_animation_state: crate::mesh3d::MeshAnimationState,
    pub nested_runtime_slot: Option<usize>,
}

impl ObjectState {
    fn sync_event_backed_prop_value(
        &mut self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
        value: i64,
    ) {
        let target = if ids.obj_patno != 0 && op == ids.obj_patno {
            ObjectEventTarget::Patno
        } else if ids.obj_x != 0 && op == ids.obj_x {
            ObjectEventTarget::X
        } else if ids.obj_y != 0 && op == ids.obj_y {
            ObjectEventTarget::Y
        } else if ids.obj_z != 0 && op == ids.obj_z {
            ObjectEventTarget::Z
        } else if ids.obj_center_x != 0 && op == ids.obj_center_x {
            ObjectEventTarget::CenterX
        } else if ids.obj_center_y != 0 && op == ids.obj_center_y {
            ObjectEventTarget::CenterY
        } else if ids.obj_center_z != 0 && op == ids.obj_center_z {
            ObjectEventTarget::CenterZ
        } else if ids.obj_center_rep_x != 0 && op == ids.obj_center_rep_x {
            ObjectEventTarget::CenterRepX
        } else if ids.obj_center_rep_y != 0 && op == ids.obj_center_rep_y {
            ObjectEventTarget::CenterRepY
        } else if ids.obj_center_rep_z != 0 && op == ids.obj_center_rep_z {
            ObjectEventTarget::CenterRepZ
        } else if ids.obj_scale_x != 0 && op == ids.obj_scale_x {
            ObjectEventTarget::ScaleX
        } else if ids.obj_scale_y != 0 && op == ids.obj_scale_y {
            ObjectEventTarget::ScaleY
        } else if ids.obj_scale_z != 0 && op == ids.obj_scale_z {
            ObjectEventTarget::ScaleZ
        } else if ids.obj_rotate_x != 0 && op == ids.obj_rotate_x {
            ObjectEventTarget::RotateX
        } else if ids.obj_rotate_y != 0 && op == ids.obj_rotate_y {
            ObjectEventTarget::RotateY
        } else if ids.obj_rotate_z != 0 && op == ids.obj_rotate_z {
            ObjectEventTarget::RotateZ
        } else if ids.obj_clip_left != 0 && op == ids.obj_clip_left {
            ObjectEventTarget::ClipLeft
        } else if ids.obj_clip_top != 0 && op == ids.obj_clip_top {
            ObjectEventTarget::ClipTop
        } else if ids.obj_clip_right != 0 && op == ids.obj_clip_right {
            ObjectEventTarget::ClipRight
        } else if ids.obj_clip_bottom != 0 && op == ids.obj_clip_bottom {
            ObjectEventTarget::ClipBottom
        } else if ids.obj_src_clip_left != 0 && op == ids.obj_src_clip_left {
            ObjectEventTarget::SrcClipLeft
        } else if ids.obj_src_clip_top != 0 && op == ids.obj_src_clip_top {
            ObjectEventTarget::SrcClipTop
        } else if ids.obj_src_clip_right != 0 && op == ids.obj_src_clip_right {
            ObjectEventTarget::SrcClipRight
        } else if ids.obj_src_clip_bottom != 0 && op == ids.obj_src_clip_bottom {
            ObjectEventTarget::SrcClipBottom
        } else if ids.obj_tr != 0 && op == ids.obj_tr {
            ObjectEventTarget::Tr
        } else if ids.obj_mono != 0 && op == ids.obj_mono {
            ObjectEventTarget::Mono
        } else if ids.obj_reverse != 0 && op == ids.obj_reverse {
            ObjectEventTarget::Reverse
        } else if ids.obj_bright != 0 && op == ids.obj_bright {
            ObjectEventTarget::Bright
        } else if ids.obj_dark != 0 && op == ids.obj_dark {
            ObjectEventTarget::Dark
        } else if ids.obj_color_r != 0 && op == ids.obj_color_r {
            ObjectEventTarget::ColorR
        } else if ids.obj_color_g != 0 && op == ids.obj_color_g {
            ObjectEventTarget::ColorG
        } else if ids.obj_color_b != 0 && op == ids.obj_color_b {
            ObjectEventTarget::ColorB
        } else if ids.obj_color_rate != 0 && op == ids.obj_color_rate {
            ObjectEventTarget::ColorRate
        } else if ids.obj_color_add_r != 0 && op == ids.obj_color_add_r {
            ObjectEventTarget::ColorAddR
        } else if ids.obj_color_add_g != 0 && op == ids.obj_color_add_g {
            ObjectEventTarget::ColorAddG
        } else if ids.obj_color_add_b != 0 && op == ids.obj_color_add_b {
            ObjectEventTarget::ColorAddB
        } else {
            self.event_target(ids, op)
        };

        let Some(ev) = self.runtime.prop_events.get_mut(target) else {
            return;
        };
        ev.set_value(value as i32);
        if !ev.check_event() {
            ev.cur_value = value as i32;
        }
    }

    /// Reset type-specific parameters (mirrors C_elm_object::init_type(true)).
    ///
    /// Important: this does NOT clear button/groups/events (those are part of init_param/reinit in the original implementation).
    pub fn init_type_like(&mut self) {
        self.backend = ObjectBackend::None;
        self.file_name = None;
        self.string_value = None;
        self.object_type = 0;

        self.number_value = 0;
        self.string_param = ObjectStringParam::default();
        self.number_param = ObjectNumberParam::default();
        self.weather_param = ObjectWeatherParam::default();
        self.weather_work = ObjectWeatherWorkState::default();
        self.thumb_save_no = -1;

        self.movie.reset();
        self.emote = ObjectEmoteParam::default();

        self.gan_file = None;
        self.gan.reset();
        self.mask_cache.clear();
        self.mesh_animation_state = crate::mesh3d::MeshAnimationState::default();
    }

    fn weather_rand_percent(&mut self, base: i64, span: i64) -> i64 {
        base + self.weather_work.rand_mod(span)
    }

    fn setup_weather_sub(&mut self, idx: usize, init_state: i64, screen_w: i64, screen_h: i64) {
        if idx >= self.weather_work.sub.len() {
            return;
        }
        let param = self.weather_param.clone();
        let screen_w = screen_w.max(1);
        let screen_h = screen_h.max(1);
        let mut sub = ObjectWeatherWorkSub::default();
        sub.state = init_state;

        if param.weather_type == 1 {
            sub.move_start_pos_x = self.weather_work.rand_mod(screen_w);
            sub.move_start_pos_y = self.weather_work.rand_mod(screen_h);
            sub.move_time_x = param.move_time_x * self.weather_rand_percent(90, 20) / 100;
            sub.move_time_y = param.move_time_y * self.weather_rand_percent(90, 20) / 100;
            sub.move_cur_time = self.weather_work.next_rand();
            sub.sin_time_x = param.sin_time_x * self.weather_rand_percent(90, 20) / 100;
            sub.sin_time_y = param.sin_time_y * self.weather_rand_percent(90, 20) / 100;
            sub.sin_power_x = param.sin_power_x * self.weather_rand_percent(90, 20) / 100;
            sub.sin_power_y = param.sin_power_y * self.weather_rand_percent(90, 20) / 100;
            sub.sin_cur_time = self.weather_work.next_rand();
            sub.scale_x = param.scale_x;
            sub.scale_y = param.scale_y;
            sub.active_time_len = param.active_time * self.weather_rand_percent(80, 40) / 100;
            sub.state_time_len = if init_state == 0 {
                self.weather_work.rand_mod(3000)
            } else {
                sub.active_time_len
            };
            sub.real_time_flag = param.real_time_flag;
        } else if param.weather_type == 2 {
            let max_distance_x = if param.center_x > screen_w / 2 {
                param.center_x
            } else {
                screen_w - param.center_x
            };
            let max_distance_y = if param.center_y > screen_h / 2 {
                param.center_y
            } else {
                screen_h - param.center_y
            };
            let max_distance =
                (((max_distance_x * max_distance_x + max_distance_y * max_distance_y) as f64)
                    .sqrt()) as i64;
            sub.move_start_distance =
                self.weather_work.rand_mod(max_distance.max(1)) * param.appear_range / 100;
            sub.move_start_degree = self.weather_work.rand_mod(3600);
            sub.move_time_x = param.move_time_x.abs() * self.weather_rand_percent(80, 40) / 100;
            sub.move_time_y = param.move_time_y.abs() * self.weather_rand_percent(80, 40) / 100;
            sub.sin_time_x = param.sin_time_x * self.weather_rand_percent(90, 20) / 100;
            sub.sin_time_y = param.sin_time_y * self.weather_rand_percent(90, 20) / 100;
            sub.sin_power_x = param.sin_power_x * self.weather_rand_percent(90, 20) / 100;
            sub.sin_power_y = param.sin_power_y * self.weather_rand_percent(90, 20) / 100;
            sub.sin_cur_time = self.weather_work.next_rand();
            sub.center_rotate = param.center_rotate * self.weather_rand_percent(90, 20) / 100;
            sub.zoom_min = param.zoom_min;
            sub.zoom_max = param.zoom_max;
            sub.scale_x = param.scale_x;
            sub.scale_y = param.scale_y;
            sub.active_time_len = param.move_time_x.abs() * max_distance / 1000 - 1000;
            sub.state_time_len = if init_state == 0 {
                self.weather_work
                    .rand_mod((1000 + sub.active_time_len + 1000).max(1))
            } else {
                sub.active_time_len
            };
            sub.move_cur_time = if sub.state_time_len == 0 {
                0
            } else {
                self.weather_work.rand_mod(sub.state_time_len.abs().max(1))
            };
            sub.real_time_flag = param.real_time_flag;
        }
        sub.restruct_flag = false;
        self.weather_work.sub[idx] = sub;
    }

    pub fn weather_sprite_count(&self) -> usize {
        match self.weather_param.weather_type {
            1 => self.weather_work.cnt_max.saturating_mul(4),
            2 => self.weather_work.cnt_max,
            _ => 0,
        }
    }

    pub fn restruct_weather_work(&mut self, screen_w: i64, screen_h: i64) {
        if self.object_type != 4 {
            return;
        }
        for sub in &mut self.weather_work.sub {
            sub.restruct_flag = true;
        }
        let cnt = self.weather_param.cnt.max(0) as usize;
        let old_cnt = self.weather_work.sub.len();
        if cnt > old_cnt {
            self.weather_work
                .sub
                .resize_with(cnt, ObjectWeatherWorkSub::default);
            for idx in old_cnt..cnt {
                self.setup_weather_sub(idx, 0, screen_w, screen_h);
            }
        }
        if cnt > self.weather_work.cnt_max {
            self.weather_work.cnt_max = cnt;
        }
    }


    pub fn init_param_like(&mut self) {
        self.base = ObjectBaseState::default();
        self.button.clear();
        self.runtime.explicit_int_props.clear();
        self.runtime.explicit_str_props.clear();
        self.runtime.prop_events.clear();
        self.runtime.prop_lists.clear();
        self.runtime.prop_event_lists.clear();
        self.frame_action = ObjectFrameActionState::default();
        self.frame_action_ch.clear();
        self.gan_file = None;
        self.gan.reset();
    }

    pub fn clear_runtime_only(&mut self) {
        self.runtime.explicit_int_props.clear();
        self.runtime.explicit_str_props.clear();
        self.runtime.prop_events.clear();
        self.runtime.prop_lists.clear();
        self.runtime.prop_event_lists.clear();
        self.frame_action = ObjectFrameActionState::default();
        self.frame_action_ch.clear();
    }

    pub fn set_int_prop(
        &mut self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
        value: i64,
    ) {
        self.runtime.explicit_int_props.insert(op);
        let ok =
            self.sync_fixed_int_prop(ids, op, value) || self.sync_special_int_prop(ids, op, value);
        assert!(ok, "unknown object int property op {}", op);
    }

    pub fn has_int_prop(&self, op: i32) -> bool {
        self.runtime.explicit_int_props.contains(&op)
    }

    pub fn remove_int_prop(&mut self, op: i32) {
        self.runtime.explicit_int_props.remove(&op);
    }

    pub fn set_str_prop(
        &mut self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
        value: String,
    ) {
        self.runtime.explicit_str_props.insert(op);
        let ok = self.sync_special_str_prop(ids, op, value);
        assert!(ok, "unknown object string property op {}", op);
    }

    pub fn lookup_str_prop(
        &self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
    ) -> Option<String> {
        self.special_str_prop(ids, op)
    }

    pub fn has_str_prop(&self, op: i32) -> bool {
        self.runtime.explicit_str_props.contains(&op)
    }

    pub fn remove_str_prop(&mut self, ids: &crate::runtime::constants::RuntimeConstants, op: i32) {
        self.runtime.explicit_str_props.remove(&op);
        if ids.obj_mesh_anim_clip_name != 0 && op == ids.obj_mesh_anim_clip_name {
            let mut next = self.mesh_animation_state.clone();
            next.clip_name = None;
            self.set_mesh_animation_state(next);
        } else if ids.obj_mesh_anim_blend_clip_name != 0 && op == ids.obj_mesh_anim_blend_clip_name
        {
            let mut next = self.mesh_animation_state.clone();
            next.blend_clip_name = None;
            self.set_mesh_animation_state(next);
        }
    }

    pub fn lookup_int_prop(
        &self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
    ) -> Option<i64> {
        self.fixed_int_prop(ids, op)
            .or_else(|| self.special_int_prop(ids, op))
    }

    pub fn get_int_prop(&self, ids: &crate::runtime::constants::RuntimeConstants, op: i32) -> i64 {
        self.lookup_int_prop(ids, op).unwrap_or(0)
    }

    pub fn runtime_slot_or(&self, fallback: usize) -> usize {
        self.nested_runtime_slot.unwrap_or(fallback)
    }

    pub fn ensure_runtime_slot(&mut self, next_slot: &mut usize) -> usize {
        if let Some(slot) = self.nested_runtime_slot {
            return slot;
        }
        let slot = *next_slot;
        *next_slot += 1;
        self.nested_runtime_slot = Some(slot);
        slot
    }

    pub fn int_list_by_op<'a>(
        &'a self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
    ) -> Option<&'a Vec<i64>> {
        if ids.obj_f != 0 && op == ids.obj_f {
            Some(&self.runtime.prop_lists.f)
        } else {
            None
        }
    }

    pub fn int_list_by_op_mut<'a>(
        &'a mut self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
    ) -> Option<&'a mut Vec<i64>> {
        if ids.obj_f != 0 && op == ids.obj_f {
            Some(&mut self.runtime.prop_lists.f)
        } else {
            None
        }
    }

    pub fn rep_int_event_list_by_rep_op<'a>(
        &'a self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
    ) -> Option<&'a Vec<IntEvent>> {
        if ids.obj_x_rep != 0 && op == ids.obj_x_rep {
            Some(&self.runtime.prop_event_lists.x_rep)
        } else if ids.obj_y_rep != 0 && op == ids.obj_y_rep {
            Some(&self.runtime.prop_event_lists.y_rep)
        } else if ids.obj_z_rep != 0 && op == ids.obj_z_rep {
            Some(&self.runtime.prop_event_lists.z_rep)
        } else if ids.obj_tr_rep != 0 && op == ids.obj_tr_rep {
            Some(&self.runtime.prop_event_lists.tr_rep)
        } else {
            None
        }
    }

    pub fn rep_int_event_list_by_rep_op_mut<'a>(
        &'a mut self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
    ) -> Option<&'a mut Vec<IntEvent>> {
        if ids.obj_x_rep != 0 && op == ids.obj_x_rep {
            Some(&mut self.runtime.prop_event_lists.x_rep)
        } else if ids.obj_y_rep != 0 && op == ids.obj_y_rep {
            Some(&mut self.runtime.prop_event_lists.y_rep)
        } else if ids.obj_z_rep != 0 && op == ids.obj_z_rep {
            Some(&mut self.runtime.prop_event_lists.z_rep)
        } else if ids.obj_tr_rep != 0 && op == ids.obj_tr_rep {
            Some(&mut self.runtime.prop_event_lists.tr_rep)
        } else {
            None
        }
    }

    pub fn int_event_by_op<'a>(
        &'a self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
    ) -> Option<&'a IntEvent> {
        self.runtime.prop_events.get(self.event_target(ids, op))
    }

    pub fn int_event_by_op_mut<'a>(
        &'a mut self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
    ) -> Option<&'a mut IntEvent> {
        let target = self.event_target(ids, op);
        self.runtime.prop_events.get_mut(target)
    }

    pub fn int_event_list_by_op<'a>(
        &'a self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
    ) -> Option<&'a Vec<IntEvent>> {
        if ids.obj_x_rep_eve != 0 && op == ids.obj_x_rep_eve {
            Some(&self.runtime.prop_event_lists.x_rep)
        } else if ids.obj_y_rep_eve != 0 && op == ids.obj_y_rep_eve {
            Some(&self.runtime.prop_event_lists.y_rep)
        } else if ids.obj_z_rep_eve != 0 && op == ids.obj_z_rep_eve {
            Some(&self.runtime.prop_event_lists.z_rep)
        } else if ids.obj_tr_rep_eve != 0 && op == ids.obj_tr_rep_eve {
            Some(&self.runtime.prop_event_lists.tr_rep)
        } else {
            None
        }
    }

    pub fn int_event_list_by_op_mut<'a>(
        &'a mut self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
    ) -> Option<&'a mut Vec<IntEvent>> {
        if ids.obj_x_rep_eve != 0 && op == ids.obj_x_rep_eve {
            Some(&mut self.runtime.prop_event_lists.x_rep)
        } else if ids.obj_y_rep_eve != 0 && op == ids.obj_y_rep_eve {
            Some(&mut self.runtime.prop_event_lists.y_rep)
        } else if ids.obj_z_rep_eve != 0 && op == ids.obj_z_rep_eve {
            Some(&mut self.runtime.prop_event_lists.z_rep)
        } else if ids.obj_tr_rep_eve != 0 && op == ids.obj_tr_rep_eve {
            Some(&mut self.runtime.prop_event_lists.tr_rep)
        } else {
            None
        }
    }

    fn sync_fixed_int_prop(
        &mut self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
        value: i64,
    ) -> bool {
        macro_rules! set_if {
            ($id:expr, $field:ident) => {
                if $id != 0 && op == $id {
                    self.base.$field = value;
                    self.sync_event_backed_prop_value(ids, op, value);
                    return true;
                }
            };
        }
        if op == ids.obj_disp {
            self.base.disp = value;
            return true;
        }
        set_if!(ids.obj_wipe_copy, wipe_copy);
        set_if!(ids.obj_wipe_erase, wipe_erase);
        set_if!(ids.obj_click_disable, click_disable);
        set_if!(ids.obj_patno, patno);
        set_if!(ids.obj_world, world);
        set_if!(ids.obj_order, order);
        set_if!(ids.obj_layer, layer);
        set_if!(ids.obj_x, x);
        set_if!(ids.obj_y, y);
        set_if!(ids.obj_z, z);
        set_if!(ids.obj_center_x, center_x);
        set_if!(ids.obj_center_y, center_y);
        set_if!(ids.obj_center_z, center_z);
        set_if!(ids.obj_center_rep_x, center_rep_x);
        set_if!(ids.obj_center_rep_y, center_rep_y);
        set_if!(ids.obj_center_rep_z, center_rep_z);
        set_if!(ids.obj_scale_x, scale_x);
        set_if!(ids.obj_scale_y, scale_y);
        set_if!(ids.obj_scale_z, scale_z);
        set_if!(ids.obj_rotate_x, rotate_x);
        set_if!(ids.obj_rotate_y, rotate_y);
        set_if!(ids.obj_rotate_z, rotate_z);
        set_if!(ids.obj_clip_use, clip_use);
        set_if!(ids.obj_clip_left, clip_left);
        set_if!(ids.obj_clip_top, clip_top);
        set_if!(ids.obj_clip_right, clip_right);
        set_if!(ids.obj_clip_bottom, clip_bottom);
        set_if!(ids.obj_src_clip_use, src_clip_use);
        set_if!(ids.obj_src_clip_left, src_clip_left);
        set_if!(ids.obj_src_clip_top, src_clip_top);
        set_if!(ids.obj_src_clip_right, src_clip_right);
        set_if!(ids.obj_src_clip_bottom, src_clip_bottom);
        set_if!(ids.obj_alpha, alpha);
        set_if!(ids.obj_tr, tr);
        set_if!(ids.obj_mono, mono);
        set_if!(ids.obj_reverse, reverse);
        set_if!(ids.obj_bright, bright);
        set_if!(ids.obj_dark, dark);
        set_if!(ids.obj_color_r, color_r);
        set_if!(ids.obj_color_g, color_g);
        set_if!(ids.obj_color_b, color_b);
        set_if!(ids.obj_color_rate, color_rate);
        set_if!(ids.obj_color_add_r, color_add_r);
        set_if!(ids.obj_color_add_g, color_add_g);
        set_if!(ids.obj_color_add_b, color_add_b);
        set_if!(ids.obj_mask_no, mask_no);
        set_if!(ids.obj_tonecurve_no, tonecurve_no);
        set_if!(ids.obj_light_no, light_no);
        set_if!(ids.obj_fog_use, fog_use);
        set_if!(ids.obj_culling, culling);
        set_if!(ids.obj_alpha_test, alpha_test);
        set_if!(ids.obj_alpha_blend, alpha_blend);
        set_if!(ids.obj_blend, blend);
        false
    }

    fn sync_special_int_prop(
        &mut self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
        value: i64,
    ) -> bool {
        if op == OBJECT_NESTED_SLOT_KEY {
            self.nested_runtime_slot = (value >= 0).then_some(value as usize);
            return true;
        }
        if ids.obj_x_rep != 0 && op == ids.obj_x_rep {
            if self.runtime.prop_event_lists.x_rep.is_empty() {
                self.runtime.prop_event_lists.x_rep.push(IntEvent::new(0));
            }
            self.runtime.prop_event_lists.x_rep[0].set_value(value as i32);
            return true;
        }
        if ids.obj_y_rep != 0 && op == ids.obj_y_rep {
            if self.runtime.prop_event_lists.y_rep.is_empty() {
                self.runtime.prop_event_lists.y_rep.push(IntEvent::new(0));
            }
            self.runtime.prop_event_lists.y_rep[0].set_value(value as i32);
            return true;
        }
        if ids.obj_z_rep != 0 && op == ids.obj_z_rep {
            if self.runtime.prop_event_lists.z_rep.is_empty() {
                self.runtime.prop_event_lists.z_rep.push(IntEvent::new(0));
            }
            self.runtime.prop_event_lists.z_rep[0].set_value(value as i32);
            return true;
        }
        if ids.obj_tr_rep != 0 && op == ids.obj_tr_rep {
            if self.runtime.prop_event_lists.tr_rep.is_empty() {
                self.runtime
                    .prop_event_lists
                    .tr_rep
                    .push(IntEvent::new(255));
            }
            self.runtime.prop_event_lists.tr_rep[0].set_value(value as i32);
            return true;
        }
        if ids.obj_mesh_anim_clip != 0 && op == ids.obj_mesh_anim_clip {
            let mut next = self.mesh_animation_state.clone();
            next.change_animation_clip(None, (value >= 0).then_some(value as usize));
            self.set_mesh_animation_state(next);
            return true;
        }
        if ids.obj_mesh_anim_rate != 0 && op == ids.obj_mesh_anim_rate {
            let mut next = self.mesh_animation_state.clone();
            next.rate = (value as f32) / 1000.0;
            self.set_mesh_animation_state(next);
            return true;
        }
        if ids.obj_mesh_anim_time_offset != 0 && op == ids.obj_mesh_anim_time_offset {
            let mut next = self.mesh_animation_state.clone();
            next.time_offset_sec = (value as f32) / 1000.0;
            self.set_mesh_animation_state(next);
            return true;
        }
        if ids.obj_mesh_anim_pause != 0 && op == ids.obj_mesh_anim_pause {
            let mut next = self.mesh_animation_state.clone();
            next.paused = value != 0;
            next.is_anim = !next.paused;
            self.set_mesh_animation_state(next);
            return true;
        }
        if ids.obj_mesh_anim_hold_time != 0 && op == ids.obj_mesh_anim_hold_time {
            let mut next = self.mesh_animation_state.clone();
            next.hold_time_sec = ((value as f32) / 1000.0).max(0.0);
            next.time_sec = if next.rate > 0.0 {
                next.hold_time_sec / next.rate.max(0.000_001)
            } else {
                0.0
            };
            self.set_mesh_animation_state(next);
            return true;
        }
        if ids.obj_mesh_anim_shift_time != 0 && op == ids.obj_mesh_anim_shift_time {
            let mut next = self.mesh_animation_state.clone();
            next.set_anim_shift_time_sec(((value as f32) / 1000.0).max(0.0));
            self.set_mesh_animation_state(next);
            return true;
        }
        if ids.obj_mesh_anim_loop != 0 && op == ids.obj_mesh_anim_loop {
            let mut next = self.mesh_animation_state.clone();
            next.looped = value != 0;
            self.set_mesh_animation_state(next);
            return true;
        }
        if ids.obj_mesh_anim_blend_clip != 0 && op == ids.obj_mesh_anim_blend_clip {
            let mut next = self.mesh_animation_state.clone();
            next.blend_clip_index = (value >= 0).then_some(value as usize);
            next.blend_clip_name = None;
            self.set_mesh_animation_state(next);
            return true;
        }
        if ids.obj_mesh_anim_blend_weight != 0 && op == ids.obj_mesh_anim_blend_weight {
            let mut next = self.mesh_animation_state.clone();
            next.blend_weight = ((value as f32) / 1000.0).clamp(0.0, 1.0);
            self.set_mesh_animation_state(next);
            return true;
        }
        false
    }

    fn special_int_prop(
        &self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
    ) -> Option<i64> {
        if op == OBJECT_NESTED_SLOT_KEY {
            return self.nested_runtime_slot.map(|v| v as i64);
        }
        if ids.obj_x_rep != 0 && op == ids.obj_x_rep {
            return self
                .runtime
                .prop_event_lists
                .x_rep
                .first()
                .map(|ev| ev.get_value() as i64);
        }
        if ids.obj_y_rep != 0 && op == ids.obj_y_rep {
            return self
                .runtime
                .prop_event_lists
                .y_rep
                .first()
                .map(|ev| ev.get_value() as i64);
        }
        if ids.obj_z_rep != 0 && op == ids.obj_z_rep {
            return self
                .runtime
                .prop_event_lists
                .z_rep
                .first()
                .map(|ev| ev.get_value() as i64);
        }
        if ids.obj_tr_rep != 0 && op == ids.obj_tr_rep {
            return self
                .runtime
                .prop_event_lists
                .tr_rep
                .first()
                .map(|ev| ev.get_value() as i64);
        }
        if ids.obj_mesh_anim_clip != 0 && op == ids.obj_mesh_anim_clip {
            return Some(
                self.mesh_animation_state
                    .clip_index
                    .map(|v| v as i64)
                    .unwrap_or(-1),
            );
        }
        if ids.obj_mesh_anim_rate != 0 && op == ids.obj_mesh_anim_rate {
            return Some((self.mesh_animation_state.rate * 1000.0).round() as i64);
        }
        if ids.obj_mesh_anim_time_offset != 0 && op == ids.obj_mesh_anim_time_offset {
            return Some((self.mesh_animation_state.time_offset_sec * 1000.0).round() as i64);
        }
        if ids.obj_mesh_anim_pause != 0 && op == ids.obj_mesh_anim_pause {
            return Some(if self.mesh_animation_state.paused {
                1
            } else {
                0
            });
        }
        if ids.obj_mesh_anim_hold_time != 0 && op == ids.obj_mesh_anim_hold_time {
            return Some((self.mesh_animation_state.hold_time_sec * 1000.0).round() as i64);
        }
        if ids.obj_mesh_anim_shift_time != 0 && op == ids.obj_mesh_anim_shift_time {
            return Some((self.mesh_animation_state.anim_shift_time_sec * 1000.0).round() as i64);
        }
        if ids.obj_mesh_anim_loop != 0 && op == ids.obj_mesh_anim_loop {
            return Some(if self.mesh_animation_state.looped {
                1
            } else {
                0
            });
        }
        if ids.obj_mesh_anim_blend_clip != 0 && op == ids.obj_mesh_anim_blend_clip {
            return Some(
                self.mesh_animation_state
                    .blend_clip_index
                    .map(|v| v as i64)
                    .unwrap_or(-1),
            );
        }
        if ids.obj_mesh_anim_blend_weight != 0 && op == ids.obj_mesh_anim_blend_weight {
            return Some((self.mesh_animation_state.blend_weight * 1000.0).round() as i64);
        }
        None
    }

    fn sync_special_str_prop(
        &mut self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
        value: String,
    ) -> bool {
        if ids.obj_mesh_anim_clip_name != 0 && op == ids.obj_mesh_anim_clip_name {
            let mut next = self.mesh_animation_state.clone();
            next.change_animation_clip(Some(value), None);
            self.set_mesh_animation_state(next);
            return true;
        }
        if ids.obj_mesh_anim_blend_clip_name != 0 && op == ids.obj_mesh_anim_blend_clip_name {
            let mut next = self.mesh_animation_state.clone();
            next.blend_clip_name = Some(value);
            next.blend_clip_index = None;
            self.set_mesh_animation_state(next);
            return true;
        }
        false
    }

    fn special_str_prop(
        &self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
    ) -> Option<String> {
        if ids.obj_mesh_anim_clip_name != 0 && op == ids.obj_mesh_anim_clip_name {
            return self.mesh_animation_state.clip_name.clone();
        }
        if ids.obj_mesh_anim_blend_clip_name != 0 && op == ids.obj_mesh_anim_blend_clip_name {
            return self.mesh_animation_state.blend_clip_name.clone();
        }
        None
    }

    fn fixed_int_prop(
        &self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
    ) -> Option<i64> {
        macro_rules! get_base_if {
            ($id:expr, $field:ident) => {
                if $id != 0 && op == $id {
                    return Some(self.base.$field);
                }
            };
        }
        macro_rules! get_event_total_if {
            ($id:expr, $target:expr) => {
                if $id != 0 && op == $id {
                    return self
                        .runtime
                        .prop_events
                        .get($target)
                        .map(|ev| ev.get_total_value() as i64);
                }
            };
        }
        if op == ids.obj_disp {
            return Some(self.base.disp);
        }
        get_base_if!(ids.obj_wipe_copy, wipe_copy);
        get_base_if!(ids.obj_wipe_erase, wipe_erase);
        get_base_if!(ids.obj_click_disable, click_disable);
        get_event_total_if!(ids.obj_patno, ObjectEventTarget::Patno);
        get_base_if!(ids.obj_world, world);
        get_base_if!(ids.obj_order, order);
        get_base_if!(ids.obj_layer, layer);
        get_event_total_if!(ids.obj_x, ObjectEventTarget::X);
        get_event_total_if!(ids.obj_y, ObjectEventTarget::Y);
        get_event_total_if!(ids.obj_z, ObjectEventTarget::Z);
        get_event_total_if!(ids.obj_center_x, ObjectEventTarget::CenterX);
        get_event_total_if!(ids.obj_center_y, ObjectEventTarget::CenterY);
        get_event_total_if!(ids.obj_center_z, ObjectEventTarget::CenterZ);
        get_event_total_if!(ids.obj_center_rep_x, ObjectEventTarget::CenterRepX);
        get_event_total_if!(ids.obj_center_rep_y, ObjectEventTarget::CenterRepY);
        get_event_total_if!(ids.obj_center_rep_z, ObjectEventTarget::CenterRepZ);
        get_event_total_if!(ids.obj_scale_x, ObjectEventTarget::ScaleX);
        get_event_total_if!(ids.obj_scale_y, ObjectEventTarget::ScaleY);
        get_event_total_if!(ids.obj_scale_z, ObjectEventTarget::ScaleZ);
        get_event_total_if!(ids.obj_rotate_x, ObjectEventTarget::RotateX);
        get_event_total_if!(ids.obj_rotate_y, ObjectEventTarget::RotateY);
        get_event_total_if!(ids.obj_rotate_z, ObjectEventTarget::RotateZ);
        get_base_if!(ids.obj_clip_use, clip_use);
        get_event_total_if!(ids.obj_clip_left, ObjectEventTarget::ClipLeft);
        get_event_total_if!(ids.obj_clip_top, ObjectEventTarget::ClipTop);
        get_event_total_if!(ids.obj_clip_right, ObjectEventTarget::ClipRight);
        get_event_total_if!(ids.obj_clip_bottom, ObjectEventTarget::ClipBottom);
        get_base_if!(ids.obj_src_clip_use, src_clip_use);
        get_event_total_if!(ids.obj_src_clip_left, ObjectEventTarget::SrcClipLeft);
        get_event_total_if!(ids.obj_src_clip_top, ObjectEventTarget::SrcClipTop);
        get_event_total_if!(ids.obj_src_clip_right, ObjectEventTarget::SrcClipRight);
        get_event_total_if!(ids.obj_src_clip_bottom, ObjectEventTarget::SrcClipBottom);
        get_base_if!(ids.obj_alpha, alpha);
        get_event_total_if!(ids.obj_tr, ObjectEventTarget::Tr);
        get_event_total_if!(ids.obj_mono, ObjectEventTarget::Mono);
        get_event_total_if!(ids.obj_reverse, ObjectEventTarget::Reverse);
        get_event_total_if!(ids.obj_bright, ObjectEventTarget::Bright);
        get_event_total_if!(ids.obj_dark, ObjectEventTarget::Dark);
        get_event_total_if!(ids.obj_color_r, ObjectEventTarget::ColorR);
        get_event_total_if!(ids.obj_color_g, ObjectEventTarget::ColorG);
        get_event_total_if!(ids.obj_color_b, ObjectEventTarget::ColorB);
        get_event_total_if!(ids.obj_color_rate, ObjectEventTarget::ColorRate);
        get_event_total_if!(ids.obj_color_add_r, ObjectEventTarget::ColorAddR);
        get_event_total_if!(ids.obj_color_add_g, ObjectEventTarget::ColorAddG);
        get_event_total_if!(ids.obj_color_add_b, ObjectEventTarget::ColorAddB);
        get_base_if!(ids.obj_mask_no, mask_no);
        get_base_if!(ids.obj_tonecurve_no, tonecurve_no);
        get_base_if!(ids.obj_light_no, light_no);
        get_base_if!(ids.obj_fog_use, fog_use);
        get_base_if!(ids.obj_culling, culling);
        get_base_if!(ids.obj_alpha_test, alpha_test);
        get_base_if!(ids.obj_alpha_blend, alpha_blend);
        get_base_if!(ids.obj_blend, blend);
        None
    }

    pub fn set_mesh_animation_state(&mut self, next: crate::mesh3d::MeshAnimationState) {
        self.apply_mesh_animation_state(next, None);
    }

    fn apply_mesh_animation_state(
        &mut self,
        next: crate::mesh3d::MeshAnimationState,
        explicit_hold_override: Option<f32>,
    ) {
        let prev = self.mesh_animation_state.clone();
        let mut merged = next.sanitized();
        let clip_changed =
            prev.clip_name != merged.clip_name || prev.clip_index != merged.clip_index;
        let pause_enter = !prev.paused && merged.paused;
        let pause_exit = prev.paused && !merged.paused;
        let prev_base = prev.current_sample_base_sec();

        merged.anim_track_no = prev.anim_track_no;
        merged.is_anim = !merged.paused;
        merged.time_sec = prev.time_sec.max(0.0);
        merged.hold_time_sec = prev.hold_time_sec.max(0.0);
        merged.prev_clip_name = prev.prev_clip_name.clone();
        merged.prev_clip_index = prev.prev_clip_index;
        merged.prev_time_sec = prev.prev_time_sec.max(0.0);
        merged.prev_time_offset_sec = prev.prev_time_offset_sec.max(0.0);
        merged.prev_rate = prev.prev_rate.max(0.0);
        merged.transition_elapsed_sec = prev.transition_elapsed_sec.max(0.0);

        if clip_changed {
            merged.change_animation_clip(merged.clip_name.clone(), merged.clip_index);
        }

        if let Some(hold_sec) = explicit_hold_override {
            let hold_sec = hold_sec.max(0.0);
            merged.hold_time_sec = hold_sec;
            merged.time_sec = if merged.rate > 0.0 {
                hold_sec / merged.rate.max(0.000_001)
            } else {
                0.0
            };
        } else if pause_enter {
            merged.hold_time_sec = prev_base;
        } else if pause_exit {
            merged.time_sec = if merged.rate > 0.0 {
                prev.hold_time_sec.max(0.0) / merged.rate.max(0.000_001)
            } else {
                prev.time_sec.max(0.0)
            };
        } else if merged.paused {
            merged.hold_time_sec = prev.hold_time_sec.max(0.0);
        } else if !clip_changed && (prev.rate - merged.rate).abs() > 0.000_001 {
            merged.time_sec = if merged.rate > 0.0 {
                prev_base / merged.rate.max(0.000_001)
            } else {
                prev.time_sec.max(0.0)
            };
        }

        self.mesh_animation_state = merged.sanitized();
    }

    pub fn sync_mesh_animation_state_from_props(
        &mut self,
        ids: &super::constants::RuntimeConstants,
    ) {
        let int_prop = |id: i32, default: i64| -> i64 {
            if id != 0 {
                self.lookup_int_prop(ids, id).unwrap_or(default)
            } else {
                default
            }
        };
        let str_prop = |id: i32| -> Option<String> {
            if id != 0 {
                self.lookup_str_prop(ids, id)
            } else {
                None
            }
        };
        let explicit_hold =
            ids.obj_mesh_anim_hold_time != 0 && self.has_int_prop(ids.obj_mesh_anim_hold_time);
        let requested_hold_sec = (int_prop(ids.obj_mesh_anim_hold_time, 0) as f32) / 1000.0;
        let requested_shift_sec = (int_prop(
            ids.obj_mesh_anim_shift_time,
            (self.mesh_animation_state.anim_shift_time_sec * 1000.0).round() as i64,
        ) as f32)
            / 1000.0;
        let next = crate::mesh3d::MeshAnimationState {
            clip_name: str_prop(ids.obj_mesh_anim_clip_name),
            clip_index: (int_prop(ids.obj_mesh_anim_clip, -1) >= 0).then_some(int_prop(
                ids.obj_mesh_anim_clip,
                -1,
            )
                as usize),
            blend_clip_name: str_prop(ids.obj_mesh_anim_blend_clip_name),
            blend_clip_index: (int_prop(ids.obj_mesh_anim_blend_clip, -1) >= 0)
                .then_some(int_prop(ids.obj_mesh_anim_blend_clip, -1) as usize),
            blend_weight: ((int_prop(ids.obj_mesh_anim_blend_weight, 0) as f32) / 1000.0)
                .clamp(0.0, 1.0),
            time_sec: self.mesh_animation_state.time_sec,
            rate: (int_prop(ids.obj_mesh_anim_rate, 1000) as f32) / 1000.0,
            time_offset_sec: (int_prop(ids.obj_mesh_anim_time_offset, 0) as f32) / 1000.0,
            hold_time_sec: if explicit_hold {
                requested_hold_sec
            } else {
                self.mesh_animation_state.hold_time_sec
            },
            paused: int_prop(ids.obj_mesh_anim_pause, 0) != 0,
            looped: int_prop(ids.obj_mesh_anim_loop, 1) != 0,
            anim_track_no: self.mesh_animation_state.anim_track_no,
            anim_shift_time_sec: requested_shift_sec.max(0.0),
            is_anim: !(int_prop(ids.obj_mesh_anim_pause, 0) != 0),
            prev_clip_name: self.mesh_animation_state.prev_clip_name.clone(),
            prev_clip_index: self.mesh_animation_state.prev_clip_index,
            prev_time_sec: self.mesh_animation_state.prev_time_sec,
            prev_time_offset_sec: self.mesh_animation_state.prev_time_offset_sec,
            prev_rate: self.mesh_animation_state.prev_rate,
            transition_elapsed_sec: self.mesh_animation_state.transition_elapsed_sec,
        };
        self.apply_mesh_animation_state(next, explicit_hold.then_some(requested_hold_sec));
    }

    pub fn uses_mesh_animation_bridge_op(
        ids: &super::constants::RuntimeConstants,
        op: i32,
    ) -> bool {
        [
            ids.obj_mesh_anim_clip,
            ids.obj_mesh_anim_clip_name,
            ids.obj_mesh_anim_rate,
            ids.obj_mesh_anim_time_offset,
            ids.obj_mesh_anim_pause,
            ids.obj_mesh_anim_hold_time,
            ids.obj_mesh_anim_shift_time,
            ids.obj_mesh_anim_loop,
            ids.obj_mesh_anim_blend_clip,
            ids.obj_mesh_anim_blend_clip_name,
            ids.obj_mesh_anim_blend_weight,
        ]
        .into_iter()
        .any(|id| id != 0 && op == id)
    }

    pub fn tick(&mut self, past_game_time: i32, past_real_time: i32) {
        let delta = past_game_time.max(0);
        self.runtime
            .prop_events
            .update_time(past_game_time, past_real_time);
        self.runtime.prop_events.frame();
        self.runtime
            .prop_event_lists
            .update_time(past_game_time, past_real_time);
        self.runtime.prop_event_lists.frame();
        self.frame_action.counter.update_time(past_game_time, past_real_time);
        for fa in &mut self.frame_action_ch {
            fa.counter.update_time(past_game_time, past_real_time);
        }
        for child in &mut self.runtime.child_objects {
            child.tick(past_game_time, past_real_time);
        }
        self.movie.tick(past_game_time, past_real_time);
        self.gan.update_time(past_game_time, past_real_time);
        if matches!(self.object_type, 6 | 7) {
            self.mesh_animation_state.advance_controller_frames(delta);
        }

        if self.object_type == 9 && self.movie.just_finished && !self.movie.auto_free_flag {
            self.movie.pause_flag = true;
        }
    }


    pub fn update_weather_time(
        &mut self,
        past_game_time: i32,
        past_real_time: i32,
        screen_w: i64,
        screen_h: i64,
    ) {
        if self.object_type != 4 || !matches!(self.weather_param.weather_type, 1 | 2) {
            return;
        }
        let cnt = self.weather_param.cnt.max(0) as usize;
        let cnt_max = self.weather_work.cnt_max.min(self.weather_work.sub.len());
        for idx in 0..cnt_max {
            let mut setup_after_sleep = false;
            {
                let sub = &mut self.weather_work.sub[idx];
                if idx >= cnt && sub.state == 0 {
                    continue;
                }
                let past_time = if sub.real_time_flag {
                    past_real_time.max(0) as i64
                } else {
                    past_game_time.max(0) as i64
                };
                sub.state_cur_time = sub.state_cur_time.saturating_add(past_time);
                sub.move_cur_time = sub.move_cur_time.saturating_add(past_time);
                sub.sin_cur_time = sub.sin_cur_time.saturating_add(past_time);

                if (idx >= cnt || sub.restruct_flag)
                    && sub.state == 2
                    && sub.state_time_len - sub.state_cur_time >= 3000
                {
                    sub.state_cur_time = sub.state_time_len.saturating_sub(1500);
                }

                while sub.state_cur_time - sub.state_time_len > 0 {
                    let amari_time = sub.state_cur_time - sub.state_time_len;
                    sub.state = (sub.state + 1) % 4;
                    if sub.state == 0 {
                        if idx >= cnt {
                            break;
                        }
                        setup_after_sleep = true;
                        break;
                    }
                    if sub.state == 1 {
                        sub.move_cur_time = amari_time;
                    }
                    sub.state_time_len = match sub.state {
                        1 => 1000,
                        2 => sub.active_time_len,
                        3 => 1000,
                        _ => sub.state_time_len,
                    };
                    sub.state_cur_time = amari_time;
                }
            }
            if setup_after_sleep {
                self.setup_weather_sub(idx, 1, screen_w, screen_h);
            }
        }
    }

    pub fn any_event_active(&self) -> bool {
        self.runtime.prop_events.any_active() || self.runtime.prop_event_lists.any_active()
    }

    pub fn end_all_events(&mut self) {
        self.runtime.prop_events.end_all();
        self.runtime.prop_event_lists.end_all();
    }

    pub fn event_target(
        &self,
        ids: &super::constants::RuntimeConstants,
        op: i32,
    ) -> ObjectEventTarget {
        if ids.obj_x_eve != 0 && op == ids.obj_x_eve {
            ObjectEventTarget::X
        } else if ids.obj_y_eve != 0 && op == ids.obj_y_eve {
            ObjectEventTarget::Y
        } else if ids.obj_x_rep_eve != 0 && op == ids.obj_x_rep_eve {
            ObjectEventTarget::XRep
        } else if ids.obj_y_rep_eve != 0 && op == ids.obj_y_rep_eve {
            ObjectEventTarget::YRep
        } else if ids.obj_z_rep_eve != 0 && op == ids.obj_z_rep_eve {
            ObjectEventTarget::ZRep
        } else if ids.obj_tr_eve != 0 && op == ids.obj_tr_eve {
            ObjectEventTarget::Tr
        } else if ids.obj_tr_rep_eve != 0 && op == ids.obj_tr_rep_eve {
            ObjectEventTarget::TrRep
        } else if ids.obj_patno_eve != 0 && op == ids.obj_patno_eve {
            ObjectEventTarget::Patno
        } else if ids.obj_z_eve != 0 && op == ids.obj_z_eve {
            ObjectEventTarget::Z
        } else if ids.obj_center_x_eve != 0 && op == ids.obj_center_x_eve {
            ObjectEventTarget::CenterX
        } else if ids.obj_center_y_eve != 0 && op == ids.obj_center_y_eve {
            ObjectEventTarget::CenterY
        } else if ids.obj_center_z_eve != 0 && op == ids.obj_center_z_eve {
            ObjectEventTarget::CenterZ
        } else if ids.obj_center_rep_x_eve != 0 && op == ids.obj_center_rep_x_eve {
            ObjectEventTarget::CenterRepX
        } else if ids.obj_center_rep_y_eve != 0 && op == ids.obj_center_rep_y_eve {
            ObjectEventTarget::CenterRepY
        } else if ids.obj_center_rep_z_eve != 0 && op == ids.obj_center_rep_z_eve {
            ObjectEventTarget::CenterRepZ
        } else if ids.obj_scale_x_eve != 0 && op == ids.obj_scale_x_eve {
            ObjectEventTarget::ScaleX
        } else if ids.obj_scale_y_eve != 0 && op == ids.obj_scale_y_eve {
            ObjectEventTarget::ScaleY
        } else if ids.obj_scale_z_eve != 0 && op == ids.obj_scale_z_eve {
            ObjectEventTarget::ScaleZ
        } else if ids.obj_rotate_x_eve != 0 && op == ids.obj_rotate_x_eve {
            ObjectEventTarget::RotateX
        } else if ids.obj_rotate_y_eve != 0 && op == ids.obj_rotate_y_eve {
            ObjectEventTarget::RotateY
        } else if ids.obj_rotate_z_eve != 0 && op == ids.obj_rotate_z_eve {
            ObjectEventTarget::RotateZ
        } else if ids.obj_clip_left_eve != 0 && op == ids.obj_clip_left_eve {
            ObjectEventTarget::ClipLeft
        } else if ids.obj_clip_top_eve != 0 && op == ids.obj_clip_top_eve {
            ObjectEventTarget::ClipTop
        } else if ids.obj_clip_right_eve != 0 && op == ids.obj_clip_right_eve {
            ObjectEventTarget::ClipRight
        } else if ids.obj_clip_bottom_eve != 0 && op == ids.obj_clip_bottom_eve {
            ObjectEventTarget::ClipBottom
        } else if ids.obj_src_clip_left_eve != 0 && op == ids.obj_src_clip_left_eve {
            ObjectEventTarget::SrcClipLeft
        } else if ids.obj_src_clip_top_eve != 0 && op == ids.obj_src_clip_top_eve {
            ObjectEventTarget::SrcClipTop
        } else if ids.obj_src_clip_right_eve != 0 && op == ids.obj_src_clip_right_eve {
            ObjectEventTarget::SrcClipRight
        } else if ids.obj_src_clip_bottom_eve != 0 && op == ids.obj_src_clip_bottom_eve {
            ObjectEventTarget::SrcClipBottom
        } else if ids.obj_mono_eve != 0 && op == ids.obj_mono_eve {
            ObjectEventTarget::Mono
        } else if ids.obj_reverse_eve != 0 && op == ids.obj_reverse_eve {
            ObjectEventTarget::Reverse
        } else if ids.obj_bright_eve != 0 && op == ids.obj_bright_eve {
            ObjectEventTarget::Bright
        } else if ids.obj_dark_eve != 0 && op == ids.obj_dark_eve {
            ObjectEventTarget::Dark
        } else if ids.obj_color_rate_eve != 0 && op == ids.obj_color_rate_eve {
            ObjectEventTarget::ColorRate
        } else if ids.obj_color_add_r_eve != 0 && op == ids.obj_color_add_r_eve {
            ObjectEventTarget::ColorAddR
        } else if ids.obj_color_add_g_eve != 0 && op == ids.obj_color_add_g_eve {
            ObjectEventTarget::ColorAddG
        } else if ids.obj_color_add_b_eve != 0 && op == ids.obj_color_add_b_eve {
            ObjectEventTarget::ColorAddB
        } else if ids.obj_color_r_eve != 0 && op == ids.obj_color_r_eve {
            ObjectEventTarget::ColorR
        } else if ids.obj_color_g_eve != 0 && op == ids.obj_color_g_eve {
            ObjectEventTarget::ColorG
        } else if ids.obj_color_b_eve != 0 && op == ids.obj_color_b_eve {
            ObjectEventTarget::ColorB
        } else {
            ObjectEventTarget::Unknown
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupListOpKind {
    Alloc,
    Free,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupOpKind {
    Sel,
    SelCancel,
    Init,
    Start,
    StartCancel,
    End,
    GetHitNo,
    GetPushedNo,
    GetDecidedNo,
    GetResult,
    GetResultButtonNo,
    Order,
    Layer,
    CancelPriority,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct GroupState {
    pub wait_flag: bool,
    pub cancel_flag: bool,
    pub cancel_se_no: i64,
    pub started: bool,

    pub hit_button_no: i64,
    pub pushed_button_no: i64,
    pub decided_button_no: i64,
    pub hit_runtime_slot: Option<usize>,
    pub pushed_runtime_slot: Option<usize>,

    pub result: i64,
    pub result_button_no: i64,

    pub order: i64,
    pub layer: i64,
    pub cancel_priority: i64,
    pub props: HashMap<i32, i64>,
    pub aux_str_props: HashMap<i32, String>,
}

impl Default for GroupState {
    fn default() -> Self {
        let mut state = Self {
            wait_flag: false,
            cancel_flag: false,
            cancel_se_no: -1,
            started: false,
            hit_button_no: -1,
            pushed_button_no: -1,
            decided_button_no: TNM_GROUP_NOT_DECIDED,
            hit_runtime_slot: None,
            pushed_runtime_slot: None,
            result: TNM_GROUP_RESULT_NONE,
            result_button_no: 0,
            order: 0,
            layer: 0,
            cancel_priority: 0,
            props: HashMap::new(),
            aux_str_props: HashMap::new(),
        };
        state.reinit();
        state
    }
}

pub const TNM_GROUP_NOT_DECIDED: i64 = -2;
pub const TNM_GROUP_CANCELED: i64 = -1;
pub const TNM_GROUP_RESULT_DECIDED: i64 = 1;
pub const TNM_GROUP_RESULT_NONE: i64 = 0;
pub const TNM_GROUP_RESULT_CANCELLED: i64 = -1;

impl GroupState {
    pub fn reinit(&mut self) {
        self.order = 0;
        self.layer = 0;
        self.cancel_priority = 0;
        self.cancel_se_no = -1;
        self.decided_button_no = TNM_GROUP_NOT_DECIDED;
        self.result = TNM_GROUP_RESULT_NONE;
        self.result_button_no = 0;
        self.started = false;
        self.wait_flag = false;
        self.cancel_flag = false;
        self.hit_button_no = -1;
        self.pushed_button_no = -1;
        self.hit_runtime_slot = None;
        self.pushed_runtime_slot = None;
    }

    pub fn init_sel(&mut self) {
        self.cancel_priority = 0;
        self.cancel_se_no = -1;
        self.decided_button_no = TNM_GROUP_NOT_DECIDED;
        self.result = TNM_GROUP_RESULT_NONE;
        self.result_button_no = 0;
        self.started = false;
        self.wait_flag = false;
        self.cancel_flag = false;
        self.hit_button_no = -1;
        self.pushed_button_no = -1;
        self.hit_runtime_slot = None;
        self.pushed_runtime_slot = None;
    }

    pub fn start(&mut self) {
        self.started = true;
        self.decided_button_no = TNM_GROUP_NOT_DECIDED;
    }

    pub fn end(&mut self) {
        self.started = false;
        self.decided_button_no = TNM_GROUP_NOT_DECIDED;
    }

    pub fn decide(&mut self, button_no: i64) -> bool {
        if !self.started {
            return false;
        }
        self.started = false;
        self.decided_button_no = button_no;
        self.result = TNM_GROUP_RESULT_DECIDED;
        self.result_button_no = button_no;
        self.hit_button_no = -1;
        self.pushed_button_no = -1;
        self.hit_runtime_slot = None;
        self.pushed_runtime_slot = None;
        true
    }

    pub fn cancel(&mut self) -> Option<i64> {
        if !self.started {
            return None;
        }
        let hit_button_no = self.hit_button_no;
        self.started = false;
        self.decided_button_no = TNM_GROUP_CANCELED;
        self.result = TNM_GROUP_RESULT_CANCELLED;
        self.result_button_no = hit_button_no;
        self.hit_button_no = -1;
        self.pushed_button_no = -1;
        self.hit_runtime_slot = None;
        self.pushed_runtime_slot = None;
        Some(hit_button_no)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MwndListOpKind {
    CloseAll,
    CloseAllWait,
    CloseAllNowait,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MwndOpKind {
    MsgBlock,
    Open,
    Close,
    CheckOpen,
    Clear,
    NovelClear,
    /// Append text to the current message buffer.
    Print,
    /// NL: line break without preserving indent.
    NewLineNoIndent,
    /// NLI: line break with indent path preserved.
    NewLineIndent,
    /// Wait for input while in message mode.
    WaitMsg,
    /// PP: wait for text completion, then wait for key.
    Pp,
    /// R: wait for text completion, then clear-ready + key wait.
    R,
    /// PAGE: wait for text completion, then page-clear + key wait.
    PageWait,

    SetName,
    ClearName,
    GetName,
    NextMsg,
    MultiMsg,
    Ruby,
    Koe,
    KoePlayWait,
    KoePlayWaitKey,
    Layer,
    World,
    SetMojiSize,
    SetMojiColor,
    SetIndent,
    ClearIndent,
    StartSlideMsg,
    EndSlideMsg,
    SlideMsg,
    InitOpenAnimeType,
    InitOpenAnimeTime,
    InitCloseAnimeType,
    InitCloseAnimeTime,
    SetOpenAnimeType,
    SetOpenAnimeTime,
    SetCloseAnimeType,
    SetCloseAnimeTime,
    GetOpenAnimeType,
    GetOpenAnimeTime,
    GetCloseAnimeType,
    GetCloseAnimeTime,
    GetDefaultOpenAnimeType,
    GetDefaultOpenAnimeTime,
    GetDefaultCloseAnimeType,
    GetDefaultCloseAnimeTime,
    Sel,
    SelCancel,
    SelMsg,
    SelMsgCancel,

    /// (bool new_line_flag) -> bool
    AddMsgCheck,
    /// (string) -> (string overflow_msg)
    AddMsg,

    InitWakuFile,
    SetWakuFile,
    GetWakuFile,
    InitFilterFile,
    SetFilterFile,
    GetFilterFile,

    ClearFace,
    SetFace,
    SetRepPos,
    MsgBtn,
    InitWindowPos,
    InitWindowSize,
    SetWindowPos,
    SetWindowSize,
    GetWindowPosX,
    GetWindowPosY,
    GetWindowSizeX,
    GetWindowSizeY,
    InitWindowMojiCnt,
    SetWindowMojiCnt,
    GetWindowMojiCntX,
    GetWindowMojiCntY,
    Unknown,
}

#[derive(Debug, Default, Clone)]
pub struct MwndSelectionChoice {
    pub text: String,
    pub kind: i64,
    pub color: i64,
}

#[derive(Debug, Default, Clone)]
pub struct MwndSelectionState {
    pub choices: Vec<MwndSelectionChoice>,
    pub cursor: usize,
    pub cancel_enable: bool,
    pub close_mwnd: bool,
    /// Conservative runtime result: selected entry index (1-based), 0 for none, -1 for cancel.
    pub result: i64,
}

#[derive(Debug, Default, Clone)]
pub struct BtnSelItemState {
    pub object_list: Vec<ObjectState>,
    pub strict: bool,
}

#[derive(Debug, Default, Clone)]
pub struct MwndState {
    pub open: bool,
    pub name_text: String,
    pub msg_text: String,
    pub waku_file: String,
    pub filter_file: String,
    pub face_file: String,
    pub face_no: i64,
    pub rep_pos: Option<(i64, i64)>,
    pub msgbtn: Option<(i64, i64, i64, i64)>,
    pub window_pos: Option<(i64, i64)>,
    pub window_size: Option<(i64, i64)>,
    pub window_moji_cnt: Option<(i64, i64)>,
    pub multi_msg: bool,
    pub ruby_text: Option<String>,
    pub koe: Option<(i64, i64)>,
    pub layer: i64,
    pub world: i64,
    pub moji_size: Option<i64>,
    pub moji_color: Option<i64>,
    pub indent: bool,
    pub slide_msg: bool,
    pub slide_time: i64,
    pub open_anime_type: i64,
    pub open_anime_time: i64,
    pub close_anime_type: i64,
    pub close_anime_time: i64,
    pub selection: Option<MwndSelectionState>,

    pub text_dirty: bool,

    pub button_list: Vec<ObjectState>,
    pub button_list_strict: bool,
    pub face_list: Vec<ObjectState>,
    pub face_list_strict: bool,
    pub object_list: Vec<ObjectState>,
    pub object_list_strict: bool,
    pub props: HashMap<i32, i64>,
    pub aux_str_props: HashMap<i32, String>,
}

#[derive(Debug, Default, Clone)]
pub struct StageFormState {
    /// Group list storage per stage index.
    pub group_lists: HashMap<i64, Vec<GroupState>>,
    /// BTNSELITEM list storage per stage index.
    pub btnselitem_lists: HashMap<i64, Vec<BtnSelItemState>>,
    /// MWND list storage per stage index.
    pub mwnd_lists: HashMap<i64, Vec<MwndState>>,
    /// World list storage per stage index.
    pub world_lists: HashMap<i64, Vec<WorldState>>,
    // --- OBJECT / OBJECTLIST ---
    /// Per-stage object state (string objects, rect objects, nested child objects, etc.).
    pub object_lists: HashMap<i64, Vec<ObjectState>>,
    /// Whether this stage's object list should enforce its current size (enabled after RESIZE).
    pub object_list_strict: HashMap<i64, bool>,
    /// Rectangle-object layer per stage (created lazily).
    pub rect_layers: HashMap<i64, LayerId>,

    /// Stable slot assignment for embedded object lists and nested child objects.
    pub embedded_object_slots: HashMap<String, usize>,
    pub next_embedded_object_slot: HashMap<i64, usize>,
    pub next_nested_object_slot: HashMap<i64, usize>,
}

// -----------------------------------------------------------------------------
// Screen (GLOBAL.SCREEN) state
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct ScreenEffectState {
    pub x: IntEvent,
    pub y: IntEvent,
    pub z: IntEvent,
    pub mono: IntEvent,
    pub reverse: IntEvent,
    pub bright: IntEvent,
    pub dark: IntEvent,
    pub color_r: IntEvent,
    pub color_g: IntEvent,
    pub color_b: IntEvent,
    pub color_rate: IntEvent,
    pub color_add_r: IntEvent,
    pub color_add_g: IntEvent,
    pub color_add_b: IntEvent,
    pub begin_order: i32,
    pub begin_layer: i32,
    pub end_order: i32,
    pub end_layer: i32,
    pub wipe_copy: i32,
    pub wipe_erase: i32,
}

impl Default for ScreenEffectState {
    fn default() -> Self {
        Self {
            x: IntEvent::new(0),
            y: IntEvent::new(0),
            z: IntEvent::new(0),
            mono: IntEvent::new(0),
            reverse: IntEvent::new(0),
            bright: IntEvent::new(0),
            dark: IntEvent::new(0),
            color_r: IntEvent::new(0),
            color_g: IntEvent::new(0),
            color_b: IntEvent::new(0),
            color_rate: IntEvent::new(0),
            color_add_r: IntEvent::new(0),
            color_add_g: IntEvent::new(0),
            color_add_b: IntEvent::new(0),
            begin_order: 0,
            begin_layer: 0,
            end_order: 0,
            end_layer: 0,
            wipe_copy: 0,
            wipe_erase: 0,
        }
    }
}
impl ScreenEffectState {
    pub fn reinit(&mut self) {
        *self = Self::default();
    }

    pub fn tick(&mut self, delta: i32) {
        self.x.tick(delta);
        self.y.tick(delta);
        self.z.tick(delta);
        self.mono.tick(delta);
        self.reverse.tick(delta);
        self.bright.tick(delta);
        self.dark.tick(delta);
        self.color_r.tick(delta);
        self.color_g.tick(delta);
        self.color_b.tick(delta);
        self.color_rate.tick(delta);
        self.color_add_r.tick(delta);
        self.color_add_g.tick(delta);
        self.color_add_b.tick(delta);
    }
}

#[derive(Debug, Clone)]
pub struct ScreenQuakeState {
    pub until: Option<Instant>,
    pub quake_type: i32,
    pub power: i32,
    pub vec: i32,
    pub center_x: i32,
    pub center_y: i32,
    pub begin_order: i32,
    pub end_order: i32,
    pub ending: bool,
}

impl Default for ScreenQuakeState {
    fn default() -> Self {
        Self {
            until: None,
            quake_type: -1,
            power: 0,
            vec: 0,
            center_x: 0,
            center_y: 0,
            begin_order: 0,
            end_order: 0,
            ending: false,
        }
    }
}

impl ScreenQuakeState {
    pub fn reinit(&mut self) {
        *self = Self::default();
    }

    pub fn start_kind(&mut self, quake_type: i32, time_ms: i64) {
        self.ending = false;
        self.quake_type = quake_type;
        let ms = time_ms.max(0) as u64;
        self.until = if ms == 0 {
            None
        } else {
            Some(Instant::now() + Duration::from_millis(ms))
        };
        if ms == 0 {
            self.reinit();
        }
    }

    pub fn end_ms(&mut self, time_ms: i64) {
        self.ending = true;
        let ms = time_ms.max(0) as u64;
        self.until = if ms == 0 {
            None
        } else {
            Some(Instant::now() + Duration::from_millis(ms))
        };
        if ms == 0 {
            self.reinit();
        }
    }

    pub fn check_value(&mut self) -> i32 {
        let _ = self.is_active();
        if self.quake_type < 0 {
            0
        } else if self.ending {
            2
        } else {
            1
        }
    }

    pub fn is_active(&mut self) -> bool {
        if let Some(t) = self.until {
            if Instant::now() >= t {
                self.reinit();
                return false;
            }
        }
        self.quake_type >= 0 && self.until.is_some()
    }

    pub fn remaining_ms(&mut self) -> u64 {
        let Some(t) = self.until else {
            return 0;
        };
        if Instant::now() >= t {
            self.reinit();
            return 0;
        }
        t.duration_since(Instant::now()).as_millis() as u64
    }
}

#[derive(Debug, Default, Clone)]
pub struct ScreenShakeState {
    pub last_value: i64,
    pub until: Option<Instant>,
}

impl ScreenShakeState {
    pub fn set_ms(&mut self, time_ms: i64) {
        self.last_value = time_ms;
        let ms = time_ms.max(0) as u64;
        self.until = if ms == 0 {
            None
        } else {
            Some(Instant::now() + Duration::from_millis(ms))
        };
    }

    pub fn tick(&mut self) {
        if let Some(t) = self.until {
            if Instant::now() >= t {
                self.until = None;
            }
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ScreenFormState {
    pub effect_list: Vec<ScreenEffectState>,
    pub quake_list: Vec<ScreenQuakeState>,
    pub shake: ScreenShakeState,
}

impl ScreenFormState {
    pub fn ensure_effect_len(&mut self, n: usize) {
        if self.effect_list.len() < n {
            self.effect_list
                .extend((0..(n - self.effect_list.len())).map(|_| ScreenEffectState::default()));
        } else if self.effect_list.len() > n {
            self.effect_list.truncate(n);
        }
    }

    pub fn ensure_quake_len(&mut self, n: usize) {
        if self.quake_list.len() < n {
            self.quake_list
                .extend((0..(n - self.quake_list.len())).map(|_| ScreenQuakeState::default()));
        } else if self.quake_list.len() > n {
            self.quake_list.truncate(n);
        }
    }

    pub fn tick(&mut self, delta: i32) {
        for effect in &mut self.effect_list {
            effect.tick(delta);
        }
        for quake in &mut self.quake_list {
            let _ = quake.is_active();
        }
        self.shake.tick();
    }
}

// -----------------------------------------------------------------------------
// Message backlog (GLOBAL.MSGBK) state
// -----------------------------------------------------------------------------

#[derive(Debug, Default, Clone)]
pub struct MsgBackEntry {
    pub pct_flag: bool,
    pub msg_str: String,
    pub original_name: String,
    pub disp_name: String,
    pub pct_pos_x: i32,
    pub pct_pos_y: i32,
    pub koe_no_list: Vec<i64>,
    pub chr_no_list: Vec<i64>,
    pub koe_play_no: i64,
    pub debug_msg: String,
    pub scn_no: i64,
    pub line_no: i64,
    pub save_id_check_flag: bool,
}

#[derive(Debug, Clone)]
pub struct MsgBackState {
    pub history: Vec<MsgBackEntry>,
    pub history_insert_pos: usize,
    pub history_last_pos: usize,
    pub new_msg_flag: bool,
}

impl Default for MsgBackState {
    fn default() -> Self {
        Self {
            history: Vec::new(),
            history_insert_pos: 0,
            history_last_pos: 0,
            new_msg_flag: true,
        }
    }
}

impl MsgBackState {
    fn reset_entry(entry: &mut MsgBackEntry) {
        *entry = MsgBackEntry {
            scn_no: -1,
            line_no: -1,
            ..MsgBackEntry::default()
        };
    }

    fn ready_msg(&mut self) -> &mut MsgBackEntry {
        if self.new_msg_flag {
            if self.history_insert_pos >= self.history.len() {
                self.history.push(MsgBackEntry {
                    scn_no: -1,
                    line_no: -1,
                    ..MsgBackEntry::default()
                });
            } else {
                Self::reset_entry(&mut self.history[self.history_insert_pos]);
            }
            self.new_msg_flag = false;
        }
        &mut self.history[self.history_insert_pos]
    }

    pub fn clear(&mut self) {
        self.history.clear();
        self.history_insert_pos = 0;
        self.history_last_pos = 0;
        self.new_msg_flag = true;
    }

    pub fn next(&mut self) {
        if self.new_msg_flag {
            return;
        }
        let Some(cur) = self.history.get(self.history_insert_pos) else {
            self.new_msg_flag = true;
            return;
        };
        if !cur.pct_flag && cur.msg_str.is_empty() {
            return;
        }
        self.history_insert_pos += 1;
        self.new_msg_flag = true;
    }

    pub fn add_koe(&mut self, koe_no: i64, chara_no: i64, scn_no: i64, line_no: i64) -> bool {
        if koe_no < 0 {
            return true;
        }
        let entry = self.ready_msg();
        entry.koe_no_list.push(koe_no);
        entry.chr_no_list.push(chara_no);
        entry.scn_no = scn_no;
        entry.line_no = line_no;
        self.history_last_pos = self.history_insert_pos;
        true
    }

    pub fn add_name(
        &mut self,
        original_name: &str,
        disp_name: &str,
        scn_no: i64,
        line_no: i64,
    ) -> bool {
        if disp_name.is_empty() {
            return true;
        }
        let entry = self.ready_msg();
        entry.original_name.clear();
        entry.original_name.push_str(original_name);
        entry.disp_name.clear();
        entry.disp_name.push_str(disp_name);
        entry.scn_no = scn_no;
        entry.line_no = line_no;
        self.history_last_pos = self.history_insert_pos;
        true
    }

    pub fn add_msg(&mut self, msg: &str, debug_msg: &str, scn_no: i64, line_no: i64) -> bool {
        if msg.is_empty() {
            return true;
        }
        let entry = self.ready_msg();
        entry.msg_str.push_str(msg);
        entry.debug_msg.clear();
        entry.debug_msg.push_str(debug_msg);
        entry.scn_no = scn_no;
        entry.line_no = line_no;
        self.history_last_pos = self.history_insert_pos;
        true
    }

    pub fn add_pct(&mut self, file_name: &str, x: i32, y: i32) -> bool {
        if file_name.is_empty() {
            return false;
        }
        self.next();
        let entry = self.ready_msg();
        entry.pct_flag = true;
        entry.pct_pos_x = x;
        entry.pct_pos_y = y;
        entry.msg_str.clear();
        entry.msg_str.push_str(file_name);
        self.history_last_pos = self.history_insert_pos;
        self.next();
        true
    }

    pub fn current_entry(&self) -> Option<&MsgBackEntry> {
        self.history.get(self.history_insert_pos)
    }
}

impl StageFormState {
    pub fn ensure_group_list(&mut self, stage_idx: i64, cnt: usize) {
        let entry = self.group_lists.entry(stage_idx).or_default();
        if entry.len() < cnt {
            entry.extend((0..(cnt - entry.len())).map(|_| GroupState::default()));
        } else if entry.len() > cnt {
            entry.truncate(cnt);
        }
    }

    pub fn clear_group_list(&mut self, stage_idx: i64) {
        self.group_lists.insert(stage_idx, Vec::new());
    }

    pub fn ensure_mwnd_list(&mut self, stage_idx: i64, cnt: usize) {
        let entry = self.mwnd_lists.entry(stage_idx).or_default();
        if entry.len() < cnt {
            entry.extend((0..(cnt - entry.len())).map(|_| MwndState::default()));
        } else if entry.len() > cnt {
            entry.truncate(cnt);
        }
    }

    pub fn close_all_mwnd(&mut self, stage_idx: i64) {
        if let Some(list) = self.mwnd_lists.get_mut(&stage_idx) {
            for m in list {
                m.open = false;
            }
        }
    }

    pub fn ensure_object_list(&mut self, stage_idx: i64, cnt: usize) {
        let entry = self.object_lists.entry(stage_idx).or_default();
        if entry.len() < cnt {
            entry.extend((0..(cnt - entry.len())).map(|_| ObjectState::default()));
        } else if entry.len() > cnt {
            entry.truncate(cnt);
        }
    }

    pub fn set_object_list_len_strict(&mut self, stage_idx: i64, cnt: usize) {
        self.ensure_object_list(stage_idx, cnt);
        self.object_list_strict.insert(stage_idx, true);
    }

    pub fn object_list_len(&self, stage_idx: i64) -> usize {
        self.object_lists
            .get(&stage_idx)
            .map(|v| v.len())
            .unwrap_or(0)
    }
}

impl MaskListState {
    pub fn new(mask_cnt: usize) -> Self {
        let mut masks = Vec::with_capacity(mask_cnt);
        for _ in 0..mask_cnt {
            masks.push(MaskState::new());
        }
        Self { masks }
    }

    pub fn ensure_size(&mut self, mask_cnt: usize) {
        if self.masks.len() < mask_cnt {
            self.masks.reserve(mask_cnt - self.masks.len());
            while self.masks.len() < mask_cnt {
                self.masks.push(MaskState::new());
            }
        } else if self.masks.len() > mask_cnt {
            self.masks.truncate(mask_cnt);
        }
    }

    pub fn tick_frame(&mut self, delta: i32) {
        for m in &mut self.masks {
            m.x_event.tick(delta);
            m.y_event.tick(delta);
            for ev in m.script_events.values_mut() {
                ev.tick(delta);
            }
        }
    }
}

impl GlobalState {
    pub fn start_wipe(&mut self, w: WipeState) {
        self.wipe = Some(w);
    }

    pub fn finish_wipe(&mut self) {
        self.wipe = None;
    }

    pub fn wipe_done(&self) -> bool {
        self.wipe.as_ref().map(|w| w.is_done()).unwrap_or(true)
    }

    pub fn tick_frame(&mut self, past_game_time: i32, past_real_time: i32) {
        self.render_frame = self.render_frame.wrapping_add(1);
        if self.wipe_done() {
            self.wipe = None;
        }
        if self.change_display_mode_proc_cnt > 0 {
            self.change_display_mode_proc_cnt -= 1;
        }
        self.mov.tick(past_real_time);

        if !self.script.counter_time_stop_flag {
            let mut counter_ids: Vec<u32> = self.counter_lists.keys().copied().collect();
            counter_ids.sort_unstable();
            for counter_id in counter_ids {
                let Some(counters) = self.counter_lists.get_mut(&counter_id) else {
                    continue;
                };
                for counter in counters {
                    counter.update_time(past_game_time, past_real_time);
                }
            }
        }

        if !self.script.frame_action_time_stop_flag {
            let mut frame_action_ids: Vec<u32> = self.frame_actions.keys().copied().collect();
            frame_action_ids.sort_unstable();
            for frame_action_id in frame_action_ids {
                let Some(fa) = self.frame_actions.get_mut(&frame_action_id) else {
                    continue;
                };
                fa.counter.update_time(past_game_time, past_real_time);
            }
            let mut frame_action_list_ids: Vec<u32> = self.frame_action_lists.keys().copied().collect();
            frame_action_list_ids.sort_unstable();
            for frame_action_list_id in frame_action_list_ids {
                let Some(list) = self.frame_action_lists.get_mut(&frame_action_list_id) else {
                    continue;
                };
                for fa in list {
                    fa.counter.update_time(past_game_time, past_real_time);
                }
            }
        }

        let mut mask_list_ids: Vec<u32> = self.mask_lists.keys().copied().collect();
        mask_list_ids.sort_unstable();
        for mask_list_id in mask_list_ids {
            let Some(ml) = self.mask_lists.get_mut(&mask_list_id) else {
                continue;
            };
            ml.tick_frame(past_game_time.max(0));
        }

        let mut screen_form_ids: Vec<u32> = self.screen_forms.keys().copied().collect();
        screen_form_ids.sort_unstable();
        for screen_form_id in screen_form_ids {
            let Some(sc) = self.screen_forms.get_mut(&screen_form_id) else {
                continue;
            };
            sc.tick(past_game_time.max(0));
        }

        let mut int_event_root_ids: Vec<u32> = self.int_event_roots.keys().copied().collect();
        int_event_root_ids.sort_unstable();
        for int_event_root_id in int_event_root_ids {
            let Some(ev) = self.int_event_roots.get_mut(&int_event_root_id) else {
                continue;
            };
            ev.update_time(past_game_time, past_real_time);
            ev.frame();
        }
        let mut int_event_list_ids: Vec<u32> = self.int_event_lists.keys().copied().collect();
        int_event_list_ids.sort_unstable();
        for int_event_list_id in int_event_list_ids {
            let Some(events) = self.int_event_lists.get_mut(&int_event_list_id) else {
                continue;
            };
            for ev in events {
                ev.update_time(past_game_time, past_real_time);
                ev.frame();
            }
        }

        let mut stage_form_ids: Vec<u32> = self.stage_forms.keys().copied().collect();
        stage_form_ids.sort_unstable();
        for stage_form_id in stage_form_ids {
            let Some(st) = self.stage_forms.get_mut(&stage_form_id) else {
                continue;
            };
            let mut object_stage_ids: Vec<i64> = st.object_lists.keys().copied().collect();
            object_stage_ids.sort_unstable();
            for object_stage_id in object_stage_ids {
                let Some(objs) = st.object_lists.get_mut(&object_stage_id) else {
                    continue;
                };
                for obj in objs {
                    obj.tick(past_game_time, past_real_time);
                }
            }
            let mut world_stage_ids: Vec<i64> = st.world_lists.keys().copied().collect();
            world_stage_ids.sort_unstable();
            for world_stage_id in world_stage_ids {
                let Some(worlds) = st.world_lists.get_mut(&world_stage_id) else {
                    continue;
                };
                for w in worlds {
                    w.update_time(past_game_time, past_real_time);
                    w.frame();
                }
            }
        }
    }
}
