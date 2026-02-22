use anyhow::Result;

use crate::runtime::globals::MaskListState;
use crate::runtime::int_event::IntEvent;
use crate::runtime::{CommandContext, Value};

fn default_for_ret_form(ret_form: i32) -> Value {
    // Bring-up heuristic used by stub: ret_form == 2 is string.
    if ret_form == 2 {
        Value::Str(String::new())
    } else {
        Value::Int(0)
    }
}

fn mask_cnt(ctx: &CommandContext) -> usize {
    ctx.tables
        .gameexe
        .as_ref()
        .and_then(|cfg| cfg.get("MASK.CNT"))
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0)
}

fn parse_element_chain(args: &[Value]) -> Option<(usize, Vec<i32>)> {
    for (i, v) in args.iter().enumerate() {
        if let Value::Element(chain) = v {
            return Some((i, chain.clone()));
        }
    }
    None
}

fn is_array_code(elm_array: i32, code: i32) -> bool {
	// If `elm_array` is not known yet (default -1), accept any non-zero marker.
	if elm_array < 0 {
		return code != 0;
	}
	code == elm_array
}

fn is_mask_like_chain(ctx: &CommandContext, form_id: u32, chain: &[i32]) -> bool {
    if chain.is_empty() || chain[0] as u32 != form_id {
        return false;
    }

    // Mask list ops are either:
    // - [FORM, GET_SIZE]
    // - [FORM, ELM_ARRAY, idx, ...]
    if chain.len() == 2 {
        // GET_SIZE is any non-array selector.
		return !is_array_code(ctx.ids.elm_array, chain[1]);
    }
	chain.len() >= 3 && is_array_code(ctx.ids.elm_array, chain[1])
}

fn confirm_if_probably_mask(ctx: &mut CommandContext, form_id: u32, params: &[Value], chain: &[i32]) {
    if ctx.globals.guessed_mask_form_id.is_some() {
        return;
    }

    // Heuristic: first time we see a mask-like chain with a string parameter,
    // treat this form as MASKLIST.
    let has_string = params.iter().any(|v| v.as_str().is_some());
	if has_string && chain.len() >= 4 && is_array_code(ctx.ids.elm_array, chain[1]) {
        ctx.globals.guessed_mask_form_id = Some(form_id);
        if let Some(ml) = ctx.globals.mask_lists.get_mut(&form_id) {
            ml.confirmed = true;
        }
    }
}

fn dispatch_int_event(ev: &mut IntEvent, params: &[Value], ret_form: i32) -> Option<Value> {
    // Bring-up interpretation (no element-code table):
    // - 4 args: SET-like (value, total_time, delay_time, speed_type)
    // - 5 args: LOOP/TURN-like (start_value, end_value, loop_time, delay_time, speed_type)
    // - 0 args: if returns non-void => CHECK, else END.
    match params.len() {
        0 => {
            if ret_form != 0 {
                let v = if ev.check_event() { 1 } else { 0 };
                return Some(Value::Int(v as i64));
            } else {
                // Treat as END to keep scripts progressing.
                ev.end_event();
            }
        }
        4 => {
            let value = params.get(0).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let total_time = params.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let delay_time = params.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let speed_type = params.get(3).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let real_flag = 0;
            ev.set_event(value, total_time, delay_time, speed_type, real_flag);
        }
        5 => {
            let start_value = params.get(0).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let end_value = params.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let loop_time = params.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let delay_time = params.get(3).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            let speed_type = params.get(4).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
            // Without the element-code table, we cannot distinguish LOOP vs TURN reliably.
            // LOOP is the safer default for bring-up.
            let real_flag = 0;
            ev.loop_event(start_value, end_value, loop_time, delay_time, speed_type, real_flag);
        }
        _ => {}
    }

    None
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let Some((chain_pos, chain)) = parse_element_chain(args) else {
        return Ok(false);
    };
    if !is_mask_like_chain(ctx, form_id, &chain) {
        return Ok(false);
    }

    // `args` layout (method call):
    // [op_id, param0, ..., Element(chain), al_id, ret_form]
    let params = if chain_pos > 1 { &args[1..chain_pos] } else { &[] };
    let al_id = args
        .get(chain_pos + 1)
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
    let ret_form = args
        .get(chain_pos + 2)
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32;
	let elm_array = ctx.ids.elm_array;

    // Decide whether we should confirm this form id as MASKLIST before borrowing `mask_lists`.
	let should_confirm = ctx.globals.guessed_mask_form_id.is_none()
        && params.iter().any(|v| v.as_str().is_some())
        && chain.len() >= 4
		&& is_array_code(elm_array, chain[1]);
    if should_confirm {
        ctx.globals.guessed_mask_form_id = Some(form_id);
    }

    let cnt = mask_cnt(ctx);
    let (handled, ret): (bool, Option<Value>) = 'blk: {
        let ml = ctx
            .globals
            .mask_lists
            .entry(form_id)
            .or_insert_with(|| MaskListState::new(cnt));
        ml.ensure_size(cnt);
        if should_confirm {
            ml.confirmed = true;
        }

        // MASKLIST.GET_SIZE
		if chain.len() == 2 && !is_array_code(elm_array, chain[1]) {
            let r = if ret_form != 0 {
                Some(Value::Int(ml.masks.len() as i64))
            } else {
                None
            };
            break 'blk (true, r);
        }

        // MASKLIST[idx]
		if chain.len() < 3 || !is_array_code(elm_array, chain[1]) {
            let r = if ret_form != 0 {
                Some(default_for_ret_form(ret_form))
            } else {
                None
            };
            break 'blk (true, r);
        }

        let idx = chain.get(2).copied().unwrap_or(0);
        let idx_usize = if idx < 0 { 0 } else { idx as usize };
        let Some(mask) = ml.masks.get_mut(idx_usize) else {
            let r = if ret_form != 0 {
                Some(default_for_ret_form(ret_form))
            } else {
                None
            };
            break 'blk (true, r);
        };

        if chain.len() == 3 {
            let r = if ret_form != 0 {
                Some(default_for_ret_form(ret_form))
            } else {
                None
            };
            break 'blk (true, r);
        }

        let op = chain[3];

        // Event sub-object: MASK[idx].X_EVE / Y_EVE
        if chain.len() >= 5 {
            let target_ev = if ml.x_eve_op == Some(op) {
                &mut mask.x_event
            } else if ml.y_eve_op == Some(op) {
                &mut mask.y_event
            } else if ml.x_eve_op.is_none() {
                ml.x_eve_op = Some(op);
                &mut mask.x_event
            } else if ml.y_eve_op.is_none() {
                ml.y_eve_op = Some(op);
                &mut mask.y_event
            } else {
                mask.extra_events.entry(op).or_insert_with(|| IntEvent::new(0))
            };
            let r = dispatch_int_event(target_ev, params, ret_form);
            break 'blk (true, r);
        }

        // No further chain: either INIT/CREATE or X/Y property.
        if params.is_empty() && ret_form == 0 {
            mask.reinit();
            break 'blk (true, None);
        }

        if let Some(name) = params.get(0).and_then(|v| v.as_str()).map(|s| s.to_string()) {
            mask.name = Some(name);
            break 'blk (true, None);
        }

        // Treat as X/Y property get/set.
        let slot = if ml.x_op == Some(op) {
            0
        } else if ml.y_op == Some(op) {
            1
        } else if ml.x_op.is_none() {
            ml.x_op = Some(op);
            0
        } else if ml.y_op.is_none() {
            ml.y_op = Some(op);
            1
        } else {
            2
        };

        match slot {
            0 => {
                if al_id == 0 {
                    let r = if ret_form != 0 {
                        Some(Value::Int(mask.x_event.get_total_value() as i64))
                    } else {
                        None
                    };
                    break 'blk (true, r);
                }
                if al_id == 1 {
                    let v = params.get(0).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    mask.x_event.set_value(v);
                    mask.x_event.frame();
                    break 'blk (true, None);
                }
            }
            1 => {
                if al_id == 0 {
                    let r = if ret_form != 0 {
                        Some(Value::Int(mask.y_event.get_total_value() as i64))
                    } else {
                        None
                    };
                    break 'blk (true, r);
                }
                if al_id == 1 {
                    let v = params.get(0).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    mask.y_event.set_value(v);
                    mask.y_event.frame();
                    break 'blk (true, None);
                }
            }
            _ => {
                if al_id == 0 {
                    let r = if ret_form != 0 {
                        let v = *mask.extra_int.get(&op).unwrap_or(&0);
                        Some(Value::Int(v as i64))
                    } else {
                        None
                    };
                    break 'blk (true, r);
                }
                if al_id == 1 {
                    let v = params.get(0).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    mask.extra_int.insert(op, v);
                    break 'blk (true, None);
                }
            }
        }

        break 'blk (true, None);
    };

    if let Some(v) = ret {
        ctx.push(v);
    }
    Ok(handled)
}

/// Best-effort dispatch for unknown global forms.
///
/// This is used when `form_global_mask` is not mapped in `IdMap`.
pub fn maybe_dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    // If explicitly mapped, the main dispatcher will call `dispatch` directly.
    if ctx.ids.form_global_mask != 0 {
        return Ok(false);
    }

    // Avoid false positives if the title does not define masks.
    if mask_cnt(ctx) == 0 {
        return Ok(false);
    }

    // If we've already guessed, only accept that exact form id.
    if let Some(fid) = ctx.globals.guessed_mask_form_id {
        if fid != form_id {
            return Ok(false);
        }
        return dispatch(ctx, form_id, args);
    }

    // Otherwise, require a strong-ish signature (mask-like chain and has string param).
    let Some((chain_pos, chain)) = parse_element_chain(args) else {
        return Ok(false);
    };
    if !is_mask_like_chain(ctx, form_id, &chain) {
        return Ok(false);
    }
    let params = if chain_pos > 1 { &args[1..chain_pos] } else { &[] };
    let has_string = params.iter().any(|v| v.as_str().is_some());
    if !has_string {
        return Ok(false);
    }

    dispatch(ctx, form_id, args)
}
