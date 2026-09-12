use anyhow::Result;

mod save_versions;

use crate::runtime::globals::{
    SaveSlotState, SyscomFallbackDialogKind, SyscomFallbackDialogState, SyscomPendingProc,
    SyscomPendingProcKind, SystemMessageBoxButton, ToggleFeatureState, ValueFeatureState,
};
use crate::runtime::{CommandContext, RuntimeSaveKind, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::assets::RgbaImage;
use crate::original_save::{self, SaveKind};

use super::prop_access;

use super::codes::syscom_op::*;

struct Call<'a> {
    op: i32,
    params: &'a [Value],
}

fn parse_call<'a>(ctx: &CommandContext, form_id: u32, args: &'a [Value]) -> Option<Call<'a>> {
    let (chain_pos, chain) = prop_access::parse_element_chain_ctx(ctx, form_id, args)?;
    if chain.len() < 2 {
        return None;
    }
    let params = prop_access::script_args(args, chain_pos);
    Some(Call {
        op: chain[1],
        params,
    })
}

fn p_i64(params: &[Value], idx: usize) -> i64 {
    params.get(idx).and_then(|v| v.as_i64()).unwrap_or(0)
}
fn p_bool(params: &[Value], idx: usize) -> bool {
    p_i64(params, idx) != 0
}

fn sg_debug_enabled_local() -> bool {
    std::env::var_os("SG_DEBUG").is_some()
}

fn set_syscom_pending_proc(ctx: &mut CommandContext, kind: SyscomPendingProcKind) {
    ctx.globals.syscom.pending_proc = Some(SyscomPendingProc {
        kind,
        warning: false,
        se_play: false,
        fade_out: false,
        leave_msgbk: false,
        save_id: 0,
    });
    ctx.globals.syscom.menu_open = false;
    ctx.globals.syscom.menu_kind = None;
    ctx.globals.syscom.menu_result = None;
}

fn gameexe_unquoted_owned(ctx: &CommandContext, key: &str) -> String {
    ctx.tables
        .gameexe
        .as_ref()
        .and_then(|cfg| cfg.get_unquoted(key))
        .unwrap_or("")
        .to_string()
}


fn gameexe_value_owned(ctx: &CommandContext, key: &str) -> String {
    ctx.tables
        .gameexe
        .as_ref()
        .and_then(|cfg| cfg.get_value(key))
        .unwrap_or("")
        .to_string()
}

fn parse_i64_list_local(raw: &str) -> Vec<i64> {
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

fn parse_first_i64_local(raw: &str) -> Option<i64> {
    raw.split(|c: char| c == ',' || c.is_whitespace())
        .find_map(|part| {
            let t = part.trim();
            if t.is_empty() { None } else { t.parse::<i64>().ok() }
        })
}

fn gameexe_i64_or(ctx: &CommandContext, key: &str, default: i64) -> i64 {
    ctx.tables
        .gameexe
        .as_ref()
        .and_then(|cfg| cfg.get_i64(key))
        .unwrap_or(default)
}

fn gameexe_bool_or(ctx: &CommandContext, key: &str, default: bool) -> bool {
    gameexe_i64_or(ctx, key, if default { 1 } else { 0 }) != 0
}

fn config_default_sound_volume(ctx: &CommandContext, sound_type: usize) -> i64 {
    let key = match sound_type {
        0 => Some("CONFIG.VOLUME.BGM"),
        1 => Some("CONFIG.VOLUME.KOE"),
        2 => Some("CONFIG.VOLUME.PCM"),
        3 => Some("CONFIG.VOLUME.SE"),
        4 => Some("CONFIG.VOLUME.MOV"),
        _ => None,
    };
    key.map(|key| gameexe_i64_or(ctx, key, 255))
        .unwrap_or(255)
        .clamp(0, 255)
}

fn config_default_sound_onoff(_ctx: &CommandContext, _sound_type: usize) -> bool {
    // The original Gameexe parser initializes every sound channel to enabled;
    // there is no #CONFIG directive that overrides these five checks.
    true
}

fn config_default_chrkoe(ctx: &CommandContext, index: usize) -> crate::runtime::globals::ConfigChrKoeState {
    let Some(entry) = ctx
        .tables
        .gameexe
        .as_ref()
        .and_then(|cfg| cfg.get_indexed_entry("CHRKOE", index))
    else {
        return crate::runtime::globals::ConfigChrKoeState::default();
    };
    // #CHRKOE.i = name, check_mode, check_name, onoff, volume, (...)
    let onoff = entry
        .item_unquoted(3)
        .and_then(|v| v.parse::<i64>().ok())
        .map(|v| v != 0)
        .unwrap_or(true);
    let volume = entry
        .item_unquoted(4)
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(255)
        .clamp(0, 255);
    crate::runtime::globals::ConfigChrKoeState { onoff, volume }
}

/// C_tnm_chrkoe masks names until their first named dialogue, unless always visible.
pub fn config_chrkoe_name_visible(ctx: &CommandContext, index: usize) -> bool {
    let Some(entry) = ctx.tables.gameexe.as_ref().and_then(|cfg| cfg.get_indexed_entry("CHRKOE", index)) else {
        return true;
    };
    let name = entry.item_unquoted(0).unwrap_or("");
    ctx.globals.syscom.chrkoe_look_flags.get(name).copied().unwrap_or_else(|| {
        !matches!(entry.item_unquoted(1), Some("0" | "2"))
    })
}

pub fn reveal_config_voice_name(ctx: &mut CommandContext, spoken_name: &str) {
    let Some(cfg) = ctx.tables.gameexe.as_ref() else { return; };
    for i in 0..configured_chrkoe_count(ctx) {
        let Some(entry) = cfg.get_indexed_entry("CHRKOE", i) else { continue; };
        if entry.item_unquoted(1) == Some("2")
            && (entry.item_unquoted(0) == Some(spoken_name) || entry.item_unquoted(2) == Some(spoken_name)) {
            let name = entry.item_unquoted(0).unwrap_or("").to_owned();
            ctx.globals.syscom.chrkoe_look_flags.insert(name, true);
        }
    }
}

fn config_default_indexed_bool(
    ctx: &CommandContext,
    prefix: &str,
    index: usize,
    default: bool,
) -> bool {
    ctx.tables
        .gameexe
        .as_ref()
        .and_then(|cfg| cfg.get_indexed_field_unquoted(prefix, index, "ONOFF"))
        .and_then(|v| v.parse::<i64>().ok())
        .map(|v| v != 0)
        .unwrap_or(default)
}

fn config_default_indexed_mode(ctx: &CommandContext, index: usize) -> i64 {
    ctx.tables
        .gameexe
        .as_ref()
        .and_then(|cfg| cfg.get_indexed_field_unquoted("CONFIG.GLOBAL_EXTRA_MODE", index, "MODE"))
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0)
}

fn config_default_font_name(ctx: &CommandContext) -> String {
    match gameexe_i64_or(ctx, "CONFIG.FONT.TYPE", 0) {
        0 => "ＭＳ ゴシック".to_string(),
        1 => "ＭＳ 明朝".to_string(),
        2 => "メイリオ".to_string(),
        3 => {
            let name = gameexe_unquoted_owned(ctx, "CONFIG.FONT.NAME");
            if name.is_empty() {
                "ＭＳ ゴシック".to_string()
            } else {
                name
            }
        }
        value => {
            log::error!(
                "invalid CONFIG.FONT.TYPE value {value}; using original default ＭＳ ゴシック"
            );
            "ＭＳ ゴシック".to_string()
        }
    }
}

pub fn original_config_defaults(ctx: &CommandContext) -> crate::runtime::globals::OriginalConfigRuntimeState {
    let mut cfg = crate::runtime::globals::OriginalConfigRuntimeState::default();
    cfg.screen_size_mode = gameexe_i64_or(ctx, "CONFIG.WINDOW_MODE", 0).clamp(0, 1);
    let screen_size = (ctx.screen_w.max(1) as i64, ctx.screen_h.max(1) as i64);
    cfg.screen_size_free = screen_size;
    // The current host exposes one active display mode to the VM.  Persist that
    // actual mode instead of the all-zero placeholder used by the old port.
    cfg.fullscreen_display_cnt = 1;
    cfg.fullscreen_display_no = 0;
    cfg.fullscreen_resolution_cnt = 1;
    cfg.fullscreen_resolution_no = 0;
    cfg.fullscreen_resolution = screen_size;
    cfg.all_sound_user_volume = gameexe_i64_or(ctx, "CONFIG.VOLUME.ALL", 255).clamp(0, 255);
    for index in 0..cfg.sound_user_volume.len() {
        cfg.sound_user_volume[index] = config_default_sound_volume(ctx, index);
        cfg.play_sound_check[index] = config_default_sound_onoff(ctx, index);
    }
    cfg.bgmfade_volume = gameexe_i64_or(ctx, "CONFIG.BGMFADE_VOLUME", 192).clamp(0, 255);
    cfg.bgmfade_use_check = gameexe_bool_or(ctx, "CONFIG.BGMFADE_ONOFF", true);
    let (r, g, b, a) = config_filter_color_default(ctx);
    cfg.filter_color_argb = ((a as u32) << 24)
        | ((r as u32) << 16)
        | ((g as u32) << 8)
        | b as u32;
    cfg.font_name = config_default_font_name(ctx);
    cfg.font_futoku = gameexe_bool_or(ctx, "CONFIG.FONT.FUTOKU", false);
    cfg.font_shadow = gameexe_i64_or(ctx, "CONFIG.FONT.SHADOW", 2).clamp(0, 3);
    cfg.message_speed = gameexe_i64_or(ctx, "CONFIG.MESSAGE_SPEED", 20);
    cfg.message_speed_nowait = gameexe_bool_or(ctx, "CONFIG.MESSAGE_SPEED_NOWAIT.ONOFF", false);
    cfg.mouse_cursor_hide_onoff = config_mouse_cursor_hide_onoff_default(ctx) != 0;
    cfg.mouse_cursor_hide_time = config_mouse_cursor_hide_time_default(ctx);
    cfg.chrkoe = (0..configured_chrkoe_count(ctx))
        .map(|index| config_default_chrkoe(ctx, index))
        .collect();
    cfg.message_chrcolor_flag = gameexe_bool_or(ctx, "CONFIG.MESSAGE_CHRCOLOR.ONOFF", true);
    cfg.object_disp_flag = (0..4)
        .map(|index| config_default_indexed_bool(ctx, "CONFIG.OBJECT_DISP", index, true))
        .collect();
    cfg.global_extra_switch_flag = (0..4)
        .map(|index| config_default_indexed_bool(ctx, "CONFIG.GLOBAL_EXTRA_SWITCH", index, true))
        .collect();
    cfg.global_extra_mode_flag = (0..4)
        .map(|index| config_default_indexed_mode(ctx, index))
        .collect();
    cfg.sleep_flag = gameexe_bool_or(ctx, "CONFIG.SLEEP.ONOFF", false);
    cfg.no_wipe_anime_flag = gameexe_bool_or(ctx, "CONFIG.NO_WIPE_ANIME.ONOFF", false);
    cfg.skip_wipe_anime_flag = gameexe_bool_or(ctx, "CONFIG.SKIP_WIPE_ANIME.ONOFF", true);
    cfg.no_mwnd_anime_flag = gameexe_bool_or(ctx, "CONFIG.NO_MWND_ANIME.ONOFF", false);
    cfg.wheel_next_message_flag = gameexe_bool_or(ctx, "CONFIG.WHEEL_NEXT_MESSAGE.ONOFF", true);
    cfg.koe_dont_stop_flag = gameexe_bool_or(ctx, "CONFIG.KOE_DONT_STOP.ONOFF", false);
    cfg.skip_unread_message_flag = gameexe_bool_or(ctx, "CONFIG.SKIP_UNREAD_MESSAGE.ONOFF", false);
    cfg
}

fn config_mouse_cursor_hide_onoff_default(ctx: &CommandContext) -> i64 {
    parse_first_i64_local(&gameexe_value_owned(ctx, "CONFIG.MOUSE_CURSOR_HIDE_ONOFF"))
        .unwrap_or(0)
        .clamp(0, 1)
}

fn config_mouse_cursor_hide_time_default(ctx: &CommandContext) -> i64 {
    parse_first_i64_local(&gameexe_value_owned(ctx, "CONFIG.MOUSE_CURSOR_HIDE_TIME"))
        .unwrap_or(5000)
        .max(0)
}

fn config_filter_color_default(ctx: &CommandContext) -> (i64, i64, i64, i64) {
    let raw = gameexe_value_owned(ctx, "CONFIG.FILTER_COLOR");
    let vals = parse_i64_list_local(&raw);
    if vals.len() >= 4 {
        (
            vals[0].clamp(0, 255),
            vals[1].clamp(0, 255),
            vals[2].clamp(0, 255),
            vals[3].clamp(0, 255),
        )
    } else {
        (0, 0, 0, 128)
    }
}


fn local_extra_index(params: &[Value]) -> usize {
    p_i64(params, 0).clamp(0, 3) as usize
}

fn local_extra_value_param(params: &[Value]) -> bool {
    let value_idx = if params.len() >= 2 { 1 } else { 0 };
    p_bool(params, value_idx)
}

fn local_extra_i64_param(params: &[Value]) -> i64 {
    let value_idx = if params.len() >= 2 { 1 } else { 0 };
    p_i64(params, value_idx)
}

fn get_local_extra(op: i32, params: &[Value], st: &crate::runtime::globals::SyscomRuntimeState) -> Option<i64> {
    let idx = local_extra_index(params);
    let sw = st.local_extra_switches.get(idx).copied().unwrap_or(st.local_extra_switch);
    let mode = st.local_extra_modes.get(idx).copied().unwrap_or(st.local_extra_mode);
    Some(match op {
        GET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG => if sw.onoff { 1 } else { 0 },
        GET_LOCAL_EXTRA_SWITCH_ENABLE_FLAG => if sw.enable { 1 } else { 0 },
        GET_LOCAL_EXTRA_SWITCH_EXIST_FLAG => if sw.exist { 1 } else { 0 },
        CHECK_LOCAL_EXTRA_SWITCH_ENABLE => sw.check_enabled(),
        GET_LOCAL_EXTRA_MODE_VALUE => mode.value,
        GET_LOCAL_EXTRA_MODE_ENABLE_FLAG => if mode.enable { 1 } else { 0 },
        GET_LOCAL_EXTRA_MODE_EXIST_FLAG => if mode.exist { 1 } else { 0 },
        CHECK_LOCAL_EXTRA_MODE_ENABLE => mode.check_enabled(),
        _ => return None,
    })
}

fn set_local_extra(op: i32, params: &[Value], st: &mut crate::runtime::globals::SyscomRuntimeState) -> bool {
    let idx = local_extra_index(params);
    let value = local_extra_value_param(params);
    match op {
        SET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG => st.local_extra_switches[idx].onoff = value,
        SET_LOCAL_EXTRA_SWITCH_ENABLE_FLAG => st.local_extra_switches[idx].enable = value,
        SET_LOCAL_EXTRA_SWITCH_EXIST_FLAG => st.local_extra_switches[idx].exist = value,
        SET_LOCAL_EXTRA_MODE_ENABLE_FLAG => st.local_extra_modes[idx].enable = value,
        SET_LOCAL_EXTRA_MODE_EXIST_FLAG => st.local_extra_modes[idx].exist = value,
        SET_LOCAL_EXTRA_MODE_VALUE => st.local_extra_modes[idx].value = local_extra_i64_param(params),
        _ => return false,
    }
    st.local_extra_switch = st.local_extra_switches[0];
    st.local_extra_mode = st.local_extra_modes[0];
    true
}

fn get_toggle_get(op: i32, st: &crate::runtime::globals::SyscomRuntimeState) -> Option<i64> {
    Some(match op {
        GET_READ_SKIP_ONOFF_FLAG => {
            if st.read_skip.onoff {
                1
            } else {
                0
            }
        }
        GET_READ_SKIP_ENABLE_FLAG => {
            if st.read_skip.enable {
                1
            } else {
                0
            }
        }
        GET_READ_SKIP_EXIST_FLAG => {
            if st.read_skip.exist {
                1
            } else {
                0
            }
        }
        CHECK_READ_SKIP_ENABLE => st.read_skip.check_enabled(),
        GET_AUTO_SKIP_ONOFF_FLAG => {
            if st.auto_skip.onoff {
                1
            } else {
                0
            }
        }
        GET_AUTO_SKIP_ENABLE_FLAG => {
            if st.auto_skip.enable {
                1
            } else {
                0
            }
        }
        GET_AUTO_SKIP_EXIST_FLAG => {
            if st.auto_skip.exist {
                1
            } else {
                0
            }
        }
        CHECK_AUTO_SKIP_ENABLE => st.auto_skip.check_enabled(),
        GET_AUTO_MODE_ONOFF_FLAG => {
            if st.auto_mode.onoff {
                1
            } else {
                0
            }
        }
        GET_AUTO_MODE_ENABLE_FLAG => {
            if st.auto_mode.enable {
                1
            } else {
                0
            }
        }
        GET_AUTO_MODE_EXIST_FLAG => {
            if st.auto_mode.exist {
                1
            } else {
                0
            }
        }
        CHECK_AUTO_MODE_ENABLE => st.auto_mode.check_enabled(),
        GET_HIDE_MWND_ONOFF_FLAG => {
            if st.hide_mwnd.onoff {
                1
            } else {
                0
            }
        }
        GET_HIDE_MWND_ENABLE_FLAG => {
            if st.hide_mwnd.enable {
                1
            } else {
                0
            }
        }
        GET_HIDE_MWND_EXIST_FLAG => {
            if st.hide_mwnd.exist {
                1
            } else {
                0
            }
        }
        CHECK_HIDE_MWND_ENABLE => st.hide_mwnd.check_enabled(),
        GET_MSG_BACK_ENABLE_FLAG => {
            if st.msg_back.enable {
                1
            } else {
                0
            }
        }
        GET_MSG_BACK_EXIST_FLAG => {
            if st.msg_back.exist {
                1
            } else {
                0
            }
        }
        CHECK_MSG_BACK_ENABLE => st.msg_back.check_enabled(),
        CHECK_MSG_BACK_OPEN => {
            if st.msg_back_open {
                1
            } else {
                0
            }
        }
        GET_RETURN_TO_SEL_ENABLE_FLAG => {
            if st.return_to_sel.enable {
                1
            } else {
                0
            }
        }
        GET_RETURN_TO_SEL_EXIST_FLAG => {
            if st.return_to_sel.exist {
                1
            } else {
                0
            }
        }
        CHECK_RETURN_TO_SEL_ENABLE => st.return_to_sel.check_enabled(),
        GET_RETURN_TO_MENU_ENABLE_FLAG => {
            if st.return_to_menu.enable {
                1
            } else {
                0
            }
        }
        GET_RETURN_TO_MENU_EXIST_FLAG => {
            if st.return_to_menu.exist {
                1
            } else {
                0
            }
        }
        CHECK_RETURN_TO_MENU_ENABLE => st.return_to_menu.check_enabled(),
        GET_END_GAME_ENABLE_FLAG => {
            if st.end_game.enable {
                1
            } else {
                0
            }
        }
        GET_END_GAME_EXIST_FLAG => {
            if st.end_game.exist {
                1
            } else {
                0
            }
        }
        CHECK_END_GAME_ENABLE => st.end_game.check_enabled(),
        GET_SAVE_ENABLE_FLAG => {
            if st.save_feature.enable {
                1
            } else {
                0
            }
        }
        GET_SAVE_EXIST_FLAG => {
            if st.save_feature.exist {
                1
            } else {
                0
            }
        }
        CHECK_SAVE_ENABLE => st.save_feature.check_enabled(),
        GET_LOAD_ENABLE_FLAG => {
            if st.load_feature.enable {
                1
            } else {
                0
            }
        }
        GET_LOAD_EXIST_FLAG => {
            if st.load_feature.exist {
                1
            } else {
                0
            }
        }
        CHECK_LOAD_ENABLE => st.load_feature.check_enabled(),
        _ => return None,
    })
}

fn apply_toggle_set(
    op: i32,
    v: bool,
    st: &mut crate::runtime::globals::SyscomRuntimeState,
) -> bool {
    match op {
        SET_READ_SKIP_ONOFF_FLAG => st.read_skip.onoff = v,
        SET_READ_SKIP_ENABLE_FLAG => st.read_skip.enable = v,
        SET_READ_SKIP_EXIST_FLAG => st.read_skip.exist = v,
        SET_AUTO_SKIP_ONOFF_FLAG => st.auto_skip.onoff = v,
        SET_AUTO_SKIP_ENABLE_FLAG => st.auto_skip.enable = v,
        SET_AUTO_SKIP_EXIST_FLAG => st.auto_skip.exist = v,
        SET_AUTO_MODE_ONOFF_FLAG => st.auto_mode.onoff = v,
        SET_AUTO_MODE_ENABLE_FLAG => st.auto_mode.enable = v,
        SET_AUTO_MODE_EXIST_FLAG => st.auto_mode.exist = v,
        SET_HIDE_MWND_ONOFF_FLAG => st.hide_mwnd.onoff = v,
        SET_HIDE_MWND_ENABLE_FLAG => st.hide_mwnd.enable = v,
        SET_HIDE_MWND_EXIST_FLAG => st.hide_mwnd.exist = v,
        SET_MSG_BACK_ENABLE_FLAG => st.msg_back.enable = v,
        SET_MSG_BACK_EXIST_FLAG => st.msg_back.exist = v,
        SET_RETURN_TO_SEL_ENABLE_FLAG => st.return_to_sel.enable = v,
        SET_RETURN_TO_SEL_EXIST_FLAG => st.return_to_sel.exist = v,
        SET_RETURN_TO_MENU_ENABLE_FLAG => st.return_to_menu.enable = v,
        SET_RETURN_TO_MENU_EXIST_FLAG => st.return_to_menu.exist = v,
        SET_END_GAME_ENABLE_FLAG => st.end_game.enable = v,
        SET_END_GAME_EXIST_FLAG => st.end_game.exist = v,
        SET_SAVE_ENABLE_FLAG => st.save_feature.enable = v,
        SET_SAVE_EXIST_FLAG => st.save_feature.exist = v,
        SET_LOAD_ENABLE_FLAG => st.load_feature.enable = v,
        SET_LOAD_EXIST_FLAG => st.load_feature.exist = v,
        _ => return false,
    }
    true
}


fn truncate_to_chars(s: &mut String, max_chars: usize) {
    if s.chars().count() > max_chars {
        *s = s.chars().take(max_chars).collect();
    }
}

/// Equivalent to C++ `C_tnm_eng::save_local_msg`. Appends to the live
/// `current_save_message` / `current_save_full_message` and, when a savepoint
/// snapshot exists, mirrors the append into `m_local_save.save_msg`/`save_full_msg`
/// without rebuilding the saved stream. Length caps mirror `TNM_SAVE_MESSAGE_MAX_LEN`
/// / `TNM_SAVE_FULL_MESSAGE_MAX_LEN` (256 TCHARs).
pub(crate) fn append_current_save_message(ctx: &mut CommandContext, msg: &str) {
    if msg.is_empty() {
        return;
    }
    ctx.globals.syscom.current_save_message.push_str(msg);
    truncate_to_chars(&mut ctx.globals.syscom.current_save_message, 256);
    ctx.globals.syscom.current_save_full_message.push_str(msg);
    truncate_to_chars(&mut ctx.globals.syscom.current_save_full_message, 256);

    if let Some(snapshot) = ctx.local_save_snapshot.as_mut() {
        snapshot.save_msg.push_str(msg);
        truncate_to_chars(&mut snapshot.save_msg, 256);
        snapshot.save_full_msg.push_str(msg);
        truncate_to_chars(&mut snapshot.save_full_msg, 256);
    }
}

fn save_load_trace_enabled() -> bool {
    std::env::var_os("SG_SAVELOAD_TRACE").is_some()
}

fn trace_save_load_event(ctx: &CommandContext, label: &str, quick: bool, idx: usize, path: Option<&Path>) {
    if !save_load_trace_enabled() {
        return;
    }
    let kind = if quick { "quick" } else { "normal" };
    if let Some(path) = path {
        eprintln!(
            "[SG_SAVELOAD_TRACE][SYSCOM] {label} kind={kind} idx={idx} path={} exists={}",
            path.display(),
            crate::resource::game_path_exists(path)
        );
    } else {
        eprintln!("[SG_SAVELOAD_TRACE][SYSCOM] {label} kind={kind} idx={idx}");
    }
}

fn ensure_slot(slots: &mut Vec<SaveSlotState>, idx: usize) -> &mut SaveSlotState {
    if slots.len() <= idx {
        slots.resize_with(idx + 1, SaveSlotState::default);
    }
    &mut slots[idx]
}

pub fn menu_save_slot(ctx: &mut CommandContext, quick: bool, idx: usize) {
    let path = slot_path_with_counts(
        &ctx.project_dir,
        quick,
        idx,
        configured_save_count(ctx, false),
        configured_save_count(ctx, true),
    );
    trace_save_load_event(ctx, "menu_save_slot", quick, idx, Some(&path));
    // Do NOT pre-populate the slot from current runtime state here. The VM-level
    // perform_runtime_save_request consumes the local_save_snapshot built at
    // SAVEPOINT time to write both the header and the slot fields. Pre-writing
    // slot data from the currently-open save/load menu (its title/append/etc.)
    // is exactly the original bug.
    let kind = if quick { RuntimeSaveKind::Quick } else { RuntimeSaveKind::Normal };
    ctx.request_runtime_save(kind, idx);
}

pub fn menu_load_slot(ctx: &mut CommandContext, quick: bool, idx: usize) {
    let path = slot_path_with_counts(
        &ctx.project_dir,
        quick,
        idx,
        configured_save_count(ctx, false),
        configured_save_count(ctx, true),
    );
    trace_save_load_event(ctx, "menu_load_slot", quick, idx, Some(&path));
    let save_cnt = configured_save_count(ctx, false);
    let quick_cnt = configured_save_count(ctx, true);
    if quick {
        ensure_slot_loaded_with_counts(
            &ctx.project_dir,
            true,
            save_cnt,
            quick_cnt,
            &mut ctx.globals.syscom.quick_save_slots,
            idx,
        );
        ctx.request_runtime_load(RuntimeSaveKind::Quick, idx);
    } else {
        ensure_slot_loaded_with_counts(
            &ctx.project_dir,
            false,
            save_cnt,
            quick_cnt,
            &mut ctx.globals.syscom.save_slots,
            idx,
        );
        ctx.request_runtime_load(RuntimeSaveKind::Normal, idx);
    }
}

fn saveload_alert_on(ctx: &CommandContext) -> bool {
    cfg_get_int(&ctx.globals.syscom, GET_SAVELOAD_ALERT_ONOFF, 1) != 0
}

fn slot_exists_for_menu_action(ctx: &mut CommandContext, quick: bool, idx: usize) -> bool {
    let save_cnt = configured_save_count(ctx, false);
    let quick_cnt = configured_save_count(ctx, true);
    if quick {
        ensure_slot_loaded_with_counts(
            &ctx.project_dir,
            true,
            save_cnt,
            quick_cnt,
            &mut ctx.globals.syscom.quick_save_slots,
            idx,
        );
        ctx.globals
            .syscom
            .quick_save_slots
            .get(idx)
            .map(|slot| slot.exist)
            .unwrap_or(false)
    } else {
        ensure_slot_loaded_with_counts(
            &ctx.project_dir,
            false,
            save_cnt,
            quick_cnt,
            &mut ctx.globals.syscom.save_slots,
            idx,
        );
        ctx.globals
            .syscom
            .save_slots
            .get(idx)
            .map(|slot| slot.exist)
            .unwrap_or(false)
    }
}

fn request_confirmed_save_or_load(
    ctx: &mut CommandContext,
    kind: SyscomPendingProcKind,
    idx: usize,
    warning: bool,
    needs_existing_slot: bool,
) -> bool {
    if warning && saveload_alert_on(ctx) && (!needs_existing_slot || slot_exists_for_menu_action(ctx, matches!(kind, SyscomPendingProcKind::QuickSave | SyscomPendingProcKind::QuickLoad), idx)) {
        ctx.globals.syscom.pending_proc = Some(SyscomPendingProc {
            kind,
            warning: true,
            se_play: false,
            fade_out: false,
            leave_msgbk: false,
            save_id: idx as i64,
        });
        ctx.globals.syscom.menu_open = false;
        ctx.globals.syscom.menu_kind = None;
        ctx.globals.syscom.menu_result = None;
        true
    } else {
        false
    }
}

fn local_save_available(ctx: &CommandContext) -> bool {
    ctx.local_save_snapshot
        .as_ref()
        .map(|s| !s.local_stream.is_empty())
        .unwrap_or(false)
        || ctx.pending_auto_savepoint
}

fn local_save_file_exists(ctx: &CommandContext, kind: SaveKind, idx: usize) -> bool {
    let path = original_save::save_file_path_with_counts(
        &ctx.project_dir,
        configured_save_count(ctx, false),
        configured_save_count(ctx, true),
        kind,
        idx,
    );
    original_save::read_slot_from_path(&path)
        .map(|slot| slot.exist)
        .unwrap_or(false)
}

pub(crate) fn save_dir(project_dir: &Path) -> PathBuf {
    original_save::save_dir(project_dir)
}

fn slot_path_with_counts(project_dir: &Path, quick: bool, idx: usize, save_cnt: usize, quick_cnt: usize) -> PathBuf {
    let kind = if quick { SaveKind::Quick } else { SaveKind::Normal };
    original_save::save_file_path_with_counts(project_dir, save_cnt, quick_cnt, kind, idx)
}


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SaveThumbType {
    Bmp,
    Png,
}

#[derive(Clone, Copy, Debug)]
struct SaveThumbConfig {
    enabled: bool,
    thumb_type: SaveThumbType,
    width: u32,
    height: u32,
}

fn save_thumb_config(ctx: &CommandContext) -> SaveThumbConfig {
    let mut out = SaveThumbConfig {
        enabled: false,
        thumb_type: SaveThumbType::Bmp,
        width: 200,
        height: 150,
    };

    let Some(cfg) = ctx.tables.gameexe.as_ref() else {
        return out;
    };

    out.enabled = cfg
        .get_usize("#SAVE_THUMB.USE")
        .or_else(|| cfg.get_usize("SAVE_THUMB.USE"))
        .unwrap_or(0)
        != 0;
    out.thumb_type = match cfg
        .get_usize("#SAVE_THUMB.TYPE")
        .or_else(|| cfg.get_usize("SAVE_THUMB.TYPE"))
        .unwrap_or(0)
    {
        1 => SaveThumbType::Png,
        _ => SaveThumbType::Bmp,
    };

    if let Some(entry) = cfg.get_entry("#SAVE_THUMB.SIZE").or_else(|| cfg.get_entry("SAVE_THUMB.SIZE")) {
        let w = entry.item_unquoted(0).and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(out.width);
        let h = entry.item_unquoted(1).and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(out.height);
        if w != 0 && h != 0 {
            out.width = w;
            out.height = h;
        }
    }

    out
}

pub(crate) fn thumb_candidate_paths(dir: &Path, idx: i64) -> [PathBuf; 2] {
    let project_dir = dir.parent().unwrap_or(dir);
    original_save::thumb_candidate_paths_for_no(project_dir, idx.max(0) as usize)
}

fn thumb_path_for_no_with_config(project_dir: &Path, config: SaveThumbConfig, save_no: usize) -> PathBuf {
    let stem = format!("{save_no:04}");
    let ext = match config.thumb_type {
        SaveThumbType::Bmp => "bmp",
        SaveThumbType::Png => "png",
    };
    save_dir(project_dir).join(format!("{stem}.{ext}"))
}

fn pick_thumb_source_name(ctx: &CommandContext) -> Option<String> {
    let table = ctx.tables.thumb_table.as_ref()?;
    let mut form_ids: Vec<u32> = ctx.globals.stage_forms.keys().copied().collect();
    form_ids.sort_unstable();
    for form_id in form_ids {
        let Some(stage) = ctx.globals.stage_forms.get(&form_id) else {
            continue;
        };
        let mut stage_ids: Vec<i64> = stage.object_lists.keys().copied().collect();
        stage_ids.sort_unstable();
        for stage_idx in stage_ids {
            let Some(objs) = stage.object_lists.get(&stage_idx) else {
                continue;
            };
            for obj in objs.iter().rev() {
                if let Some(file) = obj.file_name.as_deref() {
                    if let Some(mapped) = table.get_by_file_stem(file) {
                        return Some(mapped.clone());
                    }
                }
            }
        }
    }
    None
}

fn capture_slot_thumb(ctx: &mut CommandContext, config: SaveThumbConfig) -> anyhow::Result<RgbaImage> {
    if let Some(name) = pick_thumb_source_name(ctx) {
        if let Ok(img_id) = ctx.images.load_g00(&name, 0) {
            if let Some(img) = ctx.images.get(&img_id) {
                return Ok(resize_rgba(img.as_ref(), config.width, config.height));
            }
        }
    }

    let img = ctx.capture_frame_rgba()?;
    Ok(resize_rgba(&img, config.width, config.height))
}

pub(crate) const CAPTURE_PRIOR_NONE: i32 = 0;
pub const CAPTURE_PRIOR_SAVE: i32 = 1;
pub(crate) const CAPTURE_PRIOR_END: i32 = 2;
pub(crate) const CAPTURE_PRIOR_CAPTURE: i32 = 3;

pub(crate) fn prepare_runtime_save_thumb_capture_with_priority(
    ctx: &mut CommandContext,
    priority: i32,
) {
    let config = save_thumb_config(ctx);
    if !config.enabled {
        return;
    }
    if ctx.globals.save_thumb_capture_image.is_some()
        && priority < ctx.globals.save_thumb_capture_prior
    {
        return;
    }

    match capture_slot_thumb(ctx, config) {
        Ok(image) => {
            ctx.globals.save_thumb_capture_image = Some(image);
            ctx.globals.save_thumb_capture_prior = priority;
        }
        Err(err) => {
            log::error!("save thumbnail GPU capture failed: {err:#}");
        }
    }
}

pub(crate) fn prepare_runtime_save_thumb_capture(ctx: &mut CommandContext) {
    prepare_runtime_save_thumb_capture_with_priority(ctx, CAPTURE_PRIOR_SAVE);
}

pub fn free_runtime_save_thumb_capture(ctx: &mut CommandContext, priority: i32) {
    if priority < ctx.globals.save_thumb_capture_prior {
        return;
    }
    ctx.globals.save_thumb_capture_image = None;
    ctx.globals.save_thumb_capture_prior = CAPTURE_PRIOR_NONE;
}

pub(crate) fn prepare_runtime_save_thumb_capture_from_image(
    ctx: &mut CommandContext,
    img: &RgbaImage,
) {
    let config = save_thumb_config(ctx);
    if !config.enabled {
        return;
    }

    // C++ CAPTURE_FROM_FILE replaces the save-thumbnail texture directly and
    // deliberately leaves the current capture-priority owner unchanged.
    ctx.globals.save_thumb_capture_image = Some(img.clone());
}

pub(crate) fn capture_for_local_save(
    ctx: &mut CommandContext,
    img: &RgbaImage,
    width: u32,
    height: u32,
) -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};

    let capture_time = match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(v) => v.as_secs().min(i64::MAX as u64) as i64,
        Err(err) => {
            log::error!("GLOBAL.CAPTURE_FOR_LOCAL_SAVE cannot obtain UNIX time: {err}");
            return 0;
        }
    };
    let resized = resize_rgba(img, width.max(1), height.max(1));
    let path = original_save::save_dir(&ctx.project_dir).join(format!("{capture_time}.png"));
    if let Err(err) = write_rgba_png_opaque(&path, &resized) {
        log::error!(
            "GLOBAL.CAPTURE_FOR_LOCAL_SAVE failed to write {}: {err:#}",
            path.display()
        );
        return 0;
    }
    capture_time
}

fn write_slot_thumb_for_save_no(ctx: &mut CommandContext, save_no: usize) {
    let config = save_thumb_config(ctx);
    if !config.enabled {
        return;
    }
    let Some(img) = ctx.globals.save_thumb_capture_image.clone() else {
        ctx.unknown
            .record_note(&format!("save_thumb.capture.missing:{save_no}"));
        return;
    };
    let path = thumb_path_for_no_with_config(&ctx.project_dir, config, save_no);
    if save_load_trace_enabled() {
        eprintln!(
            "[SG_SAVELOAD_TRACE][SYSCOM] write_slot_thumb save_no={} path={} size={}x{} type={:?}",
            save_no,
            path.display(),
            img.width,
            img.height,
            config.thumb_type
        );
    }
    remove_game_file(&path);
    let result = match config.thumb_type {
        SaveThumbType::Bmp => write_rgba_bmp_top_down(&path, &img),
        SaveThumbType::Png => write_rgba_png_opaque(&path, &img),
    };
    if let Err(err) = result {
        eprintln!("[SG_SAVE] failed to write save thumb {}: {err:#}", path.display());
    }
}

pub(crate) fn write_runtime_slot_thumb(ctx: &mut CommandContext, save_no: usize) {
    write_slot_thumb_for_save_no(ctx, save_no);
}

fn escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn unescape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(ch) = it.next() {
        if ch == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => break,
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn read_slot(path: &Path) -> Option<SaveSlotState> {
    original_save::read_slot_from_path(path)
}

fn configured_chrkoe_count(ctx: &CommandContext) -> usize {
    ctx.tables
        .gameexe
        .as_ref()
        .and_then(|cfg| {
            cfg.get_usize("#CHRKOE.CNT")
                .or_else(|| cfg.get_usize("CHRKOE.CNT"))
        })
        .unwrap_or(64)
        .min(256)
}

fn resize_original_config_arrays(
    _ctx: &CommandContext,
    cfg: &mut crate::runtime::globals::OriginalConfigRuntimeState,
) {
    cfg.object_disp_flag.resize(4, true);
    cfg.object_disp_flag.truncate(4);
    cfg.global_extra_switch_flag.resize(4, true);
    cfg.global_extra_switch_flag.truncate(4);
    cfg.global_extra_mode_flag.resize(4, 0);
    cfg.global_extra_mode_flag.truncate(4);
}

pub fn config_state_for_save(
    ctx: &CommandContext,
) -> crate::runtime::globals::OriginalConfigRuntimeState {
    let mut cfg = ctx.globals.syscom.original_config.clone();
    resize_original_config_arrays(ctx, &mut cfg);

    cfg.screen_size_mode = cfg_get_int(
        &ctx.globals.syscom,
        GET_WINDOW_MODE,
        cfg.screen_size_mode,
    )
    .clamp(0, 1);
    let window_scale = cfg_get_int(
        &ctx.globals.syscom,
        GET_WINDOW_MODE_SIZE,
        cfg.screen_size_scale.0,
    )
    .max(1);
    cfg.screen_size_scale = (window_scale, window_scale);

    cfg.all_sound_user_volume = cfg_get_int(
        &ctx.globals.syscom,
        GET_ALL_VOLUME,
        cfg.all_sound_user_volume,
    )
    .clamp(0, 255);
    for (index, key) in [
        GET_BGM_VOLUME,
        GET_KOE_VOLUME,
        GET_PCM_VOLUME,
        GET_SE_VOLUME,
        GET_MOV_VOLUME,
    ]
    .into_iter()
    .enumerate()
    {
        cfg.sound_user_volume[index] = cfg_get_int(
            &ctx.globals.syscom,
            key,
            cfg.sound_user_volume[index],
        )
        .clamp(0, 255);
    }
    cfg.play_all_sound_check = cfg_get_int(
        &ctx.globals.syscom,
        GET_ALL_ONOFF,
        if cfg.play_all_sound_check { 1 } else { 0 },
    ) != 0;
    for (index, key) in [
        GET_BGM_ONOFF,
        GET_KOE_ONOFF,
        GET_PCM_ONOFF,
        GET_SE_ONOFF,
        GET_MOV_ONOFF,
    ]
    .into_iter()
    .enumerate()
    {
        cfg.play_sound_check[index] = cfg_get_int(
            &ctx.globals.syscom,
            key,
            if cfg.play_sound_check[index] { 1 } else { 0 },
        ) != 0;
    }

    cfg.bgmfade_volume = cfg_get_int(
        &ctx.globals.syscom,
        GET_BGMFADE_VOLUME,
        cfg.bgmfade_volume,
    )
    .clamp(0, 255);
    cfg.bgmfade_use_check = cfg_get_int(
        &ctx.globals.syscom,
        GET_BGMFADE_ONOFF,
        if cfg.bgmfade_use_check { 1 } else { 0 },
    ) != 0;

    let r = cfg_get_int(
        &ctx.globals.syscom,
        GET_FILTER_COLOR_R,
        ((cfg.filter_color_argb >> 16) & 0xff) as i64,
    )
    .clamp(0, 255) as u32;
    let g = cfg_get_int(
        &ctx.globals.syscom,
        GET_FILTER_COLOR_G,
        ((cfg.filter_color_argb >> 8) & 0xff) as i64,
    )
    .clamp(0, 255) as u32;
    let b = cfg_get_int(
        &ctx.globals.syscom,
        GET_FILTER_COLOR_B,
        (cfg.filter_color_argb & 0xff) as i64,
    )
    .clamp(0, 255) as u32;
    let a = cfg_get_int(
        &ctx.globals.syscom,
        GET_FILTER_COLOR_A,
        ((cfg.filter_color_argb >> 24) & 0xff) as i64,
    )
    .clamp(0, 255) as u32;
    cfg.filter_color_argb = (a << 24) | (r << 16) | (g << 8) | b;

    cfg.font_name = cfg_get_str(&ctx.globals.syscom, GET_FONT_NAME);
    cfg.font_futoku = cfg_get_int(
        &ctx.globals.syscom,
        GET_FONT_BOLD,
        if cfg.font_futoku { 1 } else { 0 },
    ) != 0;
    cfg.font_shadow = cfg_get_int(
        &ctx.globals.syscom,
        GET_FONT_DECORATION,
        cfg.font_shadow,
    );

    cfg.message_speed = cfg_get_int(
        &ctx.globals.syscom,
        GET_MESSAGE_SPEED,
        cfg.message_speed,
    );
    cfg.message_speed_nowait = ctx.globals.script.msg_nowait;
    cfg.auto_mode_onoff = ctx.globals.syscom.auto_mode.onoff;
    cfg.auto_mode_moji_wait = ctx.globals.script.auto_mode_moji_wait;
    cfg.auto_mode_min_wait = ctx.globals.script.auto_mode_min_wait;
    cfg.mouse_cursor_hide_onoff = cfg_get_int(
        &ctx.globals.syscom,
        GET_MOUSE_CURSOR_HIDE_ONOFF,
        if cfg.mouse_cursor_hide_onoff { 1 } else { 0 },
    ) != 0;
    cfg.mouse_cursor_hide_time = cfg_get_int(
        &ctx.globals.syscom,
        GET_MOUSE_CURSOR_HIDE_TIME,
        cfg.mouse_cursor_hide_time,
    )
    .max(0);
    cfg.jitan_normal_onoff = cfg_get_int(
        &ctx.globals.syscom,
        GET_JITAN_NORMAL_ONOFF,
        if cfg.jitan_normal_onoff { 1 } else { 0 },
    ) != 0;
    cfg.jitan_auto_mode_onoff = cfg_get_int(
        &ctx.globals.syscom,
        GET_JITAN_AUTO_MODE_ONOFF,
        if cfg.jitan_auto_mode_onoff { 1 } else { 0 },
    ) != 0;
    cfg.jitan_msgbk_onoff = cfg_get_int(
        &ctx.globals.syscom,
        GET_JITAN_KOE_REPLAY_ONOFF,
        if cfg.jitan_msgbk_onoff { 1 } else { 0 },
    ) != 0;
    cfg.jitan_speed = cfg_get_int(&ctx.globals.syscom, GET_JITAN_SPEED, cfg.jitan_speed);
    cfg.koe_mode = cfg_get_int(&ctx.globals.syscom, GET_KOEMODE, cfg.koe_mode);

    cfg.sleep_flag = cfg_get_int(
        &ctx.globals.syscom,
        GET_SLEEP_ONOFF,
        if cfg.sleep_flag { 1 } else { 0 },
    ) != 0;
    cfg.no_wipe_anime_flag = cfg_get_int(
        &ctx.globals.syscom,
        GET_NO_WIPE_ANIME_ONOFF,
        if cfg.no_wipe_anime_flag { 1 } else { 0 },
    ) != 0;
    cfg.skip_wipe_anime_flag = cfg_get_int(
        &ctx.globals.syscom,
        GET_SKIP_WIPE_ANIME_ONOFF,
        if cfg.skip_wipe_anime_flag { 1 } else { 0 },
    ) != 0;
    cfg.no_mwnd_anime_flag = cfg_get_int(
        &ctx.globals.syscom,
        GET_NO_MWND_ANIME_ONOFF,
        if cfg.no_mwnd_anime_flag { 1 } else { 0 },
    ) != 0;
    cfg.wheel_next_message_flag = cfg_get_int(
        &ctx.globals.syscom,
        GET_WHEEL_NEXT_MESSAGE_ONOFF,
        if cfg.wheel_next_message_flag { 1 } else { 0 },
    ) != 0;
    cfg.koe_dont_stop_flag = cfg_get_int(
        &ctx.globals.syscom,
        GET_KOE_DONT_STOP_ONOFF,
        if cfg.koe_dont_stop_flag { 1 } else { 0 },
    ) != 0;
    cfg.skip_unread_message_flag = cfg_get_int(
        &ctx.globals.syscom,
        GET_SKIP_UNREAD_MESSAGE_ONOFF,
        if cfg.skip_unread_message_flag { 1 } else { 0 },
    ) != 0;
    cfg.saveload_alert_flag = cfg_get_int(
        &ctx.globals.syscom,
        GET_SAVELOAD_ALERT_ONOFF,
        if cfg.saveload_alert_flag { 1 } else { 0 },
    ) != 0;

    cfg
}

fn apply_original_config_to_runtime(ctx: &mut CommandContext) {
    let cfg = ctx.globals.syscom.original_config.clone();
    cfg_set_int(&mut ctx.globals.syscom, GET_WINDOW_MODE, cfg.screen_size_mode.clamp(0, 1));
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_WINDOW_MODE_SIZE,
        cfg.screen_size_scale.0.max(1),
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_ALL_VOLUME,
        cfg.all_sound_user_volume.clamp(0, 255),
    );
    for (index, key) in [
        GET_BGM_VOLUME,
        GET_KOE_VOLUME,
        GET_PCM_VOLUME,
        GET_SE_VOLUME,
        GET_MOV_VOLUME,
    ]
    .into_iter()
    .enumerate()
    {
        cfg_set_int(
            &mut ctx.globals.syscom,
            key,
            cfg.sound_user_volume[index].clamp(0, 255),
        );
    }
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_ALL_ONOFF,
        if cfg.play_all_sound_check { 1 } else { 0 },
    );
    for (index, key) in [
        GET_BGM_ONOFF,
        GET_KOE_ONOFF,
        GET_PCM_ONOFF,
        GET_SE_ONOFF,
        GET_MOV_ONOFF,
    ]
    .into_iter()
    .enumerate()
    {
        cfg_set_int(
            &mut ctx.globals.syscom,
            key,
            if cfg.play_sound_check[index] { 1 } else { 0 },
        );
    }
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_BGMFADE_VOLUME,
        cfg.bgmfade_volume.clamp(0, 255),
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_BGMFADE_ONOFF,
        if cfg.bgmfade_use_check { 1 } else { 0 },
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_FILTER_COLOR_A,
        ((cfg.filter_color_argb >> 24) & 0xff) as i64,
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_FILTER_COLOR_R,
        ((cfg.filter_color_argb >> 16) & 0xff) as i64,
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_FILTER_COLOR_G,
        ((cfg.filter_color_argb >> 8) & 0xff) as i64,
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_FILTER_COLOR_B,
        (cfg.filter_color_argb & 0xff) as i64,
    );
    cfg_set_str(&mut ctx.globals.syscom, GET_FONT_NAME, cfg.font_name);
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_FONT_BOLD,
        if cfg.font_futoku { 1 } else { 0 },
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_FONT_DECORATION,
        cfg.font_shadow,
    );
    cfg_set_int(&mut ctx.globals.syscom, GET_MESSAGE_SPEED, cfg.message_speed);
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_MESSAGE_NOWAIT,
        if cfg.message_speed_nowait { 1 } else { 0 },
    );
    ctx.globals.script.msg_nowait = cfg.message_speed_nowait;
    ctx.globals.syscom.auto_mode.onoff = cfg.auto_mode_onoff;
    ctx.globals.script.auto_mode_moji_wait = cfg.auto_mode_moji_wait;
    ctx.globals.script.auto_mode_min_wait = cfg.auto_mode_min_wait;
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_AUTO_MODE_MOJI_WAIT,
        cfg.auto_mode_moji_wait,
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_AUTO_MODE_MIN_WAIT,
        cfg.auto_mode_min_wait,
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_MOUSE_CURSOR_HIDE_ONOFF,
        if cfg.mouse_cursor_hide_onoff { 1 } else { 0 },
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_MOUSE_CURSOR_HIDE_TIME,
        cfg.mouse_cursor_hide_time,
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_JITAN_NORMAL_ONOFF,
        if cfg.jitan_normal_onoff { 1 } else { 0 },
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_JITAN_AUTO_MODE_ONOFF,
        if cfg.jitan_auto_mode_onoff { 1 } else { 0 },
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_JITAN_KOE_REPLAY_ONOFF,
        if cfg.jitan_msgbk_onoff { 1 } else { 0 },
    );
    cfg_set_int(&mut ctx.globals.syscom, GET_JITAN_SPEED, cfg.jitan_speed);
    cfg_set_int(&mut ctx.globals.syscom, GET_KOEMODE, cfg.koe_mode);
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_SLEEP_ONOFF,
        if cfg.sleep_flag { 1 } else { 0 },
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_NO_WIPE_ANIME_ONOFF,
        if cfg.no_wipe_anime_flag { 1 } else { 0 },
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_SKIP_WIPE_ANIME_ONOFF,
        if cfg.skip_wipe_anime_flag { 1 } else { 0 },
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_NO_MWND_ANIME_ONOFF,
        if cfg.no_mwnd_anime_flag { 1 } else { 0 },
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_WHEEL_NEXT_MESSAGE_ONOFF,
        if cfg.wheel_next_message_flag { 1 } else { 0 },
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_KOE_DONT_STOP_ONOFF,
        if cfg.koe_dont_stop_flag { 1 } else { 0 },
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_SKIP_UNREAD_MESSAGE_ONOFF,
        if cfg.skip_unread_message_flag { 1 } else { 0 },
    );
    cfg_set_int(
        &mut ctx.globals.syscom,
        GET_SAVELOAD_ALERT_ONOFF,
        if cfg.saveload_alert_flag { 1 } else { 0 },
    );
    apply_audio_config(ctx);
}

/// Apply dialog edits through the same state and audio routing used by scripts.
pub fn apply_config_dialog_state(
    ctx: &mut CommandContext,
    mut config: crate::runtime::globals::OriginalConfigRuntimeState,
) {
    resize_original_config_arrays(ctx, &mut config);
    ctx.globals.syscom.original_config = config;
    apply_original_config_to_runtime(ctx);
}

pub fn write_config_save(ctx: &CommandContext) {
    let cfg = config_state_for_save(ctx);
    let mut stream = original_save::OriginalStreamWriter::new();
    stream.push_i32(cfg.screen_size_mode as i32);
    stream.push_i32(cfg.screen_size_mode_window as i32);
    stream.push_i32(cfg.screen_size_scale.0 as i32);
    stream.push_i32(cfg.screen_size_scale.1 as i32);
    stream.push_i32(cfg.screen_size_free.0 as i32);
    stream.push_i32(cfg.screen_size_free.1 as i32);
    stream.push_bool(cfg.fullscreen_change_resolution);
    stream.push_i32(cfg.fullscreen_display_cnt as i32);
    stream.push_i32(cfg.fullscreen_display_no as i32);
    stream.push_i32(cfg.fullscreen_resolution_cnt as i32);
    stream.push_i32(cfg.fullscreen_resolution_no as i32);
    stream.push_i32(cfg.fullscreen_resolution.0 as i32);
    stream.push_i32(cfg.fullscreen_resolution.1 as i32);
    stream.push_i32(cfg.fullscreen_mode as i32);
    stream.push_i32(cfg.fullscreen_scale.0 as i32);
    stream.push_i32(cfg.fullscreen_scale.1 as i32);
    stream.push_bool(cfg.fullscreen_scale_sync_switch);
    stream.push_i32(cfg.fullscreen_move.0 as i32);
    stream.push_i32(cfg.fullscreen_move.1 as i32);
    stream.push_i32(cfg.all_sound_user_volume.clamp(0, 255) as i32);
    for value in cfg.sound_user_volume {
        stream.push_i32(value.clamp(0, 255) as i32);
    }
    stream.push_bool(cfg.play_all_sound_check);
    for value in cfg.play_sound_check {
        stream.push_bool(value);
    }
    stream.push_i32(cfg.bgmfade_volume.clamp(0, 255) as i32);
    stream.push_bool(cfg.bgmfade_use_check);
    stream.push_u32(cfg.filter_color_argb);
    stream.push_bool(cfg.font_proportional);
    stream.push_str(&cfg.font_name);
    stream.push_i32(cfg.font_shadow as i32);
    stream.push_bool(cfg.font_futoku);
    stream.push_i32(cfg.message_speed as i32);
    stream.push_bool(cfg.message_speed_nowait);
    stream.push_bool(cfg.auto_mode_onoff);
    stream.push_i32(cfg.auto_mode_moji_wait as i32);
    stream.push_i32(cfg.auto_mode_min_wait as i32);
    stream.push_bool(cfg.mouse_cursor_hide_onoff);
    stream.push_i32(cfg.mouse_cursor_hide_time as i32);
    stream.push_bool(cfg.jitan_normal_onoff);
    stream.push_bool(cfg.jitan_auto_mode_onoff);
    stream.push_bool(cfg.jitan_msgbk_onoff);
    stream.push_i32(cfg.jitan_speed as i32);
    stream.push_i32(cfg.koe_mode as i32);
    stream.push_i32(cfg.chrkoe.len() as i32);
    for item in &cfg.chrkoe {
        stream.push_bool(item.onoff);
        stream.push_padding(3);
        stream.push_i32(item.volume.clamp(0, 255) as i32);
    }
    stream.push_bool(cfg.message_chrcolor_flag);
    stream.push_i32(cfg.object_disp_flag.len() as i32);
    for value in &cfg.object_disp_flag {
        stream.push_bool(*value);
    }
    stream.push_i32(cfg.global_extra_switch_flag.len() as i32);
    for value in &cfg.global_extra_switch_flag {
        stream.push_bool(*value);
    }
    stream.push_i32(cfg.global_extra_mode_flag.len() as i32);
    for value in &cfg.global_extra_mode_flag {
        stream.push_i32(*value as i32);
    }
    for value in [
        cfg.sleep_flag,
        cfg.no_wipe_anime_flag,
        cfg.skip_wipe_anime_flag,
        cfg.no_mwnd_anime_flag,
        cfg.wheel_next_message_flag,
        cfg.koe_dont_stop_flag,
        cfg.skip_unread_message_flag,
        cfg.saveload_alert_flag,
        cfg.saveload_dblclick_flag,
    ] {
        stream.push_bool(value);
    }
    stream.push_str(&cfg.ss_path);
    stream.push_str(&cfg.editor_path);
    stream.push_str(&cfg.koe_path);
    stream.push_str(&cfg.koe_tool_path);

    if let Err(err) = original_save::write_config_save_file(&ctx.project_dir, &stream.into_inner()) {
        log::error!("[SG_SAVE] failed to write config.sav: {err:#}");
    }
}

fn load_config_save(ctx: &mut CommandContext) -> Result<()> {
    let mut cfg = original_config_defaults(ctx);
    let config_path = original_save::save_dir(&ctx.project_dir).join("config.sav");
    if !crate::resource::game_file_exists(&config_path) {
        resize_original_config_arrays(ctx, &mut cfg);
        ctx.globals.syscom.original_config = cfg;
        apply_original_config_to_runtime(ctx);
        return Ok(());
    }
    let (header, payload) = original_save::read_config_save_file(&ctx.project_dir)?;
    save_versions::read_config(header, &payload, &mut cfg, &gameexe_unquoted_owned(ctx, "GAMEID"))?;
    resize_original_config_arrays(ctx, &mut cfg);
    ctx.globals.syscom.original_config = cfg;
    apply_original_config_to_runtime(ctx);
    Ok(())
}


fn write_read_flags(ctx: &CommandContext) -> Result<()> {
    let metadata = ctx.scene_metadata()?;
    let mut rows = Vec::with_capacity(metadata.rows.len());
    for (scene_no, (scene_name, flag_count)) in metadata.rows.iter().enumerate() {
        let mut flags = ctx
            .globals
            .read_flags
            .get(&(scene_no as i64))
            .cloned()
            .unwrap_or_default();
        flags.resize(*flag_count, 0);
        flags.truncate(*flag_count);
        rows.push((scene_name.clone(), flags));
    }
    original_save::write_read_save_file(&ctx.project_dir, &rows)
}

fn load_read_flags(ctx: &mut CommandContext) -> Result<()> {
    let read_path = original_save::save_dir(&ctx.project_dir).join("read.sav");
    if crate::resource::resolve_windows_case_insensitive_file(&read_path)?.is_none() {
        return Ok(());
    }
    let metadata = ctx.scene_metadata()?;
    ctx.globals.read_flags.clear();
    for (scene_no, (_, flag_count)) in metadata.rows.iter().enumerate() {
        ctx.globals
            .ensure_read_flag_count(scene_no as i64, *flag_count);
    }

    let rows = match original_save::read_read_save_file(&ctx.project_dir) {
        Ok(rows) => rows,
        Err(_) => return Ok(()),
    };
    for (scene_name, saved_flags) in rows {
        let Some(scene_no) = metadata.find_scene_no(&scene_name) else {
            continue;
        };
        let Some((_, real_flag_count)) = metadata.rows.get(scene_no) else {
            continue;
        };
        // Original C++ applies a row only when its saved count exactly matches
        // Gp_lexer->get_read_flag_cnt(scene_no).
        if saved_flags.len() == *real_flag_count {
            ctx.globals
                .read_flags
                .insert(scene_no as i64, saved_flags);
        }
    }
    Ok(())
}

pub fn write_global_save(ctx: &CommandContext) {
    write_config_save(ctx);
    let mut stream = original_save::OriginalStreamWriter::new();
    stream.push_i64(ctx.globals.syscom.total_play_time);

    let configured_flag_cnt = ctx
        .tables
        .gameexe
        .as_ref()
        .and_then(|cfg| cfg.get_usize("#GLOBAL_FLAG.CNT").or_else(|| cfg.get_usize("GLOBAL_FLAG.CNT")));
    let current_flag_cnt = [
        ctx.globals
            .int_lists
            .get(&(crate::runtime::forms::codes::ELM_GLOBAL_G as u32))
            .map_or(0, Vec::len),
        ctx.globals
            .int_lists
            .get(&(crate::runtime::forms::codes::ELM_GLOBAL_Z as u32))
            .map_or(0, Vec::len),
        ctx.globals
            .str_lists
            .get(&(crate::runtime::forms::codes::ELM_GLOBAL_M as u32))
            .map_or(0, Vec::len),
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    let fixed_flag_cnt = configured_flag_cnt
        .unwrap_or(current_flag_cnt.max(1000))
        .min(10000);
    let cg_flag_cnt = ctx
        .tables
        .cgtable_flag_cnt
        .or_else(|| {
            ctx.tables
                .gameexe
                .as_ref()
                .and_then(|cfg| cfg.get_usize("#CGTABLE_FLAG_CNT").or_else(|| cfg.get_usize("CGTABLE_FLAG_CNT")))
        })
        .unwrap_or(ctx.tables.cg_flags.len())
        .max(ctx.tables.cg_flags.len());
    let bgm_cnt = ctx
        .tables
        .gameexe
        .as_ref()
        .and_then(|cfg| cfg.get_usize("#BGM.CNT").or_else(|| cfg.get_usize("BGM.CNT")))
        .unwrap_or(32)
        .max(ctx.globals.bgm_table_flags.len());
    let g = ctx
        .globals
        .int_lists
        .get(&(crate::runtime::forms::codes::ELM_GLOBAL_G as u32))
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let z = ctx
        .globals
        .int_lists
        .get(&(crate::runtime::forms::codes::ELM_GLOBAL_Z as u32))
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let m = ctx
        .globals
        .str_lists
        .get(&(crate::runtime::forms::codes::ELM_GLOBAL_M as u32))
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let namae_global = ctx
        .globals
        .str_lists
        .get(&(crate::runtime::forms::codes::ELM_GLOBAL_NAMAE_GLOBAL as u32))
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    stream.push_fixed_i32_list(g, fixed_flag_cnt);
    stream.push_fixed_i32_list(z, fixed_flag_cnt);
    stream.push_fixed_str_list(m, fixed_flag_cnt);
    stream.push_fixed_str_list(namae_global, 26 + 26 * 26);
    stream.push_i32(0);

    let cg_flags: Vec<i64> = ctx.tables.cg_flags.iter().map(|v| *v as i64).collect();
    // Without a CGTABLE_FILE, native C_tnm_cg_table::init leaves flag in
    // its default extendable state. Its empty save contains only a count.
    let has_cg_table = ctx.tables.cgtable.is_some()
        || ctx.tables.gameexe.as_ref()
            .and_then(|cfg| cfg.get_unquoted("CGTABLE_FILE"))
            .is_some_and(|name| !name.is_empty());
    if !has_cg_table && cg_flags.is_empty() {
        stream.push_extend_i32_list(&[]);
    } else {
        stream.push_fixed_i32_list(&cg_flags, cg_flag_cnt);
    }

    let bgm_flags: Vec<i64> = ctx
        .globals
        .bgm_table_flags
        .iter()
        .map(|v| if *v { 1 } else { 0 })
        .collect();
    stream.push_fixed_i32_list(&bgm_flags, bgm_cnt);

    // C++ twitter_save_state persists OAuth/user state out-of-band in the
    // Windows registry and writes nothing to the global stream.  The desktop
    // port mirrors that with a sidecar next to the original save files.
    #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
    if let Err(err) = crate::runtime::twitter::save_state(ctx) {
        eprintln!("[SG_SAVE] failed to write Twitter state: {err:#}");
    }

    let chrkoe_count = configured_chrkoe_count(ctx);
    stream.push_i32(chrkoe_count as i32);
    for index in 0..chrkoe_count {
        let name = ctx.tables.gameexe.as_ref()
            .and_then(|cfg| cfg.get_indexed_item_unquoted("CHRKOE", index, 0)).unwrap_or("");
        stream.push_str(name);
        stream.push_bool(config_chrkoe_name_visible(ctx, index));
    }

    let payload = stream.into_inner();
    if let Err(err) = original_save::write_global_save_file(&ctx.project_dir, &payload) {
        eprintln!("[SG_SAVE] failed to write global.sav: {err:#}");
    }
    if let Err(err) = write_read_flags(ctx) {
        eprintln!("[SG_SAVE] failed to write read.sav: {err:#}");
    }
}

pub fn load_global_save(ctx: &mut CommandContext) -> Result<()> {
    let global_path = original_save::save_dir(&ctx.project_dir).join("global.sav");
    if crate::resource::resolve_windows_case_insensitive_file(&global_path)?.is_some() {
        let (header, payload) = original_save::read_global_save_file(&ctx.project_dir)?;
        let fixed_flag_cnt = ctx
            .tables
            .gameexe
            .as_ref()
            .and_then(|cfg| {
                cfg.get_usize("#GLOBAL_FLAG.CNT")
                    .or_else(|| cfg.get_usize("GLOBAL_FLAG.CNT"))
            })
            .map(|count| count.min(10000));
        let save_versions::GlobalSaveData {
            total_play_time, mut g, mut z, mut m, mut namae_global, cg, bgm, chrkoe_look_flags,
        } = save_versions::read_global(header, &payload)?;
        if let Some(count) = fixed_flag_cnt {
            g.resize(count, 0);
            z.resize(count, 0);
            m.resize_with(count, String::new);
        }
        namae_global.resize_with(26 + 26 * 26, String::new);
        ctx.globals.syscom.chrkoe_look_flags = chrkoe_look_flags;

        ctx.globals.syscom.total_play_time = total_play_time;
        ctx.globals
            .int_lists
            .insert(crate::runtime::forms::codes::ELM_GLOBAL_G as u32, g);
        ctx.globals
            .int_lists
            .insert(crate::runtime::forms::codes::ELM_GLOBAL_Z as u32, z);
        ctx.globals
            .str_lists
            .insert(crate::runtime::forms::codes::ELM_GLOBAL_M as u32, m);
        ctx.globals.str_lists.insert(
            crate::runtime::forms::codes::ELM_GLOBAL_NAMAE_GLOBAL as u32,
            namae_global,
        );
        // C_tnm_cg_table::load() restores the fixed C_elm_int_list<int> verbatim.
        ctx.tables.cg_flags = cg.into_iter().map(|v| v as i32).collect();
        ctx.globals.bgm_table_flags = bgm.into_iter().map(|v| v != 0).collect();
        #[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
        crate::runtime::twitter::ensure_state_loaded(ctx);
    }
    load_read_flags(ctx)?;
    load_config_save(ctx)?;
    Ok(())
}


fn ensure_slot_loaded_with_counts(
    project_dir: &Path,
    quick: bool,
    save_cnt: usize,
    quick_cnt: usize,
    slots: &mut Vec<SaveSlotState>,
    idx: usize,
) {
    // C_tnm_save_cache::load_cache() has a positive cache and a negative
    // data_none_flag cache. Once a slot has been queried, metadata commands
    // must not reopen the save file until an operation explicitly changes it.
    if slots
        .get(idx)
        .map(|slot| slot.header_cache_valid)
        .unwrap_or(false)
    {
        return;
    }

    let path = slot_path_with_counts(project_dir, quick, idx, save_cnt, quick_cnt);
    if save_load_trace_enabled() {
        let before_exist = slots.get(idx).map(|s| s.exist).unwrap_or(false);
        eprintln!(
            "[SG_SAVELOAD_TRACE][SYSCOM] ensure_slot_loaded kind={} idx={} path={} file_exists={} cached_exist={}",
            if quick { "quick" } else { "normal" },
            idx,
            path.display(),
            crate::resource::game_file_exists(&path),
            before_exist
        );
    }

    let next = match read_slot(&path) {
        Some(slot) => slot,
        None => {
            // Mirrors C_tnm_save_cache::data_none_flag: a missing/invalid save
            // is cached as absent instead of being probed again by every
            // GET_SAVE_* command in LOAD_SCENE.
            let mut slot = SaveSlotState::default();
            slot.header_cache_valid = true;
            slot
        }
    };
    *ensure_slot(slots, idx) = next;
}

pub(crate) fn read_save_flag_values(
    ctx: &mut CommandContext,
    quick: bool,
    raw_idx: i64,
    flag_cnt: usize,
) -> Option<Vec<i64>> {
    let project_dir = ctx.project_dir.clone();
    let save_cnt = configured_save_count(ctx, false);
    let quick_cnt = configured_save_count(ctx, true);
    let configured = if quick { quick_cnt } else { save_cnt };
    if raw_idx < 0 || raw_idx as usize >= configured {
        return None;
    }
    let idx = raw_idx as usize;
    let slots = if quick {
        &mut ctx.globals.syscom.quick_save_slots
    } else {
        &mut ctx.globals.syscom.save_slots
    };
    ensure_slot_loaded_with_counts(
        &project_dir,
        quick,
        save_cnt,
        quick_cnt,
        slots,
        idx,
    );
    let slot = slots.get(idx).filter(|slot| slot.exist)?;
    let count = flag_cnt.min(original_save::SAVE_FLAG_MAX_CNT);
    Some(
        (0..count)
            .map(|i| slot.values.get(&(i as i32)).copied().unwrap_or(0))
            .collect(),
    )
}

pub(crate) fn write_save_flag_values(
    ctx: &mut CommandContext,
    quick: bool,
    raw_idx: i64,
    values: &[i64],
) -> bool {
    let project_dir = ctx.project_dir.clone();
    let save_cnt = configured_save_count(ctx, false);
    let quick_cnt = configured_save_count(ctx, true);
    let configured = if quick { quick_cnt } else { save_cnt };
    if raw_idx < 0 || raw_idx as usize >= configured {
        return false;
    }
    let idx = raw_idx as usize;
    let slots = if quick {
        &mut ctx.globals.syscom.quick_save_slots
    } else {
        &mut ctx.globals.syscom.save_slots
    };
    ensure_slot_loaded_with_counts(
        &project_dir,
        quick,
        save_cnt,
        quick_cnt,
        slots,
        idx,
    );
    if !slots.get(idx).map(|slot| slot.exist).unwrap_or(false) {
        return false;
    }
    let count = values.len().min(original_save::SAVE_FLAG_MAX_CNT);
    let slot = &mut slots[idx];
    for (i, value) in values.iter().copied().take(count).enumerate() {
        if value == 0 {
            slot.values.remove(&(i as i32));
        } else {
            slot.values.insert(i as i32, value);
        }
    }
    persist_slot_with_counts(
        &project_dir,
        quick,
        save_cnt,
        quick_cnt,
        slots,
        idx,
    );
    true
}

fn sync_slots_from_disk_with_counts(
    project_dir: &Path,
    quick: bool,
    save_cnt: usize,
    quick_cnt: usize,
    slots: &mut Vec<SaveSlotState>,
    count: usize,
) {
    if slots.len() < count {
        slots.resize_with(count, SaveSlotState::default);
    }
    for idx in 0..count {
        ensure_slot_loaded_with_counts(project_dir, quick, save_cnt, quick_cnt, slots, idx);
    }
}

pub(crate) fn sync_save_slots_from_disk(ctx: &mut CommandContext, quick: bool) {
    if save_load_trace_enabled() {
        eprintln!(
            "[SG_SAVELOAD_TRACE][SYSCOM] sync_save_slots_from_disk kind={}",
            if quick { "quick" } else { "normal" }
        );
    }
    let project_dir = ctx.project_dir.clone();
    let save_cnt = configured_save_count(ctx, false);
    let quick_cnt = configured_save_count(ctx, true);
    if quick {
        sync_slots_from_disk_with_counts(
            &project_dir,
            true,
            save_cnt,
            quick_cnt,
            &mut ctx.globals.syscom.quick_save_slots,
            quick_cnt,
        );
    } else {
        sync_slots_from_disk_with_counts(
            &project_dir,
            false,
            save_cnt,
            quick_cnt,
            &mut ctx.globals.syscom.save_slots,
            save_cnt,
        );
    }
}

fn persist_slot_with_counts(
    project_dir: &Path,
    quick: bool,
    save_cnt: usize,
    quick_cnt: usize,
    slots: &mut Vec<SaveSlotState>,
    idx: usize,
) {
    let Some(slot) = slots.get(idx) else {
        return;
    };
    // SET_SAVE_COMMENT / SET_SAVE_VALUE only rewrite an existing header in the
    // original engine. They must never fabricate a local-save payload.
    if !slot.header_cache_valid || !slot.exist {
        return;
    }

    let path = slot_path_with_counts(project_dir, quick, idx, save_cnt, quick_cnt);
    let Some(existing_path) = crate::resource::resolve_game_file(&path).ok().flatten() else {
        return;
    };
    let header = original_save::OriginalSaveHeader::from_slot(slot, slot.packed_data_size);

    // C_tnm_save_cache::save_cache() calls clear_cache(save_no) before opening
    // the file and does not repopulate save_header after the write.  Therefore
    // the next GET_SAVE_* must reload the header from disk.  Keeping the Rust
    // positive cache alive here can make a later LOAD_SCENE observe state that
    // the original engine would have discarded.
    if let Some(slot) = slots.get_mut(idx) {
        slot.header_cache_valid = false;
    }

    if let Err(err) = original_save::write_header_in_place(&existing_path, &header) {
        eprintln!(
            "[SG_SAVE] failed to update original save header {}: {err:#}",
            existing_path.display()
        );
    }
}

fn slot_thumb_save_no(save_cnt: usize, quick_cnt: usize, quick: bool, idx: usize) -> usize {
    let kind = if quick { SaveKind::Quick } else { SaveKind::Normal };
    original_save::original_save_no(save_cnt, quick_cnt, kind, idx)
}

fn remove_thumb_file(project_dir: &Path, save_cnt: usize, quick_cnt: usize, quick: bool, config: SaveThumbConfig, idx: usize) {
    if !config.enabled {
        return;
    }
    let save_no = slot_thumb_save_no(save_cnt, quick_cnt, quick, idx);
    remove_game_file(&thumb_path_for_no_with_config(project_dir, config, save_no));
}

fn copy_thumb_file(project_dir: &Path, save_cnt: usize, quick_cnt: usize, quick: bool, config: SaveThumbConfig, src: usize, dst: usize) {
    if !config.enabled {
        return;
    }
    let src_no = slot_thumb_save_no(save_cnt, quick_cnt, quick, src);
    let dst_no = slot_thumb_save_no(save_cnt, quick_cnt, quick, dst);
    let src_path = thumb_path_for_no_with_config(project_dir, config, src_no);
    let dst_path = thumb_path_for_no_with_config(project_dir, config, dst_no);
    if let Some(src_path) = crate::resource::resolve_game_file(&src_path).ok().flatten() {
        if let Some(parent) = dst_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        copy_game_file(&src_path, &dst_path);
    } else {
        remove_game_file(&dst_path);
    }
}

fn swap_thumb_file(project_dir: &Path, save_cnt: usize, quick_cnt: usize, quick: bool, config: SaveThumbConfig, a: usize, b: usize) {
    if !config.enabled || a == b {
        return;
    }
    let a_no = slot_thumb_save_no(save_cnt, quick_cnt, quick, a);
    let b_no = slot_thumb_save_no(save_cnt, quick_cnt, quick, b);
    let pa = thumb_path_for_no_with_config(project_dir, config, a_no);
    let pb = thumb_path_for_no_with_config(project_dir, config, b_no);
    let tmp = pa.with_extension(format!(
        "{}.swap",
        pa.extension().and_then(|v| v.to_str()).unwrap_or("tmp")
    ));
    let pa_existing = crate::resource::resolve_game_file(&pa).ok().flatten();
    let pb_existing = crate::resource::resolve_game_file(&pb).ok().flatten();
    let a_exists = pa_existing.is_some();
    let b_exists = pb_existing.is_some();
    if let Some(existing) = pa_existing.as_ref() {
        rename_game_file(existing, &tmp);
    }
    if let Some(existing) = pb_existing.as_ref() {
        if let Some(parent) = pa.parent() {
            let _ = fs::create_dir_all(parent);
        }
        rename_game_file(existing, &pa);
    } else {
        remove_game_file(&pa);
    }
    if a_exists {
        if let Some(parent) = pb.parent() {
            let _ = fs::create_dir_all(parent);
        }
        rename_game_file(&tmp, &pb);
    } else {
        remove_game_file(&pb);
    }
    remove_game_file(&tmp);
}

fn copy_save_file(project_dir: &Path, quick: bool, save_cnt: usize, quick_cnt: usize, src: usize, dst: usize) {
    let src_path = slot_path_with_counts(project_dir, quick, src, save_cnt, quick_cnt);
    let dst_path = slot_path_with_counts(project_dir, quick, dst, save_cnt, quick_cnt);
    if let Some(src_path) = crate::resource::resolve_game_file(&src_path).ok().flatten() {
        if let Some(parent) = dst_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        copy_game_file(&src_path, &dst_path);
    } else {
        remove_game_file(&dst_path);
    }
}

fn swap_save_file(project_dir: &Path, quick: bool, save_cnt: usize, quick_cnt: usize, a: usize, b: usize) {
    if a == b {
        return;
    }
    let pa = slot_path_with_counts(project_dir, quick, a, save_cnt, quick_cnt);
    let pb = slot_path_with_counts(project_dir, quick, b, save_cnt, quick_cnt);
    let tmp = pa.with_extension("sav.swap");
    let pa_existing = crate::resource::resolve_game_file(&pa).ok().flatten();
    let pb_existing = crate::resource::resolve_game_file(&pb).ok().flatten();
    let a_exists = pa_existing.is_some();
    let b_exists = pb_existing.is_some();
    if let Some(existing) = pa_existing.as_ref() {
        rename_game_file(existing, &tmp);
    }
    if let Some(existing) = pb_existing.as_ref() {
        if let Some(parent) = pa.parent() {
            let _ = fs::create_dir_all(parent);
        }
        rename_game_file(existing, &pa);
    } else {
        remove_game_file(&pa);
    }
    if a_exists {
        if let Some(parent) = pb.parent() {
            let _ = fs::create_dir_all(parent);
        }
        rename_game_file(&tmp, &pb);
    } else {
        remove_game_file(&pb);
    }
    remove_game_file(&tmp);
}

fn copy_slot(
    project_dir: &Path,
    quick: bool,
    save_cnt: usize,
    quick_cnt: usize,
    thumb_config: SaveThumbConfig,
    slots: &mut Vec<SaveSlotState>,
    src: usize,
    dst: usize,
) -> bool {
    ensure_slot_loaded_with_counts(project_dir, quick, save_cnt, quick_cnt, slots, src);
    let Some(src_slot) = slots.get(src).cloned() else {
        return false;
    };
    if !src_slot.exist {
        return false;
    }
    *ensure_slot(slots, dst) = src_slot;
    copy_save_file(project_dir, quick, save_cnt, quick_cnt, src, dst);
    copy_thumb_file(project_dir, save_cnt, quick_cnt, quick, thumb_config, src, dst);
    true
}

fn change_slot(
    project_dir: &Path,
    quick: bool,
    save_cnt: usize,
    quick_cnt: usize,
    thumb_config: SaveThumbConfig,
    slots: &mut Vec<SaveSlotState>,
    a: usize,
    b: usize,
) -> bool {
    ensure_slot_loaded_with_counts(project_dir, quick, save_cnt, quick_cnt, slots, a);
    ensure_slot_loaded_with_counts(project_dir, quick, save_cnt, quick_cnt, slots, b);
    let max_idx = a.max(b);
    if slots.len() <= max_idx {
        slots.resize_with(max_idx + 1, SaveSlotState::default);
    }
    slots.swap(a, b);
    swap_save_file(project_dir, quick, save_cnt, quick_cnt, a, b);
    swap_thumb_file(project_dir, save_cnt, quick_cnt, quick, thumb_config, a, b);
    true
}

fn delete_slot(
    project_dir: &Path,
    quick: bool,
    save_cnt: usize,
    quick_cnt: usize,
    thumb_config: SaveThumbConfig,
    slots: &mut Vec<SaveSlotState>,
    idx: usize,
) -> bool {
    ensure_slot_loaded_with_counts(project_dir, quick, save_cnt, quick_cnt, slots, idx);
    let existed = slots.get(idx).map(|s| s.exist).unwrap_or(false);
    let mut deleted = SaveSlotState::default();
    // tnm_delete_save_file() clears the cache, then the next existence check
    // observes the missing file. We already know the deletion result here, so
    // retain the equivalent negative-cache state directly.
    deleted.header_cache_valid = true;
    *ensure_slot(slots, idx) = deleted;
    let path = slot_path_with_counts(project_dir, quick, idx, save_cnt, quick_cnt);
    remove_game_file(&path);
    remove_thumb_file(project_dir, save_cnt, quick_cnt, quick, thumb_config, idx);
    existed
}

fn capture_flags_path(image_path: &Path) -> PathBuf {
    let mut p = image_path.to_path_buf();
    let ext = p
        .extension()
        .and_then(|v| v.to_str())
        .map(|v| format!("{v}.siglus_flags"))
        .unwrap_or_else(|| "siglus_flags".to_string());
    p.set_extension(ext);
    p
}

fn named_i64(params: &[Value], id: i32, default: i64) -> i64 {
    params
        .iter()
        .find_map(|v| match v {
            Value::NamedArg { id: nid, value } if *nid == id => value.as_i64(),
            _ => None,
        })
        .unwrap_or(default)
}

fn named_element(params: &[Value], id: i32) -> Option<Vec<i32>> {
    params.iter().find_map(|v| match v {
        Value::NamedArg { id: nid, value } if *nid == id => match value.as_ref() {
            Value::Element(chain) => Some(chain.clone()),
            _ => None,
        },
        _ => None,
    })
}

fn save_capture_flags_sidecar(ctx: &CommandContext, image_path: &Path, params: &[Value]) {
    let Some(flag_chain) = named_element(params, 2) else {
        return;
    };
    let Some(flag_form) = flag_chain.first().copied() else {
        return;
    };
    let flag_index = named_i64(params, 3, 0).max(0) as usize;
    let flag_cnt = named_i64(params, 4, 0).max(0) as usize;
    let str_chain = named_element(params, 5);
    let str_index = named_i64(params, 6, 0).max(0) as usize;
    let str_cnt = named_i64(params, 7, 0).max(0) as usize;

    let mut out = String::new();
    out.push_str("version=1\n");
    out.push_str(&format!("flag_cnt={flag_cnt}\n"));
    if let Some(list) = ctx.globals.int_lists.get(&(flag_form as u32)) {
        for i in 0..flag_cnt {
            let v = list.get(flag_index + i).copied().unwrap_or(0);
            out.push_str(&format!("flag.{i}={v}\n"));
        }
    }
    if let Some(str_form) = str_chain.and_then(|v| v.first().copied()) {
        out.push_str(&format!("str_cnt={str_cnt}\n"));
        if let Some(list) = ctx.globals.str_lists.get(&(str_form as u32)) {
            for i in 0..str_cnt {
                let v = list.get(str_index + i).cloned().unwrap_or_default();
                out.push_str(&format!("str.{i}={}\n", escape_str(&v)));
            }
        }
    }
    if let Some(parent) = image_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let sidecar = capture_flags_path(image_path);
    if fs::write(&sidecar, out).is_ok() {
        mark_game_file_written(&sidecar);
    }
}

fn load_capture_flags_sidecar(
    ctx: &mut CommandContext,
    image_path: &Path,
    params: &[Value],
) -> bool {
    let path = capture_flags_path(image_path);
    let Ok(data) = crate::resource::read_file_to_string(&path) else {
        return crate::resource::game_file_exists(image_path);
    };
    if let Some(flag_chain) = named_element(params, 2) {
        if let Some(flag_form) = flag_chain.first().copied() {
            let flag_index = named_i64(params, 3, 0).max(0) as usize;
            let flag_cnt = named_i64(params, 4, 0).max(0) as usize;
            let mut values = vec![0_i64; flag_cnt];
            for line in data.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    if let Some(i) = k
                        .strip_prefix("flag.")
                        .and_then(|x| x.parse::<usize>().ok())
                    {
                        if i < values.len() {
                            values[i] = v.trim().parse::<i64>().unwrap_or(0);
                        }
                    }
                }
            }
            let list = ctx.globals.int_lists.entry(flag_form as u32).or_default();
            if list.len() < flag_index + flag_cnt {
                list.resize(flag_index + flag_cnt, 0);
            }
            for (i, v) in values.into_iter().enumerate() {
                list[flag_index + i] = v;
            }
        }
    }
    if let Some(str_chain) = named_element(params, 5) {
        if let Some(str_form) = str_chain.first().copied() {
            let str_index = named_i64(params, 6, 0).max(0) as usize;
            let str_cnt = named_i64(params, 7, 0).max(0) as usize;
            let mut values = vec![String::new(); str_cnt];
            for line in data.lines() {
                if let Some((k, v)) = line.split_once('=') {
                    if let Some(i) = k.strip_prefix("str.").and_then(|x| x.parse::<usize>().ok()) {
                        if i < values.len() {
                            values[i] = unescape_str(v);
                        }
                    }
                }
            }
            let list = ctx.globals.str_lists.entry(str_form as u32).or_default();
            if list.len() < str_index + str_cnt {
                list.resize(str_index + str_cnt, String::new());
            }
            for (i, v) in values.into_iter().enumerate() {
                list[str_index + i] = v;
            }
        }
    }
    true
}

fn write_msg_back(ctx: &CommandContext) {
    let form_id = ctx.ids.form_global_msgbk;
    if form_id == 0 {
        return;
    }
    let Some(st) = ctx.globals.msgbk_forms.get(&form_id) else {
        return;
    };
    let dir = save_dir(&ctx.project_dir);
    let path = dir.join("msg_back.txt");

    let mut out = String::new();
    for idx in st.ordered_history_indices() {
        let Some(entry) = st.history.get(idx) else { continue; };
        out.push_str(&format!("-- entry {} --\n", idx));
        if !entry.original_name.is_empty() || !entry.disp_name.is_empty() {
            out.push_str("NAME: ");
            out.push_str(&entry.disp_name);
            out.push('\n');
        }
        if !entry.msg_str.is_empty() {
            if entry.pct_flag {
                out.push_str(&format!(
                    "IMG: {} {} {}\n",
                    entry.msg_str, entry.pct_pos_x, entry.pct_pos_y
                ));
            } else {
                out.push_str("TEXT: ");
                out.push_str(&entry.msg_str);
                out.push('\n');
            }
        }
        for (koe_no, chara_no) in entry.koe_no_list.iter().zip(entry.chr_no_list.iter()) {
            out.push_str(&format!("KOE: {} {}\n", koe_no, chara_no));
        }
        if entry.scn_no >= 0 || entry.line_no >= 0 {
            out.push_str(&format!("SCENE_LINE: {} {}\n", entry.scn_no, entry.line_no));
        }
        out.push('\n');
    }
    if std::fs::write(&path, out).is_ok() {
        mark_game_file_written(&path);
    }
}

fn open_msg_back_proc(ctx: &mut CommandContext) -> bool {
    if ctx.globals.script.msg_back_disable || ctx.globals.syscom.msg_back.check_enabled() == 0 {
        return false;
    }
    ctx.globals.syscom.read_skip.onoff = false;
    ctx.globals.syscom.msg_back_open = true;
    ctx.globals.syscom.msg_back_proc_initialized = false;
    let form_id = ctx.ids.form_global_msgbk;
    let (count, target) = ctx
        .globals
        .msgbk_forms
        .get(&form_id)
        .map(|st| {
            let indices = st.ordered_history_indices();
            let target = if indices.contains(&st.history_last_pos) {
                st.history_last_pos as isize
            } else {
                indices.last().copied().map(|idx| idx as isize).unwrap_or(-1)
            };
            (indices.len(), target)
        })
        .unwrap_or((0, -1));
    ctx.globals.syscom.msg_back_view_pos = count.saturating_sub(1);
    ctx.globals.syscom.msg_back_target_no = target;
    true
}

fn configured_save_count(ctx: &CommandContext, quick: bool) -> usize {
    let keys: [&str; 2] = if quick {
        ["#QUICK_SAVE.CNT", "QUICK_SAVE.CNT"]
    } else {
        ["#SAVE.CNT", "SAVE.CNT"]
    };
    let default_count = if quick { 3 } else { 10 };
    ctx.tables
        .gameexe
        .as_ref()
        .and_then(|cfg| keys.iter().find_map(|key| cfg.get_usize(*key)))
        .unwrap_or(default_count)
        .min(10000)
}

const FALLBACK_BACK: i64 = -1;
const FALLBACK_PREV: i64 = -2;
const FALLBACK_NEXT: i64 = -3;
const FALLBACK_CLOSE: i64 = -4;
const FALLBACK_SLOTS_PER_PAGE: usize = 5;
const FALLBACK_ALL_SOUND: usize = usize::MAX;

fn fallback_button(label: impl Into<String>, value: i64) -> SystemMessageBoxButton {
    SystemMessageBoxButton {
        label: label.into(),
        value,
    }
}

fn fallback_onoff(value: bool) -> &'static str {
    if value { "ON" } else { "OFF" }
}

fn fallback_feature_available(feature: &ToggleFeatureState) -> bool {
    feature.enable && feature.exist
}

fn request_fallback_dialog(
    ctx: &mut CommandContext,
    kind: SyscomFallbackDialogKind,
    page: usize,
    return_kind: Option<SyscomFallbackDialogKind>,
    title: &str,
    body: String,
    buttons: Vec<SystemMessageBoxButton>,
) {
    ctx.globals.syscom.menu_open = false;
    ctx.globals.syscom.menu_kind = None;
    ctx.globals.syscom.menu_result = None;
    ctx.globals.syscom.fallback_dialog = Some(SyscomFallbackDialogState {
        kind,
        page,
        awaiting_result: true,
        return_kind,
    });
    let text = if body.is_empty() {
        title.to_string()
    } else {
        format!("{title}\n\n{body}")
    };
    ctx.request_internal_system_messagebox_no_return(0, false, text, buttons);
}

fn close_fallback_dialog(ctx: &mut CommandContext, save_config: bool) {
    let was_save = matches!(
        ctx.globals.syscom.fallback_dialog.as_ref().map(|state| state.kind),
        Some(SyscomFallbackDialogKind::SaveMenu)
    );
    ctx.globals.syscom.fallback_dialog = None;
    ctx.globals.syscom.fallback_origin = None;
    ctx.globals.syscom.menu_open = false;
    ctx.globals.syscom.menu_kind = None;
    ctx.globals.syscom.menu_result = None;
    if save_config {
        apply_audio_config(ctx);
        write_config_save(ctx);
    }
    if was_save {
        free_runtime_save_thumb_capture(ctx, CAPTURE_PRIOR_SAVE);
    }
}

fn open_fallback_notice(
    ctx: &mut CommandContext,
    message: impl Into<String>,
    return_kind: Option<SyscomFallbackDialogKind>,
) {
    request_fallback_dialog(
        ctx,
        SyscomFallbackDialogKind::Notice,
        0,
        return_kind,
        "SYSTEM",
        message.into(),
        vec![fallback_button("OK", FALLBACK_BACK)],
    );
}

fn open_system_menu_fallback(ctx: &mut CommandContext) {
    ctx.globals.syscom.fallback_origin = None;
    let mut buttons = Vec::new();
    if fallback_feature_available(&ctx.globals.syscom.save_feature) && local_save_available(ctx) {
        buttons.push(fallback_button("セーブ / Save", 0));
    }
    if fallback_feature_available(&ctx.globals.syscom.load_feature) {
        buttons.push(fallback_button("ロード / Load", 1));
    }
    buttons.push(fallback_button("コンフィグ / Config", 2));
    if fallback_feature_available(&ctx.globals.syscom.msg_back) {
        buttons.push(fallback_button("バックログ / Backlog", 3));
    }
    if fallback_feature_available(&ctx.globals.syscom.hide_mwnd) {
        let label = if ctx.globals.syscom.hide_mwnd.onoff {
            "メッセージウィンドウを表示 / Show Message"
        } else {
            "メッセージウィンドウを隠す / Hide Message"
        };
        buttons.push(fallback_button(label, 4));
    }
    if fallback_feature_available(&ctx.globals.syscom.return_to_menu) {
        buttons.push(fallback_button("タイトルへ戻る / Return to Title", 5));
    }
    if fallback_feature_available(&ctx.globals.syscom.end_game) {
        buttons.push(fallback_button("ゲーム終了 / Quit", 6));
    }
    buttons.push(fallback_button("閉じる / Close", FALLBACK_CLOSE));

    request_fallback_dialog(
        ctx,
        SyscomFallbackDialogKind::SystemMenu,
        0,
        None,
        "SYSTEM MENU",
        String::new(),
        buttons,
    );
}

fn slot_fallback_label(slot_no: usize, slot: Option<&SaveSlotState>) -> String {
    let display_no = slot_no + 1;
    let Some(slot) = slot.filter(|slot| slot.exist) else {
        return format!("{display_no:02}: -- EMPTY --");
    };
    let title = if slot.title.trim().is_empty() {
        if slot.message.trim().is_empty() {
            "SAVE DATA"
        } else {
            slot.message.trim()
        }
    } else {
        slot.title.trim()
    };
    let title: String = title.chars().take(24).collect();
    format!(
        "{display_no:02}: {title}  {:04}/{:02}/{:02} {:02}:{:02}",
        slot.year, slot.month, slot.day, slot.hour, slot.minute
    )
}

fn open_save_load_fallback(
    ctx: &mut CommandContext,
    save: bool,
    page: usize,
    return_kind: Option<SyscomFallbackDialogKind>,
) {
    sync_save_slots_from_disk(ctx, false);
    let count = configured_save_count(ctx, false);
    if count == 0 {
        open_fallback_notice(ctx, "No save slots are configured.", return_kind);
        return;
    }
    let page_count = ((count + FALLBACK_SLOTS_PER_PAGE - 1) / FALLBACK_SLOTS_PER_PAGE).max(1);
    let page = page.min(page_count - 1);
    let start = page * FALLBACK_SLOTS_PER_PAGE;
    let end = (start + FALLBACK_SLOTS_PER_PAGE).min(count);
    let mut buttons = Vec::new();
    for idx in start..end {
        let slot = ctx.globals.syscom.save_slots.get(idx);
        buttons.push(fallback_button(slot_fallback_label(idx, slot), idx as i64));
    }
    if page > 0 {
        buttons.push(fallback_button("◀ Previous", FALLBACK_PREV));
    }
    if page + 1 < page_count {
        buttons.push(fallback_button("Next ▶", FALLBACK_NEXT));
    }
    buttons.push(fallback_button("戻る / Back", FALLBACK_BACK));

    let body = if save && !local_save_available(ctx) {
        "A SAVEPOINT snapshot is not available yet.\nSaving is disabled until the script reaches SAVEPOINT."
            .to_string()
    } else {
        format!("Page {}/{}", page + 1, page_count)
    };
    request_fallback_dialog(
        ctx,
        if save {
            SyscomFallbackDialogKind::SaveMenu
        } else {
            SyscomFallbackDialogKind::LoadMenu
        },
        page,
        return_kind,
        if save { "SAVE" } else { "LOAD" },
        body,
        buttons,
    );
}

fn open_config_root_fallback(
    ctx: &mut CommandContext,
    return_kind: Option<SyscomFallbackDialogKind>,
) {
    request_fallback_dialog(
        ctx,
        SyscomFallbackDialogKind::ConfigRoot,
        0,
        return_kind,
        "CONFIG",
        "Settings are applied immediately and written to config.sav when this menu closes."
            .to_string(),
        vec![
            fallback_button("画面 / Display", 0),
            fallback_button("音量 / Volume", 1),
            fallback_button("メッセージ / Message", 2),
            fallback_button("オートモード / Auto Mode", 3),
            fallback_button("フォント / Font", 4),
            fallback_button("その他 / Other", 5),
            fallback_button("設定を保存して戻る / Save & Back", FALLBACK_BACK),
        ],
    );
}

fn open_config_window_fallback(ctx: &mut CommandContext) {
    let mode = cfg_get_int(&ctx.globals.syscom, GET_WINDOW_MODE, 0).clamp(0, 1);
    let size = cfg_get_int(&ctx.globals.syscom, GET_WINDOW_MODE_SIZE, 100).max(1);
    request_fallback_dialog(
        ctx,
        SyscomFallbackDialogKind::ConfigWindow,
        0,
        Some(SyscomFallbackDialogKind::ConfigRoot),
        "CONFIG - DISPLAY",
        format!(
            "Window mode: {}\nWindow scale: {}%",
            if mode == 0 { "Windowed" } else { "Fullscreen" },
            size
        ),
        vec![
            fallback_button("ウィンドウ/フルスクリーン切替", 0),
            fallback_button("ウィンドウサイズ変更", 1),
            fallback_button("戻る / Back", FALLBACK_BACK),
        ],
    );
}

fn fallback_sound_name(sound_type: usize) -> &'static str {
    match sound_type {
        FALLBACK_ALL_SOUND => "ALL",
        0 => "BGM",
        1 => "VOICE",
        2 => "PCM",
        3 => "SE",
        4 => "MOVIE",
        _ => "SOUND",
    }
}

fn fallback_sound_volume(ctx: &CommandContext, sound_type: usize) -> i64 {
    if sound_type == FALLBACK_ALL_SOUND {
        cfg_get_int(&ctx.globals.syscom, GET_ALL_VOLUME, 255).clamp(0, 255)
    } else {
        get_sound_volume_by_type(ctx, sound_type)
    }
}

fn fallback_sound_onoff(ctx: &CommandContext, sound_type: usize) -> bool {
    if sound_type == FALLBACK_ALL_SOUND {
        cfg_get_int(&ctx.globals.syscom, GET_ALL_ONOFF, 1) != 0
    } else {
        get_sound_onoff_by_type(ctx, sound_type)
    }
}

fn set_fallback_sound_volume(ctx: &mut CommandContext, sound_type: usize, value: i64) {
    if sound_type == FALLBACK_ALL_SOUND {
        cfg_set_int(&mut ctx.globals.syscom, GET_ALL_VOLUME, value.clamp(0, 255));
    } else {
        set_sound_volume_by_type(ctx, sound_type, value);
    }
    apply_audio_config(ctx);
}

fn set_fallback_sound_onoff(ctx: &mut CommandContext, sound_type: usize, value: bool) {
    if sound_type == FALLBACK_ALL_SOUND {
        cfg_set_int(
            &mut ctx.globals.syscom,
            GET_ALL_ONOFF,
            if value { 1 } else { 0 },
        );
    } else {
        set_sound_onoff_by_type(ctx, sound_type, value);
    }
    apply_audio_config(ctx);
}

fn open_config_volume_root_fallback(ctx: &mut CommandContext) {
    request_fallback_dialog(
        ctx,
        SyscomFallbackDialogKind::ConfigVolumeRoot,
        0,
        Some(SyscomFallbackDialogKind::ConfigRoot),
        "CONFIG - VOLUME",
        String::new(),
        vec![
            fallback_button("Master / All", 0),
            fallback_button("BGM", 1),
            fallback_button("Voice", 2),
            fallback_button("PCM", 3),
            fallback_button("SE", 4),
            fallback_button("Movie", 5),
            fallback_button("戻る / Back", FALLBACK_BACK),
        ],
    );
}

fn open_config_volume_fallback(ctx: &mut CommandContext, sound_type: usize) {
    let volume = fallback_sound_volume(ctx, sound_type);
    let onoff = fallback_sound_onoff(ctx, sound_type);
    let name = fallback_sound_name(sound_type);
    request_fallback_dialog(
        ctx,
        SyscomFallbackDialogKind::ConfigVolume(sound_type),
        0,
        Some(SyscomFallbackDialogKind::ConfigVolumeRoot),
        &format!("CONFIG - {name}"),
        format!(
            "Output: {}\nVolume: {}% ({volume}/255)",
            fallback_onoff(onoff),
            raw_volume_percent(volume)
        ),
        vec![
            fallback_button("ON/OFF", 0),
            fallback_button("Volume 0%", 1),
            fallback_button("Volume 25%", 2),
            fallback_button("Volume 50%", 3),
            fallback_button("Volume 75%", 4),
            fallback_button("Volume 100%", 5),
            fallback_button("戻る / Back", FALLBACK_BACK),
        ],
    );
}

fn open_config_message_fallback(ctx: &mut CommandContext) {
    let speed = cfg_get_int(&ctx.globals.syscom, GET_MESSAGE_SPEED, 20).clamp(0, 100);
    let nowait = ctx.globals.script.msg_nowait
        || cfg_get_int(&ctx.globals.syscom, GET_MESSAGE_NOWAIT, 0) != 0;
    request_fallback_dialog(
        ctx,
        SyscomFallbackDialogKind::ConfigMessage,
        0,
        Some(SyscomFallbackDialogKind::ConfigRoot),
        "CONFIG - MESSAGE",
        format!(
            "Message speed: {speed}\nInstant message: {}",
            fallback_onoff(nowait)
        ),
        vec![
            fallback_button("メッセージ速度 / Message Speed", 0),
            fallback_button("瞬間表示 / Instant", 1),
            fallback_button("戻る / Back", FALLBACK_BACK),
        ],
    );
}

fn open_config_auto_fallback(ctx: &mut CommandContext) {
    let moji_wait = ctx.globals.script.auto_mode_moji_wait.clamp(0, 500);
    let min_wait = ctx.globals.script.auto_mode_min_wait.clamp(0, 5000);
    request_fallback_dialog(
        ctx,
        SyscomFallbackDialogKind::ConfigAuto,
        0,
        Some(SyscomFallbackDialogKind::ConfigRoot),
        "CONFIG - AUTO MODE",
        format!("Per-character wait: {moji_wait} ms\nMinimum wait: {min_wait} ms"),
        vec![
            fallback_button("文字待ち時間 / Character Wait", 0),
            fallback_button("最低待ち時間 / Minimum Wait", 1),
            fallback_button("戻る / Back", FALLBACK_BACK),
        ],
    );
}

fn fallback_font_names(ctx: &mut CommandContext) -> Vec<String> {
    if ctx.globals.syscom.font_list.is_empty() {
        ctx.globals.syscom.font_list = vec![
            "ＭＳ ゴシック".to_string(),
            "ＭＳ 明朝".to_string(),
            "メイリオ".to_string(),
        ];
    }
    let current = {
        let value = cfg_get_str(&ctx.globals.syscom, GET_FONT_NAME);
        if value.is_empty() {
            config_default_font_name(ctx)
        } else {
            value
        }
    };
    let mut names = ctx.globals.syscom.font_list.clone();
    if !names.iter().any(|name| name.eq_ignore_ascii_case(&current)) {
        names.insert(0, current);
    }
    names
}

fn open_config_font_fallback(ctx: &mut CommandContext) {
    let current = {
        let value = cfg_get_str(&ctx.globals.syscom, GET_FONT_NAME);
        if value.is_empty() {
            config_default_font_name(ctx)
        } else {
            value
        }
    };
    let bold = cfg_get_int(&ctx.globals.syscom, GET_FONT_BOLD, 0) != 0;
    let decoration = cfg_get_int(&ctx.globals.syscom, GET_FONT_DECORATION, 2);
    request_fallback_dialog(
        ctx,
        SyscomFallbackDialogKind::ConfigFont,
        0,
        Some(SyscomFallbackDialogKind::ConfigRoot),
        "CONFIG - FONT",
        format!(
            "Font face: {current}\nBold: {}\nDecoration: {decoration}",
            fallback_onoff(bold)
        ),
        vec![
            fallback_button("フォント切替 / Font Face", 0),
            fallback_button("太字 / Bold", 1),
            fallback_button("装飾 / Decoration", 2),
            fallback_button("戻る / Back", FALLBACK_BACK),
        ],
    );
}

fn open_config_other_fallback(ctx: &mut CommandContext) {
    let wheel = cfg_get_int(&ctx.globals.syscom, GET_WHEEL_NEXT_MESSAGE_ONOFF, 1) != 0;
    let unread = cfg_get_int(&ctx.globals.syscom, GET_SKIP_UNREAD_MESSAGE_ONOFF, 0) != 0;
    let mouse_hide = cfg_get_int(&ctx.globals.syscom, GET_MOUSE_CURSOR_HIDE_ONOFF, 0) != 0;
    let no_wipe = cfg_get_int(&ctx.globals.syscom, GET_NO_WIPE_ANIME_ONOFF, 0) != 0;
    let no_mwnd = cfg_get_int(&ctx.globals.syscom, GET_NO_MWND_ANIME_ONOFF, 0) != 0;
    let alert = cfg_get_int(&ctx.globals.syscom, GET_SAVELOAD_ALERT_ONOFF, 1) != 0;
    request_fallback_dialog(
        ctx,
        SyscomFallbackDialogKind::ConfigOther,
        0,
        Some(SyscomFallbackDialogKind::ConfigRoot),
        "CONFIG - OTHER",
        format!(
            "Wheel advances message: {}\nSkip unread message: {}\nAuto-hide cursor: {}\nDisable wipe animation: {}\nDisable window animation: {}\nSave/load confirmation: {}",
            fallback_onoff(wheel),
            fallback_onoff(unread),
            fallback_onoff(mouse_hide),
            fallback_onoff(no_wipe),
            fallback_onoff(no_mwnd),
            fallback_onoff(alert),
        ),
        vec![
            fallback_button("Mouse wheel message", 0),
            fallback_button("Skip unread message", 1),
            fallback_button("Auto-hide cursor", 2),
            fallback_button("Wipe animation", 3),
            fallback_button("Message-window animation", 4),
            fallback_button("Save/load confirmation", 5),
            fallback_button("戻る / Back", FALLBACK_BACK),
        ],
    );
}

fn reopen_fallback_kind(
    ctx: &mut CommandContext,
    kind: SyscomFallbackDialogKind,
    return_kind: Option<SyscomFallbackDialogKind>,
) {
    match kind {
        SyscomFallbackDialogKind::SystemMenu => open_system_menu_fallback(ctx),
        SyscomFallbackDialogKind::SaveMenu => open_save_load_fallback(ctx, true, 0, return_kind),
        SyscomFallbackDialogKind::LoadMenu => open_save_load_fallback(ctx, false, 0, return_kind),
        SyscomFallbackDialogKind::ConfigRoot => open_config_root_fallback(ctx, return_kind),
        SyscomFallbackDialogKind::ConfigWindow => open_config_window_fallback(ctx),
        SyscomFallbackDialogKind::ConfigVolumeRoot => open_config_volume_root_fallback(ctx),
        SyscomFallbackDialogKind::ConfigVolume(sound_type) => {
            open_config_volume_fallback(ctx, sound_type)
        }
        SyscomFallbackDialogKind::ConfigMessage => open_config_message_fallback(ctx),
        SyscomFallbackDialogKind::ConfigAuto => open_config_auto_fallback(ctx),
        SyscomFallbackDialogKind::ConfigFont => open_config_font_fallback(ctx),
        SyscomFallbackDialogKind::ConfigOther => open_config_other_fallback(ctx),
        SyscomFallbackDialogKind::Notice => close_fallback_dialog(ctx, false),
    }
}

/// Open the built-in cross-platform Syscom UI when the game does not provide
/// its own script scene.  This replaces the original Win32-only popup path
/// without changing the script-visible save/load/config state.
pub fn open_fallback_dialog(ctx: &mut CommandContext, kind: SyscomPendingProcKind) {
    ctx.globals.system.messagebox_modal_result = None;
    ctx.globals.syscom.fallback_origin = None;
    match kind {
        SyscomPendingProcKind::OpenSyscomMenu => open_system_menu_fallback(ctx),
        SyscomPendingProcKind::OpenSave => {
            prepare_runtime_save_thumb_capture_with_priority(ctx, CAPTURE_PRIOR_SAVE);
            open_save_load_fallback(ctx, true, 0, None);
        }
        SyscomPendingProcKind::OpenLoad => {
            open_save_load_fallback(ctx, false, 0, None);
        }
        SyscomPendingProcKind::OpenConfig | SyscomPendingProcKind::OpenConfigDialog => {
            open_config_root_fallback(ctx, None)
        }
        _ => {
            log::error!("unsupported Syscom fallback request: {kind:?}");
        }
    }
}

fn queue_fallback_pending_proc(
    ctx: &mut CommandContext,
    kind: SyscomPendingProcKind,
    warning: bool,
    save_id: i64,
) {
    ctx.globals.syscom.pending_proc = Some(SyscomPendingProc {
        kind,
        warning,
        se_play: false,
        fade_out: false,
        leave_msgbk: false,
        save_id,
    });
    ctx.globals.syscom.fallback_dialog = None;
    ctx.globals.syscom.fallback_origin = None;
}

fn cycle_i64(current: i64, values: &[i64]) -> i64 {
    let index = values.iter().position(|value| *value == current);
    values[index.map_or(0, |index| (index + 1) % values.len())]
}

/// Consume a completed internal modal and advance the fallback menu state
/// machine.  This is called once per frame from CommandContext::tick_frame.
pub(crate) fn poll_fallback_dialog(ctx: &mut CommandContext) {
    let Some(result) = ctx.globals.system.messagebox_modal_result.take() else {
        return;
    };
    let Some(state) = ctx.globals.syscom.fallback_dialog.take() else {
        // Preserve results belonging to non-Syscom SYSTEM.MESSAGEBOX calls.
        ctx.globals.system.messagebox_modal_result = Some(result);
        return;
    };
    if !state.awaiting_result {
        ctx.globals.system.messagebox_modal_result = Some(result);
        ctx.globals.syscom.fallback_dialog = Some(state);
        return;
    }

    match state.kind {
        SyscomFallbackDialogKind::Notice => {
            if let Some(kind) = state.return_kind {
                let origin = ctx.globals.syscom.fallback_origin;
                match kind {
                    SyscomFallbackDialogKind::SaveMenu => {
                        open_save_load_fallback(ctx, true, 0, origin)
                    }
                    SyscomFallbackDialogKind::LoadMenu => {
                        open_save_load_fallback(ctx, false, 0, origin)
                    }
                    _ => reopen_fallback_kind(ctx, kind, origin),
                }
            } else {
                close_fallback_dialog(ctx, false);
            }
        }
        SyscomFallbackDialogKind::SystemMenu => match result {
            0 => {
                ctx.globals.syscom.fallback_origin = Some(SyscomFallbackDialogKind::SystemMenu);
                open_save_load_fallback(
                    ctx,
                    true,
                    0,
                    Some(SyscomFallbackDialogKind::SystemMenu),
                )
            }
            1 => {
                ctx.globals.syscom.fallback_origin = Some(SyscomFallbackDialogKind::SystemMenu);
                open_save_load_fallback(
                    ctx,
                    false,
                    0,
                    Some(SyscomFallbackDialogKind::SystemMenu),
                )
            }
            2 => {
                ctx.globals.syscom.fallback_origin = Some(SyscomFallbackDialogKind::SystemMenu);
                open_config_root_fallback(ctx, Some(SyscomFallbackDialogKind::SystemMenu))
            }
            3 => {
                ctx.globals.syscom.msg_back_open = true;
                queue_fallback_pending_proc(ctx, SyscomPendingProcKind::MsgBack, false, 0);
            }
            4 => {
                ctx.globals.syscom.hide_mwnd.onoff = !ctx.globals.syscom.hide_mwnd.onoff;
                open_system_menu_fallback(ctx);
            }
            5 => queue_fallback_pending_proc(ctx, SyscomPendingProcKind::ReturnToMenu, true, 0),
            6 => queue_fallback_pending_proc(ctx, SyscomPendingProcKind::EndGame, true, 0),
            _ => close_fallback_dialog(ctx, false),
        },
        SyscomFallbackDialogKind::SaveMenu | SyscomFallbackDialogKind::LoadMenu => {
            let save = state.kind == SyscomFallbackDialogKind::SaveMenu;
            match result {
                FALLBACK_PREV => open_save_load_fallback(
                    ctx,
                    save,
                    state.page.saturating_sub(1),
                    state.return_kind,
                ),
                FALLBACK_NEXT => open_save_load_fallback(
                    ctx,
                    save,
                    state.page + 1,
                    state.return_kind,
                ),
                FALLBACK_BACK | FALLBACK_CLOSE => {
                    if save {
                        free_runtime_save_thumb_capture(ctx, CAPTURE_PRIOR_SAVE);
                    }
                    if let Some(kind) = state.return_kind.or(ctx.globals.syscom.fallback_origin) {
                        reopen_fallback_kind(ctx, kind, None);
                    } else {
                        close_fallback_dialog(ctx, false);
                    }
                }
                slot if slot >= 0 => {
                    let idx = slot as usize;
                    if idx >= configured_save_count(ctx, false) {
                        open_save_load_fallback(ctx, save, state.page, state.return_kind);
                        return;
                    }
                    if save {
                        if !local_save_available(ctx) {
                            open_fallback_notice(
                                ctx,
                                "SAVEPOINT data is not available, so this slot cannot be saved.",
                                Some(SyscomFallbackDialogKind::SaveMenu),
                            );
                        } else {
                            let queued = request_confirmed_save_or_load(
                                ctx,
                                SyscomPendingProcKind::Save,
                                idx,
                                true,
                                true,
                            );
                            if queued {
                                ctx.globals.syscom.fallback_origin = None;
                            } else {
                                menu_save_slot(ctx, false, idx);
                                write_global_save(ctx);
                                ctx.globals.syscom.fallback_dialog = None;
                                ctx.globals.syscom.fallback_origin = None;
                            }
                        }
                    } else if local_save_file_exists(ctx, SaveKind::Normal, idx) {
                        let queued = request_confirmed_save_or_load(
                            ctx,
                            SyscomPendingProcKind::Load,
                            idx,
                            true,
                            false,
                        );
                        if queued {
                            ctx.globals.syscom.fallback_origin = None;
                        } else {
                            menu_load_slot(ctx, false, idx);
                            ctx.globals.syscom.fallback_dialog = None;
                            ctx.globals.syscom.fallback_origin = None;
                        }
                    } else {
                        open_fallback_notice(
                            ctx,
                            "The selected save slot is empty.",
                            Some(SyscomFallbackDialogKind::LoadMenu),
                        );
                    }
                }
                _ => open_save_load_fallback(ctx, save, state.page, state.return_kind),
            }
        }
        SyscomFallbackDialogKind::ConfigRoot => match result {
            0 => open_config_window_fallback(ctx),
            1 => open_config_volume_root_fallback(ctx),
            2 => open_config_message_fallback(ctx),
            3 => open_config_auto_fallback(ctx),
            4 => open_config_font_fallback(ctx),
            5 => open_config_other_fallback(ctx),
            _ => {
                apply_audio_config(ctx);
                write_config_save(ctx);
                if let Some(kind) = state.return_kind.or(ctx.globals.syscom.fallback_origin) {
                    reopen_fallback_kind(ctx, kind, None);
                } else {
                    close_fallback_dialog(ctx, false);
                }
            }
        },
        SyscomFallbackDialogKind::ConfigWindow => match result {
            0 => {
                let next = if cfg_get_int(&ctx.globals.syscom, GET_WINDOW_MODE, 0) == 0 {
                    1
                } else {
                    0
                };
                cfg_set_int(&mut ctx.globals.syscom, GET_WINDOW_MODE, next);
                open_config_window_fallback(ctx);
            }
            1 => {
                const SIZES: &[i64] = &[50, 75, 100, 150, 200];
                let current = cfg_get_int(&ctx.globals.syscom, GET_WINDOW_MODE_SIZE, 100);
                let next = cycle_i64(current, SIZES);
                cfg_set_int(&mut ctx.globals.syscom, GET_WINDOW_MODE_SIZE, next);
                open_config_window_fallback(ctx);
            }
            _ => {
                let origin = ctx.globals.syscom.fallback_origin;
                open_config_root_fallback(ctx, origin)
            }
        },
        SyscomFallbackDialogKind::ConfigVolumeRoot => match result {
            0 => open_config_volume_fallback(ctx, FALLBACK_ALL_SOUND),
            1 => open_config_volume_fallback(ctx, 0),
            2 => open_config_volume_fallback(ctx, 1),
            3 => open_config_volume_fallback(ctx, 2),
            4 => open_config_volume_fallback(ctx, 3),
            5 => open_config_volume_fallback(ctx, 4),
            _ => {
                let origin = ctx.globals.syscom.fallback_origin;
                open_config_root_fallback(ctx, origin)
            }
        },
        SyscomFallbackDialogKind::ConfigVolume(sound_type) => match result {
            0 => {
                let next = !fallback_sound_onoff(ctx, sound_type);
                set_fallback_sound_onoff(ctx, sound_type, next);
                open_config_volume_fallback(ctx, sound_type);
            }
            1..=5 => {
                const RAW: &[i64] = &[0, 64, 128, 191, 255];
                set_fallback_sound_volume(ctx, sound_type, RAW[(result - 1) as usize]);
                open_config_volume_fallback(ctx, sound_type);
            }
            _ => open_config_volume_root_fallback(ctx),
        },
        SyscomFallbackDialogKind::ConfigMessage => match result {
            0 => {
                const SPEEDS: &[i64] = &[0, 10, 20, 40, 60, 80, 100];
                let current = cfg_get_int(&ctx.globals.syscom, GET_MESSAGE_SPEED, 20);
                let next = cycle_i64(current, SPEEDS);
                cfg_set_int(&mut ctx.globals.syscom, GET_MESSAGE_SPEED, next);
                open_config_message_fallback(ctx);
            }
            1 => {
                let next = !(ctx.globals.script.msg_nowait
                    || cfg_get_int(&ctx.globals.syscom, GET_MESSAGE_NOWAIT, 0) != 0);
                ctx.globals.script.msg_nowait = next;
                cfg_set_int(
                    &mut ctx.globals.syscom,
                    GET_MESSAGE_NOWAIT,
                    if next { 1 } else { 0 },
                );
                open_config_message_fallback(ctx);
            }
            _ => {
                let origin = ctx.globals.syscom.fallback_origin;
                open_config_root_fallback(ctx, origin)
            }
        },
        SyscomFallbackDialogKind::ConfigAuto => match result {
            0 => {
                const WAITS: &[i64] = &[0, 20, 40, 70, 100, 150, 250, 500];
                let next = cycle_i64(ctx.globals.script.auto_mode_moji_wait, WAITS);
                ctx.globals.script.auto_mode_moji_wait = next;
                cfg_set_int(&mut ctx.globals.syscom, GET_AUTO_MODE_MOJI_WAIT, next);
                open_config_auto_fallback(ctx);
            }
            1 => {
                const WAITS: &[i64] = &[0, 100, 200, 300, 500, 750, 1000, 1500, 2000, 3000, 5000];
                let next = cycle_i64(ctx.globals.script.auto_mode_min_wait, WAITS);
                ctx.globals.script.auto_mode_min_wait = next;
                cfg_set_int(&mut ctx.globals.syscom, GET_AUTO_MODE_MIN_WAIT, next);
                open_config_auto_fallback(ctx);
            }
            _ => {
                let origin = ctx.globals.syscom.fallback_origin;
                open_config_root_fallback(ctx, origin)
            }
        },
        SyscomFallbackDialogKind::ConfigFont => match result {
            0 => {
                let names = fallback_font_names(ctx);
                let current = cfg_get_str(&ctx.globals.syscom, GET_FONT_NAME);
                let index = names
                    .iter()
                    .position(|name| name.eq_ignore_ascii_case(&current))
                    .unwrap_or(0);
                let next = names[(index + 1) % names.len()].clone();
                cfg_set_str(&mut ctx.globals.syscom, GET_FONT_NAME, next);
                open_config_font_fallback(ctx);
            }
            1 => {
                let next = cfg_get_int(&ctx.globals.syscom, GET_FONT_BOLD, 0) == 0;
                cfg_set_int(
                    &mut ctx.globals.syscom,
                    GET_FONT_BOLD,
                    if next { 1 } else { 0 },
                );
                open_config_font_fallback(ctx);
            }
            2 => {
                let current = cfg_get_int(&ctx.globals.syscom, GET_FONT_DECORATION, 2);
                cfg_set_int(
                    &mut ctx.globals.syscom,
                    GET_FONT_DECORATION,
                    (current + 1).rem_euclid(4),
                );
                open_config_font_fallback(ctx);
            }
            _ => {
                let origin = ctx.globals.syscom.fallback_origin;
                open_config_root_fallback(ctx, origin)
            }
        },
        SyscomFallbackDialogKind::ConfigOther => {
            let key = match result {
                0 => Some(GET_WHEEL_NEXT_MESSAGE_ONOFF),
                1 => Some(GET_SKIP_UNREAD_MESSAGE_ONOFF),
                2 => Some(GET_MOUSE_CURSOR_HIDE_ONOFF),
                3 => Some(GET_NO_WIPE_ANIME_ONOFF),
                4 => Some(GET_NO_MWND_ANIME_ONOFF),
                5 => Some(GET_SAVELOAD_ALERT_ONOFF),
                _ => None,
            };
            if let Some(key) = key {
                let current = cfg_get_int(&ctx.globals.syscom, key, if key == GET_WHEEL_NEXT_MESSAGE_ONOFF || key == GET_SAVELOAD_ALERT_ONOFF { 1 } else { 0 });
                cfg_set_int(&mut ctx.globals.syscom, key, if current == 0 { 1 } else { 0 });
                open_config_other_fallback(ctx);
            } else {
                let origin = ctx.globals.syscom.fallback_origin;
                open_config_root_fallback(ctx, origin);
            }
        }
    }
}

fn save_header_is_older(lhs: &SaveSlotState, rhs: &SaveSlotState) -> bool {
    macro_rules! compare_field {
        ($field:ident) => {
            if lhs.$field < rhs.$field {
                return true;
            }
            if lhs.$field > rhs.$field {
                return false;
            }
        };
    }

    // S_tnm_save_header::operator< in the original engine compares the
    // timestamp fields in this exact order (weekday is intentionally ignored),
    // then title.  Equality also returns true, so a later slot wins ties.
    compare_field!(year);
    compare_field!(month);
    compare_field!(day);
    compare_field!(hour);
    compare_field!(minute);
    compare_field!(second);
    compare_field!(millisecond);
    lhs.title <= rhs.title
}

fn newest_slot_in_range(
    slots: &[SaveSlotState],
    start: i64,
    cnt: i64,
    configured_count: usize,
) -> i64 {
    if cnt <= 0 || configured_count == 0 {
        return -1;
    }

    // The original loops [start, start + cnt) but only accepts indices inside
    // the configured save range.  Intersect first so malformed script input
    // cannot force an unbounded loop or grow the Rust slot cache.
    let end = start.saturating_add(cnt);
    let lo = start.max(0).min(configured_count as i64);
    let hi = end.max(0).min(configured_count as i64);
    if hi <= lo {
        return -1;
    }

    let mut newest_idx = -1i64;
    let mut newest_slot: Option<&SaveSlotState> = None;
    for idx in lo as usize..hi as usize {
        let Some(slot) = slots.get(idx) else {
            continue;
        };
        if !slot.exist {
            continue;
        }
        if newest_slot
            .map(|current| save_header_is_older(current, slot))
            .unwrap_or(true)
        {
            newest_idx = idx as i64;
            newest_slot = Some(slot);
        }
    }
    newest_idx
}

fn slot_i64(slot: &SaveSlotState, op: i32) -> i64 {
    match op {
        GET_SAVE_EXIST | GET_QUICK_SAVE_EXIST => {
            if slot.exist {
                1
            } else {
                0
            }
        }
        GET_SAVE_YEAR | GET_QUICK_SAVE_YEAR => slot.year,
        GET_SAVE_MONTH | GET_QUICK_SAVE_MONTH => slot.month,
        GET_SAVE_DAY | GET_QUICK_SAVE_DAY => slot.day,
        GET_SAVE_WEEKDAY | GET_QUICK_SAVE_WEEKDAY => slot.weekday,
        GET_SAVE_HOUR | GET_QUICK_SAVE_HOUR => slot.hour,
        GET_SAVE_MINUTE | GET_QUICK_SAVE_MINUTE => slot.minute,
        GET_SAVE_SECOND | GET_QUICK_SAVE_SECOND => slot.second,
        GET_SAVE_MILLISECOND | GET_QUICK_SAVE_MILLISECOND => slot.millisecond,
        _ => 0,
    }
}

fn slot_str(slot: &SaveSlotState, op: i32) -> String {
    match op {
        GET_SAVE_TITLE | GET_QUICK_SAVE_TITLE => slot.title.clone(),
        GET_SAVE_MESSAGE | GET_QUICK_SAVE_MESSAGE => slot.message.clone(),
        GET_SAVE_FULL_MESSAGE | GET_QUICK_SAVE_FULL_MESSAGE => slot.full_message.clone(),
        GET_SAVE_COMMENT | GET_QUICK_SAVE_COMMENT => slot.comment.clone(),
        GET_SAVE_APPEND_DIR | GET_QUICK_SAVE_APPEND_DIR => slot.append_dir.clone(),
        GET_SAVE_APPEND_NAME | GET_QUICK_SAVE_APPEND_NAME => slot.append_name.clone(),
        _ => String::new(),
    }
}

fn cfg_get_int(st: &crate::runtime::globals::SyscomRuntimeState, key: i32, default: i64) -> i64 {
    st.config_int.get(&key).copied().unwrap_or(default)
}

fn cfg_set_int(st: &mut crate::runtime::globals::SyscomRuntimeState, key: i32, value: i64) {
    st.config_int.insert(key, value);
}

fn volume_to_raw(v: i64) -> u8 {
    v.clamp(0, 255) as u8
}

fn standard_sound_volume_key(sound_type: usize) -> Option<i32> {
    match sound_type {
        0 => Some(GET_BGM_VOLUME),
        1 => Some(GET_KOE_VOLUME),
        2 => Some(GET_PCM_VOLUME),
        3 => Some(GET_SE_VOLUME),
        4 => Some(GET_MOV_VOLUME),
        _ => None,
    }
}

fn standard_sound_onoff_key(sound_type: usize) -> Option<i32> {
    match sound_type {
        0 => Some(GET_BGM_ONOFF),
        1 => Some(GET_KOE_ONOFF),
        2 => Some(GET_PCM_ONOFF),
        3 => Some(GET_SE_ONOFF),
        4 => Some(GET_MOV_ONOFF),
        _ => None,
    }
}

fn set_sound_volume_by_type(ctx: &mut CommandContext, sound_type: usize, value: i64) {
    if sound_type >= ctx.globals.syscom.original_config.sound_user_volume.len() {
        return;
    }
    let value = value.clamp(0, 255);
    ctx.globals.syscom.original_config.sound_user_volume[sound_type] = value;
    if let Some(key) = standard_sound_volume_key(sound_type) {
        cfg_set_int(&mut ctx.globals.syscom, key, value);
    }
}

fn get_sound_volume_by_type(ctx: &CommandContext, sound_type: usize) -> i64 {
    if sound_type >= ctx.globals.syscom.original_config.sound_user_volume.len() {
        return 255;
    }
    standard_sound_volume_key(sound_type)
        .map(|key| {
            cfg_get_int(
                &ctx.globals.syscom,
                key,
                ctx.globals.syscom.original_config.sound_user_volume[sound_type],
            )
        })
        .unwrap_or(ctx.globals.syscom.original_config.sound_user_volume[sound_type])
        .clamp(0, 255)
}

fn set_sound_onoff_by_type(ctx: &mut CommandContext, sound_type: usize, value: bool) {
    if sound_type >= ctx.globals.syscom.original_config.play_sound_check.len() {
        return;
    }
    ctx.globals.syscom.original_config.play_sound_check[sound_type] = value;
    if let Some(key) = standard_sound_onoff_key(sound_type) {
        cfg_set_int(&mut ctx.globals.syscom, key, if value { 1 } else { 0 });
    }
}

fn get_sound_onoff_by_type(ctx: &CommandContext, sound_type: usize) -> bool {
    if sound_type >= ctx.globals.syscom.original_config.play_sound_check.len() {
        return true;
    }
    standard_sound_onoff_key(sound_type)
        .map(|key| {
            cfg_get_int(
                &ctx.globals.syscom,
                key,
                if ctx.globals.syscom.original_config.play_sound_check[sound_type] {
                    1
                } else {
                    0
                },
            ) != 0
        })
        .unwrap_or(ctx.globals.syscom.original_config.play_sound_check[sound_type])
}

fn raw_volume_percent(value: i64) -> i64 {
    (value.clamp(0, 255) * 100 + 127) / 255
}

fn mul_raw(lhs: i64, rhs: i64) -> i64 {
    lhs.clamp(0, 255) * rhs.clamp(0, 255) / 255
}

fn linear_limit_i64(time: i64, start_time: i64, start: i64, end_time: i64, end: i64) -> i64 {
    if end_time <= start_time {
        return if time < start_time { start } else { end };
    }
    if time <= start_time {
        start
    } else if time >= end_time {
        end
    } else {
        start + (end - start) * (time - start_time) / (end_time - start_time)
    }
}

fn chrkoe_chara_numbers(ctx: &CommandContext, index: usize) -> Vec<i64> {
    let Some(entry) = ctx
        .tables
        .gameexe
        .as_ref()
        .and_then(|cfg| cfg.get_indexed_entry("CHRKOE", index))
    else {
        return Vec::new();
    };
    // #CHRKOE.i = name, check_mode, check_name, onoff, volume, (chara...)
    // `value_items` is intentionally a CSV-like view and does not preserve the
    // parenthesized list as one token, so recover the list from the raw value.
    let Some(open) = entry.value.find('(') else {
        return Vec::new();
    };
    let close = entry.value.rfind(')').unwrap_or(entry.value.len());
    if close <= open {
        return Vec::new();
    }
    parse_i64_list_local(&entry.value[open + 1..close])
}

fn chrkoe_route(
    config: &crate::runtime::globals::OriginalConfigRuntimeState,
    chara_lists: &[Vec<i64>],
    chara_no: i64,
) -> (bool, i64) {
    if chara_no < 0 {
        return (true, 255);
    }
    let mut onoff = true;
    let mut volume = 255;
    for (index, state) in config.chrkoe.iter().enumerate() {
        if chara_lists
            .get(index)
            .is_some_and(|numbers| numbers.iter().any(|number| *number == chara_no))
        {
            if !state.onoff {
                onoff = false;
            }
            volume = mul_raw(volume, state.volume);
        }
    }
    (onoff, volume)
}

fn pcmch_routed_volume(
    config: &crate::runtime::globals::OriginalConfigRuntimeState,
    chara_lists: &[Vec<i64>],
    state: &crate::runtime::globals::PcmChPersistentState,
    all_total: i64,
    category_total: &[i64; 5],
    bgmfade_total: i64,
    bgmfade2_total: i64,
) -> u8 {
    let volume_type = state.volume_type as i32;
    let mut volume = match volume_type {
        -1 => all_total,
        0..=4 => category_total[volume_type as usize],
        16..=31 => {
            let index = volume_type as usize;
            if config.play_sound_check[index] {
                mul_raw(all_total, config.sound_user_volume[index])
            } else {
                0
            }
        }
        _ => 255,
    };

    if state.bgm_fade_target_flag {
        volume = mul_raw(volume, bgmfade_total);
    }
    if state.bgm_fade2_target_flag {
        volume = mul_raw(volume, bgmfade2_total);
    }
    let (chara_onoff, chara_volume) = chrkoe_route(config, chara_lists, state.chara_no);
    volume = mul_raw(volume, chara_volume);
    if !chara_onoff {
        volume = 0;
    }
    volume.clamp(0, 255) as u8
}

/// Recompute the original sound routing graph.
///
/// PCMCH script volume remains on the individual Kira handle. This function
/// supplies the independent C++ `m_system_volume` term selected by
/// `volume_type`, character voice settings and the two BGM-fade target flags.
pub(crate) fn update_audio_routing(
    ctx: &mut CommandContext,
    real_delta_ms: i32,
    change_force: bool,
) {
    use crate::audio::TrackKind;

    let config = &ctx.globals.syscom.original_config;
    let chrkoe_lists = {
        if ctx.globals.sound_routing.chrkoe_chara_lists_cache.len() < config.chrkoe.len() {
            let lists = (0..config.chrkoe.len())
                .map(|index| chrkoe_chara_numbers(ctx, index))
                .collect::<Vec<Vec<i64>>>();
            ctx.globals.sound_routing.chrkoe_chara_lists_cache = std::sync::Arc::new(lists);
        }
        std::sync::Arc::clone(&ctx.globals.sound_routing.chrkoe_chara_lists_cache)
    };
    let all_total = if config.play_all_sound_check {
        config.all_sound_user_volume.clamp(0, 255)
    } else {
        0
    };
    let mut category_total = [0i64; 5];
    for (index, total) in category_total.iter_mut().enumerate() {
        *total = if config.play_sound_check[index] {
            mul_raw(all_total, config.sound_user_volume[index])
        } else {
            0
        };
    }

    // Source PCMCH channels are updated before the fade state transition in
    // ifc_sound.cpp. Their audible state decides whether both fades are needed.
    let old_fade = ctx.globals.sound_routing.bgmfade_total_volume;
    let old_fade2 = ctx.globals.sound_routing.bgmfade2_total_volume;
    let mut pcmch_bgm_fade_use = false;
    let source_channels: Vec<(usize, crate::runtime::globals::PcmChPersistentState)> = ctx
        .globals
        .pcmch_persistent
        .iter()
        .cloned()
        .enumerate()
        .filter(|(_, state)| state.bgm_fade_source_flag)
        .collect();
    for (channel, state) in source_channels {
        let routed = pcmch_routed_volume(
            &config,
            chrkoe_lists.as_slice(),
            &state,
            all_total,
            &category_total,
            old_fade,
            old_fade2,
        );
        let _ = ctx.pcm.set_slot_system_volume_raw(channel, routed);
        if routed > 0 && ctx.pcm.is_playing_slot(channel) {
            pcmch_bgm_fade_use = true;
        }
    }

    // The actual KOE player can contain either a normal message voice or an
    // EXKOE voice. EXKOE ignores the KOE on/off checkbox but still uses the KOE
    // user-volume slider; both kinds use character-group volume.
    let koe_chara_no = ctx.globals.sound_routing.koe_chara_no;
    let koe_ex_flag = ctx.globals.sound_routing.koe_ex_flag;
    let (koe_chara_onoff, koe_chara_volume) =
        chrkoe_route(&config, chrkoe_lists.as_slice(), koe_chara_no);
    let mut koe_buf_total = if koe_ex_flag {
        mul_raw(all_total, config.sound_user_volume[1])
    } else {
        category_total[1]
    };
    koe_buf_total = mul_raw(koe_buf_total, koe_chara_volume);
    if !koe_ex_flag
        && (!koe_chara_onoff
            || !config.play_all_sound_check
            || !config.play_sound_check[1]
            || config.koe_mode == 1)
    {
        // tnm_update_sound_volume stops a normal voice when its current config
        // says it must not play. EXKOE is deliberately not stopped here.
        if ctx.koe.is_playing_any() {
            let _ = ctx.koe.stop(None);
        }
        koe_buf_total = 0;
    }
    let koe_audible = koe_buf_total > 0 && ctx.koe.is_playing_any();
    let use_bgmfade = config.bgmfade_use_check
        && (ctx.globals.script.bgmfade_flag || koe_audible || pcmch_bgm_fade_use);
    let use_bgmfade2 = koe_audible || pcmch_bgm_fade_use;
    let bgmfade2_in_start = gameexe_i64_or(ctx, "BGMFADE2.IN_START_TIME", 0);
    let bgmfade2_in_len = gameexe_i64_or(ctx, "BGMFADE2.IN_TIME_LEN", 500).max(0);
    let bgmfade2_out_start = gameexe_i64_or(ctx, "BGMFADE2.OUT_START_TIME", 0);
    let bgmfade2_out_len = gameexe_i64_or(ctx, "BGMFADE2.OUT_TIME_LEN", 500).max(0);
    let bgmfade2_volume = gameexe_i64_or(ctx, "BGMFADE2.VOLUME", 0).clamp(0, 255);
    let bgmfade2_need = ctx.globals.sound_routing.bgmfade2_need_flag;

    // Keep the mutable borrow of the routing sub-state in a narrow scope.
    // The volume application below also mutates other CommandContext fields.
    let (fade, fade2) = {
        let routing = &mut ctx.globals.sound_routing;
        routing.bgmfade_cur_time = routing
            .bgmfade_cur_time
            .saturating_add(real_delta_ms.max(0) as i64);
        routing.bgmfade2_cur_time = routing
            .bgmfade2_cur_time
            .saturating_add(real_delta_ms.max(0) as i64);

        if routing.bgmfade_flag != use_bgmfade {
            routing.bgmfade_cur_time = 0;
            routing.bgmfade_start_value = routing.bgmfade_total_volume;
            routing.bgmfade_flag = use_bgmfade;
        }
        if routing.bgmfade2_flag != use_bgmfade2 {
            routing.bgmfade2_cur_time = 0;
            routing.bgmfade2_start_value = routing.bgmfade2_total_volume;
            routing.bgmfade2_flag = use_bgmfade2;
        }

        routing.bgmfade_total_volume = if routing.bgmfade_flag {
            linear_limit_i64(
                routing.bgmfade_cur_time,
                0,
                routing.bgmfade_start_value,
                1000,
                config.bgmfade_volume,
            )
        } else {
            linear_limit_i64(
                routing.bgmfade_cur_time,
                2000,
                routing.bgmfade_start_value,
                3000,
                255,
            )
        }
        .clamp(0, 255);

        routing.bgmfade2_total_volume = if routing.bgmfade2_flag {
            linear_limit_i64(
                routing.bgmfade2_cur_time,
                bgmfade2_in_start,
                routing.bgmfade2_start_value,
                bgmfade2_in_start.saturating_add(bgmfade2_in_len),
                bgmfade2_volume,
            )
        } else if bgmfade2_need {
            linear_limit_i64(
                routing.bgmfade2_cur_time,
                bgmfade2_out_start,
                routing.bgmfade2_start_value,
                bgmfade2_out_start.saturating_add(bgmfade2_out_len),
                255,
            )
        } else {
            // Original one-voice-after-another protection waits 500ms before
            // restoring BGM when no message voice is currently attached.
            linear_limit_i64(
                routing.bgmfade2_cur_time,
                500,
                routing.bgmfade2_start_value,
                500i64.saturating_add(bgmfade2_out_len),
                255,
            )
        }
        .clamp(0, 255);

        (
            routing.bgmfade_total_volume,
            routing.bgmfade2_total_volume,
        )
    };

    let bgm_total = mul_raw(category_total[0], fade) as u8;
    ctx.audio
        .set_track_master_volume_raw(TrackKind::Bgm, bgm_total);
    ctx.audio
        .set_track_master_volume_raw(TrackKind::Koe, koe_buf_total.clamp(0, 255) as u8);
    // Global PCM and every PCMCH channel have independent system-volume terms,
    // so the shared PCM track itself must remain neutral.
    ctx.audio.set_track_master_volume_raw(TrackKind::Pcm, 255);
    ctx.audio
        .set_track_master_volume_raw(TrackKind::Se, category_total[3] as u8);
    ctx.audio
        .set_track_master_volume_raw(TrackKind::Mov, category_total[4] as u8);
    let _ = ctx
        .pcm
        .set_global_system_volume_raw(category_total[2] as u8);

    let normal_channels: Vec<(usize, crate::runtime::globals::PcmChPersistentState)> = ctx
        .globals
        .pcmch_persistent
        .iter()
        .cloned()
        .enumerate()
        .filter(|(_, state)| !state.bgm_fade_source_flag)
        .collect();
    for (channel, state) in normal_channels {
        let routed = pcmch_routed_volume(
            &config,
            chrkoe_lists.as_slice(),
            &state,
            all_total,
            &category_total,
            fade,
            fade2,
        );
        let _ = ctx.pcm.set_slot_system_volume_raw(channel, routed);
    }

    if change_force {
        // Force source channels through the newly calculated target as well.
        // The C++ path reaches them on the following sound update; this extra
        // pass is required only for immediate config/load application where no
        // real-time frame has elapsed yet.
        let sources: Vec<(usize, crate::runtime::globals::PcmChPersistentState)> = ctx
            .globals
            .pcmch_persistent
            .iter()
            .cloned()
            .enumerate()
            .filter(|(_, state)| state.bgm_fade_source_flag)
            .collect();
        for (channel, state) in sources {
            let routed = pcmch_routed_volume(
                &config,
                chrkoe_lists.as_slice(),
                &state,
                all_total,
                &category_total,
                fade,
                fade2,
            );
            let _ = ctx.pcm.set_slot_system_volume_raw(channel, routed);
        }
    }
}

pub(crate) fn apply_audio_config(ctx: &mut CommandContext) {
    update_audio_routing(ctx, 0, true);
}

fn cfg_get_str(st: &crate::runtime::globals::SyscomRuntimeState, key: i32) -> String {
    st.config_str.get(&key).cloned().unwrap_or_default()
}

fn cfg_set_str(st: &mut crate::runtime::globals::SyscomRuntimeState, key: i32, value: String) {
    st.config_str.insert(key, value);
}

fn join_game_path(base: &Path, raw: &str) -> PathBuf {
    if raw.is_empty() {
        return base.to_path_buf();
    }
    let norm = raw.replace('\\', "/");
    let p = Path::new(&norm);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

fn remove_game_file(path: &Path) {
    let _ = fs::remove_file(path);
    crate::resource::invalidate_game_path_cache(path);
}

fn copy_game_file(src: &Path, dst: &Path) {
    let _ = fs::copy(src, dst);
    crate::resource::invalidate_game_path_cache(dst);
}

fn rename_game_file(src: &Path, dst: &Path) {
    let _ = fs::rename(src, dst);
    crate::resource::invalidate_game_path_cache(src);
    crate::resource::invalidate_game_path_cache(dst);
}

fn mark_game_file_written(path: &Path) {
    crate::resource::invalidate_game_path_cache(path);
}

fn opaque_rgba(img: &RgbaImage) -> RgbaImage {
    let mut rgba = img.rgba.clone();
    for px in rgba.chunks_exact_mut(4) {
        px[3] = 255;
    }
    RgbaImage {
        width: img.width,
        height: img.height,
        center_x: 0,
        center_y: 0,
        rgba,
    }
}

fn write_rgba_png(path: &Path, img: &RgbaImage) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let Some(buf) = image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone()) else {
        anyhow::bail!("invalid rgba buffer for {}x{} image", img.width, img.height);
    };
    buf.save(path)?;
    mark_game_file_written(path);
    Ok(())
}

fn write_rgba_png_opaque(path: &Path, img: &RgbaImage) -> Result<()> {
    let opaque = opaque_rgba(img);
    write_rgba_png(path, &opaque)
}

fn push_u16_le(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_u32_le(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn push_i32_le(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn write_rgba_bmp_top_down(path: &Path, img: &RgbaImage) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let width = img.width;
    let height = img.height;
    if width == 0 || height == 0 {
        anyhow::bail!("invalid zero-sized bmp image {}x{}", width, height);
    }
    let pixel_size = width.saturating_mul(height).saturating_mul(4);
    let file_size = 14u32.saturating_add(40).saturating_add(pixel_size);
    let mut out = Vec::with_capacity(file_size as usize);

    out.extend_from_slice(b"BM");
    push_u32_le(&mut out, file_size);
    push_u16_le(&mut out, 0);
    push_u16_le(&mut out, 0);
    push_u32_le(&mut out, 14 + 40);

    push_u32_le(&mut out, 40);
    push_i32_le(&mut out, width as i32);
    push_i32_le(&mut out, -(height as i32));
    push_u16_le(&mut out, 1);
    push_u16_le(&mut out, 32);
    push_u32_le(&mut out, 0);
    push_u32_le(&mut out, 0);
    push_i32_le(&mut out, 0);
    push_i32_le(&mut out, 0);
    push_u32_le(&mut out, 0);
    push_u32_le(&mut out, 0);

    for px in img.rgba.chunks_exact(4) {
        out.push(px[2]);
        out.push(px[1]);
        out.push(px[0]);
        out.push(px[3]);
    }
    fs::write(path, out)?;
    mark_game_file_written(path);
    Ok(())
}

fn resize_rgba(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    if img.width == 0 || img.height == 0 || w == 0 || h == 0 {
        return img.clone();
    }
    if img.width == w && img.height == h {
        return img.clone();
    }
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let src_y = (y as u64 * img.height as u64 / h as u64) as u32;
        for x in 0..w {
            let src_x = (x as u64 * img.width as u64 / w as u64) as u32;
            let si = ((src_y * img.width + src_x) * 4) as usize;
            let di = ((y * w + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&img.rgba[si..si + 4]);
        }
    }
    RgbaImage {
        width: w,
        height: h,
        center_x: 0,
        center_y: 0,
        rgba: out,
    }
}

pub(crate) fn resize_capture_rgba_nearest(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    resize_rgba(img, w, h)
}


fn blend_tweet_overlay_fullscreen(base: &mut RgbaImage, overlay: &RgbaImage) {
    if base.width == 0 || base.height == 0 || overlay.width == 0 || overlay.height == 0 {
        return;
    }

    // Original C_tnm_wnd::disp_proc_capture_for_tweet renders the configured
    // TWITTER.OVERLAP_IMAGE as a screen-sized D2 sprite with alpha test/blend.
    // Stretch the source over the whole capture target, then apply the normal
    // source-alpha / inverse-source-alpha blend into the X8R8G8B8-equivalent
    // opaque capture buffer.
    let scaled = resize_rgba(overlay, base.width, base.height);
    for (dst, src) in base
        .rgba
        .chunks_exact_mut(4)
        .zip(scaled.rgba.chunks_exact(4))
    {
        let alpha = u32::from(src[3]);
        if alpha == 0 {
            dst[3] = 255;
            continue;
        }
        let inv_alpha = 255 - alpha;
        for channel in 0..3 {
            let value = u32::from(src[channel]) * alpha
                + u32::from(dst[channel]) * inv_alpha
                + 127;
            dst[channel] = (value / 255) as u8;
        }
        dst[3] = 255;
    }
}

/// Create the image consumed by SYSCOM.OPEN_TWEET_DIALOG.
///
/// This mirrors C_tnm_wnd::disp_proc_capture_for_tweet: capture the game at
/// the logical screen size and, when #TWITTER.OVERLAP_IMAGE is configured and
/// resolvable, render cut 0 over the full capture using alpha blending.
pub fn capture_for_tweet(ctx: &mut CommandContext) -> Result<RgbaImage> {
    let mut capture = ctx.capture_frame_rgba()?;
    let overlap_name = gameexe_unquoted_owned(ctx, "TWITTER.OVERLAP_IMAGE");
    if overlap_name.is_empty() {
        return Ok(capture);
    }

    match ctx.images.load_g00(&overlap_name, 0) {
        Ok(image_id) => {
            if let Some(overlay) = ctx.images.get(&image_id).map(|image| (*image).clone()) {
                blend_tweet_overlay_fullscreen(&mut capture, &overlay);
            }
        }
        Err(err) => {
            // Original tnm_check_pct(..., false) simply skips a missing overlap
            // image. Keep that non-fatal behavior while making diagnostics
            // available in debug builds.
            log::debug!(
                "TWITTER.OVERLAP_IMAGE {:?} is not available; capturing without overlay: {err:#}",
                overlap_name
            );
        }
    }

    Ok(capture)
}

#[cfg(any(target_os = "macos", target_os = "windows", target_os = "linux"))]
fn open_tweet_dialog(ctx: &mut CommandContext) -> Result<()> {
    let Some(capture) = ctx.globals.capture_image.clone() else {
        log::error!("SYSCOM.OPEN_TWEET_DIALOG called without GLOBAL.CAPTURE_FOR_TWEET capture");
        return Ok(());
    };

    let out_path = save_dir(&ctx.project_dir).join("tweet.png");
    write_rgba_png_opaque(&out_path, &capture)?;
    crate::runtime::twitter::ensure_state_loaded(ctx);
    let initial_text = gameexe_unquoted_owned(ctx, "TWITTER.INITIAL_TWEET_TEXT");
    ctx.globals.twitter_dialog_request = Some(crate::runtime::twitter::TwitterDialogRequest {
        image_path: out_path.clone(),
        image_rgba: capture.rgba.clone(),
        image_width: capture.width,
        image_height: capture.height,
        initial_text,
    });
    ctx.globals
        .system
        .debug_logs
        .push(format!("open_tweet_dialog:{}", out_path.display()));
    Ok(())
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn open_tweet_dialog(_ctx: &mut CommandContext) -> Result<()> {
    log::error!("SYSCOM.OPEN_TWEET_DIALOG is not implemented on this platform");
    Ok(())
}

fn font_exists(project_dir: &Path, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if crate::text_render::font_name_matches_embedded_default(name) {
        return true;
    }

    let name_lower = name.to_ascii_lowercase();
    for font_dir in [project_dir.join("font"), project_dir.join("fonts")] {
        let Some(font_dir) = crate::resource::resolve_game_path(&font_dir).ok().flatten() else {
            continue;
        };
        let Ok(entries) = fs::read_dir(font_dir) else {
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
            if ext != "ttf" && ext != "otf" && ext != "ttc" {
                continue;
            }
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if stem == name_lower {
                return true;
            }
        }
    }
    false
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let Some(call) = parse_call(ctx, form_id, args) else {
        return Ok(false);
    };
    let op = call.op;
    let params = call.params;

    // eng_syscom.cpp::tnm_syscom_read_skip_is_enable() also gates read skip
    // on the current message's committed read flag unless unread-skip is on.
    if op == CHECK_READ_SKIP_ENABLE {
        ctx.push(Value::Int(if ctx.runtime_read_skip_is_enable() { 1 } else { 0 }));
        return Ok(true);
    }
    if matches!(
        op,
        SET_READ_SKIP_ONOFF_FLAG | SET_READ_SKIP_ENABLE_FLAG | SET_READ_SKIP_EXIST_FLAG
    ) {
        let value = p_bool(params, 0);
        match op {
            SET_READ_SKIP_ONOFF_FLAG => ctx.globals.syscom.read_skip.onoff = value,
            SET_READ_SKIP_ENABLE_FLAG => ctx.globals.syscom.read_skip.enable = value,
            SET_READ_SKIP_EXIST_FLAG => ctx.globals.syscom.read_skip.exist = value,
            _ => unreachable!(),
        }
        ctx.runtime_update_read_skip_menu();
        ctx.push(Value::Int(0));
        return Ok(true);
    }

    {
        let st = &ctx.globals.syscom;
        if let Some(v) = get_local_extra(op, params, st) {
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        if let Some(v) = get_toggle_get(op, st) {
            ctx.push(Value::Int(v));
            return Ok(true);
        }
    }
    {
        let st = &mut ctx.globals.syscom;
        if set_local_extra(op, params, st) {
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        if apply_toggle_set(op, p_bool(params, 0), st) {
            ctx.push(Value::Int(0));
            return Ok(true);
        }
    }

    match op {
        CALL_EX => {
            // C++ routes SYSCOM.CALL_EX to tnm_command_proc_farcall_ex(..., FM_VOID).
            // That transfer is implemented in the VM before generic form dispatch,
            // because only the VM owns the script call stack and proc boundary.
            // Do not fake a return value here.
            return Ok(false);
        }
        CALL_SYSCOM_MENU => {
            ctx.globals.syscom.menu_open = false;
            ctx.globals.syscom.menu_kind = None;
            ctx.globals.syscom.menu_result = None;
            ctx.globals.syscom.read_skip.onoff = false;
            ctx.globals.syscom.pending_proc = Some(SyscomPendingProc {
                kind: SyscomPendingProcKind::OpenSyscomMenu,
                warning: false,
                se_play: false,
                fade_out: false,
                leave_msgbk: false,
                save_id: 0,
            });
            ctx.globals.syscom.last_menu_call = CALL_SYSCOM_MENU;
            return Ok(true);
        }
        SET_SYSCOM_MENU_ENABLE => ctx.globals.syscom.syscom_menu_disable = false,
        SET_SYSCOM_MENU_DISABLE => ctx.globals.syscom.syscom_menu_disable = true,
        SET_MWND_BTN_ENABLE => {
            if params.is_empty() {
                ctx.globals.syscom.mwnd_btn_disable_all = false;
                if sg_debug_enabled_local() {
                    eprintln!("[SG_DEBUG][BUTTON_TRACE][SYSCOM] SET_MWND_BTN_ENABLE all disable_all=false");
                }
            } else {
                let idx = p_i64(params, 0);
                ctx.globals.syscom.mwnd_btn_disable.insert(idx, false);
                if sg_debug_enabled_local() {
                    eprintln!("[SG_DEBUG][BUTTON_TRACE][SYSCOM] SET_MWND_BTN_ENABLE idx={} disabled=false", idx);
                }
            }
        }
        SET_MWND_BTN_DISABLE => {
            if params.is_empty() {
                ctx.globals.syscom.mwnd_btn_disable_all = true;
                if sg_debug_enabled_local() {
                    eprintln!("[SG_DEBUG][BUTTON_TRACE][SYSCOM] SET_MWND_BTN_DISABLE all disable_all=true");
                }
            } else {
                let idx = p_i64(params, 0);
                ctx.globals.syscom.mwnd_btn_disable.insert(idx, true);
                if sg_debug_enabled_local() {
                    eprintln!("[SG_DEBUG][BUTTON_TRACE][SYSCOM] SET_MWND_BTN_DISABLE idx={} disabled=true", idx);
                }
            }
        }
        SET_MWND_BTN_TOUCH_ENABLE => {
            ctx.globals.syscom.mwnd_btn_touch_disable = false;
            if sg_debug_enabled_local() {
                eprintln!("[SG_DEBUG][BUTTON_TRACE][SYSCOM] SET_MWND_BTN_TOUCH_ENABLE touch_disable=false");
            }
        }
        SET_MWND_BTN_TOUCH_DISABLE => {
            ctx.globals.syscom.mwnd_btn_touch_disable = true;
            if sg_debug_enabled_local() {
                eprintln!("[SG_DEBUG][BUTTON_TRACE][SYSCOM] SET_MWND_BTN_TOUCH_DISABLE touch_disable=true");
            }
        }
        INIT_SYSCOM_FLAG => {
            let enabled = ToggleFeatureState { onoff: false, enable: true, exist: true };
            ctx.globals.syscom.read_skip = enabled;
            ctx.globals.syscom.auto_skip = enabled;
            ctx.globals.syscom.auto_mode = enabled;
            ctx.globals.syscom.hide_mwnd = enabled;
            ctx.globals.syscom.local_extra_switch = enabled;
            ctx.globals.syscom.local_extra_mode = ValueFeatureState { value: 0, enable: true, exist: true };
            ctx.globals.syscom.local_extra_switches = [enabled; 4];
            ctx.globals.syscom.local_extra_modes = [ValueFeatureState { value: 0, enable: true, exist: true }; 4];
            ctx.globals.syscom.msg_back = enabled;
            ctx.globals.syscom.return_to_sel = enabled;
            ctx.globals.syscom.return_to_menu = enabled;
            ctx.globals.syscom.end_game = enabled;
            ctx.globals.syscom.save_feature = enabled;
            ctx.globals.syscom.load_feature = enabled;
            ctx.globals.syscom.msg_back_open = false;
            ctx.runtime_update_read_skip_menu();
        }
        OPEN_MSG_BACK => {
            if open_msg_back_proc(ctx) {
                ctx.globals.syscom.pending_proc = Some(SyscomPendingProc {
                    kind: SyscomPendingProcKind::MsgBack,
                    warning: false,
                    se_play: false,
                    fade_out: false,
                    leave_msgbk: false,
                    save_id: 0,
                });
            }
            ctx.globals.syscom.last_menu_call = OPEN_MSG_BACK;
        }
        CLOSE_MSG_BACK => {
            ctx.globals.syscom.msg_back_open = false;
            ctx.globals.syscom.msg_back_proc_initialized = false;
            ctx.globals.syscom.last_menu_call = CLOSE_MSG_BACK;
        }
        RETURN_TO_SEL => {
            ctx.globals.syscom.pending_proc = Some(SyscomPendingProc {
                kind: SyscomPendingProcKind::ReturnToSel,
                warning: p_bool(params, 0),
                se_play: p_bool(params, 1),
                fade_out: p_bool(params, 2),
                leave_msgbk: false,
                save_id: 0,
            });
            ctx.globals.syscom.last_menu_call = RETURN_TO_SEL;
            ctx.globals.syscom.menu_open = false;
        }
        RETURN_TO_MENU => {
            let leave_msgbk = params
                .iter()
                .find(|v| v.named_id() == Some(0))
                .and_then(Value::as_i64)
                .unwrap_or(0)
                != 0;
            ctx.globals.syscom.pending_proc = Some(SyscomPendingProc {
                kind: SyscomPendingProcKind::ReturnToMenu,
                warning: p_bool(params, 0),
                se_play: p_bool(params, 1),
                fade_out: p_bool(params, 2),
                leave_msgbk,
                save_id: 0,
            });
            ctx.globals.syscom.last_menu_call = RETURN_TO_MENU;
            ctx.globals.syscom.menu_open = false;
        }
        END_GAME => {
            ctx.globals.syscom.pending_proc = Some(SyscomPendingProc {
                kind: SyscomPendingProcKind::EndGame,
                warning: p_bool(params, 0),
                se_play: p_bool(params, 1),
                fade_out: p_bool(params, 2),
                leave_msgbk: false,
                save_id: 0,
            });
            ctx.globals.syscom.last_menu_call = END_GAME;
            ctx.globals.syscom.menu_open = false;
        }
        REPLAY_KOE => {
            let koe_no = ctx.globals.script.cur_koe_no;
            let chara_no = ctx.globals.script.cur_chr_no;
            if koe_no >= 0 {
                // Replay is a new normal C_elm_koe playback. Re-establish the
                // actual buffer metadata so character routing and BGMFADE2 do
                // not inherit a preceding EXKOE/other-character voice.
                crate::runtime::forms::global::remember_global_koe(
                    ctx, koe_no, chara_no, false,
                );
                let append_dir = ctx.globals.append_dir.clone();
                let jitan_rate = ctx.koe_jitan_rate(None, true);
                if let Err(err) = {
                    let (koe, audio) = (&mut ctx.koe, &mut ctx.audio);
                    koe.play_koe_no_with_rate(audio, koe_no, &append_dir, jitan_rate)
                } {
                    log::error!(
                        "SYSCOM.REPLAY_KOE failed koe_no={koe_no} chara_no={chara_no}: {err:#}"
                    );
                }
                ctx.globals.syscom.replay_koe = Some((koe_no, chara_no));
            }
        }
        CHECK_REPLAY_KOE => {
            let v = if ctx.globals.script.cur_koe_no >= 0 {
                1
            } else {
                0
            };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_REPLAY_KOE_KOE_NO => {
            let v = ctx.globals.script.cur_koe_no;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_REPLAY_KOE_CHARA_NO => {
            let v = ctx.globals.script.cur_chr_no;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        CLEAR_REPLAY_KOE => {
            ctx.globals.script.cur_koe_no = -1;
            ctx.globals.script.cur_chr_no = -1;
            ctx.globals.syscom.replay_koe = None;
        }
        GET_CURRENT_SAVE_SCENE_TITLE => {
            let v = ctx.globals.syscom.current_save_scene_title.clone();
            ctx.push(Value::Str(v));
            return Ok(true);
        }
        GET_CURRENT_SAVE_MESSAGE => {
            let v = ctx.globals.syscom.current_save_message.clone();
            ctx.push(Value::Str(v));
            return Ok(true);
        }
        GET_TOTAL_PLAY_TIME => {
            let v = ctx.globals.syscom.total_play_time;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_TOTAL_PLAY_TIME => {
            ctx.globals.syscom.total_play_time = p_i64(params, 0);
            write_global_save(ctx);
        },
        CALL_SAVE_MENU => {
            prepare_runtime_save_thumb_capture_with_priority(ctx, CAPTURE_PRIOR_SAVE);
            set_syscom_pending_proc(ctx, SyscomPendingProcKind::OpenSave);
            ctx.globals.syscom.last_menu_call = CALL_SAVE_MENU;
            return Ok(true);
        }
        CALL_LOAD_MENU => {
            set_syscom_pending_proc(ctx, SyscomPendingProcKind::OpenLoad);
            ctx.globals.syscom.last_menu_call = CALL_LOAD_MENU;
            return Ok(true);
        }
        SAVE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let warning = p_bool(params, 1);
            let ok = idx < configured_save_count(ctx, false) && local_save_available(ctx);
            if ok && !request_confirmed_save_or_load(ctx, SyscomPendingProcKind::Save, idx, warning, true) {
                if ok {
                    menu_save_slot(ctx, false, idx);
                }
            }
            ctx.globals.syscom.last_menu_call = SAVE;
            ctx.push(Value::Int(if ok { 1 } else { 0 }));
            return Ok(true);
        }
        LOAD => {
            let idx = p_i64(params, 0).max(0) as usize;
            let warning = p_bool(params, 1);
            let ok = idx < configured_save_count(ctx, false)
                && local_save_file_exists(ctx, SaveKind::Normal, idx);
            if ok && !request_confirmed_save_or_load(ctx, SyscomPendingProcKind::Load, idx, warning, false) {
                menu_load_slot(ctx, false, idx);
            }
            ctx.globals.syscom.last_menu_call = LOAD;
        }
        QUICK_SAVE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let warning = p_bool(params, 1);
            let ok = idx < configured_save_count(ctx, true) && local_save_available(ctx);
            if ok && !request_confirmed_save_or_load(ctx, SyscomPendingProcKind::QuickSave, idx, warning, true) {
                if ok {
                    menu_save_slot(ctx, true, idx);
                }
            }
            ctx.globals.syscom.last_menu_call = QUICK_SAVE;
            ctx.push(Value::Int(if ok { 1 } else { 0 }));
            return Ok(true);
        }
        QUICK_LOAD => {
            let idx = p_i64(params, 0).max(0) as usize;
            let warning = p_bool(params, 1);
            let ok = idx < configured_save_count(ctx, true)
                && local_save_file_exists(ctx, SaveKind::Quick, idx);
            if ok && !request_confirmed_save_or_load(ctx, SyscomPendingProcKind::QuickLoad, idx, warning, false) {
                menu_load_slot(ctx, true, idx);
            }
            ctx.globals.syscom.last_menu_call = QUICK_LOAD;
        }
        END_SAVE => {
            // C++ `END_SAVE(warning, se_play)` always targets
            // save_cnt + quick_save_cnt; it has no script-supplied index.
            let _warning = p_bool(params, 0);
            let ok = local_save_available(ctx);
            if ok {
                prepare_runtime_save_thumb_capture_with_priority(ctx, CAPTURE_PRIOR_END);
                ctx.request_runtime_save(RuntimeSaveKind::End, 0);
            }
            ctx.push(Value::Int(if ok { 1 } else { 0 }));
            return Ok(true);
        }
        END_LOAD => {
            // C++ `END_LOAD(warning, se_play, fade_out)` loads the single end-save
            // slot at save_cnt + quick_save_cnt.
            if local_save_file_exists(ctx, SaveKind::End, 0) {
                ctx.request_runtime_load(RuntimeSaveKind::End, 0);
            }
            ctx.globals.syscom.last_menu_call = END_LOAD;
        },
        INNER_SAVE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let ok = local_save_available(ctx);
            if ok {
                ctx.request_runtime_save(RuntimeSaveKind::Inner, idx);
            }
            ctx.push(Value::Int(if ok { 1 } else { 0 }));
            return Ok(true);
        }
        INNER_LOAD => {
            let idx = p_i64(params, 0).max(0) as usize;
            ctx.globals.syscom.last_menu_call = INNER_LOAD;
            let exists = ctx
                .globals
                .syscom
                .inner_save_streams
                .get(idx)
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            if exists {
                ctx.request_runtime_load(RuntimeSaveKind::Inner, idx);
            }
            ctx.push(Value::Int(if exists { 1 } else { 0 }));
            return Ok(true);
        }
        CLEAR_INNER_SAVE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let existed = ctx
                .globals
                .syscom
                .inner_save_streams
                .get(idx)
                .map(|s| !s.is_empty())
                .unwrap_or(false);
            if let Some(slot) = ctx.globals.syscom.inner_save_streams.get_mut(idx) {
                slot.clear();
            }
            ctx.globals.syscom.inner_save_exists = ctx.globals.syscom.inner_save_streams.iter().any(|s| !s.is_empty());
            ctx.push(Value::Int(if existed { 1 } else { 0 }));
            return Ok(true);
        }
        COPY_INNER_SAVE => {
            let from = p_i64(params, 0).max(0) as usize;
            let to = p_i64(params, 1).max(0) as usize;
            let Some(stream) = ctx.globals.syscom.inner_save_streams.get(from).cloned().filter(|s| !s.is_empty()) else {
                ctx.push(Value::Int(0));
                return Ok(true);
            };
            if ctx.globals.syscom.inner_save_streams.len() <= to {
                ctx.globals.syscom.inner_save_streams.resize_with(to + 1, Vec::new);
            }
            ctx.globals.syscom.inner_save_streams[to] = stream;
            ctx.globals.syscom.inner_save_exists = true;
            ctx.push(Value::Int(1));
            return Ok(true);
        }
        CHECK_INNER_SAVE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let v = if ctx
                .globals
                .syscom
                .inner_save_streams
                .get(idx)
                .map(|s| !s.is_empty())
                .unwrap_or(false)
            {
                1
            } else {
                0
            };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        MSG_BACK_LOAD => {
            ctx.globals.syscom.pending_proc = Some(SyscomPendingProc {
                kind: SyscomPendingProcKind::BacklogLoad,
                warning: p_bool(params, 0),
                se_play: p_bool(params, 1),
                fade_out: p_bool(params, 2),
                leave_msgbk: false,
                save_id: ctx.globals.syscom.msg_back_load_tid,
            });
            ctx.globals.syscom.last_menu_call = MSG_BACK_LOAD;
            ctx.globals.syscom.msg_back_open = false;
            write_msg_back(ctx);
        }
        GET_SAVE_CNT => {
            let v = configured_save_count(ctx, false) as i64;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_QUICK_SAVE_CNT => {
            let v = configured_save_count(ctx, true) as i64;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_SAVE_NEW_NO => {
            sync_save_slots_from_disk(ctx, false);
            let save_cnt = configured_save_count(ctx, false);
            let v = match params.len() {
                0 => newest_slot_in_range(
                    &ctx.globals.syscom.save_slots,
                    0,
                    save_cnt as i64,
                    save_cnt,
                ),
                2 => newest_slot_in_range(
                    &ctx.globals.syscom.save_slots,
                    p_i64(params, 0),
                    p_i64(params, 1),
                    save_cnt,
                ),
                _ => -1,
            };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_QUICK_SAVE_NEW_NO => {
            sync_save_slots_from_disk(ctx, true);
            let quick_cnt = configured_save_count(ctx, true);
            let v = match params.len() {
                0 => newest_slot_in_range(
                    &ctx.globals.syscom.quick_save_slots,
                    0,
                    quick_cnt as i64,
                    quick_cnt,
                ),
                2 => newest_slot_in_range(
                    &ctx.globals.syscom.quick_save_slots,
                    p_i64(params, 0),
                    p_i64(params, 1),
                    quick_cnt,
                ),
                _ => -1,
            };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_SAVE_EXIST | GET_SAVE_YEAR | GET_SAVE_MONTH | GET_SAVE_DAY | GET_SAVE_WEEKDAY
        | GET_SAVE_HOUR | GET_SAVE_MINUTE | GET_SAVE_SECOND | GET_SAVE_MILLISECOND => {
            let raw_idx = p_i64(params, 0);
            let save_cnt = configured_save_count(ctx, false);
            if raw_idx < 0 || raw_idx as usize >= save_cnt {
                ctx.push(Value::Int(0));
                return Ok(true);
            }
            let idx = raw_idx as usize;
            {
                let quick_cnt = configured_save_count(ctx, true);
                ensure_slot_loaded_with_counts(
                    &ctx.project_dir,
                    false,
                    save_cnt,
                    quick_cnt,
                    &mut ctx.globals.syscom.save_slots,
                    idx,
                );
            }
            let v = ctx
                .globals
                .syscom
                .save_slots
                .get(idx)
                .map(|s| slot_i64(s, op))
                .unwrap_or(0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_SAVE_TITLE
        | GET_SAVE_MESSAGE
        | GET_SAVE_FULL_MESSAGE
        | GET_SAVE_COMMENT
        | GET_SAVE_APPEND_DIR
        | GET_SAVE_APPEND_NAME => {
            let raw_idx = p_i64(params, 0);
            let save_cnt = configured_save_count(ctx, false);
            if raw_idx < 0 || raw_idx as usize >= save_cnt {
                ctx.push(Value::Str(String::new()));
                return Ok(true);
            }
            let idx = raw_idx as usize;
            {
                let quick_cnt = configured_save_count(ctx, true);
                ensure_slot_loaded_with_counts(
                    &ctx.project_dir,
                    false,
                    save_cnt,
                    quick_cnt,
                    &mut ctx.globals.syscom.save_slots,
                    idx,
                );
            }
            let v = ctx
                .globals
                .syscom
                .save_slots
                .get(idx)
                .map(|s| slot_str(s, op))
                .unwrap_or_default();
            ctx.push(Value::Str(v));
            return Ok(true);
        }
        SET_SAVE_COMMENT => {
            let raw_idx = p_i64(params, 0);
            let save_cnt = configured_save_count(ctx, false);
            if raw_idx < 0 || raw_idx as usize >= save_cnt {
                return Ok(true);
            }
            let idx = raw_idx as usize;
            let quick_cnt = configured_save_count(ctx, true);
            ensure_slot_loaded_with_counts(
                &ctx.project_dir,
                false,
                save_cnt,
                quick_cnt,
                &mut ctx.globals.syscom.save_slots,
                idx,
            );
            // Original SET_SAVE_COMMENT first calls tnm_load_save_header(); if
            // the slot does not exist it is a no-op and must not create a save.
            if !ctx
                .globals
                .syscom
                .save_slots
                .get(idx)
                .map(|slot| slot.exist)
                .unwrap_or(false)
            {
                return Ok(true);
            }
            let comment = params
                .get(1)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            ctx.globals.syscom.save_slots[idx].comment = comment;
            persist_slot_with_counts(
                &ctx.project_dir,
                false,
                save_cnt,
                quick_cnt,
                &mut ctx.globals.syscom.save_slots,
                idx,
            );
            return Ok(true);
        }
        GET_SAVE_VALUE => {
            let raw_idx = p_i64(params, 0);
            let save_cnt = configured_save_count(ctx, false);
            if raw_idx < 0 || raw_idx as usize >= save_cnt {
                return Ok(true);
            }
            let idx = raw_idx as usize;
            let quick_cnt = configured_save_count(ctx, true);
            ensure_slot_loaded_with_counts(
                &ctx.project_dir,
                false,
                save_cnt,
                quick_cnt,
                &mut ctx.globals.syscom.save_slots,
                idx,
            );
            let Some(slot) = ctx.globals.syscom.save_slots.get(idx).filter(|slot| slot.exist) else {
                return Ok(true);
            };
            let Some(Value::Element(chain)) = params.get(1).map(|v| v.unwrap_named()) else {
                return Ok(true);
            };
            let Some(form_id) = chain.first().copied() else {
                return Ok(true);
            };
            let flag_index = p_i64(params, 2).max(0) as usize;
            let flag_cnt = (p_i64(params, 3).max(0) as usize).min(original_save::SAVE_FLAG_MAX_CNT);
            let values: Vec<i64> = (0..flag_cnt)
                .map(|i| slot.values.get(&(i as i32)).copied().unwrap_or(0))
                .collect();
            let list = ctx.globals.int_lists.entry(form_id as u32).or_default();
            if list.len() < flag_index + flag_cnt {
                list.resize(flag_index + flag_cnt, 0);
            }
            for (i, value) in values.into_iter().enumerate() {
                list[flag_index + i] = value;
            }
            // GET_SAVE_VALUE is FM_VOID in def_element_Siglus.h. It writes the
            // destination int-list and does not push a return value.
            return Ok(true);
        }
        SET_SAVE_VALUE => {
            let raw_idx = p_i64(params, 0);
            let save_cnt = configured_save_count(ctx, false);
            if raw_idx < 0 || raw_idx as usize >= save_cnt {
                return Ok(true);
            }
            let idx = raw_idx as usize;
            let quick_cnt = configured_save_count(ctx, true);
            ensure_slot_loaded_with_counts(
                &ctx.project_dir,
                false,
                save_cnt,
                quick_cnt,
                &mut ctx.globals.syscom.save_slots,
                idx,
            );
            if !ctx
                .globals
                .syscom
                .save_slots
                .get(idx)
                .map(|slot| slot.exist)
                .unwrap_or(false)
            {
                return Ok(true);
            }
            let Some(Value::Element(chain)) = params.get(1).map(|v| v.unwrap_named()) else {
                return Ok(true);
            };
            let Some(form_id) = chain.first().copied() else {
                return Ok(true);
            };
            let flag_index = p_i64(params, 2).max(0) as usize;
            let flag_cnt = (p_i64(params, 3).max(0) as usize).min(original_save::SAVE_FLAG_MAX_CNT);
            let values: Vec<i64> = (0..flag_cnt)
                .map(|i| {
                    ctx.globals
                        .int_lists
                        .get(&(form_id as u32))
                        .and_then(|list| list.get(flag_index + i).copied())
                        .unwrap_or(0)
                })
                .collect();
            let slot = &mut ctx.globals.syscom.save_slots[idx];
            for (i, value) in values.into_iter().enumerate() {
                if value == 0 {
                    slot.values.remove(&(i as i32));
                } else {
                    slot.values.insert(i as i32, value);
                }
            }
            persist_slot_with_counts(
                &ctx.project_dir,
                false,
                save_cnt,
                quick_cnt,
                &mut ctx.globals.syscom.save_slots,
                idx,
            );
            return Ok(true);
        }
        GET_QUICK_SAVE_EXIST
        | GET_QUICK_SAVE_YEAR
        | GET_QUICK_SAVE_MONTH
        | GET_QUICK_SAVE_DAY
        | GET_QUICK_SAVE_WEEKDAY
        | GET_QUICK_SAVE_HOUR
        | GET_QUICK_SAVE_MINUTE
        | GET_QUICK_SAVE_SECOND
        | GET_QUICK_SAVE_MILLISECOND => {
            let raw_idx = p_i64(params, 0);
            let quick_cnt = configured_save_count(ctx, true);
            if raw_idx < 0 || raw_idx as usize >= quick_cnt {
                ctx.push(Value::Int(0));
                return Ok(true);
            }
            let idx = raw_idx as usize;
            {
                let save_cnt = configured_save_count(ctx, false);
                ensure_slot_loaded_with_counts(
                    &ctx.project_dir,
                    true,
                    save_cnt,
                    quick_cnt,
                    &mut ctx.globals.syscom.quick_save_slots,
                    idx,
                );
            }
            let v = ctx
                .globals
                .syscom
                .quick_save_slots
                .get(idx)
                .map(|s| slot_i64(s, op))
                .unwrap_or(0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_QUICK_SAVE_TITLE
        | GET_QUICK_SAVE_MESSAGE
        | GET_QUICK_SAVE_FULL_MESSAGE
        | GET_QUICK_SAVE_COMMENT
        | GET_QUICK_SAVE_APPEND_DIR
        | GET_QUICK_SAVE_APPEND_NAME => {
            let raw_idx = p_i64(params, 0);
            let quick_cnt = configured_save_count(ctx, true);
            if raw_idx < 0 || raw_idx as usize >= quick_cnt {
                ctx.push(Value::Str(String::new()));
                return Ok(true);
            }
            let idx = raw_idx as usize;
            {
                let save_cnt = configured_save_count(ctx, false);
                ensure_slot_loaded_with_counts(
                    &ctx.project_dir,
                    true,
                    save_cnt,
                    quick_cnt,
                    &mut ctx.globals.syscom.quick_save_slots,
                    idx,
                );
            }
            let v = ctx
                .globals
                .syscom
                .quick_save_slots
                .get(idx)
                .map(|s| slot_str(s, op))
                .unwrap_or_default();
            ctx.push(Value::Str(v));
            return Ok(true);
        }
        SET_QUICK_SAVE_COMMENT => {
            let raw_idx = p_i64(params, 0);
            let quick_cnt = configured_save_count(ctx, true);
            if raw_idx < 0 || raw_idx as usize >= quick_cnt {
                return Ok(true);
            }
            let idx = raw_idx as usize;
            let save_cnt = configured_save_count(ctx, false);
            ensure_slot_loaded_with_counts(
                &ctx.project_dir,
                true,
                save_cnt,
                quick_cnt,
                &mut ctx.globals.syscom.quick_save_slots,
                idx,
            );
            if !ctx
                .globals
                .syscom
                .quick_save_slots
                .get(idx)
                .map(|slot| slot.exist)
                .unwrap_or(false)
            {
                return Ok(true);
            }
            let comment = params
                .get(1)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            ctx.globals.syscom.quick_save_slots[idx].comment = comment;
            persist_slot_with_counts(
                &ctx.project_dir,
                true,
                save_cnt,
                quick_cnt,
                &mut ctx.globals.syscom.quick_save_slots,
                idx,
            );
            return Ok(true);
        }
        GET_QUICK_SAVE_VALUE => {
            let raw_idx = p_i64(params, 0);
            let quick_cnt = configured_save_count(ctx, true);
            if raw_idx < 0 || raw_idx as usize >= quick_cnt {
                return Ok(true);
            }
            let idx = raw_idx as usize;
            let save_cnt = configured_save_count(ctx, false);
            ensure_slot_loaded_with_counts(
                &ctx.project_dir,
                true,
                save_cnt,
                quick_cnt,
                &mut ctx.globals.syscom.quick_save_slots,
                idx,
            );
            let Some(slot) = ctx
                .globals
                .syscom
                .quick_save_slots
                .get(idx)
                .filter(|slot| slot.exist)
            else {
                return Ok(true);
            };
            let Some(Value::Element(chain)) = params.get(1).map(|v| v.unwrap_named()) else {
                return Ok(true);
            };
            let Some(form_id) = chain.first().copied() else {
                return Ok(true);
            };
            let flag_index = p_i64(params, 2).max(0) as usize;
            let flag_cnt = (p_i64(params, 3).max(0) as usize).min(original_save::SAVE_FLAG_MAX_CNT);
            let values: Vec<i64> = (0..flag_cnt)
                .map(|i| slot.values.get(&(i as i32)).copied().unwrap_or(0))
                .collect();
            let list = ctx.globals.int_lists.entry(form_id as u32).or_default();
            if list.len() < flag_index + flag_cnt {
                list.resize(flag_index + flag_cnt, 0);
            }
            for (i, value) in values.into_iter().enumerate() {
                list[flag_index + i] = value;
            }
            return Ok(true);
        }
        SET_QUICK_SAVE_VALUE => {
            let raw_idx = p_i64(params, 0);
            let quick_cnt = configured_save_count(ctx, true);
            if raw_idx < 0 || raw_idx as usize >= quick_cnt {
                return Ok(true);
            }
            let idx = raw_idx as usize;
            let save_cnt = configured_save_count(ctx, false);
            ensure_slot_loaded_with_counts(
                &ctx.project_dir,
                true,
                save_cnt,
                quick_cnt,
                &mut ctx.globals.syscom.quick_save_slots,
                idx,
            );
            if !ctx
                .globals
                .syscom
                .quick_save_slots
                .get(idx)
                .map(|slot| slot.exist)
                .unwrap_or(false)
            {
                return Ok(true);
            }
            let Some(Value::Element(chain)) = params.get(1).map(|v| v.unwrap_named()) else {
                return Ok(true);
            };
            let Some(form_id) = chain.first().copied() else {
                return Ok(true);
            };
            let flag_index = p_i64(params, 2).max(0) as usize;
            let flag_cnt = (p_i64(params, 3).max(0) as usize).min(original_save::SAVE_FLAG_MAX_CNT);
            let values: Vec<i64> = (0..flag_cnt)
                .map(|i| {
                    ctx.globals
                        .int_lists
                        .get(&(form_id as u32))
                        .and_then(|list| list.get(flag_index + i).copied())
                        .unwrap_or(0)
                })
                .collect();
            let slot = &mut ctx.globals.syscom.quick_save_slots[idx];
            for (i, value) in values.into_iter().enumerate() {
                if value == 0 {
                    slot.values.remove(&(i as i32));
                } else {
                    slot.values.insert(i as i32, value);
                }
            }
            persist_slot_with_counts(
                &ctx.project_dir,
                true,
                save_cnt,
                quick_cnt,
                &mut ctx.globals.syscom.quick_save_slots,
                idx,
            );
            return Ok(true);
        }
        GET_END_SAVE_EXIST => {
            let save_cnt = configured_save_count(ctx, false);
            let quick_cnt = configured_save_count(ctx, true);
            let end_path = original_save::save_file_path_with_counts(
                &ctx.project_dir,
                save_cnt,
                quick_cnt,
                SaveKind::End,
                0,
            );
            let v = if ctx.globals.syscom.end_save_exists || crate::resource::game_file_exists(&end_path) { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        COPY_SAVE => {
            let src = p_i64(params, 0).max(0) as usize;
            let dst = p_i64(params, 1).max(0) as usize;
            let save_cnt = configured_save_count(ctx, false);
            let quick_cnt = configured_save_count(ctx, true);
            let thumb_config = save_thumb_config(ctx);
            let ok = copy_slot(
                &ctx.project_dir,
                false,
                save_cnt,
                quick_cnt,
                thumb_config,
                &mut ctx.globals.syscom.save_slots,
                src,
                dst,
            );
            ctx.globals.syscom.last_menu_call = op;
            ctx.push(Value::Int(if ok { 1 } else { 0 }));
            return Ok(true);
        }
        COPY_QUICK_SAVE => {
            let src = p_i64(params, 0).max(0) as usize;
            let dst = p_i64(params, 1).max(0) as usize;
            let save_cnt = configured_save_count(ctx, false);
            let quick_cnt = configured_save_count(ctx, true);
            let thumb_config = save_thumb_config(ctx);
            let ok = copy_slot(
                &ctx.project_dir,
                true,
                save_cnt,
                quick_cnt,
                thumb_config,
                &mut ctx.globals.syscom.quick_save_slots,
                src,
                dst,
            );
            ctx.globals.syscom.last_menu_call = op;
            ctx.push(Value::Int(if ok { 1 } else { 0 }));
            return Ok(true);
        }
        CHANGE_SAVE => {
            let a = p_i64(params, 0).max(0) as usize;
            let b = p_i64(params, 1).max(0) as usize;
            let save_cnt = configured_save_count(ctx, false);
            let quick_cnt = configured_save_count(ctx, true);
            let thumb_config = save_thumb_config(ctx);
            let ok = change_slot(
                &ctx.project_dir,
                false,
                save_cnt,
                quick_cnt,
                thumb_config,
                &mut ctx.globals.syscom.save_slots,
                a,
                b,
            );
            ctx.globals.syscom.last_menu_call = op;
            ctx.push(Value::Int(if ok { 1 } else { 0 }));
            return Ok(true);
        }
        CHANGE_QUICK_SAVE => {
            let a = p_i64(params, 0).max(0) as usize;
            let b = p_i64(params, 1).max(0) as usize;
            let save_cnt = configured_save_count(ctx, false);
            let quick_cnt = configured_save_count(ctx, true);
            let thumb_config = save_thumb_config(ctx);
            let ok = change_slot(
                &ctx.project_dir,
                true,
                save_cnt,
                quick_cnt,
                thumb_config,
                &mut ctx.globals.syscom.quick_save_slots,
                a,
                b,
            );
            ctx.globals.syscom.last_menu_call = op;
            ctx.push(Value::Int(if ok { 1 } else { 0 }));
            return Ok(true);
        }
        DELETE_SAVE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let save_cnt = configured_save_count(ctx, false);
            let quick_cnt = configured_save_count(ctx, true);
            let thumb_config = save_thumb_config(ctx);
            let ok = delete_slot(
                &ctx.project_dir,
                false,
                save_cnt,
                quick_cnt,
                thumb_config,
                &mut ctx.globals.syscom.save_slots,
                idx,
            );
            ctx.globals.syscom.last_menu_call = op;
            ctx.push(Value::Int(if ok { 1 } else { 0 }));
            return Ok(true);
        }
        DELETE_QUICK_SAVE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let save_cnt = configured_save_count(ctx, false);
            let quick_cnt = configured_save_count(ctx, true);
            let thumb_config = save_thumb_config(ctx);
            let ok = delete_slot(
                &ctx.project_dir,
                true,
                save_cnt,
                quick_cnt,
                thumb_config,
                &mut ctx.globals.syscom.quick_save_slots,
                idx,
            );
            ctx.globals.syscom.last_menu_call = op;
            ctx.push(Value::Int(if ok { 1 } else { 0 }));
            return Ok(true);
        }
        CALL_CONFIG_MENU => {
            set_syscom_pending_proc(ctx, SyscomPendingProcKind::OpenConfig);
            ctx.globals.syscom.last_menu_call = op;
        }
        CALL_CONFIG_WINDOW_MODE_MENU
        | CALL_CONFIG_VOLUME_MENU
        | CALL_CONFIG_BGMFADE_MENU
        | CALL_CONFIG_KOEMODE_MENU
        | CALL_CONFIG_CHARAKOE_MENU
        | CALL_CONFIG_JITAN_MENU
        | CALL_CONFIG_MESSAGE_SPEED_MENU
        | CALL_CONFIG_FILTER_COLOR_MENU
        | CALL_CONFIG_AUTO_MODE_MENU
        | CALL_CONFIG_FONT_MENU
        | CALL_CONFIG_SYSTEM_MENU
        | CALL_CONFIG_MOVIE_MENU => {
            // C++ cmd_syscom opens cfg_wnd_solo_* directly for these calls.
            // Re-entering CONFIG_SCENE would share the active EXCALL storage;
            // returning from the inner menu frees the outer menu's objects.
            set_syscom_pending_proc(ctx, SyscomPendingProcKind::OpenConfigDialog);
            ctx.globals.syscom.last_menu_call = op;
        }
        SET_WINDOW_MODE => cfg_set_int(&mut ctx.globals.syscom, GET_WINDOW_MODE, p_i64(params, 0)),
        SET_WINDOW_MODE_DEFAULT => {
            let value = gameexe_i64_or(ctx, "CONFIG.WINDOW_MODE", 0).clamp(0, 1);
            cfg_set_int(&mut ctx.globals.syscom, GET_WINDOW_MODE, value);
        }
        GET_WINDOW_MODE => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_WINDOW_MODE, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_WINDOW_MODE_SIZE => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_WINDOW_MODE_SIZE,
            p_i64(params, 0),
        ),
        SET_WINDOW_MODE_SIZE_DEFAULT => {
            let value = ctx.globals.syscom.original_config.screen_size_scale.0.max(1);
            cfg_set_int(&mut ctx.globals.syscom, GET_WINDOW_MODE_SIZE, value);
        }
        GET_WINDOW_MODE_SIZE => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_WINDOW_MODE_SIZE, 100);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        CHECK_WINDOW_MODE_SIZE_ENABLE => {
            ctx.push(Value::Int(1));
            return Ok(true);
        }
        SET_ALL_VOLUME => {
            let value = p_i64(params, 0).clamp(0, 255);
            ctx.globals.syscom.original_config.all_sound_user_volume = value;
            cfg_set_int(&mut ctx.globals.syscom, GET_ALL_VOLUME, value);
            apply_audio_config(ctx);
        }
        SET_BGM_VOLUME => {
            set_sound_volume_by_type(ctx, 0, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_KOE_VOLUME => {
            set_sound_volume_by_type(ctx, 1, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_PCM_VOLUME => {
            set_sound_volume_by_type(ctx, 2, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_SE_VOLUME => {
            set_sound_volume_by_type(ctx, 3, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_MOV_VOLUME => {
            set_sound_volume_by_type(ctx, 4, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_SOUND_VOLUME => {
            let sound_type = p_i64(params, 0);
            if (0..32).contains(&sound_type) {
                set_sound_volume_by_type(ctx, sound_type as usize, p_i64(params, 1));
                apply_audio_config(ctx);
            }
        }
        SET_ALL_VOLUME_DEFAULT => {
            let value = gameexe_i64_or(ctx, "CONFIG.VOLUME.ALL", 255).clamp(0, 255);
            ctx.globals.syscom.original_config.all_sound_user_volume = value;
            cfg_set_int(&mut ctx.globals.syscom, GET_ALL_VOLUME, value);
            apply_audio_config(ctx);
        }
        SET_BGM_VOLUME_DEFAULT => {
            let value = config_default_sound_volume(ctx, 0);
            set_sound_volume_by_type(ctx, 0, value);
            apply_audio_config(ctx);
        }
        SET_KOE_VOLUME_DEFAULT => {
            let value = config_default_sound_volume(ctx, 1);
            set_sound_volume_by_type(ctx, 1, value);
            apply_audio_config(ctx);
        }
        SET_PCM_VOLUME_DEFAULT => {
            let value = config_default_sound_volume(ctx, 2);
            set_sound_volume_by_type(ctx, 2, value);
            apply_audio_config(ctx);
        }
        SET_SE_VOLUME_DEFAULT => {
            let value = config_default_sound_volume(ctx, 3);
            set_sound_volume_by_type(ctx, 3, value);
            apply_audio_config(ctx);
        }
        SET_MOV_VOLUME_DEFAULT => {
            let value = config_default_sound_volume(ctx, 4);
            set_sound_volume_by_type(ctx, 4, value);
            apply_audio_config(ctx);
        }
        SET_SOUND_VOLUME_DEFAULT => {
            let sound_type = p_i64(params, 0);
            if (0..32).contains(&sound_type) {
                let value = config_default_sound_volume(ctx, sound_type as usize);
                set_sound_volume_by_type(ctx, sound_type as usize, value);
                apply_audio_config(ctx);
            }
        }
        GET_ALL_VOLUME | GET_BGM_VOLUME | GET_KOE_VOLUME | GET_PCM_VOLUME | GET_SE_VOLUME
        | GET_MOV_VOLUME => {
            let v = cfg_get_int(&ctx.globals.syscom, op, 255).clamp(0, 255);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_SOUND_VOLUME => {
            let sound_type = p_i64(params, 0);
            let value = if (0..32).contains(&sound_type) {
                get_sound_volume_by_type(ctx, sound_type as usize)
            } else {
                255
            };
            ctx.push(Value::Int(value));
            return Ok(true);
        }
        SET_ALL_ONOFF => {
            let value = p_bool(params, 0);
            ctx.globals.syscom.original_config.play_all_sound_check = value;
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_ALL_ONOFF,
                if value { 1 } else { 0 },
            );
            apply_audio_config(ctx);
        }
        SET_BGM_ONOFF => {
            set_sound_onoff_by_type(ctx, 0, p_bool(params, 0));
            apply_audio_config(ctx);
        }
        SET_KOE_ONOFF => {
            set_sound_onoff_by_type(ctx, 1, p_bool(params, 0));
            apply_audio_config(ctx);
        }
        SET_PCM_ONOFF => {
            set_sound_onoff_by_type(ctx, 2, p_bool(params, 0));
            apply_audio_config(ctx);
        }
        SET_SE_ONOFF => {
            set_sound_onoff_by_type(ctx, 3, p_bool(params, 0));
            apply_audio_config(ctx);
        }
        SET_MOV_ONOFF => {
            set_sound_onoff_by_type(ctx, 4, p_bool(params, 0));
            apply_audio_config(ctx);
        }
        SET_SOUND_ONOFF => {
            let sound_type = p_i64(params, 0);
            if (0..32).contains(&sound_type) {
                set_sound_onoff_by_type(ctx, sound_type as usize, p_bool(params, 1));
                apply_audio_config(ctx);
            }
        }
        SET_ALL_ONOFF_DEFAULT => {
            ctx.globals.syscom.original_config.play_all_sound_check = true;
            cfg_set_int(&mut ctx.globals.syscom, GET_ALL_ONOFF, 1);
            apply_audio_config(ctx);
        }
        SET_BGM_ONOFF_DEFAULT => {
            let value = config_default_sound_onoff(ctx, 0);
            set_sound_onoff_by_type(ctx, 0, value);
            apply_audio_config(ctx);
        }
        SET_KOE_ONOFF_DEFAULT => {
            let value = config_default_sound_onoff(ctx, 1);
            set_sound_onoff_by_type(ctx, 1, value);
            apply_audio_config(ctx);
        }
        SET_PCM_ONOFF_DEFAULT => {
            let value = config_default_sound_onoff(ctx, 2);
            set_sound_onoff_by_type(ctx, 2, value);
            apply_audio_config(ctx);
        }
        SET_SE_ONOFF_DEFAULT => {
            let value = config_default_sound_onoff(ctx, 3);
            set_sound_onoff_by_type(ctx, 3, value);
            apply_audio_config(ctx);
        }
        SET_MOV_ONOFF_DEFAULT => {
            let value = config_default_sound_onoff(ctx, 4);
            set_sound_onoff_by_type(ctx, 4, value);
            apply_audio_config(ctx);
        }
        SET_SOUND_ONOFF_DEFAULT => {
            let sound_type = p_i64(params, 0);
            if (0..32).contains(&sound_type) {
                let value = config_default_sound_onoff(ctx, sound_type as usize);
                set_sound_onoff_by_type(ctx, sound_type as usize, value);
                apply_audio_config(ctx);
            }
        }
        GET_ALL_ONOFF | GET_BGM_ONOFF | GET_KOE_ONOFF | GET_PCM_ONOFF | GET_SE_ONOFF
        | GET_MOV_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, op, 1);
            ctx.push(Value::Int(if v != 0 { 1 } else { 0 }));
            return Ok(true);
        }
        GET_SOUND_ONOFF => {
            let sound_type = p_i64(params, 0);
            let value = if (0..32).contains(&sound_type) {
                get_sound_onoff_by_type(ctx, sound_type as usize)
            } else {
                true
            };
            ctx.push(Value::Int(if value { 1 } else { 0 }));
            return Ok(true);
        }
        SET_BGMFADE_VOLUME => {
            let value = p_i64(params, 0).clamp(0, 255);
            ctx.globals.syscom.original_config.bgmfade_volume = value;
            cfg_set_int(&mut ctx.globals.syscom, GET_BGMFADE_VOLUME, value);
            apply_audio_config(ctx);
        }
        SET_BGMFADE_ONOFF => {
            let value = p_bool(params, 0);
            ctx.globals.syscom.original_config.bgmfade_use_check = value;
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_BGMFADE_ONOFF,
                if value { 1 } else { 0 },
            );
            apply_audio_config(ctx);
        }
        SET_BGMFADE_VOLUME_DEFAULT => {
            let value = gameexe_i64_or(ctx, "CONFIG.BGMFADE_VOLUME", 192).clamp(0, 255);
            ctx.globals.syscom.original_config.bgmfade_volume = value;
            cfg_set_int(&mut ctx.globals.syscom, GET_BGMFADE_VOLUME, value);
            apply_audio_config(ctx);
        }
        SET_BGMFADE_ONOFF_DEFAULT => {
            let value = gameexe_bool_or(ctx, "CONFIG.BGMFADE_ONOFF", true);
            ctx.globals.syscom.original_config.bgmfade_use_check = value;
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_BGMFADE_ONOFF,
                if value { 1 } else { 0 },
            );
            apply_audio_config(ctx);
        }
        GET_BGMFADE_VOLUME | GET_BGMFADE_ONOFF => {
            let default = if op == GET_BGMFADE_ONOFF { 1 } else { 192 };
            let v = cfg_get_int(&ctx.globals.syscom, op, default);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_KOEMODE => {
            let value = match p_i64(params, 0) {
                0..=2 => p_i64(params, 0),
                _ => 0,
            };
            ctx.globals.syscom.original_config.koe_mode = value;
            cfg_set_int(&mut ctx.globals.syscom, GET_KOEMODE, value);
            apply_audio_config(ctx);
        }
        SET_KOEMODE_DEFAULT => {
            ctx.globals.syscom.original_config.koe_mode = 0;
            cfg_set_int(&mut ctx.globals.syscom, GET_KOEMODE, 0);
            apply_audio_config(ctx);
        }
        GET_KOEMODE => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_KOEMODE, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_CHARAKOE_ONOFF => {
            let index = p_i64(params, 0);
            let count = configured_chrkoe_count(ctx);
            ctx.globals.syscom.original_config.chrkoe.resize_with(
                count,
                crate::runtime::globals::ConfigChrKoeState::default,
            );
            if index >= 0 {
                if let Some(item) = ctx
                    .globals
                    .syscom
                    .original_config
                    .chrkoe
                    .get_mut(index as usize)
                {
                    item.onoff = p_bool(params, 1);
                }
            }
            apply_audio_config(ctx);
        }
        SET_CHARAKOE_ONOFF_DEFAULT => {
            let index = p_i64(params, 0);
            let count = configured_chrkoe_count(ctx);
            ctx.globals.syscom.original_config.chrkoe.resize_with(
                count,
                crate::runtime::globals::ConfigChrKoeState::default,
            );
            if index >= 0 && (index as usize) < count {
                let default = config_default_chrkoe(ctx, index as usize);
                if let Some(item) = ctx
                    .globals
                    .syscom
                    .original_config
                    .chrkoe
                    .get_mut(index as usize)
                {
                    item.onoff = default.onoff;
                }
            }
            apply_audio_config(ctx);
        }
        GET_CHARAKOE_ONOFF => {
            let index = p_i64(params, 0);
            let value = if index >= 0 {
                ctx.globals
                    .syscom
                    .original_config
                    .chrkoe
                    .get(index as usize)
                    .map(|item| item.onoff)
                    .unwrap_or(false)
            } else {
                false
            };
            ctx.push(Value::Int(if value { 1 } else { 0 }));
            return Ok(true);
        }
        SET_CHARAKOE_VOLUME => {
            let index = p_i64(params, 0);
            let count = configured_chrkoe_count(ctx);
            ctx.globals.syscom.original_config.chrkoe.resize_with(
                count,
                crate::runtime::globals::ConfigChrKoeState::default,
            );
            if index >= 0 {
                if let Some(item) = ctx
                    .globals
                    .syscom
                    .original_config
                    .chrkoe
                    .get_mut(index as usize)
                {
                    item.volume = p_i64(params, 1).clamp(0, 255);
                }
            }
            apply_audio_config(ctx);
        }
        SET_CHARAKOE_VOLUME_DEFAULT => {
            let index = p_i64(params, 0);
            let count = configured_chrkoe_count(ctx);
            ctx.globals.syscom.original_config.chrkoe.resize_with(
                count,
                crate::runtime::globals::ConfigChrKoeState::default,
            );
            if index >= 0 && (index as usize) < count {
                let default = config_default_chrkoe(ctx, index as usize);
                if let Some(item) = ctx
                    .globals
                    .syscom
                    .original_config
                    .chrkoe
                    .get_mut(index as usize)
                {
                    item.volume = default.volume;
                }
            }
            apply_audio_config(ctx);
        }
        GET_CHARAKOE_VOLUME => {
            let index = p_i64(params, 0);
            let value = if index >= 0 {
                ctx.globals
                    .syscom
                    .original_config
                    .chrkoe
                    .get(index as usize)
                    .map(|item| item.volume)
                    .unwrap_or(0)
            } else {
                0
            };
            ctx.push(Value::Int(value.clamp(0, 255)));
            return Ok(true);
        }
        SET_JITAN_NORMAL_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_JITAN_NORMAL_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_JITAN_NORMAL_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_JITAN_NORMAL_ONOFF, 0)
        }
        GET_JITAN_NORMAL_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_JITAN_NORMAL_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_JITAN_AUTO_MODE_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_JITAN_AUTO_MODE_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_JITAN_AUTO_MODE_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_JITAN_AUTO_MODE_ONOFF, 0)
        }
        GET_JITAN_AUTO_MODE_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_JITAN_AUTO_MODE_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_JITAN_KOE_REPLAY_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_JITAN_KOE_REPLAY_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_JITAN_KOE_REPLAY_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_JITAN_KOE_REPLAY_ONOFF, 0)
        }
        GET_JITAN_KOE_REPLAY_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_JITAN_KOE_REPLAY_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_JITAN_SPEED => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_JITAN_SPEED,
            p_i64(params, 0).clamp(100, 300),
        ),
        SET_JITAN_SPEED_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_JITAN_SPEED, 100),
        GET_JITAN_SPEED => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_JITAN_SPEED, 100);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_MESSAGE_SPEED => {
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_MESSAGE_SPEED,
                p_i64(params, 0).clamp(0, 100),
            )
        }
        SET_MESSAGE_SPEED_DEFAULT => {
            let value = gameexe_i64_or(ctx, "CONFIG.MESSAGE_SPEED", 20);
            cfg_set_int(&mut ctx.globals.syscom, GET_MESSAGE_SPEED, value);
        }
        GET_MESSAGE_SPEED => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_MESSAGE_SPEED, 20);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_MESSAGE_NOWAIT => {
            let v = p_bool(params, 0);
            ctx.globals.script.msg_nowait = v;
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_MESSAGE_NOWAIT,
                if v { 1 } else { 0 },
            );
        }
        SET_MESSAGE_NOWAIT_DEFAULT => {
            let value = gameexe_bool_or(ctx, "CONFIG.MESSAGE_SPEED_NOWAIT.ONOFF", false);
            ctx.globals.script.msg_nowait = value;
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_MESSAGE_NOWAIT,
                if value { 1 } else { 0 },
            );
        }
        GET_MESSAGE_NOWAIT => {
            let v = if ctx.globals.script.msg_nowait {
                1
            } else {
                cfg_get_int(&ctx.globals.syscom, GET_MESSAGE_NOWAIT, 0)
            };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_AUTO_MODE_MOJI_WAIT => {
            let v = p_i64(params, 0).clamp(0, 500);
            ctx.globals.script.auto_mode_moji_wait = v;
            cfg_set_int(&mut ctx.globals.syscom, GET_AUTO_MODE_MOJI_WAIT, v);
        }
        SET_AUTO_MODE_MOJI_WAIT_DEFAULT => {
            let value = 70;
            ctx.globals.script.auto_mode_moji_wait = value;
            cfg_set_int(&mut ctx.globals.syscom, GET_AUTO_MODE_MOJI_WAIT, value);
        }
        GET_AUTO_MODE_MOJI_WAIT => {
            let v = ctx.globals.script.auto_mode_moji_wait;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_AUTO_MODE_MIN_WAIT => {
            let v = p_i64(params, 0).clamp(0, 5000);
            ctx.globals.script.auto_mode_min_wait = v;
            cfg_set_int(&mut ctx.globals.syscom, GET_AUTO_MODE_MIN_WAIT, v);
        }
        SET_AUTO_MODE_MIN_WAIT_DEFAULT => {
            let value = 300;
            ctx.globals.script.auto_mode_min_wait = value;
            cfg_set_int(&mut ctx.globals.syscom, GET_AUTO_MODE_MIN_WAIT, value);
        }
        GET_AUTO_MODE_MIN_WAIT => {
            let v = ctx.globals.script.auto_mode_min_wait;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_MOUSE_CURSOR_HIDE_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_MOUSE_CURSOR_HIDE_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_MOUSE_CURSOR_HIDE_ONOFF_DEFAULT => {
            let v = config_mouse_cursor_hide_onoff_default(ctx);
            cfg_set_int(&mut ctx.globals.syscom, GET_MOUSE_CURSOR_HIDE_ONOFF, v)
        }
        GET_MOUSE_CURSOR_HIDE_ONOFF => {
            let v = cfg_get_int(
                &ctx.globals.syscom,
                GET_MOUSE_CURSOR_HIDE_ONOFF,
                config_mouse_cursor_hide_onoff_default(ctx),
            );
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_MOUSE_CURSOR_HIDE_TIME => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_MOUSE_CURSOR_HIDE_TIME,
            p_i64(params, 0),
        ),
        SET_MOUSE_CURSOR_HIDE_TIME_DEFAULT => {
            let v = config_mouse_cursor_hide_time_default(ctx);
            cfg_set_int(&mut ctx.globals.syscom, GET_MOUSE_CURSOR_HIDE_TIME, v)
        }
        GET_MOUSE_CURSOR_HIDE_TIME => {
            let v = cfg_get_int(
                &ctx.globals.syscom,
                GET_MOUSE_CURSOR_HIDE_TIME,
                config_mouse_cursor_hide_time_default(ctx),
            );
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_FILTER_COLOR_R => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_FILTER_COLOR_R,
            p_i64(params, 0),
        ),
        SET_FILTER_COLOR_G => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_FILTER_COLOR_G,
            p_i64(params, 0),
        ),
        SET_FILTER_COLOR_B => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_FILTER_COLOR_B,
            p_i64(params, 0),
        ),
        SET_FILTER_COLOR_A => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_FILTER_COLOR_A,
            p_i64(params, 0),
        ),
        SET_FILTER_COLOR_R_DEFAULT => {
            let (r, _, _, _) = config_filter_color_default(ctx);
            cfg_set_int(&mut ctx.globals.syscom, GET_FILTER_COLOR_R, r)
        }
        SET_FILTER_COLOR_G_DEFAULT => {
            let (_, g, _, _) = config_filter_color_default(ctx);
            cfg_set_int(&mut ctx.globals.syscom, GET_FILTER_COLOR_G, g)
        }
        SET_FILTER_COLOR_B_DEFAULT => {
            let (_, _, b, _) = config_filter_color_default(ctx);
            cfg_set_int(&mut ctx.globals.syscom, GET_FILTER_COLOR_B, b)
        }
        SET_FILTER_COLOR_A_DEFAULT => {
            let (_, _, _, a) = config_filter_color_default(ctx);
            cfg_set_int(&mut ctx.globals.syscom, GET_FILTER_COLOR_A, a)
        }
        GET_FILTER_COLOR_R | GET_FILTER_COLOR_G | GET_FILTER_COLOR_B | GET_FILTER_COLOR_A => {
            let (r, g, b, a) = config_filter_color_default(ctx);
            let default = match op {
                GET_FILTER_COLOR_R => r,
                GET_FILTER_COLOR_G => g,
                GET_FILTER_COLOR_B => b,
                GET_FILTER_COLOR_A => a,
                _ => 0,
            };
            let v = cfg_get_int(&ctx.globals.syscom, op, default);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_OBJECT_DISP_ONOFF => {
            let index = p_i64(params, 0);
            if (0..4).contains(&index) {
                ctx.globals.syscom.original_config.object_disp_flag[index as usize] =
                    p_bool(params, 1);
            }
        }
        SET_OBJECT_DISP_ONOFF_DEFAULT => {
            let index = p_i64(params, 0);
            if (0..4).contains(&index) {
                // The original command reads the GLOBAL_EXTRA_SWITCH default here.
                let value = config_default_indexed_bool(
                    ctx,
                    "CONFIG.GLOBAL_EXTRA_SWITCH",
                    index as usize,
                    true,
                );
                ctx.globals.syscom.original_config.object_disp_flag[index as usize] = value;
            }
        }
        GET_OBJECT_DISP_ONOFF => {
            let index = p_i64(params, 0);
            let value = if (0..4).contains(&index) {
                ctx.globals.syscom.original_config.object_disp_flag[index as usize]
            } else {
                false
            };
            ctx.push(Value::Int(if value { 1 } else { 0 }));
            return Ok(true);
        }
        SET_GLOBAL_EXTRA_SWITCH_ONOFF => {
            let index = p_i64(params, 0);
            if (0..4).contains(&index) {
                ctx.globals.syscom.original_config.global_extra_switch_flag[index as usize] =
                    p_bool(params, 1);
            }
        }
        SET_GLOBAL_EXTRA_SWITCH_ONOFF_DEFAULT => {
            let index = p_i64(params, 0);
            if (0..4).contains(&index) {
                let value = config_default_indexed_bool(
                    ctx,
                    "CONFIG.GLOBAL_EXTRA_SWITCH",
                    index as usize,
                    true,
                );
                ctx.globals.syscom.original_config.global_extra_switch_flag[index as usize] = value;
            }
        }
        GET_GLOBAL_EXTRA_SWITCH_ONOFF => {
            let index = p_i64(params, 0);
            let value = if (0..4).contains(&index) {
                ctx.globals.syscom.original_config.global_extra_switch_flag[index as usize]
            } else {
                false
            };
            ctx.push(Value::Int(if value { 1 } else { 0 }));
            return Ok(true);
        }
        SET_GLOBAL_EXTRA_MODE_VALUE => {
            let index = p_i64(params, 0);
            if (0..4).contains(&index) {
                ctx.globals.syscom.original_config.global_extra_mode_flag[index as usize] =
                    p_i64(params, 1);
            }
        }
        SET_GLOBAL_EXTRA_MODE_VALUE_DEFAULT => {
            let index = p_i64(params, 0);
            if (0..4).contains(&index) {
                let value = config_default_indexed_mode(ctx, index as usize);
                ctx.globals.syscom.original_config.global_extra_mode_flag[index as usize] = value;
            }
        }
        GET_GLOBAL_EXTRA_MODE_VALUE => {
            let index = p_i64(params, 0);
            let value = if (0..4).contains(&index) {
                ctx.globals.syscom.original_config.global_extra_mode_flag[index as usize]
            } else {
                0
            };
            ctx.push(Value::Int(value));
            return Ok(true);
        }
        SET_SAVELOAD_ALERT_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_SAVELOAD_ALERT_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_SAVELOAD_ALERT_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_SAVELOAD_ALERT_ONOFF, 1)
        }
        GET_SAVELOAD_ALERT_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_SAVELOAD_ALERT_ONOFF, 1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_SLEEP_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_SLEEP_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_SLEEP_ONOFF_DEFAULT => {
            let value = gameexe_bool_or(ctx, "CONFIG.SLEEP.ONOFF", false);
            cfg_set_int(&mut ctx.globals.syscom, GET_SLEEP_ONOFF, if value { 1 } else { 0 });
        }
        GET_SLEEP_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_SLEEP_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_NO_WIPE_ANIME_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_NO_WIPE_ANIME_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_NO_WIPE_ANIME_ONOFF_DEFAULT => {
            let value = gameexe_bool_or(ctx, "CONFIG.NO_WIPE_ANIME.ONOFF", false);
            cfg_set_int(&mut ctx.globals.syscom, GET_NO_WIPE_ANIME_ONOFF, if value { 1 } else { 0 });
        }
        GET_NO_WIPE_ANIME_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_NO_WIPE_ANIME_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_SKIP_WIPE_ANIME_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_SKIP_WIPE_ANIME_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_SKIP_WIPE_ANIME_ONOFF_DEFAULT => {
            let value = gameexe_bool_or(ctx, "CONFIG.SKIP_WIPE_ANIME.ONOFF", true);
            cfg_set_int(&mut ctx.globals.syscom, GET_SKIP_WIPE_ANIME_ONOFF, if value { 1 } else { 0 });
        }
        GET_SKIP_WIPE_ANIME_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_SKIP_WIPE_ANIME_ONOFF, 1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_NO_MWND_ANIME_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_NO_MWND_ANIME_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_NO_MWND_ANIME_ONOFF_DEFAULT => {
            let value = gameexe_bool_or(ctx, "CONFIG.NO_MWND_ANIME.ONOFF", false);
            cfg_set_int(&mut ctx.globals.syscom, GET_NO_MWND_ANIME_ONOFF, if value { 1 } else { 0 });
        }
        GET_NO_MWND_ANIME_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_NO_MWND_ANIME_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_WHEEL_NEXT_MESSAGE_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_WHEEL_NEXT_MESSAGE_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_WHEEL_NEXT_MESSAGE_ONOFF_DEFAULT => {
            let value = gameexe_bool_or(ctx, "CONFIG.WHEEL_NEXT_MESSAGE.ONOFF", true);
            cfg_set_int(&mut ctx.globals.syscom, GET_WHEEL_NEXT_MESSAGE_ONOFF, if value { 1 } else { 0 });
        }
        GET_WHEEL_NEXT_MESSAGE_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_WHEEL_NEXT_MESSAGE_ONOFF, 1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_KOE_DONT_STOP_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_KOE_DONT_STOP_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_KOE_DONT_STOP_ONOFF_DEFAULT => {
            let value = gameexe_bool_or(ctx, "CONFIG.KOE_DONT_STOP.ONOFF", false);
            cfg_set_int(&mut ctx.globals.syscom, GET_KOE_DONT_STOP_ONOFF, if value { 1 } else { 0 });
        }
        GET_KOE_DONT_STOP_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_KOE_DONT_STOP_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_SKIP_UNREAD_MESSAGE_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_SKIP_UNREAD_MESSAGE_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_SKIP_UNREAD_MESSAGE_ONOFF_DEFAULT => {
            let value = gameexe_bool_or(ctx, "CONFIG.SKIP_UNREAD_MESSAGE.ONOFF", false);
            cfg_set_int(&mut ctx.globals.syscom, GET_SKIP_UNREAD_MESSAGE_ONOFF, if value { 1 } else { 0 });
        }
        GET_SKIP_UNREAD_MESSAGE_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_SKIP_UNREAD_MESSAGE_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_PLAY_SILENT_SOUND_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_PLAY_SILENT_SOUND_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_PLAY_SILENT_SOUND_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_PLAY_SILENT_SOUND_ONOFF, 0)
        }
        GET_PLAY_SILENT_SOUND_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_PLAY_SILENT_SOUND_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_FONT_NAME => {
            let v = params
                .get(0)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            ctx.globals.syscom.original_config.font_name = v.clone();
            cfg_set_str(&mut ctx.globals.syscom, GET_FONT_NAME, v);
        }
        SET_FONT_NAME_DEFAULT => {
            let value = config_default_font_name(ctx);
            ctx.globals.syscom.original_config.font_name = value.clone();
            cfg_set_str(&mut ctx.globals.syscom, GET_FONT_NAME, value);
        }
        GET_FONT_NAME => {
            let mut v = cfg_get_str(&ctx.globals.syscom, GET_FONT_NAME);
            if v.is_empty() {
                v = config_default_font_name(ctx);
            }
            ctx.push(Value::Str(v));
            return Ok(true);
        }
        IS_FONT_EXIST => {
            let name = params.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let exists = font_exists(&ctx.project_dir, name);
            ctx.push(Value::Int(if exists { 1 } else { 0 }));
            return Ok(true);
        }
        SET_FONT_BOLD => {
            let value = p_bool(params, 0);
            ctx.globals.syscom.original_config.font_futoku = value;
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_FONT_BOLD,
                if value { 1 } else { 0 },
            );
        }
        SET_FONT_BOLD_DEFAULT => {
            let value = gameexe_bool_or(ctx, "CONFIG.FONT.FUTOKU", false);
            ctx.globals.syscom.original_config.font_futoku = value;
            cfg_set_int(&mut ctx.globals.syscom, GET_FONT_BOLD, if value { 1 } else { 0 });
        }
        GET_FONT_BOLD => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_FONT_BOLD, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_FONT_DECORATION => {
            let value = p_i64(params, 0).clamp(0, 3);
            ctx.globals.syscom.original_config.font_shadow = value;
            cfg_set_int(&mut ctx.globals.syscom, GET_FONT_DECORATION, value);
        }
        SET_FONT_DECORATION_DEFAULT => {
            let value = gameexe_i64_or(ctx, "CONFIG.FONT.SHADOW", 2).clamp(0, 3);
            ctx.globals.syscom.original_config.font_shadow = value;
            cfg_set_int(&mut ctx.globals.syscom, GET_FONT_DECORATION, value);
        }
        GET_FONT_DECORATION => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_FONT_DECORATION, 2);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        CREATE_CAPTURE_BUFFER => {
            let w = p_i64(params, 0).max(1) as u32;
            let h = p_i64(params, 1).max(1) as u32;
            ctx.globals.syscom.capture_size = Some((w, h));
            ctx.globals.syscom.capture_buffer = None;
        }
        DESTROY_CAPTURE_BUFFER => {
            ctx.globals.syscom.capture_buffer = None;
            ctx.globals.syscom.capture_size = None;
        }
        CAPTURE_TO_CAPTURE_BUFFER => {
            let mut img = ctx.capture_frame_rgba()?;
            if let Some((w, h)) = ctx.globals.syscom.capture_size {
                img = resize_rgba(&img, w, h);
            }
            ctx.globals.syscom.capture_buffer = Some(img);
        }
        SAVE_CAPTURE_BUFFER_TO_FILE => {
            let file_name = params.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let extension = params.get(1).and_then(|v| v.as_str()).unwrap_or("");
            let mut name = file_name.to_string();
            if !extension.is_empty()
                && !name
                    .to_ascii_lowercase()
                    .ends_with(&format!(".{}", extension.to_ascii_lowercase()))
            {
                name.push('.');
                name.push_str(extension);
            }
            let path = join_game_path(&ctx.project_dir, &name);
            if ctx.globals.syscom.capture_buffer.is_none() {
                let mut img = ctx.capture_frame_rgba()?;
                if let Some((w, h)) = ctx.globals.syscom.capture_size {
                    img = resize_rgba(&img, w, h);
                }
                ctx.globals.syscom.capture_buffer = Some(img);
            }
            if let Some(img) = ctx.globals.syscom.capture_buffer.as_ref() {
                write_rgba_png(&path, img);
                save_capture_flags_sidecar(ctx, &path, params);
                ctx.push(Value::Int(1));
            } else {
                ctx.push(Value::Int(0));
            }
            return Ok(true);
        }
        LOAD_FLAG_FROM_CAPTURE_FILE => {
            let file_name = params.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let extension = params.get(1).and_then(|v| v.as_str()).unwrap_or("");
            let mut name = file_name.to_string();
            if !extension.is_empty()
                && !name
                    .to_ascii_lowercase()
                    .ends_with(&format!(".{}", extension.to_ascii_lowercase()))
            {
                name.push('.');
                name.push_str(extension);
            }
            let path = join_game_path(&ctx.project_dir, &name);
            let ok = load_capture_flags_sidecar(ctx, &path, params);
            ctx.push(Value::Int(if ok { 1 } else { 0 }));
            return Ok(true);
        }
        CAPTURE_AND_SAVE_BUFFER_TO_PNG => {
            let file_name = params.get(2).and_then(|v| v.as_str()).unwrap_or("");
            let path = join_game_path(&ctx.project_dir, file_name);
            let mut img = ctx.capture_frame_rgba()?;
            if let Some((w, h)) = ctx.globals.syscom.capture_size {
                img = resize_rgba(&img, w, h);
            }
            write_rgba_png(&path, &img);
        }
        OPEN_TWEET_DIALOG => {
            open_tweet_dialog(ctx)?;
        }
        SET_RETURN_SCENE_ONCE => {
            let name = params
                .get(0)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let z_no = p_i64(params, 1);
            ctx.globals.syscom.return_scene_once = Some((name, z_no));
        }
        GET_SYSTEM_EXTRA_INT_VALUE => {
            let v = ctx.globals.syscom.system_extra_int_value;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_SYSTEM_EXTRA_STR_VALUE => {
            let v = ctx.globals.syscom.system_extra_str_value.clone();
            ctx.push(Value::Str(v));
            return Ok(true);
        }
        JOYPAD_MODE_ACTIVE => {
            // Newer engine: SCRIPT.98/99 may override Gameexe joypad permission,
            // while this query still reports joypad mode only when the active
            // input family is actually the joypad.
            let joypad_mode_override = if ctx.excall_state.ex_call_flag {
                ctx.excall_state.joypad_mode_override
            } else {
                ctx.globals.script.joypad_mode_override
            };
            let allow = match joypad_mode_override {
                0 => false,
                1 => true,
                _ => gameexe_bool_or(ctx, "JOYPAD.ALLOW_JOYPAD_MODE", true),
            };
            ctx.push(Value::Int(if allow && ctx.input.joypad_mode_active() {
                1
            } else {
                0
            }));
            return Ok(true);
        }
        OPEN_JOYPAD_CONFIG => {
            // Recovered newer-engine behavior: this opens the engine's native
            // joypad configuration window.  The cross-platform port does not
            // implement that native window; unlike the old placeholder, this is
            // not an input-resync opcode.
            log::error!("SYSCOM.334 native Joypad configuration window is not implemented");
        }
        _ => {
            return Ok(false);
        }
    }

    ctx.push(Value::Int(0));
    Ok(true)
}

#[cfg(test)]
mod tweet_compat_tests {
    use super::*;

    #[test]
    fn tweet_overlap_is_stretched_and_source_alpha_blended() {
        let mut base = RgbaImage {
            width: 2,
            height: 2,
            center_x: 0,
            center_y: 0,
            rgba: [0, 0, 0, 255].repeat(4),
        };
        let overlay = RgbaImage {
            width: 1,
            height: 1,
            center_x: 0,
            center_y: 0,
            rgba: vec![255, 64, 0, 128],
        };

        blend_tweet_overlay_fullscreen(&mut base, &overlay);
        for pixel in base.rgba.chunks_exact(4) {
            assert_eq!(pixel, &[128, 32, 0, 255]);
        }
    }
}

#[cfg(test)]
mod global_save_init_tests {
    use super::*;
    use crate::runtime::forms::codes;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn test_project_dir() -> std::path::PathBuf {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        std::env::temp_dir().join(format!(
            "siglus-global-save-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ))
    }

    #[test]
    fn config_save_versions_preserve_settings_after_optional_fields() {
        for (minor_version, planetarian) in [(0, false), (1, false), (2, false), (2, true), (3, false)] {
            let project_dir = test_project_dir();
            let mut stream = original_save::OriginalStreamWriter::new();
            stream.push_i32(0); // screen mode
            if minor_version >= 3 || planetarian {
                stream.push_i32(1); // window mode
            }
            stream.push_i32(80);
            stream.push_i32(90);
            if minor_version >= 3 || planetarian {
                stream.push_i32(1280);
                stream.push_i32(720);
            }
            stream.push_bool(false); // fullscreen resolution change
            for value in [1, 0, 28, 27, 1920, 1080, 1, 100, 100] {
                stream.push_i32(value);
            }
            stream.push_bool(true); // fullscreen scale sync
            stream.push_i32(0);
            stream.push_i32(0);
            stream.push_i32(180); // master volume
            let sound_count = if minor_version == 0 { 5 } else { 32 };
            for _ in 0..sound_count {
                stream.push_i32(200);
            }
            for _ in 0..=sound_count {
                stream.push_bool(true);
            }
            stream.push_i32(175); // BGM fade
            stream.push_bool(true);
            stream.push_u32(0xe61d0818); // filter
            stream.push_bool(false); // proportional font
            stream.push_str("Test Font");
            stream.push_i32(2); // font shadow
            stream.push_bool(false); // bold
            stream.push_i32(20); // message speed
            stream.push_bool(false); // no wait
            stream.push_bool(false); // auto mode
            stream.push_i32(70);
            stream.push_i32(300);
            if minor_version >= 2 {
                stream.push_bool(true); // cursor auto-hide
                stream.push_i32(2468);
            }
            stream.push_bool(true); // jitan normal
            stream.push_bool(false); // jitan auto
            stream.push_bool(true); // jitan backlog
            stream.push_i32(125); // jitan speed
            stream.push_i32(1); // voice mode
            stream.push_i32(1); // character voices
            stream.push_bool(false);
            stream.push_padding(3);
            stream.push_i32(170);
            stream.push_bool(true); // character text colors
            for _ in 0..2 { // object and extra-switch flags
                stream.push_i32(4);
                for flag in [true, false, true, false] {
                    stream.push_bool(flag);
                }
            }
            stream.push_i32(4); // extra modes
            for value in [0, 1, 2, 3] {
                stream.push_i32(value);
            }
            for flag in [false, false, false, false, true, false, false] {
                stream.push_bool(flag);
            }
            if minor_version == 0 {
                stream.push_bool(true);
                stream.push_i32(0x12345678);
            }
            stream.push_bool(true); // save/load alert
            stream.push_bool(false); // save/load double click
            for path in ["screenshots", "editor"] {
                stream.push_str(path);
            }
            if minor_version >= 1 {
                stream.push_str("voices");
                stream.push_str("voice-tool");
            }
            let payload = stream.into_inner();
            let packed = original_save::pack_buffer(&payload);
            let mut data = original_save::OriginalConfigSaveHeader {
                major_version: 1,
                minor_version,
                config_data_size: packed.len() as i32,
            }.to_bytes();
            data.extend_from_slice(&packed);
            fs::create_dir_all(project_dir.join("savedata")).unwrap();
            fs::write(project_dir.join("savedata/config.sav"), data).unwrap();
            let mut ctx = CommandContext::new(project_dir.clone());
            if planetarian {
                ctx.tables.gameexe = Some(crate::formats::gameexe::GameexeConfig::from_text(
                    "#GAMEID=\"planetarian [HD Edition]\"\n",
                ));
            }
            let defaults = original_config_defaults(&ctx);

            load_config_save(&mut ctx).unwrap();
            let cfg = &ctx.globals.syscom.original_config;
            assert_eq!(cfg.screen_size_scale, (80, 90));
            if minor_version >= 3 || planetarian {
                assert_eq!(cfg.screen_size_mode_window, 1);
                assert_eq!(cfg.screen_size_free, (1280, 720));
            }
            assert_eq!(cfg.all_sound_user_volume, 180);
            assert_eq!(&cfg.sound_user_volume[..5], &[200; 5]);
            if minor_version == 0 {
                assert_eq!(&cfg.sound_user_volume[5..], &defaults.sound_user_volume[5..]);
                assert_eq!(&cfg.play_sound_check[5..], &defaults.play_sound_check[5..]);
            }
            assert_eq!(cfg.font_name, "Test Font");
            assert_eq!(cfg.auto_mode_min_wait, 300);
            assert_eq!(cfg.mouse_cursor_hide_onoff,
                if minor_version < 2 { defaults.mouse_cursor_hide_onoff } else { true });
            assert_eq!(cfg.mouse_cursor_hide_time,
                if minor_version < 2 { defaults.mouse_cursor_hide_time } else { 2468 });
            assert!(cfg.jitan_normal_onoff && cfg.jitan_msgbk_onoff);
            assert!(!cfg.jitan_auto_mode_onoff);
            assert_eq!(cfg.jitan_speed, 125);
            assert_eq!(cfg.koe_mode, 1);
            assert!(!cfg.chrkoe[0].onoff);
            assert_eq!(cfg.chrkoe[0].volume, 170);
            assert_eq!(cfg.global_extra_mode_flag, [0, 1, 2, 3]);
            assert!(cfg.saveload_alert_flag);
            assert!(!cfg.saveload_dblclick_flag);
            assert_eq!(cfg.ss_path, "screenshots");
            assert_eq!(cfg.editor_path, "editor");
            assert_eq!(cfg.koe_path,
                if minor_version == 0 { defaults.koe_path.as_str() } else { "voices" });
            assert_eq!(cfg.koe_tool_path,
                if minor_version == 0 { defaults.koe_tool_path.as_str() } else { "voice-tool" });
            let header = original_save::OriginalConfigSaveHeader {
                major_version: 1, minor_version, config_data_size: 0,
            };
            let game_id = gameexe_unquoted_owned(&ctx, "GAMEID");
            assert!(save_versions::read_config(header, &payload[..payload.len() - 1], &mut defaults.clone(), &game_id).is_err());
            let mut trailing = payload.clone();
            trailing.push(0);
            assert!(save_versions::read_config(header, &trailing, &mut defaults.clone(), &game_id).is_err());
            let unknown = original_save::OriginalConfigSaveHeader { minor_version: 99, ..header };
            assert!(save_versions::read_config(unknown, &payload, &mut defaults.clone(), &game_id)
                .unwrap_err().to_string().contains("unsupported config save version"));
            fs::remove_dir_all(project_dir).unwrap();
        }
    }

    #[test]
    fn config_dialog_edits_update_script_state_and_survive_reopening() {
        let project_dir = test_project_dir();
        fs::create_dir_all(&project_dir).unwrap();
        let mut ctx = CommandContext::new(project_dir.clone());
        let gameexe = crate::formats::gameexe::GameexeConfig::from_text(
            "#CONFIG.VOLUME.BGM=165\n#CHRKOE.000=\"Hidden\",2,\"Alias\",1,235,(0)\n");
        ctx.tables.gameexe = Some(gameexe.clone());
        load_global_save(&mut ctx).unwrap();
        let mut config = config_state_for_save(&ctx);
        config.sound_user_volume[0] = 87;
        config.play_sound_check[3] = false;
        config.message_speed = 31;
        config.auto_mode_moji_wait = 90;
        config.chrkoe[0].volume = 170;
        config.global_extra_switch_flag[0] = false;
        config.skip_unread_message_flag = true;
        apply_config_dialog_state(&mut ctx, config.clone());
        assert_eq!(cfg_get_int(&ctx.globals.syscom, GET_BGM_VOLUME, -1), 87);
        assert_eq!(ctx.globals.script.auto_mode_moji_wait, 90);
        assert_eq!(config_state_for_save(&ctx), config);
        write_config_save(&ctx);
        reveal_config_voice_name(&mut ctx, "Alias");
        write_global_save(&ctx);

        let mut reopened = CommandContext::new(project_dir.clone());
        reopened.tables.gameexe = Some(gameexe);
        load_global_save(&mut reopened).unwrap();
        assert_eq!(config_state_for_save(&reopened), config);
        assert!(config_chrkoe_name_visible(&reopened, 0));
        fs::remove_dir_all(project_dir).unwrap();
    }

    #[test]
    fn missing_global_save_keeps_zero_initialized_flags() {
        let project_dir = test_project_dir();
        fs::create_dir_all(&project_dir).expect("test project dir");
        let mut ctx = CommandContext::new(project_dir.clone());

        load_global_save(&mut ctx).expect("missing global save is valid");

        let g = &ctx.globals.int_lists[&(codes::ELM_GLOBAL_G as u32)];
        let z = &ctx.globals.int_lists[&(codes::ELM_GLOBAL_Z as u32)];
        let m = &ctx.globals.str_lists[&(codes::ELM_GLOBAL_M as u32)];
        assert!(!g.is_empty() && g.iter().all(|value| *value == 0));
        assert!(!z.is_empty() && z.iter().all(|value| *value == 0));
        assert!(!m.is_empty() && m.iter().all(String::is_empty));

        let _ = fs::remove_dir_all(project_dir);
    }

    #[test]
    fn corrupt_existing_global_save_is_an_error() {
        let project_dir = test_project_dir();
        let save_dir = project_dir.join("savedata");
        fs::create_dir_all(&save_dir).expect("test save dir");
        fs::write(save_dir.join("global.sav"), b"invalid").expect("corrupt global save");
        let mut ctx = CommandContext::new(project_dir.clone());

        assert!(load_global_save(&mut ctx).is_err());

        let _ = fs::remove_dir_all(project_dir);
    }

    #[test]
    fn global_save_with_uninitialized_cg_table_preserves_following_fields() {
        let project_dir = test_project_dir();
        let mut stream = original_save::OriginalStreamWriter::new();
        stream.push_i64(61151);
        stream.push_fixed_i32_list(&[42], 10000);
        stream.push_fixed_i32_list(&[7], 10000);
        stream.push_fixed_str_list(&["global".to_string()], 10000);
        stream.push_fixed_str_list(&[], 702);
        stream.push_i32(0);
        stream.push_extend_i32_list(&[]);
        stream.push_fixed_i32_list(&[1, 0, 1], 20);
        stream.push_i32(2);
        stream.push_str("first");
        stream.push_bool(true);
        stream.push_str("second");
        stream.push_bool(false);
        let payload = stream.into_inner();
        original_save::write_global_save_file(&project_dir, &payload).unwrap();

        let mut ctx = CommandContext::new(project_dir.clone());
        load_global_save(&mut ctx).expect("native empty extendable CG table");
        assert_eq!(ctx.globals.syscom.total_play_time, 61151);
        assert_eq!(ctx.globals.int_lists[&(codes::ELM_GLOBAL_G as u32)][0], 42);
        assert_eq!(ctx.globals.int_lists[&(codes::ELM_GLOBAL_Z as u32)][0], 7);
        assert_eq!(ctx.globals.str_lists[&(codes::ELM_GLOBAL_M as u32)][0], "global");
        assert!(ctx.tables.cg_flags.is_empty());
        assert_eq!(ctx.globals.bgm_table_flags.len(), 20);
        assert_eq!(&ctx.globals.bgm_table_flags[..3], &[true, false, true]);
        assert_eq!(ctx.globals.syscom.chrkoe_look_flags.get("first"), Some(&true));
        assert_eq!(ctx.globals.syscom.chrkoe_look_flags.get("second"), Some(&false));

        // The alternate CG layout must not hide a truncated later field.
        original_save::write_global_save_file(&project_dir, &payload[..payload.len() - 1]).unwrap();
        assert!(load_global_save(&mut ctx).is_err());
        fs::remove_dir_all(project_dir).unwrap();
    }

    #[test]
    fn global_save_writer_uses_native_empty_cg_layout() {
        let project_dir = test_project_dir();
        fs::create_dir_all(&project_dir).unwrap();
        let mut ctx = CommandContext::new(project_dir.clone());
        ctx.tables.gameexe = Some(crate::formats::gameexe::GameexeConfig::from_text(
            "#CGTABLE_FILE=\"\"\n#CGTABLE_FLAG_CNT=1000\n",
        ));
        ctx.globals.bgm_table_flags = vec![true, false, true];
        write_global_save(&ctx);
        let (_, payload) = original_save::read_global_save_file(&project_dir).unwrap();
        let mut rd = original_save::OriginalStreamReader::new(&payload);
        rd.i64().unwrap();
        rd.fixed_i32_list().unwrap();
        rd.fixed_i32_list().unwrap();
        rd.fixed_str_list().unwrap();
        rd.fixed_str_list().unwrap();
        rd.i32().unwrap();
        assert_eq!(rd.i32().unwrap(), 0, "CG count has no preceding jump");
        assert_eq!(&rd.fixed_i32_list().unwrap()[..3], &[1, 0, 1]);
        fs::remove_dir_all(project_dir).unwrap();
    }

    #[test]
    fn original_global_save_uses_one_byte_character_voice_flags() {
        for (major, minor) in [(1, 2), (1, 5), (2, 0)] {
            for empty_cg in [false, true] {
                check_original_global_save(major, minor, empty_cg);
            }
        }
    }

    fn check_original_global_save(major_version: i32, minor_version: i32, empty_cg: bool) {
        let project_dir = test_project_dir();
        fs::create_dir_all(&project_dir).expect("test project dir");

        let mut stream = original_save::OriginalStreamWriter::new();
        stream.push_i64(1234);
        stream.push_fixed_i32_list(&[42], 1);
        stream.push_fixed_i32_list(&[7], 1);
        stream.push_fixed_str_list(&["global".to_string()], 1);
        stream.push_fixed_str_list(&["名前".to_string()], 702);
        stream.push_i32(1359557559);
        if empty_cg {
            stream.push_extend_i32_list(&[]);
        } else {
            stream.push_fixed_i32_list(&[1, 0, 1], 1000);
        }
        stream.push_fixed_i32_list(&[0, 1], 52);
        stream.push_i32(2);
        stream.push_str("桜");
        stream.push_bool(true);
        stream.push_str("second");
        stream.push_bool(false);
        let payload = stream.into_inner();
        let packed = original_save::pack_buffer(&payload);
        let mut data = original_save::OriginalGlobalSaveHeader {
            major_version,
            minor_version,
            global_data_size: packed.len() as i32,
        }.to_bytes();
        data.extend_from_slice(&packed);
        let path = project_dir.join("savedata/global.sav");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, &data).expect("synthetic global save");

        let mut ctx = CommandContext::new(project_dir.clone());
        load_global_save(&mut ctx).expect("one-byte chrkoe flags");
        assert_eq!(ctx.globals.syscom.total_play_time, 1234);
        assert_eq!(ctx.globals.int_lists[&(codes::ELM_GLOBAL_G as u32)][0], 42);
        assert_eq!(ctx.globals.int_lists[&(codes::ELM_GLOBAL_Z as u32)][0], 7);
        assert_eq!(ctx.globals.str_lists[&(codes::ELM_GLOBAL_M as u32)][0], "global");
        assert_eq!(ctx.globals.str_lists[&(codes::ELM_GLOBAL_NAMAE_GLOBAL as u32)][0], "名前");
        if empty_cg {
            assert!(ctx.tables.cg_flags.is_empty());
        } else {
            assert_eq!(&ctx.tables.cg_flags[..3], &[1, 0, 1]);
        }
        assert_eq!(&ctx.globals.bgm_table_flags[..2], &[false, true]);
        assert_eq!(ctx.globals.syscom.chrkoe_look_flags.get("桜"), Some(&true));
        assert_eq!(ctx.globals.syscom.chrkoe_look_flags.get("second"), Some(&false));

        let header = original_save::OriginalGlobalSaveHeader {
            major_version, minor_version, global_data_size: 0,
        };
        assert!(save_versions::read_global(header, &payload[..payload.len() - 1]).is_err());
        let mut trailing = payload;
        trailing.push(0);
        assert!(save_versions::read_global(header, &trailing).is_err());

        fs::write(&path, &data[..data.len() - 1]).unwrap();
        assert!(load_global_save(&mut ctx).unwrap_err().to_string().contains("payload truncated"));
        data[4..8].copy_from_slice(&99i32.to_le_bytes());
        fs::write(&path, &data).unwrap();
        assert!(load_global_save(&mut ctx).unwrap_err().to_string().contains("unsupported global save version"));

        let _ = fs::remove_dir_all(project_dir);
    }
}
