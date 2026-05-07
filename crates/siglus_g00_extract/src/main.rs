use anyhow::{bail, Context, Result};
use clap::Parser;
use siglus_scene_vm::assets::{g00, RgbaImage};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Parser)]
#[command(name = "siglus_g00_extract")]
#[command(about = "Extract Siglus G00 cuts using siglus_scene_vm's engine decoder")]
struct Args {
    #[arg(long, value_name = "G00")]
    input: PathBuf,

    #[arg(long, value_name = "DIR")]
    output: PathBuf,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = Args::parse();

    let bytes = fs::read(&args.input).with_context(|| format!("read {}", args.input.display()))?;
    let decoded = g00::decode_g00(&bytes).with_context(|| format!("decode {}", args.input.display()))?;

    fs::create_dir_all(&args.output).with_context(|| format!("create {}", args.output.display()))?;

    let stem = args
        .input
        .file_stem()
        .and_then(|s| s.to_str())
        .filter(|s| !s.is_empty())
        .unwrap_or("g00");

    let meta_path = args.output.join(format!("{stem}_engine_meta.tsv"));
    let mut meta = BufWriter::new(
        File::create(&meta_path).with_context(|| format!("create {}", meta_path.display()))?,
    );
    writeln!(
        meta,
        "index\tfile\twidth\theight\tcenter_x\tcenter_y\talpha_nonzero\tbbox_left\tbbox_top\tbbox_right_exclusive\tbbox_bottom_exclusive"
    )?;

    for (index, frame) in decoded.frames.iter().enumerate() {
        validate_frame(frame).with_context(|| format!("validate frame {index}"))?;
        let file_name = format!("{stem}_cut{index:03}_engine.png");
        let png_path = args.output.join(&file_name);
        write_png(frame, &png_path).with_context(|| format!("write {}", png_path.display()))?;

        let stats = alpha_stats(frame);
        writeln!(
            meta,
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            index,
            file_name,
            frame.width,
            frame.height,
            frame.center_x,
            frame.center_y,
            stats.alpha_nonzero,
            opt_i32(stats.bbox_left),
            opt_i32(stats.bbox_top),
            opt_i32(stats.bbox_right_exclusive),
            opt_i32(stats.bbox_bottom_exclusive),
        )?;
    }
    meta.flush()?;

    println!(
        "decoded {}: kind={:?} canvas={}x{} cuts={} output={} meta={}",
        args.input.display(),
        decoded.kind,
        decoded.width,
        decoded.height,
        decoded.frames.len(),
        args.output.display(),
        meta_path.display(),
    );
    Ok(())
}

fn validate_frame(frame: &RgbaImage) -> Result<()> {
    let expected = frame
        .width
        .checked_mul(frame.height)
        .and_then(|v| v.checked_mul(4))
        .context("frame size overflow")? as usize;
    if frame.rgba.len() != expected {
        bail!(
            "RGBA length mismatch: got={} expected={} size={}x{}",
            frame.rgba.len(),
            expected,
            frame.width,
            frame.height,
        );
    }
    Ok(())
}

fn write_png(frame: &RgbaImage, path: &Path) -> Result<()> {
    let image = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba.clone())
        .context("construct image buffer from engine RGBA")?;
    image
        .save(path)
        .with_context(|| format!("save {}", path.display()))?;
    Ok(())
}

#[derive(Debug, Clone, Copy)]
struct AlphaStats {
    alpha_nonzero: usize,
    bbox_left: Option<i32>,
    bbox_top: Option<i32>,
    bbox_right_exclusive: Option<i32>,
    bbox_bottom_exclusive: Option<i32>,
}

fn alpha_stats(frame: &RgbaImage) -> AlphaStats {
    let mut alpha_nonzero = 0usize;
    let mut left = frame.width as i32;
    let mut top = frame.height as i32;
    let mut right = 0i32;
    let mut bottom = 0i32;

    for y in 0..frame.height as i32 {
        for x in 0..frame.width as i32 {
            let idx = ((y as u32 * frame.width + x as u32) * 4 + 3) as usize;
            if frame.rgba[idx] != 0 {
                alpha_nonzero += 1;
                left = left.min(x);
                top = top.min(y);
                right = right.max(x + 1);
                bottom = bottom.max(y + 1);
            }
        }
    }

    if alpha_nonzero == 0 {
        AlphaStats {
            alpha_nonzero,
            bbox_left: None,
            bbox_top: None,
            bbox_right_exclusive: None,
            bbox_bottom_exclusive: None,
        }
    } else {
        AlphaStats {
            alpha_nonzero,
            bbox_left: Some(left),
            bbox_top: Some(top),
            bbox_right_exclusive: Some(right),
            bbox_bottom_exclusive: Some(bottom),
        }
    }
}

fn opt_i32(value: Option<i32>) -> String {
    match value {
        Some(v) => v.to_string(),
        None => String::new(),
    }
}
