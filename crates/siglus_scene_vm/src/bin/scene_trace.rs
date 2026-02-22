use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};

use siglus_assets::scene_pck::{find_scene_pck_in_project, ScenePck, ScenePckDecodeOptions};

use siglus_scene_vm::runtime::CommandContext;
use siglus_scene_vm::scene_stream::SceneStream;
use siglus_scene_vm::vm::SceneVm;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);

    let mut project_dir: Option<PathBuf> = None;
    let mut pck_path: Option<PathBuf> = None;
    let mut scene_sel: Option<String> = None;
    let mut max_steps: usize = 1_000_000;

    while let Some(a) = args.next() {
        match a.as_str() {
            "--project" => project_dir = args.next().map(PathBuf::from),
            "--pck" => pck_path = args.next().map(PathBuf::from),
            "--scene" => scene_sel = args.next(),
            "--max-steps" => {
                max_steps = args
                    .next()
                    .ok_or_else(|| anyhow!("--max-steps needs a value"))?
                    .parse::<usize>()
                    .context("parse --max-steps")?;
            }
            "--help" | "-h" => {
                eprintln!(
                    "Usage: scene_trace --project <dir> [--pck <Scene.pck>] [--scene <name|index>] [--max-steps N]\n\
                     \n\
                     Notes:\n\
                       - This runs the VM directly on Scene.pck bytecode (no script compilation).\n\
                       - TEXT/NAME are printed to stdout; unknown opcodes/forms are summarized at the end.\n\
                     \n\
                     Environment variables (optional):\n\
                       SIGLUS_SCN_EXE_ANGOU_ELEMENT_HEX (16 bytes hex)\n\
                       SIGLUS_SCN_EASY_ANGOU_CODE_HEX (N bytes hex, commonly 256)\n\
                       SIGLUS_FM_LABEL (i32)\n\
                       SIGLUS_FM_LIST (i32)\n"
                );
                return Ok(());
            }
            other => return Err(anyhow!("unknown arg: {}", other)),
        }
    }

    let project_dir = project_dir.ok_or_else(|| anyhow!("--project is required"))?;
    let pck_path = match pck_path {
        Some(p) => p,
        None => find_scene_pck_in_project(&project_dir)?,
    };

    let decode_opt = ScenePckDecodeOptions::from_env()?;
    let pack = ScenePck::load_and_rebuild(&pck_path, &decode_opt)
        .with_context(|| format!("load Scene.pck from {}", pck_path.display()))?;

    let scn_no = match scene_sel.as_deref() {
        None => 0,
        Some(s) => pack
            .find_scene_no(s)
            .ok_or_else(|| anyhow!("scene not found: {}", s))?,
    };

    let chunk = pack.scn_data_slice(scn_no)?;
    if chunk.is_empty() {
        return Err(anyhow!("scene chunk is empty: {}", scn_no));
    }

    let stream = SceneStream::new(chunk)?;
    let ctx = CommandContext::new(project_dir.clone());
    let mut vm = SceneVm::new(stream, ctx);
    vm.cfg.max_steps = max_steps as u64;

    eprintln!("[scene_trace] scene={} max_steps={}", scn_no, max_steps);
    vm.run()?;

    if !vm.unknown_opcodes.is_empty() {
        eprintln!("[scene_trace] unknown opcodes:");
        for (k, v) in vm.unknown_opcodes.iter() {
            eprintln!("  0x{:02X}: {}", k, v);
        }
    }
    if !vm.unknown_forms.is_empty() {
        eprintln!("[scene_trace] unknown form codes:");
        for (k, v) in vm.unknown_forms.iter() {
            eprintln!("  {}: {}", k, v);
        }
    }
    Ok(())
}
