//! Legacy numeric form IDs and sub-op IDs.
//!
//! These values are **title-specific** and can vary across engine builds.
//! Prefer using `RuntimeConstants` as the single recovered constant source instead of relying
//! on scattered legacy definitions.

// -----------------------------------------------------------------------------
// Reverse-confirmed form IDs (same-version `se-src` + `docs/SiglusEngine.exe`)
// -----------------------------------------------------------------------------

/// `FM_STAGE`
///
/// Confirmed from:
/// - source `cmd_mwnd.cpp` / `cmd_object.cpp`
/// - decomp `sub_584940` / `sub_5899E0`
pub const FM_STAGE: i32 = 1300;

/// `FM_OBJECT`
///
/// Confirmed from:
/// - source `cmd_object.cpp`
/// - decomp `sub_5899E0`
pub const FM_OBJECT: i32 = 1310;

/// `FM_OBJECTLIST`
///
/// Confirmed from:
/// - source `cmd_object.cpp` / `elm_object.cpp`
/// - decomp `sub_589BC0`
pub const FM_OBJECTLIST: i32 = 1311;

/// `FM_MWNDLIST`
///
/// Confirmed from:
/// - source `cmd_mwnd.cpp`
/// - decomp `sub_584AA0`
pub const FM_MWNDLIST: i32 = 1321;

/// `FM_STAGELIST`
///
/// Confirmed from:
/// - source `elm_stage.cpp`
/// - decomp `sub_63FE00`
pub const FM_STAGELIST: i32 = 1301;

/// `FM_BTNSELITEM`
///
/// Confirmed from:
/// - source `elm_btn_sel_item.cpp`
/// - decomp `sub_60B910`
pub const FM_BTNSELITEM: i32 = 1340;

/// `FM_SCREEN`
///
/// Confirmed from:
/// - source `elm_screen.cpp`
/// - decomp `sub_630190`
pub const FM_SCREEN: i32 = 1350;

/// `FM_QUAKE`
///
/// Confirmed from:
/// - source `elm_screen.cpp`
/// - decomp `sub_632250`
pub const FM_QUAKE: i32 = 1360;

/// `FM_QUAKELIST`
///
/// Confirmed from:
/// - source `elm_screen.cpp`
/// - decomp `sub_632790`
pub const FM_QUAKELIST: i32 = 1361;

/// `FM_EFFECT`
///
/// Confirmed from:
/// - source `elm_screen.cpp`
/// - decomp `sub_630F40`
pub const FM_EFFECT: i32 = 1380;

/// `FM_EFFECTLIST`
///
/// Confirmed from:
/// - source `elm_screen.cpp`
/// - decomp `sub_632A60`
pub const FM_EFFECTLIST: i32 = 1381;

/// `FM_EXCALL`
///
/// Confirmed from:
/// - source `elm_excall.cpp`
/// - decomp `sub_5EFB30`
pub const FM_EXCALL: i32 = 1700;

/// `FM_FRAMEACTION`
///
/// Confirmed from:
/// - source `elm_frame_action.cpp`
/// - decomp `sub_5F5010`
pub const FM_FRAMEACTION: i32 = 1210;

/// `FM_FRAMEACTIONLIST`
///
/// Confirmed from:
/// - source `elm_frame_action.cpp` / `elm_excall.cpp`
/// - decomp `sub_5F62D0`
pub const FM_FRAMEACTIONLIST: i32 = 1211;

pub const FM_CALL: i32 = 1020;
pub const FM_CALLLIST: i32 = 1021;
pub const FM_INTEVENT: i32 = 15;
pub const FM_INTEVENTLIST: i32 = 16;

pub const CALL_ELM_L: i32 = 0;
pub const CALL_ELM_K: i32 = 1;

// -----------------------------------------------------------------------------
// Reverse-confirmed MWND child selectors
// -----------------------------------------------------------------------------

/// `ELM_MWND_OBJECT`
///
/// Confirmed from:
/// - source `elm_mwnd_waku.cpp` (`.object`)
/// - decomp `sub_617950` appending `30`
pub const ELM_MWND_OBJECT: i32 = 30;

/// `ELM_MWND_BUTTON`
///
/// Confirmed from:
/// - source `elm_mwnd_waku.cpp` (`.button`)
/// - decomp `sub_617950` appending `32`
pub const ELM_MWND_BUTTON: i32 = 32;

/// `ELM_MWND_FACE`
///
/// Confirmed from:
/// - source `elm_mwnd_waku.cpp` (`.face`)
/// - decomp `sub_617950` appending `53`
pub const ELM_MWND_FACE: i32 = 53;

// -----------------------------------------------------------------------------
// Global form IDs
// -----------------------------------------------------------------------------

/// Global form: MOV element (case 20 in the original engine global-form switch).
pub const FORM_GLOBAL_MOV: u32 = 20;

/// Global form: BGM element (case 42 in the original engine global-form switch).
pub const FORM_GLOBAL_BGM: u32 = 42;

/// Global form: PCM element (case 43 in the original engine global-form switch).
pub const FORM_GLOBAL_PCM: u32 = 43;

/// Global form: PCMCH element (case 44 in the original engine global-form switch).
pub const FORM_GLOBAL_PCMCH: u32 = 44;

/// Global form: SE element (case 45 in the original engine global-form switch).
pub const FORM_GLOBAL_SE: u32 = 45;

/// Global form: PCMEVENT element (case 52 in the original engine global-form switch).
pub const FORM_GLOBAL_PCMEVENT: u32 = 52;

/// Global form: EXCALL element (case 65 in the original engine global-form switch).
pub const FORM_GLOBAL_EXCALL: u32 = 65;

/// Global form: SCREEN helper (case 70 in the original engine global-form switch).
pub const FORM_GLOBAL_SCREEN: u32 = 70;

/// Global form: MSGBK helper (case 145 in the original engine global-form switch).
pub const FORM_GLOBAL_MSGBK: u32 = 145;

/// Global form: KOE_ST element (case 82 in the original engine global-form switch).
///
/// Note: `tnm_command_proc_koe` is a stub in the original engine as well.
pub const FORM_GLOBAL_KOE_ST: u32 = 82;

/// Global form: KEY helper (case 89 in the original engine global-form switch).
pub const FORM_GLOBAL_KEY: u32 = 89;

/// Global form: BGMTABLE element (case 123 in the original engine global-form switch).
pub const FORM_GLOBAL_BGM_TABLE: u32 = 123;

/// Global form: TIMEWAIT helper (case 54 in the original engine global-form switch).
pub const FORM_GLOBAL_TIMEWAIT: u32 = 54;

/// Global form: TIMEWAIT_KEY helper (case 55 in the original engine global-form switch).
pub const FORM_GLOBAL_TIMEWAIT_KEY: u32 = 55;

/// Global form: COUNTER list (case 63 in the original engine global-form switch).
pub const FORM_GLOBAL_COUNTER: u32 = 63;

/// Global form: FRAME_ACTION list (case 64 in the original engine global-form switch).
pub const FORM_GLOBAL_FRAME_ACTION: u32 = 64;

/// Global form IDs that behave like an int-list (backed by `tnm_command_proc_int_list`).
///
/// Source: `external ID mapping` mapping: cases that dispatch to the original engine.
pub const GLOBAL_INT_LIST_FORMS: &[u32] = &[
    9, 10, 11, 12, 13, 14, 17, 18, 21, 22, 41, 47, 56, 57, 58, 59, 61, 62, 72, 84, 90, 91, 93, 94,
    95, 100, 102, 115, 116, 119, 120, 121, 122, 125, 151, 156,
];

/// Global form IDs that behave like a string-list (backed by `tnm_command_proc_str_list`).
///
/// Source: `external ID mapping` mapping: cases that dispatch to the original engine.
pub const GLOBAL_STR_LIST_FORMS: &[u32] = &[19, 101, 117];

pub mod call_op {
    pub const L: i32 = 0;
    pub const K: i32 = 1;
}

pub mod int_event_op {
    pub const SET: i32 = 0;
    pub const LOOP: i32 = 1;
    pub const TURN: i32 = 2;
    pub const END: i32 = 3;
    pub const WAIT: i32 = 4;
    pub const CHECK: i32 = 5;
    pub const SET_REAL: i32 = 7;
    pub const LOOP_REAL: i32 = 8;
    pub const TURN_REAL: i32 = 9;
    pub const WAIT_KEY: i32 = 10;
}

pub mod int_event_list_op {
    pub const RESIZE: i32 = 1;
}

pub mod str_op {
    pub const UPPER: i32 = 0;
    pub const LOWER: i32 = 1;
    pub const LEFT: i32 = 2;
    pub const MID: i32 = 3;
    pub const RIGHT: i32 = 4;
    pub const LEN: i32 = 5;
    pub const CNT: i32 = 6;
    pub const LEFT_LEN: i32 = 7;
    pub const MID_LEN: i32 = 8;
    pub const RIGHT_LEN: i32 = 9;
    pub const SEARCH: i32 = 10;
    pub const SEARCH_LAST: i32 = 11;
    pub const TONUM: i32 = 12;
    pub const GET_CODE: i32 = 13;
}

pub mod str_list_op {
    pub const RESIZE: i32 = 2;
    pub const INIT: i32 = 3;
    pub const GET_SIZE: i32 = 4;
}
// -----------------------------------------------------------------------------
// BGM element sub-ops (from the original engine)
// -----------------------------------------------------------------------------

/// BGM element sub-op IDs.
///
/// These are the values of `v3 = *a1` in the original engine.
pub mod bgm_op {
    pub const PLAY: i64 = 0;
    pub const PLAY_ONESHOT: i64 = 1;
    pub const PLAY_WAIT: i64 = 2;

    pub const WAIT: i64 = 3;
    pub const STOP: i64 = 4;
    pub const WAIT_FADE: i64 = 5;

    pub const SET_VOLUME: i64 = 6;
    pub const SET_VOLUME_MAX: i64 = 7;
    pub const SET_VOLUME_MIN: i64 = 8;
    pub const GET_VOLUME: i64 = 9;

    pub const PAUSE: i64 = 10;
    pub const RESUME: i64 = 11;
    pub const RESUME_WAIT: i64 = 12;

    pub const CHECK: i64 = 13;

    pub const WAIT_KEY: i64 = 14;
    pub const WAIT_FADE_KEY: i64 = 15;

    pub const READY: i64 = 16;
    pub const READY_ONESHOT: i64 = 17;

    pub const GET_PLAY_POS: i64 = 18;
    pub const GET_REGIST_NAME: i64 = 19;
}

// -----------------------------------------------------------------------------
// BGMTABLE element sub-ops
// -----------------------------------------------------------------------------

pub mod bgm_table_op {
    // the original engine:
    // - if op==0 => push count
    // - case 1 => v = engine_fn(name); push v
    // - case 2 => set listen flag for current entry
    // - case 4 => set all-flag
    pub const GET_COUNT: i64 = 0;
    pub const GET_LISTEN_BY_NAME: i64 = 1;
    pub const SET_LISTEN_CURRENT: i64 = 2;
    pub const SET_ALL_FLAG: i64 = 4;
}

// -----------------------------------------------------------------------------
// PCM element sub-ops (from the original engine)
// -----------------------------------------------------------------------------

pub mod pcm_op {
    pub const PLAY: i64 = 0;
    pub const STOP: i64 = 1;
}

// -----------------------------------------------------------------------------
// SE element sub-ops (from the original engine)
// -----------------------------------------------------------------------------

pub mod se_op {
    // Note: op 0 and op 9 share the same code path in the original engine.
    pub const PLAY: i64 = 0;

    pub const SET_VOLUME: i64 = 1;
    pub const SET_VOLUME_MAX: i64 = 2;
    pub const SET_VOLUME_MIN: i64 = 3;
    pub const GET_VOLUME: i64 = 4;

    pub const PLAY_BY_FILE_NAME: i64 = 5;
    pub const PLAY_BY_KOE_NO: i64 = 6;

    pub const STOP: i64 = 7;
    pub const WAIT: i64 = 8;

    pub const PLAY_BY_SE_NO: i64 = 9;

    pub const WAIT_KEY: i64 = 10;
    pub const CHECK: i64 = 11;
}

// -----------------------------------------------------------------------------
// PCMCH element sub-ops (from the original engine)
//
// Note: The VM provides these numeric IDs in element chains or as the first
// argument for call-form style. We keep sequential defaults here, but they are
// treated as legacy runtime values.
// -----------------------------------------------------------------------------

pub mod pcmch_op {
    pub const PLAY: i64 = 0;
    pub const PLAY_LOOP: i64 = 1;
    pub const PLAY_WAIT: i64 = 2;
    pub const READY: i64 = 3;
    pub const STOP: i64 = 4;
    pub const PAUSE: i64 = 5;
    pub const RESUME: i64 = 6;
    pub const RESUME_WAIT: i64 = 7;
    pub const WAIT: i64 = 8;
    pub const WAIT_KEY: i64 = 9;
    pub const WAIT_FADE: i64 = 10;
    pub const WAIT_FADE_KEY: i64 = 11;
    pub const CHECK: i64 = 12;
    pub const SET_VOLUME: i64 = 13;
    pub const SET_VOLUME_MAX: i64 = 14;
    pub const SET_VOLUME_MIN: i64 = 15;
    pub const GET_VOLUME: i64 = 16;
}

// -----------------------------------------------------------------------------
// MOV element sub-ops (from the original engine)
// -----------------------------------------------------------------------------

pub mod mov_op {
    pub const PLAY: i64 = 0;
    pub const STOP: i64 = 1;
    pub const PLAY_WAIT: i64 = 2;
    pub const PLAY_WAIT_KEY: i64 = 3;
}

// -----------------------------------------------------------------------------
// EXCALL element sub-ops (from the original engine)
// -----------------------------------------------------------------------------

pub mod excall_op {
    pub const ARRAY_INDEX: i64 = -1;

    pub const OP_0: i64 = 0;
    pub const OP_1: i64 = 1;
    pub const OP_2: i64 = 2;
    pub const OP_3: i64 = 3;
    pub const OP_4: i64 = 4;
    pub const OP_5: i64 = 5;
    pub const OP_6: i64 = 6;
    pub const OP_7: i64 = 7;
    pub const OP_8: i64 = 8;
    pub const OP_9: i64 = 9;
    pub const OP_10: i64 = 10;
    // No op 11 in the original switch.
    pub const OP_12: i64 = 12;
    pub const OP_13: i64 = 13;
}

// -----------------------------------------------------------------------------
// Stage / Object (conservative defaults)
// -----------------------------------------------------------------------------

/// Global form: STAGE (case 135 in many titles; may vary).
pub const FORM_GLOBAL_STAGE: u32 = 135;

/// Array-index sentinel used by the engine's element chain.
pub const ELM_ARRAY: i32 = -1;

/// Stage child element: OBJECT.
pub const STAGE_ELM_OBJECT: i32 = 2;

/// `ELM_STAGE_CREATE_OBJECT`
pub const STAGE_CREATE_OBJECT: i32 = 0;

/// `ELM_STAGE_CREATE_MWND`
pub const STAGE_CREATE_MWND: i32 = 1;

/// `ELM_STAGE_MWND`
pub const STAGE_ELM_MWND: i32 = 3;

/// `ELM_STAGE_BTNSELITEM`
pub const STAGE_ELM_BTNSELITEM: i32 = 5;

/// `ELM_STAGE_OBJBTNGROUP`
pub const STAGE_ELM_OBJBTNGROUP: i32 = 6;

/// `ELM_STAGE_WORLD`
pub const STAGE_ELM_WORLD: i32 = 8;

/// `ELM_STAGE_QUAKE`
pub const STAGE_ELM_QUAKE: i32 = 7;

/// `ELM_STAGE_EFFECT`
pub const STAGE_ELM_EFFECT: i32 = 4;

// Object properties / commands.
pub const OBJECTLIST_GET_SIZE: i32 = 3;
pub const OBJECTLIST_RESIZE: i32 = 4;

pub const OBJECT_DISP: i32 = 0x03;
pub const OBJECT_PATNO: i32 = 0x04;
pub const OBJECT_ORDER: i32 = 0x06;
pub const OBJECT_X: i32 = 0x08;
pub const OBJECT_Y: i32 = 0x09;
pub const OBJECT_Z: i32 = 0x0A;
pub const OBJECT_CENTER_X: i32 = 0x0F;
pub const OBJECT_CENTER_Y: i32 = 0x10;
pub const OBJECT_CENTER_Z: i32 = 0x11;
pub const OBJECT_CENTER_REP_X: i32 = 0x13;
pub const OBJECT_CENTER_REP_Y: i32 = 0x14;
pub const OBJECT_CENTER_REP_Z: i32 = 0x15;
pub const OBJECT_SCALE_X: i32 = 0x17;
pub const OBJECT_SCALE_Y: i32 = 0x18;
pub const OBJECT_SCALE_Z: i32 = 0x19;
pub const OBJECT_ROTATE_X: i32 = 0x1B;
pub const OBJECT_ROTATE_Y: i32 = 0x1C;
pub const OBJECT_ROTATE_Z: i32 = 0x1D;
pub const OBJECT_CLIP_USE: i32 = 0x1F;
pub const OBJECT_CLIP_LEFT: i32 = 0x20;
pub const OBJECT_CLIP_TOP: i32 = 0x21;
pub const OBJECT_CLIP_RIGHT: i32 = 0x22;
pub const OBJECT_CLIP_BOTTOM: i32 = 0x23;
pub const OBJECT_COLOR_RATE: i32 = 0x40;

pub const OBJECT_LAYER: i32 = 0x07;
pub const OBJECT_WORLD: i32 = 0x05;
pub const OBJECT_BLEND: i32 = 0x49;
pub const OBJECT_SET_POS: i32 = 0x0E;
pub const OBJECT_SET_SCALE: i32 = 0x1A;
pub const OBJECT_SET_ROTATE: i32 = 0x1E;
pub const OBJECT_SET_CENTER: i32 = 0x12;
pub const OBJECT_SET_CENTER_REP: i32 = 0x16;
pub const OBJECT_SET_CLIP: i32 = 0x24;
pub const OBJECT_SET_SRC_CLIP: i32 = 0x2A;
pub const OBJECT_TR: i32 = 0x2B;
pub const OBJECT_MONO: i32 = 0x2D;
pub const OBJECT_REVERSE: i32 = 0x2E;
pub const OBJECT_BRIGHT: i32 = 0x2F;
pub const OBJECT_DARK: i32 = 0x30;
pub const OBJECT_COLOR_R: i32 = 0x31;
pub const OBJECT_COLOR_G: i32 = 0x32;
pub const OBJECT_COLOR_B: i32 = 0x33;
pub const OBJECT_COLOR_ADD_R: i32 = 0x41;
pub const OBJECT_COLOR_ADD_G: i32 = 0x42;
pub const OBJECT_COLOR_ADD_B: i32 = 0x43;
pub const OBJECT_WIPE_COPY: i32 = 0x00;
pub const OBJECT_WIPE_ERASE: i32 = 0x01;
pub const OBJECT_CLICK_DISABLE: i32 = 0x02;
pub const OBJECT_MASK_NO: i32 = 0x44;
pub const OBJECT_TONECURVE_NO: i32 = 0x45;
pub const OBJECT_LIGHT_NO: i32 = 0x4A;
pub const OBJECT_FOG_USE: i32 = 0x4B;
pub const OBJECT_CULLING: i32 = 0x46;
pub const OBJECT_ALPHA_TEST: i32 = 0x47;
pub const OBJECT_ALPHA_BLEND: i32 = 0x48;
pub const OBJECT_SRC_CLIP_USE: i32 = 0x25;
pub const OBJECT_SRC_CLIP_LEFT: i32 = 0x26;
pub const OBJECT_SRC_CLIP_TOP: i32 = 0x27;
pub const OBJECT_SRC_CLIP_RIGHT: i32 = 0x28;
pub const OBJECT_SRC_CLIP_BOTTOM: i32 = 0x29;

/// Object command: CREATE.
pub const OBJECT_CREATE: i32 = 0x72;
