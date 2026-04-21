//! Gameexe.dat decoding and INI-like parsing.
//!
//! In the original Siglus engine, `Gameexe.dat` is treated as a TCHAR text
//! blob (UTF-16LE). Some titles may store an alternative encoding (e.g.
//! Shift-JIS) or wrap the text with obfuscation + LZSS.
//!
//! This module keeps the decoding pipeline explicit:
//! - plaintext (UTF-16LE / Shift-JIS / UTF-8)
//! - optionally XOR with a chain of angou materials
//! - optional Siglus LZSS unpack

use std::collections::BTreeMap;
use std::env;
use std::path::Path;

use anyhow::{anyhow, bail, Result};
use encoding_rs::SHIFT_JIS;

use crate::angou::{xor_cycle_in_place, AngouChain, AngouStep, AngouStepKind};
use crate::lzss::lzss_unpack_lenient;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameexeTextEncoding {
    Utf16Le,
    ShiftJis,
    Utf8,
}

#[derive(Debug, Clone)]
pub struct GameexeDecodeOptions {
    pub exe_key16: Option<[u8; 16]>,
    pub base_angou_code: Option<Vec<u8>>,
    pub game_angou_code: Option<Vec<u8>>,
    pub try_lzss: bool,
    pub chain_order: Vec<AngouStepKind>,
}

impl Default for GameexeDecodeOptions {
    fn default() -> Self {
        Self {
            exe_key16: None,
            base_angou_code: None,
            game_angou_code: None,
            try_lzss: true,
            chain_order: vec![
                AngouStepKind::ExeKey16,
                AngouStepKind::BaseCode,
                AngouStepKind::GameCode,
            ],
        }
    }
}

impl GameexeDecodeOptions {
    pub fn from_project_dir(project_dir: &Path) -> Result<Self> {
        let mut opt = Self::default();
        opt.game_angou_code = Some(crate::keys::GAMEEXE_KEY.to_vec());
        if let Some(cfg) = crate::key_toml::load_key_toml_from_project_dir(project_dir)? {
            opt.exe_key16 = cfg.exe_key16;
            opt.base_angou_code = cfg.base_angou_code;
            if cfg.game_angou_code.is_some() {
                opt.game_angou_code = cfg.game_angou_code;
            }
            if let Some(order) = cfg.chain_order {
                opt.chain_order = order;
            }
        } else {
            opt.exe_key16 = crate::key_toml::load_key16_from_project_dir(project_dir)?;
        }
        apply_env_overrides(&mut opt)?;
        Ok(opt)
    }
}

#[derive(Debug, Clone)]
pub struct GameexeDecodeReport {
    pub encoding: GameexeTextEncoding,
    pub applied_xor: Vec<(AngouStepKind, usize)>,
    pub used_lzss: bool,
}

#[derive(Debug, Clone)]
pub struct GameexeEntry {
    pub line_no: usize,
    pub raw_key: String,
    pub key: String,
    pub key_parts: Vec<String>,
    pub value: String,
    pub value_items: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct GameexeConfig {
    pub entries: Vec<GameexeEntry>,
    pub map: BTreeMap<String, String>,
}

impl GameexeEntry {
    pub fn key_index(&self, prefix: &str) -> Option<usize> {
        let parts = normalized_key_parts(prefix);
        if self.key_parts.len() < parts.len() + 1 {
            return None;
        }
        if self.key_parts[..parts.len()] != parts[..] {
            return None;
        }
        self.key_parts[parts.len()].parse::<usize>().ok()
    }

    pub fn key_field_after_index(&self, prefix: &str) -> Option<&str> {
        let parts = normalized_key_parts(prefix);
        if self.key_parts.len() < parts.len() + 2 {
            return None;
        }
        if self.key_parts[..parts.len()] != parts[..] {
            return None;
        }
        Some(self.key_parts[parts.len() + 1].as_str())
    }

    pub fn item(&self, idx: usize) -> Option<&str> {
        self.value_items.get(idx).map(|s| s.as_str())
    }

    pub fn item_unquoted(&self, idx: usize) -> Option<&str> {
        self.item(idx).map(unquote_token)
    }

    pub fn scalar_unquoted(&self) -> &str {
        if self.value_items.is_empty() {
            unquote_token(&self.value)
        } else {
            unquote_token(&self.value_items[0])
        }
    }
}

fn unquote_token(s: &str) -> &str {
    let t = s.trim();
    if t.len() >= 2 && t.starts_with('"') && t.ends_with('"') {
        &t[1..t.len() - 1]
    } else {
        t
    }
}

impl GameexeConfig {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(&normalize_key(key)).map(|s| s.as_str())
    }

    pub fn get_entry(&self, key: &str) -> Option<&GameexeEntry> {
        let nk = normalize_key(key);
        self.entries.iter().rev().find(|e| e.key == nk)
    }

    pub fn get_entries<'a>(&'a self, key: &str) -> impl Iterator<Item = &'a GameexeEntry> + 'a {
        let nk = normalize_key(key);
        self.entries.iter().filter(move |e| e.key == nk)
    }

    pub fn get_all<'a>(&'a self, key: &str) -> impl Iterator<Item = &'a str> + 'a {
        self.get_entries(key).map(|e| e.value.as_str())
    }

    pub fn get_unquoted(&self, key: &str) -> Option<&str> {
        self.get_entry(key).map(|e| e.scalar_unquoted())
    }

    pub fn get_item(&self, key: &str, item: usize) -> Option<&str> {
        self.get_entry(key).and_then(|e| e.item(item))
    }

    pub fn get_item_unquoted(&self, key: &str, item: usize) -> Option<&str> {
        self.get_entry(key).and_then(|e| e.item_unquoted(item))
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get_unquoted(key).and_then(parse_i64_like)
    }

    pub fn get_usize(&self, key: &str) -> Option<usize> {
        self.get_i64(key).and_then(|v| usize::try_from(v).ok())
    }

    pub fn get_indexed(&self, prefix: &str, index: usize) -> Option<&str> {
        let key = format!("{}.{}", normalize_key(prefix), index);
        self.get(&key)
    }

    pub fn get_indexed_unquoted(&self, prefix: &str, index: usize) -> Option<&str> {
        self.get_indexed_entry(prefix, index)
            .map(|e| e.scalar_unquoted())
    }

    pub fn get_indexed_item(&self, prefix: &str, index: usize, item: usize) -> Option<&str> {
        self.get_indexed_entry(prefix, index)
            .and_then(|e| e.item(item))
    }

    pub fn get_indexed_item_unquoted(
        &self,
        prefix: &str,
        index: usize,
        item: usize,
    ) -> Option<&str> {
        self.get_indexed_entry(prefix, index)
            .and_then(|e| e.item_unquoted(item))
    }

    pub fn get_indexed_entry(&self, prefix: &str, index: usize) -> Option<&GameexeEntry> {
        // Original Gameexe keys are usually zero-padded (for example BGM.000).
        // Match by parsed key index instead of formatting the index back as a
        // non-padded decimal string, otherwise table-backed subsystems silently
        // miss registered rows. Keep reverse iteration to preserve get_entry
        // "last definition wins" behavior.
        self.entries
            .iter()
            .rev()
            .find(|e| e.key_index(prefix) == Some(index))
    }

    pub fn get_indexed_field(&self, prefix: &str, index: usize, field: &str) -> Option<&str> {
        let nf = normalize_key(field);
        self.entries
            .iter()
            .rev()
            .find(|e| e.key_index(prefix) == Some(index) && e.key_field_after_index(prefix) == Some(nf.as_str()))
            .map(|e| e.value.as_str())
    }

    pub fn get_indexed_field_unquoted(
        &self,
        prefix: &str,
        index: usize,
        field: &str,
    ) -> Option<&str> {
        let nf = normalize_key(field);
        self.entries
            .iter()
            .rev()
            .find(|e| e.key_index(prefix) == Some(index) && e.key_field_after_index(prefix) == Some(nf.as_str()))
            .map(|e| e.scalar_unquoted())
    }

    pub fn get_prefix<'a>(&'a self, prefix: &str) -> impl Iterator<Item = &'a GameexeEntry> + 'a {
        let prefix_parts = normalized_key_parts(prefix);
        self.entries.iter().filter(move |e| {
            e.key_parts.len() >= prefix_parts.len()
                && e.key_parts[..prefix_parts.len()] == prefix_parts[..]
        })
    }

    pub fn indexed_count(&self, prefix: &str) -> usize {
        if let Some(v) = self.get_usize(&format!("{}.CNT", normalize_key(prefix))) {
            return v;
        }
        let prefix_parts = normalized_key_parts(prefix);
        let mut max_idx: Option<usize> = None;
        for e in &self.entries {
            if e.key_parts.len() < prefix_parts.len() + 1 {
                continue;
            }
            if e.key_parts[..prefix_parts.len()] != prefix_parts[..] {
                continue;
            }
            let Some(idx) = e.key_parts[prefix_parts.len()].parse::<usize>().ok() else {
                continue;
            };
            max_idx = Some(max_idx.map_or(idx, |m| m.max(idx)));
        }
        max_idx.map_or(0, |m| m + 1)
    }

    pub fn from_text(text: &str) -> Self {
        let mut out = Self::default();
        for (line_no, raw_line) in text.lines().enumerate() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with(';') || !line.starts_with('#') {
                continue;
            }
            let body = &line[1..];
            let Some((k, v)) = body.split_once('=') else {
                continue;
            };
            let key = normalize_key(k);
            if key.is_empty() {
                continue;
            }
            let value = strip_inline_comment(v.trim()).trim().to_string();
            let entry = GameexeEntry {
                line_no: line_no + 1,
                raw_key: k.trim().to_string(),
                key: key.clone(),
                key_parts: split_key_parts(&key),
                value_items: split_csv_like(&value),
                value: value.clone(),
            };
            out.entries.push(entry);
            out.map.insert(key, value);
        }
        out
    }
}

fn normalized_key_parts(k: &str) -> Vec<String> {
    split_key_parts(&normalize_key(k))
}

fn split_key_parts(k: &str) -> Vec<String> {
    k.split('.')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn normalize_key(k: &str) -> String {
    let mut out = String::new();
    for ch in k.trim().chars() {
        if ch.is_ascii_whitespace() {
            continue;
        }
        out.push(ch);
    }
    out.make_ascii_uppercase();
    out
}

fn split_csv_like(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    let mut escaped = false;
    for ch in s.chars() {
        match ch {
            '"' if !escaped => {
                in_str = !in_str;
                cur.push(ch);
            }
            ',' if !in_str => {
                out.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(ch),
        }
        if ch == '\\' {
            escaped = !escaped;
        } else {
            escaped = false;
        }
    }
    if !cur.is_empty() || s.contains(',') {
        out.push(cur.trim().to_string());
    }
    out.retain(|v| !v.is_empty());
    out
}

fn parse_i64_like(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(hex) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        return i64::from_str_radix(hex.trim(), 16).ok();
    }
    t.parse::<i64>().ok()
}

fn strip_inline_comment(s: &str) -> &str {
    let mut in_str = false;
    let mut escaped = false;
    for (idx, ch) in s.char_indices() {
        match ch {
            '"' if !escaped => in_str = !in_str,
            ';' if !in_str => return &s[..idx],
            _ => {}
        }
        if ch == '\\' {
            escaped = !escaped;
        } else {
            escaped = false;
        }
    }
    s
}

pub fn decode_gameexe_dat_bytes(
    raw: &[u8],
    opt: &GameexeDecodeOptions,
) -> Result<(String, GameexeDecodeReport)> {
    if let Ok(v) = decode_gameexe_with_header(raw, opt) {
        return Ok(v);
    }

    if let Ok((s, enc)) = decode_text_guess(raw) {
        return Ok((
            s,
            GameexeDecodeReport {
                encoding: enc,
                applied_xor: Vec::new(),
                used_lzss: false,
            },
        ));
    }

    if opt.try_lzss {
        if let Ok(unpacked) = lzss_unpack_lenient(raw) {
            if let Ok((s, enc)) = decode_text_guess(&unpacked) {
                return Ok((
                    s,
                    GameexeDecodeReport {
                        encoding: enc,
                        applied_xor: Vec::new(),
                        used_lzss: true,
                    },
                ));
            }
        }
    }

    let (xor_chain, applied) = build_chain(opt)?;
    if !xor_chain.steps.is_empty() {
        let mut buf = raw.to_vec();
        xor_chain.apply_in_place(&mut buf);
        if let Ok((s, enc)) = decode_text_guess(&buf) {
            return Ok((
                s,
                GameexeDecodeReport {
                    encoding: enc,
                    applied_xor: applied.clone(),
                    used_lzss: false,
                },
            ));
        }

        if opt.try_lzss {
            if let Ok(unpacked) = lzss_unpack_lenient(&buf) {
                if let Ok((s, enc)) = decode_text_guess(&unpacked) {
                    return Ok((
                        s,
                        GameexeDecodeReport {
                            encoding: enc,
                            applied_xor: applied,
                            used_lzss: true,
                        },
                    ));
                }
            }
        }
    }

    bail!("failed to decode Gameexe.dat as plaintext or (xor/lzss) wrapped text")
}

fn decode_gameexe_with_header(
    raw: &[u8],
    opt: &GameexeDecodeOptions,
) -> Result<(String, GameexeDecodeReport)> {
    if raw.len() < 8 {
        bail!("gameexe header: too short");
    }
    let version = i32::from_le_bytes(raw[0..4].try_into().unwrap());
    let exe_angou_mode = i32::from_le_bytes(raw[4..8].try_into().unwrap());
    let mut buf = raw[8..].to_vec();

    let mut applied = Vec::new();

    if exe_angou_mode != 0 {
        if let Some(k16) = opt.exe_key16 {
            let step = AngouStep::new(AngouStepKind::ExeKey16, k16.to_vec())?;
            xor_cycle_in_place(&mut buf, &step.key);
            applied.push((AngouStepKind::ExeKey16, step.key.len()));
        }
    }
    if let Some(code) = &opt.game_angou_code {
        let step = AngouStep::new(AngouStepKind::GameCode, code.clone())?;
        xor_cycle_in_place(&mut buf, &step.key);
        applied.push((AngouStepKind::GameCode, step.key.len()));
    }

    if let Ok(unpacked) = lzss_unpack_lenient(&buf) {
        if let Ok((s, enc)) = decode_text_guess(&unpacked) {
            return Ok((
                s,
                GameexeDecodeReport {
                    encoding: enc,
                    applied_xor: applied,
                    used_lzss: true,
                },
            ));
        }
    }

    if let Ok((s, enc)) = decode_text_guess(&buf) {
        return Ok((
            s,
            GameexeDecodeReport {
                encoding: enc,
                applied_xor: applied,
                used_lzss: false,
            },
        ));
    }

    bail!("gameexe header decode failed (version={version})")
}

fn build_chain(opt: &GameexeDecodeOptions) -> Result<(AngouChain, Vec<(AngouStepKind, usize)>)> {
    let mut chain = AngouChain::default();
    for kind in &opt.chain_order {
        match kind {
            AngouStepKind::ExeKey16 => {
                if let Some(k16) = opt.exe_key16 {
                    chain
                        .steps
                        .push(AngouStep::new(AngouStepKind::ExeKey16, k16.to_vec())?);
                }
            }
            AngouStepKind::BaseCode => {
                if let Some(code) = &opt.base_angou_code {
                    chain
                        .steps
                        .push(AngouStep::new(AngouStepKind::BaseCode, code.clone())?);
                }
            }
            AngouStepKind::GameCode => {
                if let Some(code) = &opt.game_angou_code {
                    chain
                        .steps
                        .push(AngouStep::new(AngouStepKind::GameCode, code.clone())?);
                }
            }
        }
    }
    let applied = chain.describe();
    Ok((chain, applied))
}

fn apply_env_overrides(opt: &mut GameexeDecodeOptions) -> Result<()> {
    if let Ok(hex) = env::var("SIGLUS_EXE_ANGOU_HEX") {
        let bytes = crate::angou::parse_hex_bytes(&hex)?;
        if bytes.len() != 16 {
            bail!("SIGLUS_EXE_ANGOU_HEX must be 16 bytes, got {}", bytes.len());
        }
        let mut key16 = [0u8; 16];
        key16.copy_from_slice(&bytes);
        opt.exe_key16 = Some(key16);
    }
    if let Ok(hex) = env::var("SIGLUS_BASE_ANGOU_CODE_HEX") {
        opt.base_angou_code = Some(crate::angou::parse_hex_bytes(&hex)?);
    }
    if let Ok(hex) = env::var("SIGLUS_GAME_ANGOU_CODE_HEX") {
        opt.game_angou_code = Some(crate::angou::parse_hex_bytes(&hex)?);
    }
    if let Ok(order_raw) = env::var("SIGLUS_ANGOU_CHAIN_ORDER") {
        let mut order = Vec::new();
        for part in order_raw.split(',') {
            let tok = part.trim().to_ascii_lowercase();
            if tok.is_empty() {
                continue;
            }
            let kind = match tok.as_str() {
                "exe" | "exe_key16" => AngouStepKind::ExeKey16,
                "base" | "base_code" => AngouStepKind::BaseCode,
                "game" | "game_code" => AngouStepKind::GameCode,
                other => bail!("SIGLUS_ANGOU_CHAIN_ORDER: unknown item {other}"),
            };
            order.push(kind);
        }
        if !order.is_empty() {
            opt.chain_order = order;
        }
    }
    Ok(())
}

fn decode_text_guess(raw: &[u8]) -> Result<(String, GameexeTextEncoding)> {
    if let Ok(s) = decode_utf16le_text(raw) {
        if looks_like_gameexe(&s) {
            return Ok((s, GameexeTextEncoding::Utf16Le));
        }
    }

    if let Ok(s) = decode_shift_jis(raw) {
        if looks_like_gameexe(&s) {
            return Ok((s, GameexeTextEncoding::ShiftJis));
        }
    }

    if let Ok(s) = std::str::from_utf8(raw) {
        let s = s.to_string();
        if looks_like_gameexe(&s) {
            return Ok((s, GameexeTextEncoding::Utf8));
        }
    }

    Err(anyhow!("text guess failed"))
}

fn decode_shift_jis(raw: &[u8]) -> Result<String> {
    let (cow, _, had_err) = SHIFT_JIS.decode(raw);
    if had_err {
        bail!("shift-jis decode error")
    }
    Ok(cow.into_owned())
}

fn decode_utf16le_text(raw: &[u8]) -> Result<String> {
    if raw.len() < 2 {
        bail!("too short")
    }

    let (start, has_bom) = if raw.len() >= 2 && raw[0] == 0xFF && raw[1] == 0xFE {
        (2usize, true)
    } else {
        (0usize, false)
    };

    if !has_bom {
        let mut zero_odd = 0usize;
        let mut total_odd = 0usize;
        for i in (1..raw.len()).step_by(2) {
            total_odd += 1;
            if raw[i] == 0 {
                zero_odd += 1;
            }
        }
        if total_odd > 0 {
            let ratio = (zero_odd as f32) / (total_odd as f32);
            if ratio < 0.30 {
                bail!("utf16le heuristic ratio too low")
            }
        }
    }

    let mut u16s = Vec::with_capacity((raw.len() - start) / 2);
    for i in (start..raw.len()).step_by(2) {
        if i + 1 >= raw.len() {
            break;
        }
        u16s.push(u16::from_le_bytes([raw[i], raw[i + 1]]));
    }
    let s = String::from_utf16(&u16s)?;
    Ok(s)
}

fn looks_like_gameexe(s: &str) -> bool {
    let mut cnt = 0usize;
    for line in s.lines().take(200) {
        let line = line.trim();
        if line.starts_with('#') {
            cnt += 1;
            if cnt >= 3 {
                return true;
            }
        }
    }
    false
}
