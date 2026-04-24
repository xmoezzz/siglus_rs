use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use crate::disasm::ShaderKind;

#[derive(Debug, Clone)]
pub struct StructField {
    pub ty: String,
    pub name: String,
    pub semantic: String,
}

#[derive(Debug, Clone)]
struct ParsedHlsl {
    samplers: Vec<String>,
    consts: Vec<(String, String, String)>,
    input_name: String,
    output_name: String,
    input_fields: Vec<StructField>,
    output_fields: Vec<StructField>,
    body_lines: Vec<String>,
    register_count: usize,
}

pub fn discover_reference_hlsl_roots(input: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let candidates = [
        cwd.join("reference_hlsl"),
        cwd.join("output").join("hlsl"),
        input.parent().unwrap_or(Path::new(".")).join("reference_hlsl"),
        input.parent().unwrap_or(Path::new(".")).join("output").join("hlsl"),
    ];
    for c in candidates {
        if c.is_dir() && !out.iter().any(|x| x == &c) {
            out.push(c);
        }
    }
    out
}

pub fn load_reference_hlsl(roots: &[PathBuf], prefix: &str) -> Option<String> {
    for root in roots {
        let path = root.join(format!("{prefix}.hlsl"));
        if let Ok(s) = fs::read_to_string(&path) {
            return Some(s);
        }
    }
    None
}

pub fn transpile_reference_hlsl_to_wgsl(src: &str, kind: ShaderKind) -> Result<String, String> {
    let parsed = parse_hlsl(src)?;
    Ok(emit_wgsl(&parsed, kind))
}

fn parse_hlsl(src: &str) -> Result<ParsedHlsl, String> {
    let mut samplers = Vec::new();
    let mut consts = Vec::new();
    for line in src.lines() {
        let t = line.trim();
        if let Some(name) = t.strip_prefix("uniform sampler2D ").and_then(|s| s.strip_suffix(';')) {
            samplers.push(name.trim().to_string());
        } else if let Some(rest) = t.strip_prefix("static const ") {
            if let Some((lhs, rhs)) = rest.split_once('=') {
                let lhs = lhs.trim();
                let rhs = rhs.trim().trim_end_matches(';').trim();
                let parts = lhs.split_whitespace().collect::<Vec<_>>();
                if parts.len() == 2 {
                    consts.push((parts[1].to_string(), parts[0].to_string(), rhs.to_string()));
                }
            }
        }
    }

    let (input_name, input_fields) = parse_struct(src, "VS_INPUT")
        .or_else(|| parse_struct(src, "PS_INPUT"))
        .ok_or("missing input struct")?;
    let (output_name, output_fields) = parse_struct(src, "VS_OUTPUT")
        .or_else(|| parse_struct(src, "PS_OUTPUT"))
        .ok_or("missing output struct")?;

    let body = extract_main_body(src)?;
    let mut body_lines = Vec::new();
    let const_names: BTreeSet<String> = consts.iter().map(|(n, _, _)| n.clone()).collect();
    let mut max_reg = 0usize;
    for raw in body.lines() {
        let t = raw.trim();
        if t.is_empty() {
            continue;
        }
        if t == format!("{} output;", output_name) || t.starts_with("output.") && t.contains("= ") && t.contains("float4(0.0, 0.0, 0.0, 0.0)") {
            continue;
        }
        let line = translate_statement(t, &const_names, &mut max_reg)?;
        if !line.is_empty() {
            body_lines.push(line);
        }
    }

    Ok(ParsedHlsl {
        samplers,
        consts,
        input_name,
        output_name,
        input_fields,
        output_fields,
        body_lines,
        register_count: max_reg.max(1),
    })
}

fn parse_struct(src: &str, name: &str) -> Option<(String, Vec<StructField>)> {
    let marker = format!("struct {} {{", name);
    let start = src.find(&marker)?;
    let rest = &src[start + marker.len()..];
    let end = rest.find("};")?;
    let body = &rest[..end];
    let mut fields = Vec::new();
    for line in body.lines() {
        let t = line.trim();
        if t.is_empty() { continue; }
        let t = t.trim_end_matches(';');
        let (lhs, semantic) = t.split_once(':')?;
        let parts = lhs.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 2 { continue; }
        fields.push(StructField {
            ty: parts[0].to_string(),
            name: parts[1].to_string(),
            semantic: semantic.trim().to_string(),
        });
    }
    Some((name.to_string(), fields))
}

fn extract_main_body(src: &str) -> Result<String, String> {
    let main_pos = src.find(" main(").or_else(|| src.find("main("))).ok_or("missing main")?;
    let rest = &src[main_pos..];
    let brace = rest.find('{').ok_or("missing main body")?;
    let mut depth = 0i32;
    let mut end_idx = None;
    for (i, ch) in rest[brace..].char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    end_idx = Some(brace + i);
                    break;
                }
            }
            _ => {}
        }
    }
    let end = end_idx.ok_or("unterminated main body")?;
    Ok(rest[brace + 1..end].to_string())
}

fn translate_statement(line: &str, const_names: &BTreeSet<String>, max_reg: &mut usize) -> Result<String, String> {
    let mut out = line.to_string();
    out = out.trim_end_matches(';').to_string();
    out = replace_types(&out);
    out = replace_intrinsics(&out);
    out = replace_sampler_calls(&out);
    out = replace_registers(&out, const_names, max_reg);
    if out.contains('?') {
        out = replace_ternary_expr(&out)?;
    }
    Ok(format!("    {};;", out).replace(";;", ";"))
}

fn replace_types(s: &str) -> String {
    let mut out = s.to_string();
    for (a, b) in [
        ("float4(", "vec4<f32>("),
        ("float3(", "vec3<f32>("),
        ("float2(", "vec2<f32>("),
        ("float4 ", "vec4<f32> "),
        ("float3 ", "vec3<f32> "),
        ("float2 ", "vec2<f32> "),
        ("float ", "f32 "),
        ("int ", "i32 "),
    ] {
        out = out.replace(a, b);
    }
    out
}

fn replace_intrinsics(s: &str) -> String {
    let mut out = s.to_string();
    out = out.replace("lerp(", "mix(");
    out = out.replace("rsqrt(", "inverseSqrt(");
    out = out.replace("frac(", "fract(");
    while let Some(pos) = out.find("saturate(") {
        let start = pos + "saturate(".len();
        let end = find_matching_paren(&out, start - 1).unwrap_or(out.len() - 1);
        let inner = &out[start..end];
        let repl = format!("clamp({}, 0.0, 1.0)", inner);
        out.replace_range(pos..=end, &repl);
    }
    out
}

fn replace_sampler_calls(s: &str) -> String {
    let mut out = String::new();
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        if s[i..].starts_with("tex2D(") {
            let args_start = i + "tex2D(".len();
            let end = find_matching_paren(s, args_start - 1).unwrap_or(s.len() - 1);
            let args = &s[args_start..end];
            if let Some((sampler, coord)) = split_top_level_once(args, ',') {
                out.push_str(&format!("textureSample(tex_{}, samp_{}, {})", sampler.trim(), sampler.trim(), coord.trim()));
                i = end + 1;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn replace_registers(s: &str, const_names: &BTreeSet<String>, max_reg: &mut usize) -> String {
    let mut out = String::new();
    let chars = s.chars().collect::<Vec<_>>();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i] == 'c' {
            let start = i;
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_ascii_digit() { j += 1; }
            if j > i + 1 {
                let name = chars[start..j].iter().collect::<String>();
                if !const_names.contains(&name) {
                    if let Ok(idx) = name[1..].parse::<usize>() {
                        *max_reg = (*max_reg).max(idx + 1);
                        out.push_str(&format!("u.c[{}]", idx));
                        i = j;
                        continue;
                    }
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

fn replace_ternary_expr(s: &str) -> Result<String, String> {
    let q = s.find('?').ok_or_else(|| format!("bad ternary: {s}"))?;
    let c = s[q + 1..].find(':').map(|x| q + 1 + x).ok_or_else(|| format!("bad ternary colon: {s}"))?;
    let open = s[..q].rfind('(').ok_or_else(|| format!("bad ternary open: {s}"))?;
    let close = s[c + 1..].rfind(')').map(|x| c + 1 + x).ok_or_else(|| format!("bad ternary close: {s}"))?;
    let prefix = &s[..open];
    let cond = s[open + 1..q].trim();
    let true_expr = s[q + 1..c].trim();
    let false_expr = s[c + 1..close].trim();
    let suffix = &s[close + 1..];
    Ok(format!("{}select({}, {}, {}){}", prefix, false_expr, true_expr, cond, suffix))
}

fn find_matching_paren(s: &str, open_idx: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, ch) in s.char_indices().skip_while(|(i, _)| *i < open_idx) {
        match ch {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 { return Some(i); }
            }
            _ => {}
        }
    }
    None
}

fn find_top_level_char(s: &str, target: char) -> Option<usize> {
    let mut depth = 0i32;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ if ch == target && depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

fn find_matching_colon(s: &str, q_index: usize) -> Option<usize> {
    let mut depth = 0i32;
    for (i, ch) in s.char_indices().skip_while(|(i, _)| *i <= q_index) {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            ':' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

fn split_top_level_once(s: &str, needle: char) -> Option<(&str, &str)> {
    let mut depth = 0i32;
    for (i, ch) in s.char_indices() {
        match ch {
            '(' => depth += 1,
            ')' => depth -= 1,
            _ if ch == needle && depth == 0 => return Some((&s[..i], &s[i + 1..])),
            _ => {}
        }
    }
    None
}

fn emit_wgsl(hlsl: &ParsedHlsl, kind: ShaderKind) -> String {
    let mut out = String::new();
    out.push_str(&format!("struct FloatRegs {{\n    c: array<vec4<f32>, {}>,\n}};\n\n", hlsl.register_count));
    out.push_str("@group(0) @binding(0) var<uniform> u: FloatRegs;\n\n");

    let mut sampler_idx = BTreeMap::new();
    for name in &hlsl.samplers {
        let idx = name.trim_start_matches('s').parse::<u32>().unwrap_or(0);
        sampler_idx.insert(name.clone(), idx);
    }
    for name in &hlsl.samplers {
        let idx = sampler_idx[name];
        out.push_str(&format!("@group(1) @binding({}) var tex_{}: texture_2d<f32>;\n", idx * 2, name));
        out.push_str(&format!("@group(1) @binding({}) var samp_{}: sampler;\n", idx * 2 + 1, name));
    }
    if !hlsl.samplers.is_empty() {
        out.push('\n');
    }

    for (name, ty, value) in &hlsl.consts {
        out.push_str(&format!("const {}: {} = {};;\n", name, wgsl_type(ty), replace_types(value)).replace(";;", ";"));
    }
    if !hlsl.consts.is_empty() {
        out.push('\n');
    }

    emit_struct(&mut out, &hlsl.input_name, &hlsl.input_fields, kind, true);
    out.push('\n');
    emit_struct(&mut out, &hlsl.output_name, &hlsl.output_fields, kind, false);
    out.push('\n');

    match kind {
        ShaderKind::Vertex => out.push_str("@vertex\n"),
        ShaderKind::Pixel => out.push_str("@fragment\n"),
    }
    out.push_str(&format!("fn main(input: {}) -> {} {{\n", hlsl.input_name, hlsl.output_name));
    out.push_str(&format!("    var output: {};\n", hlsl.output_name));
    for line in &hlsl.body_lines {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("    return output;\n}\n");
    out
}

fn emit_struct(out: &mut String, name: &str, fields: &[StructField], kind: ShaderKind, is_input: bool) {
    out.push_str(&format!("struct {} {{\n", name));
    for field in fields {
        let attr = semantic_to_wgsl_attribute(&field.semantic, kind, is_input);
        out.push_str(&format!("    {}{}: {},\n", attr, field.name, wgsl_type(&field.ty)));
    }
    out.push_str("};\n");
}

fn wgsl_type(ty: &str) -> &'static str {
    match ty {
        "float4" | "vec4<f32>" => "vec4<f32>",
        "float3" | "vec3<f32>" => "vec3<f32>",
        "float2" | "vec2<f32>" => "vec2<f32>",
        "float" | "f32" => "f32",
        _ => "vec4<f32>",
    }
}

fn semantic_to_wgsl_attribute(semantic: &str, kind: ShaderKind, is_input: bool) -> String {
    if kind == ShaderKind::Pixel && !is_input {
        if let Some(n) = semantic.strip_prefix("COLOR").and_then(|s| s.parse::<u32>().ok()) {
            return format!("@location({}) ", n);
        }
    }
    if semantic.starts_with("POSITION") {
        if kind == ShaderKind::Vertex && !is_input {
            return "@builtin(position) ".to_string();
        }
        return "@location(0) ".to_string();
    }
    if semantic == "NORMAL" {
        return "@location(1) ".to_string();
    }
    if let Some(n) = semantic.strip_prefix("COLOR").and_then(|s| s.parse::<u32>().ok()) {
        return format!("@location({}) ", 2 + n);
    }
    if let Some(n) = semantic.strip_prefix("TEXCOORD").and_then(|s| s.parse::<u32>().ok()) {
        return format!("@location({}) ", 4 + n);
    }
    "@location(15) ".to_string()
}
