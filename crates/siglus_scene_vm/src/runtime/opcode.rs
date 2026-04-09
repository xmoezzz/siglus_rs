//! Numeric opcode/form plumbing.
//!
//! Siglus dispatches many operations by numeric form codes.
//! When porting, a non-trivial subset of codes may be unknown.
//!
//! This module provides:
//! - A stable representation of a numeric operation (OpCode)
//! - A dispatcher hook (dispatch_code)

use anyhow::Result;

use super::{forms, CommandContext, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpCode {
    pub id: u32,
}

impl OpCode {
    pub const fn form(id: u32) -> Self {
        Self { id }
    }
}

/// Dispatch a numeric form operation.
///
/// Returns true if the code was recognized and handled.
pub fn dispatch_code(ctx: &mut CommandContext, code: OpCode, args: &[Value]) -> Result<bool> {
    if let Some(h) = ctx.external_forms.clone() {
        if h.dispatch_form(ctx, code.id, args)? {
            return Ok(true);
        }
    }
    forms::dispatch_form(ctx, code.id, args)
}
