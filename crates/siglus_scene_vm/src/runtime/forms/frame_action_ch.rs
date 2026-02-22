use anyhow::Result;

use crate::runtime::{CommandContext, Value};

use super::int_list;

/// FRAME_ACTION_CH global form.
///
/// The original engine uses per-channel frame-action lists.
/// For bring-up, we treat this as an integer list (single backing store).
pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    int_list::dispatch(ctx, form_id, args)
}
