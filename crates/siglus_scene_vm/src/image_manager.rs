use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use crate::assets::{load_image_any, RgbaImage};
use crate::resource::{find_bg_image, find_g00_image};

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
    key_to_id: HashMap<ImageKey, ImageId>,
    images: Vec<Arc<RgbaImage>>,
}

impl ImageManager {
    pub fn new(project_dir: PathBuf) -> Self {
        Self {
            project_dir,
            key_to_id: HashMap::new(),
            images: Vec::new(),
        }
    }

    pub fn project_dir(&self) -> &Path {
        &self.project_dir
    }

    pub fn get(&self, id: ImageId) -> Option<&Arc<RgbaImage>> {
        self.images.get(id.index())
    }

    /// Create a 1x1 solid RGBA image and return its image id.
    ///
    /// This is used for UI placeholders (e.g. message window background) until
    /// full UI skinning is implemented.
    pub fn solid_rgba(&mut self, rgba: (u8, u8, u8, u8)) -> ImageId {
        // We keep it simple: do not cache by color for now.
        let img = RgbaImage {
            width: 1,
            height: 1,
            rgba: vec![rgba.0, rgba.1, rgba.2, rgba.3],
        };
        let id = ImageId(self.images.len() as u32);
        self.images.push(Arc::new(img));
        id
    }

    /// Load a BG resource by name (Siglus policy: g00/ then bg/, with extension fallback).
    ///
    /// BG is not animated in our current bring-up, so frame index is always 0.
    pub fn load_bg(&mut self, name: &str) -> Result<ImageId> {
        let (path, _ty) = find_bg_image(&self.project_dir, name)
            .with_context(|| format!("find bg resource {name}"))?;
        self.load_file(&path, 0)
    }

    /// Load a BG resource with an explicit frame index (kept for compatibility).
    pub fn load_bg_frame(&mut self, name: &str, frame_index: usize) -> Result<ImageId> {
        let (path, _ty) = find_bg_image(&self.project_dir, name)
            .with_context(|| format!("find bg resource {name}"))?;
        self.load_file(&path, frame_index)
    }

    /// Load an image restricted to the `g00/` directory (with extension fallback).
    ///
    /// Used for CHR / sprite image loading.
    pub fn load_g00(&mut self, name: &str, frame_index: u32) -> Result<ImageId> {
        let (path, _ty) = find_g00_image(&self.project_dir, name)
            .with_context(|| format!("find g00 resource {name}"))?;
        self.load_file(&path, frame_index as usize)
    }

    /// Load an image from an explicit path (relative to project_dir if not absolute).
    pub fn load_file(&mut self, path: &Path, frame_index: usize) -> Result<ImageId> {
        let resolved = if path.is_absolute() {
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
        self.images.push(Arc::new(img));
        id
    }
}
