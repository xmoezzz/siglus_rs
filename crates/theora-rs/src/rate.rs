use crate::mathops::{oc_bexp64, q57};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IirFilter {
    pub c: [i32; 2],
    pub g: i64,
    pub x: [i32; 2],
    pub y: [i32; 2],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrameMetrics {
    pub log_scale: i32,
    pub dup_count: u32,
    pub frame_type: u8,
    pub activity_avg: u32,
}

#[derive(Debug, Clone)]
pub struct RcState {
    pub bits_per_frame: i64,
    pub fullness: i64,
    pub target: i64,
    pub max: i64,
    pub log_npixels: i64,
    pub exp: [u32; 2],
    pub buf_delay: i32,
    pub prev_drop_count: u32,
    pub log_drop_scale: i64,
    pub log_scale: [i64; 2],
    pub log_qtarget: i64,
    pub drop_frames: bool,
    pub cap_overflow: bool,
    pub cap_underflow: bool,
    pub scalefilter: [IirFilter; 2],
    pub inter_count: i32,
    pub inter_delay: i32,
    pub inter_delay_target: i32,
    pub vfrfilter: IirFilter,
    pub twopass: i32,
    pub twopass_buffer: [u8; 48],
    pub twopass_buffer_bytes: i32,
    pub twopass_buffer_fill: i32,
    pub twopass_force_kf: bool,
    pub prev_metrics: FrameMetrics,
    pub cur_metrics: FrameMetrics,
    pub frame_metrics: Vec<FrameMetrics>,
    pub frame_metrics_head: i32,
    pub frames_total: [u32; 3],
    pub frames_left: [u32; 3],
    pub scale_sum: [i64; 2],
    pub scale_window0: i32,
    pub scale_window_end: i32,
    pub nframes: [i32; 3],
    pub rate_bias: i64,
}

pub const OC_ROUGH_TAN_LOOKUP: [u16; 18] = [
    0, 358, 722, 1098, 1491, 1910, 2365, 2868, 3437, 4096, 4881, 5850, 7094, 8784, 11254, 15286,
    23230, 46817,
];

pub fn oc_warp_alpha(alpha: i32) -> i32 {
    let mut i = (alpha * 36) >> 24;
    if i >= 17 {
        i = 16;
    }
    let t0 = OC_ROUGH_TAN_LOOKUP[i as usize] as i32;
    let t1 = OC_ROUGH_TAN_LOOKUP[i as usize + 1] as i32;
    let d = alpha * 36 - (i << 24);
    ((((t0 as i64) << 32) + (((t1 - t0) << 8) as i64) * d as i64) >> 32) as i32
}

pub fn oc_iir_filter_reinit(f: &mut IirFilter, delay: i32) {
    let alpha = (1 << 24) / delay;
    let one48 = 1i64 << 48;
    let warp = i64::from(oc_warp_alpha(alpha).max(1));
    let k1 = 3 * warp;
    let k2 = k1 * warp;
    let d = ((((1i64 << 12) + k1) << 12) + k2 + 256) >> 9;
    let a = (k2 << 23) / d;
    let ik2 = one48 / k2;
    let b1 = 2 * a * (ik2 - (1 << 24));
    let b2 = (one48 << 8) - (4 * a << 24) - b1;
    f.c[0] = ((b1 + (1i64 << 31)) >> 32) as i32;
    f.c[1] = ((b2 + (1i64 << 31)) >> 32) as i32;
    f.g = (a + 128) >> 8;
}

pub fn oc_iir_filter_init(f: &mut IirFilter, delay: i32, value: i32) {
    oc_iir_filter_reinit(f, delay);
    f.y = [value; 2];
    f.x = [value; 2];
}

pub fn oc_iir_filter_update(f: &mut IirFilter, x: i32) -> i64 {
    let c0 = f.c[0] as i64;
    let c1 = f.c[1] as i64;
    let g = f.g;
    let x0 = f.x[0] as i64;
    let x1 = f.x[1] as i64;
    let y0 = f.y[0] as i64;
    let y1 = f.y[1] as i64;
    let ya = ((x as i64 + x0 * 2 + x1) * g + y0 * c0 + y1 * c1 + (1 << 23)) >> 24;
    f.x[1] = x0 as i32;
    f.x[0] = x;
    f.y[1] = y0 as i32;
    f.y[0] = ya as i32;
    ya
}

pub fn oc_enc_find_qi_for_target(
    log_qavg: &[[i64; 64]; 2],
    qti: usize,
    qi_old: i32,
    qi_min: i32,
    log_qtarget: i64,
) -> i32 {
    let mut best_qi = qi_min;
    let mut best_qdiff = (log_qavg[qti][best_qi as usize] - log_qtarget).abs();
    for qi in (qi_min + 1)..64 {
        let qdiff = (log_qavg[qti][qi as usize] - log_qtarget).abs();
        if qdiff < best_qdiff
            || (qdiff == best_qdiff && (qi - qi_old).abs() < (best_qi - qi_old).abs())
        {
            best_qi = qi;
            best_qdiff = qdiff;
        }
    }
    best_qi
}

pub fn oc_bexp_q24(log_scale: i32) -> i64 {
    if log_scale < (23 << 24) {
        let ret = oc_bexp64(((log_scale as i64) << 33) + q57(24));
        if ret < 0x7FFF_FFFF_FFFFi64 {
            ret
        } else {
            0x7FFF_FFFF_FFFFi64
        }
    } else {
        0x7FFF_FFFF_FFFFi64
    }
}

pub fn oc_q57_to_q24(input: i64) -> i32 {
    let ret = (input + (1i64 << 32)) >> 33;
    ret.clamp(-0x7FFF_FFFF - 1, 0x7FFF_FFFF) as i32
}

pub fn oc_bexp64_q24(log_scale: i64) -> i32 {
    if log_scale < q57(8) {
        let ret = oc_bexp64(log_scale + q57(24));
        if ret < 0x7FFF_FFFF {
            ret as i32
        } else {
            0x7FFF_FFFF
        }
    } else {
        0x7FFF_FFFF
    }
}

pub fn oc_enc_calc_lambda(enc: &mut crate::encint::EncContext, qti: usize) {
    let qi = enc.state.qis[qti.min(2)] as usize;
    let q = enc.log_qavg[qti.min(1)][qi.min(63)];
    let base = oc_bexp64_q24(q) as i64;
    enc.lambda = ((base * 149 + (1 << 7)) >> 8).clamp(0, i32::MAX as i64) as i32;
}

pub fn oc_enc_rc_reset(enc: &mut crate::encint::EncContext) {
    let rc = &mut enc.rc;
    rc.fullness = rc.target;
    rc.prev_drop_count = 0;
    rc.inter_count = 0;
    rc.inter_delay = 0;
    rc.rate_bias = 0;
    rc.frame_metrics.clear();
    rc.frame_metrics_head = 0;
}

pub fn oc_rc_state_init(rc: &mut RcState, enc: &crate::encint::EncContext) {
    *rc = RcState::default();
    let npixels = i64::from(enc.info.frame_width.max(1) * enc.info.frame_height.max(1));
    rc.bits_per_frame = if enc.info.fps_denominator != 0 {
        i64::from(enc.info.target_bitrate.max(0)) * i64::from(enc.info.fps_denominator)
            / i64::from(enc.info.fps_numerator.max(1))
    } else {
        0
    };
    rc.target = rc.bits_per_frame * 32;
    rc.max = rc.target * 2;
    rc.fullness = rc.target;
    rc.log_npixels = q57(npixels.max(1).ilog2() as i32);
}

pub fn oc_rc_state_clear(rc: &mut RcState) {
    *rc = RcState::default();
}

pub fn oc_enc_rc_resize(enc: &mut crate::encint::EncContext) {
    let rc = &mut enc.rc;
    rc.bits_per_frame = if enc.info.fps_denominator != 0 {
        i64::from(enc.info.target_bitrate.max(0)) * i64::from(enc.info.fps_denominator)
            / i64::from(enc.info.fps_numerator.max(1))
    } else {
        0
    };
    rc.target = rc.bits_per_frame * i64::from(rc.buf_delay.max(1));
    rc.max = rc.target * 2;
    rc.fullness = rc.fullness.clamp(0, rc.max);
}

pub fn oc_rc_scale_drop(rc: &RcState, dup_count: u32) -> i64 {
    let scale = if dup_count > 0 { rc.log_drop_scale } else { 0 };
    (oc_bexp64(scale + q57(24)) >> 24).max(1)
}

pub fn oc_enc_select_qi(enc: &mut crate::encint::EncContext, qti: usize, clamp: i32) -> i32 {
    let qi_min = clamp.max(0);
    let target = enc.rc.log_qtarget;
    let qi = oc_enc_find_qi_for_target(
        &enc.log_qavg,
        qti.min(1),
        enc.state.qis[qti.min(2)] as i32,
        qi_min,
        target,
    );
    enc.state.qis[qti.min(2)] = qi as u8;
    qi
}

pub fn oc_enc_update_rc_state(
    enc: &mut crate::encint::EncContext,
    bits_used: i64,
    frame_type: u8,
    dup_count: u32,
    activity_avg: u32,
) -> i32 {
    let rc = &mut enc.rc;
    rc.fullness = (rc.fullness + rc.bits_per_frame - bits_used).clamp(0, rc.max);
    rc.cur_metrics = FrameMetrics {
        log_scale: oc_q57_to_q24(rc.log_qtarget),
        dup_count,
        frame_type,
        activity_avg,
    };
    rc.frame_metrics.push(rc.cur_metrics);
    if frame_type != 0 {
        rc.inter_count += 1;
    }
    if bits_used > rc.bits_per_frame {
        1
    } else {
        0
    }
}

pub fn oc_rc_buffer_val(rc: &RcState) -> i64 {
    rc.fullness
}

pub fn oc_rc_buffer_fill(rc: &mut RcState, bits: i64) -> i64 {
    rc.fullness = (rc.fullness + bits).clamp(0, rc.max);
    rc.fullness
}

pub fn oc_rc_unbuffer_val(rc: &mut RcState, bits: i64) -> i64 {
    rc.fullness = (rc.fullness - bits).clamp(0, rc.max);
    rc.fullness
}

pub fn oc_enc_rc_2pass_out(enc: &mut crate::encint::EncContext, buf: &mut Vec<u8>) -> i32 {
    let m = enc.rc.cur_metrics;
    buf.extend_from_slice(&m.log_scale.to_le_bytes());
    buf.extend_from_slice(&m.dup_count.to_le_bytes());
    buf.push(m.frame_type);
    buf.extend_from_slice(&m.activity_avg.to_le_bytes());
    13
}

pub fn oc_enc_rc_2pass_in(enc: &mut crate::encint::EncContext, buf: &[u8]) -> i32 {
    if buf.len() < 13 {
        return -1;
    }
    let log_scale = i32::from_le_bytes(buf[0..4].try_into().unwrap());
    let dup_count = u32::from_le_bytes(buf[4..8].try_into().unwrap());
    let frame_type = buf[8];
    let activity_avg = u32::from_le_bytes(buf[9..13].try_into().unwrap());
    enc.rc.prev_metrics = FrameMetrics {
        log_scale,
        dup_count,
        frame_type,
        activity_avg,
    };
    13
}

impl Default for RcState {
    fn default() -> Self {
        Self {
            bits_per_frame: 0,
            fullness: 0,
            target: 0,
            max: 0,
            log_npixels: 0,
            exp: [0; 2],
            buf_delay: 0,
            prev_drop_count: 0,
            log_drop_scale: 0,
            log_scale: [0; 2],
            log_qtarget: 0,
            drop_frames: false,
            cap_overflow: false,
            cap_underflow: false,
            scalefilter: [Default::default(), Default::default()],
            inter_count: 0,
            inter_delay: 0,
            inter_delay_target: 0,
            vfrfilter: Default::default(),
            twopass: 0,
            twopass_buffer: [0; 48],
            twopass_buffer_bytes: 0,
            twopass_buffer_fill: 0,
            twopass_force_kf: false,
            prev_metrics: Default::default(),
            cur_metrics: Default::default(),
            frame_metrics: Vec::new(),
            frame_metrics_head: 0,
            frames_total: [0; 3],
            frames_left: [0; 3],
            scale_sum: [0; 2],
            scale_window0: 0,
            scale_window_end: 0,
            nframes: [0; 3],
            rate_bias: 0,
        }
    }
}
