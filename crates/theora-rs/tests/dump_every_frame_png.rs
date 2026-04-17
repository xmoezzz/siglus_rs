use std::env;
use std::fs::{self, File};
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};

use ogg::reading::PacketReader;
use png::{BitDepth, ColorType, Encoder};
use theora_rs::{
    th_decode_packetin, th_decode_ycbcr_out, HeaderParser, Info, OggPacket, PixelFmt, TheoraError,
    YCbCrBuffer,
};

#[test]
#[ignore = "requires THEORA_TEST_INPUT=/path/to/input.ogv"]
fn dump_every_frame_png() {
    let input = match env::var_os("THEORA_TEST_INPUT") {
        Some(v) => PathBuf::from(v),
        None => {
            eprintln!("THEORA_TEST_INPUT is not set; skipping");
            return;
        }
    };

    let output = env::var_os("THEORA_TEST_OUTPUT_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/theora-frame-dumps"));

    fs::create_dir_all(&output).expect("failed to create output directory");
    dump_theora_stream_to_pngs(&input, &output).expect("failed to dump frames to PNG");
}

fn dump_theora_stream_to_pngs(
    input: &Path,
    output_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open(input)?;
    let mut reader = PacketReader::new(BufReader::new(file));
    let mut parser = HeaderParser::new();
    let mut decoder = None;
    let mut packet_index: i64 = 0;
    let mut selected_serial: Option<u32> = None;
    let mut decoded_frames = 0usize;

    while let Some(pkt) = reader.read_packet()? {
        let is_bos = pkt.first_in_stream();
        let serial = pkt.stream_serial();
        let is_eos = pkt.last_in_stream();
        let granulepos = pkt.absgp_page() as i64;
        let data = pkt.data;

        if selected_serial.is_none() {
            if is_bos && looks_like_theora_ident_header(&data) {
                selected_serial = Some(serial);
            } else {
                continue;
            }
        }

        if Some(serial) != selected_serial {
            continue;
        }

        let op = OggPacket {
            packet: data,
            b_o_s: is_bos,
            e_o_s: is_eos,
            granulepos,
            packetno: packet_index,
        };
        packet_index += 1;

        if decoder.is_none() {
            let _ = parser.push(&op)?;
            if parser.is_ready() {
                decoder = Some(parser.decoder()?);
            }
            continue;
        }

        let dec = decoder
            .as_mut()
            .expect("decoder should have been allocated after headers");
        th_decode_packetin(dec, &op)?;
        let frame = match th_decode_ycbcr_out(dec) {
            Ok(frame) => frame,
            Err(TheoraError::NotImplemented) => {
                return Err(
                    "decoder accepted packet data, but frame output is still NotImplemented".into(),
                )
            }
            Err(err) => {
                return Err(format!(
                    "frame extraction failed after packet {}: {}",
                    packet_index, err
                )
                .into())
            }
        };

        let png_path = output_dir.join(format!("frame_{decoded_frames:06}.png"));
        write_frame_png(&frame, &parser.info, &png_path)?;
        decoded_frames += 1;
    }

    if selected_serial.is_none() {
        return Err("no Theora logical bitstream found in input".into());
    }
    if !parser.is_ready() {
        return Err("stream ended before all Theora headers were parsed".into());
    }
    if decoded_frames == 0 {
        return Err("no frames were decoded and dumped".into());
    }
    Ok(())
}

fn looks_like_theora_ident_header(packet: &[u8]) -> bool {
    packet.len() >= 7 && packet[0] == 0x80 && &packet[1..7] == b"theora"
}

fn write_frame_png(
    frame: &YCbCrBuffer,
    info: &Info,
    path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let width = visible_luma_width(info, frame[0].width) as u32;
    let height = visible_luma_height(info, frame[0].height) as u32;
    let rgb = ycbcr_to_rgb(frame, info)?;

    let file = File::create(path)?;
    let writer = BufWriter::new(file);
    let mut encoder = Encoder::new(writer, width, height);
    encoder.set_color(ColorType::Rgb);
    encoder.set_depth(BitDepth::Eight);
    let mut png_writer = encoder.write_header()?;
    png_writer.write_image_data(&rgb)?;
    Ok(())
}

fn ycbcr_to_rgb(frame: &YCbCrBuffer, info: &Info) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let y_plane = &frame[0];
    let cb_plane = &frame[1];
    let cr_plane = &frame[2];

    let crop_x = info.pic_x as usize;
    let crop_y = info.pic_y as usize;
    let width = visible_luma_width(info, y_plane.width);
    let height = visible_luma_height(info, y_plane.height);
    let (hdec, vdec) = chroma_decimation(info.pixel_fmt);

    let mut out = vec![0u8; width * height * 3];
    for y in 0..height {
        for x in 0..width {
            let yy = sample_plane(y_plane, crop_x + x, crop_y + y)? as i32;
            let cb = sample_plane(cb_plane, (crop_x + x) >> hdec, (crop_y + y) >> vdec)? as i32;
            let cr = sample_plane(cr_plane, (crop_x + x) >> hdec, (crop_y + y) >> vdec)? as i32;

            let (r, g, b) = ycbcr_pixel_to_rgb(yy, cb, cr);
            let dst = (y * width + x) * 3;
            out[dst] = r;
            out[dst + 1] = g;
            out[dst + 2] = b;
        }
    }
    Ok(out)
}

fn visible_luma_width(info: &Info, fallback: i32) -> usize {
    let width = if info.pic_width != 0 {
        info.pic_width as usize
    } else {
        fallback.max(1) as usize
    };
    width.max(1)
}

fn visible_luma_height(info: &Info, fallback: i32) -> usize {
    let height = if info.pic_height != 0 {
        info.pic_height as usize
    } else {
        fallback.max(1) as usize
    };
    height.max(1)
}

fn chroma_decimation(pixel_fmt: PixelFmt) -> (usize, usize) {
    match pixel_fmt {
        PixelFmt::Pf444 => (0, 0),
        PixelFmt::Pf422 => (1, 0),
        PixelFmt::Pf420 | PixelFmt::Reserved => (1, 1),
    }
}

fn ycbcr_pixel_to_rgb(y: i32, cb: i32, cr: i32) -> (u8, u8, u8) {
    let y = y.clamp(0, 255);
    let cb = cb.clamp(0, 255);
    let cr = cr.clamp(0, 255);

    let r = (1904000 * y + 2609823 * cr - 363703744) / 1635200;
    let g = (3827562 * y - 1287801 * cb - 2672387 * cr + 447306710) / 3287200;
    let b = (952000 * y + 1649289 * cb - 225932192) / 817600;

    (clamp_to_u8_i32(r), clamp_to_u8_i32(g), clamp_to_u8_i32(b))
}

fn sample_plane(
    plane: &theora_rs::ImgPlane,
    x: usize,
    y: usize,
) -> Result<u8, Box<dyn std::error::Error>> {
    let width = plane.width.max(1) as usize;
    let height = plane.height.max(1) as usize;
    let stride = plane.stride as isize;
    if x >= width || y >= height {
        return Err(
            format!("plane sample out of bounds: ({x}, {y}) not within {width}x{height}").into(),
        );
    }
    let idx = plane.data_offset as isize + y as isize * stride + x as isize;
    if idx < 0 {
        return Err(format!(
            "negative plane index: idx={} stride={} width={} height={}",
            idx, stride, width, height
        )
        .into());
    }
    let idx = idx as usize;
    plane.data.get(idx).copied().ok_or_else(|| {
        format!(
            "plane buffer too short: idx={} len={} stride={} width={} height={} offset={}",
            idx,
            plane.data.len(),
            stride,
            width,
            height,
            plane.data_offset
        )
        .into()
    })
}

fn clamp_to_u8_i32(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}
