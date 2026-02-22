use crate::runtime::{CommandContext, Value};
use anyhow::Result;

fn ensure_len(v: &mut Vec<i64>, idx: usize) {
    if v.len() <= idx {
        v.resize(idx + 1, 0);
    }
}

/// Generic handler for global int-list forms (`tnm_command_proc_int_list`).
///
/// We implement the bring-up subset:
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

    // Property-assign call shape: [op_id, al_id, rhs, Element(chain)]
    if chain_pos == Some(3) {
        if let (Some(al_id), Some(rhs)) = (
            args.get(1).and_then(|v| v.as_i64()),
            args.get(2).and_then(|v| v.as_i64()),
        ) {
            if al_id == 1 {
                if let Some(c) = chain {
                    if c.len() >= 3 && c[1] == ctx.ids.elm_array {
                        let idx = c[2].max(0) as usize;
                        {
                            let list = ctx
                                .globals
                                .int_lists
                                .entry(form_id)
                                .or_insert_with(|| vec![0; 32]);
                            ensure_len(list, idx);
                            list[idx] = rhs;
                        }
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
            let idx = c[2].max(0) as usize;
            let out = {
                let list = ctx
                    .globals
                    .int_lists
                    .entry(form_id)
                    .or_insert_with(|| vec![0; 32]);
                ensure_len(list, idx);
                if al_id == 1 {
                    if let Some(rhs) = args.get(0).and_then(|v| v.as_i64()) {
                        list[idx] = rhs;
                    }
                    0
                } else {
                    list[idx]
                }
            };
            ctx.push(Value::Int(out));
            return Ok(true);
        }
    }

    ctx.push(Value::Int(0));
    Ok(true)
}
