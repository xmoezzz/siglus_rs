use anyhow::Result;

use crate::runtime::{CommandContext, Value};

const VK_LBUTTON: u8 = 0x01;
const VK_RBUTTON: u8 = 0x02;
const VK_RETURN: u8 = 0x0D;
const VK_ESCAPE: u8 = 0x1B;
const VK_Z: u8 = 0x5A;
const VK_X: u8 = 0x58;

pub fn query(ctx: &mut CommandContext, vk_code: i64, op: i64) -> i64 {
    if vk_code == ctx.ids.exkey_decide as i64 {
        return query_ex(ctx, true, op);
    }
    if vk_code == ctx.ids.exkey_cancel as i64 {
        return query_ex(ctx, false, op);
    }
    if !(0..=255).contains(&vk_code) {
        return 0;
    }
    query_vk(ctx, vk_code as u8, op)
}

fn query_ex(ctx: &mut CommandContext, decide: bool, op: i64) -> i64 {
    let keys: &[u8] = if decide {
        &[VK_LBUTTON, VK_RETURN, VK_X]
    } else {
        &[VK_RBUTTON, VK_ESCAPE, VK_Z]
    };
    match op {
        o if o == ctx.ids.key_op_on_down as i64 => {
            bool_i64(keys.iter().any(|&k| ctx.input.vk_down_stock(k)))
        }
        o if o == ctx.ids.key_op_on_up as i64 => {
            bool_i64(keys.iter().any(|&k| ctx.input.vk_up_stock(k)))
        }
        o if o == ctx.ids.key_op_on_down_up as i64 => {
            bool_i64(keys.iter().any(|&k| ctx.input.vk_down_up_stock(k)))
        }
        o if o == ctx.ids.key_op_is_down as i64 => {
            bool_i64(keys.iter().any(|&k| ctx.input.vk_is_down(k)))
        }
        o if o == ctx.ids.key_op_is_up as i64 => {
            bool_i64(keys.iter().all(|&k| !ctx.input.vk_is_down(k)))
        }
        _ => 0,
    }
}

fn query_vk(ctx: &mut CommandContext, vk: u8, op: i64) -> i64 {
    match op {
        o if o == ctx.ids.key_op_dir as i64 => ctx.input.dir_mask(),
        o if o == ctx.ids.key_op_on_down as i64 => bool_i64(ctx.input.vk_down_stock(vk)),
        o if o == ctx.ids.key_op_on_up as i64 => bool_i64(ctx.input.vk_up_stock(vk)),
        o if o == ctx.ids.key_op_on_down_up as i64 => bool_i64(ctx.input.vk_down_up_stock(vk)),
        o if o == ctx.ids.key_op_is_down as i64 => bool_i64(ctx.input.vk_is_down(vk)),
        o if o == ctx.ids.key_op_is_up as i64 => bool_i64(!ctx.input.vk_is_down(vk)),
        o if o == ctx.ids.key_op_on_flick as i64 => bool_i64(ctx.input.vk_flick_stock(vk)),
        o if o == ctx.ids.key_op_flick as i64 => ctx.input.vk_flick_pixel(vk).trunc() as i64,
        o if o == ctx.ids.key_op_flick_angle as i64 => {
            let angle = ctx.input.vk_flick_angle(vk) as f64;
            let deg = 180.0 - angle.to_degrees();
            (deg * 10.0).trunc() as i64
        }
        _ => 0,
    }
}

fn bool_i64(b: bool) -> i64 {
    if b {
        1
    } else {
        0
    }
}

pub fn dispatch(ctx: &mut CommandContext, _args: &[Value]) -> Result<bool> {
    let vm_call = match ctx.vm_call.as_ref() {
        Some(v) => v,
        None => return Ok(false),
    };
    let chain = &vm_call.element;
    if chain.len() < 3 || chain[0] != ctx.ids.form_global_key as i32 {
        return Ok(false);
    }

    let (vk, op) = if chain[1] == ctx.ids.elm_array {
        if chain.len() < 4 {
            return Ok(false);
        }
        (chain[2] as i64, chain[3] as i64)
    } else {
        (chain[1] as i64, chain[2] as i64)
    };

    let v = query(ctx, vk, op);
    ctx.push(Value::Int(v));
    Ok(true)
}
