//! VM wait/blocking state.
//!
//! The original engine has many commands/forms that block execution until:
//! - a certain time passes, or
//! - the user presses a key / clicks.
//!
//! Cross-platform blocking and wait model.

use std::time::{Duration, Instant};

use crate::audio::{BgmEngine, KoeEngine, PcmEngine, SeEngine};

use super::constants::RuntimeConstants;
use super::globals::{GlobalState, ObjectState, StageFormState};
use super::int_event::IntEvent;
use super::Value;

fn anim_skip_trace_enabled() -> bool {
    std::env::var_os("SG_DEBUG").is_some()
}

fn anim_skip_trace(msg: impl AsRef<str>) {
    if anim_skip_trace_enabled() {
        eprintln!("[SG_DEBUG][ANIM_SKIP_TRACE][WAIT] {}", msg.as_ref());
    }
}

fn int_event_state(ev: &IntEvent) -> String {
    format!(
        "def={} value={} cur={} start={} end={} cur_time={} end_time={} delay={} loop_type={} speed={} real={} active={}",
        ev.def_value,
        ev.value,
        ev.cur_value,
        ev.start_value,
        ev.end_value,
        ev.cur_time,
        ev.end_time,
        ev.delay_time,
        ev.loop_type,
        ev.speed_type,
        ev.real_flag,
        ev.check_event(),
    )
}

fn object_event_op_name(ids: &RuntimeConstants, op: i32) -> &'static str {
    if ids.obj_patno_eve != 0 && op == ids.obj_patno_eve { return "PATNO_EVE"; }
    if ids.obj_x_eve != 0 && op == ids.obj_x_eve { return "X_EVE"; }
    if ids.obj_y_eve != 0 && op == ids.obj_y_eve { return "Y_EVE"; }
    if ids.obj_z_eve != 0 && op == ids.obj_z_eve { return "Z_EVE"; }
    if ids.obj_center_x_eve != 0 && op == ids.obj_center_x_eve { return "CENTER_X_EVE"; }
    if ids.obj_center_y_eve != 0 && op == ids.obj_center_y_eve { return "CENTER_Y_EVE"; }
    if ids.obj_center_z_eve != 0 && op == ids.obj_center_z_eve { return "CENTER_Z_EVE"; }
    if ids.obj_center_rep_x_eve != 0 && op == ids.obj_center_rep_x_eve { return "CENTER_REP_X_EVE"; }
    if ids.obj_center_rep_y_eve != 0 && op == ids.obj_center_rep_y_eve { return "CENTER_REP_Y_EVE"; }
    if ids.obj_center_rep_z_eve != 0 && op == ids.obj_center_rep_z_eve { return "CENTER_REP_Z_EVE"; }
    if ids.obj_scale_x_eve != 0 && op == ids.obj_scale_x_eve { return "SCALE_X_EVE"; }
    if ids.obj_scale_y_eve != 0 && op == ids.obj_scale_y_eve { return "SCALE_Y_EVE"; }
    if ids.obj_scale_z_eve != 0 && op == ids.obj_scale_z_eve { return "SCALE_Z_EVE"; }
    if ids.obj_rotate_x_eve != 0 && op == ids.obj_rotate_x_eve { return "ROTATE_X_EVE"; }
    if ids.obj_rotate_y_eve != 0 && op == ids.obj_rotate_y_eve { return "ROTATE_Y_EVE"; }
    if ids.obj_rotate_z_eve != 0 && op == ids.obj_rotate_z_eve { return "ROTATE_Z_EVE"; }
    if ids.obj_clip_left_eve != 0 && op == ids.obj_clip_left_eve { return "CLIP_LEFT_EVE"; }
    if ids.obj_clip_top_eve != 0 && op == ids.obj_clip_top_eve { return "CLIP_TOP_EVE"; }
    if ids.obj_clip_right_eve != 0 && op == ids.obj_clip_right_eve { return "CLIP_RIGHT_EVE"; }
    if ids.obj_clip_bottom_eve != 0 && op == ids.obj_clip_bottom_eve { return "CLIP_BOTTOM_EVE"; }
    if ids.obj_src_clip_left_eve != 0 && op == ids.obj_src_clip_left_eve { return "SRC_CLIP_LEFT_EVE"; }
    if ids.obj_src_clip_top_eve != 0 && op == ids.obj_src_clip_top_eve { return "SRC_CLIP_TOP_EVE"; }
    if ids.obj_src_clip_right_eve != 0 && op == ids.obj_src_clip_right_eve { return "SRC_CLIP_RIGHT_EVE"; }
    if ids.obj_src_clip_bottom_eve != 0 && op == ids.obj_src_clip_bottom_eve { return "SRC_CLIP_BOTTOM_EVE"; }
    if ids.obj_tr_eve != 0 && op == ids.obj_tr_eve { return "TR_EVE"; }
    if ids.obj_mono_eve != 0 && op == ids.obj_mono_eve { return "MONO_EVE"; }
    if ids.obj_reverse_eve != 0 && op == ids.obj_reverse_eve { return "REVERSE_EVE"; }
    if ids.obj_bright_eve != 0 && op == ids.obj_bright_eve { return "BRIGHT_EVE"; }
    if ids.obj_dark_eve != 0 && op == ids.obj_dark_eve { return "DARK_EVE"; }
    if ids.obj_color_r_eve != 0 && op == ids.obj_color_r_eve { return "COLOR_R_EVE"; }
    if ids.obj_color_g_eve != 0 && op == ids.obj_color_g_eve { return "COLOR_G_EVE"; }
    if ids.obj_color_b_eve != 0 && op == ids.obj_color_b_eve { return "COLOR_B_EVE"; }
    if ids.obj_color_rate_eve != 0 && op == ids.obj_color_rate_eve { return "COLOR_RATE_EVE"; }
    if ids.obj_color_add_r_eve != 0 && op == ids.obj_color_add_r_eve { return "COLOR_ADD_R_EVE"; }
    if ids.obj_color_add_g_eve != 0 && op == ids.obj_color_add_g_eve { return "COLOR_ADD_G_EVE"; }
    if ids.obj_color_add_b_eve != 0 && op == ids.obj_color_add_b_eve { return "COLOR_ADD_B_EVE"; }
    if ids.obj_x_rep_eve != 0 && op == ids.obj_x_rep_eve { return "X_REP_EVE"; }
    if ids.obj_y_rep_eve != 0 && op == ids.obj_y_rep_eve { return "Y_REP_EVE"; }
    if ids.obj_z_rep_eve != 0 && op == ids.obj_z_rep_eve { return "Z_REP_EVE"; }
    if ids.obj_tr_rep_eve != 0 && op == ids.obj_tr_rep_eve { return "TR_REP_EVE"; }
    "UNKNOWN_EVE"
}

#[derive(Debug, Clone, Copy)]
pub enum AudioWait {
    Bgm,
    BgmFade,
    KoeAny,
    SeAny,
    PcmAny,
    PcmSlot(u8),
}

#[derive(Debug, Clone)]
pub enum EventWait {
    ObjectAll {
        stage_form_id: u32,
        stage_idx: i64,
        runtime_slot: usize,
    },
    ObjectOne {
        stage_form_id: u32,
        stage_idx: i64,
        runtime_slot: usize,
        op: i32,
    },
    ObjectList {
        stage_form_id: u32,
        stage_idx: i64,
        runtime_slot: usize,
        list_op: i32,
        list_idx: usize,
    },
    GenericIntEvent {
        form_id: u32,
        index: Option<usize>,
    },
    FogX,
    CounterThreshold {
        form_id: u32,
        index: usize,
        target: i64,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct MovieWait {
    pub stage_form_id: u32,
    pub stage_idx: i64,
    pub runtime_slot: usize,
    pub return_value_flag: bool,
}

fn object_runtime_slot(idx: usize, obj: &ObjectState) -> usize {
    obj.runtime_slot_or(idx)
}

fn find_object_by_runtime_slot<'a>(
    objects: &'a [ObjectState],
    runtime_slot: usize,
) -> Option<&'a ObjectState> {
    for (idx, obj) in objects.iter().enumerate() {
        if object_runtime_slot(idx, obj) == runtime_slot {
            return Some(obj);
        }
        if let Some(found) = find_object_by_runtime_slot(&obj.runtime.child_objects, runtime_slot) {
            return Some(found);
        }
    }
    None
}

fn find_object_by_runtime_slot_mut<'a>(
    mut objects: &'a mut [ObjectState],
    runtime_slot: usize,
) -> Option<&'a mut ObjectState> {
    let mut idx = 0usize;
    while let Some((obj, tail)) = objects.split_first_mut() {
        if object_runtime_slot(idx, obj) == runtime_slot {
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

fn object_event_list_for_wait<'a>(
    obj: &'a ObjectState,
    ids: &RuntimeConstants,
    op: i32,
) -> Option<&'a Vec<IntEvent>> {
    obj.int_event_list_by_op(ids, op)
        .or_else(|| obj.rep_int_event_list_by_rep_op(ids, op))
}

fn object_event_list_for_wait_mut<'a>(
    obj: &'a mut ObjectState,
    ids: &RuntimeConstants,
    op: i32,
) -> Option<&'a mut Vec<IntEvent>> {
    if ids.obj_x_rep_eve != 0 && op == ids.obj_x_rep_eve {
        Some(&mut obj.runtime.prop_event_lists.x_rep)
    } else if ids.obj_y_rep_eve != 0 && op == ids.obj_y_rep_eve {
        Some(&mut obj.runtime.prop_event_lists.y_rep)
    } else if ids.obj_z_rep_eve != 0 && op == ids.obj_z_rep_eve {
        Some(&mut obj.runtime.prop_event_lists.z_rep)
    } else if ids.obj_tr_rep_eve != 0 && op == ids.obj_tr_rep_eve {
        Some(&mut obj.runtime.prop_event_lists.tr_rep)
    } else if ids.obj_x_rep != 0 && op == ids.obj_x_rep {
        Some(&mut obj.runtime.prop_event_lists.x_rep)
    } else if ids.obj_y_rep != 0 && op == ids.obj_y_rep {
        Some(&mut obj.runtime.prop_event_lists.y_rep)
    } else if ids.obj_z_rep != 0 && op == ids.obj_z_rep {
        Some(&mut obj.runtime.prop_event_lists.z_rep)
    } else if ids.obj_tr_rep != 0 && op == ids.obj_tr_rep {
        Some(&mut obj.runtime.prop_event_lists.tr_rep)
    } else {
        None
    }
}

fn object_active_in_stage_state_by_runtime_slot(
    st: &StageFormState,
    stage_idx: i64,
    runtime_slot: usize,
) -> Option<&ObjectState> {
    if let Some(obj) = st
        .object_lists
        .get(&stage_idx)
        .and_then(|list| find_object_by_runtime_slot(list, runtime_slot))
    {
        return Some(obj);
    }

    if let Some(items) = st.btnselitem_lists.get(&stage_idx) {
        for item in items {
            if let Some(obj) = find_object_by_runtime_slot(&item.object_list, runtime_slot) {
                return Some(obj);
            }
        }
    }

    if let Some(mwnds) = st.mwnd_lists.get(&stage_idx) {
        for mwnd in mwnds {
            if let Some(obj) = find_object_by_runtime_slot(&mwnd.button_list, runtime_slot) {
                return Some(obj);
            }
            if let Some(obj) = find_object_by_runtime_slot(&mwnd.face_list, runtime_slot) {
                return Some(obj);
            }
            if let Some(obj) = find_object_by_runtime_slot(&mwnd.object_list, runtime_slot) {
                return Some(obj);
            }
        }
    }

    None
}

fn object_active_by_runtime_slot(
    globals: &GlobalState,
    stage_form_id: u32,
    stage_idx: i64,
    runtime_slot: usize,
) -> Option<&ObjectState> {
    globals
        .stage_forms
        .get(&stage_form_id)
        .and_then(|st| object_active_in_stage_state_by_runtime_slot(st, stage_idx, runtime_slot))
}

fn find_object_by_runtime_slot_mut_ptr(
    objects: &mut [ObjectState],
    runtime_slot: usize,
) -> Option<*mut ObjectState> {
    find_object_by_runtime_slot_mut(objects, runtime_slot).map(|obj| obj as *mut ObjectState)
}

fn object_active_by_runtime_slot_mut(
    globals: &mut GlobalState,
    stage_form_id: u32,
    stage_idx: i64,
    runtime_slot: usize,
) -> Option<&mut ObjectState> {
    let st = globals.stage_forms.get_mut(&stage_form_id)?;

    if let Some(ptr) = st
        .object_lists
        .get_mut(&stage_idx)
        .and_then(|list| find_object_by_runtime_slot_mut_ptr(list, runtime_slot))
    {
        return unsafe { Some(&mut *ptr) };
    }

    if let Some(items) = st.btnselitem_lists.get_mut(&stage_idx) {
        for item in items {
            if let Some(ptr) =
                find_object_by_runtime_slot_mut_ptr(&mut item.object_list, runtime_slot)
            {
                return unsafe { Some(&mut *ptr) };
            }
        }
    }

    if let Some(mwnds) = st.mwnd_lists.get_mut(&stage_idx) {
        for mwnd in mwnds {
            if let Some(ptr) =
                find_object_by_runtime_slot_mut_ptr(&mut mwnd.button_list, runtime_slot)
            {
                return unsafe { Some(&mut *ptr) };
            }
            if let Some(ptr) =
                find_object_by_runtime_slot_mut_ptr(&mut mwnd.face_list, runtime_slot)
            {
                return unsafe { Some(&mut *ptr) };
            }
            if let Some(ptr) =
                find_object_by_runtime_slot_mut_ptr(&mut mwnd.object_list, runtime_slot)
            {
                return unsafe { Some(&mut *ptr) };
            }
        }
    }

    None
}

fn finish_wait_skipped_event(ev: &mut IntEvent) {
    let before = if anim_skip_trace_enabled() {
        Some(int_event_state(ev))
    } else {
        None
    };
    ev.end_event();
    ev.frame();
    if let Some(before) = before {
        anim_skip_trace(format!(
            "finish_event before=[{}] after=[{}]",
            before,
            int_event_state(ev)
        ));
    }
}

fn event_prop_pairs(ids: &RuntimeConstants) -> [(i32, i32); 36] {
    [
        (ids.obj_patno_eve, ids.obj_patno),
        (ids.obj_x_eve, ids.obj_x),
        (ids.obj_y_eve, ids.obj_y),
        (ids.obj_z_eve, ids.obj_z),
        (ids.obj_center_x_eve, ids.obj_center_x),
        (ids.obj_center_y_eve, ids.obj_center_y),
        (ids.obj_center_z_eve, ids.obj_center_z),
        (ids.obj_center_rep_x_eve, ids.obj_center_rep_x),
        (ids.obj_center_rep_y_eve, ids.obj_center_rep_y),
        (ids.obj_center_rep_z_eve, ids.obj_center_rep_z),
        (ids.obj_scale_x_eve, ids.obj_scale_x),
        (ids.obj_scale_y_eve, ids.obj_scale_y),
        (ids.obj_scale_z_eve, ids.obj_scale_z),
        (ids.obj_rotate_x_eve, ids.obj_rotate_x),
        (ids.obj_rotate_y_eve, ids.obj_rotate_y),
        (ids.obj_rotate_z_eve, ids.obj_rotate_z),
        (ids.obj_clip_left_eve, ids.obj_clip_left),
        (ids.obj_clip_top_eve, ids.obj_clip_top),
        (ids.obj_clip_right_eve, ids.obj_clip_right),
        (ids.obj_clip_bottom_eve, ids.obj_clip_bottom),
        (ids.obj_src_clip_left_eve, ids.obj_src_clip_left),
        (ids.obj_src_clip_top_eve, ids.obj_src_clip_top),
        (ids.obj_src_clip_right_eve, ids.obj_src_clip_right),
        (ids.obj_src_clip_bottom_eve, ids.obj_src_clip_bottom),
        (ids.obj_tr_eve, ids.obj_tr),
        (ids.obj_mono_eve, ids.obj_mono),
        (ids.obj_reverse_eve, ids.obj_reverse),
        (ids.obj_bright_eve, ids.obj_bright),
        (ids.obj_dark_eve, ids.obj_dark),
        (ids.obj_color_r_eve, ids.obj_color_r),
        (ids.obj_color_g_eve, ids.obj_color_g),
        (ids.obj_color_b_eve, ids.obj_color_b),
        (ids.obj_color_rate_eve, ids.obj_color_rate),
        (ids.obj_color_add_r_eve, ids.obj_color_add_r),
        (ids.obj_color_add_g_eve, ids.obj_color_add_g),
        (ids.obj_color_add_b_eve, ids.obj_color_add_b),
    ]
}

fn object_prop_op_for_event_op(ids: &RuntimeConstants, event_op: i32) -> Option<i32> {
    event_prop_pairs(ids)
        .into_iter()
        .find_map(|(ev_op, prop_op)| (ev_op != 0 && event_op == ev_op).then_some(prop_op))
}

fn finish_wait_skipped_object_event_by_op(
    obj: &mut ObjectState,
    ids: &RuntimeConstants,
    event_op: i32,
) {
    let file = obj.file_name.as_deref().unwrap_or("-").to_string();
    let runtime_slot = obj.runtime_slot_or(usize::MAX);
    let event_name = object_event_op_name(ids, event_op);
    let Some(value) = obj.int_event_by_op_mut(ids, event_op).map(|ev| {
        anim_skip_trace(format!(
            "finish_object_event begin slot={} file={} op={}({}) state=[{}]",
            runtime_slot,
            file,
            event_op,
            event_name,
            int_event_state(ev)
        ));
        finish_wait_skipped_event(ev);
        anim_skip_trace(format!(
            "finish_object_event event_done slot={} file={} op={}({}) state=[{}]",
            runtime_slot,
            file,
            event_op,
            event_name,
            int_event_state(ev)
        ));
        ev.get_total_value() as i64
    }) else {
        anim_skip_trace(format!(
            "finish_object_event missing slot={} file={} op={}({})",
            runtime_slot, file, event_op, event_name
        ));
        return;
    };
    if let Some(prop_op) = object_prop_op_for_event_op(ids, event_op) {
        obj.set_int_prop(ids, prop_op, value);
        anim_skip_trace(format!(
            "finish_object_event prop_write slot={} file={} event_op={}({}) prop_op={} value={} obj_tr={} obj_alpha={} obj_pos=({}, {})",
            runtime_slot,
            file,
            event_op,
            event_name,
            prop_op,
            value,
            obj.get_int_prop(ids, ids.obj_tr),
            obj.base.alpha,
            obj.get_int_prop(ids, ids.obj_x),
            obj.get_int_prop(ids, ids.obj_y),
        ));
    } else {
        anim_skip_trace(format!(
            "finish_object_event no_prop_map slot={} file={} event_op={}({}) value={}",
            runtime_slot, file, event_op, event_name, value
        ));
    }
}

fn finish_wait_skipped_object_events(obj: &mut ObjectState, ids: &RuntimeConstants) {
    let file = obj.file_name.as_deref().unwrap_or("-").to_string();
    let runtime_slot = obj.runtime_slot_or(usize::MAX);
    anim_skip_trace(format!(
        "finish_object_all begin slot={} file={} any_active={} tr={} alpha={} pos=({}, {})",
        runtime_slot,
        file,
        obj.any_event_active(),
        obj.get_int_prop(ids, ids.obj_tr),
        obj.base.alpha,
        obj.get_int_prop(ids, ids.obj_x),
        obj.get_int_prop(ids, ids.obj_y),
    ));
    let mut final_values = Vec::new();
    for (event_op, prop_op) in event_prop_pairs(ids) {
        if event_op == 0 || prop_op == 0 {
            continue;
        }
        if let Some(ev) = obj.int_event_by_op_mut(ids, event_op) {
            if ev.check_event() {
                anim_skip_trace(format!(
                    "finish_object_all active slot={} file={} op={}({}) state=[{}]",
                    runtime_slot,
                    file,
                    event_op,
                    object_event_op_name(ids, event_op),
                    int_event_state(ev)
                ));
            }
            finish_wait_skipped_event(ev);
            final_values.push((event_op, prop_op, ev.get_total_value() as i64));
        }
    }
    obj.runtime.prop_event_lists.end_all();
    obj.runtime.prop_event_lists.frame();
    for (event_op, prop_op, value) in final_values {
        obj.set_int_prop(ids, prop_op, value);
        anim_skip_trace(format!(
            "finish_object_all prop_write slot={} file={} event_op={}({}) prop_op={} value={}",
            runtime_slot,
            file,
            event_op,
            object_event_op_name(ids, event_op),
            prop_op,
            value
        ));
    }
    anim_skip_trace(format!(
        "finish_object_all end slot={} file={} any_active={} tr={} alpha={} pos=({}, {})",
        runtime_slot,
        file,
        obj.any_event_active(),
        obj.get_int_prop(ids, ids.obj_tr),
        obj.base.alpha,
        obj.get_int_prop(ids, ids.obj_x),
        obj.get_int_prop(ids, ids.obj_y),
    ));
}

fn finish_event_wait_by_key(w: &EventWait, globals: &mut GlobalState, ids: &RuntimeConstants) {
    match w {
        EventWait::ObjectAll {
            stage_form_id,
            stage_idx,
            runtime_slot,
        } => {
            if let Some(obj) = object_active_by_runtime_slot_mut(
                globals,
                *stage_form_id,
                *stage_idx,
                *runtime_slot,
            ) {
                finish_wait_skipped_object_events(obj, ids);
            }
        }
        EventWait::ObjectOne {
            stage_form_id,
            stage_idx,
            runtime_slot,
            op,
        } => {
            if let Some(obj) = object_active_by_runtime_slot_mut(
                globals,
                *stage_form_id,
                *stage_idx,
                *runtime_slot,
            ) {
                finish_wait_skipped_object_event_by_op(obj, ids, *op);
            }
        }
        EventWait::ObjectList {
            stage_form_id,
            stage_idx,
            runtime_slot,
            list_op,
            list_idx,
        } => {
            if let Some(obj) = object_active_by_runtime_slot_mut(
                globals,
                *stage_form_id,
                *stage_idx,
                *runtime_slot,
            ) {
                let file = obj.file_name.as_deref().unwrap_or("-").to_string();
                if let Some(ev) = object_event_list_for_wait_mut(obj, ids, *list_op)
                    .and_then(|v| v.get_mut(*list_idx))
                {
                    anim_skip_trace(format!(
                        "finish_object_list_event begin stage_form={} stage={} slot={} file={} list_op={}({}) list_idx={} state=[{}]",
                        stage_form_id,
                        stage_idx,
                        runtime_slot,
                        file,
                        list_op,
                        object_event_op_name(ids, *list_op),
                        list_idx,
                        int_event_state(ev)
                    ));
                    finish_wait_skipped_event(ev);
                    anim_skip_trace(format!(
                        "finish_object_list_event end stage_form={} stage={} slot={} file={} list_op={}({}) list_idx={} state=[{}]",
                        stage_form_id,
                        stage_idx,
                        runtime_slot,
                        file,
                        list_op,
                        object_event_op_name(ids, *list_op),
                        list_idx,
                        int_event_state(ev)
                    ));
                } else {
                    anim_skip_trace(format!(
                        "finish_object_list_event missing stage_form={} stage={} slot={} file={} list_op={}({}) list_idx={}",
                        stage_form_id,
                        stage_idx,
                        runtime_slot,
                        file,
                        list_op,
                        object_event_op_name(ids, *list_op),
                        list_idx
                    ));
                }
            }
        }
        EventWait::GenericIntEvent { form_id, index } => match index {
            Some(i) => {
                if let Some(ev) = globals
                    .int_event_lists
                    .get_mut(form_id)
                    .and_then(|v| v.get_mut(*i))
                {
                    anim_skip_trace(format!(
                        "finish_generic_int_event begin form_id={} index={} state=[{}]",
                        form_id, i, int_event_state(ev)
                    ));
                    finish_wait_skipped_event(ev);
                    anim_skip_trace(format!(
                        "finish_generic_int_event end form_id={} index={} state=[{}]",
                        form_id, i, int_event_state(ev)
                    ));
                }
            }
            None => {
                if let Some(ev) = globals.int_event_roots.get_mut(form_id) {
                    anim_skip_trace(format!(
                        "finish_generic_int_event begin form_id={} index=None state=[{}]",
                        form_id, int_event_state(ev)
                    ));
                    finish_wait_skipped_event(ev);
                    anim_skip_trace(format!(
                        "finish_generic_int_event end form_id={} index=None state=[{}]",
                        form_id, int_event_state(ev)
                    ));
                }
            }
        },
        EventWait::FogX => {
            finish_wait_skipped_event(&mut globals.fog_global.x_event);
            globals.fog_global.scroll_x = globals.fog_global.x_event.get_total_value() as f32;
        }
        EventWait::CounterThreshold { .. } => {}
    }
}

#[derive(Debug, Default, Clone)]
pub struct VmWait {
    until: Option<Instant>,
    until_frame: Option<u64>,
    waiting_for_key: bool,
    /// If set, a key press cancels the current time wait (TIMEWAIT_KEY behavior).
    skip_time_on_key: bool,

    audio: Option<AudioWait>,
    audio_return_value: bool,

    event: Option<EventWait>,
    event_key_skip: bool,
    event_return_value: bool,

    movie: Option<MovieWait>,
    movie_key_skip: bool,

    global_movie: bool,
    global_movie_key_skip: bool,
    global_movie_return_value: bool,

    movie_skip_info: Option<MovieWait>,
    pending_value: Option<Value>,

    /// Blocks VM execution until a runtime modal UI supplies a result.
    system_modal: bool,
    system_modal_returns_value: bool,

    wipe: bool,
    wipe_key_skip: bool,

    block_generation: u64,
}

impl VmWait {
    pub fn block_generation(&self) -> u64 {
        self.block_generation
    }

    pub fn needs_runtime_poll(&self) -> bool {
        self.until.is_some()
            || self.until_frame.is_some()
            || self.audio.is_some()
            || self.event.is_some()
            || self.movie.is_some()
            || self.global_movie
            || self.wipe
    }

    fn mark_block_request(&mut self) {
        self.block_generation = self.block_generation.wrapping_add(1);
    }

    pub fn poll(
        &mut self,
        stack: &mut Vec<Value>,
        bgm: &mut BgmEngine,
        koe: &mut KoeEngine,
        se: &mut SeEngine,
        pcm: &mut PcmEngine,
        globals: &mut GlobalState,
        ids: &RuntimeConstants,
    ) -> bool {
        let blocked = self.is_blocked(bgm, koe, se, pcm, globals, ids);
        if !blocked {
            if let Some(v) = self.pending_value.take() {
                stack.push(v);
            }
        }
        blocked
    }

    pub fn is_blocked(
        &mut self,
        bgm: &mut BgmEngine,
        koe: &mut KoeEngine,
        se: &mut SeEngine,
        pcm: &mut PcmEngine,
        globals: &mut GlobalState,
        ids: &RuntimeConstants,
    ) -> bool {
        // Auto-clear time waits when the deadline is reached.
        if let Some(t) = self.until {
            if Instant::now() >= t {
                let key_skippable_timewait = self.skip_time_on_key;
                self.until = None;
                self.skip_time_on_key = false;
                if key_skippable_timewait {
                    anim_skip_trace("timewait_key naturally finished pending=0");
                    self.pending_value = Some(Value::Int(0));
                }
            }
        }

        if let Some(frame) = self.until_frame {
            if globals.render_frame >= frame {
                self.until_frame = None;
            }
        }

        // Auto-clear audio waits when the predicate is satisfied.
        if let Some(w) = self.audio {
            let done = match w {
                AudioWait::Bgm => !bgm.is_playing(),
                AudioWait::BgmFade => !bgm.is_fade_out_doing(),
                AudioWait::KoeAny => !koe.is_playing_any(),
                AudioWait::SeAny => !se.is_playing_any(),
                AudioWait::PcmAny => !pcm.is_playing_any(),
                AudioWait::PcmSlot(s) => !pcm.is_playing_slot(s as usize),
            };
            if done {
                self.audio = None;
                if self.audio_return_value {
                    self.pending_value = Some(Value::Int(0));
                }
                self.audio_return_value = false;
            }
        }

        // Auto-clear event waits when the predicate is satisfied.
        let event_done = if let Some(w) = self.event.as_ref() {
            match w {
                EventWait::ObjectAll {
                    stage_form_id,
                    stage_idx,
                    runtime_slot,
                } => object_active_by_runtime_slot(
                    globals,
                    *stage_form_id,
                    *stage_idx,
                    *runtime_slot,
                )
                .map(|obj| !obj.used || !obj.any_event_active())
                .unwrap_or(true),
                EventWait::ObjectOne {
                    stage_form_id,
                    stage_idx,
                    runtime_slot,
                    op,
                } => object_active_by_runtime_slot(
                    globals,
                    *stage_form_id,
                    *stage_idx,
                    *runtime_slot,
                )
                .map(|obj| {
                    !obj.used
                        || !obj
                            .int_event_by_op(ids, *op)
                            .map(|e| e.check_event())
                            .unwrap_or(false)
                })
                .unwrap_or(true),
                EventWait::ObjectList {
                    stage_form_id,
                    stage_idx,
                    runtime_slot,
                    list_op,
                    list_idx,
                } => object_active_by_runtime_slot(
                    globals,
                    *stage_form_id,
                    *stage_idx,
                    *runtime_slot,
                )
                .map(|obj| {
                    let active = object_event_list_for_wait(obj, ids, *list_op)
                        .and_then(|v| v.get(*list_idx))
                        .map(|e| e.check_event())
                        .unwrap_or(false);
                    !obj.used || !active
                })
                .unwrap_or(true),
                EventWait::GenericIntEvent { form_id, index } => match index {
                    Some(i) => globals
                        .int_event_lists
                        .get(form_id)
                        .and_then(|v| v.get(*i))
                        .map(|e| !e.check_event())
                        .unwrap_or(true),
                    None => globals
                        .int_event_roots
                        .get(form_id)
                        .map(|e| !e.check_event())
                        .unwrap_or(true),
                },
                EventWait::FogX => !globals.fog_global.x_event.check_event(),
                EventWait::CounterThreshold {
                    form_id,
                    index,
                    target,
                } => globals
                    .counter_lists
                    .get(form_id)
                    .and_then(|v| v.get(*index))
                    .map(|c| c.get_count() - *target >= 0)
                    .unwrap_or(true),
            }
        } else {
            false
        };
        if event_done {
            let was_event_key_skip = self.event_key_skip;
            anim_skip_trace(format!(
                "event_wait naturally finished event={:?} key_skip={} return_value={}",
                self.event.as_ref(), was_event_key_skip, self.event_return_value
            ));
            self.event = None;
            self.event_key_skip = false;
            if was_event_key_skip {
                self.waiting_for_key = false;
            }
            if self.event_return_value {
                self.pending_value = Some(Value::Int(0));
            }
            self.event_return_value = false;
        }

        // Auto-clear GLOBAL.MOV waits when playback ends.
        if self.global_movie {
            if !globals.mov.playing {
                if self.global_movie_return_value {
                    self.pending_value = Some(Value::Int(0));
                }
                self.global_movie = false;
                self.global_movie_key_skip = false;
                self.global_movie_return_value = false;
            }
        }

        // Auto-clear OBJECT movie waits when playback ends.
        if let Some(w) = self.movie {
            let done = object_active_by_runtime_slot(
                globals,
                w.stage_form_id,
                w.stage_idx,
                w.runtime_slot,
            )
            .map(|obj| !obj.used || !obj.movie.check_movie())
            .unwrap_or(true);

            if done {
                if w.return_value_flag {
                    self.pending_value = Some(Value::Int(0));
                }
                self.movie = None;
                self.movie_key_skip = false;
            }
        }

        // Auto-clear wipe waits when the wipe is finished.
        if self.wipe {
            if globals.wipe_done() {
                self.wipe = false;
                self.wipe_key_skip = false;
            }
        }

        self.waiting_for_key
            || self.until.is_some()
            || self.until_frame.is_some()
            || self.audio.is_some()
            || self.event.is_some()
            || self.movie.is_some()
            || self.global_movie
            || self.system_modal
            || self.wipe
    }

    pub fn wait_system_modal(&mut self, returns_value: bool) {
        self.mark_block_request();
        self.system_modal = true;
        self.system_modal_returns_value = returns_value;
    }

    pub fn finish_system_modal(&mut self, value: Value) {
        if self.system_modal {
            self.system_modal = false;
            if self.system_modal_returns_value {
                self.pending_value = Some(value);
            }
            self.system_modal_returns_value = false;
        }
    }

    pub fn system_modal_active(&self) -> bool {
        self.system_modal
    }

    pub fn wait_ms(&mut self, ms: u64) {
        if ms == 0 {
            return;
        }
        self.mark_block_request();
        self.until = Some(Instant::now() + Duration::from_millis(ms));
        self.skip_time_on_key = false;
    }

    pub fn wait_next_frame(&mut self, current_frame: u64) {
        self.mark_block_request();
        self.until_frame = Some(current_frame.saturating_add(1));
        self.skip_time_on_key = false;
    }

    /// Wait for a duration, but allow any key/mouse press to cancel the wait.
    pub fn wait_ms_key(&mut self, ms: u64) {
        if ms == 0 {
            anim_skip_trace("wait_ms_key ignored ms=0");
            return;
        }
        self.mark_block_request();
        self.until = Some(Instant::now() + Duration::from_millis(ms));
        self.skip_time_on_key = true;
        anim_skip_trace(format!("wait_ms_key start ms={} block_generation={}", ms, self.block_generation));
    }

    pub fn wait_key(&mut self) {
        self.mark_block_request();
        self.waiting_for_key = true;
    }

    pub fn wait_audio(&mut self, w: AudioWait, key: bool) {
        self.wait_audio_with_return(w, key, false);
    }

    pub fn wait_audio_with_return(&mut self, w: AudioWait, key: bool, return_value_flag: bool) {
        self.mark_block_request();
        self.audio = Some(w);
        self.audio_return_value = return_value_flag;
        if key {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_object_all_events(
        &mut self,
        stage_form_id: u32,
        stage_idx: i64,
        runtime_slot: usize,
        key_skip: bool,
    ) {
        self.mark_block_request();
        self.event = Some(EventWait::ObjectAll {
            stage_form_id,
            stage_idx,
            runtime_slot,
        });
        anim_skip_trace(format!(
            "wait_object_all_events start stage_form={} stage={} slot={} key_skip={} block_generation={}",
            stage_form_id, stage_idx, runtime_slot, key_skip, self.block_generation
        ));
        self.event_key_skip = key_skip;
        self.event_return_value = false;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_object_event(
        &mut self,
        stage_form_id: u32,
        stage_idx: i64,
        runtime_slot: usize,
        op: i32,
        key_skip: bool,
        return_value_flag: bool,
    ) {
        self.mark_block_request();
        self.event = Some(EventWait::ObjectOne {
            stage_form_id,
            stage_idx,
            runtime_slot,
            op,
        });
        anim_skip_trace(format!(
            "wait_object_event start stage_form={} stage={} slot={} op={} key_skip={} return_value={} block_generation={}",
            stage_form_id, stage_idx, runtime_slot, op, key_skip, return_value_flag, self.block_generation
        ));
        self.event_key_skip = key_skip;
        self.event_return_value = return_value_flag;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_object_event_list(
        &mut self,
        stage_form_id: u32,
        stage_idx: i64,
        runtime_slot: usize,
        list_op: i32,
        list_idx: usize,
        key_skip: bool,
        return_value_flag: bool,
    ) {
        self.mark_block_request();
        self.event = Some(EventWait::ObjectList {
            stage_form_id,
            stage_idx,
            runtime_slot,
            list_op,
            list_idx,
        });
        anim_skip_trace(format!(
            "wait_object_event_list start stage_form={} stage={} slot={} list_op={} list_idx={} key_skip={} return_value={} block_generation={}",
            stage_form_id, stage_idx, runtime_slot, list_op, list_idx, key_skip, return_value_flag, self.block_generation
        ));
        self.event_key_skip = key_skip;
        self.event_return_value = return_value_flag;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_global_movie(&mut self, key_skip: bool, return_value_flag: bool) {
        self.mark_block_request();
        self.global_movie = true;
        self.global_movie_key_skip = key_skip;
        self.global_movie_return_value = return_value_flag;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_object_movie(
        &mut self,
        stage_form_id: u32,
        stage_idx: i64,
        runtime_slot: usize,
        key_skip: bool,
        return_value_flag: bool,
    ) {
        self.mark_block_request();
        self.movie = Some(MovieWait {
            stage_form_id,
            stage_idx,
            runtime_slot,
            return_value_flag,
        });
        self.movie_key_skip = key_skip;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_generic_int_event(
        &mut self,
        form_id: u32,
        index: Option<usize>,
        key_skip: bool,
        return_value_flag: bool,
    ) {
        self.mark_block_request();
        self.event = Some(EventWait::GenericIntEvent { form_id, index });
        anim_skip_trace(format!(
            "wait_generic_int_event start form_id={} index={:?} key_skip={} return_value={} block_generation={}",
            form_id, index, key_skip, return_value_flag, self.block_generation
        ));
        self.event_key_skip = key_skip;
        self.event_return_value = return_value_flag;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_fog_x_event(&mut self, key_skip: bool, return_value_flag: bool) {
        self.mark_block_request();
        self.event = Some(EventWait::FogX);
        self.event_key_skip = key_skip;
        self.event_return_value = return_value_flag;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_counter(
        &mut self,
        form_id: u32,
        index: usize,
        target: i64,
        key_skip: bool,
        return_value_flag: bool,
    ) {
        self.mark_block_request();
        self.event = Some(EventWait::CounterThreshold {
            form_id,
            index,
            target,
        });
        self.event_key_skip = key_skip;
        self.event_return_value = return_value_flag;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_wipe(&mut self, key_skip: bool) {
        self.mark_block_request();
        self.wipe = true;
        self.wipe_key_skip = key_skip;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    /// Notify the wait system that a key/mouse input happened.
    ///
    /// Returns true if the input is interpreted as a wipe-skip (used by WIPE/WAIT_WIPE).
    pub fn notify_key(&mut self, _globals: &mut GlobalState, _ids: &RuntimeConstants) -> bool {
        let wipe_skipped = self.wipe && self.wipe_key_skip;
        self.waiting_for_key = false;
        if self.audio.is_some() && self.audio_return_value {
            self.pending_value = Some(Value::Int(1));
        }
        self.audio = None;
        self.audio_return_value = false;
        // C++ event WAIT_KEY is not skipped by arbitrary input here.
        // It is skipped only by DECIDE down-up in notify_movie_down_up().
        // C++ MOV/OBJECT movie waits are not skipped by arbitrary input here.
        // They are skipped only by DECIDE/CANCEL down-up in notify_movie_down_up().
        if self.skip_time_on_key {
            anim_skip_trace("notify_key skipped TIMEWAIT_KEY pending=1");
            self.until = None;
            self.skip_time_on_key = false;
            self.pending_value = Some(Value::Int(1));
        }

        if wipe_skipped {
            self.wipe = false;
            self.wipe_key_skip = false;
        }

        wipe_skipped
    }

    /// Notify MOV/OBJECT movie waits that DECIDE/CANCEL completed a down-up pair.
    ///
    /// This matches C++ `tnm_mov_wait_proc` / `tnm_obj_mov_wait_proc`:
    /// MOV_WAIT_KEY consumes only VK_EX_DECIDE or VK_EX_CANCEL down-up, returning
    /// 1 or -1 respectively. Generic key/mouse events must not skip movie waits.
    pub fn notify_movie_down_up(
        &mut self,
        globals: &mut GlobalState,
        ids: &RuntimeConstants,
        result: i64,
    ) -> bool {
        let mut skipped = false;
        if result == 1 && self.event_key_skip {
            if let Some(w) = self.event.take() {
                anim_skip_trace(format!(
                    "notify_movie_down_up skip event result={} event={:?} return_value={}",
                    result, w, self.event_return_value
                ));
                finish_event_wait_by_key(&w, globals, ids);
                if self.event_return_value {
                    self.pending_value = Some(Value::Int(1));
                }
                skipped = true;
            } else {
                anim_skip_trace(format!(
                    "notify_movie_down_up event_key_skip without event result={}",
                    result
                ));
            }
            self.event_key_skip = false;
            self.event_return_value = false;
        }
        if self.global_movie && self.global_movie_key_skip {
            // C++ tnm_mov_wait_proc returns first, then C_elm_mov::close() tears down
            // the native movie.  Do not clear audio_id here; RuntimeContext must still
            // see it and stop the Rust movie audio handle.
            globals.mov.playing = false;
            if self.global_movie_return_value {
                self.pending_value = Some(Value::Int(result));
            }
            self.global_movie = false;
            self.global_movie_key_skip = false;
            self.global_movie_return_value = false;
            skipped = true;
        }
        // OBJECT movie wait in C++ only consumes VK_EX_DECIDE down-up.
        // VK_EX_CANCEL is handled only by GLOBAL MOV_WAIT_KEY.
        if self.movie_key_skip && result == 1 {
            if let Some(w) = self.movie.take() {
                if w.return_value_flag {
                    self.pending_value = Some(Value::Int(1));
                }
                self.movie_skip_info = Some(w);
                skipped = true;
            }
            self.movie_key_skip = false;
        }
        if skipped {
            self.waiting_for_key = false;
        }
        skipped
    }

    /// If the current wait was skipped via key input, returns the skipped movie wait info.
    pub fn take_movie_skip(&mut self) -> Option<MovieWait> {
        self.movie_skip_info.take()
    }

    pub fn clear(&mut self) {
        self.until = None;
        self.waiting_for_key = false;
        self.skip_time_on_key = false;
        self.audio = None;
        self.audio_return_value = false;
        self.event = None;
        self.event_key_skip = false;
        self.event_return_value = false;
        self.movie = None;
        self.movie_key_skip = false;
        self.global_movie = false;
        self.global_movie_key_skip = false;
        self.global_movie_return_value = false;
        self.movie_skip_info = None;
        self.pending_value = None;
        self.system_modal = false;
        self.system_modal_returns_value = false;
        self.wipe = false;
        self.wipe_key_skip = false;
    }
}
