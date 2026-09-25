use anyhow::{Result, anyhow, bail};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::ops::Range;
use std::sync::Arc;

use siglus_assets::scene_pck::{CIndex, SceneStringCodec};

/// All fields are little-endian i32 and all offsets are relative to the start of the chunk.
#[derive(Debug, Clone, Copy)]
pub struct ScnHeader {
    pub header_size: i32,
    pub scn_ofs: i32,
    pub scn_size: i32,
    pub str_index_list_ofs: i32,
    pub str_index_cnt: i32,
    pub str_list_ofs: i32,
    pub str_cnt: i32,
    pub label_list_ofs: i32,
    pub label_cnt: i32,
    pub z_label_list_ofs: i32,
    pub z_label_cnt: i32,
    pub cmd_label_list_ofs: i32,
    pub cmd_label_cnt: i32,
    pub scn_prop_list_ofs: i32,
    pub scn_prop_cnt: i32,
    pub scn_prop_name_index_list_ofs: i32,
    pub scn_prop_name_index_cnt: i32,
    pub scn_prop_name_list_ofs: i32,
    pub scn_prop_name_cnt: i32,
    pub scn_cmd_list_ofs: i32,
    pub scn_cmd_cnt: i32,
    pub scn_cmd_name_index_list_ofs: i32,
    pub scn_cmd_name_index_cnt: i32,
    pub scn_cmd_name_list_ofs: i32,
    pub scn_cmd_name_cnt: i32,
    pub call_prop_name_index_list_ofs: i32,
    pub call_prop_name_index_cnt: i32,
    pub call_prop_name_list_ofs: i32,
    pub call_prop_name_cnt: i32,
    pub namae_list_ofs: i32,
    pub namae_cnt: i32,
    pub read_flag_list_ofs: i32,
    pub read_flag_cnt: i32,
}

impl ScnHeader {
    pub fn read(chunk: &[u8]) -> Result<Self> {
        // `S_tnm_scn_header` has 33 i32 fields. We only read the early subset we need.
        let need = 33 * 4;
        if chunk.len() < need {
            bail!("scn: chunk too short for header");
        }
        let mut p = 0usize;
        let mut rd = || {
            let v = i32::from_le_bytes(chunk[p..p + 4].try_into().unwrap());
            p += 4;
            v
        };

        let header_size = rd();
        let scn_ofs = rd();
        let scn_size = rd();
        let str_index_list_ofs = rd();
        let str_index_cnt = rd();
        let str_list_ofs = rd();
        let str_cnt = rd();
        let label_list_ofs = rd();
        let label_cnt = rd();
        let z_label_list_ofs = rd();
        let z_label_cnt = rd();
        let cmd_label_list_ofs = rd();
        let cmd_label_cnt = rd();
        let scn_prop_list_ofs = rd();
        let scn_prop_cnt = rd();
        let scn_prop_name_index_list_ofs = rd();
        let scn_prop_name_index_cnt = rd();
        let scn_prop_name_list_ofs = rd();
        let scn_prop_name_cnt = rd();
        let scn_cmd_list_ofs = rd();
        let scn_cmd_cnt = rd();
        let scn_cmd_name_index_list_ofs = rd();
        let scn_cmd_name_index_cnt = rd();
        let scn_cmd_name_list_ofs = rd();
        let scn_cmd_name_cnt = rd();
        let call_prop_name_index_list_ofs = rd();
        let call_prop_name_index_cnt = rd();
        let call_prop_name_list_ofs = rd();
        let call_prop_name_cnt = rd();
        let namae_list_ofs = rd();
        let namae_cnt = rd();
        let read_flag_list_ofs = rd();
        let read_flag_cnt = rd();

        Ok(Self {
            header_size,
            scn_ofs,
            scn_size,
            str_index_list_ofs,
            str_index_cnt,
            str_cnt,
            str_list_ofs,
            label_list_ofs,
            label_cnt,
            z_label_list_ofs,
            z_label_cnt,
            cmd_label_list_ofs,
            cmd_label_cnt,
            scn_prop_list_ofs,
            scn_prop_cnt,
            scn_prop_name_index_list_ofs,
            scn_prop_name_index_cnt,
            scn_prop_name_list_ofs,
            scn_prop_name_cnt,
            scn_cmd_list_ofs,
            scn_cmd_cnt,
            scn_cmd_name_index_list_ofs,
            scn_cmd_name_index_cnt,
            scn_cmd_name_list_ofs,
            scn_cmd_name_cnt,
            call_prop_name_index_list_ofs,
            call_prop_name_index_cnt,
            call_prop_name_list_ofs,
            call_prop_name_cnt,
            namae_list_ofs,
            namae_cnt,
            read_flag_list_ofs,
            read_flag_cnt,
        })
    }
}

fn read_indexed_utf16_name_map(
    chunk: &[u8],
    index_list_ofs: usize,
    count: usize,
    list_ofs: usize,
) -> Result<std::collections::HashMap<u32, String>> {
    let mut out = std::collections::HashMap::new();
    if index_list_ofs + count * 8 > chunk.len() || list_ofs > chunk.len() {
        return Ok(out);
    }
    for i in 0..count {
        let idx = CIndex::read(chunk, index_list_ofs + i * 8)?;
        let o = idx.offset.max(0) as usize;
        let n = idx.size.max(0) as usize;
        let byte_off = list_ofs
            .checked_add(o * 2)
            .ok_or_else(|| anyhow!("scn: name offset overflow"))?;
        let byte_end = byte_off
            .checked_add(n * 2)
            .ok_or_else(|| anyhow!("scn: name size overflow"))?;
        if byte_end > chunk.len() {
            continue;
        }
        let mut u16s = Vec::with_capacity(n);
        for j in 0..n {
            let p = byte_off + j * 2;
            let w = u16::from_le_bytes([chunk[p], chunk[p + 1]]);
            if w == 0 {
                break;
            }
            u16s.push(w);
        }
        let s = String::from_utf16_lossy(&u16s);
        if !s.is_empty() {
            out.insert(i as u32, s);
        }
    }
    Ok(out)
}

fn read_i32_at(buf: &[u8], pos: &mut usize) -> Option<i32> {
    let end = pos.checked_add(4)?;
    let bytes = buf.get(*pos..end)?;
    *pos = end;
    Some(i32::from_le_bytes(bytes.try_into().ok()?))
}

fn skip_arg_form(buf: &[u8], pos: &mut usize, depth: usize) -> bool {
    // FM_LIST recursively embeds another argument-form list. Bound nesting so
    // malformed data cannot recurse indefinitely while resolving the layout.
    if depth > 64 {
        return false;
    }
    let Some(form) = read_i32_at(buf, pos) else {
        return false;
    };
    if form != crate::runtime::constants::fm::LIST {
        return true;
    }
    skip_arg_form_list(buf, pos, depth + 1)
}

fn skip_arg_form_list(buf: &[u8], pos: &mut usize, depth: usize) -> bool {
    let Some(count) = read_i32_at(buf, pos) else {
        return false;
    };
    if count < 0 {
        return false;
    }
    for _ in 0..count as usize {
        if !skip_arg_form(buf, pos, depth) {
            return false;
        }
    }
    true
}

fn read_flag_line_no_from_chunk(
    chunk: &[u8],
    header: &ScnHeader,
    read_flag_no: usize,
) -> Option<i32> {
    let count = usize::try_from(header.read_flag_cnt).ok()?;
    if read_flag_no >= count {
        return None;
    }
    let base = usize::try_from(header.read_flag_list_ofs).ok()?;
    let start = base.checked_add(read_flag_no.checked_mul(4)?)?;
    let end = start.checked_add(4)?;
    let bytes = chunk.get(start..end)?;
    Some(i32::from_le_bytes(bytes.try_into().ok()?))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
struct ReadFlagScanState {
    pc: usize,
    line_no: i32,
    next_read_flag: usize,
}

#[derive(Debug, Clone, Copy)]
struct ReadFlagScanPred {
    prev: ReadFlagScanState,
    /// `(pc, flag_no)` for a trailing command read-flag operand consumed on
    /// the edge from `prev` to this state.
    command_read_flag: Option<(usize, i32)>,
}

#[derive(Debug, Clone, Copy)]
struct ReadFlagScanNode {
    /// Number of distinct parses reaching this state, saturated at two.  We
    /// only trust a layout when exactly one complete parse exists.
    path_count: u8,
    pred: Option<ReadFlagScanPred>,
}

#[derive(Debug, Clone, Copy)]
enum ReadFlagScanKind {
    Other,
    Text(i32),
    Command,
}

fn scan_instruction(scn: &[u8], pc: usize, line_no: i32) -> Option<(usize, i32, ReadFlagScanKind)> {
    use crate::runtime::constants::cd;

    let opcode = *scn.get(pc)?;
    let mut pos = pc.checked_add(1)?;
    let mut next_line = line_no;
    let kind = match opcode {
        cd::NONE => return None,
        cd::NL => {
            next_line = read_i32_at(scn, &mut pos)?;
            ReadFlagScanKind::Other
        }
        cd::PUSH => {
            read_i32_at(scn, &mut pos)?;
            read_i32_at(scn, &mut pos)?;
            ReadFlagScanKind::Other
        }
        cd::POP | cd::COPY => {
            read_i32_at(scn, &mut pos)?;
            ReadFlagScanKind::Other
        }
        cd::PROPERTY
        | cd::COPY_ELM
        | cd::ELM_POINT
        | cd::ARG
        | cd::EOF
        | cd::NAME
        | cd::SEL_BLOCK_START
        | cd::SEL_BLOCK_END => ReadFlagScanKind::Other,
        cd::DEC_PROP => {
            read_i32_at(scn, &mut pos)?;
            read_i32_at(scn, &mut pos)?;
            ReadFlagScanKind::Other
        }
        cd::GOTO | cd::GOTO_TRUE | cd::GOTO_FALSE => {
            read_i32_at(scn, &mut pos)?;
            ReadFlagScanKind::Other
        }
        cd::GOSUB | cd::GOSUBSTR => {
            read_i32_at(scn, &mut pos)?;
            if !skip_arg_form_list(scn, &mut pos, 0) {
                return None;
            }
            ReadFlagScanKind::Other
        }
        cd::RETURN => {
            if !skip_arg_form_list(scn, &mut pos, 0) {
                return None;
            }
            ReadFlagScanKind::Other
        }
        cd::ASSIGN => {
            read_i32_at(scn, &mut pos)?;
            read_i32_at(scn, &mut pos)?;
            read_i32_at(scn, &mut pos)?;
            ReadFlagScanKind::Other
        }
        cd::OPERATE_1 => {
            read_i32_at(scn, &mut pos)?;
            scn.get(pos)?;
            pos += 1;
            ReadFlagScanKind::Other
        }
        cd::OPERATE_2 => {
            read_i32_at(scn, &mut pos)?;
            read_i32_at(scn, &mut pos)?;
            scn.get(pos)?;
            pos += 1;
            ReadFlagScanKind::Other
        }
        cd::COMMAND => {
            // The bytecode embeds arg_list_id followed by the recursively
            // encoded argument-form list. The argument *values* and element
            // chain live on the VM stack and therefore do not appear here.
            read_i32_at(scn, &mut pos)?;
            if !skip_arg_form_list(scn, &mut pos, 0) {
                return None;
            }
            let named_count = read_i32_at(scn, &mut pos)?;
            if named_count < 0 {
                return None;
            }
            let named_bytes = (named_count as usize).checked_mul(4)?;
            pos = pos.checked_add(named_bytes)?;
            if pos > scn.len() {
                return None;
            }
            // ret_form
            read_i32_at(scn, &mut pos)?;
            ReadFlagScanKind::Command
        }
        cd::TEXT => {
            let flag_no = read_i32_at(scn, &mut pos)?;
            ReadFlagScanKind::Text(flag_no)
        }
        _ => return None,
    };
    Some((pos, next_line, kind))
}

fn add_read_flag_scan_state(
    nodes: &mut HashMap<ReadFlagScanState, ReadFlagScanNode>,
    queue: &mut BinaryHeap<Reverse<ReadFlagScanState>>,
    next: ReadFlagScanState,
    incoming_paths: u8,
    pred: ReadFlagScanPred,
) {
    use std::collections::hash_map::Entry;

    let incoming_paths = incoming_paths.min(2);
    match nodes.entry(next) {
        Entry::Vacant(entry) => {
            entry.insert(ReadFlagScanNode {
                path_count: incoming_paths,
                pred: (incoming_paths == 1).then_some(pred),
            });
            queue.push(Reverse(next));
        }
        Entry::Occupied(mut entry) => {
            // Every transition advances pc, and the priority queue is ordered
            // by pc. Therefore all predecessors of `next` are processed before
            // `next` itself is popped, so no re-queue is required here.
            let node = entry.get_mut();
            let merged = node.path_count.saturating_add(incoming_paths).min(2);
            if merged != node.path_count {
                node.path_count = merged;
                if merged != 1 {
                    node.pred = None;
                }
            }
        }
    }
}

/// Resolve, for this exact scene, which CD_COMMAND instructions are followed by
/// the historical optional `read_flag_no` i32.
///
/// Siglus used more than one compiler ABI over its lifetime, and some old
/// scenes are *mixed*: e.g. KOE continues directly with the next opcode while
/// SELBTN in the same scene already carries a trailing read flag.  A global
/// old/new version bit is therefore insufficient.
///
/// The compiler's read flags give us a stronger invariant. CD_TEXT and every
/// command-level read flag consume one monotonically increasing flag number,
/// and `read_flag_list[n].line_no` records the source line that owns n. We scan
/// the complete bytecode while allowing each CD_COMMAND boundary to have either
/// zero or one trailing flag. A branch that consumes a command flag is legal
/// only when the next i32 equals the next expected flag number and its metadata
/// line matches the current CD_NL line. We accept the layout only when exactly
/// one complete parse reaches the end while consuming the entire read-flag
/// table. Otherwise we return None and the VM keeps its existing modern-ABI
/// fallback rather than guessing.
fn resolve_command_read_flags(
    scn: &[u8],
    chunk: &[u8],
    header: &ScnHeader,
) -> Option<HashMap<usize, i32>> {
    let read_flag_cnt = usize::try_from(header.read_flag_cnt).ok()?;
    let mut read_flag_lines = Vec::with_capacity(read_flag_cnt);
    for flag_no in 0..read_flag_cnt {
        read_flag_lines.push(read_flag_line_no_from_chunk(chunk, header, flag_no)?);
    }

    let start = ReadFlagScanState {
        pc: 0,
        line_no: 0,
        next_read_flag: 0,
    };
    let mut nodes = HashMap::new();
    nodes.insert(
        start,
        ReadFlagScanNode {
            path_count: 1,
            pred: None,
        },
    );
    let mut queue = BinaryHeap::new();
    queue.push(Reverse(start));

    let mut complete_paths = 0u8;
    let mut terminal = None;

    while let Some(Reverse(state)) = queue.pop() {
        let node = *nodes.get(&state)?;
        if state.pc == scn.len() {
            if state.next_read_flag == read_flag_cnt {
                complete_paths = complete_paths.saturating_add(node.path_count).min(2);
                if complete_paths == 1 && node.path_count == 1 {
                    terminal = Some(state);
                } else {
                    terminal = None;
                }
            }
            continue;
        }

        let (next_pc, next_line, kind) = match scan_instruction(scn, state.pc, state.line_no) {
            Some(v) => v,
            None => continue,
        };

        match kind {
            ReadFlagScanKind::Text(flag_no) => {
                if flag_no < 0
                    || flag_no as usize != state.next_read_flag
                    || state.next_read_flag >= read_flag_cnt
                    || read_flag_lines[state.next_read_flag] != state.line_no
                {
                    continue;
                }
                let next = ReadFlagScanState {
                    pc: next_pc,
                    line_no: next_line,
                    next_read_flag: state.next_read_flag + 1,
                };
                add_read_flag_scan_state(
                    &mut nodes,
                    &mut queue,
                    next,
                    node.path_count,
                    ReadFlagScanPred {
                        prev: state,
                        command_read_flag: None,
                    },
                );
            }
            ReadFlagScanKind::Command => {
                // Historical form: no trailing read flag.
                add_read_flag_scan_state(
                    &mut nodes,
                    &mut queue,
                    ReadFlagScanState {
                        pc: next_pc,
                        line_no: next_line,
                        next_read_flag: state.next_read_flag,
                    },
                    node.path_count,
                    ReadFlagScanPred {
                        prev: state,
                        command_read_flag: None,
                    },
                );

                // Newer form: one trailing read_flag_no. Do not infer this from
                // the bytes alone; require the globally ordered metadata too.
                if state.next_read_flag < read_flag_cnt
                    && read_flag_lines[state.next_read_flag] == state.line_no
                {
                    let mut after_flag = next_pc;
                    if read_i32_at(scn, &mut after_flag) == i32::try_from(state.next_read_flag).ok()
                    {
                        add_read_flag_scan_state(
                            &mut nodes,
                            &mut queue,
                            ReadFlagScanState {
                                pc: after_flag,
                                line_no: next_line,
                                next_read_flag: state.next_read_flag + 1,
                            },
                            node.path_count,
                            ReadFlagScanPred {
                                prev: state,
                                command_read_flag: Some((next_pc, state.next_read_flag as i32)),
                            },
                        );
                    }
                }
            }
            ReadFlagScanKind::Other => {
                add_read_flag_scan_state(
                    &mut nodes,
                    &mut queue,
                    ReadFlagScanState {
                        pc: next_pc,
                        line_no: next_line,
                        next_read_flag: state.next_read_flag,
                    },
                    node.path_count,
                    ReadFlagScanPred {
                        prev: state,
                        command_read_flag: None,
                    },
                );
            }
        }
    }

    if complete_paths != 1 {
        return None;
    }

    let mut command_read_flags = HashMap::new();
    let mut state = terminal?;
    while state != start {
        let pred = nodes.get(&state)?.pred?;
        if let Some((pc, flag_no)) = pred.command_read_flag {
            command_read_flags.insert(pc, flag_no);
        }
        state = pred.prev;
    }
    Some(command_read_flags)
}

#[derive(Debug, Clone)]
pub struct SceneStream<'a> {
    // Owned streams keep the Scene.pck backing allocation alive while these
    // slices point into it. Borrowed streams (tests/tools) leave this None.
    owned_chunk: Option<Arc<[u8]>>,
    owned_pack: Option<Arc<Vec<u8>>>,
    pub chunk: &'a [u8],
    pub header: ScnHeader,
    pub scn: &'a [u8],
    pub str_index_list: &'a [u8],
    pub str_list: &'a [u8],
    pub label_list: &'a [u8],
    pub z_label_list: &'a [u8],
    pub scn_prop_name_map: Arc<std::collections::HashMap<u32, String>>,
    pub scn_cmd_name_map: Arc<std::collections::HashMap<u32, String>>,
    pub call_prop_name_map: Arc<std::collections::HashMap<u32, String>>,
    command_read_flags: Option<Arc<HashMap<usize, i32>>>,
    pub string_codec: SceneStringCodec,
    pub pc: usize,
}

impl<'a> SceneStream<'a> {
    pub(crate) fn instruction_end(&self, pc: usize) -> Option<usize> {
        let (mut end, _, kind) = scan_instruction(self.scn, pc, -1)?;
        if matches!(kind, ReadFlagScanKind::Command)
            && self.command_read_flags.as_ref()?.contains_key(&end)
        {
            end = end.checked_add(4)?;
        }
        (end <= self.scn.len()).then_some(end)
    }

    pub fn new(chunk: &'a [u8]) -> Result<Self> {
        Self::new_with_string_codec(chunk, SceneStringCodec::Xor)
    }

    pub fn new_with_string_codec(chunk: &'a [u8], string_codec: SceneStringCodec) -> Result<Self> {
        let header = ScnHeader::read(chunk)?;
        let scn_ofs = header.scn_ofs.max(0) as usize;
        let scn_size = header.scn_size.max(0) as usize;
        let scn_end = scn_ofs
            .checked_add(scn_size)
            .ok_or_else(|| anyhow!("scn: scn_size overflow"))?;
        if scn_end > chunk.len() {
            bail!("scn: scn stream out of bounds");
        }
        let scn = &chunk[scn_ofs..scn_end];

        let str_index_list_ofs = header.str_index_list_ofs.max(0) as usize;
        let str_index_cnt = header.str_index_cnt.max(0) as usize;
        let str_index_list_end = str_index_list_ofs
            .checked_add(str_index_cnt * 8)
            .ok_or_else(|| anyhow!("scn: str_index_list overflow"))?;
        if str_index_list_end > chunk.len() {
            bail!("scn: str_index_list out of bounds");
        }
        let str_index_list = &chunk[str_index_list_ofs..str_index_list_end];

        let str_list_ofs = header.str_list_ofs.max(0) as usize;
        if str_list_ofs > chunk.len() {
            bail!("scn: str_list_ofs out of bounds");
        }
        let str_list = &chunk[str_list_ofs..];

        let label_list_ofs = header.label_list_ofs.max(0) as usize;
        let label_cnt = header.label_cnt.max(0) as usize;
        let label_list_end = label_list_ofs
            .checked_add(label_cnt * 4)
            .ok_or_else(|| anyhow!("scn: label_list overflow"))?;
        if label_list_end > chunk.len() {
            bail!("scn: label_list out of bounds");
        }
        let label_list = &chunk[label_list_ofs..label_list_end];

        let z_label_list_ofs = header.z_label_list_ofs.max(0) as usize;
        let z_label_cnt = header.z_label_cnt.max(0) as usize;
        let z_label_list_end = z_label_list_ofs
            .checked_add(z_label_cnt * 4)
            .ok_or_else(|| anyhow!("scn: z_label_list overflow"))?;
        if z_label_list_end > chunk.len() {
            bail!("scn: z_label_list out of bounds");
        }
        let z_label_list = &chunk[z_label_list_ofs..z_label_list_end];

        let scn_prop_name_map = read_indexed_utf16_name_map(
            chunk,
            header.scn_prop_name_index_list_ofs.max(0) as usize,
            header.scn_prop_name_cnt.max(0) as usize,
            header.scn_prop_name_list_ofs.max(0) as usize,
        )?;
        let scn_cmd_name_map = read_indexed_utf16_name_map(
            chunk,
            header.scn_cmd_name_index_list_ofs.max(0) as usize,
            header.scn_cmd_name_cnt.max(0) as usize,
            header.scn_cmd_name_list_ofs.max(0) as usize,
        )?;
        let call_prop_name_map = read_indexed_utf16_name_map(
            chunk,
            header.call_prop_name_index_list_ofs.max(0) as usize,
            header.call_prop_name_cnt.max(0) as usize,
            header.call_prop_name_list_ofs.max(0) as usize,
        )?;
        let command_read_flags = resolve_command_read_flags(scn, chunk, &header).map(Arc::new);

        Ok(Self {
            owned_chunk: None,
            owned_pack: None,
            chunk,
            header,
            scn,
            str_index_list,
            str_list,
            label_list,
            z_label_list,
            scn_prop_name_map: Arc::new(scn_prop_name_map),
            scn_cmd_name_map: Arc::new(scn_cmd_name_map),
            call_prop_name_map: Arc::new(call_prop_name_map),
            command_read_flags,
            string_codec,
            pc: 0,
        })
    }

    /// Build a stream backed by an owned ref-counted scene chunk.
    pub fn new_owned_with_string_codec(
        owner: Arc<[u8]>,
        string_codec: SceneStringCodec,
    ) -> Result<SceneStream<'static>> {
        let len = owner.len();
        let chunk: &'static [u8] = unsafe { std::slice::from_raw_parts(owner.as_ptr(), len) };
        let mut stream = SceneStream::new_with_string_codec(chunk, string_codec)?;
        stream.owned_chunk = Some(owner);
        Ok(stream)
    }

    /// Build a stream that borrows a range from ref-counted backing storage.
    ///
    /// The Arc allocation is immovable and is retained by the returned stream.
    /// Extending the slice lifetime to 'static is therefore safe: clones retain
    /// the same Arc, and the slice is never exposed after its owning stream is
    /// dropped. This avoids the previous Box::leak() scene lifetime workaround.
    pub fn new_shared_range_with_string_codec(
        owner: Arc<Vec<u8>>,
        range: Range<usize>,
        string_codec: SceneStringCodec,
    ) -> Result<SceneStream<'static>> {
        if range.start > range.end || range.end > owner.len() {
            bail!("scn: shared scene range out of bounds");
        }
        let ptr = owner.as_ptr();
        let len = range.end - range.start;
        // SAFETY: owner is stored in the SceneStream before this function
        // returns. Arc keeps the allocation alive across moves and clones.
        let chunk: &'static [u8] = unsafe { std::slice::from_raw_parts(ptr.add(range.start), len) };
        let mut stream = SceneStream::new_with_string_codec(chunk, string_codec)?;
        stream.owned_pack = Some(owner);
        Ok(stream)
    }

    pub fn eof(&self) -> bool {
        self.pc >= self.scn.len()
    }

    pub fn get_prg_cntr(&self) -> usize {
        self.pc
    }

    /// Raw scene-stream bytes around `pc`, for diagnostics only.
    ///
    /// A `pc` alone cannot be mapped back to a source line when it lands inside
    /// a long compiled `if/else if` chain, and the port's line table reports the
    /// *chain's* line rather than the failing instruction's. Seeing the actual
    /// opcode bytes is the only reliable way to identify the construct.
    pub fn debug_bytes_around(&self, pc: usize, before: usize, after: usize) -> (usize, Vec<u8>) {
        let start = pc.saturating_sub(before);
        let end = (pc + after).min(self.scn.len());
        if start >= end {
            return (start, Vec::new());
        }
        (start, self.scn[start..end].to_vec())
    }

    /// Scene-stream length, so a diagnostic can bound its window.
    pub fn debug_len(&self) -> usize {
        self.scn.len()
    }

    pub fn set_prg_cntr(&mut self, prg_cntr: usize) -> Result<()> {
        if prg_cntr > self.scn.len() {
            bail!("scn: prg_cntr out of bounds");
        }
        self.pc = prg_cntr;
        Ok(())
    }

    pub fn jump_to_label(&mut self, label_no: usize) -> Result<()> {
        let cnt = self.header.label_cnt.max(0) as usize;
        if label_no >= cnt {
            bail!("scn: label_no out of range");
        }
        let off = label_no * 4;
        let label_offset = i32::from_le_bytes(self.label_list[off..off + 4].try_into().unwrap());
        self.set_prg_cntr(label_offset.max(0) as usize)
    }

    pub fn jump_to_z_label(&mut self, z_no: usize) -> Result<()> {
        let cnt = self.header.z_label_cnt.max(0) as usize;
        if z_no >= cnt {
            bail!("scn: z_label out of range");
        }
        let off = z_no * 4;
        let z_offset = i32::from_le_bytes(self.z_label_list[off..off + 4].try_into().unwrap());
        self.set_prg_cntr(z_offset.max(0) as usize)
    }

    pub fn scn_cmd_offset(&self, cmd_no: usize) -> Result<usize> {
        let cnt = self.header.scn_cmd_cnt.max(0) as usize;
        if cmd_no >= cnt {
            bail!("scn: scn_cmd_no out of range");
        }
        let ofs = self.header.scn_cmd_list_ofs.max(0) as usize;
        let byte_ofs = ofs
            .checked_add(cmd_no * 4)
            .ok_or_else(|| anyhow!("scn: scn_cmd_list overflow"))?;
        let byte_end = byte_ofs
            .checked_add(4)
            .ok_or_else(|| anyhow!("scn: scn_cmd entry overflow"))?;
        if byte_end > self.chunk.len() {
            bail!("scn: scn_cmd_list out of bounds");
        }
        let cmd_offset = i32::from_le_bytes(self.chunk[byte_ofs..byte_end].try_into().unwrap());
        let prg = cmd_offset.max(0) as usize;
        if prg > self.scn.len() {
            bail!("scn: scn_cmd offset out of bounds");
        }
        Ok(prg)
    }

    pub fn next_scn_cmd_offset_after(&self, start: usize) -> Result<Option<usize>> {
        let cnt = self.header.scn_cmd_cnt.max(0) as usize;
        let mut next: Option<usize> = None;
        for cmd_no in 0..cnt {
            let off = self.scn_cmd_offset(cmd_no)?;
            if off > start {
                next = Some(match next {
                    Some(cur) => cur.min(off),
                    None => off,
                });
            }
        }
        Ok(next)
    }

    pub fn pop_u8(&mut self) -> Result<u8> {
        if self.pc + 1 > self.scn.len() {
            bail!("scn: pop_u8 past end");
        }
        let v = self.scn[self.pc];
        self.pc += 1;
        Ok(v)
    }

    pub fn pop_u16(&mut self) -> Result<u16> {
        if self.pc + 2 > self.scn.len() {
            bail!("scn: pop_u16 past end");
        }
        let v = u16::from_le_bytes(self.scn[self.pc..self.pc + 2].try_into().unwrap());
        self.pc += 2;
        Ok(v)
    }

    pub fn pop_i32(&mut self) -> Result<i32> {
        if self.pc + 4 > self.scn.len() {
            bail!("scn: pop_i32 past end");
        }
        let v = i32::from_le_bytes(self.scn[self.pc..self.pc + 4].try_into().unwrap());
        self.pc += 4;
        Ok(v)
    }

    /// True when the complete scene bytecode and read-flag table have a unique
    /// structural interpretation. If false, callers must preserve the existing
    /// modern-ABI behavior rather than infer an old layout from local bytes.
    pub fn has_resolved_command_read_flag_layout(&self) -> bool {
        self.command_read_flags.is_some()
    }

    /// Return the compiler read flag structurally stored immediately at the
    /// current program counter, if the scene layout was uniquely resolved.
    pub fn command_read_flag_no_at_current_pc(&self) -> Option<i32> {
        self.command_read_flags
            .as_ref()
            .and_then(|flags| flags.get(&self.pc).copied())
    }

    pub fn pop_str(&mut self) -> Result<String> {
        let str_id = self.pop_i32()?;
        self.get_string(str_id as usize)
    }

    pub fn get_string(&self, str_id: usize) -> Result<String> {
        let str_cnt = self.header.str_cnt.max(0) as usize;
        if str_id >= str_cnt {
            bail!("scn: str_id out of range");
        }
        let idx = CIndex::read(self.str_index_list, str_id * 8)?;
        let o = idx.offset.max(0) as usize;
        let n = idx.size.max(0) as usize;

        let byte_off = o
            .checked_mul(2)
            .ok_or_else(|| anyhow!("scn: str offset overflow"))?;
        let byte_end = byte_off
            .checked_add(n * 2)
            .ok_or_else(|| anyhow!("scn: str size overflow"))?;
        if byte_end > self.str_list.len() {
            bail!("scn: str data out of bounds");
        }

        let key = (28807u32).wrapping_mul(str_id as u32) as u16;

        let mut u16s = Vec::with_capacity(n);
        for j in 0..n {
            let p = byte_off + j * 2;
            let raw = u16::from_le_bytes([self.str_list[p], self.str_list[p + 1]]);
            let w = match self.string_codec {
                SceneStringCodec::Plain => raw,
                SceneStringCodec::Xor => raw ^ key,
            };
            if w == 0 {
                break;
            }
            u16s.push(w);
        }
        Ok(String::from_utf16_lossy(&u16s))
    }
}

#[cfg(test)]
mod read_flag_compat_tests {
    use super::{SceneStream, SceneStringCodec};
    use crate::runtime::constants::{cd, fm};
    use std::sync::Arc;

    fn push_i32(out: &mut Vec<u8>, value: i32) {
        out.extend_from_slice(&value.to_le_bytes());
    }

    fn nl(out: &mut Vec<u8>, line: i32) {
        out.push(cd::NL);
        push_i32(out, line);
    }

    fn text(out: &mut Vec<u8>, flag_no: i32) {
        out.push(cd::TEXT);
        push_i32(out, flag_no);
    }

    fn empty_command(out: &mut Vec<u8>) -> usize {
        out.push(cd::COMMAND);
        push_i32(out, 0); // arg_list_id
        push_i32(out, 0); // arg count
        push_i32(out, 0); // named arg count
        push_i32(out, fm::VOID); // ret_form
        out.len()
    }

    fn scene_with_code(code: &[u8], read_flag_lines: &[i32]) -> Vec<u8> {
        const HEADER_WORDS: usize = 33;
        const HEADER_SIZE: usize = HEADER_WORDS * 4;
        let read_flag_ofs = HEADER_SIZE + code.len();

        let mut words = [0i32; HEADER_WORDS];
        words[0] = HEADER_SIZE as i32;
        words[1] = HEADER_SIZE as i32;
        words[2] = code.len() as i32;
        for idx in [3usize, 5, 7, 9, 11, 13, 15, 17, 19, 21, 23, 25, 27, 29] {
            words[idx] = read_flag_ofs as i32;
        }
        words[31] = read_flag_ofs as i32;
        words[32] = read_flag_lines.len() as i32;

        let mut chunk = Vec::with_capacity(read_flag_ofs + read_flag_lines.len() * 4);
        for word in words {
            chunk.extend_from_slice(&word.to_le_bytes());
        }
        chunk.extend_from_slice(code);
        for line in read_flag_lines {
            chunk.extend_from_slice(&line.to_le_bytes());
        }
        chunk
    }

    #[test]
    fn legacy_pop_collision_does_not_become_command_read_flag() {
        let mut code = Vec::new();
        for (line, flag) in [(10, 0), (20, 1), (30, 2)] {
            nl(&mut code, line);
            text(&mut code, flag);
        }
        nl(&mut code, 66);
        let command_end = empty_command(&mut code);
        // Exact old-game collision: CD_POP/FM_VOID starts 03 00 00 00 00,
        // while read flag 3 also belongs to line 66. Only the global parse can
        // prove that flag 3 is actually owned by the following CD_TEXT.
        code.push(cd::POP);
        push_i32(&mut code, fm::VOID);
        text(&mut code, 3);

        let chunk = scene_with_code(&code, &[10, 20, 30, 66]);
        let mut stream = SceneStream::new(&chunk).expect("scene stream");
        assert!(stream.has_resolved_command_read_flag_layout());
        stream.set_prg_cntr(command_end).expect("seek command end");
        assert_eq!(stream.command_read_flag_no_at_current_pc(), None);
    }

    #[test]
    fn modern_command_read_flag_is_resolved_at_exact_boundary() {
        let mut code = Vec::new();
        for (line, flag) in [(10, 0), (20, 1), (30, 2)] {
            nl(&mut code, line);
            text(&mut code, flag);
        }
        nl(&mut code, 66);
        let command_end = empty_command(&mut code);
        push_i32(&mut code, 3);
        code.push(cd::POP);
        push_i32(&mut code, fm::VOID);
        nl(&mut code, 67);
        text(&mut code, 4);

        let chunk = scene_with_code(&code, &[10, 20, 30, 66, 67]);
        let mut stream = SceneStream::new(&chunk).expect("scene stream");
        assert!(stream.has_resolved_command_read_flag_layout());
        stream.set_prg_cntr(command_end).expect("seek command end");
        assert_eq!(stream.command_read_flag_no_at_current_pc(), Some(3));
    }

    #[test]
    fn mixed_scene_resolves_each_command_independently() {
        let mut code = Vec::new();
        for (line, flag) in [(10, 0), (20, 1), (30, 2)] {
            nl(&mut code, line);
            text(&mut code, flag);
        }

        nl(&mut code, 66);
        let legacy_end = empty_command(&mut code);
        code.push(cd::POP);
        push_i32(&mut code, fm::VOID);
        text(&mut code, 3);

        nl(&mut code, 67);
        let modern_end = empty_command(&mut code);
        push_i32(&mut code, 4);
        code.push(cd::POP);
        push_i32(&mut code, fm::VOID);
        nl(&mut code, 68);
        text(&mut code, 5);

        let chunk = scene_with_code(&code, &[10, 20, 30, 66, 67, 68]);
        let mut stream = SceneStream::new(&chunk).expect("scene stream");
        assert!(stream.has_resolved_command_read_flag_layout());

        stream
            .set_prg_cntr(legacy_end)
            .expect("seek legacy command end");
        assert_eq!(stream.command_read_flag_no_at_current_pc(), None);

        stream
            .set_prg_cntr(modern_end)
            .expect("seek modern command end");
        assert_eq!(stream.command_read_flag_no_at_current_pc(), Some(4));
    }

    #[test]
    fn text_only_scene_resolves_with_no_command_flags() {
        let mut code = Vec::new();
        nl(&mut code, 7);
        text(&mut code, 0);
        let chunk = scene_with_code(&code, &[7]);
        let stream = SceneStream::new(&chunk).expect("scene stream");
        assert!(stream.has_resolved_command_read_flag_layout());
        assert_eq!(stream.command_read_flag_no_at_current_pc(), None);
    }
    #[test]
    fn owned_stream_releases_backing_after_last_clone_drops() {
        let chunk = scene_with_code(&[], &[]);
        let owner: Arc<[u8]> = Arc::from(chunk.into_boxed_slice());
        assert_eq!(Arc::strong_count(&owner), 1);
        let stream = SceneStream::new_owned_with_string_codec(owner.clone(), SceneStringCodec::Xor)
            .expect("owned scene stream");
        assert_eq!(Arc::strong_count(&owner), 2);
        let cloned = stream.clone();
        assert_eq!(Arc::strong_count(&owner), 3);
        drop(stream);
        assert_eq!(Arc::strong_count(&owner), 2);
        drop(cloned);
        assert_eq!(Arc::strong_count(&owner), 1);
    }
}
