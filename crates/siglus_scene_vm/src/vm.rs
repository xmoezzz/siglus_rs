//! Scene VM

use anyhow::{anyhow, bail, Context, Result};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::sync::Arc;

use crate::elm_code;
use crate::runtime::forms::codes;
use crate::runtime::globals::{
    ObjectFrameActionState, PendingButtonAction, PendingButtonActionKind, PendingFrameActionFinish,
};
use crate::runtime::{self, constants, CommandContext, RuntimeLoadRequest, RuntimeSaveKind, RuntimeSaveRequest, Value};
use crate::scene_stream::SceneStream;
use siglus_assets::scene_pck::{ScenePck, ScenePckDecodeOptions};

// VM tracing is diagnostic-only. Keep the disabled path to a single cached
// branch: the message expression is evaluated only when the trace filter
// matches, so format!/Debug formatting does not pollute the interpreter hot
// path.
macro_rules! vm_trace {
    ($vm:expr, $pc:expr, $msg:expr $(,)?) => {{
        if $vm.vm_trace_matches() {
            $vm.vm_trace_emit($pc, $msg);
        }
    }};
}

// SG_* diagnostic traces are intentionally lazy as well.  A disabled trace must
// not allocate Strings, snapshot stacks, or format Debug values in the VM hot
// path.  Keep all formatting behind the cached SG_DEBUG branch.
macro_rules! sg_omv_trace {
    ($vm:expr, $($arg:tt)*) => {{
        if $vm.sg_debug_enabled() {
            $vm.sg_omv_trace_emit(format_args!($($arg)*));
        }
    }};
}

const CD_NONE: u8 = constants::cd::NONE;
const CD_NL: u8 = constants::cd::NL;
const CD_PUSH: u8 = constants::cd::PUSH;
const CD_POP: u8 = constants::cd::POP;
const CD_COPY: u8 = constants::cd::COPY;
const CD_PROPERTY: u8 = constants::cd::PROPERTY;
const CD_COPY_ELM: u8 = constants::cd::COPY_ELM;
const CD_DEC_PROP: u8 = constants::cd::DEC_PROP;
const CD_ELM_POINT: u8 = constants::cd::ELM_POINT;
const CD_ARG: u8 = constants::cd::ARG;

const CD_GOTO: u8 = constants::cd::GOTO;
const CD_GOTO_TRUE: u8 = constants::cd::GOTO_TRUE;
const CD_GOTO_FALSE: u8 = constants::cd::GOTO_FALSE;
const CD_GOSUB: u8 = constants::cd::GOSUB;
const CD_GOSUBSTR: u8 = constants::cd::GOSUBSTR;
const CD_RETURN: u8 = constants::cd::RETURN;
const CD_EOF: u8 = constants::cd::EOF;

const CD_ASSIGN: u8 = constants::cd::ASSIGN;
const CD_OPERATE_1: u8 = constants::cd::OPERATE_1;
const CD_OPERATE_2: u8 = constants::cd::OPERATE_2;

// ---------------------------------------------------------------------------
// SG_VM_RING: bounded per-instruction ring buffer (diagnostic, SG_VM_RING=1).
//
// A `pop_int` underflow only reports the *current* pc, which is useless when the
// value went missing several instructions earlier: compiled `switch` chains keep
// a selector alive across dozens of compares, and our stream's operand order is
// not self-describing. Keeping the last N executed instructions together with the
// int-stack depth seen *at entry* turns "the stack is empty" into "here is the
// exact instruction after which the value stopped existing".
// ---------------------------------------------------------------------------
const SG_RING_CAP: usize = 128;

thread_local! {
    // (pc, opcode, int depth at entry, element_points len, int stack top, scn len, call depth)
    static SG_OP_RING: std::cell::RefCell<std::collections::VecDeque<(u32, u8, u32, u32, i64, u32, u32)>> =
        std::cell::RefCell::new(std::collections::VecDeque::new());
    static SG_OP_RING_ON: bool = std::env::var_os("SG_VM_RING").is_some();
    // Per-call/per-return tracing needs its own opt-in: in a map frame loop it emits
    // hundreds of thousands of lines and produced a 350 MB log in a single run.
    static SG_RET_TRACE_ON: bool = std::env::var_os("SG_VM_RET_TRACE").is_some();
    /// Scene-transition tracing (`[SG-DIAG-6/7/13]`). Opt-in: the map dispatcher
    /// re-enters its scenes per object per frame, which measured 600-950 logcat
    /// lines/s on device and rotated the crash buffer out from under us.
    static SG_SCENE_TRACE_ON: bool = std::env::var_os("SG_SCENE_TRACE").is_some();
}

#[inline]
fn sg_scene_trace() -> bool {
    SG_SCENE_TRACE_ON.with(|on| *on)
}

#[inline]
fn sg_ring_on() -> bool {
    SG_OP_RING_ON.with(|on| *on) && SG_RET_TRACE_ON.with(|on| *on)
}

#[inline]
fn sg_ring_push(
    pc: usize,
    opcode: u8,
    depth: usize,
    elm: usize,
    top: Option<i32>,
    scn_len: usize,
    call_depth: usize,
) {
    SG_OP_RING_ON.with(|on| {
        if !*on {
            return;
        }
        SG_OP_RING.with(|ring| {
            let mut ring = ring.borrow_mut();
            // A different stream (cross-scene call / proc stream) invalidates every
            // recorded pc, so start over rather than mixing two address spaces.
            if ring.back().map(|e| e.5) != Some(scn_len as u32) {
                ring.clear();
            }
            if ring.len() >= SG_RING_CAP {
                ring.pop_front();
            }
            ring.push_back((
                pc as u32,
                opcode,
                depth as u32,
                elm as u32,
                top.map(|v| v as i64).unwrap_or(i64::MIN),
                scn_len as u32,
                call_depth as u32,
            ));
        });
    });
}

const CD_COMMAND: u8 = constants::cd::COMMAND;
const CD_TEXT: u8 = constants::cd::TEXT;
const CD_NAME: u8 = constants::cd::NAME;
const CD_SEL_BLOCK_START: u8 = constants::cd::SEL_BLOCK_START;
const CD_SEL_BLOCK_END: u8 = constants::cd::SEL_BLOCK_END;

const OP_PLUS: u8 = constants::op::PLUS;
const OP_MINUS: u8 = constants::op::MINUS;
const OP_MULTIPLE: u8 = constants::op::MULTIPLE;
const OP_DIVIDE: u8 = constants::op::DIVIDE;
const OP_AMARI: u8 = constants::op::AMARI;

const OP_EQUAL: u8 = constants::op::EQUAL;
const OP_NOT_EQUAL: u8 = constants::op::NOT_EQUAL;
const OP_GREATER: u8 = constants::op::GREATER;
const OP_GREATER_EQUAL: u8 = constants::op::GREATER_EQUAL;
const OP_LESS: u8 = constants::op::LESS;
const OP_LESS_EQUAL: u8 = constants::op::LESS_EQUAL;

const OP_LOGICAL_AND: u8 = constants::op::LOGICAL_AND;
const OP_LOGICAL_OR: u8 = constants::op::LOGICAL_OR;

const OP_TILDE: u8 = constants::op::TILDE;
const OP_AND: u8 = constants::op::AND;
const OP_OR: u8 = constants::op::OR;
const OP_HAT: u8 = constants::op::HAT;
const OP_SL: u8 = constants::op::SL;
const OP_SR: u8 = constants::op::SR;
const OP_SR3: u8 = constants::op::SR3;

// C++ initializes cur_call.L / cur_call.K from Gameexe CALL_FLAG.CNT (default 50).

// -----------------------------------------------------------------------------
// VM configuration (form codes are game-specific, so keep them injectable)
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy)]
pub struct VmConfig {
    pub fm_void: i32,
    pub fm_int: i32,
    pub fm_str: i32,
    pub fm_label: i32,
    pub fm_list: i32,
    pub fm_intlist: i32,
    pub fm_strlist: i32,
    pub max_steps: u64,
}

impl VmConfig {
    pub fn from_env() -> Self {
        fn env_u64(key: &str, default: u64) -> u64 {
            std::env::var(key)
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(default)
        }

        Self {
            fm_void: constants::fm::VOID,
            fm_int: constants::fm::INT,
            fm_str: constants::fm::STR,
            fm_label: constants::fm::LABEL,
            fm_list: constants::fm::LIST,
            fm_intlist: constants::fm::INTLIST,
            fm_strlist: constants::fm::STRLIST,
            max_steps: env_u64("SIGLUS_VM_MAX_STEPS", 0),
        }
    }
}

#[derive(Debug, Clone)]
struct VmTraceConfig {
    enabled: bool,
    scene: Option<String>,
    pc_range: Option<(usize, usize)>,
    commands_enabled: bool,
}

impl VmTraceConfig {
    fn from_env() -> Self {
        let enabled = std::env::var_os("SIGLUS_TRACE_VM").is_some();
        let scene = std::env::var("SIGLUS_TRACE_VM_SCENE")
            .ok()
            .filter(|value| !value.is_empty());
        let pc_range = std::env::var("SIGLUS_TRACE_VM_PC")
            .ok()
            .and_then(|range| {
                let (start, end) = range.split_once("..")?;
                let parse = |value: &str| {
                    usize::from_str_radix(value.trim_start_matches("0x"), 16)
                        .or_else(|_| value.parse::<usize>())
                        .ok()
                };
                Some((parse(start)?, parse(end)?))
            });

        Self {
            enabled,
            scene,
            pc_range,
            commands_enabled: std::env::var_os("SIGLUS_TRACE_VM_COMMANDS").is_some(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct VmRuntimeOptions {
    inline_user_cmd_max_steps: u64,
    frame_action_max_steps: u64,
    trace_unknown_forms: bool,
    proc_flow_trace: bool,
    sg_debug: bool,
    syscom_proc_trace: bool,
    tick_trace: bool,
    frame_action_trace: bool,
    title_chain_trace: bool,
    save_load_trace: bool,
    trace_call_return_pc: bool,
    trace_frame_action_call: bool,
}

impl VmRuntimeOptions {
    fn from_env() -> Self {
        fn env_u64(key: &str) -> u64 {
            std::env::var(key)
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .unwrap_or(0)
        }

        let sg_debug = std::env::var_os("SG_DEBUG").is_some();
        Self {
            inline_user_cmd_max_steps: env_u64("SIGLUS_INLINE_USER_CMD_MAX_STEPS"),
            frame_action_max_steps: env_u64("SIGLUS_FRAME_ACTION_MAX_STEPS"),
            trace_unknown_forms: std::env::var_os("SIGLUS_TRACE_UNKNOWN_FORMS").is_some(),
            proc_flow_trace: std::env::var_os("SG_PROC_FLOW_TRACE").is_some(),
            sg_debug,
            syscom_proc_trace: sg_debug || std::env::var_os("SG_SYSCOM_PROC_TRACE").is_some(),
            tick_trace: std::env::var_os("SG_TICK_TRACE").is_some(),
            frame_action_trace: std::env::var_os("SG_FRAME_ACTION_TRACE").is_some(),
            title_chain_trace: std::env::var_os("SG_TITLE_CHAIN_TRACE").is_some(),
            save_load_trace: std::env::var_os("SG_SAVELOAD_TRACE").is_some(),
            trace_call_return_pc: std::env::var_os("SIGLUS_TRACE_CALL_RETURN_PC").is_some(),
            trace_frame_action_call: std::env::var_os("SIGLUS_TRACE_FRAME_ACTION_CALL").is_some(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct InlineExecCheckpoint {
    scene_no: Option<usize>,
    pc: usize,
    line_no: i32,
    ctx_line_no: i64,
    halted: bool,
    int_len: usize,
    str_len: usize,
    element_point_len: usize,
    call_depth: usize,
    gosub_depth: usize,
    scene_depth: usize,
    caller_return: Option<(usize, i32)>,
}

#[derive(Debug, Clone)]
struct CallProp {
    scn_no: i32,
    prop_id: i32,
    form: i32,
    decl_size: usize,
    element: Vec<i32>,
    value: CallPropValue,
}

#[derive(Debug, Clone)]
enum CallPropValue {
    Int(i32),
    Str(String),
    Element(Vec<i32>),
    IntList(Vec<i32>),
    StrList(Vec<String>),
}

#[derive(Debug, Clone)]
struct CallFrame {
    /// Original E_tnm_call_type on the callee frame: 0 NONE, 1 GOSUB, 2 FARCALL, 3 USER_CMD.
    call_type: i32,
    return_pc: usize,
    // C_elm_call::m_call_save identifies the lexer state owned by this frame.
    // It is also the only scene-boundary metadata available in the original
    // save format.
    return_scene_no: Option<usize>,
    return_scene_name: Option<String>,
    return_line_no: i32,
    ret_form: i32,
    return_override: Option<(usize, i32)>,
    excall_proc: bool,
    frame_action_proc: bool,
    arg_cnt: usize,
    delayed_ret_form: Option<i32>,
    user_props: Vec<CallProp>,
    int_args: Vec<i32>,
    str_args: Vec<String>,
}

#[derive(Debug, Clone)]
struct UserPropCell {
    form: i32,
    int_value: i32,
    str_value: String,
    element: Vec<i32>,
    int_list: Vec<i32>,
    str_list: Vec<String>,
    list_items: Vec<UserPropCell>,
}

impl UserPropCell {
    fn new(form: i32, element: Vec<i32>) -> Self {
        Self {
            form,
            int_value: 0,
            str_value: String::new(),
            element,
            int_list: Vec::new(),
            str_list: Vec::new(),
            list_items: Vec::new(),
        }
    }
}

#[derive(Debug)]
struct SceneExecFrame<'a> {
    // The original engine keeps a single VM stack/call-list across scene calls.
    // Only lexer state and scene-local property selection change.  Keep the
    // caller stream by move (not clone) and leave all VM stacks resident.
    stream: SceneStream<'a>,
    user_cmd_names: Arc<std::collections::HashMap<u32, String>>,
    call_cmd_names: Arc<std::collections::HashMap<u32, String>>,
    current_scene_no: Option<usize>,
    current_scene_name: Option<String>,
    current_line_no: i32,
    /// Call-stack depth after the cross-scene callee frame is pushed.
    /// A RETURN at this exact depth is the scene boundary return.
    call_depth: usize,
}


#[derive(Debug, Clone)]
struct ResolvedUserCommand {
    encoded_no: usize,
    name: String,
    target_scene_no: usize,
    target_offset: usize,
    include_command: bool,
}

fn siglus_name_eq(lhs: &str, rhs: &str) -> bool {
    lhs.eq_ignore_ascii_case(rhs)
}

fn find_named_index(
    names: &std::collections::HashMap<u32, String>,
    target: &str,
) -> Option<usize> {
    names.iter().find_map(|(no, name)| {
        if siglus_name_eq(name, target) {
            Some(*no as usize)
        } else {
            None
        }
    })
}

fn resolve_named_user_command_number(
    include_names: &std::collections::HashMap<u32, String>,
    local_names: &std::collections::HashMap<u32, String>,
    include_count: usize,
    target: &str,
) -> Option<(usize, bool)> {
    if let Some(no) = find_named_index(include_names, target) {
        return Some((no, true));
    }
    find_named_index(local_names, target).map(|no| (include_count + no, false))
}

#[derive(Debug, Clone)]
struct RuntimeDiskSnapshot {
    scene_name: String,
    scene_no: i32,
    line_no: i32,
    pc: i32,
    int_stack: Vec<i32>,
    str_stack: Vec<String>,
    element_points: Vec<usize>,
    call_stack: Vec<CallFrame>,
}

fn resize_i64_vec(mut v: Vec<i64>, n: usize) -> Vec<i64> {
    v.resize(n, 0);
    v
}

fn resize_string_vec(mut v: Vec<String>, n: usize) -> Vec<String> {
    v.resize_with(n, String::new);
    v
}

#[derive(Clone)]
struct VmResumePoint<'a> {
    stream: SceneStream<'a>,
    user_cmd_names: Arc<std::collections::HashMap<u32, String>>,
    call_cmd_names: Arc<std::collections::HashMap<u32, String>>,
    int_stack: Vec<i32>,
    str_stack: Vec<String>,
    element_points: Vec<usize>,
    call_stack: Vec<CallFrame>,
    gosub_return_stack: Vec<(usize, i32)>,
    user_props: BTreeMap<u16, UserPropCell>,
    scene_user_props: BTreeMap<usize, BTreeMap<u16, UserPropCell>>,
    current_scene_no: Option<usize>,
    current_scene_name: Option<String>,
    current_line_no: i32,
    globals: runtime::globals::GlobalState,
}

pub struct SceneVm<'a> {
    pub cfg: VmConfig,
    vm_trace_config: VmTraceConfig,
    runtime_options: VmRuntimeOptions,
    call_flag_count: usize,
    stream: SceneStream<'a>,

    pub ctx: CommandContext,

    // Stack model: separate int/str stacks plus element point list.
    int_stack: Vec<i32>,
    str_stack: Vec<String>,
    element_points: Vec<usize>,

    call_stack: Vec<CallFrame>,
    call_frame_pool: Vec<CallFrame>,
    gosub_return_stack: Vec<(usize, i32)>,
    user_props: BTreeMap<u16, UserPropCell>,
    // Original C++ keeps one scene-property list per scene in
    // Gp_user_scn_prop_list[scene_no]. Only the active scene is projected into
    // user_props; inactive scene-local values remain resident here.
    scene_user_props: BTreeMap<usize, BTreeMap<u16, UserPropCell>>,
    scene_stack: Vec<SceneExecFrame<'a>>,
    save_point: Option<VmResumePoint<'a>>,
    sel_point_stack: Vec<VmResumePoint<'a>>,
    current_scene_no: Option<usize>,
    current_scene_name: Option<String>,
    current_line_no: i32,
    diag_last_scene_no: Option<usize>,
    /// Original saves can contain only the active scene and a base call frame.
    /// In that layout the script may finish while saved frame actions continue
    /// to drive the scene; the Android host must not treat an empty proc flow as
    /// an Activity exit.
    legacy_saved_active_only: bool,

    pub unknown_opcodes: BTreeMap<u8, u64>,
    pub unknown_forms: BTreeMap<i32, u64>,
    // Warn-once dedup for form command chains the runtime cannot dispatch yet.
    // A hard failure here would freeze the host tick loop with no visible
    // symptom, so unhandled chains are counted and skipped instead.

    steps: u64,
    halted: bool,

    // When a command triggers a VM wait (movie wait-key etc.), its return value is produced when the wait completes.
    delayed_ret_form: Option<i32>,
    script_input_synced_this_frame: bool,
    yield_safe_after_step: bool,

    user_cmd_names: Arc<std::collections::HashMap<u32, String>>,
    call_cmd_names: Arc<std::collections::HashMap<u32, String>>,

    // C++ keeps the lexer / scene package resident. Do not reload and rebuild
    // Scene.pck for frame-action callbacks or scene-local user command calls.
    scene_pck_cache: Option<ScenePck>,
    scene_pck_append_dir: Option<String>,
    scene_stream_cache: BTreeMap<usize, SceneStream<'a>>,
    scene_name_resolve_cache: std::collections::HashMap<String, Option<usize>>,
    user_cmd_resolve_cache: std::collections::HashMap<usize, std::collections::HashMap<String, ResolvedUserCommand>>,
}

#[derive(Debug, Clone)]
struct FrameActionWork {
    stage_idx: i64,
    obj_idx: usize,
    ch_idx: Option<usize>,
    global_form_id: Option<u32>,
    object_chain: Option<Vec<i32>>,
    frame_action_chain: Option<Vec<i32>>,
    scn_name: String,
    cmd_name: String,
    args: Vec<Value>,
    count: i64,
    end_time: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FrameActionObjectRoot {
    StageObject {
        obj_idx: usize,
        child_pos: usize,
    },
    MwndObject {
        mwnd_idx: usize,
        selector: i32,
        obj_idx: usize,
        child_pos: usize,
    },
    BtnSelItemObject {
        item_idx: usize,
        obj_idx: usize,
        child_pos: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameActionObjectLocator {
    raw_form_id: i32,
    stage_idx: i64,
    root: FrameActionObjectRoot,
}

fn frame_action_stage_alias_to_index(code: i32) -> Option<i64> {
    if code == crate::runtime::forms::codes::ELM_GLOBAL_BACK {
        Some(0)
    } else if code == crate::runtime::forms::codes::ELM_GLOBAL_FRONT {
        Some(1)
    } else if code == crate::runtime::forms::codes::ELM_GLOBAL_NEXT {
        Some(2)
    } else {
        None
    }
}

fn frame_action_array_marker(code: i32, elm_array: i32) -> bool {
    code == elm_array || code == crate::runtime::forms::codes::ELM_ARRAY || code == -1
}

fn parse_frame_action_object_locator(
    chain: &[i32],
    elm_array: i32,
) -> Option<FrameActionObjectLocator> {
    let raw_form_id = *chain.first()?;
    let (stage_idx, pos) = if let Some(stage_idx) = frame_action_stage_alias_to_index(raw_form_id) {
        (stage_idx, 1usize)
    } else {
        if chain.len() < 4 || !frame_action_array_marker(chain[1], elm_array) {
            return None;
        }
        (chain[2] as i64, 3usize)
    };

    let list_op = *chain.get(pos)?;
    if !frame_action_array_marker(*chain.get(pos + 1)?, elm_array) {
        return None;
    }
    let first_idx = (*chain.get(pos + 2)?).max(0) as usize;

    let root = if list_op == crate::runtime::forms::codes::elm_value::STAGE_OBJECT {
        FrameActionObjectRoot::StageObject {
            obj_idx: first_idx,
            child_pos: pos + 3,
        }
    } else if list_op == crate::runtime::forms::codes::elm_value::STAGE_MWND {
        let selector = *chain.get(pos + 3)?;
        if !matches!(
            selector,
            crate::runtime::forms::codes::elm_value::MWND_OBJECT
                | crate::runtime::forms::codes::elm_value::MWND_BUTTON
                | crate::runtime::forms::codes::elm_value::MWND_FACE
        ) || !frame_action_array_marker(*chain.get(pos + 4)?, elm_array)
        {
            return None;
        }
        FrameActionObjectRoot::MwndObject {
            mwnd_idx: first_idx,
            selector,
            obj_idx: (*chain.get(pos + 5)?).max(0) as usize,
            child_pos: pos + 6,
        }
    } else if list_op == crate::runtime::forms::codes::elm_value::STAGE_BTNSELITEM {
        if *chain.get(pos + 3)?
            != crate::runtime::forms::codes::elm_value::BTNSELITEM_OBJECT
            || !frame_action_array_marker(*chain.get(pos + 4)?, elm_array)
        {
            return None;
        }
        FrameActionObjectRoot::BtnSelItemObject {
            item_idx: first_idx,
            obj_idx: (*chain.get(pos + 5)?).max(0) as usize,
            child_pos: pos + 6,
        }
    } else {
        return None;
    };

    Some(FrameActionObjectLocator {
        raw_form_id,
        stage_idx,
        root,
    })
}

fn frame_action_locator_object_idx(locator: FrameActionObjectLocator) -> usize {
    match locator.root {
        FrameActionObjectRoot::StageObject { obj_idx, .. }
        | FrameActionObjectRoot::MwndObject { obj_idx, .. }
        | FrameActionObjectRoot::BtnSelItemObject { obj_idx, .. } => obj_idx,
    }
}

#[cfg(test)]
mod frame_action_locator_tests {
    use super::*;

    #[test]
    fn front_object_alias_and_canonical_stage_locate_the_same_object() {
        let alias = [
            crate::runtime::forms::codes::ELM_GLOBAL_FRONT,
            crate::runtime::forms::codes::elm_value::STAGE_OBJECT,
            -1,
            115,
        ];
        let canonical = [
            crate::runtime::forms::codes::ELM_GLOBAL_STAGE,
            -1,
            1,
            crate::runtime::forms::codes::elm_value::STAGE_OBJECT,
            -1,
            115,
        ];
        let a = parse_frame_action_object_locator(&alias, -1).unwrap();
        let b = parse_frame_action_object_locator(&canonical, -1).unwrap();
        assert_eq!(a.stage_idx, b.stage_idx);
        assert_eq!(frame_action_locator_object_idx(a), 115);
        assert_eq!(frame_action_locator_object_idx(b), 115);
    }

    #[test]
    fn front_mwnd_button_alias_keeps_both_array_indices() {
        let alias = [
            crate::runtime::forms::codes::ELM_GLOBAL_FRONT,
            crate::runtime::forms::codes::elm_value::STAGE_MWND,
            -1,
            0,
            crate::runtime::forms::codes::elm_value::MWND_BUTTON,
            -1,
            9,
        ];
        let canonical = [
            crate::runtime::forms::codes::ELM_GLOBAL_STAGE,
            -1,
            1,
            crate::runtime::forms::codes::elm_value::STAGE_MWND,
            -1,
            0,
            crate::runtime::forms::codes::elm_value::MWND_BUTTON,
            -1,
            9,
        ];
        let a = parse_frame_action_object_locator(&alias, -1).unwrap();
        let b = parse_frame_action_object_locator(&canonical, -1).unwrap();
        assert_eq!(a.stage_idx, 1);
        assert_eq!(b.stage_idx, 1);
        assert_eq!(frame_action_locator_object_idx(a), 9);
        assert_eq!(frame_action_locator_object_idx(b), 9);
        assert!(matches!(
            a.root,
            FrameActionObjectRoot::MwndObject { mwnd_idx: 0, selector, obj_idx: 9, .. }
                if selector == crate::runtime::forms::codes::elm_value::MWND_BUTTON
        ));
    }

    #[test]
    fn btnselitem_object_child_locator_starts_children_after_object_index() {
        let chain = [
            crate::runtime::forms::codes::ELM_GLOBAL_FRONT,
            crate::runtime::forms::codes::elm_value::STAGE_BTNSELITEM,
            -1,
            2,
            crate::runtime::forms::codes::elm_value::BTNSELITEM_OBJECT,
            -1,
            3,
            crate::runtime::forms::codes::elm_value::OBJECT_CHILD,
            -1,
            4,
        ];
        let locator = parse_frame_action_object_locator(&chain, -1).unwrap();
        assert!(matches!(
            locator.root,
            FrameActionObjectRoot::BtnSelItemObject {
                item_idx: 2,
                obj_idx: 3,
                child_pos: 7
            }
        ));
    }
}

impl<'a> SceneVm<'a> {
    fn trace_unknown_form(&mut self, form_code: i32, site: &str) {
        *self.unknown_forms.entry(form_code).or_insert(0) += 1;
        if self.runtime_options.trace_unknown_forms {
            eprintln!(
                "[vm unknown form] site={} form={} pc=0x{:x}",
                site,
                form_code,
                self.stream.get_prg_cntr()
            );
        }
    }

    fn configured_call_flag_count(ctx: &CommandContext) -> usize {
        ctx.tables
            .gameexe
            .as_ref()
            .and_then(|cfg| {
                cfg.get_usize("#CALL_FLAG.CNT")
                    .or_else(|| cfg.get_usize("CALL_FLAG.CNT"))
            })
            .unwrap_or(50)
            .min(256)
    }

    fn blank_call_int_args(count: usize) -> Vec<i32> {
        vec![0; count]
    }

    fn blank_call_str_args(count: usize) -> Vec<String> {
        vec![String::new(); count]
    }

    fn make_call_frame(
        &self,
        ret_form: i32,
        excall_proc: bool,
        frame_action_proc: bool,
        arg_cnt: usize,
        scratch_args: Option<(Vec<i32>, Vec<String>)>,
    ) -> CallFrame {
        let (int_args, str_args) = scratch_args.unwrap_or_else(|| {
            (
                Self::blank_call_int_args(self.call_flag_count),
                Self::blank_call_str_args(self.call_flag_count),
            )
        });
        CallFrame {
            call_type: 0,
            return_pc: 0,
            return_scene_no: None,
            return_scene_name: None,
            return_line_no: -1,
            ret_form,
            return_override: None,
            excall_proc,
            frame_action_proc,
            arg_cnt,
            delayed_ret_form: None,
            user_props: Vec::new(),
            int_args,
            str_args,
        }
    }

    fn take_call_frame(
        &mut self,
        ret_form: i32,
        excall_proc: bool,
        frame_action_proc: bool,
        arg_cnt: usize,
        scratch_args: Option<(Vec<i32>, Vec<String>)>,
    ) -> CallFrame {
        let Some(mut frame) = self.call_frame_pool.pop() else {
            return self.make_call_frame(
                ret_form,
                excall_proc,
                frame_action_proc,
                arg_cnt,
                scratch_args,
            );
        };

        frame.call_type = 0;
        frame.return_pc = 0;
        frame.return_scene_no = None;
        frame.return_scene_name = None;
        frame.return_line_no = -1;
        frame.ret_form = ret_form;
        frame.return_override = None;
        frame.excall_proc = excall_proc;
        frame.frame_action_proc = frame_action_proc;
        frame.arg_cnt = arg_cnt;
        frame.delayed_ret_form = None;
        frame.user_props.clear();

        if let Some((int_args, str_args)) = scratch_args {
            frame.int_args = int_args;
            frame.str_args = str_args;
        } else {
            frame.int_args.resize(self.call_flag_count, 0);
            frame.int_args.fill(0);
            frame.str_args.resize_with(self.call_flag_count, String::new);
            for value in &mut frame.str_args {
                value.clear();
            }
        }
        frame
    }

    fn recycle_call_frame(&mut self, mut frame: CallFrame) {
        frame.user_props.clear();
        frame.return_override = None;
        frame.delayed_ret_form = None;
        // C_elm_call_list::sub_call() keeps the slot allocated and add_call()
        // reinitializes it on the next call. Keep the Rust frame allocated too
        // instead of reallocating CALL.L / CALL.K on every callback.
        self.call_frame_pool.push(frame);
    }

    fn shared_user_prop_count(&self) -> usize {
        self.scene_pck_cache
            .as_ref()
            .map(|pck| pck.inc_props.len())
            .unwrap_or_else(|| self.stream.header.scn_prop_cnt.max(0) as usize)
    }

    fn stash_current_scene_user_props(&mut self) {
        let Some(scene_no) = self.current_scene_no else {
            return;
        };
        let shared_count = self.shared_user_prop_count();
        let locals = if shared_count > u16::MAX as usize {
            BTreeMap::new()
        } else {
            self.user_props.split_off(&(shared_count as u16))
        };
        // Gp_user_scn_prop_list[scene_no] owns the scene-local cells in the
        // original engine. Move them out of the active projection instead of
        // cloning the complete property tree on every FARCALL/frame action.
        self.scene_user_props.insert(scene_no, locals);
    }

    fn activate_scene_user_prop_scope(&mut self, scene_no: usize) {
        let shared_count = self.shared_user_prop_count();
        // Callers always stash before switching scenes. Keep this defensive
        // trim so a malformed transition cannot expose the previous scene's
        // local properties under the target scene.
        if shared_count <= u16::MAX as usize {
            let _ = self.user_props.split_off(&(shared_count as u16));
        }
        if let Some(mut locals) = self.scene_user_props.remove(&scene_no) {
            self.user_props.append(&mut locals);
        }
    }

    fn enter_cross_scene_user_prop_scope(&mut self, target_scene_no: usize) {
        self.stash_current_scene_user_props();
        self.activate_scene_user_prop_scope(target_scene_no);
    }

    fn restore_cross_scene_user_prop_scope(&mut self, caller_scene_no: Option<usize>) {
        // current_scene_no still identifies the target here. Store its locals,
        // then reactivate the caller's resident locals. Shared include
        // properties never leave self.user_props and therefore need no clone.
        self.stash_current_scene_user_props();
        if let Some(scene_no) = caller_scene_no {
            self.activate_scene_user_prop_scope(scene_no);
        }
    }

    fn inline_exec_checkpoint(&self) -> InlineExecCheckpoint {
        InlineExecCheckpoint {
            scene_no: self.current_scene_no,
            pc: self.stream.get_prg_cntr(),
            line_no: self.current_line_no,
            ctx_line_no: self.ctx.current_line_no,
            halted: self.halted,
            int_len: self.int_stack.len(),
            str_len: self.str_stack.len(),
            element_point_len: self.element_points.len(),
            call_depth: self.call_stack.len(),
            gosub_depth: self.gosub_return_stack.len(),
            scene_depth: self.scene_stack.len(),
            caller_return: self.call_stack.last().map(|frame| (frame.return_pc, frame.ret_form)),
        }
    }

    fn restore_inline_exec_checkpoint(&mut self, checkpoint: InlineExecCheckpoint) -> Result<()> {
        if self.current_scene_no != checkpoint.scene_no {
            return Ok(());
        }
        if self.int_stack.len() < checkpoint.int_len
            || self.str_stack.len() < checkpoint.str_len
            || self.element_points.len() < checkpoint.element_point_len
            || self.call_stack.len() < checkpoint.call_depth
            || self.gosub_return_stack.len() < checkpoint.gosub_depth
            || self.scene_stack.len() < checkpoint.scene_depth
        {
            bail!(
                "inline user command corrupted caller execution state: scene={:?} int={}/{} str={}/{} elm={}/{} call={}/{} gosub={}/{} scene_stack={}/{}",
                checkpoint.scene_no,
                self.int_stack.len(), checkpoint.int_len,
                self.str_stack.len(), checkpoint.str_len,
                self.element_points.len(), checkpoint.element_point_len,
                self.call_stack.len(), checkpoint.call_depth,
                self.gosub_return_stack.len(), checkpoint.gosub_depth,
                self.scene_stack.len(), checkpoint.scene_depth,
            );
        }

        self.int_stack.truncate(checkpoint.int_len);
        self.str_stack.truncate(checkpoint.str_len);
        self.element_points.truncate(checkpoint.element_point_len);
        while self.call_stack.len() > checkpoint.call_depth {
            if let Some(frame) = self.call_stack.pop() {
                self.recycle_call_frame(frame);
            }
        }
        self.gosub_return_stack.truncate(checkpoint.gosub_depth);
        self.scene_stack.truncate(checkpoint.scene_depth);
        if let (Some((return_pc, ret_form)), Some(caller)) =
            (checkpoint.caller_return, self.call_stack.last_mut())
        {
            caller.return_pc = return_pc;
            caller.ret_form = ret_form;
        }
        self.current_line_no = checkpoint.line_no;
        self.ctx.current_line_no = checkpoint.ctx_line_no;
        self.halted = checkpoint.halted;
        self.stream.set_prg_cntr(checkpoint.pc)?;
        Ok(())
    }

    pub fn new(stream: SceneStream<'a>, ctx: CommandContext) -> Self {
        let cfg = VmConfig::from_env();
        let vm_trace_config = VmTraceConfig::from_env();
        let runtime_options = VmRuntimeOptions::from_env();
        let call_flag_count = Self::configured_call_flag_count(&ctx);
        let user_cmd_names = stream.scn_cmd_name_map.clone();
        let base_call = CallFrame {
            call_type: 0,
            return_pc: 0,
            return_scene_no: None,
            return_scene_name: None,
            return_line_no: -1,
            ret_form: cfg.fm_void,
            return_override: None,
            excall_proc: false,
            frame_action_proc: false,
            arg_cnt: 0,
            delayed_ret_form: None,
            user_props: Vec::new(),
            int_args: Self::blank_call_int_args(call_flag_count),
            str_args: Self::blank_call_str_args(call_flag_count),
        };
        Self {
            cfg,
            vm_trace_config,
            runtime_options,
            call_flag_count,
            stream,
            ctx,
            int_stack: Vec::new(),
            str_stack: Vec::new(),
            element_points: Vec::new(),
            call_stack: vec![base_call],
            call_frame_pool: Vec::new(),
            gosub_return_stack: Vec::new(),
            user_props: BTreeMap::new(),
            scene_user_props: BTreeMap::new(),
            scene_stack: Vec::new(),
            save_point: None,
            sel_point_stack: Vec::new(),
            current_scene_no: None,
            current_scene_name: None,
            current_line_no: -1,
            diag_last_scene_no: None,
            legacy_saved_active_only: false,
            unknown_opcodes: BTreeMap::new(),
            unknown_forms: BTreeMap::new(),

            steps: 0,
            halted: false,
            delayed_ret_form: None,
            script_input_synced_this_frame: false,
            yield_safe_after_step: false,
            user_cmd_names,
            call_cmd_names: Arc::default(),
            scene_pck_cache: None,
            scene_pck_append_dir: None,
            scene_stream_cache: BTreeMap::new(),
            scene_name_resolve_cache: std::collections::HashMap::new(),
            user_cmd_resolve_cache: std::collections::HashMap::new(),
        }
    }

    pub fn with_config(cfg: VmConfig, stream: SceneStream<'a>, ctx: CommandContext) -> Self {
        let vm_trace_config = VmTraceConfig::from_env();
        let runtime_options = VmRuntimeOptions::from_env();
        let call_flag_count = Self::configured_call_flag_count(&ctx);
        let user_cmd_names = stream.scn_cmd_name_map.clone();
        let base_call = CallFrame {
            call_type: 0,
            return_pc: 0,
            return_scene_no: None,
            return_scene_name: None,
            return_line_no: -1,
            ret_form: cfg.fm_void,
            return_override: None,
            excall_proc: false,
            frame_action_proc: false,
            arg_cnt: 0,
            delayed_ret_form: None,
            user_props: Vec::new(),
            int_args: Self::blank_call_int_args(call_flag_count),
            str_args: Self::blank_call_str_args(call_flag_count),
        };
        Self {
            cfg,
            vm_trace_config,
            runtime_options,
            call_flag_count,
            stream,
            ctx,
            int_stack: Vec::new(),
            str_stack: Vec::new(),
            element_points: Vec::new(),
            call_stack: vec![base_call],
            call_frame_pool: Vec::new(),
            gosub_return_stack: Vec::new(),
            user_props: BTreeMap::new(),
            scene_user_props: BTreeMap::new(),
            scene_stack: Vec::new(),
            save_point: None,
            sel_point_stack: Vec::new(),
            current_scene_no: None,
            current_scene_name: None,
            current_line_no: -1,
            diag_last_scene_no: None,
            legacy_saved_active_only: false,
            unknown_opcodes: BTreeMap::new(),
            unknown_forms: BTreeMap::new(),

            steps: 0,
            halted: false,
            delayed_ret_form: None,
            script_input_synced_this_frame: false,
            yield_safe_after_step: false,
            user_cmd_names,
            call_cmd_names: Arc::default(),
            scene_pck_cache: None,
            scene_pck_append_dir: None,
            scene_stream_cache: BTreeMap::new(),
            scene_name_resolve_cache: std::collections::HashMap::new(),
            user_cmd_resolve_cache: std::collections::HashMap::new(),
        }
    }

    pub fn is_blocked(&mut self) -> bool {
        self.ctx.wait_poll()
    }

    pub fn is_halted(&self) -> bool {
        self.halted
    }

    pub fn legacy_saved_active_only(&self) -> bool {
        self.legacy_saved_active_only
    }

    pub fn proc_generation(&self) -> u64 {
        self.ctx.proc_generation()
    }

    pub fn last_proc_kind(&self) -> runtime::ProcKind {
        self.ctx.last_proc_kind()
    }

    pub fn current_scene_name(&self) -> Option<&str> {
        self.current_scene_name.as_deref()
    }

    pub fn current_line_no(&self) -> i32 {
        self.current_line_no
    }

    pub fn current_scene_no(&self) -> Option<usize> {
        self.current_scene_no
    }

    pub fn take_runtime_load_completed(&mut self) -> bool {
        self.ctx.take_runtime_load_completed()
    }

    pub fn call_syscom_configured_scene(&mut self, key: &str) -> Result<bool> {
        // Match the original C++ Gp_ini fields: SAVE_SCENE, LOAD_SCENE and
        // CONFIG_SCENE store both scene name and z label number.  GameexeConfig
        // get_unquoted() returns only the first item, so using it here loses the
        // z value and incorrectly calls sys10_sc00,0.  Rewrite's Gameexe has:
        //   #SAVE_SCENE   = "sys10_sc00",02
        //   #LOAD_SCENE   = "sys10_sc00",03
        //   #CONFIG_SCENE = "sys10_sc00",04
        // The original calls tnm_scene_proc_farcall(name, z, FM_VOID, true, false).
        let entry = self
            .ctx
            .tables
            .gameexe
            .as_ref()
            .and_then(|cfg| cfg.get_entry(key).or_else(|| cfg.get_entry(&format!("#{key}"))));
        let Some(entry) = entry else {
            if self.runtime_options.proc_flow_trace {
                eprintln!(
                    "[SG_PROC_FLOW] syscom_config_scene key={} raw=<missing> scene={:?} line={} pending_proc={:?}",
                    key,
                    self.current_scene_name.as_deref(),
                    self.current_line_no,
                    self.ctx.globals.syscom.pending_proc
                );
            }
            return Ok(false);
        };

        let scene_name = entry
            .item_unquoted(0)
            .map(|s| s.trim().trim_matches('\"').trim().to_string())
            .unwrap_or_default();
        let z_no = entry
            .item_unquoted(1)
            .and_then(|s| s.trim().parse::<i32>().ok())
            .unwrap_or(0);
        let raw = format!("{scene_name},{z_no}");

        if scene_name.is_empty() {
            if self.runtime_options.proc_flow_trace {
                eprintln!(
                    "[SG_PROC_FLOW] syscom_config_scene key={} raw={:?} target=<empty> scene={:?} line={}",
                    key,
                    raw,
                    self.current_scene_name.as_deref(),
                    self.current_line_no
                );
            }
            return Ok(false);
        }

        if self.runtime_options.proc_flow_trace {
            eprintln!(
                "[SG_PROC_FLOW] syscom_config_scene key={} raw={:?} target={} z={} before_scene={:?} line={} scene_stack={} call_depth={}",
                key,
                raw,
                scene_name,
                z_no,
                self.current_scene_name.as_deref(),
                self.current_line_no,
                self.scene_stack.len(),
                self.call_stack.len()
            );
        }
        self.farcall_scene_name_ex(&scene_name, z_no, self.cfg.fm_void, true, &[])?;
        if self.runtime_options.proc_flow_trace {
            eprintln!(
                "[SG_PROC_FLOW] syscom_config_scene entered key={} now_scene={:?} line={} scene_stack={} call_depth={}",
                key,
                self.current_scene_name.as_deref(),
                self.current_line_no,
                self.scene_stack.len(),
                self.call_stack.len()
            );
        }
        Ok(true)
    }

    #[inline(always)]
    fn vm_trace_matches(&self) -> bool {
        let config = &self.vm_trace_config;
        if !config.enabled {
            return false;
        }
        if let Some(filter) = config.scene.as_deref() {
            if self.current_scene_name.as_deref() != Some(filter) {
                return false;
            }
        }
        if let Some((start, end)) = config.pc_range {
            let pc = self.stream.get_prg_cntr();
            if pc < start || pc > end {
                return false;
            }
        }
        true
    }

    fn vm_trace_stack_summary(&self) -> String {
        let mut out = String::new();
        let int_tail_start = self.int_stack.len().saturating_sub(8);
        let int_tail = &self.int_stack[int_tail_start..];
        let _ = write!(
            &mut out,
            "call_depth={} int_len={} str_len={} elm_points={:?} int_tail={:?}",
            self.call_stack.len(),
            self.int_stack.len(),
            self.str_stack.len(),
            self.element_points,
            int_tail
        );
        if let Some(last) = self.str_stack.last() {
            let preview = if last.chars().count() > 48 {
                let mut tmp = last.chars().take(48).collect::<String>();
                tmp.push('…');
                tmp
            } else {
                last.clone()
            };
            let _ = write!(&mut out, " str_top={:?}", preview);
        }
        out
    }

    fn vm_trace_emit(&self, pc: Option<usize>, msg: impl std::fmt::Display) {
        let scene = self.current_scene_name.as_deref().unwrap_or("<none>");
        let scene_no = self
            .current_scene_no
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string());
        let pc_text = pc
            .map(|v| format!("0x{v:x}"))
            .unwrap_or_else(|| "-".to_string());
        eprintln!(
            "[SG_VM_TRACE] scene={} scene_no={} line={} pc={} {} | {}",
            scene,
            scene_no,
            self.current_line_no,
            pc_text,
            msg,
            self.vm_trace_stack_summary()
        );
    }

    fn vm_opcode_name(opcode: u8) -> &'static str {
        match opcode {
            CD_NONE => "NONE",
            CD_NL => "NL",
            CD_PUSH => "PUSH",
            CD_POP => "POP",
            CD_COPY => "COPY",
            CD_PROPERTY => "PROPERTY",
            CD_COPY_ELM => "COPY_ELM",
            CD_DEC_PROP => "DEC_PROP",
            CD_ELM_POINT => "ELM_POINT",
            CD_ARG => "ARG",
            CD_GOTO => "GOTO",
            CD_GOTO_TRUE => "GOTO_TRUE",
            CD_GOTO_FALSE => "GOTO_FALSE",
            CD_GOSUB => "GOSUB",
            CD_GOSUBSTR => "GOSUBSTR",
            CD_RETURN => "RETURN",
            CD_EOF => "EOF",
            CD_ASSIGN => "ASSIGN",
            CD_OPERATE_1 => "OPERATE_1",
            CD_OPERATE_2 => "OPERATE_2",
            CD_COMMAND => "COMMAND",
            CD_TEXT => "TEXT",
            CD_NAME => "NAME",
            CD_SEL_BLOCK_START => "SEL_BLOCK_START",
            CD_SEL_BLOCK_END => "SEL_BLOCK_END",
            _ => "UNKNOWN",
        }
    }

    /// Render the SG_VM_RING buffer oldest -> newest for the `pop_int` underflow
    /// report. Read the `depth=` column downwards: the first entry whose depth is
    /// lower than the entry above it is the instruction that consumed the value.
    fn sg_ring_dump(&self) -> String {
        SG_OP_RING.with(|ring| {
            let ring = ring.borrow();
            if ring.is_empty() {
                return "\n    [no SG_VM_RING data: rerun with SG_VM_RING=1]".to_string();
            }
            let mut out = String::with_capacity(ring.len() * 72);
            for (pc, opcode, depth, elm, top, _scn_len, call_depth) in ring.iter() {
                let top = if *top == i64::MIN {
                    "<empty>".to_string()
                } else {
                    top.to_string()
                };
                out.push_str(&format!(
                    "\n    pc=0x{:x} {:<16} depth={} elm_pts={} call={} top={}",
                    pc,
                    Self::vm_opcode_name(*opcode as u8),
                    depth,
                    elm,
                    call_depth,
                    top
                ));
            }
            out
        })
    }

    #[inline(always)]
    fn vm_trace_opcode(&self, pc: usize, opcode: u8, phase: &str) {
        vm_trace!(
            self,
            Some(pc),
            format!(
                "{} opcode={}({:#04x})",
                phase,
                Self::vm_opcode_name(opcode),
                opcode
            )
        );
    }
    #[inline(always)]
    fn sg_debug_enabled(&self) -> bool {
        self.runtime_options.sg_debug
    }

    fn sg_cgm_coord_trace_emit(&self, msg: impl std::fmt::Display) {
        let scene = self.current_scene_name.as_deref().unwrap_or("<none>");
        let scene_no = self
            .current_scene_no
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string());
        eprintln!(
            "[SG_DEBUG][CGM_COORD_TRACE][VM] scene={} scene_no={} line={} pc=0x{:x} {}",
            scene,
            scene_no,
            self.current_line_no,
            self.stream.get_prg_cntr(),
            msg
        );
    }

    fn trace_cgm_coord_assign(&self, elm: &[i32], rhs: &Value) {
        if !self.sg_debug_enabled() || elm.len() < 3 {
            return;
        }
        let array_op = if self.ctx.ids.elm_array != 0 {
            self.ctx.ids.elm_array
        } else {
            crate::runtime::forms::codes::ELM_ARRAY
        };
        if elm[1] != array_op {
            return;
        }
        let head = elm[0] as u32;
        let idx = elm[2];
        if head == crate::runtime::forms::codes::elm_value::GLOBAL_B as u32 {
            let interesting = (100..=129).contains(&idx)
                || (140..=169).contains(&idx)
                || (180..=209).contains(&idx);
            if interesting {
                self.sg_cgm_coord_trace_emit(format_args!("global B[{}] <- {:?}", idx, rhs));
            }
        } else if head == crate::runtime::forms::codes::elm_value::GLOBAL_S as u32
            && (1120..=1139).contains(&idx)
        {
            self.sg_cgm_coord_trace_emit(format_args!("global S[{}] <- {:?}", idx, rhs));
        }
    }


    fn cf_branch_trace_interesting_line(&self) -> bool {
        if !self.sg_debug_enabled()
            || self.current_scene_name.as_deref() != Some("sys10_cf01")
        {
            return false;
        }
        matches!(self.current_line_no, 700..=730 | 870..=895)
    }

    fn cf_condition_trace_interesting_line(&self) -> bool {
        if !self.sg_debug_enabled() {
            return false;
        }
        matches!(
            self.current_scene_name.as_deref(),
            Some("sys10_cf01")
        ) && matches!(self.current_line_no, 700..=730 | 870..=895)
    }

    fn cf_condition_trace_prop_name(prop_id: u16) -> Option<&'static str> {
        match prop_id {
            14 => Some("ip_mx"),
            15 => Some("ip_my"),
            16 => Some("ip_wheel"),
            18 => Some("ip_bl_is"),
            19 => Some("ip_br_is"),
            20 => Some("ip_bl_on"),
            21 => Some("ip_br_on"),
            22 => Some("ip_key_enable_enter"),
            23 => Some("ip_key_enable_esc"),
            24 => Some("ip_key_is_enter"),
            25 => Some("ip_key_is_esc"),
            26 => Some("ip_key_on_enter"),
            27 => Some("ip_key_on_esc"),
            39 => Some("cntr_now"),
            40 => Some("cntr_exit"),
            41 => Some("skip_flag"),
            _ => None,
        }
    }

    fn cf_condition_trace_value_summary(&self, cell: &UserPropCell, array_idx: Option<usize>) -> String {
        if let Some(idx) = array_idx {
            if cell.form == self.cfg.fm_intlist {
                return format!("intlist[{}]={}", idx, cell.int_list.get(idx).copied().unwrap_or(0));
            }
            if cell.form == self.cfg.fm_strlist {
                return format!("strlist[{}]={:?}", idx, cell.str_list.get(idx).cloned().unwrap_or_default());
            }
            if let Some(slot) = cell.list_items.get(idx) {
                return format!("list[{}] form={} int={} str={:?} int_list_len={} str_list_len={} items={}",
                    idx,
                    slot.form,
                    slot.int_value,
                    slot.str_value,
                    slot.int_list.len(),
                    slot.str_list.len(),
                    slot.list_items.len()
                );
            }
            return format!("array[{}] <missing> form={} int_list_len={} str_list_len={} items={}",
                idx, cell.form, cell.int_list.len(), cell.str_list.len(), cell.list_items.len());
        }
        if cell.form == self.cfg.fm_int {
            return format!("int={}", cell.int_value);
        }
        if cell.form == self.cfg.fm_str {
            return format!("str={:?}", cell.str_value);
        }
        if cell.form == self.cfg.fm_intlist {
            let preview = cell.int_list.iter().take(20).copied().collect::<Vec<_>>();
            return format!("intlist len={} head={:?}", cell.int_list.len(), preview);
        }
        if cell.form == self.cfg.fm_strlist {
            let preview = cell.str_list.iter().take(6).cloned().collect::<Vec<_>>();
            return format!("strlist len={} head={:?}", cell.str_list.len(), preview);
        }
        format!("form={} int={} str={:?} int_list_len={} str_list_len={} items={}",
            cell.form, cell.int_value, cell.str_value, cell.int_list.len(), cell.str_list.len(), cell.list_items.len())
    }

    fn sg_cf_condition_trace(&self, pc: usize, msg: impl AsRef<str>) {
        if !self.sg_debug_enabled() {
            return;
        }
        let scene = self.current_scene_name.as_deref().unwrap_or("<none>");
        let scene_no = self
            .current_scene_no
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string());
        let int_tail_start = self.int_stack.len().saturating_sub(12);
        let int_tail = &self.int_stack[int_tail_start..];
        eprintln!(
            "[SG_DEBUG][CF_CONDITION_TRACE] scene={} scene_no={} line={} pc=0x{:x} {} | int_tail={:?}",
            scene,
            scene_no,
            self.current_line_no,
            pc,
            msg.as_ref(),
            int_tail
        );
    }

    fn trace_cf_condition_user_prop_read(&self, pc: usize, prop_id: u16, array_idx: Option<usize>, cell: &UserPropCell, elm: &[i32]) {
        if !self.cf_condition_trace_interesting_line() {
            return;
        }
        let Some(name) = Self::cf_condition_trace_prop_name(prop_id) else {
            return;
        };
        self.sg_cf_condition_trace(
            pc,
            format!(
                "kind=USER_PROP_READ prop={}({}) array={:?} value={} elm={:?}",
                prop_id,
                name,
                array_idx,
                self.cf_condition_trace_value_summary(cell, array_idx),
                elm
            ),
        );
    }

    fn trace_cf_condition_user_prop_assign(&self, pc: usize, prop_id: u16, array_idx: Option<usize>, old: Option<&UserPropCell>, new: Option<&UserPropCell>, rhs: &Value, elm: &[i32]) {
        if !self.cf_condition_trace_interesting_line() {
            return;
        }
        let Some(name) = Self::cf_condition_trace_prop_name(prop_id) else {
            return;
        };
        let old_summary = old
            .map(|cell| self.cf_condition_trace_value_summary(cell, array_idx))
            .unwrap_or_else(|| "<default/missing>".to_string());
        let new_summary = new
            .map(|cell| self.cf_condition_trace_value_summary(cell, array_idx))
            .unwrap_or_else(|| "<missing>".to_string());
        self.sg_cf_condition_trace(
            pc,
            format!(
                "kind=USER_PROP_ASSIGN prop={}({}) array={:?} old={} new={} rhs={:?} elm={:?}",
                prop_id,
                name,
                array_idx,
                old_summary,
                new_summary,
                rhs,
                elm
            ),
        );
    }

    fn cf_condition_op_name(opr: u8) -> &'static str {
        match opr {
            OP_PLUS => "+",
            OP_MINUS => "-",
            OP_MULTIPLE => "*",
            OP_DIVIDE => "/",
            OP_AMARI => "%",
            OP_EQUAL => "==",
            OP_NOT_EQUAL => "!=",
            OP_GREATER => ">",
            OP_GREATER_EQUAL => ">=",
            OP_LESS => "<",
            OP_LESS_EQUAL => "<=",
            OP_LOGICAL_AND => "&&",
            OP_LOGICAL_OR => "||",
            OP_TILDE => "~",
            OP_AND => "&",
            OP_OR => "|",
            OP_HAT => "^",
            OP_SL => "<<",
            OP_SR => ">>",
            OP_SR3 => ">>>",
            _ => "?",
        }
    }

    fn cf_branch_trace_stack_snapshot(&self) -> String {
        let int_tail_start = self.int_stack.len().saturating_sub(16);
        let int_tail = &self.int_stack[int_tail_start..];
        let str_tail_start = self.str_stack.len().saturating_sub(4);
        let str_tail = &self.str_stack[str_tail_start..];
        let (cur_l, cur_s, arg_cnt) = if let Some(frame) = self.call_stack.last() {
            let l_take = frame.int_args.len().min(16);
            let s_take = frame.str_args.len().min(6);
            (
                format!("{:?}", &frame.int_args[..l_take]),
                format!("{:?}", &frame.str_args[..s_take]),
                frame.arg_cnt,
            )
        } else {
            ("[]".to_string(), "[]".to_string(), 0)
        };
        format!(
            "int_len={} int_tail={:?} str_len={} str_tail={:?} elm_points={:?} call_depth={} arg_cnt={} cur_call_l0_15={} cur_call_s0_5={}",
            self.int_stack.len(),
            int_tail,
            self.str_stack.len(),
            str_tail,
            self.element_points,
            self.call_stack.len(),
            arg_cnt,
            cur_l,
            cur_s,
        )
    }

    fn sg_cf_branch_trace_emit(&self, pc: usize, msg: impl std::fmt::Display) {
        let scene = self.current_scene_name.as_deref().unwrap_or("<none>");
        let scene_no = self
            .current_scene_no
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string());
        eprintln!(
            "[SG_DEBUG][CF_BRANCH_TRACE] scene={} scene_no={} line={} pc=0x{:x} {} | {}",
            scene,
            scene_no,
            self.current_line_no,
            pc,
            msg,
            self.cf_branch_trace_stack_snapshot(),
        );
    }

    fn trace_cf_branch_goto(
        &self,
        pc: usize,
        opcode_name: &str,
        label_no: i32,
        cond: i32,
        taken: bool,
        before_tail: &[i32],
    ) {
        if self.cf_branch_trace_interesting_line() {
            self.sg_cf_branch_trace_emit(
                pc,
                format_args!(
                    "kind=GOTO opcode={} label={} cond={} taken={} before_int_tail={:?}",
                    opcode_name, label_no, cond, taken, before_tail
                ),
            );
        }
    }

    fn trace_cf_branch_farcall(
        &self,
        pc: usize,
        scene_name: &str,
        z_no: i32,
        ret_form: i32,
        ex_call_proc: bool,
        scratch_source_args: &[Value],
    ) {
        if !self.sg_debug_enabled()
            || !(self.current_scene_name.as_deref() == Some("sys10_cf01")
                && matches!(self.current_line_no, 700..=730 | 870..=895)
                && matches!(scene_name, "sys10_sm00" | "sys10_cf00")
                && matches!(z_no, 14 | 15))
        {
            return;
        }
        let args_dbg = scratch_source_args
            .iter()
            .map(|v| format!("{v:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        self.sg_cf_branch_trace_emit(
            pc,
            format_args!(
                "kind=FARCALL target={} z={} ret_form={} ex_call_proc={} argc={} args=[{}]",
                scene_name,
                z_no,
                ret_form,
                ex_call_proc,
                scratch_source_args.len(),
                args_dbg
            ),
        );
    }

    fn sg_omv_trace_emit(&self, msg: impl std::fmt::Display) {
        let scene = self.current_scene_name.as_deref().unwrap_or("<none>");
        let scene_no = self
            .current_scene_no
            .map(|v| v.to_string())
            .unwrap_or_else(|| "-".to_string());
        eprintln!(
            "[SG_DEBUG][OMV_TRACE] scene={} scene_no={} line={} pc=0x{:x} {}",
            scene,
            scene_no,
            self.current_line_no,
            self.stream.get_prg_cntr(),
            msg
        );
    }

    fn sg_omv_trace_command(
        &self,
        phase: &str,
        elm: &[i32],
        form_id: i32,
        op_id: i32,
        al_id: i32,
        ret_form: i32,
        args: &[Value],
    ) {
        if !self.sg_debug_enabled() {
            return;
        }

        let label = if form_id == crate::runtime::forms::codes::elm_value::GLOBAL_JUMP
            || (form_id == crate::runtime::forms::codes::FM_GLOBAL
                && op_id == crate::runtime::forms::codes::elm_value::GLOBAL_JUMP)
        {
            Some("GLOBAL.JUMP")
        } else if form_id == crate::runtime::forms::codes::elm_value::GLOBAL_FARCALL
            || (form_id == crate::runtime::forms::codes::FM_GLOBAL
                && op_id == crate::runtime::forms::codes::elm_value::GLOBAL_FARCALL)
        {
            Some("GLOBAL.FARCALL")
        } else if (form_id as u32 == constants::global_form::SYSCOM || form_id == constants::fm::SYSCOM)
            && op_id == crate::runtime::forms::codes::elm_value::SYSCOM_CALL_EX
        {
            Some("SYSCOM.CALL_EX")
        } else if form_id as u32 == constants::global_form::MOV || form_id == constants::fm::MOV {
            Some("MOV")
        } else if form_id == constants::fm::OBJECT
            && matches!(
                op_id,
                crate::runtime::forms::codes::object_op::CREATE_MOVIE
                    | crate::runtime::forms::codes::object_op::CREATE_MOVIE_LOOP
                    | crate::runtime::forms::codes::object_op::CREATE_MOVIE_WAIT
                    | crate::runtime::forms::codes::object_op::CREATE_MOVIE_WAIT_KEY
            )
        {
            Some("OBJECT.CREATE_MOVIE")
        } else {
            None
        };
        let Some(label) = label else {
            return;
        };

        let args_dbg = args
            .iter()
            .take(8)
            .map(|v| format!("{v:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        self.sg_omv_trace_emit(format_args!(
            "{} {} form={} op={} al_id={} ret_form={} elm={:?} argc={} args=[{}]",
            phase,
            label,
            form_id,
            op_id,
            al_id,
            ret_form,
            elm,
            args.len(),
            args_dbg
        ));
    }


    fn vm_scn_cmd_context(&self, pc: usize) -> String {
        let cnt = self.stream.header.scn_cmd_cnt.max(0) as usize;
        let mut prev: Option<(usize, usize)> = None;
        let mut next: Option<(usize, usize)> = None;
        for cmd_no in 0..cnt {
            let Ok(off) = self.stream.scn_cmd_offset(cmd_no) else {
                continue;
            };
            if off <= pc {
                prev = Some(match prev {
                    Some(cur) if cur.1 > off => cur,
                    _ => (cmd_no, off),
                });
            }
            if off > pc {
                next = Some(match next {
                    Some(cur) if cur.1 < off => cur,
                    _ => (cmd_no, off),
                });
            }
        }

        let mut out = String::new();
        if let Some((cmd_no, off)) = prev {
            let name = self.stream.scn_cmd_name_map.get(&(cmd_no as u32)).map(String::as_str).unwrap_or("<unnamed>");
            let _ = write!(&mut out, "prev_scn_cmd=#{}:{}@0x{:x} delta={} ", cmd_no, name, off, pc.saturating_sub(off));
        } else {
            let _ = write!(&mut out, "prev_scn_cmd=<none> " );
        }
        if let Some((cmd_no, off)) = next {
            let name = self.stream.scn_cmd_name_map.get(&(cmd_no as u32)).map(String::as_str).unwrap_or("<unnamed>");
            let _ = write!(&mut out, "next_scn_cmd=#{}:{}@0x{:x} distance={}", cmd_no, name, off, off.saturating_sub(pc));
        } else {
            let _ = write!(&mut out, "next_scn_cmd=<none>" );
        }
        out
    }

    pub fn take_script_proc_request(&mut self) -> bool {
        let requested = self.ctx.excall_state.script_proc_requested;
        self.ctx.excall_state.script_proc_requested = false;
        requested
    }

    pub fn take_script_proc_pop_request(&mut self) -> bool {
        let requested = self.ctx.excall_state.script_proc_pop_requested;
        self.ctx.excall_state.script_proc_pop_requested = false;
        requested
    }

    fn mark_excall_script_proc_requested(&mut self) {
        self.halted = false;
        self.ctx.excall_state.ex_call_flag = true;
        self.ctx.excall_state.script_proc_requested = true;
        // Original tnm_scene_proc_farcall(..., ex_call=true) pushes a
        // TNM_PROC_TYPE_SCRIPT proc immediately.  Make that process-stack
        // transition visible to the host at this exact instruction boundary.
        self.ctx.request_proc_boundary(runtime::ProcKind::Script);
    }

    fn mark_excall_script_proc_pop_requested(&mut self) {
        self.ctx.excall_state.ex_call_flag = false;
        self.ctx.excall_state.script_proc_pop_requested = true;
        self.ctx.input.clear_all();
        // Original tnm_scene_proc_return() pops the EXCALL SCRIPT proc before
        // resuming the caller.  If Rust keeps executing here, caller script can
        // create a new wait while the menu EXCALL is still on FlowState; the
        // later wait restore then overwrites that new wait and leaves repeated
        // SAVE/LOAD/CONFIG entry out of sync.
        self.ctx.request_proc_boundary(runtime::ProcKind::Script);
    }

    fn push_call_arg_value(&mut self, arg: &Value) {
        match arg {
            Value::NamedArg { value, .. } => self.push_call_arg_value(value),
            Value::Int(n) => self.push_int(*n as i32),
            Value::Str(s) => self.push_str(s.clone()),
            Value::Element(elm) => self.push_element(elm.clone()),
            Value::List(items) => {
                for item in items {
                    self.push_call_arg_value(item);
                }
            }
        }
    }

    fn run_user_cmd_inline_at_offset(
        &mut self,
        cmd_name: &str,
        offset: usize,
        return_pc: usize,
        end_offset: Option<usize>,
        _expected_return_pc: Option<usize>,
        ret_form: i32,
        call_args: &[Value],
        frame_action_proc: bool,
    ) -> Result<bool> {
        let checkpoint = self.inline_exec_checkpoint();
        let base_depth = checkpoint.call_depth;

        if let Some(caller) = self.call_stack.last_mut() {
            if self.runtime_options.trace_call_return_pc {
                eprintln!(
                    "[SG_CALL_PC] inline set cmd={} depth={} saved_pc=0x{:x} return_pc=0x{:x} old=0x{:x}",
                    cmd_name,
                    base_depth,
                    checkpoint.pc,
                    return_pc,
                    caller.return_pc
                );
            }
            caller.return_pc = return_pc;
            caller.return_scene_no = self.current_scene_no;
            caller.return_scene_name = self.current_scene_name.clone();
            caller.return_line_no = self.current_line_no;
            caller.ret_form = ret_form;
        }
        for arg in call_args {
            self.push_call_arg_value(arg);
        }
        let mut call_frame = self.take_call_frame(
            self.cfg.fm_void,
            false,
            frame_action_proc,
            call_args.len(),
            None,
        );
        call_frame.call_type = 3;
        call_frame.return_override = Some((return_pc, ret_form));
        self.call_stack.push(call_frame);
        self.stream.set_prg_cntr(offset)?;

        if self.runtime_options.trace_frame_action_call {
            eprintln!(
                "[SG_FRAME_ACTION_CALL] run cmd={} scene={:?} offset=0x{:x} return_pc=0x{:x} args={:?}",
                cmd_name,
                self.current_scene_no,
                offset,
                return_pc,
                call_args
            );
        }

        let max_steps = self.runtime_options.inline_user_cmd_max_steps;
        let mut steps: u64 = 0;
        let mut run_error = None;
        loop {
            if let Some(end) = end_offset {
                if self.stream.get_prg_cntr() >= end {
                    break;
                }
            }
            let wait_generation_before_step = self.ctx.wait.block_generation();
            let proc_generation_before_step = self.ctx.proc_generation();
            let running = match self.step_inner(false) {
                Ok(v) => v,
                Err(e) => {
                    run_error = Some(e);
                    break;
                }
            };
            if self.halted || !running {
                break;
            }
            if self.ctx.proc_generation() != proc_generation_before_step {
                break;
            }
            if self.ctx.wait.block_generation() != wait_generation_before_step && self.ctx.wait_poll() {
                break;
            }
            if self.call_stack.len() == base_depth {
                break;
            }
            steps = steps.saturating_add(1);
            if max_steps > 0 && steps >= max_steps {
                run_error = Some(anyhow!(
                    "inline user command exceeded SIGLUS_INLINE_USER_CMD_MAX_STEPS: cmd={}",
                    cmd_name
                ));
                break;
            }
        }

        let captured_inline_return = if ret_form == self.cfg.fm_int || ret_form == self.cfg.fm_label {
            if self.int_stack.len() > checkpoint.int_len {
                self.int_stack.last().copied().map(|v| Value::Int(v as i64))
            } else {
                None
            }
        } else if ret_form == self.cfg.fm_str {
            if self.str_stack.len() > checkpoint.str_len {
                self.str_stack.last().cloned().map(Value::Str)
            } else {
                None
            }
        } else {
            None
        };

        if self.current_scene_no == checkpoint.scene_no {
            self.restore_inline_exec_checkpoint(checkpoint)?;
            if self.runtime_options.trace_call_return_pc {
                if let Some(caller) = self.call_stack.last() {
                    eprintln!(
                        "[SG_CALL_PC] inline restore cmd={} depth={} return_pc=0x{:x}",
                        cmd_name,
                        base_depth,
                        caller.return_pc
                    );
                }
            }
        }
        if let Some(v) = captured_inline_return {
            self.ctx.stack.push(v);
        }

        if let Some(e) = run_error {
            return Err(e);
        }

        Ok(true)
    }

    fn ensure_scene_pck_cache(&mut self) -> Result<()> {
        let active_append = self.ctx.globals.append_dir.clone();
        let append_changed = self
            .scene_pck_append_dir
            .as_deref()
            .map(|cached| !cached.eq_ignore_ascii_case(&active_append))
            .unwrap_or(true);
        if self.scene_pck_cache.is_some() && !append_changed {
            return Ok(());
        }

        // Original `tnm_reload_scene_pck()` replaces the lexer package when the
        // active append changes.  All scene-number/name caches belong to that
        // package and must be discarded together.
        self.scene_pck_cache = None;
        self.scene_stream_cache.clear();
        self.scene_name_resolve_cache.clear();
        self.user_cmd_resolve_cache.clear();

        let scene_pck_path = crate::resource::find_scene_pck_path_for_append(
            &self.ctx.project_dir,
            &active_append,
        )?;

        #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
        {
            let bytes = crate::resource::read_file_bytes(&scene_pck_path)?;
            let exe = ["key.toml", "Key.toml"]
                .iter()
                .find_map(|name| {
                    let p = self.ctx.project_dir.join(name);
                    if !crate::resource::wasm_path_is_file(&p) {
                        return None;
                    }
                    let text = crate::resource::read_file_to_string(&p).ok()?;
                    siglus_assets::key_toml::parse_key_toml(&text)
                        .ok()
                        .and_then(|cfg| cfg.exe_key16)
                        .map(|v| v.to_vec())
                });
            let opt = ScenePckDecodeOptions {
                exe_angou_element: exe,
                easy_angou_code: Some(siglus_assets::keys::SCENE_KEY.to_vec()),
            };
            self.scene_pck_cache = Some(ScenePck::load_and_rebuild_from_bytes(bytes, &opt)?);
        }

        #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
        {
            let opt = ScenePckDecodeOptions::from_project_dir(&self.ctx.project_dir)?;
            self.scene_pck_cache = Some(ScenePck::load_and_rebuild(&scene_pck_path, &opt)?);
        }

        self.ctx.install_scene_metadata(
            &active_append,
            self.scene_pck_cache
                .as_ref()
                .expect("scene pck cache initialized"),
        )?;
        self.scene_pck_append_dir = Some(active_append);
        Ok(())
    }

    fn ensure_scene_stream_cached(&mut self, scene_no: usize) -> Result<()> {
        self.ensure_scene_pck_cache()?;
        if !self.scene_stream_cache.contains_key(&scene_no) {
            let chunk = {
                let pck = self
                    .scene_pck_cache
                    .as_ref()
                    .expect("scene pck cache initialized");
                pck.scn_data_slice(scene_no)?.to_vec()
            };
            let chunk_leaked: &'static [u8] = Box::leak(chunk.into_boxed_slice());
            let stream = SceneStream::new(chunk_leaked)?;
            self.scene_stream_cache.insert(scene_no, stream);
        }
        Ok(())
    }

    fn cached_scene_stream(&mut self, scene_no: usize) -> Result<SceneStream<'a>> {
        self.ensure_scene_stream_cached(scene_no)?;
        Ok(self
            .scene_stream_cache
            .get(&scene_no)
            .expect("scene stream cached")
            .clone())
    }

    fn find_scene_no_by_name(pck: &ScenePck, name: &str) -> Option<usize> {
        pck.scn_name_map.iter().find_map(|(scene_name, scene_no)| {
            if siglus_name_eq(scene_name, name) {
                Some(*scene_no)
            } else {
                None
            }
        })
    }

    fn requested_user_command_scene_no(
        &mut self,
        scn_name: Option<&str>,
    ) -> Result<Option<usize>> {
        let Some(name) = scn_name.filter(|name| !name.is_empty()) else {
            return Ok(self.current_scene_no);
        };
        if let Some(scene_no) = self.scene_name_resolve_cache.get(name) {
            return Ok(*scene_no);
        }
        self.ensure_scene_pck_cache()?;
        let pck = self
            .scene_pck_cache
            .as_ref()
            .ok_or_else(|| anyhow!("scene pck cache is not initialized"))?;
        let scene_no = Self::find_scene_no_by_name(pck, name);
        self.scene_name_resolve_cache.insert(name.to_string(), scene_no);
        Ok(scene_no)
    }

    fn resolve_user_command_by_name(
        &mut self,
        requested_scene_no: usize,
        cmd_name: &str,
    ) -> Result<Option<ResolvedUserCommand>> {
        if let Some(cached) = self
            .user_cmd_resolve_cache
            .get(&requested_scene_no)
            .and_then(|commands| commands.get(cmd_name))
        {
            return Ok(Some(cached.clone()));
        }

        self.ensure_scene_pck_cache()?;
        if self.current_scene_no != Some(requested_scene_no) {
            self.ensure_scene_stream_cached(requested_scene_no)?;
        }

        let resolved = {
            let pck = self
                .scene_pck_cache
                .as_ref()
                .ok_or_else(|| anyhow!("scene pck cache is not initialized"))?;
            let local_names = if self.current_scene_no == Some(requested_scene_no) {
                &self.user_cmd_names
            } else {
                &self
                    .scene_stream_cache
                    .get(&requested_scene_no)
                    .expect("scene stream cached")
                    .scn_cmd_name_map
            };
            let inc_cmd_cnt = pck.inc_cmds.len();
            let Some((encoded_no, include_command)) = resolve_named_user_command_number(
                &pck.inc_cmd_name_map,
                local_names,
                inc_cmd_cnt,
                cmd_name,
            ) else {
                return Ok(None);
            };

            if include_command {
                let target = pck.inc_cmds.get(encoded_no).copied().ok_or_else(|| {
                    anyhow!(
                        "include user command {} is missing from Scene.pck inc_cmds",
                        encoded_no
                    )
                })?;
                let canonical_name = pck
                    .inc_cmd_name_map
                    .get(&(encoded_no as u32))
                    .cloned()
                    .unwrap_or_else(|| cmd_name.to_string());
                if target.scn_no < 0 || target.offset < 0 {
                    bail!(
                        "invalid include user command target: cmd_no={} name={} scn_no={} offset={}",
                        encoded_no,
                        canonical_name,
                        target.scn_no,
                        target.offset
                    );
                }
                ResolvedUserCommand {
                    encoded_no,
                    name: canonical_name,
                    target_scene_no: target.scn_no as usize,
                    target_offset: target.offset as usize,
                    include_command: true,
                }
            } else {
                let local_cmd_no = encoded_no - inc_cmd_cnt;
                let target_offset = if self.current_scene_no == Some(requested_scene_no) {
                    self.stream.scn_cmd_offset(local_cmd_no)?
                } else {
                    self.scene_stream_cache
                        .get(&requested_scene_no)
                        .expect("scene stream cached")
                        .scn_cmd_offset(local_cmd_no)?
                };
                let name = local_names
                    .get(&(local_cmd_no as u32))
                    .cloned()
                    .unwrap_or_else(|| cmd_name.to_string());
                ResolvedUserCommand {
                    encoded_no,
                    name,
                    target_scene_no: requested_scene_no,
                    target_offset,
                    include_command: false,
                }
            }
        };

        self.user_cmd_resolve_cache
            .entry(requested_scene_no)
            .or_default()
            .insert(cmd_name.to_string(), resolved.clone());
        Ok(Some(resolved))
    }

    fn resolve_user_command_by_id(
        &mut self,
        requested_scene_no: usize,
        cmd_no: usize,
    ) -> Result<ResolvedUserCommand> {
        self.ensure_scene_pck_cache()?;
        let (inc_cmd_cnt, inc_target, inc_name) = {
            let pck = self
                .scene_pck_cache
                .as_ref()
                .ok_or_else(|| anyhow!("scene pck cache is not initialized"))?;
            let inc_target = pck.inc_cmds.get(cmd_no).copied();
            let inc_name = pck.inc_cmd_name_map.get(&(cmd_no as u32)).cloned();
            (pck.inc_cmds.len(), inc_target, inc_name)
        };

        if cmd_no < inc_cmd_cnt {
            let target = inc_target.ok_or_else(|| {
                anyhow!(
                    "include user command {} is missing from Scene.pck inc_cmds",
                    cmd_no
                )
            })?;
            if target.scn_no < 0 || target.offset < 0 {
                bail!(
                    "invalid include user command target: cmd_no={} name={} scn_no={} offset={}",
                    cmd_no,
                    inc_name.as_deref().unwrap_or("<unknown>"),
                    target.scn_no,
                    target.offset
                );
            }
            return Ok(ResolvedUserCommand {
                encoded_no: cmd_no,
                name: inc_name.unwrap_or_else(|| format!("<include-command-{cmd_no}>")),
                target_scene_no: target.scn_no as usize,
                target_offset: target.offset as usize,
                include_command: true,
            });
        }

        let local_cmd_no = cmd_no - inc_cmd_cnt;
        let (target_offset, name) = if self.current_scene_no == Some(requested_scene_no) {
            let target_offset = self.stream.scn_cmd_offset(local_cmd_no)?;
            let name = self
                .user_cmd_names
                .get(&(local_cmd_no as u32))
                .cloned()
                .unwrap_or_else(|| format!("<scene-command-{local_cmd_no}>"));
            (target_offset, name)
        } else {
            let target_stream = self.cached_scene_stream(requested_scene_no)?;
            let target_offset = target_stream.scn_cmd_offset(local_cmd_no)?;
            let name = target_stream
                .scn_cmd_name_map
                .get(&(local_cmd_no as u32))
                .cloned()
                .unwrap_or_else(|| format!("<scene-command-{local_cmd_no}>"));
            (target_offset, name)
        };

        Ok(ResolvedUserCommand {
            encoded_no: cmd_no,
            name,
            target_scene_no: requested_scene_no,
            target_offset,
            include_command: false,
        })
    }

    fn enter_resolved_user_command(
        &mut self,
        command: &ResolvedUserCommand,
        ret_form: i32,
        call_args: &[Value],
        excall_proc: bool,
        frame_action_proc: bool,
    ) -> Result<bool> {
        if self.current_scene_no == Some(command.target_scene_no) {
            self.enter_current_scene_user_cmd_proc_at_offset(
                command.target_offset,
                ret_form,
                call_args,
                excall_proc,
                frame_action_proc,
            )
        } else {
            self.enter_scene_user_cmd_at_scene_offset_ex(
                command.target_scene_no,
                command.target_offset,
                call_args,
                ret_form,
                excall_proc,
                frame_action_proc,
            )
        }
    }

    fn run_scene_user_cmd_inline_at_cached_scene_offset(
        &mut self,
        target_scene_no: usize,
        cmd_name: &str,
        target_offset: usize,
        call_args: &[Value],
        ret_form: i32,
        preserve_return_pc: bool,
        frame_action_proc: bool,
    ) -> Result<bool> {
        let target_stream = self.cached_scene_stream(target_scene_no)?;
        if target_offset > target_stream.scn.len() {
            bail!(
                "scene_pck: user command offset out of bounds: cmd={} scn_no={} offset=0x{:x} scn_len=0x{:x}",
                cmd_name,
                target_scene_no,
                target_offset,
                target_stream.scn.len()
            );
        }
        let (target_call_cmd_names, target_scene_name) = {
            let pck = self
                .scene_pck_cache
                .as_ref()
                .expect("scene pck cache initialized");
            (
                pck.inc_cmd_name_map.clone(),
                pck.find_scene_name(target_scene_no).map(ToOwned::to_owned),
            )
        };

        let saved_stream = std::mem::replace(&mut self.stream, target_stream);
        let target_user_cmd_names = self.stream.scn_cmd_name_map.clone();
        let saved_user_cmd_names =
            std::mem::replace(&mut self.user_cmd_names, target_user_cmd_names);
        let saved_call_cmd_names =
            std::mem::replace(&mut self.call_cmd_names, target_call_cmd_names);
        let saved_current_scene_no = self.current_scene_no;
        let saved_current_scene_name = self.current_scene_name.clone();
        let saved_current_line_no = self.current_line_no;
        let saved_ctx_scene_no = self.ctx.current_scene_no;
        let saved_ctx_scene_name = self.ctx.current_scene_name.clone();
        let saved_ctx_line_no = self.ctx.current_line_no;
        let saved_halted = self.halted;
        self.enter_cross_scene_user_prop_scope(target_scene_no);

        self.current_scene_no = Some(target_scene_no);
        self.current_scene_name = target_scene_name;
        self.current_line_no = -1;
        self.ctx.current_scene_no = Some(target_scene_no as i64);
        self.ctx.current_scene_name = self.current_scene_name.clone();
        self.ctx.current_line_no = -1;

        let target_return_pc = if preserve_return_pc {
            saved_stream.get_prg_cntr()
        } else {
            self.stream.scn.len()
        };
        let result = self.run_user_cmd_inline_at_offset(
            cmd_name,
            target_offset,
            target_return_pc,
            None,
            None,
            ret_form,
            call_args,
            frame_action_proc,
        );

        // Save the target scene's locals while current_scene_no still
        // identifies it, then reactivate the caller's resident scene scope.
        self.restore_cross_scene_user_prop_scope(saved_current_scene_no);
        self.stream = saved_stream;
        self.user_cmd_names = saved_user_cmd_names;
        self.call_cmd_names = saved_call_cmd_names;
        self.current_scene_no = saved_current_scene_no;
        self.current_scene_name = saved_current_scene_name;
        self.current_line_no = saved_current_line_no;
        self.ctx.current_scene_no = saved_ctx_scene_no;
        self.ctx.current_scene_name = saved_ctx_scene_name;
        self.ctx.current_line_no = saved_ctx_line_no;
        self.halted = saved_halted;
        result
    }

    fn run_scene_user_cmd_inline(
        &mut self,
        scn_name: Option<&str>,
        cmd_name: &str,
        call_args: &[Value],
        ret_form: i32,
        frame_action_proc: bool,
    ) -> Result<bool> {
        if frame_action_proc {
            return self.run_scene_user_cmd_frame_action_proc(scn_name, cmd_name, call_args);
        }

        let Some(requested_scene_no) = self.requested_user_command_scene_no(scn_name)? else {
            return Ok(false);
        };
        let Some(command) = self.resolve_user_command_by_name(requested_scene_no, cmd_name)? else {
            if self.runtime_options.trace_frame_action_call {
                eprintln!(
                    "[SG_FRAME_ACTION_CALL] user command not found: requested_scene={} scn_name={:?} cmd={}",
                    requested_scene_no,
                    scn_name,
                    cmd_name
                );
            }
            return Ok(false);
        };

        if self.current_scene_no == Some(command.target_scene_no) {
            let return_pc = self.stream.get_prg_cntr();
            return self.run_user_cmd_inline_at_offset(
                &command.name,
                command.target_offset,
                return_pc,
                None,
                Some(return_pc),
                ret_form,
                call_args,
                false,
            );
        }

        self.run_scene_user_cmd_inline_at_cached_scene_offset(
            command.target_scene_no,
            &command.name,
            command.target_offset,
            call_args,
            ret_form,
            false,
            false,
        )
    }

    fn run_scene_user_cmd_frame_action_proc(
        &mut self,
        scn_name: Option<&str>,
        cmd_name: &str,
        call_args: &[Value],
    ) -> Result<bool> {
        let checkpoint = self.inline_exec_checkpoint();
        let saved_scene_no = checkpoint.scene_no;
        // The callback runs on the *shared* interpreter stacks, so it can consume
        // values the suspended caller already pushed (a frame action may be drained
        // while the scenario sits in the middle of an expression). Lengths alone
        // cannot bring those back, so snapshot the values too.
        let saved_int_stack = self.int_stack.clone();
        let saved_str_stack = self.str_stack.clone();
        let saved_element_points = self.element_points.clone();
        let saved_scene_stack_len = checkpoint.scene_depth;
        let saved_call_depth = checkpoint.call_depth;

        let Some(requested_scene_no) = self.requested_user_command_scene_no(scn_name)? else {
            return Ok(false);
        };
        let Some(command) = self.resolve_user_command_by_name(requested_scene_no, cmd_name)? else {
            if self.runtime_options.trace_frame_action_call {
                eprintln!(
                    "[SG_FRAME_ACTION_CALL] user command not found: requested_scene={} scn_name={:?} cmd={}",
                    requested_scene_no,
                    scn_name,
                    cmd_name
                );
            }
            return Ok(false);
        };
        // Frame actions run as an independent nested SCRIPT proc and must
        // continue after the main scenario proc has returned. A previous proc
        // boundary must not make the callback stop before its first opcode;
        // the caller's halted state is restored by the checkpoint below.
        self.halted = false;
        self.enter_resolved_user_command(
            &command,
            self.cfg.fm_void,
            call_args,
            false,
            true,
        )?;

        if self.runtime_options.trace_frame_action_call {
            eprintln!(
                "[SG_FRAME_ACTION_CALL] proc enter cmd={} scene={:?} depth={} args={:?}",
                cmd_name,
                self.current_scene_no,
                self.call_stack.len(),
                call_args
            );
        }

        let mut completed_by_return = false;
        let mut stopped_at_proc_boundary = false;
        let mut stopped_at_wait_boundary = false;
        let mut run_error = None;
        let max_steps = self.runtime_options.frame_action_max_steps;
        let mut steps: u64 = 0;
        loop {
            let wait_generation_before_step = self.ctx.wait.block_generation();
            let proc_generation_before_step = self.ctx.proc_generation();
            let running = match self.step_inner(false) {
                Ok(v) => v,
                Err(e) => {
                    run_error = Some(e);
                    break;
                }
            };
            if self.current_scene_no == saved_scene_no
                && self.scene_stack.len() == saved_scene_stack_len
                && self.call_stack.len() == saved_call_depth
            {
                completed_by_return = true;
                break;
            }
            if self.halted || !running {
                break;
            }
            if self.ctx.proc_generation() != proc_generation_before_step {
                stopped_at_proc_boundary = true;
                break;
            }
            if self.ctx.wait.block_generation() != wait_generation_before_step && self.ctx.wait_poll() {
                stopped_at_wait_boundary = true;
                break;
            }
            steps = steps.saturating_add(1);
            if max_steps > 0 && steps >= max_steps {
                run_error = Some(anyhow!(
                    "frame_action user command exceeded SIGLUS_FRAME_ACTION_MAX_STEPS: cmd={} scene={:?}",
                    cmd_name,
                    scn_name
                ));
                break;
            }
        }

        // Original Siglus enters frame-action user commands through the normal
        // call stack (tnm_scene_proc_call_user_cmd + recursive tnm_proc_script).
        // The caller VM stacks are shared, not deep-copied.  For the synchronous
        // Rust frame phase we only need to discard any unfinished temporary
        // frames/stack tail when execution stops at a wait/proc/error boundary.
        // Normal callback writes to globals / user properties remain visible,
        // matching the original engine.
        let restore_callback_lexer = self.current_scene_no == saved_scene_no;
        if restore_callback_lexer {
            if self.runtime_options.trace_frame_action_call {
                eprintln!(
                    "[SG_FRAME_ACTION_CALL] proc exit cmd={} scene={:?} completed={} proc_boundary={} wait_boundary={} error={} restoring caller execution checkpoint",
                    cmd_name,
                    scn_name,
                    completed_by_return,
                    stopped_at_proc_boundary,
                    stopped_at_wait_boundary,
                    run_error.is_some()
                );
            }
            if completed_by_return {
                // The callback RETURNed normally, so execution resumes in the caller
                // at the pc recorded when the frame action was drained. That pc can be
                // mid-expression: the next opcode then pops an operand the callback
                // consumed, and the VM dies with `int stack underflow` (observed at
                // sys40_mp20 line 3178, pc=0x3b3cc, ring depth 3 -> 0 across the call).
                // Restore the caller's operand stacks verbatim. On the boundary paths
                // the callback is still parked in the call stack, so leave the tail
                // alone and keep the original discard-the-tail behaviour.
                self.int_stack = saved_int_stack;
                self.str_stack = saved_str_stack;
                self.element_points = saved_element_points;
            }
            self.restore_inline_exec_checkpoint(checkpoint)?;
        }

        if let Some(e) = run_error {
            return Err(e);
        }

        Ok(true)
    }

    fn enter_current_scene_user_cmd_proc_at_offset(
        &mut self,
        offset: usize,
        ret_form: i32,
        call_args: &[Value],
        excall_proc: bool,
        frame_action_proc: bool,
    ) -> Result<bool> {
        let return_pc = self.stream.get_prg_cntr();
        let depth = self.call_stack.len();
        let Some(caller) = self.call_stack.last_mut() else {
            return Ok(false);
        };
        if self.runtime_options.trace_call_return_pc {
            eprintln!(
                "[SG_CALL_PC] proc-call set depth={} offset=0x{:x} return_pc=0x{:x} old=0x{:x} frame_action={}",
                depth,
                offset,
                return_pc,
                caller.return_pc,
                frame_action_proc
            );
        }
        caller.return_pc = return_pc;
        caller.return_scene_no = self.current_scene_no;
        caller.return_scene_name = self.current_scene_name.clone();
        caller.return_line_no = self.current_line_no;
        caller.ret_form = ret_form;
        if sg_ring_on() {
            eprintln!(
                "[SG_CALL_ENTER] scene={} pc=0x{:x} offset=0x{:x} ret_form={} (void={} int={}) excall={} frame_action={} argc={} depth={}",
                self.current_scene_name.as_deref().unwrap_or("<none>"),
                return_pc,
                offset,
                ret_form,
                self.cfg.fm_void,
                self.cfg.fm_int,
                excall_proc,
                frame_action_proc,
                call_args.len(),
                depth
            );
        }
        for arg in call_args {
            self.push_call_arg_value(arg);
        }
        let mut call_frame = self.take_call_frame(
            self.cfg.fm_void,
            excall_proc,
            frame_action_proc,
            call_args.len(),
            None,
        );
        call_frame.call_type = 3;
        call_frame.return_override = Some((return_pc, ret_form));
        self.call_stack.push(call_frame);
        self.stream.set_prg_cntr(offset)?;
        if excall_proc {
            self.mark_excall_script_proc_requested();
        }
        Ok(true)
    }

    fn enter_scene_user_cmd_at_scene_offset_ex(
        &mut self,
        target_scene_no: usize,
        target_offset: usize,
        call_args: &[Value],
        ret_form: i32,
        ex_call_proc: bool,
        frame_action_proc: bool,
    ) -> Result<bool> {
        let target_stream = self.cached_scene_stream(target_scene_no)?;
        if target_offset > target_stream.scn.len() {
            bail!(
                "scene_pck: user command offset out of bounds: scn_no={} offset=0x{:x} scn_len=0x{:x}",
                target_scene_no,
                target_offset,
                target_stream.scn.len()
            );
        }

        // C++ tnm_scene_proc_call_user_cmd() stores the caller lexer position
        // on the current C_elm_call and then add_call()s one callee.  VM value
        // stacks and the call list themselves stay shared across scenes.
        let return_pc = self.stream.get_prg_cntr();
        let depth = self.call_stack.len();
        let Some(caller) = self.call_stack.last_mut() else {
            return Ok(false);
        };
        if self.runtime_options.trace_call_return_pc {
            eprintln!(
                "[SG_CALL_PC] cross-scene user-cmd set depth={} target_scene={} offset=0x{:x} return_pc=0x{:x} old=0x{:x} frame_action={}",
                depth,
                target_scene_no,
                target_offset,
                return_pc,
                caller.return_pc,
                frame_action_proc
            );
        }
        caller.return_pc = return_pc;
        caller.return_scene_no = self.current_scene_no;
        caller.return_scene_name = self.current_scene_name.clone();
        caller.return_line_no = self.current_line_no;
        caller.ret_form = ret_form;

        let target_user_cmd_names = target_stream.scn_cmd_name_map.clone();
        let (target_call_cmd_names, target_scene_name) = {
            let pck = self
                .scene_pck_cache
                .as_ref()
                .expect("scene pck cache initialized");
            (
                pck.inc_cmd_name_map.clone(),
                pck.find_scene_name(target_scene_no).map(ToOwned::to_owned),
            )
        };

        let saved_stream = std::mem::replace(&mut self.stream, target_stream);
        let saved_user_cmd_names =
            std::mem::replace(&mut self.user_cmd_names, target_user_cmd_names);
        let saved_call_cmd_names =
            std::mem::replace(&mut self.call_cmd_names, target_call_cmd_names);
        let saved_current_scene_no = self.current_scene_no;
        let saved_current_scene_name = self.current_scene_name.clone();
        let saved_current_line_no = self.current_line_no;

        self.enter_cross_scene_user_prop_scope(target_scene_no);
        self.current_scene_no = Some(target_scene_no);
        self.current_scene_name = target_scene_name;
        self.current_line_no = -1;
        self.ctx.current_scene_no = Some(target_scene_no as i64);
        self.ctx.current_scene_name = self.current_scene_name.clone();
        self.ctx.current_line_no = -1;

        for arg in call_args {
            self.push_call_arg_value(arg);
        }
        let mut call_frame = self.take_call_frame(
            self.cfg.fm_void,
            ex_call_proc,
            frame_action_proc,
            call_args.len(),
            None,
        );
        call_frame.call_type = 3;
        self.call_stack.push(call_frame);
        self.scene_stack.push(SceneExecFrame {
            stream: saved_stream,
            user_cmd_names: saved_user_cmd_names,
            call_cmd_names: saved_call_cmd_names,
            current_scene_no: saved_current_scene_no,
            current_scene_name: saved_current_scene_name,
            current_line_no: saved_current_line_no,
            call_depth: self.call_stack.len(),
        });
        self.stream.set_prg_cntr(target_offset)?;
        if ex_call_proc {
            self.mark_excall_script_proc_requested();
        }
        Ok(true)
    }

    fn enter_scene_user_cmd_call(
        &mut self,
        scn_name: Option<&str>,
        cmd_name: &str,
        call_args: &[Value],
    ) -> Result<bool> {
        let Some(requested_scene_no) = self.requested_user_command_scene_no(scn_name)? else {
            return Ok(false);
        };
        let Some(command) = self.resolve_user_command_by_name(requested_scene_no, cmd_name)? else {
            if self.runtime_options.sg_debug {
                eprintln!(
                    "[SG_DEBUG][BUTTON] user command not found for ex-call: requested_scene={} scn_name={:?} cmd={}",
                    requested_scene_no,
                    scn_name,
                    cmd_name
                );
            }
            return Ok(false);
        };

        if self.runtime_options.sg_debug {
            eprintln!(
                "[SG_DEBUG][BUTTON] enter user command requested_scene={} target_scene={} cmd={} encoded_no={} include={} offset=0x{:x}",
                requested_scene_no,
                command.target_scene_no,
                command.name.as_str(),
                command.encoded_no,
                command.include_command,
                command.target_offset
            );
        }
        self.enter_resolved_user_command(
            &command,
            self.cfg.fm_void,
            call_args,
            true,
            false,
        )
    }

    fn enter_current_scene_user_cmd_at_offset(
        &mut self,
        offset: usize,
        call_args: &[Value],
    ) -> Result<bool> {
        self.enter_current_scene_user_cmd_proc_at_offset(
            offset,
            self.cfg.fm_void,
            call_args,
            true,
            false,
        )
    }

    fn enter_scene_user_cmd_at_scene_offset(
        &mut self,
        target_scene_no: usize,
        target_offset: usize,
        call_args: &[Value],
    ) -> Result<bool> {
        self.enter_scene_user_cmd_at_scene_offset_ex(
            target_scene_no,
            target_offset,
            call_args,
            self.cfg.fm_void,
            true,
            false,
        )
    }

    fn run_current_scene_user_cmd_inline(
        &mut self,
        cmd_name: &str,
        call_args: &[Value],
    ) -> Result<bool> {
        self.run_scene_user_cmd_inline(None, cmd_name, call_args, self.cfg.fm_void, false)
    }

    fn run_scene_user_cmd_inline_at_scene_offset(
        &mut self,
        pck: &ScenePck,
        target_scene_no: usize,
        cmd_name: &str,
        target_offset: usize,
        call_args: &[Value],
        preserve_return_pc: bool,
        frame_action_proc: bool,
    ) -> Result<bool> {
        let chunk = pck.scn_data_slice(target_scene_no)?;
        let chunk_leaked: &'static [u8] = Box::leak(chunk.to_vec().into_boxed_slice());
        let target_stream: SceneStream<'a> = SceneStream::new(chunk_leaked)?;
        if target_offset > target_stream.scn.len() {
            bail!(
                "scene_pck: user command offset out of bounds: cmd={} scn_no={} offset=0x{:x} scn_len=0x{:x}",
                cmd_name,
                target_scene_no,
                target_offset,
                target_stream.scn.len()
            );
        }

        let saved_stream = std::mem::replace(&mut self.stream, target_stream);
        let target_user_cmd_names = self.stream.scn_cmd_name_map.clone();
        let target_call_cmd_names = pck.inc_cmd_name_map.clone();
        let saved_user_cmd_names =
            std::mem::replace(&mut self.user_cmd_names, target_user_cmd_names);
        let saved_call_cmd_names =
            std::mem::replace(&mut self.call_cmd_names, target_call_cmd_names);
        let saved_current_scene_no = self.current_scene_no;
        let saved_current_scene_name = self.current_scene_name.clone();
        let saved_current_line_no = self.current_line_no;
        let saved_ctx_scene_no = self.ctx.current_scene_no;
        let saved_ctx_scene_name = self.ctx.current_scene_name.clone();
        let saved_ctx_line_no = self.ctx.current_line_no;
        let saved_halted = self.halted;
        self.enter_cross_scene_user_prop_scope(target_scene_no);

        self.current_scene_no = Some(target_scene_no);
        self.current_scene_name = pck.find_scene_name(target_scene_no).map(ToOwned::to_owned);
        self.current_line_no = -1;
        self.ctx.current_scene_no = Some(target_scene_no as i64);
        self.ctx.current_scene_name = self.current_scene_name.clone();
        self.ctx.current_line_no = -1;

        let target_return_pc = if preserve_return_pc {
            saved_stream.get_prg_cntr()
        } else {
            self.stream.scn.len()
        };
        let result = self.run_user_cmd_inline_at_offset(
            cmd_name,
            target_offset,
            target_return_pc,
            None,
            None,
            self.cfg.fm_void,
            call_args,
            frame_action_proc,
        );

        // Save the target scene's locals while current_scene_no still
        // identifies it, then reactivate the caller's resident scene scope.
        self.restore_cross_scene_user_prop_scope(saved_current_scene_no);
        self.stream = saved_stream;
        self.user_cmd_names = saved_user_cmd_names;
        self.call_cmd_names = saved_call_cmd_names;
        self.current_scene_no = saved_current_scene_no;
        self.current_scene_name = saved_current_scene_name;
        self.current_line_no = saved_current_line_no;
        self.ctx.current_scene_no = saved_ctx_scene_no;
        self.ctx.current_scene_name = saved_ctx_scene_name;
        self.ctx.current_line_no = saved_ctx_line_no;
        self.halted = saved_halted;
        result
    }

    fn collect_object_frame_action_work_recursive(
        obj: &crate::runtime::globals::ObjectState,
        stage_idx: i64,
        obj_idx: usize,
        object_chain: Vec<i32>,
        out: &mut Vec<FrameActionWork>,
    ) {
        let fa = &obj.frame_action;
        if !fa.cmd_name.is_empty() {
            let mut frame_action_chain = object_chain.clone();
            frame_action_chain.push(crate::runtime::forms::codes::elm_value::OBJECT_FRAME_ACTION);
            out.push(FrameActionWork {
                stage_idx,
                obj_idx,
                ch_idx: None,
                global_form_id: None,
                object_chain: Some(object_chain.clone()),
                frame_action_chain: Some(frame_action_chain),
                scn_name: fa.scn_name.clone(),
                cmd_name: fa.cmd_name.clone(),
                args: fa.args.clone(),
                count: fa.counter.get_count(),
                end_time: fa.end_time,
            });
        }
        for (ch_idx, ch) in obj.frame_action_ch.iter().enumerate() {
            if !ch.cmd_name.is_empty() {
                let mut frame_action_chain = object_chain.clone();
                frame_action_chain
                    .push(crate::runtime::forms::codes::elm_value::OBJECT_FRAME_ACTION_CH);
                frame_action_chain.push(crate::runtime::forms::codes::ELM_ARRAY);
                frame_action_chain.push(ch_idx as i32);
                out.push(FrameActionWork {
                    stage_idx,
                    obj_idx,
                    ch_idx: Some(ch_idx),
                    global_form_id: None,
                    object_chain: Some(object_chain.clone()),
                    frame_action_chain: Some(frame_action_chain),
                    scn_name: ch.scn_name.clone(),
                    cmd_name: ch.cmd_name.clone(),
                    args: ch.args.clone(),
                    count: ch.counter.get_count(),
                    end_time: ch.end_time,
                });
            }
        }
        for (child_idx, child) in obj.runtime.child_objects.iter().enumerate() {
            // CHILD object lists are initialized with use_ini=false in C++;
            // every allocated child slot therefore has use_flag=true even when
            // its current type is NONE.  Recurse over the list itself instead
            // of treating ObjectState::used as C_elm_object::is_use().
            let mut child_chain = object_chain.clone();
            child_chain.push(crate::runtime::forms::codes::elm_value::OBJECT_CHILD);
            child_chain.push(crate::runtime::forms::codes::ELM_ARRAY);
            child_chain.push(child_idx as i32);
            Self::collect_object_frame_action_work_recursive(
                child,
                stage_idx,
                child_idx,
                child_chain,
                out,
            );
        }
    }

    fn object_child_from_chain_mut<'b>(
        mut obj: &'b mut crate::runtime::globals::ObjectState,
        object_chain: &[i32],
        mut pos: usize,
        elm_array: i32,
    ) -> Option<&'b mut crate::runtime::globals::ObjectState> {
        while pos + 2 < object_chain.len() {
            let op = object_chain[pos];
            if op != crate::runtime::forms::codes::elm_value::OBJECT_CHILD {
                break;
            }
            if object_chain[pos + 1] != elm_array
                && object_chain[pos + 1] != crate::runtime::forms::codes::ELM_ARRAY
            {
                return None;
            }
            let child_idx = object_chain[pos + 2].max(0) as usize;
            obj = obj.runtime.child_objects.get_mut(child_idx)?;
            pos += 3;
        }
        Some(obj)
    }

    fn with_frame_action_mut<R>(
        &mut self,
        item: &FrameActionWork,
        f: impl FnOnce(&mut crate::runtime::globals::ObjectFrameActionState) -> R,
    ) -> Option<R> {
        if item.object_chain.is_none() {
            let form_id = item.global_form_id?;
            if let Some(idx) = item.ch_idx {
                let list = self.ctx.globals.frame_action_lists.get_mut(&form_id)?;
                return list.get_mut(idx).map(f);
            }
            return self.ctx.globals.frame_actions.get_mut(&form_id).map(f);
        }

        let chain = item.object_chain.as_deref()?;
        let elm_array = if self.ctx.ids.elm_array != 0 {
            self.ctx.ids.elm_array
        } else {
            crate::runtime::forms::codes::ELM_ARRAY
        };
        let locator = parse_frame_action_object_locator(chain, elm_array)?;
        let form_id = crate::runtime::forms::stage::stage_storage_form_id(
            &self.ctx,
            locator.raw_form_id,
        );
        let st = self.ctx.globals.stage_forms.get_mut(&form_id)?;
        let obj = match locator.root {
            FrameActionObjectRoot::StageObject { obj_idx, child_pos } => {
                let objects = st.object_lists.get_mut(&locator.stage_idx)?;
                let obj = objects.get_mut(obj_idx)?;
                Self::object_child_from_chain_mut(obj, chain, child_pos, elm_array)?
            }
            FrameActionObjectRoot::MwndObject {
                mwnd_idx,
                selector,
                obj_idx,
                child_pos,
            } => {
                let mwnds = st.mwnd_lists.get_mut(&locator.stage_idx)?;
                let mwnd = mwnds.get_mut(mwnd_idx)?;
                let obj = if selector == crate::runtime::forms::codes::elm_value::MWND_BUTTON {
                    mwnd.button_list.get_mut(obj_idx)?
                } else if selector == crate::runtime::forms::codes::elm_value::MWND_FACE {
                    mwnd.face_list.get_mut(obj_idx)?
                } else {
                    mwnd.object_list.get_mut(obj_idx)?
                };
                Self::object_child_from_chain_mut(obj, chain, child_pos, elm_array)?
            }
            FrameActionObjectRoot::BtnSelItemObject {
                item_idx,
                obj_idx,
                child_pos,
            } => {
                let items = st.btnselitem_lists.get_mut(&locator.stage_idx)?;
                let item = items.get_mut(item_idx)?;
                let obj = item.object_list.get_mut(obj_idx)?;
                Self::object_child_from_chain_mut(obj, chain, child_pos, elm_array)?
            }
        };

        if let Some(idx) = item.ch_idx {
            obj.frame_action_ch.get_mut(idx).map(f)
        } else {
            Some(f(&mut obj.frame_action))
        }
    }

    fn begin_frame_action_finish(
        &mut self,
        item: &FrameActionWork,
    ) -> Option<(String, String, Vec<Value>)> {
        self.with_frame_action_mut(item, |fa| {
            if fa.cmd_name.is_empty() || fa.end_time < 0 {
                return None;
            }
            if fa.counter.get_count() < fa.end_time {
                return None;
            }

            // C_elm_frame_action::frame() checks the live action after the per-frame
            // do_action() callback. If that callback replaced itself, finish the
            // replacement that is now installed, not the snapshot collected earlier.
            let scn_name = fa.scn_name.clone();
            let cmd_name = fa.cmd_name.clone();
            let args = fa.args.clone();
            fa.counter.set_count(fa.end_time);
            fa.scn_name.clear();
            fa.cmd_name.clear();
            fa.end_flag = true;
            Some((scn_name, cmd_name, args))
        })?
    }

    fn end_frame_action_finish(&mut self, item: &FrameActionWork) {
        let _ = self.with_frame_action_mut(item, |fa| {
            // frame() calls reinit(true), so after finish() returns it performs
            // reinit(false). Any action started by the finish callback is cleared;
            // m_end_time is the one field reinit intentionally preserves.
            fa.reinit_without_finish();
        });
    }

    fn make_frame_action_call_args(
        frame_action_chain: Option<&Vec<i32>>,
        object_chain: Option<&Vec<i32>>,
        args: &[Value],
    ) -> Vec<Value> {
        let mut call_args = Vec::with_capacity(args.len() + 2);
        if let Some(frame_action_chain) = frame_action_chain {
            call_args.push(Value::Element(frame_action_chain.clone()));
        }
        if let Some(object_chain) = object_chain {
            call_args.push(Value::Element(object_chain.clone()));
        }
        call_args.extend(args.iter().cloned());
        call_args
    }

    fn runtime_slot_from_object_children(
        mut obj: &mut crate::runtime::globals::ObjectState,
        fallback_slot: usize,
        chain: &[i32],
        mut pos: usize,
        elm_array: i32,
        next_slot: &mut usize,
    ) -> usize {
        let object_child = crate::runtime::forms::codes::elm_value::OBJECT_CHILD;
        let mut slot = obj.runtime_slot_or(fallback_slot);
        while pos + 2 < chain.len() {
            if chain[pos] == object_child
                && (chain[pos + 1] == elm_array
                    || chain[pos + 1] == crate::runtime::forms::codes::ELM_ARRAY)
            {
                let child_idx = chain[pos + 2].max(0) as usize;
                if obj.runtime.child_objects.len() <= child_idx {
                    obj.runtime
                        .child_objects
                        .resize_with(child_idx + 1, crate::runtime::globals::ObjectState::default);
                }
                let child = &mut obj.runtime.child_objects[child_idx];
                slot = child.ensure_runtime_slot(next_slot);
                obj = child;
                pos += 3;
            } else {
                pos += 1;
            }
        }
        slot
    }

    fn runtime_slot_from_object_chain(
        &mut self,
        fallback_obj_idx: usize,
        chain: &[i32],
    ) -> usize {
        let elm_array = if self.ctx.ids.elm_array != 0 {
            self.ctx.ids.elm_array
        } else {
            crate::runtime::forms::codes::ELM_ARRAY
        };
        let Some(locator) = parse_frame_action_object_locator(chain, elm_array) else {
            return fallback_obj_idx;
        };
        let stage_form = crate::runtime::forms::stage::stage_storage_form_id(
            &self.ctx,
            locator.raw_form_id,
        );
        let Some(st) = self.ctx.globals.stage_forms.get_mut(&stage_form) else {
            return fallback_obj_idx;
        };
        let nested_slot_base = st.backend_slot_base + 100_000;
        let next_slot = st
            .next_nested_object_slot
            .entry(locator.stage_idx)
            .or_insert(nested_slot_base);

        match locator.root {
            FrameActionObjectRoot::StageObject { obj_idx, child_pos } => {
                let Some(list) = st.object_lists.get_mut(&locator.stage_idx) else {
                    return obj_idx;
                };
                let Some(obj) = list.get_mut(obj_idx) else {
                    return obj_idx;
                };
                Self::runtime_slot_from_object_children(
                    obj, obj_idx, chain, child_pos, elm_array, next_slot,
                )
            }
            FrameActionObjectRoot::MwndObject {
                mwnd_idx,
                selector,
                obj_idx,
                child_pos,
            } => {
                let Some(mwnds) = st.mwnd_lists.get_mut(&locator.stage_idx) else {
                    return obj_idx;
                };
                let Some(mwnd) = mwnds.get_mut(mwnd_idx) else {
                    return obj_idx;
                };
                let obj = if selector == crate::runtime::forms::codes::elm_value::MWND_BUTTON {
                    mwnd.button_list.get_mut(obj_idx)
                } else if selector == crate::runtime::forms::codes::elm_value::MWND_FACE {
                    mwnd.face_list.get_mut(obj_idx)
                } else {
                    mwnd.object_list.get_mut(obj_idx)
                };
                let Some(obj) = obj else {
                    return obj_idx;
                };
                Self::runtime_slot_from_object_children(
                    obj, obj_idx, chain, child_pos, elm_array, next_slot,
                )
            }
            FrameActionObjectRoot::BtnSelItemObject {
                item_idx,
                obj_idx,
                child_pos,
            } => {
                let Some(items) = st.btnselitem_lists.get_mut(&locator.stage_idx) else {
                    return obj_idx;
                };
                let Some(item) = items.get_mut(item_idx) else {
                    return obj_idx;
                };
                let Some(obj) = item.object_list.get_mut(obj_idx) else {
                    return obj_idx;
                };
                Self::runtime_slot_from_object_children(
                    obj, obj_idx, chain, child_pos, elm_array, next_slot,
                )
            }
        }
    }

    fn set_frame_action_current_object(
        &mut self,
        item: &FrameActionWork,
    ) -> (Option<(i64, usize)>, Option<Vec<i32>>) {
        let prev_target = self.ctx.globals.current_stage_object;
        let prev_chain = self.ctx.globals.current_object_chain.clone();
        if let Some(chain) = item.object_chain.clone() {
            let elm_array = if self.ctx.ids.elm_array != 0 {
                self.ctx.ids.elm_array
            } else {
                crate::runtime::forms::codes::ELM_ARRAY
            };
            if let Some(locator) = parse_frame_action_object_locator(&chain, elm_array) {
                let top_idx = frame_action_locator_object_idx(locator);
                let runtime_slot = self.runtime_slot_from_object_chain(top_idx, &chain);
                self.ctx.globals.current_stage_object = Some((locator.stage_idx, runtime_slot));
                self.ctx.globals.current_object_chain = Some(chain);
            } else {
                self.ctx.globals.current_stage_object = None;
                self.ctx.globals.current_object_chain = None;
            }
        } else {
            self.ctx.globals.current_stage_object = None;
            self.ctx.globals.current_object_chain = None;
        }
        (prev_target, prev_chain)
    }

    fn restore_frame_action_current_object(
        &mut self,
        prev_target: Option<(i64, usize)>,
        prev_chain: Option<Vec<i32>>,
    ) {
        self.ctx.globals.current_stage_object = prev_target;
        self.ctx.globals.current_object_chain = prev_chain;
    }

    fn frame_action_work_from_pending_finish(
        &self,
        pending: &PendingFrameActionFinish,
    ) -> FrameActionWork {
        let object_chain = pending.object_chain.clone();
        let elm_array = if self.ctx.ids.elm_array != 0 {
            self.ctx.ids.elm_array
        } else {
            crate::runtime::forms::codes::ELM_ARRAY
        };
        let (stage_idx, obj_idx) = object_chain
            .as_deref()
            .and_then(|chain| parse_frame_action_object_locator(chain, elm_array))
            .map(|locator| (locator.stage_idx, frame_action_locator_object_idx(locator)))
            .unwrap_or((-1, usize::MAX));

        let ch_idx = if let Some(chain) = object_chain.as_ref() {
            let base = chain.len();
            if pending.frame_action_chain.len() >= base + 3
                && pending.frame_action_chain[base]
                    == crate::runtime::forms::codes::elm_value::OBJECT_FRAME_ACTION_CH
                && pending.frame_action_chain[base + 1] == crate::runtime::forms::codes::ELM_ARRAY
            {
                Some(pending.frame_action_chain[base + 2].max(0) as usize)
            } else {
                None
            }
        } else if pending.frame_action_chain.len() >= 3
            && pending.frame_action_chain[1] == crate::runtime::forms::codes::ELM_ARRAY
        {
            Some(pending.frame_action_chain[2].max(0) as usize)
        } else {
            None
        };

        let global_form_id = if object_chain.is_none() {
            pending
                .frame_action_chain
                .first()
                .copied()
                .map(|v| v as u32)
        } else {
            None
        };

        FrameActionWork {
            stage_idx,
            obj_idx,
            ch_idx,
            global_form_id,
            object_chain,
            frame_action_chain: Some(pending.frame_action_chain.clone()),
            scn_name: pending.scn_name.clone(),
            cmd_name: pending.cmd_name.clone(),
            args: pending.args.clone(),
            count: pending.end_time,
            end_time: pending.end_time,
        }
    }

    fn run_pending_frame_action_finish(&mut self, pending: PendingFrameActionFinish) -> Result<()> {
        if pending.cmd_name.is_empty() {
            return Ok(());
        }
        let item = self.frame_action_work_from_pending_finish(&pending);
        let final_count = if pending.end_time == -1 {
            0
        } else {
            pending.end_time
        };

        // START/START_REAL/END have already modified the live slot by the time the
        // deferred callback is drained. Temporarily restore the complete old state
        // and put it into C_elm_frame_action::finish()'s callback-visible shape.
        // After the callback, restore the outer state: this reproduces the C++
        // reinit(true) ordering without letting the old finish overwrite a new START.
        let outer_state = self.with_frame_action_mut(&item, |fa| {
            let outer_state = fa.clone();
            let mut finishing = pending.snapshot.clone();
            finishing.scn_name.clear();
            finishing.cmd_name.clear();
            finishing.counter.set_count(final_count);
            finishing.end_flag = true;
            *fa = finishing;
            outer_state
        });

        let call_args = Self::make_frame_action_call_args(
            item.frame_action_chain.as_ref(),
            item.object_chain.as_ref(),
            &pending.args,
        );
        let (prev_target, prev_chain) = self.set_frame_action_current_object(&item);
        let result = self.run_scene_user_cmd_inline(
            Some(&pending.scn_name),
            &pending.cmd_name,
            &call_args,
            self.cfg.fm_void,
            true,
        );
        self.restore_frame_action_current_object(prev_target, prev_chain);

        if pending.reinit_after_finish {
            let _ = self.with_frame_action_mut(&item, |fa| {
                fa.reinit_without_finish();
            });
        } else if let Some(outer_state) = outer_state {
            let _ = self.with_frame_action_mut(&item, |fa| {
                *fa = outer_state;
            });
        }
        if let Err(e) = result {
            self.ctx.unknown.record_note(&format!(
                "frame_action.finish.failed:{}:{}:{e}",
                pending.scn_name, pending.cmd_name
            ));
        }
        Ok(())
    }

    fn run_pending_button_action(&mut self, action: PendingButtonAction) -> Result<()> {
        // Original tona3/Siglus copies runtime input into script input before
        // frame_main_proc(), while object button actions are decided later from
        // the element/frame pass.  A scene or user command entered by a button
        // action must therefore not observe the same mouse down/up stock that
        // decided the button.  Clear edge stocks here while preserving held
        // state and mouse position.
        self.ctx.input.use_current();
        self.script_input_synced_this_frame = false;

        match action.kind {
            PendingButtonActionKind::UserCall {
                scn_name,
                cmd_name,
                z_no,
            } => {
                if scn_name.is_empty() {
                    return Ok(());
                }
                if self.runtime_options.sg_debug {
                    eprintln!(
                        "[SG_DEBUG][BUTTON] run action scene={} cmd={} z_no={}",
                        scn_name, cmd_name, z_no
                    );
                }
                if !cmd_name.is_empty() {
                    let _ = self.enter_scene_user_cmd_call(Some(&scn_name), &cmd_name, &[])?;
                } else if z_no >= 0 {
                    self.farcall_scene_name_ex(
                        &scn_name,
                        z_no as i32,
                        self.cfg.fm_void,
                        true,
                        &[],
                    )?;
                }
            }
            PendingButtonActionKind::Syscom {
                sys_type,
                sys_type_opt,
                mode,
            } => {
                self.run_pending_button_syscom_action(sys_type, sys_type_opt, mode)?;
            }
        }
        Ok(())
    }

    #[inline(always)]
    fn syscom_proc_trace_enabled(&self) -> bool {
        self.runtime_options.syscom_proc_trace
    }

    fn syscom_trace_state(&self) -> String {
        let st = &self.ctx.globals.syscom;
        let msgbk_form = self.ctx.ids.form_global_msgbk;
        let msgbk_count = self
            .ctx
            .globals
            .msgbk_forms
            .get(&msgbk_form)
            .map(|m| m.history.len())
            .unwrap_or(0);
        let msgbk_visible_count = self
            .ctx
            .globals
            .msgbk_forms
            .get(&msgbk_form)
            .map(|m| {
                m.history
                    .iter()
                    .filter(|entry| {
                        entry.pct_flag
                            || !entry.msg_str.is_empty()
                            || !entry.disp_name.is_empty()
                            || !entry.original_name.is_empty()
                            || !entry.koe_no_list.is_empty()
                    })
                    .count()
            })
            .unwrap_or(0);
        format!(
            "read_skip={} auto_mode={} hide_mwnd={} msg_back_open={} msg_back_enable={} pending_proc={:?} msgbk_form={} msgbk_count={} msgbk_visible_count={} mwnd_waiting={} mwnd_visible_chars={} mwnd_wait_len={} msg_chars={}",
            st.read_skip.onoff,
            st.auto_mode.onoff,
            st.hide_mwnd.onoff,
            st.msg_back_open,
            st.msg_back.check_enabled(),
            st.pending_proc,
            msgbk_form,
            msgbk_count,
            msgbk_visible_count,
            self.ctx.ui.message_waiting(),
            self.ctx.ui.message_visible_chars(),
            self.ctx.ui.message_wait_message_len(),
            self.ctx.ui.message_text().unwrap_or("").chars().count()
        )
    }

    fn syscom_button_op(sys_type: i64) -> Option<(i32, &'static str)> {
        use crate::runtime::forms::codes::syscom_op;
        Some(match sys_type {
            1 => (syscom_op::CALL_SAVE_MENU, "CALL_SAVE_MENU"),
            2 => (syscom_op::CALL_LOAD_MENU, "CALL_LOAD_MENU"),
            3 => (syscom_op::SET_READ_SKIP_ONOFF_FLAG, "SET_READ_SKIP_ONOFF_FLAG"),
            4 => (syscom_op::SET_AUTO_MODE_ONOFF_FLAG, "SET_AUTO_MODE_ONOFF_FLAG"),
            5 => (syscom_op::RETURN_TO_SEL, "RETURN_TO_SEL"),
            6 => (syscom_op::SET_HIDE_MWND_ONOFF_FLAG, "SET_HIDE_MWND_ONOFF_FLAG"),
            7 => (syscom_op::OPEN_MSG_BACK, "OPEN_MSG_BACK"),
            8 => (syscom_op::REPLAY_KOE, "REPLAY_KOE"),
            9 => (syscom_op::QUICK_SAVE, "QUICK_SAVE"),
            10 => (syscom_op::QUICK_LOAD, "QUICK_LOAD"),
            11 => (syscom_op::CALL_CONFIG_MENU, "CALL_CONFIG_MENU"),
            12 => (syscom_op::SET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG, "SET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG"),
            13 => (syscom_op::SET_LOCAL_EXTRA_MODE_VALUE, "SET_LOCAL_EXTRA_MODE_VALUE"),
            14 => (syscom_op::SET_GLOBAL_EXTRA_SWITCH_ONOFF, "SET_GLOBAL_EXTRA_SWITCH_ONOFF"),
            15 => (syscom_op::SET_GLOBAL_EXTRA_MODE_VALUE, "SET_GLOBAL_EXTRA_MODE_VALUE"),
            _ => return None,
        })
    }

    fn dispatch_syscom_button_op(&mut self, op: i32, params: &[Value]) -> Result<bool> {
        // In this VM, form dispatch is rooted at the GLOBAL.SYSCOM element id
        // (normally 63), not at the FM_SYSCOM type id (1600).  A normal script
        // call reaches syscom.rs as the element chain [GLOBAL_SYSCOM, op].
        // Button actions must use the same root or global::dispatch_form()
        // never reaches syscom::dispatch(), and the trace shows handled=false.
        let form_id = if self.ctx.ids.form_global_syscom != 0 {
            self.ctx.ids.form_global_syscom as i32
        } else {
            constants::global_form::SYSCOM as i32
        };

        let saved_call = self.ctx.vm_call.take();
        let saved_stack_len = self.ctx.stack.len();
        self.ctx.vm_call = Some(runtime::VmCallMeta {
            element: vec![form_id, op],
            al_id: 0,
            ret_form: self.cfg.fm_void as i64,
        });

        let result = runtime::dispatch_form_code(&mut self.ctx, form_id as u32, params);

        self.ctx.vm_call = saved_call;
        self.ctx.stack.truncate(saved_stack_len);
        result
    }

    fn run_pending_button_syscom_action(
        &mut self,
        sys_type: i64,
        sys_type_opt: i64,
        mode: i64,
    ) -> Result<()> {
        let trace = self.syscom_proc_trace_enabled();
        let Some((op, op_name)) = Self::syscom_button_op(sys_type) else {
            if trace {
                eprintln!(
                    "[SYSCOM_PROC_TRACE] button sys_type={} sys_opt={} mode={} resolved=UNKNOWN before {}",
                    sys_type,
                    sys_type_opt,
                    mode,
                    self.syscom_trace_state()
                );
            }
            return Ok(());
        };

        if matches!(sys_type, 1 | 9) {
            crate::runtime::forms::syscom::prepare_runtime_save_thumb_capture(&mut self.ctx);
        }

        // This follows C_elm_object::check_button_action() in the original engine:
        // system buttons call the SYSCOM operation directly.  The form dispatcher
        // expects CommandContext::vm_call to carry the current element chain, so
        // constructing Value::Element in the argument list is not enough here.
        // Button actions also ignore command return values, so any values pushed
        // by the generic SYSCOM command handler are discarded after dispatch.
        let params: Vec<Value> = match sys_type {
            1 | 2 => Vec::new(),
            3 | 4 => vec![Value::Int(if mode == 0 { 1 } else { 0 })],
            6 => vec![Value::Int(1)],
            5 => vec![Value::Int(1), Value::Int(1), Value::Int(1)],
            7 | 8 | 11 => Vec::new(),
            9 => vec![Value::Int(sys_type_opt), Value::Int(1), Value::Int(1)],
            10 => vec![
                Value::Int(sys_type_opt),
                Value::Int(1),
                Value::Int(1),
                Value::Int(1),
            ],
            12 | 14 => vec![
                Value::Int(sys_type_opt),
                Value::Int(if mode == 0 { 1 } else { 0 }),
            ],
            13 | 15 => vec![Value::Int(sys_type_opt), Value::Int(mode + 1)],
            _ => Vec::new(),
        };
        if trace {
            eprintln!(
                "[SYSCOM_PROC_TRACE] button sys_type={} sys_opt={} mode={} resolved={}({}) params={:?} before {}",
                sys_type,
                sys_type_opt,
                mode,
                op_name,
                op,
                params,
                self.syscom_trace_state()
            );
        }
        let dispatch_result = self.dispatch_syscom_button_op(op, &params);
        if trace {
            let status = match dispatch_result.as_ref() {
                Ok(handled) => format!("ok handled={}", handled),
                Err(err) => format!("err={}", err),
            };
            eprintln!(
                "[SYSCOM_PROC_TRACE] after resolved={}({}) status={} {}",
                op_name,
                op,
                status,
                self.syscom_trace_state()
            );
        }
        dispatch_result?;
        Ok(())
    }

    fn drain_pending_button_actions(&mut self) -> Result<()> {
        let mut budget = 64usize;
        while !self.ctx.globals.pending_button_actions.is_empty() {
            if budget == 0 {
                bail!("button action queue did not drain");
            }
            budget -= 1;
            let pending = std::mem::take(&mut self.ctx.globals.pending_button_actions);
            for action in pending {
                self.run_pending_button_action(action)?;
            }
        }
        Ok(())
    }

    pub fn process_pending_button_actions(&mut self) -> Result<()> {
        self.drain_pending_button_actions()
    }

    fn drain_pending_frame_action_finishes(&mut self) -> Result<()> {
        let mut budget = 64usize;
        while !self.ctx.globals.pending_frame_action_finishes.is_empty() {
            if budget == 0 {
                bail!("frame action finish queue did not drain");
            }
            budget -= 1;
            let pending = std::mem::take(&mut self.ctx.globals.pending_frame_action_finishes);
            for item in pending {
                self.run_pending_frame_action_finish(item)?;
            }
        }
        // C_elm_excall::free() performs finish callbacks synchronously and only
        // then releases F/counters/frame-action channels/stages. Runtime form
        // dispatch cannot recurse into the VM directly, so EXCALL.FREE defers
        // this final release until the finish queue has reached the same point.
        crate::runtime::forms::excall::finalize_pending_free(&mut self.ctx);
        Ok(())
    }

    pub fn tick_frame(&mut self) -> Result<()> {
        let trace = self.runtime_options.tick_trace || self.runtime_options.frame_action_trace;
        if trace {
            eprintln!(
                "[SG_TICK_TRACE] tick_frame start blocked={} halted={} scene={:?}",
                self.is_blocked(),
                self.halted,
                self.current_scene_name()
            );
        }
        self.drain_pending_button_actions()?;
        if self.ctx.globals.syscom.pending_proc.is_some() {
            if trace || self.runtime_options.sg_debug {
                eprintln!(
                    "[SG_DEBUG][SYSCOM_PROC] stop frame tick before frame actions pending_proc={:?}",
                    self.ctx.globals.syscom.pending_proc
                );
            }
            return Ok(());
        }
        if self.ctx.globals.script.frame_action_time_stop_flag && trace {
            eprintln!(
                "[SG_TICK_TRACE] frame_action_time_stop_flag set; executing callbacks with frozen frame-action time"
            );
        }

        // C++ C_tnm_eng::frame() advances element time before frame_action_proc().
        // FRAME_ACTION callbacks read the already advanced counter value, then
        // C_elm_frame_action::frame() performs the end check later in the same
        // frame.  Keep the same order here; otherwise callbacks such as the MWND
        // rotating circle animation repeatedly observe the previous count.
        self.ctx.tick_frame();

        if let Some(result) = self.ctx.take_pending_sel_point_result() {
            // Original decide(): push result, then tnm_set_sel_point().  Keep
            // the live command return path untouched while storing a resume
            // point whose int stack has the same observable shape.
            self.int_stack.push(result);
            let point = self.make_resume_point();
            let _ = self.int_stack.pop();
            self.sel_point_stack.clear();
            self.sel_point_stack.push(point);
        }

        let mut work: Vec<FrameActionWork> = Vec::new();
        let mut global_form_ids: Vec<u32> =
            self.ctx.globals.frame_actions.keys().copied().collect();
        global_form_ids.sort_unstable();
        for form_id in global_form_ids {
            let Some(fa) = self.ctx.globals.frame_actions.get(&form_id) else {
                continue;
            };
            if !fa.cmd_name.is_empty() {
                work.push(FrameActionWork {
                    stage_idx: -1,
                    obj_idx: usize::MAX,
                    ch_idx: None,
                    global_form_id: Some(form_id),
                    object_chain: None,
                    frame_action_chain: Some(vec![form_id as i32]),
                    scn_name: fa.scn_name.clone(),
                    cmd_name: fa.cmd_name.clone(),
                    args: fa.args.clone(),
                    count: fa.counter.get_count(),
                    end_time: fa.end_time,
                });
            }
        }
        let mut global_list_form_ids: Vec<u32> = self
            .ctx
            .globals
            .frame_action_lists
            .keys()
            .copied()
            .collect();
        global_list_form_ids.sort_unstable();
        for form_id in global_list_form_ids {
            let Some(list) = self.ctx.globals.frame_action_lists.get(&form_id) else {
                continue;
            };
            for (idx, fa) in list.iter().enumerate() {
                if !fa.cmd_name.is_empty() {
                    work.push(FrameActionWork {
                        stage_idx: -1,
                        obj_idx: usize::MAX,
                        ch_idx: Some(idx),
                        global_form_id: Some(form_id),
                        object_chain: None,
                        frame_action_chain: Some(vec![
                            form_id as i32,
                            crate::runtime::forms::codes::ELM_ARRAY,
                            idx as i32,
                        ]),
                        scn_name: fa.scn_name.clone(),
                        cmd_name: fa.cmd_name.clone(),
                        args: fa.args.clone(),
                        count: fa.counter.get_count(),
                        end_time: fa.end_time,
                    });
                }
            }
        }
        let mut stage_form_ids: Vec<u32> = self.ctx.globals.stage_forms.keys().copied().collect();
        stage_form_ids.sort_unstable();
        for form_id in stage_form_ids {
            let Some(st) = self.ctx.globals.stage_forms.get(&form_id) else {
                continue;
            };
            let mut stage_ids: Vec<i64> = st.object_lists.keys().copied().collect();
            stage_ids.sort_unstable();
            for stage_idx in stage_ids {
                let Some(objs) = st.object_lists.get(&stage_idx) else {
                    continue;
                };
                for (obj_idx, obj) in objs.iter().enumerate() {
                    if st.is_embedded_object_slot(stage_idx, obj_idx)
                        || !st.object_slot_is_used(stage_idx, obj_idx)
                    {
                        continue;
                    }
                    let object_chain = vec![
                        form_id as i32,
                        self.ctx.ids.elm_array,
                        stage_idx as i32,
                        self.ctx.ids.stage_elm_object,
                        self.ctx.ids.elm_array,
                        obj_idx as i32,
                    ];
                    Self::collect_object_frame_action_work_recursive(
                        obj,
                        stage_idx,
                        obj_idx,
                        object_chain,
                        &mut work,
                    );
                }
            }

            let mut mwnd_stage_ids: Vec<i64> = st.mwnd_lists.keys().copied().collect();
            mwnd_stage_ids.sort_unstable();
            for stage_idx in mwnd_stage_ids {
                let Some(mwnds) = st.mwnd_lists.get(&stage_idx) else {
                    continue;
                };
                for (mwnd_idx, mwnd) in mwnds.iter().enumerate() {
                    for (obj_idx, obj) in mwnd.button_list.iter().enumerate() {
                        let object_chain = vec![
                            form_id as i32,
                            self.ctx.ids.elm_array,
                            stage_idx as i32,
                            crate::runtime::forms::codes::elm_value::STAGE_MWND,
                            self.ctx.ids.elm_array,
                            mwnd_idx as i32,
                            crate::runtime::forms::codes::elm_value::MWND_BUTTON,
                            self.ctx.ids.elm_array,
                            obj_idx as i32,
                        ];
                        Self::collect_object_frame_action_work_recursive(
                            obj,
                            stage_idx,
                            obj_idx,
                            object_chain,
                            &mut work,
                        );
                    }
                    for (obj_idx, obj) in mwnd.face_list.iter().enumerate() {
                        let object_chain = vec![
                            form_id as i32,
                            self.ctx.ids.elm_array,
                            stage_idx as i32,
                            crate::runtime::forms::codes::elm_value::STAGE_MWND,
                            self.ctx.ids.elm_array,
                            mwnd_idx as i32,
                            crate::runtime::forms::codes::elm_value::MWND_FACE,
                            self.ctx.ids.elm_array,
                            obj_idx as i32,
                        ];
                        Self::collect_object_frame_action_work_recursive(
                            obj,
                            stage_idx,
                            obj_idx,
                            object_chain,
                            &mut work,
                        );
                    }
                    for (obj_idx, obj) in mwnd.object_list.iter().enumerate() {
                        let object_chain = vec![
                            form_id as i32,
                            self.ctx.ids.elm_array,
                            stage_idx as i32,
                            crate::runtime::forms::codes::elm_value::STAGE_MWND,
                            self.ctx.ids.elm_array,
                            mwnd_idx as i32,
                            crate::runtime::forms::codes::elm_value::MWND_OBJECT,
                            self.ctx.ids.elm_array,
                            obj_idx as i32,
                        ];
                        Self::collect_object_frame_action_work_recursive(
                            obj,
                            stage_idx,
                            obj_idx,
                            object_chain,
                            &mut work,
                        );
                    }
                }
            }
        }
        if trace {
            eprintln!("[SG_TICK_TRACE] frame_action work items={}", work.len());
        }
        // C++ drives FRAME_ACTION from the engine proc loop independently of the
        // current script call depth. Do not stall object callbacks while a user
        // command/excall is active; MWND child setup relies on these callbacks.
        for item in work {
            if trace {
                eprintln!(
                    "[SG_TICK_TRACE] invoke stage={} obj={} ch={:?} global={:?} cmd={} count={} end_time={} args={:?}",
                    item.stage_idx,
                    item.obj_idx,
                    item.ch_idx,
                    item.global_form_id,
                    item.cmd_name,
                    item.count,
                    item.end_time,
                    item.args
                );
            }
            // Original order is C_tnm_eng::frame_action_proc() do_action() first,
            // then C_elm_frame_action::frame() checks for finish later in the frame.
            // This matters for end_time == 0 and for callbacks that replace their own
            // frame action.
            let call_args = Self::make_frame_action_call_args(
                item.frame_action_chain.as_ref(),
                item.object_chain.as_ref(),
                &item.args,
            );
            let (prev_target, prev_chain) = self.set_frame_action_current_object(&item);
            if let Err(e) = self.run_scene_user_cmd_inline(
                Some(&item.scn_name),
                &item.cmd_name,
                &call_args,
                self.cfg.fm_void,
                true,
            ) {
                self.ctx.unknown.record_note(&format!(
                    "frame_action.call.failed:{}:{}:{e}",
                    item.scn_name, item.cmd_name
                ));
            }
            self.restore_frame_action_current_object(prev_target, prev_chain);

            if let Some((finish_scn_name, finish_cmd_name, finish_args)) =
                self.begin_frame_action_finish(&item)
            {
                if trace {
                    eprintln!(
                        "[SG_TICK_TRACE] finish stage={} obj={} ch={:?} global={:?} cmd={} args={:?}",
                        item.stage_idx,
                        item.obj_idx,
                        item.ch_idx,
                        item.global_form_id,
                        finish_cmd_name,
                        finish_args
                    );
                }
                let finish_call_args = Self::make_frame_action_call_args(
                    item.frame_action_chain.as_ref(),
                    item.object_chain.as_ref(),
                    &finish_args,
                );
                let (prev_target, prev_chain) = self.set_frame_action_current_object(&item);
                if let Err(e) = self.run_scene_user_cmd_inline(
                    Some(&finish_scn_name),
                    &finish_cmd_name,
                    &finish_call_args,
                    self.cfg.fm_void,
                    true,
                ) {
                    self.ctx.unknown.record_note(&format!(
                        "frame_action.finish_call.failed:{}:{}:{e}",
                        finish_scn_name, finish_cmd_name
                    ));
                }
                self.restore_frame_action_current_object(prev_target, prev_chain);
                self.end_frame_action_finish(&item);
            }
        }
        if trace {
            eprintln!(
                "[SG_TICK_TRACE] after frame-action callbacks blocked={} halted={}",
                self.is_blocked(),
                self.halted
            );
        }
        self.script_input_synced_this_frame = false;
        Ok(())
    }

    pub fn restart_scene_name(&mut self, scene_name: &str, z_no: i32) -> Result<()> {
        // C++ restart paths call tnm_finish_local()/tnm_reinit_local() before
        // resolving and entering the target scene.  Keep the same ordering so
        // local teardown cannot depend on the new scene having loaded already.
        self.ctx.reset_for_scene_restart();
        let (stream, scene_no) = self.load_scene_stream(scene_name, z_no)?;
        self.stream = stream;
        self.int_stack.clear();
        self.str_stack.clear();
        self.element_points.clear();
        self.call_stack.clear();
        self.call_stack.push(self.scene_base_call());
        self.gosub_return_stack.clear();
        self.user_props.clear();
        self.scene_user_props.clear();
        self.scene_stack.clear();
        self.save_point = None;
        self.ctx.local_save_snapshot = None;
        self.sel_point_stack.clear();
        self.current_scene_no = Some(scene_no);
        self.current_scene_name = Some(scene_name.to_string());
        self.current_line_no = -1;
        self.ctx.current_scene_no = Some(scene_no as i64);
        self.ctx.current_scene_name = Some(scene_name.to_string());
        self.ctx.current_line_no = -1;
        self.legacy_saved_active_only = false;
        self.halted = false;
        self.delayed_ret_form = None;
        Ok(())
    }

    fn make_resume_point(&self) -> VmResumePoint<'a> {
        VmResumePoint {
            stream: self.stream.clone(),
            user_cmd_names: self.user_cmd_names.clone(),
            call_cmd_names: self.call_cmd_names.clone(),
            int_stack: self.int_stack.clone(),
            str_stack: self.str_stack.clone(),
            element_points: self.element_points.clone(),
            call_stack: self.call_stack.clone(),
            gosub_return_stack: self.gosub_return_stack.clone(),
            user_props: self.user_props.clone(),
            scene_user_props: self.scene_user_props.clone(),
            current_scene_no: self.current_scene_no,
            current_scene_name: self.current_scene_name.clone(),
            current_line_no: self.current_line_no,
            globals: self.ctx.globals.clone(),
        }
    }

    fn restore_resume_point(&mut self, point: VmResumePoint<'a>) {
        self.stream = point.stream;
        self.user_cmd_names = point.user_cmd_names;
        self.call_cmd_names = point.call_cmd_names;
        self.int_stack = point.int_stack;
        self.str_stack = point.str_stack;
        self.element_points = point.element_points;
        self.call_stack = point.call_stack;
        self.gosub_return_stack = point.gosub_return_stack;
        self.user_props = point.user_props;
        self.scene_user_props = point.scene_user_props;
        self.current_scene_no = point.current_scene_no;
        self.current_scene_name = point.current_scene_name;
        self.current_line_no = point.current_line_no;
        let mut restored_globals = point.globals;
        restored_globals.syscom.pending_proc = None;
        restored_globals.syscom.menu_open = false;
        restored_globals.syscom.menu_kind = None;
        restored_globals.syscom.fallback_dialog = None;
        restored_globals.syscom.fallback_origin = None;
        restored_globals.syscom.msg_back_open = false;
        self.ctx.globals = restored_globals;
        self.ctx.current_scene_no = self.current_scene_no.map(|v| v as i64);
        self.ctx.current_scene_name = self.current_scene_name.clone();
        self.ctx.current_line_no = self.current_line_no as i64;
        self.ctx.wait = runtime::wait::VmWait::default();
        self.ctx.stack.clear();
        self.halted = false;
        self.delayed_ret_form = None;
    }

    pub fn has_sel_point(&self) -> bool {
        !self.sel_point_stack.is_empty()
    }

    pub fn restore_last_sel_point(&mut self) -> bool {
        let Some(point) = self.sel_point_stack.last().cloned() else {
            return false;
        };
        self.restore_resume_point(point);
        true
    }

    pub fn step(&mut self) -> Result<bool> {
        match self.step_inner(true) {
            Ok(running) => Ok(running),
            Err(err) => {
                // A decoded instruction may already have consumed operands before
                // reporting an error.  Continuing from that partial PC would make
                // the following frame interpret an operand byte as a fresh opcode.
                // The original engine treats such script errors as fatal, so stop
                // the VM at the first real failure instead of cascading into
                // CD_NONE / invalid element-point errors.
                self.halted = true;
                Err(err)
            }
        }
    }

    /// Reset the infinite-loop guard for one C++-style frame_main_proc pass.
    /// This is not a scheduling quota; it only preserves SIGLUS_VM_MAX_STEPS
    /// as a hard error if a script never reaches a proc/wait boundary.
    pub fn begin_script_proc_pump(&mut self) {
        self.steps = 0;
        self.ctx.begin_frame_main_proc_pass();
    }

    /// Execute one standalone SCRIPT proc pass. Direct callers get a fresh
    /// infinite-loop guard, while the winit shell uses run_script_proc_continue()
    /// inside its C++-style frame_main_proc loop.
    pub fn run_script_proc(&mut self) -> Result<bool> {
        self.begin_script_proc_pump();
        self.run_script_proc_continue()
    }

    /// Execute the current SCRIPT proc the same way the original engine's
    /// `tnm_proc_script()` does: keep stepping while the current proc is SCRIPT,
    /// and return only when a command changes the proc, enters a wait, returns,
    /// or stops the VM. There is no per-frame instruction quota here.
    pub fn run_script_proc_continue(&mut self) -> Result<bool> {
        if self.halted {
            return Ok(false);
        }
        // A completed MESSAGE_KEY_WAIT can itself request the DISP boundary
        // used by the original skip-rate limiter. Detect that boundary even
        // though wait_poll() has just made the VM unblocked.
        let proc_generation_before_wait = self.ctx.proc_generation();
        if self.is_blocked() {
            return Ok(true);
        }
        if self.ctx.proc_generation() != proc_generation_before_wait {
            return Ok(true);
        }

        if !self.script_input_synced_this_frame {
            self.ctx.sync_script_input_from_runtime();
            self.ctx.begin_input_frame();
            self.ctx.input.next_frame();
            self.script_input_synced_this_frame = true;
        }

        loop {
            let proc_generation_before = self.ctx.proc_generation();
            let running = match self.step_inner(true) {
                Ok(running) => running,
                Err(err) => {
                    // Do not retry a half-consumed instruction on the next redraw.
                    // Retrying from its operand field is what produced the later
                    // CD_NONE and invalid-element-point cascade.
                    self.halted = true;
                    return Err(err);
                }
            };
            if !running || self.halted {
                return Ok(running);
            }

            if self.is_blocked() {
                return Ok(true);
            }
            if self.ctx.proc_generation() != proc_generation_before {
                return Ok(true);
            }
        }
    }

    #[allow(dead_code)]
    fn step_inner(&mut self, respect_wait: bool) -> Result<bool> {
        self.yield_safe_after_step = false;
        if self.halted {
            return Ok(false);
        }
        if self.current_scene_no != self.diag_last_scene_no {
            if sg_scene_trace() {
                log::warn!(
                    "[SG-DIAG-13] scene ctx: scene={:?} no={:?} line={} pc=0x{:x} call_depth={} scene_stack={} (prev={:?})",
                    self.current_scene_name,
                    self.current_scene_no,
                    self.current_line_no,
                    self.stream.get_prg_cntr(),
                    self.call_stack.len(),
                    self.scene_stack.len(),
                    self.diag_last_scene_no
                );
            }
            self.diag_last_scene_no = self.current_scene_no;
        }

        // Normal scene execution is blocked by WAIT / WAIT_KEY.
        // Frame-action inline callbacks bypass this outer wait guard so the
        // frame phase can run object callbacks while the main script is waiting.
        if respect_wait {
            let blocked = self.ctx.wait_poll();
            if blocked {
                return Ok(true);
            }
        }

        // If the main script yielded a delayed return, materialize it only when
        // the normal script pump resumes. Frame-action callbacks intentionally
        // bypass the outer wait guard, so letting them take this value would
        // resume the blocked statement before the C++ wait proc has produced
        // its return value.
        if respect_wait {
            let delayed = self
                .call_stack
                .last_mut()
                .and_then(|frame| frame.delayed_ret_form.take());
            if let Some(rf) = delayed {
                if self.ctx.stack.is_empty() {
                    self.push_default_for_ret(rf);
                } else {
                    self.take_ctx_return(rf)?;
                }
            }
        }

        if self.cfg.max_steps > 0 && self.steps >= self.cfg.max_steps {
            let scene = self.current_scene_name.as_deref().unwrap_or("<none>");
            let scene_no = self
                .current_scene_no
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".to_string());
            bail!(
                "VM reached SIGLUS_VM_MAX_STEPS={} (possible infinite loop) scene={} scene_no={} line={} pc=0x{:x}",
                self.cfg.max_steps,
                scene,
                scene_no,
                self.current_line_no,
                self.stream.get_prg_cntr()
            );
        }
        self.steps += 1;

        let pc_before = self.stream.get_prg_cntr();
        let opcode = match self.stream.pop_u8() {
            Ok(v) => v,
            Err(_) => {
                if self.at_cross_scene_return_boundary()
                    && self.return_from_scene(Vec::new())?
                {
                    return Ok(true);
                }
                log::warn!(
                    "[SG-DIAG-1] script stream exhausted (halt): scene={:?} scene_no={:?} line={} pc=0x{:x} at_cross_scene_boundary={}",
                    self.current_scene_name,
                    self.current_scene_no,
                    self.current_line_no,
                    pc_before,
                    self.at_cross_scene_return_boundary()
                );
                eprintln!(
                    "[SG-DIAG-1] script stream exhausted (halt): scene={:?} scene_no={:?} line={} pc=0x{:x} at_cross_scene_boundary={}",
                    self.current_scene_name,
                    self.current_scene_no,
                    self.current_line_no,
                    pc_before,
                    self.at_cross_scene_return_boundary()
                );
                self.halted = true;
                return Ok(false);
            }
        };

        self.vm_trace_opcode(pc_before, opcode, "before");

        sg_ring_push(
            pc_before,
            opcode,
            self.int_stack.len(),
            self.element_points.len(),
            self.int_stack.last().copied(),
            self.stream.debug_len(),
            self.call_stack.len(),
        );

        match opcode {
            CD_NL => {
                let line_no = self.stream.pop_i32()?;
                self.current_line_no = line_no;
                self.ctx.current_line_no = line_no as i64;
                self.yield_safe_after_step = true;
                // Compact element continuation is statement-local, but some title/menu code
                // keeps a base object element alive across `NL` boundaries and continues to use
                // compact child/property syntax on the following line. So we only drop the
                // continuation context when there is no live element on the VM stack anymore.
                if self.element_points.is_empty() {
                    self.ctx.globals.current_object_chain = None;
                    self.ctx.globals.current_stage_object = None;
                }
            }

            CD_PUSH => {
                let form_code = self.stream.pop_i32()?;
                self.exec_push(form_code)?;
            }
            CD_POP => {
                let form_code = self.stream.pop_i32()?;
                self.exec_pop(form_code)?;
            }
            CD_COPY => {
                let form_code = self.stream.pop_i32()?;
                self.exec_copy(form_code)?;
            }

            CD_ELM_POINT => {
                self.element_points.push(self.int_stack.len());
                vm_trace!(self,
                    None,
                    format!("ELM_POINT push start={} ", self.int_stack.len()),
                );
            }
            CD_COPY_ELM => {
                self.exec_copy_element()?;
            }

            CD_PROPERTY => {
                let elm = self.pop_element()?;
                vm_trace!(self, None, format!("CD_PROPERTY elm={:?}", elm));
                self.exec_property(elm)?;
            }
            CD_DEC_PROP => {
                let form_code = self.stream.pop_i32()?;
                let prop_id = self.stream.pop_i32()?;

                let size = if form_code == self.cfg.fm_intlist || form_code == self.cfg.fm_strlist {
                    self.pop_int()?.max(0) as usize
                } else {
                    0usize
                };
                let prop_element = vec![constants::elm::create(
                    constants::elm::OWNER_CALL_PROP,
                    0,
                    prop_id,
                )];
                let value = if form_code == self.cfg.fm_int {
                    CallPropValue::Int(0)
                } else if form_code == self.cfg.fm_str {
                    CallPropValue::Str(String::new())
                } else if form_code == self.cfg.fm_intlist {
                    CallPropValue::IntList(vec![0; size])
                } else if form_code == self.cfg.fm_strlist {
                    CallPropValue::StrList(vec![String::new(); size])
                } else {
                    CallPropValue::Element(prop_element.clone())
                };

                let frame = self
                    .call_stack
                    .last_mut()
                    .ok_or_else(|| anyhow!("call stack underflow"))?;
                frame.user_props.push(CallProp {
                    scn_no: self.current_scene_no.unwrap_or(0) as i32,
                    prop_id,
                    form: form_code,
                    decl_size: size,
                    element: prop_element,
                    value,
                });
            }
            CD_ARG => {
                // Expand stack arguments into the current call's declared properties
                // (tnm_expand_arg_into_call_flag).
                let (frame_action_proc, actual_arg_cnt, forms): (bool, usize, Vec<i32>) = {
                    let frame = self
                        .call_stack
                        .last()
                        .ok_or_else(|| anyhow!("call stack underflow"))?;
                    (
                        frame.frame_action_proc,
                        frame.arg_cnt,
                        frame.user_props.iter().map(|p| p.form).collect(),
                    )
                };

                if frame_action_proc {
                    if forms.first().copied() != Some(crate::runtime::forms::codes::FM_FRAMEACTION)
                    {
                        bail!("frame_action CD_ARG requires first argument to be FM_FRAMEACTION");
                    }
                    if actual_arg_cnt != forms.len() {
                        bail!(
                            "frame_action CD_ARG argument count mismatch: declared={} actual={}",
                            forms.len(),
                            actual_arg_cnt
                        );
                    }
                }

                // Pop values in reverse order to match the original stack layout.
                let mut values: Vec<CallPropValue> = Vec::with_capacity(forms.len());
                for &form in forms.iter().rev() {
                    let v = if form == self.cfg.fm_int {
                        CallPropValue::Int(self.pop_int()?)
                    } else if form == self.cfg.fm_str {
                        CallPropValue::Str(self.pop_str()?)
                    } else {
                        CallPropValue::Element(self.pop_element()?)
                    };
                    values.push(v);
                }
                values.reverse();

                {
                    let frame = self
                        .call_stack
                        .last_mut()
                        .ok_or_else(|| anyhow!("call stack underflow"))?;
                    for (prop, v) in frame.user_props.iter_mut().zip(values.into_iter()) {
                        match (&v, prop.form) {
                            (CallPropValue::Int(_), f) if f == self.cfg.fm_int => {
                                prop.value = v;
                            }
                            (CallPropValue::Str(_), f) if f == self.cfg.fm_str => {
                                prop.value = v;
                            }
                            (CallPropValue::Element(e), _) => {
                                // C++ tnm_expand_arg_into_call_flag() writes all non-int/str
                                // arguments into user_prop_list[i].element directly.
                                prop.element = e.clone();
                                if matches!(
                                    prop.form,
                                    crate::runtime::forms::codes::FM_INTREF
                                        | crate::runtime::forms::codes::FM_STRREF
                                        | crate::runtime::forms::codes::FM_INTLISTREF
                                        | crate::runtime::forms::codes::FM_STRLISTREF
                                        | crate::runtime::forms::codes::FM_LIST
                                ) {
                                    prop.value = CallPropValue::Element(e.clone());
                                }
                            }
                            _ => {
                                prop.value = v;
                            }
                        }
                    }
                }
                vm_trace!(
                    self,
                    Some(pc_before),
                    format!(
                        "ARG expanded frame={:?}",
                        self.call_stack.last().map(|frame| {
                            format!(
                                "ret_form={} arg_cnt={} props={:?} L0_8={:?} K0_4={:?}",
                                frame.ret_form,
                                frame.arg_cnt,
                                frame.user_props,
                                &frame.int_args[..frame.int_args.len().min(8)],
                                &frame.str_args[..frame.str_args.len().min(4)]
                            )
                        })
                    )
                );
            }

            CD_GOTO => {
                let label_no = self.stream.pop_i32()?;
                sg_omv_trace!(self, "GOTO label={} taken=true", label_no);
                self.stream.jump_to_label(label_no.max(0) as usize)?;
            }
            CD_GOTO_TRUE => {
                let label_no = self.stream.pop_i32()?;
                let trace_cf_branch = self.cf_branch_trace_interesting_line();
                let before_tail = if trace_cf_branch {
                    let start = self.int_stack.len().saturating_sub(16);
                    Some(self.int_stack[start..].to_vec())
                } else {
                    None
                };
                let cond = self.pop_int()?;
                let taken = cond != 0;
                sg_omv_trace!(self, "GOTO_TRUE label={} cond={} taken={}", label_no, cond, taken);
                if let Some(before_tail) = before_tail.as_deref() {
                    self.trace_cf_branch_goto(
                        pc_before,
                        "GOTO_TRUE",
                        label_no,
                        cond,
                        taken,
                        before_tail,
                    );
                }
                if taken {
                    self.stream.jump_to_label(label_no.max(0) as usize)?;
                }
            }
            CD_GOTO_FALSE => {
                let label_no = self.stream.pop_i32()?;
                let trace_cf_branch = self.cf_branch_trace_interesting_line();
                let before_tail = if trace_cf_branch {
                    let start = self.int_stack.len().saturating_sub(16);
                    Some(self.int_stack[start..].to_vec())
                } else {
                    None
                };
                let cond = self.pop_int()?;
                let taken = cond == 0;
                sg_omv_trace!(self, "GOTO_FALSE label={} cond={} taken={}", label_no, cond, taken);
                if let Some(before_tail) = before_tail.as_deref() {
                    self.trace_cf_branch_goto(
                        pc_before,
                        "GOTO_FALSE",
                        label_no,
                        cond,
                        taken,
                        before_tail,
                    );
                }
                if taken {
                    self.stream.jump_to_label(label_no.max(0) as usize)?;
                }
            }
            CD_GOSUB => {
                let label_no = self.stream.pop_i32()?;
                let _args = self.pop_arg_list()?;
                let return_pc = self.stream.get_prg_cntr();
                vm_trace!(self,
                    Some(pc_before),
                    format!("GOSUB label={} return_pc=0x{return_pc:x}", label_no),
                );

                // Save return info on the caller frame .
                let caller = self
                    .call_stack
                    .last_mut()
                    .ok_or_else(|| anyhow!("call stack underflow"))?;
                caller.return_pc = return_pc;
                caller.return_scene_no = self.current_scene_no;
                caller.return_scene_name = self.current_scene_name.clone();
                caller.return_line_no = self.current_line_no;
                caller.ret_form = self.cfg.fm_int;
                self.gosub_return_stack.push((return_pc, self.cfg.fm_int));

                // Enter callee context.
                let scratch_args = self.call_scratch_from_args(&_args);
                let mut callee = self.take_call_frame(
                    self.cfg.fm_void,
                    false,
                    false,
                    _args.len(),
                    Some(scratch_args),
                );
                callee.call_type = 1;
                callee.return_override = Some((return_pc, self.cfg.fm_int));
                self.call_stack.push(callee);

                self.stream.jump_to_label(label_no.max(0) as usize)?;
            }
            CD_GOSUBSTR => {
                let label_no = self.stream.pop_i32()?;
                let _args = self.pop_arg_list()?;
                let return_pc = self.stream.get_prg_cntr();
                vm_trace!(self,
                    Some(pc_before),
                    format!("GOSUBSTR label={} return_pc=0x{return_pc:x}", label_no),
                );

                let caller = self
                    .call_stack
                    .last_mut()
                    .ok_or_else(|| anyhow!("call stack underflow"))?;
                caller.return_pc = return_pc;
                caller.return_scene_no = self.current_scene_no;
                caller.return_scene_name = self.current_scene_name.clone();
                caller.return_line_no = self.current_line_no;
                caller.ret_form = self.cfg.fm_str;
                self.gosub_return_stack.push((return_pc, self.cfg.fm_str));

                let scratch_args = self.call_scratch_from_args(&_args);
                let mut callee = self.take_call_frame(
                    self.cfg.fm_void,
                    false,
                    false,
                    _args.len(),
                    Some(scratch_args),
                );
                callee.call_type = 1;
                callee.return_override = Some((return_pc, self.cfg.fm_str));
                self.call_stack.push(callee);

                self.stream.jump_to_label(label_no.max(0) as usize)?;
            }
            CD_RETURN => {
                let args = self.pop_arg_list()?;
                if sg_ring_on() {
                    // `exec_return` reads the continuation AND the result form from the
                    // frame *under* the callee, so log that one too: if it says fm_void
                    // while the call site expects an int, the result is silently dropped
                    // and the caller's next pop underflows.
                    if let Some(c) = self.call_stack.iter().rev().nth(1) {
                        eprintln!(
                            "[SG_RETURN_CALLER] caller_return_pc=0x{:x} caller_ret_form={} (void={} int={}) caller_call_type={} caller_scene={:?}",
                            c.return_pc,
                            c.ret_form,
                            self.cfg.fm_void,
                            self.cfg.fm_int,
                            c.call_type,
                            c.return_scene_name
                        );
                    }
                    match self.call_stack.last() {
                        Some(f) => eprintln!(
                            "[SG_RETURN] scene={} line={} pc=0x{:x} -> return_pc=0x{:x} return_scene={:?} return_line={} call_type={} depth={} scene_stack={} int_depth={} callee_ret_form={} override={:?} args={:?}",
                            self.current_scene_name.as_deref().unwrap_or("<none>"),
                            self.current_line_no,
                            pc_before,
                            f.return_pc,
                            f.return_scene_name,
                            f.return_line_no,
                            f.call_type,
                            self.call_stack.len(),
                            self.scene_stack.len(),
                            self.int_stack.len(),
                            f.ret_form,
                            f.return_override,
                            args
                        ),
                        None => eprintln!(
                            "[SG_RETURN] scene={} line={} pc=0x{:x} frame=<none> depth=0 scene_stack={} int_depth={}",
                            self.current_scene_name.as_deref().unwrap_or("<none>"),
                            self.current_line_no,
                            pc_before,
                            self.scene_stack.len(),
                            self.int_stack.len()
                        ),
                    }
                }
                if self.vm_trace_matches() {
                    if let Some(frame) = self.call_stack.last() {
                        self.vm_trace_emit(
                            Some(pc_before),
                            format_args!(
                                "RETURN decoded argc={} args={:?} call_depth={} scene_stack={} frame=ret_form={} arg_cnt={} props={:?} L0_8={:?} K0_4={:?}",
                                args.len(),
                                args,
                                self.call_stack.len(),
                                self.scene_stack.len(),
                                frame.ret_form,
                                frame.arg_cnt,
                                frame.user_props,
                                &frame.int_args[..frame.int_args.len().min(8)],
                                &frame.str_args[..frame.str_args.len().min(4)],
                            ),
                        );
                    } else {
                        self.vm_trace_emit(
                            Some(pc_before),
                            format_args!(
                                "RETURN decoded argc={} args={:?} call_depth={} scene_stack={} frame=<none>",
                                args.len(),
                                args,
                                self.call_stack.len(),
                                self.scene_stack.len(),
                            ),
                        );
                    }
                }
                sg_omv_trace!(self, "RETURN argc={} args={:?} call_depth={} scene_stack={}", args.len(), args, self.call_stack.len(), self.scene_stack.len());
                if self.at_cross_scene_return_boundary() {
                    if self.return_from_scene(args)? {
                        return Ok(true);
                    }
                    log::warn!(
                        "[SG-DIAG-8] CD_RETURN boundary no-return halt: scene={:?} scene_no={:?} line={} pc=0x{:x} call_depth={} scene_stack={}",
                        self.current_scene_name,
                        self.current_scene_no,
                        self.current_line_no,
                        pc_before,
                        self.call_stack.len(),
                        self.scene_stack.len()
                    );
                    self.halted = true;
                    return Ok(false);
                }
                if self.call_stack.len() == 1 {
                    // Only the scene base frame is left, so this RETURN has no caller
                    // to unwind to. The original format serializes the whole
                    // cross-scene call list, so a well-formed save always has one;
                    // this state means the active save carried no caller frames at
                    // all (an active-only legacy save). The orphaned continuation
                    // cannot be reconstructed from that save, so end the scene flow
                    // here instead of guessing a return address. Clearing
                    // `legacy_saved_active_only` hands the wind-down to the host,
                    // which returns to the title rather than leaving a halted VM on
                    // screen forever.
                    log::warn!(
                        "[SG_VM] cross-scene RETURN with no caller frame: scene={:?} scene_no={:?} line={} pc=0x{:x} legacy_active_only={} -> scene flow ends",
                        self.current_scene_name,
                        self.current_scene_no,
                        self.current_line_no,
                        pc_before,
                        self.legacy_saved_active_only
                    );
                    self.legacy_saved_active_only = false;
                    self.halted = true;
                    return Ok(false);
                }
                if self.exec_return(args)? {
                    return Ok(false);
                }
            }

            CD_ASSIGN => {
                let left_form = self.stream.pop_i32()?;
                let right_form = self.stream.pop_i32()?;
                let al_id = self.stream.pop_i32()?;
                let rhs = self.pop_value_for_form(right_form)?;
                let elm = self.pop_element()?;
                vm_trace!(self,
                    Some(pc_before),
                    format!(
                        "ASSIGN decoded left_form={} right_form={} al_id={} elm={:?} rhs={:?}",
                        left_form, right_form, al_id, elm, rhs
                    ),
                );
                self.exec_assign(elm, al_id, rhs)?;
                if self.vm_trace_matches() {
                    if let Some(frame) = self.call_stack.last() {
                        self.vm_trace_emit(
                            Some(pc_before),
                            format_args!(
                                "ASSIGN applied frame=ret_form={} arg_cnt={} props={:?} L0_8={:?} K0_4={:?}",
                                frame.ret_form,
                                frame.arg_cnt,
                                frame.user_props,
                                &frame.int_args[..frame.int_args.len().min(8)],
                                &frame.str_args[..frame.str_args.len().min(4)],
                            ),
                        );
                    } else {
                        self.vm_trace_emit(Some(pc_before), "ASSIGN applied frame=<none>");
                    }
                }
            }

            CD_OPERATE_1 => {
                let form_code = self.stream.pop_i32()?;
                let opr = self.stream.pop_u8()?;
                self.exec_operate_1(form_code, opr)?;
            }
            CD_OPERATE_2 => {
                let form_l = self.stream.pop_i32()?;
                let form_r = self.stream.pop_i32()?;
                let opr = self.stream.pop_u8()?;
                self.exec_operate_2(form_l, form_r, opr)?;
            }
            CD_COMMAND => {
                // CD_COMMAND reads: arg_list_id, arg_list, element, named_arg_cnt, named_arg_ids..., ret_form
                let arg_list_id = self.stream.pop_i32()?;
                let mut args = self.pop_arg_list()?;
                let elm = self.pop_element()?;

                let named_arg_cnt = self.stream.pop_i32()?;
                if named_arg_cnt < 0 {
                    bail!("negative named_arg_cnt={named_arg_cnt}");
                }

                let mut named_ids: Vec<i32> = Vec::with_capacity(named_arg_cnt as usize);
                for _ in 0..(named_arg_cnt as usize) {
                    named_ids.push(self.stream.pop_i32()?);
                }

                if !named_ids.is_empty() {
                    let n = named_ids.len().min(args.len());
                    for a in 0..n {
                        let idx = args.len() - 1 - a;
                        let id = named_ids[a];
                        let v = std::mem::replace(&mut args[idx], crate::runtime::Value::Int(0));
                        args[idx] = crate::runtime::Value::NamedArg {
                            id,
                            value: Box::new(v),
                        };
                    }
                }

                let ret_form = self.stream.pop_i32()?;
                if let Some(raw_head) = elm.first().copied() {
                    let form_id = self.canonical_runtime_form_id(raw_head as u32) as i32;
                    let op_id = if elm.len() >= 2 { elm[1] } else { arg_list_id };
                    self.sg_omv_trace_command("CD_COMMAND", &elm, form_id, op_id, arg_list_id, ret_form, &args);
                }
                let _ = self.ctx.take_read_flag_no_request();
                let block_generation = self.ctx.wait.block_generation();
                let proc_generation = self.ctx.proc_generation();
                self.exec_command(elm, arg_list_id, ret_form, &mut args)?;
                self.drain_runtime_save_load_requests()?;
                if self.ctx.take_read_flag_no_request() {
                    let read_flag_no = self.stream.pop_i32()?;
                    self.ctx.submit_read_flag_no(read_flag_no);
                }
                if self.ctx.proc_generation() != proc_generation {
                    return Ok(true);
                }
                if respect_wait
                    && self.ctx.wait.block_generation() != block_generation
                    && self.ctx.wait_poll()
                {
                    return Ok(true);
                }
            }
            CD_TEXT => {
                let rf_flag_no = self.stream.pop_i32()?;
                let text = self.pop_str()?;
                if !crate::runtime::forms::stage::cd_text_current_mwnd(
                    &mut self.ctx,
                    &text,
                    rf_flag_no as i64,
                ) {
                    self.ctx.ui.set_message(text);
                }
            }
            CD_NAME => {
                let name = self.pop_str()?;
                if !crate::runtime::forms::stage::cd_name_current_mwnd(&mut self.ctx, &name) {
                    self.ctx.ui.set_name(name);
                }
            }
            CD_SEL_BLOCK_START => {
                // Selection blocks are handled by higher-level UI commands.
                // Keep a marker to avoid breaking control flow.
            }
            CD_SEL_BLOCK_END => {
                // The original VM leaves a result on the int stack for certain selection constructs.
                // Default to 0 (first choice) if scripts expect a value.
                self.push_int(0);
            }

            CD_EOF => {
                log::warn!(
                    "[SG-DIAG-4] CD_EOF at scene={:?} scene_no={:?} line={} pc=0x{:x} boundary={}",
                    self.current_scene_name,
                    self.current_scene_no,
                    self.current_line_no,
                    pc_before,
                    self.at_cross_scene_return_boundary()
                );
                if self.at_cross_scene_return_boundary()
                    && self.return_from_scene(Vec::new())?
                {
                    return Ok(true);
                }
                self.halted = true;
                return Ok(false);
            }

            CD_NONE => {
                // In the original engine this is treated as a fatal script error.
                // Stop execution and record it.
                *self.unknown_opcodes.entry(opcode).or_insert(0) += 1;
                let scn_cmd_context = self.vm_scn_cmd_context(pc_before);
                let mut b = String::new();
                for &byte in &self.stream.scn[pc_before.saturating_sub(8)..self.stream.scn.len().min(pc_before + 16)] {
                    use std::fmt::Write;
                    let _ = write!(b, "{byte:02x} ");
                }
                log::warn!(
                    "[SG-DIAG-5] CD_NONE (fatal) scene={:?} scene_no={:?} line={} pc=0x{:x} ctx={} bytes={}",
                    self.current_scene_name,
                    self.current_scene_no,
                    self.current_line_no,
                    pc_before,
                    scn_cmd_context,
                    b
                );
                self.halted = true;
                return Ok(false);
            }

            other => {
                *self.unknown_opcodes.entry(other).or_insert(0) += 1;
                log::warn!(
                    "[SG-DIAG-10] unknown opcode=0x{other:02x} at pc=0x{:x}; scene={:?} scene_no={:?} line={}",
                    pc_before,
                    self.current_scene_name,
                    self.current_scene_no,
                    self.current_line_no
                );
                println!(
                    "VM unknown opcode=0x{other:02x} at pc=0x{:x}; stopping",
                    pc_before
                );
                self.halted = true;
                return Ok(false);
            }
        }

        Ok(true)
    }

    pub fn run(&mut self) -> Result<()> {
        while self.step()? {}
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Stack helpers
    // ---------------------------------------------------------------------

    fn push_int(&mut self, v: i32) {
        self.int_stack.push(v);
        vm_trace!(self, None, format!("push_int {}", v));
    }

    fn pop_int(&mut self) -> Result<i32> {
        match self.int_stack.pop() {
            Some(v) => {
                vm_trace!(self, None, format!("pop_int -> {}", v));
                Ok(v)
            }
            None => {
                vm_trace!(self, None, "pop_int underflow");
                // Report the site as precisely as possible. `pc` alone lands in
                // the middle of a long compiled `if/else if` chain and cannot be
                // mapped back to a source line, so include the element the VM was
                // evaluating (when the pop happens inside a property/command
                // dispatch) and the remaining stack depth.
                let call = self
                    .ctx
                    .vm_call
                    .as_ref()
                    .map(|m| {
                        format!(
                            "element={:?} al_id={:?} ret_form={}",
                            m.element, m.al_id, m.ret_form
                        )
                    })
                    .unwrap_or_else(|| "element=<none>".to_string());
                let pc = self.stream.get_prg_cntr();
                let (win_start, win) = self.stream.debug_bytes_around(pc, 24, 24);
                let ring = self.sg_ring_dump();
                Err(anyhow!(
                    "int stack underflow: scene={} scene_no={} line={} pc=0x{:x} depth={} {} bytes@0x{:x}={:02x?}{}",
                    self.current_scene_name.as_deref().unwrap_or("<none>"),
                    self.current_scene_no
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    self.current_line_no,
                    pc,
                    self.int_stack.len(),
                    call,
                    win_start,
                    win,
                    ring
                ))
            }
        }
    }

    fn peek_int(&self) -> Result<i32> {
        self.int_stack
            .last()
            .copied()
            .ok_or_else(|| anyhow!("int stack underflow"))
    }

    fn push_str(&mut self, s: String) {
        self.str_stack.push(s);
        vm_trace!(self, None, {
            let value = self.str_stack.last().expect("string was just pushed");
            let preview = if value.chars().count() > 48 {
                let mut preview = value.chars().take(48).collect::<String>();
                preview.push('…');
                preview
            } else {
                value.clone()
            };
            format!("push_str {:?}", preview)
        });
    }

    fn pop_str(&mut self) -> Result<String> {
        match self.str_stack.pop() {
            Some(v) => {
                vm_trace!(
                    self,
                    None,
                    format!(
                        "pop_str -> {:?}",
                        if v.chars().count() > 48 {
                            let mut preview = v.chars().take(48).collect::<String>();
                            preview.push('…');
                            preview
                        } else {
                            v.clone()
                        }
                    )
                );
                Ok(v)
            }
            None => {
                vm_trace!(self, None, "pop_str underflow");
                Err(anyhow!(
                    "str stack underflow: scene={} scene_no={} line={} pc=0x{:x}",
                    self.current_scene_name.as_deref().unwrap_or("<none>"),
                    self.current_scene_no
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    self.current_line_no,
                    self.stream.get_prg_cntr()
                ))
            }
        }
    }

    fn peek_str(&self) -> Result<String> {
        self.str_stack
            .last()
            .cloned()
            .ok_or_else(|| anyhow!("str stack underflow"))
    }

    fn push_element(&mut self, elm: Vec<i32>) {
        self.element_points.push(self.int_stack.len());
        self.int_stack.extend_from_slice(&elm);
        vm_trace!(self, None, format!("push_element {:?}", elm));
    }

    fn pop_element(&mut self) -> Result<Vec<i32>> {
        let start = match self.element_points.pop() {
            Some(v) => v,
            None => {
                vm_trace!(self, None, "pop_element underflow (missing ELM_POINT)");
                return Err(anyhow!("element stack underflow (missing ELM_POINT)"));
            }
        };
        if start > self.int_stack.len() {
            vm_trace!(self,
                None,
                format!(
                    "pop_element invalid start={} len={}",
                    start,
                    self.int_stack.len()
                ),
            );
            bail!(
                "invalid element point start={start} len={}",
                self.int_stack.len()
            );
        }
        let elm = self.int_stack[start..].to_vec();
        self.int_stack.truncate(start);
        vm_trace!(self, None, format!("pop_element -> {:?}", elm));
        Ok(elm)
    }

    fn extract_array_index(&self, elm: &[i32]) -> Option<usize> {
        if elm.len() >= 3 && elm[1] == self.ctx.ids.elm_array {
            let idx = elm[2];
            if idx >= 0 {
                return Some(idx as usize);
            }
        }
        None
    }

    fn user_prop_decl(&self, prop_id: u16) -> Option<(i32, usize)> {
        let prop_idx = prop_id as usize;
        if let Some(pck) = self.scene_pck_cache.as_ref() {
            if prop_idx < pck.inc_props.len() {
                let decl = &pck.inc_props[prop_idx];
                return Some((decl.form, decl.size.max(0) as usize));
            }
        } else if prop_idx < self.stream.header.scn_prop_cnt.max(0) as usize {
            // Some VM/unit-test entry points construct a SceneVm directly from
            // one scene chunk and do not eagerly load Scene.pck metadata.  In
            // that mode the bytecode still numbers the shared/inc properties
            // from zero, and this scene chunk carries the matching declarations
            // in its own prop table.  Treat them as authoritative until the
            // pack cache is available; otherwise scalar input/menu variables
            // are mis-created as generic lists and title clicks never latch.
            let off = (self.stream.header.scn_prop_list_ofs.max(0) as usize)
                .checked_add(prop_idx * 8)?;
            if off + 8 <= self.stream.chunk.len() {
                let form = i32::from_le_bytes(self.stream.chunk[off..off + 4].try_into().unwrap());
                let size =
                    i32::from_le_bytes(self.stream.chunk[off + 4..off + 8].try_into().unwrap());
                return Some((form, size.max(0) as usize));
            }
        }

        let local_idx = prop_idx.saturating_sub(
            self.scene_pck_cache
                .as_ref()
                .map(|pck| pck.inc_props.len())
                .unwrap_or(0),
        );
        let list_ofs = self.stream.header.scn_prop_list_ofs.max(0) as usize;
        let cnt = self.stream.header.scn_prop_cnt.max(0) as usize;
        if local_idx < cnt {
            let off = list_ofs.checked_add(local_idx * 8)?;
            if off + 8 <= self.stream.chunk.len() {
                let form = i32::from_le_bytes(self.stream.chunk[off..off + 4].try_into().unwrap());
                let size =
                    i32::from_le_bytes(self.stream.chunk[off + 4..off + 8].try_into().unwrap());
                return Some((form, size.max(0) as usize));
            }
        }
        None
    }

    fn default_user_prop_element(&self, prop_id: u16, _form: i32) -> Vec<i32> {
        let head = constants::elm::create(constants::elm::OWNER_USER_PROP, 0, prop_id as i32);
        vec![head]
    }

    fn default_user_prop_slot_element(&self, prop_id: u16, idx: usize) -> Vec<i32> {
        vec![
            constants::elm::create(constants::elm::OWNER_USER_PROP, 0, prop_id as i32),
            self.ctx.ids.elm_array,
            idx as i32,
        ]
    }

    fn default_user_prop_cell(&self, prop_id: u16) -> UserPropCell {
        let (form, size) = self
            .user_prop_decl(prop_id)
            .unwrap_or((self.cfg.fm_list, 0));
        let mut cell = UserPropCell::new(form, self.default_user_prop_element(prop_id, form));
        if form == self.cfg.fm_intlist {
            cell.int_list = vec![0; size];
        } else if form == self.cfg.fm_strlist {
            cell.str_list = vec![String::new(); size];
        } else if form == self.cfg.fm_list && size > 0 {
            let mut items = Vec::with_capacity(size);
            for i in 0..size {
                let mut slot = UserPropCell::new(
                    self.cfg.fm_list,
                    self.default_user_prop_slot_element(prop_id, i),
                );
                slot.form = self.cfg.fm_list;
                items.push(slot);
            }
            cell.list_items = items;
        }
        cell
    }

    fn user_prop_cell_from_value(
        &self,
        rhs: Value,
        declared_form: i32,
        element: Vec<i32>,
        prop_id: Option<u16>,
    ) -> UserPropCell {
        let mut cell = UserPropCell::new(declared_form, element.clone());
        match rhs {
            Value::NamedArg { value, .. } => {
                return self.user_prop_cell_from_value(*value, declared_form, element, prop_id);
            }
            Value::Int(n) => {
                cell.form = self.cfg.fm_int;
                cell.int_value = n as i32;
            }
            Value::Str(s) => {
                cell.form = self.cfg.fm_str;
                cell.str_value = s;
            }
            Value::Element(e) => {
                cell.form = declared_form;
                cell.element = e;
            }
            Value::List(items) => {
                if declared_form == self.cfg.fm_intlist {
                    cell.form = self.cfg.fm_intlist;
                    cell.int_list = items
                        .into_iter()
                        .map(|item| item.as_i64().unwrap_or(0) as i32)
                        .collect();
                } else if declared_form == self.cfg.fm_strlist {
                    cell.form = self.cfg.fm_strlist;
                    cell.str_list = items
                        .into_iter()
                        .map(|item| item.as_str().unwrap_or("").to_string())
                        .collect();
                } else {
                    cell.form = self.cfg.fm_list;
                    let mut out = Vec::with_capacity(items.len());
                    for (idx, item) in items.into_iter().enumerate() {
                        let slot_element = if let Some(pid) = prop_id {
                            self.default_user_prop_slot_element(pid, idx)
                        } else {
                            vec![]
                        };
                        out.push(self.user_prop_cell_from_value(
                            item,
                            self.cfg.fm_list,
                            slot_element,
                            prop_id,
                        ));
                    }
                    cell.list_items = out;
                }
            }
        }
        cell
    }

    fn consume_array_sub_signed<'b>(&self, sub: &'b [i32]) -> Option<(i32, &'b [i32])> {
        if sub.len() >= 2 && self.call_array_marker(sub[0]) {
            Some((sub[1], &sub[2..]))
        } else {
            None
        }
    }

    fn consume_array_sub<'b>(&self, sub: &'b [i32]) -> Option<(usize, &'b [i32])> {
        let (index, rest) = self.consume_array_sub_signed(sub)?;
        usize::try_from(index).ok().map(|index| (index, rest))
    }

    fn intlist_bit_get(values: &[i32], bit: i32, index: usize) -> i32 {
        let word = values
            .get(index / (32 / bit as usize))
            .copied()
            .unwrap_or(0) as u32;
        let shift = (index % (32 / bit as usize)) * bit as usize;
        let mask = ((1u32 << bit) - 1) << shift;
        ((word & mask) >> shift) as i32
    }

    fn intlist_dispatch_read(
        &mut self,
        values: &[i32],
        sub: &[i32],
        fallback_element: &[i32],
    ) -> Result<()> {
        use crate::runtime::forms::codes::{
            ELM_ARRAY, ELM_INTLIST_BIT, ELM_INTLIST_BIT16, ELM_INTLIST_BIT2, ELM_INTLIST_BIT4,
            ELM_INTLIST_BIT8, ELM_INTLIST_GET_SIZE,
        };
        let sub = if sub.len() == 1 && self.call_array_marker(sub[0]) {
            &[][..]
        } else {
            sub
        };
        if sub.is_empty() {
            self.push_element(fallback_element.to_vec());
            return Ok(());
        }
        if let Some((idx, rest)) = self.consume_array_sub_signed(sub) {
            if !rest.is_empty() {
                bail!("unsupported chained intlist array access {:?}", sub);
            }
            let value = usize::try_from(idx)
                .ok()
                .and_then(|idx| values.get(idx).copied())
                .unwrap_or(0);
            self.push_int(value);
            return Ok(());
        }
        match sub[0] {
            ELM_INTLIST_GET_SIZE => {
                self.push_int(values.len() as i32);
            }
            ELM_INTLIST_BIT | ELM_INTLIST_BIT2 | ELM_INTLIST_BIT4 | ELM_INTLIST_BIT8
            | ELM_INTLIST_BIT16 => {
                let bit = match sub[0] {
                    ELM_INTLIST_BIT => 1,
                    ELM_INTLIST_BIT2 => 2,
                    ELM_INTLIST_BIT4 => 4,
                    ELM_INTLIST_BIT8 => 8,
                    _ => 16,
                };
                if let Some((idx, rest)) = self.consume_array_sub_signed(&sub[1..]) {
                    if !rest.is_empty() {
                        bail!("unsupported chained intlist bit access {:?}", sub);
                    }
                    let value = usize::try_from(idx)
                        .ok()
                        .map(|idx| Self::intlist_bit_get(values, bit, idx))
                        .unwrap_or(0);
                    self.push_int(value);
                } else {
                    let mut chained = fallback_element.to_vec();
                    chained.extend_from_slice(sub);
                    self.push_element(chained);
                }
            }
            _ => self.push_element(fallback_element.to_vec()),
        }
        Ok(())
    }

    fn push_user_prop_cell_result(
        &mut self,
        cell: &UserPropCell,
        sub: &[i32],
        full_elm: &[i32],
    ) -> Result<()> {
        use crate::runtime::forms::codes::{
            ELM_STRLIST_GET_SIZE, FM_INTLISTREF, FM_INTREF, FM_STRLISTREF, FM_STRREF,
        };

        let sub = if sub.len() == 1 && self.call_array_marker(sub[0]) {
            &[][..]
        } else {
            sub
        };
        if cell.form == self.cfg.fm_int && sub.is_empty() {
            self.push_int(cell.int_value);
            return Ok(());
        }
        if cell.form == self.cfg.fm_str {
            if sub.is_empty() {
                self.push_str(cell.str_value.clone());
            } else {
                self.call_prop_eval_str_op(&cell.str_value, sub[0], &[], 0)?;
            }
            return Ok(());
        }
        if cell.form == self.cfg.fm_intlist {
            return self.intlist_dispatch_read(&cell.int_list, sub, &cell.element);
        }
        if cell.form == self.cfg.fm_strlist {
            if sub.is_empty() {
                self.push_element(cell.element.clone());
            } else if let Some((idx, rest)) = self.consume_array_sub_signed(sub) {
                let cur = usize::try_from(idx)
                    .ok()
                    .and_then(|idx| cell.str_list.get(idx).cloned())
                    .unwrap_or_default();
                if rest.is_empty() {
                    self.push_str(cur);
                } else {
                    self.call_prop_eval_str_op(&cur, rest[0], &[], 0)?;
                }
            } else if sub[0] == ELM_STRLIST_GET_SIZE {
                self.push_int(cell.str_list.len() as i32);
            } else {
                self.push_element(cell.element.clone());
            }
            return Ok(());
        }
        if matches!(
            cell.form,
            FM_INTREF | FM_STRREF | FM_INTLISTREF | FM_STRLISTREF
        ) {
            self.push_element(cell.element.clone());
            return Ok(());
        }
        if let Some((idx, rest)) = self.consume_array_sub(sub) {
            let slot = if let Some(slot) = cell.list_items.get(idx) {
                slot.clone()
            } else {
                let mut tmp = UserPropCell::new(
                    self.cfg.fm_list,
                    self.default_user_prop_slot_element(elm_code::code(full_elm[0]), idx),
                );
                tmp.form = self.cfg.fm_list;
                tmp
            };
            return self.push_user_prop_cell_result(&slot, rest, full_elm);
        }
        self.push_element(cell.element.clone());
        Ok(())
    }

    fn default_value_like(&self, v: &Value) -> Value {
        match v {
            Value::NamedArg { value, .. } => self.default_value_like(value),
            Value::Int(_) => Value::Int(0),
            Value::Str(_) => Value::Str(String::new()),
            Value::Element(_) => Value::Element(Vec::new()),
            Value::List(_) => Value::List(Vec::new()),
        }
    }
    fn call_scratch_from_args(&self, args: &[Value]) -> (Vec<i32>, Vec<String>) {
        let mut int_args = Self::blank_call_int_args(self.call_flag_count);
        let mut str_args = Self::blank_call_str_args(self.call_flag_count);
        let mut int_pos = 0usize;
        let mut str_pos = 0usize;
        for v in args {
            match v {
                Value::NamedArg { value, .. } => match value.as_ref() {
                    Value::Int(n) => {
                        if int_pos < int_args.len() {
                            int_args[int_pos] = *n as i32;
                            int_pos += 1;
                        }
                    }
                    Value::Str(s) => {
                        if str_pos < str_args.len() {
                            str_args[str_pos] = s.clone();
                            str_pos += 1;
                        }
                    }
                    _ => {}
                },
                Value::Int(n) => {
                    if int_pos < int_args.len() {
                        int_args[int_pos] = *n as i32;
                        int_pos += 1;
                    }
                }
                Value::Str(s) => {
                    if str_pos < str_args.len() {
                        str_args[str_pos] = s.clone();
                        str_pos += 1;
                    }
                }
                _ => {}
            }
        }
        (int_args, str_args)
    }

    fn call_array_marker(&self, code: i32) -> bool {
        let mapped = self.ctx.ids.elm_array;
        code == crate::runtime::forms::codes::ELM_ARRAY || (mapped >= 0 && code == mapped)
    }

    fn resolve_call_frame_index(&self, idx: i32) -> Option<usize> {
        if idx < 0 {
            return None;
        }
        let depth = self.call_stack.len();
        let rev = idx as usize;
        if rev >= depth {
            return None;
        }
        Some(depth - 1 - rev)
    }

    fn current_call_frame_index(&self) -> Option<usize> {
        if self.call_stack.is_empty() {
            None
        } else {
            Some(self.call_stack.len() - 1)
        }
    }

    fn find_call_prop_index_in_frame(&self, frame_idx: usize, call_prop_id: i32) -> Option<usize> {
        // C++ tnm_command_proc_call_prop() indexes the current call's
        // user_prop_list directly with the CALL_PROP code value:
        //   p_cur_call->user_prop_list[call_prop_id]
        // The stored C_elm_user_call_prop::prop_id is the declared property id
        // and is not the lookup key for CALL_PROP bytecode.
        if call_prop_id < 0 {
            return None;
        }
        let frame = self.call_stack.get(frame_idx)?;
        let idx = call_prop_id as usize;
        if idx < frame.user_props.len() {
            Some(idx)
        } else {
            None
        }
    }

    fn call_prop_element(prop_id: i32) -> Vec<i32> {
        vec![constants::elm::create(constants::elm::OWNER_CALL_PROP, 0, prop_id)]
    }

    fn call_prop_value_from_rhs(&self, rhs: &Value) -> (i32, CallPropValue) {
        match rhs {
            Value::NamedArg { value, .. } => self.call_prop_value_from_rhs(value),
            Value::Int(n) => (self.cfg.fm_int, CallPropValue::Int(*n as i32)),
            Value::Str(s) => (self.cfg.fm_str, CallPropValue::Str(s.clone())),
            Value::Element(e) => (self.cfg.fm_list, CallPropValue::Element(e.clone())),
            Value::List(_) => (self.cfg.fm_list, CallPropValue::Element(Vec::new())),
        }
    }

    fn ensure_call_prop_index_for_assign(
        &mut self,
        frame_idx: usize,
        call_prop_id: i32,
        rhs: &Value,
    ) -> Result<usize> {
        if let Some(idx) = self.find_call_prop_index_in_frame(frame_idx, call_prop_id) {
            return Ok(idx);
        }
        if call_prop_id < 0 {
            bail!("negative CALL_PROP id {}", call_prop_id);
        }

        // The original engine expects CALL_PROP ids to be dense list indexes
        // created by CD_DEC_PROP. If Rust reaches an assignment before a slot
        // exists, keep the same indexed layout rather than appending a slot with
        // a matching prop_id, because later CALL_PROP[0] must address slot 0.
        let (form, value) = self.call_prop_value_from_rhs(rhs);
        let target_idx = call_prop_id as usize;
        let frame = self
            .call_stack
            .get_mut(frame_idx)
            .ok_or_else(|| anyhow!("call stack frame out of range"))?;
        while frame.user_props.len() <= target_idx {
            let idx = frame.user_props.len() as i32;
            frame.user_props.push(CallProp {
                scn_no: self.current_scene_no.unwrap_or(0) as i32,
                prop_id: idx,
                form: self.cfg.fm_list,
                decl_size: 0,
                element: Self::call_prop_element(idx),
                value: CallPropValue::Element(Self::call_prop_element(idx)),
            });
        }
        let prop = frame
            .user_props
            .get_mut(target_idx)
            .ok_or_else(|| anyhow!("CALL_PROP slot allocation failed"))?;
        prop.form = form;
        prop.element = Self::call_prop_element(call_prop_id);
        prop.value = value;
        Ok(target_idx)
    }

    fn is_direct_value_form(&self, form: i32) -> bool {
        form == self.cfg.fm_int
            || form == self.cfg.fm_str
            || form == self.cfg.fm_intlist
            || form == self.cfg.fm_strlist
    }

    fn call_prop_effective_element(&self, prop: &CallProp) -> Vec<i32> {
        match &prop.value {
            CallPropValue::Element(e) if !e.is_empty() => e.clone(),
            _ => prop.element.clone(),
        }
    }

    fn compose_call_prop_tail(&self, prop: &CallProp, sub: &[i32]) -> Option<Vec<i32>> {
        if sub.is_empty() || self.is_direct_value_form(prop.form) {
            return None;
        }
        let mut element = self.call_prop_effective_element(prop);
        if element.is_empty() {
            return None;
        }
        element.extend_from_slice(sub);
        Some(element)
    }

    fn compose_user_prop_tail(
        &self,
        prop_id: u16,
        cell: &UserPropCell,
        sub: &[i32],
    ) -> Option<Vec<i32>> {
        if sub.is_empty() || self.is_direct_value_form(cell.form) {
            return None;
        }
        if let Some((idx, rest)) = self.consume_array_sub(sub) {
            let slot = cell.list_items.get(idx)?;
            let default_slot = self.default_user_prop_slot_element(prop_id, idx);
            if slot.element.is_empty() || slot.element == default_slot {
                return None;
            }
            if rest.is_empty() || self.is_direct_value_form(slot.form) {
                return Some(slot.element.clone());
            }
            let mut element = slot.element.clone();
            element.extend_from_slice(rest);
            return Some(element);
        }

        let default_root = self.default_user_prop_element(prop_id, cell.form);
        if cell.element.is_empty() || cell.element == default_root {
            return None;
        }
        let mut element = cell.element.clone();
        element.extend_from_slice(sub);
        Some(element)
    }

    fn push_call_prop_result(
        &mut self,
        prop: &CallProp,
        sub: &[i32],
        full_elm: &[i32],
    ) -> Result<()> {
        use crate::runtime::forms::codes::{
            ELM_STRLIST_GET_SIZE, FM_INT, FM_INTLIST, FM_INTLISTREF, FM_INTREF, FM_STR, FM_STRLIST,
            FM_STRLISTREF, FM_STRREF,
        };

        let sub = if sub.len() == 1 && self.call_array_marker(sub[0]) {
            &[][..]
        } else {
            sub
        };
        match prop.form {
            FM_INT if sub.is_empty() => {
                if let CallPropValue::Int(n) = &prop.value {
                    self.push_int(*n);
                } else {
                    bail!("CALL_PROP int storage mismatch for {:?}", full_elm);
                }
            }
            FM_STR if sub.is_empty() => {
                if let CallPropValue::Str(s) = &prop.value {
                    self.push_str(s.clone());
                } else {
                    bail!("CALL_PROP str storage mismatch for {:?}", full_elm);
                }
            }
            FM_STR => {
                if let CallPropValue::Str(s) = &prop.value {
                    self.call_prop_eval_str_op(s, sub[0], &[], 0)?;
                } else {
                    bail!("CALL_PROP str storage mismatch for {:?}", full_elm);
                }
            }
            FM_INTLIST => {
                if let CallPropValue::IntList(v) = &prop.value {
                    self.intlist_dispatch_read(v, sub, &prop.element)?;
                } else {
                    bail!("CALL_PROP intlist storage mismatch for {:?}", full_elm);
                }
            }
            FM_STRLIST => {
                if let CallPropValue::StrList(v) = &prop.value {
                    if sub.is_empty() {
                        self.push_element(prop.element.clone());
                    } else if let Some((idx, rest)) = self.consume_array_sub_signed(sub) {
                        let current = usize::try_from(idx)
                            .ok()
                            .and_then(|idx| v.get(idx).cloned())
                            .unwrap_or_default();
                        if rest.is_empty() {
                            self.push_str(current);
                        } else {
                            self.call_prop_eval_str_op(&current, rest[0], &[], 0)?;
                        }
                    } else if sub[0] == ELM_STRLIST_GET_SIZE {
                        self.push_int(v.len() as i32);
                    } else {
                        self.push_element(prop.element.clone());
                    }
                } else {
                    bail!("CALL_PROP strlist storage mismatch for {:?}", full_elm);
                }
            }
            FM_INTREF | FM_STRREF if sub.is_empty() => {
                // C++ tnm_command_proc_prop() does not read the scalar here.
                // For every *_REF form it pushes the referenced element back to
                // the element stack. SiglusCompiler emits another CD_PROPERTY
                // when the expression actually needs a value, so resolving the
                // target recursively at this first step consumes one
                // dereference too early and corrupts the following stack shape.
                let target = self.call_prop_effective_element(prop);
                let default_target = Self::call_prop_element(prop.prop_id);
                if target.is_empty()
                    || target == full_elm
                    || target == default_target
                {
                    bail!(
                        "unbound scalar CALL_PROP reference form={} target={:?} access={:?}",
                        prop.form,
                        target,
                        full_elm
                    );
                }
                self.push_element(target);
            }
            FM_INTLISTREF | FM_STRLISTREF => {
                // Lists remain element-valued after dereference; their ARRAY and
                // list-command suffixes are dispatched through the target chain.
                self.push_element(self.call_prop_effective_element(prop));
            }
            FM_INTREF | FM_STRREF => {
                // A non-empty suffix should normally have been composed by
                // compose_call_prop_tail(). Keep a precise failure here rather
                // than silently returning the reference itself as a scalar.
                bail!(
                    "unsupported scalar CALL_PROP reference suffix form={} sub={:?} access={:?}",
                    prop.form,
                    sub,
                    full_elm
                );
            }
            _ if !sub.is_empty() => {
                self.push_element(prop.element.clone());
            }
            _ => {
                self.push_element(prop.element.clone());
            }
        }
        Ok(())
    }

    fn call_prop_eval_str_op(
        &mut self,
        current: &str,
        op: i32,
        params: &[Value],
        al_id: i32,
    ) -> Result<()> {
        use crate::runtime::forms::codes::str_op;
        match op {
            str_op::UPPER => self.push_str(crate::runtime::string_semantics::ascii_upper(current)),
            str_op::LOWER => self.push_str(crate::runtime::string_semantics::ascii_lower(current)),
            str_op::CNT => self.push_int(crate::runtime::string_semantics::utf16_len(current) as i32),
            str_op::LEN => self.push_int(crate::runtime::string_semantics::display_width(current) as i32),
            str_op::LEFT => {
                let len = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                self.push_str(crate::runtime::string_semantics::utf16_left(current, len));
            }
            str_op::LEFT_LEN => {
                let len = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                self.push_str(crate::runtime::string_semantics::left_by_display_width(current, len));
            }
            str_op::RIGHT => {
                let len = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                self.push_str(crate::runtime::string_semantics::utf16_right(current, len));
            }
            str_op::RIGHT_LEN => {
                let len = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                self.push_str(crate::runtime::string_semantics::right_by_display_width(current, len));
            }
            str_op::MID => {
                let start = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                if al_id == 0 || params.len() <= 1 {
                    self.push_str(crate::runtime::string_semantics::utf16_slice(current, start, None));
                } else {
                    let len = params.get(1).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                    self.push_str(crate::runtime::string_semantics::utf16_slice(current, start, Some(len)));
                }
            }
            str_op::MID_LEN => {
                let start = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                let len = if al_id == 0 || params.len() <= 1 {
                    None
                } else {
                    Some(params.get(1).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize)
                };
                self.push_str(crate::runtime::string_semantics::mid_by_display_width(current, start, len));
            }
            str_op::SEARCH => {
                let needle = params.first().and_then(|v| v.as_str()).unwrap_or("");
                self.push_int(
                    crate::runtime::string_semantics::search_ascii_case_insensitive(current, needle)
                        .map(|v| v as i32)
                        .unwrap_or(-1),
                );
            }
            str_op::SEARCH_LAST => {
                let needle = params.first().and_then(|v| v.as_str()).unwrap_or("");
                self.push_int(
                    crate::runtime::string_semantics::rsearch_ascii_case_insensitive(current, needle)
                        .map(|v| v as i32)
                        .unwrap_or(-1),
                );
            }
            str_op::GET_CODE => {
                let pos = params.first().and_then(|v| v.as_i64()).unwrap_or(0);
                self.push_int(
                    usize::try_from(pos)
                        .ok()
                        .and_then(|pos| {
                            crate::runtime::string_semantics::utf16_code_unit(current, pos)
                        })
                        .map(i32::from)
                        .unwrap_or(-1),
                );
            }
            str_op::TONUM => self.push_int(current.parse::<i32>().unwrap_or(0)),
            _ => bail!("unsupported CALL_PROP string op {}", op),
        }
        Ok(())
    }

    fn assign_call_prop_result(prop: &mut CallProp, sub: &[i32], rhs: Value) -> Result<()> {
        use crate::runtime::forms::codes::{
            ELM_ARRAY, FM_INT, FM_INTLIST, FM_INTLISTREF, FM_INTREF, FM_STR, FM_STRLIST,
            FM_STRLISTREF, FM_STRREF,
        };

        match prop.form {
            FM_INT if sub.is_empty() => match rhs {
                Value::Int(n) => {
                    prop.value = CallPropValue::Int(n as i32);
                }
                _ => bail!("unsupported CALL_PROP int assign sub={:?}", sub),
            },
            FM_STR if sub.is_empty() => match rhs {
                Value::Str(s) => {
                    prop.value = CallPropValue::Str(s);
                }
                _ => bail!("unsupported CALL_PROP str assign sub={:?}", sub),
            },
            FM_INTLIST if sub.len() >= 2 && sub[0] == ELM_ARRAY => match rhs {
                Value::Int(n) => {
                    let idx = sub[1].max(0) as usize;
                    let mut dst = match std::mem::replace(
                        &mut prop.value,
                        CallPropValue::IntList(Vec::new()),
                    ) {
                        CallPropValue::IntList(v) => v,
                        other => {
                            prop.value = other;
                            bail!("CALL_PROP intlist storage mismatch");
                        }
                    };
                    if dst.len() <= idx {
                        dst.resize(idx + 1, 0);
                    }
                    dst[idx] = n as i32;
                    prop.value = CallPropValue::IntList(dst);
                }
                _ => bail!("unsupported CALL_PROP intlist assign sub={:?}", sub),
            },
            FM_STRLIST if sub.len() >= 2 && sub[0] == ELM_ARRAY => match rhs {
                Value::Str(s) => {
                    let idx = sub[1].max(0) as usize;
                    let mut dst = match std::mem::replace(
                        &mut prop.value,
                        CallPropValue::StrList(Vec::new()),
                    ) {
                        CallPropValue::StrList(v) => v,
                        other => {
                            prop.value = other;
                            bail!("CALL_PROP strlist storage mismatch");
                        }
                    };
                    if dst.len() <= idx {
                        dst.resize_with(idx + 1, String::new);
                    }
                    dst[idx] = s;
                    prop.value = CallPropValue::StrList(dst);
                }
                _ => bail!("unsupported CALL_PROP strlist assign sub={:?}", sub),
            },
            FM_INTREF | FM_STRREF | FM_INTLISTREF | FM_STRLISTREF => match rhs {
                Value::Element(e) => {
                    prop.element = e.clone();
                    prop.value = CallPropValue::Element(e);
                }
                _ => bail!("unsupported CALL_PROP ref assign sub={:?}", sub),
            },
            _ => bail!(
                "unsupported call prop assign form={} sub={:?}",
                prop.form,
                sub
            ),
        }
        Ok(())
    }

    fn set_user_int_list_value(values: &mut [i32], bit: i32, index: i32, value: i32) {
        if index < 0 || !matches!(bit, 1 | 2 | 4 | 8 | 16 | 32) {
            return;
        }
        let index = index as usize;
        if bit == 32 {
            if let Some(slot) = values.get_mut(index) {
                *slot = value;
            }
            return;
        }
        let per_word = 32usize / bit as usize;
        let Some(word) = values.get_mut(index / per_word) else {
            return;
        };
        let shift = (index % per_word) * bit as usize;
        let mask = ((1u32 << bit) - 1) << shift;
        let encoded = ((value as u32) << shift) & mask;
        *word = (((*word as u32) & !mask) | encoded) as i32;
    }

    fn get_user_int_list_value(values: &[i32], bit: i32, index: i32) -> Option<i32> {
        if index < 0 || !matches!(bit, 1 | 2 | 4 | 8 | 16 | 32) {
            return None;
        }
        let index = index as usize;
        if bit == 32 {
            return values.get(index).copied();
        }
        let per_word = 32usize / bit as usize;
        let word = *values.get(index / per_word)? as u32;
        let shift = (index % per_word) * bit as usize;
        Some(((word >> shift) & ((1u32 << bit) - 1)) as i32)
    }

    fn exec_user_prop_list_command(
        &mut self,
        prop_id: u16,
        sub: &[i32],
        al_id: i32,
        ret_form: i32,
        args: &[Value],
    ) -> Result<bool> {
        use crate::runtime::forms::codes::{
            ELM_INTLIST_BIT, ELM_INTLIST_BIT16, ELM_INTLIST_BIT2,
            ELM_INTLIST_BIT4, ELM_INTLIST_BIT8, ELM_INTLIST_CLEAR,
            ELM_INTLIST_GET_SIZE, ELM_INTLIST_INIT, ELM_INTLIST_RESIZE,
            ELM_INTLIST_SETS, ELM_STRLIST_GET_SIZE, ELM_STRLIST_INIT,
            ELM_STRLIST_RESIZE,
        };

        if sub.is_empty() {
            return Ok(false);
        }

        let (decl_form, decl_size) = self
            .user_prop_decl(prop_id)
            .unwrap_or((self.cfg.fm_list, 0));
        let mut cell = self
            .user_props
            .remove(&prop_id)
            .unwrap_or_else(|| self.default_user_prop_cell(prop_id));
        let form = cell.form;
        let mut handled = false;

        if form == self.cfg.fm_intlist {
            let mut bit = 32;
            let mut op = sub;
            if let Some(first) = op.first().copied() {
                bit = match first {
                    ELM_INTLIST_BIT => 1,
                    ELM_INTLIST_BIT2 => 2,
                    ELM_INTLIST_BIT4 => 4,
                    ELM_INTLIST_BIT8 => 8,
                    ELM_INTLIST_BIT16 => 16,
                    _ => 32,
                };
                if bit != 32 {
                    op = &op[1..];
                }
            }

            if op.len() >= 2 && self.call_array_marker(op[0]) {
                let index = op[1];
                if al_id == 0 {
                    if let Some(value) = Self::get_user_int_list_value(&cell.int_list, bit, index) {
                        self.push_int(value);
                    } else {
                        self.push_default_for_ret(ret_form);
                    }
                } else {
                    let value = args.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                    Self::set_user_int_list_value(&mut cell.int_list, bit, index, value);
                    self.push_default_for_ret(ret_form);
                }
                handled = true;
            } else if op.len() == 1 {
                match op[0] {
                    ELM_INTLIST_INIT => {
                        cell.form = decl_form;
                        cell.element = self.default_user_prop_element(prop_id, decl_form);
                        cell.int_list.clear();
                        cell.int_list.resize(decl_size, 0);
                        self.push_default_for_ret(ret_form);
                        handled = true;
                    }
                    ELM_INTLIST_RESIZE => {
                        let new_len = args
                            .first()
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0)
                            .max(0) as usize;
                        cell.int_list.resize(new_len, 0);
                        self.push_default_for_ret(ret_form);
                        handled = true;
                    }
                    ELM_INTLIST_GET_SIZE => {
                        let multiplier = 32 / bit;
                        self.push_int((cell.int_list.len() * multiplier as usize) as i32);
                        handled = true;
                    }
                    ELM_INTLIST_CLEAR => {
                        let start = args.get(0).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        let end = args.get(1).and_then(|v| v.as_i64()).unwrap_or(start as i64)
                            as i32;
                        let value = if al_id == 0 {
                            0
                        } else {
                            args.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32
                        };
                        if start <= end {
                            for index in start..=end {
                                Self::set_user_int_list_value(
                                    &mut cell.int_list,
                                    bit,
                                    index,
                                    value,
                                );
                            }
                        }
                        self.push_default_for_ret(ret_form);
                        handled = true;
                    }
                    ELM_INTLIST_SETS => {
                        let mut index = args.get(0).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        for value in args.iter().skip(1) {
                            Self::set_user_int_list_value(
                                &mut cell.int_list,
                                bit,
                                index,
                                value.as_i64().unwrap_or(0) as i32,
                            );
                            index = index.saturating_add(1);
                        }
                        self.push_default_for_ret(ret_form);
                        handled = true;
                    }
                    _ => {}
                }
            }
        } else if form == self.cfg.fm_strlist {
            if sub.len() >= 2 && self.call_array_marker(sub[0]) {
                let index = sub[1];
                let value = usize::try_from(index)
                    .ok()
                    .and_then(|index| cell.str_list.get(index).cloned());
                if sub.len() == 2 {
                    if al_id == 0 {
                        if let Some(value) = value {
                            self.push_str(value);
                        } else {
                            self.push_default_for_ret(ret_form);
                        }
                    } else if let Ok(index) = usize::try_from(index) {
                        if let Some(slot) = cell.str_list.get_mut(index) {
                            *slot = args
                                .first()
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                        }
                        self.push_default_for_ret(ret_form);
                    } else {
                        self.push_default_for_ret(ret_form);
                    }
                } else if let Some(value) = value {
                    self.call_prop_eval_str_op(&value, sub[2], args, al_id)?;
                } else {
                    self.push_default_for_ret(ret_form);
                }
                handled = true;
            } else if sub.len() == 1 {
                match sub[0] {
                    ELM_STRLIST_INIT => {
                        cell.form = decl_form;
                        cell.element = self.default_user_prop_element(prop_id, decl_form);
                        cell.str_list.clear();
                        cell.str_list.resize_with(decl_size, String::new);
                        self.push_default_for_ret(ret_form);
                        handled = true;
                    }
                    ELM_STRLIST_RESIZE => {
                        let new_len = args
                            .first()
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0)
                            .max(0) as usize;
                        cell.str_list.resize_with(new_len, String::new);
                        self.push_default_for_ret(ret_form);
                        handled = true;
                    }
                    ELM_STRLIST_GET_SIZE => {
                        self.push_int(cell.str_list.len() as i32);
                        handled = true;
                    }
                    _ => {}
                }
            }
        }

        self.user_props.insert(prop_id, cell);
        Ok(handled)
    }

    fn exec_call_prop_command(
        &mut self,
        frame_idx: usize,
        prop_idx: usize,
        sub: &[i32],
        al_id: i32,
        ret_form: i32,
        args: &[Value],
    ) -> Result<()> {
        use crate::runtime::forms::codes::{
            ELM_ARRAY, ELM_INTLIST_BIT, ELM_INTLIST_BIT16, ELM_INTLIST_BIT2, ELM_INTLIST_BIT4,
            ELM_INTLIST_BIT8, ELM_INTLIST_CLEAR, ELM_INTLIST_GET_SIZE, ELM_INTLIST_INIT,
            ELM_INTLIST_RESIZE, ELM_INTLIST_SETS, ELM_STRLIST_GET_SIZE, ELM_STRLIST_INIT,
            ELM_STRLIST_RESIZE, FM_INT, FM_INTLIST, FM_INTLISTREF, FM_INTREF, FM_STR, FM_STRLIST,
            FM_STRLISTREF, FM_STRREF,
        };

        let (form, decl_size, mut value, mut element) = {
            let prop = self
                .call_stack
                .get(frame_idx)
                .and_then(|f| f.user_props.get(prop_idx))
                .ok_or_else(|| anyhow!("call prop frame/index out of range"))?;
            (prop.form, prop.decl_size, prop.value.clone(), prop.element.clone())
        };

        let mut write_back = false;

        if !sub.is_empty() && !self.is_direct_value_form(form) {
            let mut composed = match &value {
                CallPropValue::Element(e) if !e.is_empty() => e.clone(),
                _ => element.clone(),
            };
            if !composed.is_empty() {
                composed.extend_from_slice(sub);
                let mut owned_args = args.to_vec();
                self.exec_command(composed, al_id, ret_form, &mut owned_args)?;
                return Ok(());
            }
        }

        match form {
            FM_INT => {
                if sub.is_empty() {
                    if al_id == 0 {
                        match &value {
                            CallPropValue::Int(n) => self.push_int(*n),
                            _ => bail!("CALL_PROP int storage mismatch"),
                        }
                    } else {
                        let rhs = args.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        value = CallPropValue::Int(rhs);
                        write_back = true;
                        self.push_default_for_ret(ret_form);
                    }
                } else {
                    self.push_element(element.clone());
                }
            }
            FM_STR => {
                let current = match &value {
                    CallPropValue::Str(s) => s.clone(),
                    _ => bail!("CALL_PROP str storage mismatch"),
                };
                if sub.is_empty() {
                    if al_id == 0 {
                        self.push_str(current);
                    } else {
                        let rhs = args
                            .first()
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        value = CallPropValue::Str(rhs);
                        write_back = true;
                        self.push_default_for_ret(ret_form);
                    }
                } else {
                    self.call_prop_eval_str_op(&current, sub[0], args, al_id)?;
                }
            }
            FM_INTLIST => {
                let mut list = match value {
                    CallPropValue::IntList(v) => v,
                    _ => bail!("CALL_PROP intlist storage mismatch"),
                };
                if sub.is_empty() {
                    self.push_element(element.clone());
                } else if sub.len() >= 2 && sub[0] == ELM_ARRAY {
                    let idx = sub[1].max(0) as usize;
                    if al_id == 0 {
                        self.push_int(list.get(idx).copied().unwrap_or(0));
                    } else {
                        let rhs = args.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        if list.len() <= idx {
                            list.resize(idx + 1, 0);
                        }
                        list[idx] = rhs;
                        write_back = true;
                        self.push_default_for_ret(ret_form);
                    }
                } else {
                    match sub[0] {
                        ELM_INTLIST_BIT | ELM_INTLIST_BIT2 | ELM_INTLIST_BIT4
                        | ELM_INTLIST_BIT8 | ELM_INTLIST_BIT16 => {
                            self.push_element(element.clone());
                        }
                        ELM_INTLIST_INIT => {
                            list.clear();
                            list.resize(decl_size, 0);
                            write_back = true;
                            self.push_default_for_ret(ret_form);
                        }
                        ELM_INTLIST_RESIZE => {
                            let new_len =
                                args.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                            list.resize(new_len, 0);
                            write_back = true;
                            self.push_default_for_ret(ret_form);
                        }
                        ELM_INTLIST_GET_SIZE => self.push_int(list.len() as i32),
                        ELM_INTLIST_CLEAR => {
                            let start =
                                args.get(0).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                            let end =
                                args.get(1).and_then(|v| v.as_i64()).unwrap_or(-1).max(-1) as isize;
                            let clear_value = if al_id == 0 {
                                0
                            } else {
                                args.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32
                            };
                            if !list.is_empty() && end >= 0 {
                                let end = usize::min(end as usize, list.len().saturating_sub(1));
                                for i in start..=end {
                                    if i < list.len() {
                                        list[i] = clear_value;
                                    }
                                }
                            }
                            write_back = true;
                            self.push_default_for_ret(ret_form);
                        }
                        ELM_INTLIST_SETS => {
                            let start =
                                args.get(0).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                            for (off, v) in args.iter().skip(1).enumerate() {
                                let idx = start + off;
                                if list.len() <= idx {
                                    list.resize(idx + 1, 0);
                                }
                                list[idx] = v.as_i64().unwrap_or(0) as i32;
                            }
                            write_back = true;
                            self.push_default_for_ret(ret_form);
                        }
                        _ => bail!("unsupported CALL_PROP intlist op {:?}", sub),
                    }
                }
                value = CallPropValue::IntList(list);
            }
            FM_STRLIST => {
                let mut list = match value {
                    CallPropValue::StrList(v) => v,
                    _ => bail!("CALL_PROP strlist storage mismatch"),
                };
                if sub.is_empty() {
                    self.push_element(element.clone());
                } else if sub.len() >= 2 && sub[0] == ELM_ARRAY {
                    let idx = sub[1].max(0) as usize;
                    if list.len() <= idx {
                        list.resize_with(idx + 1, String::new);
                    }
                    if sub.len() == 2 {
                        if al_id == 0 {
                            self.push_str(list[idx].clone());
                        } else {
                            let rhs = args
                                .first()
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            list[idx] = rhs;
                            write_back = true;
                            self.push_default_for_ret(ret_form);
                        }
                    } else {
                        let current = list[idx].clone();
                        self.call_prop_eval_str_op(&current, sub[2], args, al_id)?;
                    }
                } else {
                    match sub[0] {
                        ELM_STRLIST_INIT => {
                            list.clear();
                            list.resize_with(decl_size, String::new);
                            write_back = true;
                            self.push_default_for_ret(ret_form);
                        }
                        ELM_STRLIST_RESIZE => {
                            let new_len =
                                args.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                            list.resize_with(new_len, String::new);
                            write_back = true;
                            self.push_default_for_ret(ret_form);
                        }
                        ELM_STRLIST_GET_SIZE => self.push_int(list.len() as i32),
                        _ => bail!("unsupported CALL_PROP strlist op {:?}", sub),
                    }
                }
                value = CallPropValue::StrList(list);
            }
            FM_INTREF | FM_STRREF | FM_INTLISTREF | FM_STRLISTREF => {
                if sub.is_empty() {
                    if al_id == 0 {
                        if let CallPropValue::Element(e) = &value {
                            if e.is_empty() {
                                self.push_element(element.clone());
                            } else {
                                self.push_element(e.clone());
                            }
                        } else {
                            self.push_element(element.clone());
                        }
                    } else {
                        let rhs = args.first().cloned().unwrap_or(Value::Element(Vec::new()));
                        match rhs {
                            Value::Element(e) => {
                                element = e.clone();
                                value = CallPropValue::Element(e);
                                write_back = true;
                            }
                            _ => bail!("CALL_PROP ref assign requires element"),
                        }
                        self.push_default_for_ret(ret_form);
                    }
                } else if let CallPropValue::Element(e) = &value {
                    if e.is_empty() {
                        self.push_element(element.clone());
                    } else {
                        self.push_element(e.clone());
                    }
                } else {
                    self.push_element(element.clone());
                }
            }
            _ => {
                if !sub.is_empty() || al_id == 0 {
                    self.push_element(element.clone());
                } else {
                    bail!("unsupported CALL_PROP form {}", form);
                }
            }
        }

        if write_back {
            let prop = self
                .call_stack
                .get_mut(frame_idx)
                .and_then(|f| f.user_props.get_mut(prop_idx))
                .ok_or_else(|| anyhow!("call prop frame/index out of range"))?;
            prop.value = value;
            prop.element = element;
        }
        Ok(())
    }

    fn exec_call_property(&mut self, elm: &[i32]) -> Result<bool> {
        vm_trace!(self, None, format!("exec_call_property elm={:?}", elm));
        use crate::runtime::forms::codes::{
            ELM_CALL_K, ELM_CALL_L, ELM_GLOBAL_CUR_CALL, ELM_INTLIST_GET_SIZE,
            ELM_STRLIST_GET_SIZE, FM_CALL, FM_CALLLIST,
        };

        if elm.is_empty() {
            return Ok(false);
        }
        let head = elm[0];
        if head != FM_CALL && head != FM_CALLLIST && head != ELM_GLOBAL_CUR_CALL {
            return Ok(false);
        }

        let current_idx = self
            .current_call_frame_index()
            .ok_or_else(|| anyhow!("call stack underflow"))?;

        let tail: &[i32] = if head == FM_CALLLIST {
            if elm.len() < 3 || !self.call_array_marker(elm[1]) {
                bail!("malformed CALLLIST access: {:?}", elm);
            }
            self.resolve_call_frame_index(elm[2])
                .ok_or_else(|| anyhow!("CALLLIST index out of range: {}", elm[2]))?;
            &elm[3..]
        } else {
            &elm[1..]
        };

        if tail.is_empty() {
            self.push_element(elm.to_vec());
            return Ok(true);
        }

        match tail[0] {
            ELM_CALL_L => {
                let sub = &tail[1..];
                if sub.is_empty() {
                    self.push_element(elm.to_vec());
                } else if sub.len() >= 2 && self.call_array_marker(sub[0]) {
                    let idx = usize::try_from(sub[1]).ok();
                    let v = idx
                        .and_then(|idx| self.call_stack[current_idx].int_args.get(idx))
                        .copied()
                        .unwrap_or(0);
                    self.push_int(v);
                } else if sub[0] == ELM_INTLIST_GET_SIZE {
                    self.push_int(self.call_stack[current_idx].int_args.len() as i32);
                } else {
                    self.push_element(elm[..elm.len() - sub.len()].to_vec());
                }
                return Ok(true);
            }
            ELM_CALL_K => {
                let sub = &tail[1..];
                if sub.is_empty() {
                    self.push_element(elm.to_vec());
                } else if sub.len() >= 2 && self.call_array_marker(sub[0]) {
                    let idx = usize::try_from(sub[1]).ok();
                    let v = idx
                        .and_then(|idx| self.call_stack[current_idx].str_args.get(idx))
                        .cloned()
                        .unwrap_or_default();
                    if sub.len() == 2 {
                        self.push_str(v);
                    } else {
                        self.call_prop_eval_str_op(&v, sub[2], &[], 0)?;
                    }
                } else if sub[0] == ELM_STRLIST_GET_SIZE {
                    self.push_int(self.call_stack[current_idx].str_args.len() as i32);
                } else {
                    self.push_element(elm[..elm.len() - sub.len()].to_vec());
                }
                return Ok(true);
            }
            _ => {}
        }

        if elm_code::owner(tail[0]) != elm_code::ELM_OWNER_CALL_PROP {
            bail!("invalid CALL property owner for {:?}", elm);
        }

        let call_prop_id = elm_code::code(tail[0]) as i32;
        let prop_idx = self
            .find_call_prop_index_in_frame(current_idx, call_prop_id)
            .ok_or_else(|| anyhow!("missing CALL_PROP id={} for {:?}", call_prop_id, elm))?;
        let prop = self.call_stack[current_idx].user_props[prop_idx].clone();
        let sub = &tail[1..];
        if let Some(composed) = self.compose_call_prop_tail(&prop, sub) {
            self.exec_property(composed)?;
            return Ok(true);
        }
        self.push_call_prop_result(&prop, sub, elm)?;
        Ok(true)
    }

    fn exec_call_assign(&mut self, elm: &[i32], al_id: i32, rhs: Value) -> Result<bool> {
        use crate::runtime::forms::codes::{
            ELM_CALL_K, ELM_CALL_L, ELM_GLOBAL_CUR_CALL, FM_CALL, FM_CALLLIST,
        };

        if elm.is_empty() {
            return Ok(false);
        }
        let head = elm[0];
        if head != FM_CALL && head != FM_CALLLIST && head != ELM_GLOBAL_CUR_CALL {
            return Ok(false);
        }

        let current_idx = match self.current_call_frame_index() {
            Some(v) => v,
            None => return Ok(true),
        };

        let tail: &[i32] = if head == FM_CALLLIST {
            if elm.len() < 3 || !self.call_array_marker(elm[1]) {
                bail!("malformed CALLLIST assign: {:?}", elm);
            }
            self.resolve_call_frame_index(elm[2])
                .ok_or_else(|| anyhow!("CALLLIST index out of range: {}", elm[2]))?;
            &elm[3..]
        } else {
            &elm[1..]
        };

        if tail.is_empty() {
            return Ok(true);
        }

        match tail[0] {
            ELM_CALL_L => {
                let sub = &tail[1..];
                if sub.len() >= 2 && self.call_array_marker(sub[0]) {
                    if let (Ok(idx), Value::Int(n)) = (usize::try_from(sub[1]), rhs) {
                        let len = self.call_stack[current_idx].int_args.len();
                        let old = self.call_stack[current_idx].int_args.get(idx).copied();
                        vm_trace!(self,
                            None,
                            format!(
                                "CALL.L assign frame={} idx={} len={} old={:?} new={}",
                                current_idx, idx, len, old, n
                            ),
                        );
                        if let Some(slot) = self.call_stack[current_idx].int_args.get_mut(idx) {
                            *slot = n as i32;
                        }
                    }
                }
                return Ok(true);
            }
            ELM_CALL_K => {
                let sub = &tail[1..];
                if sub.len() >= 2 && self.call_array_marker(sub[0]) {
                    if let (Ok(idx), Value::Str(s)) = (usize::try_from(sub[1]), rhs) {
                        let len = self.call_stack[current_idx].str_args.len();
                        let old = self.call_stack[current_idx].str_args.get(idx).cloned();
                        vm_trace!(self,
                            None,
                            format!(
                                "CALL.K assign frame={} idx={} len={} old={:?} new={:?}",
                                current_idx, idx, len, old, s
                            ),
                        );
                        if let Some(slot) = self.call_stack[current_idx].str_args.get_mut(idx) {
                            *slot = s;
                        }
                    }
                }
                return Ok(true);
            }
            _ => {}
        }

        if elm_code::owner(tail[0]) != elm_code::ELM_OWNER_CALL_PROP {
            bail!("invalid CALL assign owner for {:?}", elm);
        }
        let call_prop_id = elm_code::code(tail[0]) as i32;
        let sub = &tail[1..];
        let prop_idx = self.ensure_call_prop_index_for_assign(current_idx, call_prop_id, &rhs)?;
        let prop_for_compose = self.call_stack[current_idx].user_props[prop_idx].clone();
        if let Some(composed) = self.compose_call_prop_tail(&prop_for_compose, sub) {
            self.exec_assign(composed, al_id, rhs)?;
            return Ok(true);
        }

        if sub.is_empty()
            && matches!(
                prop_for_compose.form,
                crate::runtime::forms::codes::FM_INTREF
                    | crate::runtime::forms::codes::FM_STRREF
            )
        {
            // Assignment to a scalar ref writes through to the referenced
            // element. Rebinding the CALL_PROP itself would lose the caller's
            // variable and contradict the compiler's STRREF/INTREF assignment
            // encoding (left form remains *REF, right form is scalar).
            let target = self.call_prop_effective_element(&prop_for_compose);
            let default_target = Self::call_prop_element(prop_for_compose.prop_id);
            if target.is_empty() || target == elm || target == default_target {
                bail!(
                    "unbound scalar CALL_PROP assignment form={} target={:?} access={:?}",
                    prop_for_compose.form,
                    target,
                    elm
                );
            }
            self.exec_assign(target, al_id, rhs)?;
            return Ok(true);
        }

        let frame = &mut self.call_stack[current_idx];
        let prop = frame
            .user_props
            .get_mut(prop_idx)
            .ok_or_else(|| anyhow!("missing CALL_PROP slot assign id={}", call_prop_id))?;
        Self::assign_call_prop_result(prop, sub, rhs)?;
        Ok(true)
    }

    fn exec_call_command(
        &mut self,
        elm: &[i32],
        al_id: i32,
        ret_form: i32,
        args: &[Value],
    ) -> Result<bool> {
        use crate::runtime::forms::codes::{
            ELM_CALL_K, ELM_CALL_L, ELM_GLOBAL_CUR_CALL, ELM_INTLIST_CLEAR, ELM_INTLIST_GET_SIZE,
            ELM_INTLIST_INIT, ELM_INTLIST_RESIZE, ELM_INTLIST_SETS, ELM_STRLIST_GET_SIZE,
            ELM_STRLIST_INIT, ELM_STRLIST_RESIZE, FM_CALL, FM_CALLLIST,
        };

        if elm.is_empty() {
            return Ok(false);
        }
        let head = elm[0];
        if head != FM_CALL && head != FM_CALLLIST && head != ELM_GLOBAL_CUR_CALL {
            return Ok(false);
        }

        let current_idx = match self.current_call_frame_index() {
            Some(v) => v,
            None => {
                self.push_default_for_ret(ret_form);
                return Ok(true);
            }
        };

        let tail: &[i32] = if head == FM_CALLLIST {
            if elm.len() < 3 || !self.call_array_marker(elm[1]) {
                self.push_default_for_ret(ret_form);
                return Ok(true);
            }
            let Some(_selected_idx) = self.resolve_call_frame_index(elm[2]) else {
                self.push_default_for_ret(ret_form);
                return Ok(true);
            };
            &elm[3..]
        } else {
            &elm[1..]
        };

        if tail.is_empty() {
            self.push_default_for_ret(ret_form);
            return Ok(true);
        }

        match tail[0] {
            ELM_CALL_L => {
                let sub = &tail[1..];
                if sub.is_empty() {
                    self.push_default_for_ret(ret_form);
                    return Ok(true);
                }
                if sub.len() >= 2 && self.call_array_marker(sub[0]) {
                    let idx = usize::try_from(sub[1]).ok();
                    if al_id == 1 {
                        let rhs = args.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        if let Some(slot) = idx
                            .and_then(|idx| self.call_stack[current_idx].int_args.get_mut(idx))
                        {
                            *slot = rhs;
                        }
                        self.push_default_for_ret(ret_form);
                    } else {
                        let value = idx
                            .and_then(|idx| self.call_stack[current_idx].int_args.get(idx))
                            .copied()
                            .unwrap_or(0);
                        self.push_int(value);
                    }
                    return Ok(true);
                }
                match sub[0] {
                    ELM_INTLIST_INIT => {
                        let values = Self::blank_call_int_args(self.call_flag_count);
                        self.call_stack[current_idx].int_args = values;
                    }
                    ELM_INTLIST_RESIZE => {
                        let new_len =
                            args.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                        self.call_stack[current_idx].int_args.resize(new_len, 0);
                    }
                    ELM_INTLIST_GET_SIZE => {
                        self.push_int(self.call_stack[current_idx].int_args.len() as i32);
                    }
                    ELM_INTLIST_CLEAR => {
                        let start = args.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
                        let end = args.get(1).and_then(|v| v.as_i64()).unwrap_or(start);
                        let value = if al_id == 0 {
                            0
                        } else {
                            args.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32
                        };
                        if start <= end {
                            let frame = &mut self.call_stack[current_idx];
                            for index in start..=end {
                                if let Ok(index) = usize::try_from(index) {
                                    if let Some(slot) = frame.int_args.get_mut(index) {
                                        *slot = value;
                                    }
                                }
                            }
                        }
                    }
                    ELM_INTLIST_SETS => {
                        let start = args.get(0).and_then(|v| v.as_i64()).unwrap_or(0);
                        let frame = &mut self.call_stack[current_idx];
                        for (offset, value) in args.iter().skip(1).enumerate() {
                            let Some(index) = start.checked_add(offset as i64) else {
                                break;
                            };
                            if let Ok(index) = usize::try_from(index) {
                                if let Some(slot) = frame.int_args.get_mut(index) {
                                    *slot = value.as_i64().unwrap_or(0) as i32;
                                }
                            }
                        }
                    }
                    _ => self.push_default_for_ret(ret_form),
                }
                return Ok(true);
            }
            ELM_CALL_K => {
                let sub = &tail[1..];
                if sub.is_empty() {
                    self.push_default_for_ret(ret_form);
                    return Ok(true);
                }
                if sub.len() >= 2 && self.call_array_marker(sub[0]) {
                    let idx = usize::try_from(sub[1]).ok();
                    if sub.len() == 2 {
                        if al_id == 1 {
                            let rhs = args
                                .first()
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            if let Some(slot) = idx
                                .and_then(|idx| self.call_stack[current_idx].str_args.get_mut(idx))
                            {
                                *slot = rhs;
                            }
                            self.push_default_for_ret(ret_form);
                        } else {
                            let value = idx
                                .and_then(|idx| self.call_stack[current_idx].str_args.get(idx))
                                .cloned()
                                .unwrap_or_default();
                            self.push_str(value);
                        }
                    } else {
                        let value = idx
                            .and_then(|idx| self.call_stack[current_idx].str_args.get(idx))
                            .cloned()
                            .unwrap_or_default();
                        self.call_prop_eval_str_op(&value, sub[2], args, al_id)?;
                    }
                    return Ok(true);
                }
                match sub[0] {
                    ELM_STRLIST_INIT => {
                        let values = Self::blank_call_str_args(self.call_flag_count);
                        self.call_stack[current_idx].str_args = values;
                    }
                    ELM_STRLIST_RESIZE => {
                        let new_len =
                            args.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                        self.call_stack[current_idx]
                            .str_args
                            .resize_with(new_len, String::new);
                    }
                    ELM_STRLIST_GET_SIZE => {
                        self.push_int(self.call_stack[current_idx].str_args.len() as i32);
                    }
                    _ => self.push_default_for_ret(ret_form),
                }
                return Ok(true);
            }
            _ => {
                if elm_code::owner(tail[0]) == elm_code::ELM_OWNER_CALL_PROP {
                    let call_prop_id = elm_code::code(tail[0]) as i32;
                    let prop_idx = self
                        .find_call_prop_index_in_frame(current_idx, call_prop_id)
                        .ok_or_else(|| {
                            anyhow!(
                                "missing CALL_PROP command id={} for {:?}",
                                call_prop_id,
                                elm
                            )
                        })?;
                    self.exec_call_prop_command(
                        current_idx,
                        prop_idx,
                        &tail[1..],
                        al_id,
                        ret_form,
                        args,
                    )?;
                    return Ok(true);
                }
                bail!("unsupported CALL command chain {:?}", elm);
            }
        }
    }

    fn push_property_value(&mut self, v: Value, array_idx: Option<usize>) {
        match v {
            Value::NamedArg { value, .. } => self.push_property_value(*value, array_idx),
            Value::Int(n) => self.push_int(n as i32),
            Value::Str(s) => self.push_str(s),
            Value::Element(elm) => self.push_element(elm),
            Value::List(items) => {
                if let Some(i) = array_idx {
                    if let Some(item) = items.get(i).cloned() {
                        self.push_property_value(item, None);
                    } else {
                        self.push_int(0);
                    }
                } else {
                    panic!("raw Value::List used as property result; expected runtime ref");
                }
            }
        }
    }

    fn assign_user_prop(&mut self, prop_id: u16, array_idx: Option<usize>, rhs: Value) {
        let decl = self
            .user_prop_decl(prop_id)
            .unwrap_or((self.cfg.fm_list, 0));
        if let Some(i) = array_idx {
            let slot_element = self.default_user_prop_slot_element(prop_id, i);
            let default_entry = self.default_user_prop_cell(prop_id);
            let existing_form = self
                .user_props
                .get(&prop_id)
                .map(|e| e.form)
                .unwrap_or(default_entry.form);
            if existing_form == self.cfg.fm_intlist {
                let entry = self.user_props.entry(prop_id).or_insert(default_entry);
                if let Some(slot) = entry.int_list.get_mut(i) {
                    *slot = rhs.as_i64().unwrap_or(0) as i32;
                }
                return;
            }
            if existing_form == self.cfg.fm_strlist {
                let entry = self.user_props.entry(prop_id).or_insert(default_entry);
                if let Some(slot) = entry.str_list.get_mut(i) {
                    *slot = rhs.as_str().unwrap_or("").to_string();
                }
                return;
            }
            let list_form = self.cfg.fm_list;
            let new_slot =
                self.user_prop_cell_from_value(rhs, list_form, slot_element, Some(prop_id));
            let head = constants::elm::create(constants::elm::OWNER_USER_PROP, 0, prop_id as i32);
            let elm_array = self.ctx.ids.elm_array;
            let entry = self.user_props.entry(prop_id).or_insert(default_entry);
            if entry.list_items.len() <= i {
                let cur = entry.list_items.len();
                entry
                    .list_items
                    .resize_with(i + 1, || UserPropCell::new(list_form, Vec::new()));
                for idx in cur..entry.list_items.len() {
                    entry.list_items[idx].form = list_form;
                    entry.list_items[idx].element = vec![head, elm_array, idx as i32];
                }
            }
            entry.list_items[i] = new_slot;
        } else {
            let element = self.default_user_prop_element(prop_id, decl.0);
            let cell = self.user_prop_cell_from_value(rhs, decl.0, element, Some(prop_id));
            self.user_props.insert(prop_id, cell);
        }
    }

    fn exec_copy_element(&mut self) -> Result<()> {
        let start = match self.element_points.last().copied() {
            Some(v) => v,
            None => {
                vm_trace!(self, None, "COPY_ELM missing prior ELM_POINT");
                return Err(anyhow!("COPY_ELM without a prior ELM_POINT"));
            }
        };
        if start > self.int_stack.len() {
            vm_trace!(self,
                None,
                format!(
                    "COPY_ELM invalid start={} len={}",
                    start,
                    self.int_stack.len()
                ),
            );
            bail!(
                "invalid element point start={start} len={}",
                self.int_stack.len()
            );
        }
        let slice = self.int_stack[start..].to_vec();
        if self.sg_mwnd_object_trace_enabled() && Self::sg_mwnd_chain_interesting(&slice) {
            self.sg_mwnd_object_trace_emit(format_args!(
                "COPY_ELM slice={:?} before_current_chain={:?} before_current_stage_object={:?}",
                slice,
                self.ctx.globals.current_object_chain,
                self.ctx.globals.current_stage_object
            ));
        }
        self.element_points.push(self.int_stack.len());
        self.int_stack.extend_from_slice(&slice);
        vm_trace!(self, None, format!("COPY_ELM copied {:?}", slice));
        Ok(())
    }

    fn pop_value_for_form(&mut self, form_code: i32) -> Result<Value> {
        if form_code == self.cfg.fm_void {
            return Ok(Value::Int(0));
        }
        if form_code == self.cfg.fm_int {
            return Ok(Value::Int(self.pop_int()? as i64));
        }
        if form_code == self.cfg.fm_str {
            return Ok(Value::Str(self.pop_str()?));
        }
        if form_code == self.cfg.fm_label {
            return Ok(Value::Int(self.pop_int()? as i64));
        }
        if form_code == self.cfg.fm_list {
            let nested = self.pop_arg_list()?;
            return Ok(Value::List(nested));
        }

        // Unknown form: treat as element.
        self.trace_unknown_form(form_code, "pop_value_for_form");
        Ok(Value::Element(self.pop_element()?))
    }

    fn pop_arg_list(&mut self) -> Result<Vec<Value>> {
        let arg_cnt_i32 = self.stream.pop_i32()?;
        if arg_cnt_i32 < 0 {
            bail!("negative arg_cnt={arg_cnt_i32}");
        }
        let arg_cnt = arg_cnt_i32 as usize;
        let mut out: Vec<Value> = vec![Value::Int(0); arg_cnt];

        // The original fills from the end (stack pop order).
        for i in (0..arg_cnt).rev() {
            let form_code = self.stream.pop_i32()?;
            let v = self.pop_value_for_form(form_code)?;
            out[i] = v;
        }
        Ok(out)
    }

    fn exec_push(&mut self, form_code: i32) -> Result<()> {
        if form_code == self.cfg.fm_void {
            return Ok(());
        }
        if form_code == self.cfg.fm_int {
            let v = self.stream.pop_i32()?;
            self.push_int(v);
            return Ok(());
        }
        if form_code == self.cfg.fm_str {
            let s = self.stream.pop_str()?;
            self.push_str(s);
            return Ok(());
        }

        // Other forms are not pushed by CD_PUSH in the fork.
        self.trace_unknown_form(form_code, "exec_push");
        Ok(())
    }

    fn exec_pop(&mut self, form_code: i32) -> Result<()> {
        if form_code == self.cfg.fm_void {
            return Ok(());
        }
        if form_code == self.cfg.fm_int {
            let _ = self.pop_int()?;
            return Ok(());
        }
        if form_code == self.cfg.fm_str {
            let _ = self.pop_str()?;
            return Ok(());
        }

        self.trace_unknown_form(form_code, "exec_pop");
        Ok(())
    }

    fn exec_copy(&mut self, form_code: i32) -> Result<()> {
        if form_code == self.cfg.fm_void {
            return Ok(());
        }
        if form_code == self.cfg.fm_int {
            let v = self.peek_int()?;
            self.push_int(v);
            return Ok(());
        }
        if form_code == self.cfg.fm_str {
            let s = self.peek_str()?;
            self.push_str(s);
            return Ok(());
        }

        // Original CD_COPY only handles scalar INT/STR forms.
        self.trace_unknown_form(form_code, "exec_copy");
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Command/Property dispatch bridging
    // ---------------------------------------------------------------------

    fn canonical_runtime_form_id(&self, form_id: u32) -> u32 {
        let ids = &self.ctx.ids;

        // EXCALL forwards its private STAGE as (global STAGE ^ 0x4000).
        // Dispatch it through the normal STAGE handler while keeping the raw
        // owner in ctx.vm_call.element[0], which stage::dispatch uses to pick
        // the correct storage container.
        if crate::runtime::forms::stage::is_stage_form_id(&self.ctx, form_id as i32) {
            return constants::global_form::STAGE_ALT;
        }
        if constants::matches_form_id(form_id, ids.form_global_mov, constants::global_form::MOV) {
            return constants::global_form::MOV;
        }
        if constants::matches_form_id(form_id, ids.form_global_bgm, constants::global_form::BGM) {
            return constants::global_form::BGM;
        }
        if constants::matches_form_id(
            form_id,
            ids.form_global_bgm_table,
            constants::global_form::BGMTABLE,
        ) {
            return constants::global_form::BGMTABLE;
        }
        if constants::matches_form_id(form_id, ids.form_global_math, constants::global_form::MATH) {
            return constants::global_form::MATH;
        }
        if constants::matches_form_id(form_id, ids.form_global_pcm, constants::global_form::PCM) {
            return constants::global_form::PCM;
        }
        if constants::matches_form_id(
            form_id,
            ids.form_global_pcmch,
            constants::global_form::PCMCH,
        ) {
            return constants::global_form::PCMCH;
        }
        if constants::matches_form_id(form_id, ids.form_global_se, constants::global_form::SE) {
            return constants::global_form::SE;
        }
        if constants::matches_form_id(
            form_id,
            ids.form_global_pcm_event,
            constants::global_form::PCMEVENT,
        ) {
            return constants::global_form::PCMEVENT;
        }
        if constants::matches_form_id(
            form_id,
            ids.form_global_excall,
            constants::global_form::EXCALL,
        ) {
            return constants::global_form::EXCALL;
        }
        if constants::matches_form_id(
            form_id,
            ids.form_global_screen,
            constants::global_form::SCREEN,
        ) {
            return constants::global_form::SCREEN;
        }
        if constants::matches_form_id(
            form_id,
            ids.form_global_msgbk,
            constants::global_form::MSGBK,
        ) {
            return constants::global_form::MSGBK;
        }
        if constants::matches_form_id(
            form_id,
            ids.form_global_koe_st,
            constants::global_form::KOE_ST,
        ) {
            return constants::global_form::KOE_ST;
        }
        if constants::matches_form_id(form_id, ids.form_global_key, constants::global_form::KEY) {
            return constants::global_form::KEY;
        }
        if form_id == constants::global_form::COUNTER {
            return constants::global_form::COUNTER;
        }
        if constants::matches_form_id(
            form_id,
            ids.form_global_frame_action,
            constants::global_form::FRAME_ACTION,
        ) {
            return constants::global_form::FRAME_ACTION;
        }
        if form_id == constants::global_form::TIMEWAIT {
            return constants::global_form::TIMEWAIT;
        }
        if form_id == constants::global_form::TIMEWAIT_KEY {
            return constants::global_form::TIMEWAIT_KEY;
        }

        form_id
    }


    #[inline(always)]
    fn sg_mwnd_object_trace_enabled(&self) -> bool {
        self.runtime_options.sg_debug
    }

    fn sg_mwnd_object_trace_emit(&self, msg: impl std::fmt::Display) {
        eprintln!("[SG_DEBUG][MWND_OBJECT_TRACE][VM] {}", msg);
    }

    fn sg_mwnd_chain_interesting(elm: &[i32]) -> bool {
        elm.iter().any(|v| {
            *v == crate::runtime::forms::codes::STAGE_ELM_MWND
                || *v == crate::runtime::forms::codes::STAGE_ELM_BTNSELITEM
                || *v == crate::runtime::forms::codes::elm_value::MWND_OBJECT
                || *v == crate::runtime::forms::codes::elm_value::MWND_BUTTON
                || *v == crate::runtime::forms::codes::elm_value::MWND_FACE
                || *v == crate::runtime::forms::codes::ELM_BTNSELITEM_OBJECT
                || *v == crate::runtime::forms::codes::elm_value::OBJECT_CHILD
                || *v == crate::runtime::forms::codes::elm_value::OBJECT_CREATE
                || *v == crate::runtime::forms::codes::elm_value::OBJECT_CREATE_RECT
                || *v == crate::runtime::forms::codes::elm_value::OBJECT_CREATE_STRING
                || *v == crate::runtime::forms::codes::elm_value::OBJECT_FRAME_ACTION
                || *v == crate::runtime::forms::codes::elm_value::OBJECT_FRAME_ACTION_CH
        })
    }

    fn is_global_indexed_list_head(&self, head: i32) -> bool {
        if head < 0 {
            return false;
        }
        let head = head as u32;
        crate::runtime::constants::global_form::INT_LIST_FORMS.contains(&head)
            || crate::runtime::constants::global_form::STR_LIST_FORMS.contains(&head)
    }

    fn is_global_indexed_list_chain(&self, elm: &[i32]) -> bool {
        if elm.len() < 3 || !self.is_global_indexed_list_head(elm[0]) {
            return false;
        }
        elm[1] == self.ctx.ids.elm_array || elm[1] == crate::runtime::forms::codes::ELM_ARRAY
    }

    fn current_object_chain_has_child_index(&self, child_idx: i32) -> bool {
        if child_idx < 0 {
            return false;
        }
        let Some(chain) = self.ctx.globals.current_object_chain.as_ref() else {
            return false;
        };
        let Some(raw_stage_form) = chain.first().copied() else {
            return false;
        };
        let stage_form = raw_stage_form;
        if !crate::runtime::forms::stage::is_stage_form_id(&self.ctx, stage_form) {
            return false;
        }
        let stage_form = crate::runtime::forms::stage::stage_storage_form_id(&self.ctx, stage_form);
        let elm_array = if self.ctx.ids.elm_array != 0 {
            self.ctx.ids.elm_array
        } else {
            crate::runtime::forms::codes::ELM_ARRAY
        };
        let stage_object = if self.ctx.ids.stage_elm_object != 0 {
            self.ctx.ids.stage_elm_object
        } else {
            crate::runtime::forms::codes::STAGE_ELM_OBJECT
        };
        if chain.len() < 6
            || crate::runtime::forms::stage::stage_storage_form_id(&self.ctx, chain[0]) != stage_form
            || chain[1] != elm_array
            || chain[2] < 0
        {
            return false;
        }

        let stage_idx = chain[2] as i64;
        let Some(stage_state) = self.ctx.globals.stage_forms.get(&stage_form) else {
            return false;
        };

        fn descend_child_chain<'a>(
            mut obj: &'a crate::runtime::globals::ObjectState,
            chain: &[i32],
            mut pos: usize,
            elm_array: i32,
        ) -> Option<&'a crate::runtime::globals::ObjectState> {
            let object_child = crate::runtime::forms::codes::elm_value::OBJECT_CHILD;
            while pos + 2 < chain.len() {
                if chain[pos] == object_child && chain[pos + 1] == elm_array && chain[pos + 2] >= 0 {
                    let idx = chain[pos + 2] as usize;
                    obj = obj.runtime.child_objects.get(idx)?;
                    pos += 3;
                } else {
                    break;
                }
            }
            Some(obj)
        }

        let current_obj = (|| -> Option<&crate::runtime::globals::ObjectState> {
            if chain[3] == stage_object {
                if chain[4] != elm_array || chain[5] < 0 {
                    return None;
                }
                let top_idx = chain[5] as usize;
                let list = stage_state.object_lists.get(&stage_idx)?;
                let obj = list.get(top_idx)?;
                descend_child_chain(obj, chain, 6, elm_array)
            } else if chain[3] == crate::runtime::forms::codes::STAGE_ELM_MWND {
                if chain.len() < 9
                    || chain[4] != elm_array
                    || chain[5] < 0
                    || chain[7] != elm_array
                    || chain[8] < 0
                {
                    return None;
                }
                let mwnd_idx = chain[5] as usize;
                let selector = chain[6];
                let obj_idx = chain[8] as usize;
                let mwnds = stage_state.mwnd_lists.get(&stage_idx)?;
                let mwnd = mwnds.get(mwnd_idx)?;
                let list = if selector == constants::MWND_BUTTON {
                    &mwnd.button_list
                } else if selector == constants::MWND_FACE {
                    &mwnd.face_list
                } else if selector == constants::MWND_OBJECT {
                    &mwnd.object_list
                } else {
                    return None;
                };
                let obj = list.get(obj_idx)?;
                descend_child_chain(obj, chain, 9, elm_array)
            } else if chain[3] == crate::runtime::forms::codes::STAGE_ELM_BTNSELITEM {
                if chain.len() < 9
                    || chain[4] != elm_array
                    || chain[5] < 0
                    || chain[7] != elm_array
                    || chain[8] < 0
                {
                    return None;
                }
                if chain[6] != crate::runtime::forms::codes::ELM_BTNSELITEM_OBJECT {
                    return None;
                }
                let item_idx = chain[5] as usize;
                let obj_idx = chain[8] as usize;
                let items = stage_state.btnselitem_lists.get(&stage_idx)?;
                let item = items.get(item_idx)?;
                let obj = item.object_list.get(obj_idx)?;
                descend_child_chain(obj, chain, 9, elm_array)
            } else {
                None
            }
        })();

        let Some(current_obj) = current_obj else {
            return false;
        };
        (child_idx as usize) < current_obj.runtime.child_objects.len()
    }

    fn object_array_property_op(&self, op: i32) -> bool {
        let ids = &self.ctx.ids;
        op == crate::runtime::forms::codes::elm_value::OBJECT_CHILD
            || (ids.obj_x_rep != 0 && op == ids.obj_x_rep)
            || (ids.obj_y_rep != 0 && op == ids.obj_y_rep)
            || (ids.obj_z_rep != 0 && op == ids.obj_z_rep)
            || (ids.obj_tr_rep != 0 && op == ids.obj_tr_rep)
            || (ids.obj_f != 0 && op == ids.obj_f)
            || (ids.obj_frame_action_ch != 0 && op == ids.obj_frame_action_ch)
    }

    fn is_current_object_child_tail(&self, elm: &[i32]) -> bool {
        if elm.len() < 2 {
            return false;
        }
        if elm[0] < 0 {
            return false;
        }
        if elm[1] != self.ctx.ids.elm_array && elm[1] != crate::runtime::forms::codes::ELM_ARRAY {
            return false;
        }
        if self.object_array_property_op(elm[0]) {
            return false;
        }
        if elm.len() == 2 {
            return true;
        }
        if self.object_array_property_op(elm[2]) {
            return elm[2] == crate::runtime::forms::codes::elm_value::OBJECT_CHILD;
        }
        elm[2] == self.ctx.ids.elm_array
            || elm[2] == crate::runtime::forms::codes::ELM_ARRAY
            || elm[2] == crate::runtime::forms::codes::ELM_UP
            || self.compact_object_op_allowed(elm[2])
    }

    fn global_indexed_list_must_dispatch_direct(&self, elm: &[i32]) -> bool {
        // A small flag index can also look like a compact object property.
        // Only prefer that shorthand when its parent object actually has the
        // requested child; otherwise G/Z accesses must reach the saved lists.
        self.is_global_indexed_list_chain(elm)
            && !(self.is_current_object_child_tail(elm)
                && self.current_object_has_child_index(elm[0]))
    }

    fn dispatch_global_indexed_list_property_direct(&mut self, elm: &[i32]) -> Result<bool> {
        if !self.global_indexed_list_must_dispatch_direct(elm) {
            return Ok(false);
        }
        let form_id = elm[0] as u32;
        self.ctx.vm_call = Some(runtime::VmCallMeta {
            element: elm.to_vec(),
            al_id: 0,
            ret_form: self.cfg.fm_int as i64,
        });
        if !runtime::dispatch_form_code(&mut self.ctx, form_id, &[])? {
            self.ctx.vm_call = None;
            bail!("unhandled global indexed-list property chain {:?}", elm);
        }
        self.ctx.vm_call = None;
        if let Some(v) = self.ctx.pop() {
            self.push_return_value_raw(v);
        } else {
            bail!("global indexed-list property returned no value: {:?}", elm);
        }
        Ok(true)
    }

    fn dispatch_global_indexed_list_assign_direct(&mut self, elm: &[i32], al_id: i32, rhs: Value) -> Result<bool> {
        if !self.global_indexed_list_must_dispatch_direct(elm) {
            return Ok(false);
        }
        let form_id = elm[0] as u32;
        let args: Vec<Value> = vec![rhs];
        self.ctx.vm_call = Some(runtime::VmCallMeta {
            element: elm.to_vec(),
            al_id: al_id as i64,
            ret_form: 0,
        });
        if !runtime::dispatch_form_code(&mut self.ctx, form_id, &args)? {
            self.ctx.vm_call = None;
            bail!("unhandled global indexed-list assignment chain {:?}", elm);
        }
        self.ctx.vm_call = None;
        self.ctx.stack.clear();
        self.drain_pending_frame_action_finishes()?;
        Ok(true)
    }

    fn dispatch_global_indexed_list_command_direct(
        &mut self,
        elm: &[i32],
        al_id: i32,
        ret_form: i32,
        args: &mut Vec<Value>,
    ) -> Result<bool> {
        if !self.global_indexed_list_must_dispatch_direct(elm) {
            return Ok(false);
        }
        let form_id = elm[0] as u32;
        self.ctx.vm_call = Some(runtime::VmCallMeta {
            element: elm.to_vec(),
            al_id: al_id as i64,
            ret_form: ret_form as i64,
        });
        if !runtime::dispatch_form_code(&mut self.ctx, form_id, args)? {
            self.ctx.vm_call = None;
            bail!("unhandled global indexed-list command chain {:?}", elm);
        }
        self.ctx.vm_call = None;
        self.drain_pending_frame_action_finishes()?;
        if ret_form != self.cfg.fm_void {
            self.take_ctx_return(ret_form)?;
        } else {
            self.ctx.stack.clear();
        }
        Ok(true)
    }


    fn try_parent_slot_property(&mut self, elm: &[i32]) -> bool {
        if elm.len() != 3 || elm[1] != self.ctx.ids.elm_array || elm[2] <= 0 {
            return false;
        }
        // Siglus object-child shorthand reuses the same compact `[slot, ELM_ARRAY, parent]`
        // shape as generic parent-slot access. When we are already inside an object chain,
        // prefer the object child interpretation so title/menu patno updates like
        // `front.object[0].[29] = ...` continue to drive the actual child objects instead
        // of disappearing into the generic parent-form property bags.
        if self.ctx.globals.current_object_chain.is_some() && self.compact_object_op_allowed(elm[0])
        {
            return false;
        }
        let parent_form = elm[2] as u32;
        let slot = elm[0];
        let ret_form = self
            .ctx
            .vm_call
            .as_ref()
            .map(|m| m.ret_form)
            .unwrap_or(self.cfg.fm_int as i64);
        if ret_form == self.cfg.fm_str as i64 {
            let value = self
                .ctx
                .globals
                .str_props
                .get(&parent_form)
                .and_then(|m| m.get(&slot))
                .cloned()
                .unwrap_or_default();
            self.push_str(value);
        } else if let Some(value) = self
            .ctx
            .globals
            .str_props
            .get(&parent_form)
            .and_then(|m| m.get(&slot))
            .cloned()
        {
            self.push_str(value);
        } else {
            let value = self
                .ctx
                .globals
                .int_props
                .get(&parent_form)
                .and_then(|m| m.get(&slot).copied())
                .unwrap_or(0);
            self.push_int(value as i32);
        }
        true
    }

    fn compact_object_op_allowed(&self, op: i32) -> bool {
        op >= 0 && op <= 187
    }

    fn compact_object_op_allowed_for_element(
        &self,
        elm: &[i32],
        allow_ambiguous_single_token_object_op: bool,
    ) -> bool {
        let Some(op) = elm.first().copied() else {
            return false;
        };
        if elm.len() == 1 && !allow_ambiguous_single_token_object_op {
            return false;
        }
        self.compact_object_op_allowed(op)
    }

    fn current_object_has_child_index(&self, child_idx: i32) -> bool {
        if self.current_object_chain_has_child_index(child_idx) {
            return true;
        }
        if child_idx < 0 {
            return false;
        }
        let Some((stage_idx, obj_idx)) = self.ctx.globals.current_stage_object else {
            return false;
        };
        let stage_form = self
            .ctx
            .globals
            .current_object_chain
            .as_ref()
            .and_then(|chain| chain.first().copied())
            .filter(|form| crate::runtime::forms::stage::is_stage_form_id(&self.ctx, *form))
            .map(|form| crate::runtime::forms::stage::stage_storage_form_id(&self.ctx, form))
            .unwrap_or_else(|| crate::runtime::forms::stage::current_stage_form_id(&self.ctx));
        let Some(stage_state) = self.ctx.globals.stage_forms.get(&stage_form) else {
            return false;
        };
        let Some(list) = stage_state.object_lists.get(&stage_idx) else {
            return false;
        };
        let Some(obj) = list.get(obj_idx) else {
            return false;
        };
        (child_idx as usize) < obj.runtime.child_objects.len()
    }

    fn try_compact_object_chain(
        &self,
        elm: &[i32],
        allow_ambiguous_single_token_object_op: bool,
    ) -> Option<Vec<i32>> {
        if elm.is_empty() {
            return None;
        }

        let op = elm[0];
        if !self.compact_object_op_allowed_for_element(
            elm,
            allow_ambiguous_single_token_object_op,
        ) {
            return None;
        }

        let elm_array = if self.ctx.ids.elm_array != 0 {
            self.ctx.ids.elm_array
        } else {
            crate::runtime::forms::codes::ELM_ARRAY
        };
        let stage_object = if self.ctx.ids.stage_elm_object != 0 {
            self.ctx.ids.stage_elm_object
        } else {
            crate::runtime::forms::codes::STAGE_ELM_OBJECT
        };

        let looks_like_absolute_stage_alias_object = elm.len() >= 4
            && constants::is_stage_global_form(elm[0] as u32, self.ctx.ids.form_global_stage)
            && elm[1] == stage_object
            && (elm[2] == elm_array || elm[2] == crate::runtime::forms::codes::ELM_ARRAY);

        if looks_like_absolute_stage_alias_object {
            return None;
        }

        // EXCALL.STAGE[index] starts with [65, 0, ARRAY, index], which also
        // matches the compact OBJECT.Z_EVE layout. Preserve the explicit
        // EXCALL chain so menu objects and button groups use its private stage.
        if elm.len() >= 4
            && constants::matches_form_id(
                elm[0] as u32,
                self.ctx.ids.form_global_excall,
                constants::global_form::EXCALL,
            )
            && elm[1] == crate::runtime::forms::codes::ELM_EXCALL_STAGE
            && (elm[2] == elm_array || elm[2] == crate::runtime::forms::codes::ELM_ARRAY)
        {
            return None;
        }

        // Original command dispatch receives the complete element chain.  The
        // only compact form we keep for an ambient object context is the
        // explicit child shorthand used after an already-resolved OBJECT.  Do
        // not append arbitrary OBJECT op ids to current_object_chain here:
        // many unrelated forms share the same numeric element values.
        if let Some(prefix) = &self.ctx.globals.current_object_chain {
            if self.is_current_object_child_tail(elm) && self.current_object_has_child_index(elm[0]) {
                let mut synthetic = prefix.clone();
                synthetic.push(crate::runtime::forms::codes::elm_value::OBJECT_CHILD);
                synthetic.push(elm_array);
                synthetic.push(elm[0]);
                if elm.len() > 2 {
                    if elm[2] == crate::runtime::forms::codes::elm_value::OBJECT_CHILD {
                        synthetic.extend_from_slice(&elm[3..]);
                    } else {
                        synthetic.extend_from_slice(&elm[2..]);
                    }
                }
                if self.sg_mwnd_object_trace_enabled()
                    && (Self::sg_mwnd_chain_interesting(elm)
                        || Self::sg_mwnd_chain_interesting(&synthetic))
                {
                    eprintln!(
                        "[SG_DEBUG][MWND_OBJECT_TRACE][VM] try_compact child-shorthand elm={:?} prefix={:?} synthetic={:?}",
                        elm,
                        prefix,
                        synthetic
                    );
                }
                return Some(synthetic);
            }
        }

        // Explicit compact absolute form: [object_op, stage_no, ARRAY, obj_no, ...]
        // This still carries both the stage and object index, so it is not an
        // ambient-context guess.
        if elm.len() >= 4
            && elm[1] >= 0
            && (elm[2] == elm_array || elm[2] == crate::runtime::forms::codes::ELM_ARRAY)
            && elm[3] >= 0
        {
            let stage_idx = elm[1];
            if !(0..3).contains(&stage_idx) {
                return None;
            }
            let stage_form = crate::runtime::forms::stage::current_stage_form_id(&self.ctx) as i32;
            let mut synthetic = vec![
                stage_form,
                elm_array,
                stage_idx,
                stage_object,
                elm_array,
                elm[3],
                op,
            ];
            if elm.len() > 4 {
                synthetic.extend_from_slice(&elm[4..]);
            }
            if self.sg_mwnd_object_trace_enabled()
                && (Self::sg_mwnd_chain_interesting(elm)
                    || Self::sg_mwnd_chain_interesting(&synthetic))
            {
                eprintln!(
                    "[SG_DEBUG][MWND_OBJECT_TRACE][VM] try_compact absolute elm={:?} synthetic={:?}",
                    elm,
                    synthetic
                );
            }
            return Some(synthetic);
        }

        None
    }

    fn try_parent_slot_assign(&mut self, elm: &[i32], rhs: &Value) -> bool {
        if elm.len() != 3 || elm[1] != self.ctx.ids.elm_array || elm[2] <= 0 {
            return false;
        }
        // See try_parent_slot_property(): inside object chains this compact syntax is used for
        // object child operations, not generic parent-form slots.
        if self.ctx.globals.current_object_chain.is_some() && self.compact_object_op_allowed(elm[0])
        {
            return false;
        }
        let parent_form = elm[2] as u32;
        let slot = elm[0];
        match rhs {
            Value::Str(s) => {
                self.ctx
                    .globals
                    .str_props
                    .entry(parent_form)
                    .or_default()
                    .insert(slot, s.clone());
            }
            Value::Int(n) => {
                self.ctx
                    .globals
                    .int_props
                    .entry(parent_form)
                    .or_default()
                    .insert(slot, *n);
            }
            Value::NamedArg { value, .. } => return self.try_parent_slot_assign(elm, value),
            _ => return false,
        }
        true
    }

    fn exec_property(&mut self, mut elm: Vec<i32>) -> Result<()> {
        if self.runtime_options.title_chain_trace
            && self.current_scene_name.as_deref() == Some("sys10_tt01")
            && matches!(elm.first().copied(), Some(83 | 84 | 24 | 25))
        {
            eprintln!(
                "[SG_TITLE_CHAIN_TRACE] line={} elm={:?} current_object_chain={:?} current_stage_object={:?}",
                self.current_line_no,
                elm,
                self.ctx.globals.current_object_chain,
                self.ctx.globals.current_stage_object
            );
        }
        vm_trace!(self, None, format!("exec_property enter elm={:?}", elm));
        if elm.is_empty() {
            self.push_int(0);
            return Ok(());
        }
        // Call-local properties (declared by CD_DEC_PROP / populated by CD_ARG).
        if self.exec_call_property(&elm)? {
            vm_trace!(self,
                None,
                format!("exec_property handled by call-property elm={:?}", elm),
            );
            return Ok(());
        }

        let head = elm[0];
        let head_owner = elm_code::owner(head);
        if head_owner == elm_code::ELM_OWNER_CALL_PROP {
            let current_idx = self
                .current_call_frame_index()
                .ok_or_else(|| anyhow!("call stack underflow"))?;
            let call_prop_id = elm_code::code(head) as i32;
            let prop_idx = self
                .find_call_prop_index_in_frame(current_idx, call_prop_id)
                .ok_or_else(|| {
                    anyhow!("missing direct CALL_PROP id={} for {:?}", call_prop_id, elm)
                })?;
            let prop = self.call_stack[current_idx].user_props[prop_idx].clone();
            if let Some(composed) = self.compose_call_prop_tail(&prop, &elm[1..]) {
                self.exec_property(composed)?;
                vm_trace!(self,
                    None,
                    format!("exec_property direct CALL_PROP composed elm={:?}", elm),
                );
                return Ok(());
            }
            self.push_call_prop_result(&prop, &elm[1..], &elm)?;
            vm_trace!(self,
                None,
                format!("exec_property direct CALL_PROP elm={:?}", elm),
            );
            return Ok(());
        }

        if head_owner == elm_code::ELM_OWNER_USER_PROP {
            let prop_id = elm_code::code(head);
            let cell = self
                .user_props
                .get(&prop_id)
                .cloned()
                .unwrap_or_else(|| self.default_user_prop_cell(prop_id));
            let array_idx = self.extract_array_index(&elm);
            self.trace_cf_condition_user_prop_read(
                self.stream.get_prg_cntr(),
                prop_id,
                array_idx,
                &cell,
                &elm,
            );
            self.push_user_prop_cell_result(&cell, &elm[1..], &elm)?;
            vm_trace!(self,
                None,
                format!("exec_property direct USER_PROP elm={:?}", elm),
            );
            return Ok(());
        }

        if head_owner != elm_code::ELM_OWNER_FORM {
            bail!(
                "unsupported property owner {} for element {:?}",
                head_owner,
                elm
            );
        }

        if self.dispatch_global_indexed_list_property_direct(&elm)? {
            vm_trace!(self, None, format!("exec_property handled by global indexed-list elm={:?}", elm));
            return Ok(());
        }

        if self.try_parent_slot_property(&elm) {
            vm_trace!(self,
                None,
                format!("exec_property handled by parent-slot elm={:?}", elm),
            );
            return Ok(());
        }
        if let Some(synthetic) = self.try_compact_object_chain(&elm, false) {
            vm_trace!(self,
                None,
                format!(
                    "exec_property compact-object elm={:?} synthetic={:?}",
                    elm, synthetic
                ),
            );
            self.ctx.vm_call = Some(runtime::VmCallMeta {
                element: synthetic.clone(),
                al_id: 0,
                ret_form: self.cfg.fm_int as i64,
            });
            let form_id = self.canonical_runtime_form_id(synthetic[0] as u32);
            if !runtime::dispatch_form_code(&mut self.ctx, form_id, &[])? {
                self.ctx.vm_call = None;
                bail!(
                    "unhandled compact object property chain {:?} -> {:?}",
                    elm,
                    synthetic
                );
            }
            self.ctx.vm_call = None;
            self.update_compact_context_from_object_dispatch_chain(&synthetic);
            if let Some(v) = self.ctx.pop() {
                self.push_return_value_raw(v);
            } else {
                bail!("compact object property chain returned no value: {:?}", elm);
            }
            return Ok(());
        }

        let form_id = self.canonical_runtime_form_id(head as u32);
        let args: Vec<Value> = Vec::new();
        self.ctx.vm_call = Some(runtime::VmCallMeta {
            element: elm.clone(),
            al_id: 0,
            ret_form: self.cfg.fm_int as i64,
        });

        vm_trace!(self,
            None,
            format!("exec_property dispatch form_id={} elm={:?}", form_id, elm),
        );
        if !runtime::dispatch_form_code(&mut self.ctx, form_id, &args)? {
            self.ctx.vm_call = None;
            bail!("unhandled form property chain {:?}", elm);
        }

        self.ctx.vm_call = None;
        if let Some(v) = self.ctx.pop() {
            self.push_return_value_raw(v);
        } else {
            bail!("property chain returned no value: {:?}", elm);
        }

        Ok(())
    }

    fn exec_assign(&mut self, elm: Vec<i32>, al_id: i32, rhs: Value) -> Result<()> {
        if elm.is_empty() {
            return Ok(());
        }

        self.trace_cgm_coord_assign(&elm, &rhs);

        // Call-local property assignment.
        if self.exec_call_assign(&elm, al_id, rhs.clone())? {
            return Ok(());
        }

        let head = elm[0];
        let head_owner = elm_code::owner(head);
        if head_owner == elm_code::ELM_OWNER_CALL_PROP {
            let current_idx = self
                .current_call_frame_index()
                .ok_or_else(|| anyhow!("call stack underflow"))?;
            let call_prop_id = elm_code::code(head) as i32;
            let prop_idx = self
                .find_call_prop_index_in_frame(current_idx, call_prop_id)
                .ok_or_else(|| {
                    anyhow!(
                        "missing direct CALL_PROP assign id={} for {:?}",
                        call_prop_id,
                        elm
                    )
                })?;
            let prop_for_compose = self.call_stack[current_idx].user_props[prop_idx].clone();
            if let Some(composed) = self.compose_call_prop_tail(&prop_for_compose, &elm[1..]) {
                self.exec_assign(composed, al_id, rhs)?;
                return Ok(());
            }
            let frame = self
                .call_stack
                .get_mut(current_idx)
                .ok_or_else(|| anyhow!("call stack underflow"))?;
            let prop = frame.user_props.get_mut(prop_idx).ok_or_else(|| {
                anyhow!("missing direct CALL_PROP slot assign id={}", call_prop_id)
            })?;
            Self::assign_call_prop_result(prop, &elm[1..], rhs)?;
            return Ok(());
        }

        if head_owner == elm_code::ELM_OWNER_USER_PROP {
            let prop_id = elm_code::code(head);
            if elm.len() >= 3 && self.call_array_marker(elm[1]) && elm[2] < 0 {
                return Ok(());
            }
            let array_idx = self.extract_array_index(&elm);
            let old_cell = self.user_props.get(&prop_id).cloned();
            self.assign_user_prop(prop_id, array_idx, rhs.clone());
            let new_cell = self.user_props.get(&prop_id);
            self.trace_cf_condition_user_prop_assign(
                self.stream.get_prg_cntr(),
                prop_id,
                array_idx,
                old_cell.as_ref(),
                new_cell,
                &rhs,
                &elm,
            );
            return Ok(());
        }

        if head_owner != elm_code::ELM_OWNER_FORM {
            bail!(
                "unsupported assignment owner {} for element {:?}",
                head_owner,
                elm
            );
        }

        if self.dispatch_global_indexed_list_assign_direct(&elm, al_id, rhs.clone())? {
            return Ok(());
        }

        if self.try_parent_slot_assign(&elm, &rhs) {
            return Ok(());
        }
        if let Some(synthetic) = self.try_compact_object_chain(&elm, true) {
            vm_trace!(self,
                None,
                format!(
                    "exec_assign compact-object elm={:?} synthetic={:?} al_id={} rhs={:?}",
                    elm, synthetic, al_id, rhs
                ),
            );
            let args: Vec<Value> = vec![rhs];
            self.ctx.vm_call = Some(runtime::VmCallMeta {
                element: synthetic.clone(),
                al_id: al_id as i64,
                ret_form: 0,
            });
            let form_id = self.canonical_runtime_form_id(synthetic[0] as u32);
            if !runtime::dispatch_form_code(&mut self.ctx, form_id, &args)? {
                self.ctx.vm_call = None;
                bail!(
                    "unhandled compact object assignment chain {:?} -> {:?}",
                    elm,
                    synthetic
                );
            }
            self.ctx.vm_call = None;
            self.update_compact_context_from_object_dispatch_chain(&synthetic);
            self.ctx.stack.clear();
            self.drain_pending_frame_action_finishes()?;
            return Ok(());
        }

        let form_id = self.canonical_runtime_form_id(head as u32);
        vm_trace!(self,
            None,
            format!(
                "exec_assign dispatch form_id={} elm={:?} al_id={} rhs={:?}",
                form_id, elm, al_id, rhs
            ),
        );
        let args: Vec<Value> = vec![rhs];
        if self.vm_trace_config.commands_enabled {
            eprintln!(
                "[vm form assign] form={} al_id={} elm={:?} rhs={:?}",
                form_id,
                al_id,
                elm,
                args.first()
            );
        }
        self.ctx.vm_call = Some(runtime::VmCallMeta {
            element: elm.clone(),
            al_id: al_id as i64,
            ret_form: 0,
        });

        if !runtime::dispatch_form_code(&mut self.ctx, form_id, &args)? {
            self.ctx.vm_call = None;
            bail!("unhandled form assignment chain {:?}", elm);
        }
        self.ctx.vm_call = None;
        self.ctx.stack.clear();
        self.drain_pending_frame_action_finishes()?;
        Ok(())
    }

    fn dispatch_owner_command(
        &mut self,
        owner: u8,
        raw_head: i32,
        ret_form: i32,
        args: &[Value],
    ) -> Result<bool> {
        let cmd_no = elm_code::code(raw_head) as usize;

        if owner == elm_code::ELM_OWNER_CALL_CMD {
            // The original headers describe ELM_OWNER_CALL_CMD as a call command
            // that does not exist, and cmd_global.cpp does not dispatch it. Do not
            // reinterpret malformed bytecode as a command name and guess BG/CHR/
            // FADE behavior.
            let name = self
                .call_cmd_names
                .get(&(cmd_no as u32))
                .map(String::as_str)
                .unwrap_or("<unknown>");
            bail!(
                "invalid CALL_CMD owner: raw_head={} cmd_no={} name={}",
                raw_head,
                cmd_no,
                name
            );
        }

        if owner != elm_code::ELM_OWNER_USER_CMD {
            return Ok(false);
        }

        // C++ tnm_command_proc_user_cmd() passes the encoded user-command ID to
        // tnm_scene_proc_call_user_cmd(). C_tnm_scene_lexer::jump_to_user_cmd()
        // resolves pack-level include commands through Scene.pck.inc_cmds and
        // resolves only later IDs through the current scene's local command table.
        let requested_scene_no = self
            .current_scene_no
            .ok_or_else(|| anyhow!("USER_CMD executed without a current scene"))?;
        let command = self.resolve_user_command_by_id(requested_scene_no, cmd_no)?;
        vm_trace!(self,
            None,
            format!(
                "USER_CMD decoded name={} target_scene={} offset=0x{:x} include={} ret_form={} args={:?}",
                command.name.as_str(),
                command.target_scene_no,
                command.target_offset,
                command.include_command,
                ret_form,
                args
            ),
        );
        sg_omv_trace!(self,
            "USER_CMD enter name={} raw_head={} cmd_no={} target_scene={} offset=0x{:x} include={} ret_form={} argc={} current_scene={:?} current_pc=0x{:x}",
            command.name.as_str(),
            raw_head,
            command.encoded_no,
            command.target_scene_no,
            command.target_offset,
            command.include_command,
            ret_form,
            args.len(),
            self.current_scene_no,
            self.stream.get_prg_cntr()
        );
        self.enter_resolved_user_command(&command, ret_form, args, false, false)
    }

    fn command_consumes_read_flag_no(&self, elm: &[i32]) -> bool {
        fn global_consumes(op: i32) -> bool {
            matches!(
                op,
                crate::runtime::forms::codes::elm_value::GLOBAL_PRINT
                    | crate::runtime::forms::codes::elm_value::GLOBAL_SEL
                    | crate::runtime::forms::codes::elm_value::GLOBAL_SEL_CANCEL
                    | crate::runtime::forms::codes::elm_value::GLOBAL_SELMSG
                    | crate::runtime::forms::codes::elm_value::GLOBAL_SELMSG_CANCEL
                    | crate::runtime::forms::codes::elm_value::GLOBAL_SELBTN
                    | crate::runtime::forms::codes::elm_value::GLOBAL_SELBTN_CANCEL
                    | crate::runtime::forms::codes::elm_value::GLOBAL_SELBTN_START
                    | crate::runtime::forms::codes::elm_value::GLOBAL_KOE
                    | crate::runtime::forms::codes::elm_value::GLOBAL_KOE_PLAY_WAIT
                    | crate::runtime::forms::codes::elm_value::GLOBAL_KOE_PLAY_WAIT_KEY
            )
        }

        fn mwnd_consumes(op: i32) -> bool {
            matches!(
                op,
                crate::runtime::forms::codes::elm_value::MWND_PRINT
                    | crate::runtime::forms::codes::elm_value::MWND_SEL
                    | crate::runtime::forms::codes::elm_value::MWND_SEL_CANCEL
                    | crate::runtime::forms::codes::elm_value::MWND_SELMSG
                    | crate::runtime::forms::codes::elm_value::MWND_SELMSG_CANCEL
                    | crate::runtime::forms::codes::elm_value::MWND_KOE
                    | crate::runtime::forms::codes::elm_value::MWND_KOE_PLAY_WAIT
                    | crate::runtime::forms::codes::elm_value::MWND_KOE_PLAY_WAIT_KEY
            )
        }

        // C++ consumes the read-flag integer inside the concrete command
        // handler after CD_COMMAND has read the command metadata. The Rust VM
        // has to make the same decision from the actual element chain. Do not
        // assume the chain is exactly [FORM, OP]: commands can arrive through
        // object, mwnd, and global aliases, so scan every adjacent form/op pair.
        for pair in elm.windows(2) {
            let form_id = self.canonical_runtime_form_id(pair[0] as u32) as i32;
            let op = pair[1];
            if form_id == crate::runtime::forms::codes::FM_GLOBAL && global_consumes(op) {
                return true;
            }
            if form_id == crate::runtime::forms::codes::FM_MWND && mwnd_consumes(op) {
                return true;
            }
        }

        false
    }

    fn exec_command(
        &mut self,
        elm: Vec<i32>,
        al_id: i32,
        ret_form: i32,
        args: &mut Vec<Value>,
    ) -> Result<()> {
        if elm.is_empty() {
            self.push_default_for_ret(ret_form);
            return Ok(());
        }

        if self.exec_call_command(&elm, al_id, ret_form, args)? {
            return Ok(());
        }

        let raw_head = elm[0];
        let owner = elm_code::owner(raw_head);

        if owner == elm_code::ELM_OWNER_CALL_PROP {
            let current_idx = self
                .current_call_frame_index()
                .ok_or_else(|| anyhow!("call stack underflow"))?;
            let call_prop_id = elm_code::code(raw_head) as i32;
            let prop_idx = self
                .find_call_prop_index_in_frame(current_idx, call_prop_id)
                .ok_or_else(|| {
                    anyhow!("missing direct CALL_PROP command id={} for {:?}", call_prop_id, elm)
                })?;
            let prop = self.call_stack[current_idx].user_props[prop_idx].clone();
            if let Some(composed) = self.compose_call_prop_tail(&prop, &elm[1..]) {
                self.exec_command(composed, al_id, ret_form, args)?;
                return Ok(());
            }
            self.push_default_for_ret(ret_form);
            return Ok(());
        }

        if owner == elm_code::ELM_OWNER_USER_PROP {
            let prop_id = elm_code::code(raw_head);
            if self.exec_user_prop_list_command(
                prop_id,
                &elm[1..],
                al_id,
                ret_form,
                args,
            )? {
                return Ok(());
            }
            let cell = self
                .user_props
                .get(&prop_id)
                .cloned()
                .unwrap_or_else(|| self.default_user_prop_cell(prop_id));
            if let Some(composed) = self.compose_user_prop_tail(prop_id, &cell, &elm[1..]) {
                self.exec_command(composed, al_id, ret_form, args)?;
                return Ok(());
            }
            if cell.form == self.cfg.fm_str && elm.len() == 2 {
                self.call_prop_eval_str_op(&cell.str_value, elm[1], args, al_id)?;
                return Ok(());
            }
            self.push_default_for_ret(ret_form);
            return Ok(());
        }

        match owner {
            o if o == elm_code::ELM_OWNER_FORM => {
                if self.dispatch_global_indexed_list_command_direct(&elm, al_id, ret_form, args)? {
                    return Ok(());
                }
                if let Some(synthetic) = self.try_compact_object_chain(&elm, true) {
                    vm_trace!(self,
                        None,
                        format!(
                            "exec_command compact-object elm={:?} synthetic={:?} al_id={} ret_form={} args={:?}",
                            elm, synthetic, al_id, ret_form, args
                        ),
                    );
                    if self.sg_mwnd_object_trace_enabled()
                        && (Self::sg_mwnd_chain_interesting(&elm) || Self::sg_mwnd_chain_interesting(&synthetic))
                    {
                        self.sg_mwnd_object_trace_emit(format_args!(
                            "exec_command compact elm={:?} synthetic={:?} al_id={} ret_form={} args={:?} current_chain={:?} current_stage_object={:?}",
                            elm,
                            synthetic,
                            al_id,
                            ret_form,
                            args,
                            self.ctx.globals.current_object_chain,
                            self.ctx.globals.current_stage_object
                        ));
                    }
                    self.ctx.vm_call = Some(runtime::VmCallMeta {
                        element: synthetic.clone(),
                        al_id: al_id as i64,
                        ret_form: ret_form as i64,
                    });
                    let form_id = self.canonical_runtime_form_id(synthetic[0] as u32) as i32;
                    let op_id = if synthetic.len() >= 2 { synthetic[1] } else { al_id };
                    self.sg_omv_trace_command(
                        "compact",
                        &synthetic,
                        form_id,
                        op_id,
                        al_id,
                        ret_form,
                        args,
                    );
                    if !runtime::dispatch_form_code(&mut self.ctx, form_id as u32, args)? {
                        self.ctx.vm_call = None;
                        bail!(
                            "unhandled compact object command chain {:?} -> {:?}",
                            elm,
                            synthetic
                        );
                    }
                    self.ctx.vm_call = None;
                    self.update_compact_context_from_object_dispatch_chain(&synthetic);
                    self.drain_pending_frame_action_finishes()?;
                    if ret_form != self.cfg.fm_void {
                        if !self.ctx.stack.is_empty() {
                            self.take_ctx_return(ret_form)?;
                            return Ok(());
                        }
                        if self.ctx.wait_poll() {
                            if let Some(frame) = self.call_stack.last_mut() {
                                frame.delayed_ret_form = Some(ret_form);
                            } else {
                                self.delayed_ret_form = Some(ret_form);
                            }
                            return Ok(());
                        }
                    }
                    self.take_ctx_return(ret_form)?;
                    return Ok(());
                }

                let form_id = self.canonical_runtime_form_id(raw_head as u32) as i32;
                if self.exec_builtin_global_control(form_id, ret_form)? {
                    if ret_form != self.cfg.fm_void {
                        self.take_ctx_return(ret_form)?;
                    } else {
                        self.ctx.stack.clear();
                    }
                    return Ok(());
                }
                if self.exec_syscom_save_value_intlistref(&elm, form_id, ret_form, args)? {
                    return Ok(());
                }
                if self.exec_builtin_scene_form(&elm, form_id, al_id, ret_form, args)? {
                    return Ok(());
                }

                let op_id = if elm.len() >= 2 { elm[1] } else { al_id };
                self.ctx.vm_call = Some(runtime::VmCallMeta {
                    element: elm.clone(),
                    al_id: al_id as i64,
                    ret_form: ret_form as i64,
                });

                if self.vm_trace_config.commands_enabled {
                    let elm_tail = elm
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                    let args_dbg = args
                        .iter()
                        .map(|v| format!("{v:?}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    eprintln!(
                        "[vm form cmd] form={} op={} argc={} ret_form={} al_id={} elm=[{}] args=[{}]",
                        form_id,
                        op_id,
                        args.len(),
                        ret_form,
                        al_id,
                        elm_tail,
                        args_dbg
                    );
                }

                self.sg_omv_trace_command(
                    "dispatch",
                    &elm,
                    form_id,
                    op_id,
                    al_id,
                    ret_form,
                    args,
                );

                if !runtime::dispatch_form_code(&mut self.ctx, form_id as u32, args)? {
                    self.ctx.vm_call = None;
                    bail!("unhandled form command chain {:?}", elm);
                }
                self.ctx.vm_call = None;
                self.drain_pending_frame_action_finishes()?;
            }
            o if o == elm_code::ELM_OWNER_USER_CMD || o == elm_code::ELM_OWNER_CALL_CMD => {
                if self.vm_trace_config.commands_enabled {
                    let cmd_no = elm_code::code(raw_head);
                    let elm_tail = elm
                        .iter()
                        .map(|v| v.to_string())
                        .collect::<Vec<_>>()
                        .join(",");
                    let args_dbg = args
                        .iter()
                        .map(|v| format!("{v:?}"))
                        .collect::<Vec<_>>()
                        .join(", ");
                    eprintln!(
                        "[vm owner cmd] owner={} cmd_no={} argc={} ret_form={} al_id={} elm=[{}] args=[{}]",
                        owner,
                        cmd_no,
                        args.len(),
                        ret_form,
                        al_id,
                        elm_tail,
                        args_dbg
                    );
                }

                if !self.dispatch_owner_command(owner, raw_head, ret_form, args)? {
                    bail!("unhandled owner command chain {:?}", elm);
                }
                if owner == elm_code::ELM_OWNER_USER_CMD {
                    // USER_CMD has transferred control to the callee. Its return
                    // value will be materialized by CD_RETURN when that callee
                    // finishes, so CD_COMMAND must not consume ctx.stack now.
                    return Ok(());
                }
            }
            _ => {
                bail!("unsupported command owner {} for element {:?}", owner, elm);
            }
        }

        if ret_form != self.cfg.fm_void {
            if !self.ctx.stack.is_empty() {
                self.take_ctx_return(ret_form)?;
                return Ok(());
            }
            if self.ctx.wait_poll() {
                if let Some(frame) = self.call_stack.last_mut() {
                    frame.delayed_ret_form = Some(ret_form);
                } else {
                    self.delayed_ret_form = Some(ret_form);
                }
                return Ok(());
            }
        }

        self.take_ctx_return(ret_form)?;
        Ok(())
    }


    fn save_kind_to_original(kind: RuntimeSaveKind) -> Option<crate::original_save::SaveKind> {
        match kind {
            RuntimeSaveKind::Normal => Some(crate::original_save::SaveKind::Normal),
            RuntimeSaveKind::Quick => Some(crate::original_save::SaveKind::Quick),
            RuntimeSaveKind::End => Some(crate::original_save::SaveKind::End),
            RuntimeSaveKind::Inner => None,
        }
    }

    fn configured_runtime_save_count(&self, quick: bool) -> usize {
        let keys: [&str; 2] = if quick {
            ["#QUICK_SAVE.CNT", "QUICK_SAVE.CNT"]
        } else {
            ["#SAVE.CNT", "SAVE.CNT"]
        };
        let default_count = if quick { 3 } else { 10 };
        self.ctx
            .tables
            .gameexe
            .as_ref()
            .and_then(|cfg| keys.iter().find_map(|key| cfg.get_usize(*key)))
            .unwrap_or(default_count)
            .min(10000)
    }

    fn runtime_save_file_path(&self, kind: RuntimeSaveKind, index: usize) -> Option<std::path::PathBuf> {
        let save_kind = Self::save_kind_to_original(kind)?;
        let save_cnt = self.configured_runtime_save_count(false);
        let quick_cnt = self.configured_runtime_save_count(true);
        Some(crate::original_save::save_file_path_with_counts(
            &self.ctx.project_dir,
            save_cnt,
            quick_cnt,
            save_kind,
            index,
        ))
    }

    fn stamp_slot_with_local_time(slot: &mut crate::runtime::globals::SaveSlotState) {
        let now = crate::platform_time::local_time_fields();
        slot.exist = true;
        slot.year = now.year as i64;
        slot.month = now.month as i64;
        slot.day = now.day as i64;
        // SYSTEMTIME.wDayOfWeek uses 0..6 with Sunday = 0.
        slot.weekday = now.weekday_sunday0 as i64;
        slot.hour = now.hour as i64;
        slot.minute = now.minute as i64;
        slot.second = now.second as i64;
        slot.millisecond = now.millisecond as i64;
    }

    /// Build the slot record that ends up in the save file header and in the in-memory
    /// `save_slots` / `quick_save_slots` tables. Mirrors C++ `tnm_save_local_on_file`:
    /// timestamps come from `GetLocalTime` (i.e. "now"), while the textual fields
    /// (title / message / full_message / append_dir / append_name) come from the
    /// engine's m_local_save snapshot.
    ///
    /// Inner-save still pulls textual fields from live runtime state because the
    /// inner-save path here is the only consumer that doesn't go through SAVEPOINT.
    fn ensure_runtime_slot_for_save(&mut self, req: RuntimeSaveRequest) -> crate::runtime::globals::SaveSlotState {
        let mut slot = crate::runtime::globals::SaveSlotState::default();
        Self::stamp_slot_with_local_time(&mut slot);
        if let Some(snapshot) = self.ctx.local_save_snapshot.as_ref() {
            slot.title = snapshot.save_scene_title.clone();
            slot.message = snapshot.save_msg.clone();
            slot.full_message = if snapshot.save_full_msg.is_empty() {
                snapshot.save_msg.clone()
            } else {
                snapshot.save_full_msg.clone()
            };
            slot.append_dir = snapshot.append_dir.clone();
            slot.append_name = snapshot.append_name.clone();
        } else {
            // No snapshot exists (e.g. inner save before any SAVEPOINT). Fall back to
            // live runtime values so inner-save still records something meaningful.
            slot.title = self.ctx.globals.syscom.current_save_scene_title.clone();
            slot.message = self.ctx.globals.syscom.current_save_message.clone();
            slot.full_message = if self.ctx.globals.syscom.current_save_full_message.is_empty() {
                self.ctx.globals.syscom.current_save_message.clone()
            } else {
                self.ctx.globals.syscom.current_save_full_message.clone()
            };
            slot.append_dir = self.ctx.globals.append_dir.clone();
            slot.append_name = self.ctx.globals.append_name.clone();
        }

        match req.kind {
            RuntimeSaveKind::Normal => {
                if self.ctx.globals.syscom.save_slots.len() <= req.index {
                    self.ctx.globals.syscom.save_slots.resize_with(req.index + 1, Default::default);
                }
                self.ctx.globals.syscom.save_slots[req.index] = slot.clone();
            }
            RuntimeSaveKind::Quick => {
                if self.ctx.globals.syscom.quick_save_slots.len() <= req.index {
                    self.ctx.globals.syscom.quick_save_slots.resize_with(req.index + 1, Default::default);
                }
                self.ctx.globals.syscom.quick_save_slots[req.index] = slot.clone();
            }
            RuntimeSaveKind::End | RuntimeSaveKind::Inner => {}
        }
        slot
    }

    fn local_flag_count(&self) -> usize {
        if let Some(configured) = self
            .ctx
            .tables
            .gameexe
            .as_ref()
            .and_then(|cfg| cfg.get_usize("#FLAG.CNT").or_else(|| cfg.get_usize("FLAG.CNT")))
        {
            return configured.min(10000);
        }

        [
            codes::ELM_GLOBAL_A,
            codes::ELM_GLOBAL_B,
            codes::ELM_GLOBAL_C,
            codes::ELM_GLOBAL_D,
            codes::ELM_GLOBAL_E,
            codes::ELM_GLOBAL_F,
            codes::ELM_GLOBAL_X,
        ]
        .into_iter()
        .map(|elm| self.int_list_by_element(elm).len())
        .chain(std::iter::once(
            self.str_list_by_element(codes::ELM_GLOBAL_S).len(),
        ))
        .max()
        .unwrap_or(1000)
        .max(1000)
        .min(10000)
    }

    fn mwnd_waku_btn_count(&self) -> usize {
        self.ctx
            .tables
            .gameexe
            .as_ref()
            .and_then(|cfg| cfg.get_usize("#WAKU.BTN.CNT").or_else(|| cfg.get_usize("WAKU.BTN.CNT")))
            .unwrap_or(8)
            .min(256)
    }

    fn int_list_by_element(&self, elm: i32) -> &[i64] {
        self.ctx
            .globals
            .int_lists
            .get(&(elm as u32))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn str_list_by_element(&self, elm: i32) -> &[String] {
        self.ctx
            .globals
            .str_lists
            .get(&(elm as u32))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    fn build_cpp_local_data_pod(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(356);
        let script = &self.ctx.globals.script;
        let syscom = &self.ctx.globals.syscom;
        let push_i32 = |out: &mut Vec<u8>, v: i64| out.extend_from_slice(&(v as i32).to_le_bytes());
        let push_bool = |out: &mut Vec<u8>, v: bool| out.push(if v { 1 } else { 0 });

        push_i32(&mut out, script.cur_koe_no);
        push_i32(&mut out, script.cur_chr_no);
        push_i32(&mut out, script.cur_read_flag_scn_no);
        push_i32(&mut out, script.cur_read_flag_flag_no);
        push_i32(&mut out, script.cursor_no);

        push_bool(&mut out, syscom.syscom_menu_disable);
        push_bool(&mut out, script.hide_mwnd_disable);
        push_bool(&mut out, script.msg_back_disable);
        push_bool(&mut out, script.shortcut_disable);

        push_bool(&mut out, script.skip_disable);
        push_bool(&mut out, script.ctrl_disable);
        push_bool(&mut out, script.not_stop_skip_by_click);
        push_bool(&mut out, script.not_skip_msg_by_click);
        push_bool(&mut out, script.skip_unread_message);
        push_bool(&mut out, script.auto_mode_flag);
        while out.len() % 4 != 0 { out.push(0); }
        push_i32(&mut out, script.auto_mode_moji_wait);
        push_i32(&mut out, script.auto_mode_min_wait);
        push_i32(&mut out, script.auto_mode_moji_cnt);
        push_i32(&mut out, script.mouse_cursor_hide_onoff);
        push_i32(&mut out, script.mouse_cursor_hide_time);
        push_i32(&mut out, script.msg_back_save_cntr);

        push_i32(&mut out, script.msg_speed);
        push_bool(&mut out, script.msg_nowait);
        push_bool(&mut out, script.async_msg_mode);
        push_bool(&mut out, script.async_msg_mode_once);
        push_bool(&mut out, script.multi_msg_mode);
        push_bool(&mut out, script.skip_trigger);
        push_bool(&mut out, script.koe_dont_stop_on_flag);
        push_bool(&mut out, script.koe_dont_stop_off_flag);

        push_bool(&mut out, syscom.mwnd_btn_disable_all);
        push_bool(&mut out, syscom.mwnd_btn_touch_disable);
        push_bool(&mut out, script.mwnd_anime_on_flag);
        push_bool(&mut out, script.mwnd_anime_off_flag);
        push_bool(&mut out, script.mwnd_disp_off_flag);

        push_bool(&mut out, script.msg_back_off);
        push_bool(&mut out, script.msg_back_disp_off);
        while out.len() % 4 != 0 { out.push(0); }
        push_i32(&mut out, script.font_bold);
        push_i32(&mut out, script.font_shadow);

        push_bool(&mut out, script.cursor_disp_off);
        push_bool(&mut out, script.cursor_move_by_key_disable);
        for key in 0u16..=255u16 {
            push_bool(&mut out, script.key_disable.contains(&(key as u8)));
        }

        push_bool(&mut out, script.quake_stop_flag);
        push_bool(&mut out, script.emote_mouth_stop_flag);
        push_bool(&mut out, self.ctx.globals.cg_table_off);
        push_bool(&mut out, script.bgmfade_flag);
        push_bool(&mut out, script.dont_set_save_point);
        push_bool(&mut out, script.ignore_r_flag);
        push_bool(&mut out, script.wait_display_vsync_off_flag);

        push_bool(&mut out, script.time_stop_flag);
        push_bool(&mut out, script.counter_time_stop_flag);
        push_bool(&mut out, script.frame_action_time_stop_flag);
        push_bool(&mut out, script.stage_time_stop_flag);
        while out.len() % 4 != 0 { out.push(0); }
        debug_assert_eq!(out.len(), 356);
        out
    }

    fn read_cpp_local_data_pod(
        &mut self,
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<()> {
        let script = &mut self.ctx.globals.script;

        script.cur_koe_no = rd.i32()? as i64;
        script.cur_chr_no = rd.i32()? as i64;
        script.cur_read_flag_scn_no = rd.i32()? as i64;
        script.cur_read_flag_flag_no = rd.i32()? as i64;
        script.cursor_no = rd.i32()? as i64;

        self.ctx.globals.syscom.syscom_menu_disable = rd.bool()?;
        script.hide_mwnd_disable = rd.bool()?;
        script.msg_back_disable = rd.bool()?;
        script.shortcut_disable = rd.bool()?;

        script.skip_disable = rd.bool()?;
        script.ctrl_disable = rd.bool()?;
        script.not_stop_skip_by_click = rd.bool()?;
        script.not_skip_msg_by_click = rd.bool()?;
        script.skip_unread_message = rd.bool()?;
        script.auto_mode_flag = rd.bool()?;
        rd.skip(2)?;
        script.auto_mode_moji_wait = rd.i32()? as i64;
        script.auto_mode_min_wait = rd.i32()? as i64;
        script.auto_mode_moji_cnt = rd.i32()? as i64;
        script.mouse_cursor_hide_onoff = rd.i32()? as i64;
        script.mouse_cursor_hide_time = rd.i32()? as i64;
        script.msg_back_save_cntr = rd.i32()? as i64;

        script.msg_speed = rd.i32()? as i64;
        script.msg_nowait = rd.bool()?;
        script.async_msg_mode = rd.bool()?;
        script.async_msg_mode_once = rd.bool()?;
        script.multi_msg_mode = rd.bool()?;
        script.skip_trigger = rd.bool()?;
        script.koe_dont_stop_on_flag = rd.bool()?;
        script.koe_dont_stop_off_flag = rd.bool()?;

        self.ctx.globals.syscom.mwnd_btn_disable_all = rd.bool()?;
        self.ctx.globals.syscom.mwnd_btn_touch_disable = rd.bool()?;
        script.mwnd_anime_on_flag = rd.bool()?;
        script.mwnd_anime_off_flag = rd.bool()?;
        script.mwnd_disp_off_flag = rd.bool()?;

        script.msg_back_off = rd.bool()?;
        script.msg_back_disp_off = rd.bool()?;
        rd.skip(2)?;
        script.font_bold = rd.i32()? as i64;
        script.font_shadow = rd.i32()? as i64;

        script.cursor_disp_off = rd.bool()?;
        script.cursor_runtime_visible = !script.cursor_disp_off;
        script.cursor_move_by_key_disable = rd.bool()?;
        script.key_disable.clear();
        for key in 0u16..=255u16 {
            if rd.bool()? {
                script.key_disable.insert(key as u8);
            }
        }

        script.quake_stop_flag = rd.bool()?;
        script.emote_mouth_stop_flag = rd.bool()?;
        self.ctx.globals.cg_table_off = rd.bool()?;
        script.bgmfade_flag = rd.bool()?;
        script.dont_set_save_point = rd.bool()?;
        script.ignore_r_flag = rd.bool()?;
        script.wait_display_vsync_off_flag = rd.bool()?;

        script.time_stop_flag = rd.bool()?;
        script.counter_time_stop_flag = rd.bool()?;
        script.frame_action_time_stop_flag = rd.bool()?;
        script.stage_time_stop_flag = rd.bool()?;
        rd.skip(3)?;

        self.ctx.globals.syscom.replay_koe = if script.cur_koe_no >= 0 {
            Some((script.cur_koe_no, script.cur_chr_no))
        } else {
            None
        };
        Ok(())
    }

    fn write_cpp_syscom_menu(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        let base = w.position();
        let s = &self.ctx.globals.syscom;
        let push_ex = |w: &mut crate::original_save::OriginalStreamWriter, exist: bool, enable: bool| {
            w.push_bool(exist);
            w.push_bool(enable);
        };
        push_ex(w, s.read_skip.exist, s.read_skip.enable);
        push_ex(w, s.unread_skip.exist, s.unread_skip.enable);
        push_ex(w, s.auto_skip.exist, s.auto_skip.enable);
        push_ex(w, s.auto_mode.exist, s.auto_mode.enable);
        push_ex(w, s.hide_mwnd.exist, s.hide_mwnd.enable);
        push_ex(w, s.msg_back.exist, s.msg_back.enable);
        push_ex(w, s.save_feature.exist, s.save_feature.enable);
        push_ex(w, s.load_feature.exist, s.load_feature.enable);
        push_ex(w, s.return_to_sel.exist, s.return_to_sel.enable);
        push_ex(w, s.config_feature.exist, s.config_feature.enable);
        push_ex(w, s.manual_feature.exist, s.manual_feature.enable);
        push_ex(w, s.version_feature.exist, s.version_feature.enable);
        push_ex(w, s.return_to_menu.exist, s.return_to_menu.enable);
        push_ex(w, s.end_game.exist, s.end_game.enable);
        push_ex(w, s.cancel_feature.exist, s.cancel_feature.enable);
        for i in 0..4 {
            let sw = s.local_extra_switches.get(i).copied().unwrap_or(if i == 0 { s.local_extra_switch } else { runtime::globals::ToggleFeatureState::default() });
            w.push_bool(sw.exist);
            w.push_bool(sw.enable);
            w.push_bool(sw.onoff);
        }
        while (w.position() - base) % 4 != 0 { w.push_bool(false); }
        for i in 0..4 {
            let mode = s.local_extra_modes.get(i).copied().unwrap_or(if i == 0 { s.local_extra_mode } else { runtime::globals::ValueFeatureState::default() });
            w.push_bool(mode.exist);
            w.push_bool(mode.enable);
            w.push_padding(2);
            w.push_i32(mode.value as i32);
        }
        debug_assert_eq!(w.position() - base, 76);
    }

    fn read_cpp_syscom_menu(
        &mut self,
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<()> {
        fn read_ex(
            rd: &mut crate::original_save::OriginalStreamReader<'_>,
            state: &mut runtime::globals::ToggleFeatureState,
        ) -> Result<()> {
            state.exist = rd.bool()?;
            state.enable = rd.bool()?;
            Ok(())
        }

        let s = &mut self.ctx.globals.syscom;
        read_ex(rd, &mut s.read_skip)?;
        read_ex(rd, &mut s.unread_skip)?;
        read_ex(rd, &mut s.auto_skip)?;
        read_ex(rd, &mut s.auto_mode)?;
        read_ex(rd, &mut s.hide_mwnd)?;
        read_ex(rd, &mut s.msg_back)?;
        read_ex(rd, &mut s.save_feature)?;
        read_ex(rd, &mut s.load_feature)?;
        read_ex(rd, &mut s.return_to_sel)?;
        read_ex(rd, &mut s.config_feature)?;
        read_ex(rd, &mut s.manual_feature)?;
        read_ex(rd, &mut s.version_feature)?;
        read_ex(rd, &mut s.return_to_menu)?;
        read_ex(rd, &mut s.end_game)?;
        read_ex(rd, &mut s.cancel_feature)?;

        for sw in &mut s.local_extra_switches {
            sw.exist = rd.bool()?;
            sw.enable = rd.bool()?;
            sw.onoff = rd.bool()?;
        }
        rd.skip(2)?;
        for mode in &mut s.local_extra_modes {
            mode.exist = rd.bool()?;
            mode.enable = rd.bool()?;
            rd.skip(2)?;
            mode.value = rd.i32()? as i64;
        }
        s.local_extra_switch = s.local_extra_switches[0];
        s.local_extra_mode = s.local_extra_modes[0];
        Ok(())
    }

    fn write_empty_counter_param(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        w.push_bool(false);
        w.push_bool(false);
        w.push_bool(false);
        w.push_bool(false);
        w.push_i32(0);
        w.push_i32(0);
        w.push_i32(0);
        w.push_i32(0);
    }

    fn write_empty_frame_action(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        w.push_i32(0);
        w.push_str("");
        w.push_str("");
        w.push_i32(0);
        self.write_empty_counter_param(w);
    }

    fn write_empty_btn_select(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        w.push_i32(0);
        w.push_padding(112);
        w.push_bool(false);
        w.push_bool(false);
        w.push_bool(false);
        w.push_bool(false);
        w.push_bool(false);
        w.push_bool(false);
        w.push_str("");
        w.push_i32(0);
        w.push_i32(0);
    }

    fn write_empty_stage(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        w.push_empty_fixed_array();
        w.push_empty_fixed_array();
        w.push_empty_fixed_array();
        self.write_empty_btn_select(w);
        w.push_empty_fixed_array();
        w.push_empty_fixed_array();
        w.push_empty_fixed_array();
    }

    fn write_empty_screen(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        w.push_empty_fixed_array();
        w.push_padding(16);
        w.push_empty_fixed_array();
    }

    fn write_empty_sound(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        w.push_str("");
        w.push_i32(0);
        w.push_i32(0);
        w.push_bool(false);
        w.push_bool(false);
        w.push_i32(0);
        w.push_i32(0);
        w.push_empty_fixed_array();
        w.push_i32(0);
        w.push_str("");
    }

    fn write_empty_msg_back(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        w.push_i32(0);
        w.push_i32(0);
        w.push_i32(0);
        w.push_i32(0);
        w.push_bool(false);
    }

    fn write_cpp_prop(&self, w: &mut crate::original_save::OriginalStreamWriter, prop_id: i32, cell: &UserPropCell) {
        w.push_i32(prop_id);
        w.push_i32(cell.form);
        w.push_i32(cell.int_value);
        w.push_str(&cell.str_value);
        w.push_element(&cell.element);
        w.push_extend_items(&cell.list_items, |w, item| self.write_cpp_prop(w, 0, item));
        w.push_i32(cell.list_items.len() as i32);
        if cell.form == self.cfg.fm_intlist {
            let vals: Vec<i64> = cell.int_list.iter().map(|v| *v as i64).collect();
            w.push_extend_i32_list(&vals);
        } else if cell.form == self.cfg.fm_strlist {
            w.push_extend_str_list(&cell.str_list);
        }
    }

    fn read_cpp_prop(&self, rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<(i32, UserPropCell)> {
        let prop_id = rd.i32()?;
        let form = rd.i32()?;
        let int_value = rd.i32()?;
        let str_value = rd.string()?;
        let element = rd.element()?;
        let list_items = rd.extend_items(|rd| {
            let (_id, cell) = self.read_cpp_prop(rd)?;
            Ok(cell)
        })?;
        let _exp_cnt = rd.i32()?;
        let mut cell = UserPropCell::new(form, element);
        cell.int_value = int_value;
        cell.str_value = str_value;
        cell.list_items = list_items;
        if form == self.cfg.fm_intlist {
            cell.int_list = rd.extend_i32_list()?.into_iter().map(|v| v as i32).collect();
        } else if form == self.cfg.fm_strlist {
            cell.str_list = rd.extend_items(|rd| rd.string())?;
        }
        Ok((prop_id, cell))
    }

    fn write_cpp_inc_prop_list(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        let shared = self.shared_user_prop_count();
        let props: Vec<(i32, UserPropCell)> = (0..shared)
            .map(|idx| {
                let prop_id = idx as u16;
                let cell = self.user_props.get(&prop_id).cloned().unwrap_or_else(|| self.default_user_prop_cell(prop_id));
                (idx as i32, cell)
            })
            .collect();
        w.push_fixed_items(&props, |w, (id, cell)| self.write_cpp_prop(w, *id, cell));
    }

    fn read_cpp_inc_prop_list(&mut self, rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<()> {
        let props = rd.fixed_items(|rd| self.read_cpp_prop(rd))?;
        for (idx, (_stored_id, cell)) in props.into_iter().enumerate() {
            self.user_props.insert(idx as u16, cell);
        }
        Ok(())
    }

    fn write_cpp_current_scene_prop_lists(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        let shared = self.shared_user_prop_count();
        let mut props: Vec<(i32, UserPropCell)> = Vec::new();
        let scene_prop_cnt = self.stream.header.scn_prop_cnt.max(0) as usize;
        for idx in 0..scene_prop_cnt {
            let prop_id = (shared + idx) as u16;
            if let Some(cell) = self.user_props.get(&prop_id).cloned() {
                props.push((idx as i32, cell));
            } else if let Some((_, _)) = self.user_prop_decl(prop_id) {
                props.push((idx as i32, self.default_user_prop_cell(prop_id)));
            }
        }
        if props.is_empty() {
            w.push_i32(0);
            return;
        }
        w.push_i32(1);
        w.push_str(self.current_scene_name.as_deref().unwrap_or(""));
        w.push_fixed_items(&props, |w, (id, cell)| self.write_cpp_prop(w, *id, cell));
    }

    fn read_cpp_scene_prop_lists(&mut self, rd: &mut crate::original_save::OriginalStreamReader<'_>, current_scene_name: &str) -> Result<()> {
        let shared = self.shared_user_prop_count();
        let scene_prop_cnt = rd.i32()?.max(0) as usize;
        for _ in 0..scene_prop_cnt {
            let scene_name = rd.string()?;
            let props = rd.fixed_items(|rd| self.read_cpp_prop(rd))?;
            if scene_name == current_scene_name {
                for (idx, (_stored_id, cell)) in props.into_iter().enumerate() {
                    self.user_props.insert((shared + idx) as u16, cell);
                }
            }
        }
        Ok(())
    }

    fn write_cpp_call_prop(&self, w: &mut crate::original_save::OriginalStreamWriter, prop: &CallProp) {
        w.push_i32(prop.scn_no);
        w.push_i32(prop.prop_id);
        let mut cell = UserPropCell::new(prop.form, prop.element.clone());
        match &prop.value {
            CallPropValue::Int(v) => cell.int_value = *v,
            CallPropValue::Str(v) => cell.str_value = v.clone(),
            CallPropValue::Element(v) => cell.element = v.clone(),
            CallPropValue::IntList(v) => cell.int_list = v.clone(),
            CallPropValue::StrList(v) => cell.str_list = v.clone(),
        }
        self.write_cpp_prop(w, prop.prop_id, &cell);
    }

    fn read_cpp_call_prop(&self, rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<CallProp> {
        let scn_no = rd.i32()?;
        let declared_prop_id = rd.i32()?;
        let (_stored_id, cell) = self.read_cpp_prop(rd)?;
        let value = if cell.form == self.cfg.fm_int {
            CallPropValue::Int(cell.int_value)
        } else if cell.form == self.cfg.fm_str {
            CallPropValue::Str(cell.str_value.clone())
        } else if cell.form == self.cfg.fm_intlist {
            CallPropValue::IntList(cell.int_list.clone())
        } else if cell.form == self.cfg.fm_strlist {
            CallPropValue::StrList(cell.str_list.clone())
        } else {
            CallPropValue::Element(cell.element.clone())
        };
        Ok(CallProp {
            scn_no,
            prop_id: declared_prop_id,
            form: cell.form,
            decl_size: cell.int_list.len().max(cell.str_list.len()).max(cell.list_items.len()),
            element: cell.element,
            value,
        })
    }

    fn write_cpp_call_frame(&self, w: &mut crate::original_save::OriginalStreamWriter, frame: &CallFrame) {
        let l: Vec<i64> = frame.int_args.iter().map(|v| *v as i64).collect();
        w.push_extend_i32_list(&l);
        w.push_extend_str_list(&frame.str_args);
        w.push_extend_items(&frame.user_props, |w, prop| {
            self.write_cpp_call_prop(w, prop)
        });
        w.push_i32(frame.call_type);
        w.push_i32(frame.ret_form);
        w.push_str(frame.return_scene_name.as_deref().unwrap_or(""));
        w.push_i32(frame.return_line_no);
        w.push_i32(frame.return_pc as i32);
    }

    fn flattened_call_stack_for_save(&self) -> Vec<CallFrame> {
        // C++ saves C_elm_call_list verbatim. The caller lexer position is
        // captured when a call is entered (tnm_save_call), so the save writer
        // must not infer or rewrite scene ownership from Rust's scene_stack.
        self.call_stack.clone()
    }

    fn read_cpp_call_frame(&self, rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<CallFrame> {
        let int_args: Vec<i32> = rd.extend_i32_list()?.into_iter().map(|v| v as i32).collect();
        let str_args: Vec<String> = rd.extend_items(|rd| rd.string())?;
        let user_props = rd.extend_items(|rd| self.read_cpp_call_prop(rd))?;
        let call_type = rd.i32()?;
        let ret_form = rd.i32()?;
        let scene_name = rd.string()?;
        let line_no = rd.i32()?;
        let return_pc = rd.i32()?.max(0) as usize;
        log::warn!(
            "[SG_SAVELOAD_PROBE] call_frame scene={:?} line={} return_pc=0x{:x} call_type={} ret_form={} int_args={} str_args={} props={}",
            scene_name,
            line_no,
            return_pc,
            call_type,
            ret_form,
            int_args.len(),
            str_args.len(),
            user_props.len(),
        );
        Ok(CallFrame {
            call_type,
            return_pc,
            return_scene_no: None,
            return_scene_name: (!scene_name.is_empty()).then_some(scene_name),
            return_line_no: line_no,
            ret_form,
            return_override: None,
            excall_proc: false,
            // C_elm_call::save() does not persist excall_flag or
            // frame_action_flag. Do not reinterpret USER_CMD as frame_action.
            frame_action_proc: false,
            arg_cnt: 0,
            delayed_ret_form: None,
            user_props,
            int_args,
            str_args,
        })
    }


    fn save_i32(v: i64) -> i32 {
        v.clamp(i32::MIN as i64, i32::MAX as i64) as i32
    }

    fn write_cpp_counter_param(&self, w: &mut crate::original_save::OriginalStreamWriter, c: &runtime::globals::Counter) {
        let (is_running, real_flag, frame_mode, frame_loop_flag, frame_start_value, frame_end_value, frame_time, cur_time) = c.save_parts();
        w.push_bool(is_running);
        w.push_bool(real_flag);
        w.push_bool(frame_mode);
        w.push_bool(frame_loop_flag);
        w.push_i32(Self::save_i32(frame_start_value));
        w.push_i32(Self::save_i32(frame_end_value));
        w.push_i32(Self::save_i32(frame_time));
        w.push_i32(Self::save_i32(cur_time));
    }

    fn read_cpp_counter_param(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<runtime::globals::Counter> {
        let is_running = rd.bool()?;
        let real_flag = rd.bool()?;
        let frame_mode = rd.bool()?;
        let frame_loop_flag = rd.bool()?;
        let frame_start_value = rd.i32()? as i64;
        let frame_end_value = rd.i32()? as i64;
        let frame_time = rd.i32()? as i64;
        let cur_time = rd.i32()? as i64;
        Ok(runtime::globals::Counter::from_save_parts(
            is_running,
            real_flag,
            frame_mode,
            frame_loop_flag,
            frame_start_value,
            frame_end_value,
            frame_time,
            cur_time,
        ))
    }

    fn write_cpp_value_prop(w: &mut crate::original_save::OriginalStreamWriter, value: &Value) {
        use crate::runtime::forms::codes;
        w.push_i32(0);
        match value {
            Value::Str(s) => {
                w.push_i32(codes::FM_STR);
                w.push_i32(0);
                w.push_str(s);
            }
            Value::Int(v) => {
                w.push_i32(codes::FM_INT);
                w.push_i32(Self::save_i32(*v));
                w.push_str("");
            }
            _ => {
                w.push_i32(codes::FM_INT);
                w.push_i32(0);
                w.push_str("");
            }
        }
        w.push_empty_element();
        w.push_i32(0);
        w.push_i32(0);
    }

    fn read_cpp_value_prop(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<Value> {
        use crate::runtime::forms::codes;
        let _id = rd.i32()?;
        let form = rd.i32()?;
        let int_value = rd.i32()?;
        let str_value = rd.string()?;
        let _element = rd.element()?;
        let _exp_list: Vec<Value> = rd.extend_items(|rd| Self::read_cpp_value_prop(rd))?;
        let _exp_cnt = rd.i32()?;
        if form == codes::FM_INTLIST {
            let _ = rd.extend_i32_list()?;
        } else if form == codes::FM_STRLIST {
            let _ = rd.extend_items(|rd| rd.string())?;
        }
        if form == codes::FM_STR {
            Ok(Value::Str(str_value))
        } else {
            Ok(Value::Int(int_value as i64))
        }
    }

    fn write_cpp_frame_action(&self, w: &mut crate::original_save::OriginalStreamWriter, fa: &runtime::globals::ObjectFrameActionState) {
        w.push_i32(Self::save_i32(fa.end_time));
        w.push_str(&fa.scn_name);
        w.push_str(&fa.cmd_name);
        w.push_extend_items(&fa.args, |w, arg| Self::write_cpp_value_prop(w, arg));
        self.write_cpp_counter_param(w, &fa.counter);
    }

    fn read_cpp_frame_action(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<runtime::globals::ObjectFrameActionState> {
        let end_time = rd.i32()? as i64;
        let scn_name = rd.string()?;
        let cmd_name = rd.string()?;
        let args = rd.extend_items(|rd| Self::read_cpp_value_prop(rd))?;
        let counter = Self::read_cpp_counter_param(rd)?;
        Ok(runtime::globals::ObjectFrameActionState {
            scn_name,
            cmd_name,
            counter,
            end_time,
            real_time_flag: false,
            end_flag: false,
            args,
        })
    }

    fn write_cpp_int_event_raw(w: &mut crate::original_save::OriginalStreamWriter, e: &runtime::int_event::IntEvent) {
        w.push_i32(e.def_value);
        w.push_i32(e.value);
        w.push_i32(e.cur_time);
        w.push_i32(e.end_time);
        w.push_i32(e.delay_time);
        w.push_i32(e.start_value);
        w.push_i32(e.cur_value);
        w.push_i32(e.end_value);
        w.push_i32(e.loop_type);
        w.push_i32(e.speed_type);
        w.push_i32(e.real_flag);
    }

    fn read_cpp_int_event_raw(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<runtime::int_event::IntEvent> {
        let def_value = rd.i32()?;
        Ok(runtime::int_event::IntEvent {
            def_value,
            value: rd.i32()?,
            cur_time: rd.i32()?,
            end_time: rd.i32()?,
            delay_time: rd.i32()?,
            start_value: rd.i32()?,
            cur_value: rd.i32()?,
            end_value: rd.i32()?,
            loop_type: rd.i32()?,
            speed_type: rd.i32()?,
            real_flag: rd.i32()?,
        })
    }

    fn write_cpp_save_event(w: &mut crate::original_save::OriginalStreamWriter, e: &runtime::int_event::IntEvent) {
        w.push_i32(e.loop_type);
        if e.loop_type != -1 {
            Self::write_cpp_int_event_raw(w, e);
        } else {
            w.push_i32(e.value);
        }
    }

    fn read_cpp_save_event(rd: &mut crate::original_save::OriginalStreamReader<'_>, def_value: i32) -> Result<runtime::int_event::IntEvent> {
        let loop_type = rd.i32()?;
        if loop_type != -1 {
            let mut e = Self::read_cpp_int_event_raw(rd)?;
            e.loop_type = loop_type;
            Ok(e)
        } else {
            let mut e = runtime::int_event::IntEvent::new(def_value);
            e.loop_type = -1;
            e.value = rd.i32()?;
            e.cur_value = e.value;
            Ok(e)
        }
    }

    fn write_cpp_int_event_extend_list(&self, w: &mut crate::original_save::OriginalStreamWriter, values: &[runtime::int_event::IntEvent]) {
        w.push_extend_items(values, |w, e| Self::write_cpp_int_event_raw(w, e));
    }

    fn read_cpp_int_event_extend_list(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<Vec<runtime::int_event::IntEvent>> {
        rd.extend_items(|rd| Self::read_cpp_int_event_raw(rd))
    }

    fn write_cpp_group(&self, w: &mut crate::original_save::OriginalStreamWriter, g: &runtime::globals::GroupState) {
        w.push_i32(Self::save_i32(g.order));
        w.push_i32(Self::save_i32(g.layer));
        w.push_i32(Self::save_i32(g.cancel_priority));
        w.push_i32(Self::save_i32(g.cancel_se_no));
        w.push_i32(Self::save_i32(g.decided_button_no));
        w.push_i32(Self::save_i32(g.result));
        w.push_i32(Self::save_i32(g.result_button_no));
        w.push_bool(g.started);
        w.push_bool(g.pause_flag);
        w.push_bool(g.wait_flag);
        w.push_bool(g.cancel_flag);
        w.push_element(&g.target_object);
    }

    fn read_cpp_group(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<runtime::globals::GroupState> {
        let mut g = runtime::globals::GroupState::default();
        g.order = rd.i32()? as i64;
        g.layer = rd.i32()? as i64;
        g.cancel_priority = rd.i32()? as i64;
        g.cancel_se_no = rd.i32()? as i64;
        g.decided_button_no = rd.i32()? as i64;
        g.result = rd.i32()? as i64;
        g.result_button_no = rd.i32()? as i64;
        g.started = rd.bool()?;
        g.pause_flag = rd.bool()?;
        g.wait_flag = rd.bool()?;
        g.cancel_flag = rd.bool()?;
        g.target_object = rd.element()?;
        Ok(g)
    }

    fn write_cpp_object(&self, w: &mut crate::original_save::OriginalStreamWriter, obj: &runtime::globals::ObjectState) {
        let b = &obj.base;
        let ev = &obj.runtime.prop_events;
        w.push_i32(Self::save_i32(obj.object_type));
        w.push_i32(Self::save_i32(b.wipe_copy));
        w.push_i32(Self::save_i32(b.wipe_erase));
        w.push_i32(Self::save_i32(b.click_disable));
        // C_elm_object_param_filter: C_rect + C_argb.
        w.push_i32(Self::save_i32(obj.rect_param.left));
        w.push_i32(Self::save_i32(obj.rect_param.top));
        w.push_i32(Self::save_i32(obj.rect_param.right));
        w.push_i32(Self::save_i32(obj.rect_param.bottom));
        w.push_i32(Self::save_i32(obj.rect_param.color_argb));
        // C_elm_object_param_string.
        w.push_i32(Self::save_i32(obj.string_param.moji_size));
        w.push_i32(Self::save_i32(obj.string_param.moji_space_x));
        w.push_i32(Self::save_i32(obj.string_param.moji_space_y));
        w.push_i32(Self::save_i32(obj.string_param.moji_cnt));
        w.push_i32(Self::save_i32(obj.string_param.moji_color));
        w.push_i32(Self::save_i32(obj.string_param.shadow_color));
        w.push_i32(Self::save_i32(obj.string_param.fuchi_color));
        w.push_i32(Self::save_i32(obj.string_param.shadow_mode));
        // C_elm_object_param_number.
        w.push_i32(Self::save_i32(obj.number_value));
        w.push_i32(Self::save_i32(obj.number_param.keta_max));
        w.push_i32(Self::save_i32(obj.number_param.disp_zero));
        w.push_i32(Self::save_i32(obj.number_param.disp_sign));
        w.push_i32(Self::save_i32(obj.number_param.tumeru_sign));
        w.push_i32(Self::save_i32(obj.number_param.space_mod));
        w.push_i32(Self::save_i32(obj.number_param.space));
        if obj.object_type == 4 {
            // C_tnm_save_stream::save(TYPE) writes the complete MSVC struct
            // byte-for-byte. C_elm_object_param_weather is 21 i32 fields, one
            // bool, then three bytes of tail padding (88 bytes total).
            let wp = &obj.weather_param;
            for v in [
                wp.weather_type,
                wp.cnt,
                wp.pat_mode,
                wp.pat_no_00,
                wp.pat_no_01,
                wp.pat_time,
                wp.move_time_x,
                wp.move_time_y,
                wp.sin_time_x,
                wp.sin_time_y,
                wp.sin_power_x,
                wp.sin_power_y,
                wp.center_x,
                wp.center_y,
                wp.center_rotate,
                wp.appear_range,
                wp.zoom_min,
                wp.zoom_max,
                wp.scale_x,
                wp.scale_y,
                wp.active_time,
            ] {
                w.push_i32(Self::save_i32(v));
            }
            w.push_bool(wp.real_time_flag);
            w.push_padding(3);
        }
        w.push_i32(Self::save_i32(obj.thumb_save_no));
        w.push_bool(obj.movie.loop_flag);
        w.push_bool(obj.movie.auto_free_flag);
        w.push_bool(obj.movie.real_time_flag);
        w.push_bool(obj.movie.pause_flag);
        if obj.object_type == 12 {
            w.push_i32(Self::save_i32(obj.emote.width));
            w.push_i32(Self::save_i32(obj.emote.height));
            for option in obj.emote.timeline_options {
                w.push_i32(Self::save_i32(option));
            }
            w.push_i32(Self::save_i32(obj.emote.koe_chara_no));
            w.push_i32(Self::save_i32(obj.emote.koe_mouth_volume));
            w.push_i32(Self::save_i32(obj.emote.rep_x));
            w.push_i32(Self::save_i32(obj.emote.rep_y));
        }
        if obj.button.enabled {
            w.push_i32(1);
            w.push_i32(Self::save_i32(obj.button.sys_type));
            w.push_i32(Self::save_i32(obj.button.sys_type_opt));
            w.push_i32(Self::save_i32(obj.button.action_no));
            w.push_i32(Self::save_i32(obj.button.se_no));
            w.push_i32(Self::save_i32(obj.button.button_no));
            w.push_empty_element();
            w.push_i32(if obj.button.push_keep { 1 } else { 0 });
            w.push_i32(Self::save_i32(obj.button.state));
            w.push_i32(Self::save_i32(obj.button.mode));
            w.push_i32(Self::save_i32(obj.button.cut_no));
            w.push_i32(-1);
            w.push_i32(-1);
            w.push_i32(Self::save_i32(obj.button.decided_action_z_no));
            w.push_i32(0);
            w.push_i32(if obj.button.alpha_test { 1 } else { 0 });
        } else {
            w.push_i32(0);
        }
        w.push_i32(Self::save_i32(b.disp));
        // The shipped Rewrite+ C++ save layout stores obp.pat_no as a scalar;
        // PATNO_EVE is serialized separately with the other animated fields.
        w.push_i32(Self::save_i32(b.patno));
        w.push_i32(Self::save_i32(b.order));
        w.push_i32(Self::save_i32(b.layer));
        w.push_i32(Self::save_i32(b.world));
        w.push_i32(Self::save_i32(b.child_sort_type));
        for e in [&ev.x, &ev.y, &ev.z, &ev.center_x, &ev.center_y, &ev.center_z, &ev.center_rep_x, &ev.center_rep_y, &ev.center_rep_z, &ev.scale_x, &ev.scale_y, &ev.scale_z, &ev.rotate_x, &ev.rotate_y, &ev.rotate_z] {
            Self::write_cpp_save_event(w, e);
        }
        w.push_i32(Self::save_i32(b.clip_use));
        for e in [&ev.clip_left, &ev.clip_top, &ev.clip_right, &ev.clip_bottom] { Self::write_cpp_save_event(w, e); }
        w.push_i32(Self::save_i32(b.src_clip_use));
        for e in [&ev.src_clip_left, &ev.src_clip_top, &ev.src_clip_right, &ev.src_clip_bottom, &ev.tr, &ev.mono, &ev.reverse, &ev.bright, &ev.dark, &ev.color_r, &ev.color_g, &ev.color_b, &ev.color_rate, &ev.color_add_r, &ev.color_add_g, &ev.color_add_b] {
            Self::write_cpp_save_event(w, e);
        }
        for v in [b.mask_no, b.tonecurve_no, b.light_no, b.fog_use, b.culling, b.alpha_test, b.alpha_blend, b.blend, 0] {
            w.push_i32(Self::save_i32(v));
        }
        self.write_cpp_int_event_extend_list(w, &obj.runtime.prop_event_lists.x_rep);
        self.write_cpp_int_event_extend_list(w, &obj.runtime.prop_event_lists.y_rep);
        self.write_cpp_int_event_extend_list(w, &obj.runtime.prop_event_lists.z_rep);
        self.write_cpp_int_event_extend_list(w, &obj.runtime.prop_event_lists.tr_rep);
        w.push_extend_i32_list(&obj.runtime.prop_lists.f);
        w.push_str(obj.file_name.as_deref().unwrap_or(""));
        w.push_str(obj.string_value.as_deref().unwrap_or(""));
        w.push_str(&obj.button.decided_action_scn_name);
        w.push_str(&obj.button.decided_action_cmd_name);
        if obj.object_type == 12 {
            for timeline in &obj.emote.timeline_names {
                w.push_str(timeline);
            }
        }
        self.write_cpp_frame_action(w, &obj.frame_action);
        w.push_extend_items(&obj.frame_action_ch, |w, fa| self.write_cpp_frame_action(w, fa));
        w.push_str(obj.gan_file.as_deref().unwrap_or(""));
        w.push_extend_items(&obj.runtime.child_objects, |w, child| self.write_cpp_object(w, child));
    }

    fn read_cpp_object(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<runtime::globals::ObjectState> {
        let mut obj = runtime::globals::ObjectState::default();
        obj.object_type = rd.i32()? as i64;
        obj.base.wipe_copy = rd.i32()? as i64;
        obj.base.wipe_erase = rd.i32()? as i64;
        obj.base.click_disable = rd.i32()? as i64;
        obj.rect_param.left = rd.i32()? as i64;
        obj.rect_param.top = rd.i32()? as i64;
        obj.rect_param.right = rd.i32()? as i64;
        obj.rect_param.bottom = rd.i32()? as i64;
        obj.rect_param.color_argb = rd.i32()? as i64;
        obj.string_param.moji_size = rd.i32()? as i64;
        obj.string_param.moji_space_x = rd.i32()? as i64;
        obj.string_param.moji_space_y = rd.i32()? as i64;
        obj.string_param.moji_cnt = rd.i32()? as i64;
        obj.string_param.moji_color = rd.i32()? as i64;
        obj.string_param.shadow_color = rd.i32()? as i64;
        obj.string_param.fuchi_color = rd.i32()? as i64;
        obj.string_param.shadow_mode = rd.i32()? as i64;
        obj.number_value = rd.i32()? as i64;
        obj.number_param.keta_max = rd.i32()? as i64;
        obj.number_param.disp_zero = rd.i32()? as i64;
        obj.number_param.disp_sign = rd.i32()? as i64;
        obj.number_param.tumeru_sign = rd.i32()? as i64;
        obj.number_param.space_mod = rd.i32()? as i64;
        obj.number_param.space = rd.i32()? as i64;
        if obj.object_type == 4 {
            obj.weather_param.weather_type = rd.i32()? as i64;
            obj.weather_param.cnt = rd.i32()? as i64;
            obj.weather_param.pat_mode = rd.i32()? as i64;
            obj.weather_param.pat_no_00 = rd.i32()? as i64;
            obj.weather_param.pat_no_01 = rd.i32()? as i64;
            obj.weather_param.pat_time = rd.i32()? as i64;
            obj.weather_param.move_time_x = rd.i32()? as i64;
            obj.weather_param.move_time_y = rd.i32()? as i64;
            obj.weather_param.sin_time_x = rd.i32()? as i64;
            obj.weather_param.sin_time_y = rd.i32()? as i64;
            obj.weather_param.sin_power_x = rd.i32()? as i64;
            obj.weather_param.sin_power_y = rd.i32()? as i64;
            obj.weather_param.center_x = rd.i32()? as i64;
            obj.weather_param.center_y = rd.i32()? as i64;
            obj.weather_param.center_rotate = rd.i32()? as i64;
            obj.weather_param.appear_range = rd.i32()? as i64;
            obj.weather_param.zoom_min = rd.i32()? as i64;
            obj.weather_param.zoom_max = rd.i32()? as i64;
            obj.weather_param.scale_x = rd.i32()? as i64;
            obj.weather_param.scale_y = rd.i32()? as i64;
            obj.weather_param.active_time = rd.i32()? as i64;
            obj.weather_param.real_time_flag = rd.bool()?;
            // MSVC pads the trailing bool to the struct's 4-byte alignment.
            rd.skip(3)?;
            // `move_time` is a Rust convenience alias for TYPE_B; it is not an
            // additional C++ serialized field.
            obj.weather_param.move_time = obj.weather_param.move_time_x;
        }
        obj.thumb_save_no = rd.i32()? as i64;
        obj.movie.loop_flag = rd.bool()?;
        obj.movie.auto_free_flag = rd.bool()?;
        obj.movie.real_time_flag = rd.bool()?;
        obj.movie.pause_flag = rd.bool()?;
        if obj.object_type == 12 {
            obj.emote.width = rd.i32()? as i64;
            obj.emote.height = rd.i32()? as i64;
            for option in &mut obj.emote.timeline_options {
                *option = rd.i32()? as i64;
            }
            obj.emote.koe_chara_no = rd.i32()? as i64;
            obj.emote.koe_mouth_volume = rd.i32()? as i64;
            obj.emote.rep_x = rd.i32()? as i64;
            obj.emote.rep_y = rd.i32()? as i64;
        }
        let button_exist = rd.i32()? != 0;
        if button_exist {
            obj.button.enabled = true;
            obj.button.sys_type = rd.i32()? as i64;
            obj.button.sys_type_opt = rd.i32()? as i64;
            obj.button.action_no = rd.i32()? as i64;
            obj.button.se_no = rd.i32()? as i64;
            obj.button.button_no = rd.i32()? as i64;
            rd.skip_element()?;
            obj.button.push_keep = rd.i32()? != 0;
            obj.button.state = rd.i32()? as i64;
            obj.button.mode = rd.i32()? as i64;
            obj.button.cut_no = rd.i32()? as i64;
            let _ = rd.i32()?;
            let _ = rd.i32()?;
            obj.button.decided_action_z_no = rd.i32()? as i64;
            let _ = rd.i32()?;
            obj.button.alpha_test = rd.i32()? != 0;
        }
        obj.base.disp = rd.i32()? as i64;
        // Rewrite+ original saves (including 0212.sav) serialize obp.pat_no as
        // the legacy scalar field. New Rust saves may emit the richer event
        // form, but load compatibility must keep accepting the shipped C++
        // layout; the surrounding fields prove which format is in use before
        // any caller continuation is restored.
        obj.base.patno = rd.i32()? as i64;
        obj.runtime.prop_events.patno = runtime::int_event::IntEvent::new(obj.base.patno as i32);
        obj.base.order = rd.i32()? as i64;
        obj.base.layer = rd.i32()? as i64;
        obj.base.world = rd.i32()? as i64;
        obj.base.child_sort_type = rd.i32()? as i64;
        obj.runtime.prop_events.x = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.y = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.z = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.center_x = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.center_y = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.center_z = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.center_rep_x = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.center_rep_y = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.center_rep_z = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.scale_x = Self::read_cpp_save_event(rd, 1000)?;
        obj.runtime.prop_events.scale_y = Self::read_cpp_save_event(rd, 1000)?;
        obj.runtime.prop_events.scale_z = Self::read_cpp_save_event(rd, 1000)?;
        obj.runtime.prop_events.rotate_x = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.rotate_y = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.rotate_z = Self::read_cpp_save_event(rd, 0)?;
        obj.base.clip_use = rd.i32()? as i64;
        obj.runtime.prop_events.clip_left = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.clip_top = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.clip_right = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.clip_bottom = Self::read_cpp_save_event(rd, 0)?;
        obj.base.src_clip_use = rd.i32()? as i64;
        obj.runtime.prop_events.src_clip_left = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.src_clip_top = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.src_clip_right = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.src_clip_bottom = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.tr = Self::read_cpp_save_event(rd, 255)?;
        obj.runtime.prop_events.mono = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.reverse = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.bright = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.dark = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.color_r = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.color_g = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.color_b = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.color_rate = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.color_add_r = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.color_add_g = Self::read_cpp_save_event(rd, 0)?;
        obj.runtime.prop_events.color_add_b = Self::read_cpp_save_event(rd, 0)?;
        obj.base.mask_no = rd.i32()? as i64;
        obj.base.tonecurve_no = rd.i32()? as i64;
        obj.base.light_no = rd.i32()? as i64;
        obj.base.fog_use = rd.i32()? as i64;
        obj.base.culling = rd.i32()? as i64;
        obj.base.alpha_test = rd.i32()? as i64;
        obj.base.alpha_blend = rd.i32()? as i64;
        obj.base.blend = rd.i32()? as i64;
        let _flags = rd.i32()?;
        obj.runtime.prop_event_lists.x_rep = Self::read_cpp_int_event_extend_list(rd)?;
        obj.runtime.prop_event_lists.y_rep = Self::read_cpp_int_event_extend_list(rd)?;
        obj.runtime.prop_event_lists.z_rep = Self::read_cpp_int_event_extend_list(rd)?;
        obj.runtime.prop_event_lists.tr_rep = Self::read_cpp_int_event_extend_list(rd)?;
        obj.runtime.prop_lists.f = rd.extend_i32_list()?;
        let file_name = rd.string()?;
        obj.file_name = if file_name.is_empty() { None } else { Some(file_name) };
        let string_value = rd.string()?;
        obj.string_value = if string_value.is_empty() { None } else { Some(string_value) };
        obj.button.decided_action_scn_name = rd.string()?;
        obj.button.decided_action_cmd_name = rd.string()?;
        if obj.object_type == 12 {
            for timeline in &mut obj.emote.timeline_names {
                *timeline = rd.string()?;
            }
        }
        obj.frame_action = Self::read_cpp_frame_action(rd)?;
        obj.frame_action_ch = rd.extend_items(|rd| Self::read_cpp_frame_action(rd))?;
        let gan_file = rd.string()?;
        obj.gan_file = if gan_file.is_empty() { None } else { Some(gan_file) };
        obj.runtime.child_objects = rd.extend_items(|rd| Self::read_cpp_object(rd))?;
        obj.used = obj.object_type != 0 || obj.file_name.is_some() || obj.string_value.is_some();
        Ok(obj)
    }

    fn write_cpp_mwnd_glyph(
        w: &mut crate::original_save::OriginalStreamWriter,
        glyph: &runtime::globals::MwndGlyphState,
    ) {
        // C_tnm_moji is eight consecutive 32-bit integers.
        w.push_i32(glyph.moji_type);
        w.push_i32(glyph.code);
        w.push_i32(Self::save_i32(glyph.size));
        w.push_i32(Self::save_i32(glyph.moji_color_no));
        w.push_i32(Self::save_i32(glyph.shadow_color_no));
        w.push_i32(Self::save_i32(glyph.fuchi_color_no));
        w.push_i32(Self::save_i32(glyph.x));
        w.push_i32(Self::save_i32(glyph.y));
        w.push_bool(glyph.appeared);
        w.push_bool(glyph.ruby);
    }

    fn read_cpp_mwnd_glyph(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<runtime::globals::MwndGlyphState> {
        let moji_type = rd.i32()?;
        let code = rd.i32()?;
        let size = rd.i32()? as i64;
        let moji_color_no = rd.i32()? as i64;
        let shadow_color_no = rd.i32()? as i64;
        let fuchi_color_no = rd.i32()? as i64;
        let x = rd.i32()? as i64;
        let y = rd.i32()? as i64;
        let appeared = rd.bool()?;
        let ruby = rd.bool()?;
        let ch = if moji_type == 0 {
            char::from_u32(code as u32).unwrap_or('\u{fffd}')
        } else {
            '\0'
        };
        Ok(runtime::globals::MwndGlyphState {
            moji_type,
            code,
            ch,
            x,
            y,
            size,
            moji_color_no,
            shadow_color_no,
            fuchi_color_no,
            shadow: shadow_color_no >= 0,
            fuchi: fuchi_color_no >= 0,
            bold: false,
            reveal_index: 1,
            ruby,
            appeared,
            message_button: None,
        })
    }

    fn active_mwnd_message_page(
        m: &runtime::globals::MwndState,
    ) -> runtime::globals::MwndMessagePageState {
        runtime::globals::MwndMessagePageState {
            msg_text: m.msg_text.clone(),
            glyphs: m.glyphs.clone(),
            disp_moji_cnt: m.disp_moji_cnt,
            hide_moji_cnt: m.hide_moji_cnt,
            cur_msg_type: m.cur_msg_type,
            cur_msg_type_decided: m.cur_msg_type_decided,
            ruby_start_pos: m.ruby_start_pos,
            ruby_start_ready: m.ruby_start_ready,
            cursor_pos: m.cursor_pos,
            moji_rep_pos: m.moji_rep_pos,
            indent_pos: m.indent_pos,
            indent_moji: m.indent_moji,
            indent_count: m.indent_count,
            line_head: m.line_head,
            ruby_pending: m.ruby_pending.clone(),
            moji_size: m.moji_size,
            moji_color: m.moji_color,
            shadow_color: m.shadow_color,
            fuchi_color: m.fuchi_color,
            chara_moji_color: m.chara_moji_color,
            chara_shadow_color: m.chara_shadow_color,
            chara_fuchi_color: m.chara_fuchi_color,
            msgbtn: m.msgbtn,
        }
    }

    fn write_cpp_mwnd_message(
        w: &mut crate::original_save::OriginalStreamWriter,
        m: &runtime::globals::MwndState,
        page: &runtime::globals::MwndMessagePageState,
    ) {
        // C_elm_mwnd_msg::PARAM (21 consecutive i32 fields).
        let (cnt_x, cnt_y) = m.window_moji_cnt.unwrap_or((0, 0));
        let (pos_x, pos_y) = m.message_pos.unwrap_or((0, 0));
        let (space_x, space_y) = m.moji_space.unwrap_or((-1, 10));
        let (talk_l, talk_t, talk_r, talk_b) = m.message_margin.unwrap_or((0, 0, 0, 0));
        for value in [
            cnt_x,
            cnt_y,
            pos_x,
            pos_y,
            page.moji_rep_pos.0,
            page.moji_rep_pos.1,
            page.moji_size.unwrap_or(m.default_moji_size),
            space_x,
            space_y,
            page.moji_color.unwrap_or(-1),
            page.shadow_color.unwrap_or(-1),
            page.fuchi_color.unwrap_or(-1),
            m.ruby_size,
            m.ruby_space,
            m.default_moji_size,
            0,
            m.name_bracket,
            talk_l,
            talk_t,
            talk_r,
            talk_b,
        ] {
            w.push_i32(Self::save_i32(value));
        }
        w.push_i32(Self::save_i32(page.chara_moji_color.unwrap_or(-1)));
        w.push_i32(Self::save_i32(page.chara_shadow_color.unwrap_or(-1)));
        w.push_i32(Self::save_i32(page.chara_fuchi_color.unwrap_or(-1)));
        w.push_i32(Self::save_i32(page.indent_pos));
        w.push_u16(page.indent_moji.unwrap_or('\0') as u32 as u16);
        w.push_i32(Self::save_i32(page.indent_count));
        w.push_i32(Self::save_i32(page.cur_msg_type));
        w.push_i32(Self::save_i32(page.ruby_start_pos.0));
        w.push_i32(Self::save_i32(page.ruby_start_pos.1));
        w.push_i32(Self::save_i32(page.disp_moji_cnt));
        w.push_i32(Self::save_i32(page.hide_moji_cnt));
        w.push_str(&page.msg_text);
        w.push_str(
            page.ruby_pending
                .as_ref()
                .map(|ruby| ruby.text.as_str())
                .unwrap_or(""),
        );
        let (btn_no, group_no, action_no, se_no) = page.msgbtn.unwrap_or((0, 0, 0, 0));
        w.push_i32(Self::save_i32(btn_no));
        w.push_i32(Self::save_i32(group_no));
        w.push_i32(Self::save_i32(action_no));
        w.push_i32(Self::save_i32(se_no));
        w.push_bool(page.cur_msg_type_decided);
        w.push_bool(page.line_head);
        w.push_bool(page.ruby_start_ready);
        w.push_bool(page.msgbtn.is_some());
        w.push_extend_items(&page.glyphs, |w, glyph| Self::write_cpp_mwnd_glyph(w, glyph));
    }

    fn read_cpp_mwnd_message(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
        m: &mut runtime::globals::MwndState,
    ) -> Result<runtime::globals::MwndMessagePageState> {
        let cnt_x = rd.i32()? as i64;
        let cnt_y = rd.i32()? as i64;
        let pos_x = rd.i32()? as i64;
        let pos_y = rd.i32()? as i64;
        let rep_x = rd.i32()? as i64;
        let rep_y = rd.i32()? as i64;
        let moji_size = rd.i32()? as i64;
        let space_x = rd.i32()? as i64;
        let space_y = rd.i32()? as i64;
        let moji_color = rd.i32()? as i64;
        let shadow_color = rd.i32()? as i64;
        let fuchi_color = rd.i32()? as i64;
        let ruby_size = rd.i32()? as i64;
        let ruby_space = rd.i32()? as i64;
        let _name_moji_size = rd.i32()?;
        let _name_newline = rd.i32()?;
        let _name_bracket = rd.i32()?;
        let talk_l = rd.i32()? as i64;
        let talk_t = rd.i32()? as i64;
        let talk_r = rd.i32()? as i64;
        let talk_b = rd.i32()? as i64;
        m.window_moji_cnt = Some((cnt_x, cnt_y));
        m.message_pos = Some((pos_x, pos_y));
        m.moji_space = Some((space_x, space_y));
        m.message_margin = Some((talk_l, talk_t, talk_r, talk_b));
        m.ruby_size = ruby_size;
        m.ruby_space = ruby_space;

        let chara_moji = rd.i32()? as i64;
        let chara_shadow = rd.i32()? as i64;
        let chara_fuchi = rd.i32()? as i64;
        let indent_pos = rd.i32()? as i64;
        let indent_u16 = rd.u16()?;
        let indent_count = rd.i32()? as i64;
        let cur_msg_type = rd.i32()? as i64;
        let ruby_x = rd.i32()? as i64;
        let ruby_y = rd.i32()? as i64;
        let disp_moji_cnt = rd.i32()? as i64;
        let hide_moji_cnt = rd.i32()? as i64;
        let debug_msg = rd.string()?;
        let ruby = rd.string()?;
        let btn_no = rd.i32()? as i64;
        let group_no = rd.i32()? as i64;
        let action_no = rd.i32()? as i64;
        let se_no = rd.i32()? as i64;
        let cur_msg_type_decided = rd.bool()?;
        let line_head = rd.bool()?;
        let ruby_start_ready = rd.bool()?;
        let button_flag = rd.bool()?;
        let mut glyphs = rd.extend_items(Self::read_cpp_mwnd_glyph)?;
        let mut body_index = 0usize;
        for glyph in &mut glyphs {
            if !glyph.ruby {
                body_index += 1;
            }
            glyph.reveal_index = body_index.max(1);
            if button_flag {
                glyph.message_button = Some(runtime::globals::MwndMessageButtonState {
                    btn_no,
                    group_no,
                    action_no,
                    se_no,
                });
            }
        }
        let msg_text = if !debug_msg.is_empty() {
            debug_msg
        } else {
            let units: Vec<u16> = glyphs
                .iter()
                .filter(|glyph| glyph.moji_type == 0 && !glyph.ruby)
                .map(|glyph| glyph.code as u16)
                .collect();
            String::from_utf16_lossy(&units)
        };
        Ok(runtime::globals::MwndMessagePageState {
            msg_text,
            glyphs,
            disp_moji_cnt,
            hide_moji_cnt,
            cur_msg_type,
            cur_msg_type_decided,
            ruby_start_pos: (ruby_x, ruby_y),
            ruby_start_ready,
            cursor_pos: (pos_x, pos_y),
            moji_rep_pos: (rep_x, rep_y),
            indent_pos,
            indent_moji: (indent_u16 != 0).then(|| char::from_u32(indent_u16 as u32).unwrap_or('\u{fffd}')),
            indent_count,
            line_head,
            ruby_pending: (!ruby.is_empty()).then_some(runtime::globals::MwndRubyPendingState {
                text: ruby,
                start_pos: Some((ruby_x, ruby_y)),
            }),
            moji_size: Some(moji_size),
            moji_color: (moji_color >= 0).then_some(moji_color),
            shadow_color: (shadow_color >= 0).then_some(shadow_color),
            fuchi_color: (fuchi_color >= 0).then_some(fuchi_color),
            chara_moji_color: (chara_moji >= 0).then_some(chara_moji),
            chara_shadow_color: (chara_shadow >= 0).then_some(chara_shadow),
            chara_fuchi_color: (chara_fuchi >= 0).then_some(chara_fuchi),
            msgbtn: button_flag.then_some((btn_no, group_no, action_no, se_no)),
        })
    }

    fn write_cpp_mwnd_waku(
        &self,
        w: &mut crate::original_save::OriginalStreamWriter,
        m: &runtime::globals::MwndState,
        name_waku: bool,
    ) {
        w.push_i32(Self::save_i32(m.msg_waku_no.unwrap_or(0)));
        w.push_str(if name_waku { "" } else { &m.waku_file });
        w.push_str(if name_waku { "" } else { &m.filter_file });
        let margin = if name_waku {
            (0, 0, 0, 0)
        } else {
            m.filter_margin.unwrap_or((0, 0, 0, 0))
        };
        for value in [margin.0, margin.1, margin.2, margin.3] {
            w.push_i32(Self::save_i32(value));
        }
        let (a, r, g, b) = if name_waku {
            (0, 0, 0, 0)
        } else {
            m.filter_color.unwrap_or((0, 0, 0, 0))
        };
        w.push_raw(&[b, g, r, a]);
        w.push_bool(!name_waku && m.filter_config_color);
        w.push_bool(!name_waku && m.filter_config_tr);
        // MSVC rounds STATE (22 bytes) up to four-byte alignment.
        w.push_padding(2);
        w.push_i32(Self::save_i32(m.msg_waku_no.unwrap_or(0)));
        w.push_i32(Self::save_i32(if name_waku { 0 } else { m.key_icon_mode }));
        let icon_pos = if name_waku { None } else { m.key_icon_pos };
        w.push_i32(Self::save_i32(icon_pos.map(|p| p.0).unwrap_or(0)));
        w.push_i32(Self::save_i32(icon_pos.map(|p| p.1).unwrap_or(0)));
        let faces: &[runtime::globals::ObjectState] = if name_waku { &[] } else { &m.face_list };
        let objects: &[runtime::globals::ObjectState] = if name_waku { &[] } else { &m.object_list };
        w.push_fixed_items(faces, |w, object| self.write_cpp_object(w, object));
        w.push_fixed_items(objects, |w, object| self.write_cpp_object(w, object));
    }

    fn read_cpp_mwnd_waku(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
        m: &mut runtime::globals::MwndState,
        name_waku: bool,
    ) -> Result<()> {
        let template_no = rd.i32()? as i64;
        let waku_file = rd.string()?;
        let filter_file = rd.string()?;
        let margin = (
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
        );
        let color = rd.take_raw(4)?;
        let filter_config_color = rd.bool()?;
        let filter_config_tr = rd.bool()?;
        rd.skip(2)?;
        let _key_template = rd.i32()?;
        let key_mode = rd.i32()? as i64;
        let key_x = rd.i32()? as i64;
        let key_y = rd.i32()? as i64;
        let faces = rd.fixed_items(Self::read_cpp_object)?;
        let objects = rd.fixed_items(Self::read_cpp_object)?;
        if !name_waku {
            m.msg_waku_no = Some(template_no);
            m.waku_file = waku_file;
            m.filter_file = filter_file;
            m.filter_margin = Some(margin);
            m.filter_color = Some((color[3], color[2], color[1], color[0]));
            m.filter_config_color = filter_config_color;
            m.filter_config_tr = filter_config_tr;
            m.key_icon_mode = key_mode;
            m.key_icon_pos = Some((key_x, key_y));
            m.face_list = faces;
            m.object_list = objects;
        }
        Ok(())
    }

    fn write_cpp_mwnd_name(
        w: &mut crate::original_save::OriginalStreamWriter,
        m: &runtime::globals::MwndState,
    ) {
        w.push_i32(0);
        let (space_x, space_y) = m.moji_space.unwrap_or((-1, 10));
        let (pos_x, pos_y) = m.name_message_pos;
        for value in [
            pos_x,
            pos_y,
            m.default_moji_size,
            space_x,
            space_y,
            m.name_text.chars().count() as i64,
            m.name_window_align,
            m.name_moji_color.unwrap_or(-1),
            m.name_shadow_color.unwrap_or(-1),
            m.name_fuchi_color.unwrap_or(-1),
        ] {
            w.push_i32(Self::save_i32(value));
        }
        w.push_str(&m.name_text);
        for value in [
            m.name_window_rect.0,
            m.name_window_rect.1,
            m.name_window_rect.2,
            m.name_window_rect.3,
        ] {
            w.push_i32(Self::save_i32(value));
        }
        w.push_extend_items(&m.name_glyphs, |w, glyph| Self::write_cpp_mwnd_glyph(w, glyph));
    }

    fn read_cpp_mwnd_name(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
        m: &mut runtime::globals::MwndState,
    ) -> Result<()> {
        let _template_no = rd.i32()?;
        let pos_x = rd.i32()? as i64;
        let pos_y = rd.i32()? as i64;
        let _size = rd.i32()?;
        let _space_x = rd.i32()?;
        let _space_y = rd.i32()?;
        let _cnt = rd.i32()?;
        m.name_window_align = rd.i32()? as i64;
        let name_moji_color = rd.i32()? as i64;
        let name_shadow_color = rd.i32()? as i64;
        let name_fuchi_color = rd.i32()? as i64;
        m.name_moji_color = (name_moji_color >= 0).then_some(name_moji_color);
        m.name_shadow_color = (name_shadow_color >= 0).then_some(name_shadow_color);
        m.name_fuchi_color = (name_fuchi_color >= 0).then_some(name_fuchi_color);
        m.name_text = rd.string()?;
        m.name_message_pos = (pos_x, pos_y);
        m.name_window_rect = (
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
        );
        m.name_glyphs = rd.extend_items(Self::read_cpp_mwnd_glyph)?;
        Ok(())
    }

    fn write_cpp_mwnd_selection(
        w: &mut crate::original_save::OriginalStreamWriter,
        m: &runtime::globals::MwndState,
    ) {
        w.push_i32(0);
        let (cnt_x, cnt_y) = m.window_moji_cnt.unwrap_or((0, 0));
        let (pos_x, pos_y) = m.message_pos.unwrap_or((0, 0));
        let (space_x, space_y) = m.moji_space.unwrap_or((-1, 10));
        for value in [
            cnt_x,
            cnt_y,
            pos_x,
            pos_y,
            m.default_moji_size,
            space_x,
            space_y,
            m.default_moji_color,
            m.default_shadow_color,
            m.default_fuchi_color,
        ] {
            w.push_i32(Self::save_i32(value));
        }
        let selection = m.selection.as_ref();
        w.push_i32(
            selection
                .map(|sel| sel.disp_item_count.min(i32::MAX as usize) as i32)
                .unwrap_or(0),
        );
        w.push_bool(selection.is_some_and(|sel| sel.cancel_enable));
        let choices: &[runtime::globals::MwndSelectionChoice] = selection
            .map(|sel| sel.choices.as_slice())
            .unwrap_or(&[]);
        w.push_u32(choices.len().min(u32::MAX as usize) as u32);
        for choice in choices {
            w.push_i32(Self::save_i32(choice.kind));
            w.push_i32(Self::save_i32(choice.pos.0));
            w.push_i32(Self::save_i32(choice.pos.1));
            w.push_str(&choice.text);
            let fallback_width = choice.text.chars().count() as i64 * m.default_moji_size;
            w.push_i32(Self::save_i32(if choice.size.0 != 0 { choice.size.0 } else { fallback_width }));
            w.push_i32(Self::save_i32(if choice.size.1 != 0 { choice.size.1 } else { m.default_moji_size }));
            w.push_extend_items(&choice.glyphs, |w, glyph| Self::write_cpp_mwnd_glyph(w, glyph));
        }
    }

    fn read_cpp_mwnd_selection(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
        m: &mut runtime::globals::MwndState,
    ) -> Result<()> {
        let _template = rd.i32()?;
        for _ in 0..10 {
            let _ = rd.i32()?;
        }
        let disp_item_count = rd.i32()?.max(0) as usize;
        let cancel_enable = rd.bool()?;
        let count = rd.i32()?.max(0) as usize;
        let mut choices = Vec::with_capacity(count);
        for _ in 0..count {
            let kind = rd.i32()? as i64;
            let x = rd.i32()? as i64;
            let y = rd.i32()? as i64;
            let text = rd.string()?;
            let sx = rd.i32()? as i64;
            let sy = rd.i32()? as i64;
            let glyphs = rd.extend_items(Self::read_cpp_mwnd_glyph)?;
            let color = glyphs
                .first()
                .map(|glyph| glyph.moji_color_no)
                .unwrap_or(m.default_moji_color);
            choices.push(runtime::globals::MwndSelectionChoice {
                text,
                kind,
                color,
                pos: (x, y),
                size: (sx, sy),
                glyphs,
            });
        }
        if !choices.is_empty() || cancel_enable {
            m.selection = Some(runtime::globals::MwndSelectionState {
                choices,
                disp_item_count,
                cursor: 0,
                cancel_enable,
                close_mwnd: false,
                result: 0,
            });
        }
        Ok(())
    }

    fn write_cpp_mwnd(&self, w: &mut crate::original_save::OriginalStreamWriter, m: &runtime::globals::MwndState) {
        // C_elm_mwnd::PARAM (43 consecutive i32 fields).
        let (window_x, window_y) = m.window_pos.unwrap_or((0, 0));
        let (window_w, window_h) = m.window_size.unwrap_or((0, 0));
        let (msg_x, msg_y) = m.message_pos.unwrap_or((0, 0));
        let (margin_l, margin_t, margin_r, margin_b) = m.message_margin.unwrap_or((0, 0, 0, 0));
        let (cnt_x, cnt_y) = m.window_moji_cnt.unwrap_or((0, 0));
        for value in [
            m.order,
            m.layer,
            m.world,
            m.novel_mode,
            m.mwnd_extend_type,
            window_x,
            window_y,
            window_w,
            window_h,
            msg_x,
            msg_y,
            margin_l,
            margin_t,
            margin_r,
            margin_b,
            cnt_x,
            cnt_y,
            m.name_disp_mode,
            m.name_bracket,
            m.name_extend_type,
            m.name_window_align,
            m.name_window_pos.0,
            m.name_window_pos.1,
            m.name_window_size.0,
            m.name_window_size.1,
            m.name_window_rect.0,
            m.name_window_rect.1,
            m.name_window_rect.2,
            m.name_window_rect.3,
            m.name_message_pos.0,
            m.name_message_pos.1,
            m.name_message_pos_rep.0,
            m.name_message_pos_rep.1,
            m.name_message_margin.0,
            m.name_message_margin.1,
            m.name_message_margin.2,
            m.name_message_margin.3,
            m.overflow_check_size,
            m.face_hide_name,
            m.open_anime_type,
            m.open_anime_time,
            m.close_anime_type,
            m.close_anime_time,
        ] {
            w.push_i32(Self::save_i32(value));
        }
        w.push_i32(Self::save_i32(m.time));
        w.push_bool(m.msg_block_started);
        w.push_bool(m.auto_proc_ready);
        w.push_bool(m.window_appear || m.open);
        w.push_bool(m.name_appear || !m.name_text.is_empty());
        w.push_bool(m.clear_ready);
        w.push_i32(Self::save_i32(m.auto_mode_end_moji_cnt));
        w.push_i32(Self::save_i32(m.target_msg_no));
        w.push_bool(m.slide_msg);
        w.push_i32(Self::save_i32(m.slide_time));
        w.push_i32(m.koe.map(|value| Self::save_i32(value.0)).unwrap_or(-1));
        w.push_bool(m.koe_play_flag || m.koe.is_some());
        w.push_i32(Self::save_i32(m.open_anime_type));
        w.push_i32(Self::save_i32(m.open_anime_time));
        w.push_i32(Self::save_i32(m.open_anime_start_time));
        w.push_i32(Self::save_i32(m.close_anime_type));
        w.push_i32(Self::save_i32(m.close_anime_time));
        w.push_i32(Self::save_i32(m.close_anime_start_time));

        w.push_i32((m.message_pages.len() + 1).min(i32::MAX as usize) as i32);
        for page in &m.message_pages {
            Self::write_cpp_mwnd_message(w, m, page);
        }
        let active = Self::active_mwnd_message_page(m);
        Self::write_cpp_mwnd_message(w, m, &active);
        self.write_cpp_mwnd_waku(w, m, false);
        Self::write_cpp_mwnd_name(w, m);
        self.write_cpp_mwnd_waku(w, m, true);
        Self::write_cpp_mwnd_selection(w, m);
    }

    fn read_cpp_mwnd(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<runtime::globals::MwndState> {
        let mut m = runtime::globals::MwndState::default();
        m.order = rd.i32()? as i64;
        m.layer = rd.i32()? as i64;
        m.world = rd.i32()? as i64;
        m.novel_mode = rd.i32()? as i64;
        m.mwnd_extend_type = rd.i32()? as i64;
        m.window_pos = Some((rd.i32()? as i64, rd.i32()? as i64));
        m.window_size = Some((rd.i32()? as i64, rd.i32()? as i64));
        m.message_pos = Some((rd.i32()? as i64, rd.i32()? as i64));
        m.message_margin = Some((
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
        ));
        m.window_moji_cnt = Some((rd.i32()? as i64, rd.i32()? as i64));
        m.name_disp_mode = rd.i32()? as i64;
        m.name_bracket = rd.i32()? as i64;
        m.name_extend_type = rd.i32()? as i64;
        m.name_window_align = rd.i32()? as i64;
        m.name_window_pos = (rd.i32()? as i64, rd.i32()? as i64);
        m.name_window_size = (rd.i32()? as i64, rd.i32()? as i64);
        m.name_window_rect = (
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
        );
        m.name_message_pos = (rd.i32()? as i64, rd.i32()? as i64);
        m.name_message_pos_rep = (rd.i32()? as i64, rd.i32()? as i64);
        m.name_message_margin = (
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
            rd.i32()? as i64,
        );
        m.overflow_check_size = rd.i32()? as i64;
        m.face_hide_name = rd.i32()? as i64;
        m.open_anime_type = rd.i32()? as i64;
        m.open_anime_time = rd.i32()? as i64;
        m.close_anime_type = rd.i32()? as i64;
        m.close_anime_time = rd.i32()? as i64;
        m.time = rd.i32()? as i64;
        m.msg_block_started = rd.bool()?;
        m.auto_proc_ready = rd.bool()?;
        m.window_appear = rd.bool()?;
        m.open = m.window_appear;
        m.name_appear = rd.bool()?;
        m.clear_ready = rd.bool()?;
        m.auto_mode_end_moji_cnt = rd.i32()? as i64;
        m.target_msg_no = rd.i32()? as i64;
        m.slide_msg = rd.bool()?;
        m.slide_time = rd.i32()? as i64;
        let koe_no = rd.i32()? as i64;
        m.koe_play_flag = rd.bool()?;
        if m.koe_play_flag {
            m.koe = Some((koe_no, 0));
        }
        m.open_anime_type = rd.i32()? as i64;
        m.open_anime_time = rd.i32()? as i64;
        m.open_anime_start_time = rd.i32()? as i64;
        m.close_anime_type = rd.i32()? as i64;
        m.close_anime_time = rd.i32()? as i64;
        m.close_anime_start_time = rd.i32()? as i64;

        let message_count = rd.i32()?.max(0) as usize;
        let mut pages = Vec::with_capacity(message_count.max(1));
        for _ in 0..message_count {
            pages.push(Self::read_cpp_mwnd_message(rd, &mut m)?);
        }
        let active_index = m
            .target_msg_no
            .clamp(0, pages.len().saturating_sub(1) as i64) as usize;
        if let Some(active) = pages.get(active_index).cloned() {
            m.message_pages = pages[..active_index].to_vec();
            m.msg_text = active.msg_text;
            m.glyphs = active.glyphs;
            m.disp_moji_cnt = active.disp_moji_cnt;
            m.hide_moji_cnt = active.hide_moji_cnt;
            m.cur_msg_type = active.cur_msg_type;
            m.cur_msg_type_decided = active.cur_msg_type_decided;
            m.ruby_start_pos = active.ruby_start_pos;
            m.ruby_start_ready = active.ruby_start_ready;
            m.cursor_pos = active.cursor_pos;
            m.moji_rep_pos = active.moji_rep_pos;
            m.indent_pos = active.indent_pos;
            m.indent_moji = active.indent_moji;
            m.indent_count = active.indent_count;
            m.line_head = active.line_head;
            m.ruby_pending = active.ruby_pending;
            m.moji_size = active.moji_size;
            m.moji_color = active.moji_color;
            m.shadow_color = active.shadow_color;
            m.fuchi_color = active.fuchi_color;
            m.chara_moji_color = active.chara_moji_color;
            m.chara_shadow_color = active.chara_shadow_color;
            m.chara_fuchi_color = active.chara_fuchi_color;
            m.msgbtn = active.msgbtn;
        }
        let mut reveal_index = 0usize;
        for page in &mut m.message_pages {
            for glyph in &mut page.glyphs {
                if !glyph.ruby {
                    reveal_index += 1;
                }
                glyph.reveal_index = reveal_index.max(1);
            }
        }
        for glyph in &mut m.glyphs {
            if !glyph.ruby {
                reveal_index += 1;
            }
            glyph.reveal_index = reveal_index.max(1);
        }
        Self::read_cpp_mwnd_waku(rd, &mut m, false)?;
        Self::read_cpp_mwnd_name(rd, &mut m)?;
        Self::read_cpp_mwnd_waku(rd, &mut m, true)?;
        Self::read_cpp_mwnd_selection(rd, &mut m)?;
        Ok(m)
    }

    fn write_cpp_world(&self, w: &mut crate::original_save::OriginalStreamWriter, world: &runtime::globals::WorldState) {
        w.push_i32(world.mode);
        for e in [&world.camera_eye_x, &world.camera_eye_y, &world.camera_eye_z, &world.camera_pint_x, &world.camera_pint_y, &world.camera_pint_z, &world.camera_up_x, &world.camera_up_y, &world.camera_up_z] {
            Self::write_cpp_int_event_raw(w, e);
        }
        for v in [world.camera_view_angle, world.mono, world.order, world.layer, world.wipe_copy, world.wipe_erase] { w.push_i32(v); }
    }

    fn read_cpp_world(rd: &mut crate::original_save::OriginalStreamReader<'_>, world_no: i32) -> Result<runtime::globals::WorldState> {
        let mut world = runtime::globals::WorldState::new(world_no);
        world.mode = rd.i32()?;
        world.camera_eye_x = Self::read_cpp_int_event_raw(rd)?;
        world.camera_eye_y = Self::read_cpp_int_event_raw(rd)?;
        world.camera_eye_z = Self::read_cpp_int_event_raw(rd)?;
        world.camera_pint_x = Self::read_cpp_int_event_raw(rd)?;
        world.camera_pint_y = Self::read_cpp_int_event_raw(rd)?;
        world.camera_pint_z = Self::read_cpp_int_event_raw(rd)?;
        world.camera_up_x = Self::read_cpp_int_event_raw(rd)?;
        world.camera_up_y = Self::read_cpp_int_event_raw(rd)?;
        world.camera_up_z = Self::read_cpp_int_event_raw(rd)?;
        world.camera_view_angle = rd.i32()?;
        world.mono = rd.i32()?;
        world.order = rd.i32()?;
        world.layer = rd.i32()?;
        world.wipe_copy = rd.i32()?;
        world.wipe_erase = rd.i32()?;
        Ok(world)
    }

    fn write_cpp_effect(&self, w: &mut crate::original_save::OriginalStreamWriter, e: &runtime::globals::ScreenEffectState) {
        for ev in [&e.x, &e.y, &e.z, &e.mono, &e.reverse, &e.bright, &e.dark, &e.color_r, &e.color_g, &e.color_b, &e.color_rate, &e.color_add_r, &e.color_add_g, &e.color_add_b] {
            Self::write_cpp_int_event_raw(w, ev);
        }
        for v in [e.begin_order, e.end_order, e.begin_layer, e.end_layer, e.wipe_copy, e.wipe_erase] { w.push_i32(v); }
    }

    fn read_cpp_effect(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<runtime::globals::ScreenEffectState> {
        let mut e = runtime::globals::ScreenEffectState::default();
        e.x = Self::read_cpp_int_event_raw(rd)?;
        e.y = Self::read_cpp_int_event_raw(rd)?;
        e.z = Self::read_cpp_int_event_raw(rd)?;
        e.mono = Self::read_cpp_int_event_raw(rd)?;
        e.reverse = Self::read_cpp_int_event_raw(rd)?;
        e.bright = Self::read_cpp_int_event_raw(rd)?;
        e.dark = Self::read_cpp_int_event_raw(rd)?;
        e.color_r = Self::read_cpp_int_event_raw(rd)?;
        e.color_g = Self::read_cpp_int_event_raw(rd)?;
        e.color_b = Self::read_cpp_int_event_raw(rd)?;
        e.color_rate = Self::read_cpp_int_event_raw(rd)?;
        e.color_add_r = Self::read_cpp_int_event_raw(rd)?;
        e.color_add_g = Self::read_cpp_int_event_raw(rd)?;
        e.color_add_b = Self::read_cpp_int_event_raw(rd)?;
        e.begin_order = rd.i32()?;
        e.end_order = rd.i32()?;
        e.begin_layer = rd.i32()?;
        e.end_layer = rd.i32()?;
        e.wipe_copy = rd.i32()?;
        e.wipe_erase = rd.i32()?;
        Ok(e)
    }

    fn write_cpp_quake(&self, w: &mut crate::original_save::OriginalStreamWriter, q: &runtime::globals::ScreenQuakeState) {
        w.push_i32(q.quake_type);
        w.push_i32(q.vec);
        w.push_i32(q.power);
        w.push_i32(q.cur_time);
        w.push_i32(q.total_time);
        w.push_i32(if q.ending { 1 } else { 0 });
        w.push_i32(q.end_cur_time);
        w.push_i32(q.end_total_time);
        w.push_i32(q.cnt);
        w.push_i32(q.end_cnt);
        w.push_i32(q.center_x);
        w.push_i32(q.center_y);
        w.push_i32(q.begin_order);
        w.push_i32(q.end_order);
    }

    fn read_cpp_quake(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<runtime::globals::ScreenQuakeState> {
        let mut q = runtime::globals::ScreenQuakeState::default();
        q.quake_type = rd.i32()?;
        q.vec = rd.i32()?;
        q.power = rd.i32()?;
        q.cur_time = rd.i32()?;
        q.total_time = rd.i32()?;
        q.ending = rd.i32()? != 0;
        q.end_cur_time = rd.i32()?;
        q.end_total_time = rd.i32()?;
        q.cnt = rd.i32()?;
        q.end_cnt = rd.i32()?;
        q.center_x = rd.i32()?;
        q.center_y = rd.i32()?;
        q.begin_order = rd.i32()?;
        q.end_order = rd.i32()?;
        Ok(q)
    }

    fn cpp_btn_select_param(
        &self,
        state: &runtime::globals::BtnSelectRuntimeState,
    ) -> [i64; 28] {
        if let Some(saved) = state.saved_cur_param {
            return saved;
        }

        // C_elm_btn_select::init initializes an empty m_def/m_cur with only
        // btn_action_no set to -1.  set_template() then replaces every field
        // below from Gameexe.ini and copies m_def into m_cur.
        let mut param = [0i64; 28];
        param[20] = -1;
        let Some(tmpl) = (state.template_no >= 0)
            .then(|| self.ctx.tables.sel_btn_templates.get(state.template_no as usize))
            .flatten()
        else {
            return param;
        };
        param = [
            tmpl.base_pos.0,
            tmpl.base_pos.1,
            tmpl.rep_pos.0,
            tmpl.rep_pos.1,
            tmpl.x_align,
            tmpl.y_align,
            tmpl.max_y_cnt,
            tmpl.line_width,
            tmpl.moji_cnt,
            tmpl.moji_pos.0,
            tmpl.moji_pos.1,
            tmpl.moji_size,
            tmpl.moji_space.0,
            tmpl.moji_space.1,
            tmpl.moji_x_align,
            tmpl.moji_y_align,
            tmpl.moji_color,
            tmpl.moji_hit_color,
            self.ctx.tables.mwnd_render.shadow_color,
            self.ctx.tables.mwnd_render.fuchi_color,
            tmpl.btn_action_no,
            tmpl.open_anime_type,
            tmpl.open_anime_time,
            tmpl.close_anime_type,
            tmpl.close_anime_time,
            tmpl.decide_anime_type,
            tmpl.decide_anime_time,
            state.sync_type,
        ];
        param
    }

    fn synthesize_cpp_btn_select_glyphs(
        text: &str,
        param: &[i64; 28],
        color: i64,
        appeared: bool,
    ) -> Vec<runtime::globals::MwndGlyphState> {
        let size = param[11];
        let space_x = param[12];
        let mut x = 0i64;
        let mut glyphs = Vec::with_capacity(text.chars().count());
        for ch in text.chars() {
            glyphs.push(runtime::globals::MwndGlyphState {
                moji_type: 0,
                code: ch as u32 as i32,
                ch,
                x: param[9].saturating_add(x),
                y: param[10],
                size,
                moji_color_no: color,
                shadow_color_no: param[18],
                fuchi_color_no: param[19],
                shadow: param[18] >= 0,
                fuchi: param[19] >= 0,
                bold: false,
                reveal_index: glyphs.len().saturating_add(1),
                ruby: false,
                appeared,
                message_button: None,
            });
            let full_advance = size.saturating_add(space_x);
            let half_width = ch.is_ascii() || matches!(ch as u32, 0xff61..=0xff9f);
            x = x.saturating_add(if half_width { full_advance / 2 } else { full_advance });
        }

        // C_elm_btn_select_item::set_msg removes one trailing spacing unit
        // before applying horizontal/vertical alignment to every glyph.
        let total_x = if glyphs.is_empty() { 0 } else { x.saturating_sub(space_x) };
        let rep_x = match param[14] {
            1 => -total_x / 2,
            2 => -total_x,
            _ => 0,
        };
        let rep_y = match param[15] {
            1 => -size / 2,
            2 => -size,
            _ => 0,
        };
        for glyph in &mut glyphs {
            glyph.x = glyph.x.saturating_add(rep_x);
            glyph.y = glyph.y.saturating_add(rep_y);
        }
        glyphs
    }

    fn write_cpp_btn_select(
        &self,
        w: &mut crate::original_save::OriginalStreamWriter,
        state: Option<&runtime::globals::BtnSelectRuntimeState>,
    ) {
        let empty = runtime::globals::BtnSelectRuntimeState::default();
        let state = state.unwrap_or(&empty);
        let param = self.cpp_btn_select_param(state);
        w.push_i32(Self::save_i32(state.template_no));
        for value in param {
            w.push_i32(Self::save_i32(value));
        }
        w.push_bool(state.appear_flag);
        w.push_bool(state.processing_flag_0);
        w.push_bool(state.processing_flag_1);
        w.push_bool(state.processing_flag_2);
        w.push_bool(state.cancel_enable);
        w.push_bool(state.capture_flag);
        w.push_str(&state.sel_start_call_scn);
        w.push_i32(Self::save_i32(state.sel_start_call_z_no));

        let loaded = state.saved_cur_param.is_some();
        w.push_extend_items(&state.choices, |w, choice| {
            let item_template_no = if choice.template_no >= 0 {
                choice.template_no
            } else {
                state.template_no
            };
            let item_template = (item_template_no >= 0)
                .then(|| self.ctx.tables.sel_btn_templates.get(item_template_no as usize))
                .flatten();
            let base_file = if loaded || !choice.base_file.is_empty() {
                choice.base_file.as_str()
            } else {
                item_template.map(|t| t.base_file.as_str()).unwrap_or("")
            };
            let filter_file = if loaded || !choice.filter_file.is_empty() {
                choice.filter_file.as_str()
            } else {
                item_template.map(|t| t.filter_file.as_str()).unwrap_or("")
            };
            let color = if choice.color >= 0 { choice.color } else { param[16] };
            let relative_x = choice.pos.0.saturating_sub(param[0]);
            let relative_y = choice.pos.1.saturating_sub(param[1]);
            let generated;
            let glyphs = if choice.glyphs.is_empty() {
                generated = Self::synthesize_cpp_btn_select_glyphs(
                    &choice.text,
                    &param,
                    color,
                    state.appear_flag
                        || state.processing_flag_0
                        || state.processing_flag_1
                        || state.processing_flag_2,
                );
                generated.as_slice()
            } else {
                choice.glyphs.as_slice()
            };

            w.push_i32(Self::save_i32(item_template_no));
            w.push_str(base_file);
            w.push_str(filter_file);
            w.push_str(&choice.text);
            w.push_i32(Self::save_i32(choice.item_type));
            w.push_i32(Self::save_i32(color));
            w.push_i32(Self::save_i32(relative_x));
            w.push_i32(Self::save_i32(relative_y));
            w.push_extend_items(glyphs, |w, glyph| Self::write_cpp_mwnd_glyph(w, glyph));
        });
    }

    fn read_cpp_btn_select(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<runtime::globals::BtnSelectRuntimeState> {
        let template_no = rd.i32()? as i64;
        let mut param = [0i64; 28];
        for value in &mut param {
            *value = rd.i32()? as i64;
        }
        let appear_flag = rd.bool()?;
        let processing_flag_0 = rd.bool()?;
        let processing_flag_1 = rd.bool()?;
        let processing_flag_2 = rd.bool()?;
        let cancel_enable = rd.bool()?;
        let capture_flag = rd.bool()?;
        let sel_start_call_scn = rd.string()?;
        let sel_start_call_z_no = rd.i32()? as i64;
        let choices = rd.extend_items(|rd| {
            let item_template_no = rd.i32()? as i64;
            let base_file = rd.string()?;
            let filter_file = rd.string()?;
            let text = rd.string()?;
            let item_type = rd.i32()? as i64;
            let color = rd.i32()? as i64;
            let relative_x = rd.i32()? as i64;
            let relative_y = rd.i32()? as i64;
            let glyphs = rd.extend_items(Self::read_cpp_mwnd_glyph)?;
            Ok(runtime::globals::BtnSelectChoiceState {
                template_no: item_template_no,
                base_file,
                filter_file,
                text,
                item_type,
                color,
                pos: (
                    param[0].saturating_add(relative_x),
                    param[1].saturating_add(relative_y),
                ),
                size: (0, 0),
                glyphs,
            })
        })?;
        let cursor = choices
            .iter()
            .position(|choice| choice.item_type == 1)
            .unwrap_or(0);

        Ok(runtime::globals::BtnSelectRuntimeState {
            template_no,
            saved_cur_param: Some(param),
            choices,
            cursor,
            pressed_index: None,
            pressed_inside: false,
            cancel_enable,
            capture_flag,
            // load() calls restruct_template() after init_work_variable().  The
            // saved processing flags are restored, but m_sync_type and every
            // animation work member remain zero exactly as in the C++ object.
            started: processing_flag_0,
            result: 0,
            sync_type: 0,
            read_flag_scene_no: -1,
            read_flag_flag_no: -1,
            sel_start_call_scn,
            sel_start_call_z_no,
            appear_flag,
            open_anime_type: 0,
            open_anime_time: 0,
            open_anime_cur_time: 0,
            close_anime_type: 0,
            close_anime_time: 0,
            close_anime_cur_time: 0,
            decide_anime_type: 0,
            decide_anime_time: 0,
            decide_anime_cur_time: 0,
            decide_sel_no: -1,
            processing_flag_0,
            processing_flag_1,
            processing_flag_2,
            capture_now_flag: false,
            result_delivered: false,
        })
    }

    fn write_cpp_stage(&self, w: &mut crate::original_save::OriginalStreamWriter, stage_idx: i64) {
        let form_id = if self.ctx.ids.form_global_stage != 0 {
            self.ctx.ids.form_global_stage
        } else {
            runtime::forms::codes::FORM_GLOBAL_STAGE
        };
        let st = self.ctx.globals.stage_forms.get(&form_id);
        let empty_groups: Vec<runtime::globals::GroupState> = Vec::new();
        let empty_objects: Vec<runtime::globals::ObjectState> = Vec::new();
        let empty_mwnds: Vec<runtime::globals::MwndState> = Vec::new();
        let empty_worlds: Vec<runtime::globals::WorldState> = Vec::new();
        let empty_effects: Vec<runtime::globals::ScreenEffectState> = Vec::new();
        let empty_quakes: Vec<runtime::globals::ScreenQuakeState> = Vec::new();
        let groups = st.and_then(|s| s.group_lists.get(&stage_idx)).unwrap_or(&empty_groups);
        let objects = st.and_then(|s| s.object_lists.get(&stage_idx)).unwrap_or(&empty_objects);
        let mwnds = st.and_then(|s| s.mwnd_lists.get(&stage_idx)).unwrap_or(&empty_mwnds);
        let worlds = st.and_then(|s| s.world_lists.get(&stage_idx)).unwrap_or(&empty_worlds);
        let effects = st.and_then(|s| s.effect_lists.get(&stage_idx)).unwrap_or(&empty_effects);
        let quakes = st.and_then(|s| s.quake_lists.get(&stage_idx)).unwrap_or(&empty_quakes);
        w.push_fixed_items(groups, |w, g| self.write_cpp_group(w, g));
        w.push_fixed_items(objects, |w, obj| self.write_cpp_object(w, obj));
        w.push_fixed_items(mwnds, |w, m| self.write_cpp_mwnd(w, m));
        let btn_select = if stage_idx == 1 {
            Some(&self.ctx.globals.selbtn)
        } else {
            st.and_then(|stage| stage.btn_select_states.get(&stage_idx))
        };
        self.write_cpp_btn_select(w, btn_select);
        w.push_fixed_items(worlds, |w, world| self.write_cpp_world(w, world));
        w.push_fixed_items(effects, |w, e| self.write_cpp_effect(w, e));
        w.push_fixed_items(quakes, |w, q| self.write_cpp_quake(w, q));
    }

    fn read_cpp_stage(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
        stage_idx: i64,
    ) -> Result<(
        runtime::globals::StageFormState,
        runtime::globals::BtnSelectRuntimeState,
    )> {
        let mut st = runtime::globals::StageFormState::default();
        st.initialized_from_gameexe = true;
        st.group_lists.insert(stage_idx, rd.fixed_items(|rd| Self::read_cpp_group(rd))?);
        st.object_lists.insert(stage_idx, rd.fixed_items(|rd| Self::read_cpp_object(rd))?);
        st.mwnd_lists.insert(stage_idx, rd.fixed_items(|rd| Self::read_cpp_mwnd(rd))?);
        let btn_select = Self::read_cpp_btn_select(rd)?;
        st.btn_select_states.insert(stage_idx, btn_select.clone());
        let mut world_no = 0i32;
        let worlds = rd.fixed_items(|rd| { let w = Self::read_cpp_world(rd, world_no); world_no += 1; w })?;
        st.world_lists.insert(stage_idx, worlds);
        st.effect_lists.insert(stage_idx, rd.fixed_items(|rd| Self::read_cpp_effect(rd))?);
        st.quake_lists.insert(stage_idx, rd.fixed_items(|rd| Self::read_cpp_quake(rd))?);
        Ok((st, btn_select))
    }

    fn write_cpp_screen(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        let form_id = self.ctx.ids.form_global_screen;
        let screen = self.ctx.globals.screen_forms.get(&form_id).cloned().unwrap_or_default();
        w.push_fixed_items(&screen.effect_list, |w, e| self.write_cpp_effect(w, e));
        w.push_i32(screen.shake.shake_no);
        w.push_i32(screen.shake.cur_time);
        w.push_i32(screen.shake.cur_x);
        w.push_i32(screen.shake.cur_y);
        w.push_fixed_items(&screen.quake_list, |w, q| self.write_cpp_quake(w, q));
    }

    fn read_cpp_screen(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<runtime::globals::ScreenFormState> {
        let effect_list = rd.fixed_items(|rd| Self::read_cpp_effect(rd))?;
        let mut shake = runtime::globals::ScreenShakeState::default();
        shake.shake_no = rd.i32()?;
        shake.cur_time = rd.i32()?;
        shake.cur_x = rd.i32()?;
        shake.cur_y = rd.i32()?;
        let quake_list = rd.fixed_items(|rd| Self::read_cpp_quake(rd))?;
        Ok(runtime::globals::ScreenFormState { effect_list, quake_list, shake })
    }

    const ORIGINAL_PCMCH_DEFAULT_CNT: usize = 16;
    const ORIGINAL_PCMCH_MAX_CNT: usize = 256;

    fn original_pcmch_count(&self) -> usize {
        self.ctx
            .tables
            .gameexe
            .as_ref()
            .and_then(|cfg| cfg.get_usize("#PCMCH.CNT").or_else(|| cfg.get_usize("PCMCH.CNT")))
            .unwrap_or(Self::ORIGINAL_PCMCH_DEFAULT_CNT)
            .min(Self::ORIGINAL_PCMCH_MAX_CNT)
    }

    fn write_cpp_pcmch(
        &self,
        w: &mut crate::original_save::OriginalStreamWriter,
        ch: usize,
    ) {
        let state = self
            .ctx
            .globals
            .pcmch_persistent
            .get(ch)
            .cloned()
            .unwrap_or_default();
        w.push_str(&state.pcm_name);
        w.push_str(&state.bgm_name);
        w.push_i32(Self::save_i32(state.koe_no));
        w.push_i32(Self::save_i32(state.se_no));
        w.push_i32(Self::save_i32(state.volume_type));
        w.push_i32(Self::save_i32(state.chara_no));
        w.push_i32(self.ctx.pcm.slot_volume_raw(ch) as i32);
        w.push_i32(Self::save_i32(self.ctx.pcm.slot_resume_delay_ms(ch)));
        w.push_bool(state.loop_flag);
        w.push_bool(state.bgm_fade_target_flag);
        w.push_bool(state.bgm_fade2_target_flag);
        w.push_bool(state.bgm_fade_source_flag);
        w.push_bool(state.ready_flag);
    }

    fn read_cpp_pcmch(
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<runtime::globals::PcmChPersistentState> {
        Ok(runtime::globals::PcmChPersistentState {
            pcm_name: rd.string()?,
            bgm_name: rd.string()?,
            koe_no: rd.i32()? as i64,
            se_no: rd.i32()? as i64,
            volume_type: rd.i32()? as i64,
            chara_no: rd.i32()? as i64,
            volume: rd.i32()? as i64,
            delay_time: rd.i32()? as i64,
            fade_in_time: 0,
            loop_flag: rd.bool()?,
            bgm_fade_target_flag: rd.bool()?,
            bgm_fade2_target_flag: rd.bool()?,
            bgm_fade_source_flag: rd.bool()?,
            ready_flag: rd.bool()?,
        })
    }

    fn write_cpp_sound(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        // Exact C_elm_sound::save order: BGM, KOE, PCM, PCMCHLIST, SE, MOV.
        w.push_str(self.ctx.bgm.current_name().unwrap_or(""));
        w.push_i32(self.ctx.bgm.volume_raw() as i32);
        w.push_i32(Self::save_i32(self.ctx.bgm.save_delay_time_ms()));
        w.push_bool(self.ctx.bgm.save_loop_flag());
        w.push_bool(self.ctx.bgm.save_pause_flag());
        w.push_i32(self.ctx.koe.volume_raw() as i32);
        w.push_i32(self.ctx.pcm.volume_raw() as i32);
        let pcmch_count = self.original_pcmch_count();
        let channels: Vec<usize> = (0..pcmch_count).collect();
        w.push_fixed_items(&channels, |w, ch| self.write_cpp_pcmch(w, *ch));
        w.push_i32(self.ctx.se.volume_raw() as i32);
        w.push_str(self.ctx.globals.mov.file_name.as_deref().unwrap_or(""));
    }

    fn read_cpp_sound(
        &mut self,
        rd: &mut crate::original_save::OriginalStreamReader<'_>,
    ) -> Result<()> {
        // The original local-load path reinitializes C_elm_sound before these
        // records are applied.  Stop every live Rust backend first so a saved
        // silent/one-shot state cannot leave pre-load playback running.
        let _ = self.ctx.bgm.stop();
        let _ = self.ctx.koe.stop(None);
        let _ = self.ctx.pcm.stop_all(None);
        let _ = self.ctx.se.stop(None);
        self.ctx.movie.stop();

        let bgm_regist_name = rd.string()?;
        let bgm_volume = rd.i32()?.clamp(0, 255) as u8;
        let bgm_delay_time = rd.i32()?.max(0) as i64;
        let bgm_loop_flag = rd.bool()?;
        let bgm_pause_flag = rd.bool()?;
        let koe_volume = rd.i32()?.clamp(0, 255) as u8;
        let pcm_volume = rd.i32()?.clamp(0, 255) as u8;
        let pcmch = rd.fixed_items(|rd| Self::read_cpp_pcmch(rd))?;
        let se_volume = rd.i32()?.clamp(0, 255) as u8;
        let mov_file_name = rd.string()?;

        {
            let (bgm, audio) = (&mut self.ctx.bgm, &mut self.ctx.audio);
            bgm.set_volume_raw(audio, bgm_volume)?;
            if !bgm_regist_name.is_empty()
                && (bgm_loop_flag || bgm_pause_flag || bgm_delay_time > 0)
            {
                // C_elm_bgm::load uses delay only as the restore predicate and
                // calls play(..., ready_flag, 0); preserve that behavior.
                if let Err(err) = bgm.play_name_script(
                    audio,
                    &bgm_regist_name,
                    bgm_loop_flag,
                    0,
                    0,
                    -1,
                    bgm_pause_flag,
                    0,
                ) {
                    log::error!("failed to restore BGM {:?}: {err:#}", bgm_regist_name);
                }
            }
        }
        {
            let (koe, audio) = (&mut self.ctx.koe, &mut self.ctx.audio);
            koe.set_volume_raw(audio, koe_volume)?;
        }
        {
            let (pcm, audio) = (&mut self.ctx.pcm, &mut self.ctx.audio);
            pcm.set_volume_raw(audio, pcm_volume)?;
        }
        {
            let (se, audio) = (&mut self.ctx.se, &mut self.ctx.audio);
            se.set_volume_raw(audio, se_volume)?;
        }

        self.ctx.globals.pcmch_persistent.clear();
        for (ch, state) in pcmch.into_iter().enumerate() {
            if let Err(err) = crate::runtime::forms::pcmch::restore_persistent_channel(
                &mut self.ctx,
                ch,
                state,
            ) {
                log::error!("failed to restore PCMCH[{ch}]: {err:#}");
            }
        }

        if mov_file_name.is_empty() {
            self.ctx.globals.mov.file_name = None;
            self.ctx.movie.stop();
        } else {
            self.ctx.globals.mov.file_name = Some(mov_file_name.clone());
            self.ctx.globals.mov.playing = false;
            if let Err(err) = self.ctx.movie.prepare(&mov_file_name) {
                log::error!("failed to restructure saved MOV {:?}: {err:#}", mov_file_name);
            }
        }
        Ok(())
    }

    fn write_cpp_pcm_event(&self, w: &mut crate::original_save::OriginalStreamWriter, ev: &runtime::globals::PcmEventState) {
        let ty = ev.event_type;
        w.push_i32(ty);
        if ty == runtime::globals::PCM_EVENT_TYPE_LOOP
            || ty == runtime::globals::PCM_EVENT_TYPE_RANDOM
        {
            w.push_i32(ev.pcm_buf_no);
            w.push_i32(ev.volume_type);
            w.push_i32(ev.chara_no);
            w.push_bool(ev.bgm_fade_target_flag);
            w.push_bool(ev.bgm_fade2_target_flag);
            w.push_bool(ev.bgm_fade_source_flag);
            w.push_bool(ev.real_flag);
            w.push_bool(ev.time_type);
            w.push_extend_items(&ev.lines, |w, line| {
                w.push_str(&line.file_name);
                w.push_i32(line.min_time);
                w.push_i32(line.max_time);
                w.push_i32(line.probability);
            });
        }
    }

    fn read_cpp_pcm_event(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<runtime::globals::PcmEventState> {
        let ty = rd.i32()?;
        let mut ev = runtime::globals::PcmEventState::default();
        if ty == runtime::globals::PCM_EVENT_TYPE_LOOP
            || ty == runtime::globals::PCM_EVENT_TYPE_RANDOM
        {
            let pcm_buf_no = rd.i32()?;
            let volume_type = rd.i32()?;
            let chara_no = rd.i32()?;
            let bgm_fade_target_flag = rd.bool()?;
            let bgm_fade2_target_flag = rd.bool()?;
            let bgm_fade_source_flag = rd.bool()?;
            let real_flag = rd.bool()?;
            let time_type = rd.bool()?;
            ev.lines = rd.extend_items(|rd| Ok(runtime::globals::PcmEventLine {
                file_name: rd.string()?,
                min_time: rd.i32()?,
                max_time: rd.i32()?,
                probability: rd.i32()?,
            }))?;
            // C_elm_pcm_event::load restarts LOOP/RANDOM and intentionally
            // discards all scheduler working values. ONESHOT is not restored.
            ev.start(
                ty,
                pcm_buf_no,
                volume_type,
                chara_no,
                bgm_fade_target_flag,
                bgm_fade2_target_flag,
                bgm_fade_source_flag,
                real_flag,
                time_type,
            );
        }
        Ok(ev)
    }

    fn write_cpp_editbox(&self, w: &mut crate::original_save::OriginalStreamWriter, e: &runtime::globals::EditBoxState) {
        w.push_bool(e.created);
        w.push_i32(e.rect_x);
        w.push_i32(e.rect_y);
        w.push_i32(e.rect_w);
        w.push_i32(e.rect_h);
        w.push_i32(e.moji_size);
    }

    fn read_cpp_editbox(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<runtime::globals::EditBoxState> {
        let mut e = runtime::globals::EditBoxState::default();
        e.created = rd.bool()?;
        e.rect_x = rd.i32()?;
        e.rect_y = rd.i32()?;
        e.rect_w = rd.i32()?;
        e.rect_h = rd.i32()?;
        e.moji_size = rd.i32()?;
        e.text.clear();
        Ok(e)
    }

    fn write_cpp_msg_back(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        let msgbk = self.ctx.globals.msgbk_forms.values().next().cloned().unwrap_or_default();
        w.push_i32(msgbk.history_cnt as i32);
        let count = msgbk.history_cnt.min(msgbk.history.len());
        for entry in msgbk.history.iter().take(count) {
            w.push_bool(entry.pct_flag);
            w.push_str(&entry.msg_str);
            w.push_str(&entry.original_name);
            w.push_str(&entry.disp_name);
            w.push_i32(entry.pct_pos_x);
            w.push_i32(entry.pct_pos_y);
            w.push_extend_i32_list(&entry.koe_no_list);
            w.push_extend_i32_list(&entry.chr_no_list);
            w.push_i32(Self::save_i32(entry.koe_play_no));
            w.push_str(&entry.debug_msg);
            w.push_i32(Self::save_i32(entry.scn_no));
            w.push_i32(Self::save_i32(entry.line_no));
            w.push_tid_zero();
            w.push_bool(entry.save_id_check_flag);
        }
        w.push_i32(msgbk.history_start_pos as i32);
        w.push_i32(msgbk.history_last_pos as i32);
        w.push_i32(msgbk.history_insert_pos as i32);
        w.push_i32(if msgbk.new_msg_flag { 1 } else { 0 });
    }

    fn read_cpp_msg_back(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<runtime::globals::MsgBackState> {
        let cnt = rd.i32()?.max(0) as usize;
        let mut st = runtime::globals::MsgBackState::default();
        st.history.clear();
        for _ in 0..cnt {
            let mut entry = runtime::globals::MsgBackEntry::default();
            entry.pct_flag = rd.bool()?;
            entry.msg_str = rd.string()?;
            entry.original_name = rd.string()?;
            entry.disp_name = rd.string()?;
            entry.pct_pos_x = rd.i32()?;
            entry.pct_pos_y = rd.i32()?;
            entry.koe_no_list = rd.extend_i32_list()?;
            entry.chr_no_list = rd.extend_i32_list()?;
            entry.koe_play_no = rd.i32()? as i64;
            entry.debug_msg = rd.string()?;
            entry.scn_no = rd.i32()? as i64;
            entry.line_no = rd.i32()? as i64;
            rd.skip(14)?;
            entry.save_id_check_flag = rd.bool()?;
            st.history.push(entry);
        }
        st.history_cnt = cnt;
        st.history_cnt_max = cnt.max(256);
        st.history_start_pos = rd.i32()?.max(0) as usize;
        st.history_last_pos = rd.i32()?.max(0) as usize;
        st.history_insert_pos = rd.i32()?.max(0) as usize;
        st.new_msg_flag = rd.i32()? != 0;
        if st.history.len() < st.history_cnt_max { st.history.resize_with(st.history_cnt_max, runtime::globals::MsgBackEntry::default); }
        Ok(st)
    }


    fn parse_cpp_tail_state(&mut self, rd: &mut crate::original_save::OriginalStreamReader<'_>, current_scene_name: &str) -> Result<Vec<CallFrame>> {
        self.read_cpp_inc_prop_list(rd)?;
        self.read_cpp_scene_prop_lists(rd, current_scene_name)?;

        let counter_list = rd.fixed_items(|rd| Self::read_cpp_counter_param(rd))?;
        if !counter_list.is_empty() {
            self.ctx.globals.counter_lists.insert(crate::runtime::forms::codes::FORM_GLOBAL_COUNTER, counter_list);
        }

        let frame_action = Self::read_cpp_frame_action(rd)?;
        self.ctx.globals.frame_actions.insert(self.ctx.ids.form_global_frame_action, frame_action);

        let frame_action_ch = rd.fixed_items(|rd| Self::read_cpp_frame_action(rd))?;
        if !frame_action_ch.is_empty() {
            self.ctx.globals.frame_action_lists.insert(self.ctx.ids.form_global_frame_action_ch, frame_action_ch);
        }

        let g00buf_files = rd.fixed_items(|rd| rd.string())?;
        self.ctx.globals.g00buf.clear();
        self.ctx.globals.g00buf_names.clear();
        self.ctx.globals.g00buf.resize(g00buf_files.len(), None);
        self.ctx.globals.g00buf_names.resize(g00buf_files.len(), None);
        for (idx, name) in g00buf_files.into_iter().enumerate() {
            if !name.is_empty() {
                self.ctx.globals.g00buf_names[idx] = Some(name.clone());
                if let Ok(img_id) = self.ctx.images.load_g00(&name, 0) {
                    self.ctx.globals.g00buf[idx] = Some(img_id);
                }
            }
        }

        let masks = rd.fixed_items(|rd| {
            let x_event = Self::read_cpp_int_event_raw(rd)?;
            let y_event = Self::read_cpp_int_event_raw(rd)?;
            let name = rd.string()?;
            Ok(runtime::globals::MaskState {
                name: if name.is_empty() { None } else { Some(name) },
                x_event,
                y_event,
                extra_int: std::collections::HashMap::new(),
                script_events: std::collections::HashMap::new(),
            })
        })?;
        if !masks.is_empty() {
            self.ctx.globals.mask_lists.insert(self.ctx.ids.form_global_mask, runtime::globals::MaskListState { masks });
        }

        let mut st = runtime::globals::StageFormState::default();
        let (back, back_btn_select) = Self::read_cpp_stage(rd, 0)?;
        let (front, front_btn_select) = Self::read_cpp_stage(rd, 1)?;
        st.initialized_from_gameexe = true;
        st.group_lists.extend(back.group_lists);
        st.object_lists.extend(back.object_lists);
        st.mwnd_lists.extend(back.mwnd_lists);
        st.btn_select_states.extend(back.btn_select_states);
        st.world_lists.extend(back.world_lists);
        st.effect_lists.extend(back.effect_lists);
        st.quake_lists.extend(back.quake_lists);
        st.group_lists.extend(front.group_lists);
        st.object_lists.extend(front.object_lists);
        st.mwnd_lists.extend(front.mwnd_lists);
        st.btn_select_states.extend(front.btn_select_states);
        st.world_lists.extend(front.world_lists);
        st.effect_lists.extend(front.effect_lists);
        st.quake_lists.extend(front.quake_lists);
        let normal_stage_form_id = if self.ctx.ids.form_global_stage != 0 {
            self.ctx.ids.form_global_stage
        } else {
            runtime::forms::codes::FORM_GLOBAL_STAGE
        };
        self.ctx.globals.stage_forms.insert(normal_stage_form_id, st);
        if !back_btn_select.choices.is_empty() {
            runtime::forms::global::prepare_saved_stage_btnselitems(
                &mut self.ctx,
                0,
                back_btn_select,
            );
        }
        self.ctx.globals.selbtn = front_btn_select;
        if !self.ctx.globals.selbtn.choices.is_empty() {
            runtime::forms::global::prepare_stage_btnselitems(&mut self.ctx);
        }

        let screen = Self::read_cpp_screen(rd)?;
        self.ctx.globals.screen_forms.insert(self.ctx.ids.form_global_screen, screen);

        self.read_cpp_sound(rd)?;

        let pcm_events = rd.fixed_items(|rd| Self::read_cpp_pcm_event(rd))?;
        if !pcm_events.is_empty() {
            self.ctx.globals.pcm_event_lists.insert(self.ctx.ids.form_global_pcm_event, pcm_events);
        }

        let mut editboxes = rd.fixed_items(|rd| Self::read_cpp_editbox(rd))?;
        if !editboxes.is_empty() {
            let screen_w = self.ctx.screen_w as i32;
            let screen_h = self.ctx.screen_h as i32;
            let display_mode_change_proc_cnt = self.ctx.globals.change_display_mode_proc_cnt;
            for eb in &mut editboxes {
                eb.design_screen_w = screen_w.max(1);
                eb.design_screen_h = screen_h.max(1);
                eb.update_rect(screen_w, screen_h);
                eb.frame(display_mode_change_proc_cnt);
            }
            let focused_idx = editboxes.iter().rposition(|eb| eb.created);
            let form_id = self.ctx.ids.form_global_editbox;
            self.ctx.globals.editbox_lists.insert(
                form_id,
                runtime::globals::EditBoxListState { boxes: editboxes },
            );
            self.ctx
                .set_focused_editbox(focused_idx.map(|idx| (form_id, idx)));
        }

        let call_cnt = rd.i32()?.max(0) as usize;
        let mut call_stack = Vec::with_capacity(call_cnt.max(1));
        for _ in 0..call_cnt {
            call_stack.push(self.read_cpp_call_frame(rd)?);
        }
        if call_stack.is_empty() {
            call_stack.push(self.scene_base_call());
        }

        let msg_back = Self::read_cpp_msg_back(rd)?;
        self.ctx.globals.msgbk_forms.insert(self.ctx.ids.form_global_msgbk, msg_back);

        self.ctx.globals.syscom.sel_save_stock_stream = rd.len_bytes()?;
        let inner_cnt = rd.i32()?.max(0) as usize;
        self.ctx.globals.syscom.inner_save_streams.clear();
        for _ in 0..inner_cnt {
            self.ctx.globals.syscom.inner_save_streams.push(rd.len_bytes()?);
        }
        self.ctx.globals.syscom.inner_save_exists = self.ctx.globals.syscom.inner_save_streams.iter().any(|s| !s.is_empty());
        let sel_save_cnt = rd.i32()?.max(0) as usize;
        self.ctx.globals.syscom.sel_save_ids.clear();
        for _ in 0..sel_save_cnt {
            self.ctx.globals.syscom.sel_save_ids.push(rd.tid()?);
        }
        Ok(call_stack)
    }

    fn restore_saved_scene_stack(
        &mut self,
        mut frames: Vec<CallFrame>,
        current_scene_name: &str,
    ) -> Result<Vec<CallFrame>> {
        // C++ does not save a separate cross-scene stack.  Each caller frame's
        // C_elm_call::m_call_save stores the lexer scene/line/pc captured by
        // tnm_save_call(), while the following callee frame stores call_type.
        // Rebuild only boundaries proven by those bytes; never infer a
        // dispatcher scene or synthesize z labels for a particular game.
        self.scene_stack.clear();
        if frames.is_empty() {
            return Ok(vec![self.scene_base_call()]);
        }

        // Scene numbers are runtime package indices and are not serialized in
        // C_elm_call. Resolve them from the saved scene names in the active
        // Scene.pck after load/reload.
        for frame in &mut frames {
            frame.return_scene_no = frame
                .return_scene_name
                .as_deref()
                .and_then(|name| {
                    self.scene_pck_cache
                        .as_ref()
                        .and_then(|cache| cache.find_scene_no(name))
                });
        }

        for callee_idx in 1..frames.len() {
            let call_type = frames[callee_idx].call_type;
            if call_type != 2 && call_type != 3 {
                continue;
            }

            let Some(caller_scene_name) = frames[callee_idx - 1]
                .return_scene_name
                .as_deref()
                .filter(|name| !name.is_empty())
            else {
                // A legacy/non-original Rust save may not contain caller lexer
                // metadata. There is no C++-justified way to recover it.
                continue;
            };

            let callee_scene_name = if callee_idx + 1 == frames.len() {
                Some(current_scene_name)
            } else {
                frames[callee_idx]
                    .return_scene_name
                    .as_deref()
                    .filter(|name| !name.is_empty())
            };
            let Some(callee_scene_name) = callee_scene_name else {
                continue;
            };

            // USER_CMD can target the current scene. Only a real scene change
            // needs a SceneExecFrame; same-scene calls are restored solely by
            // the shared C_elm_call list.
            if caller_scene_name.eq_ignore_ascii_case(callee_scene_name) {
                continue;
            }

            let Some(caller_scene_no) = self
                .scene_pck_cache
                .as_ref()
                .and_then(|cache| cache.find_scene_no(caller_scene_name))
            else {
                log::warn!(
                    "[SG_SAVELOAD] saved caller scene not found in active Scene.pck: {}",
                    caller_scene_name
                );
                continue;
            };

            let mut stream = self.cached_scene_stream(caller_scene_no)?;
            stream.set_prg_cntr(frames[callee_idx - 1].return_pc)?;
            let user_cmd_names = stream.scn_cmd_name_map.clone();
            let call_cmd_names = self
                .scene_pck_cache
                .as_ref()
                .expect("scene pck cache initialized")
                .inc_cmd_name_map
                .clone();

            self.scene_stack.push(SceneExecFrame {
                stream,
                user_cmd_names,
                call_cmd_names,
                current_scene_no: Some(caller_scene_no),
                current_scene_name: Some(caller_scene_name.to_string()),
                current_line_no: frames[callee_idx - 1].return_line_no,
                // At call entry C_elm_call_list::add_call() creates callee_idx,
                // so Rust's shared call stack length at this boundary is
                // callee_idx + 1 (base frame included).
                call_depth: callee_idx + 1,
            });
        }

        Ok(frames)
    }



    fn write_cpp_proc_record(
        w: &mut crate::original_save::OriginalStreamWriter,
        proc_type: i32,
        element: &[i32],
        option: i32,
    ) {
        w.push_i32(proc_type);
        w.push_element(element);
        w.push_i32(0);
        w.push_i32(0);
        w.push_bool(false);
        w.push_bool(false);
        w.push_bool(false);
        w.push_i32(option);
    }

    fn write_cpp_runtime_proc_stack(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        // Do not fabricate C_tnm_proc states.  Until the real C++ proc element,
        // arg_list, return_value_flag and option are mirrored from runtime state,
        // only the script proc can be represented safely; other transient waits are
        // saved as NONE rather than writing a guessed proc_type.
        let proc_type = if matches!(self.ctx.last_proc_kind(), runtime::ProcKind::Script) { 1 } else { 0 };
        Self::write_cpp_proc_record(w, proc_type, &[], 0);
        w.push_i32(0);
    }

    fn read_cpp_proc_record(&self, rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<i32> {
        let proc_type = rd.i32()?;
        let _element = rd.element()?;
        let _arg_list_id = rd.i32()?;
        let _arg_list: Vec<()> = rd.extend_items(|rd| {
            let _ = self.read_cpp_prop(rd)?;
            Ok(())
        })?;
        let _key_skip_enable_flag = rd.bool()?;
        let _skip_disable_flag = rd.bool()?;
        let _return_value_flag = rd.bool()?;
        let _option = rd.i32()?;
        Ok(proc_type)
    }

    fn decode_cpp_mwnd_element(elm: &[i32]) -> Option<(i64, usize)> {
        let is_array = |v: i32| {
            v == crate::runtime::forms::codes::ELM_ARRAY || v == -1
        };
        if elm.len() >= 4 {
            let stage_idx = if elm[0] == crate::runtime::forms::codes::ELM_GLOBAL_BACK {
                Some(0)
            } else if elm[0] == crate::runtime::forms::codes::ELM_GLOBAL_FRONT {
                Some(1)
            } else if elm[0] == crate::runtime::forms::codes::ELM_GLOBAL_NEXT {
                Some(2)
            } else {
                None
            };
            if let Some(stage_idx) = stage_idx {
                if elm[1] == crate::runtime::forms::codes::ELM_STAGE_MWND
                    && is_array(elm[2])
                    && elm[3] >= 0
                {
                    return Some((stage_idx, elm[3] as usize));
                }
            }
        }
        if elm.len() >= 6
            && elm[0] == crate::runtime::forms::codes::ELM_GLOBAL_STAGE
            && is_array(elm[1])
            && elm[2] >= 0
            && elm[3] == crate::runtime::forms::codes::ELM_STAGE_MWND
            && is_array(elm[4])
            && elm[5] >= 0
        {
            return Some((elm[2] as i64, elm[5] as usize));
        }
        None
    }

    fn apply_saved_current_mwnd_elements(
        &mut self,
        cur_mwnd: &[i32],
        cur_sel_mwnd: &[i32],
        last_mwnd: &[i32],
    ) {
        self.ctx.globals.current_mwnd_element = cur_mwnd.to_vec();
        self.ctx.globals.current_sel_mwnd_element = cur_sel_mwnd.to_vec();
        self.ctx.globals.last_mwnd_element = last_mwnd.to_vec();
        self.ctx.globals.current_mwnd_no = None;
        self.ctx.globals.current_sel_mwnd_no = None;
        self.ctx.globals.last_mwnd_no = None;

        if let Some((stage, no)) = Self::decode_cpp_mwnd_element(cur_mwnd) {
            self.ctx.globals.current_mwnd_stage_idx = stage;
            self.ctx.globals.current_mwnd_no = Some(no);
        }
        if let Some((stage, no)) = Self::decode_cpp_mwnd_element(cur_sel_mwnd) {
            self.ctx.globals.current_sel_mwnd_stage_idx = stage;
            self.ctx.globals.current_sel_mwnd_no = Some(no);
        }
        if let Some((stage, no)) = Self::decode_cpp_mwnd_element(last_mwnd) {
            self.ctx.globals.last_mwnd_stage_idx = stage;
            self.ctx.globals.last_mwnd_no = Some(no);
        }
    }

    fn current_local_save_id(&self) -> [u16; 7] {
        let now = crate::platform_time::local_time_fields();
        [
            now.year.clamp(0, u16::MAX as i32) as u16,
            now.month as u16,
            now.day as u16,
            now.hour as u16,
            now.minute as u16,
            now.second as u16,
            now.millisecond as u16,
        ]
    }

    /// Mirror of C++ `C_tnm_eng::save_local()`. Captures the engine snapshot into
    /// `ctx.local_save_snapshot` so subsequent SAVE / QUICK_SAVE / END_SAVE invocations
    /// write the savepoint-time state, not whatever transient menu state happens to be
    /// live when the user picks a slot.
    fn build_local_save_snapshot(&mut self) {
        let local_stream = self.build_original_local_stream();
        let local_ex_stream = self.build_original_local_ex_stream();
        let snapshot = crate::runtime::LocalSaveSnapshot {
            save_id: self.current_local_save_id(),
            append_dir: self.ctx.globals.append_dir.clone(),
            append_name: self.ctx.globals.append_name.clone(),
            save_scene_title: self.ctx.globals.syscom.current_save_scene_title.clone(),
            save_msg: String::new(),
            save_full_msg: self.ctx.globals.syscom.current_save_full_message.clone(),
            local_stream,
            local_ex_stream,
            sel_saves: self
                .ctx
                .local_save_snapshot
                .as_ref()
                .map(|s| s.sel_saves.clone())
                .unwrap_or_default(),
        };
        self.ctx.local_save_snapshot = Some(snapshot);
    }

    fn build_original_local_stream(&self) -> Vec<u8> {
        let mut w = crate::original_save::OriginalStreamWriter::new();
        let scene_name = self.current_scene_name.as_deref().unwrap_or("");
        let flag_cnt = self.local_flag_count();
        use crate::runtime::forms::codes;

        w.push_str(scene_name);
        w.push_i32(self.current_line_no);
        w.push_i32(self.stream.get_prg_cntr() as i32);

        self.write_cpp_runtime_proc_stack(&mut w);
        w.push_element(&self.ctx.globals.current_mwnd_element);
        w.push_element(&self.ctx.globals.current_sel_mwnd_element);
        w.push_element(&self.ctx.globals.last_mwnd_element);
        w.push_str(&self.ctx.globals.syscom.current_save_scene_title);
        let current_full_message = if self.ctx.globals.syscom.current_save_full_message.is_empty() {
            self.ctx.globals.syscom.current_save_message.as_str()
        } else {
            self.ctx.globals.syscom.current_save_full_message.as_str()
        };
        w.push_str(current_full_message);

        let btn_cnt = self.mwnd_waku_btn_count();
        for idx in 0..btn_cnt {
            w.push_bool(self.ctx.globals.syscom.mwnd_btn_disable.get(&(idx as i64)).copied().unwrap_or(false));
        }
        w.push_str(&self.ctx.globals.script.font_name);
        w.push_raw(&self.build_cpp_local_data_pod());

        w.push_i32(self.int_stack.len() as i32);
        for v in &self.int_stack { w.push_i32(*v); }
        w.push_i32(self.str_stack.len() as i32);
        for s in &self.str_stack { w.push_str(s); }
        w.push_i32(self.element_points.len() as i32);
        for p in &self.element_points { w.push_i32(*p as i32); }

        w.push_i32(self.ctx.globals.local_real_time.clamp(i32::MIN as i64, i32::MAX as i64) as i32);
        w.push_i32(self.ctx.globals.local_game_time.clamp(i32::MIN as i64, i32::MAX as i64) as i32);
        w.push_i32(self.ctx.globals.local_wipe_time.clamp(i32::MIN as i64, i32::MAX as i64) as i32);
        self.write_cpp_syscom_menu(&mut w);

        let fog = &self.ctx.globals.fog_global;
        w.push_str(&fog.name);
        Self::write_cpp_int_event_raw(&mut w, &fog.x_event);
        w.push_i32(fog.near as i32);
        w.push_i32(fog.far as i32);

        w.push_fixed_i32_list(self.int_list_by_element(codes::ELM_GLOBAL_A), flag_cnt);
        w.push_fixed_i32_list(self.int_list_by_element(codes::ELM_GLOBAL_B), flag_cnt);
        w.push_fixed_i32_list(self.int_list_by_element(codes::ELM_GLOBAL_C), flag_cnt);
        w.push_fixed_i32_list(self.int_list_by_element(codes::ELM_GLOBAL_D), flag_cnt);
        w.push_fixed_i32_list(self.int_list_by_element(codes::ELM_GLOBAL_E), flag_cnt);
        w.push_fixed_i32_list(self.int_list_by_element(codes::ELM_GLOBAL_F), flag_cnt);
        w.push_fixed_i32_list(self.int_list_by_element(codes::ELM_GLOBAL_X), flag_cnt);
        w.push_fixed_str_list(self.str_list_by_element(codes::ELM_GLOBAL_S), flag_cnt);
        w.push_extend_i32_list(&self.ctx.globals.local_flag_h);
        w.push_extend_i32_list(&self.ctx.globals.local_flag_i);
        w.push_extend_i32_list(&self.ctx.globals.local_flag_j);
        w.push_fixed_str_list(self.str_list_by_element(codes::ELM_GLOBAL_NAMAE_LOCAL), 26 + 26 * 26);

        self.write_cpp_inc_prop_list(&mut w);
        self.write_cpp_current_scene_prop_lists(&mut w);

        let counter_list = self.ctx.globals.counter_lists.values().next().cloned().unwrap_or_default();
        w.push_fixed_items(&counter_list, |w, c| self.write_cpp_counter_param(w, c));

        let frame_action = self.ctx.globals.frame_actions.values().next().cloned().unwrap_or_default();
        self.write_cpp_frame_action(&mut w, &frame_action);

        let frame_action_ch = self.ctx.globals.frame_action_lists.values().next().cloned().unwrap_or_default();
        w.push_fixed_items(&frame_action_ch, |w, fa| self.write_cpp_frame_action(w, fa));

        // Original C_elm_g00_buf::save writes the file name for each slot.
        w.push_fixed_items(&self.ctx.globals.g00buf_names, |w, name| w.push_str(name.as_deref().unwrap_or("")));

        let mask_list = self.ctx.globals.mask_lists.values().next().map(|m| m.masks.clone()).unwrap_or_default();
        w.push_fixed_items(&mask_list, |w, m| {
            Self::write_cpp_int_event_raw(w, &m.x_event);
            Self::write_cpp_int_event_raw(w, &m.y_event);
            w.push_str(m.name.as_deref().unwrap_or(""));
        });

        self.write_cpp_stage(&mut w, 0);
        self.write_cpp_stage(&mut w, 1);
        self.write_cpp_screen(&mut w);
        self.write_cpp_sound(&mut w);

        let pcm_events = self.ctx.globals.pcm_event_lists.values().next().cloned().unwrap_or_default();
        w.push_fixed_items(&pcm_events, |w, ev| self.write_cpp_pcm_event(w, ev));

        let editboxes = self.ctx.globals.editbox_lists.values().next().map(|e| e.boxes.clone()).unwrap_or_default();
        w.push_fixed_items(&editboxes, |w, e| self.write_cpp_editbox(w, e));

        let saved_call_stack = self.flattened_call_stack_for_save();
        w.push_i32(saved_call_stack.len() as i32);
        for frame in &saved_call_stack {
            self.write_cpp_call_frame(&mut w, frame);
        }
        self.write_cpp_msg_back(&mut w);

        w.push_len_bytes(&self.ctx.globals.syscom.sel_save_stock_stream);
        w.push_i32(self.ctx.globals.syscom.inner_save_streams.len() as i32);
        for stream in &self.ctx.globals.syscom.inner_save_streams {
            w.push_len_bytes(stream);
        }
        w.push_i32(self.ctx.globals.syscom.sel_save_ids.len() as i32);
        for tid in &self.ctx.globals.syscom.sel_save_ids {
            w.push_tid(tid);
        }
        w.into_inner()
    }

    fn build_original_local_ex_stream(&self) -> Vec<u8> {
        let mut w = crate::original_save::OriginalStreamWriter::new();
        let s = &self.ctx.globals.syscom;
        for i in 0..4 {
            let sw = s.local_extra_switches.get(i).copied().unwrap_or(if i == 0 { s.local_extra_switch } else { runtime::globals::ToggleFeatureState::default() });
            w.push_bool(sw.exist);
            w.push_bool(sw.enable);
            w.push_bool(sw.onoff);
        }
        for i in 0..4 {
            let mode = s.local_extra_modes.get(i).copied().unwrap_or(if i == 0 { s.local_extra_mode } else { runtime::globals::ValueFeatureState::default() });
            w.push_bool(mode.exist);
            w.push_bool(mode.enable);
            w.push_padding(2);
            w.push_i32(mode.value as i32);
        }
        let out = w.into_inner();
        debug_assert_eq!(out.len(), 44);
        out
    }

    fn parse_original_local_ex_stream(&mut self, local_ex_stream: &[u8]) -> Result<()> {
        if local_ex_stream.len() < 44 { return Ok(()); }
        let mut rd = crate::original_save::OriginalStreamReader::new(local_ex_stream);
        for i in 0..4 {
            self.ctx.globals.syscom.local_extra_switches[i].exist = rd.bool()?;
            self.ctx.globals.syscom.local_extra_switches[i].enable = rd.bool()?;
            self.ctx.globals.syscom.local_extra_switches[i].onoff = rd.bool()?;
        }
        for i in 0..4 {
            self.ctx.globals.syscom.local_extra_modes[i].exist = rd.bool()?;
            self.ctx.globals.syscom.local_extra_modes[i].enable = rd.bool()?;
            rd.skip(2)?;
            self.ctx.globals.syscom.local_extra_modes[i].value = rd.i32()? as i64;
        }
        self.ctx.globals.syscom.local_extra_switch = self.ctx.globals.syscom.local_extra_switches[0];
        self.ctx.globals.syscom.local_extra_mode = self.ctx.globals.syscom.local_extra_modes[0];
        Ok(())
    }

    fn parse_original_local_stream(&mut self, local_stream: &[u8]) -> Result<RuntimeDiskSnapshot> {
        let mut rd = crate::original_save::OriginalStreamReader::new(local_stream);
        let flag_cnt = self.local_flag_count();
        use crate::runtime::forms::codes;

        let scene_name = rd.string()?;
        let line_no = rd.i32()?;
        let pc = rd.i32()?;

        let current_proc_type = self.read_cpp_proc_record(&mut rd)?;
        let proc_stack_cnt = rd.i32()?.max(0) as usize;
        let mut proc_stack_types = Vec::with_capacity(proc_stack_cnt);
        for _ in 0..proc_stack_cnt {
            proc_stack_types.push(self.read_cpp_proc_record(&mut rd)?);
        }
        log::warn!(
            "[SG_SAVELOAD_PROBE] local_stream scene={} line={} pc=0x{:x} current_proc_type={} proc_stack_cnt={} proc_stack_types={:?}",
            scene_name,
            line_no,
            pc.max(0),
            current_proc_type,
            proc_stack_cnt,
            proc_stack_types,
        );
        let cur_mwnd = rd.element()?;
        let cur_sel_mwnd = rd.element()?;
        let last_mwnd = rd.element()?;
        self.apply_saved_current_mwnd_elements(&cur_mwnd, &cur_sel_mwnd, &last_mwnd);
        self.ctx.globals.syscom.current_save_scene_title = rd.string()?;
        self.ctx.globals.syscom.current_save_full_message = rd.string()?;
        self.ctx.globals.syscom.current_save_message.clear();

        let btn_cnt = self.mwnd_waku_btn_count();
        self.ctx.globals.syscom.mwnd_btn_disable.clear();
        for idx in 0..btn_cnt {
            if rd.bool()? {
                self.ctx.globals.syscom.mwnd_btn_disable.insert(idx as i64, true);
            }
        }
        self.ctx.globals.script.font_name = rd.string()?;
        self.read_cpp_local_data_pod(&mut rd)?;

        let int_cnt = rd.i32()?.max(0) as usize;
        let mut int_stack = Vec::with_capacity(int_cnt);
        for _ in 0..int_cnt { int_stack.push(rd.i32()?); }
        let str_cnt = rd.i32()?.max(0) as usize;
        let mut str_stack = Vec::with_capacity(str_cnt);
        for _ in 0..str_cnt { str_stack.push(rd.string()?); }
        let ep_cnt = rd.i32()?.max(0) as usize;
        let mut element_points = Vec::with_capacity(ep_cnt);
        for _ in 0..ep_cnt { element_points.push(rd.i32()?.max(0) as usize); }

        self.ctx.globals.local_real_time = rd.i32()? as i64;
        self.ctx.globals.local_game_time = rd.i32()? as i64;
        self.ctx.globals.local_wipe_time = rd.i32()? as i64;
        self.read_cpp_syscom_menu(&mut rd)?;

        let fog_name = rd.string()?;
        let fog_x = Self::read_cpp_int_event_raw(&mut rd)?;
        let fog_near = rd.i32()?;
        let fog_far = rd.i32()?;
        self.ctx.globals.fog_global.name = fog_name;
        self.ctx.globals.fog_global.enabled = !self.ctx.globals.fog_global.name.is_empty();
        self.ctx.globals.fog_global.texture_image_id = None;
        if self.ctx.globals.fog_global.enabled {
            match self.ctx.images.load_g00(&self.ctx.globals.fog_global.name, 0) {
                Ok(id) => self.ctx.globals.fog_global.texture_image_id = Some(id),
                Err(e) => log::error!(
                    "load_local fog texture '{}' failed: {e}",
                    self.ctx.globals.fog_global.name
                ),
            }
        }
        self.ctx.globals.fog_global.x_event = fog_x;
        self.ctx.globals.fog_global.scroll_x = self.ctx.globals.fog_global.x_event.get_total_value() as f32;
        self.ctx.globals.fog_global.near = fog_near as f32;
        self.ctx.globals.fog_global.far = fog_far as f32;

        let a = rd.fixed_i32_list()?;
        let b = rd.fixed_i32_list()?;
        let c = rd.fixed_i32_list()?;
        let d = rd.fixed_i32_list()?;
        let e = rd.fixed_i32_list()?;
        let f = rd.fixed_i32_list()?;
        let x = rd.fixed_i32_list()?;
        let s = rd.fixed_str_list()?;
        let h = rd.extend_i32_list()?;
        let i = rd.extend_i32_list()?;
        let j = rd.extend_i32_list()?;
        let namae_local = rd.fixed_str_list()?;
        self.ctx.globals.int_lists.insert(codes::ELM_GLOBAL_A as u32, resize_i64_vec(a, flag_cnt));
        self.ctx.globals.int_lists.insert(codes::ELM_GLOBAL_B as u32, resize_i64_vec(b, flag_cnt));
        self.ctx.globals.int_lists.insert(codes::ELM_GLOBAL_C as u32, resize_i64_vec(c, flag_cnt));
        self.ctx.globals.int_lists.insert(codes::ELM_GLOBAL_D as u32, resize_i64_vec(d, flag_cnt));
        self.ctx.globals.int_lists.insert(codes::ELM_GLOBAL_E as u32, resize_i64_vec(e, flag_cnt));
        self.ctx.globals.int_lists.insert(codes::ELM_GLOBAL_F as u32, resize_i64_vec(f, flag_cnt));
        self.ctx.globals.int_lists.insert(codes::ELM_GLOBAL_X as u32, resize_i64_vec(x, flag_cnt));
        self.ctx.globals.str_lists.insert(codes::ELM_GLOBAL_S as u32, resize_string_vec(s, flag_cnt));
        self.ctx.globals.local_flag_h = h;
        self.ctx.globals.local_flag_i = i;
        self.ctx.globals.local_flag_j = j;
        self.ctx.globals.str_lists.insert(codes::ELM_GLOBAL_NAMAE_LOCAL as u32, resize_string_vec(namae_local, 26 + 26 * 26));

        let call_stack = self.parse_cpp_tail_state(&mut rd, &scene_name)?;

        Ok(RuntimeDiskSnapshot {
            scene_name,
            scene_no: -1,
            line_no,
            pc,
            int_stack,
            str_stack,
            element_points,
            call_stack,
        })
    }

    #[inline(always)]
    fn save_load_trace_enabled(&self) -> bool {
        self.runtime_options.save_load_trace
    }

    fn perform_runtime_save_request(&mut self, req: RuntimeSaveRequest) -> Result<()> {
        if req.kind == RuntimeSaveKind::Inner {
            // C++ `tnm_saveload_proc_create_inner_save` copies the current
            // `m_local_save` into the inner-save slot. It must not reserialize the
            // live runtime (which may be the save/load menu).
            let Some(snapshot) = self.ctx.local_save_snapshot.as_ref() else {
                log::error!(
                    "[SG_SAVELOAD] inner save dropped idx={}: no local_save snapshot",
                    req.index
                );
                return Ok(());
            };
            if self.save_load_trace_enabled() {
                eprintln!("[SG_SAVELOAD_TRACE][VM] save inner idx={}", req.index);
            }
            if self.ctx.globals.syscom.inner_save_streams.len() <= req.index {
                self.ctx.globals.syscom.inner_save_streams.resize_with(req.index + 1, Vec::new);
            }
            self.ctx.globals.syscom.inner_save_streams[req.index] = snapshot.local_stream.clone();
            self.ctx.globals.syscom.inner_save_exists = true;
            return Ok(());
        }

        // Normal / quick / end save mirror C++ `tnm_save_local_on_file`: bail out when
        // there is no snapshot (equivalent to `m_local_save.save_stream.empty()`).
        // Without this, picking a slot in the save menu would otherwise serialize the
        // menu itself - the bug we're fixing.
        if self.ctx.local_save_snapshot.is_none() {
            log::error!(
                "[SG_SAVELOAD] save dropped (kind={:?} idx={}): no local_save snapshot. \
                 SAVEPOINT has not fired in the current message block - either the script \
                 set dont_set_save_point or auto-SAVEPOINT wasn't reached yet. No file written.",
                req.kind, req.index
            );
            if req.kind == RuntimeSaveKind::Normal {
                crate::runtime::forms::syscom::free_runtime_save_thumb_capture(
                    &mut self.ctx,
                    crate::runtime::forms::syscom::CAPTURE_PRIOR_SAVE,
                );
            }
            return Ok(());
        }

        // Refresh local_ex_stream from the live runtime; mirrors C++ `save_local_ex()`
        // being called inside `tnm_save_local_on_file` right before writing.
        let refreshed_ex = self.build_original_local_ex_stream();
        if let Some(snapshot) = self.ctx.local_save_snapshot.as_mut() {
            snapshot.local_ex_stream = refreshed_ex;
        }

        let slot = self.ensure_runtime_slot_for_save(req);
        let Some(path) = self.runtime_save_file_path(req.kind, req.index) else {
            if req.kind == RuntimeSaveKind::Normal {
                crate::runtime::forms::syscom::free_runtime_save_thumb_capture(
                    &mut self.ctx,
                    crate::runtime::forms::syscom::CAPTURE_PRIOR_SAVE,
                );
            }
            return Ok(());
        };
        if self.save_load_trace_enabled() {
            eprintln!(
                "[SG_SAVELOAD_TRACE][VM] save begin kind={:?} idx={} path={} file_exists_before={}",
                req.kind,
                req.index,
                path.display(),
                crate::resource::game_file_exists(&path)
            );
        }
        let snapshot = self
            .ctx
            .local_save_snapshot
            .as_ref()
            .expect("snapshot presence checked above");
        let env = crate::original_save::OriginalLocalSaveEnvelope {
            save_id: snapshot.save_id,
            append_dir: snapshot.append_dir.clone(),
            append_name: snapshot.append_name.clone(),
            title: snapshot.save_scene_title.clone(),
            message: snapshot.save_msg.clone(),
            full_message: snapshot.save_full_msg.clone(),
            local_stream: snapshot.local_stream.clone(),
            local_ex_stream: snapshot.local_ex_stream.clone(),
            sel_saves: snapshot.sel_saves.clone(),
        };
        if let Err(err) = crate::original_save::write_local_save_file(&path, &slot, &env) {
            if req.kind == RuntimeSaveKind::Normal {
                crate::runtime::forms::syscom::free_runtime_save_thumb_capture(
                    &mut self.ctx,
                    crate::runtime::forms::syscom::CAPTURE_PRIOR_SAVE,
                );
            }
            return Err(err);
        }
        crate::runtime::forms::syscom::write_global_save(&self.ctx);
        if self.save_load_trace_enabled() {
            eprintln!(
                "[SG_SAVELOAD_TRACE][VM] save written kind={:?} idx={} path={} bytes={}",
                req.kind,
                req.index,
                path.display(),
                crate::resource::game_file_len(&path).unwrap_or(0)
            );
        }
        // tnm_save_local_on_file() clears C_tnm_save_cache before writing and
        // leaves that slot uncached.  Do not immediately repopulate the Rust
        // header cache from the file; the next metadata query must reload it.
        match req.kind {
            RuntimeSaveKind::Normal => {
                if self.ctx.globals.syscom.save_slots.len() <= req.index {
                    self.ctx.globals.syscom.save_slots.resize_with(req.index + 1, Default::default);
                }
                self.ctx.globals.syscom.save_slots[req.index].header_cache_valid = false;
            }
            RuntimeSaveKind::Quick => {
                if self.ctx.globals.syscom.quick_save_slots.len() <= req.index {
                    self.ctx.globals.syscom.quick_save_slots.resize_with(req.index + 1, Default::default);
                }
                self.ctx.globals.syscom.quick_save_slots[req.index].header_cache_valid = false;
            }
            RuntimeSaveKind::End => {
                self.ctx.globals.syscom.end_save_exists = true;
            }
            RuntimeSaveKind::Inner => {}
        }
        if let Some(save_kind) = Self::save_kind_to_original(req.kind) {
            let save_no = crate::original_save::original_save_no(
                self.configured_runtime_save_count(false),
                self.configured_runtime_save_count(true),
                save_kind,
                req.index,
            );
            if self.save_load_trace_enabled() {
                eprintln!(
                    "[SG_SAVELOAD_TRACE][VM] save thumb write kind={:?} idx={} original_save_no={}",
                    req.kind,
                    req.index,
                    save_no
                );
            }
            crate::runtime::forms::syscom::write_runtime_slot_thumb(&mut self.ctx, save_no);
        }
        if req.kind == RuntimeSaveKind::Normal {
            crate::runtime::forms::syscom::free_runtime_save_thumb_capture(
                &mut self.ctx,
                crate::runtime::forms::syscom::CAPTURE_PRIOR_SAVE,
            );
        }
        Ok(())
    }

    fn perform_runtime_load_request(&mut self, req: RuntimeLoadRequest) -> Result<()> {
        if self.save_load_trace_enabled() {
            eprintln!("[SG_SAVELOAD_TRACE][VM] load begin kind={:?} idx={}", req.kind, req.index);
        }
        struct LoadedEnvelopeMeta {
            save_id: [u16; 7],
            append_dir: String,
            append_name: String,
            title: String,
            message: String,
            full_message: String,
            sel_saves: Vec<crate::original_save::OriginalLocalSaveEnvelope>,
        }
        let (local_stream, local_ex_stream, loaded_meta) = if req.kind == RuntimeSaveKind::Inner {
            let Some(stream) = self.ctx.globals.syscom.inner_save_streams.get(req.index).cloned() else { return Ok(()); };
            (stream, Vec::new(), None)
        } else {
            let Some(path) = self.runtime_save_file_path(req.kind, req.index) else { return Ok(()); };
            if self.save_load_trace_enabled() {
                eprintln!(
                    "[SG_SAVELOAD_TRACE][VM] load read kind={:?} idx={} path={} file_exists={}",
                    req.kind,
                    req.index,
                    path.display(),
                    crate::resource::game_file_exists(&path)
                );
            }
            let (_header, env) = crate::original_save::read_local_save_file(&path)?;
            let meta = LoadedEnvelopeMeta {
                save_id: env.save_id,
                append_dir: env.append_dir.clone(),
                append_name: env.append_name.clone(),
                title: env.title.clone(),
                message: env.message.clone(),
                full_message: env.full_message.clone(),
                sel_saves: env.sel_saves.clone(),
            };
            (env.local_stream, env.local_ex_stream, Some(meta))
        };
        if let Some(meta) = loaded_meta.as_ref() {
            self.ctx
                .set_active_append(meta.append_dir.clone(), meta.append_name.clone());
        }
        // VM-side equivalent of C++ `tnm_finish_local`: drop excall frames, sel
        // points, and the stale save point. The loaded scene re-establishes its
        // own context; without this, when the loaded scene eventually issues a
        // RETURN we'd pop back into the orphaned save/load menu excall frame.
        self.scene_stack.clear();
        self.scene_user_props.clear();
        self.sel_point_stack.clear();
        self.save_point = None;
        self.ctx.local_save_snapshot = None;
        self.ctx.begin_runtime_load_apply();
        let snapshot = self.parse_original_local_stream(&local_stream)?;
        self.parse_original_local_ex_stream(&local_ex_stream)?;
        // Mirror C++ `tnm_load_local_on_file` + tail of `load_local`: re-populate
        // `m_local_save` so the loaded scene can SAVE without first taking another
        // SAVEPOINT. C++ clears save_msg and copies save_full_msg = cur_full_message
        // after load_local; do the same here.
        if let Some(meta) = loaded_meta {
            self.ctx.local_save_snapshot = Some(crate::runtime::LocalSaveSnapshot {
                save_id: meta.save_id,
                append_dir: meta.append_dir,
                append_name: meta.append_name,
                save_scene_title: meta.title,
                save_msg: String::new(),
                save_full_msg: self.ctx.globals.syscom.current_save_full_message.clone(),
                local_stream: local_stream.clone(),
                local_ex_stream: local_ex_stream.clone(),
                sel_saves: meta.sel_saves,
            });
            // The header text from the loaded file (which represents the last
            // append'd-message state) takes precedence over what's left in
            // current_save_message after parse, so subsequent saves echo what the
            // user actually saw last.
            let snap = self.ctx.local_save_snapshot.as_ref().unwrap();
            self.ctx.globals.syscom.current_save_scene_title = snap.save_scene_title.clone();
        }
        if snapshot.scene_name.is_empty() {
            log::error!(
                "[SG_SAVELOAD] aborting load (kind={:?} idx={}): saved snapshot has empty scene_name. \
                 This save file is unusable; please delete it.",
                req.kind, req.index
            );
            return Ok(());
        }
        let (mut stream, scene_no) = self.load_scene_stream(&snapshot.scene_name, 0)?;
        stream.set_prg_cntr(snapshot.pc.max(0) as usize)?;
        let active_call_stack = self.restore_saved_scene_stack(
            snapshot.call_stack,
            &snapshot.scene_name,
        )?;
        self.stream = stream;
        self.int_stack = snapshot.int_stack;
        self.str_stack = snapshot.str_stack;
        self.element_points = snapshot.element_points;
        self.call_stack = active_call_stack;
        if self.call_stack.is_empty() {
            self.call_stack.push(self.scene_base_call());
        }
        self.gosub_return_stack.clear();
        self.current_scene_no = if snapshot.scene_no >= 0 { Some(snapshot.scene_no as usize) } else { Some(scene_no) };
        self.current_scene_name = Some(snapshot.scene_name);
        self.current_line_no = snapshot.line_no;
        self.legacy_saved_active_only = self.current_scene_no.is_some_and(|no| no > 0)
            && self.call_stack.len() == 1;
        self.ctx.current_scene_no = self.current_scene_no.map(|v| v as i64);
        self.ctx.current_scene_name = self.current_scene_name.clone();
        self.ctx.current_line_no = self.current_line_no as i64;
        self.ctx.wait = runtime::wait::VmWait::default();
        self.halted = false;
        self.delayed_ret_form = None;
        // C++ `C_elm_stage::load` / `C_elm_mwnd::load` end by calling each
        // object's `restruct_type()` to rebuild type-specific runtime resources.
        // Rust's sprite, image, movie, weather and mesh handles are not stored in
        // the save stream, so rebuild them recursively before resuming the script.
        self.restore_runtime_bindings_after_load();
        self.ctx.mark_runtime_load_completed();
        Ok(())
    }

    /// Rebuild every loaded object's type-specific runtime backend, including
    /// top-level stage objects, message-window embedded objects and descendants.
    /// This is the Rust equivalent of the `restruct_type()` tail in
    /// `C_elm_object::load`; CAPTURE and EMOTE retain the original engine's
    /// non-reconstructible/unsupported behavior.
    fn restore_runtime_bindings_after_load(&mut self) {
        // C++ C_elm_object::load() always finishes with restruct_type().
        // Remove each form from globals while rebuilding so the backend helper
        // can mutably access the complete CommandContext without aliasing the map.
        let form_ids: Vec<u32> = self.ctx.globals.stage_forms.keys().copied().collect();
        for form_id in form_ids {
            let Some(mut stage_form) = self.ctx.globals.stage_forms.remove(&form_id) else {
                continue;
            };
            crate::runtime::forms::stage::restore_stage_form_backends_after_load(
                &mut self.ctx,
                &mut stage_form,
            );
            self.ctx.globals.stage_forms.insert(form_id, stage_form);
        }
    }

    fn drain_runtime_save_load_requests(&mut self) -> Result<()> {
        // Auto SAVEPOINT must fire before any pending save in the same command
        // batch, so a SAVE issued from the script's first frame after a message
        // block start still has a snapshot to write.
        if self.ctx.take_pending_auto_savepoint() {
            self.build_local_save_snapshot();
        }
        if let Some(req) = self.ctx.take_runtime_save_request() {
            self.perform_runtime_save_request(req)?;
        }
        if let Some(req) = self.ctx.take_runtime_load_request() {
            self.perform_runtime_load_request(req)?;
        }
        Ok(())
    }

    fn exec_return(&mut self, args: Vec<Value>) -> Result<bool> {
        // Pop callee frame.
        let Some(callee) = self.call_stack.pop() else {
            return Ok(false);
        };

        // Return info is stored on the caller frame .
        let caller = match self.call_stack.last_mut() {
            Some(f) => f,
            None => {
                // No caller: treat as end.
                return Ok(false);
            }
        };

        // C++ `tnm_scene_proc_gosub` persists the continuation on the caller
        // call frame (`save_call`), then `load_call` restores that caller.
        // Frame-action/user-command inline calls may run nested gosubs while a
        // script gosub is waiting, so the authoritative continuation must be
        // the caller frame here rather than any callee-local scratch state.
        // The continuation belongs to the *callee*: it is recorded when the call is
        // made. The caller-frame slots are shared with every other dispatch that runs
        // while a script-level call is suspended (pending button actions, frame-action
        // finishes, excall procs), and those overwrite them with their own fm_void
        // continuation. Reading the caller slots here therefore drops the script's
        // pending call result -- observed as `caller_ret_form=0 (void)` while the call
        // site needs FM_INT, leaving the interpreter one value short, and the next
        // conditional pop dies with `int stack underflow`.
        // `return_override` is the per-callee copy of that continuation.
        let (return_pc, ret_form) = match callee.return_override {
            Some((pc, form)) => (pc, form),
            None => (caller.return_pc, caller.ret_form),
        };
        if self.runtime_options.trace_call_return_pc {
            eprintln!(
                "[SG_CALL_PC] return depth={} pc=0x{:x} ret_form={} override={:?} args={:?}",
                self.call_stack.len() + 1,
                return_pc,
                ret_form,
                callee.return_override,
                args
            );
        }
        self.stream.set_prg_cntr(return_pc)?;

        match ret_form {
            f if f == self.cfg.fm_int => {
                let v = args.get(0).and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                self.push_int(v);
            }
            f if f == self.cfg.fm_str => {
                let s = args
                    .get(0)
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                self.push_str(s);
            }
            _ => {
                // Ignore.
            }
        }

        let frame_action_proc = callee.frame_action_proc;
        let excall_proc = callee.excall_proc;
        if excall_proc {
            self.mark_excall_script_proc_pop_requested();
        }
        self.recycle_call_frame(callee);

        Ok(frame_action_proc)
    }

    fn scene_base_call(&self) -> CallFrame {
        self.make_call_frame(self.cfg.fm_void, false, false, 0, None)
    }

    fn load_scene_stream(
        &mut self,
        scene_name: &str,
        z_no: i32,
    ) -> Result<(SceneStream<'a>, usize)> {
        self.ensure_scene_pck_cache()?;
        let scene_no = self
            .scene_pck_cache
            .as_ref()
            .expect("scene pck cache initialized")
            .find_scene_no(scene_name)
            .ok_or_else(|| anyhow!("scene not found: {}", scene_name))?;
        let mut stream = self.cached_scene_stream(scene_no)?;
        sg_omv_trace!(self,
            "load_scene_stream resolved target={} scene_no={} z={} initial_pc=0x{:x} scn_len=0x{:x}",
            scene_name,
            scene_no,
            z_no,
            stream.get_prg_cntr(),
            stream.scn.len()
        );
        self.call_cmd_names = self
            .scene_pck_cache
            .as_ref()
            .expect("scene pck cache initialized")
            .inc_cmd_name_map
            .clone();
        self.user_cmd_names = stream.scn_cmd_name_map.clone();
        match stream.jump_to_z_label(z_no.max(0) as usize) {
            Ok(()) => {
                sg_omv_trace!(self,
                    "load_scene_stream entered target={} scene_no={} z={} target_pc=0x{:x} user_cmd_cnt={} call_cmd_cnt={}",
                    scene_name,
                    scene_no,
                    z_no,
                    stream.get_prg_cntr(),
                    stream.scn_cmd_name_map.len(),
                    self.call_cmd_names.len()
                );
            }
            Err(e) => {
                sg_omv_trace!(self,
                    "load_scene_stream failed target={} scene_no={} z={} error={}",
                    scene_name,
                    scene_no,
                    z_no,
                    e
                );
                return Err(e);
            }
        }
        Ok((stream, scene_no))
    }

    fn jump_to_scene_name(&mut self, scene_name: &str, z_no: i32) -> Result<()> {
        sg_omv_trace!(self, "scene_jump target={} z={}", scene_name, z_no);
        if sg_scene_trace() {
            log::warn!(
                "[SG-DIAG-6] scene_jump target={} z={} caller={:?} caller_line={} call_depth={} scene_stack={}",
                scene_name,
                z_no,
                self.current_scene_name,
                self.current_line_no,
                self.call_stack.len(),
                self.scene_stack.len()
            );
        }
        let (stream, scene_no) = self.load_scene_stream(scene_name, z_no)?;
        self.stash_current_scene_user_props();
        self.stream = stream;
        self.current_scene_no = Some(scene_no);
        self.activate_scene_user_prop_scope(scene_no);
        self.current_scene_name = Some(scene_name.to_string());
        self.current_line_no = -1;
        self.ctx.current_scene_no = Some(scene_no as i64);
        self.ctx.current_scene_name = Some(scene_name.to_string());
        self.ctx.current_line_no = -1;
        sg_omv_trace!(self,
            "scene_jump_entered target={} scene_no={} z={} pc=0x{:x}",
            scene_name,
            scene_no,
            z_no,
            self.stream.get_prg_cntr()
        );
        Ok(())
    }

    fn farcall_scene_name_ex(
        &mut self,
        scene_name: &str,
        z_no: i32,
        ret_form: i32,
        ex_call_proc: bool,
        scratch_source_args: &[Value],
    ) -> Result<()> {
        sg_omv_trace!(self,
            "scene_farcall target={} z={} ret_form={} ex_call_proc={} scratch_argc={}",
            scene_name,
            z_no,
            ret_form,
            ex_call_proc,
            scratch_source_args.len()
        );
        if sg_scene_trace() {
            log::warn!(
                "[SG-DIAG-7] scene_farcall target={} z={} ret_form={} ex_call_proc={} caller={:?} caller_line={} call_depth={} scene_stack={}",
                scene_name,
                z_no,
                ret_form,
                ex_call_proc,
                self.current_scene_name,
                self.current_line_no,
                self.call_stack.len(),
                self.scene_stack.len()
            );
        }
        self.trace_cf_branch_farcall(
            self.stream.get_prg_cntr(),
            scene_name,
            z_no,
            ret_form,
            ex_call_proc,
            scratch_source_args,
        );
        if self.sg_debug_enabled()
            && ((scene_name == "sys20_adv00" && matches!(z_no, 10 | 13 | 17))
                || (scene_name == "sys20_adv01" && z_no == 0))
        {
            let args_dbg = scratch_source_args
                .iter()
                .map(|v| format!("{v:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            self.sg_cgm_coord_trace_emit(format_args!(
                "farcall target={} z={} ret_form={} ex_call_proc={} argc={} args=[{}]",
                scene_name,
                z_no,
                ret_form,
                ex_call_proc,
                scratch_source_args.len(),
                args_dbg
            ));
        }

        self.ensure_scene_pck_cache()?;
        let scene_no = self
            .scene_pck_cache
            .as_ref()
            .expect("scene pck cache initialized")
            .find_scene_no(scene_name)
            .ok_or_else(|| anyhow!("scene not found: {}", scene_name))?;
        let mut target_stream = self.cached_scene_stream(scene_no)?;
        target_stream.jump_to_z_label(z_no.max(0) as usize)?;

        let return_pc = self.stream.get_prg_cntr();
        let depth = self.call_stack.len();
        let caller = self
            .call_stack
            .last_mut()
            .ok_or_else(|| anyhow!("call stack underflow entering FARCALL"))?;
        if self.runtime_options.trace_call_return_pc {
            eprintln!(
                "[SG_CALL_PC] cross-scene farcall set depth={} target_scene={} z={} return_pc=0x{:x} old=0x{:x}",
                depth, scene_no, z_no, return_pc, caller.return_pc
            );
        }
        caller.return_pc = return_pc;
        caller.return_scene_no = self.current_scene_no;
        caller.return_scene_name = self.current_scene_name.clone();
        caller.return_line_no = self.current_line_no;
        caller.ret_form = ret_form;

        let target_user_cmd_names = target_stream.scn_cmd_name_map.clone();
        let target_call_cmd_names = self
            .scene_pck_cache
            .as_ref()
            .expect("scene pck cache initialized")
            .inc_cmd_name_map
            .clone();
        let saved_stream = std::mem::replace(&mut self.stream, target_stream);
        let saved_user_cmd_names =
            std::mem::replace(&mut self.user_cmd_names, target_user_cmd_names);
        let saved_call_cmd_names =
            std::mem::replace(&mut self.call_cmd_names, target_call_cmd_names);
        let saved_current_scene_no = self.current_scene_no;
        let saved_current_scene_name = self.current_scene_name.clone();
        let saved_current_line_no = self.current_line_no;

        self.enter_cross_scene_user_prop_scope(scene_no);
        self.current_scene_no = Some(scene_no);
        self.current_scene_name = Some(scene_name.to_string());
        self.current_line_no = -1;
        self.ctx.current_scene_no = Some(scene_no as i64);
        self.ctx.current_scene_name = Some(scene_name.to_string());
        self.ctx.current_line_no = -1;

        let scratch_args = self.call_scratch_from_args(scratch_source_args);
        let mut call_frame = self.take_call_frame(
            self.cfg.fm_void,
            ex_call_proc,
            false,
            scratch_source_args.len(),
            Some(scratch_args),
        );
        call_frame.call_type = 2;
        self.call_stack.push(call_frame);
        self.scene_stack.push(SceneExecFrame {
            stream: saved_stream,
            user_cmd_names: saved_user_cmd_names,
            call_cmd_names: saved_call_cmd_names,
            current_scene_no: saved_current_scene_no,
            current_scene_name: saved_current_scene_name,
            current_line_no: saved_current_line_no,
            call_depth: self.call_stack.len(),
        });

        sg_omv_trace!(self,
            "scene_farcall_entered target={} scene_no={} z={} pc=0x{:x} call_depth={} scene_stack={}",
            scene_name,
            scene_no,
            z_no,
            self.stream.get_prg_cntr(),
            self.call_stack.len(),
            self.scene_stack.len()
        );
        if ex_call_proc {
            self.mark_excall_script_proc_requested();
        }
        Ok(())
    }

    #[inline(always)]
    fn at_cross_scene_return_boundary(&self) -> bool {
        self.scene_stack
            .last()
            .is_some_and(|saved| saved.call_depth == self.call_stack.len())
    }

    fn return_from_scene(&mut self, args: Vec<Value>) -> Result<bool> {
        let Some(saved) = self.scene_stack.pop() else {
            return Ok(false);
        };
        if saved.call_depth != self.call_stack.len() {
            let expected = saved.call_depth;
            self.scene_stack.push(saved);
            bail!(
                "cross-scene RETURN at wrong call depth: current={} expected={}",
                self.call_stack.len(),
                expected
            );
        }

        let callee = self
            .call_stack
            .pop()
            .ok_or_else(|| anyhow!("call stack underflow returning from scene"))?;
        let (return_pc, ret_form) = self
            .call_stack
            .last()
            .map(|caller| (caller.return_pc, caller.ret_form))
            .ok_or_else(|| anyhow!("caller frame missing returning from scene"))?;

        sg_omv_trace!(self,
            "scene_return restore_scene={:?} restore_line={} ret_form={} args={:?}",
            saved.current_scene_name,
            saved.current_line_no,
            ret_form,
            args
        );

        // Save target-scene locals and reactivate caller locals before changing
        // current_scene_no. Shared include properties stay in-place.
        self.restore_cross_scene_user_prop_scope(saved.current_scene_no);
        self.stream = saved.stream;
        self.user_cmd_names = saved.user_cmd_names;
        self.call_cmd_names = saved.call_cmd_names;
        self.current_scene_no = saved.current_scene_no;
        self.current_scene_name = saved.current_scene_name;
        self.current_line_no = saved.current_line_no;
        self.ctx.current_scene_no = self.current_scene_no.map(|v| v as i64);
        self.ctx.current_scene_name = self.current_scene_name.clone();
        self.ctx.current_line_no = self.current_line_no as i64;
        self.stream.set_prg_cntr(return_pc)?;

        match ret_form {
            f if f == self.cfg.fm_int || f == self.cfg.fm_label => {
                let v = args.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                self.push_int(v);
            }
            f if f == self.cfg.fm_str => {
                let s = args
                    .first()
                    .and_then(|v| v.as_str().map(|s| s.to_string()))
                    .unwrap_or_default();
                self.push_str(s);
            }
            _ => {}
        }

        let was_excall_proc = callee.excall_proc;
        let was_frame_action_proc = callee.frame_action_proc;
        self.recycle_call_frame(callee);
        if was_excall_proc {
            self.mark_excall_script_proc_pop_requested();
        }
        if self.cf_branch_trace_interesting_line() {
            self.sg_cf_branch_trace_emit(
                self.stream.get_prg_cntr(),
                format_args!(
                    "kind=RETURN_RESTORED ret_form={} args={:?}",
                    ret_form,
                    args
                ),
            );
        }
        sg_omv_trace!(self,
            "scene_return_restored scene={:?} scene_no={:?} line={} pc=0x{:x} call_depth={} scene_stack={} frame_action={}",
            self.current_scene_name,
            self.current_scene_no,
            self.current_line_no,
            self.stream.get_prg_cntr(),
            self.call_stack.len(),
            self.scene_stack.len(),
            was_frame_action_proc
        );
        Ok(true)
    }

    fn exec_builtin_global_control(&mut self, form_id: i32, ret_form: i32) -> Result<bool> {
        match form_id {
            constants::elm_value::GLOBAL_SAVEPOINT => {
                // C++ `ELM_GLOBAL_SAVEPOINT` temporarily pushes 1 before
                // `tnm_set_save_point()` and then replaces it with return 0.
                // A later load resumes from the saved stream with that 1 still
                // on the int stack, allowing scripts to distinguish "loaded from
                // this SAVEPOINT" from normal forward execution.
                self.int_stack.push(1);
                self.save_point = Some(self.make_resume_point());
                self.build_local_save_snapshot();
                let _ = self.int_stack.pop();
                if ret_form != self.cfg.fm_void {
                    self.ctx.stack.push(Value::Int(0));
                }
                Ok(true)
            }
            constants::elm_value::GLOBAL_CLEAR_SAVEPOINT => {
                self.save_point = None;
                self.ctx.local_save_snapshot = None;
                Ok(true)
            }
            constants::elm_value::GLOBAL_CHECK_SAVEPOINT => {
                let has = self
                    .ctx
                    .local_save_snapshot
                    .as_ref()
                    .map(|s| !s.local_stream.is_empty())
                    .unwrap_or(false);
                self.ctx.stack.push(Value::Int(if has { 1 } else { 0 }));
                Ok(true)
            }
            constants::elm_value::GLOBAL_SELPOINT => {
                let point = self.make_resume_point();
                self.sel_point_stack.clear();
                self.sel_point_stack.push(point);
                Ok(true)
            }
            constants::elm_value::GLOBAL_CLEAR_SELPOINT => {
                self.sel_point_stack.clear();
                Ok(true)
            }
            constants::elm_value::GLOBAL_CHECK_SELPOINT => {
                self.ctx
                    .stack
                    .push(Value::Int(if self.has_sel_point() { 1 } else { 0 }));
                Ok(true)
            }
            constants::elm_value::GLOBAL_STACK_SELPOINT => {
                let point = self.make_resume_point();
                self.sel_point_stack.push(point);
                Ok(true)
            }
            constants::elm_value::GLOBAL_DROP_SELPOINT => {
                let _ = self.sel_point_stack.pop();
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn exec_syscom_save_value_intlistref(
        &mut self,
        elm: &[i32],
        form_id: i32,
        ret_form: i32,
        args: &[Value],
    ) -> Result<bool> {
        use crate::runtime::forms::codes::{
            elm_value, ELM_ARRAY, FORM_GLOBAL_SYSCOM, FM_SYSCOM,
        };

        if form_id != FORM_GLOBAL_SYSCOM as i32 && form_id != FM_SYSCOM {
            return Ok(false);
        }
        let Some(op) = elm.get(1).copied() else {
            return Ok(false);
        };
        let (quick, write) = match op {
            elm_value::SYSCOM_GET_SAVE_VALUE => (false, false),
            elm_value::SYSCOM_GET_QUICK_SAVE_VALUE => (true, false),
            elm_value::SYSCOM_SET_SAVE_VALUE => (false, true),
            elm_value::SYSCOM_SET_QUICK_SAVE_VALUE => (true, true),
            _ => return Ok(false),
        };

        // All four commands are FM_VOID in def_element_Siglus.h. The second
        // parameter is an actual INTLISTREF; C++ resolves the complete S_element
        // with tnm_get_element_ptr(), so this cannot be reduced to chain[0].
        if ret_form != self.cfg.fm_void {
            return Ok(false);
        }
        let raw_save_no = args.first().and_then(Value::as_i64).unwrap_or(-1);
        let Some(Value::Element(base_chain)) = args.get(1).map(Value::unwrap_named) else {
            return Ok(true);
        };
        if base_chain.is_empty() {
            return Ok(true);
        }
        let flag_index = args.get(2).and_then(Value::as_i64).unwrap_or(0);
        let flag_cnt_raw = args.get(3).and_then(Value::as_i64).unwrap_or(0);
        if flag_cnt_raw <= 0 {
            return Ok(true);
        }
        let flag_cnt = usize::try_from(flag_cnt_raw)
            .unwrap_or(usize::MAX)
            .min(crate::original_save::SAVE_FLAG_MAX_CNT);

        if !write {
            let Some(values) = crate::runtime::forms::syscom::read_save_flag_values(
                &mut self.ctx,
                quick,
                raw_save_no,
                flag_cnt,
            ) else {
                return Ok(true);
            };
            for (i, value) in values.into_iter().enumerate() {
                let Some(index) = flag_index.checked_add(i as i64) else {
                    break;
                };
                if index < 0 {
                    continue;
                }
                let Ok(index) = i32::try_from(index) else {
                    continue;
                };
                let mut target = base_chain.clone();
                target.push(ELM_ARRAY);
                target.push(index);
                self.exec_assign(target, 1, Value::Int(value))?;
            }
        } else {
            let mut values = Vec::with_capacity(flag_cnt);
            for i in 0..flag_cnt {
                let Some(index) = flag_index.checked_add(i as i64) else {
                    break;
                };
                if index < 0 {
                    values.push(0);
                    continue;
                }
                let Ok(index) = i32::try_from(index) else {
                    values.push(0);
                    continue;
                };
                let mut source = base_chain.clone();
                source.push(ELM_ARRAY);
                source.push(index);
                self.exec_property(source)?;
                values.push(i64::from(self.pop_int()?));
            }
            let _ = crate::runtime::forms::syscom::write_save_flag_values(
                &mut self.ctx,
                quick,
                raw_save_no,
                &values,
            );
        }

        // The original command pushes nothing for FM_VOID.
        self.ctx.stack.clear();
        Ok(true)
    }

    fn exec_builtin_scene_form(
        &mut self,
        elm: &[i32],
        form_id: i32,
        al_id: i32,
        ret_form: i32,
        args: &[Value],
    ) -> Result<bool> {
        const FORM_GLOBAL_JUMP: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_JUMP;
        const FORM_GLOBAL_FARCALL: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_FARCALL;
        const FORM_GLOBAL_SYSCOM: i32 = crate::runtime::forms::codes::FORM_GLOBAL_SYSCOM as i32;
        const FORM_SYSCOM: i32 = crate::runtime::forms::codes::FM_SYSCOM;
        const ELM_SYSCOM_CALL_EX: i32 = crate::runtime::forms::codes::elm_value::SYSCOM_CALL_EX;
        if (form_id == FORM_GLOBAL_SYSCOM || form_id == FORM_SYSCOM)
            && elm.get(1).copied() == Some(ELM_SYSCOM_CALL_EX)
        {
            self.sg_omv_trace_command("builtin", elm, form_id, ELM_SYSCOM_CALL_EX, al_id, self.cfg.fm_void, args);
            let scene_name = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let z_no = if al_id == 1 {
                args.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32
            } else {
                0
            };
            let scratch_args = if al_id == 1 && args.len() > 2 {
                &args[2..]
            } else {
                &[]
            };
            self.farcall_scene_name_ex(scene_name, z_no, self.cfg.fm_void, true, scratch_args)?;
            self.ctx.stack.clear();
            return Ok(true);
        }
        if form_id == FORM_GLOBAL_JUMP {
            self.sg_omv_trace_command("builtin", &[], form_id, form_id, al_id, ret_form, args);
            let scene_name = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let z_no = if al_id >= 1 {
                args.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32
            } else {
                0
            };
            if !scene_name.is_empty() {
                self.jump_to_scene_name(scene_name, z_no)?;
            }
            return Ok(true);
        }
        if form_id == FORM_GLOBAL_FARCALL {
            self.sg_omv_trace_command("builtin", &[], form_id, form_id, al_id, ret_form, args);
            let scene_name = args.get(0).and_then(|v| v.as_str()).unwrap_or("");
            let z_no = if al_id >= 1 {
                args.get(1).and_then(|v| v.as_i64()).unwrap_or(0) as i32
            } else {
                0
            };
            if !scene_name.is_empty() {
                self.farcall_scene_name_ex(
                    scene_name,
                    z_no,
                    self.cfg.fm_int,
                    false,
                    if al_id >= 1 && args.len() > 2 {
                        &args[2..]
                    } else {
                        &[]
                    },
                )?;
            } else {
                self.push_default_for_ret(self.cfg.fm_int);
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn take_ctx_return(&mut self, ret_form: i32) -> Result<()> {
        if ret_form == self.cfg.fm_void {
            self.ctx.stack.clear();
            return Ok(());
        }

        let v = self.ctx.pop();
        match ret_form {
            f if f == self.cfg.fm_int || f == self.cfg.fm_label => match v {
                Some(Value::Int(n)) => self.push_int(n as i32),
                Some(Value::NamedArg { value, .. }) => match *value {
                    Value::Int(n) => self.push_int(n as i32),
                    _ => bail!("non-int ctx return for form {}", ret_form),
                },
                Some(_) => bail!("non-int ctx return for form {}", ret_form),
                None => bail!(
                    "missing ctx return int for form {}: scene={} scene_no={} line={} pc=0x{:x} vm_call={:?}",
                    ret_form,
                    self.current_scene_name.as_deref().unwrap_or("<none>"),
                    self.current_scene_no
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| "-".to_string()),
                    self.current_line_no,
                    self.stream.get_prg_cntr(),
                    self.ctx.vm_call
                ),
            },
            f if f == self.cfg.fm_str => match v {
                Some(Value::Str(s)) => self.push_str(s),
                Some(Value::NamedArg { value, .. }) => match *value {
                    Value::Str(s) => self.push_str(s),
                    _ => bail!("non-str ctx return for form {}", ret_form),
                },
                Some(_) => bail!("non-str ctx return for form {}", ret_form),
                None => bail!("missing ctx return str for form {}", ret_form),
            },
            f if f == self.cfg.fm_list => match v {
                Some(Value::Element(elm)) => self.push_element(elm),
                Some(Value::NamedArg { value, .. }) => match *value {
                    Value::Element(elm) => self.push_element(elm),
                    _ => bail!("non-element ctx return for FM_LIST"),
                },
                Some(Value::List(_)) => {
                    bail!("FM_LIST ctx return used raw Value::List; expected element reference")
                }
                Some(_) => bail!("non-element ctx return for FM_LIST"),
                None => bail!("missing ctx return element for FM_LIST"),
            },
            _ => match v {
                Some(Value::Element(elm)) => self.push_element(elm),
                Some(Value::NamedArg { value, .. }) => match *value {
                    Value::Element(elm) => self.push_element(elm),
                    _ => bail!("non-element ctx return for form {}", ret_form),
                },
                Some(_) => bail!("non-element ctx return for form {}", ret_form),
                None => bail!("missing ctx return element for form {}", ret_form),
            },
        }
        Ok(())
    }

    fn push_default_for_ret(&mut self, ret_form: i32) {
        if ret_form == self.cfg.fm_int || ret_form == self.cfg.fm_label {
            self.push_int(0);
        } else if ret_form == self.cfg.fm_str {
            self.push_str(String::new());
        }
    }

    fn update_compact_context_from_element(&mut self, elm: &[i32]) {
        let Some(raw_stage_form) = elm.first().copied() else {
            return;
        };
        if !crate::runtime::forms::stage::is_stage_form_id(&self.ctx, raw_stage_form) {
            return;
        }
        let stage_form = crate::runtime::forms::stage::stage_storage_form_id(&self.ctx, raw_stage_form) as i32;
        let elm_array = if self.ctx.ids.elm_array != 0 {
            self.ctx.ids.elm_array
        } else {
            crate::runtime::forms::codes::ELM_ARRAY
        };
        let stage_object = if self.ctx.ids.stage_elm_object != 0 {
            self.ctx.ids.stage_elm_object
        } else {
            crate::runtime::forms::codes::STAGE_ELM_OBJECT
        };
        let stage_mwnd = crate::runtime::forms::codes::STAGE_ELM_MWND;
        let stage_btnselitem = crate::runtime::forms::codes::STAGE_ELM_BTNSELITEM;

        fn is_array_token(token: i32, elm_array: i32) -> bool {
            token == elm_array || token == crate::runtime::forms::codes::ELM_ARRAY
        }

        fn object_chain_tail_is_plain_object_ref(
            elm: &[i32],
            mut pos: usize,
            elm_array: i32,
        ) -> bool {
            let object_child = crate::runtime::forms::codes::elm_value::OBJECT_CHILD;
            while pos + 2 < elm.len()
                && elm[pos] == object_child
                && is_array_token(elm[pos + 1], elm_array)
            {
                if elm[pos + 2] < 0 {
                    return false;
                }
                pos += 3;
            }
            pos == elm.len()
        }

        let resolved = if elm.len() >= 6
            && crate::runtime::forms::stage::stage_storage_form_id(&self.ctx, elm[0]) as i32 == stage_form
            && is_array_token(elm[1], elm_array)
            && elm[2] >= 0
            && elm[3] == stage_object
            && is_array_token(elm[4], elm_array)
            && elm[5] >= 0
            && object_chain_tail_is_plain_object_ref(elm, 6, elm_array)
        {
            Some((elm[2] as i64, elm[5] as usize))
        } else if elm.len() >= 9
            && crate::runtime::forms::stage::stage_storage_form_id(&self.ctx, elm[0]) as i32 == stage_form
            && is_array_token(elm[1], elm_array)
            && elm[2] >= 0
            && elm[3] == stage_mwnd
            && is_array_token(elm[4], elm_array)
            && elm[5] >= 0
            && matches!(
                elm[6],
                crate::runtime::forms::codes::elm_value::MWND_OBJECT
                    | crate::runtime::forms::codes::elm_value::MWND_BUTTON
                    | crate::runtime::forms::codes::elm_value::MWND_FACE
            )
            && is_array_token(elm[7], elm_array)
            && elm[8] >= 0
            && object_chain_tail_is_plain_object_ref(elm, 9, elm_array)
        {
            Some((elm[2] as i64, elm[8] as usize))
        } else if elm.len() >= 9
            && crate::runtime::forms::stage::stage_storage_form_id(&self.ctx, elm[0]) as i32 == stage_form
            && is_array_token(elm[1], elm_array)
            && elm[2] >= 0
            && elm[3] == stage_btnselitem
            && is_array_token(elm[4], elm_array)
            && elm[5] >= 0
            && elm[6] == crate::runtime::forms::codes::ELM_BTNSELITEM_OBJECT
            && is_array_token(elm[7], elm_array)
            && elm[8] >= 0
            && object_chain_tail_is_plain_object_ref(elm, 9, elm_array)
        {
            Some((elm[2] as i64, elm[8] as usize))
        } else {
            None
        };

        let Some((stage_idx, fallback_obj_idx)) = resolved else {
            return;
        };
        let runtime_slot = self.runtime_slot_from_object_chain(fallback_obj_idx, elm);
        let prev_chain = self.ctx.globals.current_object_chain.clone();
        let prev_stage_object = self.ctx.globals.current_stage_object;
        self.ctx.globals.current_object_chain = Some(elm.to_vec());
        self.ctx.globals.current_stage_object = Some((stage_idx, runtime_slot));
        if self.sg_mwnd_object_trace_enabled() && Self::sg_mwnd_chain_interesting(elm) {
            self.sg_mwnd_object_trace_emit(format_args!(
                "update_compact_context elm={:?} resolved_stage={} fallback_idx={} runtime_slot={} prev_chain={:?} prev_stage_object={:?}",
                elm,
                stage_idx,
                fallback_obj_idx,
                runtime_slot,
                prev_chain,
                prev_stage_object
            ));
        }
    }

    fn update_compact_context_from_object_dispatch_chain(&mut self, elm: &[i32]) {
        let Some(raw_stage_form) = elm.first().copied() else {
            return;
        };
        if !crate::runtime::forms::stage::is_stage_form_id(&self.ctx, raw_stage_form) {
            return;
        }
        let stage_form = crate::runtime::forms::stage::stage_storage_form_id(&self.ctx, raw_stage_form) as i32;
        let elm_array = if self.ctx.ids.elm_array != 0 {
            self.ctx.ids.elm_array
        } else {
            crate::runtime::forms::codes::ELM_ARRAY
        };
        let stage_object = if self.ctx.ids.stage_elm_object != 0 {
            self.ctx.ids.stage_elm_object
        } else {
            crate::runtime::forms::codes::STAGE_ELM_OBJECT
        };
        let stage_mwnd = crate::runtime::forms::codes::STAGE_ELM_MWND;
        let stage_btnselitem = crate::runtime::forms::codes::STAGE_ELM_BTNSELITEM;
        let object_child = crate::runtime::forms::codes::elm_value::OBJECT_CHILD;

        fn is_array_token(token: i32, elm_array: i32) -> bool {
            token == elm_array || token == crate::runtime::forms::codes::ELM_ARRAY
        }

        let mut pos = if elm.len() >= 6
            && crate::runtime::forms::stage::stage_storage_form_id(&self.ctx, elm[0]) as i32 == stage_form
            && is_array_token(elm[1], elm_array)
            && elm[2] >= 0
            && elm[3] == stage_object
            && is_array_token(elm[4], elm_array)
            && elm[5] >= 0
        {
            6usize
        } else if elm.len() >= 9
            && crate::runtime::forms::stage::stage_storage_form_id(&self.ctx, elm[0]) as i32 == stage_form
            && is_array_token(elm[1], elm_array)
            && elm[2] >= 0
            && elm[3] == stage_mwnd
            && is_array_token(elm[4], elm_array)
            && elm[5] >= 0
            && matches!(
                elm[6],
                crate::runtime::forms::codes::elm_value::MWND_OBJECT
                    | crate::runtime::forms::codes::elm_value::MWND_BUTTON
                    | crate::runtime::forms::codes::elm_value::MWND_FACE
            )
            && is_array_token(elm[7], elm_array)
            && elm[8] >= 0
        {
            9usize
        } else if elm.len() >= 9
            && crate::runtime::forms::stage::stage_storage_form_id(&self.ctx, elm[0]) as i32 == stage_form
            && is_array_token(elm[1], elm_array)
            && elm[2] >= 0
            && elm[3] == stage_btnselitem
            && is_array_token(elm[4], elm_array)
            && elm[5] >= 0
            && elm[6] == crate::runtime::forms::codes::ELM_BTNSELITEM_OBJECT
            && is_array_token(elm[7], elm_array)
            && elm[8] >= 0
        {
            9usize
        } else {
            return;
        };

        while pos + 2 < elm.len()
            && elm[pos] == object_child
            && is_array_token(elm[pos + 1], elm_array)
            && elm[pos + 2] >= 0
        {
            pos += 3;
        }

        let object_ref = elm[..pos].to_vec();
        if self.sg_mwnd_object_trace_enabled() && Self::sg_mwnd_chain_interesting(elm) {
            self.sg_mwnd_object_trace_emit(format_args!(
                "update_context_from_dispatch elm={:?} object_ref={:?} pos={}",
                elm,
                object_ref,
                pos
            ));
        }
        self.update_compact_context_from_element(&object_ref);
    }

    fn push_return_value_raw(&mut self, v: Value) {
        match v {
            Value::NamedArg { value, .. } => self.push_return_value_raw(*value),
            Value::Int(n) => self.push_int(n as i32),
            Value::Str(s) => self.push_str(s),
            Value::Element(elm) => {
                self.update_compact_context_from_element(&elm);
                self.push_element(elm);
            }
            Value::List(_) => {
                panic!("raw Value::List reached push_return_value_raw; expected runtime ref");
            }
        }
    }

    // ---------------------------------------------------------------------
    // Arithmetic / comparisons
    // ---------------------------------------------------------------------

    fn exec_operate_1(&mut self, form_code: i32, opr: u8) -> Result<()> {
        if form_code != self.cfg.fm_int {
            self.trace_unknown_form(form_code, "exec_operate_1");
            self.push_int(0);
            return Ok(());
        }

        let v = self.pop_int()?;
        let out = match opr {
            OP_PLUS => v,
            OP_MINUS => v.wrapping_neg(),
            OP_TILDE => !v,
            _ => v,
        };
        if self.cf_condition_trace_interesting_line() {
            self.sg_cf_condition_trace(
                self.stream.get_prg_cntr(),
                format!(
                    "kind=OPERATE_1 op={} in={} out={}",
                    Self::cf_condition_op_name(opr),
                    v,
                    out
                ),
            );
        }
        self.push_int(out);
        Ok(())
    }

    fn exec_operate_2(&mut self, form_l: i32, form_r: i32, opr: u8) -> Result<()> {
        // int/int
        if form_l == self.cfg.fm_int && form_r == self.cfg.fm_int {
            let r = self.pop_int()?;
            let l = self.pop_int()?;
            let out = self.calc_int_int(l, r, opr);
            if self.cf_condition_trace_interesting_line() {
                self.sg_cf_condition_trace(
                    self.stream.get_prg_cntr(),
                    format!(
                        "kind=OPERATE_2 op={} left={} right={} out={}",
                        Self::cf_condition_op_name(opr),
                        l,
                        r,
                        out
                    ),
                );
            }
            self.push_int(out);
            return Ok(());
        }

        // str/int
        if form_l == self.cfg.fm_str && form_r == self.cfg.fm_int {
            let r = self.pop_int()?;
            let l = self.pop_str()?;
            let out = self.calc_str_int(l, r, opr);
            self.push_str(out);
            return Ok(());
        }

        // str/str
        if form_l == self.cfg.fm_str && form_r == self.cfg.fm_str {
            let r = self.pop_str()?;
            let l = self.pop_str()?;
            let out = self.calc_str_str(l, r, opr);
            match out {
                Value::Int(n) => self.push_int(n as i32),
                Value::Str(s) => self.push_str(s),
                _ => {
                    self.push_int(0);
                }
            }
            return Ok(());
        }

        // Unknown combo.
        self.trace_unknown_form(form_l, "exec_operate_2.left");
        self.trace_unknown_form(form_r, "exec_operate_2.right");
        self.push_int(0);
        Ok(())
    }

    fn calc_int_int(&mut self, l: i32, r: i32, opr: u8) -> i32 {
        match opr {
            OP_PLUS => l.wrapping_add(r),
            OP_MINUS => l.wrapping_sub(r),
            OP_MULTIPLE => l.wrapping_mul(r),
            OP_DIVIDE => {
                if r == 0 {
                    0
                } else {
                    l.wrapping_div(r)
                }
            }
            OP_AMARI => {
                if r == 0 {
                    0
                } else {
                    l.wrapping_rem(r)
                }
            }

            OP_EQUAL => (l == r) as i32,
            OP_NOT_EQUAL => (l != r) as i32,
            OP_GREATER => (l > r) as i32,
            OP_GREATER_EQUAL => (l >= r) as i32,
            OP_LESS => (l < r) as i32,
            OP_LESS_EQUAL => (l <= r) as i32,

            OP_LOGICAL_OR => ((l != 0) || (r != 0)) as i32,
            OP_LOGICAL_AND => ((l != 0) && (r != 0)) as i32,

            OP_OR => l | r,
            OP_AND => l & r,
            OP_HAT => l ^ r,
            OP_SL => l.wrapping_shl((r as u32) & 31),
            OP_SR => l.wrapping_shr((r as u32) & 31),
            OP_SR3 => ((l as u32).wrapping_shr((r as u32) & 31)) as i32,

            _ => 0,
        }
    }

    fn calc_str_int(&mut self, s: String, n: i32, opr: u8) -> String {
        match opr {
            OP_MULTIPLE => {
                if n <= 0 {
                    return String::new();
                }
                let mut out = String::new();
                for _ in 0..(n as usize) {
                    out.push_str(&s);
                }
                out
            }
            _ => s,
        }
    }

    fn calc_str_str(&mut self, l: String, r: String, opr: u8) -> Value {
        match opr {
            OP_PLUS => Value::Str(format!("{l}{r}")),
            OP_EQUAL | OP_NOT_EQUAL | OP_GREATER | OP_GREATER_EQUAL | OP_LESS | OP_LESS_EQUAL => {
                // The original engine lowercases for comparisons.
                let ll = l.to_lowercase();
                let rr = r.to_lowercase();
                let cmp = ll.cmp(&rr);
                let b = match opr {
                    OP_EQUAL => cmp == std::cmp::Ordering::Equal,
                    OP_NOT_EQUAL => cmp != std::cmp::Ordering::Equal,
                    OP_GREATER => cmp == std::cmp::Ordering::Greater,
                    OP_GREATER_EQUAL => cmp != std::cmp::Ordering::Less,
                    OP_LESS => cmp == std::cmp::Ordering::Less,
                    OP_LESS_EQUAL => cmp != std::cmp::Ordering::Greater,
                    _ => false,
                };
                Value::Int(b as i64)
            }
            _ => Value::Int(0),
        }
    }
}

#[cfg(test)]
mod user_command_resolution_tests {
    use super::resolve_named_user_command_number;
    use std::collections::HashMap;

    #[test]
    fn include_command_shadows_same_named_scene_command() {
        let include_names = HashMap::from([(2, "Bg_Change".to_string())]);
        let local_names = HashMap::from([(7, "bg_change".to_string())]);
        assert_eq!(
            resolve_named_user_command_number(&include_names, &local_names, 5, "BG_CHANGE"),
            Some((2, true))
        );
    }

    #[test]
    fn scene_command_number_is_offset_by_include_count() {
        let include_names = HashMap::new();
        let local_names = HashMap::from([(7, "local_proc".to_string())]);
        assert_eq!(
            resolve_named_user_command_number(&include_names, &local_names, 5, "LOCAL_PROC"),
            Some((12, false))
        );
    }
}

#[cfg(test)]
mod call_property_reference_tests {
    use super::*;
    use crate::runtime::forms::codes::FM_INTREF;
    use crate::scene_stream::SceneStream;
    use std::path::PathBuf;

    fn empty_scene_chunk() -> Vec<u8> {
        const HEADER_WORDS: usize = 33;
        const HEADER_SIZE: i32 = (HEADER_WORDS * 4) as i32;
        let mut words = [0i32; HEADER_WORDS];
        for idx in [
            0usize, 1, 3, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23, 25, 27, 29, 31,
        ] {
            words[idx] = HEADER_SIZE;
        }
        let mut out = Vec::with_capacity(HEADER_SIZE as usize);
        for word in words {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out
    }

    #[test]
    fn scalar_call_reference_is_resolved_in_two_property_steps() {
        let chunk = empty_scene_chunk();
        let stream = SceneStream::new(&chunk).expect("empty scene stream");
        let ctx = CommandContext::new(PathBuf::from("."));
        let mut vm = SceneVm::new(stream, ctx);

        let user_prop_id = 7u16;
        let target = vec![constants::elm::create(
            constants::elm::OWNER_USER_PROP,
            0,
            user_prop_id as i32,
        )];
        let mut target_cell = UserPropCell::new(vm.cfg.fm_int, target.clone());
        target_cell.int_value = 42;
        vm.user_props.insert(user_prop_id, target_cell);

        let call_prop_id = 0;
        let call_prop_element = vec![constants::elm::create(
            constants::elm::OWNER_CALL_PROP,
            0,
            call_prop_id,
        )];
        vm.call_stack
            .last_mut()
            .expect("base call frame")
            .user_props
            .push(CallProp {
                scn_no: 0,
                prop_id: call_prop_id,
                form: FM_INTREF,
                decl_size: 0,
                element: target.clone(),
                value: CallPropValue::Element(target.clone()),
            });

        // The compiler emits CD_PROPERTY once inside bs_elm_list() to replace
        // CALL_PROP with its referenced element, then emits another CD_PROPERTY
        // when the expression needs the scalar value.
        vm.exec_property(call_prop_element)
            .expect("first reference property step");
        let resolved_target = vm.pop_element().expect("referenced element");
        assert_eq!(resolved_target, target);

        vm.exec_property(resolved_target)
            .expect("second scalar property step");
        assert_eq!(vm.pop_int().expect("referenced int value"), 42);
    }
}

#[cfg(test)]
mod command_dispatch_tests {
    use super::*;
    use crate::runtime::forms::codes::{
        ELM_ARRAY, ELM_INTLIST_GET_SIZE, ELM_INTLIST_RESIZE, ELM_STRLIST_GET_SIZE,
        ELM_STRLIST_RESIZE,
    };
    use crate::scene_stream::SceneStream;
    use std::path::PathBuf;

    fn empty_scene_chunk() -> Vec<u8> {
        const HEADER_WORDS: usize = 33;
        const HEADER_SIZE: i32 = (HEADER_WORDS * 4) as i32;
        let mut words = [0i32; HEADER_WORDS];
        for idx in [
            0usize, 1, 3, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23, 25, 27, 29, 31,
        ] {
            words[idx] = HEADER_SIZE;
        }
        let mut out = Vec::with_capacity(HEADER_SIZE as usize);
        for word in words {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out
    }

    fn test_vm() -> SceneVm<'static> {
        let chunk = Box::leak(empty_scene_chunk().into_boxed_slice());
        let stream = SceneStream::new(chunk).expect("empty scene stream");
        SceneVm::new(stream, CommandContext::new(PathBuf::from(".")))
    }

    #[test]
    fn excall_indexed_stage_creates_menu_objects_and_preserves_properties() {
        use crate::runtime::forms::{codes, excall};

        let mut vm = test_vm();
        vm.exec_command(
            vec![codes::ELM_GLOBAL_EXCALL, codes::ELM_EXCALL_ALLOC],
            0, vm.cfg.fm_void, &mut vec![],
        ).unwrap();
        let stage_form = excall::tick_targets(&vm.ctx).stage_form_id;
        for stage_idx in 0..3 {
            let mut object = vec![
                codes::ELM_GLOBAL_EXCALL, codes::ELM_EXCALL_STAGE, ELM_ARRAY, stage_idx,
                codes::ELM_STAGE_OBJECT, ELM_ARRAY, 10,
            ];
            object.push(codes::ELM_OBJECT_CREATE_RECT);
            vm.exec_command(
                object.clone(), 0, vm.cfg.fm_void,
                &mut [0, 0, 100, 80, 255, 255, 255, 255, 1]
                    .into_iter().map(Value::Int).collect(),
            ).unwrap();
            assert!(vm.ctx.globals.stage_forms[&stage_form].object_lists[&(stage_idx as i64)][10].used);

            *object.last_mut().unwrap() = codes::ELM_OBJECT_X;
            vm.exec_assign(object.clone(), 1, Value::Int(123 + stage_idx as i64)).unwrap();
            vm.exec_property(object).unwrap();
            assert_eq!(vm.pop_int().unwrap(), 123 + stage_idx);
        }
        assert!(!vm.ctx.render_list_with_effects().is_empty(), "menu objects must be drawable");
    }

    #[test]
    fn excall_indexed_stage_button_group_accepts_right_click_cancel() {
        use crate::runtime::forms::codes;
        use crate::runtime::input::VmMouseButton;

        let mut vm = test_vm();
        vm.exec_command(
            vec![codes::ELM_GLOBAL_EXCALL, codes::ELM_EXCALL_ALLOC],
            0, vm.cfg.fm_void, &mut vec![],
        ).unwrap();
        vm.ctx.excall_state.ex_call_flag = true;
        let mut group = vec![
            codes::ELM_GLOBAL_EXCALL, codes::ELM_EXCALL_STAGE, ELM_ARRAY, 1,
            codes::STAGE_ELM_OBJBTNGROUP, ELM_ARRAY, 2, constants::GROUP_INIT,
        ];
        vm.exec_command(group.clone(), 0, vm.cfg.fm_void, &mut vec![]).unwrap();
        *group.last_mut().unwrap() = constants::GROUP_START_CANCEL;
        vm.exec_command(group.clone(), 0, vm.cfg.fm_void, &mut vec![]).unwrap();
        *group.last_mut().unwrap() = constants::GROUP_GET_DECIDED_NO;
        vm.exec_command(group.clone(), 0, vm.cfg.fm_int, &mut vec![]).unwrap();
        assert_eq!(vm.pop_int().unwrap(), -2);
        vm.ctx.on_mouse_down(VmMouseButton::Right);
        vm.ctx.on_mouse_up(VmMouseButton::Right);
        vm.exec_command(group, 0, vm.cfg.fm_int, &mut vec![]).unwrap();
        assert_eq!(vm.pop_int().unwrap(), -1);
    }

    #[test]
    fn dialog_child_buttons_inherit_parent_layer_for_hover_and_click() {
        use crate::runtime::forms::{codes, excall};
        use crate::runtime::input::VmMouseButton;

        for group_no in [-1, 8] {
            let mut vm = test_vm();
            vm.exec_command(
                vec![codes::ELM_GLOBAL_EXCALL, codes::ELM_EXCALL_ALLOC],
                0, vm.cfg.fm_void, &mut vec![],
            ).unwrap();
            vm.ctx.excall_state.ex_call_flag = true;
            let root = vec![codes::ELM_GLOBAL_EXCALL, codes::ELM_EXCALL_FRONT,
                codes::ELM_STAGE_OBJECT, ELM_ARRAY, 69];
            let mut child = root.clone();
            child.extend([codes::ELM_OBJECT_CHILD, ELM_ARRAY, 1]);
            for (object, rect, button_no, layer) in [
                (&root, [0, 0, 400, 200, 255, 255, 255, 255, 1, 100, 200], 111, 300),
                (&child, [0, 0, 80, 40, 255, 255, 255, 255, 1, 20, 30], 1, 0),
            ] {
                let mut command = object.clone();
                command.push(codes::ELM_OBJECT_CREATE_RECT);
                vm.exec_command(command, 2, vm.cfg.fm_void,
                    &mut rect.into_iter().map(Value::Int).collect()).unwrap();
                let mut property = object.clone();
                property.push(codes::ELM_OBJECT_LAYER);
                vm.exec_assign(property, 1, Value::Int(layer)).unwrap();
                let mut command = object.clone();
                command.push(codes::ELM_OBJECT_SET_BUTTON);
                vm.exec_command(command, 2, vm.cfg.fm_void,
                    &mut [button_no, group_no, 1, -1].into_iter().map(Value::Int).collect()).unwrap();
            }
            if group_no >= 0 {
                vm.exec_command(vec![codes::ELM_GLOBAL_EXCALL, codes::ELM_EXCALL_FRONT,
                    codes::STAGE_ELM_OBJBTNGROUP, ELM_ARRAY, group_no as i32,
                    constants::GROUP_START_CANCEL], 0, vm.cfg.fm_void, &mut vec![]).unwrap();
            }
            let form = excall::tick_targets(&vm.ctx).stage_form_id;
            vm.ctx.on_mouse_move(140, 245);
            let obj = &vm.ctx.globals.stage_forms[&form].object_lists[&1][69];
            assert!(obj.runtime.child_objects[1].button.hit, "child must win over the dialog background");
            assert!(!obj.button.hit);
            vm.ctx.on_mouse_down(VmMouseButton::Left);
            assert!(vm.ctx.globals.stage_forms[&form].object_lists[&1][69]
                .runtime.child_objects[1].button.pushed);
            vm.ctx.on_mouse_up(VmMouseButton::Left);
            if group_no >= 0 {
                assert_eq!(vm.ctx.globals.stage_forms[&form].group_lists[&1][group_no as usize]
                    .decided_button_no, 1);
            }
        }
    }

    #[test]
    fn op_seen_flag_reads_and_writes_the_persistent_global_list() {
        use crate::runtime::forms::codes::ELM_GLOBAL_G;
        let mut vm = test_vm();
        let elm = vec![ELM_GLOBAL_G, ELM_ARRAY, 153];
        vm.ctx.globals.int_lists.entry(ELM_GLOBAL_G as u32).or_default().resize(1000, 0);
        vm.ctx.globals.int_lists.get_mut(&(ELM_GLOBAL_G as u32)).unwrap()[153] = 1;
        vm.exec_property(elm.clone()).unwrap();
        assert_eq!(vm.pop_int().unwrap(), 1, "read the OP flag restored from global.sav");
        vm.exec_assign(elm.clone(), 1, Value::Int(2)).unwrap();
        assert_eq!(vm.ctx.globals.int_lists[&(ELM_GLOBAL_G as u32)][153], 2);
        vm.ctx.reset_for_scene_restart();
        vm.exec_property(elm).unwrap();
        assert_eq!(vm.pop_int().unwrap(), 2);
    }

    #[test]
    fn wait_wipe_without_active_transition_returns_zero_each_time() {
        let mut vm = test_vm();
        for _ in 0..2 {
            vm.exec_command(
                vec![constants::elm_value::GLOBAL_WAIT_WIPE],
                0,
                vm.cfg.fm_int,
                &mut vec![],
            ).expect("WAIT_WIPE after the transition has finished");
            assert_eq!(vm.pop_int().unwrap(), 0);
            assert!(!vm.ctx.wait_poll());
            assert!(vm.ctx.stack.is_empty(), "return is delivered only once");
        }
    }

    #[test]
    fn wait_wipe_returns_completion_result_but_inline_wipe_wait_is_void() {
        for (explicit_wait, key_skip) in [(true, false), (true, true), (false, false), (false, true)] {
            let mut vm = test_vm();
            vm.exec_command(
                vec![constants::elm_value::GLOBAL_WIPE],
                0,
                vm.cfg.fm_void,
                &mut vec![],
            ).unwrap();
            if explicit_wait {
                vm.exec_command(
                    vec![constants::elm_value::GLOBAL_WAIT_WIPE],
                    0,
                    vm.cfg.fm_int,
                    &mut vec![Value::NamedArg { id: 0, value: Box::new(Value::Int(1)) }],
                ).unwrap();
            } else {
                vm.ctx.wait.wait_wipe(true);
            }
            assert!(vm.ctx.wait_poll());
            if key_skip {
                assert!(vm.ctx.wait.notify_key(&mut vm.ctx.globals, &vm.ctx.ids));
            }
            vm.ctx.finish_wipe_runtime();
            assert!(!vm.ctx.wait_poll());
            if explicit_wait {
                assert_eq!(vm.ctx.pop().and_then(|v| v.as_i64()), Some(if key_skip { 1 } else { 0 }));
            }
            assert!(vm.ctx.stack.is_empty());
            assert!(!vm.ctx.wait_poll());
            assert!(vm.ctx.stack.is_empty());
        }
    }

    #[test]
    fn wipe_without_arguments_starts_the_default_transition() {
        let mut vm = test_vm();
        vm.exec_command(
            vec![constants::elm_value::GLOBAL_WIPE],
            0,
            vm.cfg.fm_void,
            &mut vec![],
        ).unwrap();
        assert!(vm.ctx.globals.wipe.is_some(), "WIPE() is a real script command");
        assert!(vm.ctx.wait.wipe);
        assert!(vm.is_blocked());
    }

    #[test]
    fn global_title_commands_read_and_update_the_saved_scene_title() {
        let mut vm = test_vm();
        vm.exec_command(
            vec![constants::elm_value::GLOBAL_GET_TITLE], 0, vm.cfg.fm_str, &mut vec![],
        ).unwrap();
        assert_eq!(vm.pop_str().unwrap(), "");

        for title in ["第一章", "エピローグ", ""] {
            vm.exec_command(
                vec![constants::elm_value::GLOBAL_SET_TITLE], 0, vm.cfg.fm_void,
                &mut vec![Value::Str(title.into())],
            ).unwrap();
            vm.exec_command(
                vec![constants::elm_value::GLOBAL_GET_TITLE], 0, vm.cfg.fm_str, &mut vec![],
            ).unwrap();
            assert_eq!(vm.pop_str().unwrap(), title);
            assert!(vm.ctx.stack.is_empty());
            vm.build_local_save_snapshot();
            assert_eq!(vm.ctx.local_save_snapshot.as_ref().unwrap().save_scene_title, title);
        }
    }

    #[test]
    fn global_get_scene_name_returns_active_scene_on_string_stack() {
        let mut vm = test_vm();
        for scene in ["_start", "menu", "frame_action_scene"] {
            vm.current_scene_name = Some(scene.into());
            vm.ctx.current_scene_name = Some(scene.into());
            vm.exec_command(
                vec![constants::elm_value::GLOBAL_GET_SCENE_NAME],
                0,
                vm.cfg.fm_str,
                &mut vec![],
            ).unwrap();
            assert_eq!(vm.pop_str().unwrap(), scene);
            assert!(vm.ctx.stack.is_empty());
        }
    }

    #[test]
    fn global_owari_yields_to_host_for_exit_without_confirmation() {
        use crate::runtime::globals::SyscomPendingProcKind;

        let mut vm = test_vm();
        vm.ctx.globals.syscom.menu_open = true;
        let generation = vm.ctx.proc_generation();
        vm.exec_command(
            vec![constants::elm_value::GLOBAL_OWARI],
            0, vm.cfg.fm_void, &mut vec![],
        ).expect("OWARI requests normal host shutdown");
        let pending = vm.ctx.globals.syscom.pending_proc.as_ref().unwrap();
        assert_eq!(pending.kind, SyscomPendingProcKind::EndGame);
        assert!(!pending.warning && !pending.se_play && !pending.fade_out);
        assert!(!vm.ctx.globals.syscom.menu_open);
        assert_ne!(vm.ctx.proc_generation(), generation);
        assert!(vm.ctx.stack.is_empty());
    }

    #[test]
    fn global_returnmenu_yields_to_host_with_optional_scene_and_label() {
        use crate::runtime::globals::SyscomPendingProcKind;

        let mut vm = test_vm();
        for (al_id, mut args, expected) in [
            (2, vec![Value::Str("menu".into()), Value::Int(7)], Some(("menu".into(), 7))),
            (1, vec![Value::Str("title".into())], Some(("title".into(), 0))),
            (0, vec![], None),
        ] {
            let generation = vm.ctx.proc_generation();
            vm.exec_command(
                vec![constants::elm_value::GLOBAL_RETURNMENU],
                al_id,
                vm.cfg.fm_void,
                &mut args,
            ).unwrap();
            assert_ne!(vm.ctx.proc_generation(), generation);
            assert_eq!(vm.ctx.pending_menu_scene, expected);
            let pending = vm.ctx.globals.syscom.pending_proc.as_ref().unwrap();
            assert_eq!(pending.kind, SyscomPendingProcKind::ReturnToMenu);
            assert!(!pending.warning && !pending.se_play && !pending.fade_out);
            assert!(vm.ctx.stack.is_empty());
            assert!(!vm.halted);
        }
        vm.ctx.pending_menu_scene = Some(("stale".into(), 7));
        vm.ctx.reset_for_scene_restart();
        assert!(vm.ctx.pending_menu_scene.is_none());
        assert!(vm.ctx.globals.syscom.pending_proc.is_none());
    }

    #[test]
    fn user_string_commands_extract_map_date_and_time() {
        use crate::runtime::forms::codes::str_op;

        let mut vm = test_vm();
        let prop_id = 119;
        vm.assign_user_prop(prop_id, None, Value::Str("0729a".into()));
        let head = constants::elm::create(constants::elm::OWNER_USER_PROP, 0, prop_id as i32);
        for (start, len, expected) in [(0, 4, "0729"), (4, 1, "a")] {
            vm.exec_command(
                vec![head, str_op::MID],
                1,
                vm.cfg.fm_str,
                &mut vec![Value::Int(start), Value::Int(len)],
            )
            .expect("string MID command");
            assert_eq!(vm.pop_str().expect("substring"), expected);
        }
        vm.exec_command(vec![head, str_op::CNT], 0, vm.cfg.fm_int, &mut vec![])
            .expect("string CNT command");
        assert_eq!(vm.pop_int().expect("string length"), 5);
        assert_eq!(vm.user_props[&prop_id].str_value, "0729a");
    }

    #[test]
    fn user_prop_strlist_resize_and_get_size_are_not_silently_ignored() {
        let mut vm = test_vm();
        let prop_id = 0u16;
        let mut cell = UserPropCell::new(
            vm.cfg.fm_strlist,
            vm.default_user_prop_element(prop_id, vm.cfg.fm_strlist),
        );
        cell.str_list.clear();
        vm.user_props.insert(prop_id, cell);

        assert!(vm
            .exec_user_prop_list_command(
                prop_id,
                &[ELM_STRLIST_RESIZE],
                0,
                vm.cfg.fm_void,
                &[Value::Int(1)],
            )
            .expect("STRLIST.RESIZE"));
        assert_eq!(vm.user_props[&prop_id].str_list.len(), 1);

        assert!(vm
            .exec_user_prop_list_command(
                prop_id,
                &[ELM_STRLIST_GET_SIZE],
                0,
                vm.cfg.fm_int,
                &[],
            )
            .expect("STRLIST.GET_SIZE"));
        assert_eq!(vm.pop_int().expect("STRLIST size"), 1);
    }

    #[test]
    fn user_prop_intlist_resize_and_get_size_are_not_silently_ignored() {
        let mut vm = test_vm();
        let prop_id = 1u16;
        let mut cell = UserPropCell::new(
            vm.cfg.fm_intlist,
            vm.default_user_prop_element(prop_id, vm.cfg.fm_intlist),
        );
        cell.int_list.clear();
        vm.user_props.insert(prop_id, cell);

        assert!(vm
            .exec_user_prop_list_command(
                prop_id,
                &[ELM_INTLIST_RESIZE],
                0,
                vm.cfg.fm_void,
                &[Value::Int(1)],
            )
            .expect("INTLIST.RESIZE"));
        assert_eq!(vm.user_props[&prop_id].int_list.len(), 1);

        assert!(vm
            .exec_user_prop_list_command(
                prop_id,
                &[ELM_INTLIST_GET_SIZE],
                0,
                vm.cfg.fm_int,
                &[],
            )
            .expect("INTLIST.GET_SIZE"));
        assert_eq!(vm.pop_int().expect("INTLIST size"), 1);
    }

    #[test]
    fn negative_user_prop_list_index_does_not_replace_the_list_root() {
        let mut vm = test_vm();
        let prop_id = 0u16;
        let mut cell = UserPropCell::new(
            vm.cfg.fm_strlist,
            vm.default_user_prop_element(prop_id, vm.cfg.fm_strlist),
        );
        cell.str_list = vec!["keep".to_string()];
        vm.user_props.insert(prop_id, cell);

        let head = constants::elm::create(
            constants::elm::OWNER_USER_PROP,
            0,
            prop_id as i32,
        );
        vm.exec_assign(
            vec![head, ELM_ARRAY, -1],
            1,
            Value::Str("wrong".to_string()),
        )
        .expect("negative list assignment");

        let cell = &vm.user_props[&prop_id];
        assert_eq!(cell.form, vm.cfg.fm_strlist);
        assert_eq!(cell.str_list, vec!["keep".to_string()]);
    }
}
#[cfg(test)]
mod call_frame_save_metadata_tests {
    use super::*;
    use crate::original_save::{OriginalStreamReader, OriginalStreamWriter};
    use crate::scene_stream::SceneStream;
    use std::path::PathBuf;

    fn empty_scene_chunk() -> Vec<u8> {
        const HEADER_WORDS: usize = 33;
        const HEADER_SIZE: i32 = (HEADER_WORDS * 4) as i32;
        let mut words = [0i32; HEADER_WORDS];
        for idx in [
            0usize, 1, 3, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23, 25, 27, 29, 31,
        ] {
            words[idx] = HEADER_SIZE;
        }
        let mut out = Vec::with_capacity(HEADER_SIZE as usize);
        for word in words {
            out.extend_from_slice(&word.to_le_bytes());
        }
        out
    }

    #[test]
    fn call_frame_round_trip_keeps_caller_lexer_metadata() {
        let chunk = Box::leak(empty_scene_chunk().into_boxed_slice());
        let stream = SceneStream::new(chunk).expect("empty scene stream");
        let vm = SceneVm::new(stream, CommandContext::new(PathBuf::from(".")));
        let frame = CallFrame {
            call_type: 2,
            return_pc: 0x1234,
            return_scene_no: Some(7),
            return_scene_name: Some("caller_scene".to_string()),
            return_line_no: 2605,
            ret_form: vm.cfg.fm_void,
            return_override: None,
            excall_proc: false,
            frame_action_proc: false,
            arg_cnt: 0,
            delayed_ret_form: None,
            user_props: Vec::new(),
            int_args: Vec::new(),
            str_args: Vec::new(),
        };
        let mut writer = OriginalStreamWriter::new();
        vm.write_cpp_call_frame(&mut writer, &frame);
        let bytes = writer.into_inner();
        let mut reader = OriginalStreamReader::new(&bytes);
        let restored = vm.read_cpp_call_frame(&mut reader).expect("call frame");
        assert_eq!(restored.call_type, 2);
        assert_eq!(restored.return_pc, 0x1234);
        assert_eq!(restored.return_scene_name.as_deref(), Some("caller_scene"));
        assert_eq!(restored.return_line_no, 2605);
    }
}
