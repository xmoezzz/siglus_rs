use anyhow::{bail, Result};

use crate::runtime::{CommandContext, Value};

use super::codes::se_op;

fn store_or_push_se_prop(ctx: &mut CommandContext, op: i32, args: &[Value]) {
    let form_key = if ctx.ids.form_global_se != 0 {
        ctx.ids.form_global_se
    } else {
        super::codes::FORM_GLOBAL_SE
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

fn arg_int(args: &[Value], idx: usize) -> Option<i64> {
    match args.get(idx) {
        Some(Value::Int(v)) => Some(*v),
        _ => None,
    }
}

fn resolve_numeric_candidates(n: i64) -> Vec<String> {
    if n < 0 {
        return vec![n.to_string()];
    }
    // Common patterns across titles.
    vec![
        format!("{:05}", n),
        format!("{:04}", n),
        format!("{:03}", n),
        n.to_string(),
    ]
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let ret_form: Option<i64> = crate::runtime::forms::prop_access::current_vm_meta(ctx).1;
    let Some(op) =
        crate::runtime::forms::prop_access::current_op_from_ctx_or_args(ctx, args)
    else {
        bail!("SE form expects an element opcode");
    };
    let args = crate::runtime::forms::prop_access::params_without_op(ctx, args);

    match op {
        se_op::PLAY_BY_FILE_NAME => {
            let name = match arg_str(args, 0) {
                Some(s) => s,
                None => {
                    store_or_push_se_prop(ctx, op, args);
                    return Ok(true);
                }
            };
            let (se, audio) = (&mut ctx.se, &mut ctx.audio);
            let _ = se.play_file_name(audio, name)?;
            Ok(true)
        }
        se_op::STOP => {
            // The original engine supports an optional fade time.
            let fade = arg_int(args, 0);
            ctx.se.stop(fade)?;
            Ok(true)
        }
        se_op::SET_VOLUME => {
            let vol = match arg_int(args, 0) {
                Some(v) => v.clamp(0, 255) as u8,
                None => {
                    store_or_push_se_prop(ctx, op, args);
                    return Ok(true);
                }
            };
            let fade = arg_int(args, 1).unwrap_or(0);
            let (se, audio) = (&mut ctx.se, &mut ctx.audio);
            se.set_volume_raw_fade(audio, vol, fade)?;
            Ok(true)
        }
        se_op::SET_VOLUME_MAX => {
            let fade = arg_int(args, 0).unwrap_or(0);
            let (se, audio) = (&mut ctx.se, &mut ctx.audio);
            se.set_volume_raw_fade(audio, 255, fade)?;
            Ok(true)
        }
        se_op::SET_VOLUME_MIN => {
            let fade = arg_int(args, 0).unwrap_or(0);
            let (se, audio) = (&mut ctx.se, &mut ctx.audio);
            se.set_volume_raw_fade(audio, 0, fade)?;
            Ok(true)
        }
        se_op::GET_VOLUME => {
            ctx.push(Value::Int(ctx.se.volume_raw() as i64));
            Ok(true)
        }
        se_op::CHECK => {
            let playing = ctx.se.is_playing_any();
            ctx.push(Value::Int(if playing { 1 } else { 0 }));
            Ok(true)
        }
        se_op::WAIT => {
            ctx.wait
                .wait_audio(crate::runtime::wait::AudioWait::SeAny, false);
            if ret_form.unwrap_or(0) != 0 {
                ctx.push(Value::Int(0));
            }
            Ok(true)
        }
        se_op::WAIT_KEY => {
            ctx.wait
                .wait_audio(crate::runtime::wait::AudioWait::SeAny, true);
            if ret_form.unwrap_or(0) != 0 {
                ctx.push(Value::Int(0));
            }
            Ok(true)
        }

        se_op::PLAY | se_op::PLAY_BY_SE_NO => {
            let se_no = match arg_int(args, 0) {
                Some(v) => v,
                None => {
                    store_or_push_se_prop(ctx, op, args);
                    return Ok(true);
                }
            };
            let (se, audio) = (&mut ctx.se, &mut ctx.audio);
            for cand in resolve_numeric_candidates(se_no) {
                if se.play_file_name(audio, &cand).is_ok() {
                    return Ok(true);
                }
            }
            store_or_push_se_prop(ctx, op, args);
            Ok(true)
        }

        se_op::PLAY_BY_KOE_NO => {
            let koe_no = match arg_int(args, 0) {
                Some(v) => v,
                None => {
                    store_or_push_se_prop(ctx, op, args);
                    return Ok(true);
                }
            };
            let ok = {
                let (se, audio) = (&mut ctx.se, &mut ctx.audio);
                se.play_koe_no(audio, koe_no).is_ok()
            };
            if ok {
                return Ok(true);
            }
            store_or_push_se_prop(ctx, op, args);
            Ok(true)
        }

        _ => {
            store_or_push_se_prop(ctx, op, args);
            Ok(true)
        }
    }
}
