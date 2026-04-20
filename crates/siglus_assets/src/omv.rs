//! OMV (Ogg/Theora wrapper) parser.
//!
//! Parser for the OMV container format.
//!
//! OMV stores a small fixed metadata block followed by internal seek tables and
//! finally the embedded raw Ogg bitstream (Theora/Vorbis). For playback we only
//! need the display size, frame time, and the offset of the first `OggS` page.

use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub const OMV_THEORA_TYPE_RGB: u32 = 0;
pub const OMV_THEORA_TYPE_RGBA: u32 = 1;
pub const OMV_THEORA_TYPE_YUV: u32 = 2;

#[derive(Debug, Clone)]
pub struct OmvHeader {
    pub header_size: u32,
    pub version: u32,
    pub theora_type: u32,
    pub display_width: u32,
    pub display_height: u32,
    pub frame_time_us: u32,
    pub max_data_size: u32,
    pub page_count_hint: u32,
    pub packet_count_hint: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct OmvTheoraPage {
    pub ofs: u32,
    pub time: u32,
    pub key: u8,
    pub reserved: [u8; 3],
}

#[derive(Debug, Clone, Copy)]
pub struct OmvTheoraPacket {
    pub ofs: u32,
    pub time: u32,
}

#[derive(Debug, Clone)]
pub struct OmvFile {
    pub header: OmvHeader,
    pub pages: Vec<OmvTheoraPage>,
    pub packets: Vec<OmvTheoraPacket>,
    pub ogg_data_offset: u64,
}

impl OmvFile {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let bytes = std::fs::read(&path)
            .with_context(|| format!("open OMV: {}", path.as_ref().display()))?;
        let header = read_header(&bytes)?;
        let ogg_data_offset = find_ogg_offset(&bytes)?;
        Ok(Self {
            header,
            pages: Vec::new(),
            packets: Vec::new(),
            ogg_data_offset,
        })
    }

    /// Read the embedded Ogg bitstream (Theora) as bytes.
    pub fn read_embedded_ogg(path: impl AsRef<Path>) -> Result<Vec<u8>> {
        let bytes = std::fs::read(&path)
            .with_context(|| format!("open OMV: {}", path.as_ref().display()))?;
        let ogg_data_offset = find_ogg_offset(&bytes)? as usize;
        Ok(bytes[ogg_data_offset..].to_vec())
    }
}

fn read_header(buf: &[u8]) -> Result<OmvHeader> {
    if buf.len() < 0x58 {
        bail!("OMV header too small");
    }

    let header_size = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let version = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    let theora_type = u32::from_le_bytes([buf[0x28], buf[0x29], buf[0x2a], buf[0x2b]]);
    let display_width = u32::from_le_bytes([buf[0x2c], buf[0x2d], buf[0x2e], buf[0x2f]]);
    let display_height = u32::from_le_bytes([buf[0x30], buf[0x31], buf[0x32], buf[0x33]]);
    let frame_time_us = u32::from_le_bytes([buf[0x3c], buf[0x3d], buf[0x3e], buf[0x3f]]);
    let max_data_size = u32::from_le_bytes([buf[0x40], buf[0x41], buf[0x42], buf[0x43]]);
    let page_count_hint = u32::from_le_bytes([buf[0x4c], buf[0x4d], buf[0x4e], buf[0x4f]]);
    let packet_count_hint = u32::from_le_bytes([buf[0x50], buf[0x51], buf[0x52], buf[0x53]]);

    if header_size < 0x58 {
        bail!("invalid OMV header size: {header_size:#x}");
    }
    if theora_type > OMV_THEORA_TYPE_YUV {
        bail!("invalid OMV theora type: {theora_type}");
    }
    if display_width == 0 || display_height == 0 {
        bail!(
            "invalid OMV display size: {}x{}",
            display_width,
            display_height
        );
    }

    Ok(OmvHeader {
        header_size,
        version,
        theora_type,
        display_width,
        display_height,
        frame_time_us,
        max_data_size,
        page_count_hint,
        packet_count_hint,
    })
}

fn find_ogg_offset(buf: &[u8]) -> Result<u64> {
    let needle = b"OggS";
    let pos = buf
        .windows(needle.len())
        .position(|w| w == needle)
        .ok_or_else(|| anyhow::anyhow!("OggS not found in OMV payload"))?;
    Ok(pos as u64)
}
