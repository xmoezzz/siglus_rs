use crate::error::{Error, Result};
use flate2::{Decompress, FlushDecompress, Status};

pub const MSZIP_MAGIC: u16 = 0x4B43;
pub const MSZIP_BLOCK_MAX: usize = 32786;
pub const MSZIP_MASTER_HEADER_LEN: usize = 6;

pub fn decompress_mszip_payload(body: &[u8]) -> Result<Vec<u8>> {
    if body.len() < MSZIP_MASTER_HEADER_LEN {
        return Err(Error::UnexpectedEof);
    }

    let mut cursor = MSZIP_MASTER_HEADER_LEN;
    let mut block_index = 0usize;
    let mut out = Vec::new();
    let mut decompressor = Decompress::new(false);

    while cursor < body.len() {
        if cursor + 4 > body.len() {
            if body[cursor..].iter().all(|byte| *byte == 0) {
                break;
            }
            return Err(Error::Parse(format!(
                "truncated MSZIP block header at byte {}",
                cursor
            )));
        }

        let ofs = u16::from_le_bytes([body[cursor], body[cursor + 1]]) as usize;
        cursor += 2;
        let magic = u16::from_le_bytes([body[cursor], body[cursor + 1]]);
        cursor += 2;

        if magic != MSZIP_MAGIC {
            return Err(Error::Parse(format!(
                "MSZIP block {} has invalid magic 0x{:04X}",
                block_index, magic
            )));
        }
        if ofs == 0 {
            return Err(Error::Parse(format!(
                "MSZIP block {} has zero compressed size",
                block_index
            )));
        }
        if ofs >= MSZIP_BLOCK_MAX {
            return Err(Error::Parse(format!(
                "MSZIP block {} compressed size {} exceeds supported maximum {}",
                block_index, ofs, MSZIP_BLOCK_MAX - 1
            )));
        }
        if cursor + ofs > body.len() {
            return Err(Error::Parse(format!(
                "MSZIP block {} overruns payload: need {} bytes, have {}",
                block_index,
                ofs,
                body.len().saturating_sub(cursor)
            )));
        }

        let compressed = &body[cursor..cursor + ofs];
        cursor += ofs;
        let out_before = out.len();
        out.reserve(MSZIP_BLOCK_MAX);
        let status = decompressor
            .decompress_vec(compressed, &mut out, FlushDecompress::Sync)
            .map_err(|err| Error::Parse(format!(
                "MSZIP inflate failed in block {}: {}",
                block_index, err
            )))?;
        let produced = out.len().saturating_sub(out_before);

        if produced == 0 {
            return Err(Error::Parse(format!(
                "MSZIP block {} produced no output",
                block_index
            )));
        }
        if produced > MSZIP_BLOCK_MAX {
            return Err(Error::Parse(format!(
                "MSZIP block {} produced {} bytes, exceeding supported maximum {}",
                block_index, produced, MSZIP_BLOCK_MAX
            )));
        }

        match status {
            Status::Ok | Status::BufError | Status::StreamEnd => {}
        }

        block_index += 1;
    }

    Ok(out)
}
