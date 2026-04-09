//! Global SCREEN form.
//!
//! In the original engine, `GLOBAL.SCREEN` provides:
//!   - Effect list access (`SCREEN.EFFECT[...]`), with per-effect properties and int-events
//!   - Quake list access (`SCREEN.QUAKE[...]`)
//!   - Shake command (`SCREEN.SHAKE(...)`)
//!   - Convenience aliases to `EFFECT[0]` properties (`SCREEN.X`, `SCREEN.X_EVE`, ...)
//!
//! This port now routes only through confirmed selector/op ids.
//! Unhandled SCREEN chains are recorded as unknown instead of being auto-learned.

use anyhow::Result;

use crate::runtime::globals::{ScreenFormState, ScreenItemState};
use crate::runtime::{CommandContext, Value};

fn find_chain(args: &[Value]) -> Option<(usize, Vec<i32>)> {
    for (i, v) in args.iter().enumerate().rev() {
        if let Value::Element(e) = v {
            return Some((i, e.clone()));
        }
    }
    None
}

fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Int(n) => Some(*n),
        _ => None,
    }
}

fn default_for_ret_form(ret_form: i64) -> Value {
    if ret_form == 2 {
        Value::Str(String::new())
    } else {
        Value::Int(0)
    }
}

fn is_array_code(elm_array: i32, code: i32) -> bool {
    code == elm_array || code == -1
}

fn record_unknown_screen_chain(
    ctx: &mut CommandContext,
    form_id: u32,
    chain: &[i32],
    ret_form: Option<i64>,
) -> Result<bool> {
    ctx.unknown
        .record_element_chain(form_id, chain, "screen-unhandled");
    match ret_form {
        Some(rf) if rf != 0 => ctx.stack.push(default_for_ret_form(rf)),
        _ => ctx.stack.push(Value::Int(0)),
    }
    Ok(true)
}

fn is_effect_prop(ids: &crate::runtime::id_map::IdMap, op: i32) -> bool {
    matches!(
        op,
        s if s == ids.effect_x
            || s == ids.effect_y
            || s == ids.effect_z
            || s == ids.effect_mono
            || s == ids.effect_reverse
            || s == ids.effect_bright
            || s == ids.effect_dark
            || s == ids.effect_color_r
            || s == ids.effect_color_g
            || s == ids.effect_color_b
            || s == ids.effect_color_rate
            || s == ids.effect_color_add_r
            || s == ids.effect_color_add_g
            || s == ids.effect_color_add_b
            || s == ids.effect_begin_order
            || s == ids.effect_end_order
            || s == ids.effect_begin_layer
            || s == ids.effect_end_layer
    )
}

fn is_effect_event(ids: &crate::runtime::id_map::IdMap, op: i32) -> bool {
    matches!(
        op,
        s if s == ids.effect_x_eve
            || s == ids.effect_y_eve
            || s == ids.effect_z_eve
            || s == ids.effect_mono_eve
            || s == ids.effect_reverse_eve
            || s == ids.effect_bright_eve
            || s == ids.effect_dark_eve
            || s == ids.effect_color_r_eve
            || s == ids.effect_color_g_eve
            || s == ids.effect_color_b_eve
            || s == ids.effect_color_rate_eve
            || s == ids.effect_color_add_r_eve
            || s == ids.effect_color_add_g_eve
            || s == ids.effect_color_add_b_eve
    )
}

fn dispatch_int_event(
    stack: &mut Vec<Value>,
    ev: &mut crate::runtime::int_event::IntEvent,
    params: &[Value],
    ret_form: Option<i64>,
) {
    // Keep behavior aligned with the project's existing runtime convention:
    // - 0 params: if ret_form!=0 => CHECK; else => END
    // - 4 params => SET
    // - 5 params => LOOP
    match params.len() {
        0 => {
            let rf = ret_form.unwrap_or(1);
            if rf != 0 {
                let v = if ev.check_event() { 1 } else { 0 };
                stack.push(Value::Int(v));
            } else {
                ev.end_event();
            }
        }
        4 => {
            let a0 = as_i64(&params[0]).unwrap_or(0) as i32;
            let a1 = as_i64(&params[1]).unwrap_or(0) as i32;
            let a2 = as_i64(&params[2]).unwrap_or(0) as i32;
            let a3 = as_i64(&params[3]).unwrap_or(0) as i32;
            let real_flag = 0;
            ev.set_event(a0, a1, a2, a3, real_flag);
            if let Some(rf) = ret_form {
                if rf != 0 {
                    stack.push(default_for_ret_form(rf));
                }
            }
        }
        5 => {
            let a0 = as_i64(&params[0]).unwrap_or(0) as i32;
            let a1 = as_i64(&params[1]).unwrap_or(0) as i32;
            let a2 = as_i64(&params[2]).unwrap_or(0) as i32;
            let a3 = as_i64(&params[3]).unwrap_or(0) as i32;
            let a4 = as_i64(&params[4]).unwrap_or(0) as i32;
            let real_flag = 0;
            ev.loop_event(a0, a1, a2, a3, a4, real_flag);
            if let Some(rf) = ret_form {
                if rf != 0 {
                    stack.push(default_for_ret_form(rf));
                }
            }
        }
        _ => {
            // Unknown signature: do not block.
            if let Some(rf) = ret_form {
                if rf != 0 {
                    stack.push(default_for_ret_form(rf));
                }
            }
        }
    }
}

fn item_value(item: &ScreenItemState, op: i32) -> i64 {
    if op == 0 {
        return 0;
    }
    if let Some(ev) = item.events.get(&op) {
        return ev.get_total_value() as i64;
    }
    item.props.get(&op).copied().unwrap_or(0)
}

fn set_item_value(item: &mut ScreenItemState, op: i32, v: i64) {
    if op == 0 {
        return;
    }
    if let Some(ev) = item.events.get_mut(&op) {
        ev.set_value(v as i32);
        ev.frame();
        return;
    }
    item.props.insert(op, v);
}

fn screen_to_effect_prop(ids: &crate::runtime::id_map::IdMap, selector: i32) -> Option<i32> {
    match selector {
        s if ids.screen_x != 0 && s == ids.screen_x => Some(ids.effect_x),
        s if ids.screen_y != 0 && s == ids.screen_y => Some(ids.effect_y),
        s if ids.screen_z != 0 && s == ids.screen_z => Some(ids.effect_z),
        s if ids.screen_mono != 0 && s == ids.screen_mono => Some(ids.effect_mono),
        s if ids.screen_reverse != 0 && s == ids.screen_reverse => Some(ids.effect_reverse),
        s if ids.screen_bright != 0 && s == ids.screen_bright => Some(ids.effect_bright),
        s if ids.screen_dark != 0 && s == ids.screen_dark => Some(ids.effect_dark),
        s if ids.screen_color_r != 0 && s == ids.screen_color_r => Some(ids.effect_color_r),
        s if ids.screen_color_g != 0 && s == ids.screen_color_g => Some(ids.effect_color_g),
        s if ids.screen_color_b != 0 && s == ids.screen_color_b => Some(ids.effect_color_b),
        s if ids.screen_color_rate != 0 && s == ids.screen_color_rate => {
            Some(ids.effect_color_rate)
        }
        s if ids.screen_color_add_r != 0 && s == ids.screen_color_add_r => {
            Some(ids.effect_color_add_r)
        }
        s if ids.screen_color_add_g != 0 && s == ids.screen_color_add_g => {
            Some(ids.effect_color_add_g)
        }
        s if ids.screen_color_add_b != 0 && s == ids.screen_color_add_b => {
            Some(ids.effect_color_add_b)
        }
        _ => None,
    }
}

fn screen_to_effect_event(ids: &crate::runtime::id_map::IdMap, selector: i32) -> Option<i32> {
    match selector {
        s if ids.screen_x_eve != 0 && s == ids.screen_x_eve => Some(ids.effect_x_eve),
        s if ids.screen_y_eve != 0 && s == ids.screen_y_eve => Some(ids.effect_y_eve),
        s if ids.screen_z_eve != 0 && s == ids.screen_z_eve => Some(ids.effect_z_eve),
        s if ids.screen_mono_eve != 0 && s == ids.screen_mono_eve => Some(ids.effect_mono_eve),
        s if ids.screen_reverse_eve != 0 && s == ids.screen_reverse_eve => {
            Some(ids.effect_reverse_eve)
        }
        s if ids.screen_bright_eve != 0 && s == ids.screen_bright_eve => {
            Some(ids.effect_bright_eve)
        }
        s if ids.screen_dark_eve != 0 && s == ids.screen_dark_eve => Some(ids.effect_dark_eve),
        s if ids.screen_color_r_eve != 0 && s == ids.screen_color_r_eve => {
            Some(ids.effect_color_r_eve)
        }
        s if ids.screen_color_g_eve != 0 && s == ids.screen_color_g_eve => {
            Some(ids.effect_color_g_eve)
        }
        s if ids.screen_color_b_eve != 0 && s == ids.screen_color_b_eve => {
            Some(ids.effect_color_b_eve)
        }
        s if ids.screen_color_rate_eve != 0 && s == ids.screen_color_rate_eve => {
            Some(ids.effect_color_rate_eve)
        }
        s if ids.screen_color_add_r_eve != 0 && s == ids.screen_color_add_r_eve => {
            Some(ids.effect_color_add_r_eve)
        }
        s if ids.screen_color_add_g_eve != 0 && s == ids.screen_color_add_g_eve => {
            Some(ids.effect_color_add_g_eve)
        }
        s if ids.screen_color_add_b_eve != 0 && s == ids.screen_color_add_b_eve => {
            Some(ids.effect_color_add_b_eve)
        }
        _ => None,
    }
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    // Fast-path legacy behavior: some builds call SCREEN with a numeric op only.
    // Preserve the original runtime width/height helper.
    let Some((chain_pos, chain)) = find_chain(args) else {
        if let Some(Value::Int(op)) = args.get(0) {
            let w0 = ctx.screen_w as i64;
            let h0 = ctx.screen_h as i64;
            match *op {
                0 => {
                    ctx.stack.push(Value::Int(w0));
                    return Ok(true);
                }
                1 => {
                    ctx.stack.push(Value::Int(h0));
                    return Ok(true);
                }
                2 => {
                    if args.len() >= 3 {
                        let w = as_i64(&args[1]).unwrap_or(w0);
                        let h = as_i64(&args[2]).unwrap_or(h0);
                        ctx.set_screen_size(w.max(1) as u32, h.max(1) as u32);
                    }
                    ctx.stack.push(Value::Int(0));
                    return Ok(true);
                }
                _ => {
                    ctx.stack.push(Value::Int(0));
                    return Ok(true);
                }
            }
        }
        return Ok(false);
    };

    if chain.is_empty() {
        return Ok(false);
    }

    let form_id = chain[0] as u32;
    let elm_array = ctx.ids.elm_array;
    let mut st = ctx
        .globals
        .screen_forms
        .remove(&form_id)
        .unwrap_or_default();
    let result = (|| {
        // Optional command metadata exists only for call_command():
        //   [..., Element(chain), al_id, ret_form]
        let mut ret_form: Option<i64> = None;
        if chain_pos + 2 < args.len() {
            if let (Some(a), Some(r)) = (as_i64(&args[chain_pos + 1]), as_i64(&args[chain_pos + 2]))
            {
                let _ = a;
                ret_form = Some(r);
            }
        }

        // Property-assign shape (call_property_assign):
        //   [op_id, al_id, rhs, Element(chain)]
        // Property-get shape (call_property):
        //   [op_id, Element(chain)]
        let rhs: Option<&Value> = if ret_form.is_none() && chain_pos >= 2 {
            if as_i64(&args[1]).is_some() {
                Some(&args[2])
            } else {
                None
            }
        } else {
            None
        };

        // Command args sit before Element(chain). The first value is the synthetic op_id
        // inserted by VM for form calls, and is not meaningful here.
        let script_args = if chain_pos >= 1 {
            &args[1..chain_pos]
        } else {
            &[][..]
        };

        // Minimal chain: [FORM_SCREEN, selector, ...]
        if chain.len() < 2 {
            // Returning the screen element itself.
            if let Some(rf) = ret_form {
                if rf != 0 {
                    ctx.stack.push(default_for_ret_form(rf));
                }
            }
            return Ok(true);
        }
        let selector = chain[1];

        // ---------------------------------------------------------------------
        // List selector: [FORM, selector, ELM_ARRAY, idx, ...]
        // ---------------------------------------------------------------------
        if chain.len() >= 4 && is_array_code(elm_array, chain[2]) {
            let is_effect_selector =
                ctx.ids.screen_sel_effect != 0 && selector == ctx.ids.screen_sel_effect;
            let is_quake_selector =
                ctx.ids.screen_sel_quake != 0 && selector == ctx.ids.screen_sel_quake;
            if !is_effect_selector && !is_quake_selector {
                return record_unknown_screen_chain(ctx, form_id, &chain, ret_form);
            }
            let idx = chain[3] as i64;
            let list = st.lists.entry(selector).or_default();

            // Ensure list has at least one element; scripts often assume EFFECT[0] exists.
            let min_len = (idx.max(0) as usize).saturating_add(1).max(1);
            list.ensure_size(min_len);

            let item = &mut list.items[idx.max(0) as usize];

            // Item access patterns:
            //   [FORM, selector, ARRAY, idx] => element itself
            //   [FORM, selector, ARRAY, idx, op] => property/init
            //   [FORM, selector, ARRAY, idx, op, ...] => int-event subobject
            if chain.len() == 4 {
                if let Some(rf) = ret_form {
                    if rf != 0 {
                        ctx.stack.push(default_for_ret_form(rf));
                    }
                }
                return Ok(true);
            }

            let op = chain[4];

            // -----------------------------------------------------------------
            // Quake command
            //
            // QUAKE items support START/END/WAIT/CHECK. EFFECT items do not.
            // We auto-learn which list selector corresponds to QUAKE by observing
            // a START-like call (void return with at least two integer args).
            //
            // This block is intentionally conservative: it only triggers for
            // call_command-style invocations (ret_form is Some(...)).
            // -----------------------------------------------------------------
            if rhs.is_none() {
                if let Some(rf) = ret_form {
                    // START-like: (type, time, ...) -> void
                    if rf == 0 {
                        if is_quake_selector
                            && script_args.len() >= 2
                            && script_args[0].as_i64().is_some()
                            && script_args[1].as_i64().is_some()
                        {
                            let quake_type = script_args[0].as_i64().unwrap_or(0) as i32;
                            let time_ms = script_args[1].as_i64().unwrap_or(0);
                            let mut power = 0i32;
                            let mut vec = 0i32;
                            let mut cx = 0i32;
                            let mut cy = 0i32;
                            if let Some(Value::List(list)) = script_args.last() {
                                power = list.get(0).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                if quake_type == 2 {
                                    cx = list.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                    cy = list.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                } else {
                                    vec = list.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                                }
                            }
                            item.quake_set_params(quake_type, power, vec, cx, cy);
                            item.quake_start_ms(time_ms);
                            // Runtime: do not auto-wait here because START variants
                            // (WAIT / NOWAIT) are encoded in the element code.
                            ctx.stack.push(Value::Int(0));
                            return Ok(true);
                        }

                        // END-like: optional fade time -> void
                        if is_quake_selector
                            && script_args.len() == 1
                            && script_args[0].as_i64().is_some()
                        {
                            let t = script_args[0].as_i64().unwrap_or(0);
                            item.quake_end_ms(t);
                            ctx.stack.push(Value::Int(0));
                            return Ok(true);
                        }

                        // WAIT-like: no args -> void
                        if is_quake_selector && script_args.is_empty() {
                            let rem = item.quake_remaining_ms();
                            if rem > 0 {
                                ctx.wait.wait_ms(rem);
                            }
                            ctx.stack.push(Value::Int(0));
                            return Ok(true);
                        }
                    } else {
                        // CHECK-like: no args -> int
                        if is_quake_selector && script_args.is_empty() {
                            let active = item.quake_is_active();
                            ctx.stack.push(Value::Int(if active { 1 } else { 0 }));
                            return Ok(true);
                        }
                    }
                }
            }

            // IntEvent subobject on an item.
            if chain.len() >= 6 {
                if !is_effect_selector || !is_effect_event(&ctx.ids, op) {
                    return record_unknown_screen_chain(ctx, form_id, &chain, ret_form);
                }
                let ev = item
                    .events
                    .entry(op)
                    .or_insert_with(|| crate::runtime::int_event::IntEvent::new(0));
                dispatch_int_event(&mut ctx.stack, ev, script_args, ret_form);
                return Ok(true);
            }

            // INIT: no args, void
            if script_args.is_empty()
                && ret_form == Some(0)
                && ctx.ids.effect_init != 0
                && op == ctx.ids.effect_init
            {
                item.reinit();
                ctx.stack.push(Value::Int(0));
                return Ok(true);
            }

            if is_effect_selector && is_effect_prop(&ctx.ids, op) {
                if let Some(rhs) = rhs {
                    let v = as_i64(rhs).unwrap_or(0);
                    item.props.insert(op, v);
                    ctx.stack.push(Value::Int(0));
                } else {
                    let v = item.props.get(&op).copied().unwrap_or(0);
                    ctx.stack.push(Value::Int(v));
                }
                return Ok(true);
            }

            if is_effect_selector && is_effect_event(&ctx.ids, op) {
                let ev = item
                    .events
                    .entry(op)
                    .or_insert_with(|| crate::runtime::int_event::IntEvent::new(0));
                dispatch_int_event(&mut ctx.stack, ev, script_args, ret_form);
                return Ok(true);
            }

            return record_unknown_screen_chain(ctx, form_id, &chain, ret_form);
        }

        // ---------------------------------------------------------------------
        // Non-list selector: property / int-event / command.
        // ---------------------------------------------------------------------

        // A chained selector of length 3 is either:
        //   - root-level int-event: [FORM, X_EVE, <int_event_op>]
        //   - list-level op on a list selector: [FORM, EFFECT, GET_SIZE] / [FORM, EFFECT, RESIZE]
        // Without element code mapping, we disambiguate by observing parameter counts.
        if chain.len() == 3 {
            return record_unknown_screen_chain(ctx, form_id, &chain, ret_form);
        }

        // Root-level property: [FORM, prop]
        if chain.len() == 2 {
            // Compatibility: SCREEN.X maps to EFFECT[0].X
            if let Some(effect_selector) =
                (ctx.ids.screen_sel_effect != 0).then_some(ctx.ids.screen_sel_effect)
            {
                if let Some(effect_op) = screen_to_effect_prop(&ctx.ids, selector) {
                    let list = st.lists.entry(effect_selector).or_default();
                    list.ensure_size(1);
                    let item = &mut list.items[0];
                    if let Some(rhs) = rhs {
                        let v = as_i64(rhs).unwrap_or(0);
                        set_item_value(item, effect_op, v);
                        ctx.stack.push(Value::Int(0));
                    } else {
                        let v = item_value(item, effect_op);
                        ctx.stack.push(Value::Int(v));
                    }
                    return Ok(true);
                }
                if let Some(effect_event) = screen_to_effect_event(&ctx.ids, selector) {
                    let ev_op = if effect_event != 0 {
                        effect_event
                    } else {
                        selector
                    };
                    let list = st.lists.entry(effect_selector).or_default();
                    list.ensure_size(1);
                    let item = &mut list.items[0];
                    let ev = item
                        .events
                        .entry(ev_op)
                        .or_insert_with(|| crate::runtime::int_event::IntEvent::new(0));
                    dispatch_int_event(&mut ctx.stack, ev, script_args, ret_form);
                    return Ok(true);
                }
            }

            if script_args.len() == 1
                && ret_form == Some(0)
                && rhs.is_none()
                && ctx.ids.screen_sel_shake != 0
                && selector == ctx.ids.screen_sel_shake
            {
                st.last_shake = as_i64(&script_args[0]).unwrap_or(0);
                let ms = st.last_shake.max(0) as u64;
                if ms == 0 {
                    st.shake_until = None;
                } else {
                    st.shake_until =
                        Some(std::time::Instant::now() + std::time::Duration::from_millis(ms));
                }
                ctx.stack.push(Value::Int(0));
                return Ok(true);
            }

            if screen_to_effect_prop(&ctx.ids, selector).is_some()
                || screen_to_effect_event(&ctx.ids, selector).is_some()
            {
                return record_unknown_screen_chain(ctx, form_id, &chain, ret_form);
            }

            // Special-case: if this looks like width/height access, expose ctx screen size.
            // We keep the legacy op mapping (0=width, 1=height) when selector matches.
            if selector == 0 {
                ctx.stack.push(Value::Int(ctx.screen_w as i64));
                return Ok(true);
            }
            if selector == 1 {
                ctx.stack.push(Value::Int(ctx.screen_h as i64));
                return Ok(true);
            }

            return record_unknown_screen_chain(ctx, form_id, &chain, ret_form);
        }

        record_unknown_screen_chain(ctx, form_id, &chain, ret_form)
    })();
    ctx.globals.screen_forms.insert(form_id, st);
    result
}
