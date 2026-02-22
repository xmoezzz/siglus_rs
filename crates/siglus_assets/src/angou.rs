//! Siglus "angou" (encryption/obfuscation) helpers.
//!
//! ## Important
//! Different games may use different "angou" materials.
//! In practice there can be:
//! - a **base** (engine) angou code table, and
//! - a **game-specific** angou code table,
//! and both can be applied (typically as sequential XOR streams) on top of an
//! optional 16-byte exe-derived key.
//!
//! This module intentionally **exposes** those inputs, instead of hard-coding a
//! single table, so the port can support multiple titles without rewrites.

use anyhow::{bail, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AngouStepKind {
    ExeKey16,
    BaseCode,
    GameCode,
}

#[derive(Debug, Clone)]
pub struct AngouStep {
    pub kind: AngouStepKind,
    pub key: Vec<u8>,
}

impl AngouStep {
    pub fn new(kind: AngouStepKind, key: Vec<u8>) -> Result<Self> {
        if key.is_empty() {
            bail!("angou: empty key for step {kind:?}");
        }
        Ok(Self { kind, key })
    }
}

/// A chain of XOR steps. Each step is applied with cyclic indexing.
#[derive(Debug, Clone, Default)]
pub struct AngouChain {
    pub steps: Vec<AngouStep>,
}

impl AngouChain {
    pub fn apply_in_place(&self, buf: &mut [u8]) {
        for step in &self.steps {
            xor_cycle_in_place(buf, &step.key);
        }
    }

    pub fn describe(&self) -> Vec<(AngouStepKind, usize)> {
        self.steps.iter().map(|s| (s.kind, s.key.len())).collect()
    }
}

pub fn xor_cycle_in_place(buf: &mut [u8], key: &[u8]) {
    if key.is_empty() {
        return;
    }
    for (i, b) in buf.iter_mut().enumerate() {
        *b ^= key[i % key.len()];
    }
}

/// Parse a hex string into bytes.
///
/// Accepts with/without `0x` prefix and ignores spaces/underscores.
pub fn parse_hex_bytes(s: &str) -> Result<Vec<u8>> {
    let mut hex = String::with_capacity(s.len());
    for ch in s.chars() {
        if ch == 'x' || ch == 'X' {
            // keep as-is; 0x will be filtered by non-hex anyway.
        }
        if ch.is_ascii_hexdigit() {
            hex.push(ch);
        }
    }

    if hex.len() % 2 != 0 {
        bail!("hex string has odd length");
    }
    let mut out = Vec::with_capacity(hex.len() / 2);
    let bytes = hex.as_bytes();
    for i in (0..bytes.len()).step_by(2) {
        let hi = from_hex_digit(bytes[i])?;
        let lo = from_hex_digit(bytes[i + 1])?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn from_hex_digit(c: u8) -> Result<u8> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => bail!("invalid hex digit"),
    }
}
