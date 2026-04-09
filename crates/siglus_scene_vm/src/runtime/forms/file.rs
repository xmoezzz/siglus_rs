use anyhow::Result;

use crate::runtime::forms::prop_access;
use crate::runtime::{CommandContext, Value};

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let parsed = prop_access::parse_element_chain(form_id, args);
    let (chain_pos, chain) = match parsed {
        Some((pos, ch)) if ch.len() >= 2 => (Some(pos), Some(ch)),
        _ => (None, None),
    };

    if let Some(chain) = chain {
        let op = chain[1];
        let params = if chain_pos.unwrap_or(0) > 1 {
            &args[1..chain_pos.unwrap()]
        } else {
            &[]
        };
        let p_str = |i: usize| -> &str { params.get(i).and_then(|v| v.as_str()).unwrap_or("") };

        if ctx.ids.file_preload_omv != 0 && op == ctx.ids.file_preload_omv {
            let name = p_str(0);
            if !name.is_empty() {
                let _ = ctx.movie.prepare(name);
            }
            ctx.push(Value::Int(0));
            return Ok(true);
        }

        prop_access::dispatch_stateful_form(ctx, form_id, args);
        return Ok(true);
    }

    if let Some(op) = args.get(0).and_then(|v| v.as_i64()) {
        if ctx.ids.file_preload_omv != 0 && op == ctx.ids.file_preload_omv as i64 {
            let name = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
            if !name.is_empty() {
                let _ = ctx.movie.prepare(name);
            }
            ctx.push(Value::Int(0));
            return Ok(true);
        }
    }

    prop_access::dispatch_stateful_form(ctx, form_id, args);
    Ok(true)
}
