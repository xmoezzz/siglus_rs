//! Key helper logic used by INPUT/MOUSE/KEYLIST.
//!
//! The original Siglus engine exposes per-key state via a small set of query
//! operations (down/up, edge "stocks", flick, direction, etc.). Other forms
//! (INPUT/MOUSE/KEYLIST) forward to these helpers.
//!
//! For runtime we implement a pragmatic subset:
//! - "is down" / "is up"
//! - "on down" / "on up" / "on down+up"
//! - "dir" as an arrow-key bitmask
//!
//! Flick-style operations are currently stubbed to zero.

use anyhow::Result;

use crate::runtime::{CommandContext, Value};

// Windows virtual-key constants used by many titles.
const VK_LBUTTON: u8 = 0x01;
const VK_RBUTTON: u8 = 0x02;
const VK_MBUTTON: u8 = 0x04;
const VK_RETURN: u8 = 0x0D;
const VK_ESCAPE: u8 = 0x1B;
const VK_Z: u8 = 0x5A;
const VK_X: u8 = 0x58;

/// Query key state.
///
/// `vk_code` is either a 0..=255 virtual-key, or one of the "EX" keys
/// (`exkey_decide` / `exkey_cancel`) which are treated as composite inputs.
///
/// Returns an integer result (0/1 for booleans).
pub fn query(ctx: &mut CommandContext, vk_code: i64, op: i64) -> i64 {
    // Composite EX keys (these are not real VK codes).
    if vk_code == ctx.ids.exkey_decide as i64 {
        return query_ex(ctx, ExKey::Decide, op);
    }
    if vk_code == ctx.ids.exkey_cancel as i64 {
        return query_ex(ctx, ExKey::Cancel, op);
    }

    if !(0..=255).contains(&vk_code) {
        return 0;
    }
    let vk = vk_code as u8;
    query_vk(ctx, vk, op)
}

#[derive(Debug, Clone, Copy)]
enum ExKey {
    Decide,
    Cancel,
}

fn query_ex(ctx: &mut CommandContext, which: ExKey, op: i64) -> i64 {
    // Conservative defaults derived from RE:
    // - EX_DECIDE: mouse left OR Enter OR X
    // - EX_CANCEL: mouse right OR Escape OR Z
    let keys: &[u8] = match which {
        ExKey::Decide => &[VK_LBUTTON, VK_RETURN, VK_X],
        ExKey::Cancel => &[VK_RBUTTON, VK_ESCAPE, VK_Z],
    };

    match op {
        o if o == ctx.ids.key_op_dir as i64 => ctx.input.dir_mask(),
        o if o == ctx.ids.key_op_on_down as i64 => bool_i64(keys.iter().any(|&k| ctx.input.vk_down_stock(k))),
        o if o == ctx.ids.key_op_on_up as i64 => bool_i64(keys.iter().any(|&k| ctx.input.vk_up_stock(k))),
        o if o == ctx.ids.key_op_on_down_up as i64 => bool_i64(keys.iter().any(|&k| ctx.input.vk_down_up_stock(k))),
        o if o == ctx.ids.key_op_is_down as i64 => bool_i64(keys.iter().any(|&k| ctx.input.vk_is_down(k))),
        o if o == ctx.ids.key_op_is_up as i64 => bool_i64(!keys.iter().any(|&k| ctx.input.vk_is_down(k))),
        o if o == ctx.ids.key_op_on_flick as i64 => 0,
        o if o == ctx.ids.key_op_flick as i64 => 0,
        o if o == ctx.ids.key_op_flick_angle as i64 => 0,
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
    if b { 1 } else { 0 }
}

fn is_key_op(ctx: &CommandContext, v: i64) -> bool {
    v == ctx.ids.key_op_dir as i64
        || v == ctx.ids.key_op_on_down as i64
        || v == ctx.ids.key_op_on_up as i64
        || v == ctx.ids.key_op_on_down_up as i64
        || v == ctx.ids.key_op_is_down as i64
        || v == ctx.ids.key_op_is_up as i64
        || v == ctx.ids.key_op_on_flick as i64
        || v == ctx.ids.key_op_flick as i64
        || v == ctx.ids.key_op_flick_angle as i64
}

/// Global form dispatcher for KEY.
///
/// The original engine routes KEY queries through a dedicated form. Titles may
/// call it in multiple shapes depending on whether the call site is a direct
/// function call or an element-chain evaluation. For runtime, we accept both:
/// - KEY(op, vk)
/// - KEY(vk, op)
///
/// and fall back to 0 when decoding fails.
pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    // Collect integer arguments (ignore element chains).
    let mut ints: Vec<i64> = Vec::new();
    for v in args.iter() {
        if let Some(i) = v.as_i64() {
            ints.push(i);
        }
    }

    // Try to decode from the direct integer arguments.
    let (mut op, mut vk) = if ints.len() >= 2 {
        let a = ints[0];
        let b = ints[1];
        if is_key_op(ctx, a) {
            (a, b)
        } else if is_key_op(ctx, b) {
            (b, a)
        } else {
            // Heuristic: most call sites pass op first.
            (a, b)
        }
    } else {
        (0, 0)
    };

    // If not enough info, try to decode from the element chain.
    if op == 0 && vk == 0 {
        for v in args.iter().rev() {
            if let Value::Element(chain) = v {
                if chain.len() >= 3 && chain[0] == ctx.ids.form_global_key as i32 {
                    let a = chain[1] as i64;
                    let b = chain[2] as i64;
                    if is_key_op(ctx, a) {
                        op = a;
                        vk = b;
                    } else if is_key_op(ctx, b) {
                        op = b;
                        vk = a;
                    } else {
                        op = a;
                        vk = b;
                    }
                    break;
                }
            }
        }
    }

    if op == 0 && vk == 0 {
        ctx.push(Value::Int(0));
        return Ok(true);
    }

    let v = query(ctx, vk, op);
    ctx.push(Value::Int(v));
    Ok(true)
}
