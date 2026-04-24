use anyhow::Result;
use encoding_rs::{SHIFT_JIS, UTF_16BE, UTF_16LE};
use std::fs;
use std::path::{Path, PathBuf};

use crate::runtime::forms::prop_access;
use crate::runtime::{constants, CommandContext, Value};

fn resolve_text_file_path(project_dir: &Path, append_dir: &str, raw: &str) -> Option<PathBuf> {
    let raw_path = Path::new(raw);
    let mut candidates = Vec::new();
    if raw_path.is_absolute() {
        candidates.push(raw_path.to_path_buf());
    } else {
        candidates.push(project_dir.join(raw_path));
        candidates.push(project_dir.join("dat").join(raw_path));
        candidates.push(project_dir.join("save").join(raw_path));
        candidates.push(project_dir.join("savedata").join(raw_path));
        if !append_dir.is_empty() {
            let append = Path::new(append_dir);
            if append.is_absolute() {
                candidates.push(append.join(raw_path));
            } else {
                candidates.push(project_dir.join(append).join(raw_path));
            }
        }
    }

    let mut expanded = Vec::new();
    for base in candidates {
        expanded.push(base.clone());
        if base.extension().is_none() {
            expanded.push(base.with_extension("txt"));
        }
    }
    expanded.into_iter().find(|p| p.exists())
}

fn decode_text(bytes: &[u8]) -> String {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let (cow, _, _) = UTF_16LE.decode(&bytes[2..]);
        return cow.into_owned();
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let (cow, _, _) = UTF_16BE.decode(&bytes[2..]);
        return cow.into_owned();
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    let (cow, _, _) = SHIFT_JIS.decode(bytes);
    cow.into_owned()
}

fn split_lines(text: String) -> Vec<String> {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(|s| s.to_string())
        .collect()
}

fn strlist_key_from_value(value: &Value) -> Option<u32> {
    match value.unwrap_named() {
        Value::Element(chain) => chain.first().copied().map(|v| v as u32),
        Value::Int(v) if *v >= 0 => Some(*v as u32),
        _ => None,
    }
}

fn handle_load_txt(ctx: &mut CommandContext, params: &[Value]) -> Result<bool> {
    let file_name = params.get(0).and_then(|v| v.as_str()).unwrap_or("");
    let Some(target_key) = params.get(1).and_then(strlist_key_from_value) else {
        ctx.unknown.record_note("FILE.LOAD_TXT missing STRLIST target");
        ctx.push(Value::Int(0));
        return Ok(true);
    };
    if file_name.is_empty() {
        ctx.unknown.record_note("FILE.LOAD_TXT empty file name");
        ctx.push(Value::Int(0));
        return Ok(true);
    }
    let Some(path) = resolve_text_file_path(&ctx.project_dir, &ctx.globals.append_dir, file_name) else {
        ctx.unknown
            .record_note(&format!("FILE.LOAD_TXT missing file:{file_name}"));
        ctx.push(Value::Int(0));
        return Ok(true);
    };
    match fs::read(&path) {
        Ok(bytes) => {
            let lines = split_lines(decode_text(&bytes));
            let dst = ctx.globals.str_lists.entry(target_key).or_default();
            dst.clear();
            dst.extend(lines);
            ctx.push(Value::Int(1));
        }
        Err(e) => {
            ctx.unknown
                .record_note(&format!("FILE.LOAD_TXT read failed:{}:{e}", path.display()));
            ctx.push(Value::Int(0));
        }
    }
    Ok(true)
}

fn preload_omv(ctx: &mut CommandContext, name: &str) {
    if name.is_empty() {
        return;
    }
    match crate::resource::find_omv_path_with_append_dir(&ctx.project_dir, &ctx.globals.append_dir, name) {
        Ok(path) => match fs::File::open(&path) {
            Ok(mut file) => {
                use std::io::Read;
                let mut buf = vec![0u8; 1024 * 1024];
                let _ = file.read(&mut buf);
            }
            Err(e) => {
                ctx.unknown.record_note(&format!(
                    "FILE.PRELOAD_OMV open failed:{}:{e}",
                    path.display()
                ));
            }
        },
        Err(e) => {
            ctx.unknown.record_note(&format!("FILE.PRELOAD_OMV failed:{name}:{e}"));
        }
    }
}

pub fn dispatch(ctx: &mut CommandContext, form_id: u32, args: &[Value]) -> Result<bool> {
    let parsed = prop_access::parse_element_chain_ctx(ctx, form_id, args);
    let (chain_pos, chain) = match parsed {
        Some((pos, ch)) if ch.len() >= 2 => (Some(pos), Some(ch)),
        _ => (None, None),
    };

    if let Some(chain) = chain {
        let op = chain[1];
        let params = if let Some(pos) = chain_pos {
            prop_access::script_args(args, pos)
        } else {
            &[]
        };
        let p_str = |i: usize| -> &str { params.get(i).and_then(|v| v.as_str()).unwrap_or("") };

        if op == constants::elm_value::FILE_LOAD_TXT {
            return handle_load_txt(ctx, params);
        }

        if op == constants::elm_value::FILE_PRELOAD_OMV {
            preload_omv(ctx, p_str(0));
            ctx.push(Value::Int(0));
            return Ok(true);
        }

        return Ok(false);
    }

    if let Some(op) = args.get(0).and_then(|v| v.as_i64()) {
        if op == constants::elm_value::FILE_LOAD_TXT as i64 {
            return handle_load_txt(ctx, &args[1..]);
        }
        if op == constants::elm_value::FILE_PRELOAD_OMV as i64 {
            let name = args.get(1).and_then(|v| v.as_str()).unwrap_or("");
            preload_omv(ctx, name);
            ctx.push(Value::Int(0));
            return Ok(true);
        }
    }

    Ok(false)
}
