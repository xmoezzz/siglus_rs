use anyhow::{Context, Result};
use siglus_assets::scene_pck::{find_scene_pck_in_project, ScenePck, ScenePckDecodeOptions};
use siglus_scene_vm::runtime::CommandContext;
use siglus_scene_vm::scene_stream::SceneStream;
use siglus_scene_vm::vm::{SceneVm, VmConfig};
use std::path::PathBuf;

fn make_vm(scene_name: &str, z: i32) -> Result<SceneVm<'static>> {
    let project = PathBuf::from("/Users/xmoe/Documents/siglus_rs-main/testcase");
    let pck_path = find_scene_pck_in_project(&project)?;
    let opt = ScenePckDecodeOptions::from_project_dir(&project)?;
    let pack = ScenePck::load_and_rebuild(&pck_path, &opt)?;
    let scn_no = pack.find_scene_no(scene_name).with_context(|| format!("scene not found: {scene_name}"))?;
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
    for frame in 0..700u32 {
        for _ in 0..2000 {
            let running = vm.step()?;
            if !running || vm.is_halted() || vm.is_blocked() { break; }
        }
        if vm.is_blocked() { vm.ctx.wait.notify_key(); }
        vm.tick_frame()?;
        if frame % 25 == 0 || frame >= 395 && frame <= 430 {
            let mut visible = Vec::new();
            let mut movie_state = None;
            if let Some(st) = vm.ctx.globals.stage_forms.get(&vm.ctx.ids.form_global_stage) {
                if let Some(objs) = st.object_lists.get(&0) {
                    if let Some(root) = objs.get(0) {
                        for (cidx, child) in root.runtime.child_objects.iter().enumerate() {
                            if child.used {
                                let disp = child.lookup_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_disp).unwrap_or(0);
                                if cidx == 13 {
                                    let tr = child.lookup_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_tr).unwrap_or(255);
                                    let alpha = child.lookup_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_alpha).unwrap_or(255);
                                    movie_state = Some((child.movie.timer_ms, child.movie.total_ms, child.movie.playing, child.movie.pause_flag, child.movie.last_frame_idx));
                                    eprintln!("movie frame={} disp={} tr={} alpha={} playing={} pause={} timer={} last={:?}", frame, disp, tr, alpha, child.movie.playing, child.movie.pause_flag, child.movie.timer_ms, child.movie.last_frame_idx);
                                }
                                if cidx == 38 {
                                    let tr = child.lookup_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_tr).unwrap_or(255);
                                    let alpha = child.lookup_int_prop(&vm.ctx.ids, vm.ctx.ids.obj_alpha).unwrap_or(255);
                                    let pev = &child.runtime.prop_events.tr;
                                    let tr_rep: Vec<_> = child.runtime.prop_event_lists.tr_rep.iter().map(|ev| (ev.get_total_value(), ev.cur_value, ev.start_value, ev.end_value, ev.cur_time, ev.end_time)).collect();
                                    eprintln!("kuro frame={} disp={} tr={} alpha={} ev_active={} ev_cur={} ev_start={} ev_end={} ev_time={}/{} delay={} loop={} speed={} real={} tr_rep={:?}", frame, disp, tr, alpha, pev.check_event(), pev.cur_value, pev.start_value, pev.end_value, pev.cur_time, pev.end_time, pev.delay_time, pev.loop_type, pev.speed_type, pev.real_flag, tr_rep);
                                }
                                if disp != 0 {
                                    visible.push((cidx, child.file_name.clone().unwrap_or_default(), child.object_type));
                                }
                            }
                        }
                    }
                }
            }
            if frame >= 405 && frame <= 408 {
                let mut fa = Vec::new();
                let gfa: Vec<_> = vm.ctx.globals.frame_actions.iter().map(|(fid, fa)| (*fid, fa.cmd_name.clone(), fa.counter.get_count(), fa.end_time)).collect();
                let gfal: Vec<_> = vm.ctx.globals.frame_action_lists.iter().flat_map(|(fid, list)| list.iter().enumerate().filter(|(_,fa)| !fa.cmd_name.is_empty()).map(move |(i,fa)| (*fid, i, fa.cmd_name.clone(), fa.counter.get_count(), fa.end_time))).collect();
                if let Some(st) = vm.ctx.globals.stage_forms.get(&vm.ctx.ids.form_global_stage) {
                    if let Some(objs) = st.object_lists.get(&0) {
                        if let Some(root) = objs.get(0) {
                            for (cidx, child) in root.runtime.child_objects.iter().enumerate() {
                                if child.used && !child.frame_action.cmd_name.is_empty() {
                                    fa.push((cidx, child.file_name.clone().unwrap_or_default(), child.frame_action.cmd_name.clone(), child.frame_action.counter.get_count(), child.frame_action.end_time));
                                }
                            }
                        }
                    }
                }
                eprintln!("global_frame_actions={:?} list={:?} object_frame_actions={:?}", gfa, gfal, fa);
            }
            let img = vm.ctx.capture_frame_rgba();
            let nonblack = img.rgba.chunks_exact(4).filter(|px| px[0]!=0 || px[1]!=0 || px[2]!=0).count();
            if frame >= 405 && frame <= 408 {
                let rl = vm.ctx.render_list_with_effects();
                eprintln!("render frame={} count={}", frame, rl.len());
                for (i, rs) in rl.iter().enumerate().take(12) {
                    let src = rs.sprite.image_id.and_then(|id| vm.ctx.images.debug_image_info(id).and_then(|d| d.source_path.map(|p| p.display().to_string())));
                    eprintln!("  render[{i}] img={:?} src={:?} pos=({}, {}) order={} alpha={} tr={} color_rate={} blend={:?}", rs.sprite.image_id, src, rs.sprite.x, rs.sprite.y, rs.sprite.order, rs.sprite.alpha, rs.sprite.tr, rs.sprite.color_rate, rs.sprite.blend);
                    if frame == 405 {
                        if let Some(id) = rs.sprite.image_id {
                            if let Some((img, _)) = vm.ctx.images.get_entry(id) {
                                let path = PathBuf::from("/Users/xmoe/Documents/siglus_rs-main/docs").join(format!("probe_img_{i}.png"));
                                let _ = image::save_buffer(&path, &img.rgba, img.width, img.height, image::ColorType::Rgba8);
                            }
                        }
                    }
                }
            }
            println!("frame={frame} blocked={} wait={:?} visible={:?} movie13={:?} nonblack={}", vm.is_blocked(), vm.ctx.wait, visible, movie_state, nonblack);
        }
    }
    Ok(())
}
