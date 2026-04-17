use anyhow::Result;

use crate::runtime::{CommandContext, Value};

use super::key;

fn current_chain(ctx: &CommandContext) -> Option<&[i32]> {
    let vm_call = ctx.vm_call.as_ref()?;
    Some(vm_call.element.as_slice())
}

pub fn dispatch(ctx: &mut CommandContext, _args: &[Value]) -> Result<bool> {
    let Some(chain) = current_chain(ctx) else {
        return Ok(false);
    };
    if chain.len() < 2 || chain[0] != ctx.ids.form_global_keylist as i32 {
        return Ok(false);
    }
    let op = chain[1] as i64;
    match op {
        o if o == ctx.ids.elm_array as i64 => {
            if chain.len() < 4 {
                return Ok(false);
            }
            let vk = chain[2] as i64;
            let key_op = chain[3] as i64;
            let v = key::query(ctx, vk, key_op);
            ctx.push(Value::Int(v));
            Ok(true)
        }
        o if o == ctx.ids.keylist_op_wait as i64 => {
            ctx.wait.wait_key();
            Ok(true)
        }
        o if o == ctx.ids.keylist_op_wait_force as i64 => {
            ctx.wait.clear();
            ctx.wait.wait_key();
            Ok(true)
        }
        o if o == ctx.ids.keylist_op_clear as i64 => {
            ctx.input.clear_keyboard();
            Ok(true)
        }
        o if o == ctx.ids.keylist_op_next as i64 => {
            ctx.input.next_keyboard_frame();
            Ok(true)
        }
        _ => Ok(false),
    }
}
