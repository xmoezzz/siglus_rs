//! Low-level primitives shared by the independent Siglus build tools.
//!
//! The formats here follow the recovered C++ `C_stream`, `Clzss_pack`, and
//! XOR loops. Higher-level compiler and linker policy deliberately lives in
//! the three tool crates.

#![cfg_attr(target_os = "horizon", no_std)]

#[cfg(target_os = "horizon")]
extern crate alloc;

#[cfg(target_os = "horizon")]
use siglus_switch_compat as std;

#[cfg(target_os = "horizon")]
use alloc::{borrow::ToOwned, format, string::String, vec, vec::Vec};

use std::path::Path;

use anyhow::{Context, Result, bail};

/// Fixed Gameexe XOR table recovered from the existing Rust resources.
///
/// The original `TNM_GAMEEXE_DAT_ANGOU_CODE` macro is missing from the
/// recovered C++ headers, so callers must be able to override this value.
pub const RECOVERED_GAMEEXE_KEY: [u8; 256] = siglus_assets_compat::GAMEEXE_KEY;

/// Fixed scene XOR table recovered from the existing Rust resources.
///
/// The original `TNM_EASY_ANGOU_CODE` macro is missing from the recovered C++
/// headers, so callers must be able to override this value.
pub const RECOVERED_SCENE_KEY: [u8; 256] = siglus_assets_compat::SCENE_KEY;

/// Initial 16-byte executable-derived key from `exe_angou.cpp`.
pub const EXE_KEY_SEED: [u8; 16] = [
    0x36, 0x59, 0xc9, 0x73, 0x2e, 0xb5, 0x09, 0xba, 0xe4, 0x4c, 0xf2, 0x6a, 0xa2, 0x34, 0xec, 0x7c,
];

pub fn xor_cycle(data: &mut [u8], key: &[u8]) {
    if key.is_empty() {
        return;
    }
    for (byte, key_byte) in data.iter_mut().zip(key.iter().cycle()) {
        *byte ^= *key_byte;
    }
}

/// Implements `C_tnms_exe_angou::make_angou_element` exactly.
pub fn derive_exe_key(key_material: &[u8]) -> [u8; 16] {
    let mut out = EXE_KEY_SEED;
    if key_material.is_empty() {
        return out;
    }
    let count = key_material.len().max(out.len());
    for i in 0..count {
        out[i % out.len()] ^= key_material[i % key_material.len()];
    }
    out
}

/// Byte-oriented Siglus LZSS compressor (`12/4` token split, break-even 1).
///
/// The match search is deterministic and format-compatible with the original
/// `Clzss_tree_find` implementation.
pub fn lzss_pack(src: &[u8]) -> Vec<u8> {
    let mut out = vec![0; 8];
    let mut finder = LzssTreeFind::new(src);
    let mut replace_count = 0usize;
    loop {
        if replace_count > 0 {
            finder.advance(replace_count);
            replace_count = 0;
        }
        if finder.position >= src.len() {
            break;
        }
        let flag_pos = out.len();
        out.push(0);
        let mut flags = 0u8;
        for bit in 0..8 {
            if finder.matching_size >= 2 {
                replace_count = finder.matching_size;
                let distance = finder.window_top.wrapping_sub(finder.matching_target) % 4096;
                let token = ((distance as u16) << 4) | ((replace_count - 2) as u16);
                out.extend_from_slice(&token.to_le_bytes());
            } else {
                replace_count = 1;
                flags |= 1 << bit;
                out.push(src[finder.position]);
            }
            if bit != 7 {
                finder.advance(replace_count);
                replace_count = 0;
                if finder.position >= src.len() {
                    break;
                }
            }
        }
        out[flag_pos] = flags;
    }
    let archive_size = u32::try_from(out.len()).expect("LZSS output exceeds u32");
    let original_size = u32::try_from(src.len()).expect("LZSS input exceeds u32");
    out[0..4].copy_from_slice(&archive_size.to_le_bytes());
    out[4..8].copy_from_slice(&original_size.to_le_bytes());
    out
}

#[derive(Clone, Copy)]
struct LzssTreeNode {
    parent: usize,
    small: usize,
    big: usize,
}

struct LzssTreeFind<'a> {
    source: &'a [u8],
    nodes: Vec<LzssTreeNode>,
    root: usize,
    unused: usize,
    position: usize,
    matching_target: usize,
    matching_size: usize,
    window_top: usize,
}

impl<'a> LzssTreeFind<'a> {
    fn new(source: &'a [u8]) -> Self {
        let root = 4096;
        let unused = 4097;
        let mut nodes = vec![
            LzssTreeNode {
                parent: unused,
                small: unused,
                big: unused,
            };
            4098
        ];
        nodes[0].parent = root;
        nodes[root].parent = 0;
        nodes[root].big = 0;
        Self {
            source,
            nodes,
            root,
            unused,
            position: 0,
            matching_target: 0,
            matching_size: 0,
            window_top: 0,
        }
    }

    fn advance(&mut self, count: usize) {
        for _ in 0..count {
            self.position += 1;
            let page = self.position / 4096;
            self.window_top = (self.window_top + 1) % 4096;
            self.disconnect(self.window_top);
            let mut target = self.nodes[self.root].big;
            self.matching_size = 0;
            let matching_limit = (self.source.len() - self.position).min(17);
            if matching_limit == 0 {
                return;
            }
            loop {
                let mut candidate = page * 4096 + target;
                if target > self.position % 4096 {
                    candidate -= 4096;
                }
                let mut matching_count = 0;
                let mut comparison = 0i32;
                while matching_count < matching_limit {
                    comparison = i32::from(self.source[self.position + matching_count])
                        - i32::from(self.source[candidate + matching_count]);
                    if comparison != 0 {
                        break;
                    }
                    matching_count += 1;
                }
                if matching_count > self.matching_size {
                    self.matching_size = matching_count;
                    self.matching_target = target;
                    if matching_count == matching_limit {
                        self.replace(target, self.window_top);
                        break;
                    }
                }
                if self.connect_additional(&mut target, self.window_top, comparison) {
                    break;
                }
            }
        }
    }

    fn disconnect(&mut self, target: usize) {
        let node = self.nodes[target];
        if node.parent == self.unused {
            return;
        }
        let parent = node.parent;
        let next;
        if node.big == self.unused {
            next = node.small;
            self.nodes[next].parent = parent;
            if self.nodes[parent].big == target {
                self.nodes[parent].big = next;
            } else {
                self.nodes[parent].small = next;
            }
            self.nodes[target].parent = self.unused;
        } else if node.small == self.unused {
            next = node.big;
            self.nodes[next].parent = parent;
            if self.nodes[parent].big == target {
                self.nodes[parent].big = next;
            } else {
                self.nodes[parent].small = next;
            }
            self.nodes[target].parent = self.unused;
        } else {
            let mut replacement = node.small;
            while self.nodes[replacement].big != self.unused {
                replacement = self.nodes[replacement].big;
            }
            self.disconnect(replacement);
            self.replace(target, replacement);
        }
    }

    fn replace(&mut self, target: usize, replacement: usize) {
        let target_node = self.nodes[target];
        let parent = target_node.parent;
        if self.nodes[parent].small == target {
            self.nodes[parent].small = replacement;
        } else {
            self.nodes[parent].big = replacement;
        }
        self.nodes[replacement] = target_node;
        self.nodes[target_node.small].parent = replacement;
        self.nodes[target_node.big].parent = replacement;
        self.nodes[target].parent = self.unused;
    }

    fn connect_additional(&mut self, target: &mut usize, next: usize, comparison: i32) -> bool {
        let child = if comparison >= 0 {
            self.nodes[*target].big
        } else {
            self.nodes[*target].small
        };
        if child != self.unused {
            *target = child;
            return false;
        }
        if comparison >= 0 {
            self.nodes[*target].big = next;
        } else {
            self.nodes[*target].small = next;
        }
        self.nodes[next] = LzssTreeNode {
            parent: *target,
            small: self.unused,
            big: self.unused,
        };
        true
    }
}

pub fn lzss_unpack(src: &[u8]) -> Result<Vec<u8>> {
    if src.len() < 8 {
        bail!("LZSS input is shorter than its 8-byte header");
    }
    let archive_size = u32::from_le_bytes(src[0..4].try_into().unwrap()) as usize;
    let original_size = u32::from_le_bytes(src[4..8].try_into().unwrap()) as usize;
    if archive_size > src.len() || archive_size < 8 {
        bail!(
            "invalid LZSS archive size {archive_size} for {} bytes",
            src.len()
        );
    }
    let mut out = Vec::with_capacity(original_size);
    let mut pos = 8;
    while out.len() < original_size {
        if pos >= archive_size {
            bail!("truncated LZSS flag stream");
        }
        let mut flags = src[pos];
        pos += 1;
        for _ in 0..8 {
            if out.len() == original_size {
                break;
            }
            if flags & 1 != 0 {
                if pos >= archive_size {
                    bail!("truncated LZSS literal");
                }
                out.push(src[pos]);
                pos += 1;
            } else {
                if pos + 2 > archive_size {
                    bail!("truncated LZSS back-reference");
                }
                let token = u16::from_le_bytes([src[pos], src[pos + 1]]);
                pos += 2;
                let distance = (token >> 4) as usize;
                let length = (token as usize & 0x0f) + 2;
                if distance == 0 || distance > out.len() {
                    bail!(
                        "invalid LZSS distance {distance} at output byte {}",
                        out.len()
                    );
                }
                for _ in 0..length {
                    if out.len() == original_size {
                        break;
                    }
                    let value = out[out.len() - distance];
                    out.push(value);
                }
            }
            flags >>= 1;
        }
    }
    Ok(out)
}

pub fn put_i32(dst: &mut Vec<u8>, value: i32) {
    dst.extend_from_slice(&value.to_le_bytes());
}

pub fn put_index(dst: &mut Vec<u8>, offset: i32, size: i32) {
    put_i32(dst, offset);
    put_i32(dst, size);
}

pub fn utf16le(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

pub fn parse_hex(raw: &str) -> Result<Vec<u8>> {
    let compact: String = raw
        .chars()
        .filter(|c| !c.is_ascii_whitespace() && *c != ':' && *c != '-')
        .collect();
    if !compact.len().is_multiple_of(2) {
        bail!("hex value must contain an even number of digits");
    }
    let mut out = Vec::with_capacity(compact.len() / 2);
    for pair in compact.as_bytes().as_chunks::<2>().0 {
        let pair = std::str::from_utf8(pair).unwrap();
        out.push(u8::from_str_radix(pair, 16)?);
    }
    Ok(out)
}

/// Reads the repository-wide `key.toml` schema used by the runtime.  Both
/// `key = [0x.., ...]` and `key_hex = "..."` are accepted; an absent key is
/// explicitly represented as `None` so callers can select the unencrypted
/// package mode rather than silently accepting malformed material.
pub fn load_key16_from_toml(path: &Path) -> Result<Option<[u8; 16]>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    parse_key16_toml(&text)
}

pub fn parse_key16_toml(text: &str) -> Result<Option<[u8; 16]>> {
    let mut assignment = None;
    let mut collecting = false;
    let mut raw = String::new();
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if !collecting {
            let Some((lhs, rhs)) = line.split_once('=') else {
                bail!("key.toml: malformed assignment {line:?}");
            };
            let key = lhs.trim();
            if key != "key" && key != "key_hex" {
                continue;
            }
            assignment = Some(key.to_owned());
            raw.push_str(rhs.trim());
            if raw.contains(']') || (raw.starts_with('"') && raw.ends_with('"')) {
                break;
            }
            collecting = true;
        } else {
            raw.push(' ');
            raw.push_str(line);
            if line.contains(']') {
                break;
            }
        }
    }
    let Some(key) = assignment else {
        return Ok(None);
    };
    let value = raw.trim();
    let bytes = if value.starts_with('[') {
        let end = value
            .find(']')
            .ok_or_else(|| anyhow::anyhow!("key.toml: {key} array missing closing ]"))?;
        let inner = &value[1..end];
        let mut bytes = Vec::new();
        for token in inner.split(',') {
            let token = token.trim();
            if token.is_empty() {
                continue;
            }
            let byte = if let Some(hex) = token
                .strip_prefix("0x")
                .or_else(|| token.strip_prefix("0X"))
            {
                u8::from_str_radix(hex, 16)
                    .with_context(|| format!("key.toml: invalid hex byte {token}"))?
            } else {
                token
                    .parse::<u8>()
                    .with_context(|| format!("key.toml: invalid byte {token}"))?
            };
            bytes.push(byte);
        }
        if bytes.is_empty() {
            bail!("key.toml: {key} array is empty");
        }
        bytes
    } else if value.starts_with('"') && value.ends_with('"') {
        parse_hex(&value[1..value.len() - 1])
            .with_context(|| format!("key.toml: invalid hex for {key}"))?
    } else {
        bail!("key.toml: {key} must be a byte array or quoted hex string");
    };
    if bytes.len() != 16 {
        bail!(
            "key.toml: key must contain exactly 16 bytes, got {}",
            bytes.len()
        );
    }
    Ok(Some(bytes.try_into().expect("length checked")))
}

// Kept private so the compiler support crate does not depend on the broader
// asset crate at runtime. These are the workspace's recovered constant copies.
mod siglus_assets_compat {
    include!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/recovered_keys.rs"
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lzss_round_trip() {
        let src = b"abcabcabcabc -- Siglus -- abcabcabcabc";
        assert_eq!(lzss_unpack(&lzss_pack(src)).unwrap(), src);
    }

    #[test]
    fn exe_key_derivation_matches_cpp_loop() {
        assert_eq!(derive_exe_key(&[]), EXE_KEY_SEED);
        let got = derive_exe_key(b"12345678");
        assert_eq!(got[0], 0x36 ^ b'1');
        assert_eq!(got[8], 0xe4 ^ b'1');
    }

    #[test]
    fn key_toml_schema_accepts_array_hex_and_absence() {
        assert!(parse_key16_toml("title = \"demo\"\n").unwrap().is_none());
        let key =
            parse_key16_toml("key = [0x00, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]\n")
                .unwrap()
                .unwrap();
        assert_eq!(key[1], 1);
        assert_eq!(
            parse_key16_toml("key_hex = \"000102030405060708090a0b0c0d0e0f\"\n")
                .unwrap()
                .unwrap()[15],
            15
        );
        assert!(parse_key16_toml("key = [1, 2]\n").is_err());
        assert!(parse_key16_toml("key = [0xGG]\n").is_err());
        let multiline = parse_key16_toml(
            "# repository format\nkey = [\n  0x00, 0x01, 0x02, 0x03,\n  0x04, 0x05, 0x06, 0x07,\n  0x08, 0x09, 0x0A, 0x0B,\n  0x0C, 0x0D, 0x0E, 0x0F,\n]\n",
        )
        .unwrap()
        .unwrap();
        assert_eq!(multiline[15], 0x0f);
    }
}
