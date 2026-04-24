mod constants;
mod disasm;
mod emit;
mod error;
mod reader;
mod scene;

use constants::SymbolTables;
use disasm::disassemble;
use emit::emit_structured_ss;
use error::{Error, Result};
use scene::{Scene, ScnCmd, ScnProp};
use siglus_assets::scene_pck::{PackScnHeader, ScenePck, ScenePckDecodeOptions};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
struct Args {
    scene_pck: Option<PathBuf>,
    out_dir: Option<PathBuf>,
    scene: Option<String>,
    out: Option<PathBuf>,
    project_dir: Option<PathBuf>,
    key_hex: Option<String>,
    key_file: Option<PathBuf>,
    list: bool,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let mut args = parse_args()?;
    let scene_pck = args
        .scene_pck
        .take()
        .ok_or_else(|| Error::new("Scene.pck path is required"))?;
    let project_dir = args
        .project_dir
        .clone()
        .unwrap_or_else(|| infer_project_dir(&scene_pck));

    let bytes = fs::read(&scene_pck)?;
    let header = read_pack_header_for_key_check(&bytes)?;
    let requires_exe_key =
        header.original_source_header_size > 0 && header.scn_data_exe_angou_mod != 0;
    let exe_key = resolve_exe_key(&args, &project_dir, &scene_pck, requires_exe_key)?;
    let pck = load_scene_pck(&scene_pck, exe_key)?;
    let scenes = parse_scenes_from_pck(&pck)?;

    if args.list {
        print_scene_list(&scenes);
        return Ok(());
    }

    let symbols = SymbolTables::load()?;

    if let Some(scene_sel) = args.scene.as_deref() {
        let index = select_scene(&scenes, scene_sel)?;
        let text = decompile_scene(&scenes[index], &symbols)?;
        if let Some(out) = args.out.as_deref() {
            fs::write(out, text)?;
        } else {
            print!("{text}");
        }
        return Ok(());
    }

    let out_dir = args
        .out_dir
        .as_deref()
        .ok_or_else(|| Error::new("--out-dir is required when decompiling all scenes"))?;
    fs::create_dir_all(out_dir)?;
    for (i, scene) in scenes.iter().enumerate() {
        let text = decompile_scene(scene, &symbols)?;
        let base = scene
            .name
            .as_ref()
            .map(|s| sanitize_file_name(s))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("scene_{i:04}"));
        fs::write(out_dir.join(format!("{base}.ss")), text)?;
    }
    eprintln!("wrote {} scene(s) to {}", scenes.len(), out_dir.display());
    Ok(())
}

fn read_pack_header_for_key_check(bytes: &[u8]) -> Result<PackScnHeader> {
    let has_signature = bytes.len() >= 8 && &bytes[..8] == b"pack_scn";
    PackScnHeader::read(bytes, 0, has_signature).map_err(|e| Error::new(e.to_string()))
}

fn load_scene_pck(scene_pck: &Path, exe_key: Option<[u8; 16]>) -> Result<ScenePck> {
    let opt = ScenePckDecodeOptions {
        exe_angou_element: exe_key.map(|k| k.to_vec()),
        easy_angou_code: Some(siglus_assets::keys::SCENE_KEY.to_vec()),
    };
    ScenePck::load_and_rebuild(scene_pck, &opt).map_err(|e| Error::new(e.to_string()))
}

fn parse_scenes_from_pck(pck: &ScenePck) -> Result<Vec<Scene>> {
    let count = pck
        .header
        .scn_data_cnt
        .max(pck.header.scn_data_index_cnt)
        .max(0) as usize;
    let names = scene_names_by_index(pck);
    let pack_inc_prop_cnt = pck.header.inc_prop_cnt.max(0) as usize;
    let pack_inc_cmd_cnt = pck.header.inc_cmd_cnt.max(0) as usize;
    let pack_inc_prop_names =
        indexed_name_vec(&pck.inc_prop_name_map, pack_inc_prop_cnt, "inc_prop");
    let pack_inc_cmd_names = indexed_name_vec(&pck.inc_cmd_name_map, pack_inc_cmd_cnt, "inc_cmd");
    let pack_inc_props = pck
        .inc_props
        .iter()
        .map(|p| ScnProp {
            form: p.form,
            size: p.size,
        })
        .collect::<Vec<_>>();

    let mut scenes = Vec::new();
    let mut scene_index_to_output_index = vec![None; count];
    for i in 0..count {
        let payload = pck
            .scn_data_slice(i)
            .map_err(|e| Error::new(e.to_string()))?;
        if payload.is_empty() {
            continue;
        }
        let name = names.get(i).cloned().flatten();
        let out_index = scenes.len();
        let mut scene = Scene::parse(name, payload)?;
        scene.pack_inc_prop_cnt = pack_inc_prop_cnt;
        scene.pack_inc_cmd_cnt = pack_inc_cmd_cnt;
        scene.pack_inc_props = pack_inc_props.clone();
        scene.pack_inc_prop_names = pack_inc_prop_names.clone();
        scene.pack_inc_cmd_names = pack_inc_cmd_names.clone();
        scenes.push(scene);
        scene_index_to_output_index[i] = Some(out_index);
    }

    // Pack-level include commands are real user-command entry points in the
    // original lexer. They are stored in Scene.pck, not in each scene's local
    // S_tnm_scn_scn_cmd table. Add them as decompiler roots so shared frame
    // action/user callbacks are not silently omitted from the emitted .ss.
    for (inc_cmd_no, inc_cmd) in pck.inc_cmds.iter().enumerate() {
        if inc_cmd.scn_no < 0 || inc_cmd.offset < 0 {
            continue;
        }
        let scn_no = inc_cmd.scn_no as usize;
        let Some(Some(scene_out_index)) = scene_index_to_output_index.get(scn_no) else {
            continue;
        };
        let scene = &mut scenes[*scene_out_index];
        if inc_cmd.offset as usize >= scene.code.len() {
            continue;
        }
        let name = pck
            .inc_cmd_name_map
            .get(&(inc_cmd_no as u32))
            .cloned()
            .unwrap_or_else(|| format!("__inc_cmd_{inc_cmd_no}"));
        scene.scn_cmds.push(ScnCmd {
            offset: inc_cmd.offset,
        });
        scene.scn_cmd_names.push(format!("inc::{name}"));
    }

    Ok(scenes)
}

fn indexed_name_vec(
    map: &std::collections::HashMap<u32, String>,
    count: usize,
    fallback_prefix: &str,
) -> Vec<String> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        out.push(
            map.get(&(i as u32))
                .cloned()
                .unwrap_or_else(|| format!("{fallback_prefix}_{i}")),
        );
    }
    out
}

fn scene_names_by_index(pck: &ScenePck) -> Vec<Option<String>> {
    let count = pck
        .header
        .scn_data_cnt
        .max(pck.header.scn_data_index_cnt)
        .max(0) as usize;
    let mut names = vec![None; count];
    for (name, index) in &pck.scn_name_map {
        if *index < names.len() {
            names[*index] = Some(name.clone());
        }
    }
    names
}

fn decompile_scene(scene: &Scene, symbols: &SymbolTables) -> Result<String> {
    let mut scene_symbols = symbols.clone();

    // Match C_tnm_scene_lexer numbering exactly: include properties/commands
    // occupy the leading user id range; scene-local ids are offset by the
    // include count.
    for (i, name) in scene.pack_inc_prop_names.iter().enumerate() {
        scene_symbols.install_user_prop_name(i, name);
    }
    for (i, name) in scene.scn_prop_names.iter().enumerate() {
        scene_symbols.install_user_prop_name(scene.pack_inc_prop_cnt + i, name);
    }
    for (i, name) in scene.pack_inc_cmd_names.iter().enumerate() {
        scene_symbols.install_user_cmd_name(i, name);
    }
    let local_cmd_name_cnt = scene.header.scn_cmd_name_cnt.max(0) as usize;
    for i in 0..local_cmd_name_cnt.min(scene.scn_cmd_names.len()) {
        scene_symbols.install_user_cmd_name(scene.pack_inc_cmd_cnt + i, &scene.scn_cmd_names[i]);
    }

    let insns = disassemble(scene)?;
    Ok(emit_structured_ss(scene, &insns, &scene_symbols))
}

fn parse_args() -> Result<Args> {
    let mut out = Args::default();
    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--scene-pck" | "--pack" => out.scene_pck = Some(next_path(&mut it, &arg)?),
            "--out-dir" => out.out_dir = Some(next_path(&mut it, "--out-dir")?),
            "--scene" => out.scene = Some(next_value(&mut it, "--scene")?),
            "--out" => out.out = Some(next_path(&mut it, "--out")?),
            "--project-dir" => out.project_dir = Some(next_path(&mut it, "--project-dir")?),
            "--key" => out.key_hex = Some(next_value(&mut it, "--key")?),
            "--key-file" => out.key_file = Some(next_path(&mut it, "--key-file")?),
            "--list" => out.list = true,
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other if other.starts_with('-') => {
                return Err(Error::new(format!("unknown argument: {other}")))
            }
            other => {
                if out.scene_pck.is_some() {
                    return Err(Error::new(format!(
                        "unexpected positional argument: {other}"
                    )));
                }
                out.scene_pck = Some(PathBuf::from(other));
            }
        }
    }
    Ok(out)
}

fn resolve_exe_key(
    args: &Args,
    project_dir: &Path,
    scene_pck: &Path,
    required: bool,
) -> Result<Option<[u8; 16]>> {
    if let Some(hex) = args.key_hex.as_deref() {
        return parse_key16(hex).map(Some);
    }
    if let Some(path) = args.key_file.as_deref() {
        let key = siglus_assets::key_toml::load_key16_from_file(path)
            .map_err(|e| Error::new(e.to_string()))?;
        if required && key.is_none() {
            return Err(Error::new(format!(
                "Scene.pck needs exe-angou decryption, but key file {} did not contain a 16-byte key",
                path.display()
            )));
        }
        return Ok(key);
    }

    let mut candidates = Vec::new();
    candidates.push(project_dir.join("key.toml"));
    if let Some(parent) = scene_pck.parent() {
        candidates.push(parent.join("key.toml"));
        if let Some(grand) = parent.parent() {
            candidates.push(grand.join("key.toml"));
        }
    }
    dedup_paths(&mut candidates);

    for candidate in candidates {
        if candidate.is_file() {
            let key = siglus_assets::key_toml::load_key16_from_file(&candidate)
                .map_err(|e| Error::new(e.to_string()))?;
            if key.is_some() {
                return Ok(key);
            }
        }
    }

    if required {
        return Err(Error::new(
            "Scene.pck needs exe-angou decryption. Provide --key <32 hex digits> or place key.toml with key/key_hex beside the project or Scene.pck",
        ));
    }
    Ok(None)
}

fn parse_key16(s: &str) -> Result<[u8; 16]> {
    let bytes = parse_hex(s)?;
    if bytes.len() != 16 {
        return Err(Error::new(format!(
            "--key must contain exactly 16 bytes / 32 hex digits, got {} byte(s)",
            bytes.len()
        )));
    }
    let mut out = [0u8; 16];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn parse_hex(s: &str) -> Result<Vec<u8>> {
    let mut filtered = String::new();
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '0' && matches!(chars.peek(), Some('x' | 'X')) {
            chars.next();
            continue;
        }
        if ch.is_ascii_hexdigit() {
            filtered.push(ch);
        } else if matches!(
            ch,
            ',' | ' ' | '_' | '-' | ':' | '[' | ']' | '\n' | '\r' | '\t'
        ) {
            continue;
        } else {
            return Err(Error::new(format!(
                "invalid hex character in --key: {ch:?}"
            )));
        }
    }
    if filtered.len() % 2 != 0 {
        return Err(Error::new("hex key must contain an even number of digits"));
    }
    let mut out = Vec::with_capacity(filtered.len() / 2);
    for chunk in filtered.as_bytes().chunks(2) {
        let pair = std::str::from_utf8(chunk).map_err(|e| Error::new(e.to_string()))?;
        let byte = u8::from_str_radix(pair, 16)
            .map_err(|_| Error::new(format!("invalid hex byte: {pair}")))?;
        out.push(byte);
    }
    Ok(out)
}

fn dedup_paths(paths: &mut Vec<PathBuf>) {
    let mut out = Vec::new();
    for p in paths.drain(..) {
        if !out.iter().any(|x: &PathBuf| x == &p) {
            out.push(p);
        }
    }
    *paths = out;
}

fn infer_project_dir(scene_pck: &Path) -> PathBuf {
    let parent = scene_pck.parent().unwrap_or_else(|| Path::new("."));
    let name = parent.file_name().and_then(|s| s.to_str()).unwrap_or("");
    if matches!(name, "Data" | "data" | "DAT" | "dat") {
        parent.parent().unwrap_or(parent).to_path_buf()
    } else {
        parent.to_path_buf()
    }
}

fn select_scene(scenes: &[Scene], sel: &str) -> Result<usize> {
    if let Ok(index) = sel.parse::<usize>() {
        if index < scenes.len() {
            return Ok(index);
        }
        return Err(Error::new(format!("scene index {index} is out of range")));
    }
    for (i, scene) in scenes.iter().enumerate() {
        if scene.name.as_deref() == Some(sel) {
            return Ok(i);
        }
    }
    Err(Error::new(format!("scene not found: {sel}")))
}

fn print_scene_list(scenes: &[Scene]) {
    for (i, scene) in scenes.iter().enumerate() {
        let name = scene.name.as_deref().unwrap_or("");
        println!("{i}\t{name}");
    }
}

fn next_path(it: &mut impl Iterator<Item = String>, flag: &str) -> Result<PathBuf> {
    Ok(PathBuf::from(next_value(it, flag)?))
}

fn next_value(it: &mut impl Iterator<Item = String>, flag: &str) -> Result<String> {
    it.next()
        .ok_or_else(|| Error::new(format!("{flag} requires a value")))
}

fn sanitize_file_name(s: &str) -> String {
    s.chars()
        .map(|ch| match ch {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => ch,
        })
        .collect()
}

fn print_usage() {
    eprintln!(
        "usage:\n  siglus_ss_decompiler Scene.pck --out-dir DIR [--project-dir DIR] [--key HEX]\n  siglus_ss_decompiler --scene-pck Scene.pck --scene NAME_OR_INDEX [--out FILE] [--key HEX]\n  siglus_ss_decompiler --scene-pck Scene.pck --list"
    );
}
