use anyhow::Result;
use std::collections::HashMap;

use crate::runtime::{CommandContext, Value};

fn parse_element_chain<'a>(form_id: u32, args: &'a [Value]) -> Option<(usize, &'a [i32])> {
    for (i, v) in args.iter().enumerate() {
        if let Value::Element(ch) = v {
            if ch.first().copied() == Some(form_id as i32) {
                return Some((i, ch.as_slice()));
            }
        }
    }
    None
}

fn int_map<'a>(ctx: &'a mut CommandContext, form_id: u32) -> &'a mut HashMap<i32, i64> {
    ctx.globals
        .int_props
        .entry(form_id)
        .or_insert_with(HashMap::new)
}

fn str_map<'a>(ctx: &'a mut CommandContext, form_id: u32) -> &'a mut HashMap<i32, String> {
    ctx.globals
        .str_props
        .entry(form_id)
        .or_insert_with(HashMap::new)
}

/// SCRIPT global form.
///
/// In the original engine, this covers script execution / metadata / state.
/// We do not guess those semantics here.
///
/// Bring-up model: a conservative property-bag (same as SYSCOM).
pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let (op, params, rhs_assign) = if let Some((pos, chain)) = parse_element_chain(form_id, args) {
        if chain.len() < 2 {
            ctx.push(Value::Int(0));
            return Ok(true);
        }

        let op = chain[1];
        let params = &args[..pos];

        let mut assign: Option<Value> = None;
        if pos == 3 {
            if args.get(1).and_then(|v| v.as_i64()) == Some(1) {
                assign = args.get(2).cloned();
            }
        }
        if assign.is_none() && pos == 1 {
            if args.get(pos + 1).and_then(|v| v.as_i64()) == Some(1) {
                assign = args.get(0).cloned();
            }
        }
        (op, params, assign)
    } else {
        let Some(op_i64) = args.get(0).and_then(|v| v.as_i64()) else {
            ctx.unknown.record_unimplemented("SCRIPT/invalid-op");
            ctx.push(Value::Int(0));
            return Ok(true);
        };
        let params = &args[1..];
        let assign = params.get(0).cloned();
        (op_i64 as i32, params, assign)
    };

    let mut assign = rhs_assign;
    if assign.is_none() {
        assign = params.get(0).cloned();
    }

    if let Some(v) = assign {
        match v {
            Value::Str(s) => {
                str_map(ctx, form_id).insert(op, s);
            }
            Value::Int(n) => {
                int_map(ctx, form_id).insert(op, n);
            }
            _ => {}
        }
        ctx.push(Value::Int(0));
        return Ok(true);
    }

    if let Some(s) = ctx
        .globals
        .str_props
        .get(&form_id)
        .and_then(|m| m.get(&op))
        .cloned()
    {
        ctx.push(Value::Str(s));
        return Ok(true);
    }

    let v = ctx
        .globals
        .int_props
        .get(&form_id)
        .and_then(|m| m.get(&op).copied())
        .unwrap_or(0);
    if v == 0 {
        ctx.unknown
            .record_unimplemented(&format!("SCRIPT/get op={op}"));
    }
    ctx.push(Value::Int(v));
    Ok(true)
}
