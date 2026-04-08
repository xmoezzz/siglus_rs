use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::audio::bgm::decode_bgm_to_wav_bytes;
use crate::runtime::{CommandContext, Value};

use super::codes;

fn store_or_push_pcmch_prop(ctx: &mut CommandContext, ch: usize, op: i64, args: &[Value]) {
    let form_key = if ctx.ids.form_global_pcmch != 0 {
        ctx.ids.form_global_pcmch
    } else {
        super::codes::FORM_GLOBAL_PCMCH
    };
    let prop = (((ch as i32) & 0x7fff) << 16) ^ ((op as i32) & 0xffff);
    if let Some(v) = args.get(0).cloned() {
        match v {
            Value::Str(s) => {
                ctx.globals.str_props.entry(form_key).or_default().insert(prop, s);
            }
            Value::Int(n) => {
                ctx.globals.int_props.entry(form_key).or_default().insert(prop, n);
            }
            _ => {}
        }
        ctx.push(Value::Int(0));
        return;
    }
    if let Some(s) = ctx.globals.str_props.get(&form_key).and_then(|m| m.get(&prop)).cloned() {
        ctx.push(Value::Str(s));
        return;
    }
    let v = ctx.globals.int_props.get(&form_key).and_then(|m| m.get(&prop).copied()).unwrap_or(0);
    ctx.push(Value::Int(v));
}

fn arg_str<'a>(args: &'a [Value], idx: usize) -> Option<&'a str> {
    args.get(idx).and_then(|v| v.as_str())
}

fn arg_int(args: &[Value], idx: usize) -> Option<i64> {
    args.get(idx).and_then(|v| v.as_i64())
}

fn named_str<'a>(args: &'a [Value], id: i32) -> Option<&'a str> {
    args.iter().find_map(|v| match v {
        Value::NamedArg { id: nid, value } if *nid == id => value.as_str(),
        _ => None,
    })
}

fn named_int(args: &[Value], id: i32) -> Option<i64> {
    args.iter().find_map(|v| match v {
        Value::NamedArg { id: nid, value } if *nid == id => value.as_i64(),
        _ => None,
    })
}

fn parse_channel_from_chain(ctx: &CommandContext, chain: &[i32]) -> Option<(usize, i64)> {
    if chain.len() < 4 {
        return None;
    }
    if chain[0] as u32 != ctx.ids.form_global_pcmch {
        return None;
    }
    let elm_array = ctx.ids.elm_array;
    if elm_array >= 0 {
        if chain[1] != elm_array {
            return None;
        }
    } else if chain[1] == 0 {
        return None;
    }
    let ch = chain[2];
    if ch < 0 {
        return None;
    }
    let op = *chain.last()? as i64;
    Some((ch as usize, op))
}

fn resolve_numeric_candidates(n: i64) -> Vec<String> {
    if n < 0 {
        return vec![n.to_string()];
    }
    vec![
        format!("{:05}", n),
        format!("{:04}", n),
        format!("{:03}", n),
        n.to_string(),
    ]
}

fn resolve_subdir_path(project_dir: &Path, subdir: &str, file_name: &str) -> Option<PathBuf> {
    let direct = Path::new(file_name);
    if direct.exists() {
        return Some(direct.to_path_buf());
    }

    let dir = project_dir.join(subdir);
    let base = dir.join(file_name);
    if base.extension().is_some() && base.exists() {
        return Some(base);
    }

    for ext in ["wav", "nwa", "ogg", "owp", "ovk"] {
        let p = base.with_extension(ext);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn play_path_on_pcm_slot(
    ctx: &mut CommandContext,
    ch: usize,
    display_name: &str,
    path: &Path,
    loop_flag: bool,
) -> Result<()> {
    let decoded = decode_bgm_to_wav_bytes(path, None)
        .with_context(|| format!("decode audio: {}", path.display()))?;
    let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio);
    pcm.play_decoded_wav_in_slot(audio, ch, display_name, decoded.wav_bytes, loop_flag)
}

fn play_named_source(
    ctx: &mut CommandContext,
    ch: usize,
    pcm_name: Option<&str>,
    bgm_name: Option<&str>,
    koe_no: Option<i64>,
    se_no: Option<i64>,
    loop_flag: bool,
) -> Result<bool> {
    if let Some(name) = pcm_name.filter(|s| !s.is_empty()) {
        let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio);
        pcm.play_in_slot(audio, ch, name, loop_flag)?;
        return Ok(true);
    }

    if let Some(name) = bgm_name.filter(|s| !s.is_empty()) {
        if let Some(path) = resolve_subdir_path(&ctx.project_dir, "bgm", name) {
            play_path_on_pcm_slot(ctx, ch, &format!("bgm:{name}"), &path, loop_flag)?;
            return Ok(true);
        }
    }

    if let Some(no) = koe_no {
        let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio);
        pcm.play_koe_no_in_slot(audio, ch, no, loop_flag)?;
        return Ok(true);
    }

    if let Some(no) = se_no {
        for cand in resolve_numeric_candidates(no) {
            if let Some(path) = resolve_subdir_path(&ctx.project_dir, "se", &cand) {
                play_path_on_pcm_slot(ctx, ch, &format!("se:{cand}"), &path, loop_flag)?;
                return Ok(true);
            }
        }
    }

    Ok(false)
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    if args.is_empty() {
        bail!("PCMCH form expects arguments");
    }

    let mut ch: usize = 0;
    let mut op: Option<i64> = None;
    let mut real_args: &[Value] = &args[1..];
    let mut ret_form: Option<i64> = None;

    if args.len() >= 3 {
        if let (Some(Value::Element(chain)), Some(Value::Int(_)), Some(Value::Int(_))) = (
            args.get(args.len() - 3),
            args.get(args.len() - 2),
            args.get(args.len() - 1),
        ) {
            if let Some((c, o)) = parse_channel_from_chain(ctx, chain) {
                ch = c;
                op = Some(o);
                real_args = &args[1..args.len() - 3];
                ret_form = args.get(args.len() - 1).and_then(|v| v.as_i64());
            }
        }
    }

    let op = op.unwrap_or_else(|| match args[0] {
        Value::Int(v) => v,
        _ => -9_999_999,
    });
    if op == -9_999_999 {
        ctx.push(Value::Int(0));
        return Ok(true);
    }
    dispatch_inner(ctx, ch, op, real_args, ret_form)
}

fn dispatch_inner(ctx: &mut CommandContext, ch: usize, op: i64, args: &[Value], ret_form: Option<i64>) -> Result<bool> {
    match op {
        codes::pcmch_op::PLAY
        | codes::pcmch_op::PLAY_LOOP
        | codes::pcmch_op::PLAY_WAIT
        | codes::pcmch_op::READY => {
            let default_loop = op == codes::pcmch_op::PLAY_LOOP;
            let loop_flag = named_int(args, 0).map(|v| v != 0).unwrap_or(default_loop);
            let wait_flag = named_int(args, 1).map(|v| v != 0).unwrap_or(op == codes::pcmch_op::PLAY_WAIT);
            let _fade_in_time = named_int(args, 2).or_else(|| arg_int(args, 1)).unwrap_or(0);
            let pcm_name = named_str(args, 7).or_else(|| arg_str(args, 0));
            let koe_no = named_int(args, 8);
            let se_no = named_int(args, 9);
            let bgm_name = named_str(args, 10);

            let played = play_named_source(ctx, ch, pcm_name, bgm_name, koe_no, se_no, loop_flag)?;
            if !played {
                store_or_push_pcmch_prop(ctx, ch, op, args);
                return Ok(true);
            }

            if wait_flag && !loop_flag {
                ctx.wait
                    .wait_audio(crate::runtime::wait::AudioWait::PcmSlot(ch as u8), false);
            }
            Ok(true)
        }

        codes::se_op::PLAY_BY_SE_NO => {
            let se_no = match arg_int(args, 0) {
                Some(v) => v,
                None => {
                    store_or_push_pcmch_prop(ctx, ch, op, args);
                    return Ok(true);
                }
            };
            for cand in resolve_numeric_candidates(se_no) {
                if let Some(path) = resolve_subdir_path(&ctx.project_dir, "se", &cand) {
                    play_path_on_pcm_slot(ctx, ch, &format!("se:{cand}"), &path, false)?;
                    return Ok(true);
                }
            }

            store_or_push_pcmch_prop(ctx, ch, op, args);
            Ok(true)
        }
        codes::se_op::PLAY_BY_KOE_NO => {
            let koe_no = match arg_int(args, 0) {
                Some(v) => v,
                None => {
                    store_or_push_pcmch_prop(ctx, ch, op, args);
                    return Ok(true);
                }
            };
            let ok = {
                let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio);
                pcm.play_koe_no_in_slot(audio, ch, koe_no, false).is_ok()
            };
            if ok {
                return Ok(true);
            }

            store_or_push_pcmch_prop(ctx, ch, op, args);
            Ok(true)
        }

        codes::pcmch_op::STOP => {
            let fade = arg_int(args, 0);
            ctx.pcm.stop_slot(ch, fade)?;
            Ok(true)
        }
        codes::pcmch_op::PAUSE => {
            ctx.pcm.stop_slot(ch, arg_int(args, 0))?;
            Ok(true)
        }
        codes::pcmch_op::RESUME | codes::pcmch_op::RESUME_WAIT => {
            let _fade_time = arg_int(args, 0);
            let _delay_time = named_int(args, 0);
            if op == codes::pcmch_op::RESUME_WAIT {
                ctx.wait
                    .wait_audio(crate::runtime::wait::AudioWait::PcmSlot(ch as u8), false);
            }
            Ok(true)
        }
        codes::pcmch_op::WAIT => {
            ctx.wait
                .wait_audio(crate::runtime::wait::AudioWait::PcmSlot(ch as u8), false);
            if ret_form.unwrap_or(0) != 0 {
                ctx.push(Value::Int(0));
            }
            Ok(true)
        }
        codes::pcmch_op::WAIT_KEY => {
            ctx.wait
                .wait_audio(crate::runtime::wait::AudioWait::PcmSlot(ch as u8), true);
            if ret_form.unwrap_or(0) != 0 {
                ctx.push(Value::Int(0));
            }
            Ok(true)
        }
        codes::pcmch_op::WAIT_FADE | codes::pcmch_op::WAIT_FADE_KEY => {
            let key = op == codes::pcmch_op::WAIT_FADE_KEY;
            ctx.wait
                .wait_audio(crate::runtime::wait::AudioWait::PcmSlot(ch as u8), key);
            if ret_form.unwrap_or(0) != 0 {
                ctx.push(Value::Int(0));
            }
            Ok(true)
        }
        codes::pcmch_op::CHECK => {
            let playing = ctx.pcm.is_playing_slot(ch);
            ctx.push(Value::Int(if playing { 1 } else { 0 }));
            Ok(true)
        }
        codes::pcmch_op::SET_VOLUME => {
            let vol = match arg_int(args, 0) {
                Some(v) => v.clamp(0, 255) as u8,
                None => {
                    store_or_push_pcmch_prop(ctx, ch, op, args);
                    return Ok(true);
                }
            };
            let fade_time = arg_int(args, 1).unwrap_or(0);
            let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio);
            pcm.set_volume_raw_fade(audio, vol, fade_time)?;
            Ok(true)
        }
        codes::pcmch_op::SET_VOLUME_MAX => {
            let fade_time = arg_int(args, 0).unwrap_or(0);
            let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio);
            pcm.set_volume_raw_fade(audio, 255, fade_time)?;
            Ok(true)
        }
        codes::pcmch_op::SET_VOLUME_MIN => {
            let fade_time = arg_int(args, 0).unwrap_or(0);
            let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio);
            pcm.set_volume_raw_fade(audio, 0, fade_time)?;
            Ok(true)
        }
        codes::pcmch_op::GET_VOLUME => {
            let v = ctx.pcm.volume_raw() as i64;
            ctx.push(Value::Int(v));
            Ok(true)
        }
        _ => {
            store_or_push_pcmch_prop(ctx, ch, op, args);
            Ok(true)
        }
    }
}
