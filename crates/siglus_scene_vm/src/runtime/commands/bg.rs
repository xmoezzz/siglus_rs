use anyhow::Result;

use crate::runtime::commands::util::strip_vm_meta;
use crate::runtime::{Command, CommandContext, Value};

fn arg_as_str(args: &[Value], idx: usize) -> Option<&str> {
    match args.get(idx) {
        Some(Value::Str(s)) => Some(s.as_str()),
        _ => None,
    }
}

fn last_i64(args: &[Value]) -> Option<i64> {
    args.iter().rev().find_map(|v| match v {
        Value::Int(i) => Some(*i),
        _ => None,
    })
}

fn collect_i64(args: &[Value]) -> Vec<i64> {
    args.iter()
        .filter_map(|v| match v {
            Value::Int(i) => Some(*i),
            _ => None,
        })
        .collect()
}

pub fn handle(ctx: &mut CommandContext, cmd: &Command) -> Result<bool> {
    let name = cmd.name.to_ascii_uppercase();

    if name == "BG" {
        let args = strip_vm_meta(&cmd.args);
        let bg_name = match arg_as_str(args, 0) {
            Some(s) => s,
            None => return Ok(false),
        };
		let frame_idx = args
			.get(1)
			.and_then(|v| v.as_i64())
			.and_then(|x| usize::try_from(x).ok())
			.unwrap_or(0);
        let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
        gfx.object_create(images, layers, 0, 0, bg_name, 1, 0, 0, frame_idx as i64)?;
        return Ok(true);
    }

    if matches!(name.as_str(), "BG_CLEAR" | "BG_OFF" | "BGCLEAR") {
        // Clear both the gfx object and the dedicated bg sprite.
        {
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            let _ = gfx.object_clear(images, layers, 0, 0);
        }
        ctx.layers.clear_bg();
        return Ok(true);
    }

    if matches!(name.as_str(), "BG_ALPHA" | "BGALPHA" | "BG_A") {
        let args = strip_vm_meta(&cmd.args);
        let alpha_i = match last_i64(args) {
            Some(v) => v,
            None => return Ok(false),
        };
        let alpha = alpha_i.clamp(0, 255);
        let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
        gfx.object_set_alpha(images, layers, 0, 0, alpha)?;
        return Ok(true);
    }

    if matches!(name.as_str(), "BG_X" | "BGX") {
        let args = strip_vm_meta(&cmd.args);
        if let Some(x) = last_i64(args) {
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            gfx.object_set_x(images, layers, 0, 0, x)?;
            return Ok(true);
        }
        return Ok(false);
    }

    if matches!(name.as_str(), "BG_Y" | "BGY") {
        let args = strip_vm_meta(&cmd.args);
        if let Some(y) = last_i64(args) {
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            gfx.object_set_y(images, layers, 0, 0, y)?;
            return Ok(true);
        }
        return Ok(false);
    }

    if matches!(name.as_str(), "BG_POS" | "BGPOS" | "BG_MOVE" | "BGMOVE") {
        // Heuristic: use the last two integer args as (x, y), and if a third
        // integer exists before them, treat it as a duration in ms.
        let args = strip_vm_meta(&cmd.args);
        let ints = collect_i64(args);
        if ints.len() >= 2 {
            let x = ints[ints.len() - 2];
            let y = ints[ints.len() - 1];
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            gfx.object_set_pos(images, layers, 0, 0, x, y)?;
            if ints.len() >= 3 {
                let dur = ints[ints.len() - 3];
                if dur > 0 {
                    ctx.wait.wait_ms(dur as u64);
                }
            }
            return Ok(true);
        }
        return Ok(false);
    }

    if matches!(name.as_str(), "BG_PAT" | "BGPAT" | "BG_FRAME" | "BGFRAME" | "BG_NO" | "BGNO") {
        let args = strip_vm_meta(&cmd.args);
        if let Some(p) = last_i64(args) {
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            gfx.object_set_patno(images, layers, 0, 0, p)?;
            return Ok(true);
        }
        return Ok(false);
    }

    if matches!(name.as_str(), "BG_FADE" | "BGFADE") {
        // Heuristic: last int = duration (ms), previous int = alpha.
        let args = strip_vm_meta(&cmd.args);
        let ints = collect_i64(args);
        if ints.len() >= 2 {
            let alpha = ints[ints.len() - 2].clamp(0, 255);
            let dur = ints[ints.len() - 1];
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            gfx.object_set_alpha(images, layers, 0, 0, alpha)?;
            if dur > 0 {
                ctx.wait.wait_ms(dur as u64);
            }
            return Ok(true);
        }
        return Ok(false);
    }

    // If the script uses additional BG_* commands we haven't wired yet,
    // keep the VM progressing while preserving a trace for later alignment.
    if name.starts_with("BG") {
        ctx.unknown.record_unimplemented(&format!("CMD:{}", name));
        return Ok(true);
    }

    Ok(false)
}
