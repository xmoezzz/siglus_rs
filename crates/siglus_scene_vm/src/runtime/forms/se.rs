use anyhow::{bail, Result};

use crate::runtime::{CommandContext, Value};

use super::codes::se_op;

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
    if args.is_empty() {
        bail!("SE form expects at least one argument (op id)");
    }

    let op = match args[0] {
        Value::Int(v) => v,
        _ => {
            ctx.unknown.record_unimplemented("SE/invalid-op-type");
            return Ok(true);
        }
    };

    match op {
        se_op::PLAY_BY_FILE_NAME => {
            let name = match arg_str(args, 1) {
                Some(s) => s,
                None => {
                    ctx.unknown.record_unimplemented("SE/PLAY_BY_FILE_NAME/invalid-args");
                    return Ok(true);
                }
            };
			let (se, audio) = (&mut ctx.se, &mut ctx.audio);
			let _ = se.play_file_name(audio, name)?;
            Ok(true)
        }
        se_op::STOP => {
            // The original engine supports an optional fade time.
            let fade = arg_int(args, 1);
            ctx.se.stop(fade)?;
            Ok(true)
        }
        se_op::SET_VOLUME => {
            let vol = match arg_int(args, 1) {
                Some(v) => v.clamp(0, 255) as u8,
                None => {
                    ctx.unknown.record_unimplemented("SE/SET_VOLUME/invalid-args");
                    return Ok(true);
                }
            };
			let (se, audio) = (&mut ctx.se, &mut ctx.audio);
			se.set_volume_raw(audio, vol)?;
            Ok(true)
        }
        se_op::SET_VOLUME_MAX => {
			let (se, audio) = (&mut ctx.se, &mut ctx.audio);
			se.set_volume_raw(audio, 255)?;
            Ok(true)
        }
        se_op::SET_VOLUME_MIN => {
			let (se, audio) = (&mut ctx.se, &mut ctx.audio);
			se.set_volume_raw(audio, 0)?;
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
            ctx.wait.wait_audio(crate::runtime::wait::AudioWait::SeAny, false);
            Ok(true)
        }
        se_op::WAIT_KEY => {
            ctx.wait.wait_audio(crate::runtime::wait::AudioWait::SeAny, true);
            Ok(true)
        }

        se_op::PLAY | se_op::PLAY_BY_SE_NO => {
            let se_no = match arg_int(args, 1) {
                Some(v) => v,
                None => {
                    ctx.unknown.record_unimplemented("SE/PLAY/invalid-args");
                    return Ok(true);
                }
            };
			let (se, audio) = (&mut ctx.se, &mut ctx.audio);
			for cand in resolve_numeric_candidates(se_no) {
				if se.play_file_name(audio, &cand).is_ok() {
                    return Ok(true);
                }
            }
            ctx.unknown.record_unimplemented(&format!("SE/PLAY/se_no={se_no}"));
            Ok(true)
        }

        se_op::PLAY_BY_KOE_NO => {
            let koe_no = match arg_int(args, 1) {
                Some(v) => v,
                None => {
                    ctx.unknown.record_unimplemented("SE/PLAY_BY_KOE_NO/invalid-args");
                    return Ok(true);
                }
            };
            // KOE is usually in a different directory. We approximate by trying common sub-dirs.
            // If your title separates KOE and SE, provide a different IdMap route later.
			let (se, audio) = (&mut ctx.se, &mut ctx.audio);
			for cand in resolve_numeric_candidates(koe_no) {
				if se.play_file_name(audio, &cand).is_ok() {
                    return Ok(true);
                }
            }
            ctx.unknown.record_unimplemented(&format!("SE/PLAY_BY_KOE_NO/koe_no={koe_no}"));
            Ok(true)
        }

        _ => {
            ctx.unknown.record_unimplemented(&format!("SE/op={op}"));
            Ok(true)
        }
    }
}
