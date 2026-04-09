use anyhow::Result;

use crate::runtime::commands::util::strip_vm_meta;
use crate::runtime::{Command, CommandContext, Value};

fn arg_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Int(i) => Some(*i),
        _ => None,
    }
}

fn arg_str(v: &Value) -> Option<&str> {
    match v {
        Value::Str(s) => Some(s.as_str()),
        _ => None,
    }
}

fn last_i64(args: &[Value]) -> Option<i64> {
    args.iter().rev().find_map(arg_i64)
}

fn first_two_i64(args: &[Value]) -> Option<(i64, i64)> {
    let mut it = args.iter().filter_map(arg_i64);
    let a = it.next()?;
    let b = it.next()?;
    Some((a, b))
}

fn parse_indices(args: &[Value]) -> (i64, i64, usize) {
    // Heuristic:
    //   [stage, obj, ...] or [obj, ...] or [...]
    if args.len() >= 2 {
        if let (Some(stage), Some(obj)) = (arg_i64(&args[0]), arg_i64(&args[1])) {
            return (stage, obj, 2);
        }
    }
    if args.len() >= 1 {
        if let Some(obj) = arg_i64(&args[0]) {
            return (1, obj, 1);
        }
    }
    (1, 0, 0)
}

fn is_chr_cmd(name: &str) -> bool {
    let n = name.to_ascii_uppercase();
    n == "CHR" || n.starts_with("CHR_") || n == "CHAR" || n.starts_with("CHAR_")
}

fn subcmd(name: &str) -> String {
    let n = name.to_ascii_uppercase();
    if n == "CHR" || n == "CHAR" {
        return "CREATE".to_string();
    }
    if let Some(rest) = n.strip_prefix("CHR_") {
        return rest.to_string();
    }
    if let Some(rest) = n.strip_prefix("CHAR_") {
        return rest.to_string();
    }
    "CREATE".to_string()
}

pub fn handle(ctx: &mut CommandContext, cmd: &Command) -> Result<bool> {
    if !is_chr_cmd(&cmd.name) {
        return Ok(false);
    }

    let sub = subcmd(&cmd.name);
    let args = strip_vm_meta(&cmd.args);
    let (stage_idx, obj_idx, mut i) = parse_indices(args);

    match sub.as_str() {
        "CREATE" | "SET" => {
            // Heuristic ordering: file, disp?, x?, y?, pat?, layer?, order?, alpha?
            let mut file: Option<&str> = None;
            if let Some(v) = args.get(i) {
                file = arg_str(v);
                if file.is_some() {
                    i += 1;
                }
            }
            if file.is_none() {
                for (j, v) in args.iter().enumerate().skip(i) {
                    if let Some(s) = arg_str(v) {
                        file = Some(s);
                        i = j + 1;
                        break;
                    }
                }
            }
            let Some(file_name) = file else {
                // Nothing we can do.
                return Ok(true);
            };

            let disp = args.get(i).and_then(arg_i64).unwrap_or(1);
            if args.get(i).and_then(arg_i64).is_some() {
                i += 1;
            }

            let x = args.get(i).and_then(arg_i64).unwrap_or(0);
            if args.get(i).and_then(arg_i64).is_some() {
                i += 1;
            }

            let y = args.get(i).and_then(arg_i64).unwrap_or(0);
            if args.get(i).and_then(arg_i64).is_some() {
                i += 1;
            }

            let pat_no = args.get(i).and_then(arg_i64).unwrap_or(0);
            if args.get(i).and_then(arg_i64).is_some() {
                i += 1;
            }

            {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                gfx.object_create(
                    images, layers, stage_idx, obj_idx, file_name, disp, x, y, pat_no,
                )?;
            }

            // Remaining integers can be interpreted as optional (layer_no, order, alpha).
            let remain: Vec<i64> = args.iter().skip(i).filter_map(arg_i64).collect();
            if remain.len() >= 1 {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                gfx.object_set_layer(images, layers, stage_idx, obj_idx, remain[0])?;
            }
            if remain.len() >= 2 {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                gfx.object_set_order(images, layers, stage_idx, obj_idx, remain[1])?;
            }
            if remain.len() >= 3 {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                gfx.object_set_alpha(images, layers, stage_idx, obj_idx, remain[2])?;
            }

            Ok(true)
        }
        "POS" | "MOVE" => {
            if let Some((x, y)) = first_two_i64(&args[i..]) {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                gfx.object_set_pos(images, layers, stage_idx, obj_idx, x, y)?;
            }
            Ok(true)
        }
        "X" => {
            if let Some(x) = last_i64(&args[i..]) {
                let (_, y) = {
                    let gfx = &ctx.gfx;
                    gfx.object_get_pos(stage_idx, obj_idx).unwrap_or((0, 0))
                };
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                gfx.object_set_pos(images, layers, stage_idx, obj_idx, x, y)?;
            }
            Ok(true)
        }
        "Y" => {
            if let Some(y) = last_i64(&args[i..]) {
                let (x, _) = {
                    let gfx = &ctx.gfx;
                    gfx.object_get_pos(stage_idx, obj_idx).unwrap_or((0, 0))
                };
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                gfx.object_set_pos(images, layers, stage_idx, obj_idx, x, y)?;
            }
            Ok(true)
        }
        "DISP" | "SHOW" | "HIDE" => {
            let disp = match sub.as_str() {
                "SHOW" => 1,
                "HIDE" => 0,
                _ => last_i64(&args[i..]).unwrap_or(1),
            };
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            gfx.object_set_disp(images, layers, stage_idx, obj_idx, disp)?;
            Ok(true)
        }
        "PAT" | "PATNO" | "FRAME" => {
            if let Some(pat_no) = last_i64(&args[i..]) {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                gfx.object_set_patno(images, layers, stage_idx, obj_idx, pat_no)?;
            }
            Ok(true)
        }
        "LAYER" => {
            if let Some(layer_no) = last_i64(&args[i..]) {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                gfx.object_set_layer(images, layers, stage_idx, obj_idx, layer_no)?;
            }
            Ok(true)
        }
        "ORDER" => {
            if let Some(order) = last_i64(&args[i..]) {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                gfx.object_set_order(images, layers, stage_idx, obj_idx, order)?;
            }
            Ok(true)
        }
        "Z" => {
            if let Some(z) = last_i64(&args[i..]) {
                let _ = ctx.gfx.object_set_z(stage_idx, obj_idx, z);
            }
            Ok(true)
        }
        "ALPHA" | "A" => {
            if let Some(a) = last_i64(&args[i..]) {
                let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
                gfx.object_set_alpha(images, layers, stage_idx, obj_idx, a)?;
            }
            Ok(true)
        }
        "CLEAR" | "DEL" | "DELETE" => {
            let (gfx, images, layers) = (&mut ctx.gfx, &mut ctx.images, &mut ctx.layers);
            gfx.object_clear(images, layers, stage_idx, obj_idx)?;
            Ok(true)
        }
        _ => {
            // Unknown CHR_XXX subcommand: ignore to keep VM progressing.
            Ok(true)
        }
    }
}
