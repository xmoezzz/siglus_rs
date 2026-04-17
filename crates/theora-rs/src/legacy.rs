use crate::api::{
    oc_theora_info2th_info, th_packet_isheader, th_packet_iskeyframe, th_version_number,
    th_version_string, Packet,
};
use crate::apiwrapper::YuvBuffer;
use crate::codec::{Comment, Info, LegacyTheoraInfo};
use crate::decinfo::{th_decode_headerin, SetupInfo};
use crate::decoder::{
    th_decode_alloc, th_decode_ctl, th_decode_packetin, th_decode_ycbcr_out, th_granule_frame,
    th_granule_time, DecoderContext,
};
use crate::encinfo::{oc_state_flushheader, OC_PACKET_COMMENT_HDR};
use crate::encoder::{
    th_encode_alloc, th_encode_ctl, th_encode_packetout, th_encode_ycbcr_in, EncoderContext,
};
use crate::error::{Result, TheoraError};
use crate::info::{
    th_comment_add, th_comment_add_tag, th_comment_clear, th_comment_init, th_comment_query,
    th_comment_query_count, th_info_init,
};
use crate::mathops::oc_ilog;
use crate::packet::OggPacket;

#[derive(Debug, Clone, Default)]
pub struct LegacyState {
    pub info: Option<LegacyTheoraInfo>,
    pub granulepos: i64,
    pub internal_encode: Option<EncoderContext>,
    pub internal_decode: Option<DecoderContext>,
}

pub fn theora_version_string() -> &'static str {
    th_version_string()
}

pub fn theora_version_number() -> u32 {
    th_version_number()
}

pub fn theora_info_init(ci: &mut LegacyTheoraInfo) {
    *ci = LegacyTheoraInfo::default();
}

pub fn theora_info_clear(ci: &mut LegacyTheoraInfo) {
    ci.codec_setup = None;
    *ci = LegacyTheoraInfo::default();
}

pub fn theora_clear(state: &mut LegacyState) {
    *state = LegacyState::default();
}

pub fn theora_encode_init(state: &mut LegacyState, ci: &LegacyTheoraInfo) -> Result<()> {
    let info = oc_theora_info2th_info(ci);
    let enc = th_encode_alloc(&info)?;
    state.info = Some(ci.clone());
    state.granulepos = -1;
    state.internal_encode = Some(enc);
    state.internal_decode = None;
    Ok(())
}

pub fn theora_encode_yuv_in(state: &mut LegacyState, yuv: &YuvBuffer) -> Result<()> {
    let enc = state.internal_encode.as_mut().ok_or(TheoraError::Fault)?;
    th_encode_ycbcr_in(enc, &yuv.to_ycbcr())?;
    state.granulepos = enc.granulepos;
    Ok(())
}

pub fn theora_encode_packetout(state: &mut LegacyState, last: bool) -> Result<Option<OggPacket>> {
    let enc = state.internal_encode.as_mut().ok_or(TheoraError::Fault)?;
    let pkt = th_encode_packetout(enc, last)?;
    state.granulepos = enc.granulepos;
    Ok(pkt)
}

pub fn theora_encode_header(state: &mut LegacyState) -> Result<OggPacket> {
    let enc = state.internal_encode.as_mut().ok_or(TheoraError::Fault)?;
    enc.header_packet()
}

pub fn theora_encode_comment(tc: &Comment) -> Result<OggPacket> {
    let mut packet_state = OC_PACKET_COMMENT_HDR;
    oc_state_flushheader(
        None,
        &mut packet_state,
        None,
        None,
        crate::codec::OC_VENDOR_STRING,
        Some(tc),
    )?
    .ok_or(TheoraError::BadPacket)
}

pub fn theora_encode_tables(state: &mut LegacyState) -> Result<OggPacket> {
    let enc = state.internal_encode.as_mut().ok_or(TheoraError::Fault)?;
    enc.tables_packet()
}

pub fn theora_decode_header(
    ci: &mut LegacyTheoraInfo,
    cc: &mut Comment,
    op: &OggPacket,
) -> Result<()> {
    let mut info = oc_theora_info2th_info(ci);
    let mut setup = ci.codec_setup.clone();
    let ret = th_decode_headerin(&mut info, Some(cc), Some(&mut setup), op)?;
    if ret < 0 {
        return Err(TheoraError::BadHeader);
    }
    ci.codec_setup = setup;
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
    ci.colorspace = info.colorspace;
    ci.pixelformat = info.pixel_fmt;
    ci.target_bitrate = info.target_bitrate;
    ci.quality = info.quality;
    ci.keyframe_frequency_force = if info.keyframe_granule_shift >= 0 {
        1u32.checked_shl(info.keyframe_granule_shift as u32)
            .unwrap_or(0)
    } else {
        0
    };
    Ok(())
}

pub fn theora_decode_init(state: &mut LegacyState, ci: &LegacyTheoraInfo) -> Result<()> {
    let info = oc_theora_info2th_info(ci);
    let setup = ci.codec_setup.clone().ok_or(TheoraError::BadHeader)?;
    let dec = th_decode_alloc(&info, &setup)?;
    state.info = Some(ci.clone());
    state.granulepos = -1;
    state.internal_decode = Some(dec);
    state.internal_encode = None;
    Ok(())
}

pub fn theora_decode_packetin(state: &mut LegacyState, op: &OggPacket) -> Result<()> {
    let dec = state.internal_decode.as_mut().ok_or(TheoraError::Fault)?;
    th_decode_packetin(dec, op)?;
    state.granulepos = dec.granulepos;
    Ok(())
}

pub fn theora_decode_yuv_out(state: &LegacyState) -> Result<YuvBuffer> {
    let dec = state.internal_decode.as_ref().ok_or(TheoraError::Fault)?;
    Ok(YuvBuffer::from_ycbcr(&th_decode_ycbcr_out(dec)?))
}

pub fn theora_control(state: &mut LegacyState, req: i32, buf: &mut [u8]) -> Result<()> {
    if let Some(dec) = state.internal_decode.as_mut() {
        return th_decode_ctl(dec, req, buf);
    }
    if let Some(enc) = state.internal_encode.as_mut() {
        return th_encode_ctl(enc, req, buf);
    }
    Err(TheoraError::Fault)
}

pub fn theora_granule_frame(state: &LegacyState, gp: i64) -> i64 {
    if let Some(dec) = state.internal_decode.as_ref() {
        dec.granule_frame(gp)
    } else if let Some(enc) = state.internal_encode.as_ref() {
        enc.granule_frame(gp)
    } else if let Some(info) = state.info.as_ref() {
        th_granule_frame(&oc_theora_info2th_info(info), gp)
    } else {
        gp
    }
}

pub fn theora_granule_time(state: &LegacyState, gp: i64) -> f64 {
    if let Some(dec) = state.internal_decode.as_ref() {
        dec.granule_time(gp)
    } else if let Some(enc) = state.internal_encode.as_ref() {
        enc.granule_time(gp)
    } else if let Some(info) = state.info.as_ref() {
        th_granule_time(&oc_theora_info2th_info(info), gp)
    } else {
        -1.0
    }
}

pub fn theora_packet_isheader(packet: &[u8]) -> bool {
    th_packet_isheader(Packet::new(packet))
}

pub fn theora_packet_iskeyframe(packet: &[u8]) -> i32 {
    th_packet_iskeyframe(Packet::new(packet))
}

pub fn theora_granule_shift(ci: &LegacyTheoraInfo) -> i32 {
    oc_ilog(ci.keyframe_frequency_force.saturating_sub(1)).max(0)
}

pub fn theora_info_to_th_info(ci: &LegacyTheoraInfo) -> Info {
    oc_theora_info2th_info(ci)
}

pub fn theora_comment_init(tc: &mut Comment) {
    th_comment_init(tc)
}

pub fn theora_comment_query<'a>(tc: &'a Comment, tag: &str, count: i32) -> Option<&'a [u8]> {
    th_comment_query(tc, tag, count)
}

pub fn theora_comment_query_count(tc: &Comment, tag: &str) -> i32 {
    th_comment_query_count(tc, tag)
}

pub fn theora_comment_clear(tc: &mut Comment) {
    th_comment_clear(tc)
}

pub fn theora_comment_add(tc: &mut Comment, comment: &[u8]) {
    th_comment_add(tc, comment)
}

pub fn theora_comment_add_tag(tc: &mut Comment, tag: &str, value: &str) {
    th_comment_add_tag(tc, tag, value)
}

pub fn legacy_info_roundtrip_defaults() -> Info {
    let mut ci = LegacyTheoraInfo::default();
    theora_info_init(&mut ci);
    let mut info = Info::default();
    th_info_init(&mut info);
    oc_theora_info2th_info(&ci)
}
