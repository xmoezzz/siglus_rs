use crate::runtime::{CommandContext, Value};
use anyhow::Result;

pub fn dispatch(ctx: &mut CommandContext, allow_key_skip: bool, args: &[Value]) -> Result<bool> {
    let ms = args.get(0).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as u64;
    if ms == 0 {
        return Ok(true);
    }

    if allow_key_skip {
        ctx.wait.wait_ms_key(ms);
    } else {
        ctx.wait.wait_ms(ms);
    }

    Ok(true)
}
