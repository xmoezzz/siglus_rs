use anyhow::{Context, Result};
use siglus_assets::scene_pck::{find_scene_pck_in_project, ScenePck, ScenePckDecodeOptions};
use siglus_scene_vm::runtime::input::VmMouseButton;
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
    img.rgba.chunks_exact(4).filter(|px| px[3] != 0).count()
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

fn advance_one_frame(vm: &mut SceneVm<'static>) -> Result<()> {
    let _running = vm.run_script_proc()?;
    vm.tick_frame()?;
    Ok(())
}

fn step_for_frames(vm: &mut SceneVm<'static>, frames: u32) -> Result<Vec<(usize, String, i64)>> {
    let mut last_visible = Vec::new();
    for _ in 0..frames {
        advance_one_frame(vm)?;
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

fn render_rgba(vm: &mut SceneVm<'static>) -> Vec<u8> {
    vm.ctx.capture_frame_rgba().rgba
}

fn pixel_delta(a: &[u8], b: &[u8]) -> usize {
    a.chunks_exact(4)
        .zip(b.chunks_exact(4))
        .filter(|(lhs, rhs)| lhs != rhs)
        .count()
}

fn dump_button_objects(vm: &SceneVm<'static>, label: &str) {
    fn recur(
        vm: &SceneVm<'static>,
        obj: &siglus_scene_vm::runtime::globals::ObjectState,
        path: &str,
    ) {
        if obj.used && obj.button.enabled {
            eprintln!(
                "{path} file={} disp={} runtime_slot={:?} button_no={} group_no={} action_no={} pushed={} hit={} call={}/{}",
                obj.file_name.clone().unwrap_or_default(),
                obj.lookup_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_disp).unwrap_or(0),
                obj.nested_runtime_slot,
                obj.button.button_no,
                obj.button.group_no,
                obj.button.action_no,
                obj.button.pushed,
                obj.button.hit,
                obj.button.decided_action_scn_name,
                obj.button.decided_action_cmd_name,
            );
        }
        for (idx, child) in obj.runtime.child_objects.iter().enumerate() {
            recur(vm, child, &format!("{path}/{idx}"));
        }
    }

    eprintln!("-- button dump: {label} --");
    let Some(st) = vm
        .ctx
        .globals
        .stage_forms
        .get(&vm.ctx.ids.form_global_stage)
    else {
        eprintln!("no global stage form");
        return;
    };
    if let Some(groups) = st.group_lists.get(&0) {
        for (idx, g) in groups.iter().enumerate() {
            eprintln!(
                "group[{idx}] started={} wait={} cancel={} hit={} pushed={} decided={} result={}",
                g.started,
                g.wait_flag,
                g.cancel_flag,
                g.hit_button_no,
                g.pushed_button_no,
                g.decided_button_no,
                g.result_button_no
            );
        }
    }
    if let Some(objs) = st.object_lists.get(&0) {
        for (idx, obj) in objs.iter().enumerate() {
            recur(vm, obj, &format!("top[{idx}]"));
        }
    }
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
        advance_one_frame(&mut vm)?;
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
    let frame =
        first_visible_frame.context("title/menu objects never became visible within 700 frames")?;
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

#[test]
fn title_hover_changes_framebuffer() -> Result<()> {
    let mut vm = make_vm("sys10_tt01", 0)?;
    let _ = step_for_frames(&mut vm, 520)?;
    let baseline = render_rgba(&mut vm);

    // Start button settles near x=405..499, y=619 in testcase title.
    vm.ctx.on_mouse_move(450, 650);
    for _ in 0..12 {
        advance_one_frame(&mut vm)?;
    }

    let hovered = render_rgba(&mut vm);
    let delta = pixel_delta(&baseline, &hovered);
    eprintln!("title-hover pixel-delta={delta}");
    assert!(
        delta > 1000,
        "hovering Start did not visibly change the title framebuffer"
    );
    Ok(())
}

#[test]
fn title_click_start_leaves_title_scene() -> Result<()> {
    let mut vm = make_vm("sys10_tt01", 0)?;
    let _ = step_for_frames(&mut vm, 520)?;

    vm.ctx.on_mouse_move(450, 650);
    for _ in 0..6 {
        advance_one_frame(&mut vm)?;
    }
    vm.ctx.on_mouse_down(VmMouseButton::Left);
    advance_one_frame(&mut vm)?;
    vm.ctx.on_mouse_up(VmMouseButton::Left);
    advance_one_frame(&mut vm)?;

    let mut scene_names = Vec::new();
    for _ in 0..360 {
        advance_one_frame(&mut vm)?;
        if let Some(name) = vm.current_scene_name() {
            if scene_names.last().map(String::as_str) != Some(name) {
                scene_names.push(name.to_string());
            }
        }
    }

    eprintln!("title-click-start scene-trace={scene_names:?}");
    assert!(
        scene_names.iter().any(|name| name != "sys10_tt01"),
        "clicking Start never left sys10_tt01"
    );
    Ok(())
}

#[test]
fn title_click_config_opens_config_flow() -> Result<()> {
    let mut vm = make_vm("sys10_tt01", 0)?;
    let _ = step_for_frames(&mut vm, 520)?;

    // Config/menu button area settles near x=652..762, y=619 in testcase title.
    vm.ctx.on_mouse_move(705, 650);
    for _ in 0..6 {
        advance_one_frame(&mut vm)?;
    }
    let baseline_open = vm.ctx.globals.syscom.menu_open;
    vm.ctx.on_mouse_down(VmMouseButton::Left);
    advance_one_frame(&mut vm)?;
    vm.ctx.on_mouse_up(VmMouseButton::Left);

    let mut became_open = baseline_open;
    let mut scene_names = Vec::new();
    for _ in 0..180 {
        advance_one_frame(&mut vm)?;
        became_open |= vm.ctx.globals.syscom.menu_open;
        if let Some(name) = vm.current_scene_name() {
            if scene_names.last().map(String::as_str) != Some(name) {
                scene_names.push(name.to_string());
            }
        }
    }

    eprintln!(
        "title-click-config menu_open={} scene-trace={scene_names:?}",
        became_open
    );
    assert!(
        became_open || scene_names.iter().any(|name| name != "sys10_tt01"),
        "clicking Config neither opened syscom/config flow nor left title scene"
    );
    Ok(())
}
