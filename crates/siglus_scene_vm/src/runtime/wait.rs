//! VM wait/blocking state.
//!
//! The original engine has many commands/forms that block execution until:
//! - a certain time passes, or
//! - the user presses a key / clicks.
//!
//! Cross-platform blocking and wait model.

use crate::platform_time::{Duration, Instant};

use crate::audio::{BgmEngine, KoeEngine, PcmEngine, SeEngine};

use super::Value;
use super::constants::RuntimeConstants;
use super::globals::{GlobalState, ObjectState, StageFormState};
use super::int_event::IntEvent;

fn anim_skip_trace_enabled() -> bool {
    env_is_set!("SG_DEBUG")
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
    if ids.obj_patno_eve != 0 && op == ids.obj_patno_eve {
        return "PATNO_EVE";
    }
    if ids.obj_x_eve != 0 && op == ids.obj_x_eve {
        return "X_EVE";
    }
    if ids.obj_y_eve != 0 && op == ids.obj_y_eve {
        return "Y_EVE";
    }
    if ids.obj_z_eve != 0 && op == ids.obj_z_eve {
        return "Z_EVE";
    }
    if ids.obj_center_x_eve != 0 && op == ids.obj_center_x_eve {
        return "CENTER_X_EVE";
    }
    if ids.obj_center_y_eve != 0 && op == ids.obj_center_y_eve {
        return "CENTER_Y_EVE";
    }
    if ids.obj_center_z_eve != 0 && op == ids.obj_center_z_eve {
        return "CENTER_Z_EVE";
    }
    if ids.obj_center_rep_x_eve != 0 && op == ids.obj_center_rep_x_eve {
        return "CENTER_REP_X_EVE";
    }
    if ids.obj_center_rep_y_eve != 0 && op == ids.obj_center_rep_y_eve {
        return "CENTER_REP_Y_EVE";
    }
    if ids.obj_center_rep_z_eve != 0 && op == ids.obj_center_rep_z_eve {
        return "CENTER_REP_Z_EVE";
    }
    if ids.obj_scale_x_eve != 0 && op == ids.obj_scale_x_eve {
        return "SCALE_X_EVE";
    }
    if ids.obj_scale_y_eve != 0 && op == ids.obj_scale_y_eve {
        return "SCALE_Y_EVE";
    }
    if ids.obj_scale_z_eve != 0 && op == ids.obj_scale_z_eve {
        return "SCALE_Z_EVE";
    }
    if ids.obj_rotate_x_eve != 0 && op == ids.obj_rotate_x_eve {
        return "ROTATE_X_EVE";
    }
    if ids.obj_rotate_y_eve != 0 && op == ids.obj_rotate_y_eve {
        return "ROTATE_Y_EVE";
    }
    if ids.obj_rotate_z_eve != 0 && op == ids.obj_rotate_z_eve {
        return "ROTATE_Z_EVE";
    }
    if ids.obj_clip_left_eve != 0 && op == ids.obj_clip_left_eve {
        return "CLIP_LEFT_EVE";
    }
    if ids.obj_clip_top_eve != 0 && op == ids.obj_clip_top_eve {
        return "CLIP_TOP_EVE";
    }
    if ids.obj_clip_right_eve != 0 && op == ids.obj_clip_right_eve {
        return "CLIP_RIGHT_EVE";
    }
    if ids.obj_clip_bottom_eve != 0 && op == ids.obj_clip_bottom_eve {
        return "CLIP_BOTTOM_EVE";
    }
    if ids.obj_src_clip_left_eve != 0 && op == ids.obj_src_clip_left_eve {
        return "SRC_CLIP_LEFT_EVE";
    }
    if ids.obj_src_clip_top_eve != 0 && op == ids.obj_src_clip_top_eve {
        return "SRC_CLIP_TOP_EVE";
    }
    if ids.obj_src_clip_right_eve != 0 && op == ids.obj_src_clip_right_eve {
        return "SRC_CLIP_RIGHT_EVE";
    }
    if ids.obj_src_clip_bottom_eve != 0 && op == ids.obj_src_clip_bottom_eve {
        return "SRC_CLIP_BOTTOM_EVE";
    }
    if ids.obj_tr_eve != 0 && op == ids.obj_tr_eve {
        return "TR_EVE";
    }
    if ids.obj_mono_eve != 0 && op == ids.obj_mono_eve {
        return "MONO_EVE";
    }
    if ids.obj_reverse_eve != 0 && op == ids.obj_reverse_eve {
        return "REVERSE_EVE";
    }
    if ids.obj_bright_eve != 0 && op == ids.obj_bright_eve {
        return "BRIGHT_EVE";
    }
    if ids.obj_dark_eve != 0 && op == ids.obj_dark_eve {
        return "DARK_EVE";
    }
    if ids.obj_color_r_eve != 0 && op == ids.obj_color_r_eve {
        return "COLOR_R_EVE";
    }
    if ids.obj_color_g_eve != 0 && op == ids.obj_color_g_eve {
        return "COLOR_G_EVE";
    }
    if ids.obj_color_b_eve != 0 && op == ids.obj_color_b_eve {
        return "COLOR_B_EVE";
    }
    if ids.obj_color_rate_eve != 0 && op == ids.obj_color_rate_eve {
        return "COLOR_RATE_EVE";
    }
    if ids.obj_color_add_r_eve != 0 && op == ids.obj_color_add_r_eve {
        return "COLOR_ADD_R_EVE";
    }
    if ids.obj_color_add_g_eve != 0 && op == ids.obj_color_add_g_eve {
        return "COLOR_ADD_G_EVE";
    }
    if ids.obj_color_add_b_eve != 0 && op == ids.obj_color_add_b_eve {
        return "COLOR_ADD_B_EVE";
    }
    if ids.obj_x_rep_eve != 0 && op == ids.obj_x_rep_eve {
        return "X_REP_EVE";
    }
    if ids.obj_y_rep_eve != 0 && op == ids.obj_y_rep_eve {
        return "Y_REP_EVE";
    }
    if ids.obj_z_rep_eve != 0 && op == ids.obj_z_rep_eve {
        return "Z_REP_EVE";
    }
    if ids.obj_tr_rep_eve != 0 && op == ids.obj_tr_rep_eve {
        return "TR_REP_EVE";
    }
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
    PcmSlotFade(u8),
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
    ScreenEffect {
        form_id: u32,
        index: usize,
        op: i32,
    },
    StageEffect {
        stage_form_id: u32,
        stage_idx: i64,
        index: usize,
        op: i32,
    },
    Mask {
        form_id: u32,
        index: usize,
        op: i32,
    },
    FogX,
    CounterThreshold {
        form_id: u32,
        index: usize,
        target: i64,
    },
    PcmEvent {
        form_id: u32,
        index: usize,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct MovieWait {
    pub stage_form_id: u32,
    pub stage_idx: i64,
    pub runtime_slot: usize,
    pub return_value_flag: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct EmoteWait {
    pub stage_form_id: u32,
    pub stage_idx: i64,
    pub runtime_slot: usize,
    pub return_value_flag: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum QuakeWait {
    Screen {
        form_id: u32,
        index: usize,
    },
    Stage {
        stage_form_id: u32,
        stage_idx: i64,
        index: usize,
    },
}

fn quake_wait_active(globals: &GlobalState, wait: QuakeWait) -> bool {
    match wait {
        QuakeWait::Screen { form_id, index } => globals
            .screen_forms
            .get(&form_id)
            .and_then(|screen| screen.quake_list.get(index))
            .map(|quake| quake.is_active())
            .unwrap_or(false),
        QuakeWait::Stage {
            stage_form_id,
            stage_idx,
            index,
        } => globals
            .stage_forms
            .get(&stage_form_id)
            .and_then(|stage| stage.quake_lists.get(&stage_idx))
            .and_then(|quakes| quakes.get(index))
            .map(|quake| quake.is_active())
            .unwrap_or(false),
    }
}

fn stop_waited_quake(globals: &mut GlobalState, wait: QuakeWait) {
    let quake = match wait {
        QuakeWait::Screen { form_id, index } => globals
            .screen_forms
            .get_mut(&form_id)
            .and_then(|screen| screen.quake_list.get_mut(index)),
        QuakeWait::Stage {
            stage_form_id,
            stage_idx,
            index,
        } => globals
            .stage_forms
            .get_mut(&stage_form_id)
            .and_then(|stage| stage.quake_lists.get_mut(&stage_idx))
            .and_then(|quakes| quakes.get_mut(index)),
    };
    if let Some(quake) = quake {
        quake.reinit();
    }
}

fn object_runtime_slot(idx: usize, obj: &ObjectState) -> usize {
    obj.runtime_slot_or(idx)
}

fn find_object_by_runtime_slot(
    objects: &[ObjectState],
    runtime_slot: usize,
) -> Option<&ObjectState> {
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

fn find_object_by_runtime_slot_mut(
    mut objects: &mut [ObjectState],
    runtime_slot: usize,
) -> Option<&mut ObjectState> {
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
                        form_id,
                        i,
                        int_event_state(ev)
                    ));
                    finish_wait_skipped_event(ev);
                    anim_skip_trace(format!(
                        "finish_generic_int_event end form_id={} index={} state=[{}]",
                        form_id,
                        i,
                        int_event_state(ev)
                    ));
                }
            }
            None => {
                if let Some(ev) = globals.int_event_roots.get_mut(form_id) {
                    anim_skip_trace(format!(
                        "finish_generic_int_event begin form_id={} index=None state=[{}]",
                        form_id,
                        int_event_state(ev)
                    ));
                    finish_wait_skipped_event(ev);
                    anim_skip_trace(format!(
                        "finish_generic_int_event end form_id={} index=None state=[{}]",
                        form_id,
                        int_event_state(ev)
                    ));
                }
            }
        },
        EventWait::ScreenEffect { form_id, index, op } => {
            if let Some(ev) = globals
                .screen_forms
                .get_mut(form_id)
                .and_then(|screen| screen.effect_list.get_mut(*index))
                .and_then(|effect| effect.int_event_by_op_mut(ids, *op))
            {
                finish_wait_skipped_event(ev);
            }
        }
        EventWait::StageEffect {
            stage_form_id,
            stage_idx,
            index,
            op,
        } => {
            if let Some(ev) = globals
                .stage_forms
                .get_mut(stage_form_id)
                .and_then(|stage| stage.effect_lists.get_mut(stage_idx))
                .and_then(|effects| effects.get_mut(*index))
                .and_then(|effect| effect.int_event_by_op_mut(ids, *op))
            {
                finish_wait_skipped_event(ev);
            }
        }
        EventWait::Mask { form_id, index, op } => {
            if let Some(mask) = globals
                .mask_lists
                .get_mut(form_id)
                .and_then(|list| list.masks.get_mut(*index))
            {
                let ev = if *op == super::constants::elm_value::MASK_X_EVE {
                    Some(&mut mask.x_event)
                } else if *op == super::constants::elm_value::MASK_Y_EVE {
                    Some(&mut mask.y_event)
                } else {
                    None
                };
                if let Some(ev) = ev {
                    finish_wait_skipped_event(ev);
                }
            }
        }
        EventWait::FogX => {
            finish_wait_skipped_event(&mut globals.fog_global.x_event);
            globals.fog_global.scroll_x = globals.fog_global.x_event.get_total_value() as f32;
        }
        EventWait::CounterThreshold { .. } => {}
        // C++ PCMEVENT_WAIT_KEY only releases the waiting process. It does not
        // stop or fast-forward the sound event itself.
        EventWait::PcmEvent { .. } => {}
    }
}

#[derive(Debug, Default, Clone)]
pub struct VmWait {
    pub until: Option<Instant>,
    pub until_frame: Option<u64>,
    /// True only when `until` represents MWND OPEN/CLOSE animation wait.
    mwnd_animation_wait: bool,
    pub waiting_for_key: bool,
    /// TNM_PROC_TYPE_SEL_BTN. Unlike KEY_WAIT, a button selection is released
    /// only by C_elm_btn_select::is_processing() becoming false for its
    /// configured sync_type. Ordinary key/message wait notifications must not
    /// pop this process.
    selbtn: bool,
    /// OBJBTNGROUP.SEL completes only when its button group decides or cancels.
    /// An unrelated click must not supply a default selection result.
    group_selection: Option<(u32, i64, usize)>,
    /// TNM_PROC_TYPE_KEY_WAIT created by KEYLIST.WAIT/WAIT_FORCE.
    generic_key_wait: bool,
    generic_key_wait_skip_disabled: bool,
    /// TNM_PROC_TYPE_MESSAGE_WAIT: block only until the typewriter has
    /// revealed the complete message.  This is deliberately distinct from
    /// MESSAGE_KEY_WAIT, which waits for user input after reveal.
    pub message_reveal: bool,
    /// PP/R/PAGE push MESSAGE_KEY_WAIT below MESSAGE_WAIT. It must not become
    /// active until MESSAGE_WAIT has popped.
    message_key_after_reveal: bool,
    message_key_wait: bool,
    /// If set, a key press cancels the current time wait (TIMEWAIT_KEY behavior).
    skip_time_on_key: bool,

    pub audio: Option<AudioWait>,
    /// C_tnm_proc::key_skip_enable_flag for audio waits. Audio WAIT_KEY is
    /// still an audio proc; it must not be represented by the generic
    /// MESSAGE/INPUT `waiting_for_key` bit. The original flow proc consumes
    /// only VK_EX_DECIDE down-up and leaves the sound playing.
    audio_key_skip: bool,
    audio_return_value: bool,

    pub event: Option<EventWait>,
    event_key_skip: bool,
    event_return_value: bool,

    pub movie: Option<MovieWait>,
    movie_key_skip: bool,

    emote: Option<EmoteWait>,
    emote_key_skip: bool,

    global_movie: bool,
    global_movie_key_skip: bool,
    global_movie_return_value: bool,

    movie_skip_info: Option<MovieWait>,

    pub quake: Option<QuakeWait>,
    quake_key_skip: bool,
    pub pending_value: Option<Value>,

    /// Blocks VM execution until a runtime modal UI supplies a return value.
    pub system_modal: bool,

    pub wipe: bool,
    wipe_key_skip: bool,
    wipe_return_value: bool,

    block_generation: u64,
}

impl VmWait {
    pub fn block_generation(&self) -> u64 {
        self.block_generation
    }

    pub fn needs_runtime_poll(&self) -> bool {
        self.message_reveal
            || self.message_key_wait
            || self.generic_key_wait
            || self.selbtn
            || self.until.is_some()
            || self.until_frame.is_some()
            || self.audio.is_some()
            || self.event.is_some()
            || self.movie.is_some()
            || self.emote.is_some()
            || self.quake.is_some()
            || self.global_movie
            || self.wipe
    }

    /// Whether wall-clock or frame-driven state can make this wait finish.
    ///
    /// Pure key waits still need to be checked after an input event, but they
    /// must not keep the render loop running while the player is idle.
    pub fn needs_continuous_frame(&self) -> bool {
        self.message_reveal
            || self.until.is_some()
            || self.until_frame.is_some()
            || self.audio.is_some()
            || self.event.is_some()
            || self.movie.is_some()
            || self.emote.is_some()
            || self.quake.is_some()
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
        skipping: bool,
    ) -> bool {
        let blocked = self.is_blocked(bgm, koe, se, pcm, globals, ids, skipping);
        if !blocked && let Some(v) = self.pending_value.take() {
            stack.push(v);
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
        skipping: bool,
    ) -> bool {
        // TNM_PROC_TYPE_SEL_BTN is not a key wait. flow_proc.cpp
        // tnm_sel_btn_proc() keeps the process on the stack until
        // C_elm_btn_select::is_processing() becomes false for sync_type.
        if self.selbtn {
            let processing = match globals.selbtn.sync_type {
                0 => globals.selbtn.processing_flag_0,
                1 => globals.selbtn.processing_flag_1,
                2 => globals.selbtn.processing_flag_2,
                _ => false,
            };
            if !processing {
                self.selbtn = false;
            }
        }

        // Auto-clear time waits when the deadline is reached.
        if let Some(t) = self.until
            && Instant::now() >= t
        {
            let key_skippable_timewait = self.skip_time_on_key;
            self.until = None;
            self.skip_time_on_key = false;
            self.mwnd_animation_wait = false;
            if key_skippable_timewait {
                anim_skip_trace("timewait_key naturally finished pending=0");
                self.pending_value = Some(Value::Int(0));
            }
        }

        // C++ TIMEWAIT/TIMEWAIT_KEY are released immediately by global skip
        // unless the current proc has skip_disable_flag. TIMEWAIT_KEY returns
        // the same 0 value as a natural timeout.
        if skipping && self.until.is_some() {
            let return_value = self.skip_time_on_key;
            self.until = None;
            self.skip_time_on_key = false;
            self.mwnd_animation_wait = false;
            if return_value {
                self.pending_value = Some(Value::Int(0));
            }
        }

        if let Some(frame) = self.until_frame
            && globals.render_frame >= frame
        {
            self.until_frame = None;
        }

        // KEYLIST.WAIT is TNM_PROC_TYPE_KEY_WAIT and obeys global skip.
        // WAIT_FORCE sets C_tnm_proc::skip_disable_flag, so it remains blocked.
        if skipping && self.generic_key_wait && !self.generic_key_wait_skip_disabled {
            self.generic_key_wait = false;
            self.generic_key_wait_skip_disabled = false;
            self.waiting_for_key = false;
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
                AudioWait::PcmSlotFade(s) => !pcm.is_fading_slot(s as usize),
            };
            if done {
                self.audio = None;
                self.audio_key_skip = false;
                if self.audio_return_value {
                    self.pending_value = Some(Value::Int(0));
                }
                self.audio_return_value = false;
            }
        }

        // All original audio flow procs (BGM/KOE/PCM/PCMCH) test skipping
        // after natural completion. Skip releases only the proc; the player
        // keeps running. The returned value is the normal-completion value 0.
        if skipping && self.audio.is_some() {
            self.audio = None;
            self.audio_key_skip = false;
            if self.audio_return_value {
                self.pending_value = Some(Value::Int(0));
            }
            self.audio_return_value = false;
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
                .map(|obj| !obj.any_event_active())
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
                    !obj.int_event_by_op(ids, *op)
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
                    !active
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
                EventWait::ScreenEffect { form_id, index, op } => globals
                    .screen_forms
                    .get(form_id)
                    .and_then(|screen| screen.effect_list.get(*index))
                    .and_then(|effect| effect.int_event_by_op(ids, *op))
                    .map(|event| !event.check_event())
                    .unwrap_or(true),
                EventWait::StageEffect {
                    stage_form_id,
                    stage_idx,
                    index,
                    op,
                } => globals
                    .stage_forms
                    .get(stage_form_id)
                    .and_then(|stage| stage.effect_lists.get(stage_idx))
                    .and_then(|effects| effects.get(*index))
                    .and_then(|effect| effect.int_event_by_op(ids, *op))
                    .map(|event| !event.check_event())
                    .unwrap_or(true),
                EventWait::Mask { form_id, index, op } => globals
                    .mask_lists
                    .get(form_id)
                    .and_then(|list| list.masks.get(*index))
                    .and_then(|mask| {
                        if *op == super::constants::elm_value::MASK_X_EVE {
                            Some(&mask.x_event)
                        } else if *op == super::constants::elm_value::MASK_Y_EVE {
                            Some(&mask.y_event)
                        } else {
                            None
                        }
                    })
                    .map(|event| !event.check_event())
                    .unwrap_or(true),
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
                EventWait::PcmEvent { form_id, index } => globals
                    .pcm_event_lists
                    .get(form_id)
                    .and_then(|v| v.get(*index))
                    .map(|event| !event.is_active())
                    .unwrap_or(true),
            }
        } else {
            false
        };
        if event_done {
            let was_event_key_skip = self.event_key_skip;
            anim_skip_trace(format!(
                "event_wait naturally finished event={:?} key_skip={} return_value={}",
                self.event.as_ref(),
                was_event_key_skip,
                self.event_return_value
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

        // COUNTER.WAIT/WAIT_KEY is represented by CounterThreshold. Unlike
        // general INTEVENT waits, the original counter flow proc releases on
        // global skip and WAIT_KEY returns 0.
        if skipping && matches!(self.event, Some(EventWait::CounterThreshold { .. })) {
            self.event = None;
            self.event_key_skip = false;
            if self.event_return_value {
                self.pending_value = Some(Value::Int(0));
            }
            self.event_return_value = false;
            self.waiting_for_key = false;
        }

        if self
            .quake
            .map(|wait| !quake_wait_active(globals, wait))
            .unwrap_or(false)
        {
            self.quake = None;
            self.quake_key_skip = false;
            self.waiting_for_key = false;
        }
        if skipping && let Some(wait) = self.quake.take() {
            stop_waited_quake(globals, wait);
            self.quake_key_skip = false;
            self.waiting_for_key = false;
        }

        // Auto-clear GLOBAL.MOV waits when playback ends.
        if self.global_movie && !globals.mov.playing {
            if self.global_movie_return_value {
                self.pending_value = Some(Value::Int(0));
            }
            self.global_movie = false;
            self.global_movie_key_skip = false;
            self.global_movie_return_value = false;
        }

        // Auto-clear OBJECT movie waits when playback ends.
        if let Some(w) = self.movie {
            let done = object_active_by_runtime_slot(
                globals,
                w.stage_form_id,
                w.stage_idx,
                w.runtime_slot,
            )
            .map(|obj| !obj.movie.check_movie())
            .unwrap_or(true);

            if done {
                if w.return_value_flag {
                    self.pending_value = Some(Value::Int(0));
                }
                self.movie = None;
                self.movie_key_skip = false;
            }
        }

        // C++ TNM_PROC_TYPE_OBJ_EMOTE_WAIT completes when IsAnimating() becomes false.
        if let Some(w) = self.emote {
            let done = object_active_by_runtime_slot(
                globals,
                w.stage_form_id,
                w.stage_idx,
                w.runtime_slot,
            )
            .map(|obj| !obj.emote.is_animating())
            .unwrap_or(true);

            if done {
                let was_key_skip = self.emote_key_skip;
                if w.return_value_flag {
                    self.pending_value = Some(Value::Int(0));
                }
                self.emote = None;
                self.emote_key_skip = false;
                if was_key_skip {
                    self.waiting_for_key = false;
                }
            }
        }

        // Auto-clear wipe waits when the wipe is finished.
        if self.wipe && globals.wipe_done() {
            self.wipe = false;
            if self.wipe_return_value {
                self.pending_value = Some(Value::Int(0));
            }
            self.wipe_return_value = false;
            if self.wipe_key_skip {
                self.wipe_key_skip = false;
                if self.waiting_for_key {
                    self.waiting_for_key = false;
                }
            }
        }

        if let Some((form_id, stage_idx, group_idx)) = self.group_selection {
            let waiting = globals
                .stage_forms
                .get(&form_id)
                .and_then(|st| st.group_lists.get(&stage_idx))
                .and_then(|groups| groups.get(group_idx))
                .map(|group| group.wait_flag && group.started)
                .unwrap_or(false);
            if !waiting {
                self.group_selection = None;
            }
        }

        self.selbtn
            || self.group_selection.is_some()
            || self.waiting_for_key
            || self.message_reveal
            || self.until.is_some()
            || self.until_frame.is_some()
            || self.audio.is_some()
            || self.event.is_some()
            || self.movie.is_some()
            || self.emote.is_some()
            || self.quake.is_some()
            || self.global_movie
            || self.system_modal
            || self.wipe
    }

    pub fn wait_system_modal(&mut self) {
        self.mark_block_request();
        self.system_modal = true;
    }

    pub fn finish_system_modal(&mut self, value: Value) {
        if self.system_modal {
            self.system_modal = false;
            self.pending_value = Some(value);
        }
    }

    pub fn finish_system_modal_void(&mut self) {
        if self.system_modal {
            self.system_modal = false;
            self.pending_value = None;
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
        self.mwnd_animation_wait = false;
    }

    pub fn wait_mwnd_animation(&mut self, ms: u64) {
        if ms == 0 {
            return;
        }
        self.mark_block_request();
        self.until = Some(Instant::now() + Duration::from_millis(ms));
        self.skip_time_on_key = false;
        self.mwnd_animation_wait = true;
    }

    pub fn mwnd_animation_waiting(&self) -> bool {
        self.mwnd_animation_wait && self.until.is_some()
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
        self.mwnd_animation_wait = false;
        anim_skip_trace(format!(
            "wait_ms_key start ms={} block_generation={}",
            ms, self.block_generation
        ));
    }

    pub fn wait_selbtn(&mut self) {
        self.mark_block_request();
        self.selbtn = true;
    }

    pub fn set_selbtn_result(&mut self, result: i64) {
        // C_elm_btn_select::decide() pushes the selected zero-based index before
        // clearing processing_flag_2. Keep the value pending while the dedicated
        // SEL_BTN proc is still blocking; poll() materializes it when the chosen
        // sync point releases.
        self.pending_value = Some(Value::Int(result));
    }

    pub fn wait_key(&mut self) {
        self.mark_block_request();
        self.waiting_for_key = true;
    }

    pub fn wait_group_selection(&mut self, form_id: u32, stage_idx: i64, group_idx: usize) {
        self.mark_block_request();
        self.group_selection = Some((form_id, stage_idx, group_idx));
    }

    pub(crate) fn button_selection_waiting(&self) -> bool {
        self.selbtn || self.group_selection.is_some()
    }

    pub fn wait_input_key(&mut self, skip_disabled: bool) {
        self.mark_block_request();
        self.waiting_for_key = true;
        self.generic_key_wait = true;
        self.generic_key_wait_skip_disabled = skip_disabled;
    }

    pub fn waiting_for_key(&self) -> bool {
        self.waiting_for_key
    }

    pub fn wait_message_reveal(&mut self) {
        self.mark_block_request();
        self.message_reveal = true;
        self.message_key_after_reveal = false;
    }

    pub fn wait_message_reveal_then_key(&mut self) {
        self.mark_block_request();
        self.message_reveal = true;
        self.message_key_after_reveal = true;
        self.message_key_wait = false;
    }

    pub fn message_reveal_waiting(&self) -> bool {
        self.message_reveal
    }

    /// Pop MESSAGE_WAIT and expose the MESSAGE_KEY_WAIT that PP/R/PAGE had
    /// already pushed below it. Returns true when that second proc became active.
    pub fn finish_message_reveal(&mut self) -> bool {
        self.message_reveal = false;
        if self.message_key_after_reveal {
            self.message_key_after_reveal = false;
            self.message_key_wait = true;
            self.waiting_for_key = true;
            true
        } else {
            false
        }
    }

    pub fn message_key_waiting(&self) -> bool {
        self.message_key_wait
    }

    pub fn finish_message_key_wait(&mut self) {
        self.message_key_wait = false;
        self.waiting_for_key = false;
    }

    pub fn wait_quake(&mut self, wait: QuakeWait, key_skip: bool) {
        self.mark_block_request();
        self.quake = Some(wait);
        self.quake_key_skip = key_skip;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_audio(&mut self, w: AudioWait, key: bool) {
        self.wait_audio_with_return(w, key, false);
    }

    pub fn wait_audio_with_return(&mut self, w: AudioWait, key: bool, return_value_flag: bool) {
        self.mark_block_request();
        self.audio = Some(w);
        self.audio_key_skip = key;
        self.audio_return_value = return_value_flag;
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
            stage_form_id,
            stage_idx,
            runtime_slot,
            op,
            key_skip,
            return_value_flag,
            self.block_generation
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
            stage_form_id,
            stage_idx,
            runtime_slot,
            list_op,
            list_idx,
            key_skip,
            return_value_flag,
            self.block_generation
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
    }

    pub fn wait_object_emote(
        &mut self,
        stage_form_id: u32,
        stage_idx: i64,
        runtime_slot: usize,
        key_skip: bool,
        return_value_flag: bool,
    ) {
        self.mark_block_request();
        self.emote = Some(EmoteWait {
            stage_form_id,
            stage_idx,
            runtime_slot,
            return_value_flag,
        });
        self.emote_key_skip = key_skip;
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

    pub fn wait_pcm_event(
        &mut self,
        form_id: u32,
        index: usize,
        key_skip: bool,
        return_value_flag: bool,
    ) {
        self.mark_block_request();
        self.event = Some(EventWait::PcmEvent { form_id, index });
        self.event_key_skip = key_skip;
        self.event_return_value = return_value_flag;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_screen_effect(
        &mut self,
        form_id: u32,
        index: usize,
        op: i32,
        key_skip: bool,
        return_value_flag: bool,
    ) {
        self.mark_block_request();
        self.event = Some(EventWait::ScreenEffect { form_id, index, op });
        self.event_key_skip = key_skip;
        self.event_return_value = return_value_flag;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_stage_effect(
        &mut self,
        stage_form_id: u32,
        stage_idx: i64,
        index: usize,
        op: i32,
        key_skip: bool,
        return_value_flag: bool,
    ) {
        self.mark_block_request();
        self.event = Some(EventWait::StageEffect {
            stage_form_id,
            stage_idx,
            index,
            op,
        });
        self.event_key_skip = key_skip;
        self.event_return_value = return_value_flag;
        if key_skip {
            self.waiting_for_key = true;
        }
    }

    pub fn wait_mask_event(
        &mut self,
        form_id: u32,
        index: usize,
        op: i32,
        key_skip: bool,
        return_value_flag: bool,
    ) {
        self.mark_block_request();
        self.event = Some(EventWait::Mask { form_id, index, op });
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
        self.wait_wipe_with_return(key_skip, false);
    }

    pub fn wait_wipe_with_return(&mut self, key_skip: bool, return_value_flag: bool) {
        self.mark_block_request();
        self.wipe = true;
        self.wipe_key_skip = key_skip;
        self.wipe_return_value = return_value_flag;
        // C++ TNM_PROC_TYPE_WIPE_WAIT semantics (flow_proc.cpp
        // tnm_wipe_wait_proc): the wait releases when the wipe finishes by
        // time; a decide key press only *skips early*. It is never a hard
        // key wait, so waiting_for_key must stay untouched here.
    }

    /// Notify the wait system that a key/mouse input happened.
    ///
    /// Returns true if the input is interpreted as a wipe-skip (used by WIPE/WAIT_WIPE).
    pub fn notify_key(&mut self, _globals: &mut GlobalState, _ids: &RuntimeConstants) -> bool {
        let wipe_skipped = self.wipe && self.wipe_key_skip;
        self.waiting_for_key = false;
        self.generic_key_wait = false;
        self.generic_key_wait_skip_disabled = false;
        // C++ TIMEWAIT_KEY, event WAIT_KEY, and MOV/OBJECT movie waits are
        // not skipped by arbitrary key-down/mouse-down input here. They
        // consume DECIDE/CANCEL down-up in notify_movie_down_up(). Audio
        // waits follow the same rule, but consume DECIDE only.

        if wipe_skipped {
            self.wipe = false;
            self.wipe_key_skip = false;
            if self.wipe_return_value {
                self.pending_value = Some(Value::Int(1));
            }
            self.wipe_return_value = false;
        }

        wipe_skipped
    }

    /// Notify key-skippable waits that DECIDE/CANCEL completed a down-up pair.
    ///
    /// This matches C++ `tnm_time_wait_proc`, `tnm_mov_wait_proc`, and
    /// `tnm_obj_mov_wait_proc`: TIMEWAIT_KEY and MOV_WAIT_KEY consume only
    /// VK_EX_DECIDE or VK_EX_CANCEL down-up, returning 1 or -1 respectively.
    /// Generic key/mouse events must not skip these waits.
    pub fn notify_movie_down_up(
        &mut self,
        globals: &mut GlobalState,
        ids: &RuntimeConstants,
        result: i64,
    ) -> bool {
        let mut skipped = false;
        // C++ tnm_{bgm,koe,pcm,pcmch}_wait_proc consumes only a completed
        // VK_EX_DECIDE down-up for WAIT_KEY. It releases the wait but does not
        // stop the sound. CANCEL and arbitrary key-down events are ignored.
        if result == 1 && self.audio_key_skip {
            if self.audio.take().is_some() {
                if self.audio_return_value {
                    self.pending_value = Some(Value::Int(1));
                }
                skipped = true;
            }
            self.audio_key_skip = false;
            self.audio_return_value = false;
        }
        if self.skip_time_on_key && matches!(result, 1 | -1) {
            anim_skip_trace(format!(
                "notify_movie_down_up skipped TIMEWAIT_KEY pending={}",
                result
            ));
            self.until = None;
            self.skip_time_on_key = false;
            self.pending_value = Some(Value::Int(result));
            skipped = true;
        }
        if result == 1 && self.quake_key_skip {
            if let Some(wait) = self.quake.take() {
                stop_waited_quake(globals, wait);
                skipped = true;
            }
            self.quake_key_skip = false;
        }
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

        // OBJECT.EMOTE_WAIT_PLAYING_KEY consumes DECIDE only, returns 1 and
        // calls Pass() on the player; CANCEL does not release the wait.
        if self.emote_key_skip && result == 1 {
            if let Some(w) = self.emote.take() {
                if let Some(obj) = object_active_by_runtime_slot_mut(
                    globals,
                    w.stage_form_id,
                    w.stage_idx,
                    w.runtime_slot,
                ) && let Some(runtime) = obj.emote.runtime.as_mut()
                    && let Err(err) = runtime.pass()
                {
                    log::error!("EMOTE_WAIT_PLAYING_KEY Pass failed: {err:#}");
                }
                if w.return_value_flag {
                    self.pending_value = Some(Value::Int(1));
                }
                skipped = true;
            }
            self.emote_key_skip = false;
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
        self.selbtn = false;
        self.group_selection = None;
        self.until = None;
        self.mwnd_animation_wait = false;
        self.waiting_for_key = false;
        self.generic_key_wait = false;
        self.generic_key_wait_skip_disabled = false;
        self.message_reveal = false;
        self.message_key_after_reveal = false;
        self.message_key_wait = false;
        self.skip_time_on_key = false;
        self.audio = None;
        self.audio_key_skip = false;
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
        self.quake = None;
        self.quake_key_skip = false;
        self.pending_value = None;
        self.system_modal = false;
        self.wipe = false;
        self.wipe_key_skip = false;
        self.wipe_return_value = false;
    }
}

#[cfg(test)]
mod audio_wait_parity_tests {
    use super::*;
    use std::path::PathBuf;

    fn engines() -> (BgmEngine, KoeEngine, SeEngine, PcmEngine) {
        let root = PathBuf::from(".");
        (
            BgmEngine::new(root.clone()),
            KoeEngine::new(root.clone()),
            SeEngine::new(root.clone()),
            PcmEngine::new(root),
        )
    }

    #[test]
    fn audio_wait_key_ignores_generic_input_and_cancel_but_decide_returns_one() {
        let mut wait = VmWait::default();
        let mut globals = GlobalState::default();
        let ids = RuntimeConstants::default();

        wait.wait_audio_with_return(AudioWait::KoeAny, true, true);
        assert!(wait.audio.is_some());
        assert!(wait.audio_key_skip);
        assert!(!wait.waiting_for_key);

        // C++ tnm_koe_wait_proc does not consume arbitrary key-down input.
        assert!(!wait.notify_key(&mut globals, &ids));
        assert!(wait.audio.is_some());
        assert!(wait.pending_value.is_none());

        // KOE/PCM/BGM WAIT_KEY consumes VK_EX_DECIDE only, not CANCEL.
        assert!(!wait.notify_movie_down_up(&mut globals, &ids, -1));
        assert!(wait.audio.is_some());
        assert!(wait.pending_value.is_none());

        assert!(wait.notify_movie_down_up(&mut globals, &ids, 1));
        assert!(wait.audio.is_none());
        assert!(!wait.audio_key_skip);
        assert_eq!(wait.pending_value.take().and_then(|v| v.as_i64()), Some(1));
    }

    #[test]
    fn audio_wait_natural_finish_returns_zero_without_leaving_generic_key_wait() {
        let mut wait = VmWait::default();
        let (mut bgm, mut koe, mut se, mut pcm) = engines();
        let mut globals = GlobalState::default();
        let ids = RuntimeConstants::default();

        wait.wait_audio_with_return(AudioWait::KoeAny, true, true);
        assert!(!wait.is_blocked(
            &mut bgm,
            &mut koe,
            &mut se,
            &mut pcm,
            &mut globals,
            &ids,
            false,
        ));
        assert!(wait.audio.is_none());
        assert!(!wait.audio_key_skip);
        assert!(!wait.waiting_for_key);
        assert_eq!(wait.pending_value.take().and_then(|v| v.as_i64()), Some(0));
    }

    #[test]
    fn message_wait_preserves_cpp_proc_stack_order() {
        let mut wait = VmWait::default();

        wait.wait_message_reveal_then_key();
        assert!(wait.needs_continuous_frame());
        assert!(wait.message_reveal_waiting());
        assert!(!wait.message_key_waiting());
        assert!(!wait.waiting_for_key());

        assert!(wait.finish_message_reveal());
        assert!(wait.needs_runtime_poll());
        assert!(!wait.needs_continuous_frame());
        assert!(!wait.message_reveal_waiting());
        assert!(wait.message_key_waiting());
        assert!(wait.waiting_for_key());

        wait.finish_message_key_wait();
        assert!(!wait.message_key_waiting());
        assert!(!wait.waiting_for_key());
    }

    #[test]
    fn selbtn_wait_ignores_generic_key_release_and_preserves_second_choice() {
        let mut wait = VmWait::default();
        let (mut bgm, mut koe, mut se, mut pcm) = engines();
        let mut globals = GlobalState::default();
        let ids = RuntimeConstants::default();
        let mut stack = Vec::new();

        globals.selbtn.sync_type = 0;
        globals.selbtn.processing_flag_0 = true;
        wait.wait_selbtn();
        wait.set_selbtn_result(1);

        // Enter-up/message wait notifications are unrelated to SEL_BTN and
        // must not turn the pending second choice into the default FM_INT=0.
        assert!(!wait.notify_key(&mut globals, &ids));
        assert!(wait.poll(
            &mut stack,
            &mut bgm,
            &mut koe,
            &mut se,
            &mut pcm,
            &mut globals,
            &ids,
            false,
        ));
        assert!(stack.is_empty());

        globals.selbtn.processing_flag_0 = false;
        assert!(!wait.poll(
            &mut stack,
            &mut bgm,
            &mut koe,
            &mut se,
            &mut pcm,
            &mut globals,
            &ids,
            false,
        ));
        assert_eq!(stack.pop().and_then(|v| v.as_i64()), Some(1));
    }

    #[test]
    fn selbtn_sync_type_two_releases_at_decision() {
        let mut wait = VmWait::default();
        let (mut bgm, mut koe, mut se, mut pcm) = engines();
        let mut globals = GlobalState::default();
        let ids = RuntimeConstants::default();
        let mut stack = Vec::new();

        globals.selbtn.sync_type = 2;
        globals.selbtn.processing_flag_2 = true;
        wait.wait_selbtn();
        assert!(wait.poll(
            &mut stack,
            &mut bgm,
            &mut koe,
            &mut se,
            &mut pcm,
            &mut globals,
            &ids,
            false,
        ));

        wait.set_selbtn_result(1);
        globals.selbtn.processing_flag_2 = false;
        assert!(!wait.poll(
            &mut stack,
            &mut bgm,
            &mut koe,
            &mut se,
            &mut pcm,
            &mut globals,
            &ids,
            false,
        ));
        assert_eq!(stack.pop().and_then(|v| v.as_i64()), Some(1));
    }

    #[test]
    fn pure_key_wait_does_not_request_idle_frames() {
        let mut wait = VmWait::default();

        wait.wait_input_key(false);

        assert!(wait.needs_runtime_poll());
        assert!(!wait.needs_continuous_frame());
    }

    #[test]
    fn keylist_wait_obeys_skip_but_wait_force_does_not() {
        let (mut bgm, mut koe, mut se, mut pcm) = engines();
        let mut globals = GlobalState::default();
        let ids = RuntimeConstants::default();

        let mut wait = VmWait::default();
        wait.wait_input_key(false);
        assert!(!wait.is_blocked(
            &mut bgm,
            &mut koe,
            &mut se,
            &mut pcm,
            &mut globals,
            &ids,
            true,
        ));

        let mut forced = VmWait::default();
        forced.wait_input_key(true);
        assert!(forced.is_blocked(
            &mut bgm,
            &mut koe,
            &mut se,
            &mut pcm,
            &mut globals,
            &ids,
            true,
        ));
    }

    #[test]
    fn skipping_releases_audio_wait_with_zero_without_stopping_player_state() {
        let mut wait = VmWait::default();
        let (mut bgm, mut koe, mut se, mut pcm) = engines();
        let mut globals = GlobalState::default();
        let ids = RuntimeConstants::default();

        wait.wait_audio_with_return(AudioWait::KoeAny, true, true);
        assert!(!wait.is_blocked(
            &mut bgm,
            &mut koe,
            &mut se,
            &mut pcm,
            &mut globals,
            &ids,
            true,
        ));
        assert!(wait.audio.is_none());
        assert!(!wait.audio_key_skip);
        assert_eq!(wait.pending_value.take().and_then(|v| v.as_i64()), Some(0));
    }
}
