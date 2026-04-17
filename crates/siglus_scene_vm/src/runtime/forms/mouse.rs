use anyhow::Result;

use crate::runtime::{CommandContext, Value};

use super::key;

fn current_chain(ctx: &CommandContext) -> Option<&[i32]> {
    let vm_call = ctx.vm_call.as_ref()?;
    Some(vm_call.element.as_slice())
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let Some(chain) = current_chain(ctx) else {
        return Ok(false);
    };
    if chain.len() < 2 || chain[0] != ctx.ids.form_global_mouse as i32 {
        return Ok(false);
    }

    let op = chain[1] as i64;
    match op {
        o if o == ctx.ids.mouse_op_clear as i64 => {
            ctx.input.clear_mouse();
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_next as i64 => {
            ctx.input.next_mouse_frame();
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_x as i64 => {
            ctx.push(Value::Int(ctx.input.mouse_x as i64));
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_y as i64 => {
            ctx.push(Value::Int(ctx.input.mouse_y as i64));
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
                        break;
                    }
                }
            }
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_set_pos as i64 => {
            let x = args
                .get(0)
                .and_then(|v| v.as_i64())
                .unwrap_or(ctx.input.mouse_x as i64) as i32;
            let y = args
                .get(1)
                .and_then(|v| v.as_i64())
                .unwrap_or(ctx.input.mouse_y as i64) as i32;
            ctx.input.on_mouse_move(x, y);
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_wheel as i64 => {
            let d = ctx.input.take_wheel_delta();
            ctx.push(Value::Int(d as i64));
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_left as i64 => {
            let subop = chain
                .get(2)
                .copied()
                .map(|v| v as i64)
                .unwrap_or(ctx.ids.key_op_is_down as i64);
            let v = key::query(ctx, 0x01, subop);
            ctx.push(Value::Int(v));
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_right as i64 => {
            let subop = chain
                .get(2)
                .copied()
                .map(|v| v as i64)
                .unwrap_or(ctx.ids.key_op_is_down as i64);
            let v = key::query(ctx, 0x02, subop);
            ctx.push(Value::Int(v));
            Ok(true)
        }
        _ => Ok(false),
    }
}
