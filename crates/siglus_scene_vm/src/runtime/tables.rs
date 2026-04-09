//! Gameexe-driven table loading.
//!
//! This module records asset-table load failures in `UnknownOpRecorder`
//! and do not crash runtime bring-up.
//!
//! NOTE: `TCHAR` in the original engine is UTF-16LE.

use std::path::{Path, PathBuf};

use siglus_assets::{
    cgm::CgTableData,
    dbs::DbsDatabase,
    gameexe::{decode_gameexe_dat_bytes, GameexeConfig, GameexeDecodeOptions, GameexeDecodeReport},
    thumb_table::ThumbTable,
};

use super::unknown::UnknownOpRecorder;

#[derive(Debug, Default)]
pub struct AssetTables {
    pub gameexe: Option<GameexeConfig>,
    pub gameexe_report: Option<GameexeDecodeReport>,

    pub cgtable: Option<CgTableData>,
    pub cgtable_flag_cnt: Option<usize>,
    pub cg_flags: Vec<u8>,

    pub databases: Vec<DbsDatabase>,

    pub thumb_table: Option<ThumbTable>,
}

impl AssetTables {
    pub fn load(project_dir: &Path, unknown: &mut UnknownOpRecorder) -> Self {
        let mut out = Self::default();

        let Some(gameexe_path) = find_gameexe_path(project_dir) else {
            unknown.record_note("gameexe.missing");
            return out;
        };

        let raw = match std::fs::read(&gameexe_path) {
            Ok(b) => b,
            Err(e) => {
                unknown.record_note(&format!("gameexe.read.failed:{e}"));
                return out;
            }
        };

        let opt = match GameexeDecodeOptions::from_project_dir(project_dir) {
            Ok(v) => v,
            Err(e) => {
                // Keep going with defaults.
                unknown.record_note(&format!("gameexe.key_toml.invalid:{e}"));
                GameexeDecodeOptions::default()
            }
        };

        let (text, report) = match decode_gameexe_dat_bytes(&raw, &opt) {
            Ok(v) => v,
            Err(e) => {
                unknown.record_note(&format!("gameexe.decode.failed:{e}"));
                return out;
            }
        };

        let cfg = GameexeConfig::from_text(&text);
        out.gameexe_report = Some(report);
        out.gameexe = Some(cfg);

        // Drive table loading from the parsed config.
        let dat_dir = project_dir.join("dat");
        if !dat_dir.is_dir() {
            unknown.record_note("dat.dir.missing");
        }

        if let Some(cfg) = out.gameexe.as_ref() {
            // CGTABLE
            if let Some(v) = cfg.get_unquoted("CGTABLE_FILE") {
                if let Some(path) = resolve_table_path(project_dir, &dat_dir, v, Some("cgm")) {
                    match CgTableData::load(&path) {
                        Ok(t) => out.cgtable = Some(t),
                        Err(e) => unknown.record_note(&format!("cgtable.load.failed:{path:?}:{e}")),
                    }
                } else {
                    unknown.record_note(&format!("cgtable.path.missing:{v}"));
                }
            }

            // CGTABLE_FLAG_CNT
            if let Some(n) = cfg.get_usize("CGTABLE_FLAG_CNT") {
                out.cgtable_flag_cnt = Some(n);
                out.cg_flags = vec![0u8; n.max(32)];
            }

            // THUMBTABLE
            if let Some(v) = cfg.get_unquoted("THUMBTABLE_FILE") {
                if let Some(path) = resolve_table_path(project_dir, &dat_dir, v, Some("dat")) {
                    match ThumbTable::load(&path) {
                        Ok(t) => out.thumb_table = Some(t),
                        Err(e) => {
                            unknown.record_note(&format!("thumb_table.load.failed:{path:?}:{e}"))
                        }
                    }
                } else {
                    unknown.record_note(&format!("thumb_table.path.missing:{v}"));
                }
            }

            // DATABASE
            let db_cnt = cfg.indexed_count("DATABASE");
            for i in 0..db_cnt {
                let key = format!("DATABASE.{i}");
                let Some(name) = cfg
                    .get_indexed_unquoted("DATABASE", i)
                    .or_else(|| cfg.get_indexed_item_unquoted("DATABASE", i, 0))
                    .or_else(|| cfg.get_indexed_field_unquoted("DATABASE", i, "FILE"))
                    .or_else(|| cfg.get_indexed_field_unquoted("DATABASE", i, "NAME"))
                else {
                    unknown.record_note(&format!("database.name.missing:{key}"));
                    continue;
                };
                let Some(path) = resolve_table_path(project_dir, &dat_dir, name, Some("dbs"))
                else {
                    unknown.record_note(&format!("database.path.missing:{key}:{name}"));
                    continue;
                };
                match DbsDatabase::load(&path) {
                    Ok(db) => out.databases.push(db),
                    Err(e) => unknown.record_note(&format!("dbs.load.failed:{path:?}:{e}")),
                }
            }
        }

        out
    }
}

fn find_gameexe_path(project_dir: &Path) -> Option<PathBuf> {
    const CANDIDATES: &[&str] = &[
        "Gameexe.dat",
        "GameexeEN.dat",
        "GameexeZH.dat",
        "GameexeZHTW.dat",
        "GameexeDE.dat",
        "GameexeES.dat",
        "GameexeFR.dat",
        "GameexeID.dat",
    ];
    for name in CANDIDATES {
        let p = project_dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

fn resolve_table_path(
    project_dir: &Path,
    dat_dir: &Path,
    raw: &str,
    default_ext: Option<&str>,
) -> Option<PathBuf> {
    let s = raw.trim().trim_matches('"');
    if s.is_empty() {
        return None;
    }

    let normalized = s.replace('\\', "/");
    let mut candidates = Vec::new();
    candidates.push(PathBuf::from(&normalized));

    let mut with_ext = PathBuf::from(&normalized);
    if with_ext.extension().is_none() {
        if let Some(ext) = default_ext {
            with_ext.set_extension(ext);
        }
    }
    if with_ext != PathBuf::from(&normalized) {
        candidates.push(with_ext.clone());
    }

    for cand in candidates {
        let direct = if cand.is_absolute() {
            cand.clone()
        } else {
            project_dir.join(&cand)
        };
        if direct.is_file() {
            return Some(direct);
        }

        let file_name = cand.file_name().map(PathBuf::from);
        if let Some(name_only) = file_name {
            let p = dat_dir.join(&name_only);
            if p.is_file() {
                return Some(p);
            }
        }

        let p = dat_dir.join(&cand);
        if p.is_file() {
            return Some(p);
        }

        if let Ok(stripped) = cand.strip_prefix("dat") {
            let p = dat_dir.join(stripped);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}
