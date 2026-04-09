use anyhow::{bail, Result};

use crate::runtime::{CommandContext, Value};

use super::codes::mov_op;

fn store_or_push_mov_prop(ctx: &mut CommandContext, op: i64, args: &[Value]) {
    let form_key = if ctx.ids.form_global_mov != 0 {
        ctx.ids.form_global_mov
    } else {
        super::codes::FORM_GLOBAL_MOV
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
    args.get(idx).and_then(|v| v.as_str())
}

fn arg_int(args: &[Value], idx: usize) -> Option<i64> {
    args.get(idx).and_then(|v| v.as_i64())
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let args = trim_args(args);
    if args.is_empty() {
        bail!("MOV form expects at least one argument (op id)");
    }

    let op = match args[0] {
        Value::Int(v) => v,
        _ => {
            ctx.push(Value::Int(0));
            return Ok(true);
        }
    };

    match op {
        mov_op::PLAY | mov_op::PLAY_WAIT | mov_op::PLAY_WAIT_KEY => {
            let name = match arg_str(args, 1) {
                Some(s) => s,
                None => {
                    store_or_push_mov_prop(ctx, op, args);
                    return Ok(true);
                }
            };
            let _x = arg_int(args, 2).unwrap_or(0);
            let _y = arg_int(args, 3).unwrap_or(0);
            let _w = arg_int(args, 4).unwrap_or(ctx.screen_w as i64);
            let _h = arg_int(args, 5).unwrap_or(ctx.screen_h as i64);

            let wait = op == mov_op::PLAY_WAIT || op == mov_op::PLAY_WAIT_KEY;
            let key_skip = op == mov_op::PLAY_WAIT_KEY;
            let info = ctx.movie.play(name, wait, key_skip)?;
            if wait {
                if let Some(ms) = info.duration_ms() {
                    if key_skip {
                        ctx.wait.wait_ms_key(ms);
                    } else {
                        ctx.wait.wait_ms(ms);
                    }
                }
            }
            Ok(true)
        }
        mov_op::STOP => {
            ctx.movie.stop();
            Ok(true)
        }
        _ => {
            store_or_push_mov_prop(ctx, op, args);
            Ok(true)
        }
    }
}
