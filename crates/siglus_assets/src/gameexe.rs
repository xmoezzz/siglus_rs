//! Gameexe.dat decoding and INI-like parsing.
//!
//! In the original Siglus engine, `Gameexe.dat` is treated as a TCHAR text
//! blob (UTF-16LE). Some titles may store an alternative encoding (e.g.
//! Shift-JIS) or wrap the text with obfuscation + LZSS.
//!
//! This module keeps the decoding pipeline *explicit*:
//! - plaintext (UTF-16LE / Shift-JIS)
//! - optionally XOR with a chain of "angou" materials
//! - optional Siglus LZSS unpack
//!
//! The "angou" chain can include a base (engine) code and a game-specific
//! code. Both are exposed via `GameexeDecodeOptions`.

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Result};
use encoding_rs::SHIFT_JIS;
use std::env;

use crate::angou::{AngouChain, AngouStep, AngouStepKind};
use crate::lzss::lzss_unpack_lenient;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameexeTextEncoding {
    Utf16Le,
    ShiftJis,
    Utf8,
}

#[derive(Debug, Clone)]
pub struct GameexeDecodeOptions {
    /// Optional 16-byte exe-derived key.
    pub exe_key16: Option<[u8; 16]>,
    /// Optional base (engine) angou code stream.
    pub base_angou_code: Option<Vec<u8>>,
    /// Optional game-specific angou code stream.
    pub game_angou_code: Option<Vec<u8>>,
    /// Whether to attempt LZSS unpack after XOR.
    pub try_lzss: bool,

    /// Order of XOR steps.
    ///
    /// Default: `[ExeKey16, BaseCode, GameCode]`.
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
    /// Build options from environment variables.
    ///
    /// This is intentionally explicit so different titles can inject different
    /// angou materials without code changes.
    ///
    /// - `SIGLUS_EXE_ANGOU_HEX`: 16-byte hex (exe-derived key)
    /// - `SIGLUS_BASE_ANGOU_CODE_HEX`: hex blob (often 256 bytes)
    /// - `SIGLUS_GAME_ANGOU_CODE_HEX`: hex blob (often 256 bytes)
    /// - `SIGLUS_ANGOU_CHAIN_ORDER`: comma-separated list of `exe,base,game`
    pub fn from_env() -> Result<Self> {
        use crate::angou::parse_hex_bytes;

        let mut opt = Self::default();

        if let Ok(s) = env::var("SIGLUS_EXE_ANGOU_HEX") {
            let b = parse_hex_bytes(&s)?;
            if b.len() != 16 {
                bail!("SIGLUS_EXE_ANGOU_HEX must be 16 bytes, got {}", b.len());
            }
            let mut k16 = [0u8; 16];
            k16.copy_from_slice(&b);
            opt.exe_key16 = Some(k16);
        }

        if let Ok(s) = env::var("SIGLUS_BASE_ANGOU_CODE_HEX") {
            let b = parse_hex_bytes(&s)?;
            if !b.is_empty() {
                opt.base_angou_code = Some(b);
            }
        }

        if let Ok(s) = env::var("SIGLUS_GAME_ANGOU_CODE_HEX") {
            let b = parse_hex_bytes(&s)?;
            if !b.is_empty() {
                opt.game_angou_code = Some(b);
            }
        }

        if let Ok(s) = env::var("SIGLUS_ANGOU_CHAIN_ORDER") {
            let mut v = Vec::new();
            for part in s.split(',') {
                let p = part.trim().to_ascii_lowercase();
                match p.as_str() {
                    "exe" | "exe16" | "key16" => v.push(AngouStepKind::ExeKey16),
                    "base" => v.push(AngouStepKind::BaseCode),
                    "game" => v.push(AngouStepKind::GameCode),
                    "" => {}
                    _ => bail!("unknown chain element in SIGLUS_ANGOU_CHAIN_ORDER: {p}"),
                }
            }
            if !v.is_empty() {
                opt.chain_order = v;
            }
        }

        Ok(opt)
    }
}

#[derive(Debug, Clone)]
pub struct GameexeDecodeReport {
    pub encoding: GameexeTextEncoding,
    /// XOR steps applied (kind, key_len).
    pub applied_xor: Vec<(AngouStepKind, usize)>,
    /// Whether LZSS unpack was used.
    pub used_lzss: bool,
}

#[derive(Debug, Clone)]
pub struct GameexeConfig {
    /// Raw key-value map (normalized key -> value).
    pub map: BTreeMap<String, String>,
}

impl GameexeConfig {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.map.get(&normalize_key(key)).map(|s| s.as_str())
    }

    /// Parse an INI-like text where each meaningful line looks like:
    /// `#KEY = VALUE`.
    pub fn from_text(text: &str) -> Self {
        let mut map = BTreeMap::new();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            // Comments in some titles start with ';'
            if line.starts_with(';') {
                continue;
            }
            if !line.starts_with('#') {
                continue;
            }
            let body = &line[1..];
            let Some((k, v)) = body.split_once('=') else {
                continue;
            };
            let key = normalize_key(k);
            let value = v.trim().to_string();
            if !key.is_empty() {
                map.insert(key, value);
            }
        }

        Self { map }
    }
}

fn normalize_key(k: &str) -> String {
    // The original parser is fairly permissive. We normalize whitespace and
    // use upper-case ASCII for robust lookups.
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

/// Decode a `Gameexe.dat` blob into text + report.
///
/// The function tries (in order):
/// 1) Plaintext decoding (UTF-16LE, then Shift-JIS, then UTF-8)
/// 2) XOR chain (exe_key16 + base + game) and plaintext decoding
/// 3) XOR chain + LZSS unpack and plaintext decoding
pub fn decode_gameexe_dat_bytes(
    raw: &[u8],
    opt: &GameexeDecodeOptions,
) -> Result<(String, GameexeDecodeReport)> {
    // 1) plaintext
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

    // 2) XOR only
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

        // 3) XOR + LZSS
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

fn decode_text_guess(raw: &[u8]) -> Result<(String, GameexeTextEncoding)> {
    // UTF-16LE first (BOM or heuristic)
    if let Ok(s) = decode_utf16le_text(raw) {
        if looks_like_gameexe(&s) {
            return Ok((s, GameexeTextEncoding::Utf16Le));
        }
    }

    // Shift-JIS
    if let Ok(s) = decode_shift_jis(raw) {
        if looks_like_gameexe(&s) {
            return Ok((s, GameexeTextEncoding::ShiftJis));
        }
    }

    // UTF-8
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
        // Heuristic: many zeros in odd bytes for ASCII-ish UTF-16LE.
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
    // Minimal, robust check: most meaningful lines begin with '#'.
    // We require at least a few occurrences to avoid false positives.
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
