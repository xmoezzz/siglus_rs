use anyhow::{anyhow, bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::runtime::globals::SaveSlotState;

pub const SAVE_APPEND_DIR_MAX_LEN: usize = 256;
pub const SAVE_APPEND_NAME_MAX_LEN: usize = 256;
pub const SAVE_TITLE_MAX_LEN: usize = 256;
pub const SAVE_MESSAGE_MAX_LEN: usize = 256;
pub const SAVE_FULL_MESSAGE_MAX_LEN: usize = 256;
pub const SAVE_COMMENT_MAX_LEN: usize = 256;
pub const SAVE_COMMENT2_MAX_LEN: usize = 256;
pub const SAVE_FLAG_MAX_CNT: usize = 256;

const SAVE_FIXED_STRING_CNT: usize = 7;
pub const SAVE_HEADER_SIZE: usize = 10 * 4 + SAVE_FIXED_STRING_CNT * 256 * 2 + SAVE_FLAG_MAX_CNT * 4 + 4;
pub const GLOBAL_SAVE_HEADER_SIZE: usize = 12;
pub const CONFIG_SAVE_HEADER_SIZE: usize = 12;
pub const READ_SAVE_HEADER_SIZE: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveKind {
    Normal,
    Quick,
    End,
}

#[derive(Debug, Clone)]
pub struct OriginalSaveHeader {
    pub major_version: i32,
    pub minor_version: i32,
    pub year: i32,
    pub month: i32,
    pub day: i32,
    pub weekday: i32,
    pub hour: i32,
    pub minute: i32,
    pub second: i32,
    pub millisecond: i32,
    pub append_dir: String,
    pub append_name: String,
    pub title: String,
    pub message: String,
    pub full_message: String,
    pub comment: String,
    pub comment2: String,
    pub flag: [i32; SAVE_FLAG_MAX_CNT],
    pub data_size: i32,
}

impl Default for OriginalSaveHeader {
    fn default() -> Self {
        Self {
            major_version: 1,
            minor_version: 0,
            year: 0,
            month: 0,
            day: 0,
            weekday: 0,
            hour: 0,
            minute: 0,
            second: 0,
            millisecond: 0,
            append_dir: String::new(),
            append_name: String::new(),
            title: String::new(),
            message: String::new(),
            full_message: String::new(),
            comment: String::new(),
            comment2: String::new(),
            flag: [0; SAVE_FLAG_MAX_CNT],
            data_size: 0,
        }
    }
}

impl OriginalSaveHeader {
    pub fn from_slot(slot: &SaveSlotState, packed_size: usize) -> Self {
        let mut flag = [0i32; SAVE_FLAG_MAX_CNT];
        for (idx, dst) in flag.iter_mut().enumerate() {
            *dst = slot.values.get(&(idx as i32)).copied().unwrap_or(0) as i32;
        }
        Self {
            major_version: 1,
            minor_version: 0,
            year: slot.year as i32,
            month: slot.month as i32,
            day: slot.day as i32,
            weekday: slot.weekday as i32,
            hour: slot.hour as i32,
            minute: slot.minute as i32,
            second: slot.second as i32,
            millisecond: slot.millisecond as i32,
            append_dir: slot.append_dir.clone(),
            append_name: slot.append_name.clone(),
            title: slot.title.clone(),
            message: slot.message.clone(),
            full_message: slot.full_message.clone(),
            comment: slot.comment.clone(),
            comment2: String::new(),
            flag,
            data_size: packed_size as i32,
        }
    }

    pub fn to_slot(&self) -> SaveSlotState {
        let mut slot = SaveSlotState::default();
        slot.exist = self.major_version == 1 && self.minor_version == 0;
        slot.year = self.year as i64;
        slot.month = self.month as i64;
        slot.day = self.day as i64;
        slot.weekday = self.weekday as i64;
        slot.hour = self.hour as i64;
        slot.minute = self.minute as i64;
        slot.second = self.second as i64;
        slot.millisecond = self.millisecond as i64;
        slot.append_dir = self.append_dir.clone();
        slot.append_name = self.append_name.clone();
        slot.title = self.title.clone();
        slot.message = self.message.clone();
        slot.full_message = self.full_message.clone();
        slot.comment = self.comment.clone();
        for (idx, value) in self.flag.iter().enumerate() {
            if *value != 0 {
                slot.values.insert(idx as i32, *value as i64);
            }
        }
        slot
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < SAVE_HEADER_SIZE {
            bail!("save header too short: {} < {}", bytes.len(), SAVE_HEADER_SIZE);
        }
        let mut rd = Reader::new(bytes);
        let major_version = rd.i32()?;
        let minor_version = rd.i32()?;
        let year = rd.i32()?;
        let month = rd.i32()?;
        let day = rd.i32()?;
        let weekday = rd.i32()?;
        let hour = rd.i32()?;
        let minute = rd.i32()?;
        let second = rd.i32()?;
        let millisecond = rd.i32()?;
        let append_dir = rd.utf16_fixed(SAVE_APPEND_DIR_MAX_LEN)?;
        let append_name = rd.utf16_fixed(SAVE_APPEND_NAME_MAX_LEN)?;
        let title = rd.utf16_fixed(SAVE_TITLE_MAX_LEN)?;
        let message = rd.utf16_fixed(SAVE_MESSAGE_MAX_LEN)?;
        let full_message = rd.utf16_fixed(SAVE_FULL_MESSAGE_MAX_LEN)?;
        let comment = rd.utf16_fixed(SAVE_COMMENT_MAX_LEN)?;
        let comment2 = rd.utf16_fixed(SAVE_COMMENT2_MAX_LEN)?;
        let mut flag = [0i32; SAVE_FLAG_MAX_CNT];
        for dst in &mut flag {
            *dst = rd.i32()?;
        }
        let data_size = rd.i32()?;
        Ok(Self {
            major_version,
            minor_version,
            year,
            month,
            day,
            weekday,
            hour,
            minute,
            second,
            millisecond,
            append_dir,
            append_name,
            title,
            message,
            full_message,
            comment,
            comment2,
            flag,
            data_size,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(SAVE_HEADER_SIZE);
        push_i32(&mut out, self.major_version);
        push_i32(&mut out, self.minor_version);
        push_i32(&mut out, self.year);
        push_i32(&mut out, self.month);
        push_i32(&mut out, self.day);
        push_i32(&mut out, self.weekday);
        push_i32(&mut out, self.hour);
        push_i32(&mut out, self.minute);
        push_i32(&mut out, self.second);
        push_i32(&mut out, self.millisecond);
        push_utf16_fixed(&mut out, &self.append_dir, SAVE_APPEND_DIR_MAX_LEN);
        push_utf16_fixed(&mut out, &self.append_name, SAVE_APPEND_NAME_MAX_LEN);
        push_utf16_fixed(&mut out, &self.title, SAVE_TITLE_MAX_LEN);
        push_utf16_fixed(&mut out, &self.message, SAVE_MESSAGE_MAX_LEN);
        push_utf16_fixed(&mut out, &self.full_message, SAVE_FULL_MESSAGE_MAX_LEN);
        push_utf16_fixed(&mut out, &self.comment, SAVE_COMMENT_MAX_LEN);
        push_utf16_fixed(&mut out, &self.comment2, SAVE_COMMENT2_MAX_LEN);
        for v in &self.flag {
            push_i32(&mut out, *v);
        }
        push_i32(&mut out, self.data_size);
        debug_assert_eq!(out.len(), SAVE_HEADER_SIZE);
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OriginalGlobalSaveHeader {
    pub major_version: i32,
    pub minor_version: i32,
    pub global_data_size: i32,
}

impl OriginalGlobalSaveHeader {
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < GLOBAL_SAVE_HEADER_SIZE {
            bail!("global save header too short: {} < {}", bytes.len(), GLOBAL_SAVE_HEADER_SIZE);
        }
        let mut rd = Reader::new(bytes);
        Ok(Self {
            major_version: rd.i32()?,
            minor_version: rd.i32()?,
            global_data_size: rd.i32()?,
        })
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(GLOBAL_SAVE_HEADER_SIZE);
        push_i32(&mut out, self.major_version);
        push_i32(&mut out, self.minor_version);
        push_i32(&mut out, self.global_data_size);
        out
    }
}

#[derive(Debug, Clone)]
pub struct OriginalLocalSaveEnvelope {
    pub save_id: [u16; 7],
    pub append_dir: String,
    pub append_name: String,
    pub title: String,
    pub message: String,
    pub full_message: String,
    pub local_stream: Vec<u8>,
    pub local_ex_stream: Vec<u8>,
    pub sel_saves: Vec<OriginalLocalSaveEnvelope>,
}

impl OriginalLocalSaveEnvelope {
    pub fn from_slot_with_streams(slot: &SaveSlotState, local_stream: Vec<u8>, local_ex_stream: Vec<u8>) -> Self {
        Self {
            save_id: save_id_from_slot(slot),
            append_dir: slot.append_dir.clone(),
            append_name: slot.append_name.clone(),
            title: slot.title.clone(),
            message: slot.message.clone(),
            full_message: slot.full_message.clone(),
            local_stream,
            local_ex_stream,
            sel_saves: Vec::new(),
        }
    }

    pub fn empty_from_slot(slot: &SaveSlotState) -> Self {
        Self::from_slot_with_streams(slot, Vec::new(), Vec::new())
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        write_envelope(&mut out, self, true);
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut rd = Reader::new(bytes);
        read_envelope(&mut rd, true)
    }
}


fn save_id_from_slot(slot: &SaveSlotState) -> [u16; 7] {
    fn w(v: i64) -> u16 {
        v.clamp(0, u16::MAX as i64) as u16
    }
    [
        w(slot.year),
        w(slot.month),
        w(slot.day),
        w(slot.hour),
        w(slot.minute),
        w(slot.second),
        w(slot.millisecond),
    ]
}

fn write_envelope(out: &mut Vec<u8>, env: &OriginalLocalSaveEnvelope, include_sel_saves: bool) {
    for v in env.save_id {
        push_u16(out, v);
    }
    push_str_len(out, &env.append_dir);
    push_str_len(out, &env.append_name);
    push_str_len(out, &env.title);
    push_str_len(out, &env.message);
    push_str_len(out, &env.full_message);
    push_i32(out, env.local_stream.len() as i32);
    out.extend_from_slice(&env.local_stream);
    push_i32(out, env.local_ex_stream.len() as i32);
    out.extend_from_slice(&env.local_ex_stream);
    if include_sel_saves {
        push_i32(out, env.sel_saves.len() as i32);
        for child in &env.sel_saves {
            write_envelope(out, child, false);
        }
    }
}

fn read_envelope(rd: &mut Reader<'_>, include_sel_saves: bool) -> Result<OriginalLocalSaveEnvelope> {
    let mut save_id = [0u16; 7];
    for dst in &mut save_id {
        *dst = rd.u16()?;
    }
    let append_dir = rd.str_len()?;
    let append_name = rd.str_len()?;
    let title = rd.str_len()?;
    let message = rd.str_len()?;
    let full_message = rd.str_len()?;
    let local_size = rd.i32()?.max(0) as usize;
    let local_stream = rd.take(local_size)?.to_vec();
    let local_ex_size = rd.i32()?.max(0) as usize;
    let local_ex_stream = rd.take(local_ex_size)?.to_vec();
    let mut sel_saves = Vec::new();
    if include_sel_saves {
        let sel_save_cnt = rd.i32()?.max(0) as usize;
        for _ in 0..sel_save_cnt {
            sel_saves.push(read_envelope(rd, false)?);
        }
    }
    Ok(OriginalLocalSaveEnvelope {
        save_id,
        append_dir,
        append_name,
        title,
        message,
        full_message,
        local_stream,
        local_ex_stream,
        sel_saves,
    })
}

pub struct OriginalStreamWriter {
    data: Vec<u8>,
}

impl OriginalStreamWriter {
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    pub fn into_inner(self) -> Vec<u8> {
        self.data
    }

    pub fn push_i32(&mut self, v: i32) {
        push_i32(&mut self.data, v);
    }

    pub fn push_i64(&mut self, v: i64) {
        push_i64(&mut self.data, v);
    }

    pub fn push_u32(&mut self, v: u32) {
        push_u32(&mut self.data, v);
    }

    pub fn push_raw(&mut self, bytes: &[u8]) {
        self.data.extend_from_slice(bytes);
    }

    pub fn position(&self) -> usize {
        self.data.len()
    }

    pub fn push_bool(&mut self, v: bool) {
        self.data.push(if v { 1 } else { 0 });
    }

    pub fn push_padding(&mut self, n: usize) {
        self.data.resize(self.data.len() + n, 0);
    }

    pub fn push_element(&mut self, codes: &[i32]) {
        for idx in 0..31 {
            push_i32(&mut self.data, codes.get(idx).copied().unwrap_or(0));
        }
        push_i32(&mut self.data, codes.len().min(31) as i32);
    }

    pub fn push_empty_element(&mut self) {
        self.push_element(&[]);
    }

    pub fn push_empty_proc(&mut self) {
        self.push_i32(0);
        self.push_empty_element();
        self.push_i32(0);
        self.push_i32(0);
        self.push_bool(false);
        self.push_bool(false);
        self.push_bool(false);
        self.push_i32(0);
    }

    pub fn push_len_bytes(&mut self, bytes: &[u8]) {
        push_i32(&mut self.data, bytes.len() as i32);
        self.data.extend_from_slice(bytes);
    }

    pub fn push_str(&mut self, s: &str) {
        push_str_len(&mut self.data, s);
    }

    pub fn push_fixed_i32_list(&mut self, values: &[i64], fixed_len: usize) {
        let jump_pos = self.data.len();
        push_i32(&mut self.data, 0);
        push_i32(&mut self.data, fixed_len as i32);
        for idx in 0..fixed_len {
            push_i32(&mut self.data, values.get(idx).copied().unwrap_or(0) as i32);
        }
        let end = self.data.len() as i32;
        patch_i32(&mut self.data, jump_pos, end);
    }

    pub fn push_fixed_str_list(&mut self, values: &[String], fixed_len: usize) {
        let jump_pos = self.data.len();
        push_i32(&mut self.data, 0);
        push_i32(&mut self.data, fixed_len as i32);
        for idx in 0..fixed_len {
            push_str_len(&mut self.data, values.get(idx).map(String::as_str).unwrap_or(""));
        }
        let end = self.data.len() as i32;
        patch_i32(&mut self.data, jump_pos, end);
    }

    pub fn push_extend_i32_list(&mut self, values: &[i64]) {
        push_i32(&mut self.data, values.len() as i32);
        for v in values {
            push_i32(&mut self.data, *v as i32);
        }
    }

    pub fn push_extend_str_list(&mut self, values: &[String]) {
        push_i32(&mut self.data, values.len() as i32);
        for v in values {
            push_str_len(&mut self.data, v);
        }
    }

    pub fn push_tid(&mut self, tid: &[u16; 7]) {
        for v in tid {
            push_u16(&mut self.data, *v);
        }
    }

    pub fn push_empty_fixed_array(&mut self) {
        let jump_pos = self.data.len();
        push_i32(&mut self.data, 0);
        push_i32(&mut self.data, 0);
        let end = self.data.len() as i32;
        patch_i32(&mut self.data, jump_pos, end);
    }

    pub fn push_fixed_items<T, F>(&mut self, values: &[T], mut write_one: F)
    where
        F: FnMut(&mut OriginalStreamWriter, &T),
    {
        let jump_pos = self.data.len();
        push_i32(&mut self.data, 0);
        push_i32(&mut self.data, values.len() as i32);
        for value in values {
            write_one(self, value);
        }
        let end = self.data.len() as i32;
        patch_i32(&mut self.data, jump_pos, end);
    }

    pub fn push_extend_items<T, F>(&mut self, values: &[T], mut write_one: F)
    where
        F: FnMut(&mut OriginalStreamWriter, &T),
    {
        push_i32(&mut self.data, values.len() as i32);
        for value in values {
            write_one(self, value);
        }
    }

    pub fn push_tid_zero(&mut self) {
        self.push_padding(14);
    }
}

pub struct OriginalStreamReader<'a> {
    rd: Reader<'a>,
}

impl<'a> OriginalStreamReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { rd: Reader::new(data) }
    }

    pub fn i32(&mut self) -> Result<i32> {
        self.rd.i32()
    }

    pub fn i64(&mut self) -> Result<i64> {
        self.rd.i64()
    }

    pub fn bool(&mut self) -> Result<bool> {
        Ok(self.rd.u8()? != 0)
    }

    pub fn skip(&mut self, n: usize) -> Result<()> {
        self.rd.take(n).map(|_| ())
    }

    pub fn element(&mut self) -> Result<Vec<i32>> {
        let mut codes = [0i32; 31];
        for dst in &mut codes {
            *dst = self.rd.i32()?;
        }
        let cnt = self.rd.i32()?.clamp(0, 31) as usize;
        Ok(codes[..cnt].to_vec())
    }

    pub fn skip_element(&mut self) -> Result<()> {
        self.skip(31 * 4 + 4)
    }

    pub fn skip_empty_proc(&mut self) -> Result<()> {
        let _ = self.i32()?;
        self.skip_element()?;
        let _ = self.i32()?;
        let _ = self.i32()?;
        let _ = self.bool()?;
        let _ = self.bool()?;
        let _ = self.bool()?;
        let _ = self.i32()?;
        Ok(())
    }

    pub fn tid(&mut self) -> Result<[u16; 7]> {
        let mut out = [0u16; 7];
        for v in &mut out {
            *v = self.rd.u16()?;
        }
        Ok(out)
    }

    pub fn take_raw(&mut self, n: usize) -> Result<&'a [u8]> {
        self.rd.take(n)
    }

    pub fn len_bytes(&mut self) -> Result<Vec<u8>> {
        let n = self.rd.i32()?;
        if n < 0 {
            bail!("negative byte length {}", n);
        }
        Ok(self.rd.take(n as usize)?.to_vec())
    }

    pub fn remaining(&self) -> &'a [u8] {
        &self.rd.data[self.rd.pos..]
    }

    pub fn string(&mut self) -> Result<String> {
        self.rd.str_len()
    }

    fn finish_fixed_array(&mut self, jump: i32) -> Result<()> {
        if jump < 0 {
            bail!("negative fixed-array jump {}", jump);
        }
        let jump = jump as usize;
        if jump > self.rd.data.len() {
            bail!("fixed-array jump out of bounds: jump {}, stream {}", jump, self.rd.data.len());
        }
        if self.rd.pos > jump {
            bail!("fixed-array reader overran jump: pos {}, jump {}", self.rd.pos, jump);
        }
        self.rd.pos = jump;
        Ok(())
    }

    pub fn fixed_i32_list(&mut self) -> Result<Vec<i64>> {
        let jump = self.rd.i32()?;
        let cnt = self.rd.i32()?.max(0) as usize;
        let mut out = Vec::with_capacity(cnt);
        for _ in 0..cnt {
            out.push(self.rd.i32()? as i64);
        }
        self.finish_fixed_array(jump)?;
        Ok(out)
    }

    pub fn fixed_str_list(&mut self) -> Result<Vec<String>> {
        let jump = self.rd.i32()?;
        let cnt = self.rd.i32()?.max(0) as usize;
        let mut out = Vec::with_capacity(cnt);
        for _ in 0..cnt {
            out.push(self.rd.str_len()?);
        }
        self.finish_fixed_array(jump)?;
        Ok(out)
    }

    pub fn extend_i32_list(&mut self) -> Result<Vec<i64>> {
        let cnt = self.rd.i32()?.max(0) as usize;
        let mut out = Vec::with_capacity(cnt);
        for _ in 0..cnt {
            out.push(self.rd.i32()? as i64);
        }
        Ok(out)
    }

    pub fn fixed_items<T, F>(&mut self, mut read_one: F) -> Result<Vec<T>>
    where
        F: FnMut(&mut OriginalStreamReader<'a>) -> Result<T>,
    {
        let jump = self.rd.i32()?;
        let cnt = self.rd.i32()?.max(0) as usize;
        let mut out = Vec::with_capacity(cnt);
        for _ in 0..cnt {
            out.push(read_one(self)?);
        }
        self.finish_fixed_array(jump)?;
        Ok(out)
    }

    pub fn extend_items<T, F>(&mut self, mut read_one: F) -> Result<Vec<T>>
    where
        F: FnMut(&mut OriginalStreamReader<'a>) -> Result<T>,
    {
        let cnt = self.rd.i32()?.max(0) as usize;
        let mut out = Vec::with_capacity(cnt);
        for _ in 0..cnt {
            out.push(read_one(self)?);
        }
        Ok(out)
    }

    pub fn skip_fixed_items<F>(&mut self, mut skip_one: F) -> Result<()>
    where
        F: FnMut(&mut OriginalStreamReader<'a>) -> Result<()>,
    {
        let jump = self.rd.i32()?;
        let cnt = self.rd.i32()?.max(0) as usize;
        for _ in 0..cnt {
            skip_one(self)?;
        }
        self.finish_fixed_array(jump)
    }
}

pub fn save_dir(project_dir: &Path) -> PathBuf {
    project_dir.join("savedata")
}

pub fn original_save_no(save_cnt: usize, quick_save_cnt: usize, kind: SaveKind, idx: usize) -> usize {
    match kind {
        SaveKind::Normal => idx,
        SaveKind::Quick => save_cnt + idx,
        SaveKind::End => save_cnt + quick_save_cnt + idx,
    }
}

pub fn save_file_path_for_no(project_dir: &Path, save_no: usize) -> PathBuf {
    save_dir(project_dir).join(format!("{save_no:04}.sav"))
}

pub fn save_file_path_with_counts(project_dir: &Path, save_cnt: usize, quick_save_cnt: usize, kind: SaveKind, idx: usize) -> PathBuf {
    save_file_path_for_no(project_dir, original_save_no(save_cnt, quick_save_cnt, kind, idx))
}

pub fn slot_save_no(project_dir: &Path, kind: SaveKind, idx: usize) -> usize {
    let save_cnt = configured_count(project_dir, false);
    let quick_save_cnt = configured_count(project_dir, true);
    original_save_no(save_cnt, quick_save_cnt, kind, idx)
}

pub fn save_file_path(project_dir: &Path, kind: SaveKind, idx: usize) -> PathBuf {
    save_file_path_for_no(project_dir, slot_save_no(project_dir, kind, idx))
}

pub fn thumb_candidate_paths_for_no(project_dir: &Path, save_no: usize) -> [PathBuf; 2] {
    let stem = format!("{save_no:04}");
    let dir = save_dir(project_dir);
    [dir.join(format!("{stem}.png")), dir.join(format!("{stem}.bmp"))]
}

pub fn thumb_candidate_paths_with_counts(project_dir: &Path, save_cnt: usize, quick_save_cnt: usize, kind: SaveKind, idx: usize) -> [PathBuf; 2] {
    thumb_candidate_paths_for_no(project_dir, original_save_no(save_cnt, quick_save_cnt, kind, idx))
}

pub fn read_header_from_path(path: &Path) -> Result<OriginalSaveHeader> {
    let data = fs::read(path).with_context(|| format!("read save header {}", path.display()))?;
    OriginalSaveHeader::from_bytes(&data[..data.len().min(SAVE_HEADER_SIZE)])
}

pub fn read_header(project_dir: &Path, kind: SaveKind, idx: usize) -> Option<OriginalSaveHeader> {
    let path = save_file_path(project_dir, kind, idx);
    read_header_from_path(&path).ok()
}

pub fn read_slot(project_dir: &Path, kind: SaveKind, idx: usize) -> Option<SaveSlotState> {
    read_header(project_dir, kind, idx).map(|h| h.to_slot())
}

pub fn read_slot_from_path(path: &Path) -> Option<SaveSlotState> {
    read_header_from_path(path).ok().map(|h| h.to_slot())
}

pub fn write_header_in_place(path: &Path, header: &OriginalSaveHeader) -> Result<()> {
    let mut data = fs::read(path).with_context(|| format!("read save file {}", path.display()))?;
    if data.len() < SAVE_HEADER_SIZE {
        bail!("save file too short for header update: {}", path.display());
    }
    data[..SAVE_HEADER_SIZE].copy_from_slice(&header.to_bytes());
    fs::write(path, data).with_context(|| format!("write save file {}", path.display()))
}

pub fn write_local_save_file(path: &Path, slot: &SaveSlotState, env: &OriginalLocalSaveEnvelope) -> Result<()> {
    let payload = env.to_bytes();
    let packed = pack_buffer(&payload);
    let header = OriginalSaveHeader::from_slot(slot, packed.len());
    let mut out = header.to_bytes();
    out.extend_from_slice(&packed);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create save dir {}", parent.display()))?;
    }
    fs::write(path, out).with_context(|| format!("write save file {}", path.display()))
}

pub fn write_slot_file(path: &Path, slot: &SaveSlotState) -> Result<()> {
    let env = OriginalLocalSaveEnvelope::empty_from_slot(slot);
    write_local_save_file(path, slot, &env)
}

pub fn read_local_save_file(path: &Path) -> Result<(OriginalSaveHeader, OriginalLocalSaveEnvelope)> {
    let data = fs::read(path).with_context(|| format!("read save file {}", path.display()))?;
    if data.len() < SAVE_HEADER_SIZE {
        bail!("save file too short: {}", path.display());
    }
    let header = OriginalSaveHeader::from_bytes(&data[..SAVE_HEADER_SIZE])?;
    let data_size = header.data_size.max(0) as usize;
    let end = SAVE_HEADER_SIZE
        .checked_add(data_size)
        .ok_or_else(|| anyhow!("save data size overflow"))?;
    if end > data.len() {
        bail!("save payload truncated: need {}, have {}", end, data.len());
    }
    let payload = unpack_buffer(&data[SAVE_HEADER_SIZE..end])?;
    let env = OriginalLocalSaveEnvelope::from_bytes(&payload)?;
    Ok((header, env))
}

pub fn write_global_save_file(project_dir: &Path, global_stream: &[u8]) -> Result<()> {
    let packed = pack_buffer(global_stream);
    let header = OriginalGlobalSaveHeader {
        major_version: 2,
        minor_version: 0,
        global_data_size: packed.len() as i32,
    };
    let path = save_dir(project_dir).join("global.sav");
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create save dir {}", parent.display()))?;
    }
    let mut out = header.to_bytes();
    out.extend_from_slice(&packed);
    fs::write(&path, out).with_context(|| format!("write global save file {}", path.display()))
}

pub fn read_global_save_file(project_dir: &Path) -> Result<Vec<u8>> {
    let path = save_dir(project_dir).join("global.sav");
    let data = fs::read(&path).with_context(|| format!("read global save file {}", path.display()))?;
    if data.len() < GLOBAL_SAVE_HEADER_SIZE {
        bail!("global save file too short: {}", path.display());
    }
    let header = OriginalGlobalSaveHeader::from_bytes(&data[..GLOBAL_SAVE_HEADER_SIZE])?;
    if header.major_version != 2 || header.minor_version != 0 {
        bail!("unsupported global save version {}.{}", header.major_version, header.minor_version);
    }
    let size = header.global_data_size.max(0) as usize;
    let end = GLOBAL_SAVE_HEADER_SIZE
        .checked_add(size)
        .ok_or_else(|| anyhow!("global save data size overflow"))?;
    if end > data.len() {
        bail!("global save payload truncated: need {}, have {}", end, data.len());
    }
    unpack_buffer(&data[GLOBAL_SAVE_HEADER_SIZE..end])
}

pub fn pack_buffer(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + src.len() + (src.len() + 7) / 8);
    push_u32(&mut out, 0);
    push_u32(&mut out, src.len() as u32);
    for chunk in src.chunks(8) {
        let mut flag = 0u8;
        for i in 0..chunk.len() {
            flag |= 1u8 << i;
        }
        out.push(flag);
        out.extend_from_slice(chunk);
    }
    let arc_size = out.len() as u32;
    out[0..4].copy_from_slice(&arc_size.to_le_bytes());
    tpc_xor(&mut out);
    out
}

pub fn unpack_buffer(src: &[u8]) -> Result<Vec<u8>> {
    let mut data = src.to_vec();
    tpc_xor(&mut data);
    if data.len() < 8 {
        bail!("lzss payload too short");
    }
    let arc_size = u32::from_le_bytes(data[0..4].try_into().unwrap()) as usize;
    let org_size = u32::from_le_bytes(data[4..8].try_into().unwrap()) as usize;
    if arc_size > data.len() {
        bail!("lzss arc_size out of bounds: {} > {}", arc_size, data.len());
    }
    let mut out = Vec::with_capacity(org_size);
    let mut p = 8usize;
    while out.len() < org_size {
        if p >= arc_size {
            bail!("lzss payload ended before output was complete");
        }
        let mut flags = data[p];
        p += 1;
        for _ in 0..8 {
            if out.len() >= org_size {
                break;
            }
            if flags & 1 != 0 {
                if p >= arc_size {
                    bail!("lzss literal out of bounds");
                }
                out.push(data[p]);
                p += 1;
            } else {
                if p + 2 > arc_size {
                    bail!("lzss backref out of bounds");
                }
                let token = u16::from_le_bytes([data[p], data[p + 1]]);
                p += 2;
                let offset = (token >> 4) as usize;
                let len = ((token & 0x0f) as usize) + 2;
                if offset == 0 || offset > out.len() {
                    bail!("lzss invalid backref offset {} at out {}", offset, out.len());
                }
                let base = out.len() - offset;
                for i in 0..len {
                    let b = out[base + i];
                    out.push(b);
                    if out.len() >= org_size {
                        break;
                    }
                }
            }
            flags >>= 1;
        }
    }
    Ok(out)
}

pub fn configured_count(project_dir: &Path, quick: bool) -> usize {
    let keys: [&str; 2] = if quick {
        ["#QUICK_SAVE.CNT", "QUICK_SAVE.CNT"]
    } else {
        ["#SAVE.CNT", "SAVE.CNT"]
    };
    configured_usize_any(project_dir, &keys, if quick { 3 } else { 10 }).min(10000)
}

pub fn configured_flag_count(project_dir: &Path) -> usize {
    configured_usize_any(project_dir, &["#FLAG.CNT", "FLAG.CNT"], 1000).min(10000)
}

pub fn configured_mwnd_waku_btn_count(project_dir: &Path) -> usize {
    configured_usize_any(project_dir, &["#WAKU.BTN.CNT", "WAKU.BTN.CNT"], 8).min(256)
}

fn configured_usize_any(_project_dir: &Path, _keys: &[&str], default_value: usize) -> usize {
    default_value
}

fn patch_i32(out: &mut [u8], off: usize, v: i32) {
    out[off..off + 4].copy_from_slice(&v.to_le_bytes());
}

fn push_i32(out: &mut Vec<u8>, v: i32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_i64(out: &mut Vec<u8>, v: i64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_u16(out: &mut Vec<u8>, v: u16) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_str_len(out: &mut Vec<u8>, s: &str) {
    let utf16: Vec<u16> = s.encode_utf16().collect();
    push_i32(out, utf16.len().min(i32::MAX as usize) as i32);
    for ch in utf16 {
        out.extend_from_slice(&ch.to_le_bytes());
    }
}

fn push_utf16_fixed(out: &mut Vec<u8>, s: &str, units: usize) {
    let mut written = 0usize;
    for ch in s.encode_utf16().take(units.saturating_sub(1)) {
        out.extend_from_slice(&ch.to_le_bytes());
        written += 1;
    }
    while written < units {
        out.extend_from_slice(&0u16.to_le_bytes());
        written += 1;
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or_else(|| anyhow!("reader overflow"))?;
        if end > self.data.len() {
            bail!("reader out of bounds: need {}, have {}", end, self.data.len());
        }
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }

    fn i32(&mut self) -> Result<i32> {
        let b = self.take(4)?;
        Ok(i32::from_le_bytes(b.try_into().unwrap()))
    }

    fn u8(&mut self) -> Result<u8> {
        let b = self.take(1)?;
        Ok(b[0])
    }

    fn u16(&mut self) -> Result<u16> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes(b.try_into().unwrap()))
    }

    fn i64(&mut self) -> Result<i64> {
        let b = self.take(8)?;
        Ok(i64::from_le_bytes(b.try_into().unwrap()))
    }

    fn utf16_fixed(&mut self, units: usize) -> Result<String> {
        let bytes = self.take(units * 2)?;
        let mut u16s = Vec::new();
        for i in 0..units {
            let p = i * 2;
            let w = u16::from_le_bytes([bytes[p], bytes[p + 1]]);
            if w == 0 {
                break;
            }
            u16s.push(w);
        }
        Ok(String::from_utf16_lossy(&u16s))
    }

    fn str_len(&mut self) -> Result<String> {
        let len = self.i32()?;
        if len < 0 {
            bail!("negative string length {}", len);
        }
        let len = len as usize;
        let bytes = self.take(len * 2)?;
        let mut u16s = Vec::with_capacity(len);
        for i in 0..len {
            let p = i * 2;
            u16s.push(u16::from_le_bytes([bytes[p], bytes[p + 1]]));
        }
        Ok(String::from_utf16_lossy(&u16s))
    }
}

fn tpc_xor(data: &mut [u8]) {
    for (i, b) in data.iter_mut().enumerate() {
        *b ^= TPC_ANGOU_TABLE[i & 0xff];
    }
}

const TPC_ANGOU_TABLE: [u8; 256] = [
    0x8b,0xe5,0x5d,0xc3,0xa1,0xe0,0x30,0x44,0x00,0x85,0xc0,0x74,0x09,0x5f,0x5e,0x33,
    0xc0,0x5b,0x8b,0xe5,0x5d,0xc3,0x8b,0x45,0x0c,0x85,0xc0,0x75,0x14,0x8b,0x55,0xec,
    0x83,0xc2,0x20,0x52,0x6a,0x00,0xe8,0xf5,0x28,0x01,0x00,0x83,0xc4,0x08,0x89,0x45,
    0x0c,0x8b,0x45,0xe4,0x6a,0x00,0x6a,0x00,0x50,0x53,0xff,0x15,0x34,0xb1,0x43,0x00,
    0x8b,0x45,0x10,0x85,0xc0,0x74,0x05,0x8b,0x4d,0xec,0x89,0x08,0x8a,0x45,0xf0,0x84,
    0xc0,0x75,0x78,0xa1,0xe0,0x30,0x44,0x00,0x8b,0x7d,0xe8,0x8b,0x75,0x0c,0x85,0xc0,
    0x75,0x44,0x8b,0x1d,0xd0,0xb0,0x43,0x00,0x85,0xff,0x76,0x37,0x81,0xff,0x00,0x00,
    0x04,0x00,0x6a,0x00,0x76,0x43,0x8b,0x45,0xf8,0x8d,0x55,0xfc,0x52,0x68,0x00,0x00,
    0x04,0x00,0x56,0x50,0xff,0x15,0x2c,0xb1,0x43,0x00,0x6a,0x05,0xff,0xd3,0xa1,0xe0,
    0x30,0x44,0x00,0x81,0xef,0x00,0x00,0x04,0x00,0x81,0xc6,0x00,0x00,0x04,0x00,0x85,
    0xc0,0x74,0xc5,0x8b,0x5d,0xf8,0x53,0xe8,0xf4,0xfb,0xff,0xff,0x8b,0x45,0x0c,0x83,
    0xc4,0x04,0x5f,0x5e,0x5b,0x8b,0xe5,0x5d,0xc3,0x8b,0x55,0xf8,0x8d,0x4d,0xfc,0x51,
    0x57,0x56,0x52,0xff,0x15,0x2c,0xb1,0x43,0x00,0xeb,0xd8,0x8b,0x45,0xe8,0x83,0xc0,
    0x20,0x50,0x6a,0x00,0xe8,0x47,0x28,0x01,0x00,0x8b,0x7d,0xe8,0x89,0x45,0xf4,0x8b,
    0xf0,0xa1,0xe0,0x30,0x44,0x00,0x83,0xc4,0x08,0x85,0xc0,0x75,0x56,0x8b,0x1d,0xd0,
    0xb0,0x43,0x00,0x85,0xff,0x76,0x49,0x81,0xff,0x00,0x00,0x04,0x00,0x6a,0x00,0x76,
];
