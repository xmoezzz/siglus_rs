//! Global KOE_ST form.
//!
//! The public C++ implementation routes `GLOBAL.KOE_ST` to `tnm_command_proc_koe`,
//! and that command handler is a stub: it only returns the element itself when the
//! chain stops at `KOE_ST`, and otherwise treats the operation as unsupported.
//!
//! Keep the Rust port aligned to that public source surface. Voice playback flows
//! through other script/UI paths, not through `GLOBAL.KOE_ST` commands here.

use anyhow::Result;

use crate::runtime::{CommandContext, Value};

fn find_chain<'a>(ctx: &'a CommandContext, _args: &'a [Value]) -> Option<&'a [i32]> {
    let vm_call = ctx.vm_call.as_ref()?;
    Some(vm_call.element.as_slice())
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let Some(chain) = find_chain(ctx, args) else {
        return Ok(false);
    };
    if chain.is_empty() || chain[0] != ctx.ids.form_global_koe_st as i32 {
        return Ok(false);
    }

    // Public C++ source: element reference only. Any actual sub-op is unsupported.
    if chain.len() == 1 {
        return Ok(true);
    }

    Ok(false)
}
