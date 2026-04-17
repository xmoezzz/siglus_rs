#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OggPacket {
    pub packet: Vec<u8>,
    pub b_o_s: bool,
    pub e_o_s: bool,
    pub granulepos: i64,
    pub packetno: i64,
}

impl OggPacket {
    pub fn new(packet: impl Into<Vec<u8>>) -> Self {
        Self {
            packet: packet.into(),
            b_o_s: false,
            e_o_s: false,
            granulepos: 0,
            packetno: 0,
        }
    }

    pub fn with_bos(packet: impl Into<Vec<u8>>) -> Self {
        Self {
            b_o_s: true,
            ..Self::new(packet)
        }
    }

    pub fn bytes(&self) -> isize {
        self.packet.len() as isize
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.packet
    }
}

#[derive(Debug, Clone, Default)]
pub struct PackWriter {
    buf: Vec<u8>,
    cur: u8,
    nbits: u8,
}

impl PackWriter {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn write(&mut self, value: u32, nbits: usize) {
        if nbits == 0 {
            return;
        }
        debug_assert!(nbits <= 32);
        for i in (0..nbits).rev() {
            let bit = ((value >> i) & 1) as u8;
            self.cur = (self.cur << 1) | bit;
            self.nbits += 1;
            if self.nbits == 8 {
                self.buf.push(self.cur);
                self.cur = 0;
                self.nbits = 0;
            }
        }
    }

    pub fn write_i32_bits(&mut self, value: i32, nbits: usize) {
        self.write(
            (value as u32) & ((1u64 << nbits) as u32).wrapping_sub(1),
            nbits,
        );
    }

    pub fn write_octets(&mut self, buf: &[u8]) {
        for &b in buf {
            self.write(b as u32, 8);
        }
    }

    pub fn write_le_u32(&mut self, value: u32) {
        self.write((value & 0xFF) as u32, 8);
        self.write(((value >> 8) & 0xFF) as u32, 8);
        self.write(((value >> 16) & 0xFF) as u32, 8);
        self.write(((value >> 24) & 0xFF) as u32, 8);
    }

    pub fn bytes(&self) -> usize {
        self.buf.len() + usize::from(self.nbits > 0)
    }

    pub fn finish(mut self) -> Vec<u8> {
        if self.nbits > 0 {
            self.cur <<= 8 - self.nbits;
            self.buf.push(self.cur);
            self.cur = 0;
            self.nbits = 0;
        }
        self.buf
    }
}

#[cfg(test)]
mod tests {
    use super::{OggPacket, PackWriter};

    #[test]
    fn packet_defaults_match_expectation() {
        let p = OggPacket::new([1u8, 2, 3]);
        assert_eq!(p.bytes(), 3);
        assert!(!p.b_o_s);
        assert_eq!(p.as_slice(), &[1, 2, 3]);
    }

    #[test]
    fn pack_writer_writes_msb_first() {
        let mut w = PackWriter::new();
        w.write(0b101, 3);
        w.write(0b00111, 5);
        assert_eq!(w.finish(), vec![0b1010_0111]);
    }
}
