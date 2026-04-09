use anyhow::Result;

use crate::runtime::{CommandContext, Value};

use super::{key, prop_access};

const KEYLIST_FORM_STATE: u32 = 0xFF00_0003;

fn find_chain(args: &[Value]) -> Option<Vec<i32>> {
    for v in args.iter().rev() {
        if let Value::Element(e) = v {
            return Some(e.clone());
        }
    }
    None
}

fn arg_int(args: &[Value], idx: usize) -> Option<i64> {
    match args.get(idx) {
        Some(Value::Int(v)) => Some(*v),
        _ => None,
    }
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    // Support both direct-op calls and Element-chain property-style calls.
    let chain = find_chain(args);

    // If we have a chain rooted at KEYLIST, parse it first.
    if let Some(ref c) = chain {
        if !c.is_empty() && c[0] == ctx.ids.form_global_keylist as i32 {
            return dispatch_chain(ctx, c, args);
        }
    }

    // Otherwise, treat args[0] as the op id.
    let Some(Value::Int(op)) = args.get(0) else {
        return Ok(false);
    };

    dispatch_op(ctx, *op, args)
}

fn dispatch_chain(ctx: &mut CommandContext, chain: &[i32], args: &[Value]) -> Result<bool> {
    if chain.len() < 2 {
        return Ok(false);
    }
    let op = chain[1] as i64;

    // Property-style key query:
    //   [FORM_KEYLIST, ELM_ARRAY, vk, key_op?]
    if op == ctx.ids.elm_array as i64 {
        let vk = chain.get(2).copied().unwrap_or(0) as i64;
        let key_op = chain
            .get(3)
            .copied()
            .map(|x| x as i64)
            .unwrap_or(ctx.ids.key_op_is_down as i64);
        let v = key::query(ctx, vk, key_op);
        ctx.push(Value::Int(v));
        return Ok(true);
    }

    // Otherwise treat as a normal KEYLIST op (WAIT/CLEAR/NEXT/etc.).
    dispatch_op(ctx, op, args)
}

fn dispatch_op(ctx: &mut CommandContext, op: i64, args: &[Value]) -> Result<bool> {
    match op {
        o if o == ctx.ids.keylist_op_wait as i64 => {
            ctx.wait.wait_key();
            ctx.push(Value::Int(0));
            Ok(true)
        }
        o if o == ctx.ids.keylist_op_wait_force as i64 => {
            ctx.wait.clear();
            ctx.wait.wait_key();
            ctx.push(Value::Int(0));
            Ok(true)
        }
        o if o == ctx.ids.keylist_op_clear as i64 => {
            ctx.input.clear_keyboard();
            ctx.push(Value::Int(0));
            Ok(true)
        }
        o if o == ctx.ids.keylist_op_next as i64 => {
            ctx.input.next_keyboard_frame();
            ctx.push(Value::Int(0));
            Ok(true)
        }
        // Array-style key query:
        //   [-1, vk, key_op?]
        o if o == ctx.ids.elm_array as i64 => {
            let vk = arg_int(args, 1).unwrap_or(0);
            let key_op = arg_int(args, 2).unwrap_or(ctx.ids.key_op_is_down as i64);
            let v = key::query(ctx, vk, key_op);
            ctx.push(Value::Int(v));
            Ok(true)
        }
        _ => {
            let form_key = if ctx.ids.form_global_keylist != 0 {
                ctx.ids.form_global_keylist
            } else {
                KEYLIST_FORM_STATE
            };
            prop_access::store_or_push_direct_prop(ctx, form_key, op as i32, args, 1);
            Ok(true)
        }
    }
}
