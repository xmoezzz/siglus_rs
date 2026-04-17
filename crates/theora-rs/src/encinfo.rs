use crate::codec::{
    Comment, HuffCode, Info, QuantInfo, TH_NDCT_TOKENS, TH_NHUFFMAN_TABLES, TH_VERSION_MAJOR,
    TH_VERSION_MINOR, TH_VERSION_SUB,
};
use crate::enquant::oc_quant_params_pack;
use crate::error::{Result, TheoraError};
use crate::huffenc::oc_huff_codes_pack;
use crate::packet::{OggPacket, PackWriter};

pub const OC_PACKET_EMPTY: i32 = 0;
pub const OC_PACKET_READY: i32 = 1;
pub const OC_PACKET_INFO_HDR: i32 = -3;
pub const OC_PACKET_COMMENT_HDR: i32 = -2;
pub const OC_PACKET_SETUP_HDR: i32 = -1;

fn comment_header(tc: &Comment, vendor: &str) -> OggPacket {
    let mut w = PackWriter::new();
    w.write(0x81, 8);
    w.write_octets(b"theora");
    w.write_le_u32(vendor.len() as u32);
    w.write_octets(vendor.as_bytes());
    w.write_le_u32(tc.user_comments.len() as u32);
    for comment in &tc.user_comments {
        w.write_le_u32(comment.len() as u32);
        w.write_octets(comment);
    }
    OggPacket {
        packet: w.finish(),
        b_o_s: false,
        e_o_s: false,
        granulepos: 0,
        packetno: 1,
    }
}

fn info_header(info: &Info) -> OggPacket {
    let mut w = PackWriter::new();
    w.write(0x80, 8);
    w.write_octets(b"theora");
    w.write(TH_VERSION_MAJOR as u32, 8);
    w.write(TH_VERSION_MINOR as u32, 8);
    w.write(TH_VERSION_SUB as u32, 8);
    w.write(info.frame_width >> 4, 16);
    w.write(info.frame_height >> 4, 16);
    w.write(info.pic_width, 24);
    w.write(info.pic_height, 24);
    w.write(info.pic_x, 8);
    w.write(info.pic_y, 8);
    w.write(info.fps_numerator, 32);
    w.write(info.fps_denominator, 32);
    w.write(info.aspect_numerator, 24);
    w.write(info.aspect_denominator, 24);
    w.write(info.colorspace as u32, 8);
    w.write((info.target_bitrate as u32) & 0x00FF_FFFF, 24);
    w.write((info.quality as u32) & 0x3F, 6);
    w.write((info.keyframe_granule_shift as u32) & 0x1F, 5);
    w.write(info.pixel_fmt as u32, 2);
    w.write(0, 3);
    OggPacket {
        packet: w.finish(),
        b_o_s: true,
        e_o_s: false,
        granulepos: 0,
        packetno: 0,
    }
}

pub fn oc_state_flushheader(
    info: Option<&Info>,
    packet_state: &mut i32,
    qinfo: Option<&QuantInfo>,
    _codes: Option<&[[HuffCode; TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES]>,
    vendor: &str,
    tc: Option<&Comment>,
) -> Result<Option<OggPacket>> {
    let packet = match *packet_state {
        OC_PACKET_INFO_HDR => {
            let info = info.ok_or(TheoraError::Fault)?;
            info_header(info)
        }
        OC_PACKET_COMMENT_HDR => {
            let tc = tc.ok_or(TheoraError::Fault)?;
            comment_header(tc, vendor)
        }
        OC_PACKET_SETUP_HDR => {
            let qinfo = qinfo.ok_or(TheoraError::Fault)?;
            let codes = _codes.ok_or(TheoraError::Fault)?;
            let mut w = PackWriter::new();
            w.write(0x82, 8);
            w.write_octets(b"theora");
            oc_quant_params_pack(&mut w, qinfo);
            oc_huff_codes_pack(&mut w, codes)?;
            OggPacket {
                packet: w.finish(),
                b_o_s: false,
                e_o_s: false,
                granulepos: 0,
                packetno: 2,
            }
        }
        _ => return Ok(None),
    };
    *packet_state += 1;
    Ok(Some(packet))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::info::th_comment_add_tag;

    #[test]
    fn info_header_has_expected_prefix() {
        let info = Info::default();
        let mut state = OC_PACKET_INFO_HDR;
        let packet = oc_state_flushheader(Some(&info), &mut state, None, None, "vendor", None)
            .unwrap()
            .unwrap();
        assert_eq!(&packet.packet[..7], b"\x80theora");
        assert!(packet.b_o_s);
    }

    #[test]
    fn comment_header_starts_with_marker() {
        let mut comment = Comment::default();
        th_comment_add_tag(&mut comment, "ARTIST", "Xiph");
        let mut state = OC_PACKET_COMMENT_HDR;
        let packet = oc_state_flushheader(None, &mut state, None, None, "vendor", Some(&comment))
            .unwrap()
            .unwrap();
        assert_eq!(&packet.packet[..7], b"\x81theora");
    }
}
