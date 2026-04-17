use std::collections::BTreeMap;
use std::io::Cursor;

use anyhow::{anyhow, bail, Context, Result};
use lewton::audio::{read_audio_packet_generic, PreviousWindowRight};
use lewton::header::{
    read_header_comment, read_header_ident, read_header_setup, CommentHeader, SetupHeader,
};
use lewton::samples::InterleavedSamples;
use ogg::reading::PacketReader;
use theora_rs::{HeaderParser, OggPacket, PixelFmt};

pub const TH_PF_420: i32 = 0;
pub const TH_PF_422: i32 = 2;
pub const TH_PF_444: i32 = 3;

#[derive(Debug, Clone, Copy)]
pub struct VideoInfo {
    pub width: i32,
    pub height: i32,
    pub fps: f64,
    pub fmt: i32,
}

#[derive(Debug, Clone)]
struct OggPacketMeta {
    data: Vec<u8>,
    b_o_s: bool,
    e_o_s: bool,
    granulepos: i64,
    packetno: i64,
}

#[derive(Debug, Clone, Default)]
struct LogicalStream {
    packets: Vec<OggPacketMeta>,
}

pub struct TheoraFile {
    info: VideoInfo,
    video_frames: Vec<Vec<u8>>,
    video_cursor: usize,
    audio_channels: i32,
    audio_sample_rate: i32,
    has_audio_stream: bool,
    audio_samples: Vec<f32>,
    audio_cursor: usize,
}

impl TheoraFile {
    pub fn open_from_memory(data: Vec<u8>) -> Result<Self> {
        let streams = read_ogg_streams(data).context("parse ogg packets")?;
        let video_serial = find_stream_by_magic(&streams, b"theora", 0x80)
            .ok_or_else(|| anyhow!("no video stream in ogg"))?;
        let audio_serial = find_stream_by_magic(&streams, b"vorbis", 0x01);

        let (info, video_frames) = decode_theora_stream(
            streams
                .get(&video_serial)
                .ok_or_else(|| anyhow!("selected Theora stream missing"))?,
        )
        .context("decode theora stream")?;

        let (has_audio_stream, audio_channels, audio_sample_rate, audio_samples) =
            if let Some(serial) = audio_serial {
                let (channels, sample_rate, samples) = decode_vorbis_stream(
                    streams
                        .get(&serial)
                        .ok_or_else(|| anyhow!("selected Vorbis stream missing"))?,
                )
                .context("decode vorbis stream")?;
                (true, channels, sample_rate, samples)
            } else {
                (false, 0, 0, Vec::new())
            };

        Ok(Self {
            info,
            video_frames,
            video_cursor: 0,
            audio_channels,
            audio_sample_rate,
            has_audio_stream,
            audio_samples,
            audio_cursor: 0,
        })
    }

    pub fn info(&self) -> VideoInfo {
        self.info
    }

    pub fn has_audio(&self) -> bool {
        self.has_audio_stream
    }

    pub fn audio_info(&self) -> Option<(i32, i32)> {
        if !self.has_audio_stream || self.audio_channels <= 0 || self.audio_sample_rate <= 0 {
            return None;
        }
        Some((self.audio_channels, self.audio_sample_rate))
    }

    pub fn reset(&mut self) {
        self.video_cursor = 0;
        self.audio_cursor = 0;
    }

    pub fn read_video_frame(&mut self, out: &mut [u8]) -> Result<bool> {
        let Some(frame) = self.video_frames.get(self.video_cursor) else {
            return Ok(false);
        };
        if out.len() < frame.len() {
            bail!(
                "video output buffer too small: need {} bytes, got {} bytes",
                frame.len(),
                out.len()
            );
        }
        out[..frame.len()].copy_from_slice(frame);
        self.video_cursor += 1;
        Ok(true)
    }

    pub fn read_audio_samples(&mut self, out: &mut [f32]) -> Result<usize> {
        if out.is_empty() || self.audio_cursor >= self.audio_samples.len() {
            return Ok(0);
        }
        let remaining = self.audio_samples.len() - self.audio_cursor;
        let count = remaining.min(out.len());
        out[..count]
            .copy_from_slice(&self.audio_samples[self.audio_cursor..self.audio_cursor + count]);
        self.audio_cursor += count;
        Ok(count)
    }
}

fn read_ogg_streams(data: Vec<u8>) -> Result<BTreeMap<u32, LogicalStream>> {
    let mut reader = PacketReader::new(Cursor::new(data));
    let mut streams = BTreeMap::<u32, LogicalStream>::new();
    let mut packetno_by_serial = BTreeMap::<u32, i64>::new();

    while let Some(pkt) = reader.read_packet().context("ogg packet read")? {
        let serial = pkt.stream_serial();
        let b_o_s = pkt.first_in_stream();
        let e_o_s = pkt.last_in_stream();
        let granulepos = pkt.absgp_page() as i64;
        let data = pkt.data;
        let packetno = packetno_by_serial.entry(serial).or_insert(0);
        let stream = streams.entry(serial).or_default();
        stream.packets.push(OggPacketMeta {
            data,
            b_o_s,
            e_o_s,
            granulepos,
            packetno: *packetno,
        });
        *packetno += 1;
    }

    Ok(streams)
}

fn find_stream_by_magic(
    streams: &BTreeMap<u32, LogicalStream>,
    magic: &[u8; 6],
    marker: u8,
) -> Option<u32> {
    streams.iter().find_map(|(&serial, stream)| {
        let first = stream.packets.first()?;
        if first.b_o_s
            && first.data.len() >= 7
            && first.data[0] == marker
            && &first.data[1..7] == magic
        {
            Some(serial)
        } else {
            None
        }
    })
}

fn decode_theora_stream(stream: &LogicalStream) -> Result<(VideoInfo, Vec<Vec<u8>>)> {
    let mut parser = HeaderParser::new();
    let mut decoder = None;
    let mut frames = Vec::<Vec<u8>>::new();

    for pkt in &stream.packets {
        let op = OggPacket {
            packet: pkt.data.clone(),
            b_o_s: pkt.b_o_s,
            e_o_s: pkt.e_o_s,
            granulepos: pkt.granulepos,
            packetno: pkt.packetno,
        };

        if decoder.is_none() {
            let _ = parser.push(&op)?;
            if parser.is_ready() {
                decoder = Some(parser.decoder()?);
            }
            continue;
        }

        let dec = decoder.as_mut().expect("decoder allocated after headers");
        theora_rs::th_decode_packetin(dec, &op)?;
        if dec.has_decoded_frame() {
            let ycbcr = theora_rs::th_decode_ycbcr_out(dec)?;
            frames.push(pack_theorafile_frame(&parser.info, &ycbcr)?);
        }
    }

    if !parser.is_ready() {
        bail!("stream ended before all Theora headers were parsed");
    }

    let info = &parser.info;
    let width = info.pic_width.max(1) as i32;
    let height = info.pic_height.max(1) as i32;
    let fps = if info.fps_denominator != 0 {
        info.fps_numerator as f64 / info.fps_denominator as f64
    } else {
        0.0
    };
    let fmt = match info.pixel_fmt {
        PixelFmt::Pf420 | PixelFmt::Reserved => TH_PF_420,
        PixelFmt::Pf422 => TH_PF_422,
        PixelFmt::Pf444 => TH_PF_444,
    };

    Ok((
        VideoInfo {
            width,
            height,
            fps,
            fmt,
        },
        frames,
    ))
}

fn pack_theorafile_frame(
    info: &theora_rs::Info,
    ycbcr: &theora_rs::YCbCrBuffer,
) -> Result<Vec<u8>> {
    let mut out = Vec::new();

    let y_w = info.pic_width.max(1) as usize;
    let y_h = info.pic_height.max(1) as usize;
    copy_visible_plane(
        &mut out,
        &ycbcr[0],
        (info.pic_x & !1) as usize,
        (info.pic_y & !1) as usize,
        y_w,
        y_h,
    )?;

    let mut uv_w = y_w;
    let mut uv_h = y_h;
    let uv_x;
    let uv_y;
    match info.pixel_fmt {
        PixelFmt::Pf420 | PixelFmt::Reserved => {
            uv_w /= 2;
            uv_h /= 2;
            uv_x = (info.pic_x / 2) as usize;
            uv_y = (info.pic_y / 2) as usize;
        }
        PixelFmt::Pf422 => {
            uv_w /= 2;
            uv_x = (info.pic_x / 2) as usize;
            uv_y = (info.pic_y & !1) as usize;
        }
        PixelFmt::Pf444 => {
            uv_x = info.pic_x as usize;
            uv_y = info.pic_y as usize;
        }
    }

    copy_visible_plane(&mut out, &ycbcr[1], uv_x, uv_y, uv_w, uv_h)?;
    copy_visible_plane(&mut out, &ycbcr[2], uv_x, uv_y, uv_w, uv_h)?;
    Ok(out)
}

fn copy_visible_plane(
    dst: &mut Vec<u8>,
    plane: &theora_rs::ImgPlane,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
) -> Result<()> {
    let stride = plane.stride.max(0) as usize;
    let base = plane.data_offset;
    let needed = width.saturating_mul(height);
    dst.reserve(needed);
    for row in 0..height {
        let start = base
            .saturating_add((y + row).saturating_mul(stride))
            .saturating_add(x);
        let end = start.saturating_add(width);
        if end > plane.data.len() {
            bail!(
                "plane copy out of range: start={} end={} len={} stride={} width={} height={} offset={}",
                start,
                end,
                plane.data.len(),
                plane.stride,
                width,
                height,
                plane.data_offset
            );
        }
        dst.extend_from_slice(&plane.data[start..end]);
    }
    Ok(())
}

fn decode_vorbis_stream(stream: &LogicalStream) -> Result<(i32, i32, Vec<f32>)> {
    if stream.packets.len() < 3 {
        bail!("vorbis stream does not contain enough header packets");
    }

    let ident = read_header_ident(&stream.packets[0].data).context("read vorbis ident header")?;
    let _comment: CommentHeader =
        read_header_comment(&stream.packets[1].data).context("read vorbis comment header")?;
    let setup: SetupHeader = read_header_setup(
        &stream.packets[2].data,
        ident.audio_channels,
        (ident.blocksize_0, ident.blocksize_1),
    )
    .context("read vorbis setup header")?;

    let mut pwr = PreviousWindowRight::new();
    let mut samples = Vec::<f32>::new();
    for pkt in &stream.packets[3..] {
        let decoded: InterleavedSamples<f32> =
            read_audio_packet_generic(&ident, &setup, &pkt.data, &mut pwr)
                .context("decode vorbis audio packet")?;
        samples.extend_from_slice(&decoded.samples);
    }

    Ok((
        ident.audio_channels as i32,
        ident.audio_sample_rate as i32,
        samples,
    ))
}
