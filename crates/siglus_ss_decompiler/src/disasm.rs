use crate::constants::*;
use crate::error::{Error, Result};
use crate::reader::Reader;
use crate::scene::Scene;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Debug, Clone)]
pub struct Instruction {
    pub offset: usize,
    pub code: u8,
    pub op: Op,
}

#[derive(Debug, Clone)]
pub enum Op {
    None,
    Nl {
        line: i32,
    },
    Push {
        form: i32,
        value: i32,
    },
    Pop {
        form: i32,
    },
    Copy {
        form: i32,
    },
    Property,
    CopyElm,
    DecProp {
        form: i32,
        prop_id: i32,
    },
    ElmPoint,
    Arg,
    Goto {
        label: i32,
    },
    GotoTrue {
        label: i32,
    },
    GotoFalse {
        label: i32,
    },
    Gosub {
        label: i32,
        arg_forms: Vec<ArgForm>,
    },
    GosubStr {
        label: i32,
        arg_forms: Vec<ArgForm>,
    },
    Return {
        arg_forms: Vec<ArgForm>,
    },
    Assign {
        left_form: i32,
        right_form: i32,
        arg_list_id: i32,
    },
    Operate1 {
        form: i32,
        op: u8,
    },
    Operate2 {
        left_form: i32,
        right_form: i32,
        op: u8,
    },
    Command {
        arg_list_id: i32,
        arg_count: i32,
        arg_forms: Vec<ArgForm>,
        named_arg_ids: Vec<i32>,
        ret_form: i32,
    },
    Text {
        read_flag: i32,
    },
    Name,
    SelBlockStart,
    SelBlockEnd,
    Eof,
    Unknown {
        code: u8,
    },
}

#[derive(Debug, Clone)]
pub enum ArgForm {
    Form(i32),
    List(Vec<ArgForm>),
}

pub fn disassemble(scene: &Scene) -> Result<Vec<Instruction>> {
    disassemble_control_flow(scene)
}

pub fn disassemble_control_flow(scene: &Scene) -> Result<Vec<Instruction>> {
    let code = scene.code.as_slice();
    let mut entries = collect_entry_offsets(scene);
    let mut queue: VecDeque<usize> = entries.iter().copied().collect();
    let mut decoded: BTreeMap<usize, Instruction> = BTreeMap::new();
    let mut queued: BTreeSet<usize> = std::mem::take(&mut entries);

    while let Some(mut pc) = queue.pop_front() {
        if pc >= code.len() {
            continue;
        }

        loop {
            if pc >= code.len() || decoded.contains_key(&pc) {
                break;
            }

            let (insn, next_pc) = match decode_at(code, pc) {
                Ok(v) => v,
                Err(_) => {
                    // Stop only this control-flow path when it lands in a non-code byte range.
                    // Siglus scenes contain label-like offsets that are not executable roots;
                    // aborting the whole decompilation here rejects valid scenes.
                    break;
                }
            };
            if has_invalid_control_label(scene, &insn) {
                break;
            }
            let successors = successors(scene, &insn, next_pc);
            let terminal = is_terminal(&insn);
            decoded.insert(pc, insn);

            for target in successors.extra_targets {
                if target < code.len() && queued.insert(target) {
                    queue.push_back(target);
                }
            }

            if terminal || successors.fallthrough.is_none() {
                break;
            }
            pc = successors.fallthrough.unwrap();
        }
    }

    Ok(decoded.into_values().collect())
}

fn collect_entry_offsets(scene: &Scene) -> BTreeSet<usize> {
    let mut entries = BTreeSet::new();

    // Runtime roots are offset 0 plus user-command entries.  For audit-quality
    // decompilation we also need every compiler label table target: otherwise
    // unreferenced-but-callable branches and some reconstructed command bodies
    // disappear from the emitted .ss even though they are valid bytecode.
    entries.insert(0);

    for item in &scene.scn_cmds {
        insert_offset(&mut entries, item.offset, scene.code.len());
    }
    for &offset in &scene.labels {
        insert_offset(&mut entries, offset, scene.code.len());
    }
    for &offset in &scene.z_labels {
        insert_offset(&mut entries, offset, scene.code.len());
    }
    for item in &scene.cmd_labels {
        insert_offset(&mut entries, item.offset, scene.code.len());
    }

    entries
}

fn insert_offset(set: &mut BTreeSet<usize>, offset: i32, code_len: usize) {
    if offset < 0 {
        return;
    }
    let offset = offset as usize;
    if offset < code_len {
        set.insert(offset);
    }
}

fn has_invalid_control_label(scene: &Scene, insn: &Instruction) -> bool {
    fn invalid(scene: &Scene, label: i32) -> bool {
        if label < 0 {
            return true;
        }
        let Some(&offset) = scene.labels.get(label as usize) else {
            return true;
        };
        offset < 0 || offset as usize > scene.code.len()
    }
    match &insn.op {
        Op::Goto { label }
        | Op::GotoTrue { label }
        | Op::GotoFalse { label }
        | Op::Gosub { label, .. }
        | Op::GosubStr { label, .. } => invalid(scene, *label),
        _ => false,
    }
}

struct Successors {
    fallthrough: Option<usize>,
    extra_targets: Vec<usize>,
}

fn successors(scene: &Scene, insn: &Instruction, next_pc: usize) -> Successors {
    let mut extra_targets = Vec::new();
    let mut push_label = |label: i32| {
        if let Ok(target) = label_target(scene, label, insn.offset) {
            extra_targets.push(target);
        }
    };
    let fallthrough = match &insn.op {
        Op::Goto { label } => {
            push_label(*label);
            None
        }
        Op::GotoTrue { label } | Op::GotoFalse { label } => {
            push_label(*label);
            Some(next_pc)
        }
        Op::Gosub { label, .. } | Op::GosubStr { label, .. } => {
            push_label(*label);
            Some(next_pc)
        }
        Op::Return { .. } | Op::Eof => None,
        _ => Some(next_pc),
    };
    Successors {
        fallthrough,
        extra_targets,
    }
}

fn label_target(scene: &Scene, label: i32, insn_offset: usize) -> Result<usize> {
    if label < 0 {
        return Err(Error::new(format!(
            "negative label {label} referenced at CD offset 0x{insn_offset:X}"
        )));
    }
    let Some(&offset) = scene.labels.get(label as usize) else {
        return Err(Error::new(format!(
            "label {label} referenced at CD offset 0x{insn_offset:X} is outside label table"
        )));
    };
    if offset < 0 || offset as usize > scene.code.len() {
        return Err(Error::new(format!(
            "label {label} referenced at CD offset 0x{insn_offset:X} has invalid target 0x{offset:X}"
        )));
    }
    Ok(offset as usize)
}

fn is_terminal(insn: &Instruction) -> bool {
    matches!(insn.op, Op::Goto { .. } | Op::Return { .. } | Op::Eof)
}

fn decode_at(code: &[u8], offset: usize) -> Result<(Instruction, usize)> {
    let mut r = Reader::with_pos(code, offset)?;
    let decoded = decode_one(&mut r, offset).map_err(|e| {
        let opcode = code.get(offset).copied().unwrap_or(0);
        Error::new(format!(
            "bytecode decode failed at CD offset 0x{offset:X}: opcode=0x{opcode:02X} {} bytes=[{}]: {e}",
            cd_name(opcode),
            dump_bytes(code, offset, 64)
        ))
    })?;
    Ok((decoded, r.pos()))
}

fn decode_one(r: &mut Reader<'_>, offset: usize) -> Result<Instruction> {
    let code_byte = r.read_u8()?;
    let op = match code_byte {
        CD_NONE => Op::None,
        CD_NL => Op::Nl {
            line: r.read_i32()?,
        },
        CD_PUSH => Op::Push {
            form: r.read_i32()?,
            value: r.read_i32()?,
        },
        CD_POP => Op::Pop {
            form: r.read_i32()?,
        },
        CD_COPY => Op::Copy {
            form: r.read_i32()?,
        },
        CD_PROPERTY => Op::Property,
        CD_COPY_ELM => Op::CopyElm,
        CD_DEC_PROP => Op::DecProp {
            form: r.read_i32()?,
            prop_id: r.read_i32()?,
        },
        CD_ELM_POINT => Op::ElmPoint,
        CD_ARG => Op::Arg,
        CD_GOTO => Op::Goto {
            label: r.read_i32()?,
        },
        CD_GOTO_TRUE => Op::GotoTrue {
            label: r.read_i32()?,
        },
        CD_GOTO_FALSE => Op::GotoFalse {
            label: r.read_i32()?,
        },
        CD_GOSUB => {
            let label = r.read_i32()?;
            let arg_forms = read_arg_form_list(r, "CD_GOSUB")?;
            Op::Gosub { label, arg_forms }
        }
        CD_GOSUBSTR => {
            let label = r.read_i32()?;
            let arg_forms = read_arg_form_list(r, "CD_GOSUBSTR")?;
            Op::GosubStr { label, arg_forms }
        }
        CD_RETURN => {
            let arg_forms = read_arg_form_list(r, "CD_RETURN")?;
            Op::Return { arg_forms }
        }
        CD_ASSIGN => Op::Assign {
            left_form: r.read_i32()?,
            right_form: r.read_i32()?,
            arg_list_id: r.read_i32()?,
        },
        CD_OPERATE_1 => Op::Operate1 {
            form: r.read_i32()?,
            op: r.read_u8()?,
        },
        CD_OPERATE_2 => Op::Operate2 {
            left_form: r.read_i32()?,
            right_form: r.read_i32()?,
            op: r.read_u8()?,
        },
        CD_COMMAND => {
            let arg_list_id = r.read_i32()?;
            let arg_forms = read_arg_form_list(r, "CD_COMMAND")?;
            let arg_count_i32 = arg_forms.len() as i32;
            let named_count = r.read_i32()?;
            if named_count < 0 {
                return Err(Error::with_offset(
                    "CD_COMMAND has negative named arg count",
                    offset,
                ));
            }
            let mut named_arg_ids = Vec::with_capacity(named_count as usize);
            for _ in 0..named_count {
                named_arg_ids.push(r.read_i32()?);
            }
            let ret_form = r.read_i32()?;
            Op::Command {
                arg_list_id,
                arg_count: arg_count_i32,
                arg_forms,
                named_arg_ids,
                ret_form,
            }
        }
        CD_TEXT => Op::Text {
            read_flag: r.read_i32()?,
        },
        CD_NAME => Op::Name,
        CD_SEL_BLOCK_START => Op::SelBlockStart,
        CD_SEL_BLOCK_END => Op::SelBlockEnd,
        CD_EOF => Op::Eof,
        other => {
            return Err(Error::with_offset(
                format!("unknown bytecode opcode 0x{other:02X}; this normally means the previous instruction format is wrong or the offset is not a code entry"),
                offset,
            ));
        }
    };
    Ok(Instruction {
        offset,
        code: code_byte,
        op,
    })
}

fn read_nonnegative_count(r: &mut Reader<'_>, what: &str) -> Result<usize> {
    let v = r.read_i32()?;
    if v < 0 {
        return Err(Error::new(format!("{what} is negative: {v}")));
    }
    if v > 4096 {
        return Err(Error::new(format!("{what} is implausibly large: {v}")));
    }
    Ok(v as usize)
}

fn read_arg_form_list(r: &mut Reader<'_>, what: &str) -> Result<Vec<ArgForm>> {
    let count = read_nonnegative_count(r, &format!("{what} arg count"))?;
    let mut encoded = Vec::with_capacity(count);
    for _ in 0..count {
        encoded.push(read_arg_form(r, what)?);
    }
    encoded.reverse();
    Ok(encoded)
}

fn read_arg_form(r: &mut Reader<'_>, what: &str) -> Result<ArgForm> {
    let form = r.read_i32()?;
    if form == FM_LIST {
        let nested = read_arg_form_list(r, what)?;
        Ok(ArgForm::List(nested))
    } else {
        Ok(ArgForm::Form(form))
    }
}

fn dump_bytes(code: &[u8], offset: usize, max_len: usize) -> String {
    let end = offset.saturating_add(max_len).min(code.len());
    code.get(offset..end)
        .unwrap_or(&[])
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
