pub const fn q57(v: i32) -> i64 {
    (v as i64) << 57
}

pub const fn q10(v: i32) -> i32 {
    v << 10
}

#[inline]
pub fn oc_ilog(v: u32) -> i32 {
    if v == 0 {
        0
    } else {
        32 - v.leading_zeros() as i32
    }
}

#[inline]
pub fn oc_ilog32(v: u32) -> i32 {
    oc_ilog(v)
}

#[inline]
pub fn oc_ilog64(v: i64) -> i32 {
    if v <= 0 {
        0
    } else {
        64 - (v as u64).leading_zeros() as i32
    }
}

#[inline]
fn xor_mask(value: i64, mask: i64) -> i64 {
    value.wrapping_add(mask) ^ mask
}

const OC_ATANH_LOG2: [i64; 32] = [
    0x32B803473F7AD0F4,
    0x2F2A71BD4E25E916,
    0x2E68B244BB93BA06,
    0x2E39FB9198CE62E4,
    0x2E2E683F68565C8F,
    0x2E2B850BE2077FC1,
    0x2E2ACC58FE7B78DB,
    0x2E2A9E2DE52FD5F2,
    0x2E2A92A338D53EEC,
    0x2E2A8FC08F5E19B6,
    0x2E2A8F07E51A485E,
    0x2E2A8ED9BA8AF388,
    0x2E2A8ECE2FE7384A,
    0x2E2A8ECB4D3E4B1A,
    0x2E2A8ECA94940FE8,
    0x2E2A8ECA6669811D,
    0x2E2A8ECA5ADEDD6A,
    0x2E2A8ECA57FC347E,
    0x2E2A8ECA57438A43,
    0x2E2A8ECA57155FB4,
    0x2E2A8ECA5709D510,
    0x2E2A8ECA5706F267,
    0x2E2A8ECA570639BD,
    0x2E2A8ECA57060B92,
    0x2E2A8ECA57060008,
    0x2E2A8ECA5705FD25,
    0x2E2A8ECA5705FC6C,
    0x2E2A8ECA5705FC3E,
    0x2E2A8ECA5705FC33,
    0x2E2A8ECA5705FC30,
    0x2E2A8ECA5705FC2F,
    0x2E2A8ECA5705FC2F,
];

pub fn oc_bexp64(z_in: i64) -> i64 {
    let ipart = (z_in >> 57) as i32;
    if ipart < 0 {
        return 0;
    }
    if ipart >= 63 {
        return i64::MAX;
    }
    let mut z = z_in.wrapping_sub(q57(ipart));
    let mut w: i64;
    if z != 0 {
        z = z.wrapping_mul(32);
        w = 0x26A3D0E401DD846D;
        let mut i = 0usize;
        loop {
            let mask = if z < 0 { -1 } else { 0 };
            w = w.wrapping_add(xor_mask(w >> (i + 1), mask));
            z = z.wrapping_sub(xor_mask(OC_ATANH_LOG2[i], mask));
            if i >= 3 {
                break;
            }
            z = z.wrapping_mul(2);
            i += 1;
        }
        loop {
            let mask = if z < 0 { -1 } else { 0 };
            w = w.wrapping_add(xor_mask(w >> (i + 1), mask));
            z = z.wrapping_sub(xor_mask(OC_ATANH_LOG2[i], mask));
            if i >= 12 {
                break;
            }
            z = z.wrapping_mul(2);
            i += 1;
        }
        while i < 32 {
            let mask = if z < 0 { -1 } else { 0 };
            w = w.wrapping_add(xor_mask(w >> (i + 1), mask));
            z = z
                .wrapping_sub(xor_mask(OC_ATANH_LOG2[i], mask))
                .wrapping_mul(2);
            i += 1;
        }
        let mut wlo = 0i64;
        if ipart > 30 {
            loop {
                let mask = if z < 0 { -1 } else { 0 };
                wlo = wlo.wrapping_add(xor_mask(w >> i, mask));
                z = z.wrapping_sub(xor_mask(OC_ATANH_LOG2[31], mask));
                if i >= 39 {
                    break;
                }
                z = z.wrapping_mul(2);
                i += 1;
            }
            while i < 61 {
                let mask = if z < 0 { -1 } else { 0 };
                wlo = wlo.wrapping_add(xor_mask(w >> i, mask));
                z = z
                    .wrapping_sub(xor_mask(OC_ATANH_LOG2[31], mask))
                    .wrapping_mul(2);
                i += 1;
            }
        }
        w = (w << 1).wrapping_add(wlo);
    } else {
        w = 1i64 << 62;
    }
    if ipart < 62 {
        w = ((w >> (61 - ipart)) + 1) >> 1;
    }
    w
}

pub fn oc_blog64(w_in: i64) -> i64 {
    if w_in <= 0 {
        return -1;
    }
    let ipart = oc_ilog64(w_in) - 1;
    let mut w = if ipart > 61 {
        w_in >> (ipart - 61)
    } else {
        w_in << (61 - ipart)
    };
    let mut z = 0i64;
    if (w & (w - 1)) != 0 {
        let mut x = w + (1i64 << 61);
        let mut y = w - (1i64 << 61);
        let mut i = 0usize;
        while i < 4 {
            let mask = if y < 0 { -1 } else { 0 };
            z = z.wrapping_add(xor_mask(OC_ATANH_LOG2[i] >> i, mask));
            let u = x >> (i + 1);
            x = x.wrapping_sub(xor_mask(y >> (i + 1), mask));
            y = y.wrapping_sub(xor_mask(u, mask));
            i += 1;
        }
        i -= 1;
        while i < 13 {
            let mask = if y < 0 { -1 } else { 0 };
            z = z.wrapping_add(xor_mask(OC_ATANH_LOG2[i] >> i, mask));
            let u = x >> (i + 1);
            x = x.wrapping_sub(xor_mask(y >> (i + 1), mask));
            y = y.wrapping_sub(xor_mask(u, mask));
            i += 1;
        }
        i -= 1;
        while i < 32 {
            let mask = if y < 0 { -1 } else { 0 };
            z = z.wrapping_add(xor_mask(OC_ATANH_LOG2[i] >> i, mask));
            let u = x >> (i + 1);
            x = x.wrapping_sub(xor_mask(y >> (i + 1), mask));
            y = y.wrapping_sub(xor_mask(u, mask));
            i += 1;
        }
        while i < 40 {
            let mask = if y < 0 { -1 } else { 0 };
            z = z.wrapping_add(xor_mask(OC_ATANH_LOG2[31] >> i, mask));
            let u = x >> (i + 1);
            x = x.wrapping_sub(xor_mask(y >> (i + 1), mask));
            y = y.wrapping_sub(xor_mask(u, mask));
            i += 1;
        }
        i -= 1;
        while i < 62 {
            let mask = if y < 0 { -1 } else { 0 };
            z = z.wrapping_add(xor_mask(OC_ATANH_LOG2[31] >> i, mask));
            let u = x >> (i + 1);
            x = x.wrapping_sub(xor_mask(y >> (i + 1), mask));
            y = y.wrapping_sub(xor_mask(u, mask));
            i += 1;
        }
        z = (z + 8) >> 4;
    }
    q57(ipart) + z
}

pub fn oc_bexp32_q10(z: i32) -> u32 {
    let ipart = z >> 10;
    let mut n = ((z & ((1 << 10) - 1)) << 4) as i64;
    n = (n * ((n * ((n * ((n * 3548 >> 15) + 6817) >> 15) + 15823) >> 15) + 22708) >> 15) + 16384;
    if 14 - ipart > 0 {
        (((n + (1i64 << (13 - ipart))) >> (14 - ipart)) as u32)
    } else {
        ((n as u32) << (ipart - 14))
    }
}

pub fn oc_blog32_q10(w: u32) -> i32 {
    if w == 0 {
        return -1;
    }
    let ipart = oc_ilog32(w);
    let n = if ipart - 16 > 0 {
        (w >> (ipart - 16)) as i32
    } else {
        (w << (16 - ipart)) as i32
    } - 32768
        - 16384;
    let fpart =
        (n * ((n * ((n * ((n * -1402 >> 15) + 2546) >> 15) - 5216) >> 15) + 15745) >> 15) - 6793;
    (ipart << 10) + (fpart >> 4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ilog_matches_expected() {
        assert_eq!(oc_ilog32(0), 0);
        assert_eq!(oc_ilog32(1), 1);
        assert_eq!(oc_ilog32(2), 2);
        assert_eq!(oc_ilog32(3), 2);
        assert_eq!(oc_ilog32(4), 3);
    }

    #[test]
    fn q10_exp_log_are_rough_inverses() {
        let v = oc_bexp32_q10(q10(5));
        assert!(v >= 32);
        let l = oc_blog32_q10(32);
        assert!(l > q10(4) && l < q10(6));
    }
}
