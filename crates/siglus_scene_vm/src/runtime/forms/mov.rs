use anyhow::{bail, Result};

use crate::runtime::{CommandContext, Value};

use super::codes::mov_op;

fn arg_str<'a>(args: &'a [Value], idx: usize) -> Option<&'a str> {
    match args.get(idx) {
        Some(Value::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    if args.is_empty() {
        bail!("MOV form expects at least one argument (op id)");
    }

    let op = match args[0] {
        Value::Int(v) => v,
        _ => {
            ctx.unknown.record_unimplemented("MOV/invalid-op-type");
            return Ok(true);
        }
    };

    match op {
        mov_op::PLAY => {
            let name = match arg_str(args, 1) {
                Some(s) => s,
                None => {
                    ctx.unknown.record_unimplemented("MOV/PLAY/invalid-args");
                    return Ok(true);
                }
            };
            let _info = ctx.movie.play(name, false, false)?;
            Ok(true)
        }
        mov_op::PLAY_WAIT => {
            let name = match arg_str(args, 1) {
                Some(s) => s,
                None => {
                    ctx.unknown.record_unimplemented("MOV/PLAY_WAIT/invalid-args");
                    return Ok(true);
                }
            };
            let _info = ctx.movie.play(name, true, false)?;
            // Bring-up: do not block.
            Ok(true)
        }
        mov_op::PLAY_WAIT_KEY => {
            let name = match arg_str(args, 1) {
                Some(s) => s,
                None => {
                    ctx.unknown
                        .record_unimplemented("MOV/PLAY_WAIT_KEY/invalid-args");
                    return Ok(true);
                }
            };
            let _info = ctx.movie.play(name, true, true)?;
            // Bring-up: do not block.
            Ok(true)
        }
        mov_op::STOP => {
            ctx.movie.stop();
            Ok(true)
        }
        _ => {
            ctx.unknown.record_unimplemented(&format!("MOV/op={op}"));
            Ok(true)
        }
    }
}
