use anyhow::{bail, Result};

use crate::runtime::{CommandContext, Value};

use super::codes::bgm_op;

fn store_or_push_bgm_prop(ctx: &mut CommandContext, op: i64, args: &[Value]) {
    let form_key = if ctx.ids.form_global_bgm != 0 { ctx.ids.form_global_bgm } else { super::codes::FORM_GLOBAL_BGM };
    let prop = op as i32;
    if let Some(v) = args.get(1).cloned() {
        match v {
            Value::Str(s) => { ctx.globals.str_props.entry(form_key).or_default().insert(prop, s); }
            Value::Int(n) => { ctx.globals.int_props.entry(form_key).or_default().insert(prop, n); }
            _ => {}
        }
        ctx.push(Value::Int(0));
        return;
    }
    if let Some(s) = ctx.globals.str_props.get(&form_key).and_then(|m| m.get(&prop)).cloned() {
        ctx.push(Value::Str(s));
        return;
    }
    let v = ctx.globals.int_props.get(&form_key).and_then(|m| m.get(&prop).copied()).unwrap_or(0);
    ctx.push(Value::Int(v));
}

fn trim_args(args: &[Value]) -> (&[Value], Option<i64>) {
    if args.len() >= 3
        && matches!(args[args.len() - 3], Value::Element(_))
        && matches!(args[args.len() - 2], Value::Int(_))
        && matches!(args[args.len() - 1], Value::Int(_))
    {
        let ret_form = args.get(args.len() - 1).and_then(|v| v.as_i64());
        (&args[..args.len() - 3], ret_form)
    } else {
        (args, None)
    }
}

fn arg_str<'a>(args: &'a [Value], idx: usize) -> Option<&'a str> {
    args.get(idx).and_then(|v| v.as_str())
}

fn arg_int(args: &[Value], idx: usize) -> Option<i64> {
    args.get(idx).and_then(|v| v.as_i64())
}

fn named_str<'a>(args: &'a [Value], id: i32) -> Option<&'a str> {
    args.iter().find_map(|v| match v {
        Value::NamedArg { id: nid, value } if *nid == id => value.as_str(),
        _ => None,
    })
}

fn named_int(args: &[Value], id: i32) -> Option<i64> {
    args.iter().find_map(|v| match v {
        Value::NamedArg { id: nid, value } if *nid == id => value.as_i64(),
        _ => None,
    })
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let (args, ret_form) = trim_args(args);
    if args.is_empty() {
        bail!("BGM form expects at least one argument (op id)");
    }

    let op = match args[0] {
        Value::Int(v) => v,
        _ => {
            ctx.push(Value::Int(0));
            return Ok(true);
        }
    };

    let play_name = || named_str(args, 0).or_else(|| arg_str(args, 1));

    match op {
        bgm_op::PLAY | bgm_op::PLAY_WAIT | bgm_op::READY | bgm_op::PLAY_ONESHOT | bgm_op::READY_ONESHOT => {
            let name = match play_name() {
                Some(s) => s,
                None => {
                    store_or_push_bgm_prop(ctx, op, args);
                    return Ok(true);
                }
            };

            let default_loop = !matches!(op, bgm_op::PLAY_ONESHOT | bgm_op::READY_ONESHOT | bgm_op::PLAY_WAIT);
            let loop_flag = named_int(args, 1).map(|v| v != 0).unwrap_or(default_loop);
            let wait_flag = named_int(args, 2).map(|v| v != 0).unwrap_or(op == bgm_op::PLAY_WAIT);
            let _start_pos = named_int(args, 3).or_else(|| arg_int(args, 2));
            let _fade_in_time = named_int(args, 4).or_else(|| arg_int(args, 2));
            let _fade_out_time = named_int(args, 5).or_else(|| arg_int(args, 3));

            ctx.bgm.set_looping(loop_flag)?;
            {
                let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
                bgm.play_name(audio, name)?;
            }

            if wait_flag && ctx.bgm.can_wait() {
                ctx.wait
                    .wait_audio(crate::runtime::wait::AudioWait::Bgm, false);
            }
            Ok(true)
        }
        bgm_op::STOP => {
            let fade_time = arg_int(args, 1).unwrap_or(0);
            ctx.bgm.stop_fade(fade_time)?;
            Ok(true)
        }
        bgm_op::PAUSE => {
            let _fade_time = arg_int(args, 1);
            ctx.bgm.pause()?;
            Ok(true)
        }
        bgm_op::RESUME | bgm_op::RESUME_WAIT => {
            let _fade_time = arg_int(args, 1);
            let _delay_time = named_int(args, 0);
            ctx.bgm.resume()?;
            if op == bgm_op::RESUME_WAIT && ctx.bgm.can_wait() {
                ctx.wait
                    .wait_audio(crate::runtime::wait::AudioWait::Bgm, false);
            }
            Ok(true)
        }
        bgm_op::WAIT
        | bgm_op::WAIT_KEY
        | bgm_op::WAIT_FADE
        | bgm_op::WAIT_FADE_KEY => {
            if ctx.bgm.can_wait() {
                let key = op == bgm_op::WAIT_KEY || op == bgm_op::WAIT_FADE_KEY;
                ctx.wait
                    .wait_audio(crate::runtime::wait::AudioWait::Bgm, key);
            }
            if ret_form.unwrap_or(0) != 0 {
                ctx.push(Value::Int(0));
            }
            Ok(true)
        }
        bgm_op::SET_VOLUME => {
            let vol_raw = match arg_int(args, 1) {
                Some(v) => v.clamp(0, 255) as u8,
                None => {
                    store_or_push_bgm_prop(ctx, op, args);
                    return Ok(true);
                }
            };
            let fade_time = arg_int(args, 2).unwrap_or(0);
            let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
            bgm.set_volume_raw_fade(audio, vol_raw, fade_time)?;
            Ok(true)
        }
        bgm_op::SET_VOLUME_MAX => {
            let fade_time = arg_int(args, 1).unwrap_or(0);
            let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
            bgm.set_volume_raw_fade(audio, 255, fade_time)?;
            Ok(true)
        }
        bgm_op::SET_VOLUME_MIN => {
            let fade_time = arg_int(args, 1).unwrap_or(0);
            let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
            bgm.set_volume_raw_fade(audio, 0, fade_time)?;
            Ok(true)
        }
        bgm_op::GET_VOLUME => {
            ctx.push(Value::Int(ctx.bgm.volume_raw() as i64));
            Ok(true)
        }
        bgm_op::GET_REGIST_NAME => {
            let name = ctx.bgm.current_name().unwrap_or("").to_string();
            ctx.push(Value::Str(name));
            Ok(true)
        }
        bgm_op::CHECK => {
            ctx.push(Value::Int(if ctx.bgm.is_playing() { 1 } else { 0 }));
            Ok(true)
        }
        bgm_op::GET_PLAY_POS => {
            ctx.push(Value::Int(ctx.bgm.play_pos_ms() as i64));
            Ok(true)
        }
        _ => {
            store_or_push_bgm_prop(ctx, op, args);
            Ok(true)
        }
    }
}
