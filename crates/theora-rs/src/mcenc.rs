use crate::encfrag::{
    oc_enc_frag_sad2_thresh_c, oc_enc_frag_sad_c, oc_enc_frag_satd2_c, oc_enc_frag_satd_c,
};
use crate::state::{oc_mv_x, oc_mv_y, OcMv};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct McEncCtx {
    pub candidates: [[i32; 2]; 13],
    pub setb0: i32,
    pub ncandidates: i32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MbEncInfo {
    pub cneighbors: [usize; 4],
    pub pneighbors: [usize; 4],
    pub ncneighbors: u8,
    pub npneighbors: u8,
    pub refined: u8,
    pub analysis_mv: [[[i16; 2]; 2]; 3],
    pub unref_mv: [[i16; 2]; 2],
    pub block_mv: [[i16; 2]; 4],
    pub ref_mv: [[i16; 2]; 4],
    pub error: [u16; 2],
    pub satd: [u32; 2],
    pub block_satd: [u32; 4],
}

pub const OC_YSAD_THRESH1: u32 = 256;
pub const OC_YSAD_THRESH2_SCALE_BITS: u32 = 4;
pub const OC_YSAD_THRESH2_OFFSET: u32 = 64;

pub const OC_SQUARE_DX: [i32; 9] = [-1, 0, 1, -1, 0, 1, -1, 0, 1];
pub const OC_SQUARE_DY: [i32; 9] = [-1, -1, -1, 0, 0, 0, 1, 1, 1];
pub const OC_SQUARE_NSITES: [i32; 11] = [8, 5, 5, 0, 5, 3, 3, 0, 5, 3, 3];
pub const OC_SQUARE_SITES: [[i32; 8]; 11] = [
    [0, 1, 2, 3, 5, 6, 7, 8],
    [1, 2, 5, 7, 8, 0, 0, 0],
    [0, 1, 3, 6, 7, 0, 0, 0],
    [-1, 0, 0, 0, 0, 0, 0, 0],
    [3, 5, 6, 7, 8, 0, 0, 0],
    [5, 7, 8, 0, 0, 0, 0, 0],
    [3, 6, 7, 0, 0, 0, 0, 0],
    [-1, 0, 0, 0, 0, 0, 0, 0],
    [0, 1, 2, 3, 5, 0, 0, 0],
    [1, 2, 5, 0, 0, 0, 0, 0],
    [0, 1, 3, 0, 0, 0, 0, 0],
];

#[inline]
fn clamp31(v: i32) -> i32 {
    v.clamp(-31, 31)
}

pub fn oc_mcenc_find_candidates_a(
    mb_info: &[MbEncInfo],
    mcenc: &mut McEncCtx,
    accum: OcMv,
    mbi: usize,
    frame: usize,
) {
    let mut ncandidates = 1usize;
    if mb_info[mbi].ncneighbors > 0 {
        for i in 0..mb_info[mbi].ncneighbors as usize {
            let nmbi = mb_info[mbi].cneighbors[i];
            mcenc.candidates[ncandidates][0] = i32::from(mb_info[nmbi].analysis_mv[0][frame][0]);
            mcenc.candidates[ncandidates][1] = i32::from(mb_info[nmbi].analysis_mv[0][frame][1]);
            ncandidates += 1;
        }
    }
    let accum_x = oc_mv_x(accum);
    let accum_y = oc_mv_y(accum);
    mcenc.candidates[ncandidates] = [accum_x, accum_y];
    ncandidates += 1;
    mcenc.candidates[ncandidates] = [
        clamp31(i32::from(mb_info[mbi].analysis_mv[1][frame][0]) + accum_x),
        clamp31(i32::from(mb_info[mbi].analysis_mv[1][frame][1]) + accum_y),
    ];
    ncandidates += 1;
    mcenc.candidates[ncandidates] = [0, 0];
    ncandidates += 1;

    let mut a = [
        mcenc.candidates[1],
        mcenc.candidates[2],
        mcenc.candidates[3],
    ];
    a.sort_by_key(|v| v[0]);
    let mx = a[1][0];
    a.sort_by_key(|v| v[1]);
    let my = a[1][1];
    mcenc.candidates[0] = [mx, my];
    mcenc.setb0 = ncandidates as i32;
}

pub fn oc_mcenc_find_candidates_b(
    mb_info: &[MbEncInfo],
    mcenc: &mut McEncCtx,
    accum: OcMv,
    mbi: usize,
    frame: usize,
) {
    let accum_x = oc_mv_x(accum);
    let accum_y = oc_mv_y(accum);
    let mut ncandidates = mcenc.setb0 as usize;
    mcenc.candidates[ncandidates] = [
        clamp31(
            2 * i32::from(mb_info[mbi].analysis_mv[1][frame][0])
                - i32::from(mb_info[mbi].analysis_mv[2][frame][0])
                + accum_x,
        ),
        clamp31(
            2 * i32::from(mb_info[mbi].analysis_mv[1][frame][1])
                - i32::from(mb_info[mbi].analysis_mv[2][frame][1])
                + accum_y,
        ),
    ];
    ncandidates += 1;
    mcenc.ncandidates = ncandidates as i32;
}

pub fn oc_sad16_halfpel(
    frag_buf_offs: &[isize],
    fragis: &[isize; 4],
    mvoffset0: isize,
    mvoffset1: isize,
    src: &[u8],
    ref_: &[u8],
    ystride: usize,
    best_err: u32,
) -> u32 {
    let mut err = 0u32;
    for &fragi in fragis {
        let frag_offs = frag_buf_offs[fragi as usize] as usize;
        err += oc_enc_frag_sad2_thresh_c(
            &src[frag_offs..],
            &ref_[(frag_offs as isize + mvoffset0) as usize..],
            &ref_[(frag_offs as isize + mvoffset1) as usize..],
            ystride,
            best_err.saturating_sub(err),
        );
    }
    err
}

pub fn oc_satd16_halfpel(
    frag_buf_offs: &[isize],
    fragis: &[isize; 4],
    mvoffset0: isize,
    mvoffset1: isize,
    src: &[u8],
    ref_: &[u8],
    ystride: usize,
) -> u32 {
    let mut err = 0u32;
    for &fragi in fragis {
        let frag_offs = frag_buf_offs[fragi as usize] as usize;
        let mut dc = 0i32;
        err += oc_enc_frag_satd2_c(
            &mut dc,
            &src[frag_offs..],
            &ref_[(frag_offs as isize + mvoffset0) as usize..],
            &ref_[(frag_offs as isize + mvoffset1) as usize..],
            ystride,
        );
        err += dc.unsigned_abs();
    }
    err
}

pub fn oc_mcenc_ysad_check_mbcandidate_fullpel(
    frag_buf_offs: &[isize],
    fragis: &[isize; 4],
    dx: i32,
    dy: i32,
    src: &[u8],
    ref_: &[u8],
    ystride: usize,
    block_err: &mut [u32; 4],
) -> u32 {
    let mvoffset = dx as isize + dy as isize * ystride as isize;
    let mut err = 0u32;
    for (bi, &fragi) in fragis.iter().enumerate() {
        let frag_offs = frag_buf_offs[fragi as usize] as usize;
        let be = oc_enc_frag_sad_c(
            &src[frag_offs..],
            &ref_[(frag_offs as isize + mvoffset) as usize..],
            ystride,
        );
        block_err[bi] = be;
        err += be;
    }
    err
}

pub fn oc_mcenc_ysatd_check_mbcandidate_fullpel(
    frag_buf_offs: &[isize],
    fragis: &[isize; 4],
    dx: i32,
    dy: i32,
    src: &[u8],
    ref_: &[u8],
    ystride: usize,
    nosatd: bool,
) -> i32 {
    let mvoffset = dx as isize + dy as isize * ystride as isize;
    let mut err = 0i32;
    for &fragi in fragis {
        let frag_offs = frag_buf_offs[fragi as usize] as usize;
        if !nosatd {
            let mut dc = 0i32;
            err += oc_enc_frag_satd_c(
                &mut dc,
                &src[frag_offs..],
                &ref_[(frag_offs as isize + mvoffset) as usize..],
                ystride,
            ) as i32;
            err += dc.abs();
        } else {
            err += oc_enc_frag_sad_c(
                &src[frag_offs..],
                &ref_[(frag_offs as isize + mvoffset) as usize..],
                ystride,
            ) as i32;
        }
    }
    err
}

pub fn oc_mcenc_ysatd_check_bcandidate_fullpel(
    frag_offs: isize,
    dx: i32,
    dy: i32,
    src: &[u8],
    ref_: &[u8],
    ystride: usize,
) -> u32 {
    let mut dc = 0i32;
    let err = oc_enc_frag_satd_c(
        &mut dc,
        &src[frag_offs as usize..],
        &ref_[(frag_offs + dx as isize + dy as isize * ystride as isize) as usize..],
        ystride,
    );
    err + dc.unsigned_abs()
}

pub fn oc_mcenc_search_frame(
    mcenc: &mut McEncCtx,
    frag_buf_offs: &[isize],
    fragis: &[isize; 4],
    src: &[u8],
    ref_: &[u8],
    ystride: usize,
    center_mv: OcMv,
) -> (OcMv, u32) {
    let mut best_mv = center_mv;
    let mut best_err = u32::MAX;
    for i in 0..mcenc.ncandidates.max(1) as usize {
        let cand = crate::state::oc_mv(mcenc.candidates[i][0], mcenc.candidates[i][1]);
        let mut block_err = [0u32; 4];
        let err = oc_mcenc_ysad_check_mbcandidate_fullpel(
            frag_buf_offs,
            fragis,
            oc_mv_x(cand),
            oc_mv_y(cand),
            src,
            ref_,
            ystride,
            &mut block_err,
        );
        if err < best_err {
            best_err = err;
            best_mv = cand;
        }
    }
    (best_mv, best_err)
}

pub fn oc_mcenc_ysad_halfpel_mbrefine(
    frag_buf_offs: &[isize],
    fragis: &[isize; 4],
    src: &[u8],
    ref_: &[u8],
    ystride: usize,
    best_mv: OcMv,
    best_err: u32,
) -> (OcMv, u32) {
    let mut best_mv = best_mv;
    let mut best_err = best_err;
    let bx = oc_mv_x(best_mv);
    let by = oc_mv_y(best_mv);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let mvoff0 = (by + dy) as isize * ystride as isize + (bx + dx) as isize;
            let mvoff1 =
                (by + dy + ((dx ^ dy) & 1)) as isize * ystride as isize + (bx + dx) as isize;
            let err = oc_sad16_halfpel(
                frag_buf_offs,
                fragis,
                mvoff0,
                mvoff1,
                src,
                ref_,
                ystride,
                best_err,
            );
            if err < best_err {
                best_err = err;
                best_mv = crate::state::oc_mv(bx + dx, by + dy);
            }
        }
    }
    (best_mv, best_err)
}

pub fn oc_mcenc_ysatd_halfpel_mbrefine(
    frag_buf_offs: &[isize],
    fragis: &[isize; 4],
    src: &[u8],
    ref_: &[u8],
    ystride: usize,
    best_mv: OcMv,
) -> (OcMv, u32) {
    let mut best_mv = best_mv;
    let mut best_err = u32::MAX;
    let bx = oc_mv_x(best_mv);
    let by = oc_mv_y(best_mv);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let mvoff0 = (by + dy) as isize * ystride as isize + (bx + dx) as isize;
            let mvoff1 =
                (by + dy + ((dx ^ dy) & 1)) as isize * ystride as isize + (bx + dx) as isize;
            let err = oc_satd16_halfpel(frag_buf_offs, fragis, mvoff0, mvoff1, src, ref_, ystride);
            if err < best_err {
                best_err = err;
                best_mv = crate::state::oc_mv(bx + dx, by + dy);
            }
        }
    }
    (best_mv, best_err)
}

pub fn oc_mcenc_refine1mv(
    frag_buf_offs: &[isize],
    fragis: &[isize; 4],
    src: &[u8],
    ref_: &[u8],
    ystride: usize,
    seed_mv: OcMv,
) -> (OcMv, u32) {
    let mut block_err = [0u32; 4];
    let full = oc_mcenc_ysad_check_mbcandidate_fullpel(
        frag_buf_offs,
        fragis,
        oc_mv_x(seed_mv),
        oc_mv_y(seed_mv),
        src,
        ref_,
        ystride,
        &mut block_err,
    );
    oc_mcenc_ysad_halfpel_mbrefine(frag_buf_offs, fragis, src, ref_, ystride, seed_mv, full)
}

pub fn oc_mcenc_ysad_halfpel_brefine(
    src: &[u8],
    ref_: &[u8],
    ystride: usize,
    seed_mv: OcMv,
) -> (OcMv, u32) {
    let mut best_mv = seed_mv;
    let mut best_err = u32::MAX;
    let bx = oc_mv_x(seed_mv);
    let by = oc_mv_y(seed_mv);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let off0 = (by + dy) as isize * ystride as isize + (bx + dx) as isize;
            let off1 = (by + dy + ((dx ^ dy) & 1)) as isize * ystride as isize + (bx + dx) as isize;
            let err = oc_enc_frag_sad2_thresh_c(
                src,
                &ref_[off0.max(0) as usize..],
                &ref_[off1.max(0) as usize..],
                ystride,
                best_err,
            );
            if err < best_err {
                best_err = err;
                best_mv = crate::state::oc_mv(bx + dx, by + dy);
            }
        }
    }
    (best_mv, best_err)
}

pub fn oc_mcenc_ysatd_halfpel_brefine(
    src: &[u8],
    ref_: &[u8],
    ystride: usize,
    seed_mv: OcMv,
) -> (OcMv, u32) {
    let mut best_mv = seed_mv;
    let mut best_err = u32::MAX;
    let bx = oc_mv_x(seed_mv);
    let by = oc_mv_y(seed_mv);
    for dy in -1..=1 {
        for dx in -1..=1 {
            let off0 = (by + dy) as isize * ystride as isize + (bx + dx) as isize;
            let off1 = (by + dy + ((dx ^ dy) & 1)) as isize * ystride as isize + (bx + dx) as isize;
            let mut dc = 0i32;
            let err = oc_enc_frag_satd2_c(
                &mut dc,
                src,
                &ref_[off0.max(0) as usize..],
                &ref_[off1.max(0) as usize..],
                ystride,
            ) + dc.unsigned_abs();
            if err < best_err {
                best_err = err;
                best_mv = crate::state::oc_mv(bx + dx, by + dy);
            }
        }
    }
    (best_mv, best_err)
}

pub fn oc_mcenc_refine4mv(
    blocks: &[&[u8]; 4],
    refs: &[&[u8]; 4],
    ystride: usize,
    seed_mvs: [OcMv; 4],
) -> ([OcMv; 4], u32) {
    let mut out = seed_mvs;
    let mut total = 0u32;
    for bi in 0..4 {
        let (mv, err) = oc_mcenc_ysad_halfpel_brefine(blocks[bi], refs[bi], ystride, seed_mvs[bi]);
        out[bi] = mv;
        total += err;
    }
    (out, total)
}

pub fn oc_mcenc_search(
    mcenc: &mut McEncCtx,
    frag_buf_offs: &[isize],
    fragis: &[isize; 4],
    src: &[u8],
    ref_: &[u8],
    ystride: usize,
    seed_mv: OcMv,
) -> (OcMv, u32) {
    let (mv, _sad) =
        oc_mcenc_search_frame(mcenc, frag_buf_offs, fragis, src, ref_, ystride, seed_mv);
    oc_mcenc_ysatd_halfpel_mbrefine(frag_buf_offs, fragis, src, ref_, ystride, mv)
}
