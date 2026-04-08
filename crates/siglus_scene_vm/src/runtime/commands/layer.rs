use anyhow::Result;

use crate::layer::LayerManager;
use crate::runtime::commands::util::strip_vm_meta;
use crate::runtime::{Command, CommandContext, Value};

/// Small bring-up layer commands.
///
/// These exist to help validate the renderer and command plumbing.
pub fn handle(ctx: &mut CommandContext, cmd: &Command) -> Result<bool> {
    let name = cmd.name.to_ascii_uppercase();
    let args = strip_vm_meta(&cmd.args);

    match name.as_str() {
        // Reset both the layer manager and the gfx runtime.
        "LAYER_RESET" | "LAYER_CLEAR" | "CLS" => {
            ctx.layers = LayerManager::new();
            ctx.gfx = crate::runtime::graphics::GfxRuntime::new();
            return Ok(true);
        }
        // Select current layer for subsequent CHR/object operations.
        "LAYER" | "LAYER_SET" | "LAYER_SEL" | "LAYER_SELECT" => {
            if let Some(layer) = args.iter().rev().find_map(|v| match v {
                Value::Int(i) => Some(*i),
                _ => None,
            }) {
                ctx.gfx.current_layer = layer.clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                return Ok(true);
            }
            return Ok(false);
        }
        // Clear a specific layer.
        "LAYER_CLR" => {
            if let Some(layer) = args.iter().rev().find_map(|v| match v {
                Value::Int(i) => Some(*i),
                _ => None,
            }) {
                if layer >= 0 {
                    ctx.layers.clear_layer(layer as usize);
                }
                return Ok(true);
            }
            return Ok(false);
        }
        _ => {}
    }

    if name.starts_with("LAYER") {
        return Ok(true);
    }

    Ok(false)
}
