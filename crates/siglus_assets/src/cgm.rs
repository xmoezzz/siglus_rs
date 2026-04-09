//! CGM (CG table) loader.
//!
//! Parses the `.cgm` CG table format.
//!
//! The `.cgm` payload is LZSS-compressed and then obfuscated by a 256-byte XOR table.
//! Supported identifiers:
//! - `CGTABLE` : legacy table entries (name + flag_no)
//! - `CGTABLE2` : extended table entries (name + flag_no + code[5] + code_exist_cnt)

use crate::lzss;
use anyhow::{anyhow, bail, Result};
use encoding_rs::SHIFT_JIS;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub const CG_TABLE_DATA_CODE_MAX: usize = 5;
const AVG_CG_TABLE_NAME_LEN: usize = 32;

#[derive(Debug, Clone)]
pub struct CgTableEntry {
    pub name: String,
    pub flag_no: i32,
    pub code_exist_cnt: i32,
    pub code: [i32; CG_TABLE_DATA_CODE_MAX],
    pub list_no: i32,
    pub group: [i32; CG_TABLE_DATA_CODE_MAX],
}

#[derive(Debug, Clone)]
pub struct CgGroupTree {
    /// Index into `CgTableData.entries`.
    pub sub_index: usize,
    pub tree: Vec<CgGroupTree>,
}

impl CgGroupTree {
    fn new(sub_index: usize) -> Self {
        Self {
            sub_index,
            tree: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CgTableData {
    pub entries: Vec<CgTableEntry>,

    name_find: HashMap<String, usize>,
    flag_find: HashMap<i32, usize>,
    sort_list: Vec<usize>,

    /// Root group tree node.
    group_tree_root: CgGroupTree,
}

impl CgTableData {
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let mut buf = fs::read(path)?;
        let entries = expand_cgm_in_place(&mut buf)?;
        let mut out = Self {
            entries,
            name_find: HashMap::new(),
            flag_find: HashMap::new(),
            sort_list: Vec::new(),
            group_tree_root: CgGroupTree::new(0),
        };
        out.create_name_find_map();
        out.create_flag_find_map();
        out.create_sort_list();
        out.create_group_tree();
        Ok(out)
    }

    pub fn get_cg_cnt(&self) -> usize {
        self.entries.len()
    }

    pub fn get_sub_from_name(&self, name: &str) -> Option<&CgTableEntry> {
        let upper = name.to_ascii_uppercase();
        let idx = self.name_find.get(&upper).copied()?;
        self.entries.get(idx)
    }

    pub fn get_sub_from_list_no(&self, list_no: i32) -> Option<&CgTableEntry> {
        if list_no < 0 {
            return None;
        }
        self.entries.get(list_no as usize)
    }

    pub fn get_sub_from_flag_no(&self, flag_no: i32) -> Option<&CgTableEntry> {
        let idx = self.flag_find.get(&flag_no).copied()?;
        self.entries.get(idx)
    }

    /// Lookup by group code indices (gc0..gc4), matching `get_sub_pointer_from_group_code_func`.
    pub fn get_sub_from_group_code(
        &self,
        gc0: i32,
        gc1: i32,
        gc2: i32,
        gc3: i32,
        gc4: i32,
    ) -> Option<&CgTableEntry> {
        let g = self.get_group_tree_pointer(gc0, gc1, gc2, gc3, gc4)?;
        self.entries.get(g.sub_index)
    }

    /// Collect flag numbers under a group subtree (depth-first), matching `get_flag_list_func`.
    pub fn get_flag_list(&self, gc0: i32, gc1: i32, gc2: i32, gc3: i32, gc4: i32) -> Vec<i32> {
        let mut out = Vec::new();
        let Some(g) = self.get_group_tree_pointer(gc0, gc1, gc2, gc3, gc4) else {
            return out;
        };
        if g.tree.is_empty() {
            return out;
        }
        self.get_flag_list_rec(g, &mut out);
        out
    }

    fn create_name_find_map(&mut self) {
        self.name_find.clear();
        for (i, e) in self.entries.iter().enumerate() {
            // The engine uses std::map::insert which keeps the first on duplicates.
            self.name_find.entry(e.name.clone()).or_insert(i);
        }
    }

    fn create_flag_find_map(&mut self) {
        self.flag_find.clear();
        for (i, e) in self.entries.iter().enumerate() {
            self.flag_find.entry(e.flag_no).or_insert(i);
        }
    }

    fn create_sort_list(&mut self) {
        self.sort_list.clear();
        self.sort_list.extend(0..self.entries.len());
        self.sort_list.sort_by(|&a, &b| {
            let lhs = &self.entries[a];
            let rhs = &self.entries[b];
            for i in 0..CG_TABLE_DATA_CODE_MAX {
                if lhs.code[i] < rhs.code[i] {
                    return std::cmp::Ordering::Less;
                }
                if lhs.code[i] > rhs.code[i] {
                    return std::cmp::Ordering::Greater;
                }
            }
            lhs.list_no.cmp(&rhs.list_no)
        });
    }

    fn create_group_tree(&mut self) {
        if self.sort_list.is_empty() {
            self.group_tree_root = CgGroupTree::new(0);
            return;
        }

        // IMPORTANT: build into a local root first.
        //
        // The naive translation from the original implementation tends to call something like:
        //   self.create_group_tree_rec(&mut self.group_tree_root, ...)
        // which triggers E0499 (multiple mutable borrows of `self`).
        //
        // Using a local `root` allows us to take disjoint borrows of `entries` and
        // `sort_list` without ever holding `&mut self` across the recursive call.
        let mut root = CgGroupTree::new(self.sort_list[0]);
        let mut code = [-1i32; CG_TABLE_DATA_CODE_MAX];
        let entries: &mut [CgTableEntry] = &mut self.entries;
        let sort_list: &[usize] = &self.sort_list;
        Self::create_group_tree_rec(entries, sort_list, &mut root, 0, &mut code, 0);
        self.group_tree_root = root;
    }

    fn create_group_tree_rec(
        entries: &mut [CgTableEntry],
        sort_list: &[usize],
        node: &mut CgGroupTree,
        sort_list_index: usize,
        code: &mut [i32; CG_TABLE_DATA_CODE_MAX],
        code_index: usize,
    ) {
        let sort_list_index_backup = sort_list_index;

        // Determine the span in sort_list for the current prefix code[0..code_index).
        let mut end = sort_list_index;
        while end < sort_list.len() {
            let sub = &entries[sort_list[end]];
            let mut loop_out = false;
            for i in 0..code_index {
                if sub.code[i] != code[i] {
                    loop_out = true;
                    break;
                }
            }
            if loop_out {
                break;
            }
            end += 1;
        }

        if sort_list_index_backup >= end {
            return;
        }

        // Count groups at this level.
        let mut group_cnt = 0usize;
        let mut now_code = -1i32;
        for idx in sort_list_index_backup..end {
            let sub = &entries[sort_list[idx]];
            if sub.code[code_index] != now_code || code_index == (CG_TABLE_DATA_CODE_MAX - 1) {
                now_code = sub.code[code_index];
                group_cnt += 1;
            }
        }
        if group_cnt == 0 {
            return;
        }

        node.tree.clear();
        node.tree.reserve(group_cnt);

        // Match engine: node.sub points to the first element in this span.
        node.sub_index = sort_list[sort_list_index_backup];

        // Second pass: build children and assign group indices to entries.
        let mut now_code = -1i32;
        let mut groupe_no = 0i32;
        let mut tree_index = 0usize;
        let mut idx = sort_list_index_backup;
        while idx < end {
            let sub_idx = sort_list[idx];
            let sub_code = entries[sub_idx].code[code_index];
            if sub_code != now_code || code_index == (CG_TABLE_DATA_CODE_MAX - 1) {
                if now_code != -1 {
                    groupe_no += 1;
                }
                now_code = sub_code;

                // Ensure child exists.
                node.tree.push(CgGroupTree::new(sub_idx));

                if code_index + 1 < CG_TABLE_DATA_CODE_MAX {
                    code[code_index] = now_code;
                    let child = node.tree.get_mut(tree_index).unwrap();
                    Self::create_group_tree_rec(
                        entries,
                        sort_list,
                        child,
                        idx,
                        code,
                        code_index + 1,
                    );
                }

                // Match engine: child.sub points to the first element of its span.
                node.tree[tree_index].sub_index = sub_idx;
                tree_index += 1;
            }

            entries[sub_idx].group[code_index] = groupe_no;
            idx += 1;
        }
    }

    /// Match `get_groupe_tree_pointer_func` / `get_groupe_tree_pointer_funcfunc`.

    fn get_group_tree_pointer(
        &self,
        gc0: i32,
        gc1: i32,
        gc2: i32,
        gc3: i32,
        gc4: i32,
    ) -> Option<&CgGroupTree> {
        if self.group_tree_root.tree.is_empty() {
            return None;
        }
        let code = [gc0, gc1, gc2, gc3, gc4];
        // SiglusEngine starts from `&cg_table_group_tree.tree[0]`.
        Self::get_group_tree_pointer_rec(self.group_tree_root.tree.get(0)?, &code, 0)
    }

    fn get_group_tree_pointer_rec<'a>(
        group: &'a CgGroupTree,
        // NOTE: do NOT tie the lifetime of `code` to `'a`. `code` is often a
        // local temporary array in the caller; if it shares `'a` with `group`,
        // Rust will infer the returned reference is bounded by the shorter
        // temporary lifetime, causing "lifetime may not live long enough".
        code: &[i32; CG_TABLE_DATA_CODE_MAX],
        code_index: usize,
    ) -> Option<&'a CgGroupTree> {
        let gc = code[code_index];
        if gc == -1 {
            return Some(group);
        }
        if gc < 0 {
            return None;
        }
        let gc_u = gc as usize;
        if gc_u >= group.tree.len() {
            return None;
        }
        if code_index + 1 >= CG_TABLE_DATA_CODE_MAX {
            return group.tree.get(gc_u);
        }
        if code[code_index + 1] == -1 {
            return group.tree.get(gc_u);
        }
        let child = group.tree.get(gc_u)?;
        Self::get_group_tree_pointer_rec(child, code, code_index + 1)
    }

    fn get_flag_list_rec(&self, group: &CgGroupTree, out: &mut Vec<i32>) {
        if group.tree.len() <= 1 {
            out.push(self.entries[group.sub_index].flag_no);
            return;
        }
        for child in &group.tree {
            self.get_flag_list_rec(child, out);
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AvgCgTableHeader {
    head: [u8; 16],
    cnt: i32,
    auto_flag: i32,
    rev0: i32,
    rev1: i32,
}

fn expand_cgm_in_place(buf: &mut [u8]) -> Result<Vec<CgTableEntry>> {
    if buf.len() < 16 + 4 * 4 {
        bail!("CGM: input too short");
    }

    let mut off = 0usize;
    let mut head = [0u8; 16];
    head.copy_from_slice(&buf[0..16]);
    off += 16;

    let cnt = i32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
    off += 4;
    let auto_flag = i32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
    off += 4;
    let rev0 = i32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
    off += 4;
    let rev1 = i32::from_le_bytes(buf[off..off + 4].try_into().unwrap());
    off += 4;

    let header = AvgCgTableHeader {
        head,
        cnt,
        auto_flag,
        rev0,
        rev1,
    };

    if header.cnt <= 0 {
        bail!("CGM: invalid cnt={}", header.cnt);
    }

    let ident = c_string_prefix(&header.head);

    let wp = &mut buf[off..];
    tpc_angou_in_place(wp);

    let expand_data = lzss::lzss_unpack_lenient(wp)?;

    match ident.as_str() {
        "CGTABLE2" => parse_table2(&expand_data, header.cnt as usize),
        "CGTABLE" => parse_table1(&expand_data, header.cnt as usize),
        _ => bail!("CGM: unsupported identifier: {ident}"),
    }
}

fn parse_table2(expand_data: &[u8], cnt: usize) -> Result<Vec<CgTableEntry>> {
    let entry_size = AVG_CG_TABLE_NAME_LEN + 4 + (CG_TABLE_DATA_CODE_MAX * 4) + 4;
    let need = cnt
        .checked_mul(entry_size)
        .ok_or_else(|| anyhow!("CGM: size overflow"))?;
    if expand_data.len() < need {
        bail!(
            "CGM: expanded data too short (need={}, got={})",
            need,
            expand_data.len()
        );
    }

    let mut out = Vec::with_capacity(cnt);
    let mut off = 0usize;
    for i in 0..cnt {
        let name_raw = &expand_data[off..off + AVG_CG_TABLE_NAME_LEN];
        off += AVG_CG_TABLE_NAME_LEN;

        let flag_no = i32::from_le_bytes(expand_data[off..off + 4].try_into().unwrap());
        off += 4;

        let mut code = [0i32; CG_TABLE_DATA_CODE_MAX];
        for j in 0..CG_TABLE_DATA_CODE_MAX {
            code[j] = i32::from_le_bytes(expand_data[off..off + 4].try_into().unwrap());
            off += 4;
        }

        let code_exist_cnt = i32::from_le_bytes(expand_data[off..off + 4].try_into().unwrap());
        off += 4;

        let mut name = decode_name(name_raw);
        name = name.to_ascii_uppercase();

        out.push(CgTableEntry {
            name,
            flag_no,
            code_exist_cnt,
            code,
            list_no: i as i32,
            group: [-1; CG_TABLE_DATA_CODE_MAX],
        });
    }
    Ok(out)
}

fn parse_table1(expand_data: &[u8], cnt: usize) -> Result<Vec<CgTableEntry>> {
    let entry_size = AVG_CG_TABLE_NAME_LEN + 4;
    let need = cnt
        .checked_mul(entry_size)
        .ok_or_else(|| anyhow!("CGM: size overflow"))?;
    if expand_data.len() < need {
        bail!(
            "CGM: expanded data too short (need={}, got={})",
            need,
            expand_data.len()
        );
    }

    let mut out = Vec::with_capacity(cnt);
    let mut off = 0usize;
    for i in 0..cnt {
        let name_raw = &expand_data[off..off + AVG_CG_TABLE_NAME_LEN];
        off += AVG_CG_TABLE_NAME_LEN;

        let flag_no = i32::from_le_bytes(expand_data[off..off + 4].try_into().unwrap());
        off += 4;

        let mut name = decode_name(name_raw);
        name = name.to_ascii_uppercase();

        out.push(CgTableEntry {
            name,
            flag_no,
            code_exist_cnt: 0,
            code: [0; CG_TABLE_DATA_CODE_MAX],
            list_no: i as i32,
            group: [-1; CG_TABLE_DATA_CODE_MAX],
        });
    }
    Ok(out)
}

fn decode_name(name_raw: &[u8]) -> String {
    let end = name_raw
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(name_raw.len());
    let (cow, _, _) = SHIFT_JIS.decode(&name_raw[..end]);
    cow.into_owned()
}

fn c_string_prefix(buf: &[u8]) -> String {
    let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    let (cow, _, _) = SHIFT_JIS.decode(&buf[..end]);
    cow.into_owned()
}

/// `tpc_angou` is XOR with a 256-byte repeating table.
fn tpc_angou_in_place(src: &mut [u8]) {
    for (i, b) in src.iter_mut().enumerate() {
        *b ^= TPC_ANGOU_TABLE[i & 0xFF];
    }
}

// Angou XOR table (256 bytes, repeats).
const TPC_ANGOU_TABLE: [u8; 256] = [
    0x8b, 0xe5, 0x5d, 0xc3, 0xa1, 0xe0, 0x30, 0x44, 0x00, 0x85, 0xc0, 0x74, 0x09, 0x5f, 0x5e, 0x33,
    0xc0, 0x5b, 0x8b, 0xe5, 0x5d, 0xc3, 0x8b, 0x45, 0x0c, 0x85, 0xc0, 0x75, 0x14, 0x8b, 0x55, 0xec,
    0x83, 0xc2, 0x20, 0x52, 0x6a, 0x00, 0xe8, 0xf5, 0x28, 0x01, 0x00, 0x83, 0xc4, 0x08, 0x89, 0x45,
    0x0c, 0x8b, 0x45, 0xe4, 0x6a, 0x00, 0x6a, 0x00, 0x50, 0x53, 0xff, 0x15, 0x34, 0xb1, 0x43, 0x00,
    0x8b, 0x45, 0x10, 0x85, 0xc0, 0x74, 0x05, 0x8b, 0x4d, 0xec, 0x89, 0x08, 0x8a, 0x45, 0xf0, 0x84,
    0xc0, 0x75, 0x78, 0xa1, 0xe0, 0x30, 0x44, 0x00, 0x8b, 0x7d, 0xe8, 0x8b, 0x75, 0x0c, 0x85, 0xc0,
    0x75, 0x44, 0x8b, 0x1d, 0xd0, 0xb0, 0x43, 0x00, 0x85, 0xff, 0x76, 0x37, 0x81, 0xff, 0x00, 0x00,
    0x04, 0x00, 0x6a, 0x00, 0x76, 0x43, 0x8b, 0x45, 0xf8, 0x8d, 0x55, 0xfc, 0x52, 0x68, 0x00, 0x00,
    0x04, 0x00, 0x56, 0x50, 0xff, 0x15, 0x2c, 0xb1, 0x43, 0x00, 0x6a, 0x05, 0xff, 0xd3, 0xa1, 0xe0,
    0x30, 0x44, 0x00, 0x81, 0xef, 0x00, 0x00, 0x04, 0x00, 0x81, 0xc6, 0x00, 0x00, 0x04, 0x00, 0x85,
    0xc0, 0x74, 0xc5, 0x8b, 0x5d, 0xf8, 0x53, 0xe8, 0xf4, 0xfb, 0xff, 0xff, 0x8b, 0x45, 0x0c, 0x83,
    0xc4, 0x04, 0x5f, 0x5e, 0x5b, 0x8b, 0xe5, 0x5d, 0xc3, 0x8b, 0x55, 0xf8, 0x8d, 0x4d, 0xfc, 0x51,
    0x57, 0x56, 0x52, 0xff, 0x15, 0x2c, 0xb1, 0x43, 0x00, 0xeb, 0xd8, 0x8b, 0x45, 0xe8, 0x83, 0xc0,
    0x20, 0x50, 0x6a, 0x00, 0xe8, 0x47, 0x28, 0x01, 0x00, 0x8b, 0x7d, 0xe8, 0x89, 0x45, 0xf4, 0x8b,
    0xf0, 0xa1, 0xe0, 0x30, 0x44, 0x00, 0x83, 0xc4, 0x08, 0x85, 0xc0, 0x75, 0x56, 0x8b, 0x1d, 0xd0,
    0xb0, 0x43, 0x00, 0x85, 0xff, 0x76, 0x49, 0x81, 0xff, 0x00, 0x00, 0x04, 0x00, 0x6a, 0x00, 0x76,
];
