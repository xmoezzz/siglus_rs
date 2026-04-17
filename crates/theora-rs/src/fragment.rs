fn clamp255(v: i32) -> u8 {
    if v < 0 {
        0
    } else if v > 255 {
        255
    } else {
        v as u8
    }
}

fn idx(base: isize, x: isize, ystride: isize) -> usize {
    (base + x * ystride) as usize
}

pub fn oc_frag_copy_c(
    dst_frame: &mut [u8],
    dst_off: isize,
    src_frame: &[u8],
    src_off: isize,
    ystride: isize,
) {
    let mut dst = dst_off;
    let mut src = src_off;
    for _ in 0..8 {
        let d = dst as usize;
        let s = src as usize;
        dst_frame[d..d + 8].copy_from_slice(&src_frame[s..s + 8]);
        dst += ystride;
        src += ystride;
    }
}

pub fn oc_frag_copy_list_c(
    dst_frame: &mut [u8],
    src_frame: &[u8],
    ystride: isize,
    fragis: &[usize],
    frag_buf_offs: &[isize],
) {
    for &fragi in fragis {
        let off = frag_buf_offs[fragi];
        oc_frag_copy_c(dst_frame, off, src_frame, off, ystride);
    }
}

pub fn oc_frag_recon_intra_c(
    dst_frame: &mut [u8],
    dst_off: isize,
    ystride: isize,
    residue: &[i16; 64],
) {
    let mut dst = dst_off;
    for i in 0..8 {
        let d = dst as usize;
        for j in 0..8 {
            dst_frame[d + j] = clamp255(residue[i * 8 + j] as i32 + 128);
        }
        dst += ystride;
    }
}

pub fn oc_frag_recon_inter_c(
    dst_frame: &mut [u8],
    dst_off: isize,
    src_frame: &[u8],
    src_off: isize,
    ystride: isize,
    residue: &[i16; 64],
) {
    let mut dst = dst_off;
    let mut src = src_off;
    for i in 0..8 {
        let d = dst as usize;
        let s = src as usize;
        for j in 0..8 {
            dst_frame[d + j] = clamp255(residue[i * 8 + j] as i32 + src_frame[s + j] as i32);
        }
        dst += ystride;
        src += ystride;
    }
}

pub fn oc_frag_recon_inter2_c(
    dst_frame: &mut [u8],
    dst_off: isize,
    src1_frame: &[u8],
    src1_off: isize,
    src2_frame: &[u8],
    src2_off: isize,
    ystride: isize,
    residue: &[i16; 64],
) {
    let mut dst = dst_off;
    let mut src1 = src1_off;
    let mut src2 = src2_off;
    for i in 0..8 {
        let d = dst as usize;
        let s1 = src1 as usize;
        let s2 = src2 as usize;
        for j in 0..8 {
            let pred = ((src1_frame[s1 + j] as i32) + (src2_frame[s2 + j] as i32)) >> 1;
            dst_frame[d + j] = clamp255(residue[i * 8 + j] as i32 + pred);
        }
        dst += ystride;
        src1 += ystride;
        src2 += ystride;
    }
}

pub fn oc_restore_fpu_c() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intra_recon_clamps() {
        let mut dst = vec![0u8; 64];
        let mut residue = [0i16; 64];
        residue[0] = 200;
        residue[1] = -300;
        oc_frag_recon_intra_c(&mut dst, 0, 8, &residue);
        assert_eq!(dst[0], 255);
        assert_eq!(dst[1], 0);
    }
}
