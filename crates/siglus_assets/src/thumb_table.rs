//! Thumb table loader.
//!
//! Loads the thumbnail lookup table.
//!
//! File layout (on disk):
//! - `S_tnm_thumbnail_database_header`:
//!   - `i32 header_size`
//!   - `i32 version`
//!   - `i32 data_cnt`
//! - `lzss_stream[...]` (Siglus LZSS byte-oriented stream)
//!
//! Decompressed layout:
//! - Repeated `data_cnt` times:
//!   - `TCHAR pct[]` (UTF-16LE) NUL-terminated
//!   - `TCHAR thumb[]` (UTF-16LE) NUL-terminated
//!
//! The engine lowercases both strings (`str_to_lower`) and inserts them into
//! a map `pct -> thumb`.

use crate::lzss;
use crate::util::read_i32_le;
use anyhow::{bail, Result};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone)]
pub struct ThumbTable {
    header_size: i32,
    version: i32,
    map: BTreeMap<String, String>,
}

impl ThumbTable {
    /// Load and decode a `thumb_table_file.dat` from disk.
    pub fn load<P: AsRef<Path>>(path: P) -> Result<Self> {
        let bytes = fs::read(path)?;
        Self::from_bytes(&bytes)
    }

    /// Load and decode a `thumb_table_file.dat` from an in-memory buffer.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let mut off = 0usize;
        let header_size = read_i32_le(bytes, &mut off)?;
        let version = read_i32_le(bytes, &mut off)?;
        let data_cnt = read_i32_le(bytes, &mut off)?;
        if data_cnt < 0 {
            bail!("thumb_table: invalid data_cnt={data_cnt}");
        }
        if off > bytes.len() {
            bail!("thumb_table: unexpected EOF in header");
        }

        // The remaining buffer is an LZSS stream.
        let unpack = lzss::lzss_unpack(&bytes[off..])?;
        let mut uoff = 0usize;

        let mut map: BTreeMap<String, String> = BTreeMap::new();
        for _ in 0..(data_cnt as usize) {
            let pct = read_tchar_null(&unpack, &mut uoff)?;
            let thumb = read_tchar_null(&unpack, &mut uoff)?;
            let pct = to_lowercase_like_engine(&pct);
            let thumb = to_lowercase_like_engine(&thumb);
            map.insert(pct, thumb);
        }

        Ok(Self {
            header_size,
            version,
            map,
        })
    }

    pub fn header_size(&self) -> i32 {
        self.header_size
    }

    pub fn version(&self) -> i32 {
        self.version
    }

    pub fn map(&self) -> &BTreeMap<String, String> {
        &self.map
    }

    /// Lookup `pct` after applying the engine's lowercasing.
    pub fn get(&self, pct: &str) -> Option<&String> {
        let key = to_lowercase_like_engine(pct);
        self.map.get(&key)
    }

    /// Helper matching the `calc_thumb_file_name` lookup behavior:
    /// provide a file name (with or without extension); the extension is
    /// stripped and the remaining stem is lowercased before lookup.
    pub fn get_by_file_stem(&self, name: &str) -> Option<&String> {
        let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
        self.get(stem)
    }
}

fn read_tchar_null(buf: &[u8], off: &mut usize) -> Result<String> {
    let mut u16s: Vec<u16> = Vec::new();
    loop {
        if *off + 2 > buf.len() {
            bail!("thumb_table: unterminated TCHAR string");
        }
        let w = u16::from_le_bytes([buf[*off], buf[*off + 1]]);
        *off += 2;
        if w == 0 {
            break;
        }
        u16s.push(w);
    }
    Ok(String::from_utf16_lossy(&u16s))
}

/// Siglus uses `str_to_lower` on `TSTR`. We don't have the exact CRT/Win32
/// locale behavior here; this implementation applies Unicode simple lowercase
/// mapping, which matches the expected behavior for ASCII filenames.
fn to_lowercase_like_engine(s: &str) -> String {
    s.chars().flat_map(|c| c.to_lowercase()).collect()
}
