use std::path::PathBuf;

use anyhow::{anyhow, Result};

fn main() -> Result<()> {
    let arg = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("usage: omv_decode <path/to/file.omv>"))?;

    let abs = if arg.is_absolute() {
        arg
    } else {
        std::env::current_dir()?.join(arg)
    };

    let project_dir = abs
        .parent()
        .and_then(|p| {
            if p.file_name().map(|n| n == "mov").unwrap_or(false) {
                p.parent()
            } else {
                Some(p)
            }
        })
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| std::env::current_dir().unwrap());

    let mut movie = siglus_scene_vm::movie::MovieManager::new(project_dir);
    let (asset, _new) = movie.ensure_asset(abs.to_string_lossy().as_ref())?;
    println!(
        "movie: {:?}, {}x{}, fps={:?}, frames={:?}",
        asset.info.path,
        asset.info.width.unwrap_or(0),
        asset.info.height.unwrap_or(0),
        asset.info.fps,
        asset.info.decoded_frames
    );
    if let Some(first) = asset.frames.first() {
        let sample0 = first.rgba.get(0).copied().unwrap_or(0);
        println!("first frame: {}x{}, sample0={}", first.width, first.height, sample0);
    } else {
        println!("no frames decoded");
    }
    Ok(())
}
