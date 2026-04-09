use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};

use crate::angou::{self, AngouStepKind};

pub fn load_key16_from_project_dir(project_dir: &Path) -> Result<Option<[u8; 16]>> {
    let path = project_dir.join("key.toml");
    if !path.is_file() {
        return Ok(None);
    }
    load_key16_from_file(&path)
}

pub fn load_key16_from_file(path: &Path) -> Result<Option<[u8; 16]>> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    parse_key16_toml(&text)
}

fn parse_key16_toml(text: &str) -> Result<Option<[u8; 16]>> {
    let Some(bytes) = parse_named_bytes(text, "key", "key_hex")? else {
        return Ok(None);
    };
    if bytes.len() != 16 {
        bail!(
            "key.toml: key must contain exactly 16 bytes, got {}",
            bytes.len()
        );
    }
    let mut out = [0u8; 16];
    out.copy_from_slice(&bytes);
    Ok(Some(out))
}

#[derive(Debug, Clone, Default)]
pub struct KeyTomlConfig {
    pub exe_key16: Option<[u8; 16]>,
    pub base_angou_code: Option<Vec<u8>>,
    pub game_angou_code: Option<Vec<u8>>,
    pub chain_order: Option<Vec<AngouStepKind>>,
}

pub fn load_key_toml_from_project_dir(project_dir: &Path) -> Result<Option<KeyTomlConfig>> {
    let path = project_dir.join("key.toml");
    if !path.is_file() {
        return Ok(None);
    }
    load_key_toml_from_file(&path).map(Some)
}

pub fn load_key_toml_from_file(path: &Path) -> Result<KeyTomlConfig> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    parse_key_toml(&text)
}

fn parse_key_toml(text: &str) -> Result<KeyTomlConfig> {
    let mut out = KeyTomlConfig::default();

    out.exe_key16 = parse_key16_toml(text)?;
    out.base_angou_code = parse_named_bytes(text, "base_angou_code", "base_angou_hex")?;
    out.game_angou_code = parse_named_bytes(text, "game_angou_code", "game_angou_hex")?;
    out.chain_order = parse_chain_order(text)?;

    Ok(out)
}

fn parse_named_bytes(text: &str, key: &str, alt_hex_key: &str) -> Result<Option<Vec<u8>>> {
    if let Some(raw) = collect_rhs_for_key(text, key) {
        return parse_bytes_value(&raw, key);
    }
    if let Some(raw) = collect_rhs_for_key(text, alt_hex_key) {
        return parse_hex_value(&raw, alt_hex_key);
    }
    Ok(None)
}

fn parse_bytes_value(raw: &str, key: &str) -> Result<Option<Vec<u8>>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    if raw.contains('[') {
        let (inner, _) = extract_bracketed(raw)
            .ok_or_else(|| anyhow::anyhow!("key.toml: {key} array missing closing ]"))?;
        return Ok(Some(parse_byte_array(inner, key)?));
    }
    if raw.starts_with('"') && raw.ends_with('"') && raw.len() >= 2 {
        let inner = &raw[1..raw.len() - 1];
        let bytes = angou::parse_hex_bytes(inner)
            .with_context(|| format!("key.toml: invalid hex for {key}"))?;
        return Ok(Some(bytes));
    }
    Ok(None)
}

fn parse_hex_value(raw: &str, key: &str) -> Result<Option<Vec<u8>>> {
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let inner = raw.trim_matches('"');
    let bytes = angou::parse_hex_bytes(inner)
        .with_context(|| format!("key.toml: invalid hex for {key}"))?;
    Ok(Some(bytes))
}

fn parse_byte_array(inner: &str, key: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    for part in inner.split(',') {
        let tok = part.trim();
        if tok.is_empty() {
            continue;
        }
        let value = if let Some(hex) = tok.strip_prefix("0x").or_else(|| tok.strip_prefix("0X")) {
            u8::from_str_radix(hex, 16)
                .with_context(|| format!("key.toml: invalid hex byte {tok}"))?
        } else {
            let v: u16 = tok
                .parse()
                .with_context(|| format!("key.toml: invalid byte {tok}"))?;
            if v > 0xFF {
                bail!("key.toml: byte out of range {tok}");
            }
            v as u8
        };
        bytes.push(value);
    }
    if bytes.is_empty() {
        bail!("key.toml: {key} array is empty");
    }
    Ok(bytes)
}

fn collect_rhs_for_key(text: &str, key: &str) -> Option<String> {
    let mut collecting = false;
    let mut out = String::new();

    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        if !collecting {
            let Some((lhs, rhs)) = line.split_once('=') else {
                continue;
            };
            if lhs.trim() != key {
                continue;
            }
            collecting = true;
            out.push_str(rhs.trim());
            if rhs.contains(']') {
                break;
            }
        } else {
            out.push(' ');
            out.push_str(line);
            if line.contains(']') {
                break;
            }
        }
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

fn extract_bracketed(raw: &str) -> Option<(&str, &str)> {
    let start = raw.find('[')?;
    let end = raw[start + 1..].find(']').map(|v| start + 1 + v)?;
    Some((&raw[start + 1..end], &raw[end + 1..]))
}

fn parse_chain_order(text: &str) -> Result<Option<Vec<AngouStepKind>>> {
    let Some(raw) = collect_rhs_for_key(text, "chain_order") else {
        return Ok(None);
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    let inner = if raw.contains('[') {
        let (inner, _) = extract_bracketed(raw)
            .ok_or_else(|| anyhow::anyhow!("key.toml: chain_order missing closing ]"))?;
        inner
    } else {
        raw
    };
    let mut out = Vec::new();
    for part in inner.split(',') {
        let tok = part.trim().trim_matches('"').trim_matches('\'');
        if tok.is_empty() {
            continue;
        }
        let kind = match tok.to_ascii_lowercase().as_str() {
            "exe" | "exe_key16" => AngouStepKind::ExeKey16,
            "base" | "base_code" => AngouStepKind::BaseCode,
            "game" | "game_code" => AngouStepKind::GameCode,
            other => bail!("key.toml: unknown chain_order item {other}"),
        };
        out.push(kind);
    }
    if out.is_empty() {
        return Ok(None);
    }
    Ok(Some(out))
}
