//! G00 decoder.
//!
//! Implemented based on the the original implementation extractor logic provided by the user.
//!
//! Output format: RGBA8.

use crate::assets::RgbaImage;
use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum G00Type {
    Type24bit = 0,
    Type8bit = 1,
    TypeDir = 2,
    TypeJpeg = 3,
}

#[derive(Debug, Clone)]
pub struct DecodedG00 {
    pub kind: G00Type,
    pub width: u32,
    pub height: u32,
    /// For TypeDir, this contains multiple frames.
    pub frames: Vec<RgbaImage>,
}

fn read_u16le(buf: &[u8], off: usize) -> Result<u16> {
    if off + 2 > buf.len() {
        bail!("read u16le out of bounds at {off}");
    }
    Ok(u16::from_le_bytes([buf[off], buf[off + 1]]))
}

fn read_u32le(buf: &[u8], off: usize) -> Result<u32> {
    if off + 4 > buf.len() {
        bail!("read u32le out of bounds at {off}");
    }
    Ok(u32::from_le_bytes([
        buf[off],
        buf[off + 1],
        buf[off + 2],
        buf[off + 3],
    ]))
}

/// Decode a `.g00` file into RGBA frames.
pub fn decode_g00(data: &[u8]) -> Result<DecodedG00> {
    if data.len() < 1 + 2 + 2 {
        bail!("g00 too small");
    }
    let ty = data[0];
    let width = read_u16le(data, 1)? as u32;
    let height = read_u16le(data, 3)? as u32;
    let kind = match ty {
        0 => G00Type::Type24bit,
        1 => G00Type::Type8bit,
        2 => G00Type::TypeDir,
        3 => G00Type::TypeJpeg,
        other => bail!("unknown g00 type: {other}"),
    };

    let mut off = 5;

    match kind {
        G00Type::Type24bit => {
            // lzss_compress_head_t { compress_length, decompress_length }
            let decompress_length = read_u32le(data, off + 4)? as usize;
            off += 8;
            if off > data.len() {
                bail!("g00 type0 header out of bounds");
            }
            if decompress_length == 0 {
                bail!("g00 type0 decompress_length=0");
            }
            let mut out = vec![0u8; decompress_length];
            lzss_decompress_24bit(&data[off..], &mut out).context("lzss_decompress_24bit")?;

            // out is BGRA (alpha already 255). Convert to RGBA.
            let rgba = bgra_to_rgba_inplace(out);
            Ok(DecodedG00 {
                kind,
                width,
                height,
                frames: vec![RgbaImage {
                    width,
                    height,
                    rgba,
                }],
            })
        }
        G00Type::Type8bit => {
            // RealLive_g00_type1_uncompress
            let (mut out, out_len) =
                real_live_type1_uncompress(&data[off..]).context("type1 uncompress")?;
            if out_len == 0 {
                bail!("type1 produced empty output");
            }
            out.truncate(out_len);
            // output is BGRA
            let rgba = bgra_to_rgba_inplace(out);
            Ok(DecodedG00 {
                kind,
                width,
                height,
                frames: vec![RgbaImage {
                    width,
                    height,
                    rgba,
                }],
            })
        }
        G00Type::TypeDir => {
            // type2: index_entries + g02_info_list + LZSS payload
            // We do not need g02_info_list for image extraction in this stage, but we must skip it.
            let index_entries = read_u32le(data, off)? as usize;
            off += 4;
            // g02_info_t is 24 bytes in the provided extractor.
            let g02_info_size = 24usize;
            let skip = index_entries
                .checked_mul(g02_info_size)
                .context("index_entries overflow")?;
            if off + skip > data.len() {
                bail!("type2 g02_info_list out of bounds");
            }
            off += skip;

            // lzss_compress_head_t
            let decompress_length = read_u32le(data, off + 4)? as usize;
            off += 8;
            if decompress_length == 0 {
                bail!("type2 decompress_length=0");
            }
            if off > data.len() {
                bail!("type2 payload out of bounds");
            }

            let mut debuf = vec![0u8; decompress_length];
            lzss_decompress(&data[off..], &mut debuf).context("lzss_decompress")?;

            // debuf: u32 entries, then entries * {u32 offset,u32 length}
            if debuf.len() < 4 {
                bail!("type2 debuf too small");
            }
            let debuf_entries = read_u32le(&debuf, 0)? as usize;
            let pairs_off = 4usize;
            let pair_size = 8usize;
            let pairs_bytes = debuf_entries
                .checked_mul(pair_size)
                .context("debuf_entries overflow")?;
            if pairs_off + pairs_bytes > debuf.len() {
                bail!("type2 pairs out of bounds");
            }

            let mut frames: Vec<RgbaImage> = Vec::new();
            for i in 0..debuf_entries {
                let p_off = pairs_off + i * pair_size;
                let offset = read_u32le(&debuf, p_off)? as usize;
                let length = read_u32le(&debuf, p_off + 4)? as usize;
                if offset == 0 || length == 0 {
                    continue;
                }
                if offset + length > debuf.len() {
                    // Some files may store length but not used; we keep strict for now.
                    bail!("type2 pair {i} out of bounds: off={offset} len={length}");
                }
                let part_bytes = &debuf[offset..offset + length];
                let img = extract_g02_part(part_bytes)
                    .with_context(|| format!("extract g02 part idx={i}"))?;
                frames.push(img);
            }

            if frames.is_empty() {
                bail!("type2 produced no frames");
            }

            Ok(DecodedG00 {
                kind,
                width,
                height,
                frames,
            })
        }
        G00Type::TypeJpeg => {
            if off > data.len() {
                bail!("g00 type3 header out of bounds");
            }
            let jpeg = &data[off..];
            let img = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)
                .or_else(|_| image::load_from_memory(jpeg))
                .context("decode g00 jpeg")?;
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            if w != width || h != height {
                bail!("g00 jpeg size mismatch: got={w}x{h}, expect={width}x{height}");
            }
            Ok(DecodedG00 {
                kind,
                width,
                height,
                frames: vec![RgbaImage {
                    width,
                    height,
                    rgba: rgba.into_raw(),
                }],
            })
        }
    }
}

fn bgra_to_rgba_inplace(mut bgra: Vec<u8>) -> Vec<u8> {
    for px in bgra.chunks_exact_mut(4) {
        let b = px[0];
        let g = px[1];
        let r = px[2];
        let a = px[3];
        px[0] = r;
        px[1] = g;
        px[2] = b;
        px[3] = a;
    }
    bgra
}

fn real_live_type1_uncompress(compr: &[u8]) -> Result<(Vec<u8>, usize)> {
    if compr.len() < 8 {
        bail!("type1 data too small");
    }
    let total_len = read_u32le(compr, 0)? as usize;
    let uncomprlen = read_u32le(compr, 4)? as usize;
    if total_len < 8 {
        bail!("type1 total_len < 8");
    }
    if total_len > compr.len() {
        // Be strict: extractor uses the length to limit parsing.
        bail!(
            "type1 total_len out of bounds: total_len={total_len} buf={}",
            compr.len()
        );
    }

    if uncomprlen != 0 {
        let mut out = vec![0u8; uncomprlen + 64];
        let mut curbyte = 8usize;
        let mut act = 0usize;
        let mut bit_count = 0u8;
        let mut flag = 0u8;

        while act < uncomprlen && curbyte < total_len {
            if bit_count == 0 {
                flag = compr[curbyte];
                curbyte += 1;
                bit_count = 8;
            }

            if (flag & 1) != 0 {
                if curbyte >= total_len {
                    break;
                }
                out[act] = compr[curbyte];
                act += 1;
                curbyte += 1;
            } else {
                if curbyte + 2 > total_len {
                    break;
                }
                let count0 = compr[curbyte] as usize;
                let b1 = compr[curbyte + 1] as usize;
                curbyte += 2;

                let offset = (b1 << 4) | (count0 >> 4);
                let count = (count0 & 0xF) + 2;
                if offset == 0 {
                    bail!("type1 invalid offset=0");
                }
                if act < offset {
                    bail!("type1 backref before start: act={act} offset={offset}");
                }
                for _ in 0..count {
                    if act >= uncomprlen {
                        break;
                    }
                    let v = out[act - offset];
                    out[act] = v;
                    act += 1;
                }
            }

            flag >>= 1;
            bit_count = bit_count.saturating_sub(1);
        }

        Ok((out, uncomprlen))
    } else {
        let payload_len = total_len - 8;
        let mut out = vec![0u8; payload_len];
        out.copy_from_slice(&compr[8..8 + payload_len]);
        Ok((out, payload_len))
    }
}

fn lzss_decompress(src: &[u8], dst: &mut [u8]) -> Result<()> {
    let mut s = 0usize;
    let mut d = 0usize;
    while d < dst.len() {
        if s >= src.len() {
            break;
        }
        let mut flags = src[s];
        s += 1;
        for _ in 0..8 {
            if d >= dst.len() {
                break;
            }
            if (flags & 1) != 0 {
                if s >= src.len() {
                    break;
                }
                dst[d] = src[s];
                d += 1;
                s += 1;
            } else {
                if s + 2 > src.len() {
                    break;
                }
                let w = u16::from_le_bytes([src[s], src[s + 1]]) as usize;
                s += 2;
                let offset = w >> 4;
                let count = (w & 0xF) + 2;
                if offset == 0 {
                    bail!("lzss offset=0");
                }
                if d < offset {
                    bail!("lzss backref before start: d={d} offset={offset}");
                }
                for _ in 0..count {
                    if d >= dst.len() {
                        break;
                    }
                    let v = dst[d - offset];
                    dst[d] = v;
                    d += 1;
                }
            }
            flags >>= 1;
        }
    }

    if d != dst.len() {
        bail!(
            "lzss_decompress did not fill output: wrote {d} of {}",
            dst.len()
        );
    }
    Ok(())
}

fn lzss_decompress_24bit(src: &[u8], dst: &mut [u8]) -> Result<()> {
    // the original implementation extractor emits BGRA (alpha byte set to 0xFF).
    let mut s = 0usize;
    let mut d = 0usize;
    while d < dst.len() {
        if s >= src.len() {
            break;
        }
        let mut flags = src[s];
        s += 1;
        for _ in 0..8 {
            if d >= dst.len() {
                break;
            }
            if (flags & 1) != 0 {
                if s + 3 > src.len() {
                    break;
                }
                if d + 4 > dst.len() {
                    bail!("lzss24 literal would overflow dst");
                }
                // movsw; movsb; then alpha=0xFF
                dst[d] = src[s];
                dst[d + 1] = src[s + 1];
                dst[d + 2] = src[s + 2];
                dst[d + 3] = 0xFF;
                d += 4;
                s += 3;
            } else {
                if s + 2 > src.len() {
                    break;
                }
                let w = u16::from_le_bytes([src[s], src[s + 1]]) as usize;
                s += 2;
                let offset_bytes = (w >> 4) << 2; // (word>>4)*4
                let dword_count = (w & 0xF) + 1;
                let count_bytes = dword_count * 4;
                if offset_bytes == 0 {
                    bail!("lzss24 offset_bytes=0");
                }
                if d < offset_bytes {
                    bail!("lzss24 backref before start: d={d} offset={offset_bytes}");
                }
                for _ in 0..count_bytes {
                    if d >= dst.len() {
                        break;
                    }
                    let v = dst[d - offset_bytes];
                    dst[d] = v;
                    d += 1;
                }
            }
            flags >>= 1;
        }
    }

    if d != dst.len() {
        bail!(
            "lzss_decompress_24bit did not fill output: wrote {d} of {}",
            dst.len()
        );
    }
    Ok(())
}

#[derive(Debug, Clone)]
struct G02PartInfo {
    part_type: u16,
    block_count: u16,
    hs_orig_x: u32,
    hs_orig_y: u32,
    width: u32,
    height: u32,
    screen_show_x: u32,
    screen_show_y: u32,
    full_part_width: u32,
    full_part_height: u32,
}

#[derive(Debug, Clone)]
struct G02BlockInfo {
    orig_x: u16,
    orig_y: u16,
    _info: u16,
    width: u16,
    height: u16,
}

const G02_BLOCK_INFO_SIZE: usize = 92; // sizeof(g02_block_info_t) in the provided extractor

fn parse_g02_block(buf: &[u8]) -> Result<G02BlockInfo> {
    if buf.len() < G02_BLOCK_INFO_SIZE {
        bail!("g02_block_info_t truncated");
    }
    let orig_x = read_u16le(buf, 0)?;
    let orig_y = read_u16le(buf, 2)?;
    let info = read_u16le(buf, 4)?;
    let width = read_u16le(buf, 6)?;
    let height = read_u16le(buf, 8)?;
    Ok(G02BlockInfo {
        orig_x,
        orig_y,
        _info: info,
        width,
        height,
    })
}

fn parse_g02_part_info_prefix(buf: &[u8]) -> Result<G02PartInfo> {
    // We only rely on the fixed prefix fields used by the extractor.
    // Offset mapping follows the struct layout in the provided the original implementation code.
    if buf.len() < 0x24 {
        bail!("g02_part_info prefix too small");
    }
    let part_type = read_u16le(buf, 0)?;
    let block_count = read_u16le(buf, 2)?;
    let hs_orig_x = read_u32le(buf, 4)?;
    let hs_orig_y = read_u32le(buf, 8)?;
    let width = read_u32le(buf, 0x0C)?;
    let height = read_u32le(buf, 0x10)?;
    let screen_show_x = read_u32le(buf, 0x14)?;
    let screen_show_y = read_u32le(buf, 0x18)?;
    let full_part_width = read_u32le(buf, 0x1C)?;
    let full_part_height = read_u32le(buf, 0x20)?;
    Ok(G02PartInfo {
        part_type,
        block_count,
        hs_orig_x,
        hs_orig_y,
        width,
        height,
        screen_show_x,
        screen_show_y,
        full_part_width,
        full_part_height,
    })
}

fn fix_vertical_flip_bgra(width: u32, height: u32, buf: &mut [u8]) -> Result<()> {
    let stride = width.checked_mul(4).context("stride overflow")? as usize;
    let h = height as usize;
    if buf.len() != stride * h {
        bail!("fix_vertical_flip_bgra length mismatch");
    }
    let mut tmp = vec![0u8; buf.len()];
    for y in 0..h {
        let src_off = y * stride;
        let dst_off = (h - 1 - y) * stride;
        tmp[dst_off..dst_off + stride].copy_from_slice(&buf[src_off..src_off + stride]);
    }
    buf.copy_from_slice(&tmp);
    Ok(())
}

fn extract_g02_part(part_bytes: &[u8]) -> Result<RgbaImage> {
    // The extractor supports type 1/2 similarly; we handle the copy logic for type 2.
    let part = parse_g02_part_info_prefix(part_bytes).context("parse part prefix")?;
    if part.width == 0 || part.height == 0 {
        bail!("g02 part has zero dimensions");
    }

    // Heuristic: part header size is not consistent across implementations.
    // We try a few known header sizes and validate by parsing block headers + pixel chunks sequentially.
    // Candidates include 0x24 (prefix-only), 0x74 (one observed), 0xD0 (common RealLive/Siglus).
    let candidates: [usize; 10] = [0x24, 0x74, 0xD0, 0xC0, 0xE0, 0x80, 0x90, 0xA0, 0xB0, 0x100];

    let mut chosen_header: Option<usize> = None;
    for &hdr in &candidates {
        if hdr > part_bytes.len() {
            continue;
        }
        if validate_g02_layout(part_bytes, &part, hdr).is_ok() {
            chosen_header = Some(hdr);
            break;
        }
    }

    let header_size = chosen_header.context("unable to determine g02_part_info header size")?;

    // Allocate image (BGRA) and fill blocks.
    let stride = (part.width as usize)
        .checked_mul(4)
        .context("stride overflow")?;
    let mut dib = vec![0u8; stride * (part.height as usize)];

    let mut off = header_size;
    for _ in 0..part.block_count {
        let block = parse_g02_block(&part_bytes[off..])?;
        off += G02_BLOCK_INFO_SIZE;

        let bw = block.width as usize;
        let bh = block.height as usize;
        if bw == 0 || bh == 0 {
            bail!("g02 block zero size");
        }
        let px_len = bw
            .checked_mul(bh)
            .and_then(|v| v.checked_mul(4))
            .context("pixel len overflow")?;
        if off + px_len > part_bytes.len() {
            bail!("g02 block pixel data out of bounds");
        }
        let src = &part_bytes[off..off + px_len];
        off += px_len;

        let dst_x = (block.orig_x as i64) - (part.hs_orig_x as i64);
        let dst_y = (block.orig_y as i64) - (part.hs_orig_y as i64);
        if dst_x < 0 || dst_y < 0 {
            // Strict: the extractor assumes these are within.
            bail!("g02 block position before hotspot origin");
        }

        let dst_x = dst_x as usize;
        let dst_y = dst_y as usize;

        // Copy row by row (same as extractor's part_extract_buf).
        for row in 0..bh {
            let src_row_off = row * bw * 4;
            let dst_row_off = (dst_y + row) * stride + dst_x * 4;
            let dst_end = dst_row_off + bw * 4;
            if dst_end > dib.len() {
                bail!("g02 block write out of bounds");
            }
            dib[dst_row_off..dst_end].copy_from_slice(&src[src_row_off..src_row_off + bw * 4]);
        }
    }

    // Match extractor behavior: flip vertically.
    fix_vertical_flip_bgra(part.width, part.height, &mut dib)?;

    // Convert to RGBA.
    let rgba = bgra_to_rgba_inplace(dib);

    Ok(RgbaImage {
        width: part.width,
        height: part.height,
        rgba,
    })
}

fn validate_g02_layout(part_bytes: &[u8], part: &G02PartInfo, header_size: usize) -> Result<()> {
    let mut off = header_size;
    for _ in 0..part.block_count {
        if off + G02_BLOCK_INFO_SIZE > part_bytes.len() {
            bail!("block header out of bounds");
        }
        let block = parse_g02_block(&part_bytes[off..off + G02_BLOCK_INFO_SIZE])?;
        off += G02_BLOCK_INFO_SIZE;

        if block.width == 0 || block.height == 0 {
            bail!("block zero size");
        }
        if (block.width as u32) > part.width || (block.height as u32) > part.height {
            bail!("block larger than part");
        }
        // Many files keep reserved zeros; we don't strictly check reserved bytes here.

        let bw = block.width as usize;
        let bh = block.height as usize;
        let px_len = bw
            .checked_mul(bh)
            .and_then(|v| v.checked_mul(4))
            .context("pixel len overflow")?;
        if off + px_len > part_bytes.len() {
            bail!("block pixels out of bounds");
        }
        off += px_len;
    }
    Ok(())
}
