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
    project: Option<PathBuf>,

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
    let project = cli.project.clone().unwrap_or(siglus_scene_vm::app_path::resolve_app_base_path()?);

    let path = [
        "Gameexe.dat",
        "GameexeEN.dat",
        "GameexeZH.dat",
        "GameexeZHTW.dat",
        "GameexeDE.dat",
        "GameexeES.dat",
        "GameexeFR.dat",
        "GameexeID.dat",
    ]
    .iter()
    .map(|name| project.join(name))
    .find(|p| p.is_file())
    .with_context(|| format!("find Gameexe*.dat under {}", project.display()))?;
    let raw = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;

    let opt = GameexeDecodeOptions::from_project_dir(&project)?;
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
            println!("entry_count: {}", cfg.entries.len());
            println!("DATABASE inferred count: {}", cfg.indexed_count("DATABASE"));
            println!("MASK inferred count: {}", cfg.indexed_count("MASK"));
            println!("EDITBOX inferred count: {}", cfg.indexed_count("EDITBOX"));
            println!("G00BUF inferred count: {}", cfg.indexed_count("G00BUF"));

            let dat_dir = project.join("dat");
            if dat_dir.is_dir() {
                println!("dat_dir: {}", dat_dir.display());
            } else {
                println!("dat_dir: {} (missing)", dat_dir.display());
            }

            let db_cnt = cfg.indexed_count("DATABASE");
            if db_cnt > 0 {
                println!("databases:");
            }
            for i in 0..db_cnt {
                let k = format!("DATABASE.{i}");
                let Some(name) = cfg.get_indexed_unquoted("DATABASE", i).or_else(|| cfg.get_indexed_item_unquoted("DATABASE", i, 0)) else {
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
    match cfg.get_entry(key) {
        Some(v) => println!("{key}: {}", v.scalar_unquoted()),
        None => println!("{key}: (missing)"),
    }
}
