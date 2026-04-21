//! DBS (Siglus database) loader.
//!
//! This module implements the `.dbs` expansion and table access logic used by the engine runtime.
//!
//! File layout (on disk):
//! - `i32 type` (little-endian)
//! - `payload[...]` : obfuscated LZSS stream
//!
//! Expansion algorithm (`tnm_database_expand`):
//! 1) XOR payload as u32 stream with `XORCODE[2]`.
//! 2) LZSS-unpack payload into `unpack_data`.
//! 3) Apply a tiled binary mask split into A/B using `tile_copy` semantics.
//! 4) XOR A with `XORCODE[0]`, XOR B with `XORCODE[1]`.
//! 5) Re-composite A/B back into the final expanded buffer using the same mask.
//!
//! Expanded buffer layout (in memory):
//! - `S_tnm_database_header` at offset 0
//! - row headers table (array of `S_tnm_database_row_header`)
//! - column headers table (array of `S_tnm_database_column_header`)
//! - data table (row_cnt * column_cnt DWORDs)
//! - string table
//!
//! String encoding:
//! - If `type == 0`: strings are multibyte (typically Shift-JIS) and NUL-terminated.
//! - Else: strings are UTF-16LE (`TCHAR`) and NUL-terminated.

use crate::lzss;
use crate::util::read_i32_le;
use anyhow::{anyhow, bail, Result};
use encoding_rs::SHIFT_JIS;
use std::fs;
use std::path::Path;

const MAP_WIDTH: usize = 16;
const TILE_WIDTH: usize = 5;
const TILE_HEIGHT: usize = 5;

// Tile mask pattern used by the DBS expander.
const TILE: [u8; TILE_WIDTH * TILE_HEIGHT] = [
    255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 255, 255, 255, 0, 255, 0, 0, 255, 0, 0, 0, 0, 0, 255,
    255,
];

const XORCODE: [u32; 3] = [0x753A4098, 0x4A673CCC, 0xFE6215AF];

#[derive(Debug, Clone, Copy)]
pub struct DbsRowHeader {
    pub call_no: i32,
}

#[derive(Debug, Clone, Copy)]
pub struct DbsColumnHeader {
    pub call_no: i32,
    pub data_type: i32,
}

#[derive(Debug, Clone, Copy)]
struct DbsHeader {
    data_size: i32,
    row_cnt: i32,
    column_cnt: i32,
    row_header_offset: i32,
    column_header_offset: i32,
    data_offset: i32,
    str_offset: i32,
}

#[derive(Debug, Clone)]
pub struct DbsDatabase {
    db_type: i32,
    expanded: Vec<u8>,
    header: DbsHeader,
    rows: Vec<DbsRowHeader>,
    cols: Vec<DbsColumnHeader>,
    data: Vec<u32>,
    str_base: usize,
}

impl DbsDatabase {
    /// Load and decode a `.dbs` file from disk.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let bytes = fs::read(path)?;
        Self::from_bytes(&bytes)
    }

    /// Load and decode a `.dbs` file from an in-memory buffer.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() < 4 {
            bail!("DBS: file too short");
        }
        let db_type = i32::from_le_bytes(bytes[0..4].try_into().unwrap());
        let payload = &bytes[4..];
        let expanded = tnm_database_expand(payload)?;
        Self::parse_expanded(db_type, expanded)
    }

    pub fn db_type(&self) -> i32 {
        self.db_type
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub fn column_count(&self) -> usize {
        self.cols.len()
    }

    pub fn rows(&self) -> &[DbsRowHeader] {
        &self.rows
    }

    pub fn columns(&self) -> &[DbsColumnHeader] {
        &self.cols
    }

    /// Mimics `C_elm_database::get_data(int,int,int*)`.
    pub fn get_data_int(&self, item_call_no: i32, column_call_no: i32) -> Result<Option<i32>> {
        let item_no = self.get_item_no(item_call_no);
        let col_no = self.get_column_no(column_call_no);
        if item_no < 0 || col_no < 0 {
            return Ok(None);
        }
        let col_no = col_no as usize;
        if self.cols[col_no].data_type as u8 != b'V' {
            bail!("DBS: column_call_no={column_call_no} is not numeric");
        }
        let idx = (item_no as usize)
            .checked_mul(self.cols.len())
            .and_then(|v| v.checked_add(col_no))
            .ok_or_else(|| anyhow!("DBS: data index overflow"))?;
        let v = self
            .data
            .get(idx)
            .ok_or_else(|| anyhow!("DBS: data index out of range"))?;
        Ok(Some(*v as i32))
    }

    /// Mimics `C_elm_database::get_data(int,int,TSTR&)`.
    pub fn get_data_str(&self, item_call_no: i32, column_call_no: i32) -> Result<Option<String>> {
        let item_no = self.get_item_no(item_call_no);
        let col_no = self.get_column_no(column_call_no);
        if item_no < 0 || col_no < 0 {
            return Ok(None);
        }
        let col_no = col_no as usize;
        if self.cols[col_no].data_type as u8 != b'S' {
            bail!("DBS: column_call_no={column_call_no} is not string");
        }
        let idx = (item_no as usize)
            .checked_mul(self.cols.len())
            .and_then(|v| v.checked_add(col_no))
            .ok_or_else(|| anyhow!("DBS: data index overflow"))?;
        let off = self
            .data
            .get(idx)
            .ok_or_else(|| anyhow!("DBS: data index out of range"))?;
        Ok(Some(self.get_str(*off as usize)?))
    }

    /// Return the column type: 0 = missing, 1 = numeric ('V'), 2 = string ('S').
    pub fn check_column_no(&self, column_call_no: i32) -> i32 {
        let col_no = self.get_column_no(column_call_no);
        if col_no < 0 {
            return 0;
        }
        match self.cols[col_no as usize].data_type as u8 {
            b'V' => 1,
            b'S' => 2,
            _ => 0,
        }
    }

    /// Return 1 if the item call number exists, otherwise 0.
    pub fn check_item_no(&self, item_call_no: i32) -> i32 {
        if self.get_item_no(item_call_no) >= 0 { 1 } else { 0 }
    }

    /// Mimics `C_elm_database::find_num`.
    pub fn find_num(&self, column_call_no: i32, num: i32) -> Result<i32> {
        let col_no = self.get_column_no(column_call_no);
        if col_no < 0 {
            return Ok(-1);
        }
        let col_no = col_no as usize;
        if self.cols[col_no].data_type as u8 != b'V' {
            bail!("DBS: column_call_no={column_call_no} is not numeric");
        }
        for row in 0..self.rows.len() {
            let idx = row * self.cols.len() + col_no;
            if (self.data[idx] as i32) == num {
                return Ok(self.rows[row].call_no);
            }
        }
        Ok(-1)
    }

    /// Mimics `C_elm_database::find_str` (case-insensitive ASCII).
    pub fn find_str(&self, column_call_no: i32, s: &str) -> Result<i32> {
        let col_no = self.get_column_no(column_call_no);
        if col_no < 0 {
            return Ok(-1);
        }
        let col_no = col_no as usize;
        if self.cols[col_no].data_type as u8 != b'S' {
            bail!("DBS: column_call_no={column_call_no} is not string");
        }
        let needle = s.to_ascii_lowercase();
        for row in 0..self.rows.len() {
            let idx = row * self.cols.len() + col_no;
            let off = self.data[idx] as usize;
            let got = self.get_str(off)?;
            if got.to_ascii_lowercase() == needle {
                return Ok(self.rows[row].call_no);
            }
        }
        Ok(-1)
    }

    /// Mimics `C_elm_database::find_str_real` (case-sensitive).
    pub fn find_str_real(&self, column_call_no: i32, s: &str) -> Result<i32> {
        let col_no = self.get_column_no(column_call_no);
        if col_no < 0 {
            return Ok(-1);
        }
        let col_no = col_no as usize;
        if self.cols[col_no].data_type as u8 != b'S' {
            bail!("DBS: column_call_no={column_call_no} is not string");
        }
        for row in 0..self.rows.len() {
            let idx = row * self.cols.len() + col_no;
            let off = self.data[idx] as usize;
            let got = self.get_str(off)?;
            if got == s {
                return Ok(self.rows[row].call_no);
            }
        }
        Ok(-1)
    }

    fn parse_expanded(db_type: i32, expanded: Vec<u8>) -> Result<Self> {
        let mut off = 0usize;
        let data_size = read_i32_le(&expanded, &mut off)?;
        let row_cnt = read_i32_le(&expanded, &mut off)?;
        let column_cnt = read_i32_le(&expanded, &mut off)?;
        let row_header_offset = read_i32_le(&expanded, &mut off)?;
        let column_header_offset = read_i32_le(&expanded, &mut off)?;
        let data_offset = read_i32_le(&expanded, &mut off)?;
        let str_offset = read_i32_le(&expanded, &mut off)?;

        if data_size <= 0 {
            bail!("DBS: invalid data_size={data_size}");
        }
        if data_size as usize > expanded.len() {
            bail!(
                "DBS: header data_size out of range (data_size={}, buf_len={})",
                data_size,
                expanded.len()
            );
        }
        if row_cnt < 0 || column_cnt < 0 {
            bail!("DBS: negative counts (row_cnt={row_cnt}, column_cnt={column_cnt})");
        }

        let row_cnt_u = row_cnt as usize;
        let col_cnt_u = column_cnt as usize;

        let row_header_off = row_header_offset as usize;
        let col_header_off = column_header_offset as usize;
        let data_off = data_offset as usize;
        let str_off = str_offset as usize;

        if row_header_off > expanded.len()
            || col_header_off > expanded.len()
            || data_off > expanded.len()
            || str_off > expanded.len()
        {
            bail!("DBS: one or more offsets out of range");
        }

        let row_headers_bytes = row_cnt_u
            .checked_mul(4)
            .ok_or_else(|| anyhow!("DBS: row headers size overflow"))?;
        let col_headers_bytes = col_cnt_u
            .checked_mul(8)
            .ok_or_else(|| anyhow!("DBS: col headers size overflow"))?;
        let data_bytes = row_cnt_u
            .checked_mul(col_cnt_u)
            .and_then(|v| v.checked_mul(4))
            .ok_or_else(|| anyhow!("DBS: data table size overflow"))?;

        if row_header_off + row_headers_bytes > expanded.len() {
            bail!("DBS: row header table truncated");
        }
        if col_header_off + col_headers_bytes > expanded.len() {
            bail!("DBS: column header table truncated");
        }
        if data_off + data_bytes > expanded.len() {
            bail!("DBS: data table truncated");
        }
        if str_off > expanded.len() {
            bail!("DBS: string table offset out of range");
        }

        let mut rows = Vec::with_capacity(row_cnt_u);
        let mut roff = row_header_off;
        for _ in 0..row_cnt_u {
            let call_no = i32::from_le_bytes(expanded[roff..roff + 4].try_into().unwrap());
            rows.push(DbsRowHeader { call_no });
            roff += 4;
        }

        let mut cols = Vec::with_capacity(col_cnt_u);
        let mut coff = col_header_off;
        for _ in 0..col_cnt_u {
            let call_no = i32::from_le_bytes(expanded[coff..coff + 4].try_into().unwrap());
            let data_type = i32::from_le_bytes(expanded[coff + 4..coff + 8].try_into().unwrap());
            cols.push(DbsColumnHeader { call_no, data_type });
            coff += 8;
        }

        let mut data = Vec::with_capacity(row_cnt_u * col_cnt_u);
        let mut doff = data_off;
        for _ in 0..(row_cnt_u * col_cnt_u) {
            let v = u32::from_le_bytes(expanded[doff..doff + 4].try_into().unwrap());
            data.push(v);
            doff += 4;
        }

        Ok(Self {
            db_type,
            expanded,
            header: DbsHeader {
                data_size,
                row_cnt,
                column_cnt,
                row_header_offset,
                column_header_offset,
                data_offset,
                str_offset,
            },
            rows,
            cols,
            data,
            str_base: str_off,
        })
    }

    fn get_item_no(&self, item_call_no: i32) -> i32 {
        for (i, r) in self.rows.iter().enumerate() {
            if r.call_no == item_call_no {
                return i as i32;
            }
        }
        -1
    }

    fn get_column_no(&self, column_call_no: i32) -> i32 {
        for (i, c) in self.cols.iter().enumerate() {
            if c.call_no == column_call_no {
                return i as i32;
            }
        }
        -1
    }

    fn get_str(&self, str_offset: usize) -> Result<String> {
        let base = self
            .str_base
            .checked_add(str_offset)
            .ok_or_else(|| anyhow!("DBS: string offset overflow"))?;
        if base >= self.expanded.len() {
            bail!("DBS: string offset out of range");
        }

        if self.db_type == 0 {
            // Multibyte NUL-terminated.
            let end = self.expanded[base..]
                .iter()
                .position(|&b| b == 0)
                .map(|p| base + p)
                .unwrap_or(self.expanded.len());
            let bytes = &self.expanded[base..end];
            let (cow, _, _) = SHIFT_JIS.decode(bytes);
            Ok(cow.into_owned())
        } else {
            // UTF-16LE (TCHAR) NUL-terminated.
            let mut cur = base;
            let mut u16s: Vec<u16> = Vec::new();
            loop {
                if cur + 2 > self.expanded.len() {
                    break;
                }
                let w = u16::from_le_bytes([self.expanded[cur], self.expanded[cur + 1]]);
                cur += 2;
                if w == 0 {
                    break;
                }
                u16s.push(w);
            }
            Ok(String::from_utf16_lossy(&u16s))
        }
    }
}

fn tnm_database_expand(src_payload: &[u8]) -> Result<Vec<u8>> {
    if src_payload.is_empty() {
        bail!("DBS: empty payload");
    }

    // Step 1: XORCODE[2] over DWORD stream.
    let mut payload = src_payload.to_vec();
    xor_u32_in_place(&mut payload, XORCODE[2]);

    // Step 2: LZSS unpack.
    let unpack_data = lzss::lzss_unpack(&payload)?;
    let unpack_size = unpack_data.len();
    if unpack_size == 0 {
        bail!("DBS: unpack_size=0");
    }
    if unpack_size % (MAP_WIDTH * 4) != 0 {
        bail!(
            "DBS: unpack_size not aligned to map width (unpack_size={}, map_stride={})",
            unpack_size,
            MAP_WIDTH * 4
        );
    }

    let yl = unpack_size / (MAP_WIDTH * 4);

    // Step 3: split by mask.
    let mut temp_a = vec![0u8; unpack_size];
    let mut temp_b = vec![0u8; unpack_size];
    mask_copy_u32(
        &mut temp_a,
        &unpack_data,
        MAP_WIDTH,
        yl,
        &TILE,
        TILE_WIDTH,
        TILE_HEIGHT,
        0,
        128,
    )?;
    mask_copy_u32(
        &mut temp_b,
        &unpack_data,
        MAP_WIDTH,
        yl,
        &TILE,
        TILE_WIDTH,
        TILE_HEIGHT,
        1,
        128,
    )?;

    // Step 4: XOR A/B with XORCODE[0]/[1].
    xor_u32_in_place(&mut temp_a, XORCODE[0]);
    xor_u32_in_place(&mut temp_b, XORCODE[1]);

    // Step 5: composite back.
    let mut dst = vec![0u8; unpack_size];
    mask_copy_u32(
        &mut dst,
        &temp_a,
        MAP_WIDTH,
        yl,
        &TILE,
        TILE_WIDTH,
        TILE_HEIGHT,
        0,
        128,
    )?;
    mask_copy_u32(
        &mut dst,
        &temp_b,
        MAP_WIDTH,
        yl,
        &TILE,
        TILE_WIDTH,
        TILE_HEIGHT,
        1,
        128,
    )?;

    Ok(dst)
}

#[inline]
fn xor_u32_in_place(buf: &mut [u8], xor_code: u32) {
    let n = buf.len() / 4;
    for i in 0..n {
        let off = i * 4;
        let v = u32::from_le_bytes(buf[off..off + 4].try_into().unwrap()) ^ xor_code;
        buf[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }
}

/// Copy 4-byte units from `src` to `dst` using a repeated mask tile.
///
/// This implements the mask-tiled copy step used by the DBS expander.
fn mask_copy_u32(
    dst: &mut [u8],
    src: &[u8],
    xl: usize,
    yl: usize,
    mask: &[u8],
    m_xl: usize,
    m_yl: usize,
    reverse: i32,
    limit: u8,
) -> Result<()> {
    if dst.len() != src.len() {
        bail!("DBS: mask_copy size mismatch");
    }
    let expect_len = xl
        .checked_mul(yl)
        .and_then(|v| v.checked_mul(4))
        .ok_or_else(|| anyhow!("DBS: mask_copy size overflow"))?;
    if expect_len != dst.len() {
        bail!(
            "DBS: mask_copy unexpected buffer length (expect={}, got={})",
            expect_len,
            dst.len()
        );
    }
    if mask.len() != m_xl * m_yl {
        bail!("DBS: mask tile size mismatch");
    }

    for y in 0..yl {
        for x in 0..xl {
            let mx = x % m_xl;
            let my = y % m_yl;
            let mv = mask[my * m_xl + mx];
            // Exact `tile_copy` semantics for this expander:
            // - reverse==0 : copy where tile value >= limit
            // - reverse!=0 : copy where tile value < limit
            let cond = if reverse == 0 {
                mv >= limit
            } else {
                mv < limit
            };
            if !cond {
                continue;
            }
            let p = (y * xl + x) * 4;
            dst[p..p + 4].copy_from_slice(&src[p..p + 4]);
        }
    }

    Ok(())
}
