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
        self.render_text_into(images, None, text, font_px, max_w, max_h)
    }

    pub fn render_text_into(
        &self,
        images: &mut ImageManager,
        target: Option<ImageId>,
        text: &str,
        font_px: f32,
        max_w: u32,
        max_h: u32,
    ) -> Option<ImageId> {
        let img = self.render_text_rgba(text, font_px, max_w, max_h)?;
        match target {
            Some(id) => {
                images.replace_image(id, img).ok()?;
                Some(id)
            }
            None => Some(images.insert_image(img)),
        }
    }

    pub fn render_text_rgba(
        &self,
        text: &str,
        font_px: f32,
        max_w: u32,
        max_h: u32,
    ) -> Option<RgbaImage> {
        let Some(font) = self.font.as_ref() else {
            return render_text_image_basic_rgba(text, font_px as u32, max_w, max_h);
        };
        render_text_fontdue_rgba(font, text, font_px, max_w, max_h)
    }
}

pub fn render_text_image_basic(
    images: &mut ImageManager,
    text: &str,
    font_px: u32,
    max_w: u32,
    max_h: u32,
) -> Option<ImageId> {
    let img = render_text_image_basic_rgba(text, font_px, max_w, max_h)?;
    Some(images.insert_image(img))
}

pub fn render_text_image_basic_rgba(
    text: &str,
    font_px: u32,
    max_w: u32,
    max_h: u32,
) -> Option<RgbaImage> {
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

    Some(RgbaImage {
        width: max_w,
        height: max_h,
        rgba,
    })
}

fn render_text_fontdue(
    images: &mut ImageManager,
    font: &Font,
    text: &str,
    font_px: f32,
    max_w: u32,
    max_h: u32,
) -> Option<ImageId> {
    let img = render_text_fontdue_rgba(font, text, font_px, max_w, max_h)?;
    Some(images.insert_image(img))
}

fn render_text_fontdue_rgba(
    font: &Font,
    text: &str,
    font_px: f32,
    max_w: u32,
    max_h: u32,
) -> Option<RgbaImage> {
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
    let flush_word = |word: &str, x: &mut f32, y: &mut f32, rgba: &mut [u8]| {
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

    Some(RgbaImage {
        width: max_w,
        height: max_h,
        rgba,
    })
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
    match ch.to_ascii_uppercase() {
        'A' => [0x0E, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'B' => [0x1E, 0x11, 0x11, 0x1E, 0x11, 0x11, 0x1E],
        'C' => [0x0E, 0x11, 0x10, 0x10, 0x10, 0x11, 0x0E],
        'D' => [0x1E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1E],
        'E' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x1F],
        'F' => [0x1F, 0x10, 0x10, 0x1E, 0x10, 0x10, 0x10],
        'G' => [0x0E, 0x11, 0x10, 0x17, 0x11, 0x11, 0x0E],
        'H' => [0x11, 0x11, 0x11, 0x1F, 0x11, 0x11, 0x11],
        'I' => [0x0E, 0x04, 0x04, 0x04, 0x04, 0x04, 0x0E],
        'J' => [0x01, 0x01, 0x01, 0x01, 0x11, 0x11, 0x0E],
        'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1F],
        'M' => [0x11, 0x1B, 0x15, 0x15, 0x11, 0x11, 0x11],
        'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        'O' => [0x0E, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'P' => [0x1E, 0x11, 0x11, 0x1E, 0x10, 0x10, 0x10],
        'Q' => [0x0E, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0D],
        'R' => [0x1E, 0x11, 0x11, 0x1E, 0x14, 0x12, 0x11],
        'S' => [0x0F, 0x10, 0x10, 0x0E, 0x01, 0x01, 0x1E],
        'T' => [0x1F, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0E],
        'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0A, 0x04],
        'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0A],
        'X' => [0x11, 0x11, 0x0A, 0x04, 0x0A, 0x11, 0x11],
        'Y' => [0x11, 0x11, 0x0A, 0x04, 0x04, 0x04, 0x04],
        'Z' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1F],
        '0' => [0x0E, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0E],
        '1' => [0x04, 0x0C, 0x04, 0x04, 0x04, 0x04, 0x0E],
        '2' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1F],
        '3' => [0x1E, 0x01, 0x01, 0x06, 0x01, 0x01, 0x1E],
        '4' => [0x02, 0x06, 0x0A, 0x12, 0x1F, 0x02, 0x02],
        '5' => [0x1F, 0x10, 0x10, 0x1E, 0x01, 0x01, 0x1E],
        '6' => [0x0E, 0x10, 0x10, 0x1E, 0x11, 0x11, 0x0E],
        '7' => [0x1F, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        '8' => [0x0E, 0x11, 0x11, 0x0E, 0x11, 0x11, 0x0E],
        '9' => [0x0E, 0x11, 0x11, 0x0F, 0x01, 0x01, 0x0E],
        ':' => [0x00, 0x04, 0x04, 0x00, 0x04, 0x04, 0x00],
        '.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x06, 0x06],
        ',' => [0x00, 0x00, 0x00, 0x00, 0x06, 0x06, 0x04],
        '-' => [0x00, 0x00, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F],
        '/' => [0x01, 0x02, 0x04, 0x08, 0x10, 0x00, 0x00],
        '\\' => [0x10, 0x08, 0x04, 0x02, 0x01, 0x00, 0x00],
        '[' => [0x0E, 0x08, 0x08, 0x08, 0x08, 0x08, 0x0E],
        ']' => [0x0E, 0x02, 0x02, 0x02, 0x02, 0x02, 0x0E],
        '(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        ')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        '#' => [0x0A, 0x0A, 0x1F, 0x0A, 0x1F, 0x0A, 0x0A],
        '+' => [0x00, 0x04, 0x04, 0x1F, 0x04, 0x04, 0x00],
        '=' => [0x00, 0x1F, 0x00, 0x1F, 0x00, 0x00, 0x00],
        '*' => [0x00, 0x11, 0x0A, 0x1F, 0x0A, 0x11, 0x00],
        '?' => [0x0E, 0x11, 0x01, 0x02, 0x04, 0x00, 0x04],
        '!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x04],
        '>' => [0x10, 0x08, 0x04, 0x02, 0x04, 0x08, 0x10],
        '<' => [0x01, 0x02, 0x04, 0x08, 0x04, 0x02, 0x01],
        '|' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        ' ' => [0x00; 7],
        _ => [0x1F, 0x11, 0x15, 0x15, 0x15, 0x11, 0x1F],
    }
}
