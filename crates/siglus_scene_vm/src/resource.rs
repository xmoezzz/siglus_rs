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
use std::path::{Component, Path, PathBuf};
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use crate::wasm_vfs::SiglusVfs;


#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
fn path_to_wasm_vfs(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "/")
        .split('/')
        .filter(|part| !part.is_empty() && *part != ".")
        .collect::<Vec<_>>()
        .join("/")
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) fn wasm_path_exists(path: &Path) -> bool {
    crate::wasm_vfs::WasmDirectoryVfs::new().exists(&path_to_wasm_vfs(path))
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) fn wasm_path_is_file(path: &Path) -> bool {
    wasm_path_exists(path)
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) fn wasm_path_is_dir(path: &Path) -> bool {
    !crate::wasm_vfs::WasmDirectoryVfs::new()
        .list_dir(&path_to_wasm_vfs(path))
        .unwrap_or_default()
        .is_empty()
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn read_file_bytes(path: &Path) -> Result<Vec<u8>> {
    crate::wasm_vfs::WasmDirectoryVfs::new().read_all(&path_to_wasm_vfs(path))
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn read_file_bytes(path: &Path) -> Result<Vec<u8>> {
    let Some(resolved) = resolve_windows_case_insensitive_file(path)? else {
        bail!("file not found: {}", path.display());
    };
    Ok(fs::read(resolved)?)
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub fn read_file_to_string(path: &Path) -> Result<String> {
    let bytes = read_file_bytes(path)?;
    Ok(String::from_utf8(bytes)?)
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub fn read_file_to_string(path: &Path) -> Result<String> {
    Ok(String::from_utf8(read_file_bytes(path)?)?)
}

/// Pre-load the entire project directory tree into a global in-memory index.
///
/// Call once at engine startup (e.g. from `SceneVm::new`).
/// On wasm this is a no-op — the VFS is already in memory.
pub fn preload_project_file_index(project_dir: &Path) {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    index::init(project_dir);
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
mod index {
    use super::*;
    use std::collections::HashMap;
    use std::sync::OnceLock;

    pub(super) struct ProjectIndex {
        pub(super) file_map: HashMap<String, PathBuf>,
        pub(super) append_dirs: Vec<String>,
        /// Lowercase scan roots. A lookup path under one of these is answered
        /// authoritatively by the index: a miss means the file does not exist,
        /// so no syscall fallback is needed.
        scanned_roots: Vec<String>,
        /// Lowercase runtime-written prefixes (e.g. `<project>/savedata`) that
        /// must always use the syscall fallback: files appear there after the
        /// index was built.
        excluded_prefixes: Vec<String>,
    }

    static FILE_INDEX: OnceLock<ProjectIndex> = OnceLock::new();

    pub(super) fn get() -> Option<&'static ProjectIndex> {
        FILE_INDEX.get()
    }

    fn scan_dir(dir: &Path, map: &mut HashMap<String, PathBuf>) {
        for entry in walkdir::WalkDir::new(dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.into_path();
            if let Some(k) = path.as_os_str().to_str() {
                map.insert(k.to_ascii_lowercase(), path);
            }
        }
    }

    pub(super) fn init(project_dir: &Path) {
        let append_dirs = parse_select_ini_append_dirs_uncached(project_dir);
        let mut file_map = HashMap::new();
        let mut scanned_roots = Vec::new();
        let mut excluded_prefixes = Vec::new();

        for append in &append_dirs {
            // Scan each append root in full: the runtime also resolves
            // `dat/`, `sys/`, root-level files, etc., so the index must cover
            // the entire game tree, not a handful of asset subdirs.
            let root = base_in_append(project_dir, append, "");
            scan_dir(&root, &mut file_map);
            if let Some(k) = root.as_os_str().to_str() {
                scanned_roots.push(k.to_ascii_lowercase());
            }
        }

        // Runtime-written save data lives under `<project>/savedata` and must
        // always go through the syscall fallback.
        let save_root = project_dir.join("savedata");
        if let Some(k) = save_root.as_os_str().to_str() {
            excluded_prefixes.push(k.to_ascii_lowercase());
        }

        let _ = FILE_INDEX.set(ProjectIndex {
            file_map,
            append_dirs,
            scanned_roots,
            excluded_prefixes,
        });
    }

    /// Whether `key` (lowercase) lives at or under `root` (lowercase).
    fn path_under(key: &str, root: &str) -> bool {
        if !key.starts_with(root) {
            return false;
        }
        key.len() == root.len()
            || key[root.len()..].starts_with('\\')
            || key[root.len()..].starts_with('/')
    }

    /// Runtime-written paths the static index must not answer for: capture
    /// flags are written next to images (`foo.png.siglus_flags`) during play,
    /// and save data lives under `savedata/`. These always use the syscall
    /// fallback so freshly created, overwritten, or deleted files stay visible.
    pub(super) fn is_excluded(path: &Path) -> bool {
        let Some(key) = path.as_os_str().to_str() else {
            return true;
        };
        let key = key.to_ascii_lowercase();
        if key.ends_with(".siglus_flags") {
            return true;
        }
        get().is_some_and(|idx| {
            idx.excluded_prefixes
                .iter()
                .any(|prefix| path_under(&key, prefix))
        })
    }

    /// True when the index is built and `path` is inside a scanned root. For
    /// such paths a miss is authoritative: the file genuinely does not exist,
    /// so the syscall fallback is skipped.
    pub(super) fn is_authoritative(path: &Path) -> bool {
        let Some(idx) = get() else {
            return false;
        };
        let Some(key) = path.as_os_str().to_str() else {
            return false;
        };
        let key = key.to_ascii_lowercase();
        idx.scanned_roots.iter().any(|root| path_under(&key, root))
    }
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod index {
    pub(super) struct ProjectIndex;
    #[inline(always)]
    pub(super) fn get() -> Option<&'static ProjectIndex> { None }
}

fn index_lookup(path: &Path) -> Option<PathBuf> {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        let idx = index::get()?;
        let key = path.as_os_str().to_str()?.to_ascii_lowercase();
        idx.file_map.get(&key).cloned()
    }
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    { None }
}

fn path_component_eq_windows(a: &std::ffi::OsStr, b: &std::ffi::OsStr) -> bool {
    a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
}

pub(crate) fn resolve_windows_case_insensitive_path(path: &Path) -> Result<Option<PathBuf>> {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        if wasm_path_exists(path) {
            return Ok(Some(path.to_path_buf()));
        }
        return Ok(None);
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    if path.exists() {
        return Ok(Some(path.to_path_buf()));
    }

    let mut cur = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => cur.push(prefix.as_os_str()),
            Component::RootDir => cur.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => cur.push(".."),
            Component::Normal(name) => {
                let exact = cur.join(name);
                if exact.exists() {
                    cur = exact;
                    continue;
                }

                let parent = if cur.as_os_str().is_empty() {
                    Path::new(".")
                } else {
                    cur.as_path()
                };
                if !parent.is_dir() {
                    return Ok(None);
                }

                let mut matches = Vec::new();
                for entry in fs::read_dir(parent)? {
                    let entry = entry?;
                    if path_component_eq_windows(&entry.file_name(), name) {
                        matches.push(entry.path());
                    }
                }

                match matches.len() {
                    0 => return Ok(None),
                    1 => cur = matches.remove(0),
                    _ => {
                        matches.sort();
                        bail!(
                            "case-insensitive path conflict for {} under {}: {}",
                            name.to_string_lossy(),
                            parent.display(),
                            matches
                                .iter()
                                .map(|p| p.display().to_string())
                                .collect::<Vec<_>>()
                                .join(", ")
                        );
                    }
                }
            }
        }
    }

    Ok(cur.exists().then_some(cur))
}

pub(crate) fn resolve_windows_case_insensitive_file(path: &Path) -> Result<Option<PathBuf>> {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        return Ok(wasm_path_is_file(path).then_some(path.to_path_buf()));
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        // Runtime-written paths (save data, capture flags) are always queried
        // against the real filesystem.
        if index::is_excluded(path) {
            return resolve_windows_case_insensitive_file_fallback(path);
        }

        // Fast path: pre-built in-memory file index.
        if let Some(found) = index_lookup(path) {
            return Ok(Some(found));
        }

        // The index covers the whole game tree: for paths under it a miss is
        // authoritative (the file does not exist), so skip the syscall
        // fallback entirely.
        if index::is_authoritative(path) {
            return Ok(None);
        }

        // Fallback: when the index wasn't built (tests, tools, etc.) and for
        // paths outside the game tree (system fonts, engine assets, relative
        // paths).
        resolve_windows_case_insensitive_file_fallback(path)
    }
}

/// Syscall-based fallback: direct `is_file` check, then the case-insensitive
/// component walk. Used only when the index cannot answer (not built, path
/// outside the game tree, or an excluded runtime-written path).
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn resolve_windows_case_insensitive_file_fallback(path: &Path) -> Result<Option<PathBuf>> {
    if path.is_file() {
        return Ok(Some(path.to_path_buf()));
    }
    let Some(resolved) = resolve_windows_case_insensitive_path(path)? else {
        return Ok(None);
    };
    Ok(resolved.is_file().then_some(resolved))
}

/// Resolve an existing game path using the Windows case-insensitive semantics
/// expected by Siglus scripts and resource data. This works for files or
/// directories.
pub fn resolve_game_path(path: &Path) -> Result<Option<PathBuf>> {
    resolve_windows_case_insensitive_path(path)
}

/// Resolve an existing game file using Windows case-insensitive semantics.
pub fn resolve_game_file(path: &Path) -> Result<Option<PathBuf>> {
    resolve_windows_case_insensitive_file(path)
}

pub fn game_path_exists(path: &Path) -> bool {
    match resolve_windows_case_insensitive_path(path) {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(err) => {
            log::error!(
                "case-insensitive game path resolution failed for {}: {:#}",
                path.display(),
                err
            );
            false
        }
    }
}

pub fn game_file_exists(path: &Path) -> bool {
    match resolve_windows_case_insensitive_file(path) {
        Ok(Some(_)) => true,
        Ok(None) => false,
        Err(err) => {
            log::error!(
                "case-insensitive game file resolution failed for {}: {:#}",
                path.display(),
                err
            );
            false
        }
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn open_game_file(path: &Path) -> Result<fs::File> {
    let Some(resolved) = resolve_windows_case_insensitive_file(path)? else {
        bail!("file not found: {}", path.display());
    };
    Ok(fs::File::open(resolved)?)
}

/// Return the size of an existing game file while preserving the same
/// case-insensitive lookup semantics as the rest of the Siglus game VFS.
///
/// Native hosts can query filesystem metadata directly after resolution.  The
/// browser target has no meaningful `std::fs::Metadata`, so use the VFS bytes
/// instead.  Callers that only need the file length must use this helper rather
/// than depending on a host-specific metadata type.
pub(crate) fn game_file_len(path: &Path) -> Result<u64> {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        return Ok(read_file_bytes(path)?.len() as u64);
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        let Some(resolved) = resolve_windows_case_insensitive_file(path)? else {
            bail!("file not found: {}", path.display());
        };
        Ok(fs::metadata(resolved)?.len())
    }
}

fn first_existing_file_windows_ci(candidates: impl IntoIterator<Item = PathBuf>) -> Result<Option<PathBuf>> {
    for candidate in candidates {
        if let Some(path) = resolve_windows_case_insensitive_file(&candidate)? {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

pub fn load_project_key_toml(
    project_dir: &Path,
) -> Result<Option<siglus_assets::key_toml::KeyTomlConfig>> {
    let Some(path) = resolve_windows_case_insensitive_file(&project_dir.join("key.toml"))? else {
        return Ok(None);
    };
    let text = read_file_to_string(&path)?;
    Ok(Some(siglus_assets::key_toml::parse_key_toml(&text)?))
}

pub fn load_project_emote_key(project_dir: &Path) -> Result<Option<u32>> {
    Ok(load_project_key_toml(project_dir)?.and_then(|cfg| cfg.emote_key))
}

pub fn load_gameexe_decode_options(
    project_dir: &Path,
) -> Result<siglus_assets::gameexe::GameexeDecodeOptions> {
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        let mut opt = siglus_assets::gameexe::GameexeDecodeOptions::default();
        opt.game_angou_code = Some(siglus_assets::keys::GAMEEXE_KEY.to_vec());
        if let Some(cfg) = load_project_key_toml(project_dir)? {
            opt.exe_key16 = cfg.exe_key16;
            opt.base_angou_code = cfg.base_angou_code;
            if cfg.game_angou_code.is_some() {
                opt.game_angou_code = cfg.game_angou_code;
            }
            if let Some(order) = cfg.chain_order {
                opt.chain_order = order;
            }
        }
        Ok(opt)
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        Ok(siglus_assets::gameexe::GameexeDecodeOptions::from_project_dir(project_dir)?)
    }
}

pub fn find_scene_pck_path(project_dir: &Path) -> Result<PathBuf> {
    for candidate in [
        project_dir.join("Scene.pck"),
        project_dir.join("Data").join("Scene.pck"),
    ] {
        if let Some(path) = resolve_windows_case_insensitive_file(&candidate)? {
            return Ok(path);
        }
    }
    bail!("Scene.pck not found under {}", project_dir.display())
}

fn format_tried_paths(paths: &[PathBuf]) -> String {
    if paths.is_empty() {
        return String::from("<none>");
    }
    paths
        .iter()
        .map(|p| p.display().to_string())
        .collect::<Vec<_>>()
        .join("; ")
}

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
        if let Some(candidate) = resolve_windows_case_insensitive_file(&candidate)? {
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
        if let Some(candidate) = resolve_windows_case_insensitive_file(&candidate)? {
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
        if let Some(path) = resolve_windows_case_insensitive_file(p)? {
            if movie_type_from_path(&path)? == MovieType::Omv {
                return Ok(path);
            }
        }
        bail!("omv movie not found: {file_name}");
    }

    if p.components().count() > 1 {
        let candidate = project_dir.join(p);
        if let Some(candidate) = resolve_windows_case_insensitive_file(&candidate)? {
            if movie_type_from_path(&candidate)? == MovieType::Omv {
                return Ok(candidate);
            }
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
        if let Some(path) = resolve_windows_case_insensitive_file(&p)? {
            return Ok(path);
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
        if let Some(path) = resolve_windows_case_insensitive_file(p)? {
            let ty = movie_type_from_path(&path)?;
            return Ok((path, ty));
        }
        bail!("movie not found: {file_name}");
    }

    if p.components().count() > 1 {
        let candidate = project_dir.join(p);
        if let Some(candidate) = resolve_windows_case_insensitive_file(&candidate)? {
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
            if let Some(path) = resolve_windows_case_insensitive_file(&p)? {
                return Ok((path, ty));
            }
            continue;
        }

        for (ext, ty_id) in MOV_ORDER {
            let p = base.join(format!("{stem}.{ext}"));
            if let Some(path) = resolve_windows_case_insensitive_file(&p)? {
                let ty = MovieType::from_id(ty_id).expect("valid movie type");
                return Ok((path, ty));
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
        if let Some(path) = resolve_windows_case_insensitive_file(p)? {
            let ty = sound_type_from_path(&path)?;
            return Ok((path, ty));
        }
        bail!("audio not found: {file_name}; tried={}", p.display());
    }

    if p.components().count() > 1 {
        let candidate = project_dir.join(p);
        if let Some(candidate) = resolve_windows_case_insensitive_file(&candidate)? {
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

    let mut tried = Vec::new();
    for append_dir in ordered_append_dirs(project_dir, current_append_dir) {
        let base = base_in_append(project_dir, &append_dir, subdir);
        if let Some(ext) = explicit_ext {
            let ty = sound_type_from_ext(ext)?;
            let p = base.join(format!("{stem}.{ext}"));
            tried.push(p.clone());
            if let Some(path) = resolve_windows_case_insensitive_file(&p)? {
                return Ok((path, ty));
            }
            continue;
        }

        for ty in order {
            let p = base.join(format!("{stem}.{}", ty.ext()));
            tried.push(p.clone());
            if let Some(path) = resolve_windows_case_insensitive_file(&p)? {
                return Ok((path, ty));
            }
        }
    }

    bail!(
        "audio not found: {file_name}; project_dir={}; current_append_dir={}; subdir={}; tried={}",
        project_dir.display(),
        current_append_dir,
        subdir,
        format_tried_paths(&tried)
    );
}

pub(crate) fn ordered_append_dirs(project_dir: &Path, current_append_dir: &str) -> Vec<String> {
    let mut dirs = parse_select_ini_append_dirs(project_dir);
    if dirs.is_empty() {
        dirs.push(String::new());
    }

    if current_append_dir.is_empty() {
        return dirs;
    }

    if let Some(pos) = dirs.iter().position(|d| d.eq_ignore_ascii_case(current_append_dir)) {
        return dirs.into_iter().skip(pos).collect();
    }

    dirs
}

/// Enumerate possible Emote PSB files without recursively scanning the game.
///
/// This deliberately mirrors the scope of original `tnm_find_psb`: only the
/// `dat` directory of each Select.ini append is considered.  Candidates are
/// ordered by file size so key probing/bruteforce can use the cheapest PSB
/// first, while the runtime's actual named-file resolution can still follow
/// original append order later.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn find_emote_psb_candidates(project_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut candidates: Vec<(u64, PathBuf)> = Vec::new();

    for append_dir in ordered_append_dirs(project_dir, "") {
        let requested_dat = base_in_append(project_dir, &append_dir, "dat");
        let Some(dat_dir) = resolve_windows_case_insensitive_path(&requested_dat)? else {
            continue;
        };
        if !dat_dir.is_dir() {
            continue;
        }

        let entries = match fs::read_dir(&dat_dir) {
            Ok(entries) => entries,
            Err(err) => {
                log::warn!(
                    "Emote key preload: cannot enumerate {}: {}",
                    dat_dir.display(),
                    err
                );
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(err) => {
                    log::warn!(
                        "Emote key preload: failed to read an entry under {}: {}",
                        dat_dir.display(),
                        err
                    );
                    continue;
                }
            };
            let path = entry.path();
            if !path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("psb"))
            {
                continue;
            }
            let metadata = match entry.metadata() {
                Ok(metadata) if metadata.is_file() => metadata,
                Ok(_) => continue,
                Err(err) => {
                    log::warn!(
                        "Emote key preload: cannot stat {}: {}",
                        path.display(),
                        err
                    );
                    continue;
                }
            };
            candidates.push((metadata.len(), path));
        }
    }

    candidates.sort_by(|(len_a, path_a), (len_b, path_b)| {
        len_a.cmp(len_b).then_with(|| path_a.cmp(path_b))
    });
    candidates.dedup_by(|(_, path_a), (_, path_b)| path_a == path_b);
    Ok(candidates.into_iter().map(|(_, path)| path).collect())
}

fn parse_select_ini_append_dirs(project_dir: &Path) -> Vec<String> {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
        if let Some(idx) = index::get() {
            return idx.append_dirs.clone();
        }
    }
    parse_select_ini_append_dirs_uncached(project_dir)
}

fn parse_select_ini_append_dirs_uncached(project_dir: &Path) -> Vec<String> {
    let mut candidates = vec![project_dir.join("Select.ini")];
    candidates.push(project_dir.join("select.ini"));

    let path = match first_existing_file_windows_ci(candidates) {
        Ok(Some(path)) => path,
        Ok(None) | Err(_) => return vec![String::new()],
    };

    let Ok(text) = read_file_to_string(&path) else {
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
        if let Some(path) = resolve_windows_case_insensitive_file(&p)? {
            return Ok((path, pct));
        }
        bail!("not found");
    }

    for pct in ORDER {
        let p = base.join(format!("{stem}.{}", pct.ext()));
        if let Some(path) = resolve_windows_case_insensitive_file(&p)? {
            return Ok((path, pct));
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

/// Return the exact suffix of Select.ini append entries beginning at the
/// current append directory. Unlike the broader resource resolver, original
/// `tnm_find_dat`/`tnm_find_psb` do not fall back to the beginning when the
/// current append directory is absent from the Select.ini list.
fn strict_append_dirs_from_current(project_dir: &Path, current_append_dir: &str) -> Vec<String> {
    let dirs = parse_select_ini_append_dirs(project_dir);
    let Some(pos) = dirs.iter().position(|d| d.eq_ignore_ascii_case(current_append_dir)) else {
        return Vec::new();
    };
    dirs.into_iter().skip(pos).collect()
}

/// Resolve an Emote PSB exactly like original `tnm_find_psb`: locate the
/// current append entry in Select.ini, then search that entry and later ones
/// in order, appending `.psb` under `dat/`.
pub(crate) fn resolve_emote_psb_path(
    project_dir: &Path,
    current_append_dir: &str,
    file_name: &str,
) -> Result<Option<PathBuf>> {
    if file_name.is_empty() {
        return Ok(None);
    }
    for append_dir in strict_append_dirs_from_current(project_dir, current_append_dir) {
        let candidate = base_in_append(project_dir, &append_dir, "dat")
            .join(format!("{file_name}.psb"));
        if let Some(path) = resolve_windows_case_insensitive_file(&candidate)? {
            return Ok(Some(path));
        }
    }
    Ok(None)
}

/// Resolve a literal file under `dat/` with the exact original
/// `tnm_find_dat` append-list search used by Emote mouth-volume tables.
pub(crate) fn resolve_dat_file_path(
    project_dir: &Path,
    current_append_dir: &str,
    file_name: &str,
) -> Result<Option<PathBuf>> {
    if file_name.is_empty() {
        return Ok(None);
    }
    for append_dir in strict_append_dirs_from_current(project_dir, current_append_dir) {
        let candidate = base_in_append(project_dir, &append_dir, "dat").join(file_name);
        if let Some(path) = resolve_windows_case_insensitive_file(&candidate)? {
            return Ok(Some(path));
        }
    }
    Ok(None)
}
