use anyhow::Result;

use crate::runtime::forms::prop_access;
use crate::runtime::{CommandContext, Value};

fn ensure_cg_flags_size(ctx: &mut CommandContext, size: usize) {
    if let Some(cnt) = ctx.tables.cgtable_flag_cnt {
        let want = size.max(cnt);
        if ctx.tables.cg_flags.len() < want {
            ctx.tables.cg_flags.resize(want, 0);
        }
    } else if ctx.tables.cg_flags.len() < size {
        ctx.tables.cg_flags.resize(size, 0);
    }
}

fn get_cg_flag(ctx: &CommandContext, flag_no: i32) -> i64 {
    if flag_no < 0 {
        return 0;
    }
    ctx.tables
        .cg_flags
        .get(flag_no as usize)
        .copied()
        .unwrap_or(0) as i64
}

fn set_cg_flag(ctx: &mut CommandContext, flag_no: i32, value: i64) {
    if flag_no < 0 {
        return;
    }
    let idx = flag_no as usize;
    ensure_cg_flags_size(ctx, idx + 1);
    ctx.tables.cg_flags[idx] = if value != 0 { 1 } else { 0 };
}

fn cgtable_look_cnt(ctx: &CommandContext) -> i64 {
    let Some(table) = ctx.tables.cgtable.as_ref() else {
        return 0;
    };
    table
        .entries
        .iter()
        .filter(|e| e.flag_no >= 0 && get_cg_flag(ctx, e.flag_no) != 0)
        .count() as i64
}

pub(crate) fn mark_look_by_name(ctx: &mut CommandContext, name: &str) {
    if ctx.globals.cg_table_off {
        return;
    }
    let flag_no = ctx
        .tables
        .cgtable
        .as_ref()
        .and_then(|t| t.get_sub_from_name(name))
        .map(|e| e.flag_no);
    if let Some(flag_no) = flag_no {
        set_cg_flag(ctx, flag_no, 1);
    }
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let parsed = prop_access::parse_element_chain_ctx(ctx, form_id, args);
    let (chain_pos, chain) = match parsed {
        Some((pos, ch)) if ch.len() >= 2 => (Some(pos), Some(ch)),
        _ => (None, None),
    };

    if let Some(chain) = chain {
        let op = chain[1];

        if op == ctx.ids.cgtable_flag {
            let rest = &chain[2..];
            if rest.len() >= 2 && rest[0] == ctx.ids.elm_array {
                let idx_i32 = rest[1];
                if idx_i32 < 0 {
                    ctx.push(Value::Int(0));
                    return Ok(true);
                }

                let idx = idx_i32 as usize;
                ensure_cg_flags_size(ctx, idx + 1);

                let (al_id, _ret_form, rhs_value) =
                    crate::runtime::forms::prop_access::infer_assign_and_ret_ctx(
                        ctx,
                        chain_pos.unwrap_or(args.len()),
                        args,
                    );
                let rhs = rhs_value.as_ref().and_then(Value::as_i64);

                if al_id == Some(1) {
                    let v = rhs.unwrap_or(0);
                    ctx.tables.cg_flags[idx] = if v != 0 { 1 } else { 0 };
                    ctx.push(Value::Int(0));
                } else {
                    ctx.push(Value::Int(ctx.tables.cg_flags[idx] as i64));
                }
                return Ok(true);
            }

            ctx.push(Value::Int(0));
            return Ok(true);
        }

        let params = if let Some(pos) = chain_pos {
            crate::runtime::forms::prop_access::script_args(args, pos)
        } else {
            &[]
        };
        let p_int = |i: usize| -> i64 { params.get(i).and_then(|v| v.as_i64()).unwrap_or(0) };
        let p_str = |i: usize| -> &str { params.get(i).and_then(|v| v.as_str()).unwrap_or("") };

        if ctx.ids.cgtable_set_disable != 0 && op == ctx.ids.cgtable_set_disable {
            ctx.globals.cg_table_off = true;
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        if ctx.ids.cgtable_set_enable != 0 && op == ctx.ids.cgtable_set_enable {
            ctx.globals.cg_table_off = false;
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        if ctx.ids.cgtable_set_all_flag != 0 && op == ctx.ids.cgtable_set_all_flag {
            let v = p_int(0);
            let flag_nos: Vec<i32> = ctx
                .tables
                .cgtable
                .as_ref()
                .map(|t| t.entries.iter().map(|e| e.flag_no).collect())
                .unwrap_or_default();
            for flag_no in flag_nos {
                set_cg_flag(ctx, flag_no, v);
            }
            ctx.push(Value::Int(0));
            return Ok(true);
        }

        if ctx.ids.cgtable_get_cg_cnt != 0 && op == ctx.ids.cgtable_get_cg_cnt {
            let cnt = ctx
                .tables
                .cgtable
                .as_ref()
                .map(|t| t.get_cg_cnt())
                .unwrap_or(0);
            ctx.push(Value::Int(cnt as i64));
            return Ok(true);
        }

        if ctx.ids.cgtable_get_look_cnt != 0 && op == ctx.ids.cgtable_get_look_cnt {
            ctx.push(Value::Int(cgtable_look_cnt(ctx)));
            return Ok(true);
        }

        if ctx.ids.cgtable_get_look_percent != 0 && op == ctx.ids.cgtable_get_look_percent {
            let total = ctx
                .tables
                .cgtable
                .as_ref()
                .map(|t| t.get_cg_cnt())
                .unwrap_or(0) as i64;
            let looked = cgtable_look_cnt(ctx);
            let percent = if total <= 0 || looked <= 0 {
                0
            } else {
                let raw = looked * 100 / total;
                if raw <= 0 {
                    1
                } else {
                    raw
                }
            };
            ctx.push(Value::Int(percent));
            return Ok(true);
        }

        if ctx.ids.cgtable_get_flag_no_by_name != 0 && op == ctx.ids.cgtable_get_flag_no_by_name {
            let name = p_str(0);
            let res = ctx
                .tables
                .cgtable
                .as_ref()
                .and_then(|t| t.get_sub_from_name(name))
                .map(|e| e.flag_no as i64)
                .unwrap_or(-1);
            ctx.push(Value::Int(res));
            return Ok(true);
        }

        if ctx.ids.cgtable_get_look_by_name != 0 && op == ctx.ids.cgtable_get_look_by_name {
            let name = p_str(0);
            let res = if let Some(e) = ctx
                .tables
                .cgtable
                .as_ref()
                .and_then(|t| t.get_sub_from_name(name))
            {
                get_cg_flag(ctx, e.flag_no)
            } else {
                -1
            };
            ctx.push(Value::Int(res));
            return Ok(true);
        }

        if ctx.ids.cgtable_set_look_by_name != 0 && op == ctx.ids.cgtable_set_look_by_name {
            let name = p_str(0);
            let v = p_int(1);
            let flag_no = ctx
                .tables
                .cgtable
                .as_ref()
                .and_then(|t| t.get_sub_from_name(name))
                .map(|e| e.flag_no);
            if let Some(flag_no) = flag_no {
                set_cg_flag(ctx, flag_no, v);
            }
            ctx.push(Value::Int(0));
            return Ok(true);
        }

        if ctx.ids.cgtable_get_name_by_flag_no != 0 && op == ctx.ids.cgtable_get_name_by_flag_no {
            let flag_no = p_int(0) as i32;
            let name = ctx
                .tables
                .cgtable
                .as_ref()
                .and_then(|t| t.get_sub_from_flag_no(flag_no))
                .map(|e| e.name.as_str())
                .unwrap_or("");
            ctx.push(Value::Str(name.to_string()));
            return Ok(true);
        }

        prop_access::store_or_push_prop(ctx, form_id, op, chain_pos.unwrap(), args);
        return Ok(true);
    }

    Ok(false)
}
