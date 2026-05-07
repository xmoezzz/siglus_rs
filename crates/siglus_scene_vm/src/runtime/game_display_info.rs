//! Game display information shared by bundle/mobile platform UI code.
//!
//! This module intentionally keeps cover discovery outside the VM semantics.
//! Game scripts do not observe this data. Platform launchers and bundle UIs can
//! call it to decide whether to show a cover image or a text title.

use std::path::{Path, PathBuf};

use super::game_title;

const COVER_CANDIDATES: &[(&str, &str)] = &[
    ("cover.png", "image/png"),
    ("cover.jpg", "image/jpeg"),
    ("cover.jpeg", "image/jpeg"),
    ("thumbnail.png", "image/png"),
    ("icon.png", "image/png"),
];

#[derive(Clone, Debug)]
pub struct GameDisplayInfo {
    /// Display name for platform UI. This is the Gameexe `GAMENAME` when
    /// available, otherwise the game directory name, otherwise `Siglus`.
    pub name: String,
    /// Compatibility alias for existing callers that still use title wording.
    pub title: String,
    /// Optional cover discovered from the game directory.
    pub cover: Option<GameCover>,
}

#[derive(Clone, Debug)]
pub struct GameCover {
    pub bytes: Vec<u8>,
    pub mime: String,
    pub source_path: PathBuf,
}

/// Return the display name for a game directory.
///
/// This is a naming alias for `resolve_game_title_from_project_dir` so bundle
/// and mobile UI code can use game-name terminology without duplicating title
/// parsing logic.
pub fn resolve_game_name_from_project_dir(project_dir: impl AsRef<Path>) -> String {
    game_title::resolve_game_title_from_project_dir(project_dir)
}

/// Return display metadata for bundle/mobile UI.
///
/// Cover lookup is intentionally simple and explicit. If the game directory has
/// one of the conventional files below, the first existing file is returned:
///
/// - `cover.png`
/// - `cover.jpg`
/// - `cover.jpeg`
/// - `thumbnail.png`
/// - `icon.png`
///
/// If no cover file exists or the file cannot be read, `cover` is `None` and UI
/// callers should display `name` instead.
pub fn resolve_game_display_info_from_project_dir(
    project_dir: impl AsRef<Path>,
) -> GameDisplayInfo {
    let project_dir = project_dir.as_ref();
    let name = resolve_game_name_from_project_dir(project_dir);
    let cover = resolve_game_cover_from_project_dir(project_dir);
    GameDisplayInfo {
        title: name.clone(),
        name,
        cover,
    }
}

pub fn resolve_game_cover_from_project_dir(project_dir: impl AsRef<Path>) -> Option<GameCover> {
    let project_dir = project_dir.as_ref();
    for (file_name, mime) in COVER_CANDIDATES {
        let path = project_dir.join(file_name);
        if !path.is_file() {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        return Some(GameCover {
            bytes,
            mime: (*mime).to_string(),
            source_path: path,
        });
    }
    None
}
