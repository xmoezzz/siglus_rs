use crate::codec::{Info, YCbCrBuffer};
use crate::internal::OC_MB_MAP;
use crate::quant::DequantTables;

pub type OcSbMapQuad = [isize; 4];
pub type OcSbMap = [OcSbMapQuad; 4];
pub type OcMbMapPlane = [isize; 4];
pub type OcMbMap = [OcMbMapPlane; 3];
pub type OcMv = i16;

pub const OC_INTRA_FRAME: i8 = 0;
pub const OC_INTER_FRAME: i8 = 1;
pub const OC_UNKWN_FRAME: i8 = -1;

pub const OC_UMV_PADDING: i32 = 16;

pub const OC_FRAME_GOLD: i32 = 0;
pub const OC_FRAME_PREV: i32 = 1;
pub const OC_FRAME_SELF: i32 = 2;
pub const OC_FRAME_NONE: i32 = 3;
pub const OC_FRAME_IO: i32 = 3;
pub const OC_FRAME_GOLD_ORIG: i32 = 4;
pub const OC_FRAME_PREV_ORIG: i32 = 5;

pub const OC_MODE_INVALID: i8 = -1;
pub const OC_MODE_INTER_NOMV: i8 = 0;
pub const OC_MODE_INTRA: i8 = 1;
pub const OC_MODE_INTER_MV: i8 = 2;
pub const OC_MODE_INTER_MV_LAST: i8 = 3;
pub const OC_MODE_INTER_MV_LAST2: i8 = 4;
pub const OC_MODE_GOLDEN_NOMV: i8 = 5;
pub const OC_MODE_GOLDEN_MV: i8 = 6;
pub const OC_MODE_INTER_MV_FOUR: i8 = 7;
pub const OC_NMODES: usize = 8;

pub const OC_PACKET_INFO_HDR: i32 = -3;
pub const OC_PACKET_COMMENT_HDR: i32 = -2;
pub const OC_PACKET_SETUP_HDR: i32 = -1;
pub const OC_PACKET_DONE: i32 = i32::MAX;

#[inline]
pub const fn oc_mv(x: i32, y: i32) -> OcMv {
    ((x & 0xFF) | (y << 8)) as OcMv
}

#[inline]
pub const fn oc_mv_x(mv: OcMv) -> i32 {
    (mv as i8) as i32
}

#[inline]
pub const fn oc_mv_y(mv: OcMv) -> i32 {
    (mv as i32) >> 8
}

#[inline]
pub const fn oc_mv_add(a: OcMv, b: OcMv) -> OcMv {
    oc_mv(oc_mv_x(a) + oc_mv_x(b), oc_mv_y(a) + oc_mv_y(b))
}

#[inline]
pub const fn oc_mv_sub(a: OcMv, b: OcMv) -> OcMv {
    oc_mv(oc_mv_x(a) - oc_mv_x(b), oc_mv_y(a) - oc_mv_y(b))
}

#[inline]
pub const fn frame_for_mode(mode: i8) -> i32 {
    match mode {
        OC_MODE_INTER_NOMV
        | OC_MODE_INTER_MV
        | OC_MODE_INTER_MV_LAST
        | OC_MODE_INTER_MV_LAST2
        | OC_MODE_INTER_MV_FOUR => OC_FRAME_PREV,
        OC_MODE_INTRA => OC_FRAME_SELF,
        OC_MODE_GOLDEN_NOMV | OC_MODE_GOLDEN_MV => OC_FRAME_GOLD,
        _ => OC_FRAME_PREV,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SbFlags {
    pub coded_fully: bool,
    pub coded_partially: bool,
    pub quad_valid: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BorderInfo {
    pub mask: i64,
    pub npixels: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Fragment {
    pub coded: bool,
    pub invalid: bool,
    pub qii: u8,
    pub refi: u8,
    pub mb_mode: i8,
    pub borderi: i8,
    pub dc: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FragmentPlane {
    pub nhfrags: i32,
    pub nvfrags: i32,
    pub froffset: isize,
    pub nfrags: isize,
    pub nhsbs: u32,
    pub nvsbs: u32,
    pub sboffset: u32,
    pub nsbs: u32,
}

#[derive(Debug, Clone)]
pub struct TheoraState {
    pub info: Info,
    pub fplanes: [FragmentPlane; 3],
    pub frags: Vec<Fragment>,
    pub frag_buf_offs: Vec<isize>,
    pub frag_mvs: Vec<OcMv>,
    pub nfrags: isize,
    pub sb_maps: Vec<OcSbMap>,
    pub sb_flags: Vec<SbFlags>,
    pub nsbs: u32,
    pub mb_maps: Vec<OcMbMap>,
    pub mb_modes: Vec<i8>,
    pub nhmbs: u32,
    pub nvmbs: u32,
    pub nmbs: usize,
    pub coded_fragis: Vec<isize>,
    pub uncoded_fragis: Vec<isize>,
    pub ncoded_fragis: [isize; 3],
    pub ntotal_coded_fragis: isize,
    pub ref_frame_bufs: [YCbCrBuffer; 6],
    pub ref_frame_idx: [i32; 6],
    pub ref_ystride: [i32; 3],
    pub nborders: i32,
    pub borders: [BorderInfo; 16],
    pub keyframe_num: i64,
    pub curframe_num: i64,
    pub granpos: i64,
    pub frame_type: i8,
    pub granpos_bias: u8,
    pub nqis: u8,
    pub qis: [u8; 3],
    pub dequant_tables: DequantTables,
    pub loop_filter_limits: [u8; 64],
}

impl Default for TheoraState {
    fn default() -> Self {
        Self {
            info: Default::default(),
            fplanes: [Default::default(), Default::default(), Default::default()],
            frags: Vec::new(),
            frag_buf_offs: Vec::new(),
            frag_mvs: Vec::new(),
            nfrags: 0,
            sb_maps: Vec::new(),
            sb_flags: Vec::new(),
            nsbs: 0,
            mb_maps: Vec::new(),
            mb_modes: Vec::new(),
            nhmbs: 0,
            nvmbs: 0,
            nmbs: 0,
            coded_fragis: Vec::new(),
            uncoded_fragis: Vec::new(),
            ncoded_fragis: [0; 3],
            ntotal_coded_fragis: 0,
            ref_frame_bufs: [
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
                Default::default(),
            ],
            ref_frame_idx: [0; 6],
            ref_ystride: [0; 3],
            nborders: 0,
            borders: [Default::default(); 16],
            keyframe_num: 0,
            curframe_num: 0,
            granpos: 0,
            frame_type: 0,
            granpos_bias: 0,
            nqis: 0,
            qis: [0; 3],
            dequant_tables: [[[[0; 64]; 2]; 3]; 64],
            loop_filter_limits: [0; 64],
        }
    }
}

impl TheoraState {
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn has_frames(&self) -> bool {
        self.nfrags > 0 && self.fplanes.iter().any(|p| p.nfrags > 0)
    }
}

const SB_MAP: [[[usize; 2]; 4]; 4] = [
    [[0, 0], [0, 1], [3, 2], [3, 3]],
    [[0, 3], [0, 2], [3, 1], [3, 0]],
    [[1, 0], [1, 3], [2, 0], [2, 3]],
    [[1, 1], [1, 2], [2, 1], [2, 2]],
];

const OC_MVMAP: [[i8; 64]; 2] = [
    [
        -15, -15, -14, -14, -13, -13, -12, -12, -11, -11, -10, -10, -9, -9, -8, -8, -7, -7, -6, -6,
        -5, -5, -4, -4, -3, -3, -2, -2, -1, -1, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7,
        8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13, 14, 14, 15, 15, 0,
    ],
    [
        -7, -7, -7, -7, -6, -6, -6, -6, -5, -5, -5, -5, -4, -4, -4, -4, -3, -3, -3, -3, -2, -2, -2,
        -2, -1, -1, -1, -1, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5,
        5, 5, 5, 6, 6, 6, 6, 7, 7, 7, 7, 0,
    ],
];

const OC_MVMAP2: [[i8; 64]; 2] = [
    [
        -1, 0, -1, 0, -1, 0, -1, 0, -1, 0, -1, 0, -1, 0, -1, 0, -1, 0, -1, 0, -1, 0, -1, 0, -1, 0,
        -1, 0, -1, 0, -1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1,
        0, 1, 0, 1, 0, 1, 0, 1, 0,
    ],
    [
        -1, -1, -1, 0, -1, -1, -1, 0, -1, -1, -1, 0, -1, -1, -1, 0, -1, -1, -1, 0, -1, -1, -1, 0,
        -1, -1, -1, 0, -1, -1, -1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0,
        1, 1, 1, 0, 1, 1, 1, 0, 1, 1, 1, 0,
    ],
];

fn signmask(x: i32) -> i32 {
    x >> 31
}

fn div_round_pow2(dividend: i32, shift: i32, rval: i32) -> i32 {
    (dividend + signmask(dividend) + rval) >> shift
}

fn sb_quad_top_left_frag(sb_map: &OcSbMap, quadi: usize) -> isize {
    sb_map[quadi][quadi & ((quadi << 1) & 0x3)]
}

fn plane_pixel_index(plane: &crate::codec::ImgPlane, x: i32, y: i32) -> usize {
    (plane.data_offset as isize + y as isize * plane.stride as isize + x as isize) as usize
}

pub fn oc_state_set_chroma_mvs(cbmvs: &mut [OcMv; 4], lbmvs: &[OcMv; 4], pixel_fmt: i32) {
    match pixel_fmt {
        0 => {
            let dx = oc_mv_x(lbmvs[0]) + oc_mv_x(lbmvs[1]) + oc_mv_x(lbmvs[2]) + oc_mv_x(lbmvs[3]);
            let dy = oc_mv_y(lbmvs[0]) + oc_mv_y(lbmvs[1]) + oc_mv_y(lbmvs[2]) + oc_mv_y(lbmvs[3]);
            cbmvs[0] = oc_mv(div_round_pow2(dx, 2, 2), div_round_pow2(dy, 2, 2));
        }
        1 => {
            let dx0 = oc_mv_x(lbmvs[0]) + oc_mv_x(lbmvs[2]);
            let dy0 = oc_mv_y(lbmvs[0]) + oc_mv_y(lbmvs[2]);
            cbmvs[0] = oc_mv(div_round_pow2(dx0, 1, 1), div_round_pow2(dy0, 1, 1));
            let dx1 = oc_mv_x(lbmvs[1]) + oc_mv_x(lbmvs[3]);
            let dy1 = oc_mv_y(lbmvs[1]) + oc_mv_y(lbmvs[3]);
            cbmvs[1] = oc_mv(div_round_pow2(dx1, 1, 1), div_round_pow2(dy1, 1, 1));
        }
        2 => {
            let dx0 = oc_mv_x(lbmvs[0]) + oc_mv_x(lbmvs[1]);
            let dy0 = oc_mv_y(lbmvs[0]) + oc_mv_y(lbmvs[1]);
            cbmvs[0] = oc_mv(div_round_pow2(dx0, 1, 1), div_round_pow2(dy0, 1, 1));
            let dx2 = oc_mv_x(lbmvs[2]) + oc_mv_x(lbmvs[3]);
            let dy2 = oc_mv_y(lbmvs[2]) + oc_mv_y(lbmvs[3]);
            cbmvs[2] = oc_mv(div_round_pow2(dx2, 1, 1), div_round_pow2(dy2, 1, 1));
        }
        _ => {
            *cbmvs = *lbmvs;
        }
    }
}

fn oc_sb_create_plane_mapping(
    sb_maps: &mut [OcSbMap],
    sb_flags: &mut [SbFlags],
    frag0: isize,
    hfrags: i32,
    vfrags: i32,
) {
    let mut sbi = 0usize;
    let mut yfrag = frag0;
    let mut y = 0i32;
    loop {
        let imax = vfrags - y;
        let imax = if imax > 4 {
            4
        } else if imax <= 0 {
            break;
        } else {
            imax
        };
        let mut x = 0i32;
        loop {
            let jmax = hfrags - x;
            let jmax = if jmax > 4 {
                4
            } else if jmax <= 0 {
                break;
            } else {
                jmax
            };
            sb_maps[sbi] = [[-1isize; 4]; 4];
            sb_flags[sbi].quad_valid = 0;
            let mut xfrag = yfrag + x as isize;
            for i in 0..imax as usize {
                for j in 0..jmax as usize {
                    let [mbi, bi] = SB_MAP[i][j];
                    sb_maps[sbi][mbi][bi] = xfrag + j as isize;
                }
                xfrag += hfrags as isize;
            }
            for quadi in 0..4usize {
                sb_flags[sbi].quad_valid |=
                    ((sb_quad_top_left_frag(&sb_maps[sbi], quadi) >= 0) as u8) << quadi;
            }
            x += 4;
            sbi += 1;
        }
        y += 4;
        yfrag += (hfrags << 2) as isize;
    }
}

fn oc_mb_fill_ymapping(mb_map: &mut OcMbMap, fplane: &FragmentPlane, xfrag0: i32, yfrag0: i32) {
    for i in 0..2 {
        for j in 0..2 {
            mb_map[0][i << 1 | j] = ((yfrag0 + i as i32) as isize) * fplane.nhfrags as isize
                + xfrag0 as isize
                + j as isize;
        }
    }
}

fn oc_mb_fill_cmapping00(
    mb_map: &mut OcMbMap,
    fplanes: &[FragmentPlane; 3],
    mut xfrag0: i32,
    mut yfrag0: i32,
) {
    xfrag0 >>= 1;
    yfrag0 >>= 1;
    let fragi = (yfrag0 as isize) * fplanes[1].nhfrags as isize + xfrag0 as isize;
    mb_map[1][0] = fragi + fplanes[1].froffset;
    mb_map[2][0] = fragi + fplanes[2].froffset;
}

fn oc_mb_fill_cmapping01(
    mb_map: &mut OcMbMap,
    fplanes: &[FragmentPlane; 3],
    xfrag0: i32,
    mut yfrag0: i32,
) {
    yfrag0 >>= 1;
    let mut fragi = (yfrag0 as isize) * fplanes[1].nhfrags as isize + xfrag0 as isize;
    for j in 0..2usize {
        mb_map[1][j] = fragi + fplanes[1].froffset;
        mb_map[2][j] = fragi + fplanes[2].froffset;
        fragi += 1;
    }
}

fn oc_mb_fill_cmapping10(
    mb_map: &mut OcMbMap,
    fplanes: &[FragmentPlane; 3],
    mut xfrag0: i32,
    yfrag0: i32,
) {
    xfrag0 >>= 1;
    let mut fragi = (yfrag0 as isize) * fplanes[1].nhfrags as isize + xfrag0 as isize;
    for i in 0..2usize {
        mb_map[1][i << 1] = fragi + fplanes[1].froffset;
        mb_map[2][i << 1] = fragi + fplanes[2].froffset;
        fragi += fplanes[1].nhfrags as isize;
    }
}

fn oc_mb_fill_cmapping11(
    mb_map: &mut OcMbMap,
    fplanes: &[FragmentPlane; 3],
    _xfrag0: i32,
    _yfrag0: i32,
) {
    for k in 0..4usize {
        mb_map[1][k] = mb_map[0][k] + fplanes[1].froffset;
        mb_map[2][k] = mb_map[0][k] + fplanes[2].froffset;
    }
}

fn oc_mb_fill_cmapping(
    mb_map: &mut OcMbMap,
    fplanes: &[FragmentPlane; 3],
    pixel_fmt: i32,
    xfrag0: i32,
    yfrag0: i32,
) {
    match pixel_fmt {
        0 => oc_mb_fill_cmapping00(mb_map, fplanes, xfrag0, yfrag0),
        1 => oc_mb_fill_cmapping01(mb_map, fplanes, xfrag0, yfrag0),
        2 => oc_mb_fill_cmapping10(mb_map, fplanes, xfrag0, yfrag0),
        _ => oc_mb_fill_cmapping11(mb_map, fplanes, xfrag0, yfrag0),
    }
}

fn oc_mb_create_mapping(
    mb_maps: &mut [OcMbMap],
    mb_modes: &mut [i8],
    fplanes: &[FragmentPlane; 3],
    pixel_fmt: i32,
) {
    let mut sbi = 0usize;
    let mut y = 0i32;
    while y < fplanes[0].nvfrags {
        let mut x = 0i32;
        while x < fplanes[0].nhfrags {
            for ymb in 0..2usize {
                for xmb in 0..2usize {
                    let mbi = (sbi << 2) | (OC_MB_MAP[ymb][xmb] as usize);
                    let mbx = x | ((xmb as i32) << 1);
                    let mby = y | ((ymb as i32) << 1);
                    mb_maps[mbi] = [[-1isize; 4]; 3];
                    if mbx >= fplanes[0].nhfrags || mby >= fplanes[0].nvfrags {
                        mb_modes[mbi] = OC_MODE_INVALID;
                        continue;
                    }
                    oc_mb_fill_ymapping(&mut mb_maps[mbi], &fplanes[0], mbx, mby);
                    oc_mb_fill_cmapping(&mut mb_maps[mbi], fplanes, pixel_fmt, mbx, mby);
                }
            }
            x += 4;
            sbi += 1;
        }
        y += 4;
    }
}

pub fn oc_state_border_init(state: &mut TheoraState) {
    state.nborders = 0;
    let mut fragi = 0usize;
    for pli in 0..3usize {
        let fplane = state.fplanes[pli];
        let mut crop_x0 = state.info.pic_x as i32;
        let mut crop_xf = (state.info.pic_x + state.info.pic_width) as i32;
        let mut crop_y0 = state.info.pic_y as i32;
        let mut crop_yf = (state.info.pic_y + state.info.pic_height) as i32;
        if pli > 0 {
            if (state.info.pixel_fmt as i32 & 1) == 0 {
                crop_x0 >>= 1;
                crop_xf = (crop_xf + 1) >> 1;
            }
            if (state.info.pixel_fmt as i32 & 2) == 0 {
                crop_y0 >>= 1;
                crop_yf = (crop_yf + 1) >> 1;
            }
        }
        let start = fragi;
        let end = start + fplane.nfrags as usize;
        let mut y = 0;
        while fragi < end {
            let row_end = fragi + fplane.nhfrags as usize;
            let mut x = 0;
            while fragi < row_end {
                let frag = &mut state.frags[fragi];
                if x + 8 <= crop_x0
                    || crop_xf <= x
                    || y + 8 <= crop_y0
                    || crop_yf <= y
                    || crop_x0 >= crop_xf
                    || crop_y0 >= crop_yf
                {
                    frag.invalid = true;
                } else if (x < crop_x0 && crop_x0 < x + 8)
                    || (x < crop_xf && crop_xf < x + 8)
                    || (y < crop_y0 && crop_y0 < y + 8)
                    || (y < crop_yf && crop_yf < y + 8)
                {
                    let mut mask: i64 = 0;
                    let mut npixels = 0;
                    for i in 0..8 {
                        for j in 0..8 {
                            if x + j >= crop_x0
                                && x + j < crop_xf
                                && y + i >= crop_y0
                                && y + i < crop_yf
                            {
                                mask |= 1i64 << ((i << 3) | j);
                                npixels += 1;
                            }
                        }
                    }
                    let mut found = None;
                    for i in 0..state.nborders as usize {
                        if state.borders[i].mask == mask {
                            found = Some(i);
                            break;
                        }
                    }
                    let idx = found.unwrap_or_else(|| {
                        let i = state.nborders as usize;
                        state.nborders += 1;
                        state.borders[i] = BorderInfo { mask, npixels };
                        i
                    });
                    frag.borderi = idx as i8;
                } else {
                    frag.borderi = -1;
                }
                fragi += 1;
                x += 8;
            }
            y += 8;
        }
    }
}

pub fn oc_state_frarray_init(state: &mut TheoraState) -> crate::error::Result<()> {
    let yhfrags = (state.info.frame_width >> 3) as i32;
    let yvfrags = (state.info.frame_height >> 3) as i32;
    let hdec = if (state.info.pixel_fmt as i32 & 1) == 0 {
        1
    } else {
        0
    };
    let vdec = if (state.info.pixel_fmt as i32 & 2) == 0 {
        1
    } else {
        0
    };
    let chfrags = ((yhfrags + hdec) >> hdec) as i32;
    let cvfrags = ((yvfrags + vdec) >> vdec) as i32;
    let yfrags = yhfrags as isize * yvfrags as isize;
    let cfrags = chfrags as isize * cvfrags as isize;
    let nfrags = yfrags + 2 * cfrags;
    let yhsbs = ((yhfrags + 3) >> 2) as u32;
    let yvsbs = ((yvfrags + 3) >> 2) as u32;
    let chsbs = ((chfrags + 3) >> 2) as u32;
    let cvsbs = ((cvfrags + 3) >> 2) as u32;
    let ysbs = yhsbs * yvsbs;
    let csbs = chsbs * cvsbs;
    let nsbs = ysbs + 2 * csbs;
    let nmbs = (ysbs as usize) << 2;
    if yhfrags <= 0 || yvfrags <= 0 || nfrags <= 0 {
        return Err(crate::error::TheoraError::InvalidArgument);
    }
    state.fplanes[0] = FragmentPlane {
        nhfrags: yhfrags,
        nvfrags: yvfrags,
        froffset: 0,
        nfrags: yfrags,
        nhsbs: yhsbs,
        nvsbs: yvsbs,
        sboffset: 0,
        nsbs: ysbs,
    };
    state.fplanes[1] = FragmentPlane {
        nhfrags: chfrags,
        nvfrags: cvfrags,
        froffset: yfrags,
        nfrags: cfrags,
        nhsbs: chsbs,
        nvsbs: cvsbs,
        sboffset: ysbs,
        nsbs: csbs,
    };
    state.fplanes[2] = FragmentPlane {
        nhfrags: chfrags,
        nvfrags: cvfrags,
        froffset: yfrags + cfrags,
        nfrags: cfrags,
        nhsbs: chsbs,
        nvsbs: cvsbs,
        sboffset: ysbs + csbs,
        nsbs: csbs,
    };
    state.nfrags = nfrags;
    state.frags = vec![Fragment::default(); nfrags as usize];
    state.frag_mvs = vec![0; nfrags as usize];
    state.sb_maps = vec![[[-1isize; 4]; 4]; nsbs as usize];
    state.sb_flags = vec![SbFlags::default(); nsbs as usize];
    state.nsbs = nsbs;
    state.nhmbs = yhsbs << 1;
    state.nvmbs = yvsbs << 1;
    state.nmbs = nmbs;
    state.mb_maps = vec![[[-1isize; 4]; 3]; nmbs];
    state.mb_modes = vec![0; nmbs];
    state.coded_fragis = vec![0; nfrags as usize];
    state.ncoded_fragis = [0; 3];
    state.ntotal_coded_fragis = 0;
    for pli in 0..3usize {
        let fplane = state.fplanes[pli];
        let sboff = fplane.sboffset as usize;
        let nsbs_plane = fplane.nsbs as usize;
        oc_sb_create_plane_mapping(
            &mut state.sb_maps[sboff..sboff + nsbs_plane],
            &mut state.sb_flags[sboff..sboff + nsbs_plane],
            fplane.froffset,
            fplane.nhfrags,
            fplane.nvfrags,
        );
    }
    oc_mb_create_mapping(
        &mut state.mb_maps,
        &mut state.mb_modes,
        &state.fplanes,
        state.info.pixel_fmt as i32,
    );
    oc_state_border_init(state);
    Ok(())
}

pub fn oc_state_ref_bufs_init(state: &mut TheoraState, nrefs: usize) -> crate::error::Result<()> {
    if !(3..=6).contains(&nrefs) {
        return Err(crate::error::TheoraError::InvalidArgument);
    }
    let hdec = if (state.info.pixel_fmt as i32 & 1) == 0 {
        1
    } else {
        0
    };
    let vdec = if (state.info.pixel_fmt as i32 & 2) == 0 {
        1
    } else {
        0
    };
    let yhstride = state.info.frame_width as i32 + 2 * OC_UMV_PADDING;
    let yheight = state.info.frame_height as i32 + 2 * OC_UMV_PADDING;
    let chstride = ((yhstride >> hdec) + 15) & !15;
    let cheight = yheight >> vdec;
    let yoffset = (OC_UMV_PADDING + OC_UMV_PADDING * yhstride) as usize;
    let coffset = ((OC_UMV_PADDING >> hdec) + (OC_UMV_PADDING >> vdec) * chstride) as usize;
    let yplane_sz = (yhstride * yheight) as usize;
    let cplane_sz = (chstride * cheight) as usize;

    for rfi in 0..nrefs {
        state.ref_frame_bufs[rfi][0].width = state.info.frame_width as i32;
        state.ref_frame_bufs[rfi][0].height = state.info.frame_height as i32;
        state.ref_frame_bufs[rfi][0].stride = yhstride;
        state.ref_frame_bufs[rfi][0].data = vec![0; yplane_sz + 16];
        state.ref_frame_bufs[rfi][0].data_offset = yoffset;
        for pli in 1..3usize {
            state.ref_frame_bufs[rfi][pli].width = (state.info.frame_width as i32) >> hdec;
            state.ref_frame_bufs[rfi][pli].height = (state.info.frame_height as i32) >> vdec;
            state.ref_frame_bufs[rfi][pli].stride = chstride;
            state.ref_frame_bufs[rfi][pli].data = vec![0; cplane_sz + 16];
            state.ref_frame_bufs[rfi][pli].data_offset = coffset;
        }
        let src = state.ref_frame_bufs[rfi].clone();
        crate::internal::oc_ycbcr_buffer_flip(&mut state.ref_frame_bufs[rfi], &src);
    }

    state.ref_ystride[0] = -yhstride;
    state.ref_ystride[1] = -chstride;
    state.ref_ystride[2] = -chstride;
    state.frag_buf_offs = vec![0; state.nfrags as usize];
    let mut fragi = 0usize;
    for pli in 0..3usize {
        let iplane = &state.ref_frame_bufs[0][pli];
        let fplane = state.fplanes[pli];
        let mut vpix = iplane.data_offset as isize;
        let vfragi_end = (fplane.froffset + fplane.nfrags) as usize;
        while fragi < vfragi_end {
            let row_end = fragi + fplane.nhfrags as usize;
            let mut hpix = vpix;
            while fragi < row_end {
                state.frag_buf_offs[fragi] = hpix - iplane.data_offset as isize;
                hpix += 8;
                fragi += 1;
            }
            vpix += 8 * iplane.stride as isize;
        }
    }
    state.ref_frame_idx[OC_FRAME_GOLD as usize] = -1;
    state.ref_frame_idx[OC_FRAME_PREV as usize] = -1;
    state.ref_frame_idx[OC_FRAME_GOLD_ORIG as usize] = -1;
    state.ref_frame_idx[OC_FRAME_PREV_ORIG as usize] = -1;
    state.ref_frame_idx[OC_FRAME_SELF as usize] = -1;
    state.ref_frame_idx[OC_FRAME_IO as usize] = -1;
    Ok(())
}

pub fn oc_state_init(
    state: &mut TheoraState,
    info: &Info,
    nrefs: usize,
) -> crate::error::Result<()> {
    if (info.frame_width & 0xF) != 0
        || (info.frame_height & 0xF) != 0
        || info.frame_width == 0
        || info.frame_height == 0
        || info.frame_width >= 0x100000
        || info.frame_height >= 0x100000
        || info.pic_x + info.pic_width > info.frame_width
        || info.pic_y + info.pic_height > info.frame_height
        || info.pic_x > 255
        || info.frame_height - info.pic_height - info.pic_y > 255
        || info.fps_numerator < 1
        || info.fps_denominator < 1
    {
        return Err(crate::error::TheoraError::InvalidArgument);
    }
    state.clear();
    state.info = info.clone();
    state.info.pic_y = info.frame_height - info.pic_height - info.pic_y;
    state.frame_type = -1;
    oc_state_frarray_init(state)?;
    oc_state_ref_bufs_init(state, nrefs)?;
    if state.info.keyframe_granule_shift < 0 || state.info.keyframe_granule_shift > 31 {
        state.info.keyframe_granule_shift = 31;
    }
    state.keyframe_num = 0;
    state.curframe_num = -1;
    state.granpos_bias = ((
        info.version_major as i32,
        info.version_minor as i32,
        info.version_subminor as i32,
    ) >= (3, 2, 1)) as u8;
    Ok(())
}

pub fn oc_state_clear(state: &mut TheoraState) {
    state.clear();
}

pub fn oc_state_borders_fill_rows(
    state: &mut TheoraState,
    refi: usize,
    pli: usize,
    y0: i32,
    yend: i32,
) {
    let hpadding = OC_UMV_PADDING
        >> if pli != 0 && (state.info.pixel_fmt as i32 & 1) == 0 {
            1
        } else {
            0
        };
    let plane = &mut state.ref_frame_bufs[refi][pli];
    for y in y0..yend {
        let left = plane_pixel_index(plane, 0, y);
        let right = plane_pixel_index(plane, plane.width - 1, y);
        let lval = plane.data[left];
        let rval = plane.data[right];
        for i in 1..=hpadding {
            let dstl = plane_pixel_index(plane, -i, y);
            let dstr = plane_pixel_index(plane, plane.width - 1 + i, y);
            plane.data[dstl] = lval;
            plane.data[dstr] = rval;
        }
    }
}

pub fn oc_state_borders_fill_caps(state: &mut TheoraState, refi: usize, pli: usize) {
    let hpadding = OC_UMV_PADDING
        >> if pli != 0 && (state.info.pixel_fmt as i32 & 1) == 0 {
            1
        } else {
            0
        };
    let vpadding = OC_UMV_PADDING
        >> if pli != 0 && (state.info.pixel_fmt as i32 & 2) == 0 {
            1
        } else {
            0
        };
    let plane = &mut state.ref_frame_bufs[refi][pli];
    let fullw = plane.width + (hpadding << 1);
    for i in 1..=vpadding {
        for x in -hpadding..(-hpadding + fullw) {
            let top_src = plane_pixel_index(plane, x, 0);
            let bot_src = plane_pixel_index(plane, x, plane.height - 1);
            let top_dst = plane_pixel_index(plane, x, -i);
            let bot_dst = plane_pixel_index(plane, x, plane.height - 1 + i);
            plane.data[top_dst] = plane.data[top_src];
            plane.data[bot_dst] = plane.data[bot_src];
        }
    }
}

pub fn oc_state_borders_fill(state: &mut TheoraState, refi: usize) {
    for pli in 0..3usize {
        let height = state.ref_frame_bufs[refi][pli].height;
        oc_state_borders_fill_rows(state, refi, pli, 0, height);
        oc_state_borders_fill_caps(state, refi, pli);
    }
}

pub fn oc_state_get_mv_offsets(
    state: &TheoraState,
    offsets: &mut [i32; 2],
    pli: usize,
    mv: OcMv,
) -> usize {
    let ystride = state.ref_ystride[pli];
    let qpy = (pli != 0 && (state.info.pixel_fmt as i32 & 2) == 0) as usize;
    let dx = oc_mv_x(mv);
    let dy = oc_mv_y(mv);
    let my = OC_MVMAP[qpy][(dy + 31) as usize] as i32;
    let my2 = OC_MVMAP2[qpy][(dy + 31) as usize] as i32;
    let qpx = (pli != 0 && (state.info.pixel_fmt as i32 & 1) == 0) as usize;
    let mx = OC_MVMAP[qpx][(dx + 31) as usize] as i32;
    let mx2 = OC_MVMAP2[qpx][(dx + 31) as usize] as i32;
    let offs = my * ystride + mx;
    offsets[0] = offs;
    if mx2 != 0 || my2 != 0 {
        offsets[1] = offs + my2 * ystride + mx2;
        2
    } else {
        1
    }
}

fn clamp255(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

fn loop_filter_h(data: &mut [u8], base: isize, ystride: isize, bv: &[i8; 256]) {
    let mut pix = base - 2;
    for _ in 0..8 {
        let b = pix as usize;
        let f = data[b] as i32 - data[b + 3] as i32 + 3 * (data[b + 2] as i32 - data[b + 1] as i32);
        let filt = bv[(((f + 4) >> 3) + 127) as usize] as i32;
        data[b + 1] = clamp255(data[b + 1] as i32 + filt);
        data[b + 2] = clamp255(data[b + 2] as i32 - filt);
        pix += ystride;
    }
}

fn loop_filter_v(data: &mut [u8], base: isize, ystride: isize, bv: &[i8; 256]) {
    let pix = base - ystride * 2;
    for x in 0..8isize {
        let p0 = (pix + x) as usize;
        let p1 = (pix + ystride + x) as usize;
        let p2 = (pix + ystride * 2 + x) as usize;
        let p3 = (pix + ystride * 3 + x) as usize;
        let f = data[p0] as i32 - data[p3] as i32 + 3 * (data[p2] as i32 - data[p1] as i32);
        let filt = bv[(((f + 4) >> 3) + 127) as usize] as i32;
        data[p1] = clamp255(data[p1] as i32 + filt);
        data[p2] = clamp255(data[p2] as i32 - filt);
    }
}

pub fn oc_loop_filter_init_c(bv: &mut [i8; 256], flimit: i32) {
    *bv = [0; 256];
    for i in 0..flimit {
        if 127 - i - flimit >= 0 {
            bv[(127 - i - flimit) as usize] = (i - flimit) as i8;
        }
        bv[(127 - i) as usize] = (-i) as i8;
        bv[(127 + i) as usize] = i as i8;
        if 127 + i + flimit < 256 {
            bv[(127 + i + flimit) as usize] = (flimit - i) as i8;
        }
    }
}

pub fn oc_state_loop_filter_frag_rows_c(
    state: &mut TheoraState,
    bvarray: &[i8; 256],
    refi: usize,
    pli: usize,
    fragy0: i32,
    fragy_end: i32,
) {
    let fplane = state.fplanes[pli];
    let nhfrags = fplane.nhfrags as usize;
    let fragi_top = fplane.froffset as usize;
    let fragi_bot = (fplane.froffset + fplane.nfrags) as usize;
    let mut fragi0 = fragi_top + fragy0 as usize * nhfrags;
    let fragi0_end = fragi_top + fragy_end as usize * nhfrags;
    let ystride = state.ref_ystride[pli] as isize;
    let frags = state.frags.clone();
    let frag_buf_offs = state.frag_buf_offs.clone();
    let plane = &mut state.ref_frame_bufs[refi][pli];
    let plane_base = plane.data_offset as isize;
    while fragi0 < fragi0_end {
        let mut fragi = fragi0;
        let fragi_end = fragi + nhfrags;
        while fragi < fragi_end {
            if frags[fragi].coded {
                let off = plane_base + frag_buf_offs[fragi];
                if fragi > fragi0 {
                    loop_filter_h(&mut plane.data, off, ystride, bvarray);
                }
                if fragi0 > fragi_top {
                    loop_filter_v(&mut plane.data, off, ystride, bvarray);
                }
                if fragi + 1 < fragi_end && !frags[fragi + 1].coded {
                    loop_filter_h(&mut plane.data, off + 8, ystride, bvarray);
                }
                if fragi + nhfrags < fragi_bot && !frags[fragi + nhfrags].coded {
                    loop_filter_v(&mut plane.data, off + ystride * 8, ystride, bvarray);
                }
            }
            fragi += 1;
        }
        fragi0 += nhfrags;
    }
}

pub fn th_granule_frame_from_state(state: &TheoraState, granpos: i64) -> i64 {
    if granpos < 0 {
        return -1;
    }
    let iframe = granpos >> state.info.keyframe_granule_shift;
    let pframe = granpos - (iframe << state.info.keyframe_granule_shift);
    iframe + pframe
        - ((
            state.info.version_major as i32,
            state.info.version_minor as i32,
            state.info.version_subminor as i32,
        ) >= (3, 2, 1)) as i64
}

pub fn th_granule_time_from_state(state: &TheoraState, granpos: i64) -> f64 {
    if granpos < 0 || state.info.fps_numerator == 0 {
        return -1.0;
    }
    (th_granule_frame_from_state(state, granpos) + 1) as f64 * state.info.fps_denominator as f64
        / state.info.fps_numerator as f64
}

pub fn oc_set_chroma_mvs00(cbmvs: &mut [OcMv; 4], lbmvs: &[OcMv; 4]) {
    oc_state_set_chroma_mvs(cbmvs, lbmvs, 0);
}
pub fn oc_set_chroma_mvs01(cbmvs: &mut [OcMv; 4], lbmvs: &[OcMv; 4]) {
    oc_state_set_chroma_mvs(cbmvs, lbmvs, 1);
}
pub fn oc_set_chroma_mvs10(cbmvs: &mut [OcMv; 4], lbmvs: &[OcMv; 4]) {
    oc_state_set_chroma_mvs(cbmvs, lbmvs, 2);
}
pub fn oc_set_chroma_mvs11(cbmvs: &mut [OcMv; 4], lbmvs: &[OcMv; 4]) {
    *cbmvs = *lbmvs;
}

pub fn oc_sb_quad_top_left_frag(sb_map: &OcSbMap, quadi: usize) -> isize {
    sb_quad_top_left_frag(sb_map, quadi)
}

pub fn oc_state_frarray_clear(state: &mut TheoraState) {
    state.coded_fragis.clear();
    state.ncoded_fragis = [0; 3];
    state.ntotal_coded_fragis = 0;
    for frag in &mut state.frags {
        frag.coded = false;
    }
}

pub fn oc_state_ref_bufs_clear(state: &mut TheoraState) {
    state.frag_buf_offs.clear();
    for idx in &mut state.ref_frame_idx {
        *idx = -1;
    }
    for frame in &mut state.ref_frame_bufs {
        for plane in frame {
            plane.data.clear();
            plane.data_offset = 0;
            plane.width = 0;
            plane.height = 0;
            plane.stride = 0;
        }
    }
}

pub fn oc_state_accel_init_c(_state: &mut TheoraState) {}

pub fn oc_state_frag_recon_c(
    state: &mut TheoraState,
    fragi: usize,
    pli: usize,
    dct_coeffs: &mut [i16; 128],
    last_zzi: i32,
    dc_quant: u16,
) {
    if last_zzi < 2 {
        let p = ((i32::from(dct_coeffs[0]) * i32::from(dc_quant) + 15) >> 5) as i16;
        for ci in 0..64usize {
            dct_coeffs[64 + ci] = p;
        }
    } else {
        dct_coeffs[0] = (i32::from(dct_coeffs[0]) * i32::from(dc_quant)) as i16;
        let mut residue = [0i16; 64];
        let mut input = [0i16; 64];
        input.copy_from_slice(&dct_coeffs[..64]);
        crate::idct::idct8x8_c(&mut residue, &mut input, last_zzi);
        dct_coeffs[64..128].copy_from_slice(&residue);
    }
    let frag_buf_off = state.frag_buf_offs[fragi];
    let refi = state.frags[fragi].refi as i32;
    let ystride = state.ref_ystride[pli] as isize;
    let selfi = state.ref_frame_idx[OC_FRAME_SELF as usize];
    assert!(
        selfi >= 0,
        "OC_FRAME_SELF is not initialized before fragment reconstruction"
    );
    let selfi = selfi as usize;
    let dst_base = state.ref_frame_bufs[selfi][pli].data_offset as isize;
    let dst_off = dst_base + frag_buf_off;
    let recon_coeffs: &[i16; 64] = (&dct_coeffs[64..128]).try_into().unwrap();
    if refi == OC_FRAME_SELF {
        let dst = &mut state.ref_frame_bufs[selfi][pli].data;
        crate::fragment::oc_frag_recon_intra_c(dst, dst_off, ystride, recon_coeffs);
    } else {
        assert!(
            refi >= 0,
            "negative reference frame index during fragment reconstruction"
        );
        let refi = refi as usize;
        let src_slot = state.ref_frame_idx[refi];
        assert!(
            src_slot >= 0,
            "reference frame slot is not initialized during fragment reconstruction"
        );
        let src_slot = src_slot as usize;
        let src_base = state.ref_frame_bufs[src_slot][pli].data_offset as isize;
        let mv = state.frag_mvs[fragi];
        let mut mvoffsets = [0i32; 2];
        let nsrc = oc_state_get_mv_offsets(state, &mut mvoffsets, pli, mv);
        let s0 = src_base + frag_buf_off + mvoffsets[0] as isize;
        if src_slot < selfi {
            let (left, right) = state.ref_frame_bufs.split_at_mut(selfi);
            let src = &left[src_slot][pli].data;
            let dst = &mut right[0][pli].data;
            if nsrc > 1 {
                let s1 = src_base + frag_buf_off + mvoffsets[1] as isize;
                crate::fragment::oc_frag_recon_inter2_c(
                    dst,
                    dst_off,
                    src,
                    s0,
                    src,
                    s1,
                    ystride,
                    recon_coeffs,
                );
            } else {
                crate::fragment::oc_frag_recon_inter_c(
                    dst,
                    dst_off,
                    src,
                    s0,
                    ystride,
                    recon_coeffs,
                );
            }
        } else if src_slot > selfi {
            let (left, right) = state.ref_frame_bufs.split_at_mut(src_slot);
            let dst = &mut left[selfi][pli].data;
            let src = &right[0][pli].data;
            if nsrc > 1 {
                let s1 = src_base + frag_buf_off + mvoffsets[1] as isize;
                crate::fragment::oc_frag_recon_inter2_c(
                    dst,
                    dst_off,
                    src,
                    s0,
                    src,
                    s1,
                    ystride,
                    recon_coeffs,
                );
            } else {
                crate::fragment::oc_frag_recon_inter_c(
                    dst,
                    dst_off,
                    src,
                    s0,
                    ystride,
                    recon_coeffs,
                );
            }
        } else {
            let src = state.ref_frame_bufs[src_slot][pli].data.clone();
            if nsrc > 1 {
                let s1 = src_base + frag_buf_off + mvoffsets[1] as isize;
                let dst = &mut state.ref_frame_bufs[selfi][pli].data;
                crate::fragment::oc_frag_recon_inter2_c(
                    dst,
                    dst_off,
                    &src,
                    s0,
                    &src,
                    s1,
                    ystride,
                    recon_coeffs,
                );
            } else {
                let dst = &mut state.ref_frame_bufs[selfi][pli].data;
                crate::fragment::oc_frag_recon_inter_c(
                    dst,
                    dst_off,
                    &src,
                    s0,
                    ystride,
                    recon_coeffs,
                );
            }
        }
    }
}

pub fn oc_state_dump_frame(state: &TheoraState) -> String {
    format!(
        "TheoraState(frame_type={}, curframe={}, keyframe={}, granpos={}, nfrags={}, nsbs={}, nmbs={})",
        state.frame_type, state.curframe_num, state.keyframe_num, state.granpos, state.nfrags, state.nsbs, state.nmbs
    )
}
