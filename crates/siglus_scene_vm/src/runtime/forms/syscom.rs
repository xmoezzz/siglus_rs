use anyhow::Result;

use crate::runtime::globals::{
    SaveSlotState, SyscomPendingProc, SyscomPendingProcKind, ToggleFeatureState, ValueFeatureState,
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


pub(crate) fn append_current_save_message(ctx: &mut CommandContext, msg: &str) {
    if msg.is_empty() {
        return;
    }
    ctx.globals.syscom.current_save_message.push_str(msg);
    if ctx.globals.syscom.current_save_message.chars().count() > 256 {
        ctx.globals.syscom.current_save_message = ctx
            .globals
            .syscom
            .current_save_message
            .chars()
            .take(256)
            .collect();
    }
    ctx.globals.syscom.current_save_full_message.push_str(msg);
    if ctx.globals.syscom.current_save_full_message.chars().count() > 256 {
        ctx.globals.syscom.current_save_full_message = ctx
            .globals
            .syscom
            .current_save_full_message
            .chars()
            .take(256)
            .collect();
    }
}

fn current_full_save_message(ctx: &CommandContext) -> String {
    let full = ctx.globals.syscom.current_save_full_message.clone();
    if full.is_empty() {
        ctx.globals.syscom.current_save_message.clone()
    } else {
        full
    }
}

fn ensure_slot(slots: &mut Vec<SaveSlotState>, idx: usize) -> &mut SaveSlotState {
    if slots.len() <= idx {
        slots.resize_with(idx + 1, SaveSlotState::default);
    }
    &mut slots[idx]
}

pub(crate) fn menu_save_slot(ctx: &mut CommandContext, quick: bool, idx: usize) {
    let default_title = ctx.globals.syscom.current_save_scene_title.clone();
    let default_message = ctx.globals.syscom.current_save_message.clone();
    let default_full_message = current_full_save_message(ctx);
    let slot = if quick {
        ensure_slot(&mut ctx.globals.syscom.quick_save_slots, idx)
    } else {
        ensure_slot(&mut ctx.globals.syscom.save_slots, idx)
    };
    slot.exist = true;
    set_slot_timestamp(slot);
    slot.title = default_title;
    slot.message = default_message;
    slot.full_message = default_full_message;
    slot.append_dir = ctx.globals.append_dir.clone();
    slot.append_name = ctx.globals.append_name.clone();
    slot.comment.clear();
    slot.values.clear();
    let kind = if quick { RuntimeSaveKind::Quick } else { RuntimeSaveKind::Normal };
    ctx.request_runtime_save(kind, idx);
}

pub(crate) fn menu_load_slot(ctx: &mut CommandContext, quick: bool, idx: usize) {
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

pub(crate) fn save_dir(project_dir: &Path) -> PathBuf {
    original_save::save_dir(project_dir)
}

fn slot_path_with_counts(project_dir: &Path, quick: bool, idx: usize, save_cnt: usize, quick_cnt: usize) -> PathBuf {
    let kind = if quick { SaveKind::Quick } else { SaveKind::Normal };
    original_save::save_file_path_with_counts(project_dir, save_cnt, quick_cnt, kind, idx)
}

fn slot_path_ctx(ctx: &CommandContext, quick: bool, idx: usize) -> PathBuf {
    slot_path_with_counts(
        &ctx.project_dir,
        quick,
        idx,
        configured_save_count(ctx, false),
        configured_save_count(ctx, true),
    )
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

fn capture_slot_thumb(ctx: &mut CommandContext, config: SaveThumbConfig) -> RgbaImage {
    if let Some(name) = pick_thumb_source_name(ctx) {
        if let Ok(img_id) = ctx.images.load_g00(&name, 0) {
            if let Some(img) = ctx.images.get(img_id) {
                return resize_rgba(img.as_ref(), config.width, config.height);
            }
        }
    }

    let img = ctx.capture_frame_rgba();
    resize_rgba(&img, config.width, config.height)
}

fn write_slot_thumb_for_save_no(ctx: &mut CommandContext, save_no: usize) {
    let config = save_thumb_config(ctx);
    if !config.enabled {
        return;
    }
    let path = thumb_path_for_no_with_config(&ctx.project_dir, config, save_no);
    let _ = fs::remove_file(&path);
    let img = capture_slot_thumb(ctx, config);
    match config.thumb_type {
        SaveThumbType::Bmp => write_rgba_bmp_top_down(&path, &img),
        SaveThumbType::Png => write_rgba_png_opaque(&path, &img),
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

fn write_slot(path: &Path, slot: &SaveSlotState) {
    if let Err(err) = original_save::write_slot_file(path, slot) {
        eprintln!("[SG_SAVE] failed to write original save file {}: {err:#}", path.display());
    }
}

fn read_slot(path: &Path) -> Option<SaveSlotState> {
    original_save::read_slot_from_path(path)
}

fn write_global_save(ctx: &CommandContext) {
    let mut stream = original_save::OriginalStreamWriter::new();
    stream.push_i64(ctx.globals.syscom.total_play_time);

    let fixed_flag_cnt = ctx
        .tables
        .gameexe
        .as_ref()
        .and_then(|cfg| cfg.get_usize("#GLOBAL_FLAG.CNT").or_else(|| cfg.get_usize("GLOBAL_FLAG.CNT")))
        .unwrap_or(1000)
        .min(10000);
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

    let mut cg_flags: Vec<i64> = Vec::new();
    if ctx.globals.cg_table_off {
        cg_flags.push(1);
    }
    stream.push_extend_i32_list(&cg_flags);

    let bgm_flags: Vec<i64> = ctx
        .globals
        .bgm_table_flags
        .iter()
        .map(|v| if *v { 1 } else { 0 })
        .collect();
    stream.push_extend_i32_list(&bgm_flags);

    stream.push_i32(0);

    let payload = stream.into_inner();
    if let Err(err) = original_save::write_global_save_file(&ctx.project_dir, &payload) {
        eprintln!("[SG_SAVE] failed to write global.sav: {err:#}");
    }
}

fn load_global_save(ctx: &mut CommandContext) {
    let Ok(payload) = original_save::read_global_save_file(&ctx.project_dir) else {
        return;
    };
    let mut rd = original_save::OriginalStreamReader::new(&payload);
    let Ok(total_play_time) = rd.i64() else {
        return;
    };
    ctx.globals.syscom.total_play_time = total_play_time;
    if let Ok(g) = rd.fixed_i32_list() {
        ctx.globals
            .int_lists
            .insert(crate::runtime::forms::codes::ELM_GLOBAL_G as u32, g);
    } else {
        return;
    }
    if let Ok(z) = rd.fixed_i32_list() {
        ctx.globals
            .int_lists
            .insert(crate::runtime::forms::codes::ELM_GLOBAL_Z as u32, z);
    } else {
        return;
    }
    if let Ok(m) = rd.fixed_str_list() {
        ctx.globals
            .str_lists
            .insert(crate::runtime::forms::codes::ELM_GLOBAL_M as u32, m);
    } else {
        return;
    }
    if let Ok(namae_global) = rd.fixed_str_list() {
        ctx.globals.str_lists.insert(
            crate::runtime::forms::codes::ELM_GLOBAL_NAMAE_GLOBAL as u32,
            namae_global,
        );
    }
    let _ = rd.i32();
    if let Ok(cg) = rd.extend_i32_list() {
        ctx.globals.cg_table_off = cg.first().copied().unwrap_or(0) != 0;
    }
    if let Ok(bgm) = rd.extend_i32_list() {
        ctx.globals.bgm_table_flags = bgm.into_iter().map(|v| v != 0).collect();
    }
}


fn ensure_slot_loaded_with_counts(
    project_dir: &Path,
    quick: bool,
    save_cnt: usize,
    quick_cnt: usize,
    slots: &mut Vec<SaveSlotState>,
    idx: usize,
) {
    if slots.get(idx).map(|s| s.exist).unwrap_or(false) {
        return;
    }
    let path = slot_path_with_counts(project_dir, quick, idx, save_cnt, quick_cnt);
    if let Some(slot) = read_slot(&path) {
        let s = ensure_slot(slots, idx);
        *s = slot;
    }
}

fn ensure_slot_loaded_ctx(ctx: &CommandContext, quick: bool, slots: &mut Vec<SaveSlotState>, idx: usize) {
    ensure_slot_loaded_with_counts(
        &ctx.project_dir,
        quick,
        configured_save_count(ctx, false),
        configured_save_count(ctx, true),
        slots,
        idx,
    );
}

fn persist_slot_with_counts(
    project_dir: &Path,
    quick: bool,
    save_cnt: usize,
    quick_cnt: usize,
    slots: &[SaveSlotState],
    idx: usize,
) {
    if let Some(slot) = slots.get(idx) {
        let path = slot_path_with_counts(project_dir, quick, idx, save_cnt, quick_cnt);
        if path.exists() {
            match original_save::read_header_from_path(&path) {
                Ok(old_header) => {
                    let header = original_save::OriginalSaveHeader::from_slot(
                        slot,
                        old_header.data_size.max(0) as usize,
                    );
                    if let Err(err) = original_save::write_header_in_place(&path, &header) {
                        eprintln!(
                            "[SG_SAVE] failed to update original save header {}: {err:#}",
                            path.display()
                        );
                    }
                    return;
                }
                Err(err) => {
                    eprintln!(
                        "[SG_SAVE] failed to read original save header {}: {err:#}",
                        path.display()
                    );
                }
            }
        }
        write_slot(&path, slot);
    }
}

fn persist_slot_ctx(ctx: &CommandContext, quick: bool, slots: &[SaveSlotState], idx: usize) {
    persist_slot_with_counts(
        &ctx.project_dir,
        quick,
        configured_save_count(ctx, false),
        configured_save_count(ctx, true),
        slots,
        idx,
    );
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
    let _ = fs::remove_file(thumb_path_for_no_with_config(project_dir, config, save_no));
}

fn copy_thumb_file(project_dir: &Path, save_cnt: usize, quick_cnt: usize, quick: bool, config: SaveThumbConfig, src: usize, dst: usize) {
    if !config.enabled {
        return;
    }
    let src_no = slot_thumb_save_no(save_cnt, quick_cnt, quick, src);
    let dst_no = slot_thumb_save_no(save_cnt, quick_cnt, quick, dst);
    let src_path = thumb_path_for_no_with_config(project_dir, config, src_no);
    let dst_path = thumb_path_for_no_with_config(project_dir, config, dst_no);
    if src_path.exists() {
        if let Some(parent) = dst_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::copy(src_path, dst_path);
    } else {
        let _ = fs::remove_file(dst_path);
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
    let a_exists = pa.exists();
    let b_exists = pb.exists();
    if a_exists {
        let _ = fs::rename(&pa, &tmp);
    }
    if b_exists {
        if let Some(parent) = pa.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::rename(&pb, &pa);
    } else {
        let _ = fs::remove_file(&pa);
    }
    if a_exists {
        if let Some(parent) = pb.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::rename(&tmp, &pb);
    } else {
        let _ = fs::remove_file(&pb);
    }
    let _ = fs::remove_file(&tmp);
}

fn copy_save_file(project_dir: &Path, quick: bool, save_cnt: usize, quick_cnt: usize, src: usize, dst: usize) {
    let src_path = slot_path_with_counts(project_dir, quick, src, save_cnt, quick_cnt);
    let dst_path = slot_path_with_counts(project_dir, quick, dst, save_cnt, quick_cnt);
    if src_path.exists() {
        if let Some(parent) = dst_path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::copy(src_path, dst_path);
    } else {
        let _ = fs::remove_file(dst_path);
    }
}

fn swap_save_file(project_dir: &Path, quick: bool, save_cnt: usize, quick_cnt: usize, a: usize, b: usize) {
    if a == b {
        return;
    }
    let pa = slot_path_with_counts(project_dir, quick, a, save_cnt, quick_cnt);
    let pb = slot_path_with_counts(project_dir, quick, b, save_cnt, quick_cnt);
    let tmp = pa.with_extension("sav.swap");
    let a_exists = pa.exists();
    let b_exists = pb.exists();
    if a_exists {
        let _ = fs::rename(&pa, &tmp);
    }
    if b_exists {
        if let Some(parent) = pa.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::rename(&pb, &pa);
    } else {
        let _ = fs::remove_file(&pa);
    }
    if a_exists {
        if let Some(parent) = pb.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::rename(&tmp, &pb);
    } else {
        let _ = fs::remove_file(&pb);
    }
    let _ = fs::remove_file(&tmp);
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
    *ensure_slot(slots, idx) = SaveSlotState::default();
    let path = slot_path_with_counts(project_dir, quick, idx, save_cnt, quick_cnt);
    let _ = fs::remove_file(path);
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
    let _ = fs::write(capture_flags_path(image_path), out);
}

fn load_capture_flags_sidecar(
    ctx: &mut CommandContext,
    image_path: &Path,
    params: &[Value],
) -> bool {
    let path = capture_flags_path(image_path);
    let Ok(data) = fs::read_to_string(path) else {
        return image_path.exists();
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
    let _ = std::fs::write(path, out);
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

fn first_free_slot(slots: &[SaveSlotState]) -> i64 {
    for (i, s) in slots.iter().enumerate() {
        if !s.exist {
            return i as i64;
        }
    }
    slots.len() as i64
}

fn set_slot_timestamp(slot: &mut SaveSlotState) {
    use chrono::{Datelike, Local, Timelike};
    let now = Local::now();
    slot.year = now.year() as i64;
    slot.month = now.month() as i64;
    slot.day = now.day() as i64;
    slot.weekday = now.weekday().num_days_from_sunday() as i64;
    slot.hour = now.hour() as i64;
    slot.minute = now.minute() as i64;
    slot.second = now.second() as i64;
    slot.millisecond = now.timestamp_subsec_millis() as i64;
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
    let v = v.clamp(0, 100);
    ((v * 255) / 100) as u8
}

pub(crate) fn apply_audio_config(ctx: &mut CommandContext) {
    use crate::audio::TrackKind;
    let all_vol = cfg_get_int(&ctx.globals.syscom, GET_ALL_VOLUME, 100);
    let all_on = cfg_get_int(&ctx.globals.syscom, GET_ALL_ONOFF, 1) != 0;
    let all_raw = if all_on { volume_to_raw(all_vol) } else { 0 };

    let bgm_vol = cfg_get_int(&ctx.globals.syscom, GET_BGM_VOLUME, 100);
    let bgm_on = cfg_get_int(&ctx.globals.syscom, GET_BGM_ONOFF, 1) != 0;
    let bgm_raw = if bgm_on { volume_to_raw(bgm_vol) } else { 0 };

    let se_vol = cfg_get_int(&ctx.globals.syscom, GET_SE_VOLUME, 100);
    let se_on = cfg_get_int(&ctx.globals.syscom, GET_SE_ONOFF, 1) != 0;
    let se_raw = if se_on { volume_to_raw(se_vol) } else { 0 };

    let pcm_vol = cfg_get_int(&ctx.globals.syscom, GET_PCM_VOLUME, 100);
    let pcm_on = cfg_get_int(&ctx.globals.syscom, GET_PCM_ONOFF, 1) != 0;
    let pcm_raw = if pcm_on { volume_to_raw(pcm_vol) } else { 0 };

    let koe_vol = cfg_get_int(&ctx.globals.syscom, GET_KOE_VOLUME, 100);
    let koe_on = cfg_get_int(&ctx.globals.syscom, GET_KOE_ONOFF, 1) != 0;
    let koe_raw = if koe_on { volume_to_raw(koe_vol) } else { 0 };

    let mov_vol = cfg_get_int(&ctx.globals.syscom, GET_MOV_VOLUME, 100);
    let mov_on = cfg_get_int(&ctx.globals.syscom, GET_MOV_ONOFF, 1) != 0;
    let mov_raw = if mov_on { volume_to_raw(mov_vol) } else { 0 };

    let eff_bgm = (all_raw as u16 * bgm_raw as u16 / 255) as u8;
    let eff_se = (all_raw as u16 * se_raw as u16 / 255) as u8;
    let eff_pcm = (all_raw as u16 * pcm_raw as u16 / 255) as u8;
    let eff_koe = (all_raw as u16 * koe_raw as u16 / 255) as u8;
    let eff_mov = (all_raw as u16 * mov_raw as u16 / 255) as u8;

    ctx.audio
        .set_track_master_volume_raw(TrackKind::Bgm, eff_bgm);
    ctx.audio.set_track_master_volume_raw(TrackKind::Se, eff_se);
    ctx.audio
        .set_track_master_volume_raw(TrackKind::Pcm, eff_pcm);
    ctx.audio
        .set_track_master_volume_raw(TrackKind::Koe, eff_koe);
    ctx.audio
        .set_track_master_volume_raw(TrackKind::Mov, eff_mov);
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

fn write_rgba_png(path: &Path, img: &RgbaImage) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Some(buf) = image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone()) {
        let _ = buf.save(path);
    }
}

fn write_rgba_png_opaque(path: &Path, img: &RgbaImage) {
    let opaque = opaque_rgba(img);
    write_rgba_png(path, &opaque);
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

fn write_rgba_bmp_top_down(path: &Path, img: &RgbaImage) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let width = img.width;
    let height = img.height;
    if width == 0 || height == 0 {
        return;
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
    let _ = fs::write(path, out);
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

fn font_exists(project_dir: &Path, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if crate::text_render::font_name_matches_embedded_default(name) {
        return true;
    }

    let name_lower = name.to_ascii_lowercase();
    for font_dir in [project_dir.join("font"), project_dir.join("fonts")] {
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
            load_global_save(ctx);
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
        REPLAY_KOE => ctx.globals.syscom.replay_koe = Some((p_i64(params, 0), p_i64(params, 1))),
        CHECK_REPLAY_KOE => {
            let v = if ctx.globals.syscom.replay_koe.is_some() {
                1
            } else {
                0
            };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_REPLAY_KOE_KOE_NO => {
            let v = ctx.globals.syscom.replay_koe.map(|v| v.0).unwrap_or(-1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_REPLAY_KOE_CHARA_NO => {
            let v = ctx.globals.syscom.replay_koe.map(|v| v.1).unwrap_or(-1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        CLEAR_REPLAY_KOE => ctx.globals.syscom.replay_koe = None,
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
            let default_title = ctx.globals.syscom.current_save_scene_title.clone();
            let default_message = ctx.globals.syscom.current_save_message.clone();
            let default_full_message = current_full_save_message(ctx);
            let slot = ensure_slot(&mut ctx.globals.syscom.save_slots, idx);
            slot.exist = true;
            set_slot_timestamp(slot);
            slot.title = default_title;
            slot.message = default_message;
            slot.full_message = default_full_message;
            slot.append_dir = ctx.globals.append_dir.clone();
            slot.append_name = ctx.globals.append_name.clone();
            slot.comment.clear();
            slot.values.clear();
            ctx.request_runtime_save(RuntimeSaveKind::Normal, idx);
            write_global_save(ctx);
            ctx.push(Value::Int(1));
            return Ok(true);
        }
        LOAD => {
            let idx = p_i64(params, 0).max(0) as usize;
            let save_cnt = configured_save_count(ctx, false);
            let quick_cnt = configured_save_count(ctx, true);
            ensure_slot_loaded_with_counts(
                &ctx.project_dir,
                false,
                save_cnt,
                quick_cnt,
                &mut ctx.globals.syscom.save_slots,
                idx,
            );
            ctx.request_runtime_load(RuntimeSaveKind::Normal, idx);
            ctx.globals.syscom.last_menu_call = LOAD;
        }
        QUICK_SAVE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let default_title = ctx.globals.syscom.current_save_scene_title.clone();
            let default_message = ctx.globals.syscom.current_save_message.clone();
            let default_full_message = current_full_save_message(ctx);
            let slot = ensure_slot(&mut ctx.globals.syscom.quick_save_slots, idx);
            slot.exist = true;
            set_slot_timestamp(slot);
            slot.title = default_title;
            slot.message = default_message;
            slot.full_message = default_full_message;
            slot.append_dir = ctx.globals.append_dir.clone();
            slot.append_name = ctx.globals.append_name.clone();
            slot.comment.clear();
            slot.values.clear();
            ctx.request_runtime_save(RuntimeSaveKind::Quick, idx);
            write_global_save(ctx);
            ctx.push(Value::Int(1));
            return Ok(true);
        }
        QUICK_LOAD => {
            let idx = p_i64(params, 0).max(0) as usize;
            let save_cnt = configured_save_count(ctx, false);
            let quick_cnt = configured_save_count(ctx, true);
            ensure_slot_loaded_with_counts(
                &ctx.project_dir,
                true,
                save_cnt,
                quick_cnt,
                &mut ctx.globals.syscom.quick_save_slots,
                idx,
            );
            ctx.request_runtime_load(RuntimeSaveKind::Quick, idx);
            ctx.globals.syscom.last_menu_call = QUICK_LOAD;
        }
        END_SAVE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let mut slot = SaveSlotState::default();
            slot.exist = true;
            set_slot_timestamp(&mut slot);
            slot.title = ctx.globals.syscom.current_save_scene_title.clone();
            slot.message = ctx.globals.syscom.current_save_message.clone();
            slot.full_message = current_full_save_message(ctx);
            slot.append_dir = ctx.globals.append_dir.clone();
            slot.append_name = ctx.globals.append_name.clone();
            let save_cnt = configured_save_count(ctx, false);
            let quick_cnt = configured_save_count(ctx, true);
            let path = original_save::save_file_path_with_counts(
                &ctx.project_dir,
                save_cnt,
                quick_cnt,
                SaveKind::End,
                idx,
            );
            let _ = path;
            ctx.globals.syscom.end_save_exists = true;
            ctx.request_runtime_save(RuntimeSaveKind::End, idx);
            write_global_save(ctx);
            ctx.push(Value::Int(1));
            return Ok(true);
        }
        END_LOAD => {
            let idx = p_i64(params, 0).max(0) as usize;
            ctx.request_runtime_load(RuntimeSaveKind::End, idx);
            ctx.globals.syscom.last_menu_call = END_LOAD;
        },
        INNER_SAVE => {
            let idx = p_i64(params, 0).max(0) as usize;
            ctx.globals.syscom.inner_save_exists = true;
            ctx.request_runtime_save(RuntimeSaveKind::Inner, idx);
            ctx.push(Value::Int(1));
            return Ok(true);
        }
        INNER_LOAD => {
            let idx = p_i64(params, 0).max(0) as usize;
            ctx.globals.syscom.last_menu_call = INNER_LOAD;
            if ctx.globals.syscom.inner_save_exists {
                ctx.request_runtime_load(RuntimeSaveKind::Inner, idx);
            }
            ctx.push(Value::Int(if ctx.globals.syscom.inner_save_exists {
                1
            } else {
                0
            }));
            return Ok(true);
        }
        CLEAR_INNER_SAVE => {
            let existed = ctx.globals.syscom.inner_save_exists;
            ctx.globals.syscom.inner_save_exists = false;
            ctx.push(Value::Int(if existed { 1 } else { 0 }));
            return Ok(true);
        }
        COPY_INNER_SAVE => {
            ctx.globals.syscom.inner_save_exists = true;
            ctx.push(Value::Int(1));
            return Ok(true);
        }
        CHECK_INNER_SAVE => {
            let v = if ctx.globals.syscom.inner_save_exists {
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
            let v =
                configured_save_count(ctx, false).max(ctx.globals.syscom.save_slots.len()) as i64;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_QUICK_SAVE_CNT => {
            let v = configured_save_count(ctx, true).max(ctx.globals.syscom.quick_save_slots.len())
                as i64;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_SAVE_NEW_NO => {
            let v = first_free_slot(&ctx.globals.syscom.save_slots);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_QUICK_SAVE_NEW_NO => {
            let v = first_free_slot(&ctx.globals.syscom.quick_save_slots);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_SAVE_EXIST | GET_SAVE_YEAR | GET_SAVE_MONTH | GET_SAVE_DAY | GET_SAVE_WEEKDAY
        | GET_SAVE_HOUR | GET_SAVE_MINUTE | GET_SAVE_SECOND | GET_SAVE_MILLISECOND => {
            let idx = p_i64(params, 0).max(0) as usize;
            {
                let save_cnt = configured_save_count(ctx, false);
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
            let idx = p_i64(params, 0).max(0) as usize;
            {
                let save_cnt = configured_save_count(ctx, false);
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
            let idx = p_i64(params, 0).max(0) as usize;
            let comment = params
                .get(1)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let slot = ensure_slot(&mut ctx.globals.syscom.save_slots, idx);
            slot.exist = true;
            slot.comment = comment;
            {
                let save_cnt = configured_save_count(ctx, false);
                let quick_cnt = configured_save_count(ctx, true);
                persist_slot_with_counts(
                    &ctx.project_dir,
                    false,
                    save_cnt,
                    quick_cnt,
                    &ctx.globals.syscom.save_slots,
                    idx,
                );
            }
        }
        GET_SAVE_VALUE => {
            let idx = p_i64(params, 0).max(0) as usize;
            {
                let save_cnt = configured_save_count(ctx, false);
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
            if let Some(Value::Element(chain)) = params.get(1).map(|v| v.unwrap_named()) {
                let Some(form_id) = chain.first().copied() else {
                    ctx.push(Value::Int(0));
                    return Ok(true);
                };
                let flag_index = p_i64(params, 2).max(0) as usize;
                let flag_cnt = p_i64(params, 3).max(0) as usize;
                let values: Vec<i64> = (0..flag_cnt)
                    .map(|i| {
                        ctx.globals
                            .syscom
                            .save_slots
                            .get(idx)
                            .and_then(|s| s.values.get(&(i as i32)).copied())
                            .unwrap_or(0)
                    })
                    .collect();
                let list = ctx.globals.int_lists.entry(form_id as u32).or_default();
                if list.len() < flag_index + flag_cnt {
                    list.resize(flag_index + flag_cnt, 0);
                }
                for (i, v) in values.into_iter().enumerate() {
                    list[flag_index + i] = v;
                }
                ctx.push(Value::Int(0));
                return Ok(true);
            }
            let key = p_i64(params, 1) as i32;
            let v = ctx
                .globals
                .syscom
                .save_slots
                .get(idx)
                .and_then(|s| s.values.get(&key).copied())
                .unwrap_or(0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_SAVE_VALUE => {
            let idx = p_i64(params, 0).max(0) as usize;
            if let Some(Value::Element(chain)) = params.get(1).map(|v| v.unwrap_named()) {
                let Some(form_id) = chain.first().copied() else {
                    return Ok(true);
                };
                let flag_index = p_i64(params, 2).max(0) as usize;
                let flag_cnt = p_i64(params, 3).max(0) as usize;
                let values: Vec<i64> = (0..flag_cnt)
                    .map(|i| {
                        ctx.globals
                            .int_lists
                            .get(&(form_id as u32))
                            .and_then(|list| list.get(flag_index + i).copied())
                            .unwrap_or(0)
                    })
                    .collect();
                let slot = ensure_slot(&mut ctx.globals.syscom.save_slots, idx);
                slot.exist = true;
                for (i, v) in values.into_iter().enumerate() {
                    slot.values.insert(i as i32, v);
                }
                {
                let save_cnt = configured_save_count(ctx, false);
                let quick_cnt = configured_save_count(ctx, true);
                persist_slot_with_counts(
                    &ctx.project_dir,
                    false,
                    save_cnt,
                    quick_cnt,
                    &ctx.globals.syscom.save_slots,
                    idx,
                );
            }
                return Ok(true);
            }
            let key = p_i64(params, 1) as i32;
            let val = p_i64(params, 2);
            let slot = ensure_slot(&mut ctx.globals.syscom.save_slots, idx);
            slot.exist = true;
            slot.values.insert(key, val);
            {
                let save_cnt = configured_save_count(ctx, false);
                let quick_cnt = configured_save_count(ctx, true);
                persist_slot_with_counts(
                    &ctx.project_dir,
                    false,
                    save_cnt,
                    quick_cnt,
                    &ctx.globals.syscom.save_slots,
                    idx,
                );
            }
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
            let idx = p_i64(params, 0).max(0) as usize;
            {
                let save_cnt = configured_save_count(ctx, false);
                let quick_cnt = configured_save_count(ctx, true);
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
            let idx = p_i64(params, 0).max(0) as usize;
            {
                let save_cnt = configured_save_count(ctx, false);
                let quick_cnt = configured_save_count(ctx, true);
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
            let idx = p_i64(params, 0).max(0) as usize;
            let comment = params
                .get(1)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let slot = ensure_slot(&mut ctx.globals.syscom.quick_save_slots, idx);
            slot.exist = true;
            slot.comment = comment;
            {
                let save_cnt = configured_save_count(ctx, false);
                let quick_cnt = configured_save_count(ctx, true);
                persist_slot_with_counts(
                    &ctx.project_dir,
                    true,
                    save_cnt,
                    quick_cnt,
                    &ctx.globals.syscom.quick_save_slots,
                    idx,
                );
            }
        }
        GET_QUICK_SAVE_VALUE => {
            let idx = p_i64(params, 0).max(0) as usize;
            {
                let save_cnt = configured_save_count(ctx, false);
                let quick_cnt = configured_save_count(ctx, true);
                ensure_slot_loaded_with_counts(
                    &ctx.project_dir,
                    true,
                    save_cnt,
                    quick_cnt,
                    &mut ctx.globals.syscom.quick_save_slots,
                    idx,
                );
            }
            if let Some(Value::Element(chain)) = params.get(1).map(|v| v.unwrap_named()) {
                let Some(form_id) = chain.first().copied() else {
                    ctx.push(Value::Int(0));
                    return Ok(true);
                };
                let flag_index = p_i64(params, 2).max(0) as usize;
                let flag_cnt = p_i64(params, 3).max(0) as usize;
                let values: Vec<i64> = (0..flag_cnt)
                    .map(|i| {
                        ctx.globals
                            .syscom
                            .quick_save_slots
                            .get(idx)
                            .and_then(|s| s.values.get(&(i as i32)).copied())
                            .unwrap_or(0)
                    })
                    .collect();
                let list = ctx.globals.int_lists.entry(form_id as u32).or_default();
                if list.len() < flag_index + flag_cnt {
                    list.resize(flag_index + flag_cnt, 0);
                }
                for (i, v) in values.into_iter().enumerate() {
                    list[flag_index + i] = v;
                }
                ctx.push(Value::Int(0));
                return Ok(true);
            }
            let key = p_i64(params, 1) as i32;
            let v = ctx
                .globals
                .syscom
                .quick_save_slots
                .get(idx)
                .and_then(|s| s.values.get(&key).copied())
                .unwrap_or(0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_QUICK_SAVE_VALUE => {
            let idx = p_i64(params, 0).max(0) as usize;
            if let Some(Value::Element(chain)) = params.get(1).map(|v| v.unwrap_named()) {
                let Some(form_id) = chain.first().copied() else {
                    return Ok(true);
                };
                let flag_index = p_i64(params, 2).max(0) as usize;
                let flag_cnt = p_i64(params, 3).max(0) as usize;
                let values: Vec<i64> = (0..flag_cnt)
                    .map(|i| {
                        ctx.globals
                            .int_lists
                            .get(&(form_id as u32))
                            .and_then(|list| list.get(flag_index + i).copied())
                            .unwrap_or(0)
                    })
                    .collect();
                let slot = ensure_slot(&mut ctx.globals.syscom.quick_save_slots, idx);
                slot.exist = true;
                for (i, v) in values.into_iter().enumerate() {
                    slot.values.insert(i as i32, v);
                }
                {
                    let save_cnt = configured_save_count(ctx, false);
                    let quick_cnt = configured_save_count(ctx, true);
                    persist_slot_with_counts(
                        &ctx.project_dir,
                        true,
                        save_cnt,
                        quick_cnt,
                        &ctx.globals.syscom.quick_save_slots,
                        idx,
                    );
                }
                return Ok(true);
            }
            let key = p_i64(params, 1) as i32;
            let val = p_i64(params, 2);
            let slot = ensure_slot(&mut ctx.globals.syscom.quick_save_slots, idx);
            slot.exist = true;
            slot.values.insert(key, val);
            {
                let save_cnt = configured_save_count(ctx, false);
                let quick_cnt = configured_save_count(ctx, true);
                persist_slot_with_counts(
                    &ctx.project_dir,
                    true,
                    save_cnt,
                    quick_cnt,
                    &ctx.globals.syscom.quick_save_slots,
                    idx,
                );
            }
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
            let v = if ctx.globals.syscom.end_save_exists || end_path.exists() { 1 } else { 0 };
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
        CALL_CONFIG_MENU
        | CALL_CONFIG_WINDOW_MODE_MENU
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
            set_syscom_pending_proc(ctx, SyscomPendingProcKind::OpenConfig);
            ctx.globals.syscom.last_menu_call = op;
        }
        SET_WINDOW_MODE => cfg_set_int(&mut ctx.globals.syscom, GET_WINDOW_MODE, p_i64(params, 0)),
        SET_WINDOW_MODE_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_WINDOW_MODE, 0),
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
            cfg_set_int(&mut ctx.globals.syscom, GET_WINDOW_MODE_SIZE, 0)
        }
        GET_WINDOW_MODE_SIZE => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_WINDOW_MODE_SIZE, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        CHECK_WINDOW_MODE_SIZE_ENABLE => {
            ctx.push(Value::Int(1));
            return Ok(true);
        }
        SET_ALL_VOLUME => {
            cfg_set_int(&mut ctx.globals.syscom, GET_ALL_VOLUME, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_BGM_VOLUME => {
            cfg_set_int(&mut ctx.globals.syscom, GET_BGM_VOLUME, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_KOE_VOLUME => {
            cfg_set_int(&mut ctx.globals.syscom, GET_KOE_VOLUME, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_PCM_VOLUME => {
            cfg_set_int(&mut ctx.globals.syscom, GET_PCM_VOLUME, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_SE_VOLUME => {
            cfg_set_int(&mut ctx.globals.syscom, GET_SE_VOLUME, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_MOV_VOLUME => {
            cfg_set_int(&mut ctx.globals.syscom, GET_MOV_VOLUME, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_SOUND_VOLUME => {
            cfg_set_int(&mut ctx.globals.syscom, GET_SOUND_VOLUME, p_i64(params, 0));
        }
        SET_ALL_VOLUME_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_ALL_VOLUME, 100);
            apply_audio_config(ctx);
        }
        SET_BGM_VOLUME_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_BGM_VOLUME, 100);
            apply_audio_config(ctx);
        }
        SET_KOE_VOLUME_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_KOE_VOLUME, 100);
            apply_audio_config(ctx);
        }
        SET_PCM_VOLUME_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_PCM_VOLUME, 100);
            apply_audio_config(ctx);
        }
        SET_SE_VOLUME_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_SE_VOLUME, 100);
            apply_audio_config(ctx);
        }
        SET_MOV_VOLUME_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_MOV_VOLUME, 100);
            apply_audio_config(ctx);
        }
        SET_SOUND_VOLUME_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_SOUND_VOLUME, 100),
        GET_ALL_VOLUME | GET_BGM_VOLUME | GET_KOE_VOLUME | GET_PCM_VOLUME | GET_SE_VOLUME
        | GET_MOV_VOLUME | GET_SOUND_VOLUME => {
            let v = cfg_get_int(&ctx.globals.syscom, op, 100);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_ALL_ONOFF => {
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_ALL_ONOFF,
                if p_bool(params, 0) { 1 } else { 0 },
            );
            apply_audio_config(ctx);
        }
        SET_BGM_ONOFF => {
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_BGM_ONOFF,
                if p_bool(params, 0) { 1 } else { 0 },
            );
            apply_audio_config(ctx);
        }
        SET_KOE_ONOFF => {
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_KOE_ONOFF,
                if p_bool(params, 0) { 1 } else { 0 },
            );
            apply_audio_config(ctx);
        }
        SET_PCM_ONOFF => {
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_PCM_ONOFF,
                if p_bool(params, 0) { 1 } else { 0 },
            );
            apply_audio_config(ctx);
        }
        SET_SE_ONOFF => {
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_SE_ONOFF,
                if p_bool(params, 0) { 1 } else { 0 },
            );
            apply_audio_config(ctx);
        }
        SET_MOV_ONOFF => {
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_MOV_ONOFF,
                if p_bool(params, 0) { 1 } else { 0 },
            );
            apply_audio_config(ctx);
        }
        SET_SOUND_ONOFF => {
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_SOUND_ONOFF,
                if p_bool(params, 0) { 1 } else { 0 },
            );
        }
        SET_ALL_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_ALL_ONOFF, 1);
            apply_audio_config(ctx);
        }
        SET_BGM_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_BGM_ONOFF, 1);
            apply_audio_config(ctx);
        }
        SET_KOE_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_KOE_ONOFF, 1);
            apply_audio_config(ctx);
        }
        SET_PCM_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_PCM_ONOFF, 1);
            apply_audio_config(ctx);
        }
        SET_SE_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_SE_ONOFF, 1);
            apply_audio_config(ctx);
        }
        SET_MOV_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_MOV_ONOFF, 1);
            apply_audio_config(ctx);
        }
        SET_SOUND_ONOFF_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_SOUND_ONOFF, 1),
        GET_ALL_ONOFF | GET_BGM_ONOFF | GET_KOE_ONOFF | GET_PCM_ONOFF | GET_SE_ONOFF
        | GET_MOV_ONOFF | GET_SOUND_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, op, 1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_BGMFADE_VOLUME => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_BGMFADE_VOLUME,
            p_i64(params, 0),
        ),
        SET_BGMFADE_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_BGMFADE_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_BGMFADE_VOLUME_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_BGMFADE_VOLUME, 100),
        SET_BGMFADE_ONOFF_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_BGMFADE_ONOFF, 1),
        GET_BGMFADE_VOLUME | GET_BGMFADE_ONOFF => {
            let default = if op == GET_BGMFADE_ONOFF { 1 } else { 100 };
            let v = cfg_get_int(&ctx.globals.syscom, op, default);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_KOEMODE => cfg_set_int(&mut ctx.globals.syscom, GET_KOEMODE, p_i64(params, 0)),
        SET_KOEMODE_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_KOEMODE, 0),
        GET_KOEMODE => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_KOEMODE, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_CHARAKOE_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_CHARAKOE_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_CHARAKOE_ONOFF_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_CHARAKOE_ONOFF, 1),
        GET_CHARAKOE_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_CHARAKOE_ONOFF, 1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_CHARAKOE_VOLUME => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_CHARAKOE_VOLUME,
            p_i64(params, 0),
        ),
        SET_CHARAKOE_VOLUME_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_CHARAKOE_VOLUME, 100)
        }
        GET_CHARAKOE_VOLUME => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_CHARAKOE_VOLUME, 100);
            ctx.push(Value::Int(v));
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
        SET_JITAN_SPEED => cfg_set_int(&mut ctx.globals.syscom, GET_JITAN_SPEED, p_i64(params, 0)),
        SET_JITAN_SPEED_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_JITAN_SPEED, 0),
        GET_JITAN_SPEED => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_JITAN_SPEED, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_MESSAGE_SPEED => {
            cfg_set_int(&mut ctx.globals.syscom, GET_MESSAGE_SPEED, p_i64(params, 0))
        }
        SET_MESSAGE_SPEED_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_MESSAGE_SPEED, 20),
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
            ctx.globals.script.msg_nowait = false;
            cfg_set_int(&mut ctx.globals.syscom, GET_MESSAGE_NOWAIT, 0);
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
            let v = p_i64(params, 0);
            ctx.globals.script.auto_mode_moji_wait = v;
            cfg_set_int(&mut ctx.globals.syscom, GET_AUTO_MODE_MOJI_WAIT, v);
        }
        SET_AUTO_MODE_MOJI_WAIT_DEFAULT => {
            ctx.globals.script.auto_mode_moji_wait = -1;
            cfg_set_int(&mut ctx.globals.syscom, GET_AUTO_MODE_MOJI_WAIT, -1);
        }
        GET_AUTO_MODE_MOJI_WAIT => {
            let v = ctx.globals.script.auto_mode_moji_wait;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_AUTO_MODE_MIN_WAIT => {
            let v = p_i64(params, 0);
            ctx.globals.script.auto_mode_min_wait = v;
            cfg_set_int(&mut ctx.globals.syscom, GET_AUTO_MODE_MIN_WAIT, v);
        }
        SET_AUTO_MODE_MIN_WAIT_DEFAULT => {
            ctx.globals.script.auto_mode_min_wait = -1;
            cfg_set_int(&mut ctx.globals.syscom, GET_AUTO_MODE_MIN_WAIT, -1);
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
        SET_OBJECT_DISP_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_OBJECT_DISP_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_OBJECT_DISP_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_OBJECT_DISP_ONOFF, 1)
        }
        GET_OBJECT_DISP_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_OBJECT_DISP_ONOFF, 1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_GLOBAL_EXTRA_SWITCH_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_GLOBAL_EXTRA_SWITCH_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_GLOBAL_EXTRA_SWITCH_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_GLOBAL_EXTRA_SWITCH_ONOFF, 0)
        }
        GET_GLOBAL_EXTRA_SWITCH_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_GLOBAL_EXTRA_SWITCH_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_GLOBAL_EXTRA_MODE_VALUE => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_GLOBAL_EXTRA_MODE_VALUE,
            p_i64(params, 0),
        ),
        SET_GLOBAL_EXTRA_MODE_VALUE_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_GLOBAL_EXTRA_MODE_VALUE, 0)
        }
        GET_GLOBAL_EXTRA_MODE_VALUE => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_GLOBAL_EXTRA_MODE_VALUE, 0);
            ctx.push(Value::Int(v));
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
        SET_SLEEP_ONOFF_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_SLEEP_ONOFF, 1),
        GET_SLEEP_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_SLEEP_ONOFF, 1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_NO_WIPE_ANIME_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_NO_WIPE_ANIME_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_NO_WIPE_ANIME_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_NO_WIPE_ANIME_ONOFF, 0)
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
            cfg_set_int(&mut ctx.globals.syscom, GET_SKIP_WIPE_ANIME_ONOFF, 0)
        }
        GET_SKIP_WIPE_ANIME_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_SKIP_WIPE_ANIME_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_NO_MWND_ANIME_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_NO_MWND_ANIME_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_NO_MWND_ANIME_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_NO_MWND_ANIME_ONOFF, 0)
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
            cfg_set_int(&mut ctx.globals.syscom, GET_WHEEL_NEXT_MESSAGE_ONOFF, 1)
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
            cfg_set_int(&mut ctx.globals.syscom, GET_KOE_DONT_STOP_ONOFF, 0)
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
            cfg_set_int(&mut ctx.globals.syscom, GET_SKIP_UNREAD_MESSAGE_ONOFF, 0)
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
            cfg_set_str(&mut ctx.globals.syscom, GET_FONT_NAME, v);
        }
        SET_FONT_NAME_DEFAULT => cfg_set_str(&mut ctx.globals.syscom, GET_FONT_NAME, String::new()),
        GET_FONT_NAME => {
            let v = cfg_get_str(&ctx.globals.syscom, GET_FONT_NAME);
            ctx.push(Value::Str(v));
            return Ok(true);
        }
        IS_FONT_EXIST => {
            let name = params.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let exists = font_exists(&ctx.project_dir, name);
            ctx.push(Value::Int(if exists { 1 } else { 0 }));
            return Ok(true);
        }
        SET_FONT_BOLD => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_FONT_BOLD,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_FONT_BOLD_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_FONT_BOLD, 0),
        GET_FONT_BOLD => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_FONT_BOLD, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_FONT_DECORATION => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_FONT_DECORATION,
            p_i64(params, 0),
        ),
        SET_FONT_DECORATION_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_FONT_DECORATION, 0),
        GET_FONT_DECORATION => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_FONT_DECORATION, 0);
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
            let mut img = ctx.capture_frame_rgba();
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
                let mut img = ctx.capture_frame_rgba();
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
            let mut img = ctx.capture_frame_rgba();
            if let Some((w, h)) = ctx.globals.syscom.capture_size {
                img = resize_rgba(&img, w, h);
            }
            write_rgba_png(&path, &img);
        }
        OPEN_TWEET_DIALOG => {
            log::error!("SYSCOM.OPEN_TWEET_DIALOG is not implemented in this port");
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
        _ => {
            return Ok(false);
        }
    }

    ctx.push(Value::Int(0));
    Ok(true)
}
