use anyhow::{bail, Result};

use crate::runtime::forms::codes::{int_event_list_op, int_event_op};
use crate::runtime::int_event::IntEvent;
use crate::runtime::{CommandContext, Value};

use super::prop_access;

fn default_for_ret_form(ret_form: i64) -> Value {
    if prop_access::ret_form_is_string(ret_form) {
        Value::Str(String::new())
    } else {
        Value::Int(0)
    }
}

fn parse_chain<'a>(
    ctx: &'a CommandContext,
    form_id: u32,
    args: &'a [Value],
) -> Option<(usize, &'a [i32])> {
    prop_access::parse_element_chain_ctx(ctx, form_id, args)
}

fn collect_params<'a>(chain_pos: usize, args: &'a [Value]) -> &'a [Value] {
    prop_access::script_args(args, chain_pos)
}

enum PostAction {
    Push(Value),
    Wait {
        index: Option<usize>,
        key_skip: bool,
    },
}

fn apply_named_init(ev: &mut IntEvent, args: &[Value], chain_pos: usize) {
    let start = (chain_pos + 3).min(args.len());
    for arg in &args[start..] {
        if let Value::NamedArg { id: 0, value } = arg {
            if let Some(v) = value.as_i64() {
                *ev = IntEvent::new(v as i32);
            }
        }
    }
}

fn run_event_op(
    ev: &mut IntEvent,
    op: i32,
    params: &[Value],
    ret_form: Option<i64>,
    args: &[Value],
    chain_pos: usize,
    index: Option<usize>,
) -> Result<PostAction> {
    let post = match op {
        int_event_op::SET | int_event_op::SET_REAL => {
            apply_named_init(ev, args, chain_pos);
            let value = params.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let total_time = params.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let delay_time = params.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let speed_type = params.get(3).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let real_flag = if op == int_event_op::SET_REAL { 1 } else { 0 };
            ev.set_event(value, total_time, delay_time, speed_type, real_flag);
            PostAction::Push(default_for_ret_form(ret_form.unwrap_or(0)))
        }
        int_event_op::LOOP | int_event_op::LOOP_REAL => {
            let start_value = params.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let end_value = params.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let loop_time = params.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let delay_time = params.get(3).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let speed_type = params.get(4).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let real_flag = if op == int_event_op::LOOP_REAL { 1 } else { 0 };
            ev.loop_event(
                start_value,
                end_value,
                loop_time,
                delay_time,
                speed_type,
                real_flag,
            );
            PostAction::Push(default_for_ret_form(ret_form.unwrap_or(0)))
        }
        int_event_op::TURN | int_event_op::TURN_REAL => {
            let start_value = params.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let end_value = params.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let loop_time = params.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let delay_time = params.get(3).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let speed_type = params.get(4).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let real_flag = if op == int_event_op::TURN_REAL { 1 } else { 0 };
            ev.turn_event(
                start_value,
                end_value,
                loop_time,
                delay_time,
                speed_type,
                real_flag,
            );
            PostAction::Push(default_for_ret_form(ret_form.unwrap_or(0)))
        }
        int_event_op::END => {
            ev.end_event();
            PostAction::Push(default_for_ret_form(ret_form.unwrap_or(0)))
        }
        int_event_op::WAIT => PostAction::Wait {
            index,
            key_skip: false,
        },
        int_event_op::WAIT_KEY => PostAction::Wait {
            index,
            key_skip: true,
        },
        int_event_op::CHECK => PostAction::Push(Value::Int(if ev.check_event() { 1 } else { 0 })),
        _ => bail!("unsupported INTEVENT op {}", op),
    };
    Ok(post)
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let Some((chain_pos, chain)) = parse_chain(ctx, form_id, args) else {
        return Ok(false);
    };

    let params = collect_params(chain_pos, args);
    let (_al_id, ret_form, rhs) =
        super::prop_access::infer_assign_and_ret_ctx(ctx, chain_pos, args);

    let post = if form_id as i32 == crate::runtime::forms::codes::FM_INTEVENTLIST {
        if chain.len() >= 4
            && (chain[1] == ctx.ids.elm_array
                || chain[1] == crate::runtime::forms::codes::ELM_ARRAY)
        {
            let idx = chain[2].max(0) as usize;
            let op = chain[3];
            let list = ctx
                .globals
                .int_event_lists
                .entry(form_id)
                .or_insert_with(Vec::new);
            if list.len() <= idx {
                list.resize_with(idx + 1, || IntEvent::new(0));
            }
            let ev = &mut list[idx];
            run_event_op(ev, op, params, ret_form, args, chain_pos, Some(idx))?
        } else if chain.len() >= 2 && chain[1] == int_event_list_op::RESIZE {
            let n = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
            let list = ctx
                .globals
                .int_event_lists
                .entry(form_id)
                .or_insert_with(Vec::new);
            list.resize_with(n, || IntEvent::new(0));
            PostAction::Push(default_for_ret_form(ret_form.unwrap_or(0)))
        } else {
            bail!("unsupported INTEVENTLIST op chain {:?}", chain)
        }
    } else {
        let op = chain.get(1).copied().unwrap_or(0);
        let ev = ctx
            .globals
            .int_event_roots
            .entry(form_id)
            .or_insert_with(|| IntEvent::new(0));
        if let Some(Value::Int(v)) = rhs {
            *ev = IntEvent::new(v as i32);
            PostAction::Push(default_for_ret_form(ret_form.unwrap_or(0)))
        } else {
            run_event_op(ev, op, params, ret_form, args, chain_pos, None)?
        }
    };

    match post {
        PostAction::Push(v) => ctx.push(v),
        PostAction::Wait { index, key_skip } => ctx
            .wait
            .wait_generic_int_event(form_id, index, key_skip, key_skip),
    }
    Ok(true)
}
