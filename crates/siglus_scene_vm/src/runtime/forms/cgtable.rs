use anyhow::Result;

use crate::runtime::forms::stub;
use crate::runtime::{CommandContext, OpCode, Value};

fn ensure_cg_flags_size(ctx: &mut CommandContext, n: usize) {
    let mut want = n.max(32);
    if let Some(cnt) = ctx.tables.cgtable_flag_cnt {
        want = want.max(cnt.max(32));
    }
    if ctx.tables.cg_flags.len() < want {
        ctx.tables.cg_flags.resize(want, 0);
    }
}

fn cgtable_look_cnt(ctx: &CommandContext) -> i64 {
    if ctx.globals.cg_table_off {
        return 0;
    }
    let Some(t) = ctx.tables.cgtable.as_ref() else {
        return 0;
    };
    let mut cnt: i64 = 0;
    for e in &t.entries {
        if e.flag_no >= 0 {
            let idx = e.flag_no as usize;
            if idx < ctx.tables.cg_flags.len() && ctx.tables.cg_flags[idx] != 0 {
                cnt += 1;
            }
        }
    }
    cnt
}

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

    // CGTABLE.FLAG[...] behaves like an integer list exposing cg_flags.
    if ctx.ids.cgtable_flag != 0 && op == ctx.ids.cgtable_flag {
        // Remaining chain after CGTABLE.FLAG
        let rest = &chain[2..];
        if rest.len() >= 2 && rest[0] == ctx.ids.elm_array {
            let idx_i32 = rest[1];
            if idx_i32 < 0 {
                ctx.push(Value::Int(0));
                return Ok(true);
            }

            let idx = idx_i32 as usize;
            ensure_cg_flags_size(ctx, idx + 1);

            // Command call shape: [rhs?, Element(chain), al_id, ret_form]
            // Property assign shape: [op_id, al_id, rhs, Element(chain)]
            let mut al_id: Option<i64> = None;
            let mut rhs: Option<i64> = None;

            if chain_pos + 2 < args.len() {
                al_id = args.get(chain_pos + 1).and_then(|v| v.as_i64());
                if al_id == Some(1) {
                    rhs = args.get(0).and_then(|v| v.as_i64());
                }
            } else if chain_pos == args.len().saturating_sub(1) {
                al_id = args.get(1).and_then(|v| v.as_i64());
                if al_id == Some(1) {
                    rhs = args.get(2).and_then(|v| v.as_i64());
                }
            }

            if al_id == Some(1) {
                if !ctx.globals.cg_table_off {
                    let v = rhs.unwrap_or(0);
                    ctx.tables.cg_flags[idx] = if v != 0 { 1 } else { 0 };
                }
                ctx.push(Value::Int(0));
            } else {
                if ctx.globals.cg_table_off {
                    ctx.push(Value::Int(0));
                } else {
                    ctx.push(Value::Int(ctx.tables.cg_flags[idx] as i64));
                }
            }
            return Ok(true);
        }

        // Unsupported list operation.
        ctx.push(Value::Int(0));
        return Ok(true);
    }

    // For function-style ops, arguments are the values before Element(chain).
    let params = if chain_pos > 1 { &args[1..chain_pos] } else { &[] };
    let p_int = |i: usize| -> i64 { params.get(i).and_then(|v| v.as_i64()).unwrap_or(0) };
    let p_str = |i: usize| -> &str { params.get(i).and_then(|v| v.as_str()).unwrap_or("") };

    // SET_DISABLE
    if ctx.ids.cgtable_set_disable != 0 && op == ctx.ids.cgtable_set_disable {
        ctx.globals.cg_table_off = true;
        ctx.push(Value::Int(0));
        return Ok(true);
    }
    // SET_ENABLE
    if ctx.ids.cgtable_set_enable != 0 && op == ctx.ids.cgtable_set_enable {
        ctx.globals.cg_table_off = false;
        ctx.push(Value::Int(0));
        return Ok(true);
    }
    // SET_ALL_FLAG
    if ctx.ids.cgtable_set_all_flag != 0 && op == ctx.ids.cgtable_set_all_flag {
        let v = p_int(0);
        let b: u8 = if v != 0 { 1 } else { 0 };
        for x in &mut ctx.tables.cg_flags {
            *x = b;
        }
        ctx.push(Value::Int(0));
        return Ok(true);
    }

    // GET_CG_CNT
    if ctx.ids.cgtable_get_cg_cnt != 0 && op == ctx.ids.cgtable_get_cg_cnt {
        let cnt = ctx.tables.cgtable.as_ref().map(|t| t.get_cg_cnt()).unwrap_or(0);
        ctx.push(Value::Int(cnt as i64));
        return Ok(true);
    }

    // GET_LOOK_CNT
    if ctx.ids.cgtable_get_look_cnt != 0 && op == ctx.ids.cgtable_get_look_cnt {
        ctx.push(Value::Int(cgtable_look_cnt(ctx)));
        return Ok(true);
    }

    // GET_LOOK_PERCENT
    if ctx.ids.cgtable_get_look_percent != 0 && op == ctx.ids.cgtable_get_look_percent {
        let total = ctx.tables.cgtable.as_ref().map(|t| t.get_cg_cnt()).unwrap_or(0) as i64;
        if total <= 0 {
            ctx.push(Value::Int(0));
        } else {
            let looked = cgtable_look_cnt(ctx);
            ctx.push(Value::Int(looked * 100 / total));
        }
        return Ok(true);
    }

    // GET_FLAG_NO_BY_NAME
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

    // GET_LOOK_BY_NAME
    if ctx.ids.cgtable_get_look_by_name != 0 && op == ctx.ids.cgtable_get_look_by_name {
        let name = p_str(0);
        if ctx.globals.cg_table_off {
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        let res = if let Some(e) = ctx.tables.cgtable.as_ref().and_then(|t| t.get_sub_from_name(name)) {
            let idx = e.flag_no;
            if idx >= 0 {
                let idx = idx as usize;
                if idx < ctx.tables.cg_flags.len() {
                    ctx.tables.cg_flags[idx] as i64
                } else {
                    0
                }
            } else {
                0
            }
        } else {
            -1
        };
        ctx.push(Value::Int(res));
        return Ok(true);
    }

    // SET_LOOK_BY_NAME
    if ctx.ids.cgtable_set_look_by_name != 0 && op == ctx.ids.cgtable_set_look_by_name {
        let name = p_str(0);
        let v = p_int(1);
        if !ctx.globals.cg_table_off {
            if let Some(e) = ctx.tables.cgtable.as_ref().and_then(|t| t.get_sub_from_name(name)) {
                if e.flag_no >= 0 {
                    let idx = e.flag_no as usize;
                    ensure_cg_flags_size(ctx, idx + 1);
                    ctx.tables.cg_flags[idx] = if v != 0 { 1 } else { 0 };
                }
            }
        }
        ctx.push(Value::Int(0));
        return Ok(true);
    }

    // GET_NAME_BY_FLAG_NO
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

    // Unknown op.
    ctx.unknown.record_code(OpCode::form(form_id));
    ctx.unknown.record_element_chain(form_id, chain, "CGTABLE");
    stub::dispatch(ctx, form_id, args)
}
