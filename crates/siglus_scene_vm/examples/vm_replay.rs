//! Deterministic VM replay for checking that interpreter changes keep
//! behaviour: runs a scene on a virtual clock (16 ms a frame) with the same
//! clicks every time and prints, per frame, a hash of the script position and
//! of the complete render frame (every sprite's state). Two builds that
//! print the same lines behave the same over that run.
//!
//! ```text
//! cargo run --release --features virtual-clock --example vm_replay -- \
//!     <project dir> <scene> [frames] > trace.txt
//! ```
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::Duration;

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
            .context("usage: vm_replay <project> <scene> [frames]")?,
    );
    let scene = args.next().context("missing scene")?;
    let frames: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(1200);

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

    let mut all = std::collections::hash_map::DefaultHasher::new();
    // `VM_REPLAY_TIME_ONLY`: skip the trace and report the VM's time over the
    // (deterministic) run, a steadier benchmark than the real-clock one.
    let time_only = std::env::var_os("VM_REPLAY_TIME_ONLY").is_some();
    // `VM_REPLAY_CLICK_EVERY=<frames>`: the click interval (default 20);
    // longer ones stay in each scene longer, as a reader would.
    let click_every: u32 = std::env::var("VM_REPLAY_CLICK_EVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .filter(|&n| n >= 3)
        .unwrap_or(20);
    let mut vm_time = std::time::Duration::ZERO;
    let new_images = std::env::var_os("VM_REPLAY_NEW_IMAGES").is_some();
    let slow_us: Option<u64> = std::env::var("VM_REPLAY_SLOW_US")
        .ok()
        .and_then(|v| v.parse().ok());
    let mut content_hashes = std::collections::HashMap::<(u32, u64), u64>::new();
    let dump_frame = std::env::var("VM_REPLAY_DUMP")
        .ok()
        .and_then(|v| v.parse::<u32>().ok());
    for frame in 0..frames {
        siglus_scene_vm::platform_time::advance_virtual_clock(Duration::from_millis(16));
        let start = std::time::Instant::now();
        let script = vm.run_script_proc().map_err(|e| format!("{e:#}"));
        let tick = vm.tick_frame().map_err(|e| format!("{e:#}"));
        let waited = std::time::Duration::from_nanos(
            siglus_scene_vm::movie::OMV_SETTLE_NS.swap(0, std::sync::atomic::Ordering::Relaxed),
        );
        let spent = start.elapsed().saturating_sub(waited);
        vm_time += spent;
        // `VM_REPLAY_SLOW_US=<us>`: report the frames that took longer.
        if let Some(limit) = slow_us
            && spent.as_micros() as u64 >= limit
        {
            eprintln!(
                "slow frame {frame}: {} us at {:?}:{}",
                spent.as_micros(),
                vm.current_scene_name(),
                vm.current_line_no()
            );
        }
        match frame % click_every {
            0 => {
                vm.ctx.on_mouse_move(960, 540);
                vm.ctx.on_mouse_down(VmMouseButton::Left);
            }
            2 => vm.ctx.on_mouse_up(VmMouseButton::Left),
            _ => {}
        }
        if time_only {
            continue;
        }
        let render_frame = vm.ctx.render_frame_with_effects();
        // Image ids number loads, so they differ when caching changes; the
        // trace names images by their size and pixels instead.
        let mut render = format!("{render_frame:?}");
        let mut handles = Vec::new();
        let sprites = render_frame.wipe.as_ref().map_or_else(
            || render_frame.sprites.iter().collect::<Vec<_>>(),
            |wipe| {
                wipe.under
                    .iter()
                    .chain(&wipe.current)
                    .chain(&wipe.next)
                    .chain(&wipe.over)
                    .collect()
            },
        );
        for item in sprites {
            let sprite = &item.sprite;
            // `VM_REPLAY_NEW_IMAGES` also names the sprites the Vita GPU
            // path cannot draw (it then composites the frame on the CPU).
            if new_images && sprite.visible {
                let reason = if sprite.mesh_kind != 0 {
                    "mesh"
                } else if sprite.emote_render.is_some() {
                    "emote"
                } else if sprite.mask_image_id.is_some() {
                    "mask"
                } else if sprite.tonecurve_image_id.is_some() {
                    "tonecurve"
                } else if sprite.wipe_src_image_id.is_some() {
                    "wipe-src"
                } else if matches!(sprite.blend, siglus_scene_vm::layer::SpriteBlend::Overlay) {
                    "overlay"
                } else if sprite.wipe_fx_mode != 0 || sprite.mask_mode != 0 {
                    "effect"
                } else {
                    ""
                };
                if !reason.is_empty() {
                    eprintln!(
                        "cpu reason frame {frame} {reason} {:?}:{} {:?}",
                        vm.current_scene_name(),
                        vm.current_line_no(),
                        sprite.image_id.as_ref().and_then(|h| vm.ctx.images.debug_image_info(h)).and_then(|i| i.source_path)
                    );
                }
            }
            for handle in [
                &sprite.image_id,
                &sprite.mask_image_id,
                &sprite.tonecurve_image_id,
                &sprite.wipe_src_image_id,
                &sprite.fog_texture_image_id,
            ]
            .into_iter()
            .flatten()
            {
                handles.push(handle.clone());
            }
        }
        for handle in handles {
            let key = handle.key().0;
            let version = vm
                .ctx
                .images
                .debug_image_info(&handle)
                .map_or(0, |info| info.version);
            // `VM_REPLAY_NEW_IMAGES`: report each new image version drawn.
            if new_images && !content_hashes.contains_key(&(key, version)) {
                let size = vm.ctx.images.get(&handle).map(|i| (i.width, i.height));
                eprintln!(
                    "new image frame {frame} key {key} v{version} {size:?} {:?}:{} {:?}",
                    vm.current_scene_name(),
                    vm.current_line_no(),
                    vm.ctx.images.debug_image_info(&handle)
                );
            }
            let content = *content_hashes.entry((key, version)).or_insert_with(|| {
                let mut hasher = std::collections::hash_map::DefaultHasher::new();
                if let Some(image) = vm.ctx.images.get(&handle) {
                    (image.width, image.height, &image.rgba).hash(&mut hasher);
                }
                hasher.finish()
            });
            if dump_frame == Some(frame)
                && let Ok(dir) = std::env::var("VM_REPLAY_DUMP_DIR")
                && let Some(image) = vm.ctx.images.get(&handle)
            {
                let _ = image::save_buffer(
                    format!("{dir}/{content:016x}.png"),
                    &image.rgba,
                    image.width,
                    image.height,
                    image::ColorType::Rgba8,
                );
            }
            if dump_frame == Some(frame) {
                eprintln!(
                    "IMAGE {content:016x} {:?}",
                    vm.ctx.images.debug_image_info(&handle)
                );
            }
            render = render.replace(
                &format!("ImageKey({key}))"),
                &format!("Img({content:016x}))"),
            );
        }
        // Layer and sprite ids are backend handles numbered by creation
        // order (which hash-map iteration can change); positions in the
        // sprite list carry the drawing order.
        let render = strip_handle_ids(&render);
        // `VM_REPLAY_DUMP=<frame>`: that frame's render state on stderr.
        if std::env::var("VM_REPLAY_DUMP")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            == Some(frame)
        {
            eprintln!("{render:#?}");
        }
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        render.hash(&mut hasher);
        let render_hash = hasher.finish();
        render_hash.hash(&mut all);
        let wait_hash = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            format!("{:?}", vm.ctx.wait).hash(&mut hasher);
            hasher.finish() & 0xffff
        };
        println!(
            "{frame:5} {:?}:{} {:016x} w{wait_hash:04x} {:?} {:?}",
            vm.current_scene_name(),
            vm.current_line_no(),
            render_hash,
            script.err(),
            tick.err()
        );
    }
    if time_only {
        println!(
            "vm {:.0} us/frame",
            vm_time.as_micros() as f64 / f64::from(frames.max(1))
        );
        return Ok(());
    }
    println!("all {:016x}", all.finish());
    Ok(())
}

fn strip_handle_ids(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("_id: Some(") {
        let field_start = rest[..at].rfind(|c: char| !(c.is_ascii_alphanumeric() || c == '_'));
        let field = &rest[field_start.map_or(0, |i| i + 1)..at];
        let value_start = at + "_id: Some(".len();
        let digits = rest[value_start..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        out.push_str(&rest[..value_start]);
        if (field == "layer" || field == "sprite") && digits > 0 {
            out.push('#');
            rest = &rest[value_start + digits..];
        } else {
            rest = &rest[value_start..];
        }
    }
    out.push_str(rest);
    out
}
