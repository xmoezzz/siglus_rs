use anyhow::Result;

use crate::runtime::{CommandContext, Value};

use super::{key, prop_access};

const INPUT_FORM_STATE: u32 = 0xFF00_0001;

fn find_chain(args: &[Value]) -> Option<Vec<i32>> {
    for v in args.iter().rev() {
        if let Value::Element(e) = v {
            return Some(e.clone());
        }
    }
    None
}

fn parse_op(ctx: &CommandContext, args: &[Value]) -> Option<i64> {
    if let Some(Value::Int(v)) = args.get(0) {
        return Some(*v);
    }

    let chain = find_chain(args)?;
    if chain.len() >= 2 && chain[0] == ctx.ids.form_global_input as i32 {
        return Some(chain[1] as i64);
    }
    None
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let Some(op) = parse_op(ctx, args) else {
        return Ok(false);
    };

    match op {
        o if o == ctx.ids.input_op_decide as i64 => {
            let v = key::query(ctx, ctx.ids.exkey_decide as i64, ctx.ids.key_op_on_down as i64);
            ctx.push(Value::Int(v));
            Ok(true)
        }
        o if o == ctx.ids.input_op_cancel as i64 => {
            let v = key::query(ctx, ctx.ids.exkey_cancel as i64, ctx.ids.key_op_on_down as i64);
            ctx.push(Value::Int(v));
            Ok(true)
        }
        o if o == ctx.ids.input_op_clear as i64 => {
            ctx.input.clear_all();
            ctx.push(Value::Int(0));
            Ok(true)
        }
        o if o == ctx.ids.input_op_next as i64 => {
            ctx.input.next_frame();
            ctx.push(Value::Int(0));
            Ok(true)
        }
        _ => {
            let form_key = if ctx.ids.form_global_input != 0 { ctx.ids.form_global_input } else { INPUT_FORM_STATE };
            prop_access::store_or_push_direct_prop(ctx, form_key, op as i32, args, 1);
            Ok(true)
        }
    }
}
