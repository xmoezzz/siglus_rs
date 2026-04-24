use anyhow::{bail, Result};

use crate::runtime::forms::codes::{elm_value, ELM_ARRAY, FM_OBJECTEVENT, FM_OBJECTEVENTLIST};
use crate::runtime::globals::{ObjectEventTarget, ObjectState, StageFormState};
use crate::runtime::{CommandContext, Value};

use super::prop_access;

fn as_i64(v: &Value) -> Option<i64> {
    v.as_i64()
}

fn default_push(ctx: &mut CommandContext) {
    ctx.push(Value::Int(0));
}

fn parse_chain<'a>(ctx: &'a CommandContext, args: &'a [Value]) -> Option<(usize, &'a [i32])> {
    prop_access::parse_element_chain_ctx(ctx, FM_OBJECTEVENT as u32, args)
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

fn object_by_runtime_slot<'a>(
    st: &'a StageFormState,
    stage_idx: i64,
    runtime_slot: usize,
) -> Option<&'a ObjectState> {
    st.object_lists
        .get(&stage_idx)
        .and_then(|list| find_object_by_runtime_slot(list, runtime_slot))
}

fn object_by_runtime_slot_mut<'a>(
    st: &'a mut StageFormState,
    stage_idx: i64,
    runtime_slot: usize,
) -> Option<&'a mut ObjectState> {
    st.object_lists
        .get_mut(&stage_idx)
        .and_then(|list| find_object_by_runtime_slot_mut(list, runtime_slot))
}

fn target_for_set_loop_turn_stop_wait(op: i32) -> Option<ObjectEventTarget> {
    match op {
        elm_value::OBJECTEVENT_SET_X
        | elm_value::OBJECTEVENT_LOOP_X
        | elm_value::OBJECTEVENT_TURN_X
        | elm_value::OBJECTEVENT_STOP_X
        | elm_value::OBJECTEVENT_WAIT_X => Some(ObjectEventTarget::X),
        elm_value::OBJECTEVENT_SET_Y
        | elm_value::OBJECTEVENT_LOOP_Y
        | elm_value::OBJECTEVENT_TURN_Y
        | elm_value::OBJECTEVENT_STOP_Y
        | elm_value::OBJECTEVENT_WAIT_Y => Some(ObjectEventTarget::Y),
        elm_value::OBJECTEVENT_SET_Z
        | elm_value::OBJECTEVENT_LOOP_Z
        | elm_value::OBJECTEVENT_TURN_Z
        | elm_value::OBJECTEVENT_STOP_Z
        | elm_value::OBJECTEVENT_WAIT_Z => Some(ObjectEventTarget::Z),
        elm_value::OBJECTEVENT_SET_SCALE_X
        | elm_value::OBJECTEVENT_STOP_SCALE_X
        | elm_value::OBJECTEVENT_WAIT_SCALE_X => Some(ObjectEventTarget::ScaleX),
        elm_value::OBJECTEVENT_SET_SCALE_Y
        | elm_value::OBJECTEVENT_STOP_SCALE_Y
        | elm_value::OBJECTEVENT_WAIT_SCALE_Y => Some(ObjectEventTarget::ScaleY),
        elm_value::OBJECTEVENT_SET_SCALE_Z
        | elm_value::OBJECTEVENT_STOP_SCALE_Z
        | elm_value::OBJECTEVENT_WAIT_SCALE_Z => Some(ObjectEventTarget::ScaleZ),
        elm_value::OBJECTEVENT_SET_ROTATE_X
        | elm_value::OBJECTEVENT_STOP_ROTATE_X
        | elm_value::OBJECTEVENT_WAIT_ROTATE_X => Some(ObjectEventTarget::RotateX),
        elm_value::OBJECTEVENT_SET_ROTATE_Y
        | elm_value::OBJECTEVENT_STOP_ROTATE_Y
        | elm_value::OBJECTEVENT_WAIT_ROTATE_Y => Some(ObjectEventTarget::RotateY),
        elm_value::OBJECTEVENT_SET_ROTATE_Z
        | elm_value::OBJECTEVENT_STOP_ROTATE_Z
        | elm_value::OBJECTEVENT_WAIT_ROTATE_Z => Some(ObjectEventTarget::RotateZ),
        elm_value::OBJECTEVENT_SET_TR
        | elm_value::OBJECTEVENT_LOOP_TR
        | elm_value::OBJECTEVENT_TURN_TR
        | elm_value::OBJECTEVENT_STOP_TR
        | elm_value::OBJECTEVENT_WAIT_TR => Some(ObjectEventTarget::Tr),
        _ => None,
    }
}

fn event_prop_for_target(ctx: &CommandContext, target: ObjectEventTarget) -> i32 {
    match target {
        ObjectEventTarget::X => ctx.ids.obj_x_eve,
        ObjectEventTarget::Y => ctx.ids.obj_y_eve,
        ObjectEventTarget::Z => ctx.ids.obj_z_eve,
        ObjectEventTarget::ScaleX => ctx.ids.obj_scale_x_eve,
        ObjectEventTarget::ScaleY => ctx.ids.obj_scale_y_eve,
        ObjectEventTarget::ScaleZ => ctx.ids.obj_scale_z_eve,
        ObjectEventTarget::RotateX => ctx.ids.obj_rotate_x_eve,
        ObjectEventTarget::RotateY => ctx.ids.obj_rotate_y_eve,
        ObjectEventTarget::RotateZ => ctx.ids.obj_rotate_z_eve,
        ObjectEventTarget::Tr => ctx.ids.obj_tr_eve,
        _ => 0,
    }
}

fn is_set_op(op: i32) -> bool {
    matches!(
        op,
        elm_value::OBJECTEVENT_SET_X
            | elm_value::OBJECTEVENT_SET_Y
            | elm_value::OBJECTEVENT_SET_Z
            | elm_value::OBJECTEVENT_SET_SCALE_X
            | elm_value::OBJECTEVENT_SET_SCALE_Y
            | elm_value::OBJECTEVENT_SET_SCALE_Z
            | elm_value::OBJECTEVENT_SET_ROTATE_X
            | elm_value::OBJECTEVENT_SET_ROTATE_Y
            | elm_value::OBJECTEVENT_SET_ROTATE_Z
            | elm_value::OBJECTEVENT_SET_TR
    )
}

fn is_loop_op(op: i32) -> bool {
    matches!(
        op,
        elm_value::OBJECTEVENT_LOOP_X
            | elm_value::OBJECTEVENT_LOOP_Y
            | elm_value::OBJECTEVENT_LOOP_Z
            | elm_value::OBJECTEVENT_LOOP_TR
    )
}

fn is_turn_op(op: i32) -> bool {
    matches!(
        op,
        elm_value::OBJECTEVENT_TURN_X
            | elm_value::OBJECTEVENT_TURN_Y
            | elm_value::OBJECTEVENT_TURN_Z
            | elm_value::OBJECTEVENT_TURN_TR
    )
}

fn is_stop_op(op: i32) -> bool {
    matches!(
        op,
        elm_value::OBJECTEVENT_STOP_X
            | elm_value::OBJECTEVENT_STOP_Y
            | elm_value::OBJECTEVENT_STOP_Z
            | elm_value::OBJECTEVENT_STOP_SCALE_X
            | elm_value::OBJECTEVENT_STOP_SCALE_Y
            | elm_value::OBJECTEVENT_STOP_SCALE_Z
            | elm_value::OBJECTEVENT_STOP_ROTATE_X
            | elm_value::OBJECTEVENT_STOP_ROTATE_Y
            | elm_value::OBJECTEVENT_STOP_ROTATE_Z
            | elm_value::OBJECTEVENT_STOP_TR
    )
}

fn is_wait_op(op: i32) -> bool {
    matches!(
        op,
        elm_value::OBJECTEVENT_WAIT_X
            | elm_value::OBJECTEVENT_WAIT_Y
            | elm_value::OBJECTEVENT_WAIT_Z
            | elm_value::OBJECTEVENT_WAIT_SCALE_X
            | elm_value::OBJECTEVENT_WAIT_SCALE_Y
            | elm_value::OBJECTEVENT_WAIT_SCALE_Z
            | elm_value::OBJECTEVENT_WAIT_ROTATE_X
            | elm_value::OBJECTEVENT_WAIT_ROTATE_Y
            | elm_value::OBJECTEVENT_WAIT_ROTATE_Z
            | elm_value::OBJECTEVENT_WAIT_TR
    )
}

fn dispatch_object_event_on_runtime_slot(
    ctx: &mut CommandContext,
    stage_idx: i64,
    runtime_slot: usize,
    op: i32,
    script_args: &[Value],
) -> Result<bool> {
    if op == elm_value::OBJECTEVENT_WAIT_ALL {
        let active = {
            let stage_form = ctx.ids.form_global_stage;
            ctx.globals
                .stage_forms
                .get(&stage_form)
                .and_then(|st| object_by_runtime_slot(st, stage_idx, runtime_slot))
                .map(|o| o.any_event_active())
                .unwrap_or(false)
        };
        if active {
            ctx.wait.wait_object_all_events(
                ctx.ids.form_global_stage,
                stage_idx,
                runtime_slot,
                false,
            );
        }
        default_push(ctx);
        return Ok(true);
    }

    if op == elm_value::OBJECTEVENT_STOP_ALL {
        let stage_form = ctx.ids.form_global_stage;
        if let Some(st) = ctx.globals.stage_forms.get_mut(&stage_form) {
            if let Some(obj) = object_by_runtime_slot_mut(st, stage_idx, runtime_slot) {
                obj.end_all_events();
            }
        }
        default_push(ctx);
        return Ok(true);
    }

    let Some(target) = target_for_set_loop_turn_stop_wait(op) else {
        bail!("unsupported OBJECTEVENT op {}", op);
    };
    let event_prop = event_prop_for_target(ctx, target);
    if event_prop == 0 {
        bail!("OBJECTEVENT op {} has no mapped object event property", op);
    }

    if is_wait_op(op) {
        let active = {
            let stage_form = ctx.ids.form_global_stage;
            ctx.globals
                .stage_forms
                .get(&stage_form)
                .and_then(|st| object_by_runtime_slot(st, stage_idx, runtime_slot))
                .and_then(|obj| obj.runtime.prop_events.get(target))
                .map(|ev| ev.check_event())
                .unwrap_or(false)
        };
        if active {
            ctx.wait.wait_object_event(
                ctx.ids.form_global_stage,
                stage_idx,
                runtime_slot,
                event_prop,
                false,
                false,
            );
        }
        default_push(ctx);
        return Ok(true);
    }

    let stage_form = ctx.ids.form_global_stage;
    let st: &mut StageFormState = ctx.globals.stage_forms.entry(stage_form).or_default();
    let Some(obj) = object_by_runtime_slot_mut(st, stage_idx, runtime_slot) else {
        return Ok(false);
    };
    let Some(ev) = obj.runtime.prop_events.get_mut(target) else {
        bail!(
            "OBJECTEVENT target {:?} is not backed by an object IntEvent",
            target
        );
    };

    if is_set_op(op) {
        let value = script_args.first().and_then(as_i64).unwrap_or(0) as i32;
        let total_time = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
        let delay_time = script_args.get(2).and_then(as_i64).unwrap_or(0) as i32;
        let speed_type = script_args.get(3).and_then(as_i64).unwrap_or(0) as i32;
        ev.set_event(value, total_time, delay_time, speed_type, 0);
        default_push(ctx);
        return Ok(true);
    }

    if is_loop_op(op) {
        let start_value = script_args.first().and_then(as_i64).unwrap_or(0) as i32;
        let end_value = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
        let loop_time = script_args.get(2).and_then(as_i64).unwrap_or(0) as i32;
        let delay_time = script_args.get(3).and_then(as_i64).unwrap_or(0) as i32;
        ev.loop_event(start_value, end_value, loop_time, delay_time, 0, 0);
        default_push(ctx);
        return Ok(true);
    }

    if is_turn_op(op) {
        let start_value = script_args.first().and_then(as_i64).unwrap_or(0) as i32;
        let end_value = script_args.get(1).and_then(as_i64).unwrap_or(0) as i32;
        let loop_time = script_args.get(2).and_then(as_i64).unwrap_or(0) as i32;
        let delay_time = script_args.get(3).and_then(as_i64).unwrap_or(0) as i32;
        ev.turn_event(start_value, end_value, loop_time, delay_time, 0, 0);
        default_push(ctx);
        return Ok(true);
    }

    if is_stop_op(op) {
        ev.end_event();
        default_push(ctx);
        return Ok(true);
    }

    bail!("unsupported OBJECTEVENT op {}", op)
}

fn object_runtime_slot_by_stage_index(
    st: &StageFormState,
    stage_idx: i64,
    object_idx: usize,
) -> Option<usize> {
    st.object_lists
        .get(&stage_idx)
        .and_then(|list| list.get(object_idx))
        .map(|obj| obj.runtime_slot_or(object_idx))
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let Some((chain_pos, chain)) = parse_chain(ctx, args) else {
        return Ok(false);
    };
    if chain.len() < 2 {
        return Ok(false);
    }
    let op = chain[1];
    let script_args = prop_access::script_args(args, chain_pos);
    let Some((stage_idx, runtime_slot)) = ctx.globals.current_stage_object else {
        return Ok(false);
    };

    dispatch_object_event_on_runtime_slot(ctx, stage_idx, runtime_slot, op, script_args)
}

pub fn dispatch_list(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let Some((chain_pos, chain)) =
        prop_access::parse_element_chain_ctx(ctx, FM_OBJECTEVENTLIST as u32, args)
    else {
        return Ok(false);
    };
    if chain.len() < 3 {
        bail!("OBJECTEVENTLIST.ARRAY requires an index");
    }
    if chain[1] != ELM_ARRAY && chain[1] != elm_value::OBJECTEVENTLIST_ARRAY {
        bail!("unsupported OBJECTEVENTLIST op {}", chain[1]);
    }

    if chain.len() == 3 {
        ctx.push(Value::Element(chain.to_vec()));
        return Ok(true);
    }

    if chain[2] < 0 {
        bail!(
            "OBJECTEVENTLIST.ARRAY index must be non-negative: {}",
            chain[2]
        );
    }

    let Some((stage_idx, _ambient_runtime_slot)) = ctx.globals.current_stage_object else {
        return Ok(false);
    };
    let object_idx = chain[2] as usize;
    let op = chain[3];
    let script_args = prop_access::script_args(args, chain_pos);

    let runtime_slot = {
        let stage_form = ctx.ids.form_global_stage;
        let Some(st) = ctx.globals.stage_forms.get(&stage_form) else {
            return Ok(false);
        };
        let Some(runtime_slot) = object_runtime_slot_by_stage_index(st, stage_idx, object_idx)
        else {
            bail!(
                "OBJECTEVENTLIST.ARRAY[{}] has no object in stage {}",
                object_idx,
                stage_idx
            );
        };
        runtime_slot
    };

    dispatch_object_event_on_runtime_slot(ctx, stage_idx, runtime_slot, op, script_args)
}
