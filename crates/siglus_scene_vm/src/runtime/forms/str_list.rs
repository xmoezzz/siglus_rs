use crate::runtime::{CommandContext, Value};
use anyhow::Result;

fn ensure_len(v: &mut Vec<String>, idx: usize) {
    if v.len() <= idx {
        v.resize_with(idx + 1, String::new);
    }
}

/// Generic handler for global string-list forms (`tnm_command_proc_str_list`).
///
/// Runtime subset:
/// - array indexing get: LIST[i]
/// - array indexing set: LIST[i] = "..."
pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
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
        if pos > 1 {
            &args[1..pos]
        } else {
            &[]
        }
    } else {
        &[][..]
    };

    // Property-assign call shape: [op_id, al_id, rhs, Element(chain)]
    if chain_pos == Some(3) {
        if let (Some(al_id), Some(rhs)) = (
            args.get(1).and_then(|v| v.as_i64()),
            args.get(2).and_then(|v| v.as_str().map(|s| s.to_string())),
        ) {
            if al_id == 1 {
                if let Some(c) = chain {
                    if c.len() >= 3 && c[1] == ctx.ids.elm_array {
                        let idx = c[2].max(0) as usize;
                        {
                            let list = ctx
                                .globals
                                .str_lists
                                .entry(form_id)
                                .or_insert_with(Vec::new);
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
        let ret_form = args.get(pos + 2).and_then(|v| v.as_i64()).unwrap_or(0);

        if c.len() >= 3 && c[1] == ctx.ids.elm_array {
            let idx = c[2].max(0) as usize;
            if al_id == 1 {
                if let Some(rhs) = args.get(0).and_then(|v| v.as_str()).map(|s| s.to_string()) {
                    {
                        let list = ctx
                            .globals
                            .str_lists
                            .entry(form_id)
                            .or_insert_with(Vec::new);
                        ensure_len(list, idx);
                        list[idx] = rhs;
                    }
                }
                ctx.push(Value::Int(0));
            } else {
                let s = {
                    let list = ctx
                        .globals
                        .str_lists
                        .entry(form_id)
                        .or_insert_with(Vec::new);
                    ensure_len(list, idx);
                    list[idx].clone()
                };
                if ret_form == 2 {
                    ctx.push(Value::Str(s));
                } else {
                    ctx.push(Value::Str(s));
                }
            }
            return Ok(true);
        }

        if c.len() == 2 {
            if ret_form != 0 && params.is_empty() {
                let size = {
                    let list = ctx
                        .globals
                        .str_lists
                        .entry(form_id)
                        .or_insert_with(Vec::new);
                    list.len() as i64
                };
                ctx.push(Value::Int(size));
                return Ok(true);
            }

            let list = ctx
                .globals
                .str_lists
                .entry(form_id)
                .or_insert_with(Vec::new);

            if ret_form == 0 {
                if params.is_empty() {
                    list.clear();
                    ctx.push(Value::Int(0));
                    return Ok(true);
                }
                if params.len() == 1 && params[0].as_i64().is_some() {
                    let n = params[0].as_i64().unwrap_or(0).max(0) as usize;
                    list.resize_with(n, String::new);
                    ctx.push(Value::Int(0));
                    return Ok(true);
                }
                if params.len() >= 2 {
                    let has_str = params.iter().any(|v| v.as_str().is_some());
                    if has_str {
                        let start = params[0].as_i64().unwrap_or(0).max(0) as usize;
                        let mut idx = start;
                        for v in params.iter().skip(1) {
                            let s = match v {
                                Value::Str(s) => s.clone(),
                                Value::Int(n) => n.to_string(),
                                _ => String::new(),
                            };
                            ensure_len(list, idx);
                            list[idx] = s;
                            idx += 1;
                        }
                        ctx.push(Value::Int(0));
                        return Ok(true);
                    }
                    let start = params[0].as_i64().unwrap_or(0);
                    let end = params[1].as_i64().unwrap_or(start);
                    let value = if params.len() >= 3 {
                        params[2].as_str().unwrap_or("").to_string()
                    } else {
                        String::new()
                    };
                    for i in start..=end {
                        let idx = i.max(0) as usize;
                        ensure_len(list, idx);
                        list[idx] = value.clone();
                    }
                    ctx.push(Value::Int(0));
                    return Ok(true);
                }
            }
        }
    }

    ctx.push(Value::Str(String::new()));
    Ok(true)
}
