use anyhow::Result;

use crate::runtime::{globals::Counter, CommandContext, Value};

fn ensure_len(v: &mut Vec<Counter>, idx: usize) {
    if v.len() <= idx {
        v.resize(idx + 1, Counter::default());
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CounterListOp {
    GetSize,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CounterOp {
    Set,
    Get,
    Reset,
    Start,
    StartReal,
    StartFrame,
    StartFrameReal,
    StartFrameLoop,
    StartFrameLoopReal,
    Stop,
    Resume,
    Wait,
    WaitKey,
    CheckValue,
    CheckActive,
    Unknown,
}

fn resolve_counter_list_op(op: i32) -> CounterListOp {
    if op == crate::runtime::constants::COUNTERLIST_GET_SIZE {
        CounterListOp::GetSize
    } else {
        CounterListOp::Unknown
    }
}

fn resolve_counter_op(op: i32) -> CounterOp {
    match op {
        crate::runtime::constants::COUNTER_SET => CounterOp::Set,
        crate::runtime::constants::COUNTER_GET => CounterOp::Get,
        crate::runtime::constants::COUNTER_RESET => CounterOp::Reset,
        crate::runtime::constants::COUNTER_START => CounterOp::Start,
        crate::runtime::constants::COUNTER_START_REAL => CounterOp::StartReal,
        crate::runtime::constants::COUNTER_START_FRAME => CounterOp::StartFrame,
        crate::runtime::constants::COUNTER_START_FRAME_REAL => CounterOp::StartFrameReal,
        crate::runtime::constants::COUNTER_START_FRAME_LOOP => CounterOp::StartFrameLoop,
        crate::runtime::constants::COUNTER_START_FRAME_LOOP_REAL => CounterOp::StartFrameLoopReal,
        crate::runtime::constants::COUNTER_STOP => CounterOp::Stop,
        crate::runtime::constants::COUNTER_RESUME => CounterOp::Resume,
        crate::runtime::constants::COUNTER_WAIT => CounterOp::Wait,
        crate::runtime::constants::COUNTER_WAIT_KEY => CounterOp::WaitKey,
        crate::runtime::constants::COUNTER_CHECK_VALUE => CounterOp::CheckValue,
        crate::runtime::constants::COUNTER_CHECK_ACTIVE => CounterOp::CheckActive,
        _ => CounterOp::Unknown,
    }
}

fn arg_int(args: &[Value], idx: usize) -> i64 {
    args.get(idx).and_then(Value::as_i64).unwrap_or(0)
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let Some((chain_pos, chain_ref)) =
        crate::runtime::forms::prop_access::parse_element_chain_ctx(ctx, form_id, args)
    else {
        return Ok(false);
    };
    let chain = chain_ref.to_vec();

    if chain.len() >= 2 && chain[1] != ctx.ids.elm_array {
        match resolve_counter_list_op(chain[1]) {
            CounterListOp::GetSize => {
                let size = ctx
                    .globals
                    .counter_lists
                    .get(&form_id)
                    .map(|v| v.len() as i64)
                    .unwrap_or(0);
                ctx.push(Value::Int(size));
                return Ok(true);
            }
            CounterListOp::Unknown => return Ok(false),
        }
    }

    if chain.len() < 3 || chain[1] != ctx.ids.elm_array {
        return Ok(false);
    }
    let idx = chain[2].max(0) as usize;
    let params = crate::runtime::forms::prop_access::script_args(args, chain_pos);

    {
        let counters = ctx
            .globals
            .counter_lists
            .entry(form_id)
            .or_insert_with(Vec::new);
        ensure_len(counters, idx);
    }

    if chain.len() == 3 {
        let (al_id, _ret_form, rhs_value) =
            crate::runtime::forms::prop_access::infer_assign_and_ret_ctx(ctx, chain_pos, args);
        if al_id == Some(1) {
            let rhs = rhs_value.as_ref().and_then(Value::as_i64).unwrap_or(0);
            if let Some(counter) = ctx
                .globals
                .counter_lists
                .get_mut(&form_id)
                .and_then(|v| v.get_mut(idx))
            {
                counter.set_count(rhs);
            }
            ctx.push(Value::Int(0));
        } else {
            let value = ctx
                .globals
                .counter_lists
                .get(&form_id)
                .and_then(|v| v.get(idx))
                .map(|c| c.get_count())
                .unwrap_or(0);
            ctx.push(Value::Int(value));
        }
        return Ok(true);
    }

    match resolve_counter_op(chain[3]) {
        CounterOp::Set => {
            if let Some(counter) = ctx
                .globals
                .counter_lists
                .get_mut(&form_id)
                .and_then(|v| v.get_mut(idx))
            {
                counter.set_count(arg_int(params, 0));
            }
            ctx.push(Value::Int(0));
        }
        CounterOp::Get => {
            let value = ctx
                .globals
                .counter_lists
                .get(&form_id)
                .and_then(|v| v.get(idx))
                .map(|c| c.get_count())
                .unwrap_or(0);
            ctx.push(Value::Int(value));
        }
        CounterOp::Reset => {
            if let Some(counter) = ctx
                .globals
                .counter_lists
                .get_mut(&form_id)
                .and_then(|v| v.get_mut(idx))
            {
                counter.reset();
            }
            ctx.push(Value::Int(0));
        }
        CounterOp::Start => {
            if let Some(counter) = ctx
                .globals
                .counter_lists
                .get_mut(&form_id)
                .and_then(|v| v.get_mut(idx))
            {
                counter.start();
            }
            ctx.push(Value::Int(0));
        }
        CounterOp::StartReal => {
            if let Some(counter) = ctx
                .globals
                .counter_lists
                .get_mut(&form_id)
                .and_then(|v| v.get_mut(idx))
            {
                counter.start_real();
            }
            ctx.push(Value::Int(0));
        }
        CounterOp::StartFrame => {
            if let Some(counter) = ctx
                .globals
                .counter_lists
                .get_mut(&form_id)
                .and_then(|v| v.get_mut(idx))
            {
                counter.start_frame(arg_int(params, 0), arg_int(params, 1), arg_int(params, 2));
            }
            ctx.push(Value::Int(0));
        }
        CounterOp::StartFrameReal => {
            if let Some(counter) = ctx
                .globals
                .counter_lists
                .get_mut(&form_id)
                .and_then(|v| v.get_mut(idx))
            {
                counter.start_frame_real(
                    arg_int(params, 0),
                    arg_int(params, 1),
                    arg_int(params, 2),
                );
            }
            ctx.push(Value::Int(0));
        }
        CounterOp::StartFrameLoop => {
            if let Some(counter) = ctx
                .globals
                .counter_lists
                .get_mut(&form_id)
                .and_then(|v| v.get_mut(idx))
            {
                counter.start_frame_loop(
                    arg_int(params, 0),
                    arg_int(params, 1),
                    arg_int(params, 2),
                );
            }
            ctx.push(Value::Int(0));
        }
        CounterOp::StartFrameLoopReal => {
            if let Some(counter) = ctx
                .globals
                .counter_lists
                .get_mut(&form_id)
                .and_then(|v| v.get_mut(idx))
            {
                counter.start_frame_loop_real(
                    arg_int(params, 0),
                    arg_int(params, 1),
                    arg_int(params, 2),
                );
            }
            ctx.push(Value::Int(0));
        }
        CounterOp::Stop => {
            if let Some(counter) = ctx
                .globals
                .counter_lists
                .get_mut(&form_id)
                .and_then(|v| v.get_mut(idx))
            {
                counter.stop();
            }
            ctx.push(Value::Int(0));
        }
        CounterOp::Resume => {
            if let Some(counter) = ctx
                .globals
                .counter_lists
                .get_mut(&form_id)
                .and_then(|v| v.get_mut(idx))
            {
                counter.resume();
            }
            ctx.push(Value::Int(0));
        }
        CounterOp::Wait => {
            ctx.wait
                .wait_counter(form_id, idx, arg_int(params, 0), false, false);
        }
        CounterOp::WaitKey => {
            ctx.wait
                .wait_counter(form_id, idx, arg_int(params, 0), true, true);
        }
        CounterOp::CheckValue => {
            let target = arg_int(params, 0);
            let cur = ctx
                .globals
                .counter_lists
                .get(&form_id)
                .and_then(|v| v.get(idx))
                .map(|c| c.get_count())
                .unwrap_or(target);
            let ok = cur - target >= 0;
            if std::env::var_os("SG_COUNTER_TRACE").is_some() {
                eprintln!(
                    "[SG_DEBUG][COUNTER] CHECK_VALUE form={} idx={} cur={} target={} ok={}",
                    form_id, idx, cur, target, ok
                );
            }
            ctx.push(Value::Int(if ok { 1 } else { 0 }));
        }
        CounterOp::CheckActive => {
            let active = ctx
                .globals
                .counter_lists
                .get(&form_id)
                .and_then(|v| v.get(idx))
                .map(|c| c.is_running())
                .unwrap_or(false);
            ctx.push(Value::Int(if active { 1 } else { 0 }));
        }
        CounterOp::Unknown => return Ok(false),
    }

    Ok(true)
}
