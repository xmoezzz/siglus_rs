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
pub fn decode_bgm_to_wav_bytes(input: impl AsRef<Path>, entry_idx: Option<usize>) -> Result<BgmDecoded> {
    let input = input.as_ref();
    let kind = BgmContainer::from_path(input);

    match kind {
        BgmContainer::Nwa => {
            let mut reader =
                nwa::NwaReader::open(input).with_context(|| format!("open NWA: {}", input.display()))?;
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
            #[cfg(feature = "assets-vorbis")]
            {
                let wav_bytes = pack.decode_entry_vorbis_wav(idx).context("decode OVK(entry) -> WAV")?;
                Ok(BgmDecoded {
                    container: kind,
                    wav_bytes,
                    description: format!("OVK:{}[{}]", input.display(), idx),
                })
            }
            #[cfg(not(feature = "assets-vorbis"))]
            {
                let _ = pack; // silence unused warnings
                bail!("OVK Vorbis decode requires feature `siglus_scene_vm/assets-vorbis`");
            }
        }
        BgmContainer::Owp => {
            let owp = ovk::OwpFile::open(input).with_context(|| format!("open OWP: {}", input.display()))?;
            #[cfg(feature = "assets-vorbis")]
            {
                let wav_bytes = owp.decode_vorbis_wav().context("decode OWP -> WAV")?;
                Ok(BgmDecoded {
                    container: kind,
                    wav_bytes,
                    description: format!("OWP:{}", input.display()),
                })
            }
            #[cfg(not(feature = "assets-vorbis"))]
            {
                let _ = owp;
                bail!("OWP Vorbis decode requires feature `siglus_scene_vm/assets-vorbis`");
            }
        }
        BgmContainer::Ogg => {
            #[cfg(feature = "assets-vorbis")]
            {
                let bytes = fs::read(input).with_context(|| format!("read OGG: {}", input.display()))?;
                let wav_bytes = siglus_assets::vorbis::decode_ogg_vorbis_reader_to_wav(Cursor::new(bytes))
                    .context("decode OGG/Vorbis -> WAV")?;
                Ok(BgmDecoded {
                    container: kind,
                    wav_bytes,
                    description: format!("OGG:{}", input.display()),
                })
            }
            #[cfg(not(feature = "assets-vorbis"))]
            {
                bail!("OGG Vorbis decode requires feature `siglus_scene_vm/assets-vorbis`");
            }
        }
        BgmContainer::Wav => {
            let wav_bytes = fs::read(input).with_context(|| format!("read WAV: {}", input.display()))?;
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

pub fn decode_ovk_entry_by_no_to_wav_bytes(input: impl AsRef<Path>, entry_no: u32) -> Result<BgmDecoded> {
    let input = input.as_ref();
    let pack = ovk::OvkPack::open(input)
        .with_context(|| format!("open OVK: {}", input.display()))?;
    let idx = pack
        .entries()
        .iter()
        .position(|e| e.no == entry_no)
        .with_context(|| format!("OVK entry not found: no={} file={}", entry_no, input.display()))?;

    #[cfg(feature = "assets-vorbis")]
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
    #[cfg(not(feature = "assets-vorbis"))]
    {
        let _ = idx;
        bail!("OVK Vorbis decode requires feature `siglus_scene_vm/assets-vorbis`");
    }
}

/// Extract raw Ogg bytes from Siglus containers.
///
/// * `.ovk`: extracts the Ogg segment at `entry_idx`.
/// * `.owp`: decrypts the whole file to raw Ogg.
pub fn extract_ogg_bytes(input: impl AsRef<Path>, entry_idx: Option<usize>) -> Result<(Vec<u8>, String)> {
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
            let owp = ovk::OwpFile::open(input).with_context(|| format!("open OWP: {}", input.display()))?;
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
    let nested_stem = format!("{:04}/z{:09}", scn_no, koe_no_u32);

    for ext in ["wav", "nwa"] {
        let p = base.join(format!("{}.{}", nested_stem, ext));
        if p.exists() {
            return Ok(KoeSource::File(p));
        }
    }

    let ovk = base.join(format!("z{:04}.ovk", scn_no));
    if ovk.exists() {
        return Ok(KoeSource::OvkEntryByNo {
            path: ovk,
            entry_no: koe_no_u32,
        });
    }

    bail!("koe resource not found: koe_no={koe_no}")
}
