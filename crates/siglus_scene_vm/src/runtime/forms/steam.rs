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

        if ctx.ids.steam_set_achievement != 0 && op == ctx.ids.steam_set_achievement {
            let name = p_str(0);
            if !name.is_empty() {
                ctx.globals
                    .str_props
                    .entry(form_id)
                    .or_default()
                    .insert(op, name.to_string());
                ctx.globals
                    .int_props
                    .entry(form_id)
                    .or_default()
                    .insert(op, 1);
            }
            ctx.push(Value::Int(0));
            return Ok(true);
        }

        if ctx.ids.steam_reset_all_status != 0 && op == ctx.ids.steam_reset_all_status {
            ctx.globals.int_props.remove(&form_id);
            ctx.globals.str_props.remove(&form_id);
            ctx.push(Value::Int(0));
            return Ok(true);
        }

        prop_access::store_or_push_prop(ctx, form_id, op, chain_pos.unwrap(), args);
        return Ok(true);
    }

    prop_access::dispatch_stateful_form(ctx, form_id, args);
    Ok(true)
}
