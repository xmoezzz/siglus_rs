//! Structural parser for AVG32 `TPC32` scene headers and compact values
//! (labels, scenario menus and the `0x80` variable-reference numbers).
//!
//! Besides the label table, a scene header may describe a *scenario menu*:
//! a two-level list of chapters whose entries point at sub-blocks of the
//! bytecode.  Scenes with such a menu run one sub-block at a time and
//! resolve jumps relative to the current block.

use anyhow::{Result, bail};

#[cfg_attr(feature = "portable-vm", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneSubmenu {
    pub id: u8,
    pub string: usize,
    /// Per repetition: the last flag byte of that repetition's condition list.
    pub flags: Vec<u8>,
    /// Per repetition: code offset of the sub-block.
    pub starts: Vec<i32>,
}

#[cfg_attr(feature = "portable-vm", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneMenu {
    pub id: u8,
    pub string: usize,
    pub submenus: Vec<SceneSubmenu>,
}

#[cfg_attr(feature = "portable-vm", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Avg32SceneHeader {
    pub labels: Vec<u32>,
    pub menus: Vec<SceneMenu>,
    pub menu_strings: Vec<Vec<u8>>,
    /// `smenu.start`: code offset of the `0x00` byte that shows the menu.
    pub menu_start: i32,
    /// `smenu.loopback`: offset of the first repeatable block (0 if none).
    pub loopback: i32,
    /// Byte offset of the first opcode after the header.
    pub code_offset: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKind {
    Constant,
    Variable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneValue {
    pub value: u32,
    pub kind: ValueKind,
}

fn int(bytes: &[u8], at: usize) -> Result<i32> {
    match bytes.get(at..at + 4) {
        Some(slice) => Ok(i32::from_le_bytes(slice.try_into().expect("four bytes"))),
        None => bail!("avg32: truncated scene header at {at:#x}"),
    }
}

fn byte(bytes: &[u8], at: usize) -> Result<u8> {
    bytes
        .get(at)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("avg32: truncated scene header at {at:#x}"))
}

impl Avg32SceneHeader {
    pub fn parse(bytes: &[u8]) -> Result<Self> {
        if bytes.get(..5) != Some(b"TPC32") {
            bail!("avg32: expected TPC32 scene magic");
        }
        let label_count = int(bytes, 0x18)?;
        if !(0..=0x10000).contains(&label_count) {
            bail!("avg32: unreasonable label count {label_count}");
        }
        let labels = (0..label_count as usize)
            .map(|index| int(bytes, 0x20 + index * 4).map(|value| value as u32))
            .collect::<Result<Vec<_>>>()?;
        let mut pad = label_count as usize * 4 + 0x50;
        let pad2 = if int(bytes, pad)? == 5 { 4 } else { 0 };
        let menu_count = int(bytes, pad - 0x24)?;
        let repeat_count = int(bytes, pad - 0x28)? - 1;
        if !(0..=8).contains(&menu_count) {
            bail!("avg32: unreasonable scenario menu count {menu_count}");
        }
        let mut menus = Vec::with_capacity(menu_count as usize);
        let mut string_count = 0usize;
        for _ in 0..menu_count {
            let id = byte(bytes, pad)?;
            let submenu_count = byte(bytes, pad + 1)?;
            pad += 2;
            let string = string_count;
            string_count += 1;
            let mut submenus = Vec::with_capacity(usize::from(submenu_count));
            for _ in 0..submenu_count {
                let sub_id = byte(bytes, pad)?;
                let repeats = byte(bytes, pad + 1)?;
                pad += 2;
                let sub_string = string_count;
                string_count += 1;
                let mut flags = Vec::with_capacity(usize::from(repeats));
                for _ in 0..repeats {
                    let conditions = usize::from(byte(bytes, pad)?);
                    pad += 1;
                    let mut last = 0;
                    for _ in 0..conditions {
                        last = byte(bytes, pad)?;
                        pad += 3;
                    }
                    flags.push(last);
                }
                submenus.push(SceneSubmenu {
                    id: sub_id,
                    string: sub_string,
                    flags,
                    starts: Vec::new(),
                });
            }
            menus.push(SceneMenu {
                id,
                string,
                submenus,
            });
        }
        let mut menu_strings = Vec::with_capacity(string_count);
        for _ in 0..string_count {
            let length = usize::from(byte(bytes, pad)?);
            let text = bytes.get(pad + 1..pad + 1 + length).unwrap_or(&[]);
            let end = text
                .iter()
                .position(|byte| *byte == 0)
                .unwrap_or(text.len());
            menu_strings.push(text[..end].to_vec());
            pad += length + 1;
        }
        let menu_start = int(bytes, pad + pad2 + 0x0f)? - 1;
        let code_offset = pad + pad2 + 0x13;
        if code_offset > bytes.len() {
            bail!("avg32: scene header runs past the end of the scene");
        }
        let code = &bytes[code_offset..];
        let mut loopback = 0;
        if !menus.is_empty() {
            let mut cursor = menu_start + 1;
            if repeat_count > 0 {
                loopback = cursor + 4;
                for _ in 0..repeat_count {
                    cursor += int(code, cursor.max(0) as usize)? + 4;
                }
            }
            for menu in &mut menus {
                for submenu in &mut menu.submenus {
                    for _ in 0..submenu.flags.len() {
                        submenu.starts.push(cursor + 4);
                        cursor += int(code, cursor.max(0) as usize)? + 4;
                    }
                }
            }
        }
        Ok(Self {
            labels,
            menus,
            menu_strings,
            menu_start,
            loopback,
            code_offset,
        })
    }
}

/// Parses AVG32's compact integer/variable reference encoding.
pub fn parse_scene_value(bytes: &[u8]) -> Result<(SceneValue, usize)> {
    let first = *bytes
        .first()
        .ok_or_else(|| anyhow::anyhow!("avg32: missing scene value"))?;
    let len = usize::from((first >> 4) & 0x07);
    if len == 0 || len > bytes.len() {
        bail!("avg32: malformed compact scene value");
    }
    let mut value = 0u32;
    for byte in bytes[1..len].iter().rev() {
        value = (value << 8) | u32::from(*byte);
    }
    value = (value << 4) | u32::from(first & 0x0f);
    Ok((
        SceneValue {
            value,
            kind: if first & 0x80 != 0 {
                ValueKind::Variable
            } else {
                ValueKind::Constant
            },
        },
        len,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_a_compact_value() {
        assert_eq!(
            parse_scene_value(&[0x1f]).unwrap(),
            (
                SceneValue {
                    value: 15,
                    kind: ValueKind::Constant
                },
                1
            )
        );
        assert_eq!(
            parse_scene_value(&[0x91]).unwrap().0.kind,
            ValueKind::Variable
        );
        assert_eq!(parse_scene_value(&[0x21, 0x23]).unwrap().0.value, 0x231);
    }

    pub(crate) fn plain_scene(code: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"TPC32");
        bytes.extend_from_slice(&[0; 0x13]);
        bytes.extend_from_slice(&0u32.to_le_bytes()); // label count
        bytes.extend_from_slice(&0u32.to_le_bytes());
        // 0x30 bytes of fixed header: repeat count + 1 at +0x08, menus at +0x0c.
        let mut fixed = [0u8; 0x30];
        fixed[0x08..0x0c].copy_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&fixed);
        // 0x13-byte trailer; the menu start lives at +0x0f.
        let mut trailer = [0u8; 0x13];
        trailer[0x0f..0x13].copy_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&trailer);
        bytes.extend_from_slice(code);
        bytes
    }

    #[test]
    fn parses_a_header_without_menus() {
        let bytes = plain_scene(&[0xfe, b'A', 0]);
        let header = Avg32SceneHeader::parse(&bytes).unwrap();
        assert!(header.menus.is_empty());
        assert_eq!(&bytes[header.code_offset..], &[0xfe, b'A', 0]);
    }
}
