//! Siglus-like resource lookup for BG images and movies.
//!
//! This stage implements the file search policy used by the original helpers in
//! `eng_dir.cpp`:
//! - `tnm_find_g00_sub`: try `g00/<name>.<ext>` in the order
//!   `g00 -> bmp -> png -> jpg -> dds`
//! - `tnm_find_g00`: search append directories from the current append entry to
//!   the end of `Select.ini`
//! - `tnm_find_mov`: search append directories from the current append entry to
//!   the end of `Select.ini`, with extension order `wmv -> mpg -> avi`
//!
//! We keep the existing explicit-path behavior for the port, but normal resource
//! resolution follows the original directory search order.

use anyhow::{bail, Result};
use std::fs;
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

const MOV_ORDER: [(&str, i32); 4] = [("wmv", 1), ("mpg", 2), ("avi", 3), ("omv", 4)];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovieType {
    Wmv = 1,
    Mpg = 2,
    Avi = 3,
    Omv = 4,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SoundType {
    Wav = 1,
    Nwa = 2,
    Ogg = 3,
    Owp = 4,
    Ovk = 5,
}

impl SoundType {
    pub fn ext(self) -> &'static str {
        match self {
            Self::Wav => "wav",
            Self::Nwa => "nwa",
            Self::Ogg => "ogg",
            Self::Owp => "owp",
            Self::Ovk => "ovk",
        }
    }
}

impl MovieType {
    pub fn from_id(id: i32) -> Option<Self> {
        match id {
            1 => Some(Self::Wmv),
            2 => Some(Self::Mpg),
            3 => Some(Self::Avi),
            4 => Some(Self::Omv),
            _ => None,
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Self::Wmv => "wmv",
            Self::Mpg => "mpg",
            Self::Avi => "avi",
            Self::Omv => "omv",
        }
    }
}

/// Find an image path for BG loading.
///
/// Original Siglus logic:
/// 1. Search append directories from the current append entry, in order.
/// 2. In each directory, search `g00/` first.
/// 3. If not found, search `bg/`.
/// 4. For each directory, extension order is: g00, bmp, png, jpg, dds.
///
/// If `name` already contains an extension, we only test that extension.
pub fn find_bg_image(project_dir: &Path, name: &str) -> Result<(PathBuf, PctType)> {
    find_bg_image_with_append_dir(project_dir, "", name)
}

pub fn find_bg_image_with_append_dir(
    project_dir: &Path,
    current_append_dir: &str,
    name: &str,
) -> Result<(PathBuf, PctType)> {
    if name.is_empty() {
        bail!("empty bg name");
    }

    let as_path = Path::new(name);
    if as_path.components().count() > 1 {
        let candidate = project_dir.join(as_path);
        if candidate.is_file() {
            let pct = pct_from_path(&candidate)?;
            return Ok((candidate, pct));
        }
    }

    for append_dir in ordered_append_dirs(project_dir, current_append_dir) {
        if let Ok(found) = find_in_subdir(project_dir, &append_dir, "g00", name) {
            return Ok(found);
        }
        if let Ok(found) = find_in_subdir(project_dir, &append_dir, "bg", name) {
            return Ok(found);
        }
    }

    bail!("bg resource not found: {name}");
}

/// Find an image path restricted to the `g00/` directory.
///
/// Original Siglus logic searches append directories from the current append
/// entry to the end of `Select.ini`.
pub fn find_g00_image(project_dir: &Path, name: &str) -> Result<(PathBuf, PctType)> {
    find_g00_image_with_append_dir(project_dir, "", name)
}

pub fn find_g00_image_with_append_dir(
    project_dir: &Path,
    current_append_dir: &str,
    name: &str,
) -> Result<(PathBuf, PctType)> {
    if name.is_empty() {
        bail!("empty image name");
    }

    let as_path = Path::new(name);
    if as_path.components().count() > 1 {
        let candidate = project_dir.join(as_path);
        if candidate.is_file() {
            let pct = pct_from_path(&candidate)?;
            return Ok((candidate, pct));
        }
    }

    for append_dir in ordered_append_dirs(project_dir, current_append_dir) {
        if let Ok(found) = find_in_subdir(project_dir, &append_dir, "g00", name) {
            return Ok(found);
        }
    }

    bail!("g00 resource not found: {name}");
}

pub fn find_mov_path(project_dir: &Path, file_name: &str) -> Result<(PathBuf, MovieType)> {
    find_mov_path_with_append_dir(project_dir, "", file_name)
}

pub fn find_omv_path_with_append_dir(
    project_dir: &Path,
    current_append_dir: &str,
    file_name: &str,
) -> Result<PathBuf> {
    if file_name.is_empty() {
        bail!("empty movie name");
    }

    let p = Path::new(file_name);
    if p.is_absolute() {
        if p.is_file() && movie_type_from_path(p)? == MovieType::Omv {
            return Ok(p.to_path_buf());
        }
        bail!("omv movie not found: {file_name}");
    }

    if p.components().count() > 1 {
        let candidate = project_dir.join(p);
        if candidate.is_file() && movie_type_from_path(&candidate)? == MovieType::Omv {
            return Ok(candidate);
        }
    }

    let (stem, explicit_ext) = split_name_ext(file_name);
    if let Some(ext) = explicit_ext {
        if !ext.eq_ignore_ascii_case("omv") {
            bail!("object movie requires .omv: {file_name}");
        }
    }

    for append_dir in ordered_append_dirs(project_dir, current_append_dir) {
        let base = base_in_append(project_dir, &append_dir, "mov");
        let p = base.join(format!("{stem}.omv"));
        if p.is_file() {
            return Ok(p);
        }
    }

    bail!("omv movie not found: {file_name}");
}

pub fn find_mov_path_with_append_dir(
    project_dir: &Path,
    current_append_dir: &str,
    file_name: &str,
) -> Result<(PathBuf, MovieType)> {
    if file_name.is_empty() {
        bail!("empty movie name");
    }

    let p = Path::new(file_name);
    if p.is_absolute() {
        if p.is_file() {
            let ty = movie_type_from_path(p)?;
            return Ok((p.to_path_buf(), ty));
        }
        bail!("movie not found: {file_name}");
    }

    if p.components().count() > 1 {
        let candidate = project_dir.join(p);
        if candidate.is_file() {
            let ty = movie_type_from_path(&candidate)?;
            return Ok((candidate, ty));
        }
    }

    let (stem, explicit_ext) = split_name_ext(file_name);
    for append_dir in ordered_append_dirs(project_dir, current_append_dir) {
        let base = base_in_append(project_dir, &append_dir, "mov");
        if let Some(ext) = explicit_ext {
            let ty = movie_type_from_ext(ext)?;
            let p = base.join(format!("{stem}.{ext}"));
            if p.is_file() {
                return Ok((p, ty));
            }
            continue;
        }

        for (ext, ty_id) in MOV_ORDER {
            let p = base.join(format!("{stem}.{ext}"));
            if p.is_file() {
                let ty = MovieType::from_id(ty_id).expect("valid movie type");
                return Ok((p, ty));
            }
        }
    }

    bail!("movie not found: {file_name}");
}

pub fn find_audio_path_with_append_dir(
    project_dir: &Path,
    current_append_dir: &str,
    subdir: &str,
    file_name: &str,
) -> Result<(PathBuf, SoundType)> {
    if file_name.is_empty() {
        bail!("empty audio name");
    }

    let p = Path::new(file_name);
    if p.is_absolute() {
        if p.is_file() {
            let ty = sound_type_from_path(p)?;
            return Ok((p.to_path_buf(), ty));
        }
        bail!("audio not found: {file_name}");
    }

    if p.components().count() > 1 {
        let candidate = project_dir.join(p);
        if candidate.is_file() {
            let ty = sound_type_from_path(&candidate)?;
            return Ok((candidate, ty));
        }
    }

    let (stem, explicit_ext) = split_name_ext(file_name);
    let order = [
        SoundType::Wav,
        SoundType::Nwa,
        SoundType::Ogg,
        SoundType::Owp,
        SoundType::Ovk,
    ];

    for append_dir in ordered_append_dirs(project_dir, current_append_dir) {
        let base = base_in_append(project_dir, &append_dir, subdir);
        if let Some(ext) = explicit_ext {
            let ty = sound_type_from_ext(ext)?;
            let p = base.join(format!("{stem}.{ext}"));
            if p.is_file() {
                return Ok((p, ty));
            }
            continue;
        }

        for ty in order {
            let p = base.join(format!("{stem}.{}", ty.ext()));
            if p.is_file() {
                return Ok((p, ty));
            }
        }
    }

    bail!("audio not found: {file_name}");
}

fn ordered_append_dirs(project_dir: &Path, current_append_dir: &str) -> Vec<String> {
    let mut dirs = parse_select_ini_append_dirs(project_dir);
    if dirs.is_empty() {
        dirs.push(String::new());
    }

    if current_append_dir.is_empty() {
        return dirs;
    }

    if let Some(pos) = dirs.iter().position(|d| d == current_append_dir) {
        return dirs.into_iter().skip(pos).collect();
    }

    dirs
}

fn parse_select_ini_append_dirs(project_dir: &Path) -> Vec<String> {
    let mut candidates = vec![project_dir.join("Select.ini")];
    candidates.push(project_dir.join("select.ini"));

    let Some(path) = candidates.into_iter().find(|p| p.is_file()) else {
        return vec![String::new()];
    };

    let Ok(text) = fs::read_to_string(path) else {
        return vec![String::new()];
    };

    let mut out = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let mut cols = line.split('\t');
        let dir = cols.next().unwrap_or("");
        let _name = cols.next();
        if cols.next().is_some() {
            continue;
        }
        out.push(dir.to_string());
    }

    if out.is_empty() {
        out.push(String::new());
    }
    out
}

fn base_in_append(project_dir: &Path, append_dir: &str, subdir: &str) -> PathBuf {
    let mut base = project_dir.to_path_buf();
    if !append_dir.is_empty() {
        base = base.join(append_dir);
    }
    if !subdir.is_empty() {
        base = base.join(subdir);
    }
    base
}

fn find_in_subdir(
    project_dir: &Path,
    append_dir: &str,
    subdir: &str,
    name: &str,
) -> Result<(PathBuf, PctType)> {
    let base = base_in_append(project_dir, append_dir, subdir);

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

fn sound_type_from_path(p: &Path) -> Result<SoundType> {
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    sound_type_from_ext(&ext)
}

fn sound_type_from_ext(ext: &str) -> Result<SoundType> {
    match ext.to_ascii_lowercase().as_str() {
        "wav" => Ok(SoundType::Wav),
        "nwa" => Ok(SoundType::Nwa),
        "ogg" => Ok(SoundType::Ogg),
        "owp" => Ok(SoundType::Owp),
        "ovk" => Ok(SoundType::Ovk),
        _ => bail!("unknown sound extension: {ext}"),
    }
}

fn movie_type_from_path(p: &Path) -> Result<MovieType> {
    let ext = p
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    movie_type_from_ext(&ext)
}

fn movie_type_from_ext(ext: &str) -> Result<MovieType> {
    match ext.to_ascii_lowercase().as_str() {
        "wmv" => Ok(MovieType::Wmv),
        "mpg" => Ok(MovieType::Mpg),
        "avi" => Ok(MovieType::Avi),
        "omv" => Ok(MovieType::Omv),
        _ => bail!("unknown movie extension: {ext}"),
    }
}
