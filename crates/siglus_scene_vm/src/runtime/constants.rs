//! Canonical engine constants used by the runtime.
//!
//! This module is the single source of truth for recovered numeric values that
//! are already known for this project build. Runtime code should resolve
//! semantics from these constants directly instead of learning mappings at run
//! time.

pub mod op {
    pub const NONE: u8 = 0x00;
    pub const PLUS: u8 = 0x01;
    pub const MINUS: u8 = 0x02;
    pub const MULTIPLE: u8 = 0x03;
    pub const DIVIDE: u8 = 0x04;
    pub const AMARI: u8 = 0x05;
    pub const EQUAL: u8 = 0x10;
    pub const NOT_EQUAL: u8 = 0x11;
    pub const GREATER: u8 = 0x12;
    pub const GREATER_EQUAL: u8 = 0x13;
    pub const LESS: u8 = 0x14;
    pub const LESS_EQUAL: u8 = 0x15;
    pub const LOGICAL_AND: u8 = 0x20;
    pub const LOGICAL_OR: u8 = 0x21;
    pub const TILDE: u8 = 0x30;
    pub const AND: u8 = 0x31;
    pub const OR: u8 = 0x32;
    pub const HAT: u8 = 0x33;
    pub const SL: u8 = 0x34;
    pub const SR: u8 = 0x35;
    pub const SR3: u8 = 0x36;
}

pub mod cd {
    pub const NONE: u8 = 0x00;
    pub const NL: u8 = 0x01;
    pub const PUSH: u8 = 0x02;
    pub const POP: u8 = 0x03;
    pub const COPY: u8 = 0x04;
    pub const PROPERTY: u8 = 0x05;
    pub const COPY_ELM: u8 = 0x06;
    pub const DEC_PROP: u8 = 0x07;
    pub const ELM_POINT: u8 = 0x08;
    pub const ARG: u8 = 0x09;
    pub const GOTO: u8 = 0x10;
    pub const GOTO_TRUE: u8 = 0x11;
    pub const GOTO_FALSE: u8 = 0x12;
    pub const GOSUB: u8 = 0x13;
    pub const GOSUBSTR: u8 = 0x14;
    pub const RETURN: u8 = 0x15;
    pub const EOF: u8 = 0x16;
    pub const ASSIGN: u8 = 0x20;
    pub const OPERATE_1: u8 = 0x21;
    pub const OPERATE_2: u8 = 0x22;
    pub const COMMAND: u8 = 0x30;
    pub const TEXT: u8 = 0x31;
    pub const NAME: u8 = 0x32;
    pub const SEL_BLOCK_START: u8 = 0x33;
    pub const SEL_BLOCK_END: u8 = 0x34;
}

pub mod elm {
    pub const OWNER_USER_PROP: i32 = 127;
    pub const OWNER_USER_CMD: i32 = 126;
    pub const OWNER_CALL_PROP: i32 = 125;
    pub const OWNER_CALL_CMD: i32 = 124;
    pub const ARRAY: i32 = -1;
    pub const SET: i32 = -2;
    pub const TRANS: i32 = -3;
    pub const CURRENT: i32 = -4;
    pub const UP: i32 = -5;

    #[inline]
    pub const fn create(owner: i32, group: i32, code: i32) -> i32 {
        ((owner & 0xFF) << 24) + ((group & 0xFF) << 16) + (code & 0xFFFF)
    }

    #[inline]
    pub const fn owner(code: i32) -> i32 {
        (code >> 24) & 0xFF
    }

    #[inline]
    pub const fn group(code: i32) -> i32 {
        (code >> 16) & 0xFF
    }

    #[inline]
    pub const fn code(code: i32) -> i32 {
        code & 0xFFFF
    }
}

pub mod fm {
    pub const ALLEVENT: i32 = 17;
    pub const BGM: i32 = 1400;
    pub const BGMLIST: i32 = 1401;
    pub const BTNSELITEM: i32 = 1340;
    pub const BTNSELITEMLIST: i32 = 1341;
    pub const CALL: i32 = 1020;
    pub const CALLLIST: i32 = 1021;
    pub const CGTABLE: i32 = 1120;
    pub const COUNTER: i32 = 1200;
    pub const COUNTERLIST: i32 = 1201;
    pub const DATABASE: i32 = 1130;
    pub const DATABASELIST: i32 = 1131;
    pub const EDITBOX: i32 = 1370;
    pub const EDITBOXLIST: i32 = 1371;
    pub const EFFECT: i32 = 1380;
    pub const EFFECTLIST: i32 = 1381;
    pub const EXCALL: i32 = 1700;
    pub const FILE: i32 = 1110;
    pub const FRAMEACTION: i32 = 1210;
    pub const FRAMEACTIONLIST: i32 = 1211;
    pub const G00BUF: i32 = 1140;
    pub const G00BUFLIST: i32 = 1141;
    pub const GLOBAL: i32 = 1000;
    pub const GLOBALLIST: i32 = 1001;
    pub const GROUP: i32 = 1330;
    pub const GROUPLIST: i32 = 1331;
    pub const INPUT: i32 = 1520;
    pub const INT: i32 = 10;
    // Accepted as legacy name or alias of FM_INT; assigned 10 by alias convention.
    pub const INTEGER: i32 = 10;
    pub const INTEVENT: i32 = 15;
    pub const INTEVENTLIST: i32 = 16;
    pub const INTLIST: i32 = 11;
    pub const INTLISTLIST: i32 = 12;
    pub const INTLISTREF: i32 = 14;
    pub const INTREF: i32 = 13;
    pub const KEY: i32 = 1510;
    pub const KEYLIST: i32 = 1511;
    pub const KOE: i32 = 1410;
    pub const KOELIST: i32 = 1411;
    pub const LABEL: i32 = 30;
    pub const LIST: i32 = -1;
    pub const MASK: i32 = 1150;
    pub const MASKLIST: i32 = 1151;
    pub const MATH: i32 = 1100;
    pub const MOUSE: i32 = 1500;
    pub const MOV: i32 = 1440;
    pub const MWND: i32 = 1320;
    pub const MWNDBTN: i32 = 1602;
    pub const MWNDLIST: i32 = 1321;
    pub const OBJECT: i32 = 1310;
    pub const OBJECTEVENT: i32 = 1312;
    pub const OBJECTEVENTLIST: i32 = 1313;
    pub const OBJECTLIST: i32 = 1311;
    pub const PCM: i32 = 1420;
    pub const PCMCH: i32 = 1421;
    pub const PCMCHLIST: i32 = 1422;
    pub const PCMEVENT: i32 = 1450;
    pub const PCMEVENTLIST: i32 = 1451;
    pub const QUAKE: i32 = 1360;
    pub const QUAKELIST: i32 = 1361;
    pub const SCENE: i32 = 1010;
    pub const SCENELIST: i32 = 1011;
    pub const SCREEN: i32 = 1350;
    pub const SCRIPT: i32 = 1610;
    pub const SE: i32 = 1430;
    pub const STAGE: i32 = 1300;
    pub const STAGELIST: i32 = 1301;
    pub const STR: i32 = 20;
    // Accepted as legacy name or alias of FM_STR; assigned 20 by alias convention.
    pub const STRING: i32 = 20;
    pub const STRLIST: i32 = 21;
    pub const STRLISTLIST: i32 = 22;
    pub const STRLISTREF: i32 = 24;
    pub const STRREF: i32 = 23;
    pub const SYSCOM: i32 = 1600;
    pub const SYSCOMMENU: i32 = 1601;
    pub const SYSTEM: i32 = 1620;
    pub const VOID: i32 = 0;
    pub const VOIDLIST: i32 = 1;
    pub const WORLD: i32 = 1290;
    pub const WORLDLIST: i32 = 1291;
    pub const __ARGS: i32 = -2;
    pub const __ARGSREF: i32 = -3;
}

pub mod global_form {
    use crate::runtime::forms::codes;
    pub const MOV: u32 = codes::FORM_GLOBAL_MOV;
    pub const BGM: u32 = codes::FORM_GLOBAL_BGM;
    pub const PCM: u32 = codes::FORM_GLOBAL_PCM;
    pub const PCMCH: u32 = codes::FORM_GLOBAL_PCMCH;
    pub const SE: u32 = codes::FORM_GLOBAL_SE;
    pub const PCMEVENT: u32 = codes::FORM_GLOBAL_PCMEVENT;
    pub const EXCALL: u32 = codes::FORM_GLOBAL_EXCALL;
    pub const SCREEN: u32 = codes::FORM_GLOBAL_SCREEN;
    pub const MSGBK: u32 = codes::FORM_GLOBAL_MSGBK;
    pub const KOE_ST: u32 = codes::FORM_GLOBAL_KOE_ST;
    pub const KEY: u32 = codes::FORM_GLOBAL_KEY;
    pub const BGMTABLE: u32 = codes::FORM_GLOBAL_BGM_TABLE;
    pub const TIMEWAIT: u32 = codes::FORM_GLOBAL_TIMEWAIT;
    pub const TIMEWAIT_KEY: u32 = codes::FORM_GLOBAL_TIMEWAIT_KEY;
    pub const COUNTER: u32 = codes::FORM_GLOBAL_COUNTER;
    pub const FRAME_ACTION: u32 = codes::FORM_GLOBAL_FRAME_ACTION;
    pub const STAGE_DEFAULT: u32 = 73;
    pub const STAGE_ALIAS_37: u32 = 37;
    pub const STAGE_ALIAS_38: u32 = 38;
    pub const STAGE_ALT: u32 = codes::FORM_GLOBAL_STAGE;
    pub const INT_LIST_FORMS: &[u32] = codes::GLOBAL_INT_LIST_FORMS;
    pub const STR_LIST_FORMS: &[u32] = codes::GLOBAL_STR_LIST_FORMS;
}

pub mod elm_value {
    //
    // -5, confirmed
    pub const UP: i32 = -5;
    // -3, high_confidence_inferred
    pub const __TRANS: i32 = -3;
    // -2, high_confidence_inferred
    pub const __SET: i32 = -2;
    // -1, confirmed
    pub const ARRAY: i32 = -1;

    // BGM
    // 0x00, confirmed
    pub const BGM_PLAY: i32 = 0;
    // 0x01, confirmed
    pub const BGM_PLAY_ONESHOT: i32 = 1;
    // 0x02, confirmed
    pub const BGM_PLAY_WAIT: i32 = 2;
    // 0x03, confirmed
    pub const BGM_WAIT: i32 = 3;
    // 0x04, confirmed
    pub const BGM_STOP: i32 = 4;
    // 0x05, confirmed
    pub const BGM_WAIT_FADE: i32 = 5;
    // 0x06, confirmed
    pub const BGM_SET_VOLUME: i32 = 6;
    // 0x07, confirmed
    pub const BGM_SET_VOLUME_MAX: i32 = 7;
    // 0x08, confirmed
    pub const BGM_SET_VOLUME_MIN: i32 = 8;
    // 0x09, confirmed
    pub const BGM_GET_VOLUME: i32 = 9;
    // 0x0A, confirmed
    pub const BGM_PAUSE: i32 = 10;
    // 0x0B, confirmed
    pub const BGM_RESUME: i32 = 11;
    // 0x0C, confirmed
    pub const BGM_RESUME_WAIT: i32 = 12;
    // 0x0D, confirmed
    pub const BGM_CHECK: i32 = 13;
    // 0x0E, confirmed
    pub const BGM_WAIT_KEY: i32 = 14;
    // 0x0F, confirmed
    pub const BGM_WAIT_FADE_KEY: i32 = 15;
    // 0x10, confirmed
    pub const BGM_READY: i32 = 16;
    // 0x11, confirmed
    pub const BGM_READY_ONESHOT: i32 = 17;
    // 0x12, confirmed
    pub const BGM_GET_PLAY_POS: i32 = 18;
    // 0x13, confirmed
    pub const BGM_GET_REGIST_NAME: i32 = 19;

    // BGMTABLE
    // 0x0, confirmed
    pub const BGMTABLE_GET_BGM_CNT: i32 = 0;
    // 0x1, confirmed
    pub const BGMTABLE_GET_LISTEN_BY_NAME: i32 = 1;
    // 0x2, confirmed
    pub const BGMTABLE_SET_LISTEN_BY_NAME: i32 = 2;
    // 0x4, confirmed
    pub const BGMTABLE_SET_ALL_FLAG: i32 = 4;

    // BTNSELITEM
    // 0x00, confirmed
    pub const BTNSELITEM_OBJECT: i32 = 0;

    // CALL
    // 0x0, confirmed
    pub const CALL_L: i32 = 0;
    // 0x1, confirmed
    pub const CALL_K: i32 = 1;

    // CGTABLE
    // 0x0, confirmed
    pub const CGTABLE_FLAG: i32 = 0;
    // 0x1, confirmed
    pub const CGTABLE_GET_FLAG_NO_BY_NAME: i32 = 1;
    // 0x2, confirmed
    pub const CGTABLE_GET_LOOK_BY_NAME: i32 = 2;
    // 0x3, confirmed
    pub const CGTABLE_SET_LOOK_BY_NAME: i32 = 3;
    // 0x4, confirmed
    pub const CGTABLE_GET_CG_CNT: i32 = 4;
    // 0x5, confirmed
    pub const CGTABLE_GET_LOOK_CNT: i32 = 5;
    // 0x6, confirmed
    pub const CGTABLE_GET_LOOK_PERCENT: i32 = 6;
    // 0x7, confirmed
    pub const CGTABLE_SET_DISABLE: i32 = 7;
    // 0x8, confirmed
    pub const CGTABLE_SET_ENABLE: i32 = 8;
    // 0x9, confirmed
    pub const CGTABLE_SET_ALL_FLAG: i32 = 9;
    // 0xA, confirmed
    pub const CGTABLE_GET_NAME_BY_FLAG_NO: i32 = 10;

    // COUNTER
    // 0x0, confirmed
    pub const COUNTER_SET: i32 = 0;
    // 0x1, confirmed
    pub const COUNTER_GET: i32 = 1;
    // 0x2, confirmed
    pub const COUNTER_RESET: i32 = 2;
    // 0x3, confirmed
    pub const COUNTER_START: i32 = 3;
    // 0x4, confirmed
    pub const COUNTER_STOP: i32 = 4;
    // 0x5, confirmed
    pub const COUNTER_RESUME: i32 = 5;
    // 0x6, confirmed
    pub const COUNTER_WAIT: i32 = 6;
    // 0x7, confirmed
    pub const COUNTER_CHECK_VALUE: i32 = 7;
    // 0x8, confirmed
    pub const COUNTER_WAIT_KEY: i32 = 8;
    // 0x9, confirmed
    pub const COUNTER_START_REAL: i32 = 9;
    // 0xa, confirmed
    pub const COUNTER_START_FRAME: i32 = 10;
    // 0xb, confirmed
    pub const COUNTER_START_FRAME_REAL: i32 = 11;
    // 0xc, confirmed
    pub const COUNTER_START_FRAME_LOOP: i32 = 12;
    // 0xd, confirmed
    pub const COUNTER_START_FRAME_LOOP_REAL: i32 = 13;
    // 0xe, confirmed
    pub const COUNTER_CHECK_ACTIVE: i32 = 14;

    // COUNTERLIST
    // 0x1, confirmed
    pub const COUNTERLIST_GET_SIZE: i32 = 1;

    // DATABASE
    // 0x0, confirmed
    pub const DATABASE_GET_NUM: i32 = 0;
    // 0x1, confirmed
    pub const DATABASE_GET_STR: i32 = 1;
    // 0x3, confirmed
    pub const DATABASE_CHECK_ITEM: i32 = 3;
    // 0x4, confirmed
    pub const DATABASE_CHECK_COLUMN: i32 = 4;
    // 0x5, confirmed
    pub const DATABASE_FIND_NUM: i32 = 5;
    // 0x6, confirmed
    pub const DATABASE_FIND_STR: i32 = 6;
    // 0x7, confirmed
    pub const DATABASE_FIND_STR_REAL: i32 = 7;

    // DATABASELIST
    // 0x1, confirmed
    pub const DATABASELIST_GET_SIZE: i32 = 1;

    // EDITBOX
    // 0x0, confirmed
    pub const EDITBOX_CREATE: i32 = 0;
    // 0x1, confirmed
    pub const EDITBOX_DESTROY: i32 = 1;
    // 0x2, confirmed
    pub const EDITBOX_SET_TEXT: i32 = 2;
    // 0x3, confirmed
    pub const EDITBOX_GET_TEXT: i32 = 3;
    // 0x4, confirmed
    pub const EDITBOX_CHECK_DECIDED: i32 = 4;
    // 0x5, confirmed
    pub const EDITBOX_CHECK_CANCELED: i32 = 5;
    // 0x6, confirmed
    pub const EDITBOX_SET_FOCUS: i32 = 6;
    // 0x7, confirmed
    pub const EDITBOX_CLEAR_INPUT: i32 = 7;

    // EDITBOXLIST
    // 0x1, confirmed
    pub const EDITBOXLIST_CLEAR_INPUT: i32 = 1;

    // EFFECT
    // 0x00, confirmed
    pub const EFFECT_X: i32 = 0;
    // 0x01, confirmed
    pub const EFFECT_Y: i32 = 1;
    // 0x02, confirmed
    pub const EFFECT_Z: i32 = 2;
    // 0x03, confirmed
    pub const EFFECT_MONO: i32 = 3;
    // 0x04, confirmed
    pub const EFFECT_REVERSE: i32 = 4;
    // 0x05, confirmed
    pub const EFFECT_BRIGHT: i32 = 5;
    // 0x06, confirmed
    pub const EFFECT_DARK: i32 = 6;
    // 0x07, confirmed
    pub const EFFECT_COLOR_R: i32 = 7;
    // 0x08, confirmed
    pub const EFFECT_COLOR_G: i32 = 8;
    // 0x09, confirmed
    pub const EFFECT_COLOR_B: i32 = 9;
    // 0x0A, confirmed
    pub const EFFECT_COLOR_RATE: i32 = 10;
    // 0x0B, confirmed
    pub const EFFECT_COLOR_ADD_R: i32 = 11;
    // 0x0C, confirmed
    pub const EFFECT_COLOR_ADD_G: i32 = 12;
    // 0x0D, confirmed
    pub const EFFECT_COLOR_ADD_B: i32 = 13;
    // 0x0E, confirmed
    pub const EFFECT_X_EVE: i32 = 14;
    // 0x0F, confirmed
    pub const EFFECT_Y_EVE: i32 = 15;
    // 0x10, confirmed
    pub const EFFECT_Z_EVE: i32 = 16;
    // 0x11, confirmed
    pub const EFFECT_MONO_EVE: i32 = 17;
    // 0x12, confirmed
    pub const EFFECT_REVERSE_EVE: i32 = 18;
    // 0x13, confirmed
    pub const EFFECT_BRIGHT_EVE: i32 = 19;
    // 0x14, confirmed
    pub const EFFECT_DARK_EVE: i32 = 20;
    // 0x15, confirmed
    pub const EFFECT_COLOR_R_EVE: i32 = 21;
    // 0x16, confirmed
    pub const EFFECT_COLOR_G_EVE: i32 = 22;
    // 0x17, confirmed
    pub const EFFECT_COLOR_B_EVE: i32 = 23;
    // 0x18, confirmed
    pub const EFFECT_COLOR_RATE_EVE: i32 = 24;
    // 0x19, confirmed
    pub const EFFECT_COLOR_ADD_R_EVE: i32 = 25;
    // 0x1A, confirmed
    pub const EFFECT_COLOR_ADD_G_EVE: i32 = 26;
    // 0x1B, confirmed
    pub const EFFECT_COLOR_ADD_B_EVE: i32 = 27;
    // 0x1C, confirmed
    pub const EFFECT_BEGIN_ORDER: i32 = 28;
    // 0x1D, confirmed
    pub const EFFECT_END_ORDER: i32 = 29;
    // 0x1E, confirmed
    pub const EFFECT_INIT: i32 = 30;
    // 0x1F, confirmed
    pub const EFFECT_WIPE_COPY: i32 = 31;
    // 0x20, confirmed
    pub const EFFECT_WIPE_ERASE: i32 = 32;
    // 0x21, confirmed
    pub const EFFECT_BEGIN_LAYER: i32 = 33;
    // 0x22, confirmed
    pub const EFFECT_END_LAYER: i32 = 34;

    // EFFECTLIST
    // 0x1, confirmed
    pub const EFFECTLIST_RESIZE: i32 = 1;
    // 0x2, confirmed
    pub const EFFECTLIST_GET_SIZE: i32 = 2;

    // EXCALL
    // 0x0, confirmed
    pub const EXCALL_STAGE: i32 = 0;
    // 0x01, confirmed
    pub const EXCALL_BACK: i32 = 1;
    // 0x02, confirmed
    pub const EXCALL_FRONT: i32 = 2;
    // 0x03, confirmed
    pub const EXCALL_NEXT: i32 = 3;
    // 0x4, confirmed
    pub const EXCALL_ALLOC: i32 = 4;
    // 0x5, confirmed
    pub const EXCALL_FREE: i32 = 5;
    // 0x6, confirmed
    pub const EXCALL_COUNTER: i32 = 6;
    // 0x7, confirmed
    pub const EXCALL_F: i32 = 7;
    // 0x8, confirmed
    pub const EXCALL_CHECK_ALLOC: i32 = 8;
    // 0x9, confirmed
    pub const EXCALL_FRAME_ACTION: i32 = 9;
    // 0xA, confirmed
    pub const EXCALL_FRAME_ACTION_CH: i32 = 10;
    // 0xC, confirmed
    pub const EXCALL_IS_EXCALL: i32 = 12;
    // 0xD, confirmed
    pub const EXCALL_SCRIPT: i32 = 13;

    // FILE
    // 0x01, confirmed
    pub const FILE_PRELOAD_OMV: i32 = 1;

    // FRAMEACTION
    // 0x0, confirmed
    pub const FRAMEACTION_COUNTER: i32 = 0;
    // 0x1, confirmed
    pub const FRAMEACTION_START: i32 = 1;
    // 0x2, confirmed
    pub const FRAMEACTION_END: i32 = 2;
    // 0x3, confirmed
    pub const FRAMEACTION_START_REAL: i32 = 3;
    // 0x4, confirmed
    pub const FRAMEACTION_IS_END_ACTION: i32 = 4;

    // FRAMEACTIONLIST
    // 0x1, confirmed
    pub const FRAMEACTIONLIST_RESIZE: i32 = 1;
    // 0x2, confirmed
    pub const FRAMEACTIONLIST_GET_SIZE: i32 = 2;

    // G00BUF
    // 0x0, confirmed
    pub const G00BUF_LOAD: i32 = 0;
    // 0x1, confirmed
    pub const G00BUF_FREE: i32 = 1;

    // G00BUFLIST
    // 0x0, confirmed
    pub const G00BUFLIST_FREE_ALL: i32 = 0;
    // 0x2, confirmed
    pub const G00BUFLIST_GET_SIZE: i32 = 2;

    // GROUP
    // 0x00, confirmed
    pub const GROUP_SEL: i32 = 0;
    // 0x01, confirmed
    pub const GROUP_SEL_CANCEL: i32 = 1;
    // 0x02, confirmed
    pub const GROUP_INIT: i32 = 2;
    // 0x03, confirmed
    pub const GROUP_START: i32 = 3;
    // 0x04, confirmed
    pub const GROUP_START_CANCEL: i32 = 4;
    // 0x05, confirmed
    pub const GROUP_END: i32 = 5;
    // 0x06, confirmed
    pub const GROUP_GET_HIT_NO: i32 = 6;
    // 0x07, confirmed
    pub const GROUP_GET_PUSHED_NO: i32 = 7;
    // 0x08, confirmed
    pub const GROUP_GET_DECIDED_NO: i32 = 8;
    // 0x0A, confirmed
    pub const GROUP_GET_RESULT: i32 = 10;
    // 0x0D, confirmed
    pub const GROUP_GET_RESULT_BUTTON_NO: i32 = 13;
    // 0x0E, confirmed
    pub const GROUP_ORDER: i32 = 14;
    // 0x0F, confirmed
    pub const GROUP_LAYER: i32 = 15;
    // 0x10, confirmed
    pub const GROUP_CANCEL_PRIORITY: i32 = 16;

    // GROUPLIST
    // 0x01, confirmed
    pub const GROUPLIST_ALLOC: i32 = 1;
    // 0x02, confirmed
    pub const GROUPLIST_FREE: i32 = 2;

    // INPUT
    // 0x00, high_confidence_inferred
    pub const INPUT_DECIDE: i32 = 0;
    // 0x01, high_confidence_inferred
    pub const INPUT_CANCEL: i32 = 1;
    // 0x2, confirmed
    pub const INPUT_CLEAR: i32 = 2;
    // 0x3, confirmed
    pub const INPUT_NEXT: i32 = 3;

    // INTEVENT
    // 0x0, confirmed
    pub const INTEVENT_SET: i32 = 0;
    // 0x1, confirmed
    pub const INTEVENT_LOOP: i32 = 1;
    // 0x2, confirmed
    pub const INTEVENT_TURN: i32 = 2;
    // 0x3, confirmed
    pub const INTEVENT_END: i32 = 3;
    // 0x4, confirmed
    pub const INTEVENT_WAIT: i32 = 4;
    // 0x5, confirmed
    pub const INTEVENT_CHECK: i32 = 5;
    // 0x7, confirmed
    pub const INTEVENT_SET_REAL: i32 = 7;
    // 0x8, confirmed
    pub const INTEVENT_LOOP_REAL: i32 = 8;
    // 0x9, confirmed
    pub const INTEVENT_TURN_REAL: i32 = 9;
    // 0xa, confirmed
    pub const INTEVENT_WAIT_KEY: i32 = 10;

    // INTEVENTLIST
    // 0x1, confirmed
    pub const INTEVENTLIST_RESIZE: i32 = 1;

    // INTLIST
    // 0x01, confirmed
    pub const INTLIST_SETS: i32 = 1;
    // 0x02, confirmed
    pub const INTLIST_RESIZE: i32 = 2;
    // 0x03, high_confidence_inferred
    pub const INTLIST_BIT: i32 = 3;
    // 0x04, high_confidence_inferred
    pub const INTLIST_BIT2: i32 = 4;
    // 0x05, high_confidence_inferred
    pub const INTLIST_BIT4: i32 = 5;
    // 0x06, high_confidence_inferred
    pub const INTLIST_BIT8: i32 = 6;
    // 0x07, high_confidence_inferred
    pub const INTLIST_BIT16: i32 = 7;
    // 0x08, confirmed
    pub const INTLIST_CLEAR: i32 = 8;
    // 0x09, confirmed
    pub const INTLIST_GET_SIZE: i32 = 9;
    // 0x0A, confirmed
    pub const INTLIST_INIT: i32 = 10;

    // KEY
    // 0x0, confirmed
    pub const KEY_GET_FLICK_ANGLE: i32 = 0;
    // 0x1, confirmed
    pub const KEY_ON_DOWN: i32 = 1;
    // 0x4, confirmed
    pub const KEY_ON_UP: i32 = 4;
    // 0x5, confirmed
    pub const KEY_ON_DOWN_UP: i32 = 5;
    // 0x6, confirmed
    pub const KEY_IS_DOWN: i32 = 6;
    // 0x7, confirmed
    pub const KEY_IS_UP: i32 = 7;
    // 0xA, confirmed
    pub const KEY_ON_FLICK: i32 = 10;
    // 0xE, confirmed
    pub const KEY_GET_FLICK_PIXEL: i32 = 14;
    // 0xF, confirmed
    pub const KEY_GET_FLICK_MM: i32 = 15;

    // KEYLIST
    // 0x0, confirmed
    pub const KEYLIST_WAIT: i32 = 0;
    // 0x1, confirmed
    pub const KEYLIST_WAIT_FORCE: i32 = 1;
    // 0x3, confirmed
    pub const KEYLIST_CLEAR: i32 = 3;
    // 0x5, confirmed
    pub const KEYLIST_NEXT: i32 = 5;

    // MASK
    // 0x0, confirmed
    pub const MASK_CREATE: i32 = 0;
    // 0x1, confirmed
    pub const MASK_INIT: i32 = 1;
    // 0x2, confirmed
    pub const MASK_X_EVE: i32 = 2;
    // 0x3, confirmed
    pub const MASK_Y_EVE: i32 = 3;
    // 0x4, confirmed
    pub const MASK_X: i32 = 4;
    // 0x5, confirmed
    pub const MASK_Y: i32 = 5;

    // MASKLIST
    // 0x1, confirmed
    pub const MASKLIST_GET_SIZE: i32 = 1;

    // MATH
    // 0x0, confirmed
    pub const MATH_RAND: i32 = 0;
    // 0x1, confirmed
    pub const MATH_TOSTR: i32 = 1;
    // 0x2, confirmed
    pub const MATH_TIMETABLE: i32 = 2;
    // 0x3, confirmed
    pub const MATH_MAX: i32 = 3;
    // 0x4, confirmed
    pub const MATH_MIN: i32 = 4;
    // 0x5, confirmed
    pub const MATH_ABS: i32 = 5;
    // 0x6, confirmed
    pub const MATH_SIN: i32 = 6;
    // 0x7, confirmed
    pub const MATH_COS: i32 = 7;
    // 0x8, confirmed
    pub const MATH_TAN: i32 = 8;
    // 0x9, confirmed
    pub const MATH_LINEAR: i32 = 9;
    // 0xA, confirmed
    pub const MATH_LIMIT: i32 = 10;
    // 0xB, confirmed
    pub const MATH_TOSTR_ZERO: i32 = 11;
    // 0xC, confirmed
    pub const MATH_TOSTR_ZEN: i32 = 12;
    // 0xD, confirmed
    pub const MATH_TOSTR_ZEN_ZERO: i32 = 13;
    // 0xE, confirmed
    pub const MATH_SQRT: i32 = 14;
    // 0xF, confirmed
    pub const MATH_DISTANCE: i32 = 15;
    // 0x10, confirmed
    pub const MATH_ARCSIN: i32 = 16;
    // 0x11, confirmed
    pub const MATH_ARCCOS: i32 = 17;
    // 0x12, confirmed
    pub const MATH_ARCTAN: i32 = 18;
    // 0x13, confirmed
    pub const MATH_LOG: i32 = 19;
    // 0x14, confirmed
    pub const MATH_LOG2: i32 = 20;
    // 0x15, confirmed
    pub const MATH_LOG10: i32 = 21;
    // 0x16, confirmed
    pub const MATH_ANGLE: i32 = 22;
    // 0x17, confirmed
    pub const MATH_TOSTR_BY_CODE: i32 = 23;

    // MOUSE
    // 0x00, high_confidence_inferred
    pub const MOUSE_POS_X: i32 = 0;
    // 0x01, high_confidence_inferred
    pub const MOUSE_POS_Y: i32 = 1;
    // 0x02, high_confidence_inferred
    pub const MOUSE_GET_POS_X: i32 = 2;
    // 0x03, high_confidence_inferred
    pub const MOUSE_GET_POS_Y: i32 = 3;
    // 0x4, confirmed
    pub const MOUSE_CLEAR: i32 = 4;
    // 0x5, confirmed
    pub const MOUSE_WHEEL: i32 = 5;
    // 0x06, high_confidence_inferred
    pub const MOUSE_LEFT: i32 = 6;
    // 0x07, high_confidence_inferred
    pub const MOUSE_RIGHT: i32 = 7;
    // 0x8, confirmed
    pub const MOUSE_NEXT: i32 = 8;
    // 0x9, confirmed
    pub const MOUSE_GET_POS: i32 = 9;
    // 0xA, confirmed
    pub const MOUSE_SET_POS: i32 = 10;

    // MOV
    // 0x00, confirmed
    pub const MOV_PLAY: i32 = 0;
    // 0x01, confirmed
    pub const MOV_STOP: i32 = 1;
    // 0x02, confirmed
    pub const MOV_PLAY_WAIT: i32 = 2;
    // 0x03, confirmed
    pub const MOV_PLAY_WAIT_KEY: i32 = 3;

    // MSGBK
    // 0x1, confirmed
    pub const MSGBK_INSERT_MSG: i32 = 1;
    // 0x2, confirmed
    pub const MSGBK_GO_NEXT_MSG: i32 = 2;
    // 0x3, confirmed
    pub const MSGBK_ADD_MSG: i32 = 3;
    // 0x4, confirmed
    pub const MSGBK_ADD_KOE: i32 = 4;
    // 0x5, confirmed
    pub const MSGBK_ADD_NAMAE: i32 = 5;

    // MWND
    // 0x00, confirmed
    pub const MWND_SET_WAKU: i32 = 0;
    // 0x01, convention
    pub const MWND_OPEN: i32 = 1;
    // 0x02, convention
    pub const MWND_CLOSE: i32 = 2;
    // 0x03, confirmed
    pub const MWND_CLEAR: i32 = 3;
    // 0x04, confirmed
    pub const MWND_PRINT: i32 = 4;
    // 0x05, confirmed
    pub const MWND_SEL: i32 = 5;
    // 0x06, confirmed
    pub const MWND_NL: i32 = 6;
    // 0x07, confirmed
    pub const MWND_SIZE: i32 = 7;
    // 0x08, confirmed
    pub const MWND_COLOR: i32 = 8;
    // 0x09, confirmed
    pub const MWND_KOE: i32 = 9;
    // 0x0A, confirmed
    pub const MWND_LAYER: i32 = 10;
    // 0x0B, confirmed
    pub const MWND_WORLD: i32 = 11;
    // 0x0C, confirmed
    pub const MWND_RUBY: i32 = 12;
    // 0x0D, convention
    pub const MWND_CLOSE_WAIT: i32 = 13;
    // 0x0E, confirmed
    pub const MWND_CLOSE_NOWAIT: i32 = 14;
    // 0x0F, convention
    pub const MWND_OPEN_WAIT: i32 = 15;
    // 0x10, confirmed
    pub const MWND_OPEN_NOWAIT: i32 = 16;
    // 0x11, confirmed
    pub const MWND_NLI: i32 = 17;
    // 0x12, confirmed
    pub const MWND_WAIT_MSG: i32 = 18;
    // 0x13, confirmed
    pub const MWND_PP: i32 = 19;
    // 0x14, confirmed
    pub const MWND_R: i32 = 20;
    // 0x15, confirmed
    pub const MWND_SET_FACE: i32 = 21;
    // 0x16, confirmed
    pub const MWND_CLEAR_FACE: i32 = 22;
    // 0x1A, confirmed
    pub const MWND_KOE_PLAY_WAIT: i32 = 26;
    // 0x1B, confirmed
    pub const MWND_KOE_PLAY_WAIT_KEY: i32 = 27;
    // 0x1C, confirmed
    pub const MWND_CLEAR_INDENT: i32 = 28;
    // 0x1D, confirmed
    pub const MWND_NEXT_MSG: i32 = 29;
    // 0x1E, confirmed
    pub const MWND_OBJECT: i32 = 30;
    // 0x1F, confirmed
    pub const MWND_MULTI_MSG: i32 = 31;
    // 0x20, confirmed
    pub const MWND_BUTTON: i32 = 32;
    // 0x21, confirmed
    pub const MWND_GET_DEFAULT_OPEN_ANIME_TIME: i32 = 33;
    // 0x22, confirmed
    pub const MWND_SET_OPEN_ANIME_TIME: i32 = 34;
    // 0x23, confirmed
    pub const MWND_SET_CLOSE_ANIME_TIME: i32 = 35;
    // 0x24, confirmed
    pub const MWND_GET_DEFAULT_OPEN_ANIME_TYPE: i32 = 36;
    // 0x25, confirmed
    pub const MWND_SET_OPEN_ANIME_TYPE: i32 = 37;
    // 0x26, confirmed
    pub const MWND_GET_CLOSE_ANIME_TYPE: i32 = 38;
    // 0x27, confirmed
    pub const MWND_GET_DEFAULT_CLOSE_ANIME_TYPE: i32 = 39;
    // 0x28, confirmed
    pub const MWND_GET_DEFAULT_CLOSE_ANIME_TIME: i32 = 40;
    // 0x29, confirmed
    pub const MWND_INIT_OPEN_ANIME_TYPE: i32 = 41;
    // 0x2A, confirmed
    pub const MWND_INIT_OPEN_ANIME_TIME: i32 = 42;
    // 0x2B, confirmed
    pub const MWND_INIT_CLOSE_ANIME_TYPE: i32 = 43;
    // 0x2C, confirmed
    pub const MWND_INIT_CLOSE_ANIME_TIME: i32 = 44;
    // 0x2D, confirmed
    pub const MWND_SET_CLOSE_ANIME_TYPE: i32 = 45;
    // 0x2E, confirmed
    pub const MWND_GET_CLOSE_ANIME_TIME: i32 = 46;
    // 0x2F, confirmed
    pub const MWND_GET_OPEN_ANIME_TIME: i32 = 47;
    // 0x30, confirmed
    pub const MWND_GET_OPEN_ANIME_TYPE: i32 = 48;
    // 0x31, confirmed
    pub const MWND_MSG_BLOCK: i32 = 49;
    // 0x32, confirmed
    pub const MWND_SELMSG: i32 = 50;
    // 0x33, confirmed
    pub const MWND_SEL_CANCEL: i32 = 51;
    // 0x34, confirmed
    pub const MWND_SELMSG_CANCEL: i32 = 52;
    // 0x35, confirmed
    pub const MWND_FACE: i32 = 53;
    // 0x36, confirmed
    pub const MWND_PAGE: i32 = 54;
    // 0x37, confirmed
    pub const MWND____NOVEL_CLEAR: i32 = 55;
    // 0x38, confirmed
    pub const MWND_INDENT: i32 = 56;
    // 0x39, confirmed
    pub const MWND____OVER_FLOW_PRINT: i32 = 57;
    // 0x3A, confirmed
    pub const MWND_START_SLIDE_MSG: i32 = 58;
    // 0x3B, confirmed
    pub const MWND_MSG_PP_BLOCK: i32 = 59;
    // 0x3C, confirmed
    pub const MWND_END_SLIDE_MSG: i32 = 60;
    // 0x3D, confirmed
    pub const MWND____SLIDE_MSG: i32 = 61;
    // 0x3F, confirmed
    pub const MWND____OVER_FLOW_NAMAE: i32 = 63;
    // 0x40, confirmed
    pub const MWND_END_CLOSE: i32 = 64;
    // 0x41, confirmed
    pub const MWND_CHECK_OPEN: i32 = 65;
    // 0x42, confirmed
    pub const MWND_INIT_WINDOW_POS: i32 = 66;
    // 0x43, confirmed
    pub const MWND_INIT_WINDOW_SIZE: i32 = 67;
    // 0x44, confirmed
    pub const MWND_SET_WINDOW_POS: i32 = 68;
    // 0x45, confirmed
    pub const MWND_SET_WINDOW_SIZE: i32 = 69;
    // 0x46, confirmed
    pub const MWND_GET_WINDOW_POS_X: i32 = 70;
    // 0x47, confirmed
    pub const MWND_GET_WINDOW_POS_Y: i32 = 71;
    // 0x48, confirmed
    pub const MWND_GET_WINDOW_SIZE_X: i32 = 72;
    // 0x49, confirmed
    pub const MWND_GET_WINDOW_SIZE_Y: i32 = 73;
    // 0x4A, confirmed
    pub const MWND_INIT_WINDOW_MOJI_CNT: i32 = 74;
    // 0x4B, confirmed
    pub const MWND_SET_WINDOW_MOJI_CNT: i32 = 75;
    // 0x4C, confirmed
    pub const MWND_GET_WINDOW_MOJI_CNT_X: i32 = 76;
    // 0x4D, confirmed
    pub const MWND_GET_WINDOW_MOJI_CNT_Y: i32 = 77;
    // 0x4E, confirmed
    pub const MWND_SET_WAKU_FILE: i32 = 78;
    // 0x4F, confirmed
    pub const MWND_INIT_WAKU_FILE: i32 = 79;
    // 0x50, confirmed
    pub const MWND_GET_WAKU_FILE: i32 = 80;
    // 0x51, confirmed
    pub const MWND_SET_FILTER_FILE: i32 = 81;
    // 0x52, confirmed
    pub const MWND_INIT_FILTER_FILE: i32 = 82;
    // 0x53, confirmed
    pub const MWND_GET_FILTER_FILE: i32 = 83;
    // 0x54, confirmed
    pub const MWND_REP_POS: i32 = 84;
    // 0x55, confirmed
    pub const MWND_SET_NAMAE: i32 = 85;
    // 0x56, confirmed
    pub const MWND_MSGBTN: i32 = 86;

    // MWNDLIST
    // 0x01, confirmed
    pub const MWNDLIST_CLOSE: i32 = 1;
    // 0x02, confirmed
    pub const MWNDLIST_CLOSE_WAIT: i32 = 2;
    // 0x03, confirmed
    pub const MWNDLIST_CLOSE_NOWAIT: i32 = 3;

    // OBJECT
    // 0x00, confirmed
    pub const OBJECT_DISP: i32 = 0;
    // 0x01, confirmed
    pub const OBJECT_PATNO: i32 = 1;
    // 0x02, confirmed
    pub const OBJECT_LAYER: i32 = 2;
    // 0x03, confirmed
    pub const OBJECT_X: i32 = 3;
    // 0x04, confirmed
    pub const OBJECT_Y: i32 = 4;
    // 0x05, confirmed
    pub const OBJECT_Z: i32 = 5;
    // 0x06, confirmed
    pub const OBJECT_CENTER_X: i32 = 6;
    // 0x07, confirmed
    pub const OBJECT_CENTER_Y: i32 = 7;
    // 0x08, confirmed
    pub const OBJECT_CENTER_Z: i32 = 8;
    // 0x09, confirmed
    pub const OBJECT_CENTER_REP_X: i32 = 9;
    // 0x0A, confirmed
    pub const OBJECT_CENTER_REP_Y: i32 = 10;
    // 0x0B, confirmed
    pub const OBJECT_CENTER_REP_Z: i32 = 11;
    // 0x0C, confirmed
    pub const OBJECT_SCALE_X: i32 = 12;
    // 0x0D, confirmed
    pub const OBJECT_SCALE_Y: i32 = 13;
    // 0x0E, confirmed
    pub const OBJECT_SCALE_Z: i32 = 14;
    // 0x0F, confirmed
    pub const OBJECT_ROTATE_X: i32 = 15;
    // 0x10, confirmed
    pub const OBJECT_ROTATE_Y: i32 = 16;
    // 0x11, confirmed
    pub const OBJECT_ROTATE_Z: i32 = 17;
    // 0x12, confirmed
    pub const OBJECT_CLIP_USE: i32 = 18;
    // 0x13, confirmed
    pub const OBJECT_CLIP_LEFT: i32 = 19;
    // 0x14, confirmed
    pub const OBJECT_CLIP_TOP: i32 = 20;
    // 0x15, confirmed
    pub const OBJECT_CLIP_RIGHT: i32 = 21;
    // 0x16, confirmed
    pub const OBJECT_CLIP_BOTTOM: i32 = 22;
    // 0x17, confirmed
    pub const OBJECT_COLOR_RATE: i32 = 23;
    // 0x18, confirmed
    pub const OBJECT_GET_SIZE_X: i32 = 24;
    // 0x19, confirmed
    pub const OBJECT_GET_SIZE_Y: i32 = 25;
    // 0x1A, confirmed
    pub const OBJECT_SET_BUTTON_CALL: i32 = 26;
    // 0x1B, confirmed
    pub const OBJECT_TR: i32 = 27;
    // 0x1C, confirmed
    pub const OBJECT_MONO: i32 = 28;
    // 0x1D, confirmed
    pub const OBJECT_REVERSE: i32 = 29;
    // 0x1E, confirmed
    pub const OBJECT_BRIGHT: i32 = 30;
    // 0x1F, confirmed
    pub const OBJECT_DARK: i32 = 31;
    // 0x20, confirmed
    pub const OBJECT_COLOR_R: i32 = 32;
    // 0x21, confirmed
    pub const OBJECT_COLOR_G: i32 = 33;
    // 0x22, confirmed
    pub const OBJECT_COLOR_B: i32 = 34;
    // 0x23, confirmed
    pub const OBJECT_INIT: i32 = 35;
    // 0x24, confirmed
    pub const OBJECT_FREE: i32 = 36;
    // 0x25, confirmed
    pub const OBJECT_INIT_PARAM: i32 = 37;
    // 0x26, confirmed
    pub const OBJECT_CREATE: i32 = 38;
    // 0x27, confirmed
    pub const OBJECT_CREATE_STRING: i32 = 39;
    // 0x28, confirmed
    pub const OBJECT_CREATE_RECT: i32 = 40;
    // 0x29, confirmed
    pub const OBJECT_CREATE_COPY_FROM: i32 = 41;
    // 0x2A, confirmed
    pub const OBJECT_SET_BUTTON: i32 = 42;
    // 0x2B, confirmed
    pub const OBJECT_CREATE_MESH: i32 = 43;
    // 0x2C, confirmed
    pub const OBJECT_WORLD: i32 = 44;
    // 0x2D, confirmed
    pub const OBJECT_CREATE_BILLBOARD: i32 = 45;
    // 0x2E, confirmed
    pub const OBJECT_BLEND: i32 = 46;
    // 0x2F, confirmed
    pub const OBJECT_GET_SIZE_Z: i32 = 47;
    // 0x30, confirmed
    pub const OBJECT_SET_POS: i32 = 48;
    // 0x31, confirmed
    pub const OBJECT_SET_SCALE: i32 = 49;
    // 0x32, confirmed
    pub const OBJECT_SET_ROTATE: i32 = 50;
    // 0x33, confirmed
    pub const OBJECT_PATNO_EVE: i32 = 51;
    // 0x34, confirmed
    pub const OBJECT_FRAME_ACTION: i32 = 52;
    // 0x35, confirmed
    pub const OBJECT_CHANGE_FILE: i32 = 53;
    // 0x36, confirmed
    pub const OBJECT_X_REP: i32 = 54;
    // 0x37, confirmed
    pub const OBJECT_ORDER: i32 = 55;
    // 0x38, confirmed
    pub const OBJECT_WIPE_COPY: i32 = 56;
    // 0x39, confirmed
    pub const OBJECT_COLOR_ADD_R: i32 = 57;
    // 0x3A, confirmed
    pub const OBJECT_COLOR_ADD_G: i32 = 58;
    // 0x3B, confirmed
    pub const OBJECT_COLOR_ADD_B: i32 = 59;
    // 0x3C, confirmed
    pub const OBJECT_CLEAR_BUTTON_CALL: i32 = 60;
    // 0x3D, confirmed
    pub const OBJECT_CLEAR_BUTTON: i32 = 61;
    // 0x3E, confirmed
    pub const OBJECT_GET_FILE_NAME: i32 = 62;
    // 0x3F, confirmed
    pub const OBJECT_Y_REP: i32 = 63;
    // 0x40, confirmed
    pub const OBJECT_X_EVE: i32 = 64;
    // 0x41, confirmed
    pub const OBJECT_Y_EVE: i32 = 65;
    // 0x42, confirmed
    pub const OBJECT_Z_EVE: i32 = 66;
    // 0x43, confirmed
    pub const OBJECT_CENTER_X_EVE: i32 = 67;
    // 0x44, confirmed
    pub const OBJECT_CENTER_Y_EVE: i32 = 68;
    // 0x45, confirmed
    pub const OBJECT_CENTER_Z_EVE: i32 = 69;
    // 0x46, confirmed
    pub const OBJECT_CENTER_REP_X_EVE: i32 = 70;
    // 0x47, confirmed
    pub const OBJECT_CENTER_REP_Y_EVE: i32 = 71;
    // 0x48, confirmed
    pub const OBJECT_CENTER_REP_Z_EVE: i32 = 72;
    // 0x49, confirmed
    pub const OBJECT_SCALE_X_EVE: i32 = 73;
    // 0x4A, confirmed
    pub const OBJECT_SCALE_Y_EVE: i32 = 74;
    // 0x4B, confirmed
    pub const OBJECT_SCALE_Z_EVE: i32 = 75;
    // 0x4C, confirmed
    pub const OBJECT_ROTATE_X_EVE: i32 = 76;
    // 0x4D, confirmed
    pub const OBJECT_ROTATE_Y_EVE: i32 = 77;
    // 0x4E, confirmed
    pub const OBJECT_ROTATE_Z_EVE: i32 = 78;
    // 0x4F, confirmed
    pub const OBJECT_CLIP_LEFT_EVE: i32 = 79;
    // 0x50, confirmed
    pub const OBJECT_CLIP_TOP_EVE: i32 = 80;
    // 0x51, confirmed
    pub const OBJECT_CLIP_RIGHT_EVE: i32 = 81;
    // 0x52, confirmed
    pub const OBJECT_CLIP_BOTTOM_EVE: i32 = 82;
    // 0x53, confirmed
    pub const OBJECT_SRC_CLIP_LEFT_EVE: i32 = 83;
    // 0x54, confirmed
    pub const OBJECT_SRC_CLIP_TOP_EVE: i32 = 84;
    // 0x55, confirmed
    pub const OBJECT_SRC_CLIP_RIGHT_EVE: i32 = 85;
    // 0x56, confirmed
    pub const OBJECT_SRC_CLIP_BOTTOM_EVE: i32 = 86;
    // 0x57, confirmed
    pub const OBJECT_TR_EVE: i32 = 87;
    // 0x58, confirmed
    pub const OBJECT_MONO_EVE: i32 = 88;
    // 0x59, confirmed
    pub const OBJECT_REVERSE_EVE: i32 = 89;
    // 0x5A, confirmed
    pub const OBJECT_BRIGHT_EVE: i32 = 90;
    // 0x5B, confirmed
    pub const OBJECT_ALL_EVE: i32 = 91;
    // 0x5C, confirmed
    pub const OBJECT_WIPE_ERASE: i32 = 92;
    // 0x5D, confirmed
    pub const OBJECT_CHILD: i32 = 93;
    // 0x5F, confirmed
    pub const OBJECT_SET_BUTTON_STATE_NORMAL: i32 = 95;
    // 0x60, confirmed
    pub const OBJECT_SET_BUTTON_STATE_SELECT: i32 = 96;
    // 0x61, confirmed
    pub const OBJECT_SET_BUTTON_STATE_DISABLE: i32 = 97;
    // 0x62, confirmed
    pub const OBJECT_SET_BUTTON_PUSHKEEP: i32 = 98;
    // 0x63, confirmed
    pub const OBJECT_SET_STRING: i32 = 99;
    // 0x64, confirmed
    pub const OBJECT_GET_PIXEL_COLOR_A: i32 = 100;
    // 0x65, confirmed
    pub const OBJECT_GET_PIXEL_COLOR_R: i32 = 101;
    // 0x66, confirmed
    pub const OBJECT_GET_PIXEL_COLOR_G: i32 = 102;
    // 0x67, confirmed
    pub const OBJECT_GET_PIXEL_COLOR_B: i32 = 103;
    // 0x68, confirmed
    pub const OBJECT_SET_STRING_PARAM: i32 = 104;
    // 0x69, confirmed
    pub const OBJECT_DARK_EVE: i32 = 105;
    // 0x6A, confirmed
    pub const OBJECT_COLOR_R_EVE: i32 = 106;
    // 0x6B, confirmed
    pub const OBJECT_COLOR_G_EVE: i32 = 107;
    // 0x6C, confirmed
    pub const OBJECT_COLOR_B_EVE: i32 = 108;
    // 0x6D, confirmed
    pub const OBJECT_TONECURVE_NO: i32 = 109;
    // 0x6E, confirmed
    pub const OBJECT_Z_REP: i32 = 110;
    // 0x6F, confirmed
    pub const OBJECT_F: i32 = 111;
    // 0x70, confirmed
    pub const OBJECT_X_REP_EVE: i32 = 112;
    // 0x71, confirmed
    pub const OBJECT_Y_REP_EVE: i32 = 113;
    // 0x72, confirmed
    pub const OBJECT_Z_REP_EVE: i32 = 114;
    // 0x73, confirmed
    pub const OBJECT_FRAME_ACTION_CH: i32 = 115;
    // 0x74, confirmed
    pub const OBJECT_CREATE_SAVE_THUMB: i32 = 116;
    // 0x75, confirmed
    pub const OBJECT_GET_ELEMENT_NAME: i32 = 117;
    // 0x76, confirmed
    pub const OBJECT_GET_BUTTON_STATE: i32 = 118;
    // 0x77, confirmed
    pub const OBJECT_GET_BUTTON_PUSHKEEP: i32 = 119;
    // 0x78, confirmed
    pub const OBJECT_CREATE_MOVIE: i32 = 120;
    // 0x79, confirmed
    pub const OBJECT_CREATE_MOVIE_LOOP: i32 = 121;
    // 0x7A, confirmed
    pub const OBJECT_CREATE_MOVIE_WAIT: i32 = 122;
    // 0x7B, confirmed
    pub const OBJECT_GET_BUTTON_HIT_STATE: i32 = 123;
    // 0x7C, confirmed
    pub const OBJECT_GET_BUTTON_REAL_STATE: i32 = 124;
    // 0x7D, confirmed
    pub const OBJECT_PAUSE_MOVIE: i32 = 125;
    // 0x7E, confirmed
    pub const OBJECT_RESUME_MOVIE: i32 = 126;
    // 0x7F, confirmed
    pub const OBJECT_CHECK_MOVIE: i32 = 127;
    // 0x80, confirmed
    pub const OBJECT_WAIT_MOVIE: i32 = 128;
    // 0x81, confirmed
    pub const OBJECT_CREATE_WEATHER: i32 = 129;
    // 0x82, confirmed
    pub const OBJECT_SET_WEATHER_PARAM_TYPE_A: i32 = 130;
    // 0x83, confirmed
    pub const OBJECT_SET_WEATHER_PARAM_TYPE_B: i32 = 131;
    // 0x84, confirmed
    pub const OBJECT_CREATE_NUMBER: i32 = 132;
    // 0x85, confirmed
    pub const OBJECT_SET_NUMBER: i32 = 133;
    // 0x86, confirmed
    pub const OBJECT_SET_NUMBER_PARAM: i32 = 134;
    // 0x87, confirmed
    pub const OBJECT_GET_STRING: i32 = 135;
    // 0x88, confirmed
    pub const OBJECT_GET_NUMBER: i32 = 136;
    // 0x89, confirmed
    pub const OBJECT_SEEK_MOVIE: i32 = 137;
    // 0x8A, confirmed
    pub const OBJECT_GET_MOVIE_SEEK_TIME: i32 = 138;
    // 0x8B, confirmed
    pub const OBJECT_CLICK_DISABLE: i32 = 139;
    // 0x8C, confirmed
    pub const OBJECT_TR_REP_EVE: i32 = 140;
    // 0x8D, confirmed
    pub const OBJECT_TR_REP: i32 = 141;
    // 0x8E, confirmed
    pub const OBJECT_WAIT_MOVIE_KEY: i32 = 142;
    // 0x8F, confirmed
    pub const OBJECT_CREATE_MOVIE_WAIT_KEY: i32 = 143;
    // 0x90, confirmed
    pub const OBJECT_FOG_USE: i32 = 144;
    // 0x91, confirmed
    pub const OBJECT_MASK_NO: i32 = 145;
    // 0x92, confirmed
    pub const OBJECT_CULLING: i32 = 146;
    // 0x93, confirmed
    pub const OBJECT_ALPHA_TEST: i32 = 147;
    // 0x94, confirmed
    pub const OBJECT_ALPHA_BLEND: i32 = 148;
    // 0x95, confirmed
    pub const OBJECT_SRC_CLIP_USE: i32 = 149;
    // 0x96, confirmed
    pub const OBJECT_SRC_CLIP_LEFT: i32 = 150;
    // 0x97, confirmed
    pub const OBJECT_SRC_CLIP_TOP: i32 = 151;
    // 0x98, confirmed
    pub const OBJECT_SRC_CLIP_RIGHT: i32 = 152;
    // 0x99, confirmed
    pub const OBJECT_SRC_CLIP_BOTTOM: i32 = 153;
    // 0x9A, confirmed
    pub const OBJECT_COLOR_RATE_EVE: i32 = 154;
    // 0x9B, confirmed
    pub const OBJECT_COLOR_ADD_R_EVE: i32 = 155;
    // 0x9C, confirmed
    pub const OBJECT_COLOR_ADD_G_EVE: i32 = 156;
    // 0x9D, confirmed
    pub const OBJECT_COLOR_ADD_B_EVE: i32 = 157;
    // 0x9E, confirmed
    pub const OBJECT_SET_CENTER: i32 = 158;
    // 0x9F, confirmed
    pub const OBJECT_SET_CENTER_REP: i32 = 159;
    // 0xA0, confirmed
    pub const OBJECT_SET_CLIP: i32 = 160;
    // 0xA1, confirmed
    pub const OBJECT_SET_SRC_CLIP: i32 = 161;
    // 0xA2, confirmed
    pub const OBJECT_ADD_HINTS: i32 = 162;
    // 0xA3, confirmed
    pub const OBJECT_CLEAR_HINTS: i32 = 163;
    // 0xA4, confirmed
    pub const OBJECT_SET_BUTTON_GROUP: i32 = 164;
    // 0xA5, confirmed
    pub const OBJECT_CREATE_CAPTURE: i32 = 165;
    // 0xA6, confirmed
    pub const OBJECT_SET_CHILD_SORT_TYPE_DEFAULT: i32 = 166;
    // 0xA7, confirmed
    pub const OBJECT_SET_CHILD_SORT_TYPE_TEST: i32 = 167;
    // 0xA8, confirmed
    pub const OBJECT_LIGHT_NO: i32 = 168;
    // 0xA9, confirmed
    pub const OBJECT_GET_PAT_CNT: i32 = 169;
    // 0xAA, confirmed
    pub const OBJECT_CREATE_CAPTURE_THUMB: i32 = 170;
    // 0xAB, confirmed
    pub const OBJECT_END_MOVIE_LOOP: i32 = 171;
    // 0xAC, confirmed
    pub const OBJECT_SET_MOVIE_AUTO_FREE: i32 = 172;
    // 0xAE, confirmed
    pub const OBJECT_EXIST_TYPE: i32 = 174;
    // 0xAF, confirmed
    pub const OBJECT_SET_BUTTON_ALPHA_TEST: i32 = 175;
    // 0xB0, confirmed
    pub const OBJECT_GET_BUTTON_ALPHA_TEST: i32 = 176;
    // 0xB1, confirmed
    pub const OBJECT_CREATE_EMOTE: i32 = 177;
    // 0xB2, confirmed
    pub const OBJECT_EMOTE_PLAY_TIMELINE: i32 = 178;
    // 0xB3, confirmed
    pub const OBJECT_EMOTE_STOP_TIMELINE: i32 = 179;
    // 0xB5, confirmed
    pub const OBJECT_EMOTE_CHECK_PLAYING: i32 = 181;
    // 0xB6, confirmed
    pub const OBJECT_EMOTE_WAIT_PLAYING: i32 = 182;
    // 0xB7, confirmed
    pub const OBJECT_EMOTE_WAIT_PLAYING_KEY: i32 = 183;
    // 0xB8, confirmed
    pub const OBJECT_EMOTE_SKIP: i32 = 184;
    // 0xB9, confirmed
    pub const OBJECT_EMOTE_PASS: i32 = 185;
    // 0xBA, confirmed
    pub const OBJECT_EMOTE_KOE_CHARA_NO: i32 = 186;
    // 0xBB, confirmed
    pub const OBJECT_EMOTE_MOUTH_VOLUME: i32 = 187;
    // 0x01000000, confirmed
    pub const OBJECT_LOAD_GAN: i32 = 16777216;
    // 0x01000001, confirmed
    pub const OBJECT_START_GAN: i32 = 16777217;

    // OBJECTLIST
    // 0x03, confirmed
    pub const OBJECTLIST_GET_SIZE: i32 = 3;
    // 0x04, confirmed
    pub const OBJECTLIST_RESIZE: i32 = 4;

    // PCM
    // 0x00, confirmed
    pub const PCM_PLAY: i32 = 0;
    // 0x01, confirmed
    pub const PCM_STOP: i32 = 1;

    // PCMCH
    // 0x00, confirmed
    pub const PCMCH_PLAY: i32 = 0;
    // 0x01, confirmed
    pub const PCMCH_PLAY_WAIT: i32 = 1;
    // 0x02, confirmed
    pub const PCMCH_PLAY_LOOP: i32 = 2;
    // 0x03, confirmed
    pub const PCMCH_WAIT: i32 = 3;
    // 0x04, confirmed
    pub const PCMCH_CHECK: i32 = 4;
    // 0x05, confirmed
    pub const PCMCH_STOP: i32 = 5;
    // 0x06, confirmed
    pub const PCMCH_WAIT_KEY: i32 = 6;
    // 0x07, confirmed
    pub const PCMCH_WAIT_FADE_KEY: i32 = 7;
    // 0x08, confirmed
    pub const PCMCH_WAIT_FADE: i32 = 8;
    // 0x09, confirmed
    pub const PCMCH_RESUME: i32 = 9;
    // 0x0A, confirmed
    pub const PCMCH_PAUSE: i32 = 10;
    // 0x0B, confirmed
    pub const PCMCH_READY: i32 = 11;
    // 0x0C, confirmed
    pub const PCMCH_GET_VOLUME: i32 = 12;
    // 0x0D, confirmed
    pub const PCMCH_SET_VOLUME: i32 = 13;
    // 0x0E, confirmed
    pub const PCMCH_SET_VOLUME_MAX: i32 = 14;
    // 0x0F, confirmed
    pub const PCMCH_SET_VOLUME_MIN: i32 = 15;
    // 0x10, confirmed
    pub const PCMCH_READY_LOOP: i32 = 16;
    // 0x11, confirmed
    pub const PCMCH_RESUME_WAIT: i32 = 17;

    // PCMEVENT
    // 0x00, confirmed
    pub const PCMEVENT_START_ONESHOT: i32 = 0;
    // 0x01, confirmed
    pub const PCMEVENT_START_LOOP: i32 = 1;
    // 0x02, confirmed
    pub const PCMEVENT_START_RANDOM: i32 = 2;
    // 0x03, confirmed
    pub const PCMEVENT_STOP: i32 = 3;
    // 0x04, confirmed
    pub const PCMEVENT_WAIT: i32 = 4;
    // 0x05, confirmed
    pub const PCMEVENT_CHECK: i32 = 5;
    // 0x06, confirmed
    pub const PCMEVENT_WAIT_KEY: i32 = 6;

    // QUAKE
    // 0x1, confirmed
    pub const QUAKE_START_WAIT: i32 = 1;
    // 0x2, confirmed
    pub const QUAKE_START_WAIT_KEY: i32 = 2;
    // 0x5, confirmed
    pub const QUAKE_START_ALL_WAIT: i32 = 5;
    // 0x6, confirmed
    pub const QUAKE_START_ALL_WAIT_KEY: i32 = 6;
    // 0x8, confirmed
    pub const QUAKE_END: i32 = 8;
    // 0x9, confirmed
    pub const QUAKE_CHECK: i32 = 9;
    // 0xA, confirmed
    pub const QUAKE_WAIT: i32 = 10;
    // 0xB, confirmed
    pub const QUAKE_WAIT_KEY: i32 = 11;

    // SCREEN
    // 0x0, confirmed
    pub const SCREEN_MONO: i32 = 0;
    // 0x2, confirmed
    pub const SCREEN_REVERSE: i32 = 2;
    // 0x3, confirmed
    pub const SCREEN_BRIGHT: i32 = 3;
    // 0x4, confirmed
    pub const SCREEN_DARK: i32 = 4;
    // 0x5, confirmed
    pub const SCREEN_COLOR_R: i32 = 5;
    // 0x6, confirmed
    pub const SCREEN_COLOR_G: i32 = 6;
    // 0x7, confirmed
    pub const SCREEN_COLOR_B: i32 = 7;
    // 0x8, confirmed
    pub const SCREEN_COLOR_RATE: i32 = 8;
    // 0x9, confirmed
    pub const SCREEN_SHAKE: i32 = 9;
    // 0xA, confirmed
    pub const SCREEN_X: i32 = 10;
    // 0x0B, confirmed
    pub const SCREEN_X_EVE: i32 = 11;
    // 0x0C, confirmed
    pub const SCREEN_Y_EVE: i32 = 12;
    // 0x0D, confirmed
    pub const SCREEN_Z_EVE: i32 = 13;
    // 0x0E, confirmed
    pub const SCREEN_MONO_EVE: i32 = 14;
    // 0x0F, confirmed
    pub const SCREEN_REVERSE_EVE: i32 = 15;
    // 0x10, confirmed
    pub const SCREEN_BRIGHT_EVE: i32 = 16;
    // 0x11, confirmed
    pub const SCREEN_DARK_EVE: i32 = 17;
    // 0x12, confirmed
    pub const SCREEN_COLOR_R_EVE: i32 = 18;
    // 0x13, confirmed
    pub const SCREEN_COLOR_ADD_R: i32 = 19;
    // 0x14, confirmed
    pub const SCREEN_COLOR_ADD_G: i32 = 20;
    // 0x15, confirmed
    pub const SCREEN_COLOR_ADD_B: i32 = 21;
    // 0x16, confirmed
    pub const SCREEN_COLOR_G_EVE: i32 = 22;
    // 0x17, confirmed
    pub const SCREEN_COLOR_B_EVE: i32 = 23;
    // 0x18, confirmed
    pub const SCREEN_COLOR_RATE_EVE: i32 = 24;
    // 0x19, confirmed
    pub const SCREEN_QUAKE: i32 = 25;
    // 0x1A, confirmed
    pub const SCREEN_Y: i32 = 26;
    // 0x1B, confirmed
    pub const SCREEN_Z: i32 = 27;
    // 0x1C, confirmed
    pub const SCREEN_COLOR_ADD_R_EVE: i32 = 28;
    // 0x1D, confirmed
    pub const SCREEN_COLOR_ADD_G_EVE: i32 = 29;
    // 0x1E, confirmed
    pub const SCREEN_COLOR_ADD_B_EVE: i32 = 30;
    // 0x1F, confirmed
    pub const SCREEN_EFFECT: i32 = 31;

    // SCRIPT
    // 0x00, confirmed
    pub const SCRIPT_START_AUTO_MODE: i32 = 0;
    // 0x01, confirmed
    pub const SCRIPT_END_AUTO_MODE: i32 = 1;
    // 0x02, confirmed
    pub const SCRIPT_SET_SKIP_DISABLE: i32 = 2;
    // 0x03, confirmed
    pub const SCRIPT_SET_MWND_ANIME_OFF_FLAG: i32 = 3;
    // 0x04, confirmed
    pub const SCRIPT_SET_MWND_ANIME_ON_FLAG: i32 = 4;
    // 0x05, confirmed
    pub const SCRIPT_SET_SKIP_ENABLE: i32 = 5;
    // 0x06, confirmed
    pub const SCRIPT_SET_CTRL_SKIP_DISABLE: i32 = 6;
    // 0x07, confirmed
    pub const SCRIPT_SET_CTRL_SKIP_ENABLE: i32 = 7;
    // 0x08, confirmed
    pub const SCRIPT_SET_MWND_DISP_OFF_FLAG: i32 = 8;
    // 0x09, confirmed
    pub const SCRIPT_GET_MWND_ANIME_OFF_FLAG: i32 = 9;
    // 0x0A, confirmed
    pub const SCRIPT_SET_VSYNC_WAIT_OFF_FLAG: i32 = 10;
    // 0x0B, confirmed
    pub const SCRIPT_SET_QUAKE_STOP_FLAG: i32 = 11;
    // 0x0C, confirmed
    pub const SCRIPT_SET_MSG_BACK_DISABLE: i32 = 12;
    // 0x0D, confirmed
    pub const SCRIPT_SET_MSG_BACK_ENABLE: i32 = 13;
    // 0x0E, confirmed
    pub const SCRIPT_SET_MOUSE_DISP_OFF: i32 = 14;
    // 0x0F, confirmed
    pub const SCRIPT_SET_MOUSE_DISP_ON: i32 = 15;
    // 0x10, confirmed
    pub const SCRIPT_GET_MWND_ANIME_ON_FLAG: i32 = 16;
    // 0x11, confirmed
    pub const SCRIPT_GET_MWND_DISP_OFF_FLAG: i32 = 17;
    // 0x12, confirmed
    pub const SCRIPT_CHECK_SKIP: i32 = 18;
    // 0x13, confirmed
    pub const SCRIPT_SET_MOUSE_MOVE_BY_KEY_DISABLE: i32 = 19;
    // 0x14, confirmed
    pub const SCRIPT_SET_MOUSE_MOVE_BY_KEY_ENABLE: i32 = 20;
    // 0x15, confirmed
    pub const SCRIPT_SET_MSG_ASYNC_MODE_ON: i32 = 21;
    // 0x16, confirmed
    pub const SCRIPT_SET_MSG_ASYNC_MODE_OFF: i32 = 22;
    // 0x17, confirmed
    pub const SCRIPT_GET_QUAKE_STOP_FLAG: i32 = 23;
    // 0x18, confirmed
    pub const SCRIPT_GET_VSYNC_WAIT_OFF_FLAG: i32 = 24;
    // 0x19, confirmed
    pub const SCRIPT_SET_MESSAGE_SPEED: i32 = 25;
    // 0x1A, confirmed
    pub const SCRIPT_GET_MESSAGE_SPEED: i32 = 26;
    // 0x1B, confirmed
    pub const SCRIPT_SET_MESSAGE_SPEED_DEFAULT: i32 = 27;
    // 0x1C, confirmed
    pub const SCRIPT_SET_MESSAGE_NOWAIT_FLAG: i32 = 28;
    // 0x1D, confirmed
    pub const SCRIPT_GET_MESSAGE_NOWAIT_FLAG: i32 = 29;
    // 0x1E, confirmed
    pub const SCRIPT_SET_STOP_SKIP_BY_KEY_DISABLE: i32 = 30;
    // 0x1F, confirmed
    pub const SCRIPT_SET_STOP_SKIP_BY_KEY_ENABLE: i32 = 31;
    // 0x20, confirmed
    pub const SCRIPT_SET_SHORTCUT_DISABLE: i32 = 32;
    // 0x21, confirmed
    pub const SCRIPT_SET_SHORTCUT_ENABLE: i32 = 33;
    // 0x22, confirmed
    pub const SCRIPT_SET_MSG_BACK_OFF: i32 = 34;
    // 0x23, confirmed
    pub const SCRIPT_SET_MSG_BACK_ON: i32 = 35;
    // 0x24, confirmed
    pub const SCRIPT_START_BGMFADE: i32 = 36;
    // 0x25, confirmed
    pub const SCRIPT_END_BGMFADE: i32 = 37;
    // 0x26, confirmed
    pub const SCRIPT_SET_AUTO_SAVEPOINT_OFF: i32 = 38;
    // 0x27, confirmed
    pub const SCRIPT_SET_AUTO_SAVEPOINT_ON: i32 = 39;
    // 0x28, confirmed
    pub const SCRIPT_SET_KEY_DISABLE: i32 = 40;
    // 0x29, confirmed
    pub const SCRIPT_SET_KEY_ENABLE: i32 = 41;
    // 0x2A, confirmed
    pub const SCRIPT_SET_KOE_DONT_STOP_ON_FLAG: i32 = 42;
    // 0x2B, confirmed
    pub const SCRIPT_GET_KOE_DONT_STOP_ON_FLAG: i32 = 43;
    // 0x2C, confirmed
    pub const SCRIPT_SET_KOE_DONT_STOP_OFF_FLAG: i32 = 44;
    // 0x2D, confirmed
    pub const SCRIPT_GET_KOE_DONT_STOP_OFF_FLAG: i32 = 45;
    // 0x2E, confirmed
    pub const SCRIPT_SET_HIDE_MWND_DISABLE: i32 = 46;
    // 0x2F, confirmed
    pub const SCRIPT_SET_HIDE_MWND_ENABLE: i32 = 47;
    // 0x30, confirmed
    pub const SCRIPT_SET_SKIP_TRIGGER: i32 = 48;
    // 0x31, confirmed
    pub const SCRIPT_IGNORE_R_ON: i32 = 49;
    // 0x32, confirmed
    pub const SCRIPT_IGNORE_R_OFF: i32 = 50;
    // 0x33, confirmed
    pub const SCRIPT_SET_MSG_ASYNC_MODE_ON_ONCE: i32 = 51;
    // 0x34, confirmed
    pub const SCRIPT_SET_CURSOR_NO: i32 = 52;
    // 0x35, confirmed
    pub const SCRIPT_GET_CURSOR_NO: i32 = 53;
    // 0x36, confirmed
    pub const SCRIPT_SET_END_MSG_BY_KEY_DISABLE: i32 = 54;
    // 0x37, confirmed
    pub const SCRIPT_SET_END_MSG_BY_KEY_ENABLE: i32 = 55;
    // 0x38, confirmed
    pub const SCRIPT_SET_MSG_BACK_DISP_OFF: i32 = 56;
    // 0x39, confirmed
    pub const SCRIPT_SET_MSG_BACK_DISP_ON: i32 = 57;
    // 0x3C, confirmed
    pub const SCRIPT_SET_AUTO_MODE_MOJI_WAIT: i32 = 60;
    // 0x3D, confirmed
    pub const SCRIPT_SET_AUTO_MODE_MIN_WAIT: i32 = 61;
    // 0x3E, confirmed
    pub const SCRIPT_SET_AUTO_MODE_MOJI_WAIT_DEFAULT: i32 = 62;
    // 0x3F, confirmed
    pub const SCRIPT_SET_AUTO_MODE_MIN_WAIT_DEFAULT: i32 = 63;
    // 0x40, confirmed
    pub const SCRIPT_GET_AUTO_MODE_MOJI_WAIT: i32 = 64;
    // 0x41, confirmed
    pub const SCRIPT_GET_AUTO_MODE_MIN_WAIT: i32 = 65;
    // 0x42, confirmed
    pub const SCRIPT_SET_TIME_STOP_FLAG: i32 = 66;
    // 0x43, confirmed
    pub const SCRIPT_GET_TIME_STOP_FLAG: i32 = 67;
    // 0x44, confirmed
    pub const SCRIPT_SET_COUNTER_TIME_STOP_FLAG: i32 = 68;
    // 0x45, confirmed
    pub const SCRIPT_SET_FRAME_ACTION_TIME_STOP_FLAG: i32 = 69;
    // 0x46, confirmed
    pub const SCRIPT_SET_STAGE_TIME_STOP_FLAG: i32 = 70;
    // 0x47, confirmed
    pub const SCRIPT_GET_STAGE_TIME_STOP_FLAG: i32 = 71;
    // 0x48, confirmed
    pub const SCRIPT_GET_FRAME_ACTION_TIME_STOP_FLAG: i32 = 72;
    // 0x49, confirmed
    pub const SCRIPT_GET_COUNTER_TIME_STOP_FLAG: i32 = 73;
    // 0x4A, confirmed
    pub const SCRIPT_SET_SKIP_UNREAD_MESSAGE_FLAG: i32 = 74;
    // 0x4B, confirmed
    pub const SCRIPT_GET_SKIP_UNREAD_MESSAGE_FLAG: i32 = 75;
    // 0x4C, confirmed
    pub const SCRIPT_SET_AUTO_MODE_MOJI_CNT: i32 = 76;
    // 0x4D, confirmed
    pub const SCRIPT_SET_MOUSE_CURSOR_HIDE_ONOFF: i32 = 77;
    // 0x4E, confirmed
    pub const SCRIPT_GET_MOUSE_CURSOR_HIDE_ONOFF: i32 = 78;
    // 0x4F, confirmed
    pub const SCRIPT_SET_MOUSE_CURSOR_HIDE_TIME: i32 = 79;
    // 0x50, confirmed
    pub const SCRIPT_SET_MOUSE_CURSOR_HIDE_TIME_DEFAULT: i32 = 80;
    // 0x51, confirmed
    pub const SCRIPT_GET_MOUSE_CURSOR_HIDE_TIME: i32 = 81;
    // 0x52, confirmed
    pub const SCRIPT_SET_MOUSE_CURSOR_HIDE_ONOFF_DEFAULT: i32 = 82;
    // 0x53, confirmed
    pub const SCRIPT_GET_SKIP_DISABLE_FLAG: i32 = 83;
    // 0x54, confirmed
    pub const SCRIPT_GET_CTRL_SKIP_DISABLE_FLAG: i32 = 84;
    // 0x55, confirmed
    pub const SCRIPT_SET_SKIP_DISABLE_FLAG: i32 = 85;
    // 0x56, confirmed
    pub const SCRIPT_SET_CTRL_SKIP_DISABLE_FLAG: i32 = 86;
    // 0x57, confirmed
    pub const SCRIPT_SET_FONT_NAME: i32 = 87;
    // 0x58, confirmed
    pub const SCRIPT_SET_FONT_NAME_DEFAULT: i32 = 88;
    // 0x59, confirmed
    pub const SCRIPT_GET_FONT_NAME: i32 = 89;
    // 0x5A, confirmed
    pub const SCRIPT_SET_FONT_BOLD: i32 = 90;
    // 0x5B, confirmed
    pub const SCRIPT_SET_FONT_BOLD_DEFAULT: i32 = 91;
    // 0x5C, confirmed
    pub const SCRIPT_GET_FONT_BOLD: i32 = 92;
    // 0x5D, confirmed
    pub const SCRIPT_SET_FONT_SHADOW: i32 = 93;
    // 0x5E, confirmed
    pub const SCRIPT_SET_FONT_SHADOW_DEFAULT: i32 = 94;
    // 0x5F, confirmed
    pub const SCRIPT_GET_FONT_SHADOW: i32 = 95;
    // 0x60, confirmed
    pub const SCRIPT_SET_EMOTE_MOUTH_STOP_FLAG: i32 = 96;
    // 0x61, confirmed
    pub const SCRIPT_GET_EMOTE_MOUTH_STOP_FLAG: i32 = 97;

    // SE
    // 0x00, confirmed
    pub const SE_PLAY: i32 = 0;
    // 0x01, confirmed
    pub const SE_SET_VOLUME: i32 = 1;
    // 0x02, confirmed
    pub const SE_SET_VOLUME_MAX: i32 = 2;
    // 0x03, confirmed
    pub const SE_SET_VOLUME_MIN: i32 = 3;
    // 0x04, confirmed
    pub const SE_CHECK: i32 = 4;
    // 0x05, confirmed
    pub const SE_PLAY_BY_FILE_NAME: i32 = 5;
    // 0x06, confirmed
    pub const SE_PLAY_BY_KOE_NO: i32 = 6;
    // 0x07, confirmed
    pub const SE_STOP: i32 = 7;
    // 0x08, confirmed
    pub const SE_WAIT: i32 = 8;
    // 0x09, confirmed
    pub const SE_PLAY_BY_SE_NO: i32 = 9;
    // 0x0A, confirmed
    pub const SE_WAIT_KEY: i32 = 10;
    // 0x0B, confirmed
    pub const SE_GET_VOLUME: i32 = 11;

    // STAGE
    // 0x00, confirmed
    pub const STAGE_CREATE_OBJECT: i32 = 0;
    // 0x01, confirmed
    pub const STAGE_CREATE_MWND: i32 = 1;
    // 0x02, confirmed
    pub const STAGE_OBJECT: i32 = 2;
    // 0x03, confirmed
    pub const STAGE_MWND: i32 = 3;
    // 0x04, confirmed
    pub const STAGE_EFFECT: i32 = 4;
    // 0x05, confirmed
    pub const STAGE_BTNSELITEM: i32 = 5;
    // 0x06, confirmed
    pub const STAGE_OBJBTNGROUP: i32 = 6;
    // 0x07, confirmed
    pub const STAGE_QUAKE: i32 = 7;
    // 0x08, confirmed
    pub const STAGE_WORLD: i32 = 8;

    // STR
    // 0x0, confirmed
    pub const STR_UPPER: i32 = 0;
    // 0x1, confirmed
    pub const STR_LOWER: i32 = 1;
    // 0x2, confirmed
    pub const STR_LEFT: i32 = 2;
    // 0x3, confirmed
    pub const STR_MID: i32 = 3;
    // 0x4, confirmed
    pub const STR_RIGHT: i32 = 4;
    // 0x5, confirmed
    pub const STR_LEN: i32 = 5;
    // 0x6, confirmed
    pub const STR_CNT: i32 = 6;
    // 0x7, confirmed
    pub const STR_LEFT_LEN: i32 = 7;
    // 0x8, confirmed
    pub const STR_MID_LEN: i32 = 8;
    // 0x9, confirmed
    pub const STR_RIGHT_LEN: i32 = 9;
    // 0xa, confirmed
    pub const STR_SEARCH: i32 = 10;
    // 0xb, confirmed
    pub const STR_SEARCH_LAST: i32 = 11;
    // 0xc, confirmed
    pub const STR_TONUM: i32 = 12;
    // 0xd, confirmed
    pub const STR_GET_CODE: i32 = 13;

    // STRLIST
    // 0x2, confirmed
    pub const STRLIST_RESIZE: i32 = 2;
    // 0x3, confirmed
    pub const STRLIST_INIT: i32 = 3;
    // 0x4, confirmed
    pub const STRLIST_GET_SIZE: i32 = 4;

    // SYSCOM
    // 0x000, confirmed
    pub const SYSCOM_CALL_SYSCOM_MENU: i32 = 0;
    // 0x003, confirmed
    pub const SYSCOM_CALL_CONFIG_MENU: i32 = 3;
    // 0x004, confirmed
    pub const SYSCOM_SET_WINDOW_MODE: i32 = 4;
    // 0x005, confirmed
    pub const SYSCOM_INIT_SYSCOM_FLAG: i32 = 5;
    // 0x006, confirmed
    pub const SYSCOM_SET_SYSCOM_MENU_ENABLE: i32 = 6;
    // 0x007, confirmed
    pub const SYSCOM_SET_SYSCOM_MENU_DISABLE: i32 = 7;
    // 0x008, confirmed
    pub const SYSCOM_SET_NO_MWND_ANIME_ONOFF: i32 = 8;
    // 0x009, confirmed
    pub const SYSCOM_GET_WINDOW_MODE: i32 = 9;
    // 0x00A, confirmed
    pub const SYSCOM_GET_SAVELOAD_ALERT_ONOFF: i32 = 10;
    // 0x00B, confirmed
    pub const SYSCOM_SET_MWND_BTN_ENABLE: i32 = 11;
    // 0x00C, confirmed
    pub const SYSCOM_SET_MWND_BTN_DISABLE: i32 = 12;
    // 0x00D, confirmed
    pub const SYSCOM_SET_WINDOW_MODE_SIZE: i32 = 13;
    // 0x00E, confirmed
    pub const SYSCOM_SET_GLOBAL_EXTRA_SWITCH_ONOFF: i32 = 14;
    // 0x00F, confirmed
    pub const SYSCOM_SET_GLOBAL_EXTRA_SWITCH_ONOFF_DEFAULT: i32 = 15;
    // 0x010, confirmed
    pub const SYSCOM_GET_WINDOW_MODE_SIZE: i32 = 16;
    // 0x011, confirmed
    pub const SYSCOM_GET_GLOBAL_EXTRA_SWITCH_ONOFF: i32 = 17;
    // 0x012, confirmed
    pub const SYSCOM_QUICK_SAVE: i32 = 18;
    // 0x014, confirmed
    pub const SYSCOM_QUICK_LOAD: i32 = 20;
    // 0x015, confirmed
    pub const SYSCOM_SET_BGM_VOLUME: i32 = 21;
    // 0x017, confirmed
    pub const SYSCOM_SET_LOCAL_EXTRA_MODE_VALUE: i32 = 23;
    // 0x018, confirmed
    pub const SYSCOM_SET_BGM_VOLUME_DEFAULT: i32 = 24;
    // 0x019, confirmed
    pub const SYSCOM_GET_BGM_VOLUME: i32 = 25;
    // 0x01A, confirmed
    pub const SYSCOM_SET_KOE_VOLUME: i32 = 26;
    // 0x01B, confirmed
    pub const SYSCOM_SET_KOE_VOLUME_DEFAULT: i32 = 27;
    // 0x01C, confirmed
    pub const SYSCOM_GET_KOE_VOLUME: i32 = 28;
    // 0x01D, confirmed
    pub const SYSCOM_SET_PCM_VOLUME: i32 = 29;
    // 0x01E, confirmed
    pub const SYSCOM_SET_PCM_VOLUME_DEFAULT: i32 = 30;
    // 0x01F, confirmed
    pub const SYSCOM_GET_PCM_VOLUME: i32 = 31;
    // 0x020, confirmed
    pub const SYSCOM_SET_SE_VOLUME: i32 = 32;
    // 0x021, confirmed
    pub const SYSCOM_SET_SE_VOLUME_DEFAULT: i32 = 33;
    // 0x022, confirmed
    pub const SYSCOM_GET_SE_VOLUME: i32 = 34;
    // 0x023, confirmed
    pub const SYSCOM_SET_BGM_ONOFF: i32 = 35;
    // 0x024, confirmed
    pub const SYSCOM_SET_KOE_ONOFF: i32 = 36;
    // 0x025, confirmed
    pub const SYSCOM_SET_PCM_ONOFF: i32 = 37;
    // 0x026, confirmed
    pub const SYSCOM_SET_SE_ONOFF: i32 = 38;
    // 0x027, confirmed
    pub const SYSCOM_SET_ALL_VOLUME: i32 = 39;
    // 0x028, confirmed
    pub const SYSCOM_SET_ALL_VOLUME_DEFAULT: i32 = 40;
    // 0x029, confirmed
    pub const SYSCOM_GET_ALL_VOLUME: i32 = 41;
    // 0x02A, confirmed
    pub const SYSCOM_GET_BGM_ONOFF: i32 = 42;
    // 0x02B, confirmed
    pub const SYSCOM_GET_KOE_ONOFF: i32 = 43;
    // 0x02C, confirmed
    pub const SYSCOM_GET_PCM_ONOFF: i32 = 44;
    // 0x02D, confirmed
    pub const SYSCOM_GET_SE_ONOFF: i32 = 45;
    // 0x02E, confirmed
    pub const SYSCOM_SET_MESSAGE_SPEED: i32 = 46;
    // 0x02F, confirmed
    pub const SYSCOM_SET_MESSAGE_SPEED_DEFAULT: i32 = 47;
    // 0x030, confirmed
    pub const SYSCOM_GET_MESSAGE_SPEED: i32 = 48;
    // 0x031, confirmed
    pub const SYSCOM_SET_MESSAGE_NOWAIT: i32 = 49;
    // 0x032, confirmed
    pub const SYSCOM_GET_MESSAGE_NOWAIT: i32 = 50;
    // 0x033, confirmed
    pub const SYSCOM_SET_AUTO_MODE_MOJI_WAIT: i32 = 51;
    // 0x034, confirmed
    pub const SYSCOM_SET_AUTO_MODE_MOJI_WAIT_DEFAULT: i32 = 52;
    // 0x035, confirmed
    pub const SYSCOM_GET_AUTO_MODE_MOJI_WAIT: i32 = 53;
    // 0x036, confirmed
    pub const SYSCOM_SET_AUTO_MODE_MIN_WAIT: i32 = 54;
    // 0x037, confirmed
    pub const SYSCOM_SET_AUTO_MODE_MIN_WAIT_DEFAULT: i32 = 55;
    // 0x038, confirmed
    pub const SYSCOM_GET_AUTO_MODE_MIN_WAIT: i32 = 56;
    // 0x039, confirmed
    pub const SYSCOM_GET_LOCAL_EXTRA_MODE_VALUE: i32 = 57;
    // 0x03A, confirmed
    pub const SYSCOM_SET_LOCAL_EXTRA_MODE_ENABLE_FLAG: i32 = 58;
    // 0x03B, confirmed
    pub const SYSCOM_GET_LOCAL_EXTRA_MODE_ENABLE_FLAG: i32 = 59;
    // 0x03C, confirmed
    pub const SYSCOM_SET_ALL_ONOFF: i32 = 60;
    // 0x03D, confirmed
    pub const SYSCOM_GET_ALL_ONOFF: i32 = 61;
    // 0x03E, confirmed
    pub const SYSCOM_SET_LOCAL_EXTRA_MODE_EXIST_FLAG: i32 = 62;
    // 0x03F, confirmed
    pub const SYSCOM_GET_LOCAL_EXTRA_MODE_EXIST_FLAG: i32 = 63;
    // 0x040, confirmed
    pub const SYSCOM_CHECK_LOCAL_EXTRA_MODE_ENABLE: i32 = 64;
    // 0x042, confirmed
    pub const SYSCOM_CHANGE_QUICK_SAVE: i32 = 66;
    // 0x043, confirmed
    pub const SYSCOM_SAVE: i32 = 67;
    // 0x044, confirmed
    pub const SYSCOM_GET_SAVE_CNT: i32 = 68;
    // 0x045, confirmed
    pub const SYSCOM_GET_SAVE_EXIST: i32 = 69;
    // 0x046, confirmed
    pub const SYSCOM_GET_SAVE_YEAR: i32 = 70;
    // 0x047, confirmed
    pub const SYSCOM_GET_SAVE_MONTH: i32 = 71;
    // 0x048, confirmed
    pub const SYSCOM_GET_SAVE_DAY: i32 = 72;
    // 0x049, confirmed
    pub const SYSCOM_GET_SAVE_WEEKDAY: i32 = 73;
    // 0x04A, confirmed
    pub const SYSCOM_GET_SAVE_HOUR: i32 = 74;
    // 0x04B, confirmed
    pub const SYSCOM_GET_SAVE_MINUTE: i32 = 75;
    // 0x04C, confirmed
    pub const SYSCOM_GET_SAVE_SECOND: i32 = 76;
    // 0x04D, confirmed
    pub const SYSCOM_GET_SAVE_MILLISECOND: i32 = 77;
    // 0x04E, confirmed
    pub const SYSCOM_GET_SAVE_TITLE: i32 = 78;
    // 0x04F, confirmed
    pub const SYSCOM_GET_SAVE_NEW_NO: i32 = 79;
    // 0x050, confirmed
    pub const SYSCOM_SET_SAVELOAD_ALERT_ONOFF: i32 = 80;
    // 0x051, confirmed
    pub const SYSCOM_GET_NO_MWND_ANIME_ONOFF: i32 = 81;
    // 0x052, confirmed
    pub const SYSCOM_SET_FILTER_COLOR_R: i32 = 82;
    // 0x053, confirmed
    pub const SYSCOM_SET_FILTER_COLOR_R_DEFAULT: i32 = 83;
    // 0x054, confirmed
    pub const SYSCOM_GET_FILTER_COLOR_R: i32 = 84;
    // 0x055, confirmed
    pub const SYSCOM_SET_FILTER_COLOR_G: i32 = 85;
    // 0x056, confirmed
    pub const SYSCOM_SET_FILTER_COLOR_B: i32 = 86;
    // 0x057, confirmed
    pub const SYSCOM_SET_FILTER_COLOR_A: i32 = 87;
    // 0x058, confirmed
    pub const SYSCOM_SET_FILTER_COLOR_G_DEFAULT: i32 = 88;
    // 0x059, confirmed
    pub const SYSCOM_SET_FILTER_COLOR_B_DEFAULT: i32 = 89;
    // 0x05A, confirmed
    pub const SYSCOM_SET_FILTER_COLOR_A_DEFAULT: i32 = 90;
    // 0x05B, confirmed
    pub const SYSCOM_GET_FILTER_COLOR_G: i32 = 91;
    // 0x05C, confirmed
    pub const SYSCOM_GET_FILTER_COLOR_B: i32 = 92;
    // 0x05D, confirmed
    pub const SYSCOM_GET_FILTER_COLOR_A: i32 = 93;
    // 0x05E, confirmed
    pub const SYSCOM_SET_BGMFADE_VOLUME: i32 = 94;
    // 0x05F, confirmed
    pub const SYSCOM_SET_BGMFADE_VOLUME_DEFAULT: i32 = 95;
    // 0x060, confirmed
    pub const SYSCOM_GET_BGMFADE_VOLUME: i32 = 96;
    // 0x061, confirmed
    pub const SYSCOM_SET_BGMFADE_ONOFF: i32 = 97;
    // 0x062, confirmed
    pub const SYSCOM_GET_BGMFADE_ONOFF: i32 = 98;
    // 0x063, confirmed
    pub const SYSCOM_SET_WINDOW_MODE_DEFAULT: i32 = 99;
    // 0x064, confirmed
    pub const SYSCOM_SET_WINDOW_MODE_SIZE_DEFAULT: i32 = 100;
    // 0x065, confirmed
    pub const SYSCOM_SET_ALL_ONOFF_DEFAULT: i32 = 101;
    // 0x066, confirmed
    pub const SYSCOM_SET_BGM_ONOFF_DEFAULT: i32 = 102;
    // 0x067, confirmed
    pub const SYSCOM_SET_KOE_ONOFF_DEFAULT: i32 = 103;
    // 0x068, confirmed
    pub const SYSCOM_SET_PCM_ONOFF_DEFAULT: i32 = 104;
    // 0x069, confirmed
    pub const SYSCOM_SET_SE_ONOFF_DEFAULT: i32 = 105;
    // 0x06A, confirmed
    pub const SYSCOM_SET_BGMFADE_ONOFF_DEFAULT: i32 = 106;
    // 0x06B, confirmed
    pub const SYSCOM_SET_MESSAGE_NOWAIT_DEFAULT: i32 = 107;
    // 0x06C, confirmed
    pub const SYSCOM_SET_SAVELOAD_ALERT_ONOFF_DEFAULT: i32 = 108;
    // 0x06D, confirmed
    pub const SYSCOM_SET_NO_MWND_ANIME_ONOFF_DEFAULT: i32 = 109;
    // 0x06E, confirmed
    pub const SYSCOM_SET_SLEEP_ONOFF: i32 = 110;
    // 0x06F, confirmed
    pub const SYSCOM_SET_SLEEP_ONOFF_DEFAULT: i32 = 111;
    // 0x070, confirmed
    pub const SYSCOM_GET_SLEEP_ONOFF: i32 = 112;
    // 0x071, confirmed
    pub const SYSCOM_SET_NO_WIPE_ANIME_ONOFF: i32 = 113;
    // 0x072, confirmed
    pub const SYSCOM_SET_NO_WIPE_ANIME_ONOFF_DEFAULT: i32 = 114;
    // 0x073, confirmed
    pub const SYSCOM_GET_NO_WIPE_ANIME_ONOFF: i32 = 115;
    // 0x074, confirmed
    pub const SYSCOM_SET_SKIP_WIPE_ANIME_ONOFF: i32 = 116;
    // 0x075, confirmed
    pub const SYSCOM_SET_SKIP_WIPE_ANIME_ONOFF_DEFAULT: i32 = 117;
    // 0x076, confirmed
    pub const SYSCOM_GET_SKIP_WIPE_ANIME_ONOFF: i32 = 118;
    // 0x077, confirmed
    pub const SYSCOM_SET_WHEEL_NEXT_MESSAGE_ONOFF: i32 = 119;
    // 0x078, confirmed
    pub const SYSCOM_SET_WHEEL_NEXT_MESSAGE_ONOFF_DEFAULT: i32 = 120;
    // 0x079, confirmed
    pub const SYSCOM_GET_WHEEL_NEXT_MESSAGE_ONOFF: i32 = 121;
    // 0x07A, confirmed
    pub const SYSCOM_SET_KOE_DONT_STOP_ONOFF: i32 = 122;
    // 0x07B, confirmed
    pub const SYSCOM_SET_KOE_DONT_STOP_ONOFF_DEFAULT: i32 = 123;
    // 0x07C, confirmed
    pub const SYSCOM_GET_KOE_DONT_STOP_ONOFF: i32 = 124;
    // 0x07D, confirmed
    pub const SYSCOM_SET_SKIP_UNREAD_MESSAGE_ONOFF: i32 = 125;
    // 0x07E, confirmed
    pub const SYSCOM_SET_SKIP_UNREAD_MESSAGE_ONOFF_DEFAULT: i32 = 126;
    // 0x07F, confirmed
    pub const SYSCOM_GET_SKIP_UNREAD_MESSAGE_ONOFF: i32 = 127;
    // 0x081, confirmed
    pub const SYSCOM_GET_SAVE_MESSAGE: i32 = 129;
    // 0x082, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_MESSAGE: i32 = 130;
    // 0x083, confirmed
    pub const SYSCOM_GET_SAVE_COMMENT: i32 = 131;
    // 0x084, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_COMMENT: i32 = 132;
    // 0x085, confirmed
    pub const SYSCOM_SET_MWND_BTN_TOUCH_ENABLE: i32 = 133;
    // 0x086, confirmed
    pub const SYSCOM_SET_MWND_BTN_TOUCH_DISABLE: i32 = 134;
    // 0x087, confirmed
    pub const SYSCOM_CALL_CONFIG_MESSAGE_SPEED_MENU: i32 = 135;
    // 0x088, confirmed
    pub const SYSCOM_CALL_CONFIG_FILTER_COLOR_MENU: i32 = 136;
    // 0x089, confirmed
    pub const SYSCOM_CALL_CONFIG_BGMFADE_MENU: i32 = 137;
    // 0x08A, confirmed
    pub const SYSCOM_CALL_CONFIG_WINDOW_MODE_MENU: i32 = 138;
    // 0x08B, confirmed
    pub const SYSCOM_CALL_CONFIG_VOLUME_MENU: i32 = 139;
    // 0x08C, confirmed
    pub const SYSCOM_CALL_CONFIG_AUTO_MODE_MENU: i32 = 140;
    // 0x08D, confirmed
    pub const SYSCOM_CALL_CONFIG_SYSTEM_MENU: i32 = 141;
    // 0x08E, confirmed
    pub const SYSCOM_CALL_CONFIG_FONT_MENU: i32 = 142;
    // 0x08F, confirmed
    pub const SYSCOM_SET_CHARAKOE_ONOFF: i32 = 143;
    // 0x090, confirmed
    pub const SYSCOM_SET_CHARAKOE_ONOFF_DEFAULT: i32 = 144;
    // 0x091, confirmed
    pub const SYSCOM_GET_CHARAKOE_ONOFF: i32 = 145;
    // 0x092, confirmed
    pub const SYSCOM_CALL_CONFIG_CHARAKOE_MENU: i32 = 146;
    // 0x093, confirmed
    pub const SYSCOM_CALL_CONFIG_KOEMODE_MENU: i32 = 147;
    // 0x094, confirmed
    pub const SYSCOM_SET_KOEMODE: i32 = 148;
    // 0x095, confirmed
    pub const SYSCOM_SET_KOEMODE_DEFAULT: i32 = 149;
    // 0x096, confirmed
    pub const SYSCOM_GET_KOEMODE: i32 = 150;
    // 0x097, confirmed
    pub const SYSCOM_CALL_CONFIG_JITAN_MENU: i32 = 151;
    // 0x098, confirmed
    pub const SYSCOM_SET_JITAN_SPEED: i32 = 152;
    // 0x099, confirmed
    pub const SYSCOM_SET_JITAN_NORMAL_ONOFF: i32 = 153;
    // 0x09A, confirmed
    pub const SYSCOM_SET_JITAN_NORMAL_ONOFF_DEFAULT: i32 = 154;
    // 0x09B, confirmed
    pub const SYSCOM_GET_JITAN_NORMAL_ONOFF: i32 = 155;
    // 0x09C, confirmed
    pub const SYSCOM_SET_JITAN_AUTO_MODE_ONOFF: i32 = 156;
    // 0x09D, confirmed
    pub const SYSCOM_SET_JITAN_AUTO_MODE_ONOFF_DEFAULT: i32 = 157;
    // 0x09E, confirmed
    pub const SYSCOM_GET_JITAN_AUTO_MODE_ONOFF: i32 = 158;
    // 0x09F, confirmed
    pub const SYSCOM_SET_JITAN_KOE_REPLAY_ONOFF: i32 = 159;
    // 0x0A0, confirmed
    pub const SYSCOM_SET_JITAN_KOE_REPLAY_ONOFF_DEFAULT: i32 = 160;
    // 0x0A1, confirmed
    pub const SYSCOM_GET_JITAN_KOE_REPLAY_ONOFF: i32 = 161;
    // 0x0A2, confirmed
    pub const SYSCOM_SET_JITAN_SPEED_DEFAULT: i32 = 162;
    // 0x0A3, confirmed
    pub const SYSCOM_GET_JITAN_SPEED: i32 = 163;
    // 0x0A4, confirmed
    pub const SYSCOM_SET_GLOBAL_EXTRA_MODE_VALUE: i32 = 164;
    // 0x0A5, confirmed
    pub const SYSCOM_SET_GLOBAL_EXTRA_MODE_VALUE_DEFAULT: i32 = 165;
    // 0x0A6, confirmed
    pub const SYSCOM_GET_GLOBAL_EXTRA_MODE_VALUE: i32 = 166;
    // 0x0A7, confirmed
    pub const SYSCOM_CALL_CONFIG_MOVIE_MENU: i32 = 167;
    // 0x0A8, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_CNT: i32 = 168;
    // 0x0A9, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_EXIST: i32 = 169;
    // 0x0AA, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_NEW_NO: i32 = 170;
    // 0x0AB, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_YEAR: i32 = 171;
    // 0x0AC, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_MONTH: i32 = 172;
    // 0x0AD, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_DAY: i32 = 173;
    // 0x0AE, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_WEEKDAY: i32 = 174;
    // 0x0AF, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_HOUR: i32 = 175;
    // 0x0B0, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_MINUTE: i32 = 176;
    // 0x0B1, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_SECOND: i32 = 177;
    // 0x0B2, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_MILLISECOND: i32 = 178;
    // 0x0B3, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_TITLE: i32 = 179;
    // 0x0B4, confirmed
    pub const SYSCOM_SET_SAVE_COMMENT: i32 = 180;
    // 0x0B5, confirmed
    pub const SYSCOM_SET_QUICK_SAVE_COMMENT: i32 = 181;
    // 0x0B6, confirmed
    pub const SYSCOM_SET_SAVE_VALUE: i32 = 182;
    // 0x0B7, confirmed
    pub const SYSCOM_GET_SAVE_VALUE: i32 = 183;
    // 0x0B8, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_VALUE: i32 = 184;
    // 0x0B9, confirmed
    pub const SYSCOM_SET_QUICK_SAVE_VALUE: i32 = 185;
    // 0x0BA, confirmed
    pub const SYSCOM_SET_CHARAKOE_VOLUME: i32 = 186;
    // 0x0BB, confirmed
    pub const SYSCOM_SET_CHARAKOE_VOLUME_DEFAULT: i32 = 187;
    // 0x0BC, confirmed
    pub const SYSCOM_GET_CHARAKOE_VOLUME: i32 = 188;
    // 0x0BD, confirmed
    pub const SYSCOM_SET_OBJECT_DISP_ONOFF: i32 = 189;
    // 0x0BE, confirmed
    pub const SYSCOM_SET_OBJECT_DISP_ONOFF_DEFAULT: i32 = 190;
    // 0x0BF, confirmed
    pub const SYSCOM_GET_OBJECT_DISP_ONOFF: i32 = 191;
    // 0x0C0, confirmed
    pub const SYSCOM_OPEN_MSG_BACK: i32 = 192;
    // 0x0C1, confirmed
    pub const SYSCOM_CLOSE_MSG_BACK: i32 = 193;
    // 0x0C2, confirmed
    pub const SYSCOM_SET_MSG_BACK_ENABLE_FLAG: i32 = 194;
    // 0x0C3, confirmed
    pub const SYSCOM_GET_MSG_BACK_ENABLE_FLAG: i32 = 195;
    // 0x0C4, confirmed
    pub const SYSCOM_SET_MSG_BACK_EXIST_FLAG: i32 = 196;
    // 0x0C5, confirmed
    pub const SYSCOM_GET_MSG_BACK_EXIST_FLAG: i32 = 197;
    // 0x0C6, confirmed
    pub const SYSCOM_CHECK_MSG_BACK_ENABLE: i32 = 198;
    // 0x0C7, confirmed
    pub const SYSCOM_GET_TOTAL_PLAY_TIME: i32 = 199;
    // 0x0C8, confirmed
    pub const SYSCOM_SET_READ_SKIP_ONOFF_FLAG: i32 = 200;
    // 0x0C9, confirmed
    pub const SYSCOM_GET_READ_SKIP_ONOFF_FLAG: i32 = 201;
    // 0x0CA, confirmed
    pub const SYSCOM_SET_READ_SKIP_ENABLE_FLAG: i32 = 202;
    // 0x0CB, confirmed
    pub const SYSCOM_GET_READ_SKIP_ENABLE_FLAG: i32 = 203;
    // 0x0CC, confirmed
    pub const SYSCOM_SET_READ_SKIP_EXIST_FLAG: i32 = 204;
    // 0x0CD, confirmed
    pub const SYSCOM_GET_READ_SKIP_EXIST_FLAG: i32 = 205;
    // 0x0CE, confirmed
    pub const SYSCOM_CHECK_READ_SKIP_ENABLE: i32 = 206;
    // 0x0CF, confirmed
    pub const SYSCOM_SET_AUTO_SKIP_ONOFF_FLAG: i32 = 207;
    // 0x0D0, confirmed
    pub const SYSCOM_GET_AUTO_SKIP_ONOFF_FLAG: i32 = 208;
    // 0x0D1, confirmed
    pub const SYSCOM_SET_AUTO_SKIP_ENABLE_FLAG: i32 = 209;
    // 0x0D2, confirmed
    pub const SYSCOM_GET_AUTO_SKIP_ENABLE_FLAG: i32 = 210;
    // 0x0D3, confirmed
    pub const SYSCOM_SET_AUTO_SKIP_EXIST_FLAG: i32 = 211;
    // 0x0D4, confirmed
    pub const SYSCOM_GET_AUTO_SKIP_EXIST_FLAG: i32 = 212;
    // 0x0D5, confirmed
    pub const SYSCOM_CHECK_AUTO_SKIP_ENABLE: i32 = 213;
    // 0x0D6, confirmed
    pub const SYSCOM_SET_AUTO_MODE_ONOFF_FLAG: i32 = 214;
    // 0x0D7, confirmed
    pub const SYSCOM_GET_AUTO_MODE_ONOFF_FLAG: i32 = 215;
    // 0x0D8, confirmed
    pub const SYSCOM_SET_AUTO_MODE_ENABLE_FLAG: i32 = 216;
    // 0x0D9, confirmed
    pub const SYSCOM_GET_AUTO_MODE_ENABLE_FLAG: i32 = 217;
    // 0x0DA, confirmed
    pub const SYSCOM_SET_AUTO_MODE_EXIST_FLAG: i32 = 218;
    // 0x0DB, confirmed
    pub const SYSCOM_GET_AUTO_MODE_EXIST_FLAG: i32 = 219;
    // 0x0DC, confirmed
    pub const SYSCOM_CHECK_AUTO_MODE_ENABLE: i32 = 220;
    // 0x0DD, confirmed
    pub const SYSCOM_SET_HIDE_MWND_ONOFF_FLAG: i32 = 221;
    // 0x0DE, confirmed
    pub const SYSCOM_GET_HIDE_MWND_ONOFF_FLAG: i32 = 222;
    // 0x0DF, confirmed
    pub const SYSCOM_SET_HIDE_MWND_ENABLE_FLAG: i32 = 223;
    // 0x0E0, confirmed
    pub const SYSCOM_GET_HIDE_MWND_ENABLE_FLAG: i32 = 224;
    // 0x0E1, confirmed
    pub const SYSCOM_SET_HIDE_MWND_EXIST_FLAG: i32 = 225;
    // 0x0E2, confirmed
    pub const SYSCOM_GET_HIDE_MWND_EXIST_FLAG: i32 = 226;
    // 0x0E3, confirmed
    pub const SYSCOM_CHECK_HIDE_MWND_ENABLE: i32 = 227;
    // 0x0E5, confirmed
    pub const SYSCOM_SET_TOTAL_PLAY_TIME: i32 = 229;
    // 0x0E6, confirmed
    pub const SYSCOM_SET_RETURN_TO_SEL_ENABLE_FLAG: i32 = 230;
    // 0x0E7, confirmed
    pub const SYSCOM_GET_RETURN_TO_SEL_ENABLE_FLAG: i32 = 231;
    // 0x0E8, confirmed
    pub const SYSCOM_SET_RETURN_TO_SEL_EXIST_FLAG: i32 = 232;
    // 0x0E9, confirmed
    pub const SYSCOM_GET_RETURN_TO_SEL_EXIST_FLAG: i32 = 233;
    // 0x0EA, confirmed
    pub const SYSCOM_CHECK_RETURN_TO_SEL_ENABLE: i32 = 234;
    // 0x0EB, confirmed
    pub const SYSCOM_RETURN_TO_SEL: i32 = 235;
    // 0x0EC, confirmed
    pub const SYSCOM_CALL_EX: i32 = 236;
    // 0x0ED, confirmed
    pub const SYSCOM_SET_RETURN_TO_MENU_ENABLE_FLAG: i32 = 237;
    // 0x0EE, confirmed
    pub const SYSCOM_GET_RETURN_TO_MENU_ENABLE_FLAG: i32 = 238;
    // 0x0EF, confirmed
    pub const SYSCOM_SET_RETURN_TO_MENU_EXIST_FLAG: i32 = 239;
    // 0x0F0, confirmed
    pub const SYSCOM_GET_RETURN_TO_MENU_EXIST_FLAG: i32 = 240;
    // 0x0F1, confirmed
    pub const SYSCOM_CHECK_RETURN_TO_MENU_ENABLE: i32 = 241;
    // 0x0F2, confirmed
    pub const SYSCOM_END_GAME: i32 = 242;
    // 0x0F3, confirmed
    pub const SYSCOM_GET_PLAY_SILENT_SOUND_ONOFF: i32 = 243;
    // 0x0F4, confirmed
    pub const SYSCOM_SET_END_GAME_ENABLE_FLAG: i32 = 244;
    // 0x0F5, confirmed
    pub const SYSCOM_GET_END_GAME_ENABLE_FLAG: i32 = 245;
    // 0x0F6, confirmed
    pub const SYSCOM_SET_END_GAME_EXIST_FLAG: i32 = 246;
    // 0x0F7, confirmed
    pub const SYSCOM_GET_END_GAME_EXIST_FLAG: i32 = 247;
    // 0x0F8, confirmed
    pub const SYSCOM_CHECK_END_GAME_ENABLE: i32 = 248;
    // 0x0FA, confirmed
    pub const SYSCOM_SET_PLAY_SILENT_SOUND_ONOFF: i32 = 250;
    // 0x0FB, confirmed
    pub const SYSCOM_SET_SAVE_ENABLE_FLAG: i32 = 251;
    // 0x0FC, confirmed
    pub const SYSCOM_GET_SAVE_ENABLE_FLAG: i32 = 252;
    // 0x0FD, confirmed
    pub const SYSCOM_SET_SAVE_EXIST_FLAG: i32 = 253;
    // 0x0FE, confirmed
    pub const SYSCOM_GET_SAVE_EXIST_FLAG: i32 = 254;
    // 0x0FF, confirmed
    pub const SYSCOM_CHECK_SAVE_ENABLE: i32 = 255;
    // 0x100, confirmed
    pub const SYSCOM_CALL_SAVE_MENU: i32 = 256;
    // 0x101, confirmed
    pub const SYSCOM_SET_PLAY_SILENT_SOUND_ONOFF_DEFAULT: i32 = 257;
    // 0x102, confirmed
    pub const SYSCOM_SET_LOAD_ENABLE_FLAG: i32 = 258;
    // 0x103, confirmed
    pub const SYSCOM_GET_LOAD_ENABLE_FLAG: i32 = 259;
    // 0x104, confirmed
    pub const SYSCOM_SET_LOAD_EXIST_FLAG: i32 = 260;
    // 0x105, confirmed
    pub const SYSCOM_GET_LOAD_EXIST_FLAG: i32 = 261;
    // 0x106, confirmed
    pub const SYSCOM_CHECK_LOAD_ENABLE: i32 = 262;
    // 0x107, confirmed
    pub const SYSCOM_SET_MOV_VOLUME: i32 = 263;
    // 0x108, confirmed
    pub const SYSCOM_SET_MOV_VOLUME_DEFAULT: i32 = 264;
    // 0x109, confirmed
    pub const SYSCOM_GET_MOV_VOLUME: i32 = 265;
    // 0x10A, confirmed
    pub const SYSCOM_SET_MOV_ONOFF: i32 = 266;
    // 0x10B, confirmed
    pub const SYSCOM_SET_MOV_ONOFF_DEFAULT: i32 = 267;
    // 0x10C, confirmed
    pub const SYSCOM_GET_MOV_ONOFF: i32 = 268;
    // 0x10D, confirmed
    pub const SYSCOM_CALL_LOAD_MENU: i32 = 269;
    // 0x10E, confirmed
    pub const SYSCOM_GET_END_SAVE_EXIST: i32 = 270;
    // 0x10F, confirmed
    pub const SYSCOM_END_SAVE: i32 = 271;
    // 0x110, confirmed
    pub const SYSCOM_INNER_SAVE: i32 = 272;
    // 0x111, confirmed
    pub const SYSCOM_INNER_LOAD: i32 = 273;
    // 0x112, confirmed
    pub const SYSCOM_COPY_INNER_SAVE: i32 = 274;
    // 0x113, confirmed
    pub const SYSCOM_CHECK_INNER_SAVE: i32 = 275;
    // 0x114, confirmed
    pub const SYSCOM_CLEAR_INNER_SAVE: i32 = 276;
    // 0x115, confirmed
    pub const SYSCOM_SET_SOUND_VOLUME: i32 = 277;
    // 0x116, confirmed
    pub const SYSCOM_SET_SOUND_VOLUME_DEFAULT: i32 = 278;
    // 0x117, confirmed
    pub const SYSCOM_GET_SOUND_VOLUME: i32 = 279;
    // 0x118, confirmed
    pub const SYSCOM_SET_SOUND_ONOFF: i32 = 280;
    // 0x119, confirmed
    pub const SYSCOM_SET_SOUND_ONOFF_DEFAULT: i32 = 281;
    // 0x11A, confirmed
    pub const SYSCOM_GET_SOUND_ONOFF: i32 = 282;
    // 0x11B, confirmed
    pub const SYSCOM_SET_FONT_NAME: i32 = 283;
    // 0x11C, confirmed
    pub const SYSCOM_GET_FONT_NAME: i32 = 284;
    // 0x11D, confirmed
    pub const SYSCOM_IS_FONT_EXIST: i32 = 285;
    // 0x11E, confirmed
    pub const SYSCOM_CREATE_CAPTURE_BUFFER: i32 = 286;
    // 0x11F, confirmed
    pub const SYSCOM_DESTROY_CAPTURE_BUFFER: i32 = 287;
    // 0x120, confirmed
    pub const SYSCOM_REPLAY_KOE: i32 = 288;
    // 0x121, confirmed
    pub const SYSCOM_GET_REPLAY_KOE_KOE_NO: i32 = 289;
    // 0x122, confirmed
    pub const SYSCOM_CAPTURE_AND_SAVE_BUFFER_TO_PNG: i32 = 290;
    // 0x123, confirmed
    pub const SYSCOM_GET_REPLAY_KOE_CHARA_NO: i32 = 291;
    // 0x124, confirmed
    pub const SYSCOM_CHECK_REPLAY_KOE: i32 = 292;
    // 0x125, confirmed
    pub const SYSCOM_CLEAR_REPLAY_KOE: i32 = 293;
    // 0x126, confirmed
    pub const SYSCOM_GET_CURRENT_SAVE_SCENE_TITLE: i32 = 294;
    // 0x127, confirmed
    pub const SYSCOM_GET_CURRENT_SAVE_MESSAGE: i32 = 295;
    // 0x12C, confirmed
    pub const SYSCOM_SET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG: i32 = 300;
    // 0x12D, confirmed
    pub const SYSCOM_GET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG: i32 = 301;
    // 0x12E, confirmed
    pub const SYSCOM_SET_LOCAL_EXTRA_SWITCH_ENABLE_FLAG: i32 = 302;
    // 0x12F, confirmed
    pub const SYSCOM_GET_LOCAL_EXTRA_SWITCH_ENABLE_FLAG: i32 = 303;
    // 0x130, confirmed
    pub const SYSCOM_SET_LOCAL_EXTRA_SWITCH_EXIST_FLAG: i32 = 304;
    // 0x131, confirmed
    pub const SYSCOM_GET_LOCAL_EXTRA_SWITCH_EXIST_FLAG: i32 = 305;
    // 0x132, confirmed
    pub const SYSCOM_CHECK_LOCAL_EXTRA_SWITCH_ENABLE: i32 = 306;
    // 0x135, confirmed
    pub const SYSCOM_CHECK_WINDOW_MODE_SIZE_ENABLE: i32 = 309;
    // 0x137, confirmed
    pub const SYSCOM_SET_FONT_BOLD: i32 = 311;
    // 0x137, confirmed
    pub const SYSCOM_SET_MOUSE_CURSOR_HIDE_ONOFF: i32 = 311;
    // 0x138, confirmed
    pub const SYSCOM_SET_FONT_BOLD_DEFAULT: i32 = 312;
    // 0x138, confirmed
    pub const SYSCOM_SET_MOUSE_CURSOR_HIDE_ONOFF_DEFAULT: i32 = 312;
    // 0x139, confirmed
    pub const SYSCOM_GET_FONT_BOLD: i32 = 313;
    // 0x139, confirmed
    pub const SYSCOM_GET_MOUSE_CURSOR_HIDE_ONOFF: i32 = 313;
    // 0x13A, confirmed
    pub const SYSCOM_SAVE_CAPTURE_BUFFER_TO_FILE: i32 = 314;
    // 0x13B, confirmed
    pub const SYSCOM_LOAD_FLAG_FROM_CAPTURE_FILE: i32 = 315;
    // 0x13C, confirmed
    pub const SYSCOM_CAPTURE_TO_CAPTURE_BUFFER: i32 = 316;
    // 0x13D, confirmed
    pub const SYSCOM_SET_FONT_DECORATION: i32 = 317;
    // 0x13D, confirmed
    pub const SYSCOM_SET_MOUSE_CURSOR_HIDE_TIME: i32 = 317;
    // 0x13E, confirmed
    pub const SYSCOM_SET_FONT_DECORATION_DEFAULT: i32 = 318;
    // 0x13E, confirmed
    pub const SYSCOM_SET_MOUSE_CURSOR_HIDE_TIME_DEFAULT: i32 = 318;
    // 0x13F, confirmed
    pub const SYSCOM_GET_FONT_DECORATION: i32 = 319;
    // 0x13F, confirmed
    pub const SYSCOM_GET_MOUSE_CURSOR_HIDE_TIME: i32 = 319;
    // 0x140, confirmed
    pub const SYSCOM_GET_SAVE_APPEND_DIR: i32 = 320;
    // 0x141, confirmed
    pub const SYSCOM_GET_SAVE_APPEND_NAME: i32 = 321;
    // 0x142, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_APPEND_DIR: i32 = 322;
    // 0x143, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_APPEND_NAME: i32 = 323;
    // 0x144, confirmed
    pub const SYSCOM_GET_SAVE_FULL_MESSAGE: i32 = 324;
    // 0x145, confirmed
    pub const SYSCOM_GET_QUICK_SAVE_FULL_MESSAGE: i32 = 325;
    // 0x146, confirmed
    pub const SYSCOM_SET_FONT_NAME_DEFAULT: i32 = 326;
    // 0x147, confirmed
    pub const SYSCOM_OPEN_TWEET_DIALOG: i32 = 327;
    // 0x148, confirmed
    pub const SYSCOM_SET_RETURN_SCENE_ONCE: i32 = 328;
    // 0x149, confirmed
    pub const SYSCOM_CHECK_MSG_BACK_OPEN: i32 = 329;
    // 0x14A, confirmed
    pub const SYSCOM_GET_SYSTEM_EXTRA_INT_VALUE: i32 = 330;
    // 0x14B, confirmed
    pub const SYSCOM_GET_SYSTEM_EXTRA_STR_VALUE: i32 = 331;

    // SYSTEM
    // 0x0, confirmed
    pub const SYSTEM_CHECK_ACTIVE: i32 = 0;
    // 0x1, confirmed
    pub const SYSTEM_SHELL_OPEN_FILE: i32 = 1;
    // 0x2, confirmed
    pub const SYSTEM_CHECK_DUMMY_FILE_ONCE: i32 = 2;
    // 0x3, confirmed
    pub const SYSTEM_OPEN_DIALOG_FOR_CHIHAYA_BENCH: i32 = 3;
    // 0x4, confirmed
    pub const SYSTEM_GET_SPEC_INFO_FOR_CHIHAYA_BENCH: i32 = 4;
    // 0x5, confirmed
    pub const SYSTEM_SHELL_OPEN_WEB: i32 = 5;
    // 0x6, confirmed
    pub const SYSTEM_CHECK_FILE_EXIST: i32 = 6;
    // 0x7, confirmed
    pub const SYSTEM_DEBUG_MESSAGEBOX_OK: i32 = 7;
    // 0x8, confirmed
    pub const SYSTEM_DEBUG_MESSAGEBOX_OKCANCEL: i32 = 8;
    // 0x9, confirmed
    pub const SYSTEM_DEBUG_MESSAGEBOX_YESNO: i32 = 9;
    // 0xA, confirmed
    pub const SYSTEM_DEBUG_MESSAGEBOX_YESNOCANCEL: i32 = 10;
    // 0xB, confirmed
    pub const SYSTEM_DEBUG_WRITE_LOG: i32 = 11;
    // 0xC, confirmed
    pub const SYSTEM_CHECK_FILE_EXIST_SAVE_DIR: i32 = 12;
    // 0xD, confirmed
    pub const SYSTEM_CHECK_DEBUG_FLAG: i32 = 13;
    // 0xE, confirmed
    pub const SYSTEM_GET_CALENDAR: i32 = 14;
    // 0xF, confirmed
    pub const SYSTEM_GET_UNIX_TIME: i32 = 15;
    // 0x10, confirmed
    pub const SYSTEM_GET_LANGUAGE: i32 = 16;
    // 0x11, confirmed
    pub const SYSTEM_MESSAGEBOX_OK: i32 = 17;
    // 0x12, confirmed
    pub const SYSTEM_MESSAGEBOX_OKCANCEL: i32 = 18;
    // 0x13, confirmed
    pub const SYSTEM_MESSAGEBOX_YESNO: i32 = 19;
    // 0x14, confirmed
    pub const SYSTEM_MESSAGEBOX_YESNOCANCEL: i32 = 20;
    // 0x15, confirmed
    pub const SYSTEM_CLEAR_DUMMY_FILE: i32 = 21;

    // WORLD
    // 0x00, confirmed
    pub const WORLD_CAMERA_EYE_X: i32 = 0;
    // 0x01, confirmed
    pub const WORLD_CAMERA_EYE_Y: i32 = 1;
    // 0x02, confirmed
    pub const WORLD_CAMERA_EYE_Z: i32 = 2;
    // 0x03, confirmed
    pub const WORLD_CAMERA_PINT_X: i32 = 3;
    // 0x04, confirmed
    pub const WORLD_CAMERA_PINT_Y: i32 = 4;
    // 0x05, confirmed
    pub const WORLD_CAMERA_PINT_Z: i32 = 5;
    // 0x06, confirmed
    pub const WORLD_CAMERA_UP_X: i32 = 6;
    // 0x07, confirmed
    pub const WORLD_CAMERA_UP_Y: i32 = 7;
    // 0x08, confirmed
    pub const WORLD_CAMERA_UP_Z: i32 = 8;
    // 0x09, confirmed
    pub const WORLD_CALC_CAMERA_EYE: i32 = 9;
    // 0x0A, confirmed
    pub const WORLD_SET_CAMERA_EYE: i32 = 10;
    // 0x0B, confirmed
    pub const WORLD_SET_CAMERA_PINT: i32 = 11;
    // 0x0C, confirmed
    pub const WORLD_SET_CAMERA_UP: i32 = 12;
    // 0x0D, confirmed
    pub const WORLD_CAMERA_VIEW_ANGLE: i32 = 13;
    // 0x0E, confirmed
    pub const WORLD_GET_NO: i32 = 14;
    // 0x0F, confirmed
    pub const WORLD_INIT: i32 = 15;
    // 0x10, confirmed
    pub const WORLD_CALC_CAMERA_PINT: i32 = 16;
    // 0x11, confirmed
    pub const WORLD_MONO: i32 = 17;
    // 0x12, confirmed
    pub const WORLD_CAMERA_EYE_X_EVE: i32 = 18;
    // 0x13, confirmed
    pub const WORLD_CAMERA_EYE_Y_EVE: i32 = 19;
    // 0x14, confirmed
    pub const WORLD_CAMERA_EYE_Z_EVE: i32 = 20;
    // 0x15, confirmed
    pub const WORLD_CAMERA_PINT_X_EVE: i32 = 21;
    // 0x16, confirmed
    pub const WORLD_CAMERA_PINT_Y_EVE: i32 = 22;
    // 0x17, confirmed
    pub const WORLD_CAMERA_PINT_Z_EVE: i32 = 23;
    // 0x18, confirmed
    pub const WORLD_CAMERA_UP_X_EVE: i32 = 24;
    // 0x19, confirmed
    pub const WORLD_CAMERA_UP_Y_EVE: i32 = 25;
    // 0x1A, confirmed
    pub const WORLD_CAMERA_UP_Z_EVE: i32 = 26;
    // 0x1B, confirmed
    pub const WORLD_SET_CAMERA_EVE_XZ_ROTATE: i32 = 27;
    // 0x1C, confirmed
    pub const WORLD_ORDER: i32 = 28;
    // 0x1D, confirmed
    pub const WORLD_LAYER: i32 = 29;
    // 0x1E, confirmed
    pub const WORLD_WIPE_COPY: i32 = 30;
    // 0x1F, confirmed
    pub const WORLD_WIPE_ERASE: i32 = 31;

    // WORLDLIST
    // 0x01, confirmed
    pub const WORLDLIST_CREATE_WORLD: i32 = 1;
    // 0x02, confirmed
    pub const WORLDLIST_DESTROY_WORLD: i32 = 2;
}

#[inline]
pub fn canonical_form_id(configured: u32, canonical: u32) -> u32 {
    if configured != 0 {
        configured
    } else {
        canonical
    }
}

#[inline]
pub fn matches_form_id(form_id: u32, configured: u32, canonical: u32) -> bool {
    form_id == canonical || (configured != 0 && form_id == configured)
}

#[inline]
pub fn is_stage_global_form(form_id: u32, configured: u32) -> bool {
    form_id == global_form::STAGE_ALT
        || form_id == global_form::STAGE_DEFAULT
        || form_id == global_form::STAGE_ALIAS_37
        || form_id == global_form::STAGE_ALIAS_38
        || (configured != 0 && form_id == configured)
}

pub use elm_value::*;
pub use global_form::*;

// -----------------------------------------------------------------------------
// Static runtime constants.
// -----------------------------------------------------------------------------

// Runtime numeric constants recovered from the original engine and reverse results.
// Values here are fixed constants used directly by the runtime.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct RuntimeConstants {
    // Global form IDs
    pub form_global_stage: u32,
    pub form_global_mov: u32,
    pub form_global_bgm: u32,
    pub form_global_bgm_table: u32,
    pub form_global_pcm: u32,
    pub form_global_pcmch: u32,
    pub form_global_se: u32,
    pub form_global_pcm_event: u32,
    pub form_global_excall: u32,
    pub form_global_koe_st: u32,

    pub form_global_screen: u32,
    pub form_global_msgbk: u32,

    pub form_global_input: u32,
    pub form_global_mouse: u32,
    pub form_global_keylist: u32,
    pub form_global_key: u32,

    pub form_global_syscom: u32,
    pub form_global_script: u32,
    pub form_global_system: u32,
    pub form_global_frame_action: u32,
    pub form_global_frame_action_ch: u32,

    pub form_global_math: u32,
    pub form_global_cgtable: u32,
    pub form_global_database: u32,
    pub form_global_g00buf: u32,
    pub form_global_mask: u32,
    pub form_global_editbox: u32,
    pub form_global_file: u32,
    pub form_global_steam: u32,

    // SCREEN selectors and aliases (optional)
    pub screen_sel_effect: i32,
    pub screen_sel_quake: i32,
    pub screen_sel_shake: i32,

    pub screen_x: i32,
    pub screen_y: i32,
    pub screen_z: i32,
    pub screen_mono: i32,
    pub screen_reverse: i32,
    pub screen_bright: i32,
    pub screen_dark: i32,
    pub screen_color_r: i32,
    pub screen_color_g: i32,
    pub screen_color_b: i32,
    pub screen_color_rate: i32,
    pub screen_color_add_r: i32,
    pub screen_color_add_g: i32,
    pub screen_color_add_b: i32,

    pub screen_x_eve: i32,
    pub screen_y_eve: i32,
    pub screen_z_eve: i32,
    pub screen_mono_eve: i32,
    pub screen_reverse_eve: i32,
    pub screen_bright_eve: i32,
    pub screen_dark_eve: i32,
    pub screen_color_r_eve: i32,
    pub screen_color_g_eve: i32,
    pub screen_color_b_eve: i32,
    pub screen_color_rate_eve: i32,
    pub screen_color_add_r_eve: i32,
    pub screen_color_add_g_eve: i32,
    pub screen_color_add_b_eve: i32,

    // EFFECT item op IDs (optional)
    pub effect_init: i32,
    pub effect_wipe_copy: i32,
    pub effect_wipe_erase: i32,
    pub effect_x: i32,
    pub effect_y: i32,
    pub effect_z: i32,
    pub effect_mono: i32,
    pub effect_reverse: i32,
    pub effect_bright: i32,
    pub effect_dark: i32,
    pub effect_color_r: i32,
    pub effect_color_g: i32,
    pub effect_color_b: i32,
    pub effect_color_rate: i32,
    pub effect_color_add_r: i32,
    pub effect_color_add_g: i32,
    pub effect_color_add_b: i32,
    pub effect_x_eve: i32,
    pub effect_y_eve: i32,
    pub effect_z_eve: i32,
    pub effect_mono_eve: i32,
    pub effect_reverse_eve: i32,
    pub effect_bright_eve: i32,
    pub effect_dark_eve: i32,
    pub effect_color_r_eve: i32,
    pub effect_color_g_eve: i32,
    pub effect_color_b_eve: i32,
    pub effect_color_rate_eve: i32,
    pub effect_color_add_r_eve: i32,
    pub effect_color_add_g_eve: i32,
    pub effect_color_add_b_eve: i32,
    pub effect_begin_order: i32,
    pub effect_end_order: i32,
    pub effect_begin_layer: i32,
    pub effect_end_layer: i32,

    // Input/Key op codes (all externally configurable)
    pub exkey_decide: i32,
    pub exkey_cancel: i32,

    pub input_op_decide: i32,
    pub input_op_cancel: i32,
    pub input_op_clear: i32,
    pub input_op_next: i32,

    pub mouse_op_x: i32,
    pub mouse_op_y: i32,
    pub mouse_op_clear: i32,
    pub mouse_op_wheel: i32,
    pub mouse_op_left: i32,
    pub mouse_op_right: i32,
    pub mouse_op_next: i32,
    pub mouse_op_get_pos: i32,
    pub mouse_op_set_pos: i32,

    pub keylist_op_wait: i32,
    pub keylist_op_wait_force: i32,
    pub keylist_op_clear: i32,
    pub keylist_op_next: i32,

    pub key_op_dir: i32,
    pub key_op_on_down: i32,
    pub key_op_on_up: i32,
    pub key_op_on_down_up: i32,
    pub key_op_is_down: i32,
    pub key_op_is_up: i32,
    pub key_op_on_flick: i32,
    pub key_op_flick: i32,
    pub key_op_flick_angle: i32,

    // MATH element codes (all externally configurable)
    pub math_max: i32,
    pub math_min: i32,
    pub math_limit: i32,
    pub math_abs: i32,
    pub math_rand: i32,
    pub math_sqrt: i32,
    pub math_log: i32,
    pub math_log2: i32,
    pub math_log10: i32,
    pub math_sin: i32,
    pub math_cos: i32,
    pub math_tan: i32,
    pub math_arcsin: i32,
    pub math_arccos: i32,
    pub math_arctan: i32,
    pub math_distance: i32,
    pub math_angle: i32,
    pub math_linear: i32,
    pub math_tostr: i32,
    pub math_tostr_zero: i32,

    // CGTABLE element codes (all externally configurable)
    pub cgtable_flag: i32,
    pub cgtable_set_disable: i32,
    pub cgtable_set_enable: i32,
    pub cgtable_set_all_flag: i32,
    pub cgtable_get_cg_cnt: i32,
    pub cgtable_get_look_cnt: i32,
    pub cgtable_get_look_percent: i32,
    pub cgtable_get_flag_no_by_name: i32,
    pub cgtable_get_look_by_name: i32,
    pub cgtable_set_look_by_name: i32,
    pub cgtable_get_name_by_flag_no: i32,

    // DATABASE element codes (all externally configurable)
    pub database_list_get_size: i32,
    pub database_get_num: i32,
    pub database_get_str: i32,
    pub database_check_item: i32,
    pub database_check_column: i32,
    pub database_find_num: i32,
    pub database_find_str: i32,
    pub database_find_str_real: i32,

    // G00BUF element codes (all externally configurable)
    pub g00buf_list_get_size: i32,
    pub g00buf_list_free_all: i32,
    pub g00buf_load: i32,
    pub g00buf_free: i32,

    // FILE element codes (all externally configurable)
    pub file_preload_omv: i32,

    // STEAM element codes (all externally configurable)
    pub steam_set_achievement: i32,
    pub steam_reset_all_status: i32,

    // Element helpers
    pub elm_array: i32,
    pub elm_up: i32,

    // Stage element codes
    pub stage_elm_object: i32,
    pub stage_elm_world: i32,

    // World list element codes (optional)
    pub worldlist_create: i32,
    pub worldlist_destroy: i32,

    // World element codes (optional)
    pub world_init: i32,
    pub world_get_no: i32,
    pub world_mode: i32,
    pub world_camera_eye_x: i32,
    pub world_camera_eye_y: i32,
    pub world_camera_eye_z: i32,
    pub world_camera_pint_x: i32,
    pub world_camera_pint_y: i32,
    pub world_camera_pint_z: i32,
    pub world_camera_up_x: i32,
    pub world_camera_up_y: i32,
    pub world_camera_up_z: i32,
    pub world_camera_eye_x_eve: i32,
    pub world_camera_eye_y_eve: i32,
    pub world_camera_eye_z_eve: i32,
    pub world_camera_pint_x_eve: i32,
    pub world_camera_pint_y_eve: i32,
    pub world_camera_pint_z_eve: i32,
    pub world_camera_up_x_eve: i32,
    pub world_camera_up_y_eve: i32,
    pub world_camera_up_z_eve: i32,
    pub world_camera_view_angle: i32,
    pub world_set_camera_eye: i32,
    pub world_calc_camera_eye: i32,
    pub world_set_camera_pint: i32,
    pub world_calc_camera_pint: i32,
    pub world_set_camera_up: i32,
    pub world_mono: i32,
    pub world_set_camera_eve_xz_rotate: i32,
    pub world_order: i32,
    pub world_layer: i32,
    pub world_wipe_copy: i32,
    pub world_wipe_erase: i32,

    // Object element codes (subset)
    pub obj_disp: i32,
    pub obj_patno: i32,
    pub obj_alpha: i32,
    pub obj_layer: i32,
    pub obj_order: i32,
    pub obj_x: i32,
    pub obj_y: i32,
    pub obj_z: i32,
    pub obj_create: i32,

    // Object creation ops
    pub obj_create_number: i32,
    pub obj_create_weather: i32,
    pub obj_create_mesh: i32,
    pub obj_create_billboard: i32,
    pub obj_create_save_thumb: i32,
    pub obj_create_capture_thumb: i32,
    pub obj_create_capture: i32,
    pub obj_create_movie: i32,
    pub obj_create_movie_loop: i32,
    pub obj_create_movie_wait: i32,
    pub obj_create_movie_wait_key: i32,
    pub obj_create_emote: i32,
    pub obj_create_copy_from: i32,

    // Weather / Movie param ops
    pub obj_set_weather_param_type_a: i32,
    pub obj_set_weather_param_type_b: i32,
    pub obj_pause_movie: i32,
    pub obj_resume_movie: i32,
    pub obj_seek_movie: i32,
    pub obj_get_movie_seek_time: i32,
    pub obj_check_movie: i32,
    pub obj_wait_movie: i32,
    pub obj_wait_movie_key: i32,
    pub obj_end_movie_loop: i32,
    pub obj_set_movie_auto_free: i32,

    // Button ops
    pub obj_clear_button: i32,
    pub obj_set_button: i32,
    pub obj_set_button_group: i32,
    pub obj_set_button_pushkeep: i32,
    pub obj_get_button_pushkeep: i32,
    pub obj_set_button_alpha_test: i32,
    pub obj_get_button_alpha_test: i32,
    pub obj_set_button_state_normal: i32,
    pub obj_set_button_state_select: i32,
    pub obj_set_button_state_disable: i32,
    pub obj_get_button_state: i32,
    pub obj_get_button_hit_state: i32,
    pub obj_get_button_real_state: i32,
    pub obj_set_button_call: i32,
    pub obj_clear_button_call: i32,

    // Frame action and GAN
    pub obj_frame_action: i32,
    pub obj_frame_action_ch: i32,
    pub obj_load_gan: i32,
    pub obj_start_gan: i32,

    // Stage object command-like ops.
    pub obj_wipe_copy: i32,
    pub obj_wipe_erase: i32,
    pub obj_click_disable: i32,

    // Object element codes (extended subset)
    pub obj_world: i32,
    pub obj_center_x: i32,
    pub obj_center_y: i32,
    pub obj_center_z: i32,
    pub obj_set_center: i32,
    pub obj_scale_x: i32,
    pub obj_scale_y: i32,
    pub obj_scale_z: i32,
    pub obj_set_scale: i32,
    pub obj_rotate_x: i32,
    pub obj_rotate_y: i32,
    pub obj_rotate_z: i32,
    pub obj_set_rotate: i32,
    pub obj_clip_left: i32,
    pub obj_clip_top: i32,
    pub obj_clip_right: i32,
    pub obj_clip_bottom: i32,
    pub obj_set_clip: i32,
    pub obj_src_clip_left: i32,
    pub obj_src_clip_top: i32,
    pub obj_src_clip_right: i32,
    pub obj_src_clip_bottom: i32,
    pub obj_set_src_clip: i32,
    pub obj_tr: i32,
    pub obj_mono: i32,
    pub obj_reverse: i32,
    pub obj_bright: i32,
    pub obj_dark: i32,
    pub obj_color_r: i32,
    pub obj_color_g: i32,
    pub obj_color_b: i32,
    pub obj_color_rate: i32,
    pub obj_color_add_r: i32,
    pub obj_color_add_g: i32,
    pub obj_color_add_b: i32,

    pub obj_set_pos: i32,
    pub obj_x_rep: i32,
    pub obj_y_rep: i32,
    pub obj_z_rep: i32,

    pub obj_center_rep_x: i32,
    pub obj_center_rep_y: i32,
    pub obj_center_rep_z: i32,
    pub obj_set_center_rep: i32,

    pub obj_clip_use: i32,
    pub obj_src_clip_use: i32,

    pub obj_mask_no: i32,
    pub obj_tonecurve_no: i32,
    pub obj_culling: i32,
    pub obj_alpha_test: i32,
    pub obj_alpha_blend: i32,
    pub obj_blend: i32,
    pub obj_light_no: i32,
    pub obj_fog_use: i32,
    pub obj_mesh_anim_clip: i32,
    pub obj_mesh_anim_clip_name: i32,
    pub obj_mesh_anim_rate: i32,
    pub obj_mesh_anim_time_offset: i32,
    pub obj_mesh_anim_pause: i32,
    pub obj_mesh_anim_hold_time: i32,
    pub obj_mesh_anim_shift_time: i32,
    pub obj_mesh_anim_loop: i32,
    pub obj_mesh_anim_blend_clip: i32,
    pub obj_mesh_anim_blend_clip_name: i32,
    pub obj_mesh_anim_blend_weight: i32,

    // Object *_EVE element codes
    pub obj_patno_eve: i32,
    pub obj_x_eve: i32,
    pub obj_y_eve: i32,
    pub obj_z_eve: i32,

    pub obj_x_rep_eve: i32,
    pub obj_y_rep_eve: i32,
    pub obj_z_rep_eve: i32,

    pub obj_center_x_eve: i32,
    pub obj_center_y_eve: i32,
    pub obj_center_z_eve: i32,

    pub obj_center_rep_x_eve: i32,
    pub obj_center_rep_y_eve: i32,
    pub obj_center_rep_z_eve: i32,

    pub obj_scale_x_eve: i32,
    pub obj_scale_y_eve: i32,
    pub obj_scale_z_eve: i32,

    pub obj_rotate_x_eve: i32,
    pub obj_rotate_y_eve: i32,
    pub obj_rotate_z_eve: i32,

    pub obj_clip_left_eve: i32,
    pub obj_clip_top_eve: i32,
    pub obj_clip_right_eve: i32,
    pub obj_clip_bottom_eve: i32,

    pub obj_src_clip_left_eve: i32,
    pub obj_src_clip_top_eve: i32,
    pub obj_src_clip_right_eve: i32,
    pub obj_src_clip_bottom_eve: i32,

    pub obj_tr_eve: i32,
    pub obj_tr_rep: i32,
    pub obj_tr_rep_eve: i32,

    pub obj_mono_eve: i32,
    pub obj_reverse_eve: i32,
    pub obj_bright_eve: i32,
    pub obj_dark_eve: i32,

    pub obj_color_r_eve: i32,
    pub obj_color_g_eve: i32,
    pub obj_color_b_eve: i32,
    pub obj_color_rate_eve: i32,
    pub obj_color_add_r_eve: i32,
    pub obj_color_add_g_eve: i32,
    pub obj_color_add_b_eve: i32,

    // Object query methods
    pub obj_get_pat_cnt: i32,
    pub obj_get_size_x: i32,
    pub obj_get_size_y: i32,
    pub obj_get_size_z: i32,
    pub obj_get_pixel_color_r: i32,
    pub obj_get_pixel_color_g: i32,
    pub obj_get_pixel_color_b: i32,
    pub obj_get_pixel_color_a: i32,

    pub obj_f: i32,

    // Object methods (subset)
    pub obj_change_file: i32,
    pub obj_exist_type: i32,
    pub obj_set_string: i32,
    pub obj_get_string: i32,
    pub obj_set_string_param: i32,
    pub obj_set_number: i32,
    pub obj_get_number: i32,
    pub obj_set_number_param: i32,

    // Object ALL_EVE and allevent sub-ops
    pub obj_all_eve: i32,
    pub elm_allevent_end: i32,
    pub elm_allevent_wait: i32,
    pub elm_allevent_check: i32,

    // Object methods (subset)
    pub obj_init: i32,
    pub obj_free: i32,
    pub obj_init_param: i32,
    pub obj_get_file_name: i32,
}

impl Default for RuntimeConstants {
    fn default() -> Self {
        let mut out = Self {
            form_global_stage: fm::STAGE as u32,
            form_global_mov: fm::MOV as u32,
            form_global_bgm: fm::BGM as u32,
            form_global_pcm: fm::PCM as u32,
            form_global_pcmch: fm::PCMCH as u32,
            form_global_se: fm::SE as u32,
            form_global_pcm_event: fm::PCMEVENT as u32,
            form_global_excall: fm::EXCALL as u32,
            form_global_koe_st: fm::KOE as u32,
            form_global_bgm_table: global_form::BGMTABLE,

            form_global_screen: fm::SCREEN as u32,
            form_global_msgbk: global_form::MSGBK,

            form_global_input: fm::INPUT as u32,
            form_global_mouse: fm::MOUSE as u32,
            form_global_keylist: fm::KEYLIST as u32,
            form_global_key: fm::KEY as u32,

            // Optional global forms (disabled by default).
            form_global_syscom: fm::SYSCOM as u32,
            form_global_script: fm::SCRIPT as u32,
            form_global_system: fm::SYSTEM as u32,
            form_global_frame_action: fm::FRAMEACTION as u32,
            form_global_frame_action_ch: fm::FRAMEACTIONLIST as u32,

            form_global_math: fm::MATH as u32,
            form_global_cgtable: fm::CGTABLE as u32,
            form_global_database: fm::DATABASE as u32,
            form_global_g00buf: fm::G00BUF as u32,
            form_global_mask: fm::MASK as u32,
            form_global_editbox: fm::EDITBOX as u32,
            form_global_file: fm::FILE as u32,
            form_global_steam: 0,

            screen_sel_effect: elm_value::SCREEN_EFFECT,
            screen_sel_quake: elm_value::SCREEN_QUAKE,
            screen_sel_shake: elm_value::SCREEN_SHAKE,

            screen_x: elm_value::SCREEN_X,
            screen_y: elm_value::SCREEN_Y,
            screen_z: elm_value::SCREEN_Z,
            screen_mono: elm_value::SCREEN_MONO,
            screen_reverse: elm_value::SCREEN_REVERSE,
            screen_bright: elm_value::SCREEN_BRIGHT,
            screen_dark: elm_value::SCREEN_DARK,
            screen_color_r: elm_value::SCREEN_COLOR_R,
            screen_color_g: elm_value::SCREEN_COLOR_G,
            screen_color_b: elm_value::SCREEN_COLOR_B,
            screen_color_rate: elm_value::SCREEN_COLOR_RATE,
            screen_color_add_r: elm_value::SCREEN_COLOR_ADD_R,
            screen_color_add_g: elm_value::SCREEN_COLOR_ADD_G,
            screen_color_add_b: elm_value::SCREEN_COLOR_ADD_B,

            screen_x_eve: elm_value::SCREEN_X_EVE,
            screen_y_eve: elm_value::SCREEN_Y_EVE,
            screen_z_eve: elm_value::SCREEN_Z_EVE,
            screen_mono_eve: elm_value::SCREEN_MONO_EVE,
            screen_reverse_eve: elm_value::SCREEN_REVERSE_EVE,
            screen_bright_eve: elm_value::SCREEN_BRIGHT_EVE,
            screen_dark_eve: elm_value::SCREEN_DARK_EVE,
            screen_color_r_eve: elm_value::SCREEN_COLOR_R_EVE,
            screen_color_g_eve: elm_value::SCREEN_COLOR_G_EVE,
            screen_color_b_eve: elm_value::SCREEN_COLOR_B_EVE,
            screen_color_rate_eve: elm_value::SCREEN_COLOR_RATE_EVE,
            screen_color_add_r_eve: elm_value::SCREEN_COLOR_ADD_R_EVE,
            screen_color_add_g_eve: elm_value::SCREEN_COLOR_ADD_G_EVE,
            screen_color_add_b_eve: elm_value::SCREEN_COLOR_ADD_B_EVE,

            effect_init: elm_value::EFFECT_INIT,
            effect_wipe_copy: elm_value::EFFECT_WIPE_COPY,
            effect_wipe_erase: elm_value::EFFECT_WIPE_ERASE,
            effect_x: elm_value::EFFECT_X,
            effect_y: elm_value::EFFECT_Y,
            effect_z: elm_value::EFFECT_Z,
            effect_mono: elm_value::EFFECT_MONO,
            effect_reverse: elm_value::EFFECT_REVERSE,
            effect_bright: elm_value::EFFECT_BRIGHT,
            effect_dark: elm_value::EFFECT_DARK,
            effect_color_r: elm_value::EFFECT_COLOR_R,
            effect_color_g: elm_value::EFFECT_COLOR_G,
            effect_color_b: elm_value::EFFECT_COLOR_B,
            effect_color_rate: elm_value::EFFECT_COLOR_RATE,
            effect_color_add_r: elm_value::EFFECT_COLOR_ADD_R,
            effect_color_add_g: elm_value::EFFECT_COLOR_ADD_G,
            effect_color_add_b: elm_value::EFFECT_COLOR_ADD_B,
            effect_x_eve: elm_value::EFFECT_X_EVE,
            effect_y_eve: elm_value::EFFECT_Y_EVE,
            effect_z_eve: elm_value::EFFECT_Z_EVE,
            effect_mono_eve: elm_value::EFFECT_MONO_EVE,
            effect_reverse_eve: elm_value::EFFECT_REVERSE_EVE,
            effect_bright_eve: elm_value::EFFECT_BRIGHT_EVE,
            effect_dark_eve: elm_value::EFFECT_DARK_EVE,
            effect_color_r_eve: elm_value::EFFECT_COLOR_R_EVE,
            effect_color_g_eve: elm_value::EFFECT_COLOR_G_EVE,
            effect_color_b_eve: elm_value::EFFECT_COLOR_B_EVE,
            effect_color_rate_eve: elm_value::EFFECT_COLOR_RATE_EVE,
            effect_color_add_r_eve: elm_value::EFFECT_COLOR_ADD_R_EVE,
            effect_color_add_g_eve: elm_value::EFFECT_COLOR_ADD_G_EVE,
            effect_color_add_b_eve: elm_value::EFFECT_COLOR_ADD_B_EVE,
            effect_begin_order: elm_value::EFFECT_BEGIN_ORDER,
            effect_end_order: elm_value::EFFECT_END_ORDER,
            effect_begin_layer: elm_value::EFFECT_BEGIN_LAYER,
            effect_end_layer: elm_value::EFFECT_END_LAYER,

            // EX key IDs (used by INPUT/KEY).
            exkey_decide: 256,
            exkey_cancel: 257,

            // INPUT sub-ops.
            input_op_decide: 0,
            input_op_cancel: 1,
            input_op_clear: 2,
            input_op_next: 3,

            // MOUSE sub-ops.
            mouse_op_x: 0,
            mouse_op_y: 1,
            mouse_op_clear: 4,
            mouse_op_wheel: 5,
            mouse_op_left: 6,
            mouse_op_right: 7,
            mouse_op_next: 8,
            mouse_op_get_pos: 9,
            mouse_op_set_pos: 10,

            // KEYLIST sub-ops.
            keylist_op_wait: 0,
            keylist_op_wait_force: 1,
            keylist_op_clear: 3,
            keylist_op_next: 5,

            // KEY sub-ops.
            key_op_dir: 0,
            key_op_on_down: 1,
            key_op_on_up: 4,
            key_op_on_down_up: 5,
            key_op_is_down: 6,
            key_op_is_up: 7,
            key_op_on_flick: 10,
            key_op_flick: 14,
            key_op_flick_angle: 15,

            // MATH element codes (disabled by default).
            math_max: 0,
            math_min: 0,
            math_limit: 0,
            math_abs: 0,
            math_rand: 0,
            math_sqrt: 0,
            math_log: 0,
            math_log2: 0,
            math_log10: 0,
            math_sin: 0,
            math_cos: 0,
            math_tan: 0,
            math_arcsin: 0,
            math_arccos: 0,
            math_arctan: 0,
            math_distance: 0,
            math_angle: 0,
            math_linear: 0,
            math_tostr: 0,
            math_tostr_zero: 0,

            // CGTABLE element codes (disabled by default).
            cgtable_flag: 0,
            cgtable_set_disable: 0,
            cgtable_set_enable: 0,
            cgtable_set_all_flag: 0,
            cgtable_get_cg_cnt: 0,
            cgtable_get_look_cnt: 0,
            cgtable_get_look_percent: 0,
            cgtable_get_flag_no_by_name: 0,
            cgtable_get_look_by_name: 0,
            cgtable_set_look_by_name: 0,
            cgtable_get_name_by_flag_no: 0,

            // DATABASE element codes (disabled by default).
            database_list_get_size: 0,
            database_get_num: 0,
            database_get_str: 0,
            database_check_item: 0,
            database_check_column: 0,
            database_find_num: 0,
            database_find_str: 0,
            database_find_str_real: 0,

            // G00BUF element codes (disabled by default).
            g00buf_list_get_size: 0,
            g00buf_list_free_all: 0,
            g00buf_load: 0,
            g00buf_free: 0,

            // FILE element codes (disabled by default).
            file_preload_omv: 0,

            // STEAM element codes (disabled by default).
            steam_set_achievement: 0,
            steam_reset_all_status: 0,

            elm_array: -1,
            elm_up: -5,

            stage_elm_object: elm_value::STAGE_OBJECT,
            stage_elm_world: elm_value::STAGE_WORLD,

            worldlist_create: 0,
            worldlist_destroy: 0,

            world_init: 0,
            world_get_no: 0,
            world_mode: 0,
            world_camera_eye_x: 0,
            world_camera_eye_y: 0,
            world_camera_eye_z: 0,
            world_camera_pint_x: 0,
            world_camera_pint_y: 0,
            world_camera_pint_z: 0,
            world_camera_up_x: 0,
            world_camera_up_y: 0,
            world_camera_up_z: 0,
            world_camera_eye_x_eve: 0,
            world_camera_eye_y_eve: 0,
            world_camera_eye_z_eve: 0,
            world_camera_pint_x_eve: 0,
            world_camera_pint_y_eve: 0,
            world_camera_pint_z_eve: 0,
            world_camera_up_x_eve: 0,
            world_camera_up_y_eve: 0,
            world_camera_up_z_eve: 0,
            world_camera_view_angle: 0,
            world_set_camera_eye: 0,
            world_calc_camera_eye: 0,
            world_set_camera_pint: 0,
            world_calc_camera_pint: 0,
            world_set_camera_up: 0,
            world_mono: 0,
            world_set_camera_eve_xz_rotate: 0,
            world_order: 0,
            world_layer: 0,
            world_wipe_copy: 0,
            world_wipe_erase: 0,

            obj_disp: elm_value::OBJECT_DISP,
            obj_patno: elm_value::OBJECT_PATNO,
            obj_alpha: 0,
            obj_order: elm_value::OBJECT_ORDER,
            obj_layer: elm_value::OBJECT_LAYER,
            obj_x: elm_value::OBJECT_X,
            obj_y: elm_value::OBJECT_Y,
            obj_z: elm_value::OBJECT_Z,
            obj_create: elm_value::OBJECT_CREATE,
            obj_create_number: elm_value::OBJECT_CREATE_NUMBER,
            obj_create_weather: elm_value::OBJECT_CREATE_WEATHER,
            obj_create_mesh: elm_value::OBJECT_CREATE_MESH,
            obj_create_billboard: elm_value::OBJECT_CREATE_BILLBOARD,
            obj_create_save_thumb: elm_value::OBJECT_CREATE_SAVE_THUMB,
            obj_create_capture_thumb: elm_value::OBJECT_CREATE_CAPTURE_THUMB,
            obj_create_capture: elm_value::OBJECT_CREATE_CAPTURE,
            obj_create_movie: elm_value::OBJECT_CREATE_MOVIE,
            obj_create_movie_loop: elm_value::OBJECT_CREATE_MOVIE_LOOP,
            obj_create_movie_wait: elm_value::OBJECT_CREATE_MOVIE_WAIT,
            obj_create_movie_wait_key: elm_value::OBJECT_CREATE_MOVIE_WAIT_KEY,
            obj_create_emote: 0,
            obj_create_copy_from: elm_value::OBJECT_CREATE_COPY_FROM,
            obj_set_weather_param_type_a: elm_value::OBJECT_SET_WEATHER_PARAM_TYPE_A,
            obj_set_weather_param_type_b: elm_value::OBJECT_SET_WEATHER_PARAM_TYPE_B,
            obj_pause_movie: elm_value::OBJECT_PAUSE_MOVIE,
            obj_resume_movie: elm_value::OBJECT_RESUME_MOVIE,
            obj_seek_movie: elm_value::OBJECT_SEEK_MOVIE,
            obj_get_movie_seek_time: elm_value::OBJECT_GET_MOVIE_SEEK_TIME,
            obj_check_movie: elm_value::OBJECT_CHECK_MOVIE,
            obj_wait_movie: elm_value::OBJECT_WAIT_MOVIE,
            obj_wait_movie_key: elm_value::OBJECT_WAIT_MOVIE_KEY,
            obj_end_movie_loop: 0,
            obj_set_movie_auto_free: elm_value::OBJECT_SET_MOVIE_AUTO_FREE,
            obj_clear_button: elm_value::OBJECT_CLEAR_BUTTON,
            obj_set_button: elm_value::OBJECT_SET_BUTTON,
            obj_set_button_group: elm_value::OBJECT_SET_BUTTON_GROUP,
            obj_set_button_pushkeep: elm_value::OBJECT_SET_BUTTON_PUSHKEEP,
            obj_get_button_pushkeep: elm_value::OBJECT_GET_BUTTON_PUSHKEEP,
            obj_set_button_alpha_test: elm_value::OBJECT_SET_BUTTON_ALPHA_TEST,
            obj_get_button_alpha_test: elm_value::OBJECT_GET_BUTTON_ALPHA_TEST,
            obj_set_button_state_normal: elm_value::OBJECT_SET_BUTTON_STATE_NORMAL,
            obj_set_button_state_select: elm_value::OBJECT_SET_BUTTON_STATE_SELECT,
            obj_set_button_state_disable: elm_value::OBJECT_SET_BUTTON_STATE_DISABLE,
            obj_get_button_state: elm_value::OBJECT_GET_BUTTON_STATE,
            obj_get_button_hit_state: elm_value::OBJECT_GET_BUTTON_HIT_STATE,
            obj_get_button_real_state: elm_value::OBJECT_GET_BUTTON_REAL_STATE,
            obj_set_button_call: elm_value::OBJECT_SET_BUTTON_CALL,
            obj_clear_button_call: elm_value::OBJECT_CLEAR_BUTTON_CALL,
            obj_frame_action: elm_value::OBJECT_FRAME_ACTION,
            obj_frame_action_ch: elm_value::OBJECT_FRAME_ACTION_CH,
            obj_load_gan: elm_value::OBJECT_LOAD_GAN,
            obj_start_gan: elm_value::OBJECT_START_GAN,

            obj_wipe_copy: elm_value::OBJECT_WIPE_COPY,
            obj_wipe_erase: elm_value::OBJECT_WIPE_ERASE,
            obj_click_disable: elm_value::OBJECT_CLICK_DISABLE,

            // Extended subset (default unknown unless overridden)
            obj_world: elm_value::OBJECT_WORLD,
            obj_center_x: elm_value::OBJECT_CENTER_X,
            obj_center_y: elm_value::OBJECT_CENTER_Y,
            obj_center_z: elm_value::OBJECT_CENTER_Z,
            obj_set_center: elm_value::OBJECT_SET_CENTER,
            obj_scale_x: elm_value::OBJECT_SCALE_X,
            obj_scale_y: elm_value::OBJECT_SCALE_Y,
            obj_scale_z: elm_value::OBJECT_SCALE_Z,
            obj_set_scale: elm_value::OBJECT_SET_SCALE,
            obj_rotate_x: elm_value::OBJECT_ROTATE_X,
            obj_rotate_y: elm_value::OBJECT_ROTATE_Y,
            obj_rotate_z: elm_value::OBJECT_ROTATE_Z,
            obj_set_rotate: elm_value::OBJECT_SET_ROTATE,
            obj_clip_left: elm_value::OBJECT_CLIP_LEFT,
            obj_clip_top: elm_value::OBJECT_CLIP_TOP,
            obj_clip_right: elm_value::OBJECT_CLIP_RIGHT,
            obj_clip_bottom: elm_value::OBJECT_CLIP_BOTTOM,
            obj_set_clip: elm_value::OBJECT_SET_CLIP,
            obj_src_clip_left: elm_value::OBJECT_SRC_CLIP_LEFT,
            obj_src_clip_top: elm_value::OBJECT_SRC_CLIP_TOP,
            obj_src_clip_right: elm_value::OBJECT_SRC_CLIP_RIGHT,
            obj_src_clip_bottom: elm_value::OBJECT_SRC_CLIP_BOTTOM,
            obj_set_src_clip: elm_value::OBJECT_SET_SRC_CLIP,
            obj_tr: elm_value::OBJECT_TR,
            obj_mono: elm_value::OBJECT_MONO,
            obj_reverse: elm_value::OBJECT_REVERSE,
            obj_bright: elm_value::OBJECT_BRIGHT,
            obj_dark: elm_value::OBJECT_DARK,
            obj_color_r: elm_value::OBJECT_COLOR_R,
            obj_color_g: elm_value::OBJECT_COLOR_G,
            obj_color_b: elm_value::OBJECT_COLOR_B,
            obj_color_rate: elm_value::OBJECT_COLOR_RATE,
            obj_color_add_r: elm_value::OBJECT_COLOR_ADD_R,
            obj_color_add_g: elm_value::OBJECT_COLOR_ADD_G,
            obj_color_add_b: elm_value::OBJECT_COLOR_ADD_B,

            obj_set_pos: elm_value::OBJECT_SET_POS,
            obj_x_rep: elm_value::OBJECT_X_REP,
            obj_y_rep: elm_value::OBJECT_Y_REP,
            obj_z_rep: elm_value::OBJECT_Z_REP,

            obj_center_rep_x: elm_value::OBJECT_CENTER_REP_X,
            obj_center_rep_y: elm_value::OBJECT_CENTER_REP_Y,
            obj_center_rep_z: elm_value::OBJECT_CENTER_REP_Z,
            obj_set_center_rep: elm_value::OBJECT_SET_CENTER_REP,

            obj_clip_use: elm_value::OBJECT_CLIP_USE,
            obj_src_clip_use: elm_value::OBJECT_SRC_CLIP_USE,

            obj_mask_no: elm_value::OBJECT_MASK_NO,
            obj_tonecurve_no: elm_value::OBJECT_TONECURVE_NO,
            obj_culling: elm_value::OBJECT_CULLING,
            obj_alpha_test: elm_value::OBJECT_ALPHA_TEST,
            obj_alpha_blend: elm_value::OBJECT_ALPHA_BLEND,
            obj_blend: elm_value::OBJECT_BLEND,
            obj_light_no: elm_value::OBJECT_LIGHT_NO,
            obj_fog_use: elm_value::OBJECT_FOG_USE,
            obj_mesh_anim_clip: 0,
            obj_mesh_anim_clip_name: 0,
            obj_mesh_anim_rate: 0,
            obj_mesh_anim_time_offset: 0,
            obj_mesh_anim_pause: 0,
            obj_mesh_anim_hold_time: 0,
            obj_mesh_anim_shift_time: 0,
            obj_mesh_anim_loop: 0,
            obj_mesh_anim_blend_clip: 0,
            obj_mesh_anim_blend_clip_name: 0,
            obj_mesh_anim_blend_weight: 0,

            obj_patno_eve: elm_value::OBJECT_PATNO_EVE,
            obj_x_eve: elm_value::OBJECT_X_EVE,
            obj_y_eve: elm_value::OBJECT_Y_EVE,
            obj_z_eve: elm_value::OBJECT_Z_EVE,

            obj_x_rep_eve: elm_value::OBJECT_X_REP_EVE,
            obj_y_rep_eve: elm_value::OBJECT_Y_REP_EVE,
            obj_z_rep_eve: elm_value::OBJECT_Z_REP_EVE,

            obj_center_x_eve: elm_value::OBJECT_CENTER_X_EVE,
            obj_center_y_eve: elm_value::OBJECT_CENTER_Y_EVE,
            obj_center_z_eve: elm_value::OBJECT_CENTER_Z_EVE,

            obj_center_rep_x_eve: elm_value::OBJECT_CENTER_REP_X_EVE,
            obj_center_rep_y_eve: elm_value::OBJECT_CENTER_REP_Y_EVE,
            obj_center_rep_z_eve: elm_value::OBJECT_CENTER_REP_Z_EVE,

            obj_scale_x_eve: elm_value::OBJECT_SCALE_X_EVE,
            obj_scale_y_eve: elm_value::OBJECT_SCALE_Y_EVE,
            obj_scale_z_eve: elm_value::OBJECT_SCALE_Z_EVE,

            obj_rotate_x_eve: elm_value::OBJECT_ROTATE_X_EVE,
            obj_rotate_y_eve: elm_value::OBJECT_ROTATE_Y_EVE,
            obj_rotate_z_eve: elm_value::OBJECT_ROTATE_Z_EVE,

            obj_clip_left_eve: elm_value::OBJECT_CLIP_LEFT_EVE,
            obj_clip_top_eve: elm_value::OBJECT_CLIP_TOP_EVE,
            obj_clip_right_eve: elm_value::OBJECT_CLIP_RIGHT_EVE,
            obj_clip_bottom_eve: elm_value::OBJECT_CLIP_BOTTOM_EVE,

            obj_src_clip_left_eve: elm_value::OBJECT_SRC_CLIP_LEFT_EVE,
            obj_src_clip_top_eve: elm_value::OBJECT_SRC_CLIP_TOP_EVE,
            obj_src_clip_right_eve: elm_value::OBJECT_SRC_CLIP_RIGHT_EVE,
            obj_src_clip_bottom_eve: elm_value::OBJECT_SRC_CLIP_BOTTOM_EVE,

            obj_tr_eve: elm_value::OBJECT_TR_EVE,
            obj_tr_rep: elm_value::OBJECT_TR_REP,
            obj_tr_rep_eve: elm_value::OBJECT_TR_REP_EVE,

            obj_mono_eve: elm_value::OBJECT_MONO_EVE,
            obj_reverse_eve: elm_value::OBJECT_REVERSE_EVE,
            obj_bright_eve: elm_value::OBJECT_BRIGHT_EVE,
            obj_dark_eve: elm_value::OBJECT_DARK_EVE,

            obj_color_r_eve: elm_value::OBJECT_COLOR_R_EVE,
            obj_color_g_eve: elm_value::OBJECT_COLOR_G_EVE,
            obj_color_b_eve: elm_value::OBJECT_COLOR_B_EVE,
            obj_color_rate_eve: elm_value::OBJECT_COLOR_RATE_EVE,
            obj_color_add_r_eve: elm_value::OBJECT_COLOR_ADD_R_EVE,
            obj_color_add_g_eve: elm_value::OBJECT_COLOR_ADD_G_EVE,
            obj_color_add_b_eve: elm_value::OBJECT_COLOR_ADD_B_EVE,

            obj_get_pat_cnt: elm_value::OBJECT_GET_PAT_CNT,
            obj_get_size_x: elm_value::OBJECT_GET_SIZE_X,
            obj_get_size_y: elm_value::OBJECT_GET_SIZE_Y,
            obj_get_size_z: elm_value::OBJECT_GET_SIZE_Z,
            obj_get_pixel_color_r: elm_value::OBJECT_GET_PIXEL_COLOR_R,
            obj_get_pixel_color_g: elm_value::OBJECT_GET_PIXEL_COLOR_G,
            obj_get_pixel_color_b: elm_value::OBJECT_GET_PIXEL_COLOR_B,
            obj_get_pixel_color_a: elm_value::OBJECT_GET_PIXEL_COLOR_A,

            obj_f: elm_value::OBJECT_F,

            obj_change_file: elm_value::OBJECT_CHANGE_FILE,
            obj_exist_type: elm_value::OBJECT_EXIST_TYPE,
            obj_set_string: elm_value::OBJECT_SET_STRING,
            obj_get_string: elm_value::OBJECT_GET_STRING,
            obj_set_string_param: elm_value::OBJECT_SET_STRING_PARAM,
            obj_set_number: elm_value::OBJECT_SET_NUMBER,
            obj_get_number: elm_value::OBJECT_GET_NUMBER,
            obj_set_number_param: elm_value::OBJECT_SET_NUMBER_PARAM,

            obj_all_eve: elm_value::OBJECT_ALL_EVE,
            elm_allevent_end: 0,
            elm_allevent_wait: 1,
            elm_allevent_check: 2,

            obj_init: elm_value::OBJECT_INIT,
            obj_free: elm_value::OBJECT_FREE,
            obj_init_param: elm_value::OBJECT_INIT_PARAM,
            obj_get_file_name: elm_value::OBJECT_GET_FILE_NAME,
        };

        out
    }
}

fn try_load_u32_map_file(path: &Path, out: &mut HashMap<u32, String>) {
    if !path.is_file() {
        return;
    }
    if let Ok(text) = fs::read_to_string(path) {
        parse_u32_name_map_str(out, &text);
    }
}

fn try_load_i64_map_file(path: &Path, out: &mut HashMap<i64, String>) {
    if !path.is_file() {
        return;
    }
    if let Ok(text) = fs::read_to_string(path) {
        parse_i64_name_map_str(out, &text);
    }
}

fn parse_u32_name_map_str(out: &mut HashMap<u32, String>, text: &str) {
    for line in text.split(|c| c == ';' || c == '\n') {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        let Some(n) = parse_i64(k) else {
            continue;
        };
        if n < 0 {
            continue;
        }
        out.insert(n as u32, v.to_string());
    }
}

fn parse_i64_name_map_str(out: &mut HashMap<i64, String>, text: &str) {
    for line in text.split(|c| c == ';' || c == '\n') {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        let Some(n) = parse_i64(k) else {
            continue;
        };
        out.insert(n, v.to_string());
    }
}

fn parse_i64(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some(rest) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        i64::from_str_radix(rest.trim(), 16).ok()
    } else {
        s.parse::<i64>().ok()
    }
}
