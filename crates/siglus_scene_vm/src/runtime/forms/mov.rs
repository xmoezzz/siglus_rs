use anyhow::{bail, Result};

use crate::runtime::{CommandContext, Value};

use super::codes::mov_op;

fn store_or_push_mov_prop(ctx: &mut CommandContext, op: i32, args: &[Value]) {
    let form_key = if ctx.ids.form_global_mov != 0 {
        ctx.ids.form_global_mov
    } else {
        super::codes::FORM_GLOBAL_MOV
    };
    let prop = op;
    if let Some(v) = args.get(0).cloned() {
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

fn arg_str<'a>(args: &'a [Value], idx: usize) -> Option<&'a str> {
    args.get(idx).and_then(|v| v.as_str())
}

fn arg_int(args: &[Value], idx: usize) -> Option<i64> {
    args.get(idx).and_then(|v| v.as_i64())
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let Some(op) =
        crate::runtime::forms::prop_access::current_op_from_ctx_or_args(ctx, args)
    else {
        bail!("MOV form expects an element opcode");
    };
    let args = crate::runtime::forms::prop_access::params_without_op(ctx, args);

    match op {
        mov_op::PLAY | mov_op::PLAY_WAIT | mov_op::PLAY_WAIT_KEY => {
            let name = match arg_str(args, 0) {
                Some(s) => s,
                None => {
                    store_or_push_mov_prop(ctx, op, args);
                    return Ok(true);
                }
            };
            let x = arg_int(args, 1).unwrap_or(0) as i32;
            let y = arg_int(args, 2).unwrap_or(0) as i32;
            let raw_w = arg_int(args, 3).unwrap_or(ctx.screen_w as i64);
            let raw_h = arg_int(args, 4).unwrap_or(ctx.screen_h as i64);
            let w = if raw_w <= 0 { ctx.screen_w.max(1) } else { raw_w as u32 };
            let h = if raw_h <= 0 { ctx.screen_h.max(1) } else { raw_h as u32 };

            let wait = op == mov_op::PLAY_WAIT || op == mov_op::PLAY_WAIT_KEY;
            let key_skip = op == mov_op::PLAY_WAIT_KEY;
            if let Some(id) = ctx.globals.mov.audio_id.take() {
                ctx.movie.stop_audio(id);
            }
            let info = ctx.movie.play(name, wait, key_skip)?;
            let total_ms = info.duration_ms();
            ctx.globals.mov.start(name.to_string(), x, y, w, h, total_ms, key_skip);
            if std::env::var_os("SG_DEBUG").is_some() || std::env::var_os("SG_MOVIE_TRACE").is_some() {
                eprintln!(
                    "[SG_DEBUG][MOV] PLAY file={} pos=({}, {}) size={}x{} wait={} key_skip={} total_ms={:?} path={}",
                    name, x, y, w, h, wait, key_skip, total_ms, info.path.display()
                );
            }
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
            if let Some(id) = ctx.globals.mov.audio_id.take() {
                ctx.movie.stop_audio(id);
            }
            ctx.globals.mov.stop();
            ctx.movie.stop();
            Ok(true)
        }
        _ => {
            store_or_push_mov_prop(ctx, op, args);
            Ok(true)
        }
    }
}
