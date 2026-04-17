use crate::state::OC_NMODES;

pub const OC_MODE_BITS: [[u8; OC_NMODES]; 2] = [[1, 2, 3, 4, 5, 6, 7, 7], [3, 3, 3, 3, 3, 3, 3, 3]];

pub const OC_MODE_CODES: [[u8; OC_NMODES]; 2] = [
    [0x00, 0x02, 0x06, 0x0E, 0x1E, 0x3E, 0x7E, 0x7F],
    [0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07],
];

pub const OC_MV_BITS: [[u8; 64]; 2] = [
    [
        8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 7, 7, 7, 7, 7, 7, 7, 7, 6, 6, 6, 6, 4, 4,
        3, 3, 3, 4, 4, 6, 6, 6, 6, 7, 7, 7, 7, 7, 7, 7, 7, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8, 8,
        8, 8, 8, 8,
    ],
    [6; 64],
];

pub const OC_SB_RUN_VAL_MIN: [u16; 8] = [1, 2, 4, 6, 10, 18, 34, 4130];
pub const OC_SB_RUN_CODE_PREFIX: [u32; 7] = [0, 4, 0xC, 0x38, 0xF0, 0x3E0, 0x3F000];
pub const OC_SB_RUN_CODE_NBITS: [u8; 7] = [1, 3, 4, 6, 8, 10, 18];
pub const OC_BLOCK_RUN_CODE_NBITS: [u8; 30] = [
    2, 2, 3, 3, 4, 4, 6, 6, 6, 6, 7, 7, 7, 7, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
];
pub const OC_BLOCK_RUN_CODE_PATTERN: [u16; 30] = [
    0x000, 0x001, 0x004, 0x005, 0x00C, 0x00D, 0x038, 0x039, 0x03A, 0x03B, 0x078, 0x079, 0x07A,
    0x07B, 0x1F0, 0x1F1, 0x1F2, 0x1F3, 0x1F4, 0x1F5, 0x1F6, 0x1F7, 0x1F8, 0x1F9, 0x1FA, 0x1FB,
    0x1FC, 0x1FD, 0x1FE, 0x1FF,
];

pub const OC_MODE_RANKS: [[u8; OC_NMODES]; 7] = [
    [3, 4, 2, 0, 1, 5, 6, 7],
    [2, 4, 3, 0, 1, 5, 6, 7],
    [3, 4, 1, 0, 2, 5, 6, 7],
    [2, 4, 1, 0, 3, 5, 6, 7],
    [0, 4, 3, 1, 2, 5, 6, 7],
    [0, 5, 4, 2, 3, 1, 6, 7],
    [0, 1, 2, 3, 4, 5, 6, 7],
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ModeSchemeChooser {
    pub mode_ranks: [[u8; OC_NMODES]; 8],
    pub scheme0_ranks: [u8; OC_NMODES],
    pub scheme0_list: [u8; OC_NMODES],
    pub mode_counts: [u32; OC_NMODES],
    pub scheme_bits: [i32; 8],
    pub scheme_list: [u8; 8],
}

pub fn oc_mode_scheme_chooser_init(chooser: &mut ModeSchemeChooser) {
    chooser.mode_ranks[0] = chooser.scheme0_ranks;
    for si in 1..8 {
        chooser.mode_ranks[si] = OC_MODE_RANKS[si - 1];
    }
}

pub fn oc_mode_scheme_chooser_reset(chooser: &mut ModeSchemeChooser) {
    chooser.mode_counts = [0; OC_NMODES];
    chooser.scheme_bits = [0; 8];
    chooser.scheme_bits[0] = 24;
    for si in 0..8 {
        chooser.scheme_list[si] = (7 - si) as u8;
        chooser.scheme0_list[si] = si as u8;
        chooser.scheme0_ranks[si] = si as u8;
    }
    chooser.mode_ranks[0] = chooser.scheme0_ranks;
    for si in 1..8 {
        chooser.mode_ranks[si] = OC_MODE_RANKS[si - 1];
    }
}

pub fn oc_mode_scheme_chooser_scheme_mb_cost(
    chooser: &ModeSchemeChooser,
    scheme: usize,
    mb_mode: usize,
) -> i32 {
    let codebook = (scheme + 1) >> 3;
    let mut ri = chooser.mode_ranks[scheme][mb_mode] as usize;
    if scheme == 0 {
        let mc = chooser.mode_counts[mb_mode];
        while ri > 0 && mc >= chooser.mode_counts[chooser.scheme0_list[ri - 1] as usize] {
            ri -= 1;
        }
    }
    OC_MODE_BITS[codebook][ri] as i32
}

pub fn oc_mode_scheme_chooser_cost(chooser: &mut ModeSchemeChooser, mb_mode: usize) -> i32 {
    let scheme0 = chooser.scheme_list[0] as usize;
    let mut scheme1 = chooser.scheme_list[1] as usize;
    let scheme0_bits = chooser.scheme_bits[scheme0];
    let mut scheme1_bits = chooser.scheme_bits[scheme1];
    let mode_bits = oc_mode_scheme_chooser_scheme_mb_cost(chooser, scheme0, mb_mode);
    if scheme1_bits - scheme0_bits > 6 {
        return mode_bits;
    }
    let mut best_bits = scheme0_bits + mode_bits;
    let mut si = 1usize;
    loop {
        let cur_bits =
            scheme1_bits + oc_mode_scheme_chooser_scheme_mb_cost(chooser, scheme1, mb_mode);
        if cur_bits < best_bits {
            best_bits = cur_bits;
        }
        si += 1;
        if si >= 8 {
            break;
        }
        scheme1 = chooser.scheme_list[si] as usize;
        scheme1_bits = chooser.scheme_bits[scheme1];
        if scheme1_bits - scheme0_bits > 6 {
            break;
        }
    }
    best_bits - scheme0_bits
}

pub fn oc_mode_scheme_chooser_update(chooser: &mut ModeSchemeChooser, mb_mode: usize) {
    chooser.mode_counts[mb_mode] += 1;
    let mut ri = chooser.scheme0_ranks[mb_mode] as usize;
    while ri > 0 {
        let pmode = chooser.scheme0_list[ri - 1] as usize;
        if chooser.mode_counts[pmode] >= chooser.mode_counts[mb_mode] {
            break;
        }
        chooser.scheme0_ranks[pmode] += 1;
        chooser.scheme0_list[ri] = pmode as u8;
        ri -= 1;
    }
    chooser.scheme0_ranks[mb_mode] = ri as u8;
    chooser.scheme0_list[ri] = mb_mode as u8;
    chooser.mode_ranks[0] = chooser.scheme0_ranks;
    for si in 0..8usize {
        chooser.scheme_bits[si] +=
            OC_MODE_BITS[(si + 1) >> 3][chooser.mode_ranks[si][mb_mode] as usize] as i32;
    }
    for si in 1..8usize {
        let mut sj = si;
        let scheme0 = chooser.scheme_list[si];
        let bits0 = chooser.scheme_bits[scheme0 as usize];
        while sj > 0 {
            let scheme1 = chooser.scheme_list[sj - 1];
            if bits0 >= chooser.scheme_bits[scheme1 as usize] {
                break;
            }
            chooser.scheme_list[sj] = scheme1;
            sj -= 1;
        }
        chooser.scheme_list[sj] = scheme0;
    }
}

pub fn oc_sb_run_bits(run_count: i32) -> i32 {
    let mut i = 0usize;
    while run_count >= OC_SB_RUN_VAL_MIN[i + 1] as i32 {
        i += 1;
    }
    OC_SB_RUN_CODE_NBITS[i] as i32
}

pub fn oc_block_run_bits(run_count: i32) -> i32 {
    OC_BLOCK_RUN_CODE_NBITS[(run_count - 1) as usize] as i32
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FrState {
    pub bits: isize,
    pub sb_partial_count: u16,
    pub sb_full_count: u16,
    pub b_coded_count_prev: u8,
    pub b_coded_prev: i8,
    pub b_coded_count: u8,
    pub b_coded: i8,
    pub b_count: u8,
    pub sb_prefer_partial: bool,
    pub sb_partial: i8,
    pub sb_bits: u8,
    pub sb_full: i8,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QiiState {
    pub bits: isize,
    pub qi01_count: u16,
    pub qi01: i8,
    pub qi12_count: u16,
    pub qi12: i8,
}

pub fn oc_fr_state_init(fr: &mut FrState) {
    fr.bits = 0;
    fr.sb_partial_count = 0;
    fr.sb_full_count = 0;
    fr.b_coded_count_prev = 0;
    fr.b_coded_count = 0;
    fr.b_count = 0;
    fr.sb_prefer_partial = false;
    fr.sb_bits = 0;
    fr.sb_partial = -1;
    fr.sb_full = -1;
    fr.b_coded_prev = -1;
    fr.b_coded = -1;
}

pub fn oc_fr_state_sb_cost(fr: &FrState, sb_partial: i32, sb_full: i32) -> i32 {
    let mut bits = 0i32;
    let mut sb_partial_count = i32::from(fr.sb_partial_count);
    if fr.sb_partial == sb_partial as i8 {
        if sb_partial_count >= 4129 {
            bits += 1;
            sb_partial_count = 0;
        } else {
            bits -= oc_sb_run_bits(sb_partial_count);
        }
    } else {
        sb_partial_count = 0;
    }
    sb_partial_count += 1;
    bits += oc_sb_run_bits(sb_partial_count);
    if sb_partial == 0 {
        let mut sb_full_count = i32::from(fr.sb_full_count);
        if fr.sb_full == sb_full as i8 {
            if sb_full_count >= 4129 {
                bits += 1;
                sb_full_count = 0;
            } else {
                bits -= oc_sb_run_bits(sb_full_count);
            }
        } else {
            sb_full_count = 0;
        }
        sb_full_count += 1;
        bits += oc_sb_run_bits(sb_full_count);
    }
    bits
}

pub fn oc_fr_state_advance_sb(fr: &mut FrState, sb_partial: i32, sb_full: i32) {
    let mut sb_partial_count = i32::from(fr.sb_partial_count);
    if fr.sb_partial != sb_partial as i8 || sb_partial_count >= 4129 {
        sb_partial_count = 0;
    }
    sb_partial_count += 1;
    if sb_partial == 0 {
        let mut sb_full_count = i32::from(fr.sb_full_count);
        if fr.sb_full != sb_full as i8 || sb_full_count >= 4129 {
            sb_full_count = 0;
        }
        sb_full_count += 1;
        fr.sb_full_count = sb_full_count as u16;
        fr.sb_full = sb_full as i8;
        fr.b_coded = fr.b_coded_prev;
        fr.b_coded_count = fr.b_coded_count_prev;
    } else {
        fr.b_coded_prev = fr.b_coded;
        fr.b_coded_count_prev = fr.b_coded_count;
    }
    fr.sb_partial_count = sb_partial_count as u16;
    fr.sb_partial = sb_partial as i8;
    fr.b_count = 0;
    fr.sb_prefer_partial = false;
    fr.sb_bits = 0;
}

pub fn oc_fr_state_flush_sb(fr: &mut FrState) {
    let b_count = i32::from(fr.b_count);
    let b_coded_count = i32::from(fr.b_coded_count);
    let sb_full = i32::from(fr.b_coded);
    let mut sb_partial = (b_coded_count < b_count) as i32;
    if sb_partial == 0 && fr.sb_prefer_partial {
        if b_coded_count > 15 || fr.b_coded_prev < 0 {
            let sb_bits = oc_fr_state_sb_cost(fr, sb_partial, sb_full);
            fr.bits += (sb_bits - i32::from(fr.sb_bits)) as isize;
            fr.sb_bits = sb_bits as u8;
        } else {
            sb_partial = 1;
        }
    }
    oc_fr_state_advance_sb(fr, sb_partial, sb_full);
}

pub fn oc_fr_state_advance_block(fr: &mut FrState, b_coded: i32) {
    let mut sb_bits = i32::from(fr.sb_bits);
    let bits = fr.bits - sb_bits as isize;
    let mut b_count = i32::from(fr.b_count);
    let mut b_coded_count = i32::from(fr.b_coded_count);
    let mut sb_prefer_partial = fr.sb_prefer_partial;
    if b_coded_count >= b_count {
        if b_count <= 0 {
            b_count = 1;
            let mut sb_partial_bits;
            if fr.b_coded == b_coded as i8 {
                sb_partial_bits = -oc_block_run_bits(b_coded_count);
                b_coded_count += 1;
                sb_partial_bits += oc_block_run_bits(b_coded_count);
            } else {
                b_coded_count = 1;
                sb_partial_bits = 2;
            }
            sb_partial_bits += oc_fr_state_sb_cost(fr, 1, b_coded);
            sb_bits = oc_fr_state_sb_cost(fr, 0, b_coded);
            sb_prefer_partial = sb_partial_bits < sb_bits;
            if sb_prefer_partial {
                sb_bits = sb_partial_bits;
            }
        } else if fr.b_coded == b_coded as i8 {
            b_coded_count += 1;
            b_count += 1;
            if b_count < 16 {
                if sb_prefer_partial {
                    let mut sb_partial_bits = sb_bits;
                    sb_partial_bits += oc_block_run_bits(b_coded_count);
                    if b_coded_count > 0 {
                        sb_partial_bits -= oc_block_run_bits(b_coded_count - 1);
                    }
                    let full_bits = oc_fr_state_sb_cost(fr, 0, b_coded);
                    sb_prefer_partial = sb_partial_bits < full_bits;
                    sb_bits = if sb_prefer_partial {
                        sb_partial_bits
                    } else {
                        full_bits
                    };
                }
            } else if sb_prefer_partial {
                sb_prefer_partial = false;
                sb_bits = oc_fr_state_sb_cost(fr, 0, b_coded);
            }
        } else {
            if !sb_prefer_partial {
                sb_bits = oc_block_run_bits(b_coded_count);
                if b_coded_count > b_count {
                    sb_bits -= oc_block_run_bits(b_coded_count - b_count);
                }
                sb_bits += oc_fr_state_sb_cost(fr, 1, b_coded);
            }
            b_count += 1;
            b_coded_count = 1;
            sb_prefer_partial = true;
            sb_bits += 2;
        }
    } else {
        b_count += 1;
        if fr.b_coded == b_coded as i8 {
            sb_bits -= oc_block_run_bits(b_coded_count);
        } else {
            b_coded_count = 0;
        }
        b_coded_count += 1;
        sb_bits += oc_block_run_bits(b_coded_count);
    }
    fr.bits = bits + sb_bits as isize;
    fr.b_coded_count = b_coded_count as u8;
    fr.b_coded = b_coded as i8;
    fr.b_count = b_count as u8;
    fr.sb_prefer_partial = sb_prefer_partial;
    fr.sb_bits = sb_bits as u8;
}

pub fn oc_fr_skip_block(fr: &mut FrState) {
    oc_fr_state_advance_block(fr, 0);
}

pub fn oc_fr_code_block(fr: &mut FrState) {
    oc_fr_state_advance_block(fr, 1);
}

pub fn oc_fr_cost1(fr: &FrState) -> i32 {
    let mut tmp = *fr;
    oc_fr_skip_block(&mut tmp);
    let bits = tmp.bits;
    tmp = *fr;
    oc_fr_code_block(&mut tmp);
    (tmp.bits - bits) as i32
}

pub fn oc_fr_cost4(pre: &FrState, post: &FrState) -> i32 {
    let mut tmp = *pre;
    oc_fr_skip_block(&mut tmp);
    oc_fr_skip_block(&mut tmp);
    oc_fr_skip_block(&mut tmp);
    oc_fr_skip_block(&mut tmp);
    (post.bits - tmp.bits) as i32
}

pub fn oc_qii_state_init(qs: &mut QiiState) {
    qs.bits = 0;
    qs.qi01_count = 0;
    qs.qi01 = -1;
    qs.qi12_count = 0;
    qs.qi12 = -1;
}

pub fn oc_qii_state_advance(qd: &mut QiiState, qs: &QiiState, qii: i32) {
    let mut bits = qs.bits;
    let qi01 = ((qii + 1) >> 1) as i8;
    let mut qi01_count = i32::from(qs.qi01_count);
    if qi01 == qs.qi01 {
        if qi01_count >= 4129 {
            bits += 1;
            qi01_count = 0;
        } else {
            bits -= oc_sb_run_bits(qi01_count) as isize;
        }
    } else {
        qi01_count = 0;
    }
    qi01_count += 1;
    bits += oc_sb_run_bits(qi01_count) as isize;
    let mut qi12 = qs.qi12;
    let mut qi12_count = i32::from(qs.qi12_count);
    if qii != 0 {
        qi12 = (qii >> 1) as i8;
        if qi12 == qs.qi12 {
            if qi12_count >= 4129 {
                bits += 1;
                qi12_count = 0;
            } else {
                bits -= oc_sb_run_bits(qi12_count) as isize;
            }
        } else {
            qi12_count = 0;
        }
        qi12_count += 1;
        bits += oc_sb_run_bits(qi12_count) as isize;
    }
    qd.bits = bits;
    qd.qi01 = qi01;
    qd.qi01_count = qi01_count as u16;
    qd.qi12 = qi12;
    qd.qi12_count = qi12_count as u16;
}

use crate::encint::EncContext;
use crate::enquant::{oc_enc_enquant_table_init_c, oc_enc_quantize_c, OcIQuant};
use crate::fdct::oc_enc_fdct8x8_c;

pub fn oc_enc_pipeline_init(enc: &mut EncContext) {
    enc.pipe = Default::default();
    for pli in 0..3 {
        enc.pipe.fragy0[pli] = 0;
        enc.pipe.fragy_end[pli] = enc.state.fplanes[pli].nvfrags;
        oc_fr_state_init(&mut enc.pipe.fr[pli]);
        oc_qii_state_init(&mut enc.pipe.qs[pli]);
    }
}

pub fn oc_enc_pipeline_set_stripe(enc: &mut EncContext, pli: usize, fragy0: i32, fragy_end: i32) {
    enc.pipe.fragy0[pli] = fragy0;
    enc.pipe.fragy_end[pli] = fragy_end;
}

pub fn oc_enc_pipeline_finish_mcu_plane(enc: &mut EncContext, pli: usize) {
    oc_fr_state_flush_sb(&mut enc.pipe.fr[pli]);
}

pub fn oc_enc_block_transform_quantize(
    enc: &mut EncContext,
    input: &[i16; 64],
    qii: usize,
) -> [i16; 64] {
    let mut dct = [0i16; 64];
    oc_enc_fdct8x8_c(&mut dct, input);
    let mut qdct = [0i16; 64];
    let qti = (enc.state.frame_type != 0) as usize;
    let pli = 0usize;
    let qi = usize::from(enc.state.qis[qii.min(2)]);
    let dequant = &enc.state.dequant_tables[qi][pli][qti];
    let mut enquant = [OcIQuant::default(); 64];
    oc_enc_enquant_table_init_c(&mut enquant, dequant);
    let _ = oc_enc_quantize_c(&mut qdct, &dct, dequant, &enquant);
    qdct
}

pub fn oc_enc_mb_transform_quantize_inter_luma(
    enc: &mut EncContext,
    blocks: &[[i16; 64]; 4],
    qii: usize,
) -> [[i16; 64]; 4] {
    let mut out = [[0i16; 64]; 4];
    for bi in 0..4 {
        out[bi] = oc_enc_block_transform_quantize(enc, &blocks[bi], qii);
    }
    out
}

pub fn oc_enc_sb_transform_quantize_inter_chroma(
    enc: &mut EncContext,
    blocks: &[[i16; 64]; 4],
    qii: usize,
) -> [[i16; 64]; 4] {
    oc_enc_mb_transform_quantize_inter_luma(enc, blocks, qii)
}

pub fn oc_enc_mode_rd_init(enc: &mut EncContext) {
    enc.lambda = 1.max(enc.lambda);
}

pub fn oc_dct_cost2(coeffs: &[i16; 64]) -> i32 {
    coeffs.iter().map(|&c| i32::from(c).abs()).sum()
}

pub fn oc_mb_activity(blocks: &[[u8; 64]; 4]) -> u32 {
    let mut sum = 0u64;
    let mut sum2 = 0u64;
    let mut n = 0u64;
    for b in blocks {
        for &v in b {
            let x = v as u64;
            sum += x;
            sum2 += x * x;
            n += 1;
        }
    }
    if n == 0 {
        return 0;
    }
    let mean = sum / n;
    ((sum2 / n).saturating_sub(mean * mean)) as u32
}

pub fn oc_mb_activity_fast(blocks: &[[u8; 64]; 4]) -> u32 {
    oc_mb_activity(blocks)
}

pub fn oc_mb_masking(activity: u32, avg: u32) -> u32 {
    activity.saturating_add(avg / 2)
}

pub fn oc_mb_intra_satd(coeffs: &[[i16; 64]; 4]) -> u32 {
    coeffs.iter().map(|b| oc_dct_cost2(b) as u32).sum()
}

pub fn oc_analyze_intra_mb_luma(
    enc: &mut EncContext,
    coeffs: &[[i16; 64]; 4],
    activity: u32,
) -> u32 {
    let satd = oc_mb_intra_satd(coeffs);
    satd + oc_mb_masking(activity, enc.activity_avg)
}

pub fn oc_analyze_intra_chroma_block(coeffs: &[i16; 64]) -> u32 {
    oc_dct_cost2(coeffs) as u32
}

pub fn oc_enc_mb_transform_quantize_intra_luma(
    enc: &mut EncContext,
    blocks: &[[i16; 64]; 4],
    qii: usize,
) -> [[i16; 64]; 4] {
    oc_enc_mb_transform_quantize_inter_luma(enc, blocks, qii)
}

pub fn oc_enc_sb_transform_quantize_intra_chroma(
    enc: &mut EncContext,
    blocks: &[[i16; 64]; 4],
    qii: usize,
) -> [[i16; 64]; 4] {
    oc_enc_mb_transform_quantize_inter_luma(enc, blocks, qii)
}

pub fn oc_enc_analyze_intra(
    enc: &mut EncContext,
    luma: &[[i16; 64]; 4],
    chroma: &[[i16; 64]; 4],
    activity: u32,
) -> u32 {
    oc_analyze_intra_mb_luma(enc, luma, activity)
        + chroma
            .iter()
            .map(|b| oc_analyze_intra_chroma_block(b))
            .sum::<u32>()
}

pub fn oc_mode_set_cost(modes: &[u8]) -> i32 {
    modes
        .iter()
        .map(|&m| OC_MODE_BITS[0][(m as usize).min(7)] as i32)
        .sum()
}

pub fn oc_analyze_mb_mode_luma(coeffs: &[[i16; 64]; 4]) -> [u32; 8] {
    let base = oc_mb_intra_satd(coeffs);
    [
        base + 64,
        base,
        base / 2 + 32,
        base / 2 + 24,
        base / 2 + 28,
        base / 2 + 40,
        base / 2 + 36,
        base / 3 + 48,
    ]
}

pub fn oc_analyze_mb_mode_chroma(coeffs: &[[i16; 64]; 4]) -> [u32; 8] {
    let base: u32 = coeffs.iter().map(|b| oc_dct_cost2(b) as u32).sum();
    [
        base + 32,
        base,
        base / 2 + 16,
        base / 2 + 12,
        base / 2 + 14,
        base / 2 + 20,
        base / 2 + 18,
        base / 3 + 24,
    ]
}

pub fn oc_skip_cost(skip_ssd: u32) -> u32 {
    skip_ssd
}
pub fn oc_cost_intra(luma: u32, chroma: u32, rate: i32) -> u32 {
    luma + chroma + rate.max(0) as u32
}
pub fn oc_cost_inter(residual: u32, mv_bits: i32, rate: i32) -> u32 {
    residual + mv_bits.max(0) as u32 + rate.max(0) as u32
}
pub fn oc_cost_inter_nomv(residual: u32, rate: i32) -> u32 {
    residual + rate.max(0) as u32
}
pub fn oc_cost_inter1mv(residual: u32, mv_bits: i32, rate: i32) -> u32 {
    oc_cost_inter(residual, mv_bits, rate)
}
pub fn oc_cost_inter4mv(residual: u32, mv_bits: i32, rate: i32) -> u32 {
    residual + mv_bits.max(0) as u32 * 4 + rate.max(0) as u32
}

pub fn oc_enc_analyze_inter(
    enc: &mut EncContext,
    luma_modes: [u32; 8],
    chroma_modes: [u32; 8],
    mv_bits: [i32; 8],
) -> (u8, u32) {
    let mut best_mode = 0u8;
    let mut best_cost = u32::MAX;
    for mode in 0..8usize {
        let cost = if mode == 1 {
            oc_cost_intra(luma_modes[mode], chroma_modes[mode], enc.lambda)
        } else if mode == 0 || mode == 5 {
            oc_cost_inter_nomv(luma_modes[mode] + chroma_modes[mode], enc.lambda)
        } else if mode == 7 {
            oc_cost_inter4mv(
                luma_modes[mode] + chroma_modes[mode],
                mv_bits[mode],
                enc.lambda,
            )
        } else {
            oc_cost_inter1mv(
                luma_modes[mode] + chroma_modes[mode],
                mv_bits[mode],
                enc.lambda,
            )
        };
        if cost < best_cost {
            best_cost = cost;
            best_mode = mode as u8;
        }
    }
    (best_mode, best_cost)
}
