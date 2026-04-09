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
                    r#"Usage: scene_trace [--project <dir>] [--pck <Scene.pck>] [--scene <name|index>] [--max-steps N]

Notes:
  - If --project is omitted, the app base path is used.
  - Default app base path is the executable directory.
  - When SIG_TEST=1, the app base path becomes <Cargo.toml sibling>/testcase.
  - This runs the VM directly on Scene.pck bytecode (no script compilation).
  - TEXT/NAME are printed to stdout; unknown opcodes/forms are summarized at the end.

Key file (optional):
  <project>/key.toml
  key = [0x00, 0x11, ..., 0xFF]

Other controls:
  SIG_TEST=1
  SIGLUS_FM_LABEL (i32)
  SIGLUS_FM_LIST (i32)
"#
                );
                return Ok(());
            }
            other => return Err(anyhow!("unknown arg: {}", other)),
        }
    }

    let project_dir = match project_dir {
        Some(p) => p,
        None => siglus_scene_vm::app_path::resolve_app_base_path()?,
    };
    let pck_path = match pck_path {
        Some(p) => p,
        None => find_scene_pck_in_project(&project_dir)?,
    };

    let decode_opt = ScenePckDecodeOptions::from_project_dir(&project_dir)?;
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

    let mut stream = SceneStream::new(chunk)?;
    stream.jump_to_z_label(0)?;
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
