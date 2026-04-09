use anyhow::Result;

use crate::runtime::commands::util;
use crate::runtime::{Command, CommandContext, Value};

fn clamp_u8(x: i64) -> u8 {
    if x <= 0 {
        return 0;
    }
    if x >= 255 {
        return 255;
    }
    x as u8
}

fn pct_to_raw(pct: i64) -> u8 {
    let p = if pct < 0 {
        0
    } else if pct > 100 {
        100
    } else {
        pct
    };
    // Map 0..100 -> 0..255
    ((p as u32 * 255) / 100) as u8
}

/// Best-effort named audio commands.
///
/// This is bring-up quality: the goal is to let scripts run far enough to
/// validate decoding and VM control flow.
pub fn handle(ctx: &mut CommandContext, cmd: &Command) -> Result<bool> {
    let name = cmd.name.to_ascii_uppercase();
    let args = util::strip_vm_meta(&cmd.args);

    // ---- BGM ----
    if matches!(
        name.as_str(),
        "BGM" | "BGM_PLAY" | "BGMSTART" | "PLAYBGM" | "BGMON"
    ) {
        let file = args.iter().find_map(|v| match v {
            Value::Str(s) => Some(s.as_str()),
            _ => None,
        });
        if let Some(f) = file {
            // Ignore errors in bring-up: record and continue.
            let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
            let _ = bgm.play_name(audio, f);
        }
        return Ok(true);
    }
    if matches!(name.as_str(), "BGM_STOP" | "BGMSTOP" | "STOPBGM" | "BGMOFF") {
        let _ = ctx.bgm.stop();
        return Ok(true);
    }
    if matches!(name.as_str(), "BGM_VOL" | "BGM_VOLUME" | "BGMVOLUME") {
        // Accept either 0..255 or 0..100
        let vol = args.iter().find_map(|v| match v {
            Value::Int(x) => Some(*x),
            _ => None,
        });
        if let Some(v) = vol {
            let raw = if v <= 100 { pct_to_raw(v) } else { clamp_u8(v) };
            let (bgm, audio) = (&mut ctx.bgm, &mut ctx.audio);
            let _ = bgm.set_volume_raw(audio, raw);
        }
        return Ok(true);
    }
    if matches!(name.as_str(), "BGM_PAUSE" | "BGMPAUSE") {
        let _ = ctx.bgm.pause();
        return Ok(true);
    }
    if matches!(name.as_str(), "BGM_RESUME" | "BGMRESUME") {
        let _ = ctx.bgm.resume();
        return Ok(true);
    }

    // ---- SE ----
    if matches!(name.as_str(), "SE" | "SE_PLAY" | "SEPLAY") {
        let file = args.iter().find_map(|v| match v {
            Value::Str(s) => Some(s.as_str()),
            _ => None,
        });
        if let Some(f) = file {
            let (se, audio) = (&mut ctx.se, &mut ctx.audio);
            let _ = se.play_file_name(audio, f);
        }
        return Ok(true);
    }
    if matches!(name.as_str(), "SE_STOP" | "SESTOP") {
        let _ = ctx.se.stop(None);
        return Ok(true);
    }
    if matches!(name.as_str(), "SE_VOL" | "SE_VOLUME" | "SEVOLUME") {
        let vol = args.iter().find_map(|v| match v {
            Value::Int(x) => Some(*x),
            _ => None,
        });
        if let Some(v) = vol {
            let raw = if v <= 100 { pct_to_raw(v) } else { clamp_u8(v) };
            let (se, audio) = (&mut ctx.se, &mut ctx.audio);
            let _ = se.set_volume_raw(audio, raw);
        }
        return Ok(true);
    }

    // ---- PCM ----
    if matches!(name.as_str(), "PCM" | "PCM_PLAY" | "PCMPLAY") {
        let file = args.iter().find_map(|v| match v {
            Value::Str(s) => Some(s.as_str()),
            _ => None,
        });
        if let Some(f) = file {
            let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio);
            let _ = pcm.play_file_name(audio, f);
        }
        return Ok(true);
    }
    if matches!(name.as_str(), "PCM_STOP" | "PCMSTOP") {
        let _ = ctx.pcm.stop(None);
        return Ok(true);
    }

    Ok(false)
}
