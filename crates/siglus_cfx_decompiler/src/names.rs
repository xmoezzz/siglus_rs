use std::collections::BTreeSet;
use std::path::Path;

use crate::cfx::ShaderBlob;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StringEncoding {
    Ascii,
    Utf16Le,
}

impl StringEncoding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ascii => "ascii",
            Self::Utf16Le => "utf16le",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ExtractedString {
    pub offset: usize,
    pub encoding: StringEncoding,
    pub text: String,
}

pub fn extract_strings(data: &[u8]) -> Vec<ExtractedString> {
    let mut out = Vec::new();
    out.extend(extract_ascii_strings(data, 3));
    out.extend(extract_utf16le_strings(data, 3));
    out.sort_by_key(|s| (s.offset, match s.encoding { StringEncoding::Ascii => 0u8, StringEncoding::Utf16Le => 1u8 }));
    out.dedup_by(|a, b| a.offset == b.offset && a.encoding == b.encoding && a.text == b.text);
    out
}

fn extract_ascii_strings(data: &[u8], min_len: usize) -> Vec<ExtractedString> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < data.len() {
        while i < data.len() && !is_printable_ascii(data[i]) {
            i += 1;
        }
        let start = i;
        while i < data.len() && is_printable_ascii(data[i]) {
            i += 1;
        }
        if i.saturating_sub(start) >= min_len {
            let text = String::from_utf8_lossy(&data[start..i]).to_string();
            out.push(ExtractedString { offset: start, encoding: StringEncoding::Ascii, text });
        }
    }
    out
}

fn extract_utf16le_strings(data: &[u8], min_chars: usize) -> Vec<ExtractedString> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 1 < data.len() {
        while i + 1 < data.len() && !is_printable_utf16le_at(data, i) {
            i += 2;
        }
        let start = i;
        let mut chars = Vec::new();
        while i + 1 < data.len() && is_printable_utf16le_at(data, i) {
            chars.push(u16::from_le_bytes([data[i], data[i + 1]]));
            i += 2;
        }
        if chars.len() >= min_chars {
            if let Ok(text) = String::from_utf16(&chars) {
                out.push(ExtractedString { offset: start, encoding: StringEncoding::Utf16Le, text });
            }
        }
        if i == start {
            i += 2;
        }
    }
    out
}

fn is_printable_ascii(b: u8) -> bool {
    matches!(b, 0x20..=0x7e)
}

fn is_printable_utf16le_at(data: &[u8], off: usize) -> bool {
    if off + 1 >= data.len() {
        return false;
    }
    let c = u16::from_le_bytes([data[off], data[off + 1]]);
    matches!(c, 0x20..=0x7e)
}

pub fn is_hlsl_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else { return false; };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return false;
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

pub fn looks_like_technique_name(s: &str) -> bool {
    s.starts_with("tec_") || s.starts_with("tech_") || s.starts_with("technique")
}

pub fn looks_like_shader_function_name(s: &str) -> bool {
    if !is_hlsl_identifier(s) {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    lower.starts_with("vs_")
        || lower.starts_with("ps_")
        || lower.starts_with("v_")
        || lower.starts_with("p_")
        || lower.starts_with("vertex")
        || lower.starts_with("pixel")
}

pub fn looks_like_interface_or_struct_name(s: &str) -> bool {
    if !is_hlsl_identifier(s) {
        return false;
    }
    let lower = s.to_ascii_lowercase();
    lower.contains("input")
        || lower.contains("output")
        || lower.ends_with("_in")
        || lower.ends_with("_out")
        || lower.ends_with("interface")
        || lower.ends_with("struct")
}

pub fn format_original_name_report(input: &Path, data: &[u8], shaders: &[ShaderBlob]) -> String {
    let strings = extract_strings(data);
    let mut identifiers = BTreeSet::new();
    let mut techniques = BTreeSet::new();
    let mut shader_function_candidates = BTreeSet::new();
    let mut interface_candidates = BTreeSet::new();

    for s in &strings {
        if is_hlsl_identifier(&s.text) {
            identifiers.insert(s.text.clone());
        }
        if looks_like_technique_name(&s.text) {
            techniques.insert(s.text.clone());
        }
        if looks_like_shader_function_name(&s.text) {
            shader_function_candidates.insert(s.text.clone());
        }
        if looks_like_interface_or_struct_name(&s.text) {
            interface_candidates.insert(s.text.clone());
        }
    }

    let mut out = String::new();
    out.push_str(&format!("input: {}\n", input.display()));
    out.push_str("\n");
    out.push_str("name_recovery_policy:\n");
    out.push_str("  exact_shader_entry_function_name: only used if explicitly stored in source/debug/effect metadata; SM2 bytecode CTAB does not carry it.\n");
    out.push_str("  exact_interface_type_name: only used if explicitly stored in source/debug/effect metadata; SM2 bytecode declarations carry semantics, not original HLSL struct/interface type names.\n");
    out.push_str("  uniforms_samplers_and_struct_members: recovered from CTAB when present.\n");
    out.push_str("  technique_pass_names: recovered only after effect-container parsing; raw shader scanning alone cannot map them to a shader safely.\n");
    out.push_str("\n");

    out.push_str("shader_ctab_names:\n");
    for shader in shaders {
        out.push_str(&format!("  {} offset=0x{:08x} profile={}\n", shader.file_prefix(), shader.offset, shader.profile()));
        if let Some(ctab) = &shader.ctab {
            if let Some(creator) = &ctab.creator {
                out.push_str(&format!("    creator: {}\n", creator));
            }
            if let Some(target) = &ctab.target {
                out.push_str(&format!("    target: {}\n", target));
            }
            for c in &ctab.constants {
                out.push_str(&format!("    {} {} {} count={}\n", c.register_name(), c.hlsl_decl_type(), c.name, c.register_count));
                if let Some(t) = &c.type_info {
                    for m in &t.members {
                        out.push_str(&format!("      member {} {}\n", m.type_info.hlsl_type_name(), m.name));
                    }
                }
            }
        } else {
            out.push_str("    no CTAB\n");
        }
    }

    out.push_str("\ntechnique_name_candidates_from_container_strings:\n");
    for name in techniques.iter().take(2000) {
        out.push_str(&format!("  {}\n", name));
    }
    if techniques.len() > 2000 {
        out.push_str(&format!("  ... {} more\n", techniques.len() - 2000));
    }

    out.push_str("\nshader_function_name_candidates_from_container_strings:\n");
    for name in shader_function_candidates.iter().take(2000) {
        out.push_str(&format!("  {}\n", name));
    }
    if shader_function_candidates.len() > 2000 {
        out.push_str(&format!("  ... {} more\n", shader_function_candidates.len() - 2000));
    }

    out.push_str("\ninterface_or_struct_name_candidates_from_container_strings:\n");
    for name in interface_candidates.iter().take(2000) {
        out.push_str(&format!("  {}\n", name));
    }
    if interface_candidates.len() > 2000 {
        out.push_str(&format!("  ... {} more\n", interface_candidates.len() - 2000));
    }

    out.push_str("\nall_identifier_strings_from_container:\n");
    for name in identifiers.iter().take(5000) {
        out.push_str(&format!("  {}\n", name));
    }
    if identifiers.len() > 5000 {
        out.push_str(&format!("  ... {} more\n", identifiers.len() - 5000));
    }

    out.push_str("\nall_strings_with_offsets:\n");
    for s in strings.iter().take(10000) {
        out.push_str(&format!("  0x{:08x} {} {}\n", s.offset, s.encoding.as_str(), s.text));
    }
    if strings.len() > 10000 {
        out.push_str(&format!("  ... {} more\n", strings.len() - 10000));
    }

    out
}
