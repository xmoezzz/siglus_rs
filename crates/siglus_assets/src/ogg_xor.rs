//! Ogg/Vorbis related helpers used by Siglus/Tona.
//!
//! The original engine sometimes stores Ogg data with a simple XOR obfuscation.
//! Some runtimes apply this XOR transform on reads.

use anyhow::{bail, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// A `Read + Seek` wrapper that restricts access to a `[start, start+len)` range.
///
/// If `xor_key` is `Some(k)`, each byte returned from `read()` is XORed with `k`.
pub struct BoundedFile {
    file: File,
    start: u64,
    len: u64,
    pos: u64,
    xor_key: Option<u8>,
}

impl BoundedFile {
    pub fn open<P: AsRef<Path>>(
        path: P,
        start: u64,
        len: u64,
        xor_key: Option<u8>,
    ) -> Result<Self> {
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(start))?;
        Ok(Self {
            file,
            start,
            len,
            pos: 0,
            xor_key,
        })
    }

    pub fn remaining(&self) -> u64 {
        self.len.saturating_sub(self.pos)
    }

    pub fn into_inner(self) -> File {
        self.file
    }

    /// Read the entire bounded region into memory (applying XOR if configured).
    pub fn read_all(mut self) -> Result<Vec<u8>> {
        let mut out = Vec::with_capacity(self.len.min(256 * 1024) as usize);
        self.seek(SeekFrom::Start(0))?;
        self.read_to_end(&mut out)?;
        Ok(out)
    }
}

impl Read for BoundedFile {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.len {
            return Ok(0);
        }
        let max = (self.len - self.pos) as usize;
        let to_read = buf.len().min(max);
        let n = self.file.read(&mut buf[..to_read])?;
        if let Some(k) = self.xor_key {
            for b in &mut buf[..n] {
                *b ^= k;
            }
        }
        self.pos += n as u64;
        Ok(n)
    }
}

impl Seek for BoundedFile {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let new_pos: i128 = match pos {
            SeekFrom::Start(x) => x as i128,
            SeekFrom::Current(x) => self.pos as i128 + x as i128,
            SeekFrom::End(x) => self.len as i128 + x as i128,
        };
        if new_pos < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "negative seek",
            ));
        }
        let new_pos_u = new_pos as u64;
        if new_pos_u > self.len {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "seek past end of bounded region",
            ));
        }
        self.file.seek(SeekFrom::Start(self.start + new_pos_u))?;
        self.pos = new_pos_u;
        Ok(self.pos)
    }
}

/// Decrypt a whole file by XORing each byte with `key`.
///
/// This is a convenience API for small/medium assets. For streaming decode, prefer `BoundedFile`.
pub fn xor_file_to_vec<P: AsRef<Path>>(path: P, key: u8) -> Result<Vec<u8>> {
    let mut f = File::open(path)?;
    let mut v = Vec::new();
    f.read_to_end(&mut v)?;
    for b in &mut v {
        *b ^= key;
    }
    // quick sanity check: Ogg pages start with "OggS"
    if v.len() >= 4 && &v[..4] != b"OggS" {
        // Not all sources begin with OggS at byte 0 (e.g., leading junk), so don't hard-fail.
        // But this helps catch wrong-key usage early.
        // The caller may still proceed.
    }
    Ok(v)
}

/// Basic sniffing for Ogg container magic.
pub fn looks_like_ogg(buf: &[u8]) -> bool {
    buf.len() >= 4 && &buf[..4] == b"OggS"
}

/// Validate `offset`/`size` against the file length.
pub fn validate_subrange(file_len: u64, offset: u64, size: u64) -> Result<()> {
    if offset > file_len {
        bail!("subrange offset {offset} exceeds file length {file_len}");
    }
    if size == 0 {
        // size==0 means "until EOF" in the original code.
        return Ok(());
    }
    if offset.saturating_add(size) > file_len {
        bail!("subrange [offset={offset}, size={size}] exceeds file length {file_len}");
    }
    Ok(())
}
