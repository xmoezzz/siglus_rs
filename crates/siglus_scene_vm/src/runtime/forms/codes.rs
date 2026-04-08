//! Legacy numeric form IDs and sub-op IDs.
//!
//! These values are **title-specific** and can vary across engine builds.
//! Prefer configuring IDs via `IdMap` (environment variables) instead of relying
//! on these constants.

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
    9, 10, 11, 12, 13, 14, 15, 16, 17, 18,
    21, 22, 41, 47, 56, 57, 58, 59, 61, 62,
    72, 84, 90, 91, 93, 94, 95, 100, 102,
    115, 116, 119, 120, 121, 122, 125, 151, 156,
];

/// Global form IDs that behave like a string-list (backed by `tnm_command_proc_str_list`).
///
/// Source: `external ID mapping` mapping: cases that dispatch to the original engine.
pub const GLOBAL_STR_LIST_FORMS: &[u32] = &[19, 101, 117];

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
// BGMTABLE element sub-ops (placeholder)
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

// Object properties / commands.
pub const OBJECT_DISP: i32 = 13;
pub const OBJECT_PATNO: i32 = 14;
pub const OBJECT_ORDER: i32 = 16;
pub const OBJECT_LAYER: i32 = 17;
pub const OBJECT_X: i32 = 18;
pub const OBJECT_Y: i32 = 19;

/// Object command: CREATE.
pub const OBJECT_CREATE: i32 = 38;
