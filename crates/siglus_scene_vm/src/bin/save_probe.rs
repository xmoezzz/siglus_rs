//! Headless Rewrite+ save-load probe.
//!
//! Boots the VM the way `SiglusHost::init_vm` does (no window, no renderer),
//! loads a captured original save slot, and reports the VM's input-sync state
//! per frame:
//!
//!   * `ctx.input`        - the host mouse state (`host.mouse_move()` writes this)
//!   * `ctx.script_input` - what the script sees as `$ip_mx` / `$ip_my`
//!                          (`forms/mouse.rs` reads `script_input.mouse_x/y`)
//!
//! If `script_input` never follows `input`, every script-level hit test in the
//! Rewrite+ map (`sys40_mp20` compares `$ip_mx` against rectangles) fails, which
//! matches the observed "map is running but nothing responds to taps".
//!
//! Environment:
//!   SAVE_PROBE_PROJECT  project dir (default `G:\siglus\probe_project`)
//!   SAVE_PROBE_SAVE     normal save number to load (default 212)
//!   SAVE_PROBE_BOOT     boot scene (default `_start`)
//!   SAVE_PROBE_FRAMES   frames to run before loading (default 240)
//!   SAVE_PROBE_AFTER    frames to run after loading (default 900)
//!   SAVE_PROBE_X / _Y   cursor injected once the map scene is active (571/625)
//!   SAVE_PROBE_TRACE    print every frame instead of every 30

use anyhow::{Context, Result};
use siglus_assets::scene_pck::{ScenePck, ScenePckDecodeOptions};
use siglus_scene_vm::runtime::forms::syscom;
use siglus_scene_vm::runtime::input::{VmKey, VmMouseButton};
use siglus_scene_vm::runtime::CommandContext;
use siglus_scene_vm::scene_stream::SceneStream;
use siglus_scene_vm::vm::{SceneVm, VmConfig};
use std::path::{Path, PathBuf};

fn env_usize(key: &str, default: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(default)
}

fn env_i32(key: &str, default: i32) -> i32 {
    std::env::var(key)
        .ok()
        .and_then(|v| v.trim().parse::<i32>().ok())
        .unwrap_or(default)
}

fn make_vm(project: &Path, scene_name: &str, z: i32) -> Result<SceneVm<'static>> {
    let pck_path = siglus_scene_vm::resource::find_scene_pck_path(project)?;
    let opt = ScenePckDecodeOptions::from_project_dir(project)?;
    let pack = ScenePck::load_and_rebuild(&pck_path, &opt)?;
    let scn_no = pack
        .find_scene_no(scene_name)
        .with_context(|| format!("scene not found: {scene_name}"))?;
    let chunk = pack.scn_data_slice(scn_no)?;
    let chunk_leaked: &'static [u8] = Box::leak(chunk.to_vec().into_boxed_slice());
    let mut stream = SceneStream::new(chunk_leaked)?;
    stream.jump_to_z_label(z.max(0) as usize)?;

    let mut ctx = CommandContext::new(project.to_path_buf());
    ctx.screen_w = 1280;
    ctx.screen_h = 720;
    let active_append = ctx.globals.append_dir.clone();
    ctx.install_scene_metadata(&active_append, &pack)?;

    let mut vm = SceneVm::with_config(VmConfig::from_env(), stream, ctx);
    vm.cfg.max_steps = 2_000_000;
    syscom::load_global_save(&mut vm.ctx).context("load global save")?;
    vm.restart_scene_name(scene_name, z)?;
    Ok(vm)
}

fn state(vm: &mut SceneVm<'static>) -> String {
    let blocked = vm.is_blocked();
    let poll = vm.ctx.wait.needs_runtime_poll();
    format!(
        "scene={:?} line={} halted={} blocked={} wait=[until={} frame={:?} key={} reveal={} audio={} event={} movie={} modal={} wipe={}] poll={} msg={} chars={}/{}",
        vm.current_scene_name(),
        vm.current_line_no(),
        vm.is_halted(),
        blocked,
        vm.ctx.wait.until.is_some(),
        vm.ctx.wait.until_frame,
        vm.ctx.wait.waiting_for_key,
        vm.ctx.wait.message_reveal,
        vm.ctx.wait.audio.is_some(),
        vm.ctx.wait.event.is_some(),
        vm.ctx.wait.movie.is_some(),
        vm.ctx.wait.system_modal,
        vm.ctx.wait.wipe,
        poll,
        vm.ctx.ui.message_waiting(),
        vm.ctx.ui.message_visible_chars(),
        vm.ctx.ui.message_wait_message_len(),
    )
}

/// `$ip_mx` / `$ip_my` are exactly `script_input.mouse_x` / `.mouse_y`.
fn input_state(vm: &SceneVm<'static>) -> String {
    format!(
        "input=({},{}) $ip=({},{})",
        vm.ctx.input.mouse_x, vm.ctx.input.mouse_y, vm.ctx.script_input.mouse_x,
        vm.ctx.script_input.mouse_y,
    )
}

fn dump_map_objects(vm: &SceneVm<'static>) {
    let form_id = vm.ctx.ids.form_global_stage;
    let Some(st) = vm.ctx.globals.stage_forms.get(&form_id) else {
        println!("  [stage] no stage form {form_id}");
        return;
    };

    let mut stages: Vec<i64> = st.object_lists.keys().copied().collect();
    stages.sort_unstable();
    println!("  [stage] form={form_id} stage_indices={stages:?}");
    for stage_idx in [0i64, 1, 2] {
        match st.object_slot_use.get(&stage_idx) {
            Some(uses) => {
                let false_slots: Vec<usize> = uses
                    .iter()
                    .enumerate()
                    .filter(|(_, u)| !**u)
                    .map(|(i, _)| i)
                    .collect();
                println!(
                    "  [stage] stage={stage_idx} slot_use.len={} false_slots(n={})={:?}",
                    uses.len(),
                    false_slots.len(),
                    &false_slots[..false_slots.len().min(24)]
                );
                println!(
                    "  [stage]   is_used(76)={} is_used(77)={} is_used(78)={} strict={}",
                    st.object_slot_is_used(stage_idx, 76),
                    st.object_slot_is_used(stage_idx, 77),
                    st.object_slot_is_used(stage_idx, 78),
                    st.object_list_strict.get(&stage_idx).copied().unwrap_or(false)
                );
            }
            None => println!("  [stage] stage={stage_idx} no slot_use entry"),
        }
        let emb = st
            .embedded_object_slots_by_stage
            .get(&stage_idx)
            .map(|s| s.len())
            .unwrap_or(0);
        println!(
            "  [stage] stage={stage_idx} embedded_slots={emb} is_embedded(77)={} is_embedded(0)={}",
            st.is_embedded_object_slot(stage_idx, 77),
            st.is_embedded_object_slot(stage_idx, 0)
        );
        if let Some(name) = vm.current_scene_name() {
            let _ = name;
        }
        {
        }
    }
    for stage_idx in stages {
        let objs = &st.object_lists[&stage_idx];
        let with_children = objs
            .iter()
            .filter(|o| !o.runtime.child_objects.is_empty())
            .count();
        let listed = objs
            .iter()
            .filter(|o| o.object_type != 0 || !o.runtime.child_objects.is_empty())
            .count();
        println!(
            "  [stage] stage={stage_idx} slots={} non_empty={listed} with_children={with_children}",
            objs.len()
        );
        // Top-level slots with a file: does the GfxRuntime disp agree?
        if stage_idx == 1 {
            for (ti, tobj) in objs.iter().enumerate() {
                let Some(file) = tobj.file_name.as_deref() else { continue };
                if file.is_empty() {
                    continue;
                }
                let tdisp = tobj
                    .lookup_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_disp)
                    .unwrap_or(-999);
                let tslot = tobj.runtime_slot_or(ti);
                let tgfx = vm.ctx.gfx.object_peek_disp(stage_idx, tslot as i64);
                let tbind = vm.ctx.gfx.object_sprite_binding(stage_idx, tslot as i64);
                let tvis = tbind.and_then(|(lid, sid)| {
                    vm.ctx.layers.layer(lid)?.sprite(sid).map(|s| s.visible)
                });
                if tdisp != 0 || tgfx.unwrap_or(0) != 0 {
                    println!(
                        "  [top] slot={ti} file={file:?} objstate.disp={tdisp} gfx.disp={tgfx:?} sprite.visible={tvis:?}"
                    );
                }
            }
        }
        // `front.object[N]` uses the FRONT sub-stage (index 1) slot numbering.
        for idx in [0usize, 77, 78] {
            let Some(obj) = objs.get(idx) else { continue };
            let kids = obj.runtime.child_objects.len();
            if obj.object_type == 0 && kids == 0 && obj.file_name.is_none() {
                continue;
            }
            let odisp = obj
                .lookup_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_disp)
                .unwrap_or(-999);
            let oslot = obj.runtime_slot_or(idx);
            println!(
                "  [stage]   object[{idx}] type={} file={:?} children={kids} backend={:?} base.disp={} prop.disp={odisp} gfx.disp={:?}",
                obj.object_type,
                obj.file_name.as_deref().unwrap_or(""),
                obj.backend,
                obj.base.disp,
                vm.ctx.gfx.object_peek_disp(stage_idx, oslot as i64)
            );
            for ci in 90..=104usize {
                let Some(child) = obj.runtime.child_objects.get(ci) else {
                    continue;
                };
                let disp = child
                    .lookup_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_disp)
                    .unwrap_or(-999);
                let gan = child
                    .lookup_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_start_gan)
                    .unwrap_or(-999);
                let _ = gan;
                let slot = child.runtime_slot_or(ci);
                let binding = vm.ctx.gfx.object_sprite_binding(stage_idx, slot as i64);
                let gfx_disp = vm.ctx.gfx.object_peek_disp(stage_idx, slot as i64);
                let sprite_state = binding.and_then(|(lid, sid)| {
                    let layer = vm.ctx.layers.layer(lid)?;
                    let s = layer.sprite(sid)?;
                    Some(format!(
                        "visible={} alpha={} tr={} image={:?} pos=({},{})",
                        s.visible, s.alpha, s.tr, s.image_id, s.x, s.y
                    ))
                });
                println!(
                    "  [stage]     child[{ci}] file={:?} prop.disp={} base.disp={} gfx.disp={gfx_disp:?} slot={slot} binding={binding:?}",
                    child.file_name.as_deref().unwrap_or(""),
                    disp,
                    child.base.disp,
                );
                println!("  [stage]       sprite: {sprite_state:?}");
            }
        }
    }

    // Message windows are what the map/menu actually draws through.
    let mwnd_cnt = st.mwnd_lists.values().map(Vec::len).sum::<usize>();
    println!("  [stage] mwnd_lists={mwnd_cnt} total");
}

/// `OBJECT.TR` is co-backed by `ObjectState.base.tr` and the `tr` IntEvent:
/// `set_int_prop(obj_tr, v)` writes BOTH, and `effective_object_info` reads the
/// *event* whenever `ids.obj_tr_eve != 0`. Print both sides so a mismatch
/// between "what the script set" and "what the renderer reads" is visible.
fn tr_state_line(vm: &SceneVm<'static>, stage_idx: i64, idx: usize) -> Option<String> {
    let ids = &vm.ctx.ids;
    let st = vm.ctx.globals.stage_forms.get(&ids.form_global_stage)?;
    let obj = st.object_lists.get(&stage_idx)?.get(idx)?;
    let ev = &obj.runtime.prop_events.tr;
    Some(format!(
        "[tr] stage={stage_idx} object[{idx}] file={:?} base.tr={} explicit={} | ev value={} cur={} start={} end={} cur_time={} end_time={} loop={} speed={} active={} | lookup(obj_tr)={:?}",
        obj.file_name.as_deref().unwrap_or("-"),
        obj.base.tr,
        obj.has_int_prop(ids.obj_tr),
        ev.value,
        ev.cur_value,
        ev.start_value,
        ev.end_value,
        ev.cur_time,
        ev.end_time,
        ev.loop_type,
        ev.speed_type,
        ev.check_event(),
        obj.lookup_int_prop(ids, ids.obj_tr),
    ))
}

fn dump_tr_states(vm: &SceneVm<'static>, slots: &[usize]) {
    println!(
        "  [tr] ids.obj_tr={} ids.obj_tr_eve={} ids.obj_disp={}",
        vm.ctx.ids.obj_tr, vm.ctx.ids.obj_tr_eve, vm.ctx.ids.obj_disp
    );
    for &idx in slots {
        if let Some(line) = tr_state_line(vm, 1, idx) {
            println!("  {line}");
        }
    }
}

fn dump_render(vm: &mut SceneVm<'static>) {
    let list = vm.ctx.render_list_with_effects();
    let with_image = list.iter().filter(|s| s.sprite.image_id.is_some()).count();
    let visible = list
        .iter()
        .filter(|s| s.sprite.image_id.is_some() && s.sprite.visible && s.sprite.alpha > 0)
        .count();
    println!(
        "  [render] sprites={} with_image={with_image} visible={visible}",
        list.len()
    );
    let mut shown = 0usize;
    for s in &list {
        if s.sprite.image_id.is_none() {
            continue;
        }
        let info = s
            .sprite
            .image_id
            .as_ref()
            .and_then(|id| vm.ctx.images.debug_image_info(id));
        let name = info
            .as_ref()
            .and_then(|i| i.source_path.as_ref())
            .and_then(|p| p.file_name())
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "(no path)".to_string());
        println!(
            "  [render] {name:<30} vis={} alpha={:<3} pos=({},{}) img={}x{} z={} order=({},{})",
            s.sprite.visible,
            s.sprite.alpha,
            s.sprite.x,
            s.sprite.y,
            info.as_ref().map(|i| i.width).unwrap_or(0),
            info.as_ref().map(|i| i.height).unwrap_or(0),
            s.sprite.z,
            s.sorter_layer,
            s.sorter_order
        );
        shown += 1;
        if shown >= 60 {
            println!("  [render] ... truncated");
            break;
        }
    }
}
/// One VM frame, mirroring the host's pump order loosely: button actions, the
/// script proc, then the frame tick.
fn step(vm: &mut SceneVm<'static>) -> Result<bool> {
    let _ = vm.process_pending_button_actions();
    let _ = vm.run_script_proc()?;
    vm.tick_frame()?;
    Ok(vm.take_runtime_load_completed())
}

fn main() -> Result<()> {
    // The VM's scene/user-command dispatch is deeply recursive; the Windows
    // default 1 MiB main-thread stack overflows in a debug build (0xC00000FD).
    let handle = std::thread::Builder::new()
        .name("save_probe".to_string())
        .stack_size(512 * 1024 * 1024)
        .spawn(run_probe)?;
    handle
        .join()
        .map_err(|_| anyhow::anyhow!("probe thread panicked"))?
}

fn run_probe() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let project = PathBuf::from(
        std::env::var("SAVE_PROBE_PROJECT").unwrap_or_else(|_| r"G:\siglus\probe_project".into()),
    );
    let save_no = env_usize("SAVE_PROBE_SAVE", 212);
    let boot_scene = std::env::var("SAVE_PROBE_BOOT").unwrap_or_else(|_| "_start".to_string());
    let frames_before = env_usize("SAVE_PROBE_FRAMES", 240);
    let frames_after = env_usize("SAVE_PROBE_AFTER", 900);
    let click_x = env_i32("SAVE_PROBE_X", 571);
    let click_y = env_i32("SAVE_PROBE_Y", 625);
    let trace_every = if std::env::var_os("SAVE_PROBE_TRACE").is_some() {
        1
    } else {
        30
    };

    let save_path = siglus_scene_vm::original_save::save_file_path_for_no(&project, save_no);
    println!("project   = {}", project.display());
    println!("boot      = {boot_scene}");
    println!("save      = {} exists={}", save_path.display(), save_path.exists());
    println!("click     = ({click_x},{click_y})");
    println!("frames    = {frames_before} before / {frames_after} after");
    println!();

    let mut vm = make_vm(&project, &boot_scene, 0)?;
    println!("--- boot ok: {} ---", state(&mut vm));

    // `SAVE_PROBE_SKIP_LOAD` boots a scene and lets it run without restoring a
    // save. Booting `sys40_mp40` this way exercises the map dispatcher's own
    // `#z0` prologue (mp30#z0 reset -> mp40#z20 -> mp50#z0 -> mp30#z2, which
    // sets d[651]=0 and makes sys40_mp20 fade the map container in). That is the
    // control experiment for the "map container tr stays 0" defect: the loaded
    // save resumes `sys40_mp40#z0` *inside* its main loop, so the prologue is
    // skipped and `d[651]` keeps the saved value 1.
    if std::env::var_os("SAVE_PROBE_SKIP_LOAD").is_some() {
        println!("stage right after boot:");
        dump_map_objects(&vm);
        dump_tr_states(&vm, &[0, 77]);
        println!();
        println!("--- fresh-entry run: {frames_after} frames on {boot_scene} ---");
        for frame in 0..frames_after {
            if vm.ctx.wait.waiting_for_key() {
                vm.ctx.on_key_down(VmKey::Enter);
                vm.ctx.on_key_up(VmKey::Enter);
            }
            let _ = step(&mut vm)?;
            if frame % trace_every == 0 {
                println!("f{frame:<5} {} | {}", state(&mut vm), input_state(&mut vm));
                if let Some(line) = tr_state_line(&vm, 1, 77) {
                    println!("        {line}");
                }
            }
        }
        println!();
        println!("--- final (fresh entry) ---");
        println!("{} | {}", state(&mut vm), input_state(&mut vm));
        dump_tr_states(&vm, &[0, 77]);
        dump_render(&mut vm);
        return Ok(());
    }

    println!("stage right after boot (title):");
    dump_map_objects(&vm);

    println!("--- phase 1: {frames_before} frames on the title ---");
    for frame in 0..frames_before {
        let _ = step(&mut vm)?;
        if frame % trace_every == 0 {
            println!("f{frame:<5} {} | {}", state(&mut vm), input_state(&mut vm));
        }
    }

    println!();
    println!("--- render list on the TITLE (control) ---");
    dump_render(&mut vm);
    dump_map_objects(&vm);
    dump_tr_states(&vm, &[0, 77]);
    println!();
    println!("--- phase 2: load save {save_no} ---");
    syscom::menu_load_slot(&mut vm.ctx, false, save_no);
    let mut applied = None;
    for frame in 0..300 {
        let done = step(&mut vm)?;
        if done {
            applied = Some(frame);
            break;
        }
    }
    let Some(f) = applied else {
        println!("!! load request never applied: {}", state(&mut vm));
        return Ok(());
    };
    println!("load applied at +{f}: {}", state(&mut vm));
    println!("stage right after load:");
    dump_map_objects(&vm);
    dump_tr_states(&vm, &[0, 77]);
    let watch = std::env::var("SAVE_PROBE_TR_WATCH")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok());

    println!();
    println!("--- phase 3: {frames_after} frames after load ---");
    let mut last_scene = String::new();
    let mut injected = false;
    let mut advances = 0usize;

    // `SAVE_PROBE_SWEEP="x0,y0,x1,y1,step"`: instead of injecting one fixed
    // cursor position, walk the cursor over the whole map body and press+release
    // at every grid point. Which particular location marker triggers the
    // `int stack underflow` is not known, and the map script only evaluates its
    // 30 location slots while the cursor is inside the map body
    // (`sys40_mp20` original line ~1250 / file line 3056), so a single point is
    // not enough to search the space.
    let sweep: Option<(i32, i32, i32, i32, i32)> = std::env::var("SAVE_PROBE_SWEEP")
        .ok()
        .and_then(|raw| {
            let v: Vec<i32> = raw
                .split(',')
                .filter_map(|t| t.trim().parse().ok())
                .collect();
            if v.len() == 5 && v[4] > 0 {
                Some((v[0], v[1], v[2], v[3], v[4]))
            } else {
                None
            }
        });
    let mut sweep_k = 0usize;
    let mut sweep_notified = false;
    for frame in 0..frames_after {
        // Play through the restored story the way the user does: tap/Enter to
        // clear MESSAGE_KEY_WAIT. Without this the VM blocks on the restored
        // message forever and never reaches the map.
        if vm.ctx.wait.waiting_for_key() {
            advances += 1;
            vm.ctx.on_key_down(VmKey::Enter);
            vm.ctx.on_key_up(VmKey::Enter);
        }
        let _ = step(&mut vm)?;

        let scene = vm.current_scene_name().unwrap_or("").to_string();
        if scene != last_scene {
            println!("f{frame:<5} SCENE -> {scene} | {} | {}", state(&mut vm), input_state(&mut vm));
            last_scene = scene.clone();
        } else if frame % trace_every == 0 {
            println!("f{frame:<5} {} | {}", state(&mut vm), input_state(&mut vm));
            if let Some(slot) = watch {
                if let Some(line) = tr_state_line(&vm, 1, slot) {
                    println!("        {line}");
                }
            }
        }

        if !injected && sweep.is_none() && scene == "sys40_mp20" && frame > 60 {
            injected = true;
            println!();
            println!("=== cursor injection at f{frame} in {scene} ===");

            // `SAVE_PROBE_TR_SET`: a save written while the map container was
            // mid-fade records `tr_eve = {loop_type: -1, value: 0}`, which makes
            // the whole 141-child map subtree invisible. Force the container's
            // `tr` back up *before* clicking so the click can actually reach the
            // location markers; otherwise hit-testing runs against a container
            // that reports itself invisible.
            if let Ok(tr) = std::env::var("SAVE_PROBE_TR_SET") {
                if let Ok(tr) = tr.trim().parse::<i64>() {
                    let form_id = vm.ctx.ids.form_global_stage;
                    let slot = env_usize("SAVE_PROBE_TR_OBJ", 77);
                    if let Some(st) = vm.ctx.globals.stage_forms.get_mut(&form_id) {
                        if let Some(objs) = st.object_lists.get_mut(&1) {
                            if let Some(container) = objs.get_mut(slot) {
                                let before =
                                    container.lookup_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_tr);
                                container.set_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_tr, tr);
                                println!("  [proof] object[{slot}].tr {before:?} -> {tr}");
                            }
                        }
                    }
                    let _ = step(&mut vm)?;
                    dump_render(&mut vm);
                }
            }

            println!("before      : {}", input_state(&mut vm));
            vm.ctx.on_mouse_move(click_x, click_y);
            println!("after move  : {}", input_state(&mut vm));
            let _ = step(&mut vm)?;
            println!("after 1 frame: {}", input_state(&mut vm));
            vm.ctx.on_mouse_down(VmMouseButton::Left);
            let _ = step(&mut vm)?;
            println!("after click : {}", input_state(&mut vm));
            dump_map_objects(&vm);
            dump_tr_states(&vm, &[0, 77]);
            dump_render(&mut vm);

            println!("=== end injection ===");
            println!();
        }

        if let Some((x0, y0, x1, y1, st)) = sweep {
            if scene == "sys40_mp20" && frame > 60 {
                let cols = ((x1 - x0) / st).max(0) + 1;
                let rows = ((y1 - y0) / st).max(0) + 1;
                let total = (cols * rows) as usize;
                if sweep_k >= total {
                    if !sweep_notified {
                        sweep_notified = true;
                        println!("=== sweep finished: {total} points, no vm error ===");
                    }
                } else {
                    let x = x0 + (sweep_k as i32 % cols) * st;
                    let y = y0 + (sweep_k as i32 / cols) * st;
                    sweep_k += 1;
                    vm.ctx.on_mouse_move(x, y);
                    let _ = step(&mut vm)?;
                    vm.ctx.on_mouse_down(VmMouseButton::Left);
                    let _ = step(&mut vm)?;
                    vm.ctx.on_mouse_up(VmMouseButton::Left);
                    let _ = step(&mut vm)?;
                    if sweep_k % 20 == 1 {
                        println!("sweep {sweep_k}/{total} at ({x},{y}) | {}", state(&mut vm));
                    }
                }
            }
        }
    }

    println!();
    println!("--- final (advances={advances}) ---");
    println!("{} | {}", state(&mut vm), input_state(&mut vm));
    Ok(())
}
