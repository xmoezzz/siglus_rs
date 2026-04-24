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
const INIDEF_BTN_SE_CNT: usize = 16;
const INIMAX_BTN_SE_CNT: usize = 256;
const INIDEF_BTN_ACTION_CNT: usize = 16;
const INIMAX_BTN_ACTION_CNT: usize = 256;
const TNM_BTN_STATE_MAX: usize = 5;
const INIDEF_SE_CNT: usize = 16;
const INIMAX_SE_CNT: usize = 256;

#[derive(Debug, Clone, Copy)]
pub struct ButtonSeTemplate {
    pub hit_no: i64,
    pub push_no: i64,
    pub decide_no: i64,
}

impl Default for ButtonSeTemplate {
    fn default() -> Self {
        Self { hit_no: 0, push_no: -1, decide_no: 1 }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ButtonActionPattern {
    pub rep_pat_no: i64,
    pub rep_pos_x: i64,
    pub rep_pos_y: i64,
    pub rep_tr: i64,
    pub rep_bright: i64,
    pub rep_dark: i64,
}

impl Default for ButtonActionPattern {
    fn default() -> Self {
        Self {
            rep_pat_no: 0,
            rep_pos_x: 0,
            rep_pos_y: 0,
            rep_tr: 255,
            rep_bright: 0,
            rep_dark: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ButtonActionTemplate {
    pub state: [ButtonActionPattern; TNM_BTN_STATE_MAX],
}

impl Default for ButtonActionTemplate {
    fn default() -> Self {
        let mut state = [ButtonActionPattern::default(); TNM_BTN_STATE_MAX];
        state[1].rep_bright = 32;
        state[2].rep_bright = 32;
        state[2].rep_pos_x = 1;
        state[2].rep_pos_y = 1;
        Self { state }
    }
}

#[derive(Debug)]
pub struct AssetTables {
    pub gameexe: Option<GameexeConfig>,
    pub gameexe_report: Option<GameexeDecodeReport>,

    pub button_se_templates: Vec<ButtonSeTemplate>,
    pub button_action_templates: Vec<ButtonActionTemplate>,
    pub se_file_names: Vec<Option<String>>,

    pub cgtable: Option<CgTableData>,
    pub cgtable_flag_cnt: Option<usize>,
    pub cg_flags: Vec<u8>,

    pub databases: Vec<DbsDatabase>,

    pub thumb_table: Option<ThumbTable>,
}

impl Default for AssetTables {
    fn default() -> Self {
        Self {
            gameexe: None,
            gameexe_report: None,
            button_se_templates: vec![ButtonSeTemplate::default(); INIDEF_BTN_SE_CNT],
            button_action_templates: vec![ButtonActionTemplate::default(); INIDEF_BTN_ACTION_CNT],
            se_file_names: vec![None; INIDEF_SE_CNT],
            cgtable: None,
            cgtable_flag_cnt: None,
            cg_flags: Vec::new(),
            databases: Vec::new(),
            thumb_table: None,
        }
    }
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
        out.button_se_templates = load_button_se_templates(&cfg);
        out.button_action_templates = load_button_action_templates(&cfg);
        out.se_file_names = load_se_file_names(&cfg);
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



fn load_button_action_templates(cfg: &GameexeConfig) -> Vec<ButtonActionTemplate> {
    let cnt = cfg
        .get_usize("BUTTON.ACTION.CNT")
        .unwrap_or(INIDEF_BTN_ACTION_CNT)
        .min(INIMAX_BTN_ACTION_CNT);
    let mut out = vec![ButtonActionTemplate::default(); cnt];

    const STATES: [(&str, usize); TNM_BTN_STATE_MAX] = [
        ("NORMAL", 0),
        ("HIT", 1),
        ("PUSH", 2),
        ("SELECT", 3),
        ("DISABLE", 4),
    ];

    for i in 0..cnt {
        for (name, state_idx) in STATES {
            let Some(raw) = cfg.get_indexed_field("BUTTON.ACTION", i, name) else {
                continue;
            };
            let nums = parse_button_action_numbers(raw);
            if nums.len() >= 6 {
                out[i].state[state_idx] = ButtonActionPattern {
                    rep_pat_no: nums[0],
                    rep_pos_x: nums[1],
                    rep_pos_y: nums[2],
                    rep_tr: nums[3].clamp(0, 255),
                    rep_bright: nums[4].clamp(0, 255),
                    rep_dark: nums[5].clamp(0, 255),
                };
            }
        }
    }

    out
}

fn parse_button_action_numbers(raw: &str) -> Vec<i64> {
    raw.split(|c: char| c == ',' || c.is_ascii_whitespace())
        .filter(|s| !s.trim().is_empty())
        .filter_map(parse_i64_like_local)
        .collect()
}

fn load_se_file_names(cfg: &GameexeConfig) -> Vec<Option<String>> {
    let cnt = cfg
        .get_usize("SE.CNT")
        .unwrap_or(INIDEF_SE_CNT)
        .min(INIMAX_SE_CNT);
    let mut out = vec![None; cnt];

    for i in 0..cnt {
        if let Some(v) = cfg.get_indexed_unquoted("SE", i) {
            let t = v.trim();
            if !t.is_empty() {
                out[i] = Some(t.to_string());
            }
        }
    }

    out
}

fn load_button_se_templates(cfg: &GameexeConfig) -> Vec<ButtonSeTemplate> {

    let cnt = cfg
        .get_usize("BUTTON.SE.CNT")
        .unwrap_or(INIDEF_BTN_SE_CNT)
        .min(INIMAX_BTN_SE_CNT);
    let mut out = vec![ButtonSeTemplate::default(); cnt];

    for i in 0..cnt {
        if let Some(v) = cfg.get_indexed_field("BUTTON.SE", i, "HIT").and_then(parse_i64_like_local) {
            out[i].hit_no = v;
        }
        if let Some(v) = cfg.get_indexed_field("BUTTON.SE", i, "PUSH").and_then(parse_i64_like_local) {
            out[i].push_no = v;
        }
        if let Some(v) = cfg.get_indexed_field("BUTTON.SE", i, "DECIDE").and_then(parse_i64_like_local) {
            out[i].decide_no = v;
        }
    }

    out
}

fn parse_i64_like_local(s: &str) -> Option<i64> {
    let t = s.trim().trim_matches('"');
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        i64::from_str_radix(hex, 16).ok()
    } else {
        t.parse::<i64>().ok()
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
