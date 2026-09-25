//! Measure model updates using a game's actual CREATE_EMOTE sources.
//! Usage: emote_bench <project> <body.psb> <head.psb> <timeline.psb>
use anyhow::{Context, Result};
use siglus_scene_vm::emote::SiglusEmoteRuntime;
use std::{
    hash::{Hash, Hasher},
    path::PathBuf,
    time::{Duration, Instant},
};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let project = PathBuf::from(args.next().context("missing project directory")?);
    let sources = args
        .map(|name| std::fs::read(project.join("dat").join(name)))
        .collect::<std::io::Result<Vec<_>>>()?;
    let key = siglus_assets::key_toml::load_emote_key_from_project_dir(&project)?;
    let start = Instant::now();
    let mut runtime = SiglusEmoteRuntime::from_psb_sources(
        &sources.iter().map(Vec::as_slice).collect::<Vec<_>>(),
        key,
    )?;
    println!("load: {:?}", start.elapsed());
    if let Ok(timeline) = std::env::var("EMOTE_BENCH_TIMELINE") {
        runtime.play_timeline(&timeline, 0)?;
    }
    // One timeline per line: flags,name. This can reproduce a script's
    // ordinary pose plus additive/loop timelines in the same player.
    if let Ok(timelines) = std::env::var("EMOTE_BENCH_TIMELINES") {
        for entry in timelines.lines().filter(|line| !line.is_empty()) {
            let (flags, name) = entry.split_once(',').context("expected flags,timeline")?;
            runtime.play_timeline(name, flags.parse()?)?;
        }
    }
    let frames = std::env::var("EMOTE_BENCH_FRAMES")
        .ok()
        .map(|value| value.parse::<usize>())
        .transpose()?
        .unwrap_or(120)
        .max(1);
    let mut frame_times = Vec::with_capacity(frames);
    let trace = std::env::var_os("EMOTE_BENCH_TRACE").is_some();
    let mut progress = Duration::ZERO;
    let mut mouth = Duration::ZERO;
    let mut packet = Duration::ZERO;
    for frame in 0..frames {
        let frame_start = Instant::now();
        let start = Instant::now();
        runtime.progress_ms(16)?;
        progress += start.elapsed();
        let start = Instant::now();
        runtime.set_face_talk(if frame < 60 {
            0.0
        } else {
            (frame % 10) as f32 / 10.0
        })?;
        mouth += start.elapsed();
        let start = Instant::now();
        let render = runtime.packet(2048, 2048, 0, 0, false);
        packet += start.elapsed();
        frame_times.push(frame_start.elapsed().as_secs_f64() * 1000.0);
        if frame + 1 == frames {
            println!(
                "sprites: {}, layers: {}",
                render.scene.sprites.len(),
                render.scene.layer_states.len()
            );
        }
        if trace {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            format!("{:?}", render.scene).hash(&mut hash);
            println!("frame {frame}: {:016x}", hash.finish());
        }
        std::hint::black_box(render);
    }
    println!(
        "ms/frame: progress={:.3} mouth={:.3} packet={:.3}",
        progress.as_secs_f64() * 1000.0 / frames as f64,
        mouth.as_secs_f64() * 1000.0 / frames as f64,
        packet.as_secs_f64() * 1000.0 / frames as f64
    );
    frame_times.sort_by(f64::total_cmp);
    println!(
        "frame ms: p50={:.3} p95={:.3} p99={:.3} max={:.3}",
        frame_times[frames / 2],
        frame_times[(frames - 1) * 95 / 100],
        frame_times[(frames - 1) * 99 / 100],
        frame_times[frames - 1]
    );
    Ok(())
}
