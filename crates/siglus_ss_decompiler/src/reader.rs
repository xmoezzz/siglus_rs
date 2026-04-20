use crate::error::{Error, Result};

#[derive(Clone, Copy)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn with_pos(data: &'a [u8], pos: usize) -> Result<Self> {
        if pos > data.len() {
            return Err(Error::with_offset("reader start is outside input", pos));
        }
        Ok(Self { data, pos })
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn read_u8(&mut self) -> Result<u8> {
        if self.pos >= self.data.len() {
            return Err(Error::with_offset("unexpected end while reading u8", self.pos));
        }
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    pub fn read_i32(&mut self) -> Result<i32> {
        let off = self.pos;
        let b = self.read_bytes(4)?;
        Ok(i32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn read_u32(&mut self) -> Result<u32> {
        let off = self.pos;
        let b = self.read_bytes(4).map_err(|_| Error::with_offset("unexpected end while reading u32", off))?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn read_u16(&mut self) -> Result<u16> {
        let off = self.pos;
        let b = self.read_bytes(2).map_err(|_| Error::with_offset("unexpected end while reading u16", off))?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    pub fn read_bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or_else(|| Error::with_offset("byte range overflow", self.pos))?;
        if end > self.data.len() {
            return Err(Error::with_offset("unexpected end while reading bytes", self.pos));
        }
        let out = &self.data[self.pos..end];
        self.pos = end;
        Ok(out)
    }
}

pub fn checked_range(data_len: usize, ofs: i32, size: i32, what: &str) -> Result<std::ops::Range<usize>> {
    if ofs < 0 || size < 0 {
        return Err(Error::new(format!("{what} has negative offset or size")));
    }
    let start = ofs as usize;
    let len = size as usize;
    let end = start.checked_add(len).ok_or_else(|| Error::new(format!("{what} range overflow")))?;
    if end > data_len {
        return Err(Error::new(format!("{what} range 0x{start:X}..0x{end:X} is outside input size 0x{data_len:X}")));
    }
    Ok(start..end)
}

pub fn read_i32_at(data: &[u8], pos: usize) -> Result<i32> {
    let mut r = Reader::with_pos(data, pos)?;
    r.read_i32()
}

pub fn read_index_array(data: &[u8], ofs: i32, cnt: i32, what: &str) -> Result<Vec<Index>> {
    if cnt < 0 {
        return Err(Error::new(format!("{what} has negative count")));
    }
    let bytes = cnt.checked_mul(8).ok_or_else(|| Error::new(format!("{what} byte size overflow")))?;
    let range = checked_range(data.len(), ofs, bytes, what)?;
    let mut r = Reader::with_pos(data, range.start)?;
    let mut out = Vec::with_capacity(cnt as usize);
    for _ in 0..cnt {
        out.push(Index { offset: r.read_i32()?, size: r.read_i32()? });
    }
    Ok(out)
}

pub fn read_i32_array(data: &[u8], ofs: i32, cnt: i32, what: &str) -> Result<Vec<i32>> {
    if cnt < 0 {
        return Err(Error::new(format!("{what} has negative count")));
    }
    let bytes = cnt.checked_mul(4).ok_or_else(|| Error::new(format!("{what} byte size overflow")))?;
    let range = checked_range(data.len(), ofs, bytes, what)?;
    let mut r = Reader::with_pos(data, range.start)?;
    let mut out = Vec::with_capacity(cnt as usize);
    for _ in 0..cnt {
        out.push(r.read_i32()?);
    }
    Ok(out)
}

#[derive(Debug, Clone, Copy)]
pub struct Index {
    pub offset: i32,
    pub size: i32,
}
