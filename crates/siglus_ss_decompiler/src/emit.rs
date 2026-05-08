use crate::constants::ChainAtom;
use crate::constants::*;
use crate::disasm::{ArgForm, Instruction, Op};
use crate::scene::Scene;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
enum Expr {
    Int(i32),
    Str(String),
    Raw(String),
    ElmCode(i32),
    Chain(Vec<Expr>),
    Property(Vec<Expr>),
    Command {
        chain: Vec<Expr>,
        arg_list_id: i32,
        args: Vec<Expr>,
        named_arg_ids: Vec<i32>,
        ret_form: i32,
    },
    Unary {
        op: u8,
        expr: Box<Expr>,
    },
    Binary {
        op: u8,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    Gosub {
        label: String,
        args: Vec<Expr>,
        ret_form: i32,
    },
}

impl Expr {
    fn to_ss(&self, symbols: &SymbolTables) -> String {
        self.to_ss_prec(symbols, PREC_LOWEST, ChildSide::None, None)
    }

    fn to_ss_prec(
        &self,
        symbols: &SymbolTables,
        parent_prec: u8,
        side: ChildSide,
        parent_op: Option<u8>,
    ) -> String {
        let my_prec = self.precedence();
        let mut rendered = match self {
            Expr::Int(v) => v.to_string(),
            Expr::Str(s) => quote_string(s),
            Expr::Raw(s) => s.clone(),
            Expr::ElmCode(v) => symbols.elm_name(*v),
            Expr::Chain(items) => format_chain(items, symbols),
            Expr::Property(items) => format_chain(items, symbols),
            Expr::Command {
                chain,
                arg_list_id,
                args,
                named_arg_ids,
                ret_form,
            } => {
                let mut rendered = Vec::new();
                let named_start = args.len().saturating_sub(named_arg_ids.len());
                for (idx, arg) in args.iter().enumerate() {
                    if idx >= named_start {
                        let name_id = named_arg_ids.get(idx - named_start).copied().unwrap_or(-1);
                        rendered.push(format!(
                            "/*named_arg_id={}*/ {}",
                            name_id,
                            arg.to_ss(symbols)
                        ));
                    } else {
                        rendered.push(arg.to_ss(symbols));
                    }
                }
                let ret = if *ret_form == FM_VOID {
                    String::new()
                } else {
                    format!(" /* ret={} */", symbols.form_name(*ret_form))
                };
                format!(
                    "{}({}){} /* al_id={} */",
                    format_chain(chain, symbols),
                    rendered.join(", "),
                    ret,
                    arg_list_id
                )
            }
            Expr::Unary { op, expr } => {
                let operand = expr.to_ss_prec(symbols, PREC_UNARY, ChildSide::Right, Some(*op));
                format!("{}{}", op_name(*op), operand)
            }
            Expr::Binary { op, left, right } => {
                let prec = binary_precedence(*op);
                let left_s = left.to_ss_prec(symbols, prec, ChildSide::Left, Some(*op));
                let right_s = right.to_ss_prec(symbols, prec, ChildSide::Right, Some(*op));
                format!("{} {} {}", left_s, op_name(*op), right_s)
            }
            Expr::Gosub {
                label,
                args,
                ret_form,
            } => {
                let rendered = args
                    .iter()
                    .map(|e| e.to_ss(symbols))
                    .collect::<Vec<_>>()
                    .join(", ");
                let kw = if *ret_form == FM_STR {
                    "gosubstr"
                } else {
                    "gosub"
                };
                format!("{}({}) {}", kw, rendered, label)
            }
        };

        if self.needs_parentheses(parent_prec, side, parent_op) {
            rendered = format!("({rendered})");
        }
        rendered
    }

    fn precedence(&self) -> u8 {
        match self {
            Expr::Binary { op, .. } => binary_precedence(*op),
            Expr::Unary { .. } => PREC_UNARY,
            Expr::Int(_)
            | Expr::Str(_)
            | Expr::Raw(_)
            | Expr::ElmCode(_)
            | Expr::Chain(_)
            | Expr::Property(_)
            | Expr::Command { .. }
            | Expr::Gosub { .. } => PREC_PRIMARY,
        }
    }

    fn needs_parentheses(&self, parent_prec: u8, side: ChildSide, parent_op: Option<u8>) -> bool {
        if parent_prec == PREC_LOWEST {
            return false;
        }
        let my_prec = self.precedence();
        if my_prec < parent_prec {
            return true;
        }
        if my_prec > parent_prec {
            return false;
        }

        match (side, parent_op, self) {
            // Siglus binary operators are emitted as left-associative.  The
            // right child needs parentheses on equal precedence unless flattening
            // the exact same associative operator is semantics-preserving.
            (ChildSide::Right, Some(parent), Expr::Binary { op: child, .. }) => {
                !can_flatten_same_precedence(parent, *child)
            }
            // Avoid ambiguous spellings such as --x or ~~x for nested unary ops.
            (ChildSide::Right, Some(_), Expr::Unary { .. }) if parent_prec == PREC_UNARY => true,
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChildSide {
    None,
    Left,
    Right,
}

const PREC_LOWEST: u8 = 0;
const PREC_LOGICAL_OR: u8 = 10;
const PREC_LOGICAL_AND: u8 = 20;
const PREC_BIT_OR: u8 = 30;
const PREC_BIT_XOR: u8 = 31;
const PREC_BIT_AND: u8 = 32;
const PREC_EQUALITY: u8 = 40;
const PREC_RELATIONAL: u8 = 50;
const PREC_SHIFT: u8 = 60;
const PREC_ADDITIVE: u8 = 70;
const PREC_MULTIPLICATIVE: u8 = 80;
const PREC_UNARY: u8 = 90;
const PREC_PRIMARY: u8 = 100;

fn binary_precedence(op: u8) -> u8 {
    match op {
        0x21 => PREC_LOGICAL_OR,
        0x20 => PREC_LOGICAL_AND,
        0x32 => PREC_BIT_OR,
        0x33 => PREC_BIT_XOR,
        0x31 => PREC_BIT_AND,
        0x10 | 0x11 => PREC_EQUALITY,
        0x12 | 0x13 | 0x14 | 0x15 => PREC_RELATIONAL,
        0x34 | 0x35 | 0x36 => PREC_SHIFT,
        0x01 | 0x02 => PREC_ADDITIVE,
        0x03 | 0x04 | 0x05 => PREC_MULTIPLICATIVE,
        _ => PREC_LOWEST + 1,
    }
}

fn can_flatten_same_precedence(parent: u8, child: u8) -> bool {
    parent == child
        && matches!(
            parent,
            0x01  // +
                | 0x03 // *
                | 0x20 // &&
                | 0x21 // ||
                | 0x31 // &
                | 0x32 // |
                | 0x33 // ^
        )
}

#[derive(Debug, Default)]
struct VmModel {
    stack: Vec<Expr>,
    element_points: Vec<usize>,
    current_line: Option<i32>,
    sel_depth: usize,
}

fn handle_instruction(
    scene: &Scene,
    symbols: &SymbolTables,
    labels: &LabelNames,
    state: &mut VmModel,
    insn: &Instruction,
) -> Option<String> {
    match &insn.op {
        Op::None => Some(comment(insn, "CD_NONE")),
        Op::Nl { line } => {
            state.current_line = Some(*line);
            Some(format!("// line {}", line))
        }
        Op::Push { form, value } => {
            let expr = match *form {
                FM_INT => Expr::Int(*value),
                FM_STR => Expr::Str(scene.string(*value)),
                _ => Expr::Raw(format!("push({}, {})", symbols.form_name(*form), value)),
            };
            state.stack.push(expr);
            None
        }
        Op::Pop { form } => {
            if *form == FM_VOID {
                return state.stack.pop().map(|expr| format!("{};", expr.to_ss(symbols)));
            }
            let Some(expr) = state.stack.pop() else {
                return Some(format!(
                    "// {} pop {} elided: value belongs to another branch path",
                    offset_comment(insn),
                    symbols.form_name(*form)
                ));
            };
            Some(format!(
                "{}; // {} pop {}",
                expr.to_ss(symbols),
                offset_comment(insn),
                symbols.form_name(*form)
            ))
        }
        Op::Copy { form } => {
            let expr = state
                .stack
                .last()
                .cloned()
                .unwrap_or_else(|| Expr::Raw("<stack-underflow>".to_string()));
            state.stack.push(expr);
            Some(format!(
                "// {} copy {}",
                offset_comment(insn),
                symbols.form_name(*form)
            ))
        }
        Op::Property => {
            let chain = pop_element_chain(state);
            state.stack.push(Expr::Property(chain));
            None
        }
        Op::CopyElm => {
            let Some(start) = state.element_points.last().copied() else {
                return Some(format!(
                    "// {} copy element failed: missing ELM_POINT",
                    offset_comment(insn)
                ));
            };
            if start > state.stack.len() {
                return Some(format!(
                    "// {} copy element failed: bad ELM_POINT start={} stack_len={}",
                    offset_comment(insn),
                    start,
                    state.stack.len()
                ));
            }
            let new_start = state.stack.len();
            let items = state.stack[start..].to_vec();
            state.stack.extend(items);
            state.element_points.push(new_start);
            Some(format!("// {} copy element", offset_comment(insn)))
        }
        Op::DecProp { form, prop_id } => {
            let size = if *form == 11 || *form == 21 {
                state
                    .stack
                    .pop()
                    .map(|e| e.to_ss(symbols))
                    .unwrap_or_else(|| "0".to_string())
            } else {
                "0".to_string()
            };
            Some(format!(
                "property {}: {}; // prop_id={} size={} {}",
                call_prop_name(scene, *prop_id),
                symbols.form_name(*form),
                prop_id,
                size,
                offset_comment(insn)
            ))
        }
        Op::ElmPoint => {
            state.element_points.push(state.stack.len());
            None
        }
        Op::Arg => Some(format!("// {} expand call arguments", offset_comment(insn))),
        Op::Goto { label } => Some(format!(
            "goto {}; // {}",
            labels.label(*label),
            offset_comment(insn)
        )),
        Op::GotoTrue { label } => {
            let cond = state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<cond-underflow>".to_string()));
            Some(format!(
                "if ({}) goto {}; // {}",
                cond.to_ss(symbols),
                labels.label(*label),
                offset_comment(insn)
            ))
        }
        Op::GotoFalse { label } => {
            let cond = state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<cond-underflow>".to_string()));
            Some(format!(
                "if (!({})) goto {}; // {}",
                cond.to_ss(symbols),
                labels.label(*label),
                offset_comment(insn)
            ))
        }
        Op::Gosub { label, arg_forms } => {
            let args = pop_n_args(state, arg_forms.len());
            state.stack.push(Expr::Gosub {
                label: labels.label(*label),
                args,
                ret_form: FM_INT,
            });
            None
        }
        Op::GosubStr { label, arg_forms } => {
            let args = pop_n_args(state, arg_forms.len());
            state.stack.push(Expr::Gosub {
                label: labels.label(*label),
                args,
                ret_form: FM_STR,
            });
            None
        }
        Op::Return { arg_forms } => {
            let args = pop_n_args(state, arg_forms.len());
            let rendered = args
                .iter()
                .map(|e| e.to_ss(symbols))
                .collect::<Vec<_>>()
                .join(", ");
            Some(format!("return({}); // {}", rendered, offset_comment(insn)))
        }
        Op::Assign {
            left_form,
            right_form,
            arg_list_id,
        } => {
            let right = state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<rhs-underflow>".to_string()));
            let left_chain = pop_element_chain(state);
            let left = format_chain(&left_chain, symbols);
            Some(format!(
                "{} = {}; // {} left={} right={} al_id={}",
                left,
                right.to_ss(symbols),
                offset_comment(insn),
                symbols.form_name(*left_form),
                symbols.form_name(*right_form),
                arg_list_id
            ))
        }
        Op::Operate1 { form: _, op } => {
            let e = state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<unary-underflow>".to_string()));
            state.stack.push(Expr::Unary {
                op: *op,
                expr: Box::new(e),
            });
            None
        }
        Op::Operate2 {
            left_form: _,
            right_form: _,
            op,
        } => {
            let right = state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<rhs-underflow>".to_string()));
            let left = state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<lhs-underflow>".to_string()));
            state.stack.push(Expr::Binary {
                op: *op,
                left: Box::new(left),
                right: Box::new(right),
            });
            None
        }
        Op::Command {
            arg_list_id,
            arg_forms,
            named_arg_ids,
            ret_form,
            ..
        } => {
            let args = pop_n_args(state, arg_forms.len());
            let chain = pop_element_chain(state);
            let command = Expr::Command {
                chain,
                arg_list_id: *arg_list_id,
                args,
                named_arg_ids: named_arg_ids.clone(),
                ret_form: *ret_form,
            };
            state.stack.push(command);
            None
        }
        Op::Text { read_flag } => {
            let text = state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<text-underflow>".to_string()));
            let rendered = match text {
                Expr::Str(s) => s,
                other => other.to_ss(symbols),
            };
            Some(format!(
                "{} // read_flag={} {}",
                rendered,
                read_flag,
                offset_comment(insn)
            ))
        }
        Op::Name => {
            let name = state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<name-underflow>".to_string()));
            Some(format!(
                "【{}】 // {}",
                strip_quotes(name.to_ss(symbols)),
                offset_comment(insn)
            ))
        }
        Op::SelBlockStart => {
            state.sel_depth += 1;
            Some(format!(
                "// {} selection block start depth={}",
                offset_comment(insn),
                state.sel_depth
            ))
        }
        Op::SelBlockEnd => {
            let old = state.sel_depth;
            state.sel_depth = state.sel_depth.saturating_sub(1);
            Some(format!(
                "// {} selection block end depth={}",
                offset_comment(insn),
                old
            ))
        }
        Op::Eof => Some(format!("eof; // {}", offset_comment(insn))),
        Op::Unknown { code } => Some(format!(
            "__cd_unknown(0x{:02X}); // {}",
            code,
            offset_comment(insn)
        )),
    }
}

fn pop_n_args(state: &mut VmModel, n: usize) -> Vec<Expr> {
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(
            state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<arg-underflow>".to_string())),
        );
    }
    out.reverse();
    out
}

fn pop_element_chain(state: &mut VmModel) -> Vec<Expr> {
    let Some(start) = state.element_points.pop() else {
        return vec![Expr::Raw(format!(
            "__missing_elm_point(stack_len={})",
            state.stack.len()
        ))];
    };
    if start > state.stack.len() {
        return vec![Expr::Raw(format!(
            "__bad_elm_point(start={}, stack_len={})",
            start,
            state.stack.len()
        ))];
    }
    let mut items = state.stack.split_off(start);
    let mut prev_was_array = false;
    for item in &mut items {
        if let Expr::Int(v) = item {
            let value = *v;
            if !prev_was_array {
                *item = Expr::ElmCode(value);
            }
            prev_was_array = value == ELM_ARRAY;
        } else {
            prev_was_array = false;
        }
    }
    if items.is_empty() {
        items.push(Expr::Raw(format!("__empty_elm_at_stack({})", start)));
    }
    items
}

fn format_chain(items: &[Expr], symbols: &SymbolTables) -> String {
    let mut atoms = Vec::new();
    for item in items {
        collect_chain_atoms(item, symbols, &mut atoms);
    }
    symbols.format_chain_dynamic(&atoms)
}

fn collect_chain_atoms(item: &Expr, symbols: &SymbolTables, atoms: &mut Vec<ChainAtom>) {
    match item {
        Expr::Int(v) | Expr::ElmCode(v) => atoms.push(ChainAtom::Code(*v)),
        Expr::Chain(items) | Expr::Property(items) => {
            for item in items {
                collect_chain_atoms(item, symbols, atoms);
            }
        }
        other => atoms.push(ChainAtom::Text(other.to_ss(symbols))),
    }
}

fn call_prop_name(scene: &Scene, prop_id: i32) -> String {
    if prop_id < 0 {
        return format!("call_prop_{prop_id}");
    }
    scene
        .call_prop_names
        .get(prop_id as usize)
        .cloned()
        .unwrap_or_else(|| format!("call_prop_{prop_id}"))
}

fn offset_comment(insn: &Instruction) -> String {
    format!("@0x{:04X} {}", insn.offset, cd_name(insn.code))
}

fn comment(insn: &Instruction, text: &str) -> String {
    format!("// {} {}", offset_comment(insn), text)
}

fn sanitize_label_name(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("unnamed");
    }
    out
}

fn quote_string(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}

fn strip_quotes(s: String) -> String {
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s
    }
}

#[derive(Debug, Clone)]
struct LabelNames {
    names: BTreeMap<i32, String>,
}

impl LabelNames {
    fn label(&self, id: i32) -> String {
        self.names
            .get(&id)
            .cloned()
            .unwrap_or_else(|| format!("#label_{:04}", id.max(0)))
    }
}

fn build_label_names(scene: &Scene) -> LabelNames {
    let mut names = BTreeMap::new();
    for id in 0..scene.labels.len() as i32 {
        names.insert(id, format!("#label_{:04}", id));
    }
    for (z, ofs) in scene.z_labels.iter().enumerate() {
        if *ofs <= 0 {
            continue;
        }
        for (id, label_ofs) in scene.labels.iter().enumerate() {
            if label_ofs == ofs {
                names.insert(id as i32, format!("#z{}", z));
                break;
            }
        }
    }
    LabelNames { names }
}

#[allow(dead_code)]
fn format_arg_form(form: &ArgForm, symbols: &SymbolTables) -> String {
    match form {
        ArgForm::Form(v) => symbols.form_name(*v),
        ArgForm::List(forms) => format!(
            "[{}]",
            forms
                .iter()
                .map(|f| format_arg_form(f, symbols))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

#[derive(Debug, Clone)]
enum FlatKind {
    Label(String),
    Line(String),
    Goto(String),
    IfFalse { cond: String, target: String },
    IfTrue { cond: String, target: String },
}

#[derive(Debug, Clone)]
struct FlatStmt {
    offset: usize,
    kind: FlatKind,
}

pub fn emit_structured_ss(scene: &Scene, insns: &[Instruction], symbols: &SymbolTables) -> String {
    let label_names = build_label_names(scene);
    let mut out = String::new();
    if let Some(name) = &scene.name {
        out.push_str(&format!("// scene: {}\n", name));
    }
    emit_user_symbol_header(scene, symbols, &mut out);
    if !out.ends_with("\n\n") {
        out.push('\n');
    }
    let flat = preprocess_flat_stmts(build_flat_stmts(scene, insns, symbols, &label_names));
    let label_pos = build_flat_label_pos(&flat);
    let mut emitter = StructuredEmitter {
        flat: &flat,
        label_pos,
        out: String::new(),
    };
    emitter.emit_range(0, flat.len(), 0, &LoopCtx::default());
    out.push_str(&emitter.out);
    out
}

fn emit_user_symbol_header(scene: &Scene, symbols: &SymbolTables, out: &mut String) {
    out.push_str(&format!(
        "user_prop_table inc_count={} scene_count={} {{\n",
        scene.pack_inc_prop_cnt,
        scene.scn_prop_names.len()
    ));
    for (i, name) in scene.pack_inc_prop_names.iter().enumerate() {
        let (form, size) = scene
            .pack_inc_props
            .get(i)
            .map(|p| (symbols.form_name(p.form), p.size))
            .unwrap_or_else(|| ("unknown_form".to_string(), 0));
        out.push_str(&format!(
            "    inc[{}] {};\n",
            i,
            format_user_prop_decl(&form, name, size)
        ));
    }
    for (i, name) in scene.scn_prop_names.iter().enumerate() {
        let user_id = scene.pack_inc_prop_cnt + i;
        let (form, size) = scene
            .scn_props
            .get(i)
            .map(|p| (symbols.form_name(p.form), p.size))
            .unwrap_or_else(|| ("unknown_form".to_string(), 0));
        out.push_str(&format!(
            "    scene[{}] user_id={} {};\n",
            i,
            user_id,
            format_user_prop_decl(&form, name, size)
        ));
    }
    out.push_str("}\n");

    out.push_str(&format!(
        "user_cmd_table inc_count={} scene_count={} {{\n",
        scene.pack_inc_cmd_cnt,
        scene.header.scn_cmd_name_cnt.max(0)
    ));
    let body_cmds = scene
        .scn_cmd_names
        .iter()
        .map(|name| name.strip_prefix("inc::").unwrap_or(name).to_string())
        .collect::<BTreeSet<_>>();
    for (i, name) in scene.pack_inc_cmd_names.iter().enumerate() {
        let storage = if body_cmds.contains(name) { "body" } else { "extern" };
        out.push_str(&format!(
            "    inc[{}] {} user_cmd {};\n",
            i, storage, name
        ));
    }
    let local_cmd_name_cnt = scene.header.scn_cmd_name_cnt.max(0) as usize;
    for i in 0..local_cmd_name_cnt.min(scene.scn_cmd_names.len()) {
        out.push_str(&format!(
            "    scene[{}] user_id={} body user_cmd {};\n",
            i,
            scene.pack_inc_cmd_cnt + i,
            &scene.scn_cmd_names[i]
        ));
    }
    out.push_str("}\n\n");
}

fn format_user_prop_decl(form: &str, name: &str, size: i32) -> String {
    if size > 0 {
        format!("{} {}[{}]", form, name, size)
    } else {
        format!("{} {}", form, name)
    }
}

fn emit_string_table(scene: &Scene, out: &mut String) {
    if scene.strings.is_empty() {
        return;
    }
    out.push_str("\n// string_table:\n");
    for (i, s) in scene.strings.iter().enumerate() {
        if !s.is_empty() {
            out.push_str(&format!("//   str[{}] = {}\n", i, quote_string(s)));
        }
    }
}

fn build_flat_stmts(
    scene: &Scene,
    insns: &[Instruction],
    symbols: &SymbolTables,
    labels: &LabelNames,
) -> Vec<FlatStmt> {
    let mut labels_by_offset: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for (id, ofs) in scene.labels.iter().enumerate() {
        if *ofs >= 0 {
            labels_by_offset
                .entry(*ofs as usize)
                .or_default()
                .push(labels.label(id as i32));
        }
    }
    for (z, ofs) in scene.z_labels.iter().enumerate() {
        if *ofs > 0 {
            labels_by_offset
                .entry(*ofs as usize)
                .or_default()
                .push(format!("#z{}", z));
        }
    }
    let mut command_entry_offsets: BTreeSet<usize> = BTreeSet::new();
    for (i, cmd) in scene.scn_cmds.iter().enumerate() {
        if cmd.offset >= 0 {
            let offset = cmd.offset as usize;
            command_entry_offsets.insert(offset);
            let name = scene
                .scn_cmd_names
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("cmd_{}", i));
            labels_by_offset
                .entry(offset)
                .or_default()
                .push(format!("#cmd_{}", sanitize_label_name(&name)));
        }
    }

    let mut state = VmModel::default();
    let mut out = Vec::new();
    for insn in insns {
        if command_entry_offsets.contains(&insn.offset) && insn.offset != 0 {
            state = VmModel::default();
            out.push(FlatStmt {
                offset: insn.offset,
                kind: FlatKind::Line(
                    "// reset VM expression stack at user-command entry".to_string(),
                ),
            });
        }
        if let Some(names) = labels_by_offset.get(&insn.offset) {
            let mut seen = BTreeSet::new();
            for name in names {
                if seen.insert(name.clone()) {
                    out.push(FlatStmt {
                        offset: insn.offset,
                        kind: FlatKind::Label(name.clone()),
                    });
                }
            }
        }
        if let Some(kind) = handle_instruction_flat(scene, symbols, labels, &mut state, insn) {
            out.push(FlatStmt {
                offset: insn.offset,
                kind,
            });
        }
    }
    out
}

fn handle_instruction_flat(
    scene: &Scene,
    symbols: &SymbolTables,
    labels: &LabelNames,
    state: &mut VmModel,
    insn: &Instruction,
) -> Option<FlatKind> {
    match &insn.op {
        Op::None => None,
        Op::Nl { line } => {
            state.current_line = Some(*line);
            Some(FlatKind::Line(format!("// line {}", line)))
        }
        Op::Push { form, value } => {
            let expr = match *form {
                FM_INT => Expr::Int(*value),
                FM_STR => Expr::Str(scene.string(*value)),
                _ => Expr::Raw(format!("push({}, {})", symbols.form_name(*form), value)),
            };
            state.stack.push(expr);
            None
        }
        Op::Pop { form } => {
            if *form == FM_VOID {
                return state
                    .stack
                    .pop()
                    .map(|expr| FlatKind::Line(format!("{};", expr.to_ss(symbols))));
            }
            let Some(expr) = state.stack.pop() else {
                return Some(FlatKind::Line(format!(
                    "// pop {} elided: value belongs to another branch path",
                    symbols.form_name(*form)
                )));
            };
            Some(FlatKind::Line(format!(
                "{}; // pop {}",
                expr.to_ss(symbols),
                symbols.form_name(*form)
            )))
        }
        Op::Copy { form: _ } => {
            let expr = state
                .stack
                .last()
                .cloned()
                .unwrap_or_else(|| Expr::Raw("<stack-underflow>".to_string()));
            state.stack.push(expr);
            None
        }
        Op::Property => {
            let chain = pop_element_chain(state);
            state.stack.push(Expr::Property(chain));
            None
        }
        Op::CopyElm => {
            let Some(start) = state.element_points.last().copied() else {
                return Some(FlatKind::Line(
                    "// copy element failed: missing ELM_POINT".to_string(),
                ));
            };
            if start > state.stack.len() {
                return Some(FlatKind::Line(format!(
                    "// copy element failed: bad ELM_POINT start={} stack_len={}",
                    start,
                    state.stack.len()
                )));
            }
            let new_start = state.stack.len();
            let items = state.stack[start..].to_vec();
            state.stack.extend(items);
            state.element_points.push(new_start);
            None
        }
        Op::DecProp { form, prop_id } => {
            let size = if *form == FM_INTLIST || *form == FM_STRLIST {
                state
                    .stack
                    .pop()
                    .map(|e| e.to_ss(symbols))
                    .unwrap_or_else(|| "0".to_string())
            } else {
                "0".to_string()
            };
            Some(FlatKind::Line(format!(
                "property {}: {}; // prop_id={} size={}",
                call_prop_name(scene, *prop_id),
                symbols.form_name(*form),
                prop_id,
                size
            )))
        }
        Op::ElmPoint => {
            state.element_points.push(state.stack.len());
            None
        }
        Op::Arg => Some(FlatKind::Line("__expand_args();".to_string())),
        Op::Goto { label } => Some(FlatKind::Goto(labels.label(*label))),
        Op::GotoTrue { label } => {
            let cond = state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<cond-underflow>".to_string()));
            Some(FlatKind::IfTrue {
                cond: cond.to_ss(symbols),
                target: labels.label(*label),
            })
        }
        Op::GotoFalse { label } => {
            let cond = state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<cond-underflow>".to_string()));
            Some(FlatKind::IfFalse {
                cond: cond.to_ss(symbols),
                target: labels.label(*label),
            })
        }
        Op::Gosub { label, arg_forms } => {
            let args = pop_n_args(state, arg_forms.len());
            state.stack.push(Expr::Gosub {
                label: labels.label(*label),
                args,
                ret_form: FM_INT,
            });
            None
        }
        Op::GosubStr { label, arg_forms } => {
            let args = pop_n_args(state, arg_forms.len());
            state.stack.push(Expr::Gosub {
                label: labels.label(*label),
                args,
                ret_form: FM_STR,
            });
            None
        }
        Op::Return { arg_forms } => {
            let args = pop_n_args(state, arg_forms.len());
            let rendered = args
                .iter()
                .map(|e| e.to_ss(symbols))
                .collect::<Vec<_>>()
                .join(", ");
            if rendered.is_empty() {
                Some(FlatKind::Line("return;".to_string()))
            } else {
                Some(FlatKind::Line(format!("return({});", rendered)))
            }
        }
        Op::Assign {
            left_form: _,
            right_form: _,
            arg_list_id: _,
        } => {
            let right = state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<rhs-underflow>".to_string()));
            let left_chain = pop_element_chain(state);
            let left = format_chain(&left_chain, symbols);
            Some(FlatKind::Line(format!(
                "{} = {};",
                left,
                right.to_ss(symbols)
            )))
        }
        Op::Operate1 { form: _, op } => {
            let e = state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<unary-underflow>".to_string()));
            state.stack.push(Expr::Unary {
                op: *op,
                expr: Box::new(e),
            });
            None
        }
        Op::Operate2 {
            left_form: _,
            right_form: _,
            op,
        } => {
            let right = state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<rhs-underflow>".to_string()));
            let left = state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<lhs-underflow>".to_string()));
            state.stack.push(Expr::Binary {
                op: *op,
                left: Box::new(left),
                right: Box::new(right),
            });
            None
        }
        Op::Command {
            arg_list_id,
            arg_forms,
            named_arg_ids,
            ret_form,
            ..
        } => {
            let args = pop_n_args(state, arg_forms.len());
            let chain = pop_element_chain(state);
            let command = Expr::Command {
                chain,
                arg_list_id: *arg_list_id,
                args,
                named_arg_ids: named_arg_ids.clone(),
                ret_form: *ret_form,
            };
            state.stack.push(command);
            None
        }
        Op::Text { read_flag } => {
            let text = state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<text-underflow>".to_string()));
            let rendered = match text {
                Expr::Str(s) => s,
                other => other.to_ss(symbols),
            };
            Some(FlatKind::Line(format!(
                "{} // read_flag={}",
                rendered, read_flag
            )))
        }
        Op::Name => {
            let name = state
                .stack
                .pop()
                .unwrap_or_else(|| Expr::Raw("<name-underflow>".to_string()));
            Some(FlatKind::Line(format!(
                "【{}】",
                strip_quotes(name.to_ss(symbols))
            )))
        }
        Op::SelBlockStart => Some(FlatKind::Line("{ // selection".to_string())),
        Op::SelBlockEnd => Some(FlatKind::Line("} // selection".to_string())),
        Op::Eof => None,
        Op::Unknown { code } => Some(FlatKind::Line(format!("__cd_unknown(0x{:02X});", code))),
    }
}

fn build_flat_label_pos(flat: &[FlatStmt]) -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    for (i, stmt) in flat.iter().enumerate() {
        if let FlatKind::Label(name) = &stmt.kind {
            out.entry(name.clone()).or_insert(i);
        }
    }
    out
}

fn preprocess_flat_stmts(mut flat: Vec<FlatStmt>) -> Vec<FlatStmt> {
    loop {
        let before = flat_signature(&flat);
        flat = remove_gotos_to_immediate_labels(flat);
        flat = remove_unreferenced_internal_labels(flat);
        let after = flat_signature(&flat);
        if before == after {
            return flat;
        }
    }
}

fn flat_signature(flat: &[FlatStmt]) -> Vec<String> {
    flat.iter()
        .map(|stmt| match &stmt.kind {
            FlatKind::Label(name) => format!("L:{name}"),
            FlatKind::Line(line) => format!("S:{line}"),
            FlatKind::Goto(target) => format!("G:{target}"),
            FlatKind::IfFalse { cond, target } => format!("F:{cond}->{target}"),
            FlatKind::IfTrue { cond, target } => format!("T:{cond}->{target}"),
        })
        .collect()
}

fn remove_gotos_to_immediate_labels(flat: Vec<FlatStmt>) -> Vec<FlatStmt> {
    let mut out = Vec::with_capacity(flat.len());
    for i in 0..flat.len() {
        let remove = match &flat[i].kind {
            FlatKind::Goto(target) => immediate_following_labels_contain(&flat, i + 1, target),
            _ => false,
        };
        if !remove {
            out.push(flat[i].clone());
        }
    }
    out
}

fn immediate_following_labels_contain(flat: &[FlatStmt], mut i: usize, target: &str) -> bool {
    let mut saw_label = false;
    while i < flat.len() {
        match &flat[i].kind {
            FlatKind::Label(name) => {
                saw_label = true;
                if name == target {
                    return true;
                }
                i += 1;
            }
            FlatKind::Line(line) if is_line_marker(line) => {
                if saw_label {
                    return false;
                }
                i += 1;
            }
            _ => return false,
        }
    }
    false
}

fn remove_unreferenced_internal_labels(flat: Vec<FlatStmt>) -> Vec<FlatStmt> {
    let referenced = referenced_labels(&flat);
    flat.into_iter()
        .filter(|stmt| match &stmt.kind {
            FlatKind::Label(name) => is_public_entry_label(name) || referenced.contains(name),
            _ => true,
        })
        .collect()
}

fn referenced_labels(flat: &[FlatStmt]) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for stmt in flat {
        match &stmt.kind {
            FlatKind::Goto(target) => {
                out.insert(target.clone());
            }
            FlatKind::IfFalse { target, .. } | FlatKind::IfTrue { target, .. } => {
                out.insert(target.clone());
            }
            _ => {}
        }
    }
    out
}

fn is_public_entry_label(name: &str) -> bool {
    name.starts_with("#z") || name.starts_with("#cmd_")
}

fn is_line_marker(line: &str) -> bool {
    line.starts_with("// line ")
}

fn negate_condition(cond: &str) -> String {
    let trimmed = cond.trim();
    if is_simple_condition_atom(trimmed) {
        format!("!{}", trimmed)
    } else {
        format!("!({})", trimmed)
    }
}

fn is_simple_condition_atom(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$' | '.' | '[' | ']'))
}

#[derive(Debug, Clone, Default)]
struct LoopCtx {
    continue_target: Option<String>,
    break_target: Option<String>,
}

struct StructuredEmitter<'a> {
    flat: &'a [FlatStmt],
    label_pos: BTreeMap<String, usize>,
    out: String,
}

impl<'a> StructuredEmitter<'a> {
    fn emit_range(&mut self, mut i: usize, end: usize, indent: usize, loop_ctx: &LoopCtx) {
        while i < end {
            if let Some(next) = self.try_emit_guarded_prefix_loop(i, end, indent) {
                i = next;
                continue;
            }
            if let Some(next) = self.try_emit_for(i, end, indent) {
                i = next;
                continue;
            }
            if let Some(next) = self.try_emit_while(i, end, indent) {
                i = next;
                continue;
            }
            if let Some(next) = self.try_emit_if(i, end, indent, loop_ctx) {
                i = next;
                continue;
            }
            if let Some(next) = self.try_emit_if_no_else(i, end, indent, loop_ctx) {
                i = next;
                continue;
            }

            match &self.flat[i].kind {
                FlatKind::Label(name) => {
                    self.write(indent, name);
                    i += 1;
                }
                FlatKind::Line(line) => {
                    self.write(indent, line);
                    i += 1;
                }
                FlatKind::Goto(target) => {
                    if loop_ctx.continue_target.as_deref() == Some(target.as_str()) {
                        self.write(indent, "continue;");
                    } else if loop_ctx.break_target.as_deref() == Some(target.as_str()) {
                        self.write(indent, "break;");
                    } else {
                        self.write(indent, &format!("goto {};", target));
                    }
                    i += 1;
                }
                FlatKind::IfFalse { cond, target } => {
                    self.write(indent, &format!("if ({}) goto {};", negate_condition(cond), target));
                    i += 1;
                }
                FlatKind::IfTrue { cond, target } => {
                    self.write(indent, &format!("if ({}) goto {};", cond, target));
                    i += 1;
                }
            }
        }
    }

    fn try_emit_guarded_prefix_loop(&mut self, i: usize, end: usize, indent: usize) -> Option<usize> {
        let loop_label = match &self.flat[i].kind {
            FlatKind::Label(name) if !is_public_entry_label(name) => name.clone(),
            _ => return None,
        };
        let prefix_start = self.skip_labels(i);
        if prefix_start >= end {
            return None;
        }
        let cond_idx = self.find_loop_guard_after_prefix(prefix_start, end)?;
        if !self.range_contains_executable_line(prefix_start, cond_idx) {
            return None;
        }
        let (break_cond, out_label) = match &self.flat[cond_idx].kind {
            FlatKind::IfFalse { cond, target } => (negate_condition(cond), target.clone()),
            FlatKind::IfTrue { cond, target } => (cond.clone(), target.clone()),
            _ => return None,
        };
        let out_pos = *self.label_pos.get(&out_label)?;
        if out_pos <= cond_idx || out_pos > end {
            return None;
        }
        if self.has_public_entry_label_between(prefix_start, out_pos) {
            return None;
        }
        let back_idx = self.prev_non_label(out_pos)?;
        match &self.flat[back_idx].kind {
            FlatKind::Goto(target) if target == &loop_label => {}
            _ => return None,
        }

        self.write(indent, "while (true) {");
        let ctx = LoopCtx {
            continue_target: Some(loop_label),
            break_target: Some(out_label.clone()),
        };
        self.emit_range(prefix_start, cond_idx, indent + 1, &ctx);
        self.write(indent + 1, &format!("if ({}) {{", break_cond));
        self.write(indent + 2, "break;");
        self.write(indent + 1, "}");
        self.emit_range(cond_idx + 1, back_idx, indent + 1, &ctx);
        self.write(indent, "}");
        Some(self.skip_labels(out_pos))
    }

    fn try_emit_while(&mut self, i: usize, end: usize, indent: usize) -> Option<usize> {
        let (cond, out_label) = match &self.flat[i].kind {
            FlatKind::IfFalse { cond, target } => (cond.clone(), target.clone()),
            _ => return None,
        };
        let out_pos = *self.label_pos.get(&out_label)?;
        if out_pos <= i || out_pos > end {
            return None;
        }
        let back_idx = self.prev_non_label(out_pos)?;
        let loop_label = match &self.flat[back_idx].kind {
            FlatKind::Goto(target) => target.clone(),
            _ => return None,
        };
        let loop_pos = *self.label_pos.get(&loop_label)?;
        if loop_pos > i {
            return None;
        }
        if !self.only_labels_and_line_markers_between(loop_pos, i) {
            return None;
        }
        self.write(indent, &format!("while ({}) {{", cond));
        let ctx = LoopCtx {
            continue_target: Some(loop_label),
            break_target: Some(out_label.clone()),
        };
        self.emit_range(i + 1, back_idx, indent + 1, &ctx);
        self.write(indent, "}");
        Some(self.skip_labels(out_pos))
    }

    fn try_emit_for(&mut self, i: usize, end: usize, indent: usize) -> Option<usize> {
        let init_label = match &self.flat[i].kind {
            FlatKind::Goto(target) => target.clone(),
            _ => return None,
        };
        let cond_label_pos = *self.label_pos.get(&init_label)?;
        if cond_label_pos <= i || cond_label_pos >= end {
            return None;
        }
        let cond_idx = self.next_non_label(cond_label_pos)?;
        let (cond, out_label) = match &self.flat[cond_idx].kind {
            FlatKind::IfFalse { cond, target } => (cond.clone(), target.clone()),
            _ => return None,
        };
        let out_pos = *self.label_pos.get(&out_label)?;
        if out_pos <= cond_idx || out_pos > end {
            return None;
        }
        let back_idx = self.prev_non_label(out_pos)?;
        let loop_label = match &self.flat[back_idx].kind {
            FlatKind::Goto(target) => target.clone(),
            _ => return None,
        };
        let loop_pos = *self.label_pos.get(&loop_label)?;
        if !(loop_pos > i && loop_pos < cond_label_pos) {
            return None;
        }
        self.write(indent, &format!("for (; {}; ) {{", cond));
        let ctx = LoopCtx {
            continue_target: Some(loop_label),
            break_target: Some(out_label.clone()),
        };
        self.emit_range(cond_idx + 1, back_idx, indent + 1, &ctx);
        if i + 1 < cond_label_pos {
            self.emit_range(i + 1, cond_label_pos, indent + 1, &ctx);
        }
        self.write(indent, "}");
        Some(self.skip_labels(out_pos))
    }

    fn try_emit_if(
        &mut self,
        i: usize,
        end: usize,
        indent: usize,
        loop_ctx: &LoopCtx,
    ) -> Option<usize> {
        let (cond, false_label) = match &self.flat[i].kind {
            FlatKind::IfFalse { cond, target } => (cond.clone(), target.clone()),
            _ => return None,
        };
        let false_pos = *self.label_pos.get(&false_label)?;
        if false_pos <= i || false_pos > end {
            return None;
        }
        if self.has_public_entry_label_between(i + 1, false_pos) {
            return None;
        }
        let then_end_idx = self.prev_non_label(false_pos)?;
        let end_label = match &self.flat[then_end_idx].kind {
            FlatKind::Goto(target) => target.clone(),
            _ => return None,
        };
        let end_pos = *self.label_pos.get(&end_label)?;
        if end_pos <= false_pos || end_pos > end {
            return None;
        }
        if self.has_public_entry_label_between(false_pos + 1, end_pos) {
            return None;
        }

        self.write(indent, &format!("if ({}) {{", cond));
        self.emit_range(i + 1, then_end_idx, indent + 1, loop_ctx);
        self.write(indent, "}");

        let else_start = self.skip_labels(false_pos);
        if else_start < end_pos {
            self.write(indent, "else {");
            self.emit_range(else_start, end_pos, indent + 1, loop_ctx);
            self.write(indent, "}");
        }
        Some(self.skip_labels(end_pos))
    }

    fn try_emit_if_no_else(
        &mut self,
        i: usize,
        end: usize,
        indent: usize,
        loop_ctx: &LoopCtx,
    ) -> Option<usize> {
        let (cond, target_label) = match &self.flat[i].kind {
            FlatKind::IfFalse { cond, target } => (cond.clone(), target.clone()),
            FlatKind::IfTrue { cond, target } => (negate_condition(cond), target.clone()),
            _ => return None,
        };
        let target_pos = *self.label_pos.get(&target_label)?;
        if target_pos <= i || target_pos > end {
            return None;
        }
        if self.has_public_entry_label_between(i + 1, target_pos) {
            return None;
        }
        let body_start = i + 1;
        if body_start >= target_pos {
            self.write(indent, &format!("if ({}) {{", cond));
            self.write(indent, "}");
            return Some(self.skip_labels(target_pos));
        }

        self.write(indent, &format!("if ({}) {{", cond));
        self.emit_range(body_start, target_pos, indent + 1, loop_ctx);
        self.write(indent, "}");
        Some(self.skip_labels(target_pos))
    }

    fn find_loop_guard_after_prefix(&self, mut i: usize, end: usize) -> Option<usize> {
        while i < end && i < self.flat.len() {
            match &self.flat[i].kind {
                FlatKind::IfFalse { .. } | FlatKind::IfTrue { .. } => return Some(i),
                FlatKind::Line(_) | FlatKind::Label(_) => i += 1,
                FlatKind::Goto(_) => return None,
            }
        }
        None
    }

    fn range_contains_executable_line(&self, start: usize, end: usize) -> bool {
        let mut i = start;
        while i < end && i < self.flat.len() {
            if let FlatKind::Line(line) = &self.flat[i].kind {
                if !is_line_marker(line) {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    fn only_labels_and_line_markers_between(&self, start: usize, end: usize) -> bool {
        let mut i = start;
        while i < end && i < self.flat.len() {
            match &self.flat[i].kind {
                FlatKind::Label(_) => {}
                FlatKind::Line(line) if is_line_marker(line) => {}
                _ => return false,
            }
            i += 1;
        }
        true
    }

    fn has_public_entry_label_between(&self, start: usize, end: usize) -> bool {
        let mut i = start;
        while i < end && i < self.flat.len() {
            if let FlatKind::Label(name) = &self.flat[i].kind {
                if is_public_entry_label(name) {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    fn skip_labels(&self, mut i: usize) -> usize {
        while i < self.flat.len() {
            match &self.flat[i].kind {
                FlatKind::Label(_) => i += 1,
                _ => break,
            }
        }
        i
    }

    fn next_non_label(&self, mut i: usize) -> Option<usize> {
        while i < self.flat.len() {
            match &self.flat[i].kind {
                FlatKind::Label(_) => i += 1,
                _ => return Some(i),
            }
        }
        None
    }

    fn prev_non_label(&self, mut i: usize) -> Option<usize> {
        if i == 0 {
            return None;
        }
        i -= 1;
        loop {
            match &self.flat[i].kind {
                FlatKind::Label(_) => {
                    if i == 0 {
                        return None;
                    }
                    i -= 1;
                }
                _ => return Some(i),
            }
        }
    }

    fn write(&mut self, indent: usize, line: &str) {
        for _ in 0..indent {
            self.out.push_str("    ");
        }
        self.out.push_str(line);
        self.out.push('\n');
    }
}
