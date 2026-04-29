use anyhow::Result;

use crate::runtime::globals::{
    SaveSlotState, SyscomPendingProc, SyscomPendingProcKind, ToggleFeatureState, ValueFeatureState,
};
use crate::runtime::{CommandContext, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::assets::RgbaImage;

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

fn gameexe_unquoted_owned(ctx: &CommandContext, key: &str) -> String {
    ctx.tables
        .gameexe
        .as_ref()
        .and_then(|cfg| cfg.get_unquoted(key))
        .unwrap_or("")
        .to_string()
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
        GET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG => {
            if st.local_extra_switch.onoff {
                1
            } else {
                0
            }
        }
        GET_LOCAL_EXTRA_SWITCH_ENABLE_FLAG => {
            if st.local_extra_switch.enable {
                1
            } else {
                0
            }
        }
        GET_LOCAL_EXTRA_SWITCH_EXIST_FLAG => {
            if st.local_extra_switch.exist {
                1
            } else {
                0
            }
        }
        CHECK_LOCAL_EXTRA_SWITCH_ENABLE => st.local_extra_switch.check_enabled(),
        GET_LOCAL_EXTRA_MODE_VALUE => st.local_extra_mode.value,
        GET_LOCAL_EXTRA_MODE_ENABLE_FLAG => {
            if st.local_extra_mode.enable {
                1
            } else {
                0
            }
        }
        GET_LOCAL_EXTRA_MODE_EXIST_FLAG => {
            if st.local_extra_mode.exist {
                1
            } else {
                0
            }
        }
        CHECK_LOCAL_EXTRA_MODE_ENABLE => st.local_extra_mode.check_enabled(),
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
        SET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG => st.local_extra_switch.onoff = v,
        SET_LOCAL_EXTRA_SWITCH_ENABLE_FLAG => st.local_extra_switch.enable = v,
        SET_LOCAL_EXTRA_SWITCH_EXIST_FLAG => st.local_extra_switch.exist = v,
        SET_LOCAL_EXTRA_MODE_ENABLE_FLAG => st.local_extra_mode.enable = v,
        SET_LOCAL_EXTRA_MODE_EXIST_FLAG => st.local_extra_mode.exist = v,
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

fn ensure_slot(slots: &mut Vec<SaveSlotState>, idx: usize) -> &mut SaveSlotState {
    if slots.len() <= idx {
        slots.resize_with(idx + 1, SaveSlotState::default);
    }
    &mut slots[idx]
}

pub(crate) fn menu_save_slot(ctx: &mut CommandContext, quick: bool, idx: usize) {
    let slot = if quick {
        ensure_slot(&mut ctx.globals.syscom.quick_save_slots, idx)
    } else {
        ensure_slot(&mut ctx.globals.syscom.save_slots, idx)
    };
    slot.exist = true;
    set_slot_timestamp(slot);
    let path = slot_path(&save_dir(&ctx.project_dir), quick, idx);
    write_slot(&path, slot);
    if !quick {
        write_slot_thumb(ctx, idx);
    }
}

pub(crate) fn menu_load_slot(ctx: &mut CommandContext, quick: bool, idx: usize) {
    if quick {
        ensure_slot_loaded(
            &ctx.project_dir,
            true,
            &mut ctx.globals.syscom.quick_save_slots,
            idx,
        );
    } else {
        ensure_slot_loaded(
            &ctx.project_dir,
            false,
            &mut ctx.globals.syscom.save_slots,
            idx,
        );
    }
}

pub(crate) fn save_dir(project_dir: &Path) -> PathBuf {
    let cand = project_dir.join("savedata");
    if cand.is_dir() {
        return cand;
    }
    let cand = project_dir.join("save");
    if cand.is_dir() {
        return cand;
    }
    project_dir.join("savedata")
}

fn slot_path(dir: &Path, quick: bool, idx: usize) -> PathBuf {
    if quick {
        dir.join(format!("qsave_{idx}.txt"))
    } else {
        dir.join(format!("save_{idx}.txt"))
    }
}

pub(crate) fn thumb_candidate_paths(dir: &Path, idx: i64) -> [PathBuf; 2] {
    let stem = format!("{:010}", idx.max(0));
    [
        dir.join(format!("{stem}.png")),
        dir.join(format!("{stem}.bmp")),
    ]
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

fn capture_slot_thumb(ctx: &mut CommandContext) -> RgbaImage {
    const SAVE_THUMB_W: u32 = 200;
    const SAVE_THUMB_H: u32 = 150;

    if let Some(name) = pick_thumb_source_name(ctx) {
        if let Ok(img_id) = ctx.images.load_g00(&name, 0) {
            if let Some(img) = ctx.images.get(img_id) {
                return resize_rgba(img.as_ref(), SAVE_THUMB_W, SAVE_THUMB_H);
            }
        }
    }

    let img = ctx.capture_frame_rgba();
    resize_rgba(&img, SAVE_THUMB_W, SAVE_THUMB_H)
}

fn write_slot_thumb(ctx: &mut CommandContext, idx: usize) {
    let dir = save_dir(&ctx.project_dir);
    let [png_path, _bmp_path] = thumb_candidate_paths(&dir, idx as i64);
    if let Some(parent) = png_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let img = capture_slot_thumb(ctx);
    write_rgba_png(&png_path, &img);
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
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut buf = String::new();
    buf.push_str("version=1\n");
    buf.push_str(&format!("exist={}\n", if slot.exist { 1 } else { 0 }));
    buf.push_str(&format!("year={}\n", slot.year));
    buf.push_str(&format!("month={}\n", slot.month));
    buf.push_str(&format!("day={}\n", slot.day));
    buf.push_str(&format!("weekday={}\n", slot.weekday));
    buf.push_str(&format!("hour={}\n", slot.hour));
    buf.push_str(&format!("minute={}\n", slot.minute));
    buf.push_str(&format!("second={}\n", slot.second));
    buf.push_str(&format!("millisecond={}\n", slot.millisecond));
    buf.push_str(&format!("title={}\n", escape_str(&slot.title)));
    buf.push_str(&format!("message={}\n", escape_str(&slot.message)));
    buf.push_str(&format!(
        "full_message={}\n",
        escape_str(&slot.full_message)
    ));
    buf.push_str(&format!("comment={}\n", escape_str(&slot.comment)));
    buf.push_str(&format!("append_dir={}\n", escape_str(&slot.append_dir)));
    buf.push_str(&format!("append_name={}\n", escape_str(&slot.append_name)));
    for (k, v) in &slot.values {
        buf.push_str(&format!("val.{k}={v}\n"));
    }
    let _ = std::fs::write(path, buf);
}

fn read_slot(path: &Path) -> Option<SaveSlotState> {
    let data = std::fs::read_to_string(path).ok()?;
    let mut slot = SaveSlotState::default();
    for line in data.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "exist" => slot.exist = v.trim() != "0",
            "year" => slot.year = v.trim().parse().unwrap_or(0),
            "month" => slot.month = v.trim().parse().unwrap_or(0),
            "day" => slot.day = v.trim().parse().unwrap_or(0),
            "weekday" => slot.weekday = v.trim().parse().unwrap_or(0),
            "hour" => slot.hour = v.trim().parse().unwrap_or(0),
            "minute" => slot.minute = v.trim().parse().unwrap_or(0),
            "second" => slot.second = v.trim().parse().unwrap_or(0),
            "millisecond" => slot.millisecond = v.trim().parse().unwrap_or(0),
            "title" => slot.title = unescape_str(v),
            "message" => slot.message = unescape_str(v),
            "full_message" => slot.full_message = unescape_str(v),
            "comment" => slot.comment = unescape_str(v),
            "append_dir" => slot.append_dir = unescape_str(v),
            "append_name" => slot.append_name = unescape_str(v),
            _ if k.starts_with("val.") => {
                let key = k.trim_start_matches("val.").parse::<i32>().unwrap_or(0);
                let val = v.trim().parse::<i64>().unwrap_or(0);
                slot.values.insert(key, val);
            }
            _ => {}
        }
    }
    Some(slot)
}

fn ensure_slot_loaded(project_dir: &Path, quick: bool, slots: &mut Vec<SaveSlotState>, idx: usize) {
    if slots.get(idx).map(|s| s.exist).unwrap_or(false) {
        return;
    }
    let path = slot_path(&save_dir(project_dir), quick, idx);
    if let Some(slot) = read_slot(&path) {
        let s = ensure_slot(slots, idx);
        *s = slot;
    }
}

fn persist_slot(project_dir: &Path, quick: bool, slots: &[SaveSlotState], idx: usize) {
    if let Some(slot) = slots.get(idx) {
        let path = slot_path(&save_dir(project_dir), quick, idx);
        write_slot(&path, slot);
    }
}

fn remove_thumb_files(project_dir: &Path, idx: usize) {
    let dir = save_dir(project_dir);
    for p in thumb_candidate_paths(&dir, idx as i64) {
        let _ = fs::remove_file(p);
    }
}

fn copy_thumb_files(project_dir: &Path, src: usize, dst: usize) {
    let dir = save_dir(project_dir);
    let src_paths = thumb_candidate_paths(&dir, src as i64);
    let dst_paths = thumb_candidate_paths(&dir, dst as i64);
    for (src_path, dst_path) in src_paths.iter().zip(dst_paths.iter()) {
        if src_path.exists() {
            if let Some(parent) = dst_path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::copy(src_path, dst_path);
        } else {
            let _ = fs::remove_file(dst_path);
        }
    }
}

fn swap_thumb_files(project_dir: &Path, a: usize, b: usize) {
    if a == b {
        return;
    }
    let dir = save_dir(project_dir);
    let a_paths = thumb_candidate_paths(&dir, a as i64);
    let b_paths = thumb_candidate_paths(&dir, b as i64);
    for (pa, pb) in a_paths.iter().zip(b_paths.iter()) {
        let tmp = pa.with_extension(format!(
            "{}.swap",
            pa.extension().and_then(|v| v.to_str()).unwrap_or("tmp")
        ));
        let a_exists = pa.exists();
        let b_exists = pb.exists();
        if a_exists {
            let _ = fs::rename(pa, &tmp);
        }
        if b_exists {
            if let Some(parent) = pa.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::rename(pb, pa);
        } else {
            let _ = fs::remove_file(pa);
        }
        if a_exists {
            if let Some(parent) = pb.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let _ = fs::rename(&tmp, pb);
        } else {
            let _ = fs::remove_file(pb);
        }
        let _ = fs::remove_file(&tmp);
    }
}

fn copy_slot(
    project_dir: &Path,
    quick: bool,
    slots: &mut Vec<SaveSlotState>,
    src: usize,
    dst: usize,
) -> bool {
    ensure_slot_loaded(project_dir, quick, slots, src);
    let Some(src_slot) = slots.get(src).cloned() else {
        return false;
    };
    if !src_slot.exist {
        return false;
    }
    *ensure_slot(slots, dst) = src_slot;
    persist_slot(project_dir, quick, slots, dst);
    if !quick {
        copy_thumb_files(project_dir, src, dst);
    }
    true
}

fn change_slot(
    project_dir: &Path,
    quick: bool,
    slots: &mut Vec<SaveSlotState>,
    a: usize,
    b: usize,
) -> bool {
    ensure_slot_loaded(project_dir, quick, slots, a);
    ensure_slot_loaded(project_dir, quick, slots, b);
    let max_idx = a.max(b);
    if slots.len() <= max_idx {
        slots.resize_with(max_idx + 1, SaveSlotState::default);
    }
    slots.swap(a, b);
    persist_slot(project_dir, quick, slots, a);
    persist_slot(project_dir, quick, slots, b);
    if !quick {
        swap_thumb_files(project_dir, a, b);
    }
    true
}

fn delete_slot(
    project_dir: &Path,
    quick: bool,
    slots: &mut Vec<SaveSlotState>,
    idx: usize,
) -> bool {
    ensure_slot_loaded(project_dir, quick, slots, idx);
    let existed = slots.get(idx).map(|s| s.exist).unwrap_or(false);
    *ensure_slot(slots, idx) = SaveSlotState::default();
    let path = slot_path(&save_dir(project_dir), quick, idx);
    let _ = fs::remove_file(path);
    if !quick {
        remove_thumb_files(project_dir, idx);
    }
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
    for (i, entry) in st.history.iter().enumerate() {
        out.push_str(&format!("-- entry {} --\n", i));
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

fn configured_save_count(ctx: &CommandContext, quick: bool) -> usize {
    let key = if quick { "QUICK_SAVE.CNT" } else { "SAVE.CNT" };
    let default_count = if quick { 3 } else { 10 };
    ctx.tables
        .gameexe
        .as_ref()
        .and_then(|cfg| cfg.get_usize(key))
        .unwrap_or(default_count)
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

fn write_rgba_png(path: &Path, img: &RgbaImage) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Some(buf) = image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone()) {
        let _ = buf.save(path);
    }
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
        if let Some(v) = get_toggle_get(op, st) {
            ctx.push(Value::Int(v));
            return Ok(true);
        }
    }
    {
        let st = &mut ctx.globals.syscom;
        if apply_toggle_set(op, p_bool(params, 0), st) {
            ctx.push(Value::Int(0));
            return Ok(true);
        }
    }

    match op {
        CALL_EX => {
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        CALL_SYSCOM_MENU => {
            ctx.globals.syscom.menu_open = true;
            ctx.globals.syscom.menu_kind = Some(CALL_SYSCOM_MENU);
            ctx.globals.syscom.menu_result = None;
            ctx.globals.syscom.menu_cursor = 0;
            ctx.globals.syscom.last_menu_call = CALL_SYSCOM_MENU;
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        SET_SYSCOM_MENU_ENABLE => ctx.globals.syscom.syscom_menu_disable = false,
        SET_SYSCOM_MENU_DISABLE => ctx.globals.syscom.syscom_menu_disable = true,
        SET_MWND_BTN_ENABLE => {
            if params.is_empty() {
                ctx.globals.syscom.mwnd_btn_disable_all = false;
            } else {
                ctx.globals
                    .syscom
                    .mwnd_btn_disable
                    .insert(p_i64(params, 0), false);
            }
        }
        SET_MWND_BTN_DISABLE => {
            if params.is_empty() {
                ctx.globals.syscom.mwnd_btn_disable_all = true;
            } else {
                ctx.globals
                    .syscom
                    .mwnd_btn_disable
                    .insert(p_i64(params, 0), true);
            }
        }
        SET_MWND_BTN_TOUCH_ENABLE => ctx.globals.syscom.mwnd_btn_touch_disable = false,
        SET_MWND_BTN_TOUCH_DISABLE => ctx.globals.syscom.mwnd_btn_touch_disable = true,
        INIT_SYSCOM_FLAG => {
            ctx.globals.syscom.read_skip = ToggleFeatureState::default();
            ctx.globals.syscom.auto_skip = ToggleFeatureState::default();
            ctx.globals.syscom.auto_mode = ToggleFeatureState::default();
            ctx.globals.syscom.hide_mwnd = ToggleFeatureState::default();
            ctx.globals.syscom.local_extra_switch = ToggleFeatureState::default();
            ctx.globals.syscom.local_extra_mode = ValueFeatureState::default();
            ctx.globals.syscom.msg_back = ToggleFeatureState::default();
            ctx.globals.syscom.return_to_sel = ToggleFeatureState::default();
            ctx.globals.syscom.return_to_menu = ToggleFeatureState::default();
            ctx.globals.syscom.end_game = ToggleFeatureState::default();
            ctx.globals.syscom.save_feature = ToggleFeatureState::default();
            ctx.globals.syscom.load_feature = ToggleFeatureState::default();
            ctx.globals.syscom.msg_back_open = false;
        }
        SET_LOCAL_EXTRA_MODE_VALUE => ctx.globals.syscom.local_extra_mode.value = p_i64(params, 0),
        OPEN_MSG_BACK => {
            ctx.globals.syscom.msg_back_open = true;
            ctx.globals.syscom.last_menu_call = OPEN_MSG_BACK;
        }
        CLOSE_MSG_BACK => {
            ctx.globals.syscom.msg_back_open = false;
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
            ctx.globals.syscom.last_menu_call = END_GAME;
            ctx.globals.system.active_flag = false;
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
        SET_TOTAL_PLAY_TIME => ctx.globals.syscom.total_play_time = p_i64(params, 0),
        CALL_SAVE_MENU => {
            ctx.globals.syscom.menu_open = true;
            ctx.globals.syscom.menu_kind = Some(CALL_SAVE_MENU);
            ctx.globals.syscom.menu_result = None;
            ctx.globals.syscom.menu_cursor = 0;
            ctx.globals.syscom.last_menu_call = CALL_SAVE_MENU;
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        CALL_LOAD_MENU => {
            ctx.globals.syscom.menu_open = true;
            ctx.globals.syscom.menu_kind = Some(CALL_LOAD_MENU);
            ctx.globals.syscom.menu_result = None;
            ctx.globals.syscom.menu_cursor = 0;
            ctx.globals.syscom.last_menu_call = CALL_LOAD_MENU;
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        SAVE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let default_title = ctx.globals.syscom.current_save_scene_title.clone();
            let default_message = ctx.globals.syscom.current_save_message.clone();
            let slot = ensure_slot(&mut ctx.globals.syscom.save_slots, idx);
            slot.exist = true;
            set_slot_timestamp(slot);
            if slot.title.is_empty() {
                slot.title = default_title;
            }
            if slot.message.is_empty() {
                slot.message = default_message.clone();
            }
            if slot.full_message.is_empty() {
                slot.full_message = default_message;
            }
            persist_slot(&ctx.project_dir, false, &ctx.globals.syscom.save_slots, idx);
            write_slot_thumb(ctx, idx);
            ctx.push(Value::Int(1));
            return Ok(true);
        }
        LOAD => {
            let idx = p_i64(params, 0).max(0) as usize;
            ensure_slot_loaded(
                &ctx.project_dir,
                false,
                &mut ctx.globals.syscom.save_slots,
                idx,
            );
            ctx.globals.syscom.last_menu_call = LOAD;
        }
        QUICK_SAVE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let default_title = ctx.globals.syscom.current_save_scene_title.clone();
            let default_message = ctx.globals.syscom.current_save_message.clone();
            let slot = ensure_slot(&mut ctx.globals.syscom.quick_save_slots, idx);
            slot.exist = true;
            set_slot_timestamp(slot);
            if slot.title.is_empty() {
                slot.title = default_title;
            }
            if slot.message.is_empty() {
                slot.message = default_message.clone();
            }
            if slot.full_message.is_empty() {
                slot.full_message = default_message;
            }
            persist_slot(
                &ctx.project_dir,
                true,
                &ctx.globals.syscom.quick_save_slots,
                idx,
            );
            ctx.push(Value::Int(1));
            return Ok(true);
        }
        QUICK_LOAD => {
            let idx = p_i64(params, 0).max(0) as usize;
            ensure_slot_loaded(
                &ctx.project_dir,
                true,
                &mut ctx.globals.syscom.quick_save_slots,
                idx,
            );
            ctx.globals.syscom.last_menu_call = QUICK_LOAD;
        }
        END_SAVE => {
            ctx.globals.syscom.end_save_exists = true;
            ctx.push(Value::Int(1));
            return Ok(true);
        }
        END_LOAD => ctx.globals.syscom.last_menu_call = END_LOAD,
        INNER_SAVE => {
            ctx.globals.syscom.inner_save_exists = true;
            ctx.push(Value::Int(1));
            return Ok(true);
        }
        INNER_LOAD => {
            ctx.globals.syscom.last_menu_call = INNER_LOAD;
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
            ensure_slot_loaded(
                &ctx.project_dir,
                false,
                &mut ctx.globals.syscom.save_slots,
                idx,
            );
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
            ensure_slot_loaded(
                &ctx.project_dir,
                false,
                &mut ctx.globals.syscom.save_slots,
                idx,
            );
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
            persist_slot(&ctx.project_dir, false, &ctx.globals.syscom.save_slots, idx);
        }
        GET_SAVE_VALUE => {
            let idx = p_i64(params, 0).max(0) as usize;
            ensure_slot_loaded(
                &ctx.project_dir,
                false,
                &mut ctx.globals.syscom.save_slots,
                idx,
            );
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
                persist_slot(&ctx.project_dir, false, &ctx.globals.syscom.save_slots, idx);
                return Ok(true);
            }
            let key = p_i64(params, 1) as i32;
            let val = p_i64(params, 2);
            let slot = ensure_slot(&mut ctx.globals.syscom.save_slots, idx);
            slot.exist = true;
            slot.values.insert(key, val);
            persist_slot(&ctx.project_dir, false, &ctx.globals.syscom.save_slots, idx);
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
            ensure_slot_loaded(
                &ctx.project_dir,
                true,
                &mut ctx.globals.syscom.quick_save_slots,
                idx,
            );
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
            ensure_slot_loaded(
                &ctx.project_dir,
                true,
                &mut ctx.globals.syscom.quick_save_slots,
                idx,
            );
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
            persist_slot(
                &ctx.project_dir,
                true,
                &ctx.globals.syscom.quick_save_slots,
                idx,
            );
        }
        GET_QUICK_SAVE_VALUE => {
            let idx = p_i64(params, 0).max(0) as usize;
            ensure_slot_loaded(
                &ctx.project_dir,
                true,
                &mut ctx.globals.syscom.quick_save_slots,
                idx,
            );
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
                persist_slot(
                    &ctx.project_dir,
                    true,
                    &ctx.globals.syscom.quick_save_slots,
                    idx,
                );
                return Ok(true);
            }
            let key = p_i64(params, 1) as i32;
            let val = p_i64(params, 2);
            let slot = ensure_slot(&mut ctx.globals.syscom.quick_save_slots, idx);
            slot.exist = true;
            slot.values.insert(key, val);
            persist_slot(
                &ctx.project_dir,
                true,
                &ctx.globals.syscom.quick_save_slots,
                idx,
            );
        }
        GET_END_SAVE_EXIST => {
            let v = if ctx.globals.syscom.end_save_exists {
                1
            } else {
                0
            };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        COPY_SAVE => {
            let src = p_i64(params, 0).max(0) as usize;
            let dst = p_i64(params, 1).max(0) as usize;
            let ok = copy_slot(
                &ctx.project_dir,
                false,
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
            let ok = copy_slot(
                &ctx.project_dir,
                true,
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
            let ok = change_slot(
                &ctx.project_dir,
                false,
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
            let ok = change_slot(
                &ctx.project_dir,
                true,
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
            let ok = delete_slot(
                &ctx.project_dir,
                false,
                &mut ctx.globals.syscom.save_slots,
                idx,
            );
            ctx.globals.syscom.last_menu_call = op;
            ctx.push(Value::Int(if ok { 1 } else { 0 }));
            return Ok(true);
        }
        DELETE_QUICK_SAVE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let ok = delete_slot(
                &ctx.project_dir,
                true,
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
            ctx.globals.syscom.menu_open = true;
            ctx.globals.syscom.menu_kind = Some(op);
            ctx.globals.syscom.menu_result = None;
            ctx.globals.syscom.menu_cursor = 0;
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
            cfg_set_int(&mut ctx.globals.syscom, GET_MOUSE_CURSOR_HIDE_ONOFF, 0)
        }
        GET_MOUSE_CURSOR_HIDE_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_MOUSE_CURSOR_HIDE_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_MOUSE_CURSOR_HIDE_TIME => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_MOUSE_CURSOR_HIDE_TIME,
            p_i64(params, 0),
        ),
        SET_MOUSE_CURSOR_HIDE_TIME_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_MOUSE_CURSOR_HIDE_TIME, 0)
        }
        GET_MOUSE_CURSOR_HIDE_TIME => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_MOUSE_CURSOR_HIDE_TIME, 0);
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
        SET_FILTER_COLOR_R_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_FILTER_COLOR_R, 0),
        SET_FILTER_COLOR_G_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_FILTER_COLOR_G, 0),
        SET_FILTER_COLOR_B_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_FILTER_COLOR_B, 0),
        SET_FILTER_COLOR_A_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_FILTER_COLOR_A, 0),
        GET_FILTER_COLOR_R | GET_FILTER_COLOR_G | GET_FILTER_COLOR_B | GET_FILTER_COLOR_A => {
            let v = cfg_get_int(&ctx.globals.syscom, op, 0);
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
