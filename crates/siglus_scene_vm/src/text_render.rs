//! Text rendering helpers.
//!
//! TTF/OTF fonts are preferred. The lookup order mirrors the engine use case:
//! game-local fonts first, then engine-local fonts, then the compile-time
//! embedded default font, then platform fonts. If no font can be loaded, a
//! small ASCII bitmap fallback is used only to keep debug
//! text visible.

use crate::assets::RgbaImage;
use crate::image_manager::{ImageId, ImageManager};
use ab_glyph::{point, Font, FontArc, PxScale, ScaleFont};
use std::path::{Path, PathBuf};

mod embedded_font {
    include!(concat!(env!("OUT_DIR"), "/siglus_embedded_font.rs"));
}

#[derive(Debug, Clone, Copy)]
pub struct TextStyle {
    pub color: (u8, u8, u8),
    pub shadow_color: (u8, u8, u8),
    pub fuchi_color: (u8, u8, u8),
    pub shadow: bool,
    pub fuchi: bool,
    pub bold: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            color: (255, 255, 255),
            shadow_color: (0, 0, 0),
            fuchi_color: (0, 0, 0),
            shadow: true,
            fuchi: false,
            bold: false,
        }
    }
}


#[derive(Debug, Default)]
pub struct FontCache {
    font: Option<FontArc>,
    loaded_from: Option<PathBuf>,
}

impl FontCache {
    pub fn new() -> Self {
        Self {
            font: None,
            loaded_from: None,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.font.is_some()
    }

    pub fn loaded_from(&self) -> Option<&Path> {
        self.loaded_from.as_deref()
    }

    pub fn load_for_project(&mut self, project_dir: &Path) -> bool {
        if self.font.is_some() {
            return true;
        }

        let mut dirs = Vec::new();
        dirs.push(project_dir.join("font"));
        dirs.push(project_dir.join("fonts"));

        if let Ok(exe) = std::env::current_exe() {
            if let Some(exe_dir) = exe.parent() {
                dirs.push(exe_dir.join("font"));
                dirs.push(exe_dir.join("fonts"));
            }
        }

        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        dirs.push(manifest_dir.join("assets").join("font"));
        dirs.push(manifest_dir.join("assets").join("fonts"));

        for dir in dirs {
            if self.load_from_font_dir(&dir) {
                return true;
            }
        }

        if self.try_load_embedded_default_font() {
            return true;
        }

        for path in platform_font_candidates() {
            if self.try_load_font_file(&path) {
                return true;
            }
        }

        false
    }

    pub fn load_from_font_dir(&mut self, font_dir: &Path) -> bool {
        if self.font.is_some() {
            return true;
        }
        let Ok(entries) = std::fs::read_dir(font_dir) else {
            return false;
        };

        let mut files = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() && is_supported_font_path(&path) {
                files.push(path);
            }
        }
        files.sort_by_key(|path| font_path_priority(path));

        for path in files {
            if self.try_load_font_file(&path) {
                return true;
            }
        }
        false
    }

    fn try_load_font_file(&mut self, path: &Path) -> bool {
        if self.font.is_some() {
            return true;
        }
        if !path.is_file() || !is_supported_font_path(path) {
            return false;
        }
        let Ok(bytes) = std::fs::read(path) else {
            return false;
        };
        match FontArc::try_from_vec(bytes) {
            Ok(font) => {
                self.font = Some(font);
                self.loaded_from = Some(path.to_path_buf());
                true
            }
            Err(_) => false,
        }
    }

    fn try_load_embedded_default_font(&mut self) -> bool {
        if self.font.is_some() {
            return true;
        }
        let Some(bytes) = embedded_font::EMBEDDED_DEFAULT_FONT else {
            return false;
        };
        match FontArc::try_from_vec(bytes.to_vec()) {
            Ok(font) => {
                self.font = Some(font);
                let source = embedded_font::EMBEDDED_DEFAULT_FONT_SOURCE.unwrap_or("embedded:default-font");
                self.loaded_from = Some(PathBuf::from(source));
                true
            }
            Err(_) => false,
        }
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

    pub fn render_mwnd_text(
        &self,
        images: &mut ImageManager,
        text: &str,
        font_px: f32,
        max_w: u32,
        max_h: u32,
        moji_space: Option<(i64, i64)>,
    ) -> Option<ImageId> {
        let img = self.render_mwnd_text_rgba(text, font_px, max_w, max_h, moji_space)?;
        Some(images.insert_image(img))
    }

    pub fn render_mwnd_text_styled(
        &self,
        images: &mut ImageManager,
        text: &str,
        font_px: f32,
        max_w: u32,
        max_h: u32,
        moji_space: Option<(i64, i64)>,
        style: TextStyle,
    ) -> Option<ImageId> {
        let img = self.render_mwnd_text_rgba_styled(text, font_px, max_w, max_h, moji_space, style)?;
        Some(images.insert_image(img))
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
        render_text_ab_glyph_rgba(font, text, font_px, max_w, max_h)
    }

    pub fn render_mwnd_text_rgba(
        &self,
        text: &str,
        font_px: f32,
        max_w: u32,
        max_h: u32,
        moji_space: Option<(i64, i64)>,
    ) -> Option<RgbaImage> {
        self.render_mwnd_text_rgba_styled(text, font_px, max_w, max_h, moji_space, TextStyle::default())
    }

    pub fn render_mwnd_text_rgba_styled(
        &self,
        text: &str,
        font_px: f32,
        max_w: u32,
        max_h: u32,
        moji_space: Option<(i64, i64)>,
        style: TextStyle,
    ) -> Option<RgbaImage> {
        let Some(font) = self.font.as_ref() else {
            return render_text_image_basic_rgba(text, font_px as u32, max_w, max_h);
        };
        render_mwnd_text_ab_glyph_rgba_styled(font, text, font_px, max_w, max_h, moji_space, style)
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


#[derive(Debug, Clone)]
struct RasterGlyph {
    width: usize,
    height: usize,
    xmin: i32,
    ymin: i32,
    bitmap: Vec<u8>,
}

fn rasterize_ab_glyph(font: &FontArc, ch: char, font_px: f32) -> RasterGlyph {
    let scale = PxScale::from(font_px.max(1.0));
    let scaled = font.as_scaled(scale);
    let glyph_id = scaled.glyph_id(ch);
    let glyph = glyph_id.with_scale_and_position(scale, point(0.0, 0.0));
    let Some(outlined) = scaled.outline_glyph(glyph) else {
        return RasterGlyph {
            width: 0,
            height: 0,
            xmin: 0,
            ymin: 0,
            bitmap: Vec::new(),
        };
    };

    let bounds = outlined.px_bounds();
    let xmin = bounds.min.x.floor() as i32;
    let ymin = bounds.min.y.floor() as i32;
    let xmax = bounds.max.x.ceil() as i32;
    let ymax = bounds.max.y.ceil() as i32;
    let width = (xmax - xmin).max(0) as usize;
    let height = (ymax - ymin).max(0) as usize;
    if width == 0 || height == 0 {
        return RasterGlyph {
            width: 0,
            height: 0,
            xmin,
            ymin,
            bitmap: Vec::new(),
        };
    }

    let shifted_glyph = glyph_id.with_scale_and_position(scale, point((-xmin) as f32, (-ymin) as f32));
    let Some(shifted) = scaled.outline_glyph(shifted_glyph) else {
        return RasterGlyph {
            width: 0,
            height: 0,
            xmin,
            ymin,
            bitmap: Vec::new(),
        };
    };

    let mut bitmap = vec![0u8; width * height];
    shifted.draw(|gx, gy, cov| {
        let x = gx as usize;
        let y = gy as usize;
        if x < width && y < height {
            bitmap[y * width + x] = (cov * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    });

    RasterGlyph {
        width,
        height,
        xmin,
        ymin,
        bitmap,
    }
}


fn render_mwnd_text_ab_glyph_rgba(
    font: &FontArc,
    text: &str,
    font_px: f32,
    max_w: u32,
    max_h: u32,
    moji_space: Option<(i64, i64)>,
) -> Option<RgbaImage> {
    render_mwnd_text_ab_glyph_rgba_styled(font, text, font_px, max_w, max_h, moji_space, TextStyle::default())
}

fn render_mwnd_text_ab_glyph_rgba_styled(
    font: &FontArc,
    text: &str,
    font_px: f32,
    max_w: u32,
    max_h: u32,
    moji_space: Option<(i64, i64)>,
    style: TextStyle,
) -> Option<RgbaImage> {
    if text.is_empty() || max_w == 0 || max_h == 0 {
        return None;
    }

    let (space_x, space_y) = moji_space.unwrap_or((-1, 10));
    let font_cell = font_px.round().max(1.0) as i32;
    let full_cell_w = font_cell.max(1);
    let half_cell_w = (font_cell / 2).max(1);
    let line_h = (font_cell + space_y as i32).max(font_cell).max(1);
    let mut rgba = vec![0u8; (max_w * max_h * 4) as usize];

    let mut x = 0i32;
    let mut y = 0i32;

    for ch in text.chars() {
        match ch {
            '\r' => continue,
            '\n' => {
                x = 0;
                y += line_h;
                if y >= max_h as i32 {
                    break;
                }
                continue;
            }
            '\t' => {
                x += (full_cell_w + space_x as i32).max(1) * 2;
                continue;
            }
            _ => {}
        }

        let cell_w = if is_hankaku(ch) { half_cell_w } else { full_cell_w };
        let advance = (cell_w + space_x as i32).max(1);
        if x > 0 && x + cell_w > max_w as i32 {
            x = 0;
            y += line_h;
            if y >= max_h as i32 {
                break;
            }
        }

        let glyph = rasterize_ab_glyph(font, ch, font_px);
        if glyph.width == 0 || glyph.height == 0 {
            x += advance;
            continue;
        }

        let cell_inner_x = ((cell_w - glyph.width as i32) / 2).max(0);
        let cell_inner_y = ((line_h - glyph.height as i32) / 2).max(0);
        let draw_x = x + cell_inner_x + glyph.xmin.min(0);
        let draw_y = y + cell_inner_y;

        if style.fuchi {
            for (ox, oy) in [(-1, 0), (1, 0), (0, -1), (0, 1)] {
                draw_glyph_bitmap(
                    &mut rgba,
                    max_w,
                    max_h,
                    draw_x + ox,
                    draw_y + oy,
                    glyph.width,
                    glyph.height,
                    &glyph.bitmap,
                    (style.fuchi_color.0, style.fuchi_color.1, style.fuchi_color.2, 220),
                );
            }
        }
        if style.shadow {
            draw_glyph_bitmap(
                &mut rgba,
                max_w,
                max_h,
                draw_x + 1,
                draw_y + 1,
                glyph.width,
                glyph.height,
                &glyph.bitmap,
                (style.shadow_color.0, style.shadow_color.1, style.shadow_color.2, 180),
            );
        }
        draw_glyph_bitmap(
            &mut rgba,
            max_w,
            max_h,
            draw_x,
            draw_y,
            glyph.width,
            glyph.height,
            &glyph.bitmap,
            (style.color.0, style.color.1, style.color.2, 255),
        );
        if style.bold {
            draw_glyph_bitmap(
                &mut rgba,
                max_w,
                max_h,
                draw_x + 1,
                draw_y,
                glyph.width,
                glyph.height,
                &glyph.bitmap,
                (style.color.0, style.color.1, style.color.2, 220),
            );
        }

        x += advance;
    }

    Some(RgbaImage {
        width: max_w,
        height: max_h,
        rgba,
    })
}

fn is_hankaku(ch: char) -> bool {
    ch.is_ascii() || matches!(ch as u32, 0xFF61..=0xFF9F)
}

fn draw_glyph_bitmap(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    glyph_w: usize,
    glyph_h: usize,
    glyph: &[u8],
    color: (u8, u8, u8, u8),
) {
    for gy in 0..glyph_h {
        let py = y + gy as i32;
        if py < 0 || py as u32 >= h {
            continue;
        }
        for gx in 0..glyph_w {
            let px = x + gx as i32;
            if px < 0 || px as u32 >= w {
                continue;
            }
            let src = glyph[gy * glyph_w + gx];
            if src == 0 {
                continue;
            }
            let src_a = ((src as u16 * color.3 as u16) / 255) as u8;
            blend_rgba_pixel(rgba, w, px as u32, py as u32, color.0, color.1, color.2, src_a);
        }
    }
}

fn blend_rgba_pixel(
    rgba: &mut [u8],
    w: u32,
    x: u32,
    y: u32,
    sr: u8,
    sg: u8,
    sb: u8,
    sa: u8,
) {
    let idx = ((y * w + x) * 4) as usize;
    let da = rgba[idx + 3] as u16;
    let sa_u = sa as u16;
    let inv_sa = 255u16.saturating_sub(sa_u);
    let out_a = sa_u + (da * inv_sa + 127) / 255;
    if out_a == 0 {
        rgba[idx] = 0;
        rgba[idx + 1] = 0;
        rgba[idx + 2] = 0;
        rgba[idx + 3] = 0;
        return;
    }
    let blend = |src: u8, dst: u8| -> u8 {
        let src_p = src as u16 * sa_u;
        let dst_p = dst as u16 * da * inv_sa / 255;
        ((src_p + dst_p + out_a / 2) / out_a).min(255) as u8
    };
    rgba[idx] = blend(sr, rgba[idx]);
    rgba[idx + 1] = blend(sg, rgba[idx + 1]);
    rgba[idx + 2] = blend(sb, rgba[idx + 2]);
    rgba[idx + 3] = out_a.min(255) as u8;
}

fn render_text_ab_glyph_rgba(
    font: &FontArc,
    text: &str,
    font_px: f32,
    max_w: u32,
    max_h: u32,
) -> Option<RgbaImage> {
    if text.is_empty() || max_w == 0 || max_h == 0 {
        return None;
    }
    let mut rgba = vec![0u8; (max_w * max_h * 4) as usize];

    let scaled = font.as_scaled(PxScale::from(font_px.max(1.0)));
    let ascent = scaled.ascent().max(1.0);
    let line_height = (scaled.height() + scaled.line_gap()).max(1.0);

    let mut x = 0.0f32;
    let mut baseline_y = ascent.max(1.0);

    for ch in text.chars() {
        match ch {
            '\r' => continue,
            '\n' => {
                x = 0.0;
                baseline_y += line_height;
                if baseline_y - ascent >= max_h as f32 {
                    break;
                }
                continue;
            }
            '\t' => {
                x += scaled.h_advance(scaled.glyph_id(' ')).max(0.0) * 2.0;
                continue;
            }
            _ => {}
        }

        let advance = scaled.h_advance(scaled.glyph_id(ch)).max(0.0);
        if x > 0.0 && x + advance > max_w as f32 {
            x = 0.0;
            baseline_y += line_height;
            if baseline_y - ascent >= max_h as f32 {
                break;
            }
        }

        let glyph = rasterize_ab_glyph(font, ch, font_px);
        let gx = x + glyph.xmin as f32;
        let gy = baseline_y + glyph.ymin as f32;
        for gy_i in 0..glyph.height {
            let py = gy as i32 + gy_i as i32;
            if py < 0 || py as u32 >= max_h {
                continue;
            }
            for gx_i in 0..glyph.width {
                let px = gx as i32 + gx_i as i32;
                if px < 0 || px as u32 >= max_w {
                    continue;
                }
                let src = glyph.bitmap[gy_i * glyph.width + gx_i];
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
        x += advance;
    }

    Some(RgbaImage {
        width: max_w,
        height: max_h,
        rgba,
    })
}

pub fn embedded_default_font_available() -> bool {
    embedded_font::EMBEDDED_DEFAULT_FONT.is_some()
}

pub fn embedded_default_font_names() -> &'static [&'static str] {
    if embedded_default_font_available() {
        embedded_font::EMBEDDED_DEFAULT_FONT_ALIASES
    } else {
        &[]
    }
}

pub fn font_name_matches_embedded_default(name: &str) -> bool {
    if !embedded_default_font_available() {
        return false;
    }
    let needle = normalize_font_name_for_match(name);
    if needle.is_empty() {
        return false;
    }
    embedded_font::EMBEDDED_DEFAULT_FONT_ALIASES
        .iter()
        .any(|alias| normalize_font_name_for_match(alias) == needle)
}

fn normalize_font_name_for_match(name: &str) -> String {
    name.chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '-' && *ch != '_' && *ch != '.')
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

fn is_supported_font_path(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("ttf" | "otf" | "ttc")
    )
}

fn font_path_priority(path: &Path) -> (u8, u8, String) {
    let name_original = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let name = name_original.to_ascii_lowercase();
    let ext_score = match path
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("ttf") | Some("otf") => 0,
        Some("ttc") => 1,
        _ => 2,
    };
    let family_score = if name.contains("ms pgothic")
        || name.contains("mspgothic")
        || name.contains("ms-pgothic")
        || name.contains("msgothic")
        || name_original.contains("ＭＳ Ｐゴシック")
        || name_original.contains("MS PGothic")
    {
        0
    } else if name.contains("pgothic") || name_original.contains("Ｐゴシック") {
        1
    } else if name.contains("gothic") || name_original.contains("ゴシック") {
        2
    } else {
        3
    };
    (family_score, ext_score, name)
}

fn platform_font_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let windir = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let fonts = windir.join("Fonts");
        out.push(fonts.join("msgothic.ttc"));
        out.push(fonts.join("msgothic.ttf"));
        out.push(fonts.join("YuGothM.ttc"));
        out.push(fonts.join("YuGothR.ttc"));
    }

    #[cfg(target_os = "macos")]
    {
        out.push(PathBuf::from("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc"));
        out.push(PathBuf::from("/System/Library/Fonts/ヒラギノ角ゴシック W4.ttc"));
        out.push(PathBuf::from("/System/Library/Fonts/Supplemental/Arial Unicode.ttf"));
        out.push(PathBuf::from("/System/Library/Fonts/Supplemental/Osaka.ttf"));
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "android"))]
    {
        out.push(PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"));
        out.push(PathBuf::from("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc"));
        out.push(PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJKjp-Regular.otf"));
        out.push(PathBuf::from("/usr/share/fonts/truetype/fonts-japanese-gothic.ttf"));
    }

    out
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
