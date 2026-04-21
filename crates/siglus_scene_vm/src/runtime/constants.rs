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
    pub const __ARGSREF: i32 = crate::runtime::forms::codes::FM___ARGSREF;
    pub const __ARGS: i32 = crate::runtime::forms::codes::FM___ARGS;
    pub const LIST: i32 = crate::runtime::forms::codes::FM_LIST;
    pub const GLOBAL: i32 = crate::runtime::forms::codes::FM_GLOBAL;
    pub const GLOBALLIST: i32 = crate::runtime::forms::codes::FM_GLOBALLIST;
    pub const SCENE: i32 = crate::runtime::forms::codes::FM_SCENE;
    pub const SCENELIST: i32 = crate::runtime::forms::codes::FM_SCENELIST;
    pub const CALL: i32 = crate::runtime::forms::codes::FM_CALL;
    pub const CALLLIST: i32 = crate::runtime::forms::codes::FM_CALLLIST;
    pub const VOID: i32 = crate::runtime::forms::codes::FM_VOID;
    pub const VOIDLIST: i32 = crate::runtime::forms::codes::FM_VOIDLIST;
    pub const INT: i32 = crate::runtime::forms::codes::FM_INT;
    pub const INTLIST: i32 = crate::runtime::forms::codes::FM_INTLIST;
    pub const INTLISTLIST: i32 = crate::runtime::forms::codes::FM_INTLISTLIST;
    pub const INTREF: i32 = crate::runtime::forms::codes::FM_INTREF;
    pub const INTLISTREF: i32 = crate::runtime::forms::codes::FM_INTLISTREF;
    pub const INTEVENT: i32 = crate::runtime::forms::codes::FM_INTEVENT;
    pub const INTEVENTLIST: i32 = crate::runtime::forms::codes::FM_INTEVENTLIST;
    pub const ALLEVENT: i32 = crate::runtime::forms::codes::FM_ALLEVENT;
    pub const STR: i32 = crate::runtime::forms::codes::FM_STR;
    pub const STRLIST: i32 = crate::runtime::forms::codes::FM_STRLIST;
    pub const STRLISTLIST: i32 = crate::runtime::forms::codes::FM_STRLISTLIST;
    pub const STRREF: i32 = crate::runtime::forms::codes::FM_STRREF;
    pub const STRLISTREF: i32 = crate::runtime::forms::codes::FM_STRLISTREF;
    pub const LABEL: i32 = crate::runtime::forms::codes::FM_LABEL;
    pub const MATH: i32 = crate::runtime::forms::codes::FM_MATH;
    pub const FILE: i32 = crate::runtime::forms::codes::FM_FILE;
    pub const CGTABLE: i32 = crate::runtime::forms::codes::FM_CGTABLE;
    pub const BGMTABLE: i32 = crate::runtime::forms::codes::FM_BGMTABLE;
    pub const DATABASE: i32 = crate::runtime::forms::codes::FM_DATABASE;
    pub const DATABASELIST: i32 = crate::runtime::forms::codes::FM_DATABASELIST;
    pub const G00BUF: i32 = crate::runtime::forms::codes::FM_G00BUF;
    pub const G00BUFLIST: i32 = crate::runtime::forms::codes::FM_G00BUFLIST;
    pub const MASK: i32 = crate::runtime::forms::codes::FM_MASK;
    pub const MASKLIST: i32 = crate::runtime::forms::codes::FM_MASKLIST;
    pub const COUNTER: i32 = crate::runtime::forms::codes::FM_COUNTER;
    pub const COUNTERLIST: i32 = crate::runtime::forms::codes::FM_COUNTERLIST;
    pub const FRAMEACTION: i32 = crate::runtime::forms::codes::FM_FRAMEACTION;
    pub const FRAMEACTIONLIST: i32 = crate::runtime::forms::codes::FM_FRAMEACTIONLIST;
    pub const WORLD: i32 = crate::runtime::forms::codes::FM_WORLD;
    pub const WORLDLIST: i32 = crate::runtime::forms::codes::FM_WORLDLIST;
    pub const STAGE: i32 = crate::runtime::forms::codes::FM_STAGE;
    pub const STAGELIST: i32 = crate::runtime::forms::codes::FM_STAGELIST;
    pub const OBJECT: i32 = crate::runtime::forms::codes::FM_OBJECT;
    pub const OBJECTLIST: i32 = crate::runtime::forms::codes::FM_OBJECTLIST;
    pub const OBJECTEVENT: i32 = crate::runtime::forms::codes::FM_OBJECTEVENT;
    pub const OBJECTEVENTLIST: i32 = crate::runtime::forms::codes::FM_OBJECTEVENTLIST;
    pub const MWND: i32 = crate::runtime::forms::codes::FM_MWND;
    pub const MWNDLIST: i32 = crate::runtime::forms::codes::FM_MWNDLIST;
    pub const GROUP: i32 = crate::runtime::forms::codes::FM_GROUP;
    pub const GROUPLIST: i32 = crate::runtime::forms::codes::FM_GROUPLIST;
    pub const BTNSELITEM: i32 = crate::runtime::forms::codes::FM_BTNSELITEM;
    pub const BTNSELITEMLIST: i32 = crate::runtime::forms::codes::FM_BTNSELITEMLIST;
    pub const SCREEN: i32 = crate::runtime::forms::codes::FM_SCREEN;
    pub const QUAKE: i32 = crate::runtime::forms::codes::FM_QUAKE;
    pub const QUAKELIST: i32 = crate::runtime::forms::codes::FM_QUAKELIST;
    pub const EDITBOX: i32 = crate::runtime::forms::codes::FM_EDITBOX;
    pub const EDITBOXLIST: i32 = crate::runtime::forms::codes::FM_EDITBOXLIST;
    pub const EFFECT: i32 = crate::runtime::forms::codes::FM_EFFECT;
    pub const EFFECTLIST: i32 = crate::runtime::forms::codes::FM_EFFECTLIST;
    pub const MSGBK: i32 = crate::runtime::forms::codes::FM_MSGBK;
    pub const BGM: i32 = crate::runtime::forms::codes::FM_BGM;
    pub const BGMLIST: i32 = crate::runtime::forms::codes::FM_BGMLIST;
    pub const KOE: i32 = crate::runtime::forms::codes::FM_KOE;
    pub const KOELIST: i32 = crate::runtime::forms::codes::FM_KOELIST;
    pub const PCM: i32 = crate::runtime::forms::codes::FM_PCM;
    pub const PCMCH: i32 = crate::runtime::forms::codes::FM_PCMCH;
    pub const PCMCHLIST: i32 = crate::runtime::forms::codes::FM_PCMCHLIST;
    pub const SE: i32 = crate::runtime::forms::codes::FM_SE;
    pub const MOV: i32 = crate::runtime::forms::codes::FM_MOV;
    pub const PCMEVENT: i32 = crate::runtime::forms::codes::FM_PCMEVENT;
    pub const PCMEVENTLIST: i32 = crate::runtime::forms::codes::FM_PCMEVENTLIST;
    pub const MOUSE: i32 = crate::runtime::forms::codes::FM_MOUSE;
    pub const KEY: i32 = crate::runtime::forms::codes::FM_KEY;
    pub const KEYLIST: i32 = crate::runtime::forms::codes::FM_KEYLIST;
    pub const INPUT: i32 = crate::runtime::forms::codes::FM_INPUT;
    pub const SYSCOM: i32 = crate::runtime::forms::codes::FM_SYSCOM;
    pub const SYSCOMMENU: i32 = crate::runtime::forms::codes::FM_SYSCOMMENU;
    pub const MWNDBTN: i32 = crate::runtime::forms::codes::FM_MWNDBTN;
    pub const SCRIPT: i32 = crate::runtime::forms::codes::FM_SCRIPT;
    pub const SYSTEM: i32 = crate::runtime::forms::codes::FM_SYSTEM;
    pub const EXCALL: i32 = crate::runtime::forms::codes::FM_EXCALL;
    pub const STEAM: i32 = crate::runtime::forms::codes::FM_STEAM;
    pub const INTEGER: i32 = INT;
    pub const STRING: i32 = STR;
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
    pub const FRAME_ACTION_CH: u32 = codes::FORM_GLOBAL_FRAME_ACTION_CH;
    pub const STAGE: u32 = codes::FORM_GLOBAL_STAGE;
    pub const INPUT: u32 = codes::FORM_GLOBAL_INPUT;
    pub const MOUSE: u32 = codes::FORM_GLOBAL_MOUSE;
    pub const MATH: u32 = codes::FORM_GLOBAL_MATH;
    pub const CGTABLE: u32 = codes::FORM_GLOBAL_CGTABLE;
    pub const DATABASE: u32 = codes::FORM_GLOBAL_DATABASE;
    pub const G00BUF: u32 = codes::FORM_GLOBAL_G00BUF;
    pub const MASK: u32 = codes::FORM_GLOBAL_MASK;
    pub const EDITBOX: u32 = codes::FORM_GLOBAL_EDITBOX;
    pub const FILE: u32 = codes::FORM_GLOBAL_FILE;
    pub const STEAM: u32 = codes::FORM_GLOBAL_STEAM;
    pub const SYSCOM: u32 = codes::FORM_GLOBAL_SYSCOM;
    pub const SCRIPT: u32 = codes::FORM_GLOBAL_SCRIPT;
    pub const SYSTEM: u32 = codes::FORM_GLOBAL_SYSTEM;
    pub const BACK: u32 = codes::FORM_GLOBAL_BACK;
    pub const FRONT: u32 = codes::FORM_GLOBAL_FRONT;
    pub const NEXT: u32 = codes::FORM_GLOBAL_NEXT;
    pub const STAGE_DEFAULT: u32 = STAGE;
    pub const STAGE_ALIAS_37: u32 = BACK;
    pub const STAGE_ALIAS_38: u32 = FRONT;
    pub const STAGE_ALT: u32 = STAGE;
    pub const INT_LIST_FORMS: &[u32] = codes::GLOBAL_INT_LIST_FORMS;
    pub const STR_LIST_FORMS: &[u32] = codes::GLOBAL_STR_LIST_FORMS;
}


#[inline]
pub const fn matches_form_id(form_id: u32, runtime_form_id: u32, canonical_form_id: u32) -> bool {
    form_id == canonical_form_id || (runtime_form_id != 0 && form_id == runtime_form_id)
}

#[inline]
pub const fn is_stage_global_form(form_id: u32, runtime_stage_form_id: u32) -> bool {
    matches_form_id(form_id, runtime_stage_form_id, global_form::STAGE)
        || form_id == global_form::BACK
        || form_id == global_form::FRONT
        || form_id == global_form::NEXT
}

pub mod elm_value {
    pub const UP: i32 = crate::runtime::forms::codes::ELM_UP;
    pub const __TRANS: i32 = crate::runtime::forms::codes::ELM___TRANS;
    pub const __SET: i32 = crate::runtime::forms::codes::ELM___SET;
    pub const ARRAY: i32 = crate::runtime::forms::codes::ELM_ARRAY;
    pub const GLOBAL______TEST: i32 = crate::runtime::forms::codes::elm_value::GLOBAL______TEST;
    pub const GLOBAL___IAPP_DUMMY: i32 = crate::runtime::forms::codes::elm_value::GLOBAL___IAPP_DUMMY;
    pub const GLOBAL___IAPP_DUMMY2: i32 = crate::runtime::forms::codes::elm_value::GLOBAL___IAPP_DUMMY2;
    pub const GLOBAL___IAPP_DUMMY_STR: i32 = crate::runtime::forms::codes::elm_value::GLOBAL___IAPP_DUMMY_STR;
    pub const GLOBAL___IAPP_DUMMY2_STR: i32 = crate::runtime::forms::codes::elm_value::GLOBAL___IAPP_DUMMY2_STR;
    pub const GLOBAL___FOG_NAME: i32 = crate::runtime::forms::codes::elm_value::GLOBAL___FOG_NAME;
    pub const GLOBAL___FOG_X: i32 = crate::runtime::forms::codes::elm_value::GLOBAL___FOG_X;
    pub const GLOBAL___FOG_X_EVE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL___FOG_X_EVE;
    pub const GLOBAL___FOG_NEAR: i32 = crate::runtime::forms::codes::elm_value::GLOBAL___FOG_NEAR;
    pub const GLOBAL___FOG_FAR: i32 = crate::runtime::forms::codes::elm_value::GLOBAL___FOG_FAR;
    pub const GLOBAL_NOP: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_NOP;
    pub const GLOBAL_OWARI: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_OWARI;
    pub const GLOBAL_RETURNMENU: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_RETURNMENU;
    pub const GLOBAL_JUMP: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_JUMP;
    pub const GLOBAL_FARCALL: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_FARCALL;
    pub const GLOBAL_GET_SCENE_NAME: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_GET_SCENE_NAME;
    pub const GLOBAL_GET_LINE_NO: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_GET_LINE_NO;
    pub const GLOBAL_SET_TITLE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SET_TITLE;
    pub const GLOBAL_GET_TITLE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_GET_TITLE;
    pub const GLOBAL_SAVEPOINT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SAVEPOINT;
    pub const GLOBAL_CLEAR_SAVEPOINT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CLEAR_SAVEPOINT;
    pub const GLOBAL_CHECK_SAVEPOINT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CHECK_SAVEPOINT;
    pub const GLOBAL_SELPOINT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SELPOINT;
    pub const GLOBAL_CLEAR_SELPOINT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CLEAR_SELPOINT;
    pub const GLOBAL_STACK_SELPOINT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_STACK_SELPOINT;
    pub const GLOBAL_DROP_SELPOINT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_DROP_SELPOINT;
    pub const GLOBAL_CHECK_SELPOINT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CHECK_SELPOINT;
    pub const GLOBAL_TIMEWAIT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_TIMEWAIT;
    pub const GLOBAL_TIMEWAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_TIMEWAIT_KEY;
    pub const GLOBAL_FRAME: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_FRAME;
    pub const GLOBAL_DISP: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_DISP;
    pub const GLOBAL_WIPE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_WIPE;
    pub const GLOBAL_MASK_WIPE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_MASK_WIPE;
    pub const GLOBAL_WIPE_ALL: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_WIPE_ALL;
    pub const GLOBAL_MASK_WIPE_ALL: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_MASK_WIPE_ALL;
    pub const GLOBAL_WIPE_END: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_WIPE_END;
    pub const GLOBAL_WAIT_WIPE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_WAIT_WIPE;
    pub const GLOBAL_CHECK_WIPE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CHECK_WIPE;
    pub const GLOBAL_CAPTURE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CAPTURE;
    pub const GLOBAL_CAPTURE_FROM_FILE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CAPTURE_FROM_FILE;
    pub const GLOBAL_CAPTURE_FREE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CAPTURE_FREE;
    pub const GLOBAL_CAPTURE_FOR_OBJECT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CAPTURE_FOR_OBJECT;
    pub const GLOBAL_CAPTURE_FOR_OBJECT_FREE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CAPTURE_FOR_OBJECT_FREE;
    pub const GLOBAL_CAPTURE_FOR_LOCAL_SAVE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CAPTURE_FOR_LOCAL_SAVE;
    pub const GLOBAL_CAPTURE_FOR_TWEET: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CAPTURE_FOR_TWEET;
    pub const GLOBAL_CAPTURE_FREE_FOR_TWEET: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CAPTURE_FREE_FOR_TWEET;
    pub const GLOBAL_MESSAGE_BOX: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_MESSAGE_BOX;
    pub const GLOBAL_SET_MWND: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SET_MWND;
    pub const GLOBAL_GET_MWND: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_GET_MWND;
    pub const GLOBAL_SET_SEL_MWND: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SET_SEL_MWND;
    pub const GLOBAL_GET_SEL_MWND: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_GET_SEL_MWND;
    pub const GLOBAL_SET_WAKU: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SET_WAKU;
    pub const GLOBAL_OPEN: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_OPEN;
    pub const GLOBAL_OPEN_WAIT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_OPEN_WAIT;
    pub const GLOBAL_OPEN_NOWAIT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_OPEN_NOWAIT;
    pub const GLOBAL_CLOSE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CLOSE;
    pub const GLOBAL_CLOSE_WAIT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CLOSE_WAIT;
    pub const GLOBAL_CLOSE_NOWAIT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CLOSE_NOWAIT;
    pub const GLOBAL_END_CLOSE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_END_CLOSE;
    pub const GLOBAL_MSG_BLOCK: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_MSG_BLOCK;
    pub const GLOBAL_MSG_PP_BLOCK: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_MSG_PP_BLOCK;
    pub const GLOBAL_CLEAR: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CLEAR;
    pub const GLOBAL_PRINT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_PRINT;
    pub const GLOBAL_RUBY: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_RUBY;
    pub const GLOBAL_MSGBTN: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_MSGBTN;
    pub const GLOBAL_WAIT_MSG: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_WAIT_MSG;
    pub const GLOBAL_PP: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_PP;
    pub const GLOBAL_R: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_R;
    pub const GLOBAL_PAGE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_PAGE;
    pub const GLOBAL_NL: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_NL;
    pub const GLOBAL_NLI: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_NLI;
    pub const GLOBAL_INDENT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_INDENT;
    pub const GLOBAL_CLEAR_INDENT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CLEAR_INDENT;
    pub const GLOBAL_REP_POS: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_REP_POS;
    pub const GLOBAL_SIZE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SIZE;
    pub const GLOBAL_COLOR: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_COLOR;
    pub const GLOBAL_MULTI_MSG: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_MULTI_MSG;
    pub const GLOBAL_NEXT_MSG: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_NEXT_MSG;
    pub const GLOBAL_START_SLIDE_MSG: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_START_SLIDE_MSG;
    pub const GLOBAL_END_SLIDE_MSG: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_END_SLIDE_MSG;
    pub const GLOBAL_SEL: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SEL;
    pub const GLOBAL_SEL_CANCEL: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SEL_CANCEL;
    pub const GLOBAL_SELMSG: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SELMSG;
    pub const GLOBAL_SELMSG_CANCEL: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SELMSG_CANCEL;
    pub const GLOBAL_SELBTN: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SELBTN;
    pub const GLOBAL_SELBTN_READY: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SELBTN_READY;
    pub const GLOBAL_SELBTN_CANCEL: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SELBTN_CANCEL;
    pub const GLOBAL_SELBTN_CANCEL_READY: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SELBTN_CANCEL_READY;
    pub const GLOBAL_SELBTN_START: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SELBTN_START;
    pub const GLOBAL_SEL_IMAGE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SEL_IMAGE;
    pub const GLOBAL_GET_LAST_SEL_MSG: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_GET_LAST_SEL_MSG;
    pub const GLOBAL_KOE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_KOE;
    pub const GLOBAL_KOE_PLAY_WAIT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_KOE_PLAY_WAIT;
    pub const GLOBAL_KOE_PLAY_WAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_KOE_PLAY_WAIT_KEY;
    pub const GLOBAL_KOE_STOP: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_KOE_STOP;
    pub const GLOBAL_KOE_WAIT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_KOE_WAIT;
    pub const GLOBAL_KOE_WAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_KOE_WAIT_KEY;
    pub const GLOBAL_KOE_CHECK: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_KOE_CHECK;
    pub const GLOBAL_KOE_CHECK_GET_KOE_NO: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_KOE_CHECK_GET_KOE_NO;
    pub const GLOBAL_KOE_CHECK_GET_CHARA_NO: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_KOE_CHECK_GET_CHARA_NO;
    pub const GLOBAL_KOE_CHECK_IS_EX_KOE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_KOE_CHECK_IS_EX_KOE;
    pub const GLOBAL_KOE_SET_VOLUME: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_KOE_SET_VOLUME;
    pub const GLOBAL_KOE_SET_VOLUME_MAX: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_KOE_SET_VOLUME_MAX;
    pub const GLOBAL_KOE_SET_VOLUME_MIN: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_KOE_SET_VOLUME_MIN;
    pub const GLOBAL_KOE_GET_VOLUME: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_KOE_GET_VOLUME;
    pub const GLOBAL_EXKOE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_EXKOE;
    pub const GLOBAL_EXKOE_PLAY_WAIT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_EXKOE_PLAY_WAIT;
    pub const GLOBAL_EXKOE_PLAY_WAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_EXKOE_PLAY_WAIT_KEY;
    pub const GLOBAL_CLEAR_FACE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CLEAR_FACE;
    pub const GLOBAL_SET_FACE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SET_FACE;
    pub const GLOBAL_SET_NAMAE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SET_NAMAE;
    pub const GLOBAL_CLEAR_MSGBK: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CLEAR_MSGBK;
    pub const GLOBAL_INSERT_MSGBK_IMG: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_INSERT_MSGBK_IMG;
    pub const GLOBAL_A: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_A;
    pub const GLOBAL_B: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_B;
    pub const GLOBAL_C: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_C;
    pub const GLOBAL_D: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_D;
    pub const GLOBAL_E: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_E;
    pub const GLOBAL_F: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_F;
    pub const GLOBAL_G: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_G;
    pub const GLOBAL_Z: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_Z;
    pub const GLOBAL_S: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_S;
    pub const GLOBAL_M: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_M;
    pub const GLOBAL_X: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_X;
    pub const GLOBAL_NAMAE_LOCAL: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_NAMAE_LOCAL;
    pub const GLOBAL_NAMAE_GLOBAL: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_NAMAE_GLOBAL;
    pub const GLOBAL_NAMAE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_NAMAE;
    pub const GLOBAL_MATH: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_MATH;
    pub const GLOBAL_FILE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_FILE;
    pub const GLOBAL_DATABASE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_DATABASE;
    pub const GLOBAL_COUNTER: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_COUNTER;
    pub const GLOBAL_G00BUF: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_G00BUF;
    pub const GLOBAL_MASK: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_MASK;
    pub const GLOBAL_STAGE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_STAGE;
    pub const GLOBAL_BACK: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_BACK;
    pub const GLOBAL_FRONT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_FRONT;
    pub const GLOBAL_NEXT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_NEXT;
    pub const GLOBAL_MSGBK: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_MSGBK;
    pub const GLOBAL_BGM: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_BGM;
    pub const GLOBAL_KOE_ST: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_KOE_ST;
    pub const GLOBAL_PCM: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_PCM;
    pub const GLOBAL_PCMCH: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_PCMCH;
    pub const GLOBAL_PCMEVENT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_PCMEVENT;
    pub const GLOBAL_SE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SE;
    pub const GLOBAL_MOV: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_MOV;
    pub const GLOBAL_INPUT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_INPUT;
    pub const GLOBAL_MOUSE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_MOUSE;
    pub const GLOBAL_KEY: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_KEY;
    pub const GLOBAL_SCREEN: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SCREEN;
    pub const GLOBAL_FRAME_ACTION: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_FRAME_ACTION;
    pub const GLOBAL_FRAME_ACTION_CH: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_FRAME_ACTION_CH;
    pub const GLOBAL_EDITBOX: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_EDITBOX;
    pub const GLOBAL_SCRIPT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SCRIPT;
    pub const GLOBAL_SYSCOM: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SYSCOM;
    pub const GLOBAL_SYSCOM_MENU: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SYSCOM_MENU;
    pub const GLOBAL_MWND_BTN: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_MWND_BTN;
    pub const GLOBAL_CGTABLE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CGTABLE;
    pub const GLOBAL_BGMTABLE: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_BGMTABLE;
    pub const GLOBAL_SYSTEM: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SYSTEM;
    pub const GLOBAL_CALL: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CALL;
    pub const GLOBAL_CUR_CALL: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_CUR_CALL;
    pub const GLOBAL_EXCALL: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_EXCALL;
    pub const GLOBAL_INIT_CALL_STACK: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_INIT_CALL_STACK;
    pub const GLOBAL_GET_CALL_STACK_CNT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_GET_CALL_STACK_CNT;
    pub const GLOBAL_SET_CALL_STACK_CNT: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_SET_CALL_STACK_CNT;
    pub const GLOBAL_DEL_CALL_STACK: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_DEL_CALL_STACK;
    pub const GLOBAL_STEAM: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_STEAM;
    pub const CALL_L: i32 = crate::runtime::forms::codes::elm_value::CALL_L;
    pub const CALL_K: i32 = crate::runtime::forms::codes::elm_value::CALL_K;
    pub const CALLLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::CALLLIST_ARRAY;
    pub const INTLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::INTLIST_ARRAY;
    pub const INTLIST_INIT: i32 = crate::runtime::forms::codes::elm_value::INTLIST_INIT;
    pub const INTLIST_RESIZE: i32 = crate::runtime::forms::codes::elm_value::INTLIST_RESIZE;
    pub const INTLIST_GET_SIZE: i32 = crate::runtime::forms::codes::elm_value::INTLIST_GET_SIZE;
    pub const INTLIST_SETS: i32 = crate::runtime::forms::codes::elm_value::INTLIST_SETS;
    pub const INTLIST_CLEAR: i32 = crate::runtime::forms::codes::elm_value::INTLIST_CLEAR;
    pub const INTLIST_BIT: i32 = crate::runtime::forms::codes::elm_value::INTLIST_BIT;
    pub const INTLIST_BIT2: i32 = crate::runtime::forms::codes::elm_value::INTLIST_BIT2;
    pub const INTLIST_BIT4: i32 = crate::runtime::forms::codes::elm_value::INTLIST_BIT4;
    pub const INTLIST_BIT8: i32 = crate::runtime::forms::codes::elm_value::INTLIST_BIT8;
    pub const INTLIST_BIT16: i32 = crate::runtime::forms::codes::elm_value::INTLIST_BIT16;
    pub const INTLISTREF_ARRAY: i32 = crate::runtime::forms::codes::elm_value::INTLISTREF_ARRAY;
    pub const INTLISTREF_RESIZE: i32 = crate::runtime::forms::codes::elm_value::INTLISTREF_RESIZE;
    pub const INTLISTREF_GET_SIZE: i32 = crate::runtime::forms::codes::elm_value::INTLISTREF_GET_SIZE;
    pub const INTLISTREF_SETS: i32 = crate::runtime::forms::codes::elm_value::INTLISTREF_SETS;
    pub const INTLISTREF_CLEAR: i32 = crate::runtime::forms::codes::elm_value::INTLISTREF_CLEAR;
    pub const INTLISTREF_BIT: i32 = crate::runtime::forms::codes::elm_value::INTLISTREF_BIT;
    pub const INTLISTREF_BIT2: i32 = crate::runtime::forms::codes::elm_value::INTLISTREF_BIT2;
    pub const INTLISTREF_BIT4: i32 = crate::runtime::forms::codes::elm_value::INTLISTREF_BIT4;
    pub const INTLISTREF_BIT8: i32 = crate::runtime::forms::codes::elm_value::INTLISTREF_BIT8;
    pub const INTLISTREF_BIT16: i32 = crate::runtime::forms::codes::elm_value::INTLISTREF_BIT16;
    pub const INTEVENT_SET: i32 = crate::runtime::forms::codes::elm_value::INTEVENT_SET;
    pub const INTEVENT_SET_REAL: i32 = crate::runtime::forms::codes::elm_value::INTEVENT_SET_REAL;
    pub const INTEVENT_LOOP: i32 = crate::runtime::forms::codes::elm_value::INTEVENT_LOOP;
    pub const INTEVENT_LOOP_REAL: i32 = crate::runtime::forms::codes::elm_value::INTEVENT_LOOP_REAL;
    pub const INTEVENT_TURN: i32 = crate::runtime::forms::codes::elm_value::INTEVENT_TURN;
    pub const INTEVENT_TURN_REAL: i32 = crate::runtime::forms::codes::elm_value::INTEVENT_TURN_REAL;
    pub const INTEVENT_YURE: i32 = crate::runtime::forms::codes::elm_value::INTEVENT_YURE;
    pub const INTEVENT_YURE_REAL: i32 = crate::runtime::forms::codes::elm_value::INTEVENT_YURE_REAL;
    pub const INTEVENT_END: i32 = crate::runtime::forms::codes::elm_value::INTEVENT_END;
    pub const INTEVENT_WAIT: i32 = crate::runtime::forms::codes::elm_value::INTEVENT_WAIT;
    pub const INTEVENT_WAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::INTEVENT_WAIT_KEY;
    pub const INTEVENT_CHECK: i32 = crate::runtime::forms::codes::elm_value::INTEVENT_CHECK;
    pub const INTEVENT_GET_EVENT_VALUE: i32 = crate::runtime::forms::codes::elm_value::INTEVENT_GET_EVENT_VALUE;
    pub const INTEVENT___SET: i32 = crate::runtime::forms::codes::elm_value::INTEVENT___SET;
    pub const INTEVENTLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::INTEVENTLIST_ARRAY;
    pub const INTEVENTLIST_RESIZE: i32 = crate::runtime::forms::codes::elm_value::INTEVENTLIST_RESIZE;
    pub const ALLEVENT_END: i32 = crate::runtime::forms::codes::elm_value::ALLEVENT_END;
    pub const ALLEVENT_WAIT: i32 = crate::runtime::forms::codes::elm_value::ALLEVENT_WAIT;
    pub const ALLEVENT_CHECK: i32 = crate::runtime::forms::codes::elm_value::ALLEVENT_CHECK;
    pub const STR_UPPER: i32 = crate::runtime::forms::codes::elm_value::STR_UPPER;
    pub const STR_LOWER: i32 = crate::runtime::forms::codes::elm_value::STR_LOWER;
    pub const STR_LEN: i32 = crate::runtime::forms::codes::elm_value::STR_LEN;
    pub const STR_CNT: i32 = crate::runtime::forms::codes::elm_value::STR_CNT;
    pub const STR_LEFT: i32 = crate::runtime::forms::codes::elm_value::STR_LEFT;
    pub const STR_LEFT_LEN: i32 = crate::runtime::forms::codes::elm_value::STR_LEFT_LEN;
    pub const STR_MID: i32 = crate::runtime::forms::codes::elm_value::STR_MID;
    pub const STR_MID_LEN: i32 = crate::runtime::forms::codes::elm_value::STR_MID_LEN;
    pub const STR_RIGHT: i32 = crate::runtime::forms::codes::elm_value::STR_RIGHT;
    pub const STR_RIGHT_LEN: i32 = crate::runtime::forms::codes::elm_value::STR_RIGHT_LEN;
    pub const STR_SEARCH: i32 = crate::runtime::forms::codes::elm_value::STR_SEARCH;
    pub const STR_SEARCH_LAST: i32 = crate::runtime::forms::codes::elm_value::STR_SEARCH_LAST;
    pub const STR_GET_CODE: i32 = crate::runtime::forms::codes::elm_value::STR_GET_CODE;
    pub const STR_TONUM: i32 = crate::runtime::forms::codes::elm_value::STR_TONUM;
    pub const STRLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::STRLIST_ARRAY;
    pub const STRLIST_INIT: i32 = crate::runtime::forms::codes::elm_value::STRLIST_INIT;
    pub const STRLIST_RESIZE: i32 = crate::runtime::forms::codes::elm_value::STRLIST_RESIZE;
    pub const STRLIST_GET_SIZE: i32 = crate::runtime::forms::codes::elm_value::STRLIST_GET_SIZE;
    pub const STRLIST_SETS: i32 = crate::runtime::forms::codes::elm_value::STRLIST_SETS;
    pub const MATH_MAX: i32 = crate::runtime::forms::codes::elm_value::MATH_MAX;
    pub const MATH_MIN: i32 = crate::runtime::forms::codes::elm_value::MATH_MIN;
    pub const MATH_LIMIT: i32 = crate::runtime::forms::codes::elm_value::MATH_LIMIT;
    pub const MATH_ABS: i32 = crate::runtime::forms::codes::elm_value::MATH_ABS;
    pub const MATH_RAND: i32 = crate::runtime::forms::codes::elm_value::MATH_RAND;
    pub const MATH_SQRT: i32 = crate::runtime::forms::codes::elm_value::MATH_SQRT;
    pub const MATH_LOG: i32 = crate::runtime::forms::codes::elm_value::MATH_LOG;
    pub const MATH_LOG2: i32 = crate::runtime::forms::codes::elm_value::MATH_LOG2;
    pub const MATH_LOG10: i32 = crate::runtime::forms::codes::elm_value::MATH_LOG10;
    pub const MATH_SIN: i32 = crate::runtime::forms::codes::elm_value::MATH_SIN;
    pub const MATH_COS: i32 = crate::runtime::forms::codes::elm_value::MATH_COS;
    pub const MATH_TAN: i32 = crate::runtime::forms::codes::elm_value::MATH_TAN;
    pub const MATH_ARCSIN: i32 = crate::runtime::forms::codes::elm_value::MATH_ARCSIN;
    pub const MATH_ARCCOS: i32 = crate::runtime::forms::codes::elm_value::MATH_ARCCOS;
    pub const MATH_ARCTAN: i32 = crate::runtime::forms::codes::elm_value::MATH_ARCTAN;
    pub const MATH_DISTANCE: i32 = crate::runtime::forms::codes::elm_value::MATH_DISTANCE;
    pub const MATH_ANGLE: i32 = crate::runtime::forms::codes::elm_value::MATH_ANGLE;
    pub const MATH_LINEAR: i32 = crate::runtime::forms::codes::elm_value::MATH_LINEAR;
    pub const MATH_TIMETABLE: i32 = crate::runtime::forms::codes::elm_value::MATH_TIMETABLE;
    pub const MATH_TOSTR: i32 = crate::runtime::forms::codes::elm_value::MATH_TOSTR;
    pub const MATH_TOSTR_ZERO: i32 = crate::runtime::forms::codes::elm_value::MATH_TOSTR_ZERO;
    pub const MATH_TOSTR_ZEN: i32 = crate::runtime::forms::codes::elm_value::MATH_TOSTR_ZEN;
    pub const MATH_TOSTR_ZEN_ZERO: i32 = crate::runtime::forms::codes::elm_value::MATH_TOSTR_ZEN_ZERO;
    pub const MATH_TOSTR_BY_CODE: i32 = crate::runtime::forms::codes::elm_value::MATH_TOSTR_BY_CODE;
    pub const FILE_LOAD_TXT: i32 = crate::runtime::forms::codes::elm_value::FILE_LOAD_TXT;
    pub const FILE_PRELOAD_OMV: i32 = crate::runtime::forms::codes::elm_value::FILE_PRELOAD_OMV;
    pub const CGTABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::CGTABLE_FLAG;
    pub const CGTABLE_SET_DISABLE: i32 = crate::runtime::forms::codes::elm_value::CGTABLE_SET_DISABLE;
    pub const CGTABLE_SET_ENABLE: i32 = crate::runtime::forms::codes::elm_value::CGTABLE_SET_ENABLE;
    pub const CGTABLE_SET_ALL_FLAG: i32 = crate::runtime::forms::codes::elm_value::CGTABLE_SET_ALL_FLAG;
    pub const CGTABLE_GET_FLAG_NO_BY_NAME: i32 = crate::runtime::forms::codes::elm_value::CGTABLE_GET_FLAG_NO_BY_NAME;
    pub const CGTABLE_GET_NAME_BY_FLAG_NO: i32 = crate::runtime::forms::codes::elm_value::CGTABLE_GET_NAME_BY_FLAG_NO;
    pub const CGTABLE_SET_LOOK_BY_NAME: i32 = crate::runtime::forms::codes::elm_value::CGTABLE_SET_LOOK_BY_NAME;
    pub const CGTABLE_GET_LOOK_BY_NAME: i32 = crate::runtime::forms::codes::elm_value::CGTABLE_GET_LOOK_BY_NAME;
    pub const CGTABLE_GET_CG_CNT: i32 = crate::runtime::forms::codes::elm_value::CGTABLE_GET_CG_CNT;
    pub const CGTABLE_GET_LOOK_CNT: i32 = crate::runtime::forms::codes::elm_value::CGTABLE_GET_LOOK_CNT;
    pub const CGTABLE_GET_LOOK_PERCENT: i32 = crate::runtime::forms::codes::elm_value::CGTABLE_GET_LOOK_PERCENT;
    pub const BGMTABLE_GET_BGM_CNT: i32 = crate::runtime::forms::codes::elm_value::BGMTABLE_GET_BGM_CNT;
    pub const BGMTABLE_SET_ALL_FLAG: i32 = crate::runtime::forms::codes::elm_value::BGMTABLE_SET_ALL_FLAG;
    pub const BGMTABLE_SET_LISTEN_BY_NAME: i32 = crate::runtime::forms::codes::elm_value::BGMTABLE_SET_LISTEN_BY_NAME;
    pub const BGMTABLE_GET_LISTEN_BY_NAME: i32 = crate::runtime::forms::codes::elm_value::BGMTABLE_GET_LISTEN_BY_NAME;
    pub const DATABASE_GET_NUM: i32 = crate::runtime::forms::codes::elm_value::DATABASE_GET_NUM;
    pub const DATABASE_GET_STR: i32 = crate::runtime::forms::codes::elm_value::DATABASE_GET_STR;
    pub const DATABASE_GET_DATA: i32 = crate::runtime::forms::codes::elm_value::DATABASE_GET_DATA;
    pub const DATABASE_CHECK_ITEM: i32 = crate::runtime::forms::codes::elm_value::DATABASE_CHECK_ITEM;
    pub const DATABASE_CHECK_COLUMN: i32 = crate::runtime::forms::codes::elm_value::DATABASE_CHECK_COLUMN;
    pub const DATABASE_FIND_NUM: i32 = crate::runtime::forms::codes::elm_value::DATABASE_FIND_NUM;
    pub const DATABASE_FIND_STR: i32 = crate::runtime::forms::codes::elm_value::DATABASE_FIND_STR;
    pub const DATABASE_FIND_STR_REAL: i32 = crate::runtime::forms::codes::elm_value::DATABASE_FIND_STR_REAL;
    pub const DATABASELIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::DATABASELIST_ARRAY;
    pub const DATABASELIST_GET_SIZE: i32 = crate::runtime::forms::codes::elm_value::DATABASELIST_GET_SIZE;
    pub const G00BUF_LOAD: i32 = crate::runtime::forms::codes::elm_value::G00BUF_LOAD;
    pub const G00BUF_FREE: i32 = crate::runtime::forms::codes::elm_value::G00BUF_FREE;
    pub const G00BUFLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::G00BUFLIST_ARRAY;
    pub const G00BUFLIST_GET_SIZE: i32 = crate::runtime::forms::codes::elm_value::G00BUFLIST_GET_SIZE;
    pub const G00BUFLIST_FREE_ALL: i32 = crate::runtime::forms::codes::elm_value::G00BUFLIST_FREE_ALL;
    pub const MASK_INIT: i32 = crate::runtime::forms::codes::elm_value::MASK_INIT;
    pub const MASK_CREATE: i32 = crate::runtime::forms::codes::elm_value::MASK_CREATE;
    pub const MASK_X: i32 = crate::runtime::forms::codes::elm_value::MASK_X;
    pub const MASK_Y: i32 = crate::runtime::forms::codes::elm_value::MASK_Y;
    pub const MASK_X_EVE: i32 = crate::runtime::forms::codes::elm_value::MASK_X_EVE;
    pub const MASK_Y_EVE: i32 = crate::runtime::forms::codes::elm_value::MASK_Y_EVE;
    pub const MASKLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::MASKLIST_ARRAY;
    pub const MASKLIST_GET_SIZE: i32 = crate::runtime::forms::codes::elm_value::MASKLIST_GET_SIZE;
    pub const COUNTER_SET: i32 = crate::runtime::forms::codes::elm_value::COUNTER_SET;
    pub const COUNTER_GET: i32 = crate::runtime::forms::codes::elm_value::COUNTER_GET;
    pub const COUNTER_RESET: i32 = crate::runtime::forms::codes::elm_value::COUNTER_RESET;
    pub const COUNTER_START: i32 = crate::runtime::forms::codes::elm_value::COUNTER_START;
    pub const COUNTER_START_REAL: i32 = crate::runtime::forms::codes::elm_value::COUNTER_START_REAL;
    pub const COUNTER_START_FRAME: i32 = crate::runtime::forms::codes::elm_value::COUNTER_START_FRAME;
    pub const COUNTER_START_FRAME_REAL: i32 = crate::runtime::forms::codes::elm_value::COUNTER_START_FRAME_REAL;
    pub const COUNTER_START_FRAME_LOOP: i32 = crate::runtime::forms::codes::elm_value::COUNTER_START_FRAME_LOOP;
    pub const COUNTER_START_FRAME_LOOP_REAL: i32 = crate::runtime::forms::codes::elm_value::COUNTER_START_FRAME_LOOP_REAL;
    pub const COUNTER_STOP: i32 = crate::runtime::forms::codes::elm_value::COUNTER_STOP;
    pub const COUNTER_RESUME: i32 = crate::runtime::forms::codes::elm_value::COUNTER_RESUME;
    pub const COUNTER_WAIT: i32 = crate::runtime::forms::codes::elm_value::COUNTER_WAIT;
    pub const COUNTER_WAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::COUNTER_WAIT_KEY;
    pub const COUNTER_CHECK_VALUE: i32 = crate::runtime::forms::codes::elm_value::COUNTER_CHECK_VALUE;
    pub const COUNTER_CHECK_ACTIVE: i32 = crate::runtime::forms::codes::elm_value::COUNTER_CHECK_ACTIVE;
    pub const COUNTERLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::COUNTERLIST_ARRAY;
    pub const COUNTERLIST_GET_SIZE: i32 = crate::runtime::forms::codes::elm_value::COUNTERLIST_GET_SIZE;
    pub const FRAMEACTION_START: i32 = crate::runtime::forms::codes::elm_value::FRAMEACTION_START;
    pub const FRAMEACTION_START_REAL: i32 = crate::runtime::forms::codes::elm_value::FRAMEACTION_START_REAL;
    pub const FRAMEACTION_END: i32 = crate::runtime::forms::codes::elm_value::FRAMEACTION_END;
    pub const FRAMEACTION_COUNTER: i32 = crate::runtime::forms::codes::elm_value::FRAMEACTION_COUNTER;
    pub const FRAMEACTION_IS_END_ACTION: i32 = crate::runtime::forms::codes::elm_value::FRAMEACTION_IS_END_ACTION;
    pub const FRAMEACTIONLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::FRAMEACTIONLIST_ARRAY;
    pub const FRAMEACTIONLIST_RESIZE: i32 = crate::runtime::forms::codes::elm_value::FRAMEACTIONLIST_RESIZE;
    pub const FRAMEACTIONLIST_GET_SIZE: i32 = crate::runtime::forms::codes::elm_value::FRAMEACTIONLIST_GET_SIZE;
    pub const WORLD_CAMERA_EYE_X: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_EYE_X;
    pub const WORLD_CAMERA_EYE_X_EVE: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_EYE_X_EVE;
    pub const WORLD_CAMERA_EYE_Y: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_EYE_Y;
    pub const WORLD_CAMERA_EYE_Y_EVE: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_EYE_Y_EVE;
    pub const WORLD_CAMERA_EYE_Z: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_EYE_Z;
    pub const WORLD_CAMERA_EYE_Z_EVE: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_EYE_Z_EVE;
    pub const WORLD_CAMERA_PINT_X: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_PINT_X;
    pub const WORLD_CAMERA_PINT_X_EVE: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_PINT_X_EVE;
    pub const WORLD_CAMERA_PINT_Y: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_PINT_Y;
    pub const WORLD_CAMERA_PINT_Y_EVE: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_PINT_Y_EVE;
    pub const WORLD_CAMERA_PINT_Z: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_PINT_Z;
    pub const WORLD_CAMERA_PINT_Z_EVE: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_PINT_Z_EVE;
    pub const WORLD_CAMERA_UP_X: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_UP_X;
    pub const WORLD_CAMERA_UP_X_EVE: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_UP_X_EVE;
    pub const WORLD_CAMERA_UP_Y: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_UP_Y;
    pub const WORLD_CAMERA_UP_Y_EVE: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_UP_Y_EVE;
    pub const WORLD_CAMERA_UP_Z: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_UP_Z;
    pub const WORLD_CAMERA_UP_Z_EVE: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_UP_Z_EVE;
    pub const WORLD_CAMERA_VIEW_ANGLE: i32 = crate::runtime::forms::codes::elm_value::WORLD_CAMERA_VIEW_ANGLE;
    pub const WORLD_MONO: i32 = crate::runtime::forms::codes::elm_value::WORLD_MONO;
    pub const WORLD_INIT: i32 = crate::runtime::forms::codes::elm_value::WORLD_INIT;
    pub const WORLD_GET_NO: i32 = crate::runtime::forms::codes::elm_value::WORLD_GET_NO;
    pub const WORLD_SET_CAMERA_EYE: i32 = crate::runtime::forms::codes::elm_value::WORLD_SET_CAMERA_EYE;
    pub const WORLD_CALC_CAMERA_EYE: i32 = crate::runtime::forms::codes::elm_value::WORLD_CALC_CAMERA_EYE;
    pub const WORLD_SET_CAMERA_PINT: i32 = crate::runtime::forms::codes::elm_value::WORLD_SET_CAMERA_PINT;
    pub const WORLD_CALC_CAMERA_PINT: i32 = crate::runtime::forms::codes::elm_value::WORLD_CALC_CAMERA_PINT;
    pub const WORLD_SET_CAMERA_UP: i32 = crate::runtime::forms::codes::elm_value::WORLD_SET_CAMERA_UP;
    pub const WORLD_SET_CAMERA_EVE_XZ_ROTATE: i32 = crate::runtime::forms::codes::elm_value::WORLD_SET_CAMERA_EVE_XZ_ROTATE;
    pub const WORLD_ORDER: i32 = crate::runtime::forms::codes::elm_value::WORLD_ORDER;
    pub const WORLD_LAYER: i32 = crate::runtime::forms::codes::elm_value::WORLD_LAYER;
    pub const WORLD_WIPE_COPY: i32 = crate::runtime::forms::codes::elm_value::WORLD_WIPE_COPY;
    pub const WORLD_WIPE_ERASE: i32 = crate::runtime::forms::codes::elm_value::WORLD_WIPE_ERASE;
    pub const WORLDLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::WORLDLIST_ARRAY;
    pub const WORLDLIST_CREATE_WORLD: i32 = crate::runtime::forms::codes::elm_value::WORLDLIST_CREATE_WORLD;
    pub const WORLDLIST_DESTROY_WORLD: i32 = crate::runtime::forms::codes::elm_value::WORLDLIST_DESTROY_WORLD;
    pub const STAGE_CREATE_OBJECT: i32 = crate::runtime::forms::codes::elm_value::STAGE_CREATE_OBJECT;
    pub const STAGE_CREATE_MWND: i32 = crate::runtime::forms::codes::elm_value::STAGE_CREATE_MWND;
    pub const STAGE_OBJECT: i32 = crate::runtime::forms::codes::elm_value::STAGE_OBJECT;
    pub const STAGE_OBJBTNGROUP: i32 = crate::runtime::forms::codes::elm_value::STAGE_OBJBTNGROUP;
    pub const STAGE_MWND: i32 = crate::runtime::forms::codes::elm_value::STAGE_MWND;
    pub const STAGE_BTNSELITEM: i32 = crate::runtime::forms::codes::elm_value::STAGE_BTNSELITEM;
    pub const STAGE_EFFECT: i32 = crate::runtime::forms::codes::elm_value::STAGE_EFFECT;
    pub const STAGE_QUAKE: i32 = crate::runtime::forms::codes::elm_value::STAGE_QUAKE;
    pub const STAGE_WORLD: i32 = crate::runtime::forms::codes::elm_value::STAGE_WORLD;
    pub const STAGELIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::STAGELIST_ARRAY;
    pub const OBJECT___IAPP_DUMMY: i32 = crate::runtime::forms::codes::elm_value::OBJECT___IAPP_DUMMY;
    pub const OBJECT_GET_ELEMENT_NAME: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_ELEMENT_NAME;
    pub const OBJECT_DISP: i32 = crate::runtime::forms::codes::elm_value::OBJECT_DISP;
    pub const OBJECT_PATNO: i32 = crate::runtime::forms::codes::elm_value::OBJECT_PATNO;
    pub const OBJECT_WORLD: i32 = crate::runtime::forms::codes::elm_value::OBJECT_WORLD;
    pub const OBJECT_ORDER: i32 = crate::runtime::forms::codes::elm_value::OBJECT_ORDER;
    pub const OBJECT_LAYER: i32 = crate::runtime::forms::codes::elm_value::OBJECT_LAYER;
    pub const OBJECT_X: i32 = crate::runtime::forms::codes::elm_value::OBJECT_X;
    pub const OBJECT_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECT_Y;
    pub const OBJECT_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECT_Z;
    pub const OBJECT_X_REP: i32 = crate::runtime::forms::codes::elm_value::OBJECT_X_REP;
    pub const OBJECT_Y_REP: i32 = crate::runtime::forms::codes::elm_value::OBJECT_Y_REP;
    pub const OBJECT_Z_REP: i32 = crate::runtime::forms::codes::elm_value::OBJECT_Z_REP;
    pub const OBJECT_CENTER_X: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CENTER_X;
    pub const OBJECT_CENTER_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CENTER_Y;
    pub const OBJECT_CENTER_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CENTER_Z;
    pub const OBJECT_CENTER_REP_X: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CENTER_REP_X;
    pub const OBJECT_CENTER_REP_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CENTER_REP_Y;
    pub const OBJECT_CENTER_REP_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CENTER_REP_Z;
    pub const OBJECT_SCALE_X: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SCALE_X;
    pub const OBJECT_SCALE_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SCALE_Y;
    pub const OBJECT_SCALE_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SCALE_Z;
    pub const OBJECT_ROTATE_X: i32 = crate::runtime::forms::codes::elm_value::OBJECT_ROTATE_X;
    pub const OBJECT_ROTATE_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECT_ROTATE_Y;
    pub const OBJECT_ROTATE_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECT_ROTATE_Z;
    pub const OBJECT_CLIP_USE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CLIP_USE;
    pub const OBJECT_CLIP_LEFT: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CLIP_LEFT;
    pub const OBJECT_CLIP_TOP: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CLIP_TOP;
    pub const OBJECT_CLIP_RIGHT: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CLIP_RIGHT;
    pub const OBJECT_CLIP_BOTTOM: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CLIP_BOTTOM;
    pub const OBJECT_SRC_CLIP_USE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SRC_CLIP_USE;
    pub const OBJECT_SRC_CLIP_LEFT: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SRC_CLIP_LEFT;
    pub const OBJECT_SRC_CLIP_TOP: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SRC_CLIP_TOP;
    pub const OBJECT_SRC_CLIP_RIGHT: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SRC_CLIP_RIGHT;
    pub const OBJECT_SRC_CLIP_BOTTOM: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SRC_CLIP_BOTTOM;
    pub const OBJECT_TR: i32 = crate::runtime::forms::codes::elm_value::OBJECT_TR;
    pub const OBJECT_TR_REP: i32 = crate::runtime::forms::codes::elm_value::OBJECT_TR_REP;
    pub const OBJECT_MONO: i32 = crate::runtime::forms::codes::elm_value::OBJECT_MONO;
    pub const OBJECT_REVERSE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_REVERSE;
    pub const OBJECT_BRIGHT: i32 = crate::runtime::forms::codes::elm_value::OBJECT_BRIGHT;
    pub const OBJECT_DARK: i32 = crate::runtime::forms::codes::elm_value::OBJECT_DARK;
    pub const OBJECT_COLOR_R: i32 = crate::runtime::forms::codes::elm_value::OBJECT_COLOR_R;
    pub const OBJECT_COLOR_G: i32 = crate::runtime::forms::codes::elm_value::OBJECT_COLOR_G;
    pub const OBJECT_COLOR_B: i32 = crate::runtime::forms::codes::elm_value::OBJECT_COLOR_B;
    pub const OBJECT_COLOR_RATE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_COLOR_RATE;
    pub const OBJECT_COLOR_ADD_R: i32 = crate::runtime::forms::codes::elm_value::OBJECT_COLOR_ADD_R;
    pub const OBJECT_COLOR_ADD_G: i32 = crate::runtime::forms::codes::elm_value::OBJECT_COLOR_ADD_G;
    pub const OBJECT_COLOR_ADD_B: i32 = crate::runtime::forms::codes::elm_value::OBJECT_COLOR_ADD_B;
    pub const OBJECT_TONECURVE_NO: i32 = crate::runtime::forms::codes::elm_value::OBJECT_TONECURVE_NO;
    pub const OBJECT_MASK_NO: i32 = crate::runtime::forms::codes::elm_value::OBJECT_MASK_NO;
    pub const OBJECT_FOG_USE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_FOG_USE;
    pub const OBJECT_LIGHT_NO: i32 = crate::runtime::forms::codes::elm_value::OBJECT_LIGHT_NO;
    pub const OBJECT_CULLING: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CULLING;
    pub const OBJECT_ALPHA_TEST: i32 = crate::runtime::forms::codes::elm_value::OBJECT_ALPHA_TEST;
    pub const OBJECT_ALPHA_BLEND: i32 = crate::runtime::forms::codes::elm_value::OBJECT_ALPHA_BLEND;
    pub const OBJECT_BLEND: i32 = crate::runtime::forms::codes::elm_value::OBJECT_BLEND;
    pub const OBJECT_WIPE_COPY: i32 = crate::runtime::forms::codes::elm_value::OBJECT_WIPE_COPY;
    pub const OBJECT_WIPE_ERASE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_WIPE_ERASE;
    pub const OBJECT_CLICK_DISABLE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CLICK_DISABLE;
    pub const OBJECT_PATNO_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_PATNO_EVE;
    pub const OBJECT_X_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_X_EVE;
    pub const OBJECT_Y_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_Y_EVE;
    pub const OBJECT_Z_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_Z_EVE;
    pub const OBJECT_X_REP_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_X_REP_EVE;
    pub const OBJECT_Y_REP_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_Y_REP_EVE;
    pub const OBJECT_Z_REP_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_Z_REP_EVE;
    pub const OBJECT_CENTER_X_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CENTER_X_EVE;
    pub const OBJECT_CENTER_Y_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CENTER_Y_EVE;
    pub const OBJECT_CENTER_Z_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CENTER_Z_EVE;
    pub const OBJECT_CENTER_REP_X_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CENTER_REP_X_EVE;
    pub const OBJECT_CENTER_REP_Y_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CENTER_REP_Y_EVE;
    pub const OBJECT_CENTER_REP_Z_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CENTER_REP_Z_EVE;
    pub const OBJECT_SCALE_X_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SCALE_X_EVE;
    pub const OBJECT_SCALE_Y_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SCALE_Y_EVE;
    pub const OBJECT_SCALE_Z_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SCALE_Z_EVE;
    pub const OBJECT_ROTATE_X_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_ROTATE_X_EVE;
    pub const OBJECT_ROTATE_Y_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_ROTATE_Y_EVE;
    pub const OBJECT_ROTATE_Z_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_ROTATE_Z_EVE;
    pub const OBJECT_TR_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_TR_EVE;
    pub const OBJECT_TR_REP_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_TR_REP_EVE;
    pub const OBJECT_CLIP_LEFT_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CLIP_LEFT_EVE;
    pub const OBJECT_CLIP_TOP_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CLIP_TOP_EVE;
    pub const OBJECT_CLIP_RIGHT_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CLIP_RIGHT_EVE;
    pub const OBJECT_CLIP_BOTTOM_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CLIP_BOTTOM_EVE;
    pub const OBJECT_SRC_CLIP_LEFT_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SRC_CLIP_LEFT_EVE;
    pub const OBJECT_SRC_CLIP_TOP_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SRC_CLIP_TOP_EVE;
    pub const OBJECT_SRC_CLIP_RIGHT_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SRC_CLIP_RIGHT_EVE;
    pub const OBJECT_SRC_CLIP_BOTTOM_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SRC_CLIP_BOTTOM_EVE;
    pub const OBJECT_MONO_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_MONO_EVE;
    pub const OBJECT_REVERSE_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_REVERSE_EVE;
    pub const OBJECT_BRIGHT_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_BRIGHT_EVE;
    pub const OBJECT_DARK_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_DARK_EVE;
    pub const OBJECT_COLOR_R_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_COLOR_R_EVE;
    pub const OBJECT_COLOR_G_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_COLOR_G_EVE;
    pub const OBJECT_COLOR_B_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_COLOR_B_EVE;
    pub const OBJECT_COLOR_RATE_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_COLOR_RATE_EVE;
    pub const OBJECT_COLOR_ADD_R_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_COLOR_ADD_R_EVE;
    pub const OBJECT_COLOR_ADD_G_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_COLOR_ADD_G_EVE;
    pub const OBJECT_COLOR_ADD_B_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_COLOR_ADD_B_EVE;
    pub const OBJECT_ALL_EVE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_ALL_EVE;
    pub const OBJECT_FRAME_ACTION: i32 = crate::runtime::forms::codes::elm_value::OBJECT_FRAME_ACTION;
    pub const OBJECT_FRAME_ACTION_CH: i32 = crate::runtime::forms::codes::elm_value::OBJECT_FRAME_ACTION_CH;
    pub const OBJECT_CHILD: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CHILD;
    pub const OBJECT_F: i32 = crate::runtime::forms::codes::elm_value::OBJECT_F;
    pub const OBJECT_INIT: i32 = crate::runtime::forms::codes::elm_value::OBJECT_INIT;
    pub const OBJECT_INIT_PARAM: i32 = crate::runtime::forms::codes::elm_value::OBJECT_INIT_PARAM;
    pub const OBJECT_FREE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_FREE;
    pub const OBJECT_CREATE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE;
    pub const OBJECT_CREATE_RECT: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE_RECT;
    pub const OBJECT_CREATE_STRING: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE_STRING;
    pub const OBJECT_CREATE_NUMBER: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE_NUMBER;
    pub const OBJECT_CREATE_WEATHER: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE_WEATHER;
    pub const OBJECT_CREATE_SAVE_THUMB: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE_SAVE_THUMB;
    pub const OBJECT_CREATE_CAPTURE_THUMB: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE_CAPTURE_THUMB;
    pub const OBJECT_CREATE_CAPTURE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE_CAPTURE;
    pub const OBJECT_CREATE_FROM_CAPTURE_FILE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE_FROM_CAPTURE_FILE;
    pub const OBJECT_CREATE_MOVIE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE_MOVIE;
    pub const OBJECT_CREATE_MOVIE_LOOP: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE_MOVIE_LOOP;
    pub const OBJECT_CREATE_MOVIE_WAIT: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE_MOVIE_WAIT;
    pub const OBJECT_CREATE_MOVIE_WAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE_MOVIE_WAIT_KEY;
    pub const OBJECT_CREATE_EMOTE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE_EMOTE;
    pub const OBJECT_CREATE_MESH: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE_MESH;
    pub const OBJECT_CREATE_BILLBOARD: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE_BILLBOARD;
    pub const OBJECT_CREATE_COPY_FROM: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CREATE_COPY_FROM;
    pub const OBJECT_CHANGE_FILE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CHANGE_FILE;
    pub const OBJECT_EXIST_TYPE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_EXIST_TYPE;
    pub const OBJECT_SET_POS: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_POS;
    pub const OBJECT_SET_SCALE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_SCALE;
    pub const OBJECT_SET_ROTATE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_ROTATE;
    pub const OBJECT_SET_CENTER: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_CENTER;
    pub const OBJECT_SET_CENTER_REP: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_CENTER_REP;
    pub const OBJECT_SET_CLIP: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_CLIP;
    pub const OBJECT_SET_SRC_CLIP: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_SRC_CLIP;
    pub const OBJECT_GET_TYPE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_TYPE;
    pub const OBJECT_GET_SIZE_X: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_SIZE_X;
    pub const OBJECT_GET_SIZE_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_SIZE_Y;
    pub const OBJECT_GET_SIZE_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_SIZE_Z;
    pub const OBJECT_GET_PIXEL_COLOR_R: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_PIXEL_COLOR_R;
    pub const OBJECT_GET_PIXEL_COLOR_G: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_PIXEL_COLOR_G;
    pub const OBJECT_GET_PIXEL_COLOR_B: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_PIXEL_COLOR_B;
    pub const OBJECT_GET_PIXEL_COLOR_A: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_PIXEL_COLOR_A;
    pub const OBJECT_GET_FILE_NAME: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_FILE_NAME;
    pub const OBJECT_SET_STRING: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_STRING;
    pub const OBJECT_GET_STRING: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_STRING;
    pub const OBJECT_SET_STRING_PARAM: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_STRING_PARAM;
    pub const OBJECT_SET_NUMBER: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_NUMBER;
    pub const OBJECT_GET_NUMBER: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_NUMBER;
    pub const OBJECT_SET_NUMBER_PARAM: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_NUMBER_PARAM;
    pub const OBJECT_SET_WEATHER_PARAM_TYPE_A: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_WEATHER_PARAM_TYPE_A;
    pub const OBJECT_SET_WEATHER_PARAM_TYPE_B: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_WEATHER_PARAM_TYPE_B;
    pub const OBJECT_PAUSE_MOVIE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_PAUSE_MOVIE;
    pub const OBJECT_RESUME_MOVIE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_RESUME_MOVIE;
    pub const OBJECT_SEEK_MOVIE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SEEK_MOVIE;
    pub const OBJECT_GET_MOVIE_SEEK_TIME: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_MOVIE_SEEK_TIME;
    pub const OBJECT_CHECK_MOVIE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CHECK_MOVIE;
    pub const OBJECT_WAIT_MOVIE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_WAIT_MOVIE;
    pub const OBJECT_WAIT_MOVIE_KEY: i32 = crate::runtime::forms::codes::elm_value::OBJECT_WAIT_MOVIE_KEY;
    pub const OBJECT_END_MOVIE_LOOP: i32 = crate::runtime::forms::codes::elm_value::OBJECT_END_MOVIE_LOOP;
    pub const OBJECT_SET_MOVIE_AUTO_FREE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_MOVIE_AUTO_FREE;
    pub const OBJECT_CLEAR_BUTTON: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CLEAR_BUTTON;
    pub const OBJECT_SET_BUTTON: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_BUTTON;
    pub const OBJECT_SET_BUTTON_GROUP: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_BUTTON_GROUP;
    pub const OBJECT_SET_BUTTON_STATE_NORMAL: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_BUTTON_STATE_NORMAL;
    pub const OBJECT_SET_BUTTON_STATE_SELECT: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_BUTTON_STATE_SELECT;
    pub const OBJECT_SET_BUTTON_STATE_DISABLE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_BUTTON_STATE_DISABLE;
    pub const OBJECT_GET_BUTTON_STATE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_BUTTON_STATE;
    pub const OBJECT_GET_BUTTON_HIT_STATE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_BUTTON_HIT_STATE;
    pub const OBJECT_GET_BUTTON_REAL_STATE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_BUTTON_REAL_STATE;
    pub const OBJECT_SET_BUTTON_PUSHKEEP: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_BUTTON_PUSHKEEP;
    pub const OBJECT_GET_BUTTON_PUSHKEEP: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_BUTTON_PUSHKEEP;
    pub const OBJECT_SET_BUTTON_ALPHA_TEST: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_BUTTON_ALPHA_TEST;
    pub const OBJECT_GET_BUTTON_ALPHA_TEST: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_BUTTON_ALPHA_TEST;
    pub const OBJECT_CLEAR_BUTTON_CALL: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CLEAR_BUTTON_CALL;
    pub const OBJECT_SET_BUTTON_CALL: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_BUTTON_CALL;
    pub const OBJECT_LOAD_GAN: i32 = crate::runtime::forms::codes::ELM_OBJECT_LOAD_GAN;
    pub const OBJECT_START_GAN: i32 = crate::runtime::forms::codes::ELM_OBJECT_START_GAN;
    pub const OBJECT_ADD_HINTS: i32 = crate::runtime::forms::codes::elm_value::OBJECT_ADD_HINTS;
    pub const OBJECT_CLEAR_HINTS: i32 = crate::runtime::forms::codes::elm_value::OBJECT_CLEAR_HINTS;
    pub const OBJECT_SET_CHILD_SORT_TYPE_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_CHILD_SORT_TYPE_DEFAULT;
    pub const OBJECT_SET_CHILD_SORT_TYPE_TEST: i32 = crate::runtime::forms::codes::elm_value::OBJECT_SET_CHILD_SORT_TYPE_TEST;
    pub const OBJECT_GET_PAT_CNT: i32 = crate::runtime::forms::codes::elm_value::OBJECT_GET_PAT_CNT;
    pub const OBJECT_EMOTE_PLAY_TIMELINE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_EMOTE_PLAY_TIMELINE;
    pub const OBJECT_EMOTE_STOP_TIMELINE: i32 = crate::runtime::forms::codes::elm_value::OBJECT_EMOTE_STOP_TIMELINE;
    pub const OBJECT_EMOTE_CHECK_PLAYING: i32 = crate::runtime::forms::codes::elm_value::OBJECT_EMOTE_CHECK_PLAYING;
    pub const OBJECT_EMOTE_WAIT_PLAYING: i32 = crate::runtime::forms::codes::elm_value::OBJECT_EMOTE_WAIT_PLAYING;
    pub const OBJECT_EMOTE_WAIT_PLAYING_KEY: i32 = crate::runtime::forms::codes::elm_value::OBJECT_EMOTE_WAIT_PLAYING_KEY;
    pub const OBJECT_EMOTE_SKIP: i32 = crate::runtime::forms::codes::elm_value::OBJECT_EMOTE_SKIP;
    pub const OBJECT_EMOTE_PASS: i32 = crate::runtime::forms::codes::elm_value::OBJECT_EMOTE_PASS;
    pub const OBJECT_EMOTE_MOUTH_VOLUME: i32 = crate::runtime::forms::codes::elm_value::OBJECT_EMOTE_MOUTH_VOLUME;
    pub const OBJECT_EMOTE_KOE_CHARA_NO: i32 = crate::runtime::forms::codes::elm_value::OBJECT_EMOTE_KOE_CHARA_NO;
    pub const OBJECTLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::OBJECTLIST_ARRAY;
    pub const OBJECTLIST_RESIZE: i32 = crate::runtime::forms::codes::elm_value::OBJECTLIST_RESIZE;
    pub const OBJECTLIST_GET_SIZE: i32 = crate::runtime::forms::codes::elm_value::OBJECTLIST_GET_SIZE;
    pub const OBJECTEVENT_SET_X: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_SET_X;
    pub const OBJECTEVENT_SET_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_SET_Y;
    pub const OBJECTEVENT_SET_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_SET_Z;
    pub const OBJECTEVENT_SET_SCALE_X: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_SET_SCALE_X;
    pub const OBJECTEVENT_SET_SCALE_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_SET_SCALE_Y;
    pub const OBJECTEVENT_SET_SCALE_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_SET_SCALE_Z;
    pub const OBJECTEVENT_SET_ROTATE_X: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_SET_ROTATE_X;
    pub const OBJECTEVENT_SET_ROTATE_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_SET_ROTATE_Y;
    pub const OBJECTEVENT_SET_ROTATE_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_SET_ROTATE_Z;
    pub const OBJECTEVENT_SET_TR: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_SET_TR;
    pub const OBJECTEVENT_LOOP_X: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_LOOP_X;
    pub const OBJECTEVENT_LOOP_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_LOOP_Y;
    pub const OBJECTEVENT_LOOP_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_LOOP_Z;
    pub const OBJECTEVENT_LOOP_TR: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_LOOP_TR;
    pub const OBJECTEVENT_TURN_X: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_TURN_X;
    pub const OBJECTEVENT_TURN_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_TURN_Y;
    pub const OBJECTEVENT_TURN_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_TURN_Z;
    pub const OBJECTEVENT_TURN_TR: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_TURN_TR;
    pub const OBJECTEVENT_STOP_X: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_STOP_X;
    pub const OBJECTEVENT_STOP_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_STOP_Y;
    pub const OBJECTEVENT_STOP_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_STOP_Z;
    pub const OBJECTEVENT_STOP_SCALE_X: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_STOP_SCALE_X;
    pub const OBJECTEVENT_STOP_SCALE_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_STOP_SCALE_Y;
    pub const OBJECTEVENT_STOP_SCALE_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_STOP_SCALE_Z;
    pub const OBJECTEVENT_STOP_ROTATE_X: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_STOP_ROTATE_X;
    pub const OBJECTEVENT_STOP_ROTATE_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_STOP_ROTATE_Y;
    pub const OBJECTEVENT_STOP_ROTATE_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_STOP_ROTATE_Z;
    pub const OBJECTEVENT_STOP_TR: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_STOP_TR;
    pub const OBJECTEVENT_STOP_ALL: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_STOP_ALL;
    pub const OBJECTEVENT_WAIT_X: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_WAIT_X;
    pub const OBJECTEVENT_WAIT_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_WAIT_Y;
    pub const OBJECTEVENT_WAIT_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_WAIT_Z;
    pub const OBJECTEVENT_WAIT_SCALE_X: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_WAIT_SCALE_X;
    pub const OBJECTEVENT_WAIT_SCALE_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_WAIT_SCALE_Y;
    pub const OBJECTEVENT_WAIT_SCALE_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_WAIT_SCALE_Z;
    pub const OBJECTEVENT_WAIT_ROTATE_X: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_WAIT_ROTATE_X;
    pub const OBJECTEVENT_WAIT_ROTATE_Y: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_WAIT_ROTATE_Y;
    pub const OBJECTEVENT_WAIT_ROTATE_Z: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_WAIT_ROTATE_Z;
    pub const OBJECTEVENT_WAIT_TR: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_WAIT_TR;
    pub const OBJECTEVENT_WAIT_ALL: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENT_WAIT_ALL;
    pub const OBJECTEVENTLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::OBJECTEVENTLIST_ARRAY;
    pub const MWND_SET_WAKU: i32 = crate::runtime::forms::codes::elm_value::MWND_SET_WAKU;
    pub const MWND_OPEN: i32 = crate::runtime::forms::codes::elm_value::MWND_OPEN;
    pub const MWND_OPEN_WAIT: i32 = crate::runtime::forms::codes::elm_value::MWND_OPEN_WAIT;
    pub const MWND_OPEN_NOWAIT: i32 = crate::runtime::forms::codes::elm_value::MWND_OPEN_NOWAIT;
    pub const MWND_CLOSE: i32 = crate::runtime::forms::codes::elm_value::MWND_CLOSE;
    pub const MWND_CLOSE_WAIT: i32 = crate::runtime::forms::codes::elm_value::MWND_CLOSE_WAIT;
    pub const MWND_CLOSE_NOWAIT: i32 = crate::runtime::forms::codes::elm_value::MWND_CLOSE_NOWAIT;
    pub const MWND_CHECK_OPEN: i32 = crate::runtime::forms::codes::elm_value::MWND_CHECK_OPEN;
    pub const MWND_END_CLOSE: i32 = crate::runtime::forms::codes::elm_value::MWND_END_CLOSE;
    pub const MWND_MSG_BLOCK: i32 = crate::runtime::forms::codes::elm_value::MWND_MSG_BLOCK;
    pub const MWND_MSG_PP_BLOCK: i32 = crate::runtime::forms::codes::elm_value::MWND_MSG_PP_BLOCK;
    pub const MWND_CLEAR: i32 = crate::runtime::forms::codes::elm_value::MWND_CLEAR;
    pub const MWND____NOVEL_CLEAR: i32 = crate::runtime::forms::codes::elm_value::MWND____NOVEL_CLEAR;
    pub const MWND_SET_NAMAE: i32 = crate::runtime::forms::codes::elm_value::MWND_SET_NAMAE;
    pub const MWND_NAMAE: i32 = crate::runtime::forms::codes::elm_value::MWND_NAMAE;
    pub const MWND____OVER_FLOW_NAMAE: i32 = crate::runtime::forms::codes::elm_value::MWND____OVER_FLOW_NAMAE;
    pub const MWND_PRINT: i32 = crate::runtime::forms::codes::elm_value::MWND_PRINT;
    pub const MWND____OVER_FLOW_PRINT: i32 = crate::runtime::forms::codes::elm_value::MWND____OVER_FLOW_PRINT;
    pub const MWND_RUBY: i32 = crate::runtime::forms::codes::elm_value::MWND_RUBY;
    pub const MWND_WAIT_MSG: i32 = crate::runtime::forms::codes::elm_value::MWND_WAIT_MSG;
    pub const MWND_PP: i32 = crate::runtime::forms::codes::elm_value::MWND_PP;
    pub const MWND_R: i32 = crate::runtime::forms::codes::elm_value::MWND_R;
    pub const MWND_PAGE: i32 = crate::runtime::forms::codes::elm_value::MWND_PAGE;
    pub const MWND_SEL: i32 = crate::runtime::forms::codes::elm_value::MWND_SEL;
    pub const MWND_SEL_CANCEL: i32 = crate::runtime::forms::codes::elm_value::MWND_SEL_CANCEL;
    pub const MWND_SELMSG: i32 = crate::runtime::forms::codes::elm_value::MWND_SELMSG;
    pub const MWND_SELMSG_CANCEL: i32 = crate::runtime::forms::codes::elm_value::MWND_SELMSG_CANCEL;
    pub const MWND_NL: i32 = crate::runtime::forms::codes::elm_value::MWND_NL;
    pub const MWND_NLI: i32 = crate::runtime::forms::codes::elm_value::MWND_NLI;
    pub const MWND_INDENT: i32 = crate::runtime::forms::codes::elm_value::MWND_INDENT;
    pub const MWND_CLEAR_INDENT: i32 = crate::runtime::forms::codes::elm_value::MWND_CLEAR_INDENT;
    pub const MWND_SIZE: i32 = crate::runtime::forms::codes::elm_value::MWND_SIZE;
    pub const MWND_REP_POS: i32 = crate::runtime::forms::codes::elm_value::MWND_REP_POS;
    pub const MWND_COLOR: i32 = crate::runtime::forms::codes::elm_value::MWND_COLOR;
    pub const MWND_MULTI_MSG: i32 = crate::runtime::forms::codes::elm_value::MWND_MULTI_MSG;
    pub const MWND_NEXT_MSG: i32 = crate::runtime::forms::codes::elm_value::MWND_NEXT_MSG;
    pub const MWND_START_SLIDE_MSG: i32 = crate::runtime::forms::codes::elm_value::MWND_START_SLIDE_MSG;
    pub const MWND____SLIDE_MSG: i32 = crate::runtime::forms::codes::elm_value::MWND____SLIDE_MSG;
    pub const MWND_END_SLIDE_MSG: i32 = crate::runtime::forms::codes::elm_value::MWND_END_SLIDE_MSG;
    pub const MWND_MSGBTN: i32 = crate::runtime::forms::codes::elm_value::MWND_MSGBTN;
    pub const MWND_KOE: i32 = crate::runtime::forms::codes::elm_value::MWND_KOE;
    pub const MWND_KOE_PLAY_WAIT: i32 = crate::runtime::forms::codes::elm_value::MWND_KOE_PLAY_WAIT;
    pub const MWND_KOE_PLAY_WAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::MWND_KOE_PLAY_WAIT_KEY;
    pub const MWND_EXKOE: i32 = crate::runtime::forms::codes::elm_value::MWND_EXKOE;
    pub const MWND_EXKOE_PLAY_WAIT: i32 = crate::runtime::forms::codes::elm_value::MWND_EXKOE_PLAY_WAIT;
    pub const MWND_EXKOE_PLAY_WAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::MWND_EXKOE_PLAY_WAIT_KEY;
    pub const MWND_CLEAR_FACE: i32 = crate::runtime::forms::codes::elm_value::MWND_CLEAR_FACE;
    pub const MWND_SET_FACE: i32 = crate::runtime::forms::codes::elm_value::MWND_SET_FACE;
    pub const MWND_WORLD: i32 = crate::runtime::forms::codes::elm_value::MWND_WORLD;
    pub const MWND_LAYER: i32 = crate::runtime::forms::codes::elm_value::MWND_LAYER;
    pub const MWND_OBJECT: i32 = crate::runtime::forms::codes::elm_value::MWND_OBJECT;
    pub const MWND_BUTTON: i32 = crate::runtime::forms::codes::elm_value::MWND_BUTTON;
    pub const MWND_FACE: i32 = crate::runtime::forms::codes::elm_value::MWND_FACE;
    pub const MWND_INIT_WINDOW_POS: i32 = crate::runtime::forms::codes::elm_value::MWND_INIT_WINDOW_POS;
    pub const MWND_INIT_WINDOW_SIZE: i32 = crate::runtime::forms::codes::elm_value::MWND_INIT_WINDOW_SIZE;
    pub const MWND_INIT_WINDOW_MOJI_CNT: i32 = crate::runtime::forms::codes::elm_value::MWND_INIT_WINDOW_MOJI_CNT;
    pub const MWND_INIT_WAKU_FILE: i32 = crate::runtime::forms::codes::elm_value::MWND_INIT_WAKU_FILE;
    pub const MWND_INIT_FILTER_FILE: i32 = crate::runtime::forms::codes::elm_value::MWND_INIT_FILTER_FILE;
    pub const MWND_INIT_OPEN_ANIME_TYPE: i32 = crate::runtime::forms::codes::elm_value::MWND_INIT_OPEN_ANIME_TYPE;
    pub const MWND_INIT_OPEN_ANIME_TIME: i32 = crate::runtime::forms::codes::elm_value::MWND_INIT_OPEN_ANIME_TIME;
    pub const MWND_INIT_CLOSE_ANIME_TYPE: i32 = crate::runtime::forms::codes::elm_value::MWND_INIT_CLOSE_ANIME_TYPE;
    pub const MWND_INIT_CLOSE_ANIME_TIME: i32 = crate::runtime::forms::codes::elm_value::MWND_INIT_CLOSE_ANIME_TIME;
    pub const MWND_SET_WINDOW_POS: i32 = crate::runtime::forms::codes::elm_value::MWND_SET_WINDOW_POS;
    pub const MWND_SET_WINDOW_SIZE: i32 = crate::runtime::forms::codes::elm_value::MWND_SET_WINDOW_SIZE;
    pub const MWND_SET_WINDOW_MOJI_CNT: i32 = crate::runtime::forms::codes::elm_value::MWND_SET_WINDOW_MOJI_CNT;
    pub const MWND_SET_WAKU_FILE: i32 = crate::runtime::forms::codes::elm_value::MWND_SET_WAKU_FILE;
    pub const MWND_SET_FILTER_FILE: i32 = crate::runtime::forms::codes::elm_value::MWND_SET_FILTER_FILE;
    pub const MWND_SET_OPEN_ANIME_TYPE: i32 = crate::runtime::forms::codes::elm_value::MWND_SET_OPEN_ANIME_TYPE;
    pub const MWND_SET_OPEN_ANIME_TIME: i32 = crate::runtime::forms::codes::elm_value::MWND_SET_OPEN_ANIME_TIME;
    pub const MWND_SET_CLOSE_ANIME_TYPE: i32 = crate::runtime::forms::codes::elm_value::MWND_SET_CLOSE_ANIME_TYPE;
    pub const MWND_SET_CLOSE_ANIME_TIME: i32 = crate::runtime::forms::codes::elm_value::MWND_SET_CLOSE_ANIME_TIME;
    pub const MWND_GET_WINDOW_POS_X: i32 = crate::runtime::forms::codes::elm_value::MWND_GET_WINDOW_POS_X;
    pub const MWND_GET_WINDOW_POS_Y: i32 = crate::runtime::forms::codes::elm_value::MWND_GET_WINDOW_POS_Y;
    pub const MWND_GET_WINDOW_SIZE_X: i32 = crate::runtime::forms::codes::elm_value::MWND_GET_WINDOW_SIZE_X;
    pub const MWND_GET_WINDOW_SIZE_Y: i32 = crate::runtime::forms::codes::elm_value::MWND_GET_WINDOW_SIZE_Y;
    pub const MWND_GET_WINDOW_MOJI_CNT_X: i32 = crate::runtime::forms::codes::elm_value::MWND_GET_WINDOW_MOJI_CNT_X;
    pub const MWND_GET_WINDOW_MOJI_CNT_Y: i32 = crate::runtime::forms::codes::elm_value::MWND_GET_WINDOW_MOJI_CNT_Y;
    pub const MWND_GET_WAKU_FILE: i32 = crate::runtime::forms::codes::elm_value::MWND_GET_WAKU_FILE;
    pub const MWND_GET_FILTER_FILE: i32 = crate::runtime::forms::codes::elm_value::MWND_GET_FILTER_FILE;
    pub const MWND_GET_OPEN_ANIME_TYPE: i32 = crate::runtime::forms::codes::elm_value::MWND_GET_OPEN_ANIME_TYPE;
    pub const MWND_GET_OPEN_ANIME_TIME: i32 = crate::runtime::forms::codes::elm_value::MWND_GET_OPEN_ANIME_TIME;
    pub const MWND_GET_CLOSE_ANIME_TYPE: i32 = crate::runtime::forms::codes::elm_value::MWND_GET_CLOSE_ANIME_TYPE;
    pub const MWND_GET_CLOSE_ANIME_TIME: i32 = crate::runtime::forms::codes::elm_value::MWND_GET_CLOSE_ANIME_TIME;
    pub const MWND_GET_DEFAULT_OPEN_ANIME_TYPE: i32 = crate::runtime::forms::codes::elm_value::MWND_GET_DEFAULT_OPEN_ANIME_TYPE;
    pub const MWND_GET_DEFAULT_OPEN_ANIME_TIME: i32 = crate::runtime::forms::codes::elm_value::MWND_GET_DEFAULT_OPEN_ANIME_TIME;
    pub const MWND_GET_DEFAULT_CLOSE_ANIME_TYPE: i32 = crate::runtime::forms::codes::elm_value::MWND_GET_DEFAULT_CLOSE_ANIME_TYPE;
    pub const MWND_GET_DEFAULT_CLOSE_ANIME_TIME: i32 = crate::runtime::forms::codes::elm_value::MWND_GET_DEFAULT_CLOSE_ANIME_TIME;
    pub const MWNDLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::MWNDLIST_ARRAY;
    pub const MWNDLIST_CLOSE: i32 = crate::runtime::forms::codes::elm_value::MWNDLIST_CLOSE;
    pub const MWNDLIST_CLOSE_WAIT: i32 = crate::runtime::forms::codes::elm_value::MWNDLIST_CLOSE_WAIT;
    pub const MWNDLIST_CLOSE_NOWAIT: i32 = crate::runtime::forms::codes::elm_value::MWNDLIST_CLOSE_NOWAIT;
    pub const GROUP_SEL: i32 = crate::runtime::forms::codes::elm_value::GROUP_SEL;
    pub const GROUP_SEL_CANCEL: i32 = crate::runtime::forms::codes::elm_value::GROUP_SEL_CANCEL;
    pub const GROUP_INIT: i32 = crate::runtime::forms::codes::elm_value::GROUP_INIT;
    pub const GROUP_START: i32 = crate::runtime::forms::codes::elm_value::GROUP_START;
    pub const GROUP_START_CANCEL: i32 = crate::runtime::forms::codes::elm_value::GROUP_START_CANCEL;
    pub const GROUP_END: i32 = crate::runtime::forms::codes::elm_value::GROUP_END;
    pub const GROUP_GET_HIT_NO: i32 = crate::runtime::forms::codes::elm_value::GROUP_GET_HIT_NO;
    pub const GROUP_GET_PUSHED_NO: i32 = crate::runtime::forms::codes::elm_value::GROUP_GET_PUSHED_NO;
    pub const GROUP_GET_DECIDED_NO: i32 = crate::runtime::forms::codes::elm_value::GROUP_GET_DECIDED_NO;
    pub const GROUP_ON_HIT_NO: i32 = crate::runtime::forms::codes::elm_value::GROUP_ON_HIT_NO;
    pub const GROUP_ON_PUSHED_NO: i32 = crate::runtime::forms::codes::elm_value::GROUP_ON_PUSHED_NO;
    pub const GROUP_ON_DECIDED_NO: i32 = crate::runtime::forms::codes::elm_value::GROUP_ON_DECIDED_NO;
    pub const GROUP_GET_RESULT: i32 = crate::runtime::forms::codes::elm_value::GROUP_GET_RESULT;
    pub const GROUP_GET_RESULT_BUTTON_NO: i32 = crate::runtime::forms::codes::elm_value::GROUP_GET_RESULT_BUTTON_NO;
    pub const GROUP_ORDER: i32 = crate::runtime::forms::codes::elm_value::GROUP_ORDER;
    pub const GROUP_LAYER: i32 = crate::runtime::forms::codes::elm_value::GROUP_LAYER;
    pub const GROUP_CANCEL_PRIORITY: i32 = crate::runtime::forms::codes::elm_value::GROUP_CANCEL_PRIORITY;
    pub const GROUPLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::GROUPLIST_ARRAY;
    pub const GROUPLIST_ALLOC: i32 = crate::runtime::forms::codes::elm_value::GROUPLIST_ALLOC;
    pub const GROUPLIST_FREE: i32 = crate::runtime::forms::codes::elm_value::GROUPLIST_FREE;
    pub const BTNSELITEM_OBJECT: i32 = crate::runtime::forms::codes::elm_value::BTNSELITEM_OBJECT;
    pub const BTNSELITEMLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::BTNSELITEMLIST_ARRAY;
    pub const BTNSELITEMLIST_ALLOC: i32 = crate::runtime::forms::codes::elm_value::BTNSELITEMLIST_ALLOC;
    pub const BTNSELITEMLIST_FREE: i32 = crate::runtime::forms::codes::elm_value::BTNSELITEMLIST_FREE;
    pub const SCREEN_INIT: i32 = crate::runtime::forms::codes::elm_value::SCREEN_INIT;
    pub const SCREEN_X: i32 = crate::runtime::forms::codes::elm_value::SCREEN_X;
    pub const SCREEN_Y: i32 = crate::runtime::forms::codes::elm_value::SCREEN_Y;
    pub const SCREEN_Z: i32 = crate::runtime::forms::codes::elm_value::SCREEN_Z;
    pub const SCREEN_MONO: i32 = crate::runtime::forms::codes::elm_value::SCREEN_MONO;
    pub const SCREEN_REVERSE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_REVERSE;
    pub const SCREEN_BRIGHT: i32 = crate::runtime::forms::codes::elm_value::SCREEN_BRIGHT;
    pub const SCREEN_DARK: i32 = crate::runtime::forms::codes::elm_value::SCREEN_DARK;
    pub const SCREEN_COLOR_R: i32 = crate::runtime::forms::codes::elm_value::SCREEN_COLOR_R;
    pub const SCREEN_COLOR_G: i32 = crate::runtime::forms::codes::elm_value::SCREEN_COLOR_G;
    pub const SCREEN_COLOR_B: i32 = crate::runtime::forms::codes::elm_value::SCREEN_COLOR_B;
    pub const SCREEN_COLOR_RATE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_COLOR_RATE;
    pub const SCREEN_COLOR_ADD_R: i32 = crate::runtime::forms::codes::elm_value::SCREEN_COLOR_ADD_R;
    pub const SCREEN_COLOR_ADD_G: i32 = crate::runtime::forms::codes::elm_value::SCREEN_COLOR_ADD_G;
    pub const SCREEN_COLOR_ADD_B: i32 = crate::runtime::forms::codes::elm_value::SCREEN_COLOR_ADD_B;
    pub const SCREEN_X_EVE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_X_EVE;
    pub const SCREEN_Y_EVE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_Y_EVE;
    pub const SCREEN_Z_EVE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_Z_EVE;
    pub const SCREEN_MONO_EVE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_MONO_EVE;
    pub const SCREEN_REVERSE_EVE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_REVERSE_EVE;
    pub const SCREEN_BRIGHT_EVE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_BRIGHT_EVE;
    pub const SCREEN_DARK_EVE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_DARK_EVE;
    pub const SCREEN_COLOR_R_EVE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_COLOR_R_EVE;
    pub const SCREEN_COLOR_G_EVE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_COLOR_G_EVE;
    pub const SCREEN_COLOR_B_EVE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_COLOR_B_EVE;
    pub const SCREEN_COLOR_RATE_EVE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_COLOR_RATE_EVE;
    pub const SCREEN_COLOR_ADD_R_EVE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_COLOR_ADD_R_EVE;
    pub const SCREEN_COLOR_ADD_G_EVE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_COLOR_ADD_G_EVE;
    pub const SCREEN_COLOR_ADD_B_EVE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_COLOR_ADD_B_EVE;
    pub const SCREEN_EFFECT: i32 = crate::runtime::forms::codes::elm_value::SCREEN_EFFECT;
    pub const SCREEN_SHAKE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_SHAKE;
    pub const SCREEN_QUAKE: i32 = crate::runtime::forms::codes::elm_value::SCREEN_QUAKE;
    pub const QUAKE_START: i32 = crate::runtime::forms::codes::elm_value::QUAKE_START;
    pub const QUAKE_START_WAIT: i32 = crate::runtime::forms::codes::elm_value::QUAKE_START_WAIT;
    pub const QUAKE_START_WAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::QUAKE_START_WAIT_KEY;
    pub const QUAKE_START_NOWAIT: i32 = crate::runtime::forms::codes::elm_value::QUAKE_START_NOWAIT;
    pub const QUAKE_START_ALL: i32 = crate::runtime::forms::codes::elm_value::QUAKE_START_ALL;
    pub const QUAKE_START_ALL_WAIT: i32 = crate::runtime::forms::codes::elm_value::QUAKE_START_ALL_WAIT;
    pub const QUAKE_START_ALL_WAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::QUAKE_START_ALL_WAIT_KEY;
    pub const QUAKE_START_ALL_NOWAIT: i32 = crate::runtime::forms::codes::elm_value::QUAKE_START_ALL_NOWAIT;
    pub const QUAKE_END: i32 = crate::runtime::forms::codes::elm_value::QUAKE_END;
    pub const QUAKE_WAIT: i32 = crate::runtime::forms::codes::elm_value::QUAKE_WAIT;
    pub const QUAKE_WAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::QUAKE_WAIT_KEY;
    pub const QUAKE_CHECK: i32 = crate::runtime::forms::codes::elm_value::QUAKE_CHECK;
    pub const QUAKELIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::QUAKELIST_ARRAY;
    pub const EDITBOX_CREATE: i32 = crate::runtime::forms::codes::elm_value::EDITBOX_CREATE;
    pub const EDITBOX_DESTROY: i32 = crate::runtime::forms::codes::elm_value::EDITBOX_DESTROY;
    pub const EDITBOX_SET_TEXT: i32 = crate::runtime::forms::codes::elm_value::EDITBOX_SET_TEXT;
    pub const EDITBOX_GET_TEXT: i32 = crate::runtime::forms::codes::elm_value::EDITBOX_GET_TEXT;
    pub const EDITBOX_SET_FOCUS: i32 = crate::runtime::forms::codes::elm_value::EDITBOX_SET_FOCUS;
    pub const EDITBOX_CHECK_DECIDED: i32 = crate::runtime::forms::codes::elm_value::EDITBOX_CHECK_DECIDED;
    pub const EDITBOX_CHECK_CANCELED: i32 = crate::runtime::forms::codes::elm_value::EDITBOX_CHECK_CANCELED;
    pub const EDITBOX_CLEAR_INPUT: i32 = crate::runtime::forms::codes::elm_value::EDITBOX_CLEAR_INPUT;
    pub const EDITBOXLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::EDITBOXLIST_ARRAY;
    pub const EDITBOXLIST_CLEAR_INPUT: i32 = crate::runtime::forms::codes::elm_value::EDITBOXLIST_CLEAR_INPUT;
    pub const EFFECT_INIT: i32 = crate::runtime::forms::codes::elm_value::EFFECT_INIT;
    pub const EFFECT_WIPE_COPY: i32 = crate::runtime::forms::codes::elm_value::EFFECT_WIPE_COPY;
    pub const EFFECT_WIPE_ERASE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_WIPE_ERASE;
    pub const EFFECT_X: i32 = crate::runtime::forms::codes::elm_value::EFFECT_X;
    pub const EFFECT_Y: i32 = crate::runtime::forms::codes::elm_value::EFFECT_Y;
    pub const EFFECT_Z: i32 = crate::runtime::forms::codes::elm_value::EFFECT_Z;
    pub const EFFECT_MONO: i32 = crate::runtime::forms::codes::elm_value::EFFECT_MONO;
    pub const EFFECT_REVERSE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_REVERSE;
    pub const EFFECT_BRIGHT: i32 = crate::runtime::forms::codes::elm_value::EFFECT_BRIGHT;
    pub const EFFECT_DARK: i32 = crate::runtime::forms::codes::elm_value::EFFECT_DARK;
    pub const EFFECT_COLOR_R: i32 = crate::runtime::forms::codes::elm_value::EFFECT_COLOR_R;
    pub const EFFECT_COLOR_G: i32 = crate::runtime::forms::codes::elm_value::EFFECT_COLOR_G;
    pub const EFFECT_COLOR_B: i32 = crate::runtime::forms::codes::elm_value::EFFECT_COLOR_B;
    pub const EFFECT_COLOR_RATE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_COLOR_RATE;
    pub const EFFECT_COLOR_ADD_R: i32 = crate::runtime::forms::codes::elm_value::EFFECT_COLOR_ADD_R;
    pub const EFFECT_COLOR_ADD_G: i32 = crate::runtime::forms::codes::elm_value::EFFECT_COLOR_ADD_G;
    pub const EFFECT_COLOR_ADD_B: i32 = crate::runtime::forms::codes::elm_value::EFFECT_COLOR_ADD_B;
    pub const EFFECT_X_EVE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_X_EVE;
    pub const EFFECT_Y_EVE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_Y_EVE;
    pub const EFFECT_Z_EVE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_Z_EVE;
    pub const EFFECT_MONO_EVE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_MONO_EVE;
    pub const EFFECT_REVERSE_EVE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_REVERSE_EVE;
    pub const EFFECT_BRIGHT_EVE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_BRIGHT_EVE;
    pub const EFFECT_DARK_EVE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_DARK_EVE;
    pub const EFFECT_COLOR_R_EVE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_COLOR_R_EVE;
    pub const EFFECT_COLOR_G_EVE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_COLOR_G_EVE;
    pub const EFFECT_COLOR_B_EVE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_COLOR_B_EVE;
    pub const EFFECT_COLOR_RATE_EVE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_COLOR_RATE_EVE;
    pub const EFFECT_COLOR_ADD_R_EVE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_COLOR_ADD_R_EVE;
    pub const EFFECT_COLOR_ADD_G_EVE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_COLOR_ADD_G_EVE;
    pub const EFFECT_COLOR_ADD_B_EVE: i32 = crate::runtime::forms::codes::elm_value::EFFECT_COLOR_ADD_B_EVE;
    pub const EFFECT_BEGIN_ORDER: i32 = crate::runtime::forms::codes::elm_value::EFFECT_BEGIN_ORDER;
    pub const EFFECT_END_ORDER: i32 = crate::runtime::forms::codes::elm_value::EFFECT_END_ORDER;
    pub const EFFECT_BEGIN_LAYER: i32 = crate::runtime::forms::codes::elm_value::EFFECT_BEGIN_LAYER;
    pub const EFFECT_END_LAYER: i32 = crate::runtime::forms::codes::elm_value::EFFECT_END_LAYER;
    pub const EFFECTLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::EFFECTLIST_ARRAY;
    pub const EFFECTLIST_RESIZE: i32 = crate::runtime::forms::codes::elm_value::EFFECTLIST_RESIZE;
    pub const EFFECTLIST_GET_SIZE: i32 = crate::runtime::forms::codes::elm_value::EFFECTLIST_GET_SIZE;
    pub const MSGBK_INSERT_IMG: i32 = crate::runtime::forms::codes::elm_value::MSGBK_INSERT_IMG;
    pub const MSGBK_INSERT_MSG: i32 = crate::runtime::forms::codes::elm_value::MSGBK_INSERT_MSG;
    pub const MSGBK_ADD_MSG: i32 = crate::runtime::forms::codes::elm_value::MSGBK_ADD_MSG;
    pub const MSGBK_ADD_KOE: i32 = crate::runtime::forms::codes::elm_value::MSGBK_ADD_KOE;
    pub const MSGBK_ADD_NAMAE: i32 = crate::runtime::forms::codes::elm_value::MSGBK_ADD_NAMAE;
    pub const MSGBK_GO_NEXT_MSG: i32 = crate::runtime::forms::codes::elm_value::MSGBK_GO_NEXT_MSG;
    pub const BGM_PLAY: i32 = crate::runtime::forms::codes::elm_value::BGM_PLAY;
    pub const BGM_PLAY_ONESHOT: i32 = crate::runtime::forms::codes::elm_value::BGM_PLAY_ONESHOT;
    pub const BGM_PLAY_WAIT: i32 = crate::runtime::forms::codes::elm_value::BGM_PLAY_WAIT;
    pub const BGM_READY: i32 = crate::runtime::forms::codes::elm_value::BGM_READY;
    pub const BGM_READY_ONESHOT: i32 = crate::runtime::forms::codes::elm_value::BGM_READY_ONESHOT;
    pub const BGM_WAIT: i32 = crate::runtime::forms::codes::elm_value::BGM_WAIT;
    pub const BGM_WAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::BGM_WAIT_KEY;
    pub const BGM_STOP: i32 = crate::runtime::forms::codes::elm_value::BGM_STOP;
    pub const BGM_PAUSE: i32 = crate::runtime::forms::codes::elm_value::BGM_PAUSE;
    pub const BGM_RESUME: i32 = crate::runtime::forms::codes::elm_value::BGM_RESUME;
    pub const BGM_RESUME_WAIT: i32 = crate::runtime::forms::codes::elm_value::BGM_RESUME_WAIT;
    pub const BGM_WAIT_FADE: i32 = crate::runtime::forms::codes::elm_value::BGM_WAIT_FADE;
    pub const BGM_WAIT_FADE_KEY: i32 = crate::runtime::forms::codes::elm_value::BGM_WAIT_FADE_KEY;
    pub const BGM_CHECK: i32 = crate::runtime::forms::codes::elm_value::BGM_CHECK;
    pub const BGM_SET_VOLUME: i32 = crate::runtime::forms::codes::elm_value::BGM_SET_VOLUME;
    pub const BGM_SET_VOLUME_MAX: i32 = crate::runtime::forms::codes::elm_value::BGM_SET_VOLUME_MAX;
    pub const BGM_SET_VOLUME_MIN: i32 = crate::runtime::forms::codes::elm_value::BGM_SET_VOLUME_MIN;
    pub const BGM_GET_REGIST_NAME: i32 = crate::runtime::forms::codes::elm_value::BGM_GET_REGIST_NAME;
    pub const BGM_GET_VOLUME: i32 = crate::runtime::forms::codes::elm_value::BGM_GET_VOLUME;
    pub const BGM_GET_PLAY_POS: i32 = crate::runtime::forms::codes::elm_value::BGM_GET_PLAY_POS;
    pub const PCM_PLAY: i32 = crate::runtime::forms::codes::elm_value::PCM_PLAY;
    pub const PCM_STOP: i32 = crate::runtime::forms::codes::elm_value::PCM_STOP;
    pub const PCMCH_PLAY: i32 = crate::runtime::forms::codes::elm_value::PCMCH_PLAY;
    pub const PCMCH_PLAY_LOOP: i32 = crate::runtime::forms::codes::elm_value::PCMCH_PLAY_LOOP;
    pub const PCMCH_PLAY_WAIT: i32 = crate::runtime::forms::codes::elm_value::PCMCH_PLAY_WAIT;
    pub const PCMCH_READY: i32 = crate::runtime::forms::codes::elm_value::PCMCH_READY;
    pub const PCMCH_READY_LOOP: i32 = crate::runtime::forms::codes::elm_value::PCMCH_READY_LOOP;
    pub const PCMCH_STOP: i32 = crate::runtime::forms::codes::elm_value::PCMCH_STOP;
    pub const PCMCH_PAUSE: i32 = crate::runtime::forms::codes::elm_value::PCMCH_PAUSE;
    pub const PCMCH_RESUME: i32 = crate::runtime::forms::codes::elm_value::PCMCH_RESUME;
    pub const PCMCH_RESUME_WAIT: i32 = crate::runtime::forms::codes::elm_value::PCMCH_RESUME_WAIT;
    pub const PCMCH_WAIT: i32 = crate::runtime::forms::codes::elm_value::PCMCH_WAIT;
    pub const PCMCH_WAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::PCMCH_WAIT_KEY;
    pub const PCMCH_WAIT_FADE: i32 = crate::runtime::forms::codes::elm_value::PCMCH_WAIT_FADE;
    pub const PCMCH_WAIT_FADE_KEY: i32 = crate::runtime::forms::codes::elm_value::PCMCH_WAIT_FADE_KEY;
    pub const PCMCH_CHECK: i32 = crate::runtime::forms::codes::elm_value::PCMCH_CHECK;
    pub const PCMCH_GET_VOLUME: i32 = crate::runtime::forms::codes::elm_value::PCMCH_GET_VOLUME;
    pub const PCMCH_SET_VOLUME: i32 = crate::runtime::forms::codes::elm_value::PCMCH_SET_VOLUME;
    pub const PCMCH_SET_VOLUME_MAX: i32 = crate::runtime::forms::codes::elm_value::PCMCH_SET_VOLUME_MAX;
    pub const PCMCH_SET_VOLUME_MIN: i32 = crate::runtime::forms::codes::elm_value::PCMCH_SET_VOLUME_MIN;
    pub const PCMCHLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::PCMCHLIST_ARRAY;
    pub const PCMCHLIST_STOP_ALL: i32 = crate::runtime::forms::codes::elm_value::PCMCHLIST_STOP_ALL;
    pub const SE_PLAY: i32 = crate::runtime::forms::codes::elm_value::SE_PLAY;
    pub const SE_PLAY_BY_FILE_NAME: i32 = crate::runtime::forms::codes::elm_value::SE_PLAY_BY_FILE_NAME;
    pub const SE_PLAY_BY_KOE_NO: i32 = crate::runtime::forms::codes::elm_value::SE_PLAY_BY_KOE_NO;
    pub const SE_PLAY_BY_SE_NO: i32 = crate::runtime::forms::codes::elm_value::SE_PLAY_BY_SE_NO;
    pub const SE_STOP: i32 = crate::runtime::forms::codes::elm_value::SE_STOP;
    pub const SE_WAIT: i32 = crate::runtime::forms::codes::elm_value::SE_WAIT;
    pub const SE_WAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::SE_WAIT_KEY;
    pub const SE_CHECK: i32 = crate::runtime::forms::codes::elm_value::SE_CHECK;
    pub const SE_SET_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SE_SET_VOLUME;
    pub const SE_SET_VOLUME_MAX: i32 = crate::runtime::forms::codes::elm_value::SE_SET_VOLUME_MAX;
    pub const SE_SET_VOLUME_MIN: i32 = crate::runtime::forms::codes::elm_value::SE_SET_VOLUME_MIN;
    pub const SE_GET_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SE_GET_VOLUME;
    pub const MOV_PLAY: i32 = crate::runtime::forms::codes::elm_value::MOV_PLAY;
    pub const MOV_PLAY_WAIT: i32 = crate::runtime::forms::codes::elm_value::MOV_PLAY_WAIT;
    pub const MOV_PLAY_WAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::MOV_PLAY_WAIT_KEY;
    pub const MOV_STOP: i32 = crate::runtime::forms::codes::elm_value::MOV_STOP;
    pub const PCMEVENT_START_ONESHOT: i32 = crate::runtime::forms::codes::elm_value::PCMEVENT_START_ONESHOT;
    pub const PCMEVENT_START_LOOP: i32 = crate::runtime::forms::codes::elm_value::PCMEVENT_START_LOOP;
    pub const PCMEVENT_START_RANDOM: i32 = crate::runtime::forms::codes::elm_value::PCMEVENT_START_RANDOM;
    pub const PCMEVENT_STOP: i32 = crate::runtime::forms::codes::elm_value::PCMEVENT_STOP;
    pub const PCMEVENT_WAIT: i32 = crate::runtime::forms::codes::elm_value::PCMEVENT_WAIT;
    pub const PCMEVENT_WAIT_KEY: i32 = crate::runtime::forms::codes::elm_value::PCMEVENT_WAIT_KEY;
    pub const PCMEVENT_CHECK: i32 = crate::runtime::forms::codes::elm_value::PCMEVENT_CHECK;
    pub const PCMEVENTLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::PCMEVENTLIST_ARRAY;
    pub const MOUSE_CLEAR: i32 = crate::runtime::forms::codes::elm_value::MOUSE_CLEAR;
    pub const MOUSE_NEXT: i32 = crate::runtime::forms::codes::elm_value::MOUSE_NEXT;
    pub const MOUSE_POS_X: i32 = crate::runtime::forms::codes::elm_value::MOUSE_POS_X;
    pub const MOUSE_POS_Y: i32 = crate::runtime::forms::codes::elm_value::MOUSE_POS_Y;
    pub const MOUSE_GET_POS_X: i32 = crate::runtime::forms::codes::elm_value::MOUSE_GET_POS_X;
    pub const MOUSE_GET_POS_Y: i32 = crate::runtime::forms::codes::elm_value::MOUSE_GET_POS_Y;
    pub const MOUSE_GET_POS: i32 = crate::runtime::forms::codes::elm_value::MOUSE_GET_POS;
    pub const MOUSE_SET_POS: i32 = crate::runtime::forms::codes::elm_value::MOUSE_SET_POS;
    pub const MOUSE_WHEEL: i32 = crate::runtime::forms::codes::elm_value::MOUSE_WHEEL;
    pub const MOUSE_LEFT: i32 = crate::runtime::forms::codes::elm_value::MOUSE_LEFT;
    pub const MOUSE_RIGHT: i32 = crate::runtime::forms::codes::elm_value::MOUSE_RIGHT;
    pub const KEY_ON_DOWN: i32 = crate::runtime::forms::codes::elm_value::KEY_ON_DOWN;
    pub const KEY_ON_UP: i32 = crate::runtime::forms::codes::elm_value::KEY_ON_UP;
    pub const KEY_ON_DOWN_UP: i32 = crate::runtime::forms::codes::elm_value::KEY_ON_DOWN_UP;
    pub const KEY_IS_DOWN: i32 = crate::runtime::forms::codes::elm_value::KEY_IS_DOWN;
    pub const KEY_IS_UP: i32 = crate::runtime::forms::codes::elm_value::KEY_IS_UP;
    pub const KEY_ON_FLICK: i32 = crate::runtime::forms::codes::elm_value::KEY_ON_FLICK;
    pub const KEY_GET_FLICK_PIXEL: i32 = crate::runtime::forms::codes::elm_value::KEY_GET_FLICK_PIXEL;
    pub const KEY_GET_FLICK_ANGLE: i32 = crate::runtime::forms::codes::elm_value::KEY_GET_FLICK_ANGLE;
    pub const KEY_GET_FLICK_MM: i32 = crate::runtime::forms::codes::elm_value::KEY_GET_FLICK_MM;
    pub const KEYLIST_ARRAY: i32 = crate::runtime::forms::codes::elm_value::KEYLIST_ARRAY;
    pub const KEYLIST_WAIT: i32 = crate::runtime::forms::codes::elm_value::KEYLIST_WAIT;
    pub const KEYLIST_WAIT_FORCE: i32 = crate::runtime::forms::codes::elm_value::KEYLIST_WAIT_FORCE;
    pub const KEYLIST_CLEAR: i32 = crate::runtime::forms::codes::elm_value::KEYLIST_CLEAR;
    pub const KEYLIST_NEXT: i32 = crate::runtime::forms::codes::elm_value::KEYLIST_NEXT;
    pub const INPUT_CLEAR: i32 = crate::runtime::forms::codes::elm_value::INPUT_CLEAR;
    pub const INPUT_NEXT: i32 = crate::runtime::forms::codes::elm_value::INPUT_NEXT;
    pub const INPUT_DECIDE: i32 = crate::runtime::forms::codes::elm_value::INPUT_DECIDE;
    pub const INPUT_CANCEL: i32 = crate::runtime::forms::codes::elm_value::INPUT_CANCEL;
    pub const SYSCOM_SET_SYSCOM_MENU_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SYSCOM_MENU_ENABLE;
    pub const SYSCOM_SET_SYSCOM_MENU_DISABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SYSCOM_MENU_DISABLE;
    pub const SYSCOM_SET_MWND_BTN_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MWND_BTN_ENABLE;
    pub const SYSCOM_SET_MWND_BTN_DISABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MWND_BTN_DISABLE;
    pub const SYSCOM_SET_MWND_BTN_TOUCH_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MWND_BTN_TOUCH_ENABLE;
    pub const SYSCOM_SET_MWND_BTN_TOUCH_DISABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MWND_BTN_TOUCH_DISABLE;
    pub const SYSCOM_CALL_EX: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_EX;
    pub const SYSCOM_CALL_SYSCOM_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_SYSCOM_MENU;
    pub const SYSCOM_CALL_SAVE_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_SAVE_MENU;
    pub const SYSCOM_CALL_LOAD_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_LOAD_MENU;
    pub const SYSCOM_CALL_CONFIG_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_CONFIG_MENU;
    pub const SYSCOM_CALL_CONFIG_WINDOW_MODE_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_CONFIG_WINDOW_MODE_MENU;
    pub const SYSCOM_CALL_CONFIG_VOLUME_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_CONFIG_VOLUME_MENU;
    pub const SYSCOM_CALL_CONFIG_BGMFADE_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_CONFIG_BGMFADE_MENU;
    pub const SYSCOM_CALL_CONFIG_KOEMODE_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_CONFIG_KOEMODE_MENU;
    pub const SYSCOM_CALL_CONFIG_CHARAKOE_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_CONFIG_CHARAKOE_MENU;
    pub const SYSCOM_CALL_CONFIG_JITAN_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_CONFIG_JITAN_MENU;
    pub const SYSCOM_CALL_CONFIG_MESSAGE_SPEED_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_CONFIG_MESSAGE_SPEED_MENU;
    pub const SYSCOM_CALL_CONFIG_AUTO_MODE_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_CONFIG_AUTO_MODE_MENU;
    pub const SYSCOM_CALL_CONFIG_FONT_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_CONFIG_FONT_MENU;
    pub const SYSCOM_CALL_CONFIG_FILTER_COLOR_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_CONFIG_FILTER_COLOR_MENU;
    pub const SYSCOM_CALL_CONFIG_SYSTEM_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_CONFIG_SYSTEM_MENU;
    pub const SYSCOM_CALL_CONFIG_MOVIE_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_CONFIG_MOVIE_MENU;
    pub const SYSCOM_INIT_SYSCOM_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_INIT_SYSCOM_FLAG;
    pub const SYSCOM_SET_READ_SKIP_ONOFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_READ_SKIP_ONOFF_FLAG;
    pub const SYSCOM_GET_READ_SKIP_ONOFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_READ_SKIP_ONOFF_FLAG;
    pub const SYSCOM_SET_READ_SKIP_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_READ_SKIP_ENABLE_FLAG;
    pub const SYSCOM_GET_READ_SKIP_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_READ_SKIP_ENABLE_FLAG;
    pub const SYSCOM_SET_READ_SKIP_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_READ_SKIP_EXIST_FLAG;
    pub const SYSCOM_GET_READ_SKIP_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_READ_SKIP_EXIST_FLAG;
    pub const SYSCOM_CHECK_READ_SKIP_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHECK_READ_SKIP_ENABLE;
    pub const SYSCOM_SET_AUTO_SKIP_ONOFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_AUTO_SKIP_ONOFF_FLAG;
    pub const SYSCOM_GET_AUTO_SKIP_ONOFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_AUTO_SKIP_ONOFF_FLAG;
    pub const SYSCOM_SET_AUTO_SKIP_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_AUTO_SKIP_ENABLE_FLAG;
    pub const SYSCOM_GET_AUTO_SKIP_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_AUTO_SKIP_ENABLE_FLAG;
    pub const SYSCOM_SET_AUTO_SKIP_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_AUTO_SKIP_EXIST_FLAG;
    pub const SYSCOM_GET_AUTO_SKIP_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_AUTO_SKIP_EXIST_FLAG;
    pub const SYSCOM_CHECK_AUTO_SKIP_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHECK_AUTO_SKIP_ENABLE;
    pub const SYSCOM_SET_AUTO_MODE_ONOFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_AUTO_MODE_ONOFF_FLAG;
    pub const SYSCOM_GET_AUTO_MODE_ONOFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_AUTO_MODE_ONOFF_FLAG;
    pub const SYSCOM_SET_AUTO_MODE_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_AUTO_MODE_ENABLE_FLAG;
    pub const SYSCOM_GET_AUTO_MODE_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_AUTO_MODE_ENABLE_FLAG;
    pub const SYSCOM_SET_AUTO_MODE_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_AUTO_MODE_EXIST_FLAG;
    pub const SYSCOM_GET_AUTO_MODE_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_AUTO_MODE_EXIST_FLAG;
    pub const SYSCOM_CHECK_AUTO_MODE_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHECK_AUTO_MODE_ENABLE;
    pub const SYSCOM_SET_HIDE_MWND_ONOFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_HIDE_MWND_ONOFF_FLAG;
    pub const SYSCOM_GET_HIDE_MWND_ONOFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_HIDE_MWND_ONOFF_FLAG;
    pub const SYSCOM_SET_HIDE_MWND_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_HIDE_MWND_ENABLE_FLAG;
    pub const SYSCOM_GET_HIDE_MWND_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_HIDE_MWND_ENABLE_FLAG;
    pub const SYSCOM_SET_HIDE_MWND_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_HIDE_MWND_EXIST_FLAG;
    pub const SYSCOM_GET_HIDE_MWND_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_HIDE_MWND_EXIST_FLAG;
    pub const SYSCOM_CHECK_HIDE_MWND_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHECK_HIDE_MWND_ENABLE;
    pub const SYSCOM_OPEN_MSG_BACK: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_OPEN_MSG_BACK;
    pub const SYSCOM_CLOSE_MSG_BACK: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CLOSE_MSG_BACK;
    pub const SYSCOM_SET_MSG_BACK_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MSG_BACK_ENABLE_FLAG;
    pub const SYSCOM_GET_MSG_BACK_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_MSG_BACK_ENABLE_FLAG;
    pub const SYSCOM_SET_MSG_BACK_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MSG_BACK_EXIST_FLAG;
    pub const SYSCOM_GET_MSG_BACK_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_MSG_BACK_EXIST_FLAG;
    pub const SYSCOM_CHECK_MSG_BACK_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHECK_MSG_BACK_ENABLE;
    pub const SYSCOM_CHECK_MSG_BACK_OPEN: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHECK_MSG_BACK_OPEN;
    pub const SYSCOM_SET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG;
    pub const SYSCOM_GET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG;
    pub const SYSCOM_SET_LOCAL_EXTRA_SWITCH_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_LOCAL_EXTRA_SWITCH_ENABLE_FLAG;
    pub const SYSCOM_GET_LOCAL_EXTRA_SWITCH_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_LOCAL_EXTRA_SWITCH_ENABLE_FLAG;
    pub const SYSCOM_SET_LOCAL_EXTRA_SWITCH_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_LOCAL_EXTRA_SWITCH_EXIST_FLAG;
    pub const SYSCOM_GET_LOCAL_EXTRA_SWITCH_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_LOCAL_EXTRA_SWITCH_EXIST_FLAG;
    pub const SYSCOM_CHECK_LOCAL_EXTRA_SWITCH_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHECK_LOCAL_EXTRA_SWITCH_ENABLE;
    pub const SYSCOM_SET_LOCAL_EXTRA_MODE_VALUE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_LOCAL_EXTRA_MODE_VALUE;
    pub const SYSCOM_GET_LOCAL_EXTRA_MODE_VALUE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_LOCAL_EXTRA_MODE_VALUE;
    pub const SYSCOM_SET_LOCAL_EXTRA_MODE_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_LOCAL_EXTRA_MODE_ENABLE_FLAG;
    pub const SYSCOM_GET_LOCAL_EXTRA_MODE_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_LOCAL_EXTRA_MODE_ENABLE_FLAG;
    pub const SYSCOM_SET_LOCAL_EXTRA_MODE_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_LOCAL_EXTRA_MODE_EXIST_FLAG;
    pub const SYSCOM_GET_LOCAL_EXTRA_MODE_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_LOCAL_EXTRA_MODE_EXIST_FLAG;
    pub const SYSCOM_CHECK_LOCAL_EXTRA_MODE_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHECK_LOCAL_EXTRA_MODE_ENABLE;
    pub const SYSCOM_RETURN_TO_SEL: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_RETURN_TO_SEL;
    pub const SYSCOM_SET_RETURN_TO_SEL_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_RETURN_TO_SEL_ENABLE_FLAG;
    pub const SYSCOM_GET_RETURN_TO_SEL_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_RETURN_TO_SEL_ENABLE_FLAG;
    pub const SYSCOM_SET_RETURN_TO_SEL_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_RETURN_TO_SEL_EXIST_FLAG;
    pub const SYSCOM_GET_RETURN_TO_SEL_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_RETURN_TO_SEL_EXIST_FLAG;
    pub const SYSCOM_CHECK_RETURN_TO_SEL_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHECK_RETURN_TO_SEL_ENABLE;
    pub const SYSCOM_RETURN_TO_MENU: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_RETURN_TO_MENU;
    pub const SYSCOM_SET_RETURN_TO_MENU_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_RETURN_TO_MENU_ENABLE_FLAG;
    pub const SYSCOM_GET_RETURN_TO_MENU_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_RETURN_TO_MENU_ENABLE_FLAG;
    pub const SYSCOM_SET_RETURN_TO_MENU_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_RETURN_TO_MENU_EXIST_FLAG;
    pub const SYSCOM_GET_RETURN_TO_MENU_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_RETURN_TO_MENU_EXIST_FLAG;
    pub const SYSCOM_CHECK_RETURN_TO_MENU_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHECK_RETURN_TO_MENU_ENABLE;
    pub const SYSCOM_END_GAME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_END_GAME;
    pub const SYSCOM_SET_END_GAME_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_END_GAME_ENABLE_FLAG;
    pub const SYSCOM_GET_END_GAME_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_END_GAME_ENABLE_FLAG;
    pub const SYSCOM_SET_END_GAME_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_END_GAME_EXIST_FLAG;
    pub const SYSCOM_GET_END_GAME_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_END_GAME_EXIST_FLAG;
    pub const SYSCOM_CHECK_END_GAME_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHECK_END_GAME_ENABLE;
    pub const SYSCOM_GET_TOTAL_PLAY_TIME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_TOTAL_PLAY_TIME;
    pub const SYSCOM_SET_TOTAL_PLAY_TIME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_TOTAL_PLAY_TIME;
    pub const SYSCOM_REPLAY_KOE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_REPLAY_KOE;
    pub const SYSCOM_CHECK_REPLAY_KOE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHECK_REPLAY_KOE;
    pub const SYSCOM_GET_REPLAY_KOE_KOE_NO: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_REPLAY_KOE_KOE_NO;
    pub const SYSCOM_GET_REPLAY_KOE_CHARA_NO: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_REPLAY_KOE_CHARA_NO;
    pub const SYSCOM_CLEAR_REPLAY_KOE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CLEAR_REPLAY_KOE;
    pub const SYSCOM_SAVE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SAVE;
    pub const SYSCOM_QUICK_SAVE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_QUICK_SAVE;
    pub const SYSCOM_END_SAVE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_END_SAVE;
    pub const SYSCOM_SET_SAVE_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SAVE_ENABLE_FLAG;
    pub const SYSCOM_GET_SAVE_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_ENABLE_FLAG;
    pub const SYSCOM_SET_SAVE_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SAVE_EXIST_FLAG;
    pub const SYSCOM_GET_SAVE_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_EXIST_FLAG;
    pub const SYSCOM_CHECK_SAVE_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHECK_SAVE_ENABLE;
    pub const SYSCOM_LOAD: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_LOAD;
    pub const SYSCOM_QUICK_LOAD: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_QUICK_LOAD;
    pub const SYSCOM_END_LOAD: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_END_LOAD;
    pub const SYSCOM_SET_LOAD_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_LOAD_ENABLE_FLAG;
    pub const SYSCOM_GET_LOAD_ENABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_LOAD_ENABLE_FLAG;
    pub const SYSCOM_SET_LOAD_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_LOAD_EXIST_FLAG;
    pub const SYSCOM_GET_LOAD_EXIST_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_LOAD_EXIST_FLAG;
    pub const SYSCOM_CHECK_LOAD_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHECK_LOAD_ENABLE;
    pub const SYSCOM_GET_SAVE_CNT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_CNT;
    pub const SYSCOM_GET_QUICK_SAVE_CNT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_CNT;
    pub const SYSCOM_GET_SAVE_EXIST: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_EXIST;
    pub const SYSCOM_GET_QUICK_SAVE_EXIST: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_EXIST;
    pub const SYSCOM_GET_END_SAVE_EXIST: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_END_SAVE_EXIST;
    pub const SYSCOM_GET_SAVE_NEW_NO: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_NEW_NO;
    pub const SYSCOM_GET_QUICK_SAVE_NEW_NO: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_NEW_NO;
    pub const SYSCOM_GET_SAVE_YEAR: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_YEAR;
    pub const SYSCOM_GET_QUICK_SAVE_YEAR: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_YEAR;
    pub const SYSCOM_GET_SAVE_MONTH: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_MONTH;
    pub const SYSCOM_GET_QUICK_SAVE_MONTH: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_MONTH;
    pub const SYSCOM_GET_SAVE_DAY: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_DAY;
    pub const SYSCOM_GET_QUICK_SAVE_DAY: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_DAY;
    pub const SYSCOM_GET_SAVE_WEEKDAY: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_WEEKDAY;
    pub const SYSCOM_GET_QUICK_SAVE_WEEKDAY: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_WEEKDAY;
    pub const SYSCOM_GET_SAVE_HOUR: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_HOUR;
    pub const SYSCOM_GET_QUICK_SAVE_HOUR: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_HOUR;
    pub const SYSCOM_GET_SAVE_MINUTE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_MINUTE;
    pub const SYSCOM_GET_QUICK_SAVE_MINUTE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_MINUTE;
    pub const SYSCOM_GET_SAVE_SECOND: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_SECOND;
    pub const SYSCOM_GET_QUICK_SAVE_SECOND: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_SECOND;
    pub const SYSCOM_GET_SAVE_MILLISECOND: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_MILLISECOND;
    pub const SYSCOM_GET_QUICK_SAVE_MILLISECOND: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_MILLISECOND;
    pub const SYSCOM_GET_SAVE_TITLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_TITLE;
    pub const SYSCOM_GET_QUICK_SAVE_TITLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_TITLE;
    pub const SYSCOM_GET_SAVE_MESSAGE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_MESSAGE;
    pub const SYSCOM_GET_SAVE_FULL_MESSAGE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_FULL_MESSAGE;
    pub const SYSCOM_GET_QUICK_SAVE_MESSAGE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_MESSAGE;
    pub const SYSCOM_GET_QUICK_SAVE_FULL_MESSAGE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_FULL_MESSAGE;
    pub const SYSCOM_GET_SAVE_COMMENT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_COMMENT;
    pub const SYSCOM_SET_SAVE_COMMENT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SAVE_COMMENT;
    pub const SYSCOM_GET_QUICK_SAVE_COMMENT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_COMMENT;
    pub const SYSCOM_SET_QUICK_SAVE_COMMENT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_QUICK_SAVE_COMMENT;
    pub const SYSCOM_GET_SAVE_VALUE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_VALUE;
    pub const SYSCOM_GET_QUICK_SAVE_VALUE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_VALUE;
    pub const SYSCOM_SET_SAVE_VALUE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SAVE_VALUE;
    pub const SYSCOM_SET_QUICK_SAVE_VALUE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_QUICK_SAVE_VALUE;
    pub const SYSCOM_GET_SAVE_APPEND_DIR: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_APPEND_DIR;
    pub const SYSCOM_GET_QUICK_SAVE_APPEND_DIR: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_APPEND_DIR;
    pub const SYSCOM_GET_SAVE_APPEND_NAME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVE_APPEND_NAME;
    pub const SYSCOM_GET_QUICK_SAVE_APPEND_NAME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_QUICK_SAVE_APPEND_NAME;
    pub const SYSCOM_COPY_SAVE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_COPY_SAVE;
    pub const SYSCOM_COPY_QUICK_SAVE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_COPY_QUICK_SAVE;
    pub const SYSCOM_CHANGE_SAVE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHANGE_SAVE;
    pub const SYSCOM_CHANGE_QUICK_SAVE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHANGE_QUICK_SAVE;
    pub const SYSCOM_DELETE_SAVE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_DELETE_SAVE;
    pub const SYSCOM_DELETE_QUICK_SAVE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_DELETE_QUICK_SAVE;
    pub const SYSCOM_INNER_SAVE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_INNER_SAVE;
    pub const SYSCOM_INNER_LOAD: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_INNER_LOAD;
    pub const SYSCOM_CLEAR_INNER_SAVE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CLEAR_INNER_SAVE;
    pub const SYSCOM_COPY_INNER_SAVE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_COPY_INNER_SAVE;
    pub const SYSCOM_CHECK_INNER_SAVE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHECK_INNER_SAVE;
    pub const SYSCOM_MSG_BACK_LOAD: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_MSG_BACK_LOAD;
    pub const SYSCOM_GET_CURRENT_SAVE_SCENE_TITLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_CURRENT_SAVE_SCENE_TITLE;
    pub const SYSCOM_GET_CURRENT_SAVE_MESSAGE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_CURRENT_SAVE_MESSAGE;
    pub const SYSCOM_SET_WINDOW_MODE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_WINDOW_MODE;
    pub const SYSCOM_SET_WINDOW_MODE_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_WINDOW_MODE_DEFAULT;
    pub const SYSCOM_GET_WINDOW_MODE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_WINDOW_MODE;
    pub const SYSCOM_SET_WINDOW_MODE_SIZE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_WINDOW_MODE_SIZE;
    pub const SYSCOM_SET_WINDOW_MODE_SIZE_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_WINDOW_MODE_SIZE_DEFAULT;
    pub const SYSCOM_GET_WINDOW_MODE_SIZE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_WINDOW_MODE_SIZE;
    pub const SYSCOM_CHECK_WINDOW_MODE_SIZE_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CHECK_WINDOW_MODE_SIZE_ENABLE;
    pub const SYSCOM_SET_ALL_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_ALL_VOLUME;
    pub const SYSCOM_SET_ALL_VOLUME_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_ALL_VOLUME_DEFAULT;
    pub const SYSCOM_GET_ALL_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_ALL_VOLUME;
    pub const SYSCOM_SET_BGM_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_BGM_VOLUME;
    pub const SYSCOM_SET_BGM_VOLUME_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_BGM_VOLUME_DEFAULT;
    pub const SYSCOM_GET_BGM_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_BGM_VOLUME;
    pub const SYSCOM_SET_KOE_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_KOE_VOLUME;
    pub const SYSCOM_SET_KOE_VOLUME_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_KOE_VOLUME_DEFAULT;
    pub const SYSCOM_GET_KOE_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_KOE_VOLUME;
    pub const SYSCOM_SET_PCM_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_PCM_VOLUME;
    pub const SYSCOM_SET_PCM_VOLUME_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_PCM_VOLUME_DEFAULT;
    pub const SYSCOM_GET_PCM_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_PCM_VOLUME;
    pub const SYSCOM_SET_SE_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SE_VOLUME;
    pub const SYSCOM_SET_SE_VOLUME_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SE_VOLUME_DEFAULT;
    pub const SYSCOM_GET_SE_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SE_VOLUME;
    pub const SYSCOM_SET_MOV_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MOV_VOLUME;
    pub const SYSCOM_SET_MOV_VOLUME_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MOV_VOLUME_DEFAULT;
    pub const SYSCOM_GET_MOV_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_MOV_VOLUME;
    pub const SYSCOM_SET_SOUND_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SOUND_VOLUME;
    pub const SYSCOM_SET_SOUND_VOLUME_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SOUND_VOLUME_DEFAULT;
    pub const SYSCOM_GET_SOUND_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SOUND_VOLUME;
    pub const SYSCOM_SET_BGMFADE_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_BGMFADE_VOLUME;
    pub const SYSCOM_SET_BGMFADE_VOLUME_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_BGMFADE_VOLUME_DEFAULT;
    pub const SYSCOM_GET_BGMFADE_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_BGMFADE_VOLUME;
    pub const SYSCOM_SET_ALL_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_ALL_ONOFF;
    pub const SYSCOM_SET_ALL_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_ALL_ONOFF_DEFAULT;
    pub const SYSCOM_GET_ALL_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_ALL_ONOFF;
    pub const SYSCOM_SET_BGM_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_BGM_ONOFF;
    pub const SYSCOM_SET_BGM_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_BGM_ONOFF_DEFAULT;
    pub const SYSCOM_GET_BGM_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_BGM_ONOFF;
    pub const SYSCOM_SET_KOE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_KOE_ONOFF;
    pub const SYSCOM_SET_KOE_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_KOE_ONOFF_DEFAULT;
    pub const SYSCOM_GET_KOE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_KOE_ONOFF;
    pub const SYSCOM_SET_PCM_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_PCM_ONOFF;
    pub const SYSCOM_SET_PCM_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_PCM_ONOFF_DEFAULT;
    pub const SYSCOM_GET_PCM_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_PCM_ONOFF;
    pub const SYSCOM_SET_SE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SE_ONOFF;
    pub const SYSCOM_SET_SE_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SE_ONOFF_DEFAULT;
    pub const SYSCOM_GET_SE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SE_ONOFF;
    pub const SYSCOM_SET_MOV_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MOV_ONOFF;
    pub const SYSCOM_SET_MOV_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MOV_ONOFF_DEFAULT;
    pub const SYSCOM_GET_MOV_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_MOV_ONOFF;
    pub const SYSCOM_SET_SOUND_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SOUND_ONOFF;
    pub const SYSCOM_SET_SOUND_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SOUND_ONOFF_DEFAULT;
    pub const SYSCOM_GET_SOUND_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SOUND_ONOFF;
    pub const SYSCOM_SET_BGMFADE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_BGMFADE_ONOFF;
    pub const SYSCOM_SET_BGMFADE_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_BGMFADE_ONOFF_DEFAULT;
    pub const SYSCOM_GET_BGMFADE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_BGMFADE_ONOFF;
    pub const SYSCOM_SET_KOEMODE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_KOEMODE;
    pub const SYSCOM_SET_KOEMODE_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_KOEMODE_DEFAULT;
    pub const SYSCOM_GET_KOEMODE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_KOEMODE;
    pub const SYSCOM_SET_CHARAKOE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_CHARAKOE_ONOFF;
    pub const SYSCOM_SET_CHARAKOE_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_CHARAKOE_ONOFF_DEFAULT;
    pub const SYSCOM_GET_CHARAKOE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_CHARAKOE_ONOFF;
    pub const SYSCOM_SET_CHARAKOE_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_CHARAKOE_VOLUME;
    pub const SYSCOM_SET_CHARAKOE_VOLUME_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_CHARAKOE_VOLUME_DEFAULT;
    pub const SYSCOM_GET_CHARAKOE_VOLUME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_CHARAKOE_VOLUME;
    pub const SYSCOM_SET_JITAN_NORMAL_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_JITAN_NORMAL_ONOFF;
    pub const SYSCOM_SET_JITAN_NORMAL_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_JITAN_NORMAL_ONOFF_DEFAULT;
    pub const SYSCOM_GET_JITAN_NORMAL_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_JITAN_NORMAL_ONOFF;
    pub const SYSCOM_SET_JITAN_AUTO_MODE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_JITAN_AUTO_MODE_ONOFF;
    pub const SYSCOM_SET_JITAN_AUTO_MODE_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_JITAN_AUTO_MODE_ONOFF_DEFAULT;
    pub const SYSCOM_GET_JITAN_AUTO_MODE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_JITAN_AUTO_MODE_ONOFF;
    pub const SYSCOM_SET_JITAN_KOE_REPLAY_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_JITAN_KOE_REPLAY_ONOFF;
    pub const SYSCOM_SET_JITAN_KOE_REPLAY_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_JITAN_KOE_REPLAY_ONOFF_DEFAULT;
    pub const SYSCOM_GET_JITAN_KOE_REPLAY_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_JITAN_KOE_REPLAY_ONOFF;
    pub const SYSCOM_SET_JITAN_SPEED: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_JITAN_SPEED;
    pub const SYSCOM_SET_JITAN_SPEED_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_JITAN_SPEED_DEFAULT;
    pub const SYSCOM_GET_JITAN_SPEED: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_JITAN_SPEED;
    pub const SYSCOM_SET_MESSAGE_SPEED: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MESSAGE_SPEED;
    pub const SYSCOM_SET_MESSAGE_SPEED_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MESSAGE_SPEED_DEFAULT;
    pub const SYSCOM_GET_MESSAGE_SPEED: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_MESSAGE_SPEED;
    pub const SYSCOM_SET_MESSAGE_NOWAIT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MESSAGE_NOWAIT;
    pub const SYSCOM_SET_MESSAGE_NOWAIT_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MESSAGE_NOWAIT_DEFAULT;
    pub const SYSCOM_GET_MESSAGE_NOWAIT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_MESSAGE_NOWAIT;
    pub const SYSCOM_SET_AUTO_MODE_MOJI_WAIT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_AUTO_MODE_MOJI_WAIT;
    pub const SYSCOM_SET_AUTO_MODE_MOJI_WAIT_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_AUTO_MODE_MOJI_WAIT_DEFAULT;
    pub const SYSCOM_GET_AUTO_MODE_MOJI_WAIT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_AUTO_MODE_MOJI_WAIT;
    pub const SYSCOM_SET_AUTO_MODE_MIN_WAIT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_AUTO_MODE_MIN_WAIT;
    pub const SYSCOM_SET_AUTO_MODE_MIN_WAIT_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_AUTO_MODE_MIN_WAIT_DEFAULT;
    pub const SYSCOM_GET_AUTO_MODE_MIN_WAIT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_AUTO_MODE_MIN_WAIT;
    pub const SYSCOM_SET_MOUSE_CURSOR_HIDE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MOUSE_CURSOR_HIDE_ONOFF;
    pub const SYSCOM_SET_MOUSE_CURSOR_HIDE_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MOUSE_CURSOR_HIDE_ONOFF_DEFAULT;
    pub const SYSCOM_GET_MOUSE_CURSOR_HIDE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_MOUSE_CURSOR_HIDE_ONOFF;
    pub const SYSCOM_SET_MOUSE_CURSOR_HIDE_TIME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MOUSE_CURSOR_HIDE_TIME;
    pub const SYSCOM_SET_MOUSE_CURSOR_HIDE_TIME_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_MOUSE_CURSOR_HIDE_TIME_DEFAULT;
    pub const SYSCOM_GET_MOUSE_CURSOR_HIDE_TIME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_MOUSE_CURSOR_HIDE_TIME;
    pub const SYSCOM_SET_FILTER_COLOR_R: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_FILTER_COLOR_R;
    pub const SYSCOM_SET_FILTER_COLOR_R_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_FILTER_COLOR_R_DEFAULT;
    pub const SYSCOM_GET_FILTER_COLOR_R: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_FILTER_COLOR_R;
    pub const SYSCOM_SET_FILTER_COLOR_G: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_FILTER_COLOR_G;
    pub const SYSCOM_SET_FILTER_COLOR_G_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_FILTER_COLOR_G_DEFAULT;
    pub const SYSCOM_GET_FILTER_COLOR_G: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_FILTER_COLOR_G;
    pub const SYSCOM_SET_FILTER_COLOR_B: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_FILTER_COLOR_B;
    pub const SYSCOM_SET_FILTER_COLOR_B_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_FILTER_COLOR_B_DEFAULT;
    pub const SYSCOM_GET_FILTER_COLOR_B: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_FILTER_COLOR_B;
    pub const SYSCOM_SET_FILTER_COLOR_A: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_FILTER_COLOR_A;
    pub const SYSCOM_SET_FILTER_COLOR_A_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_FILTER_COLOR_A_DEFAULT;
    pub const SYSCOM_GET_FILTER_COLOR_A: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_FILTER_COLOR_A;
    pub const SYSCOM_SET_OBJECT_DISP_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_OBJECT_DISP_ONOFF;
    pub const SYSCOM_SET_OBJECT_DISP_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_OBJECT_DISP_ONOFF_DEFAULT;
    pub const SYSCOM_GET_OBJECT_DISP_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_OBJECT_DISP_ONOFF;
    pub const SYSCOM_SET_GLOBAL_EXTRA_SWITCH_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_GLOBAL_EXTRA_SWITCH_ONOFF;
    pub const SYSCOM_SET_GLOBAL_EXTRA_SWITCH_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_GLOBAL_EXTRA_SWITCH_ONOFF_DEFAULT;
    pub const SYSCOM_GET_GLOBAL_EXTRA_SWITCH_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_GLOBAL_EXTRA_SWITCH_ONOFF;
    pub const SYSCOM_SET_GLOBAL_EXTRA_MODE_VALUE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_GLOBAL_EXTRA_MODE_VALUE;
    pub const SYSCOM_SET_GLOBAL_EXTRA_MODE_VALUE_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_GLOBAL_EXTRA_MODE_VALUE_DEFAULT;
    pub const SYSCOM_GET_GLOBAL_EXTRA_MODE_VALUE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_GLOBAL_EXTRA_MODE_VALUE;
    pub const SYSCOM_SET_SAVELOAD_ALERT_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SAVELOAD_ALERT_ONOFF;
    pub const SYSCOM_SET_SLEEP_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SLEEP_ONOFF;
    pub const SYSCOM_SET_NO_WIPE_ANIME_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_NO_WIPE_ANIME_ONOFF;
    pub const SYSCOM_SET_NO_MWND_ANIME_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_NO_MWND_ANIME_ONOFF;
    pub const SYSCOM_SET_SKIP_WIPE_ANIME_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SKIP_WIPE_ANIME_ONOFF;
    pub const SYSCOM_SET_WHEEL_NEXT_MESSAGE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_WHEEL_NEXT_MESSAGE_ONOFF;
    pub const SYSCOM_SET_KOE_DONT_STOP_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_KOE_DONT_STOP_ONOFF;
    pub const SYSCOM_SET_SKIP_UNREAD_MESSAGE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SKIP_UNREAD_MESSAGE_ONOFF;
    pub const SYSCOM_SET_PLAY_SILENT_SOUND_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_PLAY_SILENT_SOUND_ONOFF;
    pub const SYSCOM_SET_SAVELOAD_ALERT_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SAVELOAD_ALERT_ONOFF_DEFAULT;
    pub const SYSCOM_SET_SLEEP_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SLEEP_ONOFF_DEFAULT;
    pub const SYSCOM_SET_NO_WIPE_ANIME_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_NO_WIPE_ANIME_ONOFF_DEFAULT;
    pub const SYSCOM_SET_NO_MWND_ANIME_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_NO_MWND_ANIME_ONOFF_DEFAULT;
    pub const SYSCOM_SET_SKIP_WIPE_ANIME_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SKIP_WIPE_ANIME_ONOFF_DEFAULT;
    pub const SYSCOM_SET_WHEEL_NEXT_MESSAGE_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_WHEEL_NEXT_MESSAGE_ONOFF_DEFAULT;
    pub const SYSCOM_SET_KOE_DONT_STOP_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_KOE_DONT_STOP_ONOFF_DEFAULT;
    pub const SYSCOM_SET_SKIP_UNREAD_MESSAGE_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_SKIP_UNREAD_MESSAGE_ONOFF_DEFAULT;
    pub const SYSCOM_SET_PLAY_SILENT_SOUND_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_PLAY_SILENT_SOUND_ONOFF_DEFAULT;
    pub const SYSCOM_GET_SAVELOAD_ALERT_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SAVELOAD_ALERT_ONOFF;
    pub const SYSCOM_GET_SLEEP_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SLEEP_ONOFF;
    pub const SYSCOM_GET_NO_WIPE_ANIME_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_NO_WIPE_ANIME_ONOFF;
    pub const SYSCOM_GET_NO_MWND_ANIME_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_NO_MWND_ANIME_ONOFF;
    pub const SYSCOM_GET_SKIP_WIPE_ANIME_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SKIP_WIPE_ANIME_ONOFF;
    pub const SYSCOM_GET_WHEEL_NEXT_MESSAGE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_WHEEL_NEXT_MESSAGE_ONOFF;
    pub const SYSCOM_GET_KOE_DONT_STOP_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_KOE_DONT_STOP_ONOFF;
    pub const SYSCOM_GET_SKIP_UNREAD_MESSAGE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SKIP_UNREAD_MESSAGE_ONOFF;
    pub const SYSCOM_GET_PLAY_SILENT_SOUND_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_PLAY_SILENT_SOUND_ONOFF;
    pub const SYSCOM_IS_FONT_EXIST: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_IS_FONT_EXIST;
    pub const SYSCOM_SET_FONT_NAME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_FONT_NAME;
    pub const SYSCOM_SET_FONT_NAME_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_FONT_NAME_DEFAULT;
    pub const SYSCOM_GET_FONT_NAME: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_FONT_NAME;
    pub const SYSCOM_SET_FONT_BOLD: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_FONT_BOLD;
    pub const SYSCOM_SET_FONT_BOLD_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_FONT_BOLD_DEFAULT;
    pub const SYSCOM_GET_FONT_BOLD: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_FONT_BOLD;
    pub const SYSCOM_SET_FONT_DECORATION: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_FONT_DECORATION;
    pub const SYSCOM_SET_FONT_DECORATION_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_FONT_DECORATION_DEFAULT;
    pub const SYSCOM_GET_FONT_DECORATION: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_FONT_DECORATION;
    pub const SYSCOM_CREATE_CAPTURE_BUFFER: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CREATE_CAPTURE_BUFFER;
    pub const SYSCOM_DESTROY_CAPTURE_BUFFER: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_DESTROY_CAPTURE_BUFFER;
    pub const SYSCOM_CAPTURE_AND_SAVE_BUFFER_TO_PNG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CAPTURE_AND_SAVE_BUFFER_TO_PNG;
    pub const SYSCOM_CAPTURE_TO_CAPTURE_BUFFER: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CAPTURE_TO_CAPTURE_BUFFER;
    pub const SYSCOM_SAVE_CAPTURE_BUFFER_TO_FILE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SAVE_CAPTURE_BUFFER_TO_FILE;
    pub const SYSCOM_LOAD_FLAG_FROM_CAPTURE_FILE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_LOAD_FLAG_FROM_CAPTURE_FILE;
    pub const SYSCOM_OPEN_TWEET_DIALOG: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_OPEN_TWEET_DIALOG;
    pub const SYSCOM_SET_RETURN_SCENE_ONCE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_SET_RETURN_SCENE_ONCE;
    pub const SYSCOM_GET_SYSTEM_EXTRA_INT_VALUE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SYSTEM_EXTRA_INT_VALUE;
    pub const SYSCOM_GET_SYSTEM_EXTRA_STR_VALUE: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_GET_SYSTEM_EXTRA_STR_VALUE;
    pub const SYSCOMMENU_SET_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOMMENU_SET_ENABLE;
    pub const SYSCOMMENU_SET_DISABLE: i32 = crate::runtime::forms::codes::elm_value::SYSCOMMENU_SET_DISABLE;
    pub const MWNDBTN_SET_ENABLE: i32 = crate::runtime::forms::codes::elm_value::MWNDBTN_SET_ENABLE;
    pub const MWNDBTN_SET_DISABLE: i32 = crate::runtime::forms::codes::elm_value::MWNDBTN_SET_DISABLE;
    pub const SCRIPT_SET_AUTO_SAVEPOINT_OFF: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_AUTO_SAVEPOINT_OFF;
    pub const SCRIPT_SET_AUTO_SAVEPOINT_ON: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_AUTO_SAVEPOINT_ON;
    pub const SCRIPT_SET_SKIP_DISABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_SKIP_DISABLE;
    pub const SCRIPT_SET_SKIP_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_SKIP_ENABLE;
    pub const SCRIPT_GET_SKIP_DISABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_SKIP_DISABLE_FLAG;
    pub const SCRIPT_SET_SKIP_DISABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_SKIP_DISABLE_FLAG;
    pub const SCRIPT_SET_CTRL_SKIP_DISABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_CTRL_SKIP_DISABLE;
    pub const SCRIPT_SET_CTRL_SKIP_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_CTRL_SKIP_ENABLE;
    pub const SCRIPT_GET_CTRL_SKIP_DISABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_CTRL_SKIP_DISABLE_FLAG;
    pub const SCRIPT_SET_CTRL_SKIP_DISABLE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_CTRL_SKIP_DISABLE_FLAG;
    pub const SCRIPT_CHECK_SKIP: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_CHECK_SKIP;
    pub const SCRIPT_SET_STOP_SKIP_BY_KEY_DISABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_STOP_SKIP_BY_KEY_DISABLE;
    pub const SCRIPT_SET_STOP_SKIP_BY_KEY_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_STOP_SKIP_BY_KEY_ENABLE;
    pub const SCRIPT_SET_END_MSG_BY_KEY_DISABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_END_MSG_BY_KEY_DISABLE;
    pub const SCRIPT_SET_END_MSG_BY_KEY_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_END_MSG_BY_KEY_ENABLE;
    pub const SCRIPT_SET_SKIP_UNREAD_MESSAGE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_SKIP_UNREAD_MESSAGE_FLAG;
    pub const SCRIPT_GET_SKIP_UNREAD_MESSAGE_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_SKIP_UNREAD_MESSAGE_FLAG;
    pub const SCRIPT_START_AUTO_MODE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_START_AUTO_MODE;
    pub const SCRIPT_END_AUTO_MODE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_END_AUTO_MODE;
    pub const SCRIPT_SET_AUTO_MODE_MOJI_WAIT: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_AUTO_MODE_MOJI_WAIT;
    pub const SCRIPT_SET_AUTO_MODE_MOJI_WAIT_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_AUTO_MODE_MOJI_WAIT_DEFAULT;
    pub const SCRIPT_GET_AUTO_MODE_MOJI_WAIT: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_AUTO_MODE_MOJI_WAIT;
    pub const SCRIPT_SET_AUTO_MODE_MIN_WAIT: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_AUTO_MODE_MIN_WAIT;
    pub const SCRIPT_SET_AUTO_MODE_MIN_WAIT_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_AUTO_MODE_MIN_WAIT_DEFAULT;
    pub const SCRIPT_GET_AUTO_MODE_MIN_WAIT: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_AUTO_MODE_MIN_WAIT;
    pub const SCRIPT_SET_AUTO_MODE_MOJI_CNT: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_AUTO_MODE_MOJI_CNT;
    pub const SCRIPT_SET_MESSAGE_SPEED: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MESSAGE_SPEED;
    pub const SCRIPT_SET_MESSAGE_SPEED_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MESSAGE_SPEED_DEFAULT;
    pub const SCRIPT_GET_MESSAGE_SPEED: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_MESSAGE_SPEED;
    pub const SCRIPT_SET_MESSAGE_NOWAIT_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MESSAGE_NOWAIT_FLAG;
    pub const SCRIPT_GET_MESSAGE_NOWAIT_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_MESSAGE_NOWAIT_FLAG;
    pub const SCRIPT_SET_MSG_ASYNC_MODE_ON: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MSG_ASYNC_MODE_ON;
    pub const SCRIPT_SET_MSG_ASYNC_MODE_ON_ONCE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MSG_ASYNC_MODE_ON_ONCE;
    pub const SCRIPT_SET_MSG_ASYNC_MODE_OFF: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MSG_ASYNC_MODE_OFF;
    pub const SCRIPT_SET_HIDE_MWND_DISABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_HIDE_MWND_DISABLE;
    pub const SCRIPT_SET_HIDE_MWND_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_HIDE_MWND_ENABLE;
    pub const SCRIPT_SET_MSG_BACK_DISABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MSG_BACK_DISABLE;
    pub const SCRIPT_SET_MSG_BACK_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MSG_BACK_ENABLE;
    pub const SCRIPT_SET_MSG_BACK_OFF: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MSG_BACK_OFF;
    pub const SCRIPT_SET_MSG_BACK_ON: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MSG_BACK_ON;
    pub const SCRIPT_SET_MSG_BACK_DISP_OFF: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MSG_BACK_DISP_OFF;
    pub const SCRIPT_SET_MSG_BACK_DISP_ON: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MSG_BACK_DISP_ON;
    pub const SCRIPT_SET_MSG_BACK_PROC_OFF: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MSG_BACK_PROC_OFF;
    pub const SCRIPT_SET_MSG_BACK_PROC_ON: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MSG_BACK_PROC_ON;
    pub const SCRIPT_SET_MOUSE_MOVE_BY_KEY_DISABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MOUSE_MOVE_BY_KEY_DISABLE;
    pub const SCRIPT_SET_MOUSE_MOVE_BY_KEY_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MOUSE_MOVE_BY_KEY_ENABLE;
    pub const SCRIPT_SET_MOUSE_DISP_OFF: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MOUSE_DISP_OFF;
    pub const SCRIPT_SET_MOUSE_DISP_ON: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MOUSE_DISP_ON;
    pub const SCRIPT_SET_MOUSE_CURSOR_HIDE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MOUSE_CURSOR_HIDE_ONOFF;
    pub const SCRIPT_SET_MOUSE_CURSOR_HIDE_ONOFF_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MOUSE_CURSOR_HIDE_ONOFF_DEFAULT;
    pub const SCRIPT_GET_MOUSE_CURSOR_HIDE_ONOFF: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_MOUSE_CURSOR_HIDE_ONOFF;
    pub const SCRIPT_SET_MOUSE_CURSOR_HIDE_TIME: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MOUSE_CURSOR_HIDE_TIME;
    pub const SCRIPT_SET_MOUSE_CURSOR_HIDE_TIME_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MOUSE_CURSOR_HIDE_TIME_DEFAULT;
    pub const SCRIPT_GET_MOUSE_CURSOR_HIDE_TIME: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_MOUSE_CURSOR_HIDE_TIME;
    pub const SCRIPT_SET_KEY_DISABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_KEY_DISABLE;
    pub const SCRIPT_SET_KEY_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_KEY_ENABLE;
    pub const SCRIPT_SET_MWND_ANIME_ON_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MWND_ANIME_ON_FLAG;
    pub const SCRIPT_GET_MWND_ANIME_ON_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_MWND_ANIME_ON_FLAG;
    pub const SCRIPT_SET_MWND_ANIME_OFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MWND_ANIME_OFF_FLAG;
    pub const SCRIPT_GET_MWND_ANIME_OFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_MWND_ANIME_OFF_FLAG;
    pub const SCRIPT_SET_MWND_DISP_OFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_MWND_DISP_OFF_FLAG;
    pub const SCRIPT_GET_MWND_DISP_OFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_MWND_DISP_OFF_FLAG;
    pub const SCRIPT_SET_QUAKE_STOP_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_QUAKE_STOP_FLAG;
    pub const SCRIPT_GET_EMOTE_MOUTH_STOP_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_EMOTE_MOUTH_STOP_FLAG;
    pub const SCRIPT_SET_EMOTE_MOUTH_STOP_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_EMOTE_MOUTH_STOP_FLAG;
    pub const SCRIPT_GET_QUAKE_STOP_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_QUAKE_STOP_FLAG;
    pub const SCRIPT_SET_VSYNC_WAIT_OFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_VSYNC_WAIT_OFF_FLAG;
    pub const SCRIPT_GET_VSYNC_WAIT_OFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_VSYNC_WAIT_OFF_FLAG;
    pub const SCRIPT_SET_KOE_DONT_STOP_ON_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_KOE_DONT_STOP_ON_FLAG;
    pub const SCRIPT_GET_KOE_DONT_STOP_ON_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_KOE_DONT_STOP_ON_FLAG;
    pub const SCRIPT_SET_KOE_DONT_STOP_OFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_KOE_DONT_STOP_OFF_FLAG;
    pub const SCRIPT_GET_KOE_DONT_STOP_OFF_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_KOE_DONT_STOP_OFF_FLAG;
    pub const SCRIPT_SET_SHORTCUT_DISABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_SHORTCUT_DISABLE;
    pub const SCRIPT_SET_SHORTCUT_ENABLE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_SHORTCUT_ENABLE;
    pub const SCRIPT_START_BGMFADE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_START_BGMFADE;
    pub const SCRIPT_END_BGMFADE: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_END_BGMFADE;
    pub const SCRIPT_SET_SKIP_TRIGGER: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_SKIP_TRIGGER;
    pub const SCRIPT_IGNORE_R_ON: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_IGNORE_R_ON;
    pub const SCRIPT_IGNORE_R_OFF: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_IGNORE_R_OFF;
    pub const SCRIPT_SET_CURSOR_NO: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_CURSOR_NO;
    pub const SCRIPT_GET_CURSOR_NO: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_CURSOR_NO;
    pub const SCRIPT_SET_TIME_STOP_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_TIME_STOP_FLAG;
    pub const SCRIPT_SET_COUNTER_TIME_STOP_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_COUNTER_TIME_STOP_FLAG;
    pub const SCRIPT_SET_FRAME_ACTION_TIME_STOP_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_FRAME_ACTION_TIME_STOP_FLAG;
    pub const SCRIPT_SET_STAGE_TIME_STOP_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_STAGE_TIME_STOP_FLAG;
    pub const SCRIPT_GET_TIME_STOP_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_TIME_STOP_FLAG;
    pub const SCRIPT_GET_COUNTER_TIME_STOP_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_COUNTER_TIME_STOP_FLAG;
    pub const SCRIPT_GET_FRAME_ACTION_TIME_STOP_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_FRAME_ACTION_TIME_STOP_FLAG;
    pub const SCRIPT_GET_STAGE_TIME_STOP_FLAG: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_STAGE_TIME_STOP_FLAG;
    pub const SCRIPT_SET_FONT_NAME: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_FONT_NAME;
    pub const SCRIPT_SET_FONT_NAME_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_FONT_NAME_DEFAULT;
    pub const SCRIPT_GET_FONT_NAME: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_FONT_NAME;
    pub const SCRIPT_SET_FONT_BOLD: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_FONT_BOLD;
    pub const SCRIPT_SET_FONT_BOLD_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_FONT_BOLD_DEFAULT;
    pub const SCRIPT_GET_FONT_BOLD: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_FONT_BOLD;
    pub const SCRIPT_SET_FONT_SHADOW: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_FONT_SHADOW;
    pub const SCRIPT_SET_FONT_SHADOW_DEFAULT: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_SET_FONT_SHADOW_DEFAULT;
    pub const SCRIPT_GET_FONT_SHADOW: i32 = crate::runtime::forms::codes::elm_value::SCRIPT_GET_FONT_SHADOW;
    pub const SYSTEM_CHECK_ACTIVE: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_CHECK_ACTIVE;
    pub const SYSTEM_CHECK_DEBUG_FLAG: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_CHECK_DEBUG_FLAG;
    pub const SYSTEM_SHELL_OPEN_FILE: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_SHELL_OPEN_FILE;
    pub const SYSTEM_SHELL_OPEN_WEB: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_SHELL_OPEN_WEB;
    pub const SYSTEM_CHECK_FILE_EXIST: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_CHECK_FILE_EXIST;
    pub const SYSTEM_CHECK_FILE_EXIST_SAVE_DIR: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_CHECK_FILE_EXIST_SAVE_DIR;
    pub const SYSTEM_CHECK_DUMMY_FILE_ONCE: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_CHECK_DUMMY_FILE_ONCE;
    pub const SYSTEM_CLEAR_DUMMY_FILE: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_CLEAR_DUMMY_FILE;
    pub const SYSTEM_OPEN_DIALOG_FOR_CHIHAYA_BENCH: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_OPEN_DIALOG_FOR_CHIHAYA_BENCH;
    pub const SYSTEM_GET_SPEC_INFO_FOR_CHIHAYA_BENCH: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_GET_SPEC_INFO_FOR_CHIHAYA_BENCH;
    pub const SYSTEM_MESSAGEBOX_OK: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_MESSAGEBOX_OK;
    pub const SYSTEM_MESSAGEBOX_OKCANCEL: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_MESSAGEBOX_OKCANCEL;
    pub const SYSTEM_MESSAGEBOX_YESNO: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_MESSAGEBOX_YESNO;
    pub const SYSTEM_MESSAGEBOX_YESNOCANCEL: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_MESSAGEBOX_YESNOCANCEL;
    pub const SYSTEM_DEBUG_MESSAGEBOX_OK: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_DEBUG_MESSAGEBOX_OK;
    pub const SYSTEM_DEBUG_MESSAGEBOX_OKCANCEL: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_DEBUG_MESSAGEBOX_OKCANCEL;
    pub const SYSTEM_DEBUG_MESSAGEBOX_YESNO: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_DEBUG_MESSAGEBOX_YESNO;
    pub const SYSTEM_DEBUG_MESSAGEBOX_YESNOCANCEL: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_DEBUG_MESSAGEBOX_YESNOCANCEL;
    pub const SYSTEM_DEBUG_WRITE_LOG: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_DEBUG_WRITE_LOG;
    pub const SYSTEM_GET_CALENDAR: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_GET_CALENDAR;
    pub const SYSTEM_GET_UNIX_TIME: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_GET_UNIX_TIME;
    pub const SYSTEM_GET_LANGUAGE: i32 = crate::runtime::forms::codes::elm_value::SYSTEM_GET_LANGUAGE;
    pub const EXCALL_ARRAY: i32 = crate::runtime::forms::codes::elm_value::EXCALL_ARRAY;
    pub const EXCALL_ALLOC: i32 = crate::runtime::forms::codes::elm_value::EXCALL_ALLOC;
    pub const EXCALL_CHECK_ALLOC: i32 = crate::runtime::forms::codes::elm_value::EXCALL_CHECK_ALLOC;
    pub const EXCALL_IS_EXCALL: i32 = crate::runtime::forms::codes::elm_value::EXCALL_IS_EXCALL;
    pub const EXCALL_FREE: i32 = crate::runtime::forms::codes::elm_value::EXCALL_FREE;
    pub const EXCALL_STAGE: i32 = crate::runtime::forms::codes::elm_value::EXCALL_STAGE;
    pub const EXCALL_BACK: i32 = crate::runtime::forms::codes::elm_value::EXCALL_BACK;
    pub const EXCALL_FRONT: i32 = crate::runtime::forms::codes::elm_value::EXCALL_FRONT;
    pub const EXCALL_NEXT: i32 = crate::runtime::forms::codes::elm_value::EXCALL_NEXT;
    pub const EXCALL_COUNTER: i32 = crate::runtime::forms::codes::elm_value::EXCALL_COUNTER;
    pub const EXCALL_FRAME_ACTION: i32 = crate::runtime::forms::codes::elm_value::EXCALL_FRAME_ACTION;
    pub const EXCALL_FRAME_ACTION_CH: i32 = crate::runtime::forms::codes::elm_value::EXCALL_FRAME_ACTION_CH;
    pub const EXCALL_F: i32 = crate::runtime::forms::codes::elm_value::EXCALL_F;
    pub const EXCALL_SCRIPT: i32 = crate::runtime::forms::codes::elm_value::EXCALL_SCRIPT;
    pub const STEAM_SET_ACHIEVEMENT: i32 = crate::runtime::forms::codes::elm_value::STEAM_SET_ACHIEVEMENT;
    pub const STEAM_RESET_ALL_STATUS: i32 = crate::runtime::forms::codes::elm_value::STEAM_RESET_ALL_STATUS;
}

pub use elm_value::*;
pub use global_form::*;

// -----------------------------------------------------------------------------
// Static runtime constants.
// -----------------------------------------------------------------------------

// Runtime numeric constants recovered from the original engine and reverse results.
// Values here are fixed constants used directly by the runtime.

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
    pub database_get_data: i32,
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
            form_global_stage: global_form::STAGE,
            form_global_mov: global_form::MOV,
            form_global_bgm: global_form::BGM,
            form_global_pcm: global_form::PCM,
            form_global_pcmch: global_form::PCMCH,
            form_global_se: global_form::SE,
            form_global_pcm_event: global_form::PCMEVENT,
            form_global_excall: global_form::EXCALL,
            form_global_koe_st: global_form::KOE_ST,
            form_global_bgm_table: global_form::BGMTABLE,

            form_global_screen: global_form::SCREEN,
            form_global_msgbk: global_form::MSGBK,

            form_global_input: global_form::INPUT,
            form_global_mouse: global_form::MOUSE,
            form_global_keylist: global_form::KEY,
            form_global_key: global_form::KEY,

            // Optional global forms (disabled by default).
            form_global_syscom: global_form::SYSCOM,
            form_global_script: global_form::SCRIPT,
            form_global_system: global_form::SYSTEM,
            form_global_frame_action: global_form::FRAME_ACTION,
            form_global_frame_action_ch: crate::runtime::forms::codes::FORM_GLOBAL_FRAME_ACTION_CH,

            form_global_math: global_form::MATH,
            form_global_cgtable: global_form::CGTABLE,
            form_global_database: global_form::DATABASE,
            form_global_g00buf: global_form::G00BUF,
            form_global_mask: global_form::MASK,
            form_global_editbox: global_form::EDITBOX,
            form_global_file: global_form::FILE,
            form_global_steam: global_form::STEAM,

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
            mouse_op_left: crate::runtime::forms::codes::elm_value::MOUSE_LEFT,
            mouse_op_right: crate::runtime::forms::codes::elm_value::MOUSE_RIGHT,
            mouse_op_next: 8,
            mouse_op_get_pos: 9,
            mouse_op_set_pos: 10,

            // KEYLIST sub-ops.
            keylist_op_wait: 0,
            keylist_op_wait_force: 1,
            keylist_op_clear: 3,
            keylist_op_next: 5,

            // KEY sub-ops.
            key_op_dir: -9999,
            key_op_on_down: 1,
            key_op_on_up: 4,
            key_op_on_down_up: 5,
            key_op_is_down: 6,
            key_op_is_up: 7,
            key_op_on_flick: 10,
            key_op_flick: 14,
            key_op_flick_angle: crate::runtime::forms::codes::elm_value::KEY_GET_FLICK_ANGLE,

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

            // CGTABLE element codes.
            cgtable_flag: elm_value::CGTABLE_FLAG,
            cgtable_set_disable: elm_value::CGTABLE_SET_DISABLE,
            cgtable_set_enable: elm_value::CGTABLE_SET_ENABLE,
            cgtable_set_all_flag: elm_value::CGTABLE_SET_ALL_FLAG,
            cgtable_get_cg_cnt: elm_value::CGTABLE_GET_CG_CNT,
            cgtable_get_look_cnt: elm_value::CGTABLE_GET_LOOK_CNT,
            cgtable_get_look_percent: elm_value::CGTABLE_GET_LOOK_PERCENT,
            cgtable_get_flag_no_by_name: elm_value::CGTABLE_GET_FLAG_NO_BY_NAME,
            cgtable_get_look_by_name: elm_value::CGTABLE_GET_LOOK_BY_NAME,
            cgtable_set_look_by_name: elm_value::CGTABLE_SET_LOOK_BY_NAME,
            cgtable_get_name_by_flag_no: elm_value::CGTABLE_GET_NAME_BY_FLAG_NO,

            // DATABASE element codes.
            database_list_get_size: elm_value::DATABASELIST_GET_SIZE,
            database_get_num: elm_value::DATABASE_GET_NUM,
            database_get_str: elm_value::DATABASE_GET_STR,
            database_get_data: elm_value::DATABASE_GET_DATA,
            database_check_item: elm_value::DATABASE_CHECK_ITEM,
            database_check_column: elm_value::DATABASE_CHECK_COLUMN,
            database_find_num: elm_value::DATABASE_FIND_NUM,
            database_find_str: elm_value::DATABASE_FIND_STR,
            database_find_str_real: elm_value::DATABASE_FIND_STR_REAL,

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
            obj_end_movie_loop: elm_value::OBJECT_END_MOVIE_LOOP,
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
            obj_load_gan: crate::runtime::forms::codes::ELM_OBJECT_LOAD_GAN,
            obj_start_gan: crate::runtime::forms::codes::ELM_OBJECT_START_GAN,

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

