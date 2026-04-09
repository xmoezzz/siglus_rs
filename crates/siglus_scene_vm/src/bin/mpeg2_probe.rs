use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "mpeg2_probe")]
#[command(about = "Probe MPEG-1/2 sequence header (no decode).")]
struct Cli {
    input: PathBuf,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let bytes =
        std::fs::read(&cli.input).with_context(|| format!("read {}", cli.input.display()))?;
    let Some(h) = siglus_assets::mpeg2::find_sequence_header(&bytes) else {
        anyhow::bail!("no MPEG sequence header found: {}", cli.input.display());
    };

    let fps = siglus_assets::mpeg2::fps_from_frame_rate_code(h.frame_rate_code);

    println!("path: {}", cli.input.display());
    println!("width: {}", h.width);
    println!("height: {}", h.height);
    println!("frame_rate_code: {}", h.frame_rate_code);
    if let Some(f) = fps {
        println!("fps: {f}");
    }

    Ok(())
}
