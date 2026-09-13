//! Emote PSB key preload policy for native hosts.
//!
//! The original engine resolves Emote models through `tnm_find_psb`, which only
//! searches the `dat` directory of Select.ini append entries.  Keep key discovery
//! within that same resource scope: never recursively scan the loose game tree,
//! and never start a DWORD bruteforce from `OBJECT.CREATE_EMOTE` in the live frame
//! loop.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use eluna::{
    bruteforce_emote_key, normalize_psb_input, PsbBruteforceOptions, PsbDecryptionKey, PsbError,
    PsbFile, PsbHeader, PsbNormalizeOptions,
};

#[derive(Debug)]
enum ProbeOutcome {
    NoEncryptedPsb,
    ValidatedKnownKey { path: PathBuf },
    NeedsKey { path: PathBuf },
}

/// Resolve the project Emote DWORD before scene execution starts.
///
/// Priority:
/// 1. `key.toml: emote_key`, validated against an encrypted PSB when one exists;
/// 2. fallback user-cache key from an earlier run where `key.toml` was not writable;
/// 3. one Eluna DWORD bruteforce against the smallest encrypted `dat/*.psb` candidate.
///
/// A newly discovered key is written back to `key.toml`.  Only if that write fails
/// is the fallback user cache used.  Failures are logged and do not abort engine
/// bootstrap; later Emote loading can report the missing/invalid key precisely.
pub fn preload_emote_key(project_dir: &Path) -> Option<u32> {
    let configured = match siglus_assets::key_toml::load_emote_key_from_project_dir(project_dir) {
        Ok(key) => key,
        Err(err) => {
            log::warn!(
                "Emote key preload: failed to read {}/key.toml: {:#}",
                project_dir.display(),
                err
            );
            None
        }
    };

    let cache_path = fallback_cache_path(project_dir);
    let cached = if configured.is_none() {
        cache_path
            .as_deref()
            .and_then(load_cached_key_best_effort)
    } else {
        None
    };
    let known_key = configured.or(cached);

    let candidates = match crate::resource::find_emote_psb_candidates(project_dir) {
        Ok(candidates) => candidates,
        Err(err) => {
            log::warn!(
                "Emote key preload: failed to enumerate append dat/*.psb candidates: {:#}",
                err
            );
            return known_key;
        }
    };

    if candidates.is_empty() {
        if let Some(key) = known_key {
            log::info!(
                "Emote key preload: no dat/*.psb candidates found; retaining configured/cached key 0x{key:08X}"
            );
        }
        return known_key;
    }

    let probe = match probe_candidates(&candidates, known_key) {
        Ok(probe) => probe,
        Err(err) => {
            log::warn!("Emote key preload: PSB probe failed: {:#}", err);
            return known_key;
        }
    };

    match probe {
        ProbeOutcome::NoEncryptedPsb => {
            if let Some(key) = known_key {
                log::info!(
                    "Emote key preload: candidate PSBs are plain/unrelated; retaining known key 0x{key:08X}"
                );
            }
            known_key
        }
        ProbeOutcome::ValidatedKnownKey { path } => {
            let key = known_key.expect("validated key outcome requires a known key");
            if configured.is_some() {
                log::info!(
                    "Emote key preload: key.toml emote_key 0x{key:08X} validated with {}",
                    path.display()
                );
            } else {
                log::info!(
                    "Emote key preload: cached emote_key 0x{key:08X} validated with {}",
                    path.display()
                );
                // A fallback cache only exists because an earlier key.toml write
                // failed.  Retry the preferred project-local persistence on every
                // validated startup so permission changes heal automatically.
                persist_recovered_key(project_dir, key, cache_path.as_deref());
            }
            Some(key)
        }
        ProbeOutcome::NeedsKey { path } => {
            if let Some(key) = known_key {
                log::warn!(
                    "Emote key preload: known emote_key 0x{key:08X} did not validate against encrypted candidates; recovering from {}",
                    path.display()
                );
            } else {
                log::info!(
                    "Emote key preload: encrypted Emote PSB detected at {}; starting one-time DWORD recovery before scene execution",
                    path.display()
                );
            }
            recover_key_from_probe(project_dir, &path, cache_path.as_deref())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CandidateEncryption {
    Plain,
    NeedsKey,
    FullProbe,
}

/// Classify the common raw-PSB case with two tiny reads instead of loading the
/// whole file.  This mirrors Eluna's current `apply_psb_decryption` boundary:
/// an explicit encryption flag requires a key, and legacy body-only variants
/// are treated as encrypted when the root byte is not an OBJECT tag. Wrapped
/// MDF/LZ4 inputs fall back to Eluna's normalizer because the inner PSB header
/// is not directly seekable.
fn classify_candidate_file(path: &Path) -> std::io::Result<CandidateEncryption> {
    const HEADER_PROBE_LEN: usize = 0x2c;
    const PSB_OBJECT_ROOT_TAG: u8 = 0x21;

    let mut file = File::open(path)?;
    let mut header_bytes = [0u8; HEADER_PROBE_LEN];
    let read = file.read(&mut header_bytes)?;
    let header_bytes = &header_bytes[..read];

    // Raw PSB signature. MDF/LZ4 and unrelated data are delegated to Eluna so
    // this host layer never duplicates wrapper decoding.
    if header_bytes.len() < 4 || &header_bytes[..4] != b"PSB\0" {
        return Ok(CandidateEncryption::FullProbe);
    }

    let header = match PsbHeader::read(header_bytes) {
        Ok(header) => header,
        Err(_) => return Ok(CandidateEncryption::FullProbe),
    };
    if header.has_encryption_flag() {
        return Ok(CandidateEncryption::NeedsKey);
    }

    let file_len = file.metadata()?.len();
    let root_offset = u64::from(header.root_offset);
    if root_offset >= file_len {
        return Ok(CandidateEncryption::FullProbe);
    }
    file.seek(SeekFrom::Start(root_offset))?;
    let mut root = [0u8; 1];
    file.read_exact(&mut root)?;
    Ok(if root[0] == PSB_OBJECT_ROOT_TAG {
        CandidateEncryption::Plain
    } else {
        CandidateEncryption::NeedsKey
    })
}

fn probe_candidates(candidates: &[PathBuf], known_key: Option<u32>) -> anyhow::Result<ProbeOutcome> {
    let mut first_encrypted: Option<PathBuf> = None;

    for path in candidates {
        let classification = match classify_candidate_file(path) {
            Ok(classification) => classification,
            Err(err) => {
                log::warn!(
                    "Emote key preload: cannot inspect candidate {}: {}",
                    path.display(),
                    err
                );
                continue;
            }
        };
        if classification == CandidateEncryption::Plain {
            continue;
        }

        let data = match fs::read(path) {
            Ok(data) => data,
            Err(err) => {
                log::warn!(
                    "Emote key preload: cannot read candidate {}: {}",
                    path.display(),
                    err
                );
                continue;
            }
        };

        if classification == CandidateEncryption::FullProbe {
            match normalize_psb_input(&data, &PsbNormalizeOptions::default()) {
                Ok(_) => continue,
                Err(PsbError::EncryptedPsbRequiresKey) => {}
                Err(err) => {
                    // `dat` can contain PSBs unrelated to Emote. Do not interpret
                    // an arbitrary parse failure as encryption/bruteforce input.
                    log::debug!(
                        "Emote key preload: skipping non-probe PSB {}: {}",
                        path.display(),
                        err
                    );
                    continue;
                }
            }
        }

        if first_encrypted.is_none() {
            first_encrypted = Some(path.clone());
        }

        if let Some(key) = known_key {
            if validate_key_bytes(&data, key) {
                return Ok(ProbeOutcome::ValidatedKnownKey { path: path.clone() });
            }
            // A corrupt/unrelated encrypted PSB should not invalidate a known
            // project key by itself. Scan remaining encrypted candidates for one
            // that validates before deciding the key is stale.
            continue;
        }

        // Without a known key, use the first encrypted candidate. Candidates are
        // already ordered by file size to minimize full-check/decrypt work.
        return Ok(ProbeOutcome::NeedsKey { path: path.clone() });
    }

    Ok(match first_encrypted {
        Some(path) => ProbeOutcome::NeedsKey { path },
        None => ProbeOutcome::NoEncryptedPsb,
    })
}

fn validate_key_bytes(data: &[u8], key: u32) -> bool {
    let options = PsbNormalizeOptions {
        decrypt_key: Some(PsbDecryptionKey::emote_key(key)),
        ..PsbNormalizeOptions::default()
    };
    PsbFile::parse_normalized(data, &options).is_ok()
}

fn recover_key_from_probe(
    project_dir: &Path,
    probe_path: &Path,
    cache_path: Option<&Path>,
) -> Option<u32> {
    let data = match fs::read(probe_path) {
        Ok(data) => data,
        Err(err) => {
            log::error!(
                "Emote key preload: cannot read recovery probe {}: {}",
                probe_path.display(),
                err
            );
            return None;
        }
    };

    let result = match bruteforce_emote_key(&data, PsbBruteforceOptions::default()) {
        Ok(result) => result,
        Err(err) => {
            log::error!(
                "Emote key preload: Eluna DWORD recovery failed for {}: {}",
                probe_path.display(),
                err
            );
            return None;
        }
    };
    let Some(result) = result else {
        log::error!(
            "Emote key preload: {} was classified as encrypted but Eluna found no DWORD key",
            probe_path.display()
        );
        return None;
    };

    let key = result.key;
    if !validate_key_bytes(&data, key) {
        // Eluna's bruteforce already performs a full PSB parse confirmation. Keep
        // this host-side check as a hard boundary before persisting the key.
        log::error!(
            "Emote key preload: recovered key 0x{key:08X} failed host validation for {}",
            probe_path.display()
        );
        return None;
    }

    log::info!(
        "Emote key preload: recovered emote_key=0x{key:08X} from {} after testing {} candidates",
        probe_path.display(),
        result.tested_keys
    );
    persist_recovered_key(project_dir, key, cache_path);
    Some(key)
}

fn persist_recovered_key(project_dir: &Path, key: u32, cache_path: Option<&Path>) {
    match siglus_assets::key_toml::write_emote_key_to_project_dir(project_dir, key) {
        Ok(path) => {
            log::info!(
                "Emote key preload: persisted emote_key=0x{key:08X} to {}",
                path.display()
            );
            if let Some(cache_path) = cache_path {
                let _ = fs::remove_file(cache_path);
            }
            return;
        }
        Err(err) => {
            log::warn!(
                "Emote key preload: could not update {}/key.toml: {:#}; using fallback cache",
                project_dir.display(),
                err
            );
        }
    }

    let Some(cache_path) = cache_path else {
        log::warn!(
            "Emote key preload: no writable platform cache location; recovered key remains in memory for this run only"
        );
        return;
    };
    let Some(parent) = cache_path.parent() else {
        return;
    };
    if let Err(err) = fs::create_dir_all(parent) {
        log::warn!(
            "Emote key preload: cannot create fallback cache {}: {}",
            parent.display(),
            err
        );
        return;
    }
    match siglus_assets::key_toml::write_emote_key_to_file(cache_path, key) {
        Ok(()) => log::info!(
            "Emote key preload: cached emote_key=0x{key:08X} at {}",
            cache_path.display()
        ),
        Err(err) => log::warn!(
            "Emote key preload: cannot persist fallback cache {}: {:#}",
            cache_path.display(),
            err
        ),
    }
}

fn load_cached_key_best_effort(path: &Path) -> Option<u32> {
    if !path.is_file() {
        return None;
    }
    match siglus_assets::key_toml::load_emote_key_from_file(path) {
        Ok(key) => key,
        Err(err) => {
            log::warn!(
                "Emote key preload: ignoring invalid fallback cache {}: {:#}",
                path.display(),
                err
            );
            None
        }
    }
}

fn fallback_cache_path(project_dir: &Path) -> Option<PathBuf> {
    let root = if let Some(path) = std::env::var_os("SIGLUS_CACHE_DIR") {
        PathBuf::from(path)
    } else {
        platform_cache_root()?
    };
    Some(
        root.join("emote_keys")
            .join(format!("{:016x}.toml", project_cache_id(project_dir))),
    )
}

fn project_cache_id(project_dir: &Path) -> u64 {
    let path = fs::canonicalize(project_dir).unwrap_or_else(|_| project_dir.to_path_buf());
    // Stable FNV-1a instead of DefaultHasher, whose algorithm is deliberately
    // unspecified and could move the cache path across Rust releases.
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in path.to_string_lossy().as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

#[cfg(target_os = "windows")]
fn platform_cache_root() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .map(|path| path.join("siglus_rs").join("cache"))
}

#[cfg(target_os = "macos")]
fn platform_cache_root() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|path| path.join("Library").join("Caches").join("siglus_rs"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn platform_cache_root() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
        return Some(PathBuf::from(path).join("siglus_rs"));
    }
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|path| path.join(".cache").join("siglus_rs"))
}

#[cfg(not(any(windows, unix)))]
fn platform_cache_root() -> Option<PathBuf> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_cache_id_is_stable_for_same_path() {
        let path = Path::new("/tmp/siglus-emote-key-test");
        assert_eq!(project_cache_id(path), project_cache_id(path));
    }

    #[test]
    fn command_context_preloads_emote_key_from_fallback_cache() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let project = std::env::temp_dir().join(format!("siglus-emote-bootstrap-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&project).unwrap();
        let Some(cache) = fallback_cache_path(&project) else {
            fs::remove_dir(&project).unwrap();
            return;
        };
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(&cache, "emote_key = 0x1234ABCD\n").unwrap();
        let ctx = crate::runtime::CommandContext::new(project.clone());
        let loaded = ctx.emote_key;
        drop(ctx);
        fs::remove_file(cache).unwrap();
        fs::remove_dir_all(project).unwrap();
        assert_eq!(loaded, Some(0x1234ABCD));
    }
}
