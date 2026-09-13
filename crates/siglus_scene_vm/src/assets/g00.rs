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
    /// Per-cut canvas dimensions before display-rectangle cropping.
    /// These are C_d3d_texture::get_original_width/height(), used by scripts.
    pub original_sizes: Vec<(u32, u32)>,
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

fn read_i32le(buf: &[u8], off: usize) -> Result<i32> {
    if off + 4 > buf.len() {
        bail!("read i32le out of bounds at {off}");
    }
    Ok(i32::from_le_bytes([
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

            // Convert BGR literals to the engine's RGBA representation as
            // they enter the output. Backreferences then reuse already
            // converted pixels, avoiding a second full-frame channel pass.
            let rgba = out;
            Ok(DecodedG00 {
                kind,
                width,
                height,
                original_sizes: vec![(width, height)],
                frames: vec![RgbaImage {
                    width,
                    height,
                    center_x: 0,
                    center_y: 0,
                    rgba,
                }],
            })
        }
        G00Type::Type8bit => {
            // C_g00_chip::get_data(type 1): the LZSS output is not a BGRA
            // framebuffer. It is WORD pal_cnt, pal_cnt DWORD BGRA entries,
            // followed by one palette index byte per pixel.
            let (out, out_len) =
                real_live_type1_uncompress(&data[off..]).context("type1 uncompress")?;
            let raw = &out[..out_len.min(out.len())];
            if raw.len() < 2 {
                bail!("type1 palette header truncated");
            }
            let pal_cnt = read_u16le(raw, 0)? as usize;
            let pal_bytes = pal_cnt
                .checked_mul(4)
                .context("type1 palette size overflow")?;
            let indices_off = 2usize
                .checked_add(pal_bytes)
                .context("type1 palette offset overflow")?;
            if indices_off > raw.len() {
                bail!(
                    "type1 palette truncated: count={} bytes={} raw={}",
                    pal_cnt,
                    pal_bytes,
                    raw.len()
                );
            }
            let pixel_count = (width as usize)
                .checked_mul(height as usize)
                .context("type1 pixel count overflow")?;
            let indices_end = indices_off
                .checked_add(pixel_count)
                .context("type1 index range overflow")?;
            if indices_end > raw.len() {
                bail!(
                    "type1 indices truncated: need={} have={}",
                    pixel_count,
                    raw.len().saturating_sub(indices_off)
                );
            }

            let palette = &raw[2..indices_off];
            let indices = &raw[indices_off..indices_end];
            let rgba_len = pixel_count
                .checked_mul(4)
                .context("type1 RGBA size overflow")?;
            let mut rgba = Vec::with_capacity(rgba_len);
            for &index in indices {
                let index = index as usize;
                if index >= pal_cnt {
                    bail!(
                        "type1 palette index out of range: index={} palette_count={}",
                        index,
                        pal_cnt
                    );
                }
                let base = index * 4;
                let b = palette[base];
                let g = palette[base + 1];
                let r = palette[base + 2];
                let a = palette[base + 3];
                rgba.extend_from_slice(&[r, g, b, a]);
            }

            Ok(DecodedG00 {
                kind,
                width,
                height,
                original_sizes: vec![(width, height)],
                frames: vec![RgbaImage {
                    width,
                    height,
                    center_x: 0,
                    center_y: 0,
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

            // C_g00::set_data() sizes m_cut_list from the outer cut count and
            // calls get_cut_data_point() for every slot. The decompressed table
            // can therefore leave later slots empty without changing PATNO numbering.
            let mut frames: Vec<RgbaImage> = Vec::with_capacity(index_entries);
            let mut original_sizes = vec![(0, 0); index_entries];
            for i in 0..index_entries {
                if i >= debuf_entries {
                    frames.push(transparent_missing_g00_cut());
                    continue;
                }
                let p_off = pairs_off + i * pair_size;
                let offset = read_u32le(&debuf, p_off)? as usize;
                let length_raw = read_u32le(&debuf, p_off + 4)? as i32;
                if offset == 0 || length_raw == 0 || offset >= debuf.len() {
                    frames.push(transparent_missing_g00_cut());
                    continue;
                }

                // get_cut_data_point() returns g00_data + offset for every
                // non-zero size (negative means a linked/reused cut). The type-2
                // C_g00_cut::set_data() implementation does not use data_size at
                // all; it advances by the fixed cut/chip headers and chip pixels.
                // Therefore the signed table size must not truncate this parser.
                let part_bytes = &debuf[offset..];
                let img = extract_g02_part(part_bytes)
                    .with_context(|| format!("extract g02 part idx={i}"))?;
                let part = parse_g02_part_info_prefix(part_bytes)?;
                original_sizes[i] = (part.width, part.height);
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
                original_sizes,
            })
        }
        G00Type::TypeJpeg => {
            if off > data.len() {
                bail!("g00 type3 header out of bounds");
            }
            let jpeg = siglus_assets::g00::decode_type3_jpeg_payload(&data[off..]);
            if !jpeg.starts_with(&[0xFF, 0xD8]) {
                bail!(
                    "g00 type3 XOR decode did not produce JPEG SOI: got={:02X?}",
                    &jpeg[..jpeg.len().min(2)]
                );
            }
            let img = image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)
                .or_else(|_| image::load_from_memory(&jpeg))
                .context("decode g00 type3 jpeg after XOR")?;
            let rgba = img.to_rgba8();
            let (w, h) = rgba.dimensions();
            if w != width || h != height {
                bail!("g00 jpeg size mismatch: got={w}x{h}, expect={width}x{height}");
            }
            Ok(DecodedG00 {
                kind,
                width,
                height,
                original_sizes: vec![(width, height)],
                frames: vec![RgbaImage {
                    width,
                    height,
                    center_x: 0,
                    center_y: 0,
                    rgba: rgba.into_raw(),
                }],
            })
        }
    }
}


fn transparent_missing_g00_cut() -> RgbaImage {
    RgbaImage {
        width: 1,
        height: 1,
        center_x: 0,
        center_y: 0,
        rgba: vec![0, 0, 0, 0],
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
    // LzssUnPack() reads [arc_size, org_size] but the original decoder does
    // not use arc_size to terminate decoding; it stops when org_size bytes
    // have been produced. Do the same, while retaining safe input bounds.
    let _arc_size = read_u32le(compr, 0)? as usize;
    let uncomprlen = read_u32le(compr, 4)? as usize;
    if uncomprlen == 0 {
        bail!("type1 org_size=0");
    }

    let mut out = vec![0u8; uncomprlen];
    let mut curbyte = 8usize;
    let mut act = 0usize;
    let mut bit_count = 0u8;
    let mut flag = 0u8;

    while act < uncomprlen {
        if bit_count == 0 {
            if curbyte >= compr.len() {
                bail!(
                    "type1 truncated before output complete: wrote={} expected={}",
                    act,
                    uncomprlen
                );
            }
            flag = compr[curbyte];
            curbyte += 1;
            bit_count = 8;
        }

        if (flag & 1) != 0 {
            if curbyte >= compr.len() {
                bail!("type1 truncated literal");
            }
            out[act] = compr[curbyte];
            act += 1;
            curbyte += 1;
        } else {
            if curbyte + 2 > compr.len() {
                bail!("type1 truncated backreference");
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
            copy_lzss_match(&mut out, &mut act, offset, count);
        }

        flag >>= 1;
        bit_count -= 1;
    }

    Ok((out, uncomprlen))
}

/// Forward overlapping LZSS copy. The original x86 code uses forward
/// `rep movsb`/`rep movsd`, so bytes written by the match may immediately
/// become source bytes for the remainder of the same match.
fn copy_lzss_match(dst: &mut [u8], position: &mut usize, offset: usize, count: usize) {
    debug_assert!(offset > 0 && *position >= offset && *position <= dst.len());
    let source = *position - offset;
    let end = *position + count.min(dst.len() - *position);
    while *position < end {
        let available = *position - source;
        let size = available.min(end - *position);
        dst.copy_within(source..source + size, *position);
        *position += size;
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
                copy_lzss_match(dst, &mut d, offset, count);
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
    // The original expands BGR pixels to four-byte pixels and then reuses
    // complete pixels for backreferences. Rust stores decoded images as RGBA,
    // so perform the BGR->RGB permutation once at literal insertion.
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
                dst[d] = src[s + 2];
                dst[d + 1] = src[s + 1];
                dst[d + 2] = src[s];
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
                copy_lzss_match(dst, &mut d, offset_bytes, count_bytes);
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

#[cfg(test)]
mod lzss_fast_path_tests {
    use super::*;

    #[test]
    fn overlapping_match_matches_original_forward_byte_copy() {
        for offset in 1..=16 {
            for count in 1..=32 {
                let mut expected: Vec<u8> = (0..64).map(|i| (i * 37) as u8).collect();
                let mut actual = expected.clone();
                let mut expected_pos = 32usize;
                for _ in 0..count {
                    if expected_pos >= expected.len() {
                        break;
                    }
                    expected[expected_pos] = expected[expected_pos - offset];
                    expected_pos += 1;
                }
                let mut actual_pos = 32usize;
                copy_lzss_match(&mut actual, &mut actual_pos, offset, count);
                assert_eq!(actual_pos, expected_pos);
                assert_eq!(actual, expected, "offset={offset} count={count}");
            }
        }
    }

    #[test]
    fn type0_literal_is_emitted_as_rgba_before_backreference_reuse() {
        // flag=1: one literal BGR=(10,20,30). The helper representation is
        // tested directly because the file header is unrelated to LZSS.
        let mut dst = [0u8; 4];
        lzss_decompress_24bit(&[1, 10, 20, 30], &mut dst).unwrap();
        assert_eq!(dst, [30, 20, 10, 255]);
    }
}

#[derive(Debug, Clone)]
struct G02PartInfo {
    _part_type: u8,
    block_count: u16,
    hs_orig_x: i32,
    hs_orig_y: i32,
    width: u32,
    height: u32,
    screen_show_x: i32,
    screen_show_y: i32,
    full_part_width: u32,
    full_part_height: u32,
}

#[derive(Debug, Clone)]
struct G02BlockInfo {
    orig_x: u16,
    orig_y: u16,
    _sprite: bool,
    width: u16,
    height: u16,
}

// MSVC default packing used by the original g00 library:
// G00_CUT_HEADER_STRUCT  = BYTE + pad + WORD + 8*i32 + 20*i32 = 0x74
// G00_CHIP_HEADER_STRUCT = 2*WORD + BYTE + pad + 2*WORD + pad2 + 20*i32 = 0x5c
const G02_PART_INFO_SIZE: usize = 0x74;
const G02_BLOCK_INFO_SIZE: usize = 0x5c;

fn parse_g02_block(buf: &[u8]) -> Result<G02BlockInfo> {
    if buf.len() < G02_BLOCK_INFO_SIZE {
        bail!("G00_CHIP_HEADER_STRUCT truncated");
    }
    let orig_x = read_u16le(buf, 0)?;
    let orig_y = read_u16le(buf, 2)?;
    let sprite = buf[4] == 1;
    let width = read_u16le(buf, 6)?;
    let height = read_u16le(buf, 8)?;
    Ok(G02BlockInfo {
        orig_x,
        orig_y,
        _sprite: sprite,
        width,
        height,
    })
}

fn parse_g02_part_info_prefix(buf: &[u8]) -> Result<G02PartInfo> {
    // G00_CUT_HEADER_STRUCT from g00.cpp, using MSVC default packing:
    //   BYTE type; BYTE padding; WORD count;
    //   int x,y,disp_xl,disp_yl,xc,yc,cut_xl,cut_yl;
    //   int keep[20];
    if buf.len() < G02_PART_INFO_SIZE {
        bail!("G00_CUT_HEADER_STRUCT truncated");
    }
    let part_type = buf[0];
    let block_count = read_u16le(buf, 2)?;
    let disp_x = read_i32le(buf, 4)?;
    let disp_y = read_i32le(buf, 8)?;
    let disp_width = read_u32le(buf, 0x0C)?;
    let disp_height = read_u32le(buf, 0x10)?;
    let center_x = read_i32le(buf, 0x14)?;
    let center_y = read_i32le(buf, 0x18)?;
    let cut_width = read_u32le(buf, 0x1C)?;
    let cut_height = read_u32le(buf, 0x20)?;
    Ok(G02PartInfo {
        _part_type: part_type,
        block_count,
        hs_orig_x: disp_x,
        hs_orig_y: disp_y,
        width: cut_width,
        height: cut_height,
        screen_show_x: center_x,
        screen_show_y: center_y,
        full_part_width: disp_width,
        full_part_height: disp_height,
    })
}

fn extract_g02_part(part_bytes: &[u8]) -> Result<RgbaImage> {
    // Original C_d3d_texture::load_g00_cut() creates the D3D texture from
    // cut_info.disp_rect, not from the whole cut_xl/cut_yl canvas. Each chip is
    // copied to chip.x - disp_rect.left, chip.y - disp_rect.top, and texture
    // center is cut_info.center - disp_rect.left/top.
    let part = parse_g02_part_info_prefix(part_bytes).context("parse part prefix")?;
    if part.width == 0 || part.height == 0 {
        bail!("g02 part has zero full-cut dimensions");
    }
    if part.full_part_width == 0 || part.full_part_height == 0 {
        bail!("g02 part has zero display dimensions");
    }

    // The original does not probe alternative layouts. It advances by
    // sizeof(G00_CUT_HEADER_STRUCT), which is 0x74 with the engine's MSVC ABI.
    let header_size = G02_PART_INFO_SIZE;

    let out_w = part.full_part_width;
    let out_h = part.full_part_height;
    let stride = (out_w as usize)
        .checked_mul(4)
        .context("stride overflow")?;
    let mut dib = vec![0u8; stride * (out_h as usize)];

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

        let dst_x_i = block.orig_x as i32 - part.hs_orig_x;
        let dst_y_i = block.orig_y as i32 - part.hs_orig_y;
        if dst_x_i < 0 || dst_y_i < 0 {
            bail!(
                "g02 block outside display rect: chip=({}, {}) disp=({}, {})",
                block.orig_x,
                block.orig_y,
                part.hs_orig_x,
                part.hs_orig_y
            );
        }
        let dst_x = dst_x_i as usize;
        let dst_y = dst_y_i as usize;
        if dst_x.saturating_add(bw) > out_w as usize || dst_y.saturating_add(bh) > out_h as usize {
            bail!(
                "g02 block write outside display rect: dst=({}, {}) size={}x{} out={}x{}",
                dst_x,
                dst_y,
                bw,
                bh,
                out_w,
                out_h
            );
        }

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

    let rgba = bgra_to_rgba_inplace(dib);

    Ok(RgbaImage {
        width: out_w,
        height: out_h,
        center_x: part.screen_show_x - part.hs_orig_x,
        center_y: part.screen_show_y - part.hs_orig_y,
        rgba,
    })
}



#[cfg(test)]
mod original_g00_layout_tests {
    use super::*;

    fn literal_type1_g00(width: u16, height: u16, raw: &[u8]) -> Vec<u8> {
        let mut payload = Vec::new();
        for chunk in raw.chunks(8) {
            payload.push(if chunk.len() == 8 {
                0xff
            } else {
                ((1u16 << chunk.len()) - 1) as u8
            });
            payload.extend_from_slice(chunk);
        }
        let mut file = Vec::new();
        file.push(1); // type 1
        file.extend_from_slice(&width.to_le_bytes());
        file.extend_from_slice(&height.to_le_bytes());
        file.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        file.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        file.extend_from_slice(&payload);
        file
    }

    #[test]
    fn type1_expands_palette_indices_like_c_g00_chip_get_data() {
        // Palette entries are DWORD BGRA in the original. Pixels are byte indices.
        let mut raw = Vec::new();
        raw.extend_from_slice(&2u16.to_le_bytes());
        raw.extend_from_slice(&[10, 20, 30, 40]); // BGRA -> RGBA 30,20,10,40
        raw.extend_from_slice(&[50, 60, 70, 80]); // BGRA -> RGBA 70,60,50,80
        raw.extend_from_slice(&[1, 0]);

        let decoded = decode_g00(&literal_type1_g00(2, 1, &raw)).unwrap();
        assert_eq!(decoded.frames.len(), 1);
        assert_eq!(decoded.frames[0].rgba, vec![70, 60, 50, 80, 30, 20, 10, 40]);
    }

    #[test]
    fn type2_cut_uses_fixed_msvc_headers_and_display_rect_coordinates() {
        let mut cut = vec![0u8; G02_PART_INFO_SIZE];
        cut[0] = 2;
        cut[2..4].copy_from_slice(&1u16.to_le_bytes()); // one chip
        cut[4..8].copy_from_slice(&10i32.to_le_bytes()); // disp left
        cut[8..12].copy_from_slice(&20i32.to_le_bytes()); // disp top
        cut[12..16].copy_from_slice(&2i32.to_le_bytes()); // disp width
        cut[16..20].copy_from_slice(&1i32.to_le_bytes()); // disp height
        cut[20..24].copy_from_slice(&11i32.to_le_bytes()); // center x
        cut[24..28].copy_from_slice(&20i32.to_le_bytes()); // center y
        cut[28..32].copy_from_slice(&32i32.to_le_bytes()); // full cut width
        cut[32..36].copy_from_slice(&32i32.to_le_bytes()); // full cut height

        let mut chip = vec![0u8; G02_BLOCK_INFO_SIZE];
        chip[0..2].copy_from_slice(&10u16.to_le_bytes());
        chip[2..4].copy_from_slice(&20u16.to_le_bytes());
        chip[4] = 1; // sprite flag; pixels themselves are still copied verbatim
        chip[6..8].copy_from_slice(&2u16.to_le_bytes());
        chip[8..10].copy_from_slice(&1u16.to_le_bytes());
        cut.extend_from_slice(&chip);
        cut.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]); // two BGRA pixels

        let image = extract_g02_part(&cut).unwrap();
        assert_eq!((image.width, image.height), (2, 1));
        assert_eq!((image.center_x, image.center_y), (1, 0));
        assert_eq!(image.rgba, vec![3, 2, 1, 4, 7, 6, 5, 8]);
    }
}
