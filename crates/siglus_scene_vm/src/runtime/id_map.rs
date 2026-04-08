//! Runtime numeric ID map.
//!
//! The original Siglus engine routes many behaviors through numeric form/element IDs.
//! When porting without a complete constant table, we can still make progress by:
//! - shipping conservative defaults, and
//! - allowing overrides via environment variables and project-local files.
//!
//! Numeric override format:
//!   SIGLUS_IDMAP="key=value;key=value;..."
//!
//! Values accept decimal (e.g. `135`) or hex (`0x87`).
//!
//! Name-map format (cmd/op maps):
//!   123=FOO\n
//!   0x10=BAR
//!
//! Entries can also be separated by semicolons. Lines starting with `#` are ignored.

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct IdMap {
    // Global form IDs
    pub form_global_stage: u32,
    pub form_global_mov: u32,
    pub form_global_bgm: u32,
    pub form_global_bgm_table: u32,
    pub form_global_pcm: u32,
    pub form_global_pcmch: u32,
    pub form_global_se: u32,
    pub form_global_pcm_event: u32,
    pub form_global_excall: u32,
    pub form_global_koe_st: u32,

    pub form_global_screen: u32,
    pub form_global_msgbk: u32,

    pub form_global_input: u32,
    pub form_global_mouse: u32,
    pub form_global_keylist: u32,
    pub form_global_key: u32,

    pub form_global_syscom: u32,
    pub form_global_script: u32,
    pub form_global_system: u32,
    pub form_global_frame_action: u32,
    pub form_global_frame_action_ch: u32,

    pub form_global_math: u32,
    pub form_global_cgtable: u32,
    pub form_global_database: u32,
    pub form_global_g00buf: u32,
    pub form_global_mask: u32,
    pub form_global_editbox: u32,
    pub form_global_file: u32,
    pub form_global_steam: u32,

    // SCREEN selectors and aliases (optional)
    pub screen_sel_effect: i32,
    pub screen_sel_quake: i32,
    pub screen_sel_shake: i32,

    pub screen_x: i32,
    pub screen_y: i32,
    pub screen_z: i32,
    pub screen_mono: i32,
    pub screen_reverse: i32,
    pub screen_bright: i32,
    pub screen_dark: i32,
    pub screen_color_r: i32,
    pub screen_color_g: i32,
    pub screen_color_b: i32,
    pub screen_color_rate: i32,
    pub screen_color_add_r: i32,
    pub screen_color_add_g: i32,
    pub screen_color_add_b: i32,

    pub screen_x_eve: i32,
    pub screen_y_eve: i32,
    pub screen_z_eve: i32,
    pub screen_mono_eve: i32,
    pub screen_reverse_eve: i32,
    pub screen_bright_eve: i32,
    pub screen_dark_eve: i32,
    pub screen_color_r_eve: i32,
    pub screen_color_g_eve: i32,
    pub screen_color_b_eve: i32,
    pub screen_color_rate_eve: i32,
    pub screen_color_add_r_eve: i32,
    pub screen_color_add_g_eve: i32,
    pub screen_color_add_b_eve: i32,

    // EFFECT item op IDs (optional)
    pub effect_init: i32,
    pub effect_wipe_copy: i32,
    pub effect_wipe_erase: i32,
    pub effect_x: i32,
    pub effect_y: i32,
    pub effect_z: i32,
    pub effect_mono: i32,
    pub effect_reverse: i32,
    pub effect_bright: i32,
    pub effect_dark: i32,
    pub effect_color_r: i32,
    pub effect_color_g: i32,
    pub effect_color_b: i32,
    pub effect_color_rate: i32,
    pub effect_color_add_r: i32,
    pub effect_color_add_g: i32,
    pub effect_color_add_b: i32,
    pub effect_x_eve: i32,
    pub effect_y_eve: i32,
    pub effect_z_eve: i32,
    pub effect_mono_eve: i32,
    pub effect_reverse_eve: i32,
    pub effect_bright_eve: i32,
    pub effect_dark_eve: i32,
    pub effect_color_r_eve: i32,
    pub effect_color_g_eve: i32,
    pub effect_color_b_eve: i32,
    pub effect_color_rate_eve: i32,
    pub effect_color_add_r_eve: i32,
    pub effect_color_add_g_eve: i32,
    pub effect_color_add_b_eve: i32,
    pub effect_begin_order: i32,
    pub effect_end_order: i32,
    pub effect_begin_layer: i32,
    pub effect_end_layer: i32,

    // Input/Key op codes (all externally configurable)
    pub exkey_decide: i32,
    pub exkey_cancel: i32,

    pub input_op_decide: i32,
    pub input_op_cancel: i32,
    pub input_op_clear: i32,
    pub input_op_next: i32,

    pub mouse_op_x: i32,
    pub mouse_op_y: i32,
    pub mouse_op_clear: i32,
    pub mouse_op_wheel: i32,
    pub mouse_op_left: i32,
    pub mouse_op_right: i32,
    pub mouse_op_next: i32,
    pub mouse_op_get_pos: i32,
    pub mouse_op_set_pos: i32,

    pub keylist_op_wait: i32,
    pub keylist_op_wait_force: i32,
    pub keylist_op_clear: i32,
    pub keylist_op_next: i32,

    pub key_op_dir: i32,
    pub key_op_on_down: i32,
    pub key_op_on_up: i32,
    pub key_op_on_down_up: i32,
    pub key_op_is_down: i32,
    pub key_op_is_up: i32,
    pub key_op_on_flick: i32,
    pub key_op_flick: i32,
    pub key_op_flick_angle: i32,


    // MATH element codes (all externally configurable)
    pub math_max: i32,
    pub math_min: i32,
    pub math_limit: i32,
    pub math_abs: i32,
    pub math_rand: i32,
    pub math_sqrt: i32,
    pub math_log: i32,
    pub math_log2: i32,
    pub math_log10: i32,
    pub math_sin: i32,
    pub math_cos: i32,
    pub math_tan: i32,
    pub math_arcsin: i32,
    pub math_arccos: i32,
    pub math_arctan: i32,
    pub math_distance: i32,
    pub math_angle: i32,
    pub math_linear: i32,
    pub math_tostr: i32,
    pub math_tostr_zero: i32,

    // CGTABLE element codes (all externally configurable)
    pub cgtable_flag: i32,
    pub cgtable_set_disable: i32,
    pub cgtable_set_enable: i32,
    pub cgtable_set_all_flag: i32,
    pub cgtable_get_cg_cnt: i32,
    pub cgtable_get_look_cnt: i32,
    pub cgtable_get_look_percent: i32,
    pub cgtable_get_flag_no_by_name: i32,
    pub cgtable_get_look_by_name: i32,
    pub cgtable_set_look_by_name: i32,
    pub cgtable_get_name_by_flag_no: i32,

    // DATABASE element codes (all externally configurable)
    pub database_list_get_size: i32,
    pub database_get_num: i32,
    pub database_get_str: i32,
    pub database_check_item: i32,
    pub database_check_column: i32,
    pub database_find_num: i32,
    pub database_find_str: i32,
    pub database_find_str_real: i32,

    // G00BUF element codes (all externally configurable)
    pub g00buf_list_get_size: i32,
    pub g00buf_list_free_all: i32,
    pub g00buf_load: i32,
    pub g00buf_free: i32,

    // FILE element codes (all externally configurable)
    pub file_preload_omv: i32,

    // STEAM element codes (all externally configurable)
    pub steam_set_achievement: i32,
    pub steam_reset_all_status: i32,

    // Element helpers
    pub elm_array: i32,
    pub elm_up: i32,

    // Stage element codes
    pub stage_elm_object: i32,
    pub stage_elm_world: i32,

    // World list element codes (optional)
    pub worldlist_create: i32,
    pub worldlist_destroy: i32,

    // World element codes (optional)
    pub world_init: i32,
    pub world_get_no: i32,
    pub world_camera_eye_x: i32,
    pub world_camera_eye_y: i32,
    pub world_camera_eye_z: i32,
    pub world_camera_pint_x: i32,
    pub world_camera_pint_y: i32,
    pub world_camera_pint_z: i32,
    pub world_camera_up_x: i32,
    pub world_camera_up_y: i32,
    pub world_camera_up_z: i32,
    pub world_camera_eye_x_eve: i32,
    pub world_camera_eye_y_eve: i32,
    pub world_camera_eye_z_eve: i32,
    pub world_camera_pint_x_eve: i32,
    pub world_camera_pint_y_eve: i32,
    pub world_camera_pint_z_eve: i32,
    pub world_camera_up_x_eve: i32,
    pub world_camera_up_y_eve: i32,
    pub world_camera_up_z_eve: i32,
    pub world_camera_view_angle: i32,
    pub world_set_camera_eye: i32,
    pub world_calc_camera_eye: i32,
    pub world_set_camera_pint: i32,
    pub world_calc_camera_pint: i32,
    pub world_set_camera_up: i32,
    pub world_mono: i32,
    pub world_set_camera_eve_xz_rotate: i32,
    pub world_order: i32,
    pub world_layer: i32,
    pub world_wipe_copy: i32,
    pub world_wipe_erase: i32,

    // Object element codes (subset)
    pub obj_disp: i32,
    pub obj_patno: i32,
    pub obj_alpha: i32,
    pub obj_layer: i32,
    pub obj_order: i32,
    pub obj_x: i32,
    pub obj_y: i32,
    pub obj_z: i32,
    pub obj_create: i32,

    // Object creation ops
    pub obj_create_number: i32,
    pub obj_create_weather: i32,
    pub obj_create_mesh: i32,
    pub obj_create_billboard: i32,
    pub obj_create_save_thumb: i32,
    pub obj_create_capture_thumb: i32,
    pub obj_create_capture: i32,
    pub obj_create_movie: i32,
    pub obj_create_movie_loop: i32,
    pub obj_create_movie_wait: i32,
    pub obj_create_movie_wait_key: i32,
    pub obj_create_emote: i32,
    pub obj_create_copy_from: i32,

    // Weather / Movie param ops
    pub obj_set_weather_param_type_a: i32,
    pub obj_set_weather_param_type_b: i32,
    pub obj_pause_movie: i32,
    pub obj_resume_movie: i32,
    pub obj_seek_movie: i32,
    pub obj_get_movie_seek_time: i32,
    pub obj_check_movie: i32,
    pub obj_wait_movie: i32,
    pub obj_wait_movie_key: i32,
    pub obj_end_movie_loop: i32,
    pub obj_set_movie_auto_free: i32,

    // Button ops
    pub obj_clear_button: i32,
    pub obj_set_button: i32,
    pub obj_set_button_group: i32,
    pub obj_set_button_pushkeep: i32,
    pub obj_get_button_pushkeep: i32,
    pub obj_set_button_alpha_test: i32,
    pub obj_get_button_alpha_test: i32,
    pub obj_set_button_state_normal: i32,
    pub obj_set_button_state_select: i32,
    pub obj_set_button_state_disable: i32,
    pub obj_get_button_state: i32,
    pub obj_get_button_hit_state: i32,
    pub obj_get_button_real_state: i32,
    pub obj_set_button_call: i32,
    pub obj_clear_button_call: i32,

    // Frame action and GAN
    pub obj_frame_action: i32,
    pub obj_frame_action_ch: i32,
    pub obj_load_gan: i32,
    pub obj_start_gan: i32,

    // Stage object command-like ops (best-effort).
    pub obj_wipe_copy: i32,
    pub obj_wipe_erase: i32,
    pub obj_click_disable: i32,

    // Object element codes (extended subset)
    pub obj_world: i32,
    pub obj_center_x: i32,
    pub obj_center_y: i32,
    pub obj_center_z: i32,
    pub obj_set_center: i32,
    pub obj_scale_x: i32,
    pub obj_scale_y: i32,
    pub obj_scale_z: i32,
    pub obj_set_scale: i32,
    pub obj_rotate_x: i32,
    pub obj_rotate_y: i32,
    pub obj_rotate_z: i32,
    pub obj_set_rotate: i32,
    pub obj_clip_left: i32,
    pub obj_clip_top: i32,
    pub obj_clip_right: i32,
    pub obj_clip_bottom: i32,
    pub obj_set_clip: i32,
    pub obj_src_clip_left: i32,
    pub obj_src_clip_top: i32,
    pub obj_src_clip_right: i32,
    pub obj_src_clip_bottom: i32,
    pub obj_set_src_clip: i32,
    pub obj_tr: i32,
    pub obj_mono: i32,
    pub obj_reverse: i32,
    pub obj_bright: i32,
    pub obj_dark: i32,
    pub obj_color_r: i32,
    pub obj_color_g: i32,
    pub obj_color_b: i32,
    pub obj_color_rate: i32,
    pub obj_color_add_r: i32,
    pub obj_color_add_g: i32,
    pub obj_color_add_b: i32,

    pub obj_set_pos: i32,
    pub obj_x_rep: i32,
    pub obj_y_rep: i32,
    pub obj_z_rep: i32,

    pub obj_center_rep_x: i32,
    pub obj_center_rep_y: i32,
    pub obj_center_rep_z: i32,
    pub obj_set_center_rep: i32,

    pub obj_clip_use: i32,
    pub obj_src_clip_use: i32,

    pub obj_mask_no: i32,
    pub obj_tonecurve_no: i32,
    pub obj_culling: i32,
    pub obj_alpha_test: i32,
    pub obj_alpha_blend: i32,
    pub obj_blend: i32,
    pub obj_light_no: i32,
    pub obj_fog_use: i32,

    // Object *_EVE element codes
    pub obj_patno_eve: i32,
    pub obj_x_eve: i32,
    pub obj_y_eve: i32,
    pub obj_z_eve: i32,

    pub obj_x_rep_eve: i32,
    pub obj_y_rep_eve: i32,
    pub obj_z_rep_eve: i32,

    pub obj_center_x_eve: i32,
    pub obj_center_y_eve: i32,
    pub obj_center_z_eve: i32,

    pub obj_center_rep_x_eve: i32,
    pub obj_center_rep_y_eve: i32,
    pub obj_center_rep_z_eve: i32,

    pub obj_scale_x_eve: i32,
    pub obj_scale_y_eve: i32,
    pub obj_scale_z_eve: i32,

    pub obj_rotate_x_eve: i32,
    pub obj_rotate_y_eve: i32,
    pub obj_rotate_z_eve: i32,

    pub obj_clip_left_eve: i32,
    pub obj_clip_top_eve: i32,
    pub obj_clip_right_eve: i32,
    pub obj_clip_bottom_eve: i32,

    pub obj_src_clip_left_eve: i32,
    pub obj_src_clip_top_eve: i32,
    pub obj_src_clip_right_eve: i32,
    pub obj_src_clip_bottom_eve: i32,

    pub obj_tr_eve: i32,
    pub obj_tr_rep: i32,
    pub obj_tr_rep_eve: i32,

    pub obj_mono_eve: i32,
    pub obj_reverse_eve: i32,
    pub obj_bright_eve: i32,
    pub obj_dark_eve: i32,

    pub obj_color_r_eve: i32,
    pub obj_color_g_eve: i32,
    pub obj_color_b_eve: i32,
    pub obj_color_rate_eve: i32,
    pub obj_color_add_r_eve: i32,
    pub obj_color_add_g_eve: i32,
    pub obj_color_add_b_eve: i32,

    // Object query methods
    pub obj_get_pat_cnt: i32,
    pub obj_get_size_x: i32,
    pub obj_get_size_y: i32,
    pub obj_get_size_z: i32,
    pub obj_get_pixel_color_r: i32,
    pub obj_get_pixel_color_g: i32,
    pub obj_get_pixel_color_b: i32,
    pub obj_get_pixel_color_a: i32,

    pub obj_f: i32,

    // Object methods (subset)
    pub obj_change_file: i32,
    pub obj_exist_type: i32,
    pub obj_set_string: i32,
    pub obj_get_string: i32,
    pub obj_set_string_param: i32,
    pub obj_set_number: i32,
    pub obj_get_number: i32,
    pub obj_set_number_param: i32,

    // Object ALL_EVE and allevent sub-ops
    pub obj_all_eve: i32,
    pub elm_allevent_end: i32,
    pub elm_allevent_wait: i32,
    pub elm_allevent_check: i32,

    // Object methods (subset)
    pub obj_init: i32,
    pub obj_free: i32,
    pub obj_init_param: i32,
    pub obj_get_file_name: i32,

    // Packed element name maps (numeric code -> readable name).
    pub user_cmd_names: HashMap<u32, String>,
    pub call_cmd_names: HashMap<u32, String>,
    pub function_names: HashMap<u32, String>,
    pub call_prop_names: HashMap<u32, String>,
    pub user_prop_names: HashMap<u32, String>,

    // Sub-op name maps (numeric op -> readable op name).
    pub bgm_op_names: HashMap<i64, String>,
    pub se_op_names: HashMap<i64, String>,
    pub pcm_op_names: HashMap<i64, String>,
    pub mov_op_names: HashMap<i64, String>,
    pub excall_op_names: HashMap<i64, String>,
    pub bgm_table_op_names: HashMap<i64, String>,
}

impl Default for IdMap {
    fn default() -> Self {
        let mut out = Self {
            // NOTE: These defaults are meant to be *reasonable* for bring-up.
            // They can be overridden by SIGLUS_IDMAP.
            form_global_stage: 135,
            // Defaults match values extracted into `runtime/forms/codes.rs`.
            form_global_mov: 20,
            form_global_bgm: 42,
            form_global_pcm: 43,
            form_global_pcmch: 44,
            form_global_se: 45,
            form_global_pcm_event: 52,
            form_global_excall: 65,
            form_global_koe_st: 82,
            form_global_bgm_table: 123,

            // Game-specific UI forms (optional; can be overridden).
            form_global_screen: 70,
            form_global_msgbk: 145,

            // Optional global forms (can be overridden).
            form_global_input: 86,
            form_global_mouse: 46,
            form_global_keylist: 24,
            form_global_key: 89,

            // Optional global forms (disabled by default).
            form_global_syscom: 0,
            form_global_script: 0,
            form_global_system: 0,
            form_global_frame_action: 0,
            form_global_frame_action_ch: 0,

            form_global_math: 0,
            form_global_cgtable: 0,
            form_global_database: 0,
            form_global_g00buf: 0,
            form_global_mask: 0,
            form_global_editbox: 0,
            form_global_file: 0,
            form_global_steam: 0,

            screen_sel_effect: 0,
            screen_sel_quake: 0,
            screen_sel_shake: 0,

            screen_x: 0,
            screen_y: 0,
            screen_z: 0,
            screen_mono: 0,
            screen_reverse: 0,
            screen_bright: 0,
            screen_dark: 0,
            screen_color_r: 0,
            screen_color_g: 0,
            screen_color_b: 0,
            screen_color_rate: 0,
            screen_color_add_r: 0,
            screen_color_add_g: 0,
            screen_color_add_b: 0,

            screen_x_eve: 0,
            screen_y_eve: 0,
            screen_z_eve: 0,
            screen_mono_eve: 0,
            screen_reverse_eve: 0,
            screen_bright_eve: 0,
            screen_dark_eve: 0,
            screen_color_r_eve: 0,
            screen_color_g_eve: 0,
            screen_color_b_eve: 0,
            screen_color_rate_eve: 0,
            screen_color_add_r_eve: 0,
            screen_color_add_g_eve: 0,
            screen_color_add_b_eve: 0,

            effect_init: 0,
            effect_wipe_copy: 0,
            effect_wipe_erase: 0,
            effect_x: 0,
            effect_y: 0,
            effect_z: 0,
            effect_mono: 0,
            effect_reverse: 0,
            effect_bright: 0,
            effect_dark: 0,
            effect_color_r: 0,
            effect_color_g: 0,
            effect_color_b: 0,
            effect_color_rate: 0,
            effect_color_add_r: 0,
            effect_color_add_g: 0,
            effect_color_add_b: 0,
            effect_x_eve: 0,
            effect_y_eve: 0,
            effect_z_eve: 0,
            effect_mono_eve: 0,
            effect_reverse_eve: 0,
            effect_bright_eve: 0,
            effect_dark_eve: 0,
            effect_color_r_eve: 0,
            effect_color_g_eve: 0,
            effect_color_b_eve: 0,
            effect_color_rate_eve: 0,
            effect_color_add_r_eve: 0,
            effect_color_add_g_eve: 0,
            effect_color_add_b_eve: 0,
            effect_begin_order: 0,
            effect_end_order: 0,
            effect_begin_layer: 0,
            effect_end_layer: 0,

            // EX key IDs (used by INPUT/KEY).
            exkey_decide: 256,
            exkey_cancel: 257,

            // INPUT sub-ops.
            input_op_decide: 0,
            input_op_cancel: 1,
            input_op_clear: 2,
            input_op_next: 3,

            // MOUSE sub-ops (best-effort bring-up).
            mouse_op_x: 0,
            mouse_op_y: 1,
            mouse_op_clear: 4,
            mouse_op_wheel: 5,
            mouse_op_left: 6,
            mouse_op_right: 7,
            mouse_op_next: 8,
            mouse_op_get_pos: 9,
            mouse_op_set_pos: 10,

            // KEYLIST sub-ops.
            keylist_op_wait: 0,
            keylist_op_wait_force: 1,
            keylist_op_clear: 3,
            keylist_op_next: 5,

            // KEY sub-ops.
            key_op_dir: 0,
            key_op_on_down: 1,
            key_op_on_up: 4,
            key_op_on_down_up: 5,
            key_op_is_down: 6,
            key_op_is_up: 7,
            key_op_on_flick: 10,
            key_op_flick: 14,
            key_op_flick_angle: 15,


            // MATH element codes (disabled by default).
            math_max: 0,
            math_min: 0,
            math_limit: 0,
            math_abs: 0,
            math_rand: 0,
            math_sqrt: 0,
            math_log: 0,
            math_log2: 0,
            math_log10: 0,
            math_sin: 0,
            math_cos: 0,
            math_tan: 0,
            math_arcsin: 0,
            math_arccos: 0,
            math_arctan: 0,
            math_distance: 0,
            math_angle: 0,
            math_linear: 0,
            math_tostr: 0,
            math_tostr_zero: 0,

            // CGTABLE element codes (disabled by default).
            cgtable_flag: 0,
            cgtable_set_disable: 0,
            cgtable_set_enable: 0,
            cgtable_set_all_flag: 0,
            cgtable_get_cg_cnt: 0,
            cgtable_get_look_cnt: 0,
            cgtable_get_look_percent: 0,
            cgtable_get_flag_no_by_name: 0,
            cgtable_get_look_by_name: 0,
            cgtable_set_look_by_name: 0,
            cgtable_get_name_by_flag_no: 0,

            // DATABASE element codes (disabled by default).
            database_list_get_size: 0,
            database_get_num: 0,
            database_get_str: 0,
            database_check_item: 0,
            database_check_column: 0,
            database_find_num: 0,
            database_find_str: 0,
            database_find_str_real: 0,

            // G00BUF element codes (disabled by default).
            g00buf_list_get_size: 0,
            g00buf_list_free_all: 0,
            g00buf_load: 0,
            g00buf_free: 0,

            // FILE element codes (disabled by default).
            file_preload_omv: 0,

            // STEAM element codes (disabled by default).
            steam_set_achievement: 0,
            steam_reset_all_status: 0,

            elm_array: -1,
            elm_up: -5,

            stage_elm_object: 2,
            stage_elm_world: 0,

            worldlist_create: 0,
            worldlist_destroy: 0,

            world_init: 0,
            world_get_no: 0,
            world_camera_eye_x: 0,
            world_camera_eye_y: 0,
            world_camera_eye_z: 0,
            world_camera_pint_x: 0,
            world_camera_pint_y: 0,
            world_camera_pint_z: 0,
            world_camera_up_x: 0,
            world_camera_up_y: 0,
            world_camera_up_z: 0,
            world_camera_eye_x_eve: 0,
            world_camera_eye_y_eve: 0,
            world_camera_eye_z_eve: 0,
            world_camera_pint_x_eve: 0,
            world_camera_pint_y_eve: 0,
            world_camera_pint_z_eve: 0,
            world_camera_up_x_eve: 0,
            world_camera_up_y_eve: 0,
            world_camera_up_z_eve: 0,
            world_camera_view_angle: 0,
            world_set_camera_eye: 0,
            world_calc_camera_eye: 0,
            world_set_camera_pint: 0,
            world_calc_camera_pint: 0,
            world_set_camera_up: 0,
            world_mono: 0,
            world_set_camera_eve_xz_rotate: 0,
            world_order: 0,
            world_layer: 0,
            world_wipe_copy: 0,
            world_wipe_erase: 0,

            obj_disp: 0x0D,
            obj_patno: 0x0E,
            obj_alpha: 0x15,
            obj_order: 0x10,
            obj_layer: 0x11,
            obj_x: 0x12,
            obj_y: 0x13,
            obj_z: 0x14,
            obj_create: 0x26,
            obj_create_number: 0,
            obj_create_weather: 0,
            obj_create_mesh: 0,
            obj_create_billboard: 0,
            obj_create_save_thumb: 0,
            obj_create_capture_thumb: 0,
            obj_create_capture: 0,
            obj_create_movie: 0,
            obj_create_movie_loop: 0,
            obj_create_movie_wait: 0,
            obj_create_movie_wait_key: 0,
            obj_create_emote: 0,
            obj_create_copy_from: 0,
            obj_set_weather_param_type_a: 0,
            obj_set_weather_param_type_b: 0,
            obj_pause_movie: 0,
            obj_resume_movie: 0,
            obj_seek_movie: 0,
            obj_get_movie_seek_time: 0,
            obj_check_movie: 0,
            obj_wait_movie: 0,
            obj_wait_movie_key: 0,
            obj_end_movie_loop: 0,
            obj_set_movie_auto_free: 0,
            obj_clear_button: 0,
            obj_set_button: 0,
            obj_set_button_group: 0,
            obj_set_button_pushkeep: 0,
            obj_get_button_pushkeep: 0,
            obj_set_button_alpha_test: 0,
            obj_get_button_alpha_test: 0,
            obj_set_button_state_normal: 0,
            obj_set_button_state_select: 0,
            obj_set_button_state_disable: 0,
            obj_get_button_state: 0,
            obj_get_button_hit_state: 0,
            obj_get_button_real_state: 0,
            obj_set_button_call: 0,
            obj_clear_button_call: 0,
            obj_frame_action: 0,
            obj_frame_action_ch: 0,
            obj_load_gan: 0,
            obj_start_gan: 0,

            // Conservative bring-up defaults.
            obj_wipe_copy: 0x0A,
            obj_wipe_erase: 0x0B,
            obj_click_disable: 0x0C,

            // Extended subset (default unknown unless overridden)
            obj_world: 0x0F,
            obj_center_x: 0,
            obj_center_y: 0,
            obj_center_z: 0,
            obj_set_center: 0,
            obj_scale_x: 0,
            obj_scale_y: 0,
            obj_scale_z: 0,
            obj_set_scale: 0,
            obj_rotate_x: 0,
            obj_rotate_y: 0,
            obj_rotate_z: 0,
            obj_set_rotate: 0,
            obj_clip_left: 0,
            obj_clip_top: 0,
            obj_clip_right: 0,
            obj_clip_bottom: 0,
            obj_set_clip: 0,
            obj_src_clip_left: 0,
            obj_src_clip_top: 0,
            obj_src_clip_right: 0,
            obj_src_clip_bottom: 0,
            obj_set_src_clip: 0,
            obj_tr: 0,
            obj_mono: 0,
            obj_reverse: 0,
            obj_bright: 0,
            obj_dark: 0,
            obj_color_r: 0,
            obj_color_g: 0,
            obj_color_b: 0,
            obj_color_rate: 0,
            obj_color_add_r: 0,
            obj_color_add_g: 0,
            obj_color_add_b: 0,

            obj_set_pos: 0,
            obj_x_rep: 0,
            obj_y_rep: 0,
            obj_z_rep: 0,

            obj_center_rep_x: 0,
            obj_center_rep_y: 0,
            obj_center_rep_z: 0,
            obj_set_center_rep: 0,

            obj_clip_use: 0,
            obj_src_clip_use: 0,

            obj_mask_no: 0,
            obj_tonecurve_no: 0,
            obj_culling: 0,
            obj_alpha_test: 0,
            obj_alpha_blend: 0,
            obj_blend: 0,
            obj_light_no: 0,
            obj_fog_use: 0,

            obj_patno_eve: 0,
            obj_x_eve: 0,
            obj_y_eve: 0,
            obj_z_eve: 0,

            obj_x_rep_eve: 0,
            obj_y_rep_eve: 0,
            obj_z_rep_eve: 0,

            obj_center_x_eve: 0,
            obj_center_y_eve: 0,
            obj_center_z_eve: 0,

            obj_center_rep_x_eve: 0,
            obj_center_rep_y_eve: 0,
            obj_center_rep_z_eve: 0,

            obj_scale_x_eve: 0,
            obj_scale_y_eve: 0,
            obj_scale_z_eve: 0,

            obj_rotate_x_eve: 0,
            obj_rotate_y_eve: 0,
            obj_rotate_z_eve: 0,

            obj_clip_left_eve: 0,
            obj_clip_top_eve: 0,
            obj_clip_right_eve: 0,
            obj_clip_bottom_eve: 0,

            obj_src_clip_left_eve: 0,
            obj_src_clip_top_eve: 0,
            obj_src_clip_right_eve: 0,
            obj_src_clip_bottom_eve: 0,

            obj_tr_eve: 0,
            obj_tr_rep: 0,
            obj_tr_rep_eve: 0,

            obj_mono_eve: 0,
            obj_reverse_eve: 0,
            obj_bright_eve: 0,
            obj_dark_eve: 0,

            obj_color_r_eve: 0,
            obj_color_g_eve: 0,
            obj_color_b_eve: 0,
            obj_color_rate_eve: 0,
            obj_color_add_r_eve: 0,
            obj_color_add_g_eve: 0,
            obj_color_add_b_eve: 0,

            obj_get_pat_cnt: 0,
            obj_get_size_x: 0,
            obj_get_size_y: 0,
            obj_get_size_z: 0,
            obj_get_pixel_color_r: 0,
            obj_get_pixel_color_g: 0,
            obj_get_pixel_color_b: 0,
            obj_get_pixel_color_a: 0,

            obj_f: 0,

            obj_change_file: 0,
            obj_exist_type: 0,
            obj_set_string: 0,
            obj_get_string: 0,
            obj_set_string_param: 0,
            obj_set_number: 0,
            obj_get_number: 0,
            obj_set_number_param: 0,

            obj_all_eve: 0,
            elm_allevent_end: 0,
            elm_allevent_wait: 0,
            elm_allevent_check: 0,

            obj_init: 0,
            obj_free: 0,
            obj_init_param: 0,
            obj_get_file_name: 0,

            user_cmd_names: HashMap::new(),
            call_cmd_names: HashMap::new(),
            function_names: HashMap::new(),
            call_prop_names: HashMap::new(),
            user_prop_names: HashMap::new(),

            bgm_op_names: HashMap::new(),
            se_op_names: HashMap::new(),
            pcm_op_names: HashMap::new(),
            mov_op_names: HashMap::new(),
            excall_op_names: HashMap::new(),
            bgm_table_op_names: HashMap::new(),
        };

        fill_default_op_maps(&mut out);

        out
    }
}

impl IdMap {
    pub fn load_from_env() -> Self {
        let mut out = Self::default();

        // Inline env override (key=value;...)
        if let Ok(spec) = env::var("SIGLUS_IDMAP") {
            apply_kv_text(&mut out, &spec);
        }
        // File-based env override.
        if let Ok(path) = env::var("SIGLUS_IDMAP_FILE") {
            if let Ok(text) = fs::read_to_string(Path::new(&path)) {
                apply_kv_text(&mut out, &text);
            }
        }

        // Packed element name maps.
        load_u32_name_map_env("SIGLUS_USER_CMD_MAP", "SIGLUS_USER_CMD_MAP_FILE", &mut out.user_cmd_names);
        load_u32_name_map_env("SIGLUS_CALL_CMD_MAP", "SIGLUS_CALL_CMD_MAP_FILE", &mut out.call_cmd_names);
        load_u32_name_map_env("SIGLUS_FUNCTION_MAP", "SIGLUS_FUNCTION_MAP_FILE", &mut out.function_names);
        load_u32_name_map_env("SIGLUS_CALL_PROP_MAP", "SIGLUS_CALL_PROP_MAP_FILE", &mut out.call_prop_names);
        load_u32_name_map_env("SIGLUS_USER_PROP_MAP", "SIGLUS_USER_PROP_MAP_FILE", &mut out.user_prop_names);

        // Sub-op name maps.
        load_i64_name_map_env("SIGLUS_BGM_OP_MAP", "SIGLUS_BGM_OP_MAP_FILE", &mut out.bgm_op_names);
        load_i64_name_map_env("SIGLUS_SE_OP_MAP", "SIGLUS_SE_OP_MAP_FILE", &mut out.se_op_names);
        load_i64_name_map_env("SIGLUS_PCM_OP_MAP", "SIGLUS_PCM_OP_MAP_FILE", &mut out.pcm_op_names);
        load_i64_name_map_env("SIGLUS_MOV_OP_MAP", "SIGLUS_MOV_OP_MAP_FILE", &mut out.mov_op_names);
        load_i64_name_map_env("SIGLUS_EXCALL_OP_MAP", "SIGLUS_EXCALL_OP_MAP_FILE", &mut out.excall_op_names);
        load_i64_name_map_env(
            "SIGLUS_BGM_TABLE_OP_MAP",
            "SIGLUS_BGM_TABLE_OP_MAP_FILE",
            &mut out.bgm_table_op_names,
        );

        out
    }

    // ---------------------------------------------------------------------
    // Packed element name lookups
    // ---------------------------------------------------------------------

    pub fn user_cmd_name(&self, cmd_no: u32) -> Option<&str> {
        self.user_cmd_names.get(&cmd_no).map(|s| s.as_str())
    }

    pub fn call_cmd_name(&self, cmd_no: u32) -> Option<&str> {
        self.call_cmd_names.get(&cmd_no).map(|s| s.as_str())
    }

    pub fn function_name(&self, fn_no: u32) -> Option<&str> {
        self.function_names.get(&fn_no).map(|s| s.as_str())
    }

    pub fn call_prop_name(&self, prop_no: u32) -> Option<&str> {
        self.call_prop_names.get(&prop_no).map(|s| s.as_str())
    }

    pub fn user_prop_name(&self, prop_no: u32) -> Option<&str> {
        self.user_prop_names.get(&prop_no).map(|s| s.as_str())
    }

    pub fn try_load_user_cmd_names_file(&mut self, path: &Path) {
        try_load_u32_map_file(path, &mut self.user_cmd_names);
    }

    pub fn try_load_call_cmd_names_file(&mut self, path: &Path) {
        try_load_u32_map_file(path, &mut self.call_cmd_names);
    }

    pub fn try_load_function_names_file(&mut self, path: &Path) {
        try_load_u32_map_file(path, &mut self.function_names);
    }

    pub fn try_load_call_prop_names_file(&mut self, path: &Path) {
        try_load_u32_map_file(path, &mut self.call_prop_names);
    }

    pub fn try_load_user_prop_names_file(&mut self, path: &Path) {
        try_load_u32_map_file(path, &mut self.user_prop_names);
    }

    // ---------------------------------------------------------------------
    // Sub-op name lookups
    // ---------------------------------------------------------------------

    pub fn bgm_op_name(&self, op: i64) -> Option<&str> {
        self.bgm_op_names.get(&op).map(|s| s.as_str())
    }

    pub fn se_op_name(&self, op: i64) -> Option<&str> {
        self.se_op_names.get(&op).map(|s| s.as_str())
    }

    pub fn pcm_op_name(&self, op: i64) -> Option<&str> {
        self.pcm_op_names.get(&op).map(|s| s.as_str())
    }

    pub fn mov_op_name(&self, op: i64) -> Option<&str> {
        self.mov_op_names.get(&op).map(|s| s.as_str())
    }

    pub fn excall_op_name(&self, op: i64) -> Option<&str> {
        self.excall_op_names.get(&op).map(|s| s.as_str())
    }

    pub fn bgm_table_op_name(&self, op: i64) -> Option<&str> {
        self.bgm_table_op_names.get(&op).map(|s| s.as_str())
    }

    pub fn try_load_bgm_op_names_file(&mut self, path: &Path) {
        try_load_i64_map_file(path, &mut self.bgm_op_names);
    }

    pub fn try_load_se_op_names_file(&mut self, path: &Path) {
        try_load_i64_map_file(path, &mut self.se_op_names);
    }

    pub fn try_load_pcm_op_names_file(&mut self, path: &Path) {
        try_load_i64_map_file(path, &mut self.pcm_op_names);
    }

    pub fn try_load_mov_op_names_file(&mut self, path: &Path) {
        try_load_i64_map_file(path, &mut self.mov_op_names);
    }

    pub fn try_load_excall_op_names_file(&mut self, path: &Path) {
        try_load_i64_map_file(path, &mut self.excall_op_names);
    }

    pub fn try_load_bgm_table_op_names_file(&mut self, path: &Path) {
        try_load_i64_map_file(path, &mut self.bgm_table_op_names);
    }

    /// Best-effort load of id map overrides from a text file.
    ///
    /// The file format matches `SIGLUS_IDMAP`:
    ///   key=value\nkey=value\n...
    pub fn try_load_idmap_file(&mut self, path: &Path) {
        if !path.is_file() {
            return;
        }
        if let Ok(text) = fs::read_to_string(path) {
            apply_kv_text(self, &text);
        }
    }
}

fn fill_default_op_maps(out: &mut IdMap) {
    // BGM
    insert_i64(&mut out.bgm_op_names, 0, "PLAY");
    insert_i64(&mut out.bgm_op_names, 1, "PLAY_ONESHOT");
    insert_i64(&mut out.bgm_op_names, 2, "PLAY_WAIT");
    insert_i64(&mut out.bgm_op_names, 3, "WAIT");
    insert_i64(&mut out.bgm_op_names, 4, "STOP");
    insert_i64(&mut out.bgm_op_names, 5, "WAIT_FADE");
    insert_i64(&mut out.bgm_op_names, 6, "SET_VOLUME");
    insert_i64(&mut out.bgm_op_names, 7, "SET_VOLUME_MAX");
    insert_i64(&mut out.bgm_op_names, 8, "SET_VOLUME_MIN");
    insert_i64(&mut out.bgm_op_names, 9, "GET_VOLUME");
    insert_i64(&mut out.bgm_op_names, 10, "PAUSE");
    insert_i64(&mut out.bgm_op_names, 11, "RESUME");
    insert_i64(&mut out.bgm_op_names, 12, "RESUME_WAIT");
    insert_i64(&mut out.bgm_op_names, 13, "CHECK");
    insert_i64(&mut out.bgm_op_names, 14, "WAIT_KEY");
    insert_i64(&mut out.bgm_op_names, 15, "WAIT_FADE_KEY");
    insert_i64(&mut out.bgm_op_names, 16, "READY");
    insert_i64(&mut out.bgm_op_names, 17, "READY_ONESHOT");
    insert_i64(&mut out.bgm_op_names, 18, "GET_PLAY_POS");
    insert_i64(&mut out.bgm_op_names, 19, "GET_REGIST_NAME");

    // SE
    insert_i64(&mut out.se_op_names, 0, "PLAY");
    insert_i64(&mut out.se_op_names, 1, "SET_VOLUME");
    insert_i64(&mut out.se_op_names, 2, "SET_VOLUME_MAX");
    insert_i64(&mut out.se_op_names, 3, "SET_VOLUME_MIN");
    insert_i64(&mut out.se_op_names, 4, "GET_VOLUME");
    insert_i64(&mut out.se_op_names, 5, "PLAY_BY_FILE_NAME");
    insert_i64(&mut out.se_op_names, 6, "PLAY_BY_KOE_NO");
    insert_i64(&mut out.se_op_names, 7, "STOP");
    insert_i64(&mut out.se_op_names, 8, "WAIT");
    insert_i64(&mut out.se_op_names, 9, "PLAY_BY_SE_NO");
    insert_i64(&mut out.se_op_names, 10, "WAIT_KEY");
    insert_i64(&mut out.se_op_names, 11, "CHECK");

    // PCM
    insert_i64(&mut out.pcm_op_names, 0, "PLAY");
    insert_i64(&mut out.pcm_op_names, 1, "STOP");

    // MOV
    insert_i64(&mut out.mov_op_names, 0, "PLAY");
    insert_i64(&mut out.mov_op_names, 1, "STOP");
    insert_i64(&mut out.mov_op_names, 2, "PLAY_WAIT");
    insert_i64(&mut out.mov_op_names, 3, "PLAY_WAIT_KEY");

    // EXCALL
    insert_i64(&mut out.excall_op_names, -1, "ARRAY_INDEX");
    for i in 0..=13i64 {
        if i == 11 {
            continue;
        }
        out.excall_op_names.entry(i).or_insert_with(|| format!("OP_{i}"));
    }

    // BGMTABLE
    insert_i64(&mut out.bgm_table_op_names, 0, "GET_COUNT");
    insert_i64(&mut out.bgm_table_op_names, 1, "GET_LISTEN_BY_NAME");
    insert_i64(&mut out.bgm_table_op_names, 2, "SET_LISTEN_CURRENT");
    insert_i64(&mut out.bgm_table_op_names, 4, "SET_ALL_FLAG");
}

fn insert_i64(map: &mut HashMap<i64, String>, k: i64, v: &str) {
    map.entry(k).or_insert_with(|| v.to_string());
}

fn apply_kv_text(out: &mut IdMap, text: &str) {
    for part in text.split(|c| c == ';' || c == '\n') {
        let part = part.trim();
        if part.is_empty() || part.starts_with('#') {
            continue;
        }
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        let Some(n) = parse_i64(v) else {
            continue;
        };
        apply_one(out, k, n);
    }
}

fn apply_one(out: &mut IdMap, k: &str, n: i64) {
    match k {
        // Global form IDs
        "form_global_stage" => out.form_global_stage = n as u32,
        "form_global_mov" => out.form_global_mov = n as u32,
        "form_global_bgm" => out.form_global_bgm = n as u32,
        "form_global_bgm_table" => out.form_global_bgm_table = n as u32,
        "form_global_pcm" => out.form_global_pcm = n as u32,
        "form_global_pcmch" => out.form_global_pcmch = n as u32,
        "form_global_se" => out.form_global_se = n as u32,
        "form_global_pcm_event" => out.form_global_pcm_event = n as u32,
        "form_global_excall" => out.form_global_excall = n as u32,
        "form_global_koe_st" => out.form_global_koe_st = n as u32,

        "form_global_input" => out.form_global_input = n as u32,
        "form_global_mouse" => out.form_global_mouse = n as u32,
        "form_global_keylist" => out.form_global_keylist = n as u32,
        "form_global_key" => out.form_global_key = n as u32,

        "form_global_syscom" => out.form_global_syscom = n as u32,
        "form_global_script" => out.form_global_script = n as u32,
        "form_global_system" => out.form_global_system = n as u32,
        "form_global_frame_action" => out.form_global_frame_action = n as u32,
        "form_global_frame_action_ch" => out.form_global_frame_action_ch = n as u32,

        "form_global_screen" => out.form_global_screen = n as u32,
        "form_global_msgbk" => out.form_global_msgbk = n as u32,


        "form_global_math" => out.form_global_math = n as u32,
        "form_global_cgtable" => out.form_global_cgtable = n as u32,
        "form_global_database" => out.form_global_database = n as u32,
        "form_global_g00buf" => out.form_global_g00buf = n as u32,
        "form_global_mask" => out.form_global_mask = n as u32,
        "form_global_editbox" => out.form_global_editbox = n as u32,
        "form_global_file" => out.form_global_file = n as u32,
        "form_global_steam" => out.form_global_steam = n as u32,

        "screen_sel_effect" => out.screen_sel_effect = n as i32,
        "screen_sel_quake" => out.screen_sel_quake = n as i32,
        "screen_sel_shake" => out.screen_sel_shake = n as i32,

        "screen_x" => out.screen_x = n as i32,
        "screen_y" => out.screen_y = n as i32,
        "screen_z" => out.screen_z = n as i32,
        "screen_mono" => out.screen_mono = n as i32,
        "screen_reverse" => out.screen_reverse = n as i32,
        "screen_bright" => out.screen_bright = n as i32,
        "screen_dark" => out.screen_dark = n as i32,
        "screen_color_r" => out.screen_color_r = n as i32,
        "screen_color_g" => out.screen_color_g = n as i32,
        "screen_color_b" => out.screen_color_b = n as i32,
        "screen_color_rate" => out.screen_color_rate = n as i32,
        "screen_color_add_r" => out.screen_color_add_r = n as i32,
        "screen_color_add_g" => out.screen_color_add_g = n as i32,
        "screen_color_add_b" => out.screen_color_add_b = n as i32,

        "screen_x_eve" => out.screen_x_eve = n as i32,
        "screen_y_eve" => out.screen_y_eve = n as i32,
        "screen_z_eve" => out.screen_z_eve = n as i32,
        "screen_mono_eve" => out.screen_mono_eve = n as i32,
        "screen_reverse_eve" => out.screen_reverse_eve = n as i32,
        "screen_bright_eve" => out.screen_bright_eve = n as i32,
        "screen_dark_eve" => out.screen_dark_eve = n as i32,
        "screen_color_r_eve" => out.screen_color_r_eve = n as i32,
        "screen_color_g_eve" => out.screen_color_g_eve = n as i32,
        "screen_color_b_eve" => out.screen_color_b_eve = n as i32,
        "screen_color_rate_eve" => out.screen_color_rate_eve = n as i32,
        "screen_color_add_r_eve" => out.screen_color_add_r_eve = n as i32,
        "screen_color_add_g_eve" => out.screen_color_add_g_eve = n as i32,
        "screen_color_add_b_eve" => out.screen_color_add_b_eve = n as i32,

        "effect_init" => out.effect_init = n as i32,
        "effect_wipe_copy" => out.effect_wipe_copy = n as i32,
        "effect_wipe_erase" => out.effect_wipe_erase = n as i32,
        "effect_x" => out.effect_x = n as i32,
        "effect_y" => out.effect_y = n as i32,
        "effect_z" => out.effect_z = n as i32,
        "effect_mono" => out.effect_mono = n as i32,
        "effect_reverse" => out.effect_reverse = n as i32,
        "effect_bright" => out.effect_bright = n as i32,
        "effect_dark" => out.effect_dark = n as i32,
        "effect_color_r" => out.effect_color_r = n as i32,
        "effect_color_g" => out.effect_color_g = n as i32,
        "effect_color_b" => out.effect_color_b = n as i32,
        "effect_color_rate" => out.effect_color_rate = n as i32,
        "effect_color_add_r" => out.effect_color_add_r = n as i32,
        "effect_color_add_g" => out.effect_color_add_g = n as i32,
        "effect_color_add_b" => out.effect_color_add_b = n as i32,
        "effect_x_eve" => out.effect_x_eve = n as i32,
        "effect_y_eve" => out.effect_y_eve = n as i32,
        "effect_z_eve" => out.effect_z_eve = n as i32,
        "effect_mono_eve" => out.effect_mono_eve = n as i32,
        "effect_reverse_eve" => out.effect_reverse_eve = n as i32,
        "effect_bright_eve" => out.effect_bright_eve = n as i32,
        "effect_dark_eve" => out.effect_dark_eve = n as i32,
        "effect_color_r_eve" => out.effect_color_r_eve = n as i32,
        "effect_color_g_eve" => out.effect_color_g_eve = n as i32,
        "effect_color_b_eve" => out.effect_color_b_eve = n as i32,
        "effect_color_rate_eve" => out.effect_color_rate_eve = n as i32,
        "effect_color_add_r_eve" => out.effect_color_add_r_eve = n as i32,
        "effect_color_add_g_eve" => out.effect_color_add_g_eve = n as i32,
        "effect_color_add_b_eve" => out.effect_color_add_b_eve = n as i32,
        "effect_begin_order" => out.effect_begin_order = n as i32,
        "effect_end_order" => out.effect_end_order = n as i32,
        "effect_begin_layer" => out.effect_begin_layer = n as i32,
        "effect_end_layer" => out.effect_end_layer = n as i32,

        "exkey_decide" => out.exkey_decide = n as i32,
        "exkey_cancel" => out.exkey_cancel = n as i32,

        "input_op_decide" => out.input_op_decide = n as i32,
        "input_op_cancel" => out.input_op_cancel = n as i32,
        "input_op_clear" => out.input_op_clear = n as i32,
        "input_op_next" => out.input_op_next = n as i32,

        "mouse_op_x" => out.mouse_op_x = n as i32,
        "mouse_op_y" => out.mouse_op_y = n as i32,
        "mouse_op_clear" => out.mouse_op_clear = n as i32,
        "mouse_op_wheel" => out.mouse_op_wheel = n as i32,
        "mouse_op_left" => out.mouse_op_left = n as i32,
        "mouse_op_right" => out.mouse_op_right = n as i32,
        "mouse_op_next" => out.mouse_op_next = n as i32,
        "mouse_op_get_pos" => out.mouse_op_get_pos = n as i32,
        "mouse_op_set_pos" => out.mouse_op_set_pos = n as i32,

        "keylist_op_wait" => out.keylist_op_wait = n as i32,
        "keylist_op_wait_force" => out.keylist_op_wait_force = n as i32,
        "keylist_op_clear" => out.keylist_op_clear = n as i32,
        "keylist_op_next" => out.keylist_op_next = n as i32,

        "key_op_dir" => out.key_op_dir = n as i32,
        "key_op_on_down" => out.key_op_on_down = n as i32,
        "key_op_on_up" => out.key_op_on_up = n as i32,
        "key_op_on_down_up" => out.key_op_on_down_up = n as i32,
        "key_op_is_down" => out.key_op_is_down = n as i32,
        "key_op_is_up" => out.key_op_is_up = n as i32,
        "key_op_on_flick" => out.key_op_on_flick = n as i32,
        "key_op_flick" => out.key_op_flick = n as i32,
        "key_op_flick_angle" => out.key_op_flick_angle = n as i32,


        // MATH element codes
        "math_max" => out.math_max = n as i32,
        "math_min" => out.math_min = n as i32,
        "math_limit" => out.math_limit = n as i32,
        "math_abs" => out.math_abs = n as i32,
        "math_rand" => out.math_rand = n as i32,
        "math_sqrt" => out.math_sqrt = n as i32,
        "math_log" => out.math_log = n as i32,
        "math_log2" => out.math_log2 = n as i32,
        "math_log10" => out.math_log10 = n as i32,
        "math_sin" => out.math_sin = n as i32,
        "math_cos" => out.math_cos = n as i32,
        "math_tan" => out.math_tan = n as i32,
        "math_arcsin" => out.math_arcsin = n as i32,
        "math_arccos" => out.math_arccos = n as i32,
        "math_arctan" => out.math_arctan = n as i32,
        "math_distance" => out.math_distance = n as i32,
        "math_angle" => out.math_angle = n as i32,
        "math_linear" => out.math_linear = n as i32,
        "math_tostr" => out.math_tostr = n as i32,
        "math_tostr_zero" => out.math_tostr_zero = n as i32,

        // CGTABLE element codes
        "cgtable_flag" => out.cgtable_flag = n as i32,
        "cgtable_set_disable" => out.cgtable_set_disable = n as i32,
        "cgtable_set_enable" => out.cgtable_set_enable = n as i32,
        "cgtable_set_all_flag" => out.cgtable_set_all_flag = n as i32,
        "cgtable_get_cg_cnt" => out.cgtable_get_cg_cnt = n as i32,
        "cgtable_get_look_cnt" => out.cgtable_get_look_cnt = n as i32,
        "cgtable_get_look_percent" => out.cgtable_get_look_percent = n as i32,
        "cgtable_get_flag_no_by_name" => out.cgtable_get_flag_no_by_name = n as i32,
        "cgtable_get_look_by_name" => out.cgtable_get_look_by_name = n as i32,
        "cgtable_set_look_by_name" => out.cgtable_set_look_by_name = n as i32,
        "cgtable_get_name_by_flag_no" => out.cgtable_get_name_by_flag_no = n as i32,

        // DATABASE element codes
        "database_list_get_size" => out.database_list_get_size = n as i32,
        "database_get_num" => out.database_get_num = n as i32,
        "database_get_str" => out.database_get_str = n as i32,
        "database_check_item" => out.database_check_item = n as i32,
        "database_check_column" => out.database_check_column = n as i32,
        "database_find_num" => out.database_find_num = n as i32,
        "database_find_str" => out.database_find_str = n as i32,
        "database_find_str_real" => out.database_find_str_real = n as i32,

        // G00BUF element codes
        "g00buf_list_get_size" => out.g00buf_list_get_size = n as i32,
        "g00buf_list_free_all" => out.g00buf_list_free_all = n as i32,
        "g00buf_load" => out.g00buf_load = n as i32,
        "g00buf_free" => out.g00buf_free = n as i32,

        // FILE element codes
        "file_preload_omv" => out.file_preload_omv = n as i32,

        // STEAM element codes
        "steam_set_achievement" => out.steam_set_achievement = n as i32,
        "steam_reset_all_status" => out.steam_reset_all_status = n as i32,

        // Element helpers
        "elm_array" => out.elm_array = n as i32,
        "elm_up" => out.elm_up = n as i32,
        "stage_elm_object" => out.stage_elm_object = n as i32,
        "stage_elm_world" => out.stage_elm_world = n as i32,

        // World list element codes
        "worldlist_create" => out.worldlist_create = n as i32,
        "worldlist_destroy" => out.worldlist_destroy = n as i32,

        // World element codes
        "world_init" => out.world_init = n as i32,
        "world_get_no" => out.world_get_no = n as i32,
        "world_camera_eye_x" => out.world_camera_eye_x = n as i32,
        "world_camera_eye_y" => out.world_camera_eye_y = n as i32,
        "world_camera_eye_z" => out.world_camera_eye_z = n as i32,
        "world_camera_pint_x" => out.world_camera_pint_x = n as i32,
        "world_camera_pint_y" => out.world_camera_pint_y = n as i32,
        "world_camera_pint_z" => out.world_camera_pint_z = n as i32,
        "world_camera_up_x" => out.world_camera_up_x = n as i32,
        "world_camera_up_y" => out.world_camera_up_y = n as i32,
        "world_camera_up_z" => out.world_camera_up_z = n as i32,
        "world_camera_eye_x_eve" => out.world_camera_eye_x_eve = n as i32,
        "world_camera_eye_y_eve" => out.world_camera_eye_y_eve = n as i32,
        "world_camera_eye_z_eve" => out.world_camera_eye_z_eve = n as i32,
        "world_camera_pint_x_eve" => out.world_camera_pint_x_eve = n as i32,
        "world_camera_pint_y_eve" => out.world_camera_pint_y_eve = n as i32,
        "world_camera_pint_z_eve" => out.world_camera_pint_z_eve = n as i32,
        "world_camera_up_x_eve" => out.world_camera_up_x_eve = n as i32,
        "world_camera_up_y_eve" => out.world_camera_up_y_eve = n as i32,
        "world_camera_up_z_eve" => out.world_camera_up_z_eve = n as i32,
        "world_camera_view_angle" => out.world_camera_view_angle = n as i32,
        "world_set_camera_eye" => out.world_set_camera_eye = n as i32,
        "world_calc_camera_eye" => out.world_calc_camera_eye = n as i32,
        "world_set_camera_pint" => out.world_set_camera_pint = n as i32,
        "world_calc_camera_pint" => out.world_calc_camera_pint = n as i32,
        "world_set_camera_up" => out.world_set_camera_up = n as i32,
        "world_mono" => out.world_mono = n as i32,
        "world_set_camera_eve_xz_rotate" => out.world_set_camera_eve_xz_rotate = n as i32,
        "world_order" => out.world_order = n as i32,
        "world_layer" => out.world_layer = n as i32,
        "world_wipe_copy" => out.world_wipe_copy = n as i32,
        "world_wipe_erase" => out.world_wipe_erase = n as i32,

        // Object properties / ops
        "obj_disp" => out.obj_disp = n as i32,
        "obj_patno" => out.obj_patno = n as i32,
        "obj_alpha" => out.obj_alpha = n as i32,
        "obj_layer" => out.obj_layer = n as i32,
        "obj_order" => out.obj_order = n as i32,
        "obj_x" => out.obj_x = n as i32,
        "obj_y" => out.obj_y = n as i32,
        "obj_z" => out.obj_z = n as i32,
        "obj_create" => out.obj_create = n as i32,
        "obj_create_number" => out.obj_create_number = n as i32,
        "obj_create_weather" => out.obj_create_weather = n as i32,
        "obj_create_mesh" => out.obj_create_mesh = n as i32,
        "obj_create_billboard" => out.obj_create_billboard = n as i32,
        "obj_create_save_thumb" => out.obj_create_save_thumb = n as i32,
        "obj_create_capture_thumb" => out.obj_create_capture_thumb = n as i32,
        "obj_create_capture" => out.obj_create_capture = n as i32,
        "obj_create_movie" => out.obj_create_movie = n as i32,
        "obj_create_movie_loop" => out.obj_create_movie_loop = n as i32,
        "obj_create_movie_wait" => out.obj_create_movie_wait = n as i32,
        "obj_create_movie_wait_key" => out.obj_create_movie_wait_key = n as i32,
        "obj_create_emote" => out.obj_create_emote = n as i32,
        "obj_create_copy_from" => out.obj_create_copy_from = n as i32,
        "obj_set_weather_param_type_a" => out.obj_set_weather_param_type_a = n as i32,
        "obj_set_weather_param_type_b" => out.obj_set_weather_param_type_b = n as i32,
        "obj_pause_movie" => out.obj_pause_movie = n as i32,
        "obj_resume_movie" => out.obj_resume_movie = n as i32,
        "obj_seek_movie" => out.obj_seek_movie = n as i32,
        "obj_get_movie_seek_time" => out.obj_get_movie_seek_time = n as i32,
        "obj_check_movie" => out.obj_check_movie = n as i32,
        "obj_wait_movie" => out.obj_wait_movie = n as i32,
        "obj_wait_movie_key" => out.obj_wait_movie_key = n as i32,
        "obj_end_movie_loop" => out.obj_end_movie_loop = n as i32,
        "obj_set_movie_auto_free" => out.obj_set_movie_auto_free = n as i32,
        "obj_clear_button" => out.obj_clear_button = n as i32,
        "obj_set_button" => out.obj_set_button = n as i32,
        "obj_set_button_group" => out.obj_set_button_group = n as i32,
        "obj_set_button_pushkeep" => out.obj_set_button_pushkeep = n as i32,
        "obj_get_button_pushkeep" => out.obj_get_button_pushkeep = n as i32,
        "obj_set_button_alpha_test" => out.obj_set_button_alpha_test = n as i32,
        "obj_get_button_alpha_test" => out.obj_get_button_alpha_test = n as i32,
        "obj_set_button_state_normal" => out.obj_set_button_state_normal = n as i32,
        "obj_set_button_state_select" => out.obj_set_button_state_select = n as i32,
        "obj_set_button_state_disable" => out.obj_set_button_state_disable = n as i32,
        "obj_get_button_state" => out.obj_get_button_state = n as i32,
        "obj_get_button_hit_state" => out.obj_get_button_hit_state = n as i32,
        "obj_get_button_real_state" => out.obj_get_button_real_state = n as i32,
        "obj_set_button_call" => out.obj_set_button_call = n as i32,
        "obj_clear_button_call" => out.obj_clear_button_call = n as i32,
        "obj_frame_action" => out.obj_frame_action = n as i32,
        "obj_frame_action_ch" => out.obj_frame_action_ch = n as i32,
        "obj_load_gan" => out.obj_load_gan = n as i32,
        "obj_start_gan" => out.obj_start_gan = n as i32,

        // Best-effort stage ops
        "obj_wipe_copy" => out.obj_wipe_copy = n as i32,
        "obj_wipe_erase" => out.obj_wipe_erase = n as i32,
        "obj_click_disable" => out.obj_click_disable = n as i32,

        // Object extended subset
        "obj_world" => out.obj_world = n as i32,
        "obj_center_x" => out.obj_center_x = n as i32,
        "obj_center_y" => out.obj_center_y = n as i32,
        "obj_center_z" => out.obj_center_z = n as i32,
        "obj_set_center" => out.obj_set_center = n as i32,
        "obj_scale_x" => out.obj_scale_x = n as i32,
        "obj_scale_y" => out.obj_scale_y = n as i32,
        "obj_scale_z" => out.obj_scale_z = n as i32,
        "obj_set_scale" => out.obj_set_scale = n as i32,
        "obj_rotate_x" => out.obj_rotate_x = n as i32,
        "obj_rotate_y" => out.obj_rotate_y = n as i32,
        "obj_rotate_z" => out.obj_rotate_z = n as i32,
        "obj_set_rotate" => out.obj_set_rotate = n as i32,
        "obj_clip_left" => out.obj_clip_left = n as i32,
        "obj_clip_top" => out.obj_clip_top = n as i32,
        "obj_clip_right" => out.obj_clip_right = n as i32,
        "obj_clip_bottom" => out.obj_clip_bottom = n as i32,
        "obj_set_clip" => out.obj_set_clip = n as i32,
        "obj_src_clip_left" => out.obj_src_clip_left = n as i32,
        "obj_src_clip_top" => out.obj_src_clip_top = n as i32,
        "obj_src_clip_right" => out.obj_src_clip_right = n as i32,
        "obj_src_clip_bottom" => out.obj_src_clip_bottom = n as i32,
        "obj_set_src_clip" => out.obj_set_src_clip = n as i32,
        "obj_tr" => out.obj_tr = n as i32,
        "obj_mono" => out.obj_mono = n as i32,
        "obj_reverse" => out.obj_reverse = n as i32,
        "obj_bright" => out.obj_bright = n as i32,
        "obj_dark" => out.obj_dark = n as i32,
        "obj_color_r" => out.obj_color_r = n as i32,
        "obj_color_g" => out.obj_color_g = n as i32,
        "obj_color_b" => out.obj_color_b = n as i32,
        "obj_color_rate" => out.obj_color_rate = n as i32,
        "obj_color_add_r" => out.obj_color_add_r = n as i32,
        "obj_color_add_g" => out.obj_color_add_g = n as i32,
        "obj_color_add_b" => out.obj_color_add_b = n as i32,

        "obj_set_pos" => out.obj_set_pos = n as i32,
        "obj_x_rep" => out.obj_x_rep = n as i32,
        "obj_y_rep" => out.obj_y_rep = n as i32,
        "obj_z_rep" => out.obj_z_rep = n as i32,

        "obj_center_rep_x" => out.obj_center_rep_x = n as i32,
        "obj_center_rep_y" => out.obj_center_rep_y = n as i32,
        "obj_center_rep_z" => out.obj_center_rep_z = n as i32,
        "obj_set_center_rep" => out.obj_set_center_rep = n as i32,

        "obj_clip_use" => out.obj_clip_use = n as i32,
        "obj_src_clip_use" => out.obj_src_clip_use = n as i32,

        "obj_mask_no" => out.obj_mask_no = n as i32,
        "obj_tonecurve_no" => out.obj_tonecurve_no = n as i32,
        "obj_culling" => out.obj_culling = n as i32,
        "obj_alpha_test" => out.obj_alpha_test = n as i32,
        "obj_alpha_blend" => out.obj_alpha_blend = n as i32,
        "obj_blend" => out.obj_blend = n as i32,
        "obj_light_no" => out.obj_light_no = n as i32,
        "obj_fog_use" => out.obj_fog_use = n as i32,

        "obj_patno_eve" => out.obj_patno_eve = n as i32,
        "obj_x_eve" => out.obj_x_eve = n as i32,
        "obj_y_eve" => out.obj_y_eve = n as i32,
        "obj_z_eve" => out.obj_z_eve = n as i32,

        "obj_x_rep_eve" => out.obj_x_rep_eve = n as i32,
        "obj_y_rep_eve" => out.obj_y_rep_eve = n as i32,
        "obj_z_rep_eve" => out.obj_z_rep_eve = n as i32,

        "obj_center_x_eve" => out.obj_center_x_eve = n as i32,
        "obj_center_y_eve" => out.obj_center_y_eve = n as i32,
        "obj_center_z_eve" => out.obj_center_z_eve = n as i32,

        "obj_center_rep_x_eve" => out.obj_center_rep_x_eve = n as i32,
        "obj_center_rep_y_eve" => out.obj_center_rep_y_eve = n as i32,
        "obj_center_rep_z_eve" => out.obj_center_rep_z_eve = n as i32,

        "obj_scale_x_eve" => out.obj_scale_x_eve = n as i32,
        "obj_scale_y_eve" => out.obj_scale_y_eve = n as i32,
        "obj_scale_z_eve" => out.obj_scale_z_eve = n as i32,

        "obj_rotate_x_eve" => out.obj_rotate_x_eve = n as i32,
        "obj_rotate_y_eve" => out.obj_rotate_y_eve = n as i32,
        "obj_rotate_z_eve" => out.obj_rotate_z_eve = n as i32,

        "obj_clip_left_eve" => out.obj_clip_left_eve = n as i32,
        "obj_clip_top_eve" => out.obj_clip_top_eve = n as i32,
        "obj_clip_right_eve" => out.obj_clip_right_eve = n as i32,
        "obj_clip_bottom_eve" => out.obj_clip_bottom_eve = n as i32,

        "obj_src_clip_left_eve" => out.obj_src_clip_left_eve = n as i32,
        "obj_src_clip_top_eve" => out.obj_src_clip_top_eve = n as i32,
        "obj_src_clip_right_eve" => out.obj_src_clip_right_eve = n as i32,
        "obj_src_clip_bottom_eve" => out.obj_src_clip_bottom_eve = n as i32,

        "obj_tr_eve" => out.obj_tr_eve = n as i32,
        "obj_tr_rep" => out.obj_tr_rep = n as i32,
        "obj_tr_rep_eve" => out.obj_tr_rep_eve = n as i32,

        "obj_mono_eve" => out.obj_mono_eve = n as i32,
        "obj_reverse_eve" => out.obj_reverse_eve = n as i32,
        "obj_bright_eve" => out.obj_bright_eve = n as i32,
        "obj_dark_eve" => out.obj_dark_eve = n as i32,

        "obj_color_r_eve" => out.obj_color_r_eve = n as i32,
        "obj_color_g_eve" => out.obj_color_g_eve = n as i32,
        "obj_color_b_eve" => out.obj_color_b_eve = n as i32,
        "obj_color_rate_eve" => out.obj_color_rate_eve = n as i32,
        "obj_color_add_r_eve" => out.obj_color_add_r_eve = n as i32,
        "obj_color_add_g_eve" => out.obj_color_add_g_eve = n as i32,
        "obj_color_add_b_eve" => out.obj_color_add_b_eve = n as i32,

        "obj_get_pat_cnt" => out.obj_get_pat_cnt = n as i32,
        "obj_get_size_x" => out.obj_get_size_x = n as i32,
        "obj_get_size_y" => out.obj_get_size_y = n as i32,
        "obj_get_size_z" => out.obj_get_size_z = n as i32,
        "obj_get_pixel_color_r" => out.obj_get_pixel_color_r = n as i32,
        "obj_get_pixel_color_g" => out.obj_get_pixel_color_g = n as i32,
        "obj_get_pixel_color_b" => out.obj_get_pixel_color_b = n as i32,
        "obj_get_pixel_color_a" => out.obj_get_pixel_color_a = n as i32,

        "obj_f" => out.obj_f = n as i32,

        "obj_change_file" => out.obj_change_file = n as i32,
        "obj_exist_type" => out.obj_exist_type = n as i32,
        "obj_set_string" => out.obj_set_string = n as i32,
        "obj_get_string" => out.obj_get_string = n as i32,
        "obj_set_string_param" => out.obj_set_string_param = n as i32,
        "obj_set_number" => out.obj_set_number = n as i32,
        "obj_get_number" => out.obj_get_number = n as i32,
        "obj_set_number_param" => out.obj_set_number_param = n as i32,

        "obj_all_eve" => out.obj_all_eve = n as i32,
        "elm_allevent_end" => out.elm_allevent_end = n as i32,
        "elm_allevent_wait" => out.elm_allevent_wait = n as i32,
        "elm_allevent_check" => out.elm_allevent_check = n as i32,

        "obj_init" => out.obj_init = n as i32,
        "obj_free" => out.obj_free = n as i32,
        "obj_init_param" => out.obj_init_param = n as i32,
        "obj_get_file_name" => out.obj_get_file_name = n as i32,
        _ => {
            // Unknown key: ignore.
        }
    }
}

fn try_load_u32_map_file(path: &Path, out: &mut HashMap<u32, String>) {
    if !path.is_file() {
        return;
    }
    if let Ok(text) = fs::read_to_string(path) {
        parse_u32_name_map_str(out, &text);
    }
}

fn try_load_i64_map_file(path: &Path, out: &mut HashMap<i64, String>) {
    if !path.is_file() {
        return;
    }
    if let Ok(text) = fs::read_to_string(path) {
        parse_i64_name_map_str(out, &text);
    }
}

fn load_u32_name_map_env(spec_env: &str, file_env: &str, out: &mut HashMap<u32, String>) {
    if let Ok(spec) = env::var(spec_env) {
        parse_u32_name_map_str(out, &spec);
    }
    if let Ok(path) = env::var(file_env) {
        if let Ok(text) = fs::read_to_string(Path::new(&path)) {
            parse_u32_name_map_str(out, &text);
        }
    }
}

fn load_i64_name_map_env(spec_env: &str, file_env: &str, out: &mut HashMap<i64, String>) {
    if let Ok(spec) = env::var(spec_env) {
        parse_i64_name_map_str(out, &spec);
    }
    if let Ok(path) = env::var(file_env) {
        if let Ok(text) = fs::read_to_string(Path::new(&path)) {
            parse_i64_name_map_str(out, &text);
        }
    }
}

fn parse_u32_name_map_str(out: &mut HashMap<u32, String>, text: &str) {
    for line in text.split(|c| c == ';' || c == '\n') {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        let Some(n) = parse_i64(k) else {
            continue;
        };
        if n < 0 {
            continue;
        }
        out.insert(n as u32, v.to_string());
    }
}

fn parse_i64_name_map_str(out: &mut HashMap<i64, String>, text: &str) {
    for line in text.split(|c| c == ';' || c == '\n') {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        let Some(n) = parse_i64(k) else {
            continue;
        };
        out.insert(n, v.to_string());
    }
}

fn parse_i64(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(rest.trim(), 16).ok()
    } else {
        s.parse::<i64>().ok()
    }
}
