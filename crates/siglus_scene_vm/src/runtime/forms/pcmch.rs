use anyhow::{bail, Result};

use crate::runtime::{CommandContext, Value};

use super::codes;

fn arg_int(args: &[Value], idx: usize) -> Option<i64> {
    match args.get(idx) {
        Some(Value::Int(v)) => Some(*v),
        _ => None,
    }
}

fn arg_str<'a>(args: &'a [Value], idx: usize) -> Option<&'a str> {
    match args.get(idx) {
        Some(Value::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn parse_channel_from_chain(ctx: &CommandContext, chain: &[i32]) -> Option<(usize, i64)> {
    // Expected bring-up shapes:
    // - [FORM, ARRAY, ch, op]
    // - [FORM, ARRAY, ch, ..., op] (we use the last element as op)
    if chain.len() < 4 {
        return None;
    }
    if chain[0] as u32 != ctx.ids.form_global_pcmch {
        return None;
    }
	// Prefer the configured ELM_ARRAY marker. If unmapped (negative), accept any non-zero marker.
	let elm_array = ctx.ids.elm_array;
	if elm_array >= 0 {
		if chain[1] != elm_array {
			return None;
		}
	} else if chain[1] == 0 {
		return None;
	}
    let ch = chain[2];
    if ch < 0 {
        return None;
    }
    let op = *chain.last()? as i64;
    Some((ch as usize, op))
}

fn resolve_numeric_candidates(n: i64) -> Vec<String> {
    if n < 0 {
        return vec![n.to_string()];
    }
    vec![
        format!("{:05}", n),
        format!("{:04}", n),
        format!("{:03}", n),
        n.to_string(),
    ]
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    if args.is_empty() {
        bail!("PCMCH form expects arguments");
    }

    // Prefer element-chain decoding when available, since PCMCH is usually a list:
    //   PCMCH_LIST[ch].OP(...)
    let mut ch: usize = 0;
    let mut op: Option<i64> = None;

    // Standard exec_command layout: [op_id, ...args, Element(chain), al_id, ret_form]
    // For list-style PCMCH, op_id is often ARRAY_INDEX (-1), so we decode from the chain.
    let mut real_args: &[Value] = &args[1..];
    if args.len() >= 3 {
        if let (Some(Value::Element(chain)), Some(Value::Int(_)), Some(Value::Int(_))) = (
            args.get(args.len() - 3),
            args.get(args.len() - 2),
            args.get(args.len() - 1),
        ) {
            if let Some((c, o)) = parse_channel_from_chain(ctx, chain) {
                ch = c;
                op = Some(o);
                real_args = &args[1..args.len() - 3];
            }
        }
    }

    let op = op.unwrap_or_else(|| match args[0] {
        Value::Int(v) => v,
        _ => -9_999_999,
    });
    if op == -9_999_999 {
        ctx.unknown.record_unimplemented("PCMCH/invalid-op-type");
        return Ok(true);
    }
    dispatch_inner(ctx, ch, op, real_args)
}

fn dispatch_inner(ctx: &mut CommandContext, ch: usize, op: i64, args: &[Value]) -> Result<bool> {
    match op {
        codes::pcmch_op::PLAY | codes::pcmch_op::PLAY_WAIT | codes::pcmch_op::READY => {
            let name = match arg_str(args, 0) {
                Some(s) => s,
                None => {
                    ctx.unknown.record_unimplemented("PCMCH/PLAY/invalid-args");
                    return Ok(true);
                }
            };
            {
                let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio);
                let _ = pcm.play_in_slot(audio, ch, name, false)?;
            }
            if op == codes::pcmch_op::PLAY_WAIT {
                ctx.wait
                    .wait_audio(crate::runtime::wait::AudioWait::PcmSlot(ch as u8), false);
            }
            Ok(true)
        }
        codes::pcmch_op::PLAY_LOOP => {
            let name = match arg_str(args, 0) {
                Some(s) => s,
                None => {
                    ctx.unknown.record_unimplemented("PCMCH/PLAY_LOOP/invalid-args");
                    return Ok(true);
                }
            };
            {
                let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio);
                let _ = pcm.play_in_slot(audio, ch, name, true)?;
            }
            Ok(true)
        }
        // Numeric registration IDs (SE/KOE-style). We approximate by trying common padding widths.
        // This matches the bring-up goal: keep the VM running and make audio audible.
        codes::se_op::PLAY_BY_SE_NO => {
            let se_no = match arg_int(args, 0) {
                Some(v) => v,
                None => {
                    ctx.unknown.record_unimplemented("PCMCH/PLAY_BY_SE_NO/invalid-args");
                    return Ok(true);
                }
            };
            for cand in resolve_numeric_candidates(se_no) {
                let ok = {
                    let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio);
                    pcm.play_in_slot(audio, ch, &cand, false).is_ok()
                };
                if ok {
                    return Ok(true);
                }
            }

            ctx.unknown.record_unimplemented(&format!("PCMCH/PLAY_BY_SE_NO/se_no={se_no}"));
            Ok(true)
        }
        codes::se_op::PLAY_BY_KOE_NO => {
            let koe_no = match arg_int(args, 0) {
                Some(v) => v,
                None => {
                    ctx.unknown.record_unimplemented("PCMCH/PLAY_BY_KOE_NO/invalid-args");
                    return Ok(true);
                }
            };
            for cand in resolve_numeric_candidates(koe_no) {
                let ok = {
                    let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio);
                    pcm.play_in_slot(audio, ch, &cand, false).is_ok()
                };
                if ok {
                    return Ok(true);
                }
            }

            ctx.unknown.record_unimplemented(&format!("PCMCH/PLAY_BY_KOE_NO/koe_no={koe_no}"));
            Ok(true)
        }

        codes::pcmch_op::STOP => {
            let fade = arg_int(args, 0);
            ctx.pcm.stop_slot(ch, fade)?;
            Ok(true)
        }
        codes::pcmch_op::PAUSE => {
            // Bring-up: treat as stop with no fade.
            ctx.pcm.stop_slot(ch, None)?;
            Ok(true)
        }
        codes::pcmch_op::RESUME | codes::pcmch_op::RESUME_WAIT => {
            // Bring-up: no persistent pause state, so resume is a no-op.
            if op == codes::pcmch_op::RESUME_WAIT {
                ctx.wait
                    .wait_audio(crate::runtime::wait::AudioWait::PcmSlot(ch as u8), false);
            }
            Ok(true)
        }
        codes::pcmch_op::WAIT => {
            ctx.wait
                .wait_audio(crate::runtime::wait::AudioWait::PcmSlot(ch as u8), false);
            Ok(true)
        }
        codes::pcmch_op::WAIT_KEY => {
            ctx.wait
                .wait_audio(crate::runtime::wait::AudioWait::PcmSlot(ch as u8), true);
            Ok(true)
        }
        codes::pcmch_op::WAIT_FADE | codes::pcmch_op::WAIT_FADE_KEY => {
            // Fade state is not modeled yet; treat as WAIT.
            let key = op == codes::pcmch_op::WAIT_FADE_KEY;
            ctx.wait
                .wait_audio(crate::runtime::wait::AudioWait::PcmSlot(ch as u8), key);
            Ok(true)
        }
        codes::pcmch_op::CHECK => {
            let playing = ctx.pcm.is_playing_slot(ch);
            ctx.push(Value::Int(if playing { 1 } else { 0 }));
            Ok(true)
        }
        codes::pcmch_op::SET_VOLUME => {
            let vol = match arg_int(args, 0) {
                Some(v) => v.clamp(0, 255) as u8,
                None => {
                    ctx.unknown.record_unimplemented("PCMCH/SET_VOLUME/invalid-args");
                    return Ok(true);
                }
            };
            // Global volume for all PCMCH channels in bring-up.
            { let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio); pcm.set_volume_raw(audio, vol)?; }
            Ok(true)
        }
        codes::pcmch_op::SET_VOLUME_MAX => {
            { let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio); pcm.set_volume_raw(audio, 255)?; }
            Ok(true)
        }
        codes::pcmch_op::SET_VOLUME_MIN => {
            { let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio); pcm.set_volume_raw(audio, 0)?; }
            Ok(true)
        }
        codes::pcmch_op::GET_VOLUME => {
            let v = ctx.pcm.volume_raw() as i64;
            ctx.push(Value::Int(v));
            Ok(true)
        }
        _ => {
            ctx.unknown.record_unimplemented(&format!("PCMCH/op={op}"));
            Ok(true)
        }
    }
}
