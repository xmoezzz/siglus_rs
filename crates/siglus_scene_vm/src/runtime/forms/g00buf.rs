use anyhow::Result;

use crate::runtime::forms::prop_access;
use crate::runtime::{CommandContext, Value};

const DEFAULT_G00BUF_CNT: usize = 64;

fn configured_g00buf_cnt(ctx: &CommandContext) -> usize {
    ctx.tables
        .gameexe
        .as_ref()
        .map(|cfg| cfg.indexed_count("G00BUF"))
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_G00BUF_CNT)
}

fn ensure_buf_size(ctx: &mut CommandContext, idx: usize) {
    let want = (idx + 1).max(configured_g00buf_cnt(ctx));
    if ctx.globals.g00buf.len() < want {
        ctx.globals.g00buf.resize(want, None);
    }
    if ctx.globals.g00buf_names.len() < want {
        ctx.globals.g00buf_names.resize(want, None);
    }
}

fn composite_slot_op_key(idx: i32, op: i32) -> i32 {
    ((idx & 0x7fff) << 16) ^ (op & 0xffff)
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let parsed = prop_access::parse_element_chain_ctx(ctx, form_id, args);
    let (chain_pos, chain) = match parsed {
        Some((pos, ch)) if ch.len() >= 2 => (Some(pos), Some(ch.to_vec())),
        _ => (None, None),
    };

    if let Some(chain) = chain.as_ref() {
        let op = chain[1];
        let params = if let Some(pos) = chain_pos {
            prop_access::script_args(args, pos)
        } else {
            &[]
        };
        let p_str = |i: usize| -> &str { params.get(i).and_then(|v| v.as_str()).unwrap_or("") };

        if ctx.ids.g00buf_list_get_size != 0 && op == ctx.ids.g00buf_list_get_size {
            let size = ctx.globals.g00buf.len().max(configured_g00buf_cnt(ctx));
            ctx.push(Value::Int(size as i64));
            return Ok(true);
        }

        if ctx.ids.g00buf_list_free_all != 0 && op == ctx.ids.g00buf_list_free_all {
            for slot in &mut ctx.globals.g00buf {
                *slot = None;
            }
            for name in &mut ctx.globals.g00buf_names {
                *name = None;
            }
            ctx.push(Value::Int(0));
            return Ok(true);
        }

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

            if ctx.ids.g00buf_load != 0 && buf_op == ctx.ids.g00buf_load {
                let name = p_str(0);
                match ctx.images.load_g00(name, 0) {
                    Ok(img_id) => {
                        ctx.globals.g00buf[idx] = Some(img_id);
                        ctx.globals.g00buf_names[idx] = Some(name.to_string());
                        ctx.push(Value::Int(0));
                    }
                    Err(_) => {
                        ctx.globals.g00buf[idx] = None;
                        ctx.globals.g00buf_names[idx] = None;
                        ctx.push(Value::Int(0));
                    }
                }
                return Ok(true);
            }

            if ctx.ids.g00buf_free != 0 && buf_op == ctx.ids.g00buf_free {
                ctx.globals.g00buf[idx] = None;
                ctx.globals.g00buf_names[idx] = None;
                ctx.push(Value::Int(0));
                return Ok(true);
            }

            let prop_key = composite_slot_op_key(idx_i32, buf_op);
            prop_access::store_or_push_prop(ctx, form_id, prop_key, chain_pos.unwrap(), args);
            return Ok(true);
        }

        prop_access::store_or_push_prop(ctx, form_id, op, chain_pos.unwrap(), args);
        return Ok(true);
    }

    Ok(false)
}
