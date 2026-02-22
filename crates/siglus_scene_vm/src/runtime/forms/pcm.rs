use anyhow::{bail, Result};

use crate::runtime::{CommandContext, Value};

use super::codes::pcm_op;

fn arg_str<'a>(args: &'a [Value], idx: usize) -> Option<&'a str> {
    match args.get(idx) {
        Some(Value::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    if args.is_empty() {
        bail!("PCM form expects at least one argument (op id)");
    }

    let op = match args[0] {
        Value::Int(v) => v,
        _ => {
            ctx.unknown.record_unimplemented("PCM/invalid-op-type");
            return Ok(true);
        }
    };

    match op {
        pcm_op::PLAY => {
            let name = match arg_str(args, 1) {
                Some(s) => s,
                None => {
                    ctx.unknown.record_unimplemented("PCM/PLAY/invalid-args");
                    return Ok(true);
                }
            };
			let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio);
			let _ = pcm.play_file_name(audio, name)?;
            Ok(true)
        }
        pcm_op::STOP => {
            ctx.pcm.stop(None)?;
            Ok(true)
        }
        _ => {
            ctx.unknown.record_unimplemented(&format!("PCM/op={op}"));
            Ok(true)
        }
    }
}
