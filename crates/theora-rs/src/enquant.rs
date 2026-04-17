use std::array::from_fn;

use crate::codec::QuantInfo;
use crate::internal::OC_IZIG_ZAG;
use crate::mathops::{oc_blog32_q10, oc_blog64, q10, q57};
use crate::quant::{DequantTables, QuantTable};

pub const OC_BIT_SCALE: i32 = 6;
pub const OC_SAD_SHIFT: usize = 6;
pub const OC_SATD_SHIFT: usize = 9;
pub const OC_RD_SCALE_BITS: i32 = 12 - OC_BIT_SCALE;
pub const OC_RD_ISCALE_BITS: i32 = 11;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OcIQuant {
    pub m: i16,
    pub l: i32,
}

pub type EnquantTables = [[Vec<[OcIQuant; 64]>; 2]; 3];

#[inline]
fn ilog_nz_32(v: u32) -> i32 {
    32 - v.leading_zeros() as i32
}

#[inline]
fn signmask(v: i32) -> i32 {
    v >> 31
}

#[inline]
fn clampi(min_v: i32, v: i32, max_v: i32) -> i32 {
    v.max(min_v).min(max_v)
}

#[inline]
fn rd_iscale(lambda: u32, rd_iscale: u32) -> u32 {
    ((lambda * rd_iscale + ((1u32 << OC_RD_ISCALE_BITS) >> 1)) >> OC_RD_ISCALE_BITS) as u32
}

pub fn oc_quant_params_clone(dst: &mut QuantInfo, src: &QuantInfo) -> i32 {
    *dst = src.clone();
    0
}

pub fn oc_iquant_init(this: &mut OcIQuant, mut d: u16) {
    d <<= 1;
    let l = ilog_nz_32(d as u32) - 1;
    let t = 1 + ((1u32 << (16 + l)) / d as u32);
    this.m = (t as i32 - 0x10000) as i16;
    this.l = l;
}

pub fn oc_enc_enquant_table_init_c(enquant: &mut [OcIQuant; 64], dequant: &QuantTable) {
    for zzi in 0..64 {
        oc_iquant_init(&mut enquant[zzi], dequant[zzi]);
    }
}

pub fn oc_enc_enquant_table_fixup_c(enquant: &mut EnquantTables, nqis: usize) {
    for plane in enquant.iter_mut() {
        for qti_tables in plane.iter_mut() {
            if qti_tables.is_empty() {
                continue;
            }
            let head = qti_tables[0];
            while qti_tables.len() < nqis {
                qti_tables.push(head);
            }
            for qii in 1..nqis.min(qti_tables.len()) {
                qti_tables[qii] = head;
            }
        }
    }
}

pub fn oc_enc_quantize_c(
    qdct: &mut [i16; 64],
    dct: &[i16; 64],
    dequant: &QuantTable,
    enquant: &[OcIQuant; 64],
) -> i32 {
    let mut nonzero = 0i32;
    for zzi in 0..64 {
        let mut val = dct[zzi] as i32;
        let d = dequant[zzi] as i32;
        val <<= 1;
        if val.abs() >= d {
            let s = signmask(val);
            val += d + (s ^ s);
            val = (((enquant[zzi].m as i32 * val) >> 16) + val) >> enquant[zzi].l;
            val -= s;
            qdct[zzi] = val as i16;
            nonzero = zzi as i32;
        } else {
            qdct[zzi] = 0;
        }
    }
    nonzero
}

const OC_RPSD: [[u16; 64]; 2] = [
    [
        52725, 17370, 10399, 6867, 5115, 3798, 2942, 2076, 17370, 9900, 6948, 4994, 3836, 2869,
        2229, 1619, 10399, 6948, 5516, 4202, 3376, 2573, 2015, 1461, 6867, 4994, 4202, 3377, 2800,
        2164, 1718, 1243, 5115, 3836, 3376, 2800, 2391, 1884, 1530, 1091, 3798, 2869, 2573, 2164,
        1884, 1495, 1212, 873, 2942, 2229, 2015, 1718, 1530, 1212, 1001, 704, 2076, 1619, 1461,
        1243, 1091, 873, 704, 474,
    ],
    [
        23411, 15604, 13529, 11601, 10683, 8958, 7840, 6142, 15604, 11901, 10718, 9108, 8290, 6961,
        6023, 4487, 13529, 10718, 9961, 8527, 7945, 6689, 5742, 4333, 11601, 9108, 8527, 7414,
        7084, 5923, 5175, 3743, 10683, 8290, 7945, 7084, 6771, 5754, 4793, 3504, 8958, 6961, 6689,
        5923, 5754, 4679, 3936, 2989, 7840, 6023, 5742, 5175, 4793, 3936, 3522, 2558, 6142, 4487,
        4333, 3743, 3504, 2989, 2558, 1829,
    ],
];

const OC_PCD: [[u16; 3]; 4] = [
    [59926, 3038, 2572],
    [55201, 5597, 4738],
    [55201, 5597, 4738],
    [47682, 9669, 8185],
];

pub fn oc_enquant_qavg_init(
    log_qavg: &mut [[i64; 64]; 2],
    log_plq: &mut [[[i16; 2]; 3]; 64],
    chroma_rd_scale: &mut [[[u16; 2]; 64]; 2],
    dequant: &DequantTables,
    pixel_fmt: usize,
) {
    for qti in 0..2 {
        for qi in 0..64 {
            let mut q2 = 0i64;
            let mut qp = [0u32; 3];
            for pli in 0..3 {
                for ci in 0..64 {
                    let qd = dequant[qi][pli][qti][OC_IZIG_ZAG[ci] as usize] as u32;
                    let rq = (OC_RPSD[qti][ci] as u32 + (qd >> 1)) / qd.max(1);
                    qp[pli] += rq * rq;
                }
                q2 += OC_PCD[pixel_fmt][pli] as i64 * qp[pli] as i64;
                log_plq[qi][pli][qti] = ((q10(32) - oc_blog32_q10(qp[pli].max(1))) >> 1) as i16;
            }
            let d = (OC_PCD[pixel_fmt][1] + OC_PCD[pixel_fmt][2]) as u32;
            let cqp = ((OC_PCD[pixel_fmt][1] as u64 * qp[1] as u64
                + OC_PCD[pixel_fmt][2] as u64 * qp[2] as u64
                + (d as u64 >> 1))
                / d as u64) as u32;
            let mut v =
                ((qp[0] + ((1u32 << (OC_RD_SCALE_BITS - 1)) as u32)) >> OC_RD_SCALE_BITS).max(1);
            v = clampi(
                (1 << (OC_RD_SCALE_BITS - 2)) as i32,
                ((cqp + (v >> 1)) / v) as i32,
                (4 << OC_RD_SCALE_BITS) as i32,
            ) as u32;
            chroma_rd_scale[qti][qi][0] = v as u16;
            let denom = rd_iscale(cqp.max(1), 1).max(1);
            let v = clampi(
                (1 << (OC_RD_ISCALE_BITS - 2)) as i32,
                ((qp[0] + (denom >> 1)) / denom) as i32,
                (4 << OC_RD_ISCALE_BITS) as i32,
            ) as u32;
            chroma_rd_scale[qti][qi][1] = v as u16;
            log_qavg[qti][qi] = (q57(48) - oc_blog64(q2.max(1))) >> 1;
        }
    }
}

pub fn empty_enquant_tables() -> EnquantTables {
    from_fn(|_| from_fn(|_| Vec::new()))
}

pub fn oc_quant_params_pack(writer: &mut crate::packet::PackWriter, qinfo: &QuantInfo) {
    let mut max_lf = 0u8;
    for &v in &qinfo.loop_filter_limits {
        max_lf = max_lf.max(v);
    }
    let mut nbits = if max_lf == 0 {
        0
    } else {
        32 - (max_lf as u32).leading_zeros() as usize
    };
    writer.write(nbits as u32, 3);
    for &v in &qinfo.loop_filter_limits {
        writer.write(v as u32, nbits);
    }

    let mut max_ac = 1u16;
    for &v in &qinfo.ac_scale {
        max_ac = max_ac.max(v);
    }
    nbits = (32 - (max_ac as u32).leading_zeros()) as usize;
    writer.write((nbits.saturating_sub(1)) as u32, 4);
    for &v in &qinfo.ac_scale {
        writer.write(v as u32, nbits);
    }

    let mut max_dc = 1u16;
    for &v in &qinfo.dc_scale {
        max_dc = max_dc.max(v);
    }
    nbits = (32 - (max_dc as u32).leading_zeros()) as usize;
    writer.write((nbits.saturating_sub(1)) as u32, 4);
    for &v in &qinfo.dc_scale {
        writer.write(v as u32, nbits);
    }

    let mut base_mats: Vec<[u8; 64]> = Vec::new();
    let mut indices = [[[0usize; 64]; 3]; 2];
    for qti in 0..2usize {
        for pli in 0..3usize {
            let qranges = &qinfo.qi_ranges[qti][pli];
            for qri in 0..qranges.base_matrices.len() {
                let mat = qranges.base_matrices[qri];
                let idx = base_mats.iter().position(|m| *m == mat).unwrap_or_else(|| {
                    let idx = base_mats.len();
                    base_mats.push(mat);
                    idx
                });
                indices[qti][pli][qri] = idx;
            }
        }
    }
    writer.write((base_mats.len().saturating_sub(1)) as u32, 9);
    for mat in &base_mats {
        for &coeff in mat {
            writer.write(coeff as u32, 8);
        }
    }

    nbits = if base_mats.len() <= 1 {
        0
    } else {
        32 - ((base_mats.len() - 1) as u32).leading_zeros() as usize
    };
    for i in 0..6usize {
        let qti = i / 3;
        let pli = i % 3;
        let qranges = &qinfo.qi_ranges[qti][pli];
        if i > 0 {
            if qti > 0 {
                let prev = &qinfo.qi_ranges[qti - 1][pli];
                if qranges.sizes == prev.sizes
                    && qranges.base_matrices.len() == prev.base_matrices.len()
                    && indices[qti][pli][..qranges.base_matrices.len()]
                        == indices[qti - 1][pli][..prev.base_matrices.len()]
                {
                    writer.write(1, 2);
                    continue;
                }
            }
            let qtj = (i - 1) / 3;
            let plj = (i - 1) % 3;
            let prev = &qinfo.qi_ranges[qtj][plj];
            if qranges.sizes == prev.sizes
                && qranges.base_matrices.len() == prev.base_matrices.len()
                && indices[qti][pli][..qranges.base_matrices.len()]
                    == indices[qtj][plj][..prev.base_matrices.len()]
            {
                writer.write(0, 1 + usize::from(qti > 0));
                continue;
            }
            writer.write(1, 1);
        }
        writer.write(indices[qti][pli][0] as u32, nbits);
        let mut qi = 0usize;
        let mut qri = 0usize;
        while qi < 63 {
            let bits = 32 - ((62 - qi) as u32).leading_zeros() as usize;
            writer.write((qranges.sizes[qri] - 1) as u32, bits);
            qi += qranges.sizes[qri] as usize;
            writer.write(indices[qti][pli][qri + 1] as u32, nbits);
            qri += 1;
        }
    }
}
