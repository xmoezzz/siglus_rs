//! Display-only cell layout. Native VM string operations deliberately do not use this module.
//!
//! `UnicodeCells` is an unofficial spacing extension, not a shaping or bidi engine.

use std::collections::HashMap;
use std::ops::Range;
use unicode_normalization::UnicodeNormalization;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TextLayoutProfile {
    #[default]
    Native,
    UnicodeCells,
}

/// One display scalar, with its original source cluster. NFC is applied only
/// to this working representation, never to script strings or save text.
#[derive(Debug, Clone)]
pub struct DisplayChar {
    pub ch: char,
    pub bytes: Range<usize>,
    pub utf16: Range<usize>,
    pub cluster_start: bool,
}

/// Font advances in em units for display characters added by the extension.
/// Editbox interaction keeps the same measurements as its rasterizer.
#[derive(Debug, Clone, Default)]
pub struct TextCellMetrics {
    pub(crate) advances: HashMap<char, f32>,
}

impl TextCellMetrics {
    pub fn cell(&self, profile: TextLayoutProfile, ch: char, full: i64) -> i64 {
        if profile.uses_font_metrics(ch)
            && let Some(ratio) = self.advances.get(&ch)
        {
            return (full as f32 * ratio).ceil().max(1.0) as i64;
        }
        profile.cell(ch, full)
    }

    pub(crate) fn extend(&mut self, other: Self) {
        self.advances.extend(other.advances);
    }
}

impl TextLayoutProfile {
    pub(crate) fn uses_font_metrics(self, ch: char) -> bool {
        self == Self::UnicodeCells
            && Self::Native.columns(ch) == 2
            && self.columns(ch) == 1
            && (ch.is_alphabetic() || ch.is_numeric())
    }
    /// Alphabetic text uses narrow Unicode widths. Other characters retain
    /// East Asian ambiguous widths, so Japanese ellipses, dashes and symbols
    /// keep their native cells. Marks and format controls consume no cell.
    /// No language or script whitelist is involved.
    pub fn columns(self, ch: char) -> i64 {
        match self {
            Self::Native => {
                if ch.is_ascii() || matches!(ch as u32, 0xff61..=0xff9f) {
                    1
                } else {
                    2
                }
            }
            Self::UnicodeCells => {
                if ch.is_alphabetic() || ch.is_numeric() {
                    ch.width().unwrap_or(0) as i64
                } else {
                    ch.width_cjk().unwrap_or(0) as i64
                }
            }
        }
    }

    pub fn cell(self, ch: char, full: i64) -> i64 {
        match self.columns(ch) {
            0 => 0,
            1 => (full + 1) / 2,
            _ => full,
        }
    }

    pub fn chars(self, text: &str) -> Vec<DisplayChar> {
        let mut utf16 = 0;
        if self == Self::Native {
            return text
                .char_indices()
                .map(|(start, ch)| {
                    let from = utf16;
                    utf16 += ch.len_utf16();
                    DisplayChar {
                        ch,
                        bytes: start..start + ch.len_utf8(),
                        utf16: from..utf16,
                        cluster_start: true,
                    }
                })
                .collect();
        }
        text.grapheme_indices(true)
            .flat_map(|(start, cluster)| {
                let from = utf16;
                utf16 += cluster.encode_utf16().count();
                cluster.nfc().enumerate().map(move |(i, ch)| DisplayChar {
                    ch,
                    bytes: start..start + cluster.len(),
                    utf16: from..utf16,
                    cluster_start: i == 0,
                })
            })
            .collect()
    }

    pub fn previous_boundary(self, text: &str, cursor: usize) -> usize {
        if self == Self::UnicodeCells {
            text.grapheme_indices(true)
                .map(|(i, _)| i)
                .take_while(|&i| i < cursor)
                .last()
                .unwrap_or(0)
        } else {
            text.char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i < cursor)
                .last()
                .unwrap_or(0)
        }
    }

    pub fn floor_boundary(self, text: &str, cursor: usize) -> usize {
        let mut cursor = cursor.min(text.len());
        while !text.is_char_boundary(cursor) {
            cursor -= 1;
        }
        if self == Self::UnicodeCells && cursor < text.len() {
            self.previous_boundary(text, cursor.saturating_add(1))
        } else {
            cursor
        }
    }

    pub fn next_boundary(self, text: &str, cursor: usize) -> usize {
        if self == Self::UnicodeCells {
            text.grapheme_indices(true)
                .map(|(i, _)| i)
                .find(|&i| i > cursor)
                .unwrap_or(text.len())
        } else {
            text.char_indices()
                .map(|(i, _)| i)
                .find(|&i| i > cursor)
                .unwrap_or(text.len())
        }
    }
}
