use anyhow::Result;

use crate::runtime::{CommandContext, Value};

use super::codes::{str_list_op, str_op};
use super::prop_access;

fn ensure_len(v: &mut Vec<String>, idx: usize) {
    if v.len() <= idx {
        v.resize_with(idx + 1, String::new);
    }
}

fn display_width_char(ch: char) -> usize {
    if ch.is_ascii() {
        1
    } else {
        2
    }
}

fn display_width_str(s: &str) -> usize {
    s.chars().map(display_width_char).sum()
}

fn left_by_display_width(s: &str, limit: usize) -> String {
    let mut width = 0usize;
    let mut out = String::new();
    for ch in s.chars() {
        let w = display_width_char(ch);
        if width + w > limit {
            break;
        }
        width += w;
        out.push(ch);
    }
    out
}

fn right_by_display_width(s: &str, limit: usize) -> String {
    let mut width = 0usize;
    let mut out: Vec<char> = Vec::new();
    for ch in s.chars().rev() {
        let w = display_width_char(ch);
        if width + w > limit {
            break;
        }
        width += w;
        out.push(ch);
    }
    out.into_iter().rev().collect()
}

fn mid_by_display_width(s: &str, start_width: usize, len_width: Option<usize>) -> String {
    let mut width = 0usize;
    let mut out = String::new();
    for ch in s.chars() {
        let ch_width = display_width_char(ch);
        if width >= start_width {
            if let Some(limit) = len_width {
                if display_width_str(&out) + ch_width > limit {
                    break;
                }
            }
            out.push(ch);
        }
        width += ch_width;
    }
    out
}

fn lower_ascii(s: &str) -> String {
    s.chars().map(|c| c.to_ascii_lowercase()).collect()
}

fn upper_ascii(s: &str) -> String {
    s.chars().map(|c| c.to_ascii_uppercase()).collect()
}

fn default_for_ret_form(ret_form: i64) -> Value {
    if prop_access::ret_form_is_string(ret_form) {
        Value::Str(String::new())
    } else {
        Value::Int(0)
    }
}

fn parse_chain<'a>(
    ctx: &'a CommandContext,
    form_id: u32,
    args: &'a [Value],
) -> Option<(usize, &'a [i32])> {
    prop_access::parse_element_chain_ctx(ctx, form_id, args)
}

fn collect_params<'a>(chain_pos: usize, args: &'a [Value]) -> &'a [Value] {
    prop_access::script_args(args, chain_pos)
}

fn execute_str_op(current: &mut String, op: i32, params: &[Value], al_id: Option<i64>) -> Value {
    match op {
        str_op::UPPER => Value::Str(upper_ascii(current)),
        str_op::LOWER => Value::Str(lower_ascii(current)),
        str_op::CNT => Value::Int(current.chars().count() as i64),
        str_op::LEN => Value::Int(display_width_str(current) as i64),
        str_op::LEFT => {
            let len = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
            Value::Str(current.chars().take(len).collect())
        }
        str_op::LEFT_LEN => {
            let len = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
            Value::Str(left_by_display_width(current, len))
        }
        str_op::RIGHT => {
            let len = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
            let total = current.chars().count();
            let start = total.saturating_sub(len);
            Value::Str(current.chars().skip(start).collect())
        }
        str_op::RIGHT_LEN => {
            let len = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
            Value::Str(right_by_display_width(current, len))
        }
        str_op::MID => {
            let start = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
            if al_id.unwrap_or(0) == 0 || params.len() <= 1 {
                Value::Str(current.chars().skip(start).collect())
            } else {
                let len = params.get(1).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                Value::Str(current.chars().skip(start).take(len).collect())
            }
        }
        str_op::MID_LEN => {
            let start = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
            let len = if al_id.unwrap_or(0) == 0 || params.len() <= 1 {
                None
            } else {
                Some(params.get(1).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize)
            };
            Value::Str(mid_by_display_width(current, start, len))
        }
        str_op::SEARCH => {
            let needle = params.first().and_then(|v| v.as_str()).unwrap_or("");
            let hay = lower_ascii(current);
            let needle = lower_ascii(needle);
            Value::Int(hay.find(&needle).map(|v| v as i64).unwrap_or(-1))
        }
        str_op::SEARCH_LAST => {
            let needle = params.first().and_then(|v| v.as_str()).unwrap_or("");
            let hay = lower_ascii(current);
            let needle = lower_ascii(needle);
            Value::Int(hay.rfind(&needle).map(|v| v as i64).unwrap_or(-1))
        }
        str_op::GET_CODE => {
            let pos = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
            let code = current.chars().nth(pos).map(|c| c as i64).unwrap_or(-1);
            Value::Int(code)
        }
        str_op::TONUM => Value::Int(current.parse::<i64>().unwrap_or(0)),
        _ => Value::Str(current.clone()),
    }
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let Some((chain_pos, chain_ref)) = parse_chain(ctx, form_id, args) else {
        ctx.push(Value::Str(String::new()));
        return Ok(true);
    };
    let chain = chain_ref.to_vec();

    let params = collect_params(chain_pos, args);
    let (al_id, ret_form) = crate::runtime::forms::prop_access::current_vm_meta(ctx);
    let (_, _, rhs_meta) =
        crate::runtime::forms::prop_access::infer_assign_and_ret_ctx(ctx, chain_pos, args);
    let rhs_cmd = if al_id == Some(1) {
        rhs_meta.clone()
    } else {
        None
    };
    let rhs_prop = rhs_meta;

    if chain.len() >= 3
        && (chain[1] == ctx.ids.elm_array || chain[1] == crate::runtime::forms::codes::ELM_ARRAY)
    {
        let idx = chain[2].max(0) as usize;
        let out = {
            let list = ctx
                .globals
                .str_lists
                .entry(form_id)
                .or_insert_with(Vec::new);
            ensure_len(list, idx);
            let current = &mut list[idx];

            if chain.len() == 3 {
                if let Some(Value::Str(rhs)) = rhs_prop.or(rhs_cmd) {
                    *current = rhs;
                    Value::Int(0)
                } else {
                    Value::Str(current.clone())
                }
            } else {
                let op = chain[3];
                execute_str_op(current, op, params, al_id)
            }
        };
        ctx.push(out);
        return Ok(true);
    }

    if chain.len() >= 2 {
        match chain[1] {
            str_list_op::INIT => {
                ctx.globals
                    .str_lists
                    .entry(form_id)
                    .or_insert_with(Vec::new)
                    .clear();
                ctx.push(Value::Int(0));
                return Ok(true);
            }
            str_list_op::RESIZE => {
                let n = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                ctx.globals
                    .str_lists
                    .entry(form_id)
                    .or_insert_with(Vec::new)
                    .resize_with(n, String::new);
                ctx.push(Value::Int(0));
                return Ok(true);
            }
            str_list_op::GET_SIZE => {
                let n = ctx
                    .globals
                    .str_lists
                    .entry(form_id)
                    .or_insert_with(Vec::new)
                    .len() as i64;
                ctx.push(Value::Int(n));
                return Ok(true);
            }
            _ => {}
        }
    }

    ctx.push(default_for_ret_form(ret_form.unwrap_or(20)));
    Ok(true)
}
