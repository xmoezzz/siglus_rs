use crate::huffman::{OC_DCT_RUN_CAT1A, OC_DCT_RUN_CAT1B};

pub fn token_skip_eob(token: usize, extra_bits: i32) -> isize {
    let adjust = match token {
        0 => 1,
        1 => 2,
        2 => 3,
        3 => 4,
        4 => 8,
        5 => 16,
        _ => 1,
    };
    -(extra_bits + adjust) as isize
}

pub fn token_skip_zrl(_token: usize, extra_bits: i32) -> isize {
    (extra_bits + 1) as isize
}

pub fn token_skip_val() -> isize {
    1
}

pub fn token_skip_run_cat1a(token: usize) -> isize {
    (token as isize - OC_DCT_RUN_CAT1A as isize + 2) as isize
}

pub fn token_skip_run(token: usize, extra_bits: i32) -> isize {
    let run_cati = token as i32 - OC_DCT_RUN_CAT1B as i32;
    let (mask, adjust) = match run_cati {
        0 => (3, 7),
        1 => (7, 11),
        2 => (0, 2),
        _ => (1, 3),
    };
    ((extra_bits & mask) + adjust) as isize
}

pub fn oc_dct_token_skip(token: usize, extra_bits: i32) -> isize {
    match token {
        0..=5 => token_skip_eob(token, extra_bits),
        6 => {
            if extra_bits == 0 {
                -((isize::MAX) as isize)
            } else {
                -(extra_bits as isize)
            }
        }
        7 | 8 => token_skip_zrl(token, extra_bits),
        9..=22 => token_skip_val(),
        23..=27 => token_skip_run_cat1a(token),
        28..=31 => token_skip_run(token, extra_bits),
        _ => 1,
    }
}
