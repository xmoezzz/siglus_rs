#[inline]
fn abs_i32(v: i32) -> u32 {
    v.unsigned_abs()
}

pub fn oc_enc_frag_sub_c(diff: &mut [i16; 64], src: &[u8], ref_: &[u8], ystride: usize) {
    for i in 0..8 {
        for j in 0..8 {
            diff[i * 8 + j] = src[i * ystride + j] as i16 - ref_[i * ystride + j] as i16;
        }
    }
}

pub fn oc_enc_frag_sub_128_c(diff: &mut [i16; 64], src: &[u8], ystride: usize) {
    for i in 0..8 {
        for j in 0..8 {
            diff[i * 8 + j] = src[i * ystride + j] as i16 - 128;
        }
    }
}

pub fn oc_enc_frag_sad_c(src: &[u8], ref_: &[u8], ystride: usize) -> u32 {
    let mut sad = 0u32;
    for i in 0..8 {
        for j in 0..8 {
            sad += abs_i32(src[i * ystride + j] as i32 - ref_[i * ystride + j] as i32);
        }
    }
    sad
}

pub fn oc_enc_frag_sad_thresh_c(src: &[u8], ref_: &[u8], ystride: usize, thresh: u32) -> u32 {
    let mut sad = 0u32;
    for i in 0..8 {
        for j in 0..8 {
            sad += abs_i32(src[i * ystride + j] as i32 - ref_[i * ystride + j] as i32);
        }
        if sad > thresh {
            break;
        }
    }
    sad
}

pub fn oc_enc_frag_sad2_thresh_c(
    src: &[u8],
    ref1: &[u8],
    ref2: &[u8],
    ystride: usize,
    thresh: u32,
) -> u32 {
    let mut sad = 0u32;
    for i in 0..8 {
        for j in 0..8 {
            let avg = ((ref1[i * ystride + j] as u16 + ref2[i * ystride + j] as u16) >> 1) as i32;
            sad += abs_i32(src[i * ystride + j] as i32 - avg);
        }
        if sad > thresh {
            break;
        }
    }
    sad
}

pub fn oc_enc_frag_intra_sad_c(src: &[u8], ystride: usize) -> u32 {
    let mut dc = 0i32;
    for i in 0..8 {
        for j in 0..8 {
            dc += src[i * ystride + j] as i32;
        }
    }
    dc = (dc + 32) >> 6;
    let mut sad = 0u32;
    for i in 0..8 {
        for j in 0..8 {
            sad += abs_i32(src[i * ystride + j] as i32 - dc);
        }
    }
    sad
}

fn oc_diff_hadamard(buf: &mut [i16; 64], src: &[u8], ref_: &[u8], ystride: usize) {
    for i in 0..8 {
        let s = i * ystride;
        let mut t0 =
            src[s + 0] as i32 - ref_[s + 0] as i32 + src[s + 4] as i32 - ref_[s + 4] as i32;
        let mut t4 =
            src[s + 0] as i32 - ref_[s + 0] as i32 - src[s + 4] as i32 + ref_[s + 4] as i32;
        let mut t1 =
            src[s + 1] as i32 - ref_[s + 1] as i32 + src[s + 5] as i32 - ref_[s + 5] as i32;
        let mut t5 =
            src[s + 1] as i32 - ref_[s + 1] as i32 - src[s + 5] as i32 + ref_[s + 5] as i32;
        let mut t2 =
            src[s + 2] as i32 - ref_[s + 2] as i32 + src[s + 6] as i32 - ref_[s + 6] as i32;
        let mut t6 =
            src[s + 2] as i32 - ref_[s + 2] as i32 - src[s + 6] as i32 + ref_[s + 6] as i32;
        let mut t3 =
            src[s + 3] as i32 - ref_[s + 3] as i32 + src[s + 7] as i32 - ref_[s + 7] as i32;
        let mut t7 =
            src[s + 3] as i32 - ref_[s + 3] as i32 - src[s + 7] as i32 + ref_[s + 7] as i32;
        let mut r = t0;
        t0 += t2;
        t2 = r - t2;
        r = t1;
        t1 += t3;
        t3 = r - t3;
        r = t4;
        t4 += t6;
        t6 = r - t6;
        r = t5;
        t5 += t7;
        t7 = r - t7;
        buf[0 * 8 + i] = (t0 + t1) as i16;
        buf[1 * 8 + i] = (t0 - t1) as i16;
        buf[2 * 8 + i] = (t2 + t3) as i16;
        buf[3 * 8 + i] = (t2 - t3) as i16;
        buf[4 * 8 + i] = (t4 + t5) as i16;
        buf[5 * 8 + i] = (t4 - t5) as i16;
        buf[6 * 8 + i] = (t6 + t7) as i16;
        buf[7 * 8 + i] = (t6 - t7) as i16;
    }
}

fn oc_diff_hadamard2(buf: &mut [i16; 64], src: &[u8], ref1: &[u8], ref2: &[u8], ystride: usize) {
    for i in 0..8 {
        let s = i * ystride;
        let avg0 = ((ref1[s + 0] as u16 + ref2[s + 0] as u16) >> 1) as i32;
        let avg4 = ((ref1[s + 4] as u16 + ref2[s + 4] as u16) >> 1) as i32;
        let mut t0 = src[s + 0] as i32 - avg0 + src[s + 4] as i32 - avg4;
        let mut t4 = src[s + 0] as i32 - avg0 - src[s + 4] as i32 + avg4;
        let avg1 = ((ref1[s + 1] as u16 + ref2[s + 1] as u16) >> 1) as i32;
        let avg5 = ((ref1[s + 5] as u16 + ref2[s + 5] as u16) >> 1) as i32;
        let mut t1 = src[s + 1] as i32 - avg1 + src[s + 5] as i32 - avg5;
        let mut t5 = src[s + 1] as i32 - avg1 - src[s + 5] as i32 + avg5;
        let avg2 = ((ref1[s + 2] as u16 + ref2[s + 2] as u16) >> 1) as i32;
        let avg6 = ((ref1[s + 6] as u16 + ref2[s + 6] as u16) >> 1) as i32;
        let mut t2 = src[s + 2] as i32 - avg2 + src[s + 6] as i32 - avg6;
        let mut t6 = src[s + 2] as i32 - avg2 - src[s + 6] as i32 + avg6;
        let avg3 = ((ref1[s + 3] as u16 + ref2[s + 3] as u16) >> 1) as i32;
        let avg7 = ((ref1[s + 7] as u16 + ref2[s + 7] as u16) >> 1) as i32;
        let mut t3 = src[s + 3] as i32 - avg3 + src[s + 7] as i32 - avg7;
        let mut t7 = src[s + 3] as i32 - avg3 - src[s + 7] as i32 + avg7;
        let mut r = t0;
        t0 += t2;
        t2 = r - t2;
        r = t1;
        t1 += t3;
        t3 = r - t3;
        r = t4;
        t4 += t6;
        t6 = r - t6;
        r = t5;
        t5 += t7;
        t7 = r - t7;
        buf[0 * 8 + i] = (t0 + t1) as i16;
        buf[1 * 8 + i] = (t0 - t1) as i16;
        buf[2 * 8 + i] = (t2 + t3) as i16;
        buf[3 * 8 + i] = (t2 - t3) as i16;
        buf[4 * 8 + i] = (t4 + t5) as i16;
        buf[5 * 8 + i] = (t4 - t5) as i16;
        buf[6 * 8 + i] = (t6 + t7) as i16;
        buf[7 * 8 + i] = (t6 - t7) as i16;
    }
}

fn oc_intra_hadamard(buf: &mut [i16; 64], src: &[u8], ystride: usize) {
    for i in 0..8 {
        let s = i * ystride;
        let mut t0 = src[s + 0] as i32 + src[s + 4] as i32;
        let mut t4 = src[s + 0] as i32 - src[s + 4] as i32;
        let mut t1 = src[s + 1] as i32 + src[s + 5] as i32;
        let mut t5 = src[s + 1] as i32 - src[s + 5] as i32;
        let mut t2 = src[s + 2] as i32 + src[s + 6] as i32;
        let mut t6 = src[s + 2] as i32 - src[s + 6] as i32;
        let mut t3 = src[s + 3] as i32 + src[s + 7] as i32;
        let mut t7 = src[s + 3] as i32 - src[s + 7] as i32;
        let mut r = t0;
        t0 += t2;
        t2 = r - t2;
        r = t1;
        t1 += t3;
        t3 = r - t3;
        r = t4;
        t4 += t6;
        t6 = r - t6;
        r = t5;
        t5 += t7;
        t7 = r - t7;
        buf[0 * 8 + i] = (t0 + t1) as i16;
        buf[1 * 8 + i] = (t0 - t1) as i16;
        buf[2 * 8 + i] = (t2 + t3) as i16;
        buf[3 * 8 + i] = (t2 - t3) as i16;
        buf[4 * 8 + i] = (t4 + t5) as i16;
        buf[5 * 8 + i] = (t4 - t5) as i16;
        buf[6 * 8 + i] = (t6 + t7) as i16;
        buf[7 * 8 + i] = (t6 - t7) as i16;
    }
}

pub fn oc_hadamard_sad(dc: &mut i32, buf: &[i16; 64]) -> u32 {
    let mut sad = 0u32;
    for i in 0..8 {
        let mut t0 = buf[i * 8 + 0] as i32 + buf[i * 8 + 4] as i32;
        let mut t4 = buf[i * 8 + 0] as i32 - buf[i * 8 + 4] as i32;
        let mut t1 = buf[i * 8 + 1] as i32 + buf[i * 8 + 5] as i32;
        let mut t5 = buf[i * 8 + 1] as i32 - buf[i * 8 + 5] as i32;
        let mut t2 = buf[i * 8 + 2] as i32 + buf[i * 8 + 6] as i32;
        let mut t6 = buf[i * 8 + 2] as i32 - buf[i * 8 + 6] as i32;
        let mut t3 = buf[i * 8 + 3] as i32 + buf[i * 8 + 7] as i32;
        let mut t7 = buf[i * 8 + 3] as i32 - buf[i * 8 + 7] as i32;
        let mut r = t0;
        t0 += t2;
        t2 = r - t2;
        r = t1;
        t1 += t3;
        t3 = r - t3;
        r = t4;
        t4 += t6;
        t6 = r - t6;
        r = t5;
        t5 += t7;
        t7 = r - t7;
        let mut row = if i > 0 { abs_i32(t0 + t1) } else { 0 };
        row += abs_i32(t0 - t1);
        row += abs_i32(t2 + t3);
        row += abs_i32(t2 - t3);
        row += abs_i32(t4 + t5);
        row += abs_i32(t4 - t5);
        row += abs_i32(t6 + t7);
        row += abs_i32(t6 - t7);
        sad += row;
    }
    *dc = buf[0..8].iter().map(|&v| v as i32).sum();
    sad
}

pub fn oc_enc_frag_satd_c(dc: &mut i32, src: &[u8], ref_: &[u8], ystride: usize) -> u32 {
    let mut buf = [0i16; 64];
    oc_diff_hadamard(&mut buf, src, ref_, ystride);
    oc_hadamard_sad(dc, &buf)
}

pub fn oc_enc_frag_satd2_c(
    dc: &mut i32,
    src: &[u8],
    ref1: &[u8],
    ref2: &[u8],
    ystride: usize,
) -> u32 {
    let mut buf = [0i16; 64];
    oc_diff_hadamard2(&mut buf, src, ref1, ref2, ystride);
    oc_hadamard_sad(dc, &buf)
}

pub fn oc_enc_frag_intra_satd_c(dc: &mut i32, src: &[u8], ystride: usize) -> u32 {
    let mut buf = [0i16; 64];
    oc_intra_hadamard(&mut buf, src, ystride);
    oc_hadamard_sad(dc, &buf)
}

pub fn oc_enc_frag_ssd_c(src: &[u8], ref_: &[u8], ystride: usize) -> u32 {
    let mut ret = 0u32;
    for y in 0..8 {
        for x in 0..8 {
            let d = src[y * ystride + x] as i32 - ref_[y * ystride + x] as i32;
            ret += (d * d) as u32;
        }
    }
    ret
}

pub fn oc_enc_frag_border_ssd_c(src: &[u8], ref_: &[u8], ystride: usize, mut mask: i64) -> u32 {
    let mut ret = 0u32;
    for y in 0..8 {
        for x in 0..8 {
            if (mask & 1) != 0 {
                let d = src[y * ystride + x] as i32 - ref_[y * ystride + x] as i32;
                ret += (d * d) as u32;
            }
            mask >>= 1;
        }
    }
    ret
}

pub fn oc_enc_frag_copy2_c(dst: &mut [u8], src1: &[u8], src2: &[u8], ystride: usize) {
    for i in 0..8 {
        for j in 0..8 {
            dst[i * ystride + j] =
                ((src1[i * ystride + j] as u16 + src2[i * ystride + j] as u16) >> 1) as u8;
        }
    }
}
