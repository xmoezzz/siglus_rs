use anyhow::{Context, Result};
use siglus_assets::scene_pck::{find_scene_pck_in_project, ScenePck, ScenePckDecodeOptions};
use siglus_scene_vm::runtime::input::VmMouseButton;
use siglus_scene_vm::runtime::CommandContext;
use siglus_scene_vm::scene_stream::SceneStream;
use siglus_scene_vm::vm::{SceneVm, VmConfig};
use std::path::PathBuf;

fn make_vm(scene_name: &str, z: i32) -> Result<SceneVm<'static>> {
    let project = PathBuf::from("/Users/xmoe/Documents/siglus_rs-main/testcase");
    let pck_path = find_scene_pck_in_project(&project)?;
    let opt = ScenePckDecodeOptions::from_project_dir(&project)?;
    let pack = ScenePck::load_and_rebuild(&pck_path, &opt)?;
    let scn_no = pack
        .find_scene_no(scene_name)
        .with_context(|| format!("scene not found: {scene_name}"))?;
    let chunk = pack.scn_data_slice(scn_no)?;
    let chunk_leaked: &'static [u8] = Box::leak(chunk.to_vec().into_boxed_slice());
    let mut stream = SceneStream::new(chunk_leaked)?;
    stream.jump_to_z_label(z as usize)?;
    let mut ctx = CommandContext::new(project);
    ctx.screen_w = 1280;
    ctx.screen_h = 720;
    let mut vm = SceneVm::with_config(VmConfig::from_env(), stream, ctx);
    vm.cfg.max_steps = 1_000_000;
    vm.restart_scene_name(scene_name, z)?;
    Ok(vm)
}

fn main() -> Result<()> {
    let mut vm = make_vm("sys10_tt01", 0)?;
    let mut scene_names = Vec::new();

    for frame in 0..900u32 {
        match frame {
            520 => vm.ctx.on_mouse_move(450, 650),
            522 => vm.ctx.on_mouse_down(VmMouseButton::Left),
            524 => vm.ctx.on_mouse_up(VmMouseButton::Left),
            _ => {}
        }

        let _running = vm.run_script_proc_slice(2000)?;
        vm.tick_frame()?;

        if let Some(name) = vm.current_scene_name() {
            if scene_names.last().map(String::as_str) != Some(name) {
                scene_names.push(name.to_string());
            }
        }
        if frame % 60 == 0 || (518..=530).contains(&frame) {
            let scene = vm.current_scene_name().map(ToOwned::to_owned);
            let line = vm.current_line_no();
            let blocked = vm.is_blocked();
            println!(
                "frame={frame} scene={:?} line={} blocked={}",
                scene, line, blocked
            );
        }
    }

    println!("scene_trace={scene_names:?}");
    Ok(())
}
