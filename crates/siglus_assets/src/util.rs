use anyhow::{bail, Result};

pub fn read_u16_le(buf: &[u8], off: &mut usize) -> Result<u16> {
    if *off + 2 > buf.len() {
        bail!("unexpected EOF while reading u16");
    }
    let v = u16::from_le_bytes([buf[*off], buf[*off + 1]]);
    *off += 2;
    Ok(v)
}

pub fn read_i32_le(buf: &[u8], off: &mut usize) -> Result<i32> {
    if *off + 4 > buf.len() {
        bail!("unexpected EOF while reading i32");
    }
    let v = i32::from_le_bytes([
        buf[*off],
        buf[*off + 1],
        buf[*off + 2],
        buf[*off + 3],
    ]);
    *off += 4;
    Ok(v)
}

pub fn read_u32_le(buf: &[u8], off: &mut usize) -> Result<u32> {
    if *off + 4 > buf.len() {
        bail!("unexpected EOF while reading u32");
    }
    let v = u32::from_le_bytes([
        buf[*off],
        buf[*off + 1],
        buf[*off + 2],
        buf[*off + 3],
    ]);
    *off += 4;
    Ok(v)
}

pub fn take_bytes<'a>(buf: &'a [u8], off: &mut usize, n: usize) -> Result<&'a [u8]> {
    if *off + n > buf.len() {
        bail!("unexpected EOF while taking {n} bytes");
    }
    let s = &buf[*off..*off + n];
    *off += n;
    Ok(s)
}

pub fn expect_eq_u32(got: u32, expect: u32, what: &str) -> Result<()> {
    if got != expect {
        bail!("{what}: expected {expect}, got {got}");
    }
    Ok(())
}
