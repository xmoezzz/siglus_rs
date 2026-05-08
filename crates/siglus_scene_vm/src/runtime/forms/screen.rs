//! GLOBAL.SCREEN form handling aligned to the original screen command path.
//!
//! The original C++ `tnm_command_proc_screen()` dispatches strictly by the
//! current element chain. It does not recover SCREEN selectors or setter RHS
//! values from arbitrary argument positions.

use anyhow::Result;

use super::prop_access;
use crate::runtime::forms::codes::int_event_op;
use crate::runtime::globals::{ScreenEffectState, ScreenFormState, ScreenQuakeState};
use crate::runtime::{CommandContext, Value};

const EFFECTLIST_RESIZE_OP: i32 = 1;
const EFFECTLIST_GET_SIZE_OP: i32 = 2;

const QUAKE_START_OP: i32 = 0;
const QUAKE_START_WAIT_OP: i32 = 1;
const QUAKE_START_WAIT_KEY_OP: i32 = 2;
const QUAKE_START_NOWAIT_OP: i32 = 3;
const QUAKE_START_ALL_OP: i32 = 4;
const QUAKE_START_ALL_WAIT_OP: i32 = 5;
const QUAKE_START_ALL_WAIT_KEY_OP: i32 = 6;
const QUAKE_START_ALL_NOWAIT_OP: i32 = 7;
const QUAKE_END_OP: i32 = 8;
const QUAKE_CHECK_OP: i32 = 9;
const QUAKE_WAIT_OP: i32 = 10;
const QUAKE_WAIT_KEY_OP: i32 = 11;

fn as_i64(v: &Value) -> Option<i64> {
    match v {
        Value::Int(n) => Some(*n),
        Value::NamedArg { value, .. } => value.as_i64(),
        _ => None,
    }
}

fn default_for_ret_form(ret_form: i64) -> Value {
    if prop_access::ret_form_is_string(ret_form) {
        Value::Str(String::new())
    } else {
        Value::Int(0)
    }
}

fn anim_skip_trace_enabled() -> bool {
    std::env::var_os("SG_DEBUG").is_some()
}

fn screen_event_state(ev: &crate::runtime::int_event::IntEvent) -> String {
    format!(
        "value={} cur={} start={} end={} cur_time={} end_time={} delay={} loop_type={} speed={} real={} active={}",
        ev.value, ev.cur_value, ev.start_value, ev.end_value, ev.cur_time, ev.end_time,
        ev.delay_time, ev.loop_type, ev.speed_type, ev.real_flag, ev.check_event()
    )
}

fn effect_prop_ref_mut<'a>(
    ids: &crate::runtime::constants::RuntimeConstants,
    effect: &'a mut ScreenEffectState,
    op: i32,
) -> Option<&'a mut i32> {
    match op {
        s if s == ids.effect_wipe_copy => Some(&mut effect.wipe_copy),
        s if s == ids.effect_wipe_erase => Some(&mut effect.wipe_erase),
        s if s == ids.effect_begin_order => Some(&mut effect.begin_order),
        s if s == ids.effect_begin_layer => Some(&mut effect.begin_layer),
        s if s == ids.effect_end_order => Some(&mut effect.end_order),
        s if s == ids.effect_end_layer => Some(&mut effect.end_layer),
        _ => None,
    }
}

fn effect_prop_value(
    ids: &crate::runtime::constants::RuntimeConstants,
    effect: &ScreenEffectState,
    op: i32,
) -> Option<i64> {
    match op {
        s if s == ids.effect_wipe_copy => Some(effect.wipe_copy as i64),
        s if s == ids.effect_wipe_erase => Some(effect.wipe_erase as i64),
        s if s == ids.effect_begin_order => Some(effect.begin_order as i64),
        s if s == ids.effect_begin_layer => Some(effect.begin_layer as i64),
        s if s == ids.effect_end_order => Some(effect.end_order as i64),
        s if s == ids.effect_end_layer => Some(effect.end_layer as i64),
        _ => None,
    }
}

fn effect_event_mut<'a>(
    ids: &crate::runtime::constants::RuntimeConstants,
    effect: &'a mut ScreenEffectState,
    op: i32,
) -> Option<&'a mut crate::runtime::int_event::IntEvent> {
    match op {
        s if s == ids.effect_x || s == ids.effect_x_eve => Some(&mut effect.x),
        s if s == ids.effect_y || s == ids.effect_y_eve => Some(&mut effect.y),
        s if s == ids.effect_z || s == ids.effect_z_eve => Some(&mut effect.z),
        s if s == ids.effect_mono || s == ids.effect_mono_eve => Some(&mut effect.mono),
        s if s == ids.effect_reverse || s == ids.effect_reverse_eve => Some(&mut effect.reverse),
        s if s == ids.effect_bright || s == ids.effect_bright_eve => Some(&mut effect.bright),
        s if s == ids.effect_dark || s == ids.effect_dark_eve => Some(&mut effect.dark),
        s if s == ids.effect_color_r || s == ids.effect_color_r_eve => Some(&mut effect.color_r),
        s if s == ids.effect_color_g || s == ids.effect_color_g_eve => Some(&mut effect.color_g),
        s if s == ids.effect_color_b || s == ids.effect_color_b_eve => Some(&mut effect.color_b),
        s if s == ids.effect_color_rate || s == ids.effect_color_rate_eve => {
            Some(&mut effect.color_rate)
        }
        s if s == ids.effect_color_add_r || s == ids.effect_color_add_r_eve => {
            Some(&mut effect.color_add_r)
        }
        s if s == ids.effect_color_add_g || s == ids.effect_color_add_g_eve => {
            Some(&mut effect.color_add_g)
        }
        s if s == ids.effect_color_add_b || s == ids.effect_color_add_b_eve => {
            Some(&mut effect.color_add_b)
        }
        _ => None,
    }
}

fn run_int_event_command(
    ctx: &mut CommandContext,
    ev: &mut crate::runtime::int_event::IntEvent,
    subop: i32,
    params: &[Value],
    ret_form: i64,
) {
    match subop {
        int_event_op::SET | int_event_op::SET_REAL => {
            let value = params.first().and_then(as_i64).unwrap_or(0) as i32;
            let total_time = params.get(1).and_then(as_i64).unwrap_or(0) as i32;
            let delay_time = params.get(2).and_then(as_i64).unwrap_or(0) as i32;
            let speed_type = params.get(3).and_then(as_i64).unwrap_or(0) as i32;
            let real_flag = if subop == int_event_op::SET_REAL {
                1
            } else {
                0
            };
            ev.set_event(value, total_time, delay_time, speed_type, real_flag);
            if anim_skip_trace_enabled() {
                eprintln!(
                    "[SG_DEBUG][ANIM_SKIP_TRACE][SCREEN] INTEVENT.SET subop={} value={} total_time={} delay={} speed={} real={} state=[{}]",
                    subop, value, total_time, delay_time, speed_type, real_flag, screen_event_state(ev)
                );
            }
            ctx.stack.push(default_for_ret_form(ret_form));
        }
        int_event_op::LOOP | int_event_op::LOOP_REAL => {
            let start_value = params.first().and_then(as_i64).unwrap_or(0) as i32;
            let end_value = params.get(1).and_then(as_i64).unwrap_or(0) as i32;
            let loop_time = params.get(2).and_then(as_i64).unwrap_or(0) as i32;
            let delay_time = params.get(3).and_then(as_i64).unwrap_or(0) as i32;
            let speed_type = params.get(4).and_then(as_i64).unwrap_or(0) as i32;
            let real_flag = if subop == int_event_op::LOOP_REAL {
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
            if anim_skip_trace_enabled() {
                eprintln!(
                    "[SG_DEBUG][ANIM_SKIP_TRACE][SCREEN] INTEVENT.LOOP subop={} start={} end={} loop_time={} delay={} speed={} real={} state=[{}]",
                    subop, start_value, end_value, loop_time, delay_time, speed_type, real_flag, screen_event_state(ev)
                );
            }
            ctx.stack.push(default_for_ret_form(ret_form));
        }
        int_event_op::TURN | int_event_op::TURN_REAL => {
            let start_value = params.first().and_then(as_i64).unwrap_or(0) as i32;
            let end_value = params.get(1).and_then(as_i64).unwrap_or(0) as i32;
            let loop_time = params.get(2).and_then(as_i64).unwrap_or(0) as i32;
            let delay_time = params.get(3).and_then(as_i64).unwrap_or(0) as i32;
            let speed_type = params.get(4).and_then(as_i64).unwrap_or(0) as i32;
            let real_flag = if subop == int_event_op::TURN_REAL {
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
            if anim_skip_trace_enabled() {
                eprintln!(
                    "[SG_DEBUG][ANIM_SKIP_TRACE][SCREEN] INTEVENT.TURN subop={} start={} end={} loop_time={} delay={} speed={} real={} state=[{}]",
                    subop, start_value, end_value, loop_time, delay_time, speed_type, real_flag, screen_event_state(ev)
                );
            }
            ctx.stack.push(default_for_ret_form(ret_form));
        }
        int_event_op::END => {
            if anim_skip_trace_enabled() {
                eprintln!(
                    "[SG_DEBUG][ANIM_SKIP_TRACE][SCREEN] INTEVENT.END before state=[{}]",
                    screen_event_state(ev)
                );
            }
            ev.end_event();
            if anim_skip_trace_enabled() {
                eprintln!(
                    "[SG_DEBUG][ANIM_SKIP_TRACE][SCREEN] INTEVENT.END after state=[{}]",
                    screen_event_state(ev)
                );
            }
            ctx.stack.push(default_for_ret_form(ret_form));
        }
        int_event_op::WAIT => {
            if anim_skip_trace_enabled() {
                eprintln!("[SG_DEBUG][ANIM_SKIP_TRACE][SCREEN] INTEVENT.WAIT requested NOTE=current implementation routes through generic form_id=0");
            }
            ctx.wait.wait_generic_int_event(0, None, false, false)
        }
        int_event_op::WAIT_KEY => {
            if anim_skip_trace_enabled() {
                eprintln!("[SG_DEBUG][ANIM_SKIP_TRACE][SCREEN] INTEVENT.WAIT_KEY requested NOTE=current implementation routes through generic form_id=0");
            }
            ctx.wait.wait_generic_int_event(0, None, true, true)
        }
        int_event_op::CHECK => ctx
            .stack
            .push(Value::Int(if ev.check_event() { 1 } else { 0 })),
        _ => ctx.stack.push(default_for_ret_form(ret_form)),
    }
}

fn screen_to_effect_prop(
    ids: &crate::runtime::constants::RuntimeConstants,
    selector: i32,
) -> Option<i32> {
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

fn screen_to_effect_event(
    ids: &crate::runtime::constants::RuntimeConstants,
    selector: i32,
) -> Option<i32> {
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

struct ScreenCall {
    chain: Vec<i32>,
    al_id: i32,
    ret_form: i64,
    rhs: Option<Value>,
    script_args: Vec<Value>,
}

fn parse_screen_call(ctx: &CommandContext, args: &[Value]) -> Option<ScreenCall> {
    let vm_call = ctx.vm_call.as_ref()?;
    if vm_call.element.is_empty() {
        return None;
    }
    let (al_id, ret_form) = prop_access::current_vm_meta(ctx);
    let al_id = al_id.unwrap_or(-1) as i32;
    let ret_form = ret_form.unwrap_or(0);
    let rhs = if al_id == 1 {
        args.first().cloned()
    } else {
        None
    };
    Some(ScreenCall {
        chain: vm_call.element.clone(),
        al_id,
        ret_form,
        rhs,
        script_args: args.to_vec(),
    })
}

fn last_list_arg(script_args: &[Value]) -> Option<&Vec<Value>> {
    script_args.last().and_then(|v| match v.unwrap_named() {
        Value::List(list) => Some(list),
        _ => None,
    })
}

fn quake_start_flags(op: i32) -> Option<(bool, bool, bool)> {
    match op {
        QUAKE_START_OP => Some((false, false, false)),
        QUAKE_START_WAIT_OP => Some((false, true, false)),
        QUAKE_START_WAIT_KEY_OP => Some((false, true, true)),
        QUAKE_START_NOWAIT_OP => Some((false, false, false)),
        QUAKE_START_ALL_OP => Some((true, false, false)),
        QUAKE_START_ALL_WAIT_OP => Some((true, true, false)),
        QUAKE_START_ALL_WAIT_KEY_OP => Some((true, true, true)),
        QUAKE_START_ALL_NOWAIT_OP => Some((true, false, false)),
        _ => None,
    }
}

fn parse_quake_command(
    item: &mut ScreenQuakeState,
    op: i32,
    script_args: &[Value],
    ctx: &mut CommandContext,
) -> bool {
    if let Some((all_range, wait_flag, key_flag)) = quake_start_flags(op) {
        let quake_type = script_args.first().and_then(as_i64).unwrap_or(0) as i32;
        let time = script_args.get(1).and_then(as_i64).unwrap_or(1000);
        let _cnt = script_args.get(2).and_then(as_i64).unwrap_or(0) as i32;
        let _end_cnt = script_args.get(3).and_then(as_i64).unwrap_or(0) as i32;
        item.begin_order = if all_range {
            script_args
                .get(4)
                .and_then(as_i64)
                .unwrap_or(i32::MIN as i64) as i32
        } else {
            script_args.get(4).and_then(as_i64).unwrap_or(0) as i32
        };
        item.end_order = if all_range {
            script_args
                .get(5)
                .and_then(as_i64)
                .unwrap_or(i32::MAX as i64) as i32
        } else {
            script_args.get(5).and_then(as_i64).unwrap_or(0) as i32
        };

        let opt = last_list_arg(script_args);
        item.power = opt
            .and_then(|list| list.first())
            .and_then(as_i64)
            .unwrap_or(0) as i32;
        item.vec = opt
            .and_then(|list| list.get(1))
            .and_then(as_i64)
            .unwrap_or(0) as i32;
        item.center_x = opt
            .and_then(|list| list.get(1))
            .and_then(as_i64)
            .unwrap_or(0) as i32;
        item.center_y = opt
            .and_then(|list| list.get(2))
            .and_then(as_i64)
            .unwrap_or(0) as i32;
        item.start_kind(quake_type, time);
        if wait_flag {
            let rem = item.remaining_ms();
            if key_flag {
                ctx.wait.wait_ms_key(rem);
            } else {
                ctx.wait.wait_ms(rem);
            }
        }
        ctx.stack.push(Value::Int(0));
        return true;
    }

    match op {
        QUAKE_END_OP => {
            item.end_ms(script_args.first().and_then(as_i64).unwrap_or(0));
            ctx.stack.push(Value::Int(0));
            true
        }
        QUAKE_WAIT_OP => {
            ctx.wait.wait_ms(item.remaining_ms());
            ctx.stack.push(Value::Int(0));
            true
        }
        QUAKE_WAIT_KEY_OP => {
            ctx.wait.wait_ms_key(item.remaining_ms());
            ctx.stack.push(Value::Int(0));
            true
        }
        QUAKE_CHECK_OP => {
            ctx.stack.push(Value::Int(item.check_value() as i64));
            true
        }
        _ => false,
    }
}

fn get_or_set_effect_scalar(
    ctx: &mut CommandContext,
    effect: &mut ScreenEffectState,
    ids: &crate::runtime::constants::RuntimeConstants,
    op: i32,
    al_id: i32,
    rhs: Option<&Value>,
    ret_form: i64,
) -> bool {
    if let Some(ev) = effect_event_mut(ids, effect, op) {
        match al_id {
            0 => {
                ctx.stack.push(Value::Int(ev.get_total_value() as i64));
                true
            }
            1 => {
                let value = rhs.and_then(as_i64).unwrap_or(0) as i32;
                ev.set_value(value);
                ev.frame();
                ctx.stack.push(default_for_ret_form(ret_form));
                true
            }
            _ => false,
        }
    } else if let Some(slot) = effect_prop_ref_mut(ids, effect, op) {
        match al_id {
            0 => {
                ctx.stack.push(Value::Int(*slot as i64));
                true
            }
            1 => {
                *slot = rhs.and_then(as_i64).unwrap_or(0) as i32;
                ctx.stack.push(default_for_ret_form(ret_form));
                true
            }
            _ => false,
        }
    } else if let Some(value) = effect_prop_value(ids, effect, op) {
        if al_id == 0 {
            ctx.stack.push(Value::Int(value));
            true
        } else {
            false
        }
    } else {
        false
    }
}

pub fn dispatch(ctx: &mut CommandContext, args: &[Value]) -> Result<bool> {
    let Some(call) = parse_screen_call(ctx, args) else {
        return Ok(false);
    };
    let chain = call.chain;
    let form_id = chain[0] as u32;
    let mut st = ctx
        .globals
        .screen_forms
        .remove(&form_id)
        .unwrap_or_default();
    let result = (|| {
        if chain.len() == 1 {
            if call.ret_form != 0 {
                ctx.stack.push(default_for_ret_form(call.ret_form));
            }
            return Ok(true);
        }

        let selector = chain[1];
        let ids = ctx.ids.clone();

        if selector == ids.screen_sel_effect {
            if chain.len() == 3 {
                match chain[2] {
                    EFFECTLIST_RESIZE_OP => {
                        st.ensure_effect_len(
                            args.first().and_then(as_i64).unwrap_or(0).max(0) as usize
                        );
                        ctx.stack.push(Value::Int(0));
                        return Ok(true);
                    }
                    EFFECTLIST_GET_SIZE_OP => {
                        ctx.stack.push(Value::Int(st.effect_list.len() as i64));
                        return Ok(true);
                    }
                    _ => return Ok(false),
                }
            }
            if chain.len() < 4 || chain[2] != ids.elm_array {
                return Ok(false);
            }
            let idx = chain[3].max(0) as usize;
            st.ensure_effect_len(idx + 1);
            let effect = &mut st.effect_list[idx];
            if chain.len() == 4 {
                if call.ret_form != 0 {
                    ctx.stack.push(default_for_ret_form(call.ret_form));
                }
                return Ok(true);
            }
            let op = chain[4];
            if op == ids.effect_init && call.script_args.is_empty() && call.ret_form == 0 {
                effect.reinit();
                ctx.stack.push(Value::Int(0));
                return Ok(true);
            }
            if chain.len() >= 6 {
                if let Some(ev) = effect_event_mut(&ids, effect, op) {
                    run_int_event_command(ctx, ev, chain[5], &call.script_args, call.ret_form);
                    return Ok(true);
                }
                return Ok(false);
            }
            return Ok(get_or_set_effect_scalar(
                ctx,
                effect,
                &ids,
                op,
                call.al_id,
                call.rhs.as_ref(),
                call.ret_form,
            ));
        }

        if selector == ids.screen_sel_quake {
            if chain.len() < 4 || chain[2] != ids.elm_array {
                return Ok(false);
            }
            let idx = chain[3].max(0) as usize;
            st.ensure_quake_len(idx + 1);
            let quake = &mut st.quake_list[idx];
            if chain.len() == 4 {
                if call.ret_form != 0 {
                    ctx.stack.push(default_for_ret_form(call.ret_form));
                }
                return Ok(true);
            }
            return Ok(parse_quake_command(quake, chain[4], &call.script_args, ctx));
        }

        if selector == ids.screen_sel_shake {
            if !call.script_args.is_empty() {
                st.shake.set_ms(call.script_args[0].as_i64().unwrap_or(0));
                ctx.stack.push(Value::Int(0));
                return Ok(true);
            }
            return Ok(false);
        }

        if let Some(effect_op) = screen_to_effect_prop(&ids, selector) {
            st.ensure_effect_len(1);
            let effect = &mut st.effect_list[0];
            return Ok(get_or_set_effect_scalar(
                ctx,
                effect,
                &ids,
                effect_op,
                call.al_id,
                call.rhs.as_ref(),
                call.ret_form,
            ));
        }

        if let Some(effect_event_op) = screen_to_effect_event(&ids, selector) {
            if chain.len() != 3 {
                return Ok(false);
            }
            st.ensure_effect_len(1);
            let effect = &mut st.effect_list[0];
            if let Some(ev) = effect_event_mut(&ids, effect, effect_event_op) {
                run_int_event_command(ctx, ev, chain[2], &call.script_args, call.ret_form);
                return Ok(true);
            }
            return Ok(false);
        }

        Ok(false)
    })();
    ctx.globals.screen_forms.insert(form_id, st);
    result
}
