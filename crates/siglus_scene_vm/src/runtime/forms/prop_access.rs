use crate::runtime::{CommandContext, Value};
use std::collections::HashMap;

pub fn parse_element_chain<'a>(form_id: u32, args: &'a [Value]) -> Option<(usize, &'a [i32])> {
    for (i, v) in args.iter().enumerate() {
        if let Value::Element(ch) = v {
            if ch.first().copied() == Some(form_id as i32) {
                return Some((i, ch.as_slice()));
            }
        }
    }
    None
}

pub fn infer_assign_and_ret(
    chain_pos: usize,
    args: &[Value],
) -> (Option<i64>, Option<i64>, Option<Value>) {
    if chain_pos + 2 < args.len() {
        let al_id = args.get(chain_pos + 1).and_then(|v| v.as_i64());
        let ret_form = args.get(chain_pos + 2).and_then(|v| v.as_i64());
        let rhs = if al_id == Some(1) && chain_pos > 0 {
            args.get(0).cloned()
        } else {
            None
        };
        return (al_id, ret_form, rhs);
    }

    if chain_pos >= 3 {
        let al_id = args.get(1).and_then(|v| v.as_i64());
        let rhs = if al_id == Some(1) {
            args.get(2).cloned()
        } else {
            None
        };
        return (al_id, None, rhs);
    }

    (None, None, None)
}

fn int_map<'a>(ctx: &'a mut CommandContext, form_id: u32) -> &'a mut HashMap<i32, i64> {
    ctx.globals
        .int_props
        .entry(form_id)
        .or_insert_with(HashMap::new)
}

fn str_map<'a>(ctx: &'a mut CommandContext, form_id: u32) -> &'a mut HashMap<i32, String> {
    ctx.globals
        .str_props
        .entry(form_id)
        .or_insert_with(HashMap::new)
}

fn int_list<'a>(ctx: &'a mut CommandContext, form_id: u32) -> &'a mut Vec<i64> {
    ctx.globals
        .int_lists
        .entry(form_id)
        .or_insert_with(|| vec![0; 32])
}

fn str_list<'a>(ctx: &'a mut CommandContext, form_id: u32) -> &'a mut Vec<String> {
    ctx.globals
        .str_lists
        .entry(form_id)
        .or_insert_with(Vec::new)
}

fn ensure_int_len(v: &mut Vec<i64>, idx: usize) {
    if v.len() <= idx {
        v.resize(idx + 1, 0);
    }
}

fn ensure_str_len(v: &mut Vec<String>, idx: usize) {
    if v.len() <= idx {
        v.resize_with(idx + 1, String::new);
    }
}

fn prefers_string(ret_form: Option<i64>, rhs: Option<&Value>) -> bool {
    matches!(ret_form, Some(2)) || matches!(rhs, Some(Value::Str(_)))
}

pub fn chain_key(parts: &[i32]) -> i32 {
    let mut h: u32 = 0x811C_9DC5;
    for &p in parts {
        h ^= p as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h as i32
}

pub fn push_stored_or_default(
    ctx: &mut CommandContext,
    form_id: u32,
    op: i32,
    ret_form: Option<i64>,
) {
    if matches!(ret_form, Some(2)) {
        let s = ctx
            .globals
            .str_props
            .get(&form_id)
            .and_then(|m| m.get(&op))
            .cloned()
            .unwrap_or_default();
        ctx.push(Value::Str(s));
        return;
    }

    if let Some(s) = ctx
        .globals
        .str_props
        .get(&form_id)
        .and_then(|m| m.get(&op))
        .cloned()
    {
        ctx.push(Value::Str(s));
        return;
    }

    let v = ctx
        .globals
        .int_props
        .get(&form_id)
        .and_then(|m| m.get(&op).copied())
        .unwrap_or(0);
    ctx.push(Value::Int(v));
}

pub fn store_or_push_prop(
    ctx: &mut CommandContext,
    form_id: u32,
    prop_key: i32,
    chain_pos: usize,
    args: &[Value],
) {
    let (al_id, ret_form, rhs) = infer_assign_and_ret(chain_pos, args);
    if al_id == Some(1) {
        if let Some(v) = rhs {
            match v {
                Value::Str(s) => {
                    str_map(ctx, form_id).insert(prop_key, s);
                }
                Value::Int(n) => {
                    int_map(ctx, form_id).insert(prop_key, n);
                }
                _ => {}
            }
        }
        ctx.push(Value::Int(0));
        return;
    }

    push_stored_or_default(ctx, form_id, prop_key, ret_form);
}

pub fn store_or_push_direct_prop(
    ctx: &mut CommandContext,
    form_id: u32,
    prop_key: i32,
    args: &[Value],
    value_idx: usize,
) {
    if let Some(v) = args.get(value_idx).cloned() {
        match v {
            Value::Str(s) => {
                str_map(ctx, form_id).insert(prop_key, s);
            }
            Value::Int(n) => {
                int_map(ctx, form_id).insert(prop_key, n);
            }
            _ => {}
        }
        ctx.push(Value::Int(0));
        return;
    }

    push_stored_or_default(ctx, form_id, prop_key, None);
}

pub fn store_or_push_indexed(
    ctx: &mut CommandContext,
    form_id: u32,
    index: usize,
    chain_pos: usize,
    args: &[Value],
) {
    let (al_id, ret_form, rhs) = infer_assign_and_ret(chain_pos, args);
    if al_id == Some(1) {
        match rhs {
            Some(Value::Str(s)) => {
                let list = str_list(ctx, form_id);
                ensure_str_len(list, index);
                list[index] = s;
            }
            Some(Value::Int(n)) => {
                let list = int_list(ctx, form_id);
                ensure_int_len(list, index);
                list[index] = n;
            }
            _ => {}
        }
        ctx.push(Value::Int(0));
        return;
    }

    if prefers_string(ret_form, rhs.as_ref()) {
        let value = {
            let list = str_list(ctx, form_id);
            ensure_str_len(list, index);
            list[index].clone()
        };
        ctx.push(Value::Str(value));
    } else {
        let value = {
            let list = int_list(ctx, form_id);
            ensure_int_len(list, index);
            list[index]
        };
        ctx.push(Value::Int(value));
    }
}

pub fn store_or_push_indexed_direct(
    ctx: &mut CommandContext,
    form_id: u32,
    index: usize,
    args: &[Value],
    value_idx: usize,
) {
    if let Some(v) = args.get(value_idx).cloned() {
        match v {
            Value::Str(s) => {
                let list = str_list(ctx, form_id);
                ensure_str_len(list, index);
                list[index] = s;
            }
            Value::Int(n) => {
                let list = int_list(ctx, form_id);
                ensure_int_len(list, index);
                list[index] = n;
            }
            _ => {}
        }
        ctx.push(Value::Int(0));
        return;
    }

    if let Some(s) = ctx
        .globals
        .str_lists
        .get(&form_id)
        .and_then(|v| v.get(index))
        .cloned()
    {
        ctx.push(Value::Str(s));
        return;
    }

    let v = ctx
        .globals
        .int_lists
        .get(&form_id)
        .and_then(|v| v.get(index).copied())
        .unwrap_or(0);
    ctx.push(Value::Int(v));
}

pub fn dispatch_stateful_form(ctx: &mut CommandContext, form_id: u32, args: &[Value]) {
    if let Some((chain_pos, chain)) = parse_element_chain(form_id, args) {
        if chain.len() >= 3 && chain[1] == ctx.ids.elm_array {
            let index = chain[2].max(0) as usize;
            if chain.len() == 3 {
                store_or_push_indexed(ctx, form_id, index, chain_pos, args);
            } else {
                let key = chain_key(&chain[1..]);
                store_or_push_prop(ctx, form_id, key, chain_pos, args);
            }
            return;
        }

        if chain.len() >= 2 {
            let key = if chain.len() == 2 {
                chain[1]
            } else {
                chain_key(&chain[1..])
            };
            store_or_push_prop(ctx, form_id, key, chain_pos, args);
            return;
        }
    }

    if let Some(op) = args.get(0).and_then(|v| v.as_i64()) {
        if op == ctx.ids.elm_array as i64 {
            let index = args.get(1).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
            store_or_push_indexed_direct(ctx, form_id, index, args, 2);
        } else {
            store_or_push_direct_prop(ctx, form_id, op as i32, args, 1);
        }
        return;
    }

    ctx.push(Value::Int(0));
}

pub fn dispatch_generic_form(ctx: &mut CommandContext, form_id: u32, args: &[Value]) {
    dispatch_stateful_form(ctx, form_id, args);
}

pub fn assign_to_chain(ctx: &mut CommandContext, chain: &[i32], value: Value) {
    if chain.is_empty() {
        return;
    }
    let form_id = chain[0].max(0) as u32;
    if chain.len() >= 3 && chain[1] == ctx.ids.elm_array {
        let index = chain[2].max(0) as usize;
        match value {
            Value::Str(s) => {
                let list = str_list(ctx, form_id);
                ensure_str_len(list, index);
                list[index] = s;
            }
            Value::Int(n) => {
                let list = int_list(ctx, form_id);
                ensure_int_len(list, index);
                list[index] = n;
            }
            _ => {}
        }
        return;
    }

    if chain.len() >= 2 {
        let key = if chain.len() == 2 {
            chain[1]
        } else {
            chain_key(&chain[1..])
        };
        match value {
            Value::Str(s) => {
                str_map(ctx, form_id).insert(key, s);
            }
            Value::Int(n) => {
                int_map(ctx, form_id).insert(key, n);
            }
            _ => {}
        }
    }
}
