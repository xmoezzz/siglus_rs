//! Gameexe-driven table loading.
//!
//! This module is **best-effort**: failures are recorded in `UnknownOpRecorder`
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

        let gameexe_path = project_dir.join("Gameexe.dat");
        if !gameexe_path.is_file() {
            return out;
        }

        let raw = match std::fs::read(&gameexe_path) {
            Ok(b) => b,
            Err(e) => {
                unknown.record_unimplemented(&format!("gameexe.read.failed:{e}"));
                return out;
            }
        };

        let opt = match GameexeDecodeOptions::from_env() {
            Ok(v) => v,
            Err(e) => {
                // Keep going with defaults.
                unknown.record_unimplemented(&format!("gameexe.env.invalid:{e}"));
                GameexeDecodeOptions::default()
            }
        };

        let (text, report) = match decode_gameexe_dat_bytes(&raw, &opt) {
            Ok(v) => v,
            Err(e) => {
                unknown.record_unimplemented(&format!("gameexe.decode.failed:{e}"));
                return out;
            }
        };

        let cfg = GameexeConfig::from_text(&text);
        out.gameexe_report = Some(report);
        out.gameexe = Some(cfg);

        // Drive table loading from the parsed config.
        let dat_dir = project_dir.join("dat");
        if !dat_dir.is_dir() {
            unknown.record_unimplemented("dat.dir.missing");
            return out;
        }

        if let Some(cfg) = out.gameexe.as_ref() {
            // CGTABLE
            if let Some(v) = cfg.get("CGTABLE_FILE") {
                if let Some(path) = resolve_table_path(&dat_dir, v, Some("cgm")) {
                    match CgTableData::load(&path) {
                        Ok(t) => out.cgtable = Some(t),
                        Err(e) => unknown.record_unimplemented(&format!("cgtable.load.failed:{path:?}:{e}")),
                    }
                }
            }

            // CGTABLE_FLAG_CNT
            if let Some(v) = cfg.get("CGTABLE_FLAG_CNT") {
                if let Ok(n) = v.trim().parse::<usize>() {
                    out.cgtable_flag_cnt = Some(n);
                    out.cg_flags = vec![0u8; n.max(32)];
                }
            }

            // THUMBTABLE
            if let Some(v) = cfg.get("THUMBTABLE_FILE") {
                if let Some(path) = resolve_table_path(&dat_dir, v, Some("dat")) {
                    match ThumbTable::load(&path) {
                        Ok(t) => out.thumb_table = Some(t),
                        Err(e) => unknown.record_unimplemented(&format!("thumb_table.load.failed:{path:?}:{e}")),
                    }
                }
            }

            // DATABASE
            let db_cnt = cfg
                .get("DATABASE.CNT")
                .and_then(|s| s.trim().parse::<usize>().ok())
                .unwrap_or(0);

            for i in 0..db_cnt {
                let key = format!("DATABASE.{i}");
                let Some(name) = cfg.get(&key) else {
                    unknown.record_unimplemented(&format!("database.name.missing:{key}"));
                    continue;
                };
                let Some(path) = resolve_table_path(&dat_dir, name, Some("dbs")) else {
                    continue;
                };
                match DbsDatabase::load(&path) {
                    Ok(db) => out.databases.push(db),
                    Err(e) => unknown.record_unimplemented(&format!("dbs.load.failed:{path:?}:{e}")),
                }
            }
        }

        out
    }
}

fn resolve_table_path(dat_dir: &Path, raw: &str, default_ext: Option<&str>) -> Option<PathBuf> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }

    let mut p = PathBuf::from(s);
    if p.extension().is_none() {
        if let Some(ext) = default_ext {
            p.set_extension(ext);
        }
    }

    // All resources are relative to `dat/` in the current bring-up.
    let joined = dat_dir.join(p);
    if joined.is_file() {
        Some(joined)
    } else {
        None
    }
}
