use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::assets::{load_image_any, RgbaImage};
use anyhow::{bail, Context, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageId(pub u32);

impl ImageId {
    pub fn index(self) -> usize {
        self.0 as usize
    }
}

#[derive(Debug, Clone)]
struct ImageKey {
    path: PathBuf,
    frame_index: usize,
}

impl PartialEq for ImageKey {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path && self.frame_index == other.frame_index
    }
}

impl Eq for ImageKey {}

impl Hash for ImageKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
        self.frame_index.hash(state);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct G00ComposePart {
    file_name: String,
    x: i32,
    y: i32,
    cut_no: i32,
    blend_type: i32,
}

fn normalized_g00_composite_descriptor(raw: &str) -> String {
    // Original tnm_load_pct_d3d_sub_split_file_name() removes every ASCII
    // space before parsing and before the composed resource is cached.
    raw.chars().filter(|&ch| ch != ' ').collect()
}

fn parse_g00_composite_descriptor(raw: &str) -> Result<Vec<G00ComposePart>> {
    if !raw.contains('|') {
        bail!("not a composed g00 descriptor: {raw}");
    }

    let compact = normalized_g00_composite_descriptor(raw);
    let bytes = compact.as_bytes();
    let mut pos = 0usize;
    let mut parts = Vec::new();

    loop {
        let name_start = pos;
        while pos < bytes.len() && bytes[pos] != b'(' && bytes[pos] != b'|' {
            pos += 1;
        }

        let mut part = G00ComposePart {
            file_name: compact[name_start..pos].to_string(),
            x: 0,
            y: 0,
            cut_no: 0,
            blend_type: 0,
        };

        if pos < bytes.len() && bytes[pos] == b'(' {
            let param_start = pos + 1;
            let Some(rel_close) = compact[param_start..].find(')') else {
                bail!("unterminated composed g00 parameters: {raw}");
            };
            let close = param_start + rel_close;
            let params: Vec<&str> = compact[param_start..close].split(',').collect();
            if params.len() < 2 {
                bail!("composed g00 parameters require x,y: {raw}");
            }
            part.x = params[0]
                .parse::<i32>()
                .with_context(|| format!("invalid composed g00 x in {raw}"))?;
            part.y = params[1]
                .parse::<i32>()
                .with_context(|| format!("invalid composed g00 y in {raw}"))?;
            for param in params.iter().skip(2) {
                if let Some(value) = param.strip_prefix("blend=") {
                    part.blend_type = value
                        .parse::<i32>()
                        .with_context(|| format!("invalid composed g00 blend in {raw}"))?;
                } else {
                    part.cut_no = param
                        .parse::<i32>()
                        .with_context(|| format!("invalid composed g00 cut in {raw}"))?;
                }
            }
            pos = close + 1;
        }

        parts.push(part);
        if pos == bytes.len() {
            break;
        }
        if bytes[pos] != b'|' {
            bail!("unexpected character in composed g00 descriptor: {raw}");
        }
        pos += 1;
        if pos == bytes.len() {
            // The original parser produces an empty final entry, which is then
            // rejected because only the first composed entry may omit a file.
            parts.push(G00ComposePart {
                file_name: String::new(),
                x: 0,
                y: 0,
                cut_no: 0,
                blend_type: 0,
            });
            break;
        }
    }

    if parts.is_empty() {
        bail!("empty composed g00 descriptor");
    }
    for (index, part) in parts.iter().enumerate().skip(1) {
        if part.file_name.is_empty() {
            bail!("composed g00 entry {index} has no file name");
        }
    }
    Ok(parts)
}

pub(crate) fn g00_composite_component_names(raw: &str) -> Option<Vec<String>> {
    if !raw.contains('|') {
        return None;
    }
    parse_g00_composite_descriptor(raw).ok().map(|parts| {
        parts
            .into_iter()
            .filter_map(|part| (!part.file_name.is_empty()).then_some(part.file_name))
            .collect()
    })
}

#[derive(Debug)]
pub struct ImageManager {
    project_dir: PathBuf,
    current_append_dir: String,
    key_to_id: HashMap<ImageKey, ImageId>,
    /// Original Tona3 keeps one C_d3d_album per resolved G00 resource.  Keep
    /// the complete cut -> ImageId table alive for the same resource lifetime
    /// so PATNO/GAN changes never decode the file again.
    g00_album_to_ids: HashMap<PathBuf, Vec<ImageId>>,
    composite_to_id: HashMap<(String, String), ImageId>,
    solid_to_id: HashMap<(u8, u8, u8, u8), ImageId>,
    images: Vec<ImageEntry>,
}

#[derive(Debug, Clone)]
struct ImageEntry {
    img: Arc<RgbaImage>,
    version: u64,
}

#[derive(Debug, Clone)]
pub struct DebugImageInfo {
    pub id: ImageId,
    pub width: u32,
    pub height: u32,
    pub version: u64,
    pub source_path: Option<PathBuf>,
    pub frame_index: Option<usize>,
}

fn compose_g00_cut(dst: &mut RgbaImage, src: &RgbaImage, x: i32, y: i32, blend_type: i32) {
    if dst.width == 0 || dst.height == 0 || src.width == 0 || src.height == 0 {
        return;
    }

    let dst_left = x.max(0) as u32;
    let dst_top = y.max(0) as u32;
    let src_left = x.saturating_neg().max(0) as u32;
    let src_top = y.saturating_neg().max(0) as u32;
    if dst_left >= dst.width || dst_top >= dst.height || src_left >= src.width || src_top >= src.height {
        return;
    }
    let width = (src.width - src_left).min(dst.width - dst_left);
    let height = (src.height - src_top).min(dst.height - dst_top);

    for row in 0..height {
        for col in 0..width {
            let si = (((src_top + row) * src.width + src_left + col) * 4) as usize;
            let di = (((dst_top + row) * dst.width + dst_left + col) * 4) as usize;
            let sr = src.rgba[si] as i64;
            let sg = src.rgba[si + 1] as i64;
            let sb = src.rgba[si + 2] as i64;
            let sa = src.rgba[si + 3] as i64;
            if sa == 0 {
                continue;
            }

            let dr = dst.rgba[di] as i64;
            let dg = dst.rgba[di + 1] as i64;
            let db = dst.rgba[di + 2] as i64;
            let da = dst.rgba[di + 3] as i64;
            // Tona3 only has the opaque-source memcpy fast path in the
            // normal-alpha branch. Add/multiply must still combine an opaque
            // source with the destination color. A transparent destination can
            // be copied for every blend mode because each equation reduces to
            // the source pixel in that case.
            if da == 0 || (sa == 255 && !matches!(blend_type, 1 | 3)) {
                dst.rgba[di..di + 4].copy_from_slice(&src.rgba[si..si + 4]);
                continue;
            }

            let ra = sa + da - (sa * da / 255);
            if ra <= 0 {
                continue;
            }
            let blend_channel = |sc: i64, dc: i64| -> i64 {
                match blend_type {
                    // Tona3's composed texture path has dedicated add and
                    // multiply equations. Every other enum value follows the
                    // normal alpha path in f_draw_alphablend().
                    1 => {
                        let mixed = (sc + dc).min(255);
                        (sa * da * mixed
                            + sa * (255 - da) * sc
                            + (255 - sa) * da * dc)
                            / ra
                            / 255
                    }
                    3 => {
                        let mixed = sc * dc / 255;
                        (sa * da * mixed
                            + sa * (255 - da) * sc
                            + (255 - sa) * da * dc)
                            / ra
                            / 255
                    }
                    _ => {
                        let work1 = (255 - sa) * da;
                        let work2 = 255 * sa * sc;
                        ((work2 + work1 * dc) >> 8) / ra
                    }
                }
            };

            dst.rgba[di] = blend_channel(sr, dr).clamp(0, 255) as u8;
            dst.rgba[di + 1] = blend_channel(sg, dg).clamp(0, 255) as u8;
            dst.rgba[di + 2] = blend_channel(sb, db).clamp(0, 255) as u8;
            dst.rgba[di + 3] = ra.clamp(0, 255) as u8;
        }
    }
}

impl ImageManager {
    pub fn new(project_dir: PathBuf) -> Self {
        Self {
            project_dir,
            current_append_dir: String::new(),
            key_to_id: HashMap::new(),
            g00_album_to_ids: HashMap::new(),
            composite_to_id: HashMap::new(),
            solid_to_id: HashMap::new(),
            images: Vec::new(),
        }
    }

    pub fn project_dir(&self) -> &Path {
        &self.project_dir
    }

    pub fn current_append_dir(&self) -> &str {
        &self.current_append_dir
    }

    pub fn set_current_append_dir(&mut self, append_dir: impl Into<String>) {
        self.current_append_dir = append_dir.into();
    }

    pub fn get(&self, id: ImageId) -> Option<&Arc<RgbaImage>> {
        self.images.get(id.index()).map(|e| &e.img)
    }

    pub fn get_entry(&self, id: ImageId) -> Option<(&Arc<RgbaImage>, u64)> {
        self.images.get(id.index()).map(|e| (&e.img, e.version))
    }

    /// Create a 1x1 solid RGBA image and return its image id.
    ///
    /// This is used for UI placeholders (e.g. message window background) until
    /// full UI skinning is implemented.
    pub fn solid_rgba(&mut self, rgba: (u8, u8, u8, u8)) -> ImageId {
        if let Some(id) = self.solid_to_id.get(&rgba) {
            return *id;
        }
        let img = RgbaImage {
            width: 1,
            height: 1,
            center_x: 0,
            center_y: 0,
            rgba: vec![rgba.0, rgba.1, rgba.2, rgba.3],
        };
        let id = ImageId(self.images.len() as u32);
        self.images.push(ImageEntry {
            img: Arc::new(img),
            version: 0,
        });
        self.solid_to_id.insert(rgba, id);
        id
    }

    /// Load a BG resource by name (Siglus policy: g00/ then bg/, with extension fallback).
    ///
    /// BG is not animated in our current bring-up, so frame index is always 0.
    pub fn load_bg(&mut self, name: &str) -> Result<ImageId> {
        let (path, _ty) = crate::resource::find_bg_image_with_append_dir(
            &self.project_dir,
            &self.current_append_dir,
            name,
        )
        .with_context(|| format!("find bg resource {name}"))?;
        self.load_file(&path, 0)
    }

    /// Load a BG resource with an explicit frame index (kept for compatibility).
    pub fn load_bg_frame(&mut self, name: &str, frame_index: usize) -> Result<ImageId> {
        let (path, _ty) = crate::resource::find_bg_image_with_append_dir(
            &self.project_dir,
            &self.current_append_dir,
            name,
        )
        .with_context(|| format!("find bg resource {name}"))?;
        self.load_file(&path, frame_index)
    }

    /// Load an image restricted to the `g00/` directory (with extension fallback).
    ///
    /// Used for CHR / sprite image loading.
    pub fn load_g00(&mut self, name: &str, frame_index: u32) -> Result<ImageId> {
        if name.contains('|') {
            if frame_index != 0 {
                bail!("composed g00 has one texture; invalid frame index {frame_index}");
            }
            return self.load_g00_composed(name);
        }
        let (path, _ty) = crate::resource::find_g00_image_with_append_dir(
            &self.project_dir,
            &self.current_append_dir,
            name,
        )
        .with_context(|| format!("find g00 resource {name}"))?;
        self.load_file(&path, frame_index as usize)
    }

    fn decode_composed_g00_part(&mut self, part: &G00ComposePart) -> Result<RgbaImage> {
        let (path, ty) = crate::resource::find_g00_image_with_append_dir(
            &self.project_dir,
            &self.current_append_dir,
            &part.file_name,
        )
        .with_context(|| format!("find composed g00 resource {}", part.file_name))?;
        if ty != crate::resource::PctType::G00 {
            bail!(
                "composed texture accepts g00 only: {} resolved as {}",
                part.file_name,
                ty.ext()
            );
        }

        let requested = if path.is_absolute() {
            path
        } else if crate::resource::resolve_game_file(&path)?.is_some() {
            path
        } else {
            self.project_dir.join(path)
        };
        let resolved = crate::resource::resolve_game_file(&requested)?
            .unwrap_or(requested);

        // Tona3 composes cuts from an already-loaded C_d3d_album. Preserve the
        // original clamp-to-last-cut behavior while reusing that same album
        // cache instead of decoding the G00 again for every component.
        let album = self.ensure_g00_album(&resolved)?;
        let max_index = album.len() - 1;
        let cut_no = part.cut_no.clamp(0, max_index as i32) as usize;
        let id = album[cut_no];
        self.get(id)
            .map(|img| (**img).clone())
            .with_context(|| format!("missing cached composed g00 image id={}", id.index()))
    }

    /// Load Siglus/Tona3's composed-G00 descriptor syntax:
    /// `base(x,y,cut,blend=n)|overlay(x,y,cut,blend=n)|...`.
    ///
    /// Tona3 creates one texture from the first cut and draws every later cut
    /// into that fixed-size texture. Coordinates are anchor-relative: each
    /// overlay is shifted by the base cut center minus the overlay cut center.
    pub fn load_g00_composed(&mut self, descriptor: &str) -> Result<ImageId> {
        let normalized = normalized_g00_composite_descriptor(descriptor);
        let cache_key = (self.current_append_dir.clone(), normalized.clone());
        if let Some(id) = self.composite_to_id.get(&cache_key) {
            return Ok(*id);
        }

        let parts = parse_g00_composite_descriptor(&normalized)?;
        let first = parts.first().context("composed g00 has no first entry")?;
        let mut composed = if first.file_name.is_empty() {
            if first.x <= 0 || first.y <= 0 {
                bail!("blank composed g00 base requires positive width,height");
            }
            let pixel_len = (first.x as usize)
                .checked_mul(first.y as usize)
                .and_then(|len| len.checked_mul(4))
                .context("blank composed g00 size overflow")?;
            RgbaImage {
                width: first.x as u32,
                height: first.y as u32,
                center_x: 0,
                center_y: 0,
                rgba: vec![0; pixel_len],
            }
        } else {
            self.decode_composed_g00_part(first)?
        };

        let base_center_x = composed.center_x;
        let base_center_y = composed.center_y;
        for part in parts.iter().skip(1) {
            let overlay = self.decode_composed_g00_part(part)?;
            let dst_x = part
                .x
                .saturating_add(base_center_x)
                .saturating_sub(overlay.center_x);
            let dst_y = part
                .y
                .saturating_add(base_center_y)
                .saturating_sub(overlay.center_y);
            compose_g00_cut(&mut composed, &overlay, dst_x, dst_y, part.blend_type);
        }

        let id = self.insert_image(composed);
        self.composite_to_id.insert(cache_key, id);
        Ok(id)
    }

    fn ensure_g00_album(&mut self, resolved: &Path) -> Result<&[ImageId]> {
        if !self.g00_album_to_ids.contains_key(resolved) {
            let bytes = crate::resource::read_file_bytes(resolved)
                .with_context(|| format!("read g00 album {:?}", resolved))?;
            let decoded = crate::assets::g00::decode_g00(&bytes)
                .with_context(|| format!("decode g00 album {:?}", resolved))?;
            if decoded.frames.is_empty() {
                bail!("g00 has no frames: {:?}", resolved);
            }

            let mut ids = Vec::with_capacity(decoded.frames.len());
            for (cut_index, img) in decoded.frames.into_iter().enumerate() {
                let id = self.insert_image(img);
                self.key_to_id.insert(
                    ImageKey {
                        path: resolved.to_path_buf(),
                        frame_index: cut_index,
                    },
                    id,
                );
                ids.push(id);
            }
            self.g00_album_to_ids.insert(resolved.to_path_buf(), ids);
        }

        self.g00_album_to_ids
            .get(resolved)
            .map(Vec::as_slice)
            .context("G00 album cache insertion failed")
    }

    /// Load an image from an explicit path (relative to project_dir if not absolute).
    pub fn load_file(&mut self, path: &Path, frame_index: usize) -> Result<ImageId> {
        let requested = if path.is_absolute() {
            path.to_path_buf()
        } else {
            // Relative paths are project-rooted (script resource names and
            // resource helper results). Join first so the lookup goes through
            // the pre-built absolute-path index instead of a doomed relative
            // query that always misses and falls back to a syscall.
            self.project_dir.join(path)
        };
        let resolved = crate::resource::resolve_game_file(&requested)?
            .unwrap_or(requested);

        let key = ImageKey {
            path: resolved.clone(),
            frame_index,
        };

        if let Some(id) = self.key_to_id.get(&key) {
            return Ok(*id);
        }

        let ext = resolved
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext == "g00" {
            // C_tnm_d3d_resource_manager::create_album_from_g00() loads and
            // caches the complete album once. GAN then only changes PATNO.
            // Do the same here: the first requested cut decodes the G00 once
            // and registers every cut; all later PATNO changes are O(1).
            let album = self.ensure_g00_album(&resolved)?;
            return album.get(frame_index).copied().with_context(|| {
                format!(
                    "g00 frame index out of range: {:?} index={} count={}",
                    resolved,
                    frame_index,
                    album.len()
                )
            });
        }

        let img = load_image_any(&resolved, frame_index)
            .with_context(|| format!("load image {:?}", resolved))?;
        let id = self.insert_image(img);
        self.key_to_id.insert(key, id);
        Ok(id)
    }

    /// Insert an already-decoded image into the manager and return a new ImageId.
    pub fn insert_image(&mut self, img: RgbaImage) -> ImageId {
        let id = ImageId(self.images.len() as u32);
        self.images.push(ImageEntry {
            img: Arc::new(img),
            version: 0,
        });
        id
    }

    pub fn insert_image_arc(&mut self, img: Arc<RgbaImage>) -> ImageId {
        let id = ImageId(self.images.len() as u32);
        self.images.push(ImageEntry { img, version: 0 });
        id
    }

    /// Replace an existing image in-place and bump its version.
    ///
    /// This allows the renderer to update the GPU texture without changing the ImageId.
    pub fn replace_image(&mut self, id: ImageId, img: RgbaImage) -> Result<()> {
        let Some(entry) = self.images.get_mut(id.index()) else {
            anyhow::bail!("replace_image: invalid ImageId {}", id.index());
        };
        entry.img = Arc::new(img);
        entry.version = entry.version.wrapping_add(1);
        Ok(())
    }

    pub fn replace_image_arc(&mut self, id: ImageId, img: Arc<RgbaImage>) -> Result<()> {
        let Some(entry) = self.images.get_mut(id.index()) else {
            anyhow::bail!("replace_image_arc: invalid ImageId {}", id.index());
        };
        entry.img = img;
        entry.version = entry.version.wrapping_add(1);
        Ok(())
    }

    pub fn debug_image_info(&self, id: ImageId) -> Option<DebugImageInfo> {
        let entry = self.images.get(id.index())?;
        let mut source_path = None;
        let mut frame_index = None;
        for (key, key_id) in &self.key_to_id {
            if *key_id == id {
                source_path = Some(key.path.clone());
                frame_index = Some(key.frame_index);
                break;
            }
        }
        Some(DebugImageInfo {
            id,
            width: entry.img.width,
            height: entry.img.height,
            version: entry.version,
            source_path,
            frame_index,
        })
    }
}

#[cfg(test)]
mod composed_g00_tests {
    use super::*;

    #[test]
    fn parses_siglus_composed_descriptor() {
        let parts = parse_g00_composite_descriptor(
            " bs3_rk2_base41(0, 0, 0) | bs3_rk2_face001(12, -3, 4, blend=1) ",
        )
        .expect("composed descriptor");
        assert_eq!(
            parts,
            vec![
                G00ComposePart {
                    file_name: "bs3_rk2_base41".to_string(),
                    x: 0,
                    y: 0,
                    cut_no: 0,
                    blend_type: 0,
                },
                G00ComposePart {
                    file_name: "bs3_rk2_face001".to_string(),
                    x: 12,
                    y: -3,
                    cut_no: 4,
                    blend_type: 1,
                },
            ]
        );
    }

    #[test]
    fn composed_cut_uses_base_and_overlay_centers() {
        let mut base = RgbaImage {
            width: 4,
            height: 4,
            center_x: 2,
            center_y: 2,
            rgba: vec![0; 4 * 4 * 4],
        };
        let overlay = RgbaImage {
            width: 2,
            height: 2,
            center_x: 1,
            center_y: 1,
            rgba: vec![255; 2 * 2 * 4],
        };

        // Tona3 draw position for (0,0) is base_center-overlay_center=(1,1).
        compose_g00_cut(&mut base, &overlay, 1, 1, 0);
        for y in 0..4usize {
            for x in 0..4usize {
                let alpha = base.rgba[(y * 4 + x) * 4 + 3];
                assert_eq!(alpha, if (1..3).contains(&x) && (1..3).contains(&y) { 255 } else { 0 });
            }
        }
        assert_eq!((base.center_x, base.center_y), (2, 2));
    }

    #[test]
    fn opaque_add_source_still_uses_tona_add_equation() {
        let mut base = RgbaImage {
            width: 1,
            height: 1,
            center_x: 0,
            center_y: 0,
            rgba: vec![20, 40, 60, 128],
        };
        let overlay = RgbaImage {
            width: 1,
            height: 1,
            center_x: 0,
            center_y: 0,
            rgba: vec![200, 100, 50, 255],
        };
        compose_g00_cut(&mut base, &overlay, 0, 0, 1);

        let expected = |sc: i64, dc: i64| {
            let sa = 255i64;
            let da = 128i64;
            let ra = 255i64;
            ((sa * da * (sc + dc).min(255)
                + sa * (255 - da) * sc
                + (255 - sa) * da * dc)
                / ra
                / 255) as u8
        };
        assert_eq!(
            base.rgba,
            vec![
                expected(200, 20),
                expected(100, 40),
                expected(50, 60),
                255,
            ]
        );
    }

    #[test]
    fn composed_alpha_matches_tona_integer_equation() {
        let mut base = RgbaImage {
            width: 1,
            height: 1,
            center_x: 0,
            center_y: 0,
            rgba: vec![20, 40, 60, 128],
        };
        let overlay = RgbaImage {
            width: 1,
            height: 1,
            center_x: 0,
            center_y: 0,
            rgba: vec![200, 100, 50, 128],
        };
        compose_g00_cut(&mut base, &overlay, 0, 0, 0);

        let sa = 128i64;
        let da = 128i64;
        let ra = sa + da - sa * da / 255;
        let expected = |sc: i64, dc: i64| {
            ((((255 * sa * sc) + ((255 - sa) * da * dc)) >> 8) / ra) as u8
        };
        assert_eq!(
            base.rgba,
            vec![
                expected(200, 20),
                expected(100, 40),
                expected(50, 60),
                ra as u8,
            ]
        );
    }
}
