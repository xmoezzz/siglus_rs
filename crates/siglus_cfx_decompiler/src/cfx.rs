use crate::ctab::{parse_ctab, ConstantTable};
use crate::disasm::{disassemble, ShaderKind};

#[derive(Debug, Clone)]
pub struct ShaderBlob {
    pub index: usize,
    pub kind: ShaderKind,
    pub major: u8,
    pub minor: u8,
    pub offset: usize,
    pub end_offset: usize,
    pub bytes: Vec<u8>,
    pub ctab: Option<ConstantTable>,
}

impl ShaderBlob {
    pub fn profile(&self) -> String {
        match self.kind {
            ShaderKind::Pixel => format!("ps_{}_{}", self.major, self.minor),
            ShaderKind::Vertex => format!("vs_{}_{}", self.major, self.minor),
        }
    }

    pub fn file_prefix(&self) -> String {
        let k = match self.kind {
            ShaderKind::Pixel => "ps",
            ShaderKind::Vertex => "vs",
        };
        format!("{}_{:04}_{:08x}", k, self.index, self.offset)
    }
}

fn read_u32_le(data: &[u8], off: usize) -> Option<u32> {
    let b = data.get(off..off + 4)?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn decode_shader_version(tok: u32) -> Option<(ShaderKind, u8, u8)> {
    let major = ((tok >> 8) & 0xff) as u8;
    let minor = (tok & 0xff) as u8;

    match tok & 0xffff_0000 {
        0xffff_0000 if major == 2 => Some((ShaderKind::Pixel, major, minor)),
        0xfffe_0000 if major == 2 => Some((ShaderKind::Vertex, major, minor)),
        _ => None,
    }
}

fn has_leading_ctab(data: &[u8], off: usize) -> bool {
    let Some(comment_token) = read_u32_le(data, off + 4) else {
        return false;
    };

    if (comment_token & 0xffff) != 0xfffe {
        return false;
    }

    let comment_dwords = ((comment_token >> 16) & 0x7fff) as usize;
    if comment_dwords < 1 {
        return false;
    }

    data.get(off + 8..off + 12) == Some(b"CTAB")
}

fn is_real_shader_start(data: &[u8], off: usize) -> Option<(ShaderKind, u8, u8)> {
    let version = read_u32_le(data, off)?;
    let decoded = decode_shader_version(version)?;

    if !has_leading_ctab(data, off) {
        return None;
    }

    Some(decoded)
}

fn shader_end_by_end_token(data: &[u8], off: usize, hard_end: usize) -> usize {
    let mut p = off + 4;
    while p + 4 <= hard_end {
        let Some(tok) = read_u32_le(data, p) else { break; };
        let opcode = tok & 0xffff;

        if opcode == 0xfffe {
            let len = ((tok >> 16) & 0x7fff) as usize;
            p = p.saturating_add(4).saturating_add(len.saturating_mul(4));
            continue;
        }

        if opcode == 0xffff {
            return (p + 4).min(hard_end);
        }

        let len = ((tok >> 24) & 0x0f) as usize;
        if len == 0 {
            p += 4;
        } else {
            p = p.saturating_add(4).saturating_add(len.saturating_mul(4));
        }
    }
    hard_end
}

fn parse_leading_ctab(data: &[u8], off: usize, end: usize) -> Option<ConstantTable> {
    let comment_token = read_u32_le(data, off + 4)?;
    if (comment_token & 0xffff) != 0xfffe {
        return None;
    }
    let dwords = ((comment_token >> 16) & 0x7fff) as usize;
    let payload_start = off + 8;
    let payload_end = payload_start.checked_add(dwords.checked_mul(4)?)?;
    if payload_end > end || payload_end > data.len() {
        return None;
    }
    parse_ctab(&data[payload_start..payload_end]).ok()
}

pub fn scan_shaders(data: &[u8]) -> Vec<ShaderBlob> {
    let mut starts: Vec<(usize, ShaderKind, u8, u8)> = Vec::new();

    let mut i = 0usize;
    while i + 12 <= data.len() {
        if let Some((kind, major, minor)) = is_real_shader_start(data, i) {
            starts.push((i, kind, major, minor));
            i += 4;
        } else {
            i += 1;
        }
    }

    let mut out = Vec::with_capacity(starts.len());
    for idx in 0..starts.len() {
        let (off, kind, major, minor) = starts[idx];
        let hard_end = if idx + 1 < starts.len() { starts[idx + 1].0 } else { data.len() };
        let end = shader_end_by_end_token(data, off, hard_end);
        let ctab = parse_leading_ctab(data, off, end);

        out.push(ShaderBlob {
            index: idx,
            kind,
            major,
            minor,
            offset: off,
            end_offset: end,
            bytes: data[off..end].to_vec(),
            ctab,
        });
    }

    out
}

pub fn disassemble_blob(blob: &ShaderBlob) -> String {
    disassemble(&blob.bytes)
}
