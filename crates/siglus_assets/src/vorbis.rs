//! Ogg/Vorbis decoding helpers.
//!
//! Siglus resources commonly embed sound as Ogg/Vorbis (sometimes inside OVK or
//! with a simple XOR obfuscation (OWP)). This module provides a small,
//! format-oriented API that decodes to interleaved PCM16.

use anyhow::{anyhow, Result};
use std::io::{Read, Seek};

#[derive(Debug, Clone)]
pub struct Pcm16 {
    /// Number of audio channels.
    pub channels: u16,
    /// Sample rate (Hz).
    pub sample_rate: u32,
    /// Interleaved signed PCM16 samples.
    pub samples: Vec<i16>,
}

/// Decode an Ogg/Vorbis stream into interleaved PCM16.
///
/// This uses `lewton::inside_ogg::OggStreamReader`, which expects a *pure audio*
/// Ogg/Vorbis stream (i.e., not OGV with multiple streams). This matches the
/// typical Siglus usage (BGM/SE stored as .ogg/.owp or embedded in OVK).
pub fn decode_ogg_vorbis_reader<T: Read + Seek>(rdr: T) -> Result<Pcm16> {
    use lewton::inside_ogg::OggStreamReader;

    let mut r = OggStreamReader::new(rdr)
        .map_err(|e| anyhow!("ogg/vorbis: failed to parse headers: {e}"))?;

    let channels = r.ident_hdr.audio_channels as u16;
    let sample_rate = r.ident_hdr.audio_sample_rate;

    let mut samples = Vec::<i16>::new();
    while let Some(pkt) = r
        .read_dec_packet_itl()
        .map_err(|e| anyhow!("ogg/vorbis: decode error: {e}"))?
    {
        samples.extend_from_slice(&pkt);
    }

    Ok(Pcm16 {
        channels,
        sample_rate,
        samples,
    })
}

/// Decode an Ogg/Vorbis blob in memory.
pub fn decode_ogg_vorbis_bytes(data: &[u8]) -> Result<Pcm16> {
    decode_ogg_vorbis_reader(std::io::Cursor::new(data))
}

/// Encode PCM16 into a minimal RIFF/WAVE (PCM) buffer.
///
/// This is intended as a convenience for exporting assets.
pub fn pcm16_to_wav_bytes(pcm: &Pcm16) -> Vec<u8> {
    // RIFF/WAVE (PCM) header: 44 bytes.
    let num_channels = pcm.channels as u16;
    let sample_rate = pcm.sample_rate as u32;
    let bits_per_sample: u16 = 16;
    let block_align: u16 = num_channels.saturating_mul(bits_per_sample / 8);
    let byte_rate: u32 = sample_rate.saturating_mul(block_align as u32);

    let data_bytes_len: u32 = (pcm.samples.len() * 2) as u32;
    let riff_size: u32 = 36u32.saturating_add(data_bytes_len);

    let mut out = Vec::with_capacity(44 + data_bytes_len as usize);

    // RIFF header
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&riff_size.to_le_bytes());
    out.extend_from_slice(b"WAVE");

    // fmt chunk
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // PCM fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // format tag: PCM
    out.extend_from_slice(&num_channels.to_le_bytes());
    out.extend_from_slice(&sample_rate.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&bits_per_sample.to_le_bytes());

    // data chunk
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_bytes_len.to_le_bytes());

    // PCM payload
    for s in &pcm.samples {
        out.extend_from_slice(&s.to_le_bytes());
    }

    out
}

/// Decode Ogg/Vorbis bytes and immediately return WAV bytes.
pub fn decode_ogg_vorbis_bytes_to_wav(data: &[u8]) -> Result<Vec<u8>> {
    let pcm = decode_ogg_vorbis_bytes(data)?;
    Ok(pcm16_to_wav_bytes(&pcm))
}

/// Decode an Ogg/Vorbis stream and immediately return WAV bytes.
pub fn decode_ogg_vorbis_reader_to_wav<T: Read + Seek>(rdr: T) -> Result<Vec<u8>> {
    let pcm = decode_ogg_vorbis_reader(rdr)?;
    Ok(pcm16_to_wav_bytes(&pcm))
}
