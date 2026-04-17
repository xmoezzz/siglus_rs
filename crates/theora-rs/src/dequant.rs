use crate::bitpack::PackBuf;
use crate::codec::{QuantBase, QuantInfo, QuantRanges};
use crate::error::{Result, TheoraError};
use crate::mathops::oc_ilog;

pub fn oc_quant_params_unpack(opb: &mut PackBuf<'_>, qinfo: &mut QuantInfo) -> Result<()> {
    let mut base_mats: Vec<QuantBase>;
    let mut sizes = [0i32; 64];
    let mut indices = [0usize; 64];

    let mut nbits = opb.read(3) as i32;
    for qi in 0..64 {
        qinfo.loop_filter_limits[qi] = opb.read(nbits) as u8;
    }

    nbits = opb.read(4) as i32 + 1;
    for qi in 0..64 {
        qinfo.ac_scale[qi] = opb.read(nbits) as u16;
    }

    nbits = opb.read(4) as i32 + 1;
    for qi in 0..64 {
        qinfo.dc_scale[qi] = opb.read(nbits) as u16;
    }

    let nbase_mats = opb.read(9) as usize + 1;
    base_mats = vec![[0u8; 64]; nbase_mats];
    for mat in &mut base_mats {
        for coeff in mat.iter_mut() {
            *coeff = opb.read(8) as u8;
        }
    }

    nbits = oc_ilog((nbase_mats.saturating_sub(1)) as u32);
    for i in 0..6 {
        let qti = i / 3;
        let pli = i % 3;
        if i > 0 {
            let duplicate = opb.read1();
            if duplicate == 0 {
                let (qtj, plj) = if qti > 0 {
                    let intra_same = opb.read1();
                    if intra_same != 0 {
                        (qti - 1, pli)
                    } else {
                        ((i - 1) / 3, (i - 1) % 3)
                    }
                } else {
                    ((i - 1) / 3, (i - 1) % 3)
                };
                qinfo.qi_ranges[qti][pli] = qinfo.qi_ranges[qtj][plj].clone();
                continue;
            }
        }

        indices[0] = opb.read(nbits) as usize;
        let mut qi = 0i32;
        let mut qri = 0usize;
        while qi < 63 {
            let bits = oc_ilog((62 - qi) as u32);
            let val = opb.read(bits) as i32;
            sizes[qri] = val + 1;
            qi += val + 1;
            qri += 1;
            indices[qri] = opb.read(nbits) as usize;
        }
        if qi > 63 {
            return Err(TheoraError::BadHeader);
        }

        let mut qranges = QuantRanges::default();
        qranges.sizes.extend_from_slice(&sizes[..qri]);
        qranges.base_matrices.reserve(qri + 1);
        for idx in 0..=qri {
            let bmi = indices[idx];
            if bmi >= nbase_mats {
                return Err(TheoraError::BadHeader);
            }
            qranges.base_matrices.push(base_mats[bmi]);
        }
        qinfo.qi_ranges[qti][pli] = qranges;
    }
    Ok(())
}

pub fn oc_quant_params_clear(qinfo: &mut QuantInfo) {
    qinfo.clear();
}
