//! Numeric opcode/form/syscall plumbing.
//!
//! Siglus dispatches many operations by numeric codes ("forms" and "syscalls").
//! When porting, a non-trivial subset of codes may be unknown.
//!
//! This module provides:
//! - A stable representation of a numeric operation (OpCode)
//! - A dispatcher hook (dispatch_code)

use anyhow::Result;

use super::{forms, syscalls, CommandContext, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OpKind {
    Syscall,
    Form,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpCode {
    pub kind: OpKind,
    pub id: u32,
}

impl OpCode {
    pub const fn syscall(id: u32) -> Self {
        Self {
            kind: OpKind::Syscall,
            id,
        }
    }

    pub const fn form(id: u32) -> Self {
        Self { kind: OpKind::Form, id }
    }
}

/// Dispatch a numeric operation.
///
/// Returns true if the code was recognized and handled.
///
/// Syscalls are not wired in this step yet; they will be added incrementally.
pub fn dispatch_code(ctx: &mut CommandContext, code: OpCode, args: &[Value]) -> Result<bool> {
    match code.kind {
        OpKind::Form => {
            if let Some(h) = ctx.external_forms.clone() {
                if h.dispatch_form(ctx, code.id, args)? {
                    return Ok(true);
                }
            }
            forms::dispatch_form(ctx, code.id, args)
        }
        OpKind::Syscall => {
            if let Some(h) = ctx.external_syscalls.clone() {
                if h.dispatch_syscall(ctx, code.id, args)? {
                    return Ok(true);
                }
            }
            syscalls::dispatch_syscall(ctx, code.id, args)
        }
    }
}

