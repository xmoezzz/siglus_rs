pub mod analyze;
pub mod api;
pub mod apiwrapper;
pub mod bitpack;
pub mod codec;
pub mod collect;
pub mod dct;
pub mod decapiwrapper;
pub mod decinfo;
pub mod decint;
pub mod decode;
pub mod decoder;
pub mod dequant;
pub mod encapiwrapper;
pub mod encfrag;
pub mod encinfo;
pub mod encint;
pub mod encode;
pub mod encoder;
pub mod enquant;
pub mod error;
pub mod fdct;
pub mod fragment;
pub mod huffdec;
pub mod huffenc;
pub mod huffman;
pub mod idct;
pub mod info;
pub mod internal;
pub mod legacy;
pub mod mathops;
pub mod mcenc;
pub mod packet;
pub mod public_api;
pub mod quant;
pub mod rate;
pub mod state;
pub mod tokenize;
pub mod vp31;

pub use api::{
    oc_theora_info2th_info, th_packet_isheader, th_packet_iskeyframe, th_version_number,
    th_version_string, Packet,
};
pub use apiwrapper::{ApiInfo, ApiWrapper, CompatState, YuvBuffer};
pub use codec::{
    Colorspace, Comment, HuffCode, ImgPlane, Info, LegacyTheoraInfo, PixelFmt, QuantBase,
    QuantInfo, QuantRanges, YCbCrBuffer, OC_VENDOR_STRING, TH_NDCT_TOKENS, TH_NHUFFMAN_TABLES,
    TH_PF_NFORMATS, TH_VERSION_MAJOR, TH_VERSION_MINOR, TH_VERSION_SUB,
};
pub use decinfo::{th_decode_headerin, th_setup_free, SetupInfo};
pub use decoder::{
    th_decode_alloc, th_decode_ctl, th_decode_free, th_decode_packetin, th_decode_ycbcr_out,
    th_granule_frame, th_granule_time, DecoderContext, TH_DECCTL_GET_PPLEVEL_MAX,
    TH_DECCTL_SET_BITS as TH_DECCTL_SET_TELEMETRY_BITS, TH_DECCTL_SET_GRANPOS,
    TH_DECCTL_SET_MBMODE as TH_DECCTL_SET_TELEMETRY_MBMODE,
    TH_DECCTL_SET_MV as TH_DECCTL_SET_TELEMETRY_MV, TH_DECCTL_SET_PPLEVEL,
    TH_DECCTL_SET_QI as TH_DECCTL_SET_TELEMETRY_QI, TH_DECCTL_SET_STRIPE_CB,
};
pub use encoder::{
    th_encode_alloc, th_encode_ctl, th_encode_flushheader, th_encode_free, th_encode_packetout,
    th_encode_ycbcr_in, EncoderContext, TH_ENCCTL_GET_SPLEVEL, TH_ENCCTL_GET_SPLEVEL_MAX,
    TH_ENCCTL_SET_DUP_COUNT, TH_ENCCTL_SET_HUFFMAN_CODES, TH_ENCCTL_SET_KEYFRAME_FREQUENCY_FORCE,
    TH_ENCCTL_SET_QUANT_PARAMS, TH_ENCCTL_SET_RATE_BUFFER, TH_ENCCTL_SET_RATE_FLAGS,
    TH_ENCCTL_SET_SPLEVEL, TH_ENCCTL_SET_VP3_COMPATIBLE,
};
pub use error::{Result, TheoraError};
pub use info::{
    th_comment_add, th_comment_add_tag, th_comment_clear, th_comment_init, th_comment_query,
    th_comment_query_count, th_info_clear, th_info_init,
};
pub use legacy::{
    legacy_info_roundtrip_defaults, theora_clear, theora_comment_add, theora_comment_add_tag,
    theora_comment_clear, theora_comment_init, theora_comment_query, theora_comment_query_count,
    theora_control, theora_decode_header, theora_decode_init, theora_decode_packetin,
    theora_decode_yuv_out, theora_encode_comment, theora_encode_header, theora_encode_init,
    theora_encode_packetout, theora_encode_tables, theora_encode_yuv_in, theora_granule_frame,
    theora_granule_shift, theora_granule_time, theora_info_clear, theora_info_init,
    theora_info_to_th_info, theora_packet_isheader, theora_packet_iskeyframe,
    theora_version_number, theora_version_string, LegacyState,
};
pub use packet::{OggPacket, PackWriter};
pub use public_api::{
    decoder_from_headers, encoder_from_info, HeaderParser, HeaderStatus, ThComment, ThDecCtx,
    ThEncCtx, ThInfo, ThSetupInfo, TheoraComment, TheoraInfo, TheoraState, TheoraYuvBuffer,
};

pub mod prelude {
    pub use crate::{
        decoder_from_headers, encoder_from_info, th_comment_add, th_comment_add_tag,
        th_comment_clear, th_comment_init, th_comment_query, th_comment_query_count,
        th_decode_alloc, th_decode_ctl, th_decode_free, th_decode_headerin, th_decode_packetin,
        th_decode_ycbcr_out, th_encode_alloc, th_encode_ctl, th_encode_flushheader, th_encode_free,
        th_encode_packetout, th_encode_ycbcr_in, th_info_clear, th_info_init, th_packet_isheader,
        th_packet_iskeyframe, th_setup_free, th_version_number, th_version_string, Comment,
        DecoderContext, EncoderContext, HeaderParser, HeaderStatus, Info, LegacyState, OggPacket,
        Result, SetupInfo, TheoraError, YCbCrBuffer,
    };
}
