use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::assets::{load_image_any, RgbaImage};
use anyhow::{Context, Result};

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

#[derive(Debug)]
pub struct ImageManager {
    project_dir: PathBuf,
    current_append_dir: String,
    key_to_id: HashMap<ImageKey, ImageId>,
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

impl ImageManager {
    pub fn new(project_dir: PathBuf) -> Self {
        Self {
            project_dir,
            current_append_dir: String::new(),
            key_to_id: HashMap::new(),
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
        let (path, _ty) = crate::resource::find_g00_image_with_append_dir(
            &self.project_dir,
            &self.current_append_dir,
            name,
        )
        .with_context(|| format!("find g00 resource {name}"))?;
        self.load_file(&path, frame_index as usize)
    }

    /// Load an image from an explicit path (relative to project_dir if not absolute).
    pub fn load_file(&mut self, path: &Path, frame_index: usize) -> Result<ImageId> {
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else if path.is_file() {
            // Resource lookup helpers return paths rooted at project_dir. When
            // project_dir itself is relative, those paths are still relative
            // (for example `testcase/g00/foo.g00`). Do not join project_dir a
            // second time; the original engine passes the resolved resource
            // path through unchanged after tnm_find_* succeeds.
            path.to_path_buf()
        } else {
            self.project_dir.join(path)
        };

        let key = ImageKey {
            path: resolved.clone(),
            frame_index,
        };

        if let Some(id) = self.key_to_id.get(&key) {
            return Ok(*id);
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
