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
    if chain.len() < 2 {
        return Ok(false);
    }

    // testcase startup still uses compact global alias 46 for mouse commands.
    // We only need the sub-op here because the caller has already routed the
    // command into the mouse form family.
    let op = chain[1] as i64;
    match op {
        o if o == ctx.ids.mouse_op_clear as i64 => {
            ctx.script_input.use_mouse_stocks();
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_next as i64 => {
            ctx.script_input.next_mouse_frame();
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_x as i64 => {
            ctx.push(Value::Int(ctx.script_input.mouse_x as i64));
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_y as i64 => {
            ctx.push(Value::Int(ctx.script_input.mouse_y as i64));
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_get_pos as i64 => {
            let x = ctx.script_input.mouse_x as i64;
            let y = ctx.script_input.mouse_y as i64;
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
                .unwrap_or(ctx.script_input.mouse_x as i64) as i32;
            let y = args
                .get(1)
                .and_then(|v| v.as_i64())
                .unwrap_or(ctx.script_input.mouse_y as i64) as i32;
            ctx.input.on_mouse_move(x, y);
            ctx.script_input.on_mouse_move(x, y);
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_wheel as i64 => {
            let d = ctx.script_input.take_wheel_delta();
            ctx.push(Value::Int(d as i64));
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_left as i64 => {
            if chain.len() == 2 {
                ctx.push(Value::Element(chain.to_vec()));
            } else {
                let subop = chain[2] as i64;
                let v = key::query(ctx, 0x01, subop);
                ctx.push(Value::Int(v));
            }
            Ok(true)
        }
        o if o == ctx.ids.mouse_op_right as i64 => {
            if chain.len() == 2 {
                ctx.push(Value::Element(chain.to_vec()));
            } else {
                let subop = chain[2] as i64;
                let v = key::query(ctx, 0x02, subop);
                ctx.push(Value::Int(v));
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}
