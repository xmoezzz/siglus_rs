use core::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TheoraError {
    Fault,
    InvalidArgument,
    BadHeader,
    NotFormat,
    Version,
    NotImplemented,
    BadPacket,
}

pub type Result<T> = core::result::Result<T, TheoraError>;

impl TheoraError {
    pub const fn code(self) -> i32 {
        match self {
            Self::Fault => -1,
            Self::InvalidArgument => -10,
            Self::BadHeader => -20,
            Self::NotFormat => -21,
            Self::Version => -22,
            Self::NotImplemented => -23,
            Self::BadPacket => -24,
        }
    }
}

impl fmt::Display for TheoraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Fault => "fault",
            Self::InvalidArgument => "invalid argument",
            Self::BadHeader => "bad header",
            Self::NotFormat => "not a Theora stream",
            Self::Version => "unsupported bitstream version",
            Self::NotImplemented => "not implemented",
            Self::BadPacket => "bad packet",
        };
        f.write_str(s)
    }
}

impl std::error::Error for TheoraError {}
