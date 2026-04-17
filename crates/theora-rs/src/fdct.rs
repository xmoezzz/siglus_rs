use crate::dct::{OC_C1S7, OC_C2S6, OC_C3S5, OC_C4S4, OC_C5S3, OC_C6S2, OC_C7S1};
use crate::internal::OC_FZIG_ZAG;

fn oc_fdct8(y: &mut [i16; 8], x: &[i16], offset: usize, stride: usize) {
    let mut t0 = x[offset + (0 << 3) * stride] as i32 + x[offset + (7 << 3) * stride] as i32;
    let mut t7 = x[offset + (0 << 3) * stride] as i32 - x[offset + (7 << 3) * stride] as i32;
    let mut t1 = x[offset + (1 << 3) * stride] as i32 + x[offset + (6 << 3) * stride] as i32;
    let mut t6 = x[offset + (1 << 3) * stride] as i32 - x[offset + (6 << 3) * stride] as i32;
    let mut t2 = x[offset + (2 << 3) * stride] as i32 + x[offset + (5 << 3) * stride] as i32;
    let mut t5 = x[offset + (2 << 3) * stride] as i32 - x[offset + (5 << 3) * stride] as i32;
    let mut t3 = x[offset + (3 << 3) * stride] as i32 + x[offset + (4 << 3) * stride] as i32;
    let mut t4 = x[offset + (3 << 3) * stride] as i32 - x[offset + (4 << 3) * stride] as i32;

    let mut r = t0 + t3;
    t3 = t0 - t3;
    t0 = r;
    r = t1 + t2;
    t2 = t1 - t2;
    t1 = r;
    r = t6 + t5;
    t5 = t6 - t5;
    t6 = r;

    let mut s = ((27146 * t5 + 0xB500) >> 16) + t5 + i32::from(t5 != 0);
    s >>= 1;
    r = t4 + s;
    t5 = t4 - s;
    t4 = r;

    s = ((27146 * t6 + 0xB500) >> 16) + t6 + i32::from(t6 != 0);
    s >>= 1;
    r = t7 + s;
    t6 = t7 - s;
    t7 = r;

    r = ((27146 * t0 + 0x4000) >> 16) + t0 + i32::from(t0 != 0);
    s = ((27146 * t1 + 0xB500) >> 16) + t1 + i32::from(t1 != 0);
    let u = (r + s) >> 1;
    let v = r - u;
    y[0] = u as i16;
    y[4] = v as i16;

    let u = ((OC_C6S2 * t2 + OC_C2S6 * t3 + 0x6CB7) >> 16) + i32::from(t3 != 0);
    s = ((OC_C6S2 * u) >> 16) - t2;
    let v = ((s * 21600 + 0x2800) >> 18) + s + i32::from(s != 0);
    y[2] = u as i16;
    y[6] = v as i16;

    let u = ((OC_C5S3 * t6 + OC_C3S5 * t5 + 0x0E3D) >> 16) + i32::from(t5 != 0);
    s = t6 - ((OC_C5S3 * u) >> 16);
    let v = ((s * 26568 + 0x3400) >> 17) + s + i32::from(s != 0);
    y[5] = u as i16;
    y[3] = v as i16;

    let u = ((OC_C7S1 * t4 + OC_C1S7 * t7 + 0x7B1B) >> 16) + i32::from(t7 != 0);
    s = ((OC_C7S1 * u) >> 16) - t4;
    let v = ((s * 20539 + 0x3000) >> 20) + s + i32::from(s != 0);
    y[1] = u as i16;
    y[7] = v as i16;
}

pub fn oc_enc_fdct8x8_c(y: &mut [i16; 64], x: &[i16; 64]) {
    let mut w = [0i16; 64];
    for i in 0..64 {
        w[i] = x[i] << 2;
    }
    w[0] += i16::from(w[0] != 0) + 1;
    w[1] += 1;
    w[8] -= 1;

    for col in 0..8 {
        let mut out = [0i16; 8];
        oc_fdct8(&mut out, &w, col, 1);
        for row in 0..8 {
            y[col * 8 + row] = out[row];
        }
    }

    let mut tmp = [0i16; 64];
    for col in 0..8 {
        let mut out = [0i16; 8];
        oc_fdct8(&mut out, y, col, 1);
        for row in 0..8 {
            tmp[col * 8 + row] = out[row];
        }
    }

    for i in 0..64 {
        y[i] = (tmp[OC_FZIG_ZAG[i] as usize] + 2) >> 2;
    }
}
