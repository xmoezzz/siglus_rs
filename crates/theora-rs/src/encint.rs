use crate::analyze::{FrState, ModeSchemeChooser, QiiState};
use crate::codec::{Comment, HuffCode, Info, QuantInfo, TH_NDCT_TOKENS, TH_NHUFFMAN_TABLES};
use crate::mcenc::MbEncInfo;
use crate::packet::OggPacket;
use crate::rate::{FrameMetrics, IirFilter, RcState};
use crate::state::TheoraState;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ModeRd {
    pub rate: i16,
    pub rmse: i16,
}

#[derive(Debug, Clone)]
pub struct EncPipelineState {
    pub dct_data: [i16; 64 * 3],
    pub bounding_values: [i8; 256],
    pub fr: [FrState; 3],
    pub qs: [QiiState; 3],
    pub skip_ssd: [Vec<u32>; 3],
    pub coded_fragis: [Vec<isize>; 3],
    pub uncoded_fragis: [Vec<isize>; 3],
    pub ncoded_fragis: [isize; 3],
    pub nuncoded_fragis: [isize; 3],
    pub froffset: [isize; 3],
    pub fragy0: [i32; 3],
    pub fragy_end: [i32; 3],
    pub sbi0: [u32; 3],
    pub sbi_end: [u32; 3],
    pub ndct_tokens1: [i32; 3],
    pub eob_run1: [i32; 3],
    pub loop_filter: i32,
}

#[derive(Debug, Clone)]
pub struct EncContext {
    pub state: TheoraState,
    pub info: Info,
    pub comment: Comment,
    pub packet_state: i32,
    pub op: Option<OggPacket>,
    pub mb_info: Vec<MbEncInfo>,
    pub frag_dc: Vec<i16>,
    pub coded_mbis: Vec<u32>,
    pub ncoded_mbis: usize,
    pub keyframe_frequency_force: u32,
    pub dup_count: u32,
    pub nqueued_dups: u32,
    pub prev_dup_count: u32,
    pub sp_level: i32,
    pub vp3_compatible: bool,
    pub coded_inter_frame: bool,
    pub prevframe_dropped: bool,
    pub huff_idxs: [[[u8; 2]; 2]; 2],
    pub mv_bits: [usize; 2],
    pub chooser: ModeSchemeChooser,
    pub pipe: EncPipelineState,
    pub mcu_nvsbs: i32,
    pub mcu_skip_ssd: Vec<u32>,
    pub dct_tokens: [Vec<Vec<u8>>; 3],
    pub extra_bits: [Vec<Vec<u16>>; 3],
    pub ndct_tokens: [[isize; 64]; 3],
    pub eob_run: [[u16; 64]; 3],
    pub dct_token_offs: [[u8; 64]; 3],
    pub dc_pred_last: [[i32; 4]; 3],
    pub lambda: i32,
    pub activity_avg: u32,
    pub luma_avg: u32,
    pub huff_codes: [[HuffCode; TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES],
    pub qinfo: QuantInfo,
    pub dequant_dc: [[[u16; 2]; 3]; 64],
    pub log_qavg: [[i64; 64]; 2],
    pub log_plq: [[[i16; 2]; 3]; 64],
    pub chroma_rd_scale: [[[u16; 2]; 64]; 2],
    pub mode_rd: [[[[ModeRd; 24]; 2]; 3]; 3],
    pub rc: RcState,
    pub scalefilter: [IirFilter; 2],
    pub prev_metrics: FrameMetrics,
}

impl Default for EncPipelineState {
    fn default() -> Self {
        Self {
            dct_data: [0; 64 * 3],
            bounding_values: [0; 256],
            fr: Default::default(),
            qs: Default::default(),
            skip_ssd: Default::default(),
            coded_fragis: Default::default(),
            uncoded_fragis: Default::default(),
            ncoded_fragis: [0; 3],
            nuncoded_fragis: [0; 3],
            froffset: [0; 3],
            fragy0: [0; 3],
            fragy_end: [0; 3],
            sbi0: [0; 3],
            sbi_end: [0; 3],
            ndct_tokens1: [0; 3],
            eob_run1: [0; 3],
            loop_filter: 0,
        }
    }
}

impl Default for EncContext {
    fn default() -> Self {
        Self {
            state: Default::default(),
            info: Default::default(),
            comment: Default::default(),
            packet_state: 0,
            op: None,
            mb_info: Vec::new(),
            frag_dc: Vec::new(),
            coded_mbis: Vec::new(),
            ncoded_mbis: 0,
            keyframe_frequency_force: 0,
            dup_count: 0,
            nqueued_dups: 0,
            prev_dup_count: 0,
            sp_level: 0,
            vp3_compatible: false,
            coded_inter_frame: false,
            prevframe_dropped: false,
            huff_idxs: [[[0; 2]; 2]; 2],
            mv_bits: [0; 2],
            chooser: Default::default(),
            pipe: Default::default(),
            mcu_nvsbs: 0,
            mcu_skip_ssd: Vec::new(),
            dct_tokens: Default::default(),
            extra_bits: Default::default(),
            ndct_tokens: [[0; 64]; 3],
            eob_run: [[0; 64]; 3],
            dct_token_offs: [[0; 64]; 3],
            dc_pred_last: [[0; 4]; 3],
            lambda: 0,
            activity_avg: 0,
            luma_avg: 0,
            huff_codes: [[HuffCode::default(); TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES],
            qinfo: Default::default(),
            dequant_dc: [[[0; 2]; 3]; 64],
            log_qavg: [[0; 64]; 2],
            log_plq: [[[0; 2]; 3]; 64],
            chroma_rd_scale: [[[0; 2]; 64]; 2],
            mode_rd: [[[[ModeRd::default(); 24]; 2]; 3]; 3],
            rc: Default::default(),
            scalefilter: [Default::default(), Default::default()],
            prev_metrics: Default::default(),
        }
    }
}
