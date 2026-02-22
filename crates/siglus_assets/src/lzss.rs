use anyhow::{anyhow, bail, Result};

/// Parse the Siglus LZSS header.
///
/// Format: `[u32 arc_size][u32 org_size][payload...]`.
#[inline]
fn parse_header(src: &[u8]) -> Result<(usize, usize)> {
    if src.len() < 8 {
        bail!("lzss: input too short");
    }
    let arc_size = u32::from_le_bytes([src[0], src[1], src[2], src[3]]) as usize;
    let org_size = u32::from_le_bytes([src[4], src[5], src[6], src[7]]) as usize;
    Ok((arc_size, org_size))
}

/// Return the decompressed size (org_size) from the header.
pub fn lzss_decompressed_size(src: &[u8]) -> Result<usize> {
    let (_, org_size) = parse_header(src)?;
    Ok(org_size)
}

/// Decompress Siglus LZSS (byte-oriented) into a fresh buffer.
///
pub fn lzss_unpack(src: &[u8]) -> Result<Vec<u8>> {
    let (arc_size, org_size) = parse_header(src)?;
    if org_size == 0 {
        bail!("lzss: org_size=0");
    }

    let payload_start = 8usize;
    let payload_end = payload_start
        .checked_add(arc_size)
        .ok_or_else(|| anyhow!("lzss: arc_size overflow"))?;
    if payload_end > src.len() {
        bail!("lzss: arc_size out of bounds (end={}, len={})", payload_end, src.len());
    }

    let mut pos = payload_start;
    let mut out: Vec<u8> = Vec::with_capacity(org_size);

    while out.len() < org_size {
        if pos >= payload_end {
            break;
        }
        let mut flags = src[pos];
        pos += 1;

        for _ in 0..8 {
            if out.len() >= org_size {
                break;
            }
            if pos >= payload_end {
                break;
            }

            if (flags & 1) != 0 {
                // literal
                out.push(src[pos]);
                pos += 1;
            } else {
                if pos + 2 > payload_end {
                    bail!("lzss: truncated backref token");
                }
                let token = u16::from_le_bytes([src[pos], src[pos + 1]]);
                pos += 2;

                let offset = (token >> 4) as usize;
                // LZSS_BREAK_EVEN is 1 in Siglus's implementation.
                let len = ((token & 0x0F) as usize) + 2;

                if offset == 0 {
                    bail!("lzss: invalid backref offset=0");
                }
                if offset > out.len() {
                    bail!(
                        "lzss: backref offset out of range (offset={}, out_len={})",
                        offset,
                        out.len()
                    );
                }

                let mut src_idx = out.len() - offset;
                for _ in 0..len {
                    if out.len() >= org_size {
                        break;
                    }
                    let b = out[src_idx];
                    out.push(b);
                    src_idx += 1;
                }
            }

            flags >>= 1;
        }
    }

    if out.len() != org_size {
        bail!("lzss: size mismatch (got={}, expected={})", out.len(), org_size);
    }

    Ok(out)
}

/// Decompress Siglus LZSS but be tolerant about the declared `arc_size`.
///
/// The original engine does not always hard-fail on mismatched `arc_size`.
/// Instead, it will typically decode until either:
/// - the output reaches `org_size`, or
/// - the input stream ends.
///
/// We still require the final output length to match `org_size`.
pub fn lzss_unpack_lenient(src: &[u8]) -> Result<Vec<u8>> {
    let (arc_size, org_size) = parse_header(src)?;
    if org_size == 0 {
        bail!("lzss(lenient): org_size=0");
    }

    let payload_start = 8usize;
    let payload_end = payload_start
        .checked_add(arc_size)
        .ok_or_else(|| anyhow!("lzss(lenient): arc_size overflow"))?
        .min(src.len());

    let mut pos = payload_start;
    let mut out: Vec<u8> = Vec::with_capacity(org_size);

    while out.len() < org_size {
        if pos >= payload_end {
            break;
        }
        let mut flags = src[pos];
        pos += 1;

        for _ in 0..8 {
            if out.len() >= org_size {
                break;
            }
            if pos >= payload_end {
                break;
            }

            if (flags & 1) != 0 {
                out.push(src[pos]);
                pos += 1;
            } else {
                if pos + 2 > payload_end {
                    break;
                }
                let token = u16::from_le_bytes([src[pos], src[pos + 1]]);
                pos += 2;

                let offset = (token >> 4) as usize;
                let len = ((token & 0x0F) as usize) + 2;

                if offset == 0 || offset > out.len() {
                    // In lenient mode, treat broken backrefs as end-of-stream.
                    break;
                }

                let mut src_idx = out.len() - offset;
                for _ in 0..len {
                    if out.len() >= org_size {
                        break;
                    }
                    let b = out[src_idx];
                    out.push(b);
                    src_idx += 1;
                }
            }

            flags >>= 1;
        }
    }

    if out.len() != org_size {
        bail!(
            "lzss(lenient): size mismatch (got={}, expected={})",
            out.len(),
            org_size
        );
    }

    Ok(out)
}

/// Decompress Siglus LZSS32 (3-byte literal + implicit alpha, dword backrefs).
///
/// The output is 32bpp BGRA (little-endian dwords).
pub fn lzss_unpack32(src: &[u8]) -> Result<Vec<u8>> {
    let (arc_size, org_size) = parse_header(src)?;
    if org_size == 0 {
        bail!("lzss32: org_size=0");
    }
    if org_size % 4 != 0 {
        bail!("lzss32: org_size not multiple of 4 (org_size={})", org_size);
    }

    let payload_start = 8usize;
    let payload_end = payload_start
        .checked_add(arc_size)
        .ok_or_else(|| anyhow!("lzss32: arc_size overflow"))?;
    if payload_end > src.len() {
        bail!("lzss32: arc_size out of bounds (end={}, len={})", payload_end, src.len());
    }

    let mut pos = payload_start;
    let mut out: Vec<u8> = Vec::with_capacity(org_size);

    while out.len() < org_size {
        if pos >= payload_end {
            break;
        }
        let mut flags = src[pos];
        pos += 1;

        for _ in 0..8 {
            if out.len() >= org_size {
                break;
            }

            if (flags & 1) != 0 {
                // literal: copy 3 bytes, then write alpha=255
                if pos + 3 > payload_end {
                    bail!("lzss32: truncated literal");
                }
                out.push(src[pos]);
                out.push(src[pos + 1]);
                out.push(src[pos + 2]);
                out.push(255);
                pos += 3;
            } else {
                // backref token is 16-bit: high 12 bits = offset (in dwords), low 4 bits = (len-1)
                if pos + 2 > payload_end {
                    bail!("lzss32: truncated backref token");
                }
                let token = u16::from_le_bytes([src[pos], src[pos + 1]]);
                pos += 2;

                let offset_dwords = (token >> 4) as usize;
                let len_dwords = ((token & 0x0F) as usize) + 1;

                if offset_dwords == 0 {
                    bail!("lzss32: invalid backref offset=0");
                }

                let offset_bytes = offset_dwords
                    .checked_mul(4)
                    .ok_or_else(|| anyhow!("lzss32: offset overflow"))?;

                if offset_bytes > out.len() {
                    bail!(
                        "lzss32: backref offset out of range (offset_bytes={}, out_len={})",
                        offset_bytes,
                        out.len()
                    );
                }

                let mut src_idx = out.len() - offset_bytes;
                for _ in 0..len_dwords {
                    if out.len() + 4 > org_size {
                        break;
                    }
                    // copy one dword; allow overlap like memmove
                    let d0 = out[src_idx];
                    let d1 = out[src_idx + 1];
                    let d2 = out[src_idx + 2];
                    let d3 = out[src_idx + 3];
                    out.push(d0);
                    out.push(d1);
                    out.push(d2);
                    out.push(d3);
                    src_idx += 4;
                }
            }

            flags >>= 1;
            if pos > payload_end {
                break;
            }
        }
    }

    if out.len() != org_size {
        bail!("lzss32: size mismatch (got={}, expected={})", out.len(), org_size);
    }

    Ok(out)
}
