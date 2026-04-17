use crate::api::oc_theora_info2th_info;
use crate::apiwrapper::{ApiWrapper, CompatState, YuvBuffer};
use crate::encoder::EncoderContext;
use crate::error::{Result, TheoraError};
use crate::packet::OggPacket;
use crate::{Comment, LegacyTheoraInfo, OC_VENDOR_STRING};

pub fn theora_encode_init(te: &mut CompatState, ci: &LegacyTheoraInfo) -> Result<()> {
    let info = oc_theora_info2th_info(ci);
    let mut encode = EncoderContext::new(info.clone());
    encode.set_vendor(OC_VENDOR_STRING);
    te.internal_encode = true;
    te.internal_decode = false;
    te.granulepos = 0;
    te.info = Some(ci.clone());
    te.th_info = Some(info);
    te.api = Some(ApiWrapper {
        setup: None,
        decode: None,
        encode: Some(encode),
    });
    Ok(())
}

pub fn theora_encode_yuv_in(te: &mut CompatState, yuv: &YuvBuffer) -> Result<()> {
    let api = te.api.as_mut().ok_or(TheoraError::Fault)?;
    let enc = api.encode.as_mut().ok_or(TheoraError::Fault)?;
    enc.ycbcr_in(&yuv.to_ycbcr())?;
    te.granulepos = enc.granulepos;
    Ok(())
}

pub fn theora_encode_packetout(te: &mut CompatState, last: bool) -> Result<Option<OggPacket>> {
    let api = te.api.as_mut().ok_or(TheoraError::Fault)?;
    let enc = api.encode.as_mut().ok_or(TheoraError::Fault)?;
    enc.packetout(last)
}

pub fn theora_encode_header(te: &mut CompatState) -> Result<OggPacket> {
    let api = te.api.as_mut().ok_or(TheoraError::Fault)?;
    let enc = api.encode.as_mut().ok_or(TheoraError::Fault)?;
    enc.header_packet()
}

pub fn theora_encode_comment(tc: &Comment) -> Result<OggPacket> {
    let mut enc = EncoderContext::default();
    enc.set_comment(tc.clone());
    enc.comment_packet()
}

pub fn theora_encode_tables(te: &mut CompatState) -> Result<OggPacket> {
    let api = te.api.as_mut().ok_or(TheoraError::Fault)?;
    let enc = api.encode.as_mut().ok_or(TheoraError::Fault)?;
    enc.tables_packet()
}
