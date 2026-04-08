use anyhow::Result;

use crate::runtime::forms::prop_access;
use crate::runtime::{CommandContext, Value};

fn find_num_from(db: &siglus_assets::dbs::DbsDatabase, start_idx: i32, col_call_no: i32, needle: i32) -> i64 {
    let start = if start_idx < 0 { 0 } else { start_idx as usize };
    let rows = db.rows();
    if start >= rows.len() {
        return -1;
    }
    for (i, r) in rows.iter().enumerate().skip(start) {
        if let Ok(Some(v)) = db.get_data_int(r.call_no, col_call_no) {
            if v == needle {
                return i as i64;
            }
        }
    }
    -1
}

fn find_str_from(
    db: &siglus_assets::dbs::DbsDatabase,
    start_idx: i32,
    col_call_no: i32,
    needle: &str,
    case_sensitive: bool,
) -> i64 {
    let start = if start_idx < 0 { 0 } else { start_idx as usize };
    let rows = db.rows();
    if start >= rows.len() {
        return -1;
    }

    if case_sensitive {
        for (i, r) in rows.iter().enumerate().skip(start) {
            if let Ok(Some(v)) = db.get_data_str(r.call_no, col_call_no) {
                if v == needle {
                    return i as i64;
                }
            }
        }
        return -1;
    }

    let needle_lc = needle.to_ascii_lowercase();
    for (i, r) in rows.iter().enumerate().skip(start) {
        if let Ok(Some(v)) = db.get_data_str(r.call_no, col_call_no) {
            if v.to_ascii_lowercase() == needle_lc {
                return i as i64;
            }
        }
    }
    -1
}

fn composite_db_op_key(db_no: i32, op: i32) -> i32 {
    ((db_no & 0x7fff) << 16) ^ (op & 0xffff)
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let parsed = prop_access::parse_element_chain(form_id, args);
    let (chain_pos, chain) = match parsed {
        Some((pos, ch)) if ch.len() >= 2 => (Some(pos), Some(ch)),
        _ => (None, None),
    };

    if let Some(chain) = chain {
        let op = chain[1];
        let params = if chain_pos.unwrap_or(0) > 1 {
            &args[1..chain_pos.unwrap()]
        } else {
            &[]
        };
        let p_i32 = |i: usize| -> i32 { params.get(i).and_then(|v| v.as_i64()).unwrap_or(0) as i32 };
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

            let db = match ctx.tables.databases.get(db_no as usize) {
                Some(db) => db,
                None => {
                    ctx.push(Value::Int(0));
                    return Ok(true);
                }
            };

            let db_op = chain[3];

            if ctx.ids.database_get_num != 0 && db_op == ctx.ids.database_get_num {
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
                let start_idx = p_i32(0);
                let col_call_no = p_i32(1);
                let value = p_i32(2);
                ctx.push(Value::Int(find_num_from(db, start_idx, col_call_no, value)));
                return Ok(true);
            }

            if ctx.ids.database_find_str != 0 && db_op == ctx.ids.database_find_str {
                let start_idx = p_i32(0);
                let col_call_no = p_i32(1);
                let needle = p_str(2);
                ctx.push(Value::Int(find_str_from(db, start_idx, col_call_no, needle, false)));
                return Ok(true);
            }

            if ctx.ids.database_find_str_real != 0 && db_op == ctx.ids.database_find_str_real {
                let start_idx = p_i32(0);
                let col_call_no = p_i32(1);
                let needle = p_str(2);
                ctx.push(Value::Int(find_str_from(db, start_idx, col_call_no, needle, true)));
                return Ok(true);
            }

            let prop_key = composite_db_op_key(db_no, db_op);
            prop_access::store_or_push_prop(ctx, form_id, prop_key, chain_pos.unwrap(), args);
            return Ok(true);
        }

        prop_access::store_or_push_prop(ctx, form_id, op, chain_pos.unwrap(), args);
        return Ok(true);
    }

    prop_access::dispatch_stateful_form(ctx, form_id, args);
    Ok(true)
}
