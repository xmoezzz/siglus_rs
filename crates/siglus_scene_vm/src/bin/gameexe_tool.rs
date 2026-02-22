#!/usr/bin/env rust

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use siglus_assets::gameexe::{decode_gameexe_dat_bytes, GameexeConfig, GameexeDecodeOptions};

#[derive(Parser, Debug)]
#[command(name = "gameexe_tool")]
#[command(about = "Decode Gameexe.dat and show derived table paths (CGTABLE/DBS/ThumbTable).")]
struct Cli {
    /// Game project directory (contains Gameexe.dat and dat/).
    #[arg(long)]
    project: PathBuf,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand, Debug)]
enum Cmd {
    /// Print a summary (encoding, xor steps, table file names).
    Summary,

    /// Dump decoded INI text to stdout.
    DumpIni,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let path = cli.project.join("Gameexe.dat");
    let raw = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;

    let opt = GameexeDecodeOptions::from_env().unwrap_or_default();
    let (text, report) = decode_gameexe_dat_bytes(&raw, &opt)
        .with_context(|| "decode Gameexe.dat (plaintext or xor/lzss)")?;

    match cli.cmd {
        Cmd::DumpIni => {
            print!("{text}");
        }
        Cmd::Summary => {
            println!("Gameexe.dat: {}", path.display());
            println!("encoding: {:?}", report.encoding);
            println!("used_lzss: {}", report.used_lzss);
            println!("xor_chain_order: {:?}", opt.chain_order);
            if report.applied_xor.is_empty() {
                println!("xor_steps_applied: (none)");
            } else {
                println!("xor_steps_applied:");
                for (k, n) in &report.applied_xor {
                    println!("  {:?} ({} bytes)", k, n);
                }
            }

            let cfg = GameexeConfig::from_text(&text);
            print_key(&cfg, "CGTABLE_FILE");
            print_key(&cfg, "CGTABLE_FLAG_CNT");
            print_key(&cfg, "THUMBTABLE_FILE");
            print_key(&cfg, "DATABASE.CNT");

            let dat_dir = cli.project.join("dat");
            if dat_dir.is_dir() {
                println!("dat_dir: {}", dat_dir.display());
            } else {
                println!("dat_dir: {} (missing)", dat_dir.display());
            }

            let db_cnt = cfg
                .get("DATABASE.CNT")
                .and_then(|s| s.trim().parse::<usize>().ok())
                .unwrap_or(0);
            if db_cnt > 0 {
                println!("databases:");
            }
            for i in 0..db_cnt {
                let k = format!("DATABASE.{i}");
                let Some(name) = cfg.get(&k) else {
                    println!("  {k}: (missing)");
                    continue;
                };
                let mut p = PathBuf::from(name.trim());
                if p.extension().is_none() {
                    p.set_extension("dbs");
                }
                let full = dat_dir.join(&p);
                println!(
                    "  {k}: {} -> {}{}",
                    name.trim(),
                    full.display(),
                    if full.is_file() { "" } else { " (missing)" }
                );
            }
        }
    }

    Ok(())
}

fn print_key(cfg: &GameexeConfig, key: &str) {
    match cfg.get(key) {
        Some(v) => println!("{key}: {v}"),
        None => println!("{key}: (missing)"),
    }
}
