use anyhow::Result;

use super::prop_access;
use crate::runtime::constants;
use crate::runtime::globals::MaskListState;
use crate::runtime::int_event::IntEvent;
use crate::runtime::{CommandContext, Value};

fn default_for_ret_form(ret_form: i32) -> Value {
    if prop_access::ret_form_is_string(ret_form as i64) {
        Value::Str(String::new())
    } else {
        Value::Int(0)
    }
}

fn mask_cnt(ctx: &CommandContext) -> usize {
    ctx.tables
        .gameexe
        .as_ref()
        .map(|cfg| cfg.indexed_count("MASK"))
        .unwrap_or(0)
}

fn is_array_code(elm_array: i32, code: i32) -> bool {
    if elm_array < 0 {
        return code != 0;
    }
    code == elm_array
}

fn is_mask_like_chain(ctx: &CommandContext, form_id: u32, chain: &[i32]) -> bool {
    if chain.is_empty() || chain[0] as u32 != form_id {
        return false;
    }
    if chain.len() == 2 {
        return !is_array_code(ctx.ids.elm_array, chain[1]);
    }
    chain.len() >= 3 && is_array_code(ctx.ids.elm_array, chain[1])
}

enum MaskPostAction {
    None,
    Wait(bool),
}

fn dispatch_int_event_exact(
    ev: &mut IntEvent,
    sub_op: i32,
    params: &[Value],
    ret_form: i32,
) -> (Option<Value>, MaskPostAction) {
    match sub_op {
        x if x == constants::elm_value::INTEVENT_SET
            || x == constants::elm_value::INTEVENT_SET_REAL =>
        {
            let value = params.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let total_time = params.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let delay_time = params.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let speed_type = params.get(3).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let real_flag = if x == constants::elm_value::INTEVENT_SET_REAL {
                1
            } else {
                0
            };
            ev.set_event(value, total_time, delay_time, speed_type, real_flag);
            (None, MaskPostAction::None)
        }
        x if x == constants::elm_value::INTEVENT_LOOP
            || x == constants::elm_value::INTEVENT_LOOP_REAL =>
        {
            let start_value = params.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let end_value = params.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let loop_time = params.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let delay_time = params.get(3).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let speed_type = params.get(4).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let real_flag = if x == constants::elm_value::INTEVENT_LOOP_REAL {
                1
            } else {
                0
            };
            ev.loop_event(
                start_value,
                end_value,
                loop_time,
                delay_time,
                speed_type,
                real_flag,
            );
            (None, MaskPostAction::None)
        }
        x if x == constants::elm_value::INTEVENT_TURN
            || x == constants::elm_value::INTEVENT_TURN_REAL =>
        {
            let start_value = params.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let end_value = params.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let loop_time = params.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let delay_time = params.get(3).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let speed_type = params.get(4).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let real_flag = if x == constants::elm_value::INTEVENT_TURN_REAL {
                1
            } else {
                0
            };
            ev.turn_event(
                start_value,
                end_value,
                loop_time,
                delay_time,
                speed_type,
                real_flag,
            );
            (None, MaskPostAction::None)
        }
        x if x == constants::elm_value::INTEVENT_END => {
            ev.end_event();
            (None, MaskPostAction::None)
        }
        x if x == constants::elm_value::INTEVENT_WAIT => (None, MaskPostAction::Wait(false)),
        x if x == constants::elm_value::INTEVENT_WAIT_KEY => (None, MaskPostAction::Wait(true)),
        x if x == constants::elm_value::INTEVENT_CHECK => {
            let v = if ret_form != 0 && ev.check_event() {
                1
            } else {
                0
            };
            (Some(Value::Int(v)), MaskPostAction::None)
        }
        _ => (None, MaskPostAction::None),
    }
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let Some((chain_pos, chain)) =
        crate::runtime::forms::prop_access::parse_element_chain_ctx(ctx, form_id, args)
            .map(|(i, ch)| (i, ch.to_vec()))
    else {
        return Ok(false);
    };
    if !is_mask_like_chain(ctx, form_id, &chain) {
        return Ok(false);
    }

    let params = crate::runtime::forms::prop_access::script_args(args, chain_pos);
    let (meta_al_id, meta_ret_form) = crate::runtime::forms::prop_access::current_vm_meta(ctx);
    let al_id = meta_al_id.unwrap_or(0) as i32;
    let ret_form = meta_ret_form.unwrap_or(0) as i32;
    let elm_array = ctx.ids.elm_array;

    let cnt = mask_cnt(ctx);
    let (handled, ret, post_action): (bool, Option<Value>, MaskPostAction) = 'blk: {
        let ml = ctx
            .globals
            .mask_lists
            .entry(form_id)
            .or_insert_with(|| MaskListState::new(cnt));
        ml.ensure_size(cnt);

        if chain.len() == 2 && !is_array_code(elm_array, chain[1]) {
            if chain[1] == constants::elm_value::MASKLIST_GET_SIZE && ret_form != 0 {
                break 'blk (
                    true,
                    Some(Value::Int(ml.masks.len() as i64)),
                    MaskPostAction::None,
                );
            }
            break 'blk (true, None, MaskPostAction::None);
        }

        if chain.len() < 4 || !is_array_code(elm_array, chain[1]) {
            let r = if ret_form != 0 {
                Some(default_for_ret_form(ret_form))
            } else {
                None
            };
            break 'blk (true, r, MaskPostAction::None);
        }

        let idx = chain.get(2).copied().unwrap_or(0).max(0) as usize;
        let Some(mask) = ml.masks.get_mut(idx) else {
            let r = if ret_form != 0 {
                Some(default_for_ret_form(ret_form))
            } else {
                None
            };
            break 'blk (true, r, MaskPostAction::None);
        };

        let op = chain[3];
        if chain.len() >= 5 {
            let target_ev = match op {
                x if x == constants::elm_value::MASK_X_EVE => &mut mask.x_event,
                x if x == constants::elm_value::MASK_Y_EVE => &mut mask.y_event,
                _ => {
                    break 'blk (true, None, MaskPostAction::None);
                }
            };
            let sub_op = chain[4];
            let (r, action) = dispatch_int_event_exact(target_ev, sub_op, params, ret_form);
            break 'blk (true, r, action);
        }

        match op {
            x if x == constants::elm_value::MASK_INIT => {
                mask.reinit();
                break 'blk (true, None, MaskPostAction::None);
            }
            x if x == constants::elm_value::MASK_CREATE => {
                if let Some(name) = params.first().and_then(|v| v.as_str()) {
                    mask.name = Some(name.to_string());
                } else {
                    mask.name = None;
                }
                break 'blk (true, None, MaskPostAction::None);
            }
            x if x == constants::elm_value::MASK_X => {
                if al_id == 0 {
                    let r = if ret_form != 0 {
                        Some(Value::Int(mask.x_event.get_total_value() as i64))
                    } else {
                        None
                    };
                    break 'blk (true, r, MaskPostAction::None);
                }
                if al_id == 1 {
                    let v = params.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    mask.x_event.set_value(v);
                    mask.x_event.frame();
                    break 'blk (true, None, MaskPostAction::None);
                }
                break 'blk (true, None, MaskPostAction::None);
            }
            x if x == constants::elm_value::MASK_Y => {
                if al_id == 0 {
                    let r = if ret_form != 0 {
                        Some(Value::Int(mask.y_event.get_total_value() as i64))
                    } else {
                        None
                    };
                    break 'blk (true, r, MaskPostAction::None);
                }
                if al_id == 1 {
                    let v = params.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    mask.y_event.set_value(v);
                    mask.y_event.frame();
                    break 'blk (true, None, MaskPostAction::None);
                }
                break 'blk (true, None, MaskPostAction::None);
            }
            _ => {
                let r = if ret_form != 0 {
                    Some(default_for_ret_form(ret_form))
                } else {
                    None
                };
                break 'blk (true, r, MaskPostAction::None);
            }
        }
    };

    if let Some(v) = ret {
        ctx.push(v);
    }
    match post_action {
        MaskPostAction::None => {}
        MaskPostAction::Wait(key_skip) => ctx.wait.wait_generic_int_event(0, None, key_skip),
    }
    Ok(handled)
}
