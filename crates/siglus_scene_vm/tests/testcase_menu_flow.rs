use anyhow::{Context, Result};
use siglus_assets::scene_pck::{find_scene_pck_in_project, ScenePck, ScenePckDecodeOptions};
use siglus_scene_vm::runtime::CommandContext;
use siglus_scene_vm::scene_stream::SceneStream;
use siglus_scene_vm::vm::{SceneVm, VmConfig};
use std::path::PathBuf;

fn project_dir() -> PathBuf {
    PathBuf::from("/Users/xmoe/Documents/siglus_rs-main/testcase")
}

fn write_capture(vm: &mut SceneVm<'static>, name: &str) -> Result<()> {
    let img = vm.ctx.capture_frame_rgba();
    let path = PathBuf::from("/Users/xmoe/Documents/siglus_rs-main/docs").join(name);
    image::save_buffer(
        &path,
        &img.rgba,
        img.width,
        img.height,
        image::ColorType::Rgba8,
    )
    .with_context(|| format!("write capture: {}", path.display()))
}

fn capture_nonzero_alpha(vm: &mut SceneVm<'static>) -> usize {
    let img = vm.ctx.capture_frame_rgba();
    img.rgba
        .chunks_exact(4)
        .filter(|px| px[3] != 0)
        .count()
}

fn capture_nonblack_rgb(vm: &mut SceneVm<'static>) -> usize {
    let img = vm.ctx.capture_frame_rgba();
    img.rgba
        .chunks_exact(4)
        .filter(|px| px[0] != 0 || px[1] != 0 || px[2] != 0)
        .count()
}

fn make_vm(scene_name: &str, z: i32) -> Result<SceneVm<'static>> {
    let project = project_dir();
    let pck_path = find_scene_pck_in_project(&project)?;
    let opt = ScenePckDecodeOptions::from_project_dir(&project)?;
    let pack = ScenePck::load_and_rebuild(&pck_path, &opt)?;
    let scn_no = pack
        .find_scene_no(scene_name)
        .with_context(|| format!("scene not found: {scene_name}"))?;
    let chunk = pack.scn_data_slice(scn_no)?;
    let chunk_leaked: &'static [u8] = Box::leak(chunk.to_vec().into_boxed_slice());
    let mut stream = SceneStream::new(chunk_leaked)?;
    stream.jump_to_z_label(0)?;
    let mut ctx = CommandContext::new(project);
    ctx.screen_w = 1280;
    ctx.screen_h = 720;
    let mut vm = SceneVm::with_config(VmConfig::from_env(), stream, ctx);
    vm.cfg.max_steps = 1_000_000;
    vm.restart_scene_name(scene_name, z)?;
    Ok(vm)
}

fn step_for_frames(vm: &mut SceneVm<'static>, frames: u32) -> Result<Vec<(usize, String, i64)>> {
    let mut last_visible = Vec::new();
    for _ in 0..frames {
        for _ in 0..2000 {
            let running = vm.step()?;
            if !running || vm.is_halted() || vm.is_blocked() {
                break;
            }
        }
        if vm.is_blocked() {
            vm.ctx.wait.notify_key();
        }
        vm.tick_frame()?;
        let mut visible = Vec::new();
        if let Some(st) = vm
            .ctx
            .globals
            .stage_forms
            .get(&vm.ctx.ids.form_global_stage)
        {
            if let Some(objs) = st.object_lists.get(&0) {
                if let Some(root) = objs.get(0) {
                    for (cidx, child) in root.runtime.child_objects.iter().enumerate() {
                        if child.used {
                            let disp = child
                                .lookup_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_disp)
                                .unwrap_or(0);
                            if disp != 0 {
                                visible.push((
                                    cidx,
                                    child.file_name.clone().unwrap_or_default(),
                                    disp,
                                ));
                            }
                        }
                    }
                }
            }
        }
        if !visible.is_empty() {
            last_visible = visible;
        }
    }
    Ok(last_visible)
}

#[test]
fn trace_menu_scene_progress() -> Result<()> {
    let mut vm = make_vm("sys10_tt01", 0)?;
    let mut first_visible_frame: Option<u32> = None;
    let mut first_visible: Vec<(usize, String, i64)> = Vec::new();
    let mut latest_visible: Vec<(usize, String, i64)> = Vec::new();
    let mut latest_playing: Vec<(usize, String, i64, bool, bool)> = Vec::new();
    let mut first_nonzero_frame: Option<u32> = None;
    let mut first_nonzero_alpha = 0usize;
    for frame in 0..700u32 {
        for _ in 0..2000 {
            let running = vm.step()?;
            if !running || vm.is_halted() || vm.is_blocked() {
                break;
            }
        }
        if vm.is_blocked() {
            vm.ctx.wait.notify_key();
        }
        vm.tick_frame()?;
        let mut visible = Vec::new();
        let mut playing = Vec::new();
        if let Some(st) = vm
            .ctx
            .globals
            .stage_forms
            .get(&vm.ctx.ids.form_global_stage)
        {
            if let Some(objs) = st.object_lists.get(&0) {
                if let Some(root) = objs.get(0) {
                    for (cidx, child) in root.runtime.child_objects.iter().enumerate() {
                        if child.used {
                            let disp = child
                                .lookup_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_disp)
                                .unwrap_or(0);
                            if disp != 0 {
                                visible.push((
                                    cidx,
                                    child.file_name.clone().unwrap_or_default(),
                                    disp,
                                ));
                            }
                            if child.object_type == 9 {
                                playing.push((
                                    cidx,
                                    child.file_name.clone().unwrap_or_default(),
                                    disp,
                                    child.movie.playing,
                                    child.movie.pause_flag,
                                ));
                            }
                        }
                    }
                }
            }
        }
        if !visible.is_empty() {
            if first_visible_frame.is_none() {
                first_visible_frame = Some(frame);
                first_visible = visible.clone();
            }
        }
        latest_visible = visible.clone();
        latest_playing = playing;
        if frame == 405 {
            let _ = write_capture(&mut vm, "testcase_menu_0405.png");
        }
        if frame == 699 {
            let _ = write_capture(&mut vm, "testcase_menu_0699.png");
        }
        if first_nonzero_frame.is_none() && frame >= 380 && frame % 10 == 0 {
            let nonzero_alpha = capture_nonzero_alpha(&mut vm);
            if nonzero_alpha > 0 {
                first_nonzero_frame = Some(frame);
                first_nonzero_alpha = nonzero_alpha;
            }
        }
    }
    let frame = first_visible_frame
        .context("title/menu objects never became visible within 700 frames")?;
    let render_frame = first_nonzero_frame
        .context("render submission stayed fully transparent within 700 frames")?;
    eprintln!("first visible frame={frame} visible={first_visible:?}");
    eprintln!("latest visible={latest_visible:?}");
    eprintln!("latest playing={latest_playing:?}");
    eprintln!("first nonzero alpha frame={render_frame} count={first_nonzero_alpha}");
    assert!(
        first_visible
            .iter()
            .any(|(_, file, _)| file == "mn_tt_rpa_sz00"),
        "expected title OMV object mn_tt_rpa_sz00 to become visible, got {first_visible:?}"
    );
    assert!(
        first_nonzero_alpha > 0,
        "expected title render output to become visible by frame {render_frame}"
    );
    Ok(())
}

#[test]
fn restart_same_vm_reaches_visible_title() -> Result<()> {
    let mut vm = make_vm("sys10_su00", 0)?;
    for _ in 0..2000 {
        let running = vm.step()?;
        if !running || vm.is_halted() || vm.is_blocked() {
            break;
        }
    }
    vm.restart_scene_name("sys10_tt01", 0)?;
    let visible = step_for_frames(&mut vm, 500)?;
    let _ = write_capture(&mut vm, "testcase_restart_menu_0500.png");
    let nonblack = capture_nonblack_rgb(&mut vm);
    eprintln!("restart-visible={visible:?}");
    eprintln!("restart-nonblack={nonblack}");
    assert!(
        visible.iter().any(|(_, file, _)| file == "mn_tt_rpa_sz00"),
        "restart-to-menu on same VM never reached visible title: {visible:?}"
    );
    assert!(nonblack > 0, "restart-to-menu title frame stayed black");
    Ok(())
}
