use crate::dct::{OC_C1S7, OC_C2S6, OC_C3S5, OC_C4S4, OC_C5S3, OC_C6S2, OC_C7S1};

fn idct8(y: &mut [i16], x: &[i16]) {
    let mut t = [0i32; 8];
    let mut r: i32;
    t[0] = (OC_C4S4 * (i32::from(x[0]) + i32::from(x[4]))) >> 16;
    t[1] = (OC_C4S4 * (i32::from(x[0]) - i32::from(x[4]))) >> 16;
    t[2] = ((OC_C6S2 * i32::from(x[2])) >> 16) - ((OC_C2S6 * i32::from(x[6])) >> 16);
    t[3] = ((OC_C2S6 * i32::from(x[2])) >> 16) + ((OC_C6S2 * i32::from(x[6])) >> 16);
    t[4] = ((OC_C7S1 * i32::from(x[1])) >> 16) - ((OC_C1S7 * i32::from(x[7])) >> 16);
    t[5] = ((OC_C3S5 * i32::from(x[5])) >> 16) - ((OC_C5S3 * i32::from(x[3])) >> 16);
    t[6] = ((OC_C5S3 * i32::from(x[5])) >> 16) + ((OC_C3S5 * i32::from(x[3])) >> 16);
    t[7] = ((OC_C1S7 * i32::from(x[1])) >> 16) + ((OC_C7S1 * i32::from(x[7])) >> 16);
    r = t[4] + t[5];
    t[5] = (OC_C4S4 * (t[4] - t[5])) >> 16;
    t[4] = r;
    r = t[7] + t[6];
    t[6] = (OC_C4S4 * (t[7] - t[6])) >> 16;
    t[7] = r;
    r = t[0] + t[3];
    t[3] = t[0] - t[3];
    t[0] = r;
    r = t[1] + t[2];
    t[2] = t[1] - t[2];
    t[1] = r;
    r = t[6] + t[5];
    t[5] = t[6] - t[5];
    t[6] = r;
    y[0] = (t[0] + t[7]) as i16;
    y[8] = (t[1] + t[6]) as i16;
    y[16] = (t[2] + t[5]) as i16;
    y[24] = (t[3] + t[4]) as i16;
    y[32] = (t[3] - t[4]) as i16;
    y[40] = (t[2] - t[5]) as i16;
    y[48] = (t[1] - t[6]) as i16;
    y[56] = (t[0] - t[7]) as i16;
}

fn idct8_4(y: &mut [i16], x: &[i16]) {
    let mut t = [0i32; 8];
    let mut r: i32;
    t[0] = (OC_C4S4 * i32::from(x[0])) >> 16;
    t[2] = (OC_C6S2 * i32::from(x[2])) >> 16;
    t[3] = (OC_C2S6 * i32::from(x[2])) >> 16;
    t[4] = (OC_C7S1 * i32::from(x[1])) >> 16;
    t[5] = -((OC_C5S3 * i32::from(x[3])) >> 16);
    t[6] = (OC_C3S5 * i32::from(x[3])) >> 16;
    t[7] = (OC_C1S7 * i32::from(x[1])) >> 16;
    r = t[4] + t[5];
    t[5] = (OC_C4S4 * (t[4] - t[5])) >> 16;
    t[4] = r;
    r = t[7] + t[6];
    t[6] = (OC_C4S4 * (t[7] - t[6])) >> 16;
    t[7] = r;
    t[1] = t[0] + t[2];
    t[2] = t[0] - t[2];
    r = t[0] + t[3];
    t[3] = t[0] - t[3];
    t[0] = r;
    r = t[6] + t[5];
    t[5] = t[6] - t[5];
    t[6] = r;
    y[0] = (t[0] + t[7]) as i16;
    y[8] = (t[1] + t[6]) as i16;
    y[16] = (t[2] + t[5]) as i16;
    y[24] = (t[3] + t[4]) as i16;
    y[32] = (t[3] - t[4]) as i16;
    y[40] = (t[2] - t[5]) as i16;
    y[48] = (t[1] - t[6]) as i16;
    y[56] = (t[0] - t[7]) as i16;
}

fn idct8_3(y: &mut [i16], x: &[i16]) {
    let mut t = [0i32; 8];
    let mut r: i32;
    t[0] = (OC_C4S4 * i32::from(x[0])) >> 16;
    t[2] = (OC_C6S2 * i32::from(x[2])) >> 16;
    t[3] = (OC_C2S6 * i32::from(x[2])) >> 16;
    t[4] = (OC_C7S1 * i32::from(x[1])) >> 16;
    t[7] = (OC_C1S7 * i32::from(x[1])) >> 16;
    t[5] = (OC_C4S4 * t[4]) >> 16;
    t[6] = (OC_C4S4 * t[7]) >> 16;
    t[1] = t[0] + t[2];
    t[2] = t[0] - t[2];
    r = t[0] + t[3];
    t[3] = t[0] - t[3];
    t[0] = r;
    r = t[6] + t[5];
    t[5] = t[6] - t[5];
    t[6] = r;
    y[0] = (t[0] + t[7]) as i16;
    y[8] = (t[1] + t[6]) as i16;
    y[16] = (t[2] + t[5]) as i16;
    y[24] = (t[3] + t[4]) as i16;
    y[32] = (t[3] - t[4]) as i16;
    y[40] = (t[2] - t[5]) as i16;
    y[48] = (t[1] - t[6]) as i16;
    y[56] = (t[0] - t[7]) as i16;
}

fn idct8_2(y: &mut [i16], x: &[i16]) {
    let mut t = [0i32; 8];
    let mut r: i32;
    t[0] = (OC_C4S4 * i32::from(x[0])) >> 16;
    t[4] = (OC_C7S1 * i32::from(x[1])) >> 16;
    t[7] = (OC_C1S7 * i32::from(x[1])) >> 16;
    t[5] = (OC_C4S4 * t[4]) >> 16;
    t[6] = (OC_C4S4 * t[7]) >> 16;
    r = t[6] + t[5];
    t[5] = t[6] - t[5];
    t[6] = r;
    y[0] = (t[0] + t[7]) as i16;
    y[8] = (t[0] + t[6]) as i16;
    y[16] = (t[0] + t[5]) as i16;
    y[24] = (t[0] + t[4]) as i16;
    y[32] = (t[0] - t[4]) as i16;
    y[40] = (t[0] - t[5]) as i16;
    y[48] = (t[0] - t[6]) as i16;
    y[56] = (t[0] - t[7]) as i16;
}

fn idct8_1(y: &mut [i16], x0: i16) {
    let v = ((OC_C4S4 * i32::from(x0)) >> 16) as i16;
    y[0] = v;
    y[8] = v;
    y[16] = v;
    y[24] = v;
    y[32] = v;
    y[40] = v;
    y[48] = v;
    y[56] = v;
}

fn idct8x8_3(y: &mut [i16; 64], x: &mut [i16; 64]) {
    let mut w = [0i16; 64];
    idct8_2(&mut w[..], &x[..]);
    idct8_1(&mut w[1..], x[8]);
    for i in 0..8 {
        let row = i * 8;
        idct8_2(&mut y[i..], &w[row..]);
    }
    for v in y.iter_mut() {
        *v = ((*v as i32 + 8) >> 4) as i16;
    }
    x[0] = 0;
    x[1] = 0;
    x[8] = 0;
}

fn idct8x8_10(y: &mut [i16; 64], x: &mut [i16; 64]) {
    let mut w = [0i16; 64];
    idct8_4(&mut w[..], &x[..]);
    idct8_3(&mut w[1..], &x[8..]);
    idct8_2(&mut w[2..], &x[16..]);
    idct8_1(&mut w[3..], x[24]);
    for i in 0..8 {
        let row = i * 8;
        idct8_4(&mut y[i..], &w[row..]);
    }
    for v in y.iter_mut() {
        *v = ((*v as i32 + 8) >> 4) as i16;
    }
    x[0] = 0;
    x[1] = 0;
    x[2] = 0;
    x[3] = 0;
    x[8] = 0;
    x[9] = 0;
    x[10] = 0;
    x[16] = 0;
    x[17] = 0;
    x[24] = 0;
}

fn idct8x8_slow(y: &mut [i16; 64], x: &mut [i16; 64]) {
    let mut w = [0i16; 64];
    for i in 0..8 {
        let row = i * 8;
        idct8(&mut w[i..], &x[row..]);
    }
    for i in 0..8 {
        let row = i * 8;
        idct8(&mut y[i..], &w[row..]);
    }
    for v in y.iter_mut() {
        *v = ((*v as i32 + 8) >> 4) as i16;
    }
    x.fill(0);
}

pub fn idct8x8_c(y: &mut [i16; 64], x: &mut [i16; 64], last_zzi: i32) {
    if last_zzi <= 3 {
        idct8x8_3(y, x);
    } else if last_zzi <= 10 {
        idct8x8_10(y, x);
    } else {
        idct8x8_slow(y, x);
    }
}

#[cfg(test)]
mod tests {
    use super::idct8x8_c;

    #[test]
    fn zero_block_stays_zero() {
        let mut y = [0i16; 64];
        let mut x = [0i16; 64];
        idct8x8_c(&mut y, &mut x, 0);
        assert!(y.iter().all(|v| *v == 0));
        assert!(x.iter().all(|v| *v == 0));
    }

    #[test]
    fn dc_only_block_is_uniform() {
        let mut y = [0i16; 64];
        let mut x = [0i16; 64];
        x[0] = 16;
        idct8x8_c(&mut y, &mut x, 0);
        let first = y[0];
        assert!(y.iter().all(|v| *v == first));
        assert_eq!(x[0], 0);
    }
}
