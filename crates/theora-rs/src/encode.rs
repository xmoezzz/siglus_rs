use crate::bitpack::PackBuf;
use crate::codec::{
    HuffCode, ImgPlane, QuantInfo, YCbCrBuffer, TH_NDCT_TOKENS, TH_NHUFFMAN_TABLES,
};
use crate::encint::EncContext;
use crate::encoder::EncoderContext;
use crate::error::{Result, TheoraError};
use crate::packet::{OggPacket, PackWriter};
use crate::state::{TheoraState, OC_INTER_FRAME, OC_INTRA_FRAME};
use crate::tokenize::{oc_decode_eob_token, oc_make_eob_token_full, oc_token_bits_zzi, TokenLog};

pub fn oc_sb_run_pack(opb: &mut PackWriter, run_count: i32) {
    match run_count {
        1 => opb.write(0b0, 1),
        2..=3 => {
            opb.write(0b100, 3);
            opb.write((run_count - 2) as u32, 1);
        }
        4..=5 => {
            opb.write(0b1100, 4);
            opb.write((run_count - 4) as u32, 1);
        }
        6..=9 => {
            opb.write(0b111000, 6);
            opb.write((run_count - 6) as u32, 2);
        }
        10..=17 => {
            opb.write(0b11110000, 8);
            opb.write((run_count - 10) as u32, 3);
        }
        18..=33 => {
            opb.write(0b1111100000, 10);
            opb.write((run_count - 18) as u32, 4);
        }
        _ => {
            opb.write(0b111111000000000000, 18);
            opb.write((run_count - 34) as u32, 12);
        }
    }
}

pub fn oc_block_run_pack(opb: &mut PackWriter, run_count: i32) {
    if !(1..=30).contains(&run_count) {
        return;
    }
    const NBITS: [u8; 30] = [
        2, 2, 3, 3, 4, 4, 6, 6, 6, 6, 7, 7, 7, 7, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9, 9,
    ];
    const PAT: [u16; 30] = [
        0x000, 0x001, 0x004, 0x005, 0x00C, 0x00D, 0x038, 0x039, 0x03A, 0x03B, 0x078, 0x079, 0x07A,
        0x07B, 0x1F0, 0x1F1, 0x1F2, 0x1F3, 0x1F4, 0x1F5, 0x1F6, 0x1F7, 0x1F8, 0x1F9, 0x1FA, 0x1FB,
        0x1FC, 0x1FD, 0x1FE, 0x1FF,
    ];
    let i = (run_count - 1) as usize;
    opb.write(PAT[i] as u32, NBITS[i] as usize);
}

pub fn oc_enc_frame_header_pack(opb: &mut PackWriter, frame_type: i8, qis: &[u8]) {
    opb.write(0, 1);
    opb.write((frame_type != 0) as u32, 1);
    let nqis = qis.len().clamp(1, 3);
    opb.write(qis[0] as u32, 6);
    if nqis > 1 {
        opb.write(1, 1);
        opb.write(qis[1] as u32, 6);
        if nqis > 2 {
            opb.write(1, 1);
            opb.write(qis[2] as u32, 6);
        } else {
            opb.write(0, 1);
        }
    } else {
        opb.write(0, 1);
    }
    if frame_type == OC_INTRA_FRAME {
        opb.write(0, 3);
    }
}

pub fn oc_enc_partial_sb_flags_pack(opb: &mut PackWriter, flags: &[bool]) {
    if flags.is_empty() {
        return;
    }
    let mut cur = flags[0];
    let mut run = 0i32;
    opb.write(cur as u32, 1);
    for &f in flags {
        if f == cur && run < 4129 {
            run += 1;
        } else {
            oc_sb_run_pack(opb, run);
            cur = f;
            run = 1;
        }
    }
    if run > 0 {
        oc_sb_run_pack(opb, run);
    }
}

pub fn oc_enc_coded_sb_flags_pack(opb: &mut PackWriter, flags: &[bool]) {
    oc_enc_partial_sb_flags_pack(opb, flags)
}

pub fn oc_enc_coded_flags_pack(opb: &mut PackWriter, flags: &[bool]) {
    if flags.is_empty() {
        return;
    }
    let mut cur = flags[0];
    let mut run = 0i32;
    opb.write(cur as u32, 1);
    for &f in flags {
        if f == cur && run < 30 {
            run += 1;
        } else {
            oc_block_run_pack(opb, run.max(1));
            cur = f;
            run = 1;
        }
    }
    if run > 0 {
        oc_block_run_pack(opb, run.max(1));
    }
}

pub fn oc_enc_mb_modes_pack(opb: &mut PackWriter, modes: &[u8]) {
    for &mode in modes {
        opb.write(mode as u32 & 7, 3);
    }
}

pub fn oc_enc_mv_pack(opb: &mut PackWriter, mv: i16) {
    let x = (mv as i8) as i32 + 32;
    let y = ((mv as i32) >> 8) + 32;
    opb.write(x as u32 & 0x3F, 6);
    opb.write(y as u32 & 0x3F, 6);
}

pub fn oc_enc_mvs_pack(opb: &mut PackWriter, mvs: &[i16]) {
    for &mv in mvs {
        oc_enc_mv_pack(opb, mv);
    }
}

pub fn oc_enc_block_qis_pack(opb: &mut PackWriter, qis: &[u8], nqis: u8) {
    let nbits = match nqis {
        0 | 1 => 0,
        2 => 1,
        _ => 2,
    };
    if nbits == 0 {
        return;
    }
    for &qi in qis {
        opb.write(qi as u32, nbits);
    }
}

pub fn oc_enc_count_tokens(log: &TokenLog<'_>) -> usize {
    let mut n = 0usize;
    for pli in 0..3 {
        for zzi in 0..64 {
            n += log.dct_tokens[pli][zzi].len();
        }
    }
    n
}

pub fn oc_enc_count_bits(
    log: &TokenLog<'_>,
    codes: &[[HuffCode; TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES],
) -> i64 {
    let mut bits = 0i64;
    for pli in 0..3 {
        for zzi in 0..64 {
            let huffi = log.dct_token_offs[pli][zzi] as usize;
            for &token in &log.dct_tokens[pli][zzi] {
                bits += i64::from(oc_token_bits_zzi(codes, huffi, zzi, token as usize));
            }
        }
    }
    bits
}

pub fn oc_select_huff_idx(
    counts: &[u32; TH_NDCT_TOKENS],
    codes: &[[HuffCode; TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES],
) -> usize {
    let mut best = 0usize;
    let mut best_cost = i64::MAX;
    for (huffi, table) in codes.iter().enumerate() {
        let mut cost = 0i64;
        for ti in 0..TH_NDCT_TOKENS {
            cost += i64::from(table[ti].nbits) * counts[ti] as i64;
        }
        if cost < best_cost {
            best_cost = cost;
            best = huffi;
        }
    }
    best
}

pub fn oc_enc_huff_group_pack(
    opb: &mut PackWriter,
    log: &TokenLog<'_>,
    pli: usize,
    zzi0: usize,
    zzi1: usize,
) {
    for zzi in zzi0..zzi1.min(64) {
        for (i, &token) in log.dct_tokens[pli][zzi].iter().enumerate() {
            let huffi =
                log.dct_token_offs[pli][zzi] as usize + crate::tokenize::OC_ZZI_HUFF_OFFSET[zzi];
            let code = log.huff_codes[huffi][token as usize];
            opb.write(code.pattern, code.nbits as usize);
            let eb = *log.extra_bits[pli][zzi].get(i).unwrap_or(&0) as i32;
            let ebits = crate::huffman::OC_DCT_TOKEN_EXTRA_BITS[token as usize] as usize;
            if ebits > 0 {
                opb.write(eb as u32, ebits);
            }
        }
    }
}

pub fn oc_enc_residual_tokens_pack(opb: &mut PackWriter, log: &TokenLog<'_>) {
    for pli in 0..3 {
        for group in 0..9 {
            let zzi0 = if group == 0 { 0 } else { group * 8 };
            let zzi1 = if group == 0 { 1 } else { (group + 1) * 8 };
            oc_enc_huff_group_pack(opb, log, pli, zzi0, zzi1);
        }
    }
}

pub fn oc_enc_drop_frame_pack(state: &TheoraState, granulepos: i64) -> OggPacket {
    let mut w = PackWriter::new();
    oc_enc_frame_header_pack(&mut w, OC_INTER_FRAME, &[state.qis[0]]);
    let mut pkt = OggPacket::new(w.finish());
    pkt.granulepos = granulepos;
    pkt
}

pub fn oc_enc_frame_pack(
    state: &TheoraState,
    coded_flags: &[bool],
    modes: &[u8],
    mvs: &[i16],
    block_qis: &[u8],
    residuals: Option<&TokenLog<'_>>,
    granulepos: i64,
) -> OggPacket {
    let mut w = PackWriter::new();
    let nqis = state.nqis.max(1) as usize;
    oc_enc_frame_header_pack(&mut w, state.frame_type, &state.qis[..nqis]);
    oc_enc_coded_flags_pack(&mut w, coded_flags);
    if state.frame_type != OC_INTRA_FRAME {
        oc_enc_mb_modes_pack(&mut w, modes);
        oc_enc_mvs_pack(&mut w, mvs);
        oc_enc_block_qis_pack(&mut w, block_qis, state.nqis);
    }
    if let Some(log) = residuals {
        oc_enc_residual_tokens_pack(&mut w, log);
    }
    let mut pkt = OggPacket::new(w.finish());
    pkt.granulepos = granulepos;
    pkt
}

pub fn oc_enc_accel_init_c(_enc: &mut EncContext) {}

pub fn oc_enc_mb_info_init(enc: &mut EncContext) {
    if enc.state.nmbs > enc.mb_info.len() {
        enc.mb_info.resize(enc.state.nmbs, Default::default());
    }
}

pub fn oc_enc_set_huffman_codes(
    enc: &mut EncContext,
    codes: [[HuffCode; TH_NDCT_TOKENS]; TH_NHUFFMAN_TABLES],
) {
    enc.huff_codes = codes;
}

pub fn oc_enc_enquant_tables_init(_enc: &mut EncContext) {}

pub fn oc_enc_quant_params_updated(enc: &mut EncContext) {
    enc.state.loop_filter_limits = enc.qinfo.loop_filter_limits;
}

pub fn oc_enc_set_quant_params(enc: &mut EncContext, qinfo: QuantInfo) {
    enc.qinfo = qinfo;
    oc_enc_quant_params_updated(enc);
}

pub fn oc_enc_clear(enc: &mut EncContext) {
    *enc = EncContext::default();
}

pub fn oc_enc_drop_frame(enc: &mut EncContext) -> OggPacket {
    enc.state.curframe_num += 1;
    enc.state.frame_type = OC_INTER_FRAME;
    oc_enc_drop_frame_pack(&enc.state, enc.state.granpos)
}

pub fn oc_enc_compress_keyframe(enc: &mut EncContext, log: Option<&TokenLog<'_>>) -> OggPacket {
    enc.state.frame_type = OC_INTRA_FRAME;
    enc.state.curframe_num += 1;
    oc_enc_frame_pack(&enc.state, &[], &[], &[], &[], log, enc.state.granpos)
}

pub fn oc_enc_compress_frame(
    enc: &mut EncContext,
    coded_flags: &[bool],
    modes: &[u8],
    mvs: &[i16],
    block_qis: &[u8],
    log: Option<&TokenLog<'_>>,
) -> OggPacket {
    enc.state.frame_type = OC_INTER_FRAME;
    enc.state.curframe_num += 1;
    oc_enc_frame_pack(
        &enc.state,
        coded_flags,
        modes,
        mvs,
        block_qis,
        log,
        enc.state.granpos,
    )
}

pub fn oc_enc_set_granpos(enc: &mut EncContext, dup_count: u32, frame_type: i8) -> i64 {
    if enc.state.curframe_num < 0 {
        enc.state.curframe_num = 0;
    }
    if frame_type == OC_INTRA_FRAME {
        enc.state.keyframe_num = enc.state.curframe_num;
    }
    let shift = enc.info.keyframe_granule_shift.max(0) as i64;
    let iframe = enc.state.keyframe_num << shift;
    let pframe = enc.state.curframe_num - enc.state.keyframe_num + i64::from(dup_count);
    enc.state.granpos = iframe + pframe;
    enc.state.granpos
}

pub fn th_encode_free(enc: &mut Option<EncoderContext>) {
    *enc = None;
}

pub fn th_encode_ctl(enc: &mut EncoderContext, req: i32, buf: &mut [u8]) -> Result<()> {
    enc.ctl(req, buf)
}

pub fn th_encode_flushheader(enc: &mut EncoderContext) -> Result<Option<OggPacket>> {
    if enc.packet_state < crate::encinfo::OC_PACKET_EMPTY {
        enc.packetout(false)
    } else {
        Ok(None)
    }
}

pub fn th_encode_ycbcr_in(enc: &mut EncoderContext, ycbcr: &YCbCrBuffer) -> Result<()> {
    enc.ycbcr_in(ycbcr)
}

pub fn th_encode_packetout(enc: &mut EncoderContext, last: bool) -> Result<Option<OggPacket>> {
    enc.packetout(last)
}

pub fn oc_pack_quant_select(opb: &mut PackWriter, qii: &[u8], nqis: u8) {
    oc_enc_block_qis_pack(opb, qii, nqis)
}

pub fn oc_unpack_quant_select(opb: &mut PackBuf<'_>, nvals: usize, nqis: u8) -> Vec<u8> {
    let nbits = match nqis {
        0 | 1 => 0,
        2 => 1,
        _ => 2,
    };
    let mut out = vec![0u8; nvals];
    if nbits == 0 {
        return out;
    }
    for v in &mut out {
        *v = opb.read(nbits) as u8;
    }
    out
}

pub fn oc_img_plane_copy_pad(dst: &mut ImgPlane, src: &ImgPlane) {
    dst.width = src.width;
    dst.height = src.height;
    dst.stride = src.stride;
    dst.data_offset = src.data_offset;
    dst.data = src.data.clone();
    if src.width <= 0 || src.height <= 0 || src.stride <= 0 {
        return;
    }
    let stride = src.stride as usize;
    let w = src.width as usize;
    let h = src.height as usize;
    if dst.data.len() < stride * h {
        dst.data.resize(stride * h, 0);
    }
    for y in 0..h {
        let row = y * stride;
        if w < stride && row + w > 0 && row + w - 1 < dst.data.len() {
            let pad = dst.data[row + w - 1];
            for x in w..stride {
                dst.data[row + x] = pad;
            }
        }
    }
}
