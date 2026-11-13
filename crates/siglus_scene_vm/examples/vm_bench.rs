//! Times the VM headlessly (script and element frame work, no renderer) on
//! the real clock, clicking through text, to measure interpreter cost; used
//! to find where console frames go.
//!
//! ```text
//! vm_bench <project dir> <scene> [frames]
//! ```
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use siglus_assets::scene_pck::ScenePck;
use siglus_scene_vm::runtime::CommandContext;
use siglus_scene_vm::runtime::input::VmMouseButton;
use siglus_scene_vm::scene_stream::SceneStream;
use siglus_scene_vm::vm::{SceneVm, VmConfig};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let project = PathBuf::from(
        args.next()
            .context("usage: vm_bench <project> <scene> [frames]")?,
    );
    let scene = args.next().context("missing scene")?;
    let frames: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(1500);

    let pck_path = siglus_scene_vm::resource::find_scene_pck_path(&project)?;
    let options = siglus_scene_vm::resource::load_scene_pck_decode_options(&project)?;
    let pack = ScenePck::load_lazy(&pck_path, &options)?;
    let scn_no = pack
        .find_scene_no(&scene)
        .with_context(|| format!("scene not found: {scene}"))?;
    let (owner, range) = pack.scn_data_shared(scn_no)?;
    let stream = SceneStream::new_shared_range_with_string_codec(owner, range, pack.string_codec)?;
    let mut ctx = CommandContext::new(project);
    let append = ctx.globals.append_dir.clone();
    ctx.install_scene_metadata(&append, &pack)?;
    ctx.screen_w = 1920;
    ctx.screen_h = 1080;
    let mut vm = SceneVm::with_config(VmConfig::from_env(), stream, ctx);
    vm.install_initial_scene_pck(pack, append);
    vm.cfg.max_steps = 1_000_000;
    vm.restart_scene_name(&scene, 0)?;

    let (mut script, mut tick) = (Duration::ZERO, Duration::ZERO);
    let (mut window_script, mut window_tick) = (Duration::ZERO, Duration::ZERO);
    for frame in 0..frames {
        let frame_start = Instant::now();
        let start = Instant::now();
        let _ = vm.run_script_proc();
        let script_time = start.elapsed();
        let start = Instant::now();
        let _ = vm.tick_frame();
        let tick_time = start.elapsed();
        script += script_time;
        tick += tick_time;
        window_script += script_time;
        window_tick += tick_time;
        vm.ctx.on_mouse_move(960, 540);
        match frame % 20 {
            0 => vm.ctx.on_mouse_down(VmMouseButton::Left),
            2 => vm.ctx.on_mouse_up(VmMouseButton::Left),
            _ => {}
        }
        if frame % 300 == 299 {
            println!(
                "frames {:5}: script {:6.0} us/frame, tick {:6.0} us/frame | {:?}",
                frame + 1,
                window_script.as_micros() as f64 / 300.0,
                window_tick.as_micros() as f64 / 300.0,
                vm.current_scene_name()
            );
            window_script = Duration::ZERO;
            window_tick = Duration::ZERO;
        }
        // Waits run on the real clock.
        std::thread::sleep(Duration::from_millis(16).saturating_sub(frame_start.elapsed()));
    }
    println!(
        "total: script {:.0} us/frame, tick {:.0} us/frame",
        script.as_micros() as f64 / frames as f64,
        tick.as_micros() as f64 / frames as f64
    );
    Ok(())
}
