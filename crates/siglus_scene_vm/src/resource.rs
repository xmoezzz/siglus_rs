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

use anyhow::{Result, bail};
use std::collections::HashMap;
#[cfg(any(feature="nfs", feature="smb"))]
pub(crate) use game_network::fs;
#[cfg(not(any(feature="nfs", feature="smb")))]
pub(crate) use std::fs;
#[cfg(target_os = "horizon")]
use std::io::Read;
use std::path::{Path, PathBuf};

use std::path::Component;

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use crate::wasm_vfs::SiglusVfs;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use std::ffi::{OsStr, OsString};
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use std::time::SystemTime;

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

#[cfg(target_os = "horizon")]
pub fn read_file_bytes(path: &Path) -> Result<Vec<u8>> {
    let Some(resolved) = resolve_windows_case_insensitive_file(path)? else {
        bail!("file not found: {}", path.display());
    };
    // std::fs::read preallocates from metadata().len(). fsdev currently
    // reports an invalid size through the custom Horizon std target, turning
    // even a small key.toml into a bogus OOM. Read incrementally instead.
    let mut file = fs::File::open(&resolved)?;
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut chunk)?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    Ok(bytes)
}

#[cfg(all(
    not(target_os = "horizon"),
    not(all(target_arch = "wasm32", target_os = "unknown"))
))]
pub fn read_file_bytes(path: &Path) -> Result<Vec<u8>> {
    let Some(resolved) = resolve_windows_case_insensitive_file(path)? else {
        bail!("file not found: {}", path.display());
    };
    match fs::read(&resolved) {
        Ok(bytes) => Ok(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            // A positive cache entry can become stale if an external process
            // replaces/removes a resource while the engine is running. Drop
            // the affected directory state and resolve once more.
            invalidate_game_path_cache(path);
            invalidate_game_path_cache(&resolved);
            let Some(re_resolved) = resolve_windows_case_insensitive_file(path)? else {
                bail!("file not found: {}", path.display());
            };
            Ok(fs::read(re_resolved)?)
        }
        Err(err) => Err(err.into()),
    }
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

#[cfg(all(not(all(target_arch = "wasm32", target_os = "unknown")), unix))]
type FoldedPathComponent = Vec<u8>;

#[cfg(all(not(all(target_arch = "wasm32", target_os = "unknown")), windows))]
type FoldedPathComponent = Vec<u16>;

#[cfg(all(
    not(all(target_arch = "wasm32", target_os = "unknown")),
    not(any(unix, windows))
))]
type FoldedPathComponent = String;

/// Fold only ASCII case while preserving every non-ASCII code unit/byte.
///
/// Siglus asset names are overwhelmingly ASCII + Japanese.  This deliberately
/// avoids locale-sensitive Unicode lower-casing/normalization while matching
/// the case-insensitive behavior that matters for original Windows game data.
#[cfg(all(not(all(target_arch = "wasm32", target_os = "unknown")), unix))]
fn fold_windows_component(name: &OsStr) -> FoldedPathComponent {
    use std::os::unix::ffi::OsStrExt;

    name.as_bytes()
        .iter()
        .map(|b| b.to_ascii_lowercase())
        .collect()
}

#[cfg(all(not(all(target_arch = "wasm32", target_os = "unknown")), windows))]
fn fold_windows_component(name: &OsStr) -> FoldedPathComponent {
    use std::os::windows::ffi::OsStrExt;

    name.encode_wide()
        .map(|unit| {
            if (b'A' as u16..=b'Z' as u16).contains(&unit) {
                unit + (b'a' - b'A') as u16
            } else {
                unit
            }
        })
        .collect()
}

#[cfg(all(
    not(all(target_arch = "wasm32", target_os = "unknown")),
    not(any(unix, windows))
))]
fn fold_windows_component(name: &OsStr) -> FoldedPathComponent {
    name.to_string_lossy()
        .chars()
        .map(|ch| ch.to_ascii_lowercase())
        .collect()
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[derive(Debug, Clone)]
enum FoldedDirectoryEntry {
    Unique(OsString),
    Conflict(Vec<OsString>),
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[derive(Debug)]
struct DirectoryCaseIndex {
    modified: Option<SystemTime>,
    entries: HashMap<FoldedPathComponent, FoldedDirectoryEntry>,
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
#[derive(Debug, Default)]
struct NativePathResolverCache {
    /// Positive full-file resolutions.  We intentionally do not cache misses:
    /// files such as captures/save thumbnails can be created while the engine
    /// is running.
    positive_files: HashMap<PathBuf, PathBuf>,
    /// Lazy per-directory case-folded entry maps.  A directory is enumerated
    /// only after exact lookup failed for a component under that directory.
    directories: HashMap<PathBuf, DirectoryCaseIndex>,
}

/// Native Windows-compatible path resolver cache.
///
/// This is deliberately a directory cache, not a startup-time project-wide
/// file database.  Keys are absolute directory/request paths, so independent
/// game roots coexist without sharing lookup state.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
static NATIVE_PATH_RESOLVER_CACHE: std::sync::Mutex<Option<NativePathResolverCache>> =
    std::sync::Mutex::new(None);

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn native_cache_key(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn directory_modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn scan_directory_case_index(parent: &Path) -> Result<Option<DirectoryCaseIndex>> {
    let read_dir = match fs::read_dir(parent) {
        Ok(v) => v,
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(None);
        }
        Err(err) => return Err(err.into()),
    };

    let mut entries: HashMap<FoldedPathComponent, FoldedDirectoryEntry> = HashMap::new();
    for entry in read_dir {
        let entry = entry?;
        let name = entry.file_name();
        let key = fold_windows_component(&name);
        use std::collections::hash_map::Entry;
        match entries.entry(key) {
            Entry::Vacant(v) => {
                v.insert(FoldedDirectoryEntry::Unique(name));
            }
            Entry::Occupied(mut o) => {
                let value = o.get_mut();
                match value {
                    FoldedDirectoryEntry::Unique(existing) => {
                        if existing != &name {
                            let first = existing.clone();
                            *value = FoldedDirectoryEntry::Conflict(vec![first, name]);
                        }
                    }
                    FoldedDirectoryEntry::Conflict(names) => {
                        if !names.iter().any(|v| v == &name) {
                            names.push(name);
                        }
                    }
                }
            }
        }
    }

    for value in entries.values_mut() {
        if let FoldedDirectoryEntry::Conflict(names) = value {
            names.sort();
            names.dedup();
        }
    }

    Ok(Some(DirectoryCaseIndex {
        modified: directory_modified(parent),
        entries,
    }))
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn cached_directory_entry(
    parent: &Path,
    requested: &OsStr,
    force_refresh: bool,
) -> Result<Option<FoldedDirectoryEntry>> {
    let parent_key = native_cache_key(parent);
    let folded = fold_windows_component(requested);

    let (has_index, cached_modified, cached_entry) = {
        let mut guard = NATIVE_PATH_RESOLVER_CACHE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let cache = guard.get_or_insert_with(NativePathResolverCache::default);
        match cache.directories.get(&parent_key) {
            Some(index) => (true, index.modified, index.entries.get(&folded).cloned()),
            None => (false, None, None),
        }
    };

    if !force_refresh {
        if let Some(entry) = cached_entry {
            return Ok(Some(entry));
        }
        if has_index && directory_modified(parent) == cached_modified {
            return Ok(None);
        }
    }

    let Some(index) = scan_directory_case_index(parent)? else {
        let mut guard = NATIVE_PATH_RESOLVER_CACHE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let cache = guard.get_or_insert_with(NativePathResolverCache::default);
        cache.directories.remove(&parent_key);
        return Ok(None);
    };
    let result = index.entries.get(&folded).cloned();

    let mut guard = NATIVE_PATH_RESOLVER_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cache = guard.get_or_insert_with(NativePathResolverCache::default);
    cache.directories.insert(parent_key, index);
    Ok(result)
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn resolve_cached_directory_entry(parent: &Path, requested: &OsStr) -> Result<Option<PathBuf>> {
    let mut refreshed = false;
    loop {
        let Some(entry) = cached_directory_entry(parent, requested, refreshed)? else {
            return Ok(None);
        };
        match entry {
            FoldedDirectoryEntry::Unique(actual_name) => {
                let candidate = parent.join(actual_name);
                if candidate.exists() {
                    return Ok(Some(candidate));
                }
                if refreshed {
                    return Ok(None);
                }
                // The directory changed after it was indexed.  Rebuild once;
                // this is also the external-mutation fallback for stale hits.
                refreshed = true;
            }
            FoldedDirectoryEntry::Conflict(names) => {
                bail!(
                    "case-insensitive path conflict for {} under {}: {}",
                    requested.to_string_lossy(),
                    parent.display(),
                    names
                        .iter()
                        .map(|name| parent.join(name).display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
    }
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn positive_file_cache_get(path: &Path) -> Option<PathBuf> {
    let key = native_cache_key(path);
    let mut guard = NATIVE_PATH_RESOLVER_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cache = guard.get_or_insert_with(NativePathResolverCache::default);
    cache.positive_files.get(&key).cloned()
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn positive_file_cache_insert(path: &Path, resolved: &Path) {
    let key = native_cache_key(path);
    let mut guard = NATIVE_PATH_RESOLVER_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cache = guard.get_or_insert_with(NativePathResolverCache::default);
    cache.positive_files.insert(key, resolved.to_path_buf());
}

/// Invalidate resolver state for a file/directory that was created, removed or
/// renamed by the engine.  External mutations are detected lazily from parent
/// directory metadata on misses/stale directory hits; engine-owned writes use
/// this hook so even filesystems with coarse directory mtimes remain correct.
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) fn invalidate_game_path_cache(path: &Path) {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent_key = native_cache_key(parent);
    let path_key = native_cache_key(path);

    let mut guard = NATIVE_PATH_RESOLVER_CACHE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let cache = guard.get_or_insert_with(NativePathResolverCache::default);
    cache
        .directories
        .retain(|dir, _| dir != &parent_key && dir != &path_key && !dir.starts_with(&path_key));
    cache.positive_files.retain(|requested, resolved| {
        if requested == &path_key || requested.starts_with(&path_key) {
            return false;
        }
        let resolved_key = native_cache_key(resolved);
        let requested_parent = requested.parent();
        let resolved_parent = resolved_key.parent();
        requested_parent != Some(parent_key.as_path())
            && resolved_parent != Some(parent_key.as_path())
            && !resolved_key.starts_with(&path_key)
    });
}

#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) fn invalidate_game_path_cache(_path: &Path) {
    // Browser File objects are registered as an immutable snapshot for one
    // launch. Runtime-generated browser files are not currently inserted into
    // that JavaScript index.
}

pub(crate) fn resolve_windows_case_insensitive_path(path: &Path) -> Result<Option<PathBuf>> {
    #[cfg(any(feature="nfs", feature="smb"))]
    if fs::is_remote(path) {
        return match fs::metadata(path) {
            Ok(metadata) => Ok((true).then(||path.to_owned())),
            Err(error) if error.kind()==std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        };
    }

    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        // `siglusFileExists` already uses the browser-side case-insensitive
        // index built from the selected directory, so no Rust-side directory
        // walk is necessary here.
        if wasm_path_exists(path) {
            return Ok(Some(path.to_path_buf()));
        }
        return Ok(None);
    }

    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    {
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
                    let Some(actual) = resolve_cached_directory_entry(parent, name)? else {
                        return Ok(None);
                    };
                    cur = actual;
                }
            }
        }

        Ok(cur.exists().then_some(cur))
    }
}

pub(crate) fn resolve_windows_case_insensitive_file(path: &Path) -> Result<Option<PathBuf>> {
    #[cfg(any(feature="nfs", feature="smb"))]
    if fs::is_remote(path) {
        return match fs::metadata(path) {
            Ok(metadata) => Ok((metadata.is_file()).then(||path.to_owned())),
            Err(error) if error.kind()==std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        };
    }

    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    {
        return Ok(wasm_path_is_file(path).then_some(path.to_path_buf()));
    }

    // libnx's fsdev mount accepts `sdmc:` paths through open(2), but this
    // custom Horizon std target currently reports false from metadata/is_file
    // for those same paths. Verify the exact shipped filename by opening it;
    // desktop targets retain the case-insensitive directory resolver below.
    #[cfg(target_os = "horizon")]
    {
        return Ok(std::fs::File::open(path).ok().map(|_| path.to_path_buf()));
    }

    #[cfg(all(
        not(target_os = "horizon"),
        not(all(target_arch = "wasm32", target_os = "unknown"))
    ))]
    {
        if let Some(cached) = positive_file_cache_get(path) {
            return Ok(Some(cached));
        }
        if path.is_file() {
            positive_file_cache_insert(path, path);
            return Ok(Some(path.to_path_buf()));
        }
        let Some(resolved) = resolve_windows_case_insensitive_path(path)? else {
            return Ok(None);
        };
        if !resolved.is_file() {
            return Ok(None);
        }
        positive_file_cache_insert(path, &resolved);
        Ok(Some(resolved))
    }
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
    match fs::File::open(&resolved) {
        Ok(file) => Ok(file),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            invalidate_game_path_cache(path);
            invalidate_game_path_cache(&resolved);
            let Some(re_resolved) = resolve_windows_case_insensitive_file(path)? else {
                bail!("file not found: {}", path.display());
            };
            Ok(fs::File::open(re_resolved)?)
        }
        Err(err) => Err(err.into()),
    }
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
        match fs::metadata(&resolved) {
            Ok(metadata) => Ok(metadata.len()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                invalidate_game_path_cache(path);
                invalidate_game_path_cache(&resolved);
                let Some(re_resolved) = resolve_windows_case_insensitive_file(path)? else {
                    bail!("file not found: {}", path.display());
                };
                Ok(fs::metadata(re_resolved)?.len())
            }
            Err(err) => Err(err.into()),
        }
    }
}

fn first_existing_file_windows_ci(
    candidates: impl IntoIterator<Item = PathBuf>,
) -> Result<Option<PathBuf>> {
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

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
static PROJECT_EXE_KEY_CACHE: std::sync::OnceLock<
    std::sync::Mutex<HashMap<PathBuf, Option<[u8; 16]>>>,
> = std::sync::OnceLock::new();

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn project_exe_key_cache() -> &'static std::sync::Mutex<HashMap<PathBuf, Option<[u8; 16]>>> {
    PROJECT_EXE_KEY_CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn cache_project_exe_key(project_dir: &Path, key: Option<[u8; 16]>) -> Option<[u8; 16]> {
    let mut cache = project_exe_key_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.insert(project_dir.to_path_buf(), key);
    key
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn cached_project_exe_key(project_dir: &Path) -> Option<Option<[u8; 16]>> {
    let cache = project_exe_key_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.get(project_dir).copied()
}

#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
fn resolve_project_exe_key(project_dir: &Path) -> Option<[u8; 16]> {
    if let Some(cached) = cached_project_exe_key(project_dir) {
        return cached;
    }

    // Read only the 16-byte executable key here. An unrelated malformed
    // optional setting in key.toml must not make a valid key look absent.
    // Full Gameexe options are built later and have their own non-fatal
    // fallback path.
    let configured_key = match siglus_assets::key_toml::load_key16_from_project_dir(project_dir) {
        Ok(key) => key,
        Err(err) => {
            log::error!(
                "failed to load configured Siglus EXE key from {}: {:#}; attempting resource key recovery",
                project_dir.display(),
                err
            );
            None
        }
    };

    // Resource-only key recovery retains several large Scene.pck working
    // buffers. On Horizon it is both unsuitable for the constrained startup
    // heap and unnecessary for a packaged game that provides key.toml. Trust
    // that explicit key; without one, let the normal decoder report a concrete
    // encrypted-resource error rather than attempting a high-memory crack.
    #[cfg(target_os = "horizon")]
    {
        if configured_key.is_none() {
            log::warn!(
                "no key.toml EXE key under {}; automatic key recovery is disabled on Horizon",
                project_dir.display()
            );
        }
        return cache_project_exe_key(project_dir, configured_key);
    }

    let game_path = match find_initial_gameexe_path(project_dir) {
        Ok(path) => path,
        Err(err) => {
            log::error!(
                "automatic Siglus EXE key recovery could not locate Gameexe.dat under {}: {:#}",
                project_dir.display(),
                err
            );
            return cache_project_exe_key(project_dir, configured_key);
        }
    };
    if !game_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dat"))
    {
        // Plain-text Gameexe.ini does not provide the encrypted resource pair
        // required by the resource-only cracker. Keep the configured behavior.
        return cache_project_exe_key(project_dir, configured_key);
    }
    let scene_path = match find_scene_pck_path(project_dir) {
        Ok(path) => path,
        Err(err) => {
            log::error!(
                "automatic Siglus EXE key recovery could not locate Scene.pck under {}: {:#}",
                project_dir.display(),
                err
            );
            return cache_project_exe_key(project_dir, configured_key);
        }
    };
    let game = match read_file_bytes(&game_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            log::error!(
                "automatic Siglus EXE key recovery failed to read {}: {:#}",
                game_path.display(),
                err
            );
            return cache_project_exe_key(project_dir, configured_key);
        }
    };
    let scene = match read_file_bytes(&scene_path) {
        Ok(bytes) => bytes,
        Err(err) => {
            log::error!(
                "automatic Siglus EXE key recovery failed to read {}: {:#}",
                scene_path.display(),
                err
            );
            return cache_project_exe_key(project_dir, configured_key);
        }
    };

    match siglus_key_recovery::resources_require_exe_key(&game, &scene) {
        Ok(false) => return cache_project_exe_key(project_dir, configured_key),
        Ok(true) => {}
        Err(err) => {
            log::error!(
                "automatic Siglus EXE key recovery could not inspect resource encryption headers in {}: {}; keeping configured key",
                project_dir.display(),
                err
            );
            return cache_project_exe_key(project_dir, configured_key);
        }
    }

    if let Some(key) = configured_key {
        match siglus_key_recovery::check_key(&game, &scene, &key) {
            Ok(siglus_key_recovery::KeyStatus::Accepted) => {
                return cache_project_exe_key(project_dir, Some(key));
            }
            Ok(siglus_key_recovery::KeyStatus::Unverifiable) => {
                // Easy-link packs keep their scene chunks uncompressed, so no
                // compressed-resource structure exists to prove or disprove the
                // key. The crack cannot converge on them either; keep the
                // configured key instead of spending minutes on a futile run.
                log::info!(
                    "Siglus resources under {} store uncompressed scene chunks; keeping the configured EXE key without recovery",
                    project_dir.display()
                );
                return cache_project_exe_key(project_dir, Some(key));
            }
            Ok(siglus_key_recovery::KeyStatus::Mismatch) => {
                log::error!(
                    "configured Siglus EXE key under {} failed resource validation; attempting automatic recovery",
                    project_dir.display()
                );
            }
            Err(err) => {
                log::error!(
                    "configured Siglus EXE key under {} could not be validated: {}; attempting automatic recovery",
                    project_dir.display(),
                    err
                );
            }
        }
    } else {
        log::warn!(
            "no usable Siglus EXE key configured under {}; attempting automatic resource recovery",
            project_dir.display()
        );
    }

    // The resource crack needs the LZSS container headers of the scene chunks.
    // An easy-link pack has none, and its uncompressed chunks are exactly what
    // the loader will use, so a crack that cannot converge would only delay
    // the boot.
    match siglus_key_recovery::scene_pack_is_compressed(&scene) {
        Ok(true) => {}
        Ok(false) => {
            log::warn!(
                "Scene.pck under {} stores uncompressed scene chunks; skipping automatic EXE key recovery",
                project_dir.display()
            );
            return cache_project_exe_key(project_dir, configured_key);
        }
        Err(err) => {
            log::error!(
                "could not inspect the Scene.pck storage form under {}: {}",
                project_dir.display(),
                err
            );
        }
    }

    let recovered = match siglus_key_recovery::recover_key_from_resources(&game, &scene) {
        Ok(key) => key,
        Err(err) => {
            log::error!(
                "automatic Siglus EXE key recovery failed for {}: {}; continuing with the configured key state",
                project_dir.display(),
                err
            );
            return cache_project_exe_key(project_dir, configured_key);
        }
    };

    log::warn!(
        "recovered and hard-validated a Siglus EXE key for {}",
        project_dir.display()
    );

    match siglus_assets::key_toml::write_key16_to_project_dir(project_dir, recovered) {
        Ok(path) => {
            invalidate_game_path_cache(&path);
            log::info!("stored recovered Siglus EXE key in {}", path.display());
        }
        Err(err) => {
            // Persistence is optional. Keep the recovered key in the process
            // cache so the current game can continue; the next launch will try
            // recovery again if the directory is still not writable.
            log::error!(
                "recovered Siglus EXE key for {} but could not write key.toml: {:#}; using the recovered key in memory for this process",
                project_dir.display(),
                err
            );
        }
    }

    cache_project_exe_key(project_dir, Some(recovered))
}

pub fn load_scene_pck_decode_options(
    project_dir: &Path,
) -> Result<siglus_assets::scene_pck::ScenePckDecodeOptions> {
    #[cfg(any(
        all(target_arch = "wasm32", target_os = "unknown"),
        target_os = "horizon"
    ))]
    {
        // Horizon must not enter the desktop resource-key recovery path: it
        // retains large Scene.pck working buffers before the pack itself is
        // loaded. `key.toml` is included with the deployment bundle and the
        // assets crate now opens that exact file directly on fsdev mounts.
        return siglus_assets::scene_pck::ScenePckDecodeOptions::from_project_dir(project_dir);
    }

    #[cfg(all(
        not(target_os = "horizon"),
        not(all(target_arch = "wasm32", target_os = "unknown"))
    ))]
    {
        let string_encryption_override = load_project_key_toml(project_dir)?
            .map(|cfg| cfg.override_string_encryption)
            .unwrap_or_default();
        Ok(siglus_assets::scene_pck::ScenePckDecodeOptions {
            exe_angou_element: resolve_project_exe_key(project_dir).map(|key| key.to_vec()),
            easy_angou_code: Some(siglus_assets::keys::SCENE_KEY.to_vec()),
            string_encryption_override,
        })
    }
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
        let recovered_or_configured = resolve_project_exe_key(project_dir);
        match siglus_assets::gameexe::GameexeDecodeOptions::from_project_dir(project_dir) {
            Ok(mut opt) => {
                opt.exe_key16 = recovered_or_configured;
                Ok(opt)
            }
            Err(err) => {
                // A malformed/unreadable key.toml must not make successful
                // resource recovery unusable. Fall back to the normal built-in
                // Gameexe mask and the in-memory recovered key.
                log::error!(
                    "failed to build Gameexe decode options from key.toml under {}: {:#}; using built-in defaults with the resolved EXE key",
                    project_dir.display(),
                    err
                );
                let mut opt = siglus_assets::gameexe::GameexeDecodeOptions {
                    exe_key16: recovered_or_configured,
                    game_angou_code: Some(siglus_assets::keys::GAMEEXE_KEY.to_vec()),
                    ..Default::default()
                };

                Ok(opt)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SelectIniAppendEntry {
    pub dir: String,
    pub name: String,
}

/// Return the append selected by the original startup path.
///
/// `tnm_init_dir()` initializes `Gp_dir->append_dir/append_name` from the first
/// `Select.ini` entry unless a command-line override is present.  The Rust host
/// currently has no `select_scene_pck` CLI equivalent, so startup always uses
/// this first entry.  Missing `Select.ini` is equivalent to an empty append.
pub(crate) fn initial_select_ini_append(project_dir: &Path) -> SelectIniAppendEntry {
    parse_select_ini_append_entries(project_dir)
        .into_iter()
        .next()
        .unwrap_or(SelectIniAppendEntry {
            dir: String::new(),
            name: String::new(),
        })
}

/// Resolve the one active Scene.pck used by the original lexer.
///
/// The C++ path is `Gp_dir->exe_dir + "\\" + Gp_dir->append_dir +
/// "\\Scene.pck"`; it does not scan later append entries when a scene name is
/// missing.  Keeping the active append explicit is important because save/load
/// and return-to-menu can legitimately switch Scene.pck.
pub fn find_scene_pck_path_for_append(project_dir: &Path, append_dir: &str) -> Result<PathBuf> {
    let mut base = project_dir.to_path_buf();
    if !append_dir.is_empty() {
        base = base.join(append_dir);
    }
    let candidate = base.join("Scene.pck");
    if let Some(path) = resolve_windows_case_insensitive_file(&candidate)? {
        return Ok(path);
    }

    // Historical extracted layouts used by this port sometimes place the root
    // Scene.pck under Data/. Preserve that compatibility only for the empty
    // append; an explicit Select.ini append must remain exact like C++.
    if append_dir.is_empty() {
        let legacy = project_dir.join("Data").join("Scene.pck");
        if let Some(path) = resolve_windows_case_insensitive_file(&legacy)? {
            return Ok(path);
        }
    }

    bail!(
        "Scene.pck not found for append {:?} under {}",
        append_dir,
        project_dir.display()
    )
}

pub fn find_scene_pck_path(project_dir: &Path) -> Result<PathBuf> {
    let append = initial_select_ini_append(project_dir);
    find_scene_pck_path_for_append(project_dir, &append.dir)
}

const GAMEEXE_CANDIDATES: &[&str] = &[
    "Gameexe.dat",
    "Gameexe.ini",
    "gameexe.dat",
    "gameexe.ini",
    "GameexeEN.dat",
    "GameexeEN.ini",
    "GameexeZH.dat",
    "GameexeZH.ini",
    "GameexeZHTW.dat",
    "GameexeZHTW.ini",
    "GameexeDE.dat",
    "GameexeDE.ini",
    "GameexeES.dat",
    "GameexeES.ini",
    "GameexeFR.dat",
    "GameexeFR.ini",
    "GameexeID.dat",
    "GameexeID.ini",
];

/// Resolve the Gameexe file used by the original startup path.
///
/// `C_tnm_ini::analize_func()` opens
/// `module_dir + Select.ini.first_dir() + LCL_GAMEEXE_DAT`; Gameexe is therefore
/// selected from the same initial append as Scene.pck, not independently from
/// the project root. Localized candidate names are retained for this port.
pub fn find_initial_gameexe_path(project_dir: &Path) -> Result<PathBuf> {
    let append = initial_select_ini_append(project_dir);
    let mut base = project_dir.to_path_buf();
    if !append.dir.is_empty() {
        base = base.join(&append.dir);
    }
    for name in GAMEEXE_CANDIDATES {
        let candidate = base.join(name);
        if let Some(path) = resolve_windows_case_insensitive_file(&candidate)? {
            return Ok(path);
        }
    }
    bail!(
        "Gameexe.dat not found for initial append {:?} under {}",
        append.dir,
        project_dir.display()
    )
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
        if let Some(path) = resolve_windows_case_insensitive_file(p)?
            && movie_type_from_path(&path)? == MovieType::Omv
        {
            return Ok(path);
        }
        bail!("omv movie not found: {file_name}");
    }

    if p.components().count() > 1 {
        let candidate = project_dir.join(p);
        if let Some(candidate) = resolve_windows_case_insensitive_file(&candidate)?
            && movie_type_from_path(&candidate)? == MovieType::Omv
        {
            return Ok(candidate);
        }
    }

    let (stem, explicit_ext) = split_name_ext(file_name);
    if let Some(ext) = explicit_ext
        && !ext.eq_ignore_ascii_case("omv")
    {
        bail!("object movie requires .omv: {file_name}");
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

/// Per-project append-dir list cache. `Select.ini` is configuration state, not
/// a live resource directory; parsing it once per concrete project root avoids
/// repeated filesystem work without turning file-existence misses into global
/// truths.
static APPEND_DIRS_CACHE: std::sync::Mutex<Option<HashMap<PathBuf, Vec<String>>>> =
    std::sync::Mutex::new(None);

pub(crate) fn ordered_append_dirs(project_dir: &Path, current_append_dir: &str) -> Vec<String> {
    #[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
    let project_key = native_cache_key(project_dir);
    #[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
    let project_key = project_dir.to_path_buf();

    let mut dirs = {
        let mut guard = APPEND_DIRS_CACHE
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let cache = guard.get_or_insert_with(HashMap::new);
        if let Some(cached_dirs) = cache.get(&project_key) {
            cached_dirs.clone()
        } else {
            let dirs = parse_select_ini_append_dirs(project_dir);
            cache.insert(project_key, dirs.clone());
            dirs
        }
    };
    if dirs.is_empty() {
        dirs.push(String::new());
    }

    if current_append_dir.is_empty() {
        return dirs;
    }

    if let Some(pos) = dirs
        .iter()
        .position(|d| d.eq_ignore_ascii_case(current_append_dir))
    {
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
        if !fs::metadata(&dat_dir).is_ok_and(|m|m.is_dir()) {
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
                    log::warn!("Emote key preload: cannot stat {}: {}", path.display(), err);
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

fn parse_select_ini_append_entries(project_dir: &Path) -> Vec<SelectIniAppendEntry> {
    let candidates = [
        project_dir.join("Select.ini"),
        project_dir.join("select.ini"),
    ];
    let path = match first_existing_file_windows_ci(candidates) {
        Ok(Some(path)) => path,
        Ok(None) | Err(_) => {
            return vec![SelectIniAppendEntry {
                dir: String::new(),
                name: String::new(),
            }];
        }
    };

    let Ok(bytes) = read_file_bytes(&path) else {
        return vec![SelectIniAppendEntry {
            dir: String::new(),
            name: String::new(),
        }];
    };
    let text = match String::from_utf8(bytes.clone()) {
        Ok(text) => text,
        Err(_) => {
            let (text, _, _) = encoding_rs::SHIFT_JIS.decode(&bytes);
            text.into_owned()
        }
    };

    let mut out = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let mut cols = line.split('\t');
        let dir = cols.next().unwrap_or("");
        let name = cols.next().unwrap_or("");
        if cols.next().is_some() {
            continue;
        }
        out.push(SelectIniAppendEntry {
            dir: dir.to_string(),
            name: name.to_string(),
        });
    }

    if out.is_empty() {
        out.push(SelectIniAppendEntry {
            dir: String::new(),
            name: String::new(),
        });
    }
    out
}

fn parse_select_ini_append_dirs(project_dir: &Path) -> Vec<String> {
    parse_select_ini_append_entries(project_dir)
        .into_iter()
        .map(|entry| entry.dir)
        .collect()
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
    if let Some((a, b)) = name.rsplit_once('.')
        && !a.is_empty()
        && !b.is_empty()
    {
        return (a, Some(b));
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
    let Some(pos) = dirs
        .iter()
        .position(|d| d.eq_ignore_ascii_case(current_append_dir))
    else {
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
        let candidate =
            base_in_append(project_dir, &append_dir, "dat").join(format!("{file_name}.psb"));
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
