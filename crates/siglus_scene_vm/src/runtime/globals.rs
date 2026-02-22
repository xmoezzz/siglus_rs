use std::collections::{HashMap, HashSet};

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

    started_at: Instant,
    end_at: Instant,
}

impl WipeState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        mask_file: Option<String>,
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
            started_at,
            end_at,
        }
    }

    pub fn is_done(&self) -> bool {
        Instant::now() >= self.end_at
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

/// Global mutable state used by various "global element" (form) handlers.
///
/// This crate keeps these structures generic on purpose: many Siglus
/// "global elements" are simple lists, counters, etc.
#[derive(Debug, Clone)]
pub struct GlobalState {
    /// Generic int-list storage keyed by the global form ID.
    pub int_lists: HashMap<u32, Vec<i64>>,
    /// Generic string-list storage keyed by the global form ID.
    pub str_lists: HashMap<u32, Vec<String>>,
    /// Counter-list storage keyed by the global form ID.
    pub counter_lists: HashMap<u32, Vec<Counter>>,

    /// Generic int properties keyed by (form_id -> op_id).
    pub int_props: HashMap<u32, HashMap<i32, i64>>,
    /// Generic string properties keyed by (form_id -> op_id).
    pub str_props: HashMap<u32, HashMap<i32, String>>,

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
    /// Auto-detected mask form id when no explicit id-map entry exists.
    pub guessed_mask_form_id: Option<u32>,

    /// EditBox subsystem state keyed by the (guessed or mapped) form id.
    pub editbox_lists: HashMap<u32, EditBoxListState>,
    /// Auto-detected editbox form id when no explicit id-map entry exists.
    pub guessed_editbox_form_id: Option<u32>,
    /// Currently focused editbox (form_id, index).
    pub focused_editbox: Option<(u32, usize)>,

    /// Stage UI subsystem state keyed by the stage form ID.
    pub stage_forms: HashMap<u32, StageFormState>,
    /// Auto-detected stage form id when no explicit id-map entry exists.
    pub guessed_stage_form_id: Option<u32>,
    /// Currently focused stage group selection (form_id, stage_idx, group_idx).
    pub focused_stage_group: Option<(u32, i64, usize)>,

    /// Screen subsystem state keyed by the screen form ID.
    pub screen_forms: HashMap<u32, ScreenFormState>,

    /// Message backlog (MSGBK) subsystem state keyed by the form ID.
    pub msgbk_forms: HashMap<u32, MsgBackState>,

    /// Active wipe transition (WIPE / MASK_WIPE).
    pub wipe: Option<WipeState>,
}

impl Default for GlobalState {
    fn default() -> Self {
        Self {
            int_lists: HashMap::new(),
            str_lists: HashMap::new(),
            counter_lists: HashMap::new(),
            int_props: HashMap::new(),
            str_props: HashMap::new(),
            cg_table_off: false,
            database_off: false,
            g00buf: Vec::new(),
            rng_state: 0,
            mask_lists: HashMap::new(),
            guessed_mask_form_id: None,

            editbox_lists: HashMap::new(),
            guessed_editbox_form_id: None,
            focused_editbox: None,

            stage_forms: HashMap::new(),
            guessed_stage_form_id: None,
            focused_stage_group: None,

            screen_forms: HashMap::new(),
            msgbk_forms: HashMap::new(),

            wipe: None,
        }
    }
}

/// A minimal counter implementation.
///
/// The original engine supports multiple timing modes (game/real/frame), looping,
/// and a sizable command surface. For bring-up, we model a monotonic millisecond
/// counter that can be started/stopped/resumed and set directly.
#[derive(Debug, Clone, Copy)]
pub struct Counter {
    base_ms: i64,
    start: Option<Instant>,
}

impl Default for Counter {
    fn default() -> Self {
        Self {
            base_ms: 0,
            start: None,
        }
    }
}

impl Counter {
    pub fn reset(&mut self) {
        self.base_ms = 0;
        self.start = None;
    }

    pub fn set_count(&mut self, count_ms: i64) {
        self.base_ms = count_ms;
        self.start = None;
    }

    pub fn start(&mut self) {
        if self.start.is_none() {
            self.start = Some(Instant::now());
        }
    }

    pub fn stop(&mut self) {
        if let Some(s) = self.start.take() {
            self.base_ms += Instant::now().duration_since(s).as_millis() as i64;
        }
    }

    pub fn resume(&mut self) {
        // In the original engine, resume continues from the current stored value.
        // Our bring-up model matches that behavior.
        self.start = Some(Instant::now());
    }

    pub fn get_count(&self) -> i64 {
        match self.start {
            Some(s) => self.base_ms + Instant::now().duration_since(s).as_millis() as i64,
            None => self.base_ms,
        }
    }

    pub fn is_running(&self) -> bool {
        self.start.is_some()
    }
}


/// Mask state (bring-up level).
///
/// The original engine backs masks with a D3D PCT album; here we only track
/// the script-visible fields and integer events.
#[derive(Debug, Clone)]
pub struct MaskState {
    pub name: Option<String>,
    pub x_event: IntEvent,
    pub y_event: IntEvent,
    pub extra_int: HashMap<i32, i32>,
    pub extra_events: HashMap<i32, IntEvent>,
}

impl MaskState {
    pub fn new() -> Self {
        Self {
            name: None,
            x_event: IntEvent::new(0),
            y_event: IntEvent::new(0),
            extra_int: HashMap::new(),
            extra_events: HashMap::new(),
        }
    }

    pub fn reinit(&mut self) {
        self.name = None;
        self.x_event.reinit();
        self.y_event.reinit();
        self.extra_int.clear();
        self.extra_events.clear();
    }
}

#[derive(Debug, Clone)]
pub struct MaskListState {
    pub masks: Vec<MaskState>,
    /// First seen property op -> X, second -> Y.
    pub x_op: Option<i32>,
    pub y_op: Option<i32>,
    /// First seen event-op -> X_EVE, second -> Y_EVE.
    pub x_eve_op: Option<i32>,
    pub y_eve_op: Option<i32>,
    pub confirmed: bool,
}

/// EditBox state (bring-up level).
///
/// We only model the minimum script-visible behavior needed to avoid deadlocks:
/// - text content
/// - decided/canceled edges (driven by Enter/Esc when focused)
#[derive(Debug, Clone)]
pub struct EditBoxState {
    pub alive: bool,
    pub text: String,
    pub decided: bool,
    pub canceled: bool,
}

impl Default for EditBoxState {
    fn default() -> Self {
        Self {
            alive: false,
            text: String::new(),
            decided: false,
            canceled: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EditBoxOpKind {
    Create,
    Destroy,
    SetText,
    GetText,
    SetFocus,
    ClearInput,
    CheckDecided,
    CheckCanceled,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct EditBoxListState {
    pub boxes: Vec<EditBoxState>,
    /// Auto-learned mapping: op-id -> semantic kind.
    pub op_map: HashMap<i32, EditBoxOpKind>,
    pub confirmed: bool,
}

impl EditBoxListState {
    pub fn new(cnt: usize) -> Self {
        Self {
            boxes: vec![EditBoxState::default(); cnt],
            op_map: HashMap::new(),
            confirmed: false,
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

    pub fn has_kind(&self, k: EditBoxOpKind) -> bool {
        self.op_map.values().any(|&v| v == k)
    }
}



// -----------------------------------------------------------------------------
// Stage/MWND/Group state
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageChildKind {
    ObjectList,
    GroupList,
    MwndList,
    Unknown,
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
    /// NUMBER object backend: a fixed sprite list (16) with per-digit sprites.
    Number {
        layer_id: LayerId,
        sprite_ids: Vec<SpriteId>,
    },
}

impl Default for ObjectBackend {
    fn default() -> Self {
        Self::None
    }
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
}

impl Default for ObjectButtonState {
    fn default() -> Self {
        Self {
            enabled: false,
            button_no: 0,
            group_no: 0,
            group_idx_override: None,
            action_no: 0,
            se_no: 0,
            push_keep: false,
            alpha_test: false,
            state: 0,
            hit: false,
            pushed: false,
            decided_action_scn_name: String::new(),
            decided_action_cmd_name: String::new(),
            decided_action_z_no: -1,
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

#[derive(Debug, Clone)]
pub struct ObjectMovieState {
    pub loop_flag: bool,
    pub auto_free_flag: bool,
    pub real_time_flag: bool,
    pub pause_flag: bool,

    /// Current playback timer in milliseconds (C++: m_omv_timer).
    pub timer_ms: u64,
    /// Total movie time in milliseconds if known.
    pub total_ms: Option<u64>,

    pub playing: bool,
    pub last_tick: Option<std::time::Instant>,
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
    }

    pub fn tick(&mut self) {
        if !self.playing || self.pause_flag {
            self.last_tick = Some(std::time::Instant::now());
            return;
        }
        let now = std::time::Instant::now();
        let last = self.last_tick.unwrap_or(now);
        let dt = now.saturating_duration_since(last);
        self.last_tick = Some(now);
        let add = dt.as_millis() as u64;
        if add == 0 {
            return;
        }
        self.timer_ms = self.timer_ms.saturating_add(add);
        if let Some(total) = self.total_ms {
            if total > 0 && self.timer_ms >= total {
                if self.loop_flag {
                    self.timer_ms %= total;
                } else {
                    self.playing = false;
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

#[derive(Debug, Default, Clone)]
pub struct ObjectCompatState {
    pub used: bool,
    pub backend: ObjectBackend,
    pub file_name: Option<String>,
    pub string_value: Option<String>,

    /// TNM_OBJECT_TYPE_* (0=none, 1=rect, 2=pct, 3=string, 4=weather, 5=number, ...).
    pub object_type: i64,

    /// For NUMBER objects (and compatibility), stores the current number value.
    pub number_value: i64,

    /// For STRING objects.
    pub string_param: ObjectStringParam,

    /// For NUMBER objects.
    pub number_param: ObjectNumberParam,

    /// For WEATHER objects (type A/B).
    pub weather_param: ObjectWeatherParam,

    /// For SAVE_THUMB / THUMB objects.
    pub thumb_save_no: i64,

    /// For MOVIE objects.
    pub movie: ObjectMovieState,

    /// For E-mote objects.
    pub emote: ObjectEmoteParam,

    /// Best-effort: last loaded GAN file.
    pub gan_file: Option<String>,

    pub button: ObjectButtonState,

    /// Last seen integer argument vector for command-like ops keyed by op.
    /// This is a compatibility cache for bring-up when numeric IDs are not fully known.
    pub cmd_int_args: HashMap<i32, Vec<i64>>,

    pub extra_int_props: HashMap<i32, i64>,
    pub extra_str_props: HashMap<i32, String>,

    // Misc compatibility storage
    pub extra_events: HashMap<i32, IntEvent>,
    pub rep_int_lists: HashMap<i32, Vec<i64>>,
    pub rep_int_event_lists: HashMap<i32, Vec<IntEvent>>,
}

impl ObjectCompatState {
    /// Reset type-specific parameters (approximation of C++ C_elm_object::init_type(true)).
    ///
    /// Important: this does NOT clear button/groups/events (those are part of init_param/reinit in C++).
    pub fn init_type_like(&mut self) {
        self.backend = ObjectBackend::None;
        self.file_name = None;
        self.string_value = None;
        self.object_type = 0;

        self.number_value = 0;
        self.string_param = ObjectStringParam::default();
        self.number_param = ObjectNumberParam::default();
        self.weather_param = ObjectWeatherParam::default();
        self.thumb_save_no = -1;

        self.movie.reset();
        self.emote = ObjectEmoteParam::default();

        self.gan_file = None;
    }

    pub fn tick(&mut self, delta: i32) {
        for ev in self.extra_events.values_mut() {
            ev.tick(delta);
        }
        for list in self.rep_int_event_lists.values_mut() {
            for ev in list {
                ev.tick(delta);
            }
        }
        self.movie.tick();

        // When a MOVIE ends, auto_free resets type; otherwise it stays paused.
        if self.object_type == 9 && !self.movie.playing {
            if self.movie.auto_free_flag {
                self.init_type_like();
            } else {
                self.movie.pause_flag = true;
            }
        }
    }

    pub fn any_event_active(&self) -> bool {
        if self.extra_events.values().any(|e| e.check_event()) {
            return true;
        }
        for list in self.rep_int_event_lists.values() {
            if list.iter().any(|e| e.check_event()) {
                return true;
            }
        }
        false
    }

    pub fn end_all_events(&mut self) {
        for ev in self.extra_events.values_mut() {
            ev.end_event();
        }
        for list in self.rep_int_event_lists.values_mut() {
            for ev in list {
                ev.end_event();
            }
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

#[derive(Debug, Default, Clone)]
pub struct GroupState {
    pub wait_flag: bool,
    pub cancel_flag: bool,
    pub cancel_se_no: i64,
    pub started: bool,

    pub hit_button_no: i64,
    pub pushed_button_no: i64,
    pub decided_button_no: i64,

    pub result: i64,
    pub result_button_no: i64,

    pub order: i64,
    pub layer: i64,
    pub cancel_priority: i64,
}

impl GroupState {
    pub fn init_sel(&mut self) {
        self.hit_button_no = -1;
        self.pushed_button_no = -1;
        self.decided_button_no = -1;
        self.result = 0;
        self.result_button_no = -1;
        self.cancel_flag = false;
        self.cancel_se_no = -1;
    }

    pub fn start(&mut self) {
        self.started = true;
    }

    pub fn end(&mut self) {
        self.wait_flag = false;
        self.started = false;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MwndListOpKind {
    CloseAll,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MwndOpKind {
    Open,
    Close,
    CheckOpen,
    Clear,
    /// Append text to the current message buffer.
    Print,
    /// Insert a line break into the message buffer.
    NewLine,
    /// Wait for input while in message mode.
    WaitMsg,
    /// Page-break wait: wait for input and clear the message afterwards.
    PageWait,

    SetName,
    ClearName,
    GetName,

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
    Unknown,
}

#[derive(Debug, Default, Clone)]
pub struct MwndState {
    pub open: bool,
    pub waku_file: String,
    pub filter_file: String,

    /// Whether any text-related op (PRINT/ADD_MSG/NL) has been observed since the last wait/clear.
    ///
    /// This is used only for conservative bring-up heuristics to avoid mis-classifying
    /// proceed-like ops (PP/R/PAGE/WAIT) as structural ops (OPEN/CLOSE/CLEAR).
    pub text_dirty: bool,

    /// Unknown integer properties (best-effort storage).
    pub props: HashMap<i32, i64>,
}

#[derive(Debug, Default, Clone)]
pub struct StageFormState {
    /// Auto-learned mapping: stage child element code -> child kind.
    pub child_kind: HashMap<i32, StageChildKind>,

    /// Group list storage per stage index.
    pub group_lists: HashMap<i64, Vec<GroupState>>,
    /// Auto-learned mapping: group-list op-id -> semantic kind.
    pub group_list_op_map: HashMap<i32, GroupListOpKind>,
    /// Auto-learned mapping: group op-id -> semantic kind.
    pub group_op_map: HashMap<i32, GroupOpKind>,

    /// MWND list storage per stage index.
    pub mwnd_lists: HashMap<i64, Vec<MwndState>>,
    /// Auto-learned mapping: mwnd-list op-id -> semantic kind.
    pub mwnd_list_op_map: HashMap<i32, MwndListOpKind>,
    /// Auto-learned mapping: mwnd op-id -> semantic kind.
    pub mwnd_op_map: HashMap<i32, MwndOpKind>,

    // --- OBJECT / OBJECTLIST ---

    /// Per-stage object compat state (used for property fallback, string objects, rect objects, etc.).
    pub object_lists: HashMap<i64, Vec<ObjectCompatState>>,
    /// Whether this stage's object list should enforce its current size (enabled after RESIZE).
    pub object_list_strict: HashMap<i64, bool>,
    /// Rectangle-object layer per stage (created lazily).
    pub rect_layers: HashMap<i64, LayerId>,

    /// Learned OBJECTLIST op ids (GET_SIZE / RESIZE).
    pub object_list_op_map: HashMap<i32, ObjectListOpKind>,
    /// Learned OBJECT op ids (INIT/FREE/CREATE/etc.).
    pub object_op_map: HashMap<i32, ObjectOpKind>,
    /// Ops that have been observed to return or accept strings.
    pub object_str_ops: HashSet<i32>,

}

// -----------------------------------------------------------------------------
// Screen (GLOBAL.SCREEN) bring-up state
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenSelectorKind {
    /// Selector that behaves like a list (e.g., EFFECTLIST / QUAKELIST).
    List,
    /// Selector that behaves like an integer event (e.g., X_EVE).
    Event,
    /// Selector that behaves like a plain property (get/set).
    Prop,
    /// Selector used as a side-effect command (e.g., SHAKE).
    Command,
    Unknown,
}

#[derive(Debug, Default, Clone)]
pub struct ScreenItemState {
    pub props: HashMap<i32, i64>,
    pub events: HashMap<i32, IntEvent>,

    /// Confirmed event-op codes in first-seen order.
    pub confirmed_event_ops: Vec<i32>,
    /// Heuristic aliasing: property-op -> event-op.
    pub prop_to_event: HashMap<i32, i32>,

    // --- Quake ---
    //
    // The original engine's QUAKE element supports START/END/WAIT/CHECK.
    // We do not render actual quake transforms yet, but we must preserve
    // script-visible control flow (wait/check loops must not deadlock).
    //
    // We model quake activity as a wall-clock deadline.
    pub quake_until: Option<Instant>,
}

impl ScreenItemState {
    pub fn reinit(&mut self) {
        self.props.clear();
        self.events.clear();
        self.confirmed_event_ops.clear();
        self.prop_to_event.clear();
        self.quake_until = None;
    }

    pub fn quake_start_ms(&mut self, time_ms: i64) {
        let ms = time_ms.max(0) as u64;
        if ms == 0 {
            self.quake_until = None;
        } else {
            self.quake_until = Some(Instant::now() + Duration::from_millis(ms));
        }
    }

    pub fn quake_end_ms(&mut self, time_ms: i64) {
        // The original engine supports an optional end fade time.
        // Bring-up: treat it as an additional active interval.
        self.quake_start_ms(time_ms);
    }

    pub fn quake_is_active(&mut self) -> bool {
        if let Some(t) = self.quake_until {
            if Instant::now() >= t {
                self.quake_until = None;
            }
        }
        self.quake_until.is_some()
    }

    pub fn quake_remaining_ms(&mut self) -> u64 {
        let Some(t) = self.quake_until else { return 0; };
        if Instant::now() >= t {
            self.quake_until = None;
            return 0;
        }
        t.duration_since(Instant::now()).as_millis() as u64
    }

    pub fn tick(&mut self, delta: i32) {
        for ev in self.events.values_mut() {
            ev.tick(delta);
        }
        // Keep quake state fresh even if the script never calls CHECK.
        let _ = self.quake_is_active();
    }
}

#[derive(Debug, Default, Clone)]
pub struct ScreenListState {
    pub items: Vec<ScreenItemState>,

    /// Learned list-level op code for GET_SIZE (chain: [FORM, selector, op]).
    pub get_size_op: Option<i32>,
    /// Learned list-level op code for RESIZE (chain: [FORM, selector, op]).
    pub resize_op: Option<i32>,
}

impl ScreenListState {
    pub fn ensure_size(&mut self, n: usize) {
        if self.items.len() < n {
            self.items
                .extend((0..(n - self.items.len())).map(|_| ScreenItemState::default()));
        } else if self.items.len() > n {
            self.items.truncate(n);
        }
    }

    pub fn tick(&mut self, delta: i32) {
        for it in &mut self.items {
            it.tick(delta);
        }
    }
}

#[derive(Debug, Default, Clone)]
pub struct ScreenFormState {
    /// Selector kind cache (chain[1] -> kind).
    pub selector_kind: HashMap<i32, ScreenSelectorKind>,

    /// Selectors that have been observed to behave like QUAKE lists.
    ///
    /// We do not rely on numeric element-code tables during bring-up, so we
    /// learn this role the first time we observe a quake START-like call.
    pub quake_selectors: HashSet<i32>,

    /// List states keyed by selector op code.
    pub lists: HashMap<i32, ScreenListState>,

    /// Root-level properties and events (direct SCREEN.X style access).
    pub root_props: HashMap<i32, i64>,
    pub root_events: HashMap<i32, IntEvent>,

    pub root_confirmed_event_ops: Vec<i32>,
    pub root_prop_to_event: HashMap<i32, i32>,

    /// Last seen shake argument (debugging only).
    pub last_shake: i64,
}

impl ScreenFormState {
    pub fn tick(&mut self, delta: i32) {
        for ev in self.root_events.values_mut() {
            ev.tick(delta);
        }
        for list in self.lists.values_mut() {
            list.tick(delta);
        }
    }
}

// -----------------------------------------------------------------------------
// Message backlog (GLOBAL.MSGBK) bring-up state
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub enum MsgBackAtom {
    Text(String),
    Name(String),
    Koe { koe_no: i64, chara_no: i64 },
}

#[derive(Debug, Default, Clone)]
pub struct MsgBackEntry {
    pub atoms: Vec<MsgBackAtom>,
}

#[derive(Debug, Default, Clone)]
pub struct MsgBackState {
    pub history: Vec<MsgBackEntry>,
    pub cur: MsgBackEntry,
}

impl MsgBackState {
    pub fn clear(&mut self) {
        self.history.clear();
        self.cur.atoms.clear();
    }

    pub fn next(&mut self) {
        if !self.cur.atoms.is_empty() {
            self.history.push(std::mem::take(&mut self.cur));
        }
        self.cur = MsgBackEntry::default();
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

    pub fn has_group_op(&self, k: GroupOpKind) -> bool {
        self.group_op_map.values().any(|&v| v == k)
    }

    pub fn has_mwnd_op(&self, k: MwndOpKind) -> bool {
        self.mwnd_op_map.values().any(|&v| v == k)
    }

    pub fn ensure_object_list(&mut self, stage_idx: i64, cnt: usize) {
        let entry = self.object_lists.entry(stage_idx).or_default();
        if entry.len() < cnt {
            entry.extend((0..(cnt - entry.len())).map(|_| ObjectCompatState::default()));
        } else if entry.len() > cnt {
            entry.truncate(cnt);
        }
    }

    pub fn set_object_list_len_strict(&mut self, stage_idx: i64, cnt: usize) {
        self.ensure_object_list(stage_idx, cnt);
        self.object_list_strict.insert(stage_idx, true);
    }

    pub fn object_list_len(&self, stage_idx: i64) -> usize {
        self.object_lists.get(&stage_idx).map(|v| v.len()).unwrap_or(0)
    }

}

impl MaskListState {
    pub fn new(mask_cnt: usize) -> Self {
        let mut masks = Vec::with_capacity(mask_cnt);
        for _ in 0..mask_cnt {
            masks.push(MaskState::new());
        }
        Self {
            masks,
            x_op: None,
            y_op: None,
            x_eve_op: None,
            y_eve_op: None,
            confirmed: false,
        }
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
            for ev in m.extra_events.values_mut() {
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

    pub fn tick_frame(&mut self) {
        if self.wipe_done() {
            self.wipe = None;
        }

        for ml in self.mask_lists.values_mut() {
            ml.tick_frame(1);
        }

        for sc in self.screen_forms.values_mut() {
            sc.tick(1);
        }

        for st in self.stage_forms.values_mut() {
            for objs in st.object_lists.values_mut() {
                for obj in objs {
                    obj.tick(1);
                }
            }
        }
    }
}
