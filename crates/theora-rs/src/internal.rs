use crate::codec::{ImgPlane, YCbCrBuffer, TH_NDCT_TOKENS, TH_PF_NFORMATS};

pub const OC_FZIG_ZAG: [u8; 128] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20,
    13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22, 15, 23, 30, 37, 44, 51, 58, 59,
    52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64,
    64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64,
    64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64, 64,
    64, 64, 64, 64, 64, 64,
];

pub const OC_IZIG_ZAG: [u8; 64] = [
    0, 1, 5, 6, 14, 15, 27, 28, 2, 4, 7, 13, 16, 26, 29, 42, 3, 8, 12, 17, 25, 30, 41, 43, 9, 11,
    18, 24, 31, 40, 44, 53, 10, 19, 23, 32, 39, 45, 52, 54, 20, 22, 33, 38, 46, 51, 55, 60, 21, 34,
    37, 47, 50, 56, 59, 61, 35, 36, 48, 49, 57, 58, 62, 63,
];

pub const OC_MB_MAP: [[u8; 2]; 2] = [[0, 3], [1, 2]];

pub const OC_MB_MAP_IDXS: [[u8; 12]; TH_PF_NFORMATS] = [
    [0, 1, 2, 3, 4, 8, 0, 0, 0, 0, 0, 0],
    [0, 1, 2, 3, 4, 5, 8, 9, 0, 0, 0, 0],
    [0, 1, 2, 3, 4, 6, 8, 10, 0, 0, 0, 0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11],
];

pub const OC_MB_MAP_NIDXS: [u8; TH_PF_NFORMATS] = [6, 8, 8, 12];

pub const OC_DCT_TOKEN_EXTRA_BITS: [u8; TH_NDCT_TOKENS] = [
    0, 0, 0, 2, 3, 4, 12, 3, 6, 0, 0, 0, 0, 1, 1, 1, 1, 2, 3, 4, 5, 6, 10, 1, 1, 1, 1, 1, 3, 4, 2,
    3,
];

pub fn oc_malloc_2d<T: Clone + Default>(height: usize, width: usize) -> Vec<Vec<T>> {
    vec![vec![T::default(); width]; height]
}

pub fn oc_calloc_2d<T: Clone + Default>(height: usize, width: usize) -> Vec<Vec<T>> {
    oc_malloc_2d(height, width)
}

pub fn oc_ycbcr_buffer_flip(dst: &mut YCbCrBuffer, src: &YCbCrBuffer) {
    for pli in 0..3 {
        let height = src[pli].height;
        let stride = -src[pli].stride;
        let offset = src[pli].data_offset as isize + ((1 - height) as isize) * (stride as isize);
        assert!(
            offset >= 0,
            "oc_ycbcr_buffer_flip produced a negative data offset"
        );
        dst[pli] = ImgPlane {
            width: src[pli].width,
            height,
            stride,
            data: src[pli].data.clone(),
            data_offset: offset as usize,
        };
    }
}
