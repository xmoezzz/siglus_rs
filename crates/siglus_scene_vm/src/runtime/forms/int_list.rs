use crate::runtime::{CommandContext, Value};
use anyhow::Result;

fn ensure_len(v: &mut Vec<i64>, idx: usize) {
    if v.len() <= idx {
        v.resize(idx + 1, 0);
    }
}

fn bit_unit(bit: i32) -> i32 {
    if bit <= 0 { 32 } else { bit }
}

fn get_bit_width(ctx: &mut CommandContext, form_id: u32, op: i32) -> i32 {
    if op == ctx.ids.elm_array {
        return 32;
    }
    if let Some(&w) = ctx.globals.intlist_bit_widths.get(&(form_id, op)) {
        return w;
    }
    let order = ctx.globals.intlist_bit_order.entry(form_id).or_default();
    if !order.contains(&op) {
        order.push(op);
    }
    let w = match order.iter().position(|&x| x == op).unwrap_or(0) {
        0 => 1,
        1 => 2,
        2 => 4,
        3 => 8,
        4 => 16,
        _ => 32,
    };
    ctx.globals.intlist_bit_widths.insert((form_id, op), w);
    w
}

fn bit_get(list: &mut Vec<i64>, bit_width: i32, index: i64) -> i64 {
    let bit_width = bit_unit(bit_width) as u32;
    if bit_width >= 32 {
        let idx = index.max(0) as usize;
        ensure_len(list, idx);
        return list[idx];
    }
    let per = 32 / bit_width;
    let idx = (index.max(0) as u32 / per) as usize;
    let shift = (index.max(0) as u32 % per) * bit_width;
    ensure_len(list, idx);
    let mask = (1u32 << bit_width) - 1;
    let raw = list[idx] as u32;
    ((raw >> shift) & mask) as i64
}

fn bit_set(list: &mut Vec<i64>, bit_width: i32, index: i64, value: i64) {
    let bit_width = bit_unit(bit_width) as u32;
    if bit_width >= 32 {
        let idx = index.max(0) as usize;
        ensure_len(list, idx);
        list[idx] = value;
        return;
    }
    let per = 32 / bit_width;
    let idx = (index.max(0) as u32 / per) as usize;
    let shift = (index.max(0) as u32 % per) * bit_width;
    ensure_len(list, idx);
    let mask = ((1u32 << bit_width) - 1) << shift;
    let raw = list[idx] as u32;
    let v = (value as u32) & ((1u32 << bit_width) - 1);
    let next = (raw & !mask) | (v << shift);
    list[idx] = next as i64;
}

/// Generic handler for global int-list forms (`tnm_command_proc_int_list`).
///
/// We implement the runtime subset:
/// - array indexing get: LIST[i]
/// - array indexing set: LIST[i] = value
///
/// More exotic sub-ops (bit ops, init, copy, etc.) can be added later.
pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    // Find element chain if present.
    let mut chain_pos: Option<usize> = None;
    let mut chain: Option<&[i32]> = None;
    for (i, v) in args.iter().enumerate() {
        if let Value::Element(c) = v {
            if !c.is_empty() && c[0] == form_id as i32 {
                chain_pos = Some(i);
                chain = Some(c);
                break;
            }
        }
    }

    let params = if let Some(pos) = chain_pos {
        if pos > 1 { &args[1..pos] } else { &[] }
    } else {
        &[][..]
    };

    // Property-assign call shape: [op_id, al_id, rhs, Element(chain)]
    if chain_pos == Some(3) {
        if let (Some(al_id), Some(rhs)) = (
            args.get(1).and_then(|v| v.as_i64()),
            args.get(2).and_then(|v| v.as_i64()),
        ) {
            if al_id == 1 {
                if let Some(c) = chain {
                    if c.len() >= 3 && c[1] == ctx.ids.elm_array {
                        let idx = c[2] as i64;
                        let list = ctx
                            .globals
                            .int_lists
                            .entry(form_id)
                            .or_insert_with(|| vec![0; 32]);
                        bit_set(list, 32, idx, rhs);
                    } else if c.len() >= 4 && c[2] == ctx.ids.elm_array {
                        let bit_width = get_bit_width(ctx, form_id, c[1]);
                        let idx = c[3] as i64;
                        let list = ctx
                            .globals
                            .int_lists
                            .entry(form_id)
                            .or_insert_with(|| vec![0; 32]);
                        bit_set(list, bit_width, idx, rhs);
                    }
                }
            }
            ctx.push(Value::Int(0));
            return Ok(true);
        }
    }

    // Command call shape: [rhs?, Element(chain), al_id, ret_form]
    if let (Some(pos), Some(c)) = (chain_pos, chain) {
        let al_id = args.get(pos + 1).and_then(|v| v.as_i64()).unwrap_or(0);
        let _ret_form = args.get(pos + 2).and_then(|v| v.as_i64()).unwrap_or(0);

        if c.len() >= 3 && c[1] == ctx.ids.elm_array {
            let idx = c[2] as i64;
            let out = {
                let list = ctx
                    .globals
                    .int_lists
                    .entry(form_id)
                    .or_insert_with(|| vec![0; 32]);
                if al_id == 1 {
                    if let Some(rhs) = args.get(0).and_then(|v| v.as_i64()) {
                        bit_set(list, 32, idx, rhs);
                    }
                    0
                } else {
                    bit_get(list, 32, idx)
                }
            };
            ctx.push(Value::Int(out));
            return Ok(true);
        }

        if c.len() >= 4 && c[2] == ctx.ids.elm_array {
            let bit_width = get_bit_width(ctx, form_id, c[1]);
            let idx = c[3] as i64;
            let out = {
                let list = ctx
                    .globals
                    .int_lists
                    .entry(form_id)
                    .or_insert_with(|| vec![0; 32]);
                if al_id == 1 {
                    if let Some(rhs) = args.get(0).and_then(|v| v.as_i64()) {
                        bit_set(list, bit_width, idx, rhs);
                    }
                    0
                } else {
                    bit_get(list, bit_width, idx)
                }
            };
            ctx.push(Value::Int(out));
            return Ok(true);
        }

        if c.len() == 3 && c[1] != ctx.ids.elm_array && c[2] != ctx.ids.elm_array {
            let bit_width = get_bit_width(ctx, form_id, c[1]);
            let list = ctx
                .globals
                .int_lists
                .entry(form_id)
                .or_insert_with(|| vec![0; 32]);

            if _ret_form != 0 && params.is_empty() {
                let unit = bit_unit(bit_width) as i64;
                let per = if unit >= 32 { 1 } else { 32 / unit };
                let size = list.len() as i64 * per as i64;
                ctx.push(Value::Int(size));
                return Ok(true);
            }

            if _ret_form == 0 {
                if params.is_empty() {
                    list.clear();
                    ctx.push(Value::Int(0));
                    return Ok(true);
                }
                if params.len() == 1 {
                    let n = params[0].as_i64().unwrap_or(0).max(0) as usize;
                    list.resize(n, 0);
                    ctx.push(Value::Int(0));
                    return Ok(true);
                }
                if params.len() >= 2 {
                    let start = params[0].as_i64().unwrap_or(0);
                    let end = params[1].as_i64().unwrap_or(start);
                    let value = if params.len() >= 3 { params[2].as_i64().unwrap_or(0) } else { 0 };
                    for i in start..=end {
                        bit_set(list, bit_width, i, value);
                    }
                    ctx.push(Value::Int(0));
                    return Ok(true);
                }
            }
        }

        if c.len() == 2 {
            if _ret_form != 0 && params.is_empty() {
                let size = {
                    let list = ctx
                        .globals
                        .int_lists
                        .entry(form_id)
                        .or_insert_with(|| vec![0; 32]);
                    list.len() as i64
                };
                ctx.push(Value::Int(size));
                return Ok(true);
            }

            let list = ctx
                .globals
                .int_lists
                .entry(form_id)
                .or_insert_with(|| vec![0; 32]);

            if _ret_form == 0 {
                if params.is_empty() {
                    list.clear();
                    ctx.push(Value::Int(0));
                    return Ok(true);
                }
                if params.len() == 1 {
                    let n = params[0].as_i64().unwrap_or(0).max(0) as usize;
                    list.resize(n, 0);
                    ctx.push(Value::Int(0));
                    return Ok(true);
                }
                if params.len() >= 2 {
                    let start = params[0].as_i64().unwrap_or(0);
                    let end = params[1].as_i64().unwrap_or(start);
                    let value = if params.len() >= 3 { params[2].as_i64().unwrap_or(0) } else { 0 };
                    for i in start..=end {
                        bit_set(list, 32, i, value);
                    }
                    ctx.push(Value::Int(0));
                    return Ok(true);
                }
            }
        }
    }

    ctx.push(Value::Int(0));
    Ok(true)
}
