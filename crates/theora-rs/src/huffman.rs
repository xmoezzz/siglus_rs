use crate::codec::TH_NDCT_TOKENS;

pub const OC_DCT_VAL_RANGE: i32 = 580;
pub const OC_NDCT_TOKEN_BITS: i32 = 5;

pub const OC_DCT_EOB1_TOKEN: usize = 0;
pub const OC_DCT_EOB2_TOKEN: usize = 1;
pub const OC_DCT_EOB3_TOKEN: usize = 2;
pub const OC_DCT_REPEAT_RUN0_TOKEN: usize = 3;
pub const OC_DCT_REPEAT_RUN1_TOKEN: usize = 4;
pub const OC_DCT_REPEAT_RUN2_TOKEN: usize = 5;
pub const OC_DCT_REPEAT_RUN3_TOKEN: usize = 6;
pub const OC_DCT_SHORT_ZRL_TOKEN: usize = 7;
pub const OC_DCT_ZRL_TOKEN: usize = 8;
pub const OC_ONE_TOKEN: usize = 9;
pub const OC_MINUS_ONE_TOKEN: usize = 10;
pub const OC_TWO_TOKEN: usize = 11;
pub const OC_MINUS_TWO_TOKEN: usize = 12;
pub const OC_DCT_VAL_CAT2: usize = 13;
pub const OC_DCT_VAL_CAT3: usize = 17;
pub const OC_DCT_VAL_CAT4: usize = 18;
pub const OC_DCT_VAL_CAT5: usize = 19;
pub const OC_DCT_VAL_CAT6: usize = 20;
pub const OC_DCT_VAL_CAT7: usize = 21;
pub const OC_DCT_VAL_CAT8: usize = 22;
pub const OC_DCT_RUN_CAT1A: usize = 23;
pub const OC_DCT_RUN_CAT1B: usize = 28;
pub const OC_DCT_RUN_CAT1C: usize = 29;
pub const OC_DCT_RUN_CAT2A: usize = 30;
pub const OC_DCT_RUN_CAT2B: usize = 31;

pub const OC_NDCT_EOB_TOKEN_MAX: usize = 7;
pub const OC_NDCT_ZRL_TOKEN_MAX: usize = 9;
pub const OC_NDCT_VAL_MAX: usize = 23;
pub const OC_NDCT_VAL_CAT1_MAX: usize = 13;
pub const OC_NDCT_VAL_CAT2_MAX: usize = 17;
pub const OC_NDCT_VAL_CAT2_SIZE: usize = OC_NDCT_VAL_CAT2_MAX - OC_DCT_VAL_CAT2;
pub const OC_NDCT_RUN_MAX: usize = 32;
pub const OC_NDCT_RUN_CAT1A_MAX: usize = 28;

pub const OC_DCT_TOKEN_EXTRA_BITS: [u8; TH_NDCT_TOKENS] = [
    0, 0, 0, 2, 3, 4, 12, 3, 6, 0, 0, 0, 0, 1, 1, 1, 1, 2, 3, 4, 5, 6, 10, 1, 1, 1, 1, 1, 3, 4, 2,
    3,
];
