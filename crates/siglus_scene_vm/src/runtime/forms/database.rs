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

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let Some((chain_pos, chain)) = parse_element_chain(form_id, args) else {
        return stub::dispatch(ctx, form_id, args);
    };

    if chain.len() < 2 {
        return stub::dispatch(ctx, form_id, args);
    }

    let op = chain[1];
    let params = if chain_pos > 1 { &args[1..chain_pos] } else { &[] };
    let p_i32 = |i: usize| -> i32 { params.get(i).and_then(|v| v.as_i64()).unwrap_or(0) as i32 };
    let p_str = |i: usize| -> &str { params.get(i).and_then(|v| v.as_str()).unwrap_or("") };

    // DATABASELIST.GET_SIZE
    if ctx.ids.database_list_get_size != 0 && op == ctx.ids.database_list_get_size {
        if ctx.globals.database_off {
            ctx.push(Value::Int(0));
        } else {
            ctx.push(Value::Int(ctx.tables.databases.len() as i64));
        }
        return Ok(true);
    }

    // DATABASELIST[db].<op>(...)
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

        // When disabled, keep VM stable and return neutral values.
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

        // GET_NUM(item_call_no, column_call_no)
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

        // GET_STR(item_call_no, column_call_no)
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

        // CHECK_ITEM(item_call_no) -> row index or -1
        if ctx.ids.database_check_item != 0 && db_op == ctx.ids.database_check_item {
            let item_call_no = p_i32(0);
            ctx.push(Value::Int(db.check_item_no(item_call_no) as i64));
            return Ok(true);
        }

        // CHECK_COLUMN(column_call_no) -> 0/1/2
        if ctx.ids.database_check_column != 0 && db_op == ctx.ids.database_check_column {
            let col_call_no = p_i32(0);
            ctx.push(Value::Int(db.check_column_no(col_call_no) as i64));
            return Ok(true);
        }

        // FIND_NUM(start_row_idx, column_call_no, value) -> row index or -1
        if ctx.ids.database_find_num != 0 && db_op == ctx.ids.database_find_num {
            let start_idx = p_i32(0);
            let col_call_no = p_i32(1);
            let value = p_i32(2);
            ctx.push(Value::Int(find_num_from(db, start_idx, col_call_no, value)));
            return Ok(true);
        }

        // FIND_STR(start_row_idx, column_call_no, str) -> row index or -1 (case-insensitive ASCII)
        if ctx.ids.database_find_str != 0 && db_op == ctx.ids.database_find_str {
            let start_idx = p_i32(0);
            let col_call_no = p_i32(1);
            let needle = p_str(2);
            ctx.push(Value::Int(find_str_from(db, start_idx, col_call_no, needle, false)));
            return Ok(true);
        }

        // FIND_STR_REAL(start_row_idx, column_call_no, str) -> row index or -1 (case-sensitive)
        if ctx.ids.database_find_str_real != 0 && db_op == ctx.ids.database_find_str_real {
            let start_idx = p_i32(0);
            let col_call_no = p_i32(1);
            let needle = p_str(2);
            ctx.push(Value::Int(find_str_from(db, start_idx, col_call_no, needle, true)));
            return Ok(true);
        }

        // Unknown database op.
        ctx.unknown.record_code(OpCode::form(form_id));
        ctx.unknown.record_element_chain(form_id, chain, "DATABASE");
        ctx.push(Value::Int(0));
        return Ok(true);
    }

    // Unknown op.
    ctx.unknown.record_code(OpCode::form(form_id));
    ctx.unknown.record_element_chain(form_id, chain, "DATABASE");
    stub::dispatch(ctx, form_id, args)
}
