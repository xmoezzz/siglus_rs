use anyhow::Result;

use crate::runtime::{CommandContext, Value};

use super::prop_access;

const SET_AUTO_SAVEPOINT_OFF: i32 = 0;
const SET_AUTO_SAVEPOINT_ON: i32 = 1;
const SET_SKIP_DISABLE: i32 = 2;
const SET_SKIP_ENABLE: i32 = 3;
const SET_SKIP_DISABLE_FLAG: i32 = 4;
const GET_SKIP_DISABLE_FLAG: i32 = 5;
const SET_CTRL_SKIP_DISABLE: i32 = 6;
const SET_CTRL_SKIP_ENABLE: i32 = 7;
const SET_CTRL_SKIP_DISABLE_FLAG: i32 = 8;
const GET_CTRL_SKIP_DISABLE_FLAG: i32 = 9;
const CHECK_SKIP: i32 = 10;
const SET_STOP_SKIP_BY_KEY_DISABLE: i32 = 11;
const SET_STOP_SKIP_BY_KEY_ENABLE: i32 = 12;
const SET_END_MSG_BY_KEY_DISABLE: i32 = 13;
const SET_END_MSG_BY_KEY_ENABLE: i32 = 14;
const SET_SKIP_UNREAD_MESSAGE_FLAG: i32 = 15;
const GET_SKIP_UNREAD_MESSAGE_FLAG: i32 = 16;
const START_AUTO_MODE: i32 = 17;
const END_AUTO_MODE: i32 = 18;
const SET_AUTO_MODE_MOJI_WAIT: i32 = 19;
const SET_AUTO_MODE_MOJI_WAIT_DEFAULT: i32 = 20;
const GET_AUTO_MODE_MOJI_WAIT: i32 = 21;
const SET_AUTO_MODE_MIN_WAIT: i32 = 22;
const SET_AUTO_MODE_MIN_WAIT_DEFAULT: i32 = 23;
const GET_AUTO_MODE_MIN_WAIT: i32 = 24;
const SET_AUTO_MODE_MOJI_CNT: i32 = 25;
const SET_MOUSE_CURSOR_HIDE_ONOFF: i32 = 26;
const SET_MOUSE_CURSOR_HIDE_ONOFF_DEFAULT: i32 = 27;
const GET_MOUSE_CURSOR_HIDE_ONOFF: i32 = 28;
const SET_MOUSE_CURSOR_HIDE_TIME: i32 = 29;
const SET_MOUSE_CURSOR_HIDE_TIME_DEFAULT: i32 = 30;
const GET_MOUSE_CURSOR_HIDE_TIME: i32 = 31;
const SET_MESSAGE_SPEED: i32 = 32;
const SET_MESSAGE_SPEED_DEFAULT: i32 = 33;
const GET_MESSAGE_SPEED: i32 = 34;
const SET_MESSAGE_NOWAIT_FLAG: i32 = 35;
const GET_MESSAGE_NOWAIT_FLAG: i32 = 36;
const SET_MSG_ASYNC_MODE_ON: i32 = 37;
const SET_MSG_ASYNC_MODE_ON_ONCE: i32 = 38;
const SET_MSG_ASYNC_MODE_OFF: i32 = 39;
const SET_HIDE_MWND_DISABLE: i32 = 40;
const SET_HIDE_MWND_ENABLE: i32 = 41;
const SET_MSG_BACK_DISABLE: i32 = 42;
const SET_MSG_BACK_ENABLE: i32 = 43;
const SET_MSG_BACK_OFF: i32 = 44;
const SET_MSG_BACK_ON: i32 = 45;
const SET_MSG_BACK_DISP_OFF: i32 = 46;
const SET_MSG_BACK_DISP_ON: i32 = 47;
const SET_MOUSE_DISP_OFF: i32 = 48;
const SET_MOUSE_DISP_ON: i32 = 49;
const SET_MOUSE_MOVE_BY_KEY_DISABLE: i32 = 50;
const SET_MOUSE_MOVE_BY_KEY_ENABLE: i32 = 51;
const SET_KEY_DISABLE: i32 = 52;
const SET_KEY_ENABLE: i32 = 53;
const SET_MWND_ANIME_OFF_FLAG: i32 = 54;
const GET_MWND_ANIME_OFF_FLAG: i32 = 55;
const SET_MWND_ANIME_ON_FLAG: i32 = 56;
const GET_MWND_ANIME_ON_FLAG: i32 = 57;
const SET_MWND_DISP_OFF_FLAG: i32 = 58;
const GET_MWND_DISP_OFF_FLAG: i32 = 59;
const SET_KOE_DONT_STOP_ON_FLAG: i32 = 60;
const GET_KOE_DONT_STOP_ON_FLAG: i32 = 61;
const SET_KOE_DONT_STOP_OFF_FLAG: i32 = 62;
const GET_KOE_DONT_STOP_OFF_FLAG: i32 = 63;
const SET_SHORTCUT_ENABLE: i32 = 64;
const SET_SHORTCUT_DISABLE: i32 = 65;
const SET_QUAKE_STOP_FLAG: i32 = 66;
const GET_QUAKE_STOP_FLAG: i32 = 67;
const SET_EMOTE_MOUTH_STOP_FLAG: i32 = 68;
const GET_EMOTE_MOUTH_STOP_FLAG: i32 = 69;
const START_BGMFADE: i32 = 70;
const END_BGMFADE: i32 = 71;
const SET_VSYNC_WAIT_OFF_FLAG: i32 = 72;
const GET_VSYNC_WAIT_OFF_FLAG: i32 = 73;
const SET_SKIP_TRIGGER: i32 = 74;
const IGNORE_R_ON: i32 = 75;
const IGNORE_R_OFF: i32 = 76;
const SET_CURSOR_NO: i32 = 77;
const GET_CURSOR_NO: i32 = 78;
const SET_TIME_STOP_FLAG: i32 = 79;
const GET_TIME_STOP_FLAG: i32 = 80;
const SET_COUNTER_TIME_STOP_FLAG: i32 = 81;
const GET_COUNTER_TIME_STOP_FLAG: i32 = 82;
const SET_FRAME_ACTION_TIME_STOP_FLAG: i32 = 83;
const GET_FRAME_ACTION_TIME_STOP_FLAG: i32 = 84;
const SET_STAGE_TIME_STOP_FLAG: i32 = 85;
const GET_STAGE_TIME_STOP_FLAG: i32 = 86;
const SET_FONT_NAME: i32 = 87;
const SET_FONT_NAME_DEFAULT: i32 = 88;
const GET_FONT_NAME: i32 = 89;
const SET_FONT_BOLD: i32 = 90;
const SET_FONT_BOLD_DEFAULT: i32 = 91;
const GET_FONT_BOLD: i32 = 92;
const SET_FONT_SHADOW: i32 = 93;
const SET_FONT_SHADOW_DEFAULT: i32 = 94;
const GET_FONT_SHADOW: i32 = 95;

struct Call<'a> {
    op: i32,
    params: &'a [Value],
}

fn parse_call(form_id: u32, args: &[Value]) -> Option<Call<'_>> {
    if let Some((chain_pos, chain)) = prop_access::parse_element_chain(form_id, args) {
        if chain.len() >= 2 {
            let params = if chain_pos > 1 {
                &args[1..chain_pos]
            } else {
                &[]
            };
            return Some(Call {
                op: chain[1],
                params,
            });
        }
    }
    let op = args.get(0).and_then(|v| v.as_i64())? as i32;
    Some(Call {
        op,
        params: &args[1..],
    })
}

fn p_i64(params: &[Value], idx: usize) -> i64 {
    params.get(idx).and_then(|v| v.as_i64()).unwrap_or(0)
}

fn p_bool(params: &[Value], idx: usize) -> bool {
    p_i64(params, idx) != 0
}

fn p_str(params: &[Value], idx: usize) -> String {
    params
        .get(idx)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let Some(call) = parse_call(form_id, args) else {
        return Ok(false);
    };
    let op = call.op;
    let params = call.params;
    let st = &mut ctx.globals.script;

    match op {
        SET_AUTO_SAVEPOINT_OFF => st.dont_set_save_point = true,
        SET_AUTO_SAVEPOINT_ON => st.dont_set_save_point = false,
        SET_SKIP_DISABLE => st.skip_disable = true,
        SET_SKIP_ENABLE => st.skip_disable = false,
        SET_SKIP_DISABLE_FLAG => st.skip_disable = p_bool(params, 0),
        GET_SKIP_DISABLE_FLAG => {
            let v = if st.skip_disable { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_CTRL_SKIP_DISABLE => st.ctrl_disable = true,
        SET_CTRL_SKIP_ENABLE => st.ctrl_disable = false,
        SET_CTRL_SKIP_DISABLE_FLAG => st.ctrl_disable = p_bool(params, 0),
        GET_CTRL_SKIP_DISABLE_FLAG => {
            let v = if st.ctrl_disable { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        CHECK_SKIP => {
            let v = !st.skip_disable && (st.skip_trigger || st.auto_mode_flag || st.msg_nowait);
            ctx.push(Value::Int(if v { 1 } else { 0 }));
            return Ok(true);
        }
        SET_STOP_SKIP_BY_KEY_DISABLE => st.not_stop_skip_by_click = true,
        SET_STOP_SKIP_BY_KEY_ENABLE => st.not_stop_skip_by_click = false,
        SET_END_MSG_BY_KEY_DISABLE => st.not_skip_msg_by_click = true,
        SET_END_MSG_BY_KEY_ENABLE => st.not_skip_msg_by_click = false,
        SET_SKIP_UNREAD_MESSAGE_FLAG => st.skip_unread_message = p_bool(params, 0),
        GET_SKIP_UNREAD_MESSAGE_FLAG => {
            let v = if st.skip_unread_message { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        START_AUTO_MODE => st.auto_mode_flag = true,
        END_AUTO_MODE => st.auto_mode_flag = false,
        SET_AUTO_MODE_MOJI_WAIT => st.auto_mode_moji_wait = p_i64(params, 0),
        SET_AUTO_MODE_MOJI_WAIT_DEFAULT => st.auto_mode_moji_wait = -1,
        GET_AUTO_MODE_MOJI_WAIT => {
            let v = st.auto_mode_moji_wait;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_AUTO_MODE_MIN_WAIT => st.auto_mode_min_wait = p_i64(params, 0),
        SET_AUTO_MODE_MIN_WAIT_DEFAULT => st.auto_mode_min_wait = -1,
        GET_AUTO_MODE_MIN_WAIT => {
            let v = st.auto_mode_min_wait;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_AUTO_MODE_MOJI_CNT => st.auto_mode_moji_cnt = p_i64(params, 0),
        SET_MOUSE_CURSOR_HIDE_ONOFF => st.mouse_cursor_hide_onoff = p_i64(params, 0),
        SET_MOUSE_CURSOR_HIDE_ONOFF_DEFAULT => st.mouse_cursor_hide_onoff = -1,
        GET_MOUSE_CURSOR_HIDE_ONOFF => {
            let v = st.mouse_cursor_hide_onoff;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_MOUSE_CURSOR_HIDE_TIME => st.mouse_cursor_hide_time = p_i64(params, 0),
        SET_MOUSE_CURSOR_HIDE_TIME_DEFAULT => st.mouse_cursor_hide_time = -1,
        GET_MOUSE_CURSOR_HIDE_TIME => {
            let v = st.mouse_cursor_hide_time;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_MESSAGE_SPEED => st.msg_speed = p_i64(params, 0),
        SET_MESSAGE_SPEED_DEFAULT => st.msg_speed = -1,
        GET_MESSAGE_SPEED => {
            let v = st.msg_speed;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_MESSAGE_NOWAIT_FLAG => st.msg_nowait = p_bool(params, 0),
        GET_MESSAGE_NOWAIT_FLAG => {
            let v = if st.msg_nowait { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_MSG_ASYNC_MODE_ON => {
            st.async_msg_mode = true;
            st.async_msg_mode_once = false;
        }
        SET_MSG_ASYNC_MODE_ON_ONCE => {
            st.async_msg_mode = true;
            st.async_msg_mode_once = true;
        }
        SET_MSG_ASYNC_MODE_OFF => {
            st.async_msg_mode = false;
            st.async_msg_mode_once = false;
        }
        SET_HIDE_MWND_DISABLE => st.hide_mwnd_disable = true,
        SET_HIDE_MWND_ENABLE => st.hide_mwnd_disable = false,
        SET_MSG_BACK_DISABLE => st.msg_back_disable = true,
        SET_MSG_BACK_ENABLE => st.msg_back_disable = false,
        SET_MSG_BACK_OFF => st.msg_back_off = true,
        SET_MSG_BACK_ON => st.msg_back_off = false,
        SET_MSG_BACK_DISP_OFF => st.msg_back_disp_off = true,
        SET_MSG_BACK_DISP_ON => st.msg_back_disp_off = false,
        SET_MOUSE_DISP_OFF => st.cursor_disp_off = true,
        SET_MOUSE_DISP_ON => st.cursor_disp_off = false,
        SET_MOUSE_MOVE_BY_KEY_DISABLE => st.cursor_move_by_key_disable = true,
        SET_MOUSE_MOVE_BY_KEY_ENABLE => st.cursor_move_by_key_disable = false,
        SET_KEY_DISABLE => {
            let vk = p_i64(params, 0);
            if (0..=255).contains(&vk) {
                st.key_disable.insert(vk as u8);
            }
        }
        SET_KEY_ENABLE => {
            let vk = p_i64(params, 0);
            if (0..=255).contains(&vk) {
                st.key_disable.remove(&(vk as u8));
            }
        }
        SET_MWND_ANIME_OFF_FLAG => st.mwnd_anime_off_flag = p_bool(params, 0),
        GET_MWND_ANIME_OFF_FLAG => {
            let v = if st.mwnd_anime_off_flag { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_MWND_ANIME_ON_FLAG => st.mwnd_anime_on_flag = p_bool(params, 0),
        GET_MWND_ANIME_ON_FLAG => {
            let v = if st.mwnd_anime_on_flag { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_MWND_DISP_OFF_FLAG => st.mwnd_disp_off_flag = p_bool(params, 0),
        GET_MWND_DISP_OFF_FLAG => {
            let v = if st.mwnd_disp_off_flag { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_KOE_DONT_STOP_ON_FLAG => st.koe_dont_stop_on_flag = p_bool(params, 0),
        GET_KOE_DONT_STOP_ON_FLAG => {
            let v = if st.koe_dont_stop_on_flag { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_KOE_DONT_STOP_OFF_FLAG => st.koe_dont_stop_off_flag = p_bool(params, 0),
        GET_KOE_DONT_STOP_OFF_FLAG => {
            let v = if st.koe_dont_stop_off_flag { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_SHORTCUT_ENABLE => st.shortcut_disable = false,
        SET_SHORTCUT_DISABLE => st.shortcut_disable = true,
        SET_QUAKE_STOP_FLAG => st.quake_stop_flag = p_bool(params, 0),
        GET_QUAKE_STOP_FLAG => {
            let v = if st.quake_stop_flag { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_EMOTE_MOUTH_STOP_FLAG => st.emote_mouth_stop_flag = p_bool(params, 0),
        GET_EMOTE_MOUTH_STOP_FLAG => {
            let v = if st.emote_mouth_stop_flag { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        START_BGMFADE => st.bgmfade_flag = true,
        END_BGMFADE => st.bgmfade_flag = false,
        SET_VSYNC_WAIT_OFF_FLAG => st.wait_display_vsync_off_flag = p_bool(params, 0),
        GET_VSYNC_WAIT_OFF_FLAG => {
            let v = if st.wait_display_vsync_off_flag { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_SKIP_TRIGGER => st.skip_trigger = true,
        IGNORE_R_ON => st.ignore_r_flag = true,
        IGNORE_R_OFF => st.ignore_r_flag = false,
        SET_CURSOR_NO => st.cursor_no = p_i64(params, 0),
        GET_CURSOR_NO => {
            let v = st.cursor_no;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_TIME_STOP_FLAG => st.time_stop_flag = p_bool(params, 0),
        GET_TIME_STOP_FLAG => {
            let v = if st.time_stop_flag { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_COUNTER_TIME_STOP_FLAG => st.counter_time_stop_flag = p_bool(params, 0),
        GET_COUNTER_TIME_STOP_FLAG => {
            let v = if st.counter_time_stop_flag { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_FRAME_ACTION_TIME_STOP_FLAG => st.frame_action_time_stop_flag = p_bool(params, 0),
        GET_FRAME_ACTION_TIME_STOP_FLAG => {
            let v = if st.frame_action_time_stop_flag { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_STAGE_TIME_STOP_FLAG => st.stage_time_stop_flag = p_bool(params, 0),
        GET_STAGE_TIME_STOP_FLAG => {
            let v = if st.stage_time_stop_flag { 1 } else { 0 };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_FONT_NAME => st.font_name = p_str(params, 0),
        SET_FONT_NAME_DEFAULT => st.font_name.clear(),
        GET_FONT_NAME => {
            let v = st.font_name.clone();
            ctx.push(Value::Str(v));
            return Ok(true);
        }
        SET_FONT_BOLD => st.font_bold = p_i64(params, 0),
        SET_FONT_BOLD_DEFAULT => st.font_bold = -1,
        GET_FONT_BOLD => {
            let v = st.font_bold;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_FONT_SHADOW => st.font_shadow = p_i64(params, 0),
        SET_FONT_SHADOW_DEFAULT => st.font_shadow = -1,
        GET_FONT_SHADOW => {
            let v = st.font_shadow;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        _ => {
            prop_access::dispatch_stateful_form(ctx, form_id, args);
            return Ok(true);
        }
    }

    ctx.push(Value::Int(0));
    Ok(true)
}
