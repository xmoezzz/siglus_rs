use crate::codec::{
    Colorspace, ImgPlane, Info, LegacyTheoraInfo, PixelFmt, YCbCrBuffer, OC_VENDOR_STRING,
    TH_VERSION_MAJOR, TH_VERSION_MINOR, TH_VERSION_SUB,
};
use crate::mathops::oc_ilog;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Packet<'a> {
    pub packet: &'a [u8],
}

impl<'a> Packet<'a> {
    pub fn new(packet: &'a [u8]) -> Self {
        Self { packet }
    }

    pub fn len(&self) -> usize {
        self.packet.len()
    }

    pub fn is_empty(&self) -> bool {
        self.packet.is_empty()
    }
}

pub fn th_version_string() -> &'static str {
    OC_VENDOR_STRING
}

pub fn th_version_number() -> u32 {
    ((TH_VERSION_MAJOR as u32) << 16) + ((TH_VERSION_MINOR as u32) << 8) + TH_VERSION_SUB as u32
}

pub fn th_packet_isheader(op: Packet<'_>) -> bool {
    if op.packet.is_empty() {
        false
    } else {
        (op.packet[0] >> 7) != 0
    }
}

pub fn th_packet_iskeyframe(op: Packet<'_>) -> i32 {
    if op.packet.is_empty() {
        0
    } else if (op.packet[0] & 0x80) != 0 {
        -1
    } else if (op.packet[0] & 0x40) == 0 {
        1
    } else {
        0
    }
}

pub fn oc_ycbcr_buffer_flip(dst: &mut YCbCrBuffer, src: &YCbCrBuffer) {
    for pli in 0..3 {
        let height = src[pli].height;
        let stride = -src[pli].stride;
        let offset = src[pli].data_offset as isize + ((1 - height) as isize) * (stride as isize);
        assert!(
            offset >= 0,
            "oc_ycbcr_buffer_flip produced a negative data offset"
        );
        dst[pli] = ImgPlane {
            width: src[pli].width,
            height,
            stride,
            data: src[pli].data.clone(),
            data_offset: offset as usize,
        };
    }
}

pub fn oc_theora_info2th_info(ci: &LegacyTheoraInfo) -> Info {
    Info {
        version_major: ci.version_major,
        version_minor: ci.version_minor,
        version_subminor: ci.version_subminor,
        frame_width: ci.width,
        frame_height: ci.height,
        pic_width: ci.frame_width,
        pic_height: ci.frame_height,
        pic_x: ci.offset_x,
        pic_y: ci.offset_y,
        fps_numerator: ci.fps_numerator,
        fps_denominator: ci.fps_denominator,
        aspect_numerator: ci.aspect_numerator,
        aspect_denominator: ci.aspect_denominator,
        colorspace: match ci.colorspace {
            Colorspace::ItuRec470M => Colorspace::ItuRec470M,
            Colorspace::ItuRec470Bg => Colorspace::ItuRec470Bg,
            Colorspace::Unspecified => Colorspace::Unspecified,
        },
        pixel_fmt: match ci.pixelformat {
            PixelFmt::Pf420 => PixelFmt::Pf420,
            PixelFmt::Pf422 => PixelFmt::Pf422,
            PixelFmt::Pf444 => PixelFmt::Pf444,
            PixelFmt::Reserved => PixelFmt::Reserved,
        },
        target_bitrate: ci.target_bitrate,
        quality: ci.quality,
        keyframe_granule_shift: if ci.keyframe_frequency_force > 0 {
            oc_ilog(ci.keyframe_frequency_force - 1).min(31)
        } else {
            0
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_number_matches_expected_layout() {
        assert_eq!(th_version_number(), 0x0003_0201);
    }

    #[test]
    fn packet_type_helpers_match_c_logic() {
        assert!(!th_packet_isheader(Packet::new(&[])));
        assert_eq!(th_packet_iskeyframe(Packet::new(&[])), 0);
        assert!(th_packet_isheader(Packet::new(&[0x80])));
        assert_eq!(th_packet_iskeyframe(Packet::new(&[0x80])), -1);
        assert_eq!(th_packet_iskeyframe(Packet::new(&[0x00])), 1);
        assert_eq!(th_packet_iskeyframe(Packet::new(&[0x40])), 0);
    }
}
