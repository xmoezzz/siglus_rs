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
    let trace = std::env::var_os("EMOTE_BENCH_TRACE").is_some();
    let mut progress = Duration::ZERO;
    let mut mouth = Duration::ZERO;
    let mut packet = Duration::ZERO;
    for frame in 0..120 {
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
        if frame == 119 {
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
        progress.as_secs_f64() * 1000.0 / 120.0,
        mouth.as_secs_f64() * 1000.0 / 120.0,
        packet.as_secs_f64() * 1000.0 / 120.0
    );
    Ok(())
}
