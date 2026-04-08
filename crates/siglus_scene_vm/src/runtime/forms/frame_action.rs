use anyhow::Result;

use crate::runtime::{CommandContext, Value};

use super::int_list;

/// FRAME_ACTION global form.
///
/// The original engine implements a complex frame-action subsystem.
/// For runtime, we model it as a generic integer list so scripts can read/write
/// action slots without panicking.
pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    int_list::dispatch(ctx, form_id, args)
}
