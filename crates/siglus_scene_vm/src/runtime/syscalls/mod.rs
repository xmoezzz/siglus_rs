//! Syscall dispatch scaffolding.
//!
//! The original Siglus VM exposes a large set of system calls (separate from
//! the "forms" switch). This workspace ports syscalls incrementally.

use anyhow::Result;

use crate::runtime::{CommandContext, Value};

fn default_for_ret_form(ret_form: i32) -> Value {
    // Heuristic: ret_form==2 is string (FM_STR), else int.
    if ret_form == 2 {
        Value::Str(String::new())
    } else {
        Value::Int(0)
    }
}

/// Dispatch a syscall by numeric ID.
///
/// Returns true if the syscall was recognized and handled.
pub fn dispatch_syscall(ctx: &mut CommandContext, syscall_id: u32, _args: &[Value]) -> Result<bool> {
    // The public Siglus source tree does not expose syscall definitions.
    // Treat syscalls as handled no-ops for now so scripts can proceed.
    if _args.len() >= 2 {
        if let (Some(ret_form), Some(Value::Element(_))) = (_args.last().and_then(|v| v.as_i64()), _args.get(_args.len() - 2)) {
            if ret_form != 0 {
                ctx.push(default_for_ret_form(ret_form as i32));
            }
        }
    }
    ctx.unknown.record_note(&format!("syscall.noop:{syscall_id}"));
    Ok(true)
}
