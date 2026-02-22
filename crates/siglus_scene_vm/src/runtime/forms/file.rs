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

    // FILE.PRELOAD_OMV(str)
    if ctx.ids.file_preload_omv != 0 && op == ctx.ids.file_preload_omv {
        let _name = p_str(0);
        // The original engine preloads OMV resources into an internal cache.
        // This port currently decodes movies lazily; keep VM stable by making this a no-op.
        ctx.push(Value::Int(0));
        return Ok(true);
    }

    ctx.unknown.record_code(OpCode::form(form_id));
    ctx.unknown.record_element_chain(form_id, chain, "FILE");
    stub::dispatch(ctx, form_id, args)
}
