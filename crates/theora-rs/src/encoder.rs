use crate::codec::{
    Comment, HuffCode, Info, QuantInfo, YCbCrBuffer, OC_VENDOR_STRING, TH_NDCT_TOKENS,
    TH_NHUFFMAN_TABLES,
};
use crate::encinfo::{
    oc_state_flushheader, OC_PACKET_EMPTY, OC_PACKET_INFO_HDR, OC_PACKET_READY, OC_PACKET_SETUP_HDR,
};
use crate::error::{Result, TheoraError};
use crate::huffenc::TH_VP31_HUFF_CODES;
use crate::mathops::oc_ilog;
use crate::packet::OggPacket;
use crate::vp31::th_vp31_quant_info;

pub const TH_ENCCTL_SET_HUFFMAN_CODES: i32 = 0;
pub const TH_ENCCTL_SET_QUANT_PARAMS: i32 = 2;
pub const TH_ENCCTL_SET_KEYFRAME_FREQUENCY_FORCE: i32 = 4;
pub const TH_ENCCTL_SET_VP3_COMPATIBLE: i32 = 10;
pub const TH_ENCCTL_GET_SPLEVEL_MAX: i32 = 12;
pub const TH_ENCCTL_SET_SPLEVEL: i32 = 14;
pub const TH_ENCCTL_GET_SPLEVEL: i32 = 16;
pub const TH_ENCCTL_SET_DUP_COUNT: i32 = 18;
pub const TH_ENCCTL_SET_RATE_FLAGS: i32 = 20;
pub const TH_ENCCTL_SET_RATE_BUFFER: i32 = 22;

const ENCODER_MAX_SPLEVEL: i32 = 3;

#[derive(Debug, Clone)]
pub struct EncoderContext {
    pub info: Info,
    pub granulepos: i64,
    pub packet_state: i32,
    pub comment: Comment,
    pub vendor: String,
    pub qinfo: QuantInfo,
    pub huff_codes: [[HuffCode; TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES],
    pub last_frame: Option<YCbCrBuffer>,
    pub queued_frames: u64,
    pub sp_level: i32,
    pub dup_count: u32,
    pub rate_flags: i32,
    pub rate_buffer: i32,
    pub vp3_compatible: bool,
}

impl Default for EncoderContext {
    fn default() -> Self {
        Self::new(Info::default())
    }
}

impl EncoderContext {
    pub fn new(info: Info) -> Self {
        Self {
            info,
            granulepos: -1,
            packet_state: OC_PACKET_INFO_HDR,
            comment: Comment::default(),
            vendor: OC_VENDOR_STRING.to_string(),
            qinfo: th_vp31_quant_info(),
            huff_codes: TH_VP31_HUFF_CODES,
            last_frame: None,
            queued_frames: 0,
            sp_level: 0,
            dup_count: 0,
            rate_flags: 0,
            rate_buffer: 0,
            vp3_compatible: false,
        }
    }

    pub fn ctl(&mut self, req: i32, buf: &mut [u8]) -> Result<()> {
        match req {
            TH_ENCCTL_GET_SPLEVEL_MAX => write_i32(buf, ENCODER_MAX_SPLEVEL),
            TH_ENCCTL_SET_SPLEVEL => {
                let level = read_i32(buf)?;
                if !(0..=ENCODER_MAX_SPLEVEL).contains(&level) {
                    return Err(TheoraError::InvalidArgument);
                }
                self.sp_level = level;
                Ok(())
            }
            TH_ENCCTL_GET_SPLEVEL => write_i32(buf, self.sp_level),
            TH_ENCCTL_SET_DUP_COUNT => {
                self.dup_count = read_u32(buf)?;
                Ok(())
            }
            TH_ENCCTL_SET_RATE_FLAGS => {
                self.rate_flags = read_i32(buf)?;
                Ok(())
            }
            TH_ENCCTL_SET_RATE_BUFFER => {
                self.rate_buffer = read_i32(buf)?;
                Ok(())
            }
            TH_ENCCTL_SET_KEYFRAME_FREQUENCY_FORCE => {
                let freq = read_u32(buf)?;
                if freq == 0 {
                    return Err(TheoraError::InvalidArgument);
                }
                self.info.keyframe_granule_shift = oc_ilog(freq - 1).min(31);
                Ok(())
            }
            TH_ENCCTL_SET_VP3_COMPATIBLE => {
                self.vp3_compatible = read_i32(buf)? != 0;
                Ok(())
            }
            TH_ENCCTL_SET_HUFFMAN_CODES | TH_ENCCTL_SET_QUANT_PARAMS => {
                Err(TheoraError::NotImplemented)
            }
            _ => Err(TheoraError::NotImplemented),
        }
    }

    pub fn set_comment(&mut self, comment: Comment) {
        self.comment = comment;
    }

    pub fn set_vendor(&mut self, vendor: impl Into<String>) {
        self.vendor = vendor.into();
    }

    pub fn set_quant_info(&mut self, qinfo: QuantInfo) {
        self.qinfo = qinfo;
    }

    pub fn set_huff_codes(&mut self, codes: [[HuffCode; TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES]) {
        self.huff_codes = codes;
    }

    pub fn ycbcr_in(&mut self, buf: &YCbCrBuffer) -> Result<()> {
        validate_frame(buf)?;
        self.last_frame = Some(buf.clone());
        self.packet_state = OC_PACKET_READY;
        self.queued_frames = self.queued_frames.saturating_add(1);
        self.granulepos = if self.granulepos < 0 {
            0
        } else {
            self.granulepos + 1
        };
        Ok(())
    }

    pub fn packetout(&mut self, _last: bool) -> Result<Option<OggPacket>> {
        if self.packet_state < OC_PACKET_EMPTY {
            return self.flush_next_header();
        }
        if self.packet_state == OC_PACKET_READY {
            return Err(TheoraError::NotImplemented);
        }
        Ok(None)
    }

    pub fn has_pending_frame(&self) -> bool {
        self.packet_state == OC_PACKET_READY
    }

    fn flush_next_header(&mut self) -> Result<Option<OggPacket>> {
        oc_state_flushheader(
            Some(&self.info),
            &mut self.packet_state,
            Some(&self.qinfo),
            Some(&self.huff_codes),
            &self.vendor,
            Some(&self.comment),
        )
    }

    pub fn header_packet(&mut self) -> Result<OggPacket> {
        self.packet_state = OC_PACKET_INFO_HDR;
        self.flush_next_header()?.ok_or(TheoraError::BadPacket)
    }

    pub fn comment_packet(&mut self) -> Result<OggPacket> {
        self.packet_state = crate::encinfo::OC_PACKET_COMMENT_HDR;
        self.flush_next_header()?.ok_or(TheoraError::BadPacket)
    }

    pub fn tables_packet(&mut self) -> Result<OggPacket> {
        self.packet_state = OC_PACKET_SETUP_HDR;
        self.flush_next_header()?.ok_or(TheoraError::BadPacket)
    }

    pub fn granule_frame(&self, gp: i64) -> i64 {
        if gp < 0 {
            return -1;
        }
        let shift = self.info.keyframe_granule_shift.max(0) as i64;
        let iframe = gp >> shift;
        let pframe = gp - (iframe << shift);
        let bias = i64::from(
            (
                self.info.version_major as i32,
                self.info.version_minor as i32,
                self.info.version_subminor as i32,
            ) >= (3, 2, 1),
        );
        iframe + pframe - bias
    }

    pub fn granule_time(&self, gp: i64) -> f64 {
        if self.info.fps_numerator == 0 || gp < 0 {
            -1.0
        } else {
            (self.granule_frame(gp) + 1) as f64 * self.info.fps_denominator as f64
                / self.info.fps_numerator as f64
        }
    }
}

fn validate_info(info: &Info) -> Result<()> {
    if info.frame_width == 0 && info.pic_width == 0 {
        return Err(TheoraError::InvalidArgument);
    }
    if info.frame_height == 0 && info.pic_height == 0 {
        return Err(TheoraError::InvalidArgument);
    }
    if info.fps_numerator == 0 || info.fps_denominator == 0 {
        return Err(TheoraError::InvalidArgument);
    }
    Ok(())
}

fn validate_frame(buf: &YCbCrBuffer) -> Result<()> {
    if buf[0].width <= 0 || buf[0].height <= 0 || buf[0].data.is_empty() {
        return Err(TheoraError::InvalidArgument);
    }
    Ok(())
}

fn read_i32(buf: &[u8]) -> Result<i32> {
    if buf.len() != 4 {
        return Err(TheoraError::InvalidArgument);
    }
    Ok(i32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]))
}

fn write_i32(buf: &mut [u8], value: i32) -> Result<()> {
    if buf.len() != 4 {
        return Err(TheoraError::InvalidArgument);
    }
    buf.copy_from_slice(&value.to_ne_bytes());
    Ok(())
}

fn read_u32(buf: &[u8]) -> Result<u32> {
    if buf.len() != 4 {
        return Err(TheoraError::InvalidArgument);
    }
    Ok(u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]))
}

pub fn th_encode_alloc(info: &Info) -> Result<EncoderContext> {
    validate_info(info)?;
    Ok(EncoderContext::new(info.clone()))
}

pub fn th_encode_free(enc: &mut Option<EncoderContext>) {
    *enc = None;
}

pub fn th_encode_ctl(enc: &mut EncoderContext, req: i32, buf: &mut [u8]) -> Result<()> {
    enc.ctl(req, buf)
}

pub fn th_encode_flushheader(enc: &mut EncoderContext) -> Result<Option<OggPacket>> {
    if enc.packet_state < crate::encinfo::OC_PACKET_EMPTY {
        enc.packetout(false)
    } else {
        Ok(None)
    }
}

pub fn th_encode_ycbcr_in(enc: &mut EncoderContext, ycbcr: &YCbCrBuffer) -> Result<()> {
    enc.ycbcr_in(ycbcr)
}

pub fn th_encode_packetout(enc: &mut EncoderContext, last: bool) -> Result<Option<OggPacket>> {
    enc.packetout(last)
}
