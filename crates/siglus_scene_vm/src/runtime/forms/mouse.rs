use anyhow::Result;

use crate::runtime::{CommandContext, Value};

use super::{key, prop_access};

const MOUSE_FORM_STATE: u32 = 0xFF00_0002;

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
            ctx.input.clear_mouse();
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
            ctx.input.next_mouse_frame();
            ctx.push(Value::Int(0));
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_get_pos as i64 => {
            let x = ctx.input.mouse_x as i64;
            let y = ctx.input.mouse_y as i64;
            let mut assigned = 0usize;
            for v in args.iter() {
                if let Value::Element(chain) = v {
                    if assigned == 0 {
                        super::prop_access::assign_to_chain(ctx, chain, Value::Int(x));
                        assigned += 1;
                    } else if assigned == 1 {
                        super::prop_access::assign_to_chain(ctx, chain, Value::Int(y));
                        assigned += 1;
                        break;
                    }
                }
            }
            if assigned >= 2 {
                ctx.push(Value::Int(0));
            } else {
                ctx.push(Value::Int(x));
                ctx.push(Value::Int(y));
            }
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_set_pos as i64 => {
            // Conservative: allow scripts to reposition the stored cursor.
            // This does not warp the OS cursor; it only changes VM-visible state.
            let x = arg_int(args, 1).unwrap_or(ctx.input.mouse_x as i64) as i32;
            let y = arg_int(args, 2).unwrap_or(ctx.input.mouse_y as i64) as i32;
            ctx.input.on_mouse_move(x, y);
            ctx.push(Value::Int(0));
            Ok(true)
        }
        _ => {
            let form_key = if ctx.ids.form_global_mouse != 0 {
                ctx.ids.form_global_mouse
            } else {
                MOUSE_FORM_STATE
            };
            prop_access::store_or_push_direct_prop(ctx, form_key, op as i32, args, 1);
            Ok(true)
        }
    }
}
