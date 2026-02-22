//! NWA audio container support.
//!
//! Decoder implementation for the NWA container format.
//!
//! Notes:
//! - `set_read_sample_pos` and `read_samples` treat one *frame* as one sample
//!   even for stereo (i.e. a frame contains L+R).
//! - For compressed NWA (`pack_mod != -1`), the current implementation supports
//!   16-bit PCM output, matching the original decoder.

use anyhow::{bail, Context, Result};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Debug, Clone)]
pub struct NwaHeader {
    pub channels: u16,
    pub bits_per_sample: u16,
    pub samples_per_sec: u32,
    pub pack_mod: i32,
    pub zero_mod: i32,
    pub unit_cnt: u32,
    pub original_size: u32,
    pub pack_size: u32,
    pub sample_cnt: u32,
    pub unit_sample_cnt: u32,
    pub last_sample_cnt: u32,
    pub last_sample_pack_size: u32,
}

impl NwaHeader {
    pub fn is_uncompressed(&self) -> bool {
        self.pack_mod == -1
    }

    pub fn frame_count(&self) -> u32 {
        if self.channels == 0 {
            return 0;
        }
        self.sample_cnt / (self.channels as u32)
    }
}

#[derive(Debug)]
struct UnitCache {
    unit_no: i32,
    unit_sample_cnt: u32,
    buf: Vec<u8>,
}

impl UnitCache {
    fn new() -> Self {
        Self {
            unit_no: -1,
            unit_sample_cnt: 0,
            buf: Vec::new(),
        }
    }
}

/// NWA reader with random access by frame index.
#[derive(Debug)]
pub struct NwaReader {
    file: File,
    base_offset: u64,
    header: NwaHeader,
    unit_offsets: Vec<u32>,
    one_sample_byte_size: u32,
    read_sample_pos: u32,
    cache: UnitCache,
}

impl NwaReader {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        Self::open_with_offset(path, 0)
    }

    pub fn open_with_offset(path: impl AsRef<Path>, base_offset: u64) -> Result<Self> {
        let mut file = File::open(&path).with_context(|| format!("open NWA: {}", path.as_ref().display()))?;
        if base_offset != 0 {
            file.seek(SeekFrom::Start(base_offset))?;
        }
        let header = read_header(&mut file)?;

        let mut unit_offsets = Vec::new();
        if header.pack_mod != -1 {
            unit_offsets.resize(header.unit_cnt as usize, 0);
            let mut tmp = vec![0u8; header.unit_cnt as usize * 4];
            file.read_exact(&mut tmp)?;
            for i in 0..header.unit_cnt as usize {
                let o = u32::from_le_bytes([
                    tmp[i * 4],
                    tmp[i * 4 + 1],
                    tmp[i * 4 + 2],
                    tmp[i * 4 + 3],
                ]);
                unit_offsets[i] = o;
            }
        }

        let one_sample_byte_size = (header.bits_per_sample as u32) / 8;

        Ok(Self {
            file,
            base_offset,
            header,
            unit_offsets,
            one_sample_byte_size,
            read_sample_pos: 0,
            cache: UnitCache::new(),
        })
    }

    pub fn header(&self) -> &NwaHeader {
        &self.header
    }

    /// Set current read position in frames.
    pub fn set_read_sample_pos(&mut self, frame_pos: u32) {
        self.read_sample_pos = frame_pos.saturating_mul(self.header.channels as u32);
    }

    /// Get current read position in frames.
    pub fn get_read_sample_pos(&self) -> u32 {
        let ch = self.header.channels as u32;
        if ch == 0 {
            0
        } else {
            self.read_sample_pos / ch
        }
    }

    /// Read `frame_cnt` frames into an interleaved PCM byte buffer.
    ///
    /// Returns exactly `frames_read` frames worth of data.
    pub fn read_samples(&mut self, frame_cnt: u32) -> Result<Vec<u8>> {
        if self.header.channels != 1 && self.header.channels != 2 {
            bail!("unsupported channels: {}", self.header.channels);
        }
        if self.header.bits_per_sample != 8 && self.header.bits_per_sample != 16 {
            bail!("unsupported bits_per_sample: {}", self.header.bits_per_sample);
        }

        let need_byte_size = (frame_cnt as u64)
            * (self.header.channels as u64)
            * (self.one_sample_byte_size as u64);
        let mut out = vec![0u8; need_byte_size as usize];
        let mut dp = 0usize;
        let mut need = need_byte_size as i64;

        while need > 0 {
            if self.read_sample_pos >= self.header.sample_cnt {
                break;
            }

            let copy_byte_size: usize;
            if self.header.pack_mod == -1 {
                // Uncompressed: data starts immediately after the header.
                copy_byte_size = self.read_no_pack_data(need as u64, &mut out[dp..])?;
            } else {
                // Compressed: decode the unit if needed, then copy from cached buffer.
                let unit_no = (self.read_sample_pos / self.header.unit_sample_cnt) as u32;
                self.read_unit(unit_no)?;

                let ofs = ((self.read_sample_pos % self.header.unit_sample_cnt) as usize)
                    * (self.one_sample_byte_size as usize);
                let mut cb = (self.cache.unit_sample_cnt as usize * self.one_sample_byte_size as usize)
                    .saturating_sub(ofs);
                if cb > need as usize {
                    cb = need as usize;
                }
                if cb == 0 {
                    break;
                }
                out[dp..dp + cb].copy_from_slice(&self.cache.buf[ofs..ofs + cb]);
                copy_byte_size = cb;
            }

            if copy_byte_size == 0 {
                break;
            }
            dp += copy_byte_size;
            need -= copy_byte_size as i64;
            self.read_sample_pos += (copy_byte_size as u32) / self.one_sample_byte_size;
        }

        out.truncate(dp);
        Ok(out)
    }

    /// Convert the entire NWA stream into a WAV (RIFF PCM) byte vector.
    pub fn to_wav_bytes(&mut self) -> Result<Vec<u8>> {
        let total_frames = self.header.frame_count();
        self.set_read_sample_pos(0);
        let pcm = self.read_samples(total_frames)?;

        let fmt_tag: u16 = 1; // PCM
        let channels = self.header.channels;
        let sample_rate = self.header.samples_per_sec;
        let bits = self.header.bits_per_sample;
        let block_align = (channels as u32 * (bits as u32 / 8)) as u16;
        let byte_rate = sample_rate * (block_align as u32);
        let data_len = pcm.len() as u32;

        let mut wav = Vec::with_capacity(44 + pcm.len());
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36u32 + data_len).to_le_bytes());
        wav.extend_from_slice(b"WAVE");

        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&fmt_tag.to_le_bytes());
        wav.extend_from_slice(&channels.to_le_bytes());
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&byte_rate.to_le_bytes());
        wav.extend_from_slice(&block_align.to_le_bytes());
        wav.extend_from_slice(&bits.to_le_bytes());

        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_len.to_le_bytes());
        wav.extend_from_slice(&pcm);
        Ok(wav)
    }

    fn read_no_pack_data(&mut self, need_byte_size: u64, out: &mut [u8]) -> Result<usize> {
        let data_ofs = (self.read_sample_pos as u64) * (self.one_sample_byte_size as u64);
        let file_ofs = self.base_offset + 44 + data_ofs; // sizeof(NWA_HEADER_STRUCT)

        self.file.seek(SeekFrom::Start(file_ofs))?;

        // Clamp at end.
        let max_data_len = (self.header.sample_cnt as u64) * (self.one_sample_byte_size as u64);
        let remain = max_data_len.saturating_sub(data_ofs);
        let to_read = std::cmp::min(need_byte_size, std::cmp::min(remain, out.len() as u64));
        let mut buf = &mut out[..to_read as usize];
        self.file.read_exact(&mut buf)?;
        Ok(to_read as usize)
    }

    fn read_unit(&mut self, unit_no: u32) -> Result<()> {
        if self.cache.unit_no == unit_no as i32 {
            return Ok(());
        }

        if unit_no >= self.header.unit_cnt {
            bail!("unit_no out of range: {} >= {}", unit_no, self.header.unit_cnt);
        }

        if self.header.pack_mod == -1 {
            bail!("read_unit called for uncompressed NWA");
        }
        if self.header.bits_per_sample != 16 {
            bail!("compressed NWA currently requires 16-bit PCM (got {})", self.header.bits_per_sample);
        }

        let (unit_sample_cnt, src_size) = if unit_no == self.header.unit_cnt - 1 {
            (self.header.last_sample_cnt, self.header.last_sample_pack_size)
        } else {
            let a = self.unit_offsets[unit_no as usize] as u64;
            let b = self.unit_offsets[unit_no as usize + 1] as u64;
            (self.header.unit_sample_cnt, (b.saturating_sub(a)) as u32)
        };

        let src_ofs = self.base_offset + (self.unit_offsets[unit_no as usize] as u64);
        self.file.seek(SeekFrom::Start(src_ofs))?;
        let mut src = vec![0u8; src_size as usize];
        self.file.read_exact(&mut src)?;

        self.cache.buf.resize(unit_sample_cnt as usize * 2, 0);
        nwa_unpack_unit(
            &src,
            self.header.pack_mod,
            self.header.zero_mod != 0,
            unit_sample_cnt,
            &mut self.cache.buf,
        )?;

        self.cache.unit_no = unit_no as i32;
        self.cache.unit_sample_cnt = unit_sample_cnt;
        Ok(())
    }
}

fn read_header(file: &mut File) -> Result<NwaHeader> {
    let mut buf = [0u8; 44];
    file.read_exact(&mut buf)?;
    let mut o = 0usize;

    let channels = u16::from_le_bytes([buf[o], buf[o + 1]]);
    o += 2;
    let bits_per_sample = u16::from_le_bytes([buf[o], buf[o + 1]]);
    o += 2;
    let samples_per_sec = u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    o += 4;
    let pack_mod = i32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    o += 4;
    let zero_mod = i32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    o += 4;
    let unit_cnt = u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    o += 4;
    let original_size = u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    o += 4;
    let pack_size = u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    o += 4;
    let sample_cnt = u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    o += 4;
    let unit_sample_cnt = u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    o += 4;
    let last_sample_cnt = u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);
    o += 4;
    let last_sample_pack_size = u32::from_le_bytes([buf[o], buf[o + 1], buf[o + 2], buf[o + 3]]);

    Ok(NwaHeader {
        channels,
        bits_per_sample,
        samples_per_sec,
        pack_mod,
        zero_mod,
        unit_cnt,
        original_size,
        pack_size,
        sample_cnt,
        unit_sample_cnt,
        last_sample_cnt,
        last_sample_pack_size,
    })
}

struct BitReaderLE<'a> {
    buf: &'a [u8],
    byte_pos: usize,
    bit_pos: u8,
}

impl<'a> BitReaderLE<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self {
            buf,
            byte_pos: 0,
            bit_pos: 0,
        }
    }

    fn read_bits(&mut self, n: u8) -> u32 {
        debug_assert!(n <= 8);
        let b0 = *self.buf.get(self.byte_pos).unwrap_or(&0);
        let b1 = *self.buf.get(self.byte_pos + 1).unwrap_or(&0);
        let w = u16::from_le_bytes([b0, b1]);
        let v = ((w as u32) >> (self.bit_pos as u32)) & ((1u32 << n) - 1);

        self.bit_pos = self.bit_pos.wrapping_add(n);
        if self.bit_pos >= 8 {
            self.byte_pos += (self.bit_pos / 8) as usize;
            self.bit_pos &= 7;
        }
        v
    }
}

fn nwa_unpack_unit(
    src: &[u8],
    pack_mod: i32,
    zero_mod: bool,
    src_smp_cnt: u32,
    dst_le_i16: &mut [u8],
) -> Result<()> {
    if dst_le_i16.len() < src_smp_cnt as usize * 2 {
        bail!("dst buffer too small");
    }

    let mut br = BitReaderLE::new(src);
    let mut now_l: i32 = 0;
    let mut now_r: i32 = 0;
    let mut zero_cnt: u32 = 0;

    let mut mod_map = pack_mod;
    match mod_map {
        0 => mod_map = 2,
        1 => mod_map = 1,
        2 => mod_map = 0,
        _ => {}
    }

    match mod_map {
        0 => unpack16(&mut br, zero_mod, src_smp_cnt, dst_le_i16, 3, &mut now_l, &mut now_r, &mut zero_cnt),
        1 => unpack16(&mut br, zero_mod, src_smp_cnt, dst_le_i16, 4, &mut now_l, &mut now_r, &mut zero_cnt),
        2 => unpack16(&mut br, zero_mod, src_smp_cnt, dst_le_i16, 5, &mut now_l, &mut now_r, &mut zero_cnt),
        3 => unpack16(&mut br, zero_mod, src_smp_cnt, dst_le_i16, 6, &mut now_l, &mut now_r, &mut zero_cnt),
        4 => unpack16(&mut br, zero_mod, src_smp_cnt, dst_le_i16, 7, &mut now_l, &mut now_r, &mut zero_cnt),
        5 => unpack16(&mut br, zero_mod, src_smp_cnt, dst_le_i16, 8, &mut now_l, &mut now_r, &mut zero_cnt),
        _ => bail!("invalid pack_mod: {}", pack_mod),
    }
}

fn get_zero_count(br: &mut BitReaderLE) -> u32 {
    let mut zero_cnt = br.read_bits(1);
    if zero_cnt == 1 {
        zero_cnt = br.read_bits(2);
        if zero_cnt == 3 {
            zero_cnt = br.read_bits(8);
        }
    }
    zero_cnt as u32
}

fn apply_delta(br: &mut BitReaderLE, mod_n: u8, data_n: u8, nowsmp: &mut i32) {
    let (bits, sign_mask, shift): (u8, u32, u8) = match (mod_n, data_n) {
        (3, 1) => (3, 0x04, 5),
        (3, 2) => (3, 0x04, 6),
        (3, 3) => (3, 0x04, 7),
        (3, 4) => (3, 0x04, 8),
        (3, 5) => (3, 0x04, 9),
        (3, 6) => (3, 0x04, 10),
        (3, 7) => (6, 0x20, 11),

        (4, 1) => (4, 0x08, 4),
        (4, 2) => (4, 0x08, 5),
        (4, 3) => (4, 0x08, 6),
        (4, 4) => (4, 0x08, 7),
        (4, 5) => (4, 0x08, 8),
        (4, 6) => (4, 0x08, 9),
        (4, 7) => (7, 0x40, 10),

        (5, 1) => (5, 0x10, 3),
        (5, 2) => (5, 0x10, 4),
        (5, 3) => (5, 0x10, 5),
        (5, 4) => (5, 0x10, 6),
        (5, 5) => (5, 0x10, 7),
        (5, 6) => (5, 0x10, 8),
        (5, 7) => (8, 0x80, 9),

        (6, 1) => (6, 0x20, 2),
        (6, 2) => (6, 0x20, 3),
        (6, 3) => (6, 0x20, 4),
        (6, 4) => (6, 0x20, 5),
        (6, 5) => (6, 0x20, 6),
        (6, 6) => (6, 0x20, 7),
        (6, 7) => (8, 0x80, 9),

        (7, 1) => (7, 0x40, 2),
        (7, 2) => (7, 0x40, 3),
        (7, 3) => (7, 0x40, 4),
        (7, 4) => (7, 0x40, 5),
        (7, 5) => (7, 0x40, 6),
        (7, 6) => (7, 0x40, 7),
        (7, 7) => (8, 0x80, 9),

        (8, 1) => (8, 0x80, 2),
        (8, 2) => (8, 0x80, 3),
        (8, 3) => (8, 0x80, 4),
        (8, 4) => (8, 0x80, 5),
        (8, 5) => (8, 0x80, 6),
        (8, 6) => (8, 0x80, 7),
        (8, 7) => (8, 0x80, 9),
        _ => return,
    };

    let mut dat_code = br.read_bits(bits);
    if (dat_code & sign_mask) != 0 {
        dat_code &= !sign_mask;
        *nowsmp -= ((dat_code as i32) << shift);
    } else {
        *nowsmp += ((dat_code as i32) << shift);
    }
}

fn unpack16(
    br: &mut BitReaderLE,
    zero_mod: bool,
    src_smp_cnt: u32,
    dst_le_i16: &mut [u8],
    mod_n: u8,
    now_l: &mut i32,
    now_r: &mut i32,
    zero_cnt: &mut u32,
) -> Result<()> {
    for i in 0..src_smp_cnt {
        let nowsmp_ref: &mut i32 = if (i & 1) == 0 { now_l } else { now_r };

        if *zero_cnt != 0 {
            *zero_cnt -= 1;
        } else {
            let mod_code = br.read_bits(3) as u8;
            match mod_code {
                0 => {
                    if zero_mod {
                        *zero_cnt = get_zero_count(br);
                    }
                }
                1 => apply_delta(br, mod_n, 1, nowsmp_ref),
                2 => apply_delta(br, mod_n, 2, nowsmp_ref),
                3 => apply_delta(br, mod_n, 3, nowsmp_ref),
                4 => apply_delta(br, mod_n, 4, nowsmp_ref),
                5 => apply_delta(br, mod_n, 5, nowsmp_ref),
                6 => apply_delta(br, mod_n, 6, nowsmp_ref),
                7 => {
                    let b = br.read_bits(1);
                    if b == 0 {
                        apply_delta(br, mod_n, 7, nowsmp_ref);
                    } else {
                        *nowsmp_ref = 0;
                    }
                }
                _ => {}
            }
        }

        // Write i16 little-endian.
        let s = (*nowsmp_ref as i16).to_le_bytes();
        let o = (i as usize) * 2;
        dst_le_i16[o] = s[0];
        dst_le_i16[o + 1] = s[1];
    }
    Ok(())
}
