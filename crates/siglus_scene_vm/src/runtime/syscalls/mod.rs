//! Syscall dispatch scaffolding.
//!
//! The original Siglus VM exposes a large set of system calls (separate from
//! the "forms" switch). This workspace ports syscalls incrementally.

use anyhow::Result;

use crate::runtime::{CommandContext, Value};

/// Dispatch a syscall by numeric ID.
///
/// Returns true if the syscall was recognized and handled.
pub fn dispatch_syscall(_ctx: &mut CommandContext, _syscall_id: u32, _args: &[Value]) -> Result<bool> {
    // Not wired yet.
    Ok(false)
}
