//! Global Stage form handler aligned to the original C++ Stage/Object/MWND/Group/BTNSELITEM split.
//!
//! This module uses explicit selector and operation dispatch only.

use anyhow::Result;

use std::path::{Path, PathBuf};

use crate::image_manager::ImageId;
use crate::layer::{LayerId, SpriteFit, SpriteId, SpriteSizeMode};
use crate::mesh3d::load_mesh_asset;
use crate::runtime::constants;
use crate::runtime::globals::{
    BtnSelItemState, GroupListOpKind, GroupOpKind, GroupState, MsgBackState, MwndListOpKind, MwndOpKind,
    MwndSelectionChoice, MwndSelectionState, MwndState, ObjectBackend, ObjectEventTarget,
    ObjectFrameActionState, ObjectListOpKind, ObjectOpKind, ObjectState, ObjectWeatherParam,
    PendingFrameActionFinish, ScreenEffectState, ScreenQuakeState, StageFormState, WorldState,
    OBJECT_NESTED_SLOT_KEY,
};
use crate::runtime::int_event::IntEvent;
use crate::runtime::Value;

use super::super::CommandContext;
use super::codes::{int_event_list_op, int_event_op, intlist_op};
use super::prop_access;
use super::syscom;

#[derive(Debug, Clone)]
struct ResolvedGameexeNamae {
    display: String,
    color_mod: Option<i64>,
    moji_color_no: Option<i64>,
    shadow_color_no: Option<i64>,
    fuchi_color_no: Option<i64>,
}

fn non_negative_color_no(v: i64) -> Option<i64> {
    (v >= 0).then_some(v)
}

fn resolve_gameexe_namae(
    tables: &crate::runtime::tables::AssetTables,
    raw: &str,
) -> ResolvedGameexeNamae {
    for ent in &tables.namae_entries {
        if ent.source == raw {
            return ResolvedGameexeNamae {
                display: ent.display.clone(),
                color_mod: Some(ent.color_mod),
                moji_color_no: Some(if ent.moji_color_no >= 0 {
                    ent.moji_color_no
                } else {
                    tables.mwnd_render.moji_color
                }),
                shadow_color_no: Some(if ent.shadow_color_no >= 0 {
                    ent.shadow_color_no
                } else {
                    tables.mwnd_render.shadow_color
                }),
                fuchi_color_no: Some(if ent.fuchi_color_no >= 0 {
                    ent.fuchi_color_no
                } else {
                    tables.mwnd_render.fuchi_color
                }),
            };
        }
    }
    ResolvedGameexeNamae {
        display: raw.to_string(),
        color_mod: None,
        moji_color_no: None,
        shadow_color_no: None,
        fuchi_color_no: None,
    }
}

fn global_stage_alias_to_index(form_id: u32) -> Option<i64> {
    if form_id == constants::global_form::BACK {
        Some(0)
    } else if form_id == constants::global_form::FRONT {
        Some(1)
    } else if form_id == constants::global_form::NEXT {
        Some(2)
    } else {
        None
    }
}

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
        || form_id == crate::runtime::constants::global_form::BACK as i32
        || form_id == crate::runtime::constants::global_form::FRONT as i32
        || form_id == crate::runtime::constants::global_form::NEXT as i32
        || form_id == crate::runtime::constants::global_form::STAGE_ALIAS_37 as i32
        || form_id == crate::runtime::constants::global_form::STAGE_ALIAS_38 as i32
}

fn mark_cgtable_look_from_object_create(
    tables: &mut crate::runtime::tables::AssetTables,
    disabled: bool,
    name: &str,
) {
    if disabled {
        return;
    }
    let flag_no = tables
        .cgtable
        .as_ref()
        .and_then(|t| t.get_sub_from_name(name))
        .map(|e| e.flag_no);
    let Some(flag_no) = flag_no else {
        return;
    };
    if flag_no < 0 {
        return;
    }
    let idx = flag_no as usize;
    let want = tables.cgtable_flag_cnt.unwrap_or(0).max(idx + 1);
    if tables.cg_flags.len() < want {
        tables.cg_flags.resize(want, 0);
    }
    tables.cg_flags[idx] = 1;
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

fn insert_capture_image_id(ctx: &mut CommandContext, prefer_object_capture: bool) -> ImageId {
    if prefer_object_capture {
        if let Some(img) = ctx.globals.capture_for_object_image.clone() {
            return ctx.images.insert_image(img);
        }
    }
    if let Some(img) = ctx.globals.capture_image.clone() {
        return ctx.images.insert_image(img);
    }
    let cap = ctx.capture_frame_rgba();
    ctx.images.insert_image(cap)
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
        // Global aliases are concrete stages in the original engine:
        // BACK=0, FRONT=1, NEXT=2. Do not collapse FRONT/NEXT into BACK.
        if chain.len() >= 4 && chain[2] == elm_array {
            if let Some(stage) = global_stage_alias_to_index(chain[0] as u32) {
                if chain.len() == 4 {
                    return Some(StageTarget::ChildItemRef {
                        stage,
                        child: chain[1],
                        idx: chain[3] as i64,
                    });
                }
                return Some(StageTarget::ChildItemOp {
                    stage,
                    child: chain[1],
                    idx: chain[3] as i64,
                    op: chain[4] as i64,
                    tail: chain.get(5..).unwrap_or(&[]).to_vec(),
                });
            }
        }
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

fn positional_i64(args: &[Value], idx: usize) -> Option<i64> {
    args.iter()
        .filter(|v| !matches!(v, Value::NamedArg { .. }))
        .filter_map(Value::as_i64)
        .nth(idx)
}

fn named_i64(args: &[Value], id: i32) -> Option<i64> {
    args.iter().find_map(|v| match v {
        Value::NamedArg { id: got, value } if *got == id => value.as_i64(),
        _ => None,
    })
}

fn sg_debug_enabled_local() -> bool {
    std::env::var_os("SG_DEBUG").is_some()
}

fn sg_debug_stage(msg: impl AsRef<str>) {
    if sg_debug_enabled_local() {
        eprintln!("[SG_DEBUG][STAGE] {}", msg.as_ref());
    }
}


fn sg_mwnd_object_trace_enabled() -> bool {
    std::env::var_os("SG_DEBUG").is_some()
}

fn sg_mwnd_object_trace(msg: impl AsRef<str>) {
    if sg_mwnd_object_trace_enabled() {
        eprintln!("[SG_DEBUG][MWND_OBJECT_TRACE][STAGE] {}", msg.as_ref());
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

fn stage_effect_event_mut<'a>(
    ids: &crate::runtime::constants::RuntimeConstants,
    effect: &'a mut ScreenEffectState,
    op: i32,
) -> Option<&'a mut IntEvent> {
    match op {
        s if s == ids.effect_x || s == ids.effect_x_eve => Some(&mut effect.x),
        s if s == ids.effect_y || s == ids.effect_y_eve => Some(&mut effect.y),
        s if s == ids.effect_z || s == ids.effect_z_eve => Some(&mut effect.z),
        s if s == ids.effect_mono || s == ids.effect_mono_eve => Some(&mut effect.mono),
        s if s == ids.effect_reverse || s == ids.effect_reverse_eve => Some(&mut effect.reverse),
        s if s == ids.effect_bright || s == ids.effect_bright_eve => Some(&mut effect.bright),
        s if s == ids.effect_dark || s == ids.effect_dark_eve => Some(&mut effect.dark),
        s if s == ids.effect_color_r || s == ids.effect_color_r_eve => Some(&mut effect.color_r),
        s if s == ids.effect_color_g || s == ids.effect_color_g_eve => Some(&mut effect.color_g),
        s if s == ids.effect_color_b || s == ids.effect_color_b_eve => Some(&mut effect.color_b),
        s if s == ids.effect_color_rate || s == ids.effect_color_rate_eve => Some(&mut effect.color_rate),
        s if s == ids.effect_color_add_r || s == ids.effect_color_add_r_eve => Some(&mut effect.color_add_r),
        s if s == ids.effect_color_add_g || s == ids.effect_color_add_g_eve => Some(&mut effect.color_add_g),
        s if s == ids.effect_color_add_b || s == ids.effect_color_add_b_eve => Some(&mut effect.color_add_b),
        _ => None,
    }
}

fn stage_effect_prop_mut<'a>(
    ids: &crate::runtime::constants::RuntimeConstants,
    effect: &'a mut ScreenEffectState,
    op: i32,
) -> Option<&'a mut i32> {
    match op {
        s if s == ids.effect_wipe_copy => Some(&mut effect.wipe_copy),
        s if s == ids.effect_wipe_erase => Some(&mut effect.wipe_erase),
        s if s == ids.effect_begin_order => Some(&mut effect.begin_order),
        s if s == ids.effect_begin_layer => Some(&mut effect.begin_layer),
        s if s == ids.effect_end_order => Some(&mut effect.end_order),
        s if s == ids.effect_end_layer => Some(&mut effect.end_layer),
        _ => None,
    }
}

fn dispatch_stage_effect_op(
    ctx: &mut CommandContext,
    effect: &mut ScreenEffectState,
    op: i32,
    tail: &[i32],
    script_args: &[Value],
    rhs: Option<&Value>,
    al_id: Option<i64>,
    ret_form: Option<i64>,
) -> bool {
    let ids = ctx.ids.clone();
    if op == ids.effect_init {
        effect.reinit();
        push_ok(ctx, ret_form);
        return true;
    }

    if !tail.is_empty() {
        if let Some(ev) = stage_effect_event_mut(&ids, effect, op) {
            if let Some(()) = dispatch_int_event_arg_slot(ctx, ev, tail, script_args, rhs, al_id, ret_form) {
                return true;
            }
            if let Some(action) = dispatch_int_event_subop(ev, tail[0], script_args, al_id) {
                match action {
                    IntEventDispatchAction::Done => ctx.stack.push(default_for_ret_form(ret_form.unwrap_or(0))),
                    IntEventDispatchAction::Wait { key_skip } => {
                        if key_skip {
                            ctx.wait.wait_generic_int_event(0, None, true, true);
                        } else {
                            ctx.wait.wait_generic_int_event(0, None, false, false);
                        }
                    }
                }
                return true;
            }
        }
        return false;
    }

    if let Some(ev) = stage_effect_event_mut(&ids, effect, op) {
        match al_id {
            Some(0) => ctx.stack.push(Value::Int(ev.get_total_value() as i64)),
            Some(1) => {
                let value = rhs.or_else(|| script_args.first()).and_then(as_i64).unwrap_or(0) as i32;
                ev.set_value(value);
                ev.frame();
                push_ok(ctx, ret_form);
            }
            _ => push_ok(ctx, ret_form),
        }
        return true;
    }

    if let Some(slot) = stage_effect_prop_mut(&ids, effect, op) {
        match al_id {
            Some(0) => ctx.stack.push(Value::Int(*slot as i64)),
            Some(1) => {
                let mut value = rhs.or_else(|| script_args.first()).and_then(as_i64).unwrap_or(0) as i32;
                if op == ids.effect_wipe_copy || op == ids.effect_wipe_erase {
                    value = if value != 0 { 1 } else { 0 };
                }
                *slot = value;
                push_ok(ctx, ret_form);
            }
            _ => push_ok(ctx, ret_form),
        }
        return true;
    }

    false
}

fn dispatch_stage_effect_list_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    op: i32,
    script_args: &[Value],
    ret_form: Option<i64>,
) -> bool {
    if op == constants::EFFECTLIST_RESIZE {
        let n = script_args.first().and_then(as_i64).unwrap_or(0).max(0) as usize;
        st.ensure_effect_list(stage_idx, n);
        push_ok(ctx, ret_form);
        return true;
    }
    if op == constants::EFFECTLIST_GET_SIZE {
        let n = st.effect_lists.get(&stage_idx).map(|v| v.len()).unwrap_or(0);
        ctx.stack.push(Value::Int(n as i64));
        return true;
    }
    false
}

fn dispatch_stage_effect_item_op(
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
    st.ensure_effect_list(stage_idx, idx + 1);
    let list = st.effect_lists.get_mut(&stage_idx).unwrap();
    let effect = &mut list[idx];
    if op == 0 && tail.is_empty() {
        push_ok(ctx, ret_form);
        return true;
    }
    dispatch_stage_effect_op(ctx, effect, op, tail, script_args, rhs, al_id, ret_form)
}

fn last_script_list_arg(script_args: &[Value]) -> Option<&Vec<Value>> {
    script_args.last().and_then(|v| match v.unwrap_named() {
        Value::List(list) => Some(list),
        _ => None,
    })
}

fn quake_start_kind(op: i32) -> Option<(bool, bool, bool)> {
    match op {
        constants::QUAKE_START => Some((false, false, false)),
        constants::QUAKE_START_WAIT => Some((false, true, false)),
        constants::QUAKE_START_WAIT_KEY => Some((false, true, true)),
        constants::QUAKE_START_NOWAIT => Some((false, false, false)),
        constants::QUAKE_START_ALL => Some((true, false, false)),
        constants::QUAKE_START_ALL_WAIT => Some((true, true, false)),
        constants::QUAKE_START_ALL_WAIT_KEY => Some((true, true, true)),
        constants::QUAKE_START_ALL_NOWAIT => Some((true, false, false)),
        _ => None,
    }
}

fn dispatch_stage_quake_item_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    idx: usize,
    op: i32,
    script_args: &[Value],
    ret_form: Option<i64>,
) -> bool {
    st.ensure_quake_list(stage_idx, idx + 1);
    let list = st.quake_lists.get_mut(&stage_idx).unwrap();
    let quake: &mut ScreenQuakeState = &mut list[idx];

    if let Some((all_range, wait_flag, key_flag)) = quake_start_kind(op) {
        let quake_type = script_args.first().and_then(as_i64).unwrap_or(0) as i32;
        let time = script_args.get(1).and_then(as_i64).unwrap_or(1000);
        let _cnt = script_args.get(2).and_then(as_i64).unwrap_or(0) as i32;
        let _end_cnt = script_args.get(3).and_then(as_i64).unwrap_or(0) as i32;
        quake.begin_order = if all_range { i32::MIN } else { 0 };
        quake.end_order = if all_range { i32::MAX } else { 0 };
        if script_args.len() >= 6 {
            quake.begin_order = script_args.get(4).and_then(as_i64).unwrap_or(quake.begin_order as i64) as i32;
            quake.end_order = script_args.get(5).and_then(as_i64).unwrap_or(quake.end_order as i64) as i32;
        }
        let opt = last_script_list_arg(script_args);
        quake.power = opt.and_then(|list| list.first()).and_then(as_i64).unwrap_or(0) as i32;
        if quake_type == 2 {
            quake.center_x = opt.and_then(|list| list.get(1)).and_then(as_i64).unwrap_or(0) as i32;
            quake.center_y = opt.and_then(|list| list.get(2)).and_then(as_i64).unwrap_or(0) as i32;
            quake.vec = 0;
        } else {
            quake.vec = opt.and_then(|list| list.get(1)).and_then(as_i64).unwrap_or(0) as i32;
            quake.center_x = 0;
            quake.center_y = 0;
        }
        quake.start_kind(quake_type, time);
        if wait_flag {
            let rem = quake.remaining_ms();
            if key_flag {
                ctx.wait.wait_ms_key(rem);
            } else {
                ctx.wait.wait_ms(rem);
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    match op {
        constants::QUAKE_END => {
            quake.end_ms(script_args.first().and_then(as_i64).unwrap_or(0));
            push_ok(ctx, ret_form);
            true
        }
        constants::QUAKE_WAIT => {
            ctx.wait.wait_ms(quake.remaining_ms());
            push_ok(ctx, ret_form);
            true
        }
        constants::QUAKE_WAIT_KEY => {
            ctx.wait.wait_ms_key(quake.remaining_ms());
            push_ok(ctx, ret_form);
            true
        }
        constants::QUAKE_CHECK => {
            ctx.stack.push(Value::Int(quake.check_value() as i64));
            true
        }
        _ => false,
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

enum IntEventDispatchAction {
    Done,
    Wait { key_skip: bool },
}

fn apply_named_event_start(ev: &mut IntEvent, script_args: &[Value]) {
    for arg in script_args {
        if let Value::NamedArg { id: 0, value } = arg {
            if let Some(v) = value.as_i64() {
                let v = v as i32;
                ev.set_value(v);
                ev.cur_value = v;
            }
        }
    }
}

fn is_element_array_marker(ctx: &CommandContext, code: i32) -> bool {
    code == ctx.ids.elm_array || code == super::codes::ELM_ARRAY || code == -1
}

fn split_property_list_tail<'a>(
    ctx: &CommandContext,
    tail: &'a [i32],
    al_id: Option<i64>,
    ret_form: Option<i64>,
    rhs: Option<&Value>,
    script_args: &[Value],
) -> (Option<i64>, &'a [i32]) {
    if tail.len() >= 2 && is_element_array_marker(ctx, tail[0]) {
        return (Some(tail[1] as i64), &tail[2..]);
    }

    // Some recovered Siglus scripts use a compact element chain for list
    // properties under OBJECT/CHILD, e.g. OBJECT.X_REP[2] appears as
    // [..., OBJECT_X_REP, 2] instead of [..., OBJECT_X_REP, ELM_ARRAY, 2].
    // Treat the compact form as an index access for assignments, reads, or
    // nested sub-operations. Plain void calls with one script argument stay as
    // list commands such as RESIZE.
    if let Some(&first) = tail.first() {
        let looks_like_index_access = first >= 0
            && (al_id == Some(1)
                || rhs.is_some()
                || matches!(ret_form, Some(rf) if rf != 0)
                || tail.len() >= 2
                || script_args.is_empty());
        if looks_like_index_access {
            return (Some(first as i64), &tail[1..]);
        }
    }

    (None, tail)
}

fn int_event_command_arg_slot_tail(ctx: &CommandContext, tail: &[i32]) -> Option<(i32, i32)> {
    if tail.len() >= 3 && is_element_array_marker(ctx, tail[1]) {
        Some((tail[0], tail[2]))
    } else {
        None
    }
}

fn int_event_arg_slot_value<'a>(
    rhs: Option<&'a Value>,
    script_args: &'a [Value],
    al_id: Option<i64>,
) -> Option<&'a Value> {
    rhs.or_else(|| {
        if al_id == Some(1) && script_args.len() == 1 {
            script_args.first()
        } else {
            None
        }
    })
}

fn dispatch_int_event_arg_slot(
    ctx: &mut CommandContext,
    ev: &mut IntEvent,
    tail: &[i32],
    script_args: &[Value],
    rhs: Option<&Value>,
    al_id: Option<i64>,
    ret_form: Option<i64>,
) -> Option<()> {
    let (subop, arg_slot) = int_event_command_arg_slot_tail(ctx, tail)?;

    if let Some(v) = int_event_arg_slot_value(rhs, script_args, al_id).and_then(as_i64) {
        // C++ handles INTEVENT.SET named argument id 0 as "start": it changes
        // the current/base event value before set_event() uses it.
        if (subop == int_event_op::SET || subop == int_event_op::SET_REAL) && arg_slot == 0 {
            ev.set_value(v as i32);
            sg_debug_stage(format!(
                "INTEVENT.SET named start={} applied through arg slot tail={:?}",
                v, tail
            ));
        } else {
            sg_debug_stage(format!(
                "INTEVENT arg slot assignment ignored subop={} slot={} value={} tail={:?}",
                subop, arg_slot, v, tail
            ));
        }
        push_ok(ctx, ret_form);
    } else if matches!(ret_form, Some(rf) if rf == 0) {
        // A void access to a command argument slot is bookkeeping, not an event command.
        push_ok(ctx, ret_form);
    } else {
        // Property-style reads of a command argument slot must not execute SET/LOOP/TURN.
        ctx.stack.push(Value::Int(ev.get_value() as i64));
    }
    Some(())
}

fn dispatch_int_event_subop(
    ev: &mut IntEvent,
    subop: i32,
    script_args: &[Value],
    _al_id: Option<i64>,
) -> Option<IntEventDispatchAction> {
    match subop {
        int_event_op::SET | int_event_op::SET_REAL => {
            apply_named_event_start(ev, script_args);
            if script_args.len() >= 4 {
                let value = script_args.get(0).and_then(as_i64).unwrap_or(0) as i32;
                let total_time = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
                let delay_time = script_args.get(2).and_then(as_i64).unwrap_or(0) as i32;
                let speed_type = script_args.get(3).and_then(as_i64).unwrap_or(0) as i32;

                let real_flag = if subop == int_event_op::SET_REAL {
                    1
                } else {
                    0
                };
                ev.set_event(value, total_time, delay_time, speed_type, real_flag);
                sg_debug_stage(format!(
                    "INTEVENT.SET subop={} value={} total_time={} delay_time={} speed_type={} real={} start={} cur={} active={}",
                    subop,
                    value,
                    total_time,
                    delay_time,
                    speed_type,
                    real_flag,
                    ev.start_value,
                    ev.cur_value,
                    ev.check_event(),
                ));
            } else {
                sg_debug_stage(format!(
                    "INTEVENT.SET subop={} ignored: argc={} args={:?}",
                    subop,
                    script_args.len(),
                    script_args,
                ));
            }
            Some(IntEventDispatchAction::Done)
        }
        int_event_op::LOOP | int_event_op::LOOP_REAL => {
            if script_args.len() >= 5 {
                let start_value = script_args.get(0).and_then(as_i64).unwrap_or(0) as i32;
                let end_value = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
                let loop_time = script_args.get(2).and_then(as_i64).unwrap_or(0) as i32;
                let delay_time = script_args.get(3).and_then(as_i64).unwrap_or(0) as i32;
                let speed_type = script_args.get(4).and_then(as_i64).unwrap_or(0) as i32;

                let real_flag = if subop == int_event_op::LOOP_REAL {
                    1
                } else {
                    0
                };
                ev.loop_event(
                    start_value,
                    end_value,
                    loop_time,
                    delay_time,
                    speed_type,
                    real_flag,
                );
                sg_debug_stage(format!(
                    "INTEVENT.LOOP subop={} start={} end={} loop_time={} delay_time={} speed_type={} real={} active={}",
                    subop,
                    start_value,
                    end_value,
                    loop_time,
                    delay_time,
                    speed_type,
                    real_flag,
                    ev.check_event(),
                ));
            } else {
                sg_debug_stage(format!(
                    "INTEVENT.LOOP subop={} ignored: argc={} args={:?}",
                    subop,
                    script_args.len(),
                    script_args,
                ));
            }
            Some(IntEventDispatchAction::Done)
        }
        int_event_op::TURN | int_event_op::TURN_REAL => {
            if script_args.len() >= 5 {
                let start_value = script_args.get(0).and_then(as_i64).unwrap_or(0) as i32;
                let end_value = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
                let loop_time = script_args.get(2).and_then(as_i64).unwrap_or(0) as i32;
                let delay_time = script_args.get(3).and_then(as_i64).unwrap_or(0) as i32;
                let speed_type = script_args.get(4).and_then(as_i64).unwrap_or(0) as i32;

                let real_flag = if subop == int_event_op::TURN_REAL {
                    1
                } else {
                    0
                };
                ev.turn_event(
                    start_value,
                    end_value,
                    loop_time,
                    delay_time,
                    speed_type,
                    real_flag,
                );
                sg_debug_stage(format!(
                    "INTEVENT.TURN subop={} start={} end={} loop_time={} delay_time={} speed_type={} real={} active={}",
                    subop,
                    start_value,
                    end_value,
                    loop_time,
                    delay_time,
                    speed_type,
                    real_flag,
                    ev.check_event(),
                ));
            } else {
                sg_debug_stage(format!(
                    "INTEVENT.TURN subop={} ignored: argc={} args={:?}",
                    subop,
                    script_args.len(),
                    script_args,
                ));
            }
            Some(IntEventDispatchAction::Done)
        }
        int_event_op::END => {
            ev.end_event();
            Some(IntEventDispatchAction::Done)
        }
        int_event_op::WAIT => Some(IntEventDispatchAction::Wait { key_skip: false }),
        int_event_op::WAIT_KEY => Some(IntEventDispatchAction::Wait { key_skip: true }),
        int_event_op::CHECK => Some(IntEventDispatchAction::Done),
        _ => None,
    }
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

const TNM_STAGE_BACK: i64 = 0;
const TNM_STAGE_FRONT: i64 = 1;
const TNM_STAGE_NEXT: i64 = 2;
const TNM_STAGE_CNT: i64 = 3;

const INIDEF_OBJECT_CNT: usize = 256;
const INIMAX_OBJECT_CNT: usize = 1024;
const INIDEF_BTN_GROUP_CNT: usize = 4;
const INIMAX_BTN_GROUP_CNT: usize = 256;
const INIDEF_WORLD_CNT: usize = 1;
const INIMAX_WORLD_CNT: usize = 256;
const INIDEF_EFFECT_CNT: usize = 4;
const INIMAX_EFFECT_CNT: usize = 256;
const INIDEF_QUAKE_CNT: usize = 16;
const INIMAX_QUAKE_CNT: usize = 256;

fn parse_i64_local(s: &str) -> Option<i64> {
    let t = s.trim().trim_matches('"');
    t.parse::<i64>().ok()
}

fn cfg_usize_or(
    ctx: &CommandContext,
    key: &str,
    default_value: usize,
    max_value: usize,
) -> usize {
    ctx.tables
        .gameexe
        .as_ref()
        .and_then(|cfg| cfg.get_usize(key))
        .unwrap_or(default_value)
        .min(max_value)
}

fn cfg_usize_or_any(
    ctx: &CommandContext,
    keys: &[&str],
    default_value: usize,
    max_value: usize,
) -> usize {
    if let Some(cfg) = ctx.tables.gameexe.as_ref() {
        for key in keys {
            if let Some(v) = cfg.get_usize(key) {
                return v.min(max_value);
            }
        }
    }
    default_value.min(max_value)
}

fn stage_object_use_flags(ctx: &CommandContext, object_cnt: usize) -> Vec<bool> {
    let mut out = vec![true; object_cnt];
    let Some(cfg) = ctx.tables.gameexe.as_ref() else {
        return out;
    };
    for i in 0..object_cnt {
        if let Some(v) = cfg
            .get_indexed_field("OBJECT", i, "USE")
            .and_then(parse_i64_local)
        {
            out[i] = v != 0;
        }
    }
    out
}

fn extend_stage_object_list_with_use_flags(
    st: &mut StageFormState,
    stage_idx: i64,
    object_use: &[bool],
) {
    let entry = st.object_lists.entry(stage_idx).or_default();
    let old_len = entry.len();
    if old_len < object_use.len() {
        entry.reserve(object_use.len() - old_len);
        for &used in &object_use[old_len..] {
            let mut obj = ObjectState::default();
            obj.used = used;
            entry.push(obj);
        }
    }

    let slot_use = st.object_slot_use.entry(stage_idx).or_default();
    if slot_use.len() < object_use.len() {
        slot_use.extend_from_slice(&object_use[slot_use.len()..]);
    }
}

fn resize_stage_object_slot_use_like_cpp(
    ctx: &CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    new_len: usize,
) {
    let slot_use = st.object_slot_use.entry(stage_idx).or_default();
    let old_len = slot_use.len();
    if new_len < old_len {
        slot_use.truncate(new_len);
    } else if new_len > old_len {
        for i in old_len..new_len {
            slot_use.push(stage_object_use_at(ctx, i));
        }
    }
}

fn stage_object_slot_use_at(ctx: &CommandContext, st: &StageFormState, stage_idx: i64, idx: usize) -> bool {
    st.object_slot_use
        .get(&stage_idx)
        .and_then(|flags| flags.get(idx))
        .copied()
        .unwrap_or_else(|| stage_object_use_at(ctx, idx))
}

fn stage_object_use_at(ctx: &CommandContext, idx: usize) -> bool {
    ctx.tables
        .gameexe
        .as_ref()
        .and_then(|cfg| cfg.get_indexed_field("OBJECT", idx, "USE"))
        .and_then(parse_i64_local)
        .map(|v| v != 0)
        .unwrap_or(true)
}

fn push_stage_object_initialized_from_gameexe(
    ctx: &CommandContext,
    list: &mut Vec<ObjectState>,
    idx: usize,
) {
    let mut obj = ObjectState::default();
    obj.used = stage_object_use_at(ctx, idx);
    list.push(obj);
}

fn resize_stage_object_list_like_cpp(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    new_len: usize,
) {
    let old_len = st.object_list_len(stage_idx);
    if new_len < old_len {
        if let Some(list) = st.object_lists.get_mut(&stage_idx) {
            for i in new_len..old_len {
                let obj = &mut list[i];
                object_clear_backend_recursive(ctx, obj, stage_idx, i);
            }
            list.truncate(new_len);
        }
    } else if new_len > old_len {
        let list = st.object_lists.entry(stage_idx).or_default();
        list.reserve(new_len - old_len);
        for i in old_len..new_len {
            push_stage_object_initialized_from_gameexe(ctx, list, i);
        }
    }
    resize_stage_object_slot_use_like_cpp(ctx, st, stage_idx, new_len);
    st.object_list_strict.insert(stage_idx, true);
}

fn extend_list_with_default<T: Default>(list: &mut Vec<T>, cnt: usize) {
    if list.len() < cnt {
        list.extend((0..(cnt - list.len())).map(|_| T::default()));
    }
}

fn extend_world_list_with_indices(list: &mut Vec<WorldState>, cnt: usize) {
    while list.len() < cnt {
        list.push(WorldState::new(list.len() as i32));
    }
}

fn ensure_stage_form_initialized_from_gameexe(ctx: &CommandContext, st: &mut StageFormState) {
    if st.initialized_from_gameexe {
        return;
    }

    // Original C++ initializes the global stage list eagerly in eng_init.cpp:
    //   m_stage_list.init(..., TNM_STAGE_CNT, false, true, NULL)
    // and C_elm_stage::init initializes each sub-list from Gp_ini counts.
    // C++ constructs BACK/FRONT/NEXT eagerly before any script can touch a stage.
    // This Rust port creates the state lazily, so BACK may already contain objects
    // by the time this initialization runs.  Preserve the original invariant by
    // sizing every stage object list to at least the largest list that already
    // exists, then use the C++ object.use defaults for newly materialized peers.
    let cfg_object_cnt = cfg_usize_or(ctx, "OBJECT.CNT", INIDEF_OBJECT_CNT, INIMAX_OBJECT_CNT);
    let existing_object_cnt = st
        .object_lists
        .values()
        .map(|list| list.len())
        .max()
        .unwrap_or(0)
        .min(INIMAX_OBJECT_CNT);
    let object_cnt = cfg_object_cnt.max(existing_object_cnt).min(INIMAX_OBJECT_CNT);
    let mut object_use = stage_object_use_flags(ctx, object_cnt);
    for list in st.object_lists.values() {
        for (idx, obj) in list.iter().enumerate().take(object_cnt) {
            if obj.used || object_is_prepared_for_stage_wipe(obj) {
                object_use[idx] = true;
            }
        }
    }
    let group_cnt = cfg_usize_or_any(
        ctx,
        &["OBJBTNGROUP.CNT", "BUTTON.GROUP.CNT"],
        INIDEF_BTN_GROUP_CNT,
        INIMAX_BTN_GROUP_CNT,
    );
    let mwnd_cnt = ctx.tables.mwnd_templates.len();
    let world_cnt = cfg_usize_or(ctx, "WORLD.CNT", INIDEF_WORLD_CNT, INIMAX_WORLD_CNT);
    let effect_cnt = cfg_usize_or(ctx, "EFFECT.CNT", INIDEF_EFFECT_CNT, INIMAX_EFFECT_CNT);
    let quake_cnt = cfg_usize_or(ctx, "QUAKE.CNT", INIDEF_QUAKE_CNT, INIMAX_QUAKE_CNT);

    for stage_idx in TNM_STAGE_BACK..TNM_STAGE_CNT {
        extend_stage_object_list_with_use_flags(st, stage_idx, &object_use);

        let groups = st.group_lists.entry(stage_idx).or_default();
        extend_list_with_default(groups, group_cnt);

        let mwnds = st.mwnd_lists.entry(stage_idx).or_default();
        extend_list_with_default(mwnds, mwnd_cnt);

        let worlds = st.world_lists.entry(stage_idx).or_default();
        extend_world_list_with_indices(worlds, world_cnt);

        let effects = st.effect_lists.entry(stage_idx).or_default();
        extend_list_with_default(effects, effect_cnt);

        let quakes = st.quake_lists.entry(stage_idx).or_default();
        extend_list_with_default(quakes, quake_cnt);
    }

    st.initialized_from_gameexe = true;
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
    ensure_stage_form_initialized_from_gameexe(ctx, &mut st);
    let r = f(ctx, &mut st);
    ctx.globals.stage_forms.insert(form_id, st);
    r
}

fn sorter_le(lhs_order: i64, lhs_layer: i64, rhs_order: i64, rhs_layer: i64) -> bool {
    lhs_order < rhs_order || (lhs_order == rhs_order && lhs_layer <= rhs_layer)
}

fn sorter_in_range(
    order: i64,
    layer: i64,
    begin_order: i32,
    begin_layer: i32,
    end_order: i32,
    end_layer: i32,
) -> bool {
    sorter_le(begin_order as i64, begin_layer as i64, order, layer)
        && sorter_le(order, layer, end_order as i64, end_layer as i64)
}

fn object_sorter(ctx: &CommandContext, obj: &ObjectState) -> (i64, i64) {
    let order = if ctx.ids.obj_order != 0 {
        obj.get_int_prop(&ctx.ids, ctx.ids.obj_order)
    } else {
        obj.base.order
    };
    let layer = if ctx.ids.obj_layer != 0 {
        obj.get_int_prop(&ctx.ids, ctx.ids.obj_layer)
    } else {
        obj.base.layer
    };
    (order, layer)
}

fn extend_stage_object_list_at_least(st: &mut StageFormState, stage_idx: i64, cnt: usize) {
    let entry = st.object_lists.entry(stage_idx).or_default();
    if entry.len() < cnt {
        entry.extend((0..(cnt - entry.len())).map(|_| ObjectState::default()));
    }
    let slot_use = st.object_slot_use.entry(stage_idx).or_default();
    if slot_use.len() < cnt {
        slot_use.extend((0..(cnt - slot_use.len())).map(|_| true));
    }
}

fn extend_stage_mwnd_list_at_least(st: &mut StageFormState, stage_idx: i64, cnt: usize) {
    let entry = st.mwnd_lists.entry(stage_idx).or_default();
    if entry.len() < cnt {
        entry.extend((0..(cnt - entry.len())).map(|_| MwndState::default()));
    }
}

fn extend_stage_group_list_at_least(st: &mut StageFormState, stage_idx: i64, cnt: usize) {
    let entry = st.group_lists.entry(stage_idx).or_default();
    if entry.len() < cnt {
        entry.extend((0..(cnt - entry.len())).map(|_| GroupState::default()));
    }
}

fn extend_stage_world_list_at_least(st: &mut StageFormState, stage_idx: i64, cnt: usize) {
    let list = st.world_lists.entry(stage_idx).or_insert_with(Vec::new);
    if list.len() < cnt {
        for i in list.len()..cnt {
            list.push(WorldState::new(i as i32));
        }
    }
}

fn extend_stage_effect_list_at_least(st: &mut StageFormState, stage_idx: i64, cnt: usize) {
    let entry = st.effect_lists.entry(stage_idx).or_default();
    if entry.len() < cnt {
        entry.extend((0..(cnt - entry.len())).map(|_| ScreenEffectState::default()));
    }
}

fn extend_stage_quake_list_at_least(st: &mut StageFormState, stage_idx: i64, cnt: usize) {
    let entry = st.quake_lists.entry(stage_idx).or_default();
    if entry.len() < cnt {
        entry.extend((0..(cnt - entry.len())).map(|_| ScreenQuakeState::default()));
    }
}

fn object_has_backend_for_stage_wipe(obj: &ObjectState) -> bool {
    !matches!(obj.backend, ObjectBackend::None)
}

fn object_is_prepared_for_stage_wipe(obj: &ObjectState) -> bool {
    obj.object_type != 0 || !obj.runtime.child_objects.is_empty() || object_has_backend_for_stage_wipe(obj)
}

fn object_slot_is_enabled_for_stage_wipe(
    ctx: &CommandContext,
    st: &StageFormState,
    idx: usize,
) -> bool {
    // C++ C_elm_stage_list::wipe checks C_elm_object::is_use(), the fixed
    // object-slot enable flag initialized from Gp_ini.  Rust ObjectState::used
    // is an active/runtime flag, so FRONT.used can be false for an initialized
    // empty slot even when BACK has prepared content for that same slot.
    // A prepared peer slot is direct runtime evidence that this slot must pass
    // the wipe gate; otherwise fall back to the Gameexe slot flag.  Do not gate
    // solely on FRONT slot-use: scripts often prepare BACK objects and then
    // WIPE them into FRONT.
    for stage_idx in TNM_STAGE_BACK..TNM_STAGE_CNT {
        if let Some(obj) = st.object_lists.get(&stage_idx).and_then(|list| list.get(idx)) {
            if obj.used || object_is_prepared_for_stage_wipe(obj) {
                return true;
            }
        }
    }
    stage_object_slot_use_at(ctx, st, TNM_STAGE_FRONT, idx)
}

fn object_wipe_copy_value(ctx: &CommandContext, obj: &ObjectState) -> i64 {
    if ctx.ids.obj_wipe_copy != 0 {
        obj.get_int_prop(&ctx.ids, ctx.ids.obj_wipe_copy)
    } else {
        obj.base.wipe_copy
    }
}

fn object_wipe_erase_value(ctx: &CommandContext, obj: &ObjectState) -> i64 {
    if ctx.ids.obj_wipe_erase != 0 {
        obj.get_int_prop(&ctx.ids, ctx.ids.obj_wipe_erase)
    } else {
        obj.base.wipe_erase
    }
}

fn clear_root_object_for_stage_wipe(
    ctx: &mut CommandContext,
    list: &mut Vec<ObjectState>,
    stage_idx: i64,
    idx: usize,
) {
    if list.len() <= idx {
        list.resize_with(idx + 1, ObjectState::default);
    }
    let used = list[idx].used;
    object_clear_backend_recursive(ctx, &mut list[idx], stage_idx, idx);
    list[idx] = ObjectState::default();
    list[idx].used = used;
}

fn copy_root_object_for_stage_wipe(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    dst_stage: i64,
    dst_idx: usize,
    src: &ObjectState,
) {
    extend_stage_object_list_at_least(st, dst_stage, dst_idx + 1);
    let mut copy = src.clone();
    let mut old = {
        let list = st.object_lists.get_mut(&dst_stage).unwrap();
        std::mem::take(&mut list[dst_idx])
    };
    object_clear_backend_recursive(ctx, &mut old, dst_stage, dst_idx);
    assign_copy_runtime_slots(st, dst_stage, &mut copy, None);
    duplicate_object_tree_backends_for_copy(ctx, st, dst_stage, &mut copy, dst_idx);
    let list = st.object_lists.get_mut(&dst_stage).unwrap();
    list[dst_idx] = copy;
}

fn clear_embedded_objects_for_stage_wipe(
    ctx: &mut CommandContext,
    list: &mut Vec<ObjectState>,
    stage_idx: i64,
) {
    for (idx, obj) in list.iter_mut().enumerate() {
        let slot = obj.runtime_slot_or(idx);
        object_clear_backend_recursive(ctx, obj, stage_idx, slot);
    }
    list.clear();
}

fn clone_embedded_objects_for_stage_wipe(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    dst_stage: i64,
    src: &[ObjectState],
) -> Vec<ObjectState> {
    let mut out = Vec::with_capacity(src.len());
    for src_obj in src {
        let mut copy = src_obj.clone();
        copy.nested_runtime_slot = None;
        let slot = nested_object_slot(st, dst_stage, &mut copy);
        assign_copy_runtime_slots(st, dst_stage, &mut copy, Some(slot));
        duplicate_object_tree_backends_for_copy(ctx, st, dst_stage, &mut copy, slot);
        out.push(copy);
    }
    out
}

fn clear_mwnd_embedded_objects_for_stage_wipe(
    ctx: &mut CommandContext,
    mwnd: &mut MwndState,
    stage_idx: i64,
) {
    clear_embedded_objects_for_stage_wipe(ctx, &mut mwnd.button_list, stage_idx);
    clear_embedded_objects_for_stage_wipe(ctx, &mut mwnd.face_list, stage_idx);
    clear_embedded_objects_for_stage_wipe(ctx, &mut mwnd.object_list, stage_idx);
}

fn copy_mwnd_for_stage_wipe(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    dst_stage: i64,
    dst_idx: usize,
    src: &MwndState,
) {
    extend_stage_mwnd_list_at_least(st, dst_stage, dst_idx + 1);
    let mut old = {
        let list = st.mwnd_lists.get_mut(&dst_stage).unwrap();
        std::mem::take(&mut list[dst_idx])
    };
    clear_mwnd_embedded_objects_for_stage_wipe(ctx, &mut old, dst_stage);

    let mut copy = src.clone();
    copy.button_list = clone_embedded_objects_for_stage_wipe(ctx, st, dst_stage, &src.button_list);
    copy.face_list = clone_embedded_objects_for_stage_wipe(ctx, st, dst_stage, &src.face_list);
    copy.object_list = clone_embedded_objects_for_stage_wipe(ctx, st, dst_stage, &src.object_list);

    let list = st.mwnd_lists.get_mut(&dst_stage).unwrap();
    list[dst_idx] = copy;
}

fn reset_mwnd_for_stage_wipe(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    idx: usize,
) {
    extend_stage_mwnd_list_at_least(st, stage_idx, idx + 1);
    let mut old = {
        let list = st.mwnd_lists.get_mut(&stage_idx).unwrap();
        std::mem::take(&mut list[idx])
    };
    clear_mwnd_embedded_objects_for_stage_wipe(ctx, &mut old, stage_idx);
    let list = st.mwnd_lists.get_mut(&stage_idx).unwrap();
    list[idx] = MwndState::default();
    ensure_mwnd(ctx, st, stage_idx, idx);
}

fn clear_btnselitem_list_for_stage_wipe(
    ctx: &mut CommandContext,
    list: &mut Vec<BtnSelItemState>,
    stage_idx: i64,
) {
    for item in list.iter_mut() {
        clear_embedded_objects_for_stage_wipe(ctx, &mut item.object_list, stage_idx);
    }
    list.clear();
}

fn clone_btnselitem_list_for_stage_wipe(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    dst_stage: i64,
    src: &[BtnSelItemState],
) -> Vec<BtnSelItemState> {
    let mut out = Vec::with_capacity(src.len());
    for src_item in src {
        let mut copy = src_item.clone();
        copy.object_list =
            clone_embedded_objects_for_stage_wipe(ctx, st, dst_stage, &src_item.object_list);
        out.push(copy);
    }
    out
}


fn stage_wipe_object_lists(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    begin_order: i32,
    end_order: i32,
    begin_layer: i32,
    end_layer: i32,
) {
    let front_len = st.object_lists.get(&1).map(|v| v.len()).unwrap_or(0);
    extend_stage_object_list_at_least(st, 0, front_len);
    extend_stage_object_list_at_least(st, 2, front_len);

    for idx in 0..front_len {
        let Some(front) = st.object_lists.get(&1).and_then(|list| list.get(idx)).cloned() else {
            continue;
        };
        if !object_slot_is_enabled_for_stage_wipe(ctx, st, idx) {
            continue;
        }

        let back = st
            .object_lists
            .get(&0)
            .and_then(|list| list.get(idx))
            .cloned()
            .unwrap_or_default();
        let (front_order, front_layer) = object_sorter(ctx, &front);
        let back_prepared = object_is_prepared_for_stage_wipe(&back);
        if sorter_in_range(front_order, front_layer, begin_order, begin_layer, end_order, end_layer)
            || back_prepared
        {
            copy_root_object_for_stage_wipe(ctx, st, 2, idx, &front);

            if object_wipe_copy_value(ctx, &front) == 0
                || back_prepared
                || object_wipe_erase_value(ctx, &back) == 1
            {
                copy_root_object_for_stage_wipe(ctx, st, 1, idx, &back);
                let list = st.object_lists.get_mut(&0).unwrap();
                clear_root_object_for_stage_wipe(ctx, list, 0, idx);
            }
        }
    }
}

fn stage_wipe_mwnd_lists(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    begin_order: i32,
    end_order: i32,
    begin_layer: i32,
    end_layer: i32,
) {
    let front_len = st.mwnd_lists.get(&1).map(|v| v.len()).unwrap_or(0);
    extend_stage_mwnd_list_at_least(st, 0, front_len);
    extend_stage_mwnd_list_at_least(st, 2, front_len);

    for idx in 0..front_len {
        ensure_mwnd(ctx, st, 1, idx);
        ensure_mwnd(ctx, st, 0, idx);
        ensure_mwnd(ctx, st, 2, idx);
        let Some(front) = st.mwnd_lists.get(&1).and_then(|list| list.get(idx)).cloned() else {
            continue;
        };
        if sorter_in_range(front.order, front.layer, begin_order, begin_layer, end_order, end_layer) {
            let back = st
                .mwnd_lists
                .get(&0)
                .and_then(|list| list.get(idx))
                .cloned()
                .unwrap_or_default();
            copy_mwnd_for_stage_wipe(ctx, st, 2, idx, &front);
            copy_mwnd_for_stage_wipe(ctx, st, 1, idx, &back);
            reset_mwnd_for_stage_wipe(ctx, st, 0, idx);
        }
    }
}

fn stage_wipe_group_lists(
    st: &mut StageFormState,
    begin_order: i32,
    end_order: i32,
    begin_layer: i32,
    end_layer: i32,
) {
    let front_len = st.group_lists.get(&1).map(|v| v.len()).unwrap_or(0);
    extend_stage_group_list_at_least(st, 0, front_len);
    extend_stage_group_list_at_least(st, 2, front_len);

    for idx in 0..front_len {
        let Some(front) = st.group_lists.get(&1).and_then(|list| list.get(idx)).cloned() else {
            continue;
        };
        if sorter_in_range(front.order, front.layer, begin_order, begin_layer, end_order, end_layer) {
            let back = st
                .group_lists
                .get(&0)
                .and_then(|list| list.get(idx))
                .cloned()
                .unwrap_or_default();
            if let Some(list) = st.group_lists.get_mut(&2) {
                list[idx] = front;
            }
            if let Some(list) = st.group_lists.get_mut(&1) {
                list[idx] = back;
            }
            if let Some(list) = st.group_lists.get_mut(&0) {
                list[idx].reinit();
            }
        }
    }
}

fn stage_wipe_btnselitem_lists(ctx: &mut CommandContext, st: &mut StageFormState) {
    let front = st.btnselitem_lists.get(&1).cloned().unwrap_or_default();
    let back = st.btnselitem_lists.get(&0).cloned().unwrap_or_default();

    let mut old_next = st.btnselitem_lists.remove(&2).unwrap_or_default();
    clear_btnselitem_list_for_stage_wipe(ctx, &mut old_next, 2);
    let next_copy = clone_btnselitem_list_for_stage_wipe(ctx, st, 2, &front);

    let front_copy = clone_btnselitem_list_for_stage_wipe(ctx, st, 1, &back);

    let mut old_front = st.btnselitem_lists.remove(&1).unwrap_or_default();
    clear_btnselitem_list_for_stage_wipe(ctx, &mut old_front, 1);

    let mut old_back = st.btnselitem_lists.remove(&0).unwrap_or_default();
    clear_btnselitem_list_for_stage_wipe(ctx, &mut old_back, 0);

    st.btnselitem_lists.insert(2, next_copy);
    st.btnselitem_lists.insert(1, front_copy);
    st.btnselitem_lists.insert(0, Vec::new());
}

fn stage_wipe_world_lists(
    st: &mut StageFormState,
    begin_order: i32,
    end_order: i32,
    begin_layer: i32,
    end_layer: i32,
) {
    let front_len = st.world_lists.get(&1).map(|v| v.len()).unwrap_or(0);
    extend_stage_world_list_at_least(st, 0, front_len);
    extend_stage_world_list_at_least(st, 2, front_len);

    for idx in 0..front_len {
        let Some(front) = st.world_lists.get(&1).and_then(|list| list.get(idx)).cloned() else {
            continue;
        };
        if sorter_in_range(
            front.order as i64,
            front.layer as i64,
            begin_order,
            begin_layer,
            end_order,
            end_layer,
        ) {
            let back = st
                .world_lists
                .get(&0)
                .and_then(|list| list.get(idx))
                .cloned()
                .unwrap_or_else(|| WorldState::new(idx as i32));
            if let Some(list) = st.world_lists.get_mut(&2) {
                list[idx] = front.clone();
            }
            if front.wipe_copy == 0 || back.wipe_erase == 1 {
                if let Some(list) = st.world_lists.get_mut(&1) {
                    list[idx] = back;
                }
                if let Some(list) = st.world_lists.get_mut(&0) {
                    list[idx].reinit();
                }
            }
        }
    }
}


fn stage_wipe_effect_lists(st: &mut StageFormState) {
    let front_len = st.effect_lists.get(&1).map(|v| v.len()).unwrap_or(0);
    extend_stage_effect_list_at_least(st, 0, front_len);
    extend_stage_effect_list_at_least(st, 2, front_len);
    for idx in 0..front_len {
        let front = st
            .effect_lists
            .get(&1)
            .and_then(|list| list.get(idx))
            .cloned()
            .unwrap_or_default();
        let back = st
            .effect_lists
            .get(&0)
            .and_then(|list| list.get(idx))
            .cloned()
            .unwrap_or_default();
        if let Some(list) = st.effect_lists.get_mut(&2) {
            list[idx] = front.clone();
        }
        if front.wipe_copy == 0 || back.wipe_erase == 1 {
            if let Some(list) = st.effect_lists.get_mut(&1) {
                list[idx] = back;
            }
            if let Some(list) = st.effect_lists.get_mut(&0) {
                list[idx].reinit();
            }
        }
    }
}

fn stage_wipe_quake_lists(st: &mut StageFormState) {
    let front_len = st.quake_lists.get(&1).map(|v| v.len()).unwrap_or(0);
    extend_stage_quake_list_at_least(st, 0, front_len);
    extend_stage_quake_list_at_least(st, 2, front_len);
    for idx in 0..front_len {
        let front = st
            .quake_lists
            .get(&1)
            .and_then(|list| list.get(idx))
            .cloned()
            .unwrap_or_default();
        let back = st
            .quake_lists
            .get(&0)
            .and_then(|list| list.get(idx))
            .cloned()
            .unwrap_or_default();
        if let Some(list) = st.quake_lists.get_mut(&2) {
            list[idx] = front;
        }
        if let Some(list) = st.quake_lists.get_mut(&1) {
            list[idx] = back;
        }
        if let Some(list) = st.quake_lists.get_mut(&0) {
            list[idx].reinit();
        }
    }
}

pub fn apply_stage_wipe(
    ctx: &mut CommandContext,
    begin_order: i32,
    end_order: i32,
    begin_layer: i32,
    end_layer: i32,
) {
    let form_id = ctx.ids.form_global_stage;
    with_stage_state(ctx, form_id, |ctx, st| {
        stage_wipe_object_lists(ctx, st, begin_order, end_order, begin_layer, end_layer);
        stage_wipe_mwnd_lists(ctx, st, begin_order, end_order, begin_layer, end_layer);
        stage_wipe_group_lists(st, begin_order, end_order, begin_layer, end_layer);
        stage_wipe_btnselitem_lists(ctx, st);
        stage_wipe_world_lists(st, begin_order, end_order, begin_layer, end_layer);
        stage_wipe_effect_lists(st);
        stage_wipe_quake_lists(st);
    });
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
    {
        let entry = st.mwnd_lists.entry(stage_idx).or_default();
        if entry.len() <= mwnd_idx {
            entry.resize_with(mwnd_idx + 1, MwndState::default);
        }
    }
    let initialized = st
        .mwnd_lists
        .get(&stage_idx)
        .and_then(|list| list.get(mwnd_idx))
        .map(|m| m.initialized_from_gameexe)
        .unwrap_or(false);
    if initialized {
        return;
    }

    let fallback_waku_no = if let Some(t) = ctx.tables.mwnd_templates.get(mwnd_idx).cloned() {
        if let Some(list) = st.mwnd_lists.get_mut(&stage_idx) {
            if let Some(m) = list.get_mut(mwnd_idx) {
                m.order = ctx.tables.mwnd_render.order;
                m.mwnd_extend_type = t.extend_type;
                m.window_pos = Some(t.window_pos);
                m.window_size = (t.window_size.0 > 0 && t.window_size.1 > 0).then_some(t.window_size);
                m.message_pos = Some(t.message_pos);
                m.message_margin = Some(t.message_margin);
                m.window_moji_cnt = (t.moji_cnt.0 > 0 && t.moji_cnt.1 > 0).then_some(t.moji_cnt);
                m.moji_size = (t.moji_size > 0).then_some(t.moji_size);
                m.moji_space = Some(t.moji_space);
                m.moji_color = non_negative_color_no(t.moji_color);
                m.shadow_color = non_negative_color_no(t.shadow_color);
                m.fuchi_color = non_negative_color_no(t.fuchi_color);
                m.name_moji_color = non_negative_color_no(t.name_moji_color);
                m.name_shadow_color = non_negative_color_no(t.name_shadow_color);
                m.name_fuchi_color = non_negative_color_no(t.name_fuchi_color);
                m.open_anime_type = t.open_anime_type;
                m.open_anime_time = t.open_anime_time;
                m.close_anime_type = t.close_anime_type;
                m.close_anime_time = t.close_anime_time;
            }
        }
        Some(t.waku_no)
    } else {
        None
    };

    if let Some(waku_no) = fallback_waku_no {
        apply_mwnd_waku_from_gameexe(ctx, st, stage_idx, mwnd_idx, Some(waku_no));
    }

    if let Some(list) = st.mwnd_lists.get_mut(&stage_idx) {
        if let Some(m) = list.get_mut(mwnd_idx) {
            m.initialized_from_gameexe = true;
        }
    }
}

fn apply_mwnd_waku_template_fields(
    m: &mut crate::runtime::globals::MwndState,
    waku: &crate::runtime::tables::WakuTemplate,
) {
    m.waku_file = waku.waku_file.clone();
    m.filter_file = waku.filter_file.clone();
    m.filter_margin = Some(waku.filter_margin);
    m.filter_color = Some(waku.filter_color);
    m.filter_config_color = waku.filter_config_color;
    m.filter_config_tr = waku.filter_config_tr;
    m.waku_extend_type = waku.extend_type;
    m.icon_no = waku.icon_no;
    m.page_icon_no = waku.page_icon_no;
    m.icon_pos_type = waku.icon_pos_type;
    m.icon_pos_base = waku.icon_pos_base;
    m.icon_pos = Some(waku.icon_pos);
    m.waku_button_layout = waku
        .buttons
        .iter()
        .map(|b| (b.pos_base, b.pos.0, b.pos.1))
        .collect();
    m.waku_face_pos = waku.face_pos.clone();
}

fn clear_mwnd_waku_template_fields(m: &mut crate::runtime::globals::MwndState) {
    m.waku_file.clear();
    m.filter_file.clear();
    m.filter_margin = None;
    m.filter_color = None;
    m.filter_config_color = false;
    m.filter_config_tr = false;
    m.waku_extend_type = 0;
    m.icon_no = -1;
    m.page_icon_no = -1;
    m.key_icon_appear = false;
    m.key_icon_pos = None;
    m.key_icon_mode = 0;
    m.icon_pos_type = 0;
    m.icon_pos_base = 0;
    m.icon_pos = None;
    m.waku_button_layout.clear();
    m.waku_face_pos.clear();
}

fn create_mwnd_face_object(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    mwnd_idx: usize,
    face_idx: usize,
    file_name: &str,
    obj: &mut ObjectState,
) {
    let slot_key = format!("mwnd_waku_face_{mwnd_idx}_{face_idx}");
    let slot = next_embedded_object_slot(st, stage_idx, &slot_key);
    let clear_slot = obj.nested_runtime_slot.unwrap_or(slot);
    object_clear_backend(ctx, obj, stage_idx, clear_slot);
    obj.nested_runtime_slot = Some(slot);
    obj.init_type_like();
    obj.init_param_like();

    if file_name.is_empty() {
        return;
    }

    let create_result = {
        let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
        gfx.object_create(
            images,
            layers,
            stage_idx,
            slot as i64,
            file_name,
            1,
            0,
            0,
            0,
        )
    };
    if let Err(err) = create_result {
        ctx.unknown.record_note(&format!(
            "MWND.WAKU.FACE.CREATE.failed:stage={stage_idx}:mwnd={mwnd_idx}:face={face_idx}:file={file_name}:{err}"
        ));
    }
    hide_embedded_gfx_backing(ctx, stage_idx, slot);

    obj.used = true;
    obj.backend = ObjectBackend::Gfx;
    obj.object_type = 2;
    obj.file_name = Some(file_name.to_string());
    obj.string_value = None;
    obj.base.disp = 1;
    obj.base.x = 0;
    obj.base.y = 0;
    obj.base.patno = 0;
    obj.base.layer = ctx.tables.mwnd_render.face_layer_rep;
    mark_cgtable_look_from_object_create(&mut ctx.tables, ctx.globals.cg_table_off, file_name);
}

fn create_mwnd_template_button_object(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    mwnd_idx: usize,
    btn_idx: usize,
    button: &crate::runtime::tables::WakuButtonTemplate,
    obj: &mut ObjectState,
) {
    if button.file_name.is_empty() {
        return;
    }
    let slot_key = format!("mwnd_waku_button_{mwnd_idx}_{btn_idx}");
    let slot = next_embedded_object_slot(st, stage_idx, &slot_key);

    // Original C_elm_mwnd_waku owns m_btn_list internally. These buttons are
    // frame/rendered only through the MWND waku tree, not as STAGE.OBJECT
    // top-level entries. Allocate a graphics runtime slot, but do not resize or
    // expose the stage object list here.
    let clear_slot = obj.nested_runtime_slot.unwrap_or(slot);
    object_clear_backend(ctx, obj, stage_idx, clear_slot);
    obj.nested_runtime_slot = Some(slot);
    obj.init_type_like();
    obj.init_param_like();

    let patno = button.cut_no.max(0);
    let create_result = {
        let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
        gfx.object_create(
            images,
            layers,
            stage_idx,
            slot as i64,
            &button.file_name,
            1,
            0,
            0,
            patno,
        )
    };
    if let Err(err) = create_result {
        ctx.unknown.record_note(&format!(
            "MWND.WAKU.BTN.CREATE.failed:stage={stage_idx}:mwnd={mwnd_idx}:button={btn_idx}:file={}:patno={patno}:{err}",
            button.file_name
        ));
    }
    hide_embedded_gfx_backing(ctx, stage_idx, slot);

    obj.used = true;
    obj.backend = ObjectBackend::Gfx;
    obj.object_type = 2;
    obj.file_name = Some(button.file_name.clone());
    obj.string_value = None;
    obj.base.disp = 1;
    obj.base.x = 0;
    obj.base.y = 0;
    obj.base.patno = patno;
    obj.base.layer = ctx.tables.mwnd_render.moji_layer_rep;
    obj.button.enabled = true;
    obj.button.button_no = btn_idx as i64;
    obj.button.group_no = -1;
    obj.button.action_no = button.action_no;
    obj.button.se_no = button.se_no;
    obj.button.sys_type = button.sys_type;
    obj.button.sys_type_opt = button.sys_type_opt;
    obj.button.mode = button.btn_mode;
    obj.button.state = 0;
    obj.button.hit = false;
    obj.button.pushed = false;
    obj.button.decided_action_scn_name = button.scn_name.clone();
    obj.button.decided_action_cmd_name = button.cmd_name.clone();
    obj.button.decided_action_z_no = button.z_no;
    obj.frame_action = ObjectFrameActionState::default();
    if !button.frame_action_cmd_name.is_empty() {
        obj.frame_action.scn_name = button.frame_action_scn_name.clone();
        obj.frame_action.cmd_name = button.frame_action_cmd_name.clone();
        obj.frame_action.end_time = -1;
        obj.frame_action.real_time_flag = false;
        obj.frame_action.end_flag = false;
        obj.frame_action.counter.start();
    }
    mark_cgtable_look_from_object_create(&mut ctx.tables, ctx.globals.cg_table_off, &button.file_name);
}

fn apply_mwnd_waku_from_gameexe(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    mwnd_idx: usize,
    requested_waku_no: Option<i64>,
) {
    let fallback = ctx
        .tables
        .mwnd_templates
        .get(mwnd_idx)
        .map(|t| t.waku_no)
        .unwrap_or(-1);
    let waku_no = requested_waku_no.unwrap_or(fallback);
    let waku_no = if waku_no < 0 { fallback } else { waku_no };
    let Some(waku) = (waku_no >= 0)
        .then_some(waku_no as usize)
        .and_then(|idx| ctx.tables.waku_templates.get(idx))
        .cloned()
    else {
        if let Some(list) = st.mwnd_lists.get_mut(&stage_idx) {
            if let Some(m) = list.get_mut(mwnd_idx) {
                clear_mwnd_waku_template_fields(m);
            }
        }
        return;
    };

    let mut button_list = {
        let list = st.mwnd_lists.get_mut(&stage_idx).unwrap();
        let m = &mut list[mwnd_idx];
        apply_mwnd_waku_template_fields(m, &waku);
        if m.button_list.len() < waku.buttons.len() {
            m.button_list.resize_with(waku.buttons.len(), ObjectState::default);
        }
        if m.face_list.len() < waku.face_pos.len() {
            m.face_list.resize_with(waku.face_pos.len(), ObjectState::default);
        }
        if m.object_list.len() < waku.object_cnt {
            m.object_list.resize_with(waku.object_cnt, ObjectState::default);
        }
        std::mem::take(&mut m.button_list)
    };

    for (btn_idx, button) in waku.buttons.iter().enumerate() {
        if btn_idx >= button_list.len() {
            button_list.resize_with(btn_idx + 1, ObjectState::default);
        }
        create_mwnd_template_button_object(
            ctx,
            st,
            stage_idx,
            mwnd_idx,
            btn_idx,
            button,
            &mut button_list[btn_idx],
        );
    }

    if let Some(list) = st.mwnd_lists.get_mut(&stage_idx) {
        if let Some(m) = list.get_mut(mwnd_idx) {
            m.button_list = button_list;
        }
    }
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

fn hide_embedded_gfx_backing(ctx: &mut CommandContext, stage_idx: i64, runtime_slot: usize) {
    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
    let _ = gfx.object_set_disp(images, layers, stage_idx, runtime_slot as i64, 0);
}

fn next_embedded_object_slot(st: &mut StageFormState, stage_idx: i64, key: &str) -> usize {
    const EMBEDDED_OBJECT_SLOT_BASE: usize = 200_000;

    let full = format!("{stage_idx}:{key}");
    if let Some(&v) = st.embedded_object_slots.get(&full) {
        return v;
    }

    let next_entry = st
        .next_embedded_object_slot
        .entry(stage_idx)
        .or_insert(EMBEDDED_OBJECT_SLOT_BASE);
    if *next_entry < EMBEDDED_OBJECT_SLOT_BASE {
        *next_entry = EMBEDDED_OBJECT_SLOT_BASE;
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

            resize_stage_object_list_like_cpp(ctx, st, stage_idx, n);
            ctx.stack.push(Value::Int(0));
            true
        }
        ObjectListOpKind::Unknown => false,
    }
}

fn dispatch_embedded_object_list_op(
    ctx: &mut CommandContext,
    stage_idx: i64,
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
            clear_embedded_object_list_tail(ctx, list, stage_idx, n);
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



fn dispatch_embedded_object_item_ref(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    list: &mut Vec<ObjectState>,
    strict: bool,
    child_idx: i64,
    ret_form: Option<i64>,
    al_id: Option<i64>,
    slot_key: &str,
    element_prefix: Vec<i32>,
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

    let indexed_slot_key = format!("{slot_key}_{idx}");
    let allocated_runtime_slot = next_embedded_object_slot(st, stage_idx, &indexed_slot_key);
    let runtime_slot = list[idx].nested_runtime_slot.unwrap_or(allocated_runtime_slot);
    sg_mwnd_object_trace(format!(
        "embedded_item_op resolved idx={} runtime_slot={} allocated_runtime_slot={} indexed_slot_key={} before_child used={} type={} backend={:?} file={} child_len={} nested_slot={:?}",
        idx,
        runtime_slot,
        allocated_runtime_slot,
        indexed_slot_key,
        list[idx].used,
        list[idx].object_type,
        list[idx].backend,
        list[idx].file_name.as_deref().unwrap_or("-"),
        list[idx].runtime.child_objects.len(),
        list[idx].nested_runtime_slot
    ));
    if list[idx].nested_runtime_slot.is_none() {
        list[idx].nested_runtime_slot = Some(runtime_slot);
    }

    ctx.globals.current_stage_object = Some((stage_idx, runtime_slot));
    ctx.globals.current_object_chain = Some(element_prefix.clone());

    if al_id == Some(1) {
        ctx.stack.push(Value::Int(0));
    } else {
        ctx.stack.push(Value::Element(element_prefix));
    }
    true
}

fn dispatch_embedded_object_child_item_op(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    parent: &mut ObjectState,
    parent_runtime_slot: usize,
    child_idx: i64,
    child_op: i32,
    child_tail: &[i32],
    script_args: &[Value],
    ret_form: Option<i64>,
    rhs: Option<&Value>,
    al_id: Option<i64>,
    parent_prefix: Option<Vec<i32>>,
) -> bool {
    if child_idx < 0 {
        match ret_form {
            Some(rf) => ctx.stack.push(default_for_ret_form(rf)),
            None => ctx.stack.push(Value::Int(0)),
        }
        return true;
    }

    let child_u = child_idx as usize;
    if parent.runtime.child_objects.len() <= child_u {
        parent
            .runtime
            .child_objects
            .resize_with(child_u + 1, ObjectState::default);
    }
    parent.used = true;
    if !parent.has_int_prop(ctx.ids.obj_disp) {
        parent.set_int_prop(&ctx.ids, ctx.ids.obj_disp, 1);
    }

    let child_runtime_slot = nested_object_slot(st, stage_idx, &mut parent.runtime.child_objects[child_u]);
    if !parent.runtime.child_objects[child_u].has_int_prop(ctx.ids.obj_disp) {
        parent.runtime.child_objects[child_u].set_int_prop(&ctx.ids, ctx.ids.obj_disp, 1);
    }
    parent.runtime.child_objects[child_u].used = true;

    let scratch_slot = 0usize;
    {
        let stage_list = st.object_lists.entry(stage_idx).or_default();
        if stage_list.len() <= scratch_slot {
            stage_list.resize_with(scratch_slot + 1, ObjectState::default);
        }
    }

    let child_snapshot = std::mem::take(&mut parent.runtime.child_objects[child_u]);
    let slot_snapshot = {
        let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
        std::mem::replace(&mut stage_list[scratch_slot], child_snapshot)
    };

    let prev_chain = ctx.globals.current_object_chain.clone();
    let prev_stage_object = ctx.globals.current_stage_object;
    if let Some(mut prefix) = parent_prefix {
        prefix.push(crate::runtime::forms::codes::elm_value::OBJECT_CHILD);
        prefix.push(ctx.ids.elm_array);
        prefix.push(child_u as i32);
        ctx.globals.current_object_chain = Some(prefix);
    }
    ctx.globals.current_stage_object = Some((stage_idx, child_runtime_slot));

    sg_mwnd_object_trace(format!(
        "embedded_child_direct enter parent_slot={} child_idx={} child_runtime_slot={} child_op={} child_tail={:?}",
        parent_runtime_slot,
        child_u,
        child_runtime_slot,
        child_op,
        child_tail
    ));

    let handled = dispatch_object_op(
        ctx,
        st,
        stage_idx,
        scratch_slot as i64,
        child_op,
        child_tail,
        script_args,
        ret_form,
        rhs,
        al_id,
    );

    ctx.globals.current_object_chain = prev_chain;
    ctx.globals.current_stage_object = prev_stage_object;

    let mut child_after = {
        let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
        std::mem::replace(&mut stage_list[scratch_slot], slot_snapshot)
    };
    child_after.nested_runtime_slot = Some(child_runtime_slot);

    sg_mwnd_object_trace(format!(
        "embedded_child_direct exit parent_slot={} child_idx={} child_runtime_slot={} handled={} after_child used={} type={} backend={:?} file={} disp={} pos=({}, {}) tr={} alpha={} nested_slot={:?}",
        parent_runtime_slot,
        child_u,
        child_runtime_slot,
        handled,
        child_after.used,
        child_after.object_type,
        child_after.backend,
        child_after.file_name.as_deref().unwrap_or("-"),
        child_after.get_int_prop(&ctx.ids, ctx.ids.obj_disp),
        child_after.get_int_prop(&ctx.ids, ctx.ids.obj_x),
        child_after.get_int_prop(&ctx.ids, ctx.ids.obj_y),
        child_after.get_int_prop(&ctx.ids, ctx.ids.obj_tr),
        child_after.get_int_prop(&ctx.ids, ctx.ids.obj_alpha),
        child_after.nested_runtime_slot
    ));

    parent.runtime.child_objects[child_u] = child_after;
    handled
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
    element_prefix: Option<Vec<i32>>,
) -> bool {
    let trace_prefix = element_prefix.clone();
    sg_mwnd_object_trace(format!(
        "embedded_item_op enter stage={} list_len={} strict={} child_idx={} op={} tail={:?} al_id={:?} ret_form={:?} args={:?} rhs={:?} slot_key={} prefix={:?}",
        stage_idx,
        list.len(),
        strict,
        child_idx,
        op,
        tail,
        al_id,
        ret_form,
        script_args,
        rhs,
        slot_key,
        trace_prefix
    ));
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
    let indexed_slot_key = format!("{slot_key}_{idx}");
    let allocated_runtime_slot = next_embedded_object_slot(st, stage_idx, &indexed_slot_key);
    let runtime_slot = list[idx].nested_runtime_slot.unwrap_or(allocated_runtime_slot);
    sg_mwnd_object_trace(format!(
        "embedded_item_op resolved idx={} runtime_slot={} allocated_runtime_slot={} indexed_slot_key={} before_child used={} type={} backend={:?} file={} child_len={} nested_slot={:?}",
        idx,
        runtime_slot,
        allocated_runtime_slot,
        indexed_slot_key,
        list[idx].used,
        list[idx].object_type,
        list[idx].backend,
        list[idx].file_name.as_deref().unwrap_or("-"),
        list[idx].runtime.child_objects.len(),
        list[idx].nested_runtime_slot
    ));

    if op == crate::runtime::forms::codes::elm_value::OBJECT_CHILD
        && tail.len() >= 3
        && (tail[0] == -1 || tail[0] == ctx.ids.elm_array || tail[0] == super::codes::ELM_ARRAY)
    {
        if list[idx].nested_runtime_slot.is_none() {
            list[idx].nested_runtime_slot = Some(runtime_slot);
        }
        let nested_child_idx = tail[1] as i64;
        let nested_child_op = tail[2];
        let nested_child_tail = &tail[3..];
        return dispatch_embedded_object_child_item_op(
            ctx,
            st,
            stage_idx,
            &mut list[idx],
            runtime_slot,
            nested_child_idx,
            nested_child_op,
            nested_child_tail,
            script_args,
            ret_form,
            rhs,
            al_id,
            element_prefix,
        );
    }

    // Embedded MWND/BTNSELITEM objects are not C_elm_stage::m_obj_list entries.
    // This scratch cell adapts the existing OBJECT dispatcher without exposing
    // the embedded object as a normal stage object. Renderer operations use
    // ObjectState::nested_runtime_slot, not scratch_slot.
    let scratch_slot = 0usize;
    {
        let stage_list = st.object_lists.entry(stage_idx).or_default();
        if stage_list.len() <= scratch_slot {
            stage_list.resize_with(scratch_slot + 1, ObjectState::default);
        }
    }
    let mut child_snapshot = std::mem::take(&mut list[idx]);
    if child_snapshot.nested_runtime_slot.is_none() {
        child_snapshot.nested_runtime_slot = Some(runtime_slot);
    }
    let slot_snapshot = {
        let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
        std::mem::take(&mut stage_list[scratch_slot])
    };
    {
        let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
        stage_list[scratch_slot] = child_snapshot;
    }
    let prev_chain = ctx.globals.current_object_chain.clone();
    let prev_stage_object = ctx.globals.current_stage_object;
    if let Some(prefix) = element_prefix {
        ctx.globals.current_object_chain = Some(prefix);
    }
    ctx.globals.current_stage_object = Some((stage_idx, scratch_slot));
    let handled = dispatch_object_op(
        ctx,
        st,
        stage_idx,
        scratch_slot as i64,
        op,
        tail,
        script_args,
        ret_form,
        rhs,
        al_id,
    );
    sg_mwnd_object_trace(format!(
        "embedded_item_op dispatched idx={} runtime_slot={} scratch_slot={} handled={} op={} tail={:?} current_chain_after_dispatch={:?} current_stage_object_after_dispatch={:?}",
        idx,
        runtime_slot,
        scratch_slot,
        handled,
        op,
        tail,
        ctx.globals.current_object_chain,
        ctx.globals.current_stage_object
    ));
    ctx.globals.current_object_chain = prev_chain;
    ctx.globals.current_stage_object = prev_stage_object;
    let mut child_after = {
        let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
        std::mem::take(&mut stage_list[scratch_slot])
    };
    if child_after.nested_runtime_slot.is_none() {
        child_after.nested_runtime_slot = Some(runtime_slot);
    }
    sg_mwnd_object_trace(format!(
        "embedded_item_op exit idx={} runtime_slot={} handled={} after_child used={} type={} backend={:?} file={} disp={} pos=({}, {}) tr={} alpha={} child_len={} nested_slot={:?}",
        idx,
        runtime_slot,
        handled,
        child_after.used,
        child_after.object_type,
        child_after.backend,
        child_after.file_name.as_deref().unwrap_or("-"),
        child_after.get_int_prop(&ctx.ids, ctx.ids.obj_disp),
        child_after.get_int_prop(&ctx.ids, ctx.ids.obj_x),
        child_after.get_int_prop(&ctx.ids, ctx.ids.obj_y),
        child_after.get_int_prop(&ctx.ids, ctx.ids.obj_tr),
        child_after.get_int_prop(&ctx.ids, ctx.ids.obj_alpha),
        child_after.runtime.child_objects.len(),
        child_after.nested_runtime_slot
    ));
    {
        let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
        stage_list[scratch_slot] = slot_snapshot;
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
        }
        | ObjectBackend::Weather {
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

pub(crate) fn resolve_capture_file_path(
    project_dir: &Path,
    append_dir: &str,
    raw: &str,
) -> Option<PathBuf> {
    let raw_path = Path::new(raw);
    let mut candidates = Vec::new();
    if raw_path.is_absolute() {
        candidates.push(raw_path.to_path_buf());
    } else {
        candidates.push(project_dir.join(raw_path));
        candidates.push(project_dir.join("savedata").join(raw_path));
        candidates.push(project_dir.join("save").join(raw_path));
        candidates.push(project_dir.join("dat").join(raw_path));
        if !append_dir.is_empty() {
            let append = Path::new(append_dir);
            if append.is_absolute() {
                candidates.push(append.join(raw_path));
            } else {
                candidates.push(project_dir.join(append).join(raw_path));
            }
        }
    }
    let mut expanded = Vec::new();
    for base in candidates {
        expanded.push(base.clone());
        if base.extension().is_none() {
            expanded.push(base.with_extension("png"));
            expanded.push(base.with_extension("bmp"));
            expanded.push(base.with_extension("jpg"));
            expanded.push(base.with_extension("jpeg"));
        }
    }
    expanded.into_iter().find(|p| p.is_file())
}

const TNM_SCALE_UNIT: i64 = 1000;
const TNM_SCREEN_RATE: i64 = 1;
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

fn overload_at_least(
    al_id: Option<i64>,
    positional_len: usize,
    level: i64,
    required_args: usize,
) -> bool {
    al_id.unwrap_or(-1) >= level || positional_len >= required_args
}

fn positional_ref_i64(pos: &[&Value], index: usize, default: i64) -> i64 {
    pos.get(index).and_then(|v| v.as_i64()).unwrap_or(default)
}

fn script_i64(args: &[Value], index: usize, default: i64) -> i64 {
    args.get(index).and_then(as_i64).unwrap_or(default)
}

fn object_event_list_default(ids: &crate::runtime::constants::RuntimeConstants, op: i32) -> i32 {
    if (ids.obj_tr_rep != 0 && op == ids.obj_tr_rep)
        || (ids.obj_tr_rep_eve != 0 && op == ids.obj_tr_rep_eve)
    {
        255
    } else {
        0
    }
}

fn resolve_object_movie_path(
    project_dir: &Path,
    append_dir: &str,
    file_name: &str,
) -> Option<PathBuf> {
    crate::resource::find_mov_path_with_append_dir(project_dir, append_dir, file_name)
        .ok()
        .map(|(path, _)| path)
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
    ctx.movie.prepare(file).ok().and_then(|info| info.duration_ms())
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

fn sample_image_component(
    ctx: &CommandContext,
    image_id: ImageId,
    x: i64,
    y: i64,
    channel: usize,
) -> i64 {
    if x < 0 || y < 0 || channel >= 4 {
        return 0;
    }
    let Some(img) = ctx.images.get(image_id) else {
        return 0;
    };
    let xi = x as u32;
    let yi = y as u32;
    if xi >= img.width || yi >= img.height {
        return 0;
    }
    let idx = ((yi * img.width + xi) * 4) as usize + channel;
    img.rgba.get(idx).copied().unwrap_or(0) as i64
}

fn sample_sprite_component(
    ctx: &CommandContext,
    layer_id: LayerId,
    sprite_id: SpriteId,
    x: i64,
    y: i64,
    channel: usize,
) -> i64 {
    let Some(image_id) = ctx
        .layers
        .layer(layer_id)
        .and_then(|layer| layer.sprite(sprite_id))
        .and_then(|spr| spr.image_id)
    else {
        return 0;
    };
    sample_image_component(ctx, image_id, x, y, channel)
}

fn sample_object_pixel_component(
    ctx: &mut CommandContext,
    obj: &ObjectState,
    stage_idx: i64,
    obj_idx: usize,
    x: i64,
    y: i64,
    cut_no: i64,
    channel: usize,
) -> i64 {
    if x < 0 || y < 0 || cut_no < 0 {
        return 0;
    }

    match &obj.backend {
        ObjectBackend::Movie {
            image_id: Some(id), ..
        } if obj.object_type == 9 && cut_no == 0 => {
            return sample_image_component(ctx, *id, x, y, channel);
        }
        ObjectBackend::Rect {
            layer_id,
            sprite_id,
            ..
        } if matches!(obj.object_type, 8 | 10 | 11) && cut_no == 0 => {
            return sample_sprite_component(ctx, *layer_id, *sprite_id, x, y, channel);
        }
        _ => {}
    }

    if !matches!(obj.object_type, 2 | 5 | 8 | 9 | 10 | 11) {
        return 0;
    }

    let file = obj
        .file_name
        .as_deref()
        .map(str::to_string)
        .or_else(|| ctx.gfx.object_peek_file(stage_idx, obj_idx as i64));
    let Some(file) = file else {
        return 0;
    };

    let Ok((path, _pct)) = crate::resource::find_g00_image_with_append_dir(
        ctx.images.project_dir(),
        &ctx.globals.append_dir,
        &file,
    ) else {
        return 0;
    };
    let Ok(id) = ctx.images.load_file(&path, cut_no as usize) else {
        return 0;
    };
    sample_image_component(ctx, id, x, y, channel)
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

    let disp = obj.lookup_int_prop(&ctx.ids, ctx.ids.obj_disp).unwrap_or(0) != 0;
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

fn duplicate_sprite_to_layer(
    ctx: &mut CommandContext,
    src_layer_id: LayerId,
    src_sprite_id: SpriteId,
    dst_layer_id: LayerId,
) -> Option<SpriteId> {
    let src_sprite = ctx
        .layers
        .layer(src_layer_id)
        .and_then(|layer| layer.sprite(src_sprite_id))
        .cloned()?;
    let dst_sprite_id = {
        let dst_layer = ctx.layers.layer_mut(dst_layer_id)?;
        let sid = dst_layer.create_sprite();
        if let Some(dst_sprite) = dst_layer.sprite_mut(sid) {
            *dst_sprite = src_sprite;
        }
        sid
    };
    Some(dst_sprite_id)
}

fn duplicate_object_backend_for_copy(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    backend: &ObjectBackend,
) -> ObjectBackend {
    match backend {
        ObjectBackend::Rect {
            layer_id,
            sprite_id,
            width,
            height,
        } => {
            let dst_layer_id = ensure_rect_layer(ctx, st, stage_idx);
            duplicate_sprite_to_layer(ctx, *layer_id, *sprite_id, dst_layer_id)
                .map(|sid| ObjectBackend::Rect {
                    layer_id: dst_layer_id,
                    sprite_id: sid,
                    width: *width,
                    height: *height,
                })
                .unwrap_or(ObjectBackend::None)
        }
        ObjectBackend::String {
            layer_id,
            sprite_id,
            width,
            height,
        } => {
            let dst_layer_id = ensure_rect_layer(ctx, st, stage_idx);
            duplicate_sprite_to_layer(ctx, *layer_id, *sprite_id, dst_layer_id)
                .map(|sid| ObjectBackend::String {
                    layer_id: dst_layer_id,
                    sprite_id: sid,
                    width: *width,
                    height: *height,
                })
                .unwrap_or(ObjectBackend::None)
        }
        ObjectBackend::Movie {
            layer_id,
            sprite_id,
            image_id,
            width,
            height,
        } => {
            let dst_layer_id = ensure_rect_layer(ctx, st, stage_idx);
            duplicate_sprite_to_layer(ctx, *layer_id, *sprite_id, dst_layer_id)
                .map(|sid| ObjectBackend::Movie {
                    layer_id: dst_layer_id,
                    sprite_id: sid,
                    image_id: *image_id,
                    width: *width,
                    height: *height,
                })
                .unwrap_or(ObjectBackend::None)
        }
        ObjectBackend::Number {
            layer_id,
            sprite_ids,
        } => {
            let dst_layer_id = ensure_rect_layer(ctx, st, stage_idx);
            let mut copied = Vec::with_capacity(sprite_ids.len());
            for sid in sprite_ids {
                if let Some(new_sid) = duplicate_sprite_to_layer(ctx, *layer_id, *sid, dst_layer_id)
                {
                    copied.push(new_sid);
                }
            }
            ObjectBackend::Number {
                layer_id: dst_layer_id,
                sprite_ids: copied,
            }
        }
        ObjectBackend::Weather {
            layer_id,
            sprite_ids,
        } => {
            let dst_layer_id = ensure_rect_layer(ctx, st, stage_idx);
            let mut copied = Vec::with_capacity(sprite_ids.len());
            for sid in sprite_ids {
                if let Some(new_sid) = duplicate_sprite_to_layer(ctx, *layer_id, *sid, dst_layer_id)
                {
                    copied.push(new_sid);
                }
            }
            ObjectBackend::Weather {
                layer_id: dst_layer_id,
                sprite_ids: copied,
            }
        }
        ObjectBackend::Gfx | ObjectBackend::None => ObjectBackend::None,
    }
}

fn sync_special_gfx_sprite_for_object(
    ctx: &mut CommandContext,
    stage_idx: i64,
    obj_slot: usize,
    obj: &ObjectState,
) {
    let Some((lid, sid)) = ctx.gfx.object_sprite_binding(stage_idx, obj_slot as i64) else {
        return;
    };
    let Some(layer) = ctx.layers.layer_mut(lid) else {
        return;
    };
    let Some(sprite) = layer.sprite_mut(sid) else {
        return;
    };

    match obj.object_type {
        6 => {
            // Original CREATE_MESH restructures a mesh object from the object's file path.
            // The WGPU path consumes mesh_file_name/mesh_kind instead of a plain image id.
            sprite.image_id = None;
            sprite.billboard = false;
            sprite.mesh_file_name = obj.file_name.clone();
            sprite.mesh_kind = 1;
            sprite.shadow_cast = true;
            sprite.shadow_receive = true;
            sprite.camera_enabled = true;
        }
        7 => {
            // Original CREATE_BILLBOARD creates a 3D camera-facing PCT/album sprite, not a mesh.
            sprite.billboard = true;
            sprite.mesh_file_name = None;
            sprite.mesh_kind = 0;
            sprite.shadow_cast = false;
            sprite.shadow_receive = false;
            sprite.camera_enabled = true;
        }
        _ => {}
    }
}

fn object_clear_backend_recursive(
    ctx: &mut CommandContext,
    obj: &mut ObjectState,
    stage_idx: i64,
    obj_idx: usize,
) {
    object_clear_backend(ctx, obj, stage_idx, obj_idx);
    for child in &mut obj.runtime.child_objects {
        if let Some(slot) = child.nested_runtime_slot {
            object_clear_backend_recursive(ctx, child, stage_idx, slot);
        } else if !matches!(child.backend, ObjectBackend::Gfx) {
            object_clear_backend_recursive(ctx, child, stage_idx, obj_idx);
        }
    }
}

fn object_reinit_finish_free_like_cpp(
    ctx: &mut CommandContext,
    obj: &mut ObjectState,
    stage_idx: i64,
    obj_idx: usize,
) {
    object_clear_backend_recursive(ctx, obj, stage_idx, obj_idx);
    obj.runtime.child_objects.clear();
    obj.init_type_like();
    obj.init_param_like();
}

fn object_init_type_free_self_like_cpp(
    ctx: &mut CommandContext,
    obj: &mut ObjectState,
    stage_idx: i64,
    obj_idx: usize,
) {
    object_clear_backend(ctx, obj, stage_idx, obj_idx);
    obj.init_type_like();
}

fn object_dst_clip_from_props(
    ids: &crate::runtime::constants::RuntimeConstants,
    obj: &ObjectState,
) -> Option<crate::layer::ClipRect> {
    if ids.obj_clip_use != 0 && obj.get_int_prop(ids, ids.obj_clip_use) != 0 {
        Some(crate::layer::ClipRect {
            left: obj.get_int_prop(ids, ids.obj_clip_left) as i32,
            top: obj.get_int_prop(ids, ids.obj_clip_top) as i32,
            right: obj.get_int_prop(ids, ids.obj_clip_right) as i32,
            bottom: obj.get_int_prop(ids, ids.obj_clip_bottom) as i32,
        })
    } else {
        None
    }
}

fn object_src_clip_from_props(
    ids: &crate::runtime::constants::RuntimeConstants,
    obj: &ObjectState,
) -> Option<crate::layer::ClipRect> {
    if ids.obj_src_clip_use != 0 && obj.get_int_prop(ids, ids.obj_src_clip_use) != 0 {
        Some(crate::layer::ClipRect {
            left: obj.get_int_prop(ids, ids.obj_src_clip_left) as i32,
            top: obj.get_int_prop(ids, ids.obj_src_clip_top) as i32,
            right: obj.get_int_prop(ids, ids.obj_src_clip_right) as i32,
            bottom: obj.get_int_prop(ids, ids.obj_src_clip_bottom) as i32,
        })
    } else {
        None
    }
}

fn sync_object_dst_clip_backend(
    ctx: &mut CommandContext,
    obj: &ObjectState,
    stage_idx: i64,
    obj_idx: usize,
) {
    let clip = object_dst_clip_from_props(&ctx.ids, obj);
    match &obj.backend {
        ObjectBackend::Gfx => {
            let use_flag = if clip.is_some() { 1 } else { 0 };
            let left = clip.map(|c| c.left as i64).unwrap_or(0);
            let top = clip.map(|c| c.top as i64).unwrap_or(0);
            let right = clip.map(|c| c.right as i64).unwrap_or(0);
            let bottom = clip.map(|c| c.bottom as i64).unwrap_or(0);
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            let _ = gfx.object_set_clip(
                images,
                layers,
                stage_idx,
                obj_idx as i64,
                use_flag,
                left,
                top,
                right,
                bottom,
            );
        }
        ObjectBackend::Rect { layer_id, sprite_id, .. }
        | ObjectBackend::String { layer_id, sprite_id, .. }
        | ObjectBackend::Movie { layer_id, sprite_id, .. } => {
            if let Some(layer) = ctx.layers.layer_mut(*layer_id) {
                if let Some(spr) = layer.sprite_mut(*sprite_id) {
                    spr.dst_clip = clip;
                }
            }
        }
        ObjectBackend::Number { layer_id, sprite_ids }
        | ObjectBackend::Weather { layer_id, sprite_ids } => {
            if let Some(layer) = ctx.layers.layer_mut(*layer_id) {
                for &sid in sprite_ids {
                    if let Some(spr) = layer.sprite_mut(sid) {
                        spr.dst_clip = clip;
                    }
                }
            }
        }
        ObjectBackend::None => {}
    }
}

fn sync_object_src_clip_backend(
    ctx: &mut CommandContext,
    obj: &ObjectState,
    stage_idx: i64,
    obj_idx: usize,
) {
    let clip = object_src_clip_from_props(&ctx.ids, obj);
    match &obj.backend {
        ObjectBackend::Gfx => {
            let use_flag = if clip.is_some() { 1 } else { 0 };
            let left = clip.map(|c| c.left as i64).unwrap_or(0);
            let top = clip.map(|c| c.top as i64).unwrap_or(0);
            let right = clip.map(|c| c.right as i64).unwrap_or(0);
            let bottom = clip.map(|c| c.bottom as i64).unwrap_or(0);
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            let _ = gfx.object_set_src_clip(
                images,
                layers,
                stage_idx,
                obj_idx as i64,
                use_flag,
                left,
                top,
                right,
                bottom,
            );
        }
        ObjectBackend::Rect { layer_id, sprite_id, .. }
        | ObjectBackend::String { layer_id, sprite_id, .. }
        | ObjectBackend::Movie { layer_id, sprite_id, .. } => {
            if let Some(layer) = ctx.layers.layer_mut(*layer_id) {
                if let Some(spr) = layer.sprite_mut(*sprite_id) {
                    spr.src_clip = clip;
                }
            }
        }
        ObjectBackend::Number { layer_id, sprite_ids }
        | ObjectBackend::Weather { layer_id, sprite_ids } => {
            if let Some(layer) = ctx.layers.layer_mut(*layer_id) {
                for &sid in sprite_ids {
                    if let Some(spr) = layer.sprite_mut(sid) {
                        spr.src_clip = clip;
                    }
                }
            }
        }
        ObjectBackend::None => {}
    }
}

fn clear_embedded_object_list_tail(
    ctx: &mut CommandContext,
    list: &mut [ObjectState],
    stage_idx: i64,
    from_idx: usize,
) {
    for (idx, obj) in list.iter_mut().enumerate().skip(from_idx) {
        let slot = obj.runtime_slot_or(idx);
        object_clear_backend_recursive(ctx, obj, stage_idx, slot);
    }
}

fn assign_copy_runtime_slots(
    st: &mut StageFormState,
    stage_idx: i64,
    obj: &mut ObjectState,
    fixed_root_slot: Option<usize>,
) {
    obj.nested_runtime_slot = fixed_root_slot;
    for child in &mut obj.runtime.child_objects {
        child.nested_runtime_slot = None;
        nested_object_slot(st, stage_idx, child);
        let child_slot = child.nested_runtime_slot;
        assign_copy_runtime_slots(st, stage_idx, child, child_slot);
    }
}

fn duplicate_object_tree_backends_for_copy(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    stage_idx: i64,
    obj: &mut ObjectState,
    obj_slot: usize,
) {
    let src_backend = obj.backend.clone();
    let src_file = obj.file_name.clone();
    obj.backend = match src_backend {
        ObjectBackend::Gfx => {
            if let Some(file) = src_file.clone() {
                let disp = obj.get_int_prop(&ctx.ids, ctx.ids.obj_disp) != 0;
                let x = obj.get_int_prop(&ctx.ids, ctx.ids.obj_x);
                let y = obj.get_int_prop(&ctx.ids, ctx.ids.obj_y);
                let pat = obj.get_int_prop(&ctx.ids, ctx.ids.obj_patno);
                {
                    let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                    let _ = gfx.object_create(
                        images,
                        layers,
                        stage_idx,
                        obj_slot as i64,
                        &file,
                        disp as i64,
                        x,
                        y,
                        pat,
                    );
                }
                if obj.nested_runtime_slot.is_some() {
                    hide_embedded_gfx_backing(ctx, stage_idx, obj_slot);
                }
                sync_special_gfx_sprite_for_object(ctx, stage_idx, obj_slot, obj);
                ObjectBackend::Gfx
            } else {
                ObjectBackend::None
            }
        }
        other => duplicate_object_backend_for_copy(ctx, st, stage_idx, &other),
    };
    if obj.file_name.is_none() {
        obj.file_name = src_file;
    }

    for child in &mut obj.runtime.child_objects {
        if let Some(slot) = child.nested_runtime_slot {
            duplicate_object_tree_backends_for_copy(ctx, st, stage_idx, child, slot);
        }
    }
}

fn update_string_backend(
    ctx: &mut CommandContext,
    st: &mut StageFormState,
    obj: &mut ObjectState,
    stage_idx: i64,
) {
    let text = obj.string_value.clone().unwrap_or_default();
    let (font_px, max_w, max_h) = string_layout(obj);

    let disp = obj.lookup_int_prop(&ctx.ids, ctx.ids.obj_disp).unwrap_or(0) != 0;
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
            .load_for_project(&ctx.project_dir);
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
    if op == constants::elm_value::OBJECT_CREATE_STRING {
        return ObjectOpKind::CreateString;
    }
    if (ids.obj_create_copy_from != 0 && op == ids.obj_create_copy_from)
        || op == constants::elm_value::OBJECT_CREATE_COPY_FROM
    {
        return ObjectOpKind::CreateCopyFrom;
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

fn object_child_tail_to_clone(
    ctx: &CommandContext,
    obj: &ObjectState,
    tail: &[i32],
) -> Option<ObjectState> {
    if tail.len() < 2 || !(tail[0] == -1 || tail[0] == ctx.ids.elm_array || tail[0] == super::codes::ELM_ARRAY) {
        return None;
    }
    let child_idx = tail[1].max(0) as usize;
    let child = obj.runtime.child_objects.get(child_idx)?;
    if tail.len() == 2 {
        return Some(child.clone());
    }
    object_op_tail_to_clone(ctx, child, tail[2], &tail[3..])
}

fn object_op_tail_to_clone(
    ctx: &CommandContext,
    obj: &ObjectState,
    op: i32,
    tail: &[i32],
) -> Option<ObjectState> {
    if op == crate::runtime::forms::codes::elm_value::OBJECT_CHILD {
        return object_child_tail_to_clone(ctx, obj, tail);
    }
    if tail.is_empty() {
        return Some(obj.clone());
    }
    None
}

fn object_list_tail_to_clone(
    ctx: &CommandContext,
    list: &[ObjectState],
    tail: &[i32],
) -> Option<ObjectState> {
    if tail.len() < 2 || !(tail[0] == -1 || tail[0] == ctx.ids.elm_array || tail[0] == super::codes::ELM_ARRAY) {
        return None;
    }
    let idx = tail[1].max(0) as usize;
    let obj = list.get(idx)?;
    if tail.len() == 2 {
        return Some(obj.clone());
    }
    object_op_tail_to_clone(ctx, obj, tail[2], &tail[3..])
}

fn embedded_object_list_for_selector<'a>(
    ctx: &CommandContext,
    st: &'a StageFormState,
    stage: i64,
    child: i32,
    idx: i64,
    op: i32,
) -> Option<&'a Vec<ObjectState>> {
    if idx < 0 {
        return None;
    }
    if child == crate::runtime::forms::codes::STAGE_ELM_MWND {
        let m = st
            .mwnd_lists
            .get(&stage)
            .and_then(|list| list.get(idx as usize))?;
        if op == crate::runtime::forms::codes::elm_value::MWND_BUTTON {
            return Some(&m.button_list);
        }
        if op == crate::runtime::forms::codes::elm_value::MWND_FACE {
            return Some(&m.face_list);
        }
        if op == crate::runtime::forms::codes::elm_value::MWND_OBJECT {
            return Some(&m.object_list);
        }
    }
    if child == crate::runtime::forms::codes::STAGE_ELM_BTNSELITEM
        && op == crate::runtime::forms::codes::ELM_BTNSELITEM_OBJECT
    {
        return st
            .btnselitem_lists
            .get(&stage)
            .and_then(|list| list.get(idx as usize))
            .map(|item| &item.object_list);
    }
    let _ = ctx;
    None
}

fn clone_object_from_element_common(
    ctx: &CommandContext,
    st: &StageFormState,
    current_stage: Option<i64>,
    current_obj_idx: Option<usize>,
    current_obj: Option<&ObjectState>,
    element: &[i32],
) -> Option<ObjectState> {
    let stage_object = if ctx.ids.stage_elm_object != 0 {
        ctx.ids.stage_elm_object
    } else {
        crate::runtime::forms::codes::STAGE_ELM_OBJECT
    };
    match parse_target(ctx, element)? {
        StageTarget::ChildItemRef { stage, child, idx } if child == stage_object && idx >= 0 => {
            if current_stage == Some(stage) && current_obj_idx == Some(idx as usize) {
                return current_obj.cloned();
            }
            st.object_lists
                .get(&stage)
                .and_then(|list| list.get(idx as usize))
                .cloned()
        }
        StageTarget::ChildItemOp {
            stage,
            child,
            idx,
            op,
            tail,
        } if child == stage_object && idx >= 0 => {
            let base = if current_stage == Some(stage) && current_obj_idx == Some(idx as usize) {
                current_obj
            } else {
                st.object_lists
                    .get(&stage)
                    .and_then(|list| list.get(idx as usize))
            }?;
            object_op_tail_to_clone(ctx, base, op as i32, &tail)
        }
        StageTarget::ChildItemOp {
            stage,
            child,
            idx,
            op,
            tail,
        } => {
            let list = embedded_object_list_for_selector(ctx, st, stage, child, idx, op as i32)?;
            object_list_tail_to_clone(ctx, list, &tail)
        }
        _ => None,
    }
}

fn clone_object_from_element_for_child_ref(
    ctx: &CommandContext,
    st: &StageFormState,
    current_stage: i64,
    current_obj_idx: usize,
    current_obj: &ObjectState,
    element: &[i32],
) -> Option<ObjectState> {
    clone_object_from_element_common(
        ctx,
        st,
        Some(current_stage),
        Some(current_obj_idx),
        Some(current_obj),
        element,
    )
}

fn clone_object_from_element_for_create_copy(
    ctx: &CommandContext,
    st: &StageFormState,
    element: &[i32],
) -> Option<ObjectState> {
    clone_object_from_element_common(ctx, st, None, None, None, element)
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
    if sg_mwnd_object_trace_enabled()
        && (op == crate::runtime::forms::codes::elm_value::OBJECT_CHILD
            || op == constants::elm_value::OBJECT_CREATE
            || op == constants::OBJECT_CREATE_RECT
            || op == constants::elm_value::OBJECT_CREATE_STRING
            || op == ctx.ids.obj_set_pos
            || op == ctx.ids.obj_x
            || op == ctx.ids.obj_y
            || op == ctx.ids.obj_disp
            || op == ctx.ids.obj_tr
            || op == ctx.ids.obj_frame_action
            || op == ctx.ids.obj_frame_action_ch)
    {
        sg_mwnd_object_trace(format!(
            "object_op enter stage={} obj={} op={} tail={:?} al_id={:?} ret_form={:?} args={:?} rhs={:?} current_chain={:?} current_stage_object={:?}",
            stage_idx,
            obj_u,
            op,
            tail,
            al_id,
            ret_form,
            script_args,
            rhs,
            ctx.globals.current_object_chain,
            ctx.globals.current_stage_object
        ));
    }
    if let Some(raw) = std::env::var_os("SG_TRACE_OBJECT_SLOT") {
        let raw = raw.to_string_lossy();
        let targets = raw
            .split(',')
            .filter_map(|s| s.trim().parse::<usize>().ok())
            .collect::<Vec<_>>();
        if targets.iter().any(|&n| n == obj_u) {
            eprintln!(
                "[SG_TRACE_OBJECT] stage={} obj={} op={} tail={:?} al_id={:?} args={:?} rhs={:?}",
                stage_idx, obj_u, op, tail, al_id, script_args, rhs
            );
        }
    }
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
    let mut copy_from_snapshot: Option<ObjectState> = None;
    if ((ctx.ids.obj_create_copy_from != 0 && op == ctx.ids.obj_create_copy_from)
        || op == constants::elm_value::OBJECT_CREATE_COPY_FROM)
        && rhs.is_none()
        && script_args.len() == 1
    {
        if let Value::Element(e) = &script_args[0] {
            copy_from_snapshot = clone_object_from_element_for_create_copy(ctx, st, e);
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
    let obj_runtime_slot = obj.runtime_slot_or(obj_u);

    fn split_frame_action_chain(
        ctx: &CommandContext,
        op: i32,
        tail: &[i32],
    ) -> (Vec<i32>, Option<Vec<i32>>) {
        let element = ctx
            .vm_call
            .as_ref()
            .map(|m| m.element.clone())
            .unwrap_or_default();
        if element.is_empty() {
            return (Vec::new(), None);
        }
        let pos = element
            .iter()
            .position(|v| *v == op)
            .unwrap_or_else(|| element.len().saturating_sub(1));
        let mut end = pos + 1;
        if op == ctx.ids.obj_frame_action_ch
            && tail.len() >= 2
            && (tail[0] == ctx.ids.elm_array || tail[0] == crate::runtime::forms::codes::ELM_ARRAY)
        {
            end = (pos + 3).min(element.len());
        }
        let frame_action_chain = element[..end].to_vec();
        let object_chain = if pos > 0 {
            Some(element[..pos].to_vec())
        } else {
            None
        };
        (frame_action_chain, object_chain)
    }

    fn queue_finish(
        ctx: &mut CommandContext,
        fa: &ObjectFrameActionState,
        frame_action_chain: Vec<i32>,
        object_chain: Option<Vec<i32>>,
    ) {
        if fa.cmd_name.is_empty() {
            return;
        }
        ctx.globals
            .pending_frame_action_finishes
            .push(PendingFrameActionFinish {
                frame_action_chain,
                object_chain,
                scn_name: fa.scn_name.clone(),
                cmd_name: fa.cmd_name.clone(),
                end_time: fa.end_time,
                args: fa.args.clone(),
            });
    }

    fn frame_action_set_from_args(
        ctx: &CommandContext,
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
        fa.scn_name = ctx.current_scene_name.clone().unwrap_or_default();
        fa.real_time_flag = real_time_flag;
        fa.end_flag = false;
        fa.counter.reset();
        if real_time_flag {
            fa.counter.start_real();
        } else {
            fa.counter.start();
        }
        fa.args = script_args.iter().skip(2).cloned().collect();
    }

    fn dispatch_object_frame_action(
        ctx: &mut CommandContext,
        fa: &mut ObjectFrameActionState,
        frame_action_chain: Vec<i32>,
        object_chain: Option<Vec<i32>>,
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
                queue_finish(ctx, fa, frame_action_chain.clone(), object_chain.clone());
                frame_action_set_from_args(ctx, fa, script_args, false);
                push_ok(ctx, ret_form);
                true
            }
            2 => {
                queue_finish(ctx, fa, frame_action_chain.clone(), object_chain.clone());
                *fa = ObjectFrameActionState::default();
                push_ok(ctx, ret_form);
                true
            }
            3 => {
                queue_finish(ctx, fa, frame_action_chain.clone(), object_chain.clone());
                frame_action_set_from_args(ctx, fa, script_args, true);
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

    if let Some(mut src) = copy_from_snapshot.take() {
        // Original C++ does p_obj->reinit(true) before p_obj->copy(src, false).
        // Clear the destination tree first, then copy source state and rebuild all
        // renderer-side resources for the destination object tree.
        let dst_runtime_slot = obj.nested_runtime_slot;
        object_clear_backend_recursive(ctx, obj, stage_idx, obj_runtime_slot);
        assign_copy_runtime_slots(st, stage_idx, &mut src, dst_runtime_slot);
        let root_slot = dst_runtime_slot.unwrap_or(obj_u);
        duplicate_object_tree_backends_for_copy(ctx, st, stage_idx, &mut src, root_slot);
        src.used = true;
        *obj = src;
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_frame_action != 0 && op == ctx.ids.obj_frame_action {
        let (frame_action_chain, object_chain) = split_frame_action_chain(ctx, op, tail);
        if dispatch_object_frame_action(
            ctx,
            &mut obj.frame_action,
            frame_action_chain,
            object_chain,
            tail,
            script_args,
            rhs,
            al_id,
            ret_form,
        ) {
            return true;
        }
    }

    if ctx.ids.obj_frame_action_ch != 0 && op == ctx.ids.obj_frame_action_ch {
        if tail.len() >= 2 && (tail[0] == ctx.ids.elm_array || tail[0] == -1) {
            let idx = tail[1].max(0) as usize;
            if obj.frame_action_ch.len() <= idx {
                obj.frame_action_ch
                    .resize_with(idx + 1, ObjectFrameActionState::default);
            }
            let (frame_action_chain, object_chain) = split_frame_action_chain(ctx, op, tail);
            if dispatch_object_frame_action(
                ctx,
                &mut obj.frame_action_ch[idx],
                frame_action_chain,
                object_chain,
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
        if !obj.has_int_prop(ctx.ids.obj_disp) {
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, 1);
        }
        if tail.len() == 2 && (tail[0] == -1 || tail[0] == ctx.ids.elm_array) {
            let child_idx = tail[1].max(0) as usize;
            if obj.runtime.child_objects.len() <= child_idx {
                obj.runtime
                    .child_objects
                    .resize_with(child_idx + 1, ObjectState::default);
            }
            let slot = nested_object_slot(st, stage_idx, &mut obj.runtime.child_objects[child_idx]);
            if !obj.runtime.child_objects[child_idx].has_int_prop(ctx.ids.obj_disp) {
                obj.runtime.child_objects[child_idx].set_int_prop(&ctx.ids, ctx.ids.obj_disp, 1);
            }
            obj.runtime.child_objects[child_idx].used = true;

            let mut element_chain = ctx.globals.current_object_chain.clone().unwrap_or_else(|| {
                vec![
                    ctx.ids.form_global_stage as i32,
                    ctx.ids.elm_array,
                    stage_idx as i32,
                    ctx.ids.stage_elm_object,
                    ctx.ids.elm_array,
                    obj_u as i32,
                ]
            });
            element_chain.push(crate::runtime::forms::codes::elm_value::OBJECT_CHILD);
            element_chain.push(ctx.ids.elm_array);
            element_chain.push(child_idx as i32);

            ctx.globals.current_object_chain = Some(element_chain.clone());
            ctx.globals.current_stage_object = Some((stage_idx, slot));

            if al_id == Some(1) {
                if let Some(Value::Element(src_element)) = rhs.or_else(|| script_args.first()) {
                    if let Some(src_obj) = clone_object_from_element_for_child_ref(
                        ctx,
                        st,
                        stage_idx,
                        obj_u,
                        obj,
                        src_element,
                    ) {
                        let mut copied = src_obj;
                        copied.nested_runtime_slot = Some(slot);
                        obj.runtime.child_objects[child_idx] = copied;
                    }
                }
                push_ok(ctx, ret_form);
            } else {
                ctx.stack.push(Value::Element(element_chain));
            }
            return true;
        }
        if tail.len() >= 3
            && (tail[0] == -1 || tail[0] == ctx.ids.elm_array || tail[0] == super::codes::ELM_ARRAY)
        {
            let child_idx = tail[1].max(0) as usize;
            let runtime_slot = {
                let list = &mut obj.runtime.child_objects;
                if list.len() <= child_idx {
                    list.resize_with(child_idx + 1, ObjectState::default);
                }
                nested_object_slot(st, stage_idx, &mut list[child_idx])
            };

            // OBJECT.CHILD is an embedded object list in the original engine, not a
            // normal C_elm_stage::m_obj_list entry. Use the parent stage slot only
            // as a temporary dispatcher cell; renderer operations are keyed by the
            // child's nested_runtime_slot through obj_runtime_slot below.
            let scratch_slot = obj_u;
            let child_snapshot = std::mem::take(&mut obj.runtime.child_objects[child_idx]);
            let slot_snapshot = {
                let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
                std::mem::take(&mut stage_list[scratch_slot])
            };
            {
                let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
                stage_list[scratch_slot] = child_snapshot;
            }
            let prev_chain = ctx.globals.current_object_chain.clone();
            let prev_stage_object = ctx.globals.current_stage_object;
            if let Some(mut prefix) = prev_chain.clone() {
                prefix.push(crate::runtime::forms::codes::elm_value::OBJECT_CHILD);
                prefix.push(ctx.ids.elm_array);
                prefix.push(child_idx as i32);
                ctx.globals.current_object_chain = Some(prefix);
            }
            ctx.globals.current_stage_object = Some((stage_idx, scratch_slot));
            sg_mwnd_object_trace(format!(
                "object_child dispatch enter parent_stage={} parent_obj={} child_idx={} child_runtime_slot={} child_op={} child_tail={:?} before_child used={} type={} backend={:?} file={} nested_slot={:?}",
                stage_idx,
                obj_u,
                child_idx,
                runtime_slot,
                tail[2],
                &tail[3..],
                true,
                0,
                "<scratch-before>",
                "-",
                Some(runtime_slot)
            ));
            let handled = dispatch_object_op(
                ctx,
                st,
                stage_idx,
                scratch_slot as i64,
                tail[2],
                &tail[3..],
                script_args,
                ret_form,
                rhs,
                al_id,
            );
            sg_mwnd_object_trace(format!(
                "object_child dispatch returned parent_stage={} parent_obj={} child_idx={} child_runtime_slot={} handled={} child_op={} child_tail={:?} current_chain={:?} current_stage_object={:?}",
                stage_idx,
                obj_u,
                child_idx,
                runtime_slot,
                handled,
                tail[2],
                &tail[3..],
                ctx.globals.current_object_chain,
                ctx.globals.current_stage_object
            ));
            ctx.globals.current_object_chain = prev_chain;
            ctx.globals.current_stage_object = prev_stage_object;
            let mut child_after = {
                let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
                std::mem::take(&mut stage_list[scratch_slot])
            };
            child_after.nested_runtime_slot = Some(runtime_slot);
            {
                let stage_list = st.object_lists.get_mut(&stage_idx).unwrap();
                stage_list[scratch_slot] = slot_snapshot;
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
                        let old_len = obj.runtime.child_objects.len();
                        if n < old_len {
                            clear_embedded_object_list_tail(
                                ctx,
                                &mut obj.runtime.child_objects,
                                stage_idx,
                                n,
                            );
                            obj.runtime.child_objects.truncate(n);
                        } else if n > old_len {
                            obj.runtime
                                .child_objects
                                .resize_with(n, ObjectState::default);
                        }
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
        let (arr_idx, t) = split_property_list_tail(ctx, tail, al_id, ret_form, rhs, script_args);

        if let Some(rep_idx) = arr_idx {
            if rep_idx < 0 {
                ctx.stack.push(Value::Int(0));
                return true;
            }
            let ri = rep_idx as usize;
            if rep_list.len() <= ri {
                rep_list.resize_with(ri + 1, || {
                    IntEvent::new(object_event_list_default(&ctx.ids, op))
                });
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
            if dispatch_int_event_arg_slot(ctx, ev, t, script_args, rhs, al_id, ret_form).is_some()
            {
                return true;
            }
            if let Some(action) = dispatch_int_event_subop(ev, t[0], script_args, al_id) {
                match t[0] {
                    int_event_op::CHECK => {
                        ctx.stack
                            .push(Value::Int(if ev.check_event() { 1 } else { 0 }));
                    }
                    _ => match action {
                        IntEventDispatchAction::Done => ctx.stack.push(Value::Int(0)),
                        IntEventDispatchAction::Wait { key_skip } => {
                            if ev.check_event() {
                                ctx.wait.wait_object_event_list(
                                    ctx.ids.form_global_stage,
                                    stage_idx,
                                    obj_runtime_slot,
                                    op,
                                    ri,
                                    key_skip,
                                    key_skip,
                                );
                                if !key_skip {
                                    push_ok(ctx, ret_form);
                                }
                            } else {
                                push_ok(ctx, ret_form);
                            }
                        }
                    },
                }
                return true;
            }
        } else if t.len() == 1 {
            match t[0] {
                int_event_list_op::RESIZE => {
                    let n = script_args.first().and_then(as_i64).unwrap_or(0).max(0) as usize;
                    rep_list
                        .resize_with(n, || IntEvent::new(object_event_list_default(&ctx.ids, op)));
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
                    rep_list.resize_with(ui + 1, || {
                        IntEvent::new(object_event_list_default(&ctx.ids, op))
                    });
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

    let compact_size_alias_x =
        rhs.is_none() && tail.is_empty() && al_id == Some(1) && ctx.ids.obj_color_rate_eve != 0
            && op == ctx.ids.obj_color_rate_eve;
    let compact_size_alias_y =
        rhs.is_none() && tail.is_empty() && al_id == Some(1) && ctx.ids.obj_color_add_r_eve != 0
            && op == ctx.ids.obj_color_add_r_eve;

    let prefer_object_query_helper = rhs.is_none()
        && tail.is_empty()
        && ((ctx.ids.obj_get_pat_cnt != 0 && op == ctx.ids.obj_get_pat_cnt)
            || (ctx.ids.obj_get_size_x != 0 && op == ctx.ids.obj_get_size_x)
            || (ctx.ids.obj_get_size_y != 0 && op == ctx.ids.obj_get_size_y)
            || (ctx.ids.obj_get_size_z != 0 && op == ctx.ids.obj_get_size_z)
            || (ctx.ids.obj_get_pixel_color_r != 0 && op == ctx.ids.obj_get_pixel_color_r)
            || (ctx.ids.obj_get_pixel_color_g != 0 && op == ctx.ids.obj_get_pixel_color_g)
            || (ctx.ids.obj_get_pixel_color_b != 0 && op == ctx.ids.obj_get_pixel_color_b)
            || (ctx.ids.obj_get_pixel_color_a != 0 && op == ctx.ids.obj_get_pixel_color_a)
            || compact_size_alias_x
            || compact_size_alias_y);

    if !prefer_object_query_helper && (is_obj_int_list || is_obj_int_event || is_obj_int_event_list)
    {
        // Plain OBJECT.*_EVE properties are INTEVENT objects, not lists.
        // Their tail begins with the INTEVENT sub-operation, e.g.
        //   OBJECT.COLOR_RATE_EVE.SET(..., start := 10)
        // appears as tail [0, -1, 10]. Do not run the compact-list-index
        // heuristic here, otherwise sub-op 0 is misread as array index 0.
        let (arr_idx, t) = if is_obj_int_event && !is_obj_int_list && !is_obj_int_event_list {
            (None, tail)
        } else {
            split_property_list_tail(ctx, tail, al_id, ret_form, rhs, script_args)
        };

        if is_obj_int_list && arr_idx.is_none() && t.len() == 1 {
            match t[0] {
                intlist_op::RESIZE => {
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
                intlist_op::GET_SIZE => {
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
            && t[0] == int_event_list_op::RESIZE
            && script_args.len() == 1
        {
            if let Some(n0) = script_args.first().and_then(as_i64) {
                let n = n0.max(0) as usize;
                if let Some(list) = obj.int_event_list_by_op_mut(&ctx.ids, op) {
                    list.resize_with(n, || IntEvent::new(object_event_list_default(&ctx.ids, op)));
                }
            }
            ctx.stack.push(Value::Int(0));
            return true;
        }

        if arr_idx.is_none() && t.is_empty() {
            if is_obj_int_event_list {
                let ent = obj.int_event_list_by_op_mut(&ctx.ids, op).unwrap();
                if ent.is_empty() {
                    ent.push(IntEvent::new(object_event_list_default(&ctx.ids, op)));
                }
                let ev = &mut ent[0];
                if let Some(Value::Int(v)) = rhs {
                    ev.set_value(*v as i32);
                    ctx.stack.push(Value::Int(0));
                } else {
                    ctx.stack.push(Value::Int(ev.get_value() as i64));
                }
                return true;
            }
            if is_obj_int_list {
                let ent = obj.int_list_by_op_mut(&ctx.ids, op).unwrap();
                if ent.is_empty() {
                    ent.push(0);
                }
                if let Some(Value::Int(v)) = rhs {
                    ent[0] = *v;
                    ctx.stack.push(Value::Int(0));
                } else {
                    ctx.stack.push(Value::Int(ent[0]));
                }
                return true;
            }
        }

        let arr_idx = if arr_idx.is_none()
            && is_obj_int_list
            && ctx.ids.obj_f != 0
            && op == ctx.ids.obj_f
            && t.is_empty()
        {
            Some(0)
        } else {
            arr_idx
        };

        if let Some(rep_idx) = arr_idx {
            if rep_idx < 0 {
                ctx.stack.push(Value::Int(0));
                return true;
            }
            let ri = rep_idx as usize;

            if is_obj_int_event_list {
                let ent = obj.int_event_list_by_op_mut(&ctx.ids, op).unwrap();
                if ent.len() <= ri {
                    ent.resize_with(ri + 1, || {
                        IntEvent::new(object_event_list_default(&ctx.ids, op))
                    });
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
                if dispatch_int_event_arg_slot(ctx, ev, t, script_args, rhs, al_id, ret_form)
                    .is_some()
                {
                    return true;
                }
                if let Some(action) = dispatch_int_event_subop(ev, t[0], script_args, al_id) {
                    match t[0] {
                        int_event_op::CHECK => {
                            ctx.stack
                                .push(Value::Int(if ev.check_event() { 1 } else { 0 }));
                        }
                        _ => match action {
                            IntEventDispatchAction::Done => ctx.stack.push(Value::Int(0)),
                            IntEventDispatchAction::Wait { key_skip } => {
                                if ev.check_event() {
                                    ctx.wait.wait_object_event_list(
                                        ctx.ids.form_global_stage,
                                        stage_idx,
                                        obj_runtime_slot,
                                        op,
                                        ri,
                                        key_skip,
                                        key_skip,
                                    );
                                    if !key_skip {
                                        push_ok(ctx, ret_form);
                                    }
                                } else {
                                    push_ok(ctx, ret_form);
                                }
                            }
                        },
                    }
                    return true;
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
            if dispatch_int_event_arg_slot(ctx, ev, t, script_args, rhs, al_id, ret_form).is_some()
            {
                return true;
            }
            if let Some(action) = dispatch_int_event_subop(ev, t[0], script_args, al_id) {
                match t[0] {
                    int_event_op::CHECK => {
                        ctx.stack
                            .push(Value::Int(if ev.check_event() { 1 } else { 0 }));
                    }
                    _ => match action {
                        IntEventDispatchAction::Done => ctx.stack.push(Value::Int(0)),
                        IntEventDispatchAction::Wait { key_skip } => {
                            if ev.check_event() {
                                ctx.wait.wait_object_event(
                                    ctx.ids.form_global_stage,
                                    stage_idx,
                                    obj_runtime_slot,
                                    op,
                                    key_skip,
                                    key_skip,
                                );
                                if !key_skip {
                                    push_ok(ctx, ret_form);
                                }
                            } else {
                                push_ok(ctx, ret_form);
                            }
                        }
                    },
                }
                return true;
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
                ctx.wait.wait_object_all_events(
                    ctx.ids.form_global_stage,
                    stage_idx,
                    obj_runtime_slot,
                    false,
                );
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
        object_reinit_finish_free_like_cpp(ctx, obj, stage_idx, obj_runtime_slot);
        obj.used = true;
        obj.backend = ObjectBackend::None;
        obj.file_name = None;
        obj.string_value = None;
        obj.init_param_like();
        push_ok(ctx, ret_form);
        return true;
    }

    if op == ctx.ids.obj_free {
        object_clear_backend_recursive(ctx, obj, stage_idx, obj_runtime_slot);
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
    if ctx.ids.obj_exist_type != 0 && op == ctx.ids.obj_exist_type {
        ctx.stack
            .push(Value::Int(if obj.object_type == 0 { 0 } else { 1 }));
        return true;
    }

    if op == constants::elm_value::OBJECT_GET_TYPE {
        ctx.stack.push(Value::Int(obj.object_type));
        return true;
    }

    if op == constants::elm_value::OBJECT_GET_ELEMENT_NAME {
        ctx.stack.push(Value::Str(format!(
            "stage[{}].object[{}]",
            stage_idx, obj_u
        )));
        return true;
    }

    if op == constants::elm_value::OBJECT_CLEAR_HINTS
        || op == constants::elm_value::OBJECT_ADD_HINTS
    {
        if op == constants::elm_value::OBJECT_CLEAR_HINTS {
            obj.base.no_event_hint = false;
        }
        let (_pos, named) = split_pos_named(script_args);
        for (id, v) in named {
            if id == 0 {
                obj.base.no_event_hint = v.as_i64().unwrap_or(0) != 0;
            }
        }
        push_ok(ctx, ret_form);
        return true;
    }

    if op == ctx.ids.obj_create {
        let Some(file) = script_args.get(0).and_then(as_str) else {
            push_ok(ctx, ret_form);
            return true;
        };

        let argc = script_args.len();
        let old_disp = obj.get_int_prop(&ctx.ids, ctx.ids.obj_disp) != 0;
        let old_x = obj.get_int_prop(&ctx.ids, ctx.ids.obj_x);
        let old_y = obj.get_int_prop(&ctx.ids, ctx.ids.obj_y);
        let old_patno = obj.get_int_prop(&ctx.ids, ctx.ids.obj_patno);
        let disp = if overload_at_least(al_id, argc, 1, 2) {
            script_i64(script_args, 1, 0) != 0
        } else {
            old_disp
        };
        let x = if overload_at_least(al_id, argc, 2, 4) {
            script_i64(script_args, 2, 0)
        } else {
            old_x
        };
        let y = if overload_at_least(al_id, argc, 2, 4) {
            script_i64(script_args, 3, 0)
        } else {
            old_y
        };
        let patno = if overload_at_least(al_id, argc, 3, 5) {
            script_i64(script_args, 4, 0)
        } else {
            old_patno
        };

        sg_debug_stage(format!(
            "stage={} obj={} CREATE(file={}) al_id={:?} disp={} x={} y={} patno={}",
            stage_idx, obj_u, file, al_id, disp, x, y, patno
        ));

        object_init_type_free_self_like_cpp(ctx, obj, stage_idx, obj_runtime_slot);

        let create_result = {
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            gfx.object_create(
                images,
                layers,
                stage_idx,
                obj_runtime_slot as i64,
                file,
                disp as i64,
                x,
                y,
                patno,
            )
        };
        let create_ok = create_result.is_ok();
        if let Err(ref err) = create_result {
            ctx.unknown.record_note(&format!(
                "OBJECT.CREATE.image.failed:stage={stage_idx}:slot={obj_u}:file={file}:patno={patno}:{err}"
            ));
        }
        sg_mwnd_object_trace(format!(
            "object_create result stage={} obj={} runtime_slot={} file={} create_ok={} nested_slot={:?} before_hide_bind={:?}",
            stage_idx,
            obj_u,
            obj_runtime_slot,
            file,
            create_ok,
            obj.nested_runtime_slot,
            ctx.gfx.object_sprite_binding(stage_idx, obj_runtime_slot as i64)
        ));
        if obj.nested_runtime_slot.is_some() {
            hide_embedded_gfx_backing(ctx, stage_idx, obj_runtime_slot);
        }
        obj.used = true;
        obj.backend = ObjectBackend::Gfx;
        obj.object_type = 2;
        obj.number_value = 0;
        obj.string_param = Default::default();
        obj.number_param = Default::default();
        obj.file_name = Some(file.to_string());
        obj.string_value = None;
        mark_cgtable_look_from_object_create(&mut ctx.tables, ctx.globals.cg_table_off, file);
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp { 1 } else { 0 });
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
                    {
                        let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                        let _ = gfx.object_set_disp(
                            images,
                            layers,
                            stage_idx,
                            obj_runtime_slot as i64,
                            if b { 1 } else { 0 },
                        );
                    }
                    if obj.nested_runtime_slot.is_some() {
                        hide_embedded_gfx_backing(ctx, stage_idx, obj_runtime_slot);
                    }
                    obj.set_int_prop(&ctx.ids, op, if b { 1 } else { 0 });
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
                ObjectBackend::Gfx => obj.get_int_prop(&ctx.ids, op),
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
                    let _ = gfx.object_set_x(images, layers, stage_idx, obj_runtime_slot as i64, v);
                    obj.set_int_prop(&ctx.ids, op, v);
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
                ObjectBackend::Gfx => obj.get_int_prop(&ctx.ids, op),
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
                    let _ = gfx.object_set_y(images, layers, stage_idx, obj_runtime_slot as i64, v);
                    obj.set_int_prop(&ctx.ids, op, v);
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
                ObjectBackend::Gfx => obj.get_int_prop(&ctx.ids, op),
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
                let _ = ctx.gfx.object_set_z(stage_idx, obj_runtime_slot as i64, v);
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
                    let pat_result = {
                        let (gfx, images, layers) =
                            (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                        gfx.object_set_pat_no(images, layers, stage_idx, obj_runtime_slot as i64, v)
                    };
                    if let Err(err) = pat_result {
                        ctx.unknown.record_note(&format!(
                            "OBJECT.PATNO.image.failed:stage={stage_idx}:slot={obj_u}:patno={v}:{err}"
                        ));
                    }
                    obj.set_int_prop(&ctx.ids, op, v);
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
                    .object_peek_patno(stage_idx, obj_runtime_slot as i64)
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
                    let _ = gfx.object_set_layer(images, layers, stage_idx, obj_runtime_slot as i64, v);
                    obj.set_int_prop(&ctx.ids, op, v);
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
                    .object_peek_layer(stage_idx, obj_runtime_slot as i64)
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
                        gfx.object_set_alpha(images, layers, stage_idx, obj_runtime_slot as i64, i64::from(a));
                    obj.set_int_prop(&ctx.ids, op, i64::from(a));
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
                    .object_peek_alpha(stage_idx, obj_runtime_slot as i64)
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
                    let _ = gfx.object_set_order(images, layers, stage_idx, obj_runtime_slot as i64, v);
                    obj.set_int_prop(&ctx.ids, op, v);
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
                    .object_peek_order(stage_idx, obj_runtime_slot as i64)
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
        mark_cgtable_look_from_object_create(&mut ctx.tables, ctx.globals.cg_table_off, name);
        if matches!(obj.backend, ObjectBackend::Gfx) {
            let disp = ctx
                .gfx
                .object_peek_disp(stage_idx, obj_runtime_slot as i64)
                .unwrap_or(0)
                != 0;
            let (x, y) = ctx
                .gfx
                .object_peek_pos(stage_idx, obj_runtime_slot as i64)
                .unwrap_or((0, 0));
            let pat = ctx
                .gfx
                .object_peek_patno(stage_idx, obj_runtime_slot as i64)
                .unwrap_or(0);
            {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                let _ = gfx.object_create(
                    images,
                    layers,
                    stage_idx,
                    obj_runtime_slot as i64,
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

        object_reinit_finish_free_like_cpp(ctx, obj, stage_idx, obj_runtime_slot);

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
        let argc = pos.len();
        let disp_i = if overload_at_least(al_id, argc, 1, 2) {
            positional_ref_i64(&pos, 1, 0)
        } else {
            0
        };
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        if overload_at_least(al_id, argc, 2, 4) {
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
        object_reinit_finish_free_like_cpp(ctx, obj, stage_idx, obj_runtime_slot);
        obj.used = true;
        obj.object_type = 4;
        obj.file_name = Some(file.to_string());
        obj.string_value = None;
        obj.number_value = 0;
        obj.string_param = Default::default();
        obj.number_param = Default::default();
        obj.weather_param = Default::default();
        obj.weather_work = Default::default();
        obj.movie.reset();
        obj.emote = Default::default();
        obj.gan_file = None;
        obj.init_param_like();
        obj.mesh_animation_state = crate::mesh3d::MeshAnimationState::default();

        let layer_id = ensure_rect_layer(ctx, st, stage_idx);
        obj.backend = ObjectBackend::Weather {
            layer_id,
            sprite_ids: Vec::new(),
        };

        let argc = pos.len();
        let disp_i = if overload_at_least(al_id, argc, 1, 2) {
            positional_ref_i64(&pos, 1, 0)
        } else {
            0
        };
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        if overload_at_least(al_id, argc, 2, 4) {
            if ctx.ids.obj_x != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_x, positional_ref_i64(&pos, 2, 0));
            }
            if ctx.ids.obj_y != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_y, positional_ref_i64(&pos, 3, 0));
            }
        }
        if ctx.ids.obj_patno != 0 {
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_patno, 0);
        }

        obj.restruct_weather_work(ctx.screen_w as i64, ctx.screen_h as i64);
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_create_mesh != 0 && op == ctx.ids.obj_create_mesh {
        let (pos, _named) = split_pos_named(script_args);
        let Some(file) = pos.get(0).and_then(|v| v.as_str()) else {
            push_ok(ctx, ret_form);
            return true;
        };
        let argc = pos.len();
        let disp_i = if overload_at_least(al_id, argc, 1, 2) {
            positional_ref_i64(&pos, 1, 0)
        } else {
            0
        };
        let x = if overload_at_least(al_id, argc, 2, 4) {
            positional_ref_i64(&pos, 2, 0)
        } else {
            0
        };
        let y = if overload_at_least(al_id, argc, 2, 4) {
            positional_ref_i64(&pos, 3, 0)
        } else {
            0
        };


        object_reinit_finish_free_like_cpp(ctx, obj, stage_idx, obj_runtime_slot);
        obj.used = true;
        obj.object_type = 6;
        obj.file_name = Some(file.to_string());
        obj.movie.reset();
        obj.emote = Default::default();
        obj.gan_file = None;
        obj.init_param_like();
        obj.mesh_animation_state = crate::mesh3d::MeshAnimationState::default();
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_x, x);
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_y, y);
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_patno, 0);

        if let Err(e) = load_mesh_asset(&ctx.project_dir, ctx.images.current_append_dir(), file) {
            ctx.unknown
                .record_note(&format!("object.create_mesh.asset.failed:{file}:{e}"));
        }
        {
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            let _ = gfx.object_create(
                images,
                layers,
                stage_idx,
                obj_runtime_slot as i64,
                file,
                disp_i,
                x,
                y,
                0,
            );
        }
        sync_special_gfx_sprite_for_object(ctx, stage_idx, obj_runtime_slot, obj);
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
        let argc = pos.len();
        let disp_i = if overload_at_least(al_id, argc, 1, 2) {
            positional_ref_i64(&pos, 1, 0)
        } else {
            0
        };
        let x = if overload_at_least(al_id, argc, 2, 4) {
            positional_ref_i64(&pos, 2, 0)
        } else {
            0
        };
        let y = if overload_at_least(al_id, argc, 2, 4) {
            positional_ref_i64(&pos, 3, 0)
        } else {
            0
        };


        object_reinit_finish_free_like_cpp(ctx, obj, stage_idx, obj_runtime_slot);
        obj.used = true;
        obj.object_type = 7;
        obj.file_name = Some(file.to_string());
        obj.movie.reset();
        obj.emote = Default::default();
        obj.gan_file = None;
        obj.init_param_like();
        obj.mesh_animation_state = crate::mesh3d::MeshAnimationState::default();
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_x, x);
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_y, y);
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_patno, 0);
        {
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            let _ = gfx.object_create(
                images,
                layers,
                stage_idx,
                obj_runtime_slot as i64,
                file,
                disp_i,
                x,
                y,
                0,
            );
        }
        sync_special_gfx_sprite_for_object(ctx, stage_idx, obj_runtime_slot, obj);
        obj.backend = ObjectBackend::Gfx;
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_create_save_thumb != 0 && op == ctx.ids.obj_create_save_thumb {
        let (pos, _named) = split_pos_named(script_args);
        let save_no = pos.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
        object_reinit_finish_free_like_cpp(ctx, obj, stage_idx, obj_runtime_slot);
        obj.used = true;
        obj.object_type = 8;
        obj.thumb_save_no = save_no;
        obj.movie.reset();
        obj.init_param_like();
        // Optional (disp, x, y) via al_id.
        let argc = pos.len();
        let disp_i = if overload_at_least(al_id, argc, 1, 2) {
            positional_ref_i64(&pos, 1, 0)
        } else {
            0
        };
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        if overload_at_least(al_id, argc, 2, 4) {
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
        let img_id = if let Some(img_id) = load_thumb_image_id(ctx, save_no) {
            img_id
        } else {
            ctx.unknown
                .record_note(&format!("save_thumb.image.missing:{save_no}"));
            insert_capture_image_id(ctx, true)
        };
        bind_capture_backend(ctx, obj, stage_idx, img_id);
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_create_capture_thumb != 0 && op == ctx.ids.obj_create_capture_thumb {
        let (pos, _named) = split_pos_named(script_args);
        let thumb_no = pos.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
        object_reinit_finish_free_like_cpp(ctx, obj, stage_idx, obj_runtime_slot);
        obj.used = true;
        obj.object_type = 11;
        obj.thumb_save_no = thumb_no;
        obj.movie.reset();
        obj.init_param_like();
        // Optional (disp, x, y) via al_id.
        let argc = pos.len();
        let disp_i = if overload_at_least(al_id, argc, 1, 2) {
            positional_ref_i64(&pos, 1, 0)
        } else {
            0
        };
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        if overload_at_least(al_id, argc, 2, 4) {
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
        let img_id = if let Some(img_id) = load_thumb_image_id(ctx, thumb_no) {
            img_id
        } else {
            ctx.unknown
                .record_note(&format!("thumb.image.missing:{thumb_no}"));
            insert_capture_image_id(ctx, true)
        };
        bind_capture_backend(ctx, obj, stage_idx, img_id);
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_create_capture != 0 && op == ctx.ids.obj_create_capture {
        let (pos, _named) = split_pos_named(script_args);
        object_reinit_finish_free_like_cpp(ctx, obj, stage_idx, obj_runtime_slot);
        obj.used = true;
        obj.object_type = 10;
        obj.movie.reset();
        obj.init_param_like();
        // Optional parameters: (disp, x, y) via al_id with different indexing.
        let argc = pos.len();
        let disp_i = if overload_at_least(al_id, argc, 1, 1) {
            positional_ref_i64(&pos, 0, 0)
        } else {
            0
        };
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        if overload_at_least(al_id, argc, 2, 3) {
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
        let img_id = insert_capture_image_id(ctx, true);
        bind_capture_backend(ctx, obj, stage_idx, img_id);
        push_ok(ctx, ret_form);
        return true;
    }

    if op == constants::elm_value::OBJECT_CREATE_FROM_CAPTURE_FILE {
        let (pos, _named) = split_pos_named(script_args);
        let file_opt = pos.get(0).and_then(|v| v.as_str());
        let path_opt = file_opt.and_then(|file| {
            resolve_capture_file_path(&ctx.project_dir, &ctx.globals.append_dir, file)
        });
        object_reinit_finish_free_like_cpp(ctx, obj, stage_idx, obj_runtime_slot);
        obj.used = true;
        obj.object_type = 10;
        obj.file_name = file_opt.map(|s| s.to_string());
        obj.movie.reset();
        obj.init_param_like();
        let img_id = if let Some(path) = path_opt {
            ctx.images
                .load_file(&path, 0)
                .unwrap_or_else(|_| insert_capture_image_id(ctx, true))
        } else {
            insert_capture_image_id(ctx, true)
        };
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

            object_reinit_finish_free_like_cpp(ctx, obj, stage_idx, obj_runtime_slot);
            obj.used = true;
            obj.object_type = 9;
            obj.file_name = Some(file.to_string());
            obj.string_value = None;
            obj.button.clear();
            obj.clear_runtime_only();

            let movie_path =
                resolve_object_movie_path(&ctx.project_dir, &ctx.globals.append_dir, file);
            let total_ms = movie_path
                .as_ref()
                .and_then(|_| movie_total_time_ms(ctx, file));
            sg_debug_stage(format!(
                "CREATE_MOVIE stage={} obj={} file={} resolved={:?} loop={} wait={} key_skip={} auto_free={} real_time={} ready_only={} total_ms={:?}",
                stage_idx,
                obj_u,
                file,
                movie_path,
                loop_flag,
                wait_flag,
                key_skip_flag,
                auto_free_flag,
                real_time_flag,
                ready_only_flag,
                total_ms
            ));
            obj.movie.start(
                total_ms,
                loop_flag,
                auto_free_flag,
                real_time_flag,
                ready_only_flag,
            );

            // Optional (disp, x, y) via al_id.
            // Use the raw argument vector when al_id selects a positional overload.
            // CD_COMMAND can wrap trailing values as NamedArg before dispatch; if we
            // only look at split_pos_named(), create_movie_loop(file, 1, 0, 0) can
            // lose disp and stay invisible while frames are decoding correctly.
            let argc = pos.len();
            let raw_arg_i64 = |index: usize, default: i64| -> i64 {
                script_args
                    .get(index)
                    .and_then(|v| v.as_i64())
                    .unwrap_or(default)
            };
            let disp_i = if al_id.unwrap_or(-1) >= 1 {
                raw_arg_i64(1, 0)
            } else if argc >= 2 {
                positional_ref_i64(&pos, 1, 0)
            } else {
                0
            };
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
            if al_id.unwrap_or(-1) >= 2 || argc >= 4 {
                let x = if al_id.unwrap_or(-1) >= 2 {
                    raw_arg_i64(2, 0)
                } else {
                    pos.get(2).and_then(|v| v.as_i64()).unwrap_or(0)
                };
                let y = if al_id.unwrap_or(-1) >= 2 {
                    raw_arg_i64(3, 0)
                } else {
                    pos.get(3).and_then(|v| v.as_i64()).unwrap_or(0)
                };
                if ctx.ids.obj_x != 0 {
                    obj.set_int_prop(&ctx.ids, ctx.ids.obj_x, x);
                }
                if ctx.ids.obj_y != 0 {
                    obj.set_int_prop(&ctx.ids, ctx.ids.obj_y, y);
                }
            }
            if std::env::var_os("SG_DEBUG").is_some() {
                eprintln!(
                    "[SG_DEBUG][MOV] object_movie.create_args stage={} obj={} file={} al_id={:?} raw_argc={} pos_argc={} disp={} x={} y={}",
                    stage_idx,
                    obj_u,
                    file,
                    al_id,
                    script_args.len(),
                    argc,
                    obj.get_int_prop(&ctx.ids, ctx.ids.obj_disp),
                    obj.get_int_prop(&ctx.ids, ctx.ids.obj_x),
                    obj.get_int_prop(&ctx.ids, ctx.ids.obj_y),
                );
            }

            if wait_flag {
                // create_movie_wait(_key) calls wait_movie(key_skip_flag, key_skip_flag).
                ctx.wait.wait_object_movie(
                    ctx.ids.form_global_stage,
                    stage_idx,
                    obj_runtime_slot,
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

        object_reinit_finish_free_like_cpp(ctx, obj, stage_idx, obj_runtime_slot);
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
        let argc = pos.len();
        let disp_i = if overload_at_least(al_id, argc, 1, 4) {
            positional_ref_i64(&pos, 3, 0)
        } else {
            0
        };
        obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp_i != 0 { 1 } else { 0 });
        if overload_at_least(al_id, argc, 2, 6) {
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
        if obj.object_type != 4 {
            push_ok(ctx, ret_form);
            return true;
        }
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
                0 => {
                    wp.cnt = iv
                        .saturating_mul(TNM_SCREEN_RATE)
                        .saturating_mul(TNM_SCREEN_RATE)
                }
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
        obj.restruct_weather_work(ctx.screen_w as i64, ctx.screen_h as i64);
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_weather_param_type_b != 0 && op == ctx.ids.obj_set_weather_param_type_b {
        if obj.object_type != 4 {
            push_ok(ctx, ret_form);
            return true;
        }
        let (_pos, named) = split_pos_named(script_args);
        let mut wp = ObjectWeatherParam {
            weather_type: 2,
            cnt: 0,
            pat_mode: 0,
            pat_no_00: 0,
            pat_no_01: 0,
            pat_time: 0,
            move_time_x: 1000,
            move_time_y: 1000,
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
                5 => {
                    wp.move_time = iv;
                    wp.move_time_x = iv;
                    wp.move_time_y = iv;
                }
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
        obj.restruct_weather_work(ctx.screen_w as i64, ctx.screen_h as i64);
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
            ctx.wait.wait_object_movie(
                ctx.ids.form_global_stage,
                stage_idx,
                obj_runtime_slot,
                false,
                false,
            );
        }
        push_ok(ctx, ret_form);
        return true;
    }
    if ctx.ids.obj_wait_movie_key != 0 && op == ctx.ids.obj_wait_movie_key {
        if obj.movie.check_movie() {
            // wait_movie(true, true)
            ctx.wait.wait_object_movie(
                ctx.ids.form_global_stage,
                stage_idx,
                obj_runtime_slot,
                true,
                true,
            );
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
        if group_no >= 0 {
            ensure_group(ctx, st, stage_idx, group_no as usize);
        }
        obj.button.hit = false;
        obj.button.pushed = false;
        push_ok(ctx, ret_form);
        return true;
    }

    if ctx.ids.obj_set_button_group != 0 && op == ctx.ids.obj_set_button_group {
        // Original C++ accepts either a numeric group number or a GROUP element.
        // A GROUP element is a STAGE.OBJBTNGROUP[...] item/reference, not OBJECT.CHILD.
        if al_id == Some(1) {
            if let Some(Value::Element(e)) = script_args.get(0) {
                match parse_target(ctx, e) {
                    Some(StageTarget::ChildItemOp { child, idx, .. })
                        if child == crate::runtime::forms::codes::STAGE_ELM_OBJBTNGROUP =>
                    {
                        obj.button.group_no = idx.max(0);
                        obj.button.group_idx_override = Some(idx.max(0) as usize);
                    }
                    Some(StageTarget::ChildItemRef { child, idx, .. })
                        if child == crate::runtime::forms::codes::STAGE_ELM_OBJBTNGROUP =>
                    {
                        obj.button.group_no = idx.max(0);
                        obj.button.group_idx_override = Some(idx.max(0) as usize);
                    }
                    _ => {}
                }
            }
        } else {
            let g = script_args.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
            obj.button.group_no = g;
            obj.button.group_idx_override = None;
        }
        if let Some(gidx) = obj.button.group_idx() {
            ensure_group(ctx, st, stage_idx, gidx);
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
        ctx.stack
            .push(Value::Int(if obj.button.action_no >= 0 && obj.button.hit {
                TNM_BTN_STATE_HIT
            } else {
                TNM_BTN_STATE_NORMAL
            }));
        return true;
    }

    if ctx.ids.obj_get_button_real_state != 0 && op == ctx.ids.obj_get_button_real_state {
        if obj.button.action_no < 0 {
            ctx.stack.push(Value::Int(TNM_BTN_STATE_NORMAL));
            return true;
        }
        // Conservative: incorporate group selection.
        let mut stt = obj.button.state;
        if stt != TNM_BTN_STATE_SELECT && stt != TNM_BTN_STATE_DISABLE {
            if let Some(gidx) = obj.button.group_idx() {
                if let Some(gl) = st.group_lists.get(&stage_idx).and_then(|v| v.get(gidx)) {
                    if gl.decided_button_no == obj.button.button_no {
                        stt = TNM_BTN_STATE_PUSH;
                    } else if gl.hit_button_no == obj.button.button_no {
                        stt = TNM_BTN_STATE_HIT;
                    } else if gl.pushed_button_no == obj.button.button_no {
                        stt = TNM_BTN_STATE_PUSH;
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
        let cmd = script_args.get(0).and_then(|v| v.as_str()).unwrap_or("");
        obj.button.decided_action_scn_name = ctx.current_scene_name.clone().unwrap_or_default();
        obj.button.decided_action_cmd_name = cmd.to_string();
        obj.button.decided_action_z_no = -1;
        if sg_debug_enabled_local() {
            eprintln!(
                "[SG_DEBUG][BUTTON] SET_BUTTON_CALL scene={} cmd={} slot={} button_no={} group_no={} action_no={}",
                obj.button.decided_action_scn_name,
                obj.button.decided_action_cmd_name,
                obj_runtime_slot,
                obj.button.button_no,
                obj.button.group_no,
                obj.button.action_no
            );
        }
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

    if op == crate::runtime::forms::codes::ELM_OBJECT_LOAD_GAN {
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

    if op == crate::runtime::forms::codes::ELM_OBJECT_START_GAN {
        let mut set_no = 0i64;
        let mut loop_flag = true;
        let mut real_time_flag = false;
        match al_id.unwrap_or(0) {
            1 => {
                set_no = script_args.get(0).and_then(as_i64).unwrap_or(0);
                loop_flag = script_args.get(1).and_then(as_i64).unwrap_or(1) != 0;
                real_time_flag = script_args.get(2).and_then(as_i64).unwrap_or(0) != 0;
            }
            0 => {
                set_no = script_args.get(0).and_then(as_i64).unwrap_or(0);
                loop_flag = script_args.get(1).and_then(as_i64).unwrap_or(1) != 0;
            }
            _ => {}
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
                let _ = gfx.object_set_pos(images, layers, stage_idx, obj_runtime_slot as i64, x, y);
                if let Some(zv) = z {
                    let _ = ctx.gfx.object_set_z(stage_idx, obj_runtime_slot as i64, zv);
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
                let _ = gfx.object_set_center(images, layers, stage_idx, obj_runtime_slot as i64, x, y);
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
                let _ = gfx.object_set_scale(images, layers, stage_idx, obj_runtime_slot as i64, x, y);
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
                    let _ = gfx.object_set_rotate(images, layers, stage_idx, obj_runtime_slot as i64, zv);
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
                        obj_runtime_slot as i64,
                        use_flag,
                        left,
                        top,
                        right,
                        bottom,
                    );
                }
                _ => {}
            }
            sync_object_dst_clip_backend(ctx, obj, stage_idx, obj_runtime_slot);
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
                        obj_runtime_slot as i64,
                        use_flag,
                        left,
                        top,
                        right,
                        bottom,
                    );
                }
                _ => {}
            }
            sync_object_src_clip_backend(ctx, obj, stage_idx, obj_runtime_slot);
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
                if op == ctx.ids.obj_clip_use
                    || op == ctx.ids.obj_clip_left
                    || op == ctx.ids.obj_clip_top
                    || op == ctx.ids.obj_clip_right
                    || op == ctx.ids.obj_clip_bottom
                {
                    sync_object_dst_clip_backend(ctx, obj, stage_idx, obj_runtime_slot);
                }
                if op == ctx.ids.obj_src_clip_use
                    || op == ctx.ids.obj_src_clip_left
                    || op == ctx.ids.obj_src_clip_top
                    || op == ctx.ids.obj_src_clip_right
                    || op == ctx.ids.obj_src_clip_bottom
                {
                    sync_object_src_clip_backend(ctx, obj, stage_idx, obj_runtime_slot);
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
                                    gfx.object_set_tr(images, layers, stage_idx, obj_runtime_slot as i64, v);
                            } else if op == ctx.ids.obj_mono {
                                let _ =
                                    gfx.object_set_mono(images, layers, stage_idx, obj_runtime_slot as i64, v);
                            } else if op == ctx.ids.obj_reverse {
                                let _ = gfx.object_set_reverse(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_runtime_slot as i64,
                                    v,
                                );
                            } else if op == ctx.ids.obj_bright {
                                let _ = gfx.object_set_bright(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_runtime_slot as i64,
                                    v,
                                );
                            } else if op == ctx.ids.obj_dark {
                                let _ =
                                    gfx.object_set_dark(images, layers, stage_idx, obj_runtime_slot as i64, v);
                            } else if op == ctx.ids.obj_color_rate {
                                let _ = gfx.object_set_color_rate(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_runtime_slot as i64,
                                    v,
                                );
                            } else if op == ctx.ids.obj_color_add_r {
                                let g = obj.get_int_prop(&ctx.ids, ctx.ids.obj_color_add_g);
                                let b = obj.get_int_prop(&ctx.ids, ctx.ids.obj_color_add_b);
                                let _ = gfx.object_set_color_add(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_runtime_slot as i64,
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
                                    obj_runtime_slot as i64,
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
                                    obj_runtime_slot as i64,
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
                                    obj_runtime_slot as i64,
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
                                    obj_runtime_slot as i64,
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
                                    obj_runtime_slot as i64,
                                    r,
                                    g,
                                    v,
                                );
                            } else if op == ctx.ids.obj_blend {
                                let _ = gfx.object_set_blend(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_runtime_slot as i64,
                                    v,
                                );
                            } else if op == ctx.ids.obj_light_no {
                                let _ = gfx.object_set_light_no(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_runtime_slot as i64,
                                    v,
                                );
                            } else if op == ctx.ids.obj_fog_use {
                                let _ = gfx.object_set_fog_use(
                                    images,
                                    layers,
                                    stage_idx,
                                    obj_runtime_slot as i64,
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

    if compact_size_alias_x
        || compact_size_alias_y
        || (ctx.ids.obj_get_size_x != 0 && op == ctx.ids.obj_get_size_x)
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

        if compact_size_alias_x || (ctx.ids.obj_get_size_x != 0 && op == ctx.ids.obj_get_size_x) {
            ctx.stack.push(Value::Int(sx));
            if std::env::var_os("SG_TITLE_HIT_TRACE").is_some() {
                if let Some(name) = obj.file_name.as_deref() {
                    eprintln!(
                        "[SG_TITLE_HIT_TRACE] GET_SIZE_X file={} al_id={:?} pat={} -> {}",
                        name, al_id, pat, sx
                    );
                }
            }
        } else if compact_size_alias_y
            || (ctx.ids.obj_get_size_y != 0 && op == ctx.ids.obj_get_size_y)
        {
            ctx.stack.push(Value::Int(sy));
            if std::env::var_os("SG_TITLE_HIT_TRACE").is_some() {
                if let Some(name) = obj.file_name.as_deref() {
                    eprintln!(
                        "[SG_TITLE_HIT_TRACE] GET_SIZE_Y file={} al_id={:?} pat={} -> {}",
                        name, al_id, pat, sy
                    );
                }
            }
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
        // C++ dispatch: al_id 0 uses cut_no=0, al_id 1 uses the third argument.
        // Unsupported object types, missing textures, out-of-range pixels, or unsupported overloads return 0.
        let channel = if ctx.ids.obj_get_pixel_color_r != 0 && op == ctx.ids.obj_get_pixel_color_r {
            0usize
        } else if ctx.ids.obj_get_pixel_color_g != 0 && op == ctx.ids.obj_get_pixel_color_g {
            1usize
        } else if ctx.ids.obj_get_pixel_color_b != 0 && op == ctx.ids.obj_get_pixel_color_b {
            2usize
        } else {
            3usize
        };
        let out = match al_id {
            Some(0) | None => {
                let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
                let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
                sample_object_pixel_component(ctx, obj, stage_idx, obj_runtime_slot, x, y, 0, channel)
            }
            Some(1) => {
                let x = script_args.get(0).and_then(as_i64).unwrap_or(0);
                let y = script_args.get(1).and_then(as_i64).unwrap_or(0);
                let cut_no = script_args.get(2).and_then(as_i64).unwrap_or(0);
                sample_object_pixel_component(ctx, obj, stage_idx, obj_runtime_slot, x, y, cut_no, channel)
            }
            _ => 0,
        };
        ctx.stack.push(Value::Int(out));
        return true;
    }

    if op == constants::elm_value::OBJECT_SET_CHILD_SORT_TYPE_DEFAULT {
        obj.base.child_sort_type = 0;
        ctx.stack.push(Value::Int(0));
        return true;
    }
    if op == constants::elm_value::OBJECT_SET_CHILD_SORT_TYPE_TEST {
        obj.base.child_sort_type = 1;
        ctx.stack.push(Value::Int(0));
        return true;
    }

    // Emote ops — not yet implemented; stub out to avoid panics.
    if op == constants::elm_value::OBJECT_EMOTE_CHECK_PLAYING {
        // Report "not playing" since emote playback is unimplemented.
        ctx.stack.push(Value::Int(0));
        return true;
    }
    if op == constants::elm_value::OBJECT_EMOTE_WAIT_PLAYING
        || op == constants::elm_value::OBJECT_EMOTE_WAIT_PLAYING_KEY
    {
        // Emotes are not implemented; resolve the wait immediately.
        push_ok(ctx, ret_form);
        return true;
    }
    if op == constants::elm_value::OBJECT_EMOTE_PLAY_TIMELINE
        || op == constants::elm_value::OBJECT_EMOTE_STOP_TIMELINE
        || op == constants::elm_value::OBJECT_EMOTE_SKIP
        || op == constants::elm_value::OBJECT_EMOTE_PASS
        || op == constants::elm_value::OBJECT_EMOTE_MOUTH_VOLUME
        || op == constants::elm_value::OBJECT_EMOTE_KOE_CHARA_NO
    {
        push_ok(ctx, ret_form);
        return true;
    }

    let k = resolve_object_op(&ctx.ids, op);
    match k {
        ObjectOpKind::Init => {
            // INIT => reinit(true)
            object_reinit_finish_free_like_cpp(ctx, obj, stage_idx, obj_runtime_slot);
            obj.runtime.child_objects.clear();
            obj.init_type_like();
            obj.init_param_like();
            obj.used = true;
            ctx.stack.push(Value::Int(0));
            true
        }
        ObjectOpKind::Free => {
            // FREE => init_type(true)
            object_clear_backend_recursive(ctx, obj, stage_idx, obj_runtime_slot);
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
        ObjectOpKind::CreateCopyFrom => {
            // CREATE_COPY_FROM is handled before the match after cloning the source object.
            // If the source element could not be resolved, match C++ command behavior by
            // returning from the command without falling into Unknown.
            push_ok(ctx, ret_form);
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

            object_reinit_finish_free_like_cpp(ctx, obj, stage_idx, obj_runtime_slot);

            let layer_id = ensure_rect_layer(ctx, st, stage_idx);
            let Some(sprite_id) = ctx
                .layers
                .layer_mut(layer_id)
                .map(|layer| layer.create_sprite())
            else {
                push_ok(ctx, ret_form);
                return true;
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

            obj.used = true;
            obj.backend = ObjectBackend::Rect {
                layer_id,
                sprite_id,
                width: w,
                height: h,
            };
            obj.object_type = 1;
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp { 1 } else { 0 });
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

            object_reinit_finish_free_like_cpp(ctx, obj, stage_idx, obj_runtime_slot);
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
            let argc = script_args.len();
            let old_disp = obj.get_int_prop(&ctx.ids, ctx.ids.obj_disp) != 0;
            let old_x = obj.get_int_prop(&ctx.ids, ctx.ids.obj_x);
            let old_y = obj.get_int_prop(&ctx.ids, ctx.ids.obj_y);
            let old_patno = obj.get_int_prop(&ctx.ids, ctx.ids.obj_patno);
            let disp = if overload_at_least(al_id, argc, 1, 2) {
                script_i64(script_args, 1, 0) != 0
            } else {
                old_disp
            };
            let x = if overload_at_least(al_id, argc, 2, 4) {
                script_i64(script_args, 2, 0)
            } else {
                old_x
            };
            let y = if overload_at_least(al_id, argc, 2, 4) {
                script_i64(script_args, 3, 0)
            } else {
                old_y
            };
            let patno = if overload_at_least(al_id, argc, 3, 5) {
                script_i64(script_args, 4, 0)
            } else {
                old_patno
            };
            sg_debug_stage(format!(
                "stage={} obj={} CREATE file={} al_id={:?} disp={} x={} y={} patno={}",
                stage_idx, obj_u, file, al_id, disp, x, y, patno
            ));

            object_init_type_free_self_like_cpp(ctx, obj, stage_idx, obj_runtime_slot);

            {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                let _ = gfx.object_create(
                    images,
                    layers,
                    stage_idx,
                    obj_runtime_slot as i64,
                    file,
                    disp as i64,
                    x,
                    y,
                    patno,
                );
            }
            if obj.nested_runtime_slot.is_some() {
                hide_embedded_gfx_backing(ctx, stage_idx, obj_runtime_slot);
            }
            obj.used = true;
            obj.backend = ObjectBackend::Gfx;
            obj.object_type = 2;
            obj.number_value = 0;
            obj.string_param = Default::default();
            obj.number_param = Default::default();
            obj.file_name = Some(file.to_string());
            obj.string_value = None;
            obj.set_int_prop(&ctx.ids, ctx.ids.obj_disp, if disp { 1 } else { 0 });
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

            if ctx.ids.obj_x != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_x, x);
            }
            if ctx.ids.obj_y != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_y, y);
            }
            if let Some(zv) = z {
                if ctx.ids.obj_z != 0 {
                    obj.set_int_prop(&ctx.ids, ctx.ids.obj_z, zv);
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
                    {
                        let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                        let _ = gfx.object_set_pos(images, layers, stage_idx, obj_runtime_slot as i64, x, y);
                    }
                    if let Some(zv) = z {
                        let _ = ctx.gfx.object_set_z(stage_idx, obj_runtime_slot as i64, zv);
                    }
                }
                _ => {
                    obj.set_int_prop(&ctx.ids, ctx.ids.obj_x, x);
                    obj.set_int_prop(&ctx.ids, ctx.ids.obj_y, y);
                    if let Some(zv) = z {
                        if ctx.ids.obj_z != 0 {
                            obj.set_int_prop(&ctx.ids, ctx.ids.obj_z, zv);
                        }
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

            if ctx.ids.obj_center_x != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_center_x, x);
            }
            if ctx.ids.obj_center_y != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_center_y, y);
            }
            if ctx.ids.obj_center_z != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_center_z, z);
            }

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
                    let _ = gfx.object_set_center(images, layers, stage_idx, obj_runtime_slot as i64, x, y);
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

            if ctx.ids.obj_scale_x != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_scale_x, x);
            }
            if ctx.ids.obj_scale_y != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_scale_y, y);
            }
            if ctx.ids.obj_scale_z != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_scale_z, z);
            }

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
                    let _ = gfx.object_set_scale(images, layers, stage_idx, obj_runtime_slot as i64, x, y);
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

            if ctx.ids.obj_rotate_x != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_rotate_x, x);
            }
            if ctx.ids.obj_rotate_y != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_rotate_y, y);
            }
            if ctx.ids.obj_rotate_z != 0 {
                obj.set_int_prop(&ctx.ids, ctx.ids.obj_rotate_z, z);
            }

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
                    let _ = gfx.object_set_rotate(images, layers, stage_idx, obj_runtime_slot as i64, z);
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
                        obj_runtime_slot as i64,
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
                        obj_runtime_slot as i64,
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
        ObjectOpKind::Unknown => {
            panic!(
                "unsupported OBJECT op {} tail={:?} al_id={:?}",
                op, tail, al_id
            );
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
        constants::MWND_MSG_BLOCK | constants::MWND_MSG_PP_BLOCK => Some(MwndOpKind::MsgBlock),
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
        constants::MWND_KOE_PLAY_WAIT_KEY | constants::MWND_EXKOE_PLAY_WAIT_KEY => {
            Some(MwndOpKind::KoePlayWaitKey)
        },
        constants::MWND_KOE_PLAY_WAIT | constants::MWND_EXKOE_PLAY_WAIT => {
            Some(MwndOpKind::KoePlayWait)
        },
        constants::MWND_KOE | constants::MWND_EXKOE => Some(MwndOpKind::Koe),
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
        constants::MWND_SET_WAKU => Some(MwndOpKind::SetWaku),
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
            g.reinit();
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
            ctx.input.use_current();
            g.init_sel();
            if k == GroupOpKind::StartCancel {
                g.cancel_flag = true;
                g.cancel_se_no = script_args.iter().find_map(as_i64).unwrap_or(-1);
            }
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
            // - consume current input edges
            // - reset selection state
            // - start selection
            // - set wait flag and focus so Enter/Esc can drive it
            ctx.input.use_current();
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

            // Block VM until a decision is produced by the runtime input bridge.
            // The original engine pushes the result only when the selection is decided.
            ctx.wait.wait_key();
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

fn global_mwnd_op_from_global_op(op: i32) -> Option<i32> {
    match op {
        constants::GLOBAL_OPEN => Some(constants::MWND_OPEN),
        constants::GLOBAL_OPEN_WAIT => Some(constants::MWND_OPEN_WAIT),
        constants::GLOBAL_OPEN_NOWAIT => Some(constants::MWND_OPEN_NOWAIT),
        constants::GLOBAL_CLOSE => Some(constants::MWND_CLOSE),
        constants::GLOBAL_CLOSE_WAIT => Some(constants::MWND_CLOSE_WAIT),
        constants::GLOBAL_CLOSE_NOWAIT => Some(constants::MWND_CLOSE_NOWAIT),
        constants::GLOBAL_END_CLOSE => Some(constants::MWND_END_CLOSE),
        constants::GLOBAL_MSG_BLOCK => Some(constants::MWND_MSG_BLOCK),
        constants::GLOBAL_MSG_PP_BLOCK => Some(constants::MWND_MSG_PP_BLOCK),
        constants::GLOBAL_CLEAR => Some(constants::MWND_CLEAR),
        constants::GLOBAL_PRINT => Some(constants::MWND_PRINT),
        constants::GLOBAL_NL => Some(constants::MWND_NL),
        constants::GLOBAL_NLI => Some(constants::MWND_NLI),
        constants::GLOBAL_WAIT_MSG => Some(constants::MWND_WAIT_MSG),
        constants::GLOBAL_PP => Some(constants::MWND_PP),
        constants::GLOBAL_R => Some(constants::MWND_R),
        constants::GLOBAL_PAGE => Some(constants::MWND_PAGE),
        constants::GLOBAL_SET_NAMAE => Some(constants::MWND_SET_NAMAE),
        constants::GLOBAL_NEXT_MSG => Some(constants::MWND_NEXT_MSG),
        constants::GLOBAL_MULTI_MSG => Some(constants::MWND_MULTI_MSG),
        constants::GLOBAL_RUBY => Some(constants::MWND_RUBY),
        constants::GLOBAL_KOE => Some(constants::MWND_KOE),
        constants::GLOBAL_KOE_PLAY_WAIT => Some(constants::MWND_KOE_PLAY_WAIT),
        constants::GLOBAL_KOE_PLAY_WAIT_KEY => Some(constants::MWND_KOE_PLAY_WAIT_KEY),
        constants::GLOBAL_EXKOE => Some(constants::MWND_EXKOE),
        constants::GLOBAL_EXKOE_PLAY_WAIT => Some(constants::MWND_EXKOE_PLAY_WAIT),
        constants::GLOBAL_EXKOE_PLAY_WAIT_KEY => Some(constants::MWND_EXKOE_PLAY_WAIT_KEY),
        constants::GLOBAL_SIZE => Some(constants::MWND_SIZE),
        constants::GLOBAL_COLOR => Some(constants::MWND_COLOR),
        constants::GLOBAL_INDENT => Some(constants::MWND_INDENT),
        constants::GLOBAL_CLEAR_INDENT => Some(constants::MWND_CLEAR_INDENT),
        constants::GLOBAL_START_SLIDE_MSG => Some(constants::MWND_START_SLIDE_MSG),
        constants::GLOBAL_END_SLIDE_MSG => Some(constants::MWND_END_SLIDE_MSG),
        constants::GLOBAL_SET_WAKU => Some(constants::MWND_SET_WAKU),
        constants::GLOBAL_CLEAR_FACE => Some(constants::MWND_CLEAR_FACE),
        constants::GLOBAL_SET_FACE => Some(constants::MWND_SET_FACE),
        constants::GLOBAL_REP_POS => Some(constants::MWND_REP_POS),
        constants::GLOBAL_MSGBTN => Some(constants::MWND_MSGBTN),
        _ => None,
    }
}

pub fn dispatch_current_mwnd_global_op(
    ctx: &mut CommandContext,
    global_op: i32,
    script_args: &[Value],
) -> bool {
    let Some(mwnd_op) = global_mwnd_op_from_global_op(global_op) else {
        return false;
    };
    let form_id = if ctx.ids.form_global_stage != 0 {
        ctx.ids.form_global_stage
    } else {
        constants::global_form::STAGE_ALT
    };
    let stage_idx = ctx.globals.current_mwnd_stage_idx;
    let mwnd_idx = ctx.globals.current_mwnd_no.unwrap_or(0);
    let (al_id, ret_form) = prop_access::current_vm_meta(ctx);
    let rhs_owned = if al_id == Some(1) {
        script_args.first().cloned()
    } else {
        None
    };
    with_stage_state(ctx, form_id, |ctx, st| {
        dispatch_mwnd_item_op(
            ctx,
            st,
            stage_idx,
            mwnd_idx,
            mwnd_op,
            &[],
            script_args,
            rhs_owned.as_ref(),
            al_id,
            ret_form,
        )
    })
}


fn current_mwnd_target(ctx: &CommandContext) -> (u32, i64, usize) {
    let form_id = if ctx.ids.form_global_stage != 0 {
        ctx.ids.form_global_stage
    } else {
        constants::global_form::STAGE_ALT
    };
    (
        form_id,
        ctx.globals.current_mwnd_stage_idx,
        ctx.globals.current_mwnd_no.unwrap_or(0),
    )
}


fn is_hankaku_moji(ch: char) -> bool {
    ch.is_ascii() || matches!(ch as u32, 0xFF61..=0xFF9F)
}

fn mwnd_message_cursor_pos(m: &MwndState) -> (i64, i64) {
    let (base_x, base_y) = m.message_pos.unwrap_or((0, 0));
    let font_cell = m.moji_size.unwrap_or(26).clamp(1, 256);
    let (space_x, space_y) = m.moji_space.unwrap_or((-1, 10));
    let full_step = (font_cell + space_x).max(1);
    let half_step = ((font_cell / 2).max(1) + space_x).max(1);
    let line_step = (font_cell + space_y).max(font_cell).max(1);
    let max_w = m
        .window_moji_cnt
        .map(|(cols, _)| {
            let cols = cols.max(1);
            (font_cell * cols + space_x * (cols - 1)).max(1)
        })
        .unwrap_or(i64::MAX / 4);

    let mut x = 0i64;
    let mut y = 0i64;
    for ch in m.msg_text.chars() {
        match ch {
            '\r' => continue,
            '\n' => {
                x = 0;
                y += line_step;
                continue;
            }
            '\t' => {
                let adv = full_step * 2;
                if x > 0 && x + adv > max_w {
                    x = 0;
                    y += line_step;
                }
                x += adv;
                continue;
            }
            _ => {}
        }
        let adv = if is_hankaku_moji(ch) { half_step } else { full_step };
        if x > 0 && x + adv > max_w {
            x = 0;
            y += line_step;
        }
        x += adv;
    }

    (base_x + x, base_y + y)
}

fn set_mwnd_key_icon_wait(ctx: &mut CommandContext, m: &mut MwndState, mode: i64) {
    m.key_icon_appear = true;
    m.key_icon_mode = mode;
    if m.icon_pos_type == 1 {
        m.key_icon_pos = Some(mwnd_message_cursor_pos(m));
    }
    if mode == 1 {
        ctx.ui.begin_wait_page_message();
    } else {
        ctx.ui.begin_wait_message();
    }
}

fn start_mwnd_auto_message(ctx: &mut CommandContext, m: &mut MwndState) {
    m.key_icon_appear = false;
    m.key_icon_pos = None;
    m.key_icon_mode = 0;
    if !m.open {
        m.open = true;
        ctx.ui.begin_mwnd_open(m.open_anime_type, m.open_anime_time);
    } else {
        ctx.ui.show_message_bg(true);
    }
}

fn clear_mwnd_message_block_now(ctx: &mut CommandContext, m: &mut MwndState) {
    m.msg_text.clear();
    m.name_text.clear();
    m.chara_color_mod = None;
    m.chara_moji_color = None;
    m.chara_shadow_color = None;
    m.chara_fuchi_color = None;
    m.key_icon_appear = false;
    m.key_icon_pos = None;
    ctx.ui.clear_message();
    ctx.ui.clear_name();
    m.multi_msg = false;
    m.text_dirty = false;
    m.clear_ready = false;
    m.msg_block_started = false;
}

fn clear_mwnd_for_novel_one_msg(m: &mut MwndState) {
    m.chara_color_mod = None;
    m.chara_moji_color = None;
    m.chara_shadow_color = None;
    m.chara_fuchi_color = None;
    m.koe = None;
}

fn start_mwnd_msg_block_if_needed(ctx: &mut CommandContext, m: &mut MwndState) {
    if m.msg_block_started {
        return;
    }

    let had_message = !m.msg_text.is_empty() || !m.name_text.is_empty() || m.text_dirty;
    if m.clear_ready {
        clear_mwnd_message_block_now(ctx, m);
    }
    clear_mwnd_for_novel_one_msg(m);
    if had_message {
        msgbk_next(ctx);
    }
    m.clear_ready = false;
    m.msg_block_started = true;
}

fn mark_mwnd_clear_ready(ctx: &mut CommandContext, m: &mut MwndState) {
    m.clear_ready = true;
    m.msg_block_started = false;
    m.multi_msg = false;
    m.text_dirty = false;
    let _ = ctx;
}

fn wait_after_mwnd_print_if_needed(ctx: &mut CommandContext, m: &mut MwndState) {
    if !ctx.globals.script.async_msg_mode && !m.multi_msg {
        set_mwnd_key_icon_wait(ctx, m, 0);
        ctx.wait.wait_key();
        mark_mwnd_clear_ready(ctx, m);
    }
}

pub fn cd_text_current_mwnd(ctx: &mut CommandContext, text: &str, _rf_flag_no: i64) -> bool {
    let (form_id, stage_idx, mwnd_idx) = current_mwnd_target(ctx);
    with_stage_state(ctx, form_id, |ctx, st| {
        ensure_mwnd(ctx, st, stage_idx, mwnd_idx);
        let Some(list) = st.mwnd_lists.get_mut(&stage_idx) else {
            return false;
        };
        let Some(m) = list.get_mut(mwnd_idx) else {
            return false;
        };

        if !text.is_empty() {
            start_mwnd_msg_block_if_needed(ctx, m);
            m.msg_text.push_str(text);
            start_mwnd_auto_message(ctx, m);
            ctx.ui.append_message(text);
            msgbk_add_text(ctx, text);
            m.text_dirty = true;
            wait_after_mwnd_print_if_needed(ctx, m);
        }
        true
    })
}

pub fn cd_name_current_mwnd(ctx: &mut CommandContext, name: &str) -> bool {
    let (form_id, stage_idx, mwnd_idx) = current_mwnd_target(ctx);
    with_stage_state(ctx, form_id, |ctx, st| {
        ensure_mwnd(ctx, st, stage_idx, mwnd_idx);
        let Some(list) = st.mwnd_lists.get_mut(&stage_idx) else {
            return false;
        };
        let Some(m) = list.get_mut(mwnd_idx) else {
            return false;
        };

        start_mwnd_msg_block_if_needed(ctx, m);
        if m.name_text.is_empty() {
            let resolved_name = resolve_gameexe_namae(&ctx.tables, name);
            let display_name = resolved_name.display;
            m.chara_color_mod = resolved_name.color_mod;
            m.chara_moji_color = resolved_name.moji_color_no;
            m.chara_shadow_color = resolved_name.shadow_color_no;
            m.chara_fuchi_color = resolved_name.fuchi_color_no;
            m.name_text = display_name.clone();
            ctx.ui.set_name(display_name.clone());
            if !display_name.is_empty() {
                msgbk_add_name(ctx, &display_name);
            }
        }
        true
    })
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
        } else if tail.len() == 2
            && (tail[0] == -1
                || tail[0] == ctx.ids.elm_array
                || tail[0] == super::codes::ELM_ARRAY)
        {
            let child_idx = tail[1] as i64;
            let element_prefix = mwnd_embedded_object_prefix(ctx, stage_idx, mwnd_idx, op, child_idx);
            dispatch_embedded_object_item_ref(
                ctx,
                st,
                stage_idx,
                &mut child_list,
                strict,
                child_idx,
                ret_form,
                al_id,
                &format!("mwnd_{selector_key}_{stage_idx}_{mwnd_idx}"),
                element_prefix,
            )
        } else if tail.len() == 1 {
            dispatch_embedded_object_list_op(
                ctx,
                stage_idx,
                &mut child_list,
                &mut strict,
                tail[0],
                script_args,
                ret_form,
            )
            .unwrap_or(false)
        } else {
            let (child_idx, child_op, child_tail) = if tail.len() >= 3
                && (tail[0] == -1
                    || tail[0] == ctx.ids.elm_array
                    || tail[0] == super::codes::ELM_ARRAY)
            {
                (tail[1] as i64, tail[2], &tail[3..])
            } else if tail.len() >= 2 {
                (tail[0] as i64, tail[1], &tail[2..])
            } else {
                (0, 0, &tail[0..0])
            };
            if child_tail.is_empty() && tail.len() < 2 {
                false
            } else {
                let element_prefix = mwnd_embedded_object_prefix(ctx, stage_idx, mwnd_idx, op, child_idx);
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
                    Some(element_prefix),
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

    match k {
        MwndOpKind::SetWaku => {
            let requested = script_args.first().and_then(Value::as_i64);
            apply_mwnd_waku_from_gameexe(ctx, st, stage_idx, mwnd_idx, requested);
            push_ok(ctx, ret_form);
            return true;
        }
        MwndOpKind::InitWakuFile => {
            apply_mwnd_waku_from_gameexe(ctx, st, stage_idx, mwnd_idx, None);
            push_ok(ctx, ret_form);
            return true;
        }
        MwndOpKind::ClearFace => {
            let mut face_list = {
                let list = st.mwnd_lists.get_mut(&stage_idx).unwrap();
                let m = &mut list[mwnd_idx];
                m.face_file.clear();
                m.face_no = 0;
                std::mem::take(&mut m.face_list)
            };
            for obj in &mut face_list {
                if let Some(slot) = obj.nested_runtime_slot {
                    object_clear_backend(ctx, obj, stage_idx, slot);
                }
                let slot = obj.nested_runtime_slot;
                *obj = ObjectState::default();
                obj.nested_runtime_slot = slot;
            }
            {
                let list = st.mwnd_lists.get_mut(&stage_idx).unwrap();
                list[mwnd_idx].face_list = face_list;
            }
            push_ok(ctx, ret_form);
            return true;
        }
        MwndOpKind::SetFace => {
            let face_file = rhs
                .and_then(|v| v.as_str())
                .or_else(|| script_args.iter().find_map(|v| v.as_str()))
                .unwrap_or("")
                .to_string();
            let mut ints = script_args.iter().filter_map(Value::as_i64);
            let face_no = ints.next().unwrap_or(0);
            let face_idx = if face_no < 0 { None } else { Some(face_no as usize) };
            let mut face_list = {
                let list = st.mwnd_lists.get_mut(&stage_idx).unwrap();
                let m = &mut list[mwnd_idx];
                m.face_no = face_no;
                m.face_file = face_file.clone();
                if !face_file.is_empty() {
                    m.aux_str_props.insert(op, face_file.clone());
                }
                m.props.insert(op, face_no);
                if let Some(idx) = face_idx {
                    if m.face_list.len() <= idx {
                        m.face_list.resize_with(idx + 1, ObjectState::default);
                    }
                }
                std::mem::take(&mut m.face_list)
            };
            if let Some(idx) = face_idx {
                create_mwnd_face_object(
                    ctx,
                    st,
                    stage_idx,
                    mwnd_idx,
                    idx,
                    &face_file,
                    &mut face_list[idx],
                );
            }
            {
                let list = st.mwnd_lists.get_mut(&stage_idx).unwrap();
                list[mwnd_idx].face_list = face_list;
            }
            push_ok(ctx, ret_form);
            return true;
        }
        _ => {}
    }

    let list = st.mwnd_lists.get_mut(&stage_idx).unwrap();
    let m = &mut list[mwnd_idx];

    match k {
        MwndOpKind::MsgBlock => {
            start_mwnd_msg_block_if_needed(ctx, m);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::Open => {
            m.open = true;
            m.text_dirty = false;
            ctx.ui.show_message_bg(true);
            ctx.ui.begin_mwnd_open(m.open_anime_type, m.open_anime_time);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::Close => {
            m.open = false;
            m.key_icon_appear = false;
            m.key_icon_pos = None;
            ctx.ui.show_message_bg(false);
            m.multi_msg = false;
            m.text_dirty = false;
            m.clear_ready = false;
            m.msg_block_started = false;
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
            mark_mwnd_clear_ready(ctx, m);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::NovelClear => {
            m.msg_text.clear();
            m.key_icon_appear = false;
            m.key_icon_pos = None;
            ctx.ui.clear_message();
            m.msg_text.push('\n');
            m.multi_msg = false;
            m.text_dirty = false;
            m.clear_ready = false;
            m.msg_block_started = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::NewLineNoIndent => {
            m.msg_text.push('\n');
            ctx.ui.append_linebreak();
            m.indent = false;
            m.text_dirty = true;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::NewLineIndent => {
            m.msg_text.push('\n');
            ctx.ui.append_linebreak();
            m.text_dirty = true;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::Print => {
            ctx.request_read_flag_no();
            let msg = rhs
                .and_then(|v| v.as_str())
                .or_else(|| script_args.iter().find_map(|v| v.as_str()))
                .unwrap_or("");
            if !msg.is_empty() {
                m.msg_text.push_str(msg);
                start_mwnd_auto_message(ctx, m);
                ctx.ui.append_message(msg);
                msgbk_add_text(ctx, msg);
                m.text_dirty = true;
                wait_after_mwnd_print_if_needed(ctx, m);
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
                start_mwnd_auto_message(ctx, m);
                ctx.ui.append_message(msg);
                msgbk_add_text(ctx, msg);
                m.text_dirty = true;
                wait_after_mwnd_print_if_needed(ctx, m);
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
            set_mwnd_key_icon_wait(ctx, m, 0);
            ctx.wait.wait_key();
            m.text_dirty = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::Pp => {
            set_mwnd_key_icon_wait(ctx, m, 0);
            ctx.wait.wait_key();
            m.text_dirty = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::R => {
            msgbk_next(ctx);
            set_mwnd_key_icon_wait(ctx, m, 0);
            ctx.ui.request_clear_message_on_wait_end();
            ctx.wait.wait_key();
            m.text_dirty = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::PageWait => {
            // Treat a page wait as a message boundary for backlog purposes.
            msgbk_next(ctx);
            set_mwnd_key_icon_wait(ctx, m, 1);
            ctx.ui.request_clear_message_on_wait_end();
            ctx.wait.wait_key();
            m.text_dirty = false;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::SetName => {
            if !m.name_text.is_empty() {
                push_ok(ctx, ret_form);
                return true;
            }
            let s = rhs
                .and_then(|v| v.as_str())
                .or_else(|| script_args.iter().find_map(|v| v.as_str()))
                .unwrap_or("");
            let resolved_name = resolve_gameexe_namae(&ctx.tables, s);
            let display_name = resolved_name.display.clone();
            m.chara_color_mod = resolved_name.color_mod;
            m.chara_moji_color = resolved_name.moji_color_no;
            m.chara_shadow_color = resolved_name.shadow_color_no;
            m.chara_fuchi_color = resolved_name.fuchi_color_no;
            m.name_text = display_name.clone();
            ctx.ui.set_name(display_name.clone());
            if !display_name.is_empty() {
                msgbk_add_name(ctx, &display_name);
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
            ctx.request_read_flag_no();
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
            // The return value is pushed by the runtime input bridge on decide/cancel.
            ctx.wait.wait_key();
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
            ctx.request_read_flag_no();
            let is_ex_koe = matches!(
                op,
                constants::MWND_EXKOE
                    | constants::MWND_EXKOE_PLAY_WAIT
                    | constants::MWND_EXKOE_PLAY_WAIT_KEY
            );
            let koe_no = if is_ex_koe {
                named_i64(script_args, 0).or_else(|| positional_i64(script_args, 0))
            } else {
                positional_i64(script_args, 0)
            }
            .unwrap_or(0);
            let chara_no = if is_ex_koe {
                named_i64(script_args, 1).or_else(|| positional_i64(script_args, 1))
            } else {
                positional_i64(script_args, 1)
            }
            .unwrap_or(-1);
            m.koe = Some((koe_no, chara_no));
            if let Err(err) = {
                let (koe, audio) = (&mut ctx.koe, &mut ctx.audio);
                koe.play_koe_no(audio, koe_no)
            } {
                eprintln!("[SG_AUDIO] mwnd.koe failed koe_no={koe_no}: {err:#}");
            }
            msgbk_add_koe(ctx, koe_no, chara_no);
            let ex_wait = is_ex_koe && named_i64(script_args, 2).unwrap_or(0) != 0;
            let ex_key_skip = is_ex_koe && named_i64(script_args, 3).unwrap_or(0) != 0;
            match k {
                MwndOpKind::KoePlayWait => {
                    ctx.wait
                        .wait_audio(crate::runtime::wait::AudioWait::KoeAny, false);
                }
                MwndOpKind::KoePlayWaitKey => {
                    ctx.wait
                        .wait_audio(crate::runtime::wait::AudioWait::KoeAny, true);
                }
                _ if ex_wait => {
                    ctx.wait
                        .wait_audio(crate::runtime::wait::AudioWait::KoeAny, ex_key_skip);
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
            let t = ctx.tables.mwnd_templates.get(mwnd_idx).cloned().unwrap_or_default();
            m.open_anime_type = t.open_anime_type;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::InitOpenAnimeTime => {
            let t = ctx.tables.mwnd_templates.get(mwnd_idx).cloned().unwrap_or_default();
            m.open_anime_time = t.open_anime_time;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::InitCloseAnimeType => {
            let t = ctx.tables.mwnd_templates.get(mwnd_idx).cloned().unwrap_or_default();
            m.close_anime_type = t.close_anime_type;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::InitCloseAnimeTime => {
            let t = ctx.tables.mwnd_templates.get(mwnd_idx).cloned().unwrap_or_default();
            m.close_anime_time = t.close_anime_time;
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
            let t = ctx.tables.mwnd_templates.get(mwnd_idx).cloned().unwrap_or_default();
            ctx.stack.push(Value::Int(t.open_anime_type));
            true
        }
        MwndOpKind::GetDefaultOpenAnimeTime => {
            let t = ctx.tables.mwnd_templates.get(mwnd_idx).cloned().unwrap_or_default();
            ctx.stack.push(Value::Int(t.open_anime_time));
            true
        }
        MwndOpKind::GetDefaultCloseAnimeType => {
            let t = ctx.tables.mwnd_templates.get(mwnd_idx).cloned().unwrap_or_default();
            ctx.stack.push(Value::Int(t.close_anime_type));
            true
        }
        MwndOpKind::GetDefaultCloseAnimeTime => {
            let t = ctx.tables.mwnd_templates.get(mwnd_idx).cloned().unwrap_or_default();
            ctx.stack.push(Value::Int(t.close_anime_time));
            true
        }
        MwndOpKind::ClearName => {
            m.name_text.clear();
            m.chara_color_mod = None;
            m.chara_moji_color = None;
            m.chara_shadow_color = None;
            m.chara_fuchi_color = None;
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::GetName => {
            ctx.stack.push(Value::Str(m.name_text.clone()));
            true
        }
        MwndOpKind::SetWaku => {
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::InitWakuFile => {
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
            let t = ctx.tables.mwnd_templates.get(mwnd_idx).cloned().unwrap_or_default();
            m.window_pos = Some(t.window_pos);
            push_ok(ctx, ret_form);
            true
        }
        MwndOpKind::InitWindowSize => {
            let t = ctx.tables.mwnd_templates.get(mwnd_idx).cloned().unwrap_or_default();
            m.window_size = Some(t.window_size);
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
            let t = ctx.tables.mwnd_templates.get(mwnd_idx).cloned().unwrap_or_default();
            m.window_moji_cnt = Some(t.moji_cnt);
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

fn stage_element_prefix(ctx: &CommandContext, stage_idx: i64) -> Vec<i32> {
    vec![ctx.ids.form_global_stage as i32, ctx.ids.elm_array, stage_idx as i32]
}

fn mwnd_embedded_object_prefix(
    ctx: &CommandContext,
    stage_idx: i64,
    mwnd_idx: usize,
    selector_op: i32,
    child_idx: i64,
) -> Vec<i32> {
    let mut prefix = stage_element_prefix(ctx, stage_idx);
    prefix.push(crate::runtime::forms::codes::STAGE_ELM_MWND);
    prefix.push(ctx.ids.elm_array);
    prefix.push(mwnd_idx as i32);
    prefix.push(selector_op);
    prefix.push(ctx.ids.elm_array);
    prefix.push(child_idx.max(0) as i32);
    prefix
}

fn btnselitem_embedded_object_prefix(
    ctx: &CommandContext,
    stage_idx: i64,
    item_idx: usize,
    child_idx: i64,
) -> Vec<i32> {
    let mut prefix = stage_element_prefix(ctx, stage_idx);
    prefix.push(crate::runtime::forms::codes::STAGE_ELM_BTNSELITEM);
    prefix.push(ctx.ids.elm_array);
    prefix.push(item_idx as i32);
    prefix.push(crate::runtime::forms::codes::ELM_BTNSELITEM_OBJECT);
    prefix.push(ctx.ids.elm_array);
    prefix.push(child_idx.max(0) as i32);
    prefix
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
    } else if tail.len() == 2
        && (tail[0] == -1 || tail[0] == ctx.ids.elm_array || tail[0] == super::codes::ELM_ARRAY)
    {
        let child_idx = tail[1] as i64;
        let element_prefix = btnselitem_embedded_object_prefix(ctx, stage_idx, item_idx, child_idx);
        dispatch_embedded_object_item_ref(
            ctx,
            st,
            stage_idx,
            &mut child_list,
            strict,
            child_idx,
            ret_form,
            al_id,
            &format!("btnselitem_{}_{}_{}", stage_idx, item_idx, op),
            element_prefix,
        )
    } else if tail.len() == 1 {
        dispatch_embedded_object_list_op(
            ctx,
            stage_idx,
            &mut child_list,
            &mut strict,
            tail[0],
            script_args,
            ret_form,
        )
        .unwrap_or(false)
    } else {
        let (child_idx, child_op, child_tail) = if tail.len() >= 3
            && (tail[0] == -1 || tail[0] == ctx.ids.elm_array || tail[0] == super::codes::ELM_ARRAY)
        {
            (tail[1] as i64, tail[2], &tail[3..])
        } else if tail.len() >= 2 {
            (tail[0] as i64, tail[1], &tail[2..])
        } else {
            (0, 0, &tail[0..0])
        };
        if child_tail.is_empty() && tail.len() < 2 {
            false
        } else {
            let element_prefix = btnselitem_embedded_object_prefix(ctx, stage_idx, item_idx, child_idx);
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
                Some(element_prefix),
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
        if args.len() == 1 {
            args.first()
        } else if chain_pos == args.len() {
            args.last()
        } else if chain_pos >= 3 && args.get(1).and_then(as_i64).is_some() {
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

    // Command arguments are the original script arguments preceding the element chain.
    let script_args = crate::runtime::forms::prop_access::script_args(args, chain_pos);

    if sg_debug_enabled_local() {
        sg_debug_stage(format!(
            "chain={:?} target={:?} al_id={:?} ret_form={:?} chain_pos={} argc={} script_args={:?} rhs={:?}",
            chain,
            tgt,
            al_id,
            ret_form,
            chain_pos,
            script_args.len(),
            script_args,
            rhs,
        ));
    }

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
                    resize_stage_object_list_like_cpp(ctx, st, stage, n);
                    ctx.stack.push(Value::Int(0));
                }
                1 => {
                    let n = script_args.first().and_then(as_i64).unwrap_or(0).max(0) as usize;
                    sg_debug_stage(format!("stage={} CREATE_MWND resize {}", stage, n));
                    let old_len = st.mwnd_lists.get(&stage).map(|v| v.len()).unwrap_or(0);
                    if n < old_len {
                        if let Some(list) = st.mwnd_lists.get_mut(&stage) {
                            for i in n..old_len {
                                clear_mwnd_embedded_objects_for_stage_wipe(ctx, &mut list[i], stage);
                            }
                        }
                    }
                    st.ensure_mwnd_list(stage, n);
                    for i in 0..n {
                        ensure_mwnd(ctx, st, stage, i);
                    }
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
                } else if child == crate::runtime::forms::codes::STAGE_ELM_EFFECT {
                    dispatch_stage_effect_list_op(ctx, st, stage, op as i32, script_args, ret_form)
                } else if child == crate::runtime::forms::codes::STAGE_ELM_QUAKE {
                    false
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
                    let prev_chain = ctx.globals.current_object_chain.clone();
                    ctx.globals.current_object_chain = Some(vec![
                        form_id as i32,
                        ctx.ids.elm_array,
                        stage as i32,
                        stage_object,
                        ctx.ids.elm_array,
                        idx as i32,
                    ]);
                    let handled = dispatch_object_op(
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
                    );
                    ctx.globals.current_object_chain = prev_chain;
                    handled
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
                } else if child == crate::runtime::forms::codes::STAGE_ELM_EFFECT {
                    dispatch_stage_effect_item_op(
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
                } else if child == crate::runtime::forms::codes::STAGE_ELM_QUAKE {
                    dispatch_stage_quake_item_op(
                        ctx,
                        st,
                        stage,
                        idx.max(0) as usize,
                        op as i32,
                        script_args,
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
                ctx.globals.current_object_chain = Some(element_chain.clone());
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
