use anyhow::Result;

use crate::runtime::forms::prop_access;
use crate::runtime::{CommandContext, Value};

fn composite_db_op_key(db_no: i32, op: i32) -> i32 {
    ((db_no & 0x7fff) << 16) ^ (op & 0xffff)
}

fn ref_chain_from_value(v: &Value) -> Option<(Option<i32>, &[i32])> {
    match v {
        Value::Element(chain) => Some((None, chain.as_slice())),
        Value::NamedArg { id, value } => match value.as_ref() {
            Value::Element(chain) => Some((Some(*id), chain.as_slice())),
            _ => None,
        },
        _ => None,
    }
}

fn database_get_data(
    ctx: &mut CommandContext,
    db: &siglus_assets::dbs::DbsDatabase,
    item_call_no: i32,
    refs: &[Value],
) {
    let mut positional_column_no = 0i32;
    for arg in refs {
        let Some((explicit_column_no, chain)) = ref_chain_from_value(arg) else {
            positional_column_no += 1;
            continue;
        };
        let column_call_no = explicit_column_no.unwrap_or(positional_column_no);
        positional_column_no += 1;

        match db.check_column_no(column_call_no) {
            1 => {
                if let Ok(Some(v)) = db.get_data_int(item_call_no, column_call_no) {
                    prop_access::assign_to_chain(ctx, chain, Value::Int(v as i64));
                } else {
                    prop_access::assign_to_chain(ctx, chain, Value::Int(0));
                }
            }
            2 => {
                if let Ok(Some(v)) = db.get_data_str(item_call_no, column_call_no) {
                    prop_access::assign_to_chain(ctx, chain, Value::Str(v));
                } else {
                    prop_access::assign_to_chain(ctx, chain, Value::Str(String::new()));
                }
            }
            _ => {
                prop_access::assign_to_chain(ctx, chain, Value::Int(0));
            }
        }
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
        let params = if let Some(pos) = chain_pos {
            prop_access::script_args(args, pos)
        } else {
            &[]
        };
        let p_i32 = |i: usize| -> i32 {
            params
                .get(i)
                .and_then(|v| v.as_i64())
                .unwrap_or(0) as i32
        };
        let p_str = |i: usize| -> &str { params.get(i).and_then(|v| v.as_str()).unwrap_or("") };

        if ctx.ids.database_list_get_size != 0 && op == ctx.ids.database_list_get_size {
            if ctx.globals.database_off {
                ctx.push(Value::Int(0));
            } else {
                ctx.push(Value::Int(ctx.tables.databases.len() as i64));
            }
            return Ok(true);
        }

        if op == ctx.ids.elm_array {
            if chain.len() < 4 {
                ctx.push(Value::Int(0));
                return Ok(true);
            }

            let db_no = chain[2];
            if db_no < 0 {
                ctx.push(Value::Int(0));
                return Ok(true);
            }

            if ctx.globals.database_off {
                let db_op = chain[3];
                if ctx.ids.database_get_str != 0 && db_op == ctx.ids.database_get_str {
                    ctx.push(Value::Str(String::new()));
                } else {
                    ctx.push(Value::Int(0));
                }
                return Ok(true);
            }

            let db = match ctx.tables.databases.get(db_no as usize).cloned() {
                Some(db) => db,
                None => {
                    ctx.push(Value::Int(0));
                    return Ok(true);
                }
            };

            let db_op = chain[3];

            if db_op == ctx.ids.database_get_num {
                let item_call_no = p_i32(0);
                let col_call_no = p_i32(1);
                let mut out: i64 = 0;
                if let Ok(Some(v)) = db.get_data_int(item_call_no, col_call_no) {
                    out = v as i64;
                }
                ctx.push(Value::Int(out));
                return Ok(true);
            }

            if ctx.ids.database_get_str != 0 && db_op == ctx.ids.database_get_str {
                let item_call_no = p_i32(0);
                let col_call_no = p_i32(1);
                let mut out = String::new();
                if let Ok(Some(v)) = db.get_data_str(item_call_no, col_call_no) {
                    out = v;
                }
                ctx.push(Value::Str(out));
                return Ok(true);
            }

            if ctx.ids.database_get_data != 0 && db_op == ctx.ids.database_get_data {
                let item_call_no = p_i32(0);
                let refs = if params.len() > 1 { &params[1..] } else { &[] };
                database_get_data(ctx, &db, item_call_no, refs);
                ctx.push(Value::Int(0));
                return Ok(true);
            }

            if ctx.ids.database_check_item != 0 && db_op == ctx.ids.database_check_item {
                let item_call_no = p_i32(0);
                ctx.push(Value::Int(db.check_item_no(item_call_no) as i64));
                return Ok(true);
            }

            if ctx.ids.database_check_column != 0 && db_op == ctx.ids.database_check_column {
                let col_call_no = p_i32(0);
                ctx.push(Value::Int(db.check_column_no(col_call_no) as i64));
                return Ok(true);
            }

            if ctx.ids.database_find_num != 0 && db_op == ctx.ids.database_find_num {
                let col_call_no = p_i32(0);
                let value = p_i32(1);
                let out = db.find_num(col_call_no, value).unwrap_or(-1);
                ctx.push(Value::Int(out as i64));
                return Ok(true);
            }

            if ctx.ids.database_find_str != 0 && db_op == ctx.ids.database_find_str {
                let col_call_no = p_i32(0);
                let needle = p_str(1);
                let out = db.find_str(col_call_no, needle).unwrap_or(-1);
                ctx.push(Value::Int(out as i64));
                return Ok(true);
            }

            if ctx.ids.database_find_str_real != 0 && db_op == ctx.ids.database_find_str_real {
                let col_call_no = p_i32(0);
                let needle = p_str(1);
                let out = db.find_str_real(col_call_no, needle).unwrap_or(-1);
                ctx.push(Value::Int(out as i64));
                return Ok(true);
            }

            let prop_key = composite_db_op_key(db_no, db_op);
            prop_access::store_or_push_prop(ctx, form_id, prop_key, chain_pos.unwrap(), args);
            return Ok(true);
        }

        prop_access::store_or_push_prop(ctx, form_id, op, chain_pos.unwrap(), args);
        return Ok(true);
    }

    Ok(false)
}
