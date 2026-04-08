//! Scene VM

use anyhow::{anyhow, bail, Result};
use std::collections::BTreeMap;

use crate::runtime::{self, Command, CommandContext, Value};
use crate::runtime::opcode::OpCode;
use crate::scene_stream::SceneStream;
use crate::elm_code;

const CD_NONE: u8 = 0x00;
const CD_NL: u8 = 0x01;
const CD_PUSH: u8 = 0x02;
const CD_POP: u8 = 0x03;
const CD_COPY: u8 = 0x04;
const CD_PROPERTY: u8 = 0x05;
const CD_COPY_ELM: u8 = 0x06;
const CD_DEC_PROP: u8 = 0x07;
const CD_ELM_POINT: u8 = 0x08;
const CD_ARG: u8 = 0x09;

const CD_GOTO: u8 = 0x10;
const CD_GOTO_TRUE: u8 = 0x11;
const CD_GOTO_FALSE: u8 = 0x12;
const CD_GOSUB: u8 = 0x13;
const CD_GOSUBSTR: u8 = 0x14;
const CD_RETURN: u8 = 0x15;
const CD_EOF: u8 = 0x16;

const CD_ASSIGN: u8 = 0x20;
const CD_OPERATE_1: u8 = 0x21;
const CD_OPERATE_2: u8 = 0x22;

const CD_COMMAND: u8 = 0x30;
const CD_TEXT: u8 = 0x31;
const CD_NAME: u8 = 0x32;
const CD_SEL_BLOCK_START: u8 = 0x33;
const CD_SEL_BLOCK_END: u8 = 0x34;

const OP_PLUS: u8 = 0x01;
const OP_MINUS: u8 = 0x02;
const OP_MULTIPLE: u8 = 0x03;
const OP_DIVIDE: u8 = 0x04;
const OP_AMARI: u8 = 0x05;

const OP_EQUAL: u8 = 0x10;
const OP_NOT_EQUAL: u8 = 0x11;
const OP_GREATER: u8 = 0x12;
const OP_GREATER_EQUAL: u8 = 0x13;
const OP_LESS: u8 = 0x14;
const OP_LESS_EQUAL: u8 = 0x15;

const OP_LOGICAL_AND: u8 = 0x20;
const OP_LOGICAL_OR: u8 = 0x21;

const OP_TILDE: u8 = 0x30;
const OP_AND: u8 = 0x31;
const OP_OR: u8 = 0x32;
const OP_HAT: u8 = 0x33;
const OP_SL: u8 = 0x34;
const OP_SR: u8 = 0x35;
const OP_SR3: u8 = 0x36;

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
        fn env_i32(key: &str, default: i32) -> i32 {
            std::env::var(key)
                .ok()
                .and_then(|v| v.parse::<i32>().ok())
                .unwrap_or(default)
        }

        fn env_u64(key: &str, default: u64) -> u64 {
            std::env::var(key)
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(default)
        }

        // These values are title-specific; override via environment variables if needed.
        // Keep them overrideable.
        let fm_void = env_i32("SIGLUS_FM_VOID", 0);
        let fm_int = env_i32("SIGLUS_FM_INT", 10);
        let fm_str = env_i32("SIGLUS_FM_STR", 20);

        // These are often game-specific or version-specific. Default to -1
        // so the VM doesn't silently misinterpret types.
        let fm_label = env_i32("SIGLUS_FM_LABEL", -1);
        let fm_list = env_i32("SIGLUS_FM_LIST", -1);
        let fm_intlist = env_i32("SIGLUS_FM_INTLIST", -1);
        let fm_strlist = env_i32("SIGLUS_FM_STRLIST", -1);

        let max_steps = env_u64("SIGLUS_VM_MAX_STEPS", 5_000_000);

        Self {
            fm_void,
            fm_int,
            fm_str,
            fm_label,
            fm_list,
            fm_intlist,
            fm_strlist,
            max_steps,
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
    generic_packed_props: BTreeMap<i32, Value>,

    pub unknown_opcodes: BTreeMap<u8, u64>,
    pub unknown_forms: BTreeMap<i32, u64>,

    steps: u64,
    halted: bool,

    // When a command triggers a VM wait (movie wait-key etc.), its return value is produced when the wait completes.
    delayed_ret_form: Option<i32>,
}

impl<'a> SceneVm<'a> {
    pub fn new(stream: SceneStream<'a>, ctx: CommandContext) -> Self {
        let cfg = VmConfig::from_env();
        let base_call = CallFrame {
            return_pc: 0,
            ret_form: cfg.fm_void,
            user_props: Vec::new(),
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
            generic_packed_props: BTreeMap::new(),
            unknown_opcodes: BTreeMap::new(),
            unknown_forms: BTreeMap::new(),

            steps: 0,
            halted: false,
            delayed_ret_form: None,
        }
    }


    pub fn with_config(cfg: VmConfig, stream: SceneStream<'a>, ctx: CommandContext) -> Self {
        let base_call = CallFrame {
            return_pc: 0,
            ret_form: cfg.fm_void,
            user_props: Vec::new(),
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
            generic_packed_props: BTreeMap::new(),
            unknown_opcodes: BTreeMap::new(),
            unknown_forms: BTreeMap::new(),

            steps: 0,
            halted: false,
            delayed_ret_form: None,
        }
    }


    pub fn is_blocked(&mut self) -> bool {
        self.ctx.wait_poll()
    }

    pub fn step(&mut self) -> Result<bool> {
        if self.halted {
            return Ok(false);
        }

        // Block execution when the runtime requested it (WAIT / WAIT_KEY).
        // Important: do NOT consume step budget while blocked.
        let blocked = self.ctx.wait_poll();
        if blocked {
            return Ok(true);
        }

        // If the previous command yielded a delayed return (movie wait-key), materialize it now.
        if let Some(rf) = self.delayed_ret_form.take() {
            self.take_ctx_return(rf);
        }

        if self.steps >= self.cfg.max_steps {
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
                self.halted = true;
                return Ok(false);
            }
        };

        match opcode {
                CD_NL => {
                    let _line_no = self.stream.pop_i32()?;
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
                    self.call_stack.push(CallFrame {
                        return_pc: 0,
                        ret_form: self.cfg.fm_void,
                        user_props: Vec::new(),
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

                    self.call_stack.push(CallFrame {
                        return_pc: 0,
                        ret_form: self.cfg.fm_void,
                        user_props: Vec::new(),
                    });

                    self.stream.jump_to_label(label_no.max(0) as usize)?;
                }
                CD_RETURN => {
                    let args = self.pop_arg_list()?;
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
                            args[idx] = crate::runtime::Value::NamedArg { id, value: Box::new(v) };
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
                    // For bring-up, default to 0 (first choice) if scripts expect a value.
                    self.push_int(0);
                }

                CD_EOF => {
                    self.halted = true;
                    return Ok(false);
                }

                CD_NONE => {
                    // In the original engine this is treated as a fatal script error.
                    // For bring-up, stop execution and record it.
                    *self.unknown_opcodes.entry(opcode).or_insert(0) += 1;
                    println!("VM hit CD_NONE at pc=0x{:x}; stopping", pc_before);
                    self.halted = true;
                    return Ok(false);
                }

                other => {
                    *self.unknown_opcodes.entry(other).or_insert(0) += 1;
                    println!("VM unknown opcode=0x{other:02x} at pc=0x{:x}; stopping", pc_before);
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
            bail!("invalid element point start={start} len={}", self.int_stack.len());
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

    fn assign_generic_packed_prop(&mut self, packed_id: i32, array_idx: Option<usize>, rhs: Value) {
        if let Some(i) = array_idx {
            let default_like = self.default_value_like(&rhs);
            let entry = self
                .generic_packed_props
                .entry(packed_id)
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
            self.generic_packed_props.insert(packed_id, rhs);
        }
    }

    fn exec_copy_element(&mut self) -> Result<()> {
        let start = *self
            .element_points
            .last()
            .ok_or_else(|| anyhow!("COPY_ELM without a prior ELM_POINT"))?;
        if start > self.int_stack.len() {
            bail!("invalid element point start={start} len={}", self.int_stack.len());
        }
        let slice = self.int_stack[start..].to_vec();
        self.element_points.push(self.int_stack.len());
        self.int_stack.extend_from_slice(&slice);
        Ok(())
    }

    fn pop_value_for_form(&mut self, form_code: i32) -> Result<Value> {
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
        *self.unknown_forms.entry(form_code).or_insert(0) += 1;
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
        *self.unknown_forms.entry(form_code).or_insert(0) += 1;
        Ok(())
    }

    fn exec_pop(&mut self, form_code: i32) -> Result<()> {
        if form_code == self.cfg.fm_int {
            let _ = self.pop_int()?;
            return Ok(());
        }
        if form_code == self.cfg.fm_str {
            let _ = self.pop_str()?;
            return Ok(());
        }

        *self.unknown_forms.entry(form_code).or_insert(0) += 1;
        Ok(())
    }

    fn exec_copy(&mut self, form_code: i32) -> Result<()> {
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
        *self.unknown_forms.entry(form_code).or_insert(0) += 1;
        self.exec_copy_element()?;
        Ok(())
    }

    // ---------------------------------------------------------------------
    // Command/Property dispatch bridging
    // ---------------------------------------------------------------------

    fn exec_property(&mut self, elm: Vec<i32>) -> Result<()> {
        if elm.is_empty() {
            self.push_int(0);
            return Ok(());
        }

        // Call-local properties (declared by CD_DEC_PROP / populated by CD_ARG).
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
            let v = self.user_props.get(&prop_id).cloned().unwrap_or(Value::Int(0));
            self.push_property_value(v, array_idx);
            return Ok(());
        }

        // The real engine passes only an element chain and expects the command
        // processor to push the return value onto the VM stack.
        //
        // In this port, we attempt a best-effort numeric dispatch using:
        // - form_id = elm[0]
        // - op_id = elm[1] (if present)
        if head_owner != 0 {
            let array_idx = self.extract_array_index(&elm);
            let v = self
                .generic_packed_props
                .get(&head)
                .cloned()
                .unwrap_or(Value::Int(0));
            self.push_property_value(v, array_idx);
            return Ok(());
        }

        let form_id = head;
        let mut args: Vec<Value> = Vec::new();
        if elm.len() >= 2 {
            args.push(Value::Int(elm[1] as i64));
        }
        args.push(Value::Element(elm.clone()));

        let cmd = Command {
            name: format!("PROPERTY_{form_id}"),
            code: Some(OpCode::form(form_id as u32)),
            args,
        };
        runtime::dispatch(&mut self.ctx, &cmd)?;

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
                _ => {
                }
            }
            return Ok(());
        }

        if head_owner == elm_code::ELM_OWNER_USER_PROP {
            let prop_id = elm_code::code(head);
            let array_idx = self.extract_array_index(&elm);
            self.assign_user_prop(prop_id, array_idx, rhs);
            return Ok(());
        }

        // Non-call-prop assignment: forward a best-effort form dispatch.
        if head_owner != 0 {
            let array_idx = self.extract_array_index(&elm);
            self.assign_generic_packed_prop(head, array_idx, rhs);
            return Ok(());
        }

        let form_id = head;
        let mut args: Vec<Value> = Vec::new();
        if elm.len() >= 2 {
            args.push(Value::Int(elm[1] as i64));
        }
        args.push(Value::Int(al_id as i64));
        args.push(rhs);
        args.push(Value::Element(elm.clone()));

        let cmd = Command {
            name: format!("ASSIGN_{form_id}"),
            code: Some(OpCode::form(form_id as u32)),
            args,
        };
        runtime::dispatch(&mut self.ctx, &cmd)?;
        self.ctx.stack.clear();
        Ok(())
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

        let form_id = elm[0];
        let owner = elm_code::owner(form_id);

        // For classic forms (owner==0), we keep the previous calling convention:
        // prepend a sub-op id derived from the element chain.
        if owner == elm_code::ELM_OWNER_FORM {
            // Map element chain to sub-op when possible.
            // In the fork, nested element codes often represent method/property selectors.
            let op_id = if elm.len() >= 2 { elm[1] } else { al_id };
            args.insert(0, Value::Int(op_id as i64));
        }

        // Preserve the element chain plus call metadata (helps interpret overloads).
        // Layout: [op_id, ...args, Element(elm), al_id, ret_form]
        args.push(Value::Element(elm.clone()));
        args.push(Value::Int(al_id as i64));
        args.push(Value::Int(ret_form as i64));

        let (name, code) = match owner {
            o if o == elm_code::ELM_OWNER_FORM => (
                format!("FORM_{form_id}"),
                Some(OpCode::form(form_id as u32)),
            ),
            o if o == elm_code::ELM_OWNER_USER_CMD || o == elm_code::ELM_OWNER_CALL_CMD => {
                let cmd_no = elm_code::code(form_id) as u32;
                let name = if o == elm_code::ELM_OWNER_CALL_CMD {
                    self.ctx
                        .ids
                        .call_cmd_name(cmd_no)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("CALL_CMD#{cmd_no}"))
                } else {
                    self.ctx
                        .ids
                        .user_cmd_name(cmd_no)
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| format!("USER_CMD#{cmd_no}"))
                };
                (name, None)
            }
            _ => {
                let cmd_no = elm_code::code(form_id) as u32;
                (
                    format!("ELM_OWNER{}_GROUP{}_CODE{}", owner, elm_code::group(form_id), cmd_no),
                    None,
                )
            }
        };

        let cmd = Command { name, code, args: args.clone() };
        runtime::dispatch(&mut self.ctx, &cmd)?;

        if ret_form != self.cfg.fm_void {
            // If a wait is active, the return value will be pushed when the wait completes.
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
            *self.unknown_forms.entry(form_code).or_insert(0) += 1;
            self.push_int(0);
            return Ok(());
        }

        let v = self.pop_int()?;
        let out = match opr {
            OP_PLUS => v,
            OP_MINUS => v.wrapping_neg(),
            OP_TILDE => !v,
            _ => {
                v
            }
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
        *self.unknown_forms.entry(form_l).or_insert(0) += 1;
        *self.unknown_forms.entry(form_r).or_insert(0) += 1;
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

            _ => {
                0
            }
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
            _ => {
                s
            }
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
            _ => {
                Value::Int(0)
            }
        }
    }
}
