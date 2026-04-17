use crate::bitpack::PackBuf;
use crate::codec::{Comment, Info, QuantInfo, TH_VERSION_MAJOR, TH_VERSION_MINOR};
use crate::dequant::oc_quant_params_unpack;
use crate::error::{Result, TheoraError};
use crate::huffdec::huff_trees_unpack;
use crate::packet::OggPacket;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetupInfo {
    pub huff_tables: Vec<Vec<i16>>,
    pub qinfo: QuantInfo,
}

fn unpack_octets(opb: &mut PackBuf<'_>, len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    for b in &mut out {
        *b = opb.read(8) as u8;
    }
    out
}

fn unpack_length(opb: &mut PackBuf<'_>) -> i32 {
    let b0 = opb.read(8) as i32;
    let b1 = opb.read(8) as i32;
    let b2 = opb.read(8) as i32;
    let b3 = opb.read(8) as i32;
    b0 | (b1 << 8) | (b2 << 16) | (b3 << 24)
}

pub fn oc_info_unpack(opb: &mut PackBuf<'_>, info: &mut Info) -> Result<()> {
    info.version_major = opb.read(8) as u8;
    info.version_minor = opb.read(8) as u8;
    info.version_subminor = opb.read(8) as u8;
    if info.version_major > TH_VERSION_MAJOR
        || (info.version_major == TH_VERSION_MAJOR && info.version_minor > TH_VERSION_MINOR)
    {
        return Err(TheoraError::Version);
    }

    info.frame_width = (opb.read(16) as u32) << 4;
    info.frame_height = (opb.read(16) as u32) << 4;
    info.pic_width = opb.read(24) as u32;
    info.pic_height = opb.read(24) as u32;
    info.pic_x = opb.read(8) as u32;
    info.pic_y = opb.read(8) as u32;
    info.fps_numerator = opb.read(32) as u32;
    info.fps_denominator = opb.read(32) as u32;

    if info.frame_width == 0
        || info.frame_height == 0
        || info.pic_width + info.pic_x > info.frame_width
        || info.pic_height + info.pic_y > info.frame_height
        || info.fps_numerator == 0
        || info.fps_denominator == 0
    {
        return Err(TheoraError::BadHeader);
    }

    info.pic_y = info.frame_height - info.pic_height - info.pic_y;
    info.aspect_numerator = opb.read(24) as u32;
    info.aspect_denominator = opb.read(24) as u32;
    info.colorspace = match opb.read(8) as i32 {
        1 => crate::codec::Colorspace::ItuRec470M,
        2 => crate::codec::Colorspace::ItuRec470Bg,
        _ => crate::codec::Colorspace::Unspecified,
    };
    info.target_bitrate = opb.read(24) as i32;
    info.quality = opb.read(6) as i32;
    info.keyframe_granule_shift = opb.read(5) as i32;
    info.pixel_fmt = match opb.read(2) as i32 {
        0 => crate::codec::PixelFmt::Pf420,
        2 => crate::codec::PixelFmt::Pf422,
        3 => crate::codec::PixelFmt::Pf444,
        _ => return Err(TheoraError::BadHeader),
    };
    let spare = opb.read(3);
    if spare != 0 || opb.bytes_left() < 0 {
        return Err(TheoraError::BadHeader);
    }
    Ok(())
}

pub fn oc_comment_unpack(opb: &mut PackBuf<'_>, tc: &mut Comment) -> Result<()> {
    let len = unpack_length(opb);
    if len < 0 || len as isize > opb.bytes_left() {
        return Err(TheoraError::BadHeader);
    }
    tc.vendor = unpack_octets(opb, len as usize);

    let comments = unpack_length(opb);
    if comments < 0 || ((comments as usize) << 2) as isize > opb.bytes_left() {
        return Err(TheoraError::BadHeader);
    }
    tc.user_comments.clear();
    for _ in 0..comments {
        let len = unpack_length(opb);
        if len < 0 || len as isize > opb.bytes_left() {
            return Err(TheoraError::BadHeader);
        }
        tc.user_comments.push(unpack_octets(opb, len as usize));
    }
    if opb.bytes_left() < 0 {
        return Err(TheoraError::BadHeader);
    }
    Ok(())
}

pub fn oc_setup_unpack(opb: &mut PackBuf<'_>, setup: &mut SetupInfo) -> Result<()> {
    oc_quant_params_unpack(opb, &mut setup.qinfo)?;
    setup.huff_tables = huff_trees_unpack(opb)?;
    Ok(())
}

pub fn th_decode_headerin(
    info: &mut Info,
    comment: Option<&mut Comment>,
    setup: Option<&mut Option<SetupInfo>>,
    op: &OggPacket,
) -> Result<i32> {
    let mut opb = PackBuf::new(&op.packet);
    let packtype = opb.read(8) as i32;

    if (packtype & 0x80) == 0 {
        if info.frame_width == 0 {
            return Err(TheoraError::NotFormat);
        }
        let tc = comment.ok_or(TheoraError::Fault)?;
        if tc.vendor.is_empty() {
            return Err(TheoraError::BadHeader);
        }
        let st = setup.ok_or(TheoraError::Fault)?;
        if st.is_none() {
            return Err(TheoraError::BadHeader);
        }
        return Ok(0);
    }

    let sig = unpack_octets(&mut opb, 6);
    if sig.as_slice() != b"theora" {
        return Err(TheoraError::NotFormat);
    }

    match packtype {
        0x80 => {
            if !op.b_o_s || info.frame_width > 0 {
                return Err(TheoraError::BadHeader);
            }
            oc_info_unpack(&mut opb, info)?;
            Ok(3)
        }
        0x81 => {
            let tc = comment.ok_or(TheoraError::Fault)?;
            if info.frame_width == 0 || !tc.vendor.is_empty() {
                return Err(TheoraError::BadHeader);
            }
            oc_comment_unpack(&mut opb, tc)?;
            Ok(2)
        }
        0x82 => {
            let tc = comment.ok_or(TheoraError::Fault)?;
            let st = setup.ok_or(TheoraError::Fault)?;
            if info.frame_width == 0 || tc.vendor.is_empty() || st.is_some() {
                return Err(TheoraError::BadHeader);
            }
            let mut tmp = SetupInfo::default();
            oc_setup_unpack(&mut opb, &mut tmp)?;
            *st = Some(tmp);
            Ok(1)
        }
        _ => Err(TheoraError::BadHeader),
    }
}

pub fn th_setup_free(setup: &mut Option<SetupInfo>) {
    *setup = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info::{th_comment_add_tag, th_info_init};

    #[test]
    fn data_packet_before_headers_is_not_format() {
        let mut info = Info::default();
        let mut comment = Comment::default();
        let mut setup = None;
        let pkt = OggPacket::new(vec![0]);
        let err =
            th_decode_headerin(&mut info, Some(&mut comment), Some(&mut setup), &pkt).unwrap_err();
        assert_eq!(err, TheoraError::NotFormat);
    }

    #[test]
    fn header_detection_rejects_wrong_signature() {
        let mut info = Info::default();
        let pkt = OggPacket::with_bos(b"\x80notora".to_vec());
        let err = th_decode_headerin(&mut info, None, None, &pkt).unwrap_err();
        assert_eq!(err, TheoraError::NotFormat);
    }

    #[test]
    fn data_packet_after_headers_requires_setup() {
        let mut info = Info::default();
        th_info_init(&mut info);
        info.frame_width = 16;
        let mut comment = Comment::default();
        th_comment_add_tag(&mut comment, "ARTIST", "Xiph");
        let pkt = OggPacket::new(vec![0]);
        let mut setup = None;
        let err =
            th_decode_headerin(&mut info, Some(&mut comment), Some(&mut setup), &pkt).unwrap_err();
        assert_eq!(err, TheoraError::BadHeader);
    }
}
