pub const PB_WINDOW_SIZE: i32 = 64;
pub const LOTS_OF_BITS: i32 = 0x4000_0000;

#[derive(Debug, Clone)]
pub struct PackBuf<'a> {
    pub(crate) data: &'a [u8],
    pub(crate) ptr: usize,
    pub(crate) window: u64,
    pub(crate) bits: i32,
    pub(crate) eof: bool,
}

impl<'a> PackBuf<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self {
            data: buf,
            ptr: 0,
            window: 0,
            bits: 0,
            eof: false,
        }
    }

    fn refill(&mut self, requested_bits: i32) -> u64 {
        let mut ptr = self.ptr;
        let stop = self.data.len();
        let mut window = self.window;
        let mut available = self.bits;
        let mut shift = PB_WINDOW_SIZE - available;
        while shift > 7 && ptr < stop {
            shift -= 8;
            window |= (self.data[ptr] as u64) << (shift as u32);
            ptr += 1;
        }
        self.ptr = ptr;
        available = PB_WINDOW_SIZE - shift;
        if requested_bits > available {
            if ptr >= stop {
                self.eof = true;
                available = LOTS_OF_BITS;
            } else {
                window |= (self.data[ptr] as u64) >> ((available & 7) as u32);
            }
        }
        self.bits = available;
        window
    }

    pub fn look1(&mut self) -> u32 {
        let mut window = self.window;
        let available = self.bits;
        if available < 1 {
            window = self.refill(1);
            self.window = window;
        }
        (window >> ((PB_WINDOW_SIZE - 1) as u32)) as u32
    }

    pub fn adv1(&mut self) {
        self.window <<= 1;
        self.bits -= 1;
    }

    pub fn read(&mut self, bits: i32) -> u32 {
        debug_assert!((0..=32).contains(&bits));
        if bits == 0 {
            return 0;
        }
        let mut window = self.window;
        let mut available = self.bits;
        if available < bits {
            window = self.refill(bits);
            available = self.bits;
        }
        let result = (window >> ((PB_WINDOW_SIZE - bits) as u32)) as u32;
        available -= bits;
        window <<= bits as u32;
        self.window = window;
        self.bits = available;
        result
    }

    pub fn read1(&mut self) -> u32 {
        let mut window = self.window;
        let mut available = self.bits;
        if available < 1 {
            window = self.refill(1);
            available = self.bits;
        }
        let result = (window >> ((PB_WINDOW_SIZE - 1) as u32)) as u32;
        available -= 1;
        window <<= 1;
        self.window = window;
        self.bits = available;
        result
    }

    pub fn bytes_left(&self) -> isize {
        if self.eof {
            -1
        } else {
            (self.data.len() - self.ptr) as isize + ((self.bits >> 3) as isize)
        }
    }

    pub fn is_eof(&self) -> bool {
        self.eof
    }
}

#[cfg(test)]
mod tests {
    use super::PackBuf;

    #[test]
    fn reads_msb_first() {
        let data = [0b1011_0010u8, 0b0101_0101u8];
        let mut pb = PackBuf::new(&data);
        assert_eq!(pb.read1(), 1);
        assert_eq!(pb.read(3), 0b011);
        assert_eq!(pb.read(4), 0b0010);
        assert_eq!(pb.read(4), 0b0101);
    }
}
