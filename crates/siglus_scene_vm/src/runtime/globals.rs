use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::assets::RgbaImage;
use crate::env::trace_env;
use crate::platform_time::{Duration, Instant};
use crate::runtime::gan::GanState;
use crate::runtime::int_event::IntEvent;

use crate::image_manager::ImageId;
use crate::layer::{LayerId, SpriteId};

/// Screen wipe transition state.
///
/// This models the timing and script-visible behavior of the original `Gp_wipe`
/// subsystem. Rendering is handled elsewhere; here we only track parameters and
/// completion.
#[derive(Debug, Clone)]
pub struct WipeState {
    /// Stage-form namespace whose NEXT stage was populated for this wipe.
    ///
    /// Normal scene stages use `FORM_GLOBAL_STAGE`; EXCALL stages use the
    /// corresponding local namespace (`FORM_GLOBAL_STAGE ^ 0x4000`).  The
    /// original `C_tnm_wipe::end()` reinitializes only the NEXT stage that
    /// belongs to the active wipe range, so the Rust runtime must retain this
    /// target until the wipe is ended.
    pub stage_form_id: u32,
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

    /// C++ mask generators consume the process RNG once at wipe creation.
    pub random_seed: u32,

    /// Original C_tnm_wipe frame state.  Time is advanced explicitly from
    /// `local_wipe_time_past`; wall-clock time must never advance a paused wipe.
    step: i32,
    cur_time_ms: i64,
    progress_value: f32,
    done: bool,
}

impl WipeState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        stage_form_id: u32,
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
        static NEXT_WIPE_SEED: AtomicU32 = AtomicU32::new(0x6d2b_79f5);
        let seq = NEXT_WIPE_SEED.fetch_add(0x9e37_79b9, Ordering::Relaxed);
        let random_seed = seq
            ^ (wipe_type as u32).rotate_left(7)
            ^ (wipe_time_ms as u32).rotate_left(17)
            ^ (start_time_ms as u32).rotate_left(25);

        Self {
            stage_form_id,
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
            random_seed,
            step: 0,
            cur_time_ms: 0,
            progress_value: 0.0,
            done: false,
        }
    }

    /// Mirrors C_tnm_wipe::update_time() followed by C_tnm_wipe::frame().
    /// The first two frame steps intentionally discard accumulated time before
    /// installing `start_time_ms`, preventing a delayed first presentation from
    /// making the wipe jump forward.
    pub fn advance(&mut self, past_time_ms: i32) {
        if self.done {
            return;
        }
        self.cur_time_ms = self.cur_time_ms.saturating_add(past_time_ms.max(0) as i64);

        if self.step == 0 {
            self.step = 1;
            return;
        }
        if self.step == 1 {
            self.cur_time_ms = self.start_time_ms as i64;
            self.step = 2;
        }

        let end = self.wipe_time_ms as f64;
        let cur = self.cur_time_ms as f64;
        let raw = if self.wipe_time_ms <= 0 {
            1.0
        } else {
            match self.speed_mode {
                1 => (cur * cur) / (end * end),
                2 => 1.0 - ((cur - end) * (cur - end)) / (end * end),
                _ => cur / end,
            }
        };
        self.progress_value = raw.clamp(0.0, 1.0) as f32;
        if self.cur_time_ms >= self.wipe_time_ms as i64 {
            self.progress_value = 1.0;
            self.done = true;
        }
    }

    pub fn is_done(&self) -> bool {
        self.done
    }

    pub fn progress(&self) -> f32 {
        self.progress_value
    }

    #[allow(dead_code)]
    pub fn remaining_ms(&self) -> u64 {
        if self.done {
            0
        } else {
            (self.wipe_time_ms as i64 - self.cur_time_ms).max(0) as u64
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScriptRuntimeState {
    // C_tnm_local_data_pod fields which are not exposed by the SCRIPT form but
    // still participate in original local-save compatibility.
    pub cur_koe_no: i64,
    pub cur_chr_no: i64,
    pub cur_read_flag_scn_no: i64,
    pub cur_read_flag_flag_no: i64,
    pub msg_back_save_cntr: i64,
    pub multi_msg_mode: bool,

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
    pub cursor_runtime_visible: bool,
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

    // SiglusEngine 1.1.141.2 joypad-mode override.  -1 means use the
    // Gameexe configuration, 0 forces joypad mode off, and 1 forces it allowed.
    pub joypad_mode_override: i64,

    pub font_name: String,
    pub font_bold: i64,
    pub font_shadow: i64,
}

impl Default for ScriptRuntimeState {
    fn default() -> Self {
        Self {
            cur_koe_no: -1,
            cur_chr_no: -1,
            cur_read_flag_scn_no: -1,
            cur_read_flag_flag_no: -1,
            msg_back_save_cntr: 0,
            multi_msg_mode: false,
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
            cursor_runtime_visible: true,
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
            cursor_no: -1,
            time_stop_flag: false,
            counter_time_stop_flag: false,
            frame_action_time_stop_flag: false,
            stage_time_stop_flag: false,
            joypad_mode_override: -1,
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
    pub request_id: u64,
    pub kind: i32,
    pub text: String,
    pub debug_only: bool,
    pub buttons: Vec<SystemMessageBoxButton>,
    pub cursor: usize,
    pub native_pending: bool,
    pub complete_wait_with_value: bool,
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
    pub messagebox_modal_result: Option<i64>,
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
            messagebox_modal_result: None,
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
    EndGame,
    ReturnToSel,
    ReturnToMenu,
    Save,
    Load,
    QuickSave,
    QuickLoad,
    BacklogLoad,
    MsgBack,
    OpenSyscomMenu,
    OpenSave,
    OpenLoad,
    OpenConfig,
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

/// Cross-platform replacement for the native Syscom dialogs used by the
/// original Windows build when a project does not provide SAVE_SCENE,
/// LOAD_SCENE, CONFIG_SCENE, or CANCEL_SCENE.  The dialog itself is rendered
/// by the engine and receives input through the normal winit path, so the same
/// state machine is used on desktop, mobile, and WebAssembly hosts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyscomFallbackDialogKind {
    SystemMenu,
    SaveMenu,
    LoadMenu,
    ConfigRoot,
    ConfigWindow,
    ConfigVolumeRoot,
    ConfigVolume(usize),
    ConfigMessage,
    ConfigAuto,
    ConfigFont,
    ConfigOther,
    Notice,
}

#[derive(Debug, Clone)]
pub struct SyscomFallbackDialogState {
    pub kind: SyscomFallbackDialogKind,
    pub page: usize,
    /// A modal result belongs to this state only after `awaiting_result` is
    /// set.  This prevents an unrelated SYSTEM.MESSAGEBOX result from being
    /// consumed by the Syscom fallback state machine.
    pub awaiting_result: bool,
    pub return_kind: Option<SyscomFallbackDialogKind>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigChrKoeState {
    pub onoff: bool,
    pub volume: i64,
}

impl Default for ConfigChrKoeState {
    fn default() -> Self {
        Self {
            onoff: true,
            volume: 255,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OriginalConfigRuntimeState {
    pub screen_size_mode: i64,
    pub screen_size_mode_window: i64,
    pub screen_size_scale: (i64, i64),
    pub screen_size_free: (i64, i64),
    pub fullscreen_change_resolution: bool,
    pub fullscreen_display_cnt: i64,
    pub fullscreen_display_no: i64,
    pub fullscreen_resolution_cnt: i64,
    pub fullscreen_resolution_no: i64,
    pub fullscreen_resolution: (i64, i64),
    pub fullscreen_mode: i64,
    pub fullscreen_scale: (i64, i64),
    pub fullscreen_scale_sync_switch: bool,
    pub fullscreen_move: (i64, i64),
    pub all_sound_user_volume: i64,
    pub sound_user_volume: [i64; 32],
    pub play_all_sound_check: bool,
    pub play_sound_check: [bool; 32],
    pub bgmfade_volume: i64,
    pub bgmfade_use_check: bool,
    pub filter_color_argb: u32,
    pub font_proportional: bool,
    pub font_name: String,
    pub font_shadow: i64,
    pub font_futoku: bool,
    pub message_speed: i64,
    pub message_speed_nowait: bool,
    pub auto_mode_onoff: bool,
    pub auto_mode_moji_wait: i64,
    pub auto_mode_min_wait: i64,
    pub mouse_cursor_hide_onoff: bool,
    pub mouse_cursor_hide_time: i64,
    pub jitan_normal_onoff: bool,
    pub jitan_auto_mode_onoff: bool,
    pub jitan_msgbk_onoff: bool,
    pub jitan_speed: i64,
    pub koe_mode: i64,
    pub chrkoe: Vec<ConfigChrKoeState>,
    pub message_chrcolor_flag: bool,
    pub object_disp_flag: Vec<bool>,
    pub global_extra_switch_flag: Vec<bool>,
    pub global_extra_mode_flag: Vec<i64>,
    pub sleep_flag: bool,
    pub no_wipe_anime_flag: bool,
    pub skip_wipe_anime_flag: bool,
    pub no_mwnd_anime_flag: bool,
    pub wheel_next_message_flag: bool,
    pub koe_dont_stop_flag: bool,
    pub skip_unread_message_flag: bool,
    pub saveload_alert_flag: bool,
    pub saveload_dblclick_flag: bool,
    pub ss_path: String,
    pub editor_path: String,
    pub koe_path: String,
    pub koe_tool_path: String,
}

impl Default for OriginalConfigRuntimeState {
    fn default() -> Self {
        Self {
            screen_size_mode: 0,
            screen_size_mode_window: 0,
            screen_size_scale: (100, 100),
            screen_size_free: (0, 0),
            fullscreen_change_resolution: false,
            fullscreen_display_cnt: 0,
            fullscreen_display_no: 0,
            fullscreen_resolution_cnt: 0,
            fullscreen_resolution_no: 0,
            fullscreen_resolution: (0, 0),
            fullscreen_mode: 0,
            fullscreen_scale: (100, 100),
            fullscreen_scale_sync_switch: true,
            fullscreen_move: (0, 0),
            all_sound_user_volume: 255,
            sound_user_volume: [255; 32],
            play_all_sound_check: true,
            play_sound_check: [true; 32],
            bgmfade_volume: 192,
            bgmfade_use_check: true,
            filter_color_argb: 0x8000_0000,
            font_proportional: false,
            font_name: "ＭＳ ゴシック".to_string(),
            font_shadow: 2,
            font_futoku: false,
            message_speed: 20,
            message_speed_nowait: false,
            auto_mode_onoff: false,
            auto_mode_moji_wait: 70,
            auto_mode_min_wait: 300,
            mouse_cursor_hide_onoff: false,
            mouse_cursor_hide_time: 5000,
            jitan_normal_onoff: false,
            jitan_auto_mode_onoff: false,
            jitan_msgbk_onoff: false,
            jitan_speed: 100,
            koe_mode: 0,
            chrkoe: vec![ConfigChrKoeState::default(); 64],
            message_chrcolor_flag: true,
            object_disp_flag: vec![true; 4],
            global_extra_switch_flag: vec![true; 4],
            global_extra_mode_flag: vec![0; 4],
            sleep_flag: false,
            no_wipe_anime_flag: false,
            skip_wipe_anime_flag: true,
            no_mwnd_anime_flag: false,
            wheel_next_message_flag: true,
            koe_dont_stop_flag: false,
            skip_unread_message_flag: false,
            saveload_alert_flag: true,
            saveload_dblclick_flag: false,
            ss_path: String::new(),
            editor_path: String::new(),
            koe_path: String::new(),
            koe_tool_path: String::new(),
        }
    }
}

#[derive(Debug, Clone)]
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
    pub unread_skip: ToggleFeatureState,
    pub auto_skip: ToggleFeatureState,
    pub auto_mode: ToggleFeatureState,
    pub hide_mwnd: ToggleFeatureState,
    pub local_extra_switch: ToggleFeatureState,
    pub local_extra_mode: ValueFeatureState,
    pub local_extra_switches: [ToggleFeatureState; 4],
    pub local_extra_modes: [ValueFeatureState; 4],
    pub msg_back: ToggleFeatureState,
    pub msg_back_open: bool,
    pub msg_back_view_pos: usize,
    pub msg_back_scroll_pos: i32,
    pub msg_back_slider_pos: i32,
    pub msg_back_target_no: isize,
    pub msg_back_mouse_target_no: isize,
    pub msg_back_msg_total_height: i32,
    pub msg_back_proc_initialized: bool,
    pub msg_back_slider_dragging: bool,
    pub msg_back_slider_drag_start_mouse: i32,
    pub msg_back_slider_drag_start_pos: i32,
    pub msg_back_content_dragging: bool,
    pub msg_back_content_drag_start_mouse: i32,
    pub msg_back_content_drag_start_scroll_pos: i32,
    pub return_to_sel: ToggleFeatureState,
    pub config_feature: ToggleFeatureState,
    pub manual_feature: ToggleFeatureState,
    pub version_feature: ToggleFeatureState,
    pub return_to_menu: ToggleFeatureState,
    pub end_game: ToggleFeatureState,
    pub cancel_feature: ToggleFeatureState,
    pub save_feature: ToggleFeatureState,
    pub load_feature: ToggleFeatureState,
    pub replay_koe: Option<(i64, i64)>,
    pub current_save_scene_title: String,
    pub current_save_message: String,
    pub current_save_full_message: String,
    pub total_play_time: i64,
    pub save_slots: Vec<SaveSlotState>,
    pub quick_save_slots: Vec<SaveSlotState>,
    pub inner_save_exists: bool,
    pub inner_save_streams: Vec<Vec<u8>>,
    pub sel_save_stock_stream: Vec<u8>,
    pub sel_save_ids: Vec<[u16; 7]>,
    pub end_save_exists: bool,
    pub last_menu_call: i32,
    pub system_extra_int_value: i64,
    pub system_extra_str_value: String,
    pub config_int: HashMap<i32, i64>,
    pub config_str: HashMap<i32, String>,
    pub original_config: OriginalConfigRuntimeState,
    pub capture_buffer: Option<RgbaImage>,
    pub capture_size: Option<(u32, u32)>,
    pub return_scene_once: Option<(String, i64)>,
    pub pending_proc: Option<SyscomPendingProc>,
    pub msg_back_load_tid: i64,
    pub fallback_dialog: Option<SyscomFallbackDialogState>,
    pub fallback_origin: Option<SyscomFallbackDialogKind>,
}

impl Default for SyscomRuntimeState {
    fn default() -> Self {
        Self {
            syscom_menu_disable: false,
            menu_open: false,
            menu_kind: None,
            menu_result: None,
            menu_cursor: 0,
            font_list: Vec::new(),
            mwnd_btn_disable_all: false,
            mwnd_btn_touch_disable: false,
            mwnd_btn_disable: HashMap::new(),
            read_skip: ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            },
            unread_skip: ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            },
            auto_skip: ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            },
            auto_mode: ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            },
            hide_mwnd: ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            },
            local_extra_switch: ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            },
            local_extra_mode: ValueFeatureState {
                value: 0,
                enable: true,
                exist: true,
            },
            local_extra_switches: [ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            }; 4],
            local_extra_modes: [ValueFeatureState {
                value: 0,
                enable: true,
                exist: true,
            }; 4],
            msg_back: ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            },
            msg_back_open: false,
            msg_back_view_pos: 0,
            msg_back_scroll_pos: 0,
            msg_back_slider_pos: 0,
            msg_back_target_no: -1,
            msg_back_mouse_target_no: -1,
            msg_back_msg_total_height: 0,
            msg_back_proc_initialized: false,
            msg_back_slider_dragging: false,
            msg_back_slider_drag_start_mouse: 0,
            msg_back_slider_drag_start_pos: 0,
            msg_back_content_dragging: false,
            msg_back_content_drag_start_mouse: 0,
            msg_back_content_drag_start_scroll_pos: 0,
            return_to_sel: ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            },
            config_feature: ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            },
            manual_feature: ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            },
            version_feature: ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            },
            return_to_menu: ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            },
            end_game: ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            },
            cancel_feature: ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            },
            save_feature: ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            },
            load_feature: ToggleFeatureState {
                onoff: false,
                enable: true,
                exist: true,
            },
            replay_koe: None,
            current_save_scene_title: String::new(),
            current_save_message: String::new(),
            current_save_full_message: String::new(),
            total_play_time: 0,
            save_slots: Vec::new(),
            quick_save_slots: Vec::new(),
            inner_save_exists: false,
            inner_save_streams: Vec::new(),
            sel_save_stock_stream: Vec::new(),
            sel_save_ids: Vec::new(),
            end_save_exists: false,
            last_menu_call: 0,
            system_extra_int_value: 0,
            system_extra_str_value: String::new(),
            config_int: HashMap::new(),
            config_str: HashMap::new(),
            original_config: OriginalConfigRuntimeState::default(),
            capture_buffer: None,
            capture_size: None,
            return_scene_once: None,
            pending_proc: None,
            msg_back_load_tid: 0,
            fallback_dialog: None,
            fallback_origin: None,
        }
    }
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
    pub x_event: IntEvent,
    pub texture_image_id: Option<ImageId>,
}

impl Default for FogGlobalState {
    fn default() -> Self {
        Self {
            enabled: false,
            name: String::new(),
            near: 0.0,
            far: 0.0,
            color: [0.62, 0.62, 0.62, 1.0],
            scroll_x: 0.0,
            x_event: IntEvent::new(0),
            texture_image_id: None,
        }
    }
}

impl FogGlobalState {
    pub fn set_x(&mut self, x: i32) {
        self.x_event.set_value(x);
        self.x_event.frame();
        self.scroll_x = self.x_event.get_total_value() as f32;
    }

    pub fn update_time(&mut self, past_game_time: i32, past_real_time: i32) {
        self.x_event.update_time(past_game_time, past_real_time);
    }

    pub fn frame(&mut self) {
        self.x_event.frame();
        self.scroll_x = self.x_event.get_total_value() as f32;
    }
}

/// Original `C_elm_pcmch` parameters persisted in local saves.
///
/// Playback handles alone cannot reconstruct the source kind (PCM/BGM/KOE/SE)
/// or the volume routing flags, so keep the exact command-side state alongside
/// the modern audio backend.
#[derive(Debug, Clone)]
pub struct PcmChPersistentState {
    pub pcm_name: String,
    pub bgm_name: String,
    pub koe_no: i64,
    pub se_no: i64,
    pub volume_type: i64,
    pub chara_no: i64,
    pub volume: i64,
    pub delay_time: i64,
    pub fade_in_time: i64,
    pub loop_flag: bool,
    pub bgm_fade_target_flag: bool,
    pub bgm_fade2_target_flag: bool,
    pub bgm_fade_source_flag: bool,
    pub ready_flag: bool,
}

impl Default for PcmChPersistentState {
    fn default() -> Self {
        Self {
            pcm_name: String::new(),
            bgm_name: String::new(),
            koe_no: -1,
            se_no: -1,
            volume_type: 2, // TNM_VOLUME_TYPE_PCM
            chara_no: -1,
            volume: 255,
            delay_time: 0,
            fade_in_time: 0,
            loop_flag: false,
            bgm_fade_target_flag: false,
            bgm_fade2_target_flag: false,
            bgm_fade_source_flag: false,
            ready_flag: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SoundRoutingState {
    /// Current C_elm_koe playback metadata.  This is independent from the
    /// message-local `cur_koe_no`: EXKOE must affect the actual buffer volume
    /// without replacing the voice attached to message history/replay.
    pub koe_chara_no: i64,
    pub koe_ex_flag: bool,
    /// Original global guard used when restoring BGMFADE2 after a message
    /// without an attached voice.
    pub bgmfade2_need_flag: bool,
    pub bgmfade_flag: bool,
    pub bgmfade_cur_time: i64,
    pub bgmfade_start_value: i64,
    pub bgmfade_total_volume: i64,
    pub bgmfade2_flag: bool,
    pub bgmfade2_cur_time: i64,
    pub bgmfade2_start_value: i64,
    pub bgmfade2_total_volume: i64,
}

impl Default for SoundRoutingState {
    fn default() -> Self {
        Self {
            koe_chara_no: -1,
            koe_ex_flag: false,
            bgmfade2_need_flag: false,
            bgmfade_flag: false,
            bgmfade_cur_time: 0,
            bgmfade_start_value: 255,
            bgmfade_total_volume: 255,
            bgmfade2_flag: false,
            bgmfade2_cur_time: 0,
            bgmfade2_start_value: 255,
            bgmfade2_total_volume: 255,
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
    /// Exact C_elm_pcmch save parameters, indexed by PCM channel.
    pub pcmch_persistent: Vec<PcmChPersistentState>,
    /// Dynamic category/chara/BGM-fade routing state from ifc_sound.cpp.
    pub sound_routing: SoundRoutingState,
    /// Original `Gp_read_flag[scene][flag]` backing store.  Rows are keyed by
    /// current Scene.pck scene number and contain one byte per lexer read flag.
    pub read_flags: HashMap<i64, Vec<u8>>,

    /// Generic integer-event roots keyed by the form ID.
    pub int_event_roots: HashMap<u32, IntEvent>,
    /// Generic integer-event lists keyed by the form ID.
    pub int_event_lists: HashMap<u32, Vec<IntEvent>>,

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
    /// Original C_elm_g00_buf persists file names, not texture handles.
    pub g00buf_names: Vec<Option<String>>,

    /// RNG state for MATH.RAND (xorshift32). 0 means "uninitialized".
    pub rng_state: u32,

    /// Mask subsystem state keyed by the (guessed or mapped) form id.
    pub mask_lists: HashMap<u32, MaskListState>,
    /// EditBox subsystem state keyed by the (guessed or mapped) form id.
    pub editbox_lists: HashMap<u32, EditBoxListState>,
    /// Currently focused editbox (form_id, index).
    pub focused_editbox: Option<(u32, usize)>,
    /// Cross-platform fallback clipboard used when the host does not expose an OS clipboard.
    pub editbox_clipboard: String,
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
    /// Current message-window handles used by GLOBAL.GET_MWND/SET_MWND.
    /// Original engine initializes these to FRONT.MWND[default_*].
    pub current_mwnd_no: Option<usize>,
    pub current_mwnd_stage_idx: i64,
    pub current_sel_mwnd_no: Option<usize>,
    pub current_sel_mwnd_stage_idx: i64,
    pub last_mwnd_no: Option<usize>,
    pub last_mwnd_stage_idx: i64,

    /// Original C_tnm_timer local fields saved by C_tnm_eng::save_local().
    pub local_real_time: i64,
    pub local_game_time: i64,
    pub local_wipe_time: i64,

    /// Original extend-enable local flag lists H/I/J.
    pub local_flag_h: Vec<i64>,
    pub local_flag_i: Vec<i64>,
    pub local_flag_j: Vec<i64>,
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

    /// Capture buffer reserved for the optional Tweet integration.
    ///
    /// This must never be used by OBJECT.CREATE_CAPTURE or save thumbnails.
    pub capture_image: Option<RgbaImage>,
    /// Capture buffer used exclusively by OBJECT.CREATE_CAPTURE.
    pub capture_for_object_image: Option<RgbaImage>,
    /// Save thumbnail capture prepared before entering the save UI.
    pub save_thumb_capture_image: Option<RgbaImage>,
    /// C++ `TNM_CAPTURE_PRIOR_*` value currently owning the save-thumbnail capture.
    pub save_thumb_capture_prior: i32,

    /// Currently selected append directory used by original file resolution helpers.
    pub append_dir: String,
    /// Display name for the currently selected append directory.
    pub append_name: String,

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
            pcmch_persistent: Vec::new(),
            sound_routing: SoundRoutingState::default(),
            read_flags: HashMap::new(),
            int_event_roots: HashMap::new(),
            int_event_lists: HashMap::new(),
            int_props: HashMap::new(),
            str_props: HashMap::new(),
            cg_table_off: false,
            database_off: false,
            g00buf: Vec::new(),
            g00buf_names: Vec::new(),
            rng_state: 0,
            mask_lists: HashMap::new(),
            editbox_lists: HashMap::new(),
            focused_editbox: None,
            editbox_clipboard: String::new(),
            change_display_mode_proc_cnt: 0,

            frame_actions: HashMap::new(),
            frame_action_lists: HashMap::new(),
            pending_frame_action_finishes: Vec::new(),
            pending_button_actions: Vec::new(),
            stage_forms: HashMap::new(),
            focused_stage_group: None,
            focused_stage_mwnd: None,
            current_mwnd_no: Some(0),
            current_mwnd_stage_idx: 1,
            current_sel_mwnd_no: Some(1),
            current_sel_mwnd_stage_idx: 1,
            last_mwnd_no: Some(0),
            last_mwnd_stage_idx: 1,
            local_real_time: 0,
            local_game_time: 0,
            local_wipe_time: 0,
            local_flag_h: Vec::new(),
            local_flag_i: Vec::new(),
            local_flag_j: Vec::new(),
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
            save_thumb_capture_image: None,
            save_thumb_capture_prior: 0,
            append_dir: String::new(),
            append_name: String::new(),

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

#[derive(Debug, Clone)]
pub struct BtnSelectChoiceState {
    /// Per-item template number saved by C_elm_btn_select_item.  Script-created
    /// items normally inherit the parent selection template.
    pub template_no: i64,
    pub base_file: String,
    pub filter_file: String,
    pub text: String,
    pub item_type: i64,
    pub color: i64,
    /// Runtime absolute screen position.  The original save stream stores this
    /// relative to S_tnm_btn_select_param::base_pos.
    pub pos: (i64, i64),
    pub size: (i64, i64),
    pub glyphs: Vec<MwndGlyphState>,
}

impl Default for BtnSelectChoiceState {
    fn default() -> Self {
        Self {
            template_no: -1,
            base_file: String::new(),
            filter_file: String::new(),
            text: String::new(),
            item_type: 1,
            color: -1,
            pos: (0, 0),
            size: (0, 0),
            glyphs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BtnSelectRuntimeState {
    pub template_no: i64,
    /// Exact serialized S_tnm_btn_select_param.  This is retained separately
    /// because C++ restores m_cur but deliberately leaves all animation work
    /// members (including m_sync_type) at their reinitialized values.
    pub saved_cur_param: Option<[i64; 28]>,
    pub choices: Vec<BtnSelectChoiceState>,
    pub cursor: usize,
    /// Mouse button currently captured by the selection manager.  The C++
    /// button manager enters PUSH on mouse-down and only decides on a matching
    /// mouse-up; it does not decide immediately on the down event.
    pub pressed_index: Option<usize>,
    pub pressed_inside: bool,
    pub cancel_enable: bool,
    pub capture_flag: bool,
    /// True only while the original button manager accepts input.
    pub started: bool,
    pub result: i64,
    pub sync_type: i64,
    pub read_flag_scene_no: i64,
    pub read_flag_flag_no: i64,
    pub sel_start_call_scn: String,
    pub sel_start_call_z_no: i64,

    // C_elm_btn_select animation and process state.  These are kept separate
    // from `started`: the original can return from SELBTN at sync points 1/2
    // while the close animation is still being drawn.
    pub appear_flag: bool,
    pub open_anime_type: i64,
    pub open_anime_time: i64,
    pub open_anime_cur_time: i64,
    pub close_anime_type: i64,
    pub close_anime_time: i64,
    pub close_anime_cur_time: i64,
    pub decide_anime_type: i64,
    pub decide_anime_time: i64,
    pub decide_anime_cur_time: i64,
    pub decide_sel_no: i64,
    pub processing_flag_0: bool,
    pub processing_flag_1: bool,
    pub processing_flag_2: bool,
    pub capture_now_flag: bool,
    pub result_delivered: bool,
}

impl Default for BtnSelectRuntimeState {
    fn default() -> Self {
        Self {
            template_no: -1,
            saved_cur_param: None,
            choices: Vec::new(),
            cursor: 0,
            pressed_index: None,
            pressed_inside: false,
            cancel_enable: false,
            capture_flag: false,
            started: false,
            result: 0,
            sync_type: 0,
            read_flag_scene_no: -1,
            read_flag_flag_no: -1,
            sel_start_call_scn: String::new(),
            sel_start_call_z_no: -1,
            appear_flag: false,
            open_anime_type: 0,
            open_anime_time: 0,
            open_anime_cur_time: 0,
            close_anime_type: 0,
            close_anime_time: 0,
            close_anime_cur_time: 0,
            decide_anime_type: 0,
            decide_anime_time: 0,
            decide_anime_cur_time: 0,
            decide_sel_no: -1,
            processing_flag_0: false,
            processing_flag_1: false,
            processing_flag_2: false,
            capture_now_flag: false,
            result_delivered: false,
        }
    }
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
    UserCall {
        scn_name: String,
        cmd_name: String,
        z_no: i64,
    },
    Syscom {
        sys_type: i64,
        sys_type_opt: i64,
        mode: i64,
    },
}

impl Default for PendingButtonActionKind {
    fn default() -> Self {
        Self::UserCall {
            scn_name: String::new(),
            cmd_name: String::new(),
            z_no: -1,
        }
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
    pub audio_start_attempted: bool,
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
            audio_start_attempted: false,
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
        self.audio_start_attempted = false;
    }

    pub fn stop(&mut self) {
        self.file_name = None;
        self.playing = false;
        self.key_skip_flag = false;
        self.timer_ms = 0;
        self.total_ms = None;
        self.last_frame_idx = None;
        self.audio_id = None;
        self.audio_start_attempted = false;
    }

    pub fn tick(&mut self, past_real_time: i32) {
        if !self.playing || self.audio_id.is_some() {
            // Once movie audio starts, its hardware playback position is the
            // master clock. The renderer synchronizes timer_ms from Kira.
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
            let add = if self.real_flag {
                past_real_time
            } else {
                past_game_time
            };
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

    pub(crate) fn save_parts(&self) -> (bool, bool, bool, bool, i64, i64, i64, i64) {
        (
            self.is_running,
            self.real_flag,
            self.frame_mode,
            self.frame_loop_flag,
            self.frame_start_value,
            self.frame_end_value,
            self.frame_time,
            self.cur_time,
        )
    }

    pub(crate) fn from_save_parts(
        is_running: bool,
        real_flag: bool,
        frame_mode: bool,
        frame_loop_flag: bool,
        frame_start_value: i64,
        frame_end_value: i64,
        frame_time: i64,
        cur_time: i64,
    ) -> Self {
        Self {
            cur_time,
            is_running,
            real_flag,
            frame_mode,
            frame_loop_flag,
            frame_start_value,
            frame_end_value,
            frame_time,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PcmEventLine {
    pub file_name: String,
    pub probability: i32,
    pub min_time: i32,
    pub max_time: i32,
}

pub const PCM_EVENT_TYPE_NONE: i32 = -1;
pub const PCM_EVENT_TYPE_ONESHOT: i32 = 0;
pub const PCM_EVENT_TYPE_LOOP: i32 = 1;
pub const PCM_EVENT_TYPE_RANDOM: i32 = 2;

#[derive(Debug, Clone)]
pub struct PcmEventState {
    pub event_type: i32,
    pub pcm_buf_no: i32,
    pub volume_type: i32,
    pub chara_no: i32,
    pub bgm_fade_target_flag: bool,
    pub bgm_fade2_target_flag: bool,
    pub bgm_fade_source_flag: bool,
    pub real_flag: bool,
    pub time_type: bool,
    pub lines: Vec<PcmEventLine>,

    // C_elm_pcm_event working members. They deliberately are not serialized:
    // LOOP/RANDOM are restarted from the beginning after loading, while
    // ONESHOT is not restored by the original save format.
    pub cur_time: i64,
    pub cur_line_no: i32,
    pub next_time: i64,
    pub last_line_no: i32,
}

impl Default for PcmEventState {
    fn default() -> Self {
        Self {
            event_type: PCM_EVENT_TYPE_NONE,
            pcm_buf_no: -1,
            volume_type: 2, // TNM_VOLUME_TYPE_PCM
            chara_no: -1,
            bgm_fade_target_flag: false,
            bgm_fade2_target_flag: false,
            bgm_fade_source_flag: false,
            real_flag: false,
            time_type: false,
            lines: Vec::new(),
            cur_time: 0,
            cur_line_no: -1,
            next_time: 0,
            last_line_no: -1,
        }
    }
}

impl PcmEventState {
    pub fn reinit(&mut self) {
        *self = Self::default();
    }

    pub fn is_active(&self) -> bool {
        self.event_type != PCM_EVENT_TYPE_NONE
    }

    pub fn start(
        &mut self,
        event_type: i32,
        pcm_buf_no: i32,
        volume_type: i32,
        chara_no: i32,
        bgm_fade_target_flag: bool,
        bgm_fade2_target_flag: bool,
        bgm_fade_source_flag: bool,
        real_flag: bool,
        time_type: bool,
    ) {
        self.event_type = event_type;
        self.pcm_buf_no = pcm_buf_no;
        self.volume_type = volume_type;
        self.chara_no = chara_no;
        self.bgm_fade_target_flag = bgm_fade_target_flag;
        self.bgm_fade2_target_flag = bgm_fade2_target_flag;
        self.bgm_fade_source_flag = bgm_fade_source_flag;
        self.real_flag = real_flag;
        self.time_type = time_type;
        self.cur_time = 0;
        self.cur_line_no = -1;
        self.next_time = 0;
        self.last_line_no = -1;
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

pub const EDITBOX_ACTION_NOT_DECIDED: i32 = 0;
pub const EDITBOX_ACTION_DECIDED: i32 = 1;
pub const EDITBOX_ACTION_CANCELED: i32 = -1;

const EDITBOX_UNDO_LIMIT: usize = 64;
const EDITBOX_TEXT_PADDING_X: i32 = 4;

#[derive(Debug, Clone, PartialEq, Eq)]
struct EditBoxUndoSnapshot {
    text: String,
    cursor_pos: usize,
    selection_anchor: Option<usize>,
}

#[derive(Debug, Clone)]
pub struct EditBoxState {
    pub created: bool,
    pub visible: bool,
    pub text: String,
    /// UTF-8 byte index. It is always normalized to a character boundary.
    pub cursor_pos: usize,
    /// The fixed end of the selection; `cursor_pos` is the active end.
    pub selection_anchor: Option<usize>,
    /// Current winit IME preedit string. It is not part of `GET_TEXT` until committed.
    pub composition_text: String,
    /// Selected/caret byte range inside `composition_text`, as supplied by winit.
    pub composition_cursor: Option<(usize, usize)>,
    /// Committed-text range replaced by the current composition.
    pub composition_range: Option<(usize, usize)>,
    /// Winit sends an empty Preedit immediately before Commit. Keep the
    /// replacement range alive until the next event tells us commit or cancel.
    pub composition_clear_pending: bool,
    /// Horizontal ES_AUTOHSCROLL-equivalent offset in logical pixels.
    pub scroll_x_px: i32,
    /// True while the left pointer owns selection capture for this editbox.
    pub mouse_selecting: bool,
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
    undo_stack: Vec<EditBoxUndoSnapshot>,
    redo_stack: Vec<EditBoxUndoSnapshot>,
}

impl Default for EditBoxState {
    fn default() -> Self {
        Self {
            created: false,
            visible: false,
            text: String::new(),
            cursor_pos: 0,
            selection_anchor: None,
            composition_text: String::new(),
            composition_cursor: None,
            composition_range: None,
            composition_clear_pending: false,
            scroll_x_px: 0,
            mouse_selecting: false,
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
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
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
        self.selection_anchor = None;
        self.clear_composition();
        self.scroll_x_px = 0;
        self.mouse_selecting = false;
        self.undo_stack.clear();
        self.redo_stack.clear();
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
        *self = Self::default();
    }

    pub fn set_text_like(&mut self, text: String) {
        self.text = text;
        self.cursor_pos = self.text.len();
        self.selection_anchor = None;
        self.clear_composition();
        self.scroll_x_px = 0;
        self.mouse_selecting = false;
        self.undo_stack.clear();
        self.redo_stack.clear();
        self.ensure_caret_visible();
    }

    pub fn insert_text_at_cursor(&mut self, text: &str) {
        self.commit_text(text);
    }

    pub fn commit_text(&mut self, text: &str) {
        let filtered: String = text.chars().filter(|ch| !ch.is_control()).collect();
        if filtered.is_empty() {
            self.cancel_composition();
            return;
        }
        let range = self
            .composition_range
            .take()
            .or_else(|| self.selection_range())
            .unwrap_or_else(|| {
                let p = self.normalized_cursor();
                (p, p)
            });
        self.push_undo_snapshot();
        self.replace_range_without_undo(range, &filtered);
        self.selection_anchor = None;
        self.clear_composition();
        self.ensure_caret_visible();
    }

    pub fn set_ime_preedit(&mut self, text: &str, cursor: Option<(usize, usize)>) {
        if text.is_empty() {
            if self.composition_range.is_some() {
                self.composition_text.clear();
                self.composition_cursor = None;
                self.composition_clear_pending = true;
                self.ensure_caret_visible();
            }
            return;
        }
        if self.composition_clear_pending {
            self.cancel_composition();
        }
        if self.composition_range.is_none() {
            let range = self.selection_range().unwrap_or_else(|| {
                let p = self.normalized_cursor();
                (p, p)
            });
            self.composition_range = Some(range);
            self.cursor_pos = range.0;
            self.selection_anchor = None;
        }
        self.composition_text.clear();
        self.composition_text.push_str(text);
        self.composition_clear_pending = false;
        self.composition_cursor = cursor.map(|(a, b)| {
            (
                Self::normalize_boundary(&self.composition_text, a),
                Self::normalize_boundary(&self.composition_text, b),
            )
        });
        self.ensure_caret_visible();
    }

    pub fn cancel_composition(&mut self) {
        let had_composition = self.composition_range.is_some() || !self.composition_text.is_empty();
        if !had_composition {
            return;
        }
        if let Some((start, _)) = self.composition_range {
            self.cursor_pos = Self::normalize_boundary(&self.text, start);
        }
        self.clear_composition();
        self.selection_anchor = None;
        self.ensure_caret_visible();
    }

    pub fn clear_composition(&mut self) {
        self.composition_text.clear();
        self.composition_cursor = None;
        self.composition_range = None;
        self.composition_clear_pending = false;
    }

    pub fn cancel_pending_composition_clear(&mut self) {
        if self.composition_clear_pending {
            self.cancel_composition();
        }
    }

    pub fn is_composing(&self) -> bool {
        self.composition_range.is_some()
    }

    pub fn selection_range(&self) -> Option<(usize, usize)> {
        let cursor = self.normalized_cursor();
        let anchor = self
            .selection_anchor
            .map(|p| Self::normalize_boundary(&self.text, p))?;
        if anchor == cursor {
            None
        } else if anchor < cursor {
            Some((anchor, cursor))
        } else {
            Some((cursor, anchor))
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection_range()?;
        Some(self.text[start..end].to_string())
    }

    pub fn select_all(&mut self) {
        self.cancel_composition();
        self.selection_anchor = Some(0);
        self.cursor_pos = self.text.len();
        self.ensure_caret_visible();
    }

    pub fn cut_selection(&mut self) -> Option<String> {
        let selected = self.selected_text()?;
        self.delete_selection();
        Some(selected)
    }

    pub fn backspace_like(&mut self) {
        self.delete_backward(false);
    }

    pub fn delete_backward(&mut self, by_word: bool) {
        if self.is_composing() {
            return;
        }
        if self.delete_selection() {
            return;
        }
        let cursor = self.normalized_cursor();
        if cursor == 0 {
            return;
        }
        let start = if by_word {
            self.word_left_boundary(cursor)
        } else {
            Self::previous_boundary(&self.text, cursor)
        };
        self.push_undo_snapshot();
        self.replace_range_without_undo((start, cursor), "");
        self.ensure_caret_visible();
    }

    pub fn delete_forward(&mut self, by_word: bool) {
        if self.is_composing() {
            return;
        }
        if self.delete_selection() {
            return;
        }
        let cursor = self.normalized_cursor();
        if cursor >= self.text.len() {
            return;
        }
        let end = if by_word {
            self.word_right_boundary(cursor)
        } else {
            Self::next_boundary(&self.text, cursor)
        };
        self.push_undo_snapshot();
        self.replace_range_without_undo((cursor, end), "");
        self.ensure_caret_visible();
    }

    pub fn move_cursor_left(&mut self, extend: bool, by_word: bool) {
        if self.is_composing() {
            return;
        }
        if !extend {
            if let Some((start, _)) = self.selection_range() {
                self.cursor_pos = start;
                self.selection_anchor = None;
                self.ensure_caret_visible();
                return;
            }
        }
        let old = self.normalized_cursor();
        let next = if by_word {
            self.word_left_boundary(old)
        } else {
            Self::previous_boundary(&self.text, old)
        };
        self.set_cursor_with_selection(old, next, extend);
    }

    pub fn move_cursor_right(&mut self, extend: bool, by_word: bool) {
        if self.is_composing() {
            return;
        }
        if !extend {
            if let Some((_, end)) = self.selection_range() {
                self.cursor_pos = end;
                self.selection_anchor = None;
                self.ensure_caret_visible();
                return;
            }
        }
        let old = self.normalized_cursor();
        let next = if by_word {
            self.word_right_boundary(old)
        } else {
            Self::next_boundary(&self.text, old)
        };
        self.set_cursor_with_selection(old, next, extend);
    }

    pub fn move_cursor_home(&mut self, extend: bool) {
        if self.is_composing() {
            return;
        }
        let old = self.normalized_cursor();
        self.set_cursor_with_selection(old, 0, extend);
    }

    pub fn move_cursor_end(&mut self, extend: bool) {
        if self.is_composing() {
            return;
        }
        let old = self.normalized_cursor();
        self.set_cursor_with_selection(old, self.text.len(), extend);
    }

    pub fn set_cursor_from_window_x(&mut self, window_x: i32, extend: bool) {
        self.cancel_composition();
        let target_x = window_x
            .saturating_sub(self.window_x)
            .saturating_sub(EDITBOX_TEXT_PADDING_X)
            .saturating_add(self.scroll_x_px)
            .max(0);
        let mut x = 0i32;
        let mut target = self.text.len();
        for (idx, ch) in self.text.char_indices() {
            let width = crate::text_render::editbox_cell_width_px(ch, self.font_px());
            if target_x < x.saturating_add((width + 1) / 2) {
                target = idx;
                break;
            }
            x = x.saturating_add(width);
            target = idx + ch.len_utf8();
        }
        let old = self.normalized_cursor();
        self.set_cursor_with_selection(old, target, extend);
    }

    pub fn caret_window_x(&self) -> i32 {
        let content_x = self.display_caret_content_x();
        self.window_x
            .saturating_add(EDITBOX_TEXT_PADDING_X)
            .saturating_add(content_x.saturating_sub(self.scroll_x_px))
    }

    pub fn ensure_caret_visible_for_focus(&mut self) {
        self.ensure_caret_visible();
    }

    pub fn undo(&mut self) {
        self.cancel_composition();
        let Some(snapshot) = self.undo_stack.pop() else {
            return;
        };
        let current = self.snapshot();
        self.redo_stack.push(current);
        self.restore_snapshot(snapshot);
    }

    pub fn redo(&mut self) {
        self.cancel_composition();
        let Some(snapshot) = self.redo_stack.pop() else {
            return;
        };
        let current = self.snapshot();
        self.undo_stack.push(current);
        self.restore_snapshot(snapshot);
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
        self.ensure_caret_visible();
    }

    pub fn frame(&mut self, display_mode_change_proc_cnt: i32) {
        self.visible = self.created && display_mode_change_proc_cnt == 0;
        if !self.visible {
            self.mouse_selecting = false;
            self.clear_composition();
        }
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

    fn font_px(&self) -> i32 {
        self.window_moji_size.max(1)
    }

    fn normalized_cursor(&self) -> usize {
        Self::normalize_boundary(&self.text, self.cursor_pos)
    }

    fn normalize_boundary(text: &str, pos: usize) -> usize {
        let mut pos = pos.min(text.len());
        while pos > 0 && !text.is_char_boundary(pos) {
            pos -= 1;
        }
        pos
    }

    fn previous_boundary(text: &str, pos: usize) -> usize {
        let pos = Self::normalize_boundary(text, pos);
        text[..pos]
            .char_indices()
            .next_back()
            .map(|(idx, _)| idx)
            .unwrap_or(0)
    }

    fn next_boundary(text: &str, pos: usize) -> usize {
        let pos = Self::normalize_boundary(text, pos);
        text[pos..]
            .chars()
            .next()
            .map(|ch| pos + ch.len_utf8())
            .unwrap_or(text.len())
    }

    fn word_class(ch: char) -> u8 {
        if ch.is_whitespace() {
            0
        } else if ch.is_alphanumeric() || ch == '_' {
            1
        } else {
            2
        }
    }

    fn word_left_boundary(&self, pos: usize) -> usize {
        let mut cursor = Self::normalize_boundary(&self.text, pos);
        while cursor > 0 {
            let prev = Self::previous_boundary(&self.text, cursor);
            let ch = self.text[prev..cursor].chars().next().unwrap_or(' ');
            if Self::word_class(ch) != 0 {
                break;
            }
            cursor = prev;
        }
        let Some(class) = (cursor > 0).then(|| {
            let prev = Self::previous_boundary(&self.text, cursor);
            let ch = self.text[prev..cursor].chars().next().unwrap_or(' ');
            Self::word_class(ch)
        }) else {
            return 0;
        };
        while cursor > 0 {
            let prev = Self::previous_boundary(&self.text, cursor);
            let ch = self.text[prev..cursor].chars().next().unwrap_or(' ');
            if Self::word_class(ch) != class {
                break;
            }
            cursor = prev;
        }
        cursor
    }

    fn word_right_boundary(&self, pos: usize) -> usize {
        let mut cursor = Self::normalize_boundary(&self.text, pos);
        if cursor >= self.text.len() {
            return self.text.len();
        }
        let class = self.text[cursor..]
            .chars()
            .next()
            .map(Self::word_class)
            .unwrap_or(0);
        while cursor < self.text.len() {
            let next = Self::next_boundary(&self.text, cursor);
            let ch = self.text[cursor..next].chars().next().unwrap_or(' ');
            if Self::word_class(ch) != class {
                break;
            }
            cursor = next;
        }
        while cursor < self.text.len() {
            let next = Self::next_boundary(&self.text, cursor);
            let ch = self.text[cursor..next].chars().next().unwrap_or(' ');
            if Self::word_class(ch) != 0 {
                break;
            }
            cursor = next;
        }
        cursor
    }

    fn set_cursor_with_selection(&mut self, old: usize, next: usize, extend: bool) {
        if extend {
            if self.selection_anchor.is_none() {
                self.selection_anchor = Some(old);
            }
        } else {
            self.selection_anchor = None;
        }
        self.cursor_pos = Self::normalize_boundary(&self.text, next);
        if self.selection_anchor == Some(self.cursor_pos) {
            self.selection_anchor = None;
        }
        self.ensure_caret_visible();
    }

    fn delete_selection(&mut self) -> bool {
        let Some(range) = self.selection_range() else {
            return false;
        };
        self.push_undo_snapshot();
        self.replace_range_without_undo(range, "");
        self.selection_anchor = None;
        self.ensure_caret_visible();
        true
    }

    fn replace_range_without_undo(&mut self, range: (usize, usize), replacement: &str) {
        let start = Self::normalize_boundary(&self.text, range.0.min(range.1));
        let end = Self::normalize_boundary(&self.text, range.0.max(range.1));
        self.text.replace_range(start..end, replacement);
        self.cursor_pos = start.saturating_add(replacement.len()).min(self.text.len());
        self.cursor_pos = Self::normalize_boundary(&self.text, self.cursor_pos);
    }

    fn snapshot(&self) -> EditBoxUndoSnapshot {
        EditBoxUndoSnapshot {
            text: self.text.clone(),
            cursor_pos: self.normalized_cursor(),
            selection_anchor: self
                .selection_anchor
                .map(|p| Self::normalize_boundary(&self.text, p)),
        }
    }

    fn restore_snapshot(&mut self, snapshot: EditBoxUndoSnapshot) {
        self.text = snapshot.text;
        self.cursor_pos = Self::normalize_boundary(&self.text, snapshot.cursor_pos);
        self.selection_anchor = snapshot
            .selection_anchor
            .map(|p| Self::normalize_boundary(&self.text, p));
        self.clear_composition();
        self.ensure_caret_visible();
    }

    fn push_undo_snapshot(&mut self) {
        let snapshot = self.snapshot();
        if self.undo_stack.last() == Some(&snapshot) {
            return;
        }
        if self.undo_stack.len() >= EDITBOX_UNDO_LIMIT {
            self.undo_stack.remove(0);
        }
        self.undo_stack.push(snapshot);
        self.redo_stack.clear();
    }

    fn display_width_before(&self, byte_pos: usize) -> i32 {
        let pos = Self::normalize_boundary(&self.text, byte_pos);
        self.text[..pos].chars().fold(0i32, |sum, ch| {
            sum.saturating_add(crate::text_render::editbox_cell_width_px(
                ch,
                self.font_px(),
            ))
        })
    }

    fn display_caret_content_x(&self) -> i32 {
        if let Some((start, _end)) = self.composition_range {
            let mut x = self.display_width_before(start);
            let comp_cursor = self
                .composition_cursor
                .map(|(_, end)| Self::normalize_boundary(&self.composition_text, end))
                .unwrap_or(self.composition_text.len());
            for ch in self.composition_text[..comp_cursor].chars() {
                x = x.saturating_add(crate::text_render::editbox_cell_width_px(
                    ch,
                    self.font_px(),
                ));
            }
            x
        } else {
            self.display_width_before(self.normalized_cursor())
        }
    }

    fn display_total_width(&self) -> i32 {
        if let Some((start, end)) = self.composition_range {
            let start = Self::normalize_boundary(&self.text, start);
            let end = Self::normalize_boundary(&self.text, end);
            let mut width = self.display_width_before(start);
            for ch in self.composition_text.chars() {
                width = width.saturating_add(crate::text_render::editbox_cell_width_px(
                    ch,
                    self.font_px(),
                ));
            }
            for ch in self.text[end..].chars() {
                width = width.saturating_add(crate::text_render::editbox_cell_width_px(
                    ch,
                    self.font_px(),
                ));
            }
            width
        } else {
            self.display_width_before(self.text.len())
        }
    }

    fn ensure_caret_visible(&mut self) {
        let available = self
            .window_w
            .saturating_sub(EDITBOX_TEXT_PADDING_X.saturating_mul(2))
            .max(1);
        let caret = self.display_caret_content_x();
        if caret < self.scroll_x_px {
            self.scroll_x_px = caret;
        } else if caret > self.scroll_x_px.saturating_add(available) {
            self.scroll_x_px = caret.saturating_sub(available);
        }
        let max_scroll = self.display_total_width().saturating_sub(available).max(0);
        self.scroll_x_px = self.scroll_x_px.clamp(0, max_scroll);
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

#[cfg(test)]
mod editbox_state_tests {
    use super::EditBoxState;

    fn editbox_with_text(text: &str) -> EditBoxState {
        let mut editbox = EditBoxState::default();
        editbox.create_like(0, 0, 320, 32, 20, 1280, 720);
        editbox.update_rect(1280, 720);
        editbox.frame(0);
        editbox.set_text_like(text.to_string());
        editbox
    }

    #[test]
    fn unicode_backspace_and_delete_keep_utf8_boundaries() {
        let mut editbox = editbox_with_text("A猫😀B");
        editbox.move_cursor_left(false, false);
        editbox.delete_backward(false);
        assert_eq!(editbox.text, "A猫B");
        assert!(editbox.text.is_char_boundary(editbox.cursor_pos));

        editbox.move_cursor_home(false);
        editbox.move_cursor_right(false, false);
        editbox.delete_forward(false);
        assert_eq!(editbox.text, "AB");
        assert!(editbox.text.is_char_boundary(editbox.cursor_pos));
    }

    #[test]
    fn committed_text_replaces_active_selection() {
        let mut editbox = editbox_with_text("abcdef");
        editbox.move_cursor_home(false);
        editbox.move_cursor_right(false, false);
        editbox.move_cursor_right(true, false);
        editbox.move_cursor_right(true, false);
        assert_eq!(editbox.selection_range(), Some((1, 3)));

        editbox.commit_text("猫");
        assert_eq!(editbox.text, "a猫def");
        assert_eq!(editbox.selection_range(), None);
        assert_eq!(editbox.cursor_pos, "a猫".len());
    }

    #[test]
    fn ime_preedit_is_transient_until_commit() {
        let mut editbox = editbox_with_text("abcd");
        editbox.move_cursor_home(false);
        editbox.move_cursor_right(false, false);
        editbox.move_cursor_right(true, false);
        editbox.move_cursor_right(true, false);

        editbox.set_ime_preedit("日本", Some((3, 6)));
        assert_eq!(editbox.text, "abcd");
        assert_eq!(editbox.composition_range, Some((1, 3)));
        assert!(editbox.is_composing());

        // Winit clears Preedit immediately before delivering Commit. The
        // original replacement range must survive that synthetic clear.
        editbox.set_ime_preedit("", None);
        assert!(editbox.composition_clear_pending);
        assert_eq!(editbox.composition_range, Some((1, 3)));

        editbox.commit_text("日本");
        assert_eq!(editbox.text, "a日本d");
        assert!(!editbox.is_composing());
        assert_eq!(editbox.cursor_pos, "a日本".len());
    }

    #[test]
    fn canceled_empty_preedit_does_not_replace_later_input() {
        let mut editbox = editbox_with_text("abcd");
        editbox.move_cursor_home(false);
        editbox.move_cursor_right(false, false);
        editbox.move_cursor_right(true, false);
        editbox.set_ime_preedit("x", Some((1, 1)));
        editbox.set_ime_preedit("", None);
        editbox.cancel_pending_composition_clear();
        editbox.commit_text("Z");
        assert_eq!(editbox.text, "aZbcd");
    }

    #[test]
    fn undo_and_redo_restore_text_cursor_and_selection() {
        let mut editbox = editbox_with_text("one");
        editbox.commit_text(" two");
        assert_eq!(editbox.text, "one two");

        editbox.undo();
        assert_eq!(editbox.text, "one");
        assert_eq!(editbox.cursor_pos, 3);

        editbox.redo();
        assert_eq!(editbox.text, "one two");
        assert_eq!(editbox.cursor_pos, 7);
    }

    #[test]
    fn mouse_drag_keeps_the_initial_selection_anchor() {
        let mut editbox = editbox_with_text("abcdef");
        editbox.set_cursor_from_window_x(editbox.window_x + 4, false);
        let anchor = editbox.cursor_pos;
        editbox.set_cursor_from_window_x(editbox.window_x + 80, true);
        assert_eq!(editbox.selection_anchor, Some(anchor));
        editbox.set_cursor_from_window_x(editbox.window_x + 120, true);
        assert_eq!(editbox.selection_anchor, Some(anchor));
        assert!(editbox.selection_range().is_some());
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
    /// CREATE_COPY_FROM
    CreateCopyFrom,
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
pub struct StringGlyphBackend {
    /// Original glyph index in the parsed string/mwnd item.
    pub glyph_index: usize,
    /// Layer-local texture origins relative to OBJECT.X/Y or the selection
    /// item origin.  Bearings and outline/shadow padding differ per layer.
    pub shadow_local_x: i32,
    pub shadow_local_y: i32,
    pub fuchi_local_x: i32,
    pub fuchi_local_y: i32,
    pub body_local_x: i32,
    pub body_local_y: i32,
    pub shadow_sprite_id: SpriteId,
    pub fuchi_sprite_id: SpriteId,
    pub body_sprite_id: SpriteId,
    pub shadow_image_id: Option<ImageId>,
    pub fuchi_image_id: Option<ImageId>,
    pub body_image_id: Option<ImageId>,
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
    /// STRING object backend: original shadow/fuchi/body sprite triplet.
    String {
        layer_id: LayerId,
        shadow_sprite_id: SpriteId,
        fuchi_sprite_id: SpriteId,
        sprite_id: SpriteId,
        shadow_image_id: Option<ImageId>,
        fuchi_image_id: Option<ImageId>,
        image_id: Option<ImageId>,
        /// Per-glyph shadow/fuchi/body sprites.  The three scalar sprite fields
        /// above alias the first entry for compatibility with older helper paths;
        /// when this list is non-empty it is authoritative.
        glyphs: Vec<StringGlyphBackend>,
        /// MWND glyphs use the configured shadow/fuchi/body relative layers.
        /// Standalone OBJECT.STRING sprites retain the object's own sorter.
        mwnd_layer_reps: bool,
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
    /// Additional cut offset applied after OBJECT.PATNO for button rendering.
    ///
    /// Original C_elm_object::frame()/create_trp() submits
    /// `obp.pat_no + button.cut_no`, and button action templates add their
    /// `rep_pat_no` on top of that submitted cut.
    pub cut_no: i64,
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
            cut_no: 0,
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

#[derive(Debug, Clone)]
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

impl Default for ObjectStringParam {
    fn default() -> Self {
        Self {
            moji_size: 12,
            moji_space_x: 0,
            moji_space_y: 0,
            moji_cnt: 0,
            moji_color: 0,
            shadow_color: 1,
            fuchi_color: 1,
            shadow_mode: -1,
        }
    }
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
        self.rand_seed = self.rand_seed.wrapping_mul(1103515245).wrapping_add(12345);
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
    pub last_tick: Option<Instant>,
    pub last_frame_idx: Option<usize>,
    pub audio_id: Option<u64>,
    pub audio_started_once: bool,
    // OBJECT.OMV mirrors the original single D3DUSAGE_DYNAMIC texture. Slot 0
    // owns that stable ImageId for the movie lifetime; slot 1 is retained only
    // for snapshot/backward state layout compatibility and is no longer used
    // for per-frame ping-pong.
    pub frame_image_ids: [Option<ImageId>; 2],
    pub frame_image_cursor: usize,
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
            audio_started_once: false,
            frame_image_ids: [None, None],
            frame_image_cursor: 0,
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
        self.last_tick = Some(Instant::now());
        self.last_frame_idx = None;
        self.audio_id = None;
        self.audio_started_once = false;
        self.frame_image_ids = [None, None];
        self.frame_image_cursor = 0;
        self.just_finished = false;
        self.just_looped = false;
        self.seeked = false;
    }

    pub fn tick(&mut self, past_game_time: i32, past_real_time: i32) {
        self.just_finished = false;
        self.just_looped = false;
        if !self.playing || self.pause_flag || self.audio_id.is_some() {
            // The audio device is the master clock while a movie soundtrack is
            // active. Advancing a second wall/game clock here causes drift and
            // lets video reach EOS before the sound handle does.
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
        // C_elm_object::seek_movie stores m_omv_timer verbatim. Loop wrapping
        // happens later from the movie update path, not in SEEK_MOVIE itself.
        self.timer_ms = time_ms;
        self.last_tick = Some(Instant::now());
        self.last_frame_idx = None;
        self.audio_started_once = false;
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

#[derive(Debug, Clone)]
pub struct ObjectEmoteParam {
    pub width: i64,
    pub height: i64,
    pub file_name: Option<String>,
    pub timeline_names: [String; 8],
    pub timeline_options: [i64; 8],
    pub koe_chara_no: i64,
    pub koe_mouth_volume: i64,
    pub rep_x: i64,
    pub rep_y: i64,
    pub runtime: Option<crate::emote::SiglusEmoteRuntime>,
}

impl Default for ObjectEmoteParam {
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            file_name: None,
            timeline_names: std::array::from_fn(|_| String::new()),
            timeline_options: [0; 8],
            koe_chara_no: -1,
            koe_mouth_volume: 0,
            rep_x: 0,
            rep_y: 0,
            runtime: None,
        }
    }
}

impl ObjectEmoteParam {
    pub fn clear_runtime(&mut self) {
        self.runtime = None;
    }

    pub fn tick(&mut self, past_game_time: i32) {
        if let Some(runtime) = self.runtime.as_mut() {
            if let Err(err) = runtime.progress_ms(past_game_time) {
                log::error!("Emote Progress failed: {err:#}");
            }
        }
    }

    pub fn is_animating(&self) -> bool {
        if let Some(runtime) = self.runtime.as_ref() {
            return runtime.is_animating();
        }
        false
    }

    pub fn clone_player_for_object(&mut self) {
        if let Some(runtime) = self.runtime.as_ref() {
            self.runtime = Some(runtime.clone_for_object());
        }
    }
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
        self.center_rep_x
            .update_time(past_game_time, past_real_time);
        self.center_rep_y
            .update_time(past_game_time, past_real_time);
        self.center_rep_z
            .update_time(past_game_time, past_real_time);
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
        self.src_clip_left
            .update_time(past_game_time, past_real_time);
        self.src_clip_top
            .update_time(past_game_time, past_real_time);
        self.src_clip_right
            .update_time(past_game_time, past_real_time);
        self.src_clip_bottom
            .update_time(past_game_time, past_real_time);
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
            f: Vec::new(),
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

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ObjectRectParam {
    pub left: i64,
    pub top: i64,
    pub right: i64,
    pub bottom: i64,
    /// Packed C++ `C_argb` value: 0xAARRGGBB.
    pub color_argb: i64,
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

    /// For RECT objects.
    pub rect_param: ObjectRectParam,

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

    pub base: ObjectBaseState,

    pub button: ObjectButtonState,

    pub runtime: ObjectRuntimeState,

    pub mesh_animation_state: crate::mesh3d::MeshAnimationState,
    /// Optional backend-only slot override for top-level objects whose logical
    /// list ownership is not represented by GfxRuntime (currently EXCALL).
    /// This is deliberately separate from nested_runtime_slot: many VM/render
    /// paths use nested_runtime_slot as the semantic marker for CHILD/MWND tree
    /// objects.
    pub backend_runtime_slot: Option<usize>,
    pub nested_runtime_slot: Option<usize>,
}

fn normalize_object_int_prop(
    ids: &crate::runtime::constants::RuntimeConstants,
    op: i32,
    value: i64,
) -> i64 {
    let byte_range = [
        ids.obj_tr,
        ids.obj_mono,
        ids.obj_reverse,
        ids.obj_bright,
        ids.obj_dark,
        ids.obj_color_r,
        ids.obj_color_g,
        ids.obj_color_b,
        ids.obj_color_rate,
        ids.obj_color_add_r,
        ids.obj_color_add_g,
        ids.obj_color_add_b,
    ];
    if byte_range.iter().any(|&id| id != 0 && op == id) {
        return value.clamp(0, 255);
    }

    if op == ids.obj_disp {
        return if value != 0 { 1 } else { 0 };
    }

    let boolean = [
        ids.obj_clip_use,
        ids.obj_src_clip_use,
        ids.obj_fog_use,
        ids.obj_culling,
        ids.obj_alpha_test,
        ids.obj_alpha_blend,
        ids.obj_wipe_copy,
        ids.obj_wipe_erase,
        ids.obj_click_disable,
    ];
    if boolean.iter().any(|&id| id != 0 && op == id) {
        return if value != 0 { 1 } else { 0 };
    }

    value
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

        self.rect_param = ObjectRectParam::default();
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
        let value = normalize_object_int_prop(ids, op, value);
        self.runtime.explicit_int_props.insert(op);
        let ok =
            self.sync_fixed_int_prop(ids, op, value) || self.sync_special_int_prop(ids, op, value);
        assert!(ok, "unknown object int property op {}", op);
    }

    pub fn set_int_prop_from_event_frame(
        &mut self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
        value: i64,
    ) {
        self.runtime.explicit_int_props.insert(op);
        macro_rules! set_if {
            ($id:expr, $field:ident) => {
                if $id != 0 && op == $id {
                    self.base.$field = value;
                    return;
                }
            };
        }
        set_if!(ids.obj_patno, patno);
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
        set_if!(ids.obj_clip_left, clip_left);
        set_if!(ids.obj_clip_top, clip_top);
        set_if!(ids.obj_clip_right, clip_right);
        set_if!(ids.obj_clip_bottom, clip_bottom);
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
        assert!(false, "unknown object event-backed int property op {}", op);
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
        self.nested_runtime_slot
            .or(self.backend_runtime_slot)
            .unwrap_or(fallback)
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
        self.frame_action
            .counter
            .update_time(past_game_time, past_real_time);
        for fa in &mut self.frame_action_ch {
            fa.counter.update_time(past_game_time, past_real_time);
        }
        for child in &mut self.runtime.child_objects {
            child.tick(past_game_time, past_real_time);
        }
        if self.object_type == 12 {
            self.emote.tick(past_game_time);
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
    /// Original C_elm_group_param::doing flag.
    pub started: bool,
    /// Original C_elm_group_param::pause_flag. A paused group remains started
    /// but does not participate in button hit testing.
    pub pause_flag: bool,
    /// Original C_elm_group_param::target_object S_element.
    pub target_object: Vec<i32>,

    pub hit_button_no: i64,
    pub pushed_button_no: i64,
    pub decided_button_no: i64,
    pub hit_runtime_slot: Option<usize>,
    pub pushed_runtime_slot: Option<usize>,
    /// Runtime-only marker for C_elm_mwnd_msg character buttons. These do not
    /// own an OBJECT runtime slot but participate in the same group state.
    pub hit_message_button: bool,
    pub pushed_message_button: bool,
    pub message_button_se_no: i64,

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
            pause_flag: false,
            target_object: Vec::new(),
            hit_button_no: -1,
            pushed_button_no: -1,
            decided_button_no: TNM_GROUP_NOT_DECIDED,
            hit_runtime_slot: None,
            pushed_runtime_slot: None,
            hit_message_button: false,
            pushed_message_button: false,
            message_button_se_no: -1,
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
        self.pause_flag = false;
        self.wait_flag = false;
        self.cancel_flag = false;
        self.hit_button_no = -1;
        self.pushed_button_no = -1;
        self.hit_runtime_slot = None;
        self.pushed_runtime_slot = None;
        self.hit_message_button = false;
        self.pushed_message_button = false;
        self.message_button_se_no = -1;
    }

    pub fn init_sel(&mut self) {
        self.cancel_priority = 0;
        self.cancel_se_no = -1;
        self.decided_button_no = TNM_GROUP_NOT_DECIDED;
        self.result = TNM_GROUP_RESULT_NONE;
        self.result_button_no = 0;
        self.started = false;
        self.pause_flag = false;
        self.wait_flag = false;
        self.cancel_flag = false;
        self.hit_button_no = -1;
        self.pushed_button_no = -1;
        self.hit_runtime_slot = None;
        self.pushed_runtime_slot = None;
        self.hit_message_button = false;
        self.pushed_message_button = false;
        self.message_button_se_no = -1;
    }

    pub fn start(&mut self) {
        self.started = true;
        self.decided_button_no = TNM_GROUP_NOT_DECIDED;
    }

    pub fn is_doing(&self) -> bool {
        self.started && !self.pause_flag
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
        self.hit_message_button = false;
        self.pushed_message_button = false;
        self.message_button_se_no = -1;
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
        self.hit_message_button = false;
        self.pushed_message_button = false;
        self.message_button_se_no = -1;
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
    OpenWait,
    OpenNowait,
    CloseWait,
    CloseNowait,
    EndClose,
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

    SetWaku,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MwndMessageButtonState {
    pub btn_no: i64,
    pub group_no: i64,
    pub action_no: i64,
    pub se_no: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MwndGlyphState {
    /// C_tnm_moji::type: 0 normal, 1 emoji A, 2 emoji B.
    pub moji_type: i32,
    /// C_tnm_moji::code. For normal text this is the UTF-16 code unit.
    pub code: i32,
    pub ch: char,
    pub x: i64,
    pub y: i64,
    pub size: i64,
    pub moji_color_no: i64,
    pub shadow_color_no: i64,
    pub fuchi_color_no: i64,
    pub shadow: bool,
    pub fuchi: bool,
    pub bold: bool,
    /// Number of revealed body glyphs required before this glyph becomes visible.
    pub reveal_index: usize,
    pub ruby: bool,
    /// C_elm_mwnd_moji::m_appeared_flag.
    pub appeared: bool,
    /// Message-button metadata registered for this glyph.
    pub message_button: Option<MwndMessageButtonState>,
}

impl Default for MwndGlyphState {
    fn default() -> Self {
        Self {
            moji_type: 0,
            code: 0,
            ch: '\0',
            x: 0,
            y: 0,
            size: 0,
            moji_color_no: 0,
            shadow_color_no: 0,
            fuchi_color_no: -1,
            shadow: true,
            fuchi: false,
            bold: false,
            reveal_index: 1,
            ruby: false,
            appeared: false,
            message_button: None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MwndRubyPendingState {
    pub text: String,
    pub start_pos: Option<(i64, i64)>,
}

/// One completed C_elm_mwnd_msg entry retained by MULTI_MSG/NEXT_MSG.
/// The active page remains in MwndState so existing form handlers can mutate it
/// without an additional indirection; completed pages are immutable snapshots.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MwndMessagePageState {
    pub msg_text: String,
    pub glyphs: Vec<MwndGlyphState>,
    pub disp_moji_cnt: i64,
    pub hide_moji_cnt: i64,
    pub cur_msg_type: i64,
    pub cur_msg_type_decided: bool,
    pub ruby_start_pos: (i64, i64),
    pub ruby_start_ready: bool,
    pub cursor_pos: (i64, i64),
    pub moji_rep_pos: (i64, i64),
    pub indent_pos: i64,
    pub indent_moji: Option<char>,
    pub indent_count: i64,
    pub line_head: bool,
    pub ruby_pending: Option<MwndRubyPendingState>,
    pub moji_size: Option<i64>,
    pub moji_color: Option<i64>,
    pub shadow_color: Option<i64>,
    pub fuchi_color: Option<i64>,
    pub chara_moji_color: Option<i64>,
    pub chara_shadow_color: Option<i64>,
    pub chara_fuchi_color: Option<i64>,
    pub msgbtn: Option<(i64, i64, i64, i64)>,
}

#[derive(Debug, Default, Clone)]
pub struct MwndSelectionChoice {
    pub text: String,
    pub kind: i64,
    pub color: i64,
    /// Original C_elm_mwnd_select_item placement and measured extent.
    pub pos: (i64, i64),
    pub size: (i64, i64),
    /// Original per-character records retained for save/load round trips.
    pub glyphs: Vec<MwndGlyphState>,
}

#[derive(Debug, Default, Clone)]
pub struct MwndSelectionState {
    pub choices: Vec<MwndSelectionChoice>,
    /// C_elm_mwnd_select::m_disp_item_cnt. This is not the keyboard cursor.
    pub disp_item_count: usize,
    pub cursor: usize,
    pub cancel_enable: bool,
    pub close_mwnd: bool,
    /// Conservative runtime result: selected entry index (1-based), 0 for none, -1 for cancel.
    pub result: i64,
}

#[derive(Debug, Default, Clone)]
pub struct BtnSelItemState {
    pub generated_objects: Vec<ObjectState>,
    pub object_list: Vec<ObjectState>,
    pub strict: bool,
    pub text: String,
    pub item_type: i64,
    pub color: i64,
    pub pos: (i64, i64),
    pub size: (i64, i64),
    pub visible: bool,
    pub selected: bool,
    pub button_action_no: i64,
    pub button_state: i64,
    /// Per-item correction produced by C_elm_btn_select::frame.
    pub animation_offset: (i64, i64),
    /// None means the normal 255 parent TR used by non-selection BTNSELITEMs.
    pub animation_tr: Option<i64>,
}

#[derive(Debug, Default, Clone)]
pub struct MwndState {
    pub initialized_from_gameexe: bool,
    pub open: bool,
    pub name_text: String,
    /// Original name-window per-character records retained by the save stream.
    pub name_glyphs: Vec<MwndGlyphState>,
    pub msg_text: String,
    pub msg_waku_no: Option<i64>,
    pub waku_file: String,
    pub filter_file: String,
    pub filter_margin: Option<(i64, i64, i64, i64)>,
    pub filter_color: Option<(u8, u8, u8, u8)>,
    pub filter_config_color: bool,
    pub filter_config_tr: bool,
    pub waku_extend_type: i64,
    pub icon_no: i64,
    pub page_icon_no: i64,
    pub key_icon_appear: bool,
    pub key_icon_mode: i64,
    pub key_icon_pos: Option<(i64, i64)>,
    pub icon_pos_type: i64,
    pub icon_pos_base: i64,
    pub icon_pos: Option<(i64, i64, i64)>,
    /// Per-button WAKU template placement: (pos_base, x, y).
    pub waku_button_layout: Vec<(i64, i64, i64)>,
    /// Per-face WAKU template placement.
    pub waku_face_pos: Vec<(i64, i64)>,
    pub face_file: String,
    pub face_no: i64,
    pub rep_pos: Option<(i64, i64)>,
    pub msgbtn: Option<(i64, i64, i64, i64)>,
    pub window_pos: Option<(i64, i64)>,
    pub window_size: Option<(i64, i64)>,
    pub message_pos: Option<(i64, i64)>,
    pub message_margin: Option<(i64, i64, i64, i64)>,
    pub window_moji_cnt: Option<(i64, i64)>,
    pub moji_space: Option<(i64, i64)>,
    pub mwnd_extend_type: i64,
    pub multi_msg: bool,
    pub vertical_writing: bool,
    /// Completed message entries created by NEXT_MSG. C++ keeps all entries in
    /// m_msg_list and draws every one of them.
    pub message_pages: Vec<MwndMessagePageState>,
    /// Original C_elm_mwnd_msg keeps one fully styled and positioned record per glyph.
    pub glyphs: Vec<MwndGlyphState>,
    /// Original m_disp_moji_cnt and m_hide_moji_cnt.
    pub disp_moji_cnt: i64,
    pub hide_moji_cnt: i64,
    pub cur_msg_type: i64,
    pub cur_msg_type_decided: bool,
    pub ruby_start_pos: (i64, i64),
    pub ruby_start_ready: bool,
    pub cursor_pos: (i64, i64),
    pub moji_rep_pos: (i64, i64),
    pub indent_pos: i64,
    pub indent_moji: Option<char>,
    pub indent_count: i64,
    pub line_head: bool,
    pub ruby_pending: Option<MwndRubyPendingState>,
    pub ruby_size: i64,
    pub ruby_space: i64,
    pub default_moji_size: i64,
    pub default_moji_color: i64,
    pub default_shadow_color: i64,
    pub default_fuchi_color: i64,
    pub default_name_moji_color: i64,
    pub default_name_shadow_color: i64,
    pub default_name_fuchi_color: i64,
    pub koe: Option<(i64, i64)>,
    /// C++ C_elm_mwnd::get_sorter() uses an order/layer pair.
    /// There is no public MWND.ORDER script element in the recovered headers;
    /// this order is initialized from the engine MWND render defaults and is
    /// kept as runtime state so wipe/render code does not read a global table
    /// in place of the per-MWND sorter.
    pub order: i64,
    pub layer: i64,
    pub world: i64,
    pub moji_size: Option<i64>,
    pub moji_color: Option<i64>,
    pub shadow_color: Option<i64>,
    pub fuchi_color: Option<i64>,
    pub chara_color_mod: Option<i64>,
    pub chara_moji_color: Option<i64>,
    pub chara_shadow_color: Option<i64>,
    pub chara_fuchi_color: Option<i64>,
    pub name_moji_color: Option<i64>,
    pub name_shadow_color: Option<i64>,
    pub name_fuchi_color: Option<i64>,
    pub indent: bool,
    pub slide_msg: bool,
    pub slide_time: i64,
    pub open_anime_type: i64,
    pub open_anime_time: i64,
    pub close_anime_type: i64,
    pub close_anime_time: i64,
    pub selection: Option<MwndSelectionState>,
    /// Runtime-only equivalent of C_elm_mwnd::m_read_flag_stock_list.
    /// Entries are committed to GlobalState::read_flags at the same message
    /// boundaries as the original engine and are intentionally not serialized
    /// in the MWND save stream.
    pub read_flag_stock: Vec<(i64, i64)>,

    // C_elm_mwnd persistent work variables needed by the original save stream.
    pub novel_mode: i64,
    pub name_disp_mode: i64,
    pub name_bracket: i64,
    pub name_extend_type: i64,
    pub name_window_align: i64,
    pub name_window_pos: (i64, i64),
    pub name_window_size: (i64, i64),
    pub name_window_rect: (i64, i64, i64, i64),
    pub name_message_pos: (i64, i64),
    pub name_message_pos_rep: (i64, i64),
    pub name_message_margin: (i64, i64, i64, i64),
    pub overflow_check_size: i64,
    pub face_hide_name: i64,
    pub time: i64,
    pub auto_proc_ready: bool,
    pub window_appear: bool,
    pub name_appear: bool,
    pub auto_mode_end_moji_cnt: i64,
    pub target_msg_no: i64,
    pub koe_play_flag: bool,
    pub open_anime_start_time: i64,
    pub close_anime_start_time: i64,

    pub text_dirty: bool,
    pub clear_ready: bool,
    pub msg_block_started: bool,

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
    /// Backend object-slot namespace for this stage form. The original engine
    /// owns separate C_elm_stage_list instances for normal and EXCALL scenes;
    /// the Rust GfxRuntime is shared, so EXCALL must not reuse normal slots.
    pub backend_slot_base: usize,
    /// C++ C_elm_stage_list::init creates BACK/FRONT/NEXT sub stages eagerly.
    pub initialized_from_gameexe: bool,
    /// Group list storage per stage index.
    pub group_lists: HashMap<i64, Vec<GroupState>>,
    /// BTNSELITEM list storage per stage index.
    pub btnselitem_lists: HashMap<i64, Vec<BtnSelItemState>>,
    /// Persistent C_elm_stage::m_btn_sel state per stage.  The global SELBTN
    /// command operates on FRONT, but stage wipe copies the whole selector to
    /// NEXT/FRONT/BACK and the original local save serializes BACK and FRONT
    /// independently.
    pub btn_select_states: HashMap<i64, BtnSelectRuntimeState>,
    /// MWND list storage per stage index.
    pub mwnd_lists: HashMap<i64, Vec<MwndState>>,
    /// World list storage per stage index.
    pub world_lists: HashMap<i64, Vec<WorldState>>,
    /// Effect list storage per stage index. Mirrors C_elm_stage::m_effect_list.
    pub effect_lists: HashMap<i64, Vec<ScreenEffectState>>,
    /// Quake list storage per stage index. Mirrors C_elm_stage::m_quake_list.
    pub quake_lists: HashMap<i64, Vec<ScreenQuakeState>>,
    // --- OBJECT / OBJECTLIST ---
    /// Per-stage object state (string objects, rect objects, nested child objects, etc.).
    pub object_lists: HashMap<i64, Vec<ObjectState>>,
    /// Fixed per-slot C++ C_elm_object::is_use() flags, separated from
    /// ObjectState::used.  The latter is an active/runtime flag in this port;
    /// C++ stage wipe gates on the slot enable flag initialized by C_elm_object_list.
    pub object_slot_use: HashMap<i64, Vec<bool>>,
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

pub fn normalize_screen_effect_scalar(
    ids: &crate::runtime::constants::RuntimeConstants,
    op: i32,
    value: i32,
) -> i32 {
    let byte_range = [
        ids.effect_mono,
        ids.effect_mono_eve,
        ids.effect_reverse,
        ids.effect_reverse_eve,
        ids.effect_bright,
        ids.effect_bright_eve,
        ids.effect_dark,
        ids.effect_dark_eve,
        ids.effect_color_r,
        ids.effect_color_r_eve,
        ids.effect_color_g,
        ids.effect_color_g_eve,
        ids.effect_color_b,
        ids.effect_color_b_eve,
        ids.effect_color_rate,
        ids.effect_color_rate_eve,
        ids.effect_color_add_r,
        ids.effect_color_add_r_eve,
        ids.effect_color_add_g,
        ids.effect_color_add_g_eve,
        ids.effect_color_add_b,
        ids.effect_color_add_b_eve,
    ];
    if byte_range.iter().any(|&id| id != 0 && op == id) {
        value.clamp(0, 255)
    } else {
        value
    }
}

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
            begin_layer: i32::MIN,
            end_order: 0,
            end_layer: i32::MAX,
            wipe_copy: 0,
            wipe_erase: 0,
        }
    }
}
impl ScreenEffectState {
    pub fn int_event_by_op(
        &self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
    ) -> Option<&IntEvent> {
        match op {
            s if s == ids.effect_x || s == ids.effect_x_eve => Some(&self.x),
            s if s == ids.effect_y || s == ids.effect_y_eve => Some(&self.y),
            s if s == ids.effect_z || s == ids.effect_z_eve => Some(&self.z),
            s if s == ids.effect_mono || s == ids.effect_mono_eve => Some(&self.mono),
            s if s == ids.effect_reverse || s == ids.effect_reverse_eve => Some(&self.reverse),
            s if s == ids.effect_bright || s == ids.effect_bright_eve => Some(&self.bright),
            s if s == ids.effect_dark || s == ids.effect_dark_eve => Some(&self.dark),
            s if s == ids.effect_color_r || s == ids.effect_color_r_eve => Some(&self.color_r),
            s if s == ids.effect_color_g || s == ids.effect_color_g_eve => Some(&self.color_g),
            s if s == ids.effect_color_b || s == ids.effect_color_b_eve => Some(&self.color_b),
            s if s == ids.effect_color_rate || s == ids.effect_color_rate_eve => {
                Some(&self.color_rate)
            }
            s if s == ids.effect_color_add_r || s == ids.effect_color_add_r_eve => {
                Some(&self.color_add_r)
            }
            s if s == ids.effect_color_add_g || s == ids.effect_color_add_g_eve => {
                Some(&self.color_add_g)
            }
            s if s == ids.effect_color_add_b || s == ids.effect_color_add_b_eve => {
                Some(&self.color_add_b)
            }
            _ => None,
        }
    }

    pub fn int_event_by_op_mut(
        &mut self,
        ids: &crate::runtime::constants::RuntimeConstants,
        op: i32,
    ) -> Option<&mut IntEvent> {
        match op {
            s if s == ids.effect_x || s == ids.effect_x_eve => Some(&mut self.x),
            s if s == ids.effect_y || s == ids.effect_y_eve => Some(&mut self.y),
            s if s == ids.effect_z || s == ids.effect_z_eve => Some(&mut self.z),
            s if s == ids.effect_mono || s == ids.effect_mono_eve => Some(&mut self.mono),
            s if s == ids.effect_reverse || s == ids.effect_reverse_eve => Some(&mut self.reverse),
            s if s == ids.effect_bright || s == ids.effect_bright_eve => Some(&mut self.bright),
            s if s == ids.effect_dark || s == ids.effect_dark_eve => Some(&mut self.dark),
            s if s == ids.effect_color_r || s == ids.effect_color_r_eve => Some(&mut self.color_r),
            s if s == ids.effect_color_g || s == ids.effect_color_g_eve => Some(&mut self.color_g),
            s if s == ids.effect_color_b || s == ids.effect_color_b_eve => Some(&mut self.color_b),
            s if s == ids.effect_color_rate || s == ids.effect_color_rate_eve => {
                Some(&mut self.color_rate)
            }
            s if s == ids.effect_color_add_r || s == ids.effect_color_add_r_eve => {
                Some(&mut self.color_add_r)
            }
            s if s == ids.effect_color_add_g || s == ids.effect_color_add_g_eve => {
                Some(&mut self.color_add_g)
            }
            s if s == ids.effect_color_add_b || s == ids.effect_color_add_b_eve => {
                Some(&mut self.color_add_b)
            }
            _ => None,
        }
    }

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

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScreenQuakeTransform {
    pub x: i32,
    pub y: i32,
    pub scale: f32,
    pub rotate_degrees: f32,
    pub center_x: i32,
    pub center_y: i32,
}

#[derive(Debug, Clone)]
pub struct ScreenQuakeState {
    pub quake_type: i32,
    pub vec: i32,
    pub power: i32,
    pub cur_time: i32,
    pub total_time: i32,
    pub ending: bool,
    pub end_cur_time: i32,
    pub end_total_time: i32,
    pub cnt: i32,
    pub end_cnt: i32,
    pub center_x: i32,
    pub center_y: i32,
    pub begin_order: i32,
    pub end_order: i32,
}

impl Default for ScreenQuakeState {
    fn default() -> Self {
        Self {
            quake_type: -1,
            vec: 0,
            power: 0,
            cur_time: 0,
            total_time: 0,
            ending: false,
            end_cur_time: 0,
            end_total_time: 0,
            cnt: 0,
            end_cnt: 0,
            center_x: 0,
            center_y: 0,
            begin_order: 0,
            end_order: 0,
        }
    }
}

fn speed_up_limit_i32(now: i32, start: i32, start_value: i32, end: i32, end_value: i32) -> i32 {
    if start == end {
        return end_value;
    }
    let lo = start.min(end) as f64;
    let hi = start.max(end) as f64;
    let t = (now as f64).clamp(lo, hi);
    let start = start as f64;
    let end = end as f64;
    let start_value = start_value as f64;
    let end_value = end_value as f64;
    (((t - start) * (t - start) * (end_value - start_value) / ((end - start) * (end - start)))
        + start_value) as i32
}

fn speed_down_limit_i32(now: i32, start: i32, start_value: i32, end: i32, end_value: i32) -> i32 {
    if start == end {
        return end_value;
    }
    let lo = start.min(end) as f64;
    let hi = start.max(end) as f64;
    let t = (now as f64).clamp(lo, hi);
    let start = start as f64;
    let end = end as f64;
    let start_value = start_value as f64;
    let end_value = end_value as f64;
    (-(t - end) * (t - end) * (end_value - start_value) / ((end - start) * (end - start))
        + end_value) as i32
}

fn linear_limit_f64(now: i32, start: i32, start_value: f64, end: i32, end_value: f64) -> f64 {
    if start == end {
        return end_value;
    }
    if now <= start.min(end) {
        return if start < end { start_value } else { end_value };
    }
    if now >= start.max(end) {
        return if start < end { end_value } else { start_value };
    }
    (end_value - start_value) * (now - start) as f64 / (end - start) as f64 + start_value
}

impl ScreenQuakeState {
    pub fn reinit(&mut self) {
        *self = Self::default();
    }

    pub fn start_kind(&mut self, quake_type: i32, time_ms: i64, cnt: i32, end_cnt: i32) {
        self.quake_type = quake_type;
        self.cur_time = 0;
        self.total_time = time_ms.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
        self.cnt = cnt;
        self.end_cnt = end_cnt;
        self.ending = false;
        self.end_cur_time = 0;
        self.end_total_time = 0;
        if self.total_time <= 0 || !(0..=3).contains(&self.quake_type) {
            self.reinit();
        }
    }

    pub fn end_ms(&mut self, time_ms: i64) {
        let time_ms = time_ms.clamp(0, i32::MAX as i64) as i32;
        if time_ms == 0 {
            self.reinit();
            return;
        }
        if self.quake_type < 0 {
            return;
        }
        self.ending = true;
        self.end_cur_time = 0;
        self.end_total_time = time_ms;
    }

    pub fn check_value(&self) -> i32 {
        if self.quake_type < 0 {
            0
        } else if self.ending {
            2
        } else {
            1
        }
    }

    pub fn is_active(&self) -> bool {
        self.quake_type >= 0 && self.total_time > 0
    }

    pub fn is_infinite(&self) -> bool {
        self.is_active() && self.cnt == 0 && self.end_cnt == 0 && !self.ending
    }

    pub fn remaining_ms(&self) -> Option<u64> {
        if !self.is_active() {
            return Some(0);
        }
        if self.ending {
            return Some(self.end_total_time.saturating_sub(self.end_cur_time).max(0) as u64);
        }
        if self.cnt == 0 && self.end_cnt == 0 {
            return None;
        }
        Some(
            self.total_time
                .saturating_mul(self.cnt.saturating_add(self.end_cnt))
                .saturating_sub(self.cur_time)
                .max(0) as u64,
        )
    }

    pub fn tick(&mut self, delta_ms: i32) {
        if !self.is_active() {
            return;
        }
        let delta_ms = delta_ms.max(0);
        self.cur_time = self.cur_time.saturating_add(delta_ms);
        if self.ending {
            self.end_cur_time = self.end_cur_time.saturating_add(delta_ms);
        }
        let loop_forever = self.cnt == 0 && self.end_cnt == 0;
        if (!loop_forever
            && self.cur_time
                >= self
                    .total_time
                    .saturating_mul(self.cnt.saturating_add(self.end_cnt)))
            || (self.ending && self.end_cur_time >= self.end_total_time)
        {
            self.reinit();
        }
    }

    pub fn transform(&self) -> ScreenQuakeTransform {
        const SCALE_UNIT: i32 = 1000;
        if !self.is_active() {
            return ScreenQuakeTransform {
                scale: 1.0,
                ..ScreenQuakeTransform::default()
            };
        }
        let quarter = (self.total_time / 4).max(1);
        let jump = self.cur_time.rem_euclid(self.total_time.max(1));
        let (x_sign, y_sign) = match self.vec {
            0 => (0, -1),
            1 => (1, -1),
            2 => (1, 0),
            3 => (1, 1),
            4 => (0, 1),
            5 => (-1, 1),
            6 => (-1, 0),
            7 => (-1, -1),
            _ => (0, 0),
        };
        let mut x = 0i32;
        let mut y = 0i32;
        let mut scale = SCALE_UNIT;
        let mut rotate = 0i32;
        match self.quake_type {
            0 => {
                let value = if jump < self.total_time / 4 {
                    speed_up_limit_i32(jump, 0, 0, quarter, self.power / 2)
                } else if jump < self.total_time / 2 {
                    speed_down_limit_i32(
                        jump - self.total_time / 4,
                        0,
                        self.power / 2,
                        quarter,
                        self.power,
                    )
                } else if jump < self.total_time * 3 / 4 {
                    speed_up_limit_i32(
                        jump - self.total_time / 2,
                        0,
                        self.power,
                        quarter,
                        self.power / 2,
                    )
                } else {
                    speed_down_limit_i32(
                        jump - self.total_time * 3 / 4,
                        0,
                        self.power / 2,
                        quarter,
                        0,
                    )
                };
                x = value.saturating_mul(x_sign);
                y = value.saturating_mul(y_sign);
            }
            1 => {
                let value = if jump < self.total_time / 4 {
                    speed_down_limit_i32(jump, 0, 0, quarter, self.power / 2)
                } else if jump < self.total_time / 2 {
                    speed_up_limit_i32(jump - self.total_time / 4, 0, self.power / 2, quarter, 0)
                } else if jump < self.total_time * 3 / 4 {
                    speed_down_limit_i32(jump - self.total_time / 2, 0, 0, quarter, -self.power / 2)
                } else {
                    speed_up_limit_i32(
                        jump - self.total_time * 3 / 4,
                        0,
                        -self.power / 2,
                        quarter,
                        0,
                    )
                };
                x = value.saturating_mul(x_sign);
                y = value.saturating_mul(y_sign);
            }
            2 => {
                let power = self.power.clamp(0, 255);
                let max_scale = 256 * SCALE_UNIT / (256 - power);
                let half_scale = (max_scale - SCALE_UNIT) / 2 + SCALE_UNIT;
                scale = if jump < self.total_time / 4 {
                    speed_up_limit_i32(jump, 0, SCALE_UNIT, quarter, half_scale)
                } else if jump < self.total_time / 2 {
                    speed_down_limit_i32(
                        jump - self.total_time / 4,
                        0,
                        half_scale,
                        quarter,
                        max_scale,
                    )
                } else if jump < self.total_time * 3 / 4 {
                    speed_up_limit_i32(
                        jump - self.total_time / 2,
                        0,
                        max_scale,
                        quarter,
                        half_scale,
                    )
                } else {
                    speed_down_limit_i32(
                        jump - self.total_time * 3 / 4,
                        0,
                        half_scale,
                        quarter,
                        SCALE_UNIT,
                    )
                };
            }
            // TNM_QUAKE_TYPE_ROTATE exists in the C++ enum/save structure,
            // but the original frame routine has no rotate branch. Imported
            // state therefore remains active without altering the render transform.
            3 => {}
            _ => {}
        }

        let loop_forever = self.cnt == 0 && self.end_cnt == 0;
        if !loop_forever && self.cur_time >= self.total_time.saturating_mul(self.cnt) {
            let fade = linear_limit_f64(
                self.cur_time,
                self.total_time.saturating_mul(self.cnt),
                1.0,
                self.total_time
                    .saturating_mul(self.cnt.saturating_add(self.end_cnt)),
                0.0,
            );
            x = (x as f64 * fade) as i32;
            y = (y as f64 * fade) as i32;
            scale = ((scale - SCALE_UNIT) as f64 * fade + SCALE_UNIT as f64) as i32;
            rotate = (rotate as f64 * fade) as i32;
        }
        if self.ending && self.end_total_time > 0 {
            let fade = linear_limit_f64(self.end_cur_time, 0, 1.0, self.end_total_time, 0.0);
            x = (x as f64 * fade) as i32;
            y = (y as f64 * fade) as i32;
            scale = ((scale - SCALE_UNIT) as f64 * fade + SCALE_UNIT as f64) as i32;
            rotate = (rotate as f64 * fade) as i32;
        }

        ScreenQuakeTransform {
            x,
            y,
            scale: scale as f32 / SCALE_UNIT as f32,
            rotate_degrees: rotate as f32 / 1000.0,
            center_x: self.center_x,
            center_y: self.center_y,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScreenShakeState {
    pub shake_no: i32,
    pub cur_time: i32,
    pub cur_x: i32,
    pub cur_y: i32,
}

impl ScreenShakeState {
    pub fn init() -> Self {
        Self {
            shake_no: -1,
            cur_time: 0,
            cur_x: 0,
            cur_y: 0,
        }
    }

    pub fn start(&mut self, shake_no: i64, table_count: usize) -> bool {
        if shake_no < 0 || shake_no as usize >= table_count {
            return false;
        }
        self.shake_no = shake_no as i32;
        self.cur_time = 0;
        self.cur_x = 0;
        self.cur_y = 0;
        true
    }

    pub fn end(&mut self) {
        *self = Self::init();
    }

    pub fn tick(&mut self, delta_ms: i32, templates: &[Vec<crate::runtime::tables::ShakeStep>]) {
        if self.shake_no < 0 || self.shake_no as usize >= templates.len() {
            self.end();
            return;
        }
        self.cur_time = self.cur_time.saturating_add(delta_ms.max(0));
        let mut total = 0i32;
        let mut found = None;
        for step in &templates[self.shake_no as usize] {
            let period_time = self.cur_time.saturating_sub(total);
            if period_time < step.time_ms {
                found = Some((step.x, step.y));
                break;
            }
            total = total.saturating_add(step.time_ms.max(0));
        }
        if let Some((x, y)) = found {
            self.cur_x = x;
            self.cur_y = y;
        } else {
            self.end();
        }
    }

    pub fn is_active(&self) -> bool {
        self.shake_no >= 0
    }
}

impl Default for ScreenShakeState {
    fn default() -> Self {
        Self::init()
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

    pub fn tick(&mut self, delta: i32, shake_templates: &[Vec<crate::runtime::tables::ShakeStep>]) {
        for effect in &mut self.effect_list {
            effect.tick(delta);
        }
        for quake in &mut self.quake_list {
            quake.tick(delta);
        }
        self.shake.tick(delta, shake_templates);
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
    pub save_id: i64,
    pub save_id_check_flag: bool,
}

#[derive(Debug, Clone)]
pub struct MsgBackState {
    pub history: Vec<MsgBackEntry>,
    pub history_cnt_max: usize,
    pub history_cnt: usize,
    pub history_start_pos: usize,
    pub history_insert_pos: usize,
    pub history_last_pos: usize,
    pub new_msg_flag: bool,
}

impl Default for MsgBackState {
    fn default() -> Self {
        let history_cnt_max = 256usize;
        Self {
            history: vec![
                MsgBackEntry {
                    scn_no: -1,
                    line_no: -1,
                    ..MsgBackEntry::default()
                };
                history_cnt_max
            ],
            history_cnt_max,
            history_cnt: 0,
            history_start_pos: 0,
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

    fn ensure_capacity(&mut self) {
        if self.history_cnt_max == 0 {
            self.history_cnt_max = 256;
        }
        if self.history.len() != self.history_cnt_max {
            self.history
                .resize_with(self.history_cnt_max, || MsgBackEntry {
                    scn_no: -1,
                    line_no: -1,
                    ..MsgBackEntry::default()
                });
        }
        self.history_insert_pos %= self.history_cnt_max;
        self.history_start_pos %= self.history_cnt_max;
        self.history_last_pos %= self.history_cnt_max;
    }

    pub fn set_history_cnt_max(&mut self, max_count: usize) {
        let max_count = max_count.max(1);
        if max_count == self.history_cnt_max {
            self.ensure_capacity();
            return;
        }
        let mut ordered = self
            .ordered_history_indices()
            .into_iter()
            .filter_map(|idx| self.history.get(idx).cloned())
            .collect::<Vec<_>>();
        if ordered.len() > max_count {
            let drop_count = ordered.len() - max_count;
            ordered.drain(0..drop_count);
        }
        self.history_cnt_max = max_count;
        self.history = vec![
            MsgBackEntry {
                scn_no: -1,
                line_no: -1,
                ..MsgBackEntry::default()
            };
            max_count
        ];
        self.history_cnt = ordered.len();
        self.history_start_pos = 0;
        for (i, entry) in ordered.into_iter().enumerate() {
            self.history[i] = entry;
        }
        self.history_insert_pos = self.history_cnt % self.history_cnt_max;
        self.history_last_pos = self
            .history_cnt
            .saturating_sub(1)
            .min(self.history_cnt_max - 1);
        self.new_msg_flag = true;
    }

    fn ready_msg(&mut self) -> &mut MsgBackEntry {
        self.ensure_capacity();
        if self.new_msg_flag {
            if self.history_cnt < self.history_cnt_max {
                self.history_cnt += 1;
            } else {
                self.history_start_pos = (self.history_start_pos + 1) % self.history_cnt_max;
            }
            Self::reset_entry(&mut self.history[self.history_insert_pos]);
            self.new_msg_flag = false;
        }
        &mut self.history[self.history_insert_pos]
    }

    pub fn clear(&mut self) {
        self.ensure_capacity();
        for entry in &mut self.history {
            Self::reset_entry(entry);
        }
        self.history_cnt = 0;
        self.history_start_pos = 0;
        self.history_insert_pos = 0;
        self.history_last_pos = 0;
        self.new_msg_flag = true;
    }

    pub fn ordered_history_indices(&self) -> Vec<usize> {
        if self.history_cnt_max == 0 || self.history_cnt == 0 {
            return Vec::new();
        }
        (0..self.history_cnt)
            .map(|i| (self.history_start_pos + i) % self.history_cnt_max)
            .collect()
    }

    pub fn next(&mut self) {
        self.ensure_capacity();
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
        self.history_insert_pos = (self.history_insert_pos + 1) % self.history_cnt_max;
        self.new_msg_flag = true;
    }

    pub fn add_koe(&mut self, koe_no: i64, chara_no: i64, scn_no: i64, line_no: i64) -> bool {
        if koe_no < 0 {
            return true;
        }
        let insert_pos = self.history_insert_pos;
        let entry = self.ready_msg();
        entry.koe_no_list.push(koe_no);
        entry.chr_no_list.push(chara_no);
        entry.scn_no = scn_no;
        entry.line_no = line_no;
        self.history_last_pos = insert_pos;
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
        let insert_pos = self.history_insert_pos;
        let entry = self.ready_msg();
        entry.original_name.clear();
        entry.original_name.push_str(original_name);
        entry.disp_name.clear();
        entry.disp_name.push_str(disp_name);
        entry.scn_no = scn_no;
        entry.line_no = line_no;
        self.history_last_pos = insert_pos;
        true
    }

    pub fn add_msg(&mut self, msg: &str, debug_msg: &str, scn_no: i64, line_no: i64) -> bool {
        if msg.is_empty() {
            return true;
        }
        let insert_pos = self.history_insert_pos;
        let entry = self.ready_msg();
        entry.msg_str.push_str(msg);
        entry.debug_msg.clear();
        entry.debug_msg.push_str(debug_msg);
        entry.scn_no = scn_no;
        entry.line_no = line_no;
        self.history_last_pos = insert_pos;
        true
    }

    pub fn add_new_line_indent(&mut self, scn_no: i64, line_no: i64) -> bool {
        let insert_pos = self.history_insert_pos;
        let entry = self.ready_msg();
        entry.msg_str.push('\n');
        entry.scn_no = scn_no;
        entry.line_no = line_no;
        self.history_last_pos = insert_pos;
        true
    }

    pub fn add_new_line_no_indent(&mut self, scn_no: i64, line_no: i64) -> bool {
        let insert_pos = self.history_insert_pos;
        let entry = self.ready_msg();
        entry.msg_str.push('\u{0007}');
        entry.scn_no = scn_no;
        entry.line_no = line_no;
        self.history_last_pos = insert_pos;
        true
    }

    pub fn add_pct(&mut self, file_name: &str, x: i32, y: i32) -> bool {
        if file_name.is_empty() {
            return false;
        }
        self.next();
        let insert_pos = self.history_insert_pos;
        let entry = self.ready_msg();
        entry.pct_flag = true;
        entry.pct_pos_x = x;
        entry.pct_pos_y = y;
        entry.msg_str.clear();
        entry.msg_str.push_str(file_name);
        self.history_last_pos = insert_pos;
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

    pub fn ensure_effect_list(&mut self, stage_idx: i64, cnt: usize) {
        let entry = self.effect_lists.entry(stage_idx).or_default();
        if entry.len() < cnt {
            entry.extend((0..(cnt - entry.len())).map(|_| ScreenEffectState::default()));
        } else if entry.len() > cnt {
            entry.truncate(cnt);
        }
    }

    pub fn ensure_quake_list(&mut self, stage_idx: i64, cnt: usize) {
        let entry = self.quake_lists.entry(stage_idx).or_default();
        if entry.len() < cnt {
            entry.extend((0..(cnt - entry.len())).map(|_| ScreenQuakeState::default()));
        } else if entry.len() > cnt {
            entry.truncate(cnt);
        }
    }

    pub fn close_all_mwnd(&mut self, stage_idx: i64) {
        if let Some(list) = self.mwnd_lists.get_mut(&stage_idx) {
            for (idx, m) in list.iter_mut().enumerate() {
                let old_open = m.open;
                m.open = false;
                if trace_env().sg_debug {
                    eprintln!(
                        "[SG_DEBUG][MWND_STATE_TRACE] scene=<runtime> scene_no=- line=- reason=STAGE_CLOSE_ALL_MWND stage={} mwnd={} old_open={} new_open={} buttons={} faces={} objects={} waku={} filter={} pos={:?} size={:?} open_anim=({}, {}) close_anim=({}, {}) selection={} msg_len={} name_len={}",
                        stage_idx,
                        idx,
                        old_open,
                        m.open,
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

        let slot_use = self.object_slot_use.entry(stage_idx).or_default();
        if slot_use.len() < cnt {
            slot_use.extend((0..(cnt - slot_use.len())).map(|_| true));
        } else if slot_use.len() > cnt {
            slot_use.truncate(cnt);
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

    pub fn is_embedded_object_slot(&self, stage_idx: i64, slot: usize) -> bool {
        self.embedded_object_slots
            .iter()
            .any(|(key, &mapped_slot)| {
                mapped_slot == slot && key.starts_with(&format!("{stage_idx}:"))
            })
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

#[derive(Debug, Clone, Copy)]
pub struct ExcallTickTargets {
    pub counter_list_id: u32,
    pub frame_action_id: u32,
    pub frame_action_ch_list_id: u32,
    pub stage_form_id: u32,
}

impl GlobalState {
    pub fn ensure_read_flag_count(&mut self, scene_no: i64, flag_count: usize) {
        if scene_no < 0 {
            return;
        }
        let row = self.read_flags.entry(scene_no).or_default();
        row.resize(flag_count, 0);
    }

    pub fn set_read_flag(&mut self, scene_no: i64, flag_no: i64) -> bool {
        if scene_no < 0 || flag_no < 0 {
            return false;
        }
        let Ok(flag_no) = usize::try_from(flag_no) else {
            return false;
        };
        let row = self.read_flags.entry(scene_no).or_default();
        if row.len() <= flag_no {
            // The number is supplied by the current scene lexer immediately
            // after the command.  Grow lazily so a valid flag can be committed
            // even before the full Scene.pck table has been initialized.
            row.resize(flag_no + 1, 0);
        }
        row[flag_no] = 1;
        true
    }

    pub fn read_flag(&self, scene_no: i64, flag_no: i64) -> bool {
        if scene_no < 0 || flag_no < 0 {
            return false;
        }
        self.read_flags
            .get(&scene_no)
            .and_then(|row| row.get(flag_no as usize))
            .copied()
            .unwrap_or(0)
            != 0
    }

    pub fn start_wipe(&mut self, w: WipeState) {
        self.wipe = Some(w);
    }

    pub fn finish_wipe(&mut self) {
        self.wipe = None;
    }

    pub fn wipe_done(&self) -> bool {
        self.wipe.as_ref().map(|w| w.is_done()).unwrap_or(true)
    }

    pub fn tick_frame(
        &mut self,
        past_game_time: i32,
        past_real_time: i32,
        shake_templates: &[Vec<crate::runtime::tables::ShakeStep>],
        excall: Option<ExcallTickTargets>,
    ) {
        // C++ eng_frame.cpp has two time domains here: ordinary/local elements
        // obey the local time-stop flags, while an allocated EXCALL runtime
        // advances independently. System scenes (Config/Save/Load) depend on
        // that distinction while the gameplay scene below them is frozen.
        let wipe_active = self.wipe.is_some();
        self.render_frame = self.render_frame.wrapping_add(1);
        self.local_real_time = self
            .local_real_time
            .saturating_add(past_real_time.max(0) as i64);
        self.local_game_time = self
            .local_game_time
            .saturating_add(past_game_time.max(0) as i64);
        let past_wipe_time = past_game_time.max(0);
        self.local_wipe_time = self.local_wipe_time.saturating_add(past_wipe_time as i64);
        if let Some(wipe) = self.wipe.as_mut() {
            wipe.advance(past_wipe_time);
        }
        // A completed wipe is torn down by CommandContext::tick_frame(), which
        // also performs the original stage[NEXT].reinit(false) semantics.
        if self.change_display_mode_proc_cnt > 0 {
            self.change_display_mode_proc_cnt -= 1;
        }
        self.mov.tick(past_real_time);

        let excall_counter_id = excall.map(|v| v.counter_list_id);
        let excall_frame_action_id = excall.map(|v| v.frame_action_id);
        let excall_frame_action_ch_list_id = excall.map(|v| v.frame_action_ch_list_id);
        let excall_stage_form_id = excall.map(|v| v.stage_form_id);

        // Ordinary/local elements. C++ skips this block while
        // m_local.pod.time_stop_flag is set, then still runs the EXCALL block.
        if !self.script.time_stop_flag {
            self.fog_global.update_time(past_game_time, past_real_time);
            self.fog_global.frame();

            if !self.script.counter_time_stop_flag {
                let mut counter_ids: Vec<u32> = self.counter_lists.keys().copied().collect();
                counter_ids.sort_unstable();
                for counter_id in counter_ids {
                    if Some(counter_id) == excall_counter_id {
                        continue;
                    }
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
                    if Some(frame_action_id) == excall_frame_action_id {
                        continue;
                    }
                    let Some(fa) = self.frame_actions.get_mut(&frame_action_id) else {
                        continue;
                    };
                    fa.counter.update_time(past_game_time, past_real_time);
                }
                let mut frame_action_list_ids: Vec<u32> =
                    self.frame_action_lists.keys().copied().collect();
                frame_action_list_ids.sort_unstable();
                for frame_action_list_id in frame_action_list_ids {
                    if Some(frame_action_list_id) == excall_frame_action_ch_list_id {
                        continue;
                    }
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
                sc.tick(past_game_time.max(0), shake_templates);
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

            if !self.script.stage_time_stop_flag {
                let mut stage_form_ids: Vec<u32> = self.stage_forms.keys().copied().collect();
                stage_form_ids.sort_unstable();
                for stage_form_id in stage_form_ids {
                    if Some(stage_form_id) == excall_stage_form_id {
                        continue;
                    }
                    self.tick_stage_form(
                        stage_form_id,
                        past_game_time,
                        past_real_time,
                        wipe_active,
                    );
                }
            }
        }

        // Original eng_frame.cpp updates EXCALL whenever m_excall.is_ready(),
        // regardless of local time_stop/counter_time_stop/frame_action_time_stop/
        // stage_time_stop. This is essential for Config animations over a
        // deliberately frozen gameplay scene.
        if let Some(excall) = excall {
            if let Some(counters) = self.counter_lists.get_mut(&excall.counter_list_id) {
                for counter in counters {
                    counter.update_time(past_game_time, past_real_time);
                }
            }
            if let Some(fa) = self.frame_actions.get_mut(&excall.frame_action_id) {
                fa.counter.update_time(past_game_time, past_real_time);
            }
            if let Some(list) = self
                .frame_action_lists
                .get_mut(&excall.frame_action_ch_list_id)
            {
                for fa in list {
                    fa.counter.update_time(past_game_time, past_real_time);
                }
            }
            self.tick_stage_form(
                excall.stage_form_id,
                past_game_time,
                past_real_time,
                wipe_active,
            );
        }
    }

    fn tick_stage_form(
        &mut self,
        stage_form_id: u32,
        past_game_time: i32,
        past_real_time: i32,
        wipe_active: bool,
    ) {
        let Some(st) = self.stage_forms.get_mut(&stage_form_id) else {
            return;
        };

        let mut object_stage_ids: Vec<i64> = st.object_lists.keys().copied().collect();
        object_stage_ids.sort_unstable();
        for object_stage_id in object_stage_ids {
            if object_stage_id == 2 && !wipe_active {
                continue;
            }
            let embedded_prefix = format!("{object_stage_id}:");
            let embedded_slots: HashSet<usize> = st
                .embedded_object_slots
                .iter()
                .filter_map(|(key, &slot)| key.starts_with(&embedded_prefix).then_some(slot))
                .collect();
            let Some(objs) = st.object_lists.get_mut(&object_stage_id) else {
                continue;
            };
            for (obj_idx, obj) in objs.iter_mut().enumerate() {
                if embedded_slots.contains(&obj_idx) {
                    continue;
                }
                obj.tick(past_game_time, past_real_time);
            }
        }

        let mut mwnd_stage_ids: Vec<i64> = st.mwnd_lists.keys().copied().collect();
        mwnd_stage_ids.sort_unstable();
        for mwnd_stage_id in mwnd_stage_ids {
            if mwnd_stage_id == 2 && !wipe_active {
                continue;
            }
            let Some(mwnds) = st.mwnd_lists.get_mut(&mwnd_stage_id) else {
                continue;
            };
            for mwnd in mwnds {
                for obj in &mut mwnd.button_list {
                    obj.tick(past_game_time, past_real_time);
                }
                for obj in &mut mwnd.face_list {
                    obj.tick(past_game_time, past_real_time);
                }
                for obj in &mut mwnd.object_list {
                    obj.tick(past_game_time, past_real_time);
                }
            }
        }

        let mut world_stage_ids: Vec<i64> = st.world_lists.keys().copied().collect();
        world_stage_ids.sort_unstable();
        for world_stage_id in world_stage_ids {
            if world_stage_id == 2 && !wipe_active {
                continue;
            }
            let Some(worlds) = st.world_lists.get_mut(&world_stage_id) else {
                continue;
            };
            for w in worlds {
                w.update_time(past_game_time, past_real_time);
                w.frame();
            }
        }

        let mut effect_stage_ids: Vec<i64> = st.effect_lists.keys().copied().collect();
        effect_stage_ids.sort_unstable();
        for effect_stage_id in effect_stage_ids {
            if effect_stage_id == 2 && !wipe_active {
                continue;
            }
            let Some(effects) = st.effect_lists.get_mut(&effect_stage_id) else {
                continue;
            };
            for effect in effects {
                effect.tick(past_game_time.max(0));
            }
        }

        let mut quake_stage_ids: Vec<i64> = st.quake_lists.keys().copied().collect();
        quake_stage_ids.sort_unstable();
        for quake_stage_id in quake_stage_ids {
            if quake_stage_id == 2 && !wipe_active {
                continue;
            }
            let Some(quakes) = st.quake_lists.get_mut(&quake_stage_id) else {
                continue;
            };
            for quake in quakes {
                quake.tick(past_game_time.max(0));
            }
        }
    }
}

#[cfg(test)]
mod wipe_stage_tick_tests {
    use super::{ExcallTickTargets, GlobalState, StageFormState, WipeState, WorldState};

    const TEST_STAGE_FORM_ID: u32 = 49;
    const NEXT_STAGE: i64 = 2;

    fn next_world_event_time(globals: &GlobalState) -> i32 {
        globals.stage_forms[&TEST_STAGE_FORM_ID].world_lists[&NEXT_STAGE][0]
            .camera_eye_x
            .cur_time
    }

    #[test]
    fn next_stage_events_advance_only_while_wipe_is_active() {
        let mut globals = GlobalState::default();
        let mut stage = StageFormState::default();
        let mut world = WorldState::new(0);
        world.camera_eye_x.set_event(100, 1_000, 0, 0, 0);
        stage.world_lists.insert(NEXT_STAGE, vec![world]);
        globals.stage_forms.insert(TEST_STAGE_FORM_ID, stage);

        globals.tick_frame(10, 10, &[], None);
        assert_eq!(next_world_event_time(&globals), 0);

        globals.start_wipe(WipeState::new(
            TEST_STAGE_FORM_ID,
            None,
            None,
            0,
            1_000,
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
        globals.tick_frame(10, 10, &[], None);
        assert_eq!(next_world_event_time(&globals), 10);

        globals.finish_wipe();
        globals.tick_frame(10, 10, &[], None);
        assert_eq!(next_world_event_time(&globals), 10);
    }

    #[test]
    fn main_time_stop_freezes_stage_but_not_wipe_clock() {
        let mut globals = GlobalState::default();
        let mut stage = StageFormState::default();
        let mut world = WorldState::new(0);
        world.camera_eye_x.set_event(100, 1_000, 0, 0, 0);
        stage.world_lists.insert(NEXT_STAGE, vec![world]);
        globals.stage_forms.insert(TEST_STAGE_FORM_ID, stage);
        globals.start_wipe(WipeState::new(
            TEST_STAGE_FORM_ID,
            None,
            None,
            0,
            1_000,
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
        globals.script.time_stop_flag = true;

        globals.tick_frame(10, 10, &[], None);

        assert_eq!(next_world_event_time(&globals), 0);
        assert_eq!(globals.local_wipe_time, 10);
    }

    #[test]
    fn stage_time_stop_freezes_stage_events_during_wipe() {
        let mut globals = GlobalState::default();
        let mut stage = StageFormState::default();
        let mut world = WorldState::new(0);
        world.camera_eye_x.set_event(100, 1_000, 0, 0, 0);
        stage.world_lists.insert(NEXT_STAGE, vec![world]);
        globals.stage_forms.insert(TEST_STAGE_FORM_ID, stage);
        globals.start_wipe(WipeState::new(
            TEST_STAGE_FORM_ID,
            None,
            None,
            0,
            1_000,
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
        globals.script.stage_time_stop_flag = true;

        globals.tick_frame(10, 10, &[], None);

        assert_eq!(next_world_event_time(&globals), 0);
    }

    #[test]
    fn excall_stage_advances_while_local_time_is_stopped() {
        const FRONT_STAGE: i64 = 1;
        const EXCALL_STAGE_FORM_ID: u32 = TEST_STAGE_FORM_ID ^ 0x4000;

        let mut globals = GlobalState::default();

        let mut normal_stage = StageFormState::default();
        let mut normal_world = WorldState::new(0);
        normal_world.camera_eye_x.set_event(100, 1_000, 0, 0, 0);
        normal_stage
            .world_lists
            .insert(FRONT_STAGE, vec![normal_world]);
        globals.stage_forms.insert(TEST_STAGE_FORM_ID, normal_stage);

        let mut excall_stage = StageFormState::default();
        let mut excall_world = WorldState::new(0);
        excall_world.camera_eye_x.set_event(100, 1_000, 0, 0, 0);
        excall_stage
            .world_lists
            .insert(FRONT_STAGE, vec![excall_world]);
        globals
            .stage_forms
            .insert(EXCALL_STAGE_FORM_ID, excall_stage);

        globals.script.time_stop_flag = true;
        globals.tick_frame(
            10,
            10,
            &[],
            Some(ExcallTickTargets {
                counter_list_id: 0x7000_0001,
                frame_action_id: 0x7000_0002,
                frame_action_ch_list_id: 0x7000_0003,
                stage_form_id: EXCALL_STAGE_FORM_ID,
            }),
        );

        assert_eq!(
            globals.stage_forms[&TEST_STAGE_FORM_ID].world_lists[&FRONT_STAGE][0]
                .camera_eye_x
                .cur_time,
            0
        );
        assert_eq!(
            globals.stage_forms[&EXCALL_STAGE_FORM_ID].world_lists[&FRONT_STAGE][0]
                .camera_eye_x
                .cur_time,
            10
        );
    }
}
