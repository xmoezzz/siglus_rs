use anyhow::Result;

use crate::runtime::forms::stub;
use crate::runtime::{CommandContext, OpCode, Value};

const DEFAULT_G00BUF_CNT: usize = 64;

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

fn ensure_buf_size(ctx: &mut CommandContext, idx: usize) {
    let want = (idx + 1).max(DEFAULT_G00BUF_CNT);
    if ctx.globals.g00buf.len() < want {
        ctx.globals.g00buf.resize(want, None);
    }
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

    // G00BUFLIST.GET_SIZE
    if ctx.ids.g00buf_list_get_size != 0 && op == ctx.ids.g00buf_list_get_size {
        let size = ctx.globals.g00buf.len().max(DEFAULT_G00BUF_CNT);
        ctx.push(Value::Int(size as i64));
        return Ok(true);
    }

    // G00BUFLIST.FREE_ALL
    if ctx.ids.g00buf_list_free_all != 0 && op == ctx.ids.g00buf_list_free_all {
        for slot in &mut ctx.globals.g00buf {
            *slot = None;
        }
        ctx.push(Value::Int(0));
        return Ok(true);
    }

    // G00BUFLIST[idx].<op>
    if op == ctx.ids.elm_array {
        if chain.len() < 4 {
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        let idx_i32 = chain[2];
        if idx_i32 < 0 {
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        let idx = idx_i32 as usize;
        ensure_buf_size(ctx, idx);
        let buf_op = chain[3];

        // LOAD(str)
        if ctx.ids.g00buf_load != 0 && buf_op == ctx.ids.g00buf_load {
            let name = p_str(0);
            match ctx.images.load_g00(name, 0) {
                Ok(img_id) => {
                    ctx.globals.g00buf[idx] = Some(img_id);
                    ctx.push(Value::Int(0));
                }
                Err(_) => {
                    ctx.globals.g00buf[idx] = None;
                    ctx.push(Value::Int(0));
                }
            }
            return Ok(true);
        }

        // FREE
        if ctx.ids.g00buf_free != 0 && buf_op == ctx.ids.g00buf_free {
            ctx.globals.g00buf[idx] = None;
            ctx.push(Value::Int(0));
            return Ok(true);
        }

        // Unknown per-buffer op.
        ctx.unknown.record_code(OpCode::form(form_id));
        ctx.unknown.record_element_chain(form_id, chain, "G00BUF");
        ctx.push(Value::Int(0));
        return Ok(true);
    }

    // Unknown op.
    ctx.unknown.record_code(OpCode::form(form_id));
    ctx.unknown.record_element_chain(form_id, chain, "G00BUF");
    stub::dispatch(ctx, form_id, args)
}
