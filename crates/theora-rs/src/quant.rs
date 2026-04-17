use crate::codec::{QuantInfo, QuantRanges};
use crate::error::{Result, TheoraError};
use crate::internal::OC_FZIG_ZAG;

pub type QuantTable = [u16; 64];
pub type DequantTables = [[[QuantTable; 2]; 3]; 64];

pub const OC_QUANT_MAX: u32 = 1024 << 2;
const OC_DC_QUANT_MIN: [u32; 2] = [4 << 2, 8 << 2];
const OC_AC_QUANT_MIN: [u32; 2] = [2 << 2, 4 << 2];

fn clampi(min_v: u32, v: u32, max_v: u32) -> u32 {
    v.max(min_v).min(max_v)
}

fn interp_matrix(
    qranges: &QuantRanges,
    qri: usize,
    qi_start: i32,
    qi_end: i32,
    qi: i32,
) -> [u8; 64] {
    let mut base = [0u8; 64];
    for ci in 0..64 {
        base[ci] = ((2
            * ((qi_end - qi) * i32::from(qranges.base_matrices[qri][ci])
                + (qi - qi_start) * i32::from(qranges.base_matrices[qri + 1][ci]))
            + qranges.sizes[qri])
            / (2 * qranges.sizes[qri])) as u8;
    }
    base
}

pub fn oc_dequant_tables_init(qinfo: &QuantInfo) -> Result<(DequantTables, [i32; 64])> {
    let mut dequant = [[[[0u16; 64]; 2]; 3]; 64];
    let mut pp_dc_scale = [0i32; 64];

    for qti in 0..2 {
        for pli in 0..3 {
            let qranges = &qinfo.qi_ranges[qti][pli];
            if qranges.base_matrices.len() != qranges.sizes.len() + 1 {
                return Err(TheoraError::BadHeader);
            }
            let mut qi = 0i32;
            for qri in 0..=qranges.sizes.len() {
                let qi_start = qi;
                let qi_end = if qri == qranges.sizes.len() {
                    qi + 1
                } else {
                    qi + qranges.sizes[qri]
                };
                let mut base = qranges.base_matrices[qri];
                loop {
                    let qfac = u32::from(qinfo.dc_scale[qi as usize]) * u32::from(base[0]);
                    pp_dc_scale[qi as usize] = (qfac / 160) as i32;
                    let q = clampi(OC_DC_QUANT_MIN[qti], (qfac / 100) << 2, OC_QUANT_MAX);
                    dequant[qi as usize][pli][qti][0] = q as u16;
                    for zzi in 1..64 {
                        let q = ((u32::from(qinfo.ac_scale[qi as usize])
                            * u32::from(base[OC_FZIG_ZAG[zzi] as usize]))
                            / 100)
                            << 2;
                        let q = clampi(OC_AC_QUANT_MIN[qti], q, OC_QUANT_MAX);
                        dequant[qi as usize][pli][qti][zzi] = q as u16;
                    }
                    qi += 1;
                    if qi >= qi_end {
                        break;
                    }
                    if qri < qranges.sizes.len() {
                        base = interp_matrix(qranges, qri, qi_start, qi_end, qi);
                    }
                }
            }
            if qi != 64 {
                return Err(TheoraError::BadHeader);
            }
        }
    }

    Ok((dequant, pp_dc_scale))
}
