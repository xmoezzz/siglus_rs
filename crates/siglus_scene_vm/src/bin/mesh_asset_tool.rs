use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use siglus_scene_vm::mesh3d::{compile_mesh_asset_file, internal_mesh_asset_path_for_source};

#[derive(Parser, Debug)]
#[command(name = "mesh_asset_tool")]
struct Args {
    /// Input mesh source file (.x/.obj) or directory.
    input: PathBuf,

    /// Output file or output root directory.
    #[arg(long)]
    output: Option<PathBuf>,

    /// Recurse when the input is a directory.
    #[arg(long, default_value_t = true)]
    recursive: bool,

    /// Overwrite existing .sgmesh files.
    #[arg(long, default_value_t = true)]
    overwrite: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    if args.input.is_file() {
        let output = args
            .output
            .clone()
            .unwrap_or_else(|| internal_mesh_asset_path_for_source(&args.input));
        compile_one(&args.input, &output, args.overwrite)?;
        println!("compiled {:?} -> {:?}", args.input, output);
        return Ok(());
    }
    if args.input.is_dir() {
        compile_dir(
            &args.input,
            args.output.as_deref(),
            args.recursive,
            args.overwrite,
        )?;
        return Ok(());
    }
    anyhow::bail!("input does not exist: {:?}", args.input)
}

fn compile_dir(
    root: &Path,
    output_root: Option<&Path>,
    recursive: bool,
    overwrite: bool,
) -> Result<()> {
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for ent in fs::read_dir(&dir).with_context(|| format!("read dir {:?}", dir))? {
            let ent = ent?;
            let path = ent.path();
            if path.is_dir() {
                if recursive {
                    stack.push(path);
                }
                continue;
            }
            if !is_mesh_source(&path) {
                continue;
            }
            let output = if let Some(out_root) = output_root {
                let rel = path.strip_prefix(root).unwrap_or(&path);
                out_root.join(rel).with_extension("sgmesh")
            } else {
                internal_mesh_asset_path_for_source(&path)
            };
            compile_one(&path, &output, overwrite)?;
            println!("compiled {:?} -> {:?}", path, output);
        }
    }
    Ok(())
}

fn compile_one(input: &Path, output: &Path, overwrite: bool) -> Result<()> {
    if output.exists() && !overwrite {
        return Ok(());
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create output dir {:?}", parent))?;
    }
    compile_mesh_asset_file(input, output)
}

fn is_mesh_source(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|s| s.to_str()).map(|s| s.to_ascii_lowercase()),
        Some(ext) if ext == "x" || ext == "obj"
    )
}
