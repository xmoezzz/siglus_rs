//! Text rendering helpers.
//!
//! TTF/OTF fonts are preferred. The lookup order mirrors the engine use case:
//! game-local fonts first, then engine-local fonts, then the compile-time
//! embedded default font, then platform fonts. If no font can be loaded, a
//! small ASCII bitmap fallback is used only to keep debug text visible.

use crate::assets::RgbaImage;
use crate::image_manager::{ImageHandle, ImageManager};
use ab_glyph::{Font, FontArc, FontRef, PxScale, ScaleFont, point};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static MISSING_CONFIGURED_FONTS_LOGGED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();

// Share one embedded font allocation between game text and desktop dialogs.
pub(crate) static DEFAULT_FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/default.ttf");

fn log_missing_configured_font_once(requested_name: &str, fallback: Option<&Path>) {
    let key = normalize_font_name_for_match(requested_name.trim_start_matches('@'));
    if key.is_empty() {
        return;
    }

    let seen = MISSING_CONFIGURED_FONTS_LOGGED.get_or_init(|| Mutex::new(HashSet::new()));
    let first = match seen.lock() {
        Ok(mut seen) => seen.insert(key),
        Err(poisoned) => poisoned.into_inner().insert(key),
    };
    if !first {
        return;
    }

    match fallback {
        Some(path) => log::error!(
            "configured font {:?} was not found; using fallback {:?}",
            requested_name,
            path
        ),
        None => log::error!(
            "configured font {:?} was not found; using embedded default font",
            requested_name
        ),
    }
}

mod embedded_font {
    pub const EMBEDDED_DEFAULT_FONT: Option<&'static [u8]> = Some(super::DEFAULT_FONT_BYTES);
    pub const EMBEDDED_DEFAULT_FONT_SOURCE: Option<&'static str> = Some("assets/fonts/default.ttf");
    pub const EMBEDDED_DEFAULT_FONT_ALIASES: &[&str] = &[
        "ＭＳ Ｐゴシック",
        "MS PGothic",
        "MS-PGothic",
        "MSPGothic",
        "msgothic",
        "default",
    ];
}

pub const TNM_FONT_SHADOW_MODE_NONE: i64 = 0;
pub const TNM_FONT_SHADOW_MODE_SHADOW: i64 = 1;
pub const TNM_FONT_SHADOW_MODE_FUCHI: i64 = 2;
pub const TNM_FONT_SHADOW_MODE_FUCHI_SHADOW: i64 = 3;

pub fn normalize_font_shadow_mode(mode: i64) -> i64 {
    mode.clamp(TNM_FONT_SHADOW_MODE_NONE, TNM_FONT_SHADOW_MODE_FUCHI_SHADOW)
}

pub fn font_shadow_mode_flags(mode: i64) -> (bool, bool) {
    match normalize_font_shadow_mode(mode) {
        TNM_FONT_SHADOW_MODE_SHADOW => (true, false),
        TNM_FONT_SHADOW_MODE_FUCHI => (false, true),
        TNM_FONT_SHADOW_MODE_FUCHI_SHADOW => (true, true),
        _ => (false, false),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct TextStyle {
    pub color: (u8, u8, u8),
    pub shadow_color: (u8, u8, u8),
    pub fuchi_color: (u8, u8, u8),
    /// Original TNM_FONT_SHADOW_MODE_* value (0..=3).
    pub shadow_mode: i64,
    pub shadow: bool,
    pub fuchi: bool,
    pub bold: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextSpriteLayer {
    Shadow,
    Fuchi,
    Body,
}

#[derive(Debug, Clone, Copy)]
pub struct PositionedTextGlyph {
    pub ch: char,
    pub x: i32,
    pub y: i32,
    pub size: f32,
    /// Render through the original vertical-font path.  Siglus enumerates an
    /// '@' face and asks GDI for a rotated outline; this flag carries that
    /// semantic into the cross-platform rasterizer instead of only changing
    /// layout coordinates.
    pub vertical: bool,
    pub style: TextStyle,
}

#[derive(Debug, Clone)]
pub struct PositionedTextRender {
    pub image: ImageHandle,
    pub offset_x: i32,
    pub offset_y: i32,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            color: (255, 255, 255),
            shadow_color: (0, 0, 0),
            fuchi_color: (0, 0, 0),
            shadow_mode: TNM_FONT_SHADOW_MODE_SHADOW,
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
    loaded_face_index: u32,
    /// Normalized engine-visible face name used to select `font`.
    ///
    /// The original engine clears its glyph cache whenever the effective
    /// SCRIPT/SYSCOM font changes.  Keeping the request here gives this port
    /// the same invalidation boundary instead of pinning the first loaded face
    /// for the lifetime of the process.
    requested_name: String,
    /// Faces already resolved, by normalized request: switching back to a
    /// face neither rescans the font directories nor reads a font file
    /// again (each scan read whole files, tens of MiB, per switch).
    resolved: std::collections::HashMap<String, (FontArc, Option<PathBuf>, u32)>,
    project_dir: Option<PathBuf>,
    fallback_files: OnceLock<Vec<FallbackFontFile>>,
    glyph_fallbacks: Mutex<std::collections::HashMap<char, Option<(usize, usize)>>>,
}

#[derive(Debug)]
struct FallbackFontFile {
    path: PathBuf,
    faces: OnceLock<Vec<FontArc>>,
}

/// Keep line metrics and existing glyphs on the selected face. Only the
/// outline/advance of a missing character comes from another face.
#[derive(Clone, Copy)]
struct ResolvedFonts<'a> {
    primary: &'a FontArc,
    cache: Option<&'a FontCache>,
}

impl std::ops::Deref for ResolvedFonts<'_> {
    type Target = FontArc;
    fn deref(&self) -> &FontArc {
        self.primary
    }
}

impl<'a> ResolvedFonts<'a> {
    fn for_char(self, ch: char) -> &'a FontArc {
        self.cache
            .map_or(self.primary, |cache| cache.font_for_char(self.primary, ch))
    }

    fn advance(self, ch: char, scale: PxScale) -> f32 {
        let font = self.for_char(ch);
        let scaled = font.as_scaled(scale);
        scaled.h_advance(scaled.glyph_id(ch)).max(0.0)
    }
}

impl FontCache {
    pub fn new() -> Self {
        Self {
            font: None,
            loaded_from: None,
            loaded_face_index: 0,
            requested_name: String::new(),
            resolved: std::collections::HashMap::new(),
            project_dir: None,
            fallback_files: OnceLock::new(),
            glyph_fallbacks: Mutex::new(std::collections::HashMap::new()),
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.font.is_some()
    }

    pub fn loaded_from(&self) -> Option<&Path> {
        self.loaded_from.as_deref()
    }

    pub fn loaded_face_index(&self) -> u32 {
        self.loaded_face_index
    }

    pub fn load_for_project(&mut self, project_dir: &Path) -> bool {
        self.load_for_project_named(project_dir, "")
    }

    /// Select the effective engine font and load it if necessary.
    ///
    /// Siglus resolves the active face in this order: local SCRIPT override,
    /// then the current SYSCOM configuration.  `tnm_update_font()` clears the
    /// original glyph manager when that face changes.  This method mirrors the
    /// same boundary and deliberately does not retain the first face forever.
    pub fn load_for_project_named(&mut self, project_dir: &Path, requested_name: &str) -> bool {
        if self.project_dir.as_deref() != Some(project_dir) {
            self.font = None;
            self.resolved.clear();
            self.fallback_files = OnceLock::new();
            self.glyph_fallbacks = Mutex::new(std::collections::HashMap::new());
            self.project_dir = Some(project_dir.to_path_buf());
        }
        let normalized = normalize_font_name_for_match(requested_name.trim_start_matches('@'));
        if self.font.is_some() && self.requested_name == normalized {
            return true;
        }

        if let Some((font, path, face)) = self.resolved.get(&normalized) {
            self.font = Some(font.clone());
            self.loaded_from = path.clone();
            self.loaded_face_index = *face;
            self.requested_name = normalized;
            return true;
        }
        let found = self.resolve_for_project_named(project_dir, requested_name, &normalized);
        if found && let Some(font) = &self.font {
            log::warn!(
                "[SG_FONT] requested={requested_name:?} source={:?} face={}",
                self.loaded_from,
                self.loaded_face_index
            );
            self.resolved.insert(
                normalized,
                (
                    font.clone(),
                    self.loaded_from.clone(),
                    self.loaded_face_index,
                ),
            );
        }
        found
    }

    fn resolved_fonts(&self) -> Option<ResolvedFonts<'_>> {
        self.font.as_ref().map(|primary| ResolvedFonts {
            primary,
            cache: Some(self),
        })
    }

    fn fallback_files(&self) -> &[FallbackFontFile] {
        self.fallback_files.get_or_init(|| {
            let mut paths = Vec::new();
            if let Some(project) = &self.project_dir {
                let mut dirs = vec![
                    project.join("font"),
                    project.join("fonts"),
                    project.join("dat"),
                ];
                dirs.extend(project_font_dirs(project));
                for dir in dirs {
                    let Some(dir) = crate::resource::resolve_game_path(&dir).ok().flatten() else {
                        continue;
                    };
                    let Ok(entries) = std::fs::read_dir(dir) else {
                        continue;
                    };
                    let mut fonts: Vec<_> = entries
                        .flatten()
                        .map(|entry| entry.path())
                        .filter(|path| is_supported_font_path(path))
                        .collect();
                    fonts.sort_by_key(|path| font_path_priority(path));
                    paths.extend(fonts);
                }
            }
            paths.extend(platform_font_candidates());
            let mut seen = HashSet::new();
            let mut files: Vec<_> = paths
                .into_iter()
                .filter(|path| seen.insert(path.clone()))
                .map(|path| FallbackFontFile {
                    path,
                    faces: OnceLock::new(),
                })
                .collect();
            if let Ok(font) = FontRef::try_from_slice(DEFAULT_FONT_BYTES) {
                files.push(FallbackFontFile {
                    path: PathBuf::from("embedded:default-font"),
                    faces: OnceLock::from(vec![FontArc::from(font)]),
                });
            }
            files
        })
    }

    fn font_for_char<'a>(&'a self, primary: &'a FontArc, ch: char) -> &'a FontArc {
        if primary.glyph_id(ch).0 != 0 || ch.is_control() {
            return primary;
        }
        let cached = self
            .glyph_fallbacks
            .lock()
            .ok()
            .and_then(|cache| cache.get(&ch).copied());
        let files = self.fallback_files();
        let selected = if let Some(cached) = cached {
            cached
        } else {
            let selected = files.iter().enumerate().find_map(|(file_index, file)| {
                let faces = file.faces.get_or_init(|| {
                    let Some((data, _)) = font_file_face(&file.path, "") else {
                        return Vec::new();
                    };
                    (0..ttf_parser::fonts_in_collection(data).unwrap_or(1))
                        .filter_map(|index| {
                            FontRef::try_from_slice_and_index(data, index)
                                .ok()
                                .map(FontArc::from)
                        })
                        .collect()
                });
                faces
                    .iter()
                    .position(|font| font.glyph_id(ch).0 != 0)
                    .map(|face_index| (file_index, face_index))
            });
            if let Ok(mut cache) = self.glyph_fallbacks.lock() {
                cache.insert(ch, selected);
                // At most 64 diagnostic lines per game, including unresolved characters.
                if cache.len() <= 64 {
                    log::warn!(
                        "[SG_FONT_FALLBACK] U+{:04X} primary={:?} fallback={:?}",
                        ch as u32,
                        self.loaded_from,
                        selected.map(|(index, face)| (&files[index].path, face))
                    );
                }
            }
            selected
        };
        selected
            .and_then(|(file, face)| files[file].faces.get().and_then(|faces| faces.get(face)))
            .unwrap_or(primary)
    }

    fn resolve_for_project_named(
        &mut self,
        project_dir: &Path,
        requested_name: &str,
        normalized: &str,
    ) -> bool {
        let normalized = normalized.to_owned();
        self.font = None;
        self.loaded_from = None;
        self.loaded_face_index = 0;
        self.requested_name = normalized.clone();

        let dirs = project_font_dirs(project_dir);

        // A configured face is resolved before generic fallback.  Local game
        // fonts are matched by file/family spelling, including the '@' prefix
        // used by the original vertical-font enumeration.
        if !normalized.is_empty() {
            let mut named_dirs = dirs.clone();
            named_dirs.insert(2, project_dir.join("dat"));
            for dir in &named_dirs {
                if self.load_named_from_font_dir(dir, &normalized) {
                    return true;
                }
            }
            if font_name_matches_embedded_default(requested_name.trim_start_matches('@'))
                && self.try_load_embedded_default_font()
            {
                return true;
            }
            for path in platform_font_candidates_for_name(requested_name) {
                if self.try_load_font_file_named(&path, &normalized) {
                    return true;
                }
            }
        }

        // Preserve the old project-first fallback for an empty or unavailable
        // configured face.  The original opens its font selection warning UI;
        // this cross-platform port logs and falls back rather than silently
        // continuing with the previously selected font.
        for dir in dirs {
            if self.load_from_font_dir(&dir) {
                if !normalized.is_empty() {
                    log_missing_configured_font_once(requested_name, self.loaded_from());
                }
                return true;
            }
        }

        if self.try_load_embedded_default_font() {
            if !normalized.is_empty() {
                log_missing_configured_font_once(requested_name, None);
            }
            return true;
        }

        for path in platform_font_candidates() {
            if self.try_load_font_file(&path) {
                return true;
            }
        }

        false
    }

    pub fn requested_name(&self) -> &str {
        &self.requested_name
    }

    fn load_named_from_font_dir(&mut self, font_dir: &Path, normalized_name: &str) -> bool {
        let Some(font_dir) = crate::resource::resolve_game_path(font_dir).ok().flatten() else {
            return false;
        };
        let Ok(entries) = std::fs::read_dir(&font_dir) else {
            return false;
        };
        let mut exact_file = Vec::new();
        let mut other = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || !is_supported_font_path(&path) {
                continue;
            }
            let file_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            let file_key = normalize_font_name_for_match(file_name.trim_start_matches('@'));
            let stem_key = normalize_font_name_for_match(stem.trim_start_matches('@'));
            if file_key == normalized_name || stem_key == normalized_name {
                exact_file.push(path);
            } else {
                other.push(path);
            }
        }
        exact_file.sort_by_key(|path| font_path_priority(path));
        other.sort_by_key(|path| font_path_priority(path));

        // A TTC filename identifies the collection, not the face.  Inspect the
        // OpenType naming table for every face and match the engine-visible
        // family, full, typographic-family, WWS-family, or PostScript name.
        exact_file
            .into_iter()
            .chain(other)
            .any(|path| self.try_load_font_file_named(&path, normalized_name))
    }

    pub fn load_from_font_dir(&mut self, font_dir: &Path) -> bool {
        if self.font.is_some() {
            return true;
        }
        let Some(font_dir) = crate::resource::resolve_game_path(font_dir).ok().flatten() else {
            return false;
        };
        let Ok(entries) = std::fs::read_dir(&font_dir) else {
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

    fn install_font_face(&mut self, path: &Path, data: &'static [u8], face_index: u32) -> bool {
        match FontRef::try_from_slice_and_index(data, face_index) {
            Ok(font) => {
                self.font = Some(FontArc::from(font));
                self.loaded_from = Some(path.to_path_buf());
                self.loaded_face_index = face_index;
                true
            }
            Err(_) => false,
        }
    }

    fn try_load_font_file_named(&mut self, path: &Path, normalized_name: &str) -> bool {
        if self.font.is_some() {
            return true;
        }
        if !crate::resource::game_file_exists(path) || !is_supported_font_path(path) {
            return false;
        }
        let Some((data, face_index)) = font_file_face(path, normalized_name) else {
            return false;
        };
        self.install_font_face(path, data, face_index)
    }

    fn try_load_font_file(&mut self, path: &Path) -> bool {
        if self.font.is_some() {
            return true;
        }
        if !crate::resource::game_file_exists(path) || !is_supported_font_path(path) {
            return false;
        }
        let Some((data, face_index)) = font_file_face(path, "") else {
            return false;
        };
        self.install_font_face(path, data, face_index)
    }

    fn try_load_embedded_default_font(&mut self) -> bool {
        if self.font.is_some() {
            return true;
        }
        let Some(bytes) = embedded_font::EMBEDDED_DEFAULT_FONT else {
            return false;
        };
        // Parsed in place: the font lives in the executable, so it costs no
        // heap (a copy was 7.5 MiB, repeated on every font switch).
        match FontRef::try_from_slice(bytes) {
            Ok(font) => {
                self.font = Some(FontArc::from(font));
                let source =
                    embedded_font::EMBEDDED_DEFAULT_FONT_SOURCE.unwrap_or("embedded:default-font");
                self.loaded_from = Some(PathBuf::from(source));
                self.loaded_face_index = 0;
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
    ) -> Option<ImageHandle> {
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
    ) -> Option<ImageHandle> {
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
    ) -> Option<ImageHandle> {
        self.render_mwnd_text_styled_into(
            images, None, text, font_px, max_w, max_h, moji_space, style,
        )
    }

    pub fn render_mwnd_text_styled_into(
        &self,
        images: &mut ImageManager,
        target: Option<ImageHandle>,
        text: &str,
        font_px: f32,
        max_w: u32,
        max_h: u32,
        moji_space: Option<(i64, i64)>,
        style: TextStyle,
    ) -> Option<ImageHandle> {
        let img =
            self.render_mwnd_text_rgba_styled(text, font_px, max_w, max_h, moji_space, style)?;
        match target {
            Some(id) => {
                images.replace_image(&id, img).ok()?;
                Some(id)
            }
            None => Some(images.insert_image(img)),
        }
    }

    pub fn render_positioned_glyphs_into(
        &self,
        images: &mut ImageManager,
        target: Option<ImageHandle>,
        glyphs: &[PositionedTextGlyph],
        min_w: u32,
        min_h: u32,
    ) -> Option<PositionedTextRender> {
        self.render_positioned_glyph_layer_into(images, target, glyphs, min_w, min_h, None)
    }

    pub fn render_positioned_glyph_layer_into(
        &self,
        images: &mut ImageManager,
        target: Option<ImageHandle>,
        glyphs: &[PositionedTextGlyph],
        min_w: u32,
        min_h: u32,
        layer: Option<TextSpriteLayer>,
    ) -> Option<PositionedTextRender> {
        if glyphs.is_empty() || min_w == 0 || min_h == 0 {
            return None;
        }
        let (img, offset_x, offset_y) =
            render_positioned_glyphs_rgba(self.resolved_fonts(), glyphs, min_w, min_h, layer)?;
        let image = match target {
            Some(id) => {
                images.replace_image(&id, img).ok()?;
                id
            }
            None => images.insert_image(img),
        };
        Some(PositionedTextRender {
            image,
            offset_x,
            offset_y,
        })
    }

    /// Rasterize one glyph into a tight texture for the original
    /// shadow/fuchi/body-per-glyph sprite model.  The returned offsets are
    /// relative to the requested glyph anchor and include outline/shadow
    /// padding and font bearing.
    pub fn render_single_glyph_layer_into(
        &self,
        images: &mut ImageManager,
        target: Option<ImageHandle>,
        glyph: PositionedTextGlyph,
        layer: TextSpriteLayer,
    ) -> Option<PositionedTextRender> {
        let mut local = glyph;
        local.x = 0;
        local.y = 0;
        self.render_positioned_glyph_layer_into(images, target, &[local], 1, 1, Some(layer))
    }

    pub fn render_text_into(
        &self,
        images: &mut ImageManager,
        target: Option<ImageHandle>,
        text: &str,
        font_px: f32,
        max_w: u32,
        max_h: u32,
    ) -> Option<ImageHandle> {
        let img = self.render_text_rgba(text, font_px, max_w, max_h)?;
        match target {
            Some(id) => {
                images.replace_image(&id, img).ok()?;
                Some(id)
            }
            None => Some(images.insert_image(img)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_editbox_into(
        &self,
        images: &mut ImageManager,
        target: Option<ImageHandle>,
        text: &str,
        cursor_pos: usize,
        selection: Option<(usize, usize)>,
        composition_text: &str,
        composition_cursor: Option<(usize, usize)>,
        composition_range: Option<(usize, usize)>,
        scroll_x_px: i32,
        caret_visible: bool,
        font_px: f32,
        max_w: u32,
        max_h: u32,
    ) -> Option<ImageHandle> {
        let img = render_editbox_rgba(
            self.resolved_fonts(),
            text,
            cursor_pos,
            selection,
            composition_text,
            composition_cursor,
            composition_range,
            scroll_x_px,
            caret_visible,
            font_px,
            max_w,
            max_h,
        )?;
        match target {
            Some(id) => {
                images.replace_image(&id, img).ok()?;
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
        let Some(font) = self.resolved_fonts() else {
            return render_text_image_basic_rgba(text, font_px as u32, max_w, max_h);
        };
        render_text_ab_glyph_rgba(font, text, font_px, max_w, max_h)
    }

    /// Rendered line count and line pitch of the layout [`Self::render_text_into`]
    /// would produce for the same arguments.
    ///
    /// Text drawn as one image has no per-row rectangles, so hit-testing has to be
    /// derived from the layout. Reporting it from the renderer keeps the drawn
    /// rows and the clickable rows identical (an estimate such as "40 + lines * 30"
    /// drifts by rows as soon as the pitch or the wrapped line count differs).
    pub fn text_line_metrics(
        &self,
        text: &str,
        font_px: f32,
        max_w: u32,
        max_h: u32,
    ) -> (usize, f32) {
        match self.resolved_fonts() {
            Some(font) => text_line_metrics_resolved(font, text, font_px, max_w, max_h),
            // The basic fallback renderer breaks on '\n' only; approximate its pitch.
            None => (
                text.lines().count().max(1),
                (font_px.max(1.0) * 1.3).max(1.0),
            ),
        }
    }

    pub fn render_mwnd_text_rgba(
        &self,
        text: &str,
        font_px: f32,
        max_w: u32,
        max_h: u32,
        moji_space: Option<(i64, i64)>,
    ) -> Option<RgbaImage> {
        self.render_mwnd_text_rgba_styled(
            text,
            font_px,
            max_w,
            max_h,
            moji_space,
            TextStyle::default(),
        )
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
        self.render_mwnd_text_rgba_layer_styled(
            text, font_px, max_w, max_h, moji_space, style, false, None,
        )
    }

    pub fn render_mwnd_text_layer_styled_into(
        &self,
        images: &mut ImageManager,
        target: Option<ImageHandle>,
        text: &str,
        font_px: f32,
        max_w: u32,
        max_h: u32,
        moji_space: Option<(i64, i64)>,
        style: TextStyle,
        vertical: bool,
        layer: TextSpriteLayer,
    ) -> Option<ImageHandle> {
        let img = self.render_mwnd_text_rgba_layer_styled(
            text,
            font_px,
            max_w,
            max_h,
            moji_space,
            style,
            vertical,
            Some(layer),
        )?;
        match target {
            Some(id) => {
                images.replace_image(&id, img).ok()?;
                Some(id)
            }
            None => Some(images.insert_image(img)),
        }
    }

    fn render_mwnd_text_rgba_layer_styled(
        &self,
        text: &str,
        font_px: f32,
        max_w: u32,
        max_h: u32,
        moji_space: Option<(i64, i64)>,
        style: TextStyle,
        vertical: bool,
        layer: Option<TextSpriteLayer>,
    ) -> Option<RgbaImage> {
        let Some(font) = self.resolved_fonts() else {
            if layer.is_none() || layer == Some(TextSpriteLayer::Body) {
                return render_text_image_basic_rgba(text, font_px as u32, max_w, max_h);
            }
            return Some(RgbaImage {
                width: max_w.max(1),
                height: max_h.max(1),
                center_x: 0,
                center_y: 0,
                rgba: vec![0; max_w.max(1) as usize * max_h.max(1) as usize * 4],
            });
        };
        render_mwnd_text_ab_glyph_rgba_styled(
            font, text, font_px, max_w, max_h, moji_space, style, layer, vertical,
        )
    }
}

pub fn editbox_cell_width_px(ch: char, font_px: i32) -> i32 {
    let full = font_px.max(1);
    if is_hankaku(ch) {
        ((full + 1) / 2).max(1)
    } else {
        full
    }
}

#[allow(clippy::too_many_arguments)]
fn render_editbox_rgba(
    font: Option<ResolvedFonts<'_>>,
    text: &str,
    cursor_pos: usize,
    selection: Option<(usize, usize)>,
    composition_text: &str,
    composition_cursor: Option<(usize, usize)>,
    composition_range: Option<(usize, usize)>,
    scroll_x_px: i32,
    caret_visible: bool,
    font_px: f32,
    max_w: u32,
    max_h: u32,
) -> Option<RgbaImage> {
    if max_w == 0 || max_h == 0 {
        return None;
    }

    const PAD_X: i32 = 3;
    let mut rgba = vec![255u8; (max_w * max_h * 4) as usize];
    let font_cell = font_px.round().max(1.0) as i32;
    let (baseline_y, glyph_top, glyph_bottom) = if let Some(font) = font {
        let scaled = font.as_scaled(PxScale::from(font_px.max(1.0)));
        let line_h = (scaled.height() + scaled.line_gap()).ceil().max(1.0) as i32;
        let top = ((max_h as i32 - line_h) / 2).max(0);
        let baseline = top + scaled.ascent().ceil() as i32;
        (baseline, top, (top + line_h).min(max_h as i32))
    } else {
        let glyph_h = (font_cell * 7 / 8).max(7);
        let top = ((max_h as i32 - glyph_h) / 2).max(0);
        (top + glyph_h, top, (top + glyph_h).min(max_h as i32))
    };

    let normalize = |value: usize, source: &str| {
        let mut value = value.min(source.len());
        while value > 0 && !source.is_char_boundary(value) {
            value -= 1;
        }
        value
    };
    let cursor_pos = normalize(cursor_pos, text);
    let selection = selection.map(|(a, b)| {
        let a = normalize(a, text);
        let b = normalize(b, text);
        if a <= b { (a, b) } else { (b, a) }
    });
    let composition_range = composition_range.map(|(a, b)| {
        let a = normalize(a, text);
        let b = normalize(b, text);
        if a <= b { (a, b) } else { (b, a) }
    });
    let composition_cursor = composition_cursor.map(|(a, b)| {
        let a = normalize(a, composition_text);
        let b = normalize(b, composition_text);
        if a <= b { (a, b) } else { (b, a) }
    });

    let mut x = PAD_X.saturating_sub(scroll_x_px.max(0));
    let mut caret_x = None;

    if let Some((comp_start, comp_end)) = composition_range {
        x = draw_editbox_committed_segment(
            &mut rgba,
            font,
            text,
            0,
            comp_start,
            cursor_pos,
            None,
            &mut caret_x,
            x,
            font_px,
            font_cell,
            baseline_y,
            glyph_top,
            glyph_bottom,
            max_w,
            max_h,
        );
        let comp_caret = composition_cursor
            .map(|(_, end)| end)
            .unwrap_or(composition_text.len());
        for (idx, ch) in composition_text.char_indices() {
            if idx == comp_caret {
                caret_x = Some(x);
            }
            let next = idx + ch.len_utf8();
            let selected = composition_cursor
                .map(|(start, end)| start != end && idx < end && next > start)
                .unwrap_or(false);
            let cell_w = editbox_cell_width_px(ch, font_cell);
            draw_editbox_cell(
                &mut rgba,
                font,
                ch,
                x,
                cell_w,
                selected,
                true,
                font_px,
                font_cell,
                baseline_y,
                glyph_top,
                glyph_bottom,
                max_w,
                max_h,
            );
            x = x.saturating_add(cell_w);
        }
        if comp_caret == composition_text.len() {
            caret_x = Some(x);
        }
        x = draw_editbox_committed_segment(
            &mut rgba,
            font,
            text,
            comp_end,
            text.len(),
            cursor_pos,
            None,
            &mut caret_x,
            x,
            font_px,
            font_cell,
            baseline_y,
            glyph_top,
            glyph_bottom,
            max_w,
            max_h,
        );
    } else {
        x = draw_editbox_committed_segment(
            &mut rgba,
            font,
            text,
            0,
            text.len(),
            cursor_pos,
            selection,
            &mut caret_x,
            x,
            font_px,
            font_cell,
            baseline_y,
            glyph_top,
            glyph_bottom,
            max_w,
            max_h,
        );
        if cursor_pos == text.len() {
            caret_x = Some(x);
        }
    }

    if caret_visible {
        let caret_x = caret_x.unwrap_or(PAD_X).clamp(0, max_w as i32 - 1);
        fill_rgba_rect(
            &mut rgba,
            max_w,
            max_h,
            caret_x,
            glyph_top.max(1),
            (caret_x + 1).min(max_w as i32),
            glyph_bottom.max(glyph_top + 1).min(max_h as i32 - 1),
            (0, 0, 0, 255),
        );
    }

    Some(RgbaImage {
        width: max_w,
        height: max_h,
        center_x: 0,
        center_y: 0,
        rgba,
    })
}

#[allow(clippy::too_many_arguments)]
fn draw_editbox_committed_segment(
    rgba: &mut [u8],
    font: Option<ResolvedFonts<'_>>,
    text: &str,
    range_start: usize,
    range_end: usize,
    cursor_pos: usize,
    selection: Option<(usize, usize)>,
    caret_x: &mut Option<i32>,
    mut x: i32,
    font_px: f32,
    font_cell: i32,
    baseline_y: i32,
    glyph_top: i32,
    glyph_bottom: i32,
    max_w: u32,
    max_h: u32,
) -> i32 {
    for (relative, ch) in text[range_start..range_end].char_indices() {
        let idx = range_start + relative;
        if idx == cursor_pos {
            *caret_x = Some(x);
        }
        let next = idx + ch.len_utf8();
        let selected = selection
            .map(|(start, end)| idx < end && next > start)
            .unwrap_or(false);
        let cell_w = editbox_cell_width_px(ch, font_cell);
        draw_editbox_cell(
            rgba,
            font,
            ch,
            x,
            cell_w,
            selected,
            false,
            font_px,
            font_cell,
            baseline_y,
            glyph_top,
            glyph_bottom,
            max_w,
            max_h,
        );
        x = x.saturating_add(cell_w);
    }
    x
}

#[allow(clippy::too_many_arguments)]
fn draw_editbox_cell(
    rgba: &mut [u8],
    font: Option<ResolvedFonts<'_>>,
    ch: char,
    cell_x: i32,
    cell_w: i32,
    selected: bool,
    composing: bool,
    font_px: f32,
    font_cell: i32,
    baseline_y: i32,
    glyph_top: i32,
    glyph_bottom: i32,
    max_w: u32,
    max_h: u32,
) {
    let left = cell_x.max(0);
    let right = cell_x.saturating_add(cell_w).min(max_w as i32);
    if selected && right > left {
        fill_rgba_rect(
            rgba,
            max_w,
            max_h,
            left,
            1,
            right,
            max_h as i32 - 1,
            (0, 120, 215, 255),
        );
    }

    let color = if selected {
        (255, 255, 255, 255)
    } else {
        (0, 0, 0, 255)
    };
    if let Some(font) = font {
        let glyph = rasterize_ab_glyph(font, ch, font_px);
        if glyph.width > 0 && glyph.height > 0 {
            let draw_x = cell_x + ((cell_w - glyph.width as i32) / 2).max(0);
            let draw_y = baseline_y + glyph.ymin;
            draw_glyph_bitmap(
                rgba,
                max_w,
                max_h,
                draw_x,
                draw_y,
                glyph.width,
                glyph.height,
                &glyph.bitmap,
                color,
            );
        }
    } else {
        draw_basic_glyph_color(
            rgba,
            max_w,
            max_h,
            cell_x,
            glyph_top,
            ch,
            (font_cell / 7).max(1) as u32,
            color,
        );
    }

    if composing && right > left {
        let underline_y = (glyph_bottom + 1).clamp(0, max_h as i32 - 1);
        fill_rgba_rect(
            rgba,
            max_w,
            max_h,
            left,
            underline_y,
            right,
            (underline_y + 1).min(max_h as i32),
            (0, 0, 0, 255),
        );
    }
}

fn fill_rgba_rect(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: (u8, u8, u8, u8),
) {
    let x0 = x0.clamp(0, width as i32);
    let y0 = y0.clamp(0, height as i32);
    let x1 = x1.clamp(x0, width as i32);
    let y1 = y1.clamp(y0, height as i32);
    for y in y0..y1 {
        for x in x0..x1 {
            let idx = (((y as u32 * width) + x as u32) * 4) as usize;
            rgba[idx] = color.0;
            rgba[idx + 1] = color.1;
            rgba[idx + 2] = color.2;
            rgba[idx + 3] = color.3;
        }
    }
}

fn draw_basic_glyph_color(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    ch: char,
    scale: u32,
    color: (u8, u8, u8, u8),
) {
    let glyph = glyph_5x7(ch);
    for (row, bits) in glyph.iter().enumerate() {
        for col in 0..5 {
            if (bits >> (4 - col)) & 1 == 0 {
                continue;
            }
            let px = x + col * scale as i32;
            let py = y + row as i32 * scale as i32;
            for sy in 0..scale as i32 {
                for sx in 0..scale as i32 {
                    let tx = px + sx;
                    let ty = py + sy;
                    if tx < 0 || ty < 0 || tx >= width as i32 || ty >= height as i32 {
                        continue;
                    }
                    blend_rgba_pixel(
                        rgba, width, tx as u32, ty as u32, color.0, color.1, color.2, color.3,
                    );
                }
            }
        }
    }
}

fn draw_basic_glyph_face(
    rgba: &mut [u8],
    width: u32,
    height: u32,
    x: i32,
    y: i32,
    ch: char,
    scale: u32,
    face_extent: i32,
    color: (u8, u8, u8, u8),
) {
    for oy in 0..=face_extent.max(0) {
        for ox in 0..=face_extent.max(0) {
            draw_basic_glyph_color(rgba, width, height, x + ox, y + oy, ch, scale, color);
        }
    }
}

pub fn render_text_image_basic(
    images: &mut ImageManager,
    text: &str,
    font_px: u32,
    max_w: u32,
    max_h: u32,
) -> Option<ImageHandle> {
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
        center_x: 0,
        center_y: 0,
        rgba,
    })
}

fn render_positioned_glyphs_rgba(
    font: Option<ResolvedFonts<'_>>,
    glyphs: &[PositionedTextGlyph],
    min_w: u32,
    min_h: u32,
    layer: Option<TextSpriteLayer>,
) -> Option<(RgbaImage, i32, i32)> {
    if glyphs.is_empty() || min_w == 0 || min_h == 0 {
        return None;
    }

    let mut min_x = 0i32;
    let mut min_y = 0i32;
    let mut max_x = min_w as i32;
    let mut max_y = min_h as i32;

    if let Some(font) = font {
        for placed in glyphs {
            let size = placed.size.max(1.0);
            let scaled = font.as_scaled(PxScale::from(size));
            let baseline = scaled.ascent().ceil().max(1.0) as i32;
            let glyph = rasterize_ab_glyph_oriented(font, placed.ch, size, placed.vertical);
            let pad = text_effect_padding(size.round().max(1.0) as i32, placed.style);
            let (origin_x, origin_y) = positioned_glyph_origin(placed, &glyph, baseline);
            let x0 = origin_x.saturating_sub(pad);
            let y0 = origin_y.saturating_sub(pad);
            let x1 = origin_x
                .saturating_add(glyph.width as i32)
                .saturating_add(pad);
            let y1 = origin_y
                .saturating_add(glyph.height as i32)
                .saturating_add(pad);
            min_x = min_x.min(x0);
            min_y = min_y.min(y0);
            max_x = max_x.max(x1);
            max_y = max_y.max(y1);
        }
    } else {
        for placed in glyphs {
            let size = placed.size.round().max(1.0) as i32;
            let scale = ((size + 6) / 7).max(1);
            min_x = min_x.min(placed.x - 2);
            min_y = min_y.min(placed.y - 2);
            max_x = max_x.max(placed.x + 5 * scale + 3);
            max_y = max_y.max(placed.y + 7 * scale + 3);
        }
    }

    let render_w = max_x.saturating_sub(min_x).max(1) as u32;
    let render_h = max_y.saturating_sub(min_y).max(1) as u32;
    let mut rgba = vec![
        0u8;
        (render_w as usize)
            .saturating_mul(render_h as usize)
            .saturating_mul(4)
    ];

    if let Some(font) = font {
        for placed in glyphs {
            let size = placed.size.max(1.0);
            let scaled = font.as_scaled(PxScale::from(size));
            let baseline = scaled.ascent().ceil().max(1.0) as i32;
            let glyph = rasterize_ab_glyph_oriented(font, placed.ch, size, placed.vertical);
            if glyph.width == 0 || glyph.height == 0 {
                continue;
            }
            let (origin_x, origin_y) = positioned_glyph_origin(placed, &glyph, baseline);
            let draw_x = origin_x - min_x;
            let draw_y = origin_y - min_y;
            let style = placed.style;
            // Original sorter layers are shadow(3), fuchi(4), body(5).
            // Preserve that order when rendering either the combined compatibility image or one layer.
            if style.shadow && (layer.is_none() || layer == Some(TextSpriteLayer::Shadow)) {
                let off = shadow_offset_for_style(size.round().max(1.0) as i32, style);
                draw_glyph_bitmap_face(
                    &mut rgba,
                    render_w,
                    render_h,
                    draw_x + off,
                    draw_y + off,
                    glyph.width,
                    glyph.height,
                    &glyph.bitmap,
                    shadow_face_extent(style),
                    (
                        style.shadow_color.0,
                        style.shadow_color.1,
                        style.shadow_color.2,
                        255,
                    ),
                );
            }
            if style.fuchi && (layer.is_none() || layer == Some(TextSpriteLayer::Fuchi)) {
                let off = fuchi_texture_offset();
                draw_glyph_bitmap_face(
                    &mut rgba,
                    render_w,
                    render_h,
                    draw_x + off,
                    draw_y + off,
                    glyph.width,
                    glyph.height,
                    &glyph.bitmap,
                    fuchi_face_extent(style),
                    (
                        style.fuchi_color.0,
                        style.fuchi_color.1,
                        style.fuchi_color.2,
                        255,
                    ),
                );
            }
            if layer.is_none() || layer == Some(TextSpriteLayer::Body) {
                draw_glyph_bitmap_face(
                    &mut rgba,
                    render_w,
                    render_h,
                    draw_x,
                    draw_y,
                    glyph.width,
                    glyph.height,
                    &glyph.bitmap,
                    body_face_extent(style),
                    (style.color.0, style.color.1, style.color.2, 255),
                );
            }
        }
    } else {
        for placed in glyphs {
            let style = placed.style;
            let scale = ((placed.size.round().max(1.0) as i32 + 6) / 7).max(1) as u32;
            let x = placed.x - min_x;
            let y = placed.y - min_y;
            if style.shadow && (layer.is_none() || layer == Some(TextSpriteLayer::Shadow)) {
                let off = shadow_offset_for_style(placed.size.round().max(1.0) as i32, style);
                draw_basic_glyph_face(
                    &mut rgba,
                    render_w,
                    render_h,
                    x + off,
                    y + off,
                    placed.ch,
                    scale,
                    shadow_face_extent(style),
                    (
                        style.shadow_color.0,
                        style.shadow_color.1,
                        style.shadow_color.2,
                        255,
                    ),
                );
            }
            if style.fuchi && (layer.is_none() || layer == Some(TextSpriteLayer::Fuchi)) {
                let off = fuchi_texture_offset();
                draw_basic_glyph_face(
                    &mut rgba,
                    render_w,
                    render_h,
                    x + off,
                    y + off,
                    placed.ch,
                    scale,
                    fuchi_face_extent(style),
                    (
                        style.fuchi_color.0,
                        style.fuchi_color.1,
                        style.fuchi_color.2,
                        255,
                    ),
                );
            }
            if layer.is_none() || layer == Some(TextSpriteLayer::Body) {
                draw_basic_glyph_face(
                    &mut rgba,
                    render_w,
                    render_h,
                    x,
                    y,
                    placed.ch,
                    scale,
                    body_face_extent(style),
                    (style.color.0, style.color.1, style.color.2, 255),
                );
            }
        }
    }

    Some((
        RgbaImage {
            width: render_w,
            height: render_h,
            center_x: 0,
            center_y: 0,
            rgba,
        },
        min_x,
        min_y,
    ))
}

#[derive(Debug, Clone)]
struct RasterGlyph {
    width: usize,
    height: usize,
    xmin: i32,
    ymin: i32,
    bitmap: Vec<u8>,
}

fn positioned_glyph_origin(
    placed: &PositionedTextGlyph,
    glyph: &RasterGlyph,
    horizontal_baseline: i32,
) -> (i32, i32) {
    if placed.vertical {
        let cell = placed.size.round().max(1.0) as i32;
        // GDI's tategaki path uses a baseline on the left edge and returns a
        // glyph inside the square character cell.  Centering the recovered
        // outline in that cell reproduces the observable placement without
        // applying the horizontal ascent a second time.
        (
            placed.x + (cell - glyph.width as i32) / 2,
            placed.y + (cell - glyph.height as i32) / 2,
        )
    } else {
        (
            placed.x + glyph.xmin,
            placed.y + horizontal_baseline + glyph.ymin,
        )
    }
}

fn rasterize_ab_glyph_oriented(
    font: ResolvedFonts<'_>,
    ch: char,
    font_px: f32,
    vertical: bool,
) -> RasterGlyph {
    if !vertical {
        return rasterize_ab_glyph(font, ch, font_px);
    }

    let (vertical_ch, rotate) = vertical_glyph_mapping(font.for_char(ch), ch);
    let mut glyph = rasterize_ab_glyph(font, vertical_ch, font_px);
    if rotate && glyph.width != 0 && glyph.height != 0 {
        glyph = rotate_raster_glyph_clockwise(glyph);
    }
    glyph.xmin = 0;
    glyph.ymin = 0;
    glyph
}

fn vertical_glyph_mapping(font: &FontArc, ch: char) -> (char, bool) {
    let mapped = match ch {
        '、' => '︑',
        '。' | '.' => '︒',
        ',' => '︐',
        ':' | '：' => '︓',
        ';' | '；' => '︔',
        '!' | '！' => '︕',
        '?' | '？' => '︖',
        '…' => '︙',
        '—' | '―' | 'ー' => '︱',
        '_' => '︳',
        '(' | '（' => '︵',
        ')' | '）' => '︶',
        '{' | '｛' => '︷',
        '}' | '｝' => '︸',
        '〔' => '︹',
        '〕' => '︺',
        '【' => '︻',
        '】' => '︼',
        '《' => '︽',
        '》' => '︾',
        '〈' => '︿',
        '〉' => '﹀',
        '「' => '﹁',
        '」' => '﹂',
        '『' => '﹃',
        '』' => '﹄',
        '[' | '［' => '﹇',
        ']' | '］' => '﹈',
        _ => ch,
    };
    let mapped_available = font.glyph_id(mapped).0 != 0;
    if mapped != ch && mapped_available {
        return (mapped, false);
    }

    // Windows '@' Japanese faces keep CJK glyphs upright, substitute vertical
    // punctuation, and rotate Latin runs.  `ab_glyph` exposes no GSUB `vert`
    // feature, so reproduce that visible orientation explicitly.
    let rotate = ch.is_ascii_alphanumeric()
        || matches!(ch, 'A'..='Z' | 'a'..='z' | '0'..='9')
        || matches!(ch, '-' | '=' | '<' | '>' | '/' | '\\' | '~');
    (ch, rotate)
}

fn rotate_raster_glyph_clockwise(src: RasterGlyph) -> RasterGlyph {
    let width = src.height;
    let height = src.width;
    let mut bitmap = vec![0u8; width.saturating_mul(height)];
    for y in 0..src.height {
        for x in 0..src.width {
            let dst_x = src.height - 1 - y;
            let dst_y = x;
            bitmap[dst_y * width + dst_x] = src.bitmap[y * src.width + x];
        }
    }
    RasterGlyph {
        width,
        height,
        xmin: 0,
        ymin: 0,
        bitmap,
    }
}

fn rasterize_ab_glyph(font: ResolvedFonts<'_>, ch: char, font_px: f32) -> RasterGlyph {
    let font = font.for_char(ch);
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

    let shifted_glyph =
        glyph_id.with_scale_and_position(scale, point((-xmin) as f32, (-ymin) as f32));
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
            bitmap[y * width + x] = gdi_gray8_alpha_from_coverage(cov);
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
    font: ResolvedFonts<'_>,
    text: &str,
    font_px: f32,
    max_w: u32,
    max_h: u32,
    moji_space: Option<(i64, i64)>,
) -> Option<RgbaImage> {
    render_mwnd_text_ab_glyph_rgba_styled(
        font,
        text,
        font_px,
        max_w,
        max_h,
        moji_space,
        TextStyle::default(),
        None,
        false,
    )
}

fn render_mwnd_text_ab_glyph_rgba_styled(
    font: ResolvedFonts<'_>,
    text: &str,
    font_px: f32,
    max_w: u32,
    max_h: u32,
    moji_space: Option<(i64, i64)>,
    style: TextStyle,
    layer: Option<TextSpriteLayer>,
    vertical: bool,
) -> Option<RgbaImage> {
    if text.is_empty() || max_w == 0 || max_h == 0 {
        return None;
    }

    let (space_x, space_y) = moji_space.unwrap_or((-1, 10));
    let font_cell = font_px.round().max(1.0) as i32;
    let line_h = (font_cell + space_y as i32).max(font_cell).max(1);
    let scaled = font.as_scaled(PxScale::from(font_px.max(1.0)));
    let baseline_y = scaled.ascent().ceil().max(1.0) as i32;
    let effect_pad = text_effect_padding(font_cell, style);
    let render_w = max_w.saturating_add(effect_pad.max(0) as u32 + 2);
    let render_h = max_h.saturating_add(
        (baseline_y + effect_pad)
            .max(font_cell / 4 + effect_pad + 2)
            .max(0) as u32,
    );
    let mut rgba = vec![0u8; (render_w * render_h * 4) as usize];

    let placed_chars = if vertical {
        layout_mwnd_text_vertical(text, font_cell, space_x as i32, line_h, max_w, max_h)
    } else {
        layout_mwnd_text(text, font_cell, space_x as i32, line_h, max_w, max_h)
    };
    for placed in placed_chars {
        let glyph = rasterize_ab_glyph_oriented(font, placed.ch, font_px, vertical);
        if glyph.width == 0 || glyph.height == 0 {
            continue;
        }

        let (draw_x, draw_y) = if vertical {
            (
                placed.x + (font_cell - glyph.width as i32) / 2,
                placed.y + (font_cell - glyph.height as i32) / 2,
            )
        } else {
            (placed.x + glyph.xmin, placed.y + baseline_y + glyph.ymin)
        };

        if style.shadow && (layer.is_none() || layer == Some(TextSpriteLayer::Shadow)) {
            let shadow_offset = shadow_offset_for_style(font_cell, style);
            draw_glyph_bitmap_face(
                &mut rgba,
                render_w,
                render_h,
                draw_x + shadow_offset,
                draw_y + shadow_offset,
                glyph.width,
                glyph.height,
                &glyph.bitmap,
                shadow_face_extent(style),
                (
                    style.shadow_color.0,
                    style.shadow_color.1,
                    style.shadow_color.2,
                    255,
                ),
            );
        }
        if style.fuchi && (layer.is_none() || layer == Some(TextSpriteLayer::Fuchi)) {
            draw_glyph_bitmap_face(
                &mut rgba,
                render_w,
                render_h,
                draw_x,
                draw_y,
                glyph.width,
                glyph.height,
                &glyph.bitmap,
                fuchi_face_extent(style),
                (
                    style.fuchi_color.0,
                    style.fuchi_color.1,
                    style.fuchi_color.2,
                    255,
                ),
            );
        }
        if layer.is_none() || layer == Some(TextSpriteLayer::Body) {
            draw_glyph_bitmap_face(
                &mut rgba,
                render_w,
                render_h,
                draw_x,
                draw_y,
                glyph.width,
                glyph.height,
                &glyph.bitmap,
                body_face_extent(style),
                (style.color.0, style.color.1, style.color.2, 255),
            );
        }
    }

    Some(RgbaImage {
        width: render_w,
        height: render_h,
        center_x: 0,
        center_y: 0,
        rgba,
    })
}

#[derive(Debug, Clone, Copy)]
struct MwndPlacedChar {
    ch: char,
    x: i32,
    y: i32,
    cell_w: i32,
}

fn layout_mwnd_text(
    text: &str,
    font_cell: i32,
    space_x: i32,
    line_h: i32,
    max_w: u32,
    max_h: u32,
) -> Vec<MwndPlacedChar> {
    let full_cell_w = font_cell.max(1);
    let half_cell_w = ((font_cell + 1) / 2).max(1);
    let max_w = max_w as i32;
    let max_h = max_h as i32;
    let mut out = Vec::new();
    let mut x = 0i32;
    let mut y = 0i32;
    let mut indent_x = 0i32;
    let mut line_head = true;

    for ch in text.chars() {
        match ch {
            '\r' => continue,
            '\n' => {
                x = indent_x;
                y += line_h;
                line_head = true;
                if y >= max_h {
                    break;
                }
                continue;
            }
            '\u{0007}' => {
                indent_x = 0;
                x = 0;
                y += line_h;
                line_head = true;
                if y >= max_h {
                    break;
                }
                continue;
            }
            '\t' => {
                x += (full_cell_w + space_x).max(1) * 2;
                line_head = false;
                continue;
            }
            _ => {}
        }

        let cell_w = if is_hankaku(ch) {
            half_cell_w
        } else {
            full_cell_w
        };
        let check_size = cell_w + space_x;
        let force_wrap = x > 0 && x + check_size > max_w + full_cell_w;
        let soft_wrap = x > 0 && x + check_size > max_w && !is_siglus_forbidden_line_head(ch);
        if force_wrap || soft_wrap {
            x = indent_x;
            y += line_h;
            line_head = true;
            if y >= max_h {
                break;
            }
            if ch == ' ' || ch == '\u{3000}' {
                continue;
            }
        }

        if line_head {
            if is_siglus_indent_open(ch) {
                indent_x = full_cell_w;
            } else if is_siglus_indent_close(ch) {
                indent_x = 0;
            }
        }

        out.push(MwndPlacedChar { ch, x, y, cell_w });
        x += (cell_w + space_x).max(1);
        line_head = false;
    }
    out
}

fn layout_mwnd_text_vertical(
    text: &str,
    font_cell: i32,
    space_y: i32,
    column_w: i32,
    max_w: u32,
    max_h: u32,
) -> Vec<MwndPlacedChar> {
    let full_cell_h = font_cell.max(1);
    let half_cell_h = ((font_cell + 1) / 2).max(1);
    let max_w = max_w as i32;
    let max_h = max_h as i32;
    let mut out = Vec::new();
    // Siglus starts a vertical run at x=0 and moves later columns toward
    // negative x.  Store the same right-to-left order inside a positive image.
    let mut x = (max_w - full_cell_h).max(0);
    let mut y = 0i32;

    for ch in text.chars() {
        match ch {
            '\r' => continue,
            '\n' | '\u{0007}' => {
                x -= column_w.max(1);
                y = 0;
                if x + full_cell_h <= 0 {
                    break;
                }
                continue;
            }
            '\t' => {
                y += (full_cell_h + space_y).max(1) * 2;
                continue;
            }
            _ => {}
        }

        let cell_h = if is_hankaku(ch) {
            half_cell_h
        } else {
            full_cell_h
        };
        let advance = (cell_h + space_y).max(1);
        if y > 0 && y + cell_h > max_h {
            x -= column_w.max(1);
            y = 0;
            if x + full_cell_h <= 0 {
                break;
            }
        }
        out.push(MwndPlacedChar {
            ch,
            x,
            y,
            cell_w: full_cell_h,
        });
        y += advance;
    }
    out
}

fn is_siglus_indent_open(ch: char) -> bool {
    matches!(ch, '「' | '『' | '（')
}

fn is_siglus_indent_close(ch: char) -> bool {
    matches!(ch, '」' | '』' | '）')
}

fn is_siglus_forbidden_line_head(ch: char) -> bool {
    matches!(
        ch,
        '、' | '。'
            | '，'
            | '．'
            | '・'
            | '：'
            | '；'
            | '？'
            | '！'
            | '」'
            | '』'
            | '）'
            | '］'
            | '｝'
            | '〉'
            | '》'
            | '】'
            | '〕'
            | 'ぁ'
            | 'ぃ'
            | 'ぅ'
            | 'ぇ'
            | 'ぉ'
            | 'っ'
            | 'ゃ'
            | 'ゅ'
            | 'ょ'
            | 'ゎ'
            | 'ァ'
            | 'ィ'
            | 'ゥ'
            | 'ェ'
            | 'ォ'
            | 'ッ'
            | 'ャ'
            | 'ュ'
            | 'ョ'
            | 'ヮ'
            | 'ｰ'
            | 'ー'
            | '～'
            | '…'
            | '‥'
    )
}

fn body_face_extent(style: TextStyle) -> i32 {
    if style.bold { 1 } else { 0 }
}

/// C_elm_object::string_frame places the 3x3/4x4 fuchi texture one pixel
/// up-left after Cfont_copy::alphablend_copy_face expands it down-right.
/// Keeping that placement separate from the face extent is important: the
/// original outline is centred around the body, not shifted to its lower-right.
fn fuchi_texture_offset() -> i32 {
    -1
}

fn fuchi_face_extent(style: TextStyle) -> i32 {
    if style.bold { 3 } else { 2 }
}

fn shadow_face_extent(style: TextStyle) -> i32 {
    if style.shadow_mode == TNM_FONT_SHADOW_MODE_FUCHI_SHADOW {
        if style.bold { 3 } else { 2 }
    } else {
        body_face_extent(style)
    }
}

fn shadow_offset_for_style(size: i32, style: TextStyle) -> i32 {
    let size = size.max(0);
    let t = (size as f32 / 32.0).clamp(0.0, 1.0);
    if style.shadow_mode == TNM_FONT_SHADOW_MODE_FUCHI_SHADOW {
        // C++: linear_limit(size, 0, 0.5, 32, 1.5) - 1.0.
        (0.5 + (1.5 - 0.5) * t - 1.0).round() as i32
    } else {
        // C++: linear_limit(size, 0, 0.5, 32, 2.0).
        (0.5 + (2.0 - 0.5) * t).round().max(1.0) as i32
    }
}

fn text_effect_padding(size: i32, style: TextStyle) -> i32 {
    let mut pad = body_face_extent(style);
    if style.fuchi {
        pad = pad.max(fuchi_face_extent(style));
    }
    if style.shadow {
        pad = pad.max(
            shadow_offset_for_style(size, style)
                .max(0)
                .saturating_add(shadow_face_extent(style)),
        );
    }
    pad + 1
}

pub fn is_hankaku(ch: char) -> bool {
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
            blend_rgba_pixel(
                rgba, w, px as u32, py as u32, color.0, color.1, color.2, src_a,
            );
        }
    }
}

/// GetGlyphOutline(..., GGO_GRAY8_BITMAP, ...) produces a 65-level coverage
/// bitmap.  tona3's Cfont_copy::gray8_copy then indexes a 66-entry table and
/// computes alpha as `(source_alpha * level) / 65`.  Quantising the portable
/// rasteriser through the same table preserves the original edge-alpha
/// staircase instead of feeding an unrelated 256-level mask to the renderer.
fn gdi_gray8_alpha_from_coverage(coverage: f32) -> u8 {
    let level = (coverage.clamp(0.0, 1.0) * 64.0).round().clamp(0.0, 64.0) as u16;
    ((255u16 * level) / 65) as u8
}

fn draw_glyph_bitmap_face(
    rgba: &mut [u8],
    w: u32,
    h: u32,
    x: i32,
    y: i32,
    glyph_w: usize,
    glyph_h: usize,
    glyph: &[u8],
    face_extent: i32,
    color: (u8, u8, u8, u8),
) {
    for oy in 0..=face_extent.max(0) {
        for ox in 0..=face_extent.max(0) {
            draw_glyph_bitmap(rgba, w, h, x + ox, y + oy, glyph_w, glyph_h, glyph, color);
        }
    }
}

fn blend_rgba_pixel(rgba: &mut [u8], w: u32, x: u32, y: u32, sr: u8, sg: u8, sb: u8, sa: u8) {
    let idx = ((y * w + x) * 4) as usize;
    // Cfont_copy uses 32-bit `int` work tables/arithmetic. The destination
    // term can reach 255^3, so u16 is non-original and overflows in debug
    // builds for ordinary coloured glyph overlaps.
    let da = rgba[idx + 3] as u32;
    let sa_u = sa as u32;
    let inv_sa = 255u32.saturating_sub(sa_u);
    // Cfont_copy's alpha table uses integer truncation:
    //   src + dst - src * dst / 255
    // Do not round here; repeated face copies otherwise diverge from tona3.
    let out_a = sa_u + da - (sa_u * da) / 255;
    if out_a == 0 {
        rgba[idx] = 0;
        rgba[idx + 1] = 0;
        rgba[idx + 2] = 0;
        rgba[idx + 3] = 0;
        return;
    }
    let blend = |src: u8, dst: u8| -> u8 {
        let src_p = src as u32 * sa_u;
        let dst_p = dst as u32 * da * inv_sa / 255;
        ((src_p + dst_p + out_a / 2) / out_a).min(255) as u8
    };
    rgba[idx] = blend(sr, rgba[idx]);
    rgba[idx + 1] = blend(sg, rgba[idx + 1]);
    rgba[idx + 2] = blend(sb, rgba[idx + 2]);
    rgba[idx + 3] = out_a.min(255) as u8;
}

/// Line count and pitch of the layout [`render_text_ab_glyph_rgba`] produces.
///
/// Mirrors that function's line breaking exactly (explicit `\n`, automatic wrap at
/// `max_w`, `line_height = scaled.height() + scaled.line_gap()`) without rasterizing,
/// so a caller that draws text as one image can still address individual rows.
pub fn text_line_metrics_ab_glyph(
    font: &FontArc,
    text: &str,
    font_px: f32,
    max_w: u32,
    max_h: u32,
) -> (usize, f32) {
    text_line_metrics_resolved(
        ResolvedFonts {
            primary: font,
            cache: None,
        },
        text,
        font_px,
        max_w,
        max_h,
    )
}

fn text_line_metrics_resolved(
    font: ResolvedFonts<'_>,
    text: &str,
    font_px: f32,
    max_w: u32,
    max_h: u32,
) -> (usize, f32) {
    let scaled = font.as_scaled(PxScale::from(font_px.max(1.0)));
    let ascent = scaled.ascent().max(1.0);
    let line_height = (scaled.height() + scaled.line_gap()).max(1.0);
    if text.is_empty() || max_w == 0 || max_h == 0 {
        return (0, line_height);
    }

    let mut lines = 1usize;
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
                lines += 1;
                continue;
            }
            '\t' => {
                x += scaled.h_advance(scaled.glyph_id(' ')).max(0.0) * 2.0;
                continue;
            }
            _ => {}
        }

        let advance = font.advance(ch, PxScale::from(font_px.max(1.0)));
        if x > 0.0 && x + advance > max_w as f32 {
            x = 0.0;
            baseline_y += line_height;
            if baseline_y - ascent >= max_h as f32 {
                break;
            }
            lines += 1;
        }
        x += advance;
    }
    (lines, line_height)
}

fn render_text_ab_glyph_rgba(
    font: ResolvedFonts<'_>,
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

        let advance = font.advance(ch, PxScale::from(font_px.max(1.0)));
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
        center_x: 0,
        center_y: 0,
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

pub fn normalized_font_name(name: &str) -> String {
    normalize_font_name_for_match(name.trim_start_matches('@'))
}

fn normalize_font_name_for_match(name: &str) -> String {
    name.chars()
        .filter(|ch| !ch.is_whitespace() && *ch != '-' && *ch != '_' && *ch != '.')
        .flat_map(|ch| ch.to_lowercase())
        .collect()
}

/// A font file seen by any `FontCache`: the normalized names of each face,
/// and the file's bytes once a face of it is used.
struct FontFileInfo {
    face_names: Vec<Vec<String>>,
    data: Option<&'static [u8]>,
}

/// Font files by path, for the whole process. Matching a requested name
/// needs only the face names, so a file is not read again for each new
/// request (a 22 MiB TTC was read on every font switch, a second or more
/// on PS Vita). Used files stay loaded once and every face and cache
/// shares them: each switch used to keep its own copy, hundreds of MiB.
static FONT_FILES: OnceLock<Mutex<std::collections::HashMap<PathBuf, FontFileInfo>>> =
    OnceLock::new();

/// The data of the font file at `path` and its face matching
/// `normalized_name` (the first face for an empty name).
fn font_file_face(path: &Path, normalized_name: &str) -> Option<(&'static [u8], u32)> {
    let files = FONT_FILES.get_or_init(|| Mutex::new(std::collections::HashMap::new()));
    let mut files = files.lock().ok()?;
    if let Some(info) = files.get_mut(path) {
        let face_index = matching_face_in_names(&info.face_names, normalized_name)?;
        let data = match info.data {
            Some(data) => data,
            None => {
                let data: &'static [u8] = Box::leak(
                    crate::resource::read_file_bytes(path)
                        .ok()?
                        .into_boxed_slice(),
                );
                info.data = Some(data);
                data
            }
        };
        return Some((data, face_index));
    }
    let bytes = crate::resource::read_file_bytes(path).ok()?;
    let face_names = font_face_names(&bytes);
    let face_index = matching_face_in_names(&face_names, normalized_name);
    // Only files that are used are kept.
    let data = face_index.map(|_| &*Box::leak(bytes.into_boxed_slice()));
    files.insert(path.to_path_buf(), FontFileInfo { face_names, data });
    Some((data?, face_index?))
}

/// The normalized engine-visible names (family, full, typographic family,
/// WWS family, PostScript) of every face in a font file or collection.
fn font_face_names(bytes: &[u8]) -> Vec<Vec<String>> {
    let face_count = ttf_parser::fonts_in_collection(bytes).unwrap_or(1);
    (0..face_count)
        .map(|face_index| {
            let Ok(face) = ttf_parser::Face::parse(bytes, face_index) else {
                return Vec::new();
            };
            face.names()
                .into_iter()
                .filter(|name| {
                    matches!(
                        name.name_id,
                        ttf_parser::name::name_id::FAMILY
                            | ttf_parser::name::name_id::FULL_NAME
                            | ttf_parser::name::name_id::POST_SCRIPT_NAME
                            | ttf_parser::name::name_id::TYPOGRAPHIC_FAMILY
                            | ttf_parser::name::name_id::WWS_FAMILY
                            | ttf_parser::name::name_id::COMPATIBLE_FULL
                    )
                })
                .filter_map(|name| name.to_string())
                .map(|value| normalize_font_name_for_match(value.trim_start_matches('@')))
                .collect()
        })
        .collect()
}

fn matching_face_in_names(face_names: &[Vec<String>], normalized_name: &str) -> Option<u32> {
    if normalized_name.is_empty() {
        return Some(0);
    }
    let mut partial_match = None;
    for (face_index, names) in face_names.iter().enumerate() {
        let face_index = face_index as u32;
        for key in names {
            if key == normalized_name {
                return Some(face_index);
            }
            if !key.is_empty()
                && partial_match.is_none()
                && (key.contains(normalized_name) || normalized_name.contains(key.as_str()))
            {
                partial_match = Some(face_index);
            }
        }
    }
    partial_match
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

fn project_font_dirs(project_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![project_dir.join("font"), project_dir.join("fonts")];
    if let Ok(exe) = std::env::current_exe()
        && let Some(exe_dir) = exe.parent()
    {
        dirs.push(exe_dir.join("font"));
        dirs.push(exe_dir.join("fonts"));
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dirs.push(manifest_dir.join("assets").join("font"));
    dirs.push(manifest_dir.join("assets").join("fonts"));
    dirs
}

fn platform_font_candidates_for_name(name: &str) -> Vec<PathBuf> {
    let key = normalize_font_name_for_match(name.trim_start_matches('@'));
    let mut out = Vec::new();

    #[cfg(target_os = "windows")]
    {
        let windir = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
        let fonts = windir.join("Fonts");
        let names: &[&str] = if key.contains("msgothic") || key.contains("ｍｓゴシック") {
            &["msgothic.ttc", "msgothic.ttf"]
        } else if key.contains("msmincho") || key.contains("ｍｓ明朝") {
            &["msmincho.ttc", "msmincho.ttf"]
        } else if key.contains("meiryo") || key.contains("メイリオ") {
            &["meiryo.ttc", "meiryob.ttc"]
        } else if key.contains("yugoth") || key.contains("游ゴシック") {
            &["YuGothM.ttc", "YuGothR.ttc"]
        } else if key.contains("yumin") || key.contains("游明朝") {
            &["YuMincho.ttc"]
        } else {
            &[]
        };
        out.extend(names.iter().map(|file| fonts.join(file)));
    }

    #[cfg(target_os = "macos")]
    {
        let dirs = [
            PathBuf::from("/System/Library/Fonts"),
            PathBuf::from("/Library/Fonts"),
        ];
        let names: &[&str] = if key.contains("gothic")
            || key.contains("ゴシック")
            || key.contains("meiryo")
            || key.contains("メイリオ")
        {
            &[
                "ヒラギノ角ゴシック W3.ttc",
                "ヒラギノ角ゴシック W6.ttc",
                "Hiragino Sans GB.ttc",
            ]
        } else if key.contains("mincho") || key.contains("明朝") {
            &["ヒラギノ明朝 ProN.ttc", "Hiragino Mincho ProN.ttc"]
        } else {
            &[]
        };
        for dir in dirs {
            out.extend(names.iter().map(|file| dir.join(file)));
        }
    }

    #[cfg(target_os = "linux")]
    {
        let names: &[&str] = if key.contains("mincho") || key.contains("明朝") {
            &[
                "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc",
                "/usr/share/fonts/truetype/dejavu/DejaVuSerif.ttf",
            ]
        } else {
            &[
                "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            ]
        };
        out.extend(names.iter().map(|name| PathBuf::from(*name)));
    }

    out
}

fn platform_font_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();

    #[cfg(target_os = "android")]
    {
        out.push(PathBuf::from("/system/fonts/NotoSansCJK-Regular.ttc"));
        out.push(PathBuf::from("/system/fonts/NotoSerifCJK-Regular.ttc"));
    }

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
        out.push(PathBuf::from(
            "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
        ));
        out.push(PathBuf::from(
            "/System/Library/Fonts/ヒラギノ角ゴシック W4.ttc",
        ));
        out.push(PathBuf::from(
            "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
        ));
        out.push(PathBuf::from(
            "/System/Library/Fonts/Supplemental/Osaka.ttf",
        ));
    }

    #[cfg(any(target_os = "linux", target_os = "freebsd", target_os = "android"))]
    {
        out.push(PathBuf::from(
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
        ));
        out.push(PathBuf::from(
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
        ));
        out.push(PathBuf::from(
            "/usr/share/fonts/opentype/noto/NotoSansCJKjp-Regular.otf",
        ));
        out.push(PathBuf::from(
            "/usr/share/fonts/truetype/fonts-japanese-gothic.ttf",
        ));
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

#[cfg(test)]
mod fallback_tests {
    use super::*;

    fn fixture() -> (FontCache, FontArc) {
        let full = FontArc::from(FontRef::try_from_slice(DEFAULT_FONT_BYTES).unwrap());
        assert_ne!(full.glyph_id('中').0, 0);
        // Reuse the repository font's outlines, with a synthetic cmap exposing
        // only 'A'. No external or commercial test font is needed.
        let mut bytes = DEFAULT_FONT_BYTES.to_vec();
        let count = u16::from_be_bytes(bytes[4..6].try_into().unwrap()) as usize;
        let record = (0..count)
            .map(|i| 12 + i * 16)
            .find(|&at| &bytes[at..at + 4] == b"cmap")
            .unwrap();
        let offset =
            u32::from_be_bytes(bytes[record + 8..record + 12].try_into().unwrap()) as usize;
        let mut cmap = Vec::new();
        for value in [0u16, 1, 3, 10] {
            cmap.extend_from_slice(&value.to_be_bytes());
        }
        cmap.extend_from_slice(&12u32.to_be_bytes());
        cmap.extend_from_slice(&12u16.to_be_bytes());
        cmap.extend_from_slice(&0u16.to_be_bytes());
        for value in [28u32, 0, 1, 65, 65, full.glyph_id('A').0 as u32] {
            cmap.extend_from_slice(&value.to_be_bytes());
        }
        bytes[offset..offset + cmap.len()].copy_from_slice(&cmap);
        bytes[record + 12..record + 16].copy_from_slice(&(cmap.len() as u32).to_be_bytes());
        let primary = FontArc::try_from_vec(bytes).unwrap();
        assert_eq!(primary.glyph_id('中').0, 0);
        let mut cache = FontCache::new();
        cache.font = Some(primary);
        cache.fallback_files = OnceLock::from(vec![FallbackFontFile {
            path: PathBuf::from("test:fallback"),
            faces: OnceLock::from(vec![full.clone()]),
        }]);
        (cache, full)
    }

    #[test]
    fn fallback_preserves_present_glyphs_and_repairs_missing_outlines() {
        let (cache, full) = fixture();
        let fonts = cache.resolved_fonts().unwrap();
        assert!(std::ptr::eq(fonts.for_char('A'), fonts.primary));
        assert_ne!(fonts.for_char('中').glyph_id('中').0, 0);
        let expected = ResolvedFonts {
            primary: &full,
            cache: None,
        };
        for vertical in [false, true] {
            for ch in ['A', '中'] {
                let actual = rasterize_ab_glyph_oriented(fonts, ch, 32.0, vertical);
                let wanted = rasterize_ab_glyph_oriented(expected, ch, 32.0, vertical);
                assert_eq!(
                    (actual.width, actual.height, actual.xmin, actual.ymin),
                    (wanted.width, wanted.height, wanted.xmin, wanted.ymin)
                );
                assert_eq!(actual.bitmap, wanted.bitmap);
            }
        }
        assert!(std::ptr::eq(fonts.for_char('\u{10ffff}'), fonts.primary));
        assert_eq!(
            cache.glyph_fallbacks.lock().unwrap().get(&'\u{10ffff}'),
            Some(&None)
        );
    }

    #[test]
    fn mixed_text_uses_the_same_fallback_for_measurement_and_rendering() {
        let (cache, full) = fixture();
        let expected = ResolvedFonts {
            primary: &full,
            cache: None,
        };
        let actual = cache.render_text_rgba("A中A中", 32.0, 55, 160).unwrap();
        let wanted = render_text_ab_glyph_rgba(expected, "A中A中", 32.0, 55, 160).unwrap();
        assert_eq!(actual.rgba, wanted.rgba);
        assert_eq!(
            cache.text_line_metrics("A中A中", 32.0, 55, 160),
            text_line_metrics_ab_glyph(&full, "A中A中", 32.0, 55, 160)
        );
        let actual = render_editbox_rgba(
            cache.resolved_fonts(),
            "A中",
            0,
            None,
            "",
            None,
            None,
            0,
            false,
            32.0,
            160,
            48,
        )
        .unwrap();
        let wanted = render_editbox_rgba(
            Some(expected),
            "A中",
            0,
            None,
            "",
            None,
            None,
            0,
            false,
            32.0,
            160,
            48,
        )
        .unwrap();
        assert_eq!(actual.rgba, wanted.rgba);
    }

    #[test]
    fn changing_game_discards_resolved_faces_and_missing_character_cache() {
        let (mut cache, _) = fixture();
        cache.project_dir = Some(PathBuf::from("first-game"));
        cache.resolved_fonts().unwrap().for_char('中');
        assert!(!cache.glyph_fallbacks.lock().unwrap().is_empty());
        cache.load_for_project_named(Path::new("different-game"), "default");
        assert!(cache.fallback_files.get().is_none());
        assert!(cache.glyph_fallbacks.lock().unwrap().is_empty());
    }
}

#[cfg(test)]
mod font_shadow_mode_tests {
    use super::*;

    #[test]
    fn original_four_state_shadow_mode_mapping() {
        assert_eq!(font_shadow_mode_flags(0), (false, false));
        assert_eq!(font_shadow_mode_flags(1), (true, false));
        assert_eq!(font_shadow_mode_flags(2), (false, true));
        assert_eq!(font_shadow_mode_flags(3), (true, true));
    }

    #[test]
    fn invalid_shadow_modes_are_clamped_like_runtime_config() {
        assert_eq!(font_shadow_mode_flags(-1), (false, false));
        assert_eq!(font_shadow_mode_flags(99), (true, true));
    }

    #[test]
    fn gdi_gray8_uses_the_original_65_level_alpha_table() {
        assert_eq!(gdi_gray8_alpha_from_coverage(0.0), 0);
        assert_eq!(gdi_gray8_alpha_from_coverage(1.0), 251);
        assert_eq!(gdi_gray8_alpha_from_coverage(32.0 / 64.0), 125);
    }

    #[test]
    fn normal_fuchi_is_centered_around_the_body_pixel() {
        let mut rgba = vec![0u8; 5 * 5 * 4];
        draw_glyph_bitmap_face(
            &mut rgba,
            5,
            5,
            2 + fuchi_texture_offset(),
            2 + fuchi_texture_offset(),
            1,
            1,
            &[255],
            2,
            (0, 0, 0, 255),
        );

        for y in 0..5usize {
            for x in 0..5usize {
                let alpha = rgba[(y * 5 + x) * 4 + 3];
                let expected = (1..=3).contains(&x) && (1..=3).contains(&y);
                assert_eq!(alpha != 0, expected, "unexpected fuchi pixel at ({x},{y})");
            }
        }
    }

    #[test]
    fn repeated_face_alpha_uses_tona3_integer_truncation() {
        let mut rgba = vec![0u8; 4];
        blend_rgba_pixel(&mut rgba, 1, 0, 0, 0, 0, 0, 128);
        blend_rgba_pixel(&mut rgba, 1, 0, 0, 0, 0, 0, 128);
        assert_eq!(rgba[3], 192);
    }

    #[test]
    fn blend_over_opaque_destination_does_not_overflow() {
        // Semi-transparent glyph blended over an already-opaque pixel:
        // dst * da * inv_sa = 255 * 255 * 127 ~= 8.3M, which overflowed the
        // u16 intermediate (`attempt to multiply with overflow` in debug).
        let mut rgba = vec![255u8; 4]; // opaque destination
        blend_rgba_pixel(&mut rgba, 1, 0, 0, 255, 255, 255, 128);
        // out_a = 128 + 255 - 128*255/255 = 255; blend -> 255
        assert_eq!(rgba[0], 255);
        assert_eq!(rgba[3], 255);
    }

    #[test]
    fn coloured_glyph_blend_uses_wide_original_integer_arithmetic() {
        let mut rgba = [255, 220, 192, 255];
        blend_rgba_pixel(&mut rgba, 1, 0, 0, 80, 160, 240, 128);
        assert_eq!(rgba, [167, 190, 216, 255]);
    }
}
