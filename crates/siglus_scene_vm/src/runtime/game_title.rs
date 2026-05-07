//! Game title resolution shared by runtime and platform UI integration.
//!
//! The title follows the engine configuration first and falls back to the game
//! directory name. `Gameexe.dat` decoding uses `key.toml` from the project
//! directory through `GameexeDecodeOptions::from_project_dir`.

use std::path::Path;

use siglus_assets::gameexe::{decode_gameexe_dat_bytes, GameexeConfig, GameexeDecodeOptions};

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

/// Return the display title for a game directory.
///
/// This is intended for platform/bundle UI code as well as runtime dialogs. It
/// reads `GAMENAME` from `Gameexe.ini` or `Gameexe.dat` when possible. If the
/// Gameexe file is missing, cannot be decoded, or does not contain a non-empty
/// `GAMENAME`, the returned value is the project directory name. As a final
/// fallback it returns `Siglus`.
pub fn resolve_game_title_from_project_dir(project_dir: impl AsRef<Path>) -> String {
    let project_dir = project_dir.as_ref();
    load_gameexe_config(project_dir)
        .as_ref()
        .and_then(game_title_from_config)
        .unwrap_or_else(|| fallback_title_from_project_dir(project_dir))
}

/// Return the runtime title using an already loaded Gameexe config first.
pub fn resolve_game_title(
    gameexe: Option<&GameexeConfig>,
    project_dir: impl AsRef<Path>,
) -> String {
    let project_dir = project_dir.as_ref();
    gameexe
        .and_then(game_title_from_config)
        .unwrap_or_else(|| fallback_title_from_project_dir(project_dir))
}

/// Extract `GAMENAME` from a parsed Gameexe config.
pub fn game_title_from_config(cfg: &GameexeConfig) -> Option<String> {
    if let Some(v) = cfg.get_unquoted("GAMENAME") {
        let s = normalize_game_title(v);
        if !s.is_empty() {
            return Some(s);
        }
    }
    for entry in cfg.entries.iter().rev() {
        if matches!(entry.key_parts.last().map(|s| s.as_str()), Some("GAMENAME")) {
            let s = normalize_game_title(entry.scalar_unquoted());
            if !s.is_empty() {
                return Some(s);
            }
        }
    }
    None
}

fn load_gameexe_config(project_dir: &Path) -> Option<GameexeConfig> {
    let gameexe_path = find_gameexe_path(project_dir)?;
    let raw = std::fs::read(&gameexe_path).ok()?;
    let text = if gameexe_path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ini"))
    {
        String::from_utf8(raw).ok()?
    } else {
        let opt = GameexeDecodeOptions::from_project_dir(project_dir).ok()?;
        decode_gameexe_dat_bytes(&raw, &opt).ok()?.0
    };
    Some(GameexeConfig::from_text(&text))
}

fn find_gameexe_path(project_dir: &Path) -> Option<std::path::PathBuf> {
    for name in GAMEEXE_CANDIDATES {
        let p = project_dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn normalize_game_title(raw: &str) -> String {
    raw.trim().trim_matches('"').trim().to_string()
}

fn fallback_title_from_project_dir(project_dir: &Path) -> String {
    project_dir
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("Siglus")
        .to_string()
}
