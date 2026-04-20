use anyhow::{bail, Result};

use crate::runtime::{CommandContext, Value};

use super::codes::excall_op;
use super::{counter, frame_action, frame_action_ch, int_list, script, stage};

const EXCALL_LOCAL_NS_XOR: u32 = 0x4000;

fn excall_form_key(ctx: &CommandContext) -> u32 {
    if ctx.ids.form_global_excall != 0 {
        ctx.ids.form_global_excall
    } else {
        super::codes::FORM_GLOBAL_EXCALL
    }
}

fn stage_form_key(ctx: &CommandContext) -> u32 {
    if ctx.ids.form_global_stage != 0 {
        ctx.ids.form_global_stage
    } else {
        super::codes::FORM_GLOBAL_STAGE
    }
}

fn script_form_key(ctx: &CommandContext) -> u32 {
    if ctx.ids.form_global_script != 0 {
        ctx.ids.form_global_script
    } else {
        0
    }
}

fn excall_stage_form_key(ctx: &CommandContext, selector: i32) -> u32 {
    let base = stage_form_key(ctx);
    if selector == 0 {
        base
    } else {
        base ^ EXCALL_LOCAL_NS_XOR
    }
}

fn synth_form_key(base: u32, selector: i32, op: i32) -> u32 {
    (base << 8) ^ (((selector as u32) & 0x0f) << 4) ^ (op as u32 & 0x0f)
}

fn parse_call<'a>(
    ctx: &'a CommandContext,
    args: &'a [Value],
) -> Option<(
    usize,
    &'a [i32],
    i32,
    i32,
    &'a [Value],
    Option<i64>,
    Option<i64>,
)> {
    let form_id = excall_form_key(ctx);
    let (chain_pos, chain) = super::prop_access::parse_element_chain_ctx(ctx, form_id, args)?;
    let (selector, op_pos) = if chain.len() >= 3
        && chain[1] == crate::runtime::forms::codes::ELM_ARRAY
        && (chain[2] == 0 || chain[2] == 1)
    {
        (chain[2], 3usize)
    } else {
        (1i32, 1usize)
    };
    let op = chain
        .get(op_pos)
        .copied()
        .or_else(|| args.get(0).and_then(|v| v.as_i64()).map(|v| v as i32))?;
    let params = if chain_pos > 1 {
        &args[1..chain_pos]
    } else {
        &[]
    };
    let (meta_al_id, meta_ret_form) = crate::runtime::forms::prop_access::current_vm_meta(ctx);
    let al_id = meta_al_id;
    let ret_form = meta_ret_form;
    Some((chain_pos, chain, selector, op, params, al_id, ret_form))
}

fn translated_call_args(
    form_id: u32,
    chain_tail: &[i32],
    params: &[Value],
    al_id: Option<i64>,
    ret_form: Option<i64>,
) -> Vec<Value> {
    let mut out = Vec::new();
    let op0 = chain_tail.first().copied().unwrap_or(0) as i64;
    out.push(Value::Int(op0));
    out.extend(params.iter().cloned());
    let mut chain = Vec::with_capacity(1 + chain_tail.len());
    chain.push(form_id as i32);
    chain.extend_from_slice(chain_tail);
    out.push(Value::Element(chain));
    out.push(Value::Int(al_id.unwrap_or(0)));
    out.push(Value::Int(ret_form.unwrap_or(0)));
    out
}

fn translated_stage_args_to_form(
    stage_form_id: u32,
    stage_idx: Option<i32>,
    chain_tail: &[i32],
    params: &[Value],
    al_id: Option<i64>,
    ret_form: Option<i64>,
) -> Vec<Value> {
    let mut out = Vec::new();
    let op0 = chain_tail.first().copied().unwrap_or(0) as i64;
    out.push(Value::Int(op0));
    out.extend(params.iter().cloned());

    let mut chain = Vec::new();
    chain.push(stage_form_id as i32);
    if let Some(idx) = stage_idx {
        chain.push(crate::runtime::forms::codes::ELM_ARRAY);
        chain.push(idx);
    }
    chain.extend_from_slice(chain_tail);
    out.push(Value::Element(chain));
    out.push(Value::Int(al_id.unwrap_or(0)));
    out.push(Value::Int(ret_form.unwrap_or(0)));
    out
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let Some((_chain_pos, chain, selector, op, params, al_id, ret_form)) = parse_call(ctx, args)
    else {
        if args.is_empty() {
            bail!("EXCALL form expects at least one argument (op id)");
        }
        return Ok(false);
    };

    let op_pos = if chain.len() >= 3
        && chain[1] == crate::runtime::forms::codes::ELM_ARRAY
        && (chain[2] == 0 || chain[2] == 1)
    {
        3usize
    } else {
        1usize
    };
    let tail = chain.get(op_pos + 1..).unwrap_or(&[]);
    let form_key = excall_form_key(ctx);

    match op {
        excall_op::OP_4 => {
            ctx.excall_state.ready = true;
            ctx.push(Value::Int(0));
        }
        excall_op::OP_5 => {
            ctx.excall_state.ready = false;
            ctx.push(Value::Int(0));
        }
        excall_op::OP_8 => {
            ctx.push(Value::Int(if ctx.excall_state.ready { 1 } else { 0 }));
        }
        excall_op::OP_12 => {
            ctx.push(Value::Int(if selector == 1 { 1 } else { 0 }));
        }
        excall_op::OP_0 => {
            let forwarded = translated_stage_args_to_form(
                excall_stage_form_key(ctx, selector),
                None,
                tail,
                params,
                al_id,
                ret_form,
            );
            return stage::dispatch(ctx, &forwarded);
        }
        excall_op::OP_1 => {
            let forwarded = translated_stage_args_to_form(
                excall_stage_form_key(ctx, selector),
                Some(0),
                tail,
                params,
                al_id,
                ret_form,
            );
            return stage::dispatch(ctx, &forwarded);
        }
        excall_op::OP_2 => {
            let forwarded = translated_stage_args_to_form(
                excall_stage_form_key(ctx, selector),
                Some(1),
                tail,
                params,
                al_id,
                ret_form,
            );
            return stage::dispatch(ctx, &forwarded);
        }
        excall_op::OP_3 => {
            let forwarded = translated_stage_args_to_form(
                excall_stage_form_key(ctx, selector),
                Some(2),
                tail,
                params,
                al_id,
                ret_form,
            );
            return stage::dispatch(ctx, &forwarded);
        }
        excall_op::OP_6 => {
            let forwarded = translated_call_args(
                synth_form_key(form_key, selector, op),
                tail,
                params,
                al_id,
                ret_form,
            );
            return counter::dispatch(ctx, synth_form_key(form_key, selector, op), &forwarded);
        }
        excall_op::OP_7 => {
            let forwarded = translated_call_args(
                synth_form_key(form_key, selector, op),
                tail,
                params,
                al_id,
                ret_form,
            );
            return int_list::dispatch(ctx, synth_form_key(form_key, selector, op), &forwarded);
        }
        excall_op::OP_9 => {
            let forwarded = translated_call_args(
                synth_form_key(form_key, selector, op),
                tail,
                params,
                al_id,
                ret_form,
            );
            return frame_action::dispatch(ctx, synth_form_key(form_key, selector, op), &forwarded);
        }
        excall_op::OP_10 => {
            let forwarded = translated_call_args(
                synth_form_key(form_key, selector, op),
                tail,
                params,
                al_id,
                ret_form,
            );
            return frame_action_ch::dispatch(
                ctx,
                synth_form_key(form_key, selector, op),
                &forwarded,
            );
        }
        excall_op::OP_13 => {
            let script_form = script_form_key(ctx);
            if script_form == 0 {
                ctx.push(Value::Int(0));
                return Ok(true);
            }
            let forwarded = translated_call_args(script_form, tail, params, al_id, ret_form);
            return script::dispatch(ctx, script_form, &forwarded);
        }
        _ => {
            let _ = form_key;
            ctx.push(Value::Int(0));
        }
    }

    Ok(true)
}
