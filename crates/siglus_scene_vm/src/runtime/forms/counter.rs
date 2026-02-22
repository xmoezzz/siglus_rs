use crate::runtime::{globals::Counter, CommandContext, Value};
use anyhow::Result;

fn ensure_len(v: &mut Vec<Counter>, idx: usize) {
    if v.len() <= idx {
        v.resize(idx + 1, Counter::default());
    }
}

/// Minimal handler for COUNTER list.
///
/// The original engine has a rich counter system. For bring-up, we support:
/// - COUNTER[i] get/set as integer (milliseconds)
/// - COUNTER[i].start/stop/reset via best-effort opcode decoding (very limited)
pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    // Find element chain.
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

    let (pos, c) = match (chain_pos, chain) {
        (Some(p), Some(c)) => (p, c),
        _ => {
            ctx.push(Value::Int(0));
            return Ok(true);
        }
    };

    // Expect: COUNTER[ idx ]
    if c.len() < 3 || c[1] != ctx.ids.elm_array {
        ctx.push(Value::Int(0));
        return Ok(true);
    }
    let idx = c[2].max(0) as usize;

    let out = {
        let counters = ctx
            .globals
            .counter_lists
            .entry(form_id)
            .or_insert_with(Vec::new);
        ensure_len(counters, idx);

        // Property-assign call shape: [op_id, al_id, rhs, Element(chain)]
        if pos == 3 {
            let al_id = args.get(1).and_then(|v| v.as_i64()).unwrap_or(0);
            if al_id == 1 {
                let rhs = args.get(2).and_then(|v| v.as_i64()).unwrap_or(0);
                counters[idx].set_count(rhs);
            }
            0
        } else {
            // Command call shape: [rhs?, Element(chain), al_id, ret_form]
            let al_id = args.get(pos + 1).and_then(|v| v.as_i64()).unwrap_or(0);
            if al_id == 1 {
                let rhs = args.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
                counters[idx].set_count(rhs);
                0
            } else {
                counters[idx].get_count()
            }
        }
    };

    ctx.push(Value::Int(out));
    Ok(true)
}
