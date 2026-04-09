#!/usr/bin/env rust

use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use siglus_scene_vm::audio::bgm;
use siglus_scene_vm::audio::{AudioHub, TrackKind};

#[derive(Parser, Debug)]
#[command(name = "bgm_tool")]
#[command(about = "Decode/extract Siglus BGM formats (NWA/OVK/OWP) and optionally play them.")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// List OVK entries (name/offset/size).
    List { input: PathBuf },

    /// Decode input (NWA/OVK/OWP/OGG) to a WAV file.
    DumpWav {
        input: PathBuf,
        #[arg(long)]
        entry: Option<usize>,
        #[arg(long)]
        out: PathBuf,
    },

    /// Extract raw Ogg bytes (OVK/OWP only).
    ExtractOgg {
        input: PathBuf,
        #[arg(long)]
        entry: Option<usize>,
        #[arg(long)]
        out: PathBuf,
    },

    /// Play the audio using Kira.
    ///
    /// This blocks until you press Enter.
    Play {
        input: PathBuf,
        #[arg(long)]
        entry: Option<usize>,
        #[arg(long, default_value_t = false)]
        r#loop: bool,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.cmd {
        Cmd::List { input } => {
            let pack = siglus_assets::ovk::OvkPack::open(&input)
                .with_context(|| format!("open OVK: {}", input.display()))?;
            for (i, e) in pack.entries().iter().enumerate() {
                println!(
                    "#{i}: no={} offset=0x{:X} size=0x{:X} sample_count={}",
                    e.no, e.offset, e.size, e.sample_count
                );
            }
        }
        Cmd::DumpWav { input, entry, out } => {
            let decoded = bgm::decode_bgm_to_wav_bytes(&input, entry)
                .with_context(|| format!("decode to wav: {}", input.display()))?;
            std::fs::write(&out, decoded.wav_bytes)
                .with_context(|| format!("write {}", out.display()))?;
        }
        Cmd::ExtractOgg { input, entry, out } => {
            let (ogg, _desc) = bgm::extract_ogg_bytes(&input, entry)
                .with_context(|| format!("extract ogg: {}", input.display()))?;
            std::fs::write(&out, ogg).with_context(|| format!("write {}", out.display()))?;
        }
        Cmd::Play {
            input,
            entry,
            r#loop,
        } => {
            play_impl(&input, entry, r#loop)?;
        }
    }

    Ok(())
}

fn play_impl(input: &Path, entry: Option<usize>, looped: bool) -> Result<()> {
    let decoded = bgm::decode_bgm_to_wav_bytes(input, entry)?;

    let mut hub = AudioHub::new();

    let data =
        kira::sound::static_sound::StaticSoundData::from_cursor(Cursor::new(decoded.wav_bytes))
            .context("kira: decode WAV bytes")?;

    let mut handle = hub.play_static(TrackKind::Bgm, data)?;

    if looped {
        // Best-effort: looping depends on the Kira version's sound settings.
        // The runtime BgmEngine keeps a loop flag for future 1:1 semantics.
    }

    println!("Playing. Press Enter to stop.");
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    let _ = handle.stop(kira::tween::Tween::default());
    Ok(())
}
