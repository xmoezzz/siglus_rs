//! Scene VM

use anyhow::{anyhow, bail, Result};
use std::collections::BTreeMap;

use crate::elm_code;
use crate::runtime::{self, constants, CommandContext, Value};
use crate::runtime::globals::{ObjectFrameActionState, PendingButtonAction, PendingFrameActionFinish};
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
    user_props: Vec<CallProp>,
    int_args: Vec<i32>,
    str_args: Vec<String>,
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
    user_props: BTreeMap<u16, Value>,
    current_scene_no: Option<usize>,
    current_scene_name: Option<String>,
    current_line_no: i32,
    ret_form: i32,
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
    user_props: BTreeMap<u16, Value>,
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
    user_props: BTreeMap<u16, Value>,
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

    user_cmd_names: std::collections::HashMap<u32, String>,
    call_cmd_names: std::collections::HashMap<u32, String>,
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

    pub fn new(stream: SceneStream<'a>, ctx: CommandContext) -> Self {
        let cfg = VmConfig::from_env();
        let user_cmd_names = stream.scn_cmd_name_map.clone();
        let base_call = CallFrame {
            return_pc: 0,
            ret_form: cfg.fm_void,
            user_props: Vec::new(),
            int_args: Vec::new(),
            str_args: Vec::new(),
        };
        Self {
            cfg,
            stream,
            ctx,
            int_stack: Vec::new(),
            str_stack: Vec::new(),
            element_points: Vec::new(),
            call_stack: vec![base_call],
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
            user_cmd_names,
            call_cmd_names: std::collections::HashMap::new(),
        }
    }

    pub fn with_config(cfg: VmConfig, stream: SceneStream<'a>, ctx: CommandContext) -> Self {
        let user_cmd_names = stream.scn_cmd_name_map.clone();
        let base_call = CallFrame {
            return_pc: 0,
            ret_form: cfg.fm_void,
            user_props: Vec::new(),
            int_args: Vec::new(),
            str_args: Vec::new(),
        };
        Self {
            cfg,
            stream,
            ctx,
            int_stack: Vec::new(),
            str_stack: Vec::new(),
            element_points: Vec::new(),
            call_stack: vec![base_call],
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
            user_cmd_names,
            call_cmd_names: std::collections::HashMap::new(),
        }
    }

    pub fn is_blocked(&mut self) -> bool {
        self.ctx.wait_poll()
    }

    pub fn is_halted(&self) -> bool {
        self.halted
    }

    pub fn current_scene_name(&self) -> Option<&str> {
        self.current_scene_name.as_deref()
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
        expected_return_pc: Option<usize>,
        call_args: &[Value],
    ) -> Result<bool> {
        let base_depth = self.call_stack.len();
        if let Some(caller) = self.call_stack.last_mut() {
            caller.return_pc = return_pc;
            caller.ret_form = self.cfg.fm_void;
        }
        for arg in call_args {
            self.push_call_arg_value(arg);
        }
        let (int_args, str_args) = self.split_call_args(call_args);
        self.call_stack.push(CallFrame {
            return_pc: 0,
            ret_form: self.cfg.fm_void,
            user_props: Vec::new(),
            int_args,
            str_args,
        });
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

        let mut budget: u64 = 20000;
        while budget > 0 {
            let running = self.step_inner(false)?;
            if self.halted || !running {
                break;
            }
            if self.call_stack.len() == base_depth {
                match expected_return_pc {
                    Some(pc) if self.stream.get_prg_cntr() == pc => break,
                    None => break,
                    _ => {}
                }
            }
            budget -= 1;
        }
        Ok(true)
    }

    fn run_scene_user_cmd_inline(
        &mut self,
        scn_name: Option<&str>,
        cmd_name: &str,
        call_args: &[Value],
    ) -> Result<bool> {
        let current_scene_no = self.current_scene_no;
        let target_scene_no = match scn_name {
            Some(name) if !name.is_empty() => {
                let scene_pck_path = find_scene_pck_in_project(&self.ctx.project_dir)?;
                let opt = ScenePckDecodeOptions::from_project_dir(&self.ctx.project_dir)?;
                let pck = ScenePck::load_and_rebuild(&scene_pck_path, &opt)?;
                pck.find_scene_no(name).or(current_scene_no)
            }
            _ => current_scene_no,
        };
        let Some(target_scene_no) = target_scene_no else {
            return Ok(false);
        };

        // Original C_tnm_scene_lexer::get_user_cmd_no searches pack-level include
        // commands first, then scene-local commands with an inc_cmd_cnt offset.
        let scene_pck_path = find_scene_pck_in_project(&self.ctx.project_dir)?;
        let opt = ScenePckDecodeOptions::from_project_dir(&self.ctx.project_dir)?;
        let pck = ScenePck::load_and_rebuild(&scene_pck_path, &opt)?;

        if let Some(inc_cmd_no) = pck.find_inc_cmd_no(cmd_name) {
            let Some(inc_cmd) = pck.inc_cmds.get(inc_cmd_no).copied() else {
                bail!("scene_pck: inc command index out of range: {}", inc_cmd_no);
            };
            if inc_cmd.scn_no < 0 || inc_cmd.offset < 0 {
                bail!(
                    "scene_pck: invalid inc command target: cmd={} scn_no={} offset={}",
                    cmd_name,
                    inc_cmd.scn_no,
                    inc_cmd.offset
                );
            }
            return self.run_scene_user_cmd_inline_at_scene_offset(
                &pck,
                inc_cmd.scn_no as usize,
                cmd_name,
                inc_cmd.offset as usize,
                call_args,
                false,
            );
        }

        if Some(target_scene_no) == self.current_scene_no {
            let cmd_no = match self.user_cmd_names.iter().find_map(|(no, name)| {
                if name.eq_ignore_ascii_case(cmd_name) { Some(*no as usize) } else { None }
            }) {
                Some(v) => v,
                None => {
                    if std::env::var_os("SIGLUS_TRACE_FRAME_ACTION_CALL").is_some() {
                        eprintln!(
                            "[SG_FRAME_ACTION_CALL] user command not found: cmd={} scene={:?} scn_name={:?}",
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
                Some(return_pc),
                call_args,
            );
        }

        let chunk = pck.scn_data_slice(target_scene_no)?;
        let chunk_leaked: &'static [u8] = Box::leak(chunk.to_vec().into_boxed_slice());
        let target_stream: SceneStream<'a> = SceneStream::new(chunk_leaked)?;
        let cmd_no = match target_stream.scn_cmd_name_map.iter().find_map(|(no, name)| {
            if name.eq_ignore_ascii_case(cmd_name) { Some(*no as usize) } else { None }
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
        self.run_scene_user_cmd_inline_at_scene_offset(
            &pck,
            target_scene_no,
            cmd_name,
            offset,
            call_args,
            false,
        )
    }

    fn run_current_scene_user_cmd_inline(
        &mut self,
        cmd_name: &str,
        call_args: &[Value],
    ) -> Result<bool> {
        self.run_scene_user_cmd_inline(None, cmd_name, call_args)
    }

    fn run_scene_user_cmd_inline_at_scene_offset(
        &mut self,
        pck: &ScenePck,
        target_scene_no: usize,
        cmd_name: &str,
        target_offset: usize,
        call_args: &[Value],
        preserve_return_pc: bool,
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
        let saved_user_cmd_names = std::mem::replace(&mut self.user_cmd_names, target_user_cmd_names);
        let saved_call_cmd_names = std::mem::replace(&mut self.call_cmd_names, target_call_cmd_names);
        let saved_current_scene_no = self.current_scene_no;
        let saved_current_scene_name = self.current_scene_name.clone();
        let saved_current_line_no = self.current_line_no;
        let saved_ctx_scene_no = self.ctx.current_scene_no;
        let saved_ctx_scene_name = self.ctx.current_scene_name.clone();
        let saved_ctx_line_no = self.ctx.current_line_no;

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
            call_args,
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
                frame_action_chain.push(crate::runtime::forms::codes::elm_value::OBJECT_FRAME_ACTION_CH);
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
            if child.used {
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


    fn object_from_frame_action_chain_mut<'b>(
        objects: &'b mut [crate::runtime::globals::ObjectState],
        object_chain: &[i32],
        elm_array: i32,
    ) -> Option<&'b mut crate::runtime::globals::ObjectState> {
        if object_chain.len() < 6 || object_chain[1] != elm_array || object_chain[4] != elm_array {
            return None;
        }
        let mut obj = objects.get_mut(object_chain[5].max(0) as usize)?;
        let mut pos = 6usize;
        while pos + 2 < object_chain.len() {
            let op = object_chain[pos];
            if op != crate::runtime::forms::codes::elm_value::OBJECT_CHILD {
                break;
            }
            if object_chain[pos + 1] != elm_array {
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
        for st in self.ctx.globals.stage_forms.values_mut() {
            let Some(objects) = st.object_lists.get_mut(&stage_idx) else {
                continue;
            };
            let Some(obj) = Self::object_from_frame_action_chain_mut(objects, chain, elm_array) else {
                continue;
            };
            if let Some(idx) = item.ch_idx {
                return obj.frame_action_ch.get_mut(idx).map(f);
            }
            return Some(f(&mut obj.frame_action));
        }
        None
    }

    fn begin_frame_action_finish(&mut self, item: &FrameActionWork) -> Option<(String, Vec<Value>)> {
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
            fa.end_flag = false;
            fa.args.clear();
            fa.counter.reset();
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

    fn runtime_slot_from_object_chain(
        &mut self,
        stage_idx: i64,
        fallback_obj_idx: usize,
        chain: &[i32],
    ) -> usize {
        let top_idx = chain
            .get(5)
            .copied()
            .unwrap_or(fallback_obj_idx as i32)
            .max(0) as usize;
        let stage_form = self.ctx.ids.form_global_stage;
        let elm_array = if self.ctx.ids.elm_array != 0 {
            self.ctx.ids.elm_array
        } else {
            crate::runtime::forms::codes::ELM_ARRAY
        };
        let object_child = crate::runtime::forms::codes::elm_value::OBJECT_CHILD;

        let Some(st) = self.ctx.globals.stage_forms.get_mut(&stage_form) else {
            return top_idx;
        };
        let next_slot = st.next_nested_object_slot.entry(stage_idx).or_insert(100000);
        let Some(list) = st.object_lists.get_mut(&stage_idx) else {
            return top_idx;
        };
        if top_idx >= list.len() {
            return top_idx;
        }

        let mut slot = list[top_idx].runtime_slot_or(top_idx);
        let mut obj = &mut list[top_idx];
        let mut pos = 6usize;
        while pos + 2 < chain.len() {
            if chain[pos] == object_child && (chain[pos + 1] == elm_array || chain[pos + 1] == crate::runtime::forms::codes::ELM_ARRAY) {
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

    fn set_frame_action_current_object(&mut self, item: &FrameActionWork) -> (Option<(i64, usize)>, Option<Vec<i32>>) {
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

    fn frame_action_work_from_pending_finish(&self, pending: &PendingFrameActionFinish) -> FrameActionWork {
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
                && pending.frame_action_chain[base] == crate::runtime::forms::codes::elm_value::OBJECT_FRAME_ACTION_CH
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
            pending.frame_action_chain.first().copied().map(|v| v as u32)
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
        let final_count = if pending.end_time == -1 { 0 } else { pending.end_time };
        let saved_state = self.with_frame_action_mut(&item, |fa| {
            let saved = fa.clone();
            fa.scn_name.clear();
            fa.cmd_name.clear();
            fa.args = pending.args.clone();
            fa.end_time = pending.end_time;
            fa.counter.set_count(final_count);
            fa.end_flag = true;
            saved
        });

        let call_args = Self::make_frame_action_call_args(
            item.frame_action_chain.as_ref(),
            item.object_chain.as_ref(),
            &pending.args,
        );
        let (prev_target, prev_chain) = self.set_frame_action_current_object(&item);
        let result = self.run_scene_user_cmd_inline(Some(&pending.scn_name), &pending.cmd_name, &call_args);
        self.restore_frame_action_current_object(prev_target, prev_chain);

        if let Some(saved) = saved_state {
            let _ = self.with_frame_action_mut(&item, |fa| {
                *fa = saved;
            });
        }
        result.map(|_| ())
    }

    fn run_pending_button_action(&mut self, action: PendingButtonAction) -> Result<()> {
        if action.scn_name.is_empty() {
            return Ok(());
        }
        if !action.cmd_name.is_empty() {
            let _ = self.run_scene_user_cmd_inline(Some(&action.scn_name), &action.cmd_name, &[])?;
        } else if action.z_no >= 0 {
            self.farcall_scene_name(&action.scn_name, action.z_no as i32, self.cfg.fm_void)?;
        }
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
        let trace = std::env::var_os("SG_TICK_TRACE").is_some();
        if trace {
            eprintln!(
                "[SG_TICK_TRACE] tick_frame start blocked={} halted={} scene={:?}",
                self.is_blocked(),
                self.halted,
                self.current_scene_name()
            );
        }
        self.ctx.tick_frame();
        if trace {
            eprintln!(
                "[SG_TICK_TRACE] after ctx.tick_frame blocked={} halted={}",
                self.is_blocked(),
                self.halted
            );
        }
        self.drain_pending_button_actions()?;
        if self.ctx.globals.script.frame_action_time_stop_flag && trace {
            eprintln!(
                "[SG_TICK_TRACE] frame_action_time_stop_flag set; executing callbacks with frozen frame-action time"
            );
        }
        let mut work: Vec<FrameActionWork> = Vec::new();
        for (form_id, fa) in &self.ctx.globals.frame_actions {
            if !fa.cmd_name.is_empty() {
                work.push(FrameActionWork {
                    stage_idx: -1,
                    obj_idx: usize::MAX,
                    ch_idx: None,
                    global_form_id: Some(*form_id),
                    object_chain: None,
                    frame_action_chain: Some(vec![*form_id as i32]),
                    scn_name: fa.scn_name.clone(),
                    cmd_name: fa.cmd_name.clone(),
                    args: fa.args.clone(),
                    count: fa.counter.get_count(),
                    end_time: fa.end_time,
                });
            }
        }
        for (form_id, list) in &self.ctx.globals.frame_action_lists {
            for (idx, fa) in list.iter().enumerate() {
                if !fa.cmd_name.is_empty() {
                    work.push(FrameActionWork {
                        stage_idx: -1,
                        obj_idx: usize::MAX,
                        ch_idx: Some(idx),
                        global_form_id: Some(*form_id),
                        object_chain: None,
                        frame_action_chain: Some(vec![*form_id as i32, crate::runtime::forms::codes::ELM_ARRAY, idx as i32]),
                        scn_name: fa.scn_name.clone(),
                        cmd_name: fa.cmd_name.clone(),
                        args: fa.args.clone(),
                        count: fa.counter.get_count(),
                        end_time: fa.end_time,
                    });
                }
            }
        }
        for st in self.ctx.globals.stage_forms.values() {
            for (stage_idx, objs) in &st.object_lists {
                for (obj_idx, obj) in objs.iter().enumerate() {
                    let object_chain = vec![
                        self.ctx.ids.form_global_stage as i32,
                        self.ctx.ids.elm_array,
                        *stage_idx as i32,
                        self.ctx.ids.stage_elm_object,
                        self.ctx.ids.elm_array,
                        obj_idx as i32,
                    ];
                    Self::collect_object_frame_action_work_recursive(
                        obj,
                        *stage_idx,
                        obj_idx,
                        object_chain,
                        &mut work,
                    );
                }
            }
        }
        if trace {
            eprintln!("[SG_TICK_TRACE] frame_action work items={}", work.len());
        }
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
            if let Some((finish_cmd_name, finish_args)) = self.begin_frame_action_finish(&item) {
                let finish_call_args = Self::make_frame_action_call_args(
                    item.frame_action_chain.as_ref(),
                    item.object_chain.as_ref(),
                    &finish_args,
                );
                let (prev_target, prev_chain) = self.set_frame_action_current_object(&item);
                let _ = self.run_scene_user_cmd_inline(Some(&item.scn_name), &finish_cmd_name, &finish_call_args)?;
                self.restore_frame_action_current_object(prev_target, prev_chain);
                self.end_frame_action_finish(&item);
                continue;
            }

            let call_args = Self::make_frame_action_call_args(
                item.frame_action_chain.as_ref(),
                item.object_chain.as_ref(),
                &item.args,
            );
            let (prev_target, prev_chain) = self.set_frame_action_current_object(&item);
            let _ = self.run_scene_user_cmd_inline(Some(&item.scn_name), &item.cmd_name, &call_args)?;
            self.restore_frame_action_current_object(prev_target, prev_chain);
        }
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

    fn step_inner(&mut self, respect_wait: bool) -> Result<bool> {
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
            self.take_ctx_return(rf);
        }

        if self.cfg.max_steps > 0 && self.steps >= self.cfg.max_steps {
            bail!(
                "VM reached SIGLUS_VM_MAX_STEPS={} (possible infinite loop)",
                self.cfg.max_steps
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

        match opcode {
            CD_NL => {
                let line_no = self.stream.pop_i32()?;
                self.current_line_no = line_no;
                self.ctx.current_line_no = line_no as i64;
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
            }
            CD_COPY_ELM => {
                self.exec_copy_element()?;
            }

            CD_PROPERTY => {
                let elm = self.pop_element()?;
                self.exec_property(elm)?;
            }
            CD_DEC_PROP => {
                let form_code = self.stream.pop_i32()?;
                let prop_id = self.stream.pop_i32()?;

                let value = if form_code == self.cfg.fm_int {
                    CallPropValue::Int(0)
                } else if form_code == self.cfg.fm_str {
                    CallPropValue::Str(String::new())
                } else if form_code == self.cfg.fm_intlist {
                    let size = self.pop_int()?.max(0) as usize;
                    CallPropValue::IntList(vec![0; size])
                } else if form_code == self.cfg.fm_strlist {
                    let size = self.pop_int()?.max(0) as usize;
                    CallPropValue::StrList(vec![String::new(); size])
                } else {
                    CallPropValue::Element(Vec::new())
                };

                let frame = self
                    .call_stack
                    .last_mut()
                    .ok_or_else(|| anyhow!("call stack underflow"))?;
                frame.user_props.push(CallProp {
                    prop_id,
                    form: form_code,
                    value,
                });
            }
            CD_ARG => {
                // Expand stack arguments into the current call's declared properties
                // (tnm_expand_arg_into_call_flag).
                let forms: Vec<i32> = {
                    let frame = self
                        .call_stack
                        .last()
                        .ok_or_else(|| anyhow!("call stack underflow"))?;
                    frame.user_props.iter().map(|p| p.form).collect()
                };

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
                    prop.value = v;
                }
            }

            CD_GOTO => {
                let label_no = self.stream.pop_i32()?;
                self.stream.jump_to_label(label_no.max(0) as usize)?;
            }
            CD_GOTO_TRUE => {
                let label_no = self.stream.pop_i32()?;
                let cond = self.pop_int()?;
                if cond != 0 {
                    self.stream.jump_to_label(label_no.max(0) as usize)?;
                }
            }
            CD_GOTO_FALSE => {
                let label_no = self.stream.pop_i32()?;
                let cond = self.pop_int()?;
                if cond == 0 {
                    self.stream.jump_to_label(label_no.max(0) as usize)?;
                }
            }
            CD_GOSUB => {
                let label_no = self.stream.pop_i32()?;
                let _args = self.pop_arg_list()?;

                // Save return info on the caller frame .
                let caller = self
                    .call_stack
                    .last_mut()
                    .ok_or_else(|| anyhow!("call stack underflow"))?;
                caller.return_pc = self.stream.get_prg_cntr();
                caller.ret_form = self.cfg.fm_int;

                // Enter callee context.
                let (int_args, str_args) = self.split_call_args(&_args);
                self.call_stack.push(CallFrame {
                    return_pc: 0,
                    ret_form: self.cfg.fm_void,
                    user_props: Vec::new(),
                    int_args,
                    str_args,
                });

                self.stream.jump_to_label(label_no.max(0) as usize)?;
            }
            CD_GOSUBSTR => {
                let label_no = self.stream.pop_i32()?;
                let _args = self.pop_arg_list()?;

                let caller = self
                    .call_stack
                    .last_mut()
                    .ok_or_else(|| anyhow!("call stack underflow"))?;
                caller.return_pc = self.stream.get_prg_cntr();
                caller.ret_form = self.cfg.fm_str;

                let (int_args, str_args) = self.split_call_args(&_args);
                self.call_stack.push(CallFrame {
                    return_pc: 0,
                    ret_form: self.cfg.fm_void,
                    user_props: Vec::new(),
                    int_args,
                    str_args,
                });

                self.stream.jump_to_label(label_no.max(0) as usize)?;
            }
            CD_RETURN => {
                let args = self.pop_arg_list()?;
                if self.call_stack.len() == 1 && self.return_from_scene(args.clone())? {
                    return Ok(true);
                }
                self.exec_return(args)?;
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
                self.exec_command(elm, arg_list_id, ret_form, &mut args)?;
            }
            CD_TEXT => {
                let _rf_flag_no = self.stream.pop_i32()?;
                let text = self.pop_str()?;
                self.ctx.ui.set_message(text);
                self.ctx.ui.begin_wait_message();
                self.ctx.wait.wait_key();
            }
            CD_NAME => {
                let name = self.pop_str()?;
                self.ctx.ui.set_name(name);
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
                println!("VM hit CD_NONE at pc=0x{:x}; stopping", pc_before);
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
    }

    fn pop_int(&mut self) -> Result<i32> {
        self.int_stack
            .pop()
            .ok_or_else(|| anyhow!("int stack underflow"))
    }

    fn peek_int(&self) -> Result<i32> {
        self.int_stack
            .last()
            .copied()
            .ok_or_else(|| anyhow!("int stack underflow"))
    }

    fn push_str(&mut self, s: String) {
        self.str_stack.push(s);
    }

    fn pop_str(&mut self) -> Result<String> {
        self.str_stack
            .pop()
            .ok_or_else(|| anyhow!("str stack underflow"))
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
    }

    fn pop_element(&mut self) -> Result<Vec<i32>> {
        let start = self
            .element_points
            .pop()
            .ok_or_else(|| anyhow!("element stack underflow (missing ELM_POINT)"))?;
        if start > self.int_stack.len() {
            bail!(
                "invalid element point start={start} len={}",
                self.int_stack.len()
            );
        }
        let elm = self.int_stack[start..].to_vec();
        self.int_stack.truncate(start);
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

    fn default_value_like(&self, v: &Value) -> Value {
        match v {
            Value::NamedArg { value, .. } => self.default_value_like(value),
            Value::Int(_) => Value::Int(0),
            Value::Str(_) => Value::Str(String::new()),
            Value::Element(_) => Value::Element(Vec::new()),
            Value::List(_) => Value::List(Vec::new()),
        }
    }
    fn split_call_args(&self, args: &[Value]) -> (Vec<i32>, Vec<String>) {
        let mut int_args = Vec::new();
        let mut str_args = Vec::new();
        for v in args {
            match v {
                Value::NamedArg { value, .. } => match value.as_ref() {
                    Value::Int(n) => int_args.push(*n as i32),
                    Value::Str(s) => str_args.push(s.clone()),
                    _ => {}
                },
                Value::Int(n) => int_args.push(*n as i32),
                Value::Str(s) => str_args.push(s.clone()),
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

    fn exec_call_property(&mut self, elm: &[i32]) -> Result<bool> {
        use crate::runtime::forms::codes::{FM_CALL, FM_CALLLIST};

        if elm.is_empty() {
            return Ok(false);
        }
        let head = elm[0];
        if head != FM_CALL && head != FM_CALLLIST {
            return Ok(false);
        }

        let (frame_idx, tail): (usize, &[i32]) = if head == FM_CALLLIST {
            if elm.len() < 3 || !self.call_array_marker(elm[1]) {
                self.push_int(0);
                return Ok(true);
            }
            let Some(frame_idx) = self.resolve_call_frame_index(elm[2]) else {
                self.push_int(0);
                return Ok(true);
            };
            (frame_idx, &elm[3..])
        } else {
            let Some(frame_idx) = self.resolve_call_frame_index(0) else {
                self.push_int(0);
                return Ok(true);
            };
            (frame_idx, &elm[1..])
        };

        if tail.is_empty() {
            self.push_int(0);
            return Ok(true);
        }

        let frame = &self.call_stack[frame_idx];
        match tail[0] {
            0 => {
                if tail.len() >= 3 && self.call_array_marker(tail[1]) {
                    let idx = tail[2].max(0) as usize;
                    self.push_int(frame.int_args.get(idx).copied().unwrap_or(0));
                } else {
                    self.push_int(frame.int_args.len() as i32);
                }
                Ok(true)
            }
            1 => {
                if tail.len() >= 3 && self.call_array_marker(tail[1]) {
                    let idx = tail[2].max(0) as usize;
                    self.push_str(frame.str_args.get(idx).cloned().unwrap_or_default());
                } else {
                    self.push_int(frame.str_args.len() as i32);
                }
                Ok(true)
            }
            _ => {
                self.push_int(0);
                Ok(true)
            }
        }
    }

    fn exec_call_assign(&mut self, elm: &[i32], rhs: Value) -> Result<bool> {
        use crate::runtime::forms::codes::{FM_CALL, FM_CALLLIST};

        if elm.is_empty() {
            return Ok(false);
        }
        let head = elm[0];
        if head != FM_CALL && head != FM_CALLLIST {
            return Ok(false);
        }

        let (frame_idx, tail): (usize, &[i32]) = if head == FM_CALLLIST {
            if elm.len() < 3 || !self.call_array_marker(elm[1]) {
                return Ok(true);
            }
            let Some(frame_idx) = self.resolve_call_frame_index(elm[2]) else {
                return Ok(true);
            };
            (frame_idx, &elm[3..])
        } else {
            let Some(frame_idx) = self.resolve_call_frame_index(0) else {
                return Ok(true);
            };
            (frame_idx, &elm[1..])
        };

        if tail.len() < 3 || !self.call_array_marker(tail[1]) {
            return Ok(true);
        }
        let idx = tail[2].max(0) as usize;
        let frame = &mut self.call_stack[frame_idx];
        match (tail[0], rhs) {
            (0, Value::Int(n)) => {
                if frame.int_args.len() <= idx {
                    frame.int_args.resize(idx + 1, 0);
                }
                frame.int_args[idx] = n as i32;
            }
            (1, Value::Str(s)) => {
                if frame.str_args.len() <= idx {
                    frame.str_args.resize_with(idx + 1, String::new);
                }
                frame.str_args[idx] = s;
            }
            _ => {}
        }
        Ok(true)
    }

    fn exec_call_command(
        &mut self,
        elm: &[i32],
        al_id: i32,
        ret_form: i32,
        args: &[Value],
    ) -> Result<bool> {
        use crate::runtime::forms::codes::{FM_CALL, FM_CALLLIST};

        if elm.is_empty() {
            return Ok(false);
        }
        let head = elm[0];
        if head != FM_CALL && head != FM_CALLLIST {
            return Ok(false);
        }

        let (frame_idx, tail): (usize, &[i32]) = if head == FM_CALLLIST {
            if elm.len() < 3 || !self.call_array_marker(elm[1]) {
                self.push_default_for_ret(ret_form);
                return Ok(true);
            }
            let Some(frame_idx) = self.resolve_call_frame_index(elm[2]) else {
                self.push_default_for_ret(ret_form);
                return Ok(true);
            };
            (frame_idx, &elm[3..])
        } else {
            let Some(frame_idx) = self.resolve_call_frame_index(0) else {
                self.push_default_for_ret(ret_form);
                return Ok(true);
            };
            (frame_idx, &elm[1..])
        };

        if tail.is_empty() {
            self.push_default_for_ret(ret_form);
            return Ok(true);
        }

        let params: &[Value] = args;
        let has_array = tail.len() >= 3 && self.call_array_marker(tail[1]);
        let idx = if has_array {
            Some(tail[2].max(0) as usize)
        } else {
            None
        };
        match tail[0] {
            0 => {
                if let Some(idx) = idx {
                    if al_id == 1 {
                        let rhs = params.first().and_then(|v| v.as_i64()).unwrap_or(0) as i32;
                        let frame = &mut self.call_stack[frame_idx];
                        if frame.int_args.len() <= idx {
                            frame.int_args.resize(idx + 1, 0);
                        }
                        frame.int_args[idx] = rhs;
                        self.push_default_for_ret(ret_form);
                    } else {
                        let v = {
                            let frame = &self.call_stack[frame_idx];
                            frame.int_args.get(idx).copied().unwrap_or(0)
                        };
                        self.push_int(v);
                    }
                } else {
                    let len = {
                        let frame = &self.call_stack[frame_idx];
                        frame.int_args.len() as i32
                    };
                    self.push_int(len);
                }
                Ok(true)
            }
            1 => {
                if let Some(idx) = idx {
                    if al_id == 1 {
                        let rhs = params
                            .first()
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let frame = &mut self.call_stack[frame_idx];
                        if frame.str_args.len() <= idx {
                            frame.str_args.resize_with(idx + 1, String::new);
                        }
                        frame.str_args[idx] = rhs;
                        self.push_default_for_ret(ret_form);
                    } else {
                        let v = {
                            let frame = &self.call_stack[frame_idx];
                            frame.str_args.get(idx).cloned().unwrap_or_default()
                        };
                        self.push_str(v);
                    }
                } else {
                    let len = {
                        let frame = &self.call_stack[frame_idx];
                        frame.str_args.len() as i32
                    };
                    self.push_int(len);
                }
                Ok(true)
            }
            _ => {
                self.push_default_for_ret(ret_form);
                Ok(true)
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
                    self.push_int(items.len() as i32);
                }
            }
        }
    }

    fn assign_user_prop(&mut self, prop_id: u16, array_idx: Option<usize>, rhs: Value) {
        if let Some(i) = array_idx {
            let default_like = self.default_value_like(&rhs);
            let entry = self
                .user_props
                .entry(prop_id)
                .or_insert_with(|| Value::List(Vec::new()));
            match entry {
                Value::List(items) => {
                    if items.len() <= i {
                        items.resize(i + 1, default_like);
                    }
                    items[i] = rhs;
                }
                other => {
                    let mut items = vec![default_like; i + 1];
                    items[i] = rhs;
                    *other = Value::List(items);
                }
            }
        } else {
            self.user_props.insert(prop_id, rhs);
        }
    }

    fn exec_copy_element(&mut self) -> Result<()> {
        let start = *self
            .element_points
            .last()
            .ok_or_else(|| anyhow!("COPY_ELM without a prior ELM_POINT"))?;
        if start > self.int_stack.len() {
            bail!(
                "invalid element point start={start} len={}",
                self.int_stack.len()
            );
        }
        let slice = self.int_stack[start..].to_vec();
        self.element_points.push(self.int_stack.len());
        self.int_stack.extend_from_slice(&slice);
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

        // Some scripts may copy elements through CD_COPY with a non-int/str form.
        // We conservatively duplicate the last element chain.
        self.trace_unknown_form(form_code, "exec_copy");
        self.exec_copy_element()?;
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

    fn try_parent_slot_property(&mut self, elm: &[i32]) -> bool {
        if elm.len() != 3 || elm[1] != self.ctx.ids.elm_array || elm[2] <= 0 {
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
        if op < 0 {
            return false;
        }
        if op <= 187 {
            return true;
        }
        false
    }

    fn try_compact_object_chain(&self, elm: &[i32]) -> Option<Vec<i32>> {
        if elm.is_empty() {
            return None;
        }

        let op = elm[0];
        if !self.compact_object_op_allowed(op) {
            return None;
        }

        let elm_array = self.ctx.ids.elm_array;
        let stage_object = if self.ctx.ids.stage_elm_object != 0 {
            self.ctx.ids.stage_elm_object
        } else {
            crate::runtime::forms::codes::STAGE_ELM_OBJECT
        };

        let looks_like_full_stage_object = elm.len() >= 4
            && elm[1] == elm_array
            && elm[3] == stage_object;
        let looks_like_absolute_stage_alias_object = elm.len() >= 4
            && constants::is_stage_global_form(elm[0] as u32, self.ctx.ids.form_global_stage)
            && elm[1] == stage_object
            && elm[2] == elm_array;

        if looks_like_absolute_stage_alias_object {
            return None;
        }

        if !looks_like_full_stage_object {
            if let Some(prefix) = &self.ctx.globals.current_object_chain {
                let mut synthetic = prefix.clone();
                synthetic.extend_from_slice(elm);
                return Some(synthetic);
            }
        }

        if elm.len() < 3 || elm[2] != elm_array || elm[1] < 0 {
            return None;
        }

        let stage_idx = elm[1];
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
            return Some(synthetic);
        }

        if let Some(prefix) = &self.ctx.globals.current_object_chain {
            if prefix.len() >= 3 && prefix[2] == stage_idx {
                let mut synthetic = prefix.clone();
                synthetic.push(op);
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

    fn exec_property(&mut self, elm: Vec<i32>) -> Result<()> {
        if elm.is_empty() {
            self.push_int(0);
            return Ok(());
        }

        // Call-local properties (declared by CD_DEC_PROP / populated by CD_ARG).
        if self.exec_call_property(&elm)? {
            return Ok(());
        }

        let head = elm[0];
        let head_owner = elm_code::owner(head);
        if head_owner == elm_code::ELM_OWNER_CALL_PROP {
            let call_prop_id = elm_code::code(head) as usize;
            let array_idx = self.extract_array_index(&elm);
            let prop_value: CallPropValue = {
                let frame = self
                    .call_stack
                    .last()
                    .ok_or_else(|| anyhow!("call stack underflow"))?;
                match frame.user_props.get(call_prop_id) {
                    Some(p) => p.value.clone(),
                    None => {
                        self.push_int(0);
                        return Ok(());
                    }
                }
            };

            match prop_value {
                CallPropValue::Int(n) => self.push_int(n),
                CallPropValue::Str(s) => self.push_str(s),
                CallPropValue::Element(e) => self.push_element(e),
                CallPropValue::IntList(v) => {
                    if let Some(i) = array_idx {
                        let x = v.get(i).copied().unwrap_or(0);
                        self.push_int(x);
                    } else {
                        self.push_int(v.len() as i32);
                    }
                }
                CallPropValue::StrList(v) => {
                    if let Some(i) = array_idx {
                        let x = v.get(i).cloned().unwrap_or_default();
                        self.push_str(x);
                    } else {
                        self.push_int(v.len() as i32);
                    }
                }
            }
            return Ok(());
        }

        if head_owner == elm_code::ELM_OWNER_USER_PROP {
            let prop_id = elm_code::code(head);
            let array_idx = self.extract_array_index(&elm);
            let v = self
                .user_props
                .get(&prop_id)
                .cloned()
                .unwrap_or(Value::Int(0));
            self.push_property_value(v, array_idx);
            return Ok(());
        }

        if head_owner != elm_code::ELM_OWNER_FORM {
            bail!(
                "unsupported property owner {} for element {:?}",
                head_owner,
                elm
            );
        }

        if self.try_parent_slot_property(&elm) {
            return Ok(());
        }
        if let Some(synthetic) = self.try_compact_object_chain(&elm) {
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
            if let Some(v) = self.ctx.pop() {
                self.push_return_value_raw(v);
            } else {
                self.push_int(0);
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

        if !runtime::dispatch_form_code(&mut self.ctx, form_id, &args)? {
            self.ctx.vm_call = None;
            bail!("unhandled form property chain {:?}", elm);
        }

        self.ctx.vm_call = None;
        if let Some(v) = self.ctx.pop() {
            self.push_return_value_raw(v);
        } else {
            self.push_int(0);
        }

        Ok(())
    }

    fn exec_assign(&mut self, elm: Vec<i32>, al_id: i32, rhs: Value) -> Result<()> {
        if elm.is_empty() {
            return Ok(());
        }

        // Call-local property assignment.
        if self.exec_call_assign(&elm, rhs.clone())? {
            return Ok(());
        }

        let head = elm[0];
        let head_owner = elm_code::owner(head);
        if head_owner == elm_code::ELM_OWNER_CALL_PROP {
            let call_prop_id = elm_code::code(head) as usize;
            let array_idx = self.extract_array_index(&elm);
            let frame = self
                .call_stack
                .last_mut()
                .ok_or_else(|| anyhow!("call stack underflow"))?;
            let prop = match frame.user_props.get_mut(call_prop_id) {
                Some(p) => p,
                None => {
                    return Ok(());
                }
            };

            match (&mut prop.value, rhs) {
                (CallPropValue::Int(dst), Value::Int(n)) => *dst = n as i32,
                (CallPropValue::Str(dst), Value::Str(s)) => *dst = s,
                (CallPropValue::Element(dst), Value::Element(e)) => *dst = e,
                (CallPropValue::IntList(dst), Value::Int(n)) => {
                    if let Some(i) = array_idx {
                        if i < dst.len() {
                            dst[i] = n as i32;
                        }
                    }
                }
                (CallPropValue::StrList(dst), Value::Str(s)) => {
                    if let Some(i) = array_idx {
                        if i < dst.len() {
                            dst[i] = s;
                        }
                    }
                }
                _ => {}
            }
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

        if self.try_parent_slot_assign(&elm, &rhs) {
            return Ok(());
        }
        if let Some(synthetic) = self.try_compact_object_chain(&elm) {
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
            self.ctx.stack.clear();
            self.drain_pending_frame_action_finishes()?;
            return Ok(());
        }

        let form_id = self.canonical_runtime_form_id(head as u32);
        let args: Vec<Value> = vec![rhs];
        if std::env::var_os("SIGLUS_TRACE_VM_COMMANDS").is_some() {
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
        args: &[Value],
    ) -> Result<bool> {
        let cmd_no = elm_code::code(raw_head) as u32;
        if owner == elm_code::ELM_OWNER_USER_CMD {
            // Original USER_CMD elements call Scene.pck user commands, not the
            // engine named-command dispatcher.  The id space is inc commands
            // first, then scene-local commands with inc_cmd_cnt added.
            let inc_cmd_cnt = self.call_cmd_names.len() as u32;
            if cmd_no < inc_cmd_cnt {
                let Some(name) = self.call_cmd_names.get(&cmd_no).cloned() else {
                    return Ok(false);
                };
                return self.run_scene_user_cmd_inline(None, &name, args);
            }

            let local_cmd_no = cmd_no - inc_cmd_cnt;
            let Some(name) = self.user_cmd_names.get(&local_cmd_no).cloned() else {
                return Ok(false);
            };
            let offset = self.stream.scn_cmd_offset(local_cmd_no as usize)?;
            let return_pc = self.stream.get_prg_cntr();
            return self.run_user_cmd_inline_at_offset(
                &name,
                offset,
                return_pc,
                Some(return_pc),
                args,
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

        match owner {
            o if o == elm_code::ELM_OWNER_FORM => {
                let form_id = self.canonical_runtime_form_id(raw_head as u32) as i32;
                if self.exec_builtin_global_control(form_id, ret_form)? {
                    if ret_form != self.cfg.fm_void {
                        self.take_ctx_return(ret_form);
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

                if std::env::var_os("SIGLUS_TRACE_VM_COMMANDS").is_some() {
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

                if !runtime::dispatch_form_code(&mut self.ctx, form_id as u32, args)? {
                    self.ctx.vm_call = None;
                    bail!("unhandled form command chain {:?}", elm);
                }
                self.ctx.vm_call = None;
                self.drain_pending_frame_action_finishes()?;
            }
            o if o == elm_code::ELM_OWNER_USER_CMD || o == elm_code::ELM_OWNER_CALL_CMD => {
                if std::env::var_os("SIGLUS_TRACE_VM_COMMANDS").is_some() {
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

                if !self.dispatch_owner_named_command(owner, raw_head, args)? {
                    bail!("unhandled owner command chain {:?}", elm);
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

        self.take_ctx_return(ret_form);
        Ok(())
    }

    fn exec_return(&mut self, args: Vec<Value>) -> Result<()> {
        // Pop callee frame.
        if self.call_stack.pop().is_none() {
            return Ok(());
        }

        // Return info is stored on the caller frame .
        let caller = match self.call_stack.last_mut() {
            Some(f) => f,
            None => {
                // No caller: treat as end.
                return Ok(());
            }
        };

        let return_pc = caller.return_pc;
        let ret_form = caller.ret_form;
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

        Ok(())
    }

    fn scene_base_call(&self) -> CallFrame {
        CallFrame {
            return_pc: 0,
            ret_form: self.cfg.fm_void,
            user_props: Vec::new(),
            int_args: Vec::new(),
            str_args: Vec::new(),
        }
    }

    fn load_scene_stream(
        &mut self,
        scene_name: &str,
        z_no: i32,
    ) -> Result<(SceneStream<'a>, usize)> {
        let scene_pck_path = find_scene_pck_in_project(&self.ctx.project_dir)?;
        let opt = ScenePckDecodeOptions::from_project_dir(&self.ctx.project_dir)?;
        let pck = ScenePck::load_and_rebuild(&scene_pck_path, &opt)?;
        let scene_no = pck
            .find_scene_no(scene_name)
            .ok_or_else(|| anyhow!("scene not found: {}", scene_name))?;
        let chunk = pck.scn_data_slice(scene_no)?;
        let chunk_leaked: &'static [u8] = Box::leak(chunk.to_vec().into_boxed_slice());
        let mut stream = SceneStream::new(chunk_leaked)?;
        self.call_cmd_names = pck.inc_cmd_name_map.clone();
        self.user_cmd_names = stream.scn_cmd_name_map.clone();
        stream.jump_to_z_label(z_no.max(0) as usize)?;
        Ok((stream, scene_no))
    }

    fn jump_to_scene_name(&mut self, scene_name: &str, z_no: i32) -> Result<()> {
        if std::env::var_os("SIGLUS_TRACE_SCENE_SWITCH").is_some() {
            eprintln!("[vm scene jump] scene={} z={}", scene_name, z_no);
        }
        let (stream, scene_no) = self.load_scene_stream(scene_name, z_no)?;
        self.stream = stream;
        self.int_stack.clear();
        self.str_stack.clear();
        self.element_points.clear();
        self.call_stack.clear();
        self.call_stack.push(self.scene_base_call());
        self.user_props.clear();
        self.current_scene_no = Some(scene_no);
        self.current_scene_name = Some(scene_name.to_string());
        self.current_line_no = -1;
        self.ctx.current_scene_no = Some(scene_no as i64);
        self.ctx.current_scene_name = Some(scene_name.to_string());
        self.ctx.current_line_no = -1;
        Ok(())
    }

    fn farcall_scene_name(&mut self, scene_name: &str, z_no: i32, ret_form: i32) -> Result<()> {
        if std::env::var_os("SIGLUS_TRACE_SCENE_SWITCH").is_some() {
            eprintln!(
                "[vm scene farcall] scene={} z={} ret_form={}",
                scene_name, z_no, ret_form
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
            user_props: std::mem::take(&mut self.user_props),
            current_scene_no: self.current_scene_no,
            current_scene_name: self.current_scene_name.clone(),
            current_line_no: self.current_line_no,
            ret_form,
        };
        self.scene_stack.push(saved);
        let (stream, scene_no) = self.load_scene_stream(scene_name, z_no)?;
        self.stream = stream;
        self.call_stack.push(self.scene_base_call());
        self.current_scene_no = Some(scene_no);
        self.current_scene_name = Some(scene_name.to_string());
        self.current_line_no = -1;
        self.ctx.current_scene_no = Some(scene_no as i64);
        self.ctx.current_scene_name = Some(scene_name.to_string());
        self.ctx.current_line_no = -1;
        Ok(())
    }

    fn return_from_scene(&mut self, args: Vec<Value>) -> Result<bool> {
        let Some(saved) = self.scene_stack.pop() else {
            return Ok(false);
        };
        if std::env::var_os("SIGLUS_TRACE_SCENE_SWITCH").is_some() {
            eprintln!(
                "[vm scene return] ret_form={} args={:?}",
                saved.ret_form, args
            );
        }
        self.stream = saved.stream;
        self.int_stack = saved.int_stack;
        self.str_stack = saved.str_stack;
        self.element_points = saved.element_points;
        self.call_stack = saved.call_stack;
        self.user_props = saved.user_props;
        self.current_scene_no = saved.current_scene_no;
        self.current_scene_name = saved.current_scene_name;
        self.current_line_no = saved.current_line_no;
        self.ctx.current_scene_no = self.current_scene_no.map(|v| v as i64);
        self.ctx.current_scene_name = self.current_scene_name.clone();
        self.ctx.current_line_no = self.current_line_no as i64;
        self.user_cmd_names = saved.user_cmd_names;
        self.call_cmd_names = saved.call_cmd_names;

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
                self.ctx.stack.push(Value::Int(if self.save_point.is_some() { 1 } else { 0 }));
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
                self.ctx.stack.push(Value::Int(if self.has_sel_point() { 1 } else { 0 }));
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
        const FORM_GLOBAL_JUMP: i32 = 4;
        const FORM_GLOBAL_FARCALL: i32 = 5;
        if form_id == FORM_GLOBAL_JUMP {
            let scene_name = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
            let z_no = if al_id >= 1 {
                args.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32
            } else {
                0
            };
            if !scene_name.is_empty() {
                self.jump_to_scene_name(scene_name, z_no)?;
            }
            self.push_default_for_ret(ret_form);
            return Ok(true);
        }
        if form_id == FORM_GLOBAL_FARCALL {
            let scene_name = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
            let z_no = if al_id >= 1 {
                args.get(2).and_then(|v| v.as_i64()).unwrap_or(0) as i32
            } else {
                0
            };
            if !scene_name.is_empty() {
                self.farcall_scene_name(scene_name, z_no, ret_form)?;
            } else {
                self.push_default_for_ret(ret_form);
            }
            return Ok(true);
        }
        Ok(false)
    }

    fn take_ctx_return(&mut self, ret_form: i32) {
        if ret_form == self.cfg.fm_void {
            // Ensure form handlers cannot leak values into subsequent non-void returns.
            self.ctx.stack.clear();
            return;
        }

        let v = self.ctx.pop();
        match ret_form {
            f if f == self.cfg.fm_int || f == self.cfg.fm_label => {
                let n = match v {
                    Some(Value::Int(n)) => n as i32,
                    _ => 0,
                };
                self.push_int(n);
            }
            f if f == self.cfg.fm_str => {
                let s = match v {
                    Some(Value::Str(s)) => s,
                    _ => String::new(),
                };
                self.push_str(s);
            }
            f if f == self.cfg.fm_list => {
                let n = match v {
                    Some(Value::List(items)) => items.len() as i32,
                    _ => 0,
                };
                self.push_int(n);
            }
            _ => {
                // Unknown return form: keep the value (if any) in unknown recorder.
                self.push_int(0);
            }
        }
    }

    fn push_default_for_ret(&mut self, ret_form: i32) {
        if ret_form == self.cfg.fm_int || ret_form == self.cfg.fm_label {
            self.push_int(0);
        } else if ret_form == self.cfg.fm_str {
            self.push_str(String::new());
        }
    }

    fn push_return_value_raw(&mut self, v: Value) {
        match v {
            Value::NamedArg { value, .. } => self.push_return_value_raw(*value),
            Value::Int(n) => self.push_int(n as i32),
            Value::Str(s) => self.push_str(s),
            Value::Element(elm) => self.push_element(elm),
            Value::List(items) => {
                self.push_int(items.len() as i32);
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
