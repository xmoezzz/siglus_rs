use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use siglus_assets::{nwa, ovk};

/// Supported container types for Siglus BGM inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgmContainer {
    /// Siglus NWA (packed PCM).
    Nwa,
    /// Siglus OVK pack (Ogg/Vorbis entries).
    Ovk,
    /// Siglus OWP (XOR-obfuscated Ogg/Vorbis).
    Owp,
    /// Plain Ogg/Vorbis.
    Ogg,
    /// Plain WAV.
    Wav,
    /// Unknown (fallback).
    Unknown,
}

impl BgmContainer {
    pub fn from_path(path: &Path) -> BgmContainer {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "nwa" => BgmContainer::Nwa,
            "ovk" => BgmContainer::Ovk,
            "owp" => BgmContainer::Owp,
            "ogg" => BgmContainer::Ogg,
            "wav" => BgmContainer::Wav,
            _ => BgmContainer::Unknown,
        }
    }
}


/// Encoded audio payload format used directly for BGM playback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BgmPlaybackFormat {
    Ogg,
    Wav,
}

/// BGM payload prepared for playback without unnecessary quality-reducing conversion.
#[derive(Debug, Clone)]
pub struct BgmPlaybackData {
    pub container: BgmContainer,
    pub format: BgmPlaybackFormat,
    pub bytes: Vec<u8>,
    pub channels: u16,
    pub sample_rate: u32,
    pub total_samples: u64,
    pub description: String,
}

#[derive(Debug, Clone, Copy)]
struct BasicAudioInfo {
    channels: u16,
    sample_rate: u32,
    total_samples: u64,
}

fn inspect_pcm_wav_bytes(wav: &[u8]) -> Result<BasicAudioInfo> {
    if wav.len() < 44 || &wav[0..4] != b"RIFF" || &wav[8..12] != b"WAVE" {
        bail!("not a RIFF/WAVE file");
    }

    let mut pos = 12usize;
    let mut channels: Option<u16> = None;
    let mut sample_rate: Option<u32> = None;
    let mut block_align: Option<usize> = None;
    let mut data_len: Option<usize> = None;

    while pos + 8 <= wav.len() {
        let id = &wav[pos..pos + 4];
        let sz = u32::from_le_bytes([wav[pos + 4], wav[pos + 5], wav[pos + 6], wav[pos + 7]]) as usize;
        pos += 8;
        if pos + sz > wav.len() {
            bail!("truncated WAV chunk");
        }
        if id == b"fmt " {
            if sz < 16 {
                bail!("truncated WAV fmt chunk");
            }
            channels = Some(u16::from_le_bytes([wav[pos + 2], wav[pos + 3]]).max(1));
            sample_rate = Some(u32::from_le_bytes([
                wav[pos + 4],
                wav[pos + 5],
                wav[pos + 6],
                wav[pos + 7],
            ]).max(1));
            block_align = Some(u16::from_le_bytes([wav[pos + 12], wav[pos + 13]]) as usize);
        } else if id == b"data" {
            data_len = Some(sz);
        }
        pos += sz;
        if (sz & 1) != 0 {
            pos += 1;
        }
    }

    let channels = channels.context("WAV fmt chunk missing channels")?;
    let sample_rate = sample_rate.context("WAV fmt chunk missing sample rate")?;
    let block_align = block_align.context("WAV fmt chunk missing block align")?.max(1);
    let data_len = data_len.context("WAV data chunk missing")?;
    Ok(BasicAudioInfo {
        channels,
        sample_rate,
        total_samples: (data_len / block_align) as u64,
    })
}

fn inspect_ogg_vorbis_bytes(ogg: &[u8]) -> Result<BasicAudioInfo> {
    let mut pos = 0usize;
    let mut channels: Option<u16> = None;
    let mut sample_rate: Option<u32> = None;
    let mut max_granule: i64 = -1;

    while pos + 27 <= ogg.len() {
        if &ogg[pos..pos + 4] != b"OggS" {
            bail!("invalid Ogg capture pattern at byte {}", pos);
        }
        let granule = i64::from_le_bytes([
            ogg[pos + 6],
            ogg[pos + 7],
            ogg[pos + 8],
            ogg[pos + 9],
            ogg[pos + 10],
            ogg[pos + 11],
            ogg[pos + 12],
            ogg[pos + 13],
        ]);
        if granule >= 0 {
            max_granule = max_granule.max(granule);
        }
        let seg_count = ogg[pos + 26] as usize;
        let seg_table = pos + 27;
        let data_start = seg_table + seg_count;
        if data_start > ogg.len() {
            bail!("truncated Ogg segment table");
        }
        let page_data_len = ogg[seg_table..data_start]
            .iter()
            .fold(0usize, |acc, b| acc.saturating_add(*b as usize));
        let data_end = data_start.saturating_add(page_data_len);
        if data_end > ogg.len() {
            bail!("truncated Ogg page data");
        }

        let mut packet_off = data_start;
        let mut packet_len = 0usize;
        for lace in &ogg[seg_table..data_start] {
            packet_len = packet_len.saturating_add(*lace as usize);
            if *lace < 255 {
                if packet_off + packet_len <= data_end {
                    let packet = &ogg[packet_off..packet_off + packet_len];
                    if packet.len() >= 16 && packet[0] == 1 && &packet[1..7] == b"vorbis" {
                        channels = Some((packet[11] as u16).max(1));
                        sample_rate = Some(u32::from_le_bytes([
                            packet[12],
                            packet[13],
                            packet[14],
                            packet[15],
                        ]).max(1));
                    }
                }
                packet_off = packet_off.saturating_add(packet_len);
                packet_len = 0;
            }
        }

        pos = data_end;
    }

    let channels = channels.context("Vorbis identification header missing")?;
    let sample_rate = sample_rate.context("Vorbis identification sample rate missing")?;
    if max_granule < 0 {
        bail!("Ogg Vorbis final granule position missing");
    }
    Ok(BasicAudioInfo {
        channels,
        sample_rate,
        total_samples: max_granule as u64,
    })
}

/// Prepare BGM bytes for direct playback.
///
/// Vorbis-based containers are kept as encoded Ogg/Vorbis bytes so Kira/Symphonia
/// decodes the original stream directly. Only NWA is converted to PCM WAV because
/// it is a Siglus PCM compression format rather than a Vorbis stream.
pub fn decode_bgm_to_playback_bytes(
    input: impl AsRef<Path>,
    entry_idx: Option<usize>,
) -> Result<BgmPlaybackData> {
    let input = input.as_ref();
    let kind = BgmContainer::from_path(input);

    match kind {
        BgmContainer::Ovk | BgmContainer::Owp | BgmContainer::Ogg => {
            let (ogg, description) = extract_ogg_bytes(input, entry_idx)?;
            let info = inspect_ogg_vorbis_bytes(&ogg)
                .with_context(|| format!("inspect Ogg/Vorbis BGM: {}", input.display()))?;
            Ok(BgmPlaybackData {
                container: kind,
                format: BgmPlaybackFormat::Ogg,
                bytes: ogg,
                channels: info.channels,
                sample_rate: info.sample_rate,
                total_samples: info.total_samples,
                description,
            })
        }
        BgmContainer::Wav => {
            let wav = fs::read(input).with_context(|| format!("read WAV: {}", input.display()))?;
            let info = inspect_pcm_wav_bytes(&wav)
                .with_context(|| format!("inspect WAV BGM: {}", input.display()))?;
            Ok(BgmPlaybackData {
                container: kind,
                format: BgmPlaybackFormat::Wav,
                bytes: wav,
                channels: info.channels,
                sample_rate: info.sample_rate,
                total_samples: info.total_samples,
                description: format!("WAV:{}", input.display()),
            })
        }
        BgmContainer::Nwa => {
            let mut reader = nwa::NwaReader::open(input)
                .with_context(|| format!("open NWA: {}", input.display()))?;
            let wav = reader.to_wav_bytes().context("decode NWA -> WAV")?;
            let info = inspect_pcm_wav_bytes(&wav)
                .with_context(|| format!("inspect NWA-decoded WAV: {}", input.display()))?;
            Ok(BgmPlaybackData {
                container: kind,
                format: BgmPlaybackFormat::Wav,
                bytes: wav,
                channels: info.channels,
                sample_rate: info.sample_rate,
                total_samples: info.total_samples,
                description: format!("NWA:{}", input.display()),
            })
        }
        BgmContainer::Unknown => {
            bail!(
                "unsupported BGM container (by extension): {}",
                input.display()
            );
        }
    }
}

/// Decoded audio payload ready for export or playback.
#[derive(Debug, Clone)]
pub struct BgmDecoded {
    pub container: BgmContainer,
    /// WAV (PCM16) bytes.
    pub wav_bytes: Vec<u8>,
    /// A helpful description for logs/UI.
    pub description: String,
}

#[derive(Debug, Clone)]
pub enum KoeSource {
    File(PathBuf),
    OvkEntryByNo { path: PathBuf, entry_no: u32 },
}

/// Decode various Siglus audio containers into WAV (PCM16).
///
/// * For `.ovk`, `entry_idx` selects which entry to decode.
/// * For other formats, `entry_idx` is ignored.
#[allow(clippy::needless_pass_by_value)]
pub fn decode_bgm_to_wav_bytes(
    input: impl AsRef<Path>,
    entry_idx: Option<usize>,
) -> Result<BgmDecoded> {
    let input = input.as_ref();
    let kind = BgmContainer::from_path(input);

    match kind {
        BgmContainer::Nwa => {
            let mut reader = nwa::NwaReader::open(input)
                .with_context(|| format!("open NWA: {}", input.display()))?;
            let wav_bytes = reader.to_wav_bytes().context("decode NWA -> WAV")?;
            Ok(BgmDecoded {
                container: kind,
                wav_bytes,
                description: format!("NWA:{}", input.display()),
            })
        }
        BgmContainer::Ovk => {
            let pack = ovk::OvkPack::open(input)
                .with_context(|| format!("open OVK: {}", input.display()))?;
            let idx = entry_idx.unwrap_or(0);
            let entry_cnt = pack.entries().len();
            if idx >= entry_cnt {
                bail!("OVK entry out of range: idx={} entries={}", idx, entry_cnt);
            }
            {
                let wav_bytes = pack
                    .decode_entry_vorbis_wav(idx)
                    .context("decode OVK(entry) -> WAV")?;
                Ok(BgmDecoded {
                    container: kind,
                    wav_bytes,
                    description: format!("OVK:{}[{}]", input.display(), idx),
                })
            }
        }
        BgmContainer::Owp => {
            let owp = ovk::OwpFile::open(input)
                .with_context(|| format!("open OWP: {}", input.display()))?;
            {
                let wav_bytes = owp.decode_vorbis_wav().context("decode OWP -> WAV")?;
                Ok(BgmDecoded {
                    container: kind,
                    wav_bytes,
                    description: format!("OWP:{}", input.display()),
                })
            }
        }
        BgmContainer::Ogg => {
            let bytes =
                fs::read(input).with_context(|| format!("read OGG: {}", input.display()))?;
            let wav_bytes =
                siglus_assets::vorbis::decode_ogg_vorbis_reader_to_wav(Cursor::new(bytes))
                    .context("decode OGG/Vorbis -> WAV")?;
            Ok(BgmDecoded {
                container: kind,
                wav_bytes,
                description: format!("OGG:{}", input.display()),
            })
        }
        BgmContainer::Wav => {
            let wav_bytes =
                fs::read(input).with_context(|| format!("read WAV: {}", input.display()))?;
            Ok(BgmDecoded {
                container: kind,
                wav_bytes,
                description: format!("WAV:{}", input.display()),
            })
        }
        BgmContainer::Unknown => {
            bail!(
                "unsupported BGM container (by extension): {}",
                input.display()
            );
        }
    }
}

pub fn decode_ovk_entry_by_no_to_wav_bytes(
    input: impl AsRef<Path>,
    entry_no: u32,
) -> Result<BgmDecoded> {
    let input = input.as_ref();
    let pack =
        ovk::OvkPack::open(input).with_context(|| format!("open OVK: {}", input.display()))?;
    let idx = pack
        .entries()
        .iter()
        .position(|e| e.no == entry_no)
        .with_context(|| {
            format!(
                "OVK entry not found: no={} file={}",
                entry_no,
                input.display()
            )
        })?;

    {
        let wav_bytes = pack
            .decode_entry_vorbis_wav(idx)
            .with_context(|| format!("decode OVK(entry no={entry_no}) -> WAV"))?;
        Ok(BgmDecoded {
            container: BgmContainer::Ovk,
            wav_bytes,
            description: format!("OVK:{}#{}", input.display(), entry_no),
        })
    }
}

/// Extract raw Ogg bytes from Siglus containers.
///
/// * `.ovk`: extracts the Ogg segment at `entry_idx`.
/// * `.owp`: decrypts the whole file to raw Ogg.
pub fn extract_ogg_bytes(
    input: impl AsRef<Path>,
    entry_idx: Option<usize>,
) -> Result<(Vec<u8>, String)> {
    let input = input.as_ref();
    let kind = BgmContainer::from_path(input);

    match kind {
        BgmContainer::Ovk => {
            let pack = ovk::OvkPack::open(input)
                .with_context(|| format!("open OVK: {}", input.display()))?;
            let idx = entry_idx.unwrap_or(0);
            let entry_cnt = pack.entries().len();
            if idx >= entry_cnt {
                bail!("OVK entry out of range: idx={} entries={}", idx, entry_cnt);
            }
            let ogg = pack.extract_entry(idx).context("extract OVK entry")?;
            Ok((ogg, format!("OVK:{}[{}]", input.display(), idx)))
        }
        BgmContainer::Owp => {
            let owp = ovk::OwpFile::open(input)
                .with_context(|| format!("open OWP: {}", input.display()))?;
            let ogg = owp.decrypt_to_vec().context("decrypt OWP -> Ogg")?;
            Ok((ogg, format!("OWP:{}", input.display())))
        }
        BgmContainer::Ogg => {
            let ogg = fs::read(input).with_context(|| format!("read OGG: {}", input.display()))?;
            Ok((ogg, format!("OGG:{}", input.display())))
        }
        _ => bail!("container does not support Ogg extraction: {:?}", kind),
    }
}

pub fn resolve_koe_source(project_dir: &Path, koe_no: i64) -> Result<KoeSource> {
    if koe_no < 0 {
        bail!("invalid koe number: {koe_no}");
    }

    let koe_no_u32 = koe_no as u32;
    let scn_no = koe_no_u32 / 100_000;
    let base = project_dir.join("koe");
    for dir in [format!("{:04}", scn_no), scn_no.to_string()] {
        for stem in [
            format!("z{:09}", koe_no_u32),
            format!("Z{:09}", koe_no_u32),
            format!("z{}", koe_no_u32),
            format!("Z{}", koe_no_u32),
        ] {
            for ext in ["wav", "nwa", "ogg"] {
                let p = base.join(&dir).join(format!("{stem}.{ext}"));
                if p.exists() {
                    return Ok(KoeSource::File(p));
                }
            }
        }
    }

    let ovk = base.join(format!("z{:04}.ovk", scn_no));
    if ovk.exists() {
        return Ok(KoeSource::OvkEntryByNo {
            path: ovk,
            entry_no: koe_no_u32 % 100_000,
        });
    }

    bail!("koe resource not found: koe_no={koe_no}")
}
