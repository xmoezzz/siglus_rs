use crate::codec::{HuffCode, Info, QuantInfo, YCbCrBuffer, TH_NDCT_TOKENS, TH_NHUFFMAN_TABLES};
use crate::decinfo::SetupInfo;
use crate::state::TheoraState;

#[derive(Debug, Clone)]
pub struct DecPipelineState {
    pub dct_coeffs: [i16; 128],
    pub bounding_values: [i8; 256],
    pub ti: [[isize; 64]; 3],
    pub ebi: [[isize; 64]; 3],
    pub eob_runs: [[isize; 64]; 3],
    pub coded_fragis: [Vec<isize>; 3],
    pub uncoded_fragis: [Vec<isize>; 3],
    pub coded_fragis_off: [usize; 3],
    pub uncoded_fragis_off: [usize; 3],
    pub ncoded_fragis: [isize; 3],
    pub nuncoded_fragis: [isize; 3],
    pub dequant: [[[[u16; 64]; 2]; 3]; 3],
    pub fragy0: [i32; 3],
    pub fragy_end: [i32; 3],
    pub pred_last: [[i32; 4]; 3],
    pub mcu_nvfrags: i32,
    pub loop_filter: i32,
    pub pp_level: i32,
}

#[derive(Debug, Clone)]
pub struct DecContext {
    pub state: TheoraState,
    pub info: Info,
    pub packet_state: i32,
    pub setup: Option<SetupInfo>,
    pub huff_codes: [[HuffCode; TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES],
    pub ti0: [[isize; 64]; 3],
    pub eob_runs: [[isize; 64]; 3],
    pub dct_tokens: Vec<u8>,
    pub extra_bits: Vec<u8>,
    pub dct_tokens_count: i32,
    pub pp_level: i32,
    pub pp_dc_scale: [i32; 64],
    pub pp_sharp_mod: [i32; 64],
    pub dc_qis: Vec<u8>,
    pub variances: Vec<i32>,
    pub pp_frame_data: Vec<u8>,
    pub pp_frame_state: i32,
    pub pp_frame_buf: YCbCrBuffer,
    pub pipe: DecPipelineState,
    pub granulepos: i64,
    pub qinfo: QuantInfo,
}

impl Default for DecPipelineState {
    fn default() -> Self {
        Self {
            dct_coeffs: [0; 128],
            bounding_values: [0; 256],
            ti: [[0; 64]; 3],
            ebi: [[0; 64]; 3],
            eob_runs: [[0; 64]; 3],
            coded_fragis: Default::default(),
            uncoded_fragis: Default::default(),
            coded_fragis_off: [0; 3],
            uncoded_fragis_off: [0; 3],
            ncoded_fragis: [0; 3],
            nuncoded_fragis: [0; 3],
            dequant: [[[[0; 64]; 2]; 3]; 3],
            fragy0: [0; 3],
            fragy_end: [0; 3],
            pred_last: [[0; 4]; 3],
            mcu_nvfrags: 0,
            loop_filter: 0,
            pp_level: 0,
        }
    }
}

impl Default for DecContext {
    fn default() -> Self {
        Self {
            state: Default::default(),
            info: Default::default(),
            packet_state: 0,
            setup: None,
            huff_codes: [[HuffCode::default(); TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES],
            ti0: [[0; 64]; 3],
            eob_runs: [[0; 64]; 3],
            dct_tokens: Vec::new(),
            extra_bits: Vec::new(),
            dct_tokens_count: 0,
            pp_level: 0,
            pp_dc_scale: [0; 64],
            pp_sharp_mod: [0; 64],
            dc_qis: Vec::new(),
            variances: Vec::new(),
            pp_frame_data: Vec::new(),
            pp_frame_state: 0,
            pp_frame_buf: [Default::default(), Default::default(), Default::default()],
            pipe: Default::default(),
            granulepos: 0,
            qinfo: Default::default(),
        }
    }
}
