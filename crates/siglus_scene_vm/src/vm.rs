//! Scene VM

use anyhow::{anyhow, bail, Result};
use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::elm_code;
use crate::runtime::globals::{
    ObjectFrameActionState, PendingButtonAction, PendingButtonActionKind, PendingFrameActionFinish,
};
use crate::runtime::{self, constants, CommandContext, Value};
use crate::scene_stream::SceneStream;
use siglus_assets::scene_pck::{find_scene_pck_in_project, ScenePck, ScenePckDecodeOptions};

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

// C++ call local scratch lists cur_call.L / cur_call.K are fixed-size 32-slot lists.
const CALL_SCRATCH_SIZE: usize = 32;

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
struct CallProp {
    prop_id: i32,
    form: i32,
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
    return_pc: usize,
    ret_form: i32,
    return_override: Option<(usize, i32)>,
    excall_proc: bool,
    frame_action_proc: bool,
    arg_cnt: usize,
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

#[derive(Debug, Clone)]
struct SceneExecFrame<'a> {
    stream: SceneStream<'a>,
    user_cmd_names: std::collections::HashMap<u32, String>,
    call_cmd_names: std::collections::HashMap<u32, String>,
    int_stack: Vec<i32>,
    str_stack: Vec<String>,
    element_points: Vec<usize>,
    call_stack: Vec<CallFrame>,
    gosub_return_stack: Vec<(usize, i32)>,
    user_props: BTreeMap<u16, UserPropCell>,
    current_scene_no: Option<usize>,
    current_scene_name: Option<String>,
    current_line_no: i32,
    ret_form: i32,
    excall_proc: bool,
}


#[derive(Debug, Clone)]
struct InterpreterExecState<'a> {
    stream: SceneStream<'a>,
    user_cmd_names: std::collections::HashMap<u32, String>,
    call_cmd_names: std::collections::HashMap<u32, String>,
    int_stack: Vec<i32>,
    str_stack: Vec<String>,
    element_points: Vec<usize>,
    call_stack: Vec<CallFrame>,
    gosub_return_stack: Vec<(usize, i32)>,
    user_props: BTreeMap<u16, UserPropCell>,
    scene_stack: Vec<SceneExecFrame<'a>>,
    current_scene_no: Option<usize>,
    current_scene_name: Option<String>,
    current_line_no: i32,
}

#[derive(Debug, Clone)]
struct VmResumePoint<'a> {
    stream: SceneStream<'a>,
    user_cmd_names: std::collections::HashMap<u32, String>,
    call_cmd_names: std::collections::HashMap<u32, String>,
    int_stack: Vec<i32>,
    str_stack: Vec<String>,
    element_points: Vec<usize>,
    call_stack: Vec<CallFrame>,
    gosub_return_stack: Vec<(usize, i32)>,
    user_props: BTreeMap<u16, UserPropCell>,
    current_scene_no: Option<usize>,
    current_scene_name: Option<String>,
    current_line_no: i32,
    globals: runtime::globals::GlobalState,
}

pub struct SceneVm<'a> {
    pub cfg: VmConfig,
    stream: SceneStream<'a>,

    pub ctx: CommandContext,

    // Stack model: separate int/str stacks plus element point list.
    int_stack: Vec<i32>,
    str_stack: Vec<String>,
    element_points: Vec<usize>,

    call_stack: Vec<CallFrame>,
    gosub_return_stack: Vec<(usize, i32)>,
    user_props: BTreeMap<u16, UserPropCell>,
    scene_stack: Vec<SceneExecFrame<'a>>,
    save_point: Option<VmResumePoint<'a>>,
    sel_point_stack: Vec<VmResumePoint<'a>>,
    current_scene_no: Option<usize>,
    current_scene_name: Option<String>,
    current_line_no: i32,

    pub unknown_opcodes: BTreeMap<u8, u64>,
    pub unknown_forms: BTreeMap<i32, u64>,

    steps: u64,
    halted: bool,

    // When a command triggers a VM wait (movie wait-key etc.), its return value is produced when the wait completes.
    delayed_ret_form: Option<i32>,
    script_input_synced_this_frame: bool,
    yield_safe_after_step: bool,

    user_cmd_names: std::collections::HashMap<u32, String>,
    call_cmd_names: std::collections::HashMap<u32, String>,

    // C++ keeps the lexer / scene package resident. Do not reload and rebuild
    // Scene.pck for frame-action callbacks or scene-local user command calls.
    scene_pck_cache: Option<ScenePck>,
    scene_stream_cache: BTreeMap<usize, SceneStream<'a>>,
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

impl<'a> SceneVm<'a> {
    fn trace_unknown_form(&mut self, form_code: i32, site: &str) {
        *self.unknown_forms.entry(form_code).or_insert(0) += 1;
        if std::env::var_os("SIGLUS_TRACE_UNKNOWN_FORMS").is_some() {
            eprintln!(
                "[vm unknown form] site={} form={} pc=0x{:x}",
                site,
                form_code,
                self.stream.get_prg_cntr()
            );
        }
    }

    fn blank_call_int_args() -> Vec<i32> {
        vec![0; CALL_SCRATCH_SIZE]
    }

    fn blank_call_str_args() -> Vec<String> {
        vec![String::new(); CALL_SCRATCH_SIZE]
    }

    fn make_call_frame(
        &self,
        ret_form: i32,
        excall_proc: bool,
        frame_action_proc: bool,
        arg_cnt: usize,
        scratch_args: Option<(Vec<i32>, Vec<String>)>,
    ) -> CallFrame {
        let (int_args, str_args) = scratch_args
            .unwrap_or_else(|| (Self::blank_call_int_args(), Self::blank_call_str_args()));
        CallFrame {
            return_pc: 0,
            ret_form,
            return_override: None,
            excall_proc,
            frame_action_proc,
            arg_cnt,
            user_props: Vec::new(),
            int_args,
            str_args,
        }
    }

    fn capture_interpreter_exec_state(&self) -> InterpreterExecState<'a> {
        InterpreterExecState {
            stream: self.stream.clone(),
            user_cmd_names: self.user_cmd_names.clone(),
            call_cmd_names: self.call_cmd_names.clone(),
            int_stack: self.int_stack.clone(),
            str_stack: self.str_stack.clone(),
            element_points: self.element_points.clone(),
            call_stack: self.call_stack.clone(),
            gosub_return_stack: self.gosub_return_stack.clone(),
            user_props: self.user_props.clone(),
            scene_stack: self.scene_stack.clone(),
            current_scene_no: self.current_scene_no,
            current_scene_name: self.current_scene_name.clone(),
            current_line_no: self.current_line_no,
        }
    }

    fn restore_interpreter_exec_state(&mut self, saved: InterpreterExecState<'a>) {
        self.stream = saved.stream;
        self.user_cmd_names = saved.user_cmd_names;
        self.call_cmd_names = saved.call_cmd_names;
        self.int_stack = saved.int_stack;
        self.str_stack = saved.str_stack;
        self.element_points = saved.element_points;
        self.call_stack = saved.call_stack;
        self.gosub_return_stack = saved.gosub_return_stack;
        self.user_props = saved.user_props;
        self.scene_stack = saved.scene_stack;
        self.current_scene_no = saved.current_scene_no;
        self.current_scene_name = saved.current_scene_name;
        self.current_line_no = saved.current_line_no;
        self.ctx.current_scene_no = self.current_scene_no.map(|v| v as i64);
        self.ctx.current_scene_name = self.current_scene_name.clone();
        self.ctx.current_line_no = self.current_line_no as i64;
    }

    pub fn new(stream: SceneStream<'a>, ctx: CommandContext) -> Self {
        let cfg = VmConfig::from_env();
        let user_cmd_names = stream.scn_cmd_name_map.clone();
        let base_call = CallFrame {
            return_pc: 0,
            ret_form: cfg.fm_void,
            return_override: None,
            excall_proc: false,
            frame_action_proc: false,
            arg_cnt: 0,
            user_props: Vec::new(),
            int_args: Self::blank_call_int_args(),
            str_args: Self::blank_call_str_args(),
        };
        Self {
            cfg,
            stream,
            ctx,
            int_stack: Vec::new(),
            str_stack: Vec::new(),
            element_points: Vec::new(),
            call_stack: vec![base_call],
            gosub_return_stack: Vec::new(),
            user_props: BTreeMap::new(),
            scene_stack: Vec::new(),
            save_point: None,
            sel_point_stack: Vec::new(),
            current_scene_no: None,
            current_scene_name: None,
            current_line_no: -1,
            unknown_opcodes: BTreeMap::new(),
            unknown_forms: BTreeMap::new(),

            steps: 0,
            halted: false,
            delayed_ret_form: None,
            script_input_synced_this_frame: false,
            yield_safe_after_step: false,
            user_cmd_names,
            call_cmd_names: std::collections::HashMap::new(),
            scene_pck_cache: None,
            scene_stream_cache: BTreeMap::new(),
        }
    }

    pub fn with_config(cfg: VmConfig, stream: SceneStream<'a>, ctx: CommandContext) -> Self {
        let user_cmd_names = stream.scn_cmd_name_map.clone();
        let base_call = CallFrame {
            return_pc: 0,
            ret_form: cfg.fm_void,
            return_override: None,
            excall_proc: false,
            frame_action_proc: false,
            arg_cnt: 0,
            user_props: Vec::new(),
            int_args: Self::blank_call_int_args(),
            str_args: Self::blank_call_str_args(),
        };
        Self {
            cfg,
            stream,
            ctx,
            int_stack: Vec::new(),
            str_stack: Vec::new(),
            element_points: Vec::new(),
            call_stack: vec![base_call],
            gosub_return_stack: Vec::new(),
            user_props: BTreeMap::new(),
            scene_stack: Vec::new(),
            save_point: None,
            sel_point_stack: Vec::new(),
            current_scene_no: None,
            current_scene_name: None,
            current_line_no: -1,
            unknown_opcodes: BTreeMap::new(),
            unknown_forms: BTreeMap::new(),

            steps: 0,
            halted: false,
            delayed_ret_form: None,
            script_input_synced_this_frame: false,
            yield_safe_after_step: false,
            user_cmd_names,
            call_cmd_names: std::collections::HashMap::new(),
            scene_pck_cache: None,
            scene_stream_cache: BTreeMap::new(),
        }
    }

    pub fn is_blocked(&mut self) -> bool {
        self.ctx.wait_poll()
    }

    pub fn is_halted(&self) -> bool {
        self.halted
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

    fn vm_trace_matches(&self) -> bool {
        if std::env::var_os("SIGLUS_TRACE_VM").is_none() {
            return false;
        }
        if let Ok(filter) = std::env::var("SIGLUS_TRACE_VM_SCENE") {
            if !filter.is_empty() && self.current_scene_name.as_deref() != Some(filter.as_str()) {
                return false;
            }
        }
        if let Ok(range) = std::env::var("SIGLUS_TRACE_VM_PC") {
            if let Some((start, end)) = range.split_once("..") {
                let parse = |s: &str| {
                    usize::from_str_radix(s.trim_start_matches("0x"), 16)
                        .or_else(|_| s.parse::<usize>())
                };
                if let (Ok(start), Ok(end)) = (parse(start), parse(end)) {
                    let pc = self.stream.get_prg_cntr();
                    if pc < start || pc > end {
                        return false;
                    }
                }
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

    fn vm_trace(&self, pc: Option<usize>, msg: impl AsRef<str>) {
        if !self.vm_trace_matches() {
            return;
        }
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
            msg.as_ref(),
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

    fn vm_trace_opcode(&self, pc: usize, opcode: u8, phase: &str) {
        if !self.vm_trace_matches() {
            return;
        }
        self.vm_trace(
            Some(pc),
            format!(
                "{} opcode={}({:#04x})",
                phase,
                Self::vm_opcode_name(opcode),
                opcode
            ),
        );
    }
    fn sg_debug_enabled() -> bool {
        std::env::var_os("SG_DEBUG").is_some()
    }

    fn sg_omv_trace(&self, msg: impl AsRef<str>) {
        if !Self::sg_debug_enabled() {
            return;
        }
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
            msg.as_ref()
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
        if !Self::sg_debug_enabled() {
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
        self.sg_omv_trace(format!(
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
    }

    fn mark_excall_script_proc_pop_requested(&mut self) {
        self.ctx.excall_state.ex_call_flag = false;
        self.ctx.excall_state.script_proc_pop_requested = true;
        self.ctx.input.clear_all();
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
        let base_depth = self.call_stack.len();
        let saved_halted = self.halted;
        let saved_scene_no = self.current_scene_no;
        let saved_pc = self.stream.get_prg_cntr();
        let saved_call_stack = self.call_stack.clone();
        let saved_caller_return = self
            .call_stack
            .last()
            .map(|caller| (caller.return_pc, caller.ret_form));
        let saved_int_stack = self.int_stack.clone();
        let saved_str_stack = self.str_stack.clone();
        let saved_element_points = self.element_points.clone();
        let saved_gosub_return_stack = self.gosub_return_stack.clone();

        if let Some(caller) = self.call_stack.last_mut() {
            if std::env::var_os("SIGLUS_TRACE_CALL_RETURN_PC").is_some() {
                eprintln!(
                    "[SG_CALL_PC] inline set cmd={} depth={} saved_pc=0x{:x} return_pc=0x{:x} old=0x{:x}",
                    cmd_name,
                    base_depth,
                    saved_pc,
                    return_pc,
                    caller.return_pc
                );
            }
            caller.return_pc = return_pc;
            caller.ret_form = ret_form;
        }
        for arg in call_args {
            self.push_call_arg_value(arg);
        }
        self.call_stack.push(self.make_call_frame(
            self.cfg.fm_void,
            false,
            frame_action_proc,
            call_args.len(),
            None,
        ));
        self.stream.set_prg_cntr(offset)?;

        if std::env::var_os("SIGLUS_TRACE_FRAME_ACTION_CALL").is_some() {
            eprintln!(
                "[SG_FRAME_ACTION_CALL] run cmd={} scene={:?} offset=0x{:x} return_pc=0x{:x} args={:?}",
                cmd_name,
                self.current_scene_no,
                offset,
                return_pc,
                call_args
            );
        }

        let max_steps = std::env::var("SIGLUS_INLINE_USER_CMD_MAX_STEPS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
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
                // Inline user commands are isolated script-proc calls.  Once
                // their temporary call frame has returned, control belongs
                // back to the outer VM even if the inner script's restored PC
                // is not the synthetic continuation we installed.  Continuing
                // here can execute data bytes after a nested gosub return.
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
            if self.int_stack.len() > saved_int_stack.len() {
                self.int_stack.last().copied().map(|v| Value::Int(v as i64))
            } else {
                None
            }
        } else if ret_form == self.cfg.fm_str {
            if self.str_stack.len() > saved_str_stack.len() {
                self.str_stack.last().cloned().map(Value::Str)
            } else {
                None
            }
        } else {
            None
        };

        if self.current_scene_no == saved_scene_no {
            self.int_stack = saved_int_stack;
            self.str_stack = saved_str_stack;
            self.element_points = saved_element_points;
            self.gosub_return_stack = saved_gosub_return_stack;
            self.call_stack = saved_call_stack;
            self.halted = saved_halted;
            self.stream.set_prg_cntr(saved_pc)?;
        }
        if let (Some((return_pc, ret_form)), Some(caller)) =
            (saved_caller_return, self.call_stack.get_mut(base_depth.saturating_sub(1)))
        {
            if std::env::var_os("SIGLUS_TRACE_CALL_RETURN_PC").is_some() {
                eprintln!(
                    "[SG_CALL_PC] inline restore cmd={} depth={} return_pc=0x{:x} old=0x{:x}",
                    cmd_name,
                    base_depth,
                    return_pc,
                    caller.return_pc
                );
            }
            caller.return_pc = return_pc;
            caller.ret_form = ret_form;
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
        if self.scene_pck_cache.is_none() {
            let scene_pck_path = find_scene_pck_in_project(&self.ctx.project_dir)?;
            let opt = ScenePckDecodeOptions::from_project_dir(&self.ctx.project_dir)?;
            self.scene_pck_cache = Some(ScenePck::load_and_rebuild(&scene_pck_path, &opt)?);
        }
        Ok(())
    }

    fn cached_scene_stream(&mut self, scene_no: usize) -> Result<SceneStream<'a>> {
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
        Ok(self
            .scene_stream_cache
            .get(&scene_no)
            .expect("scene stream cached")
            .clone())
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
        let saved_user_props = std::mem::take(&mut self.user_props);

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
        self.user_props = saved_user_props;
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

        let current_scene_no = self.current_scene_no;
        let is_current_scene = match scn_name {
            None => true,
            Some(name) if name.is_empty() => true,
            Some(name) => self
                .current_scene_name
                .as_deref()
                .map(|cur| cur.eq_ignore_ascii_case(name))
                .unwrap_or(false),
        };

        // Original C_elm_frame_action::restruct resolves m_scn_no/m_cmd_no
        // against the loaded lexer.  The per-frame action path must not reload
        // and rebuild Scene.pck every frame.
        if is_current_scene {
            let Some(_target_scene_no) = current_scene_no else {
                return Ok(false);
            };
            let cmd_no = match self.user_cmd_names.iter().find_map(|(no, name)| {
                if name.eq_ignore_ascii_case(cmd_name) {
                    Some(*no as usize)
                } else {
                    None
                }
            }) {
                Some(v) => v,
                None => {
                    if std::env::var_os("SIGLUS_TRACE_FRAME_ACTION_CALL").is_some() {
                        eprintln!(
                            "[SG_FRAME_ACTION_CALL] current-scene user command not found: cmd={} scene={:?} scn_name={:?}",
                            cmd_name,
                            self.current_scene_no,
                            scn_name
                        );
                    }
                    return Ok(false);
                }
            };
            let offset = self.stream.scn_cmd_offset(cmd_no)?;
            let return_pc = self.stream.get_prg_cntr();
            return self.run_user_cmd_inline_at_offset(
                cmd_name,
                offset,
                return_pc,
                None,
                Some(return_pc),
                ret_form,
                call_args,
                frame_action_proc,
            );
        }

        let Some(name) = scn_name.filter(|name| !name.is_empty()) else {
            return Ok(false);
        };
        self.ensure_scene_pck_cache()?;
        let Some(target_scene_no) = self
            .scene_pck_cache
            .as_ref()
            .expect("scene pck cache initialized")
            .find_scene_no(name)
        else {
            if std::env::var_os("SIGLUS_TRACE_FRAME_ACTION_CALL").is_some() {
                eprintln!(
                    "[SG_FRAME_ACTION_CALL] target scene not found: scn_name={} cmd={}",
                    name, cmd_name
                );
            }
            return Ok(false);
        };

        let target_stream = self.cached_scene_stream(target_scene_no)?;
        let cmd_no = match target_stream
            .scn_cmd_name_map
            .iter()
            .find_map(|(no, name)| {
                if name.eq_ignore_ascii_case(cmd_name) {
                    Some(*no as usize)
                } else {
                    None
                }
            }) {
            Some(v) => v,
            None => {
                if std::env::var_os("SIGLUS_TRACE_FRAME_ACTION_CALL").is_some() {
                    eprintln!(
                        "[SG_FRAME_ACTION_CALL] user command not found: cmd={} target_scene={} scn_name={:?}",
                        cmd_name,
                        target_scene_no,
                        scn_name
                    );
                }
                return Ok(false);
            }
        };
        let offset = target_stream.scn_cmd_offset(cmd_no)?;
        self.run_scene_user_cmd_inline_at_cached_scene_offset(
            target_scene_no,
            cmd_name,
            offset,
            call_args,
            ret_form,
            false,
            frame_action_proc,
        )
    }

    fn run_scene_user_cmd_frame_action_proc(
        &mut self,
        scn_name: Option<&str>,
        cmd_name: &str,
        call_args: &[Value],
    ) -> Result<bool> {
        let saved_exec = self.capture_interpreter_exec_state();
        let saved_scene_no = self.current_scene_no;
        let saved_scene_stack_len = self.scene_stack.len();
        let saved_call_depth = self.call_stack.len();

        let current_scene_no = self.current_scene_no;
        let is_current_scene = match scn_name {
            None => true,
            Some(name) if name.is_empty() => true,
            Some(name) => self
                .current_scene_name
                .as_deref()
                .map(|cur| cur.eq_ignore_ascii_case(name))
                .unwrap_or(false),
        };

        if is_current_scene {
            let Some(_) = current_scene_no else {
                return Ok(false);
            };
            let Some(cmd_no) = self.user_cmd_names.iter().find_map(|(no, name)| {
                if name.eq_ignore_ascii_case(cmd_name) {
                    Some(*no as usize)
                } else {
                    None
                }
            }) else {
                if std::env::var_os("SIGLUS_TRACE_FRAME_ACTION_CALL").is_some() {
                    eprintln!(
                        "[SG_FRAME_ACTION_CALL] current-scene user command not found: cmd={} scene={:?} scn_name={:?}",
                        cmd_name,
                        self.current_scene_no,
                        scn_name
                    );
                }
                return Ok(false);
            };
            let offset = self.stream.scn_cmd_offset(cmd_no)?;
            self.enter_current_scene_user_cmd_proc_at_offset(
                offset,
                self.cfg.fm_void,
                call_args,
                false,
                true,
            )?;
        } else {
            let Some(name) = scn_name.filter(|name| !name.is_empty()) else {
                return Ok(false);
            };
            self.ensure_scene_pck_cache()?;
            let Some(target_scene_no) = self
                .scene_pck_cache
                .as_ref()
                .expect("scene pck cache initialized")
                .find_scene_no(name)
            else {
                if std::env::var_os("SIGLUS_TRACE_FRAME_ACTION_CALL").is_some() {
                    eprintln!(
                        "[SG_FRAME_ACTION_CALL] target scene not found: scn_name={} cmd={}",
                        name, cmd_name
                    );
                }
                return Ok(false);
            };

            let target_stream = self.cached_scene_stream(target_scene_no)?;
            let Some(cmd_no) = target_stream.scn_cmd_name_map.iter().find_map(|(no, name)| {
                if name.eq_ignore_ascii_case(cmd_name) {
                    Some(*no as usize)
                } else {
                    None
                }
            }) else {
                if std::env::var_os("SIGLUS_TRACE_FRAME_ACTION_CALL").is_some() {
                    eprintln!(
                        "[SG_FRAME_ACTION_CALL] user command not found: cmd={} target_scene={} scn_name={:?}",
                        cmd_name,
                        target_scene_no,
                        scn_name
                    );
                }
                return Ok(false);
            };
            let offset = target_stream.scn_cmd_offset(cmd_no)?;
            self.enter_scene_user_cmd_at_scene_offset_ex(
                target_scene_no,
                offset,
                call_args,
                self.cfg.fm_void,
                false,
                true,
            )?;
        }

        if std::env::var_os("SIGLUS_TRACE_FRAME_ACTION_CALL").is_some() {
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
        let max_steps = std::env::var("SIGLUS_FRAME_ACTION_MAX_STEPS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
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

        // A frame-action user command is invoked from the frame phase, not from
        // the main SCRIPT proc.  Original Siglus runs it synchronously via
        // tnm_proc_script(), and CD_RETURN with frame_action_flag exits that
        // recursive script loop.  Even when the callback hits a VM error, the
        // caller lexer/call stack must not be left inside the callback body;
        // otherwise the next frame continues at the failed callback PC and can
        // run into command padding / CD_NONE.
        let restore_callback_lexer = self.current_scene_no == saved_scene_no;
        if restore_callback_lexer {
            if std::env::var_os("SIGLUS_TRACE_FRAME_ACTION_CALL").is_some() {
                eprintln!(
                    "[SG_FRAME_ACTION_CALL] proc exit cmd={} scene={:?} completed={} proc_boundary={} wait_boundary={} error={} restoring caller lexer state",
                    cmd_name,
                    scn_name,
                    completed_by_return,
                    stopped_at_proc_boundary,
                    stopped_at_wait_boundary,
                    run_error.is_some()
                );
            }
            self.restore_interpreter_exec_state(saved_exec);
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
        if std::env::var_os("SIGLUS_TRACE_CALL_RETURN_PC").is_some() {
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
        caller.ret_form = ret_form;
        for arg in call_args {
            self.push_call_arg_value(arg);
        }
        self.call_stack.push(self.make_call_frame(
            self.cfg.fm_void,
            excall_proc,
            frame_action_proc,
            call_args.len(),
            None,
        ));
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

        let saved = SceneExecFrame {
            stream: self.stream.clone(),
            user_cmd_names: self.user_cmd_names.clone(),
            call_cmd_names: self.call_cmd_names.clone(),
            int_stack: std::mem::take(&mut self.int_stack),
            str_stack: std::mem::take(&mut self.str_stack),
            element_points: std::mem::take(&mut self.element_points),
            call_stack: std::mem::take(&mut self.call_stack),
            gosub_return_stack: std::mem::take(&mut self.gosub_return_stack),
            user_props: std::mem::take(&mut self.user_props),
            current_scene_no: self.current_scene_no,
            current_scene_name: self.current_scene_name.clone(),
            current_line_no: self.current_line_no,
            ret_form,
            excall_proc: ex_call_proc,
        };
        self.scene_stack.push(saved);

        self.stream = target_stream;
        self.user_cmd_names = self.stream.scn_cmd_name_map.clone();
        self.call_cmd_names = self
            .scene_pck_cache
            .as_ref()
            .expect("scene pck cache initialized")
            .inc_cmd_name_map
            .clone();
        self.current_scene_no = Some(target_scene_no);
        self.current_scene_name = self
            .scene_pck_cache
            .as_ref()
            .expect("scene pck cache initialized")
            .find_scene_name(target_scene_no)
            .map(ToOwned::to_owned);
        self.current_line_no = -1;
        self.ctx.current_scene_no = Some(target_scene_no as i64);
        self.ctx.current_scene_name = self.current_scene_name.clone();
        self.ctx.current_line_no = -1;

        for arg in call_args {
            self.push_call_arg_value(arg);
        }
        self.call_stack.push(self.make_call_frame(
            self.cfg.fm_void,
            ex_call_proc,
            frame_action_proc,
            call_args.len(),
            None,
        ));
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
        let current_scene_no = self.current_scene_no;
        self.ensure_scene_pck_cache()?;

        let target_scene_no = match scn_name {
            Some(name) if !name.is_empty() => self
                .scene_pck_cache
                .as_ref()
                .expect("scene pck cache initialized")
                .find_scene_no(name)
                .or(current_scene_no),
            _ => current_scene_no,
        };
        let Some(target_scene_no) = target_scene_no else {
            return Ok(false);
        };

        // C++ SET_BUTTON_CALL stores a scene-local user command name and resolves it
        // with Gp_lexer->get_user_cmd_no(scene_no, cmd_name). It must not prefer
        // global inc-command names here, because button callbacks are user commands
        // in the stored scene.
        if Some(target_scene_no) == self.current_scene_no {
            let Some(cmd_no) = self.user_cmd_names.iter().find_map(|(no, name)| {
                if name.eq_ignore_ascii_case(cmd_name) {
                    Some(*no as usize)
                } else {
                    None
                }
            }) else {
                if std::env::var_os("SG_DEBUG").is_some() {
                    eprintln!(
                        "[SG_DEBUG][BUTTON] user command not found for ex-call: scene={:?} cmd={}",
                        scn_name, cmd_name
                    );
                }
                return Ok(false);
            };
            let offset = self.stream.scn_cmd_offset(cmd_no)?;
            if std::env::var_os("SG_DEBUG").is_some() {
                eprintln!(
                    "[SG_DEBUG][BUTTON] enter local user command scene={:?} cmd={} cmd_no={} offset=0x{:x}",
                    scn_name,
                    cmd_name,
                    cmd_no,
                    offset
                );
            }
            return self.enter_current_scene_user_cmd_at_offset(offset, call_args);
        }

        let target_stream = self.cached_scene_stream(target_scene_no)?;
        let Some(cmd_no) = target_stream
            .scn_cmd_name_map
            .iter()
            .find_map(|(no, name)| {
                if name.eq_ignore_ascii_case(cmd_name) {
                    Some(*no as usize)
                } else {
                    None
                }
            })
        else {
            if std::env::var_os("SG_DEBUG").is_some() {
                eprintln!(
                    "[SG_DEBUG][BUTTON] target user command not found for ex-call: target_scene={} scn_name={:?} cmd={}",
                    target_scene_no,
                    scn_name,
                    cmd_name
                );
            }
            return Ok(false);
        };
        let offset = target_stream.scn_cmd_offset(cmd_no)?;
        if std::env::var_os("SG_DEBUG").is_some() {
            eprintln!(
                "[SG_DEBUG][BUTTON] enter target user command target_scene={} scn_name={:?} cmd={} cmd_no={} offset=0x{:x}",
                target_scene_no,
                scn_name,
                cmd_name,
                cmd_no,
                offset
            );
        }
        self.enter_scene_user_cmd_at_scene_offset(target_scene_no, offset, call_args)
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
        let saved_user_props = std::mem::take(&mut self.user_props);

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
        self.user_props = saved_user_props;
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
            let child_has_frame_action = !child.frame_action.cmd_name.is_empty()
                || child.frame_action_ch.iter().any(|ch| !ch.cmd_name.is_empty());
            let child_has_nested_work = !child.runtime.child_objects.is_empty();
            if child.used || child_has_frame_action || child_has_nested_work {
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

    fn object_from_frame_action_chain_mut<'b>(
        objects: &'b mut [crate::runtime::globals::ObjectState],
        object_chain: &[i32],
        elm_array: i32,
    ) -> Option<&'b mut crate::runtime::globals::ObjectState> {
        if object_chain.len() < 6 || object_chain[1] != elm_array || object_chain[4] != elm_array {
            return None;
        }
        let obj = objects.get_mut(object_chain[5].max(0) as usize)?;
        Self::object_child_from_chain_mut(obj, object_chain, 6, elm_array)
    }

    fn object_from_mwnd_frame_action_chain_mut<'b>(
        mwnds: &'b mut [crate::runtime::globals::MwndState],
        object_chain: &[i32],
        elm_array: i32,
    ) -> Option<&'b mut crate::runtime::globals::ObjectState> {
        if object_chain.len() < 9
            || object_chain[1] != elm_array
            || object_chain[4] != elm_array
            || object_chain[7] != elm_array
        {
            return None;
        }
        if object_chain[3] != crate::runtime::forms::codes::elm_value::STAGE_MWND {
            return None;
        }
        let mwnd_idx = object_chain[5].max(0) as usize;
        let selector = object_chain[6];
        let obj_idx = object_chain[8].max(0) as usize;
        let mwnd = mwnds.get_mut(mwnd_idx)?;
        if selector == crate::runtime::forms::codes::elm_value::MWND_BUTTON {
            let obj = mwnd.button_list.get_mut(obj_idx)?;
            return Self::object_child_from_chain_mut(obj, object_chain, 9, elm_array);
        }
        if selector == crate::runtime::forms::codes::elm_value::MWND_FACE {
            let obj = mwnd.face_list.get_mut(obj_idx)?;
            return Self::object_child_from_chain_mut(obj, object_chain, 9, elm_array);
        }
        if selector == crate::runtime::forms::codes::elm_value::MWND_OBJECT {
            let obj = mwnd.object_list.get_mut(obj_idx)?;
            return Self::object_child_from_chain_mut(obj, object_chain, 9, elm_array);
        }
        None
    }

    fn with_frame_action_mut<R>(
        &mut self,
        item: &FrameActionWork,
        f: impl FnOnce(&mut crate::runtime::globals::ObjectFrameActionState) -> R,
    ) -> Option<R> {
        if item.stage_idx < 0 {
            let form_id = item.global_form_id?;
            if let Some(idx) = item.ch_idx {
                let list = self.ctx.globals.frame_action_lists.get_mut(&form_id)?;
                return list.get_mut(idx).map(f);
            }
            return self.ctx.globals.frame_actions.get_mut(&form_id).map(f);
        }

        let chain = item.object_chain.as_ref()?;
        if chain.len() < 6 {
            return None;
        }
        let stage_idx = chain[2] as i64;
        let elm_array = self.ctx.ids.elm_array;
        let mut form_ids: Vec<u32> = self.ctx.globals.stage_forms.keys().copied().collect();
        form_ids.sort_unstable();
        for form_id in form_ids {
            let Some(st) = self.ctx.globals.stage_forms.get_mut(&form_id) else {
                continue;
            };
            let obj = if chain.get(3).copied()
                == Some(crate::runtime::forms::codes::elm_value::STAGE_MWND)
            {
                let Some(mwnds) = st.mwnd_lists.get_mut(&stage_idx) else {
                    continue;
                };
                let Some(obj) = Self::object_from_mwnd_frame_action_chain_mut(mwnds, chain, elm_array)
                else {
                    continue;
                };
                obj
            } else {
                let Some(objects) = st.object_lists.get_mut(&stage_idx) else {
                    continue;
                };
                let Some(obj) = Self::object_from_frame_action_chain_mut(objects, chain, elm_array)
                else {
                    continue;
                };
                obj
            };
            if let Some(idx) = item.ch_idx {
                return obj.frame_action_ch.get_mut(idx).map(f);
            }
            return Some(f(&mut obj.frame_action));
        }
        None
    }

    fn begin_frame_action_finish(
        &mut self,
        item: &FrameActionWork,
    ) -> Option<(String, Vec<Value>)> {
        self.with_frame_action_mut(item, |fa| {
            if fa.cmd_name.is_empty() || fa.end_time < 0 {
                return None;
            }
            if fa.counter.get_count() < fa.end_time {
                return None;
            }

            let cmd_name = fa.cmd_name.clone();
            let args = fa.args.clone();
            let final_count = if fa.end_time == -1 { 0 } else { fa.end_time };
            fa.counter.set_count(final_count);
            fa.scn_name.clear();
            fa.cmd_name.clear();
            fa.end_flag = true;
            Some((cmd_name, args))
        })?
    }

    fn end_frame_action_finish(&mut self, item: &FrameActionWork) {
        let _ = self.with_frame_action_mut(item, |fa| {
            // Original C_elm_frame_action::finish only drops the end-action flag
            // after the callback. It must not wipe a new frame action that may
            // have been started by that callback.
            fa.end_flag = false;
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
        stage_idx: i64,
        fallback_obj_idx: usize,
        chain: &[i32],
    ) -> usize {
        let stage_form = self.ctx.ids.form_global_stage;
        let elm_array = if self.ctx.ids.elm_array != 0 {
            self.ctx.ids.elm_array
        } else {
            crate::runtime::forms::codes::ELM_ARRAY
        };

        let Some(st) = self.ctx.globals.stage_forms.get_mut(&stage_form) else {
            return fallback_obj_idx;
        };
        let next_slot = st
            .next_nested_object_slot
            .entry(stage_idx)
            .or_insert(100000);

        if chain.get(3).copied() == Some(crate::runtime::forms::codes::elm_value::STAGE_MWND)
            && chain.len() >= 9
        {
            let mwnd_idx = chain.get(5).copied().unwrap_or(0).max(0) as usize;
            let selector = chain.get(6).copied().unwrap_or(0);
            let obj_idx = chain
                .get(8)
                .copied()
                .unwrap_or(fallback_obj_idx as i32)
                .max(0) as usize;
            let Some(mwnds) = st.mwnd_lists.get_mut(&stage_idx) else {
                return obj_idx;
            };
            let Some(mwnd) = mwnds.get_mut(mwnd_idx) else {
                return obj_idx;
            };
            if selector == crate::runtime::forms::codes::elm_value::MWND_BUTTON {
                let Some(obj) = mwnd.button_list.get_mut(obj_idx) else {
                    return obj_idx;
                };
                return Self::runtime_slot_from_object_children(
                    obj, obj_idx, chain, 9, elm_array, next_slot,
                );
            }
            if selector == crate::runtime::forms::codes::elm_value::MWND_FACE {
                let Some(obj) = mwnd.face_list.get_mut(obj_idx) else {
                    return obj_idx;
                };
                return Self::runtime_slot_from_object_children(
                    obj, obj_idx, chain, 9, elm_array, next_slot,
                );
            }
            if selector == crate::runtime::forms::codes::elm_value::MWND_OBJECT {
                let Some(obj) = mwnd.object_list.get_mut(obj_idx) else {
                    return obj_idx;
                };
                return Self::runtime_slot_from_object_children(
                    obj, obj_idx, chain, 9, elm_array, next_slot,
                );
            }
            return obj_idx;
        }

        if chain.get(3).copied() == Some(crate::runtime::forms::codes::STAGE_ELM_BTNSELITEM)
            && chain.len() >= 9
            && chain.get(6).copied() == Some(crate::runtime::forms::codes::ELM_BTNSELITEM_OBJECT)
        {
            let item_idx = chain.get(5).copied().unwrap_or(0).max(0) as usize;
            let obj_idx = chain
                .get(8)
                .copied()
                .unwrap_or(fallback_obj_idx as i32)
                .max(0) as usize;
            let Some(items) = st.btnselitem_lists.get_mut(&stage_idx) else {
                return obj_idx;
            };
            let Some(item) = items.get_mut(item_idx) else {
                return obj_idx;
            };
            let Some(obj) = item.object_list.get_mut(obj_idx) else {
                return obj_idx;
            };
            return Self::runtime_slot_from_object_children(
                obj, obj_idx, chain, 9, elm_array, next_slot,
            );
        }

        let top_idx = chain
            .get(5)
            .copied()
            .unwrap_or(fallback_obj_idx as i32)
            .max(0) as usize;
        let Some(list) = st.object_lists.get_mut(&stage_idx) else {
            return top_idx;
        };
        if top_idx >= list.len() {
            return top_idx;
        }
        let obj = &mut list[top_idx];
        Self::runtime_slot_from_object_children(obj, top_idx, chain, 6, elm_array, next_slot)
    }

    fn set_frame_action_current_object(
        &mut self,
        item: &FrameActionWork,
    ) -> (Option<(i64, usize)>, Option<Vec<i32>>) {
        let prev_target = self.ctx.globals.current_stage_object;
        let prev_chain = self.ctx.globals.current_object_chain.clone();
        if let Some(chain) = item.object_chain.clone() {
            let top_idx = chain.get(5).copied().unwrap_or(item.obj_idx as i32).max(0) as usize;
            let runtime_slot = self.runtime_slot_from_object_chain(item.stage_idx, top_idx, &chain);
            self.ctx.globals.current_stage_object = Some((item.stage_idx, runtime_slot));
            self.ctx.globals.current_object_chain = Some(chain);
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
        let (stage_idx, obj_idx) = object_chain
            .as_ref()
            .and_then(|chain| {
                if chain.len() >= 6 {
                    Some((chain[2] as i64, chain[5].max(0) as usize))
                } else {
                    None
                }
            })
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
        let _ = self.with_frame_action_mut(&item, |fa| {
            // C_elm_frame_action::finish clears the active command before invoking
            // the end action, sets the end-action flag, and leaves any new
            // frame-action state created by that callback intact.
            fa.scn_name.clear();
            fa.cmd_name.clear();
            fa.args = pending.args.clone();
            fa.end_time = pending.end_time;
            fa.counter.set_count(final_count);
            fa.end_flag = true;
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

        let _ = self.with_frame_action_mut(&item, |fa| {
            fa.end_flag = false;
        });
        if let Err(e) = result {
            self.ctx.unknown.record_note(&format!(
                "frame_action.finish.failed:{}:{}:{e}",
                pending.scn_name, pending.cmd_name
            ));
        }
        Ok(())
    }

    fn run_pending_button_action(&mut self, action: PendingButtonAction) -> Result<()> {
        match action.kind {
            PendingButtonActionKind::UserCall {
                scn_name,
                cmd_name,
                z_no,
            } => {
                if scn_name.is_empty() {
                    return Ok(());
                }
                if std::env::var_os("SG_DEBUG").is_some() {
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

    fn run_pending_button_syscom_action(
        &mut self,
        sys_type: i64,
        sys_type_opt: i64,
        mode: i64,
    ) -> Result<()> {
        use crate::runtime::forms::codes::syscom_op;
        use crate::runtime::forms::codes::FM_SYSCOM;
        let op = match sys_type {
            1 => syscom_op::CALL_SAVE_MENU,
            2 => syscom_op::CALL_LOAD_MENU,
            3 => syscom_op::SET_READ_SKIP_ONOFF_FLAG,
            4 => syscom_op::SET_AUTO_MODE_ONOFF_FLAG,
            5 => syscom_op::RETURN_TO_SEL,
            6 => syscom_op::SET_HIDE_MWND_ONOFF_FLAG,
            7 => syscom_op::OPEN_MSG_BACK,
            8 => syscom_op::REPLAY_KOE,
            9 => syscom_op::QUICK_SAVE,
            10 => syscom_op::QUICK_LOAD,
            11 => syscom_op::CALL_CONFIG_MENU,
            12 => syscom_op::SET_LOCAL_EXTRA_SWITCH_ONOFF_FLAG,
            13 => syscom_op::SET_LOCAL_EXTRA_MODE_VALUE,
            14 => syscom_op::SET_GLOBAL_EXTRA_SWITCH_ONOFF,
            15 => syscom_op::SET_GLOBAL_EXTRA_MODE_VALUE,
            _ => return Ok(()),
        };
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
        let mut args = vec![Value::Element(vec![FM_SYSCOM, op])];
        args.extend(params);
        runtime::dispatch_form_code(&mut self.ctx, FM_SYSCOM as u32, &args)?;
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
        Ok(())
    }

    pub fn tick_frame(&mut self) -> Result<()> {
        let trace = std::env::var_os("SG_TICK_TRACE").is_some()
            || std::env::var_os("SG_FRAME_ACTION_TRACE").is_some();
        if trace {
            eprintln!(
                "[SG_TICK_TRACE] tick_frame start blocked={} halted={} scene={:?}",
                self.is_blocked(),
                self.halted,
                self.current_scene_name()
            );
        }
        self.drain_pending_button_actions()?;
        if self.ctx.globals.script.frame_action_time_stop_flag && trace {
            eprintln!(
                "[SG_TICK_TRACE] frame_action_time_stop_flag set; executing callbacks with frozen frame-action time"
            );
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
                    if st.is_embedded_object_slot(stage_idx, obj_idx) {
                        continue;
                    }
                    let object_chain = vec![
                        self.ctx.ids.form_global_stage as i32,
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
                            self.ctx.ids.form_global_stage as i32,
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
                            self.ctx.ids.form_global_stage as i32,
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
                            self.ctx.ids.form_global_stage as i32,
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

            if let Some((finish_cmd_name, finish_args)) = self.begin_frame_action_finish(&item) {
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
                    Some(&item.scn_name),
                    &finish_cmd_name,
                    &finish_call_args,
                    self.cfg.fm_void,
                    true,
                ) {
                    self.ctx.unknown.record_note(&format!(
                        "frame_action.finish_call.failed:{}:{}:{e}",
                        item.scn_name, finish_cmd_name
                    ));
                }
                self.restore_frame_action_current_object(prev_target, prev_chain);
                self.end_frame_action_finish(&item);
            }
        }
        self.ctx.tick_frame();
        if trace {
            eprintln!(
                "[SG_TICK_TRACE] after ctx.tick_frame blocked={} halted={}",
                self.is_blocked(),
                self.halted
            );
        }
        self.script_input_synced_this_frame = false;
        Ok(())
    }

    pub fn restart_scene_name(&mut self, scene_name: &str, z_no: i32) -> Result<()> {
        let (stream, scene_no) = self.load_scene_stream(scene_name, z_no)?;
        self.ctx.reset_for_scene_restart();
        self.stream = stream;
        self.int_stack.clear();
        self.str_stack.clear();
        self.element_points.clear();
        self.call_stack.clear();
        self.call_stack.push(self.scene_base_call());
        self.gosub_return_stack.clear();
        self.user_props.clear();
        self.scene_stack.clear();
        self.save_point = None;
        self.sel_point_stack.clear();
        self.current_scene_no = Some(scene_no);
        self.current_scene_name = Some(scene_name.to_string());
        self.current_line_no = -1;
        self.ctx.current_scene_no = Some(scene_no as i64);
        self.ctx.current_scene_name = Some(scene_name.to_string());
        self.ctx.current_line_no = -1;
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
        self.current_scene_no = point.current_scene_no;
        self.current_scene_name = point.current_scene_name;
        self.current_line_no = point.current_line_no;
        let mut restored_globals = point.globals;
        restored_globals.syscom.pending_proc = None;
        restored_globals.syscom.menu_open = false;
        restored_globals.syscom.menu_kind = None;
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
        self.step_inner(true)
    }

    /// Reset the infinite-loop guard for one C++-style frame_main_proc pass.
    /// This is not a scheduling quota; it only preserves SIGLUS_VM_MAX_STEPS
    /// as a hard error if a script never reaches a proc/wait boundary.
    pub fn begin_script_proc_pump(&mut self) {
        self.steps = 0;
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
        if self.is_blocked() {
            return Ok(true);
        }

        if !self.script_input_synced_this_frame {
            self.ctx.sync_script_input_from_runtime();
            self.ctx.input.next_frame();
            self.script_input_synced_this_frame = true;
        }

        loop {
            let proc_generation_before = self.ctx.proc_generation();
            let running = self.step_inner(true)?;
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

        // Normal scene execution is blocked by WAIT / WAIT_KEY.
        // Frame-action inline callbacks bypass this outer wait guard so the
        // frame phase can run object callbacks while the main script is waiting.
        if respect_wait {
            let blocked = self.ctx.wait_poll();
            if blocked {
                return Ok(true);
            }
        }

        // If the previous command yielded a delayed return (movie wait-key), materialize it now.
        if let Some(rf) = self.delayed_ret_form.take() {
            self.take_ctx_return(rf)?;
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
                if self.return_from_scene(Vec::new())? {
                    return Ok(true);
                }
                self.halted = true;
                return Ok(false);
            }
        };

        self.vm_trace_opcode(pc_before, opcode, "before");

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
                self.vm_trace(
                    None,
                    format!("ELM_POINT push start={} ", self.int_stack.len()),
                );
            }
            CD_COPY_ELM => {
                self.exec_copy_element()?;
            }

            CD_PROPERTY => {
                let elm = self.pop_element()?;
                self.vm_trace(None, format!("CD_PROPERTY elm={:?}", elm));
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
                    prop_id,
                    form: form_code,
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

            CD_GOTO => {
                let label_no = self.stream.pop_i32()?;
                self.sg_omv_trace(format!("GOTO label={} taken=true", label_no));
                self.stream.jump_to_label(label_no.max(0) as usize)?;
            }
            CD_GOTO_TRUE => {
                let label_no = self.stream.pop_i32()?;
                let cond = self.pop_int()?;
                let taken = cond != 0;
                self.sg_omv_trace(format!("GOTO_TRUE label={} cond={} taken={}", label_no, cond, taken));
                if taken {
                    self.stream.jump_to_label(label_no.max(0) as usize)?;
                }
            }
            CD_GOTO_FALSE => {
                let label_no = self.stream.pop_i32()?;
                let cond = self.pop_int()?;
                let taken = cond == 0;
                self.sg_omv_trace(format!("GOTO_FALSE label={} cond={} taken={}", label_no, cond, taken));
                if taken {
                    self.stream.jump_to_label(label_no.max(0) as usize)?;
                }
            }
            CD_GOSUB => {
                let label_no = self.stream.pop_i32()?;
                let _args = self.pop_arg_list()?;
                let return_pc = self.stream.get_prg_cntr();
                self.vm_trace(
                    Some(pc_before),
                    format!("GOSUB label={} return_pc=0x{return_pc:x}", label_no),
                );

                // Save return info on the caller frame .
                let caller = self
                    .call_stack
                    .last_mut()
                    .ok_or_else(|| anyhow!("call stack underflow"))?;
                caller.return_pc = return_pc;
                caller.ret_form = self.cfg.fm_int;
                self.gosub_return_stack.push((return_pc, self.cfg.fm_int));

                // Enter callee context.
                let scratch_args = self.call_scratch_from_args(&_args);
                let mut callee = self.make_call_frame(
                    self.cfg.fm_void,
                    false,
                    false,
                    _args.len(),
                    Some(scratch_args),
                );
                callee.return_override = Some((return_pc, self.cfg.fm_int));
                self.call_stack.push(callee);

                self.stream.jump_to_label(label_no.max(0) as usize)?;
            }
            CD_GOSUBSTR => {
                let label_no = self.stream.pop_i32()?;
                let _args = self.pop_arg_list()?;
                let return_pc = self.stream.get_prg_cntr();
                self.vm_trace(
                    Some(pc_before),
                    format!("GOSUBSTR label={} return_pc=0x{return_pc:x}", label_no),
                );

                let caller = self
                    .call_stack
                    .last_mut()
                    .ok_or_else(|| anyhow!("call stack underflow"))?;
                caller.return_pc = return_pc;
                caller.ret_form = self.cfg.fm_str;
                self.gosub_return_stack.push((return_pc, self.cfg.fm_str));

                let scratch_args = self.call_scratch_from_args(&_args);
                let mut callee = self.make_call_frame(
                    self.cfg.fm_void,
                    false,
                    false,
                    _args.len(),
                    Some(scratch_args),
                );
                callee.return_override = Some((return_pc, self.cfg.fm_str));
                self.call_stack.push(callee);

                self.stream.jump_to_label(label_no.max(0) as usize)?;
            }
            CD_RETURN => {
                let args = self.pop_arg_list()?;
                self.sg_omv_trace(format!("RETURN argc={} args={:?} call_depth={} scene_stack={}", args.len(), args, self.call_stack.len(), self.scene_stack.len()));
                if self.call_stack.len() == 1 {
                    if self.return_from_scene(args.clone())? {
                        return Ok(true);
                    }
                    self.halted = true;
                    return Ok(false);
                }
                if self.exec_return(args)? {
                    return Ok(false);
                }
            }

            CD_ASSIGN => {
                let _left_form = self.stream.pop_i32()?;
                let right_form = self.stream.pop_i32()?;
                let al_id = self.stream.pop_i32()?;
                let rhs = self.pop_value_for_form(right_form)?;
                let elm = self.pop_element()?;
                self.exec_assign(elm, al_id, rhs)?;
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
                if self.ctx.take_read_flag_no_request() {
                    let _read_flag_no = self.stream.pop_i32()?;
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
                if self.return_from_scene(Vec::new())? {
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
                eprintln!(
                    "VM hit CD_NONE scene={} line={} pc=0x{:x} {} bytes={:02x?}; stopping",
                    self.current_scene_name.as_deref().unwrap_or("<none>"),
                    self.current_line_no,
                    pc_before,
                    scn_cmd_context,
                    &self.stream.scn[pc_before.saturating_sub(8)..self.stream.scn.len().min(pc_before + 16)]
                );
                self.halted = true;
                return Ok(false);
            }

            other => {
                *self.unknown_opcodes.entry(other).or_insert(0) += 1;
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
        self.vm_trace(None, format!("push_int {}", v));
    }

    fn pop_int(&mut self) -> Result<i32> {
        match self.int_stack.pop() {
            Some(v) => {
                self.vm_trace(None, format!("pop_int -> {}", v));
                Ok(v)
            }
            None => {
                self.vm_trace(None, "pop_int underflow");
                Err(anyhow!("int stack underflow"))
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
        let preview = if s.chars().count() > 48 {
            let mut tmp = s.chars().take(48).collect::<String>();
            tmp.push('…');
            tmp
        } else {
            s.clone()
        };
        self.str_stack.push(s);
        self.vm_trace(None, format!("push_str {:?}", preview));
    }

    fn pop_str(&mut self) -> Result<String> {
        match self.str_stack.pop() {
            Some(v) => {
                let preview = if v.chars().count() > 48 {
                    let mut tmp = v.chars().take(48).collect::<String>();
                    tmp.push('…');
                    tmp
                } else {
                    v.clone()
                };
                self.vm_trace(None, format!("pop_str -> {:?}", preview));
                Ok(v)
            }
            None => {
                self.vm_trace(None, "pop_str underflow");
                Err(anyhow!("str stack underflow"))
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
        self.vm_trace(None, format!("push_element {:?}", elm));
    }

    fn pop_element(&mut self) -> Result<Vec<i32>> {
        let start = match self.element_points.pop() {
            Some(v) => v,
            None => {
                self.vm_trace(None, "pop_element underflow (missing ELM_POINT)");
                return Err(anyhow!("element stack underflow (missing ELM_POINT)"));
            }
        };
        if start > self.int_stack.len() {
            self.vm_trace(
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
        self.vm_trace(None, format!("pop_element -> {:?}", elm));
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

    fn consume_array_sub<'b>(&self, sub: &'b [i32]) -> Option<(usize, &'b [i32])> {
        if sub.len() >= 2 && self.call_array_marker(sub[0]) && sub[1] >= 0 {
            Some((sub[1] as usize, &sub[2..]))
        } else {
            None
        }
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
        if let Some((idx, rest)) = self.consume_array_sub(sub) {
            if !rest.is_empty() {
                bail!("unsupported chained intlist array access {:?}", sub);
            }
            self.push_int(values.get(idx).copied().unwrap_or(0));
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
                if let Some((idx, rest)) = self.consume_array_sub(&sub[1..]) {
                    if !rest.is_empty() {
                        bail!("unsupported chained intlist bit access {:?}", sub);
                    }
                    self.push_int(Self::intlist_bit_get(values, bit, idx));
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
            } else if let Some((idx, rest)) = self.consume_array_sub(sub) {
                let cur = cell.str_list.get(idx).cloned().unwrap_or_default();
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
        let mut int_args = Self::blank_call_int_args();
        let mut str_args = Self::blank_call_str_args();
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
                prop_id: idx,
                form: self.cfg.fm_list,
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

    fn push_call_prop_result(
        &mut self,
        prop: &CallProp,
        sub: &[i32],
        full_elm: &[i32],
    ) -> Result<()> {
        use crate::runtime::forms::codes::{
            ELM_ARRAY, ELM_STRLIST_GET_SIZE, FM_INT, FM_INTLIST, FM_INTLISTREF, FM_INTREF, FM_STR,
            FM_STRLIST, FM_STRLISTREF, FM_STRREF,
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
                    } else if let Some((idx, rest)) = self.consume_array_sub(sub) {
                        let current = v.get(idx).cloned().unwrap_or_default();
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
            FM_INTREF | FM_STRREF | FM_INTLISTREF | FM_STRLISTREF => {
                self.push_element(prop.element.clone());
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

    fn str_display_width_char(ch: char) -> usize {
        if ch.is_ascii() {
            1
        } else {
            2
        }
    }

    fn str_display_width(s: &str) -> usize {
        s.chars().map(Self::str_display_width_char).sum()
    }

    fn str_left_len(s: &str, limit: usize) -> String {
        let mut width = 0usize;
        let mut out = String::new();
        for ch in s.chars() {
            let w = Self::str_display_width_char(ch);
            if width + w > limit {
                break;
            }
            width += w;
            out.push(ch);
        }
        out
    }

    fn str_right_len(s: &str, limit: usize) -> String {
        let mut width = 0usize;
        let mut out: Vec<char> = Vec::new();
        for ch in s.chars().rev() {
            let w = Self::str_display_width_char(ch);
            if width + w > limit {
                break;
            }
            width += w;
            out.push(ch);
        }
        out.into_iter().rev().collect()
    }

    fn str_mid_len(s: &str, start_width: usize, len_width: Option<usize>) -> String {
        let mut width = 0usize;
        let mut out = String::new();
        for ch in s.chars() {
            let ch_width = Self::str_display_width_char(ch);
            if width >= start_width {
                if let Some(limit) = len_width {
                    if Self::str_display_width(&out) + ch_width > limit {
                        break;
                    }
                }
                out.push(ch);
            }
            width += ch_width;
        }
        out
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
            str_op::UPPER => {
                self.push_str(current.chars().map(|c| c.to_ascii_uppercase()).collect())
            }
            str_op::LOWER => {
                self.push_str(current.chars().map(|c| c.to_ascii_lowercase()).collect())
            }
            str_op::CNT => self.push_int(current.chars().count() as i32),
            str_op::LEN => self.push_int(Self::str_display_width(current) as i32),
            str_op::LEFT => {
                let len = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                self.push_str(current.chars().take(len).collect());
            }
            str_op::LEFT_LEN => {
                let len = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                self.push_str(Self::str_left_len(current, len));
            }
            str_op::RIGHT => {
                let len = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                let total = current.chars().count();
                let start = total.saturating_sub(len);
                self.push_str(current.chars().skip(start).collect());
            }
            str_op::RIGHT_LEN => {
                let len = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                self.push_str(Self::str_right_len(current, len));
            }
            str_op::MID => {
                let start = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                if al_id == 0 || params.len() <= 1 {
                    self.push_str(current.chars().skip(start).collect());
                } else {
                    let len = params.get(1).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                    self.push_str(current.chars().skip(start).take(len).collect());
                }
            }
            str_op::MID_LEN => {
                let start = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                let len = if al_id == 0 || params.len() <= 1 {
                    None
                } else {
                    Some(params.get(1).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize)
                };
                self.push_str(Self::str_mid_len(current, start, len));
            }
            str_op::SEARCH => {
                let needle = params.first().and_then(|v| v.as_str()).unwrap_or("");
                let hay = current.to_ascii_lowercase();
                let needle = needle.to_ascii_lowercase();
                self.push_int(hay.find(&needle).map(|v| v as i32).unwrap_or(-1));
            }
            str_op::SEARCH_LAST => {
                let needle = params.first().and_then(|v| v.as_str()).unwrap_or("");
                let hay = current.to_ascii_lowercase();
                let needle = needle.to_ascii_lowercase();
                self.push_int(hay.rfind(&needle).map(|v| v as i32).unwrap_or(-1));
            }
            str_op::GET_CODE => {
                let pos = params.first().and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                let code = current.chars().nth(pos).map(|c| c as i32).unwrap_or(-1);
                self.push_int(code);
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

        let (form, mut value, mut element) = {
            let prop = self
                .call_stack
                .get(frame_idx)
                .and_then(|f| f.user_props.get(prop_idx))
                .ok_or_else(|| anyhow!("call prop frame/index out of range"))?;
            (prop.form, prop.value.clone(), prop.element.clone())
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
        self.vm_trace(None, format!("exec_call_property elm={:?}", elm));
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
                    let idx = sub[1].max(0) as usize;
                    let v = self.call_stack[current_idx]
                        .int_args
                        .get(idx)
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
                    let idx = sub[1].max(0) as usize;
                    let v = self.call_stack[current_idx]
                        .str_args
                        .get(idx)
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
                    let idx = sub[1].max(0) as usize;
                    if let Value::Int(n) = rhs {
                        let frame = &mut self.call_stack[current_idx];
                        if frame.int_args.len() <= idx {
                            frame.int_args.resize(idx + 1, 0);
                        }
                        frame.int_args[idx] = n as i32;
                    }
                }
                return Ok(true);
            }
            ELM_CALL_K => {
                let sub = &tail[1..];
                if sub.len() >= 2 && self.call_array_marker(sub[0]) {
                    let idx = sub[1].max(0) as usize;
                    if let Value::Str(s) = rhs {
                        let frame = &mut self.call_stack[current_idx];
                        if frame.str_args.len() <= idx {
                            frame.str_args.resize_with(idx + 1, String::new);
                        }
                        frame.str_args[idx] = s;
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
            ELM_ARRAY, ELM_CALL_K, ELM_CALL_L, ELM_GLOBAL_CUR_CALL, ELM_INTLIST_CLEAR, ELM_INTLIST_GET_SIZE,
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
                    let idx = sub[1].max(0) as usize;
                    if al_id == 1 {
                        let rhs = args.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        let frame = &mut self.call_stack[current_idx];
                        if frame.int_args.len() <= idx {
                            frame.int_args.resize(idx + 1, 0);
                        }
                        frame.int_args[idx] = rhs;
                        self.push_default_for_ret(ret_form);
                    } else {
                        let v = self.call_stack[current_idx]
                            .int_args
                            .get(idx)
                            .copied()
                            .unwrap_or(0);
                        self.push_int(v);
                    }
                    return Ok(true);
                }
                match sub[0] {
                    ELM_INTLIST_INIT => {
                        self.call_stack[current_idx].int_args = Self::blank_call_int_args();
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
                        let start =
                            args.get(0).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                        let end =
                            args.get(1).and_then(|v| v.as_i64()).unwrap_or(-1).max(-1) as isize;
                        let value = if al_id == 0 {
                            0
                        } else {
                            args.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32
                        };
                        let frame = &mut self.call_stack[current_idx];
                        if !frame.int_args.is_empty() && end >= 0 {
                            let end =
                                usize::min(end as usize, frame.int_args.len().saturating_sub(1));
                            for i in start..=end {
                                if i < frame.int_args.len() {
                                    frame.int_args[i] = value;
                                }
                            }
                        }
                    }
                    ELM_INTLIST_SETS => {
                        let start =
                            args.get(0).and_then(|v| v.as_i64()).unwrap_or(0).max(0) as usize;
                        let frame = &mut self.call_stack[current_idx];
                        for (off, v) in args.iter().skip(1).enumerate() {
                            let idx = start + off;
                            if frame.int_args.len() <= idx {
                                frame.int_args.resize(idx + 1, 0);
                            }
                            frame.int_args[idx] = v.as_i64().unwrap_or(0) as i32;
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
                    let idx = sub[1].max(0) as usize;
                    if sub.len() == 2 {
                        if al_id == 1 {
                            let rhs = args
                                .first()
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let frame = &mut self.call_stack[current_idx];
                            if frame.str_args.len() <= idx {
                                frame.str_args.resize_with(idx + 1, String::new);
                            }
                            frame.str_args[idx] = rhs;
                            self.push_default_for_ret(ret_form);
                        } else {
                            let v = self.call_stack[current_idx]
                                .str_args
                                .get(idx)
                                .cloned()
                                .unwrap_or_default();
                            self.push_str(v);
                        }
                    } else {
                        let v = self.call_stack[current_idx]
                            .str_args
                            .get(idx)
                            .cloned()
                            .unwrap_or_default();
                        self.call_prop_eval_str_op(&v, sub[2], args, al_id)?;
                    }
                    return Ok(true);
                }
                match sub[0] {
                    ELM_STRLIST_INIT => {
                        self.call_stack[current_idx].str_args = Self::blank_call_str_args();
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
                if entry.int_list.len() <= i {
                    entry.int_list.resize(i + 1, 0);
                }
                entry.int_list[i] = rhs.as_i64().unwrap_or(0) as i32;
                return;
            }
            if existing_form == self.cfg.fm_strlist {
                let entry = self.user_props.entry(prop_id).or_insert(default_entry);
                if entry.str_list.len() <= i {
                    entry.str_list.resize_with(i + 1, String::new);
                }
                entry.str_list[i] = rhs.as_str().unwrap_or("").to_string();
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
                self.vm_trace(None, "COPY_ELM missing prior ELM_POINT");
                return Err(anyhow!("COPY_ELM without a prior ELM_POINT"));
            }
        };
        if start > self.int_stack.len() {
            self.vm_trace(
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
        if Self::sg_mwnd_object_trace_enabled() && Self::sg_mwnd_chain_interesting(&slice) {
            self.sg_mwnd_object_trace(format!(
                "COPY_ELM slice={:?} before_current_chain={:?} before_current_stage_object={:?}",
                slice,
                self.ctx.globals.current_object_chain,
                self.ctx.globals.current_stage_object
            ));
        }
        self.update_compact_context_from_element(&slice);
        if Self::sg_mwnd_object_trace_enabled() && Self::sg_mwnd_chain_interesting(&slice) {
            self.sg_mwnd_object_trace(format!(
                "COPY_ELM after_current_chain={:?} after_current_stage_object={:?}",
                self.ctx.globals.current_object_chain,
                self.ctx.globals.current_stage_object
            ));
        }
        self.element_points.push(self.int_stack.len());
        self.int_stack.extend_from_slice(&slice);
        self.vm_trace(None, format!("COPY_ELM copied {:?}", slice));
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

        if constants::is_stage_global_form(form_id, ids.form_global_stage) {
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


    fn sg_mwnd_object_trace_enabled() -> bool {
        std::env::var_os("SG_DEBUG").is_some()
    }

    fn sg_mwnd_object_trace(&self, msg: impl AsRef<str>) {
        if Self::sg_mwnd_object_trace_enabled() {
            eprintln!("[SG_DEBUG][MWND_OBJECT_TRACE][VM] {}", msg.as_ref());
        }
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
        let stage_form = if self.ctx.ids.form_global_stage != 0 {
            self.ctx.ids.form_global_stage as i32
        } else {
            crate::runtime::forms::codes::FORM_GLOBAL_STAGE as i32
        };
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
            || chain[0] != stage_form
            || chain[1] != elm_array
            || chain[2] < 0
        {
            return false;
        }

        let stage_idx = chain[2] as i64;
        let Some(stage_state) = self.ctx.globals.stage_forms.get(&(stage_form as u32)) else {
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
        self.is_global_indexed_list_chain(elm) && !self.is_current_object_child_tail(elm)
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
        _allow_ambiguous_single_token_object_op: bool,
    ) -> bool {
        elm.first()
            .copied()
            .map(|op| self.compact_object_op_allowed(op))
            .unwrap_or(false)
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
        let stage_form = self.ctx.ids.form_global_stage;
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

        let elm_array = self.ctx.ids.elm_array;
        let stage_object = if self.ctx.ids.stage_elm_object != 0 {
            self.ctx.ids.stage_elm_object
        } else {
            crate::runtime::forms::codes::STAGE_ELM_OBJECT
        };

        let looks_like_full_stage_object =
            elm.len() >= 4 && elm[1] == elm_array && elm[3] == stage_object;
        let looks_like_absolute_stage_alias_object = elm.len() >= 4
            && constants::is_stage_global_form(elm[0] as u32, self.ctx.ids.form_global_stage)
            && elm[1] == stage_object
            && elm[2] == elm_array;
        let looks_like_current_object_child = elm.len() >= 2
            && elm[1] == elm_array
            && elm[0] >= 0
            && (elm.len() == 2
                || self.compact_object_op_allowed(elm[2])
                || elm[2] == elm_array
                || elm[2] == crate::runtime::forms::codes::ELM_ARRAY);

        if looks_like_absolute_stage_alias_object {
            return None;
        }

        if !looks_like_full_stage_object {
            if let Some(prefix) = &self.ctx.globals.current_object_chain {
                if looks_like_current_object_child && self.is_current_object_child_tail(elm) {
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
                    if Self::sg_mwnd_object_trace_enabled()
                        && (Self::sg_mwnd_chain_interesting(elm) || Self::sg_mwnd_chain_interesting(&synthetic))
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
                let mut synthetic = prefix.clone();
                synthetic.extend_from_slice(elm);
                if Self::sg_mwnd_object_trace_enabled()
                    && (Self::sg_mwnd_chain_interesting(elm) || Self::sg_mwnd_chain_interesting(&synthetic))
                {
                    eprintln!(
                        "[SG_DEBUG][MWND_OBJECT_TRACE][VM] try_compact prefix-append elm={:?} prefix={:?} synthetic={:?}",
                        elm,
                        prefix,
                        synthetic
                    );
                }
                return Some(synthetic);
            }
        }

        if elm.len() < 3 || elm[2] != elm_array || elm[1] < 0 {
            return None;
        }

        let stage_idx = elm[1];
        if !(0..3).contains(&stage_idx) {
            return None;
        }
        let stage_form = if self.ctx.ids.form_global_stage != 0 {
            self.ctx.ids.form_global_stage as i32
        } else {
            crate::runtime::forms::codes::FORM_GLOBAL_STAGE as i32
        };

        if elm.len() >= 4 {
            let obj_idx = elm[3];
            if obj_idx < 0 {
                return None;
            }
            let mut synthetic = vec![
                stage_form,
                elm_array,
                stage_idx,
                stage_object,
                elm_array,
                obj_idx,
                op,
            ];
            if elm.len() > 4 {
                synthetic.extend_from_slice(&elm[4..]);
            }
            if Self::sg_mwnd_object_trace_enabled()
                && (Self::sg_mwnd_chain_interesting(elm) || Self::sg_mwnd_chain_interesting(&synthetic))
            {
                eprintln!(
                    "[SG_DEBUG][MWND_OBJECT_TRACE][VM] try_compact absolute elm={:?} synthetic={:?}",
                    elm,
                    synthetic
                );
            }
            return Some(synthetic);
        }

        if let Some(prefix) = &self.ctx.globals.current_object_chain {
            if prefix.len() >= 3 && prefix[2] == stage_idx {
                let mut synthetic = prefix.clone();
                synthetic.push(op);
                if Self::sg_mwnd_object_trace_enabled()
                    && (Self::sg_mwnd_chain_interesting(elm) || Self::sg_mwnd_chain_interesting(&synthetic))
                {
                    eprintln!(
                        "[SG_DEBUG][MWND_OBJECT_TRACE][VM] try_compact stage-op elm={:?} prefix={:?} synthetic={:?}",
                        elm,
                        prefix,
                        synthetic
                    );
                }
                return Some(synthetic);
            }
        }

        if let Some((current_stage, current_obj)) = self.ctx.globals.current_stage_object {
            if current_stage != stage_idx as i64 {
                return None;
            }
            return Some(vec![
                stage_form,
                elm_array,
                stage_idx,
                stage_object,
                elm_array,
                current_obj as i32,
                op,
            ]);
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
        if std::env::var_os("SG_TITLE_CHAIN_TRACE").is_some()
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
        self.vm_trace(None, format!("exec_property enter elm={:?}", elm));
        if elm.is_empty() {
            self.push_int(0);
            return Ok(());
        }
        // Call-local properties (declared by CD_DEC_PROP / populated by CD_ARG).
        if self.exec_call_property(&elm)? {
            self.vm_trace(
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
                self.vm_trace(
                    None,
                    format!("exec_property direct CALL_PROP composed elm={:?}", elm),
                );
                return Ok(());
            }
            self.push_call_prop_result(&prop, &elm[1..], &elm)?;
            self.vm_trace(
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
            self.push_user_prop_cell_result(&cell, &elm[1..], &elm)?;
            self.vm_trace(
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
            self.vm_trace(None, format!("exec_property handled by global indexed-list elm={:?}", elm));
            return Ok(());
        }

        if self.try_parent_slot_property(&elm) {
            self.vm_trace(
                None,
                format!("exec_property handled by parent-slot elm={:?}", elm),
            );
            return Ok(());
        }
        if let Some(synthetic) = self.try_compact_object_chain(&elm, false) {
            self.vm_trace(
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

        self.vm_trace(
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
            let array_idx = self.extract_array_index(&elm);
            self.assign_user_prop(prop_id, array_idx, rhs);
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
            self.vm_trace(
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
        self.vm_trace(
            None,
            format!(
                "exec_assign dispatch form_id={} elm={:?} al_id={} rhs={:?}",
                form_id, elm, al_id, rhs
            ),
        );
        let args: Vec<Value> = vec![rhs];
        if (std::env::var_os("SIGLUS_TRACE_VM_COMMANDS").is_some()) {
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

    fn dispatch_owner_named_command(
        &mut self,
        owner: u8,
        raw_head: i32,
        ret_form: i32,
        args: &[Value],
    ) -> Result<bool> {
        let cmd_no = elm_code::code(raw_head) as u32;
        if owner == elm_code::ELM_OWNER_USER_CMD {
            // C++ tnm_command_proc_user_cmd() does not execute the user command
            // synchronously. It sets the caller ret_form, saves the call frame,
            // jumps the lexer to the user command, then pushes call arguments onto
            // the VM stack. The return value is pushed later by
            // tnm_command_proc_return(), immediately before the caller resumes.
            //
            // The previous Rust inline path restored the caller PC and tried to
            // synthesize a return value inside CD_COMMAND. That is not equivalent
            // for user commands that wait, switch proc, or otherwise return later,
            // and it causes the following instruction to pop from an empty int
            // stack. Enter the user command as an actual VM call instead.
            let inc_cmd_cnt = self.call_cmd_names.len() as u32;
            let local_cmd_no = if cmd_no < inc_cmd_cnt {
                let Some(name) = self.call_cmd_names.get(&cmd_no) else {
                    return Ok(false);
                };
                match self.user_cmd_names.iter().find_map(|(no, local_name)| {
                    if local_name.eq_ignore_ascii_case(name) {
                        Some(*no)
                    } else {
                        None
                    }
                }) {
                    Some(no) => no,
                    None => return Ok(false),
                }
            } else {
                cmd_no - inc_cmd_cnt
            };

            let Some(name) = self.user_cmd_names.get(&local_cmd_no).cloned() else {
                self.sg_omv_trace(format!(
                    "USER_CMD unresolved raw_head={} cmd_no={} local_cmd_no={} inc_cmd_cnt={} ret_form={} argc={}",
                    raw_head,
                    cmd_no,
                    local_cmd_no,
                    inc_cmd_cnt,
                    ret_form,
                    args.len()
                ));
                return Ok(false);
            };
            let offset = self.stream.scn_cmd_offset(local_cmd_no as usize)?;
            self.sg_omv_trace(format!(
                "USER_CMD enter name={} raw_head={} cmd_no={} local_cmd_no={} offset=0x{:x} ret_form={} argc={} current_pc=0x{:x}",
                name,
                raw_head,
                cmd_no,
                local_cmd_no,
                offset,
                ret_form,
                args.len(),
                self.stream.get_prg_cntr()
            ));
            return self.enter_current_scene_user_cmd_proc_at_offset(
                offset,
                ret_form,
                args,
                false,
                false,
            );
        }

        let name = match owner {
            o if o == elm_code::ELM_OWNER_CALL_CMD => self.call_cmd_names.get(&cmd_no).cloned(),
            _ => None,
        };
        let Some(name) = name else {
            return Ok(false);
        };
        runtime::dispatch_named_command(&mut self.ctx, &name, args)
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

        match owner {
            o if o == elm_code::ELM_OWNER_FORM => {
                // Suppress only the exact residual bare [GLOBAL.WIPE] command shape
                // observed at sys20_adv01 loop-increment sites. Real WIPE calls with
                // arguments still go through global.rs.
                if elm.len() == 1
                    && elm[0] == crate::runtime::forms::codes::elm_value::GLOBAL_WIPE
                    && args.is_empty()
                    && ret_form == self.cfg.fm_void
                {
                    self.vm_trace(None, "suppress bare residual GLOBAL.WIPE command".to_string());
                    return Ok(());
                }

                if self.dispatch_global_indexed_list_command_direct(&elm, al_id, ret_form, args)? {
                    return Ok(());
                }
                if let Some(synthetic) = self.try_compact_object_chain(&elm, true) {
                    self.vm_trace(
                        None,
                        format!(
                            "exec_command compact-object elm={:?} synthetic={:?} al_id={} ret_form={} args={:?}",
                            elm, synthetic, al_id, ret_form, args
                        ),
                    );
                    if Self::sg_mwnd_object_trace_enabled()
                        && (Self::sg_mwnd_chain_interesting(&elm) || Self::sg_mwnd_chain_interesting(&synthetic))
                    {
                        self.sg_mwnd_object_trace(format!(
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
                        if self.ctx.wait_poll() {
                            self.delayed_ret_form = Some(ret_form);
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
                if self.exec_builtin_scene_form(form_id, al_id, ret_form, args)? {
                    return Ok(());
                }

                let op_id = if elm.len() >= 2 { elm[1] } else { al_id };
                self.ctx.vm_call = Some(runtime::VmCallMeta {
                    element: elm.clone(),
                    al_id: al_id as i64,
                    ret_form: ret_form as i64,
                });

                if (std::env::var_os("SIGLUS_TRACE_VM_COMMANDS").is_some()) {
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
                if (std::env::var_os("SIGLUS_TRACE_VM_COMMANDS").is_some()) {
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

                if !self.dispatch_owner_named_command(owner, raw_head, ret_form, args)? {
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
            if self.ctx.wait_poll() {
                self.delayed_ret_form = Some(ret_form);
                return Ok(());
            }
        }

        self.take_ctx_return(ret_form)?;
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
        let return_pc = caller.return_pc;
        let ret_form = caller.ret_form;
        if std::env::var_os("SIGLUS_TRACE_CALL_RETURN_PC").is_some() {
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

        if callee.excall_proc {
            self.mark_excall_script_proc_pop_requested();
        }

        Ok(callee.frame_action_proc)
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
        self.sg_omv_trace(format!(
            "load_scene_stream resolved target={} scene_no={} z={} initial_pc=0x{:x} scn_len=0x{:x}",
            scene_name,
            scene_no,
            z_no,
            stream.get_prg_cntr(),
            stream.scn.len()
        ));
        self.call_cmd_names = self
            .scene_pck_cache
            .as_ref()
            .expect("scene pck cache initialized")
            .inc_cmd_name_map
            .clone();
        self.user_cmd_names = stream.scn_cmd_name_map.clone();
        match stream.jump_to_z_label(z_no.max(0) as usize) {
            Ok(()) => {
                self.sg_omv_trace(format!(
                    "load_scene_stream entered target={} scene_no={} z={} target_pc=0x{:x} user_cmd_cnt={} call_cmd_cnt={}",
                    scene_name,
                    scene_no,
                    z_no,
                    stream.get_prg_cntr(),
                    stream.scn_cmd_name_map.len(),
                    self.call_cmd_names.len()
                ));
            }
            Err(e) => {
                self.sg_omv_trace(format!(
                    "load_scene_stream failed target={} scene_no={} z={} error={}",
                    scene_name,
                    scene_no,
                    z_no,
                    e
                ));
                return Err(e);
            }
        }
        Ok((stream, scene_no))
    }

    fn jump_to_scene_name(&mut self, scene_name: &str, z_no: i32) -> Result<()> {
        self.sg_omv_trace(format!("scene_jump target={} z={}", scene_name, z_no));
        let (stream, scene_no) = self.load_scene_stream(scene_name, z_no)?;
        self.stream = stream;
        self.current_scene_no = Some(scene_no);
        self.current_scene_name = Some(scene_name.to_string());
        self.current_line_no = -1;
        self.ctx.current_scene_no = Some(scene_no as i64);
        self.ctx.current_scene_name = Some(scene_name.to_string());
        self.ctx.current_line_no = -1;
        self.sg_omv_trace(format!(
            "scene_jump_entered target={} scene_no={} z={} pc=0x{:x}",
            scene_name,
            scene_no,
            z_no,
            self.stream.get_prg_cntr()
        ));
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
        self.sg_omv_trace(format!(
            "scene_farcall target={} z={} ret_form={} ex_call_proc={} scratch_argc={}",
            scene_name,
            z_no,
            ret_form,
            ex_call_proc,
            scratch_source_args.len()
        ));
        let saved = SceneExecFrame {
            stream: self.stream.clone(),
            user_cmd_names: self.user_cmd_names.clone(),
            call_cmd_names: self.call_cmd_names.clone(),
            int_stack: std::mem::take(&mut self.int_stack),
            str_stack: std::mem::take(&mut self.str_stack),
            element_points: std::mem::take(&mut self.element_points),
            call_stack: std::mem::take(&mut self.call_stack),
            gosub_return_stack: std::mem::take(&mut self.gosub_return_stack),
            user_props: std::mem::take(&mut self.user_props),
            current_scene_no: self.current_scene_no,
            current_scene_name: self.current_scene_name.clone(),
            current_line_no: self.current_line_no,
            ret_form,
            excall_proc: ex_call_proc,
        };
        self.scene_stack.push(saved);
        let (stream, scene_no) = self.load_scene_stream(scene_name, z_no)?;
        self.stream = stream;
        let scratch_args = self.call_scratch_from_args(scratch_source_args);
        self.call_stack.push(self.make_call_frame(
            self.cfg.fm_void,
            false,
            false,
            scratch_source_args.len(),
            Some(scratch_args),
        ));
        self.current_scene_no = Some(scene_no);
        self.current_scene_name = Some(scene_name.to_string());
        self.current_line_no = -1;
        self.ctx.current_scene_no = Some(scene_no as i64);
        self.ctx.current_scene_name = Some(scene_name.to_string());
        self.ctx.current_line_no = -1;
        self.sg_omv_trace(format!(
            "scene_farcall_entered target={} scene_no={} z={} pc=0x{:x} call_depth={} scene_stack={}",
            scene_name,
            scene_no,
            z_no,
            self.stream.get_prg_cntr(),
            self.call_stack.len(),
            self.scene_stack.len()
        ));
        if ex_call_proc {
            self.mark_excall_script_proc_requested();
        }
        Ok(())
    }

    fn return_from_scene(&mut self, args: Vec<Value>) -> Result<bool> {
        let Some(saved) = self.scene_stack.pop() else {
            return Ok(false);
        };
        self.sg_omv_trace(format!(
            "scene_return restore_scene={:?} restore_line={} ret_form={} args={:?}",
            saved.current_scene_name,
            saved.current_line_no,
            saved.ret_form,
            args
        ));
        self.stream = saved.stream;
        self.int_stack = saved.int_stack;
        self.str_stack = saved.str_stack;
        self.element_points = saved.element_points;
        self.call_stack = saved.call_stack;
        self.gosub_return_stack = saved.gosub_return_stack;
        self.user_props = saved.user_props;
        self.current_scene_no = saved.current_scene_no;
        self.current_scene_name = saved.current_scene_name;
        self.current_line_no = saved.current_line_no;
        self.ctx.current_scene_no = self.current_scene_no.map(|v| v as i64);
        self.ctx.current_scene_name = self.current_scene_name.clone();
        self.ctx.current_line_no = self.current_line_no as i64;
        self.user_cmd_names = saved.user_cmd_names;
        self.call_cmd_names = saved.call_cmd_names;
        let was_excall_proc = saved.excall_proc;

        match saved.ret_form {
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
        if was_excall_proc {
            self.mark_excall_script_proc_pop_requested();
        }
        self.sg_omv_trace(format!(
            "scene_return_restored scene={:?} scene_no={:?} line={} pc=0x{:x} call_depth={} scene_stack={}",
            self.current_scene_name,
            self.current_scene_no,
            self.current_line_no,
            self.stream.get_prg_cntr(),
            self.call_stack.len(),
            self.scene_stack.len()
        ));
        Ok(true)
    }

    fn exec_builtin_global_control(&mut self, form_id: i32, ret_form: i32) -> Result<bool> {
        match form_id {
            constants::elm_value::GLOBAL_SAVEPOINT => {
                self.save_point = Some(self.make_resume_point());
                if ret_form != self.cfg.fm_void {
                    self.ctx.stack.push(Value::Int(0));
                }
                Ok(true)
            }
            constants::elm_value::GLOBAL_CLEAR_SAVEPOINT => {
                self.save_point = None;
                Ok(true)
            }
            constants::elm_value::GLOBAL_CHECK_SAVEPOINT => {
                self.ctx
                    .stack
                    .push(Value::Int(if self.save_point.is_some() { 1 } else { 0 }));
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

    fn exec_builtin_scene_form(
        &mut self,
        form_id: i32,
        al_id: i32,
        ret_form: i32,
        args: &[Value],
    ) -> Result<bool> {
        const FORM_GLOBAL_JUMP: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_JUMP;
        const FORM_GLOBAL_FARCALL: i32 = crate::runtime::forms::codes::elm_value::GLOBAL_FARCALL;
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
                None => bail!("missing ctx return int for form {}", ret_form),
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
        let stage_form = if self.ctx.ids.form_global_stage != 0 {
            self.ctx.ids.form_global_stage as i32
        } else {
            crate::runtime::forms::codes::FORM_GLOBAL_STAGE as i32
        };
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
            && elm[0] == stage_form
            && is_array_token(elm[1], elm_array)
            && elm[2] >= 0
            && elm[3] == stage_object
            && is_array_token(elm[4], elm_array)
            && elm[5] >= 0
            && object_chain_tail_is_plain_object_ref(elm, 6, elm_array)
        {
            Some((elm[2] as i64, elm[5] as usize))
        } else if elm.len() >= 9
            && elm[0] == stage_form
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
            && elm[0] == stage_form
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
        let runtime_slot = self.runtime_slot_from_object_chain(stage_idx, fallback_obj_idx, elm);
        let prev_chain = self.ctx.globals.current_object_chain.clone();
        let prev_stage_object = self.ctx.globals.current_stage_object;
        self.ctx.globals.current_object_chain = Some(elm.to_vec());
        self.ctx.globals.current_stage_object = Some((stage_idx, runtime_slot));
        if Self::sg_mwnd_object_trace_enabled() && Self::sg_mwnd_chain_interesting(elm) {
            self.sg_mwnd_object_trace(format!(
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
        if elm.is_empty() {
            return;
        }
        let stage_form = if self.ctx.ids.form_global_stage != 0 {
            self.ctx.ids.form_global_stage as i32
        } else {
            crate::runtime::forms::codes::FORM_GLOBAL_STAGE as i32
        };
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
            && elm[0] == stage_form
            && is_array_token(elm[1], elm_array)
            && elm[2] >= 0
            && elm[3] == stage_object
            && is_array_token(elm[4], elm_array)
            && elm[5] >= 0
        {
            6usize
        } else if elm.len() >= 9
            && elm[0] == stage_form
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
            && elm[0] == stage_form
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
        if Self::sg_mwnd_object_trace_enabled() && Self::sg_mwnd_chain_interesting(elm) {
            self.sg_mwnd_object_trace(format!(
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
        self.push_int(out);
        Ok(())
    }

    fn exec_operate_2(&mut self, form_l: i32, form_r: i32, opr: u8) -> Result<()> {
        // int/int
        if form_l == self.cfg.fm_int && form_r == self.cfg.fm_int {
            let r = self.pop_int()?;
            let l = self.pop_int()?;
            let out = self.calc_int_int(l, r, opr);
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
