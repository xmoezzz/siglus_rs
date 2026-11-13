//! Runs a scene headlessly (clicking through text) and reports heap use:
//! live and peak bytes from a counting allocator, plus the engine's own
//! scene-pack and decoded-image figures. Used to size memory budgets for
//! consoles (PS Vita) without an emulator.
//!
//! ```text
//! memory_probe <project dir> <scene> [frames] [z label]
//! ```
use std::alloc::{GlobalAlloc, Layout, System};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{Context, Result};
use siglus_assets::scene_pck::ScenePck;
use siglus_scene_vm::runtime::CommandContext;
use siglus_scene_vm::runtime::input::VmMouseButton;
use siglus_scene_vm::scene_stream::SceneStream;
use siglus_scene_vm::vm::{SceneVm, VmConfig};

struct Counting;

thread_local! {
    static IN_TRACE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `MEMORY_PROBE_BIG=bytes`: print where allocations at least that big
/// come from.
static BIG_THRESHOLD: AtomicUsize = AtomicUsize::new(usize::MAX);

fn trace_big(size: usize) {
    if size < BIG_THRESHOLD.load(Ordering::Relaxed) || IN_TRACE.with(|t| t.replace(true)) {
        return;
    }
    let trace = std::backtrace::Backtrace::force_capture().to_string();
    let frames: Vec<&str> = trace
        .lines()
        .filter(|l| {
            l.trim_start().split_once(": ").is_some()
                && !l.contains("memory_probe")
                && !l.contains("std::")
                && !l.contains("alloc::")
                && !l.contains("core::")
                && !l.contains("__rust")
                && !l.contains("alloc")
                && !l.contains("grow")
                && !l.contains("raw_vec")
                && !l.contains("reserve")
                && !l.contains("from_elem")
                && !l.contains("with_capacity")
        })
        .take(4)
        .collect();
    eprintln!(
        "BIG {:.1} MiB:\n{}",
        size as f64 / (1024.0 * 1024.0),
        frames.join("\n")
    );
    IN_TRACE.with(|t| t.set(false));
}

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        trace_big(layout.size());
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(live, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, new_size) };
        trace_big(new_size);
        if !new.is_null() {
            if new_size >= layout.size() {
                let live = LIVE.fetch_add(new_size - layout.size(), Ordering::Relaxed) + new_size
                    - layout.size();
                PEAK.fetch_max(live, Ordering::Relaxed);
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        new
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

const MIB: f64 = 1024.0 * 1024.0;

fn main() -> Result<()> {
    if let Some(threshold) = std::env::var("MEMORY_PROBE_BIG")
        .ok()
        .and_then(|v| v.parse().ok())
    {
        BIG_THRESHOLD.store(threshold, Ordering::Relaxed);
    }
    let mut args = std::env::args().skip(1);
    let project = PathBuf::from(
        args.next()
            .context("usage: memory_probe <project> <scene> [frames] [z]")?,
    );
    let scene = args.next().context("missing scene")?;
    let frames: u32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(3000);
    let z: i32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(0);

    let pck_path = siglus_scene_vm::resource::find_scene_pck_path(&project)?;
    let options = siglus_scene_vm::resource::load_scene_pck_decode_options(&project)?;
    let pack = ScenePck::load_lazy(&pck_path, &options)?;
    let scn_no = pack
        .find_scene_no(&scene)
        .with_context(|| format!("scene not found: {scene}"))?;
    let (owner, range) = pack.scn_data_shared(scn_no)?;
    let mut stream =
        SceneStream::new_shared_range_with_string_codec(owner, range, pack.string_codec)?;
    stream.jump_to_z_label(z as usize)?;
    // As the host does: one pack shared by the first scene and the VM.
    let mut ctx = CommandContext::new(project);
    let append = ctx.globals.append_dir.clone();
    ctx.install_scene_metadata(&append, &pack)?;
    ctx.screen_w = 1280;
    ctx.screen_h = 720;
    let mut vm = SceneVm::with_config(VmConfig::from_env(), stream, ctx);
    vm.install_initial_scene_pck(pack, append);
    vm.cfg.max_steps = 1_000_000;
    vm.restart_scene_name(&scene, z)?;
    println!(
        "after load: live {:.1} MiB",
        LIVE.load(Ordering::Relaxed) as f64 / MIB
    );

    let mut window_peak = 0usize;
    for frame in 0..frames {
        // Waits run on the real clock.
        std::thread::sleep(std::time::Duration::from_millis(16));
        let _ = vm.run_script_proc();
        let _ = vm.tick_frame();
        vm.ctx.on_mouse_move(640, 360);
        match frame % 20 {
            0 => vm.ctx.on_mouse_down(VmMouseButton::Left),
            2 => vm.ctx.on_mouse_up(VmMouseButton::Left),
            _ => {}
        }
        window_peak = window_peak.max(LIVE.load(Ordering::Relaxed));
        if frame % 300 == 299 {
            let images = vm.ctx.images.debug_live_image_memory_stats();
            let scenes = vm.debug_scene_memory_stats();
            println!(
                "frame {:5}: live {:6.1} MiB, window peak {:6.1} MiB, overall peak {:6.1} MiB | pck {:.1} MiB, streams {}, images {} ({} albums) {:.1} MiB | {:?}",
                frame + 1,
                LIVE.load(Ordering::Relaxed) as f64 / MIB,
                window_peak as f64 / MIB,
                PEAK.load(Ordering::Relaxed) as f64 / MIB,
                scenes.scene_pck_bytes as f64 / MIB,
                scenes.cached_scene_streams,
                images.images,
                images.albums,
                images.rgba_bytes as f64 / MIB,
                vm.current_scene_name(),
            );
            if std::env::var_os("MEMORY_PROBE_WAIT").is_some() {
                println!("  wait: {:?}", vm.ctx.wait);
            }
            window_peak = 0;
        }
    }
    Ok(())
}
