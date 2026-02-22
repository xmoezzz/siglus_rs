use anyhow::{bail, Result};

use crate::runtime::{CommandContext, Value};

use super::codes::bgm_op;

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

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    if args.is_empty() {
        bail!("BGM form expects at least one argument (op id)");
    }

    let op = match args[0] {
        Value::Int(v) => v,
        _ => {
            ctx.unknown.record_unimplemented("BGM/invalid-op-type");
            return Ok(true);
        }
    };

    match op {
        bgm_op::PLAY | bgm_op::PLAY_WAIT | bgm_op::READY => {
            let name = match arg_str(args, 1) {
                Some(s) => s,
                None => {
                    ctx.unknown.record_unimplemented("BGM/PLAY/invalid-args");
                    return Ok(true);
                }
            };
            ctx.bgm.set_looping(true)?;
			let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
			bgm.play_name(audio, name)?;
            Ok(true)
        }
        bgm_op::PLAY_ONESHOT | bgm_op::READY_ONESHOT => {
            let name = match arg_str(args, 1) {
                Some(s) => s,
                None => {
                    ctx.unknown.record_unimplemented("BGM/PLAY_ONESHOT/invalid-args");
                    return Ok(true);
                }
            };
            ctx.bgm.set_looping(false)?;
			let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
			bgm.play_name(audio, name)?;
            Ok(true)
        }
        bgm_op::STOP => {
            ctx.bgm.stop()?;
            Ok(true)
        }
        bgm_op::PAUSE => {
            ctx.bgm.pause()?;
            Ok(true)
        }
        bgm_op::RESUME | bgm_op::RESUME_WAIT => {
            // Original supports delay/fade/wait; bring-up resumes immediately.
            let _ = arg_int(args, 1);
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
            // Block only for one-shot BGMs (looping BGMs never finish).
            if ctx.bgm.can_wait() {
                let key = op == bgm_op::WAIT_KEY || op == bgm_op::WAIT_FADE_KEY;
                ctx.wait
                    .wait_audio(crate::runtime::wait::AudioWait::Bgm, key);
            }
            Ok(true)
        }
        bgm_op::SET_VOLUME => {
            let vol_raw = match arg_int(args, 1) {
                Some(v) => v.clamp(0, 255) as u8,
                None => {
                    ctx.unknown.record_unimplemented("BGM/SET_VOLUME/invalid-args");
                    return Ok(true);
                }
            };
			let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
			bgm.set_volume_raw(audio, vol_raw)?;
            Ok(true)
        }
        bgm_op::SET_VOLUME_MAX => {
			let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
			bgm.set_volume_raw(audio, 255)?;
            Ok(true)
        }
        bgm_op::SET_VOLUME_MIN => {
			let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
			bgm.set_volume_raw(audio, 0)?;
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
            // Not implemented (needs media clock).
            ctx.push(Value::Int(0));
            Ok(true)
        }
        _ => {
            // Known ID range but not implemented yet.
            ctx.unknown.record_unimplemented(&format!("BGM/op={op}"));
            Ok(true)
        }
    }
}
