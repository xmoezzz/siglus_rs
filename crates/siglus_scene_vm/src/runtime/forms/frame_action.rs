use anyhow::Result;

use crate::runtime::forms::prop_access;
use crate::runtime::globals::{ObjectFrameActionState, PendingFrameActionFinish};
use crate::runtime::{CommandContext, Value};

fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Int(n) => Some(*n),
        Value::NamedArg { value, .. } => as_i64(value),
        _ => None,
    }
}

fn push_ok(ctx: &mut CommandContext, ret_form: Option<i64>) {
    let v = if matches!(ret_form, Some(rf) if rf == prop_access::FM_STR) {
        Value::Str(String::new())
    } else {
        Value::Int(0)
    };
    ctx.push(v);
}

fn queue_finish(
    ctx: &mut CommandContext,
    fa: &ObjectFrameActionState,
    frame_action_chain: Vec<i32>,
) {
    if fa.cmd_name.is_empty() {
        return;
    }
    ctx.globals
        .pending_frame_action_finishes
        .push(PendingFrameActionFinish {
            frame_action_chain,
            object_chain: None,
            scn_name: fa.scn_name.clone(),
            cmd_name: fa.cmd_name.clone(),
            end_time: fa.end_time,
            args: fa.args.clone(),
        });
}

fn apply_set_from_parts(
    fa: &mut ObjectFrameActionState,
    scene_name: &str,
    end_time: i64,
    cmd_name: String,
    args: Vec<Value>,
    real_time_flag: bool,
) {
    fa.end_time = end_time;
    fa.cmd_name = cmd_name;
    fa.scn_name = scene_name.to_string();
    fa.real_time_flag = real_time_flag;
    fa.end_flag = false;
    fa.counter.reset();
    if real_time_flag {
        fa.counter.start_real();
    } else {
        fa.counter.start();
    }
    fa.args = args;
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let Some((chain_pos, chain_ref)) = prop_access::parse_current_element_chain(ctx, args) else {
        return Ok(false);
    };
    let chain: Vec<i32> = chain_ref.to_vec();
    let tail: Vec<i32> = chain[1..].to_vec();
    let script_args: Vec<Value> = prop_access::script_args(args, chain_pos).to_vec();
    let scene_name = ctx.current_scene_name.clone().unwrap_or_default();
    let (al_id, ret_form) = prop_access::current_vm_meta(ctx);
    let rhs = if al_id == Some(1) {
        script_args.first().cloned()
    } else {
        None
    };

    if tail.is_empty() {
        push_ok(ctx, ret_form);
        return Ok(true);
    }

    match tail[0] {
        crate::runtime::constants::elm_value::FRAMEACTION_COUNTER => {
            let counter_tail = &tail[1..];
            if counter_tail.is_empty() {
                let set_v = rhs.as_ref().and_then(as_i64).or_else(|| {
                    if al_id == Some(1) && script_args.len() == 1 {
                        script_args.first().and_then(as_i64)
                    } else {
                        None
                    }
                });
                if let Some(v) = set_v {
                    {
                        let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                        fa.counter.set_count(v);
                    }
                    push_ok(ctx, ret_form);
                } else {
                    let count = {
                        let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                        fa.counter.get_count()
                    };
                    ctx.push(Value::Int(count));
                }
                return Ok(true);
            }

            match counter_tail[0] {
                crate::runtime::constants::COUNTER_SET => {
                    let value = script_args.get(0).and_then(as_i64).unwrap_or(0);
                    {
                        let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                        fa.counter.set_count(value);
                    }
                    push_ok(ctx, ret_form);
                }
                crate::runtime::constants::COUNTER_GET => {
                    let count = {
                        let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                        fa.counter.get_count()
                    };
                    ctx.push(Value::Int(count));
                }
                crate::runtime::constants::COUNTER_RESET => {
                    {
                        let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                        fa.counter.reset();
                    }
                    push_ok(ctx, ret_form);
                }
                crate::runtime::constants::COUNTER_START => {
                    {
                        let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                        fa.counter.start();
                    }
                    push_ok(ctx, ret_form);
                }
                crate::runtime::constants::COUNTER_START_REAL => {
                    {
                        let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                        fa.counter.start_real();
                    }
                    push_ok(ctx, ret_form);
                }
                crate::runtime::constants::COUNTER_START_FRAME => {
                    let from = script_args.get(0).and_then(as_i64).unwrap_or(0);
                    let to = script_args.get(1).and_then(as_i64).unwrap_or(0);
                    let frame_time = script_args.get(2).and_then(as_i64).unwrap_or(0);
                    {
                        let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                        fa.counter.start_frame(from, to, frame_time);
                    }
                    push_ok(ctx, ret_form);
                }
                crate::runtime::constants::COUNTER_START_FRAME_REAL => {
                    let from = script_args.get(0).and_then(as_i64).unwrap_or(0);
                    let to = script_args.get(1).and_then(as_i64).unwrap_or(0);
                    let frame_time = script_args.get(2).and_then(as_i64).unwrap_or(0);
                    {
                        let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                        fa.counter.start_frame_real(from, to, frame_time);
                    }
                    push_ok(ctx, ret_form);
                }
                crate::runtime::constants::COUNTER_START_FRAME_LOOP => {
                    let from = script_args.get(0).and_then(as_i64).unwrap_or(0);
                    let to = script_args.get(1).and_then(as_i64).unwrap_or(0);
                    let frame_time = script_args.get(2).and_then(as_i64).unwrap_or(0);
                    {
                        let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                        fa.counter.start_frame_loop(from, to, frame_time);
                    }
                    push_ok(ctx, ret_form);
                }
                crate::runtime::constants::COUNTER_START_FRAME_LOOP_REAL => {
                    let from = script_args.get(0).and_then(as_i64).unwrap_or(0);
                    let to = script_args.get(1).and_then(as_i64).unwrap_or(0);
                    let frame_time = script_args.get(2).and_then(as_i64).unwrap_or(0);
                    {
                        let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                        fa.counter.start_frame_loop_real(from, to, frame_time);
                    }
                    push_ok(ctx, ret_form);
                }
                crate::runtime::constants::COUNTER_STOP => {
                    {
                        let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                        fa.counter.stop();
                    }
                    push_ok(ctx, ret_form);
                }
                crate::runtime::constants::COUNTER_RESUME => {
                    {
                        let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                        fa.counter.resume();
                    }
                    push_ok(ctx, ret_form);
                }
                crate::runtime::constants::COUNTER_CHECK_VALUE => {
                    let target = script_args.get(0).and_then(as_i64).unwrap_or(0);
                    let count = {
                        let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                        fa.counter.get_count()
                    };
                    ctx.push(Value::Int(if count - target >= 0 { 1 } else { 0 }));
                }
                crate::runtime::constants::COUNTER_CHECK_ACTIVE => {
                    let active = {
                        let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                        fa.counter.is_running()
                    };
                    ctx.push(Value::Int(if active { 1 } else { 0 }));
                }
                crate::runtime::constants::COUNTER_WAIT
                | crate::runtime::constants::COUNTER_WAIT_KEY => return Ok(false),
                _ => return Ok(false),
            }
            Ok(true)
        }
        crate::runtime::constants::elm_value::FRAMEACTION_START => {
            let end_time = script_args.first().and_then(as_i64).unwrap_or(0);
            let cmd_name = script_args
                .get(1)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let fa_args = script_args.iter().skip(2).cloned().collect();
            {
                let fa_snapshot = ctx
                    .globals
                    .frame_actions
                    .entry(form_id)
                    .or_default()
                    .clone();
                queue_finish(ctx, &fa_snapshot, chain.clone());
                let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                apply_set_from_parts(fa, &scene_name, end_time, cmd_name, fa_args, false);
            }
            push_ok(ctx, ret_form);
            Ok(true)
        }
        crate::runtime::constants::elm_value::FRAMEACTION_END => {
            let fa_snapshot = ctx
                .globals
                .frame_actions
                .entry(form_id)
                .or_default()
                .clone();
            queue_finish(ctx, &fa_snapshot, chain.clone());
            let fa = ctx.globals.frame_actions.entry(form_id).or_default();
            *fa = ObjectFrameActionState::default();
            push_ok(ctx, ret_form);
            Ok(true)
        }
        crate::runtime::constants::elm_value::FRAMEACTION_START_REAL => {
            let end_time = script_args.first().and_then(as_i64).unwrap_or(0);
            let cmd_name = script_args
                .get(1)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let fa_args = script_args.iter().skip(2).cloned().collect();
            {
                let fa_snapshot = ctx
                    .globals
                    .frame_actions
                    .entry(form_id)
                    .or_default()
                    .clone();
                queue_finish(ctx, &fa_snapshot, chain.clone());
                let fa = ctx.globals.frame_actions.entry(form_id).or_default();
                apply_set_from_parts(fa, &scene_name, end_time, cmd_name, fa_args, true);
            }
            push_ok(ctx, ret_form);
            Ok(true)
        }
        crate::runtime::constants::elm_value::FRAMEACTION_IS_END_ACTION => {
            let end_flag = ctx
                .globals
                .frame_actions
                .entry(form_id)
                .or_default()
                .end_flag;
            ctx.push(Value::Int(if end_flag { 1 } else { 0 }));
            Ok(true)
        }
        _ => Ok(false),
    }
}
