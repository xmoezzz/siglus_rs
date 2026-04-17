//! Global Stage form handler aligned to the original C++ Stage/Object/MWND/Group/BTNSELITEM split.
//!
//! This module uses explicit selector and operation dispatch only.

use anyhow::Result;

use std::path::{Path, PathBuf};

use crate::image_manager::ImageId;
use crate::layer::{SpriteFit, SpriteId, SpriteSizeMode};
use crate::mesh3d::load_mesh_asset;
use crate::runtime::constants;
use crate::runtime::globals::{
    BtnSelItemState, GroupListOpKind, GroupOpKind, MsgBackState, MwndListOpKind, MwndOpKind,
    MwndSelectionChoice, MwndSelectionState, ObjectBackend, ObjectEventTarget,
    ObjectFrameActionState, ObjectListOpKind, ObjectOpKind, ObjectState, ObjectWeatherParam,
    StageFormState, WorldState, OBJECT_NESTED_SLOT_KEY,
};
use crate::runtime::int_event::IntEvent;
use crate::runtime::Value;

use super::super::CommandContext;
use super::prop_access;
use super::syscom;

fn is_stage_form_id(ctx: &CommandContext, form_id: i32) -> bool {
    let primary = ctx.ids.form_global_stage as i32;
    let canonical = crate::runtime::forms::codes::FORM_GLOBAL_STAGE as i32;
    let primary_local = if primary != 0 { primary ^ 0x4000 } else { 0 };
    let canonical_local = canonical ^ 0x4000;
    form_id == primary
        || form_id == canonical
        || form_id == primary_local
        || form_id == canonical_local
        || form_id == crate::runtime::constants::global_form::STAGE_DEFAULT as i32
        || form_id == crate::runtime::constants::global_form::STAGE_ALIAS_37 as i32
        || form_id == crate::runtime::constants::global_form::STAGE_ALIAS_38 as i32
}

#[derive(Debug, Clone)]
enum StageTarget {
    StageCount,
    StageOp {
        stage: i64,
        op: i64,
    },
    ChildListOp {
        stage: i64,
        child: i32,
        op: i64,
    },
    ChildItemOp {
        stage: i64,
        child: i32,
        idx: i64,
        op: i64,
        tail: Vec<i32>,
    },
    ChildItemRef {
        stage: i64,
        child: i32,
        idx: i64,
    },
}

fn load_thumb_image_id(ctx: &mut CommandContext, idx: i64) -> Option<ImageId> {
    let dir = ctx.project_dir.join("savedata");
    for path in super::syscom::thumb_candidate_paths(&dir, idx) {
        if path.exists() {
            if let Ok(img_id) = ctx.images.load_file(&path, 0) {
                return Some(img_id);
            }
        }
    }
    None
}

fn parse_mwnd_selection_args(
    script_args: &[Value],
    rhs: Option<&Value>,
) -> Vec<MwndSelectionChoice> {
    fn push_choice(out: &mut Vec<MwndSelectionChoice>, v: &Value) {
        match v.unwrap_named() {
            Value::Str(s) => out.push(MwndSelectionChoice {
                text: s.clone(),
                kind: 0,
                color: 0,
            }),
            Value::List(items) if !items.is_empty() => {
                let text = items
                    .first()
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if text.is_empty() {
                    return;
                }
                let kind = items.get(1).and_then(Value::as_i64).unwrap_or(0);
                let color = items.get(2).and_then(Value::as_i64).unwrap_or(0);
                out.push(MwndSelectionChoice { text, kind, color });
            }
            _ => {}
        }
    }

    let mut out = Vec::new();
    if let Some(v) = rhs {
        push_choice(&mut out, v);
    }
    for v in script_args {
        push_choice(&mut out, v);
    }
    out
}

fn parse_target(ctx: &CommandContext, chain: &[i32]) -> Option<StageTarget> {
    if chain.is_empty() || !is_stage_form_id(ctx, chain[0]) {
        return None;
    }
    if chain.len() == 1 {
        return Some(StageTarget::StageCount);
    }
    let elm_array = if ctx.ids.elm_array != 0 {
        ctx.ids.elm_array
    } else {
        crate::runtime::forms::codes::ELM_ARRAY
    };
    let stage_object = if ctx.ids.stage_elm_object != 0 {
        ctx.ids.stage_elm_object
    } else {
        crate::runtime::forms::codes::STAGE_ELM_OBJECT
    };
    let objectlist_get_size = constants::OBJECTLIST_GET_SIZE;
    let objectlist_resize = constants::OBJECTLIST_RESIZE;
    if chain.len() < 4 || chain[1] != elm_array {
        // Same-version decomp-confirmed testcase shape:
        // [FORM_STAGE_ALIAS, child_code, ELM_ARRAY, stage_idx, ...]
        if chain.len() >= 4 && chain[2] == elm_array {
            let child = chain[1] as i32;
            let stage = chain[3] as i64;
            if chain.len() == 4 {
                return Some(StageTarget::ChildListOp {
                    stage,
                    child,
                    op: 0,
                });
            }
            if child == stage_object && chain.len() >= 5 {
                let op = chain[4] as i64;
                let tail = chain.get(5..).unwrap_or(&[]).to_vec();
                if chain.len() == 5
                    && (op as i32 == objectlist_get_size || op as i32 == objectlist_resize)
                {
                    return Some(StageTarget::ChildListOp { stage, child, op });
                }
                return Some(StageTarget::ChildItemOp {
                    stage,
                    child,
                    idx: 0,
                    op,
                    tail,
                });
            }
            if chain.len() == 5 {
                return Some(StageTarget::ChildListOp {
                    stage,
                    child,
                    op: chain[4] as i64,
                });
            }
            if chain.len() >= 7 && chain[5] == elm_array {
                return Some(StageTarget::ChildItemOp {
                    stage,
                    child,
                    idx: chain[6] as i64,
                    op: chain[4] as i64,
                    tail: chain.get(7..).unwrap_or(&[]).to_vec(),
                });
            }
        }
        return None;
    }
    let stage = chain[2] as i64;
    if chain.len() == 4 {
        return Some(StageTarget::StageOp {
            stage,
            op: chain[3] as i64,
        });
    }
    let child = chain[3] as i32;
    if chain.len() == 5 {
        return Some(StageTarget::ChildListOp {
            stage,
            child,
            op: chain[4] as i64,
        });
    }
    if chain.len() >= 7 && chain[4] == elm_array {
        return Some(StageTarget::ChildItemOp {
            stage,
            child,
            idx: chain[5] as i64,
            op: chain[6] as i64,
            tail: chain.get(7..).unwrap_or(&[]).to_vec(),
        });
    }
    if chain.len() == 6 && chain[4] == elm_array {
        return Some(StageTarget::ChildItemRef {
            stage,
            child,
            idx: chain[5] as i64,
        });
    }
    None
}

fn as_i64(v: &Value) -> Option<i64> {
    v.as_i64()
}

fn as_str(v: &Value) -> Option<&str> {
    v.as_str()
}

fn sg_debug_enabled_local() -> bool {
    std::env::var_os("SG_DEBUG").is_some()
}

fn sg_debug_stage(msg: impl AsRef<str>) {
    if sg_debug_enabled_local() {
        eprintln!("[SG_DEBUG][STAGE] {}", msg.as_ref());
    }
}

fn default_for_ret_form(ret_form: i64) -> Value {
    if prop_access::ret_form_is_string(ret_form) {
        Value::Str(String::new())
    } else {
        Value::Int(0)
    }
}

fn push_ok(ctx: &mut CommandContext, ret_form: Option<i64>) {
    match ret_form {
        Some(0) | None => ctx.stack.push(Value::Int(0)),
        Some(rf) => ctx.stack.push(default_for_ret_form(rf)),
    }
}

fn dispatch_int_event_like(
    ev: &mut IntEvent,
    params: &[Value],
    ret_form: Option<i64>,
) -> Option<Value> {
    match params.len() {
        0 => {
            if ret_form.unwrap_or(0) != 0 {
                return Some(Value::Int(if ev.check_event() { 1 } else { 0 }));
            }
            ev.end_event();
            return Some(Value::Int(0));
        }
        4 => {
            let value = params.get(0).and_then(as_i64).unwrap_or(0) as i32;
            let total_time = params.get(1).and_then(as_i64).unwrap_or(0) as i32;
            let delay_time = params.get(2).and_then(as_i64).unwrap_or(0) as i32;
            let speed_type = params.get(3).and_then(as_i64).unwrap_or(0) as i32;
            ev.set_event(value, total_time, delay_time, speed_type, 0);
            return Some(Value::Int(0));
        }
        5 => {
            let start_value = params.get(0).and_then(as_i64).unwrap_or(0) as i32;
            let end_value = params.get(1).and_then(as_i64).unwrap_or(0) as i32;
            let loop_time = params.get(2).and_then(as_i64).unwrap_or(0) as i32;
            let delay_time = params.get(3).and_then(as_i64).unwrap_or(0) as i32;
            let speed_type = params.get(4).and_then(as_i64).unwrap_or(0) as i32;
            ev.loop_event(start_value, end_value, loop_time, delay_time, speed_type, 0);
            return Some(Value::Int(0));
        }
        _ => {}
    }
    None
}

fn try_set_ui_bg_from_name(ctx: &mut CommandContext, name: &str) {
    if name.is_empty() {
        return;
    }

    // Conservative: try direct file, then g00, then bg.
    if ctx
        .images
        .load_file(Path::new(name), 0)
        .map(|id| {
            ctx.ui.set_message_bg(id);
        })
        .is_ok()
    {
        return;
    }
    if ctx
        .images
        .load_g00(name, 0)
        .map(|id| {
            ctx.ui.set_message_bg(id);
        })
        .is_ok()
    {
        return;
    }
    let _ = ctx.images.load_bg(name).map(|id| {
        ctx.ui.set_message_bg(id);
    });
}

fn try_set_ui_filter_from_name(ctx: &mut CommandContext, name: &str) {
    if name.is_empty() {
        ctx.ui.set_message_filter(None);
        return;
    }
    if let Some(path) = resolve_filter_path(&ctx.project_dir, name) {
        if let Ok(id) = ctx.images.load_file(&path, 0) {
            ctx.ui.set_message_filter(Some(id));
            return;
        }
    }
    ctx.ui.set_message_filter(None);
}

fn stage_state_mut(ctx: &mut CommandContext, form_id: u32) -> &mut StageFormState {
    ctx.globals.stage_forms.entry(form_id).or_default()
}

fn with_stage_state<R>(
    ctx: &mut CommandContext,
    form_id: u32,
    f: impl FnOnce(&mut CommandContext, &mut StageFormState) -> R,
) -> R {
    let mut st = ctx.globals.stage_forms.remove(&form_id).unwrap_or_default();
    let r = f(ctx, &mut st);
    ctx.globals.stage_forms.insert(form_id, st);
    r
}

fn msgbk_state_mut(ctx: &mut CommandContext) -> Option<&mut MsgBackState> {
    let form_id = ctx.ids.form_global_msgbk;
    if form_id == 0 {
        return None;
    }
    Some(ctx.globals.msgbk_forms.entry(form_id).or_default())
}

fn msgbk_scene_line(ctx: &CommandContext) -> (i64, i64) {
    let scn_no = ctx.current_scene_no.unwrap_or(-1);
    let line_no = if ctx.current_line_no > 0 {
        ctx.current_line_no
    } else {
        -1
    };
    (scn_no, line_no)
}

fn msgbk_add_text(ctx: &mut CommandContext, s: &str) {
    if s.is_empty() {
        return;
    }
    let (scn_no, line_no) = msgbk_scene_line(ctx);
    let Some(st) = msgbk_state_mut(ctx) else {
        return;
    };
    st.add_msg(s, s, scn_no, line_no);
}

fn msgbk_add_name(ctx: &mut CommandContext, s: &str) {
    let (scn_no, line_no) = msgbk_scene_line(ctx);
    let Some(st) = msgbk_state_mut(ctx) else {
        return;
    };
    st.add_name(s, s, scn_no, line_no);
}

fn msgbk_add_koe(ctx: &mut CommandContext, koe_no: i64, chara_no: i64) {
    let (scn_no, line_no) = msgbk_scene_line(ctx);
    let Some(st) = msgbk_state_mut(ctx) else {
        return;
    };
    st.add_koe(koe_no, chara_no, scn_no, line_no);
}

fn msgbk_next(ctx: &mut CommandContext) {
    let Some(st) = msgbk_state_mut(ctx) else {
        return;
    };
    st.next();
}

fn ensure_group(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    group_idx: usize,
) {
    let _ = ctx;
    st.ensure_group_list(stage_idx, group_idx + 1);
    let list = st.group_lists.get_mut(&stage_idx).unwrap();
    let g = &mut list[group_idx];
    // On first touch, initialize to the same "empty" defaults used by the original engine.
    if g.hit_button_no == 0
        && g.pushed_button_no == 0
        && g.decided_button_no == 0
        && g.result == 0
        && g.result_button_no == 0
    {
        g.init_sel();
        g.order = 0;
        g.layer = 0;
        g.cancel_priority = 0;
    }
}

fn ensure_mwnd(ctx: &mut CommandContext, st: &mut StageFormState, stage_idx: i64, mwnd_idx: usize) {
    st.ensure_mwnd_list(stage_idx, mwnd_idx + 1);
    let _ = ctx;
    let _ = st;
}

fn ensure_btnselitem(
    _ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    item_idx: usize,
) {
    let list = st.btnselitem_lists.entry(stage_idx).or_default();
    if list.len() <= item_idx {
        list.resize_with(item_idx + 1, BtnSelItemState::default);
    }
}

fn next_embedded_object_slot(st: &mut StageFormState, stage_idx: i64, key: &str) -> usize {
    let full = format!("{stage_idx}:{key}");
    if let Some(&v) = st.embedded_object_slots.get(&full) {
        return v;
    }
    let base_len = st
        .object_lists
        .get(&stage_idx)
        .map(|v| v.len())
        .unwrap_or(0);
    let next_entry = st
        .next_embedded_object_slot
        .entry(stage_idx)
        .or_insert(base_len);
    if *next_entry < base_len {
        *next_entry = base_len;
    }
    let slot = *next_entry;
    *next_entry += 1;
    st.embedded_object_slots.insert(full, slot);
    slot
}

// -----------------------------------------------------------------------------
// OBJECT / OBJECTLIST
// -----------------------------------------------------------------------------

fn resolve_object_list_op(op: i32) -> ObjectListOpKind {
    if op == crate::runtime::forms::codes::OBJECTLIST_GET_SIZE {
        ObjectListOpKind::GetSize
    } else if op == crate::runtime::forms::codes::OBJECTLIST_RESIZE {
        ObjectListOpKind::Resize
    } else {
        ObjectListOpKind::Unknown
    }
}

fn dispatch_object_list_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    op: i32,
    script_args: &[Value],
    ret_form: Option<i64>,
) -> bool {
    let k = resolve_object_list_op(op);
    match k {
        ObjectListOpKind::GetSize => {
            ctx.stack
                .push(Value::Int(st.object_list_len(stage_idx) as i64));
            true
        }
        ObjectListOpKind::Resize => {
            let Some(n0) = script_args.get(0).and_then(as_i64) else {
                push_ok(ctx, ret_form);
                return true;
            };
            let n = if n0 < 0 { 0 } else { n0 as usize };
            sg_debug_stage(format!("stage={} OBJECTLIST_RESIZE {}", stage_idx, n));

            let old_len = st.object_list_len(stage_idx);
            if n < old_len {
                if let Some(list) = st.object_lists.get_mut(&stage_idx) {
                    for i in n..old_len {
                        let obj = &mut list[i];
                        object_clear_backend(ctx, obj, stage_idx, i);
                        *obj = ObjectState::default();
                    }
                }
            }

            st.set_object_list_len_strict(stage_idx, n);
            ctx.stack.push(Value::Int(0));
            true
        }
        ObjectListOpKind::Unknown => false,
    }
}

fn dispatch_embedded_object_list_op(
    ctx: &mut CommandContext,
    list: &mut Vec<ObjectState>,
    strict: &mut bool,
    op: i32,
    script_args: &[Value],
    ret_form: Option<i64>,
) -> Option<bool> {
    if op == crate::runtime::forms::codes::OBJECTLIST_GET_SIZE {
        ctx.stack.push(Value::Int(list.len() as i64));
        return Some(true);
    }
    if op == crate::runtime::forms::codes::OBJECTLIST_RESIZE {
        let n = script_args.first().and_then(as_i64).unwrap_or(0).max(0) as usize;
        if n < list.len() {
            list.truncate(n);
        } else if n > list.len() {
            list.resize_with(n, ObjectState::default);
        }
        *strict = true;
        push_ok(ctx, ret_form);
        return Some(true);
    }
    None
}

fn dispatch_embedded_object_item_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    list: &mut Vec<ObjectState>,
    strict: bool,
    child_idx: i64,
    op: i32,
    tail: &[i32],
    script_args: &[Value],
    ret_form: Option<i64>,
    rhs: Option<&Value>,
    al_id: Option<i64>,
    slot_key: &str,
) -> bool {
    if child_idx < 0 {
        match ret_form {
            Some(rf) => ctx.stack.push(default_for_ret_form(rf)),
            None => ctx.stack.push(Value::Int(0)),
        }
        return true;
    }
    let idx = child_idx as usize;
    if idx >= list.len() {
        if strict {
            match ret_form {
                Some(rf) => ctx.stack.push(default_for_ret_form(rf)),
                None => ctx.stack.push(Value::Int(0)),
            }
            return true;
        }
        list.resize_with(idx + 1, ObjectState::default);
    }
    let slot = next_embedded_object_slot(st, stage_idx, slot_key);
    if !ensure_object_for_access(st, stage_idx, slot) {
        match ret_form {
            Some(rf) => ctx.stack.push(default_for_ret_form(rf)),
            None => ctx.stack.push(Value::Int(0)),
        }
        return true;
    }
    let child_snapshot = std::mem::take(&mut list[idx]);
    let slot_snapshot = {
        let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
        std::mem::take(&mut stage_list[slot])
    };
    {
        let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
        stage_list[slot] = child_snapshot;
    }
    let handled = dispatch_object_op(
        ctx,
        st,
        stage_idx,
        slot as i64,
        op,
        tail,
        script_args,
        ret_form,
        rhs,
        al_id,
    );
    let child_after = {
        let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
        std::mem::take(&mut stage_list[slot])
    };
    {
        let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
        stage_list[slot] = slot_snapshot;
    }
    list[idx] = child_after;
    handled
}

fn ensure_object_for_access(st: &mut StageFormState, stage_idx: i64, obj_idx: usize) -> bool {
    let strict = st
        .object_list_strict
        .get(&stage_idx)
        .copied()
        .unwrap_or(false);
    let entry = st.object_lists.entry(stage_idx).or_default();
    if entry.len() <= obj_idx {
        if strict {
            return false;
        }
        entry.extend((0..(obj_idx + 1 - entry.len())).map(|_| ObjectState::default()));
    }
    true
}

fn nested_object_slot(st: &mut StageFormState, stage_idx: i64, obj: &mut ObjectState) -> usize {
    let next_entry = st
        .next_nested_object_slot
        .entry(stage_idx)
        .or_insert(100000);
    obj.ensure_runtime_slot(next_entry)
}

fn ensure_rect_layer(ctx: &mut CommandContext, st: &mut StageFormState, stage_idx: i64) -> usize {
    if let Some(&id) = st.rect_layers.get(&stage_idx) {
        return id;
    }
    let id = ctx.layers.create_layer();
    st.rect_layers.insert(stage_idx, id);
    id
}

fn object_clear_backend(
    ctx: &mut CommandContext,
    obj: &mut ObjectState,
    stage_idx: i64,
    obj_idx: usize,
) {
    if let Some(id) = obj.movie.audio_id.take() {
        ctx.movie.stop_audio(id);
    }
    if matches!(obj.backend, ObjectBackend::Gfx) {
        let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
        let _ = gfx.object_clear(images, layers, stage_idx, obj_idx as i64);
    }
    match obj.backend {
        ObjectBackend::Rect {
            layer_id,
            sprite_id,
            ..
        }
        | ObjectBackend::String {
            layer_id,
            sprite_id,
            ..
        }
        | ObjectBackend::Movie {
            layer_id,
            sprite_id,
            ..
        } => {
            if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                if let Some(spr) = layer.sprite_mut(sprite_id) {
                    spr.visible = false;
                    spr.image_id = None;
                }
            }
        }
        ObjectBackend::Number {
            layer_id,
            ref sprite_ids,
        } => {
            if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                for &sid in sprite_ids {
                    if let Some(spr) = layer.sprite_mut(sid) {
                        spr.visible = false;
                        spr.image_id = None;
                    }
                }
            }
        }
        _ => {}
    }
    obj.backend = ObjectBackend::None;
}

fn bind_capture_backend(
    ctx: &mut CommandContext,
    obj: &mut ObjectState,
    stage_idx: i64,
    img_id: ImageId,
) {
    let Some(img) = ctx.images.get(img_id) else {
        return;
    };
    let Some(layer_id) = ctx.gfx.ensure_stage_layer_id(&mut ctx.layers, stage_idx) else {
        return;
    };
    let Some(layer) = ctx.layers.layer_mut(layer_id) else {
        return;
    };
    let sprite_id = layer.create_sprite();
    if let Some(spr) = layer.sprite_mut(sprite_id) {
        spr.visible = true;
        spr.image_id = Some(img_id);
        spr.fit = SpriteFit::PixelRect;
        spr.size_mode = SpriteSizeMode::Intrinsic;
        if ctx.ids.obj_x != 0 {
            spr.x = obj.lookup_int_prop(&ctx.ids, ctx.ids.obj_x).unwrap_or(0) as i32;
        }
        if ctx.ids.obj_y != 0 {
            spr.y = obj.lookup_int_prop(&ctx.ids, ctx.ids.obj_y).unwrap_or(0) as i32;
        }
        if ctx.ids.obj_alpha != 0 {
            spr.alpha = obj
                .lookup_int_prop(&ctx.ids, ctx.ids.obj_alpha)
                .unwrap_or(255)
                .clamp(0, 255) as u8;
        }
        if ctx.ids.obj_order != 0 {
            spr.order = obj
                .lookup_int_prop(&ctx.ids, ctx.ids.obj_order)
                .unwrap_or(0) as i32;
        }
    }
    obj.backend = ObjectBackend::Rect {
        layer_id,
        sprite_id,
        width: img.width,
        height: img.height,
    };
}

const TNM_SCALE_UNIT: i64 = 1000;
const TNM_BTN_STATE_NORMAL: i64 = 0;
const TNM_BTN_STATE_HIT: i64 = 1;
const TNM_BTN_STATE_PUSH: i64 = 2;
const TNM_BTN_STATE_SELECT: i64 = 3;
const TNM_BTN_STATE_DISABLE: i64 = 4;

fn split_pos_named<'a>(args: &'a [Value]) -> (Vec<&'a Value>, Vec<(i32, &'a Value)>) {
    let mut pos = Vec::new();
    let mut named = Vec::new();
    for a in args {
        if let Value::NamedArg { id, value } = a {
            named.push((*id, value.as_ref()));
        } else {
            pos.push(a);
        }
    }
    (pos, named)
}

fn resolve_movie_path(project_dir: &Path, append_dir: &str, file_name: &str) -> Option<PathBuf> {
    crate::resource::find_mov_path_with_append_dir(project_dir, append_dir, file_name)
        .ok()
        .map(|(path, _ty)| path)
}

fn resolve_filter_path(project_dir: &Path, raw: &str) -> Option<PathBuf> {
    let mut norm = raw.replace('\\', "/");
    let mut candidates: Vec<PathBuf> = Vec::new();

    if !norm.contains('.') {
        for ext in ["png", "bmp", "jpg", "jpeg", "g00"] {
            candidates.push(project_dir.join(format!("{}.{}", norm, ext)));
            candidates.push(project_dir.join("dat").join(format!("{}.{}", norm, ext)));
        }
    }
    candidates.push(project_dir.join(&norm));
    candidates.push(project_dir.join("dat").join(&norm));

    for c in candidates {
        if c.exists() {
            return Some(c);
        }
    }
    None
}

fn movie_total_time_ms(ctx: &mut CommandContext, file: &str) -> Option<u64> {
    ctx.movie
        .prepare(file)
        .ok()
        .and_then(|info| info.duration_ms())
}

fn digits_most_significant(mut n: u64) -> Vec<i64> {
    if n == 0 {
        return vec![0];
    }
    let mut d = Vec::new();
    while n > 0 {
        d.push((n % 10) as i64);
        n /= 10;
    }
    d.reverse();
    d
}

fn update_number_backend(ctx: &mut CommandContext, obj: &mut ObjectState) {
    let (layer_id, sprite_ids) = match &obj.backend {
        ObjectBackend::Number {
            layer_id,
            sprite_ids,
        } => (*layer_id, sprite_ids.as_slice()),
        _ => return,
    };

    let Some(file) = obj.file_name.as_deref() else {
        if let Some(layer) = ctx.layers.layer_mut(layer_id) {
            for &sid in sprite_ids {
                if let Some(spr) = layer.sprite_mut(sid) {
                    spr.visible = false;
                    spr.image_id = None;
                }
            }
        }
        return;
    };

    let disp = if ctx.ids.obj_disp != 0 {
        obj.lookup_int_prop(&ctx.ids, ctx.ids.obj_disp).unwrap_or(0) != 0
    } else {
        true
    };
    let base_x = if ctx.ids.obj_x != 0 {
        obj.lookup_int_prop(&ctx.ids, ctx.ids.obj_x).unwrap_or(0) as i32
    } else {
        0
    };
    let base_y = if ctx.ids.obj_y != 0 {
        obj.lookup_int_prop(&ctx.ids, ctx.ids.obj_y).unwrap_or(0) as i32
    } else {
        0
    };
    let base_pat = if ctx.ids.obj_patno != 0 {
        obj.lookup_int_prop(&ctx.ids, ctx.ids.obj_patno)
            .unwrap_or(0)
    } else {
        0
    };

    let keta_max = obj.number_param.keta_max.max(0).min(16) as usize;
    let disp_zero = obj.number_param.disp_zero != 0;
    let disp_sign_cfg = obj.number_param.disp_sign != 0;
    let tumeru_sign = obj.number_param.tumeru_sign != 0 && !disp_zero;
    let space_mod = obj.number_param.space_mod;
    let space = obj.number_param.space;

    let n = obj.number_value;
    let sign = if n == 0 {
        0
    } else if n > 0 {
        1
    } else {
        -1
    };
    let disp_sign = disp_sign_cfg || sign == -1;

    let digits = digits_most_significant(n.unsigned_abs());
    let keta = digits.len();

    let mut pat_no = [0i64; 16];
    let mut spr_disp = [false; 16];

    if disp_zero {
        for i in 0..keta_max {
            spr_disp[i] = true;
            pat_no[i] = 0;
        }
    }

    let mut num_pos = keta_max.saturating_sub(keta);
    let mut sign_pos: Option<usize> = None;

    if disp_sign {
        let sp = if tumeru_sign {
            num_pos.saturating_sub(1)
        } else {
            0
        };
        sign_pos = Some(sp);
        num_pos = num_pos.max(sp + 1);
    }

    for (i, d) in digits.iter().enumerate() {
        let idx = num_pos + i;
        if idx < 16 {
            pat_no[idx] = *d;
            spr_disp[idx] = true;
        }
    }

    if let Some(sp) = sign_pos {
        let p = match sign {
            0 => 12,
            1 => 11,
            -1 => 10,
            _ => 0,
        };
        if sp < 16 {
            pat_no[sp] = p;
            spr_disp[sp] = true;
        }
    }

    for i in 0..16 {
        pat_no[i] = pat_no[i].saturating_add(base_pat);
    }

    // Width used for spacing when space_mod==0.
    let default_w = ctx
        .images
        .load_g00(file, base_pat.max(0) as u32)
        .ok()
        .and_then(|id| ctx.images.get(id).map(|img| img.width as i32));

    let mut offset: i32 = 0;

    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
        for (i, &sid) in sprite_ids.iter().enumerate().take(16) {
            let frame = pat_no[i].max(0) as u32;
            let img_id = ctx.images.load_g00(file, frame).ok();

            let w = img_id
                .and_then(|id| ctx.images.get(id).map(|img| img.width as i32))
                .or(default_w)
                .unwrap_or(0);

            if let Some(spr) = layer.sprite_mut(sid) {
                spr.fit = SpriteFit::PixelRect;
                spr.size_mode = SpriteSizeMode::Intrinsic;
                spr.order = i as i32;
                spr.x = base_x - offset;
                spr.y = base_y;
                spr.visible = disp && spr_disp[i] && img_id.is_some();
                spr.image_id = img_id;
            }

            offset = offset.saturating_add(space as i32);
            if space_mod == 0 {
                offset = offset.saturating_add(w);
            }
        }
    }
}

fn string_layout(obj: &ObjectState) -> (u32, u32, u32) {
    let font_px = obj.string_param.moji_size.max(0) as u32;
    let font_px = if font_px == 0 { 26 } else { font_px };

    let max_chars = obj.string_param.moji_cnt.max(0) as u32;
    let max_chars = if max_chars == 0 { 40 } else { max_chars };

    let max_w = (font_px.saturating_mul(max_chars)).max(font_px * 4);

    let text = obj.string_value.as_deref().unwrap_or("");
    let mut line_cnt = 1u32;
    if !text.is_empty() {
        let mut cur = 0u32;
        line_cnt = 0;
        for ch in text.chars() {
            if ch == '\n' {
                line_cnt += 1;
                cur = 0;
                continue;
            }
            cur += 1;
            if cur >= max_chars {
                line_cnt += 1;
                cur = 0;
            }
        }
        line_cnt += 1;
    }
    let line_h = (font_px * 8 / 7).max(font_px);
    let max_h = (line_cnt.max(1)).saturating_mul(line_h).max(line_h * 2);
    (font_px, max_w, max_h)
}

fn update_string_backend(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    obj: &mut ObjectState,
    stage_idx: i64,
) {
    let text = obj.string_value.clone().unwrap_or_default();
    let (font_px, max_w, max_h) = string_layout(obj);

    let disp = if ctx.ids.obj_disp != 0 {
        obj.lookup_int_prop(&ctx.ids, ctx.ids.obj_disp).unwrap_or(0) != 0
    } else {
        true
    };
    let x = if ctx.ids.obj_x != 0 {
        obj.lookup_int_prop(&ctx.ids, ctx.ids.obj_x).unwrap_or(0) as i32
    } else {
        0
    };
    let y = if ctx.ids.obj_y != 0 {
        obj.lookup_int_prop(&ctx.ids, ctx.ids.obj_y).unwrap_or(0) as i32
    } else {
        0
    };

    let layer_id = match obj.backend {
        ObjectBackend::String { layer_id, .. } => layer_id,
        _ => ensure_rect_layer(ctx, st, stage_idx),
    };

    let sprite_id = match obj.backend {
        ObjectBackend::String { sprite_id, .. } => sprite_id,
        _ => {
            let Some(sid) = ctx.layers.layer_mut(layer_id).map(|l| l.create_sprite()) else {
                return;
            };
            sid
        }
    };

    if !ctx.font_cache.is_loaded() {
        let _ = ctx
            .font_cache
            .load_from_font_dir(&ctx.project_dir.join("font"));
    }
    let img_id = ctx
        .font_cache
        .render_text(&mut ctx.images, &text, font_px as f32, max_w, max_h);

    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
        if let Some(spr) = layer.sprite_mut(sprite_id) {
            spr.fit = SpriteFit::PixelRect;
            spr.size_mode = SpriteSizeMode::Explicit {
                width: max_w,
                height: max_h,
            };
            spr.visible = disp;
            spr.x = x;
            spr.y = y;
            spr.image_id = img_id;
        }
    }

    obj.backend = ObjectBackend::String {
        layer_id,
        sprite_id,
        width: max_w,
        height: max_h,
    };
}

fn resolve_object_op(ids: &constants::RuntimeConstants, op: i32) -> ObjectOpKind {
    if ids.obj_init != 0 && op == ids.obj_init {
        return ObjectOpKind::Init;
    }
    if ids.obj_free != 0 && op == ids.obj_free {
        return ObjectOpKind::Free;
    }
    if ids.obj_init_param != 0 && op == ids.obj_init_param {
        return ObjectOpKind::InitParam;
    }
    if ids.obj_create != 0 && op == ids.obj_create {
        return ObjectOpKind::CreatePct;
    }
    if op == constants::OBJECT_CREATE_RECT {
        return ObjectOpKind::CreateRect;
    }
    if ids.obj_set_string != 0 && op == ids.obj_set_string {
        return ObjectOpKind::CreateString;
    }
    if ids.obj_set_pos != 0 && op == ids.obj_set_pos {
        return ObjectOpKind::SetPos;
    }
    if ids.obj_set_center != 0 && op == ids.obj_set_center {
        return ObjectOpKind::SetCenter;
    }
    if ids.obj_set_scale != 0 && op == ids.obj_set_scale {
        return ObjectOpKind::SetScale;
    }
    if ids.obj_set_rotate != 0 && op == ids.obj_set_rotate {
        return ObjectOpKind::SetRotate;
    }
    if ids.obj_set_clip != 0 && op == ids.obj_set_clip {
        return ObjectOpKind::SetClip;
    }
    if ids.obj_set_src_clip != 0 && op == ids.obj_set_src_clip {
        return ObjectOpKind::SetSrcClip;
    }
    if ids.obj_clear_button != 0 && op == ids.obj_clear_button {
        return ObjectOpKind::ClearButton;
    }
    if ids.obj_set_button != 0 && op == ids.obj_set_button {
        return ObjectOpKind::SetButton;
    }
    if ids.obj_set_button_group != 0 && op == ids.obj_set_button_group {
        return ObjectOpKind::SetButtonGroup;
    }
    ObjectOpKind::Unknown
}

struct ObjectWriteBack {
    st: *mut StageFormState,
    stage_idx: i64,
    obj_u: usize,
    obj: ObjectState,
}

impl Drop for ObjectWriteBack {
    fn drop(&mut self) {
        unsafe {
            let st = &mut *self.st;
            if let Some(list) = st.object_lists.get_mut(&self.stage_idx) {
                if self.obj_u < list.len() {
                    let obj = std::mem::replace(&mut self.obj, ObjectState::default());
                    list[self.obj_u] = obj;
                }
            }
        }
    }
}

fn dispatch_object_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    obj_idx: i64,
    op: i32,
    tail: &[i32],
    script_args: &[Value],
    ret_form: Option<i64>,
    rhs: Option<&Value>,
    al_id: Option<i64>,
) -> bool {
    if obj_idx < 0 {
        push_ok(ctx, ret_form);
        return true;
    }
    let obj_u = obj_idx as usize;
    ctx.globals.current_stage_object = Some((stage_idx, obj_u));

    if !ensure_object_for_access(st, stage_idx, obj_u) {
        // Strict out-of-range: return default based on ret_form if present.
        match ret_form {
            Some(rf) => ctx.stack.push(default_for_ret_form(rf)),
            None => ctx.stack.push(Value::Int(0)),
        }
        return true;
    }

    // We avoid sharing backend resources (sprites) across objects.
    let mut copy_from_snapshot: Option<(i64, usize, ObjectState)> = None;
    if rhs.is_none() && ret_form == Some(0) && script_args.len() == 1 {
        if let Value::Element(e) = &script_args[0] {
            if let Some(StageTarget::ChildItemOp {
                stage, child, idx, ..
            }) = parse_target(ctx, e)
            {
                if child == ctx.ids.stage_elm_object && idx >= 0 {
                    let src_u = idx as usize;
                    if let Some(src_list) = st.object_lists.get(&stage) {
                        if let Some(src_obj) = src_list.get(src_u) {
                            copy_from_snapshot = Some((stage, src_u, src_obj.clone()));
                        }
                    }
                }
            }
        }
    }

    let obj0 = {
        let list = st.object_lists.get_mut(&stage_idx).unwrap();
        std::mem::take(&mut list[obj_u])
    };
    let mut obj_write_back = ObjectWriteBack {
        st: st as *mut StageFormState,
        stage_idx,
        obj_u,
        obj: obj0,
    };
    let obj = &mut obj_write_back.obj;

    fn frame_action_set_from_args(
        fa: &mut ObjectFrameActionState,
        script_args: &[Value],
        real_time_flag: bool,
    ) {
        fa.end_time = script_args.get(0).and_then(as_i64).unwrap_or(0);
        fa.cmd_name = script_args
            .get(1)
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        fa.scn_name.clear();
        fa.real_time_flag = real_time_flag;
        fa.end_flag = false;
        fa.counter.reset();
        fa.args = script_args
            .iter()
            .skip(2)
            .filter_map(as_i64)
            .collect::<Vec<_>>();
    }

    fn dispatch_object_frame_action(
        ctx: &mut CommandContext,
        fa: &mut ObjectFrameActionState,
        tail: &[i32],
        script_args: &[Value],
        rhs: Option<&Value>,
        al_id: Option<i64>,
        ret_form: Option<i64>,
    ) -> bool {
        if tail.is_empty() {
            push_ok(ctx, ret_form);
            return true;
        }

        match tail[0] {
            0 => {
                let set_v = rhs.and_then(as_i64).or_else(|| {
                    if al_id == Some(1) && script_args.len() == 1 {
                        script_args.first().and_then(as_i64)
                    } else {
                        None
                    }
                });
                if let Some(v) = set_v {
                    fa.counter.set_count(v);
                    push_ok(ctx, ret_form);
                } else {
                    ctx.stack.push(Value::Int(fa.counter.get_count()));
                }
                true
            }
            1 => {
                frame_action_set_from_args(fa, script_args, false);
                push_ok(ctx, ret_form);
                true
            }
            2 => {
                fa.end_flag = true;
                fa.counter.reset();
                push_ok(ctx, ret_form);
                true
            }
            3 => {
                frame_action_set_from_args(fa, script_args, true);
                push_ok(ctx, ret_form);
                true
            }
            4 => {
                ctx.stack.push(Value::Int(if fa.end_flag { 1 } else { 0 }));
                true
            }
            _ => false,
        }
    }

    if let Some((src_stage, src_idx, mut src)) = copy_from_snapshot.take() {
        // Reset destination.
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        let src_file = src.file_name.clone();
        src.backend = ObjectBackend::None;
        if let Some(file) = src.file_name.clone() {
            let disp = ctx
                .gfx
                .object_peek_disp(src_stage, src_idx as i64)
                .unwrap_or(0)
                != 0;
            let (x, y) = ctx
                .gfx
                .object_peek_pos(src_stage, src_idx as i64)
                .unwrap_or((0, 0));
            let pat = ctx
                .gfx
                .object_peek_patno(src_stage, src_idx as i64)
                .unwrap_or(0);
            {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                let _ = gfx.object_create(
                    images,
                    layers,
                    stage_idx,
                    obj_u as i64,
                    &file,
                    disp as i64,
                    x,
                    y,
                    pat,
                );
            }
            src.backend = ObjectBackend::Gfx;
        }
        // Preserve file name even if gfx binding fails.
        if src.file_name.is_none() {
            src.file_name = src_file;
        }
        *obj = src;
        obj.used = true;
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_frame_action != 0
        && op == ctx.ids.obj_frame_action
        && dispatch_object_frame_action(
            ctx,
            &mut obj.frame_action,
            tail,
            script_args,
            rhs,
            al_id,
            ret_form,
        )
    {
        return true;
    }

    if ctx.ids.obj_frame_action_ch != 0 && op == ctx.ids.obj_frame_action_ch {
        if tail.len() >= 2 && (tail[0] == ctx.ids.elm_array || tail[0] == -1) {
            let idx = tail[1].max(0) as usize;
            if obj.frame_action_ch.len() <= idx {
                obj.frame_action_ch
                    .resize_with(idx + 1, ObjectFrameActionState::default);
            }
            if dispatch_object_frame_action(
                ctx,
                &mut obj.frame_action_ch[idx],
                &tail[2..],
                script_args,
                rhs,
                al_id,
                ret_form,
            ) {
                return true;
            }
        } else if tail.len() == 1 {
            match tail[0] {
                1 => {
                    let n = script_args.first().and_then(as_i64).unwrap_or(0).max(0) as usize;
                    obj.frame_action_ch
                        .resize_with(n, ObjectFrameActionState::default);
                    push_ok(ctx, ret_form);
                    return true;
                }
                2 => {
                    ctx.stack.push(Value::Int(obj.frame_action_ch.len() as i64));
                    return true;
                }
                _ => {}
            }
        }
    }

    if op == 93 {
        obj.used = true;
        if ctx.ids.obj_disp != 0 && !obj.has_int_prop(ctx.ids.obj_disp) {
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, 1);
        }
        if tail.len() >= 3 && tail[0] == -1 {
            let child_idx = tail[1].max(0) as usize;
            let slot = {
                let list = &mut obj.runtime.child_objects;
                if list.len() <= child_idx {
                    list.resize_with(child_idx + 1, ObjectState::default);
                }
                nested_object_slot(st, stage_idx, &mut list[child_idx])
            };
            if !ensure_object_for_access(st, stage_idx, slot) {
                match ret_form {
                    Some(rf) => ctx.stack.push(default_for_ret_form(rf)),
                    None => ctx.stack.push(Value::Int(0)),
                }
                return true;
            }
            let child_snapshot = std::mem::take(&mut obj.runtime.child_objects[child_idx]);
            let slot_snapshot = {
                let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
                std::mem::take(&mut stage_list[slot])
            };
            {
                let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
                stage_list[slot] = child_snapshot;
            }
            let handled = dispatch_object_op(
                ctx,
                st,
                stage_idx,
                slot as i64,
                tail[2],
                &tail[3..],
                script_args,
                ret_form,
                rhs,
                al_id,
            );
            let child_after = {
                let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
                std::mem::take(&mut stage_list[slot])
            };
            {
                let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
                stage_list[slot] = slot_snapshot;
            }
            obj.runtime.child_objects[child_idx] = child_after;
            if handled {
                return true;
            }
        }
        if tail.len() == 1 {
            match tail[0] {
                3 => {
                    ctx.stack
                        .push(Value::Int(obj.runtime.child_objects.len() as i64));
                    return true;
                }
                4 => {
                    if let Some(n0) = script_args.first().and_then(as_i64) {
                        let n = n0.max(0) as usize;
                        obj.runtime
                            .child_objects
                            .resize_with(n, ObjectState::default);
                        obj.used = true;
                    }
                    push_ok(ctx, ret_form);
                    return true;
                }
                _ => {}
            }
        }
    }

    if let Some(rep_list) = obj.rep_int_event_list_by_rep_op_mut(&ctx.ids, op) {
        let mut arr_idx: Option<i64> = None;
        let mut t = tail;
        if t.len() >= 2 && (t[0] == ctx.ids.elm_array || t[0] == -1) {
            arr_idx = Some(t[1] as i64);
            t = &t[2..];
        }

        if let Some(rep_idx) = arr_idx {
            if rep_idx < 0 {
                ctx.stack.push(Value::Int(0));
                return true;
            }
            let ri = rep_idx as usize;
            if rep_list.len() <= ri {
                rep_list.resize_with(ri + 1, || IntEvent::new(0));
            }
            let ev = &mut rep_list[ri];
            if t.is_empty() {
                if let Some(Value::Int(v)) = rhs {
                    ev.set_value(*v as i32);
                    ctx.stack.push(Value::Int(0));
                } else {
                    ctx.stack.push(Value::Int(ev.get_value() as i64));
                }
                return true;
            }
        } else if t.len() == 1 {
            match t[0] {
                crate::runtime::constants::elm_value::INTLIST_RESIZE => {
                    let n = script_args.first().and_then(as_i64).unwrap_or(0).max(0) as usize;
                    rep_list.resize_with(n, || IntEvent::new(0));
                    ctx.stack.push(Value::Int(0));
                    return true;
                }
                _ => {}
            }
        }

        if !t.is_empty() && t[0] == crate::runtime::constants::elm_value::INTLIST_CLEAR {
            let start = script_args.first().and_then(as_i64).unwrap_or(0);
            let end = script_args.get(1).and_then(as_i64).unwrap_or(start);
            let value = if al_id == Some(0) {
                0
            } else {
                script_args.get(2).and_then(as_i64).unwrap_or(0) as i32
            };
            for idx in start.min(end)..=start.max(end) {
                let ui = idx.max(0) as usize;
                if rep_list.len() <= ui {
                    rep_list.resize_with(ui + 1, || IntEvent::new(0));
                }
                rep_list[ui].set_value(value);
            }
            ctx.stack.push(Value::Int(0));
            return true;
        }
    }

    let is_obj_int_list = obj.int_list_by_op(&ctx.ids, op).is_some();
    let is_obj_int_event = obj.int_event_by_op(&ctx.ids, op).is_some();
    let is_obj_int_event_list = obj.int_event_list_by_op(&ctx.ids, op).is_some();

    if is_obj_int_list || is_obj_int_event || is_obj_int_event_list {
        let mut arr_idx: Option<i64> = None;
        let mut t = tail;
        if t.len() >= 2 && (t[0] == ctx.ids.elm_array || t[0] == -1) {
            arr_idx = Some(t[1] as i64);
            t = &t[2..];
        }

        if is_obj_int_list && arr_idx.is_none() && t.len() == 1 {
            match t[0] {
                3 => {
                    if let Some(n0) = script_args.first().and_then(as_i64) {
                        if !(ctx.ids.obj_f != 0 && op == ctx.ids.obj_f) {
                            let n = n0.max(0) as usize;
                            if let Some(list) = obj.int_list_by_op_mut(&ctx.ids, op) {
                                list.resize(n, 0);
                            }
                        }
                    }
                    ctx.stack.push(Value::Int(0));
                    return true;
                }
                4 => {
                    let n = obj
                        .int_list_by_op(&ctx.ids, op)
                        .map(|v| v.len())
                        .unwrap_or(0);
                    ctx.stack.push(Value::Int(n as i64));
                    return true;
                }
                _ => {}
            }
        }

        if is_obj_int_event_list
            && arr_idx.is_none()
            && t.len() == 1
            && t[0] == 1
            && script_args.len() == 1
        {
            if let Some(n0) = script_args.first().and_then(as_i64) {
                let n = n0.max(0) as usize;
                if let Some(list) = obj.int_event_list_by_op_mut(&ctx.ids, op) {
                    list.resize_with(n, || IntEvent::new(0));
                }
            }
            ctx.stack.push(Value::Int(0));
            return true;
        }

        if let Some(rep_idx) = arr_idx {
            if rep_idx < 0 {
                ctx.stack.push(Value::Int(0));
                return true;
            }
            let ri = rep_idx as usize;

            if is_obj_int_event_list {
                let ent = obj.int_event_list_by_op_mut(&ctx.ids, op).unwrap();
                if ent.len() <= ri {
                    ent.resize_with(ri + 1, || IntEvent::new(0));
                }
                let ev = &mut ent[ri];
                if t.is_empty() {
                    if let Some(Value::Int(v)) = rhs {
                        ev.set_value(*v as i32);
                        ctx.stack.push(Value::Int(0));
                    } else {
                        ctx.stack.push(Value::Int(ev.get_value() as i64));
                    }
                    return true;
                }
                match t[0] {
                    0 => {
                        if script_args.len() >= 4 {
                            let v = script_args.get(0).and_then(as_i64).unwrap_or(0) as i32;
                            let tt = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
                            let d = script_args.get(2).and_then(as_i64).unwrap_or(0) as i32;
                            let sp = script_args.get(3).and_then(as_i64).unwrap_or(0) as i32;
                            ev.set_event(v, tt, d, sp, 0);
                        }
                        ctx.stack.push(Value::Int(0));
                        return true;
                    }
                    1 => {
                        if script_args.len() >= 5 {
                            let sv = script_args.get(0).and_then(as_i64).unwrap_or(0) as i32;
                            let evv = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
                            let lt = script_args.get(2).and_then(as_i64).unwrap_or(0) as i32;
                            let d = script_args.get(3).and_then(as_i64).unwrap_or(0) as i32;
                            let sp = script_args.get(4).and_then(as_i64).unwrap_or(0) as i32;
                            ev.loop_event(sv, evv, lt, d, sp, 0);
                        }
                        ctx.stack.push(Value::Int(0));
                        return true;
                    }
                    2 => {
                        if ret_form.unwrap_or(0) != 0 {
                            ctx.stack
                                .push(Value::Int(if ev.check_event() { 1 } else { 0 }));
                        } else {
                            ev.end_event();
                            ctx.stack.push(Value::Int(0));
                        }
                        return true;
                    }
                    3 => {
                        if ev.check_event() {
                            ctx.wait.wait_object_event_list(
                                ctx.ids.form_global_stage,
                                stage_idx,
                                obj_u,
                                op,
                                ri,
                                false,
                            );
                        }
                        push_ok(ctx, ret_form);
                        return true;
                    }
                    4 => {
                        ctx.stack
                            .push(Value::Int(if ev.check_event() { 1 } else { 0 }));
                        return true;
                    }
                    _ => {}
                }
            }

            if is_obj_int_list {
                let ent = obj.int_list_by_op_mut(&ctx.ids, op).unwrap();
                if ent.len() <= ri {
                    if ctx.ids.obj_f != 0 && op == ctx.ids.obj_f {
                        ctx.stack.push(Value::Int(0));
                        return true;
                    }
                    ent.resize(ri + 1, 0);
                }
                if let Some(Value::Int(v)) = rhs {
                    ent[ri] = *v;
                    ctx.stack.push(Value::Int(0));
                } else {
                    ctx.stack.push(Value::Int(ent[ri]));
                }
                return true;
            }
        }

        if arr_idx.is_none() && is_obj_int_event {
            let ev = obj.int_event_by_op_mut(&ctx.ids, op).unwrap();
            if t.is_empty() {
                if let Some(Value::Int(v)) = rhs {
                    ev.set_value(*v as i32);
                    ctx.stack.push(Value::Int(0));
                } else if let Some(vm_call) = &ctx.vm_call {
                    ctx.stack.push(Value::Element(vm_call.element.clone()));
                } else {
                    ctx.stack.push(Value::Int(ev.get_value() as i64));
                }
                return true;
            }
            if (0..=4).contains(&t[0]) {
                match t[0] {
                    0 => {
                        if script_args.len() >= 4 {
                            let v = script_args.get(0).and_then(as_i64).unwrap_or(0) as i32;
                            let tt = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
                            let d = script_args.get(2).and_then(as_i64).unwrap_or(0) as i32;
                            let sp = script_args.get(3).and_then(as_i64).unwrap_or(0) as i32;
                            ev.set_event(v, tt, d, sp, 0);
                        }
                        ctx.stack.push(Value::Int(0));
                        return true;
                    }
                    1 => {
                        if script_args.len() >= 5 {
                            let sv = script_args.get(0).and_then(as_i64).unwrap_or(0) as i32;
                            let evv = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
                            let lt = script_args.get(2).and_then(as_i64).unwrap_or(0) as i32;
                            let d = script_args.get(3).and_then(as_i64).unwrap_or(0) as i32;
                            let sp = script_args.get(4).and_then(as_i64).unwrap_or(0) as i32;
                            ev.loop_event(sv, evv, lt, d, sp, 0);
                        }
                        ctx.stack.push(Value::Int(0));
                        return true;
                    }
                    2 => {
                        if ret_form.unwrap_or(0) != 0 {
                            ctx.stack
                                .push(Value::Int(if ev.check_event() { 1 } else { 0 }));
                        } else {
                            ev.end_event();
                            ctx.stack.push(Value::Int(0));
                        }
                        return true;
                    }
                    3 => {
                        if ev.check_event() {
                            ctx.wait.wait_object_event(
                                ctx.ids.form_global_stage,
                                stage_idx,
                                obj_u,
                                op,
                                false,
                            );
                        }
                        push_ok(ctx, ret_form);
                        return true;
                    }
                    4 => {
                        ctx.stack
                            .push(Value::Int(if ev.check_event() { 1 } else { 0 }));
                        return true;
                    }
                    _ => {}
                }
            }
        }
    }

    // OBJECT.ALL_EVE.{END,WAIT,CHECK}
    // The element chain is OBJECT[...].ALL_EVE.ALLEVENT_*(no args).
    // We support it when the numeric IDs are provided via RuntimeConstants.
    if op == ctx.ids.obj_all_eve {
        let sub = tail.get(0).copied().unwrap_or(0);
        if sub == ctx.ids.elm_allevent_end {
            obj.end_all_events();
            push_ok(ctx, ret_form);
            return true;
        }
        if sub == ctx.ids.elm_allevent_wait {
            if obj.any_event_active() {
                ctx.wait
                    .wait_object_all_events(ctx.ids.form_global_stage, stage_idx, obj_u, false);
            }
            push_ok(ctx, ret_form);
            return true;
        }
        if sub == ctx.ids.elm_allevent_check {
            ctx.stack
                .push(Value::Int(if obj.any_event_active() { 1 } else { 0 }));
            return true;
        }
    }

    // Keep the existing id-mapped subset for correctness when ids are available.

    // Id-mapped subset: when numeric ids are available in RuntimeConstants.
    // This must reflect actual runtime state for both Gfx and Rect backends.
    if op == ctx.ids.obj_init {
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.used = true;
        obj.backend = ObjectBackend::None;
        obj.file_name = None;
        obj.string_value = None;
        obj.init_param_like();
        push_ok(ctx, ret_form);
        return true;
    }

    if op == ctx.ids.obj_free {
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        *obj = ObjectState::default();
        push_ok(ctx, ret_form);
        return true;
    }

    if op == ctx.ids.obj_init_param {
        obj.init_param_like();
        push_ok(ctx, ret_form);
        return true;
    }

    if op == ctx.ids.obj_get_file_name {
        let s = obj.file_name.clone().unwrap_or_default();
        ctx.stack.push(Value::Str(s));
        return true;
    }

    if op == ctx.ids.obj_create {
        let Some(file) = script_args.get(0).and_then(as_str) else {
            push_ok(ctx, ret_form);
            return true;
        };

        let aid = al_id.unwrap_or(0);
        let disp = if aid >= 1 {
            script_args.get(1).and_then(as_i64).unwrap_or(0) != 0
        } else {
            false
        };
        let x = if aid >= 2 {
            script_args.get(2).and_then(as_i64).unwrap_or(0)
        } else {
            0
        };
        let y = if aid >= 2 {
            script_args.get(3).and_then(as_i64).unwrap_or(0)
        } else {
            0
        };
        let patno = if aid >= 3 {
            script_args.get(4).and_then(as_i64).unwrap_or(0)
        } else {
            0
        };

        sg_debug_stage(format!(
            "stage={} obj={} CREATE(file={}) al_id={:?} disp={} x={} y={} patno={}",
            stage_idx, obj_u, file, al_id, disp, x, y, patno
        ));

        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.init_param_like();

        {
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            let _ = gfx.object_create(
                images,
                layers,
                stage_idx,
                obj_u as i64,
                file,
                disp as i64,
                x,
                y,
                patno,
            );
        }
        obj.used = true;
        obj.backend = ObjectBackend::Gfx;
        obj.file_name = Some(file.to_string());
        obj.string_value = None;
        if ctx.ids.obj_disp != 0 {
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp { 1 } else { 0 });
        }
        if ctx.ids.obj_x != 0 {
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_x, x);
        }
        if ctx.ids.obj_y != 0 {
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_y, y);
        }
        if ctx.ids.obj_patno != 0 {
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_patno, patno);
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if op == ctx.ids.obj_disp {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            let b = v != 0;
            sg_debug_stage(format!(
                "stage={} obj={} DISP {}",
                stage_idx,
                obj_u,
                if b { 1 } else { 0 }
            ));
            match obj.backend {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.visible = b;
                        }
                    }
                }
                ObjectBackend::Gfx => {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ = gfx.object_set_disp(
                        images,
                        layers,
                        stage_idx,
                        obj_u as i64,
                        if b { 1 } else { 0 },
                    );
                }
                ObjectBackend::Number { .. } => {
                    obj.set_int_prop(&ctx.ids, op, if b { 1 } else { 0 });
                    update_number_backend(ctx, obj);
                }
                ObjectBackend::String {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.visible = b;
                        }
                    }
                    obj.set_int_prop(&ctx.ids, op, if b { 1 } else { 0 });
                }
                _ => {
                    obj.set_int_prop(&ctx.ids, op, if b { 1 } else { 0 });
                }
            }
            ctx.stack.push(Value::Int(0));
        } else {
            let v = match obj.backend {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => ctx
                    .layers
                    .layer(layer_id)
                    .and_then(|layer| layer.sprite(sprite_id))
                    .map(|spr| if spr.visible { 1 } else { 0 })
                    .unwrap_or(0),
                ObjectBackend::Gfx => ctx
                    .gfx
                    .object_peek_disp(stage_idx, obj_u as i64)
                    .unwrap_or(0),
                ObjectBackend::Number { .. } => obj.get_int_prop(&ctx.ids, op),
                ObjectBackend::String {
                    layer_id,
                    sprite_id,
                    ..
                } => ctx
                    .layers
                    .layer(layer_id)
                    .and_then(|layer| layer.sprite(sprite_id))
                    .map(|spr| if spr.visible { 1 } else { 0 })
                    .unwrap_or(0),
                _ => obj.get_int_prop(&ctx.ids, op),
            };
            ctx.stack.push(Value::Int(v));
        }
        return true;
    }

    if op == ctx.ids.obj_x {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            match obj.backend {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.x = v as i32;
                        }
                    }
                }
                ObjectBackend::Gfx => {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ = gfx.object_set_x(images, layers, stage_idx, obj_u as i64, v);
                }
                ObjectBackend::Number { .. } => {
                    obj.set_int_prop(&ctx.ids, op, v);
                    update_number_backend(ctx, obj);
                }
                _ => {
                    obj.set_int_prop(&ctx.ids, op, v);
                }
            }
            ctx.stack.push(Value::Int(0));
        } else {
            let v = match obj.backend {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => ctx
                    .layers
                    .layer(layer_id)
                    .and_then(|layer| layer.sprite(sprite_id))
                    .map(|spr| spr.x as i64)
                    .unwrap_or(0),
                ObjectBackend::Gfx => ctx
                    .gfx
                    .object_peek_pos(stage_idx, obj_u as i64)
                    .map(|(x, _)| x)
                    .unwrap_or(0),
                _ => obj.get_int_prop(&ctx.ids, op),
            };
            ctx.stack.push(Value::Int(v));
        }
        return true;
    }

    if op == ctx.ids.obj_y {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            match obj.backend {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.y = v as i32;
                        }
                    }
                }
                ObjectBackend::Gfx => {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ = gfx.object_set_y(images, layers, stage_idx, obj_u as i64, v);
                }
                ObjectBackend::Number { .. } => {
                    obj.set_int_prop(&ctx.ids, op, v);
                    update_number_backend(ctx, obj);
                }
                _ => {
                    obj.set_int_prop(&ctx.ids, op, v);
                }
            }
            ctx.stack.push(Value::Int(0));
        } else {
            let v = match obj.backend {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => ctx
                    .layers
                    .layer(layer_id)
                    .and_then(|layer| layer.sprite(sprite_id))
                    .map(|spr| spr.y as i64)
                    .unwrap_or(0),
                ObjectBackend::Gfx => ctx
                    .gfx
                    .object_peek_pos(stage_idx, obj_u as i64)
                    .map(|(_, y)| y)
                    .unwrap_or(0),
                _ => obj.get_int_prop(&ctx.ids, op),
            };
            ctx.stack.push(Value::Int(v));
        }
        return true;
    }

    if op == ctx.ids.obj_z {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            // The runtime renderer does not use Z for sorting (project constraint),
            // Keep Z in sync for callers that treat it as a property.
            if obj.backend == ObjectBackend::Gfx {
                let _ = ctx.gfx.object_set_z(stage_idx, obj_u as i64, v);
            }
            obj.set_int_prop(&ctx.ids, op, v);
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack.push(Value::Int(obj.get_int_prop(&ctx.ids, op)));
        }
        return true;
    }

    if op == ctx.ids.obj_world {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            obj.set_int_prop(&ctx.ids, op, v);
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack.push(Value::Int(obj.get_int_prop(&ctx.ids, op)));
        }
        return true;
    }

    if op == ctx.ids.obj_patno {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            match obj.backend {
                ObjectBackend::Gfx => {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ = gfx.object_set_pat_no(images, layers, stage_idx, obj_u as i64, v);
                }
                ObjectBackend::Number { .. } => {
                    obj.set_int_prop(&ctx.ids, op, v);
                    update_number_backend(ctx, obj);
                }
                _ => {
                    obj.set_int_prop(&ctx.ids, op, v);
                }
            }
            ctx.stack.push(Value::Int(0));
        } else {
            let v = match obj.backend {
                ObjectBackend::Gfx => ctx
                    .gfx
                    .object_peek_patno(stage_idx, obj_u as i64)
                    .unwrap_or(0),
                _ => obj.get_int_prop(&ctx.ids, op),
            };
            ctx.stack.push(Value::Int(v));
        }
        return true;
    }

    if op == ctx.ids.obj_layer {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            match obj.backend {
                ObjectBackend::Gfx => {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ = gfx.object_set_layer(images, layers, stage_idx, obj_u as i64, v);
                }
                _ => {
                    obj.set_int_prop(&ctx.ids, op, v);
                }
            }
            ctx.stack.push(Value::Int(0));
        } else {
            let v = match obj.backend {
                ObjectBackend::Gfx => ctx
                    .gfx
                    .object_peek_layer(stage_idx, obj_u as i64)
                    .unwrap_or(0),
                _ => obj.get_int_prop(&ctx.ids, op),
            };
            ctx.stack.push(Value::Int(v));
        }
        return true;
    }

    if op == ctx.ids.obj_alpha {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            let a = v.clamp(0, 255) as u8;
            match obj.backend {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.alpha = a;
                        }
                    }
                }
                ObjectBackend::Gfx => {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ =
                        gfx.object_set_alpha(images, layers, stage_idx, obj_u as i64, i64::from(a));
                }
                _ => {
                    obj.set_int_prop(&ctx.ids, op, a as i64);
                }
            }
            ctx.stack.push(Value::Int(0));
        } else {
            let v = match obj.backend {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => ctx
                    .layers
                    .layer(layer_id)
                    .and_then(|layer| layer.sprite(sprite_id))
                    .map(|spr| spr.alpha as i64)
                    .unwrap_or(0),
                ObjectBackend::Gfx => ctx
                    .gfx
                    .object_peek_alpha(stage_idx, obj_u as i64)
                    .unwrap_or(0),
                _ => obj.get_int_prop(&ctx.ids, op),
            };
            ctx.stack.push(Value::Int(v));
        }
        return true;
    }

    if op == ctx.ids.obj_order {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            match obj.backend {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.order = v as i32;
                        }
                    }
                }
                ObjectBackend::Gfx => {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ = gfx.object_set_order(images, layers, stage_idx, obj_u as i64, v);
                }
                _ => {
                    obj.set_int_prop(&ctx.ids, op, v);
                }
            }
            ctx.stack.push(Value::Int(0));
        } else {
            let v = match obj.backend {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => ctx
                    .layers
                    .layer(layer_id)
                    .and_then(|layer| layer.sprite(sprite_id))
                    .map(|spr| spr.order as i64)
                    .unwrap_or(0),
                ObjectBackend::Gfx => ctx
                    .gfx
                    .object_peek_order(stage_idx, obj_u as i64)
                    .unwrap_or(0),
                _ => obj.get_int_prop(&ctx.ids, op),
            };
            ctx.stack.push(Value::Int(v));
        }
        return true;
    }

    // Non-visual OBJECT properties that still map to explicit object members.
    if op == ctx.ids.obj_wipe_copy
        || op == ctx.ids.obj_wipe_erase
        || op == ctx.ids.obj_click_disable
    {
        let set_v = rhs.and_then(as_i64).or_else(|| {
            if al_id == Some(1) && script_args.len() == 1 {
                script_args.get(0).and_then(as_i64)
            } else {
                None
            }
        });
        if let Some(v) = set_v {
            obj.set_int_prop(&ctx.ids, op, v);
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack.push(Value::Int(obj.get_int_prop(&ctx.ids, op)));
        }
        return true;
    }

    // ---------------------------------------------------------------------
    // Button
    // ---------------------------------------------------------------------

    // Button-related object ops are handled only by explicit numeric IDs below.
    // Do not learn or reinterpret unknown ops here.

    // ---------------------------------------------------------------------
    // Direct translations (ID-mapped element codes)
    //
    // IMPORTANT: Many numeric IDs are game-specific. For any id-map entry that
    // defaults to 0 (unknown), we *must not* match it to avoid hijacking op=0.
    // ---------------------------------------------------------------------

    if ctx.ids.obj_exist_type != 0 && op == ctx.ids.obj_exist_type {
        ctx.stack
            .push(Value::Int(if obj.object_type == 0 { 0 } else { 1 }));
        return true;
    }

    if ctx.ids.obj_change_file != 0 && op == ctx.ids.obj_change_file {
        let Some(name) = script_args.get(0).and_then(as_str) else {
            push_ok(ctx, ret_form);
            return true;
        };
        // CHANGE_FILE does not reinit; it swaps the underlying file.
        obj.file_name = Some(name.to_string());
        if matches!(obj.backend, ObjectBackend::Gfx) {
            let disp = ctx
                .gfx
                .object_peek_disp(stage_idx, obj_u as i64)
                .unwrap_or(0)
                != 0;
            let (x, y) = ctx
                .gfx
                .object_peek_pos(stage_idx, obj_u as i64)
                .unwrap_or((0, 0));
            let pat = ctx
                .gfx
                .object_peek_patno(stage_idx, obj_u as i64)
                .unwrap_or(0);
            {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                let _ = gfx.object_create(
                    images,
                    layers,
                    stage_idx,
                    obj_u as i64,
                    name,
                    disp as i64,
                    x,
                    y,
                    pat,
                );
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_string != 0 && op == ctx.ids.obj_set_string {
        let Some(v) = script_args.get(0).and_then(as_str) else {
            push_ok(ctx, ret_form);
            return true;
        };
        obj.string_value = Some(v.to_string());
        if obj.object_type == 3 {
            update_string_backend(ctx, st, obj, stage_idx);
        }
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_get_string != 0 && op == ctx.ids.obj_get_string {
        ctx.stack
            .push(Value::Str(obj.string_value.clone().unwrap_or_default()));
        return true;
    }
    if ctx.ids.obj_set_string_param != 0 && op == ctx.ids.obj_set_string_param {
        // SET_STRING_PARAM
        // base: (moji_size, space_x, space_y, moji_cnt)
        obj.string_param.moji_size = script_args
            .get(0)
            .and_then(as_i64)
            .unwrap_or(obj.string_param.moji_size);
        obj.string_param.moji_space_x = script_args
            .get(1)
            .and_then(as_i64)
            .unwrap_or(obj.string_param.moji_space_x);
        obj.string_param.moji_space_y = script_args
            .get(2)
            .and_then(as_i64)
            .unwrap_or(obj.string_param.moji_space_y);
        obj.string_param.moji_cnt = script_args
            .get(3)
            .and_then(as_i64)
            .unwrap_or(obj.string_param.moji_cnt);
        // optional: (moji_color, shadow_color, shadow_mode, fuchi_color)
        if script_args.len() >= 7 {
            obj.string_param.moji_color = script_args
                .get(4)
                .and_then(as_i64)
                .unwrap_or(obj.string_param.moji_color);
            obj.string_param.shadow_color = script_args
                .get(5)
                .and_then(as_i64)
                .unwrap_or(obj.string_param.shadow_color);
            obj.string_param.shadow_mode = script_args
                .get(6)
                .and_then(as_i64)
                .unwrap_or(obj.string_param.shadow_mode);
        }
        if script_args.len() >= 8 {
            obj.string_param.fuchi_color = script_args
                .get(7)
                .and_then(as_i64)
                .unwrap_or(obj.string_param.fuchi_color);
        }
        if obj.object_type == 3 {
            update_string_backend(ctx, st, obj, stage_idx);
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_number != 0 && op == ctx.ids.obj_set_number {
        obj.number_value = script_args
            .get(0)
            .and_then(as_i64)
            .unwrap_or(obj.number_value);
        if matches!(obj.backend, ObjectBackend::Number { .. }) {
            update_number_backend(ctx, obj);
        }
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_get_number != 0 && op == ctx.ids.obj_get_number {
        ctx.stack.push(Value::Int(obj.number_value));
        return true;
    }
    if ctx.ids.obj_set_number_param != 0 && op == ctx.ids.obj_set_number_param {
        // SET_NUMBER_PARAM(keta_max, disp_zero, disp_sign, tumeru_sign, space_mod, space)
        obj.number_param.keta_max = script_args
            .get(0)
            .and_then(as_i64)
            .unwrap_or(obj.number_param.keta_max);
        obj.number_param.disp_zero = script_args
            .get(1)
            .and_then(as_i64)
            .unwrap_or(obj.number_param.disp_zero);
        obj.number_param.disp_sign = script_args
            .get(2)
            .and_then(as_i64)
            .unwrap_or(obj.number_param.disp_sign);
        obj.number_param.tumeru_sign = script_args
            .get(3)
            .and_then(as_i64)
            .unwrap_or(obj.number_param.tumeru_sign);
        obj.number_param.space_mod = script_args
            .get(4)
            .and_then(as_i64)
            .unwrap_or(obj.number_param.space_mod);
        obj.number_param.space = script_args
            .get(5)
            .and_then(as_i64)
            .unwrap_or(obj.number_param.space);
        if matches!(obj.backend, ObjectBackend::Number { .. }) {
            update_number_backend(ctx, obj);
        }
        push_ok(ctx, ret_form);
        return true;
    }

    // ---------------------------------------------------------------------
    // CREATE_* (ID-mapped)
    // ---------------------------------------------------------------------

    if ctx.ids.obj_create_number != 0 && op == ctx.ids.obj_create_number {
        let (pos, _named) = split_pos_named(script_args);
        let Some(file) = pos.get(0).and_then(|v| v.as_str()) else {
            push_ok(ctx, ret_form);
            return true;
        };

        object_clear_backend(ctx, obj, stage_idx, obj_u);

        obj.used = true;
        obj.object_type = 5;
        obj.number_value = 0;
        obj.string_param = Default::default();
        obj.number_param = Default::default();
        obj.weather_param = Default::default();
        obj.thumb_save_no = 0;
        obj.movie.reset();
        obj.emote = Default::default();
        obj.gan_file = None;
        obj.init_param_like();

        obj.file_name = Some(file.to_string());
        obj.string_value = None;

        let layer_id = ensure_rect_layer(ctx, st, stage_idx);
        let mut sprite_ids: Vec<SpriteId> = Vec::new();
        if let Some(layer) = ctx.layers.layer_mut(layer_id) {
            for _ in 0..16 {
                let sid = layer.create_sprite();
                if let Some(spr) = layer.sprite_mut(sid) {
                    spr.fit = SpriteFit::PixelRect;
                    spr.size_mode = SpriteSizeMode::Intrinsic;
                    spr.visible = false;
                    spr.image_id = None;
                }
                sprite_ids.push(sid);
            }
        }

        obj.backend = ObjectBackend::Number {
            layer_id,
            sprite_ids,
        };

        // Optional parameters (al_id-based fallthrough): (disp, x, y)
        let aid = al_id.unwrap_or(0);
        if ctx.ids.obj_disp != 0 {
            let disp_i = if aid >= 1 {
                pos.get(1).and_then(|v| v.as_i64()).unwrap_or(0)
            } else {
                0
            };
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        }
        if aid >= 2 {
            if ctx.ids.obj_x != 0 {
                obj.set_int_prop(
                    &ctx.ids,
                    ctx.ids.obj_x,
                    pos.get(2).and_then(|v| v.as_i64()).unwrap_or(0),
                );
            }
            if ctx.ids.obj_y != 0 {
                obj.set_int_prop(
                    &ctx.ids,
                    ctx.ids.obj_y,
                    pos.get(3).and_then(|v| v.as_i64()).unwrap_or(0),
                );
            }
        }
        if ctx.ids.obj_patno != 0 {
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_patno, 0);
        }

        update_number_backend(ctx, obj);
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_create_weather != 0 && op == ctx.ids.obj_create_weather {
        let (pos, _named) = split_pos_named(script_args);
        let Some(file) = pos.get(0).and_then(|v| v.as_str()) else {
            push_ok(ctx, ret_form);
            return true;
        };
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.used = true;
        obj.object_type = 4;
        obj.file_name = Some(file.to_string());
        obj.weather_param = Default::default();
        obj.movie.reset();
        obj.init_param_like();
        obj.mesh_animation_state = crate::mesh3d::MeshAnimationState::default();
        {
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            let _ = gfx.object_create(images, layers, stage_idx, obj_u as i64, file, 1, 0, 0, 0);
        }
        obj.backend = ObjectBackend::Gfx;
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_create_mesh != 0 && op == ctx.ids.obj_create_mesh {
        let (pos, _named) = split_pos_named(script_args);
        let Some(file) = pos.get(0).and_then(|v| v.as_str()) else {
            push_ok(ctx, ret_form);
            return true;
        };
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.used = true;
        obj.object_type = 6;
        obj.file_name = Some(file.to_string());
        obj.movie.reset();
        obj.init_param_like();
        obj.mesh_animation_state = crate::mesh3d::MeshAnimationState::default();
        {
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            let _ = gfx.object_create(images, layers, stage_idx, obj_u as i64, file, 1, 0, 0, 0);
        }
        if let Ok(asset) = load_mesh_asset(&ctx.project_dir, ctx.images.current_append_dir(), file)
        {
            if let Some(tex_path) = asset.texture_path.as_ref() {
                if let Ok(tex_id) = ctx.images.load_file(tex_path, 0) {
                    if let Some((lid, sid)) = ctx.gfx.object_sprite_binding(stage_idx, obj_u as i64)
                    {
                        if let Some(layer) = ctx.layers.layer_mut(lid) {
                            if let Some(sprite) = layer.sprite_mut(sid) {
                                sprite.image_id = Some(tex_id);
                            }
                        }
                    }
                }
            }
        }
        obj.backend = ObjectBackend::Gfx;
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_create_billboard != 0 && op == ctx.ids.obj_create_billboard {
        let (pos, _named) = split_pos_named(script_args);
        let Some(file) = pos.get(0).and_then(|v| v.as_str()) else {
            push_ok(ctx, ret_form);
            return true;
        };
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.used = true;
        obj.object_type = 7;
        obj.file_name = Some(file.to_string());
        obj.movie.reset();
        obj.init_param_like();
        obj.mesh_animation_state = crate::mesh3d::MeshAnimationState::default();
        {
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            let _ = gfx.object_create(images, layers, stage_idx, obj_u as i64, file, 1, 0, 0, 0);
        }
        if let Ok(asset) = load_mesh_asset(&ctx.project_dir, ctx.images.current_append_dir(), file)
        {
            if let Some(tex_path) = asset.texture_path.as_ref() {
                if let Ok(tex_id) = ctx.images.load_file(tex_path, 0) {
                    if let Some((lid, sid)) = ctx.gfx.object_sprite_binding(stage_idx, obj_u as i64)
                    {
                        if let Some(layer) = ctx.layers.layer_mut(lid) {
                            if let Some(sprite) = layer.sprite_mut(sid) {
                                sprite.image_id = Some(tex_id);
                            }
                        }
                    }
                }
            }
        }
        obj.backend = ObjectBackend::Gfx;
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_create_save_thumb != 0 && op == ctx.ids.obj_create_save_thumb {
        let (pos, _named) = split_pos_named(script_args);
        let save_no = pos.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.used = true;
        obj.object_type = 8;
        obj.thumb_save_no = save_no;
        obj.movie.reset();
        obj.init_param_like();
        // Optional (disp, x, y) via al_id.
        let aid = al_id.unwrap_or(0);
        if ctx.ids.obj_disp != 0 {
            let disp_i = if aid >= 1 {
                pos.get(1).and_then(|v| v.as_i64()).unwrap_or(0)
            } else {
                0
            };
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        }
        if aid >= 2 {
            if ctx.ids.obj_x != 0 {
                obj.set_int_prop(
                    &ctx.ids,
                    ctx.ids.obj_x,
                    pos.get(2).and_then(|v| v.as_i64()).unwrap_or(0),
                );
            }
            if ctx.ids.obj_y != 0 {
                obj.set_int_prop(
                    &ctx.ids,
                    ctx.ids.obj_y,
                    pos.get(3).and_then(|v| v.as_i64()).unwrap_or(0),
                );
            }
        }
        if let Some(img_id) = load_thumb_image_id(ctx, save_no) {
            bind_capture_backend(ctx, obj, stage_idx, img_id);
        } else {
            ctx.unknown
                .record_note(&format!("save_thumb.image.missing:{save_no}"));
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_create_capture_thumb != 0 && op == ctx.ids.obj_create_capture_thumb {
        let (pos, _named) = split_pos_named(script_args);
        let thumb_no = pos.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.used = true;
        obj.object_type = 11;
        obj.thumb_save_no = thumb_no;
        obj.movie.reset();
        obj.init_param_like();
        // Optional (disp, x, y) via al_id.
        let aid = al_id.unwrap_or(0);
        if ctx.ids.obj_disp != 0 {
            let disp_i = if aid >= 1 {
                pos.get(1).and_then(|v| v.as_i64()).unwrap_or(0)
            } else {
                0
            };
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        }
        if aid >= 2 {
            if ctx.ids.obj_x != 0 {
                obj.set_int_prop(
                    &ctx.ids,
                    ctx.ids.obj_x,
                    pos.get(2).and_then(|v| v.as_i64()).unwrap_or(0),
                );
            }
            if ctx.ids.obj_y != 0 {
                obj.set_int_prop(
                    &ctx.ids,
                    ctx.ids.obj_y,
                    pos.get(3).and_then(|v| v.as_i64()).unwrap_or(0),
                );
            }
        }
        if let Some(img_id) = load_thumb_image_id(ctx, thumb_no) {
            bind_capture_backend(ctx, obj, stage_idx, img_id);
        } else {
            ctx.unknown
                .record_note(&format!("thumb.image.missing:{thumb_no}"));
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_create_capture != 0 && op == ctx.ids.obj_create_capture {
        let (pos, _named) = split_pos_named(script_args);
        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.used = true;
        obj.object_type = 10;
        obj.movie.reset();
        obj.init_param_like();
        // Optional parameters: (disp, x, y) via al_id with different indexing.
        let aid = al_id.unwrap_or(0);
        if ctx.ids.obj_disp != 0 {
            let disp_i = if aid >= 1 {
                pos.get(0).and_then(|v| v.as_i64()).unwrap_or(0)
            } else {
                0
            };
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        }
        if aid >= 2 {
            if ctx.ids.obj_x != 0 {
                obj.set_int_prop(
                    &ctx.ids,
                    ctx.ids.obj_x,
                    pos.get(1).and_then(|v| v.as_i64()).unwrap_or(0),
                );
            }
            if ctx.ids.obj_y != 0 {
                obj.set_int_prop(
                    &ctx.ids,
                    ctx.ids.obj_y,
                    pos.get(2).and_then(|v| v.as_i64()).unwrap_or(0),
                );
            }
        }
        let cap = ctx.capture_frame_rgba();
        let img_id = ctx.images.insert_image(cap);
        bind_capture_backend(ctx, obj, stage_idx, img_id);
        push_ok(ctx, ret_form);
        return true;
    }

    // Movie creation variants share one implementation.
    {
        let mut loop_flag = false;
        let mut wait_flag = false;
        let mut key_skip_flag = false;
        let mut matched = false;

        if ctx.ids.obj_create_movie != 0 && op == ctx.ids.obj_create_movie {
            matched = true;
        } else if ctx.ids.obj_create_movie_loop != 0 && op == ctx.ids.obj_create_movie_loop {
            matched = true;
            loop_flag = true;
        } else if ctx.ids.obj_create_movie_wait != 0 && op == ctx.ids.obj_create_movie_wait {
            matched = true;
            wait_flag = true;
        } else if ctx.ids.obj_create_movie_wait_key != 0 && op == ctx.ids.obj_create_movie_wait_key
        {
            matched = true;
            wait_flag = true;
            key_skip_flag = true;
        }

        if matched {
            let (pos, named) = split_pos_named(script_args);
            let Some(file) = pos.get(0).and_then(|v| v.as_str()) else {
                push_ok(ctx, ret_form);
                return true;
            };

            // Named args (id -> bool)
            let mut auto_free_flag = true;
            let mut real_time_flag = true;
            let mut ready_only_flag = false;
            for (id, v) in named {
                match id {
                    0 => auto_free_flag = v.as_i64().unwrap_or(0) != 0,
                    1 => real_time_flag = v.as_i64().unwrap_or(0) != 0,
                    2 => ready_only_flag = v.as_i64().unwrap_or(0) != 0,
                    _ => {}
                }
            }

            object_clear_backend(ctx, obj, stage_idx, obj_u);
            obj.used = true;
            obj.object_type = 9;
            obj.file_name = Some(file.to_string());
            obj.string_value = None;
            obj.button.clear();
            obj.clear_runtime_only();

            let total_ms = resolve_movie_path(&ctx.project_dir, &ctx.globals.append_dir, file)
                .and_then(|_| movie_total_time_ms(ctx, file));
            obj.movie.start(
                total_ms,
                loop_flag,
                auto_free_flag,
                real_time_flag,
                ready_only_flag,
            );

            // Optional (disp, x, y) via al_id.
            let aid = al_id.unwrap_or(0);
            if ctx.ids.obj_disp != 0 {
                let disp_i = if aid >= 1 {
                    pos.get(1).and_then(|v| v.as_i64()).unwrap_or(0)
                } else {
                    0
                };
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
            }
            if aid >= 2 {
                if ctx.ids.obj_x != 0 {
                    obj.set_int_prop(
                        &ctx.ids,
                        ctx.ids.obj_x,
                        pos.get(2).and_then(|v| v.as_i64()).unwrap_or(0),
                    );
                }
                if ctx.ids.obj_y != 0 {
                    obj.set_int_prop(
                        &ctx.ids,
                        ctx.ids.obj_y,
                        pos.get(3).and_then(|v| v.as_i64()).unwrap_or(0),
                    );
                }
            }

            if wait_flag {
                // create_movie_wait(_key) calls wait_movie(key_skip_flag, key_skip_flag).
                ctx.wait.wait_object_movie(
                    ctx.ids.form_global_stage,
                    stage_idx,
                    obj_u,
                    key_skip_flag,
                    key_skip_flag,
                );
                if key_skip_flag && ret_form.unwrap_or(0) != 0 {
                    return true;
                }
            }

            push_ok(ctx, ret_form);
            return true;
        }
    }

    if ctx.ids.obj_create_emote != 0 && op == ctx.ids.obj_create_emote {
        let (pos, named) = split_pos_named(script_args);

        let width = pos.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
        let height = pos.get(1).and_then(|v| v.as_i64()).unwrap_or(0);
        let Some(file) = pos.get(2).and_then(|v| v.as_str()) else {
            push_ok(ctx, ret_form);
            return true;
        };

        let mut rep_x: i64 = 0;
        let mut rep_y: i64 = 0;
        for (id, v) in named {
            let iv = v.as_i64().unwrap_or(0);
            match id {
                0 => rep_x = iv,
                1 => rep_y = iv,
                _ => {}
            }
        }

        object_clear_backend(ctx, obj, stage_idx, obj_u);
        obj.used = true;
        obj.object_type = 12;
        obj.emote.width = width;
        obj.emote.height = height;
        obj.emote.file_name = Some(file.to_string());
        obj.emote.rep_x = rep_x;
        obj.emote.rep_y = rep_y;

        obj.file_name = Some(file.to_string());
        obj.movie.reset();
        obj.init_param_like();

        // Optional (disp, x, y) via al_id.
        let aid = al_id.unwrap_or(0);
        if ctx.ids.obj_disp != 0 {
            let disp_i = if aid >= 1 {
                pos.get(3).and_then(|v| v.as_i64()).unwrap_or(0)
            } else {
                0
            };
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        }
        if aid >= 2 {
            if ctx.ids.obj_x != 0 {
                obj.set_int_prop(
                    &ctx.ids,
                    ctx.ids.obj_x,
                    pos.get(4).and_then(|v| v.as_i64()).unwrap_or(0),
                );
            }
            if ctx.ids.obj_y != 0 {
                obj.set_int_prop(
                    &ctx.ids,
                    ctx.ids.obj_y,
                    pos.get(5).and_then(|v| v.as_i64()).unwrap_or(0),
                );
            }
        }

        push_ok(ctx, ret_form);
        return true;
    }

    // ---------------------------------------------------------------------
    // WEATHER params
    // ---------------------------------------------------------------------

    if ctx.ids.obj_set_weather_param_type_a != 0 && op == ctx.ids.obj_set_weather_param_type_a {
        let (_pos, named) = split_pos_named(script_args);
        let mut wp = ObjectWeatherParam {
            weather_type: 1,
            cnt: 0,
            pat_mode: 0,
            pat_no_00: 0,
            pat_no_01: 0,
            pat_time: 0,
            move_time_x: 0,
            move_time_y: 0,
            sin_time_x: 0,
            sin_power_x: 0,
            sin_time_y: 0,
            sin_power_y: 0,
            center_x: 0,
            center_y: 0,
            appear_range: 0,
            move_time: 0,
            center_rotate: 0,
            zoom_min: 0,
            zoom_max: 0,
            scale_x: TNM_SCALE_UNIT,
            scale_y: TNM_SCALE_UNIT,
            active_time: 45000,
            real_time_flag: false,
        };

        for (id, v) in named {
            let iv = v.as_i64().unwrap_or(0);
            match id {
                0 => wp.cnt = iv,
                1 => wp.pat_mode = iv,
                2 => wp.pat_no_00 = iv,
                3 => wp.pat_no_01 = iv,
                4 => wp.pat_time = iv,
                5 => wp.move_time_x = iv,
                6 => wp.move_time_y = iv,
                7 => wp.sin_time_x = iv,
                8 => wp.sin_power_x = iv,
                9 => wp.sin_time_y = iv,
                10 => wp.sin_power_y = iv,
                11 => wp.real_time_flag = iv != 0,
                12 => wp.scale_x = iv,
                13 => wp.scale_y = iv,
                14 => wp.active_time = iv,
                _ => {}
            }
        }

        obj.weather_param = wp;
        if matches!(obj.backend, ObjectBackend::Gfx) {
            let pat = obj.weather_param.pat_no_00;
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            let _ = gfx.object_set_patno(images, layers, stage_idx, obj_u as i64, pat);
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_weather_param_type_b != 0 && op == ctx.ids.obj_set_weather_param_type_b {
        let (_pos, named) = split_pos_named(script_args);
        let mut wp = ObjectWeatherParam {
            weather_type: 2,
            cnt: 0,
            pat_mode: 0,
            pat_no_00: 0,
            pat_no_01: 0,
            pat_time: 0,
            move_time_x: 0,
            move_time_y: 0,
            sin_time_x: 0,
            sin_power_x: 0,
            sin_time_y: 0,
            sin_power_y: 0,
            center_x: 0,
            center_y: 0,
            appear_range: 100,
            move_time: 1000,
            center_rotate: 0,
            zoom_min: TNM_SCALE_UNIT,
            zoom_max: TNM_SCALE_UNIT,
            scale_x: TNM_SCALE_UNIT,
            scale_y: TNM_SCALE_UNIT,
            active_time: 0,
            real_time_flag: false,
        };

        for (id, v) in named {
            let iv = v.as_i64().unwrap_or(0);
            match id {
                0 => wp.cnt = iv,
                1 => wp.pat_mode = iv,
                2 => wp.pat_no_00 = iv,
                3 => wp.pat_no_01 = iv,
                4 => wp.pat_time = iv,
                5 => wp.move_time = iv,
                6 => {}
                7 => wp.sin_time_x = iv,
                8 => wp.sin_power_x = iv,
                9 => wp.sin_time_y = iv,
                10 => wp.sin_power_y = iv,
                11 => wp.center_x = iv,
                12 => wp.center_y = iv,
                13 => wp.appear_range = iv,
                14 => wp.zoom_min = iv,
                15 => wp.zoom_max = iv,
                16 => wp.center_rotate = iv,
                17 => wp.real_time_flag = iv != 0,
                18 => wp.scale_x = iv,
                19 => wp.scale_y = iv,
                _ => {}
            }
        }

        obj.weather_param = wp;
        if matches!(obj.backend, ObjectBackend::Gfx) {
            let pat = obj.weather_param.pat_no_00;
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            let _ = gfx.object_set_patno(images, layers, stage_idx, obj_u as i64, pat);
        }
        push_ok(ctx, ret_form);
        return true;
    }

    // ---------------------------------------------------------------------
    // MOVIE ops
    // ---------------------------------------------------------------------

    if ctx.ids.obj_pause_movie != 0 && op == ctx.ids.obj_pause_movie {
        obj.movie.pause_flag = true;
        if let Some(id) = obj.movie.audio_id {
            ctx.movie.pause_audio(id);
        }
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_resume_movie != 0 && op == ctx.ids.obj_resume_movie {
        obj.movie.pause_flag = false;
        // If a movie was created in ready-only mode, resume starts playback.
        obj.movie.playing = true;
        if let Some(id) = obj.movie.audio_id {
            ctx.movie.resume_audio(id);
        }
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_seek_movie != 0 && op == ctx.ids.obj_seek_movie {
        let t = script_args
            .get(0)
            .and_then(|v| v.as_i64())
            .unwrap_or(0)
            .max(0) as u64;
        obj.movie.seek(t);
        if let Some(id) = obj.movie.audio_id.take() {
            ctx.movie.stop_audio(id);
        }
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_get_movie_seek_time != 0 && op == ctx.ids.obj_get_movie_seek_time {
        ctx.stack.push(Value::Int(obj.movie.get_seek_time() as i64));
        return true;
    }
    if ctx.ids.obj_check_movie != 0 && op == ctx.ids.obj_check_movie {
        ctx.stack
            .push(Value::Int(if obj.movie.check_movie() { 1 } else { 0 }));
        return true;
    }
    if ctx.ids.obj_wait_movie != 0 && op == ctx.ids.obj_wait_movie {
        if obj.movie.check_movie() {
            ctx.wait
                .wait_object_movie(ctx.ids.form_global_stage, stage_idx, obj_u, false, false);
        }
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_wait_movie_key != 0 && op == ctx.ids.obj_wait_movie_key {
        if obj.movie.check_movie() {
            // wait_movie(true, true)
            ctx.wait
                .wait_object_movie(ctx.ids.form_global_stage, stage_idx, obj_u, true, true);
            if ret_form.unwrap_or(0) != 0 {
                return true;
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_end_movie_loop != 0 && op == ctx.ids.obj_end_movie_loop {
        obj.movie.loop_flag = false;
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_set_movie_auto_free != 0 && op == ctx.ids.obj_set_movie_auto_free {
        obj.movie.auto_free_flag = script_args.get(0).and_then(|v| v.as_i64()).unwrap_or(0) != 0;
        push_ok(ctx, ret_form);
        return true;
    }

    // ---------------------------------------------------------------------
    // BUTTON ops (ID-mapped)
    // ---------------------------------------------------------------------

    if ctx.ids.obj_clear_button != 0 && op == ctx.ids.obj_clear_button {
        obj.button.clear();
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_button != 0 && op == ctx.ids.obj_set_button {
        let (pos, _named) = split_pos_named(script_args);
        let ints = [
            pos.get(0).and_then(|v| v.as_i64()).unwrap_or(0),
            pos.get(1).and_then(|v| v.as_i64()).unwrap_or(0),
            pos.get(2).and_then(|v| v.as_i64()).unwrap_or(0),
            pos.get(3).and_then(|v| v.as_i64()).unwrap_or(0),
        ];
        let mut button_no = 0i64;
        let mut group_no = 0i64;
        let mut action_no = 0i64;
        let mut se_no = 0i64;
        match al_id.unwrap_or(0) {
            2 => {
                button_no = ints[0];
                group_no = ints[1];
                action_no = ints[2];
                se_no = ints[3];
            }
            1 => {
                button_no = ints[0];
                group_no = ints[1];
            }
            _ => {
                button_no = ints[0];
            }
        }
        obj.button.enabled = true;
        obj.button.button_no = button_no;
        obj.button.group_no = group_no;
        obj.button.action_no = action_no;
        obj.button.se_no = se_no;
        obj.button.hit = false;
        obj.button.pushed = false;
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_button_group != 0 && op == ctx.ids.obj_set_button_group {
        // al_id==0: int group_no, al_id==1: element group
        if al_id == Some(1) {
            if let Some(Value::Element(e)) = script_args.get(0) {
                if let Some(StageTarget::ChildItemOp { idx: gidx, .. }) = parse_target(ctx, e) {
                    obj.button.group_idx_override = Some(gidx.max(0) as usize);
                }
            }
        } else {
            let g = script_args.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
            obj.button.group_no = g;
            obj.button.group_idx_override = None;
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_button_pushkeep != 0 && op == ctx.ids.obj_set_button_pushkeep {
        obj.button.push_keep = script_args.get(0).and_then(|v| v.as_i64()).unwrap_or(0) != 0;
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_get_button_pushkeep != 0 && op == ctx.ids.obj_get_button_pushkeep {
        ctx.stack
            .push(Value::Int(if obj.button.push_keep { 1 } else { 0 }));
        return true;
    }

    if ctx.ids.obj_set_button_alpha_test != 0 && op == ctx.ids.obj_set_button_alpha_test {
        obj.button.alpha_test = script_args.get(0).and_then(|v| v.as_i64()).unwrap_or(0) != 0;
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_get_button_alpha_test != 0 && op == ctx.ids.obj_get_button_alpha_test {
        ctx.stack
            .push(Value::Int(if obj.button.alpha_test { 1 } else { 0 }));
        return true;
    }

    if ctx.ids.obj_set_button_state_normal != 0 && op == ctx.ids.obj_set_button_state_normal {
        obj.button.state = TNM_BTN_STATE_NORMAL;
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_set_button_state_select != 0 && op == ctx.ids.obj_set_button_state_select {
        obj.button.state = TNM_BTN_STATE_SELECT;
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_set_button_state_disable != 0 && op == ctx.ids.obj_set_button_state_disable {
        obj.button.state = TNM_BTN_STATE_DISABLE;
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_get_button_state != 0 && op == ctx.ids.obj_get_button_state {
        ctx.stack.push(Value::Int(obj.button.state));
        return true;
    }

    if ctx.ids.obj_get_button_hit_state != 0 && op == ctx.ids.obj_get_button_hit_state {
        ctx.stack.push(Value::Int(if obj.button.hit {
            TNM_BTN_STATE_HIT
        } else {
            TNM_BTN_STATE_NORMAL
        }));
        return true;
    }

    if ctx.ids.obj_get_button_real_state != 0 && op == ctx.ids.obj_get_button_real_state {
        // Conservative: incorporate group selection.
        let mut stt = obj.button.state;
        if stt != TNM_BTN_STATE_SELECT && stt != TNM_BTN_STATE_DISABLE {
            if let Some(gidx) = obj.button.group_idx() {
                if let Some(gl) = st.group_lists.get(&stage_idx).and_then(|v| v.get(gidx)) {
                    if gl.decided_button_no == obj.button.button_no {
                        stt = TNM_BTN_STATE_PUSH;
                    } else if gl.pushed_button_no == obj.button.button_no {
                        stt = TNM_BTN_STATE_PUSH;
                    } else if gl.hit_button_no == obj.button.button_no {
                        stt = TNM_BTN_STATE_HIT;
                    }
                }
            } else if obj.button.pushed {
                stt = TNM_BTN_STATE_PUSH;
            } else if obj.button.hit {
                stt = TNM_BTN_STATE_HIT;
            }
        }
        ctx.stack.push(Value::Int(stt));
        return true;
    }

    if ctx.ids.obj_set_button_call != 0 && op == ctx.ids.obj_set_button_call {
        // uses current scene name + cmd_name argument.
        // This runtime does not track the current scene name yet, so we store an empty scn_name.
        let cmd = script_args.get(0).and_then(|v| v.as_str()).unwrap_or("");
        obj.button.decided_action_scn_name.clear();
        obj.button.decided_action_cmd_name = cmd.to_string();
        obj.button.decided_action_z_no = -1;
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_clear_button_call != 0 && op == ctx.ids.obj_clear_button_call {
        obj.button.decided_action_scn_name.clear();
        obj.button.decided_action_cmd_name.clear();
        obj.button.decided_action_z_no = -1;
        push_ok(ctx, ret_form);
        return true;
    }

    // ---------------------------------------------------------------------
    // FRAME_ACTION / GAN
    // ---------------------------------------------------------------------

    if ctx.ids.obj_load_gan != 0 && op == ctx.ids.obj_load_gan {
        let Some(name) = script_args.get(0).and_then(|v| v.as_str()) else {
            push_ok(ctx, ret_form);
            return true;
        };
        obj.gan_file = Some(name.to_string());
        if let Err(err) = obj
            .gan
            .load_gan(&ctx.project_dir, &ctx.globals.append_dir, name)
        {
            eprintln!("[GAN] load failed: {} ({})", name, err);
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_start_gan != 0 && op == ctx.ids.obj_start_gan {
        let mut set_no = 0i64;
        let mut loop_flag = true;
        let mut real_time_flag = false;
        if let Some(v) = script_args.get(0).and_then(as_i64) {
            set_no = v;
        }
        if let Some(v) = script_args.get(1).and_then(as_i64) {
            loop_flag = v != 0;
        }
        if let Some(v) = script_args.get(2).and_then(as_i64) {
            real_time_flag = v != 0;
        }
        obj.gan.start_anm(set_no as i32, loop_flag, real_time_flag);
        push_ok(ctx, ret_form);
        return true;
    }

    // Multi-arg setters.
    if ctx.ids.obj_set_pos != 0 && op == ctx.ids.obj_set_pos {
        let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
        let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
        let z = script_args.get(2).and_then(as_i64);
        match obj.backend {
            ObjectBackend::Rect {
                layer_id,
                sprite_id,
                ..
            } => {
                if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                    if let Some(spr) = layer.sprite_mut(sprite_id) {
                        spr.x = x as i32;
                        spr.y = y as i32;
                    }
                }
            }
            ObjectBackend::Gfx => {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                let _ = gfx.object_set_pos(images, layers, stage_idx, obj_u as i64, x, y);
                if let Some(zv) = z {
                    let _ = ctx.gfx.object_set_z(stage_idx, obj_u as i64, zv);
                }
            }
            _ => {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_x, x);
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_y, y);
                if let Some(zv) = z {
                    obj.set_int_prop(&ctx.ids, ctx.ids.obj_z, zv);
                }
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_center != 0 && op == ctx.ids.obj_set_center {
        let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
        let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
        let z = script_args.get(2).and_then(as_i64);
        if ctx.ids.obj_center_x != 0 {
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_center_x, x);
        }
        if ctx.ids.obj_center_y != 0 {
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_center_y, y);
        }
        if let Some(zv) = z {
            if ctx.ids.obj_center_z != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_center_z, zv);
            }
        }
        match obj.backend {
            ObjectBackend::Rect {
                layer_id,
                sprite_id,
                ..
            } => {
                if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                    if let Some(spr) = layer.sprite_mut(sprite_id) {
                        spr.pivot_x = x as f32;
                        spr.pivot_y = y as f32;
                    }
                }
            }
            ObjectBackend::Gfx => {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                let _ = gfx.object_set_center(images, layers, stage_idx, obj_u as i64, x, y);
            }
            _ => {}
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_center_rep != 0 && op == ctx.ids.obj_set_center_rep {
        let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
        let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
        let z = script_args.get(2).and_then(as_i64);
        if ctx.ids.obj_center_rep_x != 0 {
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_center_rep_x, x);
        }
        if ctx.ids.obj_center_rep_y != 0 {
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_center_rep_y, y);
        }
        if let Some(zv) = z {
            if ctx.ids.obj_center_rep_z != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_center_rep_z, zv);
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_scale != 0 && op == ctx.ids.obj_set_scale {
        let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
        let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
        let z = script_args.get(2).and_then(as_i64);
        if ctx.ids.obj_scale_x != 0 {
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_scale_x, x);
        }
        if ctx.ids.obj_scale_y != 0 {
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_scale_y, y);
        }
        if let Some(zv) = z {
            if ctx.ids.obj_scale_z != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_scale_z, zv);
            }
        }
        match obj.backend {
            ObjectBackend::Rect {
                layer_id,
                sprite_id,
                ..
            } => {
                if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                    if let Some(spr) = layer.sprite_mut(sprite_id) {
                        spr.scale_x = x as f32 / 1000.0;
                        spr.scale_y = y as f32 / 1000.0;
                    }
                }
            }
            ObjectBackend::Gfx => {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                let _ = gfx.object_set_scale(images, layers, stage_idx, obj_u as i64, x, y);
            }
            _ => {}
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_rotate != 0 && op == ctx.ids.obj_set_rotate {
        let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
        let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
        let z = script_args.get(2).and_then(as_i64);
        if ctx.ids.obj_rotate_x != 0 {
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_rotate_x, x);
        }
        if ctx.ids.obj_rotate_y != 0 {
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_rotate_y, y);
        }
        if let Some(zv) = z {
            if ctx.ids.obj_rotate_z != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_rotate_z, zv);
            }
        }
        if let Some(zv) = z {
            match obj.backend {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.rotate = zv as f32 * std::f32::consts::PI / 1800.0;
                        }
                    }
                }
                ObjectBackend::Gfx => {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ = gfx.object_set_rotate(images, layers, stage_idx, obj_u as i64, zv);
                }
                _ => {}
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_clip != 0 && op == ctx.ids.obj_set_clip {
        // (use, left, top, right, bottom)
        if script_args.len() >= 5 {
            let use_flag = script_args.get(0).and_then(as_i64).unwrap_or(0);
            let left = script_args.get(1).and_then(as_i64).unwrap_or(0);
            let top = script_args.get(2).and_then(as_i64).unwrap_or(0);
            let right = script_args.get(3).and_then(as_i64).unwrap_or(0);
            let bottom = script_args.get(4).and_then(as_i64).unwrap_or(0);
            if ctx.ids.obj_clip_use != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_clip_use, use_flag);
            }
            if ctx.ids.obj_clip_left != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_clip_left, left);
            }
            if ctx.ids.obj_clip_top != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_clip_top, top);
            }
            if ctx.ids.obj_clip_right != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_clip_right, right);
            }
            if ctx.ids.obj_clip_bottom != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_clip_bottom, bottom);
            }
            match obj.backend {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.dst_clip = if use_flag != 0 {
                                Some(crate::layer::ClipRect {
                                    left: left as i32,
                                    top: top as i32,
                                    right: right as i32,
                                    bottom: bottom as i32,
                                })
                            } else {
                                None
                            };
                        }
                    }
                }
                ObjectBackend::Gfx => {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ = gfx.object_set_clip(
                        images,
                        layers,
                        stage_idx,
                        obj_u as i64,
                        use_flag,
                        left,
                        top,
                        right,
                        bottom,
                    );
                }
                _ => {}
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_src_clip != 0 && op == ctx.ids.obj_set_src_clip {
        // (use, left, top, right, bottom)
        if script_args.len() >= 5 {
            let use_flag = script_args.get(0).and_then(as_i64).unwrap_or(0);
            let left = script_args.get(1).and_then(as_i64).unwrap_or(0);
            let top = script_args.get(2).and_then(as_i64).unwrap_or(0);
            let right = script_args.get(3).and_then(as_i64).unwrap_or(0);
            let bottom = script_args.get(4).and_then(as_i64).unwrap_or(0);
            if ctx.ids.obj_src_clip_use != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_src_clip_use, use_flag);
            }
            if ctx.ids.obj_src_clip_left != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_src_clip_left, left);
            }
            if ctx.ids.obj_src_clip_top != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_src_clip_top, top);
            }
            if ctx.ids.obj_src_clip_right != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_src_clip_right, right);
            }
            if ctx.ids.obj_src_clip_bottom != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_src_clip_bottom, bottom);
            }
            match obj.backend {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.src_clip = if use_flag != 0 {
                                Some(crate::layer::ClipRect {
                                    left: left as i32,
                                    top: top as i32,
                                    right: right as i32,
                                    bottom: bottom as i32,
                                })
                            } else {
                                None
                            };
                        }
                    }
                }
                ObjectBackend::Gfx => {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ = gfx.object_set_src_clip(
                        images,
                        layers,
                        stage_idx,
                        obj_u as i64,
                        use_flag,
                        left,
                        top,
                        right,
                        bottom,
                    );
                }
                _ => {}
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    {
        let mesh_anim_str_ids = [
            ctx.ids.obj_mesh_anim_clip_name,
            ctx.ids.obj_mesh_anim_blend_clip_name,
        ];
        if mesh_anim_str_ids.iter().any(|&id| id != 0 && op == id) {
            if let Some(s) = rhs.and_then(as_str).or_else(|| {
                if al_id == Some(1) && script_args.len() == 1 {
                    script_args.get(0).and_then(as_str)
                } else {
                    None
                }
            }) {
                let text = s.to_string();
                obj.set_str_prop(&ctx.ids, op, text.clone());
                let mut next = obj.mesh_animation_state.clone();
                if ctx.ids.obj_mesh_anim_clip_name != 0 && op == ctx.ids.obj_mesh_anim_clip_name {
                    next.change_animation_clip(Some(text), None);
                    if ctx.ids.obj_mesh_anim_clip != 0 {
                        obj.set_int_prop(&ctx.ids, ctx.ids.obj_mesh_anim_clip, -1);
                    }
                } else if ctx.ids.obj_mesh_anim_blend_clip_name != 0
                    && op == ctx.ids.obj_mesh_anim_blend_clip_name
                {
                    next.blend_clip_name = Some(text);
                    next.blend_clip_index = None;
                    if ctx.ids.obj_mesh_anim_blend_clip != 0 {
                        obj.set_int_prop(&ctx.ids, ctx.ids.obj_mesh_anim_blend_clip, -1);
                    }
                }
                obj.set_mesh_animation_state(next);
                ctx.stack.push(Value::Int(0));
            } else {
                let out = if ctx.ids.obj_mesh_anim_clip_name != 0
                    && op == ctx.ids.obj_mesh_anim_clip_name
                {
                    obj.mesh_animation_state
                        .clip_name
                        .clone()
                        .unwrap_or_default()
                } else if ctx.ids.obj_mesh_anim_blend_clip_name != 0
                    && op == ctx.ids.obj_mesh_anim_blend_clip_name
                {
                    obj.mesh_animation_state
                        .blend_clip_name
                        .clone()
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                ctx.stack.push(Value::Str(out));
            }
            return true;
        }

        let mesh_anim_int_ids = [
            ctx.ids.obj_mesh_anim_clip,
            ctx.ids.obj_mesh_anim_rate,
            ctx.ids.obj_mesh_anim_time_offset,
            ctx.ids.obj_mesh_anim_pause,
            ctx.ids.obj_mesh_anim_hold_time,
            ctx.ids.obj_mesh_anim_shift_time,
            ctx.ids.obj_mesh_anim_loop,
            ctx.ids.obj_mesh_anim_blend_clip,
            ctx.ids.obj_mesh_anim_blend_weight,
        ];
        if mesh_anim_int_ids.iter().any(|&id| id != 0 && op == id) {
            let set_v = rhs.and_then(as_i64).or_else(|| {
                if al_id == Some(1) && script_args.len() == 1 {
                    script_args.get(0).and_then(as_i64)
                } else {
                    None
                }
            });
            if let Some(v) = set_v {
                obj.set_int_prop(&ctx.ids, op, v);
                let mut next = obj.mesh_animation_state.clone();
                if ctx.ids.obj_mesh_anim_clip != 0 && op == ctx.ids.obj_mesh_anim_clip {
                    let clip_index = if v >= 0 { Some(v as usize) } else { None };
                    next.change_animation_clip(None, clip_index);
                    if ctx.ids.obj_mesh_anim_clip_name != 0 {
                        obj.remove_str_prop(&ctx.ids, ctx.ids.obj_mesh_anim_clip_name);
                    }
                } else if ctx.ids.obj_mesh_anim_rate != 0 && op == ctx.ids.obj_mesh_anim_rate {
                    next.rate = (v as f32) / 1000.0;
                } else if ctx.ids.obj_mesh_anim_time_offset != 0
                    && op == ctx.ids.obj_mesh_anim_time_offset
                {
                    next.time_offset_sec = (v as f32) / 1000.0;
                } else if ctx.ids.obj_mesh_anim_pause != 0 && op == ctx.ids.obj_mesh_anim_pause {
                    next.paused = v != 0;
                    next.is_anim = !next.paused;
                } else if ctx.ids.obj_mesh_anim_hold_time != 0
                    && op == ctx.ids.obj_mesh_anim_hold_time
                {
                    next.hold_time_sec = ((v as f32) / 1000.0).max(0.0);
                    next.time_sec = if next.rate > 0.0 {
                        next.hold_time_sec / next.rate.max(0.000_001)
                    } else {
                        0.0
                    };
                } else if ctx.ids.obj_mesh_anim_shift_time != 0
                    && op == ctx.ids.obj_mesh_anim_shift_time
                {
                    next.set_anim_shift_time_sec(((v as f32) / 1000.0).max(0.0));
                } else if ctx.ids.obj_mesh_anim_loop != 0 && op == ctx.ids.obj_mesh_anim_loop {
                    next.looped = v != 0;
                } else if ctx.ids.obj_mesh_anim_blend_clip != 0
                    && op == ctx.ids.obj_mesh_anim_blend_clip
                {
                    next.blend_clip_index = if v >= 0 { Some(v as usize) } else { None };
                    next.blend_clip_name = None;
                    if ctx.ids.obj_mesh_anim_blend_clip_name != 0 {
                        obj.remove_str_prop(&ctx.ids, ctx.ids.obj_mesh_anim_blend_clip_name);
                    }
                } else if ctx.ids.obj_mesh_anim_blend_weight != 0
                    && op == ctx.ids.obj_mesh_anim_blend_weight
                {
                    next.blend_weight = ((v as f32) / 1000.0).clamp(0.0, 1.0);
                }
                obj.set_mesh_animation_state(next);
                ctx.stack.push(Value::Int(0));
            } else {
                let out = if ctx.ids.obj_mesh_anim_clip != 0 && op == ctx.ids.obj_mesh_anim_clip {
                    obj.mesh_animation_state
                        .clip_index
                        .map(|v| v as i64)
                        .unwrap_or(-1)
                } else if ctx.ids.obj_mesh_anim_rate != 0 && op == ctx.ids.obj_mesh_anim_rate {
                    (obj.mesh_animation_state.rate * 1000.0).round() as i64
                } else if ctx.ids.obj_mesh_anim_time_offset != 0
                    && op == ctx.ids.obj_mesh_anim_time_offset
                {
                    (obj.mesh_animation_state.time_offset_sec * 1000.0).round() as i64
                } else if ctx.ids.obj_mesh_anim_pause != 0 && op == ctx.ids.obj_mesh_anim_pause {
                    if obj.mesh_animation_state.paused {
                        1
                    } else {
                        0
                    }
                } else if ctx.ids.obj_mesh_anim_hold_time != 0
                    && op == ctx.ids.obj_mesh_anim_hold_time
                {
                    (obj.mesh_animation_state.hold_time_sec * 1000.0).round() as i64
                } else if ctx.ids.obj_mesh_anim_shift_time != 0
                    && op == ctx.ids.obj_mesh_anim_shift_time
                {
                    (obj.mesh_animation_state.anim_shift_time_sec * 1000.0).round() as i64
                } else if ctx.ids.obj_mesh_anim_loop != 0 && op == ctx.ids.obj_mesh_anim_loop {
                    if obj.mesh_animation_state.looped {
                        1
                    } else {
                        0
                    }
                } else if ctx.ids.obj_mesh_anim_blend_clip != 0
                    && op == ctx.ids.obj_mesh_anim_blend_clip
                {
                    obj.mesh_animation_state
                        .blend_clip_index
                        .map(|v| v as i64)
                        .unwrap_or(-1)
                } else if ctx.ids.obj_mesh_anim_blend_weight != 0
                    && op == ctx.ids.obj_mesh_anim_blend_weight
                {
                    (obj.mesh_animation_state.blend_weight * 1000.0).round() as i64
                } else {
                    obj.lookup_int_prop(&ctx.ids, op).unwrap_or(0)
                };
                ctx.stack.push(Value::Int(out));
            }
            return true;
        }
    }

    // Simple int properties that do not currently affect the renderer.
    {
        let simple_ids = [
            ctx.ids.obj_world,
            ctx.ids.obj_x_rep,
            ctx.ids.obj_y_rep,
            ctx.ids.obj_z_rep,
            ctx.ids.obj_center_x,
            ctx.ids.obj_center_y,
            ctx.ids.obj_center_z,
            ctx.ids.obj_center_rep_x,
            ctx.ids.obj_center_rep_y,
            ctx.ids.obj_center_rep_z,
            ctx.ids.obj_scale_x,
            ctx.ids.obj_scale_y,
            ctx.ids.obj_scale_z,
            ctx.ids.obj_rotate_x,
            ctx.ids.obj_rotate_y,
            ctx.ids.obj_rotate_z,
            ctx.ids.obj_clip_use,
            ctx.ids.obj_clip_left,
            ctx.ids.obj_clip_top,
            ctx.ids.obj_clip_right,
            ctx.ids.obj_clip_bottom,
            ctx.ids.obj_src_clip_use,
            ctx.ids.obj_src_clip_left,
            ctx.ids.obj_src_clip_top,
            ctx.ids.obj_src_clip_right,
            ctx.ids.obj_src_clip_bottom,
            ctx.ids.obj_tr,
            ctx.ids.obj_tr_rep,
            ctx.ids.obj_mono,
            ctx.ids.obj_reverse,
            ctx.ids.obj_bright,
            ctx.ids.obj_dark,
            ctx.ids.obj_color_r,
            ctx.ids.obj_color_g,
            ctx.ids.obj_color_b,
            ctx.ids.obj_color_rate,
            ctx.ids.obj_color_add_r,
            ctx.ids.obj_color_add_g,
            ctx.ids.obj_color_add_b,
            ctx.ids.obj_mask_no,
            ctx.ids.obj_tonecurve_no,
            ctx.ids.obj_culling,
            ctx.ids.obj_alpha_test,
            ctx.ids.obj_alpha_blend,
            ctx.ids.obj_blend,
            ctx.ids.obj_light_no,
            ctx.ids.obj_fog_use,
        ];

        if simple_ids.iter().any(|&id| id != 0 && op == id) {
            let set_v = rhs.and_then(as_i64).or_else(|| {
                if al_id == Some(1) && script_args.len() == 1 {
                    script_args.get(0).and_then(as_i64)
                } else {
                    None
                }
            });
            if let Some(v) = set_v {
                obj.set_int_prop(&ctx.ids, op, v);
                if ctx.ids.obj_alpha_test != 0 && op == ctx.ids.obj_alpha_test {
                    obj.button.alpha_test = v != 0;
                }
                if op == ctx.ids.obj_tr
                    || op == ctx.ids.obj_mono
                    || op == ctx.ids.obj_reverse
                    || op == ctx.ids.obj_bright
                    || op == ctx.ids.obj_dark
                    || op == ctx.ids.obj_color_rate
                    || op == ctx.ids.obj_color_add_r
                    || op == ctx.ids.obj_color_add_g
                    || op == ctx.ids.obj_color_add_b
                    || op == ctx.ids.obj_color_r
                    || op == ctx.ids.obj_color_g
                    || op == ctx.ids.obj_color_b
                    || op == ctx.ids.obj_blend
                    || op == ctx.ids.obj_light_no
                    || op == ctx.ids.obj_fog_use
                {
                    match obj.backend {
                        ObjectBackend::Rect {
                            layer_id,
                            sprite_id,
                            ..
                        } => {
                            if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                                if let Some(spr) = layer.sprite_mut(sprite_id) {
                                    if op == ctx.ids.obj_tr {
                                        spr.tr = v.clamp(0, 255) as u8;
                                    } else if op == ctx.ids.obj_mono {
                                        spr.mono = v.clamp(0, 255) as u8;
                                    } else if op == ctx.ids.obj_reverse {
                                        spr.reverse = v.clamp(0, 255) as u8;
                                    } else if op == ctx.ids.obj_bright {
                                        spr.bright = v.clamp(0, 255) as u8;
                                    } else if op == ctx.ids.obj_dark {
                                        spr.dark = v.clamp(0, 255) as u8;
                                    } else if op == ctx.ids.obj_color_rate {
                                        spr.color_rate = v.clamp(0, 255) as u8;
                                    } else if op == ctx.ids.obj_color_add_r {
                                        spr.color_add_r = v.clamp(0, 255) as u8;
                                    } else if op == ctx.ids.obj_color_add_g {
                                        spr.color_add_g = v.clamp(0, 255) as u8;
                                    } else if op == ctx.ids.obj_color_add_b {
                                        spr.color_add_b = v.clamp(0, 255) as u8;
                                    } else if op == ctx.ids.obj_color_r {
                                        spr.color_r = v.clamp(0, 255) as u8;
                                    } else if op == ctx.ids.obj_color_g {
                                        spr.color_g = v.clamp(0, 255) as u8;
                                    } else if op == ctx.ids.obj_color_b {
                                        spr.color_b = v.clamp(0, 255) as u8;
                                    } else if op == ctx.ids.obj_blend {
                                        spr.blend = crate::layer::SpriteBlend::from_i64(v);
                                    } else if op == ctx.ids.obj_light_no {
                                        spr.light_no = v as i32;
                                    } else if op == ctx.ids.obj_fog_use {
                                        spr.fog_use = v != 0;
                                    }
                                }
                            }
                        }
                        ObjectBackend::Gfx => {
                            let (gfx, images, layers) =
                                (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                            if op == ctx.ids.obj_tr {
                                let _ =
                                    gfx.object_set_tr(images, layers, stage_idx, obj_u as i64, v);
                            } else if op == ctx.ids.obj_mono {
                                let _ =
                                    gfx.object_set_mono(images, layers, stage_idx, obj_u as i64, v);
                            } else if op == ctx.ids.obj_reverse {
                                let _ = gfx.object_set_reverse(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_u as i64,
                                    v,
                                );
                            } else if op == ctx.ids.obj_bright {
                                let _ = gfx.object_set_bright(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_u as i64,
                                    v,
                                );
                            } else if op == ctx.ids.obj_dark {
                                let _ =
                                    gfx.object_set_dark(images, layers, stage_idx, obj_u as i64, v);
                            } else if op == ctx.ids.obj_color_rate {
                                let _ = gfx.object_set_color_rate(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_u as i64,
                                    v,
                                );
                            } else if op == ctx.ids.obj_color_add_r {
                                let g = obj.get_int_prop(&ctx.ids, ctx.ids.obj_color_add_g);
                                let b = obj.get_int_prop(&ctx.ids, ctx.ids.obj_color_add_b);
                                let _ = gfx.object_set_color_add(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_u as i64,
                                    v,
                                    g,
                                    b,
                                );
                            } else if op == ctx.ids.obj_color_add_g {
                                let r = obj.get_int_prop(&ctx.ids, ctx.ids.obj_color_add_r);
                                let b = obj.get_int_prop(&ctx.ids, ctx.ids.obj_color_add_b);
                                let _ = gfx.object_set_color_add(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_u as i64,
                                    r,
                                    v,
                                    b,
                                );
                            } else if op == ctx.ids.obj_color_add_b {
                                let r = obj.get_int_prop(&ctx.ids, ctx.ids.obj_color_add_r);
                                let g = obj.get_int_prop(&ctx.ids, ctx.ids.obj_color_add_g);
                                let _ = gfx.object_set_color_add(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_u as i64,
                                    r,
                                    g,
                                    v,
                                );
                            } else if op == ctx.ids.obj_color_r {
                                let g = obj.get_int_prop(&ctx.ids, ctx.ids.obj_color_g);
                                let b = obj.get_int_prop(&ctx.ids, ctx.ids.obj_color_b);
                                let _ = gfx.object_set_color(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_u as i64,
                                    v,
                                    g,
                                    b,
                                );
                            } else if op == ctx.ids.obj_color_g {
                                let r = obj.get_int_prop(&ctx.ids, ctx.ids.obj_color_r);
                                let b = obj.get_int_prop(&ctx.ids, ctx.ids.obj_color_b);
                                let _ = gfx.object_set_color(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_u as i64,
                                    r,
                                    v,
                                    b,
                                );
                            } else if op == ctx.ids.obj_color_b {
                                let r = obj.get_int_prop(&ctx.ids, ctx.ids.obj_color_r);
                                let g = obj.get_int_prop(&ctx.ids, ctx.ids.obj_color_g);
                                let _ = gfx.object_set_color(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_u as i64,
                                    r,
                                    g,
                                    v,
                                );
                            } else if op == ctx.ids.obj_blend {
                                let _ = gfx.object_set_blend(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_u as i64,
                                    v,
                                );
                            } else if op == ctx.ids.obj_light_no {
                                let _ = gfx.object_set_light_no(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_u as i64,
                                    v,
                                );
                            } else if op == ctx.ids.obj_fog_use {
                                let _ = gfx.object_set_fog_use(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_u as i64,
                                    v,
                                );
                            }
                        }
                        _ => {}
                    }
                }
                ctx.stack.push(Value::Int(0));
            } else {
                ctx.stack.push(Value::Int(obj.get_int_prop(&ctx.ids, op)));
            }
            return true;
        }
    }

    // Query helpers.
    if ctx.ids.obj_get_pat_cnt != 0 && op == ctx.ids.obj_get_pat_cnt {
        // GET_PAT_CNT returns the available pattern count.
        let mut cnt = 0i64;
        if let Some(name) = obj.file_name.as_deref() {
            if let Ok((path, pct)) = crate::resource::find_g00_image_with_append_dir(
                ctx.images.project_dir(),
                &ctx.globals.append_dir,
                name,
            ) {
                match pct {
                    crate::resource::PctType::G00 => {
                        if let Ok(bytes) = std::fs::read(&path) {
                            if let Ok(decoded) = crate::assets::g00::decode_g00(&bytes) {
                                cnt = decoded.frames.len() as i64;
                            }
                        }
                    }
                    _ => {
                        cnt = 1;
                    }
                }
            }
        }
        ctx.stack.push(Value::Int(cnt));
        return true;
    }

    if (ctx.ids.obj_get_size_x != 0 && op == ctx.ids.obj_get_size_x)
        || (ctx.ids.obj_get_size_y != 0 && op == ctx.ids.obj_get_size_y)
        || (ctx.ids.obj_get_size_z != 0 && op == ctx.ids.obj_get_size_z)
    {
        // GET_SIZE_X/Y/Z(pat=0 or arg0)
        let pat = if al_id == Some(1) {
            script_args.get(0).and_then(as_i64).unwrap_or(0).max(0) as usize
        } else {
            0usize
        };

        let mut sx: i64 = 0;
        let mut sy: i64 = 0;
        let sz: i64 = 0;

        match obj.backend {
            ObjectBackend::Rect { width, height, .. } => {
                sx = width as i64;
                sy = height as i64;
            }
            _ => {
                if let Some(name) = obj.file_name.as_deref() {
                    if let Ok((path, _pct)) = crate::resource::find_g00_image_with_append_dir(
                        ctx.images.project_dir(),
                        &ctx.globals.append_dir,
                        name,
                    ) {
                        if let Ok(id) = ctx.images.load_file(&path, pat) {
                            if let Some(img) = ctx.images.get(id) {
                                sx = img.width as i64;
                                sy = img.height as i64;
                            }
                        }
                    }
                }
            }
        }

        if ctx.ids.obj_get_size_x != 0 && op == ctx.ids.obj_get_size_x {
            ctx.stack.push(Value::Int(sx));
        } else if ctx.ids.obj_get_size_y != 0 && op == ctx.ids.obj_get_size_y {
            ctx.stack.push(Value::Int(sy));
        } else {
            ctx.stack.push(Value::Int(sz));
        }
        return true;
    }

    if (ctx.ids.obj_get_pixel_color_r != 0 && op == ctx.ids.obj_get_pixel_color_r)
        || (ctx.ids.obj_get_pixel_color_g != 0 && op == ctx.ids.obj_get_pixel_color_g)
        || (ctx.ids.obj_get_pixel_color_b != 0 && op == ctx.ids.obj_get_pixel_color_b)
        || (ctx.ids.obj_get_pixel_color_a != 0 && op == ctx.ids.obj_get_pixel_color_a)
    {
        // GET_PIXEL_COLOR_* (x, y, pat=0 or arg2)
        let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
        let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
        let pat = if al_id == Some(1) {
            script_args.get(2).and_then(as_i64).unwrap_or(0).max(0) as usize
        } else {
            0usize
        };

        let mut out = 0i64;
        if x >= 0 && y >= 0 {
            if let Some(name) = obj.file_name.as_deref() {
                if let Ok((path, _pct)) = crate::resource::find_g00_image_with_append_dir(
                    ctx.images.project_dir(),
                    &ctx.globals.append_dir,
                    name,
                ) {
                    if let Ok(id) = ctx.images.load_file(&path, pat) {
                        if let Some(img) = ctx.images.get(id) {
                            let xi = x as u32;
                            let yi = y as u32;
                            if xi < img.width && yi < img.height {
                                let idx = ((yi * img.width + xi) * 4) as usize;
                                if idx + 4 <= img.rgba.len() {
                                    let r = img.rgba[idx] as i64;
                                    let g = img.rgba[idx + 1] as i64;
                                    let b = img.rgba[idx + 2] as i64;
                                    let a = img.rgba[idx + 3] as i64;
                                    out = if ctx.ids.obj_get_pixel_color_r != 0
                                        && op == ctx.ids.obj_get_pixel_color_r
                                    {
                                        r
                                    } else if ctx.ids.obj_get_pixel_color_g != 0
                                        && op == ctx.ids.obj_get_pixel_color_g
                                    {
                                        g
                                    } else if ctx.ids.obj_get_pixel_color_b != 0
                                        && op == ctx.ids.obj_get_pixel_color_b
                                    {
                                        b
                                    } else {
                                        a
                                    };
                                }
                            }
                        }
                    }
                }
            }
        }
        ctx.stack.push(Value::Int(out));
        return true;
    }

    let k = resolve_object_op(&ctx.ids, op);
    match k {
        ObjectOpKind::Init => {
            // INIT => reinit(true)
            object_clear_backend(ctx, obj, stage_idx, obj_u);
            obj.runtime.child_objects.clear();
            obj.init_type_like();
            obj.init_param_like();
            obj.used = true;
            ctx.stack.push(Value::Int(0));
            true
        }
        ObjectOpKind::Free => {
            // FREE => init_type(true)
            object_clear_backend(ctx, obj, stage_idx, obj_u);
            obj.init_type_like();
            obj.used = false;
            ctx.stack.push(Value::Int(0));
            true
        }
        ObjectOpKind::InitParam => {
            // INIT_PARAM => init_param(true)
            obj.init_param_like();
            ctx.stack.push(Value::Int(0));
            true
        }
        ObjectOpKind::ClearButton => {
            obj.button.clear();
            ctx.stack.push(Value::Int(0));
            true
        }
        ObjectOpKind::SetButton => {
            // Should have been handled by the early SetButton path.
            ctx.stack.push(Value::Int(0));
            true
        }
        ObjectOpKind::SetButtonGroup => {
            // Should have been handled by the early SetButtonGroup path.
            ctx.stack.push(Value::Int(0));
            true
        }

        ObjectOpKind::CreateRect => {
            if script_args.len() < 8 {
                push_ok(ctx, ret_form);
                return true;
            }
            let l = script_args.get(0).and_then(as_i64).unwrap_or(0);
            let t = script_args.get(1).and_then(as_i64).unwrap_or(0);
            let r = script_args.get(2).and_then(as_i64).unwrap_or(l);
            let b = script_args.get(3).and_then(as_i64).unwrap_or(t);

            let rr = script_args
                .get(4)
                .and_then(as_i64)
                .unwrap_or(0)
                .clamp(0, 255) as u8;
            let gg = script_args
                .get(5)
                .and_then(as_i64)
                .unwrap_or(0)
                .clamp(0, 255) as u8;
            let bb = script_args
                .get(6)
                .and_then(as_i64)
                .unwrap_or(0)
                .clamp(0, 255) as u8;
            let aa = script_args
                .get(7)
                .and_then(as_i64)
                .unwrap_or(255)
                .clamp(0, 255) as u8;

            // optional args depend on al_id; here we derive from argc.
            let disp = if script_args.len() >= 9 {
                script_args.get(8).and_then(as_i64).unwrap_or(0) != 0
            } else {
                false
            };
            let (x, y) = if script_args.len() >= 11 {
                (
                    script_args.get(9).and_then(as_i64).unwrap_or(0) as i32,
                    script_args.get(10).and_then(as_i64).unwrap_or(0) as i32,
                )
            } else {
                (0, 0)
            };

            let w = (l.abs_diff(r)).clamp(1, 32767) as u32;
            let h = (t.abs_diff(b)).clamp(1, 32767) as u32;

            // If we were a gfx object, clear it; for an existing rect backend we reuse the sprite.
            if matches!(obj.backend, ObjectBackend::Gfx) {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                let _ = gfx.object_clear(images, layers, stage_idx, obj_u as i64);
            }

            let layer_id = ensure_rect_layer(ctx, st, stage_idx);
            let sprite_id = match obj.backend {
                ObjectBackend::Rect {
                    layer_id: lid,
                    sprite_id: sid,
                    ..
                } if lid == layer_id => sid,
                _ => {
                    let Some(sid) = ctx
                        .layers
                        .layer_mut(layer_id)
                        .map(|layer| layer.create_sprite())
                    else {
                        push_ok(ctx, ret_form);
                        return true;
                    };
                    sid
                }
            };

            if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                if let Some(spr) = layer.sprite_mut(sprite_id) {
                    let img = ctx.images.solid_rgba((rr, gg, bb, aa));
                    spr.image_id = Some(img);
                    spr.fit = SpriteFit::PixelRect;
                    spr.size_mode = SpriteSizeMode::Explicit {
                        width: w,
                        height: h,
                    };
                    spr.visible = disp;
                    spr.x = x as i32;
                    spr.y = y as i32;
                }
            }

            obj.init_type_like();
            obj.init_param_like();
            obj.used = true;
            obj.backend = ObjectBackend::Rect {
                layer_id,
                sprite_id,
                width: w,
                height: h,
            };
            obj.object_type = 1;
            if ctx.ids.obj_disp != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp { 1 } else { 0 });
            }
            if ctx.ids.obj_x != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_x, x as i64);
            }
            if ctx.ids.obj_y != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_y, y as i64);
            }
            push_ok(ctx, ret_form);
            true
        }
        ObjectOpKind::CreateString => {
            let Some(s0) = script_args.get(0).and_then(as_str) else {
                push_ok(ctx, ret_form);
                return true;
            };

            // optional disp and pos depend on al_id; derive from argc.
            let disp = if script_args.len() >= 2 {
                script_args.get(1).and_then(as_i64).unwrap_or(0) != 0
            } else {
                false
            };
            let (x, y) = if script_args.len() >= 4 {
                (
                    script_args.get(2).and_then(as_i64).unwrap_or(0),
                    script_args.get(3).and_then(as_i64).unwrap_or(0),
                )
            } else {
                (0, 0)
            };

            object_clear_backend(ctx, obj, stage_idx, obj_u);
            obj.init_type_like();
            obj.init_param_like();
            obj.used = true;
            obj.backend = ObjectBackend::None;
            obj.object_type = 3;
            obj.string_value = Some(s0.to_string());

            // Preserve representable base params through the fixed object base state.
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp { 1 } else { 0 });
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_x, x);
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_y, y);

            update_string_backend(ctx, st, obj, stage_idx);
            push_ok(ctx, ret_form);
            true
        }
        ObjectOpKind::CreatePct => {
            let Some(file) = script_args.get(0).and_then(as_str) else {
                push_ok(ctx, ret_form);
                return true;
            };

            // Original cmd_object.cpp uses fall-through by al_id:
            //   al_id==1 => disp
            //   al_id==2 => disp,x,y
            //   al_id==3 => disp,x,y,patno
            let aid = al_id.unwrap_or(0);
            let disp = if aid >= 1 {
                script_args.get(1).and_then(as_i64).unwrap_or(0) != 0
            } else {
                false
            };
            let x = if aid >= 2 {
                script_args.get(2).and_then(as_i64).unwrap_or(0)
            } else {
                0
            };
            let y = if aid >= 2 {
                script_args.get(3).and_then(as_i64).unwrap_or(0)
            } else {
                0
            };
            let patno = if aid >= 3 {
                script_args.get(4).and_then(as_i64).unwrap_or(0)
            } else {
                0
            };
            sg_debug_stage(format!(
                "stage={} obj={} CREATE file={} al_id={:?} disp={} x={} y={} patno={}",
                stage_idx, obj_u, file, al_id, disp, x, y, patno
            ));

            object_clear_backend(ctx, obj, stage_idx, obj_u);
            obj.init_type_like();
            obj.init_param_like();

            {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                let _ = gfx.object_create(
                    images,
                    layers,
                    stage_idx,
                    obj_u as i64,
                    file,
                    disp as i64,
                    x,
                    y,
                    patno,
                );
            }
            obj.used = true;
            obj.backend = ObjectBackend::Gfx;
            obj.object_type = 2;
            obj.number_value = 0;
            obj.string_param = Default::default();
            obj.number_param = Default::default();
            obj.file_name = Some(file.to_string());
            obj.string_value = None;
            if ctx.ids.obj_disp != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp { 1 } else { 0 });
            }
            if ctx.ids.obj_x != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_x, x);
            }
            if ctx.ids.obj_y != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_y, y);
            }
            if ctx.ids.obj_patno != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_patno, patno);
            }
            push_ok(ctx, ret_form);
            true
        }
        ObjectOpKind::SetPos => {
            let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
            let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
            let z = script_args.get(2).and_then(as_i64);

            match obj.backend {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.x = x as i32;
                            spr.y = y as i32;
                        }
                    }
                }
                ObjectBackend::String {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.x = x as i32;
                            spr.y = y as i32;
                        }
                    }
                    obj.set_int_prop(&ctx.ids, ctx.ids.obj_x, x);
                    obj.set_int_prop(&ctx.ids, ctx.ids.obj_y, y);
                }
                ObjectBackend::Gfx => {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ = gfx.object_set_pos(images, layers, stage_idx, obj_u as i64, x, y);
                    if let Some(zv) = z {
                        let _ = ctx.gfx.object_set_z(stage_idx, obj_u as i64, zv);
                    }
                }
                _ => {
                    obj.set_int_prop(&ctx.ids, ctx.ids.obj_x, x);
                    obj.set_int_prop(&ctx.ids, ctx.ids.obj_y, y);
                    if let Some(zv) = z {
                        obj.set_int_prop(&ctx.ids, op, zv);
                    }
                }
            }

            push_ok(ctx, ret_form);
            true
        }
        ObjectOpKind::SetCenter => {
            let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
            let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
            let z = script_args.get(2).and_then(as_i64).unwrap_or(0);

            match obj.backend.clone() {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.pivot_x = x as f32;
                            spr.pivot_y = y as f32;
                        }
                    }
                }
                ObjectBackend::String {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.pivot_x = x as f32;
                            spr.pivot_y = y as f32;
                        }
                    }
                }
                ObjectBackend::Gfx => {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ = gfx.object_set_center(images, layers, stage_idx, obj_u as i64, x, y);
                }
                _ => {}
            }

            push_ok(ctx, ret_form);
            true
        }
        ObjectOpKind::SetScale => {
            let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
            let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
            let z = script_args.get(2).and_then(as_i64).unwrap_or(0);

            match obj.backend.clone() {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.scale_x = x as f32 / 1000.0;
                            spr.scale_y = y as f32 / 1000.0;
                        }
                    }
                }
                ObjectBackend::String {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.scale_x = x as f32 / 1000.0;
                            spr.scale_y = y as f32 / 1000.0;
                        }
                    }
                }
                ObjectBackend::Gfx => {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ = gfx.object_set_scale(images, layers, stage_idx, obj_u as i64, x, y);
                }
                _ => {}
            }

            push_ok(ctx, ret_form);
            true
        }
        ObjectOpKind::SetRotate => {
            let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
            let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
            let z = script_args.get(2).and_then(as_i64).unwrap_or(0);

            match obj.backend.clone() {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.rotate = z as f32 * std::f32::consts::PI / 1800.0;
                        }
                    }
                }
                ObjectBackend::String {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.rotate = z as f32 * std::f32::consts::PI / 1800.0;
                        }
                    }
                }
                ObjectBackend::Gfx => {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ = gfx.object_set_rotate(images, layers, stage_idx, obj_u as i64, z);
                }
                _ => {}
            }

            push_ok(ctx, ret_form);
            true
        }
        ObjectOpKind::SetClip => {
            let use_flag = script_args.get(0).and_then(as_i64).unwrap_or(0);
            let left = script_args.get(1).and_then(as_i64).unwrap_or(0);
            let top = script_args.get(2).and_then(as_i64).unwrap_or(0);
            let right = script_args.get(3).and_then(as_i64).unwrap_or(0);
            let bottom = script_args.get(4).and_then(as_i64).unwrap_or(0);

            match obj.backend.clone() {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.dst_clip = if use_flag != 0 {
                                Some(crate::layer::ClipRect {
                                    left: left as i32,
                                    top: top as i32,
                                    right: right as i32,
                                    bottom: bottom as i32,
                                })
                            } else {
                                None
                            };
                        }
                    }
                }
                ObjectBackend::String {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.dst_clip = if use_flag != 0 {
                                Some(crate::layer::ClipRect {
                                    left: left as i32,
                                    top: top as i32,
                                    right: right as i32,
                                    bottom: bottom as i32,
                                })
                            } else {
                                None
                            };
                        }
                    }
                }
                ObjectBackend::Gfx => {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ = gfx.object_set_clip(
                        images,
                        layers,
                        stage_idx,
                        obj_u as i64,
                        use_flag,
                        left,
                        top,
                        right,
                        bottom,
                    );
                }
                _ => {}
            }

            push_ok(ctx, ret_form);
            true
        }
        ObjectOpKind::SetSrcClip => {
            let use_flag = script_args.get(0).and_then(as_i64).unwrap_or(0);
            let left = script_args.get(1).and_then(as_i64).unwrap_or(0);
            let top = script_args.get(2).and_then(as_i64).unwrap_or(0);
            let right = script_args.get(3).and_then(as_i64).unwrap_or(0);
            let bottom = script_args.get(4).and_then(as_i64).unwrap_or(0);

            match obj.backend.clone() {
                ObjectBackend::Rect {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.src_clip = if use_flag != 0 {
                                Some(crate::layer::ClipRect {
                                    left: left as i32,
                                    top: top as i32,
                                    right: right as i32,
                                    bottom: bottom as i32,
                                })
                            } else {
                                None
                            };
                        }
                    }
                }
                ObjectBackend::String {
                    layer_id,
                    sprite_id,
                    ..
                } => {
                    if let Some(layer) = ctx.layers.layer_mut(layer_id) {
                        if let Some(spr) = layer.sprite_mut(sprite_id) {
                            spr.src_clip = if use_flag != 0 {
                                Some(crate::layer::ClipRect {
                                    left: left as i32,
                                    top: top as i32,
                                    right: right as i32,
                                    bottom: bottom as i32,
                                })
                            } else {
                                None
                            };
                        }
                    }
                }
                ObjectBackend::Gfx => {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ = gfx.object_set_src_clip(
                        images,
                        layers,
                        stage_idx,
                        obj_u as i64,
                        use_flag,
                        left,
                        top,
                        right,
                        bottom,
                    );
                }
                _ => {}
            }

            push_ok(ctx, ret_form);
            true
        }
        _ => false,
    }
}

fn ensure_world_list(st: &mut StageFormState, stage_idx: i64, cnt: usize) {
    let list = st.world_lists.entry(stage_idx).or_insert_with(Vec::new);
    if list.len() < cnt {
        for i in list.len()..cnt {
            list.push(WorldState::new(i as i32));
        }
    } else if list.len() > cnt {
        list.truncate(cnt);
    }
}

fn dispatch_world_list_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    op: i32,
    script_args: &[Value],
    ret_form: Option<i64>,
) -> bool {
    let ids = ctx.ids.clone();
    let list = st.world_lists.entry(stage_idx).or_insert_with(Vec::new);

    if ids.worldlist_create != 0 && op == ids.worldlist_create {
        let old = list.len() as i64;
        list.push(WorldState::new(list.len() as i32));
        ctx.stack.push(Value::Int(old));
        return true;
    }

    if ids.worldlist_destroy != 0 && op == ids.worldlist_destroy {
        if !list.is_empty() {
            list.pop();
        }
        ctx.stack.push(Value::Int(0));
        return true;
    }

    if script_args.is_empty() && ret_form.unwrap_or(0) != 0 {
        ctx.stack.push(Value::Int(list.len() as i64));
        return true;
    }

    if script_args.is_empty() && ret_form.unwrap_or(0) == 0 {
        let old = list.len() as i64;
        list.push(WorldState::new(list.len() as i32));
        ctx.stack.push(Value::Int(old));
        return true;
    }

    false
}

fn dispatch_world_item_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    idx: usize,
    op: i32,
    tail: &[i32],
    script_args: &[Value],
    rhs: Option<&Value>,
    al_id: Option<i64>,
    ret_form: Option<i64>,
) -> bool {
    ensure_world_list(st, stage_idx, idx + 1);
    let list = st.world_lists.get_mut(&stage_idx).unwrap();
    let w = &mut list[idx];
    let ids = ctx.ids.clone();

    let set_v = rhs.and_then(as_i64).or_else(|| {
        if al_id == Some(1) && script_args.len() == 1 {
            script_args.get(0).and_then(as_i64)
        } else {
            None
        }
    });

    let is_event_tail = !tail.is_empty() && (0..=4).contains(&tail[0]);

    let mut handle_event = |ev: &mut IntEvent| {
        world_handle_event(ctx, ev, set_v, is_event_tail, script_args, ret_form)
    };

    if ids.world_init != 0 && op == ids.world_init {
        w.reinit();
        push_ok(ctx, ret_form);
        return true;
    }
    if ids.world_get_no != 0 && op == ids.world_get_no {
        ctx.stack.push(Value::Int(w.world_no as i64));
        return true;
    }
    if ids.world_mode != 0 && op == ids.world_mode {
        if let Some(v) = set_v {
            w.mode = if v == 0 { 0 } else { 1 };
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack.push(Value::Int(w.mode as i64));
        }
        return true;
    }

    if ids.world_camera_eye_x_eve != 0 && op == ids.world_camera_eye_x_eve {
        return handle_event(&mut w.camera_eye_x);
    }
    if ids.world_camera_eye_y_eve != 0 && op == ids.world_camera_eye_y_eve {
        return handle_event(&mut w.camera_eye_y);
    }
    if ids.world_camera_eye_z_eve != 0 && op == ids.world_camera_eye_z_eve {
        return handle_event(&mut w.camera_eye_z);
    }
    if ids.world_camera_pint_x_eve != 0 && op == ids.world_camera_pint_x_eve {
        return handle_event(&mut w.camera_pint_x);
    }
    if ids.world_camera_pint_y_eve != 0 && op == ids.world_camera_pint_y_eve {
        return handle_event(&mut w.camera_pint_y);
    }
    if ids.world_camera_pint_z_eve != 0 && op == ids.world_camera_pint_z_eve {
        return handle_event(&mut w.camera_pint_z);
    }
    if ids.world_camera_up_x_eve != 0 && op == ids.world_camera_up_x_eve {
        return handle_event(&mut w.camera_up_x);
    }
    if ids.world_camera_up_y_eve != 0 && op == ids.world_camera_up_y_eve {
        return handle_event(&mut w.camera_up_y);
    }
    if ids.world_camera_up_z_eve != 0 && op == ids.world_camera_up_z_eve {
        return handle_event(&mut w.camera_up_z);
    }

    if ids.world_camera_eye_x != 0 && op == ids.world_camera_eye_x {
        if let Some(v) = set_v {
            w.camera_eye_x.set_value(v as i32);
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack
                .push(Value::Int(w.camera_eye_x.get_value() as i64));
        }
        return true;
    }
    if ids.world_camera_eye_y != 0 && op == ids.world_camera_eye_y {
        if let Some(v) = set_v {
            w.camera_eye_y.set_value(v as i32);
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack
                .push(Value::Int(w.camera_eye_y.get_value() as i64));
        }
        return true;
    }
    if ids.world_camera_eye_z != 0 && op == ids.world_camera_eye_z {
        if let Some(v) = set_v {
            w.camera_eye_z.set_value(v as i32);
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack
                .push(Value::Int(w.camera_eye_z.get_value() as i64));
        }
        return true;
    }
    if ids.world_camera_pint_x != 0 && op == ids.world_camera_pint_x {
        if let Some(v) = set_v {
            w.camera_pint_x.set_value(v as i32);
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack
                .push(Value::Int(w.camera_pint_x.get_value() as i64));
        }
        return true;
    }
    if ids.world_camera_pint_y != 0 && op == ids.world_camera_pint_y {
        if let Some(v) = set_v {
            w.camera_pint_y.set_value(v as i32);
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack
                .push(Value::Int(w.camera_pint_y.get_value() as i64));
        }
        return true;
    }
    if ids.world_camera_pint_z != 0 && op == ids.world_camera_pint_z {
        if let Some(v) = set_v {
            w.camera_pint_z.set_value(v as i32);
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack
                .push(Value::Int(w.camera_pint_z.get_value() as i64));
        }
        return true;
    }
    if ids.world_camera_up_x != 0 && op == ids.world_camera_up_x {
        if let Some(v) = set_v {
            w.camera_up_x.set_value(v as i32);
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack.push(Value::Int(w.camera_up_x.get_value() as i64));
        }
        return true;
    }
    if ids.world_camera_up_y != 0 && op == ids.world_camera_up_y {
        if let Some(v) = set_v {
            w.camera_up_y.set_value(v as i32);
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack.push(Value::Int(w.camera_up_y.get_value() as i64));
        }
        return true;
    }
    if ids.world_camera_up_z != 0 && op == ids.world_camera_up_z {
        if let Some(v) = set_v {
            w.camera_up_z.set_value(v as i32);
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack.push(Value::Int(w.camera_up_z.get_value() as i64));
        }
        return true;
    }

    if ids.world_camera_view_angle != 0 && op == ids.world_camera_view_angle {
        if let Some(v) = set_v {
            w.camera_view_angle = v as i32;
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack.push(Value::Int(w.camera_view_angle as i64));
        }
        return true;
    }
    if ids.world_mono != 0 && op == ids.world_mono {
        if let Some(v) = set_v {
            w.mono = v as i32;
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack.push(Value::Int(w.mono as i64));
        }
        return true;
    }
    if ids.world_order != 0 && op == ids.world_order {
        if let Some(v) = set_v {
            w.order = v as i32;
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack.push(Value::Int(w.order as i64));
        }
        return true;
    }
    if ids.world_layer != 0 && op == ids.world_layer {
        if let Some(v) = set_v {
            w.layer = v as i32;
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack.push(Value::Int(w.layer as i64));
        }
        return true;
    }
    if ids.world_wipe_copy != 0 && op == ids.world_wipe_copy {
        if let Some(v) = set_v {
            w.wipe_copy = v as i32;
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack.push(Value::Int(w.wipe_copy as i64));
        }
        return true;
    }
    if ids.world_wipe_erase != 0 && op == ids.world_wipe_erase {
        if let Some(v) = set_v {
            w.wipe_erase = v as i32;
            ctx.stack.push(Value::Int(0));
        } else {
            ctx.stack.push(Value::Int(w.wipe_erase as i64));
        }
        return true;
    }

    if ids.world_set_camera_eye != 0 && op == ids.world_set_camera_eye {
        let x = script_args.get(0).and_then(as_i64).unwrap_or(0) as i32;
        let y = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
        let z = script_args.get(2).and_then(as_i64).unwrap_or(0) as i32;
        w.camera_eye_x.set_value(x);
        w.camera_eye_y.set_value(y);
        w.camera_eye_z.set_value(z);
        push_ok(ctx, ret_form);
        return true;
    }

    if ids.world_set_camera_pint != 0 && op == ids.world_set_camera_pint {
        let x = script_args.get(0).and_then(as_i64).unwrap_or(0) as i32;
        let y = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
        let z = script_args.get(2).and_then(as_i64).unwrap_or(0) as i32;
        w.camera_pint_x.set_value(x);
        w.camera_pint_y.set_value(y);
        w.camera_pint_z.set_value(z);
        push_ok(ctx, ret_form);
        return true;
    }

    if ids.world_set_camera_up != 0 && op == ids.world_set_camera_up {
        let x = script_args.get(0).and_then(as_i64).unwrap_or(0) as i32;
        let y = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
        let z = script_args.get(2).and_then(as_i64).unwrap_or(0) as i32;
        w.camera_up_x.set_value(x);
        w.camera_up_y.set_value(y);
        w.camera_up_z.set_value(z);
        push_ok(ctx, ret_form);
        return true;
    }

    if ids.world_calc_camera_eye != 0 && op == ids.world_calc_camera_eye {
        let distance = script_args.get(0).and_then(as_i64).unwrap_or(0) as f64;
        let rotate_h =
            (script_args.get(1).and_then(as_i64).unwrap_or(0) as f64 / 10.0).to_radians();
        let rotate_v =
            (script_args.get(2).and_then(as_i64).unwrap_or(0) as f64 / 10.0).to_radians();
        let px = w.camera_pint_x.get_value() as f64;
        let py = w.camera_pint_y.get_value() as f64;
        let pz = w.camera_pint_z.get_value() as f64;
        let x = (px - distance * rotate_h.sin() * rotate_v.cos()) as i32;
        let y = (py + distance * rotate_v.sin()) as i32;
        let z = (pz - distance * rotate_h.cos() * rotate_v.cos()) as i32;
        w.camera_eye_x.set_value(x);
        w.camera_eye_y.set_value(y);
        w.camera_eye_z.set_value(z);
        push_ok(ctx, ret_form);
        return true;
    }

    if ids.world_calc_camera_pint != 0 && op == ids.world_calc_camera_pint {
        let distance = script_args.get(0).and_then(as_i64).unwrap_or(0) as f64;
        let rotate_h =
            (script_args.get(1).and_then(as_i64).unwrap_or(0) as f64 / 10.0).to_radians();
        let rotate_v =
            (script_args.get(2).and_then(as_i64).unwrap_or(0) as f64 / 10.0).to_radians();
        let ex = w.camera_eye_x.get_value() as f64;
        let ey = w.camera_eye_y.get_value() as f64;
        let ez = w.camera_eye_z.get_value() as f64;
        let x = (ex + distance * rotate_h.sin() * rotate_v.cos()) as i32;
        let y = (ey + distance * rotate_v.sin()) as i32;
        let z = (ez + distance * rotate_h.cos() * rotate_v.cos()) as i32;
        w.camera_pint_x.set_value(x);
        w.camera_pint_y.set_value(y);
        w.camera_pint_z.set_value(z);
        push_ok(ctx, ret_form);
        return true;
    }

    if ids.world_set_camera_eve_xz_rotate != 0 && op == ids.world_set_camera_eve_xz_rotate {
        let x = script_args.get(0).and_then(as_i64).unwrap_or(0) as i32;
        let z = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
        let time = script_args.get(2).and_then(as_i64).unwrap_or(0) as i32;
        let rep_time = script_args.get(3).and_then(as_i64).unwrap_or(0) as i32;
        let speed_type = script_args.get(4).and_then(as_i64).unwrap_or(0) as i32;

        w.camera_eye_xz_eve.loop_type = 0;
        w.camera_eye_xz_eve.cur_time = 0;
        w.camera_eye_xz_eve.end_time = time;
        w.camera_eye_xz_eve.delay_time = rep_time;
        w.camera_eye_xz_eve.speed_type = speed_type;

        w.camera_eye_x.start_value = w.camera_eye_x.value;
        w.camera_eye_z.start_value = w.camera_eye_z.value;
        w.camera_eye_x.end_value = x;
        w.camera_eye_z.end_value = z;

        w.camera_eye_x.set_value(x);
        w.camera_eye_z.set_value(z);
        push_ok(ctx, ret_form);
        return true;
    }

    if is_event_tail {
        let ev = w
            .script_events
            .entry(op)
            .or_insert_with(|| IntEvent::new(0));
        if let Some(v) = dispatch_int_event_like(ev, script_args, ret_form) {
            ctx.stack.push(v);
        } else {
            push_ok(ctx, ret_form);
        }
        return true;
    }

    if let Some(Value::Str(v)) = rhs {
        w.extra_str.insert(op, v.clone());
        ctx.stack.push(Value::Int(0));
        return true;
    }
    if let Some(Value::Int(v)) = rhs {
        w.extra_int.insert(op, *v);
        ctx.stack.push(Value::Int(0));
        return true;
    }
    if rhs.is_none() {
        if let Some(s) = w.extra_str.get(&op) {
            ctx.stack.push(Value::Str(s.to_string()));
        } else {
            ctx.stack
                .push(Value::Int(*w.extra_int.get(&op).unwrap_or(&0)));
        }
        return true;
    }

    push_ok(ctx, ret_form);
    true
}

fn world_handle_event(
    ctx: &mut CommandContext,
    ev: &mut IntEvent,
    set_v: Option<i64>,
    is_event_tail: bool,
    script_args: &[Value],
    ret_form: Option<i64>,
) -> bool {
    if is_event_tail {
        if let Some(v) = dispatch_int_event_like(ev, script_args, ret_form) {
            ctx.stack.push(v);
        } else {
            push_ok(ctx, ret_form);
        }
        return true;
    }
    if let Some(v) = set_v {
        ev.set_value(v as i32);
        ctx.stack.push(Value::Int(0));
    } else {
        ctx.stack.push(Value::Int(ev.get_value() as i64));
    }
    true
}

fn resolve_group_list_op(_ids: &constants::RuntimeConstants, op: i32) -> Option<GroupListOpKind> {
    match op {
        constants::GROUPLIST_ALLOC => Some(GroupListOpKind::Alloc),
        constants::GROUPLIST_FREE => Some(GroupListOpKind::Free),
        _ => None,
    }
}

fn resolve_group_op(_ids: &constants::RuntimeConstants, op: i32) -> Option<GroupOpKind> {
    match op {
        constants::GROUP_SEL_CANCEL => Some(GroupOpKind::SelCancel),
        constants::GROUP_SEL => Some(GroupOpKind::Sel),
        constants::GROUP_INIT => Some(GroupOpKind::Init),
        constants::GROUP_START_CANCEL => Some(GroupOpKind::StartCancel),
        constants::GROUP_START => Some(GroupOpKind::Start),
        constants::GROUP_END => Some(GroupOpKind::End),
        constants::GROUP_GET_HIT_NO => Some(GroupOpKind::GetHitNo),
        constants::GROUP_GET_PUSHED_NO => Some(GroupOpKind::GetPushedNo),
        constants::GROUP_GET_DECIDED_NO => Some(GroupOpKind::GetDecidedNo),
        constants::GROUP_GET_RESULT_BUTTON_NO => Some(GroupOpKind::GetResultButtonNo),
        constants::GROUP_GET_RESULT => Some(GroupOpKind::GetResult),
        constants::GROUP_ORDER => Some(GroupOpKind::Order),
        constants::GROUP_LAYER => Some(GroupOpKind::Layer),
        constants::GROUP_CANCEL_PRIORITY => Some(GroupOpKind::CancelPriority),
        _ => None,
    }
}

fn resolve_mwnd_list_op(_ids: &constants::RuntimeConstants, op: i32) -> Option<MwndListOpKind> {
    match op {
        constants::MWNDLIST_CLOSE_NOWAIT => Some(MwndListOpKind::CloseAllNowait),
        constants::MWNDLIST_CLOSE_WAIT => Some(MwndListOpKind::CloseAllWait),
        constants::MWNDLIST_CLOSE => Some(MwndListOpKind::CloseAll),
        _ => None,
    }
}

fn resolve_mwnd_op(_ids: &constants::RuntimeConstants, op: i32) -> Option<MwndOpKind> {
    match op {
        constants::MWND_MSG_BLOCK => Some(MwndOpKind::MsgBlock),
        constants::MWND_OPEN_NOWAIT | constants::MWND_OPEN_WAIT | constants::MWND_OPEN => {
            Some(MwndOpKind::Open)
        }
        constants::MWND_CLOSE_NOWAIT
        | constants::MWND_CLOSE_WAIT
        | constants::MWND_CLOSE
        | constants::MWND_END_CLOSE => Some(MwndOpKind::Close),
        constants::MWND_CHECK_OPEN => Some(MwndOpKind::CheckOpen),
        constants::MWND____NOVEL_CLEAR => Some(MwndOpKind::NovelClear),
        constants::MWND_CLEAR => Some(MwndOpKind::Clear),
        constants::MWND_PRINT | constants::MWND____OVER_FLOW_PRINT => Some(MwndOpKind::Print),
        constants::MWND_NL => Some(MwndOpKind::NewLineNoIndent),
        constants::MWND_NLI => Some(MwndOpKind::NewLineIndent),
        constants::MWND_WAIT_MSG => Some(MwndOpKind::WaitMsg),
        constants::MWND_PP => Some(MwndOpKind::Pp),
        constants::MWND_R => Some(MwndOpKind::R),
        constants::MWND_PAGE => Some(MwndOpKind::PageWait),
        constants::MWND_SET_NAMAE | constants::MWND____OVER_FLOW_NAMAE => Some(MwndOpKind::SetName),
        constants::MWND_NEXT_MSG => Some(MwndOpKind::NextMsg),
        constants::MWND_MULTI_MSG => Some(MwndOpKind::MultiMsg),
        constants::MWND_RUBY => Some(MwndOpKind::Ruby),
        constants::MWND_KOE_PLAY_WAIT_KEY => Some(MwndOpKind::KoePlayWaitKey),
        constants::MWND_KOE_PLAY_WAIT => Some(MwndOpKind::KoePlayWait),
        constants::MWND_KOE => Some(MwndOpKind::Koe),
        constants::MWND_LAYER => Some(MwndOpKind::Layer),
        constants::MWND_WORLD => Some(MwndOpKind::World),
        constants::MWND_SIZE => Some(MwndOpKind::SetMojiSize),
        constants::MWND_COLOR => Some(MwndOpKind::SetMojiColor),
        constants::MWND_INDENT => Some(MwndOpKind::SetIndent),
        constants::MWND_CLEAR_INDENT => Some(MwndOpKind::ClearIndent),
        constants::MWND_START_SLIDE_MSG => Some(MwndOpKind::StartSlideMsg),
        constants::MWND_END_SLIDE_MSG => Some(MwndOpKind::EndSlideMsg),
        constants::MWND____SLIDE_MSG => Some(MwndOpKind::SlideMsg),
        constants::MWND_INIT_OPEN_ANIME_TYPE => Some(MwndOpKind::InitOpenAnimeType),
        constants::MWND_INIT_OPEN_ANIME_TIME => Some(MwndOpKind::InitOpenAnimeTime),
        constants::MWND_INIT_CLOSE_ANIME_TYPE => Some(MwndOpKind::InitCloseAnimeType),
        constants::MWND_INIT_CLOSE_ANIME_TIME => Some(MwndOpKind::InitCloseAnimeTime),
        constants::MWND_SET_OPEN_ANIME_TYPE => Some(MwndOpKind::SetOpenAnimeType),
        constants::MWND_SET_OPEN_ANIME_TIME => Some(MwndOpKind::SetOpenAnimeTime),
        constants::MWND_SET_CLOSE_ANIME_TYPE => Some(MwndOpKind::SetCloseAnimeType),
        constants::MWND_SET_CLOSE_ANIME_TIME => Some(MwndOpKind::SetCloseAnimeTime),
        constants::MWND_GET_OPEN_ANIME_TYPE => Some(MwndOpKind::GetOpenAnimeType),
        constants::MWND_GET_OPEN_ANIME_TIME => Some(MwndOpKind::GetOpenAnimeTime),
        constants::MWND_GET_CLOSE_ANIME_TYPE => Some(MwndOpKind::GetCloseAnimeType),
        constants::MWND_GET_CLOSE_ANIME_TIME => Some(MwndOpKind::GetCloseAnimeTime),
        constants::MWND_GET_DEFAULT_OPEN_ANIME_TYPE => Some(MwndOpKind::GetDefaultOpenAnimeType),
        constants::MWND_GET_DEFAULT_OPEN_ANIME_TIME => Some(MwndOpKind::GetDefaultOpenAnimeTime),
        constants::MWND_GET_DEFAULT_CLOSE_ANIME_TYPE => Some(MwndOpKind::GetDefaultCloseAnimeType),
        constants::MWND_GET_DEFAULT_CLOSE_ANIME_TIME => Some(MwndOpKind::GetDefaultCloseAnimeTime),
        constants::MWND_SELMSG_CANCEL => Some(MwndOpKind::SelMsgCancel),
        constants::MWND_SELMSG => Some(MwndOpKind::SelMsg),
        constants::MWND_SEL_CANCEL => Some(MwndOpKind::SelCancel),
        constants::MWND_SEL => Some(MwndOpKind::Sel),
        constants::MWND_INIT_WAKU_FILE => Some(MwndOpKind::InitWakuFile),
        constants::MWND_SET_WAKU_FILE => Some(MwndOpKind::SetWakuFile),
        constants::MWND_GET_WAKU_FILE => Some(MwndOpKind::GetWakuFile),
        constants::MWND_INIT_FILTER_FILE => Some(MwndOpKind::InitFilterFile),
        constants::MWND_SET_FILTER_FILE => Some(MwndOpKind::SetFilterFile),
        constants::MWND_GET_FILTER_FILE => Some(MwndOpKind::GetFilterFile),
        constants::MWND_CLEAR_FACE => Some(MwndOpKind::ClearFace),
        constants::MWND_SET_FACE => Some(MwndOpKind::SetFace),
        constants::MWND_REP_POS => Some(MwndOpKind::SetRepPos),
        constants::MWND_MSGBTN => Some(MwndOpKind::MsgBtn),
        constants::MWND_INIT_WINDOW_POS => Some(MwndOpKind::InitWindowPos),
        constants::MWND_INIT_WINDOW_SIZE => Some(MwndOpKind::InitWindowSize),
        constants::MWND_SET_WINDOW_POS => Some(MwndOpKind::SetWindowPos),
        constants::MWND_SET_WINDOW_SIZE => Some(MwndOpKind::SetWindowSize),
        constants::MWND_GET_WINDOW_POS_X => Some(MwndOpKind::GetWindowPosX),
        constants::MWND_GET_WINDOW_POS_Y => Some(MwndOpKind::GetWindowPosY),
        constants::MWND_GET_WINDOW_SIZE_X => Some(MwndOpKind::GetWindowSizeX),
        constants::MWND_GET_WINDOW_SIZE_Y => Some(MwndOpKind::GetWindowSizeY),
        constants::MWND_INIT_WINDOW_MOJI_CNT => Some(MwndOpKind::InitWindowMojiCnt),
        constants::MWND_SET_WINDOW_MOJI_CNT => Some(MwndOpKind::SetWindowMojiCnt),
        constants::MWND_GET_WINDOW_MOJI_CNT_X => Some(MwndOpKind::GetWindowMojiCntX),
        constants::MWND_GET_WINDOW_MOJI_CNT_Y => Some(MwndOpKind::GetWindowMojiCntY),
        _ => None,
    }
}

fn resolve_group_list_op_kind(ids: &constants::RuntimeConstants, op: i32) -> GroupListOpKind {
    resolve_group_list_op(ids, op).unwrap_or(GroupListOpKind::Unknown)
}

fn resolve_group_op_kind(ids: &constants::RuntimeConstants, op: i32) -> GroupOpKind {
    resolve_group_op(ids, op).unwrap_or(GroupOpKind::Unknown)
}

fn dispatch_group_list_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    op: i32,
    script_args: &[Value],
    ret_form: Option<i64>,
) -> bool {
    let k = resolve_group_list_op_kind(&ctx.ids, op);
    match k {
        GroupListOpKind::Alloc => {
            let cnt = script_args.iter().find_map(as_i64).unwrap_or(0).max(0) as usize;
            st.clear_group_list(stage_idx);
            st.ensure_group_list(stage_idx, cnt);
            if let Some(rf) = ret_form {
                if rf != 0 {
                    ctx.stack.push(default_for_ret_form(rf));
                }
            }
            true
        }
        GroupListOpKind::Free => {
            st.clear_group_list(stage_idx);
            if let Some(rf) = ret_form {
                if rf != 0 {
                    ctx.stack.push(default_for_ret_form(rf));
                }
            }
            true
        }
        GroupListOpKind::Unknown => false,
    }
}

fn dispatch_group_item_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    group_idx: usize,
    op: i32,
    script_args: &[Value],
    rhs: Option<&Value>,
    al_id: Option<i64>,
    ret_form: Option<i64>,
) -> bool {
    ensure_group(ctx, st, stage_idx, group_idx);
    let k = resolve_group_op_kind(&ctx.ids, op);

    let list = st.group_lists.get_mut(&stage_idx).unwrap();
    let g = &mut list[group_idx];

    match k {
        GroupOpKind::Order => {
            if let Some(v) = rhs.and_then(as_i64) {
                g.order = v;
                ctx.stack.push(Value::Int(0));
            } else {
                ctx.stack.push(Value::Int(g.order));
            }
            true
        }
        GroupOpKind::Layer => {
            if let Some(v) = rhs.and_then(as_i64) {
                g.layer = v;
                ctx.stack.push(Value::Int(0));
            } else {
                ctx.stack.push(Value::Int(g.layer));
            }
            true
        }
        GroupOpKind::CancelPriority => {
            if let Some(v) = rhs.and_then(as_i64) {
                g.cancel_priority = v;
                ctx.stack.push(Value::Int(0));
            } else {
                ctx.stack.push(Value::Int(g.cancel_priority));
            }
            true
        }
        GroupOpKind::Init => {
            g.init_sel();
            g.wait_flag = false;
            g.started = false;
            if let Some(rf) = ret_form {
                if rf != 0 {
                    ctx.stack.push(default_for_ret_form(rf));
                }
            } else {
                ctx.stack.push(Value::Int(0));
            }
            true
        }
        GroupOpKind::Start | GroupOpKind::StartCancel => {
            g.init_sel();
            g.cancel_flag = (k == GroupOpKind::StartCancel);
            g.start();
            if let Some(rf) = ret_form {
                if rf != 0 {
                    ctx.stack.push(default_for_ret_form(rf));
                }
            } else {
                ctx.stack.push(Value::Int(0));
            }
            true
        }
        GroupOpKind::Sel | GroupOpKind::SelCancel => {
            // Mirror group_sel(_cancel) behavior:
            // - reset selection state
            // - start selection
            // - set wait flag and focus so Enter/Esc can drive it
            g.init_sel();
            g.cancel_flag = (k == GroupOpKind::SelCancel);
            if k == GroupOpKind::SelCancel {
                g.cancel_se_no = script_args.iter().find_map(as_i64).unwrap_or(-1);
            }
            g.wait_flag = true;
            g.start();

            // Focus this group for runtime key mapping (see runtime::CommandContext::on_key_down).
            ctx.globals.focused_stage_group =
                Some((ctx.ids.form_global_stage, stage_idx, group_idx));

            // Block VM until a key is pressed.
            ctx.wait.wait_key();

            if let Some(rf) = ret_form {
                if rf != 0 {
                    ctx.stack.push(default_for_ret_form(rf));
                }
            } else {
                ctx.stack.push(Value::Int(0));
            }
            true
        }
        GroupOpKind::End => {
            g.end();
            if ctx.globals.focused_stage_group
                == Some((ctx.ids.form_global_stage, stage_idx, group_idx))
            {
                ctx.globals.focused_stage_group = None;
            }
            if let Some(rf) = ret_form {
                if rf != 0 {
                    ctx.stack.push(default_for_ret_form(rf));
                }
            } else {
                ctx.stack.push(Value::Int(0));
            }
            true
        }
        GroupOpKind::GetHitNo => {
            ctx.stack.push(Value::Int(g.hit_button_no));
            true
        }
        GroupOpKind::GetPushedNo => {
            ctx.stack.push(Value::Int(g.pushed_button_no));
            true
        }
        GroupOpKind::GetDecidedNo => {
            ctx.stack.push(Value::Int(g.decided_button_no));
            true
        }
        GroupOpKind::GetResult => {
            ctx.stack.push(Value::Int(g.result));
            true
        }
        GroupOpKind::GetResultButtonNo => {
            ctx.stack.push(Value::Int(g.result_button_no));
            true
        }
        GroupOpKind::Unknown => {
            if let Some(s) = rhs.and_then(as_str) {
                g.aux_str_props.insert(op, s.to_string());
                ctx.stack.push(Value::Int(0));
                true
            } else if let Some(v) = rhs.and_then(as_i64) {
                g.props.insert(op, v);
                ctx.stack.push(Value::Int(0));
                true
            } else if rhs.is_none() && script_args.len() == 1 {
                if let Some(v) = script_args[0].as_i64() {
                    if matches!(al_id, Some(1)) {
                        g.props.insert(op, v);
                        ctx.stack.push(Value::Int(0));
                        return true;
                    }
                }
                if let Some(s) = script_args[0].as_str() {
                    if matches!(al_id, Some(1)) {
                        g.aux_str_props.insert(op, s.to_string());
                        ctx.stack.push(Value::Int(0));
                        return true;
                    }
                }
                if let Some(rf) = ret_form {
                    if rf == 2 {
                        ctx.stack.push(Value::Str(
                            g.aux_str_props.get(&op).cloned().unwrap_or_default(),
                        ));
                        return true;
                    }
                    if rf != 0 {
                        ctx.stack.push(Value::Int(*g.props.get(&op).unwrap_or(&0)));
                        return true;
                    }
                }
                false
            } else if let Some(rf) = ret_form {
                if rf == 2 {
                    ctx.stack.push(Value::Str(
                        g.aux_str_props.get(&op).cloned().unwrap_or_default(),
                    ));
                    true
                } else if rf != 0 {
                    ctx.stack.push(Value::Int(*g.props.get(&op).unwrap_or(&0)));
                    true
                } else {
                    false
                }
            } else {
                false
            }
        }
    }
}

fn resolve_mwnd_list_op_kind(ids: &constants::RuntimeConstants, op: i32) -> MwndListOpKind {
    resolve_mwnd_list_op(ids, op).unwrap_or(MwndListOpKind::Unknown)
}

fn resolve_mwnd_op_kind(ids: &constants::RuntimeConstants, op: i32) -> MwndOpKind {
    resolve_mwnd_op(ids, op).unwrap_or(MwndOpKind::Unknown)
}

fn dispatch_mwnd_list_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    op: i32,
    script_args: &[Value],
    ret_form: Option<i64>,
) -> bool {
    let k = resolve_mwnd_list_op_kind(&ctx.ids, op);
    match k {
        MwndListOpKind::CloseAll
        | MwndListOpKind::CloseAllWait
        | MwndListOpKind::CloseAllNowait => {
            let close_anim_time = st
                .mwnd_lists
                .get(&stage_idx)
                .map(|list| list.iter().map(|m| m.close_anime_time).max().unwrap_or(0))
                .unwrap_or(0);
            st.close_all_mwnd(stage_idx);
            if matches!(ctx.globals.focused_stage_mwnd, Some((form_id, sidx, _))
                if form_id == ctx.ids.form_global_stage && sidx == stage_idx)
            {
                ctx.globals.focused_stage_mwnd = None;
            }
            msgbk_next(ctx);
            let anim_time = match k {
                MwndListOpKind::CloseAllNowait => 0,
                _ => close_anim_time,
            };
            ctx.ui.begin_mwnd_close(0, anim_time);
            if matches!(k, MwndListOpKind::CloseAll | MwndListOpKind::CloseAllWait) && anim_time > 0
            {
                ctx.wait.wait_ms(anim_time.max(0) as u64);
            }
            if let Some(rf) = ret_form {
                if rf != 0 {
                    ctx.stack.push(default_for_ret_form(rf));
                }
            }
            true
        }
        MwndListOpKind::Unknown => false,
    }
}

fn dispatch_mwnd_item_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    mwnd_idx: usize,
    op: i32,
    tail: &[i32],
    script_args: &[Value],
    rhs: Option<&Value>,
    al_id: Option<i64>,
    ret_form: Option<i64>,
) -> bool {
    ensure_mwnd(ctx, st, stage_idx, mwnd_idx);
    let _m_snapshot = {
        let list = st.mwnd_lists.get_mut(&stage_idx).unwrap();
        list[mwnd_idx].clone()
    };

    if matches!(
        op,
        constants::MWND_BUTTON | constants::MWND_FACE | constants::MWND_OBJECT
    ) {
        let selector_key = if op == constants::MWND_BUTTON {
            "button"
        } else if op == constants::MWND_FACE {
            "face"
        } else {
            "object"
        };
        let (mut child_list, mut strict) = {
            let list = st.mwnd_lists.get_mut(&stage_idx).unwrap();
            let m = &mut list[mwnd_idx];
            match selector_key {
                "button" => (std::mem::take(&mut m.button_list), m.button_list_strict),
                "face" => (std::mem::take(&mut m.face_list), m.face_list_strict),
                _ => (std::mem::take(&mut m.object_list), m.object_list_strict),
            }
        };
        let handled = if tail.is_empty() {
            push_ok(ctx, ret_form);
            true
        } else if tail.len() == 1 {
            dispatch_embedded_object_list_op(
                ctx,
                &mut child_list,
                &mut strict,
                tail[0],
                script_args,
                ret_form,
            )
            .unwrap_or(false)
        } else {
            let (child_idx, child_op, child_tail) = if tail[0] == -1 && tail.len() >= 3 {
                (tail[1] as i64, tail[2], &tail[3..])
            } else if tail.len() >= 2 {
                (tail[0] as i64, tail[1], &tail[2..])
            } else {
                (0, 0, &tail[0..0])
            };
            if child_tail.is_empty() && tail.len() < 2 {
                false
            } else {
                dispatch_embedded_object_item_op(
                    ctx,
                    st,
                    stage_idx,
                    &mut child_list,
                    strict,
                    child_idx,
                    child_op,
                    child_tail,
                    script_args,
                    ret_form,
                    rhs,
                    al_id,
                    &format!("mwnd_{selector_key}_{stage_idx}_{mwnd_idx}"),
                )
            }
        };
        {
            let list = st.mwnd_lists.get_mut(&stage_idx).unwrap();
            let m = &mut list[mwnd_idx];
            match selector_key {
                "button" => {
                    m.button_list = child_list;
                    m.button_list_strict = strict;
                }
                "face" => {
                    m.face_list = child_list;
                    m.face_list_strict = strict;
                }
                _ => {
                    m.object_list = child_list;
                    m.object_list_strict = strict;
                }
            }
        }
        if handled {
            return true;
        }
    }

    let k = resolve_mwnd_op_kind(&ctx.ids, op);

    let list = st.mwnd_lists.get_mut(&stage_idx).unwrap();
    let m = &mut list[mwnd_idx];

    match k {
        MwndOpKind::MsgBlock => {
            if !m.msg_text.is_empty() || !m.name_text.is_empty() || m.text_dirty {
                msgbk_next(ctx);
            }
            m.text_dirty = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::Open => {
            m.open = true;
            m.text_dirty = false;
            ctx.ui.begin_mwnd_open(m.open_anime_type, m.open_anime_time);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::Close => {
            m.open = false;
            m.multi_msg = false;
            m.text_dirty = false;
            m.selection = None;
            if ctx.globals.focused_stage_mwnd
                == Some((ctx.ids.form_global_stage, stage_idx, mwnd_idx))
            {
                ctx.globals.focused_stage_mwnd = None;
            }
            msgbk_next(ctx);
            ctx.ui
                .begin_mwnd_close(m.close_anime_type, m.close_anime_time);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::CheckOpen => {
            ctx.stack.push(Value::Int(if m.open { 1 } else { 0 }));
            true
        }
        MwndOpKind::Clear => {
            m.msg_text.clear();
            m.multi_msg = false;
            m.text_dirty = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::NovelClear => {
            m.msg_text.clear();
            m.msg_text.push('\n');
            m.multi_msg = false;
            m.text_dirty = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::NewLineNoIndent => {
            m.msg_text.push('\n');
            m.indent = false;
            m.text_dirty = true;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::NewLineIndent => {
            m.msg_text.push('\n');
            m.text_dirty = true;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::Print => {
            let msg = rhs
                .and_then(|v| v.as_str())
                .or_else(|| script_args.iter().find_map(|v| v.as_str()))
                .unwrap_or("");
            if !msg.is_empty() {
                m.msg_text.push_str(msg);
                msgbk_add_text(ctx, msg);
                m.text_dirty = true;
            }
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::AddMsg => {
            let msg = rhs
                .and_then(|v| v.as_str())
                .or_else(|| script_args.iter().find_map(|v| v.as_str()))
                .unwrap_or("");
            if !msg.is_empty() {
                m.msg_text.push_str(msg);
                msgbk_add_text(ctx, msg);
                m.text_dirty = true;
            }
            if prop_access::ret_form_is_string_opt(ret_form) {
                ctx.stack.push(Value::Str(String::new()));
            } else {
                push_ok(ctx, ret_form);
            }
            true
        }
        MwndOpKind::AddMsgCheck => {
            ctx.stack.push(Value::Int(1));
            true
        }
        MwndOpKind::WaitMsg => {
            ctx.ui.begin_wait_message();
            ctx.wait.wait_key();
            m.text_dirty = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::Pp => {
            ctx.ui.begin_wait_message();
            ctx.wait.wait_key();
            m.text_dirty = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::R => {
            msgbk_next(ctx);
            ctx.ui.begin_wait_message();
            ctx.ui.request_clear_message_on_wait_end();
            ctx.wait.wait_key();
            m.text_dirty = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::PageWait => {
            // Treat a page wait as a message boundary for backlog purposes.
            msgbk_next(ctx);
            ctx.ui.begin_wait_message();
            ctx.ui.request_clear_message_on_wait_end();
            ctx.wait.wait_key();
            m.text_dirty = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetName => {
            let s = rhs
                .and_then(|v| v.as_str())
                .or_else(|| script_args.iter().find_map(|v| v.as_str()))
                .unwrap_or("");
            m.name_text = s.to_string();
            if !s.is_empty() {
                msgbk_add_name(ctx, s);
            }
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::NextMsg => {
            msgbk_next(ctx);
            m.text_dirty = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::MultiMsg => {
            m.multi_msg = true;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::Sel | MwndOpKind::SelCancel | MwndOpKind::SelMsg | MwndOpKind::SelMsgCancel => {
            let choices = parse_mwnd_selection_args(script_args, rhs);
            let cancel_enable = matches!(k, MwndOpKind::SelCancel | MwndOpKind::SelMsgCancel);
            let close_mwnd = matches!(k, MwndOpKind::Sel | MwndOpKind::SelCancel);

            m.open = true;
            ctx.ui.begin_mwnd_open(m.open_anime_type, m.open_anime_time);

            m.selection = Some(MwndSelectionState {
                choices,
                cursor: 0,
                cancel_enable,
                close_mwnd,
                result: 0,
            });
            ctx.globals.focused_stage_mwnd = Some((ctx.ids.form_global_stage, stage_idx, mwnd_idx));
            ctx.wait.wait_key();
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::Ruby => {
            let s = rhs
                .and_then(|v| v.as_str())
                .or_else(|| script_args.iter().find_map(|v| v.as_str()))
                .map(str::to_string);
            m.ruby_text = s;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::Koe | MwndOpKind::KoePlayWait | MwndOpKind::KoePlayWaitKey => {
            let koe_no = script_args.first().and_then(Value::as_i64).unwrap_or(0);
            let chara_no = script_args.get(1).and_then(Value::as_i64).unwrap_or(-1);
            m.koe = Some((koe_no, chara_no));
            let _ = {
                let (se, audio) = (&mut ctx.se, &mut ctx.audio);
                se.play_koe_no(audio, koe_no)
            };
            msgbk_add_koe(ctx, koe_no, chara_no);
            match k {
                MwndOpKind::KoePlayWait => {
                    ctx.wait
                        .wait_audio(crate::runtime::wait::AudioWait::SeAny, false);
                }
                MwndOpKind::KoePlayWaitKey => {
                    ctx.wait
                        .wait_audio(crate::runtime::wait::AudioWait::SeAny, true);
                }
                _ => {}
            }
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::Layer => {
            if let Some(v) = script_args.first().and_then(Value::as_i64) {
                m.layer = v;
                push_ok(ctx, ret_form);
            } else {
                ctx.stack.push(Value::Int(m.layer));
            }
            true
        }
        MwndOpKind::World => {
            if let Some(v) = script_args.first().and_then(Value::as_i64) {
                m.world = v;
                push_ok(ctx, ret_form);
            } else {
                ctx.stack.push(Value::Int(m.world));
            }
            true
        }
        MwndOpKind::SetMojiSize => {
            m.moji_size = script_args.first().and_then(Value::as_i64);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetMojiColor => {
            m.moji_color = script_args.first().and_then(Value::as_i64);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetIndent => {
            m.indent = true;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::ClearIndent => {
            m.indent = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::StartSlideMsg => {
            m.slide_msg = true;
            m.slide_time = script_args.first().and_then(Value::as_i64).unwrap_or(0);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::EndSlideMsg => {
            m.slide_msg = false;
            m.slide_time = 0;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SlideMsg => {
            m.slide_msg = true;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::InitOpenAnimeType => {
            m.open_anime_type = 0;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::InitOpenAnimeTime => {
            m.open_anime_time = 0;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::InitCloseAnimeType => {
            m.close_anime_type = 0;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::InitCloseAnimeTime => {
            m.close_anime_time = 0;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetOpenAnimeType => {
            m.open_anime_type = script_args.first().and_then(Value::as_i64).unwrap_or(0);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetOpenAnimeTime => {
            m.open_anime_time = script_args.first().and_then(Value::as_i64).unwrap_or(0);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetCloseAnimeType => {
            m.close_anime_type = script_args.first().and_then(Value::as_i64).unwrap_or(0);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetCloseAnimeTime => {
            m.close_anime_time = script_args.first().and_then(Value::as_i64).unwrap_or(0);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::GetOpenAnimeType => {
            ctx.stack.push(Value::Int(m.open_anime_type));
            true
        }
        MwndOpKind::GetOpenAnimeTime => {
            ctx.stack.push(Value::Int(m.open_anime_time));
            true
        }
        MwndOpKind::GetCloseAnimeType => {
            ctx.stack.push(Value::Int(m.close_anime_type));
            true
        }
        MwndOpKind::GetCloseAnimeTime => {
            ctx.stack.push(Value::Int(m.close_anime_time));
            true
        }
        MwndOpKind::GetDefaultOpenAnimeType => {
            ctx.stack.push(Value::Int(0));
            true
        }
        MwndOpKind::GetDefaultOpenAnimeTime => {
            ctx.stack.push(Value::Int(0));
            true
        }
        MwndOpKind::GetDefaultCloseAnimeType => {
            ctx.stack.push(Value::Int(0));
            true
        }
        MwndOpKind::GetDefaultCloseAnimeTime => {
            ctx.stack.push(Value::Int(0));
            true
        }
        MwndOpKind::ClearName => {
            m.name_text.clear();
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::GetName => {
            ctx.stack.push(Value::Str(m.name_text.clone()));
            true
        }
        MwndOpKind::InitWakuFile => {
            m.waku_file.clear();

            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetWakuFile => {
            let s = rhs
                .and_then(|v| v.as_str())
                .or_else(|| script_args.iter().find_map(|v| v.as_str()))
                .unwrap_or("");
            m.waku_file = s.to_string();
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::GetWakuFile => {
            ctx.stack.push(Value::Str(m.waku_file.clone()));
            true
        }
        MwndOpKind::InitFilterFile => {
            m.filter_file.clear();

            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetFilterFile => {
            let s = rhs
                .and_then(|v| v.as_str())
                .or_else(|| script_args.iter().find_map(|v| v.as_str()))
                .unwrap_or("");
            m.filter_file = s.to_string();

            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::GetFilterFile => {
            ctx.stack.push(Value::Str(m.filter_file.clone()));
            true
        }
        MwndOpKind::ClearFace => {
            m.face_file.clear();
            m.face_no = 0;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetFace => {
            let face_file = rhs
                .and_then(|v| v.as_str())
                .or_else(|| script_args.iter().find_map(|v| v.as_str()))
                .unwrap_or("")
                .to_string();
            let mut ints = script_args.iter().filter_map(Value::as_i64);
            let face_no = ints.next().unwrap_or(0);
            m.face_no = face_no;
            m.face_file = face_file.clone();
            if !face_file.is_empty() {
                m.aux_str_props.insert(op, face_file);
            }
            m.props.insert(op, face_no);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetRepPos => {
            if script_args.is_empty() {
                m.rep_pos = None;
            } else {
                let x = script_args.first().and_then(Value::as_i64).unwrap_or(0);
                let y = script_args.get(1).and_then(Value::as_i64).unwrap_or(0);
                m.rep_pos = Some((x, y));
            }
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::MsgBtn => {
            if script_args.is_empty() {
                m.msgbtn = None;
            } else {
                let a = script_args.first().and_then(Value::as_i64).unwrap_or(0);
                let b = script_args.get(1).and_then(Value::as_i64).unwrap_or(0);
                let c = script_args.get(2).and_then(Value::as_i64).unwrap_or(0);
                let d = script_args.get(3).and_then(Value::as_i64).unwrap_or(0);
                m.msgbtn = Some((a, b, c, d));
            }
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::InitWindowPos => {
            m.window_pos = None;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::InitWindowSize => {
            m.window_size = None;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetWindowPos => {
            let x = script_args.first().and_then(Value::as_i64).unwrap_or(0);
            let y = script_args.get(1).and_then(Value::as_i64).unwrap_or(0);
            m.window_pos = Some((x, y));
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetWindowSize => {
            let x = script_args.first().and_then(Value::as_i64).unwrap_or(0);
            let y = script_args.get(1).and_then(Value::as_i64).unwrap_or(0);
            m.window_size = Some((x, y));
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::GetWindowPosX => {
            ctx.stack
                .push(Value::Int(m.window_pos.map(|v| v.0).unwrap_or(0)));
            true
        }
        MwndOpKind::GetWindowPosY => {
            ctx.stack
                .push(Value::Int(m.window_pos.map(|v| v.1).unwrap_or(0)));
            true
        }
        MwndOpKind::GetWindowSizeX => {
            ctx.stack
                .push(Value::Int(m.window_size.map(|v| v.0).unwrap_or(0)));
            true
        }
        MwndOpKind::GetWindowSizeY => {
            ctx.stack
                .push(Value::Int(m.window_size.map(|v| v.1).unwrap_or(0)));
            true
        }
        MwndOpKind::InitWindowMojiCnt => {
            m.window_moji_cnt = None;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetWindowMojiCnt => {
            let x = script_args.first().and_then(Value::as_i64).unwrap_or(0);
            let y = script_args.get(1).and_then(Value::as_i64).unwrap_or(0);
            m.window_moji_cnt = Some((x, y));
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::GetWindowMojiCntX => {
            ctx.stack
                .push(Value::Int(m.window_moji_cnt.map(|v| v.0).unwrap_or(0)));
            true
        }
        MwndOpKind::GetWindowMojiCntY => {
            ctx.stack
                .push(Value::Int(m.window_moji_cnt.map(|v| v.1).unwrap_or(0)));
            true
        }
        MwndOpKind::Unknown => false,
    }
}

fn dispatch_btnselitem_list_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    op: i32,
    script_args: &[Value],
    ret_form: Option<i64>,
) -> bool {
    let list = st.btnselitem_lists.entry(stage_idx).or_default();
    if op == crate::runtime::forms::codes::OBJECTLIST_GET_SIZE {
        ctx.stack.push(Value::Int(list.len() as i64));
        return true;
    }
    if op == crate::runtime::forms::codes::OBJECTLIST_RESIZE {
        let n = script_args.first().and_then(as_i64).unwrap_or(0).max(0) as usize;
        if list.len() <= n {
            list.resize_with(n, BtnSelItemState::default);
        } else {
            list.truncate(n);
        }
        push_ok(ctx, ret_form);
        return true;
    }
    false
}

fn dispatch_btnselitem_item_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    item_idx: usize,
    op: i32,
    tail: &[i32],
    script_args: &[Value],
    rhs: Option<&Value>,
    al_id: Option<i64>,
    ret_form: Option<i64>,
) -> bool {
    ensure_btnselitem(ctx, st, stage_idx, item_idx);
    let (mut child_list, mut strict) = {
        let list = st.btnselitem_lists.get_mut(&stage_idx).unwrap();
        let item = &mut list[item_idx];
        (std::mem::take(&mut item.object_list), item.strict)
    };
    let handled = if tail.is_empty() {
        push_ok(ctx, ret_form);
        true
    } else if tail.len() == 1 {
        dispatch_embedded_object_list_op(
            ctx,
            &mut child_list,
            &mut strict,
            tail[0],
            script_args,
            ret_form,
        )
        .unwrap_or(false)
    } else {
        let (child_idx, child_op, child_tail) = if tail[0] == -1 && tail.len() >= 3 {
            (tail[1] as i64, tail[2], &tail[3..])
        } else if tail.len() >= 2 {
            (tail[0] as i64, tail[1], &tail[2..])
        } else {
            (0, 0, &tail[0..0])
        };
        if child_tail.is_empty() && tail.len() < 2 {
            false
        } else {
            dispatch_embedded_object_item_op(
                ctx,
                st,
                stage_idx,
                &mut child_list,
                strict,
                child_idx,
                child_op,
                child_tail,
                script_args,
                ret_form,
                rhs,
                al_id,
                &format!("btnselitem_{}_{}_{}", stage_idx, item_idx, op),
            )
        }
    };
    {
        let list = st.btnselitem_lists.get_mut(&stage_idx).unwrap();
        let item = &mut list[item_idx];
        item.object_list = child_list;
        item.strict = strict;
    }
    handled
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let Some((chain_pos, chain)) =
        crate::runtime::forms::prop_access::parse_current_element_chain(ctx, args)
    else {
        return Ok(false);
    };

    let (mut al_id, mut ret_form) = crate::runtime::forms::prop_access::current_vm_meta(ctx);

    let rhs: Option<&Value> = if al_id == Some(1) {
        if chain_pos == args.len() {
            args.get(1)
        } else if chain_pos >= 3 && as_i64(&args[1]).is_some() {
            args.get(2)
        } else {
            args.first()
        }
    } else {
        None
    };

    let Some(tgt) = parse_target(ctx, &chain) else {
        if sg_debug_enabled_local() {
            sg_debug_stage(format!("parse_target miss chain={:?}", chain));
        }
        return Ok(false);
    };
    if sg_debug_enabled_local() {
        sg_debug_stage(format!("chain={:?} target={:?}", chain, tgt));
    }

    // Command arguments are the original script arguments preceding the element chain.
    let script_args = crate::runtime::forms::prop_access::script_args(args, chain_pos);

    match tgt {
        StageTarget::StageCount => {
            // Stage count: expose 3 logical stages (BG/CHR/FX).
            ctx.stack.push(Value::Int(3));
            return Ok(true);
        }
        StageTarget::StageOp { stage, op } => {
            let form_id = ctx.ids.form_global_stage;
            with_stage_state(ctx, form_id, |ctx, st| match op as i32 {
                0 => {
                    let n = script_args.first().and_then(as_i64).unwrap_or(0).max(0) as usize;
                    sg_debug_stage(format!("stage={} CREATE_OBJECT resize {}", stage, n));
                    let old_len = st.object_list_len(stage);
                    if n < old_len {
                        if let Some(list) = st.object_lists.get_mut(&stage) {
                            for i in n..old_len {
                                let obj = &mut list[i];
                                object_clear_backend(ctx, obj, stage, i);
                                *obj = ObjectState::default();
                            }
                        }
                    }
                    st.set_object_list_len_strict(stage, n);
                    ctx.stack.push(Value::Int(0));
                }
                1 => {
                    let n = script_args.first().and_then(as_i64).unwrap_or(0).max(0) as usize;
                    sg_debug_stage(format!("stage={} CREATE_MWND resize {}", stage, n));
                    st.ensure_mwnd_list(stage, n);
                    ctx.stack.push(Value::Int(0));
                }
                _ => {
                    if let Some(rf) = ret_form {
                        if rf != 0 {
                            ctx.stack.push(default_for_ret_form(rf));
                        } else {
                            ctx.stack.push(Value::Int(0));
                        }
                    } else {
                        ctx.stack.push(Value::Int(0));
                    }
                }
            });
            return Ok(true);
        }
        StageTarget::ChildListOp { stage, child, op } => {
            let form_id = ctx.ids.form_global_stage;
            let stage_elm_object = ctx.ids.stage_elm_object;
            let stage_elm_world = ctx.ids.stage_elm_world;

            let handled = with_stage_state(ctx, form_id, |ctx, st| {
                let stage_object = if stage_elm_object != 0 {
                    stage_elm_object
                } else {
                    crate::runtime::forms::codes::STAGE_ELM_OBJECT
                };
                let stage_world = if stage_elm_world != 0 {
                    stage_elm_world
                } else {
                    crate::runtime::forms::codes::STAGE_ELM_WORLD
                };

                if child == stage_object {
                    dispatch_object_list_op(ctx, st, stage, op as i32, script_args, ret_form)
                } else if child == crate::runtime::forms::codes::STAGE_ELM_MWND {
                    dispatch_mwnd_list_op(ctx, st, stage, op as i32, script_args, ret_form)
                } else if child == crate::runtime::forms::codes::STAGE_ELM_OBJBTNGROUP {
                    dispatch_group_list_op(ctx, st, stage, op as i32, script_args, ret_form)
                } else if child == crate::runtime::forms::codes::STAGE_ELM_BTNSELITEM {
                    dispatch_btnselitem_list_op(ctx, st, stage, op as i32, script_args, ret_form)
                } else if child == stage_world {
                    dispatch_world_list_op(ctx, st, stage, op as i32, script_args, ret_form)
                } else {
                    false
                }
            });

            return Ok(handled);
        }
        StageTarget::ChildItemOp {
            stage,
            child,
            idx,
            op,
            tail,
        } => {
            let form_id = ctx.ids.form_global_stage;
            let stage_elm_object = ctx.ids.stage_elm_object;
            let stage_elm_world = ctx.ids.stage_elm_world;

            let handled = with_stage_state(ctx, form_id, |ctx, st| {
                let stage_object = if stage_elm_object != 0 {
                    stage_elm_object
                } else {
                    crate::runtime::forms::codes::STAGE_ELM_OBJECT
                };
                let stage_world = if stage_elm_world != 0 {
                    stage_elm_world
                } else {
                    crate::runtime::forms::codes::STAGE_ELM_WORLD
                };

                if child == stage_object {
                    dispatch_object_op(
                        ctx,
                        st,
                        stage,
                        idx,
                        op as i32,
                        &tail,
                        script_args,
                        ret_form,
                        rhs,
                        al_id,
                    )
                } else if child == crate::runtime::forms::codes::STAGE_ELM_OBJBTNGROUP {
                    dispatch_group_item_op(
                        ctx,
                        st,
                        stage,
                        idx.max(0) as usize,
                        op as i32,
                        script_args,
                        rhs,
                        al_id,
                        ret_form,
                    )
                } else if child == crate::runtime::forms::codes::STAGE_ELM_BTNSELITEM {
                    dispatch_btnselitem_item_op(
                        ctx,
                        st,
                        stage,
                        idx.max(0) as usize,
                        op as i32,
                        &tail,
                        script_args,
                        rhs,
                        al_id,
                        ret_form,
                    )
                } else if child == crate::runtime::forms::codes::STAGE_ELM_MWND {
                    dispatch_mwnd_item_op(
                        ctx,
                        st,
                        stage,
                        idx.max(0) as usize,
                        op as i32,
                        &tail,
                        script_args,
                        rhs,
                        al_id,
                        ret_form,
                    )
                } else if child == stage_world {
                    dispatch_world_item_op(
                        ctx,
                        st,
                        stage,
                        idx.max(0) as usize,
                        op as i32,
                        &tail,
                        script_args,
                        rhs,
                        al_id,
                        ret_form,
                    )
                } else {
                    false
                }
            });

            Ok(handled)
        }
        StageTarget::ChildItemRef { stage, child, idx } => {
            let element_chain = chain.to_vec();
            if child == crate::runtime::forms::codes::STAGE_ELM_OBJECT && idx >= 0 {
                ctx.globals.current_stage_object = Some((stage, idx as usize));
            }
            if al_id == Some(1) {
                ctx.stack.push(Value::Int(0));
            } else {
                ctx.stack.push(Value::Element(element_chain));
            }
            Ok(true)
        }
    }
}
