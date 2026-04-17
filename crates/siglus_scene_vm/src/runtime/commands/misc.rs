use anyhow::Result;

use crate::runtime::commands::util;
use crate::runtime::forms::syscom as syscom_form;
use crate::runtime::globals::WipeState;
use crate::runtime::{Command, CommandContext, Value};
use std::path::{Path, PathBuf};

fn is_noop_cmd(name: &str) -> bool {
    matches!(name, "NOP" | "VIBRATE")
}

fn is_clear_cmd(name: &str) -> bool {
    matches!(
        name,
        "CLS" | "CLEAR" | "CLEARALL" | "ALL_CLEAR" | "ALLCLEAR" | "RESET"
    )
}

/// Catch-all helpers for script commands handled as no-ops.
pub fn handle(ctx: &mut CommandContext, cmd: &Command) -> Result<bool> {
    let name = cmd.name.to_ascii_uppercase();
    let args = util::strip_vm_meta(&cmd.args);

    let mut pos: Vec<&Value> = Vec::new();
    let mut named: Vec<(i32, &Value)> = Vec::new();
    for a in args {
        if let Value::NamedArg { id, value } = a {
            named.push((*id, value.as_ref()));
        } else {
            pos.push(a);
        }
    }

    let parse_i32 = |v: &Value| -> Option<i32> { v.as_i64().and_then(|x| i32::try_from(x).ok()) };

    let parse_bool = |v: &Value| -> Option<bool> { parse_i32(v).map(|x| x != 0) };

    let parse_list_i32 = |v: &Value| -> Vec<i32> {
        match v {
            Value::List(xs) => xs
                .iter()
                .filter_map(|x| x.as_i64().and_then(|n| i32::try_from(n).ok()))
                .collect(),
            _ => Vec::new(),
        }
    };

    // WAIT family: block VM execution.
    match name.as_str() {
        // ------------------------------------------------------------------
        // WIPE family
        // ------------------------------------------------------------------
        "WIPE" | "WIPE_ALL" | "MASK_WIPE" | "MASK_WIPE_ALL" => {
            let is_mask = matches!(name.as_str(), "MASK_WIPE" | "MASK_WIPE_ALL");
            let is_all = matches!(name.as_str(), "WIPE_ALL" | "MASK_WIPE_ALL");

            let mut mask_file: Option<String> = None;
            let mut wipe_type: i32 = 0;
            let mut wipe_time: i32 = 500;
            let mut speed_mode: i32 = 0;
            let mut start_time: i32 = 0;
            let mut option: Vec<i32> = Vec::new();

            let mut begin_order: i32 = 0;
            let mut end_order: i32 = if is_all { i32::MAX } else { 0 };
            let mut begin_layer: i32 = i32::MIN;
            let mut end_layer: i32 = i32::MAX;
            let mut wait_flag: bool = true;
            let mut key_wait_mode: i32 = -1;
            let mut with_low_order: i32 = 0;

            if is_mask {
                mask_file = pos.get(0).and_then(|v| v.as_str()).map(|s| s.to_string());
                if let Some(v) = pos.get(1).and_then(|v| parse_i32(v)) {
                    wipe_type = v;
                }
                if let Some(v) = pos.get(2).and_then(|v| parse_i32(v)) {
                    wipe_time = v;
                }
                if let Some(v) = pos.get(3).and_then(|v| parse_i32(v)) {
                    speed_mode = v;
                }
                if let Some(v) = pos.get(4) {
                    option = parse_list_i32(v);
                }
            } else {
                if let Some(v) = pos.get(0).and_then(|v| parse_i32(v)) {
                    wipe_type = v;
                }
                if let Some(v) = pos.get(1).and_then(|v| parse_i32(v)) {
                    wipe_time = v;
                }
                if let Some(v) = pos.get(2).and_then(|v| parse_i32(v)) {
                    speed_mode = v;
                }
                if let Some(v) = pos.get(3) {
                    option = parse_list_i32(v);
                }
            }

            // Named args override positional args.
            for &(id, v) in &named {
                match id {
                    0 => {
                        if let Some(x) = parse_i32(v) {
                            wipe_type = x;
                        }
                    }
                    1 => {
                        if let Some(x) = parse_i32(v) {
                            wipe_time = x;
                        }
                    }
                    2 => {
                        if let Some(x) = parse_i32(v) {
                            speed_mode = x;
                        }
                    }
                    3 => {
                        option = parse_list_i32(v);
                    }
                    4 => {
                        if let Some(x) = parse_i32(v) {
                            begin_order = x;
                        }
                    }
                    5 => {
                        if let Some(x) = parse_i32(v) {
                            end_order = x;
                        }
                    }
                    6 => {
                        if let Some(x) = parse_i32(v) {
                            begin_layer = x;
                        }
                    }
                    7 => {
                        if let Some(x) = parse_i32(v) {
                            end_layer = x;
                        }
                    }
                    8 => {
                        if let Some(x) = parse_bool(v) {
                            wait_flag = x;
                        }
                    }
                    9 => {
                        if let Some(x) = parse_i32(v) {
                            key_wait_mode = x;
                        }
                    }
                    10 => {
                        if let Some(x) = parse_i32(v) {
                            with_low_order = x;
                        }
                    }
                    11 => {
                        if let Some(x) = parse_i32(v) {
                            start_time = x;
                        }
                    }
                    _ => {}
                }
            }

            if is_all {
                end_order = i32::MAX;
            }

            let mask_image_id = if let Some(ref f) = mask_file {
                resolve_mask_path(&ctx.project_dir, f)
                    .and_then(|p| ctx.images.load_file(&p, 0).ok())
            } else {
                None
            };
            ctx.globals.start_wipe(WipeState::new(
                mask_file,
                mask_image_id,
                wipe_type,
                wipe_time,
                start_time,
                speed_mode,
                option,
                begin_order,
                end_order,
                begin_layer,
                end_layer,
                wait_flag,
                key_wait_mode,
                with_low_order,
            ));

            if wait_flag {
                let key_skip = match key_wait_mode {
                    0 => false,
                    1 => true,
                    _ => {
                        ctx.globals
                            .syscom
                            .config_int
                            .get(&197)
                            .copied()
                            .unwrap_or(0)
                            != 0
                    }
                };
                ctx.wait.wait_wipe(key_skip);
            }
            return Ok(true);
        }
        "WAIT_WIPE" | "WAITWIPE" => {
            let mut key_wait_mode: i32 = -1;
            for &(id, v) in &named {
                if id == 0 {
                    if let Some(x) = parse_i32(v) {
                        key_wait_mode = x;
                    }
                }
            }
            let key_skip = match key_wait_mode {
                0 => false,
                1 => true,
                _ => {
                    ctx.globals
                        .syscom
                        .config_int
                        .get(&197)
                        .copied()
                        .unwrap_or(0)
                        != 0
                }
            };
            ctx.wait.wait_wipe(key_skip);
            return Ok(true);
        }

        "WAIT" | "SLEEP" => {
            // Convention: WAIT(ms)
            let ms = args
                .iter()
                .rev()
                .find_map(|v| match v {
                    Value::Int(x) => u64::try_from(*x).ok(),
                    _ => None,
                })
                .unwrap_or(0);
            if ms > 0 {
                ctx.wait.wait_ms(ms);
            }
            return Ok(true);
        }
        "WAITKEY" | "WAIT_KEY" | "WAIT_KEYDOWN" | "WAITCLICK" | "WAIT_CLICK" => {
            // Click and key share the same runtime path here.
            ctx.wait.wait_key();
            return Ok(true);
        }
        // Transition-ish commands: we can't animate yet, but we should honor timing.
        "FADE" | "FADEIN" | "FADE_OUT" | "FADEOUT" | "TRANS" | "TRANSITION" | "DISSOLVE"
        | "CROSSFADE" => {
            let ms = args
                .iter()
                .rev()
                .find_map(|v| match v {
                    Value::Int(x) => u64::try_from(*x).ok(),
                    _ => None,
                })
                .unwrap_or(0);
            if ms > 0 {
                ctx.wait.wait_ms(ms);
            }
            return Ok(true);
        }
        _ => {}
    }

    match name.as_str() {
        "PAUSE" | "YIELD" => {
            if let Some(ms) = pos
                .first()
                .and_then(|v| parse_i32(v))
                .map(|v| v.max(0) as u64)
            {
                if ms > 0 {
                    ctx.wait.wait_ms(ms);
                } else {
                    ctx.wait.wait_key();
                }
            } else {
                ctx.wait.wait_key();
            }
            return Ok(true);
        }
        "AUTO" | "AUTO_ON" => {
            ctx.globals.script.auto_mode_flag = true;
            ctx.globals.script.skip_trigger = false;
            return Ok(true);
        }
        "AUTO_OFF" => {
            ctx.globals.script.auto_mode_flag = false;
            return Ok(true);
        }
        "SKIP" | "SKIP_ON" => {
            if !ctx.globals.script.skip_disable {
                ctx.globals.script.skip_trigger = true;
                ctx.globals.script.auto_mode_flag = false;
            }
            return Ok(true);
        }
        "SKIP_OFF" => {
            ctx.globals.script.skip_trigger = false;
            return Ok(true);
        }
        "SOUND" | "SOUND_ON" => {
            for key in [212, 213, 214, 215, 216, 217, 218] {
                ctx.globals.syscom.config_int.insert(key, 1);
            }
            syscom_form::apply_audio_config(ctx);
            return Ok(true);
        }
        "SOUND_OFF" => {
            for key in [212, 213, 214, 215, 216, 217, 218] {
                ctx.globals.syscom.config_int.insert(key, 0);
            }
            syscom_form::apply_audio_config(ctx);
            return Ok(true);
        }
        "LOG" | "PRINT" | "DEBUG" | "TRACE" => {
            let mut parts: Vec<String> = Vec::new();
            for v in &pos {
                parts.push(match v {
                    Value::Int(x) => x.to_string(),
                    Value::Str(s) => s.clone(),
                    Value::List(xs) => format!("{:?}", xs),
                    _ => String::new(),
                });
            }
            if parts.is_empty() {
                parts.push(name.clone());
            }
            ctx.globals.system.debug_logs.push(parts.join(" "));
            return Ok(true);
        }
        _ => {}
    }

    if is_noop_cmd(name.as_str()) {
        return Ok(true);
    }

    if is_clear_cmd(name.as_str()) {
        // Best-effort: clear render state.
        ctx.layers.clear_all();
        ctx.gfx = crate::runtime::graphics::GfxRuntime::new();
        return Ok(true);
    }

    Ok(false)
}

fn resolve_mask_path(project_dir: &Path, raw: &str) -> Option<PathBuf> {
    if raw.is_empty() {
        return None;
    }
    let norm = raw.replace('\\', "/");
    let p = Path::new(&norm);
    if p.is_absolute() && p.is_file() {
        return Some(p.to_path_buf());
    }
    let mut candidates = Vec::new();
    candidates.push(project_dir.join(&norm));
    candidates.push(project_dir.join("dat").join(&norm));
    if p.extension().is_none() {
        for ext in ["png", "bmp", "jpg"] {
            candidates.push(project_dir.join(format!("{}.{}", norm, ext)));
            candidates.push(project_dir.join("dat").join(format!("{}.{}", norm, ext)));
        }
    }
    for c in candidates {
        if c.is_file() {
            return Some(c);
        }
    }
    None
}
