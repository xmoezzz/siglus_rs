//! Global PCMEVENT form.
//!
//! The original engine exposes PCM playback events to the VM (e.g. queued
//! notifications for channels). Full fidelity requires tight integration with
//! the audio backend and per-title tables.
//!
//! For runtime we provide a stable, non-blocking implementation:
//! - All known/unknown ops are accepted.
//! - Query-like ops return 0.
//! - Mutating ops return 0.
//!
//! This avoids runtime crashes and allows titles that *optionally* touch PCMEVENT
//! to keep running.

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
    if chain[0] != ctx.ids.form_global_pcm_event as i32 {
        return None;
    }
    if chain.len() >= 2 {
        return Some(chain[1] as i64);
    }
    Some(0)
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    // If called without an op id / recognizable chain, treat as not ours.
    let Some(op) = parse_op(ctx, args) else {
        return Ok(false);
    };

    // Heuristic mapping (very conservative):
    // 0 -> clear/reset, 1 -> query, 2 -> query, 3 -> next-frame.
    // All return 0 in runtime.
    match op {
        0 | 1 | 2 | 3 => {
            ctx.push(Value::Int(0));
            Ok(true)
        }
        _ => {
            ctx.push(Value::Int(0));
            Ok(true)
        }
    }
}
