use anyhow::Result;

use crate::runtime::{CommandContext, Value};

use super::codes::bgm_table_op;

fn store_or_push_bgm_table_prop(ctx: &mut CommandContext, op: i32, args: &[Value]) {
    let form_key = if ctx.ids.form_global_bgm_table != 0 {
        ctx.ids.form_global_bgm_table
    } else {
        super::codes::FORM_GLOBAL_BGM_TABLE
    };
    let prop = op;
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

fn bgm_declared_count(ctx: &CommandContext) -> usize {
    ctx.tables
        .gameexe
        .as_ref()
        .map(|cfg| {
            cfg.get_usize("BGM.CNT")
                .unwrap_or_else(|| cfg.indexed_count("BGM"))
        })
        .unwrap_or(0)
}

fn bgm_count(ctx: &CommandContext) -> usize {
    bgm_declared_count(ctx)
        .max(ctx.globals.bgm_table_flags.len())
        .max(ctx.globals.bgm_table_listened.len())
}

fn ensure_bgm_flags_size(ctx: &mut CommandContext) {
    let count = bgm_declared_count(ctx);
    if ctx.globals.bgm_table_flags.len() < count {
        ctx.globals
            .bgm_table_flags
            .resize(count, ctx.globals.bgm_table_all_flag);
    }
}

fn bgm_regist_name(ctx: &CommandContext, index: usize) -> Option<String> {
    let cfg = ctx.tables.gameexe.as_ref()?;
    cfg.get_indexed_item_unquoted("BGM", index, 0)
        .or_else(|| cfg.get_indexed_field_unquoted("BGM", index, "REGIST_NAME"))
        .or_else(|| cfg.get_indexed_unquoted("BGM", index))
        .map(|s| s.to_string())
}

fn bgm_no_by_regist_name(ctx: &CommandContext, name: &str) -> Option<usize> {
    let needle = normalize_name(name);
    let count = bgm_declared_count(ctx);
    for i in 0..count {
        let Some(regist_name) = bgm_regist_name(ctx, i) else {
            continue;
        };
        if normalize_name(&regist_name) == needle {
            return Some(i);
        }
    }
    None
}

pub(crate) fn mark_listened_by_name(ctx: &mut CommandContext, name: &str, listened: bool) -> bool {
    ensure_bgm_flags_size(ctx);
    let key = normalize_name(name);
    let index = bgm_no_by_regist_name(ctx, name);
    if let Some(index) = index {
        if ctx.globals.bgm_table_flags.len() <= index {
            ctx.globals
                .bgm_table_flags
                .resize(index + 1, ctx.globals.bgm_table_all_flag);
        }
        ctx.globals.bgm_table_flags[index] = listened;
        ctx.globals.bgm_table_listened.insert(key, listened);
        true
    } else {
        false
    }
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let args = trim_args(args);
    let Some(op) = args.get(0).and_then(|v| v.as_i64()).map(|v| v as i32) else {
        ctx.push(Value::Int(0));
        return Ok(true);
    };

    match op {
        bgm_table_op::GET_COUNT => {
            ctx.push(Value::Int(bgm_count(ctx) as i64));
            Ok(true)
        }
        bgm_table_op::GET_LISTEN_BY_NAME => {
            let Some(name) = arg_str(args, 1) else {
                ctx.push(Value::Int(-1));
                return Ok(true);
            };
            ensure_bgm_flags_size(ctx);
            let res = if let Some(index) = bgm_no_by_regist_name(ctx, name) {
                ctx.globals
                    .bgm_table_flags
                    .get(index)
                    .copied()
                    .unwrap_or(ctx.globals.bgm_table_all_flag) as i64
            } else {
                -1
            };
            ctx.push(Value::Int(res));
            Ok(true)
        }
        bgm_table_op::SET_LISTEN_CURRENT => {
            let Some(name) = arg_str(args, 1) else {
                ctx.push(Value::Int(0));
                return Ok(true);
            };
            let listened = arg_int(args, 2).unwrap_or(0) != 0;
            let _ = mark_listened_by_name(ctx, name, listened);
            ctx.push(Value::Int(0));
            Ok(true)
        }
        bgm_table_op::SET_ALL_FLAG => {
            let listened = arg_int(args, 1).unwrap_or(0) != 0;
            ctx.globals.bgm_table_all_flag = listened;
            ensure_bgm_flags_size(ctx);
            for v in &mut ctx.globals.bgm_table_flags {
                *v = listened;
            }
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
