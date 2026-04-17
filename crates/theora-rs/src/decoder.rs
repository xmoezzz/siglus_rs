use crate::codec::{Info, YCbCrBuffer};
use crate::decinfo::SetupInfo;
use crate::decint::DecContext;
use crate::decode as decode_impl;
use crate::error::{Result, TheoraError};
use crate::packet::OggPacket;
use crate::quant::oc_dequant_tables_init;
use crate::state::{
    oc_state_borders_fill, oc_state_init, OC_FRAME_GOLD, OC_FRAME_PREV, OC_FRAME_SELF,
};

pub const TH_DECCTL_GET_PPLEVEL_MAX: i32 = 1;
pub const TH_DECCTL_SET_PPLEVEL: i32 = 3;
pub const TH_DECCTL_SET_GRANPOS: i32 = 5;
pub const TH_DECCTL_SET_STRIPE_CB: i32 = 7;
pub const TH_DECCTL_SET_MBMODE: i32 = 9;
pub const TH_DECCTL_SET_MV: i32 = 11;
pub const TH_DECCTL_SET_QI: i32 = 13;
pub const TH_DECCTL_SET_BITS: i32 = 15;

const DECODER_MAX_PP_LEVEL: i32 = 7;

#[derive(Debug, Clone, Default)]
pub struct DecoderContext {
    pub info: Info,
    pub setup: SetupInfo,
    pub granulepos: i64,
    pub packet_state: i32,
    pub last_frame: Option<YCbCrBuffer>,
    pub pp_level: i32,
    pub stripe_cb_enabled: bool,
    pub telemetry_mbmode: i32,
    pub telemetry_mv: i32,
    pub telemetry_qi: i32,
    pub telemetry_bits: i32,
    pub packets_seen: u64,
    pub raw: DecContext,
    pub frame_available: bool,
}

impl DecoderContext {
    pub fn try_new(info: Info, setup: SetupInfo) -> Result<Self> {
        let mut raw = DecContext::default();
        validate_info(&info)?;
        oc_state_init(&mut raw.state, &info, 6)?;
        raw.info = info.clone();
        raw.setup = Some(setup.clone());
        raw.qinfo = setup.qinfo.clone();
        if let Ok((dequant, pp_dc_scale)) = oc_dequant_tables_init(&setup.qinfo) {
            raw.state.dequant_tables = dequant;
            raw.pp_dc_scale = pp_dc_scale;
            for qi in 0..64usize {
                let mut qsum = 0i32;
                for qti in 0..2usize {
                    for pli in 0..3usize {
                        qsum += i32::from(raw.state.dequant_tables[qi][pli][qti][12]);
                        qsum += i32::from(raw.state.dequant_tables[qi][pli][qti][17]);
                        qsum += i32::from(raw.state.dequant_tables[qi][pli][qti][18]);
                        qsum += i32::from(raw.state.dequant_tables[qi][pli][qti][24])
                            << if pli == 0 { 1 } else { 0 };
                    }
                }
                raw.pp_sharp_mod[qi] = -(qsum >> 11);
            }
        }
        raw.state.loop_filter_limits = setup.qinfo.loop_filter_limits;
        raw.pp_level = 0;
        raw.granulepos = -1;

        let mut this = Self {
            info,
            setup,
            granulepos: -1,
            packet_state: 0,
            last_frame: None,
            pp_level: 0,
            stripe_cb_enabled: false,
            telemetry_mbmode: 0,
            telemetry_mv: 0,
            telemetry_qi: 0,
            telemetry_bits: 0,
            packets_seen: 0,
            raw,
            frame_available: false,
        };
        this.install_reference_frames();
        Ok(this)
    }

    pub fn new(info: Info, setup: SetupInfo) -> Self {
        Self::try_new(info, setup).unwrap_or_default()
    }

    pub fn ctl(&mut self, req: i32, buf: &mut [u8]) -> Result<()> {
        match req {
            TH_DECCTL_GET_PPLEVEL_MAX => write_i32(buf, DECODER_MAX_PP_LEVEL),
            TH_DECCTL_SET_PPLEVEL => {
                let level = read_i32(buf)?;
                if !(0..=DECODER_MAX_PP_LEVEL).contains(&level) {
                    return Err(TheoraError::InvalidArgument);
                }
                self.pp_level = level;
                self.raw.pp_level = level;
                Ok(())
            }
            TH_DECCTL_SET_GRANPOS => {
                let gp = read_i64(buf)?;
                if gp < 0 {
                    return Err(TheoraError::InvalidArgument);
                }
                self.granulepos = gp;
                self.raw.granulepos = gp;
                Ok(())
            }
            TH_DECCTL_SET_STRIPE_CB => {
                self.stripe_cb_enabled = !buf.is_empty();
                Ok(())
            }
            TH_DECCTL_SET_MBMODE => {
                self.telemetry_mbmode = read_i32(buf)?;
                Ok(())
            }
            TH_DECCTL_SET_MV => {
                self.telemetry_mv = read_i32(buf)?;
                Ok(())
            }
            TH_DECCTL_SET_QI => {
                self.telemetry_qi = read_i32(buf)?;
                Ok(())
            }
            TH_DECCTL_SET_BITS => {
                self.telemetry_bits = read_i32(buf)?;
                Ok(())
            }
            _ => Err(TheoraError::NotImplemented),
        }
    }

    pub fn packetin(&mut self, op: &OggPacket) -> Result<()> {
        if op.packet.is_empty() {
            self.bump_granule(op.granulepos);
            self.packets_seen = self.packets_seen.saturating_add(1);
            return Ok(());
        }
        if (op.packet[0] & 0x80) != 0 {
            return Err(TheoraError::BadPacket);
        }
        let _is_keyframe = decode_impl::th_decode_packetin(&mut self.raw, op, Some(op.granulepos))?;
        self.bump_granule(op.granulepos);
        self.packet_state = self.packet_state.saturating_add(1);
        self.packets_seen = self.packets_seen.saturating_add(1);
        self.raw.granulepos = self.granulepos;
        let frame = decode_impl::th_decode_ycbcr_out(&self.raw)?;
        self.last_frame = Some(frame);
        self.frame_available = true;
        Ok(())
    }

    pub fn ycbcr_out(&self) -> Result<YCbCrBuffer> {
        if let Some(frame) = &self.last_frame {
            return Ok(frame.clone());
        }
        if self.packets_seen == 0 {
            return Err(TheoraError::BadPacket);
        }
        decode_impl::th_decode_ycbcr_out(&self.raw)
    }

    pub fn has_decoded_frame(&self) -> bool {
        self.frame_available && self.last_frame.is_some()
    }

    pub fn granule_frame(&self, gp: i64) -> i64 {
        th_granule_frame(&self.info, gp)
    }

    pub fn granule_time(&self, gp: i64) -> f64 {
        th_granule_time(&self.info, gp)
    }

    fn bump_granule(&mut self, incoming: i64) {
        if incoming >= 0 {
            self.granulepos = incoming;
        } else if self.granulepos < 0 {
            self.granulepos = 0;
        } else {
            self.granulepos += 1;
        }
    }

    fn install_reference_frames(&mut self) {
        for rfi in 0..self.raw.state.ref_frame_bufs.len() {
            if !self.raw.state.ref_frame_bufs[rfi][0].data.is_empty() {
                self.raw.state.ref_frame_bufs[rfi][0].data.fill(0x80);
            }
            if !self.raw.state.ref_frame_bufs[rfi][1].data.is_empty() {
                self.raw.state.ref_frame_bufs[rfi][1].data.fill(0x80);
            }
            if !self.raw.state.ref_frame_bufs[rfi][2].data.is_empty() {
                self.raw.state.ref_frame_bufs[rfi][2].data.fill(0x80);
            }
            self.raw.state.ref_frame_idx[rfi] = -1;
            oc_state_borders_fill(&mut self.raw.state, rfi);
        }
        self.raw.pp_frame_buf = [Default::default(), Default::default(), Default::default()];
    }

    pub fn install_black_test_frame(&mut self) {
        self.install_reference_frames();
        self.last_frame = Some(self.raw.pp_frame_buf.clone());
        self.frame_available = true;
    }
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

fn read_i64(buf: &[u8]) -> Result<i64> {
    if buf.len() != 8 {
        return Err(TheoraError::InvalidArgument);
    }
    Ok(i64::from_ne_bytes([
        buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
    ]))
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

pub fn th_granule_frame(info: &Info, gp: i64) -> i64 {
    if gp < 0 {
        return -1;
    }
    let shift = info.keyframe_granule_shift.max(0) as i64;
    let iframe = gp >> shift;
    let pframe = gp - (iframe << shift);
    let bias = i64::from(
        (
            info.version_major as i32,
            info.version_minor as i32,
            info.version_subminor as i32,
        ) >= (3, 2, 1),
    );
    iframe + pframe - bias
}

pub fn th_granule_time(info: &Info, gp: i64) -> f64 {
    if info.fps_numerator == 0 || gp < 0 {
        -1.0
    } else {
        (th_granule_frame(info, gp) + 1) as f64 * info.fps_denominator as f64
            / info.fps_numerator as f64
    }
}

pub fn th_decode_alloc(info: &Info, setup: &SetupInfo) -> Result<DecoderContext> {
    DecoderContext::try_new(info.clone(), setup.clone())
}

pub fn oc_dec_accel_init_c(_dec: &mut DecContext) {}

pub fn oc_dec_init(info: Info, setup: SetupInfo) -> DecoderContext {
    DecoderContext::new(info, setup)
}

pub fn oc_dec_clear(dec: &mut DecoderContext) {
    *dec = DecoderContext::default();
}

pub fn th_decode_free(dec: &mut Option<DecoderContext>) {
    *dec = None;
}

pub fn th_decode_ctl(dec: &mut DecoderContext, req: i32, buf: &mut [u8]) -> Result<()> {
    dec.ctl(req, buf)
}

pub fn th_decode_packetin(dec: &mut DecoderContext, op: &OggPacket) -> Result<()> {
    dec.packetin(op)
}

pub fn th_decode_ycbcr_out(dec: &DecoderContext) -> Result<YCbCrBuffer> {
    dec.ycbcr_out()
}

pub fn oc_dec_init_dummy_frame(dec: &mut DecoderContext) {
    dec.install_black_test_frame();
}

pub fn oc_render_telemetry(_dec: &DecoderContext) {}
