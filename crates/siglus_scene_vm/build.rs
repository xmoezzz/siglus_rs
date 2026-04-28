use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR is required"));
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR is required"));
    let out_file = out_dir.join("siglus_embedded_font.rs");

    let candidates = ["assets/fonts/default.ttf"];

    for rel in &candidates {
        println!("cargo:rerun-if-changed={}", manifest_dir.join(*rel).display());
    }

    let selected = candidates
        .iter()
        .map(|rel| (*rel, manifest_dir.join(*rel)))
        .find(|(_, path)| is_non_empty_file(path));

    let source = match selected {
        Some((rel, path)) => render_embedded_some(rel, &path),
        None => render_embedded_none(),
    };

    fs::write(out_file, source).expect("failed to write generated embedded font module");
}

fn is_non_empty_file(path: &Path) -> bool {
    path.is_file()
        && path
            .metadata()
            .map(|meta| meta.len() > 0)
            .unwrap_or(false)
}

fn render_embedded_some(rel: &str, path: &Path) -> String {
    let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let include_path = rust_string_literal(&canonical.to_string_lossy());
    let rel_lit = rust_string_literal(rel);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("default");
    let stem_lit = rust_string_literal(stem);
    format!(
        "pub const EMBEDDED_DEFAULT_FONT: Option<&'static [u8]> = Some(include_bytes!({include_path}) as &'static [u8]);\n\
         pub const EMBEDDED_DEFAULT_FONT_SOURCE: Option<&'static str> = Some({rel_lit});\n\
         pub const EMBEDDED_DEFAULT_FONT_ALIASES: &[&str] = &[\"ＭＳ Ｐゴシック\", \"MS PGothic\", \"MS-PGothic\", \"MSPGothic\", \"msgothic\", \"default\", {stem_lit}];\n"
    )
}

fn render_embedded_none() -> String {
    "pub const EMBEDDED_DEFAULT_FONT: Option<&'static [u8]> = None;\n\
     pub const EMBEDDED_DEFAULT_FONT_SOURCE: Option<&'static str> = None;\n\
     pub const EMBEDDED_DEFAULT_FONT_ALIASES: &[&str] = &[];\n"
        .to_string()
}

fn rust_string_literal(s: &str) -> String {
    let mut out = String::from("\"");
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out.push('"');
    out
}
