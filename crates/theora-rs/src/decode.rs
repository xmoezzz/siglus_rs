use crate::bitpack::PackBuf;
use crate::error::{Result, TheoraError};
use crate::fragment::{
    oc_frag_copy_list_c, oc_frag_recon_inter2_c, oc_frag_recon_inter_c, oc_frag_recon_intra_c,
};
use crate::huffdec::huff_token_decode_c;
use crate::idct::idct8x8_c;
use crate::internal::{OC_FZIG_ZAG, OC_MB_MAP_IDXS, OC_MB_MAP_NIDXS};
use crate::state::{
    frame_for_mode, oc_loop_filter_init_c, oc_mv, oc_state_borders_fill,
    oc_state_borders_fill_caps, oc_state_borders_fill_rows, oc_state_get_mv_offsets,
    oc_state_loop_filter_frag_rows_c, OcMv, TheoraState, OC_FRAME_GOLD, OC_FRAME_GOLD_ORIG,
    OC_FRAME_IO, OC_FRAME_NONE, OC_FRAME_PREV, OC_FRAME_PREV_ORIG, OC_FRAME_SELF, OC_INTER_FRAME,
    OC_INTRA_FRAME, OC_MODE_GOLDEN_MV, OC_MODE_INTER_MV, OC_MODE_INTER_MV_FOUR,
    OC_MODE_INTER_MV_LAST, OC_MODE_INTER_MV_LAST2, OC_MODE_INTER_NOMV, OC_MODE_INTRA,
    OC_MODE_INVALID, OC_NMODES,
};

pub const OC_PP_LEVEL_DISABLED: i32 = 0;
pub const OC_PP_LEVEL_TRACKDCQI: i32 = 1;
pub const OC_PP_LEVEL_DEBLOCKY: i32 = 2;
pub const OC_PP_LEVEL_DERINGY: i32 = 3;
pub const OC_PP_LEVEL_SDERINGY: i32 = 4;
pub const OC_PP_LEVEL_DEBLOCKC: i32 = 5;
pub const OC_PP_LEVEL_DERINGC: i32 = 6;
pub const OC_PP_LEVEL_SDERINGC: i32 = 7;
pub const OC_PP_LEVEL_MAX: i32 = 7;

pub const OC_MODE_ALPHABETS: [[u8; OC_NMODES]; 7] = [
    [3, 4, 2, 0, 1, 5, 6, 7],
    [3, 4, 0, 2, 1, 5, 6, 7],
    [3, 2, 4, 0, 1, 5, 6, 7],
    [3, 2, 0, 4, 1, 5, 6, 7],
    [0, 3, 4, 2, 1, 5, 6, 7],
    [0, 5, 3, 4, 2, 1, 6, 7],
    [0, 1, 2, 3, 4, 5, 6, 7],
];

pub const OC_INTERNAL_DCT_TOKEN_EXTRA_BITS: [u8; 15] =
    [12, 4, 3, 3, 4, 4, 5, 5, 8, 8, 8, 8, 3, 3, 6];
pub const OC_DCT_EOB_FINISH: isize = isize::MAX;
pub const OC_DCT_TOKEN_FAT_EOB: usize = 0;
pub const OC_DCT_CW_RLEN_SHIFT: i32 = 0;
pub const OC_DCT_CW_EOB_SHIFT: i32 = 8;
pub const OC_DCT_CW_FLIP_BIT: i32 = 20;
pub const OC_DCT_CW_MAG_SHIFT: i32 = 21;
pub const OC_DCT_CW_FINISH: i32 = 0;

pub const fn oc_dct_cw_pack(eobs: i32, rlen: i32, mag: i32, flip: i32) -> i32 {
    (eobs << OC_DCT_CW_EOB_SHIFT)
        | (rlen << OC_DCT_CW_RLEN_SHIFT)
        | (flip << OC_DCT_CW_FLIP_BIT)
        | ((mag - flip) * (1 << OC_DCT_CW_MAG_SHIFT))
}

pub const fn oc_dct_token_needs_more(token: usize) -> bool {
    token < OC_INTERNAL_DCT_TOKEN_EXTRA_BITS.len()
}
pub const fn oc_dct_token_eb_pos(token: usize) -> i32 {
    ((OC_DCT_CW_EOB_SHIFT - OC_DCT_CW_MAG_SHIFT) & -((token < 2) as i32))
        + (OC_DCT_CW_MAG_SHIFT & -((token < 12) as i32))
}

pub const OC_DCT_CODE_WORD: [i32; 92] = [
    0, 4096, 27262976, 26214400, 44040192, 42991616, 77594624, 76546048, 144703488, 681574400,
    143654912, 680525824, 2097162, -2097142, -1048576, 256, 512, 768, 2097153, -2097151, 2097154,
    -2097150, 2097155, -2097149, 2097156, -2097148, 2097157, -2097147, 4194305, 6291457, -4194303,
    -6291455, 2097158, 2097159, 2097160, 2097161, -2097146, -2097145, -2097144, -2097143, 4194306,
    4194307, 6291458, 6291459, -4194302, -4194301, -6291454, -6291453, -1048576, 1, 2, 3, 4, 5, 6,
    7, 2097152, -2097152, 4194304, -4194304, 6291456, -6291456, 8388608, -8388608, 10485760,
    -10485760, 12582912, -12582912, 14680064, 16777216, -14680064, -16777216, 18874368, 20971520,
    23068672, 25165824, -18874368, -20971520, -23068672, -25165824, 2048, 2304, 2560, 2816, 3072,
    3328, 3584, 3840, 1024, 1280, 1536, 1792,
];

pub const OC_SB_RUN_TREE: [i16; 22] = [
    4,
    -(1 << 8 | 1),
    -(1 << 8 | 1),
    -(1 << 8 | 1),
    -(1 << 8 | 1),
    -(1 << 8 | 1),
    -(1 << 8 | 1),
    -(1 << 8 | 1),
    -(1 << 8 | 1),
    -(3 << 8 | 2),
    -(3 << 8 | 2),
    -(3 << 8 | 3),
    -(3 << 8 | 3),
    -(4 << 8 | 4),
    -(4 << 8 | 5),
    -(4 << 8 | (2 << 4) | 0),
    17,
    2,
    -(2 << 8 | (2 << 4) | 4),
    -(2 << 8 | (2 << 4) | 8),
    -(2 << 8 | (4 << 4) | 12),
    -(2 << 8 | (12 << 4) | 28),
];

pub const OC_BLOCK_RUN_TREE: [i16; 61] = [
    5,
    -(2 << 8 | 1),
    -(2 << 8 | 1),
    -(2 << 8 | 1),
    -(2 << 8 | 1),
    -(2 << 8 | 1),
    -(2 << 8 | 1),
    -(2 << 8 | 1),
    -(2 << 8 | 1),
    -(2 << 8 | 2),
    -(2 << 8 | 2),
    -(2 << 8 | 2),
    -(2 << 8 | 2),
    -(2 << 8 | 2),
    -(2 << 8 | 2),
    -(2 << 8 | 2),
    -(2 << 8 | 2),
    -(3 << 8 | 3),
    -(3 << 8 | 3),
    -(3 << 8 | 3),
    -(3 << 8 | 3),
    -(3 << 8 | 4),
    -(3 << 8 | 4),
    -(3 << 8 | 4),
    -(3 << 8 | 4),
    -(4 << 8 | 5),
    -(4 << 8 | 5),
    -(4 << 8 | 6),
    -(4 << 8 | 6),
    33,
    36,
    39,
    44,
    1,
    -(1 << 8 | 7),
    -(1 << 8 | 8),
    1,
    -(1 << 8 | 9),
    -(1 << 8 | 10),
    2,
    -(2 << 8 | 11),
    -(2 << 8 | 12),
    -(2 << 8 | 13),
    -(2 << 8 | 14),
    4,
    -(4 << 8 | 15),
    -(4 << 8 | 16),
    -(4 << 8 | 17),
    -(4 << 8 | 18),
    -(4 << 8 | 19),
    -(4 << 8 | 20),
    -(4 << 8 | 21),
    -(4 << 8 | 22),
    -(4 << 8 | 23),
    -(4 << 8 | 24),
    -(4 << 8 | 25),
    -(4 << 8 | 26),
    -(4 << 8 | 27),
    -(4 << 8 | 28),
    -(4 << 8 | 29),
    -(4 << 8 | 30),
];

pub const OC_VLC_MODE_TREE: [i16; 26] = [
    4,
    -(1 << 8 | 0),
    -(1 << 8 | 0),
    -(1 << 8 | 0),
    -(1 << 8 | 0),
    -(1 << 8 | 0),
    -(1 << 8 | 0),
    -(1 << 8 | 0),
    -(1 << 8 | 0),
    -(2 << 8 | 1),
    -(2 << 8 | 1),
    -(2 << 8 | 1),
    -(2 << 8 | 1),
    -(3 << 8 | 2),
    -(3 << 8 | 2),
    -(4 << 8 | 3),
    17,
    3,
    -(1 << 8 | 4),
    -(1 << 8 | 4),
    -(1 << 8 | 4),
    -(1 << 8 | 4),
    -(2 << 8 | 5),
    -(2 << 8 | 5),
    -(3 << 8 | 6),
    -(3 << 8 | 7),
];

pub const OC_CLC_MODE_TREE: [i16; 9] = [
    3,
    -(3 << 8 | 0),
    -(3 << 8 | 1),
    -(3 << 8 | 2),
    -(3 << 8 | 3),
    -(3 << 8 | 4),
    -(3 << 8 | 5),
    -(3 << 8 | 6),
    -(3 << 8 | 7),
];

pub const OC_VLC_MV_COMP_TREE: [i16; 101] = [
    5,
    -(3 << 8 | 32),
    -(3 << 8 | 32),
    -(3 << 8 | 32),
    -(3 << 8 | 32),
    -(3 << 8 | 33),
    -(3 << 8 | 33),
    -(3 << 8 | 33),
    -(3 << 8 | 33),
    -(3 << 8 | 31),
    -(3 << 8 | 31),
    -(3 << 8 | 31),
    -(3 << 8 | 31),
    -(4 << 8 | 34),
    -(4 << 8 | 34),
    -(4 << 8 | 30),
    -(4 << 8 | 30),
    -(4 << 8 | 35),
    -(4 << 8 | 35),
    -(4 << 8 | 29),
    -(4 << 8 | 29),
    33,
    36,
    39,
    42,
    45,
    50,
    55,
    60,
    65,
    74,
    83,
    92,
    1,
    -(1 << 8 | 36),
    -(1 << 8 | 28),
    1,
    -(1 << 8 | 37),
    -(1 << 8 | 27),
    1,
    -(1 << 8 | 38),
    -(1 << 8 | 26),
    1,
    -(1 << 8 | 39),
    -(1 << 8 | 25),
    2,
    -(2 << 8 | 40),
    -(2 << 8 | 24),
    -(2 << 8 | 41),
    -(2 << 8 | 23),
    2,
    -(2 << 8 | 42),
    -(2 << 8 | 22),
    -(2 << 8 | 43),
    -(2 << 8 | 21),
    2,
    -(2 << 8 | 44),
    -(2 << 8 | 20),
    -(2 << 8 | 45),
    -(2 << 8 | 19),
    2,
    -(2 << 8 | 46),
    -(2 << 8 | 18),
    -(2 << 8 | 47),
    -(2 << 8 | 17),
    3,
    -(3 << 8 | 48),
    -(3 << 8 | 16),
    -(3 << 8 | 49),
    -(3 << 8 | 15),
    -(3 << 8 | 50),
    -(3 << 8 | 14),
    -(3 << 8 | 51),
    -(3 << 8 | 13),
    3,
    -(3 << 8 | 52),
    -(3 << 8 | 12),
    -(3 << 8 | 53),
    -(3 << 8 | 11),
    -(3 << 8 | 54),
    -(3 << 8 | 10),
    -(3 << 8 | 55),
    -(3 << 8 | 9),
    3,
    -(3 << 8 | 56),
    -(3 << 8 | 8),
    -(3 << 8 | 57),
    -(3 << 8 | 7),
    -(3 << 8 | 58),
    -(3 << 8 | 6),
    -(3 << 8 | 59),
    -(3 << 8 | 5),
    3,
    -(3 << 8 | 60),
    -(3 << 8 | 4),
    -(3 << 8 | 61),
    -(3 << 8 | 3),
    -(3 << 8 | 62),
    -(3 << 8 | 2),
    -(3 << 8 | 63),
    -(3 << 8 | 1),
];

pub const OC_CLC_MV_COMP_TREE: [i16; 65] = [
    6,
    -(6 << 8 | 32),
    -(6 << 8 | 32),
    -(6 << 8 | 33),
    -(6 << 8 | 31),
    -(6 << 8 | 34),
    -(6 << 8 | 30),
    -(6 << 8 | 35),
    -(6 << 8 | 29),
    -(6 << 8 | 36),
    -(6 << 8 | 28),
    -(6 << 8 | 37),
    -(6 << 8 | 27),
    -(6 << 8 | 38),
    -(6 << 8 | 26),
    -(6 << 8 | 39),
    -(6 << 8 | 25),
    -(6 << 8 | 40),
    -(6 << 8 | 24),
    -(6 << 8 | 41),
    -(6 << 8 | 23),
    -(6 << 8 | 42),
    -(6 << 8 | 22),
    -(6 << 8 | 43),
    -(6 << 8 | 21),
    -(6 << 8 | 44),
    -(6 << 8 | 20),
    -(6 << 8 | 45),
    -(6 << 8 | 19),
    -(6 << 8 | 46),
    -(6 << 8 | 18),
    -(6 << 8 | 47),
    -(6 << 8 | 17),
    -(6 << 8 | 48),
    -(6 << 8 | 16),
    -(6 << 8 | 49),
    -(6 << 8 | 15),
    -(6 << 8 | 50),
    -(6 << 8 | 14),
    -(6 << 8 | 51),
    -(6 << 8 | 13),
    -(6 << 8 | 52),
    -(6 << 8 | 12),
    -(6 << 8 | 53),
    -(6 << 8 | 11),
    -(6 << 8 | 54),
    -(6 << 8 | 10),
    -(6 << 8 | 55),
    -(6 << 8 | 9),
    -(6 << 8 | 56),
    -(6 << 8 | 8),
    -(6 << 8 | 57),
    -(6 << 8 | 7),
    -(6 << 8 | 58),
    -(6 << 8 | 6),
    -(6 << 8 | 59),
    -(6 << 8 | 5),
    -(6 << 8 | 60),
    -(6 << 8 | 4),
    -(6 << 8 | 61),
    -(6 << 8 | 3),
    -(6 << 8 | 62),
    -(6 << 8 | 2),
    -(6 << 8 | 63),
    -(6 << 8 | 1),
];

#[derive(Debug, Clone, Default)]
pub struct DecodedFlags {
    pub coded_fragis: Vec<isize>,
    pub uncoded_fragis: Vec<isize>,
    pub ncoded_fragis: [isize; 3],
    pub ntotal_coded_fragis: isize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DecFrameHeader {
    pub frame_type: i8,
    pub nqis: u8,
    pub qis: [u8; 3],
}

pub fn oc_sb_run_unpack(opb: &mut PackBuf<'_>) -> i32 {
    let mut ret = i32::from(huff_token_decode_c(opb, &OC_SB_RUN_TREE));
    if ret >= 0x10 {
        let offs = ret & 0x1F;
        ret = 6 + offs + opb.read((ret - offs) >> 4) as i32;
    }
    ret
}

pub fn oc_block_run_unpack(opb: &mut PackBuf<'_>) -> i32 {
    i32::from(huff_token_decode_c(opb, &OC_BLOCK_RUN_TREE))
}

pub fn oc_dec_frame_header_unpack(opb: &mut PackBuf<'_>) -> Result<DecFrameHeader> {
    if opb.read1() != 0 {
        return Err(TheoraError::BadPacket);
    }
    let frame_type = opb.read1() as i8;
    let mut out = DecFrameHeader {
        frame_type,
        nqis: 1,
        qis: [0; 3],
    };
    out.qis[0] = opb.read(6) as u8;
    if opb.read1() != 0 {
        out.qis[1] = opb.read(6) as u8;
        out.nqis = 2;
        if opb.read1() != 0 {
            out.qis[2] = opb.read(6) as u8;
            out.nqis = 3;
        }
    }
    if frame_type == OC_INTRA_FRAME && opb.read(3) != 0 {
        return Err(TheoraError::NotImplemented);
    }
    Ok(out)
}

pub fn oc_dec_mark_all_intra(state: &mut TheoraState) {
    state.coded_fragis.clear();
    let mut prev_ncoded = 0isize;
    let mut ncoded = 0isize;
    let mut nsbs = 0u32;
    let mut sbi = 0usize;
    for pli in 0..3usize {
        nsbs += state.fplanes[pli].nsbs;
        while sbi < nsbs as usize {
            let quad_valid = state.sb_flags[sbi].quad_valid;
            for quadi in 0..4usize {
                if (quad_valid & (1 << quadi)) == 0 {
                    continue;
                }
                for bi in 0..4usize {
                    let fragi = state.sb_maps[sbi][quadi][bi];
                    if fragi >= 0 {
                        let frag = &mut state.frags[fragi as usize];
                        frag.coded = true;
                        frag.refi = OC_FRAME_SELF as u8;
                        frag.mb_mode = OC_MODE_INTRA;
                        state.coded_fragis.push(fragi);
                        ncoded += 1;
                    }
                }
            }
            sbi += 1;
        }
        state.ncoded_fragis[pli] = ncoded - prev_ncoded;
        prev_ncoded = ncoded;
    }
    state.ntotal_coded_fragis = ncoded;
}

pub fn oc_dec_partial_sb_flags_unpack(opb: &mut PackBuf<'_>, state: &mut TheoraState) -> u32 {
    let mut flag = opb.read1() != 0;
    let mut sbi = 0usize;
    let nsbs = state.nsbs as usize;
    let mut npartial = 0u32;
    while sbi < nsbs {
        let mut run_count = oc_sb_run_unpack(opb);
        let full_run = run_count >= 4129;
        while run_count > 0 && sbi < nsbs {
            state.sb_flags[sbi].coded_partially = flag;
            state.sb_flags[sbi].coded_fully = false;
            npartial += u32::from(flag);
            sbi += 1;
            run_count -= 1;
        }
        if full_run && sbi < nsbs {
            flag = opb.read1() != 0;
        } else {
            flag = !flag;
        }
    }
    npartial
}

pub fn oc_dec_coded_sb_flags_unpack(opb: &mut PackBuf<'_>, state: &mut TheoraState) {
    let nsbs = state.nsbs as usize;
    let mut sbi = 0usize;
    while sbi < nsbs && state.sb_flags[sbi].coded_partially {
        sbi += 1;
    }
    let mut flag = opb.read1() != 0;
    while sbi < nsbs {
        let mut run_count = oc_sb_run_unpack(opb);
        let full_run = run_count >= 4129;
        while sbi < nsbs {
            if state.sb_flags[sbi].coded_partially {
                sbi += 1;
                continue;
            }
            if run_count <= 0 {
                break;
            }
            state.sb_flags[sbi].coded_fully = flag;
            sbi += 1;
            run_count -= 1;
        }
        if full_run && sbi < nsbs {
            flag = opb.read1() != 0;
        } else {
            flag = !flag;
        }
    }
}

pub fn oc_dec_coded_flags_unpack(opb: &mut PackBuf<'_>, state: &mut TheoraState) -> DecodedFlags {
    let npartial = oc_dec_partial_sb_flags_unpack(opb, state);
    if npartial < state.nsbs {
        oc_dec_coded_sb_flags_unpack(opb, state);
    }
    let mut flag = if npartial > 0 {
        opb.read1() == 0
    } else {
        false
    };
    let mut run_count = 0i32;
    let mut out = DecodedFlags::default();
    out.coded_fragis.reserve(state.nfrags as usize);
    let mut uncoded_forward: Vec<isize> = Vec::with_capacity(state.nfrags as usize);
    let mut prev_ncoded = 0isize;
    let mut ncoded = 0isize;
    let mut nsbs = 0u32;
    let mut sbi = 0usize;
    for pli in 0..3usize {
        nsbs += state.fplanes[pli].nsbs;
        while sbi < nsbs as usize {
            let quad_valid = state.sb_flags[sbi].quad_valid;
            for quadi in 0..4usize {
                if (quad_valid & (1 << quadi)) == 0 {
                    continue;
                }
                let mut quad_coded = false;
                for bi in 0..4usize {
                    let fragi = state.sb_maps[sbi][quadi][bi];
                    if fragi < 0 {
                        continue;
                    }
                    let coded = if state.sb_flags[sbi].coded_fully {
                        true
                    } else if !state.sb_flags[sbi].coded_partially {
                        false
                    } else {
                        if run_count <= 0 {
                            run_count = oc_block_run_unpack(opb);
                            flag = !flag;
                        }
                        run_count -= 1;
                        flag
                    };
                    let frag = &mut state.frags[fragi as usize];
                    frag.coded = coded;
                    frag.refi = OC_FRAME_NONE as u8;
                    quad_coded |= coded;
                    if coded {
                        out.coded_fragis.push(fragi);
                        ncoded += 1;
                    } else {
                        uncoded_forward.push(fragi);
                    }
                }
                if pli == 0 {
                    state.mb_modes[(sbi << 2) | quadi] = i8::from(quad_coded);
                }
            }
            sbi += 1;
        }
        out.ncoded_fragis[pli] = ncoded - prev_ncoded;
        prev_ncoded = ncoded;
    }
    uncoded_forward.reverse();
    out.uncoded_fragis = uncoded_forward;
    out.ntotal_coded_fragis = ncoded;
    state.coded_fragis = out.coded_fragis.clone();
    state.uncoded_fragis = out.uncoded_fragis.clone();
    state.ncoded_fragis = out.ncoded_fragis;
    state.ntotal_coded_fragis = out.ntotal_coded_fragis;
    out
}

pub fn oc_dec_mb_modes_unpack(opb: &mut PackBuf<'_>, state: &mut TheoraState) {
    let mode_scheme = opb.read(3) as usize;
    let mut scheme0_alphabet = [OC_MODE_INTER_NOMV as u8; OC_NMODES];
    let alphabet: &[u8; OC_NMODES] = if mode_scheme == 0 {
        for mi in 0..OC_NMODES {
            let val = opb.read(3) as usize;
            if val < OC_NMODES {
                scheme0_alphabet[val] = OC_MODE_ALPHABETS[6][mi];
            }
        }
        &scheme0_alphabet
    } else {
        &OC_MODE_ALPHABETS[mode_scheme - 1]
    };
    let mode_tree = if mode_scheme == 7 {
        &OC_CLC_MODE_TREE[..]
    } else {
        &OC_VLC_MODE_TREE[..]
    };
    for mbi in 0..state.nmbs {
        if state.mb_modes[mbi] > 0 {
            let mode = huff_token_decode_c(opb, mode_tree) as usize;
            state.mb_modes[mbi] = alphabet[mode.min(OC_NMODES - 1)] as i8;
        }
    }
}

pub fn oc_mv_unpack(opb: &mut PackBuf<'_>, tree: &[i16]) -> OcMv {
    let dx = i32::from(huff_token_decode_c(opb, tree)) - 32;
    let dy = i32::from(huff_token_decode_c(opb, tree)) - 32;
    oc_mv(dx, dy)
}

pub fn oc_dec_mv_unpack_and_frag_modes_fill(opb: &mut PackBuf<'_>, state: &mut TheoraState) {
    let use_clc = opb.read1() != 0;
    let mv_comp_tree = if use_clc {
        &OC_CLC_MV_COMP_TREE[..]
    } else {
        &OC_VLC_MV_COMP_TREE[..]
    };
    let pf = state.info.pixel_fmt as usize;
    let map_idxs = &OC_MB_MAP_IDXS[pf];
    let map_nidxs = OC_MB_MAP_NIDXS[pf] as usize;
    let mut prior_mv = oc_mv(0, 0);
    let mut last_mv = oc_mv(0, 0);
    for mbi in 0..state.nmbs {
        let mb_mode = state.mb_modes[mbi];
        if mb_mode == OC_MODE_INVALID {
            continue;
        }
        if mb_mode == OC_MODE_INTER_MV_FOUR {
            let mut lbmvs = [oc_mv(0, 0); 4];
            prior_mv = last_mv;
            for bi in 0..4usize {
                let fragi = state.mb_maps[mbi][0][bi];
                if fragi >= 0 && state.frags[fragi as usize].coded {
                    let mv = oc_mv_unpack(opb, mv_comp_tree);
                    last_mv = mv;
                    lbmvs[bi] = mv;
                    state.frag_mvs[fragi as usize] = mv;
                    let frag = &mut state.frags[fragi as usize];
                    frag.refi = OC_FRAME_PREV as u8;
                    frag.mb_mode = OC_MODE_INTER_MV_FOUR;
                }
            }
            let mut cbmvs = lbmvs;
            crate::state::oc_state_set_chroma_mvs(&mut cbmvs, &lbmvs, state.info.pixel_fmt as i32);
            for mapii in 4..map_nidxs {
                let mapi = map_idxs[mapii] as usize;
                let pli = mapi >> 2;
                let bi = mapi & 3;
                let fragi = state.mb_maps[mbi][pli][bi];
                if fragi >= 0 && state.frags[fragi as usize].coded {
                    state.frag_mvs[fragi as usize] = cbmvs[bi];
                    let frag = &mut state.frags[fragi as usize];
                    frag.refi = OC_FRAME_PREV as u8;
                    frag.mb_mode = OC_MODE_INTER_MV_FOUR;
                }
            }
        } else {
            let mbmv = match mb_mode {
                OC_MODE_INTER_MV => {
                    prior_mv = last_mv;
                    last_mv = oc_mv_unpack(opb, mv_comp_tree);
                    last_mv
                }
                OC_MODE_INTER_MV_LAST => last_mv,
                OC_MODE_INTER_MV_LAST2 => {
                    let mbmv = prior_mv;
                    prior_mv = last_mv;
                    last_mv = mbmv;
                    mbmv
                }
                OC_MODE_GOLDEN_MV => oc_mv_unpack(opb, mv_comp_tree),
                _ => oc_mv(0, 0),
            };
            let refi = frame_for_mode(mb_mode) as u8;
            for &mapi in map_idxs.iter().take(map_nidxs) {
                let mapi = mapi as usize;
                let fragi = state.mb_maps[mbi][mapi >> 2][mapi & 3];
                if fragi >= 0 && state.frags[fragi as usize].coded {
                    state.frag_mvs[fragi as usize] = mbmv;
                    let frag = &mut state.frags[fragi as usize];
                    frag.refi = refi;
                    frag.mb_mode = mb_mode;
                }
            }
        }
    }
}

use crate::codec::{ImgPlane, YCbCrBuffer};
use crate::decint::DecContext;
use crate::packet::OggPacket;

pub fn oc_dec_block_qis_unpack(
    opb: &mut PackBuf<'_>,
    frags: &mut [crate::state::Fragment],
    coded_fragis: &[isize],
    nqis: u8,
) {
    let ncoded_fragis = coded_fragis.len();
    if ncoded_fragis == 0 {
        return;
    }
    if nqis == 1 {
        for fragii in 0..ncoded_fragis {
            let fragi = coded_fragis[fragii];
            assert!(
                fragi >= 0,
                "negative coded fragi in oc_dec_block_qis_unpack"
            );
            frags[fragi as usize].qii = 0;
        }
    } else {
        let mut flag = (opb.read1() != 0) as u8;
        let mut nqi1 = 0usize;
        let mut fragii = 0usize;
        while fragii < ncoded_fragis {
            let run_count = oc_sb_run_unpack(opb);
            let full_run = run_count >= 4129;
            let mut run_count_left = run_count;
            loop {
                let fragi = coded_fragis[fragii];
                assert!(
                    fragi >= 0,
                    "negative coded fragi in oc_dec_block_qis_unpack pass1"
                );
                frags[fragi as usize].qii = flag;
                nqi1 += flag as usize;
                fragii += 1;
                run_count_left -= 1;
                if !(run_count_left > 0 && fragii < ncoded_fragis) {
                    break;
                }
            }
            if full_run && fragii < ncoded_fragis {
                flag = (opb.read1() != 0) as u8;
            } else {
                flag ^= 1;
            }
        }
        if nqis == 3 && nqi1 > 0 {
            fragii = 0;
            while fragii < ncoded_fragis {
                let fragi = coded_fragis[fragii];
                assert!(
                    fragi >= 0,
                    "negative coded fragi in oc_dec_block_qis_unpack pass2-scan"
                );
                if frags[fragi as usize].qii != 0 {
                    break;
                }
                fragii += 1;
            }
            flag = (opb.read1() != 0) as u8;
            while fragii < ncoded_fragis {
                let run_count = oc_sb_run_unpack(opb);
                let full_run = run_count >= 4129;
                let mut run_count_left = run_count;
                while fragii < ncoded_fragis {
                    let fragi = coded_fragis[fragii];
                    assert!(
                        fragi >= 0,
                        "negative coded fragi in oc_dec_block_qis_unpack pass2"
                    );
                    let fragi = fragi as usize;
                    if frags[fragi].qii == 0 {
                        fragii += 1;
                        continue;
                    }
                    if run_count_left <= 0 {
                        break;
                    }
                    frags[fragi].qii += flag;
                    run_count_left -= 1;
                    fragii += 1;
                }
                if full_run && fragii < ncoded_fragis {
                    flag = (opb.read1() != 0) as u8;
                } else {
                    flag ^= 1;
                }
            }
        }
    }
}

fn oc_dec_dc_coeff_unpack_frame(
    ctx: &mut DecContext,
    opb: &mut PackBuf<'_>,
    huff_idxs: [usize; 2],
    ntoks_left: &mut [[isize; 64]; 3],
) -> Result<isize> {
    let setup = ctx.setup.as_ref().ok_or(TheoraError::BadHeader)?;
    let frags = &mut ctx.state.frags;
    let coded_fragis = &ctx.state.coded_fragis;
    let mut ncoded_fragis = 0isize;
    let mut fragii = 0isize;
    let mut eobs = 0isize;
    let mut ti = 0isize;
    for pli in 0..3usize {
        let mut run_counts = [0isize; 64];
        ncoded_fragis += ctx.state.ncoded_fragis[pli];
        ctx.eob_runs[pli][0] = eobs;
        ctx.ti0[pli][0] = ti;
        let mut eobi = eobs.min(ncoded_fragis - fragii);
        let mut eob_count = eobi;
        eobs -= eobi;
        while eobi > 0 {
            frags[coded_fragis[fragii as usize] as usize].dc = 0;
            fragii += 1;
            eobi -= 1;
        }
        while fragii < ncoded_fragis {
            let tree = setup
                .huff_tables
                .get(huff_idxs[(pli + 1) >> 1])
                .ok_or(TheoraError::BadHeader)?;
            let token = huff_token_decode_c(opb, tree) as usize;
            ctx.dct_tokens.push(token as u8);
            ti += 1;
            let eb = if oc_dct_token_needs_more(token) {
                let eb = opb.read(OC_INTERNAL_DCT_TOKEN_EXTRA_BITS[token] as i32) as i32;
                ctx.dct_tokens.push(eb as u8);
                ti += 1;
                if token == OC_DCT_TOKEN_FAT_EOB {
                    ctx.dct_tokens.push((eb >> 8) as u8);
                    ti += 1;
                }
                eb << oc_dct_token_eb_pos(token)
            } else {
                0
            };
            let mut cw = OC_DCT_CODE_WORD[token] + eb;
            eobs = ((cw >> OC_DCT_CW_EOB_SHIFT) & 0xFFF) as isize;
            if cw == OC_DCT_CW_FINISH {
                eobs = OC_DCT_EOB_FINISH;
            }
            if eobs != 0 {
                let mut eobi = eobs.min(ncoded_fragis - fragii);
                eob_count += eobi;
                eobs -= eobi;
                while eobi > 0 {
                    frags[coded_fragis[fragii as usize] as usize].dc = 0;
                    fragii += 1;
                    eobi -= 1;
                }
            } else {
                let skip = ((cw >> OC_DCT_CW_RLEN_SHIFT) & 0xFF) as usize;
                {
                    let flip = cw & (1 << OC_DCT_CW_FLIP_BIT);
                    cw ^= -flip;
                }
                let mut coeff = (cw >> OC_DCT_CW_MAG_SHIFT) as i16;
                if skip != 0 {
                    coeff = 0;
                }
                run_counts[skip] += 1;
                frags[coded_fragis[fragii as usize] as usize].dc = coeff;
                fragii += 1;
            }
        }
        run_counts[63] += eob_count;
        for rli in (0..63).rev() {
            run_counts[rli] += run_counts[rli + 1];
        }
        for rli in (0..64).rev() {
            ntoks_left[pli][rli] -= run_counts[rli];
        }
    }
    ctx.dct_tokens_count = ti as i32;
    Ok(eobs)
}

fn oc_dec_ac_coeff_unpack_frame(
    ctx: &mut DecContext,
    opb: &mut PackBuf<'_>,
    zzi: usize,
    huff_idxs: [usize; 2],
    ntoks_left: &mut [[isize; 64]; 3],
    mut eobs: isize,
) -> Result<isize> {
    let setup = ctx.setup.as_ref().ok_or(TheoraError::BadHeader)?;
    let mut ti = ctx.dct_tokens_count as isize;
    for pli in 0..3usize {
        let tree = setup
            .huff_tables
            .get(huff_idxs[(pli + 1) >> 1])
            .ok_or(TheoraError::BadHeader)?;
        ctx.eob_runs[pli][zzi] = eobs;
        ctx.ti0[pli][zzi] = ti;
        let ntoks_total = ntoks_left[pli][zzi].max(0) as usize;
        let mut run_counts = [0isize; 64];
        let mut eob_count = 0isize;
        let mut ntoks = 0usize;
        while ntoks + (eobs.max(0) as usize) < ntoks_total {
            ntoks += eobs.max(0) as usize;
            eob_count += eobs.max(0);
            let token = huff_token_decode_c(opb, tree) as usize;
            ctx.dct_tokens.push(token as u8);
            ti += 1;
            let eb = if oc_dct_token_needs_more(token) {
                let eb = opb.read(OC_INTERNAL_DCT_TOKEN_EXTRA_BITS[token] as i32) as i32;
                ctx.dct_tokens.push(eb as u8);
                ti += 1;
                if token == OC_DCT_TOKEN_FAT_EOB {
                    ctx.dct_tokens.push((eb >> 8) as u8);
                    ti += 1;
                }
                eb << oc_dct_token_eb_pos(token)
            } else {
                0
            };
            let cw = OC_DCT_CODE_WORD[token] + eb;
            let skip = ((cw >> OC_DCT_CW_RLEN_SHIFT) & 0xFF) as usize;
            eobs = ((cw >> OC_DCT_CW_EOB_SHIFT) & 0xFFF) as isize;
            if cw == OC_DCT_CW_FINISH {
                eobs = OC_DCT_EOB_FINISH;
            }
            if eobs == 0 {
                run_counts[skip] += 1;
                ntoks += 1;
            }
        }
        eob_count += ntoks_total.saturating_sub(ntoks) as isize;
        eobs -= ntoks_total.saturating_sub(ntoks) as isize;
        run_counts[63] += eob_count;
        for rli in (0..63).rev() {
            run_counts[rli] += run_counts[rli + 1];
        }
        for rli in (0..(64 - zzi)).rev() {
            ntoks_left[pli][zzi + rli] -= run_counts[rli];
        }
    }
    ctx.dct_tokens_count = ti as i32;
    Ok(eobs)
}

pub fn oc_dec_residual_tokens_unpack(ctx: &mut DecContext, opb: &mut PackBuf<'_>) -> Result<()> {
    const HUFF_LIST_MAX: [usize; 5] = [1, 6, 15, 28, 64];
    let mut ntoks_left = [[0isize; 64]; 3];
    for pli in 0..3usize {
        for zzi in 0..64usize {
            ntoks_left[pli][zzi] = ctx.state.ncoded_fragis[pli];
        }
    }
    ctx.dct_tokens.clear();
    let mut huff_idxs = [opb.read(4) as usize, opb.read(4) as usize];
    let mut eobs = oc_dec_dc_coeff_unpack_frame(ctx, opb, huff_idxs, &mut ntoks_left)?;
    huff_idxs = [opb.read(4) as usize, opb.read(4) as usize];
    let mut zzi = 1usize;
    for hgi in 1..5usize {
        huff_idxs[0] += 16;
        huff_idxs[1] += 16;
        while zzi < HUFF_LIST_MAX[hgi] {
            eobs = oc_dec_ac_coeff_unpack_frame(ctx, opb, zzi, huff_idxs, &mut ntoks_left, eobs)?;
            zzi += 1;
        }
    }
    Ok(())
}

fn oc_dec_dc_unpredict_frame_plane(ctx: &mut DecContext, pli: usize) {
    let fplane = ctx.state.fplanes[pli];
    let fragy0 = ctx.pipe.fragy0[pli];
    let fragy_end = ctx.pipe.fragy_end[pli];
    let nhfrags = fplane.nhfrags as isize;
    let frags = &mut ctx.state.frags;
    let pred_last = &mut ctx.pipe.pred_last[pli];
    let mut coded = Vec::new();
    let mut uncoded = Vec::new();
    let mut ncoded_fragis = 0isize;
    let mut fragi = fplane.froffset + fragy0 as isize * nhfrags;
    for fragy in fragy0..fragy_end {
        if fragy == 0 {
            for _ in 0..fplane.nhfrags {
                let fi = fragi as usize;
                if frags[fi].coded {
                    let refi = frags[fi].refi.min(3) as usize;
                    let dc = i32::from(frags[fi].dc) + pred_last[refi];
                    frags[fi].dc = dc.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                    pred_last[refi] = i32::from(frags[fi].dc);
                    coded.push(fragi);
                    ncoded_fragis += 1;
                } else {
                    uncoded.push(fragi);
                }
                fragi += 1;
            }
        } else {
            let mut l_ref = -1i32;
            let mut ul_ref = -1i32;
            let mut u_ref = frags[(fragi - nhfrags) as usize].refi as i32;
            for fragx in 0..fplane.nhfrags {
                let ur_ref = if fragx + 1 >= fplane.nhfrags {
                    -1
                } else {
                    frags[(fragi - nhfrags + fragx as isize + 1) as usize].refi as i32
                };
                let fi = fragi as usize;
                if frags[fi].coded {
                    let refi = frags[fi].refi as i32;
                    let pred = match ((l_ref == refi) as i32)
                        | (((ul_ref == refi) as i32) << 1)
                        | (((u_ref == refi) as i32) << 2)
                        | (((ur_ref == refi) as i32) << 3)
                    {
                        1 | 3 => i32::from(frags[(fragi - 1) as usize].dc),
                        2 => i32::from(frags[(fragi - nhfrags - 1) as usize].dc),
                        4 | 6 | 12 => i32::from(frags[(fragi - nhfrags) as usize].dc),
                        5 => {
                            (i32::from(frags[(fragi - 1) as usize].dc)
                                + i32::from(frags[(fragi - nhfrags) as usize].dc))
                                / 2
                        }
                        8 => i32::from(frags[(fragi - nhfrags + 1) as usize].dc),
                        9 | 11 | 13 => {
                            (75 * i32::from(frags[(fragi - 1) as usize].dc)
                                + 53 * i32::from(frags[(fragi - nhfrags + 1) as usize].dc))
                                / 128
                        }
                        10 => {
                            (i32::from(frags[(fragi - nhfrags - 1) as usize].dc)
                                + i32::from(frags[(fragi - nhfrags + 1) as usize].dc))
                                / 2
                        }
                        14 => {
                            (3 * (i32::from(frags[(fragi - nhfrags - 1) as usize].dc)
                                + i32::from(frags[(fragi - nhfrags + 1) as usize].dc))
                                + 10 * i32::from(frags[(fragi - nhfrags) as usize].dc))
                                / 16
                        }
                        7 | 15 => {
                            let p0 = i32::from(frags[(fragi - 1) as usize].dc);
                            let p1 = i32::from(frags[(fragi - nhfrags - 1) as usize].dc);
                            let p2 = i32::from(frags[(fragi - nhfrags) as usize].dc);
                            let mut pred = (29 * (p0 + p2) - 26 * p1) / 32;
                            if (pred - p2).abs() > 128 {
                                pred = p2;
                            } else if (pred - p0).abs() > 128 {
                                pred = p0;
                            } else if (pred - p1).abs() > 128 {
                                pred = p1;
                            }
                            pred
                        }
                        _ => pred_last[refi.clamp(0, 3) as usize],
                    };
                    let refu = refi.clamp(0, 3) as usize;
                    let dc = i32::from(frags[fi].dc) + pred;
                    frags[fi].dc = dc.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
                    pred_last[refu] = i32::from(frags[fi].dc);
                    coded.push(fragi);
                    ncoded_fragis += 1;
                    l_ref = refi;
                } else {
                    uncoded.push(fragi);
                    l_ref = -1;
                }
                ul_ref = u_ref;
                u_ref = ur_ref;
                fragi += 1;
            }
        }
    }
    ctx.pipe.coded_fragis[pli] = coded;
    ctx.pipe.uncoded_fragis[pli] = uncoded;
    ctx.pipe.ncoded_fragis[pli] = ncoded_fragis;
    ctx.pipe.nuncoded_fragis[pli] = (fragy_end - fragy0) as isize * nhfrags - ncoded_fragis;
}

pub fn oc_dec_postprocess_init(ctx: &mut DecContext) {
    let flimit = i32::from(ctx.state.loop_filter_limits[ctx.state.qis[0] as usize]);
    ctx.pipe.loop_filter = i32::from(flimit != 0);
    ctx.pipe.pp_level = ctx.pp_level;
    ctx.pipe.bounding_values = [0; 256];
    if flimit != 0 {
        oc_loop_filter_init_c(&mut ctx.pipe.bounding_values, flimit);
    }
}

pub fn oc_dec_pipeline_init(ctx: &mut DecContext) {
    ctx.pipe.mcu_nvfrags = match ctx.state.info.pixel_fmt {
        crate::codec::PixelFmt::Pf420 | crate::codec::PixelFmt::Reserved => 8,
        crate::codec::PixelFmt::Pf422 | crate::codec::PixelFmt::Pf444 => 4,
    };
    ctx.pipe.ti = ctx.ti0;
    ctx.pipe.eob_runs = ctx.eob_runs;
    let mut coded_off = 0usize;
    let mut uncoded_off = ctx.state.uncoded_fragis.len();
    for pli in 0..3usize {
        ctx.pipe.coded_fragis[pli].clear();
        ctx.pipe.uncoded_fragis[pli].clear();
        ctx.pipe.coded_fragis_off[pli] = coded_off;
        ctx.pipe.uncoded_fragis_off[pli] = uncoded_off;
        ctx.pipe.ncoded_fragis[pli] = 0;
        ctx.pipe.nuncoded_fragis[pli] = 0;
        for qii in 0..ctx.state.nqis.min(3) as usize {
            for qti in 0..2usize {
                ctx.pipe.dequant[pli][qii][qti] =
                    ctx.state.dequant_tables[ctx.state.qis[qii] as usize][pli][qti];
            }
        }
        coded_off += ctx.state.ncoded_fragis[pli].max(0) as usize;
        let plane_nuncoded =
            (ctx.state.fplanes[pli].nfrags as isize - ctx.state.ncoded_fragis[pli]).max(0) as usize;
        uncoded_off = uncoded_off.saturating_sub(plane_nuncoded);
    }
    ctx.pipe.pred_last = [[0; 4]; 3];
    let flimit = i32::from(ctx.state.loop_filter_limits[ctx.state.qis[0] as usize]);
    ctx.pipe.loop_filter = i32::from(flimit != 0);
    if flimit != 0 {
        oc_loop_filter_init_c(&mut ctx.pipe.bounding_values, flimit);
    } else {
        ctx.pipe.bounding_values = [0; 256];
    }
    ctx.pipe.pp_level = if ctx.pp_level > OC_PP_LEVEL_DISABLED {
        ctx.pp_level
    } else {
        OC_PP_LEVEL_DISABLED
    };
    if ctx.pipe.pp_level == OC_PP_LEVEL_DISABLED {
        let self_idx = ctx.state.ref_frame_idx[OC_FRAME_SELF as usize];
        if self_idx >= 0 {
            ctx.pp_frame_buf = ctx.state.ref_frame_bufs[self_idx as usize].clone();
        }
    }
    ctx.pipe.dct_coeffs[..64].fill(0);
}

fn assign_frame_role(ctx: &mut DecContext, role: usize, slot: i32) {
    ctx.state.ref_frame_idx[role] = slot;
}

fn pp_is_disabled(ctx: &DecContext) -> bool {
    ctx.pipe.pp_level == OC_PP_LEVEL_DISABLED || ctx.pp_level <= OC_PP_LEVEL_DISABLED
}

fn sync_pp_from_self(ctx: &mut DecContext) {
    let self_idx = ctx.state.ref_frame_idx[OC_FRAME_SELF as usize];
    if self_idx >= 0 {
        ctx.pp_frame_buf = ctx.state.ref_frame_bufs[self_idx as usize].clone();
    }
}

fn oc_dec_init_dummy_frame(ctx: &mut DecContext) {
    let info = &ctx.state.info;
    let yhstride = ctx.state.ref_ystride[0].unsigned_abs() as usize;
    let yheight = info.frame_height as usize + 2 * crate::state::OC_UMV_PADDING as usize;
    let chstride = ctx.state.ref_ystride[1].unsigned_abs() as usize;
    let cheight = yheight
        >> if (info.pixel_fmt as i32 & 2) == 0 {
            1
        } else {
            0
        };

    assign_frame_role(ctx, OC_FRAME_GOLD as usize, 0);
    assign_frame_role(ctx, OC_FRAME_PREV as usize, 0);
    assign_frame_role(ctx, OC_FRAME_SELF as usize, 0);
    assign_frame_role(ctx, OC_FRAME_GOLD_ORIG as usize, -1);
    assign_frame_role(ctx, OC_FRAME_PREV_ORIG as usize, -1);
    assign_frame_role(ctx, OC_FRAME_IO as usize, -1);

    for pli in 0..3usize {
        if ctx.state.ref_frame_bufs[0][pli].data.is_empty() {
            continue;
        }
        let fill_len = if pli == 0 {
            yhstride * yheight
        } else {
            chstride * cheight
        };
        let plane = &mut ctx.state.ref_frame_bufs[0][pli];
        let actual_fill_len = fill_len.min(plane.data.len());
        plane.data[..actual_fill_len].fill(0x80);
    }
    ctx.pp_frame_buf = ctx.state.ref_frame_bufs[0].clone();
}

pub fn oc_dec_dc_unpredict_mcu_plane_c(ctx: &mut DecContext, pli: usize) {
    let fplane = ctx.state.fplanes[pli];
    let fragy0 = ctx.pipe.fragy0[pli];
    let fragy_end = ctx.pipe.fragy_end[pli];
    let nhfrags = fplane.nhfrags as isize;
    let pred_last = &mut ctx.pipe.pred_last[pli];
    let frags = &mut ctx.state.frags;
    let mut ncoded_fragis = 0isize;
    let mut fragi = fplane.froffset + fragy0 as isize * nhfrags;
    for fragy in fragy0..fragy_end {
        if fragy == 0 {
            for _ in 0..fplane.nhfrags {
                let fi = fragi as usize;
                if frags[fi].coded {
                    let refi = frags[fi].refi as usize;
                    pred_last[refi] = i32::from(frags[fi].dc) + pred_last[refi];
                    frags[fi].dc = pred_last[refi] as i16;
                    ncoded_fragis += 1;
                }
                fragi += 1;
            }
        } else {
            let mut l_ref = -1i32;
            let mut ul_ref = -1i32;
            let mut u_ref = frags[(fragi - nhfrags) as usize].refi as i32;
            for fragx in 0..fplane.nhfrags {
                let ur_ref = if fragx + 1 >= fplane.nhfrags {
                    -1
                } else {
                    frags[(fragi - nhfrags + 1) as usize].refi as i32
                };
                let fi = fragi as usize;
                if frags[fi].coded {
                    let refi = frags[fi].refi as i32;
                    let pred = match ((l_ref == refi) as i32)
                        | (((ul_ref == refi) as i32) << 1)
                        | (((u_ref == refi) as i32) << 2)
                        | (((ur_ref == refi) as i32) << 3)
                    {
                        0 => pred_last[refi as usize],
                        1 | 3 => i32::from(frags[(fragi - 1) as usize].dc),
                        2 => i32::from(frags[(fragi - nhfrags - 1) as usize].dc),
                        4 | 6 | 12 => i32::from(frags[(fragi - nhfrags) as usize].dc),
                        5 => {
                            (i32::from(frags[(fragi - 1) as usize].dc)
                                + i32::from(frags[(fragi - nhfrags) as usize].dc))
                                / 2
                        }
                        8 => i32::from(frags[(fragi - nhfrags + 1) as usize].dc),
                        9 | 11 | 13 => {
                            (75 * i32::from(frags[(fragi - 1) as usize].dc)
                                + 53 * i32::from(frags[(fragi - nhfrags + 1) as usize].dc))
                                / 128
                        }
                        10 => {
                            (i32::from(frags[(fragi - nhfrags - 1) as usize].dc)
                                + i32::from(frags[(fragi - nhfrags + 1) as usize].dc))
                                / 2
                        }
                        14 => {
                            (3 * (i32::from(frags[(fragi - nhfrags - 1) as usize].dc)
                                + i32::from(frags[(fragi - nhfrags + 1) as usize].dc))
                                + 10 * i32::from(frags[(fragi - nhfrags) as usize].dc))
                                / 16
                        }
                        7 | 15 => {
                            let p0 = i32::from(frags[(fragi - 1) as usize].dc);
                            let p1 = i32::from(frags[(fragi - nhfrags - 1) as usize].dc);
                            let p2 = i32::from(frags[(fragi - nhfrags) as usize].dc);
                            let mut pred = (29 * (p0 + p2) - 26 * p1) / 32;
                            if (pred - p2).abs() > 128 {
                                pred = p2;
                            } else if (pred - p0).abs() > 128 {
                                pred = p0;
                            } else if (pred - p1).abs() > 128 {
                                pred = p1;
                            }
                            pred
                        }
                        _ => pred_last[refi as usize],
                    };
                    pred_last[refi as usize] = i32::from(frags[fi].dc) + pred;
                    frags[fi].dc = pred_last[refi as usize] as i16;
                    ncoded_fragis += 1;
                    l_ref = refi;
                } else {
                    l_ref = -1;
                }
                ul_ref = u_ref;
                u_ref = ur_ref;
                fragi += 1;
            }
        }
    }
    ctx.pipe.ncoded_fragis[pli] = ncoded_fragis;
    ctx.pipe.nuncoded_fragis[pli] =
        ((fragy_end - fragy0) as isize) * (fplane.nhfrags as isize) - ncoded_fragis;
}

pub fn oc_dec_frags_recon_mcu_plane(ctx: &mut DecContext, pli: usize) {
    let ncoded_fragis = ctx.pipe.ncoded_fragis[pli].max(0) as usize;
    let coded_off = ctx.pipe.coded_fragis_off[pli];
    let mut ti = ctx.pipe.ti[pli];
    let mut eob_runs = ctx.pipe.eob_runs[pli];
    let dc_quant = [
        ctx.pipe.dequant[pli][0][0][0],
        ctx.pipe.dequant[pli][0][1][0],
    ];
    for fragii in 0..ncoded_fragis {
        let fragi = ctx.state.coded_fragis[coded_off + fragii] as usize;
        let qti = usize::from(ctx.state.frags[fragi].mb_mode != OC_MODE_INTRA);
        let ac_quant = &ctx.pipe.dequant[pli][ctx.state.frags[fragi].qii as usize][qti];
        let dct_tokens = &ctx.dct_tokens;
        ctx.pipe.dct_coeffs[..64].fill(0);
        let mut zzi = 0usize;
        let mut last_zzi = 0usize;
        while zzi < 64 {
            last_zzi = zzi;
            if eob_runs[zzi] != 0 {
                eob_runs[zzi] -= 1;
                break;
            }
            let lti = ti[zzi];
            if lti < 0 || lti as usize >= dct_tokens.len() {
                break;
            }
            let mut ltiu = lti as usize;
            let token = dct_tokens[ltiu] as usize;
            ltiu += 1;
            let mut cw = OC_DCT_CODE_WORD[token];
            if oc_dct_token_needs_more(token) {
                if ltiu >= dct_tokens.len() {
                    break;
                }
                cw += (dct_tokens[ltiu] as i32) << oc_dct_token_eb_pos(token);
                ltiu += 1;
            }
            let mut eob = ((cw >> OC_DCT_CW_EOB_SHIFT) & 0xFFF) as isize;
            if token == OC_DCT_TOKEN_FAT_EOB {
                if ltiu >= dct_tokens.len() {
                    break;
                }
                eob += (dct_tokens[ltiu] as isize) << 8;
                ltiu += 1;
                if eob == 0 {
                    eob = OC_DCT_EOB_FINISH;
                }
            }
            let rlen = ((cw >> OC_DCT_CW_RLEN_SHIFT) & 0xFF) as usize;
            let flip = cw & (1 << OC_DCT_CW_FLIP_BIT);
            cw ^= -(flip);
            let coeff = cw >> OC_DCT_CW_MAG_SHIFT;
            eob_runs[zzi] = eob;
            ti[zzi] = ltiu as isize;
            zzi += rlen;
            if zzi >= 64 {
                break;
            }
            ctx.pipe.dct_coeffs[OC_FZIG_ZAG[zzi] as usize] =
                (coeff * i32::from(ac_quant[zzi])) as i16;
            zzi += usize::from(eob == 0);
        }
        let zzi = zzi.min(64);
        ctx.pipe.dct_coeffs[0] = ctx.state.frags[fragi].dc;
        crate::state::oc_state_frag_recon_c(
            &mut ctx.state,
            fragi,
            pli,
            &mut ctx.pipe.dct_coeffs,
            last_zzi as i32,
            dc_quant[qti],
        );
    }
    ctx.pipe.ti[pli] = ti;
    ctx.pipe.eob_runs[pli] = eob_runs;
    ctx.pipe.coded_fragis_off[pli] = coded_off + ncoded_fragis;
    let nuncoded = ctx.pipe.nuncoded_fragis[pli].max(0) as usize;
    if nuncoded > 0 {
        let prev_slot = ctx.state.ref_frame_idx[OC_FRAME_PREV as usize];
        let self_slot = ctx.state.ref_frame_idx[OC_FRAME_SELF as usize];
        assert!(
            prev_slot >= 0,
            "OC_FRAME_PREV is not initialized before uncoded fragment copy"
        );
        assert!(
            self_slot >= 0,
            "OC_FRAME_SELF is not initialized before uncoded fragment copy"
        );
        let prev_slot = prev_slot as usize;
        let self_slot = self_slot as usize;
        let ystride = ctx.state.ref_ystride[pli] as isize;
        let uncoded_end = ctx.pipe.uncoded_fragis_off[pli];
        let uncoded_start = uncoded_end.saturating_sub(nuncoded);
        let fragis: Vec<usize> = ctx.state.uncoded_fragis[uncoded_start..uncoded_end]
            .iter()
            .map(|&fragi| {
                assert!(
                    fragi >= 0,
                    "uncoded fragment list contains an invalid fragment index"
                );
                fragi as usize
            })
            .collect();
        let plane_base = ctx.state.ref_frame_bufs[self_slot][pli].data_offset as isize;
        let frag_buf_offs: Vec<isize> = ctx
            .state
            .frag_buf_offs
            .iter()
            .map(|&off| plane_base + off)
            .collect();
        if prev_slot < self_slot {
            let (left, right) = ctx.state.ref_frame_bufs.split_at_mut(self_slot);
            let src = &left[prev_slot][pli].data;
            let dst = &mut right[0][pli].data;
            oc_frag_copy_list_c(dst, src, ystride, &fragis, &frag_buf_offs);
        } else if prev_slot > self_slot {
            let (left, right) = ctx.state.ref_frame_bufs.split_at_mut(prev_slot);
            let dst = &mut left[self_slot][pli].data;
            let src = &right[0][pli].data;
            oc_frag_copy_list_c(dst, src, ystride, &fragis, &frag_buf_offs);
        } else {
            let src = ctx.state.ref_frame_bufs[prev_slot][pli].data.clone();
            let dst = &mut ctx.state.ref_frame_bufs[self_slot][pli].data;
            oc_frag_copy_list_c(dst, &src, ystride, &fragis, &frag_buf_offs);
        }
        ctx.pipe.uncoded_fragis_off[pli] = uncoded_start;
    }
}

#[inline]
fn oc_mini(a: i32, b: i32) -> i32 {
    if a < b {
        a
    } else {
        b
    }
}
#[inline]
fn oc_clampi(lo: i32, v: i32, hi: i32) -> i32 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}
#[inline]
fn oc_clamp255(v: i32) -> u8 {
    if v < 0 {
        0
    } else if v > 255 {
        255
    } else {
        v as u8
    }
}

pub fn oc_filter_hedge(
    dst: &mut [u8],
    dst_off: usize,
    dst_ystride: usize,
    src: &[u8],
    src_off: usize,
    src_ystride: usize,
    qstep: i32,
    flimit: i32,
    variances: &mut [i32],
    nhfrags: usize,
) {
    let mut variance0 = 0i32;
    let mut variance1 = 0i32;
    for bx in 0..8usize {
        let mut r = [0i32; 10];
        for by in 0..10usize {
            let idx = src_off + bx + by * src_ystride;
            if idx >= src.len() {
                return;
            }
            r[by] = src[idx] as i32;
        }
        let mut sum0 = 0i32;
        let mut sum1 = 0i32;
        for by in 0..4usize {
            sum0 += (r[by + 1] - r[by]).abs();
            sum1 += (r[by + 5] - r[by + 6]).abs();
        }
        variance0 += oc_mini(255, sum0);
        variance1 += oc_mini(255, sum1);
        if sum0 < flimit && sum1 < flimit && r[5] - r[4] < qstep && r[4] - r[5] < qstep {
            let mut cdst = dst_off + bx;
            let vals = [
                (r[0] * 3 + r[1] * 2 + r[2] + r[3] + r[4] + 4) >> 3,
                (r[0] * 2 + r[1] + r[2] * 2 + r[3] + r[4] + r[5] + 4) >> 3,
                (r[0] + r[1] + r[2] + r[3] * 2 + r[4] + r[5] + r[6] + 4) >> 3,
                (r[1] + r[2] + r[3] + r[4] * 2 + r[5] + r[6] + r[7] + 4) >> 3,
                (r[2] + r[3] + r[4] + r[5] * 2 + r[6] + r[7] + r[8] + 4) >> 3,
                (r[3] + r[4] + r[5] + r[6] * 2 + r[7] + r[8] + r[9] + 4) >> 3,
                (r[4] + r[5] + r[6] + r[7] * 2 + r[8] + r[9] * 2 + 4) >> 3,
                (r[5] + r[6] + r[7] + r[8] * 2 + r[9] * 3 + 4) >> 3,
            ];
            for v in vals {
                if cdst >= dst.len() {
                    return;
                }
                dst[cdst] = v as u8;
                cdst += dst_ystride;
            }
        } else {
            let mut cdst = dst_off + bx;
            for by in 1..=8usize {
                if cdst >= dst.len() {
                    return;
                }
                dst[cdst] = r[by] as u8;
                cdst += dst_ystride;
            }
        }
    }
    if !variances.is_empty() {
        variances[0] += variance0;
        if nhfrags < variances.len() {
            variances[nhfrags] += variance1;
        }
    }
}

pub fn oc_filter_vedge(
    dst: &mut [u8],
    dst_off: usize,
    dst_ystride: usize,
    qstep: i32,
    flimit: i32,
    variances: &mut [i32],
) {
    let mut variance0 = 0i32;
    let mut variance1 = 0i32;
    let mut cdst = dst_off;
    for _by in 0..8usize {
        if cdst == 0 || cdst + 8 >= dst.len() {
            return;
        }
        let rsrc = cdst - 1;
        let mut r = [0i32; 10];
        for bx in 0..10usize {
            let idx = rsrc + bx;
            if idx >= dst.len() {
                return;
            }
            r[bx] = dst[idx] as i32;
        }
        let mut sum0 = 0i32;
        let mut sum1 = 0i32;
        for bx in 0..4usize {
            sum0 += (r[bx + 1] - r[bx]).abs();
            sum1 += (r[bx + 5] - r[bx + 6]).abs();
        }
        variance0 += oc_mini(255, sum0);
        variance1 += oc_mini(255, sum1);
        if sum0 < flimit && sum1 < flimit && r[5] - r[4] < qstep && r[4] - r[5] < qstep {
            let vals = [
                (r[0] * 3 + r[1] * 2 + r[2] + r[3] + r[4] + 4) >> 3,
                (r[0] * 2 + r[1] + r[2] * 2 + r[3] + r[4] + r[5] + 4) >> 3,
                (r[0] + r[1] + r[2] + r[3] * 2 + r[4] + r[5] + r[6] + 4) >> 3,
                (r[1] + r[2] + r[3] + r[4] * 2 + r[5] + r[6] + r[7] + 4) >> 3,
                (r[2] + r[3] + r[4] + r[5] * 2 + r[6] + r[7] + r[8] + 4) >> 3,
                (r[3] + r[4] + r[5] + r[6] * 2 + r[7] + r[8] + r[9] + 4) >> 3,
                (r[4] + r[5] + r[6] + r[7] * 2 + r[8] + r[9] * 2 + 4) >> 3,
                (r[5] + r[6] + r[7] + r[8] * 2 + r[9] * 3 + 4) >> 3,
            ];
            for (bx, v) in vals.into_iter().enumerate() {
                dst[cdst + bx] = v as u8;
            }
        }
        cdst += dst_ystride;
    }
    if !variances.is_empty() {
        variances[0] += variance0;
        if variances.len() > 1 {
            variances[1] += variance1;
        }
    }
}

pub fn oc_dec_deblock_frag_rows(ctx: &mut DecContext, pli: usize, fragy0: i32, fragy_end: i32) {
    let self_idx = ctx.state.ref_frame_idx[OC_FRAME_SELF as usize];
    if self_idx < 0 {
        return;
    }
    let self_idx = self_idx as usize;
    let src_plane = ctx.state.ref_frame_bufs[self_idx][pli].clone();
    let frame = &mut ctx.pp_frame_buf[pli];
    if frame.data.len() != src_plane.data.len() {
        *frame = src_plane.clone();
    }
    let fplane = ctx.state.fplanes[pli];
    let nhfrags = fplane.nhfrags.max(0) as usize;
    let froffset = (fplane.froffset + fragy0 as isize * fplane.nhfrags as isize).max(0) as usize;
    if froffset >= ctx.dc_qis.len() || froffset >= ctx.variances.len() {
        return;
    }
    let notstart = i32::from(fragy0 > 0);
    let notdone = i32::from(fragy_end < fplane.nvfrags);
    let clear_start = froffset + (nhfrags & (-(notstart as isize) as usize));
    let clear_count =
        ((fragy_end + notdone - fragy0 - notstart).max(0) as usize).saturating_mul(nhfrags);
    if clear_start < ctx.variances.len() {
        let clear_end = (clear_start + clear_count).min(ctx.variances.len());
        for v in &mut ctx.variances[clear_start..clear_end] {
            *v = 0;
        }
    }
    let mut variance_idx = froffset;
    let mut dc_qi_idx = froffset;
    let mut y = (fragy0 << 3) + (notstart << 2);
    let dst_ystride = frame.stride.max(0) as usize;
    let src_ystride = src_plane.stride.max(0) as usize;
    let mut dst_off = (y.max(0) as usize).saturating_mul(dst_ystride);
    let mut src_off = (y.max(0) as usize).saturating_mul(src_ystride);
    let width = frame.width.max(0) as usize;
    while y < 4 {
        if dst_off + width <= frame.data.len() && src_off + width <= src_plane.data.len() {
            frame.data[dst_off..dst_off + width]
                .copy_from_slice(&src_plane.data[src_off..src_off + width]);
        }
        dst_off += dst_ystride;
        src_off += src_ystride;
        y += 1;
    }
    let y_end = ((fragy_end - (1 - notdone)) << 3).max(y);
    while y < y_end {
        if dc_qi_idx >= ctx.dc_qis.len() {
            break;
        }
        let qstep = ctx.pp_dc_scale[ctx.dc_qis[dc_qi_idx] as usize];
        let flimit = (qstep * 3) >> 2;
        let src_base = src_off.saturating_sub(src_ystride);
        oc_filter_hedge(
            &mut frame.data,
            dst_off,
            dst_ystride,
            &src_plane.data,
            src_base,
            src_ystride,
            qstep,
            flimit,
            &mut ctx.variances[variance_idx..],
            nhfrags,
        );
        variance_idx += 1;
        dc_qi_idx += 1;
        let mut x = 8usize;
        while x < width {
            if dc_qi_idx >= ctx.dc_qis.len() {
                break;
            }
            let qstep = ctx.pp_dc_scale[ctx.dc_qis[dc_qi_idx] as usize];
            let flimit = (qstep * 3) >> 2;
            oc_filter_hedge(
                &mut frame.data,
                dst_off + x,
                dst_ystride,
                &src_plane.data,
                src_off + x - src_ystride,
                src_ystride,
                qstep,
                flimit,
                &mut ctx.variances[variance_idx..],
                nhfrags,
            );
            if dst_off + x >= dst_ystride * 4 + 4 {
                let v_off = dst_off + x - (dst_ystride * 4) - 4;
                if variance_idx > 0 {
                    oc_filter_vedge(
                        &mut frame.data,
                        v_off,
                        dst_ystride,
                        qstep,
                        flimit,
                        &mut ctx.variances[variance_idx - 1..],
                    );
                }
            }
            variance_idx += 1;
            dc_qi_idx += 1;
            x += 8;
        }
        dst_off += dst_ystride * 8;
        src_off += src_ystride * 8;
        y += 8;
    }
    if notdone == 0 {
        let height = frame.height.max(0) as usize;
        while y < height as i32 {
            if dst_off + width <= frame.data.len() && src_off + width <= src_plane.data.len() {
                frame.data[dst_off..dst_off + width]
                    .copy_from_slice(&src_plane.data[src_off..src_off + width]);
            }
            dst_off += dst_ystride;
            src_off += src_ystride;
            y += 1;
        }
        dc_qi_idx += 1;
        let mut x = 8usize;
        while x < width {
            if dc_qi_idx >= ctx.dc_qis.len() {
                break;
            }
            let qstep = ctx.pp_dc_scale[ctx.dc_qis[dc_qi_idx] as usize];
            let flimit = (qstep * 3) >> 2;
            if dst_off >= dst_ystride * 8 + 4 {
                let v_off = dst_off + x - (dst_ystride * 8) - 4;
                oc_filter_vedge(
                    &mut frame.data,
                    v_off,
                    dst_ystride,
                    qstep,
                    flimit,
                    &mut ctx.variances[variance_idx..],
                );
            }
            variance_idx += 1;
            dc_qi_idx += 1;
            x += 8;
        }
    }
}

fn oc_dering_block(
    buf: &mut [u8],
    off: usize,
    ystride: usize,
    b: i32,
    dc_scale: i32,
    sharp_mod: i32,
    strong: bool,
) {
    const OC_MOD_MAX: [i32; 2] = [24, 32];
    const OC_MOD_SHIFT: [i32; 2] = [1, 0];
    let strong_i = usize::from(strong);
    let mod_hi = oc_mini(3 * dc_scale, OC_MOD_MAX[strong_i]);
    let mut vmod = [0i32; 72];
    let mut hmod = [0i32; 72];
    let dst = off;
    let mut src = dst;
    let mut psrc = src.wrapping_sub(ystride & if (b & 4) == 0 { usize::MAX } else { 0 });
    for by in 0..9usize {
        for bx in 0..8usize {
            let a = buf.get(src + bx).copied().unwrap_or(0) as i32;
            let p = buf.get(psrc + bx).copied().unwrap_or(a as u8) as i32;
            let modv = 32 + dc_scale - ((a - p).abs() << OC_MOD_SHIFT[strong_i]);
            vmod[(by << 3) + bx] = if modv < -64 {
                sharp_mod
            } else {
                oc_clampi(0, modv, mod_hi)
            };
        }
        psrc = src;
        let step = ystride
            & if ((b & 8) == 0) || by < 7 {
                usize::MAX
            } else {
                0
            };
        src = src.wrapping_add(step);
    }
    let mut nsrc = dst;
    psrc = dst.wrapping_sub(if (b & 1) == 0 { 1 } else { 0 });
    for bx in 0..9usize {
        src = nsrc;
        for by in 0..8usize {
            let a = buf.get(src).copied().unwrap_or(0) as i32;
            let p = buf.get(psrc).copied().unwrap_or(a as u8) as i32;
            let modv = 32 + dc_scale - ((a - p).abs() << OC_MOD_SHIFT[strong_i]);
            hmod[(bx << 3) + by] = if modv < -64 {
                sharp_mod
            } else {
                oc_clampi(0, modv, mod_hi)
            };
            psrc = psrc.wrapping_add(ystride);
            src = src.wrapping_add(ystride);
        }
        psrc = nsrc;
        nsrc = nsrc.wrapping_add(if ((b & 2) == 0) || bx < 7 { 1 } else { 0 });
    }
    src = dst;
    psrc = src.wrapping_sub(ystride & if (b & 4) == 0 { usize::MAX } else { 0 });
    nsrc = src.wrapping_add(ystride);
    let mut d = dst;
    for by in 0..8usize {
        let s = src;
        let p = psrc;
        let n = nsrc;
        let mut a = 128i32;
        let mut bb = 64i32;
        let mut w = hmod[by];
        a -= w;
        bb += w * buf
            .get(s.wrapping_sub(if (b & 1) == 0 { 1 } else { 0 }))
            .copied()
            .unwrap_or(buf[s]) as i32;
        w = vmod[by << 3];
        a -= w;
        bb += w * buf.get(p).copied().unwrap_or(buf[s]) as i32;
        w = vmod[((by + 1) << 3)];
        a -= w;
        bb += w * buf.get(n).copied().unwrap_or(buf[s]) as i32;
        w = hmod[(1 << 3) + by];
        a -= w;
        bb += w * buf.get(s + 1).copied().unwrap_or(buf[s]) as i32;
        buf[d] = oc_clamp255((a * buf[s] as i32 + bb) >> 7);
        for bx in 1..7usize {
            a = 128;
            bb = 64;
            w = hmod[(bx << 3) + by];
            a -= w;
            bb += w * buf[s + bx - 1] as i32;
            w = vmod[(by << 3) + bx];
            a -= w;
            bb += w * buf.get(p + bx).copied().unwrap_or(buf[s + bx]) as i32;
            w = vmod[((by + 1) << 3) + bx];
            a -= w;
            bb += w * buf.get(n + bx).copied().unwrap_or(buf[s + bx]) as i32;
            w = hmod[((bx + 1) << 3) + by];
            a -= w;
            bb += w * buf[s + bx + 1] as i32;
            buf[d + bx] = oc_clamp255((a * buf[s + bx] as i32 + bb) >> 7);
        }
        a = 128;
        bb = 64;
        w = hmod[(7 << 3) + by];
        a -= w;
        bb += w * buf[s + 6] as i32;
        w = vmod[(by << 3) + 7];
        a -= w;
        bb += w * buf.get(p + 7).copied().unwrap_or(buf[s + 7]) as i32;
        w = vmod[((by + 1) << 3) + 7];
        a -= w;
        bb += w * buf.get(n + 7).copied().unwrap_or(buf[s + 7]) as i32;
        w = hmod[(8 << 3) + by];
        a -= w;
        bb += w * buf
            .get(s + 7 + if (b & 2) == 0 { 1 } else { 0 })
            .copied()
            .unwrap_or(buf[s + 7]) as i32;
        buf[d + 7] = oc_clamp255((a * buf[s + 7] as i32 + bb) >> 7);
        d += ystride;
        psrc = src;
        src = nsrc;
        nsrc = nsrc.wrapping_add(
            ystride
                & if ((b & 8) == 0) || by < 6 {
                    usize::MAX
                } else {
                    0
                },
        );
    }
}

const OC_DERING_THRESH1: i32 = 384;
const OC_DERING_THRESH2: i32 = 4 * OC_DERING_THRESH1;
const OC_DERING_THRESH3: i32 = 5 * OC_DERING_THRESH1;
const OC_DERING_THRESH4: i32 = 10 * OC_DERING_THRESH1;

pub fn oc_dec_dering_frag_rows(
    ctx: &mut DecContext,
    pli: usize,
    fragy0: i32,
    fragy_end: i32,
    _threshold: i32,
) {
    let frame = &mut ctx.pp_frame_buf[pli];
    let fplane = ctx.state.fplanes[pli];
    let nhfrags = fplane.nhfrags.max(0) as usize;
    let froffset = (fplane.froffset + fragy0 as isize * fplane.nhfrags as isize).max(0) as usize;
    if froffset >= ctx.variances.len() || froffset >= ctx.state.frags.len() {
        return;
    }
    let strong = ctx.pipe.pp_level
        >= if pli != 0 {
            OC_PP_LEVEL_SDERINGC
        } else {
            OC_PP_LEVEL_SDERINGY
        };
    let sthresh = if pli != 0 {
        OC_DERING_THRESH4
    } else {
        OC_DERING_THRESH3
    };
    let ystride = frame.stride.max(0) as usize;
    let mut y = (fragy0.max(0) as usize) * 8;
    let y_end = (fragy_end.min(fplane.nvfrags).max(fragy0) as usize) * 8;
    let width = frame.width.max(0) as usize;
    let height = frame.height.max(0) as usize;
    let mut variance_idx = froffset;
    let mut frag_idx = froffset;
    while y < y_end {
        let row_off = y * ystride;
        let mut x = 0usize;
        while x < width {
            if variance_idx >= ctx.variances.len() || frag_idx >= ctx.state.frags.len() {
                break;
            }
            let qi = ctx.state.qis[ctx.state.frags[frag_idx].qii as usize] as usize;
            let var = ctx.variances[variance_idx];
            let b = i32::from(x <= 0)
                | (i32::from(x + 8 >= width) << 1)
                | (i32::from(y <= 0) << 2)
                | (i32::from(y + 8 >= height) << 3);
            if strong && var > sthresh {
                oc_dering_block(
                    &mut frame.data,
                    row_off + x,
                    ystride,
                    b,
                    ctx.pp_dc_scale[qi],
                    ctx.pp_sharp_mod[qi],
                    true,
                );
                if pli != 0
                    || ((b & 1) == 0
                        && variance_idx > 0
                        && ctx.variances[variance_idx - 1] > OC_DERING_THRESH4)
                    || ((b & 2) == 0
                        && variance_idx + 1 < ctx.variances.len()
                        && ctx.variances[variance_idx + 1] > OC_DERING_THRESH4)
                    || ((b & 4) == 0
                        && variance_idx >= nhfrags
                        && ctx.variances[variance_idx - nhfrags] > OC_DERING_THRESH4)
                    || ((b & 8) == 0
                        && variance_idx + nhfrags < ctx.variances.len()
                        && ctx.variances[variance_idx + nhfrags] > OC_DERING_THRESH4)
                {
                    oc_dering_block(
                        &mut frame.data,
                        row_off + x,
                        ystride,
                        b,
                        ctx.pp_dc_scale[qi],
                        ctx.pp_sharp_mod[qi],
                        true,
                    );
                    oc_dering_block(
                        &mut frame.data,
                        row_off + x,
                        ystride,
                        b,
                        ctx.pp_dc_scale[qi],
                        ctx.pp_sharp_mod[qi],
                        true,
                    );
                }
            } else if var > OC_DERING_THRESH2 {
                oc_dering_block(
                    &mut frame.data,
                    row_off + x,
                    ystride,
                    b,
                    ctx.pp_dc_scale[qi],
                    ctx.pp_sharp_mod[qi],
                    true,
                );
            } else if var > OC_DERING_THRESH1 {
                oc_dering_block(
                    &mut frame.data,
                    row_off + x,
                    ystride,
                    b,
                    ctx.pp_dc_scale[qi],
                    ctx.pp_sharp_mod[qi],
                    false,
                );
            }
            x += 8;
            variance_idx += 1;
            frag_idx += 1;
        }
        y += 8;
    }
}

pub fn th_decode_packetin(
    ctx: &mut DecContext,
    packet: &OggPacket,
    granpos: Option<i64>,
) -> Result<bool> {
    let mut opb_opt = None;
    if packet.packet.is_empty() {
        ctx.state.frame_type = OC_INTER_FRAME;
        ctx.state.ntotal_coded_fragis = 0;
    } else {
        let mut opb = PackBuf::new(packet.as_slice());
        let hdr = oc_dec_frame_header_unpack(&mut opb)?;
        ctx.state.frame_type = hdr.frame_type;
        ctx.state.nqis = hdr.nqis;
        ctx.state.qis = hdr.qis;
        if hdr.frame_type == OC_INTRA_FRAME {
            oc_dec_mark_all_intra(&mut ctx.state);
        } else {
            let _ = oc_dec_coded_flags_unpack(&mut opb, &mut ctx.state);
        }
        opb_opt = Some(opb);
    }

    if ctx.state.frame_type != OC_INTRA_FRAME
        && (ctx.state.ref_frame_idx[OC_FRAME_GOLD as usize] < 0
            || ctx.state.ref_frame_idx[OC_FRAME_PREV as usize] < 0)
    {
        oc_dec_init_dummy_frame(ctx);
    }

    if ctx.state.ntotal_coded_fragis <= 0 {
        let gp = ((ctx.state.keyframe_num + i64::from(ctx.state.granpos_bias))
            << ctx.state.info.keyframe_granule_shift)
            + (ctx.state.curframe_num - ctx.state.keyframe_num);
        ctx.state.granpos = gp;
        ctx.state.curframe_num += 1;
        ctx.granulepos = granpos.unwrap_or(gp);
        if pp_is_disabled(ctx) {
            sync_pp_from_self(ctx);
        }
        return Ok(false);
    }

    let mut refi = 0usize;
    while refi as i32 == ctx.state.ref_frame_idx[OC_FRAME_GOLD as usize]
        || refi as i32 == ctx.state.ref_frame_idx[OC_FRAME_PREV as usize]
    {
        refi += 1;
    }
    assign_frame_role(ctx, OC_FRAME_SELF as usize, refi as i32);

    if let Some(mut opb) = opb_opt {
        if ctx.state.frame_type == OC_INTRA_FRAME {
            ctx.state.keyframe_num = ctx.state.curframe_num;
        } else {
            oc_dec_mb_modes_unpack(&mut opb, &mut ctx.state);
            oc_dec_mv_unpack_and_frag_modes_fill(&mut opb, &mut ctx.state);
        }
        oc_dec_block_qis_unpack(
            &mut opb,
            &mut ctx.state.frags,
            &ctx.state.coded_fragis,
            ctx.state.nqis,
        );
        oc_dec_residual_tokens_unpack(ctx, &mut opb)?;
    }

    let gp = ((ctx.state.keyframe_num + i64::from(ctx.state.granpos_bias))
        << ctx.state.info.keyframe_granule_shift)
        + (ctx.state.curframe_num - ctx.state.keyframe_num);
    ctx.state.granpos = gp;
    ctx.state.curframe_num += 1;
    ctx.granulepos = granpos.unwrap_or(gp);

    oc_dec_pipeline_init(ctx);
    let mut notstart = 0i32;
    let mut notdone = 1i32;
    let mut stripe_fragy = 0i32;
    while notdone != 0 {
        let mut avail_fragy_end = ctx.state.fplanes[0].nvfrags;
        notdone = i32::from(stripe_fragy + ctx.pipe.mcu_nvfrags < avail_fragy_end);
        for pli in 0..3usize {
            let frag_shift = i32::from(
                pli != 0
                    && matches!(
                        ctx.state.info.pixel_fmt,
                        crate::codec::PixelFmt::Pf420 | crate::codec::PixelFmt::Reserved
                    ),
            );
            ctx.pipe.fragy0[pli] = stripe_fragy >> frag_shift;
            ctx.pipe.fragy_end[pli] = ctx.state.fplanes[pli]
                .nvfrags
                .min(ctx.pipe.fragy0[pli] + (ctx.pipe.mcu_nvfrags >> frag_shift));
            oc_dec_dc_unpredict_mcu_plane_c(ctx, pli);
            oc_dec_frags_recon_mcu_plane(ctx, pli);
            let mut sdelay = 0i32;
            let mut edelay = 0i32;
            if ctx.pipe.loop_filter != 0 {
                sdelay += notstart;
                edelay += notdone;
                crate::state::oc_state_loop_filter_frag_rows_c(
                    &mut ctx.state,
                    &ctx.pipe.bounding_values,
                    refi,
                    pli,
                    ctx.pipe.fragy0[pli] - sdelay,
                    ctx.pipe.fragy_end[pli] - edelay,
                );
            }
            crate::state::oc_state_borders_fill_rows(
                &mut ctx.state,
                refi,
                pli,
                ((ctx.pipe.fragy0[pli] - sdelay) << 3) - (sdelay << 1),
                ((ctx.pipe.fragy_end[pli] - edelay) << 3) - (edelay << 1),
            );
            if ctx.pipe.pp_level >= OC_PP_LEVEL_DEBLOCKY + 3 * i32::from(pli != 0) {
                sdelay += notstart;
                edelay += notdone;
                oc_dec_deblock_frag_rows(
                    ctx,
                    pli,
                    ctx.pipe.fragy0[pli] - sdelay,
                    ctx.pipe.fragy_end[pli] - edelay,
                );
                if ctx.pipe.pp_level >= OC_PP_LEVEL_DERINGY + 3 * i32::from(pli != 0) {
                    sdelay += notstart;
                    edelay += notdone;
                    oc_dec_dering_frag_rows(
                        ctx,
                        pli,
                        ctx.pipe.fragy0[pli] - sdelay,
                        ctx.pipe.fragy_end[pli] - edelay,
                        8,
                    );
                }
            } else if ctx.pipe.loop_filter != 0 {
                sdelay += notstart;
                edelay += notdone;
            }
            avail_fragy_end = avail_fragy_end.min((ctx.pipe.fragy_end[pli] - edelay) << frag_shift);
        }
        notstart = 1;
        stripe_fragy += ctx.pipe.mcu_nvfrags;
    }
    for pli in 0..3usize {
        crate::state::oc_state_borders_fill_caps(&mut ctx.state, refi, pli);
    }
    if pp_is_disabled(ctx) {
        sync_pp_from_self(ctx);
    }
    let self_slot = ctx.state.ref_frame_idx[OC_FRAME_SELF as usize];
    if ctx.state.frame_type == OC_INTRA_FRAME {
        assign_frame_role(ctx, OC_FRAME_GOLD as usize, self_slot);
        assign_frame_role(ctx, OC_FRAME_PREV as usize, self_slot);
    } else {
        assign_frame_role(ctx, OC_FRAME_PREV as usize, self_slot);
    }
    Ok(ctx.state.frame_type == OC_INTRA_FRAME)
}

pub fn th_decode_ycbcr_out(ctx: &DecContext) -> Result<YCbCrBuffer> {
    let mut ycbcr: YCbCrBuffer = [
        ImgPlane::default(),
        ImgPlane::default(),
        ImgPlane::default(),
    ];
    if !pp_is_disabled(ctx) && !ctx.pp_frame_buf[0].data.is_empty() {
        crate::internal::oc_ycbcr_buffer_flip(&mut ycbcr, &ctx.pp_frame_buf);
        return Ok(ycbcr);
    }
    let self_idx = ctx.state.ref_frame_idx[OC_FRAME_SELF as usize];
    if self_idx < 0 {
        return Err(TheoraError::BadPacket);
    }
    crate::internal::oc_ycbcr_buffer_flip(&mut ycbcr, &ctx.state.ref_frame_bufs[self_idx as usize]);
    Ok(ycbcr)
}
