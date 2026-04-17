use crate::codec::{HuffCode, TH_NDCT_TOKENS, TH_NHUFFMAN_TABLES};
use crate::huffman::{
    OC_DCT_REPEAT_RUN3_TOKEN, OC_DCT_TOKEN_EXTRA_BITS, OC_DCT_VAL_CAT2, OC_DCT_VAL_CAT3,
    OC_DCT_VAL_CAT4, OC_DCT_VAL_CAT5, OC_DCT_VAL_CAT6, OC_DCT_VAL_CAT7, OC_DCT_VAL_CAT8,
};

pub const OC_DCT_EOB_TOKEN: [u8; 31] = [
    0, 1, 2, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5, 5,
];

pub const OC_DCT_EOB_EB: [u8; 31] = [
    0, 0, 0, 0, 1, 2, 3, 0, 1, 2, 3, 4, 5, 6, 7, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14,
    15,
];

pub const OC_ZZI_HUFF_OFFSET: [usize; 64] = [
    0, 0, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 2, 2, 2, 3, 3, 3, 3, 3, 3, 3, 3, 4, 4, 4, 4, 4, 4, 4, 4,
    5, 5, 5, 5, 5, 5, 5, 5, 6, 6, 6, 6, 6, 6, 6, 6, 7, 7, 7, 7, 7, 7, 7, 7, 8, 8, 8, 8, 8, 8, 8, 8,
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TokenCheckpoint {
    pub pli: usize,
    pub zzi: usize,
    pub eob_run: u16,
    pub ndct_tokens: isize,
}

#[derive(Debug, Clone)]
pub struct TokenLog<'a> {
    pub huff_codes: &'a [[HuffCode; TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES],
    pub eob_run: [[u16; 64]; 3],
    pub ndct_tokens: [[isize; 64]; 3],
    pub dct_tokens: [Vec<Vec<u8>>; 3],
    pub extra_bits: [Vec<Vec<u16>>; 3],
    pub dct_token_offs: [[u8; 64]; 3],
    pub dc_pred_last: [[i32; 4]; 3],
}

impl<'a> TokenLog<'a> {
    pub fn new(
        huff_codes: &'a [[HuffCode; TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES],
        nfrags: usize,
    ) -> Self {
        let mk_u8 = || {
            (0..64)
                .map(|_| Vec::<u8>::with_capacity(nfrags))
                .collect::<Vec<_>>()
        };
        let mk_u16 = || {
            (0..64)
                .map(|_| Vec::<u16>::with_capacity(nfrags))
                .collect::<Vec<_>>()
        };
        Self {
            huff_codes,
            eob_run: [[0; 64]; 3],
            ndct_tokens: [[0; 64]; 3],
            dct_tokens: [mk_u8(), mk_u8(), mk_u8()],
            extra_bits: [mk_u16(), mk_u16(), mk_u16()],
            dct_token_offs: [[0; 64]; 3],
            dc_pred_last: [[0; 4]; 3],
        }
    }
}

pub fn oc_make_eob_token(run_count: i32) -> usize {
    if run_count < 32 {
        OC_DCT_EOB_TOKEN[(run_count - 1) as usize] as usize
    } else {
        OC_DCT_REPEAT_RUN3_TOKEN
    }
}

pub fn oc_make_eob_token_full(run_count: i32) -> (usize, i32) {
    if run_count < 32 {
        (
            OC_DCT_EOB_TOKEN[(run_count - 1) as usize] as usize,
            OC_DCT_EOB_EB[(run_count - 1) as usize] as i32,
        )
    } else {
        (OC_DCT_REPEAT_RUN3_TOKEN, run_count)
    }
}

pub fn oc_decode_eob_token(token: usize, eb: i32) -> i32 {
    (((0x20820C41u32 >> (token * 5)) & 0x1F) as i32) + eb
}

pub fn oc_token_bits(
    codes: &[[HuffCode; TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES],
    huffi: usize,
    token: usize,
) -> i32 {
    codes[huffi][token].nbits + i32::from(OC_DCT_TOKEN_EXTRA_BITS[token])
}

pub fn oc_token_bits_zzi(
    codes: &[[HuffCode; TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES],
    huffi: usize,
    zzi: usize,
    token: usize,
) -> i32 {
    codes[huffi + OC_ZZI_HUFF_OFFSET[zzi]][token].nbits + i32::from(OC_DCT_TOKEN_EXTRA_BITS[token])
}

pub fn oc_enc_tokenlog_checkpoint(
    log: &TokenLog<'_>,
    cp: &mut TokenCheckpoint,
    pli: usize,
    zzi: usize,
) {
    cp.pli = pli;
    cp.zzi = zzi;
    cp.eob_run = log.eob_run[pli][zzi];
    cp.ndct_tokens = log.ndct_tokens[pli][zzi];
}

pub fn oc_enc_tokenlog_rollback(log: &mut TokenLog<'_>, stack: &[TokenCheckpoint]) {
    for cp in stack.iter().rev() {
        log.eob_run[cp.pli][cp.zzi] = cp.eob_run;
        log.ndct_tokens[cp.pli][cp.zzi] = cp.ndct_tokens;
        log.dct_tokens[cp.pli][cp.zzi].truncate(cp.ndct_tokens as usize);
        log.extra_bits[cp.pli][cp.zzi].truncate(cp.ndct_tokens as usize);
    }
}

pub fn oc_enc_token_log(log: &mut TokenLog<'_>, pli: usize, zzi: usize, token: usize, eb: i32) {
    let ti = log.ndct_tokens[pli][zzi] as usize;
    if log.dct_tokens[pli][zzi].len() == ti {
        log.dct_tokens[pli][zzi].push(token as u8);
        log.extra_bits[pli][zzi].push(eb as u16);
    } else {
        log.dct_tokens[pli][zzi][ti] = token as u8;
        log.extra_bits[pli][zzi][ti] = eb as u16;
    }
    log.ndct_tokens[pli][zzi] += 1;
}

pub fn oc_enc_eob_log(log: &mut TokenLog<'_>, pli: usize, zzi: usize, run_count: i32) {
    let (token, eb) = oc_make_eob_token_full(run_count);
    oc_enc_token_log(log, pli, zzi, token, eb);
}

pub fn oc_enc_tokenize_start(log: &mut TokenLog<'_>) {
    log.ndct_tokens = [[0; 64]; 3];
    log.eob_run = [[0; 64]; 3];
    log.dct_token_offs = [[0; 64]; 3];
    log.dc_pred_last = [[0; 4]; 3];
    for pli in 0..3 {
        for zzi in 0..64 {
            log.dct_tokens[pli][zzi].clear();
            log.extra_bits[pli][zzi].clear();
        }
    }
}

pub fn oc_value_token(value: i32) -> usize {
    match value {
        1 => 9,
        -1 => 10,
        2 => 11,
        -2 => 12,
        v if (-3..=3).contains(&v) => OC_DCT_VAL_CAT2 + (v.unsigned_abs() > 2) as usize,
        v if (-7..=7).contains(&v) => OC_DCT_VAL_CAT3,
        v if (-15..=15).contains(&v) => OC_DCT_VAL_CAT4,
        v if (-31..=31).contains(&v) => OC_DCT_VAL_CAT5,
        v if (-63..=63).contains(&v) => OC_DCT_VAL_CAT6,
        v if (-127..=127).contains(&v) => OC_DCT_VAL_CAT7,
        _ => OC_DCT_VAL_CAT8,
    }
}

pub fn oc_enc_tokenize_ac(
    log: &mut TokenLog<'_>,
    pli: usize,
    coeffs: &[i16; 64],
    eob: usize,
) -> i32 {
    let mut zzi = 1usize;
    let mut bits = 0i32;
    while zzi < eob.min(64) {
        let mut run = 0usize;
        while zzi + run < eob && coeffs[zzi + run] == 0 {
            run += 1;
        }
        if zzi + run >= eob {
            break;
        }
        let val = coeffs[zzi + run] as i32;
        let token = oc_value_token(val);
        let eb = val.unsigned_abs() as i32;
        if run > 0 {
            let (eob_tok, eob_eb) = oc_make_eob_token_full(run as i32);
            oc_enc_token_log(log, pli, zzi, eob_tok, eob_eb);
            bits += oc_token_bits_zzi(log.huff_codes, 0, zzi, eob_tok);
        }
        oc_enc_token_log(log, pli, zzi + run, token, eb);
        bits += oc_token_bits_zzi(log.huff_codes, 0, zzi + run, token);
        zzi += run + 1;
    }
    if eob < 64 {
        let run = (64 - eob) as i32;
        oc_enc_eob_log(log, pli, eob, run.max(1));
    }
    bits
}

pub fn oc_enc_tokenize_ac_fast(
    log: &mut TokenLog<'_>,
    pli: usize,
    coeffs: &[i16; 64],
    eob: usize,
) -> i32 {
    oc_enc_tokenize_ac(log, pli, coeffs, eob)
}

pub fn oc_enc_pred_dc_frag_rows(
    dc_coeffs: &[i16],
    pred_last: &mut [i32; 4],
    refis: &[u8],
    out: &mut [i16],
) {
    for i in 0..out.len().min(dc_coeffs.len()).min(refis.len()) {
        let refi = refis[i].min(3) as usize;
        let dc = i32::from(dc_coeffs[i]) - pred_last[refi];
        pred_last[refi] = i32::from(dc_coeffs[i]);
        out[i] = dc.clamp(i16::MIN as i32, i16::MAX as i32) as i16;
    }
}

pub fn oc_enc_tokenize_dc_frag_list(
    log: &mut TokenLog<'_>,
    pli: usize,
    dc_coeffs: &[i16],
    refis: &[u8],
) -> i32 {
    let mut pred = [0i32; 4];
    let mut pred_dc = vec![0i16; dc_coeffs.len()];
    oc_enc_pred_dc_frag_rows(dc_coeffs, &mut pred, refis, &mut pred_dc);
    let mut bits = 0i32;
    for (i, &dc) in pred_dc.iter().enumerate() {
        let token = oc_value_token(dc as i32);
        let eb = (dc as i32).unsigned_abs() as i32;
        oc_enc_token_log(log, pli, 0, token, eb);
        bits += oc_token_bits_zzi(log.huff_codes, 0, i.min(63), token);
    }
    bits
}

pub fn oc_enc_tokenize_finish(log: &mut TokenLog<'_>) {
    for pli in 0..3 {
        for zzi in 0..64 {
            if log.eob_run[pli][zzi] > 0 {
                oc_enc_eob_log(log, pli, zzi, i32::from(log.eob_run[pli][zzi]));
                log.eob_run[pli][zzi] = 0;
            }
        }
    }
}
