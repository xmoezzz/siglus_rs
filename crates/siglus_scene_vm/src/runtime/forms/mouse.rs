use anyhow::Result;

use crate::runtime::{CommandContext, Value};

use super::key;

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
    if chain.len() >= 2 && chain[0] == ctx.ids.form_global_mouse as i32 {
        return Some(chain[1] as i64);
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
    let Some(op) = parse_op(ctx, args) else {
        return Ok(false);
    };

    match op {
        o if o == ctx.ids.mouse_op_x as i64 => {
            ctx.push(Value::Int(ctx.input.mouse_x as i64));
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_y as i64 => {
            ctx.push(Value::Int(ctx.input.mouse_y as i64));
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_clear as i64 => {
            ctx.input.clear_all();
            ctx.push(Value::Int(0));
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_wheel as i64 => {
            let d = ctx.input.take_wheel_delta();
            ctx.push(Value::Int(d as i64));
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_left as i64 => {
            let v = key::query(ctx, 0x01, ctx.ids.key_op_is_down as i64);
            ctx.push(Value::Int(v));
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_right as i64 => {
            let v = key::query(ctx, 0x02, ctx.ids.key_op_is_down as i64);
            ctx.push(Value::Int(v));
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_next as i64 => {
            ctx.input.next_frame();
            ctx.push(Value::Int(0));
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_get_pos as i64 => {
            ctx.push(Value::Int(ctx.input.mouse_x as i64));
            ctx.push(Value::Int(ctx.input.mouse_y as i64));
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_set_pos as i64 => {
            // Best-effort: allow scripts to reposition the stored cursor.
            // This does not warp the OS cursor; it only changes VM-visible state.
            let x = arg_int(args, 1).unwrap_or(ctx.input.mouse_x as i64) as i32;
            let y = arg_int(args, 2).unwrap_or(ctx.input.mouse_y as i64) as i32;
            ctx.input.on_mouse_move(x, y);
            ctx.push(Value::Int(0));
            Ok(true)
        }
        _ => {
            ctx.unknown
                .record_unimplemented(&format!("MOUSE/op={op}"));
            ctx.push(Value::Int(0));
            Ok(true)
        }
    }
}
