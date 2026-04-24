use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use siglus_assets::angou::{parse_hex_bytes, xor_cycle_in_place};
use siglus_assets::key_toml::{self, KeyTomlConfig};
use siglus_assets::keys::GAMEEXE_KEY;
use siglus_assets::lzss::lzss_unpack_lenient;

const GAMEEXE_HEADER_SIZE: usize = 8;

#[derive(Debug, Clone)]
struct Args {
    input: PathBuf,
    output: Option<PathBuf>,
    project_dir: Option<PathBuf>,
    key_file: Option<PathBuf>,
    explicit_key: Option<[u8; 16]>,
    force: bool,
    verbose: bool,
}

#[derive(Debug, Clone, Copy)]
struct GameexeHeader {
    version: i32,
    exe_angou_mode: i32,
}

#[derive(Debug, Clone)]
struct DecodeConfig {
    exe_key16: Option<[u8; 16]>,
    config_source: Option<PathBuf>,
}

#[derive(Debug, Clone)]
struct DecodeResult {
    text: String,
    layout: DecodeLayout,
    applied_steps: Vec<String>,
}

#[derive(Debug, Clone)]
enum DecodeLayout {
    Headered(GameexeHeader),
    Headerless,
}

impl DecodeLayout {
    fn describe(&self) -> String {
        match self {
            DecodeLayout::Headered(h) => format!(
                "headered(version={}, exe_angou_mode={})",
                h.version, h.exe_angou_mode
            ),
            DecodeLayout::Headerless => "headerless".to_string(),
        }
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let args = parse_args()?;
    let raw = fs::read(&args.input).with_context(|| format!("read {}", args.input.display()))?;
    let config = load_decode_config(&args)?;
    let result = decode_gameexe_dat(&raw, &config, args.verbose)?;

    let out_path = args
        .output
        .clone()
        .unwrap_or_else(|| default_output_path(&args.input));
    if out_path.exists() && !args.force {
        bail!(
            "output already exists: {} (pass --force to overwrite)",
            out_path.display()
        );
    }
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
    }
    fs::write(&out_path, result.text.as_bytes())
        .with_context(|| format!("write {}", out_path.display()))?;

    eprintln!(
        "wrote {} ({}, applied={})",
        out_path.display(),
        result.layout.describe(),
        if result.applied_steps.is_empty() {
            "none".to_string()
        } else {
            result.applied_steps.join(",")
        }
    );
    Ok(())
}

fn parse_args() -> Result<Args> {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut project_dir: Option<PathBuf> = None;
    let mut key_file: Option<PathBuf> = None;
    let mut explicit_key: Option<[u8; 16]> = None;
    let mut force = false;
    let mut verbose = false;

    let mut it = env::args().skip(1).peekable();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            "--gameexe-dat" | "--input" => {
                input = Some(next_path(&mut it, &arg)?);
            }
            "--out" | "-o" => {
                output = Some(next_path(&mut it, &arg)?);
            }
            "--project-dir" => {
                project_dir = Some(next_path(&mut it, &arg)?);
            }
            "--key-file" => {
                key_file = Some(next_path(&mut it, &arg)?);
            }
            "--key" => {
                let raw = next_string(&mut it, &arg)?;
                explicit_key = Some(parse_key16_hex(&raw).with_context(|| "parse --key")?);
            }
            "--force" => force = true,
            "--verbose" => verbose = true,
            _ if arg.starts_with('-') => bail!("unknown option: {arg}"),
            _ => {
                if let Some(prev) = &input {
                    bail!(
                        "multiple input files specified: {} and {arg}",
                        prev.display()
                    );
                }
                input = Some(PathBuf::from(arg));
            }
        }
    }

    let input = input.ok_or_else(|| anyhow!("missing Gameexe.dat input"))?;
    Ok(Args {
        input,
        output,
        project_dir,
        key_file,
        explicit_key,
        force,
        verbose,
    })
}

fn print_usage() {
    println!(
        "Usage:\n  siglus_gameexe_ini <Gameexe.dat> --out <Gameexe.ini> [--key HEX] [--key-file key.toml] [--project-dir DIR] [--force] [--verbose]\n\nBehavior:\n  Expands Siglus Gameexe.dat to UTF-8 Gameexe.ini.\n  Supports the original 8-byte headered format and a headerless LZSS body format.\n\nOriginal headered format:\n  header {{ i32 version, i32 exe_angou_mode }}\n  body is XORed by the optional 16-byte exe key when exe_angou_mode != 0,\n  then XORed by the fixed GAMEEXE_KEY, then LZSS-unpacked as UTF-16LE.\n\nExternal key lookup when exe_angou_mode != 0:\n  1. --key HEX\n  2. --key-file key.toml\n  3. --project-dir/key.toml\n  4. <Gameexe.dat directory>/key.toml\n  5. ./key.toml\n"
    );
}

fn next_string<I>(it: &mut std::iter::Peekable<I>, opt: &str) -> Result<String>
where
    I: Iterator<Item = String>,
{
    it.next().ok_or_else(|| anyhow!("{opt} requires a value"))
}

fn next_path<I>(it: &mut std::iter::Peekable<I>, opt: &str) -> Result<PathBuf>
where
    I: Iterator<Item = String>,
{
    Ok(PathBuf::from(next_string(it, opt)?))
}

fn load_decode_config(args: &Args) -> Result<DecodeConfig> {
    let mut config = DecodeConfig {
        exe_key16: None,
        config_source: None,
    };

    if let Some(path) = &args.key_file {
        merge_key_toml(&mut config, path)?;
    } else {
        for path in default_key_toml_candidates(args) {
            if !path.is_file() {
                continue;
            }
            merge_key_toml(&mut config, &path)?;
            break;
        }
    }

    if let Some(key) = args.explicit_key {
        config.exe_key16 = Some(key);
    }

    Ok(config)
}

fn default_key_toml_candidates(args: &Args) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    if let Some(project_dir) = &args.project_dir {
        candidates.push(project_dir.join("key.toml"));
    }
    if let Some(parent) = args.input.parent() {
        if !parent.as_os_str().is_empty() {
            candidates.push(parent.join("key.toml"));
        }
    }
    candidates.push(PathBuf::from("key.toml"));
    candidates
}

fn merge_key_toml(config: &mut DecodeConfig, path: &Path) -> Result<()> {
    let parsed: KeyTomlConfig = key_toml::load_key_toml_from_file(path)
        .with_context(|| format!("load {}", path.display()))?;
    config.config_source = Some(path.to_path_buf());
    if parsed.exe_key16.is_some() {
        config.exe_key16 = parsed.exe_key16;
    }
    Ok(())
}

fn decode_gameexe_dat(raw: &[u8], config: &DecodeConfig, verbose: bool) -> Result<DecodeResult> {
    if raw.len() < GAMEEXE_HEADER_SIZE {
        bail!("Gameexe.dat too short: {} bytes", raw.len());
    }

    let first_i32 = read_i32_le(raw, 0)?;
    let second_i32 = read_i32_le(raw, 4)?;

    if verbose {
        let source = config
            .config_source
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "none".to_string());
        eprintln!("key config: {source}");
        eprintln!("first two i32: {first_i32}, {second_i32}");
        eprintln!("raw header probe: {}", describe_lzss_header(raw));
    }

    if first_i32 == 0 {
        let header = GameexeHeader {
            version: first_i32,
            exe_angou_mode: second_i32,
        };
        if header.exe_angou_mode != 0 && config.exe_key16.is_none() {
            bail!(
                "Gameexe.dat requires external exe-angou key, but no key was provided. Use --key HEX, --key-file key.toml, --project-dir DIR, or place key.toml beside Gameexe.dat."
            );
        }
        return decode_headered(raw, header, config, verbose)
            .with_context(|| "decode original headered Gameexe.dat");
    }

    // A non-zero first i32 is not an original header version. In practice it is
    // often the LZSS arc_size of a headerless body. Treat it as an explicit
    // headerless format instead of rejecting it as an unsupported version.
    decode_headerless(raw, config, verbose).with_context(|| {
        format!(
            "decode headerless Gameexe.dat body; first dword was {first_i32}, not an original header version"
        )
    })
}

fn decode_headered(
    raw: &[u8],
    header: GameexeHeader,
    config: &DecodeConfig,
    verbose: bool,
) -> Result<DecodeResult> {
    let mut body = raw[GAMEEXE_HEADER_SIZE..].to_vec();
    let mut applied = Vec::new();

    if header.exe_angou_mode != 0 {
        let key = config.exe_key16.expect("checked above");
        xor_cycle_in_place(&mut body, &key);
        applied.push("exe_key16".to_string());
    }

    xor_cycle_in_place(&mut body, &GAMEEXE_KEY);
    applied.push(format!("gameexe_fixed_key[{}]", GAMEEXE_KEY.len()));

    if verbose {
        eprintln!(
            "headered decrypted lzss probe: {}",
            describe_lzss_header(&body)
        );
    }

    let text = unpack_and_decode_gameexe_text(&body).with_context(|| {
        format!(
            "unpack/decode headered Gameexe.dat after {}",
            applied.join(",")
        )
    })?;

    Ok(DecodeResult {
        text,
        layout: DecodeLayout::Headered(header),
        applied_steps: applied,
    })
}

fn decode_headerless(raw: &[u8], config: &DecodeConfig, verbose: bool) -> Result<DecodeResult> {
    let mut errors = Vec::new();

    match unpack_and_decode_gameexe_text(raw) {
        Ok(text) => {
            if verbose {
                eprintln!("headerless accepted as plain LZSS body");
            }
            return Ok(DecodeResult {
                text,
                layout: DecodeLayout::Headerless,
                applied_steps: Vec::new(),
            });
        }
        Err(err) => errors.push(format!("plain LZSS body: {err:#}")),
    }

    let mut fixed = raw.to_vec();
    xor_cycle_in_place(&mut fixed, &GAMEEXE_KEY);
    if verbose {
        eprintln!(
            "headerless fixed-key decrypted lzss probe: {}",
            describe_lzss_header(&fixed)
        );
    }
    match unpack_and_decode_gameexe_text(&fixed) {
        Ok(text) => {
            return Ok(DecodeResult {
                text,
                layout: DecodeLayout::Headerless,
                applied_steps: vec![format!("gameexe_fixed_key[{}]", GAMEEXE_KEY.len())],
            });
        }
        Err(err) => errors.push(format!("fixed-key LZSS body: {err:#}")),
    }

    if let Some(key) = config.exe_key16 {
        let mut exe_then_fixed = raw.to_vec();
        xor_cycle_in_place(&mut exe_then_fixed, &key);
        xor_cycle_in_place(&mut exe_then_fixed, &GAMEEXE_KEY);
        if verbose {
            eprintln!(
                "headerless exe+fixed decrypted lzss probe: {}",
                describe_lzss_header(&exe_then_fixed)
            );
        }
        match unpack_and_decode_gameexe_text(&exe_then_fixed) {
            Ok(text) => {
                return Ok(DecodeResult {
                    text,
                    layout: DecodeLayout::Headerless,
                    applied_steps: vec![
                        "exe_key16".to_string(),
                        format!("gameexe_fixed_key[{}]", GAMEEXE_KEY.len()),
                    ],
                });
            }
            Err(err) => errors.push(format!("exe+fixed LZSS body: {err:#}")),
        }
    }

    bail!(
        "failed to decode headerless Gameexe.dat body. Tried:\n  {}\nIf this file is exe-angou encrypted, pass --key HEX or --key-file key.toml.",
        errors.join("\n  ")
    )
}

fn unpack_and_decode_gameexe_text(body: &[u8]) -> Result<String> {
    let unpacked = lzss_unpack_lenient(body).with_context(|| {
        format!(
            "lzss unpack Gameexe.dat body; lzss header {}",
            describe_lzss_header(body)
        )
    })?;
    let text = decode_utf16le(&unpacked).with_context(|| "decode Gameexe.dat UTF-16LE text")?;
    if !looks_like_gameexe_ini(&text) {
        bail!("decoded UTF-16LE text does not look like Gameexe.ini");
    }
    Ok(text)
}

fn looks_like_gameexe_ini(text: &str) -> bool {
    text.lines()
        .take(256)
        .filter(|line| line.trim_start().starts_with('#'))
        .take(3)
        .count()
        >= 1
}

fn describe_lzss_header(buf: &[u8]) -> String {
    if buf.len() < 8 {
        return format!("too short: {} bytes", buf.len());
    }
    let arc_size = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
    let org_size = u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]);
    format!(
        "arc_size={} org_size={} body_len={} first8={:02X?}",
        arc_size,
        org_size,
        buf.len(),
        &buf[..8]
    )
}

fn read_i32_le(raw: &[u8], offset: usize) -> Result<i32> {
    let end = offset + 4;
    if end > raw.len() {
        bail!("i32 read out of bounds at 0x{offset:X}");
    }
    Ok(i32::from_le_bytes(raw[offset..end].try_into().unwrap()))
}

fn parse_key16_hex(raw: &str) -> Result<[u8; 16]> {
    let bytes = parse_hex_bytes(raw)?;
    if bytes.len() != 16 {
        bail!("key must contain exactly 16 bytes, got {}", bytes.len());
    }
    let mut out = [0u8; 16];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn decode_utf16le(raw: &[u8]) -> Result<String> {
    let start = if raw.len() >= 2 && raw[0] == 0xFF && raw[1] == 0xFE {
        2usize
    } else {
        0usize
    };
    if (raw.len() - start) % 2 != 0 {
        bail!("UTF-16LE byte length is odd: {}", raw.len() - start);
    }

    let mut words = Vec::with_capacity((raw.len() - start) / 2);
    for chunk in raw[start..].chunks_exact(2) {
        words.push(u16::from_le_bytes([chunk[0], chunk[1]]));
    }
    Ok(String::from_utf16(&words)?)
}

fn default_output_path(input: &Path) -> PathBuf {
    let mut out = input.to_path_buf();
    out.set_file_name("Gameexe.ini");
    out
}
