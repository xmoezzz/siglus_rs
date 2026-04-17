use anyhow::Result;

use crate::runtime::globals::{SaveSlotState, ToggleFeatureState, ValueFeatureState};
use crate::runtime::{CommandContext, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::assets::RgbaImage;

use super::prop_access;

// Default op numbering inferred from the original the original implementation switch order.
const CALL_EX: i32 = 0;
const CALL_SYSCOM_MENU: i32 = 1;
const SET_SYSCOM_MENU_ENABLE: i32 = 2;
const SET_SYSCOM_MENU_DISABLE: i32 = 7;
const SET_MWND_BTN_ENABLE: i32 = 4;
const SET_MWND_BTN_DISABLE: i32 = 5;
const SET_MWND_BTN_TOUCH_ENABLE: i32 = 6;
const SET_MWND_BTN_TOUCH_DISABLE: i32 = 7;
const INIT_SYSCOM_FLAG: i32 = 8;
const SET_READ_SKIP_ONOFF_FLAG: i32 = 9;
const GET_READ_SKIP_ONOFF_FLAG: i32 = 10;
const SET_READ_SKIP_ENABLE_FLAG: i32 = 11;
const GET_READ_SKIP_ENABLE_FLAG: i32 = 12;
const SET_READ_SKIP_EXIST_FLAG: i32 = 13;
const GET_READ_SKIP_EXIST_FLAG: i32 = 14;
const CHECK_READ_SKIP_ENABLE: i32 = 15;
const SET_AUTO_SKIP_ONOFF_FLAG: i32 = 16;
const GET_AUTO_SKIP_ONOFF_FLAG: i32 = 17;
const SET_AUTO_SKIP_ENABLE_FLAG: i32 = 18;
const GET_AUTO_SKIP_ENABLE_FLAG: i32 = 19;
const SET_AUTO_SKIP_EXIST_FLAG: i32 = 20;
const GET_AUTO_SKIP_EXIST_FLAG: i32 = 21;
const CHECK_AUTO_SKIP_ENABLE: i32 = 22;
const SET_AUTO_MODE_ONOFF_FLAG: i32 = 23;
const GET_AUTO_MODE_ONOFF_FLAG: i32 = 24;
const SET_AUTO_MODE_ENABLE_FLAG: i32 = 25;
const GET_AUTO_MODE_ENABLE_FLAG: i32 = 26;
const SET_AUTO_MODE_EXIST_FLAG: i32 = 27;
const GET_AUTO_MODE_EXIST_FLAG: i32 = 28;
const CHECK_AUTO_MODE_ENABLE: i32 = 29;
const SET_HIDE_MWND_ONOFF_FLAG: i32 = 30;
const GET_HIDE_MWND_ONOFF_FLAG: i32 = 222;
const SET_HIDE_MWND_ENABLE_FLAG: i32 = 223;
const GET_HIDE_MWND_ENABLE_FLAG: i32 = 33;
const SET_HIDE_MWND_EXIST_FLAG: i32 = 34;
const GET_HIDE_MWND_EXIST_FLAG: i32 = 35;
const CHECK_HIDE_MWND_ENABLE: i32 = 36;
const SET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG: i32 = 37;
const GET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG: i32 = 38;
const SET_LOCAL_EXTRA_SWITCH_ENABLE_FLAG: i32 = 39;
const GET_LOCAL_EXTRA_SWITCH_ENABLE_FLAG: i32 = 40;
const SET_LOCAL_EXTRA_SWITCH_EXIST_FLAG: i32 = 41;
const GET_LOCAL_EXTRA_SWITCH_EXIST_FLAG: i32 = 42;
const CHECK_LOCAL_EXTRA_SWITCH_ENABLE: i32 = 43;
const SET_LOCAL_EXTRA_MODE_VALUE: i32 = 44;
const GET_LOCAL_EXTRA_MODE_VALUE: i32 = 45;
const SET_LOCAL_EXTRA_MODE_ENABLE_FLAG: i32 = 46;
const GET_LOCAL_EXTRA_MODE_ENABLE_FLAG: i32 = 47;
const SET_LOCAL_EXTRA_MODE_EXIST_FLAG: i32 = 48;
const GET_LOCAL_EXTRA_MODE_EXIST_FLAG: i32 = 49;
const CHECK_LOCAL_EXTRA_MODE_ENABLE: i32 = 50;
const OPEN_MSG_BACK: i32 = 51;
const CLOSE_MSG_BACK: i32 = 52;
const SET_MSG_BACK_ENABLE_FLAG: i32 = 53;
const GET_MSG_BACK_ENABLE_FLAG: i32 = 54;
const SET_MSG_BACK_EXIST_FLAG: i32 = 55;
const GET_MSG_BACK_EXIST_FLAG: i32 = 56;
const CHECK_MSG_BACK_ENABLE: i32 = 57;
const CHECK_MSG_BACK_OPEN: i32 = 58;
const RETURN_TO_SEL: i32 = 59;
const SET_RETURN_TO_SEL_ENABLE_FLAG: i32 = 60;
const GET_RETURN_TO_SEL_ENABLE_FLAG: i32 = 61;
const SET_RETURN_TO_SEL_EXIST_FLAG: i32 = 62;
const GET_RETURN_TO_SEL_EXIST_FLAG: i32 = 63;
const CHECK_RETURN_TO_SEL_ENABLE: i32 = 64;
const RETURN_TO_MENU: i32 = 65;
const SET_RETURN_TO_MENU_ENABLE_FLAG: i32 = 66;
const GET_RETURN_TO_MENU_ENABLE_FLAG: i32 = 67;
const SET_RETURN_TO_MENU_EXIST_FLAG: i32 = 68;
const GET_RETURN_TO_MENU_EXIST_FLAG: i32 = 69;
const CHECK_RETURN_TO_MENU_ENABLE: i32 = 70;
const END_GAME: i32 = 71;
const SET_END_GAME_ENABLE_FLAG: i32 = 72;
const GET_END_GAME_ENABLE_FLAG: i32 = 73;
const SET_END_GAME_EXIST_FLAG: i32 = 74;
const GET_END_GAME_EXIST_FLAG: i32 = 75;
const CHECK_END_GAME_ENABLE: i32 = 76;
const REPLAY_KOE: i32 = 77;
const CHECK_REPLAY_KOE: i32 = 78;
const GET_REPLAY_KOE_KOE_NO: i32 = 79;
const GET_REPLAY_KOE_CHARA_NO: i32 = 80;
const CLEAR_REPLAY_KOE: i32 = 81;
const GET_CURRENT_SAVE_SCENE_TITLE: i32 = 82;
const GET_CURRENT_SAVE_MESSAGE: i32 = 83;
const GET_TOTAL_PLAY_TIME: i32 = 84;
const SET_TOTAL_PLAY_TIME: i32 = 85;
const CALL_SAVE_MENU: i32 = 86;
const SET_SAVE_ENABLE_FLAG: i32 = 87;
const GET_SAVE_ENABLE_FLAG: i32 = 88;
const SET_SAVE_EXIST_FLAG: i32 = 89;
const GET_SAVE_EXIST_FLAG: i32 = 90;
const CHECK_SAVE_ENABLE: i32 = 91;
const CALL_LOAD_MENU: i32 = 92;
const SET_LOAD_ENABLE_FLAG: i32 = 93;
const GET_LOAD_ENABLE_FLAG: i32 = 94;
const SET_LOAD_EXIST_FLAG: i32 = 95;
const GET_LOAD_EXIST_FLAG: i32 = 96;
const CHECK_LOAD_ENABLE: i32 = 97;
const SAVE: i32 = 98;
const LOAD: i32 = 99;
const QUICK_SAVE: i32 = 100;
const QUICK_LOAD: i32 = 101;
const END_SAVE: i32 = 102;
const END_LOAD: i32 = 103;
const INNER_SAVE: i32 = 104;
const INNER_LOAD: i32 = 105;
const CLEAR_INNER_SAVE: i32 = 106;
const COPY_INNER_SAVE: i32 = 107;
const CHECK_INNER_SAVE: i32 = 108;
const MSG_BACK_LOAD: i32 = 109;
const GET_SAVE_CNT: i32 = 110;
const GET_QUICK_SAVE_CNT: i32 = 111;
const GET_SAVE_NEW_NO: i32 = 112;
const GET_QUICK_SAVE_NEW_NO: i32 = 113;
const GET_SAVE_EXIST: i32 = 114;
const GET_SAVE_YEAR: i32 = 115;
const GET_SAVE_MONTH: i32 = 116;
const GET_SAVE_DAY: i32 = 117;
const GET_SAVE_WEEKDAY: i32 = 118;
const GET_SAVE_HOUR: i32 = 119;
const GET_SAVE_MINUTE: i32 = 120;
const GET_SAVE_SECOND: i32 = 121;
const GET_SAVE_MILLISECOND: i32 = 122;
const GET_SAVE_TITLE: i32 = 123;
const GET_SAVE_MESSAGE: i32 = 124;
const GET_SAVE_FULL_MESSAGE: i32 = 125;
const GET_SAVE_COMMENT: i32 = 126;
const SET_SAVE_COMMENT: i32 = 127;
const GET_SAVE_VALUE: i32 = 128;
const SET_SAVE_VALUE: i32 = 129;
const GET_SAVE_APPEND_DIR: i32 = 130;
const GET_SAVE_APPEND_NAME: i32 = 131;
const GET_QUICK_SAVE_EXIST: i32 = 132;
const GET_QUICK_SAVE_YEAR: i32 = 133;
const GET_QUICK_SAVE_MONTH: i32 = 134;
const GET_QUICK_SAVE_DAY: i32 = 135;
const GET_QUICK_SAVE_WEEKDAY: i32 = 136;
const GET_QUICK_SAVE_HOUR: i32 = 137;
const GET_QUICK_SAVE_MINUTE: i32 = 138;
const GET_QUICK_SAVE_SECOND: i32 = 139;
const GET_QUICK_SAVE_MILLISECOND: i32 = 140;
const GET_QUICK_SAVE_TITLE: i32 = 141;
const GET_QUICK_SAVE_MESSAGE: i32 = 142;
const GET_QUICK_SAVE_FULL_MESSAGE: i32 = 143;
const GET_QUICK_SAVE_COMMENT: i32 = 144;
const SET_QUICK_SAVE_COMMENT: i32 = 145;
const GET_QUICK_SAVE_VALUE: i32 = 146;
const SET_QUICK_SAVE_VALUE: i32 = 147;
const GET_QUICK_SAVE_APPEND_DIR: i32 = 148;
const GET_QUICK_SAVE_APPEND_NAME: i32 = 149;
const GET_END_SAVE_EXIST: i32 = 150;
const COPY_SAVE: i32 = 151;
const CHANGE_SAVE: i32 = 152;
const DELETE_SAVE: i32 = 153;
const COPY_QUICK_SAVE: i32 = 154;
const CHANGE_QUICK_SAVE: i32 = 155;
const DELETE_QUICK_SAVE: i32 = 156;
const CALL_CONFIG_MENU: i32 = 157;
const CALL_CONFIG_WINDOW_MODE_MENU: i32 = 158;
const CALL_CONFIG_VOLUME_MENU: i32 = 159;
const CALL_CONFIG_BGMFADE_MENU: i32 = 160;
const CALL_CONFIG_KOEMODE_MENU: i32 = 161;
const CALL_CONFIG_CHARAKOE_MENU: i32 = 162;
const CALL_CONFIG_JITAN_MENU: i32 = 163;
const CALL_CONFIG_MESSAGE_SPEED_MENU: i32 = 164;
const CALL_CONFIG_FILTER_COLOR_MENU: i32 = 165;
const CALL_CONFIG_AUTO_MODE_MENU: i32 = 166;
const CALL_CONFIG_FONT_MENU: i32 = 167;
const CALL_CONFIG_SYSTEM_MENU: i32 = 168;
const CALL_CONFIG_MOVIE_MENU: i32 = 169;
const SET_WINDOW_MODE: i32 = 170;
const SET_WINDOW_MODE_DEFAULT: i32 = 171;
const GET_WINDOW_MODE: i32 = 172;
const SET_WINDOW_MODE_SIZE: i32 = 173;
const SET_WINDOW_MODE_SIZE_DEFAULT: i32 = 174;
const GET_WINDOW_MODE_SIZE: i32 = 175;
const CHECK_WINDOW_MODE_SIZE_ENABLE: i32 = 176;
const SET_ALL_VOLUME: i32 = 177;
const SET_BGM_VOLUME: i32 = 178;
const SET_KOE_VOLUME: i32 = 179;
const SET_PCM_VOLUME: i32 = 180;
const SET_SE_VOLUME: i32 = 181;
const SET_MOV_VOLUME: i32 = 182;
const SET_SOUND_VOLUME: i32 = 183;
const SET_ALL_ONOFF: i32 = 184;
const SET_BGM_ONOFF: i32 = 185;
const SET_KOE_ONOFF: i32 = 186;
const SET_PCM_ONOFF: i32 = 187;
const SET_SE_ONOFF: i32 = 188;
const SET_MOV_ONOFF: i32 = 189;
const SET_SOUND_ONOFF: i32 = 190;
const SET_ALL_VOLUME_DEFAULT: i32 = 191;
const SET_BGM_VOLUME_DEFAULT: i32 = 192;
const SET_KOE_VOLUME_DEFAULT: i32 = 193;
const SET_PCM_VOLUME_DEFAULT: i32 = 194;
const SET_SE_VOLUME_DEFAULT: i32 = 195;
const SET_MOV_VOLUME_DEFAULT: i32 = 196;
const SET_SOUND_VOLUME_DEFAULT: i32 = 197;
const SET_ALL_ONOFF_DEFAULT: i32 = 198;
const SET_BGM_ONOFF_DEFAULT: i32 = 199;
const SET_KOE_ONOFF_DEFAULT: i32 = 200;
const SET_PCM_ONOFF_DEFAULT: i32 = 201;
const SET_SE_ONOFF_DEFAULT: i32 = 202;
const SET_MOV_ONOFF_DEFAULT: i32 = 203;
const SET_SOUND_ONOFF_DEFAULT: i32 = 204;
const GET_ALL_VOLUME: i32 = 205;
const GET_BGM_VOLUME: i32 = 206;
const GET_KOE_VOLUME: i32 = 207;
const GET_PCM_VOLUME: i32 = 208;
const GET_SE_VOLUME: i32 = 209;
const GET_MOV_VOLUME: i32 = 210;
const GET_SOUND_VOLUME: i32 = 211;
const GET_ALL_ONOFF: i32 = 212;
const GET_BGM_ONOFF: i32 = 213;
const GET_KOE_ONOFF: i32 = 214;
const GET_PCM_ONOFF: i32 = 215;
const GET_SE_ONOFF: i32 = 216;
const GET_MOV_ONOFF: i32 = 217;
const GET_SOUND_ONOFF: i32 = 218;
const SET_BGMFADE_VOLUME: i32 = 219;
const SET_BGMFADE_ONOFF: i32 = 220;
const SET_BGMFADE_VOLUME_DEFAULT: i32 = 221;
const SET_BGMFADE_ONOFF_DEFAULT: i32 = 222;
const GET_BGMFADE_VOLUME: i32 = 223;
const GET_BGMFADE_ONOFF: i32 = 224;
const SET_KOEMODE: i32 = 225;
const SET_KOEMODE_DEFAULT: i32 = 226;
const GET_KOEMODE: i32 = 227;
const SET_CHARAKOE_ONOFF: i32 = 228;
const SET_CHARAKOE_ONOFF_DEFAULT: i32 = 229;
const GET_CHARAKOE_ONOFF: i32 = 230;
const SET_CHARAKOE_VOLUME: i32 = 231;
const SET_CHARAKOE_VOLUME_DEFAULT: i32 = 232;
const GET_CHARAKOE_VOLUME: i32 = 233;
const SET_JITAN_NORMAL_ONOFF: i32 = 234;
const SET_JITAN_NORMAL_ONOFF_DEFAULT: i32 = 235;
const GET_JITAN_NORMAL_ONOFF: i32 = 236;
const SET_JITAN_AUTO_MODE_ONOFF: i32 = 237;
const SET_JITAN_AUTO_MODE_ONOFF_DEFAULT: i32 = 238;
const GET_JITAN_AUTO_MODE_ONOFF: i32 = 239;
const SET_JITAN_KOE_REPLAY_ONOFF: i32 = 240;
const SET_JITAN_KOE_REPLAY_ONOFF_DEFAULT: i32 = 241;
const GET_JITAN_KOE_REPLAY_ONOFF: i32 = 242;
const SET_JITAN_SPEED: i32 = 243;
const SET_JITAN_SPEED_DEFAULT: i32 = 244;
const GET_JITAN_SPEED: i32 = 245;
const SET_MESSAGE_SPEED: i32 = 246;
const SET_MESSAGE_SPEED_DEFAULT: i32 = 247;
const GET_MESSAGE_SPEED: i32 = 248;
const SET_MESSAGE_NOWAIT: i32 = 249;
const SET_MESSAGE_NOWAIT_DEFAULT: i32 = 250;
const GET_MESSAGE_NOWAIT: i32 = 251;
const SET_AUTO_MODE_MOJI_WAIT: i32 = 252;
const SET_AUTO_MODE_MOJI_WAIT_DEFAULT: i32 = 253;
const GET_AUTO_MODE_MOJI_WAIT: i32 = 254;
const SET_AUTO_MODE_MIN_WAIT: i32 = 255;
const SET_AUTO_MODE_MIN_WAIT_DEFAULT: i32 = 256;
const GET_AUTO_MODE_MIN_WAIT: i32 = 257;
const SET_MOUSE_CURSOR_HIDE_ONOFF: i32 = 258;
const SET_MOUSE_CURSOR_HIDE_ONOFF_DEFAULT: i32 = 259;
const GET_MOUSE_CURSOR_HIDE_ONOFF: i32 = 260;
const SET_MOUSE_CURSOR_HIDE_TIME: i32 = 261;
const SET_MOUSE_CURSOR_HIDE_TIME_DEFAULT: i32 = 262;
const GET_MOUSE_CURSOR_HIDE_TIME: i32 = 263;
const SET_FILTER_COLOR_R: i32 = 264;
const SET_FILTER_COLOR_G: i32 = 265;
const SET_FILTER_COLOR_B: i32 = 266;
const SET_FILTER_COLOR_A: i32 = 267;
const SET_FILTER_COLOR_R_DEFAULT: i32 = 268;
const SET_FILTER_COLOR_G_DEFAULT: i32 = 269;
const SET_FILTER_COLOR_B_DEFAULT: i32 = 270;
const SET_FILTER_COLOR_A_DEFAULT: i32 = 271;
const GET_FILTER_COLOR_R: i32 = 272;
const GET_FILTER_COLOR_G: i32 = 273;
const GET_FILTER_COLOR_B: i32 = 274;
const GET_FILTER_COLOR_A: i32 = 275;
const SET_OBJECT_DISP_ONOFF: i32 = 276;
const SET_OBJECT_DISP_ONOFF_DEFAULT: i32 = 277;
const GET_OBJECT_DISP_ONOFF: i32 = 278;
const SET_GLOBAL_EXTRA_SWITCH_ONOFF: i32 = 279;
const SET_GLOBAL_EXTRA_SWITCH_ONOFF_DEFAULT: i32 = 280;
const GET_GLOBAL_EXTRA_SWITCH_ONOFF: i32 = 281;
const SET_GLOBAL_EXTRA_MODE_VALUE: i32 = 282;
const SET_GLOBAL_EXTRA_MODE_VALUE_DEFAULT: i32 = 283;
const GET_GLOBAL_EXTRA_MODE_VALUE: i32 = 284;
const SET_SAVELOAD_ALERT_ONOFF: i32 = 285;
const SET_SAVELOAD_ALERT_ONOFF_DEFAULT: i32 = 286;
const GET_SAVELOAD_ALERT_ONOFF: i32 = 287;
const SET_SAVELOAD_DBLCLICK_ONOFF: i32 = 288;
const SET_SAVELOAD_DBLCLICK_ONOFF_DEFAULT: i32 = 289;
const GET_SAVELOAD_DBLCLICK_ONOFF: i32 = 290;
const SET_SLEEP_ONOFF: i32 = 291;
const SET_SLEEP_ONOFF_DEFAULT: i32 = 292;
const GET_SLEEP_ONOFF: i32 = 293;
const SET_NO_WIPE_ANIME_ONOFF: i32 = 294;
const SET_NO_WIPE_ANIME_ONOFF_DEFAULT: i32 = 295;
const GET_NO_WIPE_ANIME_ONOFF: i32 = 296;
const SET_SKIP_WIPE_ANIME_ONOFF: i32 = 297;
const SET_SKIP_WIPE_ANIME_ONOFF_DEFAULT: i32 = 298;
const GET_SKIP_WIPE_ANIME_ONOFF: i32 = 299;
const SET_NO_MWND_ANIME_ONOFF: i32 = 300;
const SET_NO_MWND_ANIME_ONOFF_DEFAULT: i32 = 301;
const GET_NO_MWND_ANIME_ONOFF: i32 = 302;
const SET_WHEEL_NEXT_MESSAGE_ONOFF: i32 = 303;
const SET_WHEEL_NEXT_MESSAGE_ONOFF_DEFAULT: i32 = 304;
const GET_WHEEL_NEXT_MESSAGE_ONOFF: i32 = 305;
const SET_KOE_DONT_STOP_ONOFF: i32 = 306;
const SET_KOE_DONT_STOP_ONOFF_DEFAULT: i32 = 307;
const GET_KOE_DONT_STOP_ONOFF: i32 = 308;
const SET_SKIP_UNREAD_MESSAGE_ONOFF: i32 = 309;
const SET_SKIP_UNREAD_MESSAGE_ONOFF_DEFAULT: i32 = 310;
const GET_SKIP_UNREAD_MESSAGE_ONOFF: i32 = 311;
const SET_PLAY_SILENT_SOUND_ONOFF: i32 = 312;
const SET_PLAY_SILENT_SOUND_ONOFF_DEFAULT: i32 = 313;
const GET_PLAY_SILENT_SOUND_ONOFF: i32 = 314;
const SET_FONT_NAME: i32 = 315;
const SET_FONT_NAME_DEFAULT: i32 = 316;
const GET_FONT_NAME: i32 = 317;
const IS_FONT_EXIST: i32 = 318;
const SET_FONT_BOLD: i32 = 319;
const SET_FONT_BOLD_DEFAULT: i32 = 320;
const GET_FONT_BOLD: i32 = 321;
const SET_FONT_DECORATION: i32 = 322;
const SET_FONT_DECORATION_DEFAULT: i32 = 323;
const GET_FONT_DECORATION: i32 = 324;
const CREATE_CAPTURE_BUFFER: i32 = 325;
const DESTROY_CAPTURE_BUFFER: i32 = 326;
const CAPTURE_TO_CAPTURE_BUFFER: i32 = 327;
const SAVE_CAPTURE_BUFFER_TO_FILE: i32 = 328;
const LOAD_FLAG_FROM_CAPTURE_FILE: i32 = 329;
const CAPTURE_AND_SAVE_BUFFER_TO_PNG: i32 = 330;
const OPEN_TWEET_DIALOG: i32 = 331;
const SET_RETURN_SCENE_ONCE: i32 = 332;
const GET_SYSTEM_EXTRA_INT_VALUE: i32 = 333;
const GET_SYSTEM_EXTRA_STR_VALUE: i32 = 334;

struct Call<'a> {
    op: i32,
    params: &'a [Value],
}

fn parse_call<'a>(ctx: &CommandContext, form_id: u32, args: &'a [Value]) -> Option<Call<'a>> {
    let (chain_pos, chain) = prop_access::parse_element_chain_ctx(ctx, form_id, args)?;
    if chain.len() < 2 {
        return None;
    }
    let params = prop_access::script_args(args, chain_pos);
    Some(Call {
        op: chain[1],
        params,
    })
}

fn p_i64(params: &[Value], idx: usize) -> i64 {
    params.get(idx).and_then(|v| v.as_i64()).unwrap_or(0)
}
fn p_bool(params: &[Value], idx: usize) -> bool {
    p_i64(params, idx) != 0
}

fn get_toggle_get(op: i32, st: &crate::runtime::globals::SyscomRuntimeState) -> Option<i64> {
    Some(match op {
        GET_READ_SKIP_ONOFF_FLAG => {
            if st.read_skip.onoff {
                1
            } else {
                0
            }
        }
        GET_READ_SKIP_ENABLE_FLAG => {
            if st.read_skip.enable {
                1
            } else {
                0
            }
        }
        GET_READ_SKIP_EXIST_FLAG => {
            if st.read_skip.exist {
                1
            } else {
                0
            }
        }
        CHECK_READ_SKIP_ENABLE => st.read_skip.check_enabled(),
        GET_AUTO_SKIP_ONOFF_FLAG => {
            if st.auto_skip.onoff {
                1
            } else {
                0
            }
        }
        GET_AUTO_SKIP_ENABLE_FLAG => {
            if st.auto_skip.enable {
                1
            } else {
                0
            }
        }
        GET_AUTO_SKIP_EXIST_FLAG => {
            if st.auto_skip.exist {
                1
            } else {
                0
            }
        }
        CHECK_AUTO_SKIP_ENABLE => st.auto_skip.check_enabled(),
        GET_AUTO_MODE_ONOFF_FLAG => {
            if st.auto_mode.onoff {
                1
            } else {
                0
            }
        }
        GET_AUTO_MODE_ENABLE_FLAG => {
            if st.auto_mode.enable {
                1
            } else {
                0
            }
        }
        GET_AUTO_MODE_EXIST_FLAG => {
            if st.auto_mode.exist {
                1
            } else {
                0
            }
        }
        CHECK_AUTO_MODE_ENABLE => st.auto_mode.check_enabled(),
        GET_HIDE_MWND_ONOFF_FLAG => {
            if st.hide_mwnd.onoff {
                1
            } else {
                0
            }
        }
        GET_HIDE_MWND_ENABLE_FLAG => {
            if st.hide_mwnd.enable {
                1
            } else {
                0
            }
        }
        GET_HIDE_MWND_EXIST_FLAG => {
            if st.hide_mwnd.exist {
                1
            } else {
                0
            }
        }
        CHECK_HIDE_MWND_ENABLE => st.hide_mwnd.check_enabled(),
        GET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG => {
            if st.local_extra_switch.onoff {
                1
            } else {
                0
            }
        }
        GET_LOCAL_EXTRA_SWITCH_ENABLE_FLAG => {
            if st.local_extra_switch.enable {
                1
            } else {
                0
            }
        }
        GET_LOCAL_EXTRA_SWITCH_EXIST_FLAG => {
            if st.local_extra_switch.exist {
                1
            } else {
                0
            }
        }
        CHECK_LOCAL_EXTRA_SWITCH_ENABLE => st.local_extra_switch.check_enabled(),
        GET_LOCAL_EXTRA_MODE_VALUE => st.local_extra_mode.value,
        GET_LOCAL_EXTRA_MODE_ENABLE_FLAG => {
            if st.local_extra_mode.enable {
                1
            } else {
                0
            }
        }
        GET_LOCAL_EXTRA_MODE_EXIST_FLAG => {
            if st.local_extra_mode.exist {
                1
            } else {
                0
            }
        }
        CHECK_LOCAL_EXTRA_MODE_ENABLE => st.local_extra_mode.check_enabled(),
        GET_MSG_BACK_ENABLE_FLAG => {
            if st.msg_back.enable {
                1
            } else {
                0
            }
        }
        GET_MSG_BACK_EXIST_FLAG => {
            if st.msg_back.exist {
                1
            } else {
                0
            }
        }
        CHECK_MSG_BACK_ENABLE => st.msg_back.check_enabled(),
        CHECK_MSG_BACK_OPEN => {
            if st.msg_back_open {
                1
            } else {
                0
            }
        }
        GET_RETURN_TO_SEL_ENABLE_FLAG => {
            if st.return_to_sel.enable {
                1
            } else {
                0
            }
        }
        GET_RETURN_TO_SEL_EXIST_FLAG => {
            if st.return_to_sel.exist {
                1
            } else {
                0
            }
        }
        CHECK_RETURN_TO_SEL_ENABLE => st.return_to_sel.check_enabled(),
        GET_RETURN_TO_MENU_ENABLE_FLAG => {
            if st.return_to_menu.enable {
                1
            } else {
                0
            }
        }
        GET_RETURN_TO_MENU_EXIST_FLAG => {
            if st.return_to_menu.exist {
                1
            } else {
                0
            }
        }
        CHECK_RETURN_TO_MENU_ENABLE => st.return_to_menu.check_enabled(),
        GET_END_GAME_ENABLE_FLAG => {
            if st.end_game.enable {
                1
            } else {
                0
            }
        }
        GET_END_GAME_EXIST_FLAG => {
            if st.end_game.exist {
                1
            } else {
                0
            }
        }
        CHECK_END_GAME_ENABLE => st.end_game.check_enabled(),
        GET_SAVE_ENABLE_FLAG => {
            if st.save_feature.enable {
                1
            } else {
                0
            }
        }
        GET_SAVE_EXIST_FLAG => {
            if st.save_feature.exist {
                1
            } else {
                0
            }
        }
        CHECK_SAVE_ENABLE => st.save_feature.check_enabled(),
        GET_LOAD_ENABLE_FLAG => {
            if st.load_feature.enable {
                1
            } else {
                0
            }
        }
        GET_LOAD_EXIST_FLAG => {
            if st.load_feature.exist {
                1
            } else {
                0
            }
        }
        CHECK_LOAD_ENABLE => st.load_feature.check_enabled(),
        _ => return None,
    })
}

fn apply_toggle_set(
    op: i32,
    v: bool,
    st: &mut crate::runtime::globals::SyscomRuntimeState,
) -> bool {
    match op {
        SET_READ_SKIP_ONOFF_FLAG => st.read_skip.onoff = v,
        SET_READ_SKIP_ENABLE_FLAG => st.read_skip.enable = v,
        SET_READ_SKIP_EXIST_FLAG => st.read_skip.exist = v,
        SET_AUTO_SKIP_ONOFF_FLAG => st.auto_skip.onoff = v,
        SET_AUTO_SKIP_ENABLE_FLAG => st.auto_skip.enable = v,
        SET_AUTO_SKIP_EXIST_FLAG => st.auto_skip.exist = v,
        SET_AUTO_MODE_ONOFF_FLAG => st.auto_mode.onoff = v,
        SET_AUTO_MODE_ENABLE_FLAG => st.auto_mode.enable = v,
        SET_AUTO_MODE_EXIST_FLAG => st.auto_mode.exist = v,
        SET_HIDE_MWND_ONOFF_FLAG => st.hide_mwnd.onoff = v,
        SET_HIDE_MWND_ENABLE_FLAG => st.hide_mwnd.enable = v,
        SET_HIDE_MWND_EXIST_FLAG => st.hide_mwnd.exist = v,
        SET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG => st.local_extra_switch.onoff = v,
        SET_LOCAL_EXTRA_SWITCH_ENABLE_FLAG => st.local_extra_switch.enable = v,
        SET_LOCAL_EXTRA_SWITCH_EXIST_FLAG => st.local_extra_switch.exist = v,
        SET_LOCAL_EXTRA_MODE_ENABLE_FLAG => st.local_extra_mode.enable = v,
        SET_LOCAL_EXTRA_MODE_EXIST_FLAG => st.local_extra_mode.exist = v,
        SET_MSG_BACK_ENABLE_FLAG => st.msg_back.enable = v,
        SET_MSG_BACK_EXIST_FLAG => st.msg_back.exist = v,
        SET_RETURN_TO_SEL_ENABLE_FLAG => st.return_to_sel.enable = v,
        SET_RETURN_TO_SEL_EXIST_FLAG => st.return_to_sel.exist = v,
        SET_RETURN_TO_MENU_ENABLE_FLAG => st.return_to_menu.enable = v,
        SET_RETURN_TO_MENU_EXIST_FLAG => st.return_to_menu.exist = v,
        SET_END_GAME_ENABLE_FLAG => st.end_game.enable = v,
        SET_END_GAME_EXIST_FLAG => st.end_game.exist = v,
        SET_SAVE_ENABLE_FLAG => st.save_feature.enable = v,
        SET_SAVE_EXIST_FLAG => st.save_feature.exist = v,
        SET_LOAD_ENABLE_FLAG => st.load_feature.enable = v,
        SET_LOAD_EXIST_FLAG => st.load_feature.exist = v,
        _ => return false,
    }
    true
}

fn ensure_slot(slots: &mut Vec<SaveSlotState>, idx: usize) -> &mut SaveSlotState {
    if slots.len() <= idx {
        slots.resize_with(idx + 1, SaveSlotState::default);
    }
    &mut slots[idx]
}

pub(crate) fn menu_save_slot(ctx: &mut CommandContext, quick: bool, idx: usize) {
    let slot = if quick {
        ensure_slot(&mut ctx.globals.syscom.quick_save_slots, idx)
    } else {
        ensure_slot(&mut ctx.globals.syscom.save_slots, idx)
    };
    slot.exist = true;
    set_slot_timestamp(slot);
    let path = slot_path(&save_dir(&ctx.project_dir), quick, idx);
    write_slot(&path, slot);
    if !quick {
        write_slot_thumb(ctx, idx);
    }
}

pub(crate) fn menu_load_slot(ctx: &mut CommandContext, quick: bool, idx: usize) {
    if quick {
        ensure_slot_loaded(
            &ctx.project_dir,
            true,
            &mut ctx.globals.syscom.quick_save_slots,
            idx,
        );
    } else {
        ensure_slot_loaded(
            &ctx.project_dir,
            false,
            &mut ctx.globals.syscom.save_slots,
            idx,
        );
    }
}

pub(crate) fn save_dir(project_dir: &Path) -> PathBuf {
    let cand = project_dir.join("savedata");
    if cand.is_dir() {
        return cand;
    }
    let cand = project_dir.join("save");
    if cand.is_dir() {
        return cand;
    }
    project_dir.join("savedata")
}

fn slot_path(dir: &Path, quick: bool, idx: usize) -> PathBuf {
    if quick {
        dir.join(format!("qsave_{idx}.txt"))
    } else {
        dir.join(format!("save_{idx}.txt"))
    }
}

pub(crate) fn thumb_candidate_paths(dir: &Path, idx: i64) -> [PathBuf; 2] {
    let stem = format!("{:010}", idx.max(0));
    [
        dir.join(format!("{stem}.png")),
        dir.join(format!("{stem}.bmp")),
    ]
}

fn pick_thumb_source_name(ctx: &CommandContext) -> Option<String> {
    let table = ctx.tables.thumb_table.as_ref()?;
    for stage in ctx.globals.stage_forms.values() {
        for objs in stage.object_lists.values() {
            for obj in objs.iter().rev() {
                if let Some(file) = obj.file_name.as_deref() {
                    if let Some(mapped) = table.get_by_file_stem(file) {
                        return Some(mapped.clone());
                    }
                }
            }
        }
    }
    None
}

fn capture_slot_thumb(ctx: &mut CommandContext) -> RgbaImage {
    const SAVE_THUMB_W: u32 = 200;
    const SAVE_THUMB_H: u32 = 150;

    if let Some(name) = pick_thumb_source_name(ctx) {
        if let Ok(img_id) = ctx.images.load_g00(&name, 0) {
            if let Some(img) = ctx.images.get(img_id) {
                return resize_rgba(img.as_ref(), SAVE_THUMB_W, SAVE_THUMB_H);
            }
        }
    }

    let img = ctx.capture_frame_rgba();
    resize_rgba(&img, SAVE_THUMB_W, SAVE_THUMB_H)
}

fn write_slot_thumb(ctx: &mut CommandContext, idx: usize) {
    let dir = save_dir(&ctx.project_dir);
    let [png_path, _bmp_path] = thumb_candidate_paths(&dir, idx as i64);
    if let Some(parent) = png_path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let img = capture_slot_thumb(ctx);
    write_rgba_png(&png_path, &img);
}

fn escape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn unescape_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(ch) = it.next() {
        if ch == '\\' {
            match it.next() {
                Some('n') => out.push('\n'),
                Some('r') => out.push('\r'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some(other) => out.push(other),
                None => break,
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn write_slot(path: &Path, slot: &SaveSlotState) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut buf = String::new();
    buf.push_str("version=1\n");
    buf.push_str(&format!("exist={}\n", if slot.exist { 1 } else { 0 }));
    buf.push_str(&format!("year={}\n", slot.year));
    buf.push_str(&format!("month={}\n", slot.month));
    buf.push_str(&format!("day={}\n", slot.day));
    buf.push_str(&format!("weekday={}\n", slot.weekday));
    buf.push_str(&format!("hour={}\n", slot.hour));
    buf.push_str(&format!("minute={}\n", slot.minute));
    buf.push_str(&format!("second={}\n", slot.second));
    buf.push_str(&format!("millisecond={}\n", slot.millisecond));
    buf.push_str(&format!("title={}\n", escape_str(&slot.title)));
    buf.push_str(&format!("message={}\n", escape_str(&slot.message)));
    buf.push_str(&format!(
        "full_message={}\n",
        escape_str(&slot.full_message)
    ));
    buf.push_str(&format!("comment={}\n", escape_str(&slot.comment)));
    buf.push_str(&format!("append_dir={}\n", escape_str(&slot.append_dir)));
    buf.push_str(&format!("append_name={}\n", escape_str(&slot.append_name)));
    for (k, v) in &slot.values {
        buf.push_str(&format!("val.{k}={v}\n"));
    }
    let _ = std::fs::write(path, buf);
}

fn read_slot(path: &Path) -> Option<SaveSlotState> {
    let data = std::fs::read_to_string(path).ok()?;
    let mut slot = SaveSlotState::default();
    for line in data.lines() {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "exist" => slot.exist = v.trim() != "0",
            "year" => slot.year = v.trim().parse().unwrap_or(0),
            "month" => slot.month = v.trim().parse().unwrap_or(0),
            "day" => slot.day = v.trim().parse().unwrap_or(0),
            "weekday" => slot.weekday = v.trim().parse().unwrap_or(0),
            "hour" => slot.hour = v.trim().parse().unwrap_or(0),
            "minute" => slot.minute = v.trim().parse().unwrap_or(0),
            "second" => slot.second = v.trim().parse().unwrap_or(0),
            "millisecond" => slot.millisecond = v.trim().parse().unwrap_or(0),
            "title" => slot.title = unescape_str(v),
            "message" => slot.message = unescape_str(v),
            "full_message" => slot.full_message = unescape_str(v),
            "comment" => slot.comment = unescape_str(v),
            "append_dir" => slot.append_dir = unescape_str(v),
            "append_name" => slot.append_name = unescape_str(v),
            _ if k.starts_with("val.") => {
                let key = k.trim_start_matches("val.").parse::<i32>().unwrap_or(0);
                let val = v.trim().parse::<i64>().unwrap_or(0);
                slot.values.insert(key, val);
            }
            _ => {}
        }
    }
    Some(slot)
}

fn ensure_slot_loaded(project_dir: &Path, quick: bool, slots: &mut Vec<SaveSlotState>, idx: usize) {
    if slots.get(idx).map(|s| s.exist).unwrap_or(false) {
        return;
    }
    let path = slot_path(&save_dir(project_dir), quick, idx);
    if let Some(slot) = read_slot(&path) {
        let s = ensure_slot(slots, idx);
        *s = slot;
    }
}

fn write_msg_back(ctx: &CommandContext) {
    let form_id = ctx.ids.form_global_msgbk;
    if form_id == 0 {
        return;
    }
    let Some(st) = ctx.globals.msgbk_forms.get(&form_id) else {
        return;
    };
    let dir = save_dir(&ctx.project_dir);
    let path = dir.join("msg_back.txt");

    let mut out = String::new();
    for (i, entry) in st.history.iter().enumerate() {
        out.push_str(&format!("-- entry {} --\n", i));
        if !entry.original_name.is_empty() || !entry.disp_name.is_empty() {
            out.push_str("NAME: ");
            out.push_str(&entry.disp_name);
            out.push('\n');
        }
        if !entry.msg_str.is_empty() {
            out.push_str("TEXT: ");
            out.push_str(&entry.msg_str);
            out.push('\n');
        }
        for (koe_no, chara_no) in entry.koe_no_list.iter().zip(entry.chr_no_list.iter()) {
            out.push_str(&format!("KOE: {} {}\n", koe_no, chara_no));
        }
        if entry.scn_no >= 0 || entry.line_no >= 0 {
            out.push_str(&format!("SCENE_LINE: {} {}\n", entry.scn_no, entry.line_no));
        }
        out.push('\n');
    }
    let _ = std::fs::write(path, out);
}

fn first_free_slot(slots: &[SaveSlotState]) -> i64 {
    for (i, s) in slots.iter().enumerate() {
        if !s.exist {
            return i as i64;
        }
    }
    slots.len() as i64
}

fn set_slot_timestamp(slot: &mut SaveSlotState) {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64;
    let days = secs.div_euclid(86_400);
    let sod = secs.rem_euclid(86_400);
    slot.hour = sod / 3600;
    slot.minute = (sod % 3600) / 60;
    slot.second = sod % 60;
    slot.weekday = (days + 4).rem_euclid(7);
    slot.year = 1970;
    slot.month = 1;
    slot.day = 1 + days;
    slot.millisecond = 0;
}

fn slot_i64(slot: &SaveSlotState, op: i32) -> i64 {
    match op {
        GET_SAVE_EXIST | GET_QUICK_SAVE_EXIST => {
            if slot.exist {
                1
            } else {
                0
            }
        }
        GET_SAVE_YEAR | GET_QUICK_SAVE_YEAR => slot.year,
        GET_SAVE_MONTH | GET_QUICK_SAVE_MONTH => slot.month,
        GET_SAVE_DAY | GET_QUICK_SAVE_DAY => slot.day,
        GET_SAVE_WEEKDAY | GET_QUICK_SAVE_WEEKDAY => slot.weekday,
        GET_SAVE_HOUR | GET_QUICK_SAVE_HOUR => slot.hour,
        GET_SAVE_MINUTE | GET_QUICK_SAVE_MINUTE => slot.minute,
        GET_SAVE_SECOND | GET_QUICK_SAVE_SECOND => slot.second,
        GET_SAVE_MILLISECOND | GET_QUICK_SAVE_MILLISECOND => slot.millisecond,
        _ => 0,
    }
}

fn slot_str(slot: &SaveSlotState, op: i32) -> String {
    match op {
        GET_SAVE_TITLE | GET_QUICK_SAVE_TITLE => slot.title.clone(),
        GET_SAVE_MESSAGE | GET_QUICK_SAVE_MESSAGE => slot.message.clone(),
        GET_SAVE_FULL_MESSAGE | GET_QUICK_SAVE_FULL_MESSAGE => slot.full_message.clone(),
        GET_SAVE_COMMENT | GET_QUICK_SAVE_COMMENT => slot.comment.clone(),
        GET_SAVE_APPEND_DIR | GET_QUICK_SAVE_APPEND_DIR => slot.append_dir.clone(),
        GET_SAVE_APPEND_NAME | GET_QUICK_SAVE_APPEND_NAME => slot.append_name.clone(),
        _ => String::new(),
    }
}

fn cfg_get_int(st: &crate::runtime::globals::SyscomRuntimeState, key: i32, default: i64) -> i64 {
    st.config_int.get(&key).copied().unwrap_or(default)
}

fn cfg_set_int(st: &mut crate::runtime::globals::SyscomRuntimeState, key: i32, value: i64) {
    st.config_int.insert(key, value);
}

fn volume_to_raw(v: i64) -> u8 {
    let v = v.clamp(0, 100);
    ((v * 255) / 100) as u8
}

pub(crate) fn apply_audio_config(ctx: &mut CommandContext) {
    use crate::audio::TrackKind;
    let all_vol = cfg_get_int(&ctx.globals.syscom, GET_ALL_VOLUME, 100);
    let all_on = cfg_get_int(&ctx.globals.syscom, GET_ALL_ONOFF, 1) != 0;
    let all_raw = if all_on { volume_to_raw(all_vol) } else { 0 };

    let bgm_vol = cfg_get_int(&ctx.globals.syscom, GET_BGM_VOLUME, 100);
    let bgm_on = cfg_get_int(&ctx.globals.syscom, GET_BGM_ONOFF, 1) != 0;
    let bgm_raw = if bgm_on { volume_to_raw(bgm_vol) } else { 0 };

    let se_vol = cfg_get_int(&ctx.globals.syscom, GET_SE_VOLUME, 100);
    let se_on = cfg_get_int(&ctx.globals.syscom, GET_SE_ONOFF, 1) != 0;
    let se_raw = if se_on { volume_to_raw(se_vol) } else { 0 };

    let pcm_vol = cfg_get_int(&ctx.globals.syscom, GET_PCM_VOLUME, 100);
    let pcm_on = cfg_get_int(&ctx.globals.syscom, GET_PCM_ONOFF, 1) != 0;
    let pcm_raw = if pcm_on { volume_to_raw(pcm_vol) } else { 0 };

    let koe_vol = cfg_get_int(&ctx.globals.syscom, GET_KOE_VOLUME, 100);
    let koe_on = cfg_get_int(&ctx.globals.syscom, GET_KOE_ONOFF, 1) != 0;
    let koe_raw = if koe_on { volume_to_raw(koe_vol) } else { 0 };

    let mov_vol = cfg_get_int(&ctx.globals.syscom, GET_MOV_VOLUME, 100);
    let mov_on = cfg_get_int(&ctx.globals.syscom, GET_MOV_ONOFF, 1) != 0;
    let mov_raw = if mov_on { volume_to_raw(mov_vol) } else { 0 };

    let eff_bgm = (all_raw as u16 * bgm_raw as u16 / 255) as u8;
    let eff_se = (all_raw as u16 * se_raw as u16 / 255) as u8;
    let eff_pcm = (all_raw as u16 * pcm_raw as u16 / 255) as u8;
    let eff_koe = (all_raw as u16 * koe_raw as u16 / 255) as u8;
    let eff_mov = (all_raw as u16 * mov_raw as u16 / 255) as u8;

    ctx.audio
        .set_track_master_volume_raw(TrackKind::Bgm, eff_bgm);
    ctx.audio.set_track_master_volume_raw(TrackKind::Se, eff_se);
    // KOE is treated as PCM for now (voice).
    ctx.audio
        .set_track_master_volume_raw(TrackKind::Pcm, eff_pcm.min(eff_koe));
    ctx.audio
        .set_track_master_volume_raw(TrackKind::Mov, eff_mov);
}

fn cfg_get_str(st: &crate::runtime::globals::SyscomRuntimeState, key: i32) -> String {
    st.config_str.get(&key).cloned().unwrap_or_default()
}

fn cfg_set_str(st: &mut crate::runtime::globals::SyscomRuntimeState, key: i32, value: String) {
    st.config_str.insert(key, value);
}

fn join_game_path(base: &Path, raw: &str) -> PathBuf {
    if raw.is_empty() {
        return base.to_path_buf();
    }
    let norm = raw.replace('\\', "/");
    let p = Path::new(&norm);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    }
}

fn write_rgba_png(path: &Path, img: &RgbaImage) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Some(buf) = image::RgbaImage::from_raw(img.width, img.height, img.rgba.clone()) {
        let _ = buf.save(path);
    }
}

fn resize_rgba(img: &RgbaImage, w: u32, h: u32) -> RgbaImage {
    if img.width == 0 || img.height == 0 || w == 0 || h == 0 {
        return img.clone();
    }
    if img.width == w && img.height == h {
        return img.clone();
    }
    let mut out = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        let src_y = (y as u64 * img.height as u64 / h as u64) as u32;
        for x in 0..w {
            let src_x = (x as u64 * img.width as u64 / w as u64) as u32;
            let si = ((src_y * img.width + src_x) * 4) as usize;
            let di = ((y * w + x) * 4) as usize;
            out[di..di + 4].copy_from_slice(&img.rgba[si..si + 4]);
        }
    }
    RgbaImage {
        width: w,
        height: h,
        rgba: out,
    }
}

fn font_exists(project_dir: &Path, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let font_dir = project_dir.join("font");
    let Ok(entries) = fs::read_dir(font_dir) else {
        return false;
    };
    let name_lower = name.to_ascii_lowercase();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext != "ttf" && ext != "otf" && ext != "ttc" {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if stem == name_lower {
            return true;
        }
    }
    false
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let Some(call) = parse_call(ctx, form_id, args) else {
        return Ok(false);
    };
    let op = call.op;
    let params = call.params;

    {
        let st = &ctx.globals.syscom;
        if let Some(v) = get_toggle_get(op, st) {
            ctx.push(Value::Int(v));
            return Ok(true);
        }
    }
    {
        let st = &mut ctx.globals.syscom;
        if apply_toggle_set(op, p_bool(params, 0), st) {
            ctx.push(Value::Int(0));
            return Ok(true);
        }
    }

    match op {
        CALL_EX => {
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        CALL_SYSCOM_MENU => {
            ctx.globals.syscom.menu_open = true;
            ctx.globals.syscom.menu_kind = Some(CALL_SYSCOM_MENU);
            ctx.globals.syscom.menu_result = None;
            ctx.globals.syscom.menu_cursor = 0;
            ctx.globals.syscom.last_menu_call = CALL_SYSCOM_MENU;
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        SET_SYSCOM_MENU_ENABLE => ctx.globals.syscom.syscom_menu_disable = false,
        SET_SYSCOM_MENU_DISABLE => ctx.globals.syscom.syscom_menu_disable = true,
        SET_MWND_BTN_ENABLE => {
            if params.is_empty() {
                ctx.globals.syscom.mwnd_btn_disable_all = false;
            } else {
                ctx.globals
                    .syscom
                    .mwnd_btn_disable
                    .insert(p_i64(params, 0), false);
            }
        }
        SET_MWND_BTN_DISABLE => {
            if params.is_empty() {
                ctx.globals.syscom.mwnd_btn_disable_all = true;
            } else {
                ctx.globals
                    .syscom
                    .mwnd_btn_disable
                    .insert(p_i64(params, 0), true);
            }
        }
        SET_MWND_BTN_TOUCH_ENABLE => ctx.globals.syscom.mwnd_btn_touch_disable = false,
        SET_MWND_BTN_TOUCH_DISABLE => ctx.globals.syscom.mwnd_btn_touch_disable = true,
        INIT_SYSCOM_FLAG => {
            ctx.globals.syscom.read_skip = ToggleFeatureState::default();
            ctx.globals.syscom.auto_skip = ToggleFeatureState::default();
            ctx.globals.syscom.auto_mode = ToggleFeatureState::default();
            ctx.globals.syscom.hide_mwnd = ToggleFeatureState::default();
            ctx.globals.syscom.local_extra_switch = ToggleFeatureState::default();
            ctx.globals.syscom.local_extra_mode = ValueFeatureState::default();
            ctx.globals.syscom.msg_back = ToggleFeatureState::default();
            ctx.globals.syscom.return_to_sel = ToggleFeatureState::default();
            ctx.globals.syscom.return_to_menu = ToggleFeatureState::default();
            ctx.globals.syscom.end_game = ToggleFeatureState::default();
            ctx.globals.syscom.save_feature = ToggleFeatureState::default();
            ctx.globals.syscom.load_feature = ToggleFeatureState::default();
            ctx.globals.syscom.msg_back_open = false;
        }
        SET_LOCAL_EXTRA_MODE_VALUE => ctx.globals.syscom.local_extra_mode.value = p_i64(params, 0),
        OPEN_MSG_BACK => ctx.globals.syscom.msg_back_open = true,
        CLOSE_MSG_BACK => ctx.globals.syscom.msg_back_open = false,
        RETURN_TO_SEL => ctx.globals.syscom.last_menu_call = RETURN_TO_SEL,
        RETURN_TO_MENU => ctx.globals.syscom.last_menu_call = RETURN_TO_MENU,
        END_GAME => ctx.globals.syscom.last_menu_call = END_GAME,
        REPLAY_KOE => ctx.globals.syscom.replay_koe = Some((p_i64(params, 0), p_i64(params, 1))),
        CHECK_REPLAY_KOE => {
            let v = if ctx.globals.syscom.replay_koe.is_some() {
                1
            } else {
                0
            };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_REPLAY_KOE_KOE_NO => {
            let v = ctx.globals.syscom.replay_koe.map(|v| v.0).unwrap_or(-1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_REPLAY_KOE_CHARA_NO => {
            let v = ctx.globals.syscom.replay_koe.map(|v| v.1).unwrap_or(-1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        CLEAR_REPLAY_KOE => ctx.globals.syscom.replay_koe = None,
        GET_CURRENT_SAVE_SCENE_TITLE => {
            let v = ctx.globals.syscom.current_save_scene_title.clone();
            ctx.push(Value::Str(v));
            return Ok(true);
        }
        GET_CURRENT_SAVE_MESSAGE => {
            let v = ctx.globals.syscom.current_save_message.clone();
            ctx.push(Value::Str(v));
            return Ok(true);
        }
        GET_TOTAL_PLAY_TIME => {
            let v = ctx.globals.syscom.total_play_time;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_TOTAL_PLAY_TIME => ctx.globals.syscom.total_play_time = p_i64(params, 0),
        CALL_SAVE_MENU => {
            ctx.globals.syscom.menu_open = true;
            ctx.globals.syscom.menu_kind = Some(CALL_SAVE_MENU);
            ctx.globals.syscom.menu_result = None;
            ctx.globals.syscom.menu_cursor = 0;
            ctx.globals.syscom.last_menu_call = CALL_SAVE_MENU;
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        CALL_LOAD_MENU => {
            ctx.globals.syscom.menu_open = true;
            ctx.globals.syscom.menu_kind = Some(CALL_LOAD_MENU);
            ctx.globals.syscom.menu_result = None;
            ctx.globals.syscom.menu_cursor = 0;
            ctx.globals.syscom.last_menu_call = CALL_LOAD_MENU;
            ctx.push(Value::Int(0));
            return Ok(true);
        }
        SAVE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let slot = ensure_slot(&mut ctx.globals.syscom.save_slots, idx);
            slot.exist = true;
            set_slot_timestamp(slot);
            let path = slot_path(&save_dir(&ctx.project_dir), false, idx);
            write_slot(&path, slot);
        }
        LOAD => {
            let idx = p_i64(params, 0).max(0) as usize;
            ensure_slot_loaded(
                &ctx.project_dir,
                false,
                &mut ctx.globals.syscom.save_slots,
                idx,
            );
            ctx.globals.syscom.last_menu_call = LOAD;
        }
        QUICK_SAVE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let slot = ensure_slot(&mut ctx.globals.syscom.quick_save_slots, idx);
            slot.exist = true;
            set_slot_timestamp(slot);
            let path = slot_path(&save_dir(&ctx.project_dir), true, idx);
            write_slot(&path, slot);
        }
        QUICK_LOAD => {
            let idx = p_i64(params, 0).max(0) as usize;
            ensure_slot_loaded(
                &ctx.project_dir,
                true,
                &mut ctx.globals.syscom.quick_save_slots,
                idx,
            );
            ctx.globals.syscom.last_menu_call = QUICK_LOAD;
        }
        END_SAVE => ctx.globals.syscom.end_save_exists = true,
        END_LOAD => ctx.globals.syscom.last_menu_call = END_LOAD,
        INNER_SAVE => ctx.globals.syscom.inner_save_exists = true,
        INNER_LOAD => ctx.globals.syscom.last_menu_call = INNER_LOAD,
        CLEAR_INNER_SAVE => ctx.globals.syscom.inner_save_exists = false,
        COPY_INNER_SAVE => ctx.globals.syscom.inner_save_exists = true,
        CHECK_INNER_SAVE => {
            let v = if ctx.globals.syscom.inner_save_exists {
                1
            } else {
                0
            };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        MSG_BACK_LOAD => {
            ctx.globals.syscom.last_menu_call = MSG_BACK_LOAD;
            write_msg_back(ctx);
        }
        GET_SAVE_CNT => {
            let v = ctx.globals.syscom.save_slots.len() as i64;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_QUICK_SAVE_CNT => {
            let v = ctx.globals.syscom.quick_save_slots.len() as i64;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_SAVE_NEW_NO => {
            let v = first_free_slot(&ctx.globals.syscom.save_slots);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_QUICK_SAVE_NEW_NO => {
            let v = first_free_slot(&ctx.globals.syscom.quick_save_slots);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_SAVE_EXIST | GET_SAVE_YEAR | GET_SAVE_MONTH | GET_SAVE_DAY | GET_SAVE_WEEKDAY
        | GET_SAVE_HOUR | GET_SAVE_MINUTE | GET_SAVE_SECOND | GET_SAVE_MILLISECOND => {
            let idx = p_i64(params, 0).max(0) as usize;
            ensure_slot_loaded(
                &ctx.project_dir,
                false,
                &mut ctx.globals.syscom.save_slots,
                idx,
            );
            let v = ctx
                .globals
                .syscom
                .save_slots
                .get(idx)
                .map(|s| slot_i64(s, op))
                .unwrap_or(0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_SAVE_TITLE
        | GET_SAVE_MESSAGE
        | GET_SAVE_FULL_MESSAGE
        | GET_SAVE_COMMENT
        | GET_SAVE_APPEND_DIR
        | GET_SAVE_APPEND_NAME => {
            let idx = p_i64(params, 0).max(0) as usize;
            ensure_slot_loaded(
                &ctx.project_dir,
                false,
                &mut ctx.globals.syscom.save_slots,
                idx,
            );
            let v = ctx
                .globals
                .syscom
                .save_slots
                .get(idx)
                .map(|s| slot_str(s, op))
                .unwrap_or_default();
            ctx.push(Value::Str(v));
            return Ok(true);
        }
        SET_SAVE_COMMENT => {
            let idx = p_i64(params, 0).max(0) as usize;
            let slot = ensure_slot(&mut ctx.globals.syscom.save_slots, idx);
            slot.comment = params
                .get(1)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }
        GET_SAVE_VALUE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let key = p_i64(params, 1) as i32;
            ensure_slot_loaded(
                &ctx.project_dir,
                false,
                &mut ctx.globals.syscom.save_slots,
                idx,
            );
            let v = ctx
                .globals
                .syscom
                .save_slots
                .get(idx)
                .and_then(|s| s.values.get(&key).copied())
                .unwrap_or(0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_SAVE_VALUE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let key = p_i64(params, 1) as i32;
            let val = p_i64(params, 2);
            ensure_slot(&mut ctx.globals.syscom.save_slots, idx)
                .values
                .insert(key, val);
        }
        GET_QUICK_SAVE_EXIST
        | GET_QUICK_SAVE_YEAR
        | GET_QUICK_SAVE_MONTH
        | GET_QUICK_SAVE_DAY
        | GET_QUICK_SAVE_WEEKDAY
        | GET_QUICK_SAVE_HOUR
        | GET_QUICK_SAVE_MINUTE
        | GET_QUICK_SAVE_SECOND
        | GET_QUICK_SAVE_MILLISECOND => {
            let idx = p_i64(params, 0).max(0) as usize;
            ensure_slot_loaded(
                &ctx.project_dir,
                true,
                &mut ctx.globals.syscom.quick_save_slots,
                idx,
            );
            let v = ctx
                .globals
                .syscom
                .quick_save_slots
                .get(idx)
                .map(|s| slot_i64(s, op))
                .unwrap_or(0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_QUICK_SAVE_TITLE
        | GET_QUICK_SAVE_MESSAGE
        | GET_QUICK_SAVE_FULL_MESSAGE
        | GET_QUICK_SAVE_COMMENT
        | GET_QUICK_SAVE_APPEND_DIR
        | GET_QUICK_SAVE_APPEND_NAME => {
            let idx = p_i64(params, 0).max(0) as usize;
            ensure_slot_loaded(
                &ctx.project_dir,
                true,
                &mut ctx.globals.syscom.quick_save_slots,
                idx,
            );
            let v = ctx
                .globals
                .syscom
                .quick_save_slots
                .get(idx)
                .map(|s| slot_str(s, op))
                .unwrap_or_default();
            ctx.push(Value::Str(v));
            return Ok(true);
        }
        SET_QUICK_SAVE_COMMENT => {
            let idx = p_i64(params, 0).max(0) as usize;
            let slot = ensure_slot(&mut ctx.globals.syscom.quick_save_slots, idx);
            slot.comment = params
                .get(1)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
        }
        GET_QUICK_SAVE_VALUE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let key = p_i64(params, 1) as i32;
            ensure_slot_loaded(
                &ctx.project_dir,
                true,
                &mut ctx.globals.syscom.quick_save_slots,
                idx,
            );
            let v = ctx
                .globals
                .syscom
                .quick_save_slots
                .get(idx)
                .and_then(|s| s.values.get(&key).copied())
                .unwrap_or(0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_QUICK_SAVE_VALUE => {
            let idx = p_i64(params, 0).max(0) as usize;
            let key = p_i64(params, 1) as i32;
            let val = p_i64(params, 2);
            ensure_slot(&mut ctx.globals.syscom.quick_save_slots, idx)
                .values
                .insert(key, val);
        }
        GET_END_SAVE_EXIST => {
            let v = if ctx.globals.syscom.end_save_exists {
                1
            } else {
                0
            };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        COPY_SAVE | CHANGE_SAVE | DELETE_SAVE | COPY_QUICK_SAVE | CHANGE_QUICK_SAVE
        | DELETE_QUICK_SAVE => {
            ctx.globals.syscom.last_menu_call = op;
        }
        CALL_CONFIG_MENU
        | CALL_CONFIG_WINDOW_MODE_MENU
        | CALL_CONFIG_VOLUME_MENU
        | CALL_CONFIG_BGMFADE_MENU
        | CALL_CONFIG_KOEMODE_MENU
        | CALL_CONFIG_CHARAKOE_MENU
        | CALL_CONFIG_JITAN_MENU
        | CALL_CONFIG_MESSAGE_SPEED_MENU
        | CALL_CONFIG_FILTER_COLOR_MENU
        | CALL_CONFIG_AUTO_MODE_MENU
        | CALL_CONFIG_FONT_MENU
        | CALL_CONFIG_SYSTEM_MENU
        | CALL_CONFIG_MOVIE_MENU => {
            ctx.globals.syscom.menu_open = true;
            ctx.globals.syscom.menu_kind = Some(op);
            ctx.globals.syscom.menu_result = None;
            ctx.globals.syscom.menu_cursor = 0;
            ctx.globals.syscom.last_menu_call = op;
        }
        SET_WINDOW_MODE => cfg_set_int(&mut ctx.globals.syscom, GET_WINDOW_MODE, p_i64(params, 0)),
        SET_WINDOW_MODE_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_WINDOW_MODE, 0),
        GET_WINDOW_MODE => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_WINDOW_MODE, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_WINDOW_MODE_SIZE => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_WINDOW_MODE_SIZE,
            p_i64(params, 0),
        ),
        SET_WINDOW_MODE_SIZE_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_WINDOW_MODE_SIZE, 0)
        }
        GET_WINDOW_MODE_SIZE => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_WINDOW_MODE_SIZE, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        CHECK_WINDOW_MODE_SIZE_ENABLE => {
            ctx.push(Value::Int(1));
            return Ok(true);
        }
        SET_ALL_VOLUME => {
            cfg_set_int(&mut ctx.globals.syscom, GET_ALL_VOLUME, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_BGM_VOLUME => {
            cfg_set_int(&mut ctx.globals.syscom, GET_BGM_VOLUME, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_KOE_VOLUME => {
            cfg_set_int(&mut ctx.globals.syscom, GET_KOE_VOLUME, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_PCM_VOLUME => {
            cfg_set_int(&mut ctx.globals.syscom, GET_PCM_VOLUME, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_SE_VOLUME => {
            cfg_set_int(&mut ctx.globals.syscom, GET_SE_VOLUME, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_MOV_VOLUME => {
            cfg_set_int(&mut ctx.globals.syscom, GET_MOV_VOLUME, p_i64(params, 0));
            apply_audio_config(ctx);
        }
        SET_SOUND_VOLUME => {
            cfg_set_int(&mut ctx.globals.syscom, GET_SOUND_VOLUME, p_i64(params, 0));
        }
        SET_ALL_VOLUME_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_ALL_VOLUME, 100);
            apply_audio_config(ctx);
        }
        SET_BGM_VOLUME_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_BGM_VOLUME, 100);
            apply_audio_config(ctx);
        }
        SET_KOE_VOLUME_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_KOE_VOLUME, 100);
            apply_audio_config(ctx);
        }
        SET_PCM_VOLUME_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_PCM_VOLUME, 100);
            apply_audio_config(ctx);
        }
        SET_SE_VOLUME_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_SE_VOLUME, 100);
            apply_audio_config(ctx);
        }
        SET_MOV_VOLUME_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_MOV_VOLUME, 100);
            apply_audio_config(ctx);
        }
        SET_SOUND_VOLUME_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_SOUND_VOLUME, 100),
        GET_ALL_VOLUME | GET_BGM_VOLUME | GET_KOE_VOLUME | GET_PCM_VOLUME | GET_SE_VOLUME
        | GET_MOV_VOLUME | GET_SOUND_VOLUME => {
            let v = cfg_get_int(&ctx.globals.syscom, op, 100);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_ALL_ONOFF => {
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_ALL_ONOFF,
                if p_bool(params, 0) { 1 } else { 0 },
            );
            apply_audio_config(ctx);
        }
        SET_BGM_ONOFF => {
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_BGM_ONOFF,
                if p_bool(params, 0) { 1 } else { 0 },
            );
            apply_audio_config(ctx);
        }
        SET_KOE_ONOFF => {
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_KOE_ONOFF,
                if p_bool(params, 0) { 1 } else { 0 },
            );
            apply_audio_config(ctx);
        }
        SET_PCM_ONOFF => {
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_PCM_ONOFF,
                if p_bool(params, 0) { 1 } else { 0 },
            );
            apply_audio_config(ctx);
        }
        SET_SE_ONOFF => {
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_SE_ONOFF,
                if p_bool(params, 0) { 1 } else { 0 },
            );
            apply_audio_config(ctx);
        }
        SET_MOV_ONOFF => {
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_MOV_ONOFF,
                if p_bool(params, 0) { 1 } else { 0 },
            );
            apply_audio_config(ctx);
        }
        SET_SOUND_ONOFF => {
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_SOUND_ONOFF,
                if p_bool(params, 0) { 1 } else { 0 },
            );
        }
        SET_ALL_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_ALL_ONOFF, 1);
            apply_audio_config(ctx);
        }
        SET_BGM_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_BGM_ONOFF, 1);
            apply_audio_config(ctx);
        }
        SET_KOE_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_KOE_ONOFF, 1);
            apply_audio_config(ctx);
        }
        SET_PCM_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_PCM_ONOFF, 1);
            apply_audio_config(ctx);
        }
        SET_SE_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_SE_ONOFF, 1);
            apply_audio_config(ctx);
        }
        SET_MOV_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_MOV_ONOFF, 1);
            apply_audio_config(ctx);
        }
        SET_SOUND_ONOFF_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_SOUND_ONOFF, 1),
        GET_ALL_ONOFF | GET_BGM_ONOFF | GET_KOE_ONOFF | GET_PCM_ONOFF | GET_SE_ONOFF
        | GET_MOV_ONOFF | GET_SOUND_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, op, 1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_BGMFADE_VOLUME => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_BGMFADE_VOLUME,
            p_i64(params, 0),
        ),
        SET_BGMFADE_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_BGMFADE_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_BGMFADE_VOLUME_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_BGMFADE_VOLUME, 100),
        SET_BGMFADE_ONOFF_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_BGMFADE_ONOFF, 1),
        GET_BGMFADE_VOLUME | GET_BGMFADE_ONOFF => {
            let default = if op == GET_BGMFADE_ONOFF { 1 } else { 100 };
            let v = cfg_get_int(&ctx.globals.syscom, op, default);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_KOEMODE => cfg_set_int(&mut ctx.globals.syscom, GET_KOEMODE, p_i64(params, 0)),
        SET_KOEMODE_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_KOEMODE, 0),
        GET_KOEMODE => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_KOEMODE, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_CHARAKOE_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_CHARAKOE_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_CHARAKOE_ONOFF_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_CHARAKOE_ONOFF, 1),
        GET_CHARAKOE_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_CHARAKOE_ONOFF, 1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_CHARAKOE_VOLUME => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_CHARAKOE_VOLUME,
            p_i64(params, 0),
        ),
        SET_CHARAKOE_VOLUME_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_CHARAKOE_VOLUME, 100)
        }
        GET_CHARAKOE_VOLUME => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_CHARAKOE_VOLUME, 100);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_JITAN_NORMAL_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_JITAN_NORMAL_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_JITAN_NORMAL_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_JITAN_NORMAL_ONOFF, 0)
        }
        GET_JITAN_NORMAL_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_JITAN_NORMAL_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_JITAN_AUTO_MODE_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_JITAN_AUTO_MODE_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_JITAN_AUTO_MODE_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_JITAN_AUTO_MODE_ONOFF, 0)
        }
        GET_JITAN_AUTO_MODE_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_JITAN_AUTO_MODE_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_JITAN_KOE_REPLAY_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_JITAN_KOE_REPLAY_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_JITAN_KOE_REPLAY_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_JITAN_KOE_REPLAY_ONOFF, 0)
        }
        GET_JITAN_KOE_REPLAY_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_JITAN_KOE_REPLAY_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_JITAN_SPEED => cfg_set_int(&mut ctx.globals.syscom, GET_JITAN_SPEED, p_i64(params, 0)),
        SET_JITAN_SPEED_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_JITAN_SPEED, 0),
        GET_JITAN_SPEED => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_JITAN_SPEED, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_MESSAGE_SPEED => {
            cfg_set_int(&mut ctx.globals.syscom, GET_MESSAGE_SPEED, p_i64(params, 0))
        }
        SET_MESSAGE_SPEED_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_MESSAGE_SPEED, 0),
        GET_MESSAGE_SPEED => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_MESSAGE_SPEED, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_MESSAGE_NOWAIT => {
            let v = p_bool(params, 0);
            ctx.globals.script.msg_nowait = v;
            cfg_set_int(
                &mut ctx.globals.syscom,
                GET_MESSAGE_NOWAIT,
                if v { 1 } else { 0 },
            );
        }
        SET_MESSAGE_NOWAIT_DEFAULT => {
            ctx.globals.script.msg_nowait = false;
            cfg_set_int(&mut ctx.globals.syscom, GET_MESSAGE_NOWAIT, 0);
        }
        GET_MESSAGE_NOWAIT => {
            let v = if ctx.globals.script.msg_nowait {
                1
            } else {
                cfg_get_int(&ctx.globals.syscom, GET_MESSAGE_NOWAIT, 0)
            };
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_AUTO_MODE_MOJI_WAIT => {
            let v = p_i64(params, 0);
            ctx.globals.script.auto_mode_moji_wait = v;
            cfg_set_int(&mut ctx.globals.syscom, GET_AUTO_MODE_MOJI_WAIT, v);
        }
        SET_AUTO_MODE_MOJI_WAIT_DEFAULT => {
            ctx.globals.script.auto_mode_moji_wait = -1;
            cfg_set_int(&mut ctx.globals.syscom, GET_AUTO_MODE_MOJI_WAIT, -1);
        }
        GET_AUTO_MODE_MOJI_WAIT => {
            let v = ctx.globals.script.auto_mode_moji_wait;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_AUTO_MODE_MIN_WAIT => {
            let v = p_i64(params, 0);
            ctx.globals.script.auto_mode_min_wait = v;
            cfg_set_int(&mut ctx.globals.syscom, GET_AUTO_MODE_MIN_WAIT, v);
        }
        SET_AUTO_MODE_MIN_WAIT_DEFAULT => {
            ctx.globals.script.auto_mode_min_wait = -1;
            cfg_set_int(&mut ctx.globals.syscom, GET_AUTO_MODE_MIN_WAIT, -1);
        }
        GET_AUTO_MODE_MIN_WAIT => {
            let v = ctx.globals.script.auto_mode_min_wait;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_MOUSE_CURSOR_HIDE_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_MOUSE_CURSOR_HIDE_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_MOUSE_CURSOR_HIDE_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_MOUSE_CURSOR_HIDE_ONOFF, 0)
        }
        GET_MOUSE_CURSOR_HIDE_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_MOUSE_CURSOR_HIDE_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_MOUSE_CURSOR_HIDE_TIME => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_MOUSE_CURSOR_HIDE_TIME,
            p_i64(params, 0),
        ),
        SET_MOUSE_CURSOR_HIDE_TIME_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_MOUSE_CURSOR_HIDE_TIME, 0)
        }
        GET_MOUSE_CURSOR_HIDE_TIME => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_MOUSE_CURSOR_HIDE_TIME, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_FILTER_COLOR_R => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_FILTER_COLOR_R,
            p_i64(params, 0),
        ),
        SET_FILTER_COLOR_G => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_FILTER_COLOR_G,
            p_i64(params, 0),
        ),
        SET_FILTER_COLOR_B => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_FILTER_COLOR_B,
            p_i64(params, 0),
        ),
        SET_FILTER_COLOR_A => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_FILTER_COLOR_A,
            p_i64(params, 0),
        ),
        SET_FILTER_COLOR_R_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_FILTER_COLOR_R, 0),
        SET_FILTER_COLOR_G_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_FILTER_COLOR_G, 0),
        SET_FILTER_COLOR_B_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_FILTER_COLOR_B, 0),
        SET_FILTER_COLOR_A_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_FILTER_COLOR_A, 0),
        GET_FILTER_COLOR_R | GET_FILTER_COLOR_G | GET_FILTER_COLOR_B | GET_FILTER_COLOR_A => {
            let v = cfg_get_int(&ctx.globals.syscom, op, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_OBJECT_DISP_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_OBJECT_DISP_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_OBJECT_DISP_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_OBJECT_DISP_ONOFF, 1)
        }
        GET_OBJECT_DISP_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_OBJECT_DISP_ONOFF, 1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_GLOBAL_EXTRA_SWITCH_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_GLOBAL_EXTRA_SWITCH_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_GLOBAL_EXTRA_SWITCH_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_GLOBAL_EXTRA_SWITCH_ONOFF, 0)
        }
        GET_GLOBAL_EXTRA_SWITCH_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_GLOBAL_EXTRA_SWITCH_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_GLOBAL_EXTRA_MODE_VALUE => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_GLOBAL_EXTRA_MODE_VALUE,
            p_i64(params, 0),
        ),
        SET_GLOBAL_EXTRA_MODE_VALUE_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_GLOBAL_EXTRA_MODE_VALUE, 0)
        }
        GET_GLOBAL_EXTRA_MODE_VALUE => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_GLOBAL_EXTRA_MODE_VALUE, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_SAVELOAD_ALERT_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_SAVELOAD_ALERT_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_SAVELOAD_ALERT_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_SAVELOAD_ALERT_ONOFF, 1)
        }
        GET_SAVELOAD_ALERT_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_SAVELOAD_ALERT_ONOFF, 1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_SAVELOAD_DBLCLICK_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_SAVELOAD_DBLCLICK_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_SAVELOAD_DBLCLICK_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_SAVELOAD_DBLCLICK_ONOFF, 0)
        }
        GET_SAVELOAD_DBLCLICK_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_SAVELOAD_DBLCLICK_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_SLEEP_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_SLEEP_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_SLEEP_ONOFF_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_SLEEP_ONOFF, 1),
        GET_SLEEP_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_SLEEP_ONOFF, 1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_NO_WIPE_ANIME_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_NO_WIPE_ANIME_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_NO_WIPE_ANIME_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_NO_WIPE_ANIME_ONOFF, 0)
        }
        GET_NO_WIPE_ANIME_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_NO_WIPE_ANIME_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_SKIP_WIPE_ANIME_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_SKIP_WIPE_ANIME_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_SKIP_WIPE_ANIME_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_SKIP_WIPE_ANIME_ONOFF, 0)
        }
        GET_SKIP_WIPE_ANIME_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_SKIP_WIPE_ANIME_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_NO_MWND_ANIME_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_NO_MWND_ANIME_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_NO_MWND_ANIME_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_NO_MWND_ANIME_ONOFF, 0)
        }
        GET_NO_MWND_ANIME_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_NO_MWND_ANIME_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_WHEEL_NEXT_MESSAGE_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_WHEEL_NEXT_MESSAGE_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_WHEEL_NEXT_MESSAGE_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_WHEEL_NEXT_MESSAGE_ONOFF, 1)
        }
        GET_WHEEL_NEXT_MESSAGE_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_WHEEL_NEXT_MESSAGE_ONOFF, 1);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_KOE_DONT_STOP_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_KOE_DONT_STOP_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_KOE_DONT_STOP_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_KOE_DONT_STOP_ONOFF, 0)
        }
        GET_KOE_DONT_STOP_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_KOE_DONT_STOP_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_SKIP_UNREAD_MESSAGE_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_SKIP_UNREAD_MESSAGE_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_SKIP_UNREAD_MESSAGE_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_SKIP_UNREAD_MESSAGE_ONOFF, 0)
        }
        GET_SKIP_UNREAD_MESSAGE_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_SKIP_UNREAD_MESSAGE_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_PLAY_SILENT_SOUND_ONOFF => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_PLAY_SILENT_SOUND_ONOFF,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_PLAY_SILENT_SOUND_ONOFF_DEFAULT => {
            cfg_set_int(&mut ctx.globals.syscom, GET_PLAY_SILENT_SOUND_ONOFF, 0)
        }
        GET_PLAY_SILENT_SOUND_ONOFF => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_PLAY_SILENT_SOUND_ONOFF, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_FONT_NAME => {
            let v = params
                .get(0)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            cfg_set_str(&mut ctx.globals.syscom, GET_FONT_NAME, v);
        }
        SET_FONT_NAME_DEFAULT => cfg_set_str(&mut ctx.globals.syscom, GET_FONT_NAME, String::new()),
        GET_FONT_NAME => {
            let v = cfg_get_str(&ctx.globals.syscom, GET_FONT_NAME);
            ctx.push(Value::Str(v));
            return Ok(true);
        }
        IS_FONT_EXIST => {
            let name = params.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let exists = font_exists(&ctx.project_dir, name);
            ctx.push(Value::Int(if exists { 1 } else { 0 }));
            return Ok(true);
        }
        SET_FONT_BOLD => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_FONT_BOLD,
            if p_bool(params, 0) { 1 } else { 0 },
        ),
        SET_FONT_BOLD_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_FONT_BOLD, 0),
        GET_FONT_BOLD => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_FONT_BOLD, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        SET_FONT_DECORATION => cfg_set_int(
            &mut ctx.globals.syscom,
            GET_FONT_DECORATION,
            p_i64(params, 0),
        ),
        SET_FONT_DECORATION_DEFAULT => cfg_set_int(&mut ctx.globals.syscom, GET_FONT_DECORATION, 0),
        GET_FONT_DECORATION => {
            let v = cfg_get_int(&ctx.globals.syscom, GET_FONT_DECORATION, 0);
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        CREATE_CAPTURE_BUFFER => {
            let w = p_i64(params, 0).max(1) as u32;
            let h = p_i64(params, 1).max(1) as u32;
            ctx.globals.syscom.capture_size = Some((w, h));
            ctx.globals.syscom.capture_buffer = None;
        }
        DESTROY_CAPTURE_BUFFER => {
            ctx.globals.syscom.capture_buffer = None;
            ctx.globals.syscom.capture_size = None;
        }
        CAPTURE_TO_CAPTURE_BUFFER => {
            let mut img = ctx.capture_frame_rgba();
            if let Some((w, h)) = ctx.globals.syscom.capture_size {
                img = resize_rgba(&img, w, h);
            }
            ctx.globals.syscom.capture_buffer = Some(img);
        }
        SAVE_CAPTURE_BUFFER_TO_FILE => {
            let file_name = params.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let extension = params.get(1).and_then(|v| v.as_str()).unwrap_or("");
            let mut name = file_name.to_string();
            if !extension.is_empty()
                && !name
                    .to_ascii_lowercase()
                    .ends_with(&format!(".{}", extension.to_ascii_lowercase()))
            {
                name.push('.');
                name.push_str(extension);
            }
            let path = join_game_path(&ctx.project_dir, &name);
            if ctx.globals.syscom.capture_buffer.is_none() {
                let mut img = ctx.capture_frame_rgba();
                if let Some((w, h)) = ctx.globals.syscom.capture_size {
                    img = resize_rgba(&img, w, h);
                }
                ctx.globals.syscom.capture_buffer = Some(img);
            }
            if let Some(img) = ctx.globals.syscom.capture_buffer.as_ref() {
                write_rgba_png(&path, img);
            }
            ctx.push(Value::Int(1));
            return Ok(true);
        }
        LOAD_FLAG_FROM_CAPTURE_FILE => {
            let file_name = params.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let extension = params.get(1).and_then(|v| v.as_str()).unwrap_or("");
            let mut name = file_name.to_string();
            if !extension.is_empty()
                && !name
                    .to_ascii_lowercase()
                    .ends_with(&format!(".{}", extension.to_ascii_lowercase()))
            {
                name.push('.');
                name.push_str(extension);
            }
            let path = join_game_path(&ctx.project_dir, &name);
            ctx.push(Value::Int(if path.exists() { 1 } else { 0 }));
            return Ok(true);
        }
        CAPTURE_AND_SAVE_BUFFER_TO_PNG => {
            let file_name = params.get(2).and_then(|v| v.as_str()).unwrap_or("");
            let path = join_game_path(&ctx.project_dir, file_name);
            let mut img = ctx.capture_frame_rgba();
            if let Some((w, h)) = ctx.globals.syscom.capture_size {
                img = resize_rgba(&img, w, h);
            }
            write_rgba_png(&path, &img);
        }
        OPEN_TWEET_DIALOG => {}
        SET_RETURN_SCENE_ONCE => {
            let name = params
                .get(0)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let z_no = p_i64(params, 1);
            ctx.globals.syscom.return_scene_once = Some((name, z_no));
        }
        GET_SYSTEM_EXTRA_INT_VALUE => {
            let v = ctx.globals.syscom.system_extra_int_value;
            ctx.push(Value::Int(v));
            return Ok(true);
        }
        GET_SYSTEM_EXTRA_STR_VALUE => {
            let v = ctx.globals.syscom.system_extra_str_value.clone();
            ctx.push(Value::Str(v));
            return Ok(true);
        }
        _ => {
            return Ok(false);
        }
    }

    ctx.push(Value::Int(0));
    Ok(true)
}
