use anyhow::{bail, Result};

use crate::runtime::{CommandContext, Value};

use super::codes::excall_op;

fn excall_form_key(ctx: &CommandContext) -> u32 {
    if ctx.ids.form_global_excall != 0 {
        ctx.ids.form_global_excall
    } else {
        super::codes::FORM_GLOBAL_EXCALL
    }
}

fn store_or_push_excall_prop(ctx: &mut CommandContext, form_key: u32, op: i64, args: &[Value]) {
    let prop = op as i32;
    if let Some(v) = args.get(1).cloned() {
        match v {
            Value::Str(s) => {
                ctx.globals
                    .str_props
                    .entry(form_key)
                    .or_default()
                    .insert(prop, s);
            }
            Value::Int(n) => {
                ctx.globals
                    .int_props
                    .entry(form_key)
                    .or_default()
                    .insert(prop, n);
            }
            _ => {}
        }
        ctx.push(Value::Int(0));
        return;
    }

    if let Some(s) = ctx
        .globals
        .str_props
        .get(&form_key)
        .and_then(|m| m.get(&prop))
        .cloned()
    {
        ctx.push(Value::Str(s));
        return;
    }

    let v = ctx
        .globals
        .int_props
        .get(&form_key)
        .and_then(|m| m.get(&prop).copied())
        .unwrap_or(0);
    ctx.push(Value::Int(v));
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    if args.is_empty() {
        bail!("EXCALL form expects at least one argument (op id)");
    }

    let op = match args[0] {
        Value::Int(v) => v,
        _ => {
            ctx.push(Value::Int(0));
            return Ok(true);
        }
    };

    let form_key = excall_form_key(ctx);

    match op {
        excall_op::ARRAY_INDEX => {
            let idx = args.get(1).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
            let set_value = args.get(2).and_then(|v| v.as_i64());
            let value = {
                let list = ctx.globals.int_lists.entry(form_key).or_insert_with(|| vec![0; 32]);
                if list.len() <= idx {
                    list.resize(idx + 1, 0);
                }
                if let Some(v) = set_value {
                    list[idx] = v;
                    Some(Value::Int(0))
                } else {
                    Some(Value::Int(list[idx]))
                }
            };
            if let Some(v) = value {
                ctx.push(v);
            }
        }
        excall_op::OP_8 => {
            if args.len() >= 2 {
                let v = args.get(1).and_then(|v| v.as_i64()).unwrap_or(0) != 0;
                ctx.syscalls.flag_2148 = v;
                ctx.push(Value::Int(0));
            } else {
                ctx.push(Value::Int(if ctx.syscalls.flag_2148 { 1 } else { 0 }));
            }
        }
        excall_op::OP_12 => {
            if args.len() >= 2 {
                let v = args.get(1).and_then(|v| v.as_i64()).unwrap_or(0) != 0;
                ctx.syscalls.flag_204 = v;
                ctx.push(Value::Int(0));
            } else {
                ctx.push(Value::Int(if ctx.syscalls.flag_204 { 1 } else { 0 }));
            }
        }
        excall_op::OP_0
        | excall_op::OP_1
        | excall_op::OP_2
        | excall_op::OP_3
        | excall_op::OP_4
        | excall_op::OP_5
        | excall_op::OP_6
        | excall_op::OP_7
        | excall_op::OP_9
        | excall_op::OP_10
        | excall_op::OP_13 => {
            store_or_push_excall_prop(ctx, form_key, op, args);
        }
        _ => {
            let _label = ctx.ids.excall_op_name(op).unwrap_or("UNKNOWN");
            store_or_push_excall_prop(ctx, form_key, op, args);
        }
    }

    Ok(true)
}
