use anyhow::{bail, Result};

use crate::runtime::{CommandContext, Value};

use super::codes::bgm_op;

fn store_or_push_bgm_prop(ctx: &mut CommandContext, op: i32, args: &[Value]) {
    let form_key = if ctx.ids.form_global_bgm != 0 {
        ctx.ids.form_global_bgm
    } else {
        super::codes::FORM_GLOBAL_BGM
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
    let ret_form = crate::runtime::forms::prop_access::current_vm_meta(ctx).1;
    let Some(op) = crate::runtime::forms::prop_access::current_op_from_ctx_or_args(ctx, args)
    else {
        bail!("BGM form expects an element opcode");
    };
    let args = crate::runtime::forms::prop_access::params_without_op(ctx, args);

    let play_name = || named_str(args, 0).or_else(|| arg_str(args, 0));

    match op {
        bgm_op::PLAY
        | bgm_op::PLAY_WAIT
        | bgm_op::READY
        | bgm_op::PLAY_ONESHOT
        | bgm_op::READY_ONESHOT => {
            let name = match play_name() {
                Some(s) => s,
                None => {
                    store_or_push_bgm_prop(ctx, op, args);
                    return Ok(true);
                }
            };

            let mut loop_flag = true;
            let mut wait_flag = false;
            let mut fade_in_time = 0i64;
            let mut fade_out_time = 0i64;
            let mut start_pos_sample = -1i64;
            let mut ready_only = false;

            match op {
                bgm_op::PLAY => {
                    fade_in_time = arg_int(args, 1).unwrap_or(0);
                    fade_out_time = arg_int(args, 2).unwrap_or(0);
                }
                bgm_op::PLAY_ONESHOT => {
                    loop_flag = false;
                    fade_in_time = arg_int(args, 1).unwrap_or(0);
                    fade_out_time = arg_int(args, 2).unwrap_or(0);
                }
                bgm_op::PLAY_WAIT => {
                    loop_flag = false;
                    wait_flag = true;
                    fade_in_time = arg_int(args, 1).unwrap_or(0);
                    fade_out_time = arg_int(args, 2).unwrap_or(0);
                }
                bgm_op::READY => {
                    ready_only = true;
                    fade_out_time = arg_int(args, 1).unwrap_or(100);
                }
                bgm_op::READY_ONESHOT => {
                    loop_flag = false;
                    ready_only = true;
                    fade_in_time = arg_int(args, 1).unwrap_or(0);
                    fade_out_time = arg_int(args, 2).unwrap_or(0);
                }
                _ => {}
            }

            if let Some(v) = named_int(args, 1) {
                loop_flag = v != 0;
            }
            if let Some(v) = named_int(args, 2) {
                wait_flag = v != 0;
            }
            if let Some(v) = named_int(args, 3) {
                start_pos_sample = v;
            }
            if let Some(v) = named_int(args, 4) {
                fade_in_time = v;
            }
            if let Some(v) = named_int(args, 5) {
                fade_out_time = v;
            }

            if wait_flag {
                loop_flag = false;
            }

            let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
            bgm.play_name_script(
                audio,
                name,
                loop_flag,
                fade_in_time,
                fade_out_time,
                start_pos_sample,
                ready_only,
                0,
            )?;
            if let Some(cur) = ctx.bgm.current_name().map(|s| s.to_string()) {
                let _ = super::bgm_table::mark_listened_by_name(ctx, &cur, true);
            }
            if wait_flag {
                ctx.wait
                    .wait_audio(crate::runtime::wait::AudioWait::Bgm, false);
            }
            Ok(true)
        }
        bgm_op::STOP => {
            let fade_time = arg_int(args, 0).unwrap_or(100);
            ctx.bgm.stop_fade(fade_time)?;
            Ok(true)
        }
        bgm_op::PAUSE => {
            let fade_time = arg_int(args, 0).unwrap_or(100);
            let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
            bgm.pause_fade(audio, fade_time)?;
            Ok(true)
        }
        bgm_op::RESUME | bgm_op::RESUME_WAIT => {
            let fade_time = arg_int(args, 0).unwrap_or(0);
            let delay_time = if op == bgm_op::RESUME {
                named_int(args, 0).unwrap_or(0)
            } else {
                0
            };
            let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
            bgm.resume_script(audio, fade_time, delay_time)?;
            if op == bgm_op::RESUME_WAIT {
                ctx.wait
                    .wait_audio(crate::runtime::wait::AudioWait::Bgm, false);
            }
            Ok(true)
        }
        bgm_op::WAIT | bgm_op::WAIT_KEY => {
            ctx.wait.wait_audio_with_return(
                crate::runtime::wait::AudioWait::Bgm,
                op == bgm_op::WAIT_KEY,
                ret_form.unwrap_or(0) != 0,
            );
            Ok(true)
        }
        bgm_op::WAIT_FADE | bgm_op::WAIT_FADE_KEY => {
            ctx.wait.wait_audio_with_return(
                crate::runtime::wait::AudioWait::BgmFade,
                op == bgm_op::WAIT_FADE_KEY,
                ret_form.unwrap_or(0) != 0,
            );
            Ok(true)
        }
        bgm_op::SET_VOLUME => {
            let vol_raw = match arg_int(args, 0) {
                Some(v) => v.clamp(0, 255) as u8,
                None => {
                    store_or_push_bgm_prop(ctx, op, args);
                    return Ok(true);
                }
            };
            let fade_time = arg_int(args, 0).unwrap_or(0);
            let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
            bgm.set_volume_raw_fade(audio, vol_raw, fade_time)?;
            Ok(true)
        }
        bgm_op::SET_VOLUME_MAX => {
            let fade_time = arg_int(args, 0).unwrap_or(0);
            let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
            bgm.set_volume_raw_fade(audio, 255, fade_time)?;
            Ok(true)
        }
        bgm_op::SET_VOLUME_MIN => {
            let fade_time = arg_int(args, 0).unwrap_or(0);
            let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
            bgm.set_volume_raw_fade(audio, 0, fade_time)?;
            Ok(true)
        }
        bgm_op::GET_VOLUME => {
            ctx.push(Value::Int(ctx.bgm.volume_raw() as i64));
            Ok(true)
        }
        bgm_op::GET_REGIST_NAME => {
            ctx.push(Value::Str(ctx.bgm.current_name().unwrap_or("").to_string()));
            Ok(true)
        }
        bgm_op::CHECK => {
            ctx.push(Value::Int(ctx.bgm.check_state() as i64));
            Ok(true)
        }
        bgm_op::GET_PLAY_POS => {
            ctx.push(Value::Int(ctx.bgm.play_pos_samples() as i64));
            Ok(true)
        }
        _ => {
            store_or_push_bgm_prop(ctx, op, args);
            Ok(true)
        }
    }
}
