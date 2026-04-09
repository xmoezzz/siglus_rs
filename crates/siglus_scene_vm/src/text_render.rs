//! Text rendering helpers.
//!
//! We prefer fontdue + TTF from `project_dir/font`. If no font is available,
//! fall back to a 5x7 ASCII bitmap font so text is still visible.

use crate::assets::RgbaImage;
use crate::image_manager::{ImageId, ImageManager};
use fontdue::Font;
use std::path::Path;

#[derive(Debug, Default)]
pub struct FontCache {
    font: Option<Font>,
}

impl FontCache {
    pub fn new() -> Self {
        Self { font: None }
    }

    pub fn is_loaded(&self) -> bool {
        self.font.is_some()
    }

    pub fn load_from_font_dir(&mut self, font_dir: &Path) -> bool {
        if self.font.is_some() {
            return true;
        }
        let Ok(entries) = std::fs::read_dir(font_dir) else {
            return false;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if ext != "ttf" && ext != "otf" && ext != "ttc" {
                continue;
            }
            let Ok(bytes) = std::fs::read(&path) else {
                continue;
            };
            if let Ok(font) = Font::from_bytes(bytes, fontdue::FontSettings::default()) {
                self.font = Some(font);
                return true;
            }
        }
        false
    }

    pub fn render_text(
        &self,
        images: &mut ImageManager,
        text: &str,
        font_px: f32,
        max_w: u32,
        max_h: u32,
    ) -> Option<ImageId> {
        let Some(font) = self.font.as_ref() else {
            return render_text_image_basic(images, text, font_px as u32, max_w, max_h);
        };
        render_text_fontdue(images, font, text, font_px, max_w, max_h)
    }
}

pub fn render_text_image_basic(
    images: &mut ImageManager,
    text: &str,
    font_px: u32,
    max_w: u32,
    max_h: u32,
) -> Option<ImageId> {
    if text.is_empty() || max_w == 0 || max_h == 0 {
        return None;
    }
    let scale = (font_px / 7).max(1);
    let glyph_w = 5 * scale;
    let glyph_h = 7 * scale;
    let advance = glyph_w + scale;
    let line_height = glyph_h + scale;

    let mut rgba = vec![0u8; (max_w * max_h * 4) as usize];
    let mut x = 0u32;
    let mut y = 0u32;

    for ch in text.chars() {
        if ch == '\n' {
            x = 0;
            y = y.saturating_add(line_height);
            if y >= max_h {
                break;
            }
            continue;
        }
        if ch == '\t' {
            x = x.saturating_add(advance * 2);
            continue;
        }
        if x + glyph_w > max_w {
            x = 0;
            y = y.saturating_add(line_height);
            if y >= max_h {
                break;
            }
        }
        draw_glyph_5x7(&mut rgba, max_w, max_h, x, y, ch, scale);
        x = x.saturating_add(advance);
    }

    let img = RgbaImage {
        width: max_w,
        height: max_h,
        rgba,
    };
    Some(images.insert_image(img))
}

fn render_text_fontdue(
    images: &mut ImageManager,
    font: &Font,
    text: &str,
    font_px: f32,
    max_w: u32,
    max_h: u32,
) -> Option<ImageId> {
    if text.is_empty() || max_w == 0 || max_h == 0 {
        return None;
    }
    let mut rgba = vec![0u8; (max_w * max_h * 4) as usize];

    let mut x = 0.0f32;
    let mut y = font_px;
    let line_height = font
        .horizontal_line_metrics(font_px)
        .map(|m| (m.ascent - m.descent + m.line_gap).max(1.0))
        .unwrap_or(font_px * 1.3);

    let mut word = String::new();
    let mut flush_word = |word: &str, x: &mut f32, y: &mut f32, rgba: &mut [u8]| {
        if word.is_empty() {
            return;
        }
        let word_w: f32 = word
            .chars()
            .map(|c| font.metrics(c, font_px).advance_width)
            .sum();
        if *x + word_w > max_w as f32 {
            *x = 0.0;
            *y += line_height;
        }
        if *y >= max_h as f32 {
            return;
        }
        for ch in word.chars() {
            let metrics = font.metrics(ch, font_px);
            let (gmetrics, glyph) = font.rasterize(ch, font_px);
            let gx = *x + gmetrics.xmin as f32;
            let gy = *y + gmetrics.ymin as f32;
            for gy_i in 0..gmetrics.height {
                let py = gy as i32 + gy_i as i32;
                if py < 0 || py as u32 >= max_h {
                    continue;
                }
                for gx_i in 0..gmetrics.width {
                    let px = gx as i32 + gx_i as i32;
                    if px < 0 || px as u32 >= max_w {
                        continue;
                    }
                    let src = glyph[gy_i * gmetrics.width + gx_i];
                    if src == 0 {
                        continue;
                    }
                    let idx = ((py as u32 * max_w + px as u32) * 4) as usize;
                    rgba[idx] = 255;
                    rgba[idx + 1] = 255;
                    rgba[idx + 2] = 255;
                    rgba[idx + 3] = src;
                }
            }
            *x += metrics.advance_width;
        }
    };

    for ch in text.chars() {
        match ch {
            '\n' => {
                flush_word(&word, &mut x, &mut y, &mut rgba);
                word.clear();
                x = 0.0;
                y += line_height;
                if y >= max_h as f32 {
                    break;
                }
            }
            '\t' => {
                flush_word(&word, &mut x, &mut y, &mut rgba);
                word.clear();
                x += font_px * 2.0;
            }
            ' ' => {
                flush_word(&word, &mut x, &mut y, &mut rgba);
                word.clear();
                x += font.metrics(' ', font_px).advance_width;
            }
            _ => {
                word.push(ch);
            }
        }
    }
    flush_word(&word, &mut x, &mut y, &mut rgba);

    let img = RgbaImage {
        width: max_w,
        height: max_h,
        rgba,
    };
    Some(images.insert_image(img))
}

fn draw_glyph_5x7(rgba: &mut [u8], w: u32, h: u32, x: u32, y: u32, ch: char, scale: u32) {
    let glyph = glyph_5x7(ch);
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..5 {
            if (bits >> (4 - col)) & 1 == 0 {
                continue;
            }
            let px = x + col as u32 * scale;
            let py = y + row as u32 * scale;
            for sy in 0..scale {
                let yy = py + sy;
                if yy >= h {
                    continue;
                }
                for sx in 0..scale {
                    let xx = px + sx;
                    if xx >= w {
                        continue;
                    }
                    let idx = ((yy * w + xx) * 4) as usize;
                    rgba[idx] = 255;
                    rgba[idx + 1] = 255;
                    rgba[idx + 2] = 255;
                    rgba[idx + 3] = 255;
                }
            }
        }
    }
}

fn glyph_5x7(ch: char) -> [u8; 7] {
    let c = if ch.is_ascii_lowercase() {
        ch.to_ascii_uppercase()
    } else {
        ch
    };
    match c {
        'A' => [
            0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'B' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110,
        ],
        'C' => [
            0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110,
        ],
        'D' => [
            0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110,
        ],
        'E' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111,
        ],
        'F' => [
            0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'G' => [
            0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110,
        ],
        'H' => [
            0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001,
        ],
        'I' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b11111,
        ],
        'J' => [
            0b11111, 0b00010, 0b00010, 0b00010, 0b00010, 0b10010, 0b01100,
        ],
        'K' => [
            0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001,
        ],
        'L' => [
            0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111,
        ],
        'M' => [
            0b10001, 0b11011, 0b10101, 0b10001, 0b10001, 0b10001, 0b10001,
        ],
        'N' => [
            0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001,
        ],
        'O' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        'Q' => [
            0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101,
        ],
        'R' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001,
        ],
        'S' => [
            0b01110, 0b10001, 0b10000, 0b01110, 0b00001, 0b10001, 0b01110,
        ],
        'T' => [
            0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'U' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110,
        ],
        'V' => [
            0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100,
        ],
        'W' => [
            0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001,
        ],
        'X' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001,
        ],
        'Y' => [
            0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100,
        ],
        'Z' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111,
        ],
        '0' => [
            0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111,
        ],
        '3' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        '9' => [
            0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100,
        ],
        ' ' => [0, 0, 0, 0, 0, 0, 0],
        '.' => [0, 0, 0, 0, 0, 0b01100, 0b01100],
        ',' => [0, 0, 0, 0, 0, 0b01100, 0b01000],
        '!' => [0b00100, 0b00100, 0b00100, 0b00100, 0, 0b00100, 0],
        '?' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0, 0b00100],
        ':' => [0, 0b01100, 0b01100, 0, 0b01100, 0b01100, 0],
        ';' => [0, 0b01100, 0b01100, 0, 0b01100, 0b01000, 0],
        '-' => [0, 0, 0, 0b11111, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 0b11111],
        '/' => [0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0, 0],
        '\\' => [0b10000, 0b01000, 0b00100, 0b00010, 0b00001, 0, 0],
        '\'' => [0b00100, 0b00100, 0, 0, 0, 0, 0],
        '"' => [0b01010, 0b01010, 0, 0, 0, 0, 0],
        '[' => [
            0b01110, 0b01000, 0b01000, 0b01000, 0b01000, 0b01000, 0b01110,
        ],
        ']' => [
            0b01110, 0b00010, 0b00010, 0b00010, 0b00010, 0b00010, 0b01110,
        ],
        _ => [
            0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111,
        ],
    }
}
