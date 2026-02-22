//! OMV (Ogg/Theora wrapper) parser.
//!
//! Parser for the OMV container format.
//!
//! OMV stores:
//! - a fixed header (S_omv_header)
//! - a page list (S_omv_theora_page[]) for seek
//! - a packet list (S_omv_theora_packet[]) for seek
//! - an embedded raw Ogg bitstream (Theora)

use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct OmvHeader {
    pub id: [u8; 4],
    pub version: u8,
    pub fps: u8,
    pub reserved: u16,
    pub codec_name: [u8; 32],
    pub frame_count: u32,
    pub max_data_size: u32,
    pub base_size: (i32, i32),
    pub cut_left_top: (i32, i32),
    pub cut_right_bottom: (i32, i32),
    pub theora_page_cnt: u32,
    pub theora_page_size: u32,
    pub theora_packet_cnt: u32,
    pub theora_packet_size: u32,
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
        let mut f = File::open(&path).with_context(|| format!("open OMV: {}", path.as_ref().display()))?;
        let header = read_header(&mut f)?;
        if &header.id[0..3] != b"OMV" {
            bail!("invalid OMV magic: {:02X?}", header.id);
        }
        let mut pages = Vec::with_capacity(header.theora_page_cnt as usize);
        for _ in 0..header.theora_page_cnt {
            let mut buf = [0u8; 12];
            f.read_exact(&mut buf)?;
            pages.push(OmvTheoraPage {
                ofs: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
                time: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
                key: buf[8],
                reserved: [buf[9], buf[10], buf[11]],
            });
        }

        let mut packets = Vec::with_capacity(header.theora_packet_cnt as usize);
        for _ in 0..header.theora_packet_cnt {
            let mut buf = [0u8; 8];
            f.read_exact(&mut buf)?;
            packets.push(OmvTheoraPacket {
                ofs: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
                time: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
            });
        }

        let ogg_data_offset = f.seek(SeekFrom::Current(0))?;
        Ok(Self {
            header,
            pages,
            packets,
            ogg_data_offset,
        })
    }

    /// Read the embedded Ogg bitstream (Theora) as bytes.
    pub fn read_embedded_ogg(path: impl AsRef<Path>) -> Result<Vec<u8>> {
        let omv = Self::open(&path)?;
        let mut f = File::open(&path)?;
        f.seek(SeekFrom::Start(omv.ogg_data_offset))?;
        let mut data = Vec::new();
        f.read_to_end(&mut data)?;
        Ok(data)
    }
}

fn read_header(f: &mut File) -> Result<OmvHeader> {
    // The on-disk header is a packed POD struct (little-endian).
    // Layout (little-endian):
    // id[4], version u8, fps u8, reserved u16,
    // codec_name[32], frame_count u32, max_data_size u32,
    // base_size (i32,i32), cut_left_top (i32,i32), cut_right_bottom (i32,i32),
    // theora_page_cnt u32, theora_page_size u32, theora_packet_cnt u32, theora_packet_size u32.
    let mut buf = [0u8; 4 + 1 + 1 + 2 + 32 + 4 + 4 + 8 + 8 + 8 + 4 + 4 + 4 + 4];
    f.read_exact(&mut buf)?;
    let mut o = 0usize;
    let mut id = [0u8; 4];
    id.copy_from_slice(&buf[o..o + 4]);
    o += 4;
    let version = buf[o];
    o += 1;
    let fps = buf[o];
    o += 1;
    let reserved = u16::from_le_bytes([buf[o], buf[o + 1]]);
    o += 2;
    let mut codec_name = [0u8; 32];
    codec_name.copy_from_slice(&buf[o..o + 32]);
    o += 32;
    let frame_count = u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    o += 4;
    let max_data_size = u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    o += 4;
    let base_size = (
        i32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]),
        i32::from_le_bytes([buf[o + 4], buf[o + 5], buf[o + 6], buf[o + 7]]),
    );
    o += 8;
    let cut_left_top = (
        i32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]),
        i32::from_le_bytes([buf[o + 4], buf[o + 5], buf[o + 6], buf[o + 7]]),
    );
    o += 8;
    let cut_right_bottom = (
        i32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]),
        i32::from_le_bytes([buf[o + 4], buf[o + 5], buf[o + 6], buf[o + 7]]),
    );
    o += 8;
    let theora_page_cnt = u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    o += 4;
    let theora_page_size = u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    o += 4;
    let theora_packet_cnt = u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    o += 4;
    let theora_packet_size = u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);

    Ok(OmvHeader {
        id,
        version,
        fps,
        reserved,
        codec_name,
        frame_count,
        max_data_size,
        base_size,
        cut_left_top,
        cut_right_bottom,
        theora_page_cnt,
        theora_page_size,
        theora_packet_cnt,
        theora_packet_size,
    })
}
