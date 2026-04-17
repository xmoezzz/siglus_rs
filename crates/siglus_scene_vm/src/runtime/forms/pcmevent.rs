//! Global PCMEVENT form.
//!
//! Public C++ source routes `GLOBAL.PCMEVENT` through a list form and then an
//! indexed `PCMEVENT[i]` item with explicit commands:
//!   START_ONESHOT / START_LOOP / START_RANDOM / STOP / CHECK / WAIT / WAIT_KEY
//!
//! This Rust handler mirrors that structure directly instead of treating PCMEVENT
//! as a generic global op bucket.

use anyhow::Result;

use crate::runtime::globals::{PcmEventLine, PcmEventState};
use crate::runtime::wait::AudioWait;
use crate::runtime::{CommandContext, Value};

enum PcmEventOp {
    StartOneShot,
    StartLoop,
    StartRandom,
    Stop,
    Check,
    Wait,
    WaitKey,
    Unknown,
}

fn resolve_pcm_event_op(op: i32) -> PcmEventOp {
    match op {
        crate::runtime::constants::PCMEVENT_START_ONESHOT => PcmEventOp::StartOneShot,
        crate::runtime::constants::PCMEVENT_START_LOOP => PcmEventOp::StartLoop,
        crate::runtime::constants::PCMEVENT_START_RANDOM => PcmEventOp::StartRandom,
        crate::runtime::constants::PCMEVENT_STOP => PcmEventOp::Stop,
        crate::runtime::constants::PCMEVENT_CHECK => PcmEventOp::Check,
        crate::runtime::constants::PCMEVENT_WAIT_KEY => PcmEventOp::WaitKey,
        crate::runtime::constants::PCMEVENT_WAIT => PcmEventOp::Wait,
        _ => PcmEventOp::Unknown,
    }
}

fn named_int(args: &[Value], id: i32) -> Option<i64> {
    args.iter().find_map(|v| match v {
        Value::NamedArg { id: nid, value } if *nid == id => value.as_i64(),
        _ => None,
    })
}

fn collect_lines(args: &[Value], random: bool) -> Vec<PcmEventLine> {
    let mut out = Vec::new();
    for v in args {
        match v {
            Value::Str(s) => out.push(PcmEventLine {
                file_name: s.clone(),
                probability: if random { 1 } else { 0 },
                min_time: 0,
                max_time: 0,
            }),
            Value::List(items) if !items.is_empty() => {
                let file_name = items
                    .first()
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if file_name.is_empty() {
                    continue;
                }
                let mut line = PcmEventLine {
                    file_name,
                    probability: if random { 1 } else { 0 },
                    min_time: 0,
                    max_time: 0,
                };
                if random {
                    if let Some(v) = items.get(1).and_then(Value::as_i64) {
                        line.probability = v as i32;
                    }
                    if let Some(v) = items.get(2).and_then(Value::as_i64) {
                        line.min_time = v as i32;
                        line.max_time = v as i32;
                    }
                    if let Some(v) = items.get(3).and_then(Value::as_i64) {
                        line.max_time = v as i32;
                    }
                } else {
                    if let Some(v) = items.get(1).and_then(Value::as_i64) {
                        line.min_time = v as i32;
                        line.max_time = v as i32;
                    }
                    if let Some(v) = items.get(2).and_then(Value::as_i64) {
                        line.max_time = v as i32;
                    }
                }
                out.push(line);
            }
            _ => {}
        }
    }
    out
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let form_global_pcm_event = ctx.ids.form_global_pcm_event;
    let elm_array = ctx.ids.elm_array;
    let Some((chain_pos, chain)) = crate::runtime::forms::prop_access::parse_element_chain_ctx(
        ctx,
        form_global_pcm_event,
        args,
    ) else {
        return Ok(false);
    };
    let chain = chain.to_vec();
    if chain.len() < 3 || chain[1] != elm_array {
        return Ok(false);
    }
    let idx = chain[2].max(0) as usize;

    {
        let list = ctx
            .globals
            .pcm_event_lists
            .entry(form_global_pcm_event)
            .or_insert_with(Vec::new);
        if list.len() <= idx {
            list.resize(idx + 1, PcmEventState::default());
        }
    }

    // Public C++ list/item structure: bare [ARRAY, idx] returns the element itself.
    if chain.len() == 3 {
        return Ok(true);
    }

    let op = resolve_pcm_event_op(chain[3]);
    let script_args = if chain_pos == args.len() {
        crate::runtime::forms::prop_access::script_args(args, chain_pos)
    } else {
        &args[..chain_pos]
    };
    match op {
        PcmEventOp::StartOneShot | PcmEventOp::StartLoop | PcmEventOp::StartRandom => {
            let random = matches!(op, PcmEventOp::StartRandom);
            let looped = matches!(op, PcmEventOp::StartLoop);
            let lines = collect_lines(script_args, random);
            let active = if let Some(line) = lines.first() {
                let (pcm, audio) = (&mut ctx.pcm, &mut ctx.audio);
                pcm.play_in_slot(audio, idx, &line.file_name, looped)
                    .is_ok()
            } else {
                false
            };
            if let Some(st) = ctx
                .globals
                .pcm_event_lists
                .get_mut(&form_global_pcm_event)
                .and_then(|v| v.get_mut(idx))
            {
                st.reinit();
                st.random = random;
                st.looped = looped;
                st.volume_type = named_int(script_args, 3).unwrap_or(0) as i32;
                st.bgm_fade_target_flag = named_int(script_args, 4).unwrap_or(0) != 0;
                st.bgm_fade2_target_flag = named_int(script_args, 5).unwrap_or(0) != 0;
                st.chara_no = named_int(script_args, 6).unwrap_or(-1) as i32;
                st.time_type = named_int(script_args, 11).unwrap_or(0) != 0;
                st.bgm_fade2_source_flag = named_int(script_args, 12).unwrap_or(0) != 0;
                st.real_flag = true;
                st.lines = lines;
                st.active = active;
            }
            ctx.push(Value::Int(0));
            Ok(true)
        }
        PcmEventOp::Stop => {
            let fade = script_args.first().and_then(Value::as_i64).unwrap_or(0);
            let _ = ctx.pcm.stop_slot(idx, Some(fade));
            if let Some(st) = ctx
                .globals
                .pcm_event_lists
                .get_mut(&form_global_pcm_event)
                .and_then(|v| v.get_mut(idx))
            {
                st.active = false;
            }
            ctx.push(Value::Int(0));
            Ok(true)
        }
        PcmEventOp::Check => {
            let playing = ctx.pcm.is_playing_slot(idx);
            if let Some(st) = ctx
                .globals
                .pcm_event_lists
                .get_mut(&form_global_pcm_event)
                .and_then(|v| v.get_mut(idx))
            {
                st.active = playing;
            }
            ctx.push(Value::Int(if playing { 1 } else { 0 }));
            Ok(true)
        }
        PcmEventOp::Wait => {
            ctx.wait.wait_audio(AudioWait::PcmSlot(idx as u8), false);
            Ok(true)
        }
        PcmEventOp::WaitKey => {
            ctx.wait.wait_audio(AudioWait::PcmSlot(idx as u8), true);
            Ok(true)
        }
        PcmEventOp::Unknown => Ok(false),
    }
}
