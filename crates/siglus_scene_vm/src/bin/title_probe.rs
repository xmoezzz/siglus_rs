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
    if let Ok(range) = std::env::var("TITLE_PROBE_DUMP_SCN") {
        let (start, end) = parse_hex_range(&range).unwrap_or((0, stream.scn.len().min(0x100)));
        for pc in (start..end.min(stream.scn.len())).step_by(16) {
            let mut line = format!("{pc:08x}:");
            for b in &stream.scn[pc..(pc + 16).min(end).min(stream.scn.len())] {
                line.push_str(&format!(" {b:02x}"));
            }
            eprintln!("{line}");
        }
    }
    if std::env::var_os("TITLE_PROBE_DUMP_Z").is_some() {
        eprintln!(
            "scene={scene_name} z_cnt={} scn_ofs={}",
            stream.header.z_label_cnt, stream.header.scn_ofs
        );
        for z in 0..stream.header.z_label_cnt.max(0) as usize {
            let off = z * 4;
            let z_off =
                i32::from_le_bytes(stream.z_label_list[off..off + 4].try_into().unwrap());
            eprintln!("  z[{z}]={z_off}");
        }
    }
    stream.jump_to_z_label(z as usize)?;
    let mut ctx = CommandContext::new(project);
    ctx.screen_w = 1280;
    ctx.screen_h = 720;
    let mut vm = SceneVm::with_config(VmConfig::from_env(), stream, ctx);
    vm.cfg.max_steps = 1_000_000;
    vm.restart_scene_name(scene_name, z)?;
    Ok(vm)
}

fn parse_hex_range(s: &str) -> Option<(usize, usize)> {
    let (a, b) = s.split_once("..")?;
    let parse = |v: &str| usize::from_str_radix(v.trim_start_matches("0x"), 16).ok();
    Some((parse(a)?, parse(b)?))
}

fn main() -> Result<()> {
    let scene = std::env::var("TITLE_PROBE_SCENE").unwrap_or_else(|_| "sys10_tt01".to_string());
    let scene_z = std::env::var("TITLE_PROBE_Z")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(0);
    let mut vm = make_vm(&scene, scene_z)?;
    let mut scene_names = Vec::new();
    let click_x = std::env::var("TITLE_PROBE_X")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(450);
    let click_y = std::env::var("TITLE_PROBE_Y")
        .ok()
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(650);
    let max_frames = std::env::var("TITLE_PROBE_FRAMES")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(900);

    for frame in 0..max_frames {
        match frame {
            520 => vm.ctx.on_mouse_move(click_x, click_y),
            522 => vm.ctx.on_mouse_down(VmMouseButton::Left),
            524 => vm.ctx.on_mouse_up(VmMouseButton::Left),
            _ => {}
        }

        let _running = vm.run_script_proc()?;
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
                "frame={frame} scene={:?} line={} blocked={} input=({}, {}) script_input=({}, {})",
                scene,
                line,
                blocked,
                vm.ctx.input.mouse_x,
                vm.ctx.input.mouse_y,
                vm.ctx.script_input.mouse_x,
                vm.ctx.script_input.mouse_y
            );
            if frame == 520 {
                dump_title_menu_rects(&vm);
            }
        }
    }

    println!("scene_trace={scene_names:?}");
    Ok(())
}

fn dump_title_menu_rects(vm: &SceneVm<'static>) {
    let Some(st) = vm
        .ctx
        .globals
        .stage_forms
        .get(&vm.ctx.ids.form_global_stage)
    else {
        return;
    };
    let Some(root) = st.object_lists.get(&0).and_then(|objs| objs.get(0)) else {
        return;
    };
    for idx in 29..=36 {
        let Some(obj) = root.runtime.child_objects.get(idx) else {
            continue;
        };
        if !obj.used {
            continue;
        }
        let x = obj.lookup_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_x).unwrap_or(0);
        let y = obj.lookup_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_y).unwrap_or(0);
        let gfx_pos = vm.ctx.gfx.object_peek_pos(0, idx as i64);
        let disp = obj
            .lookup_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_disp)
            .unwrap_or(0);
        println!(
            "menu[{idx}] file={} disp={} prop_pos=({}, {}) gfx_pos={:?}",
            obj.file_name.as_deref().unwrap_or(""),
            disp,
            x,
            y,
            gfx_pos
        );
    }
}
