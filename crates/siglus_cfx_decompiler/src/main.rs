use std::env;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use siglus_cfx_decompiler::cfx::{disassemble_blob, scan_shaders, ShaderBlob};
use siglus_cfx_decompiler::disasm::{parse_shader, ShaderKind};
use siglus_cfx_decompiler::effect::{
    format_effect_map, parse_effect, safe_name, used_shader_indices,
    write_outputs_for_blob, EffectFile,
};
use siglus_cfx_decompiler::hlsl::decompile_hlsl;
use siglus_cfx_decompiler::hlsl_ref::{discover_reference_hlsl_roots, load_reference_hlsl, transpile_reference_hlsl_to_wgsl};
use siglus_cfx_decompiler::names::format_original_name_report;
use siglus_cfx_decompiler::semantic_wgsl::rewrite_wgsl_for_stage;
use siglus_cfx_decompiler::wgsl::decompile_wgsl;

fn main() {
    if let Err(e) = run() {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut input: Option<PathBuf> = None;
    let mut out_dir = PathBuf::from("output");

    let args = env::args().skip(1).collect::<Vec<_>>();
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "--out" | "-o" => {
                i += 1;
                if i >= args.len() {
                    return Err("--out requires a directory".into());
                }
                out_dir = PathBuf::from(&args[i]);
            }
            "--help" | "-h" => {
                print_usage();
                return Ok(());
            }
            x if x.starts_with('-') => return Err(format!("unknown option: {x}").into()),
            x => {
                if input.is_some() {
                    return Err("only one input path is supported".into());
                }
                input = Some(PathBuf::from(x));
            }
        }
        i += 1;
    }

    let input = input.ok_or("missing input file")?;
    let data = fs::read(&input)?;
    let reference_hlsl_roots = discover_reference_hlsl_roots(&input);
    prepare_output_dirs(&out_dir)?;

    if is_raw_shader(&data) {
        let blob = make_raw_blob(&data)?;
        write_named_blob(&out_dir, "raw_shader", &blob, &reference_hlsl_roots)?;
        fs::write(out_dir.join("summary.txt"), format_summary(&input, &[blob.clone()], None))?;
        fs::write(out_dir.join("original_names.txt"), format_original_name_report(&input, &data, &[blob]))?;
        println!("wrote raw shader output to {}", out_dir.display());
        return Ok(());
    }

    let shaders = scan_shaders(&data);
    if shaders.is_empty() {
        return Err("no D3D9 shader blobs found".into());
    }

    let effect = match parse_effect(&data, &shaders) {
        Ok(effect) => Some(effect),
        Err(e) => {
            fs::write(out_dir.join("technique_map.txt"), format!("effect parser failed: {e}\n"))?;
            None
        }
    };

    if let Some(effect) = &effect {
        fs::write(out_dir.join("technique_map.txt"), format_effect_map(effect, &shaders))?;
        fs::write(out_dir.join("wgpu_pipeline_map.json"), format_wgpu_pipeline_map(effect))?;
        write_technique_named_outputs(&out_dir, effect, &shaders, &reference_hlsl_roots)?;
        write_unmapped_outputs(&out_dir, effect, &shaders, &reference_hlsl_roots)?;
    } else {
        for blob in &shaders {
            write_named_blob(&out_dir, &format!("unmapped__{}", blob.file_prefix()), blob, &reference_hlsl_roots)?;
        }
    }

    fs::write(out_dir.join("summary.txt"), format_summary(&input, &shaders, effect.as_ref()))?;
    fs::write(out_dir.join("original_names.txt"), format_original_name_report(&input, &data, &shaders))?;

    println!("wrote decompiler output to {}", out_dir.display());
    Ok(())
}

fn prepare_output_dirs(out_dir: &Path) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(out_dir)?;
    fs::create_dir_all(out_dir.join("hlsl"))?;
    fs::create_dir_all(out_dir.join("hlsl_original"))?;
    fs::create_dir_all(out_dir.join("hlsl_rewritten"))?;
    fs::create_dir_all(out_dir.join("wgsl"))?;
    fs::create_dir_all(out_dir.join("wgsl_rewritten"))?;
    fs::create_dir_all(out_dir.join("bytecode"))?;
    fs::create_dir_all(out_dir.join("asm"))?;
    fs::create_dir_all(out_dir.join("techniques"))?;
    Ok(())
}

fn print_usage() {
    eprintln!("usage: cfx-decompiler <input.cfx|shader.fxc> [--out output]");
}

fn is_raw_shader(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }
    let v = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    matches!(v & 0xffff_0000, 0xfffe_0000 | 0xffff_0000)
}

fn make_raw_blob(data: &[u8]) -> Result<ShaderBlob, Box<dyn Error>> {
    let shader = parse_shader(data)?;
    Ok(ShaderBlob {
        index: 0,
        kind: shader.kind,
        major: shader.major,
        minor: shader.minor,
        offset: 0,
        end_offset: data.len(),
        bytes: data.to_vec(),
        ctab: None,
    })
}

fn write_technique_named_outputs(out_dir: &Path, effect: &EffectFile, shaders: &[ShaderBlob], reference_hlsl_roots: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let by_index: std::collections::BTreeMap<usize, &ShaderBlob> = shaders.iter().map(|s| (s.index, s)).collect();
    for tech in &effect.techniques {
        let tech_name = safe_name(&tech.name, &format!("technique_{}", tech.index));
        for pass in &tech.passes {
            let pass_name = safe_name(&pass.name, &format!("pass{}", pass.index));
            let prefix_base = format!("t{:04}_{}__p{:02}_{}", tech.index, tech_name, pass.index, pass_name);
            if let Some(vs) = &pass.vertex_shader {
                if let Some(idx) = vs.shader_index {
                    if let Some(blob) = by_index.get(&idx) {
                        write_named_blob_for_technique(out_dir, &format!("{}__vs", prefix_base), blob, &tech.name, reference_hlsl_roots)?;
                    }
                }
            }
            if let Some(ps) = &pass.pixel_shader {
                if let Some(idx) = ps.shader_index {
                    if let Some(blob) = by_index.get(&idx) {
                        write_named_blob_for_technique(out_dir, &format!("{}__ps", prefix_base), blob, &tech.name, reference_hlsl_roots)?;
                    }
                }
            }
        }
    }

    for tech in &effect.techniques {
        let tech_name = format!("t{:04}_{}", tech.index, safe_name(&tech.name, &format!("technique_{}", tech.index)));
        let mut s = String::new();
        s.push_str(&format!("technique {}\n", tech.name));
        for pass in &tech.passes {
            s.push_str(&format!("  pass {}\n", pass.name));
            if let Some(vs) = &pass.vertex_shader {
                s.push_str(&format!("    VS shader_index={:?} offset={:?} object_id={:?}\n", vs.shader_index, vs.shader_offset, vs.object_id));
            }
            if let Some(ps) = &pass.pixel_shader {
                s.push_str(&format!("    PS shader_index={:?} offset={:?} object_id={:?}\n", ps.shader_index, ps.shader_offset, ps.object_id));
            }
            for st in &pass.states {
                s.push_str(&format!("    state op={} {} index={} value={:?}\n", st.operation, st.operation_name, st.state_index, st.value));
            }
        }
        fs::write(out_dir.join("techniques").join(format!("{}.txt", tech_name)), s)?;
    }
    Ok(())
}

fn write_unmapped_outputs(out_dir: &Path, effect: &EffectFile, shaders: &[ShaderBlob], reference_hlsl_roots: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let used = used_shader_indices(effect);
    for blob in shaders {
        if !used.contains(&blob.index) {
            write_named_blob(out_dir, &format!("unmapped__{}", blob.file_prefix()), blob, reference_hlsl_roots)?;
        }
    }
    Ok(())
}

fn write_named_blob(out_dir: &Path, prefix: &str, blob: &ShaderBlob, reference_hlsl_roots: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let original_hlsl = load_reference_hlsl(reference_hlsl_roots, prefix);
    let rewritten_hlsl = decompile_hlsl(&blob.bytes, blob.ctab.as_ref());
    let public_hlsl = original_hlsl.as_deref().unwrap_or(&rewritten_hlsl);
    let rewritten_wgsl = match original_hlsl.as_deref() {
        Some(src) => transpile_reference_hlsl_to_wgsl(src, blob.kind).unwrap_or_else(|_| decompile_wgsl(&blob.bytes, blob.ctab.as_ref())),
        None => decompile_wgsl(&blob.bytes, blob.ctab.as_ref()),
    };
    let asm = disassemble_blob(blob);
    let ctab = blob.ctab.as_ref().map(format_ctab);
    write_outputs_for_blob(out_dir, prefix, blob, public_hlsl, &asm, ctab.as_deref())?;
    write_extended_outputs(out_dir, prefix, original_hlsl.as_deref(), &rewritten_hlsl, &rewritten_wgsl)?;
    Ok(())
}

fn write_named_blob_for_technique(out_dir: &Path, prefix: &str, blob: &ShaderBlob, technique_name: &str, reference_hlsl_roots: &[PathBuf]) -> Result<(), Box<dyn Error>> {
    let original_hlsl = load_reference_hlsl(reference_hlsl_roots, prefix);
    let rewritten_hlsl = decompile_hlsl(&blob.bytes, blob.ctab.as_ref());
    let public_hlsl = original_hlsl.as_deref().unwrap_or(&rewritten_hlsl);
    let rewritten_wgsl = match original_hlsl.as_deref() {
        Some(src) => transpile_reference_hlsl_to_wgsl(src, blob.kind).unwrap_or_else(|_| rewrite_wgsl_for_stage(technique_name, blob.kind)
            .unwrap_or_else(|| decompile_wgsl(&blob.bytes, blob.ctab.as_ref()))),
        None => rewrite_wgsl_for_stage(technique_name, blob.kind)
            .unwrap_or_else(|| decompile_wgsl(&blob.bytes, blob.ctab.as_ref())),
    };
    let asm = disassemble_blob(blob);
    let ctab = blob.ctab.as_ref().map(format_ctab);
    write_outputs_for_blob(out_dir, prefix, blob, public_hlsl, &asm, ctab.as_deref())?;
    write_extended_outputs(out_dir, prefix, original_hlsl.as_deref(), &rewritten_hlsl, &rewritten_wgsl)?;
    Ok(())
}

fn write_extended_outputs(out_dir: &Path, prefix: &str, original_hlsl: Option<&str>, rewritten_hlsl: &str, rewritten_wgsl: &str) -> Result<(), Box<dyn Error>> {
    if let Some(src) = original_hlsl {
        fs::write(out_dir.join("hlsl_original").join(format!("{prefix}.hlsl")), src)?;
    }
    fs::write(out_dir.join("hlsl_rewritten").join(format!("{prefix}.hlsl")), rewritten_hlsl)?;
    fs::write(out_dir.join("wgsl_rewritten").join(format!("{prefix}.wgsl")), rewritten_wgsl)?;
    fs::write(out_dir.join("wgsl").join(format!("{prefix}.wgsl")), rewritten_wgsl)?;
    Ok(())
}

fn format_wgpu_pipeline_map(effect: &EffectFile) -> String {
    let mut s = String::new();
    s.push_str("{\n");
    s.push_str("  \"techniques\": [\n");
    for (ti, tech) in effect.techniques.iter().enumerate() {
        if ti > 0 { s.push_str(",\n"); }
        s.push_str("    {\n");
        s.push_str(&format!("      \"index\": {},\n", tech.index));
        s.push_str(&format!("      \"name\": \"{}\",\n", json_escape(&tech.name)));
        s.push_str("      \"passes\": [\n");
        for (pi, pass) in tech.passes.iter().enumerate() {
            if pi > 0 { s.push_str(",\n"); }
            let tech_name = safe_name(&tech.name, &format!("technique_{}", tech.index));
            let pass_name = safe_name(&pass.name, &format!("pass{}", pass.index));
            let prefix_base = format!("t{:04}_{}__p{:02}_{}", tech.index, tech_name, pass.index, pass_name);
            s.push_str("        {\n");
            s.push_str(&format!("          \"index\": {},\n", pass.index));
            s.push_str(&format!("          \"name\": \"{}\",\n", json_escape(&pass.name)));
            s.push_str(&format!("          \"vertex_wgsl\": {},\n", json_opt_shader_file(&prefix_base, "vs", pass.vertex_shader.as_ref().and_then(|v| v.shader_index))));
            s.push_str(&format!("          \"fragment_wgsl\": {},\n", json_opt_shader_file(&prefix_base, "ps", pass.pixel_shader.as_ref().and_then(|p| p.shader_index))));
            s.push_str("          \"states\": [\n");
            for (si, st) in pass.states.iter().enumerate() {
                if si > 0 { s.push_str(",\n"); }
                s.push_str("            {");
                s.push_str(&format!("\"index\": {}, ", st.index));
                s.push_str(&format!("\"operation\": {}, ", st.operation));
                s.push_str(&format!("\"operation_name\": \"{}\", ", json_escape(&st.operation_name)));
                s.push_str(&format!("\"class\": \"{}\", ", json_escape(&st.class_name)));
                s.push_str(&format!("\"state_index\": {}, ", st.state_index));
                s.push_str(&format!("\"parameter\": \"{}\", ", json_escape(&st.parameter.name)));
                s.push_str(&format!("\"value\": {}", json_state_value(&st.value)));
                s.push_str("}");
            }
            s.push_str("\n          ]\n");
            s.push_str("        }");
        }
        s.push_str("\n      ]\n");
        s.push_str("    }");
    }
    s.push_str("\n  ]\n");
    s.push_str("}\n");
    s
}

fn json_opt_shader_file(prefix: &str, suffix: &str, shader_index: Option<usize>) -> String {
    if shader_index.is_some() {
        format!("\"wgsl/{}__{}.wgsl\"", json_escape(prefix), suffix)
    } else {
        "null".to_string()
    }
}

fn json_state_value(value: &siglus_cfx_decompiler::effect::StateValue) -> String {
    match value {
        siglus_cfx_decompiler::effect::StateValue::Empty => "null".to_string(),
        siglus_cfx_decompiler::effect::StateValue::ObjectId(id) => format!("{id}"),
        siglus_cfx_decompiler::effect::StateValue::Int(xs) => json_i32_array(xs),
        siglus_cfx_decompiler::effect::StateValue::Float(xs) => json_f32_array(xs),
        siglus_cfx_decompiler::effect::StateValue::Bool(xs) => json_bool_array(xs),
        siglus_cfx_decompiler::effect::StateValue::StringObject { object_id, text } => {
            format!("{{\"object_id\": {}, \"text\": {}}}", object_id, text.as_ref().map(|v| format!("\"{}\"", json_escape(v))).unwrap_or_else(|| "null".to_string()))
        }
        siglus_cfx_decompiler::effect::StateValue::Raw { offset, bytes } => {
            format!("{{\"offset\": {}, \"bytes\": {}}}", offset, bytes)
        }
    }
}

fn json_i32_array(xs: &[i32]) -> String {
    format!("[{}]", xs.iter().map(|v| v.to_string()).collect::<Vec<_>>().join(", "))
}

fn json_f32_array(xs: &[f32]) -> String {
    format!("[{}]", xs.iter().map(|v| format_float_json(*v)).collect::<Vec<_>>().join(", "))
}

fn json_bool_array(xs: &[bool]) -> String {
    format!("[{}]", xs.iter().map(|v| if *v { "true" } else { "false" }.to_string()).collect::<Vec<_>>().join(", "))
}

fn format_float_json(v: f32) -> String {
    if v.is_finite() {
        let mut s = format!("{:.9}", v);
        while s.contains('.') && s.ends_with('0') { s.pop(); }
        if s.ends_with('.') { s.push('0'); }
        s
    } else {
        "0.0".to_string()
    }
}

fn json_escape(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < ' ' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn format_ctab(ctab: &siglus_cfx_decompiler::ctab::ConstantTable) -> String {
    let mut s = String::new();
    if let Some(creator) = &ctab.creator {
        s.push_str(&format!("creator: {}\n", creator));
    }
    if let Some(target) = &ctab.target {
        s.push_str(&format!("target: {}\n", target));
    }
    s.push_str(&format!("version: 0x{:08x}\n", ctab.version));
    s.push_str(&format!("flags: 0x{:08x}\n", ctab.flags));
    s.push_str(&format!("constants: {}\n", ctab.constants.len()));
    for c in &ctab.constants {
        s.push_str(&format!("{} {} {} count={}\n", c.register_name(), c.hlsl_decl_type(), c.name, c.register_count));
    }
    s
}

fn format_summary(input: &Path, shaders: &[ShaderBlob], effect: Option<&EffectFile>) -> String {
    let mut s = String::new();
    let ps = shaders.iter().filter(|b| b.kind == ShaderKind::Pixel).count();
    let vs = shaders.iter().filter(|b| b.kind == ShaderKind::Vertex).count();
    s.push_str(&format!("input: {}\n", input.display()));
    s.push_str(&format!("shaders_total: {}\n", shaders.len()));
    s.push_str(&format!("pixel_shaders: {}\n", ps));
    s.push_str(&format!("vertex_shaders: {}\n", vs));
    if let Some(effect) = effect {
        s.push_str("effect_parser: ok\n");
        s.push_str(&format!("techniques: {}\n", effect.techniques.len()));
        s.push_str(&format!("objects: {}\n", effect.objects.len()));
        s.push_str("primary_shader_naming: tNNNN_technique__pNN_pass__vs/ps\n");
    } else {
        s.push_str("effect_parser: failed_or_not_effect\n");
        s.push_str("primary_shader_naming: unmapped__ps/vs_index_offset\n");
    }
    s.push_str("outputs: wgsl/*.wgsl, hlsl/*.hlsl, asm/*.asm, bytecode/*.bin, technique_map.txt, wgpu_pipeline_map.json, techniques/*.txt\n");
    for b in shaders {
        s.push_str(&format!(
            "{} {} offset=0x{:08x} end=0x{:08x} size={} ctab={}\n",
            b.file_prefix(),
            b.profile(),
            b.offset,
            b.end_offset,
            b.bytes.len(),
            b.ctab.is_some()
        ));
    }
    s
}
