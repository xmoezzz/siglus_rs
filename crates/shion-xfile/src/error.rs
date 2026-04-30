use std::fmt::{Display, Formatter};

#[derive(Debug, Clone)]
pub enum Error {
    UnexpectedEof,
    InvalidHeader(String),
    Lex(String),
    Parse(String),
    Semantic(String),
    Unsupported(String),
    Io(String),
}

impl Display for Error {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected end of file"),
            Self::InvalidHeader(s) => write!(f, "invalid header: {s}"),
            Self::Lex(s) => write!(f, "lexer error: {s}"),
            Self::Parse(s) => write!(f, "parser error: {s}"),
            Self::Semantic(s) => write!(f, "semantic error: {s}"),
            Self::Unsupported(s) => write!(f, "unsupported: {s}"),
            Self::Io(s) => write!(f, "io error: {s}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
