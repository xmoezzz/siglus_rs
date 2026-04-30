use crate::error::{Error, Result};
use std::fmt::{Display, Formatter};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Guid {
    canonical_upper: String,
}

impl Guid {
    pub fn parse(input: &str) -> Result<Self> {
        let trimmed = input.trim();
        let inner = trimmed
            .strip_prefix('<')
            .and_then(|s| s.strip_suffix('>'))
            .unwrap_or(trimmed);
        let upper = inner.to_ascii_uppercase();
        let parts: Vec<&str> = upper.split('-').collect();
        if parts.len() != 5 {
            return Err(Error::Parse(format!("invalid guid: {input}")));
        }
        let lens = [8, 4, 4, 4, 12];
        for (part, len) in parts.iter().zip(lens) {
            if part.len() != len || !part.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err(Error::Parse(format!("invalid guid: {input}")));
            }
        }
        Ok(Self {
            canonical_upper: upper,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.canonical_upper
    }
}

impl Display for Guid {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.canonical_upper)
    }
}
