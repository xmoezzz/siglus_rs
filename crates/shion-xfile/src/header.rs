use crate::error::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FloatSize {
    F32,
    F64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormatKind {
    Text,
    Binary,
    TextMsZip,
    BinaryMsZip,
    Unknown([u8; 4]),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XFileHeader {
    pub major: u8,
    pub minor: u8,
    pub format: FormatKind,
    pub float_size: FloatSize,
}

impl XFileHeader {
    pub fn parse(bytes: &[u8]) -> Result<(Self, usize)> {
        let mut base = 0usize;
        if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
            // Legacy exporters sometimes prepend a UTF-8 BOM before the xof header.
            base = 3;
        }
        if bytes.len().saturating_sub(base) < 16 {
            return Err(Error::UnexpectedEof);
        }
        let magic = &bytes[base..base + 4];
        if !magic.eq_ignore_ascii_case(b"xof ") {
            return Err(Error::InvalidHeader(format!(
                "expected magic 'xof ', got {:?}",
                magic
            )));
        }

        let version = std::str::from_utf8(&bytes[base + 4..base + 8])
            .map_err(|_| Error::InvalidHeader("version is not ASCII".to_string()))?;
        if version.len() != 4 || !version.chars().all(|c| c.is_ascii_digit()) {
            return Err(Error::InvalidHeader(format!("invalid version field: {version}")));
        }
        let major = version[0..2]
            .parse::<u8>()
            .map_err(|_| Error::InvalidHeader(format!("invalid major version: {version}")))?;
        let minor = version[2..4]
            .parse::<u8>()
            .map_err(|_| Error::InvalidHeader(format!("invalid minor version: {version}")))?;

        let format_raw = [bytes[base + 8], bytes[base + 9], bytes[base + 10], bytes[base + 11]];
        let mut format_norm = format_raw;
        format_norm.make_ascii_lowercase();
        let format = match &format_norm {
            b"txt " => FormatKind::Text,
            b"bin " => FormatKind::Binary,
            b"tzip" => FormatKind::TextMsZip,
            b"bzip" => FormatKind::BinaryMsZip,
            _ => FormatKind::Unknown(format_raw),
        };

        let float_raw = std::str::from_utf8(&bytes[base + 12..base + 16])
            .map_err(|_| Error::InvalidHeader("float-size field is not ASCII".to_string()))?;
        let float_size = match float_raw {
            "0032" => FloatSize::F32,
            "0064" => FloatSize::F64,
            _ => {
                return Err(Error::InvalidHeader(format!(
                    "invalid float-size field: {float_raw}"
                )))
            }
        };

        Ok((
            Self {
                major,
                minor,
                format,
                float_size,
            },
            base + 16,
        ))
    }
}
