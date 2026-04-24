use anyhow::Result;

use crate::runtime::{CommandContext, Value};

use super::key;

fn current_chain(ctx: &CommandContext) -> Option<&[i32]> {
    let vm_call = ctx.vm_call.as_ref()?;
    Some(vm_call.element.as_slice())
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, _args: &[Value]) -> Result<bool> {
    let Some(chain) = current_chain(ctx) else {
        return Ok(false);
    };
    if chain.len() < 2 || chain[0] != form_id as i32 {
        return Ok(false);
    }

    let op = chain[1] as i64;
    match op {
        o if o == ctx.ids.input_op_clear as i64 => {
            ctx.script_input.clear_all();
            Ok(true)
        }
        o if o == ctx.ids.input_op_next as i64 => {
            ctx.script_input.next_frame();
            Ok(true)
        }
        o if o == ctx.ids.input_op_decide as i64 => {
            if chain.len() == 2 {
                ctx.push(Value::Element(chain.to_vec()));
            } else {
                let subop = chain[2] as i64;
                let v = key::query(ctx, ctx.ids.exkey_decide as i64, subop);
                ctx.push(Value::Int(v));
            }
            Ok(true)
        }
        o if o == ctx.ids.input_op_cancel as i64 => {
            if chain.len() == 2 {
                ctx.push(Value::Element(chain.to_vec()));
            } else {
                let subop = chain[2] as i64;
                let v = key::query(ctx, ctx.ids.exkey_cancel as i64, subop);
                ctx.push(Value::Int(v));
            }
            Ok(true)
        }
        _ => Ok(false),
    }
}
