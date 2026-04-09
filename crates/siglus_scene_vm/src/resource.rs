//! Siglus-like resource lookup for BG images.
//!
//! This stage implements the file search policy used by `tnm_find_g00_sub`:
//! try `g00/<name>.<ext>` in the order: g00, bmp, png, jpg, dds.
//!
//! User request: **prefer g00, then fallback bg**.

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PctType {
    G00,
    Bmp,
    Png,
    Jpg,
    Dds,
}

impl PctType {
    pub fn ext(self) -> &'static str {
        match self {
            PctType::G00 => "g00",
            PctType::Bmp => "bmp",
            PctType::Png => "png",
            PctType::Jpg => "jpg",
            PctType::Dds => "dds",
        }
    }
}

const ORDER: [PctType; 5] = [
    PctType::G00,
    PctType::Bmp,
    PctType::Png,
    PctType::Jpg,
    PctType::Dds,
];

/// Find an image path for BG loading.
///
/// Policy:
/// 1. Search in `g00/` (preferred)
/// 2. If not found, search in `bg/`
///
/// For each directory, extension order is: g00, bmp, png, jpg, dds.
/// If `name` already contains an extension, we only test that extension.
pub fn find_bg_image(project_dir: &Path, name: &str) -> Result<(PathBuf, PctType)> {
    if name.is_empty() {
        bail!("empty bg name");
    }

    // If name is an explicit path, resolve relative to project_dir.
    let as_path = Path::new(name);
    if as_path.components().count() > 1 {
        let candidate = project_dir.join(as_path);
        if candidate.is_file() {
            let pct = pct_from_path(&candidate)?;
            return Ok((candidate, pct));
        }
    }

    for dir in ["g00", "bg"] {
        if let Ok(found) = find_in_subdir(project_dir, dir, name) {
            return Ok(found);
        }
    }

    bail!("bg resource not found: {name}");
}

/// Find an image path restricted to the `g00/` directory.
///
/// This is used for CHR / sprite loading.
///
/// Extension order: g00, bmp, png, jpg, dds.
/// If `name` already contains an extension, we only test that extension.
pub fn find_g00_image(project_dir: &Path, name: &str) -> Result<(PathBuf, PctType)> {
    if name.is_empty() {
        bail!("empty image name");
    }

    // If name is an explicit path, resolve relative to project_dir.
    let as_path = Path::new(name);
    if as_path.components().count() > 1 {
        let candidate = project_dir.join(as_path);
        if candidate.is_file() {
            let pct = pct_from_path(&candidate)?;
            return Ok((candidate, pct));
        }
    }

    if let Ok(found) = find_in_subdir(project_dir, "g00", name) {
        return Ok(found);
    }

    bail!("g00 resource not found: {name}");
}

fn find_in_subdir(project_dir: &Path, subdir: &str, name: &str) -> Result<(PathBuf, PctType)> {
    let base = project_dir.join(subdir);

    let (stem, explicit_ext) = split_name_ext(name);
    if let Some(ext) = explicit_ext {
        let pct = pct_from_ext(ext)?;
        let p = base.join(format!("{stem}.{ext}"));
        if p.is_file() {
            return Ok((p, pct));
        }
        bail!("not found");
    }

    for pct in ORDER {
        let p = base.join(format!("{stem}.{}", pct.ext()));
        if p.is_file() {
            return Ok((p, pct));
        }
    }

    bail!("not found");
}

fn split_name_ext(name: &str) -> (&str, Option<&str>) {
    if let Some((a, b)) = name.rsplit_once('.') {
        if !a.is_empty() && !b.is_empty() {
            return (a, Some(b));
        }
    }
    (name, None)
}

fn pct_from_path(p: &Path) -> Result<PctType> {
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    pct_from_ext(&ext)
}

fn pct_from_ext(ext: &str) -> Result<PctType> {
    match ext.to_ascii_lowercase().as_str() {
        "g00" => Ok(PctType::G00),
        "bmp" => Ok(PctType::Bmp),
        "png" => Ok(PctType::Png),
        "jpg" | "jpeg" => Ok(PctType::Jpg),
        "dds" => Ok(PctType::Dds),
        _ => bail!("unknown extension: {ext}"),
    }
}
