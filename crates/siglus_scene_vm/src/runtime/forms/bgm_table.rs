use anyhow::Result;

use crate::runtime::{CommandContext, Value};

use super::codes::bgm_table_op;

fn store_or_push_bgm_table_prop(ctx: &mut CommandContext, op: i64, args: &[Value]) {
    let form_key = if ctx.ids.form_global_bgm_table != 0 {
        ctx.ids.form_global_bgm_table
    } else {
        super::codes::FORM_GLOBAL_BGM_TABLE
    };
    let prop = op as i32;
    if let Some(v) = args.get(1).cloned() {
        match v {
            Value::Str(s) => {
                ctx.globals
                    .str_props
                    .entry(form_key)
                    .or_default()
                    .insert(prop, s);
            }
            Value::Int(n) => {
                ctx.globals
                    .int_props
                    .entry(form_key)
                    .or_default()
                    .insert(prop, n);
            }
            _ => {}
        }
        ctx.push(Value::Int(0));
        return;
    }
    if let Some(s) = ctx
        .globals
        .str_props
        .get(&form_key)
        .and_then(|m| m.get(&prop))
        .cloned()
    {
        ctx.push(Value::Str(s));
        return;
    }
    let v = ctx
        .globals
        .int_props
        .get(&form_key)
        .and_then(|m| m.get(&prop).copied())
        .unwrap_or(0);
    ctx.push(Value::Int(v));
}

fn trim_args(args: &[Value]) -> &[Value] {
    if args.len() >= 3
        && matches!(args[args.len() - 3], Value::Element(_))
        && matches!(args[args.len() - 2], Value::Int(_))
        && matches!(args[args.len() - 1], Value::Int(_))
    {
        &args[..args.len() - 3]
    } else {
        args
    }
}

fn arg_str<'a>(args: &'a [Value], idx: usize) -> Option<&'a str> {
    match args.get(idx) {
        Some(Value::Str(s)) => Some(s.as_str()),
        Some(Value::NamedArg { value, .. }) => value.as_str(),
        _ => None,
    }
}

fn arg_int(args: &[Value], idx: usize) -> Option<i64> {
    args.get(idx).and_then(|v| v.as_i64())
}

fn normalize_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let args = trim_args(args);
    let Some(op) = args.get(0).and_then(|v| v.as_i64()) else {
        ctx.push(Value::Int(0));
        return Ok(true);
    };

    match op {
        bgm_table_op::GET_COUNT => {
            ctx.push(Value::Int(ctx.globals.bgm_table_listened.len() as i64));
            Ok(true)
        }
        bgm_table_op::GET_LISTEN_BY_NAME => {
            let Some(name) = arg_str(args, 1) else {
                ctx.push(Value::Int(0));
                return Ok(true);
            };
            let key = normalize_name(name);
            let listened = ctx
                .globals
                .bgm_table_listened
                .get(&key)
                .copied()
                .unwrap_or(ctx.globals.bgm_table_all_flag);
            ctx.push(Value::Int(if listened { 1 } else { 0 }));
            Ok(true)
        }
        bgm_table_op::SET_LISTEN_CURRENT => {
            let Some(name) = arg_str(args, 1) else {
                ctx.push(Value::Int(0));
                return Ok(true);
            };
            let listened = arg_int(args, 2).unwrap_or(0) != 0;
            let key = normalize_name(name);
            ctx.globals.bgm_table_listened.insert(key, listened);
            ctx.push(Value::Int(0));
            Ok(true)
        }
        bgm_table_op::SET_ALL_FLAG => {
            let listened = arg_int(args, 1).unwrap_or(0) != 0;
            ctx.globals.bgm_table_all_flag = listened;
            for v in ctx.globals.bgm_table_listened.values_mut() {
                *v = listened;
            }
            ctx.push(Value::Int(0));
            Ok(true)
        }
        _ => {
            store_or_push_bgm_table_prop(ctx, op, args);
            Ok(true)
        }
    }
}
