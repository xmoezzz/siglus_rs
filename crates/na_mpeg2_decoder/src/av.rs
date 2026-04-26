use std::collections::VecDeque;
use std::sync::Arc;

use crate::audio::{Ac3AudioChunk, Ac3AudioDecoder, MpaAudioChunk, MpaAudioDecoder};
use crate::convert::frame_to_rgba_bt601_limited;
use crate::demux::{Demuxer, Packet, StreamType};
use crate::error::Result;
use crate::video::{Decoder as VideoDecoder, Frame};

#[derive(Clone)]
pub struct MpegRgbaFrame {
    pub pts_ms: i64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone)]
pub struct MpegAudioF32 {
    pub pts_ms: i64,
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

#[derive(Clone)]
pub enum MpegAvEvent {
    Video(MpegRgbaFrame),
    Audio(MpegAudioF32),
}

#[derive(Default)]
pub struct MpegAvPipeline {
    demux: Demuxer,
    vdec: VideoDecoder,
    adec: MpaAudioDecoder,
    ac3dec: Ac3AudioDecoder,

    pkts: Vec<Packet>,
    pub stash: VecDeque<MpegAvEvent>,
}

impl MpegAvPipeline {
    pub fn new() -> Self {
        Self {
            demux: Demuxer::new_auto(),
            vdec: VideoDecoder::new(),
            adec: MpaAudioDecoder::new(),
            ac3dec: Ac3AudioDecoder::new(),
            pkts: Vec::new(),
            stash: VecDeque::new(),
        }
    }

    #[inline]
    pub fn demuxer_mut(&mut self) -> &mut Demuxer {
        &mut self.demux
    }

    #[inline]
    pub fn video_decoder_mut(&mut self) -> &mut VideoDecoder {
        &mut self.vdec
    }

    #[inline]
    pub fn audio_decoder_mut(&mut self) -> &mut MpaAudioDecoder {
        &mut self.adec
    }

    #[inline]
    pub fn ac3_audio_decoder_mut(&mut self) -> &mut Ac3AudioDecoder {
        &mut self.ac3dec
    }

    pub fn push_with<F>(&mut self, data: &[u8], pts_90k: Option<i64>, mut on_event: F) -> Result<()>
    where
        F: FnMut(MpegAvEvent),
    {
        self.pkts.clear();
        self.demux.push_into(data, pts_90k, &mut self.pkts);

        // Move packets out to avoid borrowing self.pkts while calling &mut self handlers.
        let mut local_pkts: Vec<Packet> = Vec::new();
        std::mem::swap(&mut self.pkts, &mut local_pkts);

        for pkt in local_pkts.drain(..) {
            match pkt.stream_type {
                StreamType::MpegVideo => self.handle_video_pkt(&pkt, &mut on_event)?,
                StreamType::MpegAudio => self.handle_audio_pkt(&pkt, &mut on_event)?,
                StreamType::DvdLpcmAudio => self.handle_dvd_private_audio_pkt(&pkt, &mut on_event)?,
                StreamType::Unknown => {}
            }
        }

        std::mem::swap(&mut self.pkts, &mut local_pkts);
        self.pkts.clear();

        Ok(())
    }

    pub fn push(&mut self, data: &[u8], pts_90k: Option<i64>) -> Result<()> {
        let mut tmp: Vec<MpegAvEvent> = Vec::new();
        self.push_with(data, pts_90k, |ev| tmp.push(ev))?;
        for ev in tmp {
            self.stash.push_back(ev);
        }
        Ok(())
    }

    pub fn flush_with<F>(&mut self, mut on_event: F) -> Result<()>
    where
        F: FnMut(MpegAvEvent),
    {
        // Video: flush delayed frames.
        for f in self.vdec.flush_shared()? {
            self.emit_video_frame(f, &mut on_event)?;
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<()> {
        let mut tmp: Vec<MpegAvEvent> = Vec::new();
        self.flush_with(|ev| tmp.push(ev))?;
        for ev in tmp {
            self.stash.push_back(ev);
        }
        Ok(())
    }

    fn handle_video_pkt<F>(&mut self, pkt: &Packet, on_event: &mut F) -> Result<()>
    where
        F: FnMut(MpegAvEvent),
    {
        let decoded: Vec<Arc<Frame>> = self.vdec.decode_shared(&pkt.data, pkt.pts_90k)?;
        for f in decoded {
            self.emit_video_frame(f, on_event)?;
        }
        Ok(())
    }

    fn emit_video_frame<F>(&mut self, f: Arc<Frame>, on_event: &mut F) -> Result<()>
    where
        F: FnMut(MpegAvEvent),
    {
        let w = f.width as u32;
        let h = f.height as u32;
        let mut rgba = vec![0u8; (w as usize) * (h as usize) * 4];
        frame_to_rgba_bt601_limited(&f, &mut rgba);

        let pts_ms = pts90k_opt_to_ms(f.pts_90k);
        on_event(MpegAvEvent::Video(MpegRgbaFrame {
            pts_ms,
            width: w,
            height: h,
            rgba,
        }));
        Ok(())
    }

    fn handle_audio_pkt<F>(&mut self, pkt: &Packet, on_event: &mut F) -> Result<()>
    where
        F: FnMut(MpegAvEvent),
    {
        let pts_ms_opt = pkt.pts_90k.map(pts90k_to_ms);
        let audio_result = self
            .adec
            .push_with(&pkt.data, pts_ms_opt, |ch: MpaAudioChunk| {
                on_event(MpegAvEvent::Audio(MpegAudioF32 {
                    pts_ms: ch.pts_ms,
                    sample_rate: ch.sample_rate,
                    channels: ch.channels,
                    samples: ch.samples,
                }))
            });
        if let Err(err) = audio_result {
            if std::env::var_os("SG_MOVIE_TRACE").is_some()
                || std::env::var_os("SG_DEBUG").is_some()
            {
                eprintln!("[SG_DEBUG][MOV] mpa.audio_packet.drop: {err}");
            }
        }
        Ok(())
    }

    fn handle_dvd_private_audio_pkt<F>(&mut self, pkt: &Packet, on_event: &mut F) -> Result<()>
    where
        F: FnMut(MpegAvEvent),
    {
        let pts_ms = pts90k_opt_to_ms(pkt.pts_90k);
        let Some((&substream_id, rest)) = pkt.data.split_first() else {
            return Ok(());
        };

        if (0x80..=0x87).contains(&substream_id) {
            let payload = if rest.len() >= 3 { &rest[3..] } else { rest };
            let audio_result = self.ac3dec.push_with(payload, Some(pts_ms), |ch: Ac3AudioChunk| {
                on_event(MpegAvEvent::Audio(MpegAudioF32 {
                    pts_ms: ch.pts_ms,
                    sample_rate: ch.sample_rate,
                    channels: ch.channels,
                    samples: ch.samples,
                }))
            });
            if let Err(err) = audio_result {
                if std::env::var_os("SG_MOVIE_TRACE").is_some()
                    || std::env::var_os("SG_DEBUG").is_some()
                {
                    eprintln!("[SG_DEBUG][MOV] ac3.audio_packet.drop: {err}");
                }
            }
            return Ok(());
        }

        if (0xA0..=0xAF).contains(&substream_id) {
            if let Some(audio) = decode_dvd_private_lpcm(&pkt.data, pts_ms) {
                on_event(MpegAvEvent::Audio(audio));
            }
            return Ok(());
        }

        if std::env::var_os("SG_MOVIE_TRACE").is_some()
            || std::env::var_os("SG_DEBUG").is_some()
        {
            eprintln!(
                "[SG_DEBUG][MOV] dvd_private_audio.unsupported substream=0x{substream_id:02x} bytes={}",
                pkt.data.len()
            );
        }
        Ok(())
    }
}

#[inline]
fn pts90k_to_ms(v: i64) -> i64 {
    (v * 1000) / 90000
}

#[inline]
fn pts90k_opt_to_ms(v: Option<i64>) -> i64 {
    v.map(pts90k_to_ms).unwrap_or(0)
}

fn decode_dvd_private_lpcm(data: &[u8], pts_ms: i64) -> Option<MpegAudioF32> {
    if data.len() < 8 {
        return None;
    }
    let substream_id = data[0];
    if !(0xA0..=0xAF).contains(&substream_id) {
        if (std::env::var_os("SG_MOVIE_TRACE").is_some()
            || std::env::var_os("SG_DEBUG").is_some())
            && ((0x80..=0x8F).contains(&substream_id) || (0x90..=0x9F).contains(&substream_id))
        {
            eprintln!(
                "[SG_DEBUG][MOV] dvd_private_audio.unsupported substream=0x{substream_id:02x} bytes={}",
                data.len()
            );
        }
        return None;
    }

    // DVD private_stream_1 audio packets carry a substream id followed by a
    // 3-byte private-stream header.  The following LPCM payload begins with
    // the 3-byte DVD LPCM audio header.
    let lpcm = &data[4..];
    if lpcm.len() < 4 {
        return None;
    }

    let format = lpcm[1];
    let bits_code = (format >> 6) & 0x03;
    let rate_code = (format >> 4) & 0x03;
    let channels = ((format & 0x07) + 1) as u16;
    let sample_rate = match rate_code {
        0 => 48_000,
        1 => 96_000,
        2 => 44_100,
        3 => 32_000,
        _ => return None,
    };
    let bits_per_sample = match bits_code {
        0 => 16,
        1 => 20,
        2 => 24,
        _ => return None,
    };

    let pcm = &lpcm[3..];
    let mut samples = Vec::new();
    match bits_per_sample {
        16 => {
            let sample_count = pcm.len() / 2;
            samples.reserve(sample_count);
            for chunk in pcm.chunks_exact(2) {
                let v = i16::from_be_bytes([chunk[0], chunk[1]]) as f32 / 32768.0;
                samples.push(v.clamp(-1.0, 1.0));
            }
        }
        24 => {
            let sample_count = pcm.len() / 3;
            samples.reserve(sample_count);
            for chunk in pcm.chunks_exact(3) {
                let raw = ((chunk[0] as i32) << 24) | ((chunk[1] as i32) << 16) | ((chunk[2] as i32) << 8);
                let v = raw as f32 / 2_147_483_648.0;
                samples.push(v.clamp(-1.0, 1.0));
            }
        }
        20 => {
            if std::env::var_os("SG_MOVIE_TRACE").is_some()
                || std::env::var_os("SG_DEBUG").is_some()
            {
                eprintln!(
                    "[SG_DEBUG][MOV] dvd_lpcm_20bit.unsupported substream=0x{substream_id:02x} rate={} channels={} bytes={}",
                    sample_rate,
                    channels,
                    pcm.len()
                );
            }
            return None;
        }
        _ => return None,
    }

    if samples.is_empty() || channels == 0 {
        return None;
    }

    Some(MpegAudioF32 {
        pts_ms,
        sample_rate,
        channels,
        samples,
    })
}
