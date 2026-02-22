//! Global KOE_ST form.
//!
//! In the original engine, the KOE/voice subsystem is routed through a global
//! form ID, but in some builds it is intentionally stubbed.
//!
//! For bring-up we implement a conservative subset that never blocks:
//! - op 0: stop
//! - op 1: play by file name (if a string arg is provided)
//! - op 2: check playing
//!
//! Unknown ops are accepted and return 0.

use anyhow::Result;

use crate::runtime::{CommandContext, Value};

fn find_chain(args: &[Value]) -> Option<Vec<i32>> {
    for v in args.iter().rev() {
        if let Value::Element(e) = v {
            return Some(e.clone());
        }
    }
    None
}

fn parse_op(ctx: &CommandContext, args: &[Value]) -> Option<i64> {
    if let Some(Value::Int(v)) = args.get(0) {
        return Some(*v);
    }
    let chain = find_chain(args)?;
    if chain.is_empty() {
        return None;
    }
    if chain[0] != ctx.ids.form_global_koe_st as i32 {
        return None;
    }
    if chain.len() >= 2 {
        return Some(chain[1] as i64);
    }
    Some(0)
}

fn find_name(args: &[Value]) -> Option<&str> {
    args.iter().find_map(|v| match v {
        Value::Str(s) => Some(s.as_str()),
        _ => None,
    })
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let Some(op) = parse_op(ctx, args) else {
        return Ok(false);
    };

    match op {
        // Stop
        0 => {
            let _ = ctx.se.stop(None);
            ctx.push(Value::Int(0));
            Ok(true)
        }
        // Play by file name (best-effort)
        1 => {
            if let Some(name) = find_name(args) {
				let (se, audio) = (&mut ctx.se, &mut ctx.audio);
				let _ = se.play_file_name(audio, name);
            }
            ctx.push(Value::Int(0));
            Ok(true)
        }
        // Check playing
        2 => {
			let playing = ctx.se.is_playing_any();
			ctx.push(Value::Int(if playing { 1 } else { 0 }));
            Ok(true)
        }
        _ => {
            ctx.push(Value::Int(0));
            Ok(true)
        }
    }
}
