//! OVK (Ogg/Vorbis pack) and OWP (XORed Ogg) helpers.
//!
//! Helpers for OVK (Ogg/Vorbis pack) and OWP (XORed Ogg) audio formats.

use crate::ogg_xor::{validate_subrange, BoundedFile};
use crate::vorbis;
use anyhow::{bail, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy)]
pub struct OvkEntry {
    pub size: u32,
    pub offset: u32,
    pub no: u32,
    pub sample_count: u32,
}

#[derive(Debug, Clone)]
pub struct OvkPack {
    path: PathBuf,
    entries: Vec<OvkEntry>,
    file_len: u64,
}

impl OvkPack {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let mut f = File::open(&path)?;
        let file_len = f.metadata()?.len();

        let mut head = [0u8; 4];
        f.read_exact(&mut head)?;
        let count = u32::from_le_bytes(head) as usize;
        if count == 0 {
            bail!("OVK: zero entries");
        }
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let mut buf = [0u8; 16];
            f.read_exact(&mut buf)?;
            let size = u32::from_le_bytes(buf[0..4].try_into().unwrap());
            let offset = u32::from_le_bytes(buf[4..8].try_into().unwrap());
            let no = u32::from_le_bytes(buf[8..12].try_into().unwrap());
            let smp = u32::from_le_bytes(buf[12..16].try_into().unwrap());
            entries.push(OvkEntry {
                size,
                offset,
                no,
                sample_count: smp,
            });
        }
        // Basic bounds checks (size==0 is allowed but pointless; treat as empty).
        for (i, e) in entries.iter().enumerate() {
            if e.size == 0 {
                continue;
            }
            validate_subrange(file_len, e.offset as u64, e.size as u64)
                .map_err(|err| anyhow::anyhow!("OVK entry[{i}] out of range: {err}"))?;
        }

        Ok(Self {
            path,
            entries,
            file_len,
        })
    }

    pub fn entries(&self) -> &[OvkEntry] {
        &self.entries
    }

    pub fn get(&self, idx: usize) -> Option<OvkEntry> {
        self.entries.get(idx).copied()
    }

    /// Create a bounded reader for an entry.
    pub fn open_entry_stream(&self, idx: usize) -> Result<BoundedFile> {
        let e = self
            .entries
            .get(idx)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("OVK: entry index out of range: {idx}"))?;
        if e.size == 0 {
            bail!("OVK: entry[{idx}] has zero size");
        }
        BoundedFile::open(&self.path, e.offset as u64, e.size as u64, None)
    }

    /// Extract an entry into memory.
    pub fn extract_entry(&self, idx: usize) -> Result<Vec<u8>> {
        self.open_entry_stream(idx)?.read_all()
    }

    /// Decode an entry (expected to be Ogg/Vorbis) into interleaved PCM16.
    pub fn decode_entry_vorbis_pcm16(&self, idx: usize) -> Result<vorbis::Pcm16> {
        let s = self.open_entry_stream(idx)?;
        vorbis::decode_ogg_vorbis_reader(s)
    }

    /// Decode an entry (expected to be Ogg/Vorbis) and return a WAV (PCM16) buffer.
    pub fn decode_entry_vorbis_wav(&self, idx: usize) -> Result<Vec<u8>> {
        let s = self.open_entry_stream(idx)?;
        vorbis::decode_ogg_vorbis_reader_to_wav(s)
    }
}

/// OWP: XOR-obfuscated Ogg file. The original engine uses key 0x39.
#[derive(Debug, Clone)]
pub struct OwpFile {
    path: PathBuf,
    file_len: u64,
    pub xor_key: u8,
}

impl OwpFile {
    pub const DEFAULT_XOR_KEY: u8 = 0x39;

    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file_len = File::open(&path)?.metadata()?.len();
        Ok(Self {
            path,
            file_len,
            xor_key: Self::DEFAULT_XOR_KEY,
        })
    }

    pub fn open_stream(&self) -> Result<BoundedFile> {
        BoundedFile::open(&self.path, 0, self.file_len, Some(self.xor_key))
    }

    pub fn decrypt_to_vec(&self) -> Result<Vec<u8>> {
        self.open_stream()?.read_all()
    }

    /// Decode the XORed Ogg/Vorbis file into interleaved PCM16.
    pub fn decode_vorbis_pcm16(&self) -> Result<vorbis::Pcm16> {
        let s = self.open_stream()?;
        vorbis::decode_ogg_vorbis_reader(s)
    }

    /// Decode the XORed Ogg/Vorbis file and return a WAV (PCM16) buffer.
    pub fn decode_vorbis_wav(&self) -> Result<Vec<u8>> {
        let s = self.open_stream()?;
        vorbis::decode_ogg_vorbis_reader_to_wav(s)
    }
}
