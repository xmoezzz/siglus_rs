use anyhow::{bail, Result};

use crate::runtime::{CommandContext, Value};

use super::codes::pcm_op;

fn store_or_push_pcm_prop(ctx: &mut CommandContext, op: i32, args: &[Value]) {
    let form_key = if ctx.ids.form_global_pcm != 0 {
        ctx.ids.form_global_pcm
    } else {
        super::codes::FORM_GLOBAL_PCM
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
    match args.get(idx) {
        Some(Value::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let Some(op) = crate::runtime::forms::prop_access::current_op_from_ctx_or_args(ctx, args)
    else {
        bail!("PCM form expects an element opcode");
    };
    let args = crate::runtime::forms::prop_access::params_without_op(ctx, args);

    match op {
        pcm_op::PLAY => {
            let name = match arg_str(args, 0) {
                Some(s) => s,
                None => {
                    store_or_push_pcm_prop(ctx, op, args);
                    return Ok(true);
                }
            };
            let ok = {
                let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio);
                pcm.play_file_name(audio, name).is_ok()
            };
            if !ok {
                ctx.unknown.record_note(&format!("pcm.play.failed:{name}"));
            }
            Ok(true)
        }
        pcm_op::STOP => {
            ctx.pcm.stop(None)?;
            Ok(true)
        }
        _ => {
            store_or_push_pcm_prop(ctx, op, args);
            Ok(true)
        }
    }
}
