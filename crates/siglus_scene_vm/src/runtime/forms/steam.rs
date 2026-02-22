use anyhow::Result;

use crate::runtime::forms::stub;
use crate::runtime::{CommandContext, OpCode, Value};

fn parse_element_chain<'a>(form_id: u32, args: &'a [Value]) -> Option<(usize, &'a [i32])> {
    for (i, v) in args.iter().enumerate() {
        if let Value::Element(ch) = v {
            if ch.first().copied() == Some(form_id as i32) {
                return Some((i, ch.as_slice()));
            }
        }
    }
    None
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let Some((chain_pos, chain)) = parse_element_chain(form_id, args) else {
        return stub::dispatch(ctx, form_id, args);
    };

    if chain.len() < 2 {
        return stub::dispatch(ctx, form_id, args);
    }

    let op = chain[1];
    let params = if chain_pos > 1 { &args[1..chain_pos] } else { &[] };
    let p_str = |i: usize| -> &str { params.get(i).and_then(|v| v.as_str()).unwrap_or("") };

    // STEAM is a platform integration layer. This port intentionally keeps it as a stub.
    if ctx.ids.steam_set_achievement != 0 && op == ctx.ids.steam_set_achievement {
        let _name = p_str(0);
        ctx.push(Value::Int(0));
        return Ok(true);
    }

    if ctx.ids.steam_reset_all_status != 0 && op == ctx.ids.steam_reset_all_status {
        ctx.push(Value::Int(0));
        return Ok(true);
    }

    ctx.unknown.record_code(OpCode::form(form_id));
    ctx.unknown.record_element_chain(form_id, chain, "STEAM");
    stub::dispatch(ctx, form_id, args)
}
