//! Scene VM

use anyhow::{anyhow, bail, Result};
use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::elm_code;
use crate::runtime::globals::{
    ObjectFrameActionState, PendingButtonAction, PendingButtonActionKind, PendingFrameActionFinish,
};
use crate::runtime::{self, constants, CommandContext, RuntimeLoadRequest, RuntimeSaveKind, RuntimeSaveRequest, Value};
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
    return_pc: usize,
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
            delayed_ret_form: None,
            user_props: Vec::new(),
            int_args,
            str_args,
        }
    }

    fn shared_user_prop_count(&self) -> usize {
        self.scene_pck_cache
            .as_ref()
            .map(|pck| pck.inc_props.len())
            .unwrap_or_else(|| self.stream.header.scn_prop_cnt.max(0) as usize)
    }

    fn enter_cross_scene_user_prop_scope(&mut self) -> BTreeMap<u16, UserPropCell> {
        let saved_user_props = std::mem::take(&mut self.user_props);
        let shared_count = self.shared_user_prop_count();
        self.user_props = saved_user_props
            .iter()
            .filter_map(|(&prop_id, cell)| {
                if (prop_id as usize) < shared_count {
                    Some((prop_id, cell.clone()))
                } else {
                    None
                }
            })
            .collect();
        saved_user_props
    }

    fn restore_cross_scene_user_prop_scope(
        &mut self,
        mut saved_user_props: BTreeMap<u16, UserPropCell>,
    ) {
        let shared_count = self.shared_user_prop_count();
        for prop_id in 0..shared_count {
            saved_user_props.remove(&(prop_id as u16));
        }
        for (&prop_id, cell) in self.user_props.iter() {
            if (prop_id as usize) < shared_count {
                saved_user_props.insert(prop_id, cell.clone());
            }
        }
        self.user_props = saved_user_props;
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
            delayed_ret_form: None,
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
            delayed_ret_form: None,
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
            if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
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
            if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
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

        if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
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
        if std::env::var_os("SG_PROC_FLOW_TRACE").is_some() {
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

    fn sg_cgm_coord_trace(&self, msg: impl AsRef<str>) {
        if !Self::sg_debug_enabled() {
            return;
        }
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
            msg.as_ref()
        );
    }

    fn trace_cgm_coord_assign(&self, elm: &[i32], rhs: &Value) {
        if !Self::sg_debug_enabled() || elm.len() < 3 {
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
                self.sg_cgm_coord_trace(format!("global B[{}] <- {:?}", idx, rhs));
            }
        } else if head == crate::runtime::forms::codes::elm_value::GLOBAL_S as u32
            && (1120..=1139).contains(&idx)
        {
            self.sg_cgm_coord_trace(format!("global S[{}] <- {:?}", idx, rhs));
        }
    }


    fn cf_branch_trace_interesting_line(&self) -> bool {
        if self.current_scene_name.as_deref() != Some("sys10_cf01") {
            return false;
        }
        matches!(self.current_line_no, 700..=730 | 870..=895)
    }

    fn cf_condition_trace_interesting_line(&self) -> bool {
        if !Self::sg_debug_enabled() {
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
        if !Self::sg_debug_enabled() {
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

    fn sg_cf_branch_trace(&self, pc: usize, msg: impl AsRef<str>) {
        if !Self::sg_debug_enabled() {
            return;
        }
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
            msg.as_ref(),
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
            self.sg_cf_branch_trace(
                pc,
                format!(
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
        if !(self.current_scene_name.as_deref() == Some("sys10_cf01")
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
        self.sg_cf_branch_trace(
            pc,
            format!(
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
        let saved_user_props = self.enter_cross_scene_user_prop_scope();

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
        self.restore_cross_scene_user_prop_scope(saved_user_props);
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
            user_props: self.enter_cross_scene_user_prop_scope(),
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
        let saved_user_props = self.enter_cross_scene_user_prop_scope();

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
        self.restore_cross_scene_user_prop_scope(saved_user_props);
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

    fn syscom_proc_trace_enabled() -> bool {
        std::env::var_os("SG_SYSCOM_PROC_TRACE").is_some()
            || std::env::var_os("SG_DEBUG").is_some()
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
        let trace = Self::syscom_proc_trace_enabled();
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
        if self.ctx.globals.syscom.pending_proc.is_some() {
            if trace || std::env::var_os("SG_DEBUG").is_some() {
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
        self.ctx.local_save_snapshot = None;
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
                let before_tail_start = self.int_stack.len().saturating_sub(16);
                let before_tail = self.int_stack[before_tail_start..].to_vec();
                let cond = self.pop_int()?;
                let taken = cond != 0;
                self.sg_omv_trace(format!("GOTO_TRUE label={} cond={} taken={}", label_no, cond, taken));
                self.trace_cf_branch_goto(pc_before, "GOTO_TRUE", label_no, cond, taken, &before_tail);
                if taken {
                    self.stream.jump_to_label(label_no.max(0) as usize)?;
                }
            }
            CD_GOTO_FALSE => {
                let label_no = self.stream.pop_i32()?;
                let before_tail_start = self.int_stack.len().saturating_sub(16);
                let before_tail = self.int_stack[before_tail_start..].to_vec();
                let cond = self.pop_int()?;
                let taken = cond == 0;
                self.sg_omv_trace(format!("GOTO_FALSE label={} cond={} taken={}", label_no, cond, taken));
                self.trace_cf_branch_goto(pc_before, "GOTO_FALSE", label_no, cond, taken, &before_tail);
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
                Err(anyhow!(
                    "int stack underflow: scene={} scene_no={} line={} pc=0x{:x}",
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

    fn exec_user_prop_list_init_command(
        &mut self,
        prop_id: u16,
        sub: &[i32],
        ret_form: i32,
    ) -> Result<bool> {
        use crate::runtime::forms::codes::{ELM_INTLIST_INIT, ELM_STRLIST_INIT};

        if sub.len() != 1 {
            return Ok(false);
        }
        let (form, size) = self
            .user_prop_decl(prop_id)
            .unwrap_or((self.cfg.fm_list, 0));
        if form == self.cfg.fm_intlist && sub[0] == ELM_INTLIST_INIT {
            let mut cell = self
                .user_props
                .remove(&prop_id)
                .unwrap_or_else(|| self.default_user_prop_cell(prop_id));
            cell.form = form;
            cell.element = self.default_user_prop_element(prop_id, form);
            cell.int_list.clear();
            cell.int_list.resize(size, 0);
            self.user_props.insert(prop_id, cell);
            self.push_default_for_ret(ret_form);
            return Ok(true);
        }
        if form == self.cfg.fm_strlist && sub[0] == ELM_STRLIST_INIT {
            let mut cell = self
                .user_props
                .remove(&prop_id)
                .unwrap_or_else(|| self.default_user_prop_cell(prop_id));
            cell.form = form;
            cell.element = self.default_user_prop_element(prop_id, form);
            cell.str_list.clear();
            cell.str_list.resize_with(size, String::new);
            self.user_props.insert(prop_id, cell);
            self.push_default_for_ret(ret_form);
            return Ok(true);
        }
        Ok(false)
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
                if Self::sg_mwnd_object_trace_enabled()
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
            let stage_form = if self.ctx.ids.form_global_stage != 0 {
                self.ctx.ids.form_global_stage as i32
            } else {
                crate::runtime::forms::codes::FORM_GLOBAL_STAGE as i32
            };
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
            if Self::sg_mwnd_object_trace_enabled()
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
            let array_idx = self.extract_array_index(&elm);
            self.trace_cf_condition_user_prop_read(
                self.stream.get_prg_cntr(),
                prop_id,
                array_idx,
                &cell,
                &elm,
            );
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

        if owner == elm_code::ELM_OWNER_USER_PROP {
            let prop_id = elm_code::code(raw_head);
            if self.exec_user_prop_list_init_command(prop_id, &elm[1..], ret_form)? {
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
                if self.exec_builtin_scene_form(&elm, form_id, al_id, ret_form, args)? {
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
        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now();
        slot.exist = true;
        slot.year = now.year() as i64;
        slot.month = now.month() as i64;
        slot.day = now.day() as i64;
        // SYSTEMTIME.wDayOfWeek uses 0..6 with Sunday = 0.
        slot.weekday = now.weekday().num_days_from_sunday() as i64;
        slot.hour = now.hour() as i64;
        slot.minute = now.minute() as i64;
        slot.second = now.second() as i64;
        slot.millisecond = now.timestamp_subsec_millis() as i64;
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
        self.ctx
            .tables
            .gameexe
            .as_ref()
            .and_then(|cfg| cfg.get_usize("#FLAG.CNT").or_else(|| cfg.get_usize("FLAG.CNT")))
            .unwrap_or(1000)
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

        push_i32(&mut out, 0);
        push_i32(&mut out, 0);
        push_i32(&mut out, 0);
        push_i32(&mut out, 0);
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
        push_i32(&mut out, 0);

        push_i32(&mut out, script.msg_speed);
        push_bool(&mut out, script.msg_nowait);
        push_bool(&mut out, script.async_msg_mode);
        push_bool(&mut out, script.async_msg_mode_once);
        push_bool(&mut out, false);
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

    fn write_cpp_syscom_menu(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        let base = w.position();
        let s = &self.ctx.globals.syscom;
        let push_ex = |w: &mut crate::original_save::OriginalStreamWriter, exist: bool, enable: bool| {
            w.push_bool(exist);
            w.push_bool(enable);
        };
        push_ex(w, s.read_skip.exist, s.read_skip.enable);
        push_ex(w, false, false);
        push_ex(w, s.auto_skip.exist, s.auto_skip.enable);
        push_ex(w, s.auto_mode.exist, s.auto_mode.enable);
        push_ex(w, s.hide_mwnd.exist, s.hide_mwnd.enable);
        push_ex(w, s.msg_back.exist, s.msg_back.enable);
        push_ex(w, s.save_feature.exist, s.save_feature.enable);
        push_ex(w, s.load_feature.exist, s.load_feature.enable);
        push_ex(w, s.return_to_sel.exist, s.return_to_sel.enable);
        push_ex(w, true, true);
        push_ex(w, false, false);
        push_ex(w, false, false);
        push_ex(w, s.return_to_menu.exist, s.return_to_menu.enable);
        push_ex(w, s.end_game.exist, s.end_game.enable);
        push_ex(w, true, true);
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
        w.push_i32(self.current_scene_no.unwrap_or(0) as i32);
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
        let _scn_no = rd.i32()?;
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
        w.push_extend_items(&frame.user_props, |w, prop| self.write_cpp_call_prop(w, prop));
        let call_type = if frame.frame_action_proc { 3 } else if frame.return_pc != 0 { 1 } else { 0 };
        w.push_i32(call_type);
        w.push_i32(frame.ret_form);
        w.push_str(self.current_scene_name.as_deref().unwrap_or(""));
        w.push_i32(self.current_line_no);
        w.push_i32(frame.return_pc as i32);
    }

    fn read_cpp_call_frame(&self, rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<CallFrame> {
        let int_args: Vec<i32> = rd.extend_i32_list()?.into_iter().map(|v| v as i32).collect();
        let str_args: Vec<String> = rd.extend_items(|rd| rd.string())?;
        let user_props = rd.extend_items(|rd| self.read_cpp_call_prop(rd))?;
        let call_type = rd.i32()?;
        let ret_form = rd.i32()?;
        let _scn_name = rd.string()?;
        let _line_no = rd.i32()?;
        let return_pc = rd.i32()?.max(0) as usize;
        Ok(CallFrame {
            return_pc,
            ret_form,
            return_override: None,
            excall_proc: false,
            frame_action_proc: call_type == 3,
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
        w.push_bool(false);
        w.push_bool(g.wait_flag);
        w.push_bool(g.cancel_flag);
        w.push_empty_element();
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
        let _pause_flag = rd.bool()?;
        g.wait_flag = rd.bool()?;
        g.cancel_flag = rd.bool()?;
        let _target_object = rd.element()?;
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
        w.push_i32(0); w.push_i32(0); w.push_i32(0); w.push_i32(0); w.push_i32(0);
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
            let wp = &obj.weather_param;
            for v in [wp.weather_type, wp.cnt, wp.pat_mode, wp.pat_no_00, wp.pat_no_01, wp.pat_time, wp.move_time_x, wp.move_time_y, wp.sin_time_x, wp.sin_time_y, wp.sin_power_x, wp.sin_power_y, wp.center_x, wp.center_y, wp.center_rotate, wp.appear_range, wp.zoom_min, wp.zoom_max] {
                w.push_i32(Self::save_i32(v));
            }
            Self::write_cpp_int_event_raw(w, &ev.color_add_r);
            Self::write_cpp_int_event_raw(w, &ev.color_add_g);
            Self::write_cpp_int_event_raw(w, &ev.color_add_b);
            for v in [b.mask_no, b.tonecurve_no, b.light_no, b.fog_use, b.culling, b.alpha_test, b.alpha_blend, b.blend, 0] {
                w.push_i32(Self::save_i32(v));
            }
        }
        w.push_i32(Self::save_i32(obj.thumb_save_no));
        w.push_bool(obj.movie.loop_flag);
        w.push_bool(obj.movie.auto_free_flag);
        w.push_bool(obj.movie.real_time_flag);
        w.push_bool(obj.movie.pause_flag);
        if obj.object_type == 10 {
            w.push_i32(Self::save_i32(obj.emote.width));
            w.push_i32(Self::save_i32(obj.emote.height));
            for _ in 0..8 { w.push_i32(0); }
            w.push_i32(0);
            w.push_i32(0);
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
        if obj.object_type == 10 { for _ in 0..8 { w.push_str(""); } }
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
        rd.skip(20)?;
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
            obj.runtime.prop_events.color_add_r = Self::read_cpp_int_event_raw(rd)?;
            obj.runtime.prop_events.color_add_g = Self::read_cpp_int_event_raw(rd)?;
            obj.runtime.prop_events.color_add_b = Self::read_cpp_int_event_raw(rd)?;
            obj.base.mask_no = rd.i32()? as i64;
            obj.base.tonecurve_no = rd.i32()? as i64;
            obj.base.light_no = rd.i32()? as i64;
            obj.base.fog_use = rd.i32()? as i64;
            obj.base.culling = rd.i32()? as i64;
            obj.base.alpha_test = rd.i32()? as i64;
            obj.base.alpha_blend = rd.i32()? as i64;
            obj.base.blend = rd.i32()? as i64;
            let _ = rd.i32()?;
        }
        obj.thumb_save_no = rd.i32()? as i64;
        obj.movie.loop_flag = rd.bool()?;
        obj.movie.auto_free_flag = rd.bool()?;
        obj.movie.real_time_flag = rd.bool()?;
        obj.movie.pause_flag = rd.bool()?;
        if obj.object_type == 10 {
            obj.emote.width = rd.i32()? as i64;
            obj.emote.height = rd.i32()? as i64;
            rd.skip(8 * 4)?;
            let _ = rd.i32()?;
            let _ = rd.i32()?;
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
        obj.base.patno = rd.i32()? as i64;
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
        if obj.object_type == 10 { for _ in 0..8 { let _ = rd.string()?; } }
        obj.frame_action = Self::read_cpp_frame_action(rd)?;
        obj.frame_action_ch = rd.extend_items(|rd| Self::read_cpp_frame_action(rd))?;
        let gan_file = rd.string()?;
        obj.gan_file = if gan_file.is_empty() { None } else { Some(gan_file) };
        obj.runtime.child_objects = rd.extend_items(|rd| Self::read_cpp_object(rd))?;
        obj.used = obj.object_type != 0 || obj.file_name.is_some() || obj.string_value.is_some();
        Ok(obj)
    }

    fn write_cpp_mwnd(&self, w: &mut crate::original_save::OriginalStreamWriter, m: &runtime::globals::MwndState) {
        w.push_i32(Self::save_i32(m.world));
        w.push_i32(Self::save_i32(m.layer));
        w.push_i32(if m.open { 1 } else { 0 });
        w.push_i32(Self::save_i32(m.window_pos.map(|p| p.0).unwrap_or(0)));
        w.push_i32(Self::save_i32(m.window_pos.map(|p| p.1).unwrap_or(0)));
        w.push_i32(Self::save_i32(m.window_size.map(|p| p.0).unwrap_or(0)));
        w.push_i32(Self::save_i32(m.window_size.map(|p| p.1).unwrap_or(0)));
        w.push_i32(Self::save_i32(m.open_anime_type));
        w.push_i32(Self::save_i32(m.open_anime_time));
        w.push_i32(Self::save_i32(m.close_anime_type));
        w.push_i32(Self::save_i32(m.close_anime_time));
        w.push_i64(0);
        w.push_bool(m.msg_block_started);
        w.push_bool(false);
        w.push_bool(m.open);
        w.push_bool(!m.name_text.is_empty());
        w.push_bool(m.clear_ready);
        w.push_padding(3);
        w.push_i32(0); w.push_i32(0); w.push_i32(0);
        w.push_bool(m.slide_msg);
        w.push_padding(3);
        w.push_i32(Self::save_i32(m.slide_time));
        w.push_i32(m.koe.map(|p| Self::save_i32(p.0)).unwrap_or(0));
        w.push_bool(m.koe.is_some());
        w.push_padding(3);
        w.push_i32(Self::save_i32(m.open_anime_type));
        w.push_i32(Self::save_i32(m.open_anime_time));
        w.push_i32(0);
        w.push_i32(Self::save_i32(m.close_anime_type));
        w.push_i32(Self::save_i32(m.close_anime_time));
        w.push_i32(0);
        w.push_i32(1);
        w.push_bool(false);
        w.push_padding(3);
        w.push_str(&m.msg_text);
        w.push_str(&m.waku_file);
        w.push_str(&m.name_text);
        w.push_i32(0);
        w.push_i32(0);
        w.push_i32(0);
        w.push_extend_items(&m.object_list, |w, obj| self.write_cpp_object(w, obj));
        w.push_extend_items(&m.button_list, |w, obj| self.write_cpp_object(w, obj));
        w.push_extend_items(&m.face_list, |w, obj| self.write_cpp_object(w, obj));
    }

    fn read_cpp_mwnd(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<runtime::globals::MwndState> {
        let mut m = runtime::globals::MwndState::default();
        m.world = rd.i32()? as i64;
        m.layer = rd.i32()? as i64;
        m.open = rd.i32()? != 0;
        let wx = rd.i32()? as i64;
        let wy = rd.i32()? as i64;
        m.window_pos = Some((wx, wy));
        let ww = rd.i32()? as i64;
        let wh = rd.i32()? as i64;
        m.window_size = Some((ww, wh));
        m.open_anime_type = rd.i32()? as i64;
        m.open_anime_time = rd.i32()? as i64;
        m.close_anime_type = rd.i32()? as i64;
        m.close_anime_time = rd.i32()? as i64;
        let _ = rd.i64()?;
        m.msg_block_started = rd.bool()?;
        let _ = rd.bool()?;
        m.open = rd.bool()?;
        let _ = rd.bool()?;
        m.clear_ready = rd.bool()?;
        rd.skip(3)?;
        let _ = rd.i32()?; let _ = rd.i32()?; let _ = rd.i32()?;
        m.slide_msg = rd.bool()?;
        rd.skip(3)?;
        m.slide_time = rd.i32()? as i64;
        let koe_no = rd.i32()? as i64;
        let koe_play = rd.bool()?;
        rd.skip(3)?;
        if koe_play { m.koe = Some((koe_no, 0)); }
        m.open_anime_type = rd.i32()? as i64;
        m.open_anime_time = rd.i32()? as i64;
        let _ = rd.i32()?;
        m.close_anime_type = rd.i32()? as i64;
        m.close_anime_time = rd.i32()? as i64;
        let _ = rd.i32()?;
        let _ = rd.i32()?;
        let _ = rd.bool()?;
        rd.skip(3)?;
        m.msg_text = rd.string()?;
        m.waku_file = rd.string()?;
        m.name_text = rd.string()?;
        let _ = rd.i32()?; let _ = rd.i32()?; let _ = rd.i32()?;
        m.object_list = rd.extend_items(|rd| Self::read_cpp_object(rd))?;
        m.button_list = rd.extend_items(|rd| Self::read_cpp_object(rd))?;
        m.face_list = rd.extend_items(|rd| Self::read_cpp_object(rd))?;
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
        w.push_i32(0);
        w.push_i32(0);
        w.push_i32(if q.ending { 1 } else { 0 });
        w.push_i32(0);
        w.push_i32(0);
        w.push_i32(0);
        w.push_i32(0);
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
        let _cur_time = rd.i32()?;
        let _total_time = rd.i32()?;
        q.ending = rd.i32()? != 0;
        let _end_cur_time = rd.i32()?;
        let _end_total_time = rd.i32()?;
        let _cnt = rd.i32()?;
        let _end_cnt = rd.i32()?;
        q.center_x = rd.i32()?;
        q.center_y = rd.i32()?;
        q.begin_order = rd.i32()?;
        q.end_order = rd.i32()?;
        Ok(q)
    }

    fn write_cpp_btn_select(&self, w: &mut crate::original_save::OriginalStreamWriter) {
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

    fn read_cpp_btn_select(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<()> {
        let _ = rd.i32()?;
        rd.skip(112)?;
        for _ in 0..6 { let _ = rd.bool()?; }
        let _ = rd.string()?;
        let _ = rd.i32()?;
        let _ = rd.i32()?;
        Ok(())
    }

    fn write_cpp_stage(&self, w: &mut crate::original_save::OriginalStreamWriter, stage_idx: i64) {
        let form_id = self.ctx.ids.form_global_stage;
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
        self.write_cpp_btn_select(w);
        w.push_fixed_items(worlds, |w, world| self.write_cpp_world(w, world));
        w.push_fixed_items(effects, |w, e| self.write_cpp_effect(w, e));
        w.push_fixed_items(quakes, |w, q| self.write_cpp_quake(w, q));
    }

    fn read_cpp_stage(rd: &mut crate::original_save::OriginalStreamReader<'_>, stage_idx: i64) -> Result<runtime::globals::StageFormState> {
        let mut st = runtime::globals::StageFormState::default();
        st.initialized_from_gameexe = true;
        st.group_lists.insert(stage_idx, rd.fixed_items(|rd| Self::read_cpp_group(rd))?);
        st.object_lists.insert(stage_idx, rd.fixed_items(|rd| Self::read_cpp_object(rd))?);
        st.mwnd_lists.insert(stage_idx, rd.fixed_items(|rd| Self::read_cpp_mwnd(rd))?);
        Self::read_cpp_btn_select(rd)?;
        let mut world_no = 0i32;
        let worlds = rd.fixed_items(|rd| { let w = Self::read_cpp_world(rd, world_no); world_no += 1; w })?;
        st.world_lists.insert(stage_idx, worlds);
        st.effect_lists.insert(stage_idx, rd.fixed_items(|rd| Self::read_cpp_effect(rd))?);
        st.quake_lists.insert(stage_idx, rd.fixed_items(|rd| Self::read_cpp_quake(rd))?);
        Ok(st)
    }

    fn write_cpp_screen(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        let form_id = self.ctx.ids.form_global_screen;
        let screen = self.ctx.globals.screen_forms.get(&form_id).cloned().unwrap_or_default();
        w.push_fixed_items(&screen.effect_list, |w, e| self.write_cpp_effect(w, e));
        w.push_i32(Self::save_i32(screen.shake.last_value));
        w.push_i64(0);
        w.push_i32(0);
        w.push_fixed_items(&screen.quake_list, |w, q| self.write_cpp_quake(w, q));
    }

    fn read_cpp_screen(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<runtime::globals::ScreenFormState> {
        let effect_list = rd.fixed_items(|rd| Self::read_cpp_effect(rd))?;
        let mut shake = runtime::globals::ScreenShakeState::default();
        shake.last_value = rd.i32()? as i64;
        let _ = rd.i64()?;
        let _ = rd.i32()?;
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

    fn write_cpp_pcmch_default(w: &mut crate::original_save::OriginalStreamWriter) {
        w.push_str("");
        w.push_str("");
        w.push_i32(-1);
        w.push_i32(-1);
        w.push_i32(0);
        w.push_i32(-1);
        w.push_i32(255);
        w.push_i32(0);
        w.push_bool(false);
        w.push_bool(false);
        w.push_bool(false);
        w.push_bool(false);
        w.push_bool(false);
    }

    fn read_cpp_pcmch(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<()> {
        let _pcm_name = rd.string()?;
        let _bgm_name = rd.string()?;
        let _koe_no = rd.i32()?;
        let _se_no = rd.i32()?;
        let _volume_type = rd.i32()?;
        let _chara_no = rd.i32()?;
        let _volume = rd.i32()?;
        let _delay_time = rd.i32()?;
        let _loop_flag = rd.bool()?;
        let _bgm_fade_target_flag = rd.bool()?;
        let _bgm_fade2_target_flag = rd.bool()?;
        let _bgm_fade_source_flag = rd.bool()?;
        let _ready_flag = rd.bool()?;
        Ok(())
    }

    fn write_cpp_sound(&self, w: &mut crate::original_save::OriginalStreamWriter) {
        // C_elm_sound::save order: BGM, KOE, PCM, PCMCHLIST, SE, MOV.
        // The runtime currently does not retain all original sound parameters, so this writes
        // a structurally exact silent/default sound state instead of a truncated one.
        w.push_str("");
        w.push_i32(255);
        w.push_i32(0);
        w.push_bool(false);
        w.push_bool(false);
        w.push_i32(255);
        w.push_i32(255);
        let pcmch_defaults = vec![(); self.original_pcmch_count()];
        w.push_fixed_items(&pcmch_defaults, |w, _| Self::write_cpp_pcmch_default(w));
        w.push_i32(255);
        w.push_str("");
    }

    fn read_cpp_sound(rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<()> {
        let _bgm_regist_name = rd.string()?;
        let _bgm_volume = rd.i32()?;
        let _bgm_delay_time = rd.i32()?;
        let _bgm_loop_flag = rd.bool()?;
        let _bgm_pause_flag = rd.bool()?;
        let _koe_volume = rd.i32()?;
        let _pcm_volume = rd.i32()?;
        rd.fixed_items(|rd| Self::read_cpp_pcmch(rd))?;
        let _se_volume = rd.i32()?;
        let _mov_file_name = rd.string()?;
        Ok(())
    }

    fn write_cpp_pcm_event(&self, w: &mut crate::original_save::OriginalStreamWriter, ev: &runtime::globals::PcmEventState) {
        let ty = if ev.random { 2 } else if ev.looped { 1 } else if ev.active { 0 } else { -1 };
        w.push_i32(ty);
        if ty == 1 || ty == 2 {
            w.push_i32(0);
            w.push_i32(ev.volume_type);
            w.push_i32(ev.chara_no);
            w.push_bool(ev.bgm_fade_target_flag);
            w.push_bool(ev.bgm_fade2_target_flag);
            w.push_bool(ev.bgm_fade2_source_flag);
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
        ev.active = ty >= 0;
        ev.looped = ty == 1;
        ev.random = ty == 2;
        if ty == 1 || ty == 2 {
            let _ = rd.i32()?;
            ev.volume_type = rd.i32()?;
            ev.chara_no = rd.i32()?;
            ev.bgm_fade_target_flag = rd.bool()?;
            ev.bgm_fade2_target_flag = rd.bool()?;
            ev.bgm_fade2_source_flag = rd.bool()?;
            ev.real_flag = rd.bool()?;
            ev.time_type = rd.bool()?;
            ev.lines = rd.extend_items(|rd| Ok(runtime::globals::PcmEventLine { file_name: rd.string()?, min_time: rd.i32()?, max_time: rd.i32()?, probability: rd.i32()? }))?;
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
        let back = Self::read_cpp_stage(rd, 0)?;
        let front = Self::read_cpp_stage(rd, 1)?;
        st.initialized_from_gameexe = true;
        st.group_lists.extend(back.group_lists);
        st.object_lists.extend(back.object_lists);
        st.mwnd_lists.extend(back.mwnd_lists);
        st.world_lists.extend(back.world_lists);
        st.effect_lists.extend(back.effect_lists);
        st.quake_lists.extend(back.quake_lists);
        st.group_lists.extend(front.group_lists);
        st.object_lists.extend(front.object_lists);
        st.mwnd_lists.extend(front.mwnd_lists);
        st.world_lists.extend(front.world_lists);
        st.effect_lists.extend(front.effect_lists);
        st.quake_lists.extend(front.quake_lists);
        self.ctx.globals.stage_forms.insert(self.ctx.ids.form_global_stage, st);

        let screen = Self::read_cpp_screen(rd)?;
        self.ctx.globals.screen_forms.insert(self.ctx.ids.form_global_screen, screen);

        Self::read_cpp_sound(rd)?;

        let pcm_events = rd.fixed_items(|rd| Self::read_cpp_pcm_event(rd))?;
        if !pcm_events.is_empty() {
            self.ctx.globals.pcm_event_lists.insert(self.ctx.ids.form_global_pcm_event, pcm_events);
        }

        let editboxes = rd.fixed_items(|rd| Self::read_cpp_editbox(rd))?;
        if !editboxes.is_empty() {
            self.ctx.globals.editbox_lists.insert(self.ctx.ids.form_global_editbox, runtime::globals::EditBoxListState { boxes: editboxes });
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

    fn read_cpp_proc_record(&self, rd: &mut crate::original_save::OriginalStreamReader<'_>) -> Result<()> {
        let _proc_type = rd.i32()?;
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
        Ok(())
    }

    fn cpp_mwnd_element(stage_idx: i64, mwnd_no: Option<usize>) -> Vec<i32> {
        let Some(no) = mwnd_no else {
            return Vec::new();
        };
        let stage_head = match stage_idx {
            0 => crate::runtime::forms::codes::ELM_GLOBAL_BACK,
            2 => crate::runtime::forms::codes::ELM_GLOBAL_NEXT,
            _ => crate::runtime::forms::codes::ELM_GLOBAL_FRONT,
        };
        vec![
            stage_head,
            crate::runtime::forms::codes::ELM_STAGE_MWND,
            crate::runtime::forms::codes::ELM_ARRAY,
            no as i32,
        ]
    }

    fn decode_cpp_mwnd_element(elm: &[i32]) -> Option<(i64, usize)> {
        if elm.len() < 4 {
            return None;
        }
        let stage_idx = if elm[0] == crate::runtime::forms::codes::ELM_GLOBAL_BACK {
            0
        } else if elm[0] == crate::runtime::forms::codes::ELM_GLOBAL_FRONT {
            1
        } else if elm[0] == crate::runtime::forms::codes::ELM_GLOBAL_NEXT {
            2
        } else {
            return None;
        };
        if elm[1] != crate::runtime::forms::codes::ELM_STAGE_MWND {
            return None;
        }
        if elm[2] != crate::runtime::forms::codes::ELM_ARRAY {
            return None;
        }
        if elm[3] < 0 {
            return None;
        }
        Some((stage_idx, elm[3] as usize))
    }

    fn apply_saved_current_mwnd_elements(
        &mut self,
        cur_mwnd: &[i32],
        cur_sel_mwnd: &[i32],
        last_mwnd: &[i32],
    ) {
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
        use chrono::{Datelike, Timelike};
        let now = chrono::Local::now();
        [
            now.year().clamp(0, u16::MAX as i32) as u16,
            now.month() as u16,
            now.day() as u16,
            now.hour() as u16,
            now.minute() as u16,
            now.second() as u16,
            now.timestamp_subsec_millis() as u16,
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
        let cur_mwnd = Self::cpp_mwnd_element(
            self.ctx.globals.current_mwnd_stage_idx,
            self.ctx.globals.current_mwnd_no,
        );
        let cur_sel_mwnd = Self::cpp_mwnd_element(
            self.ctx.globals.current_sel_mwnd_stage_idx,
            self.ctx.globals.current_sel_mwnd_no,
        );
        let last_mwnd = Self::cpp_mwnd_element(
            self.ctx.globals.last_mwnd_stage_idx,
            self.ctx.globals.last_mwnd_no,
        );
        w.push_element(&cur_mwnd);
        w.push_element(&cur_sel_mwnd);
        w.push_element(&last_mwnd);
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

        w.push_i32(self.call_stack.len() as i32);
        for frame in &self.call_stack {
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

        self.read_cpp_proc_record(&mut rd)?;
        let proc_stack_cnt = rd.i32()?.max(0) as usize;
        for _ in 0..proc_stack_cnt { self.read_cpp_proc_record(&mut rd)?; }
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
        rd.skip(356)?;

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
        rd.skip(76)?;

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

    fn save_load_trace_enabled() -> bool {
        std::env::var_os("SG_SAVELOAD_TRACE").is_some()
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
            if Self::save_load_trace_enabled() {
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
            return Ok(());
        }

        // Refresh local_ex_stream from the live runtime; mirrors C++ `save_local_ex()`
        // being called inside `tnm_save_local_on_file` right before writing.
        let refreshed_ex = self.build_original_local_ex_stream();
        if let Some(snapshot) = self.ctx.local_save_snapshot.as_mut() {
            snapshot.local_ex_stream = refreshed_ex;
        }

        let slot = self.ensure_runtime_slot_for_save(req);
        let Some(path) = self.runtime_save_file_path(req.kind, req.index) else { return Ok(()); };
        if Self::save_load_trace_enabled() {
            eprintln!(
                "[SG_SAVELOAD_TRACE][VM] save begin kind={:?} idx={} path={} file_exists_before={}",
                req.kind,
                req.index,
                path.display(),
                path.exists()
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
        crate::original_save::write_local_save_file(&path, &slot, &env)?;
        crate::runtime::forms::syscom::write_global_save(&self.ctx);
        if Self::save_load_trace_enabled() {
            eprintln!(
                "[SG_SAVELOAD_TRACE][VM] save written kind={:?} idx={} path={} bytes={}",
                req.kind,
                req.index,
                path.display(),
                std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
            );
        }
        if let Some(saved_slot) = crate::original_save::read_slot_from_path(&path) {
            match req.kind {
                RuntimeSaveKind::Normal => {
                    if self.ctx.globals.syscom.save_slots.len() <= req.index {
                        self.ctx.globals.syscom.save_slots.resize_with(req.index + 1, Default::default);
                    }
                    self.ctx.globals.syscom.save_slots[req.index] = saved_slot;
                }
                RuntimeSaveKind::Quick => {
                    if self.ctx.globals.syscom.quick_save_slots.len() <= req.index {
                        self.ctx.globals.syscom.quick_save_slots.resize_with(req.index + 1, Default::default);
                    }
                    self.ctx.globals.syscom.quick_save_slots[req.index] = saved_slot;
                }
                RuntimeSaveKind::End => {
                    self.ctx.globals.syscom.end_save_exists = true;
                }
                RuntimeSaveKind::Inner => {}
            }
        }
        if let Some(save_kind) = Self::save_kind_to_original(req.kind) {
            let save_no = crate::original_save::original_save_no(
                self.configured_runtime_save_count(false),
                self.configured_runtime_save_count(true),
                save_kind,
                req.index,
            );
            if Self::save_load_trace_enabled() {
                eprintln!(
                    "[SG_SAVELOAD_TRACE][VM] save thumb write kind={:?} idx={} original_save_no={}",
                    req.kind,
                    req.index,
                    save_no
                );
            }
            crate::runtime::forms::syscom::write_runtime_slot_thumb(&mut self.ctx, save_no);
        }
        Ok(())
    }

    fn perform_runtime_load_request(&mut self, req: RuntimeLoadRequest) -> Result<()> {
        if Self::save_load_trace_enabled() {
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
            if Self::save_load_trace_enabled() {
                eprintln!(
                    "[SG_SAVELOAD_TRACE][VM] load read kind={:?} idx={} path={} file_exists={}",
                    req.kind,
                    req.index,
                    path.display(),
                    path.exists()
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
            let append_dir = meta.append_dir.clone();
            let append_name = meta.append_name.clone();
            self.ctx.globals.append_dir = append_dir.clone();
            self.ctx.globals.append_name = append_name;
            self.ctx.images.set_current_append_dir(append_dir.clone());
            self.ctx.movie.set_current_append_dir(append_dir.clone());
            self.ctx.bgm.set_current_append_dir(append_dir);
        }
        // VM-side equivalent of C++ `tnm_finish_local`: drop excall frames, sel
        // points, and the stale save point. The loaded scene re-establishes its
        // own context; without this, when the loaded scene eventually issues a
        // RETURN we'd pop back into the orphaned save/load menu excall frame.
        self.scene_stack.clear();
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
        self.stream = stream;
        self.int_stack = snapshot.int_stack;
        self.str_stack = snapshot.str_stack;
        self.element_points = snapshot.element_points;
        self.call_stack = snapshot.call_stack;
        if self.call_stack.is_empty() {
            self.call_stack.push(self.scene_base_call());
        }
        self.gosub_return_stack.clear();
        self.current_scene_no = if snapshot.scene_no >= 0 { Some(snapshot.scene_no as usize) } else { Some(scene_no) };
        self.current_scene_name = Some(snapshot.scene_name);
        self.current_line_no = snapshot.line_no;
        self.ctx.current_scene_no = self.current_scene_no.map(|v| v as i64);
        self.ctx.current_scene_name = self.current_scene_name.clone();
        self.ctx.current_line_no = self.current_line_no as i64;
        self.ctx.wait = runtime::wait::VmWait::default();
        self.halted = false;
        self.delayed_ret_form = None;
        // C++ `C_elm_stage::load` / `C_elm_mwnd::load` end by calling each
        // object's `restruct_type()` to rebuild the visible render side
        // (image asset + sprite binding + transform). Rust's gfx runtime is
        // not in the save format, so do the equivalent walk here: for every
        // Gfx-backed object whose `file_name` is set, rebuild its gfx state
        // from the loaded globals. Without this the loaded scene renders as
        // a blank canvas while the saved data is technically all there.
        self.restore_runtime_bindings_after_load();
        self.ctx.mark_runtime_load_completed();
        Ok(())
    }

    /// Walk every PCT-style object the loaded snapshot put back into
    /// `globals.stage_forms` (BG, top-level on each stage, plus mwnd-embedded
    /// button/face/object lists) and ask the gfx runtime to re-bind a sprite
    /// and re-load its image. Also writes `backend = Gfx` back into globals so
    /// the render pipeline's backend-dispatch reaches the Gfx arm instead of
    /// skipping the object (the save format never serialized the backend tag,
    /// so every loaded object starts with `backend = None`).
    ///
    /// Equivalent to the `restruct_type()` tail of C++ `C_elm_object::load`,
    /// for the PCT / SAVE_THUMB / THUMB / CAPTURE family. Specialized backends
    /// (RECT, STRING, NUMBER, WEATHER, MESH, BILLBOARD, MOVIE, EMOTE) carry
    /// runtime sprite IDs that aren't in the save format and would need their
    /// own backend-specific restruct path - logged as warnings here so we know
    /// what's still missing.
    fn restore_runtime_bindings_after_load(&mut self) {
        struct RebuildTask {
            stage_idx: i64,
            path: String,
            runtime_slot: usize,
            obj_snapshot: runtime::globals::ObjectState,
        }

        // PCT (2), SAVE_THUMB (8), THUMB (11), CAPTURE (10) - everything that
        // C++ `restruct_type` routes through a single-image Gfx pipeline. EMOTE
        // and MOVIE use specialized backends here, so they need their own
        // type-specific restruct path and are intentionally not handled as Gfx.
        fn needs_gfx_restore(obj: &runtime::globals::ObjectState) -> bool {
            if !obj.used {
                return false;
            }
            let has_file = obj.file_name.as_deref().map(|s| !s.is_empty()).unwrap_or(false);
            has_file && matches!(obj.object_type, 2 | 8 | 10 | 11)
        }

        fn assign_child_slots_and_backend(
            obj: &mut runtime::globals::ObjectState,
            next_nested: &mut usize,
        ) {
            if needs_gfx_restore(obj) {
                obj.backend = runtime::globals::ObjectBackend::Gfx;
            }
            for child in &mut obj.runtime.child_objects {
                if child.nested_runtime_slot.is_none() {
                    child.nested_runtime_slot = Some(*next_nested);
                    *next_nested += 1;
                }
                assign_child_slots_and_backend(child, next_nested);
            }
        }

        fn collect_rebuild_tasks(
            out: &mut Vec<RebuildTask>,
            stage_idx: i64,
            path: String,
            slot_hint: usize,
            obj: &runtime::globals::ObjectState,
        ) {
            let runtime_slot = obj.runtime_slot_or(slot_hint);
            if needs_gfx_restore(obj) {
                out.push(RebuildTask {
                    stage_idx,
                    path: path.clone(),
                    runtime_slot,
                    obj_snapshot: obj.clone(),
                });
            }
            for (child_idx, child) in obj.runtime.child_objects.iter().enumerate() {
                collect_rebuild_tasks(
                    out,
                    stage_idx,
                    format!("{path}.child[{child_idx}]"),
                    child_idx,
                    child,
                );
            }
        }

        // C++ load reconstructs every C_elm_object recursively via load/restruct_type.
        // Rust's save stream does not contain runtime slots, so rebuild the stable
        // slot assignment before re-binding sprites. Top-level STAGE.OBJECT keeps
        // its index slot; MWND internal roots use the embedded 200000+ range;
        // OBJECT.CHILD descendants use the nested 100000+ range.
        let stage_form_ids: Vec<u32> = self.ctx.globals.stage_forms.keys().copied().collect();
        for form_id in &stage_form_ids {
            let Some(stage_form) = self.ctx.globals.stage_forms.get_mut(form_id) else {
                continue;
            };
            let mut stage_ids: Vec<i64> = stage_form
                .object_lists
                .keys()
                .chain(stage_form.mwnd_lists.keys())
                .copied()
                .collect();
            stage_ids.sort_unstable();
            stage_ids.dedup();

            for stage_idx in stage_ids {
                let mut next_nested = stage_form
                    .next_nested_object_slot
                    .get(&stage_idx)
                    .copied()
                    .unwrap_or(100000)
                    .max(100000);
                let mut next_embedded = stage_form
                    .next_embedded_object_slot
                    .get(&stage_idx)
                    .copied()
                    .unwrap_or(200000)
                    .max(200000);
                let existing_embedded = stage_form.embedded_object_slots.clone();
                let mut embedded_assignments: Vec<(String, usize)> = Vec::new();
                let mut alloc_embedded = |key: String| -> usize {
                    let full = format!("{stage_idx}:{key}");
                    if let Some(slot) = existing_embedded.get(&full).copied() {
                        return slot;
                    }
                    let slot = next_embedded;
                    next_embedded += 1;
                    embedded_assignments.push((full, slot));
                    slot
                };

                if let Some(objs) = stage_form.object_lists.get_mut(&stage_idx) {
                    for obj in objs.iter_mut() {
                        assign_child_slots_and_backend(obj, &mut next_nested);
                    }
                }
                if let Some(mwnds) = stage_form.mwnd_lists.get_mut(&stage_idx) {
                    for (mwnd_idx, m) in mwnds.iter_mut().enumerate() {
                        for (i, obj) in m.button_list.iter_mut().enumerate() {
                            if obj.nested_runtime_slot.is_none() {
                                obj.nested_runtime_slot = Some(alloc_embedded(format!(
                                    "mwnd_button_{stage_idx}_{mwnd_idx}_{i}"
                                )));
                            }
                            assign_child_slots_and_backend(obj, &mut next_nested);
                        }
                        for (i, obj) in m.face_list.iter_mut().enumerate() {
                            if obj.nested_runtime_slot.is_none() {
                                obj.nested_runtime_slot = Some(alloc_embedded(format!(
                                    "mwnd_face_{stage_idx}_{mwnd_idx}_{i}"
                                )));
                            }
                            assign_child_slots_and_backend(obj, &mut next_nested);
                        }
                        for (i, obj) in m.object_list.iter_mut().enumerate() {
                            if obj.nested_runtime_slot.is_none() {
                                obj.nested_runtime_slot = Some(alloc_embedded(format!(
                                    "mwnd_object_{stage_idx}_{mwnd_idx}_{i}"
                                )));
                            }
                            assign_child_slots_and_backend(obj, &mut next_nested);
                        }
                    }
                }

                for (key, slot) in embedded_assignments {
                    stage_form.embedded_object_slots.entry(key).or_insert(slot);
                }
                stage_form
                    .next_nested_object_slot
                    .insert(stage_idx, next_nested);
                stage_form
                    .next_embedded_object_slot
                    .insert(stage_idx, next_embedded);
            }
        }

        let mut tasks: Vec<RebuildTask> = Vec::new();
        for form_id in &stage_form_ids {
            let Some(stage_form) = self.ctx.globals.stage_forms.get(form_id) else {
                continue;
            };
            let mut stage_ids: Vec<i64> = stage_form
                .object_lists
                .keys()
                .chain(stage_form.mwnd_lists.keys())
                .copied()
                .collect();
            stage_ids.sort_unstable();
            stage_ids.dedup();
            for stage_idx in stage_ids {
                if let Some(objs) = stage_form.object_lists.get(&stage_idx) {
                    for (obj_idx, obj) in objs.iter().enumerate() {
                        collect_rebuild_tasks(
                            &mut tasks,
                            stage_idx,
                            format!("stage[{stage_idx}].object[{obj_idx}]"),
                            obj_idx,
                            obj,
                        );
                    }
                }
                if let Some(mwnds) = stage_form.mwnd_lists.get(&stage_idx) {
                    for (mwnd_idx, m) in mwnds.iter().enumerate() {
                        for (i, obj) in m.button_list.iter().enumerate() {
                            collect_rebuild_tasks(
                                &mut tasks,
                                stage_idx,
                                format!("stage[{stage_idx}].mwnd[{mwnd_idx}].button[{i}]"),
                                i,
                                obj,
                            );
                        }
                        for (i, obj) in m.face_list.iter().enumerate() {
                            collect_rebuild_tasks(
                                &mut tasks,
                                stage_idx,
                                format!("stage[{stage_idx}].mwnd[{mwnd_idx}].face[{i}]"),
                                i,
                                obj,
                            );
                        }
                        for (i, obj) in m.object_list.iter().enumerate() {
                            collect_rebuild_tasks(
                                &mut tasks,
                                stage_idx,
                                format!("stage[{stage_idx}].mwnd[{mwnd_idx}].object[{i}]"),
                                i,
                                obj,
                            );
                        }
                    }
                }
            }
        }

        let mut unsupported_count = 0usize;
        for form_id in &stage_form_ids {
            let Some(stage_form) = self.ctx.globals.stage_forms.get(form_id) else {
                continue;
            };
            for (_stage_idx, objs) in &stage_form.object_lists {
                for obj in objs {
                    if obj.used && obj.object_type != 0 && !needs_gfx_restore(obj) {
                        unsupported_count += 1;
                    }
                }
            }
        }
        if unsupported_count > 0 {
            log::warn!(
                "[SG_SAVELOAD] {unsupported_count} loaded top-level object(s) have type-specific backends whose runtime sprite IDs are not reconstructed by the Gfx restore path"
            );
        }

        for task in tasks {
            if let Err(err) = self.ctx.gfx.restore_gfx_object_from_globals(
                &mut self.ctx.images,
                &mut self.ctx.layers,
                task.stage_idx,
                task.runtime_slot as i64,
                &task.obj_snapshot,
            ) {
                log::warn!(
                    "[SG_SAVELOAD] restore_gfx_object_from_globals path={} slot={} file={:?} failed: {err:#}",
                    task.path,
                    task.runtime_slot,
                    task.obj_snapshot.file_name
                );
            }
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
        self.trace_cf_branch_farcall(
            self.stream.get_prg_cntr(),
            scene_name,
            z_no,
            ret_form,
            ex_call_proc,
            scratch_source_args,
        );
        if (scene_name == "sys20_adv00" && matches!(z_no, 10 | 13 | 17))
            || (scene_name == "sys20_adv01" && z_no == 0)
        {
            let args_dbg = scratch_source_args
                .iter()
                .map(|v| format!("{v:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            self.sg_cgm_coord_trace(format!(
                "farcall target={} z={} ret_form={} ex_call_proc={} argc={} args=[{}]",
                scene_name,
                z_no,
                ret_form,
                ex_call_proc,
                scratch_source_args.len(),
                args_dbg
            ));
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
            user_props: self.enter_cross_scene_user_prop_scope(),
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
        self.restore_cross_scene_user_prop_scope(saved.user_props);
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
        if self.cf_branch_trace_interesting_line() {
            self.sg_cf_branch_trace(
                self.stream.get_prg_cntr(),
                format!("kind=RETURN_RESTORED ret_form={} args={:?}", saved.ret_form, args),
            );
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
            self.ctx.request_proc_boundary(runtime::ProcKind::Script);
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
