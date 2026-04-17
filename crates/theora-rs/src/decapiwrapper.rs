use crate::api::oc_theora_info2th_info;
use crate::apiwrapper::{ApiInfo, ApiWrapper, CompatState, YuvBuffer};
use crate::codec::{Colorspace, Info, LegacyTheoraInfo, PixelFmt};
use crate::decoder::DecoderContext;
use crate::error::{Result, TheoraError};
use crate::packet::OggPacket;
use crate::{th_decode_headerin, Comment, SetupInfo};

pub fn th_info2theora_info(ci: &mut LegacyTheoraInfo, info: &Info) {
    ci.version_major = info.version_major;
    ci.version_minor = info.version_minor;
    ci.version_subminor = info.version_subminor;
    ci.width = info.frame_width;
    ci.height = info.frame_height;
    ci.frame_width = info.pic_width;
    ci.frame_height = info.pic_height;
    ci.offset_x = info.pic_x;
    ci.offset_y = info.pic_y;
    ci.fps_numerator = info.fps_numerator;
    ci.fps_denominator = info.fps_denominator;
    ci.aspect_numerator = info.aspect_numerator;
    ci.aspect_denominator = info.aspect_denominator;
    ci.colorspace = match info.colorspace {
        Colorspace::ItuRec470M => Colorspace::ItuRec470M,
        Colorspace::ItuRec470Bg => Colorspace::ItuRec470Bg,
        Colorspace::Unspecified => Colorspace::Unspecified,
    };
    ci.pixelformat = match info.pixel_fmt {
        PixelFmt::Pf420 => PixelFmt::Pf420,
        PixelFmt::Pf422 => PixelFmt::Pf422,
        PixelFmt::Pf444 => PixelFmt::Pf444,
        PixelFmt::Reserved => PixelFmt::Reserved,
    };
    ci.target_bitrate = info.target_bitrate;
    ci.quality = info.quality;
    ci.keyframe_frequency_force = if info.keyframe_granule_shift >= 0 {
        1u32.checked_shl(info.keyframe_granule_shift as u32)
            .unwrap_or(0)
    } else {
        0
    };
}

pub fn theora_decode_init(
    td: &mut CompatState,
    ci: &LegacyTheoraInfo,
    setup: SetupInfo,
) -> Result<()> {
    let info = oc_theora_info2th_info(ci);
    let decode = DecoderContext::new(info.clone(), setup.clone());
    td.internal_decode = true;
    td.internal_encode = false;
    td.granulepos = 0;
    td.info = Some(ci.clone());
    td.th_info = Some(info);
    td.api = Some(ApiWrapper {
        setup: Some(setup),
        decode: Some(decode),
        encode: None,
    });
    Ok(())
}

pub fn theora_decode_header(
    ci: &mut LegacyTheoraInfo,
    cc: &mut Comment,
    op: &OggPacket,
    setup_out: &mut Option<SetupInfo>,
) -> Result<()> {
    let mut info = oc_theora_info2th_info(ci);
    let ret = th_decode_headerin(&mut info, Some(cc), Some(setup_out), op)?;
    if ret < 0 {
        return Err(TheoraError::BadHeader);
    }
    th_info2theora_info(ci, &info);
    Ok(())
}

pub fn theora_decode_packetin(td: &mut CompatState, op: &OggPacket) -> Result<()> {
    let api = td.api.as_mut().ok_or(TheoraError::Fault)?;
    let decode = api.decode.as_mut().ok_or(TheoraError::Fault)?;
    decode.packetin(op)?;
    td.granulepos = decode.granulepos;
    Ok(())
}

pub fn theora_decode_yuv_out(td: &CompatState) -> Result<YuvBuffer> {
    let api = td.api.as_ref().ok_or(TheoraError::Fault)?;
    let decode = api.decode.as_ref().ok_or(TheoraError::Fault)?;
    let buf = decode.ycbcr_out()?;
    Ok(YuvBuffer::from_ycbcr(&buf))
}
