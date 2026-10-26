//! Text encoding of a title's scenario, configuration and save data.
//!
//! AVG32 games store text as Shift-JIS, but translated releases re-encode
//! their scenarios as GBK, Big5 or UTF-8. The engine keeps every string as
//! raw bytes in the selected encoding and decodes them only when drawing or
//! looking up files. File names are first decoded with the selected encoding
//! and, when no such file exists, with Shift-JIS (translations usually keep
//! the original resource names).

use std::fmt;
use std::str::FromStr;
use std::sync::atomic::{AtomicU8, Ordering};

use encoding_rs::{BIG5, Encoding, GBK, SHIFT_JIS, UTF_8};

#[cfg_attr(feature = "portable-vm", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Nls {
    #[default]
    Sjis,
    Gbk,
    Big5,
    Utf8,
}

impl Nls {
    pub const ALL: [Nls; 4] = [Nls::Sjis, Nls::Gbk, Nls::Big5, Nls::Utf8];

    pub fn name(self) -> &'static str {
        match self {
            Nls::Sjis => "sjis",
            Nls::Gbk => "gbk",
            Nls::Big5 => "big5",
            Nls::Utf8 => "utf8",
        }
    }

    pub fn encoding(self) -> &'static Encoding {
        match self {
            Nls::Sjis => SHIFT_JIS,
            Nls::Gbk => GBK,
            Nls::Big5 => BIG5,
            Nls::Utf8 => UTF_8,
        }
    }

    /// Byte length of the character starting with `byte` (1 for ASCII and
    /// the message control bytes).
    pub fn char_len(self, byte: u8) -> usize {
        match self {
            Nls::Sjis => {
                if (0x81..=0x9f).contains(&byte) || (0xe0..=0xfc).contains(&byte) {
                    2
                } else {
                    1
                }
            }
            Nls::Gbk | Nls::Big5 => {
                if (0x81..=0xfe).contains(&byte) {
                    2
                } else {
                    1
                }
            }
            Nls::Utf8 => match byte {
                0xc0..=0xdf => 2,
                0xe0..=0xef => 3,
                0xf0..=0xf7 => 4,
                _ => 1,
            },
        }
    }

    pub fn decode(self, bytes: &[u8]) -> String {
        self.encoding()
            .decode_without_bom_handling(bytes)
            .0
            .into_owned()
    }

    /// Strict decoding: `None` if `bytes` is not valid in this encoding.
    pub fn decode_strict(self, bytes: &[u8]) -> Option<String> {
        self.encoding()
            .decode_without_bom_handling_and_without_replacement(bytes)
            .map(|text| text.into_owned())
    }

    pub fn encode(self, text: &str) -> Vec<u8> {
        self.encoding().encode(text).0.into_owned()
    }
}

impl fmt::Display for Nls {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl FromStr for Nls {
    type Err = anyhow::Error;

    fn from_str(text: &str) -> anyhow::Result<Self> {
        let key: String = text
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_lowercase())
            .collect();
        Ok(match key.as_str() {
            "sjis" | "shiftjis" | "cp932" | "ms932" | "jis" | "ja" => Nls::Sjis,
            "gbk" | "gb2312" | "gb18030" | "cp936" | "chs" | "zhcn" => Nls::Gbk,
            "big5" | "cp950" | "cht" | "zhtw" => Nls::Big5,
            "utf8" | "utf" | "unicode" => Nls::Utf8,
            _ => anyhow::bail!("unknown NLS encoding {text:?} (expected sjis, gbk, big5 or utf8)"),
        })
    }
}

#[cfg(not(feature = "portable-vm"))]
static CURRENT: AtomicU8 = AtomicU8::new(0);

/// The encoding in effect for this process.
#[cfg(not(feature = "portable-vm"))]
pub fn current() -> Nls {
    Nls::ALL[usize::from(CURRENT.load(Ordering::Relaxed)).min(Nls::ALL.len() - 1)]
}

#[cfg(not(feature = "portable-vm"))]
pub fn set(nls: Nls) {
    let index = Nls::ALL.iter().position(|each| *each == nls).unwrap_or(0);
    CURRENT.store(index as u8, Ordering::Relaxed);
}

pub fn decode(bytes: &[u8]) -> String {
    current().decode(bytes)
}

pub fn encode(text: &str) -> Vec<u8> {
    current().encode(text)
}

pub fn char_len(byte: u8) -> usize {
    current().char_len(byte)
}

/// Decodes the character at the start of `bytes`: `(char, byte length)`.
pub fn char_at(bytes: &[u8]) -> (char, usize) {
    let Some(&first) = bytes.first() else {
        return ('\0', 0);
    };
    let length = char_len(first).min(bytes.len());
    let character = decode(&bytes[..length]).chars().next().unwrap_or(' ');
    (character, length)
}

/// The character's Shift-JIS code (`0x8140` for `　`, `0x41` for `A`), used
/// by the line-breaking rules and the JIS-indexed novel font.
pub fn sjis_code(character: char) -> Option<u16> {
    let mut buffer = [0; 4];
    let (bytes, _, error) = SHIFT_JIS.encode(character.encode_utf8(&mut buffer));
    if error {
        return None;
    }
    match *bytes {
        [single] => Some(u16::from(single)),
        [lead, trail] => Some(u16::from(lead) << 8 | u16::from(trail)),
        _ => None,
    }
}

/// Re-encodes Shift-JIS bytes (engine-generated text such as full-width
/// digits) into the current encoding.
pub fn from_sjis(bytes: &[u8]) -> Vec<u8> {
    match current() {
        Nls::Sjis => bytes.to_vec(),
        nls => nls.encode(&SHIFT_JIS.decode_without_bom_handling(bytes).0),
    }
}

/// Decodes text read from a game file: UTF-8 when valid, otherwise the
/// current encoding, otherwise Shift-JIS.
pub fn decode_file_text(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_owned();
    }
    current()
        .decode_strict(bytes)
        .unwrap_or_else(|| Nls::Sjis.decode(bytes))
}

/// Decodes a file name stored in game data (archives, catalogues): the
/// current encoding when valid, otherwise Shift-JIS.
pub fn decode_name(bytes: &[u8]) -> String {
    current()
        .decode_strict(bytes)
        .unwrap_or_else(|| Nls::Sjis.decode(bytes))
}

/// The Shift-JIS reading of a file name that was decoded with the current
/// encoding, when it differs.
pub fn sjis_fallback(name: &str) -> Option<String> {
    let nls = current();
    if nls == Nls::Sjis || name.is_ascii() {
        return None;
    }
    let bytes = nls.encode(name);
    let fallback = Nls::Sjis.decode_strict(&bytes)?;
    (fallback != name).then_some(fallback)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn character_lengths_follow_the_encoding() {
        assert_eq!(Nls::Sjis.char_len(0x82), 2);
        assert_eq!(Nls::Sjis.char_len(0xa0), 1);
        assert_eq!(Nls::Gbk.char_len(0xa0), 2);
        assert_eq!(Nls::Big5.char_len(0xfe), 2);
        assert_eq!(Nls::Utf8.char_len(0xe3), 3);
        assert_eq!(Nls::Utf8.char_len(b'A'), 1);
    }

    #[test]
    fn parses_names() {
        assert_eq!("Shift_JIS".parse::<Nls>().unwrap(), Nls::Sjis);
        assert_eq!("GBK".parse::<Nls>().unwrap(), Nls::Gbk);
        assert_eq!("big5".parse::<Nls>().unwrap(), Nls::Big5);
        assert_eq!("utf-8".parse::<Nls>().unwrap(), Nls::Utf8);
        assert!("latin1".parse::<Nls>().is_err());
    }

    #[test]
    fn round_trips_and_sjis_codes() {
        for nls in Nls::ALL {
            let bytes = nls.encode("「中」");
            assert_eq!(nls.decode(&bytes), "「中」");
        }
        assert_eq!(sjis_code('【'), Some(0x8179));
        assert_eq!(sjis_code('A'), Some(0x41));
    }
}

#[cfg(feature = "portable-vm")]
thread_local! { static SCOPED: std::cell::Cell<Nls> = const { std::cell::Cell::new(Nls::Sjis) }; }
#[cfg(feature = "portable-vm")]
pub fn current() -> Nls {
    SCOPED.with(std::cell::Cell::get)
}
#[cfg(feature = "portable-vm")]
pub fn set(nls: Nls) {
    SCOPED.with(|slot| slot.set(nls));
}
#[cfg(feature = "portable-vm")]
pub struct ScopedNls(Nls);
#[cfg(feature = "portable-vm")]
impl ScopedNls {
    pub fn enter(nls: Nls) -> Self {
        let previous = current();
        set(nls);
        Self(previous)
    }
}
#[cfg(feature = "portable-vm")]
impl Drop for ScopedNls {
    fn drop(&mut self) {
        set(self.0);
    }
}
